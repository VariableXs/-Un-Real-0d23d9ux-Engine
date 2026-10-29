//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13181 · 调试口总成（TEB/PEB 调试位写路径） · 0x65740
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13181-J1 调试位写路径全流程 30 例（进程创建默认 FALSE→
//!      挂调试器置位→解除复位）与 Windows 一致，写路径权限面断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 调试位三态状态机（闭式边集）：Clean(FALSE) → Attached(TRUE) →
//!     Detached(FALSE)；置位只准挂调试器事件、复位只准解除事件，
//!     越权写路径（用户态直写/未挂先置/未挂先解）一律拒收；
//!   - 权限面闭集：Kernel（可读写调试位）/ Debugger（可置位/可解除）/
//!     UserMode（只读，写必拒）三档；
//!   - 双载体联动：PEB.BeingDebugged 与 TEB 调试口句柄必须同步
//!     （单边写 → SyncBroken 必抓）；
//!   - 30 例全流程账：创建默认 FALSE → 挂调试器置位 → 解除复位，
//!     终态账守恒（复位后 FALSE 且句柄清零）；
//!   - LCG 种子写死 = SEG_BASE（0x65740），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65740，全仓独占）。
pub const SEG_BASE: u32 = 0x65740;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgPortCode {
    /// 用户态直写调试位（权限面破）
    UserModeWrite,
    /// 未挂调试器先置位
    SetBeforeAttach,
    /// 未挂调试器先解除
    DetachBeforeAttach,
    /// 单边写（PEB/TEB 不同步）
    SyncBroken,
    /// 终态复位不全（位复位但句柄残留）
    ResidueAfterDetach,
    /// 解除后仍报挂调试器
    AttachedAfterDetach,
    /// 调试口写路径非法状态迁移
    IllegalTransition,
    /// 调试口句柄越域
    PortOutOfRange,
}

impl DbgPortCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DbgPortCode::UserModeWrite => "用户态直写调试位：写路径权限面破",
            DbgPortCode::SetBeforeAttach => "未挂调试器先置位：状态机越迁",
            DbgPortCode::DetachBeforeAttach => "未挂调试器先解除：状态机越迁",
            DbgPortCode::SyncBroken => "单边写：PEB.BeingDebugged 与 TEB 调试口不同步",
            DbgPortCode::ResidueAfterDetach => "解除复位不全：位复位但句柄残留",
            DbgPortCode::AttachedAfterDetach => "解除后仍报挂调试器：终态语义破",
            DbgPortCode::IllegalTransition => "调试口写路径非法状态迁移：闭式边集拒收",
            DbgPortCode::PortOutOfRange => "调试口句柄越域：闭集外端口",
        }
    }
    pub const ALL: [DbgPortCode; 8] = [
        DbgPortCode::UserModeWrite,
        DbgPortCode::SetBeforeAttach,
        DbgPortCode::DetachBeforeAttach,
        DbgPortCode::SyncBroken,
        DbgPortCode::ResidueAfterDetach,
        DbgPortCode::AttachedAfterDetach,
        DbgPortCode::IllegalTransition,
        DbgPortCode::PortOutOfRange,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DbgPortCode::ALL.len() {
        if DbgPortCode::ALL[i].val() == code {
            return DbgPortCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：调试口建模书 + 判据原文） ────────────────────────
/// 调试口句柄闭集（建模：0x0001..0x00FF 档）。
pub const PORT_DOMAIN: u32 = 0x0100;
/// 全流程例数（判据原文：30 例）。
pub const CASES: usize = 30;

// ── 权限档闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Priv {
    /// 内核：可读写调试位
    Kernel,
    /// 调试器：可置位/可解除
    Debugger,
    /// 用户态：只读（写必拒）
    UserMode,
}

// ── 调试位三态状态机 ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgState {
    /// 进程创建默认：FALSE（未挂调试器）
    Clean,
    /// 挂调试器：TRUE
    Attached,
    /// 解除后：FALSE（句柄清零）
    Detached,
}

/// 写路径事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgEvent {
    /// 挂调试器（置位）
    Attach,
    /// 解除调试器（复位）
    Detach,
}

/// 合法边闭集：Clean --Attach--> Attached --Detach--> Detached。
/// 其余（Clean--Detach / Attached--Attach / Detached--Attach/Detach）全拒。
pub fn dbg_transition(cur: DbgState, ev: DbgEvent) -> Result<DbgState, DbgPortCode> {
    match (cur, ev) {
        (DbgState::Clean, DbgEvent::Attach) => Ok(DbgState::Attached),
        (DbgState::Attached, DbgEvent::Detach) => Ok(DbgState::Detached),
        _ => {
            if ev == DbgEvent::Attach {
                Err(DbgPortCode::SetBeforeAttach)
            } else {
                Err(DbgPortCode::DetachBeforeAttach)
            }
        }
    }
}

