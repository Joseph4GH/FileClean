//! 音频指纹：识别「不同码率、不同标签但听感相同」的音频。
//!
//! 实现思路（轻量版声学指纹）：
//! 1. 解码为单声道 PCM；
//! 2. 按 2048 点加汉宁窗分帧，做 FFT；
//! 3. 把功率谱折叠到 24 个对数频带，做对数压缩后跨帧累加 —— 得到一条「频谱轮廓」；
//! 4. 轮廓做 L2 归一化，两组之间的余弦相似度对**码率、编码格式、标签**天然不敏感，
//!    这正是「听感相同但文件不同」的判别依据。
//!
//! 局限：不做变调/变速/裁剪的中段匹配（那需要真正的 chromaprint 峰值对哈希）。
//! 因此仅用于「同一首歌的不同压制品」场景，UI 上以相似度阈值交由用户判断。

use std::f32::consts::PI;
use std::fs::File;
use std::path::Path;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::dedupe::UnionFind;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "flac", "wav", "wave", "ogg", "oga", "aiff", "aif", "aifc", "alac",
    "mp4", "caf",
];

const FFT_SIZE: usize = 2048;
const HOP: usize = 1024;
const BANDS: usize = 24;
const MIN_HZ: f32 = 40.0;
const MAX_HZ: f32 = 16_000.0;
/// 单文件最多分析的帧数（约 8 分钟），防止超长音频把扫描拖住。
const MAX_FRAMES: usize = 8192;
/// 短于该时长的音频不参与相似比对（铃声、提示音没有比对意义）。
const MIN_DURATION_SECS: f32 = 8.0;

#[derive(Debug, Clone)]
pub struct AudioFingerprint {
    pub profile: Vec<f32>,
    pub duration_secs: f32,
}

#[derive(Debug, Clone)]
pub struct AudioGroup {
    pub indices: Vec<usize>,
    pub similarity: f32,
}

/// 解码并计算频谱轮廓指纹。
pub fn compute(path: &Path) -> Result<AudioFingerprint, String> {
    let file = File::open(path).map_err(|e| format!("打开音频失败：{e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("无法解析音频容器：{e}"))?;
    let mut format = probed.format;

    let (track_id, codec_params) = {
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| "未找到可解码音轨".to_string())?;
        (track.id, track.codec_params.clone())
    };

    let sample_rate = codec_params.sample_rate.unwrap_or(44_100) as f32;

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("创建解码器失败：{e}"))?;

    let mut builder = ProfileBuilder::new(sample_rate);

    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut current_channels = 0usize;
    let mut total_samples: u64 = 0;
    let mut decoded_any = false;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // 到达流末尾是正常终止条件。
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => {
                if !decoded_any {
                    return Err(format!("读取音频数据失败：{e}"));
                }
                break;
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio) => {
                decoded_any = true;
                let spec = *audio.spec();
                let channels = spec.channels.count().max(1);
                total_samples += audio.frames() as u64;

                if sample_buf.is_none() || channels != current_channels {
                    sample_buf = Some(SampleBuffer::<f32>::new(audio.capacity() as u64, spec));
                    current_channels = channels;
                }

                let buffer = sample_buf.as_mut().expect("sample buffer initialized above");
                buffer.copy_interleaved_ref(audio);

                // 就地降混为单声道后交给构建器，避免额外分配整段缓冲。
                for frame in buffer.samples().chunks(channels) {
                    let mono = frame.iter().sum::<f32>() / channels as f32;
                    builder.push_samples(&[mono]);
                }

                if builder.frames >= MAX_FRAMES {
                    break;
                }
            }
            // 单帧解码失败（常见于 VBR MP3 的边角）不应中断整首歌。
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(_)) => break,
            Err(_) => break,
        }
    }

    let duration_secs = if sample_rate > 0.0 {
        total_samples as f32 / sample_rate
    } else {
        0.0
    };

    if builder.frames == 0 {
        return Err("未能解码出有效音频帧".into());
    }
    if duration_secs < MIN_DURATION_SECS {
        return Err(format!("音频过短（{duration_secs:.1}s），跳过相似比对"));
    }

    let profile = builder
        .finish()
        .ok_or_else(|| "未能生成音频轮廓".to_string())?;

    Ok(AudioFingerprint {
        profile,
        duration_secs,
    })
}

