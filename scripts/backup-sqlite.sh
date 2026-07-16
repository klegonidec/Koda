#!/usr/bin/env bash
set -euo pipefail
: "${DATABASE_FILE:=/data/duo-bridge.db}"
: "${BACKUP_DIR:=/data/backups}"
mkdir -p "$BACKUP_DIR"
sqlite3 "$DATABASE_FILE" ".backup '$BACKUP_DIR/duo-bridge-$(date -u +%Y%m%dT%H%M%SZ).db'"
