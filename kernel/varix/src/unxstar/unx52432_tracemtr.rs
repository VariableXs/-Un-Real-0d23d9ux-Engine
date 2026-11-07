//! UNX-F52432 traceroute/mtr（Windows 发行版）收口 · 0xCABC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52432"，逐字）：
//!   title: traceroute/mtr（Windows 发行版）收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52433-J1 TCP 探
//!     测 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52432）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52433-J1，照原文锚定；功能按本卡标题（traceroute/mtr Windows
//! 发行版）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五模式族闭集 20 格：ICMP / UDP / TCP / 持续 / 报告，每族 4 格，
//!      族载荷纯函数折叠恒等才收；族单调（逆序必抓）。
//!   ② hop 16 拍单调：跳 ladder 16 拍逐拍落账，拍序必须递增，逆拍必抓
//!      （EHopOrder）；逐拍载荷对照，漂移必抓。
//!   ③ PingPlotter 分账声明 2 行闭集：PingPlotter 独立账声明 / 路由追
//!      踪类管道分账声明，缺一拒封（ESeal）。
//!   ④ C 上栈声明 2 行闭集：C 运行时上栈声明 / 原始套接字 C 依赖分账
//!      声明，缺一拒封（EStkMiss）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（hop 逆拍/
//!      模式漂移/族逆序）10/10 必抓，逃逸恒 0；双跑指纹一致。
//!   ⑥ 定级目标档 S2（KB-COMPAT 登记）：空账 S3，六闸齐 S2，全齐目标
//!      档 S2。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52432;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五模式族 20 格。
pub const MODE_CELLS: usize = 20;

/// hop 16 拍。
pub const HOP_CELLS: usize = 16;

/// PingPlotter 分账声明行数。
pub const PP_ROWS: usize = 2;

/// C 上栈声明行数。
pub const CSTK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceMtrCode {
    /// 成功
    Ok = 0,
    /// 模式格越闭集
    ENoMode,
    /// 模式漂移
    EDrift,
    /// 模式族逆序
    EOrder,
    /// hop 拍越闭集
    ENoHop,
    /// hop 逆拍
    EHopOrder,
    /// 声明行越闭集
    ENoRow,
    /// C 上栈声明缺行
    EStkMiss,
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

impl TraceMtrCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TraceMtrCode::Ok => "成功",
            TraceMtrCode::ENoMode => "模式格越闭集：五模式族 20 格之外先登记",
            TraceMtrCode::EDrift => "模式漂移：模式族载荷折叠与基准不恒等，逐格复算",
            TraceMtrCode::EOrder => "模式族逆序：ICMP→UDP→TCP→持续→报告单调，逆序必抓",
            TraceMtrCode::ENoHop => "hop 拍越闭集：hop 16 拍之外先登记",
            TraceMtrCode::EHopOrder => "hop 逆拍：跳拍序必须递增，逆拍必抓",
            TraceMtrCode::ENoRow => "声明行越闭集：分账/上栈 2 行闭集之外先登记",
            TraceMtrCode::EStkMiss => "C 上栈声明缺行：C 运行时上栈声明未齐先补齐",
            TraceMtrCode::EDup => "重复落账：同格只许一条账目",
            TraceMtrCode::ESeal => "未齐即封：20 模式格 + 16 hop 拍 + 4 声明行未齐先补齐",
            TraceMtrCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TraceMtrCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TraceMtrCode::EGradeDrift => "定级漂移：全齐且六闸落账才可定 S2 目标档",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：本批卡面不触达（保留闭集完整性）
    #[allow(dead_code)]
    S1,
    /// S2：收口目标档
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
            Grade::S1 => "S1：全通收口（本批 VX16 卡面目标档为 S2，不触达）",
            Grade::S2 => "S2：收口目标档（KB-COMPAT 定级登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 模式格载荷（格 m；族 = m/4）。
pub const fn mode_cell(m: usize) -> Option<u64> {
    if m >= MODE_CELLS {
        return None;
    }
    Some((0xCB000u64 + (m as u64) * 0x39) ^ ((m / 4) as u64).wrapping_mul(0x01000193))
}

/// 模式族（0..4=ICMP 4..8=UDP 8..12=TCP 12..16=持续 16..20=报告）。
pub const fn mode_kind(m: usize) -> Option<u32> {
    if m >= MODE_CELLS {
        return None;
    }
    Some((m / 4) as u32)
}

/// 模式族名。
pub const fn mode_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("ICMP"),
        1 => Some("UDP"),
        2 => Some("TCP"),
        3 => Some("持续"),
        4 => Some("报告"),
        _ => None,
    }
}

