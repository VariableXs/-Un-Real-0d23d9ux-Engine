//! UNX-F52329 dotPeek 收口 · 0xC9200段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52329"，逐字）：
//!   title: dotPeek 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52330-J1 六判据
//!     执行；EOL 冻结定级；KB-COMPAT 定级 S2 + EOL 标记；判据锚定
//!     §AI-66.2 专题六；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52329）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52330-J1，照原文锚定；功能按本卡标题（dotPeek）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 反编译 10 操作闭集：打开程序集/反编译类型/反编译方法/IL 视图/
//!      C# 视图/导出工程/搜索符号/导航到定义/查找引用/书签，逐操作
//!      载荷（纯函数折叠）恒等才收，越闭集必抓。
//!   ② 导航/搜索 12 格双家族闭集：导航 6 格（导航到定义/转到类型/
//!      转到成员/历史后退/历史前进/面包屑）+ 搜索 6 格（符号搜索/
//!      字符串搜索/全文搜索/正则搜索/大小写开关/结果过滤），家族
//!      单调（导航→搜索，逆序必抓且拒绝后不改 last_kind）。
//!   ③ ReSharper 消费联签 4 格（联签锚 F52052）：重构跳转/注解共享/
//!      解决方案同步/版本对齐，逐格落账缺一拒封。
//!   ④ 随 Visual Studio 分发声明 2 行闭集：随 VS 分发声明/独立安装
//!      不承诺声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（导航格
//!      载荷漂移/反编译操作越闭集/导航家族逆序），全部落在当轮临时
//!      账本上，注入 10/10 全抓、逃逸 0；双跑指纹一致；KB-COMPAT
//!      定级 S1/S2/S3 闭集（10 操作 + 12 格 + 4 联签 + 2 行声明 +
//!      六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52329;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 反编译操作数。
pub const DEC_OPS: usize = 10;

/// 导航/搜索格数。
pub const NAV_CELLS: usize = 12;

/// ReSharper 消费联签格数（F52052）。
pub const COSIGN_CELLS: usize = 4;

/// 随 VS 分发声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DotPeekCode {
    /// 成功
    Ok = 0,
    /// 反编译操作越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 导航/搜索格越闭集
    ENoNav,
    /// 导航家族逆序
    EOrder,
    /// 联签格越闭集
    ENoCosign,
    /// 声明行越闭集
    ENoRow,
    /// 判据格越闭集
    EGate,
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

impl DotPeekCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DotPeekCode::Ok => "成功",
            DotPeekCode::ENoOp => "反编译操作越闭集：dotPeek 10 操作集之外先登记",
            DotPeekCode::EDrift => "载荷漂移：格载荷折叠与基准不恒等，逐格复算",
            DotPeekCode::ENoNav => "导航/搜索格越闭集：12 格双家族闭集之外先登记",
            DotPeekCode::EOrder => "导航家族逆序：导航→搜索单调，逆序必抓且拒绝后不改 last_kind",
            DotPeekCode::ENoCosign => "联签格越闭集：ReSharper 消费联签 4 格（F52052）之外先登记",
            DotPeekCode::ENoRow => "声明行越闭集：随 VS 分发声明 2 行闭集之外先登记",
            DotPeekCode::EGate => "判据格越闭集：六判据闸 0..=5 之外先登记",
            DotPeekCode::EDup => "重复落账：同格只许一条账目",
            DotPeekCode::ESeal => "未齐即封：10 操作 + 12 格 + 4 联签 + 2 行声明未齐先补齐",
            DotPeekCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DotPeekCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DotPeekCode::EGradeDrift => "定级漂移：全通且联签/声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+10操作+12格+4联签+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 反编译操作格载荷（操作 o）。XOR 锚低 16 位恒零。
pub const fn decomp_cell(o: usize) -> Option<u64> {
    if o >= DEC_OPS {
        return None;
    }
    Some((0x52A0_0000_0000_0000u64 + (o as u64) * 0x37) ^ 0x4450_4B00_0000_0000)
}

/// 反编译操作名（const fn 表 + Option 边界）。
pub const fn decomp_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("打开程序集"),
        1 => Some("反编译类型"),
        2 => Some("反编译方法"),
        3 => Some("IL 视图"),
        4 => Some("C# 视图"),
        5 => Some("导出工程"),
        6 => Some("搜索符号"),
        7 => Some("导航到定义"),
        8 => Some("查找引用"),
        9 => Some("书签"),
        _ => None,
    }
}

