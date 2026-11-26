/**
 * "Scan videos": samples frames across each standalone video and, per frame, stores a
 * scene embedding (so semantic search finds moments inside videos) and looks for faces
 * of people the user has already named. Each video also gets a live-music score from
 * zero-shot prompts, used to tag concert footage and to ignore stage/crowd faces.
 *
 * Kept separate from the photo face scan: video faces never take part in clustering
 * (they carry `faces.video_ts`), so strangers in videos never become new people.
 */
use crate::{t_ai, t_common, t_face, t_sqlite, t_startup, t_video};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Default, serde::Serialize)]
pub struct VideoScanProgress {
    pub current: usize,
    pub total: usize,
    pub frames: usize,
    pub faces_matched: usize,
    pub concerts: usize,
}

#[derive(Clone, Default)]
pub struct VideoScanState {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<VideoScanProgress>>,
}

#[derive(serde::Serialize)]
pub struct VideoScanStats {
    /// Standalone videos in the library (Live Photo companions excluded).
    pub total: usize,
    pub scanned: usize,
    pub concerts: usize,
}

/// Prompts scored against every frame; the first `MUSIC_PROMPTS` describe live music.
const SCENE_PROMPTS: &[&str] = &[
    "a photo of a concert",
    "a photo of a music festival",
    "a photo of a band performing on stage",
    "a photo of a crowd at a live music show",
    "a photo of friends hanging out",
    "a photo of people in a car",
    "a photo of a party at home",
    "a photo of a city street",
    "a photo of nature and landscape",
    "a photo of a restaurant",
    "a photo of a sports game",
    "a photo of a dark room",
    "a photo of a parade",
    "a photo of a pet",
];
const MUSIC_PROMPTS: usize = 4;
/// CLIP's logit scale, so prompt similarities become a sharp softmax.
const LOGIT_SCALE: f32 = 100.0;

const STANDALONE_VIDEO: &str = "a.file_type = 2
    AND NOT EXISTS (SELECT 1 FROM afiles s WHERE s.live_photo_video_id = a.id)";

struct VideoFile {
    id: i64,
    path: String,
    duration: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
}

fn videos_to_scan(conn: &Connection) -> Result<Vec<VideoFile>, String> {
    let sql = format!(
        "SELECT a.id, f.path || '/' || a.name, a.duration, a.width, a.height
         FROM afiles a JOIN afolders f ON a.folder_id = f.id
         WHERE {STANDALONE_VIDEO}
           AND IFNULL(a.cloud_only, 0) = 0
           AND NOT EXISTS (SELECT 1 FROM video_scans v WHERE v.file_id = a.id)
         ORDER BY a.id"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(VideoFile {
                id: row.get(0)?,
                path: row.get(1)?,
                duration: row.get(2)?,
                width: row.get(3)?,
                height: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

pub fn stats() -> Result<VideoScanStats, String> {
    let conn = t_sqlite::open_conn()?;
    let (total, scanned, concerts): (i64, i64, i64) = conn
        .query_row(
            &format!(
                "SELECT COUNT(*), COUNT(v.file_id),
                        SUM(CASE WHEN v.concert_score >= ?1 THEN 1 ELSE 0 END)
                 FROM afiles a LEFT JOIN video_scans v ON v.file_id = a.id
                 WHERE {STANDALONE_VIDEO}"
            ),
            params![t_common::VIDEO_CONCERT_THRESHOLD],
            |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, Option<i64>>(2)?.unwrap_or(0))),
        )
        .map_err(|e| e.to_string())?;
    Ok(VideoScanStats {
        total: total as usize,
        scanned: scanned as usize,
        concerts: concerts as usize,
    })
}

fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Probability mass the softmax over `SCENE_PROMPTS` puts on the live-music prompts.
pub(crate) fn music_probability(frame: &[f32], prompts: &[Vec<f32>]) -> f32 {
    let logits: Vec<f32> = prompts.iter().map(|p| LOGIT_SCALE * dot(frame, p)).collect();
    let max = logits.iter().cloned().fold(f32::MIN, f32::max);
    let exp: Vec<f32> = logits.iter().map(|l| (l - max).exp()).collect();
    let sum: f32 = exp.iter().sum();
    if sum <= 0.0 {
        return 0.0;
    }
    exp[..MUSIC_PROMPTS.min(exp.len())].iter().sum::<f32>() / sum
}

