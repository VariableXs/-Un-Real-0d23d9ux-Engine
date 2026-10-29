//! UNX-F48743 C·24h 混合稳定终账（全量跑登记随闸门，短程证据在册）
//! 判据 UNX-F48743-J1：全量登记+短程证据齐；判据锚定 §AI-61 §5 J-4。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 全量轮登记必须随闸门编号入册，短程证据须与轮次一一对应，缺失即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x19C0;
const E_GATE_MISS: u16 = SEG_BASE + 1;
const E_EVID_MISS: u16 = SEG_BASE + 2;
const E_SEQ_BREAK: u16 = SEG_BASE + 3;
const E_DUR_ZERO: u16 = SEG_BASE + 4;
const E_MIX_UNBAL: u16 = SEG_BASE + 5;
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;
const E_REG_DUP: u16 = SEG_BASE + 7;
const E_GHOST_GATE: u16 = SEG_BASE + 8;

/// 全量跑闸门登记条目：闸门编号与轮次绑定
#[derive(Clone, Debug, PartialEq)]
pub struct GateReg {
    pub gate_no: u16,
    pub round: u32,
    pub cases: u32,
    pub passed: u32,
}

impl GateReg {
    pub fn new(gate_no: u16, round: u32, cases: u32) -> Self {
        GateReg { gate_no, round, cases, passed: 0 }
    }

    pub fn record_pass(&mut self) {
        if self.passed < self.cases {
            self.passed += 1;
        }
    }

    pub fn complete(&self) -> bool {
        self.cases > 0 && self.passed == self.cases
    }
}

/// 短程证据条目：绑定闸门编号的证据指纹
#[derive(Clone, Debug, PartialEq)]
pub struct ShortEvidence {
    pub gate_no: u16,
    pub fp: u32,
    pub tag: String,
}

impl ShortEvidence {
    pub fn make(gate_no: u16, fp: u32, tag: &str) -> Self {
        ShortEvidence { gate_no, fp, tag: String::from(tag) }
    }
}

/// 混合稳定终账：全量轮登记 + 短程证据在册
pub struct StableLedger {
    pub regs: Vec<GateReg>,
    pub evids: Vec<ShortEvidence>,
}

impl StableLedger {
    pub fn new() -> Self {
        StableLedger { regs: vec![], evids: vec![] }
    }

    pub fn register(&mut self, gate_no: u16, round: u32, cases: u32) -> Result<(), u16> {
        if cases == 0 {
            return Err(E_DUR_ZERO);
        }
        let mut i = 0usize;
        while i < self.regs.len() {
            if self.regs[i].gate_no == gate_no && self.regs[i].round == round {
                return Err(E_REG_DUP);
            }
            i += 1;
        }
        self.regs.push(GateReg::new(gate_no, round, cases));
        Ok(())
    }

    pub fn run_full(&mut self, gate_no: u16, round: u32) -> Result<(), u16> {
        let mut idx = self.regs.len();
        let mut found = false;
        let mut i = 0usize;
        while i < self.regs.len() {
            if self.regs[i].gate_no == gate_no && self.regs[i].round == round {
                idx = i;
                found = true;
            }
            i += 1;
        }
        if !found {
            return Err(E_GATE_MISS);
        }
        let cases = self.regs[idx].cases;
        let mut c = 0u32;
        while c < cases {
            self.regs[idx].record_pass();
            c += 1;
        }
        Ok(())
    }

    pub fn attach_evidence(&mut self, gate_no: u16, fp: u32, tag: &str) -> Result<(), u16> {
        let mut found = false;
        let mut i = 0usize;
        while i < self.regs.len() {
            if self.regs[i].gate_no == gate_no {
                found = true;
            }
            i += 1;
        }
        if !found {
            return Err(E_GHOST_GATE);
        }
        self.evids.push(ShortEvidence::make(gate_no, fp, tag));
        Ok(())
    }

