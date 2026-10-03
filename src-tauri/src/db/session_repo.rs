//! 会话仓储：扫描结果可存为项目文件，支持中断恢复与二次加载。
//!
//! 会话既写进 SQLite（便于列表查询），也可导出为独立的 `.fileclean.json`
//! 便于跨机器携带 —— 后者由命令层负责。

use rusqlite::{params, Connection};

use crate::domain::models::SessionMeta;
use crate::error::AppResult;

pub fn save(conn: &Connection, id: &str, name: &str, payload: &str) -> AppResult<SessionMeta> {
    let now = chrono::Utc::now().timestamp();

    let created_at: i64 = conn
        .query_row("SELECT created_at FROM sessions WHERE id = ?1", params![id], |r| r.get(0))
        .unwrap_or(now);

    conn.execute(
        "INSERT INTO sessions (id, name, created_at, updated_at, payload)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at, payload = excluded.payload",
        params![id, name, created_at, now, payload],
    )?;

    Ok(SessionMeta {
        id: id.to_string(),
        name: name.to_string(),
        created_at,
        updated_at: now,
        roots: Vec::new(),
        group_count: 0,
        wasted_bytes: 0,
        file_path: None,
    })
}

pub fn load(conn: &Connection, id: &str) -> AppResult<Option<String>> {
    let mut stmt = conn.prepare("SELECT payload FROM sessions WHERE id = ?1")?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

pub fn list(conn: &Connection) -> AppResult<Vec<SessionMeta>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, updated_at, payload FROM sessions ORDER BY updated_at DESC LIMIT 200",
    )?;
    let rows = stmt.query_map([], |row| {
        let payload: String = row.get(4)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            payload,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, name, created_at, updated_at, payload) = row?;
        // payload 解析失败不应让整个列表 500，降级为缺失统计。
        let (roots, group_count, wasted_bytes) =
            match serde_json::from_str::<serde_json::Value>(&payload) {
                Ok(v) => {
                    let roots = v
                        .get("roots")
                        .and_then(|r| r.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let gc = v
                        .get("groups")
                        .and_then(|g| g.as_array())
                        .map(|a| a.len() as u64)
                        .unwrap_or(0);
                    let wb = v
                        .get("stats")
                        .and_then(|s| s.get("wastedBytes"))
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0);
                    (roots, gc, wb)
                }
                Err(_) => (Vec::new(), 0, 0),
            };

        out.push(SessionMeta {
            id,
            name,
            created_at,
            updated_at,
            roots,
            group_count,
            wasted_bytes,
            file_path: None,
        });
    }
    Ok(out)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<usize> {
    Ok(conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?)
}

// ============================ 操作审计流水 ============================

pub fn record_journal(
    conn: &Connection,
    id: &str,
    mode: &str,
    dry_run: bool,
    succeeded: u64,
    failed: u64,
    freed_bytes: u64,
    payload: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO action_journal (id, created_at, mode, dry_run, succeeded, failed, freed_bytes, undone, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)",
        params![
            id,
            chrono::Utc::now().timestamp(),
            mode,
            i64::from(dry_run),
            succeeded as i64,
            failed as i64,
            freed_bytes as i64,
            payload,
        ],
    )?;
    Ok(())
}

pub fn get_journal(conn: &Connection, id: &str) -> AppResult<Option<String>> {
    let mut stmt = conn.prepare("SELECT payload FROM action_journal WHERE id = ?1")?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

pub fn mark_journal_undone(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("UPDATE action_journal SET undone = 1 WHERE id = ?1", params![id])?;
    Ok(())
}
