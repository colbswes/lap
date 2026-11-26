//! Apple Photos as a read-only library source.
//!
//! An album adopted from Apple Photos keeps its ordinary `afolders`/`afiles`
//! rows (so faces, tags, collections and embeddings stay attached), but its
//! paths live under the placeholder root [`ROOT`], which never exists on disk.
//! Every place that would read a file asks [`lookup`] first and gets pixels or
//! a playable path from PhotoKit instead. Nothing is copied out of the Photos
//! library: thumbnails land in Lap's thumbnail cache like any other source.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Placeholder root for Apple Photos-backed albums. It sits on the read-only
/// system volume, so a write that slips past a guard fails instead of landing
/// somewhere unexpected.
pub const ROOT: &str = "/.lap-apple-photos";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS photos_source_files (
        file_id INTEGER PRIMARY KEY REFERENCES afiles(id) ON DELETE CASCADE,
        asset_id TEXT NOT NULL,
        live_video INTEGER NOT NULL DEFAULT 0,
        modified INTEGER NOT NULL DEFAULT 0,
        -- 1 when the asset shows in Photos' default listing; hidden and shared assets are 0.
        listed INTEGER NOT NULL DEFAULT 1
    );
    CREATE INDEX IF NOT EXISTS idx_photos_source_files_asset ON photos_source_files(asset_id);";

fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    let has_listed: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('photos_source_files') WHERE name = 'listed')",
        [],
        |row| row.get(0),
    )?;
    if !has_listed {
        conn.execute_batch("ALTER TABLE photos_source_files ADD COLUMN listed INTEGER NOT NULL DEFAULT 1")?;
    }
    Ok(())
}

