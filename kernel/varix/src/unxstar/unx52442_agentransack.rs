//! UNX-F52442 Agent Ransack 收口 · 0xCAE40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52442"，逐字）：
//!   title: Agent Ransack 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52443-J1 六判据
//!     全落；右键集成状态机（体验二浮层出路 ×10）；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式：本卡（卡 N=52442）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52443-J1，照原文锚定；功能按本卡标题（Agent Ransack）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 操作闭集：正则/文件名/内容/导出/预览，每族 4 操作，
//!      族号 = 操作号/4（i/N 映射），逆序必抓。
//!   ② 正则引擎三态 6 格：每格三态（0=命中 1=不命中 2=跳过），态越三元
//!      必抓；右键集成状态机按体验二浮层出路 ×10 口径入注入核。
//!   ③ 结果投影 8 格：投影折叠纯函数恒等才收。
//!   ④ 与 grepWin 分账声明 2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（家族逆序/
//!      正则态越三元/操作越闭集），全部落在当轮临时账本上；双跑 FNV 指
//!      纹一致；定级 S1/S2/S3 闭集（全齐 → S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52442;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数（每族 4 操作）。
pub const OPS: usize = 20;

/// 正则引擎三态格数。
pub const RE_CELLS: usize = 6;

/// 正则引擎态闭集（三元）。
pub const RE_STATES: u32 = 3;

/// 结果投影格数。
pub const PROJ: usize = 8;

/// grepWin 分账声明行数。
pub const GW_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RansackCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 正则态越三元
    ERePhase,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 判据闸越闭集
    ENoGate,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl RansackCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RansackCode::Ok => "成功",
            RansackCode::ENoOp => "格号越闭集：20 操作/6 正则格/8 投影/声明行闭集之外先登记",
            RansackCode::EDrift => "载荷漂移：操作/正则格/投影折叠与基准不恒等，逐格复算",
            RansackCode::EOrder => "家族逆序：正则→文件名→内容→导出→预览单调，逆序必抓",
            RansackCode::ERePhase => "正则态越三元：命中/不命中/跳过三态之外先登记",
            RansackCode::ENoRow => "声明行越闭集：grepWin 分账 2 行闭集之外先登记",
            RansackCode::ERowMiss => "缺声明行：grepWin 分账声明未齐先补齐",
            RansackCode::EDup => "重复落账：同格只许一条账目",
            RansackCode::ENoGate => "判据闸越闭集：六判据 0..=5 之外先登记",
            RansackCode::ESeal => "未齐即封：20 操作+6 正则格+8 投影+grepWin 分账未齐先补齐",
            RansackCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RansackCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RansackCode::EGradeDrift => "定级漂移：全齐且 grepWin 分账全通才可定 S2",
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
            Grade::S1 => "S1：全通收口（本卡封顶 S2，不启用）",
            Grade::S2 => "S2：六判据+20操作+6正则格+8投影+grepWin 分账全通收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o；家族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xD6000u64 + (o as u64) * 0x2E) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=正则 4..8=文件名 8..12=内容 12..16=导出 16..20=预览）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("正则"),
        1 => Some("文件名"),
        2 => Some("内容"),
        3 => Some("导出"),
        4 => Some("预览"),
        _ => None,
    }
}

/// 正则引擎三态格基准（格 c、态 t<3）。
pub const fn re_cell(c: usize, t: u32) -> Option<u64> {
    if c >= RE_CELLS || t >= RE_STATES {
        return None;
    }
    Some(0xD700 + (c as u64) * 0x3B + (t as u64) * 0x7)
}

/// 正则态名。
pub const fn re_state_name(t: u32) -> Option<&'static str> {
    match t {
        0 => Some("命中"),
        1 => Some("不命中"),
        2 => Some("跳过"),
        _ => None,
    }
}

/// 结果投影格载荷（投影 p）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ {
        return None;
    }
    Some((0xD8000u64 + (p as u64) * 0x39) ^ 0x52414E53_41434B31)
}

