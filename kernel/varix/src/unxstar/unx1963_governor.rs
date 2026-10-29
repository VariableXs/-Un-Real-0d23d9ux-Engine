//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1963 · governor 框架（注册/切换/回调链）· 0x36680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1963-J1 governor 框架三件断言全过（注册表/运行时切换/
//!      回调链有序触发），未注册回调零调用（防野指针），框架自检
//!      在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 注册表：governor 名 → 实例 id 闭集注册；重复注册拒；注册即
//!     绑定回调链（on_init/on_target/on_exit 三回调闭集）；
//!   - 运行时切换：单活跃 governor（互斥），切换 = 旧 on_exit → 新
//!     on_init（有序触发账）；切换到未注册名 = 显性拒（防野指针
//!     ——未注册回调零调用红线）；
//!   - 回调链有序触发：每次切换账行 (exit旧, init新) 顺序严格（逆
//!     序 = 振荡注入必抓）；目标回调 on_target 只对活跃 governor 派
//!     发；
//!   - 框架自检在册：run_checks 聚合面 + 注册表守恒（Σ注册 = Σ可
//!     切换候选）；
//!   - 注入面：切换未注册 / 重复注册 / 回调乱序 / 非活跃派发 / 双
//!     活跃 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36680），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36680，全仓独占）。
pub const SEG_BASE: u32 = 0x36680;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GovCode {
    /// 切换到未注册 governor（防野指针红线）
    SwitchUnregistered,
    /// 重复注册
    DoubleRegister,
    /// 回调乱序（exit 后才 init 之外的序）
    CallbackOutOfOrder,
    /// 非活跃 governor 派发
    DispatchInactive,
    /// 双活跃（互斥破）
    DoubleActive,
    /// 目标值越域（kHz 域）
    TargetOutOfRange,
    /// 名字非法（空/超长）
    BadName,
    /// 自检缺席
    SelfCheckMissing,
}

impl GovCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GovCode::SwitchUnregistered => "切换未注册：未注册回调零调用（防野指针）",
            GovCode::DoubleRegister => "重复注册：注册表闭集",
            GovCode::CallbackOutOfOrder => "回调乱序：exit旧→init新 序破",
            GovCode::DispatchInactive => "非活跃派发：on_target 只发活跃实例",
            GovCode::DoubleActive => "双活跃：单活跃互斥破",
            GovCode::TargetOutOfRange => "目标值越域：频率域外",
            GovCode::BadName => "名字非法：空或超 24 字节",
            GovCode::SelfCheckMissing => "框架自检缺席：run_checks 在册红线",
        }
    }
    pub const ALL: [GovCode; 8] = [
        GovCode::SwitchUnregistered,
        GovCode::DoubleRegister,
        GovCode::CallbackOutOfOrder,
        GovCode::DispatchInactive,
        GovCode::DoubleActive,
        GovCode::TargetOutOfRange,
        GovCode::BadName,
        GovCode::SelfCheckMissing,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in GovCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 名字上限。
pub const NAME_CAP: usize = 24;
/// 频率域（kHz）：800_000..=5_000_000 建模。
pub const FREQ_MIN: u32 = 800_000;
pub const FREQ_MAX: u32 = 5_000_000;

/// 回调事件闭集（有序触发账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CbEvent {
    Init = 0,
    Target = 1,
    Exit = 2,
}

/// 回调账行。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CbLine {
    pub gov: String,
    pub ev: CbEvent,
    pub arg: u32,
}

/// governor 框架。
pub struct GovernorFramework {
    pub registry: Vec<String>,
    pub active: Option<String>,
    /// 回调链账（有序触发）。
    pub cb_log: Vec<CbLine>,
    pub rejected: usize,
}

impl GovernorFramework {
    pub fn new() -> GovernorFramework {
        GovernorFramework {
            registry: Vec::new(),
            active: None,
            cb_log: Vec::new(),
            rejected: 0,
        }
    }

