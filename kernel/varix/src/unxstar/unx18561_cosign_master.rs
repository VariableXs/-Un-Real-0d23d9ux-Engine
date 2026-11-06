//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18561 · 版本面联签总纲（三段联合冻结） · 0x80800段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18561-J1 三段（声明/兼容层/档案）联签清单归并一致断言，
//!       会签留痕齐备
//! 设计要点：
//!   - 三段各持一台状态机（草拟→复核→签署→冻结），只许单调前进，
//!     越级/倒退一律显式拒（拒因入账不静默）；
//!   - 会签留痕：每段签署留痕 = （签署方 id + 指纹 + 轮次），冻结时
//!     对全部留痕逐条重算指纹，重算不符即判篡改（FpMismatch）；
//!   - 归并一致：三段全部冻结后方可联合冻结；联合指纹由三段冻结指纹
//!     按段序合并重算；归并账逐条与各段冻结指纹对拍，一条不齐即红；
//!   - 注入核三类：越级冻结 / 重复签署 / 指纹篡改，各 10 轮全抓，
//!     漏检必须为 0（tries = caught + missed 守恒）；
//!   - 边界：签署方 0（下界）/ 必签上限 / 留痕槽满+1 / 段号越界；
//!   - LCG 种子写死 = SEG_BASE（0x80800）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 结构清单：
//!   数据结构：SigRecord（留痕）/ SegSlot（段槽）/ CosignLedger（三段联签总账）
//!   状态机  ：SegState（草拟→复核→签署→冻结，单调四态，越级倒退双拒）
//!   账本    ：签署留痕数组账 + 归并账 + 注入检出账（三类）
//!   错误路径：CosignCode 14 码闭集，code() = SEG_BASE + n（0x80800 窗）
//!   注入核  ：越级/重复/篡改三类逐类断言被显式拒绝（10/10）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80800，AI-VX11-1105 批09 第 1 槽，0x40 对齐独占窗）。
pub const SEG_BASE: u32 = 0x80800;

// ── 冻结常量（写死并注明推导） ──────────────────────────────────
/// 三段闭集：声明 / 兼容层 / 档案（判据原文给定，不可增删）。
pub const SEG_COUNT: usize = 3;
/// 每段必签方数（推导：双签 + 会签方 = 3，少一即会签不齐）。
pub const SIGNERS_REQUIRED: usize = 3;
/// 留痕槽容量（推导：必签 3 + 冗余 1 = 4；满后拒收不覆盖）。
pub const SIG_SLOTS: usize = 4;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 联合指纹稀释常量（PI 前 64 位，冻结推导种子）。
pub const FP_SEED: u64 = 0x243F_6A88_85A3_08D3;
/// 归并指纹种子（√3 前 64 位，与 FP_SEED 区分域）。
pub const MERGE_SEED: u64 = 0x1319_8A2E_0370_7344;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignCode {
    /// 越级：状态机跳步（如 Draft 直取 Frozen）
    StateSkip,
    /// 倒退：状态机回退（如 Frozen 退回 Signed）
    StateBack,
    /// 重复签署：同一签署方二签
    DupSigner,
    /// 留痕槽满：SIG_SLOTS 用尽仍收签
    SigSlotsFull,
    /// 签署不足：冻结时必签方缺席
    SignersShort,
    /// 冻结未就绪：状态未到签署态即求冻
    FreezeNotReady,
    /// 指纹不齐：留痕重算与原值不符
    FpMismatch,
    /// 联合未就绪：三段未全冻结即求联合
    JointNotReady,
    /// 联合已冻：联合冻结重复执行（幂等拒）
    JointDone,
    /// 段号越界：0..3 之外
    SegOutOfRange,
    /// 账目守恒破：留痕总数 ≠ 签署计数
    LedgerBreak,
    /// 归并不一致：归并账与冻结指纹对拍不符
    MergeMismatch,
    /// 双跑漂移：同输入两跑不同
    NotReproducible,
    /// 未知诊断码兜底
    Unknown,
}

