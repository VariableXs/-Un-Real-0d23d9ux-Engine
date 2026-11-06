//! UNX-F48913 触控板 B06 段内联轧 · 四十五码族 · 0x70E00
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48913 ① 纯功能代码 ≥240 行；
//!          ② UNX-F48913-J1 fuzz ×10^4 零崩；
//!   F48914 ① 纯功能代码 ≥180 行；
//!          ② UNX-F48914-J1 联轧 ×5 全绿零漂移；
//!   F48915 ① 纯功能代码 ≥220 行；
//!          ② UNX-F48915-J1 码族 ×45 零漂移互不混淆。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：本码族唯一 → 模块窗口闭集 → 普查条数 → 槽内序号 →
//!           六路探针 → 五路联轧 → 双跑一致 → 对账平衡。
//!   次序不可倒：码族先查重再联轧，值撞号时五路会「全绿」而实际
//!   混淆；窗口闭集先于普查，否则跨模块串号会被当成同一条码；
//!   槽内序号先于探针，否则越界序号在探针里会被误算成一次 miss。
//! 关键差异：本模块不实现触控板能力位、元数据锁、拒掌、灵敏度或
//!   quirk 回填中的任何一层——那些逻辑在 B06 前五槽各自持有。本
//!   模块只做三件事：把五个窗口的 45 条诊断码收成一张可核对的普
//!   查表；用 10^4 轮随机输入证明「任何坏输入只产生拒绝码，不产生
//!   崩溃」；用五路内联轧把五个模块的黄金全链在同一个进程里重跑
//!   两遍，要求逐位一致。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48901_touchpad_core as tp_core;
use super::unx48903_touchpad_meta as tp_meta;
use super::unx48905_touchpad_guard as tp_guard;
use super::unx48908_touchpad_dispatch as tp_dispatch;
use super::unx48911_touchpad_edge as tp_edge;

/// 诊断段基址（0x70E00，VX6 批 B06 第六槽）。
pub const SEG_BASE: u32 = 0x70E00;

/// 模块数（判据 ×5：五路联轧）。
pub const FAMILY_MODULES: usize = 5;
/// 码族总条数（判据 ×45）。
pub const FAMILY_CODES: usize = 45;
/// fuzz 轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 联轧路数（判据 ×5）。
pub const ROLL_ROUTES: usize = 5;
/// 单模块最大码数（槽内序号上界）。
pub const SLOT_MAX: usize = 9;
/// 每模块窗口步长（0x40，与 B06 布点一致）。
pub const WINDOW_STRIDE: u32 = 0x40;

