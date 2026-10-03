//! 引用完整性保护：避免「删掉被其它软件引用的文件」导致工程打不开。
//!
//! 覆盖三类引用来源：
//! 1. **素材目录数据库** —— Lightroom 目录（`*.lrcat`，本质是 SQLite）、
//!    Apple Music / iTunes 资料库（plist XML）、Premiere 工程（`*.prproj`，gzip XML）；
//! 2. **链接解析** —— 符号链接、macOS Alias（`book`/`mark` 魔数）、Windows `.lnk`；
//! 3. **系统关键目录** —— 由 [`crate::scan::filter::ProtectedPaths`] 负责，此处不重复。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rusqlite::OpenFlags;
use walkdir::WalkDir;

use crate::domain::models::{FileEntry, ReferenceHit, ReferenceReport};

/// 别名/快捷方式解析的数量上限，避免在异常目录下无限派生进程。
const MAX_LINK_RESOLUTIONS: usize = 300;
/// 目录发现的最大递归深度。
const CATALOG_MAX_DEPTH: usize = 6;

#[derive(Debug, Default)]
pub struct ReferenceIndex {
    /// 归一化后的绝对路径集合。
    exact_paths: HashSet<String>,
    /// 归一化后的基名集合（弱引用）。
    basenames: HashSet<String>,
    /// 路径 -> 引用来源说明。
    sources: std::collections::HashMap<String, String>,
    report: ReferenceReport,
}

impl ReferenceIndex {
    pub fn report(&self) -> &ReferenceReport {
        &self.report
    }

    pub fn is_empty(&self) -> bool {
        self.exact_paths.is_empty() && self.basenames.is_empty()
    }

    /// 判断某个待删除文件是否被引用。
    pub fn check(&self, entry: &FileEntry) -> Option<ReferenceHit> {
        let full = normalize(&entry.path);
        if self.exact_paths.contains(&full) {
            let source = self
                .sources
                .get(&full)
                .cloned()
                .unwrap_or_else(|| "素材库".to_string());
            return Some(ReferenceHit {
                path: entry.path.clone(),
                source: source.clone(),
                detail: format!("被 {source} 直接引用（完整路径命中）"),
            });
        }

        let base = normalize(entry.file_name());
        if self.basenames.contains(&base) {
            return Some(ReferenceHit {
                path: entry.path.clone(),
                source: "素材库".into(),
                detail: "文件名与素材库中的引用同名（可能被引用，保守起见不建议删除）".into(),
            });
        }
        None
    }

    /// 构建索引：扫描素材库 + 解析链接。
    pub fn build(roots: &[String], resolve_links: bool) -> ReferenceIndex {
        let mut index = ReferenceIndex::default();

        let catalogs = discover_catalogs(roots);
        for catalog in &catalogs {
            let name = catalog
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let ext = catalog
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();

            let result = match ext.as_str() {
                "lrcat" => read_lightroom(catalog),
                "xml" => read_plist_xml(catalog),
                "prproj" => read_premiere(catalog),
                _ => continue,
            };

            match result {
                Ok(paths) => {
                    index.report.catalogs_scanned.push(name.clone());
                    for p in paths {
                        let normalized = normalize(&p);
                        index.sources.insert(normalized.clone(), name.clone());
                        index
                            .basenames
                            .insert(normalize(file_name_of(&p)));
                        index.exact_paths.insert(normalized);
                    }
                }
                Err(e) => {
                    index.report.warnings.push(format!("{name}：{e}"));
                }
            }
        }

        if resolve_links {
            for (link, target) in collect_link_targets(roots) {
                let normalized = normalize(&target);
                index
                    .sources
                    .insert(normalized.clone(), format!("链接 {link}"));
                index.basenames.insert(normalize(file_name_of(&target)));
                index.exact_paths.insert(normalized);
            }
        }

        index
    }
}

fn file_name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// 统一归一化：小写 + 正斜杠 + 去掉重复分隔符，用于跨平台路径比较。
fn normalize(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut last_sep = false;
    for ch in path.chars() {
        let c = if ch == '\\' { '/' } else { ch };
        if c == '/' {
            if last_sep {
                continue;
            }
            last_sep = true;
        } else {
            last_sep = false;
        }
        out.extend(c.to_lowercase());
    }
    out
}

// ============================ 素材库发现 ============================

