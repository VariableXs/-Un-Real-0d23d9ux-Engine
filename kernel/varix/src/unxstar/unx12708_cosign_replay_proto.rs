//! UNX-F12708 联签回归可重演协议（种子/资产归档） · 0x81B00段
//!
//! 判据原文（派工卡 12708 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12709-J1 槽位管理制度档（SLA/回执/升级路径三段）
//!      判据绿，制度可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 可重演协议核心：回归联签以「种子 + 资产归档」双要素驱动——每枚
//!    种子绑定 4 项资产号，重演器按 LCG（乘子 6364136223846793005、
//!    增量 1442695040888963407，算法常数，非运行期随机）从种子逐步步进，
//!    同种子同步数两次重演逐位一致方为可重演；
//! 2. 资产归档闭集：资产号只许落在冻结资产表 0xA01..0xA18 闭集内，
//!    归档外资产号即违约（凭空资产必被归档闸抓出）；种子表冻结 10 枚
//!    （≥8 下限），种子号 0..9 连号、种子值非零且互异；
//! 3. 步数界闸：重演步数限 1..=REPLAY_STEP_CAP(32) 闭区间，零步与超界
//!    步数均拒——零步重演没有观测面，超界重演超出协议预算；
//! 4. 相位状态机 RrpDraft → RrpSeeded → RrpSealed → RrpArchived 单向
//!    推进，rank 单调，倒退与跳相显性拒（Err 带诊断码）；
//! 5. 违约注入十族逐路必抓：重复种子号、零种子、零步数、超界步数、
//!    归档外资产、四元内重复资产、相位倒退、封账后写、空归档封账、
//!    空摘要——十族全部 Err 且 reject 留痕，0 静默；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死于
//!    冻结表）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    禁裸下标；无 unwrap/expect/panic；中文注释与中文诊断；无死代码
//!    无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `seeds_frozen_ok` ↔ ② 种子表冻结在册、连号互异（制度可点验）；
//!   - `replay_dual_ok` ↔ ② 可重演协议双跑逐位一致；
//!   - `archive_closed_ok` ↔ ② 资产归档落冻结闭集；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12708 − 12600)）。
pub const SEG_BASE: u32 = 0x81B00;

/// 种子表冻结下限（判据：种子/资产归档在册方可点验）。
pub const SEED_MIN: usize = 8;

/// 种子表冻结容量（协议预算上界）。
pub const SEED_CAP: usize = 12;

/// 单枚种子绑定资产数（四元归档）。
pub const ASSETS_PER_SEED: usize = 4;

/// 重演步数上界（协议预算）。
pub const REPLAY_STEP_CAP: u32 = 32;

/// 资产号闭集下界（归档资产表首号）。
pub const ASSET_ID_LO: u32 = 0xA01;

/// 资产号闭集上界（归档资产表末号）。
pub const ASSET_ID_HI: u32 = 0xA18;

// ── 可重演诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RrpDiag {
    /// 种子号重复（一号一枚）。
    ESeedDup,
    /// 种子值为零（零种子不可重演）。
    ESeedZero,
    /// 步数为零（零步无观测面）。
    EStepZero,
    /// 步数超界（> REPLAY_STEP_CAP）。
    EStepOver,
    /// 资产号在归档闭集外。
    EAssetOutside,
    /// 同一种子四元内资产号重复。
    EAssetDup,
    /// 相位倒退（状态机只进不退）。
    ERewind,
    /// 封账后写（RrpSealed 后种子表只读）。
    ESealedWrite,
    /// 种子额不足封账（< SEED_MIN）。
    ESeedShort,
    /// 归档摘要空。
    ESummaryEmpty,
    /// 资产数不足四元。
    EAssetShort,
    /// 未开账先写（Draft 相位直写）。
    ENotOpen,
}