pub fn is_photos_path(path: &str) -> bool {
    path == ROOT || path.starts_with(&format!("{ROOT}/"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRef {
    pub file_id: i64,
    pub asset_id: String,
    pub live_video: bool,
    pub modified: i64,
}

fn lookup_in(conn: &Connection, path: &str) -> rusqlite::Result<Option<SourceRef>> {
    let path = Path::new(path);
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(None);
    };
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='photos_source_files')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    conn.query_row(
        "SELECT s.file_id, s.asset_id, s.live_video, s.modified
         FROM afiles a JOIN afolders f ON f.id = a.folder_id
         JOIN photos_source_files s ON s.file_id = a.id
         WHERE f.path = ?1 AND a.name = ?2",
        params![folder.to_string_lossy(), name.to_string_lossy()],
        |row| {
            Ok(SourceRef {
                file_id: row.get(0)?,
                asset_id: row.get(1)?,
                live_video: row.get::<_, i64>(2)? != 0,
                modified: row.get(3)?,
            })
        },
    )
    .optional()
}

/// Resolves a placeholder path in the current library to its Apple Photos asset.
pub fn lookup(path: &str) -> Option<SourceRef> {
    if !is_photos_path(path) {
        return None;
    }
    let conn = crate::t_sqlite::open_conn().ok()?;
    lookup_in(&conn, path).ok().flatten()
}

/// Modification stamp used for thumbnail cache keys of placeholder paths.
pub fn source_mtime(path: &str) -> Option<i64> {
    lookup(path).map(|source| source.modified)
}

#[cfg(target_os = "macos")]
mod native {
    use std::ffi::{c_char, CStr, CString};

    unsafe extern "C" {
        fn lap_photos_image(
            id: *const c_char,
            size: u32,
            network: bool,
            length: *mut usize,
            error: *mut *mut c_char,
        ) -> *mut u8;
        fn lap_photos_media_path(
            id: *const c_char,
            kind: i32,
            cache_dir: *const c_char,
            network: bool,
            error: *mut *mut c_char,
        ) -> *mut c_char;
        fn lap_photos_list(request: *const c_char, error: *mut *mut c_char) -> *mut c_char;
        fn lap_photos_revision() -> i64;
        fn lap_photos_shared_albums(error: *mut *mut c_char) -> *mut c_char;
        fn lap_photos_shared_album_assets(album: *const c_char, error: *mut *mut c_char) -> *mut c_char;
        fn lap_photos_update(request: *const c_char, error: *mut *mut c_char) -> bool;
        fn lap_photos_save_edit(id: *const c_char, jpeg_path: *const c_char, error: *mut *mut c_char) -> bool;
        fn lap_photos_create(path: *const c_char, paired: *const c_char, video: bool, error: *mut *mut c_char) -> *mut c_char;
        fn lap_free_string(ptr: *const c_char);
    }

    unsafe fn take_error(error: *mut c_char) -> Option<String> {
        if error.is_null() {
            return None;
        }
        let message = unsafe { CStr::from_ptr(error) }.to_string_lossy().into_owned();
        unsafe { lap_free_string(error) };
        Some(message)
    }

    unsafe fn take_string(value: *mut c_char) -> String {
        let text = unsafe { CStr::from_ptr(value) }.to_string_lossy().into_owned();
        unsafe { lap_free_string(value) };
        text
    }

    pub fn image(id: &str, size: u32, network: bool) -> Result<Option<Vec<u8>>, String> {
        let id = CString::new(id).map_err(|e| e.to_string())?;
        let mut length = 0usize;
        let mut error = std::ptr::null_mut();
        // The bridge returns malloc'd bytes that we copy and free.
        unsafe {
            let bytes = lap_photos_image(id.as_ptr(), size, network, &mut length, &mut error);
            if bytes.is_null() {
                return match take_error(error) {
                    Some(message) => Err(message),
                    None => Ok(None),
                };
            }
            let data = std::slice::from_raw_parts(bytes, length).to_vec();
            lap_free_string(bytes as *const c_char);
            Ok(Some(data))
        }
    }

    pub fn media_path(id: &str, live: bool, cache_dir: &str, network: bool) -> Result<Option<String>, String> {
        let id = CString::new(id).map_err(|e| e.to_string())?;
        let cache_dir = CString::new(cache_dir).map_err(|e| e.to_string())?;
        let mut error = std::ptr::null_mut();
        unsafe {
            let path = lap_photos_media_path(id.as_ptr(), live as i32, cache_dir.as_ptr(), network, &mut error);
            if path.is_null() {
                return match take_error(error) {
                    Some(message) => Err(message),
                    None => Ok(None),
                };
            }
            Ok(Some(take_string(path)))
        }
    }

    pub fn revision() -> i64 {
        // No arguments; returns a plain counter.
        unsafe { lap_photos_revision() }
    }

    pub fn shared_albums() -> Result<String, String> {
        let mut error = std::ptr::null_mut();
        unsafe {
            let json = lap_photos_shared_albums(&mut error);
            if json.is_null() {
                return Err(take_error(error).unwrap_or_else(|| "Apple Photos request failed.".into()));
            }
            Ok(take_string(json))
        }
    }

    pub fn shared_album_assets(album: &str) -> Result<String, String> {
        let album = CString::new(album).map_err(|e| e.to_string())?;
        let mut error = std::ptr::null_mut();
        unsafe {
            let json = lap_photos_shared_album_assets(album.as_ptr(), &mut error);
            if json.is_null() {
                return Err(take_error(error).unwrap_or_else(|| "Apple Photos request failed.".into()));
            }
            Ok(take_string(json))
        }
    }

    fn check(ok: bool, error: *mut c_char) -> Result<(), String> {
        if ok {
            return Ok(());
        }
        Err(unsafe { take_error(error) }.unwrap_or_else(|| "Apple Photos did not accept the change.".into()))
    }

    pub fn update(request_json: &str) -> Result<(), String> {
        let request = CString::new(request_json).map_err(|e| e.to_string())?;
        let mut error = std::ptr::null_mut();
        let ok = unsafe { lap_photos_update(request.as_ptr(), &mut error) };
        check(ok, error)
    }

    pub fn save_edit(id: &str, jpeg_path: &str) -> Result<(), String> {
        let id = CString::new(id).map_err(|e| e.to_string())?;
        let path = CString::new(jpeg_path).map_err(|e| e.to_string())?;
        let mut error = std::ptr::null_mut();
        let ok = unsafe { lap_photos_save_edit(id.as_ptr(), path.as_ptr(), &mut error) };
        check(ok, error)
    }

    pub fn create(path: &str, paired: Option<&str>, video: bool) -> Result<String, String> {
        let path = CString::new(path).map_err(|e| e.to_string())?;
        let paired = paired.map(CString::new).transpose().map_err(|e| e.to_string())?;
        let mut error = std::ptr::null_mut();
        unsafe {
            let id = lap_photos_create(
                path.as_ptr(),
                paired.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
                video,
                &mut error,
            );
            if id.is_null() {
                return Err(take_error(error).unwrap_or_else(|| "Apple Photos did not add the file.".into()));
            }
            Ok(take_string(id))
        }
    }

    pub fn list(request_json: &str) -> Result<String, String> {
        let request = CString::new(request_json).map_err(|e| e.to_string())?;
        let mut error = std::ptr::null_mut();
        unsafe {
            let json = lap_photos_list(request.as_ptr(), &mut error);
            if json.is_null() {
                return Err(take_error(error).unwrap_or_else(|| "Apple Photos request failed.".into()));
            }
            Ok(take_string(json))
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    const UNSUPPORTED: &str = "Apple Photos requires macOS.";
    pub fn image(_: &str, _: u32, _: bool) -> Result<Option<Vec<u8>>, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn media_path(_: &str, _: bool, _: &str, _: bool) -> Result<Option<String>, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn revision() -> i64 {
        -1
    }
    pub fn update(_: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub fn shared_albums() -> Result<String, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn shared_album_assets(_: &str) -> Result<String, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn save_edit(_: &str, _: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub fn create(_: &str, _: Option<&str>, _: bool) -> Result<String, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn list(_: &str) -> Result<String, String> {
        Err(UNSUPPORTED.into())
    }
}

/// Thumbnail pixels from Photos' local versions; never downloads from iCloud.
pub fn thumbnail_bytes(source: &SourceRef, size: u32) -> Result<Option<Vec<u8>>, String> {
    native::image(&source.asset_id, size, false)
}

/// Viewer-sized pixels; downloads the original from iCloud when needed.
pub fn preview_bytes(source: &SourceRef, size: u32) -> Result<Option<Vec<u8>>, String> {
    native::image(&source.asset_id, size, true)
}

fn live_cache_dir() -> Result<PathBuf, String> {
    Ok(crate::t_cache_storage::root()?.join("apple-photos-live"))
}

/// Keeps the Live Photo motion cache bounded; clips are re-fetched on demand.
fn prune_live_cache(dir: &Path, limit: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| (metadata.modified().ok(), metadata.len(), entry.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    files.sort_by_key(|(modified, _, _)| *modified);
    for (_, len, path) in files {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

/// A playable file for a video row (read in place inside the Photos library)
/// or a Live Photo motion row (cached clip). Downloads from iCloud when needed.
pub fn media_path(source: &SourceRef) -> Result<Option<PathBuf>, String> {
    // Video players issue many Range requests; resolve each asset once.
    static RESOLVED: std::sync::LazyLock<std::sync::Mutex<HashMap<(String, bool, i64), PathBuf>>> =
        std::sync::LazyLock::new(Default::default);
    let key = (source.asset_id.clone(), source.live_video, source.modified);
    if let Some(path) = RESOLVED.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
        if path.is_file() {
            return Ok(Some(path));
        }
    }
    let resolved = resolve_media_path(source)?;
    if let (Some(path), Ok(mut cache)) = (resolved.as_ref(), RESOLVED.lock()) {
        if cache.len() > 256 {
            cache.clear();
        }
        cache.insert(key, path.clone());
    }
    Ok(resolved)
}

fn resolve_media_path(source: &SourceRef) -> Result<Option<PathBuf>, String> {
    let dir = live_cache_dir()?;
    let path = native::media_path(&source.asset_id, source.live_video, &dir.to_string_lossy(), true)?;
    if source.live_video {
        prune_live_cache(&dir, 512 * 1024 * 1024);
    }
    Ok(path.map(PathBuf::from))
}

/// Moves the assets behind these placeholder paths to Apple Photos' Recently
/// Deleted (macOS asks the user to confirm) and returns the catalog rows whose
/// asset is gone afterwards, including Live Photo motion rows. The caller
/// removes those rows. Assets the user chose to keep are left untouched.
pub fn delete_from_photos(paths: &[String]) -> Result<Vec<i64>, String> {
    let sources: Vec<SourceRef> = paths.iter().filter_map(|path| lookup(path)).collect();
    let mut ids: Vec<String> = sources.iter().map(|source| source.asset_id.clone()).collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    crate::t_apple_photos::delete_apple_photos_assets_by_uuids(&ids)?;
    let remaining: std::collections::HashSet<String> = list_by_ids(&ids, false)?.into_iter().map(|a| a.id).collect();
    let gone: Vec<&String> = ids.iter().filter(|id| !remaining.contains(*id)).collect();
    let conn = crate::t_sqlite::open_conn()?;
    let mut file_ids = Vec::new();
    for id in gone {
        let mut stmt = conn
            .prepare("SELECT file_id FROM photos_source_files WHERE asset_id = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([id], |row| row.get(0)).map_err(|e| e.to_string())?;
        for row in rows {
            file_ids.push(row.map_err(|e| e.to_string())?);
        }
    }
    Ok(file_ids)
}

/// Placeholder paths of the still images read from Apple Photos, keyed by asset UUID.
pub(crate) fn still_paths_by_uuid(conn: &Connection) -> rusqlite::Result<HashMap<String, Vec<PathBuf>>> {
    let mut paths = HashMap::<String, Vec<PathBuf>>::new();
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='photos_source_files')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(paths);
    }
    let mut stmt = conn.prepare(
        "SELECT s.asset_id, f.path, a.name FROM photos_source_files s
         JOIN afiles a ON a.id = s.file_id JOIN afolders f ON f.id = a.folder_id
         WHERE s.live_video = 0 AND a.file_type <> 2",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?;
    for row in rows {
        let (asset_id, folder, name) = row?;
        let uuid = asset_id.split('/').next().unwrap_or_default().to_ascii_uppercase();
        paths.entry(uuid).or_default().push(Path::new(&folder).join(name));
    }
    Ok(paths)
}

/// Photos asset ids behind these catalog rows (deduplicated; Live Photo motion rows map to their photo).
fn asset_ids_for_files(file_ids: &[i64]) -> Result<Vec<String>, String> {
    let conn = crate::t_sqlite::open_conn()?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='photos_source_files')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for file_id in file_ids {
        if let Some(id) = conn
            .query_row("SELECT asset_id FROM photos_source_files WHERE file_id = ?1", [file_id], |row| row.get::<_, String>(0))
            .optional()
            .map_err(|e| e.to_string())?
        {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    Ok(ids)
}

/// A change Lap made to catalog rows that Apple Photos can store too.
#[derive(Default)]
pub struct AssetChange {
    pub favorite: Option<bool>,
    /// Some(None) clears the location.
    pub location: Option<Option<(f64, f64)>>,
}

/// Sends favorites and locations edited in Lap to Apple Photos in the background.
/// Rows that are not Apple Photos items are ignored.
pub fn push_change(file_ids: Vec<i64>, change: AssetChange) {
    if change.favorite.is_none() && change.location.is_none() {
        return;
    }
    let library = crate::t_config::current_library_id().unwrap_or_default();
    std::thread::spawn(move || {
        let result = crate::t_cmds::with_current_library(&library, || asset_ids_for_files(&file_ids)).and_then(|ids| {
            if ids.is_empty() {
                return Ok(());
            }
            let mut request = serde_json::json!({ "ids": ids });
            if let Some(favorite) = change.favorite {
                request["favorite"] = favorite.into();
            }
            if let Some(location) = change.location {
                request["location"] = location
                    .map(|(latitude, longitude)| serde_json::json!({ "latitude": latitude, "longitude": longitude }))
                    .unwrap_or(serde_json::Value::Null);
            }
            native::update(&request.to_string())
        });
        if let Err(error) = result {
            eprintln!("Could not update Apple Photos: {error}");
        }
    });
}

/// Full-resolution pixels of an Apple Photos item, upright, for the editor.
pub fn full_image_bytes(path: &str) -> Result<Vec<u8>, String> {
    let source = lookup(path).ok_or("Item not found in Apple Photos")?;
    preview_bytes(&source, 16384)?.ok_or_else(|| "Apple Photos could not load this item.".into())
}

/// Saves an edited JPEG as the item's current version in Apple Photos.
pub fn save_edit(path: &str, jpeg: &[u8]) -> Result<(), String> {
    let source = lookup(path).ok_or("Item not found in Apple Photos")?;
    let temp = std::env::temp_dir().join(format!("lap-photos-edit-{}.jpg", uuid::Uuid::new_v4().simple()));
    std::fs::write(&temp, jpeg).map_err(|e| e.to_string())?;
    let result = native::save_edit(&source.asset_id, &temp.to_string_lossy());
    let _ = std::fs::remove_file(&temp);
    result
}

/// Adds a local file (and its Live Photo motion file, if paired) to Apple
/// Photos, syncs it into the connected folder and returns its placeholder path.
pub fn add_to_photos(file_path: &str, paired_video: Option<&str>) -> Result<String, String> {
    let video = crate::t_utils::get_file_type(file_path) == Some(2);
    let asset_id = native::create(file_path, paired_video, video)?;
    let album_id = connected_album_id(&*crate::t_sqlite::open_conn()?)
        .map_err(|e| e.to_string())?
        .ok_or("Apple Photos is not connected.")?;
    sync_album(album_id)?;
    let conn = crate::t_sqlite::open_conn()?;
    conn.query_row(
        "SELECT f.path || '/' || a.name FROM photos_source_files s
         JOIN afiles a ON a.id = s.file_id JOIN afolders f ON f.id = a.folder_id
         WHERE s.asset_id = ?1 AND s.live_video = 0",
        [&asset_id],
        |row| row.get(0),
    )
    .map_err(|e| format!("Added to Apple Photos, but Lap could not find it yet: {e}"))
}

fn image_data_url(id: &str, size: u32, network: bool) -> Option<String> {
    use base64::Engine;
    let bytes = native::image(id, size, network).ok()??;
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[derive(Deserialize)]
struct SharedAlbumListing {
    id: String,
    title: String,
    count: usize,
    #[serde(rename = "coverId")]
    cover_id: Option<String>,
}

/// iCloud Shared Albums, with small cover images inlined as data URLs.
pub fn shared_albums(
    state: &crate::t_apple_photos::ApplePhotosExportState,
    force_refresh: bool,
) -> Result<Vec<crate::t_apple_photos::AppleSharedAlbum>, String> {
    if !force_refresh {
        if let Some(cached) = state.shared_albums_cache.lock().ok().and_then(|cache| cache.clone()) {
            return Ok(cached);
        }
    }
    let listing: Vec<SharedAlbumListing> = serde_json::from_str(&native::shared_albums()?).map_err(|e| e.to_string())?;
    let albums: Vec<_> = listing
        .into_iter()
        .map(|album| crate::t_apple_photos::AppleSharedAlbum {
            cover_path: album.cover_id.as_deref().and_then(|id| image_data_url(id, 160, false)),
            uuid: album.id,
            title: if album.title.is_empty() { "Untitled Shared Album".into() } else { album.title },
            owner: String::new(),
            count: album.count,
        })
        .collect();
    if let Ok(mut cache) = state.shared_albums_cache.lock() {
        *cache = Some(albums.clone());
    }
    Ok(albums)
}

/// Items of one Shared Album with grid-sized previews inlined as data URLs.
pub fn shared_album_photos(album_id: &str) -> Result<Vec<crate::t_apple_photos::AppleSharedPhoto>, String> {
    let assets: Vec<PhotosAsset> =
        serde_json::from_str(&native::shared_album_assets(album_id)?).map_err(|e| e.to_string())?;
    Ok(assets
        .into_iter()
        .map(|asset| crate::t_apple_photos::AppleSharedPhoto {
            preview_path: image_data_url(&asset.id, 360, false),
            original_path: None,
            filename: asset.filename,
            date: asset.created.and_then(|t| chrono::DateTime::from_timestamp(t, 0)).map(|t| t.to_rfc3339()),
            is_movie: asset.video,
            is_local_original: true,
            width: u32::try_from(asset.width).ok(),
            height: u32::try_from(asset.height).ok(),
            uuid: asset.id,
        })
        .collect())
}

/// A large preview of one Shared Album item, downloading from iCloud if needed.
pub fn shared_photo_preview(id: &str) -> Result<Option<String>, String> {
    Ok(image_data_url(id, 2048, true))
}

/// Refuses write operations on Apple Photos-backed paths.
pub fn deny_write(path: &str) -> Result<(), String> {
    if is_photos_path(path) {
        return Err("This item lives in Apple Photos. Edit, move or delete it in the Photos app.".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PhotosAsset {
    pub id: String,
    #[serde(default)]
    pub modified: i64,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub video: bool,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    #[serde(default)]
    pub altitude: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct Listing {
    limited: bool,
    assets: Vec<PhotosAsset>,
}

/// `everything` includes hidden and shared assets; `ids` restricts the listing.
fn list_assets(ids: Option<&[String]>, everything: bool, details: bool) -> Result<Listing, String> {
    let request = serde_json::json!({ "ids": ids, "everything": everything, "details": details });
    serde_json::from_str(&native::list(&request.to_string())?).map_err(|e| e.to_string())
}

fn list_by_ids(ids: &[String], details: bool) -> Result<Vec<PhotosAsset>, String> {
    let mut assets = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        assets.extend(list_assets(Some(chunk), true, details)?.assets);
    }
    Ok(assets)
}

/// The album that reads from Apple Photos in the current library, if any.
/// PhotoKit only exposes the System Photo Library, so there is at most one.
pub fn connected_album_id(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM albums WHERE path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/' ORDER BY id LIMIT 1",
        [ROOT],
        |row| row.get(0),
    )
    .optional()
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub album_id: i64,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
}

fn folder_for(conn: &Connection, album_id: i64, path: &str) -> rusqlite::Result<i64> {
    if let Some(id) = conn
        .query_row("SELECT id FROM afolders WHERE path = ?1", [path], |row| row.get(0))
        .optional()?
    {
        return Ok(id);
    }
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    conn.execute(
        "INSERT INTO afolders(album_id, name, path, is_favorite, has_subfolders) VALUES (?1, ?2, ?3, 0, 0)",
        params![album_id, name, path],
    )?;
    Ok(conn.last_insert_rowid())
}

fn unique_name(conn: &Connection, folder_id: i64, name: &str) -> rusqlite::Result<String> {
    let taken = |candidate: &str| -> rusqlite::Result<bool> {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM afiles WHERE folder_id = ?1 AND name = ?2 COLLATE NOCASE)",
            params![folder_id, candidate],
            |row| row.get(0),
        )
    };
    if !taken(name)? {
        return Ok(name.to_string());
    }
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 1.. {
        let candidate = format!("{stem} ({n}){ext}");
        if !taken(&candidate)? {
            return Ok(candidate);
        }
    }
    unreachable!()
}

fn insert_file(
    conn: &Connection,
    folder_id: i64,
    name: &str,
    file_type: i64,
    asset: &PhotosAsset,
    scan_time: i64,
) -> rusqlite::Result<i64> {
    let extension = Path::new(name).extension().map(|e| e.to_string_lossy().to_uppercase());
    let (geo_name, admin1, admin2, cc) = crate::t_sqlite::AFile::geocode(asset.latitude, asset.longitude);
    let created = asset.created.unwrap_or(asset.modified);
    let taken = Some(created);
    conn.execute(
        "INSERT INTO afiles(folder_id, name, name_pinyin, size, file_type, format_label, created_at, modified_at,
            taken_date, width, height, duration, is_favorite, rotate, e_orientation, gps_latitude, gps_longitude,
            gps_altitude, geo_name, geo_admin1, geo_admin2, geo_cc, last_scan_time)
         VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, 1, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            folder_id,
            name,
            crate::t_utils::natural_sort_key(&name.to_lowercase()),
            file_type,
            extension,
            created,
            asset.modified,
            taken,
            asset.width,
            asset.height,
            if file_type == 2 { Some(asset.duration.round() as i64) } else { None },
            asset.favorite as i64,
            asset.latitude,
            asset.longitude,
            asset.altitude,
            geo_name,
            admin1,
            admin2,
            cc,
            scan_time,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Adds assets Lap hasn't seen, refreshes edited ones, and removes rows whose
/// asset is gone from Apple Photos. Hidden and shared assets already in the
/// catalog stay; new ones are not added.
pub(crate) fn sync_album_in(
    conn: &mut Connection,
    album_id: i64,
    library: Vec<PhotosAsset>,
    limited: bool,
    details: impl Fn(&[String]) -> Result<Vec<PhotosAsset>, String>,
    still_present: impl Fn(&[String]) -> Result<Vec<String>, String>,
) -> Result<SyncReport, String> {
    let root: String = conn
        .query_row("SELECT path FROM albums WHERE id = ?1", [album_id], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    ensure_schema(conn).map_err(|e| e.to_string())?;
    let mut known = HashMap::<String, i64>::new();
    let mut unlisted = std::collections::HashSet::<String>::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT s.asset_id, s.modified, s.listed FROM photos_source_files s
                 JOIN afiles a ON a.id = s.file_id JOIN afolders f ON f.id = a.folder_id
                 WHERE f.album_id = ?1 AND s.live_video = 0",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([album_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, modified, listed) = row.map_err(|e| e.to_string())?;
            if listed == 0 {
                unlisted.insert(id.clone());
            }
            known.insert(id, modified);
        }
    }
    let mut report = SyncReport { album_id, ..Default::default() };
    let in_library: HashMap<&str, i64> = library.iter().map(|a| (a.id.as_str(), a.modified)).collect();
    let new_ids: Vec<String> = library.iter().filter(|a| !known.contains_key(&a.id)).map(|a| a.id.clone()).collect();
    let changed: Vec<(&str, i64)> = in_library
        .iter()
        .filter(|(id, modified)| known.get(**id).is_some_and(|old| old != *modified))
        .map(|(id, modified)| (*id, *modified))
        .collect();
    // Hidden and shared assets never show in the default listing, and PhotoKit
    // won't return hidden ones while the Hidden album is locked, so only assets
    // that were listed before can be judged deleted. A listed asset that
    // disappears may have just been hidden; ask PhotoKit directly first.
    let absent: Vec<String> = known
        .keys()
        .filter(|id| !in_library.contains_key(id.as_str()) && !unlisted.contains(*id))
        .cloned()
        .collect();
    let relisted: Vec<&String> = unlisted.iter().filter(|id| in_library.contains_key(id.as_str())).collect();
    let (gone, newly_unlisted): (Vec<String>, Vec<String>) = if limited || absent.is_empty() {
        (Vec::new(), Vec::new()) // Limited access hides assets that still exist; never treat that as deletion.
    } else {
        let present: std::collections::HashSet<String> = still_present(&absent)?.into_iter().collect();
        absent.into_iter().partition(|id| !present.contains(id))
    };
    let changed_ids: Vec<String> = changed.iter().map(|(id, _)| id.to_string()).collect();
    let mut fetched = details(&[new_ids.clone(), changed_ids].concat())?;
    let refreshed: Vec<PhotosAsset> = fetched.iter().filter(|a| known.contains_key(&a.id)).cloned().collect();
    fetched.retain(|a| !known.contains_key(&a.id));
    let added = fetched;
    let scan_time = chrono::Utc::now().timestamp_millis();

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for id in &relisted {
        tx.execute("UPDATE photos_source_files SET listed = 1 WHERE asset_id = ?1", [id]).map_err(|e| e.to_string())?;
    }
    for id in &newly_unlisted {
        tx.execute("UPDATE photos_source_files SET listed = 0 WHERE asset_id = ?1", [id]).map_err(|e| e.to_string())?;
    }
    for (id, modified) in &changed {
        tx.execute("UPDATE photos_source_files SET modified = ?1 WHERE asset_id = ?2", params![modified, id])
            .map_err(|e| e.to_string())?;
        report.updated += 1;
    }
    // Favorites and locations changed in Photos win over the catalog's copy.
    for asset in &refreshed {
        let (geo_name, admin1, admin2, cc) = crate::t_sqlite::AFile::geocode(asset.latitude, asset.longitude);
        tx.execute(
            "UPDATE afiles SET is_favorite = ?1, gps_latitude = ?2, gps_longitude = ?3, gps_altitude = ?4,
                geo_name = ?5, geo_admin1 = ?6, geo_admin2 = ?7, geo_cc = ?8
             WHERE id IN (SELECT file_id FROM photos_source_files WHERE asset_id = ?9 AND live_video = 0)",
            params![asset.favorite as i64, asset.latitude, asset.longitude, asset.altitude, geo_name, admin1, admin2, cc, asset.id],
        )
        .map_err(|e| e.to_string())?;
    }
    for id in &gone {
        tx.execute(
            "DELETE FROM afiles WHERE id IN (SELECT file_id FROM photos_source_files WHERE asset_id = ?1)",
            [id],
        )
        .map_err(|e| e.to_string())?;
        report.removed += 1;
    }
    for asset in &added {
        let created = chrono::DateTime::from_timestamp(asset.created.unwrap_or(asset.modified), 0)
            .map(|utc| utc.with_timezone(&chrono::Local))
            .unwrap_or_else(chrono::Local::now);
        let folder_path = format!("{root}/{}/{}", created.format("%Y"), created.format("%Y-%m"));
        let year_path = format!("{root}/{}", created.format("%Y"));
        folder_for(&tx, album_id, &root).map_err(|e| e.to_string())?;
        folder_for(&tx, album_id, &year_path).map_err(|e| e.to_string())?;
        let folder_id = folder_for(&tx, album_id, &folder_path).map_err(|e| e.to_string())?;
        let fallback = format!("{}.{}", asset.id.split('/').next().unwrap_or("photo"), if asset.video { "MOV" } else { "JPG" });
        let filename = if asset.filename.is_empty() { fallback } else { asset.filename.clone() };
        let name = unique_name(&tx, folder_id, &filename).map_err(|e| e.to_string())?;
        let file_id = insert_file(&tx, folder_id, &name, if asset.video { 2 } else { 1 }, asset, scan_time)
            .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO photos_source_files(file_id, asset_id, live_video, modified) VALUES (?1, ?2, 0, ?3)",
            params![file_id, asset.id, asset.modified],
        )
        .map_err(|e| e.to_string())?;
        if asset.live && !asset.video {
            let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let motion_name = unique_name(&tx, folder_id, &format!("{stem}.MOV")).map_err(|e| e.to_string())?;
            let motion = PhotosAsset { video: true, duration: 3.0, ..asset.clone() };
            let motion_id = insert_file(&tx, folder_id, &motion_name, 2, &motion, scan_time).map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO photos_source_files(file_id, asset_id, live_video, modified) VALUES (?1, ?2, 1, ?3)",
                params![motion_id, asset.id, asset.modified],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE afiles SET media_subtype = 'live_photo', live_photo_video_id = ?1 WHERE id = ?2",
                params![motion_id, file_id],
            )
            .map_err(|e| e.to_string())?;
        }
        report.added += 1;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(report)
}

/// Which of these assets still exist (possibly hidden). PhotoKit answers for
/// shared assets and for hidden ones only while the Hidden album is unlocked,
/// so Photos' own database decides the rest. If that can't be read, every
/// asset counts as present: Lap never removes what it can't confirm is gone.
fn assets_still_present(ids: &[String]) -> Result<Vec<String>, String> {
    let mut present: Vec<String> = list_by_ids(ids, false)?.into_iter().map(|a| a.id).collect();
    let unknown: Vec<&String> = ids.iter().filter(|id| !present.contains(id)).collect();
    if unknown.is_empty() {
        return Ok(present);
    }
    let alive = crate::t_apple_photos::default_photos_library_path()
        .map(|library| library.join("database/Photos.sqlite"))
        .and_then(|db| Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok())
        .and_then(|photos| {
            let mut stmt = photos.prepare("SELECT 1 FROM ZASSET WHERE ZUUID = ?1 AND ZTRASHEDSTATE = 0").ok()?;
            unknown
                .iter()
                .map(|id| {
                    let uuid = id.split('/').next().unwrap_or_default();
                    stmt.exists([uuid]).ok().map(|exists| (id.to_string(), exists))
                })
                .collect::<Option<Vec<_>>>()
        });
    match alive {
        Some(rows) => present.extend(rows.into_iter().filter(|(_, exists)| *exists).map(|(id, _)| id)),
        None => present.extend(unknown.into_iter().cloned()),
    }
    Ok(present)
}

fn sync_album(album_id: i64) -> Result<SyncReport, String> {
    let listing = list_assets(None, false, false)?;
    let mut conn = crate::t_sqlite::open_conn()?;
    let report = sync_album_in(
        &mut conn,
        album_id,
        listing.assets,
        listing.limited,
        |ids| list_by_ids(ids, true),
        assets_still_present,
    )?;
    drop(conn);
    if report.added + report.removed > 0 {
        crate::t_sqlite::Album::recount_album(album_id)?;
    }
    Ok(report)
}

fn emit_refresh(app: &tauri::AppHandle) {
    use tauri::Emitter;
    let _ = app.emit("library-folder-sync-finished", ());
    let _ = app.emit("library-total-refreshed", ());
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotosSourceStatus {
    pub album_id: Option<i64>,
}

#[tauri::command]
pub fn photos_source_status() -> Result<PhotosSourceStatus, String> {
    let conn = crate::t_sqlite::open_conn()?;
    Ok(PhotosSourceStatus { album_id: connected_album_id(&conn).map_err(|e| e.to_string())? })
}

/// Cheap change counter; the frontend syncs only when it moves.
#[tauri::command]
pub fn photos_source_revision() -> i64 {
    native::revision()
}

/// Connects the System Photo Library as a read-only album and imports its catalog.
#[tauri::command]
pub async fn photos_source_connect(app: tauri::AppHandle, name: String) -> Result<SyncReport, String> {
    let report = tauri::async_runtime::spawn_blocking(move || {
        let album_id = {
            let conn = crate::t_sqlite::open_conn()?;
            if let Some(id) = connected_album_id(&conn).map_err(|e| e.to_string())? {
                id
            } else {
                // Ask for Photos access before creating anything.
                list_assets(Some(&[]), false, false)?;
                let now = chrono::Utc::now().timestamp();
                conn.execute(
                    "INSERT INTO albums(name, path, created_at, modified_at, display_order_id, description, indexed, total,
                        skipped_count, skipped_size, failed_count, failed_size, merged_count, merged_size, last_scan_time, small_image_filter)
                     VALUES (?1, ?2, ?3, ?3, (SELECT COALESCE(MAX(display_order_id), 0) + 1 FROM albums), '', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)",
                    params![name, format!("{ROOT}/library"), now],
                )
                .map_err(|e| e.to_string())?;
                conn.last_insert_rowid()
            }
        };
        sync_album(album_id)
    })
    .await
    .map_err(|e| e.to_string())??;
    emit_refresh(&app);
    Ok(report)
}

/// Picks up additions, edits and deletions made in Apple Photos.
#[tauri::command]
pub async fn photos_source_sync(app: tauri::AppHandle) -> Result<Option<SyncReport>, String> {
    let report = tauri::async_runtime::spawn_blocking(move || {
        let album_id = connected_album_id(&*crate::t_sqlite::open_conn()?).map_err(|e| e.to_string())?;
        album_id.map(sync_album).transpose()
    })
    .await
    .map_err(|e| e.to_string())??;
    if report.as_ref().is_some_and(|r| r.added + r.updated + r.removed > 0) {
        emit_refresh(&app);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str, modified: i64) -> PhotosAsset {
        PhotosAsset {
            id: id.into(), modified, filename: format!("{}.HEIC", &id[..3]), created: Some(1_600_000_000),
            video: false, live: false, width: 4, height: 3, duration: 0.0, favorite: false,
            latitude: None, longitude: None, altitude: None,
        }
    }

    fn sync_library() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE albums(id INTEGER PRIMARY KEY, path TEXT);
             CREATE TABLE afolders(id INTEGER PRIMARY KEY, album_id INTEGER, name TEXT, path TEXT, is_favorite INTEGER, has_subfolders INTEGER);
             CREATE TABLE afiles(id INTEGER PRIMARY KEY, folder_id INTEGER REFERENCES afolders(id) ON DELETE CASCADE, name TEXT,
                 name_pinyin TEXT, size INTEGER, file_type INTEGER, format_label TEXT, created_at INTEGER, modified_at INTEGER,
                 taken_date INTEGER, width INTEGER, height INTEGER, duration INTEGER, is_favorite INTEGER, rotate INTEGER,
                 e_orientation INTEGER, gps_latitude REAL, gps_longitude REAL, gps_altitude REAL, geo_name TEXT, geo_admin1 TEXT,
                 geo_admin2 TEXT, geo_cc TEXT, last_scan_time INTEGER, media_subtype TEXT, live_photo_video_id INTEGER);
             INSERT INTO albums VALUES (1, '/.lap-apple-photos/1');",
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn sync_adds_new_assets_with_live_motion_and_refreshes_edits() {
        let mut conn = sync_library();
        let mut live = asset("LLL/L0/001", 5);
        live.live = true;
        let report = sync_album_in(&mut conn, 1, vec![asset("AAA/L0/001", 1), live.clone()], false,
            |ids| Ok(ids.iter().map(|id| if id.starts_with("LLL") { live.clone() } else { asset(id, 1) }).collect()),
            |_| Ok(Vec::new())).unwrap();
        assert_eq!((report.added, report.updated, report.removed), (2, 0, 0));
        let year = chrono::DateTime::from_timestamp(1_600_000_000, 0).unwrap().with_timezone(&chrono::Local);
        let folder = format!("/.lap-apple-photos/1/{}/{}", year.format("%Y"), year.format("%Y-%m"));
        let motion = lookup_in(&conn, &format!("{folder}/LLL.MOV")).unwrap().unwrap();
        assert!(motion.live_video);
        let linked: i64 = conn.query_row("SELECT live_photo_video_id FROM afiles WHERE name='LLL.HEIC'", [], |r| r.get(0)).unwrap();
        assert_eq!(linked, motion.file_id);

        // An edit in Photos (here: favoriting) refreshes the stamp and pulls the favorite back.
        let report = sync_album_in(&mut conn, 1, vec![asset("AAA/L0/001", 9), live], false,
            |ids| Ok(ids.iter().map(|id| PhotosAsset { favorite: true, ..asset(id, 9) }).collect()),
            |_| Ok(Vec::new())).unwrap();
        assert_eq!((report.added, report.updated), (0, 1));
        assert_eq!(lookup_in(&conn, &format!("{folder}/AAA.HEIC")).unwrap().unwrap().modified, 9);
        let favorite: i64 = conn.query_row("SELECT is_favorite FROM afiles WHERE name='AAA.HEIC'", [], |r| r.get(0)).unwrap();
        assert_eq!(favorite, 1);
    }

    #[test]
    fn sync_removes_only_assets_photos_no_longer_has() {
        let mut conn = sync_library();
        let all = vec![asset("AAA/L0/001", 1), asset("HID/L0/001", 1), asset("DEL/L0/001", 1)];
        let initial = all.clone();
        sync_album_in(&mut conn, 1, all, false, move |_| Ok(initial.clone()), |_| Ok(Vec::new())).unwrap();
        // HID is hidden (missing from the default listing but still present); DEL was deleted.
        let report = sync_album_in(&mut conn, 1, vec![asset("AAA/L0/001", 1)], false, |_| Ok(Vec::new()),
            |ids| Ok(ids.iter().filter(|id| id.starts_with("HID")).cloned().collect())).unwrap();
        assert_eq!(report.removed, 1);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM afiles", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
        // Once known to be hidden, it is never judged deleted again, even when PhotoKit can't see it.
        let report = sync_album_in(&mut conn, 1, vec![asset("AAA/L0/001", 1)], false, |_| Ok(Vec::new()),
            |_| Ok(Vec::new())).unwrap();
        assert_eq!(report.removed, 0);
        // Unhiding lists it again.
        sync_album_in(&mut conn, 1, vec![asset("AAA/L0/001", 1), asset("HID/L0/001", 1)], false,
            |_| Ok(Vec::new()), |_| Ok(Vec::new())).unwrap();
        let listed: i64 = conn.query_row("SELECT listed FROM photos_source_files WHERE asset_id='HID/L0/001'", [], |r| r.get(0)).unwrap();
        assert_eq!(listed, 1);
        // Limited access never deletes.
        let report = sync_album_in(&mut conn, 1, Vec::new(), true, |_| Ok(Vec::new()), |_| Ok(Vec::new())).unwrap();
        assert_eq!(report.removed, 0);
    }

    #[test]
    fn placeholder_paths_are_recognized_and_write_protected() {
        assert!(is_photos_path("/.lap-apple-photos/1/a.jpg"));
        assert!(!is_photos_path("/.lap-apple-photos-other/a.jpg"));
        assert!(deny_write("/.lap-apple-photos/1/a.jpg").is_err());
        assert!(deny_write("/Users/me/a.jpg").is_ok());
    }
}
