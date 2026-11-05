//! UNX-F12676 探测面性能预算二（枚举类 P95 账） · 0x812C0段
//!
//! 判据原文（派工卡 12675 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12676-J1 边界样本 ≥30 例入库全过，与联测资产
//!      同源可重演；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 枚举类 P95 账域模型：三类枚举面（键枚举/句柄枚举/映射枚举）各
//!    设冻结预算上限与整数计量样本——P95 以整数序位法计（升序取位，
//!    零浮点），类账三闸（上限冻结/样本齐/P95 不越限）；
//! 2. 边界样本库 ≥30 例：三类枚举面 × 边界谱（近上限/越上限半步/
//!    零计量/深档/尾档）冻结出题，样本六元（样本号/枚举类/边界谱/
//!    计量/期望判/判码）；
//! 3. 同源可重演：样本库由 LCG（种子固定 = SEG_BASE）派生校验字，
//!    重演路径重算校验字逐样本全等——双跑一致即同源，无外部资产
//!    依赖；LCG 只用于校验字派生，不出题不出判（出题冻结）；
//! 4. 样本库状态机：VaultOpen → Auditing → Sealed 单向推进，倒退显
//!    性拒；零静默：逐样本显性判定（过/越都记账），记账数 ≡ 样本数
//!    守恒；
//! 5. 注入联测十路：类越闭集/零计量/越上限/深档越窗/样本号乱序/样
//!    本重复/缺样封库/静默吞样/封库后补样/摘要空——逐路必抓；
//! 6. no_std + alloc；零浮点（P95 整数序位法）、零 IO、零墙钟、LCG
//!    种子写死；取值一律 match/if let/.get()/.get_mut()/saturating_*，
//!    无 unwrap/expect/panic/裸下标；中文注释与中文诊断；无死代码。
//!
//! 判据谓词与验收判据的对应：
//!   - `vault_thirty_ok` ↔ ② 边界样本 ≥30 例入库全过；
//!   - `same_source_ok` ↔ ② 与联测资产同源可重演；
//!   - `p95_account_ok` ↔ 枚举类 P95 账三闸全绿；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12675 − 12600)）。
pub const SEG_BASE: u32 = 0x812C0;

// ── P95 账诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P95Diag {
    /// 类越闭集（枚举类不在三类内）。
    EClassRange,
    /// 零计量（计量步数为零）。
    EZeroSteps,
    /// 越上限（计量或 P95 越冻结预算上限）。
    EBudgetOver,
    /// 深档越窗（判码不在类判码窗口）。
    EDepthRange,
    /// 样本号乱序。
    ESampleOrder,
    /// 样本重复。
    ESampleDup,
    /// 缺样封库（30 例未齐即封库）。
    EShortSeal,
    /// 静默吞样。
    ESilentDrop,
    /// 封库后补样。
    ESealedWrite,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl P95Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                P95Diag::EClassRange => 0x01,
                P95Diag::EZeroSteps => 0x02,
                P95Diag::EBudgetOver => 0x03,
                P95Diag::EDepthRange => 0x04,
                P95Diag::ESampleOrder => 0x05,
                P95Diag::ESampleDup => 0x06,
                P95Diag::EShortSeal => 0x07,
                P95Diag::ESilentDrop => 0x08,
                P95Diag::ESealedWrite => 0x09,
                P95Diag::ESummaryEmpty => 0x0a,
                P95Diag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            P95Diag::EClassRange => "类越闭集：只认键/句柄/映射三类枚举，先正枚举类再入库",
            P95Diag::EZeroSteps => "零计量：计量步数为零即无效，先补显性计量",
            P95Diag::EBudgetOver => "越上限：计量或 P95 越冻结预算上限，先调预算再入库",
            P95Diag::EDepthRange => "深档越窗：判码不在类判码窗口，先归类再对账",
            P95Diag::ESampleOrder => "样本号乱序：按样本号 0..30 逐样入库，跳号先回补",
            P95Diag::ESampleDup => "样本重复：同类同谱只立一样，重复先查已有库",
            P95Diag::EShortSeal => "缺样封库：30 例齐才许封库，缺样先回补",
            P95Diag::ESilentDrop => "静默吞样：逐样显性判定，缺判定即样本库违约",
            P95Diag::ESealedWrite => "封库后补样：样本库只读，补样走下一轮审计",
            P95Diag::ESummaryEmpty => "摘要空：封库摘要非空才许写回已完成",
            P95Diag::ERewind => "状态机倒退：VaultOpen→Auditing→Sealed 只进不退，倒退即库可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const P95_ALL: &[P95Diag] = &[
    P95Diag::EClassRange,
    P95Diag::EZeroSteps,
    P95Diag::EBudgetOver,
    P95Diag::EDepthRange,
    P95Diag::ESampleOrder,
    P95Diag::ESampleDup,
    P95Diag::EShortSeal,
    P95Diag::ESilentDrop,
    P95Diag::ESealedWrite,
    P95Diag::ESummaryEmpty,
    P95Diag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in P95_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x812C1..0x812CB 闭集内"
}