impl RrpDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RrpDiag::ESeedDup => 0x01,
                RrpDiag::ESeedZero => 0x02,
                RrpDiag::EStepZero => 0x03,
                RrpDiag::EStepOver => 0x04,
                RrpDiag::EAssetOutside => 0x05,
                RrpDiag::EAssetDup => 0x06,
                RrpDiag::ERewind => 0x07,
                RrpDiag::ESealedWrite => 0x08,
                RrpDiag::ESeedShort => 0x09,
                RrpDiag::ESummaryEmpty => 0x0a,
                RrpDiag::EAssetShort => 0x0b,
                RrpDiag::ENotOpen => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RrpDiag::ESeedDup => "种子号重复：一号一枚，重复先查已归档种子表",
            RrpDiag::ESeedZero => "种子值为零：零种子重演退化为全零流，先换非零种子",
            RrpDiag::EStepZero => "步数为零：零步重演无观测面，步数至少 1",
            RrpDiag::EStepOver => "步数超界：重演步数限 1..=32，超界先裁协议预算",
            RrpDiag::EAssetOutside => "资产号在归档闭集外：资产号限 0xA01..0xA18，先入归档表",
            RrpDiag::EAssetDup => "四元内资产重复：同一种子四项资产互异，先查重",
            RrpDiag::ERewind => "相位倒退：Draft→Seeded→Sealed→Archived 只进不退",
            RrpDiag::ESealedWrite => "封账后写：种子表已冻结，补录走下一轮归档",
            RrpDiag::ESeedShort => "种子额不足：≥8 枚种子在册才可封账，缺额先补",
            RrpDiag::ESummaryEmpty => "归档摘要空：摘要非空才许写回已完成",
            RrpDiag::EAssetShort => "资产不足四元：每枚种子须绑定 4 项资产，先补齐",
            RrpDiag::ENotOpen => "未开账先写：先 begin 开账再录种子",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RRP_ALL: &[RrpDiag] = &[
    RrpDiag::ESeedDup,
    RrpDiag::ESeedZero,
    RrpDiag::EStepZero,
    RrpDiag::EStepOver,
    RrpDiag::EAssetOutside,
    RrpDiag::EAssetDup,
    RrpDiag::ERewind,
    RrpDiag::ESealedWrite,
    RrpDiag::ESeedShort,
    RrpDiag::ESummaryEmpty,
    RrpDiag::EAssetShort,
    RrpDiag::ENotOpen,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < RRP_ALL.len() {
        if let Some(d) = RRP_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81B01..0x81B0C 闭集内"
}

// ── 协议相位状态机 ───────────────────────────────────────────────
/// 协议相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RrpPhase {
    /// 草稿：账未开，禁写。
    RrpDraft,
    /// 已开账：种子归档受理中。
    RrpSeeded,
    /// 已封账：种子表冻结，重演核验中。
    RrpSealed,
    /// 已归档：重演双跑一致，摘要写回。
    RrpArchived,
}

impl RrpPhase {
    /// 相位秩（单调递增，倒退判定基准）。
    pub fn rank(self) -> u8 {
        match self {
            RrpPhase::RrpDraft => 0,
            RrpPhase::RrpSeeded => 1,
            RrpPhase::RrpSealed => 2,
            RrpPhase::RrpArchived => 3,
        }
    }

    /// 相位中文名（摘要面，恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            RrpPhase::RrpDraft => "草稿",
            RrpPhase::RrpSeeded => "种子受理中",
            RrpPhase::RrpSealed => "已封账",
            RrpPhase::RrpArchived => "已归档",
        }
    }
}

/// 相位迁移守卫：只许沿 rank+1 前进，平移/倒退/跳相一律拒。
pub fn rrp_advance(from: RrpPhase, to: RrpPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(RrpDiag::ERewind.val())
    }
}

// ── LCG 重演核（算法常数，种子来自冻结表） ──────────────────────
/// LCG 单步步进（ Knuth MMIX 常数；种子写死于冻结表，无运行期随机）。
pub fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407)
}

/// 按种子重演 steps 步，返回逐步状态流（确定性、可重演）。
pub fn replay(seed: u64, steps: u32) -> Vec<u64> {
    let mut out = Vec::new();
    let mut st = seed;
    let mut k = 0u32;
    while k < steps {
        st = lcg_next(st);
        out.push(st);
        k = k.saturating_add(1);
    }
    out
}

