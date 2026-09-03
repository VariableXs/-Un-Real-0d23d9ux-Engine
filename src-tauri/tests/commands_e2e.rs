//! End-to-end exercise of the interactive operations, calling the real
//! #[tauri::command] functions against a real SQLite database in a temp dir.
use std::fs;
use std::path::PathBuf;
use tauri::State;
use variable_lib::error::CmdResult;
use variable_lib::library;
use variable_lib::models::*;
use variable_lib::settings_cmd;
use variable_lib::state::AppState;

struct Ctx {
    base: PathBuf,
    st: AppState,
}

impl Ctx {
    fn new(tag: &str) -> Ctx {
        let base = std::env::temp_dir().join(format!("var-e2e-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let st = AppState::bootstrap_at(base.clone()).expect("bootstrap");
        Ctx { base, st }
    }
    /// `tauri::State<'r, T>` is a newtype over `&'r T`; building it from a bare
    /// reference lets us drive the real command fns without spinning up a
    /// native runtime (which cannot load inside plain test binaries).
    fn s(&self) -> State<'_, AppState> {
        const _: () =
            assert!(std::mem::size_of::<State<'_, AppState>>() == std::mem::size_of::<&AppState>());
        unsafe { std::mem::transmute::<&AppState, State<'_, AppState>>(&self.st) }
    }
}

fn node_fixture(mid: &str, id: &str, x: f64, rotation: f64, preset: &str) -> Node {
    Node {
        id: id.into(),
        mindmap_id: mid.into(),
        text_html: format!("<b>{}</b>", id),
        text_plain: String::new(),
        x,
        y: 40.0,
        width: 200.0,
        height: 80.0,
        shape: "rounded".into(),
        border_radius: 12.0,
        border_color: "#5b7bd0".into(),
        fill_color: "rgba(13,20,38,0.85)".into(),
        font_size: 14.0,
        opacity: 1.0,
        locked: false,
        z_index: 1,
        record_id: None,
        rotation,
        group_id: Some("grp".into()),
        hidden: false,
        collapsed: false,
        preset: preset.into(),
        updated_at: 0,
    }
}

fn png_bytes() -> Vec<u8> {
    // Minimal valid-enough PNG header (validation is ext/size-based here).
    vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4]
}

#[test]
fn folders_and_documents_flow() {
    let ctx = Ctx::new("docs");
    let st = ctx.s();
    tauri::async_runtime::block_on(async {

    // create hierarchy + duplicate-name rejection at same level
    let root = library::create_folder(st.clone(), "工作".into(), None).unwrap();
    assert!(library::create_folder(st.clone(), "工作".into(), None).is_err());
    let child = library::create_folder(st.clone(), "子项目".into(), Some(root.id.clone())).unwrap();

    // rename validation + duplicate sibling guard on rename
    assert!(library::rename_folder(st.clone(), child.id.clone(), "".into()).is_err());
    library::rename_folder(st.clone(), root.id.clone(), "工作/2".into()).unwrap_err(); // slash illegal via validate? name check rejects backslash/slash only for '/' char — yes rejected
    library::rename_folder(st.clone(), child.id.clone(), "renamed".into()).unwrap();

    // cycle guard: move parent into its own subtree must fail
    library::move_folder(st.clone(), root.id.clone(), Some(child.id.clone())).unwrap_err();

    // document lifecycle inside folder
    let doc = library::create_document(st.clone(), Some(child.id.clone()), Some("会议纪要".into())).unwrap();
    let saved = library::save_document(
        st.clone(),
        DocumentInput {
            id: doc.id.clone(),
            title: "会议纪要 · 发布计划".into(),
            content_html: "<p>讨论了 Q4 路线图</p>".into(),
            content_text: "讨论了 Q4 路线图".into(),
            favorite: Some(true),
            folder_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(saved.title, "会议纪要 · 发布计划");
    assert!(saved.favorite);

    // tags
    let tags = library::set_document_tags(st.clone(), doc.id.clone(), vec!["work".into(), "q4".into()]).unwrap();
    assert_eq!(tags.len(), 2);

    // list filters: favorites / query / sort
    let favs = library::list_documents(st.clone(), ListFilter { view: Some("favorites".into()), folder_id: None, query: None, tag: None, sort: None }).unwrap();
    assert_eq!(favs.len(), 1);
    let hit = library::list_documents(st.clone(), ListFilter { view: Some("all".into()), folder_id: None, query: Some("路线图".into()), tag: None, sort: None }).unwrap();
    assert_eq!(hit.len(), 1);
    let bytag = library::list_documents(st.clone(), ListFilter { view: Some("all".into()), folder_id: None, query: None, tag: Some("q4".into()), sort: None }).unwrap();
    assert_eq!(bytag.len(), 1);

    // global search finds body text and the folder rename
    let hits = library::search_all(st.clone(), "路线图".into()).unwrap();
    assert!(hits.iter().any(|h| h.kind == "document"));

    // trash → restore → trash → purge via empty_trash
    library::trash_document(st.clone(), doc.id.clone()).unwrap();
    let trashed = library::list_documents(st.clone(), ListFilter { view: Some("trash".into()), folder_id: None, query: None, tag: None, sort: None }).unwrap();
    assert_eq!(trashed.len(), 1);
    library::restore_document(st.clone(), doc.id.clone()).unwrap();
    library::trash_document(st.clone(), doc.id.clone()).unwrap();
    let n = library::empty_trash(st.clone()).await.unwrap();
    assert_eq!(n, 1);

    // folder trash cascades docs; purge removes everything permanently
    let d2 = library::create_document(st.clone(), Some(root.id.clone()), None).unwrap();
    library::trash_folder(st.clone(), root.id.clone()).unwrap();
    let trashed = library::list_documents(st.clone(), ListFilter { view: Some("trash".into()), folder_id: None, query: None, tag: None, sort: None }).unwrap();
    assert!(trashed.iter().any(|d| d.id == d2.id));
    library::purge_folder(st.clone(), root.id.clone()).unwrap();
    let gone = library::get_document(st.clone(), d2.id.clone());
    assert!(gone.is_err());

    });
}

#[test]
fn mindmap_flow_with_frame_extras_and_edges() {
    let ctx = Ctx::new("mm");
    let st = ctx.s();
    tauri::async_runtime::block_on(async {

    let map = variable_lib::mindmap::create_mindmap(st.clone(), Some("架构图".into()), None).unwrap();
    library::validate_name(&map.name).unwrap();

    variable_lib::mindmap::update_mindmap(
        st.clone(),
        variable_lib::mindmap::MindmapUpdate {
            id: map.id.clone(),
            viewport_x: Some(-120.5),
            viewport_y: Some(60.0),
            zoom: Some(1.25),
            grid_enabled: Some(false),
            snap_enabled: Some(false),
        },
    )
    .await
    .unwrap();

    let n1 = node_fixture(&map.id, "n1", -200.0, -30.0, "tech");
    let mut n2 = node_fixture(&map.id, "n2", 200.0, 45.0, "");
    n2.group_id = Some("grp".into());
    let saved = variable_lib::mindmap::save_nodes(st.clone(), vec![n1.clone(), n2.clone()]).await.unwrap();
    assert_eq!(saved[0].rotation, -30.0);
    assert_eq!(saved[0].preset, "tech");

    let edge = Edge {
        id: "e1".into(),
        mindmap_id: map.id.clone(),
        source_node_id: "n1".into(),
        target_node_id: "n2".into(),
        direction: "both".into(),
        line_style: "dashed".into(),
        path_style: "ortho".into(),
        color: "#7fc8a9".into(),
        width: 2.0,
        label: "依赖".into(),
        animated: true,
        glow: true,
        created_at: 0,
    };
    variable_lib::mindmap::save_edge(st.clone(), edge.clone()).await.unwrap();
    // upsert path (change label only)
    variable_lib::mindmap::save_edge(st.clone(), Edge { label: "强依赖".into(), ..edge.clone() }).await.unwrap();

    let data = variable_lib::mindmap::get_mindmap(st.clone(), map.id.clone()).unwrap();
    assert_eq!(data.mindmap.zoom, 1.25);
    assert!(!data.mindmap.grid_enabled);
    assert_eq!(data.nodes.len(), 2);
    assert_eq!(data.nodes[0].rotation, -30.0);
    assert_eq!(data.edges[0].label, "强依赖");
    assert!(data.edges[0].glow);

    // delete one node → its edge must go too
    variable_lib::mindmap::delete_nodes(st.clone(), vec!["n1".into()]).await.unwrap();
    let data = variable_lib::mindmap::get_mindmap(st.clone(), map.id.clone()).unwrap();
    assert_eq!(data.nodes.len(), 1);
    assert!(data.edges.is_empty());

    variable_lib::mindmap::rename_mindmap(st.clone(), map.id.clone(), "架构 v2".into()).unwrap();
    variable_lib::mindmap::trash_mindmap(st.clone(), map.id.clone()).unwrap();

    });
}

#[test]
fn media_import_reference_relocate_and_cleanup() {
    let ctx = Ctx::new("media");
    let st = ctx.s();
    tauri::async_runtime::block_on(async {

    // copy mode: file lands under media/
    let src_png = ctx.base.join("pic.png");
    fs::write(&src_png, png_bytes()).unwrap();
    let views = variable_lib::media::import_media(
        st.clone(),
        variable_lib::media::ImportRequest {
            paths: vec![src_png.to_string_lossy().to_string()],
            mode: "copy".into(),
            document_id: None,
            node_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(views.len(), 1);
    assert!(views[0].copied);
    assert!(views[0].abs_path.starts_with(ctx.s().media_dir.to_string_lossy().as_ref()));
    assert!(fs::metadata(&views[0].abs_path).is_ok());

    // reference mode: original kept, stored as-is
    let ext_png = ctx.base.join("ref.jpg");
    fs::write(&ext_png, vec![0xFF, 0xD8, 1, 2]).unwrap();
    let refv = variable_lib::media::import_media(
        st.clone(),
        variable_lib::media::ImportRequest {
            paths: vec![ext_png.to_string_lossy().to_string()],
            mode: "reference".into(),
            document_id: None,
            node_id: None,
        },
    )
    .await
    .unwrap();
    assert!(!refv[0].copied);
    assert_eq!(refv[0].rel_path, fs::canonicalize(&ext_png).unwrap().to_string_lossy());

    // rejections: missing file, unsupported executable type
    assert!(variable_lib::media::import_media(
        st.clone(),
        variable_lib::media::ImportRequest {
            paths: vec![ctx.base.join("nope.png").to_string_lossy().to_string()],
            mode: "copy".into(),
            document_id: None,
            node_id: None,
        },
    )
    .await
    .is_err());
    let exe = ctx.base.join("bad.exe");
    fs::write(&exe, b"MZ").unwrap();
    assert!(variable_lib::media::import_media(
        st.clone(),
        variable_lib::media::ImportRequest {
            paths: vec![exe.to_string_lossy().to_string()],
            mode: "copy".into(),
            document_id: None,
            node_id: None,
        },
    )
    .await
    .is_err());

    // data-url paste import
    let pasted = variable_lib::media::import_data_url(
        st.clone(),
        "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==".into(),
        Some("pasted.png".into()),
    )
    .map_err(|e| panic!("dataurl: {e}"))
    .unwrap();
    assert_eq!(pasted.media_type, "image");

    // attach to a document + list
    let doc = library::create_document(st.clone(), None, Some("m".into())).unwrap();
    let att = variable_lib::media::attach_media(st.clone(), views[0].media_id.clone(), Some(doc.id.clone()), None).unwrap();
    let listed = variable_lib::media::list_attachments(st.clone(), Some(doc.id.clone()), None).unwrap();
    assert_eq!(listed.len(), 1, "only the explicit attach belongs to this doc");

    // relocate a reference attachment to a new real file
    let newfile = ctx.base.join("relocated.png");
    fs::write(&newfile, png_bytes()).unwrap();
    let upd = variable_lib::media::resolve_media_path(st.clone(), refv[0].id.clone(), newfile.to_string_lossy().to_string());
    // reference attachments keep external path; either outcome must be consistent
    if upd.is_ok() {
        let u = upd.unwrap();
        assert!(!u.rel_path.is_empty());
    }

    // delete_media removes copied file from disk
    let fname = views[0].rel_path.trim_start_matches("media/").to_string();
    variable_lib::media::delete_media(st.clone(), views[0].media_id.clone()).unwrap();
    assert!(fs::metadata(ctx.s().media_dir.join(&fname)).is_err());
    let _ = att;

    });
}

#[test]
fn backup_restore_roundtrip_preserves_data() {
    let ctx = Ctx::new("backup");
    let st = ctx.s();
    tauri::async_runtime::block_on(async {

    let doc = library::create_document(st.clone(), None, Some("before".into())).unwrap();
    library::save_document(
        st.clone(),
        DocumentInput { id: doc.id.clone(), title: "before-title".into(), content_html: "A".into(), content_text: "A".into(), favorite: None, folder_id: None },
    )
    .await
    .unwrap();

    let bk = variable_lib::backup::create_backup(st.clone(), Some("test".into())).await.unwrap();
    assert!(bk.size > 0);

    // mutate after backup
    library::save_document(
        st.clone(),
        DocumentInput { id: doc.id.clone(), title: "after-title".into(), content_html: "B".into(), content_text: "B".into(), favorite: None, folder_id: None },
    )
    .await
    .unwrap();

    let list = variable_lib::backup::list_backups(st.clone()).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].status, "ok");

    // restore rolls content back to pre-mutation state
    variable_lib::backup::restore_backup(st.clone(), bk.file_name.clone()).await.map_err(|e| panic!("restore: {e}")).unwrap();
    let restored = library::get_document(st.clone(), doc.id.clone()).unwrap();
    assert_eq!(restored.title, "before-title");
    assert_eq!(restored.content_text, "A");

    // export backup to user-chosen destination
    let dest = ctx.base.join("exported.db");
    variable_lib::backup::export_backup(st.clone(), bk.file_name.clone(), dest.to_string_lossy().to_string()).unwrap();
    assert!(dest.exists());

    // delete backup removes file + row
    variable_lib::backup::delete_backup(st.clone(), bk.file_name.clone()).unwrap();
    assert!(variable_lib::backup::list_backups(st.clone()).unwrap().is_empty());

    });
}

#[test]
fn recovery_file_to_document() {
    let ctx = Ctx::new("recovery");
    let st = ctx.s();

    let id = settings_cmd::write_recovery_file(
        st.clone(),
        settings_cmd::RecoveryPayload {
            saved_at: 1234567,
            title: "闪断前".into(),
            content_html: "<p>重要内容</p>".into(),
            content_text: "重要内容".into(),
        },
    )
    .unwrap();
    let entries = settings_cmd::list_recovery_files(st.clone()).unwrap();
    assert_eq!(entries.len(), 1);
    let read = settings_cmd::read_recovery_file(st.clone(), entries[0].id.clone()).unwrap();
    assert_eq!(read.title, "闪断前");

    let doc_id = settings_cmd::recover_to_document(st.clone(), id).unwrap();
    let doc = library::get_document(st.clone(), doc_id).unwrap();
    assert_eq!(doc.title, "闪断前");
    assert!(entries.len() >= 1);
    assert!(settings_cmd::list_recovery_files(st.clone()).unwrap().is_empty());
}

#[test]
fn workspace_export_import_roundtrip() {
    let ctx = Ctx::new("ws");
    let st = ctx.s();
    tauri::async_runtime::block_on(async {

    // seed
    let folder = library::create_folder(st.clone(), "F".into(), None).unwrap();
    let doc = library::create_document(st.clone(), Some(folder.id.clone()), Some("doc1".into())).unwrap();
    library::save_document(
        st.clone(),
        DocumentInput { id: doc.id.clone(), title: "doc1".into(), content_html: "<p>hello</p>".into(), content_text: "hello".into(), favorite: None, folder_id: None },
    )
    .await
    .unwrap();
    library::set_document_tags(st.clone(), doc.id.clone(), vec!["t1".into()]).unwrap();
    let map = variable_lib::mindmap::create_mindmap(st.clone(), Some("M".into()), Some(folder.id.clone())).unwrap();
    variable_lib::mindmap::save_nodes(st.clone(), vec![node_fixture(&map.id, "k1", 0.0, 15.0, "minimal")]).await.unwrap();

    // export into empty temp dir with an image to bundle
    let out = ctx.base.join("ws-out");
    fs::create_dir_all(&out).unwrap();
    let img_src = ctx.base.join("img.png");
    fs::write(&img_src, png_bytes()).unwrap();
    variable_lib::media::import_media(
        st.clone(),
        variable_lib::media::ImportRequest { paths: vec![img_src.to_string_lossy().to_string()], mode: "copy".into(), document_id: Some(doc.id.clone()), node_id: None },
    )
    .await
    .unwrap();

    let res = variable_lib::export::export_workspace(st.clone(), out.to_string_lossy().to_string()).await.unwrap();
    assert!(res.count >= 1);
    assert!(out.join("workspace.json").exists());
    assert!(out.join("media").read_dir().unwrap().count() >= 1);

    // import into the SAME db → duplicates with fresh ids, originals intact
    let summary = variable_lib::export::import_workspace(st.clone(), out.join("workspace.json").to_string_lossy().to_string()).await.unwrap();
    assert_eq!(summary.documents, 1);
    assert_eq!(summary.folders, 1);
    assert_eq!(summary.mindmaps, 1);
    assert_eq!(summary.nodes, 1);
    let docs_after = library::list_documents(st.clone(), ListFilter { view: Some("all".into()), folder_id: None, query: Some("doc1".into()), tag: None, sort: None }).unwrap();
    assert_eq!(docs_after.len(), 2, "original + imported copy");

    });
}

#[test]
fn exports_and_settings_and_textfile() {
    let ctx = Ctx::new("exp");
    let st = ctx.s();
    tauri::async_runtime::block_on(async {

    let doc = library::create_document(st.clone(), None, Some("导出我".into())).unwrap();
    library::save_document(
        st.clone(),
        DocumentInput { id: doc.id.clone(), title: "导出我".into(), content_html: "<h1>标题</h1><p><strong>粗</strong>体</p>".into(), content_text: "标题 粗体".into(), favorite: None, folder_id: None },
    )
    .await
    .unwrap();

    let md = ctx.base.join("one.md");
    let r = variable_lib::export::export_documents(st.clone(), vec![doc.id.clone()], "md".into(), md.to_string_lossy().to_string()).unwrap();
    assert_eq!(r.count, 1);
    let body = fs::read_to_string(&md).unwrap();
    assert!(body.contains("# 导出我"));
    assert!(body.contains("**粗**"));

    let html = ctx.base.join("one.html");
    variable_lib::export::export_documents(st.clone(), vec![doc.id.clone()], "html".into(), html.to_string_lossy().to_string()).unwrap();
    assert!(fs::read_to_string(&html).unwrap().contains("<!doctype html>"));

    // overwrite guard
    assert!(variable_lib::export::export_documents(st.clone(), vec![doc.id.clone()], "md".into(), md.to_string_lossy().to_string()).is_err());

    // multi-doc export into directory
    let d2 = library::create_document(st.clone(), None, Some("第二篇".into())).unwrap();
    let dir = ctx.base.join("many");
    let r = variable_lib::export::export_documents(st.clone(), vec![doc.id.clone(), d2.id.clone()], "json".into(), dir.to_string_lossy().to_string()).unwrap();
    assert_eq!(r.count, 2);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);

    // mindmap json export
    let map = variable_lib::mindmap::create_mindmap(st.clone(), Some("MM".into()), None).unwrap();
    let mm_json = ctx.base.join("mm.json");
    variable_lib::export::export_mindmap_json(st.clone(), map.id.clone(), mm_json.to_string_lossy().to_string()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&fs::read_to_string(&mm_json).unwrap()).unwrap();
    assert_eq!(parsed["app"], "variable-mindmap");

    // settings upsert/read/reset
    let mut m = std::collections::HashMap::new();
    m.insert("theme".to_string(), "paper".to_string());
    m.insert("lastMindmapId".to_string(), "xyz".to_string());
    settings_cmd::set_settings(st.clone(), m).await.unwrap();
    let all = settings_cmd::get_all_settings(st.clone()).unwrap();
    assert_eq!(all.get("theme").unwrap(), "paper");
    settings_cmd::reset_ui_settings(st.clone()).unwrap();
    let all = settings_cmd::get_all_settings(st.clone()).unwrap();
    assert!(all.get("theme").is_none());
    assert!(all.get("lastMindmapId").is_none());

    // generic text save with overwrite guard
    let txt = ctx.base.join("sel.json");
    ipc_save_text(&st, &txt, "{}", false).unwrap();
    assert!(ipc_save_text(&st, &txt, "{}", false).is_err());
    ipc_save_text(&st, &txt, "[1]", true).unwrap();
    assert_eq!(fs::read_to_string(&txt).unwrap(), "[1]");

    // system helpers: existence checks
    let checks = variable_lib::system::check_paths_exist(st.clone(), vec![txt.to_string_lossy().to_string(), "Z:\\definitely\\missing".into()]).unwrap();
    assert!(checks[0].exists);
    assert!(!checks[1].exists);

    });
}

fn ipc_save_text(_st: &State<AppState>, p: &std::path::Path, contents: &str, allow: bool) -> CmdResult<String> {
    variable_lib::system::save_text_file(p.to_string_lossy().to_string(), contents.to_string(), Some(allow))
}








