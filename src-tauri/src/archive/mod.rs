//! 压缩包内条目扫描：不落盘解压即可检索 ZIP / 7Z 内部的重复文件。
//!
//! - **ZIP**：原生流式读取，逐条目计算内容哈希；
//! - **7Z**：原生流式读取（`sevenz-rust`）；
//! - **RAR**：格式封闭，仅在系统存在 `7z` / `7zz` 命令行工具时通过其列表输出
//!   （尺寸 + CRC32）做指纹；缺失工具时如实返回「不支持」而不是静默跳过。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::models::HashAlgorithm;
use crate::hash::algorithms;

pub const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "7z", "rar", "jar", "apk", "cbz", "war", "ear"];

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub archive_path: String,
    /// 压缩包内部路径。
    pub inner_path: String,
    pub size: u64,
    /// 内容指纹。ZIP/7Z 为内容哈希；RAR 为 `size:crc32`。
    pub fingerprint: String,
    /// 指纹是否为密码学哈希（false 表示只是 CRC32 级别的弱指纹）。
    pub strong: bool,
}

#[derive(Debug, Default)]
pub struct ArchiveScanOutcome {
    pub entries: Vec<ArchiveEntry>,
    pub archives_scanned: u64,
    pub warnings: Vec<String>,
}

/// 扫描一批压缩包。
pub fn scan_archives(
    paths: &[PathBuf],
    algo: HashAlgorithm,
    max_entry_bytes: u64,
) -> ArchiveScanOutcome {
    let mut out = ArchiveScanOutcome::default();

    for path in paths {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();

        let result = match ext.as_str() {
            "zip" | "jar" | "apk" | "cbz" | "war" | "ear" => scan_zip(path, algo, max_entry_bytes),
            "7z" => scan_7z(path, algo, max_entry_bytes),
            "rar" => scan_rar_via_cli(path),
            other => Err(format!("不支持的压缩格式：{other}")),
        };

        match result {
            Ok(mut entries) => {
                out.archives_scanned += 1;
                out.entries.append(&mut entries);
            }
            Err(e) => {
                out.warnings
                    .push(format!("{}：{e}", path.file_name().unwrap_or_default().to_string_lossy()));
            }
        }
    }

    out
}

fn scan_zip(
    path: &Path,
    algo: HashAlgorithm,
    max_entry_bytes: u64,
) -> Result<Vec<ArchiveEntry>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析 ZIP 失败：{e}"))?;

    let mut out = Vec::new();
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                // 加密条目 / 损坏条目：跳过单条而不是整包失败。
                tracing::debug!(archive = %path.display(), error = %e, "跳过 ZIP 条目");
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        let size = entry.size();
        if max_entry_bytes > 0 && size > max_entry_bytes {
            continue;
        }

        // 先限定读取长度，防止 ZIP 炸弹把磁盘打满。
        let mut limited = (&mut entry).take(size);
        let fingerprint = algorithms::hash_reader(algo, &mut limited)
            .map_err(|e| format!("读取条目失败：{e}"))?;

        out.push(ArchiveEntry {
            archive_path: path.to_string_lossy().to_string(),
            inner_path: entry.name().to_string(),
            size,
            fingerprint,
            strong: true,
        });
    }
    Ok(out)
}

fn scan_7z(
    path: &Path,
    algo: HashAlgorithm,
    max_entry_bytes: u64,
) -> Result<Vec<ArchiveEntry>, String> {
    use sevenz_rust::{Password, SevenZReader};

    let archive_path = path.to_string_lossy().to_string();
    let mut reader =
        SevenZReader::open(path, Password::empty()).map_err(|e| format!("解析 7Z 失败：{e}"))?;

    let mut out: Vec<ArchiveEntry> = Vec::new();
    let mut failure: Option<String> = None;

    let walk_result = reader.for_each_entries(|entry, reader| {
        if entry.is_directory() {
            return Ok(true);
        }
        let size = entry.size();
        if max_entry_bytes > 0 && size > max_entry_bytes {
            return Ok(true);
        }

        let mut limited = reader.take(size);
        match algorithms::hash_reader(algo, &mut limited) {
            Ok(fingerprint) => {
                out.push(ArchiveEntry {
                    archive_path: archive_path.clone(),
                    inner_path: entry.name().to_string(),
                    size,
                    fingerprint,
                    strong: true,
                });
                Ok(true)
            }
            Err(e) => {
                failure = Some(format!("读取 7Z 条目失败：{e}"));
                Ok(false)
            }
        }
    });

    if let Some(e) = failure {
        return Err(e);
    }
    walk_result.map_err(|e| format!("遍历 7Z 失败：{e}"))?;
    Ok(out)
}