impl CosignCode {
    /// 诊断码 = SEG_BASE + 序号（段内闭集，const 可用于静态裁决表）。
    pub const fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文建议（文案闭集，每码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            CosignCode::StateSkip => "越级跳步：按 草拟→复核→签署→冻结 逐级走",
            CosignCode::StateBack => "状态倒退：冻结后不回退，重开新账",
            CosignCode::DupSigner => "重复签署：一签署方一签，改签用会签方",
            CosignCode::SigSlotsFull => "留痕槽满：4 槽用尽，归并旧账再开新账",
            CosignCode::SignersShort => "签署不足：必签 3 方齐才准冻",
            CosignCode::FreezeNotReady => "冻结未就绪：先到签署态再冻",
            CosignCode::FpMismatch => "指纹不齐：留痕被改，按原 payload 重签",
            CosignCode::JointNotReady => "联合未就绪：三段全冻结才准联合",
            CosignCode::JointDone => "联合已冻：联合冻结幂等，不重复归并",
            CosignCode::SegOutOfRange => "段号越界：段号只在 0..3 内取",
            CosignCode::LedgerBreak => "账目不守恒：留痕数对不上签署计数",
            CosignCode::MergeMismatch => "归并不一致：归并账与冻结指纹重算对拍",
            CosignCode::NotReproducible => "双跑漂移：冻结种子重跑对拍",
            CosignCode::Unknown => "未知诊断码：不在 0x80800 段闭集内",
        }
    }
    pub const ALL: [CosignCode; 14] = [
        CosignCode::StateSkip,
        CosignCode::StateBack,
        CosignCode::DupSigner,
        CosignCode::SigSlotsFull,
        CosignCode::SignersShort,
        CosignCode::FreezeNotReady,
        CosignCode::FpMismatch,
        CosignCode::JointNotReady,
        CosignCode::JointDone,
        CosignCode::SegOutOfRange,
        CosignCode::LedgerBreak,
        CosignCode::MergeMismatch,
        CosignCode::NotReproducible,
        CosignCode::Unknown,
    ];
}

/// 码值反查中文建议（段外码落 Unknown 兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CosignCode::ALL.len() {
        if CosignCode::ALL[i].code() == code {
            return CosignCode::ALL[i].detail();
        }
        i += 1;
    }
    CosignCode::Unknown.detail()
}

// ── 指纹原语（确定性，无随机） ──────────────────────────────────
/// 64 位雪崩混合（SplitMix 终混两步，写死常量，双跑必同）。
pub fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    x
}

/// 单条留痕指纹：段号、签署方、payload、轮次四元混合。
/// 推导：seg 先乘黄金比例再加签者左移 8 位，确保不同段不同签者不撞域。
pub fn sign_fp(seg: u32, signer: u32, payload: u64, round: u32) -> u64 {
    let a = (seg as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) + ((signer as u64) << 8);
    mix64(mix64(a) ^ payload ^ (round as u64))
}

// ── 段内状态机（单调四态） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegState {
    Draft = 0,
    Reviewed = 1,
    Signed = 2,
    Frozen = 3,
}

impl SegState {
    /// 状态秩（0..4，单调轴）。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单调前进一步：恰好 +1 放行，倒退拒 StateBack，跳级拒 StateSkip。
    pub fn advance(self, to: SegState) -> Result<SegState, u32> {
        let from = self.rank();
        let dst = to.rank();
        if dst == from + 1 {
            Ok(to)
        } else if dst <= from {
            Err(CosignCode::StateBack.code())
        } else {
            Err(CosignCode::StateSkip.code())
        }
    }
}

