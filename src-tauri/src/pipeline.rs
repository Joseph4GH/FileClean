//! 扫描编排（服务层）：把遍历 → 预筛 → 精确哈希 → 相似检测 → 引用检查 → 智能标记
//! 串成一条流水线，并在每个阶段把进度推给前端。
//!
//! 这一层不依赖任何 HTTP / Tauri 类型，因此 CLI 与 GUI 可以复用同一条链路。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::actions::ActionContext;
use crate::archive;
use crate::autoselect;
use crate::config::AppConfig;
use crate::db::cache_repo::{self, CacheWrite, CachedFile};
use crate::db::Db;
use crate::dedupe::exact::{self, ExactGroup};
use crate::dedupe::perceptual::{self, ImageFingerprint};
use crate::dedupe::{audio, raw_pair};
use crate::domain::models::*;
use crate::error::{AppError, AppResult};
use crate::hash::algorithms;
use crate::hash::engine;
use crate::refs::ReferenceIndex;
use crate::scan::filter::{path_to_string, FilterEngine};
use crate::scan::walker::{self, WalkContext, WalkOutcome};

/// 进度回调。实现方负责节流（前端事件、CLI 输出）。
pub type ProgressSink<'a> = dyn Fn(ScanProgress) + Send + Sync + 'a;

pub struct ScanOutcome {
    pub result: ScanResult,
    /// 本次命中的缓存条目数（写入缓存时统计）。
    pub cache_writes: usize,
}

