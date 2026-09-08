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
// L-1/V-1：VM 内 agent（仅引导器编排的 VM 档启用）
#[cfg(feature = "vm-agent")]
pub mod vm_agent;

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
            // 批次C-5：L4 智能让位 —— 独占全屏前台监测（让位/恢复）+ 反作弊进程
            // 看护（kbdhook 主动停用 + 前端横幅；进程与数据通道全保留）。
            shell::winman::spawn_fullscreen_watcher(app.handle().clone());
            shell::winman::spawn_anticheat_watcher(app.handle().clone());
            // 批次W-5：显示器热切换看护（分屏记忆 + 出屏窗口吸附回主屏）
            shell::winman::spawn_display_watcher(app.handle().clone());
            // D-3：全域软件接管看门狗（逃逸窗口探测；ask/auto/off 策略，
            // 维护模式暂停；白名单先于逻辑执行，默认「询问」不自动回收）
            shell::shell_watch::spawn_watchdog(
                app.handle().clone(),
                &app.state::<AppState>().inner(),
            );
            // S-1 防截屏看护线程：开启期间周期补打新窗口（含嵌入窗口）
            shell::privacy_shield::spawn_watcher(app.handle().clone());
            // F-6：计划备份定时器（daily/weekly；启动时补跑错过的任务）
            shell::sysmaint::start_scheduler(app.handle().clone());
            // AI-09 M-27：目录监控哨兵（从配置恢复哨兵线程，含静音时段）
            shell::fileops::startup_init(&app.state::<AppState>(), app.handle());
            // AI-07 N-15：剪贴板历史看护（序列号轮询 + DPAPI 落盘 + 敏感名单）
            {
                let st = app.state::<AppState>();
                shell::cliphist::spawn_cliphist_watcher(app.handle().clone(), st.data_dir.clone());
            }
            // AI-07 N-18：宏引擎运行时（Ctrl+Esc 急停监测 + cron 调度）
            shell::macros::spawn_macro_runtime(app.handle().clone());
            // 兼容层：Wallpaper Engine 冲突检测与自动缓解（libcef 0x80000003 根因）
            shell::compat::apply_if_needed_at_startup(app.handle());
            shell::compat::spawn_compat_watcher(app.handle().clone());
            // AI-13 U-20 内存守护（5s 采样 + 泄漏看门狗）与 M-53 崩溃转储钩子
            shell::perf::spawn_mem_warden();
            shell::perf::install_crash_hook(app.state::<AppState>().data_dir.join("crashes"));
            // AI-12 M-45：输入设备热插拔监听（只观察，重注册动作由前端执行）
            shell::compat::spawn_hotplug_watcher(app.handle().clone());
            // AI-14 N-28/Z-53：本地网关自动拉起（配置为开时；默认关闭零监听）
            shell::openhub::gateway_autostart(&app.state::<AppState>());
            // AI-15 V-83/V-86：计划任务工坊 + 启动延迟编排运行时（30s 轮询触发器）
            shell::workshop::spawn_workshop_runtime(app.handle().clone());
            shell::workshop::spawn_startdelay_runtime(app.handle().clone());
            // AI-16 Z-49：提醒中心运行时（系统时钟锚定 1s 粒度；重启补发未触发提醒）
            // L-1：VM 档 agent 心跳（宿主引导器探测 47631；退出回发 EXIT 通知宿主卸盘）
            #[cfg(feature = "vm-agent")]
            vm_agent::spawn();
            // D-1：VM 档 Shell 模式（Winlogon Shell=Variable.exe）——拉起隐藏 explorer 服务进程兜底
            #[cfg(windows)]
            if shell::shellmode::is_shell_mode() {
                shell::shellmode::ensure_explorer_service();
            }
            // D-2：直跑档 Shell 崩溃自检（连续 3 次 60s 内启动 → 自动回退 explorer）
            #[cfg(windows)]
            if let Some(true) = shell::directshell::boot_selfcheck() {
                log_line(&app.state::<AppState>(), "D-2 selfcheck: shell crash >3, reverted to explorer");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Exit forensics: distinguish "someone requested close" from
            // "the window died on its own" (webview crash / system).
            use tauri::Manager;
            let st = window.app_handle().state::<AppState>();
            match event {
                tauri::WindowEvent::CloseRequested { .. } => log_line(&st, "window close REQUESTED"),
                tauri::WindowEvent::Destroyed => {
                    log_line(&st, "window DESTROYED");
                    // X-1 扩展崩溃隔离：宿主 webview 死亡只标记扩展卡，主进程无感
                    shell::extensions::mark_crashed(window.label());
                }
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
            // ---- AI-16 启动与声音通知组（Z-43…Z-49） ----
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
            shell::mousefeel::mouse_params_get,
            shell::mousefeel::mouse_params_write,
            shell::mousefeel::mouse_params_rollback,
            shell::mousefeel::pointer_speed_temp,
            shell::mousefeel::pointer_speed_restore,
            shell::tools::tool_data_read,
            shell::tools::tool_data_write,
            shell::tools::tool_secure_read,
            shell::tools::tool_secure_write,
            shell::tools::snapshot_capture,
            // ---- AI-08 基础工具组（Z-22…Z-28 支撑 + V-97/98 打印双件）----
            shell::tools::sys_self_info,
            shell::tools::cursor_pos,
            shell::tools::http_fetch,
            shell::print::print_assoc_check,
            shell::print::print_files,
            shell::print::print_list,
            shell::print::print_jobs,
            shell::print::print_job_set,
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
            shell::sysenv::sysenv_overview,
            shell::sysenv::sysenv_display_set,
            shell::taskman::proc_list,
            shell::taskman::proc_kill,
            shell::taskman::perf_cpu,
            shell::taskman::startup_list,
            shell::taskman::startup_disable,
            shell::taskman::service_list,
            shell::taskman::service_set,
            shell::fsindex::fsindex_status,
            shell::fsindex::fsindex_query,
            shell::sysmaint::backup_schedule_get,
            shell::sysmaint::backup_schedule_set,
            shell::sysmaint::backup_run_now,
            shell::sysmaint::update_scan,
            shell::sysmaint::update_apply,
            shell::sysmaint::maintain_selfcheck,
            shell::audioime::mixer_list,
            shell::audioime::mixer_set,
            shell::audioime::ime_status,
            shell::audioime::ime_list,
            shell::audioime::ime_switch,
            shell::audioime::media_status,
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
            // ---- AI-09 文件操作组（M-21/Z-29..Z-35/M-19..M-27）----
            shell::fileops::checksum,
            shell::fileops::checksum_cancel,
            shell::fileops::dupe_scan,
            shell::fileops::space_scan,
            shell::fileops::batch_rename_preview,
            shell::fileops::batch_rename_apply,
            shell::fileops::batch_rename_undo,
            shell::fileops::sendto_list,
            shell::fileops::sendto_custom_add,
            shell::fileops::sendto_custom_remove,
            shell::fileops::sendto_copy,
            shell::fileops::net_drives,
            shell::fileops::who_locks,
            shell::fileops::archive_ls,
            shell::fileops::archive_extract_one,
            shell::fileops::sentinel_list,
            shell::fileops::sentinel_add,
            shell::fileops::sentinel_remove,
            shell::fileops::sentinel_toggle,
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
            shell::envs::env_diff,
            shell::envs::env_discard,
            shell::envs::env_merge,
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
            shell::shell_watch::watch_get_settings,
            shell::shell_watch::watch_set_settings,
            shell::shell_watch::watch_dismiss,
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
            shell::launcher::tp_set_dpi_fix,
            shell::launcher::tp_rename,
            shell::launcher::tp_launch,
            shell::launcher::tp_set_icon,
            shell::launcher::tp_scan_start_menu,
            shell::launcher::tp_portableize,
            shell::launcher::tp_launch_admin,
            shell::launcher::icon_dataurl,
            shell::launcher::icon_jumbo_dataurl,
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
            shell::embed::embed_adopt,
            shell::embed::embed_pick_window,
            shell::compat_probe::compat_set_override,
            shell::embed::embed_bounds,
            shell::embed::embed_visible,
            shell::embed::embed_close,
            shell::embed::embed_close_all,
            shell::embed::embed_focus,
            shell::embed::embed_input,
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
            shell::privacy_shield::shield_set,
            shell::privacy_shield::shield_get,
            shell::extensions::ext_list,
            shell::extensions::ext_rescan,
            shell::extensions::ext_set_enabled,
            shell::extensions::ext_open_web,
            shell::extensions::ext_close,
            shell::extensions::ext_invoke,
            shell::extensions::ext_audit,
            shell::extensions::ext_install_example,
            shell::extensions::ext_market_list,
            shell::extensions::ext_market_import,
            shell::extensions::ext_market_install,
            shell::extensions::ext_market_remove,
            shell::ext_plugin::ext_plugin_load,
            shell::ext_plugin::ext_plugin_unload,
            shell::ext_plugin::ext_daemon_start,
            shell::ext_plugin::ext_daemon_status,
            shell::ext_plugin::ext_daemon_example,
            shell::netconsent::net_consent_check,
            shell::netconsent::net_consent_set,
            // ---- AI-10 文件管理与数据安全组（U-16/U-25…U-36/N-31/V-31）----
            // U-25 版本时光机
            shell::versions::ver_watch,
            shell::versions::ver_snapshot,
            shell::versions::ver_list,
            shell::versions::ver_read,
            shell::versions::ver_diff,
            shell::versions::ver_restore,
            shell::versions::ver_gc,
            shell::versions::ver_policy_set,
            shell::versions::ver_watched_list,
            // U-26 全局文件标签
            shell::tags::tag_set,
            shell::tags::tag_star,
            shell::tags::tag_get,
            shell::tags::tag_map,
            shell::tags::tag_all,
            shell::tags::tag_move,
            shell::tags::tag_filter,
            shell::tags::tag_smart_list,
            shell::tags::tag_smart_add,
            shell::tags::tag_smart_remove,
            // U-27 回收站 2.0 策略引擎
            shell::recycle::rec_policy_get,
            shell::recycle::rec_policy_set,
            shell::recycle::rec_policy_preview,
            shell::recycle::rec_policy_apply,
            // U-28 传输指挥台
            shell::transfer::tr_enqueue,
            shell::transfer::tr_list,
            shell::transfer::tr_pause,
            shell::transfer::tr_resume,
            shell::transfer::tr_cancel,
            shell::transfer::tr_retry,
            shell::transfer::tr_clear_done,
            // U-29 存档柜
            shell::archive::arch_create,
            shell::archive::arch_list,
            shell::archive::arch_browse,
            shell::archive::arch_read,
            shell::archive::arch_audit,
            shell::archive::arch_extract,
            shell::archive::arch_repair,
            shell::archive::arch_remove,
            // U-30 数据血缘
            shell::lineage::lin_record,
            shell::lineage::lin_list,
            shell::lineage::lin_stats,
            shell::lineage::lin_export,
            shell::lineage::lin_burn,
            // U-31 隐私仪表盘（审计时间线）
            shell::privacy::priv_log,
            shell::privacy::priv_timeline,
            shell::privacy::priv_audit_pause,
            shell::privacy::priv_audit_enabled,
            // U-32 应用防火墙 2.0
            shell::netconsent::fw_profile_get,
            shell::netconsent::fw_profile_set,
            shell::netconsent::fw_check,
            shell::netconsent::fw_traffic,
            shell::netconsent::fw_alerts,
            shell::netconsent::fw_alert_resolve,
            shell::netconsent::fw_profiles,
            // U-33 诱饵文件系统
            shell::privacy::canary_plant,
            shell::privacy::canary_list,
            shell::privacy::canary_touch,
            shell::privacy::canary_whitelist,
            shell::privacy::canary_remove,
            // U-34 紧急擦拭
            shell::panic::panic_config_get,
            shell::panic::panic_config_set,
            shell::panic::panic_trigger,
            shell::panic::panic_drill,
            // U-35 信任链中心
            shell::trust::trust_register,
            shell::trust::trust_verify,
            shell::trust::trust_wall,
            shell::trust::trust_reverify_all,
            shell::trust::trust_remove,
            // U-36 隐身会话
            shell::incognito::inc_start,
            shell::incognito::inc_status,
            shell::incognito::inc_write,
            shell::incognito::inc_list,
            shell::incognito::inc_end,
            // N-31 本地使用洞察
            shell::insights::ins_record,
            shell::insights::ins_dashboard,
            shell::insights::ins_suggestions,
            shell::insights::ins_gc,
            shell::insights::ins_burn,
            // U-16 + V-31 explorer 扩展
            shell::explorer::ex_list_paged,
            shell::explorer::ex_view_get,
            shell::explorer::ex_view_set,
            shell::explorer::ex_column_chain,
            // AI-10 数据安全中心独立窗口
            shell::tray::open_datavault,
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
            exec::residue_resolve,
            exec::residue_whitelist_add,
            exec::residue_whitelist_list,
            exec::exit_prepare,
            shell::installer::install_mode_launch,
            shell::installer::install_list,
            shell::installer::install_analyze,
            shell::installer::install_commit,
            shell::installer::install_discard,
            shell::installer::profile_infer,
            shell::winman::win_set_avoid_taskbar,
            shell::winman::win_health_scan,
            shell::winman::win_suspend,
            shell::winman::win_resume,
    shell::winman::win_hide_to_tray,
    shell::winman::power_action,
    shell::winman::shortcuts_apply,
            shell::compat::compat_check,
            // ---- AI-12 兼容纵深组（Z-15…Z-21、M-37…M-45 支撑）----
            shell::compat::compat_uwp_list,
            shell::compat::compat_elevation_probe,
            shell::compat::compat_driver_scan,
            shell::compat::compat_host_probe,
            shell::compat::compat_shim_report,
            shell::compat::compat_shim_stats,
            shell::compat::compat_icon_probe,
            shell::compat::compat_volumes,
            shell::compat::compat_heal_paths,
            shell::compat::compat_apply,
            shell::compat::compat_restore,
            shell::compat::shell_execute,
            shell::compat::shell_activate_application,
            shell::compat::shell_item_icon,
            shell::compat::shell_context_menu,
            shell::compat::shell_forward_gesture,
    shell::sysinfo::sys_brief,
    shell::sysinfo::sys_disk_health,
    shell::sysinfo::sys_disks,
    shell::sysinfo::sys_user,
    shell::sysinfo::net_ip,
    shell::directshell::directshell_status,
    shell::directshell::directshell_set,
            mindmap::nodes_versions,
            shell::xflow::drag_track,
            // ---- AI-07 效率中枢：N-15 剪贴板历史 / N-18 宏引擎护栏 ----
            shell::cliphist::cliphist_list,
            shell::cliphist::cliphist_pin,
            shell::cliphist::cliphist_remove,
            shell::cliphist::cliphist_clear,
            shell::cliphist::cliphist_burn,
            shell::cliphist::cliphist_config_get,
            shell::cliphist::cliphist_config_set,
            shell::cliphist::cliphist_write_back,
            shell::macros::macro_emergency_stop,
            shell::macros::macro_emergency_clear,
            shell::macros::macro_is_stopped,
            shell::macros::macro_uac_foreground,
            shell::macros::macro_upsert_trigger,
            shell::macros::macro_remove_trigger,
            shell::macros::macro_list_triggers,
            shell::macros::macro_send_text,
            // ---- AI-11 系统集成与硬件组（U-43..U-48 / N-19..N-25 / V-51..V-60）----
            shell::sysprobe::monitor_list,
            shell::sysprobe::port_table,
            shell::sysprobe::eventlog_recent,
            shell::sysprobe::bigfile_scan,
            shell::sysprobe::sys_uptime,
            shell::sysprobe::startup_procs,
            shell::sysprobe::selfheal_checks,
            shell::sysprobe::heal_run,
            shell::sysprobe::pwrloss_check,
            shell::sysprobe::predwarm,
            shell::sysprobe::periph_probe,
            shell::winpower::keepawake_set,
            shell::winpower::keepawake_get,
            shell::winpower::power_schemes_list,
            shell::winpower::power_scheme_set,
            shell::winpower::battery_health,
            shell::winpower::gamma_set,
            shell::winpower::gamma_restore,
            shell::winpower::proc_priority_set,
            shell::winpower::proxy_get,
            shell::winpower::proxy_set,
            shell::winpower::net_ping,
            // ---- AI-14 开放接口组（U-37/38/39、Z-51/52/55、N-28/30）----
            shell::openhub::openhub_config_get,
            shell::openhub::openhub_config_set,
            shell::openhub::deeplink_parse,
            shell::openhub::vxs_validate_cmd,
            shell::openhub::vxs_extract,
            shell::openhub::openhub_data_export,
            shell::openhub::openhub_stream_emit,
            shell::openhub::openhub_stream_tail,
            shell::openhub::safehouse_check,
            shell::openhub::safehouse_exec,
            shell::openhub::gateway_status,
            shell::openhub::gateway_token_regen,
            shell::openhub::openhub_connector_query,
            shell::openhub::companion_inbox,
            shell::openhub::companion_inbox_clear,
            shell::openhub::deeplink_register,
            shell::openhub::deeplink_unregister,
            // ---- AI-13 性能与长跑组（U-19/U-20/U-22、M-46…M-48/M-53/M-54、N-35/N-36）----
            shell::perf::perf_mem_snapshot,
            shell::perf::perf_mem_warden_status,
            shell::perf::perf_io_copy,
            shell::perf::perf_io_pause,
            shell::perf::perf_io_resume,
            shell::perf::perf_io_cancel,
            shell::perf::perf_io_progress,
            shell::perf::perf_log_usage,
            shell::perf::perf_log_rotate,
            shell::perf::perf_settings_preflight,
            shell::perf::perf_db_compact,
            shell::perf::perf_instance_list,
            shell::perf::perf_instance_create,
            shell::perf::perf_instance_delete,
            shell::perf::perf_instance_heartbeat,
            shell::perf::perf_relay_export,
            shell::perf::perf_relay_import,
            shell::perf::perf_cpu_quota_set,
            shell::perf::perf_crash_dumps,
            shell::perf::perf_boot_stage,
            shell::perf::perf_boot_stages,
            // ---- AI-15 开放工具组（M-57/59/63、V-81..V-90）----
            shell::opentools::webhook_rules_get,
            shell::opentools::webhook_rules_set,
            shell::opentools::webhook_dispatch,
            shell::opentools::webhook_test,
            shell::opentools::webhook_log_list,
            shell::opentools::embed_manifest_scan,
            shell::opentools::vxs_scan_cmd,
            shell::opentools::cfg_diff,
            shell::opentools::sandbox_trial_begin,
            shell::opentools::sandbox_trial_end,
            shell::opentools::sandbox_trial_list,
            shell::winget::winget_status,
            shell::winget::winget_search,
            shell::winget::winget_list_installed,
            shell::winget::winget_upgrade_list,
            shell::winget::winget_install,
            shell::winget::winget_upgrade_one,
            shell::winget::winget_uninstall,
            shell::envedit::env_overview,
            shell::envedit::env_backup_list,
            shell::envedit::env_var_set,
            shell::envedit::env_var_delete,
            shell::envedit::env_restore_backup,
            shell::workshop::sched_list,
            shell::workshop::sched_upsert,
            shell::workshop::sched_remove,
            shell::workshop::sched_toggle,
            shell::workshop::sched_log_list,
            shell::workshop::sched_run_now,
            shell::workshop::workshop_idle_report,
            shell::workshop::startdelay_get,
            shell::workshop::startdelay_set,
            shell::workshop::startdelay_timeline,
            shell::assocguard::assoc_snapshot_take,
            shell::assocguard::assoc_snapshot_list,
            shell::assocguard::assoc_snapshot_remove,
            shell::assocguard::assoc_snapshot_diff,
            shell::assocguard::assoc_snapshot_restore,
            shell::assocguard::residue_scan_app,
            shell::assocguard::residue_delete,
            shell::svcgraph::svc_graph,
            shell::svcgraph::svc_impact,
            shell::svcgraph::svc_topo,
            // ---- AI-19 无障碍与本地化组（M-73/M-74；模块文件由 AI-19 交付时接线）----
        ])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => {
            app.run(|_app, event| {
                // L-1：VM 档引擎退出 → 通知宿主引导器安全卸盘
                #[cfg(feature = "vm-agent")]
                if let tauri::RunEvent::Exit = event {
                    vm_agent::notify_host_exit();
                }
                // D-1：Shell 模式下回收 explorer 服务进程（零残留）
                #[cfg(windows)]
                if let tauri::RunEvent::Exit = event {
                    shell::shellmode::cleanup_explorer_service();
                }
                // AI-11 红线：退出还原宿主状态（gamma 字节级还原 + 解除保持唤醒）
                // + 写干净关机标记（V-59 断电自检判定基准）
                #[cfg(windows)]
                if let tauri::RunEvent::Exit = event {
                    shell::winpower::restore_on_exit();
                    let st = _app.state::<AppState>();
                    let _ = std::fs::write(
                        st.data_dir.join(shell::sysprobe::CLEAN_SHUTDOWN_FILE),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis().to_string())
                            .unwrap_or_default(),
                    );
                }
                #[cfg(not(all(feature = "vm-agent", windows)))]
                let _ = &event;
            });
        }
        Err(e) => eprintln!("Variable failed to start: {e}"),
    }
}

pub fn log_line(st: &AppState, msg: &str) {
    state::append_log(&st.logs_dir, msg);
}