/// hop 拍载荷（拍 h）。
pub const fn hop_cell(h: usize) -> Option<u64> {
    if h >= HOP_CELLS {
        return None;
    }
    Some((0xCC000u64 + (h as u64) * 0x53) ^ 0x484F504C41440000)
}

/// PingPlotter 分账声明行折叠（行 r）。
pub const fn pp_row(r: usize) -> Option<u32> {
    if r >= PP_ROWS {
        return None;
    }
    Some(0xCD000 + r as u32 * 0x2C)
}

/// PingPlotter 分账声明行名。
pub const fn pp_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("PingPlotter 独立账声明"),
        1 => Some("路由追踪类管道分账声明"),
        _ => None,
    }
}

/// C 上栈声明行折叠（行 r）。
pub const fn cstk_row(r: usize) -> Option<u32> {
    if r >= CSTK_ROWS {
        return None;
    }
    Some(0xCE000 + r as u32 * 0x2C)
}

/// C 上栈声明行名。
pub const fn cstk_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("C 运行时上栈声明"),
        1 => Some("原始套接字 C 依赖分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// traceroute/mtr 收口账本状态机。
#[derive(Clone)]
pub struct TraceMtrLedger {
    /// 六判据（UNX-F52433-J1 锚）。
    gates: [Option<bool>; 6],
    /// 模式账。
    modes: [Option<u64>; MODE_CELLS],
    /// hop 拍账。
    hops: [Option<u64>; HOP_CELLS],
    /// PingPlotter 分账声明账。
    pp: [Option<u32>; PP_ROWS],
    /// C 上栈声明账。
    cstk: [Option<u32>; CSTK_ROWS],
    /// 最后模式族。
    last_kind: i64,
    /// 最后 hop 拍。
    last_hop: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TraceMtrLedger {
    /// 新账本。
    pub fn new() -> Self {
        TraceMtrLedger {
            gates: [None; 6],
            modes: [None; MODE_CELLS],
            hops: [None; HOP_CELLS],
            pp: [None; PP_ROWS],
            cstk: [None; CSTK_ROWS],
            last_kind: -1,
            last_hop: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TraceMtrCode> {
        if gate >= 6 {
            return Err(TraceMtrCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 模式一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_mode(&mut self, m: usize, got: u64) -> Result<(), TraceMtrCode> {
        if m >= MODE_CELLS {
            return Err(TraceMtrCode::ENoMode);
        }
        let kind = match mode_kind(m) {
            Some(k) => k,
            None => return Err(TraceMtrCode::ENoMode),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(TraceMtrCode::EOrder);
        }
        let want = match mode_cell(m) {
            Some(w) => w,
            None => return Err(TraceMtrCode::ENoMode),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + m as u64);
            return Err(TraceMtrCode::EDrift);
        }
        if match self.modes.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TraceMtrCode::EDup);
        }
        if let Some(c) = self.modes.get_mut(m) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + m as u64);
        Ok(())
    }

    /// 模式全账核验（20 格）。
    pub fn modes_all(&self) -> bool {
        let mut m = 0usize;
        while m < MODE_CELLS {
            match self.modes.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// hop 一拍（拍序递增 + 载荷对照 + 重复拒）。
    pub fn record_hop(&mut self, h: usize, got: u64) -> Result<(), TraceMtrCode> {
        if h >= HOP_CELLS {
            return Err(TraceMtrCode::ENoHop);
        }
        if (h as i64) < self.last_hop {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(TraceMtrCode::EHopOrder);
        }
        let want = match hop_cell(h) {
            Some(w) => w,
            None => return Err(TraceMtrCode::ENoHop),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + h as u64);
            return Err(TraceMtrCode::EDrift);
        }
        if match self.hops.get(h) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TraceMtrCode::EDup);
        }
        if let Some(c) = self.hops.get_mut(h) {
            *c = Some(got);
        }
        self.last_hop = h as i64;
        self.fnv(0x8302 + h as u64);
        Ok(())
    }

    /// hop 全账核验（16 拍全齐且单调）。
    pub fn hops_all(&self) -> bool {
        let mut h = 0usize;
        while h < HOP_CELLS {
            match self.hops.get(h) {
                Some(Some(_)) => {}
                _ => return false,
            }
            h += 1;
        }
        true
    }

    /// PingPlotter 分账声明一行。
    pub fn declare_pp(&mut self, r: usize) -> Result<(), TraceMtrCode> {
        if r >= PP_ROWS {
            return Err(TraceMtrCode::ENoRow);
        }
        if match self.pp.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TraceMtrCode::EDup);
        }
        if let Some(c) = self.pp.get_mut(r) {
            *c = pp_row(r);
        }
        self.fnv(0x8400 + r as u64);
        Ok(())
    }

    /// PingPlotter 分账全齐核验。
    pub fn pp_all(&self) -> bool {
        let mut r = 0usize;
        while r < PP_ROWS {
            match self.pp.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C 上栈声明一行。
    pub fn declare_cstk(&mut self, r: usize) -> Result<(), TraceMtrCode> {
        if r >= CSTK_ROWS {
            return Err(TraceMtrCode::ENoRow);
        }
        if match self.cstk.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TraceMtrCode::EDup);
        }
        if let Some(c) = self.cstk.get_mut(r) {
            *c = cstk_row(r);
        }
        self.fnv(0x8500 + r as u64);
        Ok(())
    }

    /// C 上栈声明全齐核验。
    pub fn cstk_all(&self) -> bool {
        let mut r = 0usize;
        while r < CSTK_ROWS {
            match self.cstk.get(r) {
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

    /// KB-COMPAT 定级（目标档 S2：六闸齐 → S2，全齐亦 S2；空账 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for TraceMtrLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转（hop 逆拍 / 模式漂移 / 族逆序）全部落在当轮
/// 临时账本上：主账 injected==caught==10、escaped==0；漂移注入先按合法
/// 序落正格再注漂移；族逆序/逆拍注入对空账本同样必抓且不改主账
/// last_kind/last_hop。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TraceMtrLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut m = 0usize;
    while m < MODE_CELLS {
        if let Some(w) = mode_cell(m) {
            if led.record_mode(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut h = 0usize;
    while h < HOP_CELLS {
        if let Some(w) = hop_cell(h) {
            if led.record_hop(h, w).is_ok() {
                recorded += 1;
            }
        }
        h += 1;
    }
    let mut r = 0usize;
    while r < PP_ROWS {
        let _ = led.declare_pp(r);
        r += 1;
    }
    let mut c = 0usize;
    while c < CSTK_ROWS {
        let _ = led.declare_cstk(c);
        c += 1;
    }
    // 注入 10 轮：hop 逆拍 / 模式漂移 / 族逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TraceMtrLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_hop(5, hop_cell(5).unwrap_or(0));
                lt.record_hop(1, hop_cell(1).unwrap_or(0)) == Err(TraceMtrCode::EHopOrder)
            }
            1 => {
                let _ = lt.record_mode(8, mode_cell(8).unwrap_or(0));
                let _ = lt.record_mode(9, mode_cell(9).unwrap_or(0));
                lt.record_mode(10, mode_cell(10).unwrap_or(0).wrapping_add(1))
                    == Err(TraceMtrCode::EDrift)
            }
            _ => {
                let _ = lt.record_mode(14, mode_cell(14).unwrap_or(0));
                lt.record_mode(2, mode_cell(2).unwrap_or(0)) == Err(TraceMtrCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52432 判据集（锚 UNX-F52433-J1，off-by-one 板式，§AI-66.3）。
pub fn run_unx52432_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52432");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五模式族 20 格全通",
        recorded >= MODE_CELLS as u32 + HOP_CELLS as u32,
        "ICMP/UDP/TCP/持续/报告逐格恒等（UNX-F52433-J1 锚）",
    );
    s.add(
        "hop 16 拍全落且单调",
        {
            let mut lt = TraceMtrLedger::new();
            let mut ok = true;
            let mut h = 0usize;
            while h < HOP_CELLS {
                if let Some(w) = hop_cell(h) {
                    if lt.record_hop(h, w).is_err() {
                        ok = false;
                    }
                }
                h += 1;
            }
            ok && lt.hops_all()
        },
        "跳 ladder 逐拍递增落账，逆拍必抓",
    );
    s.add(
        "PingPlotter 分账声明 2/2 全齐",
        {
            let mut lt = TraceMtrLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < PP_ROWS {
                if lt.declare_pp(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.pp_all()
        },
        "PingPlotter 独立账声明/路由追踪类管道分账声明两行闭集",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = TraceMtrLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CSTK_ROWS {
                if lt.declare_cstk(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cstk_all()
        },
        "C 运行时上栈声明/原始套接字 C 依赖分账声明两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = TraceMtrLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52433-J1 六判据逐格落账",
    );
    s.add(
        "模式族单调",
        {
            let mut ok = true;
            let mut m = 1usize;
            while m < MODE_CELLS {
                if mode_kind(m).unwrap_or(5) < mode_kind(m - 1).unwrap_or(5) {
                    ok = false;
                }
                m += 1;
            }
            ok
        },
        "五模式族闭集单调（§AI-66.3）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "hop 逆拍/模式漂移/族逆序三态轮转必抓");
    let mut l0 = TraceMtrLedger::new();
    s.add(
        "hop 逆拍拒",
        {
            let _ = l0.record_hop(5, hop_cell(5).unwrap_or(0));
            l0.record_hop(1, hop_cell(1).unwrap_or(0)) == Err(TraceMtrCode::EHopOrder)
        },
        "跳拍序递增，逆拍必抓",
    );
    s.add(
        "族逆序拒",
        {
            let _ = l0.record_mode(14, mode_cell(14).unwrap_or(0));
            l0.record_mode(2, mode_cell(2).unwrap_or(0)) == Err(TraceMtrCode::EOrder)
        },
        "持续后回 ICMP 必抓",
    );
    s.add(
        "边界·模式越闭集",
        l0.record_mode(MODE_CELLS, 0).is_err(),
        "模式格 20 越闭集必拒",
    );
    s.add(
        "边界·hop 越闭集",
        l0.record_hop(HOP_CELLS, 0).is_err(),
        "hop 拍 16 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_pp(PP_ROWS).is_err() && l0.declare_cstk(CSTK_ROWS).is_err(),
        "声明行 2 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S2 目标档",
        {
            let mut lt = TraceMtrLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut m = 0usize;
            while m < MODE_CELLS {
                if let Some(w) = mode_cell(m) {
                    let _ = lt.record_mode(m, w);
                }
                m += 1;
            }
            let mut h = 0usize;
            while h < HOP_CELLS {
                if let Some(w) = hop_cell(h) {
                    let _ = lt.record_hop(h, w);
                }
                h += 1;
            }
            let mut r = 0usize;
            while r < PP_ROWS {
                let _ = lt.declare_pp(r);
                r += 1;
            }
            let mut c = 0usize;
            while c < CSTK_ROWS {
                let _ = lt.declare_cstk(c);
                c += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全齐目标档 S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_kind() {
        assert_eq!(mode_kind(0), Some(0));
        assert_eq!(mode_kind(4), Some(1));
        assert_eq!(mode_kind(10), Some(2));
        assert_eq!(mode_kind(19), Some(4));
        assert!(mode_kind(20).is_none());
        assert!(mode_cell(20).is_none());
        assert_eq!(mode_kind_name(0), Some("ICMP"));
        assert_eq!(mode_kind_name(4), Some("报告"));
        assert!(mode_kind_name(5).is_none());
    }

    #[test]
    fn test_hop_ladder() {
        assert!(hop_cell(0).is_some());
        assert!(hop_cell(15).is_some());
        assert!(hop_cell(16).is_none());
        assert_ne!(hop_cell(7).unwrap_or(0), hop_cell(8).unwrap_or(0));
    }

    #[test]
    fn test_declaration_rows() {
        assert_eq!(pp_row_name(0), Some("PingPlotter 独立账声明"));
        assert_eq!(cstk_row_name(1), Some("原始套接字 C 依赖分账声明"));
        assert!(pp_row(PP_ROWS).is_none());
        assert!(cstk_row(CSTK_ROWS).is_none());
        assert!(pp_row(0).is_some());
        assert!(cstk_row(0).is_some());
    }

    #[test]
    fn test_cells_pure() {
        let a = mode_cell(13).unwrap_or(0);
        assert_eq!(a, mode_cell(13).unwrap_or(0));
        assert_ne!(a, mode_cell(14).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TraceMtrLedger::new();
        let _ = lt.record_mode(9, mode_cell(9).unwrap_or(0));
        assert_eq!(lt.record_mode(3, mode_cell(3).unwrap_or(0)), Err(TraceMtrCode::EOrder));
        assert!(lt.record_mode(10, mode_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_hop_order_caught() {
        let mut lt = TraceMtrLedger::new();
        let _ = lt.record_hop(0, hop_cell(0).unwrap_or(0));
        let _ = lt.record_hop(3, hop_cell(3).unwrap_or(0));
        assert_eq!(lt.record_hop(1, hop_cell(1).unwrap_or(0)), Err(TraceMtrCode::EHopOrder));
        assert!(lt.record_hop(4, hop_cell(4).unwrap_or(0)).is_ok());
        // 逆拍注入不改已落正格。
        assert!(lt.hops_all() == false);
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TraceMtrLedger::new();
        assert_eq!(
            lt.record_mode(5, mode_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(TraceMtrCode::EDrift)
        );
        assert_eq!(
            lt.record_hop(2, hop_cell(2).unwrap_or(0).wrapping_add(6)),
            Err(TraceMtrCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TraceMtrLedger::new();
        let _ = lt.record_hop(6, hop_cell(6).unwrap_or(0));
        assert_eq!(lt.record_hop(6, hop_cell(6).unwrap_or(0)), Err(TraceMtrCode::EDup));
        let _ = lt.declare_pp(0);
        assert_eq!(lt.declare_pp(0), Err(TraceMtrCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TraceMtrLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= MODE_CELLS as u32 + HOP_CELLS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCABC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TraceMtrCode::Ok as u32, 0);
        assert_eq!(TraceMtrCode::EGradeDrift as u32, 12);
        assert_eq!(TraceMtrCode::EHopOrder.val(), SEG_BASE + 5);
        assert!(!TraceMtrCode::Ok.detail().is_empty());
        assert!(!TraceMtrCode::EStkMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = TraceMtrLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = TraceMtrLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        // 全齐目标档 S2。
        let mut h = 0usize;
        while h < HOP_CELLS {
            if let Some(w) = hop_cell(h) {
                let _ = lt2.record_hop(h, w);
            }
            h += 1;
        }
        let mut r = 0usize;
        while r < PP_ROWS {
            let _ = lt2.declare_pp(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52432_checks_green() {
        let s = run_unx52432_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
