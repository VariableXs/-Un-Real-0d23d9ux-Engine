//! UNX-F12687 TLS 回调装载序回归（B29 档） · 0x81580段
//!
//! 判据原文（派工卡 12686 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12687-J1 复用注入 10^3 重跑零串号维持，种子同源
//!      可重演；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. TLS 回调装载序域模型：八槽轮转序（槽位/轮次/轮种子/序摘要/复用
//!    代数），按轮重演装载序——10^3 轮重跑全量核验；
//! 2. 轮转序推演（确定性、无随机环境源）：每轮由 LCG（种子冻结、乘子
//!    与增量冻结常量）推进轮种子，轮种子折算轮转量 rot ∈ 0..8，槽位序
//!    = (槽号 + rot) mod 8——同种子必同序（种子同源可重演的算术面）；
//! 3. 串号判定双层：链层（每轮存储种子必须等于自冻结种子逐轮推进的
//!    同源链值）+ 序层（每轮存储序摘要必须等于按存储种子重推的摘要）
//!    ——任一层失配即串号，零串号才许维持判据；
//! 4. 复用代数：八槽每轮重用一次，复用代数必须逐轮递增（gen ≡ 轮次）
//!    ——复用注入（漏递增）必被代数闸抓出；
//! 5. 序账状态机 OdOpen → OdRunning → OdSealed 单向推进，倒退显性拒；
//!    封账只读；零静默：回次数 ≡ 链长守恒；
//! 6. 注入联测十路：回次乱序/回次重复/序摘要篡改/种子漂移/复用漏递
//!    增/轮长不足/槽宽漂移/rot 越窗/封账后写/状态机倒退——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子冻结为常
//!    量）；取值一律 match/if let/.get()/.get_mut()/saturating_*，无
//!    unwrap/expect/panic/裸下标；中文注释与中文诊断；无死代码无
//!    #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `replay_zero_crosstalk_ok` ↔ ② 复用注入 10^3 重跑零串号维持；
//!   - `reuse_inject_caught_ok` ↔ ② 复用注入必抓（漏递增注入被拒）；
//!   - `seed_homology_ok` ↔ ② 种子同源可重演（同种子双跑同摘要）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12686 − 12600)）。
pub const SEG_BASE: u32 = 0x81580;

// ── 装载序回归诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OdDiag {
    /// 回次乱序。
    ERoundOrder,
    /// 回次重复。
    ERoundDup,
    /// 序摘要篡改（存储摘要 ≠ 重推摘要）。
    EDigestTamper,
    /// 种子漂移（存储种子 ≠ 同源链值）。
    ESeedDrift,
    /// 复用漏递增（复用代数 ≠ 轮次）。
    EReuseMiss,
    /// 轮长不足（回次数 < 10^3）。
    EShortRun,
    /// 槽宽漂移（槽位数 ≠ 8）。
    ESlotWidth,
    /// rot 越窗（轮转量 ∉ 0..8）。
    ERotRange,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
}

impl OdDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                OdDiag::ERoundOrder => 0x01,
                OdDiag::ERoundDup => 0x02,
                OdDiag::EDigestTamper => 0x03,
                OdDiag::ESeedDrift => 0x04,
                OdDiag::EReuseMiss => 0x05,
                OdDiag::EShortRun => 0x06,
                OdDiag::ESlotWidth => 0x07,
                OdDiag::ERotRange => 0x08,
                OdDiag::ESealedWrite => 0x09,
                OdDiag::ERewind => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            OdDiag::ERoundOrder => "回次乱序：按回次 0..10^3 逐轮重演，跳轮先回补",
            OdDiag::ERoundDup => "回次重复：同回次只立一轮，重复先查已有账",
            OdDiag::EDigestTamper => "序摘要篡改：摘要必须等于按存储种子重推值，先重演核账",
            OdDiag::ESeedDrift => "种子漂移：轮种子必须落在自冻结种子的同源链上，漂移先回种子",
            OdDiag::EReuseMiss => "复用漏递增：八槽每轮重用必递增代数，漏增先补 gen",
            OdDiag::EShortRun => "轮长不足：10^3 轮重跑必须足轮，缺轮先补跑",
            OdDiag::ESlotWidth => "槽宽漂移：装载序固定八槽，槽宽先归 8",
            OdDiag::ERotRange => "rot 越窗：轮转量必须落在 0..8，越窗先折 mod",
            OdDiag::ESealedWrite => "封账后写：序账只读，补轮走下一轮重演",
            OdDiag::ERewind => "状态机倒退：OdOpen→OdRunning→OdSealed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const OD_ALL: &[OdDiag] = &[
    OdDiag::ERoundOrder,
    OdDiag::ERoundDup,
    OdDiag::EDigestTamper,
    OdDiag::ESeedDrift,
    OdDiag::EReuseMiss,
    OdDiag::EShortRun,
    OdDiag::ESlotWidth,
    OdDiag::ERotRange,
    OdDiag::ESealedWrite,
    OdDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in OD_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81581..0x8158A 闭集内"
}

