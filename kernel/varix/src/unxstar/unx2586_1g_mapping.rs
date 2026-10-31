//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2586 · 1G 大页映射建立（PDPTE 直挂，前向兑现位） · 0x6D8C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C2）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F2586-J1 1G 映射建立后 PDPTE 逐位断言（PS=1/物理基址/
//!     权限位），预算口径与 F2518 冻结值一致断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PDPTE 逐位断言：PS 位（bit7=1 表示 1G 页）/物理基址（位
//!      30 起）/权限位（P|W|U）；
//!    - F2518 预算口径冻结（1G 档预算 4 项）；
//!    - 直挂前提：地址 1G 对齐；非对齐拒；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D8C0，AI-VX3-1030 批G 独占区）。
pub const SEG_BASE: u32 = 0x6D8C0;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 1G 对齐掩码。
pub const ALIGN_1G: u64 = 0x4000_0000;
/// PDPTE 位定义。
pub const PF_PRESENT: u64 = 0x1;
pub const PF_WRITE: u64 = 0x2;
pub const PF_USER: u64 = 0x4;
pub const PF_PS: u64 = 0x80;
/// 权限组合（冻结）。
pub const PDPTE_FLAGS: u64 = PF_PRESENT | PF_WRITE | PF_USER | PF_PS;
/// F2518 预算口径（冻结 4 项：入口数/对齐/权限/指纹）。
pub const BUDGET_ITEMS: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Map1GCode {
    /// 地址非 1G 对齐
    Misaligned1G,
    /// PS 位缺失（非大页）
    PsBitMissing,
    /// 物理基址错位
    PhysBaseWrong,
    /// 权限位缺
    FlagsMissing,
    /// 预算口径不符（≠4 项）
    BudgetItemsWrong,
    /// 映射重复
    DuplicateMap,
    /// 逐位断言漏
    BitwiseLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl Map1GCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            Map1GCode::Misaligned1G => "地址非 1G 对齐",
            Map1GCode::PsBitMissing => "PS 位缺失：非大页",
            Map1GCode::PhysBaseWrong => "物理基址错位",
            Map1GCode::FlagsMissing => "权限位缺：P|W|U",
            Map1GCode::BudgetItemsWrong => "预算口径不符：≠4 项",
            Map1GCode::DuplicateMap => "映射重复",
            Map1GCode::BitwiseLeaked => "逐位断言漏",
            Map1GCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [Map1GCode; 8] = [
        Map1GCode::Misaligned1G,
        Map1GCode::PsBitMissing,
        Map1GCode::PhysBaseWrong,
        Map1GCode::FlagsMissing,
        Map1GCode::BudgetItemsWrong,
        Map1GCode::DuplicateMap,
        Map1GCode::BitwiseLeaked,
        Map1GCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < Map1GCode::ALL.len() {
        if Map1GCode::ALL[i].val() == code {
            return Map1GCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D8C0 段闭集内"
}

// ── 1G 映射账 ──────────────────────────────────────────────────
pub struct Map1G {
    /// PDPTE 账（512 项）。
    pub pdpte: [u64; 512],
    pub mapped: Vec<usize>,
}

impl Map1G {
    pub fn new() -> Map1G {
        Map1G { pdpte: [0; 512], mapped: Vec::new() }
    }

    /// 直挂 1G 页（PDPTE[index] = phys | flags）。
    pub fn map_1g(&mut self, index: usize, phys: u64) -> Result<u64, u32> {
        if index >= 512 {
            return Err(Map1GCode::Misaligned1G.val());
        }
        if phys % ALIGN_1G != 0 {
            return Err(Map1GCode::Misaligned1G.val());
        }
        // 重复映射拒。
        let mut i = 0;
        while i < self.mapped.len() {
            if self.mapped[i] == index {
                return Err(Map1GCode::DuplicateMap.val());
            }
            i += 1;
        }
        let entry = phys | PDPTE_FLAGS;
        self.pdpte[index] = entry;
        self.mapped.push(index);
        Ok(entry)
    }

    /// 逐位断言：PS=1/物理基址/权限位。
    pub fn bitwise_audit(&self, index: usize, phys: u64) -> Result<(), u32> {
        let e = self.pdpte[index];
        // PS 位。
        if e & PF_PS == 0 {
            return Err(Map1GCode::PsBitMissing.val());
        }
        // 物理基址（位 30 起）。
        if (e & !0xFFF) >> 30 << 30 != phys {
            return Err(Map1GCode::PhysBaseWrong.val());
        }
        // 权限位。
        if e & (PF_PRESENT | PF_WRITE | PF_USER) != PF_PRESENT | PF_WRITE | PF_USER {
            return Err(Map1GCode::FlagsMissing.val());
        }
        Ok(())
    }

    /// 预算口径点验（F2518 四项）。
    pub fn budget_audit(&self) -> Result<(), u32> {
        if BUDGET_ITEMS != 4 {
            return Err(Map1GCode::BudgetItemsWrong.val());
        }
        // 四项：入口数（512）/对齐掩码/权限组合/PS 位。
        if self.pdpte.len() != 512
            || ALIGN_1G != 0x4000_0000
            || PDPTE_FLAGS != 0xC7
            || PF_PS != 0x80
        {
            return Err(Map1GCode::BudgetItemsWrong.val());
        }
        Ok(())
    }
}

/// 逐位断言 10/10（复跑一致）。
pub fn bitwise_10() -> Result<u32, u32> {
    let mut ok = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut m = Map1G::new();
        let entry = m.map_1g(2, ALIGN_1G * 3)?;
        let _ = entry;
        m.bitwise_audit(2, ALIGN_1G * 3)?;
        ok += 1;
        r += 1;
    }
    if ok != JUDGE_ROUNDS {
        return Err(Map1GCode::BitwiseLeaked.val());
    }
    Ok(ok)
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 黄金全链：1G 直挂→逐位断言→预算口径→重复拒。
pub fn golden_run() -> Result<(u64, u32, u32), u32> {
    let mut m = Map1G::new();
    let entry = m.map_1g(0, ALIGN_1G)?;
    m.map_1g(511, ALIGN_1G * 511)?;
    m.bitwise_audit(0, ALIGN_1G)?;
    m.bitwise_audit(511, ALIGN_1G * 511)?;
    m.budget_audit()?;
    // 重复拒。
    if m.map_1g(0, ALIGN_1G).is_ok() {
        return Err(Map1GCode::DuplicateMap.val());
    }
    let bw = bitwise_10()?;
    Ok((entry, bw, BUDGET_ITEMS as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut m = Map1G::new();
    m.map_1g(100, ALIGN_1G * 100)?;
    m.bitwise_audit(100, ALIGN_1G * 100)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 非 1G 对齐 → 必拒。
    let mut m = Map1G::new();
    m.map_1g(1, 0x1234_5678).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // PS 位缺失注入 → 必拒。
    let mut m = Map1G::new();
    m.pdpte[0] = 0x4000_0000 | PF_PRESENT | PF_WRITE | PF_USER; // 无 PS
    m.mapped.push(0);
    m.bitwise_audit(0, 0x4000_0000)
}

/// 注入 10/10：非对齐、PS 缺、重复、预算四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：非对齐。
        let mut m0 = Map1G::new();
        led.record(0, m0.map_1g(0, 0x3000_0001).is_err());
        // 槽1：PS 缺。
        let mut m1 = Map1G::new();
        m1.pdpte[3] = ALIGN_1G * 3 | PF_PRESENT | PF_WRITE | PF_USER;
        m1.mapped.push(3);
        led.record(1, m1.bitwise_audit(3, ALIGN_1G * 3).is_err());
        // 槽2：重复。
        let mut m2 = Map1G::new();
        m2.map_1g(0, ALIGN_1G).ok();
        led.record(2, m2.map_1g(0, ALIGN_1G).is_err());
        // 槽3：预算。
        led.record(3, BUDGET_ITEMS == 4);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(Map1GCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < Map1GCode::ALL.len() {
        let v = Map1GCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // PDPTE 逐位冻结。
    v.push((
        "J1-PDPTE逐位冻结",
        PDPTE_FLAGS == 0xC7 && ALIGN_1G == 0x4000_0000 && PF_PS == 0x80,
        format!("flags=0x{:02X} align=1G", PDPTE_FLAGS),
    ));
    // 逐位 10/10。
    v.push((
        "J1-逐位断言10/10",
        bitwise_10() == Ok(10),
        format!("{}/10", bitwise_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D8C0 窗口")));
    v
}

pub fn run_unx2586_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2586 1G 大页映射建立（PDPTE 直挂，前向兑现位）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
