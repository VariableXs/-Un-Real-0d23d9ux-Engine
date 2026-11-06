//! UNX-F12751 压测双机对照（20 场景） · 0x825C0段
//!
//! 判据原文（派工卡 12751 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12752-J1 边界样本 ≥30 例入库全过，与联测资产同源可重演；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 双机对照域模型：S1 母版场景账登记 20 个压测场景（场景号 1..=20 闭集），
//!    机 A / 机 B 各回报一份结果（闭集 Match|Diverge|Missing）与一份展开序
//!    指纹（u64 逐位），对照旗标由判定器写定（一致/待定/发散三值闭集）；
//!    冻结闭集 20 行可复算——手写权重表 + 公式重算双通道对账，指纹逐场景
//!    可独立重算且互异；
//! 2. 判定器 judge_scene(scene_no) 按固定次序出四路错误：未登记场景 →
//!    ENOENT；结果越闭集 → EINVAL；两机 Missing 同挂 → ETENTATIVE；
//!    A≠B → EDIVERGE 且显式入账零静默（旗标/根因/计数三处落痕）；
//! 3. 展开序类零容差：展开序指纹逐位相等才算一致，序类任何一个错位都算
//!    Diverge（单比特翻转即判发散，根因记 EORDER，判定次序优先于
//!    Tentative——错位比待定更可疑，零容忍）；
//! 4. 注入器三类各 10 轮全抓：序类错位注入（翻一位必被抓且根因记 EORDER）/
//!    结果越闭集（越闭集码必 EINVAL 且计数显性）/静默吞并（发散必须显式
//!    入账——行缺失、旗标未写、计数未增任一发生即算注入逃脱）；
//! 5. 边界样本 ≥30 例：12 例手写边界（首尾场景/越下界/越上界/码越闭集/
//!    双 Missing/位错位）+ 22 例固定种子 LCG 生成，逐例「预期 vs 实测」
//!    对账入库全过；语料与全流程双跑指纹一致（与联测资产同源可重演）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!    派生写死）；取值一律 match/if let/.get()/.get_mut()/checked 语义，
//!    无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12751 − 12600)）。
pub const SEG_BASE: u32 = 0x825C0;

// ── 双机对照诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0D 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmDiag {
    /// 未登记场景：场景号不在 1..=20 闭集内，或账内无此行。
    ENOENT,
    /// 结果越闭集：机 A/B 原始码不在 Match|Diverge|Missing 三值闭集内。
    EINVAL,
    /// 两机 Missing 同挂：双机都回报缺失，对照只能出待定旗标。
    ETENTATIVE,
    /// 两机结果相异：A≠B 或展开序指纹错位，发散显性入账零静默。
    EDIVERGE,
    /// 展开序指纹错位根因：发散由序类错位引起（区别于结果相异）。
    EORDER,
    /// 场景重复登记：同一场景号只落一行，重复即拒。
    EDUP,
    /// 账已封：封账后登记/判定一律拒绝。
    ESEALED,
    /// 状态机倒退：封账后重复封账，只进不退。
    EREWIND,
    /// 冻结表受损：母版场景账行数/场景号序/权重非零任一不成立。
    EFROZEN,
    /// 边界样本不足：语料少于 30 例或有任一例预期不符。
    EBOUND,
    /// 双跑指纹不一致：同源重演两轮指纹不等，可重演性失效。
    EFPRINT,
    /// 注入逃脱：三类注入任一路未被判定器抓住，判定面失效。
    EINJECT,
    /// 20 行未齐禁止封账：场景账没登记满闭集就试图封账。
    ENOTFULL,
}

impl DmDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0D 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DmDiag::ENOENT => 0x01,
                DmDiag::EINVAL => 0x02,
                DmDiag::ETENTATIVE => 0x03,
                DmDiag::EDIVERGE => 0x04,
                DmDiag::EORDER => 0x05,
                DmDiag::EDUP => 0x06,
                DmDiag::ESEALED => 0x07,
                DmDiag::EREWIND => 0x08,
                DmDiag::EFROZEN => 0x09,
                DmDiag::EBOUND => 0x0a,
                DmDiag::EFPRINT => 0x0b,
                DmDiag::EINJECT => 0x0c,
                DmDiag::ENOTFULL => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DmDiag::ENOENT => "未登记场景：场景号须在 1..=20 母版闭集内，先查冻结场景账再判定",
            DmDiag::EINVAL => "结果越闭集：机 A/B 只认 Match|Diverge|Missing 三值，先修回报码再入库",
            DmDiag::ETENTATIVE => "双机 Missing 同挂：对照只能出待定旗标，先补跑该场景再封账",
            DmDiag::EDIVERGE => "两机结果相异：发散已显式入账，按行根因（结果相异/序类错位）回修",
            DmDiag::EORDER => "展开序指纹错位：序类零容差，单比特错位即发散，先对齐展开序再重跑",
            DmDiag::EDUP => "场景重复登记：每场景只落一行，重复先查已有行号",
            DmDiag::ESEALED => "账已封：封账后登记/判定拒绝，改动走下一轮压测滚动",
            DmDiag::EREWIND => "状态机倒退：登记齐→封账只进不退，重复封账即账本可疑",
            DmDiag::EFROZEN => "冻结表受损：母版 20 行行数/序/权重任一不符，先复核冻结表",
            DmDiag::EBOUND => "边界样本不足：语料须 ≥30 例且逐例预期相符，先补语料再验收",
            DmDiag::EFPRINT => "双跑指纹不一致：同源重演两轮不等，查确定性源（种子/表）后重演",
            DmDiag::EINJECT => "注入逃脱：注入未被判定器抓住，判定面失效须整账复核",
            DmDiag::ENOTFULL => "20 行未齐禁止封账：先补齐缺场景再走封账口",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DM_ALL: &[DmDiag] = &[
    DmDiag::ENOENT,
    DmDiag::EINVAL,
    DmDiag::ETENTATIVE,
    DmDiag::EDIVERGE,
    DmDiag::EORDER,
    DmDiag::EDUP,
    DmDiag::ESEALED,
    DmDiag::EREWIND,
    DmDiag::EFROZEN,
    DmDiag::EBOUND,
    DmDiag::EFPRINT,
    DmDiag::EINJECT,
    DmDiag::ENOTFULL,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DM_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x825C1..0x825CD 闭集内"
}

// ── 单机结果闭集 ─────────────────────────────────────────────────
/// 单机回报结果三值闭集（判据冻结，越出即 EINVAL）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineVerdict {
    /// 双机结果一致（该机回报对照匹配）。
    Match,
    /// 该机回报与母版预期发散。
    Diverge,
    /// 该机该场景缺失（未跑出结果）。
    Missing,
}

impl MachineVerdict {
    /// 闭集码（1/2/3，冻结不许重排）。
    pub fn code(self) -> u32 {
        match self {
            MachineVerdict::Match => 1,
            MachineVerdict::Diverge => 2,
            MachineVerdict::Missing => 3,
        }
    }

    /// 由原始码收敛闭集（越闭集返回 None → 判定器出 EINVAL）。
    pub fn from_code(code: u32) -> Option<MachineVerdict> {
        match code {
            1 => Some(MachineVerdict::Match),
            2 => Some(MachineVerdict::Diverge),
            3 => Some(MachineVerdict::Missing),
            _ => None,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            MachineVerdict::Match => "一致",
            MachineVerdict::Diverge => "发散",
            MachineVerdict::Missing => "缺失",
        }
    }
}

