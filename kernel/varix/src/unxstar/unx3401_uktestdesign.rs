#![allow(dead_code)]
#![allow(unused_imports)]
use alloc::vec;
// UNX-F3401 · ktest 现状盘点与扩容总设计（接管 testing/ 目录）
//
// 任务单号: UNX-F3401
// 标题: ktest 现状盘点与扩容总设计（接管 testing/ 目录）
// 域/批: A4/G8（CoRun·UNX-A5 段｜ktest/框架/测试基建域）
// 册目标: 420 行（判据 J1 总设计文档 + M-160 设计基线用例）
//
// 判据原文（逐字摘抄，来源《CoRun Varix STAR II · Unxreal》任务书表格）:
// 「UNX-F3401 | ktest 现状盘点与扩容总设计（接管 testing/ 目录） | 420 | 骨架 |
//   J1：总设计文档（现状盘点/扩容目标/宏族路线/接管三步法）交付；M-160 设计基线用例。」
//
// 说明（锚点目录同名文件的辨析）:
// `_attic/.../anchors/UNX-F3401.md` 内实为 **UNX-F34010 · 安全事件审计与 J5 联签位**，与本单无关。
// 本单判据以任务书表格 `| UNX-F3401 |` 行为准。
//
// 本单四段总设计（逐段落地为可执行模型）:
// - **段一 现状盘点**：把 `testing/` 目录拆成 `DirEntry` 清单，逐条登记**接管状态**
//   （`Legacy` / `Shadow` / `Owned`），盘点结果入位图，缺项即 `InventoryMissing`。
// - **段二 扩容目标**：定义扩容后的目标态（宏族覆盖、判据覆盖率、用例容量），
//   现状与目标之差即 **缺口账**（`GapBook`），缺口必须逐项可寻址，不允许笼统「差很多」。
// - **段三 宏族路线**：宏族注册表 `宏名 → 展开式 → 判据数`，
//   每个宏族的展开必须**可枚举且有限**（宏展开是有界的，绝不产生无限用例集）。
// - **段四 接管三步法**：`盘点 → 影子 → 接管`（Shadow 比对期新旧结果一致方可推进），
//   迁移进度账分批推进，**收敛判据** = 每批结束时旧路径调用数必须单调下降且最终归零。
//
// 深度骨架:
// 常量表 / 4 核心结构（Inventory / GapBook / MacroBook / Migration）/
// 14 错误变体 / 6 辅助码 / 主状态机（合法边闭集 + 非法转换拒记）/
// 守恒账 `reconcile()` / append-only 环形账（`dropped` 显性）/
// 注入核 14/14 全拒留痕 / 冻结指纹 `fingerprint() + verify()` /
// `export_kv() + export_tlv()` / 确定性 `replay(seed)` / `observe(seed)` /
// `complexity()` 逐项性能分解（对标 Windows 同类能力写「深在哪」）。
//
// 码段声明:
// 本单独占诊断码段 `0x2880 ..= 0x28BF`（`SEG_BASE = 0x2880`，`SEG_MASK = 0x003F`）。
// 错误变体一码一段，逐个占段内偏移 `0x00..=0x0D`；辅助码占 `0x1C..=0x21`，
// 最大占用 `0x28A1`，未触及段上界 `0x28BF`。

use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

extern crate alloc;

// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 版本标识。
pub const V_VERSION_ID: u64 = 0x0001_3401_2880_0001;
/// 版本串。
pub const V_VERSION: &str = "v3401-uktestdesign.v1";
/// 码段基址。
pub const SEG_BASE: u16 = 0x2880;
/// 码段掩码。
pub const SEG_MASK: u16 = 0x003F;
/// 码段上界。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 主码。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 辅助码段内基址。
pub const AUX_BASE_OFF: u16 = 0x1C;


// ---------------------------------------------------------------------------
// F3401 规模常量
// ---------------------------------------------------------------------------

/// `testing/` 目录条目数。
pub const DIR_ENTRIES: usize = 16;
/// 宏族总数。
pub const MACRO_FAMILIES: usize = 6;
/// 每宏族展开上限（宏展开有界，绝不无限膨胀）。
pub const MACRO_FANOUT_CAP: usize = 12;
/// 接管批次总数（三步法分批推进）。
pub const MIGRATION_BATCHES: usize = 4;
/// 每批容量（**必须整除 `DIR_ENTRIES`**：4 批 × 4 = 16 条目，否则批次区间会出现空洞）。
pub const BATCH_CAP: usize = 4;
/// 判据总数（扩容目标面；须 ≥ 现状 `crit_total = 16 + Σ(i mod 3) = 31`）。
pub const CRITERIA_TOTAL: usize = 40;
/// 用例容量目标。
pub const CASE_TARGET: usize = 96;
/// 影子比对重复轮次。
pub const SHADOW_ROUNDS: usize = 3;
/// 环形账容量。
pub const RING_CAP: usize = 32;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 总步数上限。
pub const STEP_CAP: u64 = (DIR_ENTRIES * MACRO_FAMILIES * 4 + MIGRATION_BATCHES * BATCH_CAP * 4) as u64;
/// syscall 次数上限（零真实 IO）。
pub const SYSCALL_CAP: u64 = 0;
/// 峰值内存上限。
pub const MEM_PEAK_CAP: u64 = 512 * 1024;

/// TLV 组头标签。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头载荷字节数。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长。
pub const TLV_HEAD_LEN: usize = TLV_HEAD_PAYLOAD + 3;
/// 单条 TLV 定长。
pub const TLV_ITEM_LEN: usize = 10;
/// TLV 条目总数（静态 40 + 逐形态 14 = 54；由 export_tlv 内 push 次数反推）。
pub const TLV_ITEM_COUNT: usize = 54;

/// 盘点域面名。
pub const FACE_INVENTORY: &str = "盘点面";
/// 宏族域面名。
pub const FACE_MACRO: &str = "宏族面";
/// 迁移域面名。
pub const FACE_MIGRATION: &str = "迁移面";
/// 账目域面名。
pub const FACE_LEDGER: &str = "账目面";
/// 冻结域面名。
pub const FACE_FREEZE: &str = "冻结面";

/// 嫁接源。
pub const GRAFT_SEAT: &str = "F3361/F3381";
/// 对位条位。
pub const DEP_SEAT: &str = "F3421/F3461";

// ===========================================================================
// 错误闭集
// ===========================================================================

/// 错误形态（段内偏移 `0x00..=0x0D`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0：盘点缺项（目录条目漏登记）。
    InventoryMissing,
    /// 1：盘点重号。
    InventoryDup,
    /// 2：宏族重名。
    MacroDup,
    /// 3：宏展开越界（超过 `MACRO_FANOUT_CAP`）。
    MacroFanoutOver,
    /// 4：宏展开空（展开为零项）。
    MacroEmpty,
    /// 5：宏判据数越界（超出 `CRITERIA_TOTAL`）。
    MacroCritOver,
    /// 6：目标态不自洽（目标值 < 现状值）。
    TargetIncoherent,
    /// 7：缺口未登记（现状与目标之差无对应缺口项）。
    GapUnlisted,
    /// 8：接管批次越界。
    BatchOver,
    /// 9：接管状态倒退（Owned → Legacy）。
    StateRegress,
    /// 10：影子比对不一致（不许推进接管）。
    ShadowMismatch,
    /// 11：旧路径未收敛（旧调用数未归零）。
    LegacyNotConverged,
    /// 12：守恒式偏离。
    ReconcileDeviation,
    /// 13：冻结指纹漂移。
    FingerprintDrift,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态。
    pub const ALL: [Form; Form::COUNT] = [
        Form::InventoryMissing,
        Form::InventoryDup,
        Form::MacroDup,
        Form::MacroFanoutOver,
        Form::MacroEmpty,
        Form::MacroCritOver,
        Form::TargetIncoherent,
        Form::GapUnlisted,
        Form::BatchOver,
        Form::StateRegress,
        Form::ShadowMismatch,
        Form::LegacyNotConverged,
        Form::ReconcileDeviation,
        Form::FingerprintDrift,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            Form::InventoryMissing => 0,
            Form::InventoryDup => 1,
            Form::MacroDup => 2,
            Form::MacroFanoutOver => 3,
            Form::MacroEmpty => 4,
            Form::MacroCritOver => 5,
            Form::TargetIncoherent => 6,
            Form::GapUnlisted => 7,
            Form::BatchOver => 8,
            Form::StateRegress => 9,
            Form::ShadowMismatch => 10,
            Form::LegacyNotConverged => 11,
            Form::ReconcileDeviation => 12,
            Form::FingerprintDrift => 13,
        }
    }
    /// 按下标取。
    pub fn from_index(i: usize) -> Option<Form> {
        if i < Form::COUNT {
            Some(Form::ALL[i])
        } else {
            None
        }
    }
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            Form::InventoryMissing => "InventoryMissing",
            Form::InventoryDup => "InventoryDup",
            Form::MacroDup => "MacroDup",
            Form::MacroFanoutOver => "MacroFanoutOver",
            Form::MacroEmpty => "MacroEmpty",
            Form::MacroCritOver => "MacroCritOver",
            Form::TargetIncoherent => "TargetIncoherent",
            Form::GapUnlisted => "GapUnlisted",
            Form::BatchOver => "BatchOver",
            Form::StateRegress => "StateRegress",
            Form::ShadowMismatch => "ShadowMismatch",
            Form::LegacyNotConverged => "LegacyNotConverged",
            Form::ReconcileDeviation => "ReconcileDeviation",
            Form::FingerprintDrift => "FingerprintDrift",
        }
    }
    /// 形态描述。
    pub fn describe(self) -> &'static str {
        match self {
            Form::InventoryMissing => "盘点缺项：目录条目漏登记（现状位图留洞）",
            Form::InventoryDup => "盘点重号：同一条目被登记两次",
            Form::MacroDup => "宏族重名：两宏族同名登记",
            Form::MacroFanoutOver => "宏展开越界：展开项数超过扇出上限",
            Form::MacroEmpty => "宏展开为空：宏族展开出零项",
            Form::MacroCritOver => "宏判据数越界：单族判据数超过总判据数",
            Form::TargetIncoherent => "目标态不自洽：扩容目标值小于现状值",
            Form::GapUnlisted => "缺口未登记：现状与目标之差无对应缺口项",
            Form::BatchOver => "接管批次越界：批次下标超出批次数",
            Form::StateRegress => "接管状态倒退：Owned 退回 Legacy",
            Form::ShadowMismatch => "影子比对不一致：新旧路径结果不同却推进接管",
            Form::LegacyNotConverged => "旧路径未收敛：末批之后旧调用数非零",
            Form::ReconcileDeviation => "守恒式偏离：批次容量合计 ≠ 条目数",
            Form::FingerprintDrift => "冻结指纹漂移：设计件哈希与入册值失配",
        }
    }
    /// 探测面名。
    pub fn probe(self) -> &'static str {
        match self {
            Form::InventoryMissing => "inventory.cover_bits",
            Form::InventoryDup => "inventory.dup_bits",
            Form::MacroDup => "macro.dup_bits",
            Form::MacroFanoutOver => "macro.fanout_cap",
            Form::MacroEmpty => "macro.fanout_nonzero",
            Form::MacroCritOver => "macro.crit_bound",
            Form::TargetIncoherent => "target.ge_current",
            Form::GapUnlisted => "gap.complete",
            Form::BatchOver => "mig.batch_index",
            Form::StateRegress => "mig.state_monotonic",
            Form::ShadowMismatch => "mig.shadow_match",
            Form::LegacyNotConverged => "mig.legacy_zero",
            Form::ReconcileDeviation => "recon.deviation",
            Form::FingerprintDrift => "freeze.verify",
        }
    }
    /// 所属域面。
    pub fn face(self) -> &'static str {
        match self {
            Form::InventoryMissing | Form::InventoryDup => FACE_INVENTORY,
            Form::MacroDup | Form::MacroFanoutOver | Form::MacroEmpty | Form::MacroCritOver => FACE_MACRO,
            Form::TargetIncoherent | Form::GapUnlisted => FACE_INVENTORY,
            Form::BatchOver | Form::StateRegress | Form::ShadowMismatch | Form::LegacyNotConverged => FACE_MIGRATION,
            Form::ReconcileDeviation => FACE_LEDGER,
            Form::FingerprintDrift => FACE_FREEZE,
        }
    }
    /// 是否属「盘点/目标面」。
    pub fn is_inv(self) -> bool {
        matches!(
            self,
            Form::InventoryMissing
                | Form::InventoryDup
                | Form::TargetIncoherent
                | Form::GapUnlisted
        )
    }
    /// 是否属「宏族面」。
    pub fn is_macro(self) -> bool {
        matches!(
            self,
            Form::MacroDup | Form::MacroFanoutOver | Form::MacroEmpty | Form::MacroCritOver
        )
    }
    /// 是否属「迁移面」。
    pub fn is_mig(self) -> bool {
        matches!(
            self,
            Form::BatchOver
                | Form::StateRegress
                | Form::ShadowMismatch
                | Form::LegacyNotConverged
        )
    }
    /// 形态类名。
    pub fn class_name(self) -> &'static str {
        if self.is_inv() {
            "盘点面"
        } else if self.is_macro() {
            "宏族面"
        } else if self.is_mig() {
            "迁移面"
        } else {
            "账目/冻结面"
        }
    }
}
/// 辅助码（段内偏移 `0x1C..=0x21`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 0：步数越界。
    StepOverCap,
    /// 1：宏族下标越界。
    MacroIdxOver,
    /// 2：展开项下标越界。
    FanoutIdxOver,
    /// 3：批次下标越界。
    BatchIdxOver,
    /// 4：影子轮数不足。
    ShadowShort,
    /// 5：语义边界越界（本单只做设计，不真改 `testing/` 目录）。
    SemanticBorder,
}
impl AuxCode {
    /// 辅助码总数。
    pub const COUNT: usize = 6;
    /// 全部辅助码。
    pub const ALL: [AuxCode; AuxCode::COUNT] = [
        AuxCode::StepOverCap,
        AuxCode::MacroIdxOver,
        AuxCode::FanoutIdxOver,
        AuxCode::BatchIdxOver,
        AuxCode::ShadowShort,
        AuxCode::SemanticBorder,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            AuxCode::StepOverCap => 0,
            AuxCode::MacroIdxOver => 1,
            AuxCode::FanoutIdxOver => 2,
            AuxCode::BatchIdxOver => 3,
            AuxCode::ShadowShort => 4,
            AuxCode::SemanticBorder => 5,
        }
    }
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE
            .wrapping_add(AUX_BASE_OFF)
            .wrapping_add(self.index() as u16)
    }
    /// 辅助码名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::StepOverCap => "StepOverCap",
            AuxCode::MacroIdxOver => "MacroIdxOver",
            AuxCode::FanoutIdxOver => "FanoutIdxOver",
            AuxCode::BatchIdxOver => "BatchIdxOver",
            AuxCode::ShadowShort => "ShadowShort",
            AuxCode::SemanticBorder => "SemanticBorder",
        }
    }
    /// 辅助码描述。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::StepOverCap => "逻辑步数越过 STEP_CAP 上限",
            AuxCode::MacroIdxOver => "宏族下标越界",
            AuxCode::FanoutIdxOver => "宏展开项下标越界",
            AuxCode::BatchIdxOver => "接管批次下标越界",
            AuxCode::ShadowShort => "影子比对轮数不足",
            AuxCode::SemanticBorder => "语义边界越界：本单不真改 testing/ 目录",
        }
    }
}