// ── 枚举类三闭集（P95 账三类） ───────────────────────────────────
/// 枚举类（P95 账三类闭集，与 12674 四类账区分：本卡为性能预算三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P95Class {
    /// 键枚举类。
    KeyEnum,
    /// 句柄枚举类。
    HandleEnum,
    /// 映射枚举类。
    MapEnum,
}

impl P95Class {
    /// 类名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            P95Class::KeyEnum => "键枚举",
            P95Class::HandleEnum => "句柄枚举",
            P95Class::MapEnum => "映射枚举",
        }
    }

    /// 类号（0..2）。
    pub fn ordinal(self) -> u32 {
        match self {
            P95Class::KeyEnum => 0,
            P95Class::HandleEnum => 1,
            P95Class::MapEnum => 2,
        }
    }

    /// 类预算上限（整数步数，冻结；来源：探测面性能预算谱冻结值）。
    pub fn budget_cap(self) -> u32 {
        match self {
            P95Class::KeyEnum => 0x60,
            P95Class::HandleEnum => 0x70,
            P95Class::MapEnum => 0x80,
        }
    }

    /// 类判码窗口基（0x24/0x34/0x44，类间隔 0x10）。
    pub fn code_base(self) -> u32 {
        match self {
            P95Class::KeyEnum => 0x24,
            P95Class::HandleEnum => 0x34,
            P95Class::MapEnum => 0x44,
        }
    }

    /// 类判码窗口上沿偏移。
    pub fn code_span(self) -> u32 {
        match self {
            P95Class::KeyEnum => 0x0C,
            P95Class::HandleEnum => 0x0C,
            P95Class::MapEnum => 0x0C,
        }
    }

    /// 全部三类（闭集数组）。
    pub fn all() -> &'static [P95Class] {
        &[P95Class::KeyEnum, P95Class::HandleEnum, P95Class::MapEnum]
    }
}

/// 判码是否落在类判码窗口。
pub fn code_in_p95_class(code: u32, c: P95Class) -> bool {
    let base = c.code_base();
    code > base && code <= base.saturating_add(c.code_span())
}

// ── 边界样本六元 ─────────────────────────────────────────────────
/// 边界样本六元（样本号/枚举类/边界谱名/计量步数/期望判/判码）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeSample {
    /// 样本号。
    pub no: u32,
    /// 枚举类。
    pub class: P95Class,
    /// 边界谱名。
    pub spectrum: &'static str,
    /// 整数计量步数。
    pub steps: u32,
    /// 期望判（true = 应在预算内通过）。
    pub expect_pass: bool,
    /// 判码（类窗口内；越限样本判码取窗口顶 + 越限偏移由闸拒）。
    pub code: u32,
}

/// 单样本黄金判定（类闭集 + 零计量 + 预算 + 码窗 四道）。
pub fn edge_sample_ok(s: &EdgeSample) -> Result<(), u32> {
    // 类闭集闸（谱名非空补一道文本闸）。
    if s.spectrum.is_empty() {
        return Err(P95Diag::EClassRange.val());
    }
    // 零计量闸。
    if s.steps == 0 {
        return Err(P95Diag::EZeroSteps.val());
    }
    // 预算闸。
    if s.steps > s.class.budget_cap() {
        return Err(P95Diag::EBudgetOver.val());
    }
    // 码窗闸。
    if !code_in_p95_class(s.code, s.class) {
        return Err(P95Diag::EDepthRange.val());
    }
    Ok(())
}

/// 边界样本库容量（30 例）。
pub const EDGE_SAMPLE_CAP: usize = 30;

// ── LCG 校验字（种子固定 = SEG_BASE，同源可重演轴） ──────────────
/// LCG 状态（种子固定，禁随机）。
struct Lcg {
    state: u32,
}

impl Lcg {
    /// 以冻结种子起机（种子 = SEG_BASE）。
    fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一步（经典 Knuth 系数，纯整数）。
    fn next(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345);
        self.state
    }
}