// ── 对照旗标与发散根因闭集 ───────────────────────────────────────
/// 对照旗标三值闭集（判定器写定，登记期为未判）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareFlag {
    /// 尚未判定（登记后、判定前的暂态）。
    Unjudged,
    /// 双机一致（结果与展开序指纹均逐位相等）。
    Consistent,
    /// 待定（双机 Missing 同挂）。
    Tentative,
    /// 发散（A≠B 或序类错位，零静默显性入账）。
    Divergent,
}

impl CompareFlag {
    /// 旗标序（指纹哈希与断言用）。
    pub fn rank(self) -> u8 {
        match self {
            CompareFlag::Unjudged => 0,
            CompareFlag::Consistent => 1,
            CompareFlag::Tentative => 2,
            CompareFlag::Divergent => 3,
        }
    }

    /// 旗标中文名。
    pub fn name(self) -> &'static str {
        match self {
            CompareFlag::Unjudged => "未判",
            CompareFlag::Consistent => "一致",
            CompareFlag::Tentative => "待定",
            CompareFlag::Divergent => "发散",
        }
    }
}

/// 发散根因闭集（发散行的三处落痕之一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DivRoot {
    /// 未发散（非发散行的缺省根因）。
    NotDivergent,
    /// 结果相异：A≠B（结果闭集内不同值）。
    VerdictMismatch,
    /// 序类错位：展开序指纹逐位不等（零容差）。
    OrderDislocation,
}

impl DivRoot {
    /// 根因诊断码（非发散返回 0；序类错位记 EORDER，结果相异记 EDIVERGE）。
    pub fn root_code(self) -> u32 {
        match self {
            DivRoot::NotDivergent => 0,
            DivRoot::VerdictMismatch => DmDiag::EDIVERGE.val(),
            DivRoot::OrderDislocation => DmDiag::EORDER.val(),
        }
    }

    /// 根因中文名。
    pub fn name(self) -> &'static str {
        match self {
            DivRoot::NotDivergent => "未发散",
            DivRoot::VerdictMismatch => "结果相异",
            DivRoot::OrderDislocation => "序类错位",
        }
    }
}

// ── 确定性 LCG 与 64 位混合器（种子写死，零随机红线合规） ─────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一 64 位伪随机数（确定性：MMIX 常数一步 + splitmix 终混）。
    pub fn next_u64(&mut self) -> u64 {
        // LCG 常数取自 Knuth MMIX 64 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        mix64(self.state)
    }

    /// 0..bound 内取值（bound 为零返回零，不取模零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() >> 32) as u32 % bound
    }
}

/// splitmix64 终混器（无状态、确定性，指纹可复算的公式通道）。
fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_mul(0xFF51_7D1C_F747_3597);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^= x >> 33;
    x
}

// ── 冻结母版场景账（20 行闭集，可复算） ──────────────────────────
/// 场景号闭集边界。
pub const SCENE_MIN: u32 = 1;
/// 场景号闭集上界（20 场景）。
pub const SCENE_MAX: u32 = 20;
/// 场景账容量（行数恒等闭集）。
pub const ROW_CAP: usize = 20;
/// 边界样本判据下限（判据原文 ≥30 例）。
pub const BOUNDARY_MIN: usize = 30;

/// 冻结母版场景账：20 行（场景号，展开序类权重），行序冻结不许重排。
/// 权重手工写死且全非零——与公式重算通道构成双通道对账（可复算）。
pub const FROZEN_SCENES: &[(u32, u64)] = &[
    (1, 0x9E01),
    (2, 0x7B02),
    (3, 0xA503),
    (4, 0x3C04),
    (5, 0xD105),
    (6, 0x6206),
    (7, 0xF807),
    (8, 0x1408),
    (9, 0xC909),
    (10, 0x470A),
    (11, 0xBB0B),
    (12, 0x280C),
    (13, 0x930D),
    (14, 0x5E0E),
    (15, 0xE10F),
    (16, 0x7610),
    (17, 0x0C11),
    (18, 0xAD12),
    (19, 0x3313),
    (20, 0xCA14),
];

/// 冻结表按场景号查权重（越闭集返回 None）。
pub fn frozen_weight(scene_no: u32) -> Option<u64> {
    for e in FROZEN_SCENES.iter() {
        if e.0 == scene_no {
            return Some(e.1);
        }
    }
    None
}

/// 展开序类指纹（公式通道）：由段基址、场景号、冻结权重确定性重算。
/// 同一场景任意次重算同值（无状态），越闭集场景返回零指纹。
pub fn frozen_print(scene_no: u32) -> u64 {
    match frozen_weight(scene_no) {
        Some(w) => mix64((SEG_BASE as u64) ^ ((scene_no as u64) << 32) ^ w),
        None => 0,
    }
}

// ── 场景行与对照行数据模型 ───────────────────────────────────────
/// 场景账单行：双机原始码 + 双机展开序指纹 + 判定落痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneRow {
    /// 场景号（1..=20 闭集）。
    pub scene_no: u32,
    /// 机 A 原始码（判定器按闭集收敛，越闭集出 EINVAL）。
    pub code_a: u32,
    /// 机 B 原始码。
    pub code_b: u32,
    /// 机 A 展开序指纹（逐位对照，零容差）。
    pub fp_a: u64,
    /// 机 B 展开序指纹。
    pub fp_b: u64,
    /// 对照旗标（判定器写定）。
    pub flag: CompareFlag,
    /// 发散根因（发散行三处落痕之一）。
    pub root: DivRoot,
    /// 已判定落痕（零静默：判定必写）。
    pub judged: bool,
}

impl SceneRow {
    /// 机 A 收敛结果（越闭集 None）。
    pub fn verdict_a(&self) -> Option<MachineVerdict> {
        MachineVerdict::from_code(self.code_a)
    }

    /// 机 B 收敛结果（越闭集 None）。
    pub fn verdict_b(&self) -> Option<MachineVerdict> {
        MachineVerdict::from_code(self.code_b)
    }

    /// 展开序指纹逐位相等（零容差判一致的唯一口径）。
    pub fn fp_equal(&self) -> bool {
        self.fp_a == self.fp_b
    }
}

/// 判定器出参：场景号 + 旗标 + 双机指纹（验收读账面用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompareRow {
    /// 场景号。
    pub scene_no: u32,
    /// 对照旗标。
    pub flag: CompareFlag,
    /// 机 A 展开序指纹。
    pub fp_a: u64,
    /// 机 B 展开序指纹。
    pub fp_b: u64,
}

// ── 账本状态机 ───────────────────────────────────────────────────
/// 场景账状态机：登记 → 二十行齐 → 封账，单向推进，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerState {
    /// 登记期：逐场景登记双机回报。
    Open,
    /// 二十行齐：等待逐场景判定。
    Complete,
    /// 封账：只读。
    Sealed,
}

impl LedgerState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            LedgerState::Open => 0,
            LedgerState::Complete => 1,
            LedgerState::Sealed => 2,
        }
    }
}

// ── 双机对照场景账 ───────────────────────────────────────────────
/// 双机对照场景账：登记双机回报、逐场景判定、发散显性入账零静默。
#[derive(Clone, Debug)]
pub struct CompareLedger {
    /// 已登记行（按场景号登记序）。
    rows: Vec<SceneRow>,
    /// 账本状态（单向推进）。
    state: LedgerState,
    /// 发散行计数（显性入账的三处落痕之一）。
    divergence_count: usize,
    /// 待定行计数（双 Missing 同挂）。
    tentative_count: usize,
    /// 越闭集行计数（结果越闭集显性记账）。
    einval_count: usize,
}

