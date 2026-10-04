"""Tests for updater-json.py. Run: python3 scripts/release/test_updater_json.py"""

import datetime
import importlib.util
import pathlib
import tempfile
import unittest

_spec = importlib.util.spec_from_file_location(
    "updater_json", pathlib.Path(__file__).with_name("updater-json.py")
)
uj = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(uj)

NOW = datetime.datetime(2026, 10, 4, 12, 0, 0, 123, tzinfo=datetime.timezone.utc)


def make(folder, version, skip=()):
    for _, suffix in uj.ARTIFACTS:
        name = f"meet-ai-{version}-{suffix}"
        if name not in skip:
            (folder / name).write_bytes(b"x")
        if f"{name}.sig" not in skip:
            (folder / f"{name}.sig").write_text(f"sig-{suffix}\n")


class Build(unittest.TestCase):
    def test_all_platforms(self):
        with tempfile.TemporaryDirectory() as d:
            make(pathlib.Path(d), "1.2.3")
            m = uj.build("1.2.3", "v1.2.3", "o/r", d, "notes", NOW)
        self.assertEqual(m["version"], "1.2.3")
        self.assertEqual(m["pub_date"], "2026-10-04T12:00:00Z")
        self.assertEqual(
            sorted(m["platforms"]),
            [
                "darwin-aarch64",
                "darwin-aarch64-app",
                "linux-x86_64",
                "linux-x86_64-appimage",
                "windows-x86_64",
                "windows-x86_64-nsis",
            ],
        )
        win = m["platforms"]["windows-x86_64"]
        self.assertEqual(win["signature"], "sig-windows-x64-setup.exe")
        self.assertEqual(
            win["url"],
            "https://github.com/o/r/releases/download/v1.2.3/meet-ai-1.2.3-windows-x64-setup.exe",
        )
        self.assertTrue(m["platforms"]["linux-x86_64"]["url"].endswith(".AppImage"))
        self.assertNotIn("deb", str(m))

    def test_missing_sig_names_it(self):
        with tempfile.TemporaryDirectory() as d:
            make(pathlib.Path(d), "1.0.0", skip={"meet-ai-1.0.0-linux-x86_64.AppImage.sig"})
            with self.assertRaisesRegex(ValueError, "linux-x86_64.AppImage.sig"):
                uj.build("1.0.0", "v1.0.0", "o/r", d, now=NOW)

    def test_missing_artifact_names_it(self):
        with tempfile.TemporaryDirectory() as d:
            make(pathlib.Path(d), "1.0.0", skip={"meet-ai-1.0.0-macos-arm64.app.tar.gz"})
            with self.assertRaisesRegex(ValueError, "macos-arm64.app.tar.gz"):
                uj.build("1.0.0", "v1.0.0", "o/r", d, now=NOW)


if __name__ == "__main__":
    unittest.main()
