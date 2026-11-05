"""Exercise the shipped DirectShow DLL in a graph without installing it.

Loads its class factory directly, connects it to Windows' Null Renderer, and
runs the graph against our shared-memory writer. No registry changes or camera
registration. Refuses to touch the channel if another producer already owns it.
"""
import ctypes as C
from ctypes import wintypes as W
from pathlib import Path
import sys
import threading
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "desktop"))
from PIL import Image
from camerabridge.virtual_camera import VirtualCamera

HRESULT = C.c_long
PTR = C.c_void_p


class GUID(C.Structure):
    _fields_ = [("data", C.c_ubyte * 16)]
    def __init__(self, text):
        super().__init__()
        self.data[:] = uuid.UUID(text).bytes_le


def check(value):
    if value < 0:
        raise OSError(f"COM operation failed: 0x{value & 0xffffffff:08x}")


def call(pointer, slot, args=(), values=(), result=HRESULT):
    table = C.cast(pointer, C.POINTER(C.POINTER(PTR))).contents
    return C.WINFUNCTYPE(result, PTR, *args)(table[slot])(pointer, *values)


def query(pointer, iid):
    target = PTR()
    check(call(pointer, 0, (C.POINTER(GUID), C.POINTER(PTR)), (C.byref(GUID(iid)), C.byref(target))))
    return target


def main():
    kernel = C.WinDLL("kernel32", use_last_error=True)
    kernel.OpenMutexW.argtypes, kernel.OpenMutexW.restype = [W.DWORD, W.BOOL, W.LPCWSTR], W.HANDLE
    existing = kernel.OpenMutexW(0x100000, False, "UnityCapture_Mutx")
    if existing:
        kernel.CloseHandle.argtypes = [W.HANDLE]
        kernel.CloseHandle(existing)
        raise RuntimeError("Another camera producer/consumer owns the channel; close it before this smoke test")
    ole = C.OleDLL("ole32")
    ole.CoInitializeEx.argtypes = [PTR, W.DWORD]
    ole.CoCreateInstance.argtypes = [C.POINTER(GUID), PTR, W.DWORD, C.POINTER(GUID), C.POINTER(PTR)]
    check(ole.CoInitializeEx(None, 0))
    refs = []
    camera, worker = None, None
    stop = threading.Event()
    try:
        dll = C.WinDLL(str(Path(__file__).resolve().parents[1] / "third_party/unitycapture/UnityCaptureFilter64.dll"))
        dll.DllGetClassObject.argtypes = [C.POINTER(GUID), C.POINTER(GUID), C.POINTER(PTR)]
        factory = PTR()
        check(dll.DllGetClassObject(C.byref(GUID("5c2cd55c-92ad-4999-8666-912bd3e70010")),
                                   C.byref(GUID("00000001-0000-0000-c000-000000000046")), C.byref(factory)))
        refs.append(factory)
        source = PTR()
        base_filter = GUID("56a86895-0ad4-11ce-b03a-0020af0ba770")
        check(call(factory, 3, (PTR, C.POINTER(GUID), C.POINTER(PTR)), (None, C.byref(base_filter), C.byref(source))))
        refs.append(source)
        graph = PTR()
        check(ole.CoCreateInstance(C.byref(GUID("e436ebb3-524f-11ce-9f53-0020af0ba770")), None, 1,
                                   C.byref(GUID("56a868a9-0ad4-11ce-b03a-0020af0ba770")), C.byref(graph)))
        refs.append(graph)
        renderer = PTR()
        check(ole.CoCreateInstance(C.byref(GUID("c1f400a4-3f08-11d3-9f0b-006008039e37")), None, 1,
                                   C.byref(base_filter), C.byref(renderer)))
        refs.append(renderer)
        for pointer, name in ((source, "CameraBridge"), (renderer, "Null Renderer")):
            check(call(graph, 3, (PTR, W.LPCWSTR), (pointer, name)))

        def first_pin(filter_pointer):
            enum = PTR()
            check(call(filter_pointer, 10, (C.POINTER(PTR),), (C.byref(enum),)))
            refs.append(enum)
            pin, fetched = PTR(), W.ULONG()
            check(call(enum, 3, (W.ULONG, C.POINTER(PTR), C.POINTER(W.ULONG)), (1, C.byref(pin), C.byref(fetched))))
            if not fetched.value: raise RuntimeError("Filter has no pins")
            refs.append(pin)
            return pin

        check(call(graph, 11, (PTR, PTR), (first_pin(source), first_pin(renderer))))
        control = query(graph, "56a868b1-0ad4-11ce-b03a-0020af0ba770")
        refs.append(control)
        camera = VirtualCamera()
        image = Image.new("RGBA", (1280, 720), (20, 120, 220, 255))
        camera.send(image)

        def publish():
            while not stop.wait(0.05): camera.send(image)

        worker = threading.Thread(target=publish)
        worker.start()
        check(call(control, 7))  # IMediaControl::Run
        time.sleep(1)
        state = C.c_long()
        check(call(control, 10, (C.c_long, C.POINTER(C.c_long)), (3000, C.byref(state))))
        if state.value != 2: raise RuntimeError("DirectShow graph did not enter Running state")
        check(call(control, 9))
        print("Bundled DirectShow filter -> Null Renderer: RUNNING / STOPPED successfully")
    finally:
        stop.set()
        if worker: worker.join(3)
        for pointer in reversed(refs): call(pointer, 2, result=W.ULONG)
        if camera: camera.close()
        ole.CoUninitialize()


if __name__ == "__main__": main()
