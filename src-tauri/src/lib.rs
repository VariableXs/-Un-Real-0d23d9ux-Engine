pub mod backup;
pub mod boot;
pub mod db;
pub mod error;
pub mod exec;
pub mod export;
pub mod library;
pub mod media;
pub mod mindmap;
pub mod models;
pub mod project_scan;
pub mod settings_cmd;
pub mod cli;
pub mod shell;
pub mod state;
pub mod system;
pub mod workspace;

use state::AppState;
use tauri::Manager;

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() && args[0].starts_with("--") {
        if let Some(code) = crate::cli::run_cli(&args) {
            std::process::exit(code);
        }
    }
    // 单实例守卫：双开时第二实例的全局快捷键整表注册必然失败
    // （RegisterHotKey 是系统级，第一实例已占用 ctrl+alt+*，表现为
    // "注册失败（被系统或其他软件占用）"全量弹窗 —— 实机反馈）。
    shell::single_instance::enforce();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    // 批次E（规格 4.7）：accel → SHORTCUT_MAP 查表 → dispatch_action。
                    // 快捷键表由 init_shortcuts（默认）/ shortcuts_apply（用户自定义）统一维护。
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let accel = shortcut.into_string();
                        if let Some(action) = shell::winman::shortcut_action(&accel) {
                            shell::winman::dispatch_action_pub(app, &action);
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            let st = AppState::bootstrap()?;
            log_line(&st, "app bootstrap dirs ok");
            // 批次B-6（M1）：残留扫描基线——环境启动即对宿主观测面快照（会话差集用）
            exec::residue_baseline_take();
            app.manage(st);
            // Real loading happens here and is streamed to the UI as
            // `boot://event` progress events (no synthetic timeline).
            boot::spawn_boot_loader(app.handle().clone());
            // M8 拔出保护：仅便携模式生效（内部自行判断），1s 轮询数据卷 + 周期 WAL checkpoint
            shell::usb::spawn_removal_watcher(app.handle().clone());
            // 批次E-8：通讯软件未读提醒（微信/QQ/钉钉/飞书…，仅窗口标题，不读消息内容）
            shell::imwatch::spawn_im_watcher(app.handle().clone());
            // OS 系统托盘（M5）：图标 + 菜单，失败不阻断启动。
            if let Err(e) = shell::tray::init(app.handle()) {
                eprintln!("tray init failed: {e}");
            }
            // M6/批次E：全局快捷键统一走 winman::init_shortcuts（默认表）。
            // 此前这里是第二份硬编码表（super+e / ctrl+alt+o / super+n），与默认表
            // 不一致且含 Windows 保留键，导致启动日志一直报 register failed。
            shell::winman::init_shortcuts(app.handle());
            // 批次E-18：双击 Esc 切环境/Windows；Del+Backspace 真正退出
            shell::kbdhook::spawn_env_monitor(app.handle().clone());
            // 兼容层：Wallpaper Engine 冲突检测与自动缓解（libcef 0x80000003 根因）
            shell::compat::apply_if_needed_at_startup(app.handle());
            shell::compat::spawn_compat_watcher(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Exit forensics: distinguish "someone requested close" from
            // "the window died on its own" (webview crash / system).
            use tauri::Manager;
            let st = window.app_handle().state::<AppState>();
            match event {
                tauri::WindowEvent::CloseRequested { .. } => log_line(&st, "window close REQUESTED"),
                tauri::WindowEvent::Destroyed => log_line(&st, "window DESTROYED"),
                // 批次0（规格 10.1）：桌面窗口获得焦点 → 自动恢复置顶覆盖。
                // 启动第三方软件时会暂时撤销置顶让其浮于桌面之上，回到桌面即恢复。
                // 兼容态（Wallpaper Engine 运行中）不动置顶，避免与 WorkerW 抢合成器
                // 导致 libcef 0x80000003 与 DWM 卡死。
                tauri::WindowEvent::Focused(true) if window.label() == "desktop" => {
                    if !shell::compat::is_compat_active() {
                        let _ = window.set_always_on_top(true);
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            boot::boot_replay,
            system::app_bootstrap,
            system::open_path,
            system::reveal_path,
            system::check_paths_exist,
            system::log_frontend,
            system::save_text_file,
            workspace::ws_default_dir,
            workspace::ws_list,
            workspace::ws_read_text,
            workspace::ws_create_dir,
            workspace::ws_rename,
            workspace::ws_move,
            workspace::ws_copy_in,
            workspace::ws_delete_trash,
            project_scan::project_scan,
            project_scan::project_read_file,
            project_scan::project_read_bytes,
            project_scan::read_text_file,
    project_scan::write_text_file,
            library::list_folders,
            library::create_folder,
            library::rename_folder,
            library::move_folder,
            library::trash_folder,
            library::restore_folder,
            library::purge_folder,
            library::list_documents,
            library::create_document,
            library::get_document,
            library::save_document,
            library::move_document,
            library::set_document_favorite,
            library::set_document_tags,
            library::list_document_tags,
            library::trash_document,
            library::restore_document,
            library::purge_documents,
            library::empty_trash,
            library::search_all,
            mindmap::list_mindmaps,
            mindmap::create_mindmap,
            mindmap::get_mindmap,
            mindmap::update_mindmap,
            mindmap::rename_mindmap,
            mindmap::trash_mindmap,
            mindmap::save_nodes,
            mindmap::delete_nodes,
            mindmap::save_edge,
            mindmap::delete_edges,
            media::import_media,
            media::import_data_url,
            media::attach_media,
            media::list_attachments,
            media::resolve_media_path,
            media::delete_media,
            settings_cmd::get_all_settings,
            settings_cmd::set_settings,
            settings_cmd::reset_ui_settings,
            settings_cmd::write_recovery_file,
            settings_cmd::list_recovery_files,
            settings_cmd::read_recovery_file,
            settings_cmd::delete_recovery_file,
            settings_cmd::recover_to_document,
            backup::create_backup,
            backup::list_backups,
            backup::restore_backup,
            backup::delete_backup,
            backup::export_backup,
            export::export_documents,
            export::export_mindmap_json,
            export::export_workspace,
            export::import_workspace,
            shell::hardware::privacy_usage,
            shell::hardware::audio_get,
            shell::hardware::audio_set,
            shell::hardware::audio_devices,
            shell::hardware::audio_set_default,
            shell::hardware::wifi_get,
            shell::hardware::wifi_scan,
            shell::hardware::wifi_disconnect,
            shell::hardware::wifi_set,
            shell::hardware::bluetooth_get,
            shell::hardware::bluetooth_set,
            shell::hardware::bt_devices,
            shell::hardware::bt_connect,
            shell::hardware::bt_disconnect,
            shell::hardware::battery_get,
            shell::hardware::brightness_get,
            shell::hardware::brightness_set,
            shell::explorer::ex_home,
            shell::explorer::ex_drives,
            shell::explorer::ex_variable_dirs,
            shell::explorer::ex_list,
            shell::explorer::ex_mkdir,
            shell::explorer::ex_rename,
            shell::explorer::ex_move,
            shell::explorer::ex_copy,
            shell::explorer::ex_trash,
            shell::explorer::ex_search,
            shell::explorer::ex_conflicts,
            shell::explorer::ex_purge,
            shell::explorer::ex_fav_list,
            shell::explorer::ex_fav_add,
            shell::explorer::ex_fav_remove,
            shell::explorer::ex_thumbnail,
            shell::recycle::rec_list,
            shell::recycle::rec_restore,
            shell::recycle::rec_purge,
            shell::recycle::rec_empty,
            shell::recycle::rec_count,
            shell::recovery::container_diag,
            shell::recovery::container_repair,
            shell::recovery::container_rescue_export,
            shell::recovery::container_init,
            shell::recovery::container_stats,
            shell::recovery::vhdx_probe,
            shell::recovery::revocation_list_export,
            shell::recovery::diag_flags,
            shell::browsers::browser_detect,
            shell::browsers::browser_profiles,
            shell::browsers::browser_profile_add,
            shell::browsers::browser_profile_rename,
            shell::browsers::browser_profile_clone,
            shell::browsers::browser_profile_delete,
            shell::browsers::browser_profile_launch,
            shell::browsers::browser_import,
            shell::browsers::browser_running,
            shell::code::code_status,
            shell::code::code_deploy,
            shell::code::code_register,
            shell::code::code_launch,
            shell::toolchains::toolchain_status,
            shell::toolchains::toolchain_deploy,
            shell::git_panel::git_status,
            shell::git_panel::git_log,
            shell::git_panel::git_branches,
            shell::git_panel::ssh_keys,
            shell::git_panel::ssh_key_generate,
            shell::git_panel::ssh_key_delete,
            shell::search::workspace_search,
            shell::search::bigfile_slice,
            shell::search::editor_goto,
            shell::envs::env_list,
            shell::envs::env_create,
            shell::envs::env_switch,
            shell::envs::env_delete,
            shell::envs::env_clone,
            shell::envs::env_nested,
            shell::diagnostic::diagnostic_export,
            shell::diagnostic::demo_capsule,
            shell::ecosystem::portability_assess,
            shell::ecosystem::ecosystem_migrate,
            shell::ecosystem::steam_library_scan,
            shell::ecosystem::steam_launch,
            shell::ecosystem::aumid_launch,
            shell::ecosystem::file_assoc_list,
            shell::ecosystem::file_assoc_set,
            shell::ecosystem::file_assoc_resolve,
            shell::ecosystem::file_assoc_remove,
            shell::network::net_status,
            shell::network::net_proxy_start,
            shell::network::net_proxy_stop,
            shell::network::net_kill_switch,
            shell::network::net_rules_list,
            shell::network::net_rule_grant,
            shell::network::net_rule_revoke,
            shell::security::pe_analyze,
            shell::security::disasm_entry,
            shell::security::sandbox_probe,
            shell::security::sandbox_wsb_generate,
            shell::security::security_report_export,
            shell::security::security_env_preset,
            shell::launcher::tp_add,
            shell::launcher::tp_list,
            shell::launcher::tp_remove,
            shell::launcher::tp_purge,
            shell::launcher::tp_set_grade,
            shell::launcher::tp_rename,
            shell::launcher::tp_launch,
            shell::launcher::tp_set_icon,
            shell::launcher::tp_scan_start_menu,
            shell::launcher::tp_portableize,
            shell::launcher::tp_launch_admin,
            shell::launcher::icon_dataurl,
            shell::appman::tp_running,
            shell::appman::official_usage,
            shell::appman::official_purge,
            shell::usb::usb_status,
            shell::usb::usb_pack,
            shell::usb::usb_verify,
            shell::wallpaper::wp_monitors,
            shell::wallpaper::wp_set_monitor,
            shell::wallpaper::wp_pick_daily,
            shell::wallpaper::wp_engine_scan,
            shell::wallpaper::wp_scene_shader,
            shell::embed::embed_launch,
            shell::embed::embed_bounds,
            shell::embed::embed_visible,
            shell::embed::embed_close,
            shell::embed::embed_focus,
            shell::privacy::vault_status,
            shell::privacy::vault_init,
            shell::privacy::vault_unlock,
            shell::privacy::vault_lock,
            shell::privacy::vault_import,
            shell::privacy::vault_list,
            shell::privacy::vault_export,
            shell::privacy::vault_destroy,
            shell::privacy::privacy_shred,
            shell::privacy::privacy_audit,
            shell::netconsent::net_consent_check,
            shell::netconsent::net_consent_set,
            shell::terminal::term_status,
            shell::terminal::term_open,
            shell::ai::ai_tool_status,
            shell::ai::ai_install_node,
            shell::ai::ai_install_tool,
            shell::ai::identity_list,
            shell::ai::identity_add,
            shell::ai::identity_remove,
            shell::ai::ai_launch,
            shell::ai::ai_verify,
            exec::profile_templates,
            exec::profile_apply,
            exec::profile_set,
            exec::profile_dryrun,
            exec::residue_scan,
            shell::winman::win_set_avoid_taskbar,
    shell::winman::win_hide_to_tray,
    shell::winman::power_action,
    shell::winman::shortcuts_apply,
            shell::compat::compat_check,
            shell::compat::compat_apply,
            shell::compat::compat_restore,
            shell::compat::shell_execute,
            shell::compat::shell_activate_application,
            shell::compat::shell_item_icon,
            shell::compat::shell_context_menu,
            shell::compat::shell_forward_gesture,
    shell::sysinfo::sys_brief,
    shell::sysinfo::sys_disks,
    shell::sysinfo::sys_user,
    shell::sysinfo::net_ip,
            mindmap::nodes_versions,
            shell::xflow::drag_track
        ])
        .run(tauri::generate_context!());
    if let Err(e) = app {
        eprintln!("Variable failed to start: {e}");
    }
}

pub fn log_line(st: &AppState, msg: &str) {
    state::append_log(&st.logs_dir, msg);
}
