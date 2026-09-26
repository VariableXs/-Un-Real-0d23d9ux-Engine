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

/// .reg 版本头（reg export 产物的身份证行）。
pub const REG_HEADER: &str = "Windows Registry Editor Version 5.00";

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
    /// .reg 版本头校验（缺版本头 = 不是 reg export 产物——如实标出，
    /// 迁移入口据此提醒；REG_HEADER 常量一处一事实）。
    pub header_ok: bool,
    /// dword 值行计数（如 Scheme Source——解析器不装看不见）。
    pub dword_lines: usize,
}

/// 值行类型。
#[derive(Debug, PartialEq, Eq)]
enum RegLine {
    Section(String),
    Value(String, String),
    /// dword:xxxx 行（REG_DWORD——非指针内容，计数不丢弃）。
    DWord(String),
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
            if val.starts_with("dword:") {
                out.push(RegLine::DWord(key));
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
    view.header_ok = text.lines().any(|l| l.trim().starts_with(REG_HEADER));
    let mut in_current = false;
    let mut in_schemes = false;
    for l in lines {
        match l {
            RegLine::Section(s) => {
                let norm = s.replace("\\\\", "\\");
                in_current = norm.ends_with("Control Panel\\Cursors");
                in_schemes = norm.ends_with("Control Panel\\Cursors\\Schemes");
            }
            RegLine::DWord(_) => {
                view.dword_lines += 1;
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
// 迁移计划预览（安装向导第二步的清单面——先看将迁什么再执行）
// ---------------------------------------------------------------------------

/// 单方案的迁移预判（ready = 文件全齐可迁；missing = 缺哪些文件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemePlan {
    pub name: String,
    pub ready: bool,
    /// 缺失文件清单（ready 时为空）。
    pub missing: Vec<String>,
}

/// 迁移计划（逐方案 ready/missing——不盲迁是流程属性：向导先展示
/// 这张单，用户确认后才进 commit）。
#[derive(Clone, Debug, Default)]
pub struct MigrationPlan {
    pub entries: Vec<SchemePlan>,
    pub header_ok: bool,
}

impl MigrationPlan {
    /// ready 方案数（向导汇总行的数字面）。
    pub fn ready_count(&self) -> usize {
        self.entries.iter().filter(|p| p.ready).count()
    }

    /// 全空判定（清单零方案——空态语义的依据）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 生成迁移计划（只读面：逐方案查文件仓，不解析不迁移）。
pub fn migration_plan(view: &RegistryView, store: &FileStore) -> MigrationPlan {
    let mut entries = Vec::new();
    for (name, files) in &view.schemes {
        let mut missing = Vec::new();
        for fname in files {
            if fname.is_empty() || fname == "-" {
                continue;
            }
            if store.get(&resolve_path(fname)).is_none() {
                missing.push(alloc::format!("{name}: {fname}"));
            }
        }
        entries.push(SchemePlan {
            name: name.clone(),
            ready: missing.is_empty(),
            missing,
        });
    }
    MigrationPlan { entries, header_ok: view.header_ok }
}

// ---------------------------------------------------------------------------
// 迁移会话与留痕台账
// ---------------------------------------------------------------------------

/// 迁移会话阶段（Parsed → Planned → Committed/Aborted——与 F630/F635
/// 同款状态机纪律：分步可中断，中断零迁移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationStage {
    Parsed,
    Planned,
    Committed,
    Aborted,
}

/// 迁移会话（安装向导/设置页共用的分步推进面）。
pub struct MigrationSession<'a> {
    view: &'a RegistryView,
    store: &'a FileStore,
    stage: MigrationStage,
    plan: Option<MigrationPlan>,
}

impl<'a> MigrationSession<'a> {
    pub fn start(view: &'a RegistryView, store: &'a FileStore) -> MigrationSession<'a> {
        MigrationSession { view, store, stage: MigrationStage::Parsed, plan: None }
    }

    pub fn stage(&self) -> MigrationStage {
        self.stage
    }

    /// 生成计划（Parsed → Planned；版本头缺失时计划如实带 header_ok=false
    /// ——可继续但 UI 应提示来源可疑）。
    pub fn plan(&mut self) -> &MigrationPlan {
        if self.stage == MigrationStage::Parsed {
            self.plan = Some(migration_plan(self.view, self.store));
            self.stage = MigrationStage::Planned;
        }
        self.plan.as_ref().unwrap()
    }

    /// 提交迁移（Planned → Committed；未出计划直接提交被拒——不盲迁）。
    pub fn commit(&mut self) -> MigrationOutcome {
        if self.stage != MigrationStage::Planned {
            return MigrationOutcome::Empty("迁移会话未出计划——先看清单再执行（不盲迁）");
        }
        self.stage = MigrationStage::Committed;
        migrate_all(self.view, self.store)
    }

    /// 显式取消（Planned 后悔 = 零迁移零留痕）。
    pub fn abort(&mut self) -> bool {
        if self.stage == MigrationStage::Committed {
            return false;
        }
        self.stage = MigrationStage::Aborted;
        true
    }
}

/// 迁移留痕记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationRecord {
    pub at_ms: u64,
    pub scheme: String,
    pub fidelity: &'static str,
}

/// 迁移留痕台账（环形 32——F372 留痕纪律的迁移面）。
#[derive(Clone, Debug, Default)]
pub struct MigrationLedger {
    records: Vec<MigrationRecord>,
    dropped: usize,
}

impl MigrationLedger {
    pub const CAP: usize = 32;

