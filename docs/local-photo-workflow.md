# Local Photo Workflow (Collection Scopes & Apple Photos Import)

This personal fork adds two focused features to Lap so you can keep everyday photos separate from family archives while still searching across everything in one catalog.

## 1. Collection Scopes (`Everyday`, `Family Archive`, `Everything`)

- Use the **Everyday / Family / All** switcher at the top of the left sidebar to switch views at any time.
- Click the **filter button** next to the switcher (or right-click any folder root in **Folders**) to choose which root folders belong to **Everyday** and which belong to **Family Archive**.
- **Everything** always includes all root folders in the library without moving or duplicating files.
- The active scope applies consistently across Library counts, Folders, Smart Albums, Semantic Search, Calendar, Tags, People, Location, Camera, Collections, and Map — and persists per library across restarts.
- People stay in one shared catalog, so you never have to label the same person twice; switching scopes shows only the people and counts present in the active scope.
- Search shows the active scope in both the Search sidebar and the content header, with a one-click **Search Everything** action when you want a cross-collection search.

## 2. Guided & CLI Apple Photos Import

### In-app on macOS

Open **Import from Apple Photos…** from the Welcome screen, the Library dropdown menu (top-left), or the Folders panel `...` menu:

- Choose a destination folder (defaults to `~/Pictures/Lap-Everyday`) and target scope (**Everyday** or **Family Archive**).
- Choose edited photo handling (`Edited if available, else original` by default, `Both`, or `Originals only`), Live Photo preservation, optional iCloud `--download-missing`, and optional sample filters (`Limit`, `From date`, `Album`, or custom `.photoslibrary`).
- Supports **Dry run** previews, live progress streaming, cancellation, a completion report, and automatic adding + scanning in Lap.

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
