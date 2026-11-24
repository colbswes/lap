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
        .join("Lap-Everyday")
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
