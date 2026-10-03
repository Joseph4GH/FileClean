//! 结果操作命令：重新自动标记、手动改判、预览、导出。

use std::io::Read;
use std::path::{Path, PathBuf};

use tauri::State;

use crate::autoselect;
use crate::commands::{finished_result, session_mut};
use crate::dedupe::{audio, perceptual};
use crate::domain::models::{
    AutoMarkConfig, DuplicateGroup, ExportFormat, MarkDecision, ScanResult, TextDiffPreview,
    ThumbnailPreview,
};
use crate::error::{AppError, AppResult};
use crate::export;
use crate::state::AppState;

/// 文本预览读取的上限，防止用户对一个 20 GB 的 ISO 点「对比」。
const PREVIEW_LIMIT: u64 = 512 * 1024;
const SNIPPET_WINDOW: usize = 400;

#[tauri::command]
pub fn apply_auto_mark(
    state: State<'_, AppState>,
    scan_id: String,
    mark_config: AutoMarkConfig,
) -> AppResult<ScanResult> {
    session_mut(&state, &scan_id, |session| {
        let result = session
            .result
            .as_mut()
            .ok_or_else(|| AppError::config("扫描尚未完成，无法应用保留策略"))?;

        session.mark_config = mark_config.clone();

        for group in &mut result.groups {
            // 手动改判过的文件不参与自动覆盖：用户意图优先。
            let manual: Vec<usize> = group
                .files
                .iter()
                .enumerate()
                .filter(|(_, f)| f.reasons.iter().any(|r| r.starts_with("手动")))
                .map(|(i, _)| i)
                .collect();

            let mark = autoselect::mark_group(&group.files, group.kind, &mark_config);
            for m in &mark.marks {
                if manual.contains(&m.index) {
                    continue;
                }
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

        result.groups.sort_by(|a, b| {
            b.wasted_bytes
                .cmp(&a.wasted_bytes)
                .then_with(|| b.total_bytes.cmp(&a.total_bytes))
        });
        recompute_stats(result);
        Ok(result.clone())
    })
}

#[tauri::command]
pub fn set_file_decision(
    state: State<'_, AppState>,
    scan_id: String,
    group_id: String,
    path: String,
    decision: MarkDecision,
) -> AppResult<ScanResult> {
    session_mut(&state, &scan_id, |session| {
        let result = session
            .result
            .as_mut()
            .ok_or_else(|| AppError::config("扫描尚未完成"))?;

        let group = find_group(result, &group_id)?;

        let file = group
            .files
            .iter_mut()
            .find(|f| f.entry.path == path)
            .ok_or_else(|| AppError::NotFound(path.clone()))?;

        if decision == MarkDecision::Remove && file.protected.is_some() {
            return Err(AppError::forbidden(format!(
                "该文件受保护，不能标记为待清理：{}",
                file.protected.clone().unwrap_or_default()
            )));
        }

        file.decision = decision;
        file.reasons = match decision {
            MarkDecision::Keep => vec!["手动标记：保留".into()],
            MarkDecision::Remove => vec!["手动标记：待清理".into()],
            MarkDecision::Undecided => vec!["手动标记：未决策".into()],
        };
        file.is_preferred = decision == MarkDecision::Keep;

        group.recompute();
        recompute_stats(result);
        Ok(result.clone())
    })
}

#[tauri::command]
pub fn set_group_decision(
    state: State<'_, AppState>,
    scan_id: String,
    group_id: String,
    decision: MarkDecision,
) -> AppResult<ScanResult> {
    session_mut(&state, &scan_id, |session| {
        let result = session.result.as_mut().ok_or_else(|| AppError::config("扫描尚未完成"))?;
        let group = find_group(result, &group_id)?;

        // 「整组标记为清理」会删空分组，必须至少保留一份。
        let protected_count = group.files.iter().filter(|f| !f.is_deletable()).count();
        if decision == MarkDecision::Remove && protected_count == group.files.len() {
            return Err(AppError::forbidden("该分组内全部文件都受保护，无法标记清理"));
        }

        for file in &mut group.files {
            if decision == MarkDecision::Remove && !file.is_deletable() {
                file.decision = MarkDecision::Keep;
                continue;
            }
            file.decision = decision;
        }

        if decision == MarkDecision::Remove {
            // 保证至少留一份：优先留受保护/首选文件。
            if !group.files.iter().any(|f| f.decision == MarkDecision::Keep) {
                if let Some(idx) = group
                    .files
                    .iter()
                    .position(|f| f.is_preferred)
                    .or(Some(0))
                {
                    group.files[idx].decision = MarkDecision::Keep;
                    group.files[idx]
                        .reasons
                        .push("整组标记清理时自动保留的一份".into());
                }
            }
        }

        group.recompute();
        recompute_stats(result);
        Ok(result.clone())
    })
}

/// 反向选择：选中所有非首选文件进行清理。
#[tauri::command]
pub fn invert_group(
    state: State<'_, AppState>,
    scan_id: String,
    group_id: Option<String>,
) -> AppResult<ScanResult> {
    session_mut(&state, &scan_id, |session| {
        let result = session.result.as_mut().ok_or_else(|| AppError::config("扫描尚未完成"))?;

        match group_id {
            Some(id) => {
                let group = find_group(result, &id)?;
                autoselect::invert(&mut group.files);
                group.recompute();
            }
            None => {
                for group in &mut result.groups {
                    if group.informational {
                        continue;
                    }
                    autoselect::invert(&mut group.files);
                    group.recompute();
                }
            }
        }

        recompute_stats(result);
        Ok(result.clone())
    })
}

pub fn find_group<'a>(result: &'a mut ScanResult, group_id: &str) -> AppResult<&'a mut DuplicateGroup> {
    result
        .groups
        .iter_mut()
        .find(|g| g.id == group_id)
        .ok_or_else(|| AppError::NotFound(format!("分组 {group_id}")))
}

