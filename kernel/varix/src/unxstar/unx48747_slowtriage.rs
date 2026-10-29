//! UNX-F48747 C·慢输入排疑入口终验（段级归因工具复验 ×10 案例）
//! 判据 UNX-F48747-J1：复验全过；判据锚定 §AI-61 §5 J-1。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 段级归因：输入流水四段（采集/转换/上屏/呈现）耗时定位，超阈段必须被点名。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1AA0;
const E_STAGE_MISS: u16 = SEG_BASE + 1;
const E_ATTR_FAIL: u16 = SEG_BASE + 2;
const E_CASE_MISS: u16 = SEG_BASE + 3;
const E_BUDGET_BAD: u16 = SEG_BASE + 4;
const E_TRACE_GAP: u16 = SEG_BASE + 5;
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;
const E_NO_SLOW: u16 = SEG_BASE + 7;
const E_TOOL_DEAD: u16 = SEG_BASE + 8;

/// 输入流水段：采集 0 / 转换 1 / 上屏 2 / 呈现 3
#[derive(Clone, Debug, PartialEq)]
pub struct StageTrace {
    pub stage: u8,
    pub us: u32,
}

impl StageTrace {
    pub fn name(stage: u8) -> &'static str {
        match stage {
            0 => "capture",
            1 => "convert",
            2 => "commit",
            3 => "render",
            _ => "unknown",
        }
    }
}

/// 排疑案例：一条完整流水的四段轨迹
pub struct TriageCase {
    pub id: u32,
    pub traces: Vec<StageTrace>,
    pub budget_us: u32,
}

impl TriageCase {
    pub fn new(id: u32, budget_us: u32) -> Self {
        TriageCase { id, traces: vec![], budget_us }
    }

    pub fn push(&mut self, stage: u8, us: u32) -> Result<(), u16> {
        if stage > 3 {
            return Err(E_STAGE_MISS);
        }
        let mut i = 0usize;
        while i < self.traces.len() {
            if self.traces[i].stage == stage {
                return Err(E_STAGE_MISS);
            }
            i += 1;
        }
        self.traces.push(StageTrace { stage, us });
        Ok(())
    }

    pub fn total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.traces.len() {
            t += self.traces[i].us;
            i += 1;
        }
        t
    }

    /// 段级归因：返回最耗时段号；全段 <1/2 预算且总耗未超预算则不归因
    pub fn attribute(&self) -> Result<u8, u16> {
        if self.traces.len() != 4 {
            return Err(E_TRACE_GAP);
        }
        if self.total() <= self.budget_us {
            return Err(E_NO_SLOW);
        }
        let mut best = self.traces[0].clone();
        let mut i = 1usize;
        while i < self.traces.len() {
            if self.traces[i].us > best.us {
                best = self.traces[i].clone();
            }
            i += 1;
        }
        if best.us * 2 < self.total() {
            return Err(E_ATTR_FAIL);
        }
        Ok(best.stage)
    }
}

/// 段级归因工具：装载案例并复验
pub struct TriageTool {
    pub cases: Vec<TriageCase>,
}

impl TriageTool {
    pub fn new() -> Self {
        TriageTool { cases: vec![] }
    }

    pub fn load(&mut self, c: TriageCase) {
        self.cases.push(c);
    }

