use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use rusqlite::OptionalExtension;

use crate::db::connection::{self, discover_database_path, open_database};
use crate::db::errors::{Error, Result};
use crate::db::models::{
    AppData, DataSourceKind, ImportStats, InputOptions, JsonMessageRecord, MessageRecord,
    SessionRecord, SessionSummary, TokenUsage, UsageEvent,
};
use crate::utils::time::timestamp_ms_to_local;

#[derive(Clone, Debug)]
struct SessionRow {
    id: String,
    parent_id: Option<String>,
    project_name: Option<String>,
    project_worktree: Option<PathBuf>,
    title: Option<String>,
    time_created: Option<DateTime<Local>>,
    time_updated: Option<DateTime<Local>>,
    time_archived: Option<DateTime<Local>>,
}

pub fn load_app_data(options: &InputOptions) -> Result<AppData> {
    if let Some(json_path) = &options.json_path {
        return load_from_json(json_path);
    }

    let db_path = discover_database_path(options.database_path.as_deref()).ok_or_else(|| {
        Error::database_not_found(connection::default_database_candidates(
            options.database_path.as_deref(),
        ))
    })?;
    load_from_sqlite(&db_path)
}

pub fn load_from_sqlite(db_path: &Path) -> Result<AppData> {
    let conn = open_database(db_path)?;

    let mut session_stmt = conn
        .prepare(
            "
        SELECT s.id, s.parent_id, s.title, s.time_created, s.time_updated, s.time_archived,
               p.name as project_name, p.worktree as project_worktree
        FROM session s
        LEFT JOIN project p ON s.project_id = p.id
        ORDER BY s.time_created DESC
        ",
        )
        .map_err(Error::database_query)?;

    let sessions_iter = session_stmt
        .query_map([], |row| {
            Ok(SessionRow {
                id: row.get("id")?,
                parent_id: row.get("parent_id")?,
                project_name: row.get("project_name")?,
                project_worktree: row
                    .get::<_, Option<String>>("project_worktree")?
                    .map(PathBuf::from),
                title: row.get("title")?,
                time_created: row
                    .get::<_, Option<i64>>("time_created")?
                    .and_then(timestamp_ms_to_local),
                time_updated: row
                    .get::<_, Option<i64>>("time_updated")?
                    .and_then(timestamp_ms_to_local),
                time_archived: row
                    .get::<_, Option<i64>>("time_archived")?
                    .and_then(timestamp_ms_to_local),
            })
        })
        .map_err(Error::database_query)?;

    let mut sessions = Vec::new();
    for session in sessions_iter {
        sessions.push(session.map_err(Error::database_query)?);
    }

    let session_lookup = sessions
        .iter()
        .map(|session| (session.id.clone(), session.clone()))
        .collect::<BTreeMap<_, _>>();
    let sqlite_messages = load_messages_sqlite(&conn, &session_lookup)?;

    let mut all_events = Vec::new();
    let mut all_messages = Vec::new();
    let mut session_records = Vec::new();
    for message in sqlite_messages.messages {
        if let Some(event) = message.event {
            all_events.push(event);
        }
        all_messages.push(message.record);
    }
    for session in sessions {
        if let (Some(created_at), Some(updated_at)) = (session.time_created, session.time_updated) {
            session_records.push(SessionRecord {
                session_id: session.id.clone(),
                created_at,
                updated_at,
            });
        }
    }

    finalize_app_data(
        all_events,
        all_messages,
        session_records,
        ImportStats {
            skipped_sqlite_messages: sqlite_messages.skipped_messages,
            ..ImportStats::default()
        },
        DataSourceKind::Sqlite,
    )
}

struct ParsedMessage {
    record: MessageRecord,
    event: Option<UsageEvent>,
}

struct SqliteMessageLoad {
    messages: Vec<ParsedMessage>,
    skipped_messages: usize,
}

enum ParseMessagePayload {
    Parsed(Box<Option<ParsedMessage>>),
    InvalidJson,
}

