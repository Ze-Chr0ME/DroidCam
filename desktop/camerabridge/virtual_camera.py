"""Unity Capture's documented shared-memory ABI, implemented with Win32.

No Unity runtime or OBS is needed. See third_party/unitycapture for upstream
source and its MIT notice. This module does not register/install the filter.
"""
import ctypes
from ctypes import wintypes
import struct

CAPACITY = 3840 * 2160 * 4 * 2
HEADER = struct.Struct("<I7i")


class VirtualCamera:
    def __init__(self, suffix=""):
        self.k = ctypes.WinDLL("kernel32", use_last_error=True)
        signatures = {
            "CreateMutexW": ([ctypes.c_void_p, wintypes.BOOL, wintypes.LPCWSTR], wintypes.HANDLE),
            "CreateEventW": ([ctypes.c_void_p, wintypes.BOOL, wintypes.BOOL, wintypes.LPCWSTR], wintypes.HANDLE),
            "CreateFileMappingW": ([wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                                    wintypes.DWORD, wintypes.DWORD, wintypes.LPCWSTR], wintypes.HANDLE),
            "MapViewOfFile": ([wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD,
                               wintypes.DWORD, ctypes.c_size_t], ctypes.c_void_p),
            "WaitForSingleObject": ([wintypes.HANDLE, wintypes.DWORD], wintypes.DWORD),
            "ReleaseMutex": ([wintypes.HANDLE], wintypes.BOOL),
            "SetEvent": ([wintypes.HANDLE], wintypes.BOOL),
            "UnmapViewOfFile": ([ctypes.c_void_p], wintypes.BOOL),
            "CloseHandle": ([wintypes.HANDLE], wintypes.BOOL),
        }
        for name, (args, result) in signatures.items():
            fn = getattr(self.k, name)
            fn.argtypes, fn.restype = args, result
        self.handles = []
        self.view = None
        self.last_image = None
        try:
            self.mutex = self._handle(self.k.CreateMutexW(None, False, "UnityCapture_Mutx" + suffix))
            self.want = self._handle(self.k.CreateEventW(None, False, False, "UnityCapture_Want" + suffix))
            self.sent = self._handle(self.k.CreateEventW(None, False, False, "UnityCapture_Sent" + suffix))
            self.mapping = self._handle(self.k.CreateFileMappingW(
                wintypes.HANDLE(-1), None, 4, 0, HEADER.size + CAPACITY, "UnityCapture_Data" + suffix))
            self.view = self.k.MapViewOfFile(self.mapping, 2, 0, 0, HEADER.size + CAPACITY)
            if not self.view:
                raise ctypes.WinError(ctypes.get_last_error())
        except Exception:
            self.close()
            raise

    def _handle(self, handle):
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        self.handles.append(handle)
        return handle

    def send(self, image):
        if image is self.last_image:
            self.k.SetEvent(self.sent)
            self.k.WaitForSingleObject(self.want, 0)
            return
        # DirectShow RGB with positive biHeight is bottom-up. Keep preview top-down.
        data = image.tobytes("raw", "RGBA", 0, -1)
        if len(data) > CAPACITY:
            raise ValueError("Frame too large")
        result = self.k.WaitForSingleObject(self.mutex, 1000)
        if result not in (0, 0x80):  # WAIT_OBJECT_0 or abandoned mutex
            raise RuntimeError("Virtual camera is busy")
        try:
            header = HEADER.pack(CAPACITY, image.width, image.height, image.width,
                                 0, 1, 0, 1000)  # uint8 RGBA, resize enabled
            ctypes.memmove(self.view, header, len(header))
            ctypes.memmove(self.view + HEADER.size, data, len(data))
            self.last_image = image
        finally:
            self.k.ReleaseMutex(self.mutex)
        self.k.SetEvent(self.sent)
        self.k.WaitForSingleObject(self.want, 0)

    def close(self):
        if self.view:
            self.k.UnmapViewOfFile(self.view)
            self.view = None
        for handle in reversed(self.handles):
            self.k.CloseHandle(handle)
        self.handles.clear()