/// RAR 没有可用的纯 Rust 解码器；借用系统 7-Zip 的列表输出（尺寸 + CRC32）作为指纹。
fn scan_rar_via_cli(path: &Path) -> Result<Vec<ArchiveEntry>, String> {
    let tool = find_7z_tool().ok_or_else(|| {
        "RAR 需要系统安装 7-Zip 命令行工具（7z / 7zz）才能扫描，已跳过".to_string()
    })?;

    let output = Command::new(tool)
        .arg("l")
        .arg("-slt")
        .arg("-bd")
        .arg(path)
        .output()
        .map_err(|e| format!("调用 7z 失败：{e}"))?;

    if !output.status.success() {
        return Err("7z 无法读取该 RAR（可能加密或版本过旧）".into());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let archive_path = path.to_string_lossy().to_string();
    let mut out = Vec::new();
    let mut size: u64 = 0;
    let mut crc = String::new();
    let mut name = String::new();

    let flush = |out: &mut Vec<ArchiveEntry>, name: &mut String, size: u64, crc: &str| {
        if !name.is_empty() && !crc.is_empty() {
            out.push(ArchiveEntry {
                archive_path: String::new(), // 由调用方回填
                inner_path: std::mem::take(name),
                size,
                fingerprint: format!("{size}:{}", crc.to_ascii_lowercase()),
                strong: false,
            });
        } else {
            name.clear();
        }
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush(&mut out, &mut name, size, &crc);
            size = 0;
            crc.clear();
            continue;
        }
        if let Some(v) = line.strip_prefix("Path = ") {
            name = v.to_string();
        } else if let Some(v) = line.strip_prefix("Size = ") {
            size = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("CRC = ") {
            crc = v.trim().to_string();
        }
    }
    flush(&mut out, &mut name, size, &crc);

    for e in &mut out {
        e.archive_path = archive_path.clone();
    }
    Ok(out)
}

fn find_7z_tool() -> Option<&'static str> {
    for candidate in ["7zz", "7z", "/opt/homebrew/bin/7zz", "/usr/local/bin/7z"] {
        let ok = Command::new(candidate)
            .arg("i")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(candidate);
        }
    }
    None
}

/// 按指纹分组，仅在**同一个压缩包内部**判定重复 —— 跨包条目虽然内容相同，
/// 但删除其中一份并不能让另一个包变小，对用户没有意义。
pub fn group_internal(entries: &[ArchiveEntry], min_group_size: usize) -> Vec<(String, Vec<usize>)> {
    let mut buckets: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        buckets
            .entry((e.archive_path.as_str(), e.fingerprint.as_str()))
            .or_default()
            .push(i);
    }

    let mut groups: Vec<(String, Vec<usize>)> = buckets
        .into_iter()
        .filter(|(_, idxs)| idxs.len() >= min_group_size)
        .map(|((_, fp), idxs)| (fp.to_string(), idxs))
        .collect();
    groups.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    groups
}

/// 重写 ZIP 以移除指定条目（保留其余条目的原始压缩方式与元数据）。
///
/// 采用「写临时文件 + 原子替换」，中途失败不会破坏原压缩包。
pub fn remove_zip_entries(archive_path: &Path, inner_paths: &[String]) -> Result<u64, String> {
    use std::io::Write;

    if inner_paths.is_empty() {
        return Ok(0);
    }
    let removals: std::collections::HashSet<&str> = inner_paths.iter().map(|s| s.as_str()).collect();

    let file = std::fs::File::open(archive_path).map_err(|e| format!("打开压缩包失败：{e}"))?;
    let mut source = zip::ZipArchive::new(file).map_err(|e| format!("解析 ZIP 失败：{e}"))?;

    let tmp_path = archive_path.with_extension("fileclean-tmp");
    let tmp_file = std::fs::File::create(&tmp_path).map_err(|e| format!("创建临时文件失败：{e}"))?;
    let mut writer = zip::ZipWriter::new(tmp_file);

    let mut removed = 0u64;
    let mut buffer = vec![0u8; 256 * 1024];

    for i in 0..source.len() {
        let mut entry = source.by_index(i).map_err(|e| format!("读取条目失败：{e}"))?;
        let name = entry.name().to_string();

        if removals.contains(name.as_str()) {
            removed += 1;
            continue;
        }

        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
            .compression_method(entry.compression())
            .unix_permissions(entry.unix_mode().unwrap_or(0o644));

        writer
            .start_file(name, options)
            .map_err(|e| format!("写入条目失败：{e}"))?;
        loop {
            let n = entry.read(&mut buffer).map_err(|e| format!("读取条目失败：{e}"))?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buffer[..n])
                .map_err(|e| format!("写入条目失败：{e}"))?;
        }
    }

    writer.finish().map_err(|e| format!("收尾写入失败：{e}"))?;

    if removed == 0 {
        let _ = std::fs::remove_file(&tmp_path);
        return Ok(0);
    }

    std::fs::rename(&tmp_path, archive_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("替换原压缩包失败：{e}")
    })?;

    Ok(removed)
}
