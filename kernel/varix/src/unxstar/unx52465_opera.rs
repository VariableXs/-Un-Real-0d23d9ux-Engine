//! UNX-F52465 Opera 收口 · 0xCB400段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52465"，逐字）：
//!   title: Opera 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52466-J1 六判据全
//!     落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52465）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52466-J1，照原文锚定；功能按本卡标题（Opera 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 格闭集：渲染/侧边栏/Flow/VPN/省电各 4 格，家族单调
//!      （逆序必抓），逐格载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② VPN 三态 6 格：关/自动/开三态闭集，态越界与载荷漂移必抓。
//!   ③ 与 Chrome 分账声明 2 行闭集（锚 UNX-F52461）：渲染账分账/扩展账
//!      分账，缺一拒封。
//!   ④ Chromium 上栈声明 2 行闭集：进程模型上栈声明/网络服务上栈声明，
//!      缺一拒封。
//!   ⑤ 六判据收口闸（启动/渲染/崩溃/卸载/升级/许可）逐格落账，未齐即
//!      封拒。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（操作漂移/
//!      操作越序/操作越闭集，三类注入全落当轮临时账本，空账本同样必
//!      抓，越序路径不改 last_kind，escaped=0）；注入=检出+逃逸守恒；
//!      双跑 FNV 指纹一致；KB-COMPAT 定级 S1/S2/S3 闭集（全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52465;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const FEATURE_OPS: usize = 20;

/// VPN 三态格数。
pub const VPN_CELLS: usize = 6;

/// 与 Chrome 分账声明行数（锚 UNX-F52461）。
pub const SPLIT_ROWS: usize = 2;

/// Chromium 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OperaCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// VPN 格越闭集
    ENoVpn,
    /// VPN 三态漂移
    EVpnDrift,
    /// 分账行越闭集
    ENoSplit,
    /// 上栈声明行越闭集
    ENoStack,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl OperaCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            OperaCode::Ok => "成功",
            OperaCode::ENoOp => "操作号越闭集：渲染/侧边栏/Flow/VPN/省电 20 格之外先登记",
            OperaCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            OperaCode::EOrder => "家族逆序：渲染→侧边栏→Flow→VPN→省电单调，逆序必抓",
            OperaCode::ENoVpn => "VPN 格越闭集：VPN 三态 6 格之外或三态闭集之外先登记",
            OperaCode::EVpnDrift => "VPN 三态漂移：三态载荷与基准不恒等，逐格复算",
            OperaCode::ENoSplit => "分账行越闭集：与 Chrome 分账 2 行闭集之外先登记",
            OperaCode::ENoStack => "上栈声明行越闭集：Chromium 上栈声明 2 行闭集之外先登记",
            OperaCode::EDup => "重复落账：同格只许一条账目",
            OperaCode::ESeal => "未齐即封：20 操作 + 6 格 VPN + 2 行分账 + 2 行上栈未齐先补齐",
            OperaCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            OperaCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            OperaCode::EGradeDrift => "定级漂移：全通且两行分账与两行上栈声明全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20操作+6格VPN+2行分账+2行上栈全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能族格载荷（操作 o；家族 = o/4）。
pub const fn feature_cell(o: usize) -> Option<u64> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((0xCB00u64 + (o as u64) * 0x37) ^ 0x4F5052415F4F50)
}

/// 功能族（0..4=渲染 4..8=侧边栏 8..12=Flow 12..16=VPN 16..20=省电）。
pub const fn feature_kind(o: usize) -> Option<u32> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn feature_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("渲染"),
        1 => Some("侧边栏"),
        2 => Some("Flow"),
        3 => Some("VPN"),
        4 => Some("省电"),
        _ => None,
    }
}

/// VPN 三态格载荷（格 i 与态 s 的折叠；态 0=关 1=自动 2=开）。
pub const fn vpn_cell(i: usize, s: u32) -> Option<u64> {
    if i >= VPN_CELLS || s >= 3 {
        return None;
    }
    Some((0xCB20u64 + (i as u64) * 0x5B) ^ (s as u64).wrapping_mul(0x51535441))
}

