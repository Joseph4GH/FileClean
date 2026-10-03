//! 智能选择助手：用可组合的规则链自动决定「每组保留哪一份」。
//!
//! 规则链按数组顺序构成**字典序优先级**：前一条规则能分出胜负就不再往后看。
//! 这正是需求里「优先保留 RAW 格式，若无 RAW 则保留最高分辨率 JPG」的表达方式 ——
//! 把 `PreferExtension` 放在 `KeepLargest` 前面即可。

use std::cmp::Ordering;

use crate::dedupe::exact::same_physical_file;
use crate::domain::models::{
    AutoMarkConfig, DuplicateFile, MarkDecision, MarkRule, MarkRuleEntry, MatchKind,
};

#[derive(Debug, Clone)]
pub struct FileMark {
    pub index: usize,
    pub decision: MarkDecision,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GroupMark {
    pub preferred: Option<usize>,
    pub marks: Vec<FileMark>,
}

/// 对单个分组执行自动标记。
pub fn mark_group(
    files: &[DuplicateFile],
    kind: MatchKind,
    config: &AutoMarkConfig,
) -> GroupMark {
    if files.is_empty() {
        return GroupMark {
            preferred: None,
            marks: Vec::new(),
        };
    }

    // 信息性分组（RAW/JPEG 配对）不参与清理，全部保留。
    if kind.is_informational() {
        return GroupMark {
            preferred: None,
            marks: files
                .iter()
                .enumerate()
                .map(|(i, _)| FileMark {
                    index: i,
                    decision: MarkDecision::Keep,
                    reasons: vec!["RAW 与 JPEG 是同一张照片的两种载体，整体保留".into()],
                })
                .collect(),
        };
    }

    // 相似组是否纳入自动标记，由用户决定（精确重复更安全）。
    let is_similar = matches!(kind, MatchKind::Perceptual | MatchKind::Audio);
    if is_similar && !config.also_mark_similar {
        return GroupMark {
            preferred: None,
            marks: files
                .iter()
                .enumerate()
                .map(|(i, _)| FileMark {
                    index: i,
                    decision: MarkDecision::Undecided,
                    reasons: vec!["相似组默认不自动标记，请人工确认后选择".into()],
                })
                .collect(),
        };
    }

    let active_rules: Vec<&MarkRuleEntry> = config.rules.iter().filter(|r| r.enabled).collect();

    // 排序：索引 0 为「最该保留」。
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|&a, &b| compare(files, a, b, &active_rules));

    let keep_count = config.keep_per_group.max(1).min(files.len());
    let mut total_keep = keep_count;

    // 受保护的文件无论排序如何都必须保留。
    let protected: Vec<usize> = files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.protected.is_some())
        .map(|(i, _)| i)
        .collect();
    if config.never_delete_protected {
        total_keep = total_keep.max(protected.len());
    }

    let mut kept: Vec<usize> = Vec::new();
    // 硬链接同源：同一物理文件的多个路径不能互相「去重」，全部保留。
    for (i, file) in files.iter().enumerate() {
        if files
            .iter()
            .enumerate()
            .any(|(j, other)| j < i && same_physical_file(&file.entry, &other.entry))
        {
            kept.push(i);
        }
    }
    for idx in &protected {
        if !kept.contains(idx) {
            kept.push(*idx);
        }
    }
    for idx in &order {
        if kept.len() >= total_keep {
            break;
        }
        if !kept.contains(idx) {
            kept.push(*idx);
        }
    }
    // 保底：任何情况下每组至少留一份，绝不允许「清空整组」。
    if kept.is_empty() {
        if let Some(first) = order.first() {
            kept.push(*first);
        }
    }

    let preferred = order
        .iter()
        .copied()
        .find(|i| kept.contains(i));

    let mut marks: Vec<FileMark> = Vec::with_capacity(files.len());
    for (i, file) in files.iter().enumerate() {
        if kept.contains(&i) {
            let mut reasons = Vec::new();
            if let Some(reason) = &file.protected {
                reasons.push(format!("已保护：{reason}"));
            }
            if kept.iter().any(|k| same_physical_file(&file.entry, &files[*k].entry)) {
                reasons.push("与组内其它条目指向同一物理文件（硬链接），删除不释放空间".into());
            }
            if reasons.is_empty() {
                if let Some(winner) = preferred {
                    if winner == i {
                        reasons.push(describe_winner(files, i, &order, &active_rules));
                    } else {
                        reasons.push("按保底策略保留".into());
                    }
                }
            }
            marks.push(FileMark {
                index: i,
                decision: MarkDecision::Keep,
                reasons,
            });
        } else {
            marks.push(FileMark {
                index: i,
                decision: MarkDecision::Remove,
                reasons: describe_loser(files, i, preferred, &active_rules),
            });
        }
    }

    GroupMark { preferred, marks }
}

