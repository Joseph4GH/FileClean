//! FileClean —— Tauri 2 + Rust 桌面端重复文件扫描与清理工具。
//!
//! 分层结构（自外向内）：
//! ```text
//! commands/     控制器：解析入参 → 调用服务 → 格式化响应，不含业务逻辑
//! pipeline.rs   服务编排：把各阶段串成流水线，不依赖 HTTP / Tauri 类型
//! scan/ hash/ dedupe/ archive/ refs/ autoselect/ actions/ export/   领域能力
//! db/           仓储层：SQLite 连接、迁移、缓存与会话读写
//! config/ error/ logging/ state/                                     基础设施
//! ```

pub mod actions;
pub mod archive;
pub mod autoselect;
pub mod cli;
pub mod commands;
pub mod config;
pub mod db;
pub mod dedupe;
pub mod domain;
pub mod error;
pub mod export;
pub mod hash;
pub mod logging;
pub mod pipeline;
pub mod refs;
pub mod scan;
pub mod state;

use config::AppConfig;
use db::Db;
use state::AppState;

/// 进程统一入口：CLI 参数优先，否则启动图形界面。
pub fn run() {
    if cli::is_cli_invocation() {
        let config = match AppConfig::from_env() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("配置错误：{e}");
                std::process::exit(1);
            }
        };
        logging::init(&config);
        std::process::exit(cli::run(&config));
    }

    run_gui();
}

fn run_gui() {
    // 配置与数据库在 Tauri 之前初始化：任何启动期错误都能被清晰报出，
    // 而不是变成一个「白屏窗口」。
    let config = match AppConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("配置错误：{e}");
            std::process::exit(1);
        }
    };
    logging::init(&config);

    let db = match Db::open(&config.db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("数据库初始化失败：{e}");
            std::process::exit(1);
        }
    };

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        data_dir = %config.data_dir.display(),
        "FileClean 启动"
    );

    let state = AppState::new(config, db);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .setup(|app| {
            use tauri::Manager;
            // 启动时清一次过期缓存，避免库无限增长。
            let state = app.state::<AppState>();
            let ttl = state.config.cache_ttl_secs();
            match state.db.write(|tx| db::cache_repo::purge_expired(tx, ttl)) {
                Ok(removed) if removed > 0 => {
                    tracing::info!(removed, "已清理过期缓存条目");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "清理过期缓存失败"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // ---- 扫描控制 ----
            commands::scan_cmd::start_scan,
            commands::scan_cmd::cancel_scan,
            commands::scan_cmd::get_scan_status,
            commands::scan_cmd::get_scan_result,
            commands::scan_cmd::list_scans,
            commands::scan_cmd::default_options,
            commands::scan_cmd::default_mark_config,
            commands::scan_cmd::system_info,
            commands::scan_cmd::cache_stats,
            commands::scan_cmd::clear_cache,
            // ---- 结果操作 ----
            commands::result_cmd::apply_auto_mark,
            commands::result_cmd::set_file_decision,
            commands::result_cmd::set_group_decision,
            commands::result_cmd::invert_group,
            commands::result_cmd::preview_text,
            commands::result_cmd::preview_thumbnail,
            commands::result_cmd::preview_audio,
            commands::result_cmd::export_report,
            // ---- 动作 ----
            commands::action_cmd::execute_actions,
            commands::action_cmd::undo_action,
            commands::action_cmd::remove_archive_entries,
            commands::action_cmd::reveal_in_finder,
            commands::action_cmd::keep_file,
            commands::action_cmd::preview_plan,
            // ---- 会话 ----
            commands::session_cmd::save_session,
            commands::session_cmd::update_session,
            commands::session_cmd::list_sessions,
            commands::session_cmd::delete_session,
            commands::session_cmd::load_session,
            commands::session_cmd::export_session_file,
            commands::session_cmd::import_session_file,
            commands::session_cmd::session_alive,
            commands::session_cmd::adopt_result,
        ])
        .run(tauri::generate_context!())
        .expect("启动 FileClean 失败");
}
