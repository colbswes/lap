/**
 * Apple Photos helpers outside the library source: Apple-named pets and people
 * (Photos.sqlite, read-only), opening the Photos app, and moving assets to
 * Recently Deleted. The library and Shared Albums are read through PhotoKit;
 * see t_photos_source.rs.
 */

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

pub const DEFAULT_EXPORT_DB_NAME: &str = ".apple-photos-export.db";

/// Caches the Shared Album list between sidebar refreshes.
#[derive(Default)]
pub struct ApplePhotosExportState {
    pub shared_albums_cache: Mutex<Option<Vec<AppleSharedAlbum>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleSharedAlbum {
    pub uuid: String,
    pub title: String,
    pub owner: String,
    pub count: usize,
    pub cover_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleSharedPhoto {
    pub uuid: String,
    pub filename: String,
    pub date: Option<String>,
    pub is_movie: bool,
    pub is_local_original: bool,
    pub preview_path: Option<String>,
    pub original_path: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

pub fn default_export_destination() -> PathBuf {
    dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Apple Photos Export")
}

pub fn default_photos_library_path() -> Option<PathBuf> {
    dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .map(|p| p.join("Photos Library.photoslibrary"))
}

pub fn open_apple_photos_app() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-a", "Photos"])
            .spawn()
            .map_err(|e| format!("Failed to open Apple Photos: {e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Apple Photos is only available on macOS.".to_string())
    }
}

fn open_export_db_readonly(db_path: &Path) -> Result<rusqlite::Connection, rusqlite::Error> {
    if let Ok(conn) = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        return Ok(conn);
    }
    let escaped = db_path.to_string_lossy().replace('?', "%3F").replace('#', "%23");
    rusqlite::Connection::open_with_flags(
        format!("file:{escaped}?immutable=1"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn lap_delete_apple_photos_assets(
        uuids_json: *const std::ffi::c_char,
        error_out: *mut *mut std::ffi::c_char,
    ) -> std::ffi::c_int;
    fn lap_free_string(ptr: *const std::ffi::c_char);
}

#[cfg(target_os = "macos")]
pub fn delete_apple_photos_assets_by_uuids(uuids: &[String]) -> Result<usize, String> {
    if uuids.is_empty() {
        return Ok(0);
    }
    let json = serde_json::to_string(uuids).map_err(|e| e.to_string())?;
    let c_json = std::ffi::CString::new(json).map_err(|e| e.to_string())?;
    let mut err_ptr: *mut std::ffi::c_char = std::ptr::null_mut();

    let result = unsafe { lap_delete_apple_photos_assets(c_json.as_ptr(), &mut err_ptr) };
    if result < 0 {
        let msg = if !err_ptr.is_null() {
            let text = unsafe { std::ffi::CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .into_owned();
            unsafe { lap_free_string(err_ptr) };
            text
        } else {
            "Failed to delete assets from Apple Photos.".to_string()
        };
        return Err(msg);
    }
    if !err_ptr.is_null() {
        unsafe { lap_free_string(err_ptr) };
    }
    Ok(result as usize)
}

#[cfg(not(target_os = "macos"))]
pub fn delete_apple_photos_assets_by_uuids(_uuids: &[String]) -> Result<usize, String> {
    Ok(0)
}


// ----------------------------------------------------------------------------
// Apple Photos pet identities as confirmed seeds
// ----------------------------------------------------------------------------

/// Outcome of importing Apple Photos' named pets.
#[derive(Debug, Default, Serialize)]
pub struct AppleIdentityImport {
    /// Lap detections confirmed with an Apple name.
    pub confirmed: usize,
    /// Distinct names imported.
    pub names: usize,
    /// Apple detections whose photo isn't in Lap (not exported / not indexed).
    pub missing_photo: usize,
    /// Apple detections with no matching Lap detection (run a face scan first).
    pub missing_detection: usize,
    /// Detections the user already confirmed as someone else (left unchanged).
    pub kept_user_choice: usize,
}

/// One named Apple detection: asset, identity, face/body geometry (top-left origin), kind.
struct AppleNamedDetection {
    uuid: String,
    /// Apple person (after merges); display names aren't unique ("Chris" x2).
    identity: i64,
    /// Full name when set, else the display name.
    name: String,
    /// Face center (top-left normalized), when `ZSIZE > 0`.
    face_center: Option<(f32, f32)>,
    /// Face diameter normalized to `max(width, height)`, when `ZSIZE > 0`.
    face_size: Option<f32>,
    /// Body center (top-left normalized), when `ZBODYWIDTH > 0 && ZBODYHEIGHT > 0`.
    body_center: Option<(f32, f32)>,
    /// Body bounding box `(x, y, width, height)` (top-left normalized).
    body_box: Option<(f32, f32, f32, f32)>,
    /// Species inferred from `ZDETECTIONTYPE` (3 = dog, 4 = cat).
    species: &'static str,
    kind: crate::t_sqlite::PersonKind,
}

impl AppleNamedDetection {
    /// Pixel bounding box `(x, y, w, h)` in upright image dimensions, preferring
    /// Apple's full-body box and falling back to a square around the face center.
    fn pixel_box(&self, width: f32, height: f32) -> Option<(f32, f32, f32, f32)> {
        if let Some((x, y, w, h)) = self.body_box {
            let px = (x * width).clamp(0.0, width);
            let py = (y * height).clamp(0.0, height);
            let pw = (w * width).min(width - px);
            let ph = (h * height).min(height - py);
            if pw >= 1.0 && ph >= 1.0 {
                return Some((px, py, pw, ph));
            }
        }
        if let (Some((cx, cy)), Some(size)) = (self.face_center, self.face_size) {
            let side = size * width.max(height);
            let px = (cx * width - side / 2.0).clamp(0.0, width);
            let py = (cy * height - side / 2.0).clamp(0.0, height);
            let pw = (cx * width + side / 2.0).min(width) - px;
            let ph = (cy * height + side / 2.0).min(height) - py;
            if pw >= 1.0 && ph >= 1.0 {
                return Some((px, py, pw, ph));
            }
        }
        None
    }
}

/// Apple's named detections from the Photos library database (read-only).
fn read_apple_named_detections(photos_db: &Path) -> Result<Vec<AppleNamedDetection>, String> {
    let escaped = photos_db.to_string_lossy().replace('?', "%3F").replace('#', "%23");
    let conn = rusqlite::Connection::open_with_flags(
        format!("file:{escaped}?mode=ro&immutable=1"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| format!("Cannot open Photos library database: {e}"))?;

    let mut merge = std::collections::HashMap::new();
    let mut names = std::collections::HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT Z_PK, ZMERGETARGETPERSON, COALESCE(NULLIF(TRIM(ZFULLNAME), ''), NULLIF(TRIM(ZDISPLAYNAME), '')) FROM ZPERSON")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<String>>(2)?)))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (pk, target, name) = row.map_err(|e| e.to_string())?;
            if let Some(target) = target {
                merge.insert(pk, target);
            }
            if let Some(name) = name {
                names.insert(pk, name);
            }
        }
    }
    let resolve = |mut pk: i64| {
        for _ in 0..16 {
            match merge.get(&pk) {
                Some(&next) if next != pk => pk = next,
                _ => break,
            }
        }
        pk
    };

    // Detection types: 1 = human face; 3 = dog, 4 = cat. Only pets are imported:
    // Apple's pet identities seed Lap's pet recognition. Include body-only pet
    // detections (ZSIZE = 0, ZBODYWIDTH > 0) as well as face detections.
    let mut stmt = conn
        .prepare(
            "SELECT f.ZDETECTIONTYPE, f.ZPERSONFORFACE, a.ZUUID,
                    COALESCE(f.ZCENTERX, 0.0), COALESCE(f.ZCENTERY, 0.0), COALESCE(f.ZSIZE, 0.0),
                    COALESCE(f.ZBODYCENTERX, 0.0), COALESCE(f.ZBODYCENTERY, 0.0),
                    COALESCE(f.ZBODYWIDTH, 0.0), COALESCE(f.ZBODYHEIGHT, 0.0)
             FROM ZDETECTEDFACE f JOIN ZASSET a ON a.Z_PK = f.ZASSETFORFACE
             WHERE f.ZPERSONFORFACE IS NOT NULL
               AND (COALESCE(f.ZSIZE, 0.0) > 0
                    OR (COALESCE(f.ZBODYWIDTH, 0.0) > 0 AND COALESCE(f.ZBODYHEIGHT, 0.0) > 0))
               AND f.ZDETECTIONTYPE IN (3, 4)",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
                r.get::<_, f64>(6)?,
                r.get::<_, f64>(7)?,
                r.get::<_, f64>(8)?,
                r.get::<_, f64>(9)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (dtype, person, uuid, cx, cy, size, bcx, bcy, bw, bh) = row.map_err(|e| e.to_string())?;
        let identity = resolve(person);
        let Some(name) = names.get(&identity) else { continue };
        // Apple stores centers with a bottom-left origin.
        let face_center = if size > 0.0 { Some((cx as f32, 1.0 - cy as f32)) } else { None };
        let face_size = if size > 0.0 { Some(size as f32) } else { None };
        let (body_center, body_box) = if bw > 0.0 && bh > 0.0 {
            let top_cy = 1.0 - bcy;
            let x = (bcx - bw / 2.0).clamp(0.0, 1.0) as f32;
            let y = (top_cy - bh / 2.0).clamp(0.0, 1.0) as f32;
            let w = (bw as f32).min(1.0 - x);
            let h = (bh as f32).min(1.0 - y);
            (Some((bcx as f32, top_cy as f32)), Some((x, y, w, h)))
        } else {
            (None, None)
        };
        out.push(AppleNamedDetection {
            uuid: uuid.trim().to_ascii_uppercase(),
            identity,
            name: name.clone(),
            face_center,
            face_size,
            body_center,
            body_box,
            species: if dtype == 4 { "cat" } else { "dog" },
            kind: crate::t_sqlite::PersonKind::Pet,
        });
    }
    Ok(out)
}

fn match_apple_detection(
    conn: &rusqlite::Connection,
    file: (i64, f32, f32),
    det: &AppleNamedDetection,
) -> Result<Option<(i64, Option<i64>, bool)>, String> {
    if let Some(pt) = det.face_center {
        if let Some(hit) = crate::t_sqlite::Face::detection_at_with_conn(conn, file, det.kind, pt)? {
            return Ok(Some(hit));
        }
    }
    if let Some(pt) = det.body_center {
        if let Some(hit) = crate::t_sqlite::Face::detection_at_with_conn(conn, file, det.kind, pt)? {
            return Ok(Some(hit));
        }
    }
    Ok(None)
}

/// Detect pets on demand for an Apple-named-pet photo that has not been scanned
/// for pets yet (or where Apple Vision missed a close-up), storing new `pet`
/// detections and returning a match for `det` if found.
fn detect_or_seed_apple_pet(
    conn: &rusqlite::Connection,
    app_handle: &AppHandle,
    face_state: &crate::t_face::FaceState,
    scanned_files: &mut std::collections::HashSet<i64>,
    file: (i64, f32, f32),
    file_path: &Path,
    det: &AppleNamedDetection,
) -> Result<Option<(i64, Option<i64>, bool)>, String> {
    let (file_id, width, height) = file;
    let path_str = file_path.to_string_lossy();
    let Some(image_bytes) = crate::t_face::decode_image_for_faces(file_id, &path_str) else {
        return Ok(None);
    };

    let mut engine = face_state.0.lock().map_err(|_| "Face engine lock poisoned".to_string())?;
    if !engine.is_loaded() {
        engine.load_models(app_handle)?;
    }

    // 1. Run Apple Vision pet detection once per file (handles older photos scanned
    //    before pet detection existed, without needing a full library rescan).
    if scanned_files.insert(file_id) {
        if let Ok((mut pets, (proc_w, proc_h))) = engine.detect_pets_from_bytes(&image_bytes) {
            if proc_w > 0 && proc_h > 0 && !pets.is_empty() {
                let scale_x = width / proc_w as f32;
                let scale_y = height / proc_h as f32;
                for pet in &mut pets {
                    pet.bbox.x *= scale_x;
                    pet.bbox.y *= scale_y;
                    pet.bbox.width *= scale_x;
                    pet.bbox.height *= scale_y;
                }
                let existing = crate::t_sqlite::Face::bboxes_for_file_with_conn(conn, file_id)
                    .unwrap_or_default();
                let mut added_any = false;
                for pet in &pets {
                    let b = &pet.bbox;
                    let overlaps_existing = existing.iter().any(|(kind, (x, y, w, h))| {
                        kind == "pet"
                            && crate::t_face::box_iou((b.x, b.y, b.width, b.height), (*x, *y, *w, *h))
                                >= crate::t_face::RESCAN_SAME_FACE_IOU
                    });
                    if overlaps_existing {
                        continue;
                    }
                    let bbox_json = serde_json::json!({
                        "x": b.x,
                        "y": b.y,
                        "width": b.width,
                        "height": b.height,
                        "confidence": b.confidence,
                    })
                    .to_string();
                    if crate::t_sqlite::Face::add_with_conn(
                        conn,
                        file_id,
                        &bbox_json,
                        &pet.embedding,
                        pet.low_quality,
                        "pet",
                        pet.species,
                    )
                    .is_ok()
                    {
                        added_any = true;
                        let _ = crate::t_sqlite::Face::delete_overlapping_unassigned_face_false_positives_with_conn(
                            conn,
                            file_id,
                            (b.x, b.y, b.width, b.height),
                        );
                    }
                }
                if added_any {
                    let _ = crate::t_sqlite::Face::mark_scanned_with_conn(conn, file_id, 1);
                }
            }
        }
    }

    if let Some(hit) = match_apple_detection(conn, file, det)? {
        return Ok(Some(hit));
    }

    // 2. Vision occasionally misses close-up pet faces or curled-up cats that Apple Photos
    //    tagged; embed Apple's bounding box directly with the pet model as a fallback.
    let Some((px, py, pw, ph)) = det.pixel_box(width, height) else {
        return Ok(None);
    };
    let norm_box = (px / width, py / height, pw / width, ph / height);
    let Ok(embedding) = engine.embed_pet_box_from_bytes(&image_bytes, norm_box) else {
        return Ok(None);
    };
    drop(engine);

    let bbox_json = serde_json::json!({
        "x": px,
        "y": py,
        "width": pw,
        "height": ph,
        "confidence": 1.0_f32,
    })
    .to_string();
    let face_id = crate::t_sqlite::Face::add_with_conn(
        conn,
        file_id,
        &bbox_json,
        &embedding,
        false,
        "pet",
        Some(det.species),
    )?;
    let _ = crate::t_sqlite::Face::delete_overlapping_unassigned_face_false_positives_with_conn(
        conn,
        file_id,
        (px, py, pw, ph),
    );
    let _ = crate::t_sqlite::Face::mark_scanned_with_conn(conn, file_id, 1);
    Ok(Some((face_id, None, false)))
}

/// Confirm Lap's pets using the pet names given in Apple Photos.
///
/// Matches each named Apple pet detection to the Lap pet detection whose box
/// contains Apple's face or body center, scanning Apple-tagged pet photos on
/// demand if they haven't been scanned for pets yet. Reads the Photos database
/// directly; Photos does not need to be open.
pub fn import_apple_identities(
    app_handle: &AppHandle,
    face_state: &crate::t_face::FaceState,
) -> Result<AppleIdentityImport, String> {
    let photos_db = default_photos_library_path()
        .map(|p| p.join("database/Photos.sqlite"))
        .filter(|p| p.is_file())
        .ok_or("Apple Photos library not found")?;
    let mut conn = crate::t_sqlite::open_conn()?;
    let (result, updated_ids) = import_apple_identities_inner(
        &mut conn,
        &photos_db,
        &default_export_destination(),
        false,
        Some((app_handle, face_state)),
    )?;
    drop(conn);
    for pid in updated_ids {
        let _ = crate::t_sqlite::Person::update_thumbnail(pid);
    }
    Ok(result)
}

/// Import into `conn`; with `dry_run`, only counts what would be confirmed.
#[cfg(test)]
pub(crate) fn import_apple_identities_with(
    conn: &mut rusqlite::Connection,
    photos_db: &Path,
    export_dir: &Path,
    dry_run: bool,
    detector: Option<(&AppHandle, &crate::t_face::FaceState)>,
) -> Result<AppleIdentityImport, String> {
    let (result, _) = import_apple_identities_inner(conn, photos_db, export_dir, dry_run, detector)?;
    Ok(result)
}

fn import_apple_identities_inner(
    conn: &mut rusqlite::Connection,
    photos_db: &Path,
    export_dir: &Path,
    dry_run: bool,
    detector: Option<(&AppHandle, &crate::t_face::FaceState)>,
) -> Result<(AppleIdentityImport, Vec<i64>), String> {
    let detections = read_apple_named_detections(photos_db)?;
    // Candidate Lap paths per Apple Photos asset UUID: items read from Apple Photos,
    // plus files in a legacy osxphotos export folder when one exists.
    let mut paths = crate::t_photos_source::still_paths_by_uuid(conn).map_err(|e| e.to_string())?;
    let export_db = export_dir.join(DEFAULT_EXPORT_DB_NAME);
    if let Ok(export) = open_export_db_readonly(&export_db) {
        let mut stmt = export
            .prepare("SELECT uuid, filepath FROM export_data")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (uuid, rel) = row.map_err(|e| e.to_string())?;
            let lower = rel.to_ascii_lowercase();
            if lower.ends_with(".mov")
                || lower.ends_with(".mp4")
                || lower.ends_with(".aae")
                || lower.ends_with(".xmp")
            {
                continue;
            }
            let full = export_dir.join(&rel);
            let list = paths.entry(uuid.trim().to_ascii_uppercase()).or_default();
            if !list.iter().any(|existing| existing == &full) {
                list.push(full);
            }
        }
    }

    let mut result = AppleIdentityImport::default();
    let mut scanned_files: std::collections::HashSet<i64> = std::collections::HashSet::new();

    // Pass 1: match each Apple detection to a Lap detection.
    // (apple identity) -> [(face_id, current person, confirmed)]
    let total_detections = detections.len();
    if let Some((app_handle, _)) = detector {
        let _ = app_handle.emit(
            "apple_pet_import_progress",
            serde_json::json!({ "current": 0, "total": total_detections }),
        );
    }
    let mut matched: std::collections::HashMap<i64, Vec<(i64, Option<i64>, bool)>> = Default::default();
    let mut seen_hits: std::collections::HashSet<(i64, i64)> = Default::default();
    let mut identity_info: std::collections::HashMap<i64, (String, crate::t_sqlite::PersonKind)> = Default::default();
    for (idx, det) in detections.iter().enumerate() {
        if let Some((app_handle, _)) = detector {
            let _ = app_handle.emit(
                "apple_pet_import_progress",
                serde_json::json!({ "current": idx + 1, "total": total_detections }),
            );
        }
        let Some(rels) = paths.get(&det.uuid) else {
            result.missing_photo += 1;
            continue;
        };
        let mut lap_files: Vec<((i64, f32, f32), PathBuf)> = Vec::new();
        for full in rels {
            if let Some(file) = crate::t_sqlite::Face::file_for_path_with_conn(conn, full)? {
                lap_files.push((file, full.clone()));
            }
        }
        if lap_files.is_empty() {
            result.missing_photo += 1;
            continue;
        }

        let mut hit = None;
        for &(file, _) in &lap_files {
            if let Some(found) = match_apple_detection(conn, file, det)? {
                hit = Some(found);
                break;
            }
        }
        if hit.is_none() && !dry_run {
            if let Some((app_handle, face_state)) = detector {
                for &(file, ref full) in &lap_files {
                    if let Some(found) = detect_or_seed_apple_pet(
                        conn,
                        app_handle,
                        face_state,
                        &mut scanned_files,
                        file,
                        full,
                        det,
                    )? {
                        hit = Some(found);
                        break;
                    }
                }
            }
        }

        let Some(hit) = hit else {
            result.missing_detection += 1;
            continue;
        };
        identity_info.entry(det.identity).or_insert((det.name.clone(), det.kind));
        if seen_hits.insert((det.identity, hit.0)) {
            matched.entry(det.identity).or_default().push(hit);
        }
    }

    // Pass 2: pick the Lap person for each Apple identity. Prefer the Lap person the
    // user already confirmed for most of these faces (keeps Lap's naming), then a
    // Lap person with the same name, else create one.
    let mut person_ids: std::collections::HashMap<i64, i64> = Default::default();
    for (identity, hits) in &matched {
        let (name, kind) = &identity_info[identity];
        let mut votes: std::collections::HashMap<i64, usize> = Default::default();
        for &(_, current, confirmed) in hits {
            if let (Some(pid), true) = (current, confirmed) {
                *votes.entry(pid).or_default() += 1;
            }
        }
        let confirmed_total: usize = votes.values().sum();
        let majority = votes
            .iter()
            .max_by_key(|(_, n)| **n)
            .filter(|(_, n)| **n * 2 > confirmed_total)
            .map(|(&pid, _)| pid);
        let person_id = match majority {
            Some(pid) => pid,
            None => match crate::t_sqlite::Person::find_by_name_with_conn(conn, name, None, Some(*kind))? {
                Some(p) => p.id,
                None if dry_run => -(person_ids.len() as i64) - 1,
                None => crate::t_sqlite::Person::create_of_kind_with_conn(conn, Some(name), *kind)?,
            },
        };
        person_ids.insert(*identity, person_id);
        if dry_run {
            let lap_name: Option<String> = conn
                .query_row("SELECT name FROM persons WHERE id = ?1", [person_id], |r| r.get(0))
                .ok()
                .flatten();
            eprintln!("map: apple={} -> lap={:?} ({} faces)", name, lap_name, hits.len());
        }

        for &(face_id, current, confirmed) in hits {
            if confirmed && current.is_some_and(|c| c != person_id) {
                result.kept_user_choice += 1;
                if dry_run {
                    let lap_name: Option<String> = conn
                        .query_row("SELECT name FROM persons WHERE id = ?1", [current.unwrap()], |r| r.get(0))
                        .ok()
                        .flatten();
                    eprintln!("conflict: apple={} lap={:?}", name, lap_name);
                }
                continue;
            }
            if !(confirmed && current == Some(person_id)) && !dry_run {
                crate::t_sqlite::Face::assign_confirmed_with_conn(conn, face_id, person_id)?;
            }
            result.confirmed += 1;
        }
    }
    let mut updated_ids: Vec<i64> = if dry_run {
        Vec::new()
    } else {
        person_ids.values().copied().collect()
    };
    updated_ids.sort_unstable();
    updated_ids.dedup();
    result.names = person_ids.len();
    Ok((result, updated_ids))
}

#[cfg(test)]
mod identity_import_tests {
    use super::*;

    /// Dry run against a copy of the real library database.
    /// `LAP_DB_COPY=/path/copy.db cargo test --bin Lap apple_identity_dry_run -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn apple_identity_dry_run() {
        let mut conn = rusqlite::Connection::open(std::env::var("LAP_DB_COPY").unwrap()).unwrap();
        let photos_db = default_photos_library_path().unwrap().join("database/Photos.sqlite");
        let result = import_apple_identities_with(&mut conn, &photos_db, &default_export_destination(), true, None).unwrap();
        println!("{:?}", result);
    }

    #[test]
    fn import_handles_edited_export_paths_and_body_only_pet_detections() {
        let root = std::env::temp_dir().join(format!(
            "lap_apple_pet_import_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let export_dir = root.join("export");
        let month_dir = export_dir.join("2024").join("2024-11");
        std::fs::create_dir_all(&month_dir).unwrap();

        // Mock Apple Photos.sqlite with:
        //  - Goose (cat, dtype 4): body-only detection (ZSIZE=0) on an edited asset
        //  - Rory (dog, dtype 3): face detection on an unedited asset
        let photos_db = root.join("Photos.sqlite");
        let pconn = rusqlite::Connection::open(&photos_db).unwrap();
        pconn
            .execute_batch(
                "CREATE TABLE ZPERSON (Z_PK INTEGER PRIMARY KEY, ZMERGETARGETPERSON INTEGER, ZFULLNAME TEXT, ZDISPLAYNAME TEXT);
                 CREATE TABLE ZASSET (Z_PK INTEGER PRIMARY KEY, ZUUID TEXT);
                 CREATE TABLE ZDETECTEDFACE (
                     Z_PK INTEGER PRIMARY KEY,
                     ZDETECTIONTYPE INTEGER,
                     ZPERSONFORFACE INTEGER,
                     ZASSETFORFACE INTEGER,
                     ZCENTERX REAL, ZCENTERY REAL, ZSIZE REAL,
                     ZBODYCENTERX REAL, ZBODYCENTERY REAL, ZBODYWIDTH REAL, ZBODYHEIGHT REAL
                 );
                 INSERT INTO ZPERSON VALUES (1, NULL, 'Goose', 'Goose'), (2, NULL, 'Rory', 'Rory');
                 INSERT INTO ZASSET VALUES (10, 'UUID-GOOSE'), (11, 'UUID-RORY');
                 INSERT INTO ZDETECTEDFACE VALUES
                     (100, 4, 1, 10, 0.0, 0.0, 0.0, 0.5, 0.4, 0.4, 0.4),
                     (101, 3, 2, 11, 0.5, 0.5, 0.2, 0.5, 0.5, 0.4, 0.4);",
            )
            .unwrap();
        drop(pconn);

        // Mock .apple-photos-export.db where Goose's skipped original comes BEFORE the exported _edited file.
        let export_db = export_dir.join(DEFAULT_EXPORT_DB_NAME);
        let econn = rusqlite::Connection::open(&export_db).unwrap();
        econn
            .execute_batch(
                "CREATE TABLE export_data (id INTEGER PRIMARY KEY, uuid TEXT, filepath TEXT);
                 INSERT INTO export_data (uuid, filepath) VALUES
                     ('uuid-goose', '2024/2024-11/IMG_1000.HEIC'),
                     ('uuid-goose', '2024/2024-11/IMG_1000_edited.heic'),
                     ('UUID-RORY',  '2024/2024-11/IMG_2000.HEIC');",
            )
            .unwrap();
        drop(econn);

        // Mock Lap DB containing only IMG_1000_edited.heic and IMG_2000.HEIC.
        let mut lconn = rusqlite::Connection::open_in_memory().unwrap();
        lconn
            .execute_batch(
                "CREATE TABLE albums (id INTEGER PRIMARY KEY, path TEXT, file_types INTEGER, small_image_filter INTEGER, excluded_folders TEXT);
                 CREATE TABLE afolders (id INTEGER PRIMARY KEY, album_id INTEGER, path TEXT, is_excluded_from_search INTEGER);
                 CREATE TABLE afiles (id INTEGER PRIMARY KEY, folder_id INTEGER, name TEXT, size INTEGER, file_type INTEGER, taken_date INTEGER, is_favorite INTEGER, live_photo_video_id INTEGER, width INTEGER, height INTEGER, e_orientation INTEGER, has_faces INTEGER);
                 CREATE TABLE persons (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, kind TEXT NOT NULL DEFAULT 'person', thumbnail BLOB, cover_face_id INTEGER, created_at INTEGER);
                 CREATE TABLE faces (id INTEGER PRIMARY KEY AUTOINCREMENT, file_id INTEGER, bbox TEXT, embedding BLOB, person_id INTEGER, created_at INTEGER, confirmed INTEGER NOT NULL DEFAULT 0, low_quality INTEGER NOT NULL DEFAULT 0, kind TEXT NOT NULL DEFAULT 'face', species TEXT, video_ts INTEGER);
                 CREATE TABLE face_rejections (face_id INTEGER, person_id INTEGER, PRIMARY KEY (face_id, person_id));",
            )
            .unwrap();
        let export_str = export_dir.to_string_lossy().to_string();
        let folder_str = month_dir.to_string_lossy().to_string();
        lconn
            .execute("INSERT INTO albums VALUES (1, ?1, 7, NULL, '[]')", rusqlite::params![export_str])
            .unwrap();
        lconn
            .execute("INSERT INTO afolders VALUES (1, 1, ?1, 0)", rusqlite::params![folder_str])
            .unwrap();
        lconn
            .execute_batch(
                "INSERT INTO afiles VALUES
                     (1, 1, 'IMG_1000_edited.heic', 100, 1, 1700000000, 0, NULL, 1000, 1000, 1, 1),
                     (2, 1, 'IMG_2000.HEIC', 100, 1, 1700000001, 0, NULL, 1000, 1000, 1, 1);
                 INSERT INTO faces (id, file_id, bbox, kind) VALUES
                     (10, 1, '{\"x\":300,\"y\":400,\"width\":400,\"height\":400}', 'pet'),
                     (11, 2, '{\"x\":300,\"y\":300,\"width\":400,\"height\":400}', 'pet');",
            )
            .unwrap();

        let result = import_apple_identities_with(&mut lconn, &photos_db, &export_dir, false, None).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(result.confirmed, 2);
        assert_eq!(result.names, 2);
        assert_eq!(result.missing_photo, 0);
        assert_eq!(result.missing_detection, 0);

        let mut names: Vec<(String, i64)> = lconn
            .prepare("SELECT p.name, f.confirmed FROM faces f JOIN persons p ON p.id = f.person_id ORDER BY p.name")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        names.sort();
        assert_eq!(names, vec![("Goose".to_string(), 1), ("Rory".to_string(), 1)]);
    }
}