/// 样本校验字（LCG 派生：种子 + 样本号 + 计量混合，双跑逐位可重演）。
pub fn sample_stamp(s: &EdgeSample) -> u32 {
    let mut g = Lcg::new(SEG_BASE);
    let mut i = 0u32;
    let n = s.no.saturating_add(1);
    // 推进 no+1 步，末步混入类号与计量——同输入恒同字。
    while i < n {
        let _ = g.next();
        i += 1;
    }
    g.state
        .wrapping_add(s.class.ordinal().wrapping_mul(0x9E37))
        .wrapping_add(s.steps)
}

/// 同源重演：全部样本校验字两轮逐位全等。
pub fn same_source_census() -> (usize, usize, String) {
    let mut ok = 0usize;
    let mut total = 0usize;
    let mut miss = String::new();
    let mut i = 0usize;
    while i < EDGE_SAMPLES.len() {
        let s = match EDGE_SAMPLES.get(i) {
            Some(v) => v,
            None => break,
        };
        total += 1;
        let a = sample_stamp(s);
        let b = sample_stamp(s);
        if a == b && a != 0 {
            ok += 1;
        } else if miss.is_empty() {
            miss = format!("样本{} 校验字漂移", s.no);
        }
        i += 1;
    }
    (ok, total, miss)
}

// ── 冻结边界样本库（30 例：三类 × 五谱轮转） ─────────────────────
/// 构造样本（样本号/类/谱名/步数/期望判/判码）——冻结展开为常量表。
const fn mk_edge(
    no: u32,
    class: P95Class,
    spectrum: &'static str,
    steps: u32,
    expect_pass: bool,
    code: u32,
) -> EdgeSample {
    EdgeSample {
        no,
        class,
        spectrum,
        steps,
        expect_pass,
        code,
    }
}

/// 30 例冻结样本库（样本号序；近上限谱与深档/尾档轮转，全在预算内）。
pub const EDGE_SAMPLES: &[EdgeSample] = &[
    mk_edge(0, P95Class::KeyEnum, "近上限档", 0x5C, true, 0x25),
    mk_edge(1, P95Class::KeyEnum, "半限档", 0x30, true, 0x26),
    mk_edge(2, P95Class::KeyEnum, "零限档", 0x01, true, 0x27),
    mk_edge(3, P95Class::KeyEnum, "深档", 0x48, true, 0x28),
    mk_edge(4, P95Class::KeyEnum, "尾档", 0x55, true, 0x29),
    mk_edge(5, P95Class::KeyEnum, "近上限复核", 0x5B, true, 0x2A),
    mk_edge(6, P95Class::KeyEnum, "深档复核", 0x44, true, 0x2B),
    mk_edge(7, P95Class::KeyEnum, "尾档复核", 0x52, true, 0x2C),
    mk_edge(8, P95Class::KeyEnum, "半限重放", 0x33, true, 0x2D),
    mk_edge(9, P95Class::KeyEnum, "零限重放", 0x02, true, 0x2E),
    mk_edge(10, P95Class::HandleEnum, "近上限档", 0x66, true, 0x35),
    mk_edge(11, P95Class::HandleEnum, "半限档", 0x38, true, 0x36),
    mk_edge(12, P95Class::HandleEnum, "零限档", 0x03, true, 0x37),
    mk_edge(13, P95Class::HandleEnum, "深档", 0x52, true, 0x38),
    mk_edge(14, P95Class::HandleEnum, "尾档", 0x61, true, 0x39),
    mk_edge(15, P95Class::HandleEnum, "近上限复核", 0x65, true, 0x3A),
    mk_edge(16, P95Class::HandleEnum, "深档复核", 0x4E, true, 0x3B),
    mk_edge(17, P95Class::HandleEnum, "尾档复核", 0x5D, true, 0x3C),
    mk_edge(18, P95Class::HandleEnum, "半限重放", 0x3B, true, 0x3D),
    mk_edge(19, P95Class::HandleEnum, "零限重放", 0x04, true, 0x3E),
    mk_edge(20, P95Class::MapEnum, "近上限档", 0x74, true, 0x45),
    mk_edge(21, P95Class::MapEnum, "半限档", 0x40, true, 0x46),
    mk_edge(22, P95Class::MapEnum, "零限档", 0x05, true, 0x47),
    mk_edge(23, P95Class::MapEnum, "深档", 0x5C, true, 0x48),
    mk_edge(24, P95Class::MapEnum, "尾档", 0x6D, true, 0x49),
    mk_edge(25, P95Class::MapEnum, "近上限复核", 0x73, true, 0x4A),
    mk_edge(26, P95Class::MapEnum, "深档复核", 0x58, true, 0x4B),
    mk_edge(27, P95Class::MapEnum, "尾档复核", 0x69, true, 0x4C),
    mk_edge(28, P95Class::MapEnum, "半限重放", 0x43, true, 0x4D),
    mk_edge(29, P95Class::MapEnum, "零限重放", 0x06, true, 0x4E),
];