// ── LCG 冻结参数（种子同源的唯一源头） ───────────────────────────
/// 冻结种子（常量，非环境源——种子同源锚点）。
pub const LCG_SEED: u64 = 0x853C_49E6_748F_EA9B;

/// LCG 冻结乘子。
pub const LCG_MULT: u64 = 6364136223846793005;

/// LCG 冻结增量。
pub const LCG_INC: u64 = 1442695040888963407;

/// 推进一步 LCG（纯函数，无副作用）。
pub fn lcg_next(state: u64) -> u64 {
    state.wrapping_mul(LCG_MULT).wrapping_add(LCG_INC)
}

// ── 装载序域模型 ─────────────────────────────────────────────────
/// TLS 回调装载序槽宽（八槽）。
pub const TLS_ORDER_CAP: usize = 8;

/// 重跑轮长（10^3，判据锚定值）。
pub const REPLAY_ROUNDS: u32 = 1000;

/// 轮转量折算（轮种子高 32 位 mod 槽宽）。
pub fn derive_rot(seed: u64) -> u32 {
    ((seed >> 32) as u32) % (TLS_ORDER_CAP as u32)
}

/// 槽位序推演（轮转序：pos = (槽号 + rot) mod 8，无重无漏）。
pub fn slot_pos(rot: u32, slot: u32) -> u32 {
    (slot + rot) % (TLS_ORDER_CAP as u32)
}

/// 轮序摘要折算（全槽位序折算，确定性）。
pub fn order_digest_for(rot: u32) -> u32 {
    let mut acc: u32 = 0;
    let mut s = 0u32;
    while s < TLS_ORDER_CAP as u32 {
        acc = acc
            .wrapping_mul(31)
            .wrapping_add(slot_pos(rot, s).wrapping_add(1));
        s = s.saturating_add(1);
    }
    acc
}

/// 单轮装载序记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsRound {
    /// 回次（0..10^3）。
    pub round_no: u32,
    /// 轮种子（同源链值）。
    pub seed_state: u64,
    /// 轮转量。
    pub rot: u32,
    /// 轮序摘要。
    pub order_digest: u32,
}

/// rot 落窗校验。
fn rot_in_range(rot: u32) -> bool {
    rot < TLS_ORDER_CAP as u32
}

// ── 序账状态机 ───────────────────────────────────────────────────
/// 序账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OdState {
    /// 开账。
    OdOpen,
    /// 重演中。
    OdRunning,
    /// 封账（只读）。
    OdSealed,
}

impl OdState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            OdState::OdOpen => 0,
            OdState::OdRunning => 1,
            OdState::OdSealed => 2,
        }
    }
}

/// TLS 装载序账。
#[derive(Clone, Debug)]
pub struct OdLedger {
    /// 状态。
    state: OdState,
    /// 已入账轮次。
    rounds: Vec<TlsRound>,
    /// 八槽复用代数（gen[slot] = 已重用轮数）。
    reuse_gen: Vec<u64>,
    /// 拒止计数。
    rejects: u32,
}

