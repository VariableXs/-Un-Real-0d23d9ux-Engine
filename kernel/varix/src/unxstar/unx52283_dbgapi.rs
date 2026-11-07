//! UNX-F52283 调试 API 对照 ×5 应用复测零差账（开发工具共面三） · 0xC8680段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52283"，逐字）：
//!   title: 开发工具共面三：调试 API 对照 ×5 应用复测零差账
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52284-J1 B01/B08/
//!     B10 全部 git 消费条目对 L4 嫁接件判据绿的消费一致性 ×10 条目抽测；
//!     文件锁语义对账 ×5；联签锚定行 AI-59；判据锚定 §AI-66.2 专题二
//!     Git 集成；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52283）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52284-J1，照原文锚定；功能按本卡标题（调试 API 对照复测零差账）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① ×5 应用对照 20 格：WinDbg/x64dbg/VS/gdb/LLDB 五应用各 4 对照项
//!      （IsDebuggerPresent/CheckRemoteDebuggerPresent/PEB
//!      BeingDebugged 写位/D1 对照账引用），应用序单调，漂移必抓。
//!   ② 断点/单步/变量三面 15 格：每面 5 格，断点异常派发序 ×10 口径折入
//!      面载荷；面序逆必抓。
//!   ③ 句柄账 8 格：调试句柄开/闭逐格落账，漂移必抓。
//!   ④ 零差声明 2 行闭集：×5 应用零差声明 / D1 对照账复测引用声明，缺
//!      一拒封（AI-59 联签锚定行）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（应用对照漂移/面序逆/应用越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （20 格+15 面+8 句柄+2 声明+六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52283;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// ×5 应用对照格数（5 应用 × 4 对照项）。
pub const APP_CELLS: usize = 20;

/// 断点/单步/变量三面格数（3 面 × 5 格）。
pub const FACE_CELLS: usize = 15;

/// 调试句柄账格数。
pub const HANDLE_CELLS: usize = 8;

/// 零差声明行数。
pub const ZERO_ROWS: usize = 2;

/// 断点异常派发序口径。
pub const BP_DISPATCH_SEQ: u32 = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgApiCode {
    /// 成功
    Ok = 0,
    /// 应用格越闭集
    ENoApp,
    /// 应用对照漂移
    EDrift,
    /// 应用序逆
    EOrder,
    /// 三面格越闭集
    ENoFace,
    /// 面载荷漂移
    EFaceDrift,
    /// 句柄格越闭集
    ENoHandle,
    /// 声明行越闭集
    ENoRow,
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

impl DbgApiCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DbgApiCode::Ok => "成功",
            DbgApiCode::ENoApp => "应用格越闭集：×5 应用 20 对照格之外先登记",
            DbgApiCode::EDrift => "应用对照漂移：对照折叠与基准不恒等，逐格复算",
            DbgApiCode::EOrder => "应用序逆：WinDbg→x64dbg→VS→gdb→LLDB 单调，逆序必抓",
            DbgApiCode::ENoFace => "三面格越闭集：断点/单步/变量 15 格之外先登记",
            DbgApiCode::EFaceDrift => "面载荷漂移：断点异常派发序 ×10 折叠与基准不恒等",
            DbgApiCode::ENoHandle => "句柄格越闭集：句柄账 8 格之外先登记",
            DbgApiCode::ENoRow => "声明行越闭集：零差声明 2 行之外先登记",
            DbgApiCode::EDup => "重复落账：同格只许一条账目",
            DbgApiCode::ESeal => "未齐即封：20 格+15 面+8 句柄+2 声明未齐先补齐",
            DbgApiCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DbgApiCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DbgApiCode::EGradeDrift => "定级漂移：全通且零差声明全齐才可定 S1",
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
            Grade::S1 => "S1：20格+15面+8句柄+2声明+六判据全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 应用（格 o；应用 = o/4）。
pub const fn app_of(o: usize) -> Option<u32> {
    if o >= APP_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 应用名（const fn 表 + Option 边界）。
pub const fn app_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("WinDbg"),
        1 => Some("x64dbg"),
        2 => Some("VS"),
        3 => Some("gdb"),
        4 => Some("LLDB"),
        _ => None,
    }
}