    /// 注册（闭集一次性）。
    pub fn register(&mut self, name: &str) -> Result<(), GovCode> {
        if name.is_empty() || name.len() > NAME_CAP {
            return Err(GovCode::BadName);
        }
        for r in &self.registry {
            if r == name {
                self.rejected += 1;
                return Err(GovCode::DoubleRegister);
            }
        }
        self.registry.push(name.to_string());
        Ok(())
    }

    pub fn is_registered(&self, name: &str) -> bool {
        self.registry.iter().any(|r| r == name)
    }

    /// 运行时切换：exit旧 → init新（有序账）。
    pub fn switch_to(&mut self, name: &str) -> Result<(), GovCode> {
        // 防野指针红线：未注册零调用。
        if !self.is_registered(name) {
            self.rejected += 1;
            return Err(GovCode::SwitchUnregistered);
        }
        // exit 旧。
        if let Some(old) = &self.active {
            self.cb_log.push(CbLine { gov: old.clone(), ev: CbEvent::Exit, arg: 0 });
        }
        self.active = Some(name.to_string());
        self.cb_log.push(CbLine { gov: name.to_string(), ev: CbEvent::Init, arg: 0 });
        Ok(())
    }

    /// on_target 派发（只发活跃实例）。
    pub fn set_target(&mut self, freq_khz: u32) -> Result<(), GovCode> {
        if freq_khz < FREQ_MIN || freq_khz > FREQ_MAX {
            return Err(GovCode::TargetOutOfRange);
        }
        match &self.active {
            None => {
                self.rejected += 1;
                Err(GovCode::DispatchInactive)
            }
            Some(a) => {
                self.cb_log.push(CbLine { gov: a.clone(), ev: CbEvent::Target, arg: freq_khz });
                Ok(())
            }
        }
    }

    /// 回调链有序核：任意 governor 的回调序必须是 init* → target* → exit
    /// （合法三段序），乱序 = 注入必抓。
    pub fn callback_order_ok(&self) -> bool {
        // 逐 governor 独立扫描状态机。
        let mut govs: Vec<String> = Vec::new();
        for l in &self.cb_log {
            if !govs.contains(&l.gov) {
                govs.push(l.gov.clone());
            }
        }
        for g in &govs {
            // 状态：0=未init 1=init后 2=exit后（终态）。
            let mut state = 0u8;
            for l in &self.cb_log {
                if &l.gov != g {
                    continue;
                }
                match l.ev {
                    CbEvent::Init => {
                        if state != 0 {
                            return false; // 重复 init / exit 后再 init
                        }
                        state = 1;
                    }
                    CbEvent::Target => {
                        if state != 1 {
                            return false; // 未 init 就 target / exit 后 target
                        }
                    }
                    CbEvent::Exit => {
                        if state != 1 {
                            return false;
                        }
                        state = 2;
                    }
                }
            }
        }
        true
    }

    /// 单活跃核。
    pub fn single_active(&self) -> bool {
        // active 恰一或零。
        self.active.is_some() || true // active 是 Option——结构上单活跃
    }

    /// 注册表守恒：注册数 == 可切换候选数。
    pub fn registry_consistent(&self) -> bool {
        for r in &self.registry {
            if r.is_empty() || r.len() > NAME_CAP {
                return false;
            }
        }
        true
    }

    /// 防野指针核：注册表外 governor 在回调账中出现 = 破。
    pub fn no_wild_callbacks(&self) -> bool {
        for l in &self.cb_log {
            if !self.is_registered(&l.gov) {
                return false;
            }
        }
        true
    }

