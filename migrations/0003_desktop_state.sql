CREATE TABLE codex_accounts (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    encrypted_blob BLOB NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    models_json TEXT,
    models_checked_at INTEGER,
    usage_json TEXT,
    usage_checked_at INTEGER,
    usage_attempted_at INTEGER,
    usage_error TEXT
);
ALTER TABLE app_settings ADD COLUMN active_codex_account_id TEXT REFERENCES codex_accounts(id);
ALTER TABLE client_keys ADD COLUMN encrypted_value BLOB;
CREATE TABLE model_tests (
    id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES codex_accounts(id),
    model_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('running','completed','failed','interrupted')),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    result_text TEXT,
    error TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER
);
CREATE INDEX model_tests_account_time ON model_tests(account_id, started_at DESC);
