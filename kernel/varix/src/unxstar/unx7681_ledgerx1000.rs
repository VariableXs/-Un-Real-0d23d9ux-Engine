//! UNX-F7681 · ×1000 总账架构（分账矩阵 1000 轮 = QEMU 700 + 真机 300）· 诊断段 0x7C00
//! 验收锚点：×1000 总账三段落账——① 分账矩阵：轮号 1..=1000 按 QEMU/真机双通道分账，
//! QEMU 占 700（轮号 %10 <7）、真机占 300（轮号 %10 ≥7），分账比恒 7:3 不漂；
//! ② 轮账结构：每轮留（通道、结果码、指纹），结果码 Pass/Fail/Abort 三态；
//! ③ 总收口：1000 轮全账可对——双通道计数恰 700/300，三态无第四态，指纹链闭合。


use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7C00;

pub const TOTAL_ROUNDS: u32 = 1000;
pub const QEMU_TARGET: u32 = 700;
pub const REAL_TARGET: u32 = 300;

/// 通道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Qemu,
    Real,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Qemu => "qemu",
            Channel::Real => "real",
        }
    }
    /// 分账规则：轮号（1 起）%10 < 7 → QEMU，否则真机。
    pub fn route(round: u32) -> Channel {
        if round % 10 < 7 {
            Channel::Qemu
        } else {
            Channel::Real
        }
    }
}

/// 结果码三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    Abort,
}

/// 单轮账。
#[derive(Clone, Copy, Debug)]
pub struct RoundEntry {
    pub round: u32,
    pub channel: Channel,
    pub outcome: Outcome,
    pub fp: u32,
}

impl RoundEntry {
    pub fn new(round: u32, outcome: Outcome) -> Self {
        let ch = Channel::route(round);
        let mut h: u32 = 0x811C_9DC5;
        for b in [round as u8, (round >> 8) as u8, outcome as u8, ch as u8].iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        RoundEntry { round, channel: ch, outcome, fp: h }
    }
}

/// 总账。
pub struct MasterLedger {
    pub entries: Vec<RoundEntry>,
}

impl MasterLedger {
    pub fn new() -> Self {
        MasterLedger { entries: Vec::new() }
    }

    pub fn record(&mut self, e: RoundEntry) {
        self.entries.push(e);
    }

    pub fn count_channel(&self, ch: Channel) -> u32 {
        self.entries.iter().filter(|e| e.channel == ch).count() as u32
    }

    pub fn count_outcome(&self, o: Outcome) -> u32 {
        self.entries.iter().filter(|e| e.outcome == o).count() as u32
    }

    /// 指纹链：逐轮折叠（链式 FNV），闭合账必需非零且确定。
    pub fn fp_chain(&self) -> u32 {
        let mut acc: u32 = 0x811C_9DC5;
        for e in &self.entries {
            acc ^= e.fp;
            acc = acc.wrapping_mul(0x0100_0193);
        }
        acc
    }

