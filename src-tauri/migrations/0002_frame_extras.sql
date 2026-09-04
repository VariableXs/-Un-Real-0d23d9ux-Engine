-- Frame system extensions: rotation, grouping, visibility, collapse,
-- style presets and edge glow. All columns defaulted so existing rows upgrade
-- transparently and the migration is idempotent via user_version gating.
ALTER TABLE nodes ADD COLUMN rotation REAL NOT NULL DEFAULT 0;
ALTER TABLE nodes ADD COLUMN group_id TEXT;
ALTER TABLE nodes ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
ALTER TABLE nodes ADD COLUMN collapsed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE nodes ADD COLUMN preset TEXT NOT NULL DEFAULT '';
ALTER TABLE edges ADD COLUMN glow INTEGER NOT NULL DEFAULT 0;
