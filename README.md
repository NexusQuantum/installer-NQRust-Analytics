# NexusQuantum Analytics Installer

An interactive Terminal User Interface (TUI) installer for deploying the NexusQuantum Analytics stack using Docker Compose.

## Overview

This installer provides a guided setup experience for the NexusQuantum Analytics platform, which includes:
- **Analytics Engine** - Core query processing engine
- **Ibis Server** - Python-based data transformation layer
- **Analytics Service** - AI-powered analytics assistance
- **Analytics UI** - Web-based user interface
- **Qdrant** - Vector database for embeddings
- **Northwind DB** - PostgreSQL demo database

## Prerequisites

Before running the installer, ensure you have:

1. **Docker & Docker Compose** - [Install Docker](https://docs.docker.com/get-docker/)
2. **Rust** (for building from source) - [Install Rust](https://rustup.rs/)
3. **GitHub Personal Access Token** (PAT) with `read:packages` scope
   - Required to pull container images from GitHub Container Registry (ghcr.io)
   - [Create a PAT](https://github.com/settings/tokens/new) with the `read:packages` permission

## Quick Start

### Option 1: Install Pre-built Binary (Recommended)

Untuk instalasi cepat menggunakan binary pre-built:

```bash
# Set GitHub token untuk akses private repository
export GITHUB_TOKEN="your_github_token_here"

# Download dan install
curl -fsSL https://raw.githubusercontent.com/YOUR_USERNAME/installer-analytics/main/install.sh | bash
```

**Catatan:** Karena repository ini private, Anda perlu GitHub Personal Access Token. Lihat [RELEASE.md](./RELEASE.md) untuk detail lengkap.

### Option 2: Build from Source

### 1. Clone the Repository

```bash
git clone https://github.com/NexusQuantum/installer-NQRust-Analytics.git
cd installer-NQRust-Analytics
```

### 2. Authenticate with GitHub Container Registry

```bash
docker login ghcr.io
# Username: your-github-username
# Password: your-personal-access-token (NOT your GitHub password)
```

### 3. Run the Installer

```bash
cargo run
```

## Usage Guide

The installer provides an interactive TUI with the following screens:

### 1. Confirmation Screen
- Shows whether `.env` and `config.yaml` files exist
- Options:
  - **Generate .env** - Create environment configuration
  - **Generate config.yaml** - Select AI provider configuration
  - **Proceed** - Start installation (only if both files exist)
  - **Cancel** - Exit installer

### 2. Environment Setup (if .env missing)
- Configure:
  - OpenAI API Key (required)
  - Generation Model (default: `gpt-4o-mini`)
  - UI Port (default: `3000`)
  - AI Service Port (default: `5555`)
- Navigation:
  - `↑/↓` - Move between fields
  - `Enter` - Edit field
  - `Ctrl+S` - Save and continue
  - `Esc` - Cancel

### 3. Config Selection (if config.yaml missing)
- Choose from 13+ AI provider templates:
  - OpenAI, Anthropic, Azure OpenAI
  - DeepSeek, Google Gemini, xAI Grok
  - Groq, Ollama, LM Studio
  - And more...
- Navigation:
  - `↑/↓` - Browse providers
  - `Enter` - Select provider
  - `Esc` - Cancel

### 4. Installation Progress
- Real-time logs of Docker Compose operations
- Progress bar showing completion percentage
- Service-by-service status updates

### 5. Success/Error Screen
- Shows installation result
- Displays full installation logs
- `Ctrl+C` to exit

## Configuration

### Environment Variables (.env)

The installer generates a `.env` file based on `.env.example`. Key variables:

```bash
# Service Ports
ANALYTICS_ENGINE_PORT=8080
ANALYTICS_UI_PORT=3000
IBIS_SERVER_PORT=8000
ANALYTICS_AI_SERVICE_PORT=5555

# AI Configuration
OPENAI_API_KEY=your-api-key-here
GENERATION_MODEL=gpt-4o-mini

# Database
POSTGRES_DB=northwind
POSTGRES_USER=demo
POSTGRES_PASSWORD=demo123
```

### AI Provider Configuration (config.yaml)

The installer uses modular templates from `config_templates/`:
- `common/` - Shared engine, pipeline, and settings
- `providers/` - Provider-specific configurations

Templates are embedded in the binary at compile time.

## Architecture

```
installer-analytics/
├── src/
│   ├── app/           # Application state and logic
│   ├── ui/            # TUI rendering components
│   ├── templates.rs   # Config template system
│   └── utils.rs       # File utilities
├── config_templates/  # Modular config templates
│   ├── common/        # Shared sections
│   └── providers/     # Provider-specific configs
├── bootstrap/         # Docker initialization scripts
├── docker-compose.yaml
├── env_template       # Template for .env generation
└── northwind.sql      # Demo database schema
```

## Troubleshooting

### "unauthorized" error when pulling images

**Problem**: Docker cannot pull images from `ghcr.io`

**Solution**: 
1. Create a GitHub Personal Access Token with `read:packages` scope
2. Run `docker login ghcr.io` and use your PAT as the password

### ".env file detected but doesn't exist"

**Problem**: The installer detects a `.env` file in a parent directory

**Solution**: This was fixed in recent versions. Update to the latest version or ensure no `.env` exists in parent directories.

### Port conflicts

**Problem**: Services fail to start due to port conflicts

**Solution**: Edit `.env` and change the conflicting ports:
```bash
ANALYTICS_UI_PORT=3001  # Change from 3000
```

### Build errors

**Problem**: Rust compilation fails

**Solution**:
```bash
# Clean build artifacts
cargo clean

# Rebuild
cargo build --release
```

## Releases

Aplikasi ini menggunakan sistem rilis otomatis dengan GitHub Actions. Setiap kali tag versi di-push (contoh: `v0.1.0`), GitHub akan otomatis:

- Build binary untuk Linux x86_64 (static musl)
- Membuat GitHub Release
- Upload binary sebagai release asset

Lihat [RELEASE.md](./RELEASE.md) untuk dokumentasi lengkap tentang:
- Cara membuat release
- Instalasi binary pre-built
- Konfigurasi GitHub token untuk private repository
- Troubleshooting

## Development

### Building from Source

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run tests
cargo test
```

### Project Structure

- **App State** (`src/app/mod.rs`) - Main application logic and state machine
- **UI Components** (`src/ui/`) - Ratatui-based TUI screens
- **Templates** (`src/templates.rs`) - Config generation system
- **Utils** (`src/utils.rs`) - File detection and project root resolution

## License

Copyright (c) Idham <idhammultazam7@gmail.com>

This project is licensed under the MIT License - see the [LICENSE](./LICENSE) file for details.

## Support

For issues and questions:
- GitHub Issues: [NexusQuantum/installer-NQRust-Analytics](https://github.com/NexusQuantum/installer-NQRust-Analytics/issues)
- Email: idhammultazam7@gmail.com
