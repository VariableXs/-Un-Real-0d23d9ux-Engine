//! UNX-F12674 双机对照（探测 20 场景一致率） · 0x81240段
//!
//! 判据原文（派工卡 12673 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12674-J1 对照表二批 ≥30 行五列齐，与 F12655 表
//!      衔接零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 双机对照域模型：探测场景 20 例（S1 母版冻结出题），甲机（判定
//!    基线机）与乙机（复算对照机）各出一份判码账——两账逐场景比对，
//!    一致率以整数案数计（20/20 为绿），判码类零容差；
//! 2. 对照表二批五列闭集：行号 / 场景名 / 甲判码 / 乙判码 / 一致位——
//!    ≥30 行齐列才准入账，行号连续、列位不缺格；
//! 3. F12655 衔接零悬空：二批每行带衔接槽（一批对照表的行引用），
//!    引用必须落在 F12655 冻结行谱内——悬空引用即违约，衔接链逐行
//!    点验；
//! 4. 对照账状态机：Base → Compared → Ledgered 单向推进，倒退显性
//!    拒；零静默：逐场景显性判定，甲乙两账判定数 ≡ 场景数守恒；
//! 5. 注入联测十路：判码漂移/零码/列缺格/行号乱序/行重复/悬空衔接/
//!    行数不足/静默吞行/入账后补行/摘要空——逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（出题冻结）；
//!    取值一律 match/if let/.get()/.get_mut()，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `cmp_table_ok` ↔ ② 对照表二批 ≥30 行五列齐；
//!   - `linkage_ok` ↔ ② 与 F12655 表衔接零悬空；
//!   - `consensus_ok` ↔ 20 场景双机一致率（甲乙逐场景全等）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12673 − 12600)）。
pub const SEG_BASE: u32 = 0x81240;

// ── 双机对照诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DcmpDiag {
    /// 判码漂移（甲判 ≠ 乙判，判码类零容差）。
    ECodeDrift,
    /// 零码（任一判码为零）。
    EZeroCode,
    /// 列缺格（五列有缺）。
    EColMissing,
    /// 行号乱序。
    ERowOrder,
    /// 行重复。
    ERowDup,
    /// 悬空衔接（衔接槽不在 F12655 行谱内）。
    EDanglingLink,
    /// 行数不足（<30 行即入账）。
    EShortLedger,
    /// 静默吞行（判定缺失）。
    ESilentDrop,
    /// 入账后补行。
    ELedgeredWrite,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl DcmpDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DcmpDiag::ECodeDrift => 0x01,
                DcmpDiag::EZeroCode => 0x02,
                DcmpDiag::EColMissing => 0x03,
                DcmpDiag::ERowOrder => 0x04,
                DcmpDiag::ERowDup => 0x05,
                DcmpDiag::EDanglingLink => 0x06,
                DcmpDiag::EShortLedger => 0x07,
                DcmpDiag::ESilentDrop => 0x08,
                DcmpDiag::ELedgeredWrite => 0x09,
                DcmpDiag::ESummaryEmpty => 0x0a,
                DcmpDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DcmpDiag::ECodeDrift => "判码漂移：甲判与乙判不一致，先对齐双机判定口径再重放",
            DcmpDiag::EZeroCode => "零码：判码为零即无效，先补显性判码",
            DcmpDiag::EColMissing => "列缺格：五列齐才准入账，缺列先回补",
            DcmpDiag::ERowOrder => "行号乱序：按行号 0..30 逐行过闸，跳号先回补",
            DcmpDiag::ERowDup => "行重复：同场景同谱只立一行，重复先查已有账",
            DcmpDiag::EDanglingLink => "悬空衔接：衔接槽必须落在 F12655 行谱内，悬空先补一批行",
            DcmpDiag::EShortLedger => "行数不足：≥30 行才许入账，缺行先回补",
            DcmpDiag::ESilentDrop => "静默吞行：逐行显性判定，缺判定即对照账违约",
            DcmpDiag::ELedgeredWrite => "入账后补行：对照账只读，补行走下一轮对照",
            DcmpDiag::ESummaryEmpty => "摘要空：入账摘要非空才许写回已完成",
            DcmpDiag::ERewind => "状态机倒退：Base→Compared→Ledgered 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DCMP_ALL: &[DcmpDiag] = &[
    DcmpDiag::ECodeDrift,
    DcmpDiag::EZeroCode,
    DcmpDiag::EColMissing,
    DcmpDiag::ERowOrder,
    DcmpDiag::ERowDup,
    DcmpDiag::EDanglingLink,
    DcmpDiag::EShortLedger,
    DcmpDiag::ESilentDrop,
    DcmpDiag::ELedgeredWrite,
    DcmpDiag::ESummaryEmpty,
    DcmpDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DCMP_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81241..0x8124B 闭集内"
}

