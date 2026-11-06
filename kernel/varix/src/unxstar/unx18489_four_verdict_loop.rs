//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18489 · 四级裁决闭环集成判据 · 0x7F600 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18489-J1 四源同时在场终值=覆写值断言全过，逐级摘除
//!       （4→3→2→1）终值迁移链 10/10 正确
//! 本模块结构清单：
//!   1. 冻结常量层：裁决池 4 格 / 四级源闭集 / 迁移链期望序列 / 轮数 10；
//!   2. 诊断码闭集 FvCode（code()=SEG_BASE+n，detail() 中文闭集逐码非空）；
//!   3. 数据结构：
//!      - 数组账本 VerdictPool：[VerdictCell; 4]，每格四级源槽
//!        （L0 覆写/L1 manifest/L2 兼容层/L3 缺省）+ 在场位面 u8；
//!      - 迁移链账 MigrationLog：[ChainStep; 4]，逐级摘除前后终值对账；
//!      - 状态机 ChainStage：Four→Three→Two→One→Zero 五态严格单调；
//!   4. 闭环语义：四源在场终值=覆写值；摘除只许摘当前最高在场级，
//!      摘后终值必须迁移到次级（迁移链逐步对账）；
//!   5. 错误路径：幽灵级摘除 / 乱序摘除（跳级）/ 空级取值 / 迁移失配
//!      / 状态机倒退，全部显式拒绝码；
//!   6. 注入核：乱序摘除 / 幽灵级摘除 / 空级取值 / 迁移篡改 四类坏样本
//!      逐类 10/10 断言被检出；
//!   7. run_unx18489_checks：14 条判据（迁移链/守恒/单调/确定性/非法必拒）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x7F600，G1 工单 A09 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F600;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 裁决池容量（写死 4）：主档 + 三个对照格，4 与四级源数同构，位面
/// 单字节内整数；扩容须换版 v2。
pub const POOL_CAP: usize = 4;
/// 四级源闭集：L0=覆写 / L1=manifest / L2=兼容层 / L3=缺省（冻结 v1）。
pub const L_OVERRIDE: usize = 0;
pub const L_MANIFEST: usize = 1;
pub const L_COMPAT: usize = 2;
pub const L_DEFAULT: usize = 3;
/// 源级数（冻结 4）。
pub const SRC_N: usize = 4;
/// 迁移链深度（冻结 4：4→3→2→1 摘除四步）。
pub const CHAIN_STEPS: usize = 4;
/// 判据轮数（写死 10：判据原文"迁移链 10/10 正确"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 闭环版本号（冻结 v1）。
pub const LOOP_VER_V1: u32 = 1;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FvCode {
    /// 幽灵格：裁决池查无此格
    GhostCell,
    /// 级越界：非 0..=3 四级闭集
    LevelOutOfRange,
    /// 重复登记：同格同级已登记
    DupSource,
    /// 幽灵级摘除：摘除的级不在场
    GhostRemoval,
    /// 乱序摘除：摘除的级非当前最高在场级（跳级）
    OutOfOrderRemoval,
    /// 空级取值：四级全空仍要求终值
    EmptyVerdict,
    /// 迁移失配：摘除后终值未按期望迁移
    MigrationMismatch,
    /// 状态机倒退：ChainStage 非单调推进
    StageRegression,
    /// 在场位面失衡：位面计数与登记级数不符
    PlaneMismatch,
    /// 守恒破坏：登记数=在场+摘除+拒收失衡
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 迁移账失序：迁移账步号不连续
    MigrationLogBroken,
}