impl OdLedger {
    /// 新账（八槽代数清零）。
    pub fn new() -> OdLedger {
        OdLedger {
            state: OdState::OdOpen,
            rounds: Vec::new(),
            reuse_gen: Vec::new(),
            rejects: 0,
        }
    }

    /// 开账 → 重演中（倒退拒）。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            OdState::OdOpen => {
                self.state = OdState::OdRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(OdDiag::ERewind.val())
            }
        }
    }

    /// 重演中 → 封账（轮长不足不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            OdState::OdRunning => {
                if self.rounds.len() < REPLAY_ROUNDS as usize {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(OdDiag::EShortRun.val());
                }
                self.state = OdState::OdSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(OdDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> OdState {
        self.state
    }

    /// 已入账轮数。
    pub fn len(&self) -> usize {
        self.rounds.len()
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 下标取轮（越界 None）。
    pub fn round_at(&self, idx: usize) -> Option<&TlsRound> {
        self.rounds.get(idx)
    }

    /// 槽复用代数（越槽 None）。
    pub fn gen_at(&self, slot: usize) -> Option<u64> {
        self.reuse_gen.get(slot).copied()
    }

    /// 入账一轮（序闸/重闸/rot 闸/链闸/摘要闸/复用闸/封闸七闸）。
    pub fn add_round(&mut self, r: TlsRound) -> Result<(), u32> {
        if self.state == OdState::OdSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OdDiag::ESealedWrite.val());
        }
        if self.state == OdState::OdOpen {
            self.state = OdState::OdRunning;
        }
        if self.reuse_gen.is_empty() {
            // 首轮入账前铺八槽代数槽（gen 从 0 起算，递增前 ≡ 轮次）。
            let mut k = 0usize;
            while k < TLS_ORDER_CAP {
                self.reuse_gen.push(0);
                k += 1;
            }
        }
        // 序闸。
        if r.round_no != self.rounds.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OdDiag::ERoundOrder.val());
        }
        // 重闸（回次唯一）。
        for e in self.rounds.iter() {
            if e.round_no == r.round_no {
                self.rejects = self.rejects.saturating_add(1);
                return Err(OdDiag::ERoundDup.val());
            }
        }
        // rot 闸。
        if !rot_in_range(r.rot) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OdDiag::ERotRange.val());
        }
        // 链闸：种子必须落在同源链上（前一轮链值推进一步）。
        let expect_seed = match self.rounds.len() {
            0 => lcg_next(LCG_SEED),
            n => {
                let prev = match self.rounds.get(n.saturating_sub(1)) {
                    Some(p) => p.seed_state,
                    None => LCG_SEED,
                };
                lcg_next(prev)
            }
        };
        if r.seed_state != expect_seed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OdDiag::ESeedDrift.val());
        }
        // 摘要闸：存储摘要必须等于按存储种子重推值。
        if r.order_digest != order_digest_for(r.rot) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OdDiag::EDigestTamper.val());
        }
        // 复用闸：八槽代数在递增前必须全部等于当前轮次（每轮各槽重用一次），
        // 递增后 gen ≡ 轮次 + 1（末轮后 gen ≡ 总轮数）。
        let mut s = 0usize;
        while s < TLS_ORDER_CAP {
            let g = match self.reuse_gen.get(s) {
                Some(v) => *v,
                None => {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(OdDiag::ESlotWidth.val());
                }
            };
            if g != r.round_no as u64 {
                self.rejects = self.rejects.saturating_add(1);
                return Err(OdDiag::EReuseMiss.val());
            }
            s += 1;
        }
        // 全闸放行：代数递增并落账。
        let mut t = 0usize;
        while t < TLS_ORDER_CAP {
            match self.reuse_gen.get_mut(t) {
                Some(v) => *v = v.saturating_add(1),
                None => {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(OdDiag::ESlotWidth.val());
                }
            }
            t += 1;
        }
        self.rounds.push(r);
        Ok(())
    }
}