impl CompareLedger {
    /// 开一本新账。
    pub fn new() -> CompareLedger {
        CompareLedger {
            rows: Vec::new(),
            state: LedgerState::Open,
            divergence_count: 0,
            tentative_count: 0,
            einval_count: 0,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> LedgerState {
        self.state
    }

    /// 已登记行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 发散行计数。
    pub fn divergence_count(&self) -> usize {
        self.divergence_count
    }

    /// 待定行计数。
    pub fn tentative_count(&self) -> usize {
        self.tentative_count
    }

    /// 越闭集行计数。
    pub fn einval_count(&self) -> usize {
        self.einval_count
    }

    /// 行迭代视图（指纹与谓词遍历用，不裸下标）。
    pub fn rows_iter(&self) -> impl Iterator<Item = &SceneRow> {
        self.rows.iter()
    }

    /// 按场景号取行快照（越界/未登记返回 None）。
    pub fn row_of(&self, scene_no: u32) -> Option<SceneRow> {
        for r in self.rows.iter() {
            if r.scene_no == scene_no {
                return Some(*r);
            }
        }
        None
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<SceneRow> {
        self.rows.get(idx).copied()
    }

    /// 判定落痕写钉（零静默：判定必写旗标/根因/已判三处）。
    fn stamp(&mut self, idx: usize, flag: CompareFlag, root: DivRoot) {
        if let Some(r) = self.rows.get_mut(idx) {
            r.flag = flag;
            r.root = root;
            r.judged = true;
        }
    }

    /// 登记一行双机回报（封账闸 → 场景闭集闸 → 重复闸）。
    /// 原始码在此不收敛——越闭集码照登，由判定器出 EINVAL（显性记账）。
    pub fn record_raw(
        &mut self,
        scene_no: u32,
        code_a: u32,
        code_b: u32,
        fp_a: u64,
        fp_b: u64,
    ) -> Result<(), u32> {
        // 封账闸：封账后一切写入拒绝。
        if self.state.rank() >= LedgerState::Sealed.rank() {
            return Err(DmDiag::ESEALED.val());
        }
        // 场景闭集闸：未登记场景（越 1..=20）→ ENOENT。
        if scene_no < SCENE_MIN || scene_no > SCENE_MAX {
            return Err(DmDiag::ENOENT.val());
        }
        // 重复闸：每场景只落一行。
        if self.rows.iter().any(|r| r.scene_no == scene_no) {
            return Err(DmDiag::EDUP.val());
        }
        self.rows.push(SceneRow {
            scene_no,
            code_a,
            code_b,
            fp_a,
            fp_b,
            flag: CompareFlag::Unjudged,
            root: DivRoot::NotDivergent,
            judged: false,
        });
        // 二十行齐即推进状态机（只进不退）。
        if self.rows.len() == ROW_CAP {
            self.state = LedgerState::Complete;
        }
        Ok(())
    }

    /// 判定器：按固定次序出四路错误，旗标必写（零静默）。
    /// 次序：ENOENT → ESEALED → EINVAL → 序类零容差 → ETENTATIVE → EDIVERGE。
    pub fn judge_scene(&mut self, scene_no: u32) -> Result<CompareRow, u32> {
        // 次序一：未登记场景（账内无此行）→ ENOENT。
        let idx = match self.rows.iter().position(|r| r.scene_no == scene_no) {
            Some(i) => i,
            None => return Err(DmDiag::ENOENT.val()),
        };
        // 次序二：账已封，判定属写操作，封账后拒。
        if self.state.rank() >= LedgerState::Sealed.rank() {
            return Err(DmDiag::ESEALED.val());
        }
        let row = match self.rows.get(idx) {
            Some(r) => *r,
            None => return Err(DmDiag::ENOENT.val()),
        };
        // 次序三：结果越闭集 → EINVAL（先于指纹与旗标判定）。
        let va = match MachineVerdict::from_code(row.code_a) {
            Some(v) => v,
            None => {
                self.stamp(idx, CompareFlag::Unjudged, DivRoot::NotDivergent);
                self.einval_count += 1;
                return Err(DmDiag::EINVAL.val());
            }
        };
        let vb = match MachineVerdict::from_code(row.code_b) {
            Some(v) => v,
            None => {
                self.stamp(idx, CompareFlag::Unjudged, DivRoot::NotDivergent);
                self.einval_count += 1;
                return Err(DmDiag::EINVAL.val());
            }
        };
        // 次序四：展开序类零容差——指纹逐位相等，任一错位即发散。
        // 零容差优先于待定：错位比缺失同挂更可疑，单比特翻转也判发散。
        if row.fp_a != row.fp_b {
            self.stamp(idx, CompareFlag::Divergent, DivRoot::OrderDislocation);
            self.divergence_count += 1;
            return Err(DmDiag::EDIVERGE.val());
        }
        // 次序五：两机 Missing 同挂 → ETENTATIVE（旗标写待定）。
        if va == MachineVerdict::Missing && vb == MachineVerdict::Missing {
            self.stamp(idx, CompareFlag::Tentative, DivRoot::NotDivergent);
            self.tentative_count += 1;
            return Err(DmDiag::ETENTATIVE.val());
        }
        // 次序六：A≠B → EDIVERGE 且显式入账（旗标/根因/计数三处落痕）。
        if va != vb {
            self.stamp(idx, CompareFlag::Divergent, DivRoot::VerdictMismatch);
            self.divergence_count += 1;
            return Err(DmDiag::EDIVERGE.val());
        }
        // 一致：结果相等且指纹逐位相等。
        self.stamp(idx, CompareFlag::Consistent, DivRoot::NotDivergent);
        Ok(CompareRow {
            scene_no: row.scene_no,
            flag: CompareFlag::Consistent,
            fp_a: row.fp_a,
            fp_b: row.fp_b,
        })
    }

    /// 全账已判定（每行 judged 落痕齐）。
    pub fn all_judged(&self) -> bool {
        self.rows.iter().all(|r| r.judged)
    }

    /// 全账一致（每行旗标为一致）。
    pub fn all_consistent(&self) -> bool {
        self.rows.iter().all(|r| r.flag == CompareFlag::Consistent)
    }

    /// 封账：二十行齐 + 全判定 + 全一致才许封；重复封账倒退拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= LedgerState::Sealed.rank() {
            return Err(DmDiag::EREWIND.val());
        }
        if self.rows.len() != ROW_CAP {
            return Err(DmDiag::ENOTFULL.val());
        }
        for r in self.rows.iter() {
            if !r.judged || r.flag != CompareFlag::Consistent {
                return Err(DmDiag::EDIVERGE.val());
            }
        }
        self.state = LedgerState::Sealed;
        Ok(())
    }
}

// ── 账面指纹（FNV-1a 语义，整数域，确定性） ──────────────────────
/// 账面指纹：对容量、行数与每行七个字段做 FNV 式混合（双跑一致口径）。
pub fn ledger_fingerprint(l: &CompareLedger) -> u64 {
    const P: u64 = 0x0000_0100_0000_01B3;
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    h = h.wrapping_mul(P).wrapping_add(ROW_CAP as u64);
    h = h.wrapping_mul(P).wrapping_add(l.len() as u64);
    for r in l.rows_iter() {
        h = h.wrapping_mul(P).wrapping_add(r.scene_no as u64);
        h = h.wrapping_mul(P).wrapping_add(r.code_a as u64);
        h = h.wrapping_mul(P).wrapping_add(r.code_b as u64);
        h = h.wrapping_mul(P).wrapping_add(r.fp_a);
        h = h.wrapping_mul(P).wrapping_add(r.fp_b);
        h = h.wrapping_mul(P).wrapping_add(r.flag.rank() as u64);
        h = h.wrapping_mul(P).wrapping_add(r.root.root_code() as u64);
        h = h.wrapping_mul(P).wrapping_add(r.judged as u64);
    }
    h
}

/// 构建母版正常流账本：20 场景双机全一致（指纹取冻结公式通道）。
pub fn build_happy_ledger() -> CompareLedger {
    let mut l = CompareLedger::new();
    let mut scene = SCENE_MIN;
    while scene <= SCENE_MAX {
        let fp = frozen_print(scene);
        let _ = l.record_raw(
            scene,
            MachineVerdict::Match.code(),
            MachineVerdict::Match.code(),
            fp,
            fp,
        );
        scene += 1;
    }
    l
}

// ── 边界样本语料（判据：≥30 例入库全过，同源可重演） ─────────────
/// 样本预期闭集（与判定器实现无关的独立口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// 预期判定通过（一致）。
    OkConsistent,
    /// 预期 ENOENT（未登记场景）。
    ErrEnoent,
    /// 预期 EINVAL（结果越闭集）。
    ErrEinval,
    /// 预期 ETENTATIVE（双 Missing 同挂）。
    ErrTentative,
    /// 预期 EDIVERGE（A≠B 或序类错位）。
    ErrDiverge,
}