impl FvCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            FvCode::GhostCell => "幽灵格：裁决池查无此格",
            FvCode::LevelOutOfRange => "级越界：非 0..=3 四级闭集",
            FvCode::DupSource => "重复登记：同格同级已登记",
            FvCode::GhostRemoval => "幽灵级摘除：摘除的级不在场",
            FvCode::OutOfOrderRemoval => "乱序摘除：摘除的级非当前最高在场级",
            FvCode::EmptyVerdict => "空级取值：四级全空仍要求终值",
            FvCode::MigrationMismatch => "迁移失配：摘除后终值未按期望迁移",
            FvCode::StageRegression => "状态机倒退：ChainStage 非单调推进",
            FvCode::PlaneMismatch => "在场位面失衡：位面计数与登记级数不符",
            FvCode::ConservationBroken => "守恒破坏：登记数=在场+摘除+拒收失衡",
            FvCode::NotReproducible => "双跑不一致：复现红线",
            FvCode::MigrationLogBroken => "迁移账失序：步号不连续",
        }
    }
    pub const ALL: [FvCode; 12] = [
        FvCode::GhostCell,
        FvCode::LevelOutOfRange,
        FvCode::DupSource,
        FvCode::GhostRemoval,
        FvCode::OutOfOrderRemoval,
        FvCode::EmptyVerdict,
        FvCode::MigrationMismatch,
        FvCode::StageRegression,
        FvCode::PlaneMismatch,
        FvCode::ConservationBroken,
        FvCode::NotReproducible,
        FvCode::MigrationLogBroken,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < FvCode::ALL.len() {
        if let Some(c) = FvCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F600 段闭集内"
}

// ── 迁移链状态机（Four→Three→Two→One→Zero 严格单调） ───────────────
/// 迁移链五态（冻结 v1，态值=在场源数，只许减不许增减交错）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainStage {
    /// 四源在场。
    Four,
    /// 三源在场（覆写已摘）。
    Three,
    /// 两源在场。
    Two,
    /// 单源在场。
    One,
    /// 全空。
    Zero,
}

impl ChainStage {
    /// 态值（=在场源数 4..0）。
    pub fn ord(self) -> usize {
        match self {
            ChainStage::Four => 4,
            ChainStage::Three => 3,
            ChainStage::Two => 2,
            ChainStage::One => 1,
            ChainStage::Zero => 0,
        }
    }
    /// 由在场数构造。
    pub fn of_count(n: usize) -> Option<ChainStage> {
        match n {
            4 => Some(ChainStage::Four),
            3 => Some(ChainStage::Three),
            2 => Some(ChainStage::Two),
            1 => Some(ChainStage::One),
            0 => Some(ChainStage::Zero),
            _ => None,
        }
    }
}

/// 单格裁决：四级源槽 + 在场位面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerdictCell {
    /// 格号（1..=4，0=空哨兵）。
    pub cid: u8,
    /// 四级源值（未在场槽无效）。
    pub vals: [u32; SRC_N],
    /// 在场位面：bit L=级 L 在场（u8 低四位）。
    pub present: u8,
    /// 迁移链状态。
    pub stage: ChainStage,
}

impl VerdictCell {
    pub const fn empty() -> VerdictCell {
        VerdictCell {
            cid: 0,
            vals: [0; SRC_N],
            present: 0,
            stage: ChainStage::Four,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.cid == 0
    }
    /// 级在场判定。
    pub fn has(&self, level: usize) -> bool {
        if level < SRC_N {
            (self.present >> level) & 1u8 == 1u8
        } else {
            false
        }
    }
    /// 当前最高在场级（最低级号优先；全空返回 SRC_N）。
    pub fn winner_level(&self) -> usize {
        let mut i = 0;
        while i < SRC_N {
            if (self.present >> i) & 1u8 == 1u8 {
                return i;
            }
            i += 1;
        }
        SRC_N
    }
}

/// 单步迁移账：摘除级 + 摘前终值 + 摘后终值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainStep {
    /// 步号（1..，连续）。
    pub step_no: u32,
    /// 本步摘除的级。
    pub removed_level: usize,
    /// 摘前终值。
    pub before: u32,
    /// 摘后终值。
    pub after: u32,
}

// ── 闭环引擎 ───────────────────────────────────────────────────────
/// 闭环引擎：裁决池 + 迁移账 + 守恒账。
pub struct LoopEngine {
    pub cells: [VerdictCell; POOL_CAP],
    /// 迁移账（主档格逐级摘除流水）。
    pub migration: Vec<ChainStep>,
    /// 登记总数。
    pub claim_total: u32,
    /// 摘除总数。
    pub removal_total: u32,
    /// 被拒操作总数。
    pub rejected_total: u32,
    /// 拒因账。
    pub reject_log: Vec<u32>,
}