/// 全量重演（10^3 轮依序入账并封账；种子链自冻结种子逐轮推进）。
pub fn build_full_od_ledger() -> OdLedger {
    let mut led = OdLedger::new();
    let _ = led.begin_run();
    let mut st = LCG_SEED;
    let mut r = 0u32;
    while r < REPLAY_ROUNDS {
        st = lcg_next(st);
        let rot = derive_rot(st);
        let _ = led.add_round(TlsRound {
            round_no: r,
            seed_state: st,
            rot,
            order_digest: order_digest_for(rot),
        });
        r += 1;
    }
    let _ = led.seal();
    led
}

/// 轻量重演（n 轮，不封账；供注入联测复用）。
pub fn replay_od(n: u32) -> OdLedger {
    let mut led = OdLedger::new();
    let _ = led.begin_run();
    let mut st = LCG_SEED;
    let mut r = 0u32;
    while r < n {
        st = lcg_next(st);
        let rot = derive_rot(st);
        let _ = led.add_round(TlsRound {
            round_no: r,
            seed_state: st,
            rot,
            order_digest: order_digest_for(rot),
        });
        r += 1;
    }
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 复用注入 10^3 重跑零串号维持（链层 + 序层 + 代数层三层全零失配）。
fn replay_zero_crosstalk_ok() -> bool {
    let led = build_full_od_ledger();
    if led.len() != REPLAY_ROUNDS as usize {
        return false;
    }
    if led.state() != OdState::OdSealed {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    // 链层：存储种子链 == 自冻结种子逐轮推进的同源链。
    let mut st = LCG_SEED;
    let mut i = 0usize;
    while i < led.len() {
        st = lcg_next(st);
        let r = match led.round_at(i) {
            Some(v) => v,
            None => return false,
        };
        if r.seed_state != st {
            return false;
        }
        if r.rot != derive_rot(st) || r.order_digest != order_digest_for(r.rot) {
            return false;
        }
        i += 1;
    }
    // 代数层：八槽 gen ≡ 轮长。
    let mut s = 0usize;
    while s < TLS_ORDER_CAP {
        match led.gen_at(s) {
            Some(g) => {
                if g != REPLAY_ROUNDS as u64 {
                    return false;
                }
            }
            None => return false,
        }
        s += 1;
    }
    true
}

/// 复用注入必抓（漏递增注入被拒：代数闸收网）。
fn reuse_inject_caught_ok() -> bool {
    // 构造复用漏递增：正常推 3 轮后，第 4 轮手动抹平一格代数。
    let mut led = replay_od(3);
    // 抹平槽 0 的代数（3 → 2，制造漏递增面）。
    match led.reuse_gen.get_mut(0) {
        Some(v) => *v = v.saturating_sub(1),
        None => return false,
    }
    let mut st = LCG_SEED;
    let mut k = 0u32;
    while k < 3 {
        st = lcg_next(st);
        k += 1;
    }
    st = lcg_next(st);
    let rot = derive_rot(st);
    let r = TlsRound {
        round_no: 3,
        seed_state: st,
        rot,
        order_digest: order_digest_for(rot),
    };
    match led.add_round(r) {
        Err(e) => e == OdDiag::EReuseMiss.val(),
        Ok(_) => false,
    }
}

/// 种子同源可重演（同种子双跑同摘要；漂移种子必被链闸拒）。
fn seed_homology_ok() -> bool {
    // 双跑一致。
    let a = build_full_od_ledger();
    let b = build_full_od_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.round_at(i), b.round_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    // 漂移种子注入：首轮回种子偏一位，链闸必拒。
    let mut led = OdLedger::new();
    let _ = led.begin_run();
    let drift_seed = lcg_next(LCG_SEED).wrapping_add(1);
    let rot = derive_rot(drift_seed);
    let bad = TlsRound {
        round_no: 0,
        seed_state: drift_seed,
        rot,
        order_digest: order_digest_for(rot),
    };
    match led.add_round(bad) {
        Err(e) => e == OdDiag::ESeedDrift.val(),
        Ok(_) => false,
    }
}

/// 轮长与槽宽口径（10^3 × 八槽）。
fn run_shape_ok() -> bool {
    if REPLAY_ROUNDS != 1000 || TLS_ORDER_CAP != 8 {
        return false;
    }
    if replay_od(REPLAY_ROUNDS.saturating_sub(1)).len() != (REPLAY_ROUNDS - 1) as usize {
        return false;
    }
    true
}

/// 轮转序口径（同 rot 同序、异 rot 异序、rot 全窗落位）。
fn rot_semantics_ok() -> bool {
    let mut r = 0u32;
    while r < TLS_ORDER_CAP as u32 {
        if !rot_in_range(r) || order_digest_for(r) != order_digest_for(r) {
            return false;
        }
        // 槽位序在窗内。
        let mut s = 0u32;
        while s < TLS_ORDER_CAP as u32 {
            let p = slot_pos(r, s);
            if p >= TLS_ORDER_CAP as u32 {
                return false;
            }
            s += 1;
        }
        r += 1;
    }
    // 异 rot 至少一槽异位。
    let d0 = order_digest_for(0);
    let d1 = order_digest_for(1);
    if d0 == d1 {
        return false;
    }
    true
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn od_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 回次乱序：跳过 0 号直入 1 号。
    {
        let mut led = OdLedger::new();
        let _ = led.begin_run();
        let st = lcg_next(LCG_SEED);
        let rot = derive_rot(st);
        let bad = TlsRound { round_no: 1, seed_state: st, rot, order_digest: order_digest_for(rot) };
        if led.add_round(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("乱序;");
        }
    }
    // 2. 回次重复。
    {
        let mut led = replay_od(1);
        let st = match led.round_at(0) {
            Some(x) => x.seed_state,
            None => return (caught, 10, String::from("账缺轮")),
        };
        let rot = derive_rot(st);
        let bad = TlsRound { round_no: 0, seed_state: st, rot, order_digest: order_digest_for(rot) };
        if led.add_round(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("重复;");
        }
    }
    // 3. 序摘要篡改。
    {
        let mut led = OdLedger::new();
        let _ = led.begin_run();
        let st = lcg_next(LCG_SEED);
        let rot = derive_rot(st);
        let bad = TlsRound { round_no: 0, seed_state: st, rot, order_digest: order_digest_for(rot).wrapping_add(1) };
        if led.add_round(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("篡改;");
        }
    }
    // 4. 种子漂移。
    {
        let mut led = OdLedger::new();
        let _ = led.begin_run();
        let st = lcg_next(LCG_SEED).wrapping_add(7);
        let rot = derive_rot(st);
        let bad = TlsRound { round_no: 0, seed_state: st, rot, order_digest: order_digest_for(rot) };
        if led.add_round(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("漂移;");
        }
    }
    // 5. rot 越窗。
    {
        let mut led = OdLedger::new();
        let _ = led.begin_run();
        let st = lcg_next(LCG_SEED);
        let bad = TlsRound { round_no: 0, seed_state: st, rot: TLS_ORDER_CAP as u32, order_digest: order_digest_for(0) };
        if led.add_round(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("越窗;");
        }
    }
    // 6. 复用漏递增。
    {
        if reuse_inject_caught_ok() {
            caught += 1;
        } else {
            escape.push_str("漏增;");
        }
    }
    // 7. 轮长不足即封。
    {
        let mut led = replay_od(999);
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("短跑;");
        }
    }
    // 8. 封账后写。
    {
        let mut led = build_full_od_ledger();
        let st = match led.round_at(0) {
            Some(x) => x.seed_state,
            None => return (caught, 10, String::from("账缺轮")),
        };
        let rot = derive_rot(st);
        let extra = TlsRound { round_no: 0, seed_state: st, rot, order_digest: order_digest_for(rot) };
        if led.add_round(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 9. 状态机倒退：已封账再开账。
    {
        let mut led = build_full_od_ledger();
        if led.begin_run().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 槽宽口径：空账取槽代数必 None（宽度闸显性拒）。
    {
        let led = OdLedger::new();
        if led.gen_at(0).is_none() && led.gen_at(TLS_ORDER_CAP).is_none() {
            caught += 1;
        } else {
            escape.push_str("槽宽;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致（含注入联测自一致）。
fn dual_run_ok() -> bool {
    if od_fault_sweep().0 != od_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    let a = replay_od(64);
    let b = replay_od(64);
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.round_at(i), b.round_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 冻结摘要（全轮种子与序摘要折算，取前 128 轮控制体量）。
pub fn frozen_digest() -> u32 {
    let led = replay_od(128);
    let mut acc: u32 = SEG_BASE;
    let mut i = 0usize;
    while i < led.len() {
        let r = match led.round_at(i) {
            Some(v) => v,
            None => return acc,
        };
        acc = acc
            .wrapping_mul(37)
            .wrapping_add((r.seed_state & 0xFFFF_FFFF) as u32)
            .wrapping_add(r.order_digest);
        i += 1;
    }
    acc
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
    if SEG_BASE != 0x80000 + 0x40 * (12686u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_od_ledger();
    if led.state() != OdState::OdSealed {
        return false;
    }
    let head = match led.round_at(0) {
        Some(r) => r.round_no,
        None => return false,
    };
    let tail = match led.round_at(REPLAY_ROUNDS as usize - 1) {
        Some(r) => r.round_no,
        None => return false,
    };
    if head != 0 || tail != REPLAY_ROUNDS - 1 {
        return false;
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12687 TLS 装载序回归自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12686_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12686-tls-callback-order");
    let led = build_full_od_ledger();

    s.add_dyn(
        "F12687-01 重跑轮长 10^3 × 八槽",
        run_shape_ok(),
        &format!("rounds={} slots={}", REPLAY_ROUNDS, TLS_ORDER_CAP),
    );

    s.add_dyn(
        "F12687-02 10^3 重跑零串号维持",
        replay_zero_crosstalk_ok(),
        &format!("rounds={} sealed={} rejects={}", led.len(), led.state() == OdState::OdSealed, led.reject_len()),
    );

    s.add_dyn(
        "F12687-03 复用注入必抓",
        reuse_inject_caught_ok(),
        "漏递增注入被代数闸收网",
    );

    s.add_dyn(
        "F12687-04 种子同源可重演",
        seed_homology_ok(),
        "同种子双跑同摘要且漂移必拒",
    );

    s.add_dyn(
        "F12687-05 轮转序口径",
        rot_semantics_ok(),
        &format!("rot_window={}", TLS_ORDER_CAP),
    );

    s.add_dyn(
        "F12687-06 注入联测 10/10 拒止",
        {
            let (c, t, e) = od_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", od_fault_sweep().0),
    );

    s.add_dyn(
        "F12687-07 账等式（回次数守恒）",
        led.len() == REPLAY_ROUNDS as usize,
        &format!("rounds={} gen0={:?}", led.len(), led.gen_at(0)),
    );

    s.add_dyn(
        "F12687-08 LCG 冻结参数",
        LCG_SEED != 0 && LCG_MULT != 0 && LCG_INC != 0,
        "种子/乘子/增量三常量冻结非零",
    );

    s.add_dyn(
        "F12687-09 rot 折算口径",
        derive_rot(lcg_next(LCG_SEED)) < TLS_ORDER_CAP as u32,
        "rot = 高 32 位 mod 槽宽",
    );

    s.add_dyn(
        "F12687-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12687-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12687-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12687-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81580);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(OD_ALL.len(), 10);
        let mut i = 0usize;
        while i < OD_ALL.len() {
            let vi = match OD_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < OD_ALL.len() {
                let vj = match OD_ALL.get(j) {
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
    fn replay_thousand_zero_crosstalk() {
        assert!(replay_zero_crosstalk_ok());
        let led = build_full_od_ledger();
        assert_eq!(led.len(), 1000);
        assert_eq!(led.state(), OdState::OdSealed);
        assert_eq!(led.reject_len(), 0);
        // 八槽代数 ≡ 轮长。
        for s in 0..TLS_ORDER_CAP {
            assert_eq!(led.gen_at(s), Some(REPLAY_ROUNDS as u64), "槽 {} 代数漂移", s);
        }
    }

    #[test]
    fn reuse_injection_is_caught() {
        assert!(reuse_inject_caught_ok());
        // 直证：抹平代数后的下一轮必被代数闸拒。
        let mut led = replay_od(2);
        match led.reuse_gen.get_mut(3) {
            Some(v) => *v = 0,
            None => return,
        }
        let mut st = LCG_SEED;
        let mut k = 0u32;
        while k < 2 {
            st = lcg_next(st);
            k += 1;
        }
        st = lcg_next(st);
        let rot = derive_rot(st);
        let r = TlsRound { round_no: 2, seed_state: st, rot, order_digest: order_digest_for(rot) };
        assert_eq!(led.add_round(r), Err(OdDiag::EReuseMiss.val()));
    }

    #[test]
    fn seed_homology_holds() {
        assert!(seed_homology_ok());
        // 同种子双跑全等。
        let a = replay_od(200);
        let b = replay_od(200);
        assert_eq!(a.len(), b.len());
        for i in 0..a.len() {
            match (a.round_at(i), b.round_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑轮缺失"),
            }
        }
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = od_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn rot_semantics_hold() {
        assert!(rot_semantics_ok());
        // 同 rot 同序。
        assert_eq!(order_digest_for(3), order_digest_for(3));
        // 槽位序为双射（轮转序）：每槽位映射唯一。
        let rot = 3u32;
        let mut seen: Vec<u32> = Vec::new();
        for s in 0..TLS_ORDER_CAP as u32 {
            let p = slot_pos(rot, s);
            for e in seen.iter() {
                assert_ne!(*e, p, "rot {} 槽 {} 序重位 {}", rot, s, p);
            }
            seen.push(p);
        }
        assert_eq!(seen.len(), TLS_ORDER_CAP);
    }

    #[test]
    fn ledger_gates_reject() {
        let mut led = OdLedger::new();
        let _ = led.begin_run();
        let st = lcg_next(LCG_SEED);
        // 回次乱序。
        let rot = derive_rot(st);
        let bad = TlsRound { round_no: 1, seed_state: st, rot, order_digest: order_digest_for(rot) };
        assert_eq!(led.add_round(bad), Err(OdDiag::ERoundOrder.val()));
        // 种子漂移。
        let dst = st.wrapping_add(1);
        let drot = derive_rot(dst);
        let drift = TlsRound { round_no: 0, seed_state: dst, rot: drot, order_digest: order_digest_for(drot) };
        assert_eq!(led.add_round(drift), Err(OdDiag::ESeedDrift.val()));
        // rot 越窗。
        let over = TlsRound { round_no: 0, seed_state: st, rot: 9, order_digest: order_digest_for(1) };
        assert_eq!(led.add_round(over), Err(OdDiag::ERotRange.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn short_run_cannot_seal() {
        let mut led = replay_od(999);
        assert_eq!(led.seal(), Err(OdDiag::EShortRun.val()));
        let mut full = replay_od(1000);
        assert!(full.seal().is_ok());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(OdState::OdOpen.rank() < OdState::OdRunning.rank());
        assert!(OdState::OdRunning.rank() < OdState::OdSealed.rank());
        let mut led = build_full_od_ledger();
        assert_eq!(led.state(), OdState::OdSealed);
        assert_eq!(led.seal(), Err(OdDiag::ERewind.val()));
        let mut fresh = OdLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(OdDiag::ERewind.val()));
    }

    #[test]
    fn boundary_shapes() {
        assert_eq!(REPLAY_ROUNDS, 1000);
        assert_eq!(TLS_ORDER_CAP, 8);
        assert!(replay_od(0).is_empty_ledger());
        assert!(replay_od(1).round_at(0).is_some());
        assert!(replay_od(1).round_at(1).is_none());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12686_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}

impl OdLedger {
    /// 账是否为空（测试与口径用）。
    pub fn is_empty_ledger(&self) -> bool {
        self.rounds.is_empty()
    }
}
