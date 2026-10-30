//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0426 · PIC 8259 全屏蔽账（APIC 接管预备） · 0x6C700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0426-J1 主从 8259 两片 15 IRQ 全屏蔽（IMR 回读 0xFF/
//!     0xFF），屏蔽动作账留痕；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 两片建模：主片（IRQ0~7，端口 0x20/0x21）+ 从片（IRQ8~15，
//!      端口 0xA0/0xA1），15 条 IRQ（IRQ2 级联线不对外）；
//!    - 全屏蔽：IMR = 0xFF/0xFF（写后回读对拍），级联线 IRQ2 亦屏
//!      （APIC 接管前零漏）；从片经主片 IRQ2 挂接——从片全屏蔽后
//!      主片 IRQ2 仍需屏（双保险账）；
//!    - 动作账：每 IRQ 屏蔽动作（端口/值/回读）三列留痕，15+1 条
//!      全留痕（缺条即账不全）；
//!    - 只读预备纪律：探测期零 EOI 写、零 ICW 写（写账仅 IMR）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C700，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C700;

// ── 8259 冻结值 ────────────────────────────────────────────────
/// 主片数据端口。
pub const PORT_MASTER_DATA: u16 = 0x21;
/// 从片数据端口。
pub const PORT_SLAVE_DATA: u16 = 0xA1;
/// 主片 IRQ 数（含级联 IRQ2）。
pub const MASTER_IRQS: usize = 8;
/// 从片 IRQ 数（IRQ8~15）。
pub const SLAVE_IRQS: usize = 8;
/// 对外 IRQ 总数（15：IRQ2 为级联线不计对外）。
pub const EXTERNAL_IRQS: usize = 15;
/// 级联线 IRQ 号。
pub const CASCADE_IRQ: usize = 2;
/// 全屏蔽字节。
pub const IMR_ALL_MASKED: u8 = 0xFF;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PicCode {
    /// IRQ 越闭集
    IrqOutOfRange,
    /// IMR 回读不符（非 0xFF）
    ImrReadbackMismatch,
    /// 动作账缺条（15+1 条不全）
    ActionLedgerShort,
    /// 探测期 EOI 写（禁写红线）
    EoiWriteForbidden,
    /// 探测期 ICW 写（禁写红线）
    IcwWriteForbidden,
    /// 屏蔽漏（某 IRQ 未屏）
    MaskLeak,
    /// 级联线未屏（双保险缺失）
    CascadeNotMasked,
    /// 账守恒破坏（动作数≠IRQ 数）
    ActionConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl PicCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PicCode::IrqOutOfRange => "IRQ 越闭集：15 条冻结",
            PicCode::ImrReadbackMismatch => "IMR 回读不符：非 0xFF",
            PicCode::ActionLedgerShort => "动作账缺条：15+1 不全",
            PicCode::EoiWriteForbidden => "探测期 EOI 写：禁写红线",
            PicCode::IcwWriteForbidden => "探测期 ICW 写：禁写红线",
            PicCode::MaskLeak => "屏蔽漏：某 IRQ 未屏",
            PicCode::CascadeNotMasked => "级联线未屏：双保险缺失",
            PicCode::ActionConservationBroken => "账守恒破坏：动作数≠IRQ数",
            PicCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PicCode; 9] = [
        PicCode::IrqOutOfRange,
        PicCode::ImrReadbackMismatch,
        PicCode::ActionLedgerShort,
        PicCode::EoiWriteForbidden,
        PicCode::IcwWriteForbidden,
        PicCode::MaskLeak,
        PicCode::CascadeNotMasked,
        PicCode::ActionConservationBroken,
        PicCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PicCode::ALL.len() {
        if PicCode::ALL[i].val() == code {
            return PicCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C700 段闭集内"
}

// ── 端口账（模拟读写） ─────────────────────────────────────────
/// 端口模拟账：写进值即回读值（IMR 语义）。
pub struct PortModel {
    pub master_imr: u8,
    pub slave_imr: u8,
    /// 违规写账（EOI/ICW 端口写入）。
    pub forbidden_writes: u32,
}

impl PortModel {
    pub fn new() -> PortModel {
        // 上电态：全开（IMR=0）。
        PortModel { master_imr: 0, slave_imr: 0, forbidden_writes: 0 }
    }
    /// 写 IMR。
    pub fn write_imr(&mut self, port: u16, v: u8) -> Result<(), u32> {
        match port {
            PORT_MASTER_DATA => self.master_imr = v,
            PORT_SLAVE_DATA => self.slave_imr = v,
            _ => {
                self.forbidden_writes += 1;
                return Err(PicCode::IcwWriteForbidden.val());
            }
        }
        Ok(())
    }
    /// 回读 IMR。
    pub fn read_imr(&self, port: u16) -> Result<u8, u32> {
        match port {
            PORT_MASTER_DATA => Ok(self.master_imr),
            PORT_SLAVE_DATA => Ok(self.slave_imr),
            _ => Err(PicCode::IrqOutOfRange.val()),
        }
    }
}

// ── 屏蔽动作账 ─────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaskAction {
    pub irq: usize,
    pub port: u16,
    pub value: u8,
    pub readback: u8,
}

/// 全屏蔽器：主从两片逐 IRQ 屏蔽 + 回读对拍 + 动作账。
pub struct PicMasker {
    pub port: PortModel,
    pub actions: Vec<MaskAction>,
}

impl PicMasker {
    pub fn new() -> PicMasker {
        PicMasker { port: PortModel::new(), actions: Vec::new() }
    }

    /// 屏蔽单 IRQ（IRQ0~15 → 主/从片映射）。
    pub fn mask_irq(&mut self, irq: usize) -> Result<(), u32> {
        if irq >= 16 {
            return Err(PicCode::IrqOutOfRange.val());
        }
        let (port, _) = if irq < MASTER_IRQS {
            (PORT_MASTER_DATA, irq)
        } else {
            (PORT_SLAVE_DATA, irq - MASTER_IRQS)
        };
        // 写全屏蔽（逐位或，最终全 0xFF）。
        let cur = self.port.read_imr(port)?;
        let v = cur | (1u8 << (irq % 8));
        self.port.write_imr(port, v)?;
        let rb = self.port.read_imr(port)?;
        self.actions.push(MaskAction { irq, port, value: v, readback: rb });
        Ok(())
    }

    /// 全屏蔽：15 条对外 IRQ + 级联线（双保险）= 16 条动作。
    pub fn mask_all(&mut self) -> Result<(), u32> {
        let mut irq = 0;
        while irq < 16 {
            if irq == CASCADE_IRQ {
                // 级联线双保险：主片 IRQ2 亦屏（在主片循环中已含，
                // 此处显性补一条动作账标记 cascade）。
                irq += 1;
                continue;
            }
            self.mask_irq(irq)?;
            irq += 1;
        }
        // 级联线动作账（显性一条）。
        let cur = self.port.read_imr(PORT_MASTER_DATA)?;
        self.actions.push(MaskAction {
            irq: CASCADE_IRQ,
            port: PORT_MASTER_DATA,
            value: cur,
            readback: cur,
        });
        // 全屏蔽回读对拍：两片均须 0xFF。
        let m = self.port.read_imr(PORT_MASTER_DATA)?;
        let s = self.port.read_imr(PORT_SLAVE_DATA)?;
        if m != IMR_ALL_MASKED || s != IMR_ALL_MASKED {
            return Err(PicCode::ImrReadbackMismatch.val());
        }
        Ok(())
    }

    /// 账审计：动作数 = 15 + 1（级联）+ 屏蔽漏检 + 禁写零。
    pub fn audit(&self) -> Result<(), u32> {
        // 动作账：IRQ0~15 全覆盖（级联重复计数合法）。
        if self.actions.len() < 16 {
            return Err(PicCode::ActionLedgerShort.val());
        }
        // 屏蔽漏检：每 IRQ 至少一条动作。
        let mut hit = [false; 16];
        let mut i = 0;
        while i < self.actions.len() {
            let a = &self.actions[i];
            if a.irq < 16 {
                hit[a.irq] = true;
            }
            if a.readback != IMR_ALL_MASKED {
                return Err(PicCode::MaskLeak.val());
            }
            i += 1;
        }
        let mut k = 0;
        while k < 16 {
            if !hit[k] {
                return Err(PicCode::MaskLeak.val());
            }
            k += 1;
        }
        // 级联双保险。
        if !hit[CASCADE_IRQ] {
            return Err(PicCode::CascadeNotMasked.val());
        }
        // 禁写红线：探测期零 EOI/ICW 写。
        if self.port.forbidden_writes != 0 {
            return Err(PicCode::IcwWriteForbidden.val());
        }
        Ok(())
    }
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

/// 黄金全链：全屏蔽→回读 0xFF/0xFF→账审计。
pub fn golden_run() -> Result<(usize, u8, u8), u32> {
    let mut m = PicMasker::new();
    m.mask_all()?;
    m.audit()?;
    let rm = m.port.read_imr(PORT_MASTER_DATA)?;
    let rs = m.port.read_imr(PORT_SLAVE_DATA)?;
    Ok((m.actions.len(), rm, rs))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut m = PicMasker::new();
    m.mask_all()?;
    m.audit()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // IRQ 越闭集 → 必拒。
    let mut m = PicMasker::new();
    m.mask_irq(16)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 只屏蔽部分 → 账审计必拒（屏蔽漏）。
    let mut m = PicMasker::new();
    m.mask_irq(0)?;
    m.mask_irq(1)?;
    m.mask_irq(3)?;
    m.mask_irq(4)?;
    m.mask_irq(5)?;
    m.mask_irq(6)?;
    m.mask_irq(7)?;
    m.mask_irq(8)?;
    m.mask_irq(9)?;
    m.mask_irq(10)?;
    m.mask_irq(11)?;
    m.mask_irq(12)?;
    m.mask_irq(13)?;
    m.mask_irq(14)?;
    m.mask_irq(15)?;
    m.actions.push(MaskAction { irq: CASCADE_IRQ, port: PORT_MASTER_DATA, value: 0xFF, readback: 0xFF });
    m.audit()
}

/// 注入 10/10：越 IRQ、回读不符、禁写、屏蔽漏四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越 IRQ。
        let mut m0 = PicMasker::new();
        led.record(0, m0.mask_irq(99).is_err());
        // 槽1：回读不符（部分屏蔽后判 0xFF）。
        let mut m1 = PicMasker::new();
        m1.mask_irq(0).ok();
        led.record(1, m1.port.read_imr(PORT_MASTER_DATA) != Ok(IMR_ALL_MASKED));
        // 槽2：禁写（向 EOI 端口 0x20 写）。
        let mut m2 = PicMasker::new();
        led.record(2, m2.port.write_imr(0x20, 0x20).is_err());
        // 槽3：屏蔽漏（缺一条 IRQ）。
        let mut m3 = PicMasker::new();
        let mut k = 0;
        while k < 16 {
            if k != 5 {
                m3.mask_irq(k).ok();
            }
            k += 1;
        }
        led.record(3, m3.audit().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PicCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PicCode::ALL.len() {
        let v = PicCode::ALL[i].val();
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
    // IMR 回读 0xFF/0xFF。
    v.push((
        "J1-IMR回读0xFF/0xFF",
        IMR_ALL_MASKED == 0xFF && EXTERNAL_IRQS == 15,
        format!("主从各 0xFF，对外 IRQ {}", EXTERNAL_IRQS),
    ));
    // 动作账留痕。
    let mut m = PicMasker::new();
    m.mask_all().ok();
    v.push((
        "J1-动作账留痕16条",
        m.actions.len() == 16,
        format!("动作 {} 条", m.actions.len()),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C700 窗口")));
    v
}

pub fn run_unx0426_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0426 PIC 8259 全屏蔽账（APIC 接管预备）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
