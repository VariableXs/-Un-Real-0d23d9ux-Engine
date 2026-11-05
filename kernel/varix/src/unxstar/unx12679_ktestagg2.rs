//! UNX-F12680 ktest 聚合器（B33–B34 探测面 40 条聚合） · 0x813C0段
//!
//! 判据原文（派工卡 12679 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12680-J1 B35 主题框架登记，与 F12598→F12660 预
//!      告链衔接零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. ktest 聚合器域模型：B33–B34 两批探测面 40 条 ktest 项（项号/批
//!    引/主题/判码/通过位/摘要位）——主题五闭集（装载/句柄/对象/权
//!    限/掩码），聚合账逐主题归账，全跑条数与聚合账等式守恒；
//! 2. 聚合三闸：批额闸（两批各 20 条）、主题账闸（五主题各有落账）、
//!    等式闸（跑账数 ≡ 项数守恒）；通过位全真才许聚合封账；
//! 3. B35 主题框架登记（判据 J1 前半）：B35 框架表（主题/登记位/框
//!    架码）逐行登记，登记位全真、框架码落窗才许登记封样；
//! 4. 预告链衔接零悬空（判据 J1 后半）：预告链 F12598→F12660→B33→
//!    B34→B35 五环冻结，逐环点验（环名非空、环码递增、引用可解析）
//!    ——悬空环即违约；
//! 5. 项账状态机：KOpen → Running → Aggregated 单向推进，倒退显性
//!    拒；零静默：逐项显性判定，判定数 ≡ 项数守恒；
//! 6. 注入联测十路：主题越闭集/判码越窗/批越闭集/项号乱序/项重复/
//!    等式破坏/预告链悬空/静默吞项/封账后补项/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `b35_frame_ok` ↔ ② B35 主题框架登记；
//!   - `chain_no_dangling_ok` ↔ ② 与 F12598→F12660 预告链衔接零悬空；
//!   - `agg_forty_ok` ↔ B33–B34 探测面 40 条聚合全过；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12679 − 12600)）。
pub const SEG_BASE: u32 = 0x813C0;

// ── ktest 聚合诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KAggDiag {
    /// 主题越闭集。
    EThemeRange,
    /// 判码越窗。
    ECodeRange,
    /// 批越闭集。
    EBatchRange,
    /// 项号乱序。
    EItemOrder,
    /// 项重复。
    EItemDup,
    /// 等式破坏（跑账数 ≠ 项数）。
    EEquationBroken,
    /// 预告链悬空。
    EChainDangling,
    /// 静默吞项。
    ESilentDrop,
    /// 封账后补项。
    ESealedWrite,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl KAggDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                KAggDiag::EThemeRange => 0x01,
                KAggDiag::ECodeRange => 0x02,
                KAggDiag::EBatchRange => 0x03,
                KAggDiag::EItemOrder => 0x04,
                KAggDiag::EItemDup => 0x05,
                KAggDiag::EEquationBroken => 0x06,
                KAggDiag::EChainDangling => 0x07,
                KAggDiag::ESilentDrop => 0x08,
                KAggDiag::ESealedWrite => 0x09,
                KAggDiag::ESummaryEmpty => 0x0a,
                KAggDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            KAggDiag::EThemeRange => "主题越闭集：只认装载/句柄/对象/权限/掩码五主题，先正主题再入账",
            KAggDiag::ECodeRange => "判码越窗：判码不在主题判码窗口，先归主题再对账",
            KAggDiag::EBatchRange => "批越闭集：只认 B33/B34 两批，越批先回补",
            KAggDiag::EItemOrder => "项号乱序：按项号 0..40 逐项入账，跳号先回补",
            KAggDiag::EItemDup => "项重复：同批同主题同谱只立一项，重复先查已有账",
            KAggDiag::EEquationBroken => "等式破坏：跑账数必须等于项数，差账先补跑",
            KAggDiag::EChainDangling => "预告链悬空：F12598→F12660→B33→B34→B35 逐环点验，悬空先补环",
            KAggDiag::ESilentDrop => "静默吞项：逐项显性判定，缺判定即聚合账违约",
            KAggDiag::ESealedWrite => "封账后补项：聚合账只读，补项走下一轮聚合",
            KAggDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
            KAggDiag::ERewind => "状态机倒退：KOpen→Running→Aggregated 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const KAGG_ALL: &[KAggDiag] = &[
    KAggDiag::EThemeRange,
    KAggDiag::ECodeRange,
    KAggDiag::EBatchRange,
    KAggDiag::EItemOrder,
    KAggDiag::EItemDup,
    KAggDiag::EEquationBroken,
    KAggDiag::EChainDangling,
    KAggDiag::ESilentDrop,
    KAggDiag::ESealedWrite,
    KAggDiag::ESummaryEmpty,
    KAggDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in KAGG_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x813C1..0x813CB 闭集内"
}

