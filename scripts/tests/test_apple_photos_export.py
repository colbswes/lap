import json
import tempfile
import unittest
from pathlib import Path

from scripts.apple_photos_export import (
    DEFAULT_DIRECTORY_TEMPLATE,
    DEFAULT_EXPORT_DB_NAME,
    build_osxphotos_command,
    has_existing_xmp_sidecars,
    parse_osxphotos_report,
    resolve_osxphotos_runner,
)


class ApplePhotosExportTests(unittest.TestCase):
    def test_runner_prefers_osxphotos_then_uvx_then_uv(self):
        self.assertEqual(
            resolve_osxphotos_runner(lambda name: "/usr/local/bin/osxphotos" if name == "osxphotos" else None),
            ["/usr/local/bin/osxphotos"],
        )
        self.assertEqual(
            resolve_osxphotos_runner(lambda name: "/opt/homebrew/bin/uvx" if name == "uvx" else None),
            ["/opt/homebrew/bin/uvx", "--quiet", "osxphotos"],
        )
        self.assertEqual(
            resolve_osxphotos_runner(lambda name: "/opt/homebrew/bin/uv" if name == "uv" else None),
            ["/opt/homebrew/bin/uv", "tool", "run", "--quiet", "osxphotos"],
        )
        with self.assertRaises(RuntimeError):
            resolve_osxphotos_runner(lambda _name: None)

    def test_build_command_defaults_protect_lap_edits_and_live_photos(self):
        dest = Path("/tmp/lap-everyday")
        cmd = build_osxphotos_command(["osxphotos"], dest)
        self.assertEqual(cmd[:3], ["osxphotos", "export", str(dest)])
        self.assertIn("--update", cmd)
        self.assertIn("--skip-original-if-edited", cmd)
        self.assertIn("--touch-file", cmd)
        self.assertIn("--directory", cmd)
        self.assertEqual(cmd[cmd.index("--directory") + 1], DEFAULT_DIRECTORY_TEMPLATE)
        self.assertEqual(cmd[cmd.index("--exportdb") + 1], str(dest / DEFAULT_EXPORT_DB_NAME))
        self.assertNotIn("--cleanup", cmd)
        self.assertNotIn("--sidecar", cmd)
        self.assertNotIn("--skip-live", cmd)

    def test_build_command_modes_and_sample_filters(self):
        dest = Path("/tmp/lap-sample")
        cmd_both = build_osxphotos_command(
            ["uvx", "--quiet", "osxphotos"],
            dest,
            versions="both",
            albums=["Favorites", "Trips"],
            from_date="2025-01-01",
            to_date="2025-02-01",
            limit=25,
            download_missing=True,
            xmp_sidecar=True,
            album_keyword=True,
            dry_run=True,
        )
        self.assertIn("--edited-suffix", cmd_both)
        self.assertIn("_edited", cmd_both)
        self.assertIn("--from-date", cmd_both)
        self.assertIn("--to-date", cmd_both)
        self.assertIn("--limit", cmd_both)
        self.assertIn("25", cmd_both)
        self.assertIn("--download-missing", cmd_both)
        self.assertIn("--sidecar", cmd_both)
        self.assertIn("--sidecar-drop-ext", cmd_both)
        self.assertIn("--album-keyword", cmd_both)
        self.assertIn("--dry-run", cmd_both)
        self.assertEqual(cmd_both.count("--album"), 2)

        cmd_orig = build_osxphotos_command(["osxphotos"], dest, versions="originals", skip_live=True)
        self.assertIn("--skip-edited", cmd_orig)
        self.assertIn("--skip-live", cmd_orig)
        self.assertNotIn("--skip-original-if-edited", cmd_orig)

    def test_parse_report_and_existing_xmp_detection(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            self.assertFalse(has_existing_xmp_sidecars(root))

            nested = root / "2025" / "2025-01"
            nested.mkdir(parents=True)
            (nested / "IMG_0001.xmp").write_text("<x:xmpmeta/>", encoding="utf-8")
            self.assertTrue(has_existing_xmp_sidecars(root))

            report_path = root / "report.json"
            report_path.write_text(
                json.dumps(
                    [
                        {"filename": "IMG_0001.HEIC", "exported": True},
                        {"filename": "IMG_0001.MOV", "exported": True},
                        {"filename": "IMG_0002.HEIC", "updated": True},
                        {"filename": "IMG_0003.HEIC", "skipped": True},
                        {"filename": "IMG_0004.HEIC", "missing": True},
                        {"filename": "IMG_0005.HEIC", "error": "Permission denied"},
                    ]
                ),
                encoding="utf-8",
            )
            counts = parse_osxphotos_report(report_path)
            self.assertEqual(
                counts,
                {
                    "exported": 2,
                    "updated": 1,
                    "skipped": 1,
                    "missing": 1,
                    "errors": 1,
                    "live_photo_videos": 1,
                },
            )


if __name__ == "__main__":
    unittest.main()