// ── 数据结构：留痕 / 段槽 / 总账 ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SigRecord {
    /// 签署方 id（0..SIGNERS_REQUIRED 为必签集）。
    pub signer: u32,
    /// 签署时算出的留痕指纹（冻结时逐条重算对拍）。
    pub fp: u64,
    /// 签署轮次（判据轮数轴，非墙钟）。
    pub round: u32,
}

#[derive(Clone, Copy)]
pub struct SegSlot {
    /// 段号（0=声明 1=兼容层 2=档案）。
    pub seg: u32,
    /// 段状态机。
    pub state: SegState,
    /// 留痕数组账（定容，满拒收）。
    pub recs: [Option<SigRecord>; SIG_SLOTS],
    /// 已用留痕数。
    pub rec_n: usize,
    /// 冻结指纹（冻结时算出，联合归并时消费）。
    pub frozen_fp: u64,
}

impl SegSlot {
    pub const fn new(seg: u32) -> SegSlot {
        SegSlot {
            seg,
            state: SegState::Draft,
            recs: [None; SIG_SLOTS],
            rec_n: 0,
            frozen_fp: 0,
        }
    }

    /// 签署：复核后、冻结前方可签；同签署方拒重；槽满拒收。
    pub fn sign(&mut self, signer: u32, payload: u64, round: u32) -> Result<u64, u32> {
        if self.state.rank() < SegState::Reviewed.rank() {
            // 复核未过不收签：等价越级。
            return Err(CosignCode::StateSkip.code());
        }
        if self.state == SegState::Frozen {
            // 冻结后不收签：等价倒退。
            return Err(CosignCode::StateBack.code());
        }
        if self.rec_n >= SIG_SLOTS {
            return Err(CosignCode::SigSlotsFull.code());
        }
        let mut i = 0;
        while i < SIG_SLOTS {
            if let Some(r) = self.recs[i] {
                if r.signer == signer {
                    return Err(CosignCode::DupSigner.code());
                }
            }
            i += 1;
        }
        let f = sign_fp(self.seg, signer, payload, round);
        self.recs[self.rec_n] = Some(SigRecord { signer, fp: f, round });
        self.rec_n += 1;
        Ok(f)
    }

    /// 篡改核（仅注入面调用）：翻转首条留痕指纹三个位。
    pub fn tamper_first(&mut self) {
        if let Some(r0) = self.recs[0].as_mut() {
            r0.fp ^= 0x0001_0001_0000_0001;
        }
    }

    /// 冻结：状态须已到签署态、必签方齐、全部留痕重算对拍通过。
    pub fn freeze(&mut self, payload: u64) -> Result<u64, u32> {
        if self.state.rank() < SegState::Signed.rank() {
            return Err(CosignCode::FreezeNotReady.code());
        }
        if self.rec_n < SIGNERS_REQUIRED {
            return Err(CosignCode::SignersShort.code());
        }
        let mut acc: u64 = FP_SEED;
        let mut i = 0;
        while i < self.rec_n {
            let want = match self.recs[i] {
                Some(r) => {
                    let w = sign_fp(self.seg, r.signer, payload, r.round);
                    if w != r.fp {
                        // 留痕被篡改：重算不符必拒。
                        return Err(CosignCode::FpMismatch.code());
                    }
                    r
                }
                None => return Err(CosignCode::LedgerBreak.code()),
            };
            acc = mix64(acc ^ want.fp ^ (i as u64 + 1));
            i += 1;
        }
        self.state = self.state.advance(SegState::Frozen)?;
        self.frozen_fp = acc;
        Ok(acc)
    }
}

/// 三段联签总账（数组账本 + 归并账 + 计数守恒）。
pub struct CosignLedger {
    /// 三段槽（0=声明 1=兼容层 2=档案）。
    pub segs: [SegSlot; SEG_COUNT],
    /// 联合指纹（联合冻结后置位，幂等闸用）。
    pub joint: Option<u64>,
    /// 归并账：各段冻结指纹按段序入账。
    pub merge_log: Vec<u64>,
    /// 签署总计数（与留痕总数守恒）。
    pub sign_count: u32,
    /// 冻结总计数（与归并账长度守恒）。
    pub freeze_count: u32,
}