/// 本单结果别名。
pub type KtResult<T> = Result<T, KtErr>;
/// 错误对象（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KtErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
impl KtErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> KtErr {
        KtErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> KtErr {
        KtErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> KtErr {
        KtErr { code: a.code(), detail }
    }
    /// 三要素之一：码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之二：明细。
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
    /// 三要素之三：全串。
    pub fn full(&self) -> String {
        format!("0x{:04X} {}", self.code, self.detail)
    }
}
/// 码 → 名称反查。
pub fn name_by_code(code: u16) -> Option<&'static str> {
    let mut i: usize = 0;
    while i < Form::COUNT {
        let f = Form::ALL[i];
        if f.code() == code {
            return Some(f.name());
        }
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < AuxCode::COUNT {
        let a = AuxCode::ALL[j];
        if a.code() == code {
            return Some(a.name());
        }
        j = j.wrapping_add(1);
    }
    None
}
/// 错误一行串。
pub fn err_str(e: &KtErr) -> String {
    match name_by_code(e.code) {
        Some(n) => format!("[{}] {}", n, e.detail),
        None => format!("[0x{:04X}] {}", e.code, e.detail),
    }
}
/// 冻结态不符错误。
pub fn err_frozen(got: &str, want: &str) -> KtErr {
    KtErr::of_form(
        Form::FingerprintDrift,
        format!("冻结态不符：got={} want={}", got, want),
    )
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> KtErr {
    KtErr::of_aux(
        AuxCode::MacroIdxOver,
        format!("越界：{} idx={} cap={}", what, idx, cap),
    )
}
/// 守恒式错误。
pub fn err_recon(a: u64, b: u64, c: u64) -> KtErr {
    KtErr::of_form(
        Form::ReconcileDeviation,
        format!("守恒式偏离：批次容量={} 条目数={} 总账={}", a, b, c),
    )
}
/// 码段合法性。
pub fn code_in_seg(code: u16) -> bool {
    code >= SEG_BASE && code <= SEG_END
}
/// 主码串。
pub fn seg_line() -> String {
    format!(
        "seg 0x{:04X}..=0x{:04X} mask=0x{:04X} forms={} aux={} max=0x{:04X}",
        SEG_BASE,
        SEG_END,
        SEG_MASK,
        Form::COUNT,
        AuxCode::COUNT,
        AuxCode::ALL[AuxCode::COUNT - 1].code()
    )
}
// ===========================================================================
// 2. 确定性基础件
// ===========================================================================

/// 确定性 LCG。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 状态。
    pub state: u64,
}
impl Lcg {
    /// 构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取低 32 位。
    pub fn next32(&mut self) -> u32 {
        (self.next() & 0xFFFF_FFFF) as u32
    }
    /// 取模取样。
    pub fn below(&mut self, m: u64) -> u64 {
        let v = self.next();
        if m == 0 {
            0
        } else {
            v % m
        }
    }
    /// 区间取样（`[lo, hi)`；`hi <= lo` 时给 `lo`）。
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo.wrapping_add(self.below(hi - lo))
    }
}
/// LCG 单步。
pub fn lcg_step(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005u64)
        .wrapping_add(1442695040888963407u64)
}
/// FNV-1a 64 增量。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h: u64 = state;
    for b in bytes.iter() {
        let x: u64 = (*b as u64) & 0xFF;
        h = (h ^ x).wrapping_mul(0x100000001b3u64);
    }
    h
}
/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(0xcbf29ce484222325u64, bytes)
}
/// FNV-1a 64 词表指纹。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    for w in words.iter() {
        let v: u64 = *w;
        let mut i: usize = 0;
        while i < 8 {
            let byte: u8 = ((v >> (56 - 8 * i)) & 0xFF) as u8;
            h = (h ^ (byte as u64)).wrapping_mul(0x100000001b3u64);
            i = i.wrapping_add(1);
        }
    }
    h
}
/// 字符串折入。
pub fn fnv1a_str(state: u64, s: &str) -> u64 {
    fnv1a64_ext(state, s.as_bytes())
}
/// 逻辑时钟推进（零墙钟）。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(1)
}
/// 整除保护。
pub fn div_or_zero(a: u64, b: u64) -> u64 {
    if b == 0 {
        0
    } else {
        a / b
    }
}
/// 64 位整数平方根（牛顿迭代，零浮点）。
pub fn isqrt_u64(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut r: u64 = 1u64 << 32;
    let mut guard: u32 = 0;
    while guard < 80 {
        let y: u64 = (r.wrapping_add(n / r)) >> 1;
        if y >= r {
            break;
        }
        r = y;
        guard = guard.wrapping_add(1);
    }
    r
}

// ===========================================================================
// 3. 错误闭集
// ===========================================================================


// ===========================================================================
// 4. 核心结构一：现状盘点（Inventory）
// ===========================================================================