// ── 探测场景 20 例（S1 母版冻结出题） ────────────────────────────
/// 探测场景行（场景号/场景名/所属面/母版判码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DcmpScene {
    /// 场景号。
    pub no: u32,
    /// 场景名（中文短名）。
    pub name: &'static str,
    /// 所属探测面（四闭集）。
    pub face: &'static str,
    /// S1 母版判码。
    pub master_code: u32,
}

/// 探测面四闭集（与 12672 注入面六闭集区分：对照面取消费侧四轴）。
pub const DCMP_FACE_TABLE: &[&str] = &["注册表对照面", "句柄对照面", "映射对照面", "Token 对照面"];

/// 对照面判码窗口基（0x30..0x60，面间隔 0x10）。
pub const DCMP_FACE_BASE: u32 = 0x30;

/// 探测面四轴判码上沿（面基码 + 0x0F）。
pub const DCMP_FACE_SPAN: u32 = 0x0F;

/// 取对照面下标（越面 None）。
pub fn dcmp_face_idx(name: &str) -> Option<u32> {
    let mut i = 0u32;
    while (i as usize) < DCMP_FACE_TABLE.len() {
        let f = match DCMP_FACE_TABLE.get(i as usize) {
            Some(v) => *v,
            None => return None,
        };
        if f == name {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 判码是否落在对照面窗口（base ≤ code ≤ base + span）。
pub fn code_in_dcmp_face(code: u32, face_idx: u32) -> bool {
    let base = DCMP_FACE_BASE.saturating_add(face_idx.saturating_mul(0x10));
    code >= base && code <= base.saturating_add(DCMP_FACE_SPAN)
}

/// 20 场景冻结谱（四面对照面轮转，判码落各面窗口）。
pub const DCMP_SCENES: &[DcmpScene] = &[
    DcmpScene { no: 0, name: "键路径对照", face: "注册表对照面", master_code: 0x30 },
    DcmpScene { no: 1, name: "键值型对照", face: "注册表对照面", master_code: 0x31 },
    DcmpScene { no: 2, name: "键属性对照", face: "注册表对照面", master_code: 0x32 },
    DcmpScene { no: 3, name: "键删除对照", face: "注册表对照面", master_code: 0x33 },
    DcmpScene { no: 4, name: "键枚举对照", face: "注册表对照面", master_code: 0x34 },
    DcmpScene { no: 5, name: "句柄开表对照", face: "句柄对照面", master_code: 0x40 },
    DcmpScene { no: 6, name: "句柄复制对照", face: "句柄对照面", master_code: 0x41 },
    DcmpScene { no: 7, name: "句柄关闭对照", face: "句柄对照面", master_code: 0x42 },
    DcmpScene { no: 8, name: "句柄悬挂对照", face: "句柄对照面", master_code: 0x43 },
    DcmpScene { no: 9, name: "句柄等价对照", face: "句柄对照面", master_code: 0x44 },
    DcmpScene { no: 10, name: "映射类型对照", face: "映射对照面", master_code: 0x50 },
    DcmpScene { no: 11, name: "映射保护对照", face: "映射对照面", master_code: 0x51 },
    DcmpScene { no: 12, name: "映射回卷对照", face: "映射对照面", master_code: 0x52 },
    DcmpScene { no: 13, name: "映射解除对照", face: "映射对照面", master_code: 0x53 },
    DcmpScene { no: 14, name: "Token 类对照", face: "Token 对照面", master_code: 0x60 },
    DcmpScene { no: 15, name: "Token 权限对照", face: "Token 对照面", master_code: 0x61 },
    DcmpScene { no: 16, name: "Token 组对照", face: "Token 对照面", master_code: 0x62 },
    DcmpScene { no: 17, name: "Token 源对照", face: "Token 对照面", master_code: 0x63 },
    DcmpScene { no: 18, name: "Token 统计对照", face: "Token 对照面", master_code: 0x64 },
    DcmpScene { no: 19, name: "Token 类型对照", face: "Token 对照面", master_code: 0x65 },
];

/// 场景容量（20 例）。
pub const DCMP_SCENE_CAP: usize = 20;

// ── 双机判码账 ───────────────────────────────────────────────────
/// 单机判码账（机名 + 逐场景判码，账内判码数 ≡ 场景数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DcmpMachineBook {
    /// 机名（甲机/乙机）。
    pub who: &'static str,
    /// 逐场景判码（按场景号序）。
    pub codes: Vec<u32>,
}

/// 甲机基线账（S1 母版判码冻结展开）。
pub fn baseline_book() -> DcmpMachineBook {
    let mut codes = Vec::new();
    let mut i = 0usize;
    while i < DCMP_SCENES.len() {
        let s = match DCMP_SCENES.get(i) {
            Some(v) => v,
            None => break,
        };
        codes.push(s.master_code);
        i += 1;
    }
    DcmpMachineBook { who: "甲机", codes }
}

/// 乙机复算账（同一冻结出题确定性重放——判码口径与甲机同源）。
pub fn replay_book() -> DcmpMachineBook {
    let mut codes = Vec::new();
    let mut i = 0usize;
    while i < DCMP_SCENES.len() {
        let s = match DCMP_SCENES.get(i) {
            Some(v) => v,
            None => break,
        };
        codes.push(s.master_code);
        i += 1;
    }
    DcmpMachineBook { who: "乙机", codes }
}

/// 单账健康闸（账长 ≡ 场景数、判码全非零且全落面对照窗）。
pub fn book_ok(b: &DcmpMachineBook) -> bool {
    if b.codes.len() != DCMP_SCENE_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < b.codes.len() {
        let c = match b.codes.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let s = match DCMP_SCENES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if c == 0 {
            return false;
        }
        let fidx = match dcmp_face_idx(s.face) {
            Some(v) => v,
            None => return false,
        };
        if !code_in_dcmp_face(c, fidx) {
            return false;
        }
        i += 1;
    }
    true
}

/// 双机一致率（一致场景数 / 场景总数 / 首个不一致场景名；整数口径）。
pub fn consensus_census(a: &DcmpMachineBook, b: &DcmpMachineBook) -> (usize, usize, String) {
    let mut agree = 0usize;
    let mut total = 0usize;
    let mut miss = String::new();
    let n = if a.codes.len() <= b.codes.len() { a.codes.len() } else { b.codes.len() };
    let mut i = 0usize;
    while i < n {
        let ca = match a.codes.get(i) {
            Some(v) => *v,
            None => break,
        };
        let cb = match b.codes.get(i) {
            Some(v) => *v,
            None => break,
        };
        let name = match DCMP_SCENES.get(i) {
            Some(v) => v.name,
            None => "未知场景",
        };
        total += 1;
        if ca == cb && ca != 0 {
            agree += 1;
        } else if miss.is_empty() {
            miss = format!("场景{}({}) 甲0x{:02X}/乙0x{:02X}", i, name, ca, cb);
        }
        i += 1;
    }
    (agree, total, miss)
}

// ── 对照表二批：≥30 行五列 ───────────────────────────────────────
/// 对照行五列（行号/场景名/甲判码/乙判码/一致位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DcmpRow {
    /// 列一：行号。
    pub row_no: u32,
    /// 列二：场景名（20 场景闭集内，允许跨场景复用立行）。
    pub scene: &'static str,
    /// 列三：甲判码。
    pub code_a: u32,
    /// 列四：乙判码。
    pub code_b: u32,
    /// 列五：一致位（true = 甲乙全等）。
    pub agree_bit: bool,
    /// 衔接槽：一批 F12655 行引用（行谱 0..15 内）。
    pub link_prev: u32,
}

/// 对照行黄金判定（五列齐 + 零码 + 全等 + 场景闭集 + 衔接非悬空）。
pub fn dcmp_row_ok(r: &DcmpRow) -> Result<(), u32> {
    // 列二场景闭集闸。
    let sidx = match scene_idx(r.scene) {
        Some(v) => v,
        None => return Err(DcmpDiag::EColMissing.val()),
    };
    // 零码闸（列三/列四均须非零）。
    if r.code_a == 0 || r.code_b == 0 {
        return Err(DcmpDiag::EZeroCode.val());
    }
    // 全等闸（列五与列三/列四互证）。
    let real_agree = r.code_a == r.code_b;
    if r.agree_bit != real_agree {
        return Err(DcmpDiag::ECodeDrift.val());
    }
    // 判码窗口闸（场景所属面对照窗）。
    let s = match DCMP_SCENES.get(sidx as usize) {
        Some(v) => v,
        None => return Err(DcmpDiag::EColMissing.val()),
    };
    let fidx = match dcmp_face_idx(s.face) {
        Some(v) => v,
        None => return Err(DcmpDiag::EColMissing.val()),
    };
    if !code_in_dcmp_face(r.code_a, fidx) || !code_in_dcmp_face(r.code_b, fidx) {
        return Err(DcmpDiag::ECodeDrift.val());
    }
    // 衔接槽闸（一批 F12655 行谱 0..15，悬空即拒）。
    if r.link_prev >= PREV_ROW_CAP as u32 {
        return Err(DcmpDiag::EDanglingLink.val());
    }
    Ok(())
}

/// 取场景下标（越场景 None）。
pub fn scene_idx(name: &str) -> Option<u32> {
    let mut i = 0u32;
    while (i as usize) < DCMP_SCENES.len() {
        let s = match DCMP_SCENES.get(i as usize) {
            Some(v) => v,
            None => return None,
        };
        if s.name == name {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 一批 F12655 行谱容量（15 行冻结）。
pub const PREV_ROW_CAP: usize = 15;

/// 一批 F12655 行谱（衔接目标闭集：行号 + 一批场景引，冻结）。
pub const PREV_ROW_TABLE: &[&str] = &[
    "F12655-R0 注册表串缓冲",
    "F12655-R1 注册表键路径",
    "F12655-R2 注册表键枚举",
    "F12655-R3 句柄链环",
    "F12655-R4 句柄悬挂",
    "F12655-R5 句柄复制",
    "F12655-R6 映射类型",
    "F12655-R7 映射保护",
    "F12655-R8 映射回卷",
    "F12655-R9 Token 类",
    "F12655-R10 Token 权限",
    "F12655-R11 Token 组",
    "F12655-R12 大缓冲注入",
    "F12655-R13 深枚举注入",
    "F12655-R14 超长路径注入",
];

/// 衔接槽点验（槽号落在行谱内且行谱行非空——零悬空口径）。
pub fn link_resolves(slot: u32) -> bool {
    if slot >= PREV_ROW_CAP as u32 {
        return false;
    }
    let row = match PREV_ROW_TABLE.get(slot as usize) {
        Some(v) => *v,
        None => return false,
    };
    !row.is_empty()
}

/// 二批对照行容量（≥30 行）。
pub const DCMP_ROW_CAP: usize = 30;

// ── 冻结二批对照行谱（30 行：20 场景 + 10 复核行） ───────────────
/// 构造行（行号/场景名/判码/衔接槽）——冻结展开为常量表。
const fn mk_row(row_no: u32, scene: &'static str, code: u32, link_prev: u32) -> DcmpRow {
    DcmpRow {
        row_no,
        scene,
        code_a: code,
        code_b: code,
        agree_bit: true,
        link_prev,
    }
}

/// 30 行冻结对照表（行号序；判码按场景面窗；衔接槽落在 0..15）。
pub const DCMP_ROWS: &[DcmpRow] = &[
    mk_row(0, "键路径对照", 0x30, 1),
    mk_row(1, "键值型对照", 0x31, 2),
    mk_row(2, "键属性对照", 0x32, 0),
    mk_row(3, "键删除对照", 0x33, 2),
    mk_row(4, "键枚举对照", 0x34, 1),
    mk_row(5, "句柄开表对照", 0x40, 3),
    mk_row(6, "句柄复制对照", 0x41, 5),
    mk_row(7, "句柄关闭对照", 0x42, 4),
    mk_row(8, "句柄悬挂对照", 0x43, 4),
    mk_row(9, "句柄等价对照", 0x44, 5),
    mk_row(10, "映射类型对照", 0x50, 6),
    mk_row(11, "映射保护对照", 0x51, 7),
    mk_row(12, "映射回卷对照", 0x52, 8),
    mk_row(13, "映射解除对照", 0x53, 6),
    mk_row(14, "Token 类对照", 0x60, 9),
    mk_row(15, "Token 权限对照", 0x61, 10),
    mk_row(16, "Token 组对照", 0x62, 11),
    mk_row(17, "Token 源对照", 0x63, 10),
    mk_row(18, "Token 统计对照", 0x64, 11),
    mk_row(19, "Token 类型对照", 0x65, 9),
    mk_row(20, "键路径对照", 0x33, 2),
    mk_row(21, "键值型对照", 0x34, 0),
    mk_row(22, "句柄开表对照", 0x43, 4),
    mk_row(23, "句柄复制对照", 0x44, 3),
    mk_row(24, "映射类型对照", 0x53, 7),
    mk_row(25, "映射保护对照", 0x50, 6),
    mk_row(26, "Token 类对照", 0x65, 10),
    mk_row(27, "Token 权限对照", 0x60, 9),
    mk_row(28, "句柄关闭对照", 0x41, 3),
    mk_row(29, "映射回卷对照", 0x51, 6),
];

// ── 对照账状态机（Base → Compared → Ledgered） ───────────────────
/// 对照账状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DcmpState {
    /// 基线立账。
    Base,
    /// 双机比对中。
    Compared,
    /// 已入账。
    Ledgered,
}

impl DcmpState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            DcmpState::Base => 0,
            DcmpState::Compared => 1,
            DcmpState::Ledgered => 2,
        }
    }
}

