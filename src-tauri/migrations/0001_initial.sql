-- SORTD schema v1.
-- Timestamps are Unix seconds unless the column name ends in `_ms`.

-- Locations the user explicitly authorized via a native folder dialog.
-- `kind` is 'scan' (indexed) or 'destination' (organization target only).
CREATE TABLE locations (
    id           INTEGER PRIMARY KEY,
    path         TEXT NOT NULL UNIQUE,
    kind         TEXT NOT NULL CHECK (kind IN ('scan', 'destination')),
    added_at     INTEGER NOT NULL,
    last_scan_id INTEGER
);

CREATE TABLE scans (
    id            INTEGER PRIMARY KEY,
    location_id   INTEGER NOT NULL REFERENCES locations(id) ON DELETE CASCADE,
    root          TEXT NOT NULL,
    started_at    INTEGER NOT NULL,
    finished_at   INTEGER,
    status        TEXT NOT NULL CHECK (status IN ('running', 'completed', 'cancelled', 'failed', 'interrupted')),
    files_seen    INTEGER NOT NULL DEFAULT 0,
    dirs_seen     INTEGER NOT NULL DEFAULT 0,
    bytes_seen    INTEGER NOT NULL DEFAULT 0,
    files_added   INTEGER NOT NULL DEFAULT 0,
    files_updated INTEGER NOT NULL DEFAULT 0,
    files_removed INTEGER NOT NULL DEFAULT 0,
    errors        INTEGER NOT NULL DEFAULT 0,
    skipped       INTEGER NOT NULL DEFAULT 0,
    message       TEXT
);
CREATE INDEX idx_scans_location ON scans(location_id, started_at DESC);

CREATE TABLE files (
    id                  INTEGER PRIMARY KEY,
    location_id         INTEGER NOT NULL REFERENCES locations(id) ON DELETE CASCADE,
    path                TEXT NOT NULL UNIQUE,
    parent              TEXT NOT NULL,
    name                TEXT NOT NULL,
    extension           TEXT,
    size                INTEGER NOT NULL,
    created_at          INTEGER,
    modified_at         INTEGER,
    accessed_at         INTEGER,
    is_hidden           INTEGER NOT NULL DEFAULT 0,
    is_readonly         INTEGER NOT NULL DEFAULT 0,
    is_system           INTEGER NOT NULL DEFAULT 0,
    is_symlink          INTEGER NOT NULL DEFAULT 0,
    is_executable       INTEGER NOT NULL DEFAULT 0,
    is_placeholder      INTEGER NOT NULL DEFAULT 0,
    attributes          INTEGER NOT NULL DEFAULT 0,
    mime                TEXT,
    category            TEXT NOT NULL,
    category_confidence REAL NOT NULL DEFAULT 0,
    category_override   TEXT,
    -- Hash cache. Valid only while (size, modified_at) == (hashed_size, hashed_mtime).
    partial_hash        TEXT,
    full_hash           TEXT,
    hashed_size         INTEGER,
    hashed_mtime        INTEGER,
    last_seen_scan      INTEGER NOT NULL,
    first_seen_scan     INTEGER NOT NULL,
    -- Last scan in which size or modification time changed.
    changed_scan        INTEGER NOT NULL,
    indexed_at          INTEGER NOT NULL
);
CREATE INDEX idx_files_parent    ON files(parent);
CREATE INDEX idx_files_size      ON files(size DESC);
CREATE INDEX idx_files_modified  ON files(modified_at DESC);
CREATE INDEX idx_files_ext       ON files(extension);
CREATE INDEX idx_files_category  ON files(category);
CREATE INDEX idx_files_location  ON files(location_id, last_seen_scan);
CREATE INDEX idx_files_full_hash ON files(full_hash) WHERE full_hash IS NOT NULL;

CREATE TABLE directories (
    id             INTEGER PRIMARY KEY,
    location_id    INTEGER NOT NULL REFERENCES locations(id) ON DELETE CASCADE,
    path           TEXT NOT NULL UNIQUE,
    parent         TEXT,
    name           TEXT NOT NULL,
    depth          INTEGER NOT NULL,
    own_size       INTEGER NOT NULL DEFAULT 0,
    own_files      INTEGER NOT NULL DEFAULT 0,
    total_size     INTEGER NOT NULL DEFAULT 0,
    total_files    INTEGER NOT NULL DEFAULT 0,
    total_dirs     INTEGER NOT NULL DEFAULT 0,
    -- 0 when part of the subtree could not be read (permissions, errors,
    -- exclusions). Totals are then a lower bound, never an exact claim.
    complete       INTEGER NOT NULL DEFAULT 1,
    is_hidden      INTEGER NOT NULL DEFAULT 0,
    modified_at    INTEGER,
    last_seen_scan INTEGER NOT NULL
);
CREATE INDEX idx_dirs_parent   ON directories(parent);
CREATE INDEX idx_dirs_location ON directories(location_id, last_seen_scan);
CREATE INDEX idx_dirs_size     ON directories(total_size DESC);