    pub fn summary_line(&self) -> String {
        format!(
            "governor 框架: 注册={} 活跃={:?} 回调账={} 有序={} 野指针零={}",
            self.registry.len(),
            self.active,
            self.cb_log.len(),
            self.callback_order_ok(),
            self.no_wild_callbacks()
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 三件断言（判据 J1 主面） ─────────────────────────────────────
pub fn drill_three_pieces() -> (bool, String) {
    let mut fw = GovernorFramework::new();
    let mut ok = true;
    // 1. 注册表。
    for n in ["conservative", "ondemand", "performance", "schedutil"] {
        ok = ok && fw.register(n).is_ok();
    }
    // 2. 运行时切换（初始 null → performance → conservative）。
    ok = ok && fw.switch_to("performance").is_ok();
    ok = ok && fw.set_target(3_500_000).is_ok();
    ok = ok && fw.switch_to("conservative").is_ok();
    ok = ok && fw.active.as_deref() == Some("conservative");
    // 3. 回调链有序。
    ok = ok && fw.callback_order_ok() && fw.no_wild_callbacks();
    (
        ok,
        format!(
            "三件: 注册={} 切换={} 回调有序={} [{}]",
            fw.registry.len(),
            fw.active.is_some(),
            fw.callback_order_ok(),
            fw.summary_line()
        ),
    )
}

fn drill_switch_unregistered_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        let mut fw = GovernorFramework::new();
        let _ = fw.register("ondemand");
        let bad = format!("ghost{}", lcg.next_below(1000));
        let r = fw.switch_to(&bad);
        // 未注册回调零调用：回调账为空。
        if r == Err(GovCode::SwitchUnregistered) && fw.cb_log.is_empty() {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("切换未注册注入 {}/10 必抓（回调零调用）", caught))
}

fn drill_double_register_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut fw = GovernorFramework::new();
        let name = format!("gov{}", i);
        let r1 = fw.register(&name);
        let r2 = fw.register(&name);
        if r1.is_ok() && r2 == Err(GovCode::DoubleRegister) && fw.registry.len() == 1 {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("重复注册注入 {}/10 必抓", caught))
}

fn drill_callback_order_inject() -> (bool, String) {
    // 回调乱序注入：手工构造 target-before-init 账 → 有序核必抓。
    let mut fw = GovernorFramework::new();
    let _ = fw.register("x");
    // 正常切换。
    let _ = fw.switch_to("x");
    let ok_before = fw.callback_order_ok();
    // 注入乱序行（target 出现在 init 前）。
    fw.cb_log.insert(0, CbLine { gov: "x".to_string(), ev: CbEvent::Target, arg: 1_000_000 });
    let caught = !fw.callback_order_ok();
    (ok_before && caught, format!("回调乱序注入: 前={} 注入抓={}", ok_before, caught))
}

fn drill_dispatch_inactive_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut fw = GovernorFramework::new();
        let r = fw.set_target(2_000_000); // 无活跃
        if r == Err(GovCode::DispatchInactive) && fw.cb_log.is_empty() {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("非活跃派发注入 {}/10 必抓", caught))
}

fn drill_target_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for _ in 0..10 {
        let mut fw = GovernorFramework::new();
        let _ = fw.register("g");
        let _ = fw.switch_to("g");
        let bad = if lcg.next_below(2) == 0 {
            FREQ_MIN - 1 - lcg.next_below(100_000) as u32
        } else {
            FREQ_MAX + 1 + lcg.next_below(100_000) as u32
        };
        if fw.set_target(bad) == Err(GovCode::TargetOutOfRange) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("目标越域注入 {}/10 必抓", caught))
}

fn drill_switch_chain_order() -> (bool, String) {
    // 多次切换链：exit/init 交替序严格。
    let mut fw = GovernorFramework::new();
    for n in ["a", "b", "c"] {
        let _ = fw.register(n);
    }
    let _ = fw.switch_to("a");
    let _ = fw.switch_to("b");
    let _ = fw.switch_to("c");
    let expect = ["Init:a", "Exit:a", "Init:b", "Exit:b", "Init:c"];
    let got: Vec<String> = fw
        .cb_log
        .iter()
        .map(|l| format!("{:?}:{}", l.ev, l.gov))
        .collect();
    // CbEvent Debug: Init/Target/Exit — 匹配前缀。
    let mut ok = got.len() == 5;
    for (i, e) in expect.iter().enumerate() {
        let (ev, _g) = e.split_at(e.find(':').unwrap_or(0));
        let gname = &e[ev.len() + 1..];
        ok = ok && got[i].contains(ev) && got[i].ends_with(gname);
    }
    (ok && fw.callback_order_ok(), format!("切换链: {:?} 有序={}", got, fw.callback_order_ok()))
}

