/**
 * Guided one-way Apple Photos export via `osxphotos` (or `uvx osxphotos`).
 * Keeps exports incremental (--update), preserves Live Photo pairs, and avoids
 * overwriting Lap edits/XMP sidecars by default.
 */

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

pub const DEFAULT_DIRECTORY_TEMPLATE: &str = "{created.year}/{created.year}-{created.mm}";
pub const DEFAULT_EXPORT_DB_NAME: &str = ".apple-photos-export.db";
pub const DEFAULT_RAW_REPORT_NAME: &str = ".osxphotos-last-report.json";
pub const DEFAULT_SUMMARY_REPORT_NAME: &str = ".lap-apple-photos-report.json";

#[derive(Default)]
pub struct ApplePhotosExportState {
    active_pid: Mutex<Option<u32>>,
    cancelled: AtomicBool,
    shared_albums_cache: Mutex<Option<Vec<AppleSharedAlbum>>>,
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

impl ApplePhotosExportState {
    pub fn try_begin(&self) -> Result<(), String> {
        let guard = self.active_pid.lock().map_err(|_| "Export state lock poisoned".to_string())?;
        if guard.is_some() {
            return Err("An Apple Photos export is already running.".to_string());
        }
        self.cancelled.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn set_pid(&self, pid: Option<u32>) {
        if let Ok(mut guard) = self.active_pid.lock() {
            *guard = pid;
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) -> Result<bool, String> {
        self.cancelled.store(true, Ordering::SeqCst);
        let pid = self
            .active_pid
            .lock()
            .map_err(|_| "Export state lock poisoned".to_string())?
            .clone();
        if let Some(pid) = pid {
            #[cfg(unix)]
            {
                let _ = Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .status();
            }
            #[cfg(windows)]
            {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .status();
            }
            return Ok(true);
        }
        Ok(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplePhotosToolStatus {
    pub available: bool,
    pub runner_label: Option<String>,
    pub default_dest: String,
    pub default_library_exists: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplePhotosExportRequest {
    pub dest: String,
    #[serde(default = "default_versions")]
    pub versions: String,
    #[serde(default)]
    pub library: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub from_date: Option<String>,
    #[serde(default)]
    pub to_date: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub skip_live: bool,
    #[serde(default = "default_true")]
    pub exclude_shared: bool,
    #[serde(default)]
    pub download_missing: bool,
    #[serde(default)]
    pub xmp_sidecar: bool,
    #[serde(default)]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

fn default_versions() -> String {
    "edited-prefer".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplePhotosProgressEvent {
    pub line: String,
    pub stream: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApplePhotosExportResult {
    pub destination: String,
    pub versions: String,
    pub dry_run: bool,
    pub cancelled: bool,
    pub exit_code: i32,
    pub exported: usize,
    pub updated: usize,
    pub skipped: usize,
    pub missing: usize,
    pub errors: usize,
    pub live_photo_videos: usize,
    pub elapsed_seconds: f64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerSpec {
    pub program: PathBuf,
    pub prefix_args: Vec<String>,
    pub label: String,
}

fn augmented_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(path_var) = std::env::var("PATH") {
        dirs.extend(std::env::split_paths(&path_var));
    }
    let home = dirs::home_dir();
    let extras = [
        Some(PathBuf::from("/opt/homebrew/bin")),
        Some(PathBuf::from("/usr/local/bin")),
        home.as_ref().map(|h| h.join(".local/bin")),
        home.as_ref().map(|h| h.join(".cargo/bin")),
        home.as_ref().map(|h| h.join(".pyenv/shims")),
    ];
    for candidate in extras.into_iter().flatten() {
        if !dirs.iter().any(|existing| existing == &candidate) {
            dirs.push(candidate);
        }
    }
    dirs
}

fn find_executable_in_dirs(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn resolve_osxphotos_runner() -> Result<RunnerSpec, String> {
    let dirs = augmented_search_dirs();
    if let Some(bin) = find_executable_in_dirs("osxphotos", &dirs) {
        return Ok(RunnerSpec {
            program: bin,
            prefix_args: Vec::new(),
            label: "osxphotos".to_string(),
        });
    }
    if let Some(bin) = find_executable_in_dirs("uvx", &dirs) {
        return Ok(RunnerSpec {
            program: bin,
            prefix_args: vec!["--quiet".to_string(), "osxphotos".to_string()],
            label: "uvx osxphotos".to_string(),
        });
    }
    if let Some(bin) = find_executable_in_dirs("uv", &dirs) {
        return Ok(RunnerSpec {
            program: bin,
            prefix_args: vec![
                "tool".to_string(),
                "run".to_string(),
                "--quiet".to_string(),
                "osxphotos".to_string(),
            ],
            label: "uv tool run osxphotos".to_string(),
        });
    }
    Err("Could not find 'osxphotos' or 'uvx'/'uv'. Install uv (`brew install uv`) or osxphotos (`pipx install osxphotos`).".to_string())
}

fn augmented_path_env() -> Option<std::ffi::OsString> {
    let dirs = augmented_search_dirs();
    std::env::join_paths(dirs).ok()
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

pub fn check_tool_status() -> ApplePhotosToolStatus {
    let default_dest = default_export_destination().to_string_lossy().to_string();
    let default_library_exists = default_photos_library_path()
        .map(|p| p.exists())
        .unwrap_or(false);

    match resolve_osxphotos_runner() {
        Ok(runner) => ApplePhotosToolStatus {
            available: true,
            runner_label: Some(runner.label),
            default_dest,
            default_library_exists,
            error: None,
        },
        Err(err) => ApplePhotosToolStatus {
            available: false,
            runner_label: None,
            default_dest,
            default_library_exists,
            error: Some(err),
        },
    }
}

pub fn expand_user_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if trimmed == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(trimmed)
}

pub fn build_export_args(
    request: &ApplePhotosExportRequest,
    dest: &Path,
    raw_report_path: &Path,
) -> Result<Vec<String>, String> {
    let versions = request.versions.trim();
    if !matches!(versions, "edited-prefer" | "both" | "originals") {
        return Err(format!("Unsupported versions mode: {versions}"));
    }

    let db_path = dest.join(DEFAULT_EXPORT_DB_NAME);
    let mut args = vec![
        "export".to_string(),
        dest.to_string_lossy().to_string(),
        "--update".to_string(),
        "--exportdb".to_string(),
        db_path.to_string_lossy().to_string(),
        "--directory".to_string(),
        DEFAULT_DIRECTORY_TEMPLATE.to_string(),
        "--touch-file".to_string(),
        "--report".to_string(),
        raw_report_path.to_string_lossy().to_string(),
    ];

    match versions {
        "edited-prefer" => args.push("--skip-original-if-edited".to_string()),
        "originals" => args.push("--skip-edited".to_string()),
        "both" => {
            args.push("--edited-suffix".to_string());
            args.push("_edited".to_string());
        }
        _ => {}
    }

    if request.skip_live {
        args.push("--skip-live".to_string());
    }

    if request.exclude_shared {
        args.push("--not-shared".to_string());
    }

    if let Some(library) = request.library.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--library".to_string());
        args.push(expand_user_path(library).to_string_lossy().to_string());
    }

    if let Some(album) = request.album.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--album".to_string());
        args.push(album.to_string());
    }

    if let Some(from_date) = request.from_date.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--from-date".to_string());
        args.push(from_date.to_string());
    }

    if let Some(to_date) = request.to_date.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--to-date".to_string());
        args.push(to_date.to_string());
    }

    if let Some(limit) = request.limit {
        if limit == 0 {
            return Err("Limit must be greater than 0.".to_string());
        }
        args.push("--newest-first".to_string());
        args.push("--limit".to_string());
        args.push(limit.to_string());
    }

    if request.download_missing {
        args.push("--download-missing".to_string());
    }

    if request.xmp_sidecar {
        args.push("--sidecar".to_string());
        args.push("xmp".to_string());
        args.push("--sidecar-drop-ext".to_string());
    }

    if request.dry_run {
        args.push("--dry-run".to_string());
    }

    Ok(args)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedReportCounts {
    pub exported: usize,
    pub updated: usize,
    pub skipped: usize,
    pub missing: usize,
    pub errors: usize,
    pub live_photo_videos: usize,
}

pub fn parse_osxphotos_report(report_path: &Path) -> ParsedReportCounts {
    let mut counts = ParsedReportCounts::default();
    let Ok(content) = std::fs::read_to_string(report_path) else {
        return counts;
    };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&content)
    else {
        return counts;
    };

    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let exported = obj.get("exported").and_then(|v| v.as_bool()).unwrap_or(false);
        let updated = obj.get("updated").and_then(|v| v.as_bool()).unwrap_or(false);
        let skipped = obj.get("skipped").and_then(|v| v.as_bool()).unwrap_or(false);
        let missing = obj.get("missing").and_then(|v| v.as_bool()).unwrap_or(false);
        let has_error = match obj.get("error") {
            Some(serde_json::Value::Null) | None => false,
            Some(serde_json::Value::Bool(b)) => *b,
            Some(serde_json::Value::String(s)) => !s.trim().is_empty(),
            Some(_) => true,
        };

        if exported {
            counts.exported += 1;
        } else if updated {
            counts.updated += 1;
        } else if skipped {
            counts.skipped += 1;
        } else if missing {
            counts.missing += 1;
        }

        if has_error {
            counts.errors += 1;
        }

        if let Some(filename) = obj.get("filename").and_then(|v| v.as_str()) {
            if filename.to_ascii_lowercase().ends_with(".mov") && (exported || updated) {
                counts.live_photo_videos += 1;
            }
        }
    }

    counts
}

pub fn run_export_blocking(
    app: &AppHandle,
    state: &ApplePhotosExportState,
    request: ApplePhotosExportRequest,
) -> Result<ApplePhotosExportResult, String> {
    let raw_dest = request.dest.trim();
    if raw_dest.is_empty() {
        return Err("Please choose a destination folder.".to_string());
    }

    state.try_begin()?;
    let dest = expand_user_path(raw_dest);
    let runner = resolve_osxphotos_runner()?;

    std::fs::create_dir_all(&dest)
        .map_err(|e| format!("Failed to create destination folder '{}': {e}", dest.display()))?;

    let raw_report_path = dest.join(DEFAULT_RAW_REPORT_NAME);
    if raw_report_path.exists() {
        let _ = std::fs::remove_file(&raw_report_path);
    }

    let export_args = build_export_args(&request, &dest, &raw_report_path)?;

    let mut command = Command::new(&runner.program);
    command
        .args(&runner.prefix_args)
        .args(&export_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(path_env) = augmented_path_env() {
        command.env("PATH", path_env);
    }
    command.env("PYTHONUNBUFFERED", "1");

    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to start {}: {e}", runner.label))?;

    state.set_pid(Some(child.id()));

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let last_stderr = Arc::new(Mutex::new(None::<String>));

    let stdout_app = app.clone();
    let stdout_handle = std::thread::spawn(move || {
        if let Some(pipe) = stdout {
            let reader = BufReader::new(pipe);
            for line in reader.lines().map_while(Result::ok) {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() {
                    let _ = stdout_app.emit(
                        "apple-photos-export-progress",
                        ApplePhotosProgressEvent {
                            line: trimmed,
                            stream: "stdout".to_string(),
                        },
                    );
                }
            }
        }
    });

    let stderr_app = app.clone();
    let stderr_last = last_stderr.clone();
    let stderr_handle = std::thread::spawn(move || {
        if let Some(pipe) = stderr {
            let reader = BufReader::new(pipe);
            for line in reader.lines().map_while(Result::ok) {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() {
                    if let Ok(mut guard) = stderr_last.lock() {
                        *guard = Some(trimmed.clone());
                    }
                    let _ = stderr_app.emit(
                        "apple-photos-export-progress",
                        ApplePhotosProgressEvent {
                            line: trimmed,
                            stream: "stderr".to_string(),
                        },
                    );
                }
            }
        }
    });

    let status = child
        .wait()
        .map_err(|e| format!("Failed while waiting for {}: {e}", runner.label));

    state.set_pid(None);
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    let status = status?;
    let elapsed_seconds = (started.elapsed().as_secs_f64() * 100.0).round() / 100.0;
    let exit_code = status.code().unwrap_or(-1);
    let cancelled = state.is_cancelled();
    let counts = parse_osxphotos_report(&raw_report_path);

    let error_message = if cancelled {
        Some("Export cancelled.".to_string())
    } else if exit_code != 0 {
        last_stderr
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .or_else(|| Some(format!("{} exited with code {exit_code}.", runner.label)))
    } else {
        None
    };

    let result = ApplePhotosExportResult {
        destination: dest.to_string_lossy().to_string(),
        versions: request.versions,
        dry_run: request.dry_run,
        cancelled,
        exit_code,
        exported: counts.exported,
        updated: counts.updated,
        skipped: counts.skipped,
        missing: counts.missing,
        errors: counts.errors,
        live_photo_videos: counts.live_photo_videos,
        elapsed_seconds,
        error_message,
    };

    let summary_path = dest.join(DEFAULT_SUMMARY_REPORT_NAME);
    if let Ok(json) = serde_json::to_string_pretty(&result) {
        let _ = std::fs::write(summary_path, format!("{json}\n"));
    }

    Ok(result)
}

fn resolve_osxphotos_python_command() -> Result<(PathBuf, Vec<String>), String> {
    let dirs = augmented_search_dirs();
    // If osxphotos is installed directly, its script shebang points to its Python interpreter.
    if let Some(osxphotos_bin) = find_executable_in_dirs("osxphotos", &dirs) {
        if let Ok(content) = std::fs::read_to_string(&osxphotos_bin) {
            if let Some(first_line) = content.lines().next() {
                if let Some(shebang) = first_line.strip_prefix("#!") {
                    let parts: Vec<&str> = shebang.trim().split_whitespace().collect();
                    if let Some(first) = parts.first() {
                        let py_path = PathBuf::from(first);
                        if py_path.is_file() {
                            let extra: Vec<String> =
                                parts[1..].iter().map(|s| s.to_string()).collect();
                            return Ok((py_path, extra));
                        }
                    }
                }
            }
        }
    }
    if let Some(uvx_bin) = find_executable_in_dirs("uvx", &dirs) {
        return Ok((
            uvx_bin,
            vec![
                "--quiet".to_string(),
                "--from".to_string(),
                "osxphotos".to_string(),
                "python".to_string(),
            ],
        ));
    }
    if let Some(uv_bin) = find_executable_in_dirs("uv", &dirs) {
        return Ok((
            uv_bin,
            vec![
                "tool".to_string(),
                "run".to_string(),
                "--quiet".to_string(),
                "--from".to_string(),
                "osxphotos".to_string(),
                "python".to_string(),
            ],
        ));
    }
    Err("Could not find 'osxphotos' or 'uvx'/'uv' to read Apple Shared Albums.".to_string())
}

fn run_osxphotos_python_json<T: for<'de> Deserialize<'de>>(
    script: &str,
    extra_args: &[&str],
) -> Result<T, String> {
    let (program, prefix) = resolve_osxphotos_python_command()?;
    let mut cmd = Command::new(&program);
    cmd.args(&prefix)
        .arg("-c")
        .arg(script)
        .args(extra_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path_env) = augmented_path_env() {
        cmd.env("PATH", path_env);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to query Apple Photos shared albums: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("Apple Photos query exited with status {}", output.status)
        } else {
            stderr
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .map_err(|e| format!("Failed to parse Apple Photos shared album data: {e}"))
}

pub fn list_shared_albums(
    state: &ApplePhotosExportState,
    force_refresh: bool,
) -> Result<Vec<AppleSharedAlbum>, String> {
    if !force_refresh {
        if let Ok(guard) = state.shared_albums_cache.lock() {
            if let Some(cached) = guard.as_ref() {
                return Ok(cached.clone());
            }
        }
    }

    const SCRIPT: &str = r#"
import json, os, osxphotos

def best_preview(photo):
    for cand in (photo.path_edited, photo.path):
        if cand and os.path.isfile(cand):
            return cand
    derivs = [d for d in (photo.path_derivatives or []) if d and os.path.isfile(d)]
    if derivs:
        derivs.sort(key=lambda d: os.path.getsize(d), reverse=True)
        return derivs[0]
    return None

db = osxphotos.PhotosDB()
albums = []
for album in sorted(db.album_info_shared, key=lambda a: (a.title or '').lower()):
    photos = album.photos
    cover = None
    for p in photos:
        cover = best_preview(p)
        if cover:
            break
    albums.append({
        'uuid': album.uuid,
        'title': album.title or 'Untitled Shared Album',
        'owner': album.owner or '',
        'count': len(photos),
        'coverPath': cover,
    })
print(json.dumps(albums, ensure_ascii=False))
"#;

    let albums: Vec<AppleSharedAlbum> = run_osxphotos_python_json(SCRIPT, &[])?;
    if let Ok(mut guard) = state.shared_albums_cache.lock() {
        *guard = Some(albums.clone());
    }
    Ok(albums)
}

pub fn get_shared_album_photos(album_uuid: &str) -> Result<Vec<AppleSharedPhoto>, String> {
    let uuid = album_uuid.trim();
    if uuid.is_empty() {
        return Err("Shared album UUID is required.".to_string());
    }

    const SCRIPT: &str = r#"
import json, os, sys, osxphotos

target_uuid = sys.argv[1]

def best_preview(photo):
    derivs = [d for d in (photo.path_derivatives or []) if d and os.path.isfile(d)]
    if derivs:
        derivs.sort(key=lambda d: os.path.getsize(d), reverse=True)
        return derivs[0]
    for cand in (photo.path_edited, photo.path):
        if cand and os.path.isfile(cand):
            return cand
    return None

def local_original(photo):
    for cand in (photo.path_edited, photo.path):
        if cand and os.path.isfile(cand):
            return cand
    return None

db = osxphotos.PhotosDB()
album = next((a for a in db.album_info_shared if a.uuid == target_uuid), None)
if album is None:
    print('[]')
    raise SystemExit(0)

photos = sorted(
    album.photos,
    key=lambda p: p.date.timestamp() if p.date else 0,
    reverse=True,
)
items = []
for p in photos:
    orig = local_original(p)
    preview = best_preview(p) or orig
    items.append({
        'uuid': p.uuid,
        'filename': p.original_filename or p.filename or p.uuid,
        'date': p.date.isoformat() if p.date else None,
        'isMovie': bool(p.ismovie),
        'isLocalOriginal': orig is not None,
        'previewPath': preview,
        'originalPath': orig,
        'width': int(p.width) if p.width else None,
        'height': int(p.height) if p.height else None,
    })
print(json.dumps(items, ensure_ascii=False))
"#;

    run_osxphotos_python_json(SCRIPT, &[uuid])
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

/// Walk up from a deleted file path looking for an `osxphotos` export database
/// (`.apple-photos-export.db`). Returns `(export_root, db_path)` if found.
fn find_export_db_for_path(file_path: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut current = file_path.parent()?;
    for _ in 0..8 {
        let candidate = current.join(DEFAULT_EXPORT_DB_NAME);
        if candidate.is_file() {
            return Some((current.to_path_buf(), candidate));
        }
        current = current.parent()?;
    }
    None
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

/// Look up deduplicated Apple Photos asset UUIDs for any deleted paths that originated
/// from an `osxphotos` export directory containing `.apple-photos-export.db`.
pub fn lookup_uuids_for_deleted_paths(paths: &[String]) -> Vec<String> {
    use std::collections::{HashMap, HashSet};

    let mut by_db: HashMap<PathBuf, (PathBuf, Vec<String>)> = HashMap::new();
    for raw in paths {
        let path = Path::new(raw);
        let Some((root, db_path)) = find_export_db_for_path(path) else {
            continue;
        };
        let Ok(rel) = path.strip_prefix(&root) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.is_empty() {
            continue;
        }
        by_db
            .entry(db_path)
            .or_insert_with(|| (root, Vec::new()))
            .1
            .push(rel_str);
    }

    let mut uuids = Vec::new();
    let mut seen = HashSet::new();

    for (db_path, (_root, rel_paths)) in by_db {
        let Ok(conn) = open_export_db_readonly(&db_path) else {
            continue;
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT uuid FROM export_data
             WHERE (filepath = ?1 OR filepath_normalized = ?2)
               AND uuid IS NOT NULL AND TRIM(uuid) <> ''
             LIMIT 1",
        ) else {
            continue;
        };
        for rel in rel_paths {
            let normalized = rel.to_lowercase();
            let found: Result<String, _> = stmt.query_row(rusqlite::params![rel, normalized], |row| row.get(0));
            if let Ok(uuid) = found {
                let cleaned = uuid.trim().to_uppercase();
                if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
                    uuids.push(cleaned);
                }
            }
        }
    }

    uuids
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

/// Automatically move deleted exported photos to Apple Photos' Recently Deleted album
/// on a background thread when the preference is enabled.
pub fn sync_deleted_paths_to_apple_photos(paths: Vec<String>) {
    if cfg!(test) || paths.is_empty() || !crate::t_config::sync_delete_to_apple_photos() {
        return;
    }
    let uuids = lookup_uuids_for_deleted_paths(&paths);
    if uuids.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        match delete_apple_photos_assets_by_uuids(&uuids) {
            Ok(count) => {
                if count > 0 {
                    eprintln!("Moved {count} deleted photo(s) to Apple Photos Recently Deleted.");
                }
            }
            Err(err) => {
                eprintln!("Apple Photos delete sync error: {err}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_export_args_defaults_and_modes() {
        let dest = Path::new("/tmp/lap-everyday");
        let report = dest.join(DEFAULT_RAW_REPORT_NAME);

        let default_req = ApplePhotosExportRequest {
            dest: dest.to_string_lossy().to_string(),
            versions: "edited-prefer".to_string(),
            library: None,
            album: None,
            from_date: None,
            to_date: None,
            limit: None,
            skip_live: false,
            exclude_shared: true,
            download_missing: false,
            xmp_sidecar: false,
            dry_run: false,
        };
        let args = build_export_args(&default_req, dest, &report).unwrap();
        assert!(args.contains(&"--update".to_string()));
        assert!(args.contains(&"--skip-original-if-edited".to_string()));
        assert!(args.contains(&"--touch-file".to_string()));
        assert!(args.contains(&"--not-shared".to_string()));
        assert!(!args.contains(&"--cleanup".to_string()));
        assert!(!args.contains(&"--sidecar".to_string()));
        assert!(!args.contains(&"--skip-live".to_string()));

        let custom_req = ApplePhotosExportRequest {
            versions: "both".to_string(),
            album: Some("Favorites".to_string()),
            from_date: Some("2025-01-01".to_string()),
            limit: Some(50),
            exclude_shared: false,
            download_missing: true,
            dry_run: true,
            ..default_req
        };
        let custom_args = build_export_args(&custom_req, dest, &report).unwrap();
        assert!(custom_args.contains(&"--edited-suffix".to_string()));
        assert!(custom_args.contains(&"_edited".to_string()));
        assert!(custom_args.contains(&"--album".to_string()));
        assert!(custom_args.contains(&"Favorites".to_string()));
        assert!(custom_args.contains(&"--newest-first".to_string()));
        assert!(custom_args.contains(&"--limit".to_string()));
        assert!(custom_args.contains(&"50".to_string()));
        assert!(!custom_args.contains(&"--not-shared".to_string()));
        assert!(custom_args.contains(&"--download-missing".to_string()));
        assert!(custom_args.contains(&"--dry-run".to_string()));
    }

    #[test]
    fn parse_report_counts_all_statuses_and_live_photo_videos() {
        let tmp = std::env::temp_dir().join(format!(
            "lap_apple_photos_report_{}.json",
            std::process::id()
        ));
        std::fs::write(
            &tmp,
            r#"[
                {"filename": "IMG_1.HEIC", "exported": true},
                {"filename": "IMG_1.MOV", "exported": true},
                {"filename": "IMG_2.HEIC", "updated": true},
                {"filename": "IMG_3.HEIC", "skipped": true},
                {"filename": "IMG_4.HEIC", "missing": true},
                {"filename": "IMG_5.HEIC", "error": "Permission denied"}
            ]"#,
        )
        .unwrap();

        let counts = parse_osxphotos_report(&tmp);
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(
            counts,
            ParsedReportCounts {
                exported: 2,
                updated: 1,
                skipped: 1,
                missing: 1,
                errors: 1,
                live_photo_videos: 1,
            }
        );
    }

    #[test]
    fn lookup_uuids_resolves_and_deduplicates_exported_files() {
        let root = std::env::temp_dir().join(format!(
            "lap_apple_photos_uuid_lookup_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let sub = root.join("2025").join("2025-06");
        std::fs::create_dir_all(&sub).unwrap();

        let db_path = root.join(DEFAULT_EXPORT_DB_NAME);
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE export_data (
                id INTEGER PRIMARY KEY,
                filepath_normalized TEXT,
                filepath TEXT,
                uuid TEXT
            );
            INSERT INTO export_data (filepath_normalized, filepath, uuid) VALUES
                ('2025/2025-06/img_0001.heic', '2025/2025-06/IMG_0001.HEIC', 'aaaa-1111'),
                ('2025/2025-06/img_0001.mov',  '2025/2025-06/IMG_0001.MOV',  'aaaa-1111'),
                ('2025/2025-06/img_0002.jpg',  '2025/2025-06/IMG_0002.JPG',  'BBBB-2222');",
        )
        .unwrap();
        drop(conn);

        let paths = vec![
            sub.join("IMG_0001.HEIC").to_string_lossy().to_string(),
            sub.join("IMG_0001.MOV").to_string_lossy().to_string(),
            sub.join("IMG_0002.JPG").to_string_lossy().to_string(),
            "/tmp/non-exported-family-scan.jpg".to_string(),
        ];

        let uuids = lookup_uuids_for_deleted_paths(&paths);
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(uuids, vec!["AAAA-1111".to_string(), "BBBB-2222".to_string()]);
    }
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
    let export_db = export_dir.join(DEFAULT_EXPORT_DB_NAME);
    let export = open_export_db_readonly(&export_db)
        .map_err(|e| format!("No Apple Photos export found in {}: {e}", export_dir.display()))?;

    let detections = read_apple_named_detections(photos_db)?;
    // An asset UUID can have multiple rows in export_data (e.g. a skipped original
    // alongside the exported `_edited` version); keep all candidate paths.
    let mut paths: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    {
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
            let list = paths.entry(uuid.trim().to_ascii_uppercase()).or_default();
            if !list.iter().any(|existing| existing == &rel) {
                list.push(rel);
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
        for rel in rels {
            let full = export_dir.join(rel);
            if let Some(file) = crate::t_sqlite::Face::file_for_path_with_conn(conn, &full)? {
                lap_files.push((file, full));
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
                 CREATE TABLE faces (id INTEGER PRIMARY KEY AUTOINCREMENT, file_id INTEGER, bbox TEXT, embedding BLOB, person_id INTEGER, created_at INTEGER, confirmed INTEGER NOT NULL DEFAULT 0, low_quality INTEGER NOT NULL DEFAULT 0, kind TEXT NOT NULL DEFAULT 'face', species TEXT);
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