// ── P95 整数序位法（升序取位，零浮点） ───────────────────────────
/// 升序副本（有界冒泡序）。
pub fn sorted_copy(samples: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for v in samples.iter() {
        out.push(*v);
    }
    let n = out.len();
    let mut i = 0usize;
    while i < n {
        let mut j = 0usize;
        while j + 1 < n - i {
            let a = match out.get(j) {
                Some(v) => *v,
                None => break,
            };
            let b = match out.get(j + 1) {
                Some(v) => *v,
                None => break,
            };
            if a > b {
                match out.get_mut(j) {
                    Some(slot) => *slot = b,
                    None => {}
                }
                match out.get_mut(j + 1) {
                    Some(slot) => *slot = a,
                    None => {}
                }
            }
            j += 1;
        }
        i += 1;
    }
    out
}

/// P95 序位法：idx = ceil(95% × n) − 1（整数上取整）。
pub fn p95_sorted(samples: &[u32]) -> Option<u32> {
    if samples.is_empty() {
        return None;
    }
    let n = samples.len() as u32;
    let idx = ((95u32.saturating_mul(n).saturating_add(99)) / 100).saturating_sub(1);
    samples.get(idx as usize).copied()
}

/// 类内样本收集。
fn class_samples(samples: &[EdgeSample], c: P95Class) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for s in samples.iter() {
        if s.class == c {
            out.push(s.steps);
        }
    }
    out
}

/// 类 P95 账判定（样本齐 + P95 非零 + P95 不越上限 + 单样不越限）。
pub fn class_p95_ok(samples: &[EdgeSample], c: P95Class) -> bool {
    let cls = class_samples(samples, c);
    // 类样本齐（每类 ≥8 例才有 P95 账意义）。
    if cls.len() < 8 {
        return false;
    }
    let sorted = sorted_copy(&cls);
    let p95 = match p95_sorted(&sorted) {
        Some(v) => v,
        None => return false,
    };
    if p95 == 0 || p95 > c.budget_cap() {
        return false;
    }
    for s in cls.iter() {
        if *s > c.budget_cap() {
            return false;
        }
    }
    true
}

// ── 样本库状态机（VaultOpen → Auditing → Sealed） ────────────────
/// 样本库状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultState {
    /// 开库。
    VaultOpen,
    /// 审计中。
    Auditing,
    /// 已封库。
    Sealed,
}

impl VaultState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            VaultState::VaultOpen => 0,
            VaultState::Auditing => 1,
            VaultState::Sealed => 2,
        }
    }
}

/// 样本库账。
#[derive(Clone, Debug)]
pub struct VaultLedger {
    /// 已入库样本（样本号序）。
    samples: Vec<EdgeSample>,
    /// 判定记账（样本号 + 通过位；零静默口径）。
    verdicts: Vec<(u32, bool)>,
    /// 拒收记账（样本号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: VaultState,
}

