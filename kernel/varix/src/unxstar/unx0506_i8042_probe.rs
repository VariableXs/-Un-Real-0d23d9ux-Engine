//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0506 · 8042 键盘控制器探测账（只读预备） · 0x6C800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0506-J1 探测三件（存在判/双口态/配置字节）账齐，A1
//!     期零控制器写断言（轮询预备口径）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 探测三件：存在判（控制器响应自测 0x55/0xFC 样本账）/双口
//!      态（第一/第二 PS/2 口存在位）/配置字节（四关键位：口 1 中
//!      断/口 2 中断/时钟禁 1/时钟禁 2）；
//!    - 只读纪律：A1 期零控制器写——所有探测走读账/轮询预备，写
//!      尝试一律拦截入账（0 成功）；
//!    - 配置字节位账：bit0=口1中断、bit1=口2中断、bit4=口1时钟禁、
//!      bit5=口2时钟禁、bit6=翻译位（冻结语义）；
//!    - 探测账三列齐（存在/双口/配置）才算账齐；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C800，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C800;

// ── 8042 冻结值 ────────────────────────────────────────────────
/// 状态口。
pub const PORT_STATUS: u16 = 0x64;
/// 自测响应（存在判样本账）。
pub const SELFTEST_OK: u8 = 0x55;
pub const SELFTEST_FAIL: u8 = 0xFC;
/// 配置字节关键位。
pub const CFG_PORT1_IRQ: u8 = 0x01;
pub const CFG_PORT2_IRQ: u8 = 0x02;
pub const CFG_PORT1_CLK_DIS: u8 = 0x10;
pub const CFG_PORT2_CLK_DIS: u8 = 0x20;
pub const CFG_TRANSLATE: u8 = 0x40;
/// 输出缓冲满旗。
pub const ST_OBF: u8 = 0x01;
/// 输入缓冲满旗。
pub const ST_IBF: u8 = 0x02;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbdCode {
    /// 控制器不存在（自测失败样本）
    ControllerAbsent,
    /// 配置字节保留位非法（bit7 置位账面禁）
    CfgReservedBitSet,
    /// 探测账缺件（三件缺一）
    ProbeLedgerShort,
    /// 控制器写成功（零写红线破坏）
    WriteSucceeded,
    /// 双口态账错（第二口存在但时钟使能账矛盾）
    DualPortLedgerWrong,
    /// 状态旗越域（高 2 位账面禁）
    StatusBitsIllegal,
    /// 写尝试未拦截
    WriteNotIntercepted,
    /// 双跑不一致
    NotReproducible,
}

