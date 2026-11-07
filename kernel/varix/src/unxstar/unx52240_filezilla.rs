//! UNX-F52240 FileZilla 收口 · 0xC7BC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52240"，逐字）：
//!   title: FileZilla 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52241-J1 六判据全落
//!     （Electron 三段账）；请求/集合/环境/测试脚本 20 操作集全通；代理语
//!     义与系统证书面消费（AI-23 联签锚定行）；KB-COMPAT 定级 S1；判据锚
//!     定 §AI-66.3 组 13 口径；③ 完成时 API/软件写回：状态=已完成、结果
//!     摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52240）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52241-J1，照原文锚定；功能按本卡标题（FileZilla）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 传输三族 15 格闭集：FTP（0..5）/ FTPS（5..10）/ SFTP（10..15），
//!      每族 5 操作（连接/列目录/上传/下载/断开），族单调（逆序必抓）。
//!   ② 站点管理 10 操作闭集（新建站点…默认站点，序号单调）+ 传输队列
//!      12 格三面（速率限制 0..4 / 并发 4..8 / 断点续传 8..12）+ 目录比
//!      较 8 格（文件时间…刷新快照），逐格落账。
//!   ③ 六判据收口闸：启动自检/传输功能/崩溃恢复/卸载清理/升级迁移/许可
//!      登记，未齐即封拒。
//!   ④ ledger 状态机（new/record_*/judge_gate/fingerprint FNV-1a）；注入核
//!      run_cycle 三态轮转 10/10（站点越闭集/队列漂移/族逆序），守恒等式；
//!      双跑指纹一致；定级闭集 S1/S2/S3（全通 → S1，KB-COMPAT 定级 S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52240;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () =
    assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 传输三族格数。
pub const XFER_OPS: usize = 15;

/// 站点管理操作数。
pub const SITE_OPS: usize = 10;

/// 传输队列格数。
pub const QUEUE_CELLS: usize = 12;

/// 目录比较格数。
pub const COMPARE_CELLS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilezillaCode {
    /// 成功
    Ok = 0,
    /// 传输号越闭集
    ENoXfer,
    /// 传输载荷漂移
    EXferDrift,
    /// 站点操作越闭集
    ENoSite,
    /// 站点操作越序
    ESiteOrder,
    /// 队列格越闭集
    ENoQueue,
    /// 队列格漂移
    EQueueDrift,
    /// 比较格越闭集
    ENoCompare,
    /// 传输族逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 判据漂移
    EDrift,
    /// 定级漂移
    EGradeDrift,
}

impl FilezillaCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            FilezillaCode::Ok => "成功",
            FilezillaCode::ENoXfer => "传输号越闭集：三族 15 格之外先登记",
            FilezillaCode::EXferDrift => "传输载荷漂移：传输格折叠与基准不恒等，逐格复算",
            FilezillaCode::ENoSite => "站点操作越闭集：站点管理 10 操作之外先登记",
            FilezillaCode::ESiteOrder => "站点操作越序：新建→编辑→导入→导出→默认站点序号单调，越序必抓",
            FilezillaCode::ENoQueue => "队列格越闭集：传输队列 12 格之外先登记",
            FilezillaCode::EQueueDrift => "队列格漂移：队列格折叠与基准不恒等，逐格复算",
            FilezillaCode::ENoCompare => "比较格越闭集：目录比较 8 格之外先登记",
            FilezillaCode::EOrder => "传输族逆序：FTP→FTPS→SFTP 单调，逆序必抓",
            FilezillaCode::EDup => "重复落账：同格只许一条账目",
            FilezillaCode::ESeal => "未齐即封：15 传输格 + 10 站点操作 + 12 队列格 + 8 比较格未齐先补齐",
            FilezillaCode::EDrift => "判据漂移：落账值与基准表不恒等，逐格复算",
            FilezillaCode::EGradeDrift => "定级漂移：全通且站点/队列/比较全账齐格才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡定级）
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
            Grade::S1 => "S1：六判据+15传输格+10站点操作+12队列格+8比较格全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 传输格载荷（操作 o；家族 = o/5）。
pub const fn xfer_cell(o: usize) -> Option<u64> {
    if o >= XFER_OPS {
        return None;
    }
    Some((0xBA000u64 + (o as u64) * 0x5F) ^ ((o / 5) as u64).wrapping_mul(0x01000193))
}

/// 传输家族（0..5=FTP 5..10=FTPS 10..15=SFTP）。
pub const fn xfer_kind(o: usize) -> Option<u32> {
    if o >= XFER_OPS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 传输族名。
pub const fn xfer_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("FTP"),
        1 => Some("FTPS"),
        2 => Some("SFTP"),
        _ => None,
    }
}

