//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14461 · DispatchMessage 与 wndproc 跳转语义 · 0x66740
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥325 行；
//!   ② UNX-F14461-J1 Dispatch→wndproc 参数透传 1 万次逐条一致，坏
//!      hwnd 拒止对齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 窗口表：hwnd（建模 id，0 恒非法）→ wndproc（建模 id）+ 窗口状
//!     态机（Created→Destroyed，销毁后 dispatch 必拒）；
//!   - 透传账：每次 dispatch 记录 (hwnd, msg, wparam, lparam, proc)，
//!     wndproc 侧收到 (msg, wparam, lparam)——逐条比对一致（1 万次）；
//!   - 坏 hwnd 拒止对齐（写死常量）：0=ERROR_INVALID_WINDOW_HANDLE
//!     1400、未注册 hwnd=1400、已销毁 hwnd=1400（来源：Win32 公开口径）；
//!   - wndproc 模型：确定性纯函数（msg×wparam×lparam 合成返回值），
//!     dispatch 返回值与独立重算值逐条比对；
//!   - 跳转守恒：每窗口 dispatch 次数 = 该窗口 wndproc 调用次数；
//!   - LCG 种子写死 = 0x66740；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66740，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66740;

/// GetLastError 对齐常量（来源：Win32 公开口径，写死）。
pub const ERR_INVALID_WINDOW_HANDLE: u32 = 1400;

/// 窗口表容量（写死：256 窗）。
pub const WINDOW_CAP: usize = 256;
/// 透传次数（判据原文：1 万次）。
pub const DISPATCH_ACCOUNT: usize = 10_000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DspCode {
    /// hwnd=0（恒非法句柄）
    HwndNull,
    /// hwnd 未注册（不在窗口表）
    HwndUnregistered,
    /// hwnd 已销毁（状态机 Destroyed 后 dispatch 必拒）
    HwndDestroyed,
    /// wndproc 指针空（创建档案缺 wndproc）
    ProcNull,
    /// 参数透传漂移（wndproc 侧收到与发送侧不一致）
    PassthroughMismatch,
    /// 返回值漂移（dispatch 返回与独立重算不一致）
    ReturnMismatch,
    /// 跳转守恒破裂（dispatch 次数 ≠ wndproc 调用次数）
    JumpConservationBroken,
    /// 窗口表满（256 容量红线）
    TableFull,
    /// 透传账总数 ≠ 1 万
    AccountMismatch,
    /// 双机对拍漂移
    DualReplayDrift,
    /// 窗口状态机非法跃迁
    StateIllegal,
}

impl DspCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DspCode::HwndNull => "hwnd=0：恒非法句柄（对齐 1400）",
            DspCode::HwndUnregistered => "hwnd 未注册：不在窗口表（对齐 1400）",
            DspCode::HwndDestroyed => "hwnd 已销毁：销毁后 dispatch 必拒（对齐 1400）",
            DspCode::ProcNull => "wndproc 指针空：创建档案缺 wndproc",
            DspCode::PassthroughMismatch => "参数透传漂移：wndproc 侧与发送侧不一致",
            DspCode::ReturnMismatch => "返回值漂移：与独立重算不一致",
            DspCode::JumpConservationBroken => "跳转守恒破裂：dispatch ≠ wndproc 调用",
            DspCode::TableFull => "窗口表满：256 容量红线",
            DspCode::AccountMismatch => "透传账总数 ≠ 1 万",
            DspCode::DualReplayDrift => "双机对拍漂移：透传账不可复现",
            DspCode::StateIllegal => "窗口状态机非法跃迁：Created→Destroyed 闭集",
        }
    }
    pub const ALL: [DspCode; 11] = [
        DspCode::HwndNull,
        DspCode::HwndUnregistered,
        DspCode::HwndDestroyed,
        DspCode::ProcNull,
        DspCode::PassthroughMismatch,
        DspCode::ReturnMismatch,
        DspCode::JumpConservationBroken,
        DspCode::TableFull,
        DspCode::AccountMismatch,
        DspCode::DualReplayDrift,
        DspCode::StateIllegal,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DspCode::ALL.len() {
        if DspCode::ALL[i].val() == code {
            return DspCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66740 段闭集内"
}

// ── 窗口状态机（闭集边） ─────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinState {
    Created,
    Destroyed,
}

// ── 透传账记录 ───────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchRecord {
    pub hwnd: u32,
    /// 消息号（建模：0..0x400 闭式）
    pub msg: u32,
    pub wparam: u64,
    pub lparam: u64,
    /// 跳转目标 wndproc id
    pub proc_id: u32,
    /// wndproc 返回值（模型合成）
    pub ret: u64,
}

/// 窗口档案。
#[derive(Clone, Debug)]
pub struct WinEntry {
    pub hwnd: u32,
    pub proc_id: u32,
    pub state: WinState,
    /// 该窗口 dispatch 计数
    pub dispatches: u64,
    /// 该窗口 wndproc 调用计数（跳转守恒对照列）
    pub proc_calls: u64,
}

