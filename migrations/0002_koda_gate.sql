ALTER TABLE sessions ADD COLUMN workflow_type TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE sessions ADD COLUMN policy_snapshot_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE sessions ADD COLUMN evidence_hash TEXT;
ALTER TABLE sessions ADD COLUMN approval_state TEXT NOT NULL DEFAULT 'not_required';
ALTER TABLE sessions ADD COLUMN provider_id TEXT;
ALTER TABLE sessions ADD COLUMN model_id TEXT;
ALTER TABLE sessions ADD COLUMN estimated_cost_micros INTEGER;
ALTER TABLE sessions ADD COLUMN published_merge_request_url TEXT;
ALTER TABLE integrations ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'token';
ALTER TABLE project_bindings ADD COLUMN updated_at TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS encrypted_secrets (
    key TEXT PRIMARY KEY NOT NULL,
    ciphertext BLOB NOT NULL,
    nonce BLOB NOT NULL,
    key_version INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_configs (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    base_url TEXT,
    secret_key_ref TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS mcp_servers (
    id TEXT PRIMARY KEY NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    transport TEXT NOT NULL CHECK(transport IN ('remote_http','local_stdio')),
    endpoint TEXT,
    secret_key_ref TEXT,
    allowed_hosts_json TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_mcp_servers (
    project_binding_id TEXT NOT NULL REFERENCES project_bindings(id) ON DELETE CASCADE,
    mcp_server_id TEXT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY(project_binding_id, mcp_server_id)
);

CREATE TABLE IF NOT EXISTS skill_versions (
    id TEXT PRIMARY KEY NOT NULL,
    skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    content TEXT NOT NULL,
    checksum TEXT NOT NULL,
    created_by TEXT REFERENCES users(id),
    created_at TEXT NOT NULL,
    UNIQUE(skill_id, version)
);

CREATE TABLE IF NOT EXISTS project_skill_versions (
    project_binding_id TEXT NOT NULL REFERENCES project_bindings(id) ON DELETE CASCADE,
    skill_version_id TEXT NOT NULL REFERENCES skill_versions(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY(project_binding_id, skill_version_id)
);

CREATE TABLE IF NOT EXISTS session_artifacts (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS policy_evaluations (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    rule TEXT NOT NULL,
    verdict TEXT NOT NULL CHECK(verdict IN ('allow','deny','warn')),
    details_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS approvals (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    actor_user_id TEXT NOT NULL REFERENCES users(id),
    decision TEXT NOT NULL CHECK(decision IN ('approve','reject')),
    evidence_hash TEXT NOT NULL,
    reason TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(session_id, evidence_hash)
);

CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_user_id TEXT REFERENCES users(id),
    action TEXT NOT NULL,
    target_type TEXT,
    target_id TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_workflow ON sessions(workflow_type, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_artifacts_session ON session_artifacts(session_id, created_at);
CREATE INDEX IF NOT EXISTS idx_approvals_session ON approvals(session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_log(created_at DESC);
