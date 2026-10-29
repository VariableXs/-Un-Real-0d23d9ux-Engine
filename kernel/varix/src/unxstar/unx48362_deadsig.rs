//! unx48362_deadsig — UNX-F48362 丢键/无反馈指纹捕获（dead key、100ms 无反馈候选标记）（诊断段 0xCE20）。
//!
//! 判据原文：
//!   J1 注入 ×10 全捕获标结论；判据锚定 体验十三 挫败指纹。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. dead key 语义：dead key 按下进入组合等待态（等待基字组合）；
//!      100ms 内组合键到达 → 组合成功（出合体字指纹）；100ms 到点
//!      无后续 → 悬挂超时（捕获指纹并标结论，死键不静默吞——用
//!      户按了没反应就是挫败，必须留痕可查）。
//!   2. 无反馈候选标记：候选请求发出后 100ms 内无候选返回 → 标
//!      记"候选无反馈"指纹（请求 tick + 等待时长），无反馈不是
//!      空结果，是观测事故。
//!   3. 捕获口径（J1 ×10）：10 个注入场景（组合成功/悬挂超时/显
//!      式取消/候选无反馈四类结论）逐场景跑，每场景必产出指纹并
//!      标结论——捕获率 100%（每注入必有账），结论账四分立。
//!   4. 时间确定性：tick 单调推进，100ms 判定闭区间口径（≥100
//!      即超时），无真实时钟。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCE20;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_NO_CAPTURE: u16 = SEG_BASE + 1;  // 注入未捕获（捕获率红线）
const E_STATE_BAD: u16 = SEG_BASE + 2;   // 组合态机非法迁移
const E_TICK_BAD: u16 = SEG_BASE + 3;    // tick 回退
const E_FP_BAD: u16 = SEG_BASE + 4;      // 指纹不符
const E_LEDGER_BAD: u16 = SEG_BASE + 5;  // 结论账不平
const E_REPLAY_DIFF: u16 = SEG_BASE + 6; // 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// 无反馈阈值（ms，章节口径）。
pub const NO_FEEDBACK_MS: u32 = 100;

/// 结论四分立。
pub const CC_COMPOSE: u8 = 0;  // 组合成功
pub const CC_HANG: u8 = 1;     // dead key 悬挂超时
pub const CC_CANCEL: u8 = 2;   // 显式取消
pub const CC_NOCAND: u8 = 3;   // 候选无反馈
pub const CC_N: usize = 4;
pub const CC_NAMES: [&str; CC_N] = ["组合成功", "悬挂超时", "显式取消", "候选无反馈"];

/// 捕获指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeadSig {
    pub conclusion: u8,   // 结论四分立
    pub key: u8,          // 触发键（dead key 或候选请求键）
    pub t_request: u32,   // 请求时刻
    pub t_settle: u32,    // 结算时刻（超时点/组合点/取消点）
    pub fp: u64,          // 指纹（可复算）
}

impl DeadSig {
    pub fn verify(&self) -> Result<(), u16> {
        if self.conclusion as usize >= CC_N {
            return Err(E_STATE_BAD);
        }
        if self.t_settle < self.t_request {
            return Err(E_TICK_BAD);
        }
        if self.fp != sig_fp(self.conclusion, self.key, self.t_request, self.t_settle) {
            return Err(E_FP_BAD);
        }
        Ok(())
    }
}

/// 指纹（结论/键/双时刻四元折叠）。
pub fn sig_fp(conclusion: u8, key: u8, t_request: u32, t_settle: u32) -> u64 {
    let mut f: u64 = 0xCE20_CE21 ^ (conclusion as u64);
    f = f.rotate_left(8) ^ (key as u64);
    f = f.rotate_left(8) ^ (t_request as u64);
    f = f.rotate_left(8) ^ (t_settle as u64);
    f.rotate_left(8)
}

/// 捕获面：dead key 组合态机 + 候选无反馈标记。
pub struct DeadCap {
    wait_key: Option<(u8, u32)>, // (dead key, 请求 tick)
    pub sigs: Vec<DeadSig>,
    pub cc_count: [u32; CC_N],
    pub inject_total: u32,
    pub reject_total: u32,
    last_tick: u32,
}

impl DeadCap {
    pub fn new() -> DeadCap {
        DeadCap {
            wait_key: None,
            sigs: Vec::new(),
            cc_count: [0; CC_N],
            inject_total: 0,
            reject_total: 0,
            last_tick: 0,
        }
    }

