from dataclasses import asdict, replace
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "desktop"))
from camerabridge.settings import Settings, load, save, validate, needs_reconnect


class SettingsTests(unittest.TestCase):
    def test_old_1080p_settings_migrate_without_losing_other_preferences(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.json"
            path.write_text('{"resolution":"1920x1080","fps":24,"mirror":true}', encoding="utf-8")
            self.assertEqual(load(path), Settings(fps=24, mirror=True))
        with self.assertRaises(ValueError):
            validate({"resolution":"1920x1080"})

    def test_settings_survive_restart(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.json"
            settings = Settings(resolution="960x540", fps=24, bitrate_mbps=1.5, preview_fps=15, decoder="cpu", mirror=True)
            save(path, settings)
            self.assertEqual(load(path), settings)
            self.assertFalse(path.with_suffix(".tmp").exists())

    def test_corrupt_settings_fall_back(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.json"
            path.write_text('{"fps": 9000}', encoding="utf-8")
            with self.assertLogs(level="WARNING"):
                self.assertEqual(load(path), Settings())

    def test_invalid_values_rejected(self):
        for key, value in (("fps", 0), ("fps", True), ("resolution", "8000x8000"),
                           ("preview_fps", -1), ("bitrate_mbps", float("nan")),
                           ("decoder", "unknown"), ("steady_fps", "true"),
                           ("phone_preview", "false"), ("lens", "unknown")):
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                validate({**asdict(Settings()), key: value})

    def test_local_settings_do_not_reconnect(self):
        before = Settings()
        self.assertFalse(needs_reconnect(before, replace(before, preview_fps=5, mirror=True)))
        self.assertTrue(needs_reconnect(before, replace(before, fps=15)))
        self.assertTrue(needs_reconnect(before, replace(before, decoder="cpu")))

    def test_preview_is_smooth_by_default(self):
        self.assertEqual(Settings().preview_fps, 30)

    def test_wire_settings_are_bounded_and_in_bits_per_second(self):
        self.assertEqual(Settings(bitrate_mbps=1.5).wire(), dict(width=1280, height=720, fps=30, bitrate=1500000,
                                                               steady_fps=True, phone_preview=True, lens="rear"))


if __name__ == "__main__": unittest.main()
