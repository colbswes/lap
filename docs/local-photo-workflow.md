# Local Photo Workflow (Album Visibility & Apple Photos)

This fork adds two features to Lap: hiding sidebar items so you can keep some photos out of everyday browsing while keeping everything in one catalog, and reading your Apple Photos library on macOS.

## 1. Hide / Show Folders, Collections, Smart Collections, and Tags

- Hover any **Folder**, **Collection**, **Smart collection**, or **Tag** in the sidebar and click the eye icon (or open its `...` / right-click menu and choose **Hide** / **Show**).
- Hidden items stay dimmed in their sidebar list with a crossed-out eye icon so you can click directly into them or unhide them in one click, and **Show all** is available in the header menu whenever any item is hidden.
- Hidden items are excluded from Library counts, Smart collections, Search, Calendar, Tags, People, Locations, Cameras, Collections, and Map — and persist per library across restarts.
- You can also switch between **Tags**, **Collections**, and **Smart collections** from their menus (**Create smart collection**, **Save as collection**, **Add tag to photos…**, **Duplicate**).

## 2. Apple Photos (macOS)

Choose **Connect Apple Photos** from the Library menu, the Folders `...` menu, or the Welcome screen. After macOS grants Photos access, Lap adds an **Apple Photos** folder that reads the System Photo Library through PhotoKit. Nothing is copied: thumbnails go into Lap's thumbnail cache, and full-size photos and videos load from Photos, which downloads iCloud originals on demand. Live Photo motion clips are the only cached media (up to 512 MB). PhotoKit only exposes the System Photo Library, so a Lap library connects to one Photos library at most.

Lap picks up photos added, edited or deleted in Photos within about 15 seconds while it is open, and catches up when it starts. Faces, tags, collections, events, search and the rest of Lap's analysis work on these items like any other.

- **Favorites and locations** set in Lap are written to Photos, and changes made in Photos flow back on the next sync.
- **Edits** from Lap's image editor are saved as a Photos edit, which you can revert in Photos. Saving a copy adds a new item to Photos.
- **Copy to the Apple Photos folder** adds a local photo or video (with its Live Photo motion, if paired) to Photos.
- **Delete** in Lap moves items to Photos' Recently Deleted after macOS asks you to confirm.
- **Rename, move and XMP fields** (title, caption, keywords) don't apply: Photos has no file names or folders, and PhotoKit offers no API for those fields.
- **Disconnect Apple Photos** (same menus, or the folder's `...` menu) removes the folder and Lap's catalog data for those items. Your photos stay in Apple Photos.

### Shared Albums

iCloud Shared Albums appear in the collapsible **Apple Shared Albums** section at the bottom of the Folders sidebar. Lap reads them through PhotoKit, shows previews, and loads a larger preview (from iCloud if needed) when you open an item.

## 3. Running This Fork Locally

```bash
# 1. Start the Vite dev server + Tauri desktop app
cd src-vite && npx --yes pnpm@10 dev
# In another terminal:
cd src-tauri && cargo run
```

(Or build the release macOS app bundle with `npx --yes @tauri-apps/cli@2 build`.)