-- Trigram full-text index over names and paths (substring matching).
CREATE VIRTUAL TABLE files_fts USING fts5(
    name, path,
    content = 'files', content_rowid = 'id',
    tokenize = 'trigram'
);
CREATE TRIGGER files_ai AFTER INSERT ON files BEGIN
    INSERT INTO files_fts(rowid, name, path) VALUES (new.id, new.name, new.path);
END;
CREATE TRIGGER files_ad AFTER DELETE ON files BEGIN
    INSERT INTO files_fts(files_fts, rowid, name, path) VALUES ('delete', old.id, old.name, old.path);
END;
CREATE TRIGGER files_au AFTER UPDATE OF name, path ON files BEGIN
    INSERT INTO files_fts(files_fts, rowid, name, path) VALUES ('delete', old.id, old.name, old.path);
    INSERT INTO files_fts(rowid, name, path) VALUES (new.id, new.name, new.path);
END;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Activity log. `details` is JSON metadata (paths, counts) — never file contents.
CREATE TABLE activity (
    id      INTEGER PRIMARY KEY,
    ts_ms   INTEGER NOT NULL,
    kind    TEXT NOT NULL,
    summary TEXT NOT NULL,
    details TEXT,
    batch_id INTEGER
);
CREATE INDEX idx_activity_ts ON activity(ts_ms DESC);

-- Operation journal. Items are written *before* they execute so an
-- interrupted run can be detected and reconciled on next start.
CREATE TABLE operation_batches (
    id           INTEGER PRIMARY KEY,
    created_at   INTEGER NOT NULL,
    executed_at  INTEGER,
    finished_at  INTEGER,
    label        TEXT NOT NULL,
    origin       TEXT NOT NULL,
    status       TEXT NOT NULL CHECK (status IN ('planned', 'running', 'completed', 'partial', 'failed', 'interrupted', 'undone', 'discarded')),
    dry_run      INTEGER NOT NULL DEFAULT 0,
    conflict_policy TEXT NOT NULL DEFAULT 'skip',
    allow_cross_volume INTEGER NOT NULL DEFAULT 0,
    total        INTEGER NOT NULL DEFAULT 0,
    succeeded    INTEGER NOT NULL DEFAULT 0,
    failed       INTEGER NOT NULL DEFAULT 0,
    skipped      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE operation_items (
    id            INTEGER PRIMARY KEY,
    batch_id      INTEGER NOT NULL REFERENCES operation_batches(id) ON DELETE CASCADE,
    seq           INTEGER NOT NULL,
    op            TEXT NOT NULL CHECK (op IN ('move', 'rename', 'copy', 'mkdir', 'trash')),
    source        TEXT,
    destination   TEXT,
    status        TEXT NOT NULL CHECK (status IN ('pending', 'running', 'done', 'failed', 'skipped', 'undone', 'undo_failed', 'interrupted')),
    issue         TEXT,
    error         TEXT,
    cross_volume  INTEGER NOT NULL DEFAULT 0,
    is_dir        INTEGER NOT NULL DEFAULT 0,
    -- Snapshot taken at preview time; re-validated immediately before execution.
    source_size   INTEGER,
    source_mtime  INTEGER,
    content_hash  TEXT,
    verified      INTEGER NOT NULL DEFAULT 0,
    started_ms    INTEGER,
    finished_ms   INTEGER
);
CREATE INDEX idx_op_items_batch ON operation_items(batch_id, seq);

CREATE TABLE recommendations (
    id          INTEGER PRIMARY KEY,
    file_id     INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    destination TEXT NOT NULL,
    reason      TEXT NOT NULL,
    signals     TEXT NOT NULL,
    confidence  REAL NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('pending', 'applied', 'dismissed')),
    created_at  INTEGER NOT NULL,
    UNIQUE (file_id)
);
CREATE INDEX idx_reco_status ON recommendations(status, confidence DESC);

CREATE TABLE search_history (
    id    INTEGER PRIMARY KEY,
    query TEXT NOT NULL UNIQUE,
    ts    INTEGER NOT NULL
);
