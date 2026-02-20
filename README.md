# Rustyfin

A **local-first** Jellyfin-class media server built entirely in Rust, designed for simplicity, performance, and self-hosting.

## Overview

Rustyfin is a lightweight, single-binary media server that provides a complete media management and streaming solution. Built with modern technologies and best practices, it offers powerful features while maintaining ease of deployment and operation.

### Key Features

- 🎬 **Complete Media Library Management**
  - Movies and TV series support with automatic metadata enrichment
  - Hierarchical organization (Series → Seasons → Episodes)
  - Multi-library support with configurable media paths

- 🔐 **Multi-User Authentication**
  - Secure user management with role-based access (admin/user)
  - Argon2 password hashing with JWT-based authentication
  - Per-user preferences and playback state tracking

- 📡 **Flexible Streaming**
  - Direct Play via HTTP Range requests (RFC 7233 compliant)
  - Adaptive HLS transcoding for broad device compatibility
  - Hardware-accelerated transcoding (NVENC, VAAPI, QSV, VideoToolbox)

- 🎨 **Rich Metadata & Artwork**
  - Integration with TMDB for movies and TV shows
  - Automatic artwork download and caching with resizing
  - User-configurable metadata overrides with field locks

- 📝 **Subtitle Support**
  - Sidecar subtitle discovery (SRT, VTT, ASS, SUB, SSA, etc.)
  - Embedded subtitle track enumeration
  - Language, forced, and SDH/HI markers

- 📊 **Real-Time Updates**
  - Server-Sent Events (SSE) for live progress updates
  - Scan progress, metadata refresh, and job status notifications

- 🐳 **Docker-Ready**
  - Multi-stage Docker builds for minimal image size
  - Pre-configured compose files for CPU and GPU acceleration
  - Persistent volumes for configuration, cache, and media

## Architecture

Rustyfin is built as a **modular monolith** - a single server process with clear internal boundaries:

- **`crates/server`** - Axum web server with REST API endpoints
- **`crates/db`** - SQLite database layer with migrations and repositories
- **`crates/core`** - Shared domain types and business logic
- **`crates/scanner`** - Media file discovery and parsing
- **`crates/metadata`** - External metadata provider integration (TMDB)
- **`crates/transcoder`** - FFmpeg orchestration for transcoding
- **`ui`** - Next.js web application for user interface

## Tech Stack

- **Backend**: Rust 2024 Edition
- **Web Framework**: Axum with Tower middleware
- **Database**: SQLite with WAL mode
- **Media Processing**: FFmpeg & ffprobe
- **Authentication**: Argon2 + JWT
- **Frontend**: Next.js (TypeScript/React)
- **Streaming**: HLS with hls.js for adaptive playback

## Getting Started

### Prerequisites

- **Rust 1.83+** (for building from source)
- **FFmpeg** with ffprobe (required for media processing)
- **Node.js 18+** (for building the UI)

### Quick Start with Docker

The easiest way to run Rustyfin is with Docker:

```bash
# CPU-only mode
./scripts/docker-compose-safe.sh up -d

# NVIDIA GPU acceleration
./scripts/docker-compose-safe.sh -f docker-compose.gpu.yml up -d

# Intel/AMD VAAPI acceleration
./scripts/docker-compose-safe.sh -f docker-compose.vaapi.yml up -d
```

If you want this fix to be permanent for all new terminal sessions (so plain
`docker compose ...` also works), run once:

```bash
./scripts/install-tempdir-fix.sh
source ~/.zshrc
```

Default admin credentials on first run:
- **Username**: `admin`
- **Password**: `admin` (change immediately!)

The server will be available at `http://localhost:8096`

### Building from Source

```bash
# Clone the repository
git clone https://github.com/Iwan-Teague/Rustyfin.git
cd Rustyfin

# Build the backend
cargo build --release

# Build the UI
cd ui
npm install
npm run build
cd ..

# Run the server
./target/release/rustfin-server
```

The server will create a default SQLite database in `./config/rustfin.db` on first run.

### Configuration

Rustyfin follows a **database-first** configuration approach. All settings are stored in SQLite and can be managed through the web UI:

1. Log in with admin credentials
2. Navigate to the admin dashboard
3. Create libraries and configure media paths
4. Trigger library scans to populate your media collection

### Docker Volumes

The Docker setup uses the following persistent volumes:

- `/config` - Database and application configuration
- `/cache` - Metadata cache and artwork storage
- `/transcode` - Temporary transcoding files
- `/media` - Your media files (mount your media directories here)

## Features in Detail

### Metadata & Providers

Rustyfin automatically enriches your media with metadata from TMDB:

- Title, overview, release dates, and ratings
- Cast and crew information
- Episode listings for TV series
- Posters, backdrops, and artwork

Provider IDs can be embedded in folder names using the format `[tmdb=12345]` or managed through the UI.

### Playback Sessions

The server intelligently decides the best playback method:

- **Direct Play**: Stream the file as-is when the client supports it
- **Remux**: Change container format without re-encoding
- **Transcode**: Re-encode video/audio for compatibility

Progress tracking automatically syncs across devices for a seamless experience.

### Missing Episodes

For TV series, Rustyfin can display placeholder entries for missing episodes based on metadata from providers, helping you track what's missing from your collection.

### Hardware Acceleration

When available, Rustyfin can leverage GPU acceleration for transcoding:

- **NVIDIA**: NVENC (H.264/HEVC encoding)
- **Intel**: Quick Sync Video (QSV)
- **AMD/Intel**: VAAPI
- **Apple**: VideoToolbox

Detection is automatic, and the best available accelerator is selected.

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test suites
cargo test --package rustfin-scanner
cargo test --package rustfin-server --test integration
```

### Code Quality

The project uses:
- `rustfmt` for code formatting
- `clippy` for linting

```bash
# Format code
cargo fmt

# Run linter
cargo clippy -- -D warnings
```

## Project Status

Rustyfin is in active development. Core features are implemented and tested:

- ✅ User authentication and management
- ✅ Library scanning and media discovery
- ✅ Metadata enrichment with TMDB
- ✅ Direct Play and HLS streaming
- ✅ Hardware-accelerated transcoding
- ✅ Subtitle support
- ✅ Docker deployment
- ✅ Web UI with video player

See the [implementation tracker](docs/project/RUSTFIN_AI_PROJECT_TRACKER.md) for detailed status.

## Contributing

Contributions are welcome! Please ensure:

1. Code follows the existing style (run `cargo fmt` and `cargo clippy`)
2. New features include tests
3. Commits are descriptive

## License

MIT License - see LICENSE file for details.

## Acknowledgments

- Inspired by [Jellyfin](https://jellyfin.org/)
- Built with [Axum](https://github.com/tokio-rs/axum)
- Metadata provided by [The Movie Database (TMDB)](https://www.themoviedb.org/)