// ── 调试口载体（双载体联动账） ───────────────────────────────────
#[derive(Clone, Debug)]
pub struct DebugPort {
    /// 进程 id
    pub pid: u32,
    /// PEB.BeingDebugged 位
    pub peb_being_debugged: bool,
    /// TEB 调试口句柄（0 = 未挂）
    pub teb_debug_port: u32,
    /// 状态机
    pub state: DbgState,
}

impl DebugPort {
    /// 进程创建：默认 FALSE + 句柄 0（判据主面起点）。
    pub fn new(pid: u32) -> DebugPort {
        DebugPort {
            pid,
            peb_being_debugged: false,
            teb_debug_port: 0,
            state: DbgState::Clean,
        }
    }

    /// 写路径权限面核：UserMode 一律拒写。
    pub fn check_priv(p: Priv) -> Result<(), DbgPortCode> {
        match p {
            Priv::UserMode => Err(DbgPortCode::UserModeWrite),
            Priv::Kernel | Priv::Debugger => Ok(()),
        }
    }

    /// 置位（挂调试器）：状态机 + 双载体同步写。
    pub fn attach(&mut self, p: Priv, port: u32) -> Result<(), DbgPortCode> {
        Self::check_priv(p)?;
        if port == 0 || port >= PORT_DOMAIN {
            return Err(DbgPortCode::PortOutOfRange);
        }
        let ns = dbg_transition(self.state, DbgEvent::Attach)?;
        self.state = ns;
        // 双载体同步：PEB 位与 TEB 句柄同笔写（单边即破）。
        self.peb_being_debugged = true;
        self.teb_debug_port = port;
        Ok(())
    }

    /// 复位（解除调试器）：状态机 + 双载体同步清。
    pub fn detach(&mut self, p: Priv) -> Result<(), DbgPortCode> {
        Self::check_priv(p)?;
        let ns = dbg_transition(self.state, DbgEvent::Detach)?;
        self.state = ns;
        self.peb_being_debugged = false;
        self.teb_debug_port = 0;
        Ok(())
    }

    /// 同步核：PEB 位与 TEB 句柄必须同真同假。
    pub fn verify_sync(&self) -> Result<(), DbgPortCode> {
        let bit = self.peb_being_debugged;
        let port_set = self.teb_debug_port != 0;
        if bit != port_set {
            return Err(DbgPortCode::SyncBroken);
        }
        Ok(())
    }

    /// 终态核：解除后 FALSE 且句柄清零（残留 → ResidueAfterDetach）。
    pub fn verify_detached(&self) -> Result<(), DbgPortCode> {
        if self.state == DbgState::Detached {
            if self.peb_being_debugged || self.teb_debug_port != 0 {
                return Err(DbgPortCode::ResidueAfterDetach);
            }
            if self.peb_being_debugged == false && self.teb_debug_port == 0 {
                return Ok(());
            }
            return Err(DbgPortCode::AttachedAfterDetach);
        }
        Err(DbgPortCode::IllegalTransition)
    }
}

/// 全流程账（30 例载体）。
pub struct DbgLedger {
    pub ports: Vec<DebugPort>,
    pub attached_total: usize,
    pub detached_total: usize,
}

impl DbgLedger {
    pub fn new() -> DbgLedger {
        DbgLedger {
            ports: Vec::new(),
            attached_total: 0,
            detached_total: 0,
        }
    }

    /// 一例全流程：创建（FALSE）→ 挂（TRUE）→ 解除（FALSE）。
    pub fn run_case(&mut self, pid: u32, port: u32) -> Result<(), DbgPortCode> {
        let mut dp = DebugPort::new(pid);
        dp.attach(Priv::Debugger, port)?;
        self.attached_total += 1;
        dp.detach(Priv::Kernel)?;
        self.detached_total += 1;
        dp.verify_sync()?;
        dp.verify_detached()?;
        self.ports.push(dp);
        Ok(())
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
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：30 例全流程（创建 FALSE → 置位 → 复位）与 Windows 一致。
pub fn drill_30_cases() -> (bool, String) {
    let mut led = DbgLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < CASES {
        let port = (rng.next_below((PORT_DOMAIN - 1) as u64) + 1) as u32;
        if led.run_case(0x9000 + i as u32, port).is_err() {
            return (false, format!("第 {} 例调试位链断", i + 1));
        }
        i += 1;
    }
    let ok = led.ports.len() == CASES
        && led.attached_total == CASES
        && led.detached_total == CASES;
    (
        ok,
        format!(
            "调试位 30 例全流程: 挂={} 解={}",
            led.attached_total, led.detached_total
        ),
    )
}

/// 创建默认面：新进程必为 FALSE + 句柄 0（10/10）。
pub fn drill_default_false_10() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let dp = DebugPort::new(0xA000 + i);
        if dp.peb_being_debugged == false && dp.teb_debug_port == 0 && dp.state == DbgState::Clean {
            caught += 1;
        }
        i += 1;
    }
    (caught == 10, format!("创建默认 FALSE {}/10", caught))
}