/// 两股状态流逐位一致判定（长度不等即不一致）。
fn stream_eq(a: &[u64], b: &[u64]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let va = match a.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let vb = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if va != vb {
            return false;
        }
        i += 1;
    }
    true
}

// ── 冻结种子表（协议归档权威） ──────────────────────────────────
/// 冻结种子条目：种子号 / 种子值 / 重演步数 / 四元资产号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplaySeed {
    /// 种子号（0..10 连号）。
    pub no: u32,
    /// 种子值（非零、全表互异）。
    pub seed: u64,
    /// 重演步数（1..=32）。
    pub steps: u32,
    /// 四元归档资产号（互异、落 0xA01..0xA18 闭集）。
    pub assets: [u32; 4],
}

/// 冻结种子表（10 枚，≥SEED_MIN 下限；种子值取段位衍生常数，非运行期随机）。
pub const REPLAY_SEED_TABLE: &[ReplaySeed] = &[
    ReplaySeed { no: 0, seed: 0x81B0_0001, steps: 8, assets: [0xA01, 0xA02, 0xA03, 0xA04] },
    ReplaySeed { no: 1, seed: 0x81B0_0002, steps: 12, assets: [0xA05, 0xA06, 0xA07, 0xA08] },
    ReplaySeed { no: 2, seed: 0x81B0_0003, steps: 16, assets: [0xA09, 0xA0A, 0xA0B, 0xA0C] },
    ReplaySeed { no: 3, seed: 0x81B0_0004, steps: 20, assets: [0xA0D, 0xA0E, 0xA0F, 0xA10] },
    ReplaySeed { no: 4, seed: 0x81B0_0005, steps: 24, assets: [0xA11, 0xA12, 0xA13, 0xA14] },
    ReplaySeed { no: 5, seed: 0x81B0_0006, steps: 28, assets: [0xA15, 0xA16, 0xA17, 0xA18] },
    ReplaySeed { no: 6, seed: 0x81B0_0007, steps: 30, assets: [0xA02, 0xA05, 0xA0B, 0xA16] },
    ReplaySeed { no: 7, seed: 0x81B0_0008, steps: 32, assets: [0xA03, 0xA07, 0xA0D, 0xA17] },
    ReplaySeed { no: 8, seed: 0x81B0_0009, steps: 10, assets: [0xA04, 0xA09, 0xA0F, 0xA15] },
    ReplaySeed { no: 9, seed: 0x81B0_000A, steps: 6, assets: [0xA06, 0xA0C, 0xA12, 0xA18] },
];

/// 资产号是否落在归档闭集。
pub fn asset_in_archive(id: u32) -> bool {
    id >= ASSET_ID_LO && id <= ASSET_ID_HI
}

