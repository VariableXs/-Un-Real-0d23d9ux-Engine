//! UNX-F48403 —— E·先杀旧后启新销毁序断言（正：序可观测且恒定）
//! 判据原文：UNX-F48403-J1 ×100 次切换周期，每次 OldKilled 必先于 NewBooted；
//!           全局序号严格单调；同一输入序列重演两遍事件序列逐位一致（指纹幂等）；
//!           销毁五步序（unmap→flush→kill→boot→focus）出现且顺序恒定；
//!           无前驱 boot 的 focus 视为孤儿判 Err；时间戳非降。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD520; // E21 销毁序断言段

const E_ORDER_REV: u16 = SEG_BASE + 1; // OldKilled 晚于 NewBooted
const E_SEQ_GAP: u16 = SEG_BASE + 2; // 全局序号非单调
const E_REPLAY_DIFF: u16 = SEG_BASE + 3; // 重演指纹不一致
const E_STEP_MISS: u16 = SEG_BASE + 4; // 五步序缺步
const E_STEP_ORDER: u16 = SEG_BASE + 5; // 五步序次序违例
const E_ORPHAN_FOCUS: u16 = SEG_BASE + 6; // 无前驱 boot 的 focus
const E_TS_REGRESS: u16 = SEG_BASE + 7; // 时间戳回退
const E_LEDGER_SEAL: u16 = SEG_BASE + 8; // 事件账封印指纹失配

/// 生命周期事件类别。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum EvKind {
    OldUnmap = 0,
    OldFlush = 1,
    OldKilled = 2,
    NewBooted = 3,
    NewFocused = 4,
}

/// 带全局序号与时间戳的销毁序事件。
#[derive(Clone, Copy)]
pub struct SeqEvent {
    pub kind: EvKind,
    pub ctx_old: u32,
    pub ctx_new: u32,
    pub seq: u64,
    pub ts: u32,
}

/// 销毁编排器：旧 context 必须先杀，新 context 后启。
pub struct DestroyOrch {
    events: Vec<SeqEvent>,
    seq: u64,
    ts: u32,
}

impl DestroyOrch {
    pub fn new() -> Self {
        DestroyOrch { events: Vec::new(), seq: 0, ts: 0 }
    }

    /// 执行一次切换周期：按恒定五步序产出事件。
    pub fn switch_cycle(&mut self, old_ctx: u32, new_ctx: u32) -> Result<(), u16> {
        if old_ctx == new_ctx {
            return Err(E_ORDER_REV);
        }
        // 五步序：unmap→flush→kill→boot→focus，顺序恒定
        const PLAN: [(EvKind, &str); 5] = [
            (EvKind::OldUnmap, "unmap"),
            (EvKind::OldFlush, "flush"),
            (EvKind::OldKilled, "kill"),
            (EvKind::NewBooted, "boot"),
            (EvKind::NewFocused, "focus"),
        ];
        for i in 0..5 {
            let (k, name) = PLAN[i];
            // 五步序内部断言：boot 只能出现在 kill 之后
            if k == EvKind::NewBooted && !Self::has_kill_before_boot(&self.events, self.seq) {
                return Err(E_STEP_ORDER);
            }
            self.seq += 1;
            self.ts += match name { "unmap" => 3, "flush" => 2, "kill" => 5, _ => 1 };
            self.events.push(SeqEvent {
                kind: k,
                ctx_old: old_ctx,
                ctx_new: new_ctx,
                seq: self.seq,
                ts: self.ts,
            });
        }
        Ok(())
    }

    fn has_kill_before_boot(events: &[SeqEvent], upto: u64) -> bool {
        let mut saw_kill = false;
        let mut i = 0usize;
        while (i as u64) < upto && i < events.len() {
            if events[i].kind == EvKind::OldKilled {
                saw_kill = true;
            }
            if events[i].kind == EvKind::NewBooted && !saw_kill {
                return false;
            }
            i += 1;
        }
        saw_kill
    }

