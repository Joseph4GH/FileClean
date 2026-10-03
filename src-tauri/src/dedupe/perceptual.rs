//! 感知哈希：识别「被裁剪、调色、压缩或调整过尺寸」的同一张图片。
//!
//! - **pHash**（DCT 感知哈希）：对亮度做 32×32 DCT，取左上 8×8 低频系数与中位数比较，
//!   对整体调色、轻微裁剪、重新压缩最稳健。
//! - **dHash**（差值哈希）：9×8 灰度图相邻像素比较，对缩放极其不敏感。
//!
//! 两者都以 64 位表示；检索用 **BK-tree** 做汉明距离近邻查询，复杂度远低于两两比对。

use std::collections::HashMap;
use std::path::Path;

use image::imageops::FilterType;
use image::GrayImage;

use crate::dedupe::UnionFind;

pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "avif",
    "ico", "pnm", "tga", "dds",
];

pub const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "dng", "raf", "orf", "rw2", "pef", "srw",
    "3fr", "erf", "kdc", "mrw", "rwl", "x3f",
];

pub const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "jfif"];

/// 使用的 pHash 有效位数（8×8 去掉 DC 分量）。
const PHASH_BITS: u32 = 63;
const DCT_SIZE: usize = 32;
const PHASH_SIZE: usize = 8;

/// 解码像素上限：超过则跳过，防止异常图片打爆内存。
const MAX_PIXELS: u64 = 120_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageFingerprint {
    pub phash: u64,
    pub dhash: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct SimilarGroup {
    pub indices: Vec<usize>,
    pub similarity: f32,
    pub distance: u32,
}

/// 计算一张图片的感知指纹。失败返回人类可读原因（跳过该文件而不是中断扫描）。
pub fn compute(path: &Path) -> Result<ImageFingerprint, String> {
    let (w, h) = image::image_dimensions(path).map_err(|e| format!("读取图片尺寸失败：{e}"))?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("图片分辨率过大（{w}×{h}），已跳过"));
    }

    let decoded = image::open(path).map_err(|e| format!("解码图片失败：{e}"))?;
    let luma = decoded.to_luma8();

    Ok(ImageFingerprint {
        phash: phash(&luma),
        dhash: dhash(&luma),
        width: w,
        height: h,
    })
}

fn phash(luma: &GrayImage) -> u64 {
    let small = image::imageops::resize(luma, DCT_SIZE as u32, DCT_SIZE as u32, FilterType::Triangle);
    let mut input = Vec::with_capacity(DCT_SIZE * DCT_SIZE);
    for p in small.pixels() {
        input.push(p[0] as f32);
    }

    let coeffs = dct_2d(&input, DCT_SIZE);

    // 中位数剔除 DC 分量后计算，保证亮度整体偏移不影响结果。
    let mut low: Vec<f32> = Vec::with_capacity(PHASH_SIZE * PHASH_SIZE - 1);
    for y in 0..PHASH_SIZE {
        for x in 0..PHASH_SIZE {
            if x == 0 && y == 0 {
                continue;
            }
            low.push(coeffs[y * DCT_SIZE + x]);
        }
    }
    let median = median(&mut low);

    let mut bits: u64 = 0;
    let mut bit = 0u32;
    for y in 0..PHASH_SIZE {
        for x in 0..PHASH_SIZE {
            if x == 0 && y == 0 {
                continue;
            }
            if coeffs[y * DCT_SIZE + x] > median {
                bits |= 1u64 << bit;
            }
            bit += 1;
        }
    }
    bits
}

fn dhash(luma: &GrayImage) -> u64 {
    let small = image::imageops::resize(luma, 9, 8, FilterType::Triangle);
    let mut bits: u64 = 0;
    let mut bit = 0u32;
    for y in 0..8u32 {
        for x in 0..8u32 {
            let left = small.get_pixel(x, y)[0];
            let right = small.get_pixel(x + 1, y)[0];
            if right > left {
                bits |= 1u64 << bit;
            }
            bit += 1;
        }
    }
    bits
}

