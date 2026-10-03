//! 增量扫描缓存仓储。
//!
//! 判定“文件未变更”的依据是 **路径 + 大小 + 修改时间 + inode** 四元组。
//! 只有四者完全一致才复用旧的指纹结果，避免出现「改了内容但 mtime 被伪造」的漏检。

use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::error::AppResult;

#[derive(Debug, Clone)]
pub struct CachedFile {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub inode: u64,
    pub partial_hash: Option<String>,
    pub full_hash: Option<String>,
    pub full_algo: Option<String>,
    pub phash: Option<u64>,
    pub audio_sig: Option<String>,
    pub scanned_at: i64,
}

impl CachedFile {
    /// 判断缓存条目能否直接复用。
    pub fn matches(&self, size: u64, mtime: i64, inode: u64) -> bool {
        self.size == size && self.mtime == mtime && (inode == 0 || self.inode == 0 || self.inode == inode)
    }
}

#[derive(Debug, Clone)]
pub struct CacheWrite {
    pub path: String,
    pub root: String,
    pub size: u64,
    pub mtime: i64,
    pub inode: u64,
    pub partial_hash: Option<String>,
    pub full_hash: Option<String>,
    pub full_algo: Option<String>,
    pub phash: Option<u64>,
    pub audio_sig: Option<String>,
}