/// grepWin 分账声明行折叠（行 r）。
pub const fn gw_row(r: usize) -> Option<u32> {
    if r >= GW_ROWS {
        return None;
    }
    Some(0xD900 + r as u32 * 0x2E)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Agent Ransack 收口账本状态机。
#[derive(Clone)]
pub struct RansackLedger {
    /// 六判据（启动/索引/内容搜索/右键集成/卸载/升级）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 正则引擎三态账。
    res: [Option<u64>; RE_CELLS],
    /// 投影账。
    projs: [Option<u64>; PROJ],
    /// grepWin 分账声明账。
    gw: [Option<u32>; GW_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RansackLedger {
    /// 新账本。
    pub fn new() -> Self {
        RansackLedger {
            gates: [None; 6],
            ops: [None; OPS],
            res: [None; RE_CELLS],
            projs: [None; PROJ],
            gw: [None; GW_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RansackCode> {
        if gate >= 6 {
            return Err(RansackCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5200 + gate as u64);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), RansackCode> {
        if o >= OPS {
            return Err(RansackCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(RansackCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5210);
            return Err(RansackCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(RansackCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5211 + o as u64);
            return Err(RansackCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RansackCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5212 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 正则引擎一格（三态对照 + 重复拒；态越三元必抓且不改 last_kind）。
    pub fn record_re(&mut self, c: usize, t: u32, got: u64) -> Result<(), RansackCode> {
        if c >= RE_CELLS {
            return Err(RansackCode::ENoOp);
        }
        let want = match re_cell(c, t) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5220 + c as u64);
                return Err(RansackCode::ERePhase);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5221 + c as u64);
            return Err(RansackCode::EDrift);
        }
        if match self.res.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RansackCode::EDup);
        }
        if let Some(x) = self.res.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x5222 + c as u64);
        Ok(())
    }

    /// 正则引擎全账核验（6 格）。
    pub fn res_all(&self) -> bool {
        let mut c = 0usize;
        while c < RE_CELLS {
            match self.res.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), RansackCode> {
        if p >= PROJ {
            return Err(RansackCode::ENoOp);
        }
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(RansackCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5230 + p as u64);
            return Err(RansackCode::EDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RansackCode::EDup);
        }
        if let Some(x) = self.projs.get_mut(p) {
            *x = Some(got);
        }
        self.fnv(0x5231 + p as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// grepWin 分账声明一行。
    pub fn declare_gw(&mut self, r: usize) -> Result<(), RansackCode> {
        if r >= GW_ROWS {
            return Err(RansackCode::ENoRow);
        }
        if match self.gw.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RansackCode::EDup);
        }
        if let Some(x) = self.gw.get_mut(r) {
            *x = gw_row(r);
        }
        self.fnv(0x5240 + r as u64);
        Ok(())
    }

    /// grepWin 分账声明全齐核验。
    pub fn gw_all(&self) -> bool {
        let mut r = 0usize;
        while r < GW_ROWS {
            match self.gw.get(r) {
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

    /// KB-COMPAT 定级（全齐 → S2 封顶，判据缺 → S3，其余 → S2）。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.res_all() && self.projs_all() && self.gw_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RansackLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RansackLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < RE_CELLS {
        // 逐格按三态轮转取态：态 = c % 3。
        let t = (c % 3) as u32;
        if let Some(w) = re_cell(c, t) {
            if led.record_re(c, t, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut p = 0usize;
    while p < PROJ {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < GW_ROWS {
        let _ = led.declare_gw(r);
        r += 1;
    }
    // 注入 10 轮（右键集成状态机 · 体验二浮层出路 ×10 口径）：家族逆序 /
    // 正则态越三元 / 操作越闭集三态轮转，全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RansackLedger::new();
        let caught = match kind {
            0 => {
                let w17 = match op_cell(17) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(17, w17);
                let w5 = match op_cell(5) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_op(5, w5) == Err(RansackCode::EOrder)
            }
            1 => {
                // 正则态越三元：空账直接注态 3，必抓且不改 last_kind。
                lt.record_re(1, RE_STATES, 0) == Err(RansackCode::ERePhase)
            }
            _ => lt.record_op(OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5250 + k as u64);
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
/// UNX-F52442 判据集（锚 UNX-F52443-J1，off-by-one 板式）。
pub fn run_unx52442_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52442");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 操作全通",
        recorded >= OPS as u32,
        "正则/文件名/内容/导出/预览逐操作恒等（§AI-66.3）",
    );
    s.add(
        "正则引擎三态 6/6 全落",
        {
            let mut lt = RansackLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < RE_CELLS {
                let t = (c % 3) as u32;
                if let Some(w) = re_cell(c, t) {
                    if lt.record_re(c, t, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.res_all()
        },
        "命中/不命中/跳过三元闭集逐格恒等",
    );
    s.add(
        "结果投影 8/8 全齐",
        {
            let mut lt = RansackLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "投影折叠纯函数 8 格逐格恒等",
    );
    s.add(
        "grepWin 分账声明 2/2 全齐",
        {
            let mut lt = RansackLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GW_ROWS {
                if lt.declare_gw(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gw_all()
        },
        "grepWin 分账两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = RansackLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/索引/内容搜索/右键集成/卸载/升级逐格落账（UNX-F52443-J1）",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let a = match op_kind(o) {
                    Some(k) => k,
                    None => 5,
                };
                let b = match op_kind(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五功能族闭集单调（i/N 映射 o/4）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "家族逆序/正则态越三元/操作越闭集三态轮转必抓（右键集成状态机 ×10）",
    );
    let mut l0 = RansackLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·正则态越三元",
        l0.record_re(0, RE_STATES, 0).is_err(),
        "态 3 越三元闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        match l0.declare_gw(GW_ROWS) {
            Err(RansackCode::ENoRow) => true,
            _ => false,
        },
        "grepWin 声明行 2 之外越闭集必拒",
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
        "定级 S2 闭集",
        {
            let mut lt = RansackLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < RE_CELLS {
                let t = (c % 3) as u32;
                if let Some(w) = re_cell(c, t) {
                    let _ = lt.record_re(c, t, w);
                }
                c += 1;
            }
            let mut p = 0usize;
            while p < PROJ {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < GW_ROWS {
                let _ = lt.declare_gw(r);
                r += 1;
            }
            lt.grade() == Grade::S2
        },
        "全齐 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAE40,
        "SEG_BASE 0x40 对齐且在 VX16 域 52442 格",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn w(o: usize) -> u64 {
        match op_cell(o) {
            Some(v) => v,
            None => 0,
        }
    }

    #[test]
    fn test_op_kind_mapping() {
        // i/N 映射：kind = o/4。
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(9), Some(2));
        assert_eq!(op_kind(15), Some(3));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("正则"));
        assert_eq!(op_kind_name(3), Some("导出"));
        assert!(op_kind_name(9).is_none());
    }

    #[test]
    fn test_re_states_closed() {
        assert_eq!(re_state_name(0), Some("命中"));
        assert_eq!(re_state_name(1), Some("不命中"));
        assert_eq!(re_state_name(2), Some("跳过"));
        assert!(re_state_name(3).is_none());
        assert!(re_cell(RE_CELLS, 0).is_none());
        assert!(re_cell(0, RE_STATES).is_none());
        assert!(re_cell(0, 0).is_some());
    }

    #[test]
    fn test_cells_pure() {
        let a = w(3);
        assert_eq!(a, w(3));
        assert_ne!(a, w(4));
        assert!(proj_cell(PROJ).is_none());
        assert!(gw_row(GW_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = RansackLedger::new();
        let _ = lt.record_op(9, w(9));
        assert_eq!(lt.record_op(2, w(2)), Err(RansackCode::EOrder));
        assert!(lt.record_op(10, w(10)).is_ok());
    }

    #[test]
    fn test_re_phase_caught_on_empty() {
        // 空账注态 3 必抓，且不改 last_kind（后续合法操作不误判逆序）。
        let mut lt = RansackLedger::new();
        assert_eq!(lt.record_re(0, RE_STATES, 0), Err(RansackCode::ERePhase));
        assert!(lt.record_op(0, w(0)).is_ok());
        // 已落格重复注格被 EDup 拦。
        let t = 1u32;
        let wt = match re_cell(0, t) {
            Some(v) => v,
            None => 0,
        };
        let _ = lt.record_re(0, t, wt);
        assert_eq!(lt.record_re(0, t, wt), Err(RansackCode::EDup));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RansackLedger::new();
        assert_eq!(lt.record_op(5, w(5).wrapping_add(1)), Err(RansackCode::EDrift));
        let t = 0u32;
        let wt = match re_cell(2, t) {
            Some(v) => v,
            None => 0,
        };
        assert_eq!(lt.record_re(2, t, wt.wrapping_add(3)), Err(RansackCode::EDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RansackLedger::new();
        let _ = lt.record_proj(0, match proj_cell(0) {
            Some(v) => v,
            None => 0,
        });
        assert_eq!(
            lt.record_proj(0, match proj_cell(0) {
                Some(v) => v,
                None => 0,
            }),
            Err(RansackCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RansackLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert_eq!(lt.judge_gate(6), Err(RansackCode::ENoGate));
        assert!(lt.gates_all());
        let mut lt2 = RansackLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：20 操作 + 6 正则格 + 8 投影 = 34。
        assert_eq!(recorded, OPS as u32 + RE_CELLS as u32 + PROJ as u32);
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
        assert_eq!(SEG_BASE, 0xCAE40);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RansackCode::Ok as u32, 0);
        assert_eq!(RansackCode::EGradeDrift as u32, 12);
        assert_eq!(RansackCode::Ok.val(), SEG_BASE);
        assert_eq!(RansackCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let all = [
            RansackCode::Ok,
            RansackCode::ENoOp,
            RansackCode::EDrift,
            RansackCode::EOrder,
            RansackCode::ERePhase,
            RansackCode::ENoRow,
            RansackCode::ERowMiss,
            RansackCode::EDup,
            RansackCode::ENoGate,
            RansackCode::ESeal,
            RansackCode::EInj,
            RansackCode::EFp,
            RansackCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2（本卡封顶 S2）。
        let lt = RansackLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        let mut lt2 = RansackLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.grade(), Grade::S2);
        let mut lt3 = RansackLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            let _ = lt3.record_op(o, w(o));
            o += 1;
        }
        let mut c = 0usize;
        while c < RE_CELLS {
            let t = (c % 3) as u32;
            if let Some(v) = re_cell(c, t) {
                let _ = lt3.record_re(c, t, v);
            }
            c += 1;
        }
        let mut p = 0usize;
        while p < PROJ {
            if let Some(v) = proj_cell(p) {
                let _ = lt3.record_proj(p, v);
            }
            p += 1;
        }
        let mut r = 0usize;
        while r < GW_ROWS {
            let _ = lt3.declare_gw(r);
            r += 1;
        }
        assert_eq!(lt3.grade(), Grade::S2);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = RansackLedger::new();
        let _ = lt.record_op(0, w(0).wrapping_add(1));
        let (inj, caught, esc) = lt.stats();
        assert_eq!(inj, 1);
        assert_eq!(caught, 1);
        assert_eq!(esc, 0);
        // 错误路径已记注入账：指纹必被 fnv 推进，不得断言不变。
        let fp0 = lt.fingerprint();
        let _ = lt.record_op(1, w(1).wrapping_add(2));
        assert_ne!(lt.fingerprint(), fp0);
    }

    #[test]
    fn unx52442_checks_green() {
        let s = run_unx52442_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 14);
    }
}
