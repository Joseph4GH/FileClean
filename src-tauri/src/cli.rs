//! 命令行接口：让查重能力可以被备份脚本、NAS 定时任务与 CI/CD 直接调用。
//!
//! 设计原则：
//! - 人类可读摘要走 stderr，机器可读结果走 stdout（`--json`），互不污染；
//! - 默认**只读**，任何清理动作都需要显式 `--trash/--hardlink/--move-to` 加 `--yes`；
//! - 退出码：0 成功、1 出错、3 发现重复项（便于在 shell 里做条件分支）。

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use clap::{Args, Parser, Subcommand};

use crate::actions::{self, ActionContext};
use crate::config::AppConfig;
use crate::db::Db;
use crate::domain::models::*;
use crate::error::AppResult;
use crate::export;
use crate::pipeline;
use crate::scan::filter::ProtectedPaths;

const EXIT_OK: i32 = 0;
const EXIT_ERROR: i32 = 1;
const EXIT_FOUND: i32 = 3;

#[derive(Parser, Debug)]
#[command(
    name = "fileclean",
    version,
    about = "FileClean — 重复文件扫描与安全清理（Tauri + Rust）",
    long_about = "不传任何子命令时启动图形界面。\n\n\
        CLI 示例：\n  \
        fileclean scan ~/Pictures ~/Downloads --algorithm blake3 --format html --output report.html\n  \
        fileclean scan /data --perceptual --perceptual-threshold 0.92 --json\n  \
        fileclean scan ~/Photos --trash --yes --min-size 1048576"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 扫描目录并查找重复文件
    Scan(ScanArgs),
    /// 打印默认保留策略（可直接作为 --rule 的取值参考）
    Rules,
    /// 列出本地保存的会话
    Sessions,
}

#[derive(Args, Debug)]
pub struct ScanArgs {
    /// 一个或多个扫描根目录
    #[arg(required = true, value_name = "PATH")]
    paths: Vec<String>,

    #[arg(short = 'a', long, default_value = "blake3", value_name = "ALGO")]
    algorithm: String,

    #[arg(long, default_value_t = 1, value_name = "BYTES")]
    min_size: u64,

    #[arg(long, value_name = "BYTES")]
    max_size: Option<u64>,

    #[arg(long, value_delimiter = ',', value_name = "EXT")]
    include_ext: Vec<String>,

    #[arg(long, value_delimiter = ',', value_name = "EXT")]
    exclude_ext: Vec<String>,

    #[arg(long, value_delimiter = ',', value_name = "GLOB")]
    exclude_glob: Vec<String>,

    #[arg(long)]
    include_hidden: bool,

    #[arg(long)]
    follow_symlinks: bool,

    #[arg(long)]
    no_cache: bool,

    #[arg(long)]
    no_protect_system: bool,

    #[arg(short = 'j', long, default_value_t = 0, value_name = "N")]
    threads: usize,

    #[arg(long, default_value_t = 2, value_name = "N")]
    min_group: usize,

    /// 启用图片感知哈希相似检测
    #[arg(long)]
    perceptual: bool,

    #[arg(long, default_value_t = 0.90, value_name = "0-1")]
    perceptual_threshold: f32,

    /// 启用音频指纹相似检测
    #[arg(long)]
    audio: bool,

    #[arg(long, default_value_t = 0.92, value_name = "0-1")]
    audio_threshold: f32,

    /// 扫描 ZIP / 7Z 内部条目（RAR 需系统安装 7z）
    #[arg(long)]
    archives: bool,

    /// 检查素材库引用与链接，保护被引用的文件
    #[arg(long)]
    references: bool,

    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    raw_jpeg_pairing: bool,

    /// 生成报告文件
    #[arg(long, value_name = "FORMAT", value_parser = ["csv", "html", "xml", "json"])]
    format: Option<String>,

    #[arg(short = 'o', long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// 把完整结果以 JSON 打到 stdout
    #[arg(long)]
    json: bool,

    #[arg(short = 'q', long)]
    quiet: bool,

    /// ===== 以下为清理相关（默认不执行）=====
    #[arg(long)]
    trash: bool,

    #[arg(long)]
    hardlink: bool,

    #[arg(long, value_name = "DIR")]
    move_to: Option<PathBuf>,

    /// 只演练，不落盘
    #[arg(long)]
    dry_run: bool,

    /// 显式确认执行清理（缺少则拒绝执行）
    #[arg(long)]
    yes: bool,

    /// 在默认策略前追加一条保留规则，可重复
    #[arg(long, value_name = "RULE", value_parser = ["newest", "oldest", "oldest-created", "shortest", "longest-name", "shallowest", "largest", "most-linked", "in-photos", "prefer-raw"])]
    rule: Vec<String>,
}

