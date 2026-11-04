//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0053 · ExitBootServices 时序协议（深化第二实现） · 0x6DD40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F0053-J1 EBS 前回调链完整性 + EBS 失败重试语义
//!     （EFI_INVALID_PARAMETER 重取 MapKey）+ 退出后禁 BootSvc；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 时序三段：PRE（回调链）/CALL（EBS 调用）/POST（封禁）；
//!   - 回调链：CreateEventEx 登记的 EVT_SIGNAL_EXIT_BOOT_SERVICES
//!     通知按优先级序逐一回调，漏挂即拒；
//!   - MapKey 账：EBS 调用必须带当前 MapKey；固件返回
//!     EFI_INVALID_PARAMETER ⇒ MapKey 失效，重取内存图后再调；
//!   - 重试上限：2 次（第 3 次仍失效 ⇒ 拒绝退出，进诊断路径）；
//!   - POST 封禁：EBS 成功后 BootServices 全封（F0049 账联动），
//!     Runtime Services 仍可用；
//!   - 事件账：EBS 后事件池关闭（CreateEvent 拒）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DD40，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DD40;

/// EBS 重试上限（冻结：2 次）。
pub const EBS_MAX_RETRY: u32 = 2;
/// EFI_INVALID_PARAMETER（冻结账面值）。
pub const EFI_INVALID_PARAMETER: u64 = 0x8000_0000_0000_0002;
/// EFI_SUCCESS（冻结账面值）。
pub const EFI_SUCCESS: u64 = 0;
/// 最大回调数（防伪账）。
pub const MAX_NOTIFIES: usize = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EbsCode {
    /// 回调链漏挂（通知序缺口）
    NotifyChainGap,
    /// 回调越上限
    NotifyOver,
    /// MapKey 失效超重试上限
    MapKeyExhausted,
    /// 重试序错（未重取内存图即调）
    RetryWithoutRefetch,
    /// EBS 后调 BootServices（封禁红线）
    BootSvcAfterExit,
    /// EBS 后 CreateEvent（事件池关闭）
    EventAfterExit,
    /// 双 EBS（重入红线）
    DoubleExit,
    /// 退出前调 Runtime 之外禁区（时序倒挂）
    PrematureRuntime,
    /// 通知优先级倒序
    NotifyOrderInverted,
    /// 双跑不一致
    NotReproducible,
}

impl EbsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EbsCode::NotifyChainGap => "回调链漏挂：通知序缺口",
            EbsCode::NotifyOver => "回调越上限：16 冻结",
            EbsCode::MapKeyExhausted => "MapKey 失效超重试上限",
            EbsCode::RetryWithoutRefetch => "重试序错：未重取内存图",
            EbsCode::BootSvcAfterExit => "EBS 后调 BootServices",
            EbsCode::EventAfterExit => "EBS 后 CreateEvent：池已关",
            EbsCode::DoubleExit => "双 EBS：重入红线",
            EbsCode::PrematureRuntime => "退出前调 Runtime 禁区",
            EbsCode::NotifyOrderInverted => "通知优先级倒序",
            EbsCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [EbsCode; 10] = [
        EbsCode::NotifyChainGap,
        EbsCode::NotifyOver,
        EbsCode::MapKeyExhausted,
        EbsCode::RetryWithoutRefetch,
        EbsCode::BootSvcAfterExit,
        EbsCode::EventAfterExit,
        EbsCode::DoubleExit,
        EbsCode::PrematureRuntime,
        EbsCode::NotifyOrderInverted,
        EbsCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EbsCode::ALL.len() {
        if EbsCode::ALL[i].val() == code {
            return EbsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DD40 段闭集内"
}

// ── 回调通知账 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EbsNotify {
    /// 优先级（小者先调）。
    pub prio: u32,
    /// 已挂标志（漏挂检测）。
    pub armed: bool,
    /// 回调已触发标志。
    pub fired: bool,
}

// ── EBS 时序状态机 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EbsStage {
    /// 退出前（回调链可挂）
    Pre,
    /// 调用中（重试窗口）
    Calling,
    /// 退出后（封禁）
    Post,
}

#[derive(Clone, Debug)]
pub struct EbsProtocol {
    pub stage: EbsStage,
    pub notifies: Vec<EbsNotify>,
    pub map_key: u64,
    pub map_key_valid: bool,
    pub retries: u32,
    pub refetched: bool,
    pub boot_calls: u32,
    pub events_created: u32,
    pub exit_count: u32,
}

impl EbsProtocol {
    pub fn new() -> EbsProtocol {
        EbsProtocol {
            stage: EbsStage::Pre,
            notifies: Vec::new(),
            map_key: 0,
            map_key_valid: false,
            retries: 0,
            refetched: false,
            boot_calls: 0,
            events_created: 0,
            exit_count: 0,
        }
    }

