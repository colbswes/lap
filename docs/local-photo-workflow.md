# Local Photo Workflow (Album Visibility & Apple Photos Import)

This personal fork adds two focused features to Lap so you can keep everyday photos separate from archives while keeping everything in one catalog.

## 1. Hide / Show Folders, Collections, Smart Collections, and Tags

- Hover any **Folder**, **Collection**, **Smart collection**, or **Tag** in the sidebar and click the eye icon (or open its `...` / right-click menu and choose **Hide** / **Show**).
- Hidden items stay dimmed in their sidebar list with a crossed-out eye icon so you can click directly into them or unhide them in one click, and **Show all** is available in the header menu whenever any item is hidden.
- Hidden items are excluded from Library counts, Smart collections, Search, Calendar, Tags, People, Locations, Cameras, Collections, and Map — and persist per library across restarts.
- You can also switch between **Tags**, **Collections**, and **Smart collections** from their menus (**Create smart collection**, **Save as collection**, **Add tag to photos…**, **Duplicate**).

## 2. Guided & CLI Apple Photos Import

### In-app on macOS

Open **Import from Apple Photos…** from the Welcome screen, the Library dropdown menu (top-left), or the Folders panel `...` menu:

- Choose a destination folder (defaults to `~/Pictures/Lap-Everyday`).
- Choose edited photo handling (`Edited if available, else original` by default, `Both`, or `Originals only`), Live Photo preservation, optional iCloud `--download-missing`, and optional sample filters (`Limit`, `From date`, `Album`, or custom `.photoslibrary`).
- Supports **Dry run** previews, live progress streaming, cancellation, a completion report, and automatic adding + scanning in Lap.
- **Exclude iCloud Shared Albums** is enabled by default so personal library exports stay clean; your Apple Shared Albums still appear in the collapsible **Apple Shared Albums** section at the bottom of the Folders sidebar, where clicking any shared album lets you browse its cached Apple Photos previews (0 extra disk space), open Apple Photos, or import just that album on demand.
- **Automatic Apple Photos deletion (macOS)**: enabled by default (**Settings → Advanced → Also delete exported photos from Apple Photos**). When you delete a photo in Lap that originated from an Apple Photos export folder (`.apple-photos-export.db`), Lap automatically looks up its Apple Photos UUID and moves it to Apple Photos' **Recently Deleted** album via PhotoKit (macOS asks to confirm). Deleting photos from non-Apple folders (like your Family Archive) never touches Apple Photos.

### From the terminal

Use `scripts/apple_photos_export.py` directly if you want to script or run the same incremental export from the command line.

It automatically uses `osxphotos` if installed, or falls back to `uvx osxphotos`.

### Test with a small sample first (recommended)

```bash
python3 scripts/apple_photos_export.py \
  --dest ~/Pictures/Lap-Everyday \
  --limit 50 \
  --dry-run
```

Remove `--dry-run` to export the sample:

```bash
python3 scripts/apple_photos_export.py \
  --dest ~/Pictures/Lap-Everyday \
  --limit 50
```

### Run full or incremental exports

```bash
python3 scripts/apple_photos_export.py \
  --dest ~/Pictures/Lap-Everyday
```

- **Edited vs. originals**: defaults to `--versions edited-prefer` (exports the edited version if edited, otherwise the original). Pass `--versions both` to keep both (`_edited` suffix), or `--versions originals` for originals only.
- **Live Photos**: image + `.mov` companions are exported into the same `YYYY/YYYY-MM` folder with matching stems so Lap pairs them automatically.
- **No duplicates, no clobbering Lap**: `--update` and `<dest>/.apple-photos-export.db` export only new or updated items on repeat runs. `--cleanup` is never used, and `.xmp` sidecars are disabled by default so tags, ratings, and edits made in Lap are never overwritten.
- **iCloud-optimized libraries**: pass `--download-missing` if originals are stored in iCloud.
- **Optional first-run metadata**: pass `--xmp-sidecar --album-keyword` on an initial export if you want Apple Photos keywords/album names exported to `.xmp` sidecars (omit on subsequent runs after editing metadata in Lap).

## 3. Running This Fork Locally

```bash
# 1. Start the Vite dev server + Tauri desktop app
cd src-vite && npx --yes pnpm@10 dev
# In another terminal:
cd src-tauri && cargo run
```

(Or build the release macOS app bundle with `npx --yes @tauri-apps/cli@2 build`.)