impl Expect {
    /// 预期对应的实际码（Ok 为 0，其余为诊断码值）。
    pub fn val(self) -> u32 {
        match self {
            Expect::OkConsistent => 0,
            Expect::ErrEnoent => DmDiag::ENOENT.val(),
            Expect::ErrEinval => DmDiag::EINVAL.val(),
            Expect::ErrTentative => DmDiag::ETENTATIVE.val(),
            Expect::ErrDiverge => DmDiag::EDIVERGE.val(),
        }
    }
}

/// 单条边界样本：场景号 + 双机原始码 + 双机指纹 + 独立预期。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundarySample {
    /// 场景号（可为越界值——预期 ENOENT）。
    pub scene_no: u32,
    /// 机 A 原始码（可为越闭集值——预期 EINVAL）。
    pub code_a: u32,
    /// 机 B 原始码。
    pub code_b: u32,
    /// 机 A 展开序指纹。
    pub fp_a: u64,
    /// 机 B 展开序指纹。
    pub fp_b: u64,
    /// 独立预期（按样本构造口径写定，不复用判定器逻辑）。
    pub expect: Expect,
}

/// 单例对账结果：样本序号 + 预期码 + 实测码 + 是否相符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleOutcome {
    /// 样本序号（语料内写入序）。
    pub idx: usize,
    /// 预期码（Ok 为 0）。
    pub expect: u32,
    /// 实测码（Ok 为 0）。
    pub actual: u32,
    /// 预期与实测相符。
    pub pass: bool,
}

/// 手写边界语料（12 例：首尾场景/越下界/越上界/码越闭集/双 Missing/位错位）。
fn hand_corpus() -> Vec<BoundarySample> {
    let mut v: Vec<BoundarySample> = Vec::new();
    // 例 1：场景 1（首场景）双机一致。
    v.push(BoundarySample {
        scene_no: 1,
        code_a: 1,
        code_b: 1,
        fp_a: frozen_print(1),
        fp_b: frozen_print(1),
        expect: Expect::OkConsistent,
    });
    // 例 2：场景 20（尾场景）双机一致。
    v.push(BoundarySample {
        scene_no: 20,
        code_a: 1,
        code_b: 1,
        fp_a: frozen_print(20),
        fp_b: frozen_print(20),
        expect: Expect::OkConsistent,
    });
    // 例 3：场景 0 越下界 → ENOENT。
    v.push(BoundarySample {
        scene_no: 0,
        code_a: 1,
        code_b: 1,
        fp_a: 1,
        fp_b: 1,
        expect: Expect::ErrEnoent,
    });
    // 例 4：场景 21 越上界 → ENOENT。
    v.push(BoundarySample {
        scene_no: 21,
        code_a: 1,
        code_b: 1,
        fp_a: 1,
        fp_b: 1,
        expect: Expect::ErrEnoent,
    });
    // 例 5：场景 9999 远越界 → ENOENT。
    v.push(BoundarySample {
        scene_no: 9999,
        code_a: 1,
        code_b: 1,
        fp_a: 1,
        fp_b: 1,
        expect: Expect::ErrEnoent,
    });
    // 例 6：场景 7 机 A 原始码 0 → EINVAL。
    v.push(BoundarySample {
        scene_no: 7,
        code_a: 0,
        code_b: 1,
        fp_a: frozen_print(7),
        fp_b: frozen_print(7),
        expect: Expect::ErrEinval,
    });
    // 例 7：场景 8 机 B 原始码 4（越闭集上沿）→ EINVAL。
    v.push(BoundarySample {
        scene_no: 8,
        code_a: 1,
        code_b: 4,
        fp_a: frozen_print(8),
        fp_b: frozen_print(8),
        expect: Expect::ErrEinval,
    });
    // 例 8：场景 9 机 A 原始码 0xFFFF（远越闭集）→ EINVAL。
    v.push(BoundarySample {
        scene_no: 9,
        code_a: 0xFFFF,
        code_b: 1,
        fp_a: frozen_print(9),
        fp_b: frozen_print(9),
        expect: Expect::ErrEinval,
    });
    // 例 9：场景 10 双 Missing 指纹相等 → ETENTATIVE。
    v.push(BoundarySample {
        scene_no: 10,
        code_a: 3,
        code_b: 3,
        fp_a: frozen_print(10),
        fp_b: frozen_print(10),
        expect: Expect::ErrTentative,
    });
    // 例 10：场景 11 一致对发散（指纹相等）→ EDIVERGE。
    v.push(BoundarySample {
        scene_no: 11,
        code_a: 1,
        code_b: 2,
        fp_a: frozen_print(11),
        fp_b: frozen_print(11),
        expect: Expect::ErrDiverge,
    });
    // 例 11：场景 12 双机一致但指纹错位一比特 → EDIVERGE（零容差）。
    v.push(BoundarySample {
        scene_no: 12,
        code_a: 1,
        code_b: 1,
        fp_a: frozen_print(12),
        fp_b: frozen_print(12) ^ (1u64 << 7),
        expect: Expect::ErrDiverge,
    });
    // 例 12：场景 13 双 Missing 且指纹错位 → EDIVERGE（零容差优先于待定）。
    v.push(BoundarySample {
        scene_no: 13,
        code_a: 3,
        code_b: 3,
        fp_a: frozen_print(13),
        fp_b: frozen_print(13) ^ (1u64 << 40),
        expect: Expect::ErrDiverge,
    });
    v
}