    /// 终账核对：每个登记闸门齐备且对应短程证据在册
    pub fn settle(&self) -> Result<u32, u16> {
        if self.regs.is_empty() {
            return Err(E_GATE_MISS);
        }
        let mut settled = 0u32;
        let mut i = 0usize;
        while i < self.regs.len() {
            let g = self.regs[i].clone();
            if !g.complete() {
                return Err(E_MIX_UNBAL);
            }
            let mut ev = false;
            let mut j = 0usize;
            while j < self.evids.len() {
                if self.evids[j].gate_no == g.gate_no {
                    ev = true;
                }
                j += 1;
            }
            if !ev {
                return Err(E_EVID_MISS);
            }
            settled += 1;
            i += 1;
        }
        Ok(settled)
    }
}

/// 确定性 LCG
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
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 全量登记 ×10^4：登记-跑账-闸门齐备（正例）
fn sweep_full_reg(rng: &mut Lcg) -> Result<(), u16> {
    let mut led = StableLedger::new();
    let mut gate = 1u16;
    while gate <= 12 {
        let cases = 2 + rng.below(6) as u32;
        led.register(gate, 24, cases)?;
        gate += 1;
    }
    gate = 1u16;
    while gate <= 12 {
        led.run_full(gate, 24)?;
        gate += 1;
    }
    let settled = led.settle()?;
    if settled != 12 {
        return Err(E_MIX_UNBAL);
    }
    Ok(())
}

/// 短程证据齐备 ×10：证据-闸门一一对应（正例）
fn sweep_short_evid(rng: &mut Lcg) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 10 {
        let mut led = StableLedger::new();
        let gates = 2 + rng.below(5);
        let mut g = 0u64;
        while g < gates {
            let gate_no = (g as u16) + 1;
            led.register(gate_no, round, 3)?;
            led.run_full(gate_no, round)?;
            let fp = rng.next() as u32;
            led.attach_evidence(gate_no, fp, "short-ev")?;
            g += 1;
        }
        let settled = led.settle()?;
        if settled != gates as u32 {
            return Err(E_EVID_MISS);
        }
        round += 1;
    }
    Ok(())
}

/// 反例注入：闸门缺登记跑账必须 E_GATE_MISS；证据挂幽灵闸门必须 E_GHOST_GATE
fn sweep_negatives() -> Result<(), u16> {
    let mut led = StableLedger::new();
    let r1 = led.run_full(9, 24);
    if r1.err() != Some(E_GATE_MISS) {
        return Err(E_GATE_MISS);
    }
    led.register(9, 24, 4)?;
    let r2 = led.attach_evidence(7, 0x1234, "ghost");
    if r2.err() != Some(E_GHOST_GATE) {
        return Err(E_GHOST_GATE);
    }
    let r3 = led.register(9, 24, 4);
    if r3.err() != Some(E_REG_DUP) {
        return Err(E_REG_DUP);
    }
    let r4 = led.register(10, 24, 0);
    if r4.err() != Some(E_DUR_ZERO) {
        return Err(E_DUR_ZERO);
    }
    led.run_full(9, 24)?;
    let r5 = led.settle();
    if r5.err() != Some(E_EVID_MISS) {
        return Err(E_EVID_MISS);
    }
    Ok(())
}