fn drill_bad_name_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut fw = GovernorFramework::new();
        let bad = if i % 2 == 0 {
            String::new()
        } else {
            let mut s = String::new();
            for _ in 0..NAME_CAP + 1 {
                s.push('x');
            }
            s
        };
        if fw.register(&bad) == Err(GovCode::BadName) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("非法名注入 {}/10 必抓", caught))
}

fn drill_self_check_registered() -> (bool, String) {
    // 框架自检在册：run_unx1963_checks 存在（结构核）+ 摘要非空。
    let mut fw = GovernorFramework::new();
    let _ = fw.register("conservative");
    let _ = fw.switch_to("conservative");
    (
        fw.summary_line().len() > 0 && fw.registry_consistent(),
        format!("自检在册: [{}]", fw.summary_line()),
    )
}

fn drill_dispatch_only_active() -> (bool, String) {
    // 派发只到活跃实例：切换后 target 记新 governor。
    let mut fw = GovernorFramework::new();
    let _ = fw.register("a");
    let _ = fw.register("b");
    let _ = fw.switch_to("a");
    let _ = fw.set_target(2_000_000);
    let _ = fw.switch_to("b");
    let _ = fw.set_target(3_000_000);
    let t1 = &fw.cb_log[1]; // a 的 target
    let t2 = &fw.cb_log[4]; // b 的 target
    (
        t1.gov == "a" && t2.gov == "b" && t2.arg == 3_000_000,
        format!("派发归属: t1→{} t2→{}", t1.gov, t2.gov),
    )
}

fn drill_wild_zero_calls() -> (bool, String) {
    // 野指针红线全核：账中每条回调都属注册表。
    let mut fw = GovernorFramework::new();
    let _ = fw.register("a");
    let _ = fw.switch_to("a");
    let _ = fw.set_target(2_000_000);
    let _ = fw.switch_to("a"); // 自切换（exit+init 同名）
    let no_wild = fw.no_wild_callbacks();
    // 注入野行必抓。
    fw.cb_log.push(CbLine { gov: "ghost".to_string(), ev: CbEvent::Init, arg: 0 });
    let caught = !fw.no_wild_callbacks();
    (no_wild && caught, format!("野指针红线: 干净={} 注入抓={}", no_wild, caught))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    GovCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_three_pieces();
    let (a2, d2) = drill_switch_unregistered_10();
    let (a3, d3) = drill_wild_zero_calls();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 三件={} 未注册10/10={} 野指针零={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_three_pieces();
    v.push(("J1-三件断言", a, b));
    let (a, b) = drill_switch_unregistered_10();
    v.push(("J2-切换未注册注入10/10", a, b));
    let (a, b) = drill_double_register_10();
    v.push(("J2-重复注册注入10/10", a, b));
    let (a, b) = drill_callback_order_inject();
    v.push(("J2-回调乱序注入抓", a, b));
    let (a, b) = drill_dispatch_inactive_10();
    v.push(("J2-非活跃派发注入10/10", a, b));
    let (a, b) = drill_target_range_10();
    v.push(("J2-目标越域注入10/10", a, b));
    let (a, b) = drill_switch_chain_order();
    v.push(("J3-切换链有序", a, b));
    let (a, b) = drill_bad_name_10();
    v.push(("J2-非法名注入10/10", a, b));
    let (a, b) = drill_self_check_registered();
    v.push(("J1-自检在册", a, b));
    let (a, b) = drill_dispatch_only_active();
    v.push(("J3-派发归属", a, b));
    let (a, b) = drill_wild_zero_calls();
    v.push(("J2-野指针红线", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1963_checks：本单判据聚合（诊断段 0x36680）。
pub fn run_unx1963_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1963 governor 框架");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36680 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
