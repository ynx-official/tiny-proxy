-- Existing installations also start in follow-system mode.
ALTER TABLE app_settings ADD COLUMN theme_preference TEXT NOT NULL DEFAULT 'system'
    CHECK (theme_preference IN ('system', 'light', 'dark'));
