//! Persistent SQLite cache for header-scan results.
//!
//! On each run we walk the source tree and build a temp table of (path,
//! mtime), then diff it against the persistent store in a single query.
//! Only files that are new or whose mtime changed are rescanned.  When a
//! header changes, every source that transitively includes it (tracked
//! via a reverse index) is invalidated.
//!
//! The cache lives at `{output_dir}/../.header-cache.db` so it survives
//! the output-dir clearing that the ninja target performs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::Context;
use rusqlite::{params, Connection};

/// Files with these extensions are tracked for mtime changes.
const TRACKED_EXTS: &[&str] = &["h", "hpp", "hxx", "inl", "cpp", "cxx", "cc", "c"];

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS forward (
    path     TEXT PRIMARY KEY,
    mtime    INTEGER NOT NULL,
    headers  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS reverse (
    header  TEXT NOT NULL,
    source  TEXT NOT NULL,
    PRIMARY KEY (header, source)
);
CREATE INDEX IF NOT EXISTS idx_rev_hdr ON reverse(header);
";

/// Open (or create) the cache database next to the output directory.
pub fn open_db(output_dir: &Path) -> anyhow::Result<Connection> {
    let db_dir = output_dir.parent().unwrap_or(output_dir);
    let db_path = db_dir.join(".header-cache.db");
    let db = Connection::open(&db_path)
        .with_context(|| format!("Opening cache db '{}'", db_path.display()))?;
    db.execute_batch(SCHEMA)?;
    Ok(db)
}

/// What changed since the last run, after reverse-index invalidation.
pub struct Diff {
    /// Files that need scanning (new, changed, or invalidated).
    pub needs_scan: HashSet<PathBuf>,
    /// Files deleted from disk (purged from cache).
    pub deleted: HashSet<PathBuf>,
}

/// Walk `roots`, diff mtimes against the cache, invalidate consumers of
/// changed headers.  Returns the set of files that need fresh scanning.
pub fn compute_diff(db: &Connection, roots: &[PathBuf]) -> anyhow::Result<Diff> {
    db.execute_batch("CREATE TEMP TABLE scan (path TEXT PRIMARY KEY, mtime INTEGER NOT NULL);")?;

    let mut insert = db.prepare("INSERT OR IGNORE INTO scan VALUES (?1, ?2)")?;
    for root in roots {
        walk(root, &mut |path, mtime| {
            insert.execute(params![path, mtime as i64]).ok();
        });
    }

    let mut needs_scan: HashSet<PathBuf> = HashSet::new();
    let mut changed_headers: Vec<PathBuf> = Vec::new();
    let mut deleted: HashSet<PathBuf> = HashSet::new();

    // new or changed
    {
        let mut q = db.prepare(
            "SELECT s.path, s.mtime, c.mtime FROM scan s \
             LEFT JOIN forward c ON s.path = c.path",
        )?;
        let rows = q.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?;
        for row in rows {
            let (p, new_mt, old_mt) = row?;
            let path = PathBuf::from(&p);
            match old_mt {
                None => {
                    needs_scan.insert(path);
                }
                Some(old) if old != new_mt => {
                    if is_header(&p) {
                        changed_headers.push(path.clone());
                    }
                    needs_scan.insert(path);
                }
                _ => {}
            }
        }
    }

    // deleted (in cache but not on disk)
    {
        let mut q = db.prepare(
            "SELECT c.path FROM forward c LEFT JOIN scan s ON c.path = s.path WHERE s.path IS NULL",
        )?;
        let rows = q.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            deleted.insert(PathBuf::from(row?));
        }
    }

    // Invalidate consumers of changed headers.
    if !changed_headers.is_empty() {
        let mut q =
            db.prepare("SELECT DISTINCT source FROM reverse WHERE header = ?1")?;
        for hdr in &changed_headers {
            let rows = q.query_map(params![hdr.to_str().unwrap_or("")], |row| {
                row.get::<_, String>(0)
            })?;
            for row in rows {
                let src = PathBuf::from(row?);
                needs_scan.insert(src);
            }
        }
    }

    // Purge deleted entries.
    if !deleted.is_empty() {
        let mut df = db.prepare("DELETE FROM forward WHERE path = ?1")?;
        let mut dr = db.prepare("DELETE FROM reverse WHERE source = ?1 OR header = ?1")?;
        for path in &deleted {
            let p = path.to_str().unwrap_or("");
            df.execute(params![p]).ok();
            dr.execute(params![p]).ok();
        }
    }

    db.execute_batch("DROP TABLE IF EXISTS scan;").ok();

    Ok(Diff {
        needs_scan,
        deleted,
    })
}

/// Store a fresh scan result.
pub fn store_scan(
    db: &Connection,
    source: &Path,
    mtime: u64,
    headers: &[PathBuf],
) -> anyhow::Result<()> {
    let src = source.to_str().context("Non-UTF-8 source path in store_scan")?;
    let json = serde_json::to_string(
        &headers
            .iter()
            .map(|p| p.to_str().unwrap_or(""))
            .collect::<Vec<_>>(),
    )?;

    // Diff against old headers and update reverse index.
    let old_json: Option<String> = db
        .query_row(
            "SELECT headers FROM forward WHERE path = ?1",
            params![src],
            |row| row.get(0),
        )
        .ok();

    if let Some(ref old) = old_json {
        let old_list: Vec<String> = serde_json::from_str(old)?;
        let mut del = db.prepare("DELETE FROM reverse WHERE header = ?1 AND source = ?2")?;
        for h in &old_list {
            del.execute(params![h, src]).ok();
        }
    }

    db.execute(
        "INSERT OR REPLACE INTO forward VALUES (?1, ?2, ?3)",
        params![src, mtime as i64, &json],
    )?;

    let mut ins = db.prepare("INSERT OR IGNORE INTO reverse VALUES (?1, ?2)")?;
    for h in headers {
        ins.execute(params![h.to_str().unwrap_or(""), src]).ok();
    }

    Ok(())
}

/// Look up cached transitive headers for a source whose mtime hasn't changed.
/// Returns the full transitive closure (all headers this source transitively
/// includes), or `None` if the source's mtime doesn't match or there's no entry.
pub fn get_cached_headers(db: &Connection, path: &Path, mtime: u64) -> Option<Vec<PathBuf>> {
    let s = path.to_str()?;
    let (cached_mt, json): (i64, String) = db
        .query_row(
            "SELECT mtime, headers FROM forward WHERE path = ?1 AND mtime = ?2",
            params![s, mtime as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()?;
    if cached_mt != mtime as i64 {
        return None;
    }
    let list: Vec<String> = serde_json::from_str(&json).ok()?;
    Some(list.into_iter().map(PathBuf::from).collect())
}

fn is_header(name: &str) -> bool {
    name.ends_with(".h") || name.ends_with(".hpp") || name.ends_with(".hxx") || name.ends_with(".inl")
}

fn walk(root: &Path, f: &mut dyn FnMut(&str, u64)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, ".git" | ".claude" | "target" | "node_modules") {
                continue;
            }
            walk(&path, f);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if TRACKED_EXTS.contains(&ext) {
                if let Ok(meta) = std::fs::metadata(&path) {
                    if let Ok(mtime) = meta.modified() {
                        if let Ok(ns) = mtime.duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64)
                        {
                            if let Some(s) = path.to_str() {
                                f(s, ns);
                            }
                        }
                    }
                }
            }
        }
    }
}
