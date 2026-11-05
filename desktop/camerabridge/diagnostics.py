"""An isolated packaged-app check; never installs a camera or opens a LAN port."""
from fractions import Fraction
from dataclasses import replace
import json
from pathlib import Path
import tempfile
import tkinter as tk
import traceback
import uuid

import av
from PIL import Image


def self_test(output):
    result = {"ok": False}
    try:
        from . import app
        from .decoder import Decoder
        from .receiver import Receiver
        from .virtual_camera import VirtualCamera
        encoder = av.CodecContext.create("libx264", "w")
        encoder.width, encoder.height, encoder.pix_fmt = 1280, 720, "yuv420p"
        encoder.time_base = Fraction(1, 20)
        encoder.options = {"preset": "ultrafast", "tune": "zerolatency", "profile": "baseline"}
        frame = av.VideoFrame.from_image(Image.new("RGB", (1280, 720), (200, 30, 40)))
        encoded = list(encoder.encode(frame)) + list(encoder.encode(None))
        decoder = Decoder()
        decoded = [image for packet in encoded for image in decoder.decode(bytes(packet))]
        assert len(decoded) == 1 and decoded[0].size == (1280, 720)
        errors = []
        with tempfile.TemporaryDirectory() as directory:
            app.DATA = Path(directory)
            app.Receiver = lambda identity, status, callback: Receiver(identity, status, callback, port=0, host="127.0.0.1")
            app.VirtualCamera = lambda: VirtualCamera("SelfTest" + uuid.uuid4().hex)
            root = tk.Tk()
            root.withdraw()
            root.report_callback_exception = lambda *error: errors.append(str(error))
            application = app.App(root)
            assert application.preview.bind("<Button-3>"), "Preview right-click menu is not bound"
            application.set_streaming(False)
            application.set_pc_preview(False)
            assert not application.receiver.desired_stream, "PC preview toggle unexpectedly starts camera"
            application.apply_settings(replace(application.settings, bitrate_mbps=4.0))
            assert application.receiver.desired_stream and not application.paused, "Settings failed to start the camera"
            application.adopt_phone_state(dict(revision=application.receiver.revision, phone_preview=False,
                                               lens="front", streaming=True))
            assert not application.settings.phone_preview and application.settings.lens == "front"
            root.after(100, application.open_settings)
            root.after(200, lambda: application.apply_settings(replace(application.settings, preview_fps=15)))
            root.after(300, application.toggle_pause)
            root.after(500, application.toggle_pause)
            root.after(700, application.close)
            root.mainloop()
            if errors:
                raise RuntimeError(errors)
            saved = json.loads((Path(directory) / "settings.json").read_text(encoding="utf-8"))
            assert saved["preview_fps"] == 15
        for filename in ("UnityCaptureFilter64.dll", "UnityCaptureFilter32.dll", "Install-Camera.ps1"):
            assert (app.ROOT / "third_party/unitycapture" / filename).exists(), filename
        result.update(ok=True, h264_decode=True, d3d11_context=decoder.hardware,
                      tk_ui=True, qr_render=True, native_shared_memory=True, camera_setup_files=True,
                      settings_dialog=True, persistent_settings=True)
        result.update(preview_context_menu=True, settings_start_camera=True, independent_preview=True, phone_state_sync=True)
    except Exception:
        result["error"] = traceback.format_exc()
    Path(output).write_text(json.dumps(result, indent=2), encoding="utf-8")
    if not result["ok"]:
        raise SystemExit(1)
