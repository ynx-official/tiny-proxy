-- Revocation now removes the whole key record. Retain the legacy column so
-- existing migration checksums and active digest-only keys remain compatible.
DELETE FROM client_keys WHERE revoked_at IS NOT NULL;