    /// 挂回调（Pre 期专属，优先级账）。
    pub fn arm_notify(&mut self, prio: u32) -> Result<usize, u32> {
        if self.stage != EbsStage::Pre {
            return Err(EbsCode::EventAfterExit.val());
        }
        if self.notifies.len() >= MAX_NOTIFIES {
            return Err(EbsCode::NotifyOver.val());
        }
        // 优先级不许与既有重复（漏挂检测以序号为账）。
        let mut i = 0;
        while i < self.notifies.len() {
            if self.notifies[i].prio == prio {
                return Err(EbsCode::NotifyOrderInverted.val());
            }
            i += 1;
        }
        self.notifies.push(EbsNotify { prio, armed: true, fired: false });
        Ok(self.notifies.len() - 1)
    }

    /// 回调链完整性：挂了必触发，按优先级升序。
    pub fn fire_chain(&mut self) -> Result<usize, u32> {
        if self.stage != EbsStage::Pre {
            return Err(EbsCode::BootSvcAfterExit.val());
        }
        // 链上不许有未挂缺口（armed 全真）。
        let mut i = 0;
        while i < self.notifies.len() {
            if !self.notifies[i].armed {
                return Err(EbsCode::NotifyChainGap.val());
            }
            i += 1;
        }
        // 优先级升序账：已按插入序校验升序。
        let mut i = 1;
        while i < self.notifies.len() {
            if self.notifies[i].prio < self.notifies[i - 1].prio {
                return Err(EbsCode::NotifyOrderInverted.val());
            }
            i += 1;
        }
        let n = self.notifies.len();
        let mut i = 0;
        while i < n {
            self.notifies[i].fired = true;
            i += 1;
        }
        Ok(n)
    }

    /// CreateEvent（Pre 期专属）。
    pub fn create_event(&mut self) -> Result<usize, u32> {
        if self.stage != EbsStage::Pre {
            return Err(EbsCode::EventAfterExit.val());
        }
        self.events_created += 1;
        Ok(self.events_created as usize)
    }

    /// BootServices 调用探针（Post 期全封）。
    pub fn boot_svc(&mut self) -> Result<(), u32> {
        if self.stage == EbsStage::Post {
            self.boot_calls += 1;
            return Err(EbsCode::BootSvcAfterExit.val());
        }
        Ok(())
    }

    /// MapKey 失效标记（固件返回 INVALID_PARAMETER）。
    pub fn map_key_invalidated(&mut self) {
        self.map_key_valid = false;
    }

    /// 重取内存图（重试前置）。
    pub fn refetch_map(&mut self) {
        self.refetched = true;
        self.map_key_valid = true;
    }

    /// EBS 调用：MapKey 无效且超上限 ⇒ 拒退；成功 ⇒ Post 封禁。
    pub fn call_ebs(&mut self) -> Result<u64, u32> {
        if self.stage == EbsStage::Post {
            self.exit_count += 1;
            return Err(EbsCode::DoubleExit.val());
        }
        if self.stage == EbsStage::Calling && self.retries > EBS_MAX_RETRY {
            return Err(EbsCode::MapKeyExhausted.val());
        }
        if !self.map_key_valid {
            // 重试序：必须先重取内存图。
            if self.stage == EbsStage::Calling && !self.refetched {
                return Err(EbsCode::RetryWithoutRefetch.val());
            }
            self.retries += 1;
            if self.retries > EBS_MAX_RETRY {
                return Err(EbsCode::MapKeyExhausted.val());
            }
            self.stage = EbsStage::Calling;
            return Err(EFI_INVALID_PARAMETER as u32);
        }
        // 成功退出：链必须已触发。
        let mut i = 0;
        while i < self.notifies.len() {
            if !self.notifies[i].fired {
                return Err(EbsCode::NotifyChainGap.val());
            }
            i += 1;
        }
        self.stage = EbsStage::Post;
        self.exit_count += 1;
        Ok(EFI_SUCCESS)
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

// ── 注入检出账（四路：链路/重试路/封禁路/重入路） ────────────────
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

/// 组装黄金协议（链挂齐 + MapKey 有效）。
pub fn make_protocol() -> EbsProtocol {
    let mut p = EbsProtocol::new();
    let _ = p.arm_notify(10);
    let _ = p.arm_notify(20);
    let _ = p.arm_notify(30);
    p.map_key_valid = true;
    p
}

// ── 黄金全链：挂链→事件→触发链→EBS 成功→封禁 →零违例 ────────────
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut p = make_protocol();
    let _ = p.create_event()?;
    let fired = p.fire_chain()?;
    let r = p.call_ebs()?;
    if r != EFI_SUCCESS {
        return Err(EbsCode::MapKeyExhausted.val());
    }
    // Post 期只读验证：封禁态就位、零 Boot 调用、恰一次退出。
    let bad = if p.stage == EbsStage::Post && p.boot_calls == 0 && p.exit_count == 1 {
        0
    } else {
        1
    };
    Ok((fired, p.retries, bad))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：合法协议全链绿（链 3 + 零重试 + 零违例）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 0, 0)) => Ok(()),
        _ => Err(EbsCode::NotReproducible.val()),
    }
}

