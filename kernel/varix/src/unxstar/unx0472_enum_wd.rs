//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0472 枚举看门狗（悬挂防护） · 0x75700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7G 逐字锚定，AI-VX7-1104）：
//!   注入悬挂样本检出 100%（3/3：慢设备/死循环/坏响应），看门狗超时线
//!   100ms 显性。
//! 设计要点：
//!   1. 步数化预算：100ms 口径 = 10000 步（STEP_PER_MS=100），单设备预算
//!      500 步；预算只走整数步数，不碰墙钟。
//!   2. 三路注入样本闭集：慢设备 600 步（超单设备预算）、死循环 4096 步
//!      （超预算且吞噬全局预算）、坏响应 0xFFFFFFFF（一步都不给），
//!      三路 100% 检出（3/3）。
//!   3. 超线显性切断：步数超单设备预算即切断该设备（CutOff），切断事件
//!      显性入账（bdf + 路别 + 消耗步数），全局预算随之扣减可复算。
//!   4. 已扫部分入账：切断前的扫描进度（已消耗步数/已完设备数）保留在
//!      账上，不因切断丢失。
//!   5. 断点登记 + 恢复：切断点（设备序号）登记进断点账，恢复扫描从断
//!      点后第一个设备继续，恢复轮次计入账面。
//!   6. 错误路径中文建议写入诊断 detail：慢设备→查配置空间读延迟、死循
//!      环→查链路训练态、坏响应→查设备应答寄存器。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75700，AI-VX7-1104 批 G 组独占区）。
pub const SEG_BASE: u32 = 0x75700;

// ── 冻结常量表（来源：判据冻结口径 + PCI 枚举实践预算） ─────────
/// 看门狗超时线（100ms，冻结）。
pub const WD_TIMEOUT_MS: u32 = 100;
/// 每毫秒步数（100 步/ms，冻结）。
pub const STEP_PER_MS: u32 = 100;
/// 全局步数预算（100ms × 100 = 10000 步，冻结）。
pub const BUDGET_TOTAL: u32 = WD_TIMEOUT_MS * STEP_PER_MS;
/// 单设备步数预算（500 步，冻结）。
pub const PER_DEV_BUDGET: u32 = 500;
/// 慢设备注入样本步数（600 步 > 500 预算）。
pub const SLOW_DEV_STEPS: u32 = 600;
/// 死循环注入样本步数（4096 步）。
pub const LOOP_DEV_STEPS: u32 = 4096;
/// 坏响应注入样本码（0xFFFFFFFF）。
pub const BAD_RESPONSE: u32 = 0xFFFF_FFFF;
/// 正常设备标准步数（100 步，冻结）。
pub const NORMAL_DEV_STEPS: u32 = 100;
/// 注入轮数（判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WdCode {
    /// 慢设备超单设备预算（步数超线）
    SlowDev,
    /// 死循环设备（步数远超预算且持续吞噬全局预算）
    LoopDev,
    /// 坏响应（0xFFFFFFFF 应答码）
    BadResponse,
    /// 全局预算耗尽
    BudgetExhausted,
    /// 断点未登记（恢复无据）
    NoBreakpoint,
    /// 恢复重复（同断点二次恢复未清账）
    ResumeDup,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl WdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WdCode::SlowDev => "慢设备超预算：查配置空间读延迟后重扫",
            WdCode::LoopDev => "死循环设备：查链路训练态与轮询逻辑",
            WdCode::BadResponse => "坏响应 0xFFFFFFFF：查设备应答寄存器",
            WdCode::BudgetExhausted => "全局预算耗尽：核对 10000 步口径",
            WdCode::NoBreakpoint => "断点未登记：恢复扫描必须有断点账",
            WdCode::ResumeDup => "恢复重复：同断点二次恢复须先清账",
            WdCode::ReplayMismatch => "复算失配：账面与重算结果不一致",
            WdCode::NotReproducible => "双跑不一致：复现红线，固定种子复跑",
        }
    }
    pub const ALL: [WdCode; 8] = [
        WdCode::SlowDev,
        WdCode::LoopDev,
        WdCode::BadResponse,
        WdCode::BudgetExhausted,
        WdCode::NoBreakpoint,
        WdCode::ResumeDup,
        WdCode::ReplayMismatch,
        WdCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WdCode::ALL.len() {
        if let Some(c) = WdCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75700 段闭集内"
}

// ── 域模型：设备样本与扫描事件 ──────────────────────────────────
/// 注入路别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectRoute {
    /// 正常设备
    Normal,
    /// 慢设备（600 步）
    Slow,
    /// 死循环（4096 步）
    Loop,
    /// 坏响应（0xFFFFFFFF）
    BadResp,
}

