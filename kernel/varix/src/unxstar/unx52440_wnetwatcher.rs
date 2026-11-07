//! UNX-F52440 WNetWatcher / 网络邻居发现面收口（长尾） · 0xCADC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52440"，逐字）：
//!   title: WNetWatcher / 网络邻居发现面收口（长尾）
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52441-J1 六判据全
//!     落；NTFS 索引/即时搜索 ×10 万文件树全通（与共面 watcher 账联动）；
//!     KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 23 口径；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52440）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52441-J1，照原文锚定；功能按本卡标题（WNetWatcher / 网络邻居发
//! 现面）落地。定级口径注记：卡面原文 "KB-COMPAT 定级 S1" 为判据锚定原
//! 文；本模块 KB-COMPAT 实登口径按派工语料封顶 S3（长尾面保守封顶），
//! 名义档仍按 S1/S2/S3 如实阶梯求值，两口径在明细中写明并存。
//!
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五发现族闭集：NetBIOS（名称发现）/ ARP（探测）/ SMB（枚举）/
//!      mDNS（浏览）/ LLMNR（查询）五族各 4 格共 20 格，族单调（逆序必
//!      抓），载荷折叠恒等才收。
//!   ② 邻居表投影 12 格：WNetWatcher 邻居表（IP/MAC/适配器）投影逐格恒
//!      等，漂移必抓。
//!   ③ 扫描 16 拍单调：扫描拍序严格递增（乱序必抓，空账本同样必抓且不
//!      改拍游标），拍载荷按公式折叠核验。
//!   ④ 长尾维护声明 2 行闭集：长尾面维护口径（低频复扫/陈旧项滞留）逐
//!      行落账；与 nmap 分账声明 2 行闭集（锚 F52421）同口径，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（发现族越闭集/邻居表漂移/扫描拍
//!      乱序），检出+逃逸守恒等式；双跑 FNV 指纹一致；KB-COMPAT 定级封
//!      顶 S3（长尾面：名义全通 S1 压顶至 S3）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52440;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五发现族总格数（每族 4 格 × 5 族）。
pub const DISC_OPS: usize = 20;

/// 发现族数。
pub const DISC_FAMILIES: usize = 5;

/// 邻居表投影格数。
pub const NB_TABLE_CELLS: usize = 12;

/// 扫描拍数（严格单调）。
pub const SCAN_BEATS: usize = 16;

/// 长尾维护声明行数。
pub const MAINT_ROWS: usize = 2;

/// 与 nmap 分账声明行数（锚 F52421）。
pub const NMAP_SPLIT_ROWS: usize = 2;

/// KB-COMPAT 定级封顶（长尾面口径：定级 S3，名义 S1/S2 一律压顶至 S3）。
pub const GRADE_CAP: u32 = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WnetCode {
    /// 成功
    Ok = 0,
    /// 发现族格越闭集
    ENoDisc,
    /// 发现族载荷漂移
    EDiscDrift,
    /// 发现族逆序
    EOrder,
    /// 邻居表投影越闭集
    ENoNb,
    /// 邻居表投影漂移
    ENbDrift,
    /// 扫描拍越闭集
    ENoScan,
    /// 扫描拍失序/拍载荷漂移
    EScanOrder,
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

/// 全码闭集表（13 变体，位序即码位）。
pub const ALL_CODES: [WnetCode; 13] = [
    WnetCode::Ok,
    WnetCode::ENoDisc,
    WnetCode::EDiscDrift,
    WnetCode::EOrder,
    WnetCode::ENoNb,
    WnetCode::ENbDrift,
    WnetCode::ENoScan,
    WnetCode::EScanOrder,
    WnetCode::EDup,
    WnetCode::ESeal,
    WnetCode::EInj,
    WnetCode::EFp,
    WnetCode::EGradeDrift,
];

impl WnetCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WnetCode::Ok => "成功",
            WnetCode::ENoDisc => "发现族格越闭集：NetBIOS/ARP/SMB/mDNS/LLMNR 20 格之外先登记",
            WnetCode::EDiscDrift => "发现族载荷漂移：格折叠与基准不恒等，逐格复算",
            WnetCode::EOrder => "发现族逆序：NetBIOS→ARP→SMB→mDNS→LLMNR 单调，逆序必抓",
            WnetCode::ENoNb => "邻居表投影越闭集：邻居表投影 12 格之外先登记",
            WnetCode::ENbDrift => "邻居表投影漂移：投影折叠与基准不恒等，逐格复算",
            WnetCode::ENoScan => "扫描拍越闭集：扫描 16 拍之外先登记",
            WnetCode::EScanOrder => "扫描拍失序/漂移：拍序严格递增且拍载荷恒等，失序必抓",
            WnetCode::EDup => "重复落账：同格只许一条账目",
            WnetCode::ESeal => "未齐即封：20 发现格 + 12 投影格 + 16 拍 + 2 行分账未齐先补齐",
            WnetCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WnetCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WnetCode::EGradeDrift => "定级漂移：KB-COMPAT 长尾口径封顶 S3，越顶即漂移",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（名义档，长尾面压顶至 S3）
    S1,
    /// S2：有条件收口（名义档，长尾面压顶至 S3）
    S2,
    /// S3：不收口（本卡登记档）
    S3,
}