impl CosignLedger {
    pub fn new() -> CosignLedger {
        CosignLedger {
            segs: [SegSlot::new(0), SegSlot::new(1), SegSlot::new(2)],
            joint: None,
            merge_log: Vec::new(),
            sign_count: 0,
            freeze_count: 0,
        }
    }

    /// 段状态推进（段号越界显式拒）。
    pub fn promote(&mut self, seg: usize, to: SegState) -> Result<(), u32> {
        if seg >= SEG_COUNT {
            return Err(CosignCode::SegOutOfRange.code());
        }
        let s = &mut self.segs[seg];
        s.state = s.state.advance(to)?;
        Ok(())
    }

    /// 签署并计数（计数用 saturating 防溢出，账不丢）。
    pub fn sign(&mut self, seg: usize, signer: u32, payload: u64, round: u32) -> Result<u64, u32> {
        if seg >= SEG_COUNT {
            return Err(CosignCode::SegOutOfRange.code());
        }
        let f = self.segs[seg].sign(signer, payload, round)?;
        self.sign_count = self.sign_count.saturating_add(1);
        Ok(f)
    }

    /// 冻结单段并落归并账。
    pub fn freeze_seg(&mut self, seg: usize, payload: u64) -> Result<u64, u32> {
        if seg >= SEG_COUNT {
            return Err(CosignCode::SegOutOfRange.code());
        }
        let f = self.segs[seg].freeze(payload)?;
        self.freeze_count = self.freeze_count.saturating_add(1);
        self.merge_log.push(f);
        Ok(f)
    }

    /// 会签留痕守恒：三段留痕总数 = 签署总计数。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut total = 0usize;
        let mut i = 0;
        while i < SEG_COUNT {
            total += self.segs[i].rec_n;
            i += 1;
        }
        if total as u32 != self.sign_count {
            return Err(CosignCode::LedgerBreak.code());
        }
        if self.merge_log.len() as u32 != self.freeze_count {
            return Err(CosignCode::LedgerBreak.code());
        }
        Ok(())
    }

    /// 三段联合冻结：全 Frozen + 归并账恰三条 + 幂等闸。
    pub fn joint_freeze(&mut self) -> Result<u64, u32> {
        if self.joint.is_some() {
            return Err(CosignCode::JointDone.code());
        }
        let mut i = 0;
        while i < SEG_COUNT {
            if self.segs[i].state != SegState::Frozen {
                return Err(CosignCode::JointNotReady.code());
            }
            if self.segs[i].rec_n < SIGNERS_REQUIRED {
                return Err(CosignCode::SignersShort.code());
            }
            i += 1;
        }
        // 归并一致：归并账须恰为三段冻结指纹（段序）。
        if self.merge_log.len() != SEG_COUNT {
            return Err(CosignCode::LedgerBreak.code());
        }
        let mut acc: u64 = MERGE_SEED;
        i = 0;
        while i < SEG_COUNT {
            let got = match self.merge_log.get(i) {
                Some(v) => *v,
                None => return Err(CosignCode::MergeMismatch.code()),
            };
            if got != self.segs[i].frozen_fp {
                return Err(CosignCode::MergeMismatch.code());
            }
            acc = mix64(acc ^ got ^ (i as u64 + 1));
            i += 1;
        }
        self.joint = Some(acc);
        Ok(acc)
    }
}