pub fn recompute_stats(result: &mut ScanResult) {
    result.stats.groups = result.groups.len() as u64;
    result.stats.duplicate_files = result
        .groups
        .iter()
        .flat_map(|g| g.files.iter())
        .filter(|f| f.decision == MarkDecision::Remove)
        .count() as u64;
    result.stats.wasted_bytes = result.groups.iter().map(|g| g.wasted_bytes).sum();
    result.stats.protected_files = result
        .groups
        .iter()
        .flat_map(|g| g.files.iter())
        .filter(|f| f.protected.is_some())
        .count() as u64;
}

// ============================ 预览 ============================

#[tauri::command]
pub fn preview_text(left: String, right: String) -> AppResult<TextDiffPreview> {
    let left_path = PathBuf::from(&left);
    let right_path = PathBuf::from(&right);

    let left_meta = std::fs::metadata(&left_path).map_err(|_| AppError::NotFound(left.clone()))?;
    let right_meta = std::fs::metadata(&right_path).map_err(|_| AppError::NotFound(right.clone()))?;

    let first_diff = compare_streams(&left_path, &right_path)?;

    let left_bytes = read_head(&left_path, PREVIEW_LIMIT)?;
    let right_bytes = read_head(&right_path, PREVIEW_LIMIT)?;

    let binary = looks_binary(&left_bytes) || looks_binary(&right_bytes);

    let offset = first_diff.unwrap_or(0);
    let (left_snippet, right_snippet) = if binary {
        (String::new(), String::new())
    } else {
        (
            snippet(&left_bytes, offset),
            snippet(&right_bytes, offset),
        )
    };

    Ok(TextDiffPreview {
        left_path: left,
        right_path: right,
        identical: first_diff.is_none() && left_meta.len() == right_meta.len(),
        identical_prefix_bytes: first_diff.unwrap_or_else(|| left_meta.len().min(right_meta.len())),
        first_difference_offset: first_diff,
        left_snippet,
        right_snippet,
        left_truncated: left_meta.len() > PREVIEW_LIMIT,
        right_truncated: right_meta.len() > PREVIEW_LIMIT,
        binary,
    })
}

