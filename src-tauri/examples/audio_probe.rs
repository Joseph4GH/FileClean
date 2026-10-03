//! 音频指纹诊断：打印各音频的时长、轮廓与两两余弦相似度。

use std::path::Path;

use fileclean_lib::dedupe::audio::{compute, cosine};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut fps = Vec::new();
    for a in &args {
        match compute(Path::new(a)) {
            Ok(fp) => {
                println!(
                    "{:<44} 时长={:>6.2}s  维度={}  前6维={:?}",
                    a,
                    fp.duration_secs,
                    fp.profile.len(),
                    fp.profile
                        .iter()
                        .take(6)
                        .map(|v| (v * 1000.0).round() / 1000.0)
                        .collect::<Vec<_>>()
                );
                fps.push((a.clone(), fp));
            }
            Err(e) => println!("{:<44} 失败: {e}", a),
        }
    }

    println!("\n--- 两两余弦相似度 ---");
    for i in 0..fps.len() {
        for j in (i + 1)..fps.len() {
            println!(
                "  {:.4}   {} <-> {}",
                cosine(&fps[i].1.profile, &fps[j].1.profile),
                fps[i].0,
                fps[j].0
            );
        }
    }
}