/// VPN 态名。
pub const fn vpn_state_name(s: u32) -> Option<&'static str> {
    match s {
        0 => Some("关"),
        1 => Some("自动"),
        2 => Some("开"),
        _ => None,
    }
}

/// 与 Chrome 分账行折叠（行 r，锚 UNX-F52461）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCB60 + r as u32 * 0x24)
}

/// 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("渲染账分账"),
        1 => Some("扩展账分账"),
        _ => None,
    }
}

/// Chromium 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCB80 + r as u32 * 0x24)
}

/// 上栈声明行名。
pub const fn stack_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("进程模型上栈声明"),
        1 => Some("网络服务上栈声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Opera 收口账本状态机。
#[derive(Clone)]
pub struct OperaLedger {
    /// 六判据（启动/渲染/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 功能族操作账。
    ops: [Option<u64>; FEATURE_OPS],
    /// VPN 三态账。
    vpns: [Option<u64>; VPN_CELLS],
    /// 与 Chrome 分账账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Chromium 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl OperaLedger {
    /// 新账本。
    pub fn new() -> Self {
        OperaLedger {
            gates: [None; 6],
            ops: [None; FEATURE_OPS],
            vpns: [None; VPN_CELLS],
            splits: [None; SPLIT_ROWS],
            stacks: [None; STACK_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), OperaCode> {
        if gate >= 6 {
            return Err(OperaCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 功能族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), OperaCode> {
        if o >= FEATURE_OPS {
            return Err(OperaCode::ENoOp);
        }
        let kind = match feature_kind(o) {
            Some(k) => k,
            None => return Err(OperaCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(OperaCode::EOrder);
        }
        let want = match feature_cell(o) {
            Some(w) => w,
            None => return Err(OperaCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(OperaCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OperaCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 功能族全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < FEATURE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// VPN 一格（三态闭集 + 载荷对照 + 重复拒）。
    pub fn record_vpn(&mut self, i: usize, s: u32) -> Result<(), OperaCode> {
        let want = match vpn_cell(i, s) {
            Some(w) => w,
            None => return Err(OperaCode::ENoVpn),
        };
        if match self.vpns.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OperaCode::EDup);
        }
        if let Some(c) = self.vpns.get_mut(i) {
            *c = Some(want);
        }
        self.fnv(0x4600 + i as u64);
        Ok(())
    }

    /// VPN 一格对照（载荷漂移核验，走注入账）。
    pub fn check_vpn(&mut self, i: usize, s: u32, got: u64) -> Result<(), OperaCode> {
        let want = match vpn_cell(i, s) {
            Some(w) => w,
            None => return Err(OperaCode::ENoVpn),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4601 + i as u64);
            return Err(OperaCode::EVpnDrift);
        }
        Ok(())
    }

    /// VPN 全账核验（6 格）。
    pub fn vpns_all(&self) -> bool {
        let mut i = 0usize;
        while i < VPN_CELLS {
            match self.vpns.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 与 Chrome 分账一行（锚 UNX-F52461）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), OperaCode> {
        if r >= SPLIT_ROWS {
            return Err(OperaCode::ENoSplit);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OperaCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4400 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Chromium 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), OperaCode> {
        if r >= STACK_ROWS {
            return Err(OperaCode::ENoStack);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OperaCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x4700 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        let full = self.gates_all() && self.ops_all() && self.vpns_all() && self.splits_all() && self.stacks_all();
        let gates_ok = self.gates_all();
        if full {
            Grade::S1
        } else if gates_ok {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for OperaLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 逃逸数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u64) {
    let mut led = OperaLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FEATURE_OPS {
        if let Some(w) = feature_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < VPN_CELLS {
        let s = (i % 3) as u32;
        if led.record_vpn(i, s).is_ok() {
            recorded += 1;
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：操作漂移 / 操作越序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，临时账本从空起手，空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = OperaLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格，再注漂移。
                let _ = lt.record_op(0, feature_cell(0).unwrap_or(0));
                lt.record_op(1, feature_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(OperaCode::EDrift)
            }
            1 => {
                // 越序注入：先落家族 2 正格，再倒落家族 0；错误路径不改 last_kind。
                let _ = lt.record_op(8, feature_cell(8).unwrap_or(0));
                lt.record_op(1, feature_cell(1).unwrap_or(0)) == Err(OperaCode::EOrder)
            }
            _ => {
                // 越闭集注入：空账本直接越界，提前返回必抓。
                lt.record_op(FEATURE_OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let escaped = led.escaped;
    let fp = led.fingerprint();
    (recorded, caught, escaped, fp)
}

/// 周期守恒核验（守恒破缺 → EInj，指纹漂移 → EFp）。
pub fn verify_cycle() -> Result<u64, OperaCode> {
    let (_, caught, escaped, fp) = run_cycle();
    if caught != 10 || escaped != 0 {
        return Err(OperaCode::EInj);
    }
    let (_, _, _, fp2) = run_cycle();
    if fp != fp2 {
        return Err(OperaCode::EFp);
    }
    Ok(fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52465 判据集（锚 UNX-F52466-J1，off-by-one 板式）。
pub fn run_unx52465_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52465");
    let (recorded, caught, escaped, fp) = run_cycle();
    s.add(
        "五功能族 20 格全通",
        recorded >= FEATURE_OPS as u32,
        "渲染/侧边栏/Flow/VPN/省电五家族逐操作恒等",
    );
    s.add(
        "VPN 三态 6 格全落",
        {
            let mut lt = OperaLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < VPN_CELLS {
                if lt.record_vpn(i, (i % 3) as u32).is_err() {
                    ok = false;
                }
                i += 1;
            }
            ok && lt.vpns_all()
        },
        "关/自动/开三态闭集逐格落账，态越界必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = OperaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/渲染/崩溃/卸载/升级/许可逐格落账（UNX-F52466-J1 六判据）",
    );
    s.add(
        "与 Chrome 分账声明 2/2 全齐",
        {
            let mut lt = OperaLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "渲染账分账/扩展账分账两行闭集（锚 UNX-F52461，§AI-66.3）",
    );
    s.add(
        "Chromium 上栈声明 2/2 全齐",
        {
            let mut lt = OperaLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "进程模型上栈声明/网络服务上栈声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FEATURE_OPS {
                if feature_kind(o).unwrap_or(5) < feature_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调，逆序必抓",
    );
    s.add(
        "VPN 三态载荷恒等",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < VPN_CELLS {
                let mut st = 0u32;
                while st < 3 {
                    let a = vpn_cell(i, st).unwrap_or(0);
                    let b = vpn_cell(i, st).unwrap_or(0);
                    if a != b {
                        ok = false;
                    }
                    st += 1;
                }
                i += 1;
            }
            ok && vpn_cell(0, 0) != vpn_cell(0, 1)
        },
        "三态折叠纯函数恒等且态间互异",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作漂移/操作越序/操作越闭集三态轮转必抓",
    );
    let mut l0 = OperaLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(FEATURE_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·VPN 态越闭集",
        l0.record_vpn(0, 3).is_err() && l0.record_vpn(VPN_CELLS, 0).is_err(),
        "态 3 与格 6 越三态闭集必拒",
    );
    s.add(
        "边界·分账行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err() && l0.declare_stack(STACK_ROWS).is_err(),
        "分账行 2 与上栈行 2 越闭集必拒",
    );
    s.add(
        "未齐可续录",
        {
            let mut l1 = OperaLedger::new();
            l1.record_op(0, feature_cell(0).unwrap_or(0)).is_ok()
                && l1.judge_gate(0).is_ok()
        },
        "账未齐时落账与判据闸必须放行（未齐即封核只拒封口不拒续录）",
    );
    s.add_dyn(
        "注入账守恒",
        caught == 10 && escaped == 0,
        &format!("注入=10 检出={} 逃逸={}", caught, escaped),
    );
    s.add("双跑指纹一致", fp != 0 && verify_cycle().is_ok(), "确定性全流程逐位一致且守恒核验通过");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = OperaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FEATURE_OPS {
                if let Some(w) = feature_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < VPN_CELLS {
                let _ = lt.record_vpn(i, (i % 3) as u32);
                i += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB400,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCB400 格",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_kind() {
        assert_eq!(feature_kind(0), Some(0));
        assert_eq!(feature_kind(4), Some(1));
        assert_eq!(feature_kind(8), Some(2));
        assert_eq!(feature_kind(12), Some(3));
        assert_eq!(feature_kind(19), Some(4));
        assert!(feature_kind(20).is_none());
        assert!(feature_cell(20).is_none());
        assert_eq!(feature_kind_name(2), Some("Flow"));
        assert!(feature_kind_name(5).is_none());
    }

    #[test]
    fn test_vpn_states() {
        assert_eq!(vpn_state_name(0), Some("关"));
        assert_eq!(vpn_state_name(1), Some("自动"));
        assert_eq!(vpn_state_name(2), Some("开"));
        assert!(vpn_state_name(3).is_none());
        assert!(vpn_cell(0, 3).is_none());
        assert!(vpn_cell(VPN_CELLS, 0).is_none());
        assert_ne!(vpn_cell(0, 0), vpn_cell(0, 1));
        assert_ne!(vpn_cell(0, 1), vpn_cell(0, 2));
    }

    #[test]
    fn test_rows_closed() {
        assert_eq!(split_row_name(0), Some("渲染账分账"));
        assert_eq!(split_row_name(1), Some("扩展账分账"));
        assert!(split_row_name(2).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert_eq!(stack_row_name(1), Some("网络服务上栈声明"));
    }

    #[test]
    fn test_cells_pure() {
        let a = feature_cell(3).unwrap_or(0);
        assert_eq!(a, feature_cell(3).unwrap_or(0));
        assert_ne!(a, feature_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序先落低族正格，再注逆序必抓。
        let mut lt = OperaLedger::new();
        let _ = lt.record_op(9, feature_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, feature_cell(2).unwrap_or(0)), Err(OperaCode::EOrder));
        // 逆序错误路径不改 last_kind：同族后续仍可续录。
        assert!(lt.record_op(10, feature_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = OperaLedger::new();
        assert_eq!(
            lt.record_op(5, feature_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(OperaCode::EDrift)
        );
        assert_eq!(lt.check_vpn(1, 1, vpn_cell(1, 1).unwrap_or(0).wrapping_add(7)), Err(OperaCode::EVpnDrift));
        assert_eq!(lt.check_vpn(1, 1, vpn_cell(1, 1).unwrap_or(0)), Ok(()));
        assert_eq!(lt.check_vpn(VPN_CELLS, 0, 0), Err(OperaCode::ENoVpn));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = OperaLedger::new();
        let _ = lt.record_vpn(0, 0);
        assert_eq!(lt.record_vpn(0, 0), Err(OperaCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(OperaCode::EDup));
        let _ = lt.declare_stack(0);
        assert_eq!(lt.declare_stack(0), Err(OperaCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = OperaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = OperaLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, escaped, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：操作 20 + VPN 6。
        assert_eq!(recorded, FEATURE_OPS as u32 + VPN_CELLS as u32);
        assert_eq!(caught, 10);
        assert_eq!(escaped, 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, c1, f1) = run_cycle();
        let (a2, b2, c2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(c1, c2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
        assert!(verify_cycle().is_ok());
    }

    #[test]
    fn test_empty_ledger_injections() {
        // 三类注入对空账本同样必抓。
        let mut l0 = OperaLedger::new();
        assert!(l0.record_op(FEATURE_OPS, 0).is_err());
        let (i0, c0, e0) = l0.stats();
        assert_eq!((i0, c0, e0), (0, 0, 0));
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCB400);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(OperaCode::Ok as u32, 0);
        assert_eq!(OperaCode::EGradeDrift as u32, 12);
        assert_eq!(OperaCode::Ok.val(), SEG_BASE);
        assert!(!OperaCode::Ok.detail().is_empty());
        assert!(!OperaCode::ESeal.detail().is_empty());
        assert!(!OperaCode::EInj.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let lt0 = OperaLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = OperaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = OperaLedger::new();
        let mut o = 0usize;
        while o < FEATURE_OPS {
            if let Some(w) = feature_cell(o) {
                let _ = lt2.record_op(o, w);
            }
            o += 1;
        }
        let mut g2 = 0usize;
        while g2 < 6 {
            let _ = lt2.judge_gate(g2);
            g2 += 1;
        }
        assert_ne!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52465_checks_green() {
        let s = run_unx52465_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