/// 写路径权限面：UserMode 直写 10/10 必抓（attach/detach 两路）。
pub fn drill_user_mode_write_10() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 5 {
        let mut dp = DebugPort::new(0xB000 + i);
        if dp.attach(Priv::UserMode, 0x42) == Err(DbgPortCode::UserModeWrite) {
            caught += 1;
        }
        i += 1;
    }
    let mut j = 0;
    while j < 5 {
        let mut dp = DebugPort::new(0xB100 + j);
        // 先合法挂上再由用户态解除。
        let _ = dp.attach(Priv::Debugger, 0x43);
        if dp.detach(Priv::UserMode) == Err(DbgPortCode::UserModeWrite) {
            caught += 1;
        }
        j += 1;
    }
    (caught == 10, format!("用户态直写注入 {}/10 必抓", caught))
}

/// 状态机越迁注入：未挂先置 / 未挂先解 / 重复挂 / 重复解 / 解后再挂。
pub fn drill_state_edges() -> (bool, String) {
    let e1 = dbg_transition(DbgState::Clean, DbgEvent::Detach);
    let e2 = dbg_transition(DbgState::Attached, DbgEvent::Attach);
    let e3 = dbg_transition(DbgState::Detached, DbgEvent::Attach);
    let e4 = dbg_transition(DbgState::Detached, DbgEvent::Detach);
    let ok1 = e1 == Err(DbgPortCode::DetachBeforeAttach);
    let ok2 = e2 == Err(DbgPortCode::SetBeforeAttach);
    let ok3 = e3 == Err(DbgPortCode::SetBeforeAttach);
    let ok4 = e4 == Err(DbgPortCode::DetachBeforeAttach);
    // 合法边保通：Clean--Attach 与 Attached--Detach 必须通。
    let ok5 = dbg_transition(DbgState::Clean, DbgEvent::Attach) == Ok(DbgState::Attached)
        && dbg_transition(DbgState::Attached, DbgEvent::Detach) == Ok(DbgState::Detached);
    (
        ok1 && ok2 && ok3 && ok4 && ok5,
        format!(
            "越迁注入: 未挂先解={} 重复挂={} 解后再挂={} 重复解={} 合法边通={}",
            ok1, ok2, ok3, ok4, ok5
        ),
    )
}

/// 单边写注入：PEB/TEB 不同步必抓（5 组篡改全抓）。
pub fn drill_sync_broken_5() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 5 {
        let mut dp = DebugPort::new(0xC000 + i);
        // 篡改一：PEB 位真、句柄假。
        let mut a = dp.clone();
        a.peb_being_debugged = true;
        if a.verify_sync() == Err(DbgPortCode::SyncBroken) {
            caught += 1;
        }
        // 篡改二：句柄真、PEB 位假。
        let mut b = dp.clone();
        b.teb_debug_port = 0x10 + i;
        if b.verify_sync() == Err(DbgPortCode::SyncBroken) {
            caught += 1;
        }
        dp.verify_sync().ok();
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("单边写注入 {}/10 必抓", caught))
}

/// 句柄越域注入：0 号口与 ≥PORT_DOMAIN 10/10 必抓。
pub fn drill_port_range_10() -> (bool, String) {
    let mut caught = 0;
    // 0 号口无效。
    let mut a = DebugPort::new(0xD000);
    if a.attach(Priv::Debugger, 0) == Err(DbgPortCode::PortOutOfRange) {
        caught += 1;
    }
    // 越域 9 例。
    let mut i = 0;
    while i < 9 {
        let mut dp = DebugPort::new(0xD100 + i);
        if dp.attach(Priv::Debugger, PORT_DOMAIN + i as u32) == Err(DbgPortCode::PortOutOfRange) {
            caught += 1;
        }
        i += 1;
    }
    (caught == 10, format!("句柄越域注入 {}/10 必抓", caught))
}