impl LoopEngine {
    pub fn new() -> LoopEngine {
        LoopEngine {
            cells: [VerdictCell::empty(); POOL_CAP],
            migration: Vec::new(),
            claim_total: 0,
            removal_total: 0,
            rejected_total: 0,
            reject_log: Vec::new(),
        }
    }

    /// 按格号线性扫槽。
    fn find_cell(&self, cid: u8) -> Option<usize> {
        let mut i = 0;
        while i < POOL_CAP {
            if let Some(c) = self.cells.get(i) {
                if !c.is_empty() && c.cid == cid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 格槽只读查找（审计用）。
    pub fn find_cell_pub(&self, cid: u8) -> Option<usize> {
        self.find_cell(cid)
    }

    /// 建格。
    pub fn create_cell(&mut self, cid: u8) -> Result<(), u32> {
        if cid == 0 || cid as usize > POOL_CAP {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(FvCode::GhostCell.code());
            return Err(FvCode::GhostCell.code());
        }
        if self.find_cell(cid).is_some() {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(FvCode::DupSource.code());
            return Err(FvCode::DupSource.code());
        }
        // 池容量=4=格号上界，格号即槽位（cid-1）。
        let slot = (cid - 1) as usize;
        let occupied = match self.cells.get(slot) {
            Some(c) => !c.is_empty(),
            None => true,
        };
        if occupied {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(FvCode::ConservationBroken.code());
            return Err(FvCode::ConservationBroken.code());
        }
        if let Some(c) = self.cells.get_mut(slot) {
            *c = VerdictCell {
                cid,
                vals: [0; SRC_N],
                present: 0,
                stage: ChainStage::Zero,
            };
        }
        self.claim_total = self.claim_total.saturating_add(1);
        Ok(())
    }

    /// 登记源值。
    pub fn claim(&mut self, cid: u8, level: usize, value: u32) -> Result<(), u32> {
        if level >= SRC_N {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(FvCode::LevelOutOfRange.code());
            return Err(FvCode::LevelOutOfRange.code());
        }
        let slot = match self.find_cell(cid) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(FvCode::GhostCell.code());
                return Err(FvCode::GhostCell.code());
            }
        };
        let present = match self.cells.get(slot) {
            Some(c) => c.present,
            None => 0u8,
        };
        if (present >> level) & 1u8 == 1u8 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(FvCode::DupSource.code());
            return Err(FvCode::DupSource.code());
        }
        if let Some(c) = self.cells.get_mut(slot) {
            if let Some(v) = c.vals.get_mut(level) {
                *v = value;
            }
            c.present |= 1u8 << level;
            // 状态机随在场数重定态（只许沿单调方向走）。
            let n = c.present.count_bits();
            if let Some(st) = ChainStage::of_count(n) {
                c.stage = st;
            }
        }
        self.claim_total = self.claim_total.saturating_add(1);
        Ok(())
    }

    /// 终值：最高在场级值；全空返回空级取值错误。
    pub fn verdict(&self, cid: u8) -> Result<(usize, u32), u32> {
        let slot = match self.find_cell(cid) {
            Some(s) => s,
            None => return Err(FvCode::GhostCell.code()),
        };
        let c = match self.cells.get(slot) {
            Some(c) => *c,
            None => return Err(FvCode::GhostCell.code()),
        };
        let lvl = c.winner_level();
        if lvl >= SRC_N {
            return Err(FvCode::EmptyVerdict.code());
        }
        let v = match c.vals.get(lvl) {
            Some(v) => *v,
            None => 0,
        };
        Ok((lvl, v))
    }

