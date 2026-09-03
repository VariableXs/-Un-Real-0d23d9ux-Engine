pub mod backup;
pub mod db;
pub mod error;
pub mod export;
pub mod library;
pub mod media;
pub mod mindmap;
pub mod models;
pub mod project_scan;
pub mod settings_cmd;
pub mod state;
pub mod system;
pub mod workspace;

use state::AppState;
use tauri::Manager;

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let st = AppState::bootstrap()?;
            log_line(&st, "app bootstrap ok");
            app.manage(st);
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
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
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
            export::import_workspace
        ])
        .run(tauri::generate_context!());
    if let Err(e) = app {
        eprintln!("Variable failed to start: {e}");
    }
}

pub fn log_line(st: &AppState, msg: &str) {
    state::append_log(&st.logs_dir, msg);
}