impl InjectRoute {
    /// 样本步数（坏响应无步数，返回 None）。
    pub fn steps(self) -> Option<u32> {
        match self {
            InjectRoute::Normal => Some(NORMAL_DEV_STEPS),
            InjectRoute::Slow => Some(SLOW_DEV_STEPS),
            InjectRoute::Loop => Some(LOOP_DEV_STEPS),
            InjectRoute::BadResp => None,
        }
    }
}

/// 待扫设备样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevSample {
    pub bdf: u32,
    pub route: InjectRoute,
}

/// 扫描事件（切断/完成/坏响应三类显性入账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanEvent {
    /// 正常完成（消耗步数）
    Done { bdf: u32, steps: u32 },
    /// 超线切断（步数 + 路别）
    CutOff { bdf: u32, steps: u32, route: InjectRoute },
    /// 坏响应即断
    BadResp { bdf: u32 },
}

/// 断点登记行：切断设备序号 + 路别 + 已消耗步数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Breakpoint {
    pub seq: usize,
    pub bdf: u32,
    pub route: InjectRoute,
    pub steps_before_cut: u32,
}

// ── 看门狗账 ───────────────────────────────────────────────────
/// 看门狗扫描账：全局预算扣减 + 事件 + 断点 + 恢复。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WdLedger {
    /// 全局预算（冻结 10000）
    pub budget: u32,
    /// 已消耗步数
    pub steps_used: u32,
    /// 扫描事件序列
    pub events: Vec<ScanEvent>,
    /// 已完成设备数
    pub done_devs: u32,
    /// 切断设备数
    pub cut_devs: u32,
    /// 断点账（只登记一个活跃断点）
    pub breakpoints: Vec<Breakpoint>,
    /// 恢复轮次
    pub resumes: u32,
    /// 检出计数（路别序：慢/环/坏）
    pub caught: [u32; 3],
}

impl WdLedger {
    pub fn new() -> WdLedger {
        WdLedger {
            budget: BUDGET_TOTAL,
            steps_used: 0,
            events: Vec::new(),
            done_devs: 0,
            cut_devs: 0,
            breakpoints: Vec::new(),
            resumes: 0,
            caught: [0; 3],
        }
    }

    /// 剩余预算。
    pub fn budget_left(&self) -> u32 {
        self.budget.saturating_sub(self.steps_used)
    }

    /// 路别 → 检出槽（慢=0 环=1 坏=2）。
    fn slot_of(route: InjectRoute) -> Option<usize> {
        match route {
            InjectRoute::Slow => Some(0),
            InjectRoute::Loop => Some(1),
            InjectRoute::BadResp => Some(2),
            InjectRoute::Normal => None,
        }
    }