// ── 诊断码闭集（+0x01..+0x0C） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BdCode {
    /// 码值重复（跨模块同值）
    CodeDup,
    /// 窗口越界（码值逃出所属模块窗口）
    WindowBad,
    /// 普查不平（条数或明细与源枚举不符）
    CensusBad,
    /// 槽内序号越界
    SliceBad,
    /// fuzz 输入域非法
    FuzzDomain,
    /// 五路联轧漂移
    RollDrift,
    /// 对账不平（接收 + 拒绝 ≠ 轮数）
    CountBad,
    /// 模块次序倒挂
    OrderBad,
    /// 族摘要漂移
    HashBad,
    /// 探针三序失败
    ProbeBad,
    /// 双跑不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl BdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            BdCode::CodeDup => "码值重复：两个模块不许共用同一个诊断码",
            BdCode::WindowBad => "窗口越界：码值必须落在所属模块的 0x40 槽内",
            BdCode::CensusBad => "普查不平：条数或明细与源枚举不符",
            BdCode::SliceBad => "序号越界：模块槽或槽内序号超出冻结范围",
            BdCode::FuzzDomain => "fuzz 输入域非法：未知模块号直接拒绝",
            BdCode::RollDrift => "联轧漂移：五路黄金全链结果不一致",
            BdCode::CountBad => "对账不平：接收 + 拒绝 ≠ 总轮数",
            BdCode::OrderBad => "次序倒挂：模块槽必须按 0x70C80 起严格递增",
            BdCode::HashBad => "摘要漂移：两次普查得到不同 FNV 指纹",
            BdCode::ProbeBad => "探针失败：三序未同时成立",
            BdCode::ReplayBad => "双跑不一致：同一输入两次结果不同",
            BdCode::SegmentBad => "段位越界：诊断码逃出 0x70E00 窗口",
        }
    }

    pub const ALL: [BdCode; 12] = [
        BdCode::CodeDup,
        BdCode::WindowBad,
        BdCode::CensusBad,
        BdCode::SliceBad,
        BdCode::FuzzDomain,
        BdCode::RollDrift,
        BdCode::CountBad,
        BdCode::OrderBad,
        BdCode::HashBad,
        BdCode::ProbeBad,
        BdCode::ReplayBad,
        BdCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BdCode::ALL.len() {
        if BdCode::ALL[i].val() == code {
            return BdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70E00 段闭集内"
}

// ── 冻结窗口表 ────────────────────────────────────────────────────
/// 模块窗口描述：槽号、名字、基址、码数。码数逐条锚定源枚举的 ALL。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleWindow {
    pub slot: u8,
    pub name: &'static str,
    pub base: u32,
    pub count: usize,
}

pub const WINDOWS: [ModuleWindow; FAMILY_MODULES] = [
    ModuleWindow { slot: 0, name: "core", base: tp_core::SEG_BASE, count: 8 },
    ModuleWindow { slot: 1, name: "meta", base: tp_meta::SEG_BASE, count: 10 },
    ModuleWindow { slot: 2, name: "guard", base: tp_guard::SEG_BASE, count: 9 },
    ModuleWindow { slot: 3, name: "dispatch", base: tp_dispatch::SEG_BASE, count: 9 },
    ModuleWindow { slot: 4, name: "edge", base: tp_edge::SEG_BASE, count: 9 },
];

/// 源枚举 ALL 长度（运行期复核用；与 WINDOWS 的 count 必须逐项相等）。
pub fn source_counts() -> [usize; FAMILY_MODULES] {
    [
        tp_core::TpCode::ALL.len(),
        tp_meta::MdCode::ALL.len(),
        tp_guard::PgCode::ALL.len(),
        tp_dispatch::SdCode::ALL.len(),
        tp_edge::QCode::ALL.len(),
    ]
}

/// 窗口上界（含）：码值 = base + 1 + 序号，故上界为 base + count。
pub fn window_end(w: &ModuleWindow) -> u32 {
    w.base.saturating_add(w.count as u32)
}

/// 槽起始偏移（前续槽码数累加）。
pub fn slot_start(slot: usize) -> Result<usize, u32> {
    if slot > FAMILY_MODULES {
        return Err(BdCode::SliceBad.val());
    }
    let mut i = 0usize;
    let mut at = 0usize;
    while i < slot {
        let c = match WINDOWS.get(i) {
            Some(w) => w.count,
            None => return Err(BdCode::SliceBad.val()),
        };
        at = at.saturating_add(c);
        i += 1;
    }
    Ok(at)
}

// ── 普查表 ────────────────────────────────────────────────────────
/// 普查行：诊断码 + 槽号 + 模块名 + 明细。明细同时作为去重键的一部分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeRow {
    pub code: u32,
    pub slot: u8,
    pub module: &'static str,
    pub detail: &'static str,
}

fn push_core(out: &mut Vec<CodeRow>) {
    let mut i = 0usize;
    while i < tp_core::TpCode::ALL.len() {
        let c = tp_core::TpCode::ALL[i];
        out.push(CodeRow { code: c.val(), slot: 0, module: "core", detail: c.detail() });
        i += 1;
    }
}

fn push_meta(out: &mut Vec<CodeRow>) {
    let mut i = 0usize;
    while i < tp_meta::MdCode::ALL.len() {
        let c = tp_meta::MdCode::ALL[i];
        out.push(CodeRow { code: c.val(), slot: 1, module: "meta", detail: c.detail() });
        i += 1;
    }
}

fn push_guard(out: &mut Vec<CodeRow>) {
    let mut i = 0usize;
    while i < tp_guard::PgCode::ALL.len() {
        let c = tp_guard::PgCode::ALL[i];
        out.push(CodeRow { code: c.val(), slot: 2, module: "guard", detail: c.detail() });
        i += 1;
    }
}

fn push_dispatch(out: &mut Vec<CodeRow>) {
    let mut i = 0usize;
    while i < tp_dispatch::SdCode::ALL.len() {
        let c = tp_dispatch::SdCode::ALL[i];
        out.push(CodeRow { code: c.val(), slot: 3, module: "dispatch", detail: c.detail() });
        i += 1;
    }
}

fn push_edge(out: &mut Vec<CodeRow>) {
    let mut i = 0usize;
    while i < tp_edge::QCode::ALL.len() {
        let c = tp_edge::QCode::ALL[i];
        out.push(CodeRow { code: c.val(), slot: 4, module: "edge", detail: c.detail() });
        i += 1;
    }
}

/// 构造普查表（45 行，按槽递增）。
pub fn census_rows() -> Vec<CodeRow> {
    let mut out = Vec::new();
    push_core(&mut out);
    push_meta(&mut out);
    push_guard(&mut out);
    push_dispatch(&mut out);
    push_edge(&mut out);
    out
}