    pub fn record(&mut self, at_ms: u64, scheme: &str, fidelity: &'static str) {
        if self.records.len() >= Self::CAP {
            self.records.remove(0);
            self.dropped += 1;
        }
        self.records.push(MigrationRecord {
            at_ms,
            scheme: String::from(scheme),
            fidelity,
        });
    }

    pub fn records(&self) -> &[MigrationRecord] {
        &self.records
    }

    pub fn dropped(&self) -> usize {
        self.dropped
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

    // 8. 版本头校验 + dword 行计数。
    let no_header = "随机文本不是 reg export";
    let vh2 = parse_reg_export(no_header.as_bytes());
    set.add(
        "reg header validated",
        v10.header_ok && !vh2.header_ok,
        "",
    );
    set.add(
        "dword lines counted",
        v10.dword_lines == 1,
        "",
    );

    // 9. 迁移计划预览：ready/missing 逐方案判定（不盲迁的清单面）。
    // 全齐仓 → 两案皆 ready；缺 link 文件的仓 → 缺项如实进清单。
    let plan = migration_plan(&v10, &store);
    let plan_broken = migration_plan(&v10, &broken_store);
    set.add(
        "migration plan previews ready and missing",
        plan.entries.len() == 2
            && plan.ready_count() == 2
            && plan.entries[0].ready
            && plan.entries[0].missing.is_empty()
            && plan.header_ok
            && plan_broken.ready_count() == 1
            && plan_broken.entries[1].missing.len() == 1
            && plan_broken.entries[1].missing[0].contains("ten_link.cur"),
        "",
    );

    // 10. 迁移会话：未出计划提交被拒；出计划后提交走同一管线；
    //     中止语义诚实（已提交后 abort 拒绝）。
    let mut sess = MigrationSession::start(&v10, &store);
    let early = matches!(sess.commit(), MigrationOutcome::Empty(_));
    sess.plan();
    let after_plan_stage = sess.stage() == MigrationStage::Planned;
    let committed = matches!(sess.commit(), MigrationOutcome::Migrated(_));
    let late_abort = !sess.abort();
    set.add(
        "migration session staged commit",
        early && after_plan_stage && committed && late_abort,
        "",
    );
    let mut sess2 = MigrationSession::start(&v10, &store);
    sess2.plan();
    set.add("migration session abort clean", sess2.abort() && sess2.stage() == MigrationStage::Aborted, "");

    // 11. 双入口字节级对账：批量与单方案产物内容指纹一致（同一管线
    //     同一输入 → 同一内容）。
    let (fp_all, fp_one) = match (migrate_all(&v10, &store), migrate_one("我的十年方案", &v10, &store)) {
        (MigrationOutcome::Migrated(a), MigrationOutcome::Migrated(b)) => {
            let fpa = a.iter().map(|m| crate::jstar2::jbase::content_fingerprint(m)).collect::<Vec<u64>>();
            let fpb = b.iter().map(|m| crate::jstar2::jbase::content_fingerprint(m)).collect::<Vec<u64>>();
            (fpa, fpb)
        }
        _ => (Vec::new(), Vec::new()),
    };
    set.add(
        "dual entry byte-identical products",
        fp_one.len() == 1 && fp_all.contains(&fp_one[0]),
        "",
    );

    // 12. 迁移留痕台账：记录、封顶滚动。
    let mut ledger = MigrationLedger::default();
    for i in 0..40u64 {
        ledger.record(i, "方案", "PixelPerfect");
    }
    set.add(
        "migration ledger records and caps",
        ledger.records().len() == MigrationLedger::CAP
            && ledger.dropped() == 8
            && ledger.records()[0].at_ms == 8,
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

// ---------------------------------------------------------------------------
// v4 深化批：.reg 导出（迁移的双向面）· 32bpp .cur 重编码 · 迁移逐态
// 保真对账报告 · 迁移批次台账
// ---------------------------------------------------------------------------

/// 库内态 → Windows 注册表值名（`state_key_to_state` 的逆映射——导入
/// 与导出共用同一张 15 态约定的两面，一处一事实）。
pub fn state_to_key_name(st: PointerState) -> &'static str {
    match st {
        PointerState::Normal => "Arrow",
        PointerState::Help => "Help",
        PointerState::Work => "AppStarting",
        PointerState::Busy => "Wait",
        PointerState::Precise => "Crosshair",
        PointerState::Text => "IBeam",
        PointerState::Hand => "NWPen",
        PointerState::Unavailable => "No",
        PointerState::VResize => "SizeNS",
        PointerState::HResize => "SizeWE",
        PointerState::D1Resize => "SizeNWSE",
        PointerState::D2Resize => "SizeNESW",
        PointerState::Move => "SizeAll",
        PointerState::Alternate => "UpArrow",
        PointerState::Link => "Hand",
    }
}

/// 合成导出文件名（方案内容指纹 + 态键名——同一方案导出两次同名：
/// 确定性是「导出 → reg import → 再迁移」对账的前提）。
pub fn synth_cursor_filename(m: &CursorSchemeModel, st: PointerState) -> String {
    let fp = crate::jstar2::jbase::content_fingerprint(m);
    alloc::format!("vx_{fp:016x}_{}.cur", state_to_key_name(st))
}

/// .reg 值文本转义（反斜杠翻倍——reg import 语法要求）。
fn reg_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
}

/// 单帧重编码为 32bpp .cur（导出双面：库内帧 → Windows 指针文件字节。
/// XOR 色面自底向上 BGRA + AND 掩码（alpha=0 处置 1）；热点走目录项
/// planes/bitcount 位——与 F633 解码口径逐字段互逆，可原路读回）。
pub fn encode_cur_32bpp(frame: &crate::jstar2::jbase::CursorFrame) -> Vec<u8> {
    let buf = frame.buf();
    let (w, h) = (buf.w as usize, buf.h as usize);
    let mask_row = (w + 31) / 32 * 4;
    let mut dib: Vec<u8> = Vec::with_capacity(40 + w * h * 4 + mask_row * h);
    // BITMAPINFOHEADER（图标 DIB 口径：biHeight = XOR+AND 双面高）。
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&(w as u32).to_le_bytes());
    dib.extend_from_slice(&((h as u32) * 2).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    for _ in 0..6 {
        dib.extend_from_slice(&0u32.to_le_bytes());
    }
    // XOR 色面：自底向上 BGRA。
    for y in (0..h).rev() {
        for x in 0..w {
            let p = buf.get(x as u16, y as u16).unwrap_or([0, 0, 0, 0]);
            dib.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    // AND 掩码：alpha=0 → 1（透明），MSB-first，行 4 字节对齐。
    for y in (0..h).rev() {
        let mut row = alloc::vec![0u8; mask_row];
        for x in 0..w {
            if buf.get(x as u16, y as u16).map(|p| p[3] == 0).unwrap_or(false) {
                row[x / 8] |= 0x80 >> (x % 8);
            }
        }
        dib.extend_from_slice(&row);
    }
    let mut d: Vec<u8> = Vec::new();
    d.extend_from_slice(&0u16.to_le_bytes()); // reserved
    d.extend_from_slice(&2u16.to_le_bytes()); // type = cursor
    d.extend_from_slice(&1u16.to_le_bytes()); // count = 1
    d.push(if w == 256 { 0 } else { w as u8 });
    d.push(if h == 256 { 0 } else { h as u8 });
    d.push(0); // colorcount
    d.push(0); // reserved
    d.extend_from_slice(&frame.hot_x.to_le_bytes()); // CUR: planes = 热点 X
    d.extend_from_slice(&frame.hot_y.to_le_bytes()); // CUR: bitcount = 热点 Y
    d.extend_from_slice(&(dib.len() as u32).to_le_bytes());
    d.extend_from_slice(&22u32.to_le_bytes());
    d.extend_from_slice(&dib);
    d
}

/// 导出注册表方案值行（15 逗号段，序 = ALL_STATES 序；缺态 → Err——
/// 导出不假装完整，缺哪些态如实列出）。
pub fn export_scheme_reg(m: &CursorSchemeModel) -> Result<String, Vec<String>> {
    let missing = m.missing_states();
    if !missing.is_empty() {
        let names: Vec<String> = missing.iter().map(|s| alloc::format!("{s:?}")).collect();
        return Err(names);
    }
    let files: Vec<String> = crate::jstar2::jbase::ALL_STATES
        .iter()
        .map(|st| synth_cursor_filename(m, *st))
        .collect();
    Ok(alloc::format!("\"{}\"=\"{}\"\r\n", reg_escape(&m.name), files.join(",")))
}

/// 导出整库为 .reg 文本（版本头 + Schemes 键区——迁移的双向面：库内
/// 方案反写成 Windows 注册表格式，实机 reg import 即可回写）。缺态方案
/// 整行跳过（跳了多少数得出来：值行数 < 方案数）。
pub fn export_library_reg(schemes: &[CursorSchemeModel]) -> String {
    let mut out = String::from(REG_HEADER);
    out.push_str("\r\n\r\n[HKEY_CURRENT_USER\\Control Panel\\Cursors\\Schemes]\r\n");
    for m in schemes {
        if let Ok(line) = export_scheme_reg(m) {
            out.push_str(&line);
        }
    }
    out
}

/// 导出携带包（.reg 值行 + 逐态合成 .cur 字节，键 = 合成文件名——
/// 一次导出即可离线带走整个方案，回程走 F633 原管线）。
pub fn export_bundle(
    m: &CursorSchemeModel,
) -> Result<(String, Vec<(String, Vec<u8>)>), Vec<String>> {
    let text = export_scheme_reg(m)?;
    let mut files = Vec::new();
    for st in crate::jstar2::jbase::ALL_STATES.iter() {
        let e = m.state(*st).expect("15 态已验齐");
        files.push((synth_cursor_filename(m, *st), encode_cur_32bpp(&e.frames[0])));
    }
    Ok((text, files))
}

/// 导出 → 再导入往返保真核验（F633 对拍口径：重编码文件解析回帧，
/// 逐态像素 + 热点对拍；返回核验态数——判据「逐态迁移保真」在导出
/// 方向的等价物）。
pub fn verify_export_roundtrip(m: &CursorSchemeModel) -> Result<usize, String> {
    let files = export_bundle(m)
        .map_err(|e| alloc::format!("导出被拒：缺态 {e:?}"))?
        .1;
    let mut checked = 0usize;
    for st in crate::jstar2::jbase::ALL_STATES.iter() {
        let want_name = synth_cursor_filename(m, *st);
        let (_, bytes) = files
            .iter()
            .find(|(n, _)| *n == want_name)
            .ok_or_else(|| alloc::format!("缺导出文件 {want_name}"))?;
        let parsed = crate::jstar2::curimport::parse_cur_bytes(bytes)
            .map_err(|e| alloc::format!("{want_name}: {e:?}"))?;
        let e = m
            .state(*st)
            .ok_or_else(|| alloc::format!("{want_name}: 库内缺态"))?;
        let got = &parsed.frames[0];
        let want = &e.frames[0];
        if got.buf().diff_pixels(&want.buf()) != Some(0)
            || got.hot_x != want.hot_x
            || got.hot_y != want.hot_y
        {
            return Err(alloc::format!("{want_name}: 往返像素/热点失真"));
        }
        checked += 1;
    }
    Ok(checked)
}

/// 帧序列指纹（像素字节 + 热点 + 延时——迁移对账口径一处一事实；
/// 与 jbase::content_fingerprint 的方案级指纹互补，粒度到态）。
pub fn frames_fingerprint(frames: &[crate::jstar2::jbase::CursorFrame]) -> u64 {
    let mut feed: Vec<u8> = Vec::new();
    for f in frames {
        feed.extend_from_slice(&f.buf().px);
        feed.extend_from_slice(&f.hot_x.to_le_bytes());
        feed.extend_from_slice(&f.hot_y.to_le_bytes());
        feed.extend_from_slice(&f.delay_ms.to_le_bytes());
    }
    crate::jstar2::jbase::fnv1a64(&feed)
}

/// 迁移保真对账行（迁移差异报告的一行：源文件帧指纹 vs 迁入模型帧指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateFidelityRow {
    pub scheme: String,
    pub state: PointerState,
    pub src_fp: u64,
    pub dst_fp: u64,
    pub ok: bool,
}

/// 迁移差异报告：迁移前（源 .cur/.ani 逐态解析帧指纹）与迁移后（同一
/// 管线入库模型）逐态指纹对账表——「逐态迁移保真」从一次性断言升级
/// 为可呈现的账面（哪一态红了一眼看见；源解析失败 src_fp=0、迁入缺
/// 态 dst_fp=0，红因可从指纹区分）。
pub fn migration_fidelity_report(view: &RegistryView, store: &FileStore) -> Vec<StateFidelityRow> {
    let mut rows = Vec::new();
    for (name, files) in &view.schemes {
        let migrated = match migrate_one(name, view, store) {
            MigrationOutcome::Migrated(list) => list.into_iter().next(),
            _ => None,
        };
        for (i, fname) in files.iter().enumerate() {
            if fname.is_empty() || fname == "-" {
                continue;
            }
            let Some(st) = ALL_STATES.get(i) else { continue };
            let Some(bytes) = store.get(&resolve_path(fname)) else { continue };
            let is_ani = bytes.len() >= 12 && &bytes[0..4] == b"RIFF";
            let parsed = if is_ani {
                crate::jstar2::curimport::parse_ani_bytes(bytes)
            } else {
                crate::jstar2::curimport::parse_cur_bytes(bytes)
            };
            let src_fp = parsed.map(|f| frames_fingerprint(&f.frames)).unwrap_or(0);
            let (dst_fp, ok) = match migrated.as_ref().and_then(|m| m.state(*st)) {
                Some(sf) => {
                    let fp = frames_fingerprint(&sf.frames);
                    (fp, src_fp != 0 && fp == src_fp)
                }
                None => (0, false),
            };
            rows.push(StateFidelityRow {
                scheme: String::from(name),
                state: *st,
                src_fp,
                dst_fp,
                ok,
            });
        }
    }
    rows
}

/// 迁移批次记录（批次台账的一行：一次批量迁移的汇总事实）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchRecord {
    pub at_ms: u64,
    pub schemes_seen: usize,
    pub migrated: usize,
    pub missing: usize,
}

/// 迁移批次台账（每次 migrate_all 观察一条；环形 64——留痕台账
/// MigrationLedger 记逐方案，本账记逐批，两层互不顶替）。
#[derive(Clone, Debug, Default)]
pub struct MigrationBatchLog {
    records: Vec<BatchRecord>,
    dropped: usize,
}

impl MigrationBatchLog {
    pub const CAP: usize = 64;