fn load_messages_sqlite(
    conn: &rusqlite::Connection,
    sessions: &BTreeMap<String, SessionRow>,
) -> Result<SqliteMessageLoad> {
    let mut stmt = conn
        .prepare("SELECT session_id, data FROM message ORDER BY session_id ASC, time_created ASC")
        .map_err(Error::database_query)?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(Error::database_query)?;
    let mut messages = Vec::new();
    let mut skipped_messages = 0usize;
    for row in rows {
        let (session_id, payload) = row.map_err(Error::database_query)?;
        let Some(session) = sessions.get(&session_id) else {
            skipped_messages = skipped_messages.saturating_add(1);
            continue;
        };

        match parse_message_payload(&payload, session, DataSourceKind::Sqlite)? {
            ParseMessagePayload::Parsed(parsed) => {
                if let Some(message) = *parsed {
                    messages.push(message);
                }
            }
            ParseMessagePayload::InvalidJson => {
                skipped_messages = skipped_messages.saturating_add(1);
            }
        }
    }
    Ok(SqliteMessageLoad {
        messages,
        skipped_messages,
    })
}

pub fn load_from_json(path: &Path) -> Result<AppData> {
    if path.is_dir() {
        return load_from_json_directory(path);
    }

    let contents = fs::read_to_string(path).map_err(|e| Error::json_read(path, e))?;

    let json = serde_json::from_str::<serde_json::Value>(&contents)
        .map_err(|e| Error::json_parse(path, e))?;

    match json {
        serde_json::Value::Array(items) => load_from_json_values(items, path),
        serde_json::Value::Object(_) => load_from_json_values(vec![json], path),
        _ => Err(Error::unsupported_json_format(path)),
    }
}

fn load_from_json_directory(path: &Path) -> Result<AppData> {
    let mut files = Vec::new();
    collect_json_files(path, &mut files)?;
    files.sort();

    let mut values = Vec::new();
    for file in &files {
        let contents = fs::read_to_string(file).map_err(|e| Error::json_read(file, e))?;
        let value = serde_json::from_str::<serde_json::Value>(&contents)
            .map_err(|e| Error::json_parse(file, e))?;

        match value {
            serde_json::Value::Array(items) => values.extend(items),
            serde_json::Value::Object(_) => values.push(value),
            _ => return Err(Error::unsupported_json_format(file)),
        }
    }

    load_from_json_values(values, path)
}

fn collect_json_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(path).map_err(|e| Error::directory_read(path, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::directory_read(path, e))?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_json_files(&entry_path, files)?;
        } else if entry_path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(entry_path);
        }
    }
    Ok(())
}

fn load_from_json_values(values: Vec<serde_json::Value>, source_path: &Path) -> Result<AppData> {
    let mut all_events = Vec::new();
    let mut all_messages = Vec::new();
    let mut import_stats = ImportStats::default();
    for value in values {
        let record: JsonMessageRecord = match serde_json::from_value(value) {
            Ok(record) => record,
            Err(_) => {
                import_stats.skipped_json_records =
                    import_stats.skipped_json_records.saturating_add(1);
                continue;
            }
        };

        let session_id = source_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("json")
            .to_string();

        let inferred_session_id = record
            .path
            .as_ref()
            .and_then(|path| {
                path.cwd
                    .as_ref()
                    .or(path.root.as_ref())
                    .and_then(|candidate| candidate.file_name())
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned)
            })
            .filter(|value| value.starts_with("ses_"));

        let session_row = SessionRow {
            id: session_id,
            parent_id: None,
            project_name: None,
            project_worktree: None,
            title: None,
            time_created: None,
            time_updated: None,
            time_archived: None,
        };

        let session_row = SessionRow {
            id: inferred_session_id.unwrap_or(session_row.id),
            ..session_row
        };

        if let Some(message) = parse_json_record(record, &session_row, DataSourceKind::Json) {
            if let Some(event) = message.event {
                all_events.push(event);
            }
            all_messages.push(message.record);
        }
    }

    finalize_app_data(
        all_events,
        all_messages,
        Vec::new(),
        import_stats,
        DataSourceKind::Json,
    )
}