#[tauri::command]
pub fn preview_thumbnail(path: String, max_edge: Option<u32>) -> ThumbnailPreview {
    let edge = max_edge.unwrap_or(320).clamp(48, 1600);
    match perceptual::thumbnail_data_url(Path::new(&path), edge, 78) {
        Ok((data_url, width, height)) => ThumbnailPreview {
            path,
            data_url,
            width,
            height,
            error: None,
        },
        Err(e) => ThumbnailPreview {
            path,
            data_url: String::new(),
            width: 0,
            height: 0,
            error: Some(e),
        },
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPreview {
    pub path: String,
    pub duration_secs: f32,
    /// 24 个对数频带的能量轮廓，前端直接画成频谱柱状图。
    pub bands: Vec<f32>,
    pub error: Option<String>,
}

/// 音频「指纹轮廓」预览：比波形更能说明「这是不是同一段音频」，
/// 且无需把解码后的整段 PCM 传给前端。
#[tauri::command]
pub fn preview_audio(path: String) -> AudioPreview {
    match audio::compute(Path::new(&path)) {
        Ok(fp) => AudioPreview {
            path,
            duration_secs: fp.duration_secs,
            bands: fp.profile,
            error: None,
        },
        Err(e) => AudioPreview {
            path,
            duration_secs: 0.0,
            bands: Vec::new(),
            error: Some(e),
        },
    }
}

fn compare_streams(a: &Path, b: &Path) -> AppResult<Option<u64>> {
    let mut fa = std::fs::File::open(a)?;
    let mut fb = std::fs::File::open(b)?;
    let mut buf_a = vec![0u8; 64 * 1024];
    let mut buf_b = vec![0u8; 64 * 1024];
    let mut offset: u64 = 0;

    loop {
        let na = read_up_to(&mut fa, &mut buf_a)?;
        let nb = read_up_to(&mut fb, &mut buf_b)?;

        let common = na.min(nb);
        for i in 0..common {
            if buf_a[i] != buf_b[i] {
                return Ok(Some(offset + i as u64));
            }
        }
        if na != nb {
            return Ok(Some(offset + common as u64));
        }
        if na == 0 {
            return Ok(None);
        }
        offset += na as u64;
    }
}

fn read_up_to(file: &mut std::fs::File, buf: &mut [u8]) -> AppResult<usize> {
    let mut filled = 0usize;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(AppError::Io(e)),
        }
    }
    Ok(filled)
}

fn read_head(path: &Path, limit: u64) -> AppResult<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    file.take(limit).read_to_end(&mut buf)?;
    Ok(buf)
}

fn looks_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let sample = &bytes[..bytes.len().min(4096)];
    let weird = sample
        .iter()
        .filter(|b| **b == 0 || (**b < 0x09) || (**b > 0x0d && **b < 0x20))
        .count();
    weird * 100 / sample.len() > 8
}

/// 取差异位置附近的片段，并把不可打印字符替换成 `·`，让差异一眼可见。
fn snippet(bytes: &[u8], offset: u64) -> String {
    let start = (offset as usize).saturating_sub(SNIPPET_WINDOW);
    let end = ((offset as usize) + SNIPPET_WINDOW).min(bytes.len());
    if start >= bytes.len() {
        return String::new();
    }
    let slice = &bytes[start..end];
    let mut out = String::with_capacity(slice.len());
    for &b in slice {
        match b {
            b'\n' => out.push('\n'),
            b'\t' => out.push('\t'),
            0x20..=0x7e => out.push(b as char),
            _ => out.push('·'),
        }
    }
    out
}

// ============================ 导出 ============================

#[tauri::command]
pub fn export_report(
    state: State<'_, AppState>,
    scan_id: String,
    format: ExportFormat,
    out_path: Option<String>,
) -> AppResult<export::ExportSummary> {
    let result = finished_result(&state, &scan_id)?;
    let path = match out_path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => export::default_path(&result, format, &state.config.export_dir),
    };
    let summary = export::export(&result, format, &path)?;
    tracing::info!(format = ?format, rows = summary.rows, "报告已导出");
    Ok(summary)
}