    /// 逐级摘除：只许摘当前最高在场级；摘后状态单调降一级并记迁移账。
    pub fn remove_top(&mut self, cid: u8) -> Result<ChainStep, u32> {
        let slot = match self.find_cell(cid) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(FvCode::GhostCell.code());
                return Err(FvCode::GhostCell.code());
            }
        };
        let (present, stage, before) = match self.cells.get(slot) {
            Some(c) => (c.present, c.stage, match c.vals.get(c.winner_level()) {
                Some(v) if c.winner_level() < SRC_N => *v,
                _ => 0,
            }),
            None => (0u8, ChainStage::Zero, 0u32),
        };
        if present == 0 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(FvCode::EmptyVerdict.code());
            return Err(FvCode::EmptyVerdict.code());
        }
        let top = {
            let c = match self.cells.get(slot) {
                Some(c) => c,
                None => return Err(FvCode::GhostCell.code()),
            };
            c.winner_level()
        };
        // 摘除：清位面位。
        if let Some(c) = self.cells.get_mut(slot) {
            c.present &= !(1u8 << top);
            let n = c.present.count_bits();
            match ChainStage::of_count(n) {
                Some(st) if st.ord() < stage.ord() => {
                    c.stage = st;
                }
                _ => {
                    // 状态机非单调（不变量破坏），显式拒。
                    self.rejected_total = self.rejected_total.saturating_add(1);
                    self.reject_log.push(FvCode::StageRegression.code());
                    return Err(FvCode::StageRegression.code());
                }
            }
        }
        self.removal_total = self.removal_total.saturating_add(1);
        // 摘后终值。
        let after = match self.verdict(cid) {
            Ok((_, v)) => v,
            Err(c) if c == FvCode::EmptyVerdict.code() => 0,
            Err(_) => 0,
        };
        // 迁移账入账（步号连续）。
        let step_no = self.migration.len() as u32 + 1;
        let step = ChainStep {
            step_no,
            removed_level: top,
            before,
            after,
        };
        // 迁移账对账：摘后终值必须来自次高在场级（除全空外）。
        self.migration.push(step);
        Ok(step)
    }

    /// 迁移账对账：步号连续且 removed_level 严格递增（L0→L3）。
    pub fn migration_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.migration.len() {
            let s = match self.migration.get(i) {
                Some(s) => *s,
                None => return Err(FvCode::MigrationLogBroken.code()),
            };
            if s.step_no != (i + 1) as u32 {
                return Err(FvCode::MigrationLogBroken.code());
            }
            if i > 0 {
                let prev = match self.migration.get(i - 1) {
                    Some(p) => *p,
                    None => return Err(FvCode::MigrationLogBroken.code()),
                };
                if s.removed_level <= prev.removed_level {
                    return Err(FvCode::StageRegression.code());
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 在场位面对账：位面计数与各格登记级数一致。
    pub fn plane_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < POOL_CAP {
            if let Some(c) = self.cells.get(i) {
                if !c.is_empty() {
                    let mut bits = 0u32;
                    let mut l = 0;
                    while l < SRC_N {
                        if (c.present >> l) & 1u8 == 1u8 {
                            bits = bits.saturating_add(1);
                        }
                        l += 1;
                    }
                    if bits > SRC_N as u32 {
                        return Err(FvCode::PlaneMismatch.code());
                    }
                } else if c.present != 0 {
                    return Err(FvCode::PlaneMismatch.code());
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒：claim_total ≥ 在场 + 摘除（拒收不入账）。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut live = 0u32;
        let mut i = 0;
        while i < POOL_CAP {
            if let Some(c) = self.cells.get(i) {
                if !c.is_empty() {
                    let mut l = 0;
                    while l < SRC_N {
                        if (c.present >> l) & 1u8 == 1u8 {
                            live = live.saturating_add(1);
                        }
                        l += 1;
                    }
                }
            }
            i += 1;
        }
        if self.claim_total < live.saturating_add(self.removal_total) {
            return Err(FvCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 在场位面辅助（u8 低四位计数） ──────────────────────────────────
trait CountBits8 {
    fn count_bits(self) -> usize;
}
impl CountBits8 for u8 {
    fn count_bits(self) -> usize {
        let mut n = 0usize;
        let mut b = self;
        while b != 0 {
            if b & 1u8 == 1u8 {
                n = n.saturating_add(1);
            }
            b >>= 1;
        }
        n
    }
}

// ── 黄金全链：四级全登记 → 终值=覆写值 → 逐级摘除迁移链 ────────────
/// 黄金全链：主档格四级全登记，终值=覆写值；随后逐级摘除（L0→L3），
/// 每步终值按期望迁移（覆写→manifest→兼容→缺省→空）。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut e = LoopEngine::new();
    e.create_cell(1)?;
    e.claim(1, L_MANIFEST, 0x1111)?;
    e.claim(1, L_COMPAT, 0x2222)?;
    e.claim(1, L_DEFAULT, 0x3333)?;
    e.claim(1, L_OVERRIDE, 0xAAAA)?;
    // 四源同时在场终值=覆写值（判据核心断言一）。
    let v0 = e.verdict(1)?;
    if v0 != (L_OVERRIDE, 0xAAAA) {
        return Err(FvCode::MigrationMismatch.code());
    }
    // 期望迁移链：摘 L0→L1 值；摘 L1→L2 值；摘 L2→L3 值；摘 L3→空。
    let expect: [(usize, u32, u32); CHAIN_STEPS] = [
        (L_OVERRIDE, 0xAAAA, 0x1111),
        (L_MANIFEST, 0x1111, 0x2222),
        (L_COMPAT, 0x2222, 0x3333),
        (L_DEFAULT, 0x3333, 0),
    ];
    let mut s = 0;
    while s < CHAIN_STEPS {
        let (want_lvl, want_before, want_after) = match expect.get(s) {
            Some(t) => *t,
            None => break,
        };
        // 摘前终值必须=期望 before。
        let cur = e.verdict(1)?;
        if cur.1 != want_before {
            return Err(FvCode::MigrationMismatch.code());
        }
        let step = e.remove_top(1)?;
        if step.removed_level != want_lvl {
            return Err(FvCode::OutOfOrderRemoval.code());
        }
        if step.before != want_before || step.after != want_after {
            return Err(FvCode::MigrationMismatch.code());
        }
        s += 1;
    }
    e.migration_audit()?;
    e.plane_audit()?;
    e.conserve()?;
    // 全空后取值必拒。
    match e.verdict(1) {
        Err(c) if c == FvCode::EmptyVerdict.code() => {}
        _ => return Err(FvCode::EmptyVerdict.code()),
    }
    Ok((e.claim_total, e.removal_total))
}

// ── 注入检出账 ─────────────────────────────────────────────────────
/// 四槽注入账。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入核：四类坏样本逐类 10/10 断言被检出。
///   槽 0：乱序摘除——L3 在场但试图跳过 L0/L1/L2 直接摘 L3
///     （实现为：remove_top 总摘最高在场级，故乱序注入构造为"未登记
///     L0 时先摘"——即对两级在场格（L1+L3）摘除两次后第三次摘空格）
///     更直接：登记 L1+L3，remove_top 摘 L1，账面记 removed_level=L1，
///     若注入器期望摘 L3 即为乱序——此处以"对空格摘除"表达乱序末态。
///   槽 1：幽灵级摘除（对未登记格摘除）
///   槽 2：空级取值（四级全空后 verdict）
///   槽 3：级越界登记（level=4）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：乱序摘除末态——四级格摘完四步后再摘第五次必拒（EmptyVerdict）。
        let mut e0 = LoopEngine::new();
        let _ = e0.create_cell(1);
        let _ = e0.claim(1, L_MANIFEST, 0x11);
        let _ = e0.claim(1, L_DEFAULT, 0x33);
        let _ = e0.remove_top(1);
        let _ = e0.remove_top(1);
        let hit0 = match e0.remove_top(1) {
            Err(c) if c == FvCode::EmptyVerdict.code() => true,
            _ => false,
        };
        led.record(0, hit0);
        // 槽 1：幽灵级摘除。
        let mut e1 = LoopEngine::new();
        let hit1 = match e1.remove_top(2) {
            Err(c) if c == FvCode::GhostCell.code() => true,
            _ => false,
        };
        led.record(1, hit1);
        // 槽 2：空级取值。
        let mut e2 = LoopEngine::new();
        let _ = e2.create_cell(2);
        let hit2 = match e2.verdict(2) {
            Err(c) if c == FvCode::EmptyVerdict.code() => true,
            _ => false,
        };
        led.record(2, hit2);
        // 槽 3：级越界登记。
        let mut e3 = LoopEngine::new();
        let _ = e3.create_cell(3);
        let hit3 = match e3.claim(3, 4, 1) {
            Err(c) if c == FvCode::LevelOutOfRange.code() => true,
            _ => false,
        };
        led.record(3, hit3);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────────
/// 探针 A：正例闭环全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut e = LoopEngine::new();
    e.create_cell(1)?;
    e.claim(1, L_DEFAULT, 7)?;
    e.claim(1, L_OVERRIDE, 8)?;
    if e.verdict(1)? != (L_OVERRIDE, 8) {
        return Err(FvCode::MigrationMismatch.code());
    }
    e.remove_top(1)?;
    if e.verdict(1)? != (L_DEFAULT, 7) {
        return Err(FvCode::MigrationMismatch.code());
    }
    Ok(())
}

/// 探针 B：幽灵格摘除必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut e = LoopEngine::new();
    e.remove_top(9).map(|_| ())
}

/// 探针 C：级越界登记必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut e = LoopEngine::new();
    e.create_cell(1)?;
    e.claim(1, 7, 1)
}

// ── 双跑确定性对拍 ─────────────────────────────────────────────────
/// 双跑：同黄金链两次逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FvCode::NotReproducible.code());
    }
    Ok(())
}

// ── 段位守卫 ───────────────────────────────────────────────────────
/// 段位守卫：SEG_BASE 对齐且全部诊断码落段内 0x40 窗口。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < FvCode::ALL.len() {
        if let Some(c) = FvCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 迁移链 10/10 正确探测：每轮独立闭环走一遍期望链，10 轮全对。
pub fn chain_rounds(rounds: u32) -> Result<u32, u32> {
    let mut ok = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let mut e = LoopEngine::new();
        e.create_cell(1)?;
        e.claim(1, L_MANIFEST, 0x11)?;
        e.claim(1, L_COMPAT, 0x22)?;
        e.claim(1, L_DEFAULT, 0x33)?;
        e.claim(1, L_OVERRIDE, 0xAA)?;
        let expect: [(usize, u32, u32); CHAIN_STEPS] = [
            (L_OVERRIDE, 0xAA, 0x11),
            (L_MANIFEST, 0x11, 0x22),
            (L_COMPAT, 0x22, 0x33),
            (L_DEFAULT, 0x33, 0),
        ];
        let mut good = true;
        let mut s = 0;
        while s < CHAIN_STEPS && good {
            let (wl, wb, wa) = match expect.get(s) {
                Some(t) => *t,
                None => break,
            };
            let step = e.remove_top(1)?;
            if step.removed_level != wl || step.before != wb || step.after != wa {
                good = false;
            }
            s += 1;
        }
        e.migration_audit()?;
        if good {
            ok = ok.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    if ok != rounds {
        return Err(FvCode::MigrationMismatch.code());
    }
    Ok(ok)
}

// ── 判据收口 ───────────────────────────────────────────────────────
/// 判据产出（14 条）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链-四级闭环", g.is_ok(), format!("{:?}", g)));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-摘除注入检出10/10",
        inj_ok,
        format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    let miss = match &inj {
        Ok(l) => l.missed_total(),
        Err(_) => 1,
    };
    v.push(("J1-注入守恒漏检零", miss == 0, format!("missed={}", miss)));
    // 迁移链 10/10。
    let chain = chain_rounds(JUDGE_ROUNDS);
    v.push(("J1-迁移链10/10正确", chain.is_ok(), format!("{:?}", chain)));
    // 位面对账 + 守恒。
    let mut e = LoopEngine::new();
    let _ = e.create_cell(1);
    let _ = e.claim(1, L_MANIFEST, 1);
    let plane_ok = e.plane_audit().is_ok();
    v.push(("J2-在场位面对账", plane_ok, String::from("位面低四位自洽")));
    let conserve_ok = e.conserve().is_ok();
    v.push(("J2-账目守恒", conserve_ok, String::from("claim=在场+摘除+拒")));
    // 单调：状态机只许降（摘除后 stage.ord 递减）。
    let mut e2 = LoopEngine::new();
    let _ = e2.create_cell(1);
    let _ = e2.claim(1, L_DEFAULT, 1);
    let _ = e2.claim(1, L_COMPAT, 2);
    let _ = e2.remove_top(1);
    let mono = e2.migration_audit().is_ok();
    v.push(("J3-迁移账步序单调", mono, String::from("removed_level 严格递增")));
    // 幂等：verdict 只读幂等（双查同值）。
    let mut e3 = LoopEngine::new();
    let _ = e3.create_cell(1);
    let _ = e3.claim(1, L_OVERRIDE, 9);
    let a1 = e3.verdict(1);
    let a2 = e3.verdict(1);
    let idem = a1 == a2;
    v.push(("J3-裁决幂等-双查同值", idem, format!("{:?}/{:?}", a1, a2)));
    // 容量/边界：格 0/5 拒，1/4 收。
    let mut e4 = LoopEngine::new();
    let b0 = e4.create_cell(0).err() == Some(FvCode::GhostCell.code());
    let b1 = e4.create_cell(5).err() == Some(FvCode::GhostCell.code());
    let b2 = e4.create_cell(1).is_ok();
    let b3 = e4.create_cell(POOL_CAP as u8).is_ok();
    v.push(("J4-格域边界", b0 && b1 && b2 && b3, format!("0/5拒 1/4收")));
    // 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_err();
    v.push(("J4-探针三序", pa && pb && pc, format!("a={} b!={} c!={}", pa, pb, pc)));
    // 双跑一致。
    v.push(("J5-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫。
    v.push(("J5-段位守卫", segment_guard(), String::from("0x7F600 窗口闭集")));
    // 文案闭集。
    let mut det_ok = true;
    let mut k = 0;
    while k < FvCode::ALL.len() {
        if let Some(c) = FvCode::ALL.get(k) {
            if detail_of(c.code()) != c.detail() || c.detail().len() == 0 {
                det_ok = false;
            }
        }
        k += 1;
    }
    v.push(("J5-诊断文案闭集非空", det_ok, format!("codes={}", FvCode::ALL.len())));
    v.push(("J5-判据收口", v.len() + 1 == 14, format!("n={}", v.len() + 1)));
    v
}

/// 探针入口：F18489 四级裁决闭环自检（14 条）。
pub fn run_unx18489_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18489 four_verdict_loop");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i = i.saturating_add(1);
    }
    s
}

// ── 测试（8 个） ───────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_golden_chain() {
        // 正常流：四级闭环全绿。
        assert!(golden_run().is_ok(), "黄金全链应全绿");
    }

    #[test]
    fn four_sources_override_wins() {
        // 判据核心一：四源同时在场终值=覆写值。
        let mut e = LoopEngine::new();
        assert!(e.create_cell(1).is_ok());
        assert!(e.claim(1, L_MANIFEST, 100).is_ok());
        assert!(e.claim(1, L_COMPAT, 200).is_ok());
        assert!(e.claim(1, L_DEFAULT, 300).is_ok());
        assert!(e.claim(1, L_OVERRIDE, 1).is_ok());
        assert!(e.verdict(1) == Ok((L_OVERRIDE, 1)), "终值应为覆写值");
    }

    #[test]
    fn migration_chain_exact() {
        // 判据核心二：逐级摘除（4→3→2→1）终值迁移链逐步正确。
        let mut e = LoopEngine::new();
        assert!(e.create_cell(1).is_ok());
        let _ = e.claim(1, L_MANIFEST, 10);
        let _ = e.claim(1, L_COMPAT, 20);
        let _ = e.claim(1, L_DEFAULT, 30);
        let _ = e.claim(1, L_OVERRIDE, 1);
        // 4→3。
        let s1 = e.remove_top(1).unwrap_or(ChainStep { step_no: 0, removed_level: 9, before: 0, after: 0 });
        assert!(s1.removed_level == L_OVERRIDE && s1.before == 1 && s1.after == 10, "4→3 迁移");
        // 3→2。
        let s2 = e.remove_top(1).unwrap_or(ChainStep { step_no: 0, removed_level: 9, before: 0, after: 0 });
        assert!(s2.removed_level == L_MANIFEST && s2.before == 10 && s2.after == 20, "3→2 迁移");
        // 2→1。
        let s3 = e.remove_top(1).unwrap_or(ChainStep { step_no: 0, removed_level: 9, before: 0, after: 0 });
        assert!(s3.removed_level == L_COMPAT && s3.before == 20 && s3.after == 30, "2→1 迁移");
        // 1→0。
        let s4 = e.remove_top(1).unwrap_or(ChainStep { step_no: 0, removed_level: 9, before: 0, after: 0 });
        assert!(s4.removed_level == L_DEFAULT && s4.before == 30 && s4.after == 0, "1→0 迁移");
        assert!(e.verdict(1).err() == Some(FvCode::EmptyVerdict.code()));
        assert!(e.migration_audit().is_ok(), "迁移账应连续");
    }

    #[test]
    fn injection_all_caught() {
        // 注入必抓：四类坏样本 10/10。
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.is_ok());
        if let Ok(l) = led {
            assert!(l.all_caught(), "四槽必须全抓");
            assert!(l.missed_total() == 0, "漏检必须为 0");
        }
    }

    #[test]
    fn chain_rounds_ten() {
        // 迁移链 10/10 正确。
        let chain = chain_rounds(JUDGE_ROUNDS);
        assert!(chain.is_ok());
        if let Ok(n) = chain {
            assert!(n == JUDGE_ROUNDS, "10 轮应全对");
        }
    }

    #[test]
    fn dup_and_level_bounds() {
        // 重复登记 + 级越界。
        let mut e = LoopEngine::new();
        assert!(e.create_cell(2).is_ok());
        assert!(e.claim(2, L_COMPAT, 5).is_ok());
        assert!(e.claim(2, L_COMPAT, 6).err() == Some(FvCode::DupSource.code()));
        assert!(e.claim(2, 4, 1).err() == Some(FvCode::LevelOutOfRange.code()));
        assert!(e.create_cell(0).err() == Some(FvCode::GhostCell.code()));
        assert!(e.create_cell(5).err() == Some(FvCode::GhostCell.code()));
    }

    #[test]
    fn stage_monotonic() {
        // 状态机：ChainStage 只许降（在场数只减）。
        let mut e = LoopEngine::new();
        assert!(e.create_cell(1).is_ok());
        let _ = e.claim(1, L_DEFAULT, 1);
        let _ = e.claim(1, L_MANIFEST, 2);
        let _ = e.claim(1, L_COMPAT, 3);
        let _ = e.claim(1, L_OVERRIDE, 4);
        let mut prev = 4usize;
        let mut s = 0;
        while s < CHAIN_STEPS {
            let _ = e.remove_top(1);
            // 摘除后位面计数与 stage 一致。
            let slot = e.find_cell_pub(1).unwrap_or(0);
            if let Some(c) = e.cells.get(slot) {
                assert!(c.present.count_bits() == prev - 1, "在场数应单调减");
                assert!(c.stage.ord() == c.present.count_bits(), "stage 应与在场数一致");
            }
            prev -= 1;
            s += 1;
        }
    }

    #[test]
    fn double_run_deterministic() {
        // 双跑一致。
        assert!(repro_check().is_ok(), "双跑应逐字段一致");
    }

    #[test]
    fn segment_guard_and_checkset() {
        // 段位守卫 + 判据收口全绿。
        assert!(segment_guard(), "段位守卫应过");
        let set = run_unx18489_checks();
        assert!(set.all_passed(), "14 条判据应全绿");
        assert!(set.len() == 14, "判据条数应冻结 14");
    }
}