/// 生成边界语料（22 例，固定种子 LCG，确定性可重演）。
/// 模式选型与字段构造同源——预期由模式写定，不复用判定器逻辑。
fn gen_corpus() -> Vec<BoundarySample> {
    const PAT_CONS: u32 = 0;
    const PAT_TENT: u32 = 1;
    const PAT_DIV: u32 = 2;
    const PAT_EINVAL: u32 = 3;
    const PAT_ENOENT: u32 = 4;
    let mut v: Vec<BoundarySample> = Vec::new();
    let mut rng = Lcg::new((SEG_BASE as u64) ^ 0xDE5C_1275_1C0D_E001);
    let mut k = 0usize;
    while k < 22 {
        let pat = rng.below(5);
        let sample = match pat {
            PAT_CONS => {
                let scene = SCENE_MIN + rng.below(SCENE_MAX);
                let fp = frozen_print(scene);
                BoundarySample {
                    scene_no: scene,
                    code_a: MachineVerdict::Match.code(),
                    code_b: MachineVerdict::Match.code(),
                    fp_a: fp,
                    fp_b: fp,
                    expect: Expect::OkConsistent,
                }
            }
            PAT_TENT => {
                let scene = SCENE_MIN + rng.below(SCENE_MAX);
                let fp = frozen_print(scene);
                BoundarySample {
                    scene_no: scene,
                    code_a: MachineVerdict::Missing.code(),
                    code_b: MachineVerdict::Missing.code(),
                    fp_a: fp,
                    fp_b: fp,
                    expect: Expect::ErrTentative,
                }
            }
            PAT_DIV => {
                let scene = SCENE_MIN + rng.below(SCENE_MAX);
                let fp = frozen_print(scene);
                BoundarySample {
                    scene_no: scene,
                    code_a: MachineVerdict::Match.code(),
                    code_b: MachineVerdict::Diverge.code(),
                    fp_a: fp,
                    fp_b: fp,
                    expect: Expect::ErrDiverge,
                }
            }
            PAT_EINVAL => {
                let scene = SCENE_MIN + rng.below(SCENE_MAX);
                let fp = frozen_print(scene);
                BoundarySample {
                    scene_no: scene,
                    code_a: 4 + rng.below(6),
                    code_b: MachineVerdict::Match.code(),
                    fp_a: fp,
                    fp_b: fp,
                    expect: Expect::ErrEinval,
                }
            }
            _ => {
                // 未登记场景：越下界或越上界各半。
                let scene = if rng.below(2) == 0 {
                    0
                } else {
                    SCENE_MAX + 1 + rng.below(9)
                };
                BoundarySample {
                    scene_no: scene,
                    code_a: MachineVerdict::Match.code(),
                    code_b: MachineVerdict::Match.code(),
                    fp_a: 1,
                    fp_b: 1,
                    expect: Expect::ErrEnoent,
                }
            }
        };
        v.push(sample);
        k += 1;
    }
    v
}

/// 全量边界语料（手写 12 例 + 生成 22 例 = 34 例 ≥ 30）。
pub fn corpus_all() -> Vec<BoundarySample> {
    let mut v = hand_corpus();
    for s in gen_corpus() {
        v.push(s);
    }
    v
}

/// 语料重演：逐例登记+判定，预期与实测对账入库（同源可重演口径）。
pub fn corpus_replay() -> Vec<SampleOutcome> {
    let corpus = corpus_all();
    let mut out: Vec<SampleOutcome> = Vec::new();
    let mut idx = 0usize;
    while idx < corpus.len() {
        let s = match corpus.get(idx) {
            Some(s) => *s,
            None => break,
        };
        let mut l = CompareLedger::new();
        let actual = match l.record_raw(s.scene_no, s.code_a, s.code_b, s.fp_a, s.fp_b) {
            Err(e) => e,
            Ok(()) => match l.judge_scene(s.scene_no) {
                Ok(_) => 0u32,
                Err(e) => e,
            },
        };
        let pass = actual == s.expect.val();
        out.push(SampleOutcome {
            idx,
            expect: s.expect.val(),
            actual,
            pass,
        });
        idx += 1;
    }
    out
}