pub fn run_scan(
    scan_id: &str,
    options: &ScanOptions,
    mark_config: &AutoMarkConfig,
    db: &Db,
    app_config: &AppConfig,
    cancel: &AtomicBool,
    emit: &ProgressSink<'_>,
) -> AppResult<ScanOutcome> {
    options.validate()?;
    let started = Instant::now();
    let threads = if options.threads > 0 {
        options.threads
    } else {
        app_config.effective_threads()
    };

    let mut progress: ScanProgress;
    let mut stats = ScanStats::default();

    // ---------------- 阶段 1：遍历 + 过滤 ----------------
    let filter_engine = FilterEngine::new(&options.filter, options.protect_system_dirs)?;

    let mut walk_ctx = WalkContext {
        cancel,
        max_files: app_config.max_scan_files,
        on_progress: Box::new(|seen, path| {
            let mut p = ScanProgress::new(scan_id, ScanPhase::Walking);
            p.files_seen = seen;
            p.current_path = path.to_string();
            p.message = format!("已发现 {seen} 个文件");
            emit(p);
        }),
    };

    let WalkOutcome {
        entries,
        stats: walk_stats,
        skipped_examples,
    } = walker::walk_roots(&options.roots, &filter_engine, &mut walk_ctx)?;

    stats.files_seen = walk_stats.files_seen;
    stats.files_scanned = walk_stats.files_accepted;
    stats.files_skipped = walk_stats.files_rejected;
    stats.bytes_scanned = walk_stats.bytes_accepted;
    stats.protected_dirs_skipped = walk_stats.protected_dirs_skipped;
    stats.inaccessible_paths = walk_stats.inaccessible_paths;

    progress = ScanProgress::new(scan_id, ScanPhase::Filtering);
    progress.files_seen = stats.files_seen;
    progress.files_total = stats.files_scanned;
    progress.percent = 0.06;
    progress.message = format!(
        "通过过滤的文件 {} 个（跳过 {} 个，受保护目录 {} 个）",
        stats.files_scanned, stats.files_skipped, stats.protected_dirs_skipped
    );
    emit(progress);

    if entries.is_empty() {
        return Ok(ScanOutcome {
            result: finish(scan_id, options, Vec::new(), stats, started, 0),
            cache_writes: 0,
        });
    }

    // ---------------- 增量缓存加载 ----------------
    let cache: HashMap<String, CachedFile> = if options.use_cache {
        db.read(|conn| cache_repo::load_for_roots(conn, &options.roots, app_config.cache_ttl_secs()))?
    } else {
        HashMap::new()
    };

    // ---------------- 阶段 2：体积预筛 ----------------
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        by_size.entry(entry.size).or_default().push(i);
    }

    // RAW/JPEG 配对必须先算，因为它们要从后续所有候选中剔除。
    let pairs = if options.raw_jpeg_pairing {
        raw_pair::find_pairs(&entries)
    } else {
        Vec::new()
    };
    let paired: std::collections::HashSet<usize> =
        raw_pair::paired_indices(&pairs).into_iter().collect();

    let size_candidates: Vec<usize> = by_size
        .values()
        .filter(|idxs| {
            idxs.iter().filter(|i| !paired.contains(i)).count() >= options.min_group_size
        })
        .flat_map(|idxs| idxs.iter().copied())
        .filter(|i| !paired.contains(i))
        .collect();

    // ---------------- 阶段 3：分块指纹预筛 ----------------
    let partial_entries: Vec<FileEntry> =
        size_candidates.iter().map(|&i| entries[i].clone()).collect();

    let partial_batch = engine::compute_partial(
        &partial_entries,
        &cache,
        cancel,
        threads,
        &|done, path| {
            let mut p = ScanProgress::new(scan_id, ScanPhase::PartialHash);
            p.files_seen = done;
            p.files_total = partial_entries.len() as u64;
            p.current_path = path.to_string();
            p.percent = 0.06 + 0.24 * ratio(done, partial_entries.len() as u64);
            p.message = format!("分块指纹预筛 {done}/{}", partial_entries.len());
            emit(p);
        },
    )?;

    stats.partial_hashed = partial_batch.values.iter().filter(|v| v.is_some()).count() as u64;
    stats.cache_hits += partial_batch.cache_hits();
    stats.cache_misses += partial_batch.values.len() as u64 - partial_batch.cache_hits();

    for (path, err) in &partial_batch.failures {
        tracing::debug!(path = %crate::logging::redact_path(path), error = %err, "分块指纹计算失败");
    }

    // 用 (size, partial) 重新分组，只有真正有嫌疑的才进入全量哈希。
    let mut candidate_buckets: HashMap<(u64, String), Vec<usize>> = HashMap::new();
    for (pos, &entry_idx) in size_candidates.iter().enumerate() {
        if let Some(Some(hash)) = partial_batch.values.get(pos) {
            candidate_buckets
                .entry((entries[entry_idx].size, hash.clone()))
                .or_default()
                .push(entry_idx);
        }
    }

    let full_indices: Vec<usize> = candidate_buckets
        .values()
        .filter(|v| v.len() >= options.min_group_size)
        .flat_map(|v| v.iter().copied())
        .collect();
    let full_entries: Vec<FileEntry> = full_indices.iter().map(|&i| entries[i].clone()).collect();

    // ---------------- 阶段 4：全量哈希 ----------------
    let full_batch = engine::compute_full(
        &full_entries,
        &cache,
        options.algorithm,
        cancel,
        threads,
        &|done, path| {
            let mut p = ScanProgress::new(scan_id, ScanPhase::FullHash);
            p.files_seen = done;
            p.files_total = full_entries.len() as u64;
            p.current_path = path.to_string();
            p.percent = 0.30 + 0.30 * ratio(done, full_entries.len() as u64);
            p.message = format!("完整哈希 {done}/{}", full_entries.len());
            emit(p);
        },
    )?;

    stats.full_hashed = full_batch.values.iter().filter(|v| v.is_some()).count() as u64;
    stats.cache_hits += full_batch.cache_hits();
    stats.cache_misses += full_batch.values.len() as u64 - full_batch.cache_hits();

    // ---------------- 阶段 5：精确分组 ----------------
    let exact_groups = exact::build_groups(&full_entries, &full_batch.values, options.min_group_size);
    let mut groups: Vec<DuplicateGroup> =
        build_exact_groups(&exact_groups, &full_entries, &full_batch.values, options.algorithm);

    // 已进入精确重复组的文件不再参与相似检测，避免同一堆文件被报两次。
    let mut consumed: std::collections::HashSet<String> = groups
        .iter()
        .flat_map(|g| g.files.iter().map(|f| f.entry.path.clone()))
        .collect();

    // ---------------- 阶段 6：图片感知哈希 ----------------
    if options.detect_perceptual {
        let image_entries: Vec<FileEntry> = entries
            .iter()
            .filter(|e| {
                perceptual::IMAGE_EXTENSIONS.contains(&e.extension.as_str())
                    && !consumed.contains(&e.path)
            })
            .cloned()
            .collect();

        let fingerprints = engine::parallel_map(
            &image_entries,
            threads,
            cancel,
            &|done, path| {
                let mut p = ScanProgress::new(scan_id, ScanPhase::Perceptual);
                p.files_seen = done;
                p.files_total = image_entries.len() as u64;
                p.current_path = path.to_string();
                p.percent = 0.60 + 0.15 * ratio(done, image_entries.len() as u64);
                p.message = format!("感知哈希 {done}/{}", image_entries.len());
                emit(p);
            },
            |e: &FileEntry| e.path.clone(),
            |e: &FileEntry| perceptual::compute(std::path::Path::new(&e.path)),
        )?;

        let mut indexed: Vec<(usize, ImageFingerprint)> = Vec::new();
        for (i, fp) in fingerprints.into_iter().enumerate() {
            if let Some(fp) = fp {
                indexed.push((i, fp));
            }
        }

        let flat_fingerprints: Vec<ImageFingerprint> = indexed.iter().map(|(_, fp)| *fp).collect();
        let similar = perceptual::group_similar(
            &flat_fingerprints,
            options.perceptual_threshold,
            options.min_group_size,
        );

        for group in similar {
            let files: Vec<DuplicateFile> = group
                .indices
                .iter()
                .map(|&k| {
                    let entry = &image_entries[indexed[k].0];
                    let fp = indexed[k].1;
                    let mut f = DuplicateFile::from_entry(entry.clone());
                    f.fingerprint = Some(format!("{:016x}/{:016x}", fp.phash, fp.dhash));
                    f
                })
                .collect();

            consumed.extend(files.iter().map(|f| f.entry.path.clone()));
            groups.push(new_group(
                MatchKind::Perceptual,
                format!("phash-{}", group.indices.len()),
                files,
                Some(group.similarity),
            ));
        }
    }

    // ---------------- 阶段 7：音频指纹 ----------------
    if options.audio_fingerprint {
        let audio_entries: Vec<FileEntry> = entries
            .iter()
            .filter(|e| {
                audio::AUDIO_EXTENSIONS.contains(&e.extension.as_str())
                    && !consumed.contains(&e.path)
            })
            .cloned()
            .collect();

        let prints = engine::parallel_map(
            &audio_entries,
            threads,
            cancel,
            &|done, path| {
                let mut p = ScanProgress::new(scan_id, ScanPhase::Audio);
                p.files_seen = done;
                p.files_total = audio_entries.len() as u64;
                p.current_path = path.to_string();
                p.percent = 0.75 + 0.10 * ratio(done, audio_entries.len() as u64);
                p.message = format!("音频指纹 {done}/{}", audio_entries.len());
                emit(p);
            },
            |e: &FileEntry| e.path.clone(),
            |e: &FileEntry| audio::compute(std::path::Path::new(&e.path)),
        )?;

        let indexed: Vec<(usize, audio::AudioFingerprint)> = prints
            .into_iter()
            .enumerate()
            .filter_map(|(i, p)| p.map(|p| (i, p)))
            .collect();

        let audio_groups = audio::group_similar(&indexed, options.audio_threshold, options.min_group_size);

        for group in audio_groups {
            let files: Vec<DuplicateFile> = group
                .indices
                .iter()
                .map(|&k| {
                    let entry = &audio_entries[indexed[k].0];
                    let mut f = DuplicateFile::from_entry(entry.clone());
                    f.fingerprint = Some(audio::encode_profile(&indexed[k].1.profile));
                    f
                })
                .collect();
            consumed.extend(files.iter().map(|f| f.entry.path.clone()));
            groups.push(new_group(
                MatchKind::Audio,
                format!("audio-{}", group.indices.len()),
                files,
                Some(group.similarity),
            ));
        }
    }

    // ---------------- 阶段 8：压缩包内部条目 ----------------
    if options.scan_archives {
        let archives: Vec<std::path::PathBuf> = entries
            .iter()
            .filter(|e| archive::ARCHIVE_EXTENSIONS.contains(&e.extension.as_str()))
            .map(|e| std::path::PathBuf::from(&e.path))
            .collect();

        progress = ScanProgress::new(scan_id, ScanPhase::Archives);
        progress.percent = 0.86;
        progress.message = format!("扫描 {} 个压缩包", archives.len());
        emit(progress);

        let outcome = archive::scan_archives(&archives, options.algorithm, options.archive_max_entry_bytes);
        stats.archive_entries = outcome.entries.len() as u64;

        for group in archive::group_internal(&outcome.entries, options.min_group_size) {
            let (fingerprint, indices) = group;
            let files: Vec<DuplicateFile> = indices
                .iter()
                .map(|&i| {
                    let e = &outcome.entries[i];
                    let mut f = DuplicateFile::from_entry(FileEntry {
                        path: e.archive_path.clone(),
                        root: String::new(),
                        size: e.size,
                        modified: 0,
                        created: 0,
                        extension: String::new(),
                        inode: 0,
                        device: 0,
                        hard_links: 1,
                        is_symlink: false,
                        depth: 0,
                        relative_path: e.inner_path.clone(),
                    });
                    f.hash = Some(e.fingerprint.clone());
                    f.archive_inner_path = Some(e.inner_path.clone());
                    f.reasons
                        .push("位于压缩包内部，需要重写压缩包才能移除".into());
                    f
                })
                .collect();
            groups.push(new_group(
                MatchKind::ArchiveInternal,
                fingerprint,
                files,
                None,
            ));
        }

        for warning in outcome.warnings {
            tracing::warn!(warning = %warning, "压缩包扫描告警");
        }
    }

    // ---------------- 阶段 9：引用完整性 ----------------
    let mut reference_report = ReferenceReport::default();
    if options.check_references {
        progress = ScanProgress::new(scan_id, ScanPhase::References);
        progress.percent = 0.90;
        progress.message = "检查素材库引用与链接".into();
        emit(progress);

        let index = ReferenceIndex::build(&options.roots, true);
        reference_report = index.report().clone();

        for group in &mut groups {
            for file in &mut group.files {
                if let Some(hit) = index.check(&file.entry) {
                    file.protected = Some(hit.detail.clone());
                    stats.referenced_files += 1;
                }
            }
        }
    }

    // ---------------- 阶段 10：RAW/JPEG 配对作为信息性分组 ----------------
    if !pairs.is_empty() {
        let files: Vec<DuplicateFile> = pairs
            .iter()
            .flat_map(|p| p.all_indices())
            .filter_map(|i| entries.get(i))
            .map(|e| {
                let mut f = DuplicateFile::from_entry(e.clone());
                f.decision = MarkDecision::Keep;
                f.protected = Some("属于 RAW/JPEG 配对，不参与重复清理".into());
                f.reasons
                    .push("与同名的另一种格式属于同一张照片，已整体保留".into());
                f
            })
            .collect();
        if files.len() >= 2 {
            groups.push(new_group(
                MatchKind::RawJpegPair,
                format!("pair-{}", pairs.len()),
                files,
                None,
            ));
        }
    }

    // ---------------- 阶段 11：智能标记 ----------------
    progress = ScanProgress::new(scan_id, ScanPhase::Marking);
    progress.percent = 0.94;
    progress.message = "应用保留策略".into();
    emit(progress);

    for group in &mut groups {
        let mark = autoselect::mark_group(&group.files, group.kind, mark_config);
        for m in &mark.marks {
            if let Some(file) = group.files.get_mut(m.index) {
                file.decision = m.decision;
                if !m.reasons.is_empty() {
                    file.reasons = m.reasons.clone();
                }
                file.is_preferred = mark.preferred == Some(m.index);
            }
        }
        group.recompute();
    }

    groups.sort_by(|a, b| {
        b.wasted_bytes
            .cmp(&a.wasted_bytes)
            .then_with(|| b.total_bytes.cmp(&a.total_bytes))
    });

    stats.groups = groups.len() as u64;
    stats.duplicate_files = groups
        .iter()
        .flat_map(|g| g.files.iter())
        .filter(|f| f.decision == MarkDecision::Remove)
        .count() as u64;
    stats.wasted_bytes = groups.iter().map(|g| g.wasted_bytes).sum();
    stats.protected_files = groups
        .iter()
        .flat_map(|g| g.files.iter())
        .filter(|f| f.protected.is_some())
        .count() as u64;

    // 受保护目录 / 不可访问路径样例挂进日志，便于排障。
    for (path, reason) in skipped_examples.iter().take(10) {
        tracing::debug!(path = %crate::logging::redact_path(path), reason = %reason, "目录已跳过");
    }

    // ---------------- 缓存回写 ----------------
    let cache_writes = if options.use_cache {
        write_cache(db, &entries, &size_candidates, &partial_batch.values, &full_indices, &full_batch.values, options)?
    } else {
        0
    };

    let mut result = finish(scan_id, options, groups, stats, started, cache_writes);
    // 引用报告作为统计附件返回（放 stats 之外的独立字段会更重，这里合并进结果）。
    if !reference_report.catalogs_scanned.is_empty() || !reference_report.warnings.is_empty() {
        tracing::info!(
            catalogs = reference_report.catalogs_scanned.len(),
            warnings = reference_report.warnings.len(),
            "引用检查完成"
        );
    }
    result.stats.duration_ms = started.elapsed().as_millis() as u64;

    // `stats` 已被 finish 消费，这里从 result 里取，避免出现「移动后再用」。
    progress = ScanProgress::new(scan_id, ScanPhase::Done);
    progress.percent = 1.0;
    progress.files_seen = result.stats.files_seen;
    progress.files_total = result.stats.files_scanned;
    progress.groups_found = result.stats.groups;
    progress.cache_hits = result.stats.cache_hits;
    progress.elapsed_ms = result.stats.duration_ms;
    progress.message = format!(
        "完成：{} 个分组，可回收 {}",
        result.stats.groups,
        human_bytes(result.stats.wasted_bytes)
    );
    emit(progress);

    Ok(ScanOutcome {
        result,
        cache_writes,
    })
}