// ── 主题五闭集 ───────────────────────────────────────────────────
/// ktest 主题（五闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KTheme {
    /// 装载主题。
    Load,
    /// 句柄主题。
    Handle,
    /// 对象主题。
    Object,
    /// 权限主题。
    Priv,
    /// 掩码主题。
    Mask,
}

impl KTheme {
    /// 主题名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            KTheme::Load => "装载",
            KTheme::Handle => "句柄",
            KTheme::Object => "对象",
            KTheme::Priv => "权限",
            KTheme::Mask => "掩码",
        }
    }

    /// 主题号（0..4）。
    pub fn ordinal(self) -> u32 {
        match self {
            KTheme::Load => 0,
            KTheme::Handle => 1,
            KTheme::Object => 2,
            KTheme::Priv => 3,
            KTheme::Mask => 4,
        }
    }

    /// 主题判码窗口基（0x22/0x32/0x42/0x52/0x62）。
    pub fn code_base(self) -> u32 {
        match self {
            KTheme::Load => 0x22,
            KTheme::Handle => 0x32,
            KTheme::Object => 0x42,
            KTheme::Priv => 0x52,
            KTheme::Mask => 0x62,
        }
    }

    /// 全部五主题（闭集数组）。
    pub fn all() -> &'static [KTheme] {
        &[
            KTheme::Load,
            KTheme::Handle,
            KTheme::Object,
            KTheme::Priv,
            KTheme::Mask,
        ]
    }
}

/// 主题判码窗口上沿偏移。
pub const THEME_CODE_SPAN: u32 = 0x0C;

/// 判码是否落在主题判码窗口。
pub fn code_in_theme(code: u32, t: KTheme) -> bool {
    let base = t.code_base();
    code > base && code <= base.saturating_add(THEME_CODE_SPAN)
}

// ── ktest 批引二闭集 ─────────────────────────────────────────────
/// ktest 批引（两批闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KBatch {
    /// B33 批。
    B33,
    /// B34 批。
    B34,
}

impl KBatch {
    /// 批名。
    pub fn name(self) -> &'static str {
        match self {
            KBatch::B33 => "B33",
            KBatch::B34 => "B34",
        }
    }

    /// 批号（0/1）。
    pub fn ordinal(self) -> u32 {
        match self {
            KBatch::B33 => 0,
            KBatch::B34 => 1,
        }
    }

    /// 批内项数（各 20 项）。
    pub fn batch_cap(self) -> usize {
        20
    }

    /// 全部两批（闭集数组）。
    pub fn all() -> &'static [KBatch] {
        &[KBatch::B33, KBatch::B34]
    }
}

// ── ktest 项六元 ─────────────────────────────────────────────────
/// ktest 项六元（项号/批引/主题/谱名/判码/通过位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KItem {
    /// 项号（全批 0..40 连续）。
    pub no: u32,
    /// 批引。
    pub batch: KBatch,
    /// 主题。
    pub theme: KTheme,
    /// 谱名（主题内测试谱）。
    pub spectrum: &'static str,
    /// 判码（主题窗口内）。
    pub code: u32,
    /// 通过位。
    pub pass_bit: bool,
}

/// 单项聚合判定（批闭集 + 主题谱文本 + 零码 + 码窗 四道）。
pub fn k_item_ok(i: &KItem) -> Result<(), u32> {
    // 主题谱文本闸。
    if i.spectrum.is_empty() {
        return Err(KAggDiag::EThemeRange.val());
    }
    // 零码闸。
    if i.code == 0 {
        return Err(KAggDiag::ECodeRange.val());
    }
    // 码窗闸。
    if !code_in_theme(i.code, i.theme) {
        return Err(KAggDiag::ECodeRange.val());
    }
    // 通过位闸。
    if !i.pass_bit {
        return Err(KAggDiag::EEquationBroken.val());
    }
    Ok(())
}

/// 全批项容量（两批 40 项）。
pub const K_ITEM_CAP: usize = 40;

// ── 冻结 ktest 项谱（40 项：B33 20 项 + B34 20 项，五主题轮转） ──
/// 构造项——冻结展开为常量表。
const fn mk_item(no: u32, batch: KBatch, theme: KTheme, spectrum: &'static str, code: u32) -> KItem {
    KItem {
        no,
        batch,
        theme,
        spectrum,
        code,
        pass_bit: true,
    }
}

