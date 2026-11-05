"""Low-delay H.264 decoder with D3D11 acceleration when available."""
import logging
import av


class Decoder:
    def __init__(self, hardware=True):
        self.hardware = False
        if hardware:
            try:
                from av.codec.hwaccel import HWAccel
                self.codec = av.CodecContext.create("h264", "r", hwaccel=HWAccel(
                    device_type="d3d11va", allow_software_fallback=True))
                self.hardware = self.codec.is_hwaccel
            except Exception:
                logging.info("D3D11 decoding unavailable; using software decoder")
                self.codec = av.CodecContext.create("h264", "r")
        else:
            self.codec = av.CodecContext.create("h264", "r")
        self.codec.thread_count = 2
        self.codec.thread_type = "SLICE"  # Frame threading adds live-video delay.
        self.codec.options = {"flags": "low_delay"}

    def decode(self, payload):
        for frame in self.codec.decode(av.Packet(payload)):
            if frame.width > 1920 or frame.height > 1080:
                raise ValueError("Video exceeds the negotiated 1080p limit")
            yield frame.to_image()
