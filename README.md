# NexusQuantum Analytics Installer

An interactive Terminal User Interface (TUI) installer for deploying the NexusQuantum Analytics stack using Docker Compose.

## Overview

This installer provides a guided setup experience for the NexusQuantum Analytics platform, which includes:
- **Analytics Engine** - Core query processing engine
- **Ibis Server** - Python-based data transformation layer
- **Analytics Service** - AI-powered analytics assistance
- **Analytics UI** - Web-based user interface with user management
- **Qdrant** - Vector database for embeddings
- **Northwind DB** - PostgreSQL demo database

### New Features (v2.0+)
- **User Management** - Built-in authentication with email/password and optional OAuth (Google/GitHub)
- **Multi-Dashboard Support** - Create and manage multiple dashboards per user
- **Sharing** - Share dashboards and chat history with team members
- **Role-Based Access Control** - Admin, Editor, and Viewer roles

## Prerequisites

Before running the installer, ensure you have the **Docker stack** (required by `nqrust-analytics install`):

1. **Docker** (engine + CLI) — [Install Docker](https://docs.docker.com/get-docker/)
2. **Docker Compose v2** — `docker compose` (Compose v2 plugin; not legacy `docker-compose`)
3. **Docker Buildx** (BuildKit) — `docker buildx` (usually included with Docker CE)
4. **Access to Docker daemon** — run with `sudo` or add your user to the `docker` group

5. **Rust** (for building from source) — [Install Rust](https://rustup.rs/)
6. **GitHub Personal Access Token** (PAT) with `read:packages` scope
   - Required to pull container images from GitHub Container Registry (ghcr.io)
   - [Create a PAT](https://github.com/settings/tokens/new) with the `read:packages` permission

## Quick Start

### Option A: One-liner install (preferred)
```bash
curl -fsSL https://raw.githubusercontent.com/NexusQuantum/installer-NQRust-Analytics/main/scripts/install/install.sh | bash
```
Installs the latest `.deb` from GitHub Releases and makes `nqrust-analytics` available in `$PATH`. Then run:
```bash
nqrust-analytics install
```

### Option B: Install from release asset
1) Download the latest `.deb` from the [Releases](https://github.com/NexusQuantum/installer-NQRust-Analytics/releases) page. Example:
```bash
curl -LO https://github.com/NexusQuantum/installer-NQRust-Analytics/releases/latest/download/nqrust-analytics_*.deb
```
2) Install the package (adds the `nqrust-analytics` binary into `/usr/bin`):
```bash
sudo apt install ./nqrust-analytics_*.deb
# or: sudo dpkg -i nqrust-analytics_*.deb
```
3) Run the installer:
```bash
nqrust-analytics install
```
> Note: the binary name is `nqrust-analytics` (replaces the older `installer-analytics`).

### Option C: Build from source

1) Clone the repository
```bash
git clone https://github.com/NexusQuantum/installer-NQRust-Analytics.git
cd installer-NQRust-Analytics
```

2) Authenticate with GitHub Container Registry
```bash
docker login ghcr.io
# Username: your-github-username
# Password: your-personal-access-token (NOT your GitHub password)
```

3) Run the installer
```bash
cargo run
```

### Option D: Airgapped/Offline Installation

For environments **without internet access** (airgapped, isolated networks, or offline VMs):

**On a machine with internet (build machine):**
```bash
# 1. Clone and checkout airgapped branch
git clone https://github.com/NexusQuantum/installer-NQRust-Analytics.git
cd installer-NQRust-Analytics
git checkout airgapped-single-binary

# 2. Login to GitHub Container Registry
docker login ghcr.io

# 3. Build airgapped binary (~3-4 GB, includes all Docker images)
./scripts/airgapped/build-single-binary.sh
```

**Transfer to airgapped machine** (via USB/SCP/physical media):
```bash
# Copy the single binary file
cp nqrust-analytics-airgapped /path/to/transfer/
```

