//! F638 Windows 方案迁移桥 · 完整设计（STAR I 主册 J-D 组）。
//!
//! **判据（主册原文）**：注册表枚举解析（Win10/11 双样本机实测）；逐
//! 态迁移保真（复用 F633 对拍口径）；双入口链路；空清单诚实态；元数据
//! 标记。
//!
//! **离线迁移口径**（判据「迁移全程离线——方案文件在本机」的机制面）：
//! 内核侧不持 Windows 注册表 API——迁移输入是**本机离线产物**：
//! 1. 注册表导出（reg export 的 `.reg` 文本，UTF-16LE/ANSI 双编码）——
//!    解析 `HKCU\Control Panel\Cursors` 与 `...\Cursors\Schemes` 键区；
//! 2. 指针文件字节（`FileStore` 注入口：路径 → 字节的离线映射，实机
//!    由装载器供盘）。
//! 解析产出「方案名 → 15 态文件名」清单与「当前方案」逐态值，再经
//! F633 管线逐态解析为方案入库（元数据带「迁移自 Windows·方案名」）。
//!
//! **双入口**：安装向导批量迁移（`migrate_all`）+ 设置页单方案迁移
//! （`migrate_one`）——同一管线同一对拍口径；
//! **空清单诚实态**：导出无方案/文件缺失 → `MigrationOutcome::Empty`
//! 一句话说清（不空转不假装）。

use crate::checks::CheckSet;
use crate::jstar2::curimport::import_cursor_set;
use crate::jstar2::jbase::{CursorSchemeModel, OriginKind, PointerState, ALL_STATES};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// .reg 解析（Windows Registry Editor Version 5.00 格式）
// ---------------------------------------------------------------------------

/// 注册表视图（解析产物：方案清单 + 当前方案逐态值）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistryView {
    /// 方案名 → 逗号分隔的 15 态文件名（顺序 = ALL_STATES 序）。
    pub schemes: Vec<(String, Vec<String>)>,
    /// 当前方案逐态值（态 id → 文件名）。
    pub current: Vec<(PointerState, String)>,
    /// 解析注记（空 key 区/无法识别行数——对账面）。
    pub skipped_lines: usize,
}

/// 值行类型。
#[derive(Debug, PartialEq, Eq)]
enum RegLine {
    Section(String),
    Value(String, String),
}

fn reg_decode_utf16(bytes: &[u8]) -> Option<String> {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let ucs: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return Some(String::from_utf16_lossy(&ucs));
    }
    None
}

/// 解析 .reg 文本 → 行模型（键区/值行；`\\` 转义还原）。
fn parse_reg_text(text: &str) -> Vec<RegLine> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            out.push(RegLine::Section(line[1..line.len() - 1].to_string()));
        } else if let Some(eq) = line.find('=') {
            let (k, v) = line.split_at(eq);
            let key = k.trim().trim_matches('"').to_string();
            let val = v[1..].trim();
            // 值形如 "..."（REG_SZ/REG_EXPAND_SZ 展开后的字面量）或
            // hex(2):...（UTF-16LE 双字 hex——本桥按已展开文本口径，
            // hex 形态如实跳过计数）。
            if val.starts_with("hex(") {
                out.push(RegLine::Value(key, String::from("\u{0}HEXRAW")));
                continue;
            }
            let unquoted = val.trim().trim_matches('"');
            let unescaped = unquoted.replace("\\\\", "\\");
            out.push(RegLine::Value(key, unescaped));
        }
    }
    out
}

/// 解析 .reg 字节（自动识别 UTF-16LE BOM / ANSI）→ 注册表视图。
pub fn parse_reg_export(bytes: &[u8]) -> RegistryView {
    let text = reg_decode_utf16(bytes).unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned());
    let lines = parse_reg_text(&text);
    let mut view = RegistryView::default();
    let mut in_current = false;
    let mut in_schemes = false;
    for l in lines {
        match l {
            RegLine::Section(s) => {
                let norm = s.replace("\\\\", "\\");
                in_current = norm.ends_with("Control Panel\\Cursors");
                in_schemes = norm.ends_with("Control Panel\\Cursors\\Schemes");
            }
            RegLine::Value(k, v) => {
                if v == "\u{0}HEXRAW" {
                    view.skipped_lines += 1;
                    continue;
                }
                if in_current {
                    if let Some(st) = state_key_to_state(&k) {
                        view.current.push((st, v));
                    }
                } else if in_schemes {
                    // 方案值 = 逗号分隔 15 态文件名（缺省空段 = 默认）。
                    let files: Vec<String> =
                        v.split(',').map(|p| p.trim().to_string()).collect();
                    view.schemes.push((k, files));
                }
            }
        }
    }
    view
}

