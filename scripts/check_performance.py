"""Synthetic local receiver smoke benchmark; excludes encoding/setup time.

This is not a Redmi/Wi-Fi/target-app benchmark. It exercises pinned TLS,
H.264 decoding, 720p output conversion and native shared-memory publication.
"""
import ctypes as C
from ctypes import wintypes as W
from fractions import Fraction
import json
from pathlib import Path
import socket
import ssl
import sys
import tempfile
import threading
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "desktop"))
import av
from PIL import Image, ImageDraw
from camerabridge.protocol import FRAME_HEADER, read_json, send_json
from camerabridge.receiver import Receiver
from camerabridge.virtual_camera import VirtualCamera


class Counters(C.Structure):
    _fields_ = [("cb", W.DWORD), ("PageFaultCount", W.DWORD)] + [
        (name, C.c_size_t) for name in ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
        "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]


def memory():
    kernel = C.WinDLL("kernel32")
    kernel.GetCurrentProcess.restype = W.HANDLE
    psapi = C.WinDLL("psapi")
    psapi.GetProcessMemoryInfo.argtypes = [W.HANDLE, C.POINTER(Counters), W.DWORD]
    result = Counters(); result.cb = C.sizeof(result)
    if not psapi.GetProcessMemoryInfo(kernel.GetCurrentProcess(), C.byref(result), C.sizeof(result)):
        raise C.WinError()
    return result.WorkingSetSize


def main():
    fps, count = 30, 150
    codec = av.CodecContext.create("libx264", "w")
    codec.width, codec.height, codec.pix_fmt = 1280, 720, "yuv420p"
    codec.time_base, codec.framerate, codec.bit_rate = Fraction(1, fps), Fraction(fps), 3000000
    codec.options = {"preset": "ultrafast", "tune": "zerolatency", "profile": "baseline", "g": str(fps)}
    background = Image.effect_noise((1280, 720), 30).convert("RGB")
    packets = []
    for index in range(count):
        image = background.copy()
        draw = ImageDraw.Draw(image)
        x = index * 11 % 1100
        draw.rectangle((x, 100, x + 150, 500), fill=(20, 180, 130))
        frame = av.VideoFrame.from_image(image); frame.pts = index
        packets.extend(bytes(packet) for packet in codec.encode(frame))
    packets.extend(bytes(packet) for packet in codec.encode(None))
    del codec, background, image, frame
    camera = VirtualCamera("Perf" + uuid.uuid4().hex)
    got, peak = 0, memory()
    done = threading.Event()

    def output(image):
        nonlocal got, peak
        camera.send(image)
        got += 1
        if got % 10 == 0: peak = max(peak, memory())
        if got == count: done.set()

    with tempfile.TemporaryDirectory() as directory:
        receiver = Receiver(directory, lambda kind, text: print(text) if kind == "error" else None,
                            output, port=0, host="127.0.0.1")
        receiver.start()
        try:
            context = ssl.create_default_context(cafile=str(Path(directory) / "cert.pem")); context.check_hostname = False
            with context.wrap_socket(socket.create_connection(("127.0.0.1", receiver.port)), server_hostname="CameraBridge") as stream:
                send_json(stream, {"v": 1, "codec": "h264", "token": receiver.token})
                if not read_json(stream)["ok"]: raise RuntimeError("Handshake rejected")
                start, cpu = time.perf_counter(), time.process_time()
                for index, packet in enumerate(packets):
                    stream.sendall(FRAME_HEADER.pack(len(packet), 0) + packet)
                    target = start + (index + 1) / fps
                    time.sleep(max(0, target - time.perf_counter()))
                if not done.wait(5): raise RuntimeError("Not all benchmark frames arrived")
                elapsed, cpu = time.perf_counter() - start, time.process_time() - cpu
        finally:
            receiver.stop(); camera.close()
    report = dict(scenario="Synthetic 720p/30fps local TLS receiver; no UI or consumer app",
                  decoded_frames=got, duration_seconds=round(elapsed, 3),
                  process_cpu_seconds=round(cpu, 3), average_cpu_one_core_percent=round(cpu / elapsed * 100, 1),
                  sampled_working_set_mib=round(peak / 1024**2, 1),
                  limitations="Includes synthetic sender overhead; excludes Redmi, Wi-Fi, GUI and consuming video apps")
    path = Path(__file__).resolve().parents[1] / "build" / "performance-smoke.json"
    path.parent.mkdir(exist_ok=True)
    path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__": main()