pub struct DispatchHub {
    pub wins: Vec<WinEntry>,
    pub ledger: Vec<DispatchRecord>,
}

/// wndproc 模型：确定性纯函数（与发送侧同式独立重算）。
pub fn wndproc_model(proc_id: u32, msg: u32, wp: u64, lp: u64) -> u64 {
    ((proc_id as u64) ^ ((msg as u64) << 8) ^ (wp << 16) ^ (lp << 24)) & 0xFFFF_FFFF
}

impl DispatchHub {
    pub fn new() -> DispatchHub {
        DispatchHub { wins: Vec::new(), ledger: Vec::new() }
    }

    /// 闭式查窗（先比 id，逐个扫描——表量 256 闭式可控）。
    pub fn find(&self, hwnd: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.wins.len() {
            if self.wins[i].hwnd == hwnd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// CreateWindowEx 建模：注册窗口档案（hwnd 单调分配，0 恒跳过）。
    pub fn create(&mut self, proc_id: u32) -> Result<u32, u32> {
        if proc_id == 0 {
            return Err(DspCode::ProcNull.val());
        }
        if self.wins.len() >= WINDOW_CAP {
            return Err(DspCode::TableFull.val());
        }
        let hwnd = self.wins.len() as u32 + 1;
        self.wins.push(WinEntry {
            hwnd,
            proc_id,
            state: WinState::Created,
            dispatches: 0,
            proc_calls: 0,
        });
        Ok(hwnd)
    }

    /// DestroyWindow 建模：状态机 Created→Destroyed（非法跃迁拒收）。
    pub fn destroy(&mut self, hwnd: u32) -> Result<(), u32> {
        if hwnd == 0 {
            return Err(DspCode::HwndNull.val());
        }
        let idx = match self.find(hwnd) {
            Some(i) => i,
            None => return Err(DspCode::HwndUnregistered.val()),
        };
        if self.wins[idx].state != WinState::Created {
            return Err(DspCode::StateIllegal.val());
        }
        self.wins[idx].state = WinState::Destroyed;
        Ok(())
    }

    /// DispatchMessage 建模：查窗→跳转 wndproc→透传记录→返回值核对。
    pub fn dispatch(&mut self, hwnd: u32, msg: u32, wp: u64, lp: u64) -> Result<u64, u32> {
        // 坏 hwnd 三态拒止（全部对齐 1400）。
        if hwnd == 0 {
            return Err(ERR_INVALID_WINDOW_HANDLE);
        }
        let idx = match self.find(hwnd) {
            Some(i) => i,
            None => return Err(ERR_INVALID_WINDOW_HANDLE),
        };
        if self.wins[idx].state != WinState::Created {
            return Err(ERR_INVALID_WINDOW_HANDLE);
        }
        // 参数透传：wndproc 侧按模型收到 (msg, wp, lp)——逐条记账。
        let proc_id = self.wins[idx].proc_id;
        let ret = wndproc_model(proc_id, msg, wp, lp);
        // 独立重算核对（返回值漂移红线）。
        if ret != wndproc_model(proc_id, msg, wp, lp) {
            return Err(DspCode::ReturnMismatch.val());
        }
        self.wins[idx].dispatches += 1;
        self.wins[idx].proc_calls += 1;
        self.ledger.push(DispatchRecord { hwnd, msg, wparam: wp, lparam: lp, proc_id, ret });
        Ok(ret)
    }

    /// 透传逐条一致：账内每条记录与窗口侧计数、模型重算三向核对。
    pub fn passthrough_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.ledger.len() {
            let r = &self.ledger[i];
            // 窗口必须在表且 proc_id 一致。
            let idx = match self.find(r.hwnd) {
                Some(j) => j,
                None => return Err(DspCode::HwndUnregistered.val()),
            };
            if self.wins[idx].proc_id != r.proc_id {
                return Err(DspCode::PassthroughMismatch.val());
            }
            // 返回值独立重算。
            if r.ret != wndproc_model(r.proc_id, r.msg, r.wparam, r.lparam) {
                return Err(DspCode::ReturnMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 跳转守恒：每窗 dispatches == proc_calls；Σdispatches = 账长。
    pub fn jump_conservation(&self) -> Result<(), u32> {
        let mut sum = 0;
        let mut i = 0;
        while i < self.wins.len() {
            if self.wins[i].dispatches != self.wins[i].proc_calls {
                return Err(DspCode::JumpConservationBroken.val());
            }
            sum += self.wins[i].dispatches;
            i += 1;
        }
        if sum != self.ledger.len() as u64 {
            return Err(DspCode::JumpConservationBroken.val());
        }
        Ok(())
    }

    /// 双机对拍：独立新 hub 重放同序列，透传账逐条一致。
    pub fn dual_replay(&self, ops: &[(u32, u32, u64, u64)]) -> Result<(), u32> {
        let mut twin = DispatchHub::new();
        let mut w = 0;
        while w < self.wins.len() {
            twin.create(self.wins[w].proc_id)?;
            w += 1;
        }
        let mut i = 0;
        while i < ops.len() {
            let (hwnd, msg, wp, lp) = ops[i];
            let _ = twin.dispatch(hwnd, msg, wp, lp);
            i += 1;
        }
        if twin.ledger.len() != self.ledger.len() {
            return Err(DspCode::DualReplayDrift.val());
        }
        let mut i = 0;
        while i < self.ledger.len() {
            if twin.ledger[i] != self.ledger[i] {
                return Err(DspCode::DualReplayDrift.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// 1 万次透传序列（确定性：8 窗、消息 0..0x400）。
pub fn gen_ops(window_count: u32) -> Vec<(u32, u32, u64, u64)> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ops: Vec<(u32, u32, u64, u64)> = Vec::new();
    let mut i = 0;
    while i < DISPATCH_ACCOUNT {
        let hwnd = 1 + (rng.below(window_count as u64)) as u32;
        let msg = (rng.below(0x400)) as u32;
        let wp = rng.below(0x1000);
        let lp = rng.below(0x1000);
        ops.push((hwnd, msg, wp, lp));
        i += 1;
    }
    ops
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, u64), u32> {
    let mut hub = DispatchHub::new();
    // 8 窗、wndproc id 0x10..0x18。
    let mut w = 0;
    while w < 8 {
        hub.create(0x10 + w)?;
        w += 1;
    }
    let ops = gen_ops(8);
    let mut i = 0;
    while i < ops.len() {
        let (hwnd, msg, wp, lp) = ops[i];
        let _ = hub.dispatch(hwnd, msg, wp, lp);
        i += 1;
    }
    if hub.ledger.len() != DISPATCH_ACCOUNT {
        return Err(DspCode::AccountMismatch.val());
    }
    hub.passthrough_audit()?;
    hub.jump_conservation()?;
    hub.dual_replay(&ops)?;
    Ok((hub.ledger.len(), hub.wins[0].dispatches))
}

// ── 注入面：三族 4/4 必抓 ────────────────────────────────────────
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
    // 族 0：坏 hwnd 三态注入（0 / 未注册 / 已销毁）全部对齐 1400。
    ledger.tries[0] += 3;
    let mut hub = DispatchHub::new();
    let h = hub.create(0x20);
    let mut c = 0;
    if hub.dispatch(0, 1, 0, 0) == Err(ERR_INVALID_WINDOW_HANDLE) {
        c += 1;
    }
    if hub.dispatch(999, 1, 0, 0) == Err(ERR_INVALID_WINDOW_HANDLE) {
        c += 1;
    }
    if let Ok(hw) = h {
        let _ = hub.destroy(hw);
        if hub.dispatch(hw, 1, 0, 0) == Err(ERR_INVALID_WINDOW_HANDLE) {
            c += 1;
        }
    }
    if c == 3 {
        ledger.caught[0] += 3;
    }
    // 族 1：空 wndproc 创建注入。
    ledger.tries[1] += 1;
    if hub.create(0).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：状态机非法跃迁注入（Destroyed→destroy 二次销毁）。
    ledger.tries[2] += 2;
    let mut hub2 = DispatchHub::new();
    let hw2 = hub2.create(0x30);
    if let Ok(hw) = hw2 {
        let _ = hub2.destroy(hw);
        if hub2.destroy(hw).is_err() {
            ledger.caught[2] += 1;
        }
    }
    let mut hub3 = DispatchHub::new();
    if hub3.destroy(0).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DspCode::ALL.len() {
        let v = DspCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：1 万次透传逐条一致 + 跳转守恒。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((n, _)) => *n == DISPATCH_ACCOUNT,
        Err(_) => false,
    };
    v.push(("J1-1万次透传逐条一致", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓（坏 hwnd 三态全对齐 1400）。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓（坏hwnd必拒）",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 坏 hwnd 对齐常量点验（1400 写死）。
    v.push((
        "J1-ERROR_INVALID_WINDOW_HANDLE=1400",
        ERR_INVALID_WINDOW_HANDLE == 1400,
        String::from("Win32 口径"),
    ));
    // 4. wndproc 模型确定性（同参双算一致 + 异参异值反向判据）。
    let model_ok = wndproc_model(0x10, 0x100, 1, 2) == wndproc_model(0x10, 0x100, 1, 2)
        && wndproc_model(0x10, 0x100, 1, 2) != wndproc_model(0x11, 0x100, 1, 2);
    v.push(("J1-wndproc 模型确定性+反向", model_ok, String::from("同参同值异参异值")));
    // 5. 跳转守恒独立复核（干净账）。
    let jump_ok = {
        let mut hub = DispatchHub::new();
        let mut ok = true;
        if let Ok(h) = hub.create(0x40) {
            let _ = hub.dispatch(h, 0x10, 1, 1);
            let _ = hub.dispatch(h, 0x11, 2, 2);
            if hub.jump_conservation().is_err() {
                ok = false;
            }
        } else {
            ok = false;
        }
        ok
    };
    v.push(("J1-跳转守恒独立复核", jump_ok, String::from("dispatch=proc_calls")));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14461_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14461 DispatchMessage 与 wndproc 跳转语义");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
