"""Bounded, versioned wire protocol. All traffic runs inside pinned TLS."""
import json
import struct

from PIL import Image, ImageOps

MAX_PACKET = 2 * 1024 * 1024
MAX_PIXELS = 4096 * 4096
FRAME_HEADER = struct.Struct(">II")  # H.264 access-unit byte count, clockwise rotation
STATUS_MESSAGE = 0xFFFFFFFF


def read_exact(stream, count):
    data = bytearray()
    while len(data) < count:
        chunk = stream.recv(count - len(data))
        if not chunk:
            raise EOFError("Phone disconnected")
        data.extend(chunk)
    return bytes(data)


def read_json(stream, limit=4096):
    data = bytearray()
    while len(data) < limit:
        chunk = read_exact(stream, 1)
        if chunk == b"\n":
            result = json.loads(data)
            if not isinstance(result, dict):
                raise ValueError("Expected a JSON object")
            return result
        data.extend(chunk)
    raise ValueError("Handshake too large")


def send_json(stream, data):
    stream.sendall(json.dumps(data, separators=(",", ":")).encode() + b"\n")


def read_frame(stream, allow_status=False):
    length, rotation = FRAME_HEADER.unpack(read_exact(stream, FRAME_HEADER.size))
    if allow_status:
        if length == 0 and rotation == 0:
            return b"", 0  # Armed/idle heartbeat; the PC can start capture on reconnect.
        if rotation == STATUS_MESSAGE and 0 < length <= 4096:
            return read_exact(stream, length), rotation
    if not 0 < length <= MAX_PACKET or rotation not in (0, 90, 180, 270):
        raise ValueError("Invalid video frame header")
    return read_exact(stream, length), rotation


def normalize_frame(image, rotation, size=(1280, 720), mirror=False):
    if image.width * image.height > MAX_PIXELS:
        raise ValueError("Invalid video dimensions")
    if rotation:
        image = image.rotate(-rotation, expand=True)
    if mirror:
        image = ImageOps.mirror(image)
    if image.size != size:
        image = ImageOps.pad(image, size, method=Image.Resampling.BILINEAR, color=(13, 19, 29))
    return image.convert("RGBA")