// ── 冻结裁决表（输入三段状态 → 期望码；0=就绪） ────────────────
/// 裁决表闭式：三段全 Frozen 就绪；任一段未到 Frozen 拒 JointNotReady。
/// 四例样本覆盖：全就绪 / 声明未冻 / 兼容层未冻 / 档案未冻。
pub const ADJUDICATION_CASES: [([SegState; SEG_COUNT], u32); 4] = [
    (
        [SegState::Frozen, SegState::Frozen, SegState::Frozen],
        0,
    ),
    (
        [SegState::Signed, SegState::Frozen, SegState::Frozen],
        CosignCode::JointNotReady.code(),
    ),
    (
        [SegState::Frozen, SegState::Draft, SegState::Frozen],
        CosignCode::JointNotReady.code(),
    ),
    (
        [SegState::Frozen, SegState::Frozen, SegState::Reviewed],
        CosignCode::JointNotReady.code(),
    ),
];

/// 冻结裁决：输入三段状态判联合就绪（0=就绪，否则约定码）。
pub fn adjudicate_joint(states: [SegState; SEG_COUNT]) -> u32 {
    let mut i = 0;
    while i < SEG_COUNT {
        if states[i].rank() < SegState::Frozen.rank() {
            return CosignCode::JointNotReady.code();
        }
        i += 1;
    }
    0
}

/// 裁决表逐例对拍（判据复算核）。
pub fn adjudication_ok() -> bool {
    let mut i = 0;
    while i < ADJUDICATION_CASES.len() {
        let (states, want) = ADJUDICATION_CASES[i];
        if adjudicate_joint(states) != want {
            return false;
        }
        i += 1;
    }
    true
}

// ── 标准流程与黄金全链 ──────────────────────────────────────────
/// 标准冻结流程：一段三签（必签集 0..3）→冻结，返回冻结指纹。
pub fn sign_and_freeze(l: &mut CosignLedger, seg: usize, payload: u64, round: u32) -> Result<u64, u32> {
    l.promote(seg, SegState::Reviewed)?;
    l.promote(seg, SegState::Signed)?;
    let mut k = 0;
    while k < SIGNERS_REQUIRED {
        l.sign(seg, k as u32, payload, round + k as u32)?;
        k += 1;
    }
    l.freeze_seg(seg, payload)
}

/// 黄金全链：三段各三签三冻→守恒→联合冻结→归并逐条对拍。
pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let mut l = CosignLedger::new();
    let mut g = 0;
    while g < SEG_COUNT {
        // payload 由段号推导：确保三段域互异不撞指纹。
        let payload = 0x5EED_0000u64 + g as u64;
        sign_and_freeze(&mut l, g, payload, 1)?;
        g += 1;
    }
    l.conserve()?;
    let jf = l.joint_freeze()?;
    // 归并一致：归并账逐条等于各段冻结指纹。
    let mut i = 0;
    while i < SEG_COUNT {
        let got = match l.merge_log.get(i) {
            Some(v) => *v,
            None => return Err(CosignCode::MergeMismatch.code()),
        };
        if got != l.segs[i].frozen_fp {
            return Err(CosignCode::MergeMismatch.code());
        }
        i += 1;
    }
    Ok((l.sign_count, l.freeze_count, jf))
}