fn ratio(done: u64, total: u64) -> f32 {
    if total == 0 {
        1.0
    } else {
        (done as f32 / total as f32).clamp(0.0, 1.0)
    }
}

fn new_group(
    kind: MatchKind,
    signature: String,
    files: Vec<DuplicateFile>,
    similarity: Option<f32>,
) -> DuplicateGroup {
    let mut group = DuplicateGroup {
        id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
        kind,
        kind_label: kind.label().to_string(),
        signature,
        title: files
            .first()
            .map(|f| f.entry.file_name().to_string())
            .unwrap_or_default(),
        files,
        wasted_bytes: 0,
        total_bytes: 0,
        similarity,
        informational: kind.is_informational(),
    };
    group.recompute();
    group
}

fn build_exact_groups(
    exact_groups: &[ExactGroup],
    entries: &[FileEntry],
    hashes: &[Option<String>],
    _algo: HashAlgorithm,
) -> Vec<DuplicateGroup> {
    exact_groups
        .iter()
        .map(|eg| {
            let files: Vec<DuplicateFile> = eg
                .indices
                .iter()
                .map(|&i| {
                    let mut f = DuplicateFile::from_entry(entries[i].clone());
                    f.hash = hashes.get(i).cloned().flatten();
                    f
                })
                .collect();
            let mut group = new_group(MatchKind::Exact, eg.signature.clone(), files, None);
            // 硬链接同源：删除不会释放空间，直接标保护。
            if eg.distinct_count < eg.member_count {
                protect_hardlinks(&mut group);
            }
            group
        })
        .collect()
}

