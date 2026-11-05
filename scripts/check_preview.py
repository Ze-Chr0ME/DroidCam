"""Run the real Tk preview against a synthetic 30-fps source, without LAN/setup.

This catches a GUI refresh cap that receiver/codec-only benchmarks cannot see.
No screenshots, camera registration, or changes to user preferences.
"""
from dataclasses import replace
import json
from pathlib import Path
import sys
import tempfile
import threading
import time
import tkinter as tk
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "desktop"))
from PIL import Image
from camerabridge import app
from camerabridge.receiver import Receiver
from camerabridge.virtual_camera import VirtualCamera


def main():
    with tempfile.TemporaryDirectory() as directory:
        app.DATA = Path(directory)
        app.Receiver = lambda identity, status, frame: Receiver(identity, status, frame, port=0, host="127.0.0.1")
        publications = []

        class Output:
            def __init__(self):
                self.camera = VirtualCamera("PreviewTest" + uuid.uuid4().hex)
                self.previous = None

            def send(self, image):
                self.camera.send(image)
                if image is not self.previous and image.size == (1280, 720) and image.getpixel((0, 0))[1:3] == (80, 160):
                    publications.append(time.monotonic())
                self.previous = image

            def close(self): self.camera.close()

        app.VirtualCamera = Output
        root = tk.Tk()
        root.withdraw()
        failures = []
        root.report_callback_exception = lambda *error: failures.append(str(error))
        application = app.App(root)
        root.update_idletasks()
        # Keep this isolated window hidden but exercise a realistic 600x360 preview.
        application.preview.winfo_width = lambda: 600
        application.preview.winfo_height = lambda: 360
        draws = []
        original = application.preview.configure

        def count_draw(**kwargs):
            if "image" in kwargs:
                pixel = root.tk.call(str(kwargs["image"]), "get", 0, 0)
                # Count live synthetic frames, excluding the standby image drawn on disable.
                if tuple(pixel[1:]) == (80, 160):
                    draws.append((phase[0], time.monotonic()))
            return original(**kwargs)

        application.preview.configure = count_draw
        phase = ["smooth"]
        stop = threading.Event()
        images = [Image.new("RGBA", (1280, 720), (i * 4, 80, 160, 255)) for i in range(60)]

        def source():
            index, started = 0, time.monotonic()
            while not stop.is_set():
                application.on_frame(images[index % len(images)])
                index += 1
                stop.wait(max(0, started + index / 30 - time.monotonic()))

        producer = threading.Thread(target=source, daemon=True)
        producer.start()

        def slow():
            phase[0] = "low_power"
            application.apply_settings(replace(application.settings, preview_fps=5))

        def disabled():
            phase[0] = "disabled"
            application.apply_settings(replace(application.settings, preview_fps=0))

        def finish():
            stop.set()
            application.close()

        root.after(2400, slow)
        root.after(3600, disabled)
        root.after(4200, finish)
        root.mainloop()
        producer.join(1)
        if failures:
            raise RuntimeError(failures)
        smooth = [t for name, t in draws if name == "smooth"]
        slow_draws = [t for name, t in draws if name == "low_power"]
        rate = (len(smooth) - 1) / (smooth[-1] - smooth[0])
        assert 24 <= rate <= 33, f"Smooth preview is unexpectedly capped or running too fast: {rate:.1f} fps"
        assert 2 <= len(slow_draws) <= 8, f"Low-power cap was ignored: {len(slow_draws)} draws"
        assert not [t for name, t in draws if name == "disabled"], "Disabled preview still drawing live frames"
        output_fps = (len(publications) - 1) / (publications[-1] - publications[0])
        assert 27 <= output_fps <= 33, f"Webcam publication failed to follow 30-fps source: {output_fps:.1f}"
        result = dict(ok=True, scenario="Real Tk preview with synthetic 720p/30fps input, 600x360 preview",
                      rendered_preview_fps=round(rate, 1), lower_preview_cap=True, disabled_preview=True,
                      published_fps=round(output_fps, 1),
                      limitations="Synthetic source; no Redmi, Wi-Fi or consuming video app")
        output = Path(__file__).resolve().parents[1] / "build" / "preview-check.json"
        output.parent.mkdir(exist_ok=True)
        output.write_text(json.dumps(result, indent=2), encoding="utf-8")
        print(json.dumps(result, indent=2))


if __name__ == "__main__": main()
