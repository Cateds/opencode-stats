use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use rusqlite::OptionalExtension;

use crate::db::connection::{discover_database_path, open_database};
use crate::db::models::{
    AppData, DataSourceKind, InputOptions, JsonMessageRecord, SessionSummary, TokenUsage,
    UsageEvent,
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
    time_archived: Option<DateTime<Local>>,
}

pub fn load_app_data(options: &InputOptions) -> Result<AppData> {
    if let Some(json_path) = &options.json_path {
        return load_from_json(json_path);
    }

    let db_path = discover_database_path(options.database_path.as_deref()).with_context(|| {
        let candidates =
            crate::db::connection::default_database_candidates(options.database_path.as_deref())
                .into_iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
        format!("could not find a valid OpenCode database; checked: {candidates}")
    })?;
    load_from_sqlite(&db_path)
}

pub fn load_from_sqlite(db_path: &Path) -> Result<AppData> {
    let conn = open_database(db_path)?;

    let mut session_stmt = conn.prepare(
        "
        SELECT s.id, s.parent_id, s.title, s.time_created, s.time_archived,
               p.name as project_name, p.worktree as project_worktree
        FROM session s
        LEFT JOIN project p ON s.project_id = p.id
        ORDER BY s.time_created DESC
        ",
    )?;

    let sessions_iter = session_stmt.query_map([], |row| {
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
            time_archived: row
                .get::<_, Option<i64>>("time_archived")?
                .and_then(timestamp_ms_to_local),
        })
    })?;

    let mut all_events = Vec::new();
    for session in sessions_iter {
        let session = session?;
        let events = load_session_events_sqlite(&conn, &session)?;
        all_events.extend(events);
    }

    finalize_app_data(all_events, DataSourceKind::Sqlite)
}

fn load_session_events_sqlite(
    conn: &rusqlite::Connection,
    session: &SessionRow,
) -> Result<Vec<UsageEvent>> {
    let mut stmt =
        conn.prepare("SELECT data FROM message WHERE session_id = ? ORDER BY time_created ASC")?;

    let rows = stmt.query_map([&session.id], |row| row.get::<_, String>(0))?;
    let mut events = Vec::new();
    for row in rows {
        let payload = row?;
        let Some(event) = parse_message_payload(&payload, session, DataSourceKind::Sqlite)? else {
            continue;
        };
        if event.tokens.total() > 0 {
            events.push(event);
        }
    }
    Ok(events)
}

pub fn load_from_json(path: &Path) -> Result<AppData> {
    if path.is_dir() {
        return load_from_json_directory(path);
    }

    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read JSON file {}", path.display()))?;

    let json = serde_json::from_str::<serde_json::Value>(&contents)
        .with_context(|| format!("failed to parse JSON file {}", path.display()))?;

    match json {
        serde_json::Value::Array(items) => load_from_json_values(items, path),
        serde_json::Value::Object(_) => load_from_json_values(vec![json], path),
        _ => bail!("unsupported JSON input format at {}", path.display()),
    }
}

fn load_from_json_directory(path: &Path) -> Result<AppData> {
    let mut files = Vec::new();
    collect_json_files(path, &mut files)?;
    files.sort();

    let mut values = Vec::new();
    for file in files {
        let contents = fs::read_to_string(&file)
            .with_context(|| format!("failed to read JSON file {}", file.display()))?;
        let value = serde_json::from_str::<serde_json::Value>(&contents)
            .with_context(|| format!("failed to parse JSON file {}", file.display()))?;
        values.push(value);
    }

    load_from_json_values(values, path)
}

fn collect_json_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(path).with_context(|| format!("failed to read dir {}", path.display()))?
    {
        let entry = entry?;
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
    for value in values {
        let record: JsonMessageRecord = match serde_json::from_value(value) {
            Ok(record) => record,
            Err(_) => continue,
        };

        if record.role.as_deref() != Some("assistant") {
            continue;
        }

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
            time_archived: None,
        };

        let session_row = SessionRow {
            id: inferred_session_id.unwrap_or(session_row.id),
            ..session_row
        };

        if let Some(event) = parse_json_record(record, &session_row, DataSourceKind::Json)
            && event.tokens.total() > 0
        {
            all_events.push(event);
        }
    }

    finalize_app_data(all_events, DataSourceKind::Json)
}

fn parse_message_payload(
    payload: &str,
    session: &SessionRow,
    source: DataSourceKind,
) -> Result<Option<UsageEvent>> {
    let record: JsonMessageRecord = match serde_json::from_str(payload) {
        Ok(record) => record,
        Err(_) => return Ok(None),
    };

    Ok(parse_json_record(record, session, source))
}

fn parse_json_record(
    record: JsonMessageRecord,
    session: &SessionRow,
    source: DataSourceKind,
) -> Option<UsageEvent> {
    if record.role.as_deref() != Some("assistant") {
        return None;
    }

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

    let provider_id = record.provider_id.clone().or_else(|| {
        record
            .model
            .as_ref()
            .and_then(|model| model.provider_id.clone())
    });
    let model_id = record
        .model_id
        .or_else(|| record.model.and_then(|model| model.model_id))
        .unwrap_or_else(|| "unknown".to_string());

    let created_at = record
        .time
        .as_ref()
        .and_then(|time| time.created)
        .and_then(timestamp_ms_to_local);
    let completed_at = record
        .time
        .as_ref()
        .and_then(|time| time.completed)
        .and_then(timestamp_ms_to_local);

    Some(UsageEvent {
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
        stored_cost_usd: record
            .cost
            .filter(|value| *value > rust_decimal::Decimal::ZERO),
        source,
    })
}

fn finalize_app_data(events: Vec<UsageEvent>, source: DataSourceKind) -> Result<AppData> {
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

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(session_ids.iter()), |row| {
        Ok((
            row.get::<_, String>("tool_name")?,
            row.get::<_, String>("status")?,
            row.get::<_, i64>("count")?,
        ))
    })?;

    let mut stats = BTreeMap::new();
    for row in rows {
        let (tool_name, status, count) = row?;
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

    for project_dir in fs::read_dir(storage_dir)? {
        let project_dir = project_dir?;
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
    let mut stmt = conn.prepare(
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
    )?;

    let pattern = format!("%{}%", query.to_lowercase());
    let rows = stmt.query_map([pattern], |row| row.get::<_, String>(0))?;
    Ok(rows.filter_map(Result::ok).collect())
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
        .optional()?;
    Ok(count.unwrap_or_default() > 0)
}

#[cfg(test)]
mod tests {
    use super::parse_json_record;
    use crate::db::models::{
        DataSourceKind, JsonCacheTokensRecord, JsonMessageRecord, JsonPathRecord, JsonTimeRecord,
        JsonTokensRecord,
    };

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
            time_archived: None,
        };

        let event = parse_json_record(record, &session, DataSourceKind::Json).unwrap();
        assert_eq!(event.tokens.total(), 33);
        assert_eq!(event.model_id, "claude-sonnet-4.5");
        assert_eq!(event.provider_id.as_deref(), Some("anthropic"));
    }
}
