//! Performing artists attached to individual photos and videos (e.g. the set a clip was filmed at).
//!
//! Artists are deliberately separate from tags and people: they only exist while at least one file
//! references them, and an event collection's lineup is derived from its files' artists.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::t_sqlite::open_conn;

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AArtist {
    pub id: i64,
    pub name: String,
    /// Number of files (overall, or within the requested collection).
    pub count: i64,
}

pub fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS aartists (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS uidx_aartists_name ON aartists(name COLLATE NOCASE);
        CREATE TABLE IF NOT EXISTS afile_artists (
            file_id INTEGER NOT NULL,
            artist_id INTEGER NOT NULL,
            PRIMARY KEY (file_id, artist_id),
            FOREIGN KEY (file_id) REFERENCES afiles(id) ON DELETE CASCADE,
            FOREIGN KEY (artist_id) REFERENCES aartists(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_afile_artists_artist ON afile_artists(artist_id, file_id);",
    )
    .map_err(|e| e.to_string())
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn validate_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 255 || name.contains('\0') {
        return Err("Invalid artist name: use 1–255 characters".into());
    }
    Ok(name)
}

/// Find an artist by case-insensitive name, creating it when missing.
pub fn find_or_create(conn: &Connection, name: &str) -> Result<i64, String> {
    let name = validate_name(name)?;
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM aartists WHERE name = ?1 COLLATE NOCASE",
            [name],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO aartists (name, created_at) VALUES (?1, ?2)",
        params![name, now_ts()],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

/// Artists exist only while referenced; drop any left without files.
fn delete_orphans(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "DELETE FROM aartists WHERE NOT EXISTS (SELECT 1 FROM afile_artists fa WHERE fa.artist_id = aartists.id)",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn query_artists(conn: &Connection, sql: &str, param: Option<i64>) -> Result<Vec<AArtist>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let map = |r: &rusqlite::Row| {
        Ok(AArtist {
            id: r.get(0)?,
            name: r.get(1)?,
            count: r.get(2)?,
        })
    };
    let rows = match param {
        Some(p) => stmt.query_map([p], map),
        None => stmt.query_map([], map),
    }
    .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn list() -> Result<Vec<AArtist>, String> {
    let conn = open_conn()?;
    query_artists(
        &conn,
        "SELECT ar.id, ar.name, COUNT(fa.file_id)
         FROM aartists ar JOIN afile_artists fa ON fa.artist_id = ar.id
         GROUP BY ar.id ORDER BY ar.name COLLATE NOCASE",
        None,
    )
}

pub fn for_file(file_id: i64) -> Result<Vec<AArtist>, String> {
    let conn = open_conn()?;
    query_artists(
        &conn,
        "SELECT ar.id, ar.name, (SELECT COUNT(*) FROM afile_artists c WHERE c.artist_id = ar.id)
         FROM afile_artists fa JOIN aartists ar ON ar.id = fa.artist_id
         WHERE fa.file_id = ?1 ORDER BY ar.name COLLATE NOCASE",
        Some(file_id),
    )
}

/// The lineup of a collection: artists set on its files, most-filmed first.
pub fn for_collection(collection_id: i64) -> Result<Vec<AArtist>, String> {
    let conn = open_conn()?;
    query_artists(
        &conn,
        "SELECT ar.id, ar.name, COUNT(DISTINCT cf.file_id) AS n
         FROM acollections_files cf
         JOIN afile_artists fa ON fa.file_id = cf.file_id
         JOIN aartists ar ON ar.id = fa.artist_id
         WHERE cf.collection_id = ?1
         GROUP BY ar.id ORDER BY n DESC, ar.name COLLATE NOCASE",
        Some(collection_id),
    )
}

/// An artist within an events scope, with how many distinct days they were filmed.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AEventArtist {
    /// Artist id, or -1 for files in the scope without any artist.
    pub id: i64,
    pub name: String,
    pub count: i64,
    pub days: i64,
    pub last_date: Option<i64>,
}

/// Artists in every event of `kind`, or in one event; most recently seen first.
/// Ends with a `-1` entry counting the scope's files that have no artist yet.
pub fn for_events(kind: Option<&str>, event_id: Option<i64>) -> Result<Vec<AEventArtist>, String> {
    let conn = open_conn()?;
    let (scope, param): (&str, Box<dyn rusqlite::ToSql>) = match (event_id, kind) {
        (Some(id), _) => ("SELECT DISTINCT cf.file_id FROM acollections_files cf WHERE cf.collection_id = ?1", Box::new(id)),
        (None, Some(kind)) => (
            "SELECT DISTINCT cf.file_id FROM acollections_files cf JOIN acollections c ON c.id = cf.collection_id WHERE c.kind = ?1",
            Box::new(kind.to_string()),
        ),
        (None, None) => return Err("An event or event type is required".into()),
    };
    let day = "date(NULLIF(a.taken_date, 0), 'unixepoch', 'localtime')";
    let sql = format!(
        "WITH scope(file_id) AS ({scope}),
         files AS (SELECT a.id, a.taken_date FROM afiles a JOIN scope s ON s.file_id = a.id
                   WHERE a.id NOT IN (SELECT live_photo_video_id FROM afiles WHERE live_photo_video_id IS NOT NULL))
         SELECT ar.id, ar.name, COUNT(DISTINCT a.id), COUNT(DISTINCT {day}), MAX(NULLIF(a.taken_date, 0)) AS last
         FROM files a JOIN afile_artists fa ON fa.file_id = a.id JOIN aartists ar ON ar.id = fa.artist_id
         GROUP BY ar.id
         UNION ALL
         SELECT -1, '', COUNT(*), COUNT(DISTINCT {day}), MAX(NULLIF(a.taken_date, 0))
         FROM files a WHERE NOT EXISTS (SELECT 1 FROM afile_artists fa WHERE fa.file_id = a.id)"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([param.as_ref()], |r| {
            Ok(AEventArtist {
                id: r.get(0)?,
                name: r.get(1)?,
                count: r.get(2)?,
                days: r.get(3)?,
                last_date: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut artists: Vec<AEventArtist> = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|artist| artist.id > 0 || artist.count > 0)
        .collect();
    artists.sort_by(|a, b| {
        (a.id < 0)
            .cmp(&(b.id < 0))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(artists)
}

fn unique_ids(file_ids: Vec<i64>) -> Vec<i64> {
    file_ids
        .into_iter()
        .filter(|id| *id > 0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn add_to_files_on(conn: &Connection, name: &str, file_ids: &[i64]) -> Result<i64, String> {
    let artist_id = find_or_create(conn, name)?;
    let mut stmt = conn
        .prepare_cached(
            "INSERT OR IGNORE INTO afile_artists (file_id, artist_id)
             SELECT id, ?2 FROM afiles WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;
    for file_id in file_ids {
        stmt.execute(params![file_id, artist_id])
            .map_err(|e| e.to_string())?;
    }
    Ok(artist_id)
}

/// Set an artist (by name, created if needed) on the given files.
pub fn add_to_files(name: &str, file_ids: Vec<i64>) -> Result<AArtist, String> {
    let file_ids = unique_ids(file_ids);
    let mut conn = open_conn()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let artist_id = add_to_files_on(&tx, name, &file_ids)?;
    delete_orphans(&tx)?;
    let artist = tx
        .query_row(
            "SELECT id, name, (SELECT COUNT(*) FROM afile_artists WHERE artist_id = ?1) FROM aartists WHERE id = ?1",
            [artist_id],
            |r| Ok(AArtist { id: r.get(0)?, name: r.get(1)?, count: r.get(2)? }),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No matching files to tag".to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(artist)
}

pub fn remove_from_files(artist_id: i64, file_ids: Vec<i64>) -> Result<usize, String> {
    let file_ids = unique_ids(file_ids);
    let mut conn = open_conn()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut changed = 0;
    {
        let mut stmt = tx
            .prepare_cached("DELETE FROM afile_artists WHERE file_id = ?1 AND artist_id = ?2")
            .map_err(|e| e.to_string())?;
        for file_id in &file_ids {
            changed += stmt
                .execute(params![file_id, artist_id])
                .map_err(|e| e.to_string())?;
        }
    }
    delete_orphans(&tx)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(changed)
}

/// Rename an artist; renaming onto an existing artist's name merges the two.
pub fn rename(artist_id: i64, name: &str) -> Result<i64, String> {
    let name = validate_name(name)?;
    let mut conn = open_conn()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM aartists WHERE name = ?1 COLLATE NOCASE AND id != ?2",
            params![name, artist_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let result = if let Some(target) = existing {
        tx.execute(
            "INSERT OR IGNORE INTO afile_artists (file_id, artist_id)
             SELECT file_id, ?1 FROM afile_artists WHERE artist_id = ?2",
            params![target, artist_id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM aartists WHERE id = ?1", [artist_id])
            .map_err(|e| e.to_string())?;
        target
    } else {
        let changed = tx
            .execute("UPDATE aartists SET name = ?1 WHERE id = ?2", params![name, artist_id])
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("Artist not found".into());
        }
        artist_id
    };
    tx.commit().map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE afiles(id INTEGER PRIMARY KEY);
             INSERT INTO afiles VALUES (1),(2),(3);",
        )
        .unwrap();
        initialize(&conn).unwrap();
        conn
    }

    #[test]
    fn find_or_create_is_case_insensitive() {
        let conn = conn();
        let a = find_or_create(&conn, "Fred again..").unwrap();
        let b = find_or_create(&conn, "  fred AGAIN.. ").unwrap();
        assert_eq!(a, b);
        assert!(find_or_create(&conn, "   ").is_err());
    }

    #[test]
    fn add_skips_missing_files_and_orphans_are_removed() {
        let conn = conn();
        let id = add_to_files_on(&conn, "Bicep", &[1, 2, 99]).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM afile_artists WHERE artist_id=?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
        conn.execute("DELETE FROM afiles", []).unwrap();
        delete_orphans(&conn).unwrap();
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM aartists", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }
}