/// Mean of the top-3 cosine similarities to a person's reference faces (same ranking
/// as Review suggestions).
pub(crate) fn person_score(embedding: &[f32], refs: &[Vec<f32>]) -> f32 {
    let mut sims: Vec<f32> = refs.iter().map(|r| dot(embedding, r)).collect();
    sims.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let k = sims.len().min(3);
    if k == 0 {
        return f32::MIN;
    }
    sims[..k].iter().sum::<f32>() / k as f32
}

/// Reference embeddings of every named person: their confirmed faces, or all their
/// faces when none are confirmed (the rule Review suggestions uses).
fn named_person_refs(conn: &Connection) -> Result<Vec<(i64, Vec<Vec<f32>>)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT p.id, f.embedding, f.confirmed
             FROM faces f JOIN persons p ON p.id = f.person_id
             WHERE f.kind = 'face' AND p.kind = 'person' AND f.embedding IS NOT NULL
               AND trim(IFNULL(p.name, '')) != '' AND p.name NOT GLOB ?1 || ' [0-9]*'",
        )
        .map_err(|e| e.to_string())?;
    let mut by_person: HashMap<i64, (Vec<Vec<f32>>, Vec<Vec<f32>>)> = HashMap::new();
    let rows = stmt
        .query_map(params![t_sqlite::PersonKind::Person.default_name_prefix()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, i64>(2)? != 0))
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (person_id, bytes, confirmed) = row.map_err(|e| e.to_string())?;
        if bytes.len() % 4 != 0 {
            continue;
        }
        let emb: Vec<f32> = bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let entry = by_person.entry(person_id).or_default();
        if confirmed { entry.0.push(emb) } else { entry.1.push(emb) }
    }
    Ok(by_person
        .into_iter()
        .map(|(id, (confirmed, all))| (id, if confirmed.is_empty() { all } else { confirmed }))
        .filter(|(_, refs)| !refs.is_empty())
        .collect())
}

/// Best face found for one person in one video.
struct PersonHit {
    score: f32,
    ts: u64,
    face: t_face::FaceData,
    frame_dims: (u32, u32),
}