/// 把「与其它成员指向同一 inode」的条目标记为受保护。
fn protect_hardlinks(group: &mut DuplicateGroup) {
    let entries: Vec<FileEntry> = group.files.iter().map(|f| f.entry.clone()).collect();
    let mut protected: Vec<usize> = Vec::new();

    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            if exact::same_physical_file(&entries[i], &entries[j]) {
                if !protected.contains(&i) {
                    protected.push(i);
                }
                if !protected.contains(&j) {
                    protected.push(j);
                }
            }
        }
    }

    // 若全部成员互为硬链接，说明这就是同一个文件，保留一个即可；
    // 否则只保护重复出现的那部分，正常副本仍可清理。
    if protected.len() == entries.len() {
        protected.clear();
        protected.push(0);
    }

    for idx in protected {
        if let Some(file) = group.files.get_mut(idx) {
            file.protected = Some("与组内其它条目指向同一物理文件（硬链接）".into());
        }
    }
}

fn write_cache(
    db: &Db,
    entries: &[FileEntry],
    partial_indices: &[usize],
    partial_values: &[Option<String>],
    full_indices: &[usize],
    full_values: &[Option<String>],
    options: &ScanOptions,
) -> AppResult<usize> {
    // 先按索引汇总每个文件拿到了哪些指纹，再一次性落库，避免同一行写两次。
    let mut records: HashMap<usize, CacheWrite> = HashMap::new();

    let slot = |idx: usize| -> CacheWrite {
        let entry = &entries[idx];
        CacheWrite {
            path: entry.path.clone(),
            root: entry.root.clone(),
            size: entry.size,
            mtime: entry.modified,
            inode: entry.inode,
            partial_hash: None,
            full_hash: None,
            full_algo: None,
            phash: None,
            audio_sig: None,
        }
    };

    for (pos, &idx) in partial_indices.iter().enumerate() {
        if let Some(Some(hash)) = partial_values.get(pos) {
            records.entry(idx).or_insert_with(|| slot(idx)).partial_hash = Some(hash.clone());
        }
    }

    let algo_name = options.algorithm.as_str().to_string();
    for (pos, &idx) in full_indices.iter().enumerate() {
        if let Some(Some(hash)) = full_values.get(pos) {
            let record = records.entry(idx).or_insert_with(|| slot(idx));
            record.full_hash = Some(hash.clone());
            record.full_algo = Some(algo_name.clone());
        }
    }

    if records.is_empty() {
        return Ok(0);
    }

    let items: Vec<CacheWrite> = records.into_values().collect();
    let total = items.len();

    // 分批提交：单事务过大时 SQLite 的 WAL 会膨胀，分批兼顾吞吐与稳定性。
    db.write_batch(|conn| {
        for chunk in items.chunks(4096) {
            let tx = conn.transaction()?;
            cache_repo::upsert_many(&tx, chunk)?;
            tx.commit()?;
        }
        Ok(())
    })?;

    Ok(total)
}