**On airgapped machine (no internet needed):**
```bash
# 0. (Optional) If Docker is not installed: use Docker airgapped installer first
#    See docs/AIRGAPPED-INSTALLATION.md — "Docker Airgapped Installer"

# 1. Make executable
chmod +x nqrust-analytics-airgapped

# 2. Run installer (auto-extracts and loads Docker images)
./nqrust-analytics-airgapped install
```

> 📖 **See [Airgapped Installation Guide](docs/AIRGAPPED-INSTALLATION.md) for complete instructions, Docker offline installer, and FAQ.**

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

# Document RAG (optional) — enables PDF upload, indexing, and Q&A.
# Leave blank to skip the document feature; SQL Q&A works without it.
# Get an API key at https://dash.pageindex.ai/
PAGEINDEX_API_KEY=

# Database
DB_TYPE=pg
PG_URL=postgres://demo:demo123@northwind-db:5432/northwind
POSTGRES_DB=northwind
POSTGRES_USER=demo
POSTGRES_PASSWORD=demo123

# Authentication (auto-generated by installer)
JWT_SECRET=<auto-generated-secure-key>

# OAuth Configuration (optional)
GOOGLE_OAUTH_ENABLED=false
GOOGLE_CLIENT_ID=
GOOGLE_CLIENT_SECRET=
GITHUB_OAUTH_ENABLED=false
GITHUB_CLIENT_ID=
GITHUB_CLIENT_SECRET=
```

**Note:** The installer automatically generates a secure `JWT_SECRET` for authentication. If you need to enable OAuth login, edit the `.env` file after installation and set the appropriate OAuth credentials.

### AI Provider Configuration (config.yaml)

The installer uses modular templates from `config_templates/`:
- `common/` - Shared engine, pipeline, and settings
- `providers/` - Provider-specific configurations

Templates are embedded in the binary at compile time.

## Post-Installation Setup

After the installer completes successfully:

1. **Access the application** at http://localhost:3000

2. **Register the first user account**
   - The first user to register will automatically become an administrator
   - Use a valid email address and secure password

3. **Optional: Enable OAuth login**
   - Edit `.env` and set `GOOGLE_OAUTH_ENABLED=true` or `GITHUB_OAUTH_ENABLED=true`
   - Add your OAuth client ID and secret from:
     - Google: https://console.cloud.google.com/apis/credentials
     - GitHub: https://github.com/settings/developers
   - Restart the services: `docker compose restart analytics-ui`

4. **Create your first dashboard**
   - Navigate to the Home page
   - Click "New Dashboard" to create a personalized dashboard
   - Start querying your data using natural language

5. **Invite team members**
   - Share dashboards with other users (view or edit permissions)
   - Share chat history threads for collaboration

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

### "password authentication failed for user \"analytics\"" / "Role \"analytics\" does not exist"

**Problem**: Analytics UI cannot connect to PostgreSQL; Postgres logs show that the role `analytics` does not exist.

**Cause**: PostgreSQL only runs scripts in `/docker-entrypoint-initdb.d/` when the data volume is **empty** (first run). If you had already run the stack before the analytics DB/user was added, the existing `northwind_data` volume was reused and the init script that creates the `analytics` user never ran.

**Solution** (pick one):

1. **Recommended (with current installer)**  
   Re-run the installer or `docker compose up -d` after pulling the latest compose bundle. The stack now includes an **analytics-db-init** service that runs idempotently after Postgres is up and creates the `analytics` user and database if missing. Ensure `scripts/ensure-analytics-db.sh` exists in your project directory (the installer writes it if missing).

2. **One-time manual fix**  
   Create the user and database once, then restart the UI:
   ```bash
   docker exec -i analytics-northwind-db-1 psql -U demo -d postgres -c "CREATE USER analytics WITH PASSWORD 'analytics123'; CREATE DATABASE analytics OWNER analytics; GRANT ALL PRIVILEGES ON DATABASE analytics TO analytics;"
   docker compose restart analytics-ui
   ```
   (Container name may differ; use `docker ps` to find the northwind-db container.)

3. **Fresh start (data loss)**  
   Remove the Postgres volume so init runs again:
   ```bash
   docker compose down
   docker volume rm analytics_northwind_data
   docker compose up -d
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