/// Store one video's results, replacing anything from an earlier scan (except faces the
/// user confirmed). Returns the number of people found.
fn save_video(
    conn: &mut Connection,
    video: &VideoFile,
    frames: &[(u64, Vec<f32>)],
    concert_score: Option<f32>,
    hits: HashMap<i64, PersonHit>,
) -> Result<usize, String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM video_frames WHERE file_id = ?1", params![video.id])
        .map_err(|e| e.to_string())?;
    {
        let mut insert = tx
            .prepare("INSERT OR REPLACE INTO video_frames (file_id, ts, embed) VALUES (?1, ?2, ?3)")
            .map_err(|e| e.to_string())?;
        for (ts, emb) in frames {
            let bytes: Vec<u8> = emb.iter().flat_map(|v| v.to_le_bytes()).collect();
            insert.execute(params![video.id, *ts as i64, bytes]).map_err(|e| e.to_string())?;
        }
    }
    tx.execute(
        "DELETE FROM faces WHERE file_id = ?1 AND video_ts IS NOT NULL AND confirmed = 0",
        params![video.id],
    )
    .map_err(|e| e.to_string())?;
    // People already in this video (confirmed, or from a cover-frame scan) stay as they are.
    let present: Vec<i64> = {
        let mut stmt = tx
            .prepare("SELECT DISTINCT person_id FROM faces WHERE file_id = ?1 AND person_id IS NOT NULL")
            .map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map(params![video.id], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        ids
    };
    let mut found = 0;
    for (person_id, hit) in hits {
        if present.contains(&person_id) {
            continue;
        }
        // Boxes are stored in the video's stored frame size, like the photo scan does.
        let (fw, fh) = hit.frame_dims;
        let (sx, sy) = match (video.width, video.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 && fw > 0 && fh > 0 => (w as f32 / fw as f32, h as f32 / fh as f32),
            _ => (1.0, 1.0),
        };
        let b = &hit.face.bbox;
        let bbox = serde_json::json!({
            "x": b.x * sx, "y": b.y * sy, "width": b.width * sx, "height": b.height * sy,
            "confidence": b.confidence,
        })
        .to_string();
        let face_id = t_sqlite::Face::add_with_conn(&tx, video.id, &bbox, &hit.face.embedding, hit.face.low_quality, "face", None)?;
        // Strong matches are assigned (still unconfirmed); weaker ones only show up
        // under Review suggestions for that person.
        let assign = (hit.score >= t_common::VIDEO_FACE_ASSIGN_SIMILARITY).then_some(person_id);
        tx.execute(
            "UPDATE faces SET video_ts = ?1, person_id = ?2, confirmed = 0 WHERE id = ?3",
            params![hit.ts as i64, assign, face_id],
        )
        .map_err(|e| e.to_string())?;
        found += 1;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    tx.execute(
        "INSERT OR REPLACE INTO video_scans (file_id, scanned_at, frame_count, concert_score) VALUES (?1, ?2, ?3, ?4)",
        params![video.id, now, frames.len() as i64, concert_score],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(found)
}

struct VideoAnalysis {
    /// (second, normalized scene embedding) per sampled frame.
    frames: Vec<(u64, Vec<f32>)>,
    concert_score: Option<f32>,
    /// Best face per named person (person id -> hit).
    hits: HashMap<i64, PersonHit>,
}

/// Sample a video's frames, embed each, score it for live music and find named people.
/// Returns `None` when cancelled part-way.
async fn analyze_video<E, D>(
    video: &VideoFile,
    prompts: &[Vec<f32>],
    refs: &[(i64, Vec<Vec<f32>>)],
    cancel: &AtomicBool,
    mut embed: E,
    mut detect: D,
) -> Option<VideoAnalysis>
where
    E: FnMut(&[u8]) -> Result<Vec<f32>, String>,
    D: FnMut(&[u8]) -> Result<(Vec<t_face::FaceData>, (u32, u32)), String>,
{
    let duration = match video.duration {
        Some(d) if d > 0 => d as u64,
        _ => t_video::get_video_duration(&video.path).await.unwrap_or(0),
    };

    let mut frame_embeds: Vec<(u64, Vec<f32>)> = Vec::new();
    let mut frame_faces: Vec<(u64, Vec<t_face::FaceData>, (u32, u32))> = Vec::new();
    for ts in t_video::sample_timestamps(duration, t_common::VIDEO_SCAN_INTERVAL_SECS, t_common::VIDEO_SCAN_MAX_FRAMES) {
        if cancel.load(Ordering::SeqCst) {
            return None;
        }
        let Ok(Some(jpeg)) = t_video::get_video_frame_at(&video.path, ts, t_common::VIDEO_SCAN_FRAME_SIZE).await else {
            continue;
        };
        if let Ok(embed) = embed(&jpeg) {
            frame_embeds.push((ts, normalize(embed)));
        }
        if !refs.is_empty() {
            if let Ok((faces, dims)) = detect(&jpeg) {
                if !faces.is_empty() {
                    frame_faces.push((ts, faces, dims));
                }
            }
        }
    }
    if cancel.load(Ordering::SeqCst) {
        return None;
    }

    let concert_score = (!frame_embeds.is_empty()).then(|| {
        frame_embeds.iter().map(|(_, e)| music_probability(e, &prompts)).sum::<f32>() / frame_embeds.len() as f32
    });
    let is_concert = concert_score.is_some_and(|s| s >= t_common::VIDEO_CONCERT_THRESHOLD);

    // Each face votes for its single closest named person; keep that person's best face.
    let min_score = 1.0 - t_common::FACE_SUGGESTION_MAX_DISTANCE;
    let mut hits: HashMap<i64, PersonHit> = HashMap::new();
    for (ts, faces, dims) in frame_faces {
        for face in faces {
            if is_concert && face.bbox.height < dims.1 as f32 * t_common::VIDEO_CONCERT_MIN_FACE_FRACTION {
                continue;
            }
            let Some((person_id, score)) = refs
                .iter()
                .map(|(id, r)| (*id, person_score(&face.embedding, r)))
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            else {
                continue;
            };
            if score < min_score || hits.get(&person_id).is_some_and(|h| h.score >= score) {
                continue;
            }
            hits.insert(person_id, PersonHit { score, ts, face, frame_dims: dims });
        }
    }
    Some(VideoAnalysis { frames: frame_embeds, concert_score, hits })
}

pub fn start(app: AppHandle, state: VideoScanState) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("Video scan is already running".to_string());
    }
    state.cancel.store(false, Ordering::SeqCst);
    *state.progress.lock().unwrap() = VideoScanProgress::default();

    tauri::async_runtime::spawn(async move {
        let result = run(&app, &state).await;
        let progress = state.progress.lock().unwrap().clone();
        let cancelled = state.cancel.load(Ordering::SeqCst);
        state.running.store(false, Ordering::SeqCst);
        let _ = app.emit(
            "video_scan_finished",
            serde_json::json!({
                "scanned": progress.current,
                "total": progress.total,
                "faces_matched": progress.faces_matched,
                "concerts": progress.concerts,
                "cancelled": cancelled,
                "error": result.err(),
            }),
        );
    });
    Ok(())
}

