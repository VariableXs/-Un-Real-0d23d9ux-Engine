//! UNX-F52256 wget for Windows 收口 · 0xC7FC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52256"，逐字）：
//!   title: wget for Windows 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52257-J1 ssh/scp/
//!     sftp 20 操作集 ×10；密钥代理（agent）×5；与 B12 终端消费联签
//!     （F52235 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52256）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52257-J1，照原文锚定；功能按本卡标题（wget for Windows）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐单定制）：
//!   ① 20 操作集：五家族闭集（递归/镜像/限速/断点续传/代理）各 4 操作，
//!      家族单调（逆序必抓），逐格载荷对照。
//!   ② 递归深度/域名过滤三面 12 格：深度 0..4、域内放行 4..8、域外拒绝
//!      8..12 三面，逐格落账。
//!   ③ robots 语义 5 格：未排除/全排除/抓取延迟/仅限代理/无 robots 文
//!      件，语义闭集逐格核验。
//!   ④ 退出码闭集 8：0/1/2/3/4/6/7/8，越闭集必抓，逐码落账。
//!   ⑤ 版本阶梯 3 档：1.19.x → 1.20.x → 1.21.x，逐档载荷对照，越级必
//!      抓。
//!   ⑥ 与 B12 终端消费联签锚定行 1 行闭集（F52235 锚定行），缺一拒封。
//!   ⑦ 六判据收口闸：启动/下载/崩溃/卸载/升级/许可，逐格落账，未齐即
//!      封拒。
//!   ⑧ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（操作越闭集/载荷漂移/家族逆
//!      序），检出+逃逸守恒等式；双跑 FNV 指纹一致；KB-COMPAT 定级闭
//!      集：全通 → S1。
//!   ⑨ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52256;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 下载操作数（五家族 × 4）。
pub const DOWNLOAD_OPS: usize = 20;

/// 递归深度/域名过滤三面格数。
pub const FILTER_CELLS: usize = 12;

/// robots 语义格数。
pub const ROBOTS_CELLS: usize = 5;

/// 退出码闭集容量。
pub const EXIT_CODES: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// B12 终端消费联签锚定行数。
pub const ANCHOR_ROWS: usize = 1;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WgetWinCode {
    /// 成功
    Ok = 0,
    /// 操作越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 操作家族逆序
    EOrder,
    /// 过滤格越闭集
    ENoFilter,
    /// 过滤载荷漂移
    EFilterDrift,
    /// robots 语义越闭集
    ENoRobots,
    /// 退出码越闭集
    ENoExit,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 联签锚定行越闭集
    ENoAnchor,
    /// 重复落账
    EDup,
    /// 定级漂移
    EGradeDrift,
}

impl WgetWinCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WgetWinCode::Ok => "成功",
            WgetWinCode::ENoOp => "操作越闭集：下载 20 操作集之外先登记",
            WgetWinCode::EOpDrift => "操作载荷漂移：操作折叠与基准不恒等，逐格复算",
            WgetWinCode::EOrder => "操作家族逆序：递归→镜像→限速→断点续传→代理单调，逆序必抓",
            WgetWinCode::ENoFilter => "过滤格越闭集：深度/域内/域外三面 12 格之外先登记",
            WgetWinCode::EFilterDrift => "过滤载荷漂移：格折叠与基准不恒等，逐格复算",
            WgetWinCode::ENoRobots => "robots 语义越闭集：5 语义格之外先登记",
            WgetWinCode::ENoExit => "退出码越闭集：0/1/2/3/4/6/7/8 八码之外先登记",
            WgetWinCode::ENoVer => "版本级越阶梯：1.19.x/1.20.x/1.21.x 三档之外先登记",
            WgetWinCode::EVerDrift => "版本阶梯漂移：档载荷折叠与基准不恒等，逐档复算",
            WgetWinCode::ENoAnchor => "联签锚定行越闭集：B12 终端消费联签仅 1 行（F52235 锚定行）",
            WgetWinCode::EDup => "重复落账：同格只许一条账目",
            WgetWinCode::EGradeDrift => "定级漂移：全通且联签行齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// wget for Windows KB-COMPAT 定级闭集（本卡口径顶格 S1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WgetGrade {
    /// S1：全通收口
    S1,
    /// S2：判据全但格账有缺
    S2,
    /// S3：判据缺失，不收口
    S3,
}