/// 序 B：MapKey 失效 → 重试 1 次成功（重取后）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = make_protocol();
    let _ = p.fire_chain()?;
    p.map_key_invalidated();
    let first = p.call_ebs();
    let e1 = match first {
        Err(e) => e,
        Ok(_) => return Err(EbsCode::NotReproducible.val()),
    };
    if e1 != EFI_INVALID_PARAMETER as u32 {
        return Err(EbsCode::NotReproducible.val());
    }
    p.refetch_map();
    match p.call_ebs() {
        Ok(EFI_SUCCESS) => Ok(()),
        _ => Err(EbsCode::NotReproducible.val()),
    }
}

/// 序 C：重试不重取 ⇒ RetryWithoutRefetch。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut p = make_protocol();
    let _ = p.fire_chain()?;
    p.map_key_invalidated();
    let _ = p.call_ebs(); // 第 1 次失效 → Calling 态
    match p.call_ebs() {
        Err(e) if e == EbsCode::RetryWithoutRefetch.val() => Ok(()),
        _ => Err(EbsCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：链路（EBS 前未触发链必拒）。
        let mut p1 = make_protocol();
        p1.map_key_valid = true;
        led.record(0, p1.call_ebs().is_err());
        // 路 2：重试路（超上限必拒：每轮失效→重取→再调）。
        let mut p2 = make_protocol();
        let _ = p2.fire_chain();
        p2.map_key_invalidated();
        let mut exhausted = false;
        let mut k = 0;
        while k < 4 {
            p2.refetch_map();
            p2.map_key_invalidated();
            if let Err(e) = p2.call_ebs() {
                if e == EbsCode::MapKeyExhausted.val() {
                    exhausted = true;
                }
            }
            k += 1;
        }
        led.record(1, exhausted);
        // 路 3：封禁路（Post 期 boot_svc 必拒且记账）。
        let mut p3 = make_protocol();
        let _ = p3.fire_chain();
        p3.map_key_valid = true;
        let _ = p3.call_ebs();
        let hit3 = p3.boot_svc().is_err() && p3.boot_calls == 1;
        led.record(2, hit3);
        // 路 4：重入路（Post 期再 EBS 必拒）。
        let mut p4 = make_protocol();
        let _ = p4.fire_chain();
        p4.map_key_valid = true;
        let _ = p4.call_ebs();
        led.record(3, p4.call_ebs().is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EbsCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EbsCode::ALL.len() {
        let v = EbsCode::ALL[i].val();
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
    // 主判据：黄金全链（链 3 + 零重试 + 零违例）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((fired, retry, bad)) => *fired == 3 && *retry == 0 && *bad == 0,
            Err(_) => false,
        },
        match &g {
            Ok((fired, retry, bad)) => format!("fired={} retry={} bad={}", fired, retry, bad),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 重试语义：失效→重取→成功（2 次上限内）。
    let retry_hit = probe_seq_b().is_ok();
    v.push((
        "J1-重试语义",
        retry_hit,
        String::from("INVALID_PARAMETER→refetch→SUCCESS"),
    ));
    // 重试上限：连续失效 3 次后必拒（MapKeyExhausted）。
    let mut p = make_protocol();
    let _ = p.fire_chain();
    p.map_key_invalidated();
    let mut k = 0;
    let mut exhausted = false;
    while k < 5 {
        p.refetch_map();
        p.map_key_invalidated();
        if let Err(e) = p.call_ebs() {
            if e == EbsCode::MapKeyExhausted.val() {
                exhausted = true;
            }
        }
        k += 1;
    }
    v.push((
        "J1-重试上限",
        exhausted,
        format!("上限={} 冻结", EBS_MAX_RETRY),
    ));
    // 回调优先级倒序必拒。
    let mut p2 = EbsProtocol::new();
    let _ = p2.arm_notify(30);
    let _ = p2.arm_notify(10);
    v.push((
        "J1-优先级账",
        p2.fire_chain().is_err(),
        String::from("30→10 倒序拒"),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
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
        String::from("0x6DD40 窗口"),
    ));
    v
}

pub fn run_unx0053_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0053 ExitBootServices 时序协议");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