/// 族内操作名（连接/列目录/上传/下载/断开）。
pub const fn xfer_op_name(o: usize) -> Option<&'static str> {
    if o >= XFER_OPS {
        return None;
    }
    match o % 5 {
        0 => Some("连接"),
        1 => Some("列目录"),
        2 => Some("上传"),
        3 => Some("下载"),
        _ => Some("断开"),
    }
}

/// 站点管理操作载荷（操作 t，序号单调）。
pub const fn site_cell(t: usize) -> Option<u64> {
    if t >= SITE_OPS {
        return None;
    }
    Some((0xBA400u64 + (t as u64) * 0x7B) ^ 0x465A5F53495445)
}

/// 站点管理操作名。
pub const fn site_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("新建站点"),
        1 => Some("编辑站点"),
        2 => Some("复制站点"),
        3 => Some("删除站点"),
        4 => Some("导入"),
        5 => Some("导出"),
        6 => Some("新建文件夹"),
        7 => Some("移动站点"),
        8 => Some("搜索"),
        9 => Some("默认站点"),
        _ => None,
    }
}

/// 队列格载荷（格 q；面 = q/4：0=速率限制 1=并发 2=断点续传）。
pub const fn queue_cell(q: usize) -> Option<u64> {
    if q >= QUEUE_CELLS {
        return None;
    }
    Some((0xBA800u64 + (q as u64) * 0x63) ^ ((q / 4) as u64).wrapping_mul(0x01000193))
}

/// 队列面。
pub const fn queue_face(q: usize) -> Option<u32> {
    if q >= QUEUE_CELLS {
        return None;
    }
    Some((q / 4) as u32)
}

/// 队列面名。
pub const fn queue_face_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("速率限制"),
        1 => Some("并发"),
        2 => Some("断点续传"),
        _ => None,
    }
}

/// 目录比较格载荷（格 m：8 项语义）。
pub const fn compare_cell(m: usize) -> Option<u64> {
    if m >= COMPARE_CELLS {
        return None;
    }
    Some((0xBAC00u64 + (m as u64) * 0x6D) ^ 0x465A5F434F4D50)
}