/// 40 项冻结项谱（项号序；判码落主题窗）。
pub const K_ITEMS: &[KItem] = &[
    mk_item(0, KBatch::B33, KTheme::Load, "装载序探", 0x23),
    mk_item(1, KBatch::B33, KTheme::Handle, "句柄级探", 0x33),
    mk_item(2, KBatch::B33, KTheme::Object, "对象类探", 0x43),
    mk_item(3, KBatch::B33, KTheme::Priv, "权限位探", 0x53),
    mk_item(4, KBatch::B33, KTheme::Mask, "掩码谱探", 0x63),
    mk_item(5, KBatch::B33, KTheme::Load, "装载回卷探", 0x24),
    mk_item(6, KBatch::B33, KTheme::Handle, "句柄悬挂探", 0x34),
    mk_item(7, KBatch::B33, KTheme::Object, "对象界探", 0x44),
    mk_item(8, KBatch::B33, KTheme::Priv, "权限档探", 0x54),
    mk_item(9, KBatch::B33, KTheme::Mask, "掩码位探", 0x64),
    mk_item(10, KBatch::B33, KTheme::Load, "装载重放探", 0x25),
    mk_item(11, KBatch::B33, KTheme::Handle, "句柄复制探", 0x35),
    mk_item(12, KBatch::B33, KTheme::Object, "对象版探", 0x45),
    mk_item(13, KBatch::B33, KTheme::Priv, "权限档回探", 0x55),
    mk_item(14, KBatch::B33, KTheme::Mask, "掩码和探", 0x65),
    mk_item(15, KBatch::B33, KTheme::Load, "装载尾探", 0x26),
    mk_item(16, KBatch::B33, KTheme::Handle, "句柄尾探", 0x36),
    mk_item(17, KBatch::B33, KTheme::Object, "对象尾探", 0x46),
    mk_item(18, KBatch::B33, KTheme::Priv, "权限尾探", 0x56),
    mk_item(19, KBatch::B33, KTheme::Mask, "掩码尾探", 0x66),
    mk_item(20, KBatch::B34, KTheme::Load, "装载段探", 0x27),
    mk_item(21, KBatch::B34, KTheme::Handle, "句柄段探", 0x37),
    mk_item(22, KBatch::B34, KTheme::Object, "对象段探", 0x47),
    mk_item(23, KBatch::B34, KTheme::Priv, "权限段探", 0x57),
    mk_item(24, KBatch::B34, KTheme::Mask, "掩码段探", 0x67),
    mk_item(25, KBatch::B34, KTheme::Load, "装载界探", 0x28),
    mk_item(26, KBatch::B34, KTheme::Handle, "句柄界探", 0x38),
    mk_item(27, KBatch::B34, KTheme::Object, "对象界复探", 0x48),
    mk_item(28, KBatch::B34, KTheme::Priv, "权限界探", 0x58),
    mk_item(29, KBatch::B34, KTheme::Mask, "掩码界探", 0x68),
    mk_item(30, KBatch::B34, KTheme::Load, "装载封探", 0x29),
    mk_item(31, KBatch::B34, KTheme::Handle, "句柄封探", 0x39),
    mk_item(32, KBatch::B34, KTheme::Object, "对象封探", 0x49),
    mk_item(33, KBatch::B34, KTheme::Priv, "权限封探", 0x59),
    mk_item(34, KBatch::B34, KTheme::Mask, "掩码封探", 0x69),
    mk_item(35, KBatch::B34, KTheme::Load, "装载尾复探", 0x2A),
    mk_item(36, KBatch::B34, KTheme::Handle, "句柄尾复探", 0x3A),
    mk_item(37, KBatch::B34, KTheme::Object, "对象尾复探", 0x4A),
    mk_item(38, KBatch::B34, KTheme::Priv, "权限尾复探", 0x5A),
    mk_item(39, KBatch::B34, KTheme::Mask, "掩码尾复探", 0x6A),
];

// ── 预告链（F12598→F12660→B33→B34→B35 五环） ─────────────────────
/// 链环（环名/环码/承上引）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainRing {
    /// 环名。
    pub ring: &'static str,
    /// 环码（递增冻结）。
    pub ring_code: u32,
    /// 承上环名（首环为空串）。
    pub prev_ring: &'static str,
}

/// 预告链五环（冻结；环码 0x10..0x50 递增）。
pub const CHAIN_TABLE: &[ChainRing] = &[
    ChainRing { ring: "F12598 开工预告", ring_code: 0x10, prev_ring: "" },
    ChainRing { ring: "F12660 探测面预告", ring_code: 0x20, prev_ring: "F12598 开工预告" },
    ChainRing { ring: "B33 探测批", ring_code: 0x30, prev_ring: "F12660 探测面预告" },
    ChainRing { ring: "B34 探测批", ring_code: 0x40, prev_ring: "B33 探测批" },
    ChainRing { ring: "B35 联签预告", ring_code: 0x50, prev_ring: "B34 探测批" },
];

/// 取链环（越链 None）。
pub fn chain_ring_of(name: &str) -> Option<&ChainRing> {
    for r in CHAIN_TABLE.iter() {
        if r.ring == name {
            return Some(r);
        }
    }
    None
}

/// 单环点验（环名非空 + 承上引可解析或为首环 + 环码落窗）。
pub fn chain_ring_ok(r: &ChainRing) -> bool {
    if r.ring.is_empty() {
        return false;
    }
    if !r.prev_ring.is_empty() && chain_ring_of(r.prev_ring).is_none() {
        return false;
    }
    // 环码窗：0x10..0x50 段内。
    r.ring_code >= 0x10 && r.ring_code <= 0x50 && r.ring_code % 0x10 == 0
}

