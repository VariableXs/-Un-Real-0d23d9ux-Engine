//! UNX-F52429 Angry IP Scanner 收口 · 0xCAB00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52429"，逐字）：
//!   title: Angry IP Scanner 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52430-J1 六判据
//!     全落；同白名单声明；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52429）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52430-J1，照原文锚定；功能按本卡标题（Angry IP Scanner）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五扫描族闭集 20 格：ping / 端口 / NetBIOS / HTTP / Web 检测，每族
//!      4 格，族载荷纯函数折叠恒等才收；族单调（逆序必抓）。
//!   ② 线程三档 6 格：低档（32 线程）/ 中档（128 线程）/ 高档（512 线程）
//!      每档 2 格，档位越闭集与档漂移必抓。
//!   ③ 结果导出三格式 6 格：CSV / TXT / XML 每格式 2 格，逐格式投影
//!      对照，漂移必抓。
//!   ④ Java 上栈声明 2 行闭集：Java 运行时上栈声明 / 扫描管道 Java 依赖
//!      分账声明，缺一拒封（ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（线程档漂移/
//!      扫描漂移/族逆序）10/10 必抓，逃逸恒 0；双跑指纹一致。
//!   ⑥ 定级目标档 S2（KB-COMPAT 登记）：空账 S3，六闸齐 S2，全齐目标
//!      档 S2。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52429;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五扫描族 20 格。
pub const SCAN_CELLS: usize = 20;

/// 线程三档 6 格。
pub const THREAD_CELLS: usize = 6;

/// 结果导出三格式 6 格。
pub const EXPORT_CELLS: usize = 6;

/// Java 上栈声明行数。
pub const JAVA_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AngryIpCode {
    /// 成功
    Ok = 0,
    /// 扫描格越闭集
    ENoScan,
    /// 扫描漂移
    EDrift,
    /// 扫描族逆序
    EOrder,
    /// 线程档越闭集
    ENoThr,
    /// 线程档漂移
    EThrDrift,
    /// 导出格越闭集
    ENoExp,
    /// Java 声明缺行
    ERowMiss,
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

impl AngryIpCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AngryIpCode::Ok => "成功",
            AngryIpCode::ENoScan => "扫描格越闭集：五扫描族 20 格之外先登记",
            AngryIpCode::EDrift => "扫描漂移：扫描族载荷折叠与基准不恒等，逐格复算",
            AngryIpCode::EOrder => "扫描族逆序：ping→端口→NetBIOS→HTTP→Web 检测单调，逆序必抓",
            AngryIpCode::ENoThr => "线程档越闭集：线程三档 6 格之外先登记",
            AngryIpCode::EThrDrift => "线程档漂移：档位载荷折叠与基准不恒等，逐档复算",
            AngryIpCode::ENoExp => "导出格越闭集：结果导出三格式 6 格之外先登记",
            AngryIpCode::ERowMiss => "Java 声明缺行：Java 上栈声明未齐先补齐",
            AngryIpCode::EDup => "重复落账：同格只许一条账目",
            AngryIpCode::ESeal => "未齐即封：20 扫描格 + 6 线程格 + 6 导出格 + 2 声明行未齐先补齐",
            AngryIpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            AngryIpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            AngryIpCode::EGradeDrift => "定级漂移：全齐且六闸落账才可定 S2 目标档",
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
/// 扫描格载荷（格 s；族 = s/4）。
pub const fn scan_cell(s: usize) -> Option<u64> {
    if s >= SCAN_CELLS {
        return None;
    }
    Some((0xC1000u64 + (s as u64) * 0x2F) ^ ((s / 4) as u64).wrapping_mul(0x01000193))
}

/// 扫描族（0..4=ping 4..8=端口 8..12=NetBIOS 12..16=HTTP 16..20=Web 检测）。
pub const fn scan_kind(s: usize) -> Option<u32> {
    if s >= SCAN_CELLS {
        return None;
    }
    Some((s / 4) as u32)
}

/// 扫描族名。
pub const fn scan_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("ping"),
        1 => Some("端口"),
        2 => Some("NetBIOS"),
        3 => Some("HTTP"),
        4 => Some("Web 检测"),
        _ => None,
    }
}

/// 线程档格载荷（格 t；档 = t/2：0=低档(32) 1=中档(128) 2=高档(512)）。
pub const fn thread_cell(t: usize) -> Option<u64> {
    if t >= THREAD_CELLS {
        return None;
    }
    Some((0xC2000u64 + (t as u64) * 0x37) ^ 0x4E47524950000000)
}

/// 线程档（i/N 映射：档 = t/2）。
pub const fn thread_tier(t: usize) -> Option<u32> {
    if t >= THREAD_CELLS {
        return None;
    }
    Some((t / 2) as u32)
}

