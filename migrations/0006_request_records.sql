CREATE TABLE request_records (
    id TEXT PRIMARY KEY NOT NULL,
    method TEXT NOT NULL,
    route TEXT NOT NULL,
    model TEXT NOT NULL DEFAULT '',
    key_id TEXT,
    account_id TEXT,
    stream INTEGER,
    body_bytes INTEGER,
    message_count INTEGER,
    tool_count INTEGER,
    outcome TEXT NOT NULL,
    phase TEXT NOT NULL,
    http_status INTEGER,
    upstream_status INTEGER,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    elapsed_ms INTEGER NOT NULL DEFAULT 0,
    first_byte_at INTEGER,
    first_output_at INTEGER,
    last_activity_at INTEGER,
    input_tokens INTEGER,
    output_tokens INTEGER
);
CREATE INDEX request_records_time ON request_records(started_at DESC, id);
CREATE TABLE failure_records (
    request_id TEXT PRIMARY KEY NOT NULL REFERENCES request_records(id) ON DELETE CASCADE,
    occurred_at INTEGER NOT NULL,
    phase TEXT NOT NULL,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    http_status INTEGER NOT NULL
);
CREATE INDEX failure_records_time ON failure_records(occurred_at DESC, request_id);