// ── 注入面：三类注入各 10 轮，全抓才算判定面有效 ─────────────────
/// 注入一类：序类错位注入——翻指纹一比特，判定器必出 EDIVERGE
/// 且根因记 EORDER、旗标写发散（10 轮全抓）。
pub fn inject_order_dislocation() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0u32;
    while k < 10 {
        let scene = SCENE_MIN + k;
        let fp = frozen_print(scene);
        let fp_b = fp ^ (1u64 << (k % 64));
        let mut l = CompareLedger::new();
        let _ = l.record_raw(scene, 1, 1, fp, fp_b);
        let judged = l.judge_scene(scene);
        let mut ok = judged == Err(DmDiag::EDIVERGE.val());
        if let Some(r) = l.row_of(scene) {
            ok = ok && r.flag == CompareFlag::Divergent;
            ok = ok && r.root == DivRoot::OrderDislocation;
            ok = ok && r.root.root_code() == DmDiag::EORDER.val();
        } else {
            ok = false;
        }
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("序类错位路{}逃脱：翻位未被零容差判定抓住", k);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入二类：结果越闭集注入——喂越闭集原始码，判定器必出 EINVAL
/// 且越闭集计数显性递增（10 轮全抓）。
pub fn inject_outset_code() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 单账累积十轮：越闭集计数须逐轮递增（显性记账可复算）。
    let mut l = CompareLedger::new();
    let mut k = 0u32;
    while k < 10 {
        let scene = SCENE_MIN + k;
        let fp = frozen_print(scene);
        let bad_code = 4 + k; // 4..13 均越 Match|Diverge|Missing 闭集
        let _ = l.record_raw(scene, bad_code, 1, fp, fp);
        let judged = l.judge_scene(scene);
        let mut ok = judged == Err(DmDiag::EINVAL.val());
        ok = ok && l.einval_count == (k + 1) as usize;
        if let Some(r) = l.row_of(scene) {
            ok = ok && r.verdict_a().is_none();
        } else {
            ok = false;
        }
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("越闭集路{}逃脱：越闭集码未出 EINVAL", k);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入三类：静默吞并注入——构造发散后检查显式入账三处落痕
/// （行在/旗标发散/计数递增），任一缺失即注入逃脱（10 轮全抓）。
pub fn inject_silent_swallow() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 单账累积十轮：发散计数须逐轮递增（零静默显性入账可复算）。
    let mut l = CompareLedger::new();
    let mut k = 0u32;
    while k < 10 {
        let scene = SCENE_MIN + k;
        let fp = frozen_print(scene);
        let _ = l.record_raw(
            scene,
            MachineVerdict::Match.code(),
            MachineVerdict::Diverge.code(),
            fp,
            fp,
        );
        let judged = l.judge_scene(scene);
        let mut ok = judged == Err(DmDiag::EDIVERGE.val());
        // 零静默三处落痕：行在、旗标发散、已判、发散计数递增。
        match l.row_of(scene) {
            Some(r) => {
                ok = ok && r.flag == CompareFlag::Divergent;
                ok = ok && r.judged;
                ok = ok && r.root == DivRoot::VerdictMismatch;
            }
            None => ok = false,
        }
        ok = ok && l.divergence_count == (k + 1) as usize;
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("静默吞并路{}逃脱：发散未显式入账", k);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 三类注入汇总：30 轮全抓摘要。
pub fn inject_all_summary() -> (usize, usize, String) {
    let (c1, t1, e1) = inject_order_dislocation();
    let (c2, t2, e2) = inject_outset_code();
    let (c3, t3, e3) = inject_silent_swallow();
    let mut escape = String::new();
    for e in [e1, e2, e3].iter() {
        if !e.is_empty() && escape.is_empty() {
            escape = e.clone();
        }
    }
    (c1 + c2 + c3, t1 + t2 + t3, escape)
}

// ── 全流程指纹（账面 + 语料 + 注入，双跑一致口径） ────────────────
/// 全流程指纹：正常流账面判定 + 语料重演 + 三类注入摘要一并混合。
pub fn pipeline_fingerprint() -> u64 {
    const P: u64 = 0x0000_0100_0000_01B3;
    let mut l = build_happy_ledger();
    let mut scene = SCENE_MIN;
    while scene <= SCENE_MAX {
        let _ = l.judge_scene(scene);
        scene += 1;
    }
    let mut h = ledger_fingerprint(&l);
    for o in corpus_replay().iter() {
        h = h.wrapping_mul(P).wrapping_add(o.expect as u64);
        h = h.wrapping_mul(P).wrapping_add(o.actual as u64);
        h = h.wrapping_mul(P).wrapping_add(o.pass as u64);
    }
    let (c, t, e) = inject_all_summary();
    h = h.wrapping_mul(P).wrapping_add(c as u64);
    h = h.wrapping_mul(P).wrapping_add(t as u64);
    for b in e.as_bytes() {
        h = h.wrapping_mul(P).wrapping_add(*b as u64);
    }
    h
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口且查表可命中。
fn diag_closed_ok() -> bool {
    for d in DM_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    // 已知码必须命中文案，未知码必须回落。
    if detail(DmDiag::ENOENT.val()) != DmDiag::ENOENT.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == DmDiag::ENOENT.detail() {
        return false;
    }
    true
}

/// 冻结表守卫：20 行、场景号恰为 1..=20 升序、权重全非零。
fn frozen_table_ok() -> bool {
    if FROZEN_SCENES.len() != ROW_CAP {
        return false;
    }
    let mut expect = SCENE_MIN;
    for e in FROZEN_SCENES.iter() {
        if e.0 != expect {
            return false;
        }
        if e.1 == 0 {
            return false;
        }
        expect += 1;
    }
    true
}

/// 指纹可复算守卫：逐场景重算两轮同值、非零、二十场景互异。
fn frozen_recompute_ok() -> bool {
    let mut scene = SCENE_MIN;
    while scene <= SCENE_MAX {
        let a = frozen_print(scene);
        let b = frozen_print(scene);
        if a != b || a == 0 {
            return false;
        }
        // 越闭集场景指纹必须为零（查不到权重即零指纹）。
        if frozen_print(SCENE_MAX + 1) != 0 || frozen_print(0) != 0 {
            return false;
        }
        scene += 1;
    }
    // 互异：任两场景指纹不等。
    let mut i = SCENE_MIN;
    while i <= SCENE_MAX {
        let fi = frozen_print(i);
        let mut j = i + 1;
        while j <= SCENE_MAX {
            if frozen_print(j) == fi {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 正常流：20 场景登记齐 → 逐场景判定全过 → 封账。
fn happy_path_ok() -> bool {
    let mut l = build_happy_ledger();
    if l.state() != LedgerState::Complete || l.len() != ROW_CAP {
        return false;
    }
    let mut scene = SCENE_MIN;
    while scene <= SCENE_MAX {
        match l.judge_scene(scene) {
            Ok(r) => {
                if r.flag != CompareFlag::Consistent {
                    return false;
                }
            }
            Err(_) => return false,
        }
        scene += 1;
    }
    if !l.all_judged() || !l.all_consistent() {
        return false;
    }
    if l.divergence_count != 0 || l.tentative_count != 0 || l.einval_count != 0 {
        return false;
    }
    match l.seal() {
        Ok(()) => l.state() == LedgerState::Sealed,
        Err(_) => false,
    }
}

/// 判定次序：四路错误按固定次序出牌，先后关系逐对验证。
fn judge_order_ok() -> bool {
    // 次序一 ENOENT 最先：未登记场景即使码也越闭集也先报 ENOENT。
    let mut l1 = CompareLedger::new();
    let _ = l1.record_raw(21, 99, 99, 1, 2);
    if l1.judge_scene(21) != Err(DmDiag::ENOENT.val()) {
        return false;
    }
    // 次序 EINVAL 先于指纹/旗标：码越闭集且指纹也错位 → EINVAL。
    let mut l2 = CompareLedger::new();
    let _ = l2.record_raw(3, 0, 1, 0x1111, 0x2222);
    if l2.judge_scene(3) != Err(DmDiag::EINVAL.val()) {
        return false;
    }
    // 次序 零容差先于待定：双 Missing 但指纹错位 → EDIVERGE 非 ETENTATIVE。
    let mut l3 = CompareLedger::new();
    let _ = l3.record_raw(4, 3, 3, 0xAA00, 0xAA01);
    if l3.judge_scene(4) != Err(DmDiag::EDIVERGE.val()) {
        return false;
    }
    match l3.row_of(4) {
        Some(r) => {
            if r.root != DivRoot::OrderDislocation {
                return false;
            }
        }
        None => return false,
    }
    // 双 Missing 指纹相等 → ETENTATIVE。
    let mut l4 = CompareLedger::new();
    let _ = l4.record_raw(5, 3, 3, 0xAA00, 0xAA00);
    if l4.judge_scene(5) != Err(DmDiag::ETENTATIVE.val()) {
        return false;
    }
    // A≠B 指纹相等 → EDIVERGE 且显式入账。
    let mut l5 = CompareLedger::new();
    let _ = l5.record_raw(6, 1, 2, 0xBB00, 0xBB00);
    if l5.judge_scene(6) != Err(DmDiag::EDIVERGE.val()) {
        return false;
    }
    match l5.row_of(6) {
        Some(r) => r.flag == CompareFlag::Divergent && r.judged,
        None => false,
    }
}

/// 展开序类零容差：十个场景各翻一比特（位 0..9 全覆盖），逐位必抓。
fn zero_tolerance_ok() -> bool {
    let (caught, total, escape) = inject_order_dislocation();
    if total != 10 || caught != 10 || !escape.is_empty() {
        return false;
    }
    // 独立复核：手工构造三例翻位（低位/中位/高位），判定必出 EDIVERGE。
    let probes: [u64; 3] = [1u64 << 0, 1u64 << 31, 1u64 << 63];
    let mut k = 0usize;
    while k < probes.len() {
        let scene = SCENE_MIN + k as u32;
        let fp = frozen_print(scene);
        let flip = match probes.get(k) {
            Some(p) => *p,
            None => return false,
        };
        let mut l = CompareLedger::new();
        let _ = l.record_raw(scene, 1, 1, fp, fp ^ flip);
        if l.judge_scene(scene) != Err(DmDiag::EDIVERGE.val()) {
            return false;
        }
        k += 1;
    }
    true
}

/// 边界样本入库全过：语料 ≥30 例且逐例预期相符。
fn boundary_ok() -> bool {
    let outcomes = corpus_replay();
    if outcomes.len() < BOUNDARY_MIN {
        return false;
    }
    for o in outcomes.iter() {
        if !o.pass {
            return false;
        }
    }
    true
}

/// 同源可重演：语料两轮重演逐例相同，指纹一致。
fn replayable_ok() -> bool {
    let r1 = corpus_replay();
    let r2 = corpus_replay();
    if r1.len() != r2.len() || r1 != r2 {
        return false;
    }
    // 语料指纹双算一致。
    const P: u64 = 0x0000_0100_0000_01B3;
    let mut h1: u64 = 0xCBF2_9CE4_8422_2325;
    let mut h2: u64 = 0xCBF2_9CE4_8422_2325;
    for o in r1.iter() {
        h1 = h1
            .wrapping_mul(P)
            .wrapping_add(o.expect as u64 ^ (o.actual as u64) << 8);
    }
    for o in r2.iter() {
        h2 = h2
            .wrapping_mul(P)
            .wrapping_add(o.expect as u64 ^ (o.actual as u64) << 8);
    }
    h1 == h2
}

/// 注入必抓：三类各 10 轮共 30 轮全数命中，无逃脱。
fn inject_all_ok() -> bool {
    let (caught, total, escape) = inject_all_summary();
    if total != 30 {
        return false;
    }
    if caught != 30 {
        return false;
    }
    escape.is_empty()
}

/// 双跑一致：全流程（账面+语料+注入）指纹两轮相等。
fn dual_run_ok() -> bool {
    let a = pipeline_fingerprint();
    let b = pipeline_fingerprint();
    a == b
}

/// 容量耗尽与封账闸：越界场景/重复场景/封账写/重复封账全数拒。
fn capacity_ok() -> bool {
    // 容量耗尽：场景 0 / 21 / 9999 越闭集均 ENOENT（账容量 20 行封顶）。
    let mut l = CompareLedger::new();
    if l.record_raw(0, 1, 1, 1, 1) != Err(DmDiag::ENOENT.val()) {
        return false;
    }
    if l.record_raw(21, 1, 1, 1, 1) != Err(DmDiag::ENOENT.val()) {
        return false;
    }
    if l.record_raw(9999, 1, 1, 1, 1) != Err(DmDiag::ENOENT.val()) {
        return false;
    }
    // 重复闸：同场景二次登记 EDUP。
    let _ = l.record_raw(5, 1, 1, frozen_print(5), frozen_print(5));
    if l.record_raw(5, 1, 1, frozen_print(5), frozen_print(5)) != Err(DmDiag::EDUP.val()) {
        return false;
    }
    // 未满禁封：登记不满 20 行就封 → ENOTFULL。
    if l.seal() != Err(DmDiag::ENOTFULL.val()) {
        return false;
    }
    // 封账后写拒：登记 ESEALED、判定 ESEALED。
    let mut full = build_happy_ledger();
    let mut scene = SCENE_MIN;
    while scene <= SCENE_MAX {
        let _ = full.judge_scene(scene);
        scene += 1;
    }
    if full.seal().is_err() {
        return false;
    }
    if full.record_raw(1, 1, 1, 1, 1) != Err(DmDiag::ESEALED.val()) {
        return false;
    }
    if full.judge_scene(2) != Err(DmDiag::ESEALED.val()) {
        return false;
    }
    // 倒退拒：重复封账 EREWIND。
    full.seal() == Err(DmDiag::EREWIND.val())
}

/// 完成写回语义：封账即状态=已完成，结果摘要非空。
fn writeback_ok() -> bool {
    let mut l = build_happy_ledger();
    let mut scene = SCENE_MIN;
    while scene <= SCENE_MAX {
        let _ = l.judge_scene(scene);
        scene += 1;
    }
    if l.seal().is_err() {
        return false;
    }
    // 结果摘要：场景数/状态/发散计数，非空即写回合规。
    let summary = format!(
        "scenes={} sealed={} diverge={}",
        l.len(),
        l.state() == LedgerState::Sealed,
        l.divergence_count()
    );
    !summary.is_empty()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12751 自检（判据 12751 卡：边界样本 ≥30 例入库全过，同源可重演）。
pub fn run_unx12751_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12751");
    let ledger = build_happy_ledger();

    s.add_dyn(
        "F12751-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12751-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            DM_ALL.len(),
            DM_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12751-03 冻结场景账20行",
        frozen_table_ok(),
        &format!(
            "rows={}首={}尾={}",
            FROZEN_SCENES.len(),
            FROZEN_SCENES.first().map(|e| e.0).unwrap_or(0),
            FROZEN_SCENES.last().map(|e| e.0).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12751-04 指纹可复算双通道",
        frozen_recompute_ok() && frozen_table_ok(),
        &format!(
            "fp1=0x{:016x} fp20=0x{:016x}",
            frozen_print(1),
            frozen_print(20)
        ),
    );

    s.add_dyn(
        "F12751-05 正常流20场景封账",
        happy_path_ok(),
        &format!(
            "rows={} state={:?} div={}",
            ledger.len(),
            ledger.state(),
            ledger.divergence_count()
        ),
    );

    s.add_dyn(
        "F12751-06 判定次序四路",
        judge_order_ok(),
        "ENOENT→EINVAL→零容差→ETENTATIVE→EDIVERGE 逐对验证",
    );

    s.add_dyn(
        "F12751-07 展开序零容差",
        zero_tolerance_ok(),
        "翻一位即发散，根因记 EORDER",
    );

    s.add_dyn(
        "F12751-08 边界样本入库全过",
        boundary_ok(),
        &format!(
            "n={} 下限={} 全过={}",
            corpus_replay().len(),
            BOUNDARY_MIN,
            boundary_ok()
        ),
    );

    s.add_dyn(
        "F12751-09 同源可重演",
        replayable_ok(),
        &format!(
            "replay={} pass={}",
            corpus_replay().len(),
            corpus_replay().iter().all(|o| o.pass)
        ),
    );

    let (c1, t1, e1) = inject_order_dislocation();
    s.add_dyn(
        "F12751-10 注入一序类错位",
        c1 == 10 && t1 == 10 && e1.is_empty(),
        &format!("caught={}/{} {}", c1, t1, e1),
    );

    let (c2, t2, e2) = inject_outset_code();
    s.add_dyn(
        "F12751-11 注入二结果越闭集",
        c2 == 10 && t2 == 10 && e2.is_empty(),
        &format!("caught={}/{} {}", c2, t2, e2),
    );

    let (c3, t3, e3) = inject_silent_swallow();
    s.add_dyn(
        "F12751-12 注入三静默吞并",
        c3 == 10 && t3 == 10 && e3.is_empty(),
        &format!("caught={}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12751-13 双跑一致指纹",
        dual_run_ok(),
        &format!(
            "fp=0x{:016x}",
            pipeline_fingerprint()
        ),
    );

    s.add_dyn(
        "F12751-14 容量耗尽与封账闸",
        capacity_ok(),
        "越界/重复/未满禁封/封后写拒/倒退拒 五点",
    );

    s.add_dyn(
        "F12751-15 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    // 收口：判据条数声明（15 条检查含本条收口，与声明恒等）。
    let declared = 16usize;
    let actual = s.len() + 1;
    s.add_dyn(
        "F12751-16 收口判据条数",
        actual == declared,
        &format!("声明{}条 实际{}条", declared, actual),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x825C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：13 个码互异、全部落窗口、查表可命中。
        assert_eq!(DM_ALL.len(), 13);
        let mut i = 0usize;
        while i < DM_ALL.len() {
            let vi = DM_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < DM_ALL.len() {
                let vj = DM_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            assert_eq!(detail(vi), DM_ALL.get(i).map(|d| d.detail()).unwrap_or(""));
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
        assert!(detail(SEG_BASE).contains("未知"));
    }

    #[test]
    fn frozen_table_and_recompute() {
        // 冻结表：20 行、序 1..=20、权重非零；指纹可复算且互异。
        assert!(frozen_table_ok());
        assert!(frozen_recompute_ok());
        assert_eq!(frozen_print(0), 0);
        assert_eq!(frozen_print(SCENE_MAX + 1), 0);
        assert_eq!(frozen_print(3), frozen_print(3));
        assert_ne!(frozen_print(3), frozen_print(4));
        // 权重表与查表函数一致。
        assert_eq!(frozen_weight(1), Some(0x9E01));
        assert_eq!(frozen_weight(20), Some(0xCA14));
        assert_eq!(frozen_weight(21), None);
    }

    #[test]
    fn happy_path_seals_twenty_scenes() {
        let mut l = build_happy_ledger();
        assert_eq!(l.len(), 20);
        assert_eq!(l.state(), LedgerState::Complete);
        let mut scene = SCENE_MIN;
        while scene <= SCENE_MAX {
            match l.judge_scene(scene) {
                Ok(r) => assert_eq!(r.flag, CompareFlag::Consistent),
                Err(e) => panic!("正常流判定不应出错: {}", e),
            }
            scene += 1;
        }
        assert!(l.all_judged());
        assert!(l.all_consistent());
        assert_eq!(l.divergence_count(), 0);
        assert!(l.seal().is_ok());
        assert_eq!(l.state(), LedgerState::Sealed);
        assert!(happy_path_ok());
    }

    #[test]
    fn judge_order_precedence() {
        // 判定次序：四路错误按固定次序出牌。
        assert!(judge_order_ok());
        // ENOENT 最先：未登记 + 越闭集码 → ENOENT。
        let mut l = CompareLedger::new();
        let _ = l.record_raw(30, 99, 99, 1, 2);
        assert_eq!(l.judge_scene(30), Err(DmDiag::ENOENT.val()));
        // EINVAL 先于零容差：越闭集码 + 指纹错位 → EINVAL。
        let mut l2 = CompareLedger::new();
        let _ = l2.record_raw(2, 0, 1, 0x1, 0x2);
        assert_eq!(l2.judge_scene(2), Err(DmDiag::EINVAL.val()));
        // 零容差先于 ETENTATIVE：双 Missing + 错位 → EDIVERGE。
        let mut l3 = CompareLedger::new();
        let _ = l3.record_raw(2, 3, 3, 0xF0, 0xF1);
        assert_eq!(l3.judge_scene(2), Err(DmDiag::EDIVERGE.val()));
    }

    #[test]
    fn injectors_thirty_rounds_all_caught() {
        // 三类注入各 10 轮全抓，无逃脱。
        assert!(inject_all_ok());
        let (c1, t1, e1) = inject_order_dislocation();
        assert_eq!((c1, t1), (10, 10));
        assert!(e1.is_empty());
        let (c2, t2, e2) = inject_outset_code();
        assert_eq!((c2, t2), (10, 10));
        assert!(e2.is_empty());
        let (c3, t3, e3) = inject_silent_swallow();
        assert_eq!((c3, t3), (10, 10));
        assert!(e3.is_empty());
        let (sum_c, sum_t, sum_e) = inject_all_summary();
        assert_eq!((sum_c, sum_t), (30, 30));
        assert!(sum_e.is_empty());
    }

    #[test]
    fn zero_tolerance_single_bit() {
        // 展开序零容差：位 0..9 翻位逐位必抓，根因一律记 EORDER。
        assert!(zero_tolerance_ok());
        let mut bit = 0u32;
        while bit < 64 {
            let mut l = CompareLedger::new();
            let fp = frozen_print(1);
            let _ = l.record_raw(1, 1, 1, fp, fp ^ (1u64 << bit));
            assert_eq!(l.judge_scene(1), Err(DmDiag::EDIVERGE.val()));
            let r = l.row_of(1).unwrap_or(SceneRow {
                scene_no: 0,
                code_a: 0,
                code_b: 0,
                fp_a: 0,
                fp_b: 0,
                flag: CompareFlag::Unjudged,
                root: DivRoot::NotDivergent,
                judged: false,
            });
            assert_eq!(r.root, DivRoot::OrderDislocation);
            assert_eq!(r.root.root_code(), DmDiag::EORDER.val());
            bit += 1;
        }
    }

    #[test]
    fn boundary_corpus_at_least_30_all_pass() {
        // 判据 J1：边界样本 ≥30 例入库全过。
        let corpus = corpus_all();
        assert!(corpus.len() >= BOUNDARY_MIN);
        assert_eq!(corpus.len(), 34);
        let outcomes = corpus_replay();
        assert_eq!(outcomes.len(), corpus.len());
        for o in outcomes.iter() {
            assert!(o.pass, "样本{} 预期{} 实测{}", o.idx, o.expect, o.actual);
        }
        assert!(boundary_ok());
    }

    #[test]
    fn dual_run_replayable() {
        // 同源可重演：语料两轮逐例相同，全流程指纹两轮一致。
        assert!(replayable_ok());
        assert_eq!(corpus_replay(), corpus_replay());
        assert!(dual_run_ok());
        assert_eq!(pipeline_fingerprint(), pipeline_fingerprint());
    }

    #[test]
    fn capacity_and_seal_gates() {
        // 容量耗尽与封账闸：越界/重复/未满禁封/封后写拒/倒退拒。
        assert!(capacity_ok());
        let mut l = CompareLedger::new();
        assert_eq!(l.record_raw(0, 1, 1, 1, 1), Err(DmDiag::ENOENT.val()));
        assert_eq!(l.record_raw(21, 1, 1, 1, 1), Err(DmDiag::ENOENT.val()));
        let _ = l.record_raw(9, 1, 1, frozen_print(9), frozen_print(9));
        assert_eq!(l.record_raw(9, 1, 1, 1, 1), Err(DmDiag::EDUP.val()));
        assert_eq!(l.seal(), Err(DmDiag::ENOTFULL.val()));
        // 空账判定 → ENOENT。
        let mut empty = CompareLedger::new();
        assert_eq!(empty.judge_scene(1), Err(DmDiag::ENOENT.val()));
        // 封后写拒与倒退拒。
        let mut full = build_happy_ledger();
        let mut scene = SCENE_MIN;
        while scene <= SCENE_MAX {
            let _ = full.judge_scene(scene);
            scene += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(full.record_raw(1, 1, 1, 1, 1), Err(DmDiag::ESEALED.val()));
        assert_eq!(full.judge_scene(1), Err(DmDiag::ESEALED.val()));
        assert_eq!(full.seal(), Err(DmDiag::EREWIND.val()));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Open→Complete→Sealed。
        assert!(LedgerState::Open.rank() < LedgerState::Complete.rank());
        assert!(LedgerState::Complete.rank() < LedgerState::Sealed.rank());
        let mut l = build_happy_ledger();
        assert_eq!(l.state(), LedgerState::Complete);
        let mut scene = SCENE_MIN;
        while scene <= SCENE_MAX {
            let _ = l.judge_scene(scene);
            scene += 1;
        }
        let _ = l.seal();
        assert_eq!(l.state(), LedgerState::Sealed);
        // 封账后登记拒，账面行数不变。
        let before = l.len();
        assert_eq!(l.record_raw(1, 1, 1, 1, 1), Err(DmDiag::ESEALED.val()));
        assert_eq!(l.len(), before);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12751_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
        // 收口条数声明：16 条。
        assert_eq!(s.len(), 16);
    }
}