// ── 注入检出账（三类槽） ────────────────────────────────────────
pub struct InjectLedger {
    /// 各类注入次数。
    pub tries: [u32; 3],
    /// 各类抓到次数。
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一笔（槽号越界静默丢弃由界外判据兜底）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 漏检数（tries - caught，逐类求和，必须为 0）。
    pub fn missed(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
    /// 全抓断言（三类都有轮次且逐类 caught == tries）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入核：三类违约各 JUDGE_ROUNDS 轮，逐类断言被显式拒绝。
///   类 0：越级冻结（Draft 直取冻结 → FreezeNotReady）
///   类 1：重复签署（同签署方二签 → DupSigner）
///   类 2：指纹篡改（留痕改位 → 冻结重算不符 FpMismatch）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 类 0：越级冻结。
        let mut a = CosignLedger::new();
        let hit_a = match a.freeze_seg(0, 1) {
            Err(c) if c == CosignCode::FreezeNotReady.code() => true,
            _ => false,
        };
        led.record(0, hit_a);
        // 类 1：重复签署（先造合法双签，再让签署方 0 二签）。
        let mut b = CosignLedger::new();
        b.promote(0, SegState::Reviewed)?;
        b.promote(0, SegState::Signed)?;
        b.sign(0, 0, 7, 1)?;
        let hit_b = match b.sign(0, 0, 7, 2) {
            Err(c) if c == CosignCode::DupSigner.code() => true,
            _ => false,
        };
        led.record(1, hit_b);
        // 类 2：指纹篡改（三签后翻首条指纹，冻结必须拒）。
        let mut c = CosignLedger::new();
        c.promote(0, SegState::Reviewed)?;
        c.promote(0, SegState::Signed)?;
        c.sign(0, 0, 7, 1)?;
        c.sign(0, 1, 7, 1)?;
        c.sign(0, 2, 7, 1)?;
        c.segs[0].tamper_first();
        let hit_c = match c.freeze_seg(0, 7) {
            Err(cc) if cc == CosignCode::FpMismatch.code() => true,
            _ => false,
        };
        led.record(2, hit_c);
        r += 1;
    }
    Ok(led)
}

// ── 边界与探针 ──────────────────────────────────────────────────
/// 边界探针：签署方下界 0 / 必签上限-1 / 段号越界三档。
pub fn boundary_probe() -> Result<(), u32> {
    let mut l = CosignLedger::new();
    l.promote(0, SegState::Reviewed)?;
    l.promote(0, SegState::Signed)?;
    // 下界：签署方 0 合法。
    l.sign(0, 0, 3, 1)?;
    // 必签上限-1：合法（SIGNERS_REQUIRED-1 为必签集末位）。
    l.sign(0, (SIGNERS_REQUIRED - 1) as u32, 3, 1)?;
    // 段号越界：必拒 SegOutOfRange。
    match l.sign(SEG_COUNT, 0, 3, 1) {
        Err(c) if c == CosignCode::SegOutOfRange.code() => {}
        _ => return Err(CosignCode::SegOutOfRange.code()),
    }
    Ok(())
}

/// 容量探针：留痕槽 SIG_SLOTS 用满后第 +1 签必拒（账不丢）。
pub fn capacity_probe() -> Result<(), u32> {
    let mut l = CosignLedger::new();
    l.promote(1, SegState::Reviewed)?;
    l.promote(1, SegState::Signed)?;
    let mut k = 0u32;
    // 用 10.. 之外的签署方避开必签集，只测容量。
    while (k as usize) < SIG_SLOTS {
        l.sign(1, 10 + k, 5, 1)?;
        k += 1;
    }
    let before = l.sign_count;
    match l.sign(1, 99, 5, 1) {
        Err(c) if c == CosignCode::SigSlotsFull.code() => {
            // 满拒不丢账：计数保持。
            if l.sign_count != before {
                return Err(CosignCode::LedgerBreak.code());
            }
            Ok(())
        }
        _ => Err(CosignCode::SigSlotsFull.code()),
    }
}

/// 探针 A：联合未就绪（只冻一段即联合）必拒。
pub fn probe_joint_not_ready() -> Result<(), u32> {
    let mut l = CosignLedger::new();
    sign_and_freeze(&mut l, 0, 9, 1)?;
    match l.joint_freeze() {
        Err(c) if c == CosignCode::JointNotReady.code() => Ok(()),
        _ => Err(CosignCode::JointNotReady.code()),
    }
}

/// 探针 B：联合冻结幂等（二次联合）必拒。
pub fn probe_joint_idempotent() -> Result<(), u32> {
    let mut l = CosignLedger::new();
    let mut g = 0;
    while g < SEG_COUNT {
        sign_and_freeze(&mut l, g, 0x100 + g as u64, 1)?;
        g += 1;
    }
    l.joint_freeze()?;
    match l.joint_freeze() {
        Err(c) if c == CosignCode::JointDone.code() => Ok(()),
        _ => Err(CosignCode::JointDone.code()),
    }
}