/// 二维 DCT-II（可分离实现：先行后列）。
fn dct_2d(pixels: &[f32], n: usize) -> Vec<f32> {
    let mut rows = vec![0f32; n * n];
    let mut row_out = vec![0f32; n];
    for y in 0..n {
        dct_1d(&pixels[y * n..y * n + n], &mut row_out, n);
        rows[y * n..y * n + n].copy_from_slice(&row_out);
    }

    let mut out = vec![0f32; n * n];
    let mut col_in = vec![0f32; n];
    let mut col_out = vec![0f32; n];
    for x in 0..n {
        for y in 0..n {
            col_in[y] = rows[y * n + x];
        }
        dct_1d(&col_in, &mut col_out, n);
        for y in 0..n {
            out[y * n + x] = col_out[y];
        }
    }
    out
}

fn dct_1d(input: &[f32], output: &mut [f32], n: usize) {
    use std::f32::consts::PI;
    let scale0 = (1.0 / n as f32).sqrt();
    let scale = (2.0 / n as f32).sqrt();
    for (u, slot) in output.iter_mut().enumerate().take(n) {
        let mut sum = 0f32;
        for (x, value) in input.iter().enumerate().take(n) {
            sum += value * (((2 * x + 1) as f32 * u as f32 * PI) / (2.0 * n as f32)).cos();
        }
        *slot = sum * if u == 0 { scale0 } else { scale };
    }
}

fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[values.len() / 2]
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

pub fn similarity_from_distance(distance: u32) -> f32 {
    (1.0 - distance as f32 / PHASH_BITS as f32).clamp(0.0, 1.0)
}

// ============================ BK-tree ============================

struct BkNode {
    hash: u64,
    children: HashMap<u32, usize>,
    items: Vec<usize>,
}

/// 度量树：汉明距离满足三角不等式，因此可以用 BK-tree 做半径查询剪枝。
struct BkTree {
    nodes: Vec<BkNode>,
}

impl BkTree {
    fn new() -> Self {
        BkTree { nodes: Vec::new() }
    }

    fn insert(&mut self, hash: u64, item: usize) {
        if self.nodes.is_empty() {
            self.nodes.push(BkNode {
                hash,
                children: HashMap::new(),
                items: vec![item],
            });
            return;
        }

        let mut cur = 0usize;
        loop {
            let d = hamming(self.nodes[cur].hash, hash);
            if d == 0 {
                self.nodes[cur].items.push(item);
                return;
            }
            match self.nodes[cur].children.get(&d).copied() {
                Some(next) => cur = next,
                None => {
                    let idx = self.nodes.len();
                    self.nodes.push(BkNode {
                        hash,
                        children: HashMap::new(),
                        items: vec![item],
                    });
                    self.nodes[cur].children.insert(d, idx);
                    return;
                }
            }
        }
    }

    fn search(&self, hash: u64, radius: u32) -> Vec<usize> {
        let mut hits = Vec::new();
        if self.nodes.is_empty() {
            return hits;
        }
        let mut stack = vec![0usize];
        while let Some(cur) = stack.pop() {
            let node = &self.nodes[cur];
            let d = hamming(node.hash, hash);
            if d <= radius {
                hits.extend_from_slice(&node.items);
            }
            let lo = d.saturating_sub(radius);
            let hi = d.saturating_add(radius);
            for (&edge, &child) in &node.children {
                if edge >= lo && edge <= hi {
                    stack.push(child);
                }
            }
        }
        hits
    }
}