fn finish(
    scan_id: &str,
    options: &ScanOptions,
    groups: Vec<DuplicateGroup>,
    stats: ScanStats,
    started: Instant,
    _cache_writes: usize,
) -> ScanResult {
    let mut stats = stats;
    stats.duration_ms = started.elapsed().as_millis() as u64;
    ScanResult {
        scan_id: scan_id.to_string(),
        roots: options.roots.clone(),
        algorithm: options.algorithm,
        options: options.clone(),
        groups,
        stats,
        created_at: chrono::Utc::now().timestamp(),
    }
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// 供命令层构造动作上下文。
pub fn action_context<'a>(
    config: &'a AppConfig,
    options: &'a ScanOptions,
    protected: &'a crate::scan::filter::ProtectedPaths,
) -> ActionContext<'a> {
    ActionContext {
        config,
        allowed_roots: &options.roots,
        protected,
    }
}

/// 便于外部（CLI）在没有 Tauri 事件系统时静默消费进度。
pub fn silent_sink() -> impl Fn(ScanProgress) + Send + Sync {
    |_| {}
}

/// 供 UI 计算「相对扫描根」展示用的辅助函数。
pub fn relative_of(entry: &FileEntry) -> String {
    if entry.relative_path.is_empty() {
        path_to_string(std::path::Path::new(&entry.path))
    } else {
        entry.relative_path.clone()
    }
}

/// 取消检查的便捷封装。
pub fn check_cancel(cancel: &AtomicBool) -> AppResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(AppError::Cancelled)
    } else {
        Ok(())
    }
}

/// 计算单文件的哈希（预览 / 校验场景）。
pub fn hash_single(path: &str, algo: HashAlgorithm) -> AppResult<String> {
    let p = std::path::Path::new(path);
    let size = std::fs::metadata(p).map_err(|_| AppError::NotFound(path.to_string()))?.len();
    algorithms::hash_file(algo, p, size).map_err(AppError::Io)
}