    fn tick_ok(&mut self, t: u32) -> Result<(), u16> {
        if t < self.last_tick {
            self.reject_total += 1;
            return Err(E_TICK_BAD);
        }
        self.last_tick = t;
        Ok(())
    }

    /// dead key 按下：进入组合等待。
    pub fn dead_down(&mut self, key: u8, t: u32) -> Result<(), u16> {
        self.tick_ok(t)?;
        self.inject_total += 1;
        if self.wait_key.is_some() {
            // 已在等待态再来 dead key：前一个悬挂结算，新的接管。
            let (k, t0) = self.wait_key.take().unwrap_or((0, 0));
            self.settle(CC_HANG, k, t0, t)?;
        }
        self.wait_key = Some((key, t));
        Ok(())
    }

    /// 组合键到达：等待态则组合成功；100ms 判定由调用方时刻驱动。
    pub fn compose(&mut self, base: u8, t: u32) -> Result<u64, u16> {
        self.tick_ok(t)?;
        self.inject_total += 1;
        match self.wait_key.take() {
            None => {
                self.reject_total += 1;
                Err(E_STATE_BAD)
            }
            Some((k, t0)) => {
                if t.saturating_sub(t0) >= NO_FEEDBACK_MS {
                    // 组合到得太晚：先挂悬挂结论，组合另行请求。
                    self.settle(CC_HANG, k, t0, t0 + NO_FEEDBACK_MS)?;
                    self.wait_key = Some((base, t));
                    Ok(self.sigs[self.sigs.len() - 1].fp)
                } else {
                    self.settle(CC_COMPOSE, k, t0, t)
                }
            }
        }
    }

    /// 显式取消（Esc 语义）。
    pub fn cancel(&mut self, t: u32) -> Result<u64, u16> {
        self.tick_ok(t)?;
        self.inject_total += 1;
        match self.wait_key.take() {
            None => {
                self.reject_total += 1;
                Err(E_STATE_BAD)
            }
            Some((k, t0)) => self.settle(CC_CANCEL, k, t0, t),
        }
    }

    /// 超时巡检：等待态过 100ms 即悬挂结算（结论自带等待时长）。
    pub fn sweep(&mut self, t: u32) -> Result<u32, u16> {
        self.tick_ok(t)?;
        match self.wait_key {
            Some((k, t0)) if t.saturating_sub(t0) >= NO_FEEDBACK_MS => {
                self.wait_key = None;
                self.settle(CC_HANG, k, t0, t)?;
                Ok(1)
            }
            _ => Ok(0),
        }
    }

    /// 候选请求 + 无反馈判定：发出请求（模拟为带请求时刻的键），
    /// 结算时刻与请求差 ≥100 即候选无反馈结论。
    pub fn cand_settle(&mut self, key: u8, t_request: u32, t_settle: u32) -> Result<u64, u16> {
        self.tick_ok(t_settle)?;
        self.inject_total += 1;
        if t_settle.saturating_sub(t_request) >= NO_FEEDBACK_MS {
            self.settle(CC_NOCAND, key, t_request, t_settle)
        } else {
            // 有反馈及时返回：不产挫败指纹，但注入账照记。
            Ok(0)
        }
    }

    /// 结算：产指纹标结论入账。
    fn settle(&mut self, cc: u8, key: u8, t_request: u32, t_settle: u32) -> Result<u64, u16> {
        let sig = DeadSig {
            conclusion: cc,
            key,
            t_request,
            t_settle,
            fp: sig_fp(cc, key, t_request, t_settle),
        };
        sig.verify()?;
        self.cc_count[cc as usize] += 1;
        let f = sig.fp;
        self.sigs.push(sig);
        Ok(f)
    }

    pub fn sig_n(&self) -> usize {
        self.sigs.len()
    }