impl ScanArgs {
    fn to_options(&self) -> AppResult<ScanOptions> {
        let mut options = ScanOptions::default();
        options.roots = self
            .paths
            .iter()
            .map(|p| {
                std::fs::canonicalize(p)
                    .map(|c| c.to_string_lossy().to_string())
                    .unwrap_or_else(|_| p.clone())
            })
            .collect();

        options.algorithm = self.algorithm.parse()?;
        options.filter.min_size = self.min_size;
        options.filter.max_size = self.max_size;
        options.filter.include_extensions = self.include_ext.clone();
        options.filter.exclude_extensions = self.exclude_ext.clone();
        options.filter.include_hidden = self.include_hidden;
        options.filter.follow_symlinks = self.follow_symlinks;
        options.filter.modified_after = None;
        options.filter.modified_before = None;
        options.filter.min_depth = 0;
        if !self.exclude_glob.is_empty() {
            options.filter.exclude_globs.extend(self.exclude_glob.clone());
        }

        options.use_cache = !self.no_cache;
        options.protect_system_dirs = !self.no_protect_system;
        options.threads = self.threads;
        options.min_group_size = self.min_group.max(2);

        options.detect_perceptual = self.perceptual;
        options.perceptual_threshold = self.perceptual_threshold;
        options.audio_fingerprint = self.audio;
        options.audio_threshold = self.audio_threshold;
        options.scan_archives = self.archives;
        options.check_references = self.references;
        options.raw_jpeg_pairing = self.raw_jpeg_pairing;

        options.validate()?;
        Ok(options)
    }

    fn to_mark_config(&self) -> AutoMarkConfig {
        let mut rules = Vec::new();
        for r in &self.rule {
            let rule = match r.as_str() {
                "newest" => MarkRule::KeepNewest,
                "oldest" => MarkRule::KeepOldest,
                "oldest-created" => MarkRule::KeepOldestCreated,
                "shortest" => MarkRule::KeepShortestPath,
                "longest-name" => MarkRule::KeepLongestName,
                "shallowest" => MarkRule::KeepShallowest,
                "largest" => MarkRule::KeepLargest,
                "most-linked" => MarkRule::KeepMostLinked,
                "in-photos" => MarkRule::KeepInPath {
                    pattern: "原片".into(),
                },
                "prefer-raw" => MarkRule::PreferExtension {
                    extensions: vec![
                        "cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "pef", "srw",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                },
                _ => continue,
            };
            rules.push(MarkRuleEntry {
                rule,
                enabled: true,
            });
        }

        if rules.is_empty() {
            AutoMarkConfig::default()
        } else {
            // 用户显式给出的规则优先，默认链作为兜底追加在后面。
            let mut config = AutoMarkConfig::default();
            rules.extend(config.rules.drain(..));
            config.rules = rules;
            config
        }
    }

    fn delete_mode(&self) -> Option<DeleteMode> {
        if self.trash {
            Some(DeleteMode::Trash)
        } else if self.hardlink {
            Some(DeleteMode::HardlinkReplace)
        } else if self.move_to.is_some() {
            Some(DeleteMode::MoveTo)
        } else {
            None
        }
    }
}

/// 判断本次进程是否应当走 CLI 分支。
pub fn is_cli_invocation() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else {
        return false;
    };
    if first.starts_with("--") || first.starts_with('-') {
        // `--help` / `--version` 交给 CLI 处理；其余短横线参数视为 GUI 启动参数。
        return matches!(first.as_str(), "--help" | "-h" | "--version" | "-V");
    }
    matches!(first.as_str(), "scan" | "rules" | "sessions" | "help")
}

/// CLI 入口。返回进程退出码。
pub fn run(config: &AppConfig) -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // clap 的 help / version 走 stdout 且退出码为 0。
            let _ = e.print();
            return if e.use_stderr() { EXIT_ERROR } else { EXIT_OK };
        }
    };

    match cli.command {
        None => {
            eprintln!("未指定子命令。可用：scan / rules / sessions（或不带参数启动图形界面）");
            EXIT_ERROR
        }
        Some(Command::Rules) => {
            for entry in AutoMarkConfig::default().rules {
                println!("{}", entry.rule.label());
            }
            EXIT_OK
        }
        Some(Command::Sessions) => match list_sessions(config) {
            Ok(text) => {
                print!("{text}");
                EXIT_OK
            }
            Err(e) => {
                eprintln!("读取会话失败：{e}");
                EXIT_ERROR
            }
        },
        Some(Command::Scan(args)) => match run_scan(config, &args) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("错误：{e}");
                EXIT_ERROR
            }
        },
    }
}

fn list_sessions(config: &AppConfig) -> AppResult<String> {
    let db = Db::open(&config.db_path)?;
    let sessions = db.read(crate::db::session_repo::list)?;
    if sessions.is_empty() {
        return Ok("（暂无保存的会话）\n".to_string());
    }
    let mut out = String::new();
    for s in sessions {
        out.push_str(&format!(
            "{}  {:>6} 组  {:>10}  {}\n",
            s.id,
            s.group_count,
            pipeline::human_bytes(s.wasted_bytes),
            s.name
        ));
    }
    Ok(out)
}