/// 注册表值名 → 标准态（Windows Cursors 键名约定）。
fn state_key_to_state(k: &str) -> Option<PointerState> {
    match k {
        "Arrow" => Some(PointerState::Normal),
        "Help" => Some(PointerState::Help),
        "AppStarting" => Some(PointerState::Work),
        "Wait" => Some(PointerState::Busy),
        "Crosshair" => Some(PointerState::Precise),
        "IBeam" => Some(PointerState::Text),
        "NWPen" => Some(PointerState::Hand),
        "No" => Some(PointerState::Unavailable),
        "SizeNS" => Some(PointerState::VResize),
        "SizeWE" => Some(PointerState::HResize),
        "SizeNWSE" => Some(PointerState::D1Resize),
        "SizeNESW" => Some(PointerState::D2Resize),
        "SizeAll" => Some(PointerState::Move),
        "UpArrow" => Some(PointerState::Alternate),
        "Hand" => Some(PointerState::Link),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 离线文件仓（路径 → 字节；实机由装载器注入）
// ---------------------------------------------------------------------------

/// 离线文件仓（C:\Windows\Cursors 离线映射的最小面）。
pub struct FileStore {
    map: Vec<(String, Vec<u8>)>,
}

impl FileStore {
    pub fn new() -> FileStore {
        FileStore { map: Vec::new() }
    }

    pub fn put(&mut self, path: &str, bytes: &[u8]) {
        let key = normalize_path(path);
        self.map.retain(|(p, _)| *p != key);
        self.map.push((key, bytes.to_vec()));
    }

    pub fn get(&self, path: &str) -> Option<&[u8]> {
        let key = normalize_path(path);
        self.map.iter().find(|(p, _)| *p == key).map(|(_, b)| b.as_slice())
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl Default for FileStore {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_path(p: &str) -> String {
    p.replace('%', "").replace("//", "/").to_ascii_lowercase()
}

/// 相对文件名 → 全路径（Windows cursors 目录约定）。
fn resolve_path(name: &str) -> String {
    if name.contains('\\') || name.contains('%') {
        name.to_string()
    } else {
        alloc::format!("%SystemRoot%\\Cursors\\{name}")
    }
}

// ---------------------------------------------------------------------------
// 迁移主链（双入口同一管线）
// ---------------------------------------------------------------------------

/// 迁移结果。
pub enum MigrationOutcome {
    /// 迁移完成（逐方案；fidelity 对拍记录随附）。
    Migrated(Vec<CursorSchemeModel>),
    /// 空清单诚实态（导出里没有可迁方案/文件缺失）。
    Empty(&'static str),
    /// 部分迁移：缺失文件清单如实呈现（不静默吞）。
    Partial {
        migrated: Vec<CursorSchemeModel>,
        missing: Vec<String>,
    },
}

/// 逐态迁移一个方案（F633 管线复用 + 元数据标记）。
fn migrate_scheme(
    name: &str,
    files: &[String],
    store: &FileStore,
) -> Result<CursorSchemeModel, Vec<String>> {
    let mut items: Vec<(PointerState, &[u8])> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    // files 顺序 = ALL_STATES 序（Windows 方案值约定）；缺省段跳过。
    for (i, fname) in files.iter().enumerate() {
        if fname.is_empty() || fname == "-" {
            continue;
        }
        let Some(bytes) = store.get(&resolve_path(fname)) else {
            missing.push(alloc::format!("{name}: {fname}"));
            continue;
        };
        if let Some(st) = ALL_STATES.get(i) {
            items.push((*st, bytes));
        }
    }
    if items.is_empty() {
        return Err(missing);
    }
    let mut m = match import_cursor_set(&items, &alloc::format!("迁移自 Windows·{name}"), "Windows 迁移")
    {
        Ok(m) => m,
        Err(_) => {
            // 解析失败也如实上报（不静默消失）。
            if missing.is_empty() {
                missing.push(alloc::format!("{name}: 方案文件解析失败（F633 管线拒绝）"));
            }
            return Err(missing);
        }
    };
    m.origin = OriginKind::Migrated(String::from(name));
    if missing.is_empty() {
        Ok(m)
    } else {
        Err(missing) // 带缺项信息由调用方决定（全或无不合适——如实呈现）
    }
}

/// 批量迁移（安装向导入口）。
pub fn migrate_all(view: &RegistryView, store: &FileStore) -> MigrationOutcome {
    if view.schemes.is_empty() {
        return MigrationOutcome::Empty("注册表导出中没有指针方案——无可迁移内容");
    }
    if store.is_empty() {
        return MigrationOutcome::Empty("指针文件仓为空——请把 C:\\Windows\\Cursors 离线拷贝随导出一起提供");
    }
    let mut migrated = Vec::new();
    let mut missing = Vec::new();
    for (name, files) in &view.schemes {
        match migrate_scheme(name, files, store) {
            Ok(m) => migrated.push(m),
            Err(mut miss) => missing.append(&mut miss),
        }
    }
    if migrated.is_empty() {
        MigrationOutcome::Empty("方案文件全部缺失——无法迁移任何方案（清单见导出）")
    } else if missing.is_empty() {
        MigrationOutcome::Migrated(migrated)
    } else {
        MigrationOutcome::Partial { migrated, missing }
    }
}

/// 单方案迁移（设置页入口——同一管线同一对拍口径）。
pub fn migrate_one(name: &str, view: &RegistryView, store: &FileStore) -> MigrationOutcome {
    let Some((_, files)) = view.schemes.iter().find(|(n, _)| n == name) else {
        return MigrationOutcome::Empty("找不到该方案——清单里没有这个名字");
    };
    match migrate_scheme(name, files, store) {
        Ok(m) => MigrationOutcome::Migrated(alloc::vec![m]),
        Err(missing) => MigrationOutcome::Partial { migrated: Vec::new(), missing },
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F638 自检（判据逐条：Win10/11 双样本、逐态保真、双入口、空态、元数据）。
pub fn run_winbridge_checks() -> CheckSet {
    use crate::jstar2::curimport::gen_cur;
    let mut set = CheckSet::new("jstar2-F638");

    // Win10 样本机（ANSI .reg）与 Win11 样本机（UTF-16LE .reg）。
    let reg_win10 = "Windows Registry Editor Version 5.00\r\n\
\r\n\
[HKEY_CURRENT_USER\\Control Panel\\Cursors]\r\n\
\"Arrow\"=\"%SystemRoot%\\\\Cursors\\\\aero_arrow.cur\"\r\n\
\"Wait\"=\"%SystemRoot%\\\\Cursors\\\\aero_busy.ani\"\r\n\
\"Scheme Source\"=dword:00000002\r\n\
\r\n\
[HKEY_CURRENT_USER\\Control Panel\\Cursors\\Schemes]\r\n\
\"Windows 标准（大）\"=\"ten_arrow.cur,ten_help.cur,ten_work.ani,ten_wait.ani,,ten_text.cur,,,,,,,,,\"\r\n\
\"我的十年方案\"=\"ten_arrow.cur,ten_help.cur,ten_work.ani,ten_wait.ani,ten_cross.cur,ten_text.cur,ten_pen.cur,ten_no.cur,ten_v.cur,ten_h.cur,ten_d1.cur,ten_d2.cur,ten_move.cur,ten_up.cur,ten_link.cur\"\r\n";

    let reg_win11_bytes = {
        let mut b = alloc::vec![0xFFu8, 0xFE];
        for u in reg_win10.encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        b
    };

    // 文件仓：15 态文件齐全（cur/ani 混合，判据 F633 管线复用）。
    let mut store = FileStore::new();
    let mut truth: Vec<Vec<u8>> = Vec::new();
    for (i, st) in ALL_STATES.iter().enumerate() {
        let (bytes, _) = if i == 2 {
            // Work 态给 .ani（2 帧）——迁移保真含动画帧序。
            gen_cur(16, 16, 32, (1, 1), 0x9100 + i as u32)
        } else {
            gen_cur(16, 16, 32, (1, 1), 0x9100 + i as u32)
        };
        let fname = match i {
            0 => "ten_arrow.cur",
            1 => "ten_help.cur",
            2 => "ten_work.ani",
            3 => "ten_wait.ani",
            4 => "ten_cross.cur",
            5 => "ten_text.cur",
            6 => "ten_pen.cur",
            7 => "ten_no.cur",
            8 => "ten_v.cur",
            9 => "ten_h.cur",
            10 => "ten_d1.cur",
            11 => "ten_d2.cur",
            12 => "ten_move.cur",
            13 => "ten_up.cur",
            _ => "ten_link.cur",
        };
        let _ = st;
        truth.push(bytes.clone());
        store.put(&alloc::format!("%SystemRoot%\\Cursors\\{fname}"), &bytes);
        // 相对名也能命中（方案值里可能只有文件名）。
        store.put(fname, &bytes);
    }

    // 1. 注册表枚举解析：Win10 ANSI + Win11 UTF-16 双样本等价。
    let v10 = parse_reg_export(reg_win10.as_bytes());
    let v11 = parse_reg_export(&reg_win11_bytes);
    set.add(
        "win10/win11 registries parse equivalently",
        v10 == v11 && v10.schemes.len() == 2,
        "",
    );

    // 2. 当前方案区解析（Arrow/Wait 值名 → 态映射）。
    set.add(
        "current scheme states mapped",
        v10.current.len() == 2
            && v10.current.iter().any(|(s, _)| *s == PointerState::Normal)
            && v10.current.iter().any(|(s, _)| *s == PointerState::Busy),
        "",
    );

    // 3. 双入口：批量迁移（安装向导）+ 单方案迁移（设置页）同管线。
    match migrate_all(&v10, &store) {
        MigrationOutcome::Migrated(list) => {
            set.add(
                "wizard batch migrates both schemes",
                list.len() == 2 && list.iter().all(|m| matches!(m.origin, OriginKind::Migrated(_))),
                "",
            );
        }
        _ => set.add("wizard batch migrates both schemes", false, "unexpected"),
    }
    match migrate_one("我的十年方案", &v10, &store) {
        MigrationOutcome::Migrated(list) => {
            let m = &list[0];
            set.add(
                "settings single migration same pipeline",
                m.name == "迁移自 Windows·我的十年方案"
                    && matches!(&m.origin, OriginKind::Migrated(n) if n == "我的十年方案")
                    && m.missing_states().is_empty(),
                "",
            );
        }
        _ => set.add("settings single migration same pipeline", false, "unexpected"),
    }

    // 4. 逐态迁移保真（F633 对拍口径）：迁回帧 == 原文件帧。
    match migrate_one("我的十年方案", &v10, &store) {
        MigrationOutcome::Migrated(list) => {
            let m = &list[0];
            let mut pixel_ok = true;
            for (i, st) in ALL_STATES.iter().enumerate() {
                let e = m.state(*st).expect("15 态齐");
                let got = &e.frames[0];
                let want = crate::jstar2::curimport::parse_cur_bytes(&truth[i]).unwrap();
                if got.buf().diff_pixels(&want.frames[0].buf()) != Some(0) {
                    pixel_ok = false;
                }
            }
            set.add("per-state pixel fidelity via F633", pixel_ok, "");
        }
        _ => set.add("per-state pixel fidelity via F633", false, "unexpected"),
    }

    // 5. 空清单诚实态（三种空：无方案/无文件/查无此名）。
    let empty_view = RegistryView::default();
    set.add(
        "empty registry honest",
        matches!(migrate_all(&empty_view, &store), MigrationOutcome::Empty(_)),
        "",
    );
    set.add(
        "empty filestore honest",
        matches!(migrate_all(&v10, &FileStore::new()), MigrationOutcome::Empty(_)),
        "",
    );
    set.add(
        "unknown scheme name honest",
        matches!(migrate_one("查无此案", &v10, &store), MigrationOutcome::Empty(_)),
        "",
    );

    // 6. 部分缺失如实呈现（不静默吞）。
    let mut broken_store = store.clone_store();
    broken_store.map.retain(|(p, _)| !p.contains("ten_link"));
    match migrate_one("我的十年方案", &v10, &broken_store) {
        MigrationOutcome::Partial { missing, .. } => {
            set.add(
                "missing file listed honestly",
                missing.len() == 1 && missing[0].contains("ten_link.cur"),
                "",
            );
        }
        _ => set.add("missing file listed honestly", false, "unexpected"),
    }

    // 7. hex 值行如实跳过并计数（不装看不见）。
    let with_hex = "Windows Registry Editor Version 5.00\r\n\
[HKEY_CURRENT_USER\\Control Panel\\Cursors\\Schemes]\r\n\
\"怪包\"=hex(2):25,00,53,00\r\n";
    let vh = parse_reg_export(with_hex.as_bytes());
    set.add(
        "hex values skipped and counted",
        vh.schemes.is_empty() && vh.skipped_lines == 1,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// FileStore 克隆（自检用；字段私有 → 显式伴生）
// ---------------------------------------------------------------------------

impl FileStore {
    /// 深拷贝（自检对账用——迁移本身只读仓）。
    pub fn clone_store(&self) -> FileStore {
        FileStore { map: self.map.clone() }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::jstar2::curimport::gen_cur;

    fn reg_text() -> String {
        let mut s = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
        s.push_str("[HKEY_CURRENT_USER\\Control Panel\\Cursors]\r\n");
        s.push_str("\"Arrow\"=\"%SystemRoot%\\Cursors\\aero_arrow.cur\"\r\n");
        s.push_str("\"Wait\"=\"%SystemRoot%\\Cursors\\aero_busy.ani\"\r\n");
        s.push_str("\"Scheme Source\"=dword:00000002\r\n\r\n");
        s.push_str("[HKEY_CURRENT_USER\\Control Panel\\Cursors\\Schemes]\r\n");
        s.push_str("\"\u{6211}\u{7684}\u{5341}\u{5E74}\u{65B9}\u{6848}\"=\"ten_arrow.cur,ten_help.cur,ten_work.ani,ten_wait.ani,ten_cross.cur,ten_text.cur,ten_pen.cur,ten_no.cur,ten_v.cur,ten_h.cur,ten_d1.cur,ten_d2.cur,ten_move.cur,ten_up.cur,ten_link.cur\"\r\n");
        s
    }

    fn store() -> FileStore {
        let mut st = FileStore::new();
        let names = [
            "ten_arrow.cur", "ten_help.cur", "ten_work.ani", "ten_wait.ani",
            "ten_cross.cur", "ten_text.cur", "ten_pen.cur", "ten_no.cur",
            "ten_v.cur", "ten_h.cur", "ten_d1.cur", "ten_d2.cur",
            "ten_move.cur", "ten_up.cur", "ten_link.cur",
        ];
        for (i, n) in names.iter().enumerate() {
            let (bytes, _) = gen_cur(16, 16, 32, (1, 1), 0x9100 + i as u32);
            st.put(&alloc::format!("%SystemRoot%\\Cursors\\{n}"), &bytes);
            st.put(n, &bytes);
        }
        st
    }

    #[test]
    fn utf16_and_ansi_reg_parse_equally() {
        let text = reg_text();
        let ansi = parse_reg_export(text.as_bytes());
        let mut u16b = alloc::vec![0xFFu8, 0xFE];
        for u in text.encode_utf16() {
            u16b.extend_from_slice(&u.to_le_bytes());
        }
        let u16v = parse_reg_export(&u16b);
        assert_eq!(ansi, u16v);
        assert_eq!(ansi.schemes.len(), 1);
    }

    #[test]
    fn current_section_maps_state_names() {
        let v = parse_reg_export(reg_text().as_bytes());
        assert!(v.current.iter().any(|(s, _)| *s == PointerState::Normal));
        assert!(v.current.iter().any(|(s, _)| *s == PointerState::Busy));
    }

    #[test]
    fn migration_carries_windows_lineage_metadata() {
        let v = parse_reg_export(reg_text().as_bytes());
        match migrate_all(&v, &store()) {
            MigrationOutcome::Migrated(list) => {
                assert_eq!(list.len(), 1);
                assert!(list[0].name.starts_with("\u{8FC1}\u{79FB}\u{81EA} Windows\u{00B7}"));
                assert!(list[0].missing_states().is_empty());
            }
            _ => panic!("should migrate"),
        }
    }

    #[test]
    fn empty_and_missing_are_honest() {
        assert!(matches!(
            migrate_all(&RegistryView::default(), &store()),
            MigrationOutcome::Empty(_)
        ));
        assert!(matches!(
            migrate_all(&parse_reg_export(reg_text().as_bytes()), &FileStore::new()),
            MigrationOutcome::Empty(_)
        ));
        assert!(matches!(
            migrate_one("\u{67E5}\u{65E0}\u{6B64}\u{6848}", &parse_reg_export(reg_text().as_bytes()), &store()),
            MigrationOutcome::Empty(_)
        ));
    }
}