fn parse_message_payload(
    payload: &str,
    session: &SessionRow,
    source: DataSourceKind,
) -> Result<ParseMessagePayload> {
    let record: JsonMessageRecord = match serde_json::from_str(payload) {
        Ok(record) => record,
        Err(_) => return Ok(ParseMessagePayload::InvalidJson),
    };

    Ok(ParseMessagePayload::Parsed(Box::new(parse_json_record(
        record, session, source,
    ))))
}

fn parse_json_record(
    record: JsonMessageRecord,
    session: &SessionRow,
    source: DataSourceKind,
) -> Option<ParsedMessage> {
    let role = normalize_optional_text(record.role.clone());
    let provider_id = normalize_optional_text(record.provider_id.clone()).or_else(|| {
        record
            .model
            .as_ref()
            .and_then(|model| normalize_optional_text(model.provider_id.clone()))
    });
    let model_id = normalize_optional_text(record.model_id.clone()).or_else(|| {
        record
            .model
            .as_ref()
            .and_then(|model| normalize_optional_text(model.model_id.clone()))
    });
    let created_at = record
        .time
        .as_ref()
        .and_then(|time| time.created)
        .and_then(timestamp_ms_to_local);

    let message = MessageRecord {
        session_id: session.id.clone(),
        role: role.clone(),
        provider_id: provider_id.clone(),
        model_id: model_id.clone(),
        created_at,
        source,
    };

    if role.as_deref() != Some("assistant") {
        return Some(ParsedMessage {
            record: message,
            event: None,
        });
    }

    let model_id = model_id?;

    let tokens = TokenUsage {
        input: record
            .tokens
            .as_ref()
            .and_then(|value| value.input)
            .unwrap_or(0),
        output: record
            .tokens
            .as_ref()
            .and_then(|value| value.output)
            .unwrap_or(0),
        cache_read: record
            .tokens
            .as_ref()
            .and_then(|value| value.cache.as_ref())
            .and_then(|value| value.read)
            .unwrap_or(0),
        cache_write: record
            .tokens
            .as_ref()
            .and_then(|value| value.cache.as_ref())
            .and_then(|value| value.write)
            .unwrap_or(0),
    };

    let completed_at = record
        .time
        .as_ref()
        .and_then(|time| time.completed)
        .and_then(timestamp_ms_to_local);

    let event = UsageEvent {
        session_id: session.id.clone(),
        parent_session_id: session.parent_id.clone(),
        session_title: session.title.clone(),
        session_started_at: session.time_created,
        session_archived_at: session.time_archived,
        project_name: session.project_name.clone(),
        project_path: record
            .path
            .as_ref()
            .and_then(|path| path.cwd.clone().or(path.root.clone()))
            .or_else(|| session.project_worktree.clone()),
        provider_id,
        model_id,
        agent: record.agent,
        finish_reason: record.finish,
        tokens,
        created_at,
        completed_at,
        stored_cost_usd: record.cost,
        source,
    };

    Some(ParsedMessage {
        record: message,
        event: (event.tokens.total() > 0).then_some(event),
    })
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn finalize_app_data(
    events: Vec<UsageEvent>,
    messages: Vec<MessageRecord>,
    session_records: Vec<SessionRecord>,
    import_stats: ImportStats,
    source: DataSourceKind,
) -> Result<AppData> {
    let mut grouped: BTreeMap<String, Vec<UsageEvent>> = BTreeMap::new();
    for event in events {
        grouped
            .entry(event.session_id.clone())
            .or_default()
            .push(event);
    }

    let mut sessions = Vec::new();
    let mut flattened = Vec::new();
    for (session_id, mut events) in grouped {
        events.sort_by_key(|event| event.created_at);
        if let Some(summary) = SessionSummary::from_events(session_id, events.clone()) {
            flattened.extend(events);
            sessions.push(summary);
        }
    }

    sessions.sort_by_key(|session| session.start_time());
    sessions.reverse();
    flattened.sort_by_key(|event| event.created_at);

    Ok(AppData {
        events: flattened,
        messages,
        session_records,
        import_stats,
        sessions,
        source,
    })
}

#[allow(dead_code)]
pub fn load_tool_usage_for_sessions(
    db_path: &Path,
    session_ids: &[String],
) -> Result<BTreeMap<String, (u64, u64)>> {
    if session_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let conn = open_database(db_path)?;
    let placeholders = session_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "
        SELECT json_extract(data, '$.tool') as tool_name,
               json_extract(data, '$.state.status') as status,
               COUNT(*) as count
        FROM part
        WHERE session_id IN ({})
          AND json_valid(data) = 1
          AND json_extract(data, '$.type') = 'tool'
          AND json_extract(data, '$.tool') IS NOT NULL
          AND json_extract(data, '$.state.status') IN ('completed', 'error')
        GROUP BY tool_name, status
        ",
        placeholders
    );

    let mut stmt = conn.prepare(&query).map_err(Error::database_query)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(session_ids.iter()), |row| {
            Ok((
                row.get::<_, String>("tool_name")?,
                row.get::<_, String>("status")?,
                row.get::<_, i64>("count")?,
            ))
        })
        .map_err(Error::database_query)?;

    let mut stats = BTreeMap::new();
    for row in rows {
        let (tool_name, status, count) = row.map_err(Error::database_query)?;
        let count = count.max(0) as u64;
        let entry = stats.entry(tool_name).or_insert((0, 0));
        if status == "completed" {
            entry.0 += count;
        } else {
            entry.1 += count;
        }
    }

    Ok(stats)
}

