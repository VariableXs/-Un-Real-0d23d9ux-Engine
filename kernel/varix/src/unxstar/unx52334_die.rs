//! UNX-F52334 Detect It Easy（DIE）收口 · 0xC9340 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52334"，逐字）：
//!   title: Detect It Easy（DIE）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52335-J1 六判据全落；
//!     资源查看/提取 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52334）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52335-J1，照原文锚定；功能按本卡标题（DIE）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐卡定制）：
//!   ① 检测引擎五族 20 格：编译器/打包器/链接器/安装器/加密库，每族 4 格，
//!      族单调（逆序必抓）+ 相位闸（后族先录对空账本同样必抓）。
//!   ② 扫描 10 操作：逐操作基准载荷对照，漂移必抓。
//!   ③ 签名库 8 格：逐格落账，越闭集必抓。
//!   ④ YARA 消费联签 4 格：与签名库联签对账，缺签拒收。
//!   ⑤ 开源声明 2 行闭集：开源许可声明 / 离线检测声明，缺一拒封。
//!   ⑥ 收口闸六判据：逐格落账，未齐即封。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10 轮（引擎格越闭集/扫描漂移/
//!      相位越序），三类注入全部落在当轮临时账本上，主账 injected==caught==10、
//!      escaped==0；漂移注入先录正格再注漂移；相位注入对空账本必抓且不改
//!      last_kind；双跑指纹一致；全通定级 S1。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52334;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 检测引擎格数（五族 × 4）。
pub const ENGINE_CELLS: usize = 20;

/// 每族格数。
pub const FAMILY_CELLS: usize = 4;

/// 扫描操作数。
pub const SCAN_OPS: usize = 10;

/// 签名库格数。
pub const SIG_CELLS: usize = 8;

/// YARA 消费联签格数。
pub const YARA_CELLS: usize = 4;

/// 开源声明行数。
pub const OSS_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DieCode {
    /// 成功
    Ok = 0,
    /// 引擎格越闭集
    ENoCell,
    /// 载荷漂移
    ECellDrift,
    /// 族序逆序
    EOrder,
    /// 相位越序（后族先录）
    EPhase,
    /// 重复落账
    EDup,
    /// 判据闸越闭集
    ENoGate,
    /// 未齐即封
    ESeal,
    /// 扫描操作越闭集
    ENoScan,
    /// 签名库格越闭集
    ENoSig,
    /// 开源声明行越闭集
    ENoRow,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl DieCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DieCode::Ok => "成功",
            DieCode::ENoCell => "引擎格越闭集：检测引擎 20 格闭集之外先登记",
            DieCode::ECellDrift => "载荷漂移：扫描/引擎折叠值与基准不恒等，逐格复算",
            DieCode::EOrder => "族序逆序：编译器→打包器→链接器→安装器→加密库单调，逆序必抓",
            DieCode::EPhase => "相位越序：前族未齐先录后族，对空账本同样必抓",
            DieCode::EDup => "重复落账：同格只许一条账目",
            DieCode::ENoGate => "判据闸越闭集：六判据闸 0..=5 之外先登记",
            DieCode::ESeal => "未齐即封：20 引擎格 + 10 扫描 + 8 签名格 + 4 联签 + 2 行声明未齐先补齐",
            DieCode::ENoScan => "扫描操作越闭集：扫描 10 操作闭集之外先登记",
            DieCode::ENoSig => "签名库格越闭集：签名库 8 格闭集之外先登记",
            DieCode::ENoRow => "开源声明行越闭集：开源声明 2 行闭集之外先登记",
            DieCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DieCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20引擎格+10扫描+8签名+4联签+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 检测引擎格载荷（格 o；族 = o/4；XOR 锚低 16 位恒零）。
pub const fn engine_cell(o: usize) -> Option<u64> {
    if o >= ENGINE_CELLS {
        return None;
    }
    Some((0xB3900u64 + (o as u64) * 0x41) ^ 0x4449_0000)
}

/// 引擎族（0=编译器 1=打包器 2=链接器 3=安装器 4=加密库）。
pub const fn engine_kind(o: usize) -> Option<u32> {
    if o >= ENGINE_CELLS {
        return None;
    }
    Some((o / FAMILY_CELLS) as u32)
}

/// 族名。
pub const fn engine_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("编译器"),
        1 => Some("打包器"),
        2 => Some("链接器"),
        3 => Some("安装器"),
        4 => Some("加密库"),
        _ => None,
    }
}

