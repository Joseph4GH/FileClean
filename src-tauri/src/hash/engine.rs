//! 并行哈希调度。
//!
//! 只有「通过体积预筛」的文件才会进入这一层；每个阶段都优先命中 SQLite 增量缓存，
//! 未命中的文件交给 rayon 线程池并行计算，I/O 与 CPU 都能被吃满。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use rayon::prelude::*;

use crate::db::cache_repo::CachedFile;
use crate::domain::models::{FileEntry, HashAlgorithm};
use crate::error::{AppError, AppResult};
use crate::hash::algorithms;

/// 进度回调：`(已完成数量, 当前文件路径)`。
pub type TickFn<'a> = dyn Fn(u64, &str) + Send + Sync + 'a;

/// 每处理多少个文件回调一次进度，避免事件风暴。
const TICK_EVERY: u64 = 64;

#[derive(Debug, Default)]
pub struct HashBatch {
    /// 与输入等长；`None` 表示该项被取消或计算失败。
    pub values: Vec<Option<String>>,
    pub from_cache: Vec<bool>,
    pub failures: Vec<(String, String)>,
}

impl HashBatch {
    pub fn cache_hits(&self) -> u64 {
        self.from_cache.iter().filter(|v| **v).count() as u64
    }
}

/// 构建按需线程数的 rayon 线程池（`threads == 0` 表示交由 rayon 自适应）。
pub fn build_pool(threads: usize) -> AppResult<rayon::ThreadPool> {
    let mut builder = rayon::ThreadPoolBuilder::new();
    if threads > 0 {
        builder = builder.num_threads(threads);
    }
    builder
        // 单次任务粒度已经足够，栈默认值即可。
        .thread_name(|i| format!("fileclean-hash-{i}"))
        .build()
        .map_err(|e| AppError::internal(format!("哈希线程池创建失败：{e}")))
}

/// 阶段一：分块预筛指纹（64 KiB 头 + 64 KiB 尾 + 体积），命中缓存直接复用。
pub fn compute_partial(
    entries: &[FileEntry],
    cache: &HashMap<String, CachedFile>,
    cancel: &AtomicBool,
    threads: usize,
    tick: &TickFn<'_>,
) -> AppResult<HashBatch> {
    let pool = build_pool(threads)?;
    let done = AtomicU64::new(0);
    let failures: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

    let results: Vec<(Option<String>, bool)> = pool.install(|| {
        entries
            .par_iter()
            .map(|entry| {
                if cancel.load(Ordering::Relaxed) {
                    return (None, false);
                }

                if let Some(cached) = cache.get(&entry.path) {
                    if cached.matches(entry.size, entry.modified, entry.inode) {
                        if let Some(hash) = cached.partial_hash.clone() {
                            bump(&done, tick, &entry.path);
                            return (Some(hash), true);
                        }
                    }
                }

                let value = match algorithms::partial_hash(std::path::Path::new(&entry.path), entry.size)
                {
                    Ok(h) => Some(h),
                    Err(e) => {
                        failures
                            .lock()
                            .expect("failures mutex poisoned")
                            .push((entry.path.clone(), e.to_string()));
                        None
                    }
                };
                bump(&done, tick, &entry.path);
                (value, false)
            })
            .collect()
    });

    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }

    Ok(HashBatch {
        values: results.iter().map(|(v, _)| v.clone()).collect(),
        from_cache: results.iter().map(|(_, c)| *c).collect(),
        failures: failures.into_inner().unwrap_or_default(),
    })
}

/// 阶段二：全量哈希。只有「体积 + 分块指纹」都相同且组内成员数 > 1 的文件才走到这里。
pub fn compute_full(
    entries: &[FileEntry],
    cache: &HashMap<String, CachedFile>,
    algo: HashAlgorithm,
    cancel: &AtomicBool,
    threads: usize,
    tick: &TickFn<'_>,
) -> AppResult<HashBatch> {
    let pool = build_pool(threads)?;
    let done = AtomicU64::new(0);
    let failures: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    let algo_name = algo.as_str();

    let results: Vec<(Option<String>, bool)> = pool.install(|| {
        entries
            .par_iter()
            .map(|entry| {
                if cancel.load(Ordering::Relaxed) {
                    return (None, false);
                }

                if let Some(cached) = cache.get(&entry.path) {
                    if cached.matches(entry.size, entry.modified, entry.inode)
                        && cached.full_algo.as_deref() == Some(algo_name)
                    {
                        if let Some(hash) = cached.full_hash.clone() {
                            bump(&done, tick, &entry.path);
                            return (Some(hash), true);
                        }
                    }
                }

                let value =
                    match algorithms::hash_file(algo, std::path::Path::new(&entry.path), entry.size)
                    {
                        Ok(h) => Some(h),
                        Err(e) => {
                            failures
                                .lock()
                                .expect("failures mutex poisoned")
                                .push((entry.path.clone(), e.to_string()));
                            None
                        }
                    };
                bump(&done, tick, &entry.path);
                (value, false)
            })
            .collect()
    });

    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }

    Ok(HashBatch {
        values: results.iter().map(|(v, _)| v.clone()).collect(),
        from_cache: results.iter().map(|(_, c)| *c).collect(),
        failures: failures.into_inner().unwrap_or_default(),
    })
}

/// 通用并行映射：感知哈希 / 音频指纹等阶段复用同一套线程池与取消语义。
pub fn parallel_map<T, R, F>(
    items: &[T],
    threads: usize,
    cancel: &AtomicBool,
    tick: &TickFn<'_>,
    path_of: impl Fn(&T) -> String + Sync,
    f: F,
) -> AppResult<Vec<Option<R>>>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Result<R, String> + Sync,
{
    let pool = build_pool(threads)?;
    let done = AtomicU64::new(0);

    let results: Vec<Result<R, String>> = pool.install(|| {
        items
            .par_iter()
            .map(|item| {
                if cancel.load(Ordering::Relaxed) {
                    return Err("cancelled".to_string());
                }
                let out = f(item);
                let p = path_of(item);
                bump(&done, tick, &p);
                out
            })
            .collect()
    });

    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }

    Ok(results.into_iter().map(|r| r.ok()).collect())
}

fn bump(done: &AtomicU64, tick: &TickFn<'_>, path: &str) {
    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
    if n % TICK_EVERY == 0 {
        tick(n, path);
    }
}
