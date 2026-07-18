# Koda — Developer Guide

## Prerequisites

| Requirement | Linux (Debian/Ubuntu) | macOS | Windows |
|---|---|---|---|
| **Git** | `sudo apt install git` | `brew install git` or Xcode CLT | `winget install Git.Git` |
| **Rust** (1.85) | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` | same | [rustup-init.exe](https://rustup.rs) |
| **Node.js** (22+) | `sudo apt install nodejs npm` *(see note)* | `brew install node` | `winget install OpenJS.NodeJS.LTS` |
| **SQLite** (CLI, optional) | `sudo apt install sqlite3` | `brew install sqlite` | `winget install SQLite.SQLite` |
| **Docker** (optional) | [docs.docker.com/engine/install](https://docs.docker.com/engine/install/ubuntu/) | [Docker Desktop](https://docs.docker.com/docker-for-mac/install/) | [Docker Desktop](https://docs.docker.com/docker-for-windows/install/) |
| **Docker Compose** (optional) | included with Docker Engine | included with Docker Desktop | included with Docker Desktop |

> **Node.js on Ubuntu/Debian:** The system `apt` package may be outdated. Prefer:
> ```bash
> curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash -
> sudo apt install -y nodejs
> ```

---

## 1. Clone & Enter the Repository

```bash
git clone <repository-url> koda
cd koda
```

---

## 2. Configure Environment

Copy the example environment file and adjust values:

```bash
cp .env.example .env
```

Edit `.env` with your own secrets. At a minimum, change these for local development:

```
APP_SETUP_PASSWORD=dev-setup-password
APP_MASTER_KEY=$(openssl rand -base64 32)
```

> **Generate a secure master key:**
> - Linux/macOS: `openssl rand -base64 32`
> - Windows: PowerShell: `[Convert]::ToBase64String((1..32 | ForEach-Object { Get-Random -Max 256 }))`

---

## 3. Create Data Directory

SQLite stores the database file here:

```bash
# Linux / macOS
mkdir -p data

# Windows (PowerShell)
New-Item -ItemType Directory -Force -Path data
```

---

## 4. Build & Run the Backend

The Rust binary compiles and embeds SQLite migrations, then serves the API on `http://localhost:8080`.

```bash
# Build and run (auto-runs migrations on first start)
cargo run
```

> **Windows note:** If you encounter a linker error, install [Build Tools for Visual Studio 2022](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022) with the "Desktop development with C++" workload. Alternatively, use `rustup default stable-msvc` after installing.

For faster iteration, you can also run in release mode:

```bash
cargo run --release
```

---

## 5. Build & Serve the Frontend

The React frontend is served as static files by the Rust backend. Build it once, then the backend serves it automatically.

```bash
cd frontend
npm install
npm run build
cd ..
```

For frontend development with hot-reload, run the Vite dev server separately:

```bash
cd frontend
npm install
npm run dev
```

Then configure the backend's `KODA_STATIC_DIR` (or rely on the built files in `frontend/dist/`).

---

## 6. Open the App

Visit **[http://localhost:8080/setup](http://localhost:8080/setup)** and enter the `APP_SETUP_PASSWORD` from your `.env` to bootstrap the admin account (password must be ≥ 14 characters).

---

## Docker Setup (Alternative)

If you prefer to run everything in containers (including the OpenCode sidecar and egress proxy):

```bash
# Build all images
docker compose build

# Start services in the background
docker compose up -d

# Follow logs
docker compose logs -f

# Stop
docker compose down
```

The app will be available at `http://localhost:8080`.

---

## Platform-Specific Notes

### Linux (Debian/Ubuntu)

```bash
# Install system dependencies for Rust compilation
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev sqlite3

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Install Node.js 22
curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash -
sudo apt install -y nodejs

# Verify
rustc --version    # should be 1.85.0
node --version     # should be v24.x

# Run
cp .env.example .env
# edit .env
mkdir -p data
cargo run
```

### macOS

```bash
# Install Homebrew if not present
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install dependencies
brew install rust node pkg-config openssl sqlite

# Verify
rustc --version    # should be 1.85.0
node --version     # should be v24.x

# Run
cp .env.example .env
# edit .env
mkdir -p data
cargo run
```

### Windows (PowerShell)

```powershell
# Install Rust (download and run rustup-init.exe, then restart shell)
# https://rustup.rs

# Install Node.js
winget install OpenJS.NodeJS.LTS

# Verify
rustc --version
node --version

# Run
Copy-Item .env.example .env
# edit .env
New-Item -ItemType Directory -Force -Path data
cargo run
```

On Windows, use the `stable-msvc` Rust toolchain. Install [Build Tools for Visual Studio 2022](https://visualstudio.microsoft.com/downloads/?q=build+tools#build-tools-for-visual-studio-2022) (select "Desktop development with C++") if you hit linker errors.

---

## Useful Commands

| Action | Command |
|---|---|
| Build backend (debug) | `cargo build` |
| Build backend (release) | `cargo build --release` |
| Run backend | `cargo run` |
| Run tests | `cargo test` |
| Lint | `cargo clippy` |
| Format check | `cargo fmt --check` |
| Install frontend deps | `cd frontend && npm install` |
| Build frontend | `cd frontend && npm run build` |
| Frontend dev server | `cd frontend && npm run dev` |
| Docker build | `docker compose build` |
| Docker up | `docker compose up -d` |
| Docker logs | `docker compose logs -f` |
| Docker down | `docker compose down` |
| Open SQLite shell | `sqlite3 data/koda.db` |

---

## Environment Variables

See `.env.example` for all variables. Key ones for local dev:

| Variable | Default | Notes |
|---|---|---|
| `APP_BIND` | `0.0.0.0:8080` | Listen address |
| `APP_BASE_URL` | `http://localhost:8080` | Public-facing URL |
| `DATABASE_URL` | `sqlite:///data/koda.db?mode=rwc` | Auto-creates file |
| `APP_SETUP_PASSWORD` | `change-me-before-production` | Bootstrap admin password |
| `APP_MASTER_KEY` | — | 32-byte base64 key (generate with `openssl rand -base64 32`) |
| `RUST_LOG` | `info,koda=debug` | Logging verbosity |

Secrets can also be injected via `*_FILE` environment variables (e.g. `APP_MASTER_KEY_FILE=/run/secrets/master_key`).