/// 探针 C：归并账缺条（清空后联合）必拒 LedgerBreak。
pub fn probe_merge_break() -> Result<(), u32> {
    let mut l = CosignLedger::new();
    let mut g = 0;
    while g < SEG_COUNT {
        sign_and_freeze(&mut l, g, 0x200 + g as u64, 1)?;
        g += 1;
    }
    // 注入：清空归并账再联合——账不齐必拒。
    l.merge_log.clear();
    match l.joint_freeze() {
        Err(c) if c == CosignCode::LedgerBreak.code() => Ok(()),
        _ => Err(CosignCode::LedgerBreak.code()),
    }
}

/// 双跑一致：黄金全链两跑逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CosignCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：SEG 对齐/窗位 + 诊断码闭集段内 + 文案非空。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x84000).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < CosignCode::ALL.len() {
        let v = CosignCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if CosignCode::ALL[i].detail().len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 运行期摘要（动态明细消费）：三段态 + 计数 + 联合位。
pub fn summary() -> String {
    let g = golden_run();
    format!("{:?}", g)
}

// ── 检查入口（判据 8–20 条，静态主体 + 动态 ≤8） ────────────────
pub fn run_unx18561_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18561 版本面联签总纲（三段联合冻结）");

    // ── 静态明细（主体） ─────────────────────────────────────
    let golden = golden_run();
    s.add("J1-三段联签黄金全链", golden.is_ok(), "三段各三签三冻，联合指纹归并成功");
    let signs_ok = match &golden {
        Ok((sc, fc, _)) => {
            *sc == (SEG_COUNT * SIGNERS_REQUIRED) as u32 && *fc == SEG_COUNT as u32
        }
        Err(_) => false,
    };
    s.add("J1-会签留痕齐备", signs_ok, "留痕 9 条冻结 3 次，与三段必签推导一致");
    s.add("J1-归并裁决表四例对拍", adjudication_ok(), "三态样本逐例与冻结裁决表一致");
    s.add("J3-状态机单调（越级倒退双拒）", state_machine_probe(), "advance 只放行 +1 步");
    s.add("J4-边界签署方与段号", boundary_probe().is_ok(), "签署方 0/上限-1 合法，段号越界必拒");
    s.add("J4-留痕槽满拒收不丢账", capacity_probe().is_ok(), "满后 +1 签拒且计数守恒");
    s.add("J5-联合未就绪必拒", probe_joint_not_ready().is_ok(), "单段冻结即联合被 JointNotReady 拒");
    s.add("J5-联合幂等必拒", probe_joint_idempotent().is_ok(), "二次联合被 JointDone 拒");
    s.add("J5-归并账缺条必拒", probe_merge_break().is_ok(), "清空归并账后被 LedgerBreak 拒");
    s.add("J7-段位守卫与文案闭集", segment_guard(), "0x80800 段 0x40 窗，14 码全有中文建议");

    // ── 动态明细（≤8 槽，运行期文本） ────────────────────────
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    let (t, c) = match &led {
        Ok(l) => (l.tries[0] + l.tries[1] + l.tries[2], l.caught[0] + l.caught[1] + l.caught[2]),
        Err(_) => (0u32, 0u32),
    };
    s.add_dyn("J2-注入10/10全抓", led_ok, &format!("三类各{}轮 抓{}/{}", JUDGE_ROUNDS, c, t));
    s.add_dyn("J2-注入漏检为零", match &led { Ok(l) => l.missed() == 0, Err(_) => false }, &format!("漏检{}", match &led { Ok(l) => l.missed(), Err(_) => 99u32 }));
    s.add_dyn("J2-注入守恒", t == c, &format!("tries{}=caught{}+missed{}", t, c, t.saturating_sub(c)));

    let rep = repro_check().is_ok();
    s.add_dyn("J6-双跑一致", rep, &format!("golden={:?}", summary()));

    let gok = golden.is_ok();
    s.add_dyn("J1-归并一致断言", gok, &format!("总账={:?}", golden));

    s
}