#[allow(dead_code)]
pub fn maybe_read_session_title_from_storage(session_id: &str) -> Result<Option<String>> {
    let storage_dir =
        dirs::data_local_dir().map(|path| path.join("opencode").join("storage").join("session"));
    let Some(storage_dir) = storage_dir else {
        return Ok(None);
    };
    if !storage_dir.exists() {
        return Ok(None);
    }

    let entries = fs::read_dir(&storage_dir).map_err(|e| Error::directory_read(&storage_dir, e))?;
    for project_dir in entries {
        let project_dir = project_dir.map_err(|e| Error::directory_read(&storage_dir, e))?;
        let session_file = project_dir.path().join(format!("{}.json", session_id));
        if !session_file.exists() {
            continue;
        }
        let contents = fs::read_to_string(&session_file).ok();
        let value = contents
            .as_deref()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
        let title = value
            .and_then(|json| json.get("title").cloned())
            .and_then(|value| value.as_str().map(ToOwned::to_owned));
        if title.is_some() {
            return Ok(title);
        }
    }

    Ok(None)
}

#[allow(dead_code)]
pub fn load_database_path_if_available(custom: Option<&Path>) -> Option<PathBuf> {
    discover_database_path(custom)
}

#[allow(dead_code)]
pub fn find_matching_models(db_path: &Path, query: &str) -> Result<Vec<String>> {
    let conn = open_database(db_path)?;
    let mut stmt = conn
        .prepare(
            "
        SELECT DISTINCT COALESCE(
            json_extract(data, '$.modelID'),
            json_extract(data, '$.model.modelID'),
            'unknown'
        ) as model_name
        FROM message
        WHERE json_valid(data) = 1
          AND json_extract(data, '$.role') = 'assistant'
          AND LOWER(COALESCE(
            json_extract(data, '$.modelID'),
            json_extract(data, '$.model.modelID'),
            'unknown'
          )) LIKE ?
        ORDER BY model_name
        ",
        )
        .map_err(Error::database_query)?;

    let pattern = format!("%{}%", query.to_lowercase());
    let rows = stmt
        .query_map([pattern], |row| row.get::<_, String>(0))
        .map_err(Error::database_query)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[allow(dead_code)]
pub fn detect_session_title_for_event(event: &UsageEvent) -> Option<String> {
    event.session_title.clone().or_else(|| {
        maybe_read_session_title_from_storage(&event.session_id)
            .ok()
            .flatten()
    })
}

#[allow(dead_code)]
pub fn session_has_messages(db_path: &Path, session_id: &str) -> Result<bool> {
    let conn = open_database(db_path)?;
    let count: Option<i64> = conn
        .query_row(
            "SELECT COUNT(*) FROM message WHERE session_id = ?",
            [session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Error::database_query)?;
    Ok(count.unwrap_or_default() > 0)
}

#[cfg(test)]
mod tests {
    use super::{load_from_json_values, load_from_sqlite, parse_json_record};
    use crate::db::models::{
        DataSourceKind, JsonCacheTokensRecord, JsonMessageRecord, JsonPathRecord, JsonTimeRecord,
        JsonTokensRecord,
    };
    use rusqlite::Connection;
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_assistant_json_record() {
        let record = JsonMessageRecord {
            role: Some("assistant".to_string()),
            provider_id: Some("anthropic".to_string()),
            model_id: Some("claude-sonnet-4.5".to_string()),
            tokens: Some(JsonTokensRecord {
                input: Some(10),
                output: Some(20),
                cache: Some(JsonCacheTokensRecord {
                    read: Some(1),
                    write: Some(2),
                }),
            }),
            time: Some(JsonTimeRecord {
                created: Some(1_710_000_000_000),
                completed: Some(1_710_000_001_000),
            }),
            path: Some(JsonPathRecord {
                cwd: Some("C:/repo".into()),
                root: None,
            }),
            agent: Some("build".to_string()),
            finish: Some("stop".to_string()),
            cost: None,
            model: None,
        };

        let session = super::SessionRow {
            id: "ses_1".to_string(),
            parent_id: None,
            project_name: None,
            project_worktree: None,
            title: None,
            time_created: None,
            time_updated: None,
            time_archived: None,
        };

        let parsed = parse_json_record(record, &session, DataSourceKind::Json).unwrap();
        let event = parsed.event.unwrap();
        assert_eq!(event.tokens.total(), 33);
        assert_eq!(event.model_id, "claude-sonnet-4.5");
        assert_eq!(event.provider_id.as_deref(), Some("anthropic"));
    }

    #[test]
    fn tracks_skipped_json_records_during_import() {
        let path = Path::new("/tmp/import.json");
        let data = load_from_json_values(
            vec![
                json!({
                    "role": "assistant",
                    "providerID": "openai",
                    "modelID": "gpt-5",
                    "tokens": { "input": 10, "output": 20 },
                    "time": { "created": 1_710_000_000_000i64, "completed": 1_710_000_001_000i64 }
                }),
                json!(42),
            ],
            path,
        )
        .unwrap();

        assert_eq!(data.import_stats.skipped_json_records, 1);
        assert_eq!(data.messages.len(), 1);
        assert_eq!(data.events.len(), 1);
    }

    #[test]
    fn tracks_skipped_sqlite_messages_during_import() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("oc-stats-load-test-{nonce}.db"));
        let conn = Connection::open(&db_path).unwrap();

        conn.execute_batch(
            "
            CREATE TABLE project (id TEXT PRIMARY KEY, name TEXT, worktree TEXT);
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                parent_id TEXT,
                title TEXT,
                time_created INTEGER,
                time_updated INTEGER,
                time_archived INTEGER
            );
            CREATE TABLE message (
                session_id TEXT,
                data TEXT,
                time_created INTEGER
            );
            ",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO project (id, name, worktree) VALUES (?1, ?2, ?3)",
            ("proj_1", "demo", "/tmp/demo"),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session (id, project_id, title, time_created, time_updated) VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_1", "proj_1", "Demo", 1_710_000_000_000i64, 1_710_000_001_000i64),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (session_id, data, time_created) VALUES (?1, ?2, ?3)",
            (
                "ses_1",
                r#"{"role":"assistant","providerID":"openai","modelID":"gpt-5","tokens":{"input":10,"output":20},"time":{"created":1710000000000,"completed":1710000001000}}"#,
                1_710_000_000_000i64,
            ),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (session_id, data, time_created) VALUES (?1, ?2, ?3)",
            ("ses_1", "not-json", 1_710_000_000_001i64),
        )
        .unwrap();
        drop(conn);

        let data = load_from_sqlite(&db_path).unwrap();

        assert_eq!(data.import_stats.skipped_sqlite_messages, 1);
        assert_eq!(data.messages.len(), 1);
        assert_eq!(data.events.len(), 1);

        let _ = fs::remove_file(db_path);
    }
}