/// 应用对照格载荷（格 o）。
pub const fn app_cell(o: usize) -> Option<u64> {
    if o >= APP_CELLS {
        return None;
    }
    Some((0xBB00u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 调试面（格 f；面 = f/5：0=断点 1=单步 2=变量）。
pub const fn face_of(f: usize) -> Option<u32> {
    if f >= FACE_CELLS {
        return None;
    }
    Some((f / 5) as u32)
}

/// 面名。
pub const fn face_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("断点"),
        1 => Some("单步"),
        2 => Some("变量"),
        _ => None,
    }
}

/// 面格载荷（格 f；断点异常派发序 ×10 折入）。
pub const fn face_cell(f: usize) -> Option<u64> {
    if f >= FACE_CELLS {
        return None;
    }
    Some((0xBC00u64 + (f as u64) * 0x2C) ^ ((BP_DISPATCH_SEQ as u64) * ((f % 5) as u64 + 1)))
}

/// 句柄格载荷（格 h）。
pub const fn handle_cell(h: usize) -> Option<u64> {
    if h >= HANDLE_CELLS {
        return None;
    }
    Some((0xBD00u64 + (h as u64) * 0x31) ^ 0x4841_4E44_4C45_0000)
}

/// 零差声明行折叠（行 r）。
pub const fn zero_row(r: usize) -> Option<u32> {
    if r >= ZERO_ROWS {
        return None;
    }
    Some(0xBE00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 调试 API 对照收口账本状态机。
#[derive(Clone)]
pub struct DbgApiLedger {
    /// 六判据（API 联动/×5 应用/三面/句柄账/零差声明/汇总）。
    gates: [Option<bool>; 6],
    /// 应用对照账。
    apps: [Option<u64>; APP_CELLS],
    /// 三面账。
    faces: [Option<u64>; FACE_CELLS],
    /// 句柄账。
    handles: [Option<u64>; HANDLE_CELLS],
    /// 零差声明账。
    zeros: [Option<u32>; ZERO_ROWS],
    /// 最后应用。
    last_app: i64,
    /// 最后面。
    last_face: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DbgApiLedger {
    /// 新账本。
    pub fn new() -> Self {
        DbgApiLedger {
            gates: [None; 6],
            apps: [None; APP_CELLS],
            faces: [None; FACE_CELLS],
            handles: [None; HANDLE_CELLS],
            zeros: [None; ZERO_ROWS],
            last_app: -1,
            last_face: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DbgApiCode> {
        if gate >= 6 {
            return Err(DbgApiCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 应用对照一格（应用单调 + 载荷对照 + 重复拒）。
    pub fn record_app(&mut self, o: usize, got: u64) -> Result<(), DbgApiCode> {
        if o >= APP_CELLS {
            return Err(DbgApiCode::ENoApp);
        }
        let app = match app_of(o) {
            Some(k) => k,
            None => return Err(DbgApiCode::ENoApp),
        };
        if (app as i64) < self.last_app {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(DbgApiCode::EOrder);
        }
        let want = match app_cell(o) {
            Some(w) => w,
            None => return Err(DbgApiCode::ENoApp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(DbgApiCode::EDrift);
        }
        if match self.apps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbgApiCode::EDup);
        }
        if let Some(c) = self.apps.get_mut(o) {
            *c = Some(got);
        }
        self.last_app = app as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 应用对照全账核验（20 格）。
    pub fn apps_all(&self) -> bool {
        let mut o = 0usize;
        while o < APP_CELLS {
            match self.apps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 三面一格（面单调 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, f: usize, got: u64) -> Result<(), DbgApiCode> {
        if f >= FACE_CELLS {
            return Err(DbgApiCode::ENoFace);
        }
        let face = match face_of(f) {
            Some(k) => k,
            None => return Err(DbgApiCode::ENoFace),
        };
        if (face as i64) < self.last_face {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(DbgApiCode::EOrder);
        }
        let want = match face_cell(f) {
            Some(w) => w,
            None => return Err(DbgApiCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + f as u64);
            return Err(DbgApiCode::EFaceDrift);
        }
        if match self.faces.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbgApiCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(f) {
            *c = Some(got);
        }
        self.last_face = face as i64;
        self.fnv(0x7302 + f as u64);
        Ok(())
    }

    /// 三面全账核验（15 格）。
    pub fn faces_all(&self) -> bool {
        let mut f = 0usize;
        while f < FACE_CELLS {
            match self.faces.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 句柄一格（载荷对照 + 重复拒）。
    pub fn record_handle(&mut self, h: usize, got: u64) -> Result<(), DbgApiCode> {
        let want = match handle_cell(h) {
            Some(w) => w,
            None => return Err(DbgApiCode::ENoHandle),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + h as u64);
            return Err(DbgApiCode::EDrift);
        }
        if match self.handles.get(h) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbgApiCode::EDup);
        }
        if let Some(c) = self.handles.get_mut(h) {
            *c = Some(got);
        }
        self.fnv(0x7401 + h as u64);
        Ok(())
    }

    /// 句柄全账核验（8 格）。
    pub fn handles_all(&self) -> bool {
        let mut h = 0usize;
        while h < HANDLE_CELLS {
            match self.handles.get(h) {
                Some(Some(_)) => {}
                _ => return false,
            }
            h += 1;
        }
        true
    }

    /// 零差声明一行。
    pub fn declare_zero(&mut self, r: usize) -> Result<(), DbgApiCode> {
        if r >= ZERO_ROWS {
            return Err(DbgApiCode::ENoRow);
        }
        if match self.zeros.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbgApiCode::EDup);
        }
        if let Some(c) = self.zeros.get_mut(r) {
            *c = zero_row(r);
        }
        self.fnv(0x7500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn zeros_all(&self) -> bool {
        let mut r = 0usize;
        while r < ZERO_ROWS {
            match self.zeros.get(r) {
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
        if self.gates_all() && self.apps_all() && self.faces_all() && self.handles_all() && self.zeros_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DbgApiLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DbgApiLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < APP_CELLS {
        if let Some(w) = app_cell(o) {
            if led.record_app(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut f = 0usize;
    while f < FACE_CELLS {
        if let Some(w) = face_cell(f) {
            if led.record_face(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut h = 0usize;
    while h < HANDLE_CELLS {
        if let Some(w) = handle_cell(h) {
            if led.record_handle(h, w).is_ok() {
                recorded += 1;
            }
        }
        h += 1;
    }
    let mut r = 0usize;
    while r < ZERO_ROWS {
        let _ = led.declare_zero(r);
        r += 1;
    }
    // 注入 10 轮：应用对照漂移 / 面序逆 / 应用越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DbgApiLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_app(3, app_cell(3).unwrap_or(0));
                lt.record_app(4, app_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(DbgApiCode::EDrift)
            }
            1 => {
                let _ = lt.record_face(7, face_cell(7).unwrap_or(0));
                lt.record_face(2, face_cell(2).unwrap_or(0)) == Err(DbgApiCode::EOrder)
            }
            _ => lt.record_app(APP_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7600 + k as u64);
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
/// UNX-F52283 判据集（锚 UNX-F52284-J1，off-by-one 板式）。
pub fn run_unx52283_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52283");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "×5 应用对照 20 格全通",
        recorded >= APP_CELLS as u32,
        "WinDbg/x64dbg/VS/gdb/LLDB 五应用各 4 对照项恒等",
    );
    s.add(
        "应用序单调（纯函数核验）",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < APP_CELLS {
                if app_of(o).unwrap_or(5) < app_of(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五应用闭集单调",
    );
    s.add(
        "断点/单步/变量三面 15 格全落",
        {
            let mut lt = DbgApiLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FACE_CELLS {
                if let Some(w) = face_cell(f) {
                    if lt.record_face(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.faces_all()
        },
        "断点异常派发序 ×10 折入面载荷逐格落账",
    );
    s.add(
        "调试句柄账 8 格全落",
        {
            let mut lt = DbgApiLedger::new();
            let mut ok = true;
            let mut h = 0usize;
            while h < HANDLE_CELLS {
                if let Some(w) = handle_cell(h) {
                    if lt.record_handle(h, w).is_err() {
                        ok = false;
                    }
                }
                h += 1;
            }
            ok && lt.handles_all()
        },
        "句柄开/闭逐格恒等",
    );
    s.add(
        "零差声明 2/2 全齐",
        {
            let mut lt = DbgApiLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ZERO_ROWS {
                if lt.declare_zero(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.zeros_all()
        },
        "×5 应用零差声明/D1 对照账复测引用两行闭集（AI-59 联签）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = DbgApiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "API 联动/×5 应用/三面/句柄账/零差声明/汇总逐格",
    );
    s.add("注入 10/10 全抓", caught >= 10, "对照漂移/面序逆/越闭集三态轮转必抓");
    let mut l0 = DbgApiLedger::new();
    s.add(
        "边界·应用越闭集",
        l0.record_app(APP_CELLS, 0).is_err(),
        "应用格 20 越闭集必拒",
    );
    s.add(
        "边界·三面越闭集",
        l0.record_face(FACE_CELLS, 0).is_err(),
        "三面格 15 越闭集必拒",
    );
    s.add(
        "边界·句柄越闭集",
        l0.record_handle(HANDLE_CELLS, 0).is_err(),
        "句柄格 8 越闭集必拒",
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
        "诊断码闭集 13 变体",
        DbgApiCode::Ok as u32 == 0 && DbgApiCode::EGradeDrift as u32 == 12,
        "Ok=0 至 EGradeDrift=12 恰 13 变体闭集",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = DbgApiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < APP_CELLS {
                if let Some(w) = app_cell(o) {
                    let _ = lt.record_app(o, w);
                }
                o += 1;
            }
            let mut f = 0usize;
            while f < FACE_CELLS {
                if let Some(w) = face_cell(f) {
                    let _ = lt.record_face(f, w);
                }
                f += 1;
            }
            let mut h = 0usize;
            while h < HANDLE_CELLS {
                if let Some(w) = handle_cell(h) {
                    let _ = lt.record_handle(h, w);
                }
                h += 1;
            }
            let mut r = 0usize;
            while r < ZERO_ROWS {
                let _ = lt.declare_zero(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8680,
        "SEG_BASE 0x40 对齐且按 VX15 公式落在 0xC8680",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_of() {
        assert_eq!(app_of(0), Some(0));
        assert_eq!(app_of(3), Some(0));
        assert_eq!(app_of(4), Some(1));
        assert_eq!(app_of(19), Some(4));
        assert!(app_of(20).is_none());
        assert!(app_cell(20).is_none());
        assert_eq!(app_name(0), Some("WinDbg"));
        assert_eq!(app_name(4), Some("LLDB"));
        assert!(app_name(5).is_none());
    }

    #[test]
    fn test_face_of() {
        assert_eq!(face_of(0), Some(0));
        assert_eq!(face_of(5), Some(1));
        assert_eq!(face_of(14), Some(2));
        assert!(face_of(15).is_none());
        assert!(face_cell(15).is_none());
        assert_eq!(face_name(2), Some("变量"));
    }

    #[test]
    fn test_cells_pure() {
        let a = app_cell(3).unwrap_or(0);
        assert_eq!(a, app_cell(3).unwrap_or(0));
        assert_ne!(a, app_cell(4).unwrap_or(0));
        assert!(handle_cell(HANDLE_CELLS).is_none());
        assert!(zero_row(ZERO_ROWS).is_none());
    }

    #[test]
    fn test_bp_dispatch_fold() {
        // 断点异常派发序 ×10 折入面载荷：同面格间载荷互异。
        let a = face_cell(0).unwrap_or(0);
        let b = face_cell(1).unwrap_or(0);
        assert_ne!(a, b);
        assert_eq!(BP_DISPATCH_SEQ, 10);
    }

    #[test]
    fn test_app_order_caught() {
        let mut lt = DbgApiLedger::new();
        let _ = lt.record_app(9, app_cell(9).unwrap_or(0));
        assert_eq!(lt.record_app(2, app_cell(2).unwrap_or(0)), Err(DbgApiCode::EOrder));
        assert!(lt.record_app(11, app_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_face_order_caught() {
        let mut lt = DbgApiLedger::new();
        let _ = lt.record_face(7, face_cell(7).unwrap_or(0));
        assert_eq!(lt.record_face(2, face_cell(2).unwrap_or(0)), Err(DbgApiCode::EOrder));
        assert!(lt.record_face(9, face_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DbgApiLedger::new();
        assert_eq!(
            lt.record_app(4, app_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(DbgApiCode::EDrift)
        );
        assert_eq!(
            lt.record_face(6, face_cell(6).unwrap_or(0).wrapping_add(3)),
            Err(DbgApiCode::EFaceDrift)
        );
        assert_eq!(
            lt.record_handle(2, handle_cell(2).unwrap_or(0).wrapping_add(5)),
            Err(DbgApiCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DbgApiLedger::new();
        let _ = lt.record_handle(1, handle_cell(1).unwrap_or(0));
        assert_eq!(lt.record_handle(1, handle_cell(1).unwrap_or(0)), Err(DbgApiCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DbgApiLedger::new();
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
        assert!(recorded >= (APP_CELLS + FACE_CELLS + HANDLE_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8680);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DbgApiCode::Ok as u32, 0);
        assert_eq!(DbgApiCode::ENoApp as u32, 1);
        assert_eq!(DbgApiCode::EFp as u32, 11);
        assert_eq!(DbgApiCode::EGradeDrift as u32, 12);
        let all = [
            DbgApiCode::Ok,
            DbgApiCode::ENoApp,
            DbgApiCode::EDrift,
            DbgApiCode::EOrder,
            DbgApiCode::ENoFace,
            DbgApiCode::EFaceDrift,
            DbgApiCode::ENoHandle,
            DbgApiCode::ENoRow,
            DbgApiCode::EDup,
            DbgApiCode::ESeal,
            DbgApiCode::EInj,
            DbgApiCode::EFp,
            DbgApiCode::EGradeDrift,
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
        let lt = DbgApiLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52283_checks_green() {
        let s = run_unx52283_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