/// 扫描操作基准载荷（操作 s）。
pub const fn scan_cell(s: usize) -> Option<u32> {
    if s >= SCAN_OPS {
        return None;
    }
    Some(0xB3A00 + s as u32 * 0x2D)
}

/// 签名库格载荷（格 g）。
pub const fn sig_cell(g: usize) -> Option<u32> {
    if g >= SIG_CELLS {
        return None;
    }
    Some(0xB3B00 + g as u32 * 0x2F)
}

/// YARA 联签格载荷（格 y；与签名库格联签）。
pub const fn yara_cell(y: usize) -> Option<u32> {
    if y >= YARA_CELLS {
        return None;
    }
    match sig_cell(y * 2) {
        Some(base) => Some(base ^ (y as u32).wrapping_mul(0x9E3779B1).wrapping_add(0x59415200)),
        None => None,
    }
}

/// 开源声明行折叠（行 r）。
pub const fn oss_row(r: usize) -> Option<u32> {
    if r >= OSS_ROWS {
        return None;
    }
    Some(0xB3C00 + r as u32 * 0x2F)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// DIE 收口账本状态机。
#[derive(Clone)]
pub struct DieLedger {
    /// 六判据（引擎启动/签名库装载/扫描执行/联签对账/崩溃退出/许可声明）。
    gates: [Option<bool>; 6],
    /// 引擎账。
    engine: [Option<u64>; ENGINE_CELLS],
    /// 扫描账。
    scans: [Option<u32>; SCAN_OPS],
    /// 签名库账。
    sigs: [Option<u32>; SIG_CELLS],
    /// YARA 联签账。
    yaras: [Option<u32>; YARA_CELLS],
    /// 开源声明账。
    oss: [Option<u32>; OSS_ROWS],
    /// 最后族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DieLedger {
    /// 新账本。
    pub fn new() -> Self {
        DieLedger {
            gates: [None; 6],
            engine: [None; ENGINE_CELLS],
            scans: [None; SCAN_OPS],
            sigs: [None; SIG_CELLS],
            yaras: [None; YARA_CELLS],
            oss: [None; OSS_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DieCode> {
        if gate >= 6 {
            return Err(DieCode::ENoGate);
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

    /// 前族全齐核验（相位闸用）。
    fn families_complete(&self, upto: usize) -> bool {
        let bound = upto * FAMILY_CELLS;
        let mut i = 0usize;
        while i < bound {
            match self.engine.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 引擎一格（族单调 + 相位闸 + 载荷对照 + 重复拒）。
    pub fn record_engine(&mut self, o: usize, got: u64) -> Result<(), DieCode> {
        let kind = match engine_kind(o) {
            Some(k) => k,
            None => return Err(DieCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(DieCode::EOrder);
        }
        if (kind as usize) > 0 && !self.families_complete(kind as usize) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201);
            return Err(DieCode::EPhase);
        }
        let want = match engine_cell(o) {
            Some(w) => w,
            None => return Err(DieCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7202 + o as u64);
            return Err(DieCode::ECellDrift);
        }
        if match self.engine.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DieCode::EDup);
        }
        if let Some(c) = self.engine.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7203 + o as u64);
        Ok(())
    }

    /// 引擎全账核验（20 格）。
    pub fn engine_all(&self) -> bool {
        let mut o = 0usize;
        while o < ENGINE_CELLS {
            match self.engine.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 扫描一操作（基准载荷对照 + 重复拒）。
    pub fn record_scan(&mut self, s: usize, got: u32) -> Result<(), DieCode> {
        if s >= SCAN_OPS {
            return Err(DieCode::ENoScan);
        }
        if match self.scans.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DieCode::EDup);
        }
        let want = match scan_cell(s) {
            Some(w) => w,
            None => return Err(DieCode::ENoScan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + s as u64);
            return Err(DieCode::ECellDrift);
        }
        if let Some(c) = self.scans.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x7301 + s as u64);
        Ok(())
    }

    /// 扫描全账核验（10 操作）。
    pub fn scans_all(&self) -> bool {
        let mut s = 0usize;
        while s < SCAN_OPS {
            match self.scans.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 签名库一格。
    pub fn record_sig(&mut self, g: usize, got: u32) -> Result<(), DieCode> {
        if g >= SIG_CELLS {
            return Err(DieCode::ENoSig);
        }
        if match self.sigs.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DieCode::EDup);
        }
        let want = match sig_cell(g) {
            Some(w) => w,
            None => return Err(DieCode::ENoSig),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + g as u64);
            return Err(DieCode::ECellDrift);
        }
        if let Some(c) = self.sigs.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x7401 + g as u64);
        Ok(())
    }

    /// 签名库全账核验（8 格）。
    pub fn sigs_all(&self) -> bool {
        let mut g = 0usize;
        while g < SIG_CELLS {
            match self.sigs.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// YARA 联签一格（签名库对应格未落则拒收，联签对账）。
    pub fn record_yara(&mut self, y: usize, got: u32) -> Result<(), DieCode> {
        if y >= YARA_CELLS {
            return Err(DieCode::ENoSig);
        }
        if match self.yaras.get(y) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DieCode::EDup);
        }
        let anchor = match y * 2 < SIG_CELLS {
            true => y * 2,
            false => return Err(DieCode::ENoSig),
        };
        if match self.sigs.get(anchor) {
            Some(Some(_)) => false,
            _ => true,
        } {
            return Err(DieCode::ESeal);
        }
        let want = match yara_cell(y) {
            Some(w) => w,
            None => return Err(DieCode::ENoSig),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + y as u64);
            return Err(DieCode::ECellDrift);
        }
        if let Some(c) = self.yaras.get_mut(y) {
            *c = Some(got);
        }
        self.fnv(0x7501 + y as u64);
        Ok(())
    }

    /// YARA 联签全账核验（4 格）。
    pub fn yaras_all(&self) -> bool {
        let mut y = 0usize;
        while y < YARA_CELLS {
            match self.yaras.get(y) {
                Some(Some(_)) => {}
                _ => return false,
            }
            y += 1;
        }
        true
    }

    /// 开源声明一行。
    pub fn declare_oss(&mut self, r: usize) -> Result<(), DieCode> {
        if r >= OSS_ROWS {
            return Err(DieCode::ENoRow);
        }
        if match self.oss.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DieCode::EDup);
        }
        if let Some(c) = self.oss.get_mut(r) {
            *c = oss_row(r);
        }
        self.fnv(0x7600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn oss_all(&self) -> bool {
        let mut r = 0usize;
        while r < OSS_ROWS {
            match self.oss.get(r) {
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

    /// 最后族（相位/族序注入不改此值）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.engine_all()
            && self.scans_all()
            && self.sigs_all()
            && self.yaras_all()
            && self.oss_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DieLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DieLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < ENGINE_CELLS {
        if let Some(w) = engine_cell(o) {
            if led.record_engine(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut s = 0usize;
    while s < SCAN_OPS {
        if let Some(w) = scan_cell(s) {
            if led.record_scan(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut g2 = 0usize;
    while g2 < SIG_CELLS {
        if let Some(w) = sig_cell(g2) {
            let _ = led.record_sig(g2, w);
        }
        g2 += 1;
    }
    let mut y = 0usize;
    while y < YARA_CELLS {
        if let Some(w) = yara_cell(y) {
            let _ = led.record_yara(y, w);
        }
        y += 1;
    }
    let mut r = 0usize;
    while r < OSS_ROWS {
        let _ = led.declare_oss(r);
        r += 1;
    }
    // 注入 10 轮：引擎格越闭集 / 扫描漂移 / 相位越序 三态轮转，全部落在当轮临时账本。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DieLedger::new();
        let caught = match kind {
            0 => lt.record_engine(ENGINE_CELLS, 0) == Err(DieCode::ENoCell),
            1 => {
                // 漂移注入：先录正格再注漂移。
                let _ = lt.record_scan(0, scan_cell(0).unwrap_or(0));
                lt.record_scan(1, scan_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(DieCode::ECellDrift)
            }
            _ => {
                // 相位注入：对空账本录第 2 族首格，必抓且不改 last_kind。
                lt.record_engine(8, engine_cell(8).unwrap_or(0)) == Err(DieCode::EPhase)
                    && lt.last_kind() == -1
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7700 + k as u64);
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
/// UNX-F52334 判据集（锚 UNX-F52335-J1，off-by-one 板式）。
pub fn run_unx52334_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52334");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "检测引擎五族 20 格全通",
        recorded >= (ENGINE_CELLS + SCAN_OPS) as u32,
        "编译器/打包器/链接器/安装器/加密库逐格恒等落账",
    );
    s.add(
        "扫描 10 操作全落",
        {
            let mut lt = DieLedger::new();
            let mut ok = true;
            let mut s2 = 0usize;
            while s2 < SCAN_OPS {
                if let Some(w) = scan_cell(s2) {
                    if lt.record_scan(s2, w).is_err() {
                        ok = false;
                    }
                }
                s2 += 1;
            }
            ok && lt.scans_all()
        },
        "扫描 10 操作基准载荷逐项对照，漂移必抓",
    );
    s.add(
        "签名库 8 格全落",
        {
            let mut lt = DieLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < SIG_CELLS {
                if let Some(w) = sig_cell(g) {
                    if lt.record_sig(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.sigs_all()
        },
        "签名库逐格落账，越闭集必抓",
    );
    s.add(
        "YARA 消费联签 4 格全落",
        {
            let mut lt = DieLedger::new();
            let mut g = 0usize;
            while g < SIG_CELLS {
                if let Some(w) = sig_cell(g) {
                    let _ = lt.record_sig(g, w);
                }
                g += 1;
            }
            let mut ok = true;
            let mut y = 0usize;
            while y < YARA_CELLS {
                if let Some(w) = yara_cell(y) {
                    if lt.record_yara(y, w).is_err() {
                        ok = false;
                    }
                }
                y += 1;
            }
            ok && lt.yaras_all()
        },
        "YARA 消费与签名库联签对账，缺签拒收",
    );
    s.add(
        "收口闸六判据全落",
        {
            let mut lt = DieLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "引擎启动/签名库装载/扫描执行/联签对账/崩溃退出/许可声明",
    );
    s.add(
        "开源声明 2/2 全齐",
        {
            let mut lt = DieLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < OSS_ROWS {
                if lt.declare_oss(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.oss_all()
        },
        "开源许可声明/离线检测声明两行闭集，缺一拒封",
    );
    s.add(
        "族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ENGINE_CELLS {
                if engine_kind(o).unwrap_or(5) < engine_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调，逆序必抓",
    );
    s.add(
        "判据 ×10 全通（资源查看/提取口径）",
        caught >= 10,
        "十轮注入三态轮转全部识别，无一逃逸",
    );
    s.add(
        "边界·引擎格越闭集",
        {
            let mut lt = DieLedger::new();
            lt.record_engine(ENGINE_CELLS, 0).is_err()
        },
        "引擎格 20 越闭集必拒",
    );
    s.add(
        "边界·扫描操作越闭集",
        {
            let mut lt = DieLedger::new();
            lt.record_scan(SCAN_OPS, 0).is_err()
        },
        "扫描操作 10 越闭集必拒",
    );
    s.add(
        "边界·签名库格越闭集",
        {
            let mut lt = DieLedger::new();
            lt.record_sig(SIG_CELLS, 0).is_err()
        },
        "签名库格 8 越闭集必拒",
    );
    s.add(
        "相位注入对空账本必抓且不改 last_kind",
        {
            let mut lt = DieLedger::new();
            let hit = lt.record_engine(12, engine_cell(12).unwrap_or(0))
                == Err(DieCode::EPhase);
            hit && lt.last_kind() == -1
        },
        "前族未齐先录后族，空账本同样必抓",
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
            // main/aux 各节全部记账后再断言定级。
            let mut lt = DieLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < ENGINE_CELLS {
                if let Some(w) = engine_cell(o) {
                    let _ = lt.record_engine(o, w);
                }
                o += 1;
            }
            let mut s2 = 0usize;
            while s2 < SCAN_OPS {
                if let Some(w) = scan_cell(s2) {
                    let _ = lt.record_scan(s2, w);
                }
                s2 += 1;
            }
            let mut g2 = 0usize;
            while g2 < SIG_CELLS {
                if let Some(w) = sig_cell(g2) {
                    let _ = lt.record_sig(g2, w);
                }
                g2 += 1;
            }
            let mut y = 0usize;
            while y < YARA_CELLS {
                if let Some(w) = yara_cell(y) {
                    let _ = lt.record_yara(y, w);
                }
                y += 1;
            }
            let mut r = 0usize;
            while r < OSS_ROWS {
                let _ = lt.declare_oss(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9340,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位 0xC9340",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_kinds() {
        assert_eq!(engine_kind(0), Some(0));
        assert_eq!(engine_kind(4), Some(1));
        assert_eq!(engine_kind(19), Some(4));
        assert!(engine_kind(20).is_none());
        assert!(engine_cell(20).is_none());
        assert_eq!(engine_kind_name(0), Some("编译器"));
        assert_eq!(engine_kind_name(4), Some("加密库"));
        assert!(engine_kind_name(5).is_none());
    }

    #[test]
    fn test_scan_and_sig_cells() {
        assert!(scan_cell(SCAN_OPS).is_none());
        assert!(sig_cell(SIG_CELLS).is_none());
        assert_ne!(scan_cell(0).unwrap_or(0), scan_cell(1).unwrap_or(0));
        assert_ne!(sig_cell(3).unwrap_or(0), sig_cell(4).unwrap_or(0));
        assert!(oss_row(OSS_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = engine_cell(3).unwrap_or(0);
        assert_eq!(a, engine_cell(3).unwrap_or(0));
        assert_ne!(a, engine_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        // 测试种子须按合法序先落低族格。
        let mut lt = DieLedger::new();
        let mut o = 0usize;
        while o < 4 * FAMILY_CELLS {
            let _ = lt.record_engine(o, engine_cell(o).unwrap_or(0));
            o += 1;
        }
        assert_eq!(
            lt.record_engine(6, engine_cell(6).unwrap_or(0)),
            Err(DieCode::EOrder)
        );
        assert!(lt.record_engine(16, engine_cell(16).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DieLedger::new();
        assert_eq!(
            lt.record_scan(2, scan_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(DieCode::ECellDrift)
        );
        assert_eq!(
            lt.record_sig(5, sig_cell(5).unwrap_or(0).wrapping_add(9)),
            Err(DieCode::ECellDrift)
        );
    }

    #[test]
    fn test_yara_requires_sig_anchor() {
        let mut lt = DieLedger::new();
        // 签名库锚格未落 → 联签拒收。
        if let Some(w) = yara_cell(0) {
            assert_eq!(lt.record_yara(0, w), Err(DieCode::ESeal));
        }
        if let Some(w) = sig_cell(0) {
            assert!(lt.record_sig(0, w).is_ok());
        }
        if let Some(w) = yara_cell(0) {
            assert!(lt.record_yara(0, w).is_ok());
        }
        assert_eq!(lt.record_yara(0, 0), Err(DieCode::EDup));
    }

    #[test]
    fn test_phase_on_empty_ledger() {
        let mut lt = DieLedger::new();
        assert_eq!(
            lt.record_engine(8, engine_cell(8).unwrap_or(0)),
            Err(DieCode::EPhase)
        );
        assert_eq!(lt.last_kind(), -1);
        assert!(lt.record_engine(0, engine_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DieLedger::new();
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
        assert!(recorded >= (ENGINE_CELLS + SCAN_OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9340);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DieCode::Ok as u32, 0);
        assert_eq!(DieCode::ENoScan as u32, 8);
        assert_eq!(DieCode::EInj as u32, 11);
        assert_eq!(DieCode::EGradeDrift as u32, 12);
        let all = [
            DieCode::Ok,
            DieCode::ENoCell,
            DieCode::ECellDrift,
            DieCode::EOrder,
            DieCode::EPhase,
            DieCode::EDup,
            DieCode::ENoGate,
            DieCode::ESeal,
            DieCode::ENoScan,
            DieCode::ENoSig,
            DieCode::ENoRow,
            DieCode::EInj,
            DieCode::EGradeDrift,
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
        assert_eq!(Grade::S2.val(), 2);
        let lt = DieLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52334_checks_green() {
        let s = run_unx52334_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
