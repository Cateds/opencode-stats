use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to build HTTP client: {0}")]
    HttpClientBuild(#[source] reqwest::Error),

    #[error("fetch {url} timed out")]
    HttpTimeout { url: String },

    #[error("failed to fetch {url}: {source}")]
    HttpFetch {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("request to {url} failed: {status}")]
    HttpStatus { url: String, status: u16 },

    #[error("failed to decode JSON response from {url}: {source}")]
    JsonDecode {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("could not determine cache directory")]
    CacheDirNotFound,

    #[error("failed to create cache dir {path}: {source}")]
    CacheDirCreate {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read cache file {path}: {source}")]
    CacheRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write cache file {path}: {source}")]
    CacheWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse cached models: {0}")]
    CacheParse(#[from] serde_json::Error),

    #[error("failed to determine current directory: {source}")]
    CurrentDir {
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read OpenCode config {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse OpenCode config {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: json5::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
