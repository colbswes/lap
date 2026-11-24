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
    #[serde(default)]
    pub download_missing: bool,
    #[serde(default)]
    pub xmp_sidecar: bool,
    #[serde(default)]
    pub dry_run: bool,
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
            download_missing: false,
            xmp_sidecar: false,
            dry_run: false,
        };
        let args = build_export_args(&default_req, dest, &report).unwrap();
        assert!(args.contains(&"--update".to_string()));
        assert!(args.contains(&"--skip-original-if-edited".to_string()));
        assert!(args.contains(&"--touch-file".to_string()));
        assert!(!args.contains(&"--cleanup".to_string()));
        assert!(!args.contains(&"--sidecar".to_string()));
        assert!(!args.contains(&"--skip-live".to_string()));

        let custom_req = ApplePhotosExportRequest {
            versions: "both".to_string(),
            album: Some("Favorites".to_string()),
            from_date: Some("2025-01-01".to_string()),
            limit: Some(50),
            download_missing: true,
            dry_run: true,
            ..default_req
        };
        let custom_args = build_export_args(&custom_req, dest, &report).unwrap();
        assert!(custom_args.contains(&"--edited-suffix".to_string()));
        assert!(custom_args.contains(&"_edited".to_string()));
        assert!(custom_args.contains(&"--album".to_string()));
        assert!(custom_args.contains(&"Favorites".to_string()));
        assert!(custom_args.contains(&"--limit".to_string()));
        assert!(custom_args.contains(&"50".to_string()));
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
}