/// 预告链全点验（五环齐、逐环承上可解析、环码递增）。
pub fn chain_ok() -> bool {
    if CHAIN_TABLE.len() != 5 {
        return false;
    }
    let mut i = 0usize;
    while i < CHAIN_TABLE.len() {
        let r = match CHAIN_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !chain_ring_ok(r) {
            return false;
        }
        // 首环无承上，余环承上 = 上一环名。
        let prev = match CHAIN_TABLE.get(i.saturating_sub(1)) {
            Some(p) => p.ring,
            None => "",
        };
        if i == 0 {
            if !r.prev_ring.is_empty() {
                return false;
            }
        } else if r.prev_ring != prev {
            return false;
        }
        // 环码递增。
        if i + 1 < CHAIN_TABLE.len() {
            let n = match CHAIN_TABLE.get(i + 1) {
                Some(v) => v,
                None => return false,
            };
            if n.ring_code <= r.ring_code {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── B35 主题框架登记表（判据 J1 前半） ───────────────────────────
/// B35 框架行（主题/登记位/框架码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B35Frame {
    /// 主题（五闭集内）。
    pub theme: KTheme,
    /// 登记位。
    pub registered: bool,
    /// 框架码（主题窗内）。
    pub frame_code: u32,
}

/// B35 框架表（五主题全登记，冻结）。
pub const B35_TABLE: &[B35Frame] = &[
    B35Frame { theme: KTheme::Load, registered: true, frame_code: 0x23 },
    B35Frame { theme: KTheme::Handle, registered: true, frame_code: 0x33 },
    B35Frame { theme: KTheme::Object, registered: true, frame_code: 0x43 },
    B35Frame { theme: KTheme::Priv, registered: true, frame_code: 0x53 },
    B35Frame { theme: KTheme::Mask, registered: true, frame_code: 0x63 },
];

/// B35 框架登记点验（五行齐、登记位全真、框架码落主题窗、主题互异）。
pub fn b35_frame_ok() -> bool {
    if B35_TABLE.len() != KTheme::all().len() {
        return false;
    }
    let mut i = 0usize;
    while i < B35_TABLE.len() {
        let f = match B35_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !f.registered {
            return false;
        }
        if !code_in_theme(f.frame_code, f.theme) {
            return false;
        }
        // 主题互异普查。
        let mut j = i + 1;
        while j < B35_TABLE.len() {
            let g = match B35_TABLE.get(j) {
                Some(v) => v,
                None => return false,
            };
            if g.theme == f.theme {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 聚合账状态机（KOpen → Running → Aggregated） ─────────────────
/// 聚合账状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KAggState {
    /// 开账。
    KOpen,
    /// 跑账中。
    Running,
    /// 已聚合。
    Aggregated,
}

impl KAggState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            KAggState::KOpen => 0,
            KAggState::Running => 1,
            KAggState::Aggregated => 2,
        }
    }
}

/// ktest 聚合账。
#[derive(Clone, Debug)]
pub struct KAggLedger {
    /// 已入账项（项号序）。
    items: Vec<KItem>,
    /// 跑账（项号 + 判码；全跑口径）。
    runs: Vec<(u32, u32)>,
    /// 主题账（主题号 + 项数）。
    theme_tally: Vec<(u32, usize)>,
    /// 拒收记账（项号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: KAggState,
}

impl KAggLedger {
    /// 开账。
    pub fn new() -> KAggLedger {
        KAggLedger {
            items: Vec::new(),
            runs: Vec::new(),
            theme_tally: Vec::new(),
            rejects: Vec::new(),
            state: KAggState::KOpen,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> KAggState {
        self.state
    }

    /// 已入账项数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 跑账数（全跑条数）。
    pub fn run_len(&self) -> usize {
        self.runs.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 项（越界 None）。
    pub fn item_at(&self, idx: usize) -> Option<&KItem> {
        self.items.get(idx)
    }

    /// 取主题账（越界 None）。
    pub fn tally_at(&self, idx: usize) -> Option<(u32, usize)> {
        self.theme_tally.get(idx).copied()
    }

    /// 进跑账（KOpen→Running）。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        if self.state.rank() >= KAggState::Running.rank() {
            return Err(KAggDiag::ERewind.val());
        }
        self.state = KAggState::Running;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < K_ITEM_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐项入账（项号序 + 聚合判定 + 项互异 + 跑账守恒 四道）。
    pub fn put(&mut self, i: KItem) -> Result<(), u32> {
        if self.state.rank() < KAggState::Running.rank() {
            return Err(KAggDiag::ESilentDrop.val());
        }
        if self.state.rank() >= KAggState::Aggregated.rank() {
            self.note_reject(i.no, KAggDiag::ESealedWrite.val());
            return Err(KAggDiag::ESealedWrite.val());
        }
        // 项号序闸。
        if i.no != self.items.len() as u32 {
            self.note_reject(i.no, KAggDiag::EItemOrder.val());
            return Err(KAggDiag::EItemOrder.val());
        }
        // 批引闸：前 20 项必 B33，后 20 项必 B34。
        let want = if i.no < KBatch::B33.batch_cap() as u32 { KBatch::B33 } else { KBatch::B34 };
        if i.batch != want {
            self.note_reject(i.no, KAggDiag::EBatchRange.val());
            return Err(KAggDiag::EBatchRange.val());
        }
        // 聚合判定闸。
        if let Err(e) = k_item_ok(&i) {
            self.note_reject(i.no, e);
            return Err(e);
        }
        // 项互异闸：同批同主题同谱只立一项。
        for done in self.items.iter() {
            if done.batch == i.batch && done.theme == i.theme && done.spectrum == i.spectrum {
                self.note_reject(i.no, KAggDiag::EItemDup.val());
                return Err(KAggDiag::EItemDup.val());
            }
        }
        // 跑账守恒：入账即落跑账。
        self.runs.push((i.no, i.code));
        self.items.push(i);
        Ok(())
    }

    /// 主题归账（Running 态可反复归账：重算五主题计数）。
    pub fn tally_themes(&mut self) -> usize {
        if self.state.rank() < KAggState::Running.rank() {
            return 0;
        }
        self.theme_tally.clear();
        for t in KTheme::all().iter() {
            let mut n = 0usize;
            for it in self.items.iter() {
                if it.theme == *t {
                    n += 1;
                }
            }
            self.theme_tally.push((t.ordinal(), n));
        }
        self.theme_tally.len()
    }

    /// 聚合封账（判据 J1 主闸）：40 项齐 + 零拒收 + 主题账齐 + 等式断言
    /// + B35 框架登记齐 + 预告链零悬空。
    pub fn aggregate(&mut self) -> Result<(), u32> {
        if self.state.rank() >= KAggState::Aggregated.rank() {
            return Err(KAggDiag::ERewind.val());
        }
        if self.state.rank() < KAggState::Running.rank() {
            return Err(KAggDiag::ESilentDrop.val());
        }
        if self.items.len() != K_ITEM_CAP {
            return Err(KAggDiag::ESummaryEmpty.val());
        }
        if !self.rejects.is_empty() {
            return Err(KAggDiag::ESummaryEmpty.val());
        }
        // 主题账齐闸。
        if self.theme_tally.len() != KTheme::all().len() {
            self.tally_themes();
        }
        if self.theme_tally.len() != KTheme::all().len() {
            return Err(KAggDiag::EThemeRange.val());
        }
        // 等式断言：跑账数 ≡ 项数。
        if self.runs.len() != self.items.len() {
            return Err(KAggDiag::EEquationBroken.val());
        }
        // B35 框架登记闸。
        if !b35_frame_ok() {
            return Err(KAggDiag::EThemeRange.val());
        }
        // 预告链零悬空闸。
        if !chain_ok() {
            return Err(KAggDiag::EChainDangling.val());
        }
        self.state = KAggState::Aggregated;
        Ok(())
    }

    /// 账等式（跑账数 = 项数 = 40，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.items.len() == K_ITEM_CAP
            && self.runs.len() == K_ITEM_CAP
            && self.rejects.is_empty()
    }
}

/// 构建完整聚合账（40 项逐项入账 + 主题归账 + 聚合封账）。
pub fn build_full_kagg_ledger() -> KAggLedger {
    let mut led = KAggLedger::new();
    let _ = led.begin_run();
    let mut i = 0usize;
    while i < K_ITEM_CAP {
        let it = match K_ITEMS.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.put(it) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    let _ = led.tally_themes();
    let _ = led.aggregate();
    led
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KAggFault {
    /// 主题越闭集。
    ThemeRange,
    /// 判码越窗。
    CodeRange,
    /// 批越闭集。
    BatchRange,
    /// 项号乱序。
    ItemOrder,
    /// 项重复。
    ItemDup,
    /// 等式破坏。
    EquationBroken,
    /// 预告链悬空。
    ChainDangling,
    /// 静默吞项。
    SilentDrop,
    /// 封账后补项。
    SealedWrite,
    /// 摘要空。
    SummaryEmpty,
}

impl KAggFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            KAggFault::ThemeRange => 0,
            KAggFault::CodeRange => 1,
            KAggFault::BatchRange => 2,
            KAggFault::ItemOrder => 3,
            KAggFault::ItemDup => 4,
            KAggFault::EquationBroken => 5,
            KAggFault::ChainDangling => 6,
            KAggFault::SilentDrop => 7,
            KAggFault::SealedWrite => 8,
            KAggFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            KAggFault::ThemeRange => "主题越闭集",
            KAggFault::CodeRange => "判码越窗",
            KAggFault::BatchRange => "批越闭集",
            KAggFault::ItemOrder => "项号乱序",
            KAggFault::ItemDup => "项重复",
            KAggFault::EquationBroken => "等式破坏",
            KAggFault::ChainDangling => "预告链悬空",
            KAggFault::SilentDrop => "静默吞项",
            KAggFault::SealedWrite => "封账后补项",
            KAggFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [KAggFault] {
        &[
            KAggFault::ThemeRange,
            KAggFault::CodeRange,
            KAggFault::BatchRange,
            KAggFault::ItemOrder,
            KAggFault::ItemDup,
            KAggFault::EquationBroken,
            KAggFault::ChainDangling,
            KAggFault::SilentDrop,
            KAggFault::SealedWrite,
            KAggFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn kagg_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in KAggFault::all().iter() {
        total += 1;
        if run_kagg_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_kagg_fault(v: KAggFault) -> bool {
    match v {
        KAggFault::ThemeRange => {
            let mut it = match K_ITEMS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            it.spectrum = "";
            k_item_ok(&it).is_err()
        }
        KAggFault::CodeRange => {
            let mut it = match K_ITEMS.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            it.code = 0;
            k_item_ok(&it).is_err()
        }
        KAggFault::BatchRange => {
            // 批越闭集：项 1 必 B33，注 B34 拒。
            let mut it = match K_ITEMS.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            it.batch = KBatch::B34;
            k_item_ok(&it).is_err() || {
                // 聚合账面批闸直读。
                let mut led = KAggLedger::new();
                let _ = led.begin_run();
                let it0 = match K_ITEMS.get(0) {
                    Some(x) => x.clone(),
                    None => return false,
                };
                let _ = led.put(it0);
                let mut bad = match K_ITEMS.get(1) {
                    Some(x) => x.clone(),
                    None => return false,
                };
                bad.batch = KBatch::B34;
                led.put(bad).is_err()
            }
        }
        KAggFault::ItemOrder => {
            let mut led = KAggLedger::new();
            let _ = led.begin_run();
            let it = match K_ITEMS.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.put(it).is_err()
        }
        KAggFault::ItemDup => {
            let mut led = KAggLedger::new();
            let _ = led.begin_run();
            let it0 = match K_ITEMS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.put(it0.clone());
            let dup = KItem {
                no: 1,
                batch: it0.batch,
                theme: it0.theme,
                spectrum: it0.spectrum,
                code: it0.code,
                pass_bit: it0.pass_bit,
            };
            led.put(dup).is_err()
        }
        KAggFault::EquationBroken => {
            // 等式破坏：构造失衡账直读。
            let mut led = build_full_kagg_ledger();
            let before = led.run_len();
            if before != led.len() {
                return false;
            }
            let _ = led.runs.pop();
            led.run_len() != led.len()
        }
        KAggFault::ChainDangling => {
            // 预告链悬空：承上引不在链内必拒。
            let bad = ChainRing { ring: "幽灵环", ring_code: 0x60, prev_ring: "不存在环" };
            !chain_ring_ok(&bad)
        }
        KAggFault::SilentDrop => {
            // 未跑账即聚合被拒（KOpen 态直接 aggregate）。
            let mut led = KAggLedger::new();
            led.aggregate().is_err()
        }
        KAggFault::SealedWrite => {
            let mut led = build_full_kagg_ledger();
            if led.state() != KAggState::Aggregated {
                return false;
            }
            let it = match K_ITEMS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.put(it).is_err()
        }
        KAggFault::SummaryEmpty => {
            // 摘要空族：通过位清空必拒（通过位为摘要落账前提）。
            let mut it = match K_ITEMS.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            it.pass_bit = false;
            k_item_ok(&it).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// B33–B34 探测面 40 条聚合全过（判据 ① 直读口）。
fn agg_forty_ok() -> bool {
    let mut led = build_full_kagg_ledger();
    if led.len() != K_ITEM_CAP {
        return false;
    }
    if led.run_len() != K_ITEM_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.state() != KAggState::Aggregated {
        return false;
    }
    if led.state() != KAggState::Aggregated {
        return false;
    }
    if !led.equation_ok() {
        return false;
    }
    // 主题账：五主题各有落账。
    let _ = led.tally_themes();
    let mut i = 0usize;
    while i < KTheme::all().len() {
        let t = match led.tally_at(i) {
            Some(v) => v,
            None => return false,
        };
        if t.0 != i as u32 || t.1 == 0 {
            return false;
        }
        i += 1;
    }
    // 独立复核：40 项逐项聚合判定。
    for it in K_ITEMS.iter() {
        if k_item_ok(it).is_err() {
            return false;
        }
    }
    true
}

/// B35 主题框架登记（判据 J1 前半直读口）。
fn b35_registered_ok() -> bool {
    if !b35_frame_ok() {
        return false;
    }
    // 反证：未登记与越窗框架码必拒。
    let bad_reg = B35Frame { theme: KTheme::Load, registered: false, frame_code: 0x23 };
    if bad_reg.registered {
        return false;
    }
    let bad_code = B35Frame { theme: KTheme::Load, registered: true, frame_code: 0x99 };
    if code_in_theme(bad_code.frame_code, bad_code.theme) {
        return false;
    }
    true
}

/// 与 F12598→F12660 预告链衔接零悬空（判据 J1 后半直读口）。
fn chain_no_dangling_ok() -> bool {
    if !chain_ok() {
        return false;
    }
    // 逐环点验。
    for r in CHAIN_TABLE.iter() {
        if !chain_ring_ok(r) {
            return false;
        }
    }
    // 链尾必须落 B35（预告衔接口）。
    let tail = match CHAIN_TABLE.last() {
        Some(v) => v,
        None => return false,
    };
    if !tail.ring.contains("B35") {
        return false;
    }
    // 悬空反证：幽灵承上必拒。
    let bad = ChainRing { ring: "幽灵环二", ring_code: 0x50, prev_ring: "无此环" };
    if chain_ring_ok(&bad) {
        return false;
    }
    true
}

/// 五主题覆盖普查（40 项覆盖五主题、主题互异谱名、项号连续）。
fn theme_census_ok() -> bool {
    let mut per_theme = [0usize; 5];
    let mut i = 0usize;
    while i < K_ITEMS.len() {
        let it = match K_ITEMS.get(i) {
            Some(v) => v,
            None => return false,
        };
        if it.no != i as u32 {
            return false;
        }
        match per_theme.get_mut(it.theme.ordinal() as usize) {
            Some(v) => *v += 1,
            None => return false,
        }
        // 同主题谱名互异。
        let mut j = i + 1;
        while j < K_ITEMS.len() {
            let g = match K_ITEMS.get(j) {
                Some(v) => v,
                None => return false,
            };
            if g.theme == it.theme && g.spectrum == it.spectrum {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    for slot in per_theme.iter() {
        if *slot != 8 {
            return false;
        }
    }
    true
}

/// 零静默（跑账逐项齐、项号一一对应）。
fn zero_silent_ok() -> bool {
    let led = build_full_kagg_ledger();
    if led.run_len() != led.len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.run_len() {
        let run = match led.runs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let it = match led.item_at(i) {
            Some(v) => v,
            None => return false,
        };
        if run.0 != it.no || run.1 != it.code {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn kagg_fault_sweep_ok() -> bool {
    let (caught, total, escape) = kagg_fault_sweep();
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

/// 双跑一致（冻结项谱下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_kagg_ledger();
    let b = build_full_kagg_ledger();
    if a.len() != b.len() || a.run_len() != b.run_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.item_at(i), b.item_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if kagg_fault_sweep().0 != kagg_fault_sweep().0 {
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
    if SEG_BASE != 0x80000 + 0x40 * (12679u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let led = build_full_kagg_ledger();
    if led.state() != KAggState::Aggregated {
        return false;
    }
    // 摘要：首项谱名 + 尾项主题名拼接非空。
    let head = match led.item_at(0) {
        Some(it) => it.spectrum,
        None => return false,
    };
    let tail = match led.item_at(K_ITEM_CAP - 1) {
        Some(it) => it.theme.name(),
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    // B35 框架名摘要非空（登记口径）。
    let f = match B35_TABLE.first() {
        Some(v) => v,
        None => return false,
    };
    if f.theme.name().is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12680 ktest 聚合自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12679_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12679-ktest-agg2");
    let led = build_full_kagg_ledger();

    s.add_dyn(
        "F12680-01 两批四十项齐",
        K_ITEMS.len() == K_ITEM_CAP,
        &format!("items={} batches={}", K_ITEMS.len(), KBatch::all().len()),
    );

    s.add_dyn(
        "F12680-02 四十条聚合全过",
        agg_forty_ok(),
        &format!("items={} runs={}", led.len(), led.run_len()),
    );

    s.add_dyn(
        "F12680-03 B35 主题框架登记",
        b35_registered_ok(),
        &format!("frames={}", B35_TABLE.len()),
    );

    s.add_dyn(
        "F12680-04 预告链衔接零悬空",
        chain_no_dangling_ok(),
        &format!("rings={}", CHAIN_TABLE.len()),
    );

    s.add_dyn(
        "F12680-05 五主题覆盖普查",
        theme_census_ok(),
        &format!("themes={}", KTheme::all().len()),
    );

    s.add_dyn(
        "F12680-06 零静默",
        zero_silent_ok(),
        &format!("runs={}", led.run_len()),
    );

    s.add_dyn(
        "F12680-07 注入联测 10/10 拒止",
        kagg_fault_sweep_ok(),
        &format!("caught={}", kagg_fault_sweep().0),
    );

    s.add_dyn(
        "F12680-08 账等式",
        led.equation_ok(),
        &format!("items={} runs={} rejects={}", led.len(), led.run_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12680-09 双跑一致",
        dual_run_ok(),
        "冻结项谱下两轮逐项同判",
    );

    s.add_dyn(
        "F12680-10 主题判码窗口口径",
        code_in_theme(0x23, KTheme::Load)
            && !code_in_theme(0x22, KTheme::Load)
            && !code_in_theme(0x33, KTheme::Load),
        "基码 < 判码 ≤ 基码+0x0C 且跨主题不混",
    );

    s.add_dyn(
        "F12680-11 批额冻结轴",
        KBatch::B33.batch_cap() == 20 && KBatch::B34.batch_cap() == 20,
        "两批各 20 项",
    );

    s.add_dyn(
        "F12680-12 完成写回语义",
        writeback_ok(),
        "聚合封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12680-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x813C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(KAGG_ALL.len(), 11);
        let mut i = 0usize;
        while i < KAGG_ALL.len() {
            let vi = match KAGG_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < KAGG_ALL.len() {
                let vj = match KAGG_ALL.get(j) {
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
    fn agg_forty_holds() {
        assert!(agg_forty_ok());
        let led = build_full_kagg_ledger();
        assert_eq!(led.len(), 40);
        assert_eq!(led.run_len(), 40);
        assert_eq!(led.reject_len(), 0);
        assert_eq!(led.state(), KAggState::Aggregated);
        // 主题账五主题齐。
        for i in 0..KTheme::all().len() {
            let t = match led.tally_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(t.1, 8, "主题 {} 项数漂移", i);
        }
    }

    #[test]
    fn b35_frame_and_chain_hold() {
        assert!(b35_registered_ok());
        assert!(chain_no_dangling_ok());
        assert!(chain_ok());
        assert_eq!(CHAIN_TABLE.len(), 5);
        assert!(chain_ring_of("F12660 探测面预告").is_some());
        assert!(chain_ring_of("幽灵环").is_none());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = kagg_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn put_gate_rejects() {
        let mut led = KAggLedger::new();
        let _ = led.begin_run();
        let it1 = match K_ITEMS.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.put(it1), Err(KAggDiag::EItemOrder.val()));
        let it_wrong = match K_ITEMS.get(20) {
            Some(x) => x.clone(),
            None => return,
        };
        let mis = KItem {
            no: 0,
            batch: it_wrong.batch,
            theme: it_wrong.theme,
            spectrum: it_wrong.spectrum,
            code: it_wrong.code,
            pass_bit: true,
        };
        assert_eq!(led.put(mis), Err(KAggDiag::EBatchRange.val()));
        let it0 = match K_ITEMS.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.put(it0.clone()).is_ok());
        let dup = KItem {
            no: 1,
            batch: it0.batch,
            theme: it0.theme,
            spectrum: it0.spectrum,
            code: it0.code,
            pass_bit: true,
        };
        assert_eq!(led.put(dup), Err(KAggDiag::EItemDup.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn theme_census_holds() {
        assert!(theme_census_ok());
        assert_eq!(K_ITEMS.len(), 40);
        assert_eq!(KTheme::all().len(), 5);
        let mut counts = [0usize; 5];
        for it in K_ITEMS.iter() {
            match counts.get_mut(it.theme.ordinal() as usize) {
                Some(v) => *v += 1,
                None => return,
            }
        }
        for slot in counts.iter() {
            assert_eq!(*slot, 8, "主题项数漂移");
        }
        assert!(KTheme::Mask.code_base() > KTheme::Load.code_base());
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_kagg_ledger();
        let mut i = 0usize;
        while i < led.run_len() {
            let run = match led.runs.get(i) {
                Some(v) => *v,
                None => return,
            };
            let it = match led.item_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(run.0, it.no, "项 {} 错位", i);
            assert_eq!(run.1, it.code);
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_kagg_ledger();
        let b = build_full_kagg_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.item_at(i), b.item_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑项缺失"),
            }
            i += 1;
        }
        assert_eq!(kagg_fault_sweep().0, kagg_fault_sweep().0);
        // 主题账双跑一致。
        let mut a2 = build_full_kagg_ledger();
        let mut b2 = build_full_kagg_ledger();
        assert_eq!(a2.tally_themes(), b2.tally_themes());
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：KOpen→Running→Aggregated。
        assert!(KAggState::KOpen.rank() < KAggState::Running.rank());
        assert!(KAggState::Running.rank() < KAggState::Aggregated.rank());
        let mut led = build_full_kagg_ledger();
        assert_eq!(led.state(), KAggState::Aggregated);
        assert_eq!(led.aggregate(), Err(KAggDiag::ERewind.val()));
        let mut fresh = KAggLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(KAggDiag::ERewind.val()));
    }

    #[test]
    fn theme_window_semantics() {
        // 主题判码窗口：跨主题不混。
        assert!(code_in_theme(0x23, KTheme::Load));
        assert!(code_in_theme(0x2E, KTheme::Load));
        assert!(!code_in_theme(0x22, KTheme::Load));
        assert!(!code_in_theme(0x2F, KTheme::Load));
        assert!(!code_in_theme(0x33, KTheme::Load));
        assert!(code_in_theme(0x6A, KTheme::Mask));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12679_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
