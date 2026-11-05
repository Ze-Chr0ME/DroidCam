import ctypes
from dataclasses import asdict, replace
import logging
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time
import tkinter as tk
from tkinter import messagebox, ttk
import winreg

from PIL import Image, ImageDraw, ImageFont, ImageTk
import qrcode

from .receiver import Receiver, local_addresses
from .virtual_camera import VirtualCamera
from . import settings as preferences

BG, PANEL, TEXT, MUTED, ACCENT = "#0c1420", "#152131", "#e8eef6", "#93a5bc", "#66e2bf"
ROOT = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parents[2]))
DATA = Path(os.environ.get("LOCALAPPDATA", Path.home())) / "CameraBridge"


def installed():
    key = r"SOFTWARE\Classes\CLSID\{5C2CD55C-92AD-4999-8666-912BD3E70010}\InprocServer32"
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, key, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as handle:
            path = winreg.QueryValue(handle, None)
            expected = Path(os.environ["ProgramFiles"]) / "CameraBridge" / "Camera" / "UnityCaptureFilter64.dll"
            return Path(path).resolve() == expected.resolve() and expected.exists()
    except OSError:
        return False


def standby_image():
    image = Image.new("RGBA", (1280, 720), "#0d131d")
    draw = ImageDraw.Draw(image)
    try:
        font = ImageFont.truetype("segoeui.ttf", 42)
        small = ImageFont.truetype("segoeui.ttf", 22)
    except OSError:
        font = small = ImageFont.load_default()
    draw.rounded_rectangle((550, 210, 730, 350), radius=30, outline=ACCENT, width=5)
    draw.ellipse((610, 250, 670, 310), outline=ACCENT, width=4)
    draw.text((640, 420), "CameraBridge", fill=TEXT, font=font, anchor="mm")
    draw.text((640, 475), "Waiting for your phone", fill=MUTED, font=small, anchor="mm")
    return image


