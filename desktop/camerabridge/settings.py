"""Validated, persistent settings shared with the Android stream handshake."""
from dataclasses import asdict, dataclass
import json
import logging
from pathlib import Path

RESOLUTIONS = ("640x360", "960x540", "1280x720")
FRAME_RATES = (15, 20, 24, 30)
PREVIEW_RATES = (0, 5, 15, 30)
BITRATES = (1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0)


@dataclass(frozen=True)
class Settings:
    resolution: str = "1280x720"
    fps: int = 30
    bitrate_mbps: float = 3.0
    preview_fps: int = 30
    decoder: str = "auto"
    steady_fps: bool = True
    mirror: bool = False
    phone_preview: bool = True
    lens: str = "rear"

    @property
    def size(self):
        return tuple(map(int, self.resolution.split("x")))

    def wire(self):
        width, height = self.size
        return dict(width=width, height=height, fps=self.fps,
                    bitrate=int(self.bitrate_mbps * 1_000_000), steady_fps=self.steady_fps,
                    phone_preview=self.phone_preview, lens=self.lens)


PRESETS = {
    "Balanced": Settings(),
    "Smooth on slower Wi-Fi": Settings(resolution="640x360", bitrate_mbps=1.5),
    "More detail": Settings(resolution="1280x720", bitrate_mbps=6.0),
    "Power saver": Settings(resolution="640x360", fps=15, bitrate_mbps=1.0, preview_fps=5),
}


def validate(values):
    if not isinstance(values, dict):
        raise ValueError("Settings must be an object")
    known = Settings.__dataclass_fields__
    result = Settings(**{key: value for key, value in values.items() if key in known})
    if result.resolution not in RESOLUTIONS:
        raise ValueError("Choose a supported resolution")
    if type(result.fps) is not int or result.fps not in FRAME_RATES:
        raise ValueError("Choose a supported stream frame rate")
    if type(result.preview_fps) is not int or result.preview_fps not in PREVIEW_RATES:
        raise ValueError("Choose a supported preview frame rate")
    if type(result.bitrate_mbps) not in (int, float) or result.bitrate_mbps not in BITRATES:
        raise ValueError("Choose a supported bitrate")
    if result.decoder not in ("auto", "cpu"):
        raise ValueError("Choose Auto or CPU decoding")
    if type(result.steady_fps) is not bool or type(result.mirror) is not bool or type(result.phone_preview) is not bool:
        raise ValueError("Invalid camera toggles")
    if result.lens not in ("rear", "front"):
        raise ValueError("Choose front or rear camera")
    return result


def load(path):
    try:
        values = json.loads(Path(path).read_text(encoding="utf-8"))
        if isinstance(values, dict) and values.get("resolution") == "1920x1080":
            values["resolution"] = "1280x720"
        return validate(values)
    except FileNotFoundError:
        return Settings()
    except (OSError, ValueError, TypeError):
        logging.warning("Could not read settings; using defaults", exc_info=True)
        return Settings()


def save(path, settings):
    validate(asdict(settings))
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(asdict(settings), indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def needs_reconnect(before, after):
    return before.wire() != after.wire() or before.decoder != after.decoder