fn compare(
    files: &[DuplicateFile],
    a: usize,
    b: usize,
    rules: &[&MarkRuleEntry],
) -> Ordering {
    for entry in rules {
        let ord = apply_rule(&entry.rule, files, a, b);
        if ord != Ordering::Equal {
            return ord;
        }
    }
    // 稳定兜底：路径短者优先，再按字典序保证结果可复现。
    files[a]
        .entry
        .path_len()
        .cmp(&files[b].entry.path_len())
        .then_with(|| files[a].entry.path.cmp(&files[b].entry.path))
}

/// `Ordering::Less` 表示 a 比 b 更值得保留。
fn apply_rule(rule: &MarkRule, files: &[DuplicateFile], a: usize, b: usize) -> Ordering {
    let fa = &files[a].entry;
    let fb = &files[b].entry;

    match rule {
        MarkRule::KeepNewest => fb.modified.cmp(&fa.modified),
        MarkRule::KeepOldest => fa.modified.cmp(&fb.modified),
        MarkRule::KeepOldestCreated => {
            // created 为 0 表示平台不提供，退化为 modified 比较。
            let (ca, cb) = (
                if fa.created > 0 { fa.created } else { fa.modified },
                if fb.created > 0 { fb.created } else { fb.modified },
            );
            ca.cmp(&cb)
        }
        MarkRule::KeepShortestPath => fa.path_len().cmp(&fb.path_len()),
        MarkRule::KeepLongestName => fb
            .file_name()
            .chars()
            .count()
            .cmp(&fa.file_name().chars().count()),
        MarkRule::KeepShallowest => fa.depth.cmp(&fb.depth),
        MarkRule::KeepLargest => fb.size.cmp(&fa.size),
        MarkRule::KeepMostLinked => fb.hard_links.cmp(&fa.hard_links),
        MarkRule::KeepInPath { pattern } => {
            let needle = pattern.to_lowercase();
            let ha = fa.path.to_lowercase().contains(&needle);
            let hb = fb.path.to_lowercase().contains(&needle);
            match (ha, hb) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => Ordering::Equal,
            }
        }
        MarkRule::PreferExtension { extensions } => {
            let rank = |ext: &str| -> usize {
                extensions
                    .iter()
                    .position(|e| e.trim_start_matches('.').eq_ignore_ascii_case(ext))
                    .unwrap_or(usize::MAX)
            };
            rank(&fa.extension).cmp(&rank(&fb.extension))
        }
    }
}

fn describe_winner(
    files: &[DuplicateFile],
    winner: usize,
    order: &[usize],
    rules: &[&MarkRuleEntry],
) -> String {
    let Some(second) = order.iter().copied().find(|i| *i != winner) else {
        return "组内唯一保留项".into();
    };
    match deciding_rule(files, winner, second, rules) {
        Some(rule) => format!("保留原因：{}", rule.label()),
        None => "保留原因：路径最短（默认兜底规则）".into(),
    }
}

fn describe_loser(
    files: &[DuplicateFile],
    loser: usize,
    winner: Option<usize>,
    rules: &[&MarkRuleEntry],
) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(w) = winner {
        reasons.push(format!(
            "组内已保留 {}",
            files[w].entry.file_name()
        ));
        if let Some(rule) = deciding_rule(files, w, loser, rules) {
            reasons.push(format!("保留项更优：{}", rule.label()));
        }
    }
    reasons
}

fn deciding_rule<'a>(
    files: &[DuplicateFile],
    better: usize,
    worse: usize,
    rules: &[&'a MarkRuleEntry],
) -> Option<&'a MarkRule> {
    rules
        .iter()
        .find(|entry| apply_rule(&entry.rule, files, better, worse) == Ordering::Less)
        .map(|entry| &entry.rule)
}

