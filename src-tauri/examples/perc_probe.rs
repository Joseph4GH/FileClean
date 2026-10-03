//! 临时诊断工具：打印若干图片的 pHash / dHash 与两两汉明距离。
//! 用法：cargo run --example perc_probe -- <img1> <img2> ...

use std::path::Path;

use fileclean_lib::dedupe::perceptual::{compute, hamming, similarity_from_distance};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("用法: perc_probe <img>...");
        std::process::exit(2);
    }

    let mut fps = Vec::new();
    for a in &args {
        match compute(Path::new(a)) {
            Ok(fp) => {
                println!(
                    "{:<56} {}x{}  phash={:016x}  dhash={:016x}",
                    a,
                    fp.width,
                    fp.height,
                    fp.phash,
                    fp.dhash
                );
                fps.push((a.clone(), fp));
            }
            Err(e) => println!("{:<56} 失败: {e}", a),
        }
    }

    println!("\n--- 两两汉明距离 ---");
    for i in 0..fps.len() {
        for j in (i + 1)..fps.len() {
            let dp = hamming(fps[i].1.phash, fps[j].1.phash);
            let dd = hamming(fps[i].1.dhash, fps[j].1.dhash);
            println!(
                "{}  <->  {}\n    phash dist={:>2}  sim={:.4}   dhash dist={:>2}  sim={:.4}",
                fps[i].0,
                fps[j].0,
                dp,
                similarity_from_distance(dp),
                dd,
                similarity_from_distance(dd),
            );
        }
    }
}
