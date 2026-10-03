//! 哈希算法实现层：统一入口 + 分级读取策略。
//!
//! 三级策略，精度递增、代价递增：
//! 1. `partial_hash` —— 首尾各 64 KiB + 文件长度，用于把绝大多数「大小相同但内容不同」
//!    的文件在几十毫秒内筛掉；
//! 2. `hash_file` —— 对通过预筛的候选做全量哈希，是最终判定依据；
//! 3. 超大文件走内存映射，避免把 4K 视频 / ISO 整体读进堆内存。

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use digest::Digest;

use crate::domain::models::HashAlgorithm;

/// 流式读取的块大小。1 MiB 在 NVMe 上是吞吐与内存占用的较优折中。
const CHUNK: usize = 1024 * 1024;

/// 分块预筛读取的头部 / 尾部窗口。
pub const PARTIAL_WINDOW: u64 = 64 * 1024;

/// 超过该体积才启用内存映射：小文件建映射表比直接读还慢。
pub const MMAP_THRESHOLD: u64 = 32 * 1024 * 1024;

enum HasherImpl {
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
    Blake3(Box<blake3::Hasher>),
    Xxh3(Box<xxhash_rust::xxh3::Xxh3>),
}

impl HasherImpl {
    fn new(algo: HashAlgorithm) -> Self {
        match algo {
            HashAlgorithm::Md5 => HasherImpl::Md5(md5::Md5::new()),
            HashAlgorithm::Sha1 => HasherImpl::Sha1(sha1::Sha1::new()),
            HashAlgorithm::Sha256 => HasherImpl::Sha256(sha2::Sha256::new()),
            HashAlgorithm::Blake3 => HasherImpl::Blake3(Box::new(blake3::Hasher::new())),
            HashAlgorithm::Xxhash3 => HasherImpl::Xxh3(Box::new(xxhash_rust::xxh3::Xxh3::new())),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            HasherImpl::Md5(h) => h.update(data),
            HasherImpl::Sha1(h) => h.update(data),
            HasherImpl::Sha256(h) => h.update(data),
            HasherImpl::Blake3(h) => {
                h.update(data);
            }
            HasherImpl::Xxh3(h) => h.update(data),
        }
    }

    fn finalize_hex(self) -> String {
        match self {
            HasherImpl::Md5(h) => hex::encode(h.finalize()),
            HasherImpl::Sha1(h) => hex::encode(h.finalize()),
            HasherImpl::Sha256(h) => hex::encode(h.finalize()),
            HasherImpl::Blake3(h) => h.finalize().to_hex().to_string(),
            HasherImpl::Xxh3(h) => format!("{:016x}", h.digest()),
        }
    }
}

/// 对任意 `Read` 做流式计算（压缩包内条目、内存缓冲都走这里）。
pub fn hash_reader(algo: HashAlgorithm, reader: &mut dyn Read) -> io::Result<String> {
    let mut hasher = HasherImpl::new(algo);
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize_hex())
}

pub fn hash_bytes(algo: HashAlgorithm, data: &[u8]) -> String {
    let mut hasher = HasherImpl::new(algo);
    hasher.update(data);
    hasher.finalize_hex()
}

/// 预筛指纹：`size:head_tail_xxh3`。不追求密码学强度，只求极快。
pub fn partial_hash(path: &Path, size: u64) -> io::Result<String> {
    let mut file = File::open(path)?;

    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    hasher.update(&size.to_le_bytes());

    let head_len = PARTIAL_WINDOW.min(size) as usize;
    let mut head = vec![0u8; head_len];
    if head_len > 0 {
        read_exact_up_to(&mut file, &mut head)?;
        hasher.update(&head);
    }

    // 只有当文件足够大时尾部窗口才与头部不重叠。
    if size > PARTIAL_WINDOW * 2 {
        use std::io::{Seek, SeekFrom};
        let tail_len = PARTIAL_WINDOW as usize;
        let mut tail = vec![0u8; tail_len];
        file.seek(SeekFrom::End(-(tail_len as i64)))?;
        read_exact_up_to(&mut file, &mut tail)?;
        hasher.update(&tail);
    }

    Ok(format!("{}:{:016x}", size, hasher.digest()))
}

/// 全量哈希。大文件走内存映射（blake3 还能借 rayon 做文件内并行）。
pub fn hash_file(algo: HashAlgorithm, path: &Path, size: u64) -> io::Result<String> {
    if size >= MMAP_THRESHOLD {
        if let Some(hexdigest) = try_mmap_hash(algo, path) {
            return Ok(hexdigest);
        }
        // mmap 失败（网络文件系统 / 权限 / 文件被截断）时静默回退到流式读取。
    }
    let mut file = File::open(path)?;
    // File 是 Read；用带预分配的缓冲减少系统调用。
    let mut reader = io::BufReader::with_capacity(CHUNK, &mut file);
    hash_reader(algo, &mut reader)
}

fn try_mmap_hash(algo: HashAlgorithm, path: &Path) -> Option<String> {
    // BLAKE3 自带 mmap + rayon 路径：按需分片读取，既不占堆内存也能单文件跑满多核。
    if algo == HashAlgorithm::Blake3 {
        return blake3::Hasher::new()
            .update_mmap_rayon(path)
            .ok()
            .map(|h| h.finalize().to_hex().to_string());
    }

    let file = File::open(path).ok()?;
    // SAFETY: 只读映射。若文件在映射期间被外部截断，访问会触发 SIGBUS ——
    // 这是 mmap 固有的取舍；调用方对「扫描中文件被修改」的场景已有缓存失效兜底。
    let mmap = unsafe { memmap2::Mmap::map(&file).ok()? };

    let mut hasher = HasherImpl::new(algo);
    hasher.update(&mmap);
    Some(hasher.finalize_hex())
}

/// 读取直到填满缓冲区或到达文件末尾（文件可能小于缓冲区）。
fn read_exact_up_to(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_file(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(bytes).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn same_content_same_hash() {
        for algo in HashAlgorithm::all() {
            let a = tmp_file(b"hello fileclean");
            let b = tmp_file(b"hello fileclean");
            let ha = hash_file(algo, a.path(), 15).unwrap();
            let hb = hash_file(algo, b.path(), 15).unwrap();
            assert_eq!(ha, hb, "算法 {algo:?} 对相同内容应产生相同摘要");
        }
    }

    #[test]
    fn different_content_different_hash() {
        for algo in HashAlgorithm::all() {
            let a = tmp_file(b"content-a");
            let b = tmp_file(b"content-b");
            assert_ne!(
                hash_file(algo, a.path(), 9).unwrap(),
                hash_file(algo, b.path(), 9).unwrap(),
                "算法 {algo:?} 对相同长度不同内容必须区分"
            );
        }
    }

    #[test]
    fn partial_hash_distinguishes_same_size() {
        let a = tmp_file(&vec![0xAAu8; 4096]);
        let b = tmp_file(&vec![0xBBu8; 4096]);
        assert_ne!(
            partial_hash(a.path(), 4096).unwrap(),
            partial_hash(b.path(), 4096).unwrap()
        );
    }

    #[test]
    fn digest_lengths_match_declaration() {
        let f = tmp_file(b"abc");
        for algo in HashAlgorithm::all() {
            assert_eq!(
                hash_file(algo, f.path(), 3).unwrap().len(),
                algo.digest_hex_len(),
                "算法 {algo:?} 摘要长度与声明不符"
            );
        }
    }
}