/// 状态机探针：越级（+2）与倒退（-1）双拒，恰 +1 放行。
pub fn state_machine_probe() -> bool {
    // 越级：Draft → Signed（跳过复核）。
    let skip = SegState::Draft.advance(SegState::Signed)
        == Err(CosignCode::StateSkip.code());
    // 倒退：Frozen → Reviewed。
    let back = SegState::Frozen.advance(SegState::Reviewed)
        == Err(CosignCode::StateBack.code());
    // 同态平移也算倒退。
    let flat = SegState::Signed.advance(SegState::Signed)
        == Err(CosignCode::StateBack.code());
    // 恰 +1 放行。
    let ok = SegState::Draft.advance(SegState::Reviewed).is_ok();
    skip && back && flat && ok
}

// ── 测试区（≥8 个 #[test]，覆盖契约 §4 全形态） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18561_checks();
        assert!(set.all_passed(), "UNX-F18561 判据集必须全绿: {}", set.red_items_text());
        assert!(set.len() >= 8 && set.len() <= 20);
    }

    #[test]
    fn test_golden_run_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (sc, fc, _jf) = g.unwrap_or((0, 0, 0));
        assert_eq!(sc, (SEG_COUNT * SIGNERS_REQUIRED) as u32);
        assert_eq!(fc, SEG_COUNT as u32);
    }

    #[test]
    fn test_state_machine_monotonic() {
        assert!(state_machine_probe());
        // 判定次序：越级优先于倒退语义互斥，两码不同。
        assert_ne!(CosignCode::StateSkip.code(), CosignCode::StateBack.code());
    }

    #[test]
    fn test_dup_signer_rejected() {
        let mut l = CosignLedger::new();
        l.promote(0, SegState::Reviewed).unwrap_or(());
        l.promote(0, SegState::Signed).unwrap_or(());
        assert!(l.sign(0, 0, 7, 1).is_ok());
        assert_eq!(l.sign(0, 0, 7, 2).err(), Some(CosignCode::DupSigner.code()));
    }

    #[test]
    fn test_tamper_caught() {
        let mut l = CosignLedger::new();
        l.promote(0, SegState::Reviewed).unwrap_or(());
        l.promote(0, SegState::Signed).unwrap_or(());
        let mut k = 0;
        while k < SIGNERS_REQUIRED {
            l.sign(0, k as u32, 7, 1).unwrap_or(0);
            k += 1;
        }
        l.segs[0].tamper_first();
        assert_eq!(l.freeze_seg(0, 7).err(), Some(CosignCode::FpMismatch.code()));
    }

    #[test]
    fn test_boundary_and_capacity() {
        assert!(boundary_probe().is_ok());
        assert!(capacity_probe().is_ok());
        // 上限+1：第 SIG_SLOTS+1 签必拒已由 capacity_probe 断言。
    }

    #[test]
    fn test_joint_gates() {
        assert!(probe_joint_not_ready().is_ok());
        assert!(probe_joint_idempotent().is_ok());
        assert!(probe_merge_break().is_ok());
        // 裁决表逐例。
        assert!(adjudication_ok());
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS);
        let l = led.unwrap_or(InjectLedger::new());
        assert!(l.all_caught());
        assert_eq!(l.missed(), 0);
        for i in 0..3 {
            assert_eq!(l.tries[i], JUDGE_ROUNDS);
            assert_eq!(l.caught[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn test_repro_dual_run() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or((0, 0, 0));
        let b = golden_run().unwrap_or((1, 1, 1));
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80800);
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in CosignCode::ALL.iter() {
            assert_eq!(detail(c.code()), c.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }
}
