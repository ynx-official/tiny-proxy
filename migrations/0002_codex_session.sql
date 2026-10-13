-- Keep the original SIWC registration and encrypted credentials intact. Codex
-- credentials must never be selected by a SIWC registration ID or vice versa.
CREATE TABLE codex_session (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    encrypted_blob BLOB NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    expires_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