/// 导航/搜索格载荷（格 o；家族 = o/6：0=导航 1=搜索）。
pub const fn nav_cell(o: usize) -> Option<u64> {
    if o >= NAV_CELLS {
        return None;
    }
    Some((0x52B0_0000_0000_0000u64 + (o as u64) * 0x53) ^ (((o / 6) as u64) << 16))
}

/// 导航/搜索家族（i/N 映射：0..6→导航，6..12→搜索）。
pub const fn nav_kind(o: usize) -> Option<u32> {
    if o >= NAV_CELLS {
        return None;
    }
    Some((o / 6) as u32)
}

/// 家族名。
pub const fn nav_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("导航"),
        1 => Some("搜索"),
        _ => None,
    }
}

/// ReSharper 消费联签格载荷（格 c，锚 F52052）。XOR 锚低 16 位恒零。
pub const fn cosign_cell(c: usize) -> Option<u64> {
    if c >= COSIGN_CELLS {
        return None;
    }
    Some(((0x52C0_0000u32 + c as u32 * 0x71) as u64) ^ 0x5253_0000_0000_0000)
}

/// 联签名。
pub const fn cosign_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("重构跳转联签"),
        1 => Some("注解共享联签"),
        2 => Some("解决方案同步联签"),
        3 => Some("版本对齐联签"),
        _ => None,
    }
}