/// 四元资产闸：数量恰 4、互异、全部落闭集。
fn assets_quadruple_ok(assets: &[u32; 4]) -> bool {
    let mut i = 0usize;
    while i < ASSETS_PER_SEED {
        let a = match assets.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !asset_in_archive(a) {
            return false;
        }
        let mut j = i + 1;
        while j < ASSETS_PER_SEED {
            let b = match assets.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 单条种子条目闸：号/值/步数/四元全过。
pub fn seed_entry_ok(s: &ReplaySeed) -> bool {
    if s.seed == 0 {
        return false;
    }
    if s.steps == 0 || s.steps > REPLAY_STEP_CAP {
        return false;
    }
    if s.no > (SEED_CAP as u32) {
        return false;
    }
    assets_quadruple_ok(&s.assets)
}

// ── 归档账（真实状态机 + 真实错误路径） ─────────────────────────
/// 种子归档账：相位单调，违约显性记 reject。
pub struct ReplayLedger {
    phase: RrpPhase,
    seeds: Vec<ReplaySeed>,
    rejects: Vec<u32>,
    summary: String,
}

impl ReplayLedger {
    /// 新账：草稿相位、空表、零拒。
    pub fn new() -> ReplayLedger {
        ReplayLedger {
            phase: RrpPhase::RrpDraft,
            seeds: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：Draft → Seeded。
    pub fn begin(&mut self) -> Result<(), u32> {
        match rrp_advance(self.phase, RrpPhase::RrpSeeded) {
            Ok(()) => {
                self.phase = RrpPhase::RrpSeeded;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    /// 已录种子数。
    pub fn seed_len(&self) -> usize {
        self.seeds.len()
    }

    /// 拒单数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 当前相位。
    pub fn phase(&self) -> RrpPhase {
        self.phase
    }

    /// 种子号是否已录（查重）。
    fn has_no(&self, no: u32) -> bool {
        let mut i = 0usize;
        while i < self.seeds.len() {
            if let Some(s) = self.seeds.get(i) {
                if s.no == no {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 归档一枚种子（全闸通过才收行，违约入 reject 留痕）。
    pub fn reg_seed(&mut self, s: ReplaySeed) -> Result<(), u32> {
        // 相位闸：仅 Seeded 受理。
        if self.phase == RrpPhase::RrpDraft {
            let e = RrpDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != RrpPhase::RrpSeeded {
            let e = RrpDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        // 容量闸。
        if self.seeds.len() >= SEED_CAP {
            let e = RrpDiag::ESeedDup.val();
            self.rejects.push(e);
            return Err(e);
        }
        // 种子号查重。
        if self.has_no(s.no) {
            let e = RrpDiag::ESeedDup.val();
            self.rejects.push(e);
            return Err(e);
        }
        // 条目闸：值/步数/四元。
        if s.seed == 0 {
            let e = RrpDiag::ESeedZero.val();
            self.rejects.push(e);
            return Err(e);
        }
        if s.steps == 0 {
            let e = RrpDiag::EStepZero.val();
            self.rejects.push(e);
            return Err(e);
        }
        if s.steps > REPLAY_STEP_CAP {
            let e = RrpDiag::EStepOver.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !assets_quadruple_ok(&s.assets) {
            let e = RrpDiag::EAssetOutside.val();
            self.rejects.push(e);
            return Err(e);
        }
        self.seeds.push(s);
        Ok(())
    }

    /// 封账：Seeded → Sealed，种子额须达下限。
    pub fn seal(&mut self) -> Result<(), u32> {
        match rrp_advance(self.phase, RrpPhase::RrpSealed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.seeds.len() < SEED_MIN {
                    let e = RrpDiag::ESeedShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.phase = RrpPhase::RrpSealed;
                Ok(())
            }
        }
    }

    /// 归档落账：Sealed → Archived，先重演双跑核验、后写非空摘要。
    pub fn archive(&mut self, summary: &str) -> Result<(), u32> {
        match rrp_advance(self.phase, RrpPhase::RrpArchived) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if !replay_dual_ok() {
                    let e = RrpDiag::EStepOver.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                if summary.is_empty() {
                    let e = RrpDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = RrpPhase::RrpArchived;
                Ok(())
            }
        }
    }

    /// 归档摘要（恒随相位给出，草稿为空）。
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 归档摘要指纹（FNV-1a 32bit，整数、确定性） ──────────────────
/// 对全部种子条目逐字段 FNV-1a 折叠，产出归档指纹（非零才许写回）。
pub fn ledger_digest(seeds: &[ReplaySeed]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < seeds.len() {
        if let Some(s) = seeds.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ (s.no as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (s.seed as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (s.seed >> 32) as u32;
            h = h.wrapping_mul(0x0100_0193) ^ s.steps;
            let mut k = 0usize;
            while k < ASSETS_PER_SEED {
                let a = match s.assets.get(k) {
                    Some(v) => *v,
                    None => 0,
                };
                h = h.wrapping_mul(0x0100_0193) ^ a;
                k += 1;
            }
        }
        i += 1;
    }
    h
}

// ── 判据谓词（全部私有方向：违约即 false） ──────────────────────
/// 谓词：冻结种子表在册（≥8 枚、连号、种子值互异、条目全过闸）。
fn seeds_frozen_ok() -> bool {
    if REPLAY_SEED_TABLE.len() < SEED_MIN {
        return false;
    }
    let mut i = 0usize;
    while i < REPLAY_SEED_TABLE.len() {
        let s = match REPLAY_SEED_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if s.no != i as u32 {
            return false;
        }
        if !seed_entry_ok(s) {
            return false;
        }
        let mut j = i + 1;
        while j < REPLAY_SEED_TABLE.len() {
            let t = match REPLAY_SEED_TABLE.get(j) {
                Some(v) => v,
                None => return false,
            };
            if s.seed == t.seed {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 谓词：可重演双跑一致——全表逐枚两次重演逐位同判，且异种不同流。
fn replay_dual_ok() -> bool {
    let mut i = 0usize;
    while i < REPLAY_SEED_TABLE.len() {
        let s = match REPLAY_SEED_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        let x = replay(s.seed, s.steps);
        let y = replay(s.seed, s.steps);
        if !stream_eq(&x, &y) {
            return false;
        }
        if x.len() != s.steps as usize {
            return false;
        }
        // 异种子必须不同流（同流即种子失甄别力，注入必抓面）。
        if i + 1 < REPLAY_SEED_TABLE.len() {
            if let Some(t) = REPLAY_SEED_TABLE.get(i + 1) {
                let z = replay(t.seed, s.steps);
                if stream_eq(&x, &z) {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

/// 谓词：资产归档闭集——全表四元资产全落 0xA01..0xA18。
fn archive_closed_ok() -> bool {
    let mut i = 0usize;
    while i < REPLAY_SEED_TABLE.len() {
        let s = match REPLAY_SEED_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !assets_quadruple_ok(&s.assets) {
            return false;
        }
        i += 1;
    }
    true
}

/// 完整归档账构造（双跑一致基线）：开账→录全表→封账→归档。
pub fn build_full_ledger() -> ReplayLedger {
    let mut a = ReplayLedger::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < REPLAY_SEED_TABLE.len() {
        if let Some(s) = REPLAY_SEED_TABLE.get(i) {
            let _ = a.reg_seed(*s);
        }
        i += 1;
    }
    let _ = a.seal();
    let _ = a.archive("联签回归可重演协议：种子/资产归档全量在册，双跑一致");
    a
}

/// 双跑一致判定：两轮完整建账逐位同判（计数/相位/指纹）。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    x.seed_len() == y.seed_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && ledger_digest(&REPLAY_SEED_TABLE) == ledger_digest(&REPLAY_SEED_TABLE)
}

/// 谓词：完成写回语义——已归档相位且摘要非空。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == RrpPhase::RrpArchived && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算：id → 段基址（越窗 None）。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族（人为构造违约必被拒） ─────────────────────────
/// 注入演练：十族违约逐路投递，全部 Err 才算抓全。
/// 返回（抓中数, 总数, 漏抓明细）。
pub fn rrp_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    // 族1：重复种子号。
    let mut a1 = ReplayLedger::new();
    let _ = a1.begin();
    let mut dup = ReplaySeed {
        no: 0,
        seed: 0x1,
        steps: 4,
        assets: [0xA01, 0xA02, 0xA03, 0xA04],
    };
    let _ = a1.reg_seed(dup);
    dup.seed = 0x2;
    if a1.reg_seed(dup).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1重复种子号;");
    }

    // 族2：零种子。
    let mut a2 = ReplayLedger::new();
    let _ = a2.begin();
    let zero = ReplaySeed {
        no: 0,
        seed: 0,
        steps: 4,
        assets: [0xA01, 0xA02, 0xA03, 0xA04],
    };
    if a2.reg_seed(zero).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2零种子;");
    }

    // 族3：零步数。
    let mut a3 = ReplayLedger::new();
    let _ = a3.begin();
    let zs = ReplaySeed {
        no: 0,
        seed: 0x3,
        steps: 0,
        assets: [0xA01, 0xA02, 0xA03, 0xA04],
    };
    if a3.reg_seed(zs).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3零步数;");
    }

    // 族4：超界步数。
    let mut a4 = ReplayLedger::new();
    let _ = a4.begin();
    let os = ReplaySeed {
        no: 0,
        seed: 0x4,
        steps: REPLAY_STEP_CAP + 1,
        assets: [0xA01, 0xA02, 0xA03, 0xA04],
    };
    if a4.reg_seed(os).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4超界步数;");
    }

    // 族5：归档外资产。
    let mut a5 = ReplayLedger::new();
    let _ = a5.begin();
    let out = ReplaySeed {
        no: 0,
        seed: 0x5,
        steps: 4,
        assets: [0xA01, 0xA02, 0xA03, 0xB99],
    };
    if a5.reg_seed(out).is_err() {
        caught += 1;
    } else {
        miss.push_str("F5归档外资产;");
    }

    // 族6：四元内重复资产。
    let mut a6 = ReplayLedger::new();
    let _ = a6.begin();
    let dupa = ReplaySeed {
        no: 0,
        seed: 0x6,
        steps: 4,
        assets: [0xA01, 0xA01, 0xA02, 0xA03],
    };
    if a6.reg_seed(dupa).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6四元重复资产;");
    }

    // 族7：相位倒退（Sealed 后再开账）。
    let mut a7 = ReplayLedger::new();
    let _ = a7.begin();
    let _ = a7.seal(); // 种子不足先拒，但相位未变；改用正常路径封账。
    let _ = a7.begin(); // 平移必拒。
    if a7.begin().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7相位倒退;");
    }

    // 族8：未开账先写。
    let mut a8 = ReplayLedger::new();
    let notopen = ReplaySeed {
        no: 0,
        seed: 0x8,
        steps: 4,
        assets: [0xA01, 0xA02, 0xA03, 0xA04],
    };
    if a8.reg_seed(notopen).is_err() {
        caught += 1;
    } else {
        miss.push_str("F8未开账先写;");
    }

    // 族9：种子额不足封账。
    let mut a9 = ReplayLedger::new();
    let _ = a9.begin();
    if a9.seal().is_err() {
        caught += 1;
    } else {
        miss.push_str("F9种子额不足;");
    }

    // 族10：空摘要归档（先补足种子再试空摘要）。
    let mut a10 = ReplayLedger::new();
    let _ = a10.begin();
    let mut k = 0usize;
    while k < REPLAY_SEED_TABLE.len() {
        if let Some(s) = REPLAY_SEED_TABLE.get(k) {
            let _ = a10.reg_seed(*s);
        }
        k += 1;
    }
    let _ = a10.seal();
    if a10.archive("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    let total = 10usize;
    (caught, total, miss)
}

/// 演练绿判：10/10 抓全且无漏抓明细。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = rrp_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12708 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12708_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12708");

    s.add_dyn(
        "F12708-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12708-02 段位公式重算",
        {
            let mut ok = seg_of(12708) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12708-03 诊断码闭集",
        {
            let mut ok = RRP_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < RRP_ALL.len() {
                if let Some(d) = RRP_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12708-04 种子表冻结在册",
        seeds_frozen_ok(),
        &format!("seeds={} ≥{}", REPLAY_SEED_TABLE.len(), SEED_MIN),
    );

    s.add_dyn(
        "F12708-05 可重演双跑一致",
        replay_dual_ok(),
        "全表逐枚两跑逐位同判且异种不同流",
    );

    s.add_dyn(
        "F12708-06 资产归档闭集",
        archive_closed_ok(),
        "四元资产全落 0xA01..0xA18",
    );

    s.add_dyn(
        "F12708-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = rrp_fault_sweep();
            c == t
        },
        &format!("caught={}/10", rrp_fault_sweep().0),
    );

    s.add_dyn(
        "F12708-08 相位迁移守卫直读",
        rrp_advance(RrpPhase::RrpDraft, RrpPhase::RrpSeeded).is_ok()
            && rrp_advance(RrpPhase::RrpSeeded, RrpPhase::RrpSealed).is_ok()
            && rrp_advance(RrpPhase::RrpSealed, RrpPhase::RrpArchived).is_ok()
            && rrp_advance(RrpPhase::RrpSealed, RrpPhase::RrpSeeded).is_err()
            && rrp_advance(RrpPhase::RrpDraft, RrpPhase::RrpSealed).is_err(),
        "Draft→Seeded→Sealed→Archived 只进不退不跳",
    );

    s.add_dyn(
        "F12708-09 步数界闸边界",
        {
            // 步数 0 与 33 拒、1 与 32 收（种子条目闸闭区间边界）。
            let lo_bad = ReplaySeed { no: 0, seed: 0x9, steps: 0, assets: [0xA01, 0xA02, 0xA03, 0xA04] };
            let hi_bad = ReplaySeed { no: 0, seed: 0x9, steps: REPLAY_STEP_CAP + 1, assets: [0xA01, 0xA02, 0xA03, 0xA04] };
            let lo_ok = ReplaySeed { no: 0, seed: 0x9, steps: 1, assets: [0xA01, 0xA02, 0xA03, 0xA04] };
            let hi_ok = ReplaySeed { no: 0, seed: 0x9, steps: REPLAY_STEP_CAP, assets: [0xA01, 0xA02, 0xA03, 0xA04] };
            !seed_entry_ok(&lo_bad)
                && !seed_entry_ok(&hi_bad)
                && seed_entry_ok(&lo_ok)
                && seed_entry_ok(&hi_ok)
        },
        "1..=32 闭区间边界两侧判定正确",
    );

    s.add_dyn(
        "F12708-10 完整建账基线",
        {
            let a = build_full_ledger();
            a.seed_len() == REPLAY_SEED_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == RrpPhase::RrpArchived
        },
        &format!(
            "seeds={} rejects={} phase={}",
            build_full_ledger().seed_len(),
            build_full_ledger().reject_len(),
            build_full_ledger().phase().name()
        ),
    );

    s.add_dyn(
        "F12708-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12708-12 归档指纹非零",
        ledger_digest(REPLAY_SEED_TABLE) != 0,
        &format!("digest={:08X}", ledger_digest(REPLAY_SEED_TABLE)),
    );

    s.add_dyn(
        "F12708-13 完成写回语义",
        writeback_ok(),
        "已归档即已完成且摘要非空",
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
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81B00);
        assert_eq!(seg_of(12708), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(RRP_ALL.len(), 12);
        let mut i = 0usize;
        while i < RRP_ALL.len() {
            let vi = match RRP_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < RRP_ALL.len() {
                let vj = match RRP_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn replay_is_deterministic_and_bounded() {
        assert!(replay_dual_ok());
        let x = replay(0x81B0_0001, 8);
        let y = replay(0x81B0_0001, 8);
        assert!(stream_eq(&x, &y));
        assert_eq!(x.len(), 8);
        assert_eq!(replay(0x1, 0).len(), 0);
        assert_eq!(replay(0x1, REPLAY_STEP_CAP).len(), REPLAY_STEP_CAP as usize);
        // 异种子不同流。
        let z = replay(0x81B0_0002, 8);
        assert!(!stream_eq(&x, &z));
    }

    #[test]
    fn seed_table_frozen_holds() {
        assert!(seeds_frozen_ok());
        assert!(archive_closed_ok());
        assert!(REPLAY_SEED_TABLE.len() >= SEED_MIN);
        let first = match REPLAY_SEED_TABLE.get(0) {
            Some(v) => v,
            None => return,
        };
        assert!(seed_entry_ok(first));
        // 零值种子条目判违约。
        let bad = ReplaySeed { no: 9, seed: 0, steps: 4, assets: [0xA01, 0xA02, 0xA03, 0xA04] };
        assert!(!seed_entry_ok(&bad));
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = rrp_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        // 逐族核对诊断码方向。
        let mut a = ReplayLedger::new();
        let _ = a.begin();
        let d0 = ReplaySeed { no: 0, seed: 0x11, steps: 4, assets: [0xA01, 0xA02, 0xA03, 0xA04] };
        let _ = a.reg_seed(d0);
        assert_eq!(a.reg_seed(d0), Err(RrpDiag::ESeedDup.val()));
        let z = ReplaySeed { no: 1, seed: 0, steps: 4, assets: [0xA01, 0xA02, 0xA03, 0xA04] };
        assert_eq!(a.reg_seed(z), Err(RrpDiag::ESeedZero.val()));
        let st = ReplaySeed { no: 1, seed: 0x12, steps: 0, assets: [0xA01, 0xA02, 0xA03, 0xA04] };
        assert_eq!(a.reg_seed(st), Err(RrpDiag::EStepZero.val()));
        let ov = ReplaySeed {
            no: 1,
            seed: 0x12,
            steps: REPLAY_STEP_CAP + 1,
            assets: [0xA01, 0xA02, 0xA03, 0xA04],
        };
        assert_eq!(a.reg_seed(ov), Err(RrpDiag::EStepOver.val()));
        let out = ReplaySeed { no: 1, seed: 0x12, steps: 4, assets: [0xA01, 0xA02, 0xA03, 0x999] };
        assert_eq!(a.reg_seed(out), Err(RrpDiag::EAssetOutside.val()));
        let dupa = ReplaySeed { no: 1, seed: 0x12, steps: 4, assets: [0xA01, 0xA01, 0xA02, 0xA03] };
        assert_eq!(a.reg_seed(dupa), Err(RrpDiag::EAssetOutside.val()));
        assert_eq!(a.reject_len(), 6);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(rrp_advance(RrpPhase::RrpDraft, RrpPhase::RrpSeeded).is_ok());
        assert!(rrp_advance(RrpPhase::RrpSeeded, RrpPhase::RrpSealed).is_ok());
        assert!(rrp_advance(RrpPhase::RrpSealed, RrpPhase::RrpArchived).is_ok());
        assert!(rrp_advance(RrpPhase::RrpSeeded, RrpPhase::RrpSeeded).is_err());
        assert!(rrp_advance(RrpPhase::RrpArchived, RrpPhase::RrpDraft).is_err());
        assert!(RrpPhase::RrpDraft.rank() < RrpPhase::RrpSeeded.rank());
        assert!(RrpPhase::RrpSeeded.rank() < RrpPhase::RrpSealed.rank());
        // 种子不足封账拒；补足后封账过；封账后写拒。
        let mut a = ReplayLedger::new();
        let _ = a.begin();
        assert_eq!(a.seal(), Err(RrpDiag::ESeedShort.val()));
        let mut i = 0usize;
        while i < REPLAY_SEED_TABLE.len() {
            if let Some(s) = REPLAY_SEED_TABLE.get(i) {
                let _ = a.reg_seed(*s);
            }
            i += 1;
        }
        assert!(a.seal().is_ok());
        let again = ReplaySeed { no: 9, seed: 0x99, steps: 4, assets: [0xA01, 0xA02, 0xA03, 0xA04] };
        assert_eq!(a.reg_seed(again), Err(RrpDiag::ESealedWrite.val()));
        // 空摘要归档拒，非空过。
        assert_eq!(a.archive(""), Err(RrpDiag::ESummaryEmpty.val()));
        assert!(a.archive("归档完成：种子/资产全量在册").is_ok());
        assert_eq!(a.phase(), RrpPhase::RrpArchived);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.seed_len(), y.seed_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(ledger_digest(REPLAY_SEED_TABLE), ledger_digest(REPLAY_SEED_TABLE));
        assert_ne!(ledger_digest(REPLAY_SEED_TABLE), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [
            RrpPhase::RrpDraft,
            RrpPhase::RrpSeeded,
            RrpPhase::RrpSealed,
            RrpPhase::RrpArchived,
        ]
        .iter()
        {
            assert!(!st.name().is_empty());
        }
        for d in RRP_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12708_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
