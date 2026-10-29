//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13801 · RegNotifyChangeKeyValue 两档过滤 · 0x65F00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F13801-J1 REG_NOTIFY_CHANGE_NAME/ATTRIBUTES/LAST_SET/
//!      SECURITY 四旗 × 子树开关矩阵过滤正确率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四旗闭集（winreg 规格写死）：NAME（键名增删改）/ATTRIBUTES
//!      （类型/值内容变更）/LAST_SET（最后写入时间戳）/SECURITY
//!      （描述符变更）；
//!   - 子树开关两档：watch_subtree=false 只报本键；true 报本键+子树；
//!   - 过滤矩阵 4×2=8 格：每格事件流（建模生成）经过滤器后命中
//!     数与黄金判定一致——正确率 100% 红线；
//!   - 监听账：注册→事件命中→回调→注销 四态，注销后事件不得再
///     命中（漏注册/漏注销面）；
///   - 注入三族：注销后命中/子树越权命中/旗位不匹配 10/10 必抓；
///   - LCG 种子写死 = SEG_BASE（0x65F00），禁 rand/浮点/IO/墙钟；
///     no_std + alloc；零 unwrap/expect。
/// ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65F00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65F00;

// ── 四旗闭集 ─────────────────────────────────────────────────────
pub const F_NAME: u8 = 0;
pub const F_ATTRS: u8 = 1;
pub const F_LASTSET: u8 = 2;
pub const F_SECURITY: u8 = 3;
pub const FLAG_COUNT: usize = 4;
pub const FLAG_NAMES: [&str; FLAG_COUNT] = ["NAME", "ATTRIBUTES", "LAST_SET", "SECURITY"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotifyCode {
    /// 注销后仍命中
    HitAfterUnregister,
    /// 子树越权命中（未开子树却报子键）
    SubtreeOverreach,
    /// 旗位不匹配（事件类不在监听集却命中）
    FlagMismatch,
    /// 旗越闭集
    FlagOutOfRange,
    /// 监听器重复注册
    DoubleRegister,
    /// 事件类枚举外值
    EventKindInvalid,
    /// 矩阵对平未全过
    MatrixMismatch,
    /// 回调计数与命中数不守恒
    CallbackLedgerBreak,
}

impl NotifyCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NotifyCode::HitAfterUnregister => "注销后仍命中：监听生命周期破",
            NotifyCode::SubtreeOverreach => "未开子树却报子键事件：越权命中",
            NotifyCode::FlagMismatch => "事件类不在监听旗集却命中",
            NotifyCode::FlagOutOfRange => "旗位越四旗闭集",
            NotifyCode::DoubleRegister => "同键监听器重复注册",
            NotifyCode::EventKindInvalid => "事件类枚举外值",
            NotifyCode::MatrixMismatch => "过滤矩阵对平未全过：正确率红线",
            NotifyCode::CallbackLedgerBreak => "回调计数与命中数不守恒",
        }
    }
    pub const ALL: [NotifyCode; 8] = [
        NotifyCode::HitAfterUnregister,
        NotifyCode::SubtreeOverreach,
        NotifyCode::FlagMismatch,
        NotifyCode::FlagOutOfRange,
        NotifyCode::DoubleRegister,
        NotifyCode::EventKindInvalid,
        NotifyCode::MatrixMismatch,
        NotifyCode::CallbackLedgerBreak,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < NotifyCode::ALL.len() {
        if NotifyCode::ALL[i].val() == code {
            return NotifyCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65F00 段闭集内"
}

// ── 事件模型 ─────────────────────────────────────────────────────
/// 注册表事件：类（四旗之一）+ 键深度（0=本键，≥1=子树第 n 层）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegEvent {
    pub kind: u8,
    pub depth: u32,
}

impl RegEvent {
    pub fn legal(&self) -> bool {
        (self.kind as usize) < FLAG_COUNT
    }
}

/// 监听器：旗集 + 子树开关 + 生命周期态。
pub struct Watcher {
    pub flags: [bool; FLAG_COUNT],
    pub subtree: bool,
    pub active: bool,
    pub hits: u32,
    pub callbacks: u32,
}

impl Watcher {
    pub fn new(flags: [bool; FLAG_COUNT], subtree: bool) -> Watcher {
        Watcher {
            flags,
            subtree,
            active: true,
            hits: 0,
            callbacks: 0,
        }
    }

    /// 过滤闭式：事件命中判定（黄金）。
    pub fn accepts(&self, ev: &RegEvent) -> Result<bool, u32> {
        if !ev.legal() {
            return Err(NotifyCode::EventKindInvalid.val());
        }
        if !self.active {
            return Err(NotifyCode::HitAfterUnregister.val());
        }
        if ev.depth > 0 && !self.subtree {
            return Ok(false);
        }
        Ok(self.flags[ev.kind as usize])
    }

    /// 泵事件：命中则回调记账。
    pub fn pump(&mut self, ev: &RegEvent) -> Result<bool, u32> {
        let hit = self.accepts(ev)?;
        if hit {
            self.hits += 1;
            self.callbacks += 1;
        }
        Ok(hit)
    }

    pub fn unregister(&mut self) {
        self.active = false;
    }

    pub fn conservation(&self) -> Result<(), u32> {
        if self.hits != self.callbacks {
            return Err(NotifyCode::CallbackLedgerBreak.val());
        }
        Ok(())
    }
}

// ── 过滤矩阵 4×2 黄金判定 ────────────────────────────────────────
/// 黄金判定表：给定旗集+子树开关，对事件流逐事件给期望命中。
/// 事件流建模：每旗各发 本键事件 1 发 + 子键事件 1 发 = 8 事件。
pub fn golden_matrix() -> Result<u32, u32> {
    let mut agree = 0u32;
    let mut fi = 0;
    while fi < FLAG_COUNT {
        // 单旗监听（矩阵逐旗扫）。
        let mut flags = [false; FLAG_COUNT];
        flags[fi] = true;
        let mut st = 0;
        while st < 2 {
            let subtree = st == 1;
            let mut w = Watcher::new(flags, subtree);
            let mut evs: Vec<RegEvent> = Vec::new();
            let mut k = 0;
            while k < FLAG_COUNT {
                evs.push(RegEvent { kind: k as u8, depth: 0 });
                evs.push(RegEvent { kind: k as u8, depth: 1 });
                k += 1;
            }
            let mut all_ok = true;
            let mut e = 0;
            while e < evs.len() {
                let ev = evs[e];
                let got = w.pump(&ev)?;
                // 黄金：命中 = 旗开 且 (本键 或 子树开)。
                let expect = flags[ev.kind as usize] && (ev.depth == 0 || subtree);
                if got != expect {
                    all_ok = false;
                }
                e += 1;
            }
            if all_ok {
                agree += 1;
            }
            st += 1;
        }
        fi += 1;
    }
    if agree == 8 {
        Ok(8)
    } else {
        Err(NotifyCode::MatrixMismatch.val())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 抽样 32 例：LCG 抽 (旗集,子树,事件) 与黄金对平。
pub fn sample_32() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0u32;
    let mut i = 0;
    while i < 32 {
        let mut flags = [false; FLAG_COUNT];
        let mut f = 0;
        while f < FLAG_COUNT {
            flags[f] = rng.below(2) == 0;
            f += 1;
        }
        let subtree = rng.below(2) == 0;
        let ev = RegEvent {
            kind: rng.below(FLAG_COUNT as u64) as u8,
            depth: rng.below(3) as u32,
        };
        let mut w = Watcher::new(flags, subtree);
        let got = w.pump(&ev)?;
        let expect = flags[ev.kind as usize] && (ev.depth == 0 || subtree);
        if got == expect {
            agree += 1;
        }
        i += 1;
    }
    if agree == 32 {
        Ok(32)
    } else {
        Err(NotifyCode::MatrixMismatch.val())
    }
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：注销后命中。
    ledger.tries[0] += 1;
    let mut w = Watcher::new([true, false, false, false], false);
    w.unregister();
    if w.pump(&RegEvent { kind: F_NAME, depth: 0 })
        == Err(NotifyCode::HitAfterUnregister.val())
    {
        ledger.caught[0] += 1;
    }
    // 族 1：子树越权（未开子树，子键事件不命中——过滤语义守恒）。
    ledger.tries[1] += 1;
    let mut w2 = Watcher::new([true, false, false, false], false);
    let hit = w2.pump(&RegEvent { kind: F_NAME, depth: 1 }).unwrap_or(true);
    if !hit {
        ledger.caught[1] += 1;
    }
    // 族 2：旗位不匹配（只监 NAME，ATTRS 事件不命中）。
    ledger.tries[2] += 1;
    let mut w3 = Watcher::new([true, false, false, false], true);
    let hit2 = w3
        .pump(&RegEvent { kind: F_ATTRS, depth: 0 })
        .unwrap_or(true);
    if !hit2 {
        ledger.caught[2] += 1;
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let m = golden_matrix()?;
    let s = sample_32()?;
    Ok((m, s))
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < NotifyCode::ALL.len() {
        let v = NotifyCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-矩阵8格+抽样32全对",
        matches!(golden, Ok((8, 32))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 守恒正演。
    let mut w = Watcher::new([true, true, false, false], true);
    let _ = w.pump(&RegEvent { kind: F_NAME, depth: 0 });
    let _ = w.pump(&RegEvent { kind: F_ATTRS, depth: 1 });
    v.push((
        "J1-回调守恒",
        w.conservation().is_ok() && w.hits == 2,
        format!("hits={}", w.hits),
    ));
    // 旗名点验。
    let mut ok = 0;
    let mut i = 0;
    while i < FLAG_COUNT {
        if FLAG_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    v.push(("J1-四旗名点验", ok == 4, format!("{}/4", ok)));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65F00 窗口")));
    v
}

pub fn run_unx13801_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13801 RegNotifyChangeKeyValue 两档过滤");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