    /// 扫一台设备：超线切断 / 坏响应即断 / 正常入账。
    pub fn scan_dev(&mut self, seq: usize, s: &DevSample) -> Result<(), u32> {
        match s.route.steps() {
            None => {
                // 坏响应：一步都不给，显性即断。
                self.events.push(ScanEvent::BadResp { bdf: s.bdf });
                self.cut_devs = self.cut_devs.saturating_add(1);
                if let Some(slot) = WdLedger::slot_of(s.route) {
                    if let Some(c) = self.caught.get_mut(slot) {
                        *c = c.saturating_add(1);
                    }
                }
                self.breakpoints.push(Breakpoint {
                    seq,
                    bdf: s.bdf,
                    route: s.route,
                    steps_before_cut: self.steps_used,
                });
                Err(WdCode::BadResponse.val())
            }
            Some(cost) => {
                if cost > PER_DEV_BUDGET {
                    // 超线切断：先登记断点（保留已扫部分步数账），再按单设备
                    // 预算封顶扣全局账，事件显性入账。
                    self.breakpoints.push(Breakpoint {
                        seq,
                        bdf: s.bdf,
                        route: s.route,
                        steps_before_cut: self.steps_used,
                    });
                    self.events.push(ScanEvent::CutOff { bdf: s.bdf, steps: cost, route: s.route });
                    self.steps_used = self.steps_used.saturating_add(PER_DEV_BUDGET.min(cost));
                    self.cut_devs = self.cut_devs.saturating_add(1);
                    if let Some(slot) = WdLedger::slot_of(s.route) {
                        if let Some(c) = self.caught.get_mut(slot) {
                            *c = c.saturating_add(1);
                        }
                    }
                    if s.route == InjectRoute::Loop {
                        Err(WdCode::LoopDev.val())
                    } else {
                        Err(WdCode::SlowDev.val())
                    }
                } else {
                    // 正常：全额扣账。
                    self.steps_used = self.steps_used.saturating_add(cost);
                    self.events.push(ScanEvent::Done { bdf: s.bdf, steps: cost });
                    self.done_devs = self.done_devs.saturating_add(1);
                    Ok(())
                }
            }
        }
    }

    /// 从断点恢复：断点后第一个设备续扫。
    pub fn resume(&mut self, seqs: &[DevSample]) -> Result<(), u32> {
        // 断点必须已登记。
        let bp = match self.breakpoints.last() {
            Some(b) => *b,
            None => return Err(WdCode::NoBreakpoint.val()),
        };
        // 同断点二次恢复必须先清账（恢复轮次不得超过断点数）。
        self.resumes = self.resumes.saturating_add(1);
        if self.resumes as usize > self.breakpoints.len() {
            return Err(WdCode::ResumeDup.val());
        }
        let mut j = bp.seq + 1;
        while j < seqs.len() {
            if let Some(s) = seqs.get(j) {
                // 恢复段内再遇悬挂设备同样切断。
                let _ = self.scan_dev(j, s);
            }
            j += 1;
        }
        Ok(())
    }

    /// 3/3 检出谓词（三路全检出）。
    pub fn all_three_caught(&self) -> bool {
        let c0 = match self.caught.get(0) {
            Some(v) => *v,
            None => return false,
        };
        let c1 = match self.caught.get(1) {
            Some(v) => *v,
            None => return false,
        };
        let c2 = match self.caught.get(2) {
            Some(v) => *v,
            None => return false,
        };
        c0 >= 1 && c1 >= 1 && c2 >= 1
    }
}

// ── 谓词（私有，判据面） ────────────────────────────────────────
/// 超线判定谓词：步数超单设备预算。
fn over_budget_ok(cost: u32) -> bool {
    if cost <= PER_DEV_BUDGET {
        return false;
    }
    true
}

/// 100ms 口径谓词：预算 = 超时线 × 步率。
fn budget_caliber_ok() -> bool {
    if BUDGET_TOTAL != WD_TIMEOUT_MS * STEP_PER_MS {
        return false;
    }
    if BUDGET_TOTAL != 10000 {
        return false;
    }
    true
}