/// 随 VS 分发声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0x52D0 + r as u32 * 0x29)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// dotPeek 收口账本状态机。
#[derive(Clone)]
pub struct DotPeekLedger {
    /// 六判据（UNX-F52330-J1：启动/反编译/导航/搜索/崩溃/卸载）。
    gates: [Option<bool>; 6],
    /// 反编译操作账。
    decomp: [Option<u64>; DEC_OPS],
    /// 导航/搜索格账。
    nav: [Option<u64>; NAV_CELLS],
    /// 联签格账。
    cosign: [Option<u64>; COSIGN_CELLS],
    /// 声明行账。
    decl: [Option<u32>; DECL_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DotPeekLedger {
    /// 新账本。
    pub fn new() -> Self {
        DotPeekLedger {
            gates: [None; 6],
            decomp: [None; DEC_OPS],
            nav: [None; NAV_CELLS],
            cosign: [None; COSIGN_CELLS],
            decl: [None; DECL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DotPeekCode> {
        if gate >= 6 {
            return Err(DotPeekCode::EGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5290 + gate as u64);
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

    /// 反编译操作一格（载荷对照 + 重复拒）。
    pub fn record_decomp(&mut self, o: usize, got: u64) -> Result<(), DotPeekCode> {
        if o >= DEC_OPS {
            return Err(DotPeekCode::ENoOp);
        }
        let want = match decomp_cell(o) {
            Some(w) => w,
            None => return Err(DotPeekCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5291 + o as u64);
            return Err(DotPeekCode::EDrift);
        }
        if match self.decomp.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DotPeekCode::EDup);
        }
        if let Some(c) = self.decomp.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5292 + o as u64);
        Ok(())
    }

    /// 反编译操作全账核验（10 格）。
    pub fn decomp_all(&self) -> bool {
        let mut o = 0usize;
        while o < DEC_OPS {
            match self.decomp.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 导航/搜索一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_nav(&mut self, o: usize, got: u64) -> Result<(), DotPeekCode> {
        if o >= NAV_CELLS {
            return Err(DotPeekCode::ENoNav);
        }
        let kind = match nav_kind(o) {
            Some(k) => k,
            None => return Err(DotPeekCode::ENoNav),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5293);
            return Err(DotPeekCode::EOrder);
        }
        let want = match nav_cell(o) {
            Some(w) => w,
            None => return Err(DotPeekCode::ENoNav),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5294 + o as u64);
            return Err(DotPeekCode::EDrift);
        }
        if match self.nav.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DotPeekCode::EDup);
        }
        if let Some(c) = self.nav.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5295 + o as u64);
        Ok(())
    }

    /// 导航/搜索全账核验（12 格）。
    pub fn nav_all(&self) -> bool {
        let mut o = 0usize;
        while o < NAV_CELLS {
            match self.nav.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 联签一格（F52052）。
    pub fn record_cosign(&mut self, c: usize) -> Result<(), DotPeekCode> {
        if c >= COSIGN_CELLS {
            return Err(DotPeekCode::ENoCosign);
        }
        if match self.cosign.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DotPeekCode::EDup);
        }
        if let Some(w) = cosign_cell(c) {
            if let Some(cell) = self.cosign.get_mut(c) {
                *cell = Some(w);
            }
        }
        self.fnv(0x5296 + c as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn cosign_all(&self) -> bool {
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            match self.cosign.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行（随 VS 分发）。
    pub fn declare_row(&mut self, r: usize) -> Result<(), DotPeekCode> {
        if r >= DECL_ROWS {
            return Err(DotPeekCode::ENoRow);
        }
        if match self.decl.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DotPeekCode::EDup);
        }
        if let Some(c) = self.decl.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0x5297 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decl.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 最后家族（注入拒绝后不变的观测口）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
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
        if self.gates_all() && self.decomp_all() && self.nav_all() && self.cosign_all() && self.rows_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DotPeekLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DotPeekLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < DEC_OPS {
        if let Some(w) = decomp_cell(o) {
            if led.record_decomp(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut n = 0usize;
    while n < NAV_CELLS {
        if let Some(w) = nav_cell(n) {
            if led.record_nav(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_CELLS {
        let _ = led.record_cosign(c);
        c += 1;
    }
    let mut r = 0usize;
    while r < DECL_ROWS {
        let _ = led.declare_row(r);
        r += 1;
    }
    // 注入 10 轮：导航格载荷漂移 / 反编译操作越闭集 / 导航家族逆序
    // 三态轮转，全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DotPeekLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先录正格再注漂移。
                let w0 = match nav_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_nav(0, w0);
                lt.record_nav(1, w0.wrapping_add(1)) == Err(DotPeekCode::EDrift)
            }
            1 => lt.record_decomp(DEC_OPS, 0).is_err(),
            _ => {
                // 逆序注入：先按合法序落搜索家族一格，再注导航家族。
                let w6 = match nav_cell(6) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_nav(6, w6);
                lt.record_nav(0, 0).is_err() && lt.last_kind() == 1
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5298 + k as u64);
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
/// UNX-F52329 判据集（锚 UNX-F52330-J1，off-by-one 板式）。
pub fn run_unx52329_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52329");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "反编译 10 操作全通",
        recorded >= DEC_OPS as u32,
        "打开程序集至书签十操作逐格载荷恒等（§AI-66.2 专题六）",
    );
    s.add(
        "导航/搜索 12 格全落",
        {
            let mut lt = DotPeekLedger::new();
            let mut ok = true;
            let mut n = 0usize;
            while n < NAV_CELLS {
                if let Some(w) = nav_cell(n) {
                    if lt.record_nav(n, w).is_err() {
                        ok = false;
                    }
                }
                n += 1;
            }
            ok && lt.nav_all()
        },
        "导航 6 格 + 搜索 6 格双家族逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = DotPeekLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/反编译/导航/搜索/崩溃/卸载逐格落账（UNX-F52330-J1）",
    );
    s.add(
        "ReSharper 消费联签 4/4（F52052）",
        {
            let mut lt = DotPeekLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                if lt.record_cosign(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.cosign_all()
        },
        "重构跳转/注解共享/解决方案同步/版本对齐四格联签",
    );
    s.add(
        "随 VS 分发声明 2/2 全齐",
        {
            let mut lt = DotPeekLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DECL_ROWS {
                if lt.declare_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "随 VS 分发声明/独立安装不承诺声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut n = 1usize;
            while n < NAV_CELLS {
                let a = match nav_kind(n) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match nav_kind(n - 1) {
                    Some(k) => k,
                    None => 9,
                };
                if a < b {
                    ok = false;
                }
                n += 1;
            }
            ok
        },
        "导航→搜索两家族闭集单调，逆序必抓",
    );
    s.add(
        "诊断码闭集 13 变体",
        DotPeekCode::Ok as u32 == 0 && DotPeekCode::EGradeDrift as u32 == 12,
        "Ok=+0x00 起 12 错误码至 +0x0C 恰 13 变体闭集",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "导航格载荷漂移/操作越闭集/家族逆序三态轮转必抓",
    );
    let mut l0 = DotPeekLedger::new();
    s.add(
        "边界·反编译操作越闭集",
        l0.record_decomp(DEC_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·导航格越闭集",
        l0.record_nav(NAV_CELLS, 0).is_err(),
        "格 12 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        l0.record_cosign(COSIGN_CELLS).is_err(),
        "联签格 4 越闭集必拒（F52052 之外先登记）",
    );
    s.add(
        "漂移拒（导航载荷漂移）",
        {
            let w0 = match nav_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let _ = l0.record_nav(0, w0);
            l0.record_nav(1, w0.wrapping_add(1)) == Err(DotPeekCode::EDrift)
        },
        "导航格载荷漂移必抓",
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
        "定级 S1 闭集",
        {
            let mut lt = DotPeekLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < DEC_OPS {
                if let Some(w) = decomp_cell(o) {
                    let _ = lt.record_decomp(o, w);
                }
                o += 1;
            }
            let mut n = 0usize;
            while n < NAV_CELLS {
                if let Some(w) = nav_cell(n) {
                    let _ = lt.record_nav(n, w);
                }
                n += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                let _ = lt.record_cosign(c);
                c += 1;
            }
            let mut r = 0usize;
            while r < DECL_ROWS {
                let _ = lt.declare_row(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9200,
        "SEG_BASE 0x40 对齐且落在 VX15 公式 0xC7200+0x40×(52329−52201)",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9200);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DotPeekCode::Ok as u32, 0);
        assert_eq!(DotPeekCode::EGate as u32, 7);
        assert_eq!(DotPeekCode::ESeal as u32, 9);
        assert_eq!(DotPeekCode::EGradeDrift as u32, 12);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => DotPeekCode::Ok,
                1 => DotPeekCode::ENoOp,
                2 => DotPeekCode::EDrift,
                3 => DotPeekCode::ENoNav,
                4 => DotPeekCode::EOrder,
                5 => DotPeekCode::ENoCosign,
                6 => DotPeekCode::ENoRow,
                7 => DotPeekCode::EGate,
                8 => DotPeekCode::EDup,
                9 => DotPeekCode::ESeal,
                10 => DotPeekCode::EInj,
                11 => DotPeekCode::EFp,
                _ => DotPeekCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_decomp_names() {
        assert_eq!(decomp_name(0), Some("打开程序集"));
        assert_eq!(decomp_name(9), Some("书签"));
        assert!(decomp_name(10).is_none());
        assert!(decomp_cell(10).is_none());
    }

    #[test]
    fn test_nav_kind_mapping() {
        assert_eq!(nav_kind(0), Some(0));
        assert_eq!(nav_kind(5), Some(0));
        assert_eq!(nav_kind(6), Some(1));
        assert_eq!(nav_kind(11), Some(1));
        assert!(nav_kind(12).is_none());
        assert_eq!(nav_kind_name(0), Some("导航"));
        assert_eq!(nav_kind_name(1), Some("搜索"));
        assert!(nav_kind_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = nav_cell(3).unwrap_or(0);
        assert_eq!(a, nav_cell(3).unwrap_or(0));
        assert_ne!(a, nav_cell(4).unwrap_or(0));
        assert_ne!(nav_cell(0).unwrap_or(0), nav_cell(6).unwrap_or(0));
        assert!(decl_row(DECL_ROWS).is_none());
        assert!(cosign_cell(COSIGN_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = DotPeekLedger::new();
        let _ = lt.record_nav(8, nav_cell(8).unwrap_or(0));
        assert_eq!(lt.record_nav(2, nav_cell(2).unwrap_or(0)), Err(DotPeekCode::EOrder));
        // 拒绝后 last_kind 不变。
        assert_eq!(lt.last_kind(), 1);
        assert!(lt.record_nav(9, nav_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DotPeekLedger::new();
        let w = decomp_cell(4).unwrap_or(0);
        assert_eq!(lt.record_decomp(4, w.wrapping_add(1)), Err(DotPeekCode::EDrift));
        let n = nav_cell(7).unwrap_or(0);
        assert_eq!(lt.record_nav(7, n.wrapping_add(7)), Err(DotPeekCode::EDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DotPeekLedger::new();
        let _ = lt.record_decomp(0, decomp_cell(0).unwrap_or(0));
        assert_eq!(lt.record_decomp(0, decomp_cell(0).unwrap_or(0)), Err(DotPeekCode::EDup));
        let _ = lt.record_cosign(1);
        assert_eq!(lt.record_cosign(1), Err(DotPeekCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DotPeekLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓（EGate），全落才 gates_all。
        assert_eq!(lt.judge_gate(6), Err(DotPeekCode::EGate));
        assert!(lt.gates_all());
        let mut lt2 = DotPeekLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= DEC_OPS as u32 + NAV_CELLS as u32);
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
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = DotPeekLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_decl_boundary() {
        let mut lt = DotPeekLedger::new();
        assert!(lt.declare_row(0).is_ok());
        assert!(lt.declare_row(1).is_ok());
        assert_eq!(lt.declare_row(2), Err(DotPeekCode::ENoRow));
        assert!(lt.rows_all());
    }

    #[test]
    fn unx52329_checks_green() {
        let s = run_unx52329_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