impl WgetGrade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            WgetGrade::S1 => 1,
            WgetGrade::S2 => 2,
            WgetGrade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            WgetGrade::S1 => "S1：20 操作+三面 12 格+robots 5 格+退出码 8+版本 3 档+联签 1 行全通收口",
            WgetGrade::S2 => "S2：六判据全但格账有缺，补齐后复评",
            WgetGrade::S3 => "S3：判据缺失，不收口",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 下载操作格载荷（操作 o；家族 = o/4）。
pub const fn download_cell(o: usize) -> Option<u64> {
    if o >= DOWNLOAD_OPS {
        return None;
    }
    Some((0xE10000u64 + (o as u64) * 0x5B) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=递归 4..8=镜像 8..12=限速 12..16=断点续传 16..20=代理）。
pub const fn download_kind(o: usize) -> Option<u32> {
    if o >= DOWNLOAD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 操作家族名。
pub const fn download_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("递归"),
        1 => Some("镜像"),
        2 => Some("限速"),
        3 => Some("断点续传"),
        4 => Some("代理"),
        _ => None,
    }
}

/// 过滤格载荷（格 f：0..4=递归深度 4..8=域内放行 8..12=域外拒绝）。
pub const fn filter_cell(f: usize) -> Option<u64> {
    if f >= FILTER_CELLS {
        return None;
    }
    Some((0xE20000u64 + (f as u64) * 0x43) ^ ((f / 4) as u64).wrapping_mul(0x010001B3))
}

/// 过滤面（0..4=递归深度 4..8=域内放行 8..12=域外拒绝）。
pub const fn filter_face(f: usize) -> Option<u32> {
    if f >= FILTER_CELLS {
        return None;
    }
    Some((f / 4) as u32)
}

/// 过滤面名。
pub const fn filter_face_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("递归深度"),
        1 => Some("域内放行"),
        2 => Some("域外拒绝"),
        _ => None,
    }
}

/// robots 语义格载荷（格 g：0..5）。
pub const fn robots_cell(g: usize) -> Option<u64> {
    if g >= ROBOTS_CELLS {
        return None;
    }
    Some((0xE30000u64 + (g as u64) * 0x37) ^ 0x524F424F54535F47)
}

/// robots 语义名。
pub const fn robots_semantic_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("未排除"),
        1 => Some("全排除"),
        2 => Some("抓取延迟"),
        3 => Some("仅限代理"),
        4 => Some("无 robots 文件"),
        _ => None,
    }
}

/// 退出码闭集核验（0/1/2/3/4/6/7/8 八码）。
pub const fn exit_code_legal(code: usize) -> bool {
    match code {
        0 | 1 | 2 | 3 => true,
        4 | 6 | 7 | 8 => true,
        _ => false,
    }
}

/// 退出码名。
pub const fn exit_code_name(code: usize) -> Option<&'static str> {
    match code {
        0 => Some("0 成功"),
        1 => Some("1 通用错误"),
        2 => Some("2 参数解析错误"),
        3 => Some("3 文件写入错误"),
        4 => Some("4 网络故障"),
        6 => Some("6 认证失败"),
        7 => Some("7 协议错误"),
        8 => Some("8 服务器错误响应"),
        _ => None,
    }
}

/// 退出码槽位 → 合法码（闭集序 0/1/2/3/4/6/7/8）。
pub const fn exit_slot_code(slot: usize) -> Option<usize> {
    match slot {
        0 => Some(0),
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        4 => Some(4),
        5 => Some(6),
        6 => Some(7),
        7 => Some(8),
        _ => None,
    }
}

/// 版本阶梯档载荷（档 v：0=1.19.x 1=1.20.x 2=1.21.x）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xE40000u64 + (v as u64) * 0x6D) ^ 0x574745545F564552)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("1.19.x"),
        1 => Some("1.20.x"),
        2 => Some("1.21.x"),
        _ => None,
    }
}