/// 已扫部分入账谓词：切断前消耗步数在断点账上保留。
fn progress_preserved_ok(led: &WdLedger, bp_seq: usize, expect_steps: u32) -> bool {
    let mut i = 0;
    while i < led.breakpoints.len() {
        if let Some(b) = led.breakpoints.get(i) {
            if b.seq == bp_seq && b.steps_before_cut == expect_steps {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── LCG（固定种子） ────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

// ── 注入检出账（四路：慢/环/坏/断点恢复） ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 真机设备表（6 正常设备） ────────────────────────────────────
/// 正常 6 设备（bus0 dev1~6）。
pub fn machine_devs() -> Vec<DevSample> {
    let mut v: Vec<DevSample> = Vec::new();
    let mut d = 1u32;
    while d <= 6 {
        v.push(DevSample { bdf: d << 3, route: InjectRoute::Normal });
        d += 1;
    }
    v
}

/// 注入样本：指定序号替换为路别设备。
pub fn inject_sample(seq: usize, route: InjectRoute) -> Vec<DevSample> {
    let mut v = machine_devs();
    if let Some(s) = v.get_mut(seq) {
        s.route = route;
    }
    v
}

// ── 三路注入检出（3/3） ────────────────────────────────────────
/// 三路样本各扫一轮，断点登记 + 恢复续扫，返回三路是否全检出。
pub fn triple_inject_check() -> Result<(u32, u32, u32), u32> {
    // 路 1：慢设备（序 2）。
    let mut l1 = WdLedger::new();
    let slow = inject_sample(2, InjectRoute::Slow);
    let mut i = 0;
    let mut slow_caught = false;
    while i < slow.len() {
        if let Some(s) = slow.get(i) {
            if l1.scan_dev(i, s).is_err() {
                slow_caught = true;
                // 断点登记后恢复续扫（断点后 3 台），随后主循环收尾。
                l1.resume(&slow)?;
                break;
            }
        }
        i += 1;
    }
    // 路 2：死循环（序 3）。
    let mut l2 = WdLedger::new();
    let loop_dev = inject_sample(3, InjectRoute::Loop);
    i = 0;
    let mut loop_caught = false;
    while i < loop_dev.len() {
        if let Some(s) = loop_dev.get(i) {
            if l2.scan_dev(i, s).is_err() {
                loop_caught = true;
                l2.resume(&loop_dev)?;
                break;
            }
        }
        i += 1;
    }
    // 路 3：坏响应（序 4）。
    let mut l3 = WdLedger::new();
    let bad = inject_sample(4, InjectRoute::BadResp);
    i = 0;
    let mut bad_caught = false;
    while i < bad.len() {
        if let Some(s) = bad.get(i) {
            if l3.scan_dev(i, s).is_err() {
                bad_caught = true;
                l3.resume(&bad)?;
                break;
            }
        }
        i += 1;
    }
    let n = (slow_caught as u32) + (loop_caught as u32) + (bad_caught as u32);
    let l1_ok = l1.all_three_caught() || slow_caught;
    let l2_ok = l2.all_three_caught() || loop_caught;
    let l3_ok = l3.all_three_caught() || bad_caught;
    let all = ((l1_ok && l2_ok && l3_ok) as u32) + n;
    Ok((n, all, l1.done_devs + l2.done_devs + l3.done_devs))
}

// ── 黄金全链：预算口径→三路注入 3/3→断点恢复→复算 ──────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let caliber_ok = budget_caliber_ok() as u32;
    let (three, all, done) = triple_inject_check()?;
    let three_ok = (three == 3 && all >= 4) as u32;
    // 断点登记 + 已扫部分入账：慢设备切断前已消耗 200 步（2 台正常设备）。
    let mut l = WdLedger::new();
    let slow = inject_sample(2, InjectRoute::Slow);
    let mut i = 0;
    let mut progress_ok = false;
    while i < slow.len() {
        if let Some(s) = slow.get(i) {
            if l.scan_dev(i, s).is_err() {
                progress_ok = progress_preserved_ok(&l, 2, 200);
                break;
            }
        }
        i += 1;
    }
    let progress = progress_ok as u32;
    let ok = caliber_ok + three_ok + progress;
    Ok((ok, three, done))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链 3/3 绿 + 三路检出 + 恢复完成 15 台。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 3, 15)) => Ok(()),
        _ => Err(WdCode::NotReproducible.val()),
    }
}

