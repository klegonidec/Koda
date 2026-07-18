ALTER TABLE project_bindings ADD COLUMN github_repo_id TEXT;
ALTER TABLE project_bindings ADD COLUMN github_repo_full_name TEXT;

CREATE TABLE IF NOT EXISTS github_installations (
    id TEXT PRIMARY KEY NOT NULL,
    installation_id INTEGER NOT NULL UNIQUE,
    account_login TEXT NOT NULL,
    account_type TEXT NOT NULL DEFAULT 'Organization',
    repos_json TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_github_installations_account ON github_installations(account_login);