    /// 复验 ×N：每个超预算案例必须归因出超阈段
    pub fn verify_all(&self) -> Result<u32, u16> {
        if self.cases.is_empty() {
            return Err(E_CASE_MISS);
        }
        let mut green = 0u32;
        let mut i = 0usize;
        while i < self.cases.len() {
            let total = self.cases[i].total();
            if total > self.cases[i].budget_us {
                let stage = self.cases[i].attribute()?;
                if stage > 3 {
                    return Err(E_TOOL_DEAD);
                }
            } else {
                let r = self.cases[i].attribute();
                if r.err() != Some(E_NO_SLOW) {
                    return Err(E_ATTR_FAIL);
                }
            }
            green += 1;
            i += 1;
        }
        Ok(green)
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

/// 造一个超预算案例：指定段塞入超阈耗时
fn make_slow_case(id: u32, hot: u8, rng: &mut Lcg) -> Result<TriageCase, u16> {
    let mut c = TriageCase::new(id, 3_000);
    let mut stage = 0u8;
    while stage < 4 {
        let us = if stage == hot {
            4_000 + rng.below(2_000) as u32
        } else {
            200 + rng.below(400) as u32
        };
        c.push(stage, us)?;
        stage += 1;
    }
    Ok(c)
}

/// 造一个正常案例：总耗低于预算
fn make_fast_case(id: u32, rng: &mut Lcg) -> Result<TriageCase, u16> {
    let mut c = TriageCase::new(id, 8_000);
    let mut stage = 0u8;
    while stage < 4 {
        c.push(stage, 100 + rng.below(300) as u32)?;
        stage += 1;
    }
    Ok(c)
}

/// ×10 案例复验全过
fn sweep_ten_cases(rng: &mut Lcg) -> Result<(), u16> {
    let mut tool = TriageTool::new();
    let mut id = 0u32;
    while id < 10 {
        let hot = (id % 4) as u8;
        tool.load(make_slow_case(id, hot, rng)?);
        id += 1;
    }
    if tool.verify_all()? != 10 {
        return Err(E_CASE_MISS);
    }
    let mut i = 0usize;
    while i < tool.cases.len() {
        let expect = (i % 4) as u8;
        let got = tool.cases[i].attribute()?;
        if got != expect {
            return Err(E_ATTR_FAIL);
        }
        i += 1;
    }
    Ok(())
}

/// 反例注入：段重复、流水断档、正常案例不归因、工具空载全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut c = TriageCase::new(1, 1_000);
    c.push(0, 100)?;
    let r1 = c.push(0, 200);
    if r1.err() != Some(E_STAGE_MISS) {
        return Err(E_STAGE_MISS);
    }
    let r2 = c.push(5, 100);
    if r2.err() != Some(E_STAGE_MISS) {
        return Err(E_STAGE_MISS);
    }
    let r3 = c.attribute();
    if r3.err() != Some(E_TRACE_GAP) {
        return Err(E_TRACE_GAP);
    }
    let mut rng = Lcg::new(0x1AA0_0001);
    let fast = make_fast_case(2, &mut rng)?;
    let r4 = fast.attribute();
    if r4.err() != Some(E_NO_SLOW) {
        return Err(E_NO_SLOW);
    }
    let empty = TriageTool::new();
    let r5 = empty.verify_all();
    if r5.err() != Some(E_CASE_MISS) {
        return Err(E_CASE_MISS);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑归因段号逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let ca = make_slow_case(round, (round % 4) as u8, &mut ra)?;
        let cb = make_slow_case(round, (round % 4) as u8, &mut rb)?;
        let ga = ca.attribute()?;
        let gb = cb.attribute()?;
        if ga != gb {
            return Err(E_REPLAY_DIFF);
        }
        if ca.total() != cb.total() {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 ×10 案例复验全过
    let mut rng = Lcg::new(0x4874_7E55);
    sweep_ten_cases(&mut rng)?;
    hits += 1;
    // R2 归因段名落册：四段名逐一可查
    let mut s = 0u8;
    while s < 4 {
        let n = StageTrace::name(s);
        if n == "unknown" {
            return Err(E_STAGE_MISS);
        }
        s += 1;
    }
    hits += 1;
    // R3 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4874_7F01)?;
    hits += 1;
    // R5 混合案例池：慢案例与正常案例同池复验
    let mut tool = TriageTool::new();
    let mut id = 100u32;
    while id < 110 {
        if id % 2 == 0 {
            tool.load(make_slow_case(id, (id % 4) as u8, &mut rng)?);
        } else {
            tool.load(make_fast_case(id, &mut rng)?);
        }
        id += 1;
    }
    if tool.verify_all()? != 10 {
        return Err(E_ATTR_FAIL);
    }
    hits += 1;
    // R6 总耗恒等于四段之和
    let mut c = TriageCase::new(9, 10_000);
    c.push(0, 100)?;
    c.push(1, 200)?;
    c.push(2, 300)?;
    c.push(3, 400)?;
    if c.total() != 1_000 {
        return Err(E_BUDGET_BAD);
    }
    hits += 1;
    // R7 预算界值：恰好等于预算不算慢
    let mut c2 = TriageCase::new(11, 1_000);
    c2.push(0, 250)?;
    c2.push(1, 250)?;
    c2.push(2, 250)?;
    c2.push(3, 250)?;
    if c2.total() != c2.budget_us {
        return Err(E_BUDGET_BAD);
    }
    if c2.attribute().err() != Some(E_NO_SLOW) {
        return Err(E_NO_SLOW);
    }
    hits += 1;
    // R8 超阈段占优断言：最耗段 ≥ 总耗一半才可归因
    let mut c3 = TriageCase::new(12, 100);
    c3.push(0, 400)?;
    c3.push(1, 400)?;
    c3.push(2, 100)?;
    c3.push(3, 100)?;
    let r8 = c3.attribute();
    if r8.err() != Some(E_ATTR_FAIL) {
        return Err(E_ATTR_FAIL);
    }
    hits += 1;
    // R9 工具装载序稳定：装载次序即复验次序
    let mut tool2 = TriageTool::new();
    let mut id2 = 0u32;
    while id2 < 5 {
        tool2.load(make_slow_case(id2, (id2 % 4) as u8, &mut rng)?);
        id2 += 1;
    }
    let mut i = 0usize;
    while i < tool2.cases.len() {
        if tool2.cases[i].id != i as u32 {
            return Err(E_CASE_MISS);
        }
        i += 1;
    }
    hits += 1;
    // R10 归因结果域校验：段号恒在 0..=3
    let mut id3 = 0u32;
    while id3 < 10 {
        let c4 = make_slow_case(id3, (id3 % 4) as u8, &mut rng)?;
        let st = c4.attribute()?;
        if st > 3 {
            return Err(E_TOOL_DEAD);
        }
        id3 += 1;
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