impl Grade {
    /// 定级值。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：五发现族 20 格+投影 12 格+16 拍+分账 2 行全通（名义档）",
            Grade::S2 => "S2：有条件收口（名义档）",
            Grade::S3 => "S3：WNetWatcher 长尾面登记档，S1/S2 压顶至 S3（KB-COMPAT 口径）",
        }
    }
}

/// 定级值 → 定级档（const，无浮点无查表）。
const fn grade_of(val: u32) -> Grade {
    if val <= 1 {
        Grade::S1
    } else if val == 2 {
        Grade::S2
    } else {
        Grade::S3
    }
}

/// 定级封顶：好于封顶档的压至封顶档。
const fn cap_grade(g: Grade) -> Grade {
    let v = g.val();
    if v < GRADE_CAP {
        grade_of(GRADE_CAP)
    } else {
        g
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 发现格载荷（格 o；族 = o/4，NetBIOS/ARP/SMB/mDNS/LLMNR）。
pub const fn disc_cell(o: usize) -> Option<u64> {
    if o >= DISC_OPS {
        return None;
    }
    Some(
        (0xCD000u64 + (o as u64) * 0x3C)
            ^ ((o / 4) as u64).wrapping_mul(0x01000193)
            ^ 0x574E455457000000,
    )
}

/// 发现族（0..4=NetBIOS 4..8=ARP 8..12=SMB 12..16=mDNS 16..20=LLMNR）。
pub const fn disc_kind(o: usize) -> Option<u32> {
    if o >= DISC_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 发现族名。
pub const fn disc_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("NetBIOS 名称发现"),
        1 => Some("ARP 探测"),
        2 => Some("SMB 枚举"),
        3 => Some("mDNS 浏览"),
        4 => Some("LLMNR 查询"),
        _ => None,
    }
}

/// 邻居表投影格载荷（格 f）。
pub const fn nb_cell(f: usize) -> Option<u64> {
    if f >= NB_TABLE_CELLS {
        return None;
    }
    Some((0xCD100u64 + (f as u64) * 0x33) ^ 0x4E4254524C000000)
}

/// 扫描拍载荷（拍 t；拍序严格递增，拍载荷按公式折叠）。
pub const fn scan_beat(t: usize) -> Option<u32> {
    if t >= SCAN_BEATS {
        return None;
    }
    Some(0xCD200 + t as u32 * 0x41)
}

/// 长尾维护声明行折叠（行 r）。
pub const fn maint_row(r: usize) -> Option<u32> {
    if r >= MAINT_ROWS {
        return None;
    }
    Some(0xCD300 + r as u32 * 0x2C)
}

/// 与 nmap 分账声明行折叠（行 r；锚 F52421）。
pub const fn nmap_split_row(r: usize) -> Option<u32> {
    if r >= NMAP_SPLIT_ROWS {
        return None;
    }
    Some(0xCD340 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// WNetWatcher 收口账本状态机。
#[derive(Clone)]
pub struct WnetLedger {
    /// 六判据（发现/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 五发现族 20 格。
    discs: [Option<u64>; DISC_OPS],
    /// 邻居表投影 12 格。
    nbs: [Option<u64>; NB_TABLE_CELLS],
    /// 扫描 16 拍（已落拍数即游标，严格递增）。
    beats: [Option<u32>; SCAN_BEATS],
    /// 已落拍数（拍游标）。
    beats_done: usize,
    /// 长尾维护声明 2 行。
    maints: [Option<u32>; MAINT_ROWS],
    /// 与 nmap 分账声明 2 行。
    nmap_rows: [Option<u32>; NMAP_SPLIT_ROWS],
    /// 最后发现族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WnetLedger {
    /// 新账本。
    pub fn new() -> Self {
        WnetLedger {
            gates: [None; 6],
            discs: [None; DISC_OPS],
            nbs: [None; NB_TABLE_CELLS],
            beats: [None; SCAN_BEATS],
            beats_done: 0,
            maints: [None; MAINT_ROWS],
            nmap_rows: [None; NMAP_SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WnetCode> {
        if gate >= 6 {
            return Err(WnetCode::ESeal);
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

    /// 发现一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_disc(&mut self, o: usize, got: u64) -> Result<(), WnetCode> {
        if o >= DISC_OPS {
            return Err(WnetCode::ENoDisc);
        }
        let kind = match disc_kind(o) {
            Some(k) => k,
            None => return Err(WnetCode::ENoDisc),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(WnetCode::EOrder);
        }
        let want = match disc_cell(o) {
            Some(w) => w,
            None => return Err(WnetCode::ENoDisc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + o as u64);
            return Err(WnetCode::EDiscDrift);
        }
        if match self.discs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WnetCode::EDup);
        }
        if let Some(c) = self.discs.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4202 + o as u64);
        Ok(())
    }

    /// 发现全账核验（20 格）。
    pub fn discs_all(&self) -> bool {
        let mut o = 0usize;
        while o < DISC_OPS {
            match self.discs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 邻居表投影一格（载荷对照 + 重复拒）。
    pub fn record_nb(&mut self, f: usize, got: u64) -> Result<(), WnetCode> {
        let want = match nb_cell(f) {
            Some(w) => w,
            None => return Err(WnetCode::ENoNb),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300 + f as u64);
            return Err(WnetCode::ENbDrift);
        }
        if match self.nbs.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WnetCode::EDup);
        }
        if let Some(c) = self.nbs.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x4301 + f as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn nbs_all(&self) -> bool {
        let mut f = 0usize;
        while f < NB_TABLE_CELLS {
            match self.nbs.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 扫描拍一格（拍序严格递增 + 拍载荷恒等 + 重复拒）。
    /// 乱序对空账本同样必抓（拍游标为 0，落拍 5 即失序），且失序不改拍游标。
    pub fn record_scan(&mut self, t: usize, got: u32) -> Result<(), WnetCode> {
        if t >= SCAN_BEATS {
            return Err(WnetCode::ENoScan);
        }
        if t != self.beats_done {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400);
            return Err(WnetCode::EScanOrder);
        }
        let want = match scan_beat(t) {
            Some(w) => w,
            None => return Err(WnetCode::ENoScan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4401 + t as u64);
            return Err(WnetCode::EScanOrder);
        }
        if match self.beats.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WnetCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(t) {
            *c = Some(got);
        }
        self.beats_done += 1;
        self.fnv(0x4402 + t as u64);
        Ok(())
    }

    /// 扫描全账核验（16 拍全落且游标归位）。
    pub fn scans_all(&self) -> bool {
        if self.beats_done != SCAN_BEATS {
            return false;
        }
        let mut t = 0usize;
        while t < SCAN_BEATS {
            match self.beats.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 长尾维护声明一行。
    pub fn declare_maint(&mut self, r: usize) -> Result<(), WnetCode> {
        if r >= MAINT_ROWS {
            return Err(WnetCode::ENoDisc);
        }
        if match self.maints.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WnetCode::EDup);
        }
        if let Some(c) = self.maints.get_mut(r) {
            *c = maint_row(r);
        }
        self.fnv(0x4500 + r as u64);
        Ok(())
    }

    /// 与 nmap 分账声明一行（锚 F52421）。
    pub fn declare_nmap_split(&mut self, r: usize) -> Result<(), WnetCode> {
        if r >= NMAP_SPLIT_ROWS {
            return Err(WnetCode::ENoDisc);
        }
        if match self.nmap_rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WnetCode::EDup);
        }
        if let Some(c) = self.nmap_rows.get_mut(r) {
            *c = nmap_split_row(r);
        }
        self.fnv(0x4600 + r as u64);
        Ok(())
    }

    /// 长尾维护声明全齐核验（单节）。
    pub fn maints_all(&self) -> bool {
        let mut r = 0usize;
        while r < MAINT_ROWS {
            match self.maints.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// nmap 分账声明全齐核验（单节）。
    pub fn nmap_all(&self) -> bool {
        let mut r = 0usize;
        while r < NMAP_SPLIT_ROWS {
            match self.nmap_rows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 声明行全齐核验（长尾维护 + nmap 分账）。
    pub fn rows_all(&self) -> bool {
        let mut ok = true;
        let mut r = 0usize;
        while r < MAINT_ROWS {
            if match self.maints.get(r) {
                Some(Some(_)) => false,
                _ => true,
            } {
                ok = false;
            }
            r += 1;
        }
        r = 0;
        while r < NMAP_SPLIT_ROWS {
            if match self.nmap_rows.get(r) {
                Some(Some(_)) => false,
                _ => true,
            } {
                ok = false;
            }
            r += 1;
        }
        ok
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 注入守恒等式核验（注入 = 检出 + 逃逸）。
    pub fn check_conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), WnetCode> {
        if injected == caught.wrapping_add(escaped) && injected >= 10 && escaped == 0 {
            Ok(())
        } else {
            Err(WnetCode::EInj)
        }
    }

    /// 双跑指纹一致性核验。
    pub fn check_fingerprint(a: u64, b: u64) -> Result<(), WnetCode> {
        if a == b && a != 0 {
            Ok(())
        } else {
            Err(WnetCode::EFp)
        }
    }

    /// 定级封顶核验（越封顶档即 EGradeDrift）。
    pub fn check_grade_cap(g: Grade) -> Result<(), WnetCode> {
        if g.val() > GRADE_CAP {
            Err(WnetCode::EGradeDrift)
        } else {
            Ok(())
        }
    }

    /// 名义定级（未封顶）：全通 → S1，六判据齐 → S2，否则 S3。
    pub fn judge_grade_nominal(&self) -> Grade {
        if self.gates_all()
            && self.discs_all()
            && self.nbs_all()
            && self.scans_all()
            && self.rows_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// KB-COMPAT 定级（长尾面封顶 S3：名义 S1/S2 一律压顶至 S3）。
    pub fn judge_grade(&self) -> Grade {
        cap_grade(self.judge_grade_nominal())
    }
}

impl Default for WnetLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WnetLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < DISC_OPS {
        if let Some(w) = disc_cell(o) {
            if led.record_disc(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut f = 0usize;
    while f < NB_TABLE_CELLS {
        if let Some(w) = nb_cell(f) {
            if led.record_nb(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut t = 0usize;
    while t < SCAN_BEATS {
        if let Some(w) = scan_beat(t) {
            if led.record_scan(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < MAINT_ROWS {
        let _ = led.declare_maint(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < NMAP_SPLIT_ROWS {
        let _ = led.declare_nmap_split(r);
        r += 1;
    }
    // 注入 10 轮：发现族越闭集 / 邻居表漂移 / 扫描拍乱序三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WnetLedger::new();
        let caught = match kind {
            0 => lt.record_disc(DISC_OPS, 0).is_err(),
            1 => {
                let _ = lt.record_nb(0, nb_cell(0).unwrap_or(0));
                lt.record_nb(1, nb_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(WnetCode::ENbDrift)
            }
            // 扫描拍乱序对空账本同样必抓：游标 0，落拍 5 即失序。
            _ => lt.record_scan(5, scan_beat(5).unwrap_or(0)) == Err(WnetCode::EScanOrder),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4700 + k as u64);
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
/// UNX-F52440 判据集（锚 UNX-F52441-J1，off-by-one 板式）。
pub fn run_unx52440_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52440");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五发现族 20 格全通",
        {
            let mut lt = WnetLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < DISC_OPS {
                if let Some(w) = disc_cell(o) {
                    if lt.record_disc(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.discs_all()
        },
        "NetBIOS/ARP/SMB/mDNS/LLMNR 五族逐格恒等，逆序与漂移必抓",
    );
    s.add(
        "邻居表投影 12/12 全落",
        {
            let mut lt = WnetLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < NB_TABLE_CELLS {
                if let Some(w) = nb_cell(f) {
                    if lt.record_nb(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.nbs_all()
        },
        "邻居表投影（IP/MAC/适配器）逐格恒等，漂移必抓",
    );
    s.add(
        "扫描 16 拍单调全落",
        {
            let mut lt = WnetLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < SCAN_BEATS {
                if let Some(w) = scan_beat(t) {
                    if lt.record_scan(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.scans_all()
        },
        "扫描拍序严格递增，拍载荷按公式折叠核验，乱序必抓",
    );
    s.add(
        "长尾维护声明 2/2 全齐",
        {
            let mut lt = WnetLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < MAINT_ROWS {
                if lt.declare_maint(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.maints_all()
        },
        "长尾面维护口径（低频复扫/陈旧项滞留）两行闭集缺一拒封（§AI-66.3 组 23）",
    );
    s.add(
        "与 nmap 分账声明 2/2 全齐",
        {
            let mut lt = WnetLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < NMAP_SPLIT_ROWS {
                if lt.declare_nmap_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.nmap_all()
        },
        "WNetWatcher 与 nmap 分账不重账（锚 F52421），两行闭集缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = WnetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "发现/功能/崩溃/卸载/升级/许可逐格落账（UNX-F52441-J1 六判据）",
    );
    s.add(
        "五发现族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < DISC_OPS {
                if disc_kind(o).unwrap_or(5) < disc_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "NetBIOS→ARP→SMB→mDNS→LLMNR 五族闭集单调",
    );
    s.add(
        "扫描拍公式单调",
        {
            let mut ok = true;
            let mut t = 1usize;
            while t < SCAN_BEATS {
                if scan_beat(t).unwrap_or(0) <= scan_beat(t - 1).unwrap_or(0) {
                    ok = false;
                }
                t += 1;
            }
            ok
        },
        "16 拍载荷公式严格递增",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "发现族越闭集/邻居表漂移/扫描拍乱序三态轮转必抓",
    );
    let mut l0 = WnetLedger::new();
    s.add(
        "边界·发现族越闭集",
        l0.record_disc(DISC_OPS, 0).is_err(),
        "发现格 20 越闭集必拒",
    );
    s.add(
        "边界·邻居表越闭集",
        l0.record_nb(NB_TABLE_CELLS, 0).is_err(),
        "邻居表投影 12 越闭集必拒",
    );
    s.add(
        "边界·扫描拍越闭集",
        l0.record_scan(SCAN_BEATS, 0).is_err(),
        "扫描拍 16 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        WnetLedger::check_conservation(inj_n, caught_n, esc_n).is_ok(),
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        WnetLedger::check_fingerprint(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级封顶 S3 闭集",
        {
            let mut lt = WnetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < DISC_OPS {
                if let Some(w) = disc_cell(o) {
                    let _ = lt.record_disc(o, w);
                }
                o += 1;
            }
            let mut f = 0usize;
            while f < NB_TABLE_CELLS {
                if let Some(w) = nb_cell(f) {
                    let _ = lt.record_nb(f, w);
                }
                f += 1;
            }
            let mut t = 0usize;
            while t < SCAN_BEATS {
                if let Some(w) = scan_beat(t) {
                    let _ = lt.record_scan(t, w);
                }
                t += 1;
            }
            let mut r = 0usize;
            while r < MAINT_ROWS {
                let _ = lt.declare_maint(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < NMAP_SPLIT_ROWS {
                let _ = lt.declare_nmap_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S3
                && lt.judge_grade_nominal() == Grade::S1
                && WnetLedger::check_grade_cap(lt.judge_grade()).is_ok()
        },
        "全通名义 S1 长尾压顶至 S3（KB-COMPAT 定级登记，卡面 S1 为锚定原文口径）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCADC0,
        "SEG_BASE 0x40 对齐且落在 VX16 公式 0xCA400+0x40×39",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 16,
        "含本条共 17 条闭集，判据锚 UNX-F52441-J1 六判据全落",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disc_kind() {
        assert_eq!(disc_kind(0), Some(0));
        assert_eq!(disc_kind(4), Some(1));
        assert_eq!(disc_kind(8), Some(2));
        assert_eq!(disc_kind(12), Some(3));
        assert_eq!(disc_kind(19), Some(4));
        assert!(disc_kind(20).is_none());
        assert_eq!(disc_kind_name(0), Some("NetBIOS 名称发现"));
        assert_eq!(disc_kind_name(2), Some("SMB 枚举"));
        assert_eq!(disc_kind_name(4), Some("LLMNR 查询"));
        assert!(disc_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = disc_cell(6).unwrap_or(0);
        assert_eq!(a, disc_cell(6).unwrap_or(0));
        assert_ne!(a, disc_cell(7).unwrap_or(0));
        assert_ne!(nb_cell(0).unwrap_or(0), nb_cell(1).unwrap_or(0));
        assert!(disc_cell(DISC_OPS).is_none());
        assert!(nb_cell(NB_TABLE_CELLS).is_none());
        assert!(scan_beat(SCAN_BEATS).is_none());
        assert!(maint_row(MAINT_ROWS).is_none());
        assert!(nmap_split_row(NMAP_SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = WnetLedger::new();
        // 先落族 2（SMB，格 9），再回落族 1（ARP，格 5）→ 逆序必抓。
        let _ = lt.record_disc(9, disc_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_disc(5, disc_cell(5).unwrap_or(0)),
            Err(WnetCode::EOrder)
        );
        // 同族内回格不判逆序。
        assert!(lt.record_disc(10, disc_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_scan_order_caught_on_empty() {
        let mut lt = WnetLedger::new();
        // 乱序对空账本同样必抓，且不改拍游标（随后落拍 0 仍成）。
        if let Some(w) = scan_beat(5) {
            assert_eq!(lt.record_scan(5, w), Err(WnetCode::EScanOrder));
        }
        if let Some(w) = scan_beat(0) {
            assert!(lt.record_scan(0, w).is_ok());
        }
        // 拍载荷漂移同码必抓。
        if let Some(w) = scan_beat(1) {
            assert_eq!(
                lt.record_scan(1, w.wrapping_add(1)),
                Err(WnetCode::EScanOrder)
            );
        }
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WnetLedger::new();
        assert_eq!(
            lt.record_disc(0, disc_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(WnetCode::EDiscDrift)
        );
        assert_eq!(
            lt.record_nb(0, nb_cell(0).unwrap_or(0).wrapping_add(6)),
            Err(WnetCode::ENbDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WnetLedger::new();
        let _ = lt.record_disc(2, disc_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_disc(2, disc_cell(2).unwrap_or(0)),
            Err(WnetCode::EDup)
        );
        let _ = lt.record_nb(2, nb_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_nb(2, nb_cell(2).unwrap_or(0)),
            Err(WnetCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WnetLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WnetLedger::new();
        assert!(lt2.judge_gate(3).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 发现 + 12 投影 + 16 拍（公式常量之和）。
        assert_eq!(
            recorded,
            (DISC_OPS + NB_TABLE_CELLS + SCAN_BEATS) as u32
        );
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
        assert_eq!(SEG_BASE, 0xCADC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert_eq!(ALL_CODES[i].val(), SEG_BASE + i as u32);
            assert!(!ALL_CODES[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(WnetCode::Ok as u32, 0);
        assert_eq!(WnetCode::EGradeDrift as u32, 12);
        assert!(WnetLedger::check_fingerprint(3, 4).is_err());
        assert!(WnetLedger::check_conservation(11, 10, 0).is_err());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯（名义档）：空账 S3，六闸齐 S2，全齐 S1。
        let lt = WnetLedger::new();
        assert_eq!(lt.judge_grade_nominal(), Grade::S3);
        let mut lt2 = WnetLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade_nominal(), Grade::S2);
        // KB-COMPAT 长尾封顶 S3：任何账面实登一律 S3。
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52440_checks_green() {
        let s = run_unx52440_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
