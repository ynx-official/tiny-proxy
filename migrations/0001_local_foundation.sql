CREATE TABLE account_registrations (
    id TEXT PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL UNIQUE CHECK (client_id <> 'dynamic_agent_client'),
    provider_kind TEXT NOT NULL DEFAULT 'siwc',
    issuer TEXT NOT NULL DEFAULT 'https://auth.openai.com',
    subject TEXT,
    label TEXT NOT NULL DEFAULT '',
    permission_state TEXT NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE app_settings (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    host_id TEXT NOT NULL,
    active_registration_id TEXT REFERENCES account_registrations(id) ON DELETE SET NULL
);
CREATE TABLE credential_bundles (
    registration_id TEXT PRIMARY KEY NOT NULL REFERENCES account_registrations(id) ON DELETE CASCADE,
    encrypted_blob BLOB NOT NULL,
    protection_kind TEXT NOT NULL DEFAULT 'windows-dpapi-user',
    format_version INTEGER NOT NULL DEFAULT 1,
    generation INTEGER NOT NULL CHECK (generation > 0),
    expires_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE client_keys (
    id TEXT PRIMARY KEY NOT NULL,
    label TEXT NOT NULL,
    public_prefix TEXT NOT NULL,
    secret_digest BLOB NOT NULL CHECK (length(secret_digest) = 32),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_used_at INTEGER,
    revoked_at INTEGER
);