fn discover_catalogs(roots: &[String]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    // 标准位置优先。
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        for candidate in [
            "Pictures/Lightroom",
            "Pictures",
            "Music/Music",
            "Music/iTunes",
            "Music",
            "Movies",
            "Documents",
            "Desktop",
        ] {
            let dir = home.join(candidate);
            if dir.is_dir() {
                collect_from_dir(&dir, &mut found, &mut seen);
            }
        }
    }

    for root in roots {
        let dir = PathBuf::from(root);
        if dir.is_dir() {
            collect_from_dir(&dir, &mut found, &mut seen);
        }
    }

    // 去重后限制总量，避免用户把整个磁盘选进来时枚举过度。
    found.truncate(64);
    found
}

fn collect_from_dir(dir: &Path, found: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    let walker = WalkDir::new(dir)
        .max_depth(CATALOG_MAX_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !matches!(name.as_ref(), "node_modules" | ".git" | "Caches" | ".Trash")
        });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();

        let interesting = match ext.as_str() {
            "lrcat" | "prproj" => true,
            "xml" => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                name.contains("library") || name.contains("itunes")
            }
            _ => false,
        };

        if interesting && seen.insert(path.to_path_buf()) {
            found.push(path.to_path_buf());
            if found.len() >= 128 {
                return;
            }
        }
    }
}

// ============================ Lightroom ============================

fn read_lightroom(path: &Path) -> Result<Vec<String>, String> {
    let conn = rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("打开 Lightroom 目录失败：{e}"))?;

    let sql = "SELECT r.absolutePath, f.pathFromRoot, fi.baseName, fi.extension
               FROM AgLibraryFile fi
               JOIN AgLibraryFolder f ON fi.folder = f.id
               JOIN AgLibraryRootFolder r ON f.rootFolder = r.id";

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("读取 Lightroom 目录结构失败：{e}"))?;

    let rows = stmt
        .query_map([], |row| {
            let root: String = row.get(0)?;
            let rel: String = row.get(1)?;
            let base: String = row.get(2)?;
            let ext: Option<String> = row.get(3)?;
            Ok((root, rel, base, ext))
        })
        .map_err(|e| format!("查询 Lightroom 素材失败：{e}"))?;

    let mut out = Vec::new();
    for row in rows.flatten() {
        let (root, rel, base, ext) = row;
        let mut full = PathBuf::from(root).join(rel);
        let file_name = match ext {
            Some(e) if !e.is_empty() => format!("{base}.{e}"),
            _ => base,
        };
        full.push(file_name);
        out.push(full.to_string_lossy().to_string());
    }
    Ok(out)
}

// ============================ Apple Music / iTunes ============================

fn read_plist_xml(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读取资料库 XML 失败：{e}"))?;

    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = text[cursor..].find("file://") {
        let start = cursor + offset + "file://".len();
        let rest = &text[start..];
        let end = rest.find('<').unwrap_or(rest.len());
        let raw = rest[..end].trim();
        if let Some(decoded) = percent_decode(raw) {
            out.push(decoded);
        }
        cursor = start + end;
        if out.len() > 500_000 {
            break;
        }
    }

    if out.is_empty() {
        return Err("未在资料库中找到任何文件引用".into());
    }
    Ok(out)
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            match u8::from_str_radix(hex, 16) {
                Ok(b) => {
                    out.push(b);
                    i += 3;
                    continue;
                }
                Err(_) => {
                    out.push(bytes[i]);
                    i += 1;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let text = String::from_utf8_lossy(&out).to_string();

    // `file://localhost/Users/...` 与 `file:///Users/...` 两种写法都要处理。
    let trimmed = text
        .strip_prefix("localhost")
        .map(|s| s.to_string())
        .unwrap_or(text);
    Some(trimmed)
}

// ============================ Premiere ============================

fn read_premiere(path: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开工程失败：{e}"))?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut xml = String::new();
    decoder
        .read_to_string(&mut xml)
        .map_err(|e| format!("解压工程失败（可能不是 gzip 压缩的 .prproj）：{e}"))?;

    let mut out = Vec::new();
    for key in ["<ActualMediaFilePath>", "<FilePath>"] {
        let mut cursor = 0usize;
        while let Some(offset) = xml[cursor..].find(key) {
            let start = cursor + offset + key.len();
            let rest = &xml[start..];
            let end = rest.find('<').unwrap_or(rest.len());
            let raw = rest[..end].trim();
            if !raw.is_empty() {
                let decoded = percent_decode(raw)
                    .unwrap_or_else(|| raw.to_string())
                    .replace("file://localhost", "")
                    .replace("file://", "");
                if decoded.starts_with('/') || decoded.contains(":\\") {
                    out.push(decoded);
                }
            }
            cursor = start + end;
        }
    }

    if out.is_empty() {
        return Err("工程中未找到媒体引用记录".into());
    }
    Ok(out)
}

// ============================ 链接解析 ============================

/// 收集 `(链接路径, 目标路径)` 对。
fn collect_link_targets(roots: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();

    for root in roots {
        let walker = WalkDir::new(root)
            .max_depth(CATALOG_MAX_DEPTH)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !matches!(name.as_ref(), "node_modules" | ".git" | "Caches" | "Library")
            });

        for entry in walker.flatten() {
            if out.len() >= MAX_LINK_RESOLUTIONS {
                return out;
            }
            let path = entry.path();
            let file_type = entry.file_type();

            if file_type.is_symlink() {
                if let Ok(target) = std::fs::read_link(path) {
                    let absolute = if target.is_absolute() {
                        target
                    } else {
                        path.parent().map(|p| p.join(&target)).unwrap_or(target)
                    };
                    out.push((
                        path.to_string_lossy().to_string(),
                        absolute.to_string_lossy().to_string(),
                    ));
                }
                continue;
            }

            if !file_type.is_file() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();

            if ext == "lnk" {
                if let Some(target) = parse_lnk(path) {
                    out.push((path.to_string_lossy().to_string(), target));
                }
                continue;
            }

            if is_macos_alias(path) {
                if let Some(target) = resolve_macos_alias(path) {
                    out.push((path.to_string_lossy().to_string(), target));
                }
            }
        }
    }

    out
}

