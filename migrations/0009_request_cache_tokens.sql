-- NULL preserves unknown cache usage for existing requests; never backfill zero.
ALTER TABLE request_records ADD COLUMN cached_input_tokens INTEGER;