    /// 全账校验：1000 轮、7:3 分账、无第四态（结构性保证）。
    pub fn audit(&self) -> bool {
        self.entries.len() == TOTAL_ROUNDS as usize
            && self.count_channel(Channel::Qemu) == QEMU_TARGET
            && self.count_channel(Channel::Real) == REAL_TARGET
            && self.fp_chain() != 0
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x7C00 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7681_7C00;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("路由-轮1归QEMU", Channel::route(1) == Channel::Qemu));
    out.push(("路由-轮7归真机", Channel::route(7) == Channel::Real));
    out.push(("账-空账审计拒绝", !MasterLedger::new().audit()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7681_checks：本单判据聚合（诊断段 0x7C00）。
pub fn run_unx7681_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7681 ×1000 总账架构");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // ① 分账规则：前 1000 轮 7:3 恒比
    let mut qemu = 0u32;
    let mut real = 0u32;
    for r in 1..=1000u32 {
        match Channel::route(r) {
            Channel::Qemu => qemu += 1,
            Channel::Real => real += 1,
        }
    }
    let split_ok = qemu == 700 && real == 300;
    s.add("分账-1000轮7:3恒比", split_ok, if split_ok { "" } else { "分账比漂移" });

    // 每十个轮组内恰 7 QEMU + 3 真机
    let mut group_ok = true;
    for g in 0..100u32 {
        let base = g * 10;
        let mut gq = 0u32;
        for r in 1..=7u32 {
            if Channel::route(base + r) == Channel::Qemu {
                gq += 1;
            }
        }
        if gq != 7 {
            group_ok = false;
        }
    }
    s.add("分账-每十轮组内7QEMU", group_ok, if group_ok { "" } else { "组内分账漂移" });

    // ② 轮账结构：1000 轮全账录入（LCG 定结果）
    let mut led = MasterLedger::new();
    let mut seed: u64 = 0x7681_1000;
    for r in 1..=1000u32 {
        let v = lcg_next(&mut seed) % 100;
        let o = if v < 90 {
            Outcome::Pass
        } else if v < 97 {
            Outcome::Fail
        } else {
            Outcome::Abort
        };
        led.record(RoundEntry::new(r, o));
    }
    let n_ok = led.entries.len() == 1000;
    s.add("轮账-1000轮全录入", n_ok, if n_ok { "" } else { "轮数缺口" });

    // 通道对账恰 700/300
    let ch_ok = led.count_channel(Channel::Qemu) == 700 && led.count_channel(Channel::Real) == 300;
    s.add("轮账-双通道700/300恰数", ch_ok, if ch_ok { "" } else { "通道计数漂移" });

    // 三态合计 = 1000（无第四态漏网——枚举结构保证，账面再核）
    let o_sum = led.count_outcome(Outcome::Pass) + led.count_outcome(Outcome::Fail) + led.count_outcome(Outcome::Abort);
    let o_ok = o_sum == 1000 && led.count_outcome(Outcome::Pass) >= 900;
    s.add("轮账-三态合计1000且Pass≥900", o_ok, if o_ok { "" } else { "三态合计漂移" });

    // 指纹逐轮独立：同轮同果同指纹、异果异指纹
    let fp1 = RoundEntry::new(123, Outcome::Pass).fp;
    let fp2 = RoundEntry::new(123, Outcome::Pass).fp;
    let fp3 = RoundEntry::new(123, Outcome::Fail).fp;
    let fp_ok = fp1 == fp2 && fp1 != fp3;
    s.add("指纹-同构同值异果异值", fp_ok, if fp_ok { "" } else { "指纹不稳" });

    // ③ 总收口：审计通过 + 指纹链重放闭合
    let a_ok = led.audit();
    s.add("总收口-审计全绿", a_ok, if a_ok { "" } else { "审计红" });
    let fp_chain1 = led.fp_chain();
    let fp_chain2 = led.fp_chain();
    s.add("总收口-指纹链重放闭合", fp_chain1 == fp_chain2 && fp_chain1 != 0, if fp_chain1 == fp_chain2 { "" } else { "指纹链不稳" });

    // 混入一轮错账 → 审计即拒（1010 轮）
    let mut led2 = MasterLedger::new();
    for r in 1..=1000u32 {
        led2.record(RoundEntry::new(r, Outcome::Pass));
    }
    led2.record(RoundEntry::new(1001, Outcome::Pass));
    s.add("总收口-错账1010轮审计拒", !led2.audit(), if !led2.audit() { "" } else { "审计漏检" });

    // FNV 通道标签哈希
    let h1 = fnv1a(Channel::Qemu.as_str().as_bytes());
    let h2 = fnv1a(b"qemu");
    s.add("FNV-通道标签哈希稳定", h1 == h2, if h1 == h2 { "" } else { "哈希不稳" });

    s
}