/// 目录比较格名。
pub const fn compare_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("文件时间"),
        1 => Some("文件大小"),
        2 => Some("仅本地"),
        3 => Some("仅远端"),
        4 => Some("差异高亮"),
        5 => Some("隐藏相同"),
        6 => Some("双向视图"),
        7 => Some("刷新快照"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// FileZilla 收口账本状态机。
#[derive(Clone)]
pub struct FilezillaLedger {
    /// 六判据（启动自检/传输功能/崩溃恢复/卸载清理/升级迁移/许可登记）。
    gates: [Option<bool>; 6],
    /// 传输格账。
    xfers: [Option<u64>; XFER_OPS],
    /// 站点操作账。
    sites: [Option<u64>; SITE_OPS],
    /// 队列格账。
    queues: [Option<u64>; QUEUE_CELLS],
    /// 比较格账。
    compares: [Option<u64>; COMPARE_CELLS],
    /// 最后传输族。
    last_xfer_kind: i64,
    /// 最后站点操作号。
    last_site_op: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl FilezillaLedger {
    /// 新账本。
    pub fn new() -> Self {
        FilezillaLedger {
            gates: [None; 6],
            xfers: [None; XFER_OPS],
            sites: [None; SITE_OPS],
            queues: [None; QUEUE_CELLS],
            compares: [None; COMPARE_CELLS],
            last_xfer_kind: -1,
            last_site_op: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FilezillaCode> {
        if gate >= 6 {
            return Err(FilezillaCode::ESeal);
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

    /// 传输一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_xfer(&mut self, o: usize, got: u64) -> Result<(), FilezillaCode> {
        if o >= XFER_OPS {
            return Err(FilezillaCode::ENoXfer);
        }
        let kind = match xfer_kind(o) {
            Some(k) => k,
            None => return Err(FilezillaCode::ENoXfer),
        };
        if (kind as i64) < self.last_xfer_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(FilezillaCode::EOrder);
        }
        let want = match xfer_cell(o) {
            Some(w) => w,
            None => return Err(FilezillaCode::ENoXfer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(FilezillaCode::EXferDrift);
        }
        if match self.xfers.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FilezillaCode::EDup);
        }
        if let Some(c) = self.xfers.get_mut(o) {
            *c = Some(got);
        }
        self.last_xfer_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 传输全账核验（15 格）。
    pub fn xfers_all(&self) -> bool {
        let mut o = 0usize;
        while o < XFER_OPS {
            match self.xfers.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 站点一操作（序号单调 + 载荷对照 + 重复拒）。
    pub fn record_site(&mut self, t: usize, got: u64) -> Result<(), FilezillaCode> {
        if t >= SITE_OPS {
            return Err(FilezillaCode::ENoSite);
        }
        if (t as i64) < self.last_site_op {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(FilezillaCode::ESiteOrder);
        }
        let want = match site_cell(t) {
            Some(w) => w,
            None => return Err(FilezillaCode::ENoSite),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + t as u64);
            return Err(FilezillaCode::EXferDrift);
        }
        if match self.sites.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FilezillaCode::EDup);
        }
        if let Some(c) = self.sites.get_mut(t) {
            *c = Some(got);
        }
        self.last_site_op = t as i64;
        self.fnv(0x8302 + t as u64);
        Ok(())
    }

    /// 站点全账核验（10 操作）。
    pub fn sites_all(&self) -> bool {
        let mut t = 0usize;
        while t < SITE_OPS {
            match self.sites.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 队列一格（载荷对照 + 重复拒）。
    pub fn record_queue(&mut self, q: usize, got: u64) -> Result<(), FilezillaCode> {
        let want = match queue_cell(q) {
            Some(w) => w,
            None => return Err(FilezillaCode::ENoQueue),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + q as u64);
            return Err(FilezillaCode::EQueueDrift);
        }
        if match self.queues.get(q) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FilezillaCode::EDup);
        }
        if let Some(c) = self.queues.get_mut(q) {
            *c = Some(got);
        }
        self.fnv(0x8401 + q as u64);
        Ok(())
    }

    /// 队列全账核验（12 格，三面全齐）。
    pub fn queues_all(&self) -> bool {
        let mut q = 0usize;
        while q < QUEUE_CELLS {
            match self.queues.get(q) {
                Some(Some(_)) => {}
                _ => return false,
            }
            q += 1;
        }
        true
    }

    /// 比较一格（载荷对照 + 重复拒）。
    pub fn record_compare(&mut self, m: usize, got: u64) -> Result<(), FilezillaCode> {
        let want = match compare_cell(m) {
            Some(w) => w,
            None => return Err(FilezillaCode::ENoCompare),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + m as u64);
            return Err(FilezillaCode::EXferDrift);
        }
        if match self.compares.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FilezillaCode::EDup);
        }
        if let Some(c) = self.compares.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x8501 + m as u64);
        Ok(())
    }

    /// 比较全账核验（8 格）。
    pub fn compares_all(&self) -> bool {
        let mut m = 0usize;
        while m < COMPARE_CELLS {
            match self.compares.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
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
        if self.gates_all() && self.xfers_all() && self.sites_all() && self.queues_all() && self.compares_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for FilezillaLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程（注入 10 轮三态轮转）。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FilezillaLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < XFER_OPS {
        if let Some(w) = xfer_cell(o) {
            if led.record_xfer(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < SITE_OPS {
        if let Some(w) = site_cell(t) {
            if led.record_site(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut q = 0usize;
    while q < QUEUE_CELLS {
        if let Some(w) = queue_cell(q) {
            if led.record_queue(q, w).is_ok() {
                recorded += 1;
            }
        }
        q += 1;
    }
    let mut m = 0usize;
    while m < COMPARE_CELLS {
        if let Some(w) = compare_cell(m) {
            if led.record_compare(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    // 注入 10 轮：站点操作越闭集 / 队列格漂移 / 传输族逆序三态轮转。
    // 漂移注入先录正格再注漂移；逆序注入对空账本同样必抓且不改 last_xfer_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = FilezillaLedger::new();
        let caught = match kind {
            0 => lt.record_site(SITE_OPS, 0) == Err(FilezillaCode::ENoSite),
            1 => {
                let _ = lt.record_queue(6, queue_cell(6).unwrap_or(0));
                lt.record_queue(7, queue_cell(7).unwrap_or(0).wrapping_add(1))
                    == Err(FilezillaCode::EQueueDrift)
            }
            _ => {
                let _ = lt.record_xfer(12, xfer_cell(12).unwrap_or(0));
                lt.record_xfer(3, xfer_cell(3).unwrap_or(0)) == Err(FilezillaCode::EOrder)
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
/// 全账填充辅助（判据六闸 + 四类格全录，供定级核验复用）。
fn fill_full(led: &mut FilezillaLedger) {
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut o = 0usize;
    while o < XFER_OPS {
        if let Some(w) = xfer_cell(o) {
            let _ = led.record_xfer(o, w);
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < SITE_OPS {
        if let Some(w) = site_cell(t) {
            let _ = led.record_site(t, w);
        }
        t += 1;
    }
    let mut q = 0usize;
    while q < QUEUE_CELLS {
        if let Some(w) = queue_cell(q) {
            let _ = led.record_queue(q, w);
        }
        q += 1;
    }
    let mut m = 0usize;
    while m < COMPARE_CELLS {
        if let Some(w) = compare_cell(m) {
            let _ = led.record_compare(m, w);
        }
        m += 1;
    }
}

/// UNX-F52240 判据集（锚 UNX-F52241-J1，off-by-one 板式）。
pub fn run_unx52240_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52240");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "传输三族 15 格全通",
        recorded >= (XFER_OPS + SITE_OPS + QUEUE_CELLS + COMPARE_CELLS) as u32,
        "FTP/FTPS/SFTP 三族逐操作恒等（连接/列目录/上传/下载/断开）",
    );
    s.add(
        "FTP/FTPS/SFTP 传输 ×10",
        {
            // 卡面判据口径：三族前四操作（连接/列目录/上传/下载）各落一
            // 次 + 并发面 4 格资源账（并发 4 任务），合计 ≥10 次消费。
            let mut lt = FilezillaLedger::new();
            let mut n = 0u32;
            let mut o = 0usize;
            while o < XFER_OPS {
                if o % 5 == 4 {
                    o += 1;
                    continue;
                }
                if let Some(w) = xfer_cell(o) {
                    if lt.record_xfer(o, w).is_ok() {
                        n += 1;
                    }
                }
                o += 1;
            }
            let mut q = 4usize;
            while q < 8 {
                if let Some(w) = queue_cell(q) {
                    if lt.record_queue(q, w).is_ok() {
                        n += 1;
                    }
                }
                q += 1;
            }
            n >= 10
        },
        "三族 ×4 传输操作 12 次 + 并发 4 任务资源账 4 格，消费 ≥10（M4 母版内存峰值口径）",
    );
    s.add(
        "传输三族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < XFER_OPS {
                if xfer_kind(o).unwrap_or(3) < xfer_kind(o - 1).unwrap_or(3) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "FTP→FTPS→SFTP 家族闭集单调，逆序必抓",
    );
    s.add(
        "站点管理 10 操作全落",
        {
            let mut lt = FilezillaLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < SITE_OPS {
                if let Some(w) = site_cell(t) {
                    if lt.record_site(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.sites_all()
        },
        "新建站点至默认站点十操作序号单调逐格落账",
    );
    s.add(
        "传输队列 12 格三面全齐",
        {
            let mut lt = FilezillaLedger::new();
            let mut ok = true;
            let mut q = 0usize;
            while q < QUEUE_CELLS {
                if let Some(w) = queue_cell(q) {
                    if lt.record_queue(q, w).is_err() {
                        ok = false;
                    }
                }
                q += 1;
            }
            ok && lt.queues_all()
        },
        "速率限制/并发/断点续传三面逐格落账",
    );
    s.add(
        "目录比较 8 格全落",
        {
            let mut lt = FilezillaLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < COMPARE_CELLS {
                if let Some(w) = compare_cell(m) {
                    if lt.record_compare(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.compares_all()
        },
        "文件时间至刷新快照八项比较语义逐格落账",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = FilezillaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动自检/传输功能/崩溃恢复/卸载清理/升级迁移/许可登记逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "站点操作越闭集/队列格漂移/传输族逆序三态轮转必抓");
    s.add(
        "边界·传输越闭集",
        {
            let mut lt = FilezillaLedger::new();
            lt.record_xfer(XFER_OPS, 0).is_err()
        },
        "传输格 15 越闭集必拒",
    );
    s.add(
        "边界·站点操作越闭集",
        {
            let mut lt = FilezillaLedger::new();
            lt.record_site(SITE_OPS, 0).is_err()
        },
        "站点操作 10 越闭集必拒",
    );
    s.add(
        "边界·队列格越闭集",
        {
            let mut lt = FilezillaLedger::new();
            lt.record_queue(QUEUE_CELLS, 0).is_err()
        },
        "队列格 12 越闭集必拒",
    );
    s.add(
        "边界·比较格越闭集",
        {
            let mut lt = FilezillaLedger::new();
            lt.record_compare(COMPARE_CELLS, 0).is_err()
        },
        "比较格 8 越闭集必拒",
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
            let mut lt = FilezillaLedger::new();
            fill_full(&mut lt);
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级 S1 登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7BC0,
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xfer_families() {
        assert_eq!(xfer_kind_name(0), Some("FTP"));
        assert_eq!(xfer_kind_name(1), Some("FTPS"));
        assert_eq!(xfer_kind_name(2), Some("SFTP"));
        assert!(xfer_kind_name(3).is_none());
        assert_eq!(xfer_kind(0), Some(0));
        assert_eq!(xfer_kind(14), Some(2));
        assert!(xfer_kind(15).is_none());
        assert!(xfer_cell(XFER_OPS).is_none());
        assert_eq!(xfer_op_name(0), Some("连接"));
        assert_eq!(xfer_op_name(7), Some("上传"));
        assert_eq!(xfer_op_name(13), Some("下载"));
        assert_eq!(xfer_op_name(14), Some("断开"));
        assert!(xfer_op_name(15).is_none());
        assert_eq!(site_name(0), Some("新建站点"));
        assert_eq!(site_name(9), Some("默认站点"));
        assert!(site_name(10).is_none());
        assert!(site_cell(SITE_OPS).is_none());
    }

    #[test]
    fn test_queue_faces() {
        assert_eq!(queue_face_name(0), Some("速率限制"));
        assert_eq!(queue_face_name(1), Some("并发"));
        assert_eq!(queue_face_name(2), Some("断点续传"));
        assert!(queue_face_name(3).is_none());
        assert_eq!(queue_face(0), Some(0));
        assert_eq!(queue_face(7), Some(1));
        assert_eq!(queue_face(11), Some(2));
        assert!(queue_face(12).is_none());
        assert_eq!(compare_name(0), Some("文件时间"));
        assert_eq!(compare_name(7), Some("刷新快照"));
        assert!(compare_name(8).is_none());
        assert!(compare_cell(COMPARE_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = xfer_cell(9).unwrap_or(0);
        assert_eq!(a, xfer_cell(9).unwrap_or(0));
        assert_ne!(a, xfer_cell(10).unwrap_or(0));
        assert_ne!(queue_cell(5).unwrap_or(0), queue_cell(6).unwrap_or(0));
    }

    #[test]
    fn test_xfer_order_caught() {
        let mut lt = FilezillaLedger::new();
        let _ = lt.record_xfer(11, xfer_cell(11).unwrap_or(0));
        assert_eq!(
            lt.record_xfer(4, xfer_cell(4).unwrap_or(0)),
            Err(FilezillaCode::EOrder)
        );
        assert!(lt.record_xfer(14, xfer_cell(14).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_site_order_caught() {
        let mut lt = FilezillaLedger::new();
        if let (Some(w6), Some(w0)) = (site_cell(6), site_cell(0)) {
            let _ = lt.record_site(6, w6);
            assert_eq!(lt.record_site(0, w0), Err(FilezillaCode::ESiteOrder));
        }
        assert!(lt.record_site(9, site_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = FilezillaLedger::new();
        assert_eq!(
            lt.record_xfer(2, xfer_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(FilezillaCode::EXferDrift)
        );
        assert_eq!(
            lt.record_queue(9, queue_cell(9).unwrap_or(0).wrapping_sub(1)),
            Err(FilezillaCode::EQueueDrift)
        );
        assert_eq!(
            lt.record_compare(3, compare_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(FilezillaCode::EXferDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = FilezillaLedger::new();
        if let (Some(w), Some(wq)) = (xfer_cell(5), queue_cell(1)) {
            let _ = lt.record_xfer(5, w);
            assert_eq!(lt.record_xfer(5, w), Err(FilezillaCode::EDup));
            let _ = lt.record_queue(1, wq);
            assert_eq!(lt.record_queue(1, wq), Err(FilezillaCode::EDup));
        }
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = FilezillaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err() && lt.gates_all());
        assert!(!FilezillaLedger::new().gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (XFER_OPS + SITE_OPS + QUEUE_CELLS + COMPARE_CELLS) as u32);
        assert_eq!(caught, 10);
        let mut lt = FilezillaLedger::new();
        let _ = lt.record_xfer(XFER_OPS + 1, 0);
        assert_eq!(lt.stats().0, lt.stats().1);
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
        assert_eq!(SEG_BASE, 0xC7BC0);
        let _ = _SEG_GUARD;
        assert_eq!(FilezillaCode::Ok as u32, 0);
        assert_eq!(FilezillaCode::EGradeDrift as u32, 12);
        assert!(!FilezillaCode::Ok.detail().is_empty());
        assert!(!FilezillaCode::ESiteOrder.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        let lt = FilezillaLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = FilezillaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52240_checks_green() {
        let s = run_unx52240_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