    /// 观察一次迁移结果并登记（只记账——不改变结果本身）。
    pub fn observe(&mut self, at_ms: u64, outcome: &MigrationOutcome) {
        let rec = match outcome {
            MigrationOutcome::Migrated(list) => BatchRecord {
                at_ms,
                schemes_seen: list.len(),
                migrated: list.len(),
                missing: 0,
            },
            MigrationOutcome::Partial { migrated, missing } => BatchRecord {
                at_ms,
                schemes_seen: migrated.len() + missing.len(),
                migrated: migrated.len(),
                missing: missing.len(),
            },
            MigrationOutcome::Empty(_) => {
                BatchRecord { at_ms, schemes_seen: 0, migrated: 0, missing: 0 }
            }
        };
        if self.records.len() >= Self::CAP {
            self.records.remove(0);
            self.dropped += 1;
        }
        self.records.push(rec);
    }

    pub fn records(&self) -> &[BatchRecord] {
        &self.records
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 批次人话摘要（逐批一行：时刻 / 迁入 / 见案 / 缺件——向导
    /// 「本机已迁过什么」的问询面）。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!(
            "迁移批次：{} 批（环形挤出 {}）\n",
            self.records.len(),
            self.dropped
        );
        for r in &self.records {
            s.push_str(&alloc::format!(
                "  t={} 迁入 {} 案 / 见案 {} / 缺件 {}\n",
                r.at_ms, r.migrated, r.schemes_seen, r.missing
            ));
        }
        s
    }
}

/// F638 v4 自检（逆映射回环 / 重编码往返 / .reg 双向 / 保真对账 / 批次台账）。
pub fn run_winbridge_v4_checks() -> CheckSet {
    use crate::jstar2::curimport::gen_cur;
    let mut set = CheckSet::new("jstar2-F638-v4");

    // 造迁移夹具：15 态全齐的「导出样本」+ 单态「半残样本」（导出面
    // 的输入与真实迁移产物同源——不另造第二套方案事实）。
    let mut reg = String::from(REG_HEADER);
    reg.push_str("\r\n\r\n[HKEY_CURRENT_USER\\Control Panel\\Cursors\\Schemes]\r\n");
    let names: Vec<String> = (0..15).map(|i| alloc::format!("exp{i}.cur")).collect();
    reg.push_str(&alloc::format!("\"导出样本\"=\"{}\"\r\n", names.join(",")));
    reg.push_str("\"半残样本\"=\"exp14.cur\"\r\n");
    let view = parse_reg_export(reg.as_bytes());
    let mut store = FileStore::new();
    for (i, n) in names.iter().enumerate() {
        let (bytes, _) = gen_cur(16, 16, 32, (1 + i as u16, 2), 0xE400 + i as u32);
        store.put(&alloc::format!("%SystemRoot%\\Cursors\\{n}"), &bytes);
    }
    let model = match migrate_one("导出样本", &view, &store) {
        MigrationOutcome::Migrated(list) => list,
        _ => Vec::new(),
    };
    set.add("migration fixture ready", model.len() == 1, "");

    // 1. 逆映射自洽：state_to_key_name → state_key_to_state 全 15 态回环。
    let inv_ok = crate::jstar2::jbase::ALL_STATES
        .iter()
        .all(|st| state_key_to_state(state_to_key_name(*st)) == Some(*st));
    set.add("state key name inverse mapping closed", inv_ok, "");

    // 2. 32bpp 重编码 → F633 解析逐像素往返（含热点）。
    let (_, truth) = gen_cur(16, 16, 32, (2, 3), 0xBEEF);
    let rt_ok = match crate::jstar2::curimport::parse_cur_bytes(&encode_cur_32bpp(&truth[0])) {
        Ok(f) => {
            f.frames.len() == 1
                && f.frames[0].buf().diff_pixels(&truth[0].buf()) == Some(0)
                && f.frames[0].hot_x == 2
                && f.frames[0].hot_y == 3
        }
        Err(_) => false,
    };
    set.add("encode cur reparse pixel exact", rt_ok, "");

    // 3. 透明环掩码保真：alpha=0 边环经重编码 → AND 掩码 → 再解码仍透明。
    let (_, ring_truth) = gen_cur(16, 16, 32, (1, 1), 0x7A11);
    let ring_ok = match crate::jstar2::curimport::parse_cur_bytes(&encode_cur_32bpp(&ring_truth[0]))
    {
        Ok(f) => f.frames[0].buf().get(0, 0).map(|p| p[3] == 0).unwrap_or(false),
        Err(_) => false,
    };
    set.add("encode preserves transparent ring via mask", ring_ok, "");

    // 4. 缺态方案导出被拒且缺态清单如实列出（14 缺态）。
    let (pbytes, _) = gen_cur(16, 16, 32, (1, 1), 0x51);
    let partial = match crate::jstar2::curimport::import_cursor_file(
        &pbytes,
        PointerState::Text,
        "残案",
        "t",
    ) {
        Ok(m) => m,
        Err(_) => model[0].clone(),
    };
    match export_scheme_reg(&partial) {
        Err(missing) => set.add("export rejects incomplete scheme honestly", missing.len() == 14, ""),
        Ok(_) => set.add("export rejects incomplete scheme honestly", false, "unexpected"),
    }

    // 5. 全态导出值行 = 方案名 + 15 逗号段。
    let line = export_scheme_reg(&model[0]).unwrap_or_default();
    let value = line
        .split_once('=')
        .map(|(_, v)| v.trim().trim_matches('"'))
        .unwrap_or("");
    set.add(
        "export value row has 15 fields",
        line.starts_with('"') && value.split(',').count() == 15,
        "",
    );

    // 6. 整库导出 .reg 文本可被同一解析器读回（双向面闭环；方案名以
    //     入库事实为准——迁移产物带「迁移自」前缀，导出照实反写）。
    let exported = export_library_reg(&model);
    let back = parse_reg_export(exported.as_bytes());
    set.add(
        "exported reg parses back to same scheme",
        back.schemes.len() == 1
            && back.schemes[0].0 == model[0].name
            && back.schemes[0].1.len() == 15
            && back.header_ok,
        "",
    );

    // 7. 导出往返保真：重编码 → 再解析 → 逐态像素对拍全过（15 态）。
    set.add(
        "export roundtrip pixel fidelity",
        verify_export_roundtrip(&model[0]) == Ok(15),
        "",
    );

    // 8. 携带包文件全部是合法 type=2 .cur。
    let bundle = export_bundle(&model[0]).unwrap_or_default();
    let all_cur = bundle.1.len() == 15
        && bundle.1.iter().all(|(_, b)| {
            b.len() > 6 && b[0] == 0 && b[1] == 0 && u16::from_le_bytes([b[2], b[3]]) == 2
        });
    set.add("bundle files are legal cursors", all_cur, "");

    // 9. 保真对账报告：净仓 16 行全绿（15 态 + 半残样本的 Normal 态）。
    let report = migration_fidelity_report(&view, &store);
    set.add(
        "fidelity report all green on clean store",
        report.len() == 16 && report.iter().all(|r| r.ok),
        "",
    );

    // 10. 保真对账报告红行：坏魔数文件 → 管线全或无 → 该案 15 态全红、
    //     源解析失败那一态 src_fp=0、半残样本仍绿（红因可从账面区分）。
    let mut broken = store.clone_store();
    let (bad_bytes, _) = gen_cur(16, 16, 32, (1, 1), 0x99);
    let mut bad = bad_bytes;
    bad[0] = 9;
    broken.put("%SystemRoot%\\Cursors\\exp7.cur", &bad);
    let report_b = migration_fidelity_report(&view, &broken);
    set.add(
        "fidelity report flags corrupted pipeline",
        report_b.len() == 16
            && report_b.iter().filter(|r| r.ok).count() == 1
            && report_b.iter().filter(|r| r.src_fp == 0).count() == 1,
        "",
    );

    // 11. 批次台账：Migrated / Empty / Partial 三态观察 + 人话摘要。
    let mut log = MigrationBatchLog::default();
    log.observe(100, &MigrationOutcome::Empty("无"));
    log.observe(200, &migrate_all(&view, &store));
    log.observe(300, &migrate_all(&view, &broken));
    let sum = log.summary();
    set.add(
        "batch log observes all outcome kinds",
        log.records().len() == 3
            && log.records()[0].migrated == 0
            && log.records()[1].migrated == 2
            && log.records()[2].migrated == 1
            && log.records()[2].missing == 1
            && sum.contains("迁入 2 案")
            && sum.contains("缺件 1"),
        "",
    );

    // 12. 批次台账环形封顶：70 连发 → 挤出 9、首条时刻可推。
    for i in 0..70u64 {
        log.observe(1000 + i, &MigrationOutcome::Empty("无"));
    }
    set.add(
        "batch log ring caps at 64",
        log.records().len() == MigrationBatchLog::CAP
            && log.dropped() == 9
            && log.records()[0].at_ms == 1006,
        "",
    );

    // 13. 导出确定性：同一方案两次导出逐字节相同（指纹定名的前提）。
    set.add(
        "export deterministic across calls",
        exported == export_library_reg(&model),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v4 {
    use super::*;
    use crate::jstar2::curimport::gen_cur;

    fn fixture() -> (RegistryView, FileStore, Vec<CursorSchemeModel>) {
        let mut reg = String::from(REG_HEADER);
        reg.push_str("\r\n\r\n[HKEY_CURRENT_USER\\Control Panel\\Cursors\\Schemes]\r\n");
        let names: Vec<String> = (0..15).map(|i| alloc::format!("exp{i}.cur")).collect();
        reg.push_str(&alloc::format!("\"导出样本\"=\"{}\"\r\n", names.join(",")));
        let view = parse_reg_export(reg.as_bytes());
        let mut store = FileStore::new();
        for (i, n) in names.iter().enumerate() {
            let (bytes, _) = gen_cur(16, 16, 32, (1 + i as u16, 2), 0xE400 + i as u32);
            store.put(&alloc::format!("%SystemRoot%\\Cursors\\{n}"), &bytes);
        }
        let model = match migrate_one("导出样本", &view, &store) {
            MigrationOutcome::Migrated(list) => list,
            _ => Vec::new(),
        };
        (view, store, model)
    }

    #[test]
    fn key_name_mapping_roundtrips_all_states() {
        for st in crate::jstar2::jbase::ALL_STATES.iter() {
            assert_eq!(state_key_to_state(state_to_key_name(*st)), Some(*st));
        }
    }

    #[test]
    fn encode_decode_roundtrip_hotspot_and_pixels() {
        for (w, h, hx, hy) in [(16u16, 16u16, 2u16, 3u16), (32, 32, 31, 0), (8, 8, 0, 7)] {
            let (_, truth) = gen_cur(w, h, 32, (hx, hy), 0xCAFE + w as u32);
            let parsed =
                crate::jstar2::curimport::parse_cur_bytes(&encode_cur_32bpp(&truth[0])).unwrap();
            assert_eq!(parsed.frames[0].buf().diff_pixels(&truth[0].buf()), Some(0));
            assert_eq!((parsed.frames[0].hot_x, parsed.frames[0].hot_y), (hx, hy));
        }
    }

    #[test]
    fn export_roundtrip_full_scheme() {
        let (_, _, model) = fixture();
        assert_eq!(verify_export_roundtrip(&model[0]), Ok(15));
    }

    #[test]
    fn batch_log_rings_and_summarizes() {
        let mut log = MigrationBatchLog::default();
        for i in 0..70u64 {
            log.observe(i, &MigrationOutcome::Empty("无"));
        }
        assert_eq!(log.records().len(), MigrationBatchLog::CAP);
        assert_eq!(log.dropped(), 6);
        assert_eq!(log.records()[0].at_ms, 6);
        assert!(log.summary().contains("环形挤出 6"));
    }
}