fn u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// 一次性把指定扫描根下的有效缓存载入内存。
///
/// 返回 `HashMap<path, CachedFile>` —— 扫描阶段全程只读内存，避免逐文件查库。
pub fn load_for_roots(
    conn: &Connection,
    roots: &[String],
    ttl_secs: i64,
) -> AppResult<HashMap<String, CachedFile>> {
    let mut map = HashMap::new();
    if roots.is_empty() {
        return Ok(map);
    }

    let cutoff = chrono::Utc::now().timestamp() - ttl_secs;

    // 这里必须用**具名参数**。位置参数 `?N` 交给循环手算下标极易错位：早先写成
    // `?{i+2} / ?{i+3}`，第二个根就复用了 `?3`，SQL 只声明 4 个占位符却绑定了 5 个
    // 值（`Got 5, needed 4`）；而且 `?3` 被重复使用还会让第二个根的长度错绑到
    // 第一个根上。具名参数让「名字」与「值」一一对应，彻底免掉下标算术。
    let mut sql = String::from(
        "SELECT path, size, mtime, inode, partial_hash, full_hash, full_algo, phash, audio_sig, scanned_at
         FROM files WHERE scanned_at >= :cutoff AND (",
    );
    for i in 0..roots.len() {
        if i > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str(&format!("substr(path, 1, :len{i}) = :root{i}"));
    }
    sql.push(')');

    // 参数名需要比 bind 活得更久，先各自落到 Vec 里持有所有权。
    let names: Vec<(String, String)> = (0..roots.len())
        .map(|i| (format!(":len{i}"), format!(":root{i}")))
        .collect();
    let lens: Vec<i64> = roots
        .iter()
        .map(|root| i64::try_from(root.chars().count()).unwrap_or(i64::MAX))
        .collect();

    let mut bind: Vec<(&str, &dyn rusqlite::ToSql)> = Vec::with_capacity(roots.len() * 2 + 1);
    bind.push((":cutoff", &cutoff));
    for i in 0..roots.len() {
        bind.push((names[i].0.as_str(), &lens[i]));
        bind.push((names[i].1.as_str(), &roots[i]));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(bind.as_slice(), |row| {
        let size: i64 = row.get(1)?;
        let inode: i64 = row.get(3)?;
        let phash: Option<i64> = row.get(7)?;
        Ok(CachedFile {
            path: row.get(0)?,
            size: size.max(0) as u64,
            mtime: row.get(2)?,
            inode: inode.max(0) as u64,
            partial_hash: row.get(4)?,
            full_hash: row.get(5)?,
            full_algo: row.get(6)?,
            phash: phash.map(|v| v as u64),
            audio_sig: row.get(8)?,
            scanned_at: row.get(9)?,
        })
    })?;

    for row in rows {
        let item = row?;
        map.insert(item.path.clone(), item);
    }
    Ok(map)
}

/// 批量 upsert，调用方负责开启事务包裹。
pub fn upsert_many(conn: &Connection, items: &[CacheWrite]) -> AppResult<()> {
    if items.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().timestamp();
    let mut stmt = conn.prepare(
        "INSERT INTO files (path, root, size, mtime, inode, partial_hash, full_hash, full_algo, phash, audio_sig, scanned_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(path) DO UPDATE SET
            root = excluded.root,
            size = excluded.size,
            mtime = excluded.mtime,
            inode = excluded.inode,
            partial_hash = COALESCE(excluded.partial_hash, files.partial_hash),
            full_hash   = COALESCE(excluded.full_hash,   files.full_hash),
            full_algo   = COALESCE(excluded.full_algo,   files.full_algo),
            phash       = COALESCE(excluded.phash,       files.phash),
            audio_sig   = COALESCE(excluded.audio_sig,   files.audio_sig),
            scanned_at  = excluded.scanned_at",
    )?;

    for item in items {
        stmt.execute(params![
            item.path,
            item.root,
            u64_to_i64(item.size),
            item.mtime,
            u64_to_i64(item.inode),
            item.partial_hash,
            item.full_hash,
            item.full_algo,
            item.phash.map(|v| v as i64),
            item.audio_sig,
            now,
        ])?;
    }
    Ok(())
}

/// 清理过期缓存，避免库无限增长。
pub fn purge_expired(conn: &Connection, ttl_secs: i64) -> AppResult<usize> {
    let cutoff = chrono::Utc::now().timestamp() - ttl_secs;
    let n = conn.execute("DELETE FROM files WHERE scanned_at < ?1", params![cutoff])?;
    Ok(n)
}

/// 移除指定扫描根下的全部缓存（用户点「清空缓存重扫」时使用）。
pub fn purge_root(conn: &Connection, root: &str) -> AppResult<usize> {
    let len = i64::try_from(root.chars().count()).unwrap_or(i64::MAX);
    let n = conn.execute(
        "DELETE FROM files WHERE substr(path, 1, ?1) = ?2",
        params![len, root],
    )?;
    Ok(n)
}

pub fn purge_all(conn: &Connection) -> AppResult<usize> {
    let n = conn.execute("DELETE FROM files", [])?;
    Ok(n)
}

pub fn count(conn: &Connection) -> AppResult<u64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    Ok(n.max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::migrate(&conn).unwrap();
        conn
    }

    fn cached(path: &str, root: &str) -> CacheWrite {
        CacheWrite {
            path: path.to_string(),
            root: root.to_string(),
            size: 16,
            mtime: 1_700_000_000,
            inode: 42,
            partial_hash: Some("aa".into()),
            full_hash: Some("bb".into()),
            full_algo: Some("blake3".into()),
            phash: None,
            audio_sig: None,
        }
    }

    /// 回归测试：多根加载曾经抛 `Got 5, needed 4`。
    ///
    /// 原实现用 `?{i+2} / ?{i+3}` 拼位置参数，第二个根复用 `?3`，SQL 只声明 4 个
    /// 占位符却绑定了 5 个值。**只有 1 个根时不会暴露**，所以之前单根冒烟测试全绿。
    #[test]
    fn load_for_roots_supports_multiple_roots() {
        let conn = mem();
        upsert_many(
            &conn,
            &[
                cached("/tmp/a/one.txt", "/tmp/a"),
                cached("/tmp/long-root-name/two.txt", "/tmp/long-root-name"),
                cached("/tmp/c/three.txt", "/tmp/c"),
            ],
        )
        .unwrap();

        // 单根（旧实现唯一能跑通的情形）
        let one = load_for_roots(&conn, &["/tmp/a".to_string()], 86_400).unwrap();
        assert_eq!(one.len(), 1, "单根应命中 1 条");

        // 双根 —— 旧实现就是在这里 panic 的
        let roots2 = vec!["/tmp/a".to_string(), "/tmp/long-root-name".to_string()];
        let two = load_for_roots(&conn, &roots2, 86_400).unwrap();
        assert_eq!(two.len(), 2, "两个根都应命中（长度不同，可验证长度参数各自绑定）");
        assert!(two.contains_key("/tmp/long-root-name/two.txt"));

        // 三根
        let roots3 = vec![
            "/tmp/a".to_string(),
            "/tmp/long-root-name".to_string(),
            "/tmp/c".to_string(),
        ];
        let three = load_for_roots(&conn, &roots3, 86_400).unwrap();
        assert_eq!(three.len(), 3);
    }

    /// 无匹配根时必须返回空，而不是因为拼接错误把整表捞出来。
    #[test]
    fn load_for_roots_returns_empty_when_nothing_matches() {
        let conn = mem();
        upsert_many(&conn, &[cached("/tmp/a/one.txt", "/tmp/a")]).unwrap();

        let roots = vec!["/nowhere".to_string(), "/tmp/zzz".to_string()];
        let hit = load_for_roots(&conn, &roots, 86_400).unwrap();
        assert!(hit.is_empty(), "不应命中任何缓存，实际 {} 条", hit.len());
    }

    #[test]
    fn expired_entries_are_not_reused() {
        let conn = mem();
        upsert_many(&conn, &[cached("/tmp/a/one.txt", "/tmp/a")]).unwrap();

        // ttl 为负 → cutoff 在未来 → 任何条目都已过期
        let hit = load_for_roots(&conn, &["/tmp/a".to_string()], -60).unwrap();
        assert!(hit.is_empty(), "过期缓存不得复用");
    }

    #[test]
    fn matches_requires_size_mtime_and_inode() {
        let entry = CachedFile {
            path: "/tmp/a/one.txt".into(),
            size: 100,
            mtime: 1_700_000_000,
            inode: 7,
            partial_hash: None,
            full_hash: None,
            full_algo: None,
            phash: None,
            audio_sig: None,
            scanned_at: 0,
        };

        assert!(entry.matches(100, 1_700_000_000, 7));
        assert!(!entry.matches(101, 1_700_000_000, 7), "体积变了不能复用");
        assert!(!entry.matches(100, 1_700_000_001, 7), "mtime 被伪造不能复用");
        assert!(!entry.matches(100, 1_700_000_000, 8), "inode 变了说明文件被替换");
        // 平台拿不到 inode 时（值为 0）退化为三元组判定。
        assert!(entry.matches(100, 1_700_000_000, 0));
    }
}
