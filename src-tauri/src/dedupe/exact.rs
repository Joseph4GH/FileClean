//! 精确去重：按「体积 + 全量哈希」分组。
//!
//! 同一分组内如果出现 `(device, inode)` 相同的成员，说明它们是同一物理文件的
//! 多个硬链接 —— 删除其中任一都不会释放空间，必须在动作层拦截。

use std::collections::HashMap;

use crate::domain::models::{FileEntry, MatchKind};

#[derive(Debug, Clone)]
pub struct ExactGroup {
    pub signature: String,
    pub size: u64,
    /// 组内成员在输入切片中的下标。
    pub indices: Vec<usize>,
    /// 该组内成员数（不是独立分身数）。
    pub member_count: usize,
    /// 去掉硬链接同源后的独立副本数。
    pub distinct_count: usize,
}

/// 输入必须已经过「体积预筛」，`hashes[i]` 为 `entries[i]` 的全量哈希。
pub fn build_groups(
    entries: &[FileEntry],
    hashes: &[Option<String>],
    min_group_size: usize,
) -> Vec<ExactGroup> {
    let mut buckets: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, hash) in hashes.iter().enumerate() {
        if let Some(h) = hash.as_deref() {
            buckets.entry(h).or_default().push(i);
        }
    }

    let mut groups: Vec<ExactGroup> = buckets
        .into_iter()
        .filter(|(_, idxs)| idxs.len() >= min_group_size)
        .map(|(hash, mut idxs)| {
            idxs.sort_unstable();

            // 统计互不相同的物理文件数（硬链接同源只算一个）。
            let mut seen: Vec<(u64, u64)> = Vec::new();
            for &i in &idxs {
                let e = &entries[i];
                let key = (e.device, e.inode);
                if e.inode != 0 && !seen.contains(&key) {
                    seen.push(key);
                } else if e.inode == 0 {
                    // 平台不支持 inode：退化为按路径计数。
                    seen.push((i as u64, i as u64));
                }
            }

            ExactGroup {
                signature: hash.to_string(),
                size: entries[idxs[0]].size,
                member_count: idxs.len(),
                distinct_count: seen.len().max(1),
                indices: idxs,
            }
        })
        .collect();

    // 浪费空间多的组排前面，用户第一眼看到最值得处理的。
    groups.sort_by(|a, b| {
        let wa = a.size * (a.member_count as u64 - a.distinct_count.min(a.member_count) as u64);
        let wb = b.size * (b.member_count as u64 - b.distinct_count.min(b.member_count) as u64);
        wb.cmp(&wa).then_with(|| b.size.cmp(&a.size))
    });
    groups
}

/// 组内是否为同一物理文件（硬链接同源）。
pub fn same_physical_file(a: &FileEntry, b: &FileEntry) -> bool {
    a.inode != 0 && b.inode != 0 && a.inode == b.inode && a.device == b.device
}

pub fn kind() -> MatchKind {
    MatchKind::Exact
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::FileEntry;

    fn entry(path: &str, size: u64, inode: u64) -> FileEntry {
        FileEntry {
            path: path.into(),
            root: "/r".into(),
            size,
            modified: 0,
            created: 0,
            extension: "bin".into(),
            inode,
            device: 1,
            hard_links: 1,
            is_symlink: false,
            depth: 1,
            relative_path: path.into(),
        }
    }

    #[test]
    fn groups_only_real_duplicates() {
        let entries = vec![
            entry("/r/a.bin", 100, 1),
            entry("/r/b.bin", 100, 2),
            entry("/r/c.bin", 100, 3),
            entry("/r/d.bin", 200, 4),
        ];
        let hashes = vec![
            Some("h1".into()),
            Some("h1".into()),
            Some("h1".into()),
            Some("h2".into()),
        ];
        let groups = build_groups(&entries, &hashes, 2);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].indices, vec![0, 1, 2]);
        assert_eq!(groups[0].distinct_count, 3);
    }

    #[test]
    fn detects_hardlink_same_inode() {
        let entries = vec![entry("/r/a.bin", 100, 7), entry("/r/b.bin", 100, 7)];
        let hashes = vec![Some("h".into()), Some("h".into())];
        let groups = build_groups(&entries, &hashes, 2);
        assert_eq!(groups[0].member_count, 2);
        assert_eq!(groups[0].distinct_count, 1);
        assert!(same_physical_file(&entries[0], &entries[1]));
    }
}