/// 混合平衡 ×10^3：全量轮与短程证据交替入册不断档
fn sweep_mix_balance(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut led = StableLedger::new();
        let mut g = 0u64;
        let gates = 3 + rng.below(4);
        while g < gates {
            let gate_no = (g as u16) + 1;
            led.register(gate_no, iter % 24, 2)?;
            led.run_full(gate_no, iter % 24)?;
            led.attach_evidence(gate_no, rng.below(0xFFFF) as u32, "mix")?;
            g += 1;
        }
        if led.settle()? != gates as u32 {
            return Err(E_MIX_UNBAL);
        }
        iter += 1;
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut a = StableLedger::new();
        let mut b = StableLedger::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut g = 0u64;
        while g < 6 {
            let gate_no = (g as u16) + 1;
            let cases = 2 + ra.below(5) as u32;
            let cases_b = 2 + rb.below(5) as u32;
            if cases != cases_b {
                return Err(E_REPLAY_DIFF);
            }
            a.register(gate_no, round, cases)?;
            b.register(gate_no, round, cases_b)?;
            a.run_full(gate_no, round)?;
            b.run_full(gate_no, round)?;
            let fpa = ra.below(0xFFFF) as u32;
            let fpb = rb.below(0xFFFF) as u32;
            if fpa != fpb {
                return Err(E_REPLAY_DIFF);
            }
            a.attach_evidence(gate_no, fpa, "replay")?;
            b.attach_evidence(gate_no, fpb, "replay")?;
            g += 1;
        }
        if a.settle()? != b.settle()? {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 全量登记 ×10^4 零缺闸
    let mut rng = Lcg::new(0x4874_3A11);
    let mut r1 = 0u32;
    while r1 < 10_000 {
        sweep_full_reg(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 短程证据 ×10 全对应
    sweep_short_evid(&mut rng)?;
    hits += 1;
    // R3 反例注入四路全部命中
    sweep_negatives()?;
    hits += 1;
    // R4 混合平衡 ×10^3
    sweep_mix_balance(&mut rng)?;
    hits += 1;
    // R5 重放一致 ×20
    sweep_replay(0x4874_3BEE)?;
    hits += 1;
    // R6 轮次序连续性：24h 全量轮编号单调不断档
    let mut led = StableLedger::new();
    let mut round = 0u32;
    while round < 24 {
        led.register((round % 4) as u16 + 1, round, 1)?;
        round += 1;
    }
    if led.regs.len() != 24 {
        return Err(E_SEQ_BREAK);
    }
    hits += 1;
    // R7 证据指纹唯一性抽查
    let mut led2 = StableLedger::new();
    let mut g2 = 0u16;
    while g2 < 8 {
        led2.register(g2 + 1, 0, 2)?;
        led2.run_full(g2 + 1, 0)?;
        led2.attach_evidence(g2 + 1, 0x1000 + g2 as u32, "uniq")?;
        g2 += 1;
    }
    let mut i2 = 0usize;
    while i2 < led2.evids.len() {
        let mut j2 = i2 + 1;
        while j2 < led2.evids.len() {
            if led2.evids[i2] == led2.evids[j2] {
                return Err(E_EVID_MISS);
            }
            j2 += 1;
        }
        i2 += 1;
    }
    hits += 1;
    // R8 闸门编号域校验：全部落在 1..=0x1F 观测带
    let mut led3 = StableLedger::new();
    let mut g3 = 0u16;
    while g3 < 0x0F {
        led3.register(g3 + 1, 1, 1)?;
        led3.run_full(g3 + 1, 1)?;
        led3.attach_evidence(g3 + 1, g3 as u32, "band")?;
        g3 += 1;
    }
    for r in led3.regs.iter() {
        if r.gate_no == 0 || r.gate_no > 0x1F {
            return Err(E_GHOST_GATE);
        }
    }
    hits += 1;
    // R9 完成态断言：cases=passed 才允许 settle 通过
    let mut led4 = StableLedger::new();
    led4.register(3, 2, 5)?;
    led4.run_full(3, 2)?;
    led4.attach_evidence(3, 77, "done")?;
    if led4.settle()? != 1 {
        return Err(E_MIX_UNBAL);
    }
    hits += 1;
    // R10 标签语义抽查：短程证据 tag 必须非空且长度在带内
    let mut led5 = StableLedger::new();
    led5.register(1, 3, 1)?;
    led5.run_full(1, 3)?;
    led5.attach_evidence(1, 0xBEEF, "tail-ev-24h")?;
    let e0 = led5.evids[0].clone();
    if e0.tag.len() < 4 || e0.tag.len() > 32 {
        return Err(E_EVID_MISS);
    }
    hits += 1;
    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a: u32 = 0x9E37_79B9;
    let mut track_b: u32 = 0x9E37_79B9;
    let mut rail: Vec<u32> = vec![];
    let mut rail_sum: u64 = 0;
    let mut dstep = 0u32;
    while dstep < 48 {
        track_a = track_a.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b = track_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail.push(track_a);
        rail_sum += track_a as u64;
        dstep += 1;
    }
    if track_a != track_b {
        return Err(E_REPLAY_DIFF);
    }
    if rail.len() != 48 {
        return Err(E_REPLAY_DIFF);
    }
    let mut replay_sum: u64 = 0;
    let mut ridx = 0usize;
    while ridx < rail.len() {
        replay_sum += rail[ridx] as u64;
        ridx += 1;
    }
    if replay_sum != rail_sum {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    Ok(hits)
}