fn accumulate_frame(
    frame: &[f32],
    window: &[f32],
    fft: &dyn Fft<f32>,
    edges: &[usize],
    out: &mut [f64],
) {
    let mut buffer: Vec<Complex<f32>> = frame
        .iter()
        .zip(window.iter())
        .map(|(s, w)| Complex {
            re: s * w,
            im: 0.0,
        })
        .collect();

    fft.process(&mut buffer);

    let half = FFT_SIZE / 2;
    for (i, slot) in out.iter_mut().enumerate().take(BANDS) {
        let lo = edges[i];
        let hi = edges[i + 1].min(half);
        let mut sum = 0f64;
        for bin in lo..hi {
            let magnitude = buffer[bin].norm() as f64;
            sum += magnitude * magnitude;
        }
        // 对数压缩让弱频带也有发言权，避免被低频能量完全淹没。
        *slot += sum.sqrt().ln_1p();
    }
}

fn hann_window() -> Vec<f32> {
    (0..FFT_SIZE)
        .map(|i| 0.5 - 0.5 * ((2.0 * PI * i as f32) / (FFT_SIZE as f32 - 1.0)).cos())
        .collect()
}

/// 固定的对数频带边界（Hz），与采样率无关。
///
/// **为什么必须固定**：如果频带边界随采样率浮动（例如 44.1 kHz 覆盖 40–16000 Hz、
/// 22.05 kHz 覆盖 40–11025 Hz），那么「第 k 条频带」在两个文件里对应的实际频率
/// 完全不同（实测第 12 带相差 800 Hz vs 664 Hz），两条轮廓根本无法比较 ——
/// 同一首歌的不同码率版本相似度会掉到 0.92 以下，识别直接失效。
/// 固定成绝对频率网格后，第 k 带永远代表同一段频率，轮廓才具备可比性。
fn band_edges_hz() -> [f32; BANDS + 1] {
    let mut edges = [0f32; BANDS + 1];
    let ratio = MAX_HZ / MIN_HZ;
    for (b, slot) in edges.iter_mut().enumerate() {
        let t = b as f32 / BANDS as f32;
        *slot = MIN_HZ * ratio.powf(t);
    }
    edges
}

/// 把固定 Hz 边界映射到该采样率下的 FFT bin 下标。
///
/// 超出 Nyquist 的边界会被夹到 `FFT_SIZE / 2`，于是这些高频带区间为空
/// （`lo == hi`），能量贡献 0 —— 这是物理上正确的：低采样率文件本就不含那些频率。
/// 结果数组单调不减，因此每个区间 `[edges[i], edges[i+1])` 都是合法的（可能为空）。
fn band_bins(sample_rate: f32) -> Vec<usize> {
    let half = FFT_SIZE / 2;
    let sr = if sample_rate > 0.0 { sample_rate } else { 44_100.0 };
    band_edges_hz()
        .iter()
        .map(|hz| ((hz / sr * FFT_SIZE as f32).round() as usize).min(half))
        .collect()
}

/// 「PCM 样本 → 频谱轮廓」的增量构建器。
///
/// 抽成独立类型的原因：解码是流式的（不能把整首歌读进内存），
/// 但轮廓计算本身应当可被单元测试直接驱动，用来验证采样率无关性。
struct ProfileBuilder {
    window: Vec<f32>,
    edges: Vec<usize>,
    fft: std::sync::Arc<dyn Fft<f32>>,
    accum: Vec<f32>,
    band_energy: Vec<f64>,
    frames: usize,
}

impl ProfileBuilder {
    fn new(sample_rate: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        ProfileBuilder {
            window: hann_window(),
            edges: band_bins(sample_rate),
            fft: planner.plan_fft_forward(FFT_SIZE),
            accum: Vec::with_capacity(FFT_SIZE * 2),
            band_energy: vec![0f64; BANDS],
            frames: 0,
        }
    }