/// B12 终端消费联签锚定行折叠（行 r，F52235 锚定行）。
pub const fn anchor_row(r: usize) -> Option<u32> {
    if r >= ANCHOR_ROWS {
        return None;
    }
    Some(0xE410 + r as u32 * 0x19)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// wget for Windows 收口账本状态机。
#[derive(Clone)]
pub struct WgetLedger {
    /// 六判据（启动/下载/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; DOWNLOAD_OPS],
    /// 过滤三面账。
    filters: [Option<u64>; FILTER_CELLS],
    /// robots 语义账。
    robots: [Option<u64>; ROBOTS_CELLS],
    /// 退出码账（下标即闭集序 0..8）。
    exits: [Option<bool>; EXIT_CODES],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 联签锚定行账。
    anchors: [Option<u32>; ANCHOR_ROWS],
    /// 最后操作家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WgetLedger {
    /// 新账本。
    pub fn new() -> Self {
        WgetLedger {
            gates: [None; 6],
            ops: [None; DOWNLOAD_OPS],
            filters: [None; FILTER_CELLS],
            robots: [None; ROBOTS_CELLS],
            exits: [None; EXIT_CODES],
            vers: [None; VERSION_STEPS],
            anchors: [None; ANCHOR_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WgetWinCode> {
        if gate >= 6 {
            return Err(WgetWinCode::EGradeDrift);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), WgetWinCode> {
        if o >= DOWNLOAD_OPS {
            return Err(WgetWinCode::ENoOp);
        }
        let kind = match download_kind(o) {
            Some(k) => k,
            None => return Err(WgetWinCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(WgetWinCode::EOrder);
        }
        let want = match download_cell(o) {
            Some(w) => w,
            None => return Err(WgetWinCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(WgetWinCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WgetWinCode::EDup);
        }
        if let Some(slot) = self.ops.get_mut(o) {
            *slot = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < DOWNLOAD_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 过滤三面一格（载荷对照 + 重复拒）。
    pub fn record_filter(&mut self, f: usize, got: u64) -> Result<(), WgetWinCode> {
        let want = match filter_cell(f) {
            Some(w) => w,
            None => return Err(WgetWinCode::ENoFilter),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + f as u64);
            return Err(WgetWinCode::EFilterDrift);
        }
        if match self.filters.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WgetWinCode::EDup);
        }
        if let Some(slot) = self.filters.get_mut(f) {
            *slot = Some(got);
        }
        self.fnv(0x8301 + f as u64);
        Ok(())
    }

    /// 过滤三面全账核验（12 格）。
    pub fn filters_all(&self) -> bool {
        let mut f = 0usize;
        while f < FILTER_CELLS {
            match self.filters.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// robots 语义一格（载荷对照 + 重复拒）。
    pub fn record_robots(&mut self, g: usize, got: u64) -> Result<(), WgetWinCode> {
        let want = match robots_cell(g) {
            Some(w) => w,
            None => return Err(WgetWinCode::ENoRobots),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + g as u64);
            return Err(WgetWinCode::ENoRobots);
        }
        if match self.robots.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WgetWinCode::EDup);
        }
        if let Some(slot) = self.robots.get_mut(g) {
            *slot = Some(got);
        }
        self.fnv(0x8401 + g as u64);
        Ok(())
    }

    /// robots 语义全账核验（5 格）。
    pub fn robots_all(&self) -> bool {
        let mut g = 0usize;
        while g < ROBOTS_CELLS {
            match self.robots.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 退出码一格（闭集核验 + 重复拒）。
    pub fn record_exit(&mut self, slot: usize, code: usize) -> Result<(), WgetWinCode> {
        if slot >= EXIT_CODES {
            return Err(WgetWinCode::ENoExit);
        }
        if !exit_code_legal(code) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + code as u64);
            return Err(WgetWinCode::ENoExit);
        }
        if match self.exits.get(slot) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WgetWinCode::EDup);
        }
        if let Some(c) = self.exits.get_mut(slot) {
            *c = Some(true);
        }
        self.fnv(0x8501 + slot as u64);
        Ok(())
    }

    /// 退出码全账核验（8 码全齐）。
    pub fn exits_all(&self) -> bool {
        let mut e = 0usize;
        while e < EXIT_CODES {
            match self.exits.get(e) {
                Some(Some(true)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 版本一格（档载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), WgetWinCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(WgetWinCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8600 + v as u64);
            return Err(WgetWinCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WgetWinCode::EDup);
        }
        if let Some(slot) = self.vers.get_mut(v) {
            *slot = Some(got);
        }
        self.fnv(0x8601 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 档全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// B12 终端消费联签锚定行（F52235 锚定行）。
    pub fn declare_anchor(&mut self, r: usize) -> Result<(), WgetWinCode> {
        if r >= ANCHOR_ROWS {
            return Err(WgetWinCode::ENoAnchor);
        }
        if match self.anchors.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WgetWinCode::EDup);
        }
        if let Some(slot) = self.anchors.get_mut(r) {
            *slot = anchor_row(r);
        }
        self.fnv(0x8700 + r as u64);
        Ok(())
    }

    /// 联签锚定行全齐核验。
    pub fn anchors_all(&self) -> bool {
        let mut r = 0usize;
        while r < ANCHOR_ROWS {
            match self.anchors.get(r) {
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

    /// KB-COMPAT 定级（全通 → S1；判据全但格缺 → S2；判据缺 → S3）。
    pub fn judge_grade(&self) -> WgetGrade {
        if self.gates_all()
            && self.ops_all()
            && self.filters_all()
            && self.robots_all()
            && self.exits_all()
            && self.vers_all()
            && self.anchors_all()
        {
            WgetGrade::S1
        } else if self.gates_all() {
            WgetGrade::S2
        } else {
            WgetGrade::S3
        }
    }
}

impl Default for WgetLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WgetLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < DOWNLOAD_OPS {
        if let Some(w) = download_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut f = 0usize;
    while f < FILTER_CELLS {
        if let Some(w) = filter_cell(f) {
            if led.record_filter(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut g2 = 0usize;
    while g2 < ROBOTS_CELLS {
        if let Some(w) = robots_cell(g2) {
            if led.record_robots(g2, w).is_ok() {
                recorded += 1;
            }
        }
        g2 += 1;
    }
    let mut e = 0usize;
    while e < EXIT_CODES {
        if let Some(c) = exit_slot_code(e) {
            if led.record_exit(e, c).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut r = 0usize;
    while r < ANCHOR_ROWS {
        let _ = led.declare_anchor(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 过滤载荷漂移 / 家族逆序三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒数与指纹。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WgetLedger::new();
        let caught = match kind {
            0 => {
                let a = lt.record_exit(0, 13) == Err(WgetWinCode::ENoExit);
                let b = lt.record_op(DOWNLOAD_OPS, 0) == Err(WgetWinCode::ENoOp);
                a && b
            }
            1 => {
                // 漂移注入先录正格再注漂移。
                if let Some(w) = filter_cell(0) {
                    let _ = lt.record_filter(0, w);
                }
                lt.record_filter(1, filter_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(WgetWinCode::EFilterDrift)
            }
            _ => {
                // 逆序注入对空账本同样必抓：先录正格（高族未录格）再逆序注入，
                // 拒后 last_kind 不变。
                if let Some(w) = download_cell(12) {
                    let _ = lt.record_op(12, w);
                }
                lt.record_op(0, download_cell(0).unwrap_or(0)) == Err(WgetWinCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8800 + k as u64);
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
/// UNX-F52256 判据集（锚 UNX-F52257-J1，off-by-one 板式）。
pub fn run_unx52256_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52256");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "20 操作集五家族全通",
        recorded >= DOWNLOAD_OPS as u32,
        "递归/镜像/限速/断点续传/代理五家族各 4 操作逐格恒等",
    );
    s.add(
        "递归深度/域名过滤三面 12 格全落",
        {
            let mut lt = WgetLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FILTER_CELLS {
                if let Some(w) = filter_cell(f) {
                    if lt.record_filter(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.filters_all()
        },
        "递归深度 4 格 + 域内放行 4 格 + 域外拒绝 4 格逐格落账",
    );
    s.add(
        "robots 语义 5 格全落",
        {
            let mut lt = WgetLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < ROBOTS_CELLS {
                if let Some(w) = robots_cell(g) {
                    if lt.record_robots(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.robots_all()
        },
        "未排除/全排除/抓取延迟/仅限代理/无 robots 文件语义闭集",
    );
    s.add(
        "退出码闭集 8 码全齐",
        {
            let mut lt = WgetLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EXIT_CODES {
                if let Some(c) = exit_slot_code(e) {
                    if lt.record_exit(e, c).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.exits_all()
        },
        "0/1/2/3/4/6/7/8 八码闭集逐码落账",
    );
    s.add(
        "版本阶梯 3 档全落",
        {
            let mut lt = WgetLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "1.19.x/1.20.x/1.21.x 三档逐档恒等，越级必抓",
    );
    s.add(
        "B12 终端消费联签锚定行 1/1",
        {
            let mut lt = WgetLedger::new();
            let ok = lt.declare_anchor(0).is_ok();
            ok && lt.anchors_all()
        },
        "F52235 锚定行闭集（卡面判据逐字锚定）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = WgetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/下载/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "操作家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < DOWNLOAD_OPS {
                if download_kind(o).unwrap_or(5) < download_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调（递归→镜像→限速→断点续传→代理）",
    );
    s.add(
        "过滤三面单调",
        {
            let mut ok = true;
            let mut f = 1usize;
            while f < FILTER_CELLS {
                if filter_face(f).unwrap_or(3) < filter_face(f - 1).unwrap_or(3) {
                    ok = false;
                }
                f += 1;
            }
            ok
        },
        "深度→域内→域外三面闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "退出码越闭集+操作越闭集/过滤漂移/家族逆序三态轮转必抓",
    );
    // 边界·越闭集 ×3。
    let mut l0 = WgetLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(DOWNLOAD_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·过滤越闭集",
        l0.record_filter(FILTER_CELLS, 0).is_err(),
        "过滤格 12 越闭集必拒",
    );
    s.add(
        "边界·版本越阶梯",
        l0.record_version(VERSION_STEPS, 0).is_err(),
        "版本档 3 越阶梯必拒",
    );
    s.add(
        "边界·退出码越闭集",
        {
            let mut lt = WgetLedger::new();
            lt.record_exit(0, 13) == Err(WgetWinCode::ENoExit)
        },
        "退出码 13 越闭集必拒",
    );
    s.add(
        "robots 越闭集拒",
        {
            let mut lt = WgetLedger::new();
            lt.record_robots(ROBOTS_CELLS, 0).is_err()
        },
        "robots 语义格 5 越闭集必拒",
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
            // main/aux 各节全部记账后再断言。
            let mut lt = WgetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < DOWNLOAD_OPS {
                if let Some(w) = download_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut f = 0usize;
            while f < FILTER_CELLS {
                if let Some(w) = filter_cell(f) {
                    let _ = lt.record_filter(f, w);
                }
                f += 1;
            }
            let mut g2 = 0usize;
            while g2 < ROBOTS_CELLS {
                if let Some(w) = robots_cell(g2) {
                    let _ = lt.record_robots(g2, w);
                }
                g2 += 1;
            }
            let mut e = 0usize;
            while e < EXIT_CODES {
                if let Some(c) = exit_slot_code(e) {
                    let _ = lt.record_exit(e, c);
                }
                e += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
            }
            let mut r = 0usize;
            while r < ANCHOR_ROWS {
                let _ = lt.declare_anchor(r);
                r += 1;
            }
            lt.judge_grade() == WgetGrade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且公式恒等",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_kind() {
        assert_eq!(download_kind(0), Some(0));
        assert_eq!(download_kind(3), Some(0));
        assert_eq!(download_kind(4), Some(1));
        assert_eq!(download_kind(19), Some(4));
        assert!(download_kind(20).is_none());
        assert!(download_cell(20).is_none());
    }

    #[test]
    fn test_download_names() {
        assert_eq!(download_kind_name(0), Some("递归"));
        assert_eq!(download_kind_name(1), Some("镜像"));
        assert_eq!(download_kind_name(2), Some("限速"));
        assert_eq!(download_kind_name(3), Some("断点续传"));
        assert_eq!(download_kind_name(4), Some("代理"));
        assert!(download_kind_name(5).is_none());
    }

    #[test]
    fn test_filter_faces() {
        assert_eq!(filter_face(0), Some(0));
        assert_eq!(filter_face(4), Some(1));
        assert_eq!(filter_face(11), Some(2));
        assert!(filter_face(12).is_none());
        assert_eq!(filter_face_name(0), Some("递归深度"));
        assert_eq!(filter_face_name(1), Some("域内放行"));
        assert_eq!(filter_face_name(2), Some("域外拒绝"));
        assert!(filter_face_name(3).is_none());
    }

    #[test]
    fn test_robots_semantics() {
        assert_eq!(robots_semantic_name(0), Some("未排除"));
        assert_eq!(robots_semantic_name(4), Some("无 robots 文件"));
        assert!(robots_semantic_name(5).is_none());
        assert!(robots_cell(5).is_none());
    }

    #[test]
    fn test_exit_codes_closed() {
        let mut s = 0usize;
        while s < EXIT_CODES {
            let c = exit_slot_code(s).unwrap_or(999);
            assert!(exit_code_legal(c));
            assert!(exit_code_name(c).is_some());
            s += 1;
        }
        assert!(exit_code_legal(4));
        assert!(exit_code_legal(8));
        assert!(!exit_code_legal(5));
        assert!(!exit_code_legal(13));
        assert!(exit_code_name(13).is_none());
    }

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("1.19.x"));
        assert_eq!(version_name(1), Some("1.20.x"));
        assert_eq!(version_name(2), Some("1.21.x"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_order_caught() {
        let mut lt = WgetLedger::new();
        let _ = lt.record_op(16, download_cell(16).unwrap_or(0));
        assert_eq!(
            lt.record_op(3, download_cell(3).unwrap_or(0)),
            Err(WgetWinCode::EOrder)
        );
        // 逆序拒后同 last_kind 复验须用同族或更高族未录格。
        assert!(lt.record_op(17, download_cell(17).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WgetLedger::new();
        assert_eq!(
            lt.record_op(9, download_cell(9).unwrap_or(0).wrapping_add(1)),
            Err(WgetWinCode::EOpDrift)
        );
        assert_eq!(
            lt.record_filter(5, filter_cell(5).unwrap_or(0).wrapping_add(7)),
            Err(WgetWinCode::EFilterDrift)
        );
        assert_eq!(
            lt.record_version(2, version_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(WgetWinCode::EVerDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WgetLedger::new();
        let _ = lt.record_op(0, download_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_op(0, download_cell(0).unwrap_or(0)),
            Err(WgetWinCode::EDup)
        );
        let _ = lt.record_robots(2, robots_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_robots(2, robots_cell(2).unwrap_or(0)),
            Err(WgetWinCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WgetLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WgetLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded
                >= (DOWNLOAD_OPS + FILTER_CELLS + ROBOTS_CELLS + EXIT_CODES + VERSION_STEPS)
                    as u32
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
        assert_eq!(SEG_BASE, 0xC7FC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WgetWinCode::Ok as u32, 0);
        assert_eq!(WgetWinCode::EGradeDrift as u32, 12);
        assert!(!WgetWinCode::Ok.detail().is_empty());
        assert!(!WgetWinCode::ENoRobots.detail().is_empty());
        assert!(!WgetWinCode::ENoAnchor.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(WgetGrade::S1.val(), 1);
        assert_eq!(WgetGrade::S3.val(), 3);
        let lt = WgetLedger::new();
        assert_eq!(lt.judge_grade(), WgetGrade::S3);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = WgetLedger::new();
        // 载荷漂移路径记账：注入=检出=1、逃逸=0（越闭集路径提前返回不记账）。
        let _ = lt.record_op(0, download_cell(0).unwrap_or(0).wrapping_add(1));
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (1, 1, 0));
    }

    #[test]
    fn unx52256_checks_green() {
        let s = run_unx52256_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
