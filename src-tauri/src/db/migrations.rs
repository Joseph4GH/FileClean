//! 可回滚的数据库迁移。版本号存放于 `PRAGMA user_version`。

use rusqlite::Connection;

use crate::error::AppResult;

pub const LATEST_VERSION: i64 = 1;

const V1: &str = r#"
-- 增量扫描缓存：以路径为主键，记录指纹与文件元数据。
CREATE TABLE IF NOT EXISTS files (
    path          TEXT PRIMARY KEY,
    root          TEXT NOT NULL,
    size          INTEGER NOT NULL,
    mtime         INTEGER NOT NULL,
    inode         INTEGER NOT NULL DEFAULT 0,
    partial_hash  TEXT,
    full_hash     TEXT,
    full_algo     TEXT,
    phash         INTEGER,
    audio_sig     TEXT,
    scanned_at    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_files_size   ON files(size);
CREATE INDEX IF NOT EXISTS idx_files_hash   ON files(full_hash);
CREATE INDEX IF NOT EXISTS idx_files_root   ON files(root);
CREATE INDEX IF NOT EXISTS idx_files_scanned ON files(scanned_at);

-- 扫描会话：既支持中断恢复，也支持把结果存成项目文件。
CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    payload     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at DESC);

-- 操作审计流水：每一次删除/替换/移动都留痕，便于追责与撤销。
CREATE TABLE IF NOT EXISTS action_journal (
    id          TEXT PRIMARY KEY,
    created_at  INTEGER NOT NULL,
    mode        TEXT NOT NULL,
    dry_run     INTEGER NOT NULL DEFAULT 0,
    succeeded   INTEGER NOT NULL DEFAULT 0,
    failed      INTEGER NOT NULL DEFAULT 0,
    freed_bytes INTEGER NOT NULL DEFAULT 0,
    undone      INTEGER NOT NULL DEFAULT 0,
    payload     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_journal_created ON action_journal(created_at DESC);
"#;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if current >= LATEST_VERSION {
        return Ok(());
    }

    if current < 1 {
        // execute_batch 本身是隐式事务，失败即整体回滚，不会留下半截 schema。
        conn.execute_batch(V1)?;
    }

    conn.pragma_update(None, "user_version", LATEST_VERSION)?;
    tracing::info!(from = current, to = LATEST_VERSION, "数据库迁移完成");
    Ok(())
}