/// 基于 pHash + BK-tree 的相似分组。
///
/// `min_similarity` 即用户拖动的阈值滑块（0.9 = 90% 相似）。
pub fn group_similar(
    fingerprints: &[ImageFingerprint],
    min_similarity: f32,
    min_group_size: usize,
) -> Vec<SimilarGroup> {
    if fingerprints.len() < min_group_size {
        return Vec::new();
    }

    let radius = ((1.0 - min_similarity.clamp(0.0, 1.0)) * PHASH_BITS as f32).round() as u32;

    let mut tree = BkTree::new();
    for (i, fp) in fingerprints.iter().enumerate() {
        tree.insert(fp.phash, i);
    }

    let mut uf = UnionFind::new(fingerprints.len());
    for (i, fp) in fingerprints.iter().enumerate() {
        for hit in tree.search(fp.phash, radius) {
            if hit != i {
                uf.union(i, hit);
            }
        }
    }

    uf.groups(min_group_size)
        .into_iter()
        .map(|indices| {
            // 组内两两求最坏距离；组很大时抽样，避免 O(k²) 拖慢整体。
            let step = (indices.len() / 64).max(1);
            let mut best = u32::MAX;
            for (ai, &a) in indices.iter().enumerate() {
                for &b in indices.iter().skip(ai + 1).step_by(step) {
                    best = best.min(hamming(fingerprints[a].phash, fingerprints[b].phash));
                }
            }
            let distance = if best == u32::MAX { 0 } else { best };
            SimilarGroup {
                indices,
                distance,
                similarity: similarity_from_distance(distance),
            }
        })
        .collect()
}

// ============================ 缩略图 ============================

/// 生成缩略图的 data URL（内联进前端，无需 asset 协议与额外 CSP 放行）。
pub fn thumbnail_data_url(path: &Path, max_edge: u32, quality: u8) -> Result<(String, u32, u32), String> {
    let decoded = image::open(path).map_err(|e| format!("解码失败：{e}"))?;
    let (w, h) = (decoded.width(), decoded.height());
    if w == 0 || h == 0 {
        return Err("图片尺寸为 0".into());
    }

    let scale = (max_edge as f32 / w.max(h) as f32).min(1.0);
    let tw = ((w as f32 * scale).round() as u32).max(1);
    let th = ((h as f32 * scale).round() as u32).max(1);

    let thumb = decoded.resize_exact(tw, th, FilterType::Triangle);
    let mut buf = Vec::new();
    {
        let mut encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
        // 统一转 RGB 输出，规避 CMYK / 带 alpha 图片的编码限制。
        encoder
            .encode_image(&thumb.to_rgb8())
            .map_err(|e| format!("编码缩略图失败：{e}"))?;
    }

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok((format!("data:image/jpeg;base64,{encoded}"), w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn solid(w: u32, h: u32, color: [u8; 3]) -> tempfile::NamedTempFile {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(w, h, Rgb(color));
        let f = tempfile::NamedTempFile::new().unwrap();
        img.save(f.path().with_extension("png")).unwrap();
        f
    }

    #[test]
    fn identical_images_have_zero_distance() {
        let a = solid(64, 64, [30, 120, 200]);
        let pa = a.path().with_extension("png");
        let fp1 = compute(&pa).unwrap();
        let fp2 = compute(&pa).unwrap();
        assert_eq!(hamming(fp1.phash, fp2.phash), 0);
    }

    #[test]
    fn threshold_maps_to_radius() {
        // 90% 相似 ≈ 允许 6 位差异（63 * 0.1）。
        let radius = ((1.0f32 - 0.9) * PHASH_BITS as f32).round() as u32;
        assert_eq!(radius, 6);
        assert!((similarity_from_distance(0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn bk_tree_finds_within_radius() {
        let mut tree = BkTree::new();
        tree.insert(0b0, 0);
        tree.insert(0b111, 1); // 距离 3
        tree.insert(!0u64, 2); // 距离 63
        let hits = tree.search(0b0, 3);
        assert!(hits.contains(&0));
        assert!(hits.contains(&1));
        assert!(!hits.contains(&2));
    }

    #[test]
    fn group_merges_near_duplicates() {
        let base = ImageFingerprint {
            phash: 0x0,
            dhash: 0,
            width: 10,
            height: 10,
        };
        let near = ImageFingerprint {
            phash: 0b11,
            ..base
        };
        let far = ImageFingerprint {
            phash: 0xFFFF_FFFF_FFFF_FFFF,
            ..base
        };
        let groups = group_similar(&[base, near, far], 0.9, 2);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].indices, vec![0, 1]);
    }
}