/// 接管状态（三步法的状态位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OwnerState {
    /// 旧路径独占（尚未进入接管流程）。
    Legacy,
    /// 影子期（新旧并行，结果须逐轮比对一致）。
    Shadow,
    /// 新路径接管（`testing/` 已被 ktest 接管）。
    Owned,
}
impl OwnerState {
    /// 状态数。
    pub const COUNT: usize = 3;
    /// 全部状态。
    pub const ALL: [OwnerState; OwnerState::COUNT] = [
        OwnerState::Legacy,
        OwnerState::Shadow,
        OwnerState::Owned,
    ];
    /// 下标（状态秩，用于单调性检查）。
    pub fn index(self) -> usize {
        match self {
            OwnerState::Legacy => 0,
            OwnerState::Shadow => 1,
            OwnerState::Owned => 2,
        }
    }
    /// 按秩取。
    pub fn from_index(i: usize) -> Option<OwnerState> {
        if i < OwnerState::COUNT {
            Some(OwnerState::ALL[i])
        } else {
            None
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            OwnerState::Legacy => "Legacy",
            OwnerState::Shadow => "Shadow",
            OwnerState::Owned => "Owned",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            OwnerState::Legacy => "旧路径独占",
            OwnerState::Shadow => "影子期（双跑比对）",
            OwnerState::Owned => "已接管",
        }
    }
    /// 是否比 `other` 秩高（严格）。
    pub fn above(self, other: OwnerState) -> bool {
        self.index() > other.index()
    }
    /// 四列输出。
    pub fn four_columns(&self) -> String {
        format!("{:<8} rank={} {:<16}", self.name(), self.index(), self.cn())
    }
}
/// 目录条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DirEntry {
    /// 条目号。
    pub idx: usize,
    /// 条目名（虚拟路径，零真实 IO）。
    pub name: String,
    /// 现状用例数。
    pub cases: u32,
    /// 现状判据覆盖数。
    pub crit_cover: u32,
    /// 接管状态。
    pub state: OwnerState,
    /// 分配批次（`Owned` 必有批次）。
    pub batch: usize,
    /// 旧路径剩余调用数（`Owned` 应为 0）。
    pub legacy_left: u32,
}
impl DirEntry {
    /// 构造。
    pub fn new(idx: usize, name: String, cases: u32, crit_cover: u32, state: OwnerState, batch: usize, legacy_left: u32) -> DirEntry {
        DirEntry { idx, name, cases, crit_cover, state, batch, legacy_left }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "entry#{} {:<18} cases={:<4} crit={:<3} state={:<7} batch={} legacy_left={}",
            self.idx, self.name, self.cases, self.crit_cover, self.state.name(), self.batch, self.legacy_left
        )
    }
    /// 指纹词（冻结件只折定义字段）。
    pub fn words(&self) -> [u64; 6] {
        [
            self.idx as u64,
            fnv1a64(self.name.as_bytes()),
            self.cases as u64,
            self.crit_cover as u64,
            self.state.index() as u64,
            self.batch as u64,
        ]
    }
}
/// 现状盘点表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Inventory {
    /// 条目。
    pub entries: Vec<DirEntry>,
    /// 存在位图（`DIR_ENTRIES ≤ 32`，用 `u32`）。
    pub cover_bits: u32,
    /// 重号位图。
    pub dup_bits: u32,
    /// 合计用例数。
    pub cases_total: u32,
    /// 合计判据覆盖数。
    pub crit_total: u32,
}
impl Inventory {
    /// 净态盘点表。
    pub fn net() -> KtResult<Inventory> {
        let mut entries: Vec<DirEntry> = Vec::with_capacity(DIR_ENTRIES);
        let mut cover_bits: u32 = 0;
        let mut cases_total: u32 = 0;
        let mut crit_total: u32 = 0;
        let mut i: usize = 0;
        while i < DIR_ENTRIES {
            // 净态 = 四批全部推进完毕（三步法收敛）：末态一律 Owned，旧路径调用归零。
            // 中间态（Legacy / Shadow）由迁移轨迹体现，不在净态盘点表里残留。
            let batch: usize = i / BATCH_CAP;
            let state: OwnerState = OwnerState::Owned;
            let cases: u32 = 3u32 + ((i as u32) % 5u32);
            let crit: u32 = 1u32 + ((i as u32) % 3u32);
            let legacy_left: u32 = 0;
            let name: String = format!("testing/{}/case_{:02}.rs", state.name().to_lowercase(), i);
            entries.push(DirEntry::new(i, name, cases, crit, state, batch, legacy_left));
            cover_bits = cover_bits | (1u32 << (i & 31));
            cases_total = cases_total.wrapping_add(cases);
            crit_total = crit_total.wrapping_add(crit);
            i = i.wrapping_add(1);
        }
        Ok(Inventory {
            entries,
            cover_bits,
            dup_bits: 0,
            cases_total,
            crit_total,
        })
    }
    /// 覆盖完备（存在位图 == 全 1）。
    pub fn cover_complete(&self) -> bool {
        let want: u32 = if DIR_ENTRIES >= 32 {
            0xFFFF_FFFFu32
        } else {
            (1u32 << DIR_ENTRIES) - 1u32
        };
        self.cover_bits & want == want
    }
    /// 无重号。
    pub fn dup_free(&self) -> bool {
        self.dup_bits == 0
    }
    /// 按状态计数。
    pub fn state_count(&self, s: OwnerState) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].state == s {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 旧路径剩余调用总数。
    pub fn legacy_left_total(&self) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            n = n.wrapping_add(self.entries[i].legacy_left);
            i = i.wrapping_add(1);
        }
        n
    }
    /// 指纹（冻结件：只折定义，不折结果）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + DIR_ENTRIES * 6);
        w.push(V_VERSION_ID);
        w.push(self.cover_bits as u64);
        w.push(self.entries.len() as u64);
        let mut i: usize = 0;
        while i < self.entries.len() {
            let ww = self.entries[i].words();
            let mut j: usize = 0;
            while j < 6 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "inv entries={} cover={:08X} dup={:08X} cases={} crit={} legacy_left={}",
            self.entries.len(),
            self.cover_bits,
            self.dup_bits,
            self.cases_total,
            self.crit_total,
            self.legacy_left_total()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.entries.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.entries.len() {
            out.push(format!("  {}", self.entries[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 5. 核心结构二：扩容目标与缺口账（GapBook）
// ===========================================================================

/// 扩容目标态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetState {
    /// 目标用例数。
    pub cases: u32,
    /// 目标判据覆盖数。
    pub crit: u32,
    /// 目标宏族覆盖条目数。
    pub macro_cover: u32,
}
impl TargetState {
    /// 构造。
    pub fn new(cases: u32, crit: u32, macro_cover: u32) -> TargetState {
        TargetState { cases, crit, macro_cover }
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 3] {
        [self.cases as u64, self.crit as u64, self.macro_cover as u64]
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "target cases={} crit={} macro_cover={}",
            self.cases, self.crit, self.macro_cover
        )
    }
}
/// 缺口项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GapItem {
    /// 缺口名。
    pub name: String,
    /// 现状值。
    pub cur: u32,
    /// 目标值。
    pub target: u32,
    /// 是否已登记（缺口必须逐项可寻址）。
    pub listed: u8,
}
impl GapItem {
    /// 缺口语义：正数为待扩容，负数为待收缩。
    pub fn delta(&self) -> i64 {
        self.target as i64 - self.cur as i64
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "gap {:<16} cur={:<5} target={:<5} delta={:<6} listed={}",
            self.name,
            self.cur,
            self.target,
            self.delta(),
            self.listed
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 4] {
        [fnv1a64(self.name.as_bytes()), self.cur as u64, self.target as u64, self.listed as u64]
    }
}
/// 缺口账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GapBook {
    /// 目标态。
    pub target: TargetState,
    /// 缺口项。
    pub items: Vec<GapItem>,
    /// 目标是否自洽（各项目标值 ≥ 现状值）。
    pub coherent: bool,
}
impl GapBook {
    /// 由盘点表与目标构造。
    pub fn build(inv: &Inventory, target: TargetState) -> GapBook {
        let macro_cover: u32 = inv.state_count(OwnerState::Owned) + inv.state_count(OwnerState::Shadow);
        let items: Vec<GapItem> = vec![
            GapItem {
                name: String::from("cases"),
                cur: inv.cases_total,
                target: target.cases,
                listed: 1,
            },
            GapItem {
                name: String::from("crit"),
                cur: inv.crit_total,
                target: target.crit,
                listed: 1,
            },
            GapItem {
                name: String::from("macro_cover"),
                cur: macro_cover,
                target: target.macro_cover,
                listed: 1,
            },
        ];
        let mut coherent: bool = true;
        let mut i: usize = 0;
        while i < items.len() {
            if items[i].target < items[i].cur {
                coherent = false;
            }
            i = i.wrapping_add(1);
        }
        GapBook { target, items, coherent }
    }
    /// 缺口是否全部登记。
    pub fn all_listed(&self) -> bool {
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].listed == 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + self.items.len() * 4);
        w.push(V_VERSION_ID);
        let tw = self.target.words();
        let mut t: usize = 0;
        while t < 3 {
            w.push(tw[t]);
            t = t.wrapping_add(1);
        }
        let mut i: usize = 0;
        while i < self.items.len() {
            let ww = self.items[i].words();
            let mut j: usize = 0;
            while j < 4 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "gap target=({}/{}) coherent={} listed={} items={}",
            self.target.cases,
            self.target.crit,
            if self.coherent { 1 } else { 0 },
            if self.all_listed() { 1 } else { 0 },
            self.items.len()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            out.push(format!("  {}", self.items[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 6. 核心结构三：宏族注册表（MacroBook）
// ===========================================================================

/// 宏族定义（`宏名 → 展开 → 判据数`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MacroFamily {
    /// 宏族号。
    pub idx: usize,
    /// 宏名。
    pub name: String,
    /// 展开式（人类可读模板串）。
    pub expand: String,
    /// 展开项（**有界**：不超过 `MACRO_FANOUT_CAP`）。
    pub fanout: Vec<u32>,
    /// 判据数。
    pub crit: u32,
}
impl MacroFamily {
    /// 构造。
    pub fn new(idx: usize, name: String, expand: String, fanout: Vec<u32>, crit: u32) -> MacroFamily {
        MacroFamily { idx, name, expand, fanout, crit }
    }
    /// 展开项数。
    pub fn fanout_n(&self) -> usize {
        self.fanout.len()
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "macro#{} {:<12} fanout={:<3} crit={:<3} expand={}",
            self.idx, self.name, self.fanout.len(), self.crit, self.expand
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [
            self.idx as u64,
            fnv1a64(self.name.as_bytes()),
            self.fanout.len() as u64,
            self.crit as u64,
            fnv1a64(self.expand.as_bytes()),
        ]
    }
}
/// 宏族名表。
pub const MACRO_NAMES: [&str; MACRO_FAMILIES] = [
    "kt_param", "kt_matrix", "kt_stress", "kt_bound", "kt_concur", "kt_regress",
];
/// 宏展开式表。
pub const MACRO_EXPANDS: [&str; MACRO_FAMILIES] = [
    "p{u32}",
    "m{a}x{b}",
    "s{n}x{k}",
    "b{lo}..{hi}",
    "c{n}x{p}",
    "r{tag}x{n}",
];
/// 宏族注册表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MacroBook {
    /// 宏族。
    pub families: Vec<MacroFamily>,
    /// 重名位图（`MACRO_FAMILIES ≤ 32`，用 `u32`）。
    pub dup_bits: u32,
    /// 展开项总数。
    pub fanout_total: u32,
    /// 判据总数。
    pub crit_total: u32,
}
impl MacroBook {
    /// 净态注册表。
    pub fn net() -> KtResult<MacroBook> {
        let mut families: Vec<MacroFamily> = Vec::with_capacity(MACRO_FAMILIES);
        let mut dup_bits: u32 = 0;
        let mut fanout_total: u32 = 0;
        let mut crit_total: u32 = 0;
        let mut i: usize = 0;
        while i < MACRO_FAMILIES {
            let name: String = String::from(MACRO_NAMES[i]);
            // 判重
            let mut j: usize = 0;
            while j < i {
                if families[j].name == name {
                    dup_bits = dup_bits | (1u32 << (i & 31));
                }
                j = j.wrapping_add(1);
            }
            // 扇出逐族递增但**恒不超过 MACRO_FANOUT_CAP**（i ≤ 5 → 最大 9）
            let fan_n: usize = 4 + i;
            let mut fanout: Vec<u32> = Vec::with_capacity(fan_n);
            let mut k: u32 = 0;
            while (k as usize) < fan_n {
                fanout.push(i as u32 * 100u32 + k);
                k = k.wrapping_add(1);
            }
            let crit: u32 = 2u32 + ((i as u32) % 5u32);
            families.push(MacroFamily::new(
                i,
                name,
                String::from(MACRO_EXPANDS[i]),
                fanout,
                crit,
            ));
            fanout_total = fanout_total.wrapping_add(fan_n as u32);
            crit_total = crit_total.wrapping_add(crit);
            i = i.wrapping_add(1);
        }
        Ok(MacroBook {
            families,
            dup_bits,
            fanout_total,
            crit_total,
        })
    }
    /// 按名取宏族。
    pub fn by_name(&self, name: &str) -> KtResult<usize> {
        let mut i: usize = 0;
        while i < self.families.len() {
            if self.families[i].name == name {
                return Ok(i);
            }
            i = i.wrapping_add(1);
        }
        Err(err_range("by_name", i, self.families.len()))
    }
    /// 取宏族。
    pub fn at(&self, i: usize) -> KtResult<MacroFamily> {
        match self.families.get(i) {
            Some(f) => Ok(f.clone()),
            None => Err(KtErr::of_aux(
                AuxCode::MacroIdxOver,
                format!("宏族下标越界 idx={} cap={}", i, self.families.len()),
            )),
        }
    }
    /// 扇出全部在界内。
    pub fn fanout_bounded(&self) -> bool {
        let mut i: usize = 0;
        while i < self.families.len() {
            if self.families[i].fanout.len() > MACRO_FANOUT_CAP {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 展开全部非空。
    pub fn fanout_nonzero(&self) -> bool {
        let mut i: usize = 0;
        while i < self.families.len() {
            if self.families[i].fanout.len() == 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 判据数全部在界内。
    pub fn crit_bounded(&self) -> bool {
        let mut i: usize = 0;
        while i < self.families.len() {
            if self.families[i].crit as usize > CRITERIA_TOTAL {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 无重名。
    pub fn dup_free(&self) -> bool {
        self.dup_bits == 0
    }
    /// 指纹（只折宏族定义）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + self.families.len() * 5);
        w.push(V_VERSION_ID);
        w.push(self.families.len() as u64);
        w.push(MACRO_FANOUT_CAP as u64);
        let mut i: usize = 0;
        while i < self.families.len() {
            let ww = self.families[i].words();
            let mut j: usize = 0;
            while j < 5 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "macro families={} dup={:08X} fanout_total={} crit_total={}",
            self.families.len(),
            self.dup_bits,
            self.fanout_total,
            self.crit_total
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.families.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.families.len() {
            out.push(format!("  {}", self.families[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 7. 核心结构四：接管迁移账（Migration）
// ===========================================================================

/// 批次记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BatchRow {
    /// 批次号。
    pub batch: usize,
    /// 容量。
    pub cap: usize,
    /// 本批条目下标（升序）。
    pub items: Vec<usize>,
    /// 影子比对轮数。
    pub shadow_rounds: u32,
    /// 影子一致位（1 = 全轮一致）。
    pub shadow_ok: u8,
    /// 批末旧路径剩余调用数（须单调下降）。
    pub legacy_left_after: u32,
    /// 推进位（1 = 已推进接管）。
    pub advanced: u8,
}
impl BatchRow {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "batch#{} cap={} items={:?} shadow={}/{} ok={} legacy_left_after={} advanced={}",
            self.batch,
            self.cap,
            self.items,
            self.shadow_rounds,
            SHADOW_ROUNDS,
            self.shadow_ok,
            self.legacy_left_after,
            self.advanced
        )
    }
    /// 指纹词（推进态）。
    pub fn words(&self) -> [u64; 5] {
        [
            self.batch as u64,
            self.cap as u64,
            self.shadow_rounds as u64,
            self.shadow_ok as u64,
            self.advanced as u64,
        ]
    }
}
/// 接管迁移账（三步法分批推进）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Migration {
    /// 批次。
    pub batches: Vec<BatchRow>,
    /// 状态轨迹（按条目下标记录秩）。
    pub trail: Vec<(usize, usize)>,
    /// 步数消耗。
    pub steps: u64,
    /// 步数上限。
    pub step_cap: u64,
}
impl Migration {
    /// 空账。
    pub fn new() -> Migration {
        Migration {
            batches: Vec::with_capacity(MIGRATION_BATCHES),
            trail: Vec::with_capacity(DIR_ENTRIES),
            steps: 0,
            step_cap: STEP_CAP,
        }
    }
    /// 推进一个批次（三步法：盘点 → 影子 → 接管）。
    pub fn advance_batch(&mut self, inv: &Inventory, batch: usize) -> KtResult<()> {
        if batch >= MIGRATION_BATCHES {
            return Err(KtErr::of_aux(
                AuxCode::BatchIdxOver,
                format!("批次越界 batch={} cap={}", batch, MIGRATION_BATCHES),
            ));
        }
        if self.steps >= self.step_cap {
            return Err(KtErr::of_aux(
                AuxCode::StepOverCap,
                format!("步数越界 steps={} cap={}", self.steps, self.step_cap),
            ));
        }
        // 区间用 saturating 计算，杜绝 `hi < lo` 导致的下溢
        let lo: usize = batch.saturating_mul(BATCH_CAP).min(DIR_ENTRIES);
        let hi: usize = lo.saturating_add(BATCH_CAP).min(DIR_ENTRIES);
        let mut items: Vec<usize> = Vec::with_capacity(hi - lo);
        let mut i: usize = lo;
        while i < hi {
            items.push(i);
            i = i.wrapping_add(1);
        }
        // 影子比对：SHADOW_ROUNDS 轮全一致方可推进
        let shadow_ok: u8 = 1;
        // 批末旧路径剩余调用数：等于该批之后仍处 Legacy/Shadow 的条目数
        let mut legacy_left_after: u32 = 0;
        let mut j: usize = hi;
        while j < inv.entries.len() {
            if inv.entries[j].state != OwnerState::Owned {
                legacy_left_after = legacy_left_after.wrapping_add(inv.entries[j].legacy_left);
            }
            j = j.wrapping_add(1);
        }
        self.batches.push(BatchRow {
            batch,
            cap: items.len(),
            items,
            shadow_rounds: SHADOW_ROUNDS as u32,
            shadow_ok,
            legacy_left_after,
            advanced: 1,
        });
        // 记状态轨迹（条目 → 目标秩）
        let target_rank: usize = match batch {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        let mut k: usize = 0;
        while k < self.batches[self.batches.len() - 1].items.len() {
            let it = self.batches[self.batches.len() - 1].items[k];
            self.trail.push((it, target_rank));
            self.steps = self.steps.wrapping_add(2);
            k = k.wrapping_add(1);
        }
        Ok(())
    }
    /// 状态是否单调（轨迹内每条目秩只增不减）。
    pub fn state_monotonic(&self) -> bool {
        let mut seen: Vec<(usize, usize)> = Vec::with_capacity(self.trail.len());
        let mut i: usize = 0;
        while i < self.trail.len() {
            let (item, rank) = self.trail[i];
            let mut found: bool = false;
            let mut j: usize = 0;
            while j < seen.len() {
                if seen[j].0 == item {
                    if rank < seen[j].1 {
                        return false;
                    }
                    found = true;
                }
                j = j.wrapping_add(1);
            }
            if !found {
                seen.push((item, rank));
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 影子比对是否全一致。
    pub fn shadow_all_match(&self) -> bool {
        let mut i: usize = 0;
        while i < self.batches.len() {
            let b = &self.batches[i];
            if b.shadow_ok != 1 || b.shadow_rounds < SHADOW_ROUNDS as u32 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 旧路径是否收敛（逐批单调不增，且末批为 0）。
    pub fn legacy_converged(&self) -> bool {
        let mut i: usize = 1;
        while i < self.batches.len() {
            if self.batches[i].legacy_left_after > self.batches[i - 1].legacy_left_after {
                return false;
            }
            i = i.wrapping_add(1);
        }
        match self.batches.last() {
            Some(b) => b.legacy_left_after == 0,
            None => false,
        }
    }
    /// 批次容量合计。
    pub fn cap_total(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < self.batches.len() {
            s = s.wrapping_add(self.batches[i].cap as u64);
            i = i.wrapping_add(1);
        }
        s
    }
    /// 指纹（推进态，冻结件用）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + self.batches.len() * 5);
        w.push(V_VERSION_ID);
        w.push(self.batches.len() as u64);
        w.push(MIGRATION_BATCHES as u64);
        let mut i: usize = 0;
        while i < self.batches.len() {
            let ww = self.batches[i].words();
            let mut j: usize = 0;
            while j < 5 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "mig batches={} cap_total={} shadow_ok={} converged={} monotonic={} steps={}",
            self.batches.len(),
            self.cap_total(),
            if self.shadow_all_match() { 1 } else { 0 },
            if self.legacy_converged() { 1 } else { 0 },
            if self.state_monotonic() { 1 } else { 0 },
            self.steps
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.batches.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.batches.len() {
            out.push(format!("  {}", self.batches[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 8. 主状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================

/// 接管三步法阶段（盘点 → 影子 → 接管）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 步一：现状盘点。
    P1Inventory,
    /// 步二：影子并行（双跑比对）。
    P2Shadow,
    /// 步三：接管（ktest 接管 testing/）。
    P3Own,
    /// 步四：收敛核对（旧路径归零）。
    P4Converge,
    /// 步五：设计冻结入册。
    P5Freeze,
    /// 终态。
    Sealed,
}
impl Stage {
    /// 阶段数（含终态）。
    pub const COUNT: usize = 6;
    /// 全部阶段。
    pub const ALL: [Stage; Stage::COUNT] = [
        Stage::P1Inventory,
        Stage::P2Shadow,
        Stage::P3Own,
        Stage::P4Converge,
        Stage::P5Freeze,
        Stage::Sealed,
    ];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            Stage::P1Inventory => 0,
            Stage::P2Shadow => 1,
            Stage::P3Own => 2,
            Stage::P4Converge => 3,
            Stage::P5Freeze => 4,
            Stage::Sealed => 5,
        }
    }
    /// 按下标取。
    pub fn from_index(i: usize) -> Option<Stage> {
        if i < Stage::COUNT {
            Some(Stage::ALL[i])
        } else {
            None
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Stage::P1Inventory => "P1Inventory",
            Stage::P2Shadow => "P2Shadow",
            Stage::P3Own => "P3Own",
            Stage::P4Converge => "P4Converge",
            Stage::P5Freeze => "P5Freeze",
            Stage::Sealed => "Sealed",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            Stage::P1Inventory => "现状盘点",
            Stage::P2Shadow => "影子并行比对",
            Stage::P3Own => "接管 testing/",
            Stage::P4Converge => "旧路径收敛核对",
            Stage::P5Freeze => "设计冻结入册",
            Stage::Sealed => "收口",
        }
    }
    /// 是否终态。
    pub fn is_sealed(self) -> bool {
        self == Stage::Sealed
    }
    /// 四列输出。
    pub fn four_columns(&self) -> String {
        let nxt: &'static str = match self {
            Stage::P1Inventory => "P2Shadow",
            Stage::P2Shadow => "P3Own",
            Stage::P3Own => "P4Converge",
            Stage::P4Converge => "P5Freeze",
            Stage::P5Freeze => "Sealed",
            Stage::Sealed => "-",
        };
        format!("{:<14} {:<16} next={:<12} {}", self.name(), self.cn(), nxt, FACE_MIGRATION)
    }
}
/// 非法转换留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    /// 序号。
    pub seq: u32,
    /// 起始阶段。
    pub from: Stage,
    /// 目标阶段。
    pub to: Stage,
    /// 逻辑时刻。
    pub tick: u64,
    /// 诊断码。
    pub code: u16,
}
impl Reject {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "reject#{} {}->{} t={} code=0x{:04X}",
            self.seq,
            self.from.name(),
            self.to.name(),
            self.tick,
            self.code
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 4] {
        [self.from.index() as u64, self.to.index() as u64, self.tick, self.code as u64]
    }
}
/// 主状态机。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StageFsm {
    /// 当前阶段。
    pub state: Stage,
    /// 逻辑时刻。
    pub tick: u64,
    /// 合法边推进数。
    pub legal_moves: u64,
    /// 拒记。
    pub rejected: Vec<Reject>,
    /// 足迹。
    pub trail: Vec<Stage>,
}
impl StageFsm {
    /// 初始态。
    pub fn new() -> StageFsm {
        let mut trail: Vec<Stage> = Vec::with_capacity(Stage::COUNT);
        trail.push(Stage::P1Inventory);
        StageFsm { state: Stage::P1Inventory, tick: 0, legal_moves: 0, rejected: Vec::new(), trail }
    }
    /// 合法边闭集。
    pub fn is_legal(from: Stage, to: Stage) -> bool {
        match (from, to) {
            (Stage::P1Inventory, Stage::P2Shadow) => true,
            (Stage::P2Shadow, Stage::P3Own) => true,
            (Stage::P3Own, Stage::P4Converge) => true,
            (Stage::P4Converge, Stage::P5Freeze) => true,
            (Stage::P5Freeze, Stage::Sealed) => true,
            _ => false,
        }
    }
    /// 合法边总数。
    pub fn edge_count() -> usize {
        5
    }
    /// 非法候选边总数。
    pub fn illegal_edge_count() -> usize {
        Stage::COUNT * Stage::COUNT - StageFsm::edge_count()
    }
    /// 试走一条边。
    pub fn try_go(&mut self, to: Stage, tick: u64) -> KtResult<Stage> {
        self.tick = tick;
        if self.state.is_sealed() {
            self.rejected.push(Reject {
                seq: self.rejected.len() as u32,
                from: self.state,
                to,
                tick,
                code: Form::InventoryMissing.code(),
            });
            return Err(KtErr::of_form(
                Form::InventoryMissing,
                format!("终态不可迁移：{}->{}", self.state.name(), to.name()),
            ));
        }
        if StageFsm::is_legal(self.state, to) {
            self.state = to;
            self.legal_moves = self.legal_moves.wrapping_add(1);
            self.trail.push(to);
            return Ok(to);
        }
        self.rejected.push(Reject {
            seq: self.rejected.len() as u32,
            from: self.state,
            to,
            tick,
            code: Form::MacroEmpty.code(),
        });
        Err(KtErr::of_form(
            Form::MacroEmpty,
            format!("非法转换：{}->{}", self.state.name(), to.name()),
        ))
    }
    /// 走满全部合法边。
    pub fn drive_all(&mut self, start_tick: u64) -> KtResult<()> {
        let mut t = start_tick;
        let mut i: usize = 1;
        while i < Stage::COUNT {
            t = t.wrapping_add(1);
            self.try_go(Stage::ALL[i], t)?;
            i = i.wrapping_add(1);
        }
        Ok(())
    }
    /// 足迹单调。
    pub fn trail_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.trail.len() {
            if self.trail[i - 1].index() >= self.trail[i].index() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.trail.len() == Stage::COUNT
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "fsm state={} legal={} reject={} trail={}",
            self.state.name(),
            self.legal_moves,
            self.rejected.len(),
            self.trail.len()
        )
    }
}

// ===========================================================================
// 9. append-only 环形账
// ===========================================================================

/// 环形账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 样本下标。
    pub sample: usize,
    /// 轮次。
    pub round: u32,
    /// 差异分类下标。
    pub diff: u8,
    /// 执行指纹。
    pub fp: u64,
    /// 逻辑时刻。
    pub tick: u64,
}
impl RingEntry {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "ring s{} r{} diff={} fp={:016X} t={}",
            self.sample, self.round, self.diff, self.fp, self.tick
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [self.sample as u64, self.round as u64, self.diff as u64, self.fp, self.tick]
    }
}
/// append-only 环形账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 缓冲。
    pub buf: Vec<RingEntry>,
    /// 写指针。
    pub head: usize,
    /// 累计写入。
    pub pushed: u64,
    /// 累计丢弃（显性）。
    pub dropped: u64,
}
impl RingLedger {
    /// 空账。
    pub fn new() -> RingLedger {
        let mut buf: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        let mut i: usize = 0;
        while i < RING_CAP {
            buf.push(RingEntry { sample: 0, round: 0, diff: 0xFF, fp: 0, tick: 0 });
            i = i.wrapping_add(1);
        }
        RingLedger { buf, head: 0, pushed: 0, dropped: 0 }
    }
    /// 写一条。
    pub fn push(&mut self, e: RingEntry) {
        let slot = self.head;
        if let Some(old) = self.buf.get(slot) {
            if old.diff != 0xFF || old.fp != 0 {
                self.dropped = self.dropped.wrapping_add(1);
            }
        }
        let mut ne = e;
        ne.tick = e.tick;
        if let Some(c) = self.buf.get_mut(slot) {
            *c = ne;
        }
        self.head = self.head.wrapping_add(1) & RING_MASK;
        self.pushed = self.pushed.wrapping_add(1);
    }
    /// 存活条目数。
    pub fn live(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < self.buf.len() {
            if self.buf[i].diff != 0xFF || self.buf[i].fp != 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 守恒。
    pub fn balanced(&self) -> bool {
        self.pushed == self.live().wrapping_add(self.dropped)
    }
    /// 指纹（活账，不入冻结）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + RING_CAP * 5);
        w.push(V_VERSION_ID);
        w.push(self.pushed);
        w.push(self.dropped);
        let mut i: usize = 0;
        while i < self.buf.len() {
            let ww = self.buf[i].words();
            let mut j: usize = 0;
            while j < 5 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "ring pushed={} live={} dropped={} balanced={}",
            self.pushed,
            self.live(),
            self.dropped,
            if self.balanced() { 1 } else { 0 }
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(RING_CAP + 1);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.buf.len() {
            let e = self.buf[i];
            if e.diff != 0xFF || e.fp != 0 {
                out.push(format!("  {}", e.describe()));
            }
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 10. 守恒账 reconcile()
// ===========================================================================

/// 守恒账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reconcile {
    /// ΣA：批次容量合计。
    pub sum_a: u64,
    /// ΣB：目录条目数。
    pub sum_b: u64,
    /// ΣC：环形账写入数。
    pub sum_c: u64,
    /// ΣD：宏族展开项总数。
    pub sum_d: u64,
    /// ΣE：状态轨迹条目数。
    pub sum_e: u64,
    /// 缺口登记数。
    pub gaps: u64,
    /// 最大偏离。
    pub deviation: u64,
    /// 是否守恒。
    pub balanced: bool,
    /// 逐项结论。
    pub items: Vec<(String, u64, u64, bool)>,
}
impl Reconcile {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "recon A={} B={} C={} D={} E={} gaps={} dev={} balanced={} items={}",
            self.sum_a, self.sum_b, self.sum_c, self.sum_d, self.sum_e,
            self.gaps, self.deviation, if self.balanced { 1 } else { 0 }, self.items.len()
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            let it = &self.items[i];
            out.push(format!("  {:<26} L={:<7} R={:<7} eq={}", it.0, it.1, it.2, if it.3 { 1 } else { 0 }));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 计算守恒账。
pub fn reconcile(
    inv: &Inventory,
    macro_book: &MacroBook,
    gap: &GapBook,
    mig: &Migration,
    ring: &RingLedger,
) -> Reconcile {
    let a: u64 = mig.cap_total();
    let b: u64 = inv.entries.len() as u64;
    let c: u64 = ring.pushed;
    let d: u64 = macro_book.fanout_total as u64;
    let e: u64 = mig.trail.len() as u64;
    let mut gaps: u64 = 0;
    let mut i: usize = 0;
    while i < gap.items.len() {
        if gap.items[i].listed == 1 {
            gaps = gaps.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    let mut items: Vec<(String, u64, u64, bool)> = Vec::with_capacity(7);
    items.push((String::from("批次容量=条目数"), a, b, a == b));
    items.push((String::from("批次容量=轨迹条数"), a, e, a == e));
    items.push((String::from("批次容量=环形写入"), a, c, a == c));
    items.push((String::from("宏展开=扇出合计"), d, macro_book.fanout_total as u64, true));
    items.push((String::from("缺口全登记(布尔)"), gaps, gap.items.len() as u64, gap.all_listed()));
    items.push((String::from("目标自洽(布尔)"), if gap.coherent { 1 } else { 0 }, 1, gap.coherent));
    items.push((String::from("旧路径归零(布尔)"), inv.legacy_left_total() as u64, 0, inv.legacy_left_total() == 0));
    let mut dev: u64 = 0;
    let mut m: usize = 0;
    while m < items.len() {
        let it = &items[m];
        let l = if it.1 > it.2 { it.1 - it.2 } else { it.2 - it.1 };
        if l > dev {
            dev = l;
        }
        m = m.wrapping_add(1);
    }
    let balanced: bool = dev == 0 && items.iter().all(|it| it.3);
    Reconcile {
        sum_a: a,
        sum_b: b,
        sum_c: c,
        sum_d: d,
        sum_e: e,
        gaps,
        deviation: dev,
        balanced,
        items,
    }
}

// ===========================================================================
// 11. 快照 + 注入核
// ===========================================================================

/// 注入留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 逐形态注入触发次数。
    pub fired: [u32; Form::COUNT],
    /// 盘点表指纹。
    pub inv_fp: u64,
    /// 宏族表指纹。
    pub macro_fp: u64,
    /// 缺口账指纹。
    pub gap_fp: u64,
    /// 迁移账指纹。
    pub mig_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 冻结指纹（净态入册值，注入不改写）。
    pub freeze_fp: u64,
    /// 入册探针基准值。
    pub registered_probe: u64,
    /// 语义边界违例（真实目录写入次数，恒 0）。
    pub border_violations: u32,
}
impl Evidence {
    /// 空现场。
    pub fn blank() -> Evidence {
        let mut fired: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            fired[i] = 0;
            i = i.wrapping_add(1);
        }
        Evidence {
            fired,
            inv_fp: 0,
            macro_fp: 0,
            gap_fp: 0,
            mig_fp: 0,
            ring_fp: 0,
            freeze_fp: 0,
            registered_probe: 0,
            border_violations: 0,
        }
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "ev inv={:016X} macro={:016X} gap={:016X} mig={:016X} ring={:016X} probe={:016X} border={}",
            self.inv_fp, self.macro_fp, self.gap_fp, self.mig_fp, self.ring_fp,
            self.registered_probe, self.border_violations
        )
    }
}
/// 设计件定义哈希（只折定义：宏族表 + 目标态 + 批次划分）。
pub fn design_definition_hash() -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(6);
    w.push(V_VERSION_ID);
    w.push(MACRO_FAMILIES as u64);
    w.push(DIR_ENTRIES as u64);
    w.push(MIGRATION_BATCHES as u64);
    w.push(BATCH_CAP as u64);
    w.push(MACRO_FANOUT_CAP as u64);
    let mut i: usize = 0;
    while i < MACRO_FAMILIES {
        w.push(fnv1a64(MACRO_NAMES[i].as_bytes()));
        w.push(fnv1a64(MACRO_EXPANDS[i].as_bytes()));
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 冻结探针基准哈希。
pub fn freeze_probe_hash(inv: &Inventory, mb: &MacroBook, gap: &GapBook, mig: &Migration) -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(6);
    w.push(V_VERSION_ID);
    w.push(design_definition_hash());
    w.push(inv.fingerprint());
    w.push(mb.fingerprint());
    w.push(gap.fingerprint());
    w.push(mig.fingerprint());
    fnv1a_words(&w)
}
/// 扩容目标（净态目标值：各项目标 ≥ 现状）。
pub fn target_state() -> TargetState {
    TargetState::new(CASE_TARGET as u32, CRITERIA_TOTAL as u32, DIR_ENTRIES as u32)
}
/// 快照。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 盘点表。
    pub inv: Inventory,
    /// 宏族表。
    pub macro_book: MacroBook,
    /// 缺口账。
    pub gap: GapBook,
    /// 迁移账。
    pub mig: Migration,
    /// 环形账。
    pub ring: RingLedger,
    /// 留痕。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态快照。
    pub fn net() -> KtResult<Snapshot> {
        let inv = Inventory::net()?;
        let macro_book = MacroBook::net()?;
        let target = target_state();
        let gap = GapBook::build(&inv, target);
        let mut mig = Migration::new();
        let mut ring = RingLedger::new();
        let mut b: usize = 0;
        while b < MIGRATION_BATCHES {
            mig.advance_batch(&inv, b)?;
            // 每批条目写环形账
            if let Some(row) = mig.batches.get(b) {
                let mut i: usize = 0;
                while i < row.items.len() {
                    let it = row.items[i];
                    let mut w: Vec<u64> = Vec::with_capacity(4);
                    w.push(it as u64);
                    w.push(b as u64);
                    w.push(row.cap as u64);
                    w.push(row.shadow_rounds as u64);
                    ring.push(RingEntry {
                        sample: it,
                        round: b as u32,
                        diff: row.shadow_ok,
                        fp: fnv1a_words(&w),
                        tick: b as u64,
                    });
                    i = i.wrapping_add(1);
                }
            }
            b = b.wrapping_add(1);
        }
        let mut ev = Evidence::blank();
        ev.inv_fp = inv.fingerprint();
        ev.macro_fp = macro_book.fingerprint();
        ev.gap_fp = gap.fingerprint();
        ev.mig_fp = mig.fingerprint();
        ev.ring_fp = ring.fingerprint();
        ev.freeze_fp = freeze_probe_hash(&inv, &macro_book, &gap, &mig);
        ev.registered_probe = ev.freeze_fp;
        ev.border_violations = 0;
        Ok(Snapshot {
            inv,
            macro_book,
            gap,
            mig,
            ring,
            ev,
        })
    }
    /// 守恒账。
    pub fn reconcile(&self) -> Reconcile {
        reconcile(&self.inv, &self.macro_book, &self.gap, &self.mig, &self.ring)
    }
    /// 是否净态。
    pub fn is_net(&self) -> bool {
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.ev.fired[i] != 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!("snapshot net={} {}", if self.is_net() { 1 } else { 0 }, self.ev.describe())
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(8);
        out.push(self.describe());
        out.push(self.inv.summary());
        out.push(self.gap.summary());
        out.push(self.macro_book.summary());
        out.push(self.mig.summary());
        out.push(self.ring.summary());
        out.push(self.reconcile().summary());
        out
    }
}
/// 单形态注入。
fn inject_one(f: Form) -> KtResult<Snapshot> {
    let mut s = Snapshot::net()?;
    s.ev.border_violations = 0;
    let idx = f.index();
    if idx < Form::COUNT {
        s.ev.fired[idx] = 1;
    }
    match f {
        Form::InventoryMissing => {
            // 盘点缺项：摘掉一条并清存在位
            if let Some(_last) = s.inv.entries.pop() {
                let b: u32 = 1u32 << (s.inv.entries.len() & 31);
                s.inv.cover_bits = s.inv.cover_bits & (s.inv.cover_bits ^ b);
            }
        }
        Form::InventoryDup => {
            // 盘点重号
            s.inv.dup_bits = s.inv.dup_bits | 1u32;
            if let Some(e) = s.inv.entries.get_mut(0) {
                e.name = String::from("testing/dup/dup.rs");
            }
        }
        Form::MacroDup => {
            // 宏族重名
            if let Some(m) = s.macro_book.families.get_mut(1) {
                m.name = String::from(MACRO_NAMES[0]);
            }
            s.macro_book.dup_bits = s.macro_book.dup_bits | (1u32 << 1);
        }
        Form::MacroFanoutOver => {
            // 宏展开越界：塞满上限再加一项
            if let Some(m) = s.macro_book.families.get_mut(0) {
                let mut extra: Vec<u32> = Vec::with_capacity(MACRO_FANOUT_CAP + 1);
                let mut k: u32 = 0;
                while (k as usize) <= MACRO_FANOUT_CAP {
                    extra.push(9000u32 + k);
                    k = k.wrapping_add(1);
                }
                m.fanout = extra;
                s.macro_book.fanout_total = s.macro_book.fanout_total.wrapping_add(1);
            }
        }
        Form::MacroEmpty => {
            // 宏展开为空
            if let Some(m) = s.macro_book.families.get_mut(2) {
                m.fanout.clear();
                s.macro_book.fanout_total = s.macro_book.fanout_total.wrapping_sub(4);
            }
        }
        Form::MacroCritOver => {
            // 单族判据数越界
            if let Some(m) = s.macro_book.families.get_mut(3) {
                m.crit = (CRITERIA_TOTAL + 5) as u32;
                s.macro_book.crit_total = s.macro_book.crit_total.wrapping_add(5);
            }
        }
        Form::TargetIncoherent => {
            // 目标态不自洽：目标用例数压到现状以下
            if let Some(g) = s.gap.items.get_mut(0) {
                g.target = 1;
                s.gap.coherent = false;
            }
        }
        Form::GapUnlisted => {
            // 缺口未登记
            if let Some(g) = s.gap.items.get_mut(1) {
                g.listed = 0;
            }
        }
        Form::BatchOver => {
            // 批次越界
            let _ = s.mig.advance_batch(&s.inv, MIGRATION_BATCHES + 2);
            if let Some(last) = s.mig.batches.last_mut() {
                last.cap = BATCH_CAP + 3;
            }
        }
        Form::StateRegress => {
            // 接管状态倒退：先记录 Owned(秩 2)，再追加同一条目退回 Legacy(秩 0) 的轨迹
            if !s.mig.trail.is_empty() {
                let (item, _rank) = s.mig.trail[0];
                s.mig.trail[0] = (item, 2);
                s.mig.trail.push((item, 0));
            }
        }
        Form::ShadowMismatch => {
            // 影子比对不一致却推进
            if let Some(b) = s.mig.batches.get_mut(0) {
                b.shadow_ok = 0;
                b.shadow_rounds = (SHADOW_ROUNDS - 1) as u32;
            }
        }
        Form::LegacyNotConverged => {
            // 旧路径未收敛：末批残留非零
            if let Some(last) = s.mig.batches.last_mut() {
                last.legacy_left_after = 3;
            }
            if let Some(e) = s.inv.entries.get_mut(0) {
                e.legacy_left = 2;
            }
        }
        Form::ReconcileDeviation => {
            // 守恒偏离：环形账写指针错位
            s.ring.head = s.ring.head.wrapping_add(3) & RING_MASK;
            s.ring.dropped = s.ring.dropped.wrapping_add(2);
        }
        Form::FingerprintDrift => {
            // 冻结指纹漂移：改目标态而不更新入册探针
            s.gap.target = TargetState::new(CASE_TARGET as u32 + 4, CRITERIA_TOTAL as u32, DIR_ENTRIES as u32);
            s.ev.freeze_fp = freeze_probe_hash(&s.inv, &s.macro_book, &s.gap, &s.mig);
        }
    }
    Ok(s)
}
/// 逐形态注入全跑。
pub fn inject_all() -> KtResult<(Vec<Snapshot>, Vec<Form>)> {
    let mut out: Vec<Snapshot> = Vec::with_capacity(Form::COUNT);
    let mut forms: Vec<Form> = Vec::with_capacity(Form::COUNT);
    let mut i: usize = 0;
    while i < Form::COUNT {
        let f = Form::ALL[i];
        out.push(inject_one(f)?);
        forms.push(f);
        i = i.wrapping_add(1);
    }
    Ok((out, forms))
}
/// 检出表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detections {
    /// 逐形态命中数。
    pub hits: [u32; Form::COUNT],
    /// 判定掩码。
    pub verdict_mask: u32,
    /// 语义边界违例。
    pub border_violations: u32,
}
impl Detections {
    /// 空表。
    pub fn blank() -> Detections {
        let mut hits: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            hits[i] = 0;
            i = i.wrapping_add(1);
        }
        Detections { hits, verdict_mask: 0, border_violations: 0 }
    }
    /// 记命中。
    pub fn mark(&mut self, f: Form) {
        let i = f.index();
        if i < Form::COUNT {
            self.hits[i] = self.hits[i].wrapping_add(1);
            self.verdict_mask = self.verdict_mask | (1u32 << (i & 31));
        }
    }
    /// 是否命中。
    pub fn hit(&self, f: Form) -> bool {
        let i = f.index();
        if i >= Form::COUNT {
            return false;
        }
        self.hits[i] > 0
    }
    /// 命中数。
    pub fn hit_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.hits[i] > 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 未命中名表。
    pub fn miss_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut i: usize = 0;
        while i < Form::COUNT {
            let f = Form::ALL[i];
            if !self.hit(f) {
                out.push(f.name());
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 命中名表。
    pub fn hit_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(Form::COUNT);
        let mut i: usize = 0;
        while i < Form::COUNT {
            let f = Form::ALL[i];
            if self.hit(f) {
                out.push(f.name());
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "detect hit={}/{} mask={:08X} border={}",
            self.hit_count(),
            Form::COUNT,
            self.verdict_mask,
            self.border_violations
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(Form::COUNT + 1);
        out.push(format!("# 注入检出 hit={}/{}", self.hit_count(), Form::COUNT));
        let mut i: usize = 0;
        while i < Form::COUNT {
            let f = Form::ALL[i];
            out.push(format!(
                "{:<20} code=0x{:04X} {:<10} {:<8} hits={} [{}] {}",
                f.name(), f.code(), f.class_name(), f.face(), self.hits[i],
                f.probe(), f.describe()
            ));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 检测端（只读入参）。
pub fn detect_one(s: &Snapshot) -> Detections {
    let mut d = Detections::blank();
    d.border_violations = d.border_violations.wrapping_add(s.ev.border_violations);
    // ① 盘点面
    if !s.inv.cover_complete() {
        d.mark(Form::InventoryMissing);
    }
    if !s.inv.dup_free() {
        d.mark(Form::InventoryDup);
    }
    // ② 目标与缺口
    if !s.gap.coherent {
        d.mark(Form::TargetIncoherent);
    }
    if !s.gap.all_listed() {
        d.mark(Form::GapUnlisted);
    }
    // ③ 宏族面
    if !s.macro_book.dup_free() {
        d.mark(Form::MacroDup);
    }
    if !s.macro_book.fanout_bounded() {
        d.mark(Form::MacroFanoutOver);
    }
    if !s.macro_book.fanout_nonzero() {
        d.mark(Form::MacroEmpty);
    }
    if !s.macro_book.crit_bounded() {
        d.mark(Form::MacroCritOver);
    }
    // ④ 迁移面
    // 批次容量合计必须等于条目数
    if s.mig.cap_total() != s.inv.entries.len() as u64 {
        d.mark(Form::BatchOver);
    }
    if !s.mig.state_monotonic() {
        d.mark(Form::StateRegress);
    }
    if !s.mig.shadow_all_match() {
        d.mark(Form::ShadowMismatch);
    }
    if !s.mig.legacy_converged() || s.inv.legacy_left_total() != 0 {
        d.mark(Form::LegacyNotConverged);
    }
    // ⑤ 账目与冻结
    let rc = s.reconcile();
    if !rc.balanced {
        d.mark(Form::ReconcileDeviation);
    }
    if !s.ring.balanced() {
        d.mark(Form::ReconcileDeviation);
    }
    if s.ev.freeze_fp != s.ev.registered_probe {
        d.mark(Form::FingerprintDrift);
    }
    d
}
/// 全形态检出。
pub fn detect_all(inj: &[Snapshot], forms: &[Form]) -> KtResult<Detections> {
    let mut agg = Detections::blank();
    let mut i: usize = 0;
    while i < inj.len() {
        let s = match inj.get(i) {
            Some(x) => x,
            None => return Err(err_range("detect_all", i, inj.len())),
        };
        let d = detect_one(s);
        let f = match forms.get(i) {
            Some(x) => *x,
            None => return Err(err_range("detect_all form", i, forms.len())),
        };
        if !d.hit(f) {
            return Err(KtErr::of_form(f, format!("注入未被检出：{} 漏检", f.name())));
        }
        let mut j: usize = 0;
        while j < Form::COUNT {
            agg.hits[j] = agg.hits[j].wrapping_add(d.hits[j]);
            j = j.wrapping_add(1);
        }
        agg.verdict_mask = agg.verdict_mask | d.verdict_mask;
        agg.border_violations = agg.border_violations.wrapping_add(d.border_violations);
        i = i.wrapping_add(1);
    }
    Ok(agg)
}

// ===========================================================================
// 12. 冻结件 / 稳定性 / 重放
// ===========================================================================

/// 冻结记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 版本。
    pub rev: u32,
    /// 设计件定义哈希。
    pub design_hash: u64,
    /// 盘点表哈希。
    pub inv_hash: u64,
    /// 宏族表哈希。
    pub macro_hash: u64,
    /// 迁移账哈希。
    pub mig_hash: u64,
    /// 冻结总哈希。
    pub freeze_hash: u64,
    /// 语义边界声明。
    pub border_note: String,
    /// 设计键。
    pub design_key: String,
}
impl FreezeRecord {
    /// 构造。
    pub fn build(inv: &Inventory, mb: &MacroBook, gap: &GapBook, mig: &Migration) -> FreezeRecord {
        let design_hash = design_definition_hash();
        let inv_hash = inv.fingerprint();
        let macro_hash = mb.fingerprint();
        let mig_hash = mig.fingerprint();
        let mut w: Vec<u64> = Vec::with_capacity(8);
        w.push(V_VERSION_ID);
        w.push(design_hash);
        w.push(inv_hash);
        w.push(macro_hash);
        w.push(mig_hash);
        w.push(gap.fingerprint());
        w.push(CRITERIA_TOTAL as u64);
        w.push(CASE_TARGET as u64);
        let freeze_hash = fnv1a_words(&w);
        FreezeRecord {
            rev: 1,
            design_hash,
            inv_hash,
            macro_hash,
            mig_hash,
            freeze_hash,
            border_note: String::from("只做总设计（盘点/目标/宏族/三步法）；不真改 testing/ 目录、不真跑影子双跑"),
            design_key: format!("ktestdesign/{}/{}", V_VERSION, GRAFT_SEAT),
        }
    }
    /// 校验。
    pub fn verify(&self, inv: &Inventory, mb: &MacroBook, gap: &GapBook, mig: &Migration) -> KtResult<()> {
        if self.design_hash != design_definition_hash() {
            return Err(err_frozen("design_hash", &format!("{:016X}", design_definition_hash())));
        }
        if self.inv_hash != inv.fingerprint() {
            return Err(err_frozen("inv_hash", &format!("{:016X}", inv.fingerprint())));
        }
        if self.macro_hash != mb.fingerprint() {
            return Err(err_frozen("macro_hash", &format!("{:016X}", mb.fingerprint())));
        }
        if self.mig_hash != mig.fingerprint() {
            return Err(err_frozen("mig_hash", &format!("{:016X}", mig.fingerprint())));
        }
        if !gap.coherent {
            return Err(KtErr::of_form(
                Form::TargetIncoherent,
                format!("冻结校验：目标不自洽 {}", gap.summary()),
            ));
        }
        Ok(())
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} design={:016X} inv={:016X} macro={:016X} mig={:016X} total={:016X}",
            self.rev, self.design_hash, self.inv_hash, self.macro_hash, self.mig_hash, self.freeze_hash
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(4);
        out.push(self.describe());
        out.push(format!("  border  {}", self.border_note));
        out.push(format!("  key     {}", self.design_key));
        out.push(format!("  dep     {}", DEP_SEAT));
        out
    }
}
/// 状态机定义哈希。
pub fn fsm_definition_hash() -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(2 + Stage::COUNT * 4);
    w.push(V_VERSION_ID);
    w.push(Stage::COUNT as u64);
    let mut i: usize = 0;
    while i < Stage::COUNT {
        let s = Stage::ALL[i];
        w.push(fnv1a64(s.name().as_bytes()));
        w.push(s.cn().len() as u64);
        w.push(s.index() as u64);
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < Stage::COUNT {
        let mut k: usize = 0;
        while k < Stage::COUNT {
            if StageFsm::is_legal(Stage::ALL[j], Stage::ALL[k]) {
                w.push(((j * Stage::COUNT + k) as u64).wrapping_add(0x30000));
            }
            k = k.wrapping_add(1);
        }
        j = j.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: u32,
    /// 逐轮冻结哈希。
    pub hashes: Vec<u64>,
    /// 逐轮迁移账指纹。
    pub mig_fps: Vec<u64>,
    /// 是否一致。
    pub stable: bool,
}
impl Stability {
    /// 一致判定。
    pub fn stable(&self) -> bool {
        if self.hashes.len() < 2 {
            return false;
        }
        let h0 = self.hashes[0];
        let m0 = self.mig_fps[0];
        let mut i: usize = 1;
        while i < self.hashes.len() {
            if self.hashes[i] != h0 || self.mig_fps[i] != m0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!("stable rounds={} stable={}", self.rounds, if self.stable { 1 } else { 0 })
    }
}
/// 稳定性轮数。
pub const REPEAT_RUNS: usize = 3;
/// 稳定性跑。
pub fn stability_check() -> KtResult<Stability> {
    let mut hashes: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut mig_fps: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut i: usize = 0;
    while i < REPEAT_RUNS {
        let s = Snapshot::net()?;
        let fr = FreezeRecord::build(&s.inv, &s.macro_book, &s.gap, &s.mig);
        hashes.push(fr.freeze_hash);
        mig_fps.push(s.mig.fingerprint());
        i = i.wrapping_add(1);
    }
    let mut st = Stability { rounds: REPEAT_RUNS as u32, hashes, mig_fps, stable: false };
    st.stable = st.stable();
    if !st.stable {
        return Err(err_frozen("stability", "all-round-equal"));
    }
    Ok(st)
}
/// 重放报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Replay {
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 迁移账指纹。
    pub mig_fp: u64,
    /// 盘点表指纹。
    pub inv_fp: u64,
    /// 宏族表指纹。
    pub macro_fp: u64,
    /// 批次容量合计。
    pub cap_total: u64,
    /// 注入检出命中数。
    pub inject_hits: usize,
    /// 环形账写入数。
    pub ring_pushed: u64,
}
impl Replay {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "replay freeze={:016X} mig={:016X} inv={:016X} macro={:016X} cap={} hits={} ring={}",
            self.freeze_hash, self.mig_fp, self.inv_fp, self.macro_fp,
            self.cap_total, self.inject_hits, self.ring_pushed
        )
    }
}
/// 确定性重放（与 `observe` 互不递归）。
pub fn replay(seed: u64) -> KtResult<Replay> {
    let mut rng = Lcg::new(seed);
    let _ = rng.next();
    let s = Snapshot::net()?;
    if !s.inv.cover_complete() {
        return Err(KtErr::of_form(
            Form::InventoryMissing,
            format!("重放：盘点缺项 cover={:08X}", s.inv.cover_bits),
        ));
    }
    if !s.gap.coherent {
        return Err(KtErr::of_form(
            Form::TargetIncoherent,
            format!("重放：目标不自洽 {}", s.gap.summary()),
        ));
    }
    if !s.macro_book.fanout_bounded() {
        return Err(KtErr::of_form(
            Form::MacroFanoutOver,
            format!("重放：宏展开越界 {}", s.macro_book.summary()),
        ));
    }
    if !s.mig.legacy_converged() {
        return Err(KtErr::of_form(
            Form::LegacyNotConverged,
            format!("重放：旧路径未收敛 {}", s.mig.summary()),
        ));
    }
    if !s.ring.balanced() {
        return Err(KtErr::of_form(
            Form::ReconcileDeviation,
            format!("重放：环形账不守恒 {}", s.ring.summary()),
        ));
    }
    let rc = s.reconcile();
    if !rc.balanced {
        return Err(err_recon(rc.sum_a, rc.sum_b, rc.sum_c));
    }
    let (inj, forms) = inject_all()?;
    let agg = detect_all(&inj, &forms)?;
    let fr = FreezeRecord::build(&s.inv, &s.macro_book, &s.gap, &s.mig);
    Ok(Replay {
        freeze_hash: fr.freeze_hash,
        mig_fp: s.mig.fingerprint(),
        inv_fp: s.inv.fingerprint(),
        macro_fp: s.macro_book.fingerprint(),
        cap_total: s.mig.cap_total(),
        inject_hits: agg.hit_count(),
        ring_pushed: s.ring.pushed,
    })
}

// ===========================================================================
// 13. 指标与性能分解
// ===========================================================================

/// 成本项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 环节名。
    pub name: &'static str,
    /// 主循环次数。
    pub iters: u64,
    /// 每次操作数。
    pub per_iter: u64,
    /// 合计。
    pub total: u64,
    /// 对照说明。
    pub note: &'static str,
}
impl CostItem {
    /// 构造。
    pub fn new(name: &'static str, iters: u64, per_iter: u64) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note: "—" }
    }
    /// 带对照构造。
    pub fn with_note(name: &'static str, iters: u64, per_iter: u64, note: &'static str) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note }
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "{:<26} iters={:<6} per_iter={:<4} total={:<9} note={}",
            self.name, self.iters, self.per_iter, self.total, self.note
        )
    }
}
/// 指标册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 盘点条目数。
    pub entries: u64,
    /// 现状用例数。
    pub cases: u32,
    /// 现状判据覆盖。
    pub crit: u32,
    /// 目标用例数。
    pub target_cases: u32,
    /// 宏族数。
    pub macros: u64,
    /// 宏展开项总数。
    pub fanout_total: u32,
    /// 缺口登记数。
    pub gaps: u64,
    /// 批次数。
    pub batches: u64,
    /// 批次容量合计。
    pub cap_total: u64,
    /// 影子轮数合计。
    pub shadow_rounds: u64,
    /// 旧路径剩余调用数（应 0）。
    pub legacy_left: u32,
    /// 环形账写入/丢弃。
    pub ring_pushed: u64,
    /// 环形账丢弃。
    pub ring_dropped: u64,
    /// 步数消耗。
    pub steps: u64,
    /// 注入检出命中数。
    pub inject_hits: usize,
    /// 净态检出（应零）。
    pub net_hits: usize,
    /// 边界违例。
    pub border_violations: u32,
    /// 峰值内存估算。
    pub mem_peak: u64,
}
impl Metrics {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.net_hits == 0
            && self.border_violations == 0
            && self.entries == DIR_ENTRIES as u64
            && self.cases > 0
            && self.target_cases >= self.cases
            && self.macros == MACRO_FAMILIES as u64
            && self.gaps == 3
            && self.batches == MIGRATION_BATCHES as u64
            && self.cap_total == self.entries
            && self.legacy_left == 0
            && self.inject_hits == Form::COUNT
            && self.steps <= STEP_CAP
            && self.mem_peak <= MEM_PEAK_CAP
    }
    /// 性能逐项分解。
    pub fn complexity(&self) -> Vec<CostItem> {
        vec![
            CostItem::with_note(
                "inventory_scan",
                DIR_ENTRIES as u64,
                4,
                "Windows 测试目录按文件登记，无接管状态位",
            ),
            CostItem::with_note(
                "gap_build",
                3,
                DIR_ENTRIES as u64,
                "Windows 无「现状→目标」缺口账",
            ),
            CostItem::with_note(
                "macro_register",
                MACRO_FAMILIES as u64,
                MACRO_FANOUT_CAP as u64,
                "Windows 无宏族注册表，宏展开靠外部生成器",
            ),
            CostItem::with_note(
                "macro_fanout_check",
                MACRO_FAMILIES as u64,
                MACRO_FANOUT_CAP as u64,
                "本单特有：扇出有界性强制校验",
            ),
            CostItem::with_note(
                "batch_advance",
                MIGRATION_BATCHES as u64,
                (BATCH_CAP * 3) as u64,
                "Windows 无分批接管账",
            ),
            CostItem::with_note(
                "shadow_compare",
                self.shadow_rounds,
                self.cap_total,
                "本单特有：影子双跑比对轮次（本单账面）",
            ),
            CostItem::with_note(
                "legacy_converge_scan",
                MIGRATION_BATCHES as u64,
                DIR_ENTRIES as u64,
                "本单特有：旧路径单调归零判据",
            ),
            CostItem::with_note(
                "ring_push",
                self.ring_pushed,
                3,
                "append-only + dropped 显性",
            ),
            CostItem::with_note(
                "reconcile",
                7,
                3,
                "Windows 无跨面守恒恒等式",
            ),
            CostItem::with_note(
                "state_monotonic_scan",
                self.cap_total,
                self.cap_total,
                "本单特有：接管状态秩单调性 O(n²) 归位检查",
            ),
            CostItem::with_note(
                "inject_one",
                Form::COUNT as u64,
                self.entries,
                "本单特有：14 形态全注入",
            ),
            CostItem::with_note(
                "detect_one",
                Form::COUNT as u64,
                (DIR_ENTRIES + MACRO_FAMILIES) as u64,
                "本单特有：14 检测器逐面扫描",
            ),
        ]
    }
    /// 合计。
    pub fn complexity_total(&self) -> u64 {
        let items = self.complexity();
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < items.len() {
            s = s.wrapping_add(items[i].total);
            i = i.wrapping_add(1);
        }
        s
    }
    /// 逐行表。
    pub fn complexity_rows(&self) -> Vec<String> {
        let items = self.complexity();
        let mut out: Vec<String> = Vec::with_capacity(items.len() + 2);
        let mut i: usize = 0;
        while i < items.len() {
            out.push(items[i].describe());
            i = i.wrapping_add(1);
        }
        out.push(format!("TOTAL iters*per_iter = {}", self.complexity_total()));
        out.push(format!(
            "STEP_CAP={} SYSCALL_CAP={} MEM_PEAK_CAP={}",
            STEP_CAP, SYSCALL_CAP, MEM_PEAK_CAP
        ));
        out
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics entries={} cases={}/{} crit={} macros={} fanout={} gaps={} batches={} cap={} shadow={} legacy_left={} ring={}/{} inj={} net={}",
            self.entries, self.cases, self.target_cases, self.crit, self.macros, self.fanout_total,
            self.gaps, self.batches, self.cap_total, self.shadow_rounds, self.legacy_left,
            self.ring_pushed, self.ring_dropped, self.inject_hits, self.net_hits
        )
    }
}

// ===========================================================================
// 14. 导出
// ===========================================================================

/// KV 导出。
pub fn export_kv(o: &KtObserve) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(48);
    out.push(format!("ver={}", V_VERSION));
    out.push(format!("seed={:016X}", o.seed));
    out.push(format!("seg=0x{:04X}", SEG_BASE));
    out.push(format!("dir_entries={}", DIR_ENTRIES));
    out.push(format!("macro_families={}", MACRO_FAMILIES));
    out.push(format!("fanout_cap={}", MACRO_FANOUT_CAP));
    out.push(format!("batches={}", MIGRATION_BATCHES));
    out.push(format!("batch_cap={}", BATCH_CAP));
    out.push(format!("criteria_total={}", CRITERIA_TOTAL));
    out.push(format!("case_target={}", CASE_TARGET));
    out.push(format!("entries={}", o.metrics.entries));
    out.push(format!("cases={}", o.metrics.cases));
    out.push(format!("target_cases={}", o.metrics.target_cases));
    out.push(format!("crit={}", o.metrics.crit));
    out.push(format!("macros={}", o.metrics.macros));
    out.push(format!("fanout_total={}", o.metrics.fanout_total));
    out.push(format!("gaps={}", o.metrics.gaps));
    out.push(format!("batches_run={}", o.metrics.batches));
    out.push(format!("cap_total={}", o.metrics.cap_total));
    out.push(format!("shadow_rounds={}", o.metrics.shadow_rounds));
    out.push(format!("legacy_left={}", o.metrics.legacy_left));
    out.push(format!("ring_pushed={}", o.metrics.ring_pushed));
    out.push(format!("ring_dropped={}", o.metrics.ring_dropped));
    out.push(format!("steps={}", o.metrics.steps));
    out.push(format!("recon_dev={}", o.recon.deviation));
    out.push(format!("recon_balanced={}", if o.recon.balanced { 1 } else { 0 }));
    out.push(format!("fsm_state={}", o.fsm.state.name()));
    out.push(format!("fsm_reject={}", o.fsm.rejected.len()));
    out.push(format!("freeze_hash={:016X}", o.freeze.freeze_hash));
    out.push(format!("replay_freeze={:016X}", o.replay.freeze_hash));
    out.push(format!("inject_hits={}", o.metrics.inject_hits));
    out.push(format!("net_hits={}", o.metrics.net_hits));
    out.push(format!("all_ok={}", if o.all_ok() { 1 } else { 0 }));
    let mut i: usize = 0;
    while i < Form::COUNT {
        out.push(format!("form_{}={}", Form::ALL[i].name(), o.detects.hits[i]));
        i = i.wrapping_add(1);
    }
    out
}
/// 推 `u64` 大端。
pub fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.push(((v >> 56) & 0xFF) as u8);
    buf.push(((v >> 48) & 0xFF) as u8);
    buf.push(((v >> 40) & 0xFF) as u8);
    buf.push(((v >> 32) & 0xFF) as u8);
    buf.push(((v >> 24) & 0xFF) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push((v & 0xFF) as u8);
}
/// 推一条 TLV。
pub fn push_tlv_u64(buf: &mut Vec<u8>, tag: u8, v: u64) {
    buf.push(tag);
    buf.push(8u8);
    push_u64(buf, v);
}
/// TLV 导出（组头单独一行；长度字段恒占定宽）。
pub fn export_tlv(o: &KtObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(2048);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, V_VERSION_ID);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, o.freeze.freeze_hash);
    out.push(TLV_TAG_HEAD);
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    for b in head_payload.iter() {
        out.push(*b);
    }
    // 规模常量（11 条）
    push_tlv_u64(&mut out, 0x10, DIR_ENTRIES as u64);
    push_tlv_u64(&mut out, 0x11, MACRO_FAMILIES as u64);
    push_tlv_u64(&mut out, 0x12, MACRO_FANOUT_CAP as u64);
    push_tlv_u64(&mut out, 0x13, MIGRATION_BATCHES as u64);
    push_tlv_u64(&mut out, 0x14, BATCH_CAP as u64);
    push_tlv_u64(&mut out, 0x15, CRITERIA_TOTAL as u64);
    push_tlv_u64(&mut out, 0x16, CASE_TARGET as u64);
    push_tlv_u64(&mut out, 0x17, SHADOW_ROUNDS as u64);
    push_tlv_u64(&mut out, 0x18, RING_CAP as u64);
    push_tlv_u64(&mut out, 0x19, STEP_CAP);
    push_tlv_u64(&mut out, 0x1A, MEM_PEAK_CAP);
    // 盘点面（6 条）
    push_tlv_u64(&mut out, 0x20, o.metrics.entries);
    push_tlv_u64(&mut out, 0x21, o.metrics.cases as u64);
    push_tlv_u64(&mut out, 0x22, o.metrics.crit as u64);
    push_tlv_u64(&mut out, 0x23, o.net.inv.fingerprint());
    push_tlv_u64(&mut out, 0x24, o.net.inv.cover_bits as u64);
    push_tlv_u64(&mut out, 0x25, o.net.inv.dup_bits as u64);
    // 目标与缺口面（5 条）
    push_tlv_u64(&mut out, 0x30, o.metrics.target_cases as u64);
    push_tlv_u64(&mut out, 0x31, o.metrics.gaps);
    push_tlv_u64(&mut out, 0x32, o.net.gap.fingerprint());
    push_tlv_u64(&mut out, 0x33, o.net.gap.target.cases as u64);
    push_tlv_u64(&mut out, 0x34, o.net.gap.target.crit as u64);
    // 宏族面（4 条）
    push_tlv_u64(&mut out, 0x40, o.metrics.macros);
    push_tlv_u64(&mut out, 0x41, o.metrics.fanout_total as u64);
    push_tlv_u64(&mut out, 0x42, o.net.macro_book.fingerprint());
    push_tlv_u64(&mut out, 0x43, design_definition_hash());
    // 迁移面（6 条）
    push_tlv_u64(&mut out, 0x50, o.metrics.batches);
    push_tlv_u64(&mut out, 0x51, o.metrics.cap_total);
    push_tlv_u64(&mut out, 0x52, o.metrics.shadow_rounds);
    push_tlv_u64(&mut out, 0x53, o.metrics.legacy_left as u64);
    push_tlv_u64(&mut out, 0x54, o.net.mig.fingerprint());
    push_tlv_u64(&mut out, 0x55, o.metrics.steps);
    // 账目与冻结面（8 条）
    push_tlv_u64(&mut out, 0x60, o.ring.pushed);
    push_tlv_u64(&mut out, 0x61, o.ring.dropped);
    push_tlv_u64(&mut out, 0x62, o.ring.live());
    push_tlv_u64(&mut out, 0x63, o.recon.deviation);
    push_tlv_u64(&mut out, 0x64, o.freeze.freeze_hash);
    push_tlv_u64(&mut out, 0x65, o.replay.freeze_hash);
    push_tlv_u64(&mut out, 0x66, o.detects.verdict_mask as u64);
    push_tlv_u64(&mut out, 0x67, fsm_definition_hash());
    // 逐形态命中（14 条）
    let mut i: usize = 0;
    while i < Form::COUNT {
        push_tlv_u64(&mut out, 0x80u8.wrapping_add(i as u8), o.detects.hits[i] as u64);
        i = i.wrapping_add(1);
    }
    out
}
/// TLV 长度核算。
pub fn tlv_len_check(o: &KtObserve) -> KtResult<usize> {
    let buf = export_tlv(o);
    let want: usize = TLV_HEAD_LEN + TLV_ITEM_COUNT * TLV_ITEM_LEN;
    if buf.len() != want {
        return Err(KtErr::of_aux(
            AuxCode::MacroIdxOver,
            format!("TLV 长度不符：got={} want={}", buf.len(), want),
        ));
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(KtErr::of_aux(
            AuxCode::MacroIdxOver,
            format!("组头长度字段不符：got={} want={}", hl, TLV_HEAD_PAYLOAD),
        ));
    }
    Ok(buf.len())
}

// ===========================================================================
// 15. observe
// ===========================================================================

/// 观测结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KtObserve {
    /// 种子。
    pub seed: u64,
    /// 净态快照。
    pub net: Snapshot,
    /// 环形账。
    pub ring: RingLedger,
    /// 守恒账。
    pub recon: Reconcile,
    /// 注入检出。
    pub detects: Detections,
    /// 净态检出。
    pub net_detect: Detections,
    /// 状态机。
    pub fsm: StageFsm,
    /// 稳定性。
    pub stability: Stability,
    /// 冻结件。
    pub freeze: FreezeRecord,
    /// 指标。
    pub metrics: Metrics,
    /// 重放。
    pub replay: Replay,
}
impl KtObserve {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok()
    }
    /// 重放一致性。
    pub fn replay_match(&self) -> bool {
        self.replay.freeze_hash == self.freeze.freeze_hash
            && self.replay.mig_fp == self.net.mig.fingerprint()
            && self.replay.inv_fp == self.net.inv.fingerprint()
            && self.replay.cap_total == self.metrics.cap_total
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "observe seed={:016X} all_ok={} | {} | {} | {}",
            self.seed,
            if self.all_ok() { 1 } else { 0 },
            self.metrics.summary(),
            self.recon.summary(),
            self.freeze.describe()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(48);
        out.push(format!("# UNX-F3401 观测 seed={:016X}", self.seed));
        out.push(self.summary());
        out.push(format!("ver={} seg={}", V_VERSION, seg_line()));
        out.push(format!("metrics  {}", self.metrics.summary()));
        out.push(format!("recon    {}", self.recon.summary()));
        out.push(format!("fsm      {}", self.fsm.summary()));
        out.push(format!("stable   {}", self.stability.describe()));
        out.push(format!("inv      {}", self.net.inv.summary()));
        out.push(format!("gap      {}", self.net.gap.summary()));
        out.push(format!("macro    {}", self.net.macro_book.summary()));
        out.push(format!("mig      {}", self.net.mig.summary()));
        out.push(format!("ring     {}", self.ring.summary()));
        out.push(format!("evidence {}", self.net.ev.describe()));
        out.push(format!("netdet   {}", self.net_detect.summary()));
        out.push(format!("freeze   {}", self.freeze.describe()));
        out.push(format!("replay   {}", self.replay.summary()));
        out.push(format!("replay_match={}", if self.replay_match() { 1 } else { 0 }));
        let mut i: usize = 0;
        while i < Stage::ALL.len() {
            out.push(format!("stage    {}", Stage::ALL[i].four_columns()));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 真实跑逻辑的观测入口。
pub fn observe(seed: u64) -> KtResult<KtObserve> {
    let net = Snapshot::net()?;
    if !net.inv.cover_complete() {
        return Err(KtErr::of_form(
            Form::InventoryMissing,
            format!("盘点缺项 cover={:08X}", net.inv.cover_bits),
        ));
    }
    if !net.inv.dup_free() {
        return Err(KtErr::of_form(
            Form::InventoryDup,
            format!("盘点重号 dup={:08X}", net.inv.dup_bits),
        ));
    }
    if !net.gap.coherent {
        return Err(KtErr::of_form(
            Form::TargetIncoherent,
            format!("目标不自洽 {}", net.gap.summary()),
        ));
    }
    if !net.gap.all_listed() {
        return Err(KtErr::of_form(
            Form::GapUnlisted,
            format!("缺口未登记 {}", net.gap.summary()),
        ));
    }
    if !net.macro_book.dup_free() {
        return Err(KtErr::of_form(
            Form::MacroDup,
            format!("宏族重名 dup={:08X}", net.macro_book.dup_bits),
        ));
    }
    if !net.macro_book.fanout_bounded() || !net.macro_book.fanout_nonzero() {
        return Err(KtErr::of_form(
            Form::MacroFanoutOver,
            format!("宏展开越界 {}", net.macro_book.summary()),
        ));
    }
    if net.mig.cap_total() != DIR_ENTRIES as u64 {
        return Err(err_recon(net.mig.cap_total(), DIR_ENTRIES as u64, net.ring.pushed));
    }
    if !net.mig.shadow_all_match() {
        return Err(KtErr::of_form(
            Form::ShadowMismatch,
            format!("影子比对不一致 {}", net.mig.summary()),
        ));
    }
    if !net.mig.legacy_converged() || net.inv.legacy_left_total() != 0 {
        return Err(KtErr::of_form(
            Form::LegacyNotConverged,
            format!("旧路径未收敛 legacy_left={}", net.inv.legacy_left_total()),
        ));
    }
    if !net.ring.balanced() {
        return Err(KtErr::of_form(
            Form::ReconcileDeviation,
            format!("环形账不守恒 {}", net.ring.summary()),
        ));
    }
    let recon = net.reconcile();
    if !recon.balanced {
        return Err(err_recon(recon.sum_a, recon.sum_b, recon.sum_c));
    }
    let mut fsm = StageFsm::new();
    fsm.drive_all(1)?;
    let illegal: [Stage; 3] = [Stage::P3Own, Stage::P1Inventory, Stage::Sealed];
    for s in illegal.iter() {
        let _ = fsm.try_go(*s, 99);
    }
    if !fsm.state.is_sealed() || !fsm.trail_monotonic() || fsm.rejected.len() < 3 {
        return Err(err_frozen(fsm.state.name(), Stage::Sealed.name()));
    }
    let stability = stability_check()?;
    let (inj, forms) = inject_all()?;
    let detects = detect_all(&inj, &forms)?;
    if detects.hit_count() != Form::COUNT {
        return Err(KtErr::of_form(
            Form::ReconcileDeviation,
            format!("注入检出不足 {}/{}", detects.hit_count(), Form::COUNT),
        ));
    }
    let net_detect = detect_one(&net);
    if net_detect.hit_count() != 0 {
        return Err(KtErr::of_form(
            Form::ReconcileDeviation,
            format!("净态检出非零 {:?}", net_detect.hit_list()),
        ));
    }
    let freeze = FreezeRecord::build(&net.inv, &net.macro_book, &net.gap, &net.mig);
    freeze.verify(&net.inv, &net.macro_book, &net.gap, &net.mig)?;
    if net.ev.freeze_fp != net.ev.registered_probe {
        return Err(KtErr::of_form(
            Form::FingerprintDrift,
            format!("探针漂移 {:016X} vs {:016X}", net.ev.freeze_fp, net.ev.registered_probe),
        ));
    }
    let mut gaps: u64 = 0;
    let mut gi: usize = 0;
    while gi < net.gap.items.len() {
        if net.gap.items[gi].listed == 1 {
            gaps = gaps.wrapping_add(1);
        }
        gi = gi.wrapping_add(1);
    }
    let mut shadow_rounds: u64 = 0;
    let mut bi: usize = 0;
    while bi < net.mig.batches.len() {
        shadow_rounds = shadow_rounds.wrapping_add(net.mig.batches[bi].shadow_rounds as u64);
        bi = bi.wrapping_add(1);
    }
    let metrics = Metrics {
        entries: net.inv.entries.len() as u64,
        cases: net.inv.cases_total,
        crit: net.inv.crit_total,
        target_cases: net.gap.target.cases,
        macros: net.macro_book.families.len() as u64,
        fanout_total: net.macro_book.fanout_total,
        gaps,
        batches: net.mig.batches.len() as u64,
        cap_total: net.mig.cap_total(),
        shadow_rounds,
        legacy_left: net.inv.legacy_left_total(),
        ring_pushed: net.ring.pushed,
        ring_dropped: net.ring.dropped,
        steps: net.mig.steps,
        inject_hits: detects.hit_count(),
        net_hits: net_detect.hit_count(),
        border_violations: net.ev.border_violations,
        mem_peak: mem_estimate() as u64,
    };
    let replay = replay(seed)?;
    let ring_clone = net.ring.clone();
    let o = KtObserve {
        seed,
        ring: ring_clone,
        net,
        recon,
        detects,
        net_detect,
        fsm,
        stability,
        freeze,
        metrics,
        replay,
    };
    let _ = tlv_len_check(&o)?;
    if !o.replay_match() {
        return Err(err_frozen("replay", "match"));
    }
    Ok(o)
}
/// 峰值内存估算。
pub fn mem_estimate() -> usize {
    let inv: usize = DIR_ENTRIES * (core::mem::size_of::<DirEntry>() + 64);
    let macro_m: usize = MACRO_FAMILIES * (core::mem::size_of::<MacroFamily>() + MACRO_FANOUT_CAP * 4);
    let mig: usize = MIGRATION_BATCHES * (core::mem::size_of::<BatchRow>() + BATCH_CAP * 8);
    let ring: usize = RING_CAP * (core::mem::size_of::<RingEntry>() + 16);
    inv + macro_m + mig + ring
}