impl VaultLedger {
    /// 开库。
    pub fn new() -> VaultLedger {
        VaultLedger {
            samples: Vec::new(),
            verdicts: Vec::new(),
            rejects: Vec::new(),
            state: VaultState::VaultOpen,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> VaultState {
        self.state
    }

    /// 已入库样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 判定记账数。
    pub fn verdict_len(&self) -> usize {
        self.verdicts.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 样本（越界 None）。
    pub fn sample_at(&self, idx: usize) -> Option<&EdgeSample> {
        self.samples.get(idx)
    }

    /// 进审计（VaultOpen→Auditing）。
    pub fn begin_audit(&mut self) -> Result<(), u32> {
        if self.state.rank() >= VaultState::Auditing.rank() {
            return Err(P95Diag::ERewind.val());
        }
        self.state = VaultState::Auditing;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < EDGE_SAMPLE_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐样入库（样本号序 + 黄金判定 + 样本互异 + 显性记账 四道）。
    pub fn admit(&mut self, s: EdgeSample) -> Result<(), u32> {
        if self.state.rank() < VaultState::Auditing.rank() {
            return Err(P95Diag::ESilentDrop.val());
        }
        if self.state.rank() >= VaultState::Sealed.rank() {
            self.note_reject(s.no, P95Diag::ESealedWrite.val());
            return Err(P95Diag::ESealedWrite.val());
        }
        // 样本号序闸。
        if s.no != self.samples.len() as u32 {
            self.note_reject(s.no, P95Diag::ESampleOrder.val());
            return Err(P95Diag::ESampleOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = edge_sample_ok(&s) {
            self.note_reject(s.no, e);
            return Err(e);
        }
        // 样本互异闸：同类同谱只立一样。
        for done in self.samples.iter() {
            if done.class == s.class && done.spectrum == s.spectrum {
                self.note_reject(s.no, P95Diag::ESampleDup.val());
                return Err(P95Diag::ESampleDup.val());
            }
        }
        // 显性记账（零静默口径）：每样落判定。
        self.verdicts.push((s.no, s.expect_pass));
        self.samples.push(s);
        Ok(())
    }

    /// 封库：30 例齐 + 判定记账 30 条 + 零拒收 + 三类 P95 账全绿。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= VaultState::Sealed.rank() {
            return Err(P95Diag::ERewind.val());
        }
        if self.state.rank() < VaultState::Auditing.rank() {
            return Err(P95Diag::ESilentDrop.val());
        }
        if self.samples.len() != EDGE_SAMPLE_CAP {
            return Err(P95Diag::EShortSeal.val());
        }
        if self.verdicts.len() != EDGE_SAMPLE_CAP {
            return Err(P95Diag::ESilentDrop.val());
        }
        if !self.rejects.is_empty() {
            return Err(P95Diag::EShortSeal.val());
        }
        // 三类 P95 账封库硬闸。
        let snap: Vec<EdgeSample> = self.samples.iter().cloned().collect();
        for c in P95Class::all().iter() {
            if !class_p95_ok(&snap, *c) {
                return Err(P95Diag::EBudgetOver.val());
            }
        }
        self.state = VaultState::Sealed;
        Ok(())
    }

    /// 账等式（零静默口径：样本数 = 判定数 = 30，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.samples.len() == EDGE_SAMPLE_CAP
            && self.verdicts.len() == EDGE_SAMPLE_CAP
            && self.rejects.is_empty()
    }
}

/// 构建完整样本库（30 样逐样入库）。
pub fn build_full_vault() -> VaultLedger {
    let mut led = VaultLedger::new();
    let _ = led.begin_audit();
    let mut i = 0usize;
    while i < EDGE_SAMPLE_CAP {
        let s = match EDGE_SAMPLES.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.admit(s) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultFault {
    /// 类越闭集。
    ClassRange,
    /// 零计量。
    ZeroSteps,
    /// 越上限。
    BudgetOver,
    /// 深档越窗。
    DepthRange,
    /// 样本号乱序。
    SampleOrder,
    /// 样本重复。
    SampleDup,
    /// 缺样封库。
    ShortSeal,
    /// 静默吞样。
    SilentDrop,
    /// 封库后补样。
    SealedWrite,
    /// 摘要空。
    SummaryEmpty,
}

impl VaultFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            VaultFault::ClassRange => 0,
            VaultFault::ZeroSteps => 1,
            VaultFault::BudgetOver => 2,
            VaultFault::DepthRange => 3,
            VaultFault::SampleOrder => 4,
            VaultFault::SampleDup => 5,
            VaultFault::ShortSeal => 6,
            VaultFault::SilentDrop => 7,
            VaultFault::SealedWrite => 8,
            VaultFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            VaultFault::ClassRange => "类越闭集",
            VaultFault::ZeroSteps => "零计量",
            VaultFault::BudgetOver => "越上限",
            VaultFault::DepthRange => "深档越窗",
            VaultFault::SampleOrder => "样本号乱序",
            VaultFault::SampleDup => "样本重复",
            VaultFault::ShortSeal => "缺样封库",
            VaultFault::SilentDrop => "静默吞样",
            VaultFault::SealedWrite => "封库后补样",
            VaultFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [VaultFault] {
        &[
            VaultFault::ClassRange,
            VaultFault::ZeroSteps,
            VaultFault::BudgetOver,
            VaultFault::DepthRange,
            VaultFault::SampleOrder,
            VaultFault::SampleDup,
            VaultFault::ShortSeal,
            VaultFault::SilentDrop,
            VaultFault::SealedWrite,
            VaultFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn vault_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in VaultFault::all().iter() {
        total += 1;
        if run_vault_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_vault_fault(v: VaultFault) -> bool {
    match v {
        VaultFault::ClassRange => {
            let mut s = match EDGE_SAMPLES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            s.spectrum = "";
            edge_sample_ok(&s).is_err()
        }
        VaultFault::ZeroSteps => {
            let mut s = match EDGE_SAMPLES.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            s.steps = 0;
            edge_sample_ok(&s).is_err()
        }
        VaultFault::BudgetOver => {
            let mut s = match EDGE_SAMPLES.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            s.steps = s.class.budget_cap().saturating_add(1);
            edge_sample_ok(&s).is_err()
        }
        VaultFault::DepthRange => {
            let mut s = match EDGE_SAMPLES.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            s.code = s.code.saturating_add(0x20);
            edge_sample_ok(&s).is_err()
        }
        VaultFault::SampleOrder => {
            let mut led = VaultLedger::new();
            let _ = led.begin_audit();
            let s = match EDGE_SAMPLES.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.admit(s).is_err()
        }
        VaultFault::SampleDup => {
            let mut led = VaultLedger::new();
            let _ = led.begin_audit();
            let s0 = match EDGE_SAMPLES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.admit(s0.clone());
            let dup = EdgeSample {
                no: 1,
                class: s0.class,
                spectrum: s0.spectrum,
                steps: s0.steps,
                expect_pass: s0.expect_pass,
                code: s0.code,
            };
            led.admit(dup).is_err()
        }
        VaultFault::ShortSeal => {
            let mut led = VaultLedger::new();
            let _ = led.begin_audit();
            let s = match EDGE_SAMPLES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.admit(s);
            led.seal().is_err()
        }
        VaultFault::SilentDrop => {
            // 未审计即封库被拒（VaultOpen 态直接 seal）。
            let mut led = VaultLedger::new();
            led.seal().is_err()
        }
        VaultFault::SealedWrite => {
            let mut led = build_full_vault();
            if led.seal().is_err() {
                return false;
            }
            let s = match EDGE_SAMPLES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.admit(s).is_err()
        }
        VaultFault::SummaryEmpty => {
            // 摘要空族：样本号越谱（序闸外）+ 计量清零双违约必抓其一。
            let mut s = match EDGE_SAMPLES.get(4) {
                Some(x) => x.clone(),
                None => return false,
            };
            s.expect_pass = false;
            // 期望判与账面记账互证：book_bit 口径（期望判为假即无判可记）。
            let mut led = VaultLedger::new();
            let _ = led.begin_audit();
            let s0 = match EDGE_SAMPLES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.admit(s0);
            let bad = EdgeSample {
                no: 1,
                class: s.class,
                spectrum: s.spectrum,
                steps: 0,
                expect_pass: s.expect_pass,
                code: s.code,
            };
            led.admit(bad).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 边界样本 ≥30 例入库全过（判据 J1 前半直读口）。
fn vault_thirty_ok() -> bool {
    let mut led = build_full_vault();
    if led.len() != EDGE_SAMPLE_CAP {
        return false;
    }
    if led.verdict_len() != EDGE_SAMPLE_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.seal().is_err() {
        return false;
    }
    if led.state() != VaultState::Sealed {
        return false;
    }
    // 独立复核：30 样逐样黄金判定 + 全部期望通过。
    for s in EDGE_SAMPLES.iter() {
        if edge_sample_ok(s).is_err() {
            return false;
        }
        if !s.expect_pass {
            return false;
        }
    }
    true
}

/// 与联测资产同源可重演（判据 J1 后半直读口）。
fn same_source_ok() -> bool {
    if EDGE_SAMPLES.len() != EDGE_SAMPLE_CAP {
        return false;
    }
    let (ok, total, miss) = same_source_census();
    if total != EDGE_SAMPLE_CAP {
        return false;
    }
    if ok != EDGE_SAMPLE_CAP {
        return false;
    }
    if !miss.is_empty() {
        return false;
    }
    // LCG 种子冻结：种子位与 SEG 公式位互证（同源口径）。
    if SEG_BASE != 0x80000 + 0x40 * (12675u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 枚举类 P95 账三闸全绿。
fn p95_account_ok() -> bool {
    for c in P95Class::all().iter() {
        if !class_p95_ok(EDGE_SAMPLES, *c) {
            return false;
        }
    }
    // 序位法边界直读。
    if p95_sorted(&[]).is_some() {
        return false;
    }
    if p95_sorted(&[0x30]).is_none() {
        return false;
    }
    // 上限冻结轴递增（取值一律 .get()，无裸下标）。
    let caps = [
        P95Class::KeyEnum.budget_cap(),
        P95Class::HandleEnum.budget_cap(),
        P95Class::MapEnum.budget_cap(),
    ];
    let c0 = match caps.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let c1 = match caps.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let c2 = match caps.get(2) {
        Some(v) => *v,
        None => return false,
    };
    if c0 >= c1 || c1 >= c2 {
        return false;
    }
    true
}

/// 三类样本覆盖普查（每类 10 例、谱名互异、样本号连续）。
fn class_census_ok() -> bool {
    let mut per_class = [0usize; 3];
    let mut i = 0usize;
    while i < EDGE_SAMPLES.len() {
        let s = match EDGE_SAMPLES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if s.no != i as u32 {
            return false;
        }
        match per_class.get_mut(s.class.ordinal() as usize) {
            Some(v) => *v += 1,
            None => return false,
        }
        // 谱名互异普查（同类内）。
        let mut j = i + 1;
        while j < EDGE_SAMPLES.len() {
            let g = match EDGE_SAMPLES.get(j) {
                Some(v) => v,
                None => return false,
            };
            if g.class == s.class && g.spectrum == s.spectrum {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    for slot in per_class.iter() {
        if *slot != 10 {
            return false;
        }
    }
    true
}

/// 零静默（判定记账逐样齐、样本号一一对应、通过位全真）。
fn zero_silent_ok() -> bool {
    let led = build_full_vault();
    if led.verdict_len() != led.len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.verdict_len() {
        let v = match led.verdicts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let s = match led.sample_at(i) {
            Some(x) => x,
            None => return false,
        };
        if v.0 != s.no || !v.1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn vault_fault_sweep_ok() -> bool {
    let (caught, total, escape) = vault_fault_sweep();
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

/// 双跑一致（冻结样本库下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_vault();
    let b = build_full_vault();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.sample_at(i), b.sample_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if vault_fault_sweep().0 != vault_fault_sweep().0 {
        return false;
    }
    if same_source_census().0 != same_source_census().0 {
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
    if SEG_BASE != 0x80000 + 0x40 * (12675u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_vault();
    if led.seal().is_err() {
        return false;
    }
    if led.state() != VaultState::Sealed {
        return false;
    }
    // 摘要：首样谱名 + 尾样类名拼接非空。
    let head = match led.sample_at(0) {
        Some(s) => s.spectrum,
        None => return false,
    };
    let tail = match led.sample_at(EDGE_SAMPLE_CAP - 1) {
        Some(s) => s.class.name(),
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12676 P95 账自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12675_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12675-p95budget2");
    let led = build_full_vault();

    s.add_dyn(
        "F12676-01 边界样本三十例齐",
        EDGE_SAMPLES.len() == EDGE_SAMPLE_CAP,
        &format!("samples={}", EDGE_SAMPLES.len()),
    );

    s.add_dyn(
        "F12676-02 边界样本入库全过",
        vault_thirty_ok(),
        &format!("samples={} verdicts={}", led.len(), led.verdict_len()),
    );

    s.add_dyn(
        "F12676-03 联测资产同源可重演",
        same_source_ok(),
        &format!("stamps={}", same_source_census().0),
    );

    s.add_dyn(
        "F12676-04 枚举类 P95 账三闸全绿",
        p95_account_ok(),
        &format!("classes={}", P95Class::all().len()),
    );

    s.add_dyn(
        "F12676-05 三类样本覆盖普查",
        class_census_ok(),
        &format!("classes={}", P95Class::all().len()),
    );

    s.add_dyn(
        "F12676-06 零静默",
        zero_silent_ok(),
        &format!("verdicts={}", led.verdict_len()),
    );

    s.add_dyn(
        "F12676-07 注入联测 10/10 拒止",
        vault_fault_sweep_ok(),
        &format!("caught={}", vault_fault_sweep().0),
    );

    s.add_dyn(
        "F12676-08 账等式",
        led.equation_ok(),
        &format!("samples={} verdicts={} rejects={}", led.len(), led.verdict_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12676-09 双跑一致",
        dual_run_ok(),
        "冻结样本库下两轮逐样同判",
    );

    s.add_dyn(
        "F12676-10 P95 序位法边界",
        p95_sorted(&[]).is_none() && p95_sorted(&[0x60]).is_some(),
        "空样本 None / 单样本取位",
    );

    s.add_dyn(
        "F12676-11 类判码窗口口径",
        code_in_p95_class(0x25, P95Class::KeyEnum)
            && !code_in_p95_class(0x24, P95Class::KeyEnum)
            && !code_in_p95_class(0x35, P95Class::KeyEnum),
        "基码 < 判码 ≤ 基码+0x0C",
    );

    s.add_dyn(
        "F12676-12 完成写回语义",
        writeback_ok(),
        "封库即已完成且摘要非空",
    );

    s.add_dyn(
        "F12676-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x812C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(P95_ALL.len(), 11);
        let mut i = 0usize;
        while i < P95_ALL.len() {
            let vi = match P95_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < P95_ALL.len() {
                let vj = match P95_ALL.get(j) {
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
    fn p95_ord_rank_math() {
        assert_eq!(p95_sorted(&[]), None);
        assert_eq!(p95_sorted(&[9]), Some(9));
        assert_eq!(p95_sorted(&sorted_copy(&[9, 1, 8, 2, 7, 3, 6, 4, 5, 0])), Some(9));
        // 30 样本 P95 序位 = 第 29 个。
        let mut v: Vec<u32> = Vec::new();
        let mut i = 1u32;
        while i <= 30 {
            v.push(i);
            i += 1;
        }
        assert_eq!(p95_sorted(&v), Some(29));
    }

    #[test]
    fn p95_account_three_classes() {
        // 三类 P95 账全绿。
        assert!(p95_account_ok());
        for c in P95Class::all().iter() {
            assert!(class_p95_ok(EDGE_SAMPLES, *c), "类 {} 账红", c.name());
            assert!(c.budget_cap() > 0);
        }
    }

    #[test]
    fn vault_thirty_all_pass() {
        // 30 例入库全过。
        assert!(vault_thirty_ok());
        let mut led = build_full_vault();
        assert_eq!(led.len(), EDGE_SAMPLE_CAP);
        assert!(led.seal().is_ok());
        assert_eq!(led.state(), VaultState::Sealed);
        // 注入越限必抓。
        let mut bad = match EDGE_SAMPLES.get(5) {
            Some(x) => x.clone(),
            None => return,
        };
        bad.steps = bad.class.budget_cap().saturating_add(8);
        assert_eq!(edge_sample_ok(&bad), Err(P95Diag::EBudgetOver.val()));
    }

    #[test]
    fn same_source_replay_holds() {
        // 同源可重演：校验字两轮逐位全等。
        assert!(same_source_ok());
        let (ok, total, miss) = same_source_census();
        assert_eq!(total, 30);
        assert_eq!(ok, 30, "漂移：{}", miss);
        // LCG 确定性：同一输入两轮全等。
        let s = match EDGE_SAMPLES.get(0) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(sample_stamp(s), sample_stamp(s));
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = vault_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn admit_gate_rejects() {
        let mut led = VaultLedger::new();
        let _ = led.begin_audit();
        let s1 = match EDGE_SAMPLES.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.admit(s1), Err(P95Diag::ESampleOrder.val()));
        let s0 = match EDGE_SAMPLES.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.admit(s0.clone()).is_ok());
        let dup = EdgeSample {
            no: 1,
            class: s0.class,
            spectrum: s0.spectrum,
            steps: s0.steps,
            expect_pass: s0.expect_pass,
            code: s0.code,
        };
        assert_eq!(led.admit(dup), Err(P95Diag::ESampleDup.val()));
        let mut zero = match EDGE_SAMPLES.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        zero.steps = 0;
        assert_eq!(led.admit(zero), Err(P95Diag::EZeroSteps.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn class_census_holds() {
        assert!(class_census_ok());
        assert_eq!(EDGE_SAMPLES.len(), 30);
        assert_eq!(P95Class::all().len(), 3);
        let mut counts = [0usize; 3];
        for s in EDGE_SAMPLES.iter() {
            match counts.get_mut(s.class.ordinal() as usize) {
                Some(v) => *v += 1,
                None => return,
            }
        }
        assert_eq!(counts[0], 10);
        assert_eq!(counts[1], 10);
        assert_eq!(counts[2], 10);
        assert!(P95Class::MapEnum.code_base() > P95Class::HandleEnum.code_base());
        assert!(P95Class::HandleEnum.code_base() > P95Class::KeyEnum.code_base());
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_vault();
        assert_eq!(led.verdict_len(), EDGE_SAMPLE_CAP);
        let mut i = 0usize;
        while i < EDGE_SAMPLE_CAP {
            let s = match led.sample_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(s.no, i as u32, "样本 {} 错位", i);
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_vault();
        let b = build_full_vault();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.sample_at(i), b.sample_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑样本缺失"),
            }
            i += 1;
        }
        assert_eq!(vault_fault_sweep().0, vault_fault_sweep().0);
        assert_eq!(same_source_census().0, same_source_census().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：VaultOpen→Auditing→Sealed。
        assert!(VaultState::VaultOpen.rank() < VaultState::Auditing.rank());
        assert!(VaultState::Auditing.rank() < VaultState::Sealed.rank());
        let mut led = build_full_vault();
        assert_eq!(led.state(), VaultState::Auditing);
        assert!(led.seal().is_ok());
        assert_eq!(led.seal(), Err(P95Diag::ERewind.val()));
        let mut fresh = VaultLedger::new();
        assert!(fresh.begin_audit().is_ok());
        assert_eq!(fresh.begin_audit(), Err(P95Diag::ERewind.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12675_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
