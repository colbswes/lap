#!/usr/bin/env python3
"""Repeatable one-way Apple Photos export helper for Lap.

Exports photos and videos from an Apple Photos library into a date-organized
folder tree that Lap can index as an Album (for example, inside the "Everyday"
collection scope).

Key behaviors:
- Uses `osxphotos export --update` with a persistent export database inside the
  destination folder so subsequent runs export only new or updated items without
  producing duplicates.
- Never passes `--cleanup`, and disables XMP sidecar generation by default, so
  tags, ratings, favorites, and folder organization done later in Lap are never
  overwritten or removed.
- Keeps Live Photo image + `.mov` companions together in the same folder with
  matching stems so Lap pairs them natively.
- Automatically uses `osxphotos` on PATH, or falls back to `uvx osxphotos`.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Sequence

DEFAULT_DIRECTORY_TEMPLATE = "{created.year}/{created.year}-{created.mm}"
DEFAULT_EXPORT_DB_NAME = ".apple-photos-export.db"
DEFAULT_REPORT_NAME = ".lap-apple-photos-report.json"
DEFAULT_RAW_REPORT_NAME = ".osxphotos-last-report.json"


@dataclass
class ExportSummary:
    destination: str
    versions: str
    dry_run: bool
    live_photos: bool
    xmp_sidecar: bool
    exported: int = 0
    updated: int = 0
    skipped: int = 0
    missing: int = 0
    errors: int = 0
    live_photo_videos: int = 0
    elapsed_seconds: float = 0.0
    command: list[str] | None = None
    exit_code: int = 0


def resolve_osxphotos_runner(
    which_fn=shutil.which,
) -> list[str]:
    """Return the command prefix used to invoke osxphotos."""
    osxphotos_bin = which_fn("osxphotos")
    if osxphotos_bin:
        return [osxphotos_bin]

    uvx_bin = which_fn("uvx")
    if uvx_bin:
        return [uvx_bin, "--quiet", "osxphotos"]

    uv_bin = which_fn("uv")
    if uv_bin:
        return [uv_bin, "tool", "run", "--quiet", "osxphotos"]

    raise RuntimeError(
        "Neither 'osxphotos' nor 'uvx'/'uv' was found on PATH. "
        "Install uv (`brew install uv`) or osxphotos (`pipx install osxphotos`)."
    )


def build_osxphotos_command(
    runner: Sequence[str],
    dest: Path,
    *,
    versions: str = "edited-prefer",
    library: Path | None = None,
    directory_template: str = DEFAULT_DIRECTORY_TEMPLATE,
    albums: Sequence[str] = (),
    from_date: str | None = None,
    to_date: str | None = None,
    limit: int | None = None,
    skip_live: bool = False,
    download_missing: bool = False,
    xmp_sidecar: bool = False,
    album_keyword: bool = False,
    dry_run: bool = False,
    export_db_path: Path | None = None,
    raw_report_path: Path | None = None,
) -> list[str]:
    """Build the exact `osxphotos export` command line."""
    if versions not in {"edited-prefer", "both", "originals"}:
        raise ValueError(f"Unsupported versions mode: {versions}")

    db_path = export_db_path or (dest / DEFAULT_EXPORT_DB_NAME)
    report_path = raw_report_path or (dest / DEFAULT_RAW_REPORT_NAME)

    cmd = [
        *runner,
        "export",
        str(dest),
        "--update",
        "--exportdb",
        str(db_path),
        "--directory",
        directory_template,
        "--touch-file",
        "--report",
        str(report_path),
    ]

    if versions == "edited-prefer":
        cmd.append("--skip-original-if-edited")
    elif versions == "originals":
        cmd.append("--skip-edited")
    elif versions == "both":
        cmd.extend(["--edited-suffix", "_edited"])

    if skip_live:
        cmd.append("--skip-live")

    if library is not None:
        cmd.extend(["--library", str(library)])

    for album in albums:
        album_name = album.strip()
        if album_name:
            cmd.extend(["--album", album_name])

    if from_date:
        cmd.extend(["--from-date", from_date])

    if to_date:
        cmd.extend(["--to-date", to_date])

    if limit is not None:
        if limit <= 0:
            raise ValueError("--limit must be a positive integer")
        cmd.extend(["--limit", str(limit)])

    if download_missing:
        cmd.append("--download-missing")

    if xmp_sidecar:
        cmd.extend(["--sidecar", "xmp", "--sidecar-drop-ext"])
        if album_keyword:
            cmd.append("--album-keyword")

    if dry_run:
        cmd.append("--dry-run")

    return cmd


def parse_osxphotos_report(report_path: Path) -> dict[str, int]:
    """Summarize counts from an osxphotos JSON export report."""
    counts = {
        "exported": 0,
        "updated": 0,
        "skipped": 0,
        "missing": 0,
        "errors": 0,
        "live_photo_videos": 0,
    }
    if not report_path.exists():
        return counts

    try:
        payload = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return counts

    if not isinstance(payload, list):
        return counts

    for entry in payload:
        if not isinstance(entry, dict):
            continue
        if entry.get("exported"):
            counts["exported"] += 1
        elif entry.get("updated"):
            counts["updated"] += 1
        elif entry.get("skipped"):
            counts["skipped"] += 1
        elif entry.get("missing"):
            counts["missing"] += 1

        if entry.get("error"):
            counts["errors"] += 1

        filename = str(entry.get("filename") or "")
        if filename.lower().endswith(".mov") and (
            entry.get("exported") or entry.get("updated")
        ):
            counts["live_photo_videos"] += 1

    return counts


def has_existing_xmp_sidecars(dest: Path) -> bool:
    """Return True if destination already contains any .xmp sidecar files."""
    if not dest.exists():
        return False
    for _, _, filenames in os.walk(dest):
        for name in filenames:
            if name.lower().endswith(".xmp"):
                return True
    return False


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Repeatable one-way Apple Photos export helper for Lap."
    )
    parser.add_argument(
        "--dest",
        required=True,
        type=Path,
        help="Destination folder that Lap indexes as an Album (e.g. ~/Pictures/Lap-Everyday).",
    )
    parser.add_argument(
        "--library",
        type=Path,
        default=None,
        help="Path to an Apple Photos .photoslibrary bundle (defaults to the system library).",
    )
    parser.add_argument(
        "--versions",
        choices=["edited-prefer", "both", "originals"],
        default="edited-prefer",
        help=(
            "How to handle edited photos: 'edited-prefer' exports the edited version when "
            "available and the original otherwise (default); 'both' exports original + _edited; "
            "'originals' exports originals only."
        ),
    )
    parser.add_argument(
        "--directory-template",
        default=DEFAULT_DIRECTORY_TEMPLATE,
        help=f"osxphotos --directory template (default: {DEFAULT_DIRECTORY_TEMPLATE}).",
    )
    parser.add_argument(
        "--album",
        action="append",
        default=[],
        help="Export only items in this Apple Photos album (may be passed multiple times).",
    )
    parser.add_argument(
        "--from-date",
        default=None,
        help="Export items taken on or after this date (e.g. 2025-01-01).",
    )
    parser.add_argument(
        "--to-date",
        default=None,
        help="Export items taken before this date (e.g. 2026-01-01).",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=None,
        help="Export at most N items (great for testing a small sample first).",
    )
    parser.add_argument(
        "--skip-live",
        action="store_true",
        help="Do not export Live Photo .mov companion videos.",
    )
    parser.add_argument(
        "--download-missing",
        action="store_true",
        help="Attempt to download missing iCloud-optimized originals during export.",
    )
    parser.add_argument(
        "--xmp-sidecar",
        action="store_true",
        help=(
            "Export Apple Photos metadata as .xmp sidecars (off by default to avoid "
            "overwriting ratings/tags edited inside Lap)."
        ),
    )
    parser.add_argument(
        "--album-keyword",
        action="store_true",
        help="When --xmp-sidecar is enabled, include Apple Photos album names as XMP keywords.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Preview what would be exported without writing photo files.",
    )
    return parser


def format_summary(summary: ExportSummary) -> str:
    mode_tag = " [DRY RUN]" if summary.dry_run else ""
    lines = [
        "",
        f"=== Apple Photos -> Lap Export Summary{mode_tag} ===",
        f"Destination:       {summary.destination}",
        f"Versions mode:     {summary.versions}",
        f"Live Photo pairs:  {'preserved' if summary.live_photos else 'skipped'}",
        f"XMP sidecars:      {'enabled' if summary.xmp_sidecar else 'disabled (preserves Lap edits)'}",
        f"Exported new:      {summary.exported}",
        f"Updated:           {summary.updated}",
        f"Skipped existing:  {summary.skipped}",
        f"Missing (iCloud):  {summary.missing}",
        f"Errors:            {summary.errors}",
        f"Elapsed:           {summary.elapsed_seconds:.1f}s",
    ]
    if not summary.dry_run and summary.exit_code == 0:
        lines.append(
            "Next step:         Open Lap, add/rescan this folder, and assign it to the Everyday scope."
        )
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_arg_parser()
    args = parser.parse_args(argv)

    dest: Path = args.dest.expanduser().resolve()
    library: Path | None = args.library.expanduser().resolve() if args.library else None

    if args.xmp_sidecar and has_existing_xmp_sidecars(dest):
        print(
            "Warning: destination already contains .xmp sidecar files. "
            "Passing --xmp-sidecar on repeat runs may overwrite edits made in Lap.",
            file=sys.stderr,
        )

    try:
        runner = resolve_osxphotos_runner()
    except RuntimeError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 2

    if not args.dry_run:
        dest.mkdir(parents=True, exist_ok=True)
    else:
        dest.mkdir(parents=True, exist_ok=True)

    raw_report_path = dest / DEFAULT_RAW_REPORT_NAME
    if raw_report_path.exists():
        try:
            raw_report_path.unlink()
        except OSError:
            pass

    cmd = build_osxphotos_command(
        runner,
        dest,
        versions=args.versions,
        library=library,
        directory_template=args.directory_template,
        albums=args.album,
        from_date=args.from_date,
        to_date=args.to_date,
        limit=args.limit,
        skip_live=args.skip_live,
        download_missing=args.download_missing,
        xmp_sidecar=args.xmp_sidecar,
        album_keyword=args.album_keyword,
        dry_run=args.dry_run,
        raw_report_path=raw_report_path,
    )

    print("Running:", " ".join(cmd))
    started = time.monotonic()
    completed = subprocess.run(cmd, check=False)
    elapsed = time.monotonic() - started

    counts = parse_osxphotos_report(raw_report_path)
    summary = ExportSummary(
        destination=str(dest),
        versions=args.versions,
        dry_run=bool(args.dry_run),
        live_photos=not args.skip_live,
        xmp_sidecar=bool(args.xmp_sidecar),
        exported=counts["exported"],
        updated=counts["updated"],
        skipped=counts["skipped"],
        missing=counts["missing"],
        errors=counts["errors"],
        live_photo_videos=counts["live_photo_videos"],
        elapsed_seconds=round(elapsed, 2),
        command=cmd,
        exit_code=completed.returncode,
    )

    report_out = dest / DEFAULT_REPORT_NAME
    try:
        report_out.write_text(
            json.dumps(asdict(summary), indent=2) + "\n",
            encoding="utf-8",
        )
    except OSError as exc:
        print(f"Warning: could not write summary report to {report_out}: {exc}", file=sys.stderr)

    print(format_summary(summary))
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