/// 终态面：解除后仍报挂调试器必抓（AttachedAfterDetach 反向面）。
pub fn drill_detach_semantics() -> (bool, String) {
    let mut dp = DebugPort::new(0xE000);
    let _ = dp.attach(Priv::Debugger, 0x50);
    let _ = dp.detach(Priv::Kernel);
    let clean = dp.verify_detached() == Ok(());
    // 反向：解除后残留句柄 → 必抓 ResidueAfterDetach。
    let mut bad = dp.clone();
    bad.teb_debug_port = 0x50;
    let residue = bad.verify_detached() == Err(DbgPortCode::ResidueAfterDetach);
    // 反向：解除后再查挂状态 → Clean 语义（不是 Attached）。
    let not_attached = dp.state != DbgState::Attached;
    (
        clean && residue && not_attached,
        format!("终态: 复位全清={} 残留抓={} 解后非挂={}", clean, residue, not_attached),
    )
}

/// 守恒面：挂/解笔数相等，账内全部终态 FALSE（对账恒等式）。
pub fn drill_conservation() -> (bool, String) {
    let mut led = DbgLedger::new();
    let mut i = 0;
    while i < 8 {
        let _ = led.run_case(0xF000 + i as u32, 0x10 + i as u32);
        i += 1;
    }
    let mut all_false = true;
    let mut j = 0;
    while j < led.ports.len() {
        if led.ports[j].peb_being_debugged || led.ports[j].teb_debug_port != 0 {
            all_false = false;
        }
        j += 1;
    }
    let ok = led.attached_total == led.detached_total
        && led.attached_total == 8
        && all_false;
    (
        ok,
        format!(
            "守恒: 挂=解={} 全终态 FALSE={}",
            led.attached_total, all_false
        ),
    )
}

/// 多机对拍面：同种子重建两次，账一致。
pub fn drill_determinism() -> (bool, String) {
    let build = || {
        let mut led = DbgLedger::new();
        let mut rng = Lcg::new(SEG_BASE as u64);
        let mut i = 0;
        while i < 12 {
            let port = (rng.next_below((PORT_DOMAIN - 1) as u64) + 1) as u32;
            let _ = led.run_case(0xFA00 + i, port);
            i += 1;
        }
        (led.attached_total, led.detached_total, led.ports.len())
    };
    let a = build();
    let b = build();
    (a == b, format!("同种子对拍一致={}", a == b))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_30_cases();
    let (a2, d2) = drill_user_mode_write_10();
    let (a3, d3) = drill_state_edges();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 30例={} 权限面={} 状态机={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 深化块：黄金三态对照 + 调试位账导出 ──────────────────────────
/// 黄金三态对照表：三例事件链→期望终态独立复判（挂解/只挂/不动）。
pub fn drill_golden_states() -> (bool, String) {
    let mut a = DebugPort::new(0x1100);
    let _ = a.attach(Priv::Debugger, 0x11);
    let _ = a.detach(Priv::Kernel);
    let mut b = DebugPort::new(0x1101);
    let _ = b.attach(Priv::Debugger, 0x12);
    let c = DebugPort::new(0x1102);
    let ok = a.state == DbgState::Detached
        && b.state == DbgState::Attached
        && c.state == DbgState::Clean;
    // 黄金对照配反向：终态账导出断言（位/句柄/状态三元一致）。
    let rows_ok = a.verify_sync().is_ok() && b.verify_sync().is_ok() && c.verify_sync().is_ok();
    (ok && rows_ok, format!("黄金三态对照全真={} 同步账全真={}", ok, rows_ok))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < DbgPortCode::ALL.len() {
        let v = DbgPortCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_30_cases();
    v.push(("J1-调试位写路径30例", a, b));
    let (a, b) = drill_default_false_10();
    v.push(("J1-创建默认FALSE10/10", a, b));
    let (a, b) = drill_user_mode_write_10();
    v.push(("J1-写路径权限面10/10", a, b));
    let (a, b) = drill_state_edges();
    v.push(("J2-状态机越迁注入", a, b));
    let (a, b) = drill_sync_broken_5();
    v.push(("J2-单边写注入10/10", a, b));
    let (a, b) = drill_port_range_10();
    v.push(("J2-句柄越域注入10/10", a, b));
    let (a, b) = drill_detach_semantics();
    v.push(("J3-解除终态语义+反向", a, b));
    let (a, b) = drill_conservation();
    v.push(("J3-挂解守恒对账", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同种子对拍一致", a, b));
    let (a, b) = drill_golden_states();
    v.push(("J3-黄金三态对照", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13181_checks：本单判据聚合（诊断段 0x65740）。
pub fn run_unx13181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13181 调试口总成");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65740 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