/// macOS Alias 文件以 `book` + 4 字节版本 + `mark` 开头。
fn is_macos_alias(path: &Path) -> bool {
    let mut header = [0u8; 12];
    match std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut header)) {
        Ok(()) => &header[0..4] == b"book" && &header[8..12] == b"mark",
        Err(_) => false,
    }
}

fn resolve_macos_alias(path: &Path) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        // Finder 是系统唯一可靠的 Alias 解析器，通过 AppleScript 借道。
        let script = format!(
            "try\n tell application \"Finder\" to set p to POSIX path of (alias (POSIX file \"{}\"))\n return p\nend try",
            path.to_string_lossy().replace('"', "\\\"")
        );
        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            return None;
        }
        Some(value)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        None
    }
}

/// `.lnk` 解析：完整 Shell Link 二进制格式很复杂，这里做的是**启发式**提取 ——
/// 扫描 LinkInfo / StringData 段落中的本地路径字符串。命中失败只是漏保护，不会误删。
fn parse_lnk(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;

    // 先试 UTF-16LE（Shell Link 的 StringData 默认编码）。
    let mut candidates: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i + 1 < data.len() {
        if data[i].is_ascii_alphanumeric() || data[i] == b'/' || data[i] == b'\\' {
            let start = i;
            let mut units: Vec<u16> = Vec::new();
            let mut j = i;
            while j + 1 < data.len() {
                let unit = u16::from_le_bytes([data[j], data[j + 1]]);
                if unit == 0 {
                    break;
                }
                let ch = char::from_u32(unit as u32)?;
                if ch.is_control() {
                    break;
                }
                units.push(unit);
                j += 2;
            }
            if j > start + 6 {
                if let Ok(s) = String::from_utf16(&units) {
                    if s.contains(":\\") || s.starts_with("\\\\") {
                        candidates.push(s);
                    }
                }
            }
            i = j.max(i + 2);
        } else {
            i += 1;
        }
    }

    candidates
        .into_iter()
        .max_by_key(|s| s.len())
        .map(|s| s.trim_end_matches('\0').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_is_case_and_separator_insensitive() {
        assert_eq!(normalize("/Users/A/B\\C"), "/users/a/b/c");
        assert_eq!(normalize("/Users//A///B"), "/users/a/b");
    }

    #[test]
    fn percent_decode_handles_chinese() {
        let decoded = percent_decode("localhost/Users/x/%E5%8E%9F%E7%89%87/a.jpg").unwrap();
        assert_eq!(decoded, "/Users/x/原片/a.jpg");
    }

    #[test]
    fn reference_index_protects_by_path_and_basename() {
        let mut index = ReferenceIndex::default();
        index.exact_paths.insert(normalize("/a/b/c.jpg"));
        index.basenames.insert(normalize("c.jpg"));

        let entry = FileEntry {
            path: "/a/b/c.jpg".into(),
            root: "/a".into(),
            size: 1,
            modified: 0,
            created: 0,
            extension: "jpg".into(),
            inode: 1,
            device: 1,
            hard_links: 1,
            is_symlink: false,
            depth: 2,
            relative_path: "b/c.jpg".into(),
        };
        assert!(index.check(&entry).is_some());

        // 只是同名（不同目录）也应触发弱保护提示。
        let other = FileEntry {
            path: "/x/y/c.jpg".into(),
            ..entry
        };
        let hit = index.check(&other).unwrap();
        assert!(hit.detail.contains("同名"));
    }
}