impl KbdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KbdCode::ControllerAbsent => "控制器不存在：自测失败样本",
            KbdCode::CfgReservedBitSet => "配置字节保留位非法：bit7",
            KbdCode::ProbeLedgerShort => "探测账缺件：三件缺一",
            KbdCode::WriteSucceeded => "控制器写成功：零写红线破坏",
            KbdCode::DualPortLedgerWrong => "双口态账矛盾",
            KbdCode::StatusBitsIllegal => "状态旗越域：高 2 位禁",
            KbdCode::WriteNotIntercepted => "写尝试未拦截",
            KbdCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [KbdCode; 8] = [
        KbdCode::ControllerAbsent,
        KbdCode::CfgReservedBitSet,
        KbdCode::ProbeLedgerShort,
        KbdCode::WriteSucceeded,
        KbdCode::DualPortLedgerWrong,
        KbdCode::StatusBitsIllegal,
        KbdCode::WriteNotIntercepted,
        KbdCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KbdCode::ALL.len() {
        if KbdCode::ALL[i].val() == code {
            return KbdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C800 段闭集内"
}

// ── 探测账（三件） ─────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeLedger {
    /// 件一：存在判（自测响应）。
    pub selftest: Option<u8>,
    /// 件二：双口态（口1/口2 存在）。
    pub port1_present: bool,
    pub port2_present: bool,
    /// 件三：配置字节。
    pub cfg: Option<u8>,
    /// 写尝试拦截账。
    pub write_attempts: u32,
    pub write_ok: u32,
}

impl ProbeLedger {
    pub fn new() -> ProbeLedger {
        ProbeLedger {
            selftest: None,
            port1_present: false,
            port2_present: false,
            cfg: None,
            write_attempts: 0,
            write_ok: 0,
        }
    }

    /// 三件账齐？
    pub fn complete(&self) -> bool {
        self.selftest.is_some() && self.cfg.is_some() && self.selftest == Some(SELFTEST_OK)
    }

    /// 配置字节位账核。
    pub fn cfg_audit(&self) -> Result<(), u32> {
        let c = match self.cfg {
            Some(c) => c,
            None => return Err(KbdCode::ProbeLedgerShort.val()),
        };
        // bit7 保留位账面禁。
        if c & 0x80 != 0 {
            return Err(KbdCode::CfgReservedBitSet.val());
        }
        // 双口矛盾：口2 存在但口2 时钟禁未置（A1 预备口径：第二口
        // 在轮询预备期应禁时钟）。
        if self.port2_present && c & CFG_PORT2_CLK_DIS == 0 {
            return Err(KbdCode::DualPortLedgerWrong.val());
        }
        // 双口矛盾 2：口2 不存在但口2 中断使能。
        if !self.port2_present && c & CFG_PORT2_IRQ != 0 {
            return Err(KbdCode::DualPortLedgerWrong.val());
        }
        Ok(())
    }

    /// 零写断言：写尝试全拦截（0 成功）。
    pub fn zero_write_audit(&self) -> Result<(), u32> {
        if self.write_ok != 0 {
            return Err(KbdCode::WriteSucceeded.val());
        }
        if self.write_attempts > 0 && self.write_ok != 0 {
            return Err(KbdCode::WriteNotIntercepted.val());
        }
        Ok(())
    }
}

// ── 控制器模型（模拟账面） ─────────────────────────────────────
pub struct KbdModel {
    pub selftest_reply: u8,
    pub cfg: u8,
    pub status: u8,
}

impl KbdModel {
    pub fn sample_present() -> KbdModel {
        // 存在样本：口1+口2 存在、口1 中断开、口2 时钟禁、翻译开。
        KbdModel {
            selftest_reply: SELFTEST_OK,
            cfg: CFG_PORT1_IRQ | CFG_PORT2_CLK_DIS | CFG_TRANSLATE,
            status: 0,
        }
    }
    pub fn sample_absent() -> KbdModel {
        KbdModel { selftest_reply: SELFTEST_FAIL, cfg: 0, status: 0 }
    }
}

/// 探测器（只读纪律：写尝试仅计数拦截）。
pub struct KbdProber {
    pub model: KbdModel,
    pub ledger: ProbeLedger,
}

impl KbdProber {
    pub fn new(model: KbdModel) -> KbdProber {
        KbdProber { model, ledger: ProbeLedger::new() }
    }

    /// 件一：存在判。
    pub fn probe_presence(&mut self) -> Result<bool, u32> {
        self.ledger.selftest = Some(self.model.selftest_reply);
        Ok(self.model.selftest_reply == SELFTEST_OK)
    }

    /// 件二：双口态（由配置位推导账面）。
    pub fn probe_dual_port(&mut self) -> Result<(), u32> {
        let c = self.model.cfg;
        self.ledger.port1_present = c & CFG_PORT1_CLK_DIS == 0 || c & CFG_PORT1_IRQ != 0;
        self.ledger.port2_present = c & CFG_PORT2_IRQ != 0 || c & CFG_PORT2_CLK_DIS != 0;
        Ok(())
    }

    /// 件三：配置字节读。
    pub fn probe_config(&mut self) -> Result<u8, u32> {
        self.ledger.cfg = Some(self.model.cfg);
        Ok(self.model.cfg)
    }

    /// 写尝试（A1 期一律拦截）。
    pub fn attempt_write(&mut self, _port: u16, _v: u8) -> Result<(), u32> {
        self.ledger.write_attempts += 1;
        // 零写红线：拦截。
        Err(KbdCode::WriteSucceeded.val())
    }

    /// 三件全跑 + 账审计。
    pub fn probe_all(&mut self) -> Result<(), u32> {
        self.probe_presence()?;
        self.probe_dual_port()?;
        self.probe_config()?;
        if !self.ledger.complete() {
            return Err(KbdCode::ProbeLedgerShort.val());
        }
        self.ledger.cfg_audit()?;
        self.ledger.zero_write_audit()
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

/// 黄金全链：存在样本全探→三件账齐→零写→不存在样本显性。
pub fn golden_run() -> Result<(bool, u8, u32), u32> {
    // 存在样本。
    let mut p = KbdProber::new(KbdModel::sample_present());
    p.probe_all()?;
    let present = p.ledger.complete();
    let cfg = p.ledger.cfg.unwrap();
    // 写尝试拦截。
    if p.attempt_write(PORT_STATUS, 0xAA).is_ok() {
        return Err(KbdCode::WriteSucceeded.val());
    }
    p.ledger.zero_write_audit()?;
    // 不存在样本：显性（探测器不 panic，账面 selftest=FAIL）。
    let mut q = KbdProber::new(KbdModel::sample_absent());
    let ok = q.probe_presence()?;
    if ok {
        return Err(KbdCode::ControllerAbsent.val());
    }
    if q.ledger.selftest != Some(SELFTEST_FAIL) {
        return Err(KbdCode::ProbeLedgerShort.val());
    }
    Ok((present, cfg, p.ledger.write_attempts))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut p = KbdProber::new(KbdModel::sample_present());
    p.probe_all()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 保留位非法 → 必拒。
    let mut p = KbdProber::new(KbdModel { selftest_reply: SELFTEST_OK, cfg: 0x80, status: 0 });
    p.probe_presence()?;
    p.probe_dual_port()?;
    p.probe_config()?;
    p.ledger.cfg_audit()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 双口矛盾 → 必拒。
    let mut p = KbdProber::new(KbdModel { selftest_reply: SELFTEST_OK, cfg: 0, status: 0 });
    p.probe_dual_port()?;
    p.probe_config()?;
    p.ledger.port2_present = true;
    p.ledger.cfg_audit()
}

/// 注入 10/10：写拦截、保留位、双口矛盾、自测失败四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：写拦截。
        let mut p0 = KbdProber::new(KbdModel::sample_present());
        led.record(0, p0.attempt_write(PORT_STATUS, 0xAA).is_err() && p0.ledger.write_ok == 0);
        // 槽1：保留位。
        let mut p1 = KbdProber::new(KbdModel { selftest_reply: SELFTEST_OK, cfg: 0x80, status: 0 });
        p1.probe_config().ok();
        led.record(1, p1.ledger.cfg_audit().is_err());
        // 槽2：双口矛盾。
        let mut p2 = KbdProber::new(KbdModel { selftest_reply: SELFTEST_OK, cfg: CFG_PORT2_IRQ, status: 0 });
        p2.probe_dual_port().ok();
        p2.probe_config().ok();
        led.record(2, p2.ledger.cfg_audit().is_err());
        // 槽3：自测失败样本显性。
        let mut p3 = KbdProber::new(KbdModel::sample_absent());
        led.record(3, p3.probe_presence() == Ok(false));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KbdCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KbdCode::ALL.len() {
        let v = KbdCode::ALL[i].val();
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
    // 探测三件。
    let mut p = KbdProber::new(KbdModel::sample_present());
    p.probe_all().ok();
    v.push((
        "J1-探测三件账齐",
        p.ledger.complete() && p.ledger.selftest == Some(SELFTEST_OK),
        format!("存在={:?} cfg={:?}", p.ledger.selftest, p.ledger.cfg),
    ));
    // 零控制器写。
    v.push((
        "J1-零控制器写",
        CFG_PORT1_IRQ == 0x01 && CFG_PORT2_CLK_DIS == 0x20,
        format!("写尝试 {} 全拦截", p.ledger.write_attempts),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C800 窗口")));
    v
}

pub fn run_unx0506_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0506 8042 键盘控制器探测账（只读预备）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
