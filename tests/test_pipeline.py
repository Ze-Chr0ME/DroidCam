import ctypes
from fractions import Fraction
import io
import json
from pathlib import Path
import socket
import ssl
import struct
import sys
import tempfile
import threading
import time
import unittest
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "desktop"))
import av
from PIL import Image
from camerabridge.decoder import Decoder
from camerabridge.protocol import FRAME_HEADER, MAX_PACKET, STATUS_MESSAGE, normalize_frame, read_frame, read_json, send_json
from camerabridge.receiver import Receiver, make_identity
from camerabridge.virtual_camera import CAPACITY, HEADER, VirtualCamera
from camerabridge.settings import Settings


def packets(count=8, size=(1280, 720)):
    encoder = av.CodecContext.create("libx264", "w")
    encoder.width, encoder.height = size
    encoder.pix_fmt = "yuv420p"
    encoder.time_base = Fraction(1, 20)
    encoder.framerate = Fraction(20, 1)
    encoder.options = {"preset": "ultrafast", "tune": "zerolatency", "profile": "baseline", "g": "20"}
    result = []
    for index in range(count):
        image = Image.new("RGB", size, (220, 30, 40))
        frame = av.VideoFrame.from_image(image)
        frame.pts = index
        result.extend(bytes(packet) for packet in encoder.encode(frame))
    result.extend(bytes(packet) for packet in encoder.encode(None))
    return result


class MemorySocket:
    def __init__(self, data): self.data = io.BytesIO(data)
    def recv(self, size): return self.data.read(min(size, 3))


class ProtocolTests(unittest.TestCase):
    def test_fragmented_wire_frame(self):
        data = b"arbitrary-access-unit"
        self.assertEqual(read_frame(MemorySocket(FRAME_HEADER.pack(len(data), 90) + data)), (data, 90))

    def test_reject_unbounded_frame_and_rotation(self):
        for length, rotation in [(0, 0), (MAX_PACKET + 1, 0), (10, 45)]:
            with self.assertRaises(ValueError): read_frame(MemorySocket(FRAME_HEADER.pack(length, rotation)))

    def test_truncation_and_handshake_bounds(self):
        with self.assertRaises(EOFError): read_frame(MemorySocket(FRAME_HEADER.pack(10, 0) + b"short"))
        with self.assertRaises(ValueError): read_json(MemorySocket(b"x" * 5000))
        with self.assertRaises(ValueError): read_json(MemorySocket(b"[]\n"))

    def test_rotation_and_letterboxing(self):
        image = Image.new("RGB", (80, 40), "red")
        result = normalize_frame(image, 90, size=(80, 40))
        self.assertEqual(result.size, (80, 40))
        self.assertEqual(result.getpixel((0, 0)), (13, 19, 29, 255))
        self.assertEqual(result.getpixel((40, 20)), (255, 0, 0, 255))

    def test_control_heartbeat_requires_negotiation(self):
        heartbeat = FRAME_HEADER.pack(0, 0)
        self.assertEqual(read_frame(MemorySocket(heartbeat), allow_status=True), (b"", 0))
        with self.assertRaises(ValueError): read_frame(MemorySocket(heartbeat))
        status = b'{"phone_preview":false}'
        self.assertEqual(read_frame(MemorySocket(FRAME_HEADER.pack(len(status), STATUS_MESSAGE) + status), allow_status=True),
                         (status, STATUS_MESSAGE))
        with self.assertRaises(ValueError):
            read_frame(MemorySocket(FRAME_HEADER.pack(4097, STATUS_MESSAGE)), allow_status=True)


class CodecTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls): cls.video = packets()

    def test_software_decode(self):
        decoder = Decoder(hardware=False)
        frames = [image for data in self.video for image in decoder.decode(data)]
        self.assertEqual(len(frames), 8)
        self.assertEqual(frames[0].size, (1280, 720))
        red, green, blue = frames[0].getpixel((100, 100))
        self.assertGreater(red, 200)
        self.assertLess(green, 45)

    def test_default_decoder(self):
        decoder = Decoder()
        frames = [image for data in self.video for image in decoder.decode(data)]
        self.assertEqual(len(frames), 8)
        print("Default decoder hardware enabled:", decoder.hardware)

    def test_virtual_camera_shared_memory(self):
        camera = VirtualCamera("Test" + uuid.uuid4().hex)
        try:
            image = Image.new("RGBA", (8, 4), (22, 44, 66, 255))
            camera.send(image)
            header = HEADER.unpack(ctypes.string_at(camera.view, HEADER.size))
            self.assertEqual(header[:4], (CAPACITY, 8, 4, 8))
            self.assertEqual(ctypes.string_at(camera.view + HEADER.size, 4), bytes([22, 44, 66, 255]))
            camera.send(image)  # Same-frame fast path must preserve the native buffer.
            self.assertEqual(ctypes.string_at(camera.view + HEADER.size, 4), bytes([22, 44, 66, 255]))
        finally:
            camera.close()


class ReceiverTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.frames = []
        self.signal = threading.Event()
        self.stats_signal = threading.Event()
        self.errors = []
        self.phone_states = []
        self.state_signal = threading.Event()
        self.receiver = Receiver(self.temp.name, self.got_status, self.got_frame, port=0, host="127.0.0.1")
        self.receiver.start()

    def tearDown(self):
        self.receiver.stop()
        self.temp.cleanup()

    def got_frame(self, image):
        self.frames.append(image)
        self.signal.set()

    def got_status(self, kind, text):
        if kind == "fps": self.stats_signal.set()
        if kind == "error": self.errors.append(text)
        if kind == "phone_state":
            self.phone_states.append(text)
            self.state_signal.set()

    def connect(self):
        # Trust only the receiver's specific self-signed identity, as the Android pin does.
        context = ssl.create_default_context(cafile=str(Path(self.temp.name) / "cert.pem"))
        context.check_hostname = False
        return context.wrap_socket(socket.create_connection(("127.0.0.1", self.receiver.port)), server_hostname="CameraBridge")

    def test_auth_rejection_then_valid_reconnection(self):
        with self.connect() as stream:
            send_json(stream, {"v": 1, "codec": "h264", "token": "incorrect"})
            self.assertFalse(read_json(stream)["ok"])
        video = packets(2)
        for _ in range(2):
            self.signal.clear()
            with self.connect() as stream:
                send_json(stream, {"v": 1, "codec": "h264", "token": self.receiver.token})
                self.assertTrue(read_json(stream)["ok"])
                for packet in video:
                    stream.sendall(FRAME_HEADER.pack(len(packet), 0) + packet)
                self.assertTrue(self.signal.wait(5), "No decoded frame received")
        self.assertEqual(self.frames[0].size, (1280, 720))

    def test_pairing_survives_pc_restart(self):
        _, fingerprint = make_identity(self.temp.name)
        self.assertEqual(fingerprint, self.receiver.fingerprint)
        other = Receiver(self.temp.name, lambda *args: None, lambda *args: None)
        self.assertTrue(other.token == self.receiver.token)
        with tempfile.TemporaryDirectory() as other_directory:
            unrelated = Receiver(other_directory, lambda *args: None, lambda *args: None)
            self.assertTrue(unrelated.token != self.receiver.token)

    def remote_connect(self):
        stream = self.connect()
        send_json(stream, {"v": 2, "codec": "h264", "stream_settings": True, "remote_control": True,
                           "token": self.receiver.token})
        reply = read_json(stream)
        self.assertTrue(reply["remote_control"])
        return stream, reply

    def test_pc_starts_stopped_phone_after_settings(self):
        self.receiver.set_streaming(False)
        stream, reply = self.remote_connect()
        try:
            self.assertFalse(reply["streaming"])
            stream.sendall(FRAME_HEADER.pack(0, 0))
            self.assertTrue(self.receiver.connected)
            self.receiver.configure(Settings(fps=24, phone_preview=False, lens="front"), activate=True)
            stream.settimeout(2)
            self.assertEqual(stream.recv(1), b"")
        finally:
            stream.close()
        # The foreground service reconnects itself with its existing pairing.
        stream, updated = self.remote_connect()
        with stream:
            self.assertTrue(updated["streaming"])
            self.assertEqual(updated["fps"], 24)
            self.assertFalse(updated["phone_preview"])
            self.assertEqual(updated["lens"], "front")

    def test_phone_state_ignores_stale_settings(self):
        stream, reply = self.remote_connect()
        with stream:
            stale = json.dumps({"revision": reply["revision"] - 1, "phone_preview": True}).encode()
            current = json.dumps({"revision": reply["revision"], "phone_preview": False, "streaming": True}).encode()
            stream.sendall(FRAME_HEADER.pack(len(stale), STATUS_MESSAGE) + stale)
            stream.sendall(FRAME_HEADER.pack(len(current), STATUS_MESSAGE) + current)
            self.assertTrue(self.state_signal.wait(2))
            self.assertEqual(len(self.phone_states), 1)
            self.assertFalse(self.phone_states[0]["phone_preview"])

    def test_settings_negotiation_and_reconnection(self):
        self.receiver.configure(Settings(resolution="960x540", fps=24, bitrate_mbps=1.5, decoder="cpu"))
        with self.connect() as stream:
            send_json(stream, {"v": 1, "codec": "h264", "stream_settings": True, "token": self.receiver.token})
            reply = read_json(stream)
            self.assertEqual((reply["width"], reply["height"], reply["fps"], reply["bitrate"]), (960, 540, 24, 1500000))
            for packet in packets(1):
                stream.sendall(FRAME_HEADER.pack(len(packet), 0) + packet)
            self.assertTrue(self.signal.wait(5))
            self.assertEqual(self.frames[-1].size, (960, 540))
            # Local preview changes preserve the authenticated connection.
            self.receiver.configure(Settings(resolution="960x540", fps=24, bitrate_mbps=1.5, decoder="cpu", preview_fps=5))
            self.assertTrue(self.receiver.connected)
            # Stream changes must renegotiate with the same pairing token.
            self.receiver.configure(Settings(fps=15, decoder="cpu"))
            stream.settimeout(2)
            self.assertEqual(stream.recv(1), b"")
        with self.connect() as stream:
            send_json(stream, {"v": 1, "codec": "h264", "stream_settings": True, "token": self.receiver.token})
            self.assertEqual(read_json(stream)["fps"], 15)

    def test_live_stats_do_not_interrupt_stream(self):
        with self.connect() as stream:
            send_json(stream, {"v": 1, "codec": "h264", "stream_settings": True, "token": self.receiver.token})
            self.assertTrue(read_json(stream)["ok"])
            for packet in packets(18):
                stream.sendall(FRAME_HEADER.pack(len(packet), 0) + packet)
                time.sleep(0.07)
            self.assertTrue(self.stats_signal.wait(2), "Live FPS statistics were never emitted")
            self.assertFalse(self.errors)
            self.assertTrue(self.receiver.connected)


if __name__ == "__main__": unittest.main(verbosity=2)