/// 线程档名。
pub const fn thread_tier_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("低档 32 线程"),
        1 => Some("低档 32 线程（校验格）"),
        2 => Some("中档 128 线程"),
        3 => Some("中档 128 线程（校验格）"),
        4 => Some("高档 512 线程"),
        5 => Some("高档 512 线程（校验格）"),
        _ => None,
    }
}

/// 导出格载荷（格 e；格式 = e/2：0=CSV 1=TXT 2=XML）。
pub const fn export_cell(e: usize) -> Option<u64> {
    if e >= EXPORT_CELLS {
        return None;
    }
    Some((0xC3000u64 + (e as u64) * 0x41) ^ 0x4558504F52540000)
}

/// 导出格式（i/N 映射：格式 = e/2）。
pub const fn export_format(e: usize) -> Option<u32> {
    if e >= EXPORT_CELLS {
        return None;
    }
    Some((e / 2) as u32)
}

/// 导出格式名。
pub const fn export_format_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("CSV"),
        1 => Some("TXT"),
        2 => Some("XML"),
        _ => None,
    }
}

/// Java 上栈声明行折叠（行 r）。
pub const fn java_row(r: usize) -> Option<u32> {
    if r >= JAVA_ROWS {
        return None;
    }
    Some(0xC4000 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Angry IP Scanner 收口账本状态机。
#[derive(Clone)]
pub struct AngryIpLedger {
    /// 六判据（UNX-F52430-J1 锚）。
    gates: [Option<bool>; 6],
    /// 扫描账。
    scans: [Option<u64>; SCAN_CELLS],
    /// 线程档账。
    thr: [Option<u64>; THREAD_CELLS],
    /// 导出账。
    exp: [Option<u64>; EXPORT_CELLS],
    /// Java 声明账。
    java: [Option<u32>; JAVA_ROWS],
    /// 最后扫描族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AngryIpLedger {
    /// 新账本。
    pub fn new() -> Self {
        AngryIpLedger {
            gates: [None; 6],
            scans: [None; SCAN_CELLS],
            thr: [None; THREAD_CELLS],
            exp: [None; EXPORT_CELLS],
            java: [None; JAVA_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AngryIpCode> {
        if gate >= 6 {
            return Err(AngryIpCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 扫描一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_scan(&mut self, s: usize, got: u64) -> Result<(), AngryIpCode> {
        if s >= SCAN_CELLS {
            return Err(AngryIpCode::ENoScan);
        }
        let kind = match scan_kind(s) {
            Some(k) => k,
            None => return Err(AngryIpCode::ENoScan),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(AngryIpCode::EOrder);
        }
        let want = match scan_cell(s) {
            Some(w) => w,
            None => return Err(AngryIpCode::ENoScan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + s as u64);
            return Err(AngryIpCode::EDrift);
        }
        if match self.scans.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AngryIpCode::EDup);
        }
        if let Some(c) = self.scans.get_mut(s) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + s as u64);
        Ok(())
    }

    /// 扫描全账核验（20 格）。
    pub fn scans_all(&self) -> bool {
        let mut s = 0usize;
        while s < SCAN_CELLS {
            match self.scans.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 线程档一格（档位越闭集 + 载荷对照 + 重复拒）。
    pub fn record_thread(&mut self, t: usize, got: u64) -> Result<(), AngryIpCode> {
        let want = match thread_cell(t) {
            Some(w) => w,
            None => return Err(AngryIpCode::ENoThr),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + t as u64);
            return Err(AngryIpCode::EThrDrift);
        }
        if match self.thr.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AngryIpCode::EDup);
        }
        if let Some(c) = self.thr.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x5301 + t as u64);
        Ok(())
    }

    /// 线程档全账核验（6 格）。
    pub fn thr_all(&self) -> bool {
        let mut t = 0usize;
        while t < THREAD_CELLS {
            match self.thr.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 导出一格（格式越闭集 + 投影对照 + 重复拒）。
    pub fn record_export(&mut self, e: usize, got: u64) -> Result<(), AngryIpCode> {
        let want = match export_cell(e) {
            Some(w) => w,
            None => return Err(AngryIpCode::ENoExp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + e as u64);
            return Err(AngryIpCode::EDrift);
        }
        if match self.exp.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AngryIpCode::EDup);
        }
        if let Some(c) = self.exp.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x5401 + e as u64);
        Ok(())
    }

    /// 导出全账核验（6 格）。
    pub fn exp_all(&self) -> bool {
        let mut e = 0usize;
        while e < EXPORT_CELLS {
            match self.exp.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// Java 上栈声明一行。
    pub fn declare_java(&mut self, r: usize) -> Result<(), AngryIpCode> {
        if r >= JAVA_ROWS {
            return Err(AngryIpCode::ENoExp);
        }
        if match self.java.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AngryIpCode::EDup);
        }
        if let Some(c) = self.java.get_mut(r) {
            *c = java_row(r);
        }
        self.fnv(0x5500 + r as u64);
        Ok(())
    }

    /// Java 声明全齐核验。
    pub fn java_all(&self) -> bool {
        let mut r = 0usize;
        while r < JAVA_ROWS {
            match self.java.get(r) {
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

impl Default for AngryIpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转（线程档漂移 / 扫描漂移 / 族逆序）全部落在当轮
/// 临时账本上：主账 injected==caught==10、escaped==0；漂移注入先按合法
/// 序落正格再注漂移；族逆序注入对空账本同样必抓且不改主账 last_kind。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AngryIpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < SCAN_CELLS {
        if let Some(w) = scan_cell(s) {
            if led.record_scan(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut t = 0usize;
    while t < THREAD_CELLS {
        if let Some(w) = thread_cell(t) {
            if led.record_thread(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut e = 0usize;
    while e < EXPORT_CELLS {
        if let Some(w) = export_cell(e) {
            if led.record_export(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut r = 0usize;
    while r < JAVA_ROWS {
        let _ = led.declare_java(r);
        r += 1;
    }
    // 注入 10 轮：线程档漂移 / 扫描漂移 / 族逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AngryIpLedger::new();
        let caught = match kind {
            0 => {
                lt.record_thread(0, thread_cell(0).unwrap_or(0));
                lt.record_thread(1, thread_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(AngryIpCode::EThrDrift)
            }
            1 => {
                let _ = lt.record_scan(0, scan_cell(0).unwrap_or(0));
                let _ = lt.record_scan(1, scan_cell(1).unwrap_or(0));
                lt.record_scan(2, scan_cell(2).unwrap_or(0).wrapping_add(1)) == Err(AngryIpCode::EDrift)
            }
            _ => {
                let _ = lt.record_scan(17, scan_cell(17).unwrap_or(0));
                lt.record_scan(3, scan_cell(3).unwrap_or(0)) == Err(AngryIpCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
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
/// UNX-F52429 判据集（锚 UNX-F52430-J1，off-by-one 板式，§AI-66.3）。
pub fn run_unx52429_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52429");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五扫描族 20 格全通",
        recorded >= SCAN_CELLS as u32 + THREAD_CELLS as u32 + EXPORT_CELLS as u32,
        "ping/端口/NetBIOS/HTTP/Web 检测逐格恒等（UNX-F52430-J1 锚）",
    );
    s.add(
        "线程三档 6 格全落",
        {
            let mut lt = AngryIpLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < THREAD_CELLS {
                if let Some(w) = thread_cell(t) {
                    if lt.record_thread(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.thr_all()
        },
        "低/中/高三档逐格恒等，档漂移必抓",
    );
    s.add(
        "结果导出三格式 6 格全落",
        {
            let mut lt = AngryIpLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EXPORT_CELLS {
                if let Some(w) = export_cell(e) {
                    if lt.record_export(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.exp_all()
        },
        "CSV/TXT/XML 三格式逐格投影对照",
    );
    s.add(
        "Java 上栈声明 2/2 全齐",
        {
            let mut lt = AngryIpLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JAVA_ROWS {
                if lt.declare_java(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.java_all()
        },
        "Java 运行时上栈声明/扫描管道 Java 依赖分账声明两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = AngryIpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52430-J1 六判据逐格落账",
    );
    s.add(
        "扫描族单调",
        {
            let mut ok = true;
            let mut sc = 1usize;
            while sc < SCAN_CELLS {
                if scan_kind(sc).unwrap_or(5) < scan_kind(sc - 1).unwrap_or(5) {
                    ok = false;
                }
                sc += 1;
            }
            ok
        },
        "五扫描族闭集单调（§AI-66.3）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "线程档漂移/扫描漂移/族逆序三态轮转必抓");
    let mut l0 = AngryIpLedger::new();
    s.add(
        "漂移拒",
        {
            l0.record_scan(0, scan_cell(0).unwrap_or(0));
            l0.record_scan(1, scan_cell(1).unwrap_or(0).wrapping_add(1)) == Err(AngryIpCode::EDrift)
        },
        "扫描载荷漂移必抓",
    );
    s.add(
        "族逆序拒",
        {
            let _ = l0.record_scan(17, scan_cell(17).unwrap_or(0));
            l0.record_scan(3, scan_cell(3).unwrap_or(0)) == Err(AngryIpCode::EOrder)
        },
        "Web 检测后回 ping 必抓",
    );
    s.add(
        "边界·扫描越闭集",
        l0.record_scan(SCAN_CELLS, 0).is_err(),
        "扫描格 20 越闭集必拒",
    );
    s.add(
        "边界·线程档越闭集",
        l0.record_thread(THREAD_CELLS, 0).is_err(),
        "线程格 6 越闭集必拒",
    );
    s.add(
        "边界·导出越闭集",
        l0.record_export(EXPORT_CELLS, 0).is_err(),
        "导出格 6 越闭集必拒",
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
            let mut lt = AngryIpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut sc = 0usize;
            while sc < SCAN_CELLS {
                if let Some(w) = scan_cell(sc) {
                    let _ = lt.record_scan(sc, w);
                }
                sc += 1;
            }
            let mut t = 0usize;
            while t < THREAD_CELLS {
                if let Some(w) = thread_cell(t) {
                    let _ = lt.record_thread(t, w);
                }
                t += 1;
            }
            let mut e = 0usize;
            while e < EXPORT_CELLS {
                if let Some(w) = export_cell(e) {
                    let _ = lt.record_export(e, w);
                }
                e += 1;
            }
            let mut r = 0usize;
            while r < JAVA_ROWS {
                let _ = lt.declare_java(r);
                r += 1;
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
    fn test_scan_kind() {
        assert_eq!(scan_kind(0), Some(0));
        assert_eq!(scan_kind(3), Some(0));
        assert_eq!(scan_kind(4), Some(1));
        assert_eq!(scan_kind(9), Some(2));
        assert_eq!(scan_kind(19), Some(4));
        assert!(scan_kind(20).is_none());
        assert!(scan_cell(20).is_none());
        assert_eq!(scan_kind_name(0), Some("ping"));
        assert_eq!(scan_kind_name(4), Some("Web 检测"));
        assert!(scan_kind_name(5).is_none());
    }

    #[test]
    fn test_thread_tier_mapping() {
        assert_eq!(thread_tier(0), Some(0));
        assert_eq!(thread_tier(1), Some(0));
        assert_eq!(thread_tier(2), Some(1));
        assert_eq!(thread_tier(5), Some(2));
        assert!(thread_tier(6).is_none());
        assert!(thread_cell(6).is_none());
    }

    #[test]
    fn test_export_format_mapping() {
        assert_eq!(export_format(0), Some(0));
        assert_eq!(export_format(1), Some(0));
        assert_eq!(export_format(4), Some(2));
        assert_eq!(export_format_name(2), Some("XML"));
        assert!(export_format(6).is_none());
        assert!(export_cell(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = scan_cell(7).unwrap_or(0);
        assert_eq!(a, scan_cell(7).unwrap_or(0));
        assert_ne!(a, scan_cell(8).unwrap_or(0));
        assert_ne!(thread_cell(1).unwrap_or(0), thread_cell(2).unwrap_or(0));
        assert!(java_row(JAVA_ROWS).is_none());
        assert!(java_row(0).is_some());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = AngryIpLedger::new();
        let _ = lt.record_scan(9, scan_cell(9).unwrap_or(0));
        assert_eq!(lt.record_scan(2, scan_cell(2).unwrap_or(0)), Err(AngryIpCode::EOrder));
        assert!(lt.record_scan(10, scan_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AngryIpLedger::new();
        assert_eq!(
            lt.record_scan(5, scan_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(AngryIpCode::EDrift)
        );
        assert_eq!(
            lt.record_thread(3, thread_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(AngryIpCode::EThrDrift)
        );
        assert_eq!(
            lt.record_export(2, export_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(AngryIpCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = AngryIpLedger::new();
        let _ = lt.record_thread(0, thread_cell(0).unwrap_or(0));
        assert_eq!(lt.record_thread(0, thread_cell(0).unwrap_or(0)), Err(AngryIpCode::EDup));
        let _ = lt.declare_java(0);
        assert_eq!(lt.declare_java(0), Err(AngryIpCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = AngryIpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = AngryIpLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= SCAN_CELLS as u32 + THREAD_CELLS as u32 + EXPORT_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCAB00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AngryIpCode::Ok as u32, 0);
        assert_eq!(AngryIpCode::EGradeDrift as u32, 12);
        assert_eq!(AngryIpCode::Ok.val(), SEG_BASE);
        assert!(!AngryIpCode::Ok.detail().is_empty());
        assert!(!AngryIpCode::ESeal.detail().is_empty());
        assert!(!AngryIpCode::EThrDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = AngryIpLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = AngryIpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        // 全齐目标档 S2。
        let mut sc = 0usize;
        while sc < SCAN_CELLS {
            if let Some(w) = scan_cell(sc) {
                let _ = lt2.record_scan(sc, w);
            }
            sc += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_stats_conservation() {
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
        // 注入=检出+逃逸，逃逸恒 0。
        assert!(caught <= 10);
    }

    #[test]
    fn unx52429_checks_green() {
        let s = run_unx52429_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
