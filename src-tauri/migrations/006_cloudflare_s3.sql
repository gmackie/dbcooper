CREATE TABLE IF NOT EXISTS cloudflare_resources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    name TEXT NOT NULL,
    account_id TEXT NOT NULL,
    extra TEXT,
    synced_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(kind, account_id, resource_id)
);

ALTER TABLE connections ADD COLUMN extra TEXT;