fn run_scan(config: &AppConfig, args: &ScanArgs) -> AppResult<i32> {
    let options = args.to_options()?;
    let mark_config = args.to_mark_config();

    let db = Db::open(&config.db_path)?;
    let cancel = AtomicBool::new(false);
    let scan_id = uuid::Uuid::new_v4().to_string();

    let sink = |p: ScanProgress| {
        if args.quiet || args.json {
            return;
        }
        eprint!("\r\x1b[K{}  {:.0}%  {}", p.phase_label, p.percent * 100.0, p.message);
    };

    let outcome = pipeline::run_scan(
        &scan_id,
        &options,
        &mark_config,
        &db,
        config,
        &cancel,
        &sink,
    )?;

    let result = outcome.result;

    if !args.quiet && !args.json {
        eprintln!("\r\x1b[K扫描完成。");
        eprintln!(
            "  文件：发现 {} 个，参与哈希 {} 个（缓存命中 {}）",
            result.stats.files_seen, result.stats.files_scanned, result.stats.cache_hits
        );
        eprintln!(
            "  结果：{} 个分组，{} 个待清理，可回收 {}",
            result.stats.groups,
            result.stats.duplicate_files,
            pipeline::human_bytes(result.stats.wasted_bytes)
        );
        if result.stats.protected_dirs_skipped > 0 {
            eprintln!(
                "  保护：跳过 {} 个系统关键目录",
                result.stats.protected_dirs_skipped
            );
        }
        if !result.groups.is_empty() {
            eprintln!("\n  占用空间最大的分组：");
            for group in result.groups.iter().take(10) {
                eprintln!(
                    "    {:<28} {} 项 · {} · 可回收 {}",
                    truncate(&group.title, 28),
                    group.files.len(),
                    group.kind_label,
                    pipeline::human_bytes(group.wasted_bytes)
                );
            }
        }
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }

    if let Some(fmt) = &args.format {
        let format: ExportFormat = fmt.parse()?;
        let path = args
            .output
            .clone()
            .unwrap_or_else(|| export::default_path(&result, format, &config.export_dir));
        let summary = export::export(&result, format, &path)?;
        if !args.quiet {
            eprintln!("  报告：{}（{} 行）", summary.path, summary.rows);
        }
    } else if let Some(out) = &args.output {
        let summary = export::export(&result, ExportFormat::Html, out)?;
        if !args.quiet {
            eprintln!("  报告：{}", summary.path);
        }
    }

    // ---------------- 可选：执行清理 ----------------
    if let Some(mode) = args.delete_mode() {
        let targets = collect_targets(&result);
        if targets.is_empty() {
            if !args.quiet {
                eprintln!("  没有需要清理的文件。");
            }
            return Ok(if result.stats.groups > 0 { EXIT_FOUND } else { EXIT_OK });
        }

        let protected = ProtectedPaths::new(config.protect_system_dirs);
        let ctx = ActionContext {
            config,
            allowed_roots: &options.roots,
            protected: &protected,
        };

        let plan = ActionPlan {
            mode,
            move_dir: args
                .move_to
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            dry_run: args.dry_run,
            targets,
            confirmed: args.yes,
        };

        let outcome = actions::execute(&plan, &ctx)?;

        if !args.quiet {
            eprintln!(
                "  {}：成功 {} 个，失败 {} 个，释放 {}",
                if args.dry_run { "演练" } else { mode.label() },
                outcome.succeeded,
                outcome.failed,
                pipeline::human_bytes(outcome.freed_bytes)
            );
            for item in outcome.outcomes.iter().filter(|o| !o.success).take(10) {
                eprintln!(
                    "    ✗ {} — {}",
                    truncate(&item.path, 60),
                    item.error.as_deref().unwrap_or("未知原因")
                );
            }
        }
    }

    Ok(if result.stats.groups > 0 {
        EXIT_FOUND
    } else {
        EXIT_OK
    })
}

fn collect_targets(result: &ScanResult) -> Vec<ActionTarget> {
    result
        .groups
        .iter()
        .flat_map(|group| {
            let keep = group
                .files
                .iter()
                .find(|f| f.decision == MarkDecision::Keep)
                .map(|f| f.entry.path.clone());

            group.files.iter().filter_map(move |f| {
                if f.decision != MarkDecision::Remove || !f.is_deletable() {
                    return None;
                }
                Some(ActionTarget {
                    path: f.entry.path.clone(),
                    keep_path: keep.clone(),
                    group_id: group.id.clone(),
                    size: f.entry.size,
                    archive_inner_path: f.archive_inner_path.clone(),
                })
            })
        })
        .collect()
}

fn truncate(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}