/// 反向选择：把所有「非首选」的文件标记为待清理。
pub fn invert(files: &mut [DuplicateFile]) {
    for file in files.iter_mut() {
        if file.protected.is_some() || file.is_symlink {
            file.decision = MarkDecision::Keep;
            continue;
        }
        file.decision = if file.is_preferred {
            MarkDecision::Remove
        } else {
            MarkDecision::Keep
        };
        file.reasons = vec!["反向选择：原首选改为清理，其余保留".into()];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::FileEntry;

    fn dup(path: &str, modified: i64, created: i64, size: u64) -> DuplicateFile {
        let ext = path.rsplit('.').next().unwrap_or("").to_string();
        let depth = path.matches('/').count();
        DuplicateFile::from_entry(FileEntry {
            path: path.into(),
            root: "/r".into(),
            size,
            modified,
            created,
            extension: ext,
            inode: 0,
            device: 0,
            hard_links: 1,
            is_symlink: false,
            depth,
            relative_path: path.into(),
        })
    }

    fn cfg(rules: Vec<MarkRule>) -> AutoMarkConfig {
        AutoMarkConfig {
            rules: rules
                .into_iter()
                .map(|rule| MarkRuleEntry {
                    rule,
                    enabled: true,
                })
                .collect(),
            keep_per_group: 1,
            never_delete_protected: true,
            also_mark_similar: false,
        }
    }

    #[test]
    fn prefers_raw_over_jpeg() {
        let files = vec![
            dup("/r/a.jpg", 100, 100, 5_000_000),
            dup("/r/a.cr2", 50, 50, 30_000_000),
        ];
        let config = cfg(vec![MarkRule::PreferExtension {
            extensions: vec!["cr2".into()],
        }]);
        let mark = mark_group(&files, MatchKind::Exact, &config);
        assert_eq!(mark.preferred, Some(1));
        let decisions: Vec<MarkDecision> = mark.marks.iter().map(|m| m.decision).collect();
        assert_eq!(decisions, vec![MarkDecision::Remove, MarkDecision::Keep]);
    }

    #[test]
    fn keeps_shortest_path_when_no_other_rule_decides() {
        let files = vec![
            dup("/r/deep/nested/very/a.bin", 100, 100, 10),
            dup("/r/a.bin", 100, 100, 10),
        ];
        let config = cfg(vec![MarkRule::KeepShortestPath]);
        let mark = mark_group(&files, MatchKind::Exact, &config);
        assert_eq!(mark.preferred, Some(1));
    }

    #[test]
    fn rule_order_is_respected() {
        // 先按创建时间最早；若相同才看体积。
        let files = vec![
            dup("/r/new_big.bin", 200, 200, 999),
            dup("/r/old_small.bin", 100, 100, 1),
        ];
        let config = cfg(vec![MarkRule::KeepOldestCreated, MarkRule::KeepLargest]);
        let mark = mark_group(&files, MatchKind::Exact, &config);
        assert_eq!(mark.preferred, Some(1), "创建时间优先规则应压过体积规则");
    }

    #[test]
    fn protected_files_are_never_marked_for_removal() {
        let mut files = vec![dup("/r/a.bin", 1, 1, 10), dup("/r/b.bin", 99, 99, 10)];
        files[0].protected = Some("被 Lightroom 引用".into());
        let config = cfg(vec![MarkRule::KeepNewest]);
        let mark = mark_group(&files, MatchKind::Exact, &config);
        let protected_mark = mark.marks.iter().find(|m| m.index == 0).unwrap();
        assert_eq!(protected_mark.decision, MarkDecision::Keep);
    }

    #[test]
    fn informational_groups_keep_everything() {
        let files = vec![dup("/r/a.cr2", 1, 1, 10), dup("/r/a.jpg", 1, 1, 10)];
        let mark = mark_group(&files, MatchKind::RawJpegPair, &AutoMarkConfig::default());
        assert!(mark.marks.iter().all(|m| m.decision == MarkDecision::Keep));
    }

    #[test]
    fn similar_groups_are_not_auto_marked_by_default() {
        let files = vec![dup("/r/a.jpg", 1, 1, 10), dup("/r/b.jpg", 2, 2, 10)];
        let mark = mark_group(&files, MatchKind::Perceptual, &AutoMarkConfig::default());
        assert!(mark.marks.iter().all(|m| m.decision == MarkDecision::Undecided));
    }
}
