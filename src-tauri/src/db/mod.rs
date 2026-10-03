//! 仓储层：SQLite 连接管理与迁移。
//!
//! 桌面场景下写入是批量、短时的，因此采用「单连接 + WAL + 互斥锁」，
//! 既避免连接池的额外开销，又保证多线程扫描时的写入串行安全。

pub mod cache_repo;
pub mod migrations;
pub mod session_repo;

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::{Connection, Transaction};

use crate::error::{AppError, AppResult};

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(|e| {
            AppError::Db(e)
        })?;

        // WAL 让读不阻塞写；NORMAL 同步级别在桌面断电风险下足够且快得多。
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -32000;",
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;

        migrations::migrate(&conn)?;

        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    /// 只读访问连接（绝大多数查询走这里）。
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let guard = self.conn.lock();
        f(&guard)
    }

    /// 事务写入：闭包返回 Err 时自动回滚，保证多步写入的原子性。
    pub fn write<T>(&self, f: impl FnOnce(&Transaction<'_>) -> AppResult<T>) -> AppResult<T> {
        let mut guard = self.conn.lock();
        let tx = guard.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// 批量写入的优化入口：调用方自行控制事务边界。
    pub fn write_batch<T>(&self, f: impl FnOnce(&mut Connection) -> AppResult<T>) -> AppResult<T> {
        let mut guard = self.conn.lock();
        f(&mut guard)
    }

    pub fn file_size_bytes(&self) -> u64 {
        // 这里无法拿到物理文件大小（WAL 下会更复杂），返回逻辑大小即可。
        self.read(|conn| {
            let page_count: i64 =
                conn.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap_or(0);
            let page_size: i64 =
                conn.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap_or(0);
            Ok((page_count.max(0) as u64) * (page_size.max(0) as u64))
        })
        .unwrap_or(0)
    }
}
