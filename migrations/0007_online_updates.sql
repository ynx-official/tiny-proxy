CREATE TABLE update_preferences (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    check_mode TEXT NOT NULL DEFAULT 'startup' CHECK (check_mode IN ('startup', 'interval', 'disabled')),
    interval_hours INTEGER NOT NULL DEFAULT 24 CHECK (interval_hours BETWEEN 1 AND 8760),
    notify INTEGER NOT NULL DEFAULT 1 CHECK (notify IN (0, 1)),
    last_checked_at INTEGER NOT NULL DEFAULT 0
);
INSERT INTO update_preferences (singleton_id) VALUES (1);