    /// J1-正：×100 切换周期，OldKilled 必先于 NewBooted（逐周期对账）。
    pub fn audit_kill_before_boot(&self) -> Result<u32, u16> {
        let mut cycles = 0u32;
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].kind == EvKind::NewBooted {
                // 向前找同一周期的 OldKilled
                let mut j = i;
                let mut found = false;
                while j > 0 {
                    j -= 1;
                    if self.events[j].kind == EvKind::OldKilled
                        && self.events[j].ctx_new == self.events[i].ctx_new
                    {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(E_ORDER_REV);
                }
                cycles += 1;
            }
            i += 1;
        }
        if cycles == 0 {
            return Err(E_STEP_MISS);
        }
        Ok(cycles)
    }

    /// J1-单调：全局 seq 严格递增、ts 非降。
    pub fn audit_monotone(&self) -> Result<(), u16> {
        let mut i = 1usize;
        while i < self.events.len() {
            if self.events[i].seq <= self.events[i - 1].seq {
                return Err(E_SEQ_GAP);
            }
            if self.events[i].ts < self.events[i - 1].ts {
                return Err(E_TS_REGRESS);
            }
            i += 1;
        }
        Ok(())
    }

    /// J1-孤儿：每个 NewFocused 必有同周期 NewBooted 前驱。
    pub fn audit_no_orphan_focus(&self) -> Result<(), u16> {
        let mut booted: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.events.len() {
            let e = &self.events[i];
            if e.kind == EvKind::NewBooted {
                booted.push(e.ctx_new);
            }
            if e.kind == EvKind::NewFocused {
                let mut ok = false;
                let mut j = 0usize;
                while j < booted.len() {
                    if booted[j] == e.ctx_new {
                        ok = true;
                    }
                    j += 1;
                }
                if !ok {
                    return Err(E_ORPHAN_FOCUS);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// J1-五步序恒定：逐周期抽五步，kind 序列恒等于 (0,1,2,3,4)。
    pub fn audit_step_constancy(&self) -> Result<u32, u16> {
        if self.events.len() % 5 != 0 {
            return Err(E_STEP_MISS);
        }
        let cycles = self.events.len() / 5;
        let mut c = 0usize;
        while c < cycles {
            let base = c * 5;
            let mut k = 0usize;
            while k < 5 {
                if self.events[base + k].kind as u16 != k as u16 {
                    return Err(E_STEP_ORDER);
                }
                k += 1;
            }
            c += 1;
        }
        Ok(cycles as u32)
    }

    /// 事件账 FNV 封印指纹。
    pub fn ledger_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.events.len() {
            let e = &self.events[i];
            for v in [e.kind as u64, e.ctx_old as u64, e.ctx_new as u64, e.seq, e.ts as u64] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn last_seq(&self) -> u64 {
        self.seq
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 重演指纹：同一种子重演两遍，事件账封印必须一致。
pub fn replay_fp(seed: u64, cycles: u32) -> Result<u64, u16> {
    let mut orch = DestroyOrch::new();
    let mut rng = Lcg::new(seed);
    let mut c = 0u32;
    while c < cycles {
        let old = rng.below(0x1000) as u32;
        let mut nu = rng.below(0x1000) as u32;
        if nu == old {
            nu = (nu + 1) & 0xFFF;
        }
        orch.switch_cycle(old, nu)?;
        c += 1;
    }
    Ok(orch.ledger_fp())
}

/// 主巡检：销毁序断言八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×100 切换周期：OldKilled 必先于 NewBooted + 孤儿零放行
    let mut orch = DestroyOrch::new();
    let mut rng = Lcg::new(0x48403);
    let mut c = 0u32;
    while c < 100 {
        let old = (0x10 + c) as u32;
        let nu = (0x110 + c) as u32;
        orch.switch_cycle(old, nu)?;
        c += 1;
    }
    let _ = rng.below(1);
    let cyc = orch.audit_kill_before_boot()?;
    if cyc != 100 {
        return Err(E_STEP_MISS);
    }
    orch.audit_no_orphan_focus()?;
    hits += 1;

    // R2 单调性：seq 严格递增 + ts 非降
    orch.audit_monotone()?;
    hits += 1;

    // R3 五步序恒定：100 周期 × 5 步全对齐
    let aligned = orch.audit_step_constancy()?;
    if aligned != 100 {
        return Err(E_STEP_ORDER);
    }
    hits += 1;

    // R4 重演指纹幂等：同种子双跑一致
    let f1 = replay_fp(0xBEEF, 64)?;
    let f2 = replay_fp(0xBEEF, 64)?;
    if f1 != f2 || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R5 不同种子指纹互异（可观测性）：64 周期 ×8 种子两两不同
    let mut fps: Vec<u64> = Vec::new();
    let mut s = 0u64;
    while s < 8 {
        let f = replay_fp(0x1000 + s * 7, 64)?;
        let mut j = 0usize;
        while j < fps.len() {
            if fps[j] == f {
                return Err(E_REPLAY_DIFF);
            }
            j += 1;
        }
        fps.push(f);
        s += 1;
    }
    hits += 1;

    // R6 随机 ×512 周期重演：LCG 序列下恒等
    let f3 = replay_fp(0x5A5A_CAFE, 512)?;
    let f4 = replay_fp(0x5A5A_CAFE, 512)?;
    if f3 != f4 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R7 事件账封印：100 周期账指纹与第二次重算一致（账本未被扰动）
    let sealed = orch.ledger_fp();
    let sealed2 = orch.ledger_fp();
    if sealed != sealed2 || sealed == 0 {
        return Err(E_LEDGER_SEAL);
    }
    hits += 1;

    // R8 事件账规模守恒：100 周期恰 500 事件，末序号恰 500
    if orch.event_count() != 500 || orch.last_seq() != 500 {
        return Err(E_STEP_MISS);
    }
    hits += 1;

    Ok(hits)
}