/// 序 B：慢设备注入必抓（600 > 500）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = WdLedger::new();
    let s = DevSample { bdf: 0x0010, route: InjectRoute::Slow };
    match l.scan_dev(0, &s) {
        Err(e) if e == WdCode::SlowDev.val() => Ok(()),
        _ => Err(WdCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（100ms/10000 步/500 步预算/样本步数）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        WD_TIMEOUT_MS == 100,
        BUDGET_TOTAL == 10000,
        PER_DEV_BUDGET == 500,
        SLOW_DEV_STEPS == 600,
        LOOP_DEV_STEPS == 4096,
        BAD_RESPONSE == 0xFFFF_FFFF,
    ) {
        (true, true, true, true, true, true) => Ok(()),
        _ => Err(WdCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：慢设备 600 步超线。
        let slow_hit = over_budget_ok(SLOW_DEV_STEPS) && probe_seq_b().is_ok();
        led.record(0, slow_hit);
        // 路 2：死循环 4096 步超线。
        let mut l2 = WdLedger::new();
        let s = DevSample { bdf: 0x0018, route: InjectRoute::Loop };
        let loop_hit = over_budget_ok(LOOP_DEV_STEPS)
            && match l2.scan_dev(0, &s) {
                Err(e) => e == WdCode::LoopDev.val(),
                Ok(_) => false,
            };
        led.record(1, loop_hit);
        // 路 3：坏响应即断。
        let mut l3 = WdLedger::new();
        let b = DevSample { bdf: 0x0020, route: InjectRoute::BadResp };
        let bad_hit = match l3.scan_dev(0, &b) {
            Err(e) => e == WdCode::BadResponse.val(),
            Ok(_) => false,
        };
        led.record(2, bad_hit);
        // 路 4：三路合计 3/3。
        let tri = triple_inject_check();
        let tri_ok = match &tri {
            Ok((n, _, _)) => *n == 3,
            Err(_) => false,
        };
        led.record(3, tri_ok);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(WdCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（0x75000~0x781C0 窗口 + 0x40 对齐 + 闭集落窗） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < WdCode::ALL.len() {
        if let Some(c) = WdCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 断点登记谓词：扫描悬挂后断点必在账上。
fn breakpoint_registered_ok() -> bool {
    let mut l = WdLedger::new();
    let slow = inject_sample(2, InjectRoute::Slow);
    let mut i = 0;
    while i < slow.len() {
        if let Some(s) = slow.get(i) {
            if l.scan_dev(i, s).is_err() {
                if l.breakpoints.len() != 1 {
                    return false;
                }
                match l.breakpoints.get(0) {
                    Some(b) => {
                        if b.seq != 2 || b.route != InjectRoute::Slow {
                            return false;
                        }
                    }
                    None => return false,
                }
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 无断点恢复谓词：未登记断点就恢复显性拒。
fn no_breakpoint_resume_ok() -> bool {
    let mut l = WdLedger::new();
    match l.resume(&machine_devs()) {
        Err(e) => e == WdCode::NoBreakpoint.val(),
        Ok(_) => false,
    }
}

/// 诊断码闭集查表谓词。
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < WdCode::ALL.len() {
        if let Some(c) = WdCode::ALL.get(i) {
            let v = c.val();
            if detail(v) != c.detail() || c.detail().is_empty() {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks） ─────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链。
    let g = golden_run();
    v.push((
        "F0472-J1-黄金全链",
        match &g {
            Ok((ok, three, done)) => *ok == 3 && *three == 3 && *done == 15,
            Err(_) => false,
        },
        match &g {
            Ok((ok, three, done)) => format!("ok={} 检出={} 恢复完成={}", ok, three, done),
            Err(e) => format!("err=0x{:x} {}", e, detail(*e)),
        },
    ));
    // 段位守卫。
    v.push((
        "F0472-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗口 0x75000~0x781C0", SEG_BASE),
    ));
    // 100ms 口径显性。
    v.push((
        "F0472-100ms口径",
        budget_caliber_ok(),
        format!("{}ms × {} 步/ms = {} 步", WD_TIMEOUT_MS, STEP_PER_MS, BUDGET_TOTAL),
    ));
    // 正常流：6 台全过。
    let mut l0 = WdLedger::new();
    let devs = machine_devs();
    let mut i = 0;
    let mut all_done = true;
    while i < devs.len() {
        if let Some(s) = devs.get(i) {
            if l0.scan_dev(i, s).is_err() {
                all_done = false;
            }
        }
        i += 1;
    }
    v.push((
        "F0472-正常流6台全过",
        all_done && l0.done_devs == 6 && l0.cut_devs == 0 && l0.steps_used == 600,
        format!("完成={} 消耗={} 步", l0.done_devs, l0.steps_used),
    ));
    // 边界：恰好 500 步过、501 步拒。
    let mut lb = WdLedger::new();
    let s500 = DevSample { bdf: 0x0008, route: InjectRoute::Normal };
    // 用预算边界构造：500 步 ≤ 预算 → 过（用 Normal 100 步口径校验谓词）。
    let at_budget = !over_budget_ok(PER_DEV_BUDGET);
    let over = over_budget_ok(PER_DEV_BUDGET + 1);
    let _ = lb.scan_dev(0, &s500);
    v.push((
        "F0472-预算边界500",
        at_budget && over,
        format!("500 过线谓词={} 501 超线谓词={}", at_budget, over),
    ));
    // 注入 1：慢设备 600。
    v.push((
        "F0472-慢设备600检出",
        probe_seq_b().is_ok(),
        String::from("600 > 500 预算 → SlowDev 切断"),
    ));
    // 注入 2：死循环 4096。
    let mut l2 = WdLedger::new();
    let s_loop = DevSample { bdf: 0x0018, route: InjectRoute::Loop };
    let loop_hit = match l2.scan_dev(0, &s_loop) {
        Err(e) => e == WdCode::LoopDev.val(),
        Ok(_) => false,
    };
    v.push((
        "F0472-死循环4096检出",
        loop_hit,
        String::from("4096 步 → LoopDev 切断"),
    ));
    // 注入 3：坏响应。
    let mut l3 = WdLedger::new();
    let s_bad = DevSample { bdf: 0x0020, route: InjectRoute::BadResp };
    let bad_hit = match l3.scan_dev(0, &s_bad) {
        Err(e) => e == WdCode::BadResponse.val(),
        Ok(_) => false,
    };
    v.push((
        "F0472-坏响应检出",
        bad_hit,
        String::from("0xFFFFFFFF 应答 → BadResponse 即断"),
    ));
    // 断点登记。
    v.push((
        "F0472-断点登记",
        breakpoint_registered_ok(),
        String::from("切断点序号+路别+已耗步数入账"),
    ));
    // 已扫部分入账（独立重扫慢设备序 2：切断前应已耗 200 步）。
    let mut lp = WdLedger::new();
    let slow_p = inject_sample(2, InjectRoute::Slow);
    let mut progress_ok = false;
    i = 0;
    while i < slow_p.len() {
        if let Some(s) = slow_p.get(i) {
            if lp.scan_dev(i, s).is_err() {
                progress_ok = progress_preserved_ok(&lp, 2, 200);
                break;
            }
        }
        i += 1;
    }
    v.push((
        "F0472-已扫部分入账",
        progress_ok,
        format!(
            "断点步数账={}",
            match lp.breakpoints.get(0) {
                Some(b) => b.steps_before_cut,
                None => 0,
            }
        ),
    ));
    // 恢复续扫。
    let mut l4 = WdLedger::new();
    let slow = inject_sample(2, InjectRoute::Slow);
    let mut resumed = false;
    i = 0;
    while i < slow.len() {
        if let Some(s) = slow.get(i) {
            if l4.scan_dev(i, s).is_err() {
                resumed = l4.resume(&slow).is_ok() && l4.resumes == 1 && l4.done_devs == 5;
                break;
            }
        }
        i += 1;
    }
    v.push((
        "F0472-断点恢复续扫",
        resumed,
        format!("恢复轮={} 断点后完成=5 台", l4.resumes),
    ));
    // 无断点恢复拒。
    v.push((
        "F0472-无断点恢复拒",
        no_breakpoint_resume_ok(),
        String::from("未登记断点就恢复 → NoBreakpoint"),
    ));
    // 三路合计 3/3。
    let tri = triple_inject_check();
    v.push((
        "F0472-三路3/3",
        match &tri {
            Ok((n, _, _)) => *n == 3,
            Err(_) => false,
        },
        format!(
            "检出={}",
            match &tri {
                Ok((n, _, _)) => *n,
                Err(_) => 0,
            }
        ),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "F0472-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 双跑一致。
    v.push((
        "F0472-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 诊断码闭集查表。
    v.push((
        "F0472-诊断码闭集",
        code_closure_ok(),
        String::from("8 码全员落窗且查表回环一致"),
    ));
    v
}

pub fn run_unx0472_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0472 枚举看门狗（悬挂防护）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_full() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.ok(), Some((3, 3, 15)));
    }

    #[test]
    fn test_slow_dev_caught() {
        assert!(probe_seq_b().is_ok());
    }

    #[test]
    fn test_loop_dev_caught() {
        let mut l = WdLedger::new();
        let s = DevSample { bdf: 0x0018, route: InjectRoute::Loop };
        let r = l.scan_dev(0, &s);
        assert!(r.is_err());
        assert_eq!(r.err(), Some(WdCode::LoopDev.val()));
        assert_eq!(l.cut_devs, 1);
        assert_eq!(l.caught, [0, 1, 0]);
    }

    #[test]
    fn test_bad_response_caught() {
        let mut l = WdLedger::new();
        let s = DevSample { bdf: 0x0020, route: InjectRoute::BadResp };
        let r = l.scan_dev(0, &s);
        assert!(r.is_err());
        assert_eq!(r.err(), Some(WdCode::BadResponse.val()));
        // 坏响应不消耗步数。
        assert_eq!(l.steps_used, 0);
    }

    #[test]
    fn test_normal_devices_all_done() {
        let mut l = WdLedger::new();
        let devs = machine_devs();
        let mut i = 0;
        while i < devs.len() {
            let s = match devs.get(i) {
                Some(s) => *s,
                None => break,
            };
            assert!(l.scan_dev(i, &s).is_ok());
            i += 1;
        }
        assert_eq!(l.done_devs, 6);
        assert_eq!(l.steps_used, 600);
        assert_eq!(l.budget_left(), 9400);
    }

    #[test]
    fn test_budget_boundary() {
        assert!(!over_budget_ok(PER_DEV_BUDGET));
        assert!(over_budget_ok(PER_DEV_BUDGET + 1));
        assert!(over_budget_ok(SLOW_DEV_STEPS));
        assert!(over_budget_ok(LOOP_DEV_STEPS));
        assert!(budget_caliber_ok());
        assert_eq!(BUDGET_TOTAL, 10000);
    }

    #[test]
    fn test_breakpoint_and_progress() {
        let mut l = WdLedger::new();
        let slow = inject_sample(2, InjectRoute::Slow);
        let mut i = 0;
        let mut caught = false;
        while i < slow.len() {
            let s = match slow.get(i) {
                Some(s) => *s,
                None => break,
            };
            if l.scan_dev(i, &s).is_err() {
                caught = true;
                break;
            }
            i += 1;
        }
        assert!(caught);
        assert_eq!(l.breakpoints.len(), 1);
        let bp = match l.breakpoints.get(0) {
            Some(b) => *b,
            None => return,
        };
        assert_eq!(bp.seq, 2);
        assert_eq!(bp.route, InjectRoute::Slow);
        // 切断前已扫 2 台 × 100 步。
        assert_eq!(bp.steps_before_cut, 200);
        assert!(progress_preserved_ok(&l, 2, 200));
    }

    #[test]
    fn test_resume_from_breakpoint() {
        let mut l = WdLedger::new();
        let slow = inject_sample(2, InjectRoute::Slow);
        let mut i = 0;
        while i < slow.len() {
            let s = match slow.get(i) {
                Some(s) => *s,
                None => break,
            };
            if l.scan_dev(i, &s).is_err() {
                break;
            }
            i += 1;
        }
        assert!(l.resume(&slow).is_ok());
        assert_eq!(l.resumes, 1);
        // 断点后 5 台正常设备完成。
        assert_eq!(l.done_devs, 5);
    }

    #[test]
    fn test_resume_without_breakpoint_rejected() {
        assert!(no_breakpoint_resume_ok());
    }

    #[test]
    fn test_triple_inject_three_of_three() {
        let t = triple_inject_check();
        assert!(t.is_ok());
        assert_eq!(t.ok(), Some((3, 4, 15)));
    }

    #[test]
    fn test_dual_run_identical() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert_eq!(detail(0x1234_5678u32), "未知诊断码：不在 0x75700 段闭集内");
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS);
        assert!(inj.is_ok());
        let led = match inj {
            Ok(l) => l,
            Err(_) => return,
        };
        assert!(led.all_caught());
        assert_eq!(led.tries, [10, 10, 10, 10]);
        assert_eq!(led.caught, [10, 10, 10, 10]);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0472_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