class App:
    def __init__(self, window):
        self.window = window
        window.title("CameraBridge 0.3.0 · Your phone, your webcam")
        window.geometry("1080x720")
        window.minsize(960, 640)
        window.configure(bg=BG)
        self.events = queue.SimpleQueue()
        self.lock = threading.Lock()
        self.latest, self.updated = None, 0.0
        self.settings = preferences.load(DATA / "settings.json")
        self.standby_base = standby_image()
        self.standby = self.standby_base.resize(self.settings.size, Image.Resampling.BILINEAR)
        self.last_preview_fps = self.settings.preview_fps or 30
        self.preview_frames = 0
        self.preview_started = time.monotonic()
        self.settings_window = None
        self.stopping = threading.Event()
        self.frame_ready = threading.Event()
        self.paused = False
        self.receiver = None
        self.publisher = None
        self.build_ui()
        try:
            self.receiver = Receiver(DATA / "identity", self.on_status, self.on_frame)
            self.receiver.configure(self.settings)
            self.receiver.start()
        except Exception as error:
            messagebox.showerror("Cannot start receiver", str(error) + "\nClose other CameraBridge instances or check the log.")
            window.destroy()
            return
        self.render_qr()
        self.publisher = threading.Thread(target=self.publish, daemon=True)
        self.publisher.start()
        window.protocol("WM_DELETE_WINDOW", self.close)
        window.after(100, self.refresh)
        window.after(0, self.draw_preview)

    def label(self, parent, text, size=11, color=TEXT, bg=PANEL, **kwargs):
        return tk.Label(parent, text=text, font=("Segoe UI", size), fg=color, bg=bg, **kwargs)

    def button(self, parent, text, command, primary=False):
        return tk.Button(parent, text=text, command=command, font=("Segoe UI", 11),
                         bg=ACCENT if primary else "#26374c", fg=BG if primary else TEXT,
                         activebackground="#8cebd0", activeforeground=BG, relief="flat",
                         bd=0, padx=16, pady=10, cursor="hand2")

    def build_ui(self):
        header = tk.Frame(self.window, bg=BG)
        header.pack(fill="x", padx=26, pady=(22, 16))
        self.label(header, "◉  CameraBridge", 25, ACCENT, BG).pack(side="left")
        self.label(header, "ANDROID → WINDOWS  /  LOCAL WI-FI", 10, MUTED, BG).pack(side="right")
        content = tk.Frame(self.window, bg=BG)
        content.pack(fill="both", expand=True, padx=26)
        side = tk.Frame(content, bg=PANEL, width=346)
        side.pack(side="right", fill="y", padx=(18, 0))
        side.pack_propagate(False)
        left = tk.Frame(content, bg=PANEL)
        left.pack(side="left", fill="both", expand=True)
        self.preview_heading = self.label(left, "LIVE PREVIEW", 10, MUTED)
        self.preview_heading.pack(anchor="w", padx=20, pady=(18, 8))
        self.preview = tk.Label(left, bg="#0d131d", bd=0)
        self.preview.pack(fill="both", expand=True, padx=16, pady=8)
        self.preview.bind("<Button-3>", self.preview_menu)
        self.preview_heading.bind("<Button-3>", self.preview_menu)
        self.connection = self.label(left, "Waiting for your phone", 13, ACCENT)
        self.connection.pack(anchor="w", padx=20, pady=(10, 3))
        self.stats = self.label(left, self.settings_description(), 10, MUTED)
        self.stats.pack(anchor="w", padx=20, pady=(0, 12))
        controls = tk.Frame(left, bg=PANEL)
        controls.pack(fill="x", padx=20, pady=(0, 16))
        self.pause_button = self.button(controls, "Stop camera", self.toggle_pause)
        self.pause_button.pack(side="left")
        self.mirror = tk.BooleanVar(value=self.settings.mirror)
        tk.Checkbutton(controls, text="Mirror output", variable=self.mirror, command=self.set_mirror,
                       bg=PANEL, fg=TEXT, selectcolor=BG, activebackground=PANEL,
                       activeforeground=TEXT, font=("Segoe UI", 10)).pack(side="right")
        self.label(side, "Connect your phone", 19).pack(anchor="w", padx=20, pady=(18, 4))
        self.label(side, "Open the Android app and scan this code.", 10, MUTED).pack(anchor="w", padx=20)
        self.qr = tk.Label(side, bg="white", bd=8)
        self.qr.pack(pady=(14, 10))
        addresses = local_addresses()
        style = ttk.Style()
        style.theme_use("clam")
        style.configure("TCombobox", fieldbackground="#26374c", background="#26374c", foreground=TEXT)
        self.address = tk.StringVar(value=addresses[0])
        self.address_box = ttk.Combobox(side, textvariable=self.address, values=addresses,
                                       state="readonly", font=("Segoe UI", 11))
        self.address_box.pack(fill="x", padx=20)
        self.address_box.bind("<<ComboboxSelected>>", lambda _: self.render_qr())
        self.label(side, "Pick the address on your phone’s Wi-Fi network.", 9, MUTED).pack(pady=4)
        self.button(side, "Copy pairing text", self.copy_pairing).pack(fill="x", padx=20, pady=(3, 10))
        self.camera_state = self.label(side, "", 10, MUTED)
        self.camera_state.pack(anchor="w", padx=20)
        self.button(side, "Set up Windows webcam", self.install_camera, True).pack(fill="x", padx=20, pady=8)
        self.label(side, "Then select “CameraBridge” in your video app.\nUse your PC microphone for audio.",
                   10, MUTED, justify="left").pack(anchor="w", padx=20, pady=(0, 10))
        footer = tk.Frame(self.window, bg=BG)
        footer.pack(fill="x", padx=26, pady=(14, 18))
        self.label(footer, "Encrypted Wi-Fi · paired phone can stream while locked", 10, MUTED, BG).pack(side="left")
        self.show_preview = tk.BooleanVar(value=self.settings.preview_fps > 0)
        tk.Checkbutton(footer, text="Preview", variable=self.show_preview, command=self.toggle_preview, bg=BG, fg=MUTED,
                       selectcolor=PANEL, activebackground=BG, activeforeground=TEXT,
                       font=("Segoe UI", 10)).pack(side="right", padx=12)
        tk.Button(footer, text="Help", command=self.help, bg=BG, fg=ACCENT, relief="flat",
                  font=("Segoe UI", 10), cursor="hand2").pack(side="right")
        tk.Button(footer, text="Settings", command=self.open_settings, bg=BG, fg=ACCENT, relief="flat",
                  font=("Segoe UI", 10), cursor="hand2").pack(side="right", padx=12)

    def settings_description(self):
        return f"{self.settings.resolution} · target {self.settings.fps} fps · {self.settings.bitrate_mbps:g} Mb/s"

    def apply_settings(self, settings, activate=True):
        preferences.save(DATA / "settings.json", settings)
        previous = self.settings
        self.settings = settings
        if settings.preview_fps:
            self.last_preview_fps = settings.preview_fps
        self.show_preview.set(settings.preview_fps > 0)
        self.mirror.set(settings.mirror)
        if previous.size != settings.size:
            self.standby = self.standby_base.resize(settings.size, Image.Resampling.BILINEAR)
            with self.lock:
                self.latest, self.updated = None, 0.0
        if activate:
            self.paused = False
            self.pause_button.configure(text="Stop camera")
        self.receiver.configure(settings, activate=activate)
        self.stats.configure(text=self.settings_description())
        self.frame_ready.set()

    def set_pc_preview(self, enabled):
        try:
            self.apply_settings(replace(self.settings, preview_fps=self.last_preview_fps if enabled else 0), activate=False)
        except OSError as error:
            messagebox.showerror("Cannot save settings", str(error))

    def set_phone_preview(self, enabled):
        try:
            self.apply_settings(replace(self.settings, phone_preview=enabled), activate=False)
        except OSError as error:
            messagebox.showerror("Cannot save settings", str(error))

    def preview_menu(self, event):
        previous = getattr(self, "preview_context", None)
        if previous is not None:
            previous.destroy()
        menu = tk.Menu(self.window, tearoff=False, bg=PANEL, fg=TEXT, activebackground=ACCENT, activeforeground=BG)
        self.preview_context = menu
        menu.add_command(label="Disable PC preview", command=lambda: self.set_pc_preview(False),
                         state="normal" if self.settings.preview_fps else "disabled")
        menu.add_command(label="Enable PC preview", command=lambda: self.set_pc_preview(True),
                         state="disabled" if self.settings.preview_fps else "normal")
        menu.add_separator()
        menu.add_command(label="Disable phone preview", command=lambda: self.set_phone_preview(False))
        menu.add_command(label="Enable phone preview", command=lambda: self.set_phone_preview(True))
        menu.add_separator()
        menu.add_command(label="Start camera", command=lambda: self.set_streaming(True))
        menu.add_command(label="Stop camera", command=lambda: self.set_streaming(False))
        menu.add_command(label="Settings…", command=self.open_settings)
        try:
            menu.tk_popup(event.x_root, event.y_root)
        finally:
            menu.grab_release()

    def set_streaming(self, enabled):
        self.paused = not enabled
        self.pause_button.configure(text="Stop camera" if enabled else "Start camera")
        self.receiver.set_streaming(enabled)
        self.frame_ready.set()

    def adopt_phone_state(self, state):
        if state.get("revision") != self.receiver.revision:
            return  # An acknowledgement queued before a newer PC change is stale.
        changes = {}
        if type(state.get("phone_preview")) is bool:
            changes["phone_preview"] = state["phone_preview"]
        if state.get("lens") in ("front", "rear"):
            changes["lens"] = state["lens"]
        if changes:
            updated = replace(self.settings, **changes)
            if updated != self.settings:
                try:
                    preferences.save(DATA / "settings.json", updated)
                except OSError:
                    logging.warning("Could not persist phone-side settings", exc_info=True)
                self.settings = updated
                self.receiver.settings = updated
        if type(state.get("streaming")) is bool:
            self.paused = not state["streaming"]
            self.receiver.desired_stream = state["streaming"]
            self.pause_button.configure(text="Start camera" if self.paused else "Stop camera")
            if self.paused:
                with self.lock:
                    self.latest, self.updated = None, 0.0
                self.stats.configure(text="Camera stopped · phone ready for PC control")
            self.frame_ready.set()
        if state.get("error"):
            self.connection.configure(text=str(state["error"])[:500], wraplength=max(300, self.preview.winfo_width() - 16))

    def open_settings(self):
        if self.settings_window and self.settings_window.winfo_exists():
            self.settings_window.lift()
            return
        dialog = self.settings_window = tk.Toplevel(self.window)
        dialog.title("CameraBridge settings")
        dialog.configure(bg=PANEL)
        dialog.resizable(False, False)
        dialog.transient(self.window)
        body = tk.Frame(dialog, bg=PANEL, padx=24, pady=18)
        body.pack(fill="both", expand=True)
        self.label(body, "Video & performance", 18).grid(row=0, column=0, columnspan=2, sticky="w", pady=(0, 12))
        variables = {key: tk.StringVar(value=str(value)) for key, value in asdict(self.settings).items()
                     if key not in ("mirror", "steady_fps", "phone_preview")}
        preset = tk.StringVar(value="Custom")

        def combo(row, title, variable, values):
            self.label(body, title, 11).grid(row=row, column=0, sticky="w", padx=(0, 18), pady=7)
            field = ttk.Combobox(body, textvariable=variable, values=values, state="readonly", width=24)
            field.grid(row=row, column=1, sticky="ew", pady=7)
            return field

        preset_box = combo(1, "Preset", preset, ["Custom", *preferences.PRESETS])
        combo(2, "Stream resolution", variables["resolution"], preferences.RESOLUTIONS)
        combo(3, "Stream FPS", variables["fps"], preferences.FRAME_RATES)
        combo(4, "Bitrate (Mb/s)", variables["bitrate_mbps"], [f"{x:.1f}" for x in preferences.BITRATES])
        combo(5, "PC preview FPS (0 = off)", variables["preview_fps"], preferences.PREVIEW_RATES)
        decoder = tk.StringVar(value="Auto (GPU if available)" if self.settings.decoder == "auto" else "CPU")
        combo(6, "PC video decoder", decoder, ("Auto (GPU if available)", "CPU"))
        combo(7, "Phone camera", variables["lens"], ("rear", "front"))
        steady = tk.BooleanVar(value=self.settings.steady_fps)
        mirror = tk.BooleanVar(value=self.settings.mirror)
        phone_preview = tk.BooleanVar(value=self.settings.phone_preview)
        for row, title, variable in ((8, "Prefer steady FPS in low light", steady), (9, "Mirror webcam output", mirror),
                                     (10, "Show preview on the phone", phone_preview)):
            tk.Checkbutton(body, text=title, variable=variable, bg=PANEL, fg=TEXT, selectcolor=BG,
                           activebackground=PANEL, activeforeground=TEXT, font=("Segoe UI", 10)).grid(
                               row=row, column=0, columnspan=2, sticky="w", pady=5)
        self.label(body, "Apply settings starts the paired phone camera automatically.\nEnable PC control on the phone once; its screen may then lock.\nSteady FPS can make a dark room look darker.",
                   10, MUTED, justify="left").grid(row=11, column=0, columnspan=2, sticky="w", pady=10)

        def use_preset(_):
            if preset.get() not in preferences.PRESETS:
                return
            chosen = preferences.PRESETS[preset.get()]
            for key, variable in variables.items():
                if key != "lens":
                    variable.set(str(getattr(chosen, key)))
            steady.set(chosen.steady_fps)
            decoder.set("Auto (GPU if available)" if chosen.decoder == "auto" else "CPU")

        preset_box.bind("<<ComboboxSelected>>", use_preset)

        def apply():
            try:
                chosen = preferences.validate(dict(
                    resolution=variables["resolution"].get(), fps=int(variables["fps"].get()),
                    bitrate_mbps=float(variables["bitrate_mbps"].get()),
                    preview_fps=int(variables["preview_fps"].get()),
                    decoder="cpu" if decoder.get() == "CPU" else "auto",
                    steady_fps=steady.get(), mirror=mirror.get(),
                    phone_preview=phone_preview.get(), lens=variables["lens"].get()))
                self.apply_settings(chosen)
            except (OSError, ValueError) as error:
                messagebox.showerror("Cannot save settings", str(error), parent=dialog)
                return
            dialog.destroy()

        actions = tk.Frame(body, bg=PANEL)
        actions.grid(row=12, column=0, columnspan=2, sticky="e", pady=(8, 0))
        self.button(actions, "Cancel", dialog.destroy).pack(side="left", padx=8)
        self.button(actions, "Apply settings", apply, True).pack(side="left")

    def render_qr(self):
        if self.receiver:
            # Integer scaling preserves sharp QR modules.
            qr = qrcode.QRCode(box_size=1, border=4)
            qr.add_data(self.receiver.pairing(self.address.get()))
            qr.make(fit=True)
            image = qr.make_image(fill_color="black", back_color="white").convert("RGB")
            scale = max(1, 240 // image.width)
            image = image.resize((image.width * scale, image.height * scale), Image.Resampling.NEAREST)
            self.qr_image = ImageTk.PhotoImage(image)
            self.qr.configure(image=self.qr_image)

    def copy_pairing(self):
        self.window.clipboard_clear()
        self.window.clipboard_append(self.receiver.pairing(self.address.get()))

    def set_mirror(self):
        try:
            self.apply_settings(replace(self.settings, mirror=self.mirror.get()))
        except OSError as error:
            self.mirror.set(self.settings.mirror)
            messagebox.showerror("Cannot save settings", str(error))

    def toggle_preview(self):
        try:
            self.apply_settings(replace(self.settings, preview_fps=self.last_preview_fps if self.show_preview.get() else 0), activate=False)
        except OSError as error:
            self.show_preview.set(self.settings.preview_fps > 0)
            messagebox.showerror("Cannot save settings", str(error))

    def toggle_pause(self):
        self.set_streaming(self.paused)

    def install_camera(self):
        script = ROOT / "third_party" / "unitycapture" / "Install-Camera.ps1"
        if not script.exists():
            messagebox.showerror("Setup missing", "Rebuild the Windows package; the camera setup script is missing.")
            return
        program = Path(sys.executable)
        args = subprocess.list2cmdline(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(script),
                                        "-ReceiverPath", str(program)])
        shell = ctypes.WinDLL("shell32", use_last_error=True)
        shell.ShellExecuteW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_wchar_p,
                                       ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_int]
        shell.ShellExecuteW.restype = ctypes.c_void_p
        result = shell.ShellExecuteW(None, "runas", "powershell.exe", args, None, 1)
        if not result or result <= 32:
            messagebox.showinfo("Webcam setup", "Windows webcam setup was cancelled or could not start. You can try again.")

    def on_status(self, kind, text):
        if kind == "disconnected":
            with self.lock:
                self.latest, self.updated = None, 0.0
            self.frame_ready.set()
        self.events.put((kind, text))

    def on_frame(self, image):
        with self.lock:
            self.latest, self.updated = image, time.monotonic()
        self.frame_ready.set()

    def current_image(self):
        with self.lock:
            if self.paused or time.monotonic() - self.updated > 1.5:
                return self.standby
            return self.latest or self.standby

    def publish(self):
        camera = None
        try:
            camera = VirtualCamera()
            while not self.stopping.is_set():
                self.frame_ready.clear()
                image = self.current_image()
                camera.send(image)
                # Wake on decoded frames instead of sampling them on another
                # drifting timer. Idle heartbeats keep the native filter alive.
                timeout = 0.2 if image is self.standby else 1 / self.settings.fps
                self.frame_ready.wait(timeout)
        except Exception as error:
            logging.exception("Virtual camera output failed")
            self.events.put(("error", "Webcam output failed: " + str(error)))
        finally:
            if camera:
                # Overwrite the last frame so closing never leaves a private image behind.
                try:
                    camera.send(self.standby)
                finally:
                    camera.close()

    def draw_preview(self):
        """Independent drawing timer: a slow status poll must never cap preview FPS."""
        if self.stopping.is_set():
            return
        started = time.monotonic()
        if self.window.state() == "iconic":
            self.window.after(250, self.draw_preview)
            return
        source = self.current_image() if self.settings.preview_fps else self.standby
        size = (max(100, self.preview.winfo_width()), max(100, self.preview.winfo_height()))
        fps = self.settings.preview_fps
        if fps != getattr(self, "preview_clock_fps", None):
            self.preview_clock_fps = fps
            self.preview_deadline = started
        new_source = source is not getattr(self, "preview_source", None)
        resized = size != getattr(self, "preview_size", None)
        if resized or (new_source and started >= self.preview_deadline):
            scale = min(size[0] / source.width, size[1] / source.height, 1)
            target = (max(1, int(source.width * scale)), max(1, int(source.height * scale)))
            # Resize directly from the source, avoiding a full-frame copy before thumbnailing.
            image = source.resize(target, Image.Resampling.BILINEAR)
            self.preview_image = ImageTk.PhotoImage(image)
            self.preview.configure(image=self.preview_image)
            self.preview_source, self.preview_size = source, size
            if source is not self.standby:
                self.preview_frames += 1
            # Keep an absolute deadline. Relative sleeps accumulate Windows timer
            # jitter; waiting a whole frame after a missed source update halves FPS.
            period = 1 / max(fps, 1)
            self.preview_deadline = max(self.preview_deadline + period, started)
        elapsed = time.monotonic() - self.preview_started
        if elapsed >= 1:
            text = f"LIVE PREVIEW · {self.preview_frames / elapsed:.0f} fps" if source is not self.standby else "PREVIEW OFF" if not self.settings.preview_fps else "LIVE PREVIEW · waiting"
            self.preview_heading.configure(text=text)
            self.preview_frames, self.preview_started = 0, time.monotonic()
        remaining_ms = (self.preview_deadline - time.monotonic()) * 1000
        delay = max(1, round(remaining_ms)) if remaining_ms > 0 else 4
        if source is self.standby:
            delay = 200
        self.window.after(delay, self.draw_preview)

    def refresh(self):
        while not self.events.empty():
            kind, text = self.events.get()
            if kind == "fps":
                self.stats.configure(text=text)
            elif kind == "phone_state":
                self.adopt_phone_state(text)
            elif kind == "notice":
                self.stats.configure(text=text)
            else:
                self.connection.configure(text=text, wraplength=max(300, self.preview.winfo_width() - 16))
        ready = installed()
        self.camera_state.configure(text="● Windows webcam installed" if ready else "○ Windows webcam needs setup",
                                    fg=ACCENT if ready else MUTED)
        self.window.after(1000, self.refresh)

    def help(self):
        messagebox.showinfo("Connecting CameraBridge",
            "1. Click Set up Windows webcam and approve Windows setup.\n"
            "2. Install CameraBridge-Android.apk on your phone.\n"
            "3. Put both devices on the same home/private network.\n"
            "4. Scan the QR code inside the phone app; this enables PC control.\n"
            "5. Restart your video app and choose CameraBridge as its camera.\n\n"
            "If connection fails, choose a different PC address above. Avoid guest Wi-Fi, "
            "which can block connections between devices. Setup allows this receiver through "
            "Windows Firewall on private networks only.\n\n"
            "Settings controls resolution, stream FPS, bitrate, previews, camera lens and decoding. "
            "Apply settings automatically starts the armed phone camera. Right-click the PC preview "
            "to disable only its display, leaving the webcam running.\n\n"
            "This version sends video only. Bluetooth video is not supported. "
            "Enable PC control visibly once, then the phone can stream with its screen off. "
            "Stop sharing on the phone disarms remote control. On Redmi, allow background activity "
            "in the phone app's battery settings if the OS suspends it.")

    def close(self):
        self.stopping.set()
        self.frame_ready.set()
        if self.receiver:
            self.receiver.stop()
        if self.publisher:
            self.publisher.join(timeout=3)
        self.window.destroy()


def main():
    DATA.mkdir(parents=True, exist_ok=True)
    logging.basicConfig(filename=DATA / "receiver.log", level=logging.INFO,
                        format="%(asctime)s %(levelname)s %(message)s")
    window = tk.Tk()
    App(window)
    window.mainloop()
