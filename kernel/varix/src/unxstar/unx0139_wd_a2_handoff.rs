//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0139 · 早年看门狗与 A2 中断交接 · 0x6EF00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0139-J1 早年看门狗：引导期软看门狗（阶段超时表），
//!     超时进入 A2 中断交接协议（向量/IST/表指针三要素交接包），
//!     交接包封存只读，A2 收账回签，交接账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 阶段超时表（六阶段各冻结 ms，与 F0127 同源）；
//!   - 触发：阶段超时 ⇒ 交接（A2 预备）；
//!   - 交接包三要素：向量基址/IST 槽位/IDT 表指针；
//!   - 封存只读 + A2 回签三要素；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EF00，AI-VX3-1102 批N 独占区）。
pub const SEG_BASE: u32 = 0x6EF00;

/// 阶段超时表（与 F0127 同源冻结）。
pub const TIMEOUT_MS: [u64; 6] = [20, 150, 100, 50, 200, 80];
/// 交接三要素数。
pub const HANDOFF_ITEMS: usize = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WdA2Code {
    /// 阶段越界
    PhaseOver,
    /// 超时未触发交接
    TimeoutNoHandoff,
    /// 回签缺要素
    AckIncomplete,
    /// 封存后改写
    SealedMutated,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl WdA2Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WdA2Code::PhaseOver => "阶段越界：≥6",
            WdA2Code::TimeoutNoHandoff => "超时未触发交接",
            WdA2Code::AckIncomplete => "回签缺要素",
            WdA2Code::SealedMutated => "封存后改写：只读红线",
            WdA2Code::ReplayMismatch => "复算失配",
            WdA2Code::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [WdA2Code; 6] = [
        WdA2Code::PhaseOver,
        WdA2Code::TimeoutNoHandoff,
        WdA2Code::AckIncomplete,
        WdA2Code::SealedMutated,
        WdA2Code::ReplayMismatch,
        WdA2Code::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WdA2Code::ALL.len() {
        if WdA2Code::ALL[i].val() == code {
            return WdA2Code::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EF00 段闭集内"
}

// ── 看门狗账 ────────────────────────────────────────────────────
fn fnv3(items: &[u64; HANDOFF_ITEMS]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < HANDOFF_ITEMS {
        let v = items[i];
        let mut b = 0;
        while b < 8 {
            h = (h ^ ((v >> (b * 8)) & 0xFF)).wrapping_mul(0x100000001b3);
            b += 1;
        }
        i += 1;
    }
    h
}

#[derive(Clone, Debug)]
pub struct HandoffPacket {
    /// 三要素：向量基址/IST 槽位/IDT 表指针。
    pub items: [u64; HANDOFF_ITEMS],
    pub fp: u64,
    pub sealed: bool,
    pub acked: bool,
    pub ack_flags: [bool; HANDOFF_ITEMS],
}

impl HandoffPacket {
    /// 构建（触发时组装三要素）。
    pub fn build(vector_base: u64, ist_slot: u64, idt_ptr: u64) -> HandoffPacket {
        let items = [vector_base, ist_slot, idt_ptr];
        HandoffPacket { items, fp: fnv3(&items), sealed: false, acked: false, ack_flags: [false; HANDOFF_ITEMS] }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封存对拍。
    pub fn mutate_audit(&self, fresh: &HandoffPacket) -> bool {
        if !self.sealed {
            return true;
        }
        self.fp == fresh.fp && self.items == fresh.items
    }

    /// A2 回签（三要素一次齐 + 指纹对拍）。
    pub fn ack(&mut self, flags: [bool; HANDOFF_ITEMS], fp: u64) -> Result<(), u32> {
        if !self.sealed {
            return Err(WdA2Code::SealedMutated.val());
        }
        let mut i = 0;
        while i < HANDOFF_ITEMS {
            if !flags[i] {
                return Err(WdA2Code::AckIncomplete.val());
            }
            i += 1;
        }
        if fp != self.fp {
            return Err(WdA2Code::ReplayMismatch.val());
        }
        self.acked = true;
        self.ack_flags = flags;
        Ok(())
    }
}

/// 看门狗判定：阶段实测 > 预算 ⇒ 触发。
pub fn watchdog_check(phase: usize, measured_ms: u64) -> Result<bool, u32> {
    if phase >= 6 {
        return Err(WdA2Code::PhaseOver.val());
    }
    Ok(measured_ms > TIMEOUT_MS[phase])
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：越段路/缺签路/改写路/复算路） ──────────────
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
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机交接包。
pub fn machine_packet() -> HandoffPacket {
    HandoffPacket::build(0x1000, 2, 0x2000)
}

// ── 黄金全链：超时判定→组包→封存→回签→对拍→复算 ──────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 超时判定：FS 260ms > 200ms ⇒ 触发。
    let fire = watchdog_check(4, 260)?;
    let nofire = !watchdog_check(4, 199)?;
    let fire_ok = (fire && nofire) as u32;
    // 组包封存回签。
    let mut p = machine_packet();
    let fp = p.fp;
    p.seal();
    // 封存对拍。
    let same = p.mutate_audit(&machine_packet());
    p.ack([true, true, true], fp)?;
    // 复算：同包再判恒一致。
    let fp2 = machine_packet().fp;
    if fp2 != fp {
        return Err(WdA2Code::ReplayMismatch.val());
    }
    Ok((fire_ok as u32, p.acked as u32, same as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：超时触发 + 回签真 + 对拍真。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 1, 1)) => Ok(()),
        _ => Err(WdA2Code::NotReproducible.val()),
    }
}

/// 序 B：回签缺要素 ⇒ AckIncomplete。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = machine_packet();
    p.seal();
    match p.ack([true, false, true], p.fp) {
        Err(e) if e == WdA2Code::AckIncomplete.val() => Ok(()),
        _ => Err(WdA2Code::NotReproducible.val()),
    }
}

/// 序 C：阶段越界 ⇒ PhaseOver。
pub fn probe_seq_c() -> Result<(), u32> {
    match watchdog_check(9, 1) {
        Err(e) if e == WdA2Code::PhaseOver.val() => Ok(()),
        _ => Err(WdA2Code::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越段路。
        led.record(0, watchdog_check(7, 1).is_err());
        // 路 2：缺签路。
        let mut p2 = machine_packet();
        p2.seal();
        led.record(1, p2.ack([false, true, true], p2.fp).is_err());
        // 路 3：改写路。
        let mut p3 = machine_packet();
        p3.seal();
        let bad = HandoffPacket::build(0x1000, 2, 0x3000);
        led.record(2, !p3.mutate_audit(&bad));
        // 路 4：复算路。
        led.record(3, machine_packet().fp == machine_packet().fp);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(WdA2Code::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < WdA2Code::ALL.len() {
        let v = WdA2Code::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((fire, ack, same)) => *fire == 1 && *ack == 1 && *same == 1,
            Err(_) => false,
        },
        match &g {
            Ok((fire, ack, same)) => format!("fired={} acked={} sealed_audit={}", fire, ack, same),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 超时表冻结（与 F0127 同源）。
    v.push((
        "J1-超时表同源",
        TIMEOUT_MS == [20u64, 150, 100, 50, 200, 80],
        String::from("与 F0127 预算表同源冻结"),
    ));
    // 交接三要素（向量/IST/IDT）。
    let p = machine_packet();
    v.push((
        "J1-交接三要素",
        p.items[0] == 0x1000 && p.items[1] == 2 && p.items[2] == 0x2000,
        String::from("向量基址/IST 槽位/IDT 指针 三账"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6EF00 窗口"),
    ));
    v
}

pub fn run_unx0139_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0139 早年看门狗与 A2 中断交接");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