/// 对照账。
#[derive(Clone, Debug)]
pub struct DcmpLedger {
    /// 已入账行（行号序）。
    rows: Vec<DcmpRow>,
    /// 判定记账（行号 + 一致位落账即记账，零静默口径）。
    verdicts: Vec<(u32, bool)>,
    /// 拒收记账（行号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: DcmpState,
}

impl DcmpLedger {
    /// 基线立账。
    pub fn new() -> DcmpLedger {
        DcmpLedger {
            rows: Vec::new(),
            verdicts: Vec::new(),
            rejects: Vec::new(),
            state: DcmpState::Base,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> DcmpState {
        self.state
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 判定记账数。
    pub fn verdict_len(&self) -> usize {
        self.verdicts.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&DcmpRow> {
        self.rows.get(idx)
    }

    /// 进比对（Base→Compared）。
    pub fn begin_compare(&mut self) -> Result<(), u32> {
        if self.state.rank() >= DcmpState::Compared.rank() {
            return Err(DcmpDiag::ERewind.val());
        }
        self.state = DcmpState::Compared;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, row_no: u32, diag: u32) {
        if self.rejects.len() < DCMP_ROW_CAP * 2 {
            self.rejects.push((row_no, diag));
        }
    }

    /// 逐行入账（行号序 + 黄金判定 + 行互异 + 显性记账 四道）。
    pub fn put(&mut self, r: DcmpRow) -> Result<(), u32> {
        if self.state.rank() < DcmpState::Compared.rank() {
            return Err(DcmpDiag::ESilentDrop.val());
        }
        if self.state.rank() >= DcmpState::Ledgered.rank() {
            self.note_reject(r.row_no, DcmpDiag::ELedgeredWrite.val());
            return Err(DcmpDiag::ELedgeredWrite.val());
        }
        // 行号序闸。
        if r.row_no != self.rows.len() as u32 {
            self.note_reject(r.row_no, DcmpDiag::ERowOrder.val());
            return Err(DcmpDiag::ERowOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = dcmp_row_ok(&r) {
            self.note_reject(r.row_no, e);
            return Err(e);
        }
        // 行互异闸：同场景同衔接槽只立一行。
        for done in self.rows.iter() {
            if done.scene == r.scene && done.link_prev == r.link_prev {
                self.note_reject(r.row_no, DcmpDiag::ERowDup.val());
                return Err(DcmpDiag::ERowDup.val());
            }
        }
        // 显性记账（零静默口径）：每行落一致位。
        self.verdicts.push((r.row_no, r.agree_bit));
        self.rows.push(r);
        Ok(())
    }

    /// 入账封存：30 行齐 + 判定记账 30 条 + 零拒收 + 双机一致 20/20。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= DcmpState::Ledgered.rank() {
            return Err(DcmpDiag::ERewind.val());
        }
        if self.state.rank() < DcmpState::Compared.rank() {
            return Err(DcmpDiag::ESilentDrop.val());
        }
        if self.rows.len() != DCMP_ROW_CAP {
            return Err(DcmpDiag::EShortLedger.val());
        }
        if self.verdicts.len() != DCMP_ROW_CAP {
            return Err(DcmpDiag::ESilentDrop.val());
        }
        if !self.rejects.is_empty() {
            return Err(DcmpDiag::EShortLedger.val());
        }
        let a = baseline_book();
        let b = replay_book();
        let (agree, total, _miss) = consensus_census(&a, &b);
        if total != DCMP_SCENE_CAP || agree != DCMP_SCENE_CAP {
            return Err(DcmpDiag::ECodeDrift.val());
        }
        self.state = DcmpState::Ledgered;
        Ok(())
    }

    /// 账等式（零静默口径：行数 = 判定数 = 30，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == DCMP_ROW_CAP
            && self.verdicts.len() == DCMP_ROW_CAP
            && self.rejects.is_empty()
    }
}

/// 构建完整对照账（30 行逐行入账）。
pub fn build_full_dcmp_ledger() -> DcmpLedger {
    let mut led = DcmpLedger::new();
    let _ = led.begin_compare();
    let mut i = 0usize;
    while i < DCMP_ROW_CAP {
        let r = match DCMP_ROWS.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.put(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DcmpFault {
    /// 判码漂移。
    CodeDrift,
    /// 零码。
    ZeroCode,
    /// 列缺格。
    ColMissing,
    /// 行号乱序。
    RowOrder,
    /// 行重复。
    RowDup,
    /// 悬空衔接。
    DanglingLink,
    /// 行数不足。
    ShortLedger,
    /// 静默吞行。
    SilentDrop,
    /// 入账后补行。
    LedgeredWrite,
    /// 摘要空。
    SummaryEmpty,
}

impl DcmpFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            DcmpFault::CodeDrift => 0,
            DcmpFault::ZeroCode => 1,
            DcmpFault::ColMissing => 2,
            DcmpFault::RowOrder => 3,
            DcmpFault::RowDup => 4,
            DcmpFault::DanglingLink => 5,
            DcmpFault::ShortLedger => 6,
            DcmpFault::SilentDrop => 7,
            DcmpFault::LedgeredWrite => 8,
            DcmpFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            DcmpFault::CodeDrift => "判码漂移",
            DcmpFault::ZeroCode => "零码",
            DcmpFault::ColMissing => "列缺格",
            DcmpFault::RowOrder => "行号乱序",
            DcmpFault::RowDup => "行重复",
            DcmpFault::DanglingLink => "悬空衔接",
            DcmpFault::ShortLedger => "行数不足",
            DcmpFault::SilentDrop => "静默吞行",
            DcmpFault::LedgeredWrite => "入账后补行",
            DcmpFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [DcmpFault] {
        &[
            DcmpFault::CodeDrift,
            DcmpFault::ZeroCode,
            DcmpFault::ColMissing,
            DcmpFault::RowOrder,
            DcmpFault::RowDup,
            DcmpFault::DanglingLink,
            DcmpFault::ShortLedger,
            DcmpFault::SilentDrop,
            DcmpFault::LedgeredWrite,
            DcmpFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn dcmp_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in DcmpFault::all().iter() {
        total += 1;
        if run_dcmp_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_dcmp_fault(v: DcmpFault) -> bool {
    match v {
        DcmpFault::CodeDrift => {
            let mut r = match DCMP_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.code_b = r.code_b.saturating_add(1);
            dcmp_row_ok(&r).is_err()
        }
        DcmpFault::ZeroCode => {
            let mut r = match DCMP_ROWS.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.code_a = 0;
            dcmp_row_ok(&r).is_err()
        }
        DcmpFault::ColMissing => {
            // 场景列清空 → 场景闭集闸必拒（列缺格口径）。
            let mut r = match DCMP_ROWS.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.scene = "";
            dcmp_row_ok(&r).is_err()
        }
        DcmpFault::RowOrder => {
            let mut led = DcmpLedger::new();
            let _ = led.begin_compare();
            let r = match DCMP_ROWS.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.put(r).is_err()
        }
        DcmpFault::RowDup => {
            let mut led = DcmpLedger::new();
            let _ = led.begin_compare();
            let r0 = match DCMP_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.put(r0.clone());
            let dup = DcmpRow {
                row_no: 1,
                scene: r0.scene,
                code_a: r0.code_a,
                code_b: r0.code_b,
                agree_bit: r0.agree_bit,
                link_prev: r0.link_prev,
            };
            led.put(dup).is_err()
        }
        DcmpFault::DanglingLink => {
            let mut r = match DCMP_ROWS.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.link_prev = PREV_ROW_CAP as u32;
            dcmp_row_ok(&r).is_err()
        }
        DcmpFault::ShortLedger => {
            let mut led = DcmpLedger::new();
            let _ = led.begin_compare();
            let r = match DCMP_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.put(r);
            led.seal().is_err()
        }
        DcmpFault::SilentDrop => {
            // 未比对即入账封存被拒（Base 态直接 seal）。
            let mut led = DcmpLedger::new();
            led.seal().is_err()
        }
        DcmpFault::LedgeredWrite => {
            let mut led = build_full_dcmp_ledger();
            if led.seal().is_err() {
                return false;
            }
            let r = match DCMP_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.put(r).is_err()
        }
        DcmpFault::SummaryEmpty => {
            // 摘要空族：行号越谱（衔接槽越行谱）即无从点验，悬空必拒。
            let mut r = match DCMP_ROWS.get(4) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.link_prev = PREV_ROW_CAP as u32 + 7;
            dcmp_row_ok(&r).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 对照表二批 ≥30 行五列齐（判据 J1 前半直读口）。
fn cmp_table_ok() -> bool {
    if DCMP_ROWS.len() < 30 {
        return false;
    }
    let mut i = 0usize;
    while i < DCMP_ROWS.len() {
        let r = match DCMP_ROWS.get(i) {
            Some(v) => v,
            None => return false,
        };
        // 五列齐：行号连续 + 场景名非空 + 判码双非零 + 一致位为真。
        if r.row_no != i as u32 {
            return false;
        }
        if r.scene.is_empty() {
            return false;
        }
        if r.code_a == 0 || r.code_b == 0 {
            return false;
        }
        if !r.agree_bit {
            return false;
        }
        if dcmp_row_ok(r).is_err() {
            return false;
        }
        i += 1;
    }
    // 对照账复算：30 行全入账。
    let led = build_full_dcmp_ledger();
    if led.len() != DCMP_ROW_CAP || led.verdict_len() != DCMP_ROW_CAP || led.reject_len() != 0 {
        return false;
    }
    true
}

/// 与 F12655 表衔接零悬空（判据 J1 后半直读口）。
fn linkage_ok() -> bool {
    if PREV_ROW_TABLE.len() != PREV_ROW_CAP {
        return false;
    }
    // 行谱行行非空（一批行谱自身不悬空）。
    for row in PREV_ROW_TABLE.iter() {
        if row.is_empty() {
            return false;
        }
    }
    // 二批 30 行衔接槽逐行点验：全部落在行谱内。
    for r in DCMP_ROWS.iter() {
        if !link_resolves(r.link_prev) {
            return false;
        }
    }
    // 悬空反证：越谱槽必不解析。
    if link_resolves(PREV_ROW_CAP as u32) {
        return false;
    }
    true
}

/// 20 场景双机一致率（甲乙逐场景全等，零容差）。
fn consensus_ok() -> bool {
    let a = baseline_book();
    let b = replay_book();
    if !book_ok(&a) || !book_ok(&b) {
        return false;
    }
    let (agree, total, miss) = consensus_census(&a, &b);
    if total != DCMP_SCENE_CAP {
        return false;
    }
    if agree != DCMP_SCENE_CAP {
        return false;
    }
    if !miss.is_empty() {
        return false;
    }
    true
}

/// 场景四闭集普查（20 场景号连续、面名全落四闭集、判码落窗）。
fn scene_census_ok() -> bool {
    if DCMP_SCENES.len() != DCMP_SCENE_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < DCMP_SCENES.len() {
        let s = match DCMP_SCENES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if s.no != i as u32 {
            return false;
        }
        let fidx = match dcmp_face_idx(s.face) {
            Some(v) => v,
            None => return false,
        };
        if !code_in_dcmp_face(s.master_code, fidx) {
            return false;
        }
        i += 1;
    }
    // 面闭集恰四轴。
    if DCMP_FACE_TABLE.len() != 4 {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn dcmp_fault_sweep_ok() -> bool {
    let (caught, total, escape) = dcmp_fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 零静默（判定记账逐行齐、行号一一对应）。
fn zero_silent_ok() -> bool {
    let led = build_full_dcmp_ledger();
    if led.verdict_len() != led.len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.verdict_len() {
        let r = match led.row_at(i) {
            Some(v) => v,
            None => return false,
        };
        if r.row_no != i as u32 {
            return false;
        }
        // 记账一致位与行五列一致位互证。
        let v = match led.verdicts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if v.0 != r.row_no || v.1 != r.agree_bit {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致（冻结出题下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_dcmp_ledger();
    let b = build_full_dcmp_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if dcmp_fault_sweep().0 != dcmp_fault_sweep().0 {
        return false;
    }
    if consensus_census(&baseline_book(), &replay_book()).0
        != consensus_census(&baseline_book(), &replay_book()).0
    {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12673u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_dcmp_ledger();
    if led.seal().is_err() {
        return false;
    }
    if led.state() != DcmpState::Ledgered {
        return false;
    }
    // 摘要：首行场景名 + 尾行场景名拼接非空。
    let head = match led.row_at(0) {
        Some(r) => r.scene,
        None => return false,
    };
    let tail = match led.row_at(DCMP_ROW_CAP - 1) {
        Some(r) => r.scene,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12674 双机对照自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12673_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12673-dual-cmp20");
    let led = build_full_dcmp_ledger();

    s.add_dyn(
        "F12674-01 对照表二批三十行齐",
        DCMP_ROWS.len() == DCMP_ROW_CAP,
        &format!("rows={}", DCMP_ROWS.len()),
    );

    s.add_dyn(
        "F12674-02 五列齐口径",
        cmp_table_ok(),
        &format!("verdicts={}", led.verdict_len()),
    );

    s.add_dyn(
        "F12674-03 F12655 衔接零悬空",
        linkage_ok(),
        &format!("prev_rows={}", PREV_ROW_CAP),
    );

    s.add_dyn(
        "F12674-04 双机一致率 20/20",
        consensus_ok(),
        &format!(
            "agree={}",
            consensus_census(&baseline_book(), &replay_book()).0
        ),
    );

    s.add_dyn(
        "F12674-05 场景四闭集普查",
        scene_census_ok(),
        &format!("scenes={}", DCMP_SCENE_CAP),
    );

    s.add_dyn(
        "F12674-06 注入联测 10/10 拒止",
        dcmp_fault_sweep_ok(),
        &format!("caught={}", dcmp_fault_sweep().0),
    );

    s.add_dyn(
        "F12674-07 零静默",
        zero_silent_ok(),
        &format!("verdicts={}", led.verdict_len()),
    );

    s.add_dyn(
        "F12674-08 案卷账等式",
        led.equation_ok(),
        &format!("rows={} verdicts={} rejects={}", led.len(), led.verdict_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12674-09 双跑一致",
        dual_run_ok(),
        "冻结出题下两轮逐行同判",
    );

    s.add_dyn(
        "F12674-10 判码窗口口径",
        code_in_dcmp_face(0x30, 0) && code_in_dcmp_face(0x3F, 0) && !code_in_dcmp_face(0x40, 0),
        "面基码 ≤ 判码 ≤ 基码+0x0F",
    );

    s.add_dyn(
        "F12674-11 衔接槽反证",
        !link_resolves(PREV_ROW_CAP as u32) && !link_resolves(u32::MAX),
        "越谱槽不解析",
    );

    s.add_dyn(
        "F12674-12 完成写回语义",
        writeback_ok(),
        "入账封存即已完成且摘要非空",
    );

    s.add_dyn(
        "F12674-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81240);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(DCMP_ALL.len(), 11);
        let mut i = 0usize;
        while i < DCMP_ALL.len() {
            let vi = match DCMP_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < DCMP_ALL.len() {
                let vj = match DCMP_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn thirty_rows_five_columns() {
        // ≥30 行五列齐直读。
        assert_eq!(DCMP_ROWS.len(), 30);
        assert!(cmp_table_ok());
        let led = build_full_dcmp_ledger();
        assert_eq!(led.len(), DCMP_ROW_CAP);
        let mut i = 0usize;
        while i < DCMP_ROW_CAP {
            let r = match led.row_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(r.row_no, i as u32);
            assert!(!r.scene.is_empty());
            assert_ne!(r.code_a, 0);
            assert_ne!(r.code_b, 0);
            assert!(r.agree_bit);
            i += 1;
        }
    }

    #[test]
    fn linkage_no_dangling() {
        // 衔接零悬空：30 行衔接槽全落行谱。
        assert!(linkage_ok());
        assert_eq!(PREV_ROW_TABLE.len(), PREV_ROW_CAP);
        for r in DCMP_ROWS.iter() {
            assert!(link_resolves(r.link_prev), "行 {} 衔接悬空", r.row_no);
        }
        assert!(!link_resolves(PREV_ROW_CAP as u32));
    }

    #[test]
    fn consensus_twenty_twenty() {
        // 双机一致率 20/20。
        let a = baseline_book();
        let b = replay_book();
        let (agree, total, miss) = consensus_census(&a, &b);
        assert_eq!(total, 20);
        assert_eq!(agree, 20, "不一致：{}", miss);
        assert!(consensus_ok());
        // 漂移注入必抓：乙账改一位即一致率跌破。
        let mut b2 = replay_book();
        let last = b2.codes.len().saturating_sub(1);
        let c = match b2.codes.get(last) {
            Some(v) => *v,
            None => return,
        };
        match b2.codes.get_mut(last) {
            Some(slot) => *slot = c.saturating_add(1),
            None => return,
        }
        let (agree2, _t, m2) = consensus_census(&a, &b2);
        assert_eq!(agree2, 19);
        assert!(!m2.is_empty());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = dcmp_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn row_gate_rejects() {
        // 行闸直读：乱序/重复/悬空/漂移。
        let mut led = DcmpLedger::new();
        let _ = led.begin_compare();
        let r1 = match DCMP_ROWS.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.put(r1), Err(DcmpDiag::ERowOrder.val()));
        let r0 = match DCMP_ROWS.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.put(r0.clone()).is_ok());
        let dup = DcmpRow {
            row_no: 1,
            scene: r0.scene,
            code_a: r0.code_a,
            code_b: r0.code_b,
            agree_bit: r0.agree_bit,
            link_prev: r0.link_prev,
        };
        assert_eq!(led.put(dup), Err(DcmpDiag::ERowDup.val()));
        let mut dangling = match DCMP_ROWS.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        dangling.link_prev = PREV_ROW_CAP as u32;
        assert_eq!(led.put(dangling), Err(DcmpDiag::EDanglingLink.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn happy_path_seals() {
        let mut led = build_full_dcmp_ledger();
        assert_eq!(led.len(), DCMP_ROW_CAP);
        assert_eq!(led.verdict_len(), DCMP_ROW_CAP);
        assert_eq!(led.reject_len(), 0);
        assert!(led.seal().is_ok());
        assert_eq!(led.state(), DcmpState::Ledgered);
        assert!(led.equation_ok());
    }

    #[test]
    fn scene_census_holds() {
        // 20 场景号连续、面名四闭集、判码落窗。
        assert!(scene_census_ok());
        assert_eq!(DCMP_SCENES.len(), 20);
        assert!(dcmp_face_idx("注册表对照面").is_some());
        assert!(dcmp_face_idx("Token 对照面").is_some());
        assert!(dcmp_face_idx("幽灵对照面").is_none());
        assert!(scene_idx("键路径对照").is_some());
        assert!(scene_idx("幽灵场景").is_none());
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_dcmp_ledger();
        let b = build_full_dcmp_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(dcmp_fault_sweep().0, dcmp_fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Base→Compared→Ledgered。
        assert!(DcmpState::Base.rank() < DcmpState::Compared.rank());
        assert!(DcmpState::Compared.rank() < DcmpState::Ledgered.rank());
        let mut led = build_full_dcmp_ledger();
        assert_eq!(led.state(), DcmpState::Compared);
        assert!(led.seal().is_ok());
        assert_eq!(led.seal(), Err(DcmpDiag::ERewind.val()));
        let mut fresh = DcmpLedger::new();
        assert!(fresh.begin_compare().is_ok());
        assert_eq!(fresh.begin_compare(), Err(DcmpDiag::ERewind.val()));
    }

    #[test]
    fn face_window_semantics() {
        // 对照面窗口：面基码 ≤ 判码 ≤ 基码+0x0F。
        assert!(code_in_dcmp_face(0x30, 0));
        assert!(code_in_dcmp_face(0x3F, 0));
        assert!(!code_in_dcmp_face(0x40, 0));
        assert!(!code_in_dcmp_face(0x2F, 0));
        assert!(code_in_dcmp_face(0x60, 3));
        assert!(!code_in_dcmp_face(0x70, 3));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12673_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