    /// 捕获率审计（J1 口径）：注入数 > 0 且全部结算（无悬挂等待
    /// 残留）+ 指纹逐条复核 + 结论账守恒。
    pub fn audit(&self) -> Result<(), u16> {
        if self.wait_key.is_some() {
            return Err(E_NO_CAPTURE); // 有注入未结算即捕获不全
        }
        if self.inject_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        let mut s: u32 = 0;
        let mut c = 0usize;
        while c < CC_N {
            s += self.cc_count[c];
            c += 1;
        }
        if s != self.sigs.len() as u32 {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.sigs.len() {
            self.sigs[i].verify()?;
            i += 1;
        }
        Ok(())
    }

    /// 全账指纹（复跑对照）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0xCE20_CE22;
        let mut i = 0usize;
        while i < self.sigs.len() {
            fp = fp.rotate_left(7) ^ self.sigs[i].fp;
            i += 1;
        }
        fp
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.sigs.is_empty() {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// J1 十注入场景（键, 时刻差, 结论类型）：
/// 0..4 组合成功（差 10..90ms）、5..6 悬挂（sweep 超时）、7 显式取消、
/// 8..9 候选无反馈。
pub const CASES: [(u8, u32, u8); 10] = [
    (0x5E, 10, CC_COMPOSE),  // ^ + e
    (0x60, 30, CC_COMPOSE),  // ` + a
    (0xA8, 50, CC_COMPOSE),  // ¨ + u
    (0xB4, 70, CC_COMPOSE),  // ´ + o
    (0x7E, 90, CC_COMPOSE),  // ~ + n
    (0x5E, 100, CC_HANG),    // ^ 悬挂
    (0x60, 250, CC_HANG),    // ` 长悬
    (0xB4, 40, CC_CANCEL),   // ´ 取消
    (0x31, 100, CC_NOCAND),  // 候选 1 无反馈
    (0x32, 180, CC_NOCAND),  // 候选 2 无反馈
];

/// 自例程判据：逐条落实 J1（注入 ×10 全捕获标结论），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 注入 ×10 全捕获：十场景逐跑，每场景产指纹标结论，捕获
    //    率 100%（注入后无残留等待），结论账 == 10。
    let mut dc = DeadCap::new();
    let mut t = 1000u32;
    let mut i = 0usize;
    while i < CASES.len() {
        let (key, gap, want_cc) = CASES[i];
        match want_cc {
            CC_COMPOSE => {
                dc.dead_down(key, t)?;
                dc.compose(b'e', t + gap)?;
            }
            CC_HANG => {
                dc.dead_down(key, t)?;
                dc.sweep(t + gap)?;
            }
            CC_CANCEL => {
                dc.dead_down(key, t)?;
                dc.cancel(t + gap)?;
            }
            _ => {
                dc.cand_settle(key, t, t + gap)?;
            }
        }
        i += 1;
    }
    if dc.sig_n() != 10 {
        return Err(E_NO_CAPTURE);
    }
    if dc.cc_count[CC_COMPOSE as usize] != 5
        || dc.cc_count[CC_HANG as usize] != 2
        || dc.cc_count[CC_CANCEL as usize] != 1
        || dc.cc_count[CC_NOCAND as usize] != 2
    {
        return Err(E_LEDGER_BAD);
    }
    dc.audit()?;
    hits += 1;

    // R2 指纹逐条可复算：10 条捕获指纹与 sig_fp 直算逐位一致（不
    //    复用 settle 内路径）；时刻差与结论自洽（悬挂 ≥100，组合
    //    <100）。
    let mut dc = DeadCap::new();
    let mut t = 2000u32;
    let mut i = 0usize;
    while i < CASES.len() {
        let (key, gap, want_cc) = CASES[i];
        match want_cc {
            CC_COMPOSE => {
                dc.dead_down(key, t)?;
                dc.compose(b'e', t + gap)?;
            }
            CC_HANG => {
                dc.dead_down(key, t)?;
                dc.sweep(t + gap)?;
            }
            CC_CANCEL => {
                dc.dead_down(key, t)?;
                dc.cancel(t + gap)?;
            }
            _ => {
                dc.cand_settle(key, t, t + gap)?;
            }
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < dc.sigs.len() {
        let s = dc.sigs[i];
        if s.fp != sig_fp(s.conclusion, s.key, s.t_request, s.t_settle) {
            return Err(E_FP_BAD);
        }
        if s.conclusion == CC_HANG
            && s.t_settle.saturating_sub(s.t_request) < NO_FEEDBACK_MS
        {
            return Err(E_STATE_BAD);
        }
        if s.conclusion == CC_COMPOSE
            && s.t_settle.saturating_sub(s.t_request) >= NO_FEEDBACK_MS
        {
            return Err(E_STATE_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R3 组合态机迁移合法性：无等待态 compose/cancel 必拒（E_STATE
    // _BAD）；等待中再来 dead key：旧悬挂结算 + 新接管（两笔账）。
    let mut dc = DeadCap::new();
    if dc.compose(b'e', 100) != Err(E_STATE_BAD) {
        return Err(E_STATE_BAD);
    }
    if dc.cancel(100) != Err(E_STATE_BAD) {
        return Err(E_STATE_BAD);
    }
    dc.dead_down(0x5E, 100)?;
    dc.dead_down(0x60, 130)?; // 旧 ^ 悬挂（差 30 不满 100 → 立即结算为悬挂）
    if dc.sigs.len() != 1 || dc.cc_count[CC_HANG as usize] != 1 {
        return Err(E_LEDGER_BAD);
    }
    dc.cancel(200)?; // 新 ` 显式取消
    if dc.cc_count[CC_CANCEL as usize] != 1 {
        return Err(E_LEDGER_BAD);
    }
    dc.audit()?;
    hits += 1;

    // R4 100ms 边界：恰 99 组合成功、恰 100 悬挂（闭区间口径）。
    let mut dc = DeadCap::new();
    dc.dead_down(0x5E, 100)?;
    dc.compose(b'e', 199)?; // 99ms
    if dc.cc_count[CC_COMPOSE as usize] != 1 {
        return Err(E_STATE_BAD);
    }
    dc.dead_down(0x5E, 300)?;
    dc.sweep(400)?; // 100ms
    if dc.cc_count[CC_HANG as usize] != 1 {
        return Err(E_STATE_BAD);
    }
    dc.audit()?;
    hits += 1;

    // R5 捕获率红线可红：注入后不结算残留等待 → audit 必红（判据
    //    非恒绿——漏捕可检出）。
    let mut dc = DeadCap::new();
    dc.dead_down(0x5E, 100)?;
    if dc.audit() != Err(E_NO_CAPTURE) {
        return Err(E_NO_CAPTURE);
    }
    dc.sweep(250)?;
    dc.audit()?;
    hits += 1;

    // R6 指纹敏感度与 tick 回退：同结论同时刻同键同指纹；键变/
    //    时刻变指纹变；tick 回退拒收留痕。
    let f1 = sig_fp(CC_COMPOSE, 0x5E, 100, 150);
    let f2 = sig_fp(CC_COMPOSE, 0x5E, 100, 150);
    if f1 != f2 {
        return Err(E_REPLAY_DIFF);
    }
    if sig_fp(CC_COMPOSE, 0x5F, 100, 150) == f1
        || sig_fp(CC_COMPOSE, 0x5E, 101, 150) == f1
        || sig_fp(CC_HANG, 0x5E, 100, 150) == f1
    {
        return Err(E_FP_BAD);
    }
    let mut dc = DeadCap::new();
    dc.dead_down(0x5E, 200)?;
    if dc.compose(b'e', 150) != Err(E_TICK_BAD) {
        return Err(E_TICK_BAD);
    }
    if dc.reject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 候选及时反馈不产挫败账：50ms 反馈返回 → 无指纹（非挫败），
    //    注入账照记；无反馈（≥100）才产 CC_NOCAND。
    let mut dc = DeadCap::new();
    dc.cand_settle(0x31, 1000, 1050)?; // 50ms 及时
    if dc.sig_n() != 0 {
        return Err(E_LEDGER_BAD);
    }
    if dc.inject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    dc.cand_settle(0x31, 2000, 2100)?; // 100ms 无反馈
    if dc.sig_n() != 1 || dc.cc_count[CC_NOCAND as usize] != 1 {
        return Err(E_NO_CAPTURE);
    }
    dc.audit()?;
    hits += 1;

    // R8 复跑一致与写回（判据 J2）：十场景第二轮复跑全账指纹逐位
    //    一致（确定性）；写回过。
    let run_all = || -> Result<DeadCap, u16> {
        let mut dc = DeadCap::new();
        let mut t = 1000u32;
        let mut i = 0usize;
        while i < CASES.len() {
            let (key, gap, want_cc) = CASES[i];
            match want_cc {
                CC_COMPOSE => {
                    dc.dead_down(key, t)?;
                    dc.compose(b'e', t + gap)?;
                }
                CC_HANG => {
                    dc.dead_down(key, t)?;
                    dc.sweep(t + gap)?;
                }
                CC_CANCEL => {
                    dc.dead_down(key, t)?;
                    dc.cancel(t + gap)?;
                }
                _ => {
                    dc.cand_settle(key, t, t + gap)?;
                }
            }
            i += 1;
        }
        Ok(dc)
    };
    let d1 = run_all()?;
    let d2 = run_all()?;
    if d1.fingerprint() != d2.fingerprint() {
        return Err(E_REPLAY_DIFF);
    }
    d1.audit()?;
    if d1.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    d1.writeback(true, 0x1E)?;
    hits += 1;

    Ok(hits)
}
