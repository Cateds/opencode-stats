use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to open database at '{path}': {source}")]
    DatabaseOpen {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("could not find a valid OpenCode database; checked: {}", format_candidates(.candidates))]
    DatabaseNotFound { candidates: Vec<PathBuf> },

    #[error("database query failed: {source}")]
    DatabaseQuery {
        #[source]
        source: rusqlite::Error,
    },

    #[error("failed to read JSON file '{path}': {source}")]
    JsonRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse JSON file '{path}': {source}")]
    JsonParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("unsupported JSON input format at '{path}'")]
    UnsupportedJsonFormat { path: PathBuf },

    #[error("failed to read directory '{path}': {source}")]
    DirectoryRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn format_candidates(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl Error {
    pub fn database_open(path: impl Into<PathBuf>, source: rusqlite::Error) -> Self {
        Self::DatabaseOpen {
            path: path.into(),
            source,
        }
    }

    pub fn database_not_found(candidates: Vec<PathBuf>) -> Self {
        Self::DatabaseNotFound { candidates }
    }

    pub fn database_query(source: rusqlite::Error) -> Self {
        Self::DatabaseQuery { source }
    }

    pub fn json_read(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::JsonRead {
            path: path.into(),
            source,
        }
    }

    pub fn json_parse(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::JsonParse {
            path: path.into(),
            source,
        }
    }

    pub fn unsupported_json_format(path: impl Into<PathBuf>) -> Self {
        Self::UnsupportedJsonFormat { path: path.into() }
    }

    pub fn directory_read(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::DirectoryRead {
            path: path.into(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