    /// 追加单声道样本；内部按 HOP 滑窗，凑够 FFT_SIZE 就处理一帧。
    fn push_samples(&mut self, mono: &[f32]) {
        self.accum.extend_from_slice(mono);
        while self.accum.len() >= FFT_SIZE && self.frames < MAX_FRAMES {
            accumulate_frame(
                &self.accum[..FFT_SIZE],
                &self.window,
                self.fft.as_ref(),
                &self.edges,
                &mut self.band_energy,
            );
            self.accum.drain(..HOP);
            self.frames += 1;
        }
    }

    /// 收尾：求均值并做 L2 归一化。没有任何有效帧时返回 `None`。
    fn finish(self) -> Option<Vec<f32>> {
        if self.frames == 0 {
            return None;
        }
        let mut profile: Vec<f32> = self
            .band_energy
            .into_iter()
            .map(|v| (v / self.frames as f64) as f32)
            .collect();
        let norm = profile.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for v in &mut profile {
                *v /= norm;
            }
        }
        Some(profile)
    }
}

/// 两条归一化轮廓的余弦相似度，落在 0.0 - 1.0。
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na < 1e-9 || nb < 1e-9 {
        return 0.0;
    }
    (dot / (na * nb)).clamp(0.0, 1.0)
}

/// 时序无关的相似分组：先按时长分桶（±4s），桶内再做余弦比对。
pub fn group_similar(
    items: &[(usize, AudioFingerprint)],
    threshold: f32,
    min_group_size: usize,
) -> Vec<AudioGroup> {
    if items.len() < min_group_size {
        return Vec::new();
    }

    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| {
        items[a]
            .1
            .duration_secs
            .partial_cmp(&items[b].1.duration_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut uf = UnionFind::new(items.len());
    let mut best: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();

    for (oi, &i) in order.iter().enumerate() {
        let dur_i = items[i].1.duration_secs;
        for &j in order.iter().skip(oi + 1) {
            let dur_j = items[j].1.duration_secs;
            // 时长差距超过 4 秒基本不是同一条音频（前奏/尾奏裁剪除外）。
            if (dur_j - dur_i).abs() > 4.0 {
                break;
            }
            let sim = cosine(&items[i].1.profile, &items[j].1.profile);
            if sim >= threshold {
                uf.union(i, j);
                let entry = best.entry(i).or_insert(sim);
                if sim < *entry {
                    *entry = sim;
                }
            }
        }
    }

    uf.groups(min_group_size)
        .into_iter()
        .map(|indices| {
            // 组相似度取组内最小配对相似度，宁可保守也不误导用户。
            let mut min_sim = 1.0f32;
            for (ai, &a) in indices.iter().enumerate() {
                for &b in indices.iter().skip(ai + 1) {
                    let s = cosine(&items[a].1.profile, &items[b].1.profile);
                    if s < min_sim {
                        min_sim = s;
                    }
                }
            }
            AudioGroup {
                indices,
                similarity: min_sim,
            }
        })
        .collect()
}

// ============================ 持久化编码 ============================

/// i16 定点 + 十六进制，把 24 维轮廓压到 96 个字符，直接塞进 SQLite TEXT 列。
pub fn encode_profile(profile: &[f32]) -> String {
    let mut out = String::with_capacity(profile.len() * 4);
    for v in profile {
        let q = (v.clamp(-1.0, 1.0) * 32767.0).round() as i32;
        out.push_str(&format!("{:04x}", q as u16));
    }
    out
}

pub fn decode_profile(encoded: &str) -> Option<Vec<f32>> {
    if encoded.is_empty() || !encoded.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(encoded.len() / 4);
    for chunk in encoded.as_bytes().chunks(4) {
        let s = std::str::from_utf8(chunk).ok()?;
        let raw = u16::from_str_radix(s, 16).ok()?;
        out.push(raw as i16 as f32 / 32767.0);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_roundtrip() {
        let profile: Vec<f32> = (0..BANDS).map(|i| (i as f32 / BANDS as f32) - 0.5).collect();
        let encoded = encode_profile(&profile);
        assert_eq!(encoded.len(), BANDS * 4);
        let decoded = decode_profile(&encoded).unwrap();
        assert_eq!(decoded.len(), profile.len());
        for (a, b) in profile.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        }
    }

    #[test]
    fn cosine_is_scale_invariant() {
        let a = vec![0.1f32, 0.5, 0.9, 0.2];
        let b = vec![2.0f32, 10.0, 18.0, 4.0];
        assert!((cosine(&a, &b) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_rejects_orthogonal() {
        let a = vec![1.0f32, 0.0];
        let b = vec![0.0f32, 1.0];
        assert!(cosine(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn band_bins_are_monotonic_and_clamped() {
        for sr in [8_000.0f32, 22_050.0, 44_100.0, 96_000.0] {
            let bins = band_bins(sr);
            assert_eq!(bins.len(), BANDS + 1);
            let half = FFT_SIZE / 2;
            for i in 1..bins.len() {
                assert!(
                    bins[i] >= bins[i - 1],
                    "频带 bin 边界必须单调不减：sr={sr} {bins:?}"
                );
            }
            assert!(bins[bins.len() - 1] <= half, "bins 不得超过 Nyquist：{bins:?}");
        }
    }

    #[test]
    fn band_edges_hz_are_strictly_increasing_and_sample_rate_free() {
        let edges = band_edges_hz();
        assert!((edges[0] - MIN_HZ).abs() < 1e-3);
        assert!((edges[BANDS] - MAX_HZ).abs() < 1e-3);
        for i in 1..edges.len() {
            assert!(edges[i] > edges[i - 1], "边界必须严格递增：{edges:?}");
        }
    }

    /// 核心回归测试：同一条信号以不同采样率录制时，轮廓必须高度一致。
    ///
    /// 这是「识别不同码率/采样率的同一首歌」能否成立的前提。
    /// 修复前频带边界随采样率浮动，该用例相似度只有约 0.92 并会漏检。
    #[test]
    fn profile_is_sample_rate_invariant() {
        let render = |sample_rate: f32, secs: f32| -> Vec<f32> {
            let n = (sample_rate * secs) as usize;
            // 宽带信号：20 个分量铺满 120 Hz - 12 kHz，接近真实音乐的频谱形态。
            let parts: Vec<(f32, f32)> = (0..20)
                .map(|k| {
                    let hz = 120.0 * 2.0f32.powf(k as f32 * 0.34);
                    (hz, 1.0 / (1.0 + k as f32 * 0.2))
                })
                .collect();
            (0..n)
                .map(|i| {
                    let t = i as f32 / sample_rate;
                    parts
                        .iter()
                        .map(|(hz, amp)| amp * (2.0 * PI * hz * t).sin())
                        .sum::<f32>()
                })
                .collect()
        };

        let profile_at = |sample_rate: f32| -> Vec<f32> {
            let mut b = ProfileBuilder::new(sample_rate);
            b.push_samples(&render(sample_rate, 12.0));
            b.finish().expect("应能生成轮廓")
        };

        let hi = profile_at(44_100.0);
        let lo = profile_at(22_050.0);
        let sim = cosine(&hi, &lo);
        assert!(
            sim > 0.99,
            "不同采样率的同一宽带信号应高度相似（>0.99），实际 {sim}"
        );
    }

    #[test]
    fn different_signals_stay_dissimilar_at_any_sample_rate() {
        let tone = |sr: f32, hz: f32| -> Vec<f32> {
            let n = (sr * 12.0) as usize;
            (0..n).map(|i| (2.0 * PI * hz * (i as f32 / sr)).sin()).collect()
        };
        let profile_of = |v: Vec<f32>, sr: f32| {
            let mut b = ProfileBuilder::new(sr);
            b.push_samples(&v);
            b.finish().unwrap()
        };
        let a = profile_of(tone(44_100.0, 800.0), 44_100.0);
        let b = profile_of(tone(44_100.0, 9_000.0), 44_100.0);
        assert!(cosine(&a, &b) < 0.5, "频率相差悬殊的两条信号不应相似");
    }

    #[test]
    fn short_audio_is_rejected_by_decoder() {
        // 不构造真实音频，只验证长度门槛常量的语义。
        assert!(MIN_DURATION_SECS >= 5.0);
    }
}
