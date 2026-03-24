use std::env;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::db::errors::{Error, Result};

pub fn default_database_candidates(custom_path: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = custom_path {
        candidates.push(path.to_path_buf());
    }

    if let Ok(path) = env::var("OCMONITOR_DATABASE_FILE") {
        candidates.push(PathBuf::from(path));
    }

    if let Some(home) = dirs::home_dir() {
        candidates.push(
            home.join(".local")
                .join("share")
                .join("opencode")
                .join("opencode.db"),
        );
    }

    if cfg!(target_os = "windows") {
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local_app_data)
                    .join("opencode")
                    .join("opencode.db"),
            );
        }
        if let Ok(appdata) = env::var("APPDATA") {
            candidates.push(PathBuf::from(appdata).join("opencode").join("opencode.db"));
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = dirs::home_dir() {
            candidates.push(
                home.join("Library")
                    .join("Application Support")
                    .join("opencode")
                    .join("opencode.db"),
            );
        }
    } else if let Some(data_dir) = dirs::data_local_dir() {
        candidates.push(data_dir.join("opencode").join("opencode.db"));
    }

    dedupe_preserve_order(candidates)
}

pub fn discover_database_path(custom_path: Option<&Path>) -> Option<PathBuf> {
    default_database_candidates(custom_path)
        .into_iter()
        .find(|candidate| {
            candidate.exists() && database_has_expected_tables(candidate).unwrap_or(false)
        })
}

pub fn open_database(path: &Path) -> Result<Connection> {
    Connection::open(path).map_err(|e| Error::database_open(path, e))
}

pub fn database_has_expected_tables(path: &Path) -> Result<bool> {
    let conn = Connection::open(path).map_err(|e| Error::database_open(path, e))?;

    for table in ["session", "message", "project"] {
        let exists = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .map_err(Error::database_query)?;
        if exists == 0 {
            return Ok(false);
        }
    }

    Ok(true)
}

fn dedupe_preserve_order(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for path in paths {
        if seen.insert(path.clone()) {
            result.push(path);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{database_has_expected_tables, default_database_candidates};
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    #[test]
    fn custom_path_has_priority() {
        let custom = Path::new("custom.db");
        let candidates = default_database_candidates(Some(custom));
        assert_eq!(candidates.first().unwrap(), custom);
    }

    #[test]
    fn rejects_sqlite_without_expected_schema() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("oc-stats-schema-test-{nonce}.db"));
        let conn = Connection::open(&db_path).unwrap();
        conn.execute("CREATE TABLE only_one(id INTEGER)", [])
            .unwrap();
        drop(conn);

        assert!(!database_has_expected_tables(&db_path).unwrap());
        let _ = fs::remove_file(db_path);
    }
}
