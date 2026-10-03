//! RAW + JPEG 配对识别。
//!
//! 相机（以及多数手机）会同时写出一份 RAW 与一份 JPEG，两者同名不同后缀。
//! 它们**不是重复文件**，而是同一张照片的两种载体 —— 批量清理时把 JPEG 删掉
//! 往往正是用户后悔的那一步。这里把它们识别出来并标记为整体，UI 上给出提示。

use std::collections::HashMap;

use crate::domain::models::FileEntry;
use crate::dedupe::perceptual::{JPEG_EXTENSIONS, RAW_EXTENSIONS};

#[derive(Debug, Clone)]
pub struct RawJpegPair {
    /// 不含扩展名的文件名（保留原始大小写）。
    pub stem: String,
    pub dir: String,
    pub raw_indices: Vec<usize>,
    pub jpeg_indices: Vec<usize>,
}

impl RawJpegPair {
    pub fn all_indices(&self) -> Vec<usize> {
        let mut v = self.raw_indices.clone();
        v.extend_from_slice(&self.jpeg_indices);
        v
    }

    pub fn label(&self) -> String {
        format!(
            "{}（{} 个 RAW + {} 个 JPEG）",
            self.stem,
            self.raw_indices.len(),
            self.jpeg_indices.len()
        )
    }
}

pub fn find_pairs(entries: &[FileEntry]) -> Vec<RawJpegPair> {
    let mut buckets: HashMap<(String, String), (String, Vec<usize>, Vec<usize>)> = HashMap::new();

    for (i, entry) in entries.iter().enumerate() {
        let ext = entry.extension.as_str();
        let is_raw = RAW_EXTENSIONS.contains(&ext);
        let is_jpeg = JPEG_EXTENSIONS.contains(&ext);
        if !is_raw && !is_jpeg {
            continue;
        }

        let name = entry.file_name();
        let Some(dot) = name.rfind('.') else { continue };
        let stem = name[..dot].to_string();
        if stem.is_empty() {
            continue;
        }

        let key = (entry.parent_dir().to_string(), stem.to_ascii_lowercase());
        let slot = buckets
            .entry(key)
            .or_insert_with(|| (stem.clone(), Vec::new(), Vec::new()));
        if is_raw {
            slot.1.push(i);
        } else {
            slot.2.push(i);
        }
    }

    let mut pairs: Vec<RawJpegPair> = buckets
        .into_iter()
        .filter(|(_, (_, raws, jpegs))| !raws.is_empty() && !jpegs.is_empty())
        .map(|((dir, _), (stem, mut raw_indices, mut jpeg_indices))| {
            raw_indices.sort_unstable();
            jpeg_indices.sort_unstable();
            RawJpegPair {
                stem,
                dir,
                raw_indices,
                jpeg_indices,
            }
        })
        .collect();

    pairs.sort_by(|a, b| a.dir.cmp(&b.dir).then_with(|| a.stem.cmp(&b.stem)));
    pairs
}

/// 返回所有「属于某个 RAW/JPEG 配对」的文件下标，用于把它们从精确/相似去重候选中剔除。
pub fn paired_indices(pairs: &[RawJpegPair]) -> Vec<usize> {
    let mut out: Vec<usize> = pairs.iter().flat_map(|p| p.all_indices()).collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::FileEntry;

    fn entry(path: &str, ext: &str) -> FileEntry {
        FileEntry {
            path: path.into(),
            root: "/r".into(),
            size: 10,
            modified: 0,
            created: 0,
            extension: ext.into(),
            inode: 0,
            device: 0,
            hard_links: 1,
            is_symlink: false,
            depth: 2,
            relative_path: path.trim_start_matches("/r/").into(),
        }
    }

    #[test]
    fn pairs_raw_with_jpeg_same_stem() {
        let entries = vec![
            entry("/r/IMG_0001.CR2", "cr2"),
            entry("/r/IMG_0001.JPG", "jpg"),
            entry("/r/IMG_0002.CR2", "cr2"),
        ];
        let pairs = find_pairs(&entries);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].stem, "IMG_0001");
        assert_eq!(pairs[0].raw_indices, vec![0]);
        assert_eq!(pairs[0].jpeg_indices, vec![1]);
    }

    #[test]
    fn different_folders_do_not_pair() {
        let entries = vec![
            entry("/r/a/IMG_1.NEF", "nef"),
            entry("/r/b/IMG_1.JPG", "jpg"),
        ];
        assert!(find_pairs(&entries).is_empty());
    }
}