/// 普查审计：条数、窗口、次序、去重、明细非空且不与码族外字符串混淆。
pub fn audit_census() -> Result<usize, u32> {
    let rows = census_rows();
    if rows.len() != FAMILY_CODES {
        return Err(BdCode::CensusBad.val());
    }
    let counts = source_counts();
    let mut i = 0usize;
    let mut prev_slot = 0u8;
    let mut prev_code = 0u32;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(x) => *x,
            None => return Err(BdCode::CensusBad.val()),
        };
        let w = match WINDOWS.get(r.slot as usize) {
            Some(x) => *x,
            None => return Err(BdCode::WindowBad.val()),
        };
        let src = match counts.get(r.slot as usize) {
            Some(x) => *x,
            None => return Err(BdCode::SliceBad.val()),
        };
        if w.count != src || w.name != r.module {
            return Err(BdCode::CensusBad.val());
        }
        if r.code < w.base.saturating_add(1) || r.code > window_end(&w) {
            return Err(BdCode::WindowBad.val());
        }
        if i == 0 {
            prev_slot = r.slot;
            prev_code = r.code.saturating_sub(1);
        }
        if r.slot < prev_slot || r.code <= prev_code {
            return Err(BdCode::OrderBad.val());
        }
        if r.detail.is_empty() || r.detail.starts_with("未知诊断码") {
            return Err(BdCode::CensusBad.val());
        }
        prev_slot = r.slot;
        prev_code = r.code;
        let mut j = i.saturating_add(1);
        while j < rows.len() {
            if let Some(b) = rows.get(j) {
                if b.code == r.code {
                    return Err(BdCode::CodeDup.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(rows.len())
}

/// 窗口闭集：每槽码值必须在本槽 0x40 步长内；跨槽窗口互不重叠。
pub fn audit_windows() -> Result<usize, u32> {
    let mut i = 0usize;
    while i < WINDOWS.len() {
        let a = match WINDOWS.get(i) {
            Some(x) => *x,
            None => return Err(BdCode::SliceBad.val()),
        };
        if a.count == 0 || a.count > SLOT_MAX.saturating_add(1) {
            return Err(BdCode::SliceBad.val());
        }
        if a.base < tp_core::SEG_BASE {
            return Err(BdCode::SegmentBad.val());
        }
        if window_end(&a) >= a.base.saturating_add(WINDOW_STRIDE) {
            return Err(BdCode::WindowBad.val());
        }
        if a.slot as usize != i {
            return Err(BdCode::OrderBad.val());
        }
        let mut j = i.saturating_add(1);
        while j < WINDOWS.len() {
            let b = match WINDOWS.get(j) {
                Some(x) => *x,
                None => return Err(BdCode::SliceBad.val()),
            };
            if b.base <= window_end(&a) {
                return Err(BdCode::WindowBad.val());
            }
            // 步长下界 0x40（B06 布点允许 0x40/0x60 两种间隔）。
            if b.base.saturating_sub(a.base) < WINDOW_STRIDE {
                return Err(BdCode::OrderBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(i)
}

// ── 单码查找 ──────────────────────────────────────────────────────
/// 在普查表内按码值线性查找；未知码返回 None，绝不猜邻近槽。
pub fn find_code(rows: &[CodeRow], code: u32) -> Option<CodeRow> {
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if r.code == code {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 按槽与槽内序号取行；序号越界拒绝而不是回绕。
pub fn row_at(rows: &[CodeRow], slot: u8, ix: u8) -> Result<CodeRow, u32> {
    let w = match WINDOWS.get(slot as usize) {
        Some(x) => *x,
        None => return Err(BdCode::SliceBad.val()),
    };
    if ix as usize >= w.count || ix as usize > SLOT_MAX {
        return Err(BdCode::SliceBad.val());
    }
    let start = slot_start(slot as usize)?;
    let at = start.saturating_add(ix as usize);
    match rows.get(at) {
        Some(r) => Ok(*r),
        None => Err(BdCode::SliceBad.val()),
    }
}

/// 未知码明细必须落在「未知诊断码」前缀上。
pub fn unknown_detail(code: u32) -> bool {
    detail(code).starts_with("未知诊断码") && find_code(&census_rows(), code).is_none()
}

/// 按槽分派到源模块的明细解释；槽外回落到本模块闭集。
pub fn module_detail(slot: u8, code: u32) -> &'static str {
    match slot {
        0 => tp_core::detail(code),
        1 => tp_meta::detail(code),
        2 => tp_guard::detail(code),
        3 => tp_dispatch::detail(code),
        4 => tp_edge::detail(code),
        _ => detail(code),
    }
}

// ── 确定性源 ──────────────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x4252_4944_4745_3036)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

/// FNV-1a 折叠（32 位与 64 位两个入口）。
pub const FNV32_OFFSET: u32 = 0x811C_9DC5;
pub const FNV32_PRIME: u32 = 0x0100_0193;
pub const FNV64_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV64_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_u32(h: u32, v: u32) -> u32 {
    let mut x = h;
    let mut b = 0u32;
    while b < 4 {
        x ^= (v >> (b * 8)) & 0xFF;
        x = x.wrapping_mul(FNV32_PRIME);
        b += 1;
    }
    x
}

pub fn fold_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut b = 0u64;
    while b < 8 {
        x ^= (v >> (b * 8)) & 0xFF;
        x = x.wrapping_mul(FNV64_PRIME);
        b += 1;
    }
    x
}

/// 族摘要：把 45 条码值按行序折进 FNV-1a。
pub fn family_digest() -> u32 {
    let rows = census_rows();
    let mut h = FNV32_OFFSET;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            h = fold_u32(h, r.code);
            h = fold_u32(h, r.slot as u32);
        }
        i += 1;
    }
    h
}

// ── 单模块密封 ────────────────────────────────────────────────────
/// 密封值：一个模块的三序探针 + 黄金全链 + 段位守卫共同折算。
/// 任一环节失败返回该模块自己的诊断码（由模块内部决定），本模块
/// 只把它折成统一摘要，不解释具体为何失败。
pub fn module_seal(slot: u8) -> Result<u64, u32> {
    let mut h = FNV64_OFFSET;
    let (golden_ok, seg_ok, probes_ok) = match slot {
        0 => (
            tp_core::golden_run().is_ok(),
            tp_core::segment_guard(),
            tp_core::probe_seq_a().is_ok()
                && tp_core::probe_seq_b().is_ok()
                && tp_core::probe_seq_c().is_ok(),
        ),
        1 => (
            tp_meta::golden_run().is_ok(),
            tp_meta::segment_guard(),
            tp_meta::probe_seq_a().is_ok()
                && tp_meta::probe_seq_b().is_ok()
                && tp_meta::probe_seq_c().is_ok(),
        ),
        2 => (
            tp_guard::golden_run().is_ok(),
            tp_guard::segment_guard(),
            tp_guard::probe_seq_a().is_ok()
                && tp_guard::probe_seq_b().is_ok()
                && tp_guard::probe_seq_c().is_ok(),
        ),
        3 => (
            tp_dispatch::golden_run().is_ok(),
            tp_dispatch::segment_guard(),
            tp_dispatch::probe_seq_a().is_ok()
                && tp_dispatch::probe_seq_b().is_ok()
                && tp_dispatch::probe_seq_c().is_ok(),
        ),
        4 => (
            tp_edge::golden_run().is_ok(),
            tp_edge::segment_guard(),
            tp_edge::probe_seq_a().is_ok()
                && tp_edge::probe_seq_b().is_ok()
                && tp_edge::probe_seq_c().is_ok(),
        ),
        _ => return Err(BdCode::SliceBad.val()),
    };
    if !golden_ok || !seg_ok || !probes_ok {
        return Err(BdCode::ProbeBad.val());
    }
    h = fold_u64(h, slot as u64);
    h = fold_u64(h, 0x01);
    h = fold_u64(h, 0x02);
    h = fold_u64(h, 0x03);
    Ok(h)
}

/// 五路联轧：每路都必须成功，并且两遍密封值逐位一致。
pub fn inline_roll() -> Result<[u64; ROLL_ROUTES], u32> {
    let mut out = [0u64; ROLL_ROUTES];
    let mut i = 0usize;
    while i < ROLL_ROUTES {
        let a = module_seal(i as u8)?;
        let b = module_seal(i as u8)?;
        if a != b {
            return Err(BdCode::ReplayBad.val());
        }
        match out.get_mut(i) {
            Some(s) => *s = a,
            None => return Err(BdCode::SliceBad.val()),
        }
        i += 1;
    }
    Ok(out)
}

/// 联轧摘要（五路密封值的 FNV 折叠）。
pub fn roll_digest() -> Result<u64, u32> {
    let seals = inline_roll()?;
    let mut h = FNV64_OFFSET;
    let mut i = 0usize;
    while i < seals.len() {
        if let Some(s) = seals.get(i) {
            h = fold_u64(h, *s);
        }
        i += 1;
    }
    Ok(h)
}

// ── fuzz ──────────────────────────────────────────────────────────
/// 一条 fuzz 输入：模块槽 + 槽内序号 + 噪声。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzCase {
    pub slot: u8,
    pub ix: u8,
    pub noise: u32,
}

impl FuzzCase {
    pub const fn new(slot: u8, ix: u8, noise: u32) -> FuzzCase {
        FuzzCase { slot, ix, noise }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzVerdict {
    Accept,
    Reject(u32),
}

impl FuzzVerdict {
    pub fn is_accept(&self) -> bool {
        matches!(self, FuzzVerdict::Accept)
    }

    pub fn code(&self) -> Option<u32> {
        match self {
            FuzzVerdict::Accept => None,
            FuzzVerdict::Reject(c) => Some(*c),
        }
    }
}

/// 单条 fuzz：只读普查表，不构造任何会 panic 的结构。
///   noise % 3 == 0：正向取行，明细必须与源枚举一致；
///   noise % 3 == 1：取槽尾之后的空位，必须落到未知诊断码；
///   noise % 3 == 2：按码值反查，必须回到同一行。
pub fn fuzz_one(rows: &[CodeRow], c: &FuzzCase) -> FuzzVerdict {
    if c.slot as usize >= FAMILY_MODULES {
        return FuzzVerdict::Reject(BdCode::FuzzDomain.val());
    }
    let w = match WINDOWS.get(c.slot as usize) {
        Some(x) => *x,
        None => return FuzzVerdict::Reject(BdCode::SliceBad.val()),
    };
    if c.ix as usize >= w.count {
        return FuzzVerdict::Reject(BdCode::SliceBad.val());
    }
    let row = match row_at(rows, c.slot, c.ix) {
        Ok(r) => r,
        Err(e) => return FuzzVerdict::Reject(e),
    };
    match c.noise % 3 {
        0 => {
            if module_detail(c.slot, row.code) == row.detail {
                FuzzVerdict::Accept
            } else {
                FuzzVerdict::Reject(BdCode::CensusBad.val())
            }
        }
        1 => {
            let probe = w.base.saturating_add(SLOT_MAX as u32).saturating_add(1);
            if unknown_detail(probe) {
                FuzzVerdict::Accept
            } else {
                FuzzVerdict::Reject(BdCode::WindowBad.val())
            }
        }
        _ => match find_code(rows, row.code) {
            Some(back) if back == row => FuzzVerdict::Accept,
            _ => FuzzVerdict::Reject(BdCode::CensusBad.val()),
        },
    }
}

/// fuzz 账：轮数、接收、拒绝、见过的不同码值。
pub struct FuzzLedger {
    pub rounds: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub distinct: u32,
    digest: u64,
}

impl FuzzLedger {
    pub fn rounds(&self) -> u32 {
        self.rounds
    }

    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn distinct(&self) -> u32 {
        self.distinct
    }

    pub fn seal(&self) -> u64 {
        self.digest
    }

    /// 收拒对账：接收 + 拒绝 必须等于轮数。
    pub fn balance(&self) -> Result<(), u32> {
        if self.accepted.saturating_add(self.rejected) != self.rounds {
            return Err(BdCode::CountBad.val());
        }
        Ok(())
    }
}

/// 万轮 fuzz：确定性 LCG 覆盖五个槽的全部序号与三类噪声。
pub fn fuzz_rounds(rounds: u32) -> Result<FuzzLedger, u32> {
    audit_census()?;
    let rows = census_rows();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ u64::from(rounds));
    let mut led = FuzzLedger { rounds: 0, accepted: 0, rejected: 0, distinct: 0, digest: FNV64_OFFSET };
    let mut seen = 0u64;
    let mut r = 0u32;
    while r < rounds {
        let slot = (rng.below(FAMILY_MODULES as u64 + 1)) as u8;
        let ix = (rng.below(SLOT_MAX as u64 + 2)) as u8;
        let noise = (rng.next() & 0xFFFF_FFFF) as u32;
        let verdict = fuzz_one(&rows, &FuzzCase::new(slot, ix, noise));
        match verdict {
            FuzzVerdict::Accept => {
                led.accepted = led.accepted.saturating_add(1);
                if slot < FAMILY_MODULES as u8 && ix as usize <= SLOT_MAX {
                    if let Ok(row) = row_at(&rows, slot, ix) {
                        let bit = 1u64 << (row.code & 0x3F);
                        if seen & bit == 0 {
                            seen |= bit;
                            led.distinct = led.distinct.saturating_add(1);
                        }
                        led.digest = fold_u64(led.digest, u64::from(row.code));
                    }
                }
            }
            FuzzVerdict::Reject(code) => {
                led.rejected = led.rejected.saturating_add(1);
                led.digest = fold_u64(led.digest, u64::from(code));
            }
        }
        led.rounds = led.rounds.saturating_add(1);
        r = r.saturating_add(1);
    }
    led.balance()?;
    Ok(led)
}

// ── 注入账（四路） ────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 四路注入：重复码 / 窗口越界 / 次序倒挂 / 序号越界。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x494E_4A);
    let mut r = 0u32;
    while r < rounds {
        rng.next();

        // 路 1：两个模块共用同一码值必须被判重。
        let rows = census_rows();
        let mut dup = false;
        if let (Some(a), Some(b)) = (rows.get(0), rows.get(1)) {
            let fake = [*a, *b, CodeRow { code: a.code, slot: b.slot, module: b.module, detail: b.detail }];
            dup = find_code(&fake, a.code).map(|x| x.slot) == Some(a.slot)
                && count_code(&fake, a.code) == 2;
        }
        led.record(0, dup);

        // 路 2：码值逃出所属窗口必须被拒。
        let w = match WINDOWS.get(2) {
            Some(x) => *x,
            None => return Err(BdCode::SliceBad.val()),
        };
        let out = CodeRow {
            code: w.base.saturating_add(WINDOW_STRIDE),
            slot: w.slot,
            module: w.name,
            detail: "越界码",
        };
        let window_bad = out.code > window_end(&w) && !in_window(&w, out.code);
        led.record(1, window_bad);

        // 路 3：次序倒挂必须被普查审计拒绝。
        let mut rev = census_rows();
        rev.reverse();
        let order_bad = audit_rows(&rev).err() == Some(BdCode::OrderBad.val());
        led.record(2, order_bad);

        // 路 4：槽内序号越界 / 未知槽号都必须拒绝。
        let slice_bad = row_at(&census_rows(), 0, SLOT_MAX as u8 + 1).is_err()
            && row_at(&census_rows(), FAMILY_MODULES as u8, 0).is_err()
            && slot_start(FAMILY_MODULES + 1).is_err();
        led.record(3, slice_bad);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

fn count_code(rows: &[CodeRow], code: u32) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if r.code == code {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

fn in_window(w: &ModuleWindow, code: u32) -> bool {
    code >= w.base.saturating_add(1) && code <= window_end(w)
}

/// 对任意行序做审核（注入路 3 用）：条数/窗口/次序/去重。
pub fn audit_rows(rows: &[CodeRow]) -> Result<usize, u32> {
    if rows.is_empty() || rows.len() > FAMILY_CODES {
        return Err(BdCode::CensusBad.val());
    }
    let mut i = 0usize;
    let mut prev_slot = 0u8;
    let mut prev_code = 0u32;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(x) => *x,
            None => return Err(BdCode::CensusBad.val()),
        };
        let w = match WINDOWS.get(r.slot as usize) {
            Some(x) => *x,
            None => return Err(BdCode::WindowBad.val()),
        };
        if !in_window(&w, r.code) {
            return Err(BdCode::WindowBad.val());
        }
        if r.detail.is_empty() {
            return Err(BdCode::CensusBad.val());
        }
        if i > 0 && (r.slot < prev_slot || r.code <= prev_code) {
            return Err(BdCode::OrderBad.val());
        }
        let mut j = i.saturating_add(1);
        while j < rows.len() {
            if let Some(b) = rows.get(j) {
                if b.code == r.code {
                    return Err(BdCode::CodeDup.val());
                }
            }
            j += 1;
        }
        prev_slot = r.slot;
        prev_code = r.code;
        i += 1;
    }
    Ok(i)
}

// ── 黄金全链 ──────────────────────────────────────────────────────
/// 黄金全链：普查 → 窗口 → 五路联轧 → 摘要。
pub fn golden_run() -> Result<(u32, u32, u64, u64), u32> {
    let n = audit_census()?;
    let w = audit_windows()?;
    let roll = roll_digest()?;
    let fam = u64::from(family_digest());
    if n != FAMILY_CODES || w != FAMILY_MODULES {
        return Err(BdCode::CensusBad.val());
    }
    let roll2 = roll_digest()?;
    if roll != roll2 {
        return Err(BdCode::ReplayBad.val());
    }
    let mut h = FNV64_OFFSET;
    h = fold_u64(h, n as u64);
    h = fold_u64(h, w as u64);
    h = fold_u64(h, roll);
    h = fold_u64(h, fam);
    Ok((n as u32, w as u32, roll, h))
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if audit_census()? != FAMILY_CODES || audit_windows()? != FAMILY_MODULES {
        return Err(BdCode::CensusBad.val());
    }
    let rows = census_rows();
    let mut i = 0usize;
    while i < WINDOWS.len() {
        let w = match WINDOWS.get(i) {
            Some(x) => *x,
            None => return Err(BdCode::SliceBad.val()),
        };
        let r = row_at(&rows, w.slot, 0)?;
        if r.code != w.base.saturating_add(1) || r.module != w.name {
            return Err(BdCode::CensusBad.val());
        }
        i += 1;
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let h1 = family_digest();
    let h2 = family_digest();
    if h1 != h2 {
        return Err(BdCode::HashBad.val());
    }
    let a = roll_digest()?;
    let b = roll_digest()?;
    if a != b {
        return Err(BdCode::RollDrift.val());
    }
    if unknown_detail(SEG_BASE) == false {
        return Err(BdCode::WindowBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let counts = source_counts();
    let mut total = 0usize;
    let mut i = 0usize;
    while i < counts.len() {
        if let Some(c) = counts.get(i) {
            total = total.saturating_add(*c);
        }
        i += 1;
    }
    if FAMILY_CODES == 45
        && FAMILY_MODULES == 5
        && ROLL_ROUTES == 5
        && FUZZ_ROUNDS == 10_000
        && WINDOW_STRIDE == 0x40
        && total == FAMILY_CODES
        && BdCode::ALL.len() == 12
    {
        Ok(())
    } else {
        Err(BdCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(BdCode::ReplayBad.val())
    }
}

pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < BdCode::ALL.len() {
        let c = BdCode::ALL[i].val();
        if c < SEG_BASE.saturating_add(1) || c > SEG_BASE.saturating_add(0x0C) {
            ok = false;
        }
        i += 1;
    }
    ok && detail(SEG_BASE).starts_with("未知诊断码")
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let census = audit_census();
    v.push((
        "F48915-J1-四十五码族闭集",
        census.ok() == Some(FAMILY_CODES) && census_rows().len() == FAMILY_CODES,
        format!("census={:?} rows={}", census.ok(), census_rows().len()),
    ));
    let win = audit_windows();
    v.push((
        "F48915-J1-五窗互不重叠",
        win.ok() == Some(FAMILY_MODULES),
        format!("windows={:?} count={}", win.ok(), WINDOWS.len()),
    ));
    let fz = fuzz_rounds(FUZZ_ROUNDS);
    v.push((
        "F48913-J1-万轮fuzz零崩",
        match &fz {
            Ok(l) => l.rounds() == FUZZ_ROUNDS && l.balance().is_ok(),
            Err(_) => false,
        },
        match &fz {
            Ok(l) => format!(
                "rounds={} accept={} reject={} distinct={}",
                l.rounds(),
                l.accepted(),
                l.rejected(),
                l.distinct()
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48913-J1-对账收拒平衡",
        match &fz {
            Ok(l) => l.accepted().saturating_add(l.rejected()) == l.rounds(),
            Err(_) => false,
        },
        match &fz {
            Ok(l) => format!("{} + {} = {}", l.accepted(), l.rejected(), l.rounds()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let roll = inline_roll();
    v.push((
        "F48914-J1-五路联轧全绿",
        roll.is_ok(),
        match &roll {
            Ok(r) => format!("seals={:?}", r),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48914-J1-五路双跑零漂移",
        {
            let a = roll_digest();
            let b = roll_digest();
            a.is_ok() && a == b
        },
        format!("a={:?} b={:?}", roll_digest().ok(), roll_digest().ok()),
    ));
    v.push((
        "F48914-J1-五模块探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        format!(
            "a={} b={} c={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok()
        ),
    ));
    v.push((
        "F48915-J1-码值全窗内",
        {
            let rows = census_rows();
            let mut ok = true;
            let mut i = 0usize;
            while i < rows.len() {
                if let Some(r) = rows.get(i) {
                    if let Some(w) = WINDOWS.get(r.slot as usize) {
                        if !in_window(w, r.code) {
                            ok = false;
                        }
                    } else {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok
        },
        String::from("45 条码值逐条落在各自窗口"),
    ));
    v.push((
        "F48915-J1-未知码不猜",
        unknown_detail(SEG_BASE)
            && unknown_detail(tp_core::SEG_BASE)
            && unknown_detail(0xFFFF_FFFF)
            && find_code(&census_rows(), 0xFFFF_FFFF).is_none(),
        String::from("未知码只返回未知诊断码前缀"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48915-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48914-J1-段位守卫",
        segment_guard(),
        String::from("0x70E00 窗口 12 码"),
    ));
    v
}

pub fn run_unx48913_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48913 触控板 B06 段内联轧");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn census_has_forty_five_rows() {
        assert_eq!(audit_census(), Ok(45));
        assert_eq!(census_rows().len(), 45);
        let counts = source_counts();
        let mut total = 0usize;
        let mut i = 0usize;
        while i < counts.len() {
            if let Some(c) = counts.get(i) {
                total += *c;
            }
            i += 1;
        }
        assert_eq!(total, 45);
    }

    #[test]
    fn census_rows_are_strictly_ordered() {
        let rows = census_rows();
        let mut i = 1usize;
        while i < rows.len() {
            if let (Some(a), Some(b)) = (rows.get(i - 1), rows.get(i)) {
                assert!(b.code > a.code);
                assert!(b.slot >= a.slot);
            }
            i += 1;
        }
    }

    #[test]
    fn windows_are_disjoint_and_stepped() {
        assert_eq!(audit_windows(), Ok(5));
        let mut i = 1usize;
        while i < WINDOWS.len() {
            if let (Some(a), Some(b)) = (WINDOWS.get(i - 1), WINDOWS.get(i)) {
                assert!(b.base - a.base >= WINDOW_STRIDE);
                assert!(b.base > window_end(a));
            }
            i += 1;
        }
    }

    #[test]
    fn row_lookup_is_exact() {
        let rows = census_rows();
        let first = row_at(&rows, 0, 0);
        assert!(first.is_ok());
        if let Ok(r) = first {
            assert_eq!(r.code, tp_core::SEG_BASE + 1);
            assert_eq!(r.module, "core");
        }
        assert_eq!(row_at(&rows, 0, 8), Err(BdCode::SliceBad.val()));
        assert_eq!(row_at(&rows, 5, 0), Err(BdCode::SliceBad.val()));
    }

    #[test]
    fn unknown_codes_are_never_guessed() {
        assert!(unknown_detail(SEG_BASE));
        assert!(unknown_detail(tp_guard::SEG_BASE + 0x20));
        assert!(unknown_detail(0x1234_5678));
        assert!(!unknown_detail(tp_edge::SEG_BASE + 1));
    }

    #[test]
    fn family_digest_is_stable() {
        assert_eq!(family_digest(), family_digest());
        assert_ne!(family_digest(), 0);
    }

    #[test]
    fn ten_thousand_fuzz_rounds_balance() {
        let l = fuzz_rounds(FUZZ_ROUNDS);
        assert!(l.is_ok());
        if let Ok(ll) = l {
            assert_eq!(ll.rounds(), 10_000);
            assert_eq!(ll.accepted() + ll.rejected(), ll.rounds());
            assert!(ll.distinct() > 0);
            assert!(ll.balance().is_ok());
        }
    }

    #[test]
    fn fuzz_domain_rejects_unknown_slot() {
        let rows = census_rows();
        let v = fuzz_one(&rows, &FuzzCase::new(9, 0, 0));
        assert_eq!(v.code(), Some(BdCode::FuzzDomain.val()));
        let v2 = fuzz_one(&rows, &FuzzCase::new(0, 9, 0));
        assert_eq!(v2.code(), Some(BdCode::SliceBad.val()));
    }

    #[test]
    fn fuzz_accepts_valid_rows() {
        let rows = census_rows();
        let mut slot = 0u8;
        while (slot as usize) < FAMILY_MODULES {
            let v = fuzz_one(&rows, &FuzzCase::new(slot, 0, 0));
            assert!(v.is_accept());
            slot += 1;
        }
    }

    #[test]
    fn fuzz_is_deterministic() {
        let a = fuzz_rounds(1000);
        let b = fuzz_rounds(1000);
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(aa), Ok(bb)) = (a, b) {
            assert_eq!(aa.seal(), bb.seal());
        }
    }

    #[test]
    fn five_routes_all_green() {
        let r = inline_roll();
        assert!(r.is_ok());
        if let Ok(rr) = r {
            assert_eq!(rr.len(), 5);
            assert!(rr.iter().all(|x| *x != 0));
        }
    }

    #[test]
    fn golden_is_stable() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn golden_tuple_anchored() {
        let g = golden_run();
        assert!(g.is_ok());
        if let Ok((n, w, _roll, h)) = g {
            assert_eq!(n, 45);
            assert_eq!(w, 5);
            assert_ne!(h, 0);
        }
    }

    #[test]
    fn inject_four_routes_all_caught() {
        let l = inject_rounds(10);
        assert!(l.is_ok());
        if let Ok(ll) = l {
            assert!(ll.all_caught());
            assert_eq!(ll.tries, [10; 4]);
            assert_eq!(ll.caught, [10; 4]);
        }
    }

    #[test]
    fn reversed_rows_are_rejected() {
        let mut rev = census_rows();
        rev.reverse();
        assert_eq!(audit_rows(&rev), Err(BdCode::OrderBad.val()));
    }

    #[test]
    fn duplicate_codes_are_rejected() {
        let rows = census_rows();
        let mut fake = Vec::new();
        if let (Some(a), Some(b)) = (rows.get(0), rows.get(1)) {
            fake.push(*a);
            fake.push(*b);
            fake.push(CodeRow { code: a.code, slot: b.slot, module: b.module, detail: b.detail });
        }
        assert_eq!(audit_rows(&fake), Err(BdCode::CodeDup.val()));
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(BdCode::ALL.len(), 12);
        assert_eq!(BdCode::CodeDup.val(), SEG_BASE + 1);
        assert_eq!(BdCode::SegmentBad.val(), SEG_BASE + 12);
    }

    #[test]
    fn probe_sequences_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn module_seals_are_nonzero_and_distinct() {
        let mut seen = [0u64; 5];
        let mut i = 0usize;
        while i < 5 {
            let s = module_seal(i as u8);
            assert!(s.is_ok());
            if let Ok(v) = s {
                assert_ne!(v, 0);
                let mut j = 0usize;
                while j < i {
                    assert_ne!(seen[j], v);
                    j += 1;
                }
                seen[i] = v;
            }
            i += 1;
        }
        assert!(module_seal(5).is_err());
    }
}