pub fn cancel(state: &VideoScanState) {
    state.cancel.store(true, Ordering::SeqCst);
}

pub fn status(state: &VideoScanState) -> (bool, Option<VideoScanProgress>) {
    let running = state.running.load(Ordering::SeqCst);
    (running, running.then(|| state.progress.lock().unwrap().clone()))
}

async fn run(app: &AppHandle, state: &VideoScanState) -> Result<(), String> {
    let app_for_models = app.clone();
    tauri::async_runtime::spawn_blocking(move || t_startup::ensure_ai(&app_for_models, None))
        .await
        .map_err(|e| e.to_string())??;
    let face_state = app.state::<t_face::FaceState>().inner().clone();
    {
        let mut engine = face_state.0.lock().unwrap();
        if !engine.is_loaded() {
            engine.load_models(app)?;
        }
    }

    let prompts: Vec<Vec<f32>> = {
        let ai = app.state::<t_ai::AiState>();
        let mut engine = ai.0.lock().unwrap();
        SCENE_PROMPTS
            .iter()
            .map(|p| engine.encode_text(p).map(normalize))
            .collect::<Result<_, _>>()?
    };

    let mut conn = t_sqlite::open_conn()?;
    let refs = named_person_refs(&conn)?;
    let videos = videos_to_scan(&conn)?;
    state.progress.lock().unwrap().total = videos.len();
    let emit = |p: &VideoScanProgress| {
        let _ = app.emit("video_scan_progress", p.clone());
    };
    emit(&state.progress.lock().unwrap());

    for video in &videos {
        if state.cancel.load(Ordering::SeqCst) {
            break;
        }
        let embed = |jpeg: &[u8]| {
            let ai = app.state::<t_ai::AiState>();
            let mut engine = ai.0.lock().unwrap();
            engine.encode_image_from_bytes(jpeg)
        };
        let detect = |jpeg: &[u8]| face_state.0.lock().unwrap().process_image_from_bytes(jpeg);
        let Some(analysis) = analyze_video(video, &prompts, &refs, &state.cancel, embed, detect).await else {
            break; // cancelled: a half-sampled video is rescanned next time
        };
        let VideoAnalysis { frames: frame_embeds, concert_score, hits } = analysis;
        let is_concert = concert_score.is_some_and(|s| s >= t_common::VIDEO_CONCERT_THRESHOLD);
        let found = save_video(&mut conn, video, &frame_embeds, concert_score, hits)?;
        let mut p = state.progress.lock().unwrap();
        p.current += 1;
        p.frames += frame_embeds.len();
        p.faces_matched += found;
        p.concerts += is_concert as usize;
        emit(&p);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_probability_counts_only_live_music_prompts() {
        // Six orthogonal prompts: the first four are the live-music ones.
        let prompts: Vec<Vec<f32>> = (0..6).map(|i| (0..6).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
        let near = |i: usize| -> Vec<f32> { (0..6).map(|j| if i == j { 0.5 } else { 0.0 }).collect() };
        assert!(music_probability(&near(0), &prompts) > 0.99);
        assert!(music_probability(&near(3), &prompts) > 0.99);
        assert!(music_probability(&near(5), &prompts) < 0.01);
    }

    #[test]
    fn person_score_averages_top_three() {
        let refs = vec![vec![1.0, 0.0], vec![0.8, 0.6], vec![0.0, 1.0], vec![0.6, 0.8]];
        let s = person_score(&[1.0, 0.0], &refs);
        assert!((s - (1.0 + 0.8 + 0.6) / 3.0).abs() < 1e-6);
        assert_eq!(person_score(&[1.0, 0.0], &[]), f32::MIN);
    }

    /// End-to-end check on a COPY of a real library with real models:
    /// `VIDEO_SCAN_DB=/copy/of/default.db VIDEO_SCAN_IDS=1,2,3 VIDEO_SCAN_OUT=/dir \
    ///  cargo test --release --bin Lap real_library_video_scan -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_library_video_scan() {
        let db = std::env::var("VIDEO_SCAN_DB").unwrap();
        let out = std::path::PathBuf::from(std::env::var("VIDEO_SCAN_OUT").unwrap());
        let ids: Vec<i64> = std::env::var("VIDEO_SCAN_IDS").unwrap().split(',').map(|x| x.trim().parse().unwrap()).collect();
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(rt.spawn(async move {
            t_sqlite::TEST_DB_PATH.with(|p| p.replace(Some(db)));
            let mut conn = t_sqlite::open_conn().unwrap();
            t_sqlite::ensure_video_scan_schema(&conn).unwrap();
            let mut ai = t_ai::AiEngine::load_bundled_for_test();
            let mut faces = t_face::FaceEngine::load_bundled_for_test();
            let prompts: Vec<Vec<f32>> = SCENE_PROMPTS.iter().map(|p| normalize(ai.encode_text(p).unwrap())).collect();
            let refs = named_person_refs(&conn).unwrap();
            let names: HashMap<i64, String> = conn
                .prepare("SELECT id, name FROM persons").unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
                .map(|r| r.unwrap()).collect();
            println!("{} named people", refs.len());
            let videos: Vec<VideoFile> = videos_to_scan(&conn).unwrap().into_iter().filter(|v| ids.contains(&v.id)).collect();
            let cancel = AtomicBool::new(false);
            let started = std::time::Instant::now();
            for video in &videos {
                let label: Option<String> = conn
                    .query_row(
                        "SELECT group_concat(DISTINCT c.kind) FROM acollections_files cf JOIN acollections c ON c.id = cf.collection_id WHERE cf.file_id = ?1",
                        [video.id], |r| r.get(0)).unwrap();
                let a = analyze_video(video, &prompts, &refs, &cancel,
                    |b| ai.encode_image_from_bytes(b),
                    |b| faces.process_image_from_bytes(b)).await.unwrap();
                let people: Vec<String> = a.hits.iter()
                    .map(|(id, h)| format!("{} {:.2}@{}s{}", names[id], h.score, h.ts,
                        if h.score >= t_common::VIDEO_FACE_ASSIGN_SIMILARITY { "" } else { " (suggest)" }))
                    .collect();
                println!("video {} [{}] frames {} concert {:.2} people {:?}",
                    video.id, label.unwrap_or_default(), a.frames.len(), a.concert_score.unwrap_or(0.0), people);
                save_video(&mut conn, video, &a.frames, a.concert_score, a.hits).unwrap();
            }
            println!("scanned {} videos in {:.1}s", videos.len(), started.elapsed().as_secs_f32());

            // Face crops come from the frame each face was found in.
            let face_ids: Vec<i64> = conn.prepare("SELECT id FROM faces WHERE video_ts IS NOT NULL").unwrap()
                .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
            for id in &face_ids {
                let jpeg = tokio::task::block_in_place(|| t_sqlite::Person::generate_face_thumbnail(&conn, *id)).unwrap();
                std::fs::write(out.join(format!("face_{id}.jpg")), jpeg.expect("video face crop")).unwrap();
            }
            println!("wrote {} video face crops", face_ids.len());

            // "concert videos": known matches (events + live-music clips), video-only.
            let intent = t_sqlite::SearchIntent::parse(&conn, "concert videos").unwrap();
            let known = intent.matching_file_ids(&conn, " AND a.file_type = 2").unwrap();
            let scanned_concerts: i64 = conn.query_row(
                "SELECT COUNT(*) FROM video_scans WHERE concert_score >= ?1", [t_common::VIDEO_CONCERT_THRESHOLD], |r| r.get(0)).unwrap();
            println!("'concert videos' -> {} known matches; {} scanned clips scored as live music", known.len(), scanned_concerts);
        })).unwrap();
    }

}
