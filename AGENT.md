# AGENT.md

Context for AI assistants working on oc-stats.

## Project Overview

oc-stats is a terminal dashboard for tracking OpenCode usage statistics. It reads usage data from the OpenCode SQLite database (or JSON export) and displays token usage, costs, model breakdown, and activity heatmap in a ratatui-based TUI.

## Build & Test Commands

```bash
cargo build          # Build the project
cargo build --release  # Release build
cargo test           # Run tests
cargo clippy         # Lint checks
cargo fmt            # Format code
cargo run            # Run the app
```

## Project Structure

```plaintext
src/
├── main.rs              # Entry point, CLI args
├── db/
│   ├── mod.rs
│   ├── connection.rs    # SQLite connection handling
│   ├── models.rs        # Data models (UsageEvent, TokenUsage, etc.)
│   └── queries.rs       # Database queries
├── cache/
│   ├── mod.rs
│   ├── http_client.rs   # HTTP client for remote pricing
│   ├── models_cache.rs  # Model pricing catalog, remote refresh
│   └── opencode_config.rs  # OpenCode config parsing
├── analytics/
│   ├── mod.rs           # Analytics snapshot builder
│   ├── daily.rs         # Daily aggregation
│   ├── weekly.rs        # Weekly aggregation
│   ├── monthly.rs       # Monthly aggregation
│   ├── model_stats.rs   # Model/provider statistics
│   └── heatmap_data.rs  # 365-day heatmap data
├── ui/
│   ├── mod.rs
│   ├── app.rs           # Main app state and event loop
│   ├── overview.rs      # Overview page rendering
│   ├── models.rs        # Models/Providers pages
│   ├── export.rs        # Share card generation
│   ├── theme.rs         # Dark/light themes
│   └── widgets/
│       ├── heatmap.rs   # Activity heatmap widget
│       ├── linechart.rs # Line chart widget
│       └── common.rs    # Shared UI utilities
└── utils/
    ├── mod.rs
    ├── formatting.rs    # Number/date formatting
    ├── pricing.rs       # Price calculation helpers
    └── time.rs          # Time range handling
```

## Key Data Models

- `UsageEvent`: A single AI interaction with tokens, model, timestamps
- `TokenUsage`: input, output, cache_read, cache_write counts
- `AppData`: All loaded data (events, messages, sessions)
- `PricingCatalog`: Model pricing with local cache and remote refresh
- `AnalyticsSnapshot`: Computed statistics for display

## Data Flow

1. `db/queries.rs::load_app_data()` loads from SQLite or JSON
2. `cache/models_cache.rs::PricingCatalog::load()` loads pricing (cached or remote)
3. `analytics/mod.rs::build_snapshot()` computes statistics
4. `ui/app.rs::App` runs the TUI event loop

## Key Rules

### Data Loading

- Default database: `%APPDATA%/opencode/opencode.db` (Windows), `~/.local/share/opencode/opencode.db` (Linux), `~/Library/Application Support/opencode/opencode.db` (macOS)
- Fallback to JSON export with `--json` flag
- Only assistant messages count toward usage

### Pricing

- Local cache: `~/.config/oc-stats/models.json`
- Remote source: `https://models.dev/api.json`
- Cache TTL: 1 hour
- User overrides in OpenCode config take priority
- Fallback: `cacheWrite = input`, `cacheRead = input * 0.1`
- Prefer stored cost from database when available

### UI

- Inline viewport (not alt screen), fixed height ~23 lines
- Pages: Overview, Models, Providers
- Time ranges: All, 30d, 7d (cycles with `r` key)
- Heatmap always shows 365 days

## Keybindings

- `Left/Right/Tab`: Switch pages
- `r`: Cycle time range
- `1/2/3`: Direct range selection
- `Ctrl+S`: Copy summary to clipboard
- `t`: Toggle theme
- `q/Esc`: Quit

## Code Style

- No comments unless requested
- Keep modules separated by concern
- Pure analytics functions where possible
- Handle missing data gracefully (use `Option`, defaults)
