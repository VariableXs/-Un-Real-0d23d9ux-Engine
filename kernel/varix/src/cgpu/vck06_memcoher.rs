//! vck06_memcoher — CGPU-F1606 异构内存一致性（位置标签·显式原则·边界纪律·USM 双路）。
//!
//! 判据原文（施工书锚点 CGPU-F1606）：一致性：内存模型（引擎私有内存+共
//! 享内存两级——每缓冲区位置标签（GPU/NPU/Shared）——位置标签规范；一致
//! 性级别（宽松（引擎内）/显式同步（跨引擎）——两级模型（禁隐式共享（隐
//! 式跨引擎访问=缺陷——显式原则；缓存一致性（CPU 缓存×设备缓存（cache
//! flush/sync 点显式化——sync 点 API（flush 时机规范（交接边界必 sync——
//! 边界纪律；USM 评估（统一共享内存（平台支持矩阵（Intel/AMD——支持则零
//! 拷贝路径——USM 路径与拷贝路径双实现；一致性行为测试（写读序测试（跨
//! 引擎写后读一致——序测试矩阵（宽松/显式/USM 三路——三路对拍；调试（内
//! 存位置可视化+违规访问检测（位置违规即诊断——违规检测器；测试（两级模
//! 型/边界 sync/USM 双路/三路对拍/违规检测五组）。判据：位置标签、显式原
//! 则、边界纪律、USM 双路、三路对拍、违规检测、五组、判据。
//!
//! **职责定位**：两级内存模型（引擎私有 Gpu/Npu × 共享 Shared 三态位置标
//! 签——每缓冲区出生即挂牌，位置是事实不是猜测）+一致性级别两级（宽松=
//! 引擎内自由访问；显式同步=跨引擎必须踩 sync 点——**禁隐式**：无 sync 的
//! 跨引擎访问即违规诊断，显式原则是铁律）+sync 点 API（flush 时机规范：
//! 交接边界必 sync——边界纪律落地为 flush_at 的边界参数强制）+USM 平台支
//! 持矩阵（Intel 全支持/AMD 部分/缺省不支持——支持则零拷贝 USM 路径，否
//! 则拷贝路径——双实现裁决与 vck05 交接路径表同口径）+写读序三路对拍（宽
//! 松/显式/USM 三种访问模式各跑一遍写→sync→读序账，结果必须一致——一致
//! 性行为回归面）+违规检测器（位置违规+一致性违规双查——显性诊断不静默
//! 透支）。诊断码段 0x56xx 四码。前置 vck05（fence/交接面）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const K06_MEMCOHER_VERSION: &str = "K06-memcoher-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x56xx 段独占——四码互异）
// ---------------------------------------------------------------------------

/// 诊断码（0x56xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum McCode {
    /// 0x5601 位置违规（引擎 X 直接访问他引擎私有缓冲——位置标签铁律）。
    McLocationViolation,
    /// 0x5602 一致性违规（跨引擎访问 Shared 缓冲无 sync——隐式访问即缺陷）。
    McImplicitAccess,
    /// 0x5603 边界纪律违例（交接边界未 flush——边界必 sync 被跳过）。
    McBoundaryMissed,
    /// 0x5604 序对拍失配（三路写读序结果不一致——一致性回归面破）。
    McOrderMismatch,
}

impl McCode {
    pub const ALL: [McCode; 4] = [
        McCode::McLocationViolation,
        McCode::McImplicitAccess,
        McCode::McBoundaryMissed,
        McCode::McOrderMismatch,
    ];

    /// 人读诊断（发生了什么/为什么/怎么办）。
    pub const fn detail(self) -> &'static str {
        match self {
            McCode::McLocationViolation => "位置违规：引擎不得直读他引擎私有缓冲——走共享或交接",
            McCode::McImplicitAccess => "一致性违规：跨引擎访问必须显式 sync——隐式访问即缺陷",
            McCode::McBoundaryMissed => "边界纪律违例：交接边界必 flush——先补 sync 点再过界",
            McCode::McOrderMismatch => "序对拍失配：三路写读结果不一致——先查路径实现",
        }
    }
}

// ---------------------------------------------------------------------------
// 一、两级内存模型：位置标签 + 缓冲区账（判据「位置标签」）
// ---------------------------------------------------------------------------

/// 内存位置三态（位置标签规范——每缓冲区出生即挂牌）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemLocation {
    /// GPU 引擎私有。
    GpuPrivate,
    /// NPU 引擎私有。
    NpuPrivate,
    /// 共享（跨引擎可见——但须显式 sync 才可跨引擎访问）。
    Shared,
}

/// 访问引擎（vck05 PrefEngine 同口径四档——消费面一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessEngine {
    Gpu,
    Npu,
    Dsp,
    IspMedia,
}

/// 缓冲区对象（bid × 位置 × 创建引擎——位置是登记事实）。
#[derive(Clone, Copy, Debug)]
pub struct BufferObject {
    /// 缓冲区 id。
    pub bid: u32,
    /// 位置标签。
    pub location: MemLocation,
    /// 创建引擎（私有缓冲的主人）。
    pub owner: AccessEngine,
}

/// 缓冲区登记账。
pub struct BufferBook {
    bufs: Vec<BufferObject>,
    /// 位置违规检出数。
    pub location_violations: u32,
    /// 一致性违规检出数。
    pub implicit_violations: u32,
}

impl BufferBook {
    /// 空账。
    pub fn new() -> Self {
        BufferBook { bufs: Vec::new(), location_violations: 0, implicit_violations: 0 }
    }

    /// **登记缓冲**（挂牌——同 id 复用不重开）。
    pub fn declare(&mut self, bid: u32, location: MemLocation, owner: AccessEngine) {
        let mut i = 0usize;
        while i < self.bufs.len() {
            if self.bufs[i].bid == bid {
                return; // 已存在——复用（幂等）
            }
            i += 1;
        }
        self.bufs.push(BufferObject { bid, location, owner });
    }

    /// 查位置。
    pub fn location_of(&self, bid: u32) -> Option<MemLocation> {
        let mut i = 0usize;
        while i < self.bufs.len() {
            if self.bufs[i].bid == bid {
                return Some(self.bufs[i].location);
            }
            i += 1;
        }
        None
    }

    fn owner_of(&self, bid: u32) -> Option<AccessEngine> {
        let mut i = 0usize;
        while i < self.bufs.len() {
            if self.bufs[i].bid == bid {
                return Some(self.bufs[i].owner);
            }
            i += 1;
        }
        None
    }

    /// **违规检测器**（判据「违规检测」——访问请求裁决）：
    /// ① 位置违规：访问他引擎**私有**缓冲（Gpu 私有只有 Gpu 能碰）；
    /// ② 一致性违规：跨引擎访问 **Shared** 缓冲但该 bid 无 sync 记账
    /// （禁隐式——显式原则铁律）。返回违例码（None=合法放行）。
    pub fn check_access(
        &mut self,
        engine: AccessEngine,
        bid: u32,
        synced_bids: &[u32],
    ) -> Option<McCode> {
        let loc = self.location_of(bid)?;
        let owner = self.owner_of(bid)?;
        match loc {
            // 私有缓冲：只有主人能碰——他人访问即位置违规。
            MemLocation::GpuPrivate | MemLocation::NpuPrivate => {
                if owner != engine {
                    self.location_violations += 1;
                    return Some(McCode::McLocationViolation);
                }
                None
            }
            // 共享缓冲：跨引擎访问必须显式 sync（同引擎自访宽松放行）。
            MemLocation::Shared => {
                if owner != engine {
                    let mut synced = false;
                    let mut i = 0usize;
                    while i < synced_bids.len() {
                        if synced_bids[i] == bid {
                            synced = true;
                        }
                        i += 1;
                    }
                    if !synced {
                        self.implicit_violations += 1;
                        return Some(McCode::McImplicitAccess);
                    }
                }
                None
            }
        }
    }

    /// 登记数。
    pub fn len(&self) -> usize {
        self.bufs.len()
    }

    /// 空判据。
    pub fn is_empty(&self) -> bool {
        self.bufs.is_empty()
    }

    /// **位置可视化**（判据「内存位置可视化」——读屏一行）。
    pub fn visualize(&self) -> String {
        let mut out = String::from("位置图:");
        let mut i = 0usize;
        while i < self.bufs.len() {
            let tag = match self.bufs[i].location {
                MemLocation::GpuPrivate => "GPU私",
                MemLocation::NpuPrivate => "NPU私",
                MemLocation::Shared => "共享",
            };
            out.push_str(&alloc::format!(" b{}={}", self.bufs[i].bid, tag));
            i += 1;
        }
        out
    }
}

impl Default for BufferBook {
    fn default() -> Self {
        BufferBook::new()
    }
}

// ---------------------------------------------------------------------------
// 二、sync 点 API：边界纪律（判据「边界纪律」——交接边界必 sync）
// ---------------------------------------------------------------------------

/// **边界纪律裁决**（flush 时机规范——交接边界必 sync）：
/// `at_boundary=true`（交接边界）而 `flushed=false` → 边界纪律违例
/// McBoundaryMissed；非边界（引擎内部刷新）无 sync 不追责（宽松面）。
/// 裁决 O(1)——锚点口径。
pub const fn boundary_check(at_boundary: bool, flushed: bool) -> Option<McCode> {
    if at_boundary && !flushed {
        return Some(McCode::McBoundaryMissed);
    }
    None
}

/// sync 点账（flush 时机留痕——边界纪律审计面）。
#[derive(Clone, Debug)]
pub struct SyncLedger {
    /// flush 记录（bid × 是否边界）。
    pub flushes: Vec<(u32, bool)>,
    /// 边界纪律违例数。
    pub boundary_misses: u32,
}

impl SyncLedger {
    /// 空账。
    pub fn new() -> Self {
        SyncLedger { flushes: Vec::new(), boundary_misses: 0 }
    }

    /// **flush**（时机规范执行点——boundary=true 即交接边界）。
    pub fn flush(&mut self, bid: u32, at_boundary: bool) -> Option<McCode> {
        let v = boundary_check(at_boundary, true);
        self.flushes.push((bid, at_boundary));
        v
    }

    /// **跳过 flush**（违规路径显性化——审计用，正常流程不走）。
    pub fn skip_flush(&mut self, at_boundary: bool) -> Option<McCode> {
        let v = boundary_check(at_boundary, false);
        if v == Some(McCode::McBoundaryMissed) {
            self.boundary_misses += 1;
        }
        v
    }

    /// bid 是否已 flush。
    pub fn is_flushed(&self, bid: u32) -> bool {
        let mut i = 0usize;
        while i < self.flushes.len() {
            if self.flushes[i].0 == bid {
                return true;
            }
            i += 1;
        }
        false
    }
}

impl Default for SyncLedger {
    fn default() -> Self {
        SyncLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 三、USM 平台支持矩阵与双路裁决（判据「USM 双路」）
// ---------------------------------------------------------------------------

/// 平台（USM 支持矩阵行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsmPlatform {
    /// Intel（USM 全支持——零拷贝路径可用）。
    Intel,
    /// AMD（USM 部分支持——SVM 面有限，一期按不支持降级拷贝路径）。
    Amd,
    /// 未知/未登记（缺省不支持——诚实降级）。
    Unknown,
}

/// 访问路径双实现（与 vck05 交接路径表同口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessPath {
    /// USM 零拷贝（统一共享内存直接访问）。
    Usm,
    /// 显式拷贝（分配+拷贝+sync）。
    Copy,
}

/// **平台支持矩阵**（判据「平台支持矩阵（Intel/AMD）」——静态可查）。
pub const fn usm_supported(p: UsmPlatform) -> bool {
    match p {
        UsmPlatform::Intel => true,
        UsmPlatform::Amd => false, // 一期口径：SVM 面有限按不支持降级——诚实
        UsmPlatform::Unknown => false,
    }
}

/// **双路裁决**（支持→USM 零拷贝；不支持→显式拷贝——双实现单裁决）。
pub const fn decide_access_path(p: UsmPlatform) -> AccessPath {
    if usm_supported(p) {
        AccessPath::Usm
    } else {
        AccessPath::Copy
    }
}

// ---------------------------------------------------------------------------
// 四、写读序三路对拍（判据「三路对拍」——一致性回归面）
// ---------------------------------------------------------------------------

/// 访问模式三路（序测试矩阵行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderMode {
    /// 宽松（引擎内——无跨引擎语义）。
    Relaxed,
    /// 显式同步（跨引擎——sync 点必踩）。
    ExplicitSync,
    /// USM（零拷贝——硬件一致面，写读序由 USM 契约保证）。
    Usm,
}

/// 单路写读序结果（写值 × 读值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrderResult {
    /// 写入值。
    pub written: u64,
    /// 读回值。
    pub read: u64,
}

impl OrderResult {
    /// 写读一致（同值——一致性面绿）。
    pub const fn coherent(&self) -> bool {
        self.written == self.read
    }
}

/// **三路对拍**（判据「三路对拍」——同写入值跑三路，读回必须全等：
/// 宽松/显式/USM 三路的语义差异只在同步要求，不在最终可见性——最终
/// 读回不一致即路径实现有错 McOrderMismatch）。O(1) 对拍。
pub fn triple_order_check(relaxed: OrderResult, explicit: OrderResult, usm: OrderResult) -> Option<McCode> {
    let all_coherent = relaxed.coherent() && explicit.coherent() && usm.coherent();
    let all_same = relaxed.read == explicit.read && explicit.read == usm.read;
    if all_coherent && all_same {
        None
    } else {
        Some(McCode::McOrderMismatch)
    }
}

// ---------------------------------------------------------------------------
// 判据（判据面零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F1606 域自检（判据 9 条六组：位置标签/显式原则/边界纪律/USM 双路/三路对拍/违规检测+元）。
pub fn run_vck06_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F1606");

    // --- 位置标签（判据一）---
    // loc-01：挂牌往返+私有主访问放行——GpuPrivate/NpuPrivate/Shared 三态
    // 登记后 location_of 读回一致；主人访问自身私有缓冲合法。
    let mut book = BufferBook::new();
    book.declare(1, MemLocation::GpuPrivate, AccessEngine::Gpu);
    book.declare(2, MemLocation::NpuPrivate, AccessEngine::Npu);
    book.declare(3, MemLocation::Shared, AccessEngine::Gpu);
    let loc_ok = book.location_of(1) == Some(MemLocation::GpuPrivate)
        && book.location_of(2) == Some(MemLocation::NpuPrivate)
        && book.location_of(3) == Some(MemLocation::Shared)
        && book.location_of(99).is_none()
        && book.check_access(AccessEngine::Gpu, 1, &[]).is_none()
        && book.check_access(AccessEngine::Npu, 2, &[]).is_none()
        && book.len() == 3;
    if loc_ok {
        s.ok("K06-loc-01-位置标签挂牌往返");
    } else {
        s.fail("K06-loc-01-位置标签挂牌往返", "位置标签失效");
    }

    // --- 显式原则（判据二）---
    // loc-02：位置违规——NPU 引擎直读 GPU 私有缓冲 → McLocationViolation
    // 计数；GPU 读自己私有的 NPU 邻居缓冲同理。
    let v1 = book.check_access(AccessEngine::Npu, 1, &[]);
    let v2 = book.check_access(AccessEngine::Dsp, 2, &[]);
    let viol_ok = v1 == Some(McCode::McLocationViolation)
        && v2 == Some(McCode::McLocationViolation)
        && book.location_violations == 2;
    if viol_ok {
        s.ok("K06-loc-02-私有位置违规检出");
    } else {
        s.fail("K06-loc-02-私有位置违规检出", "位置违规检出失效");
    }

    // loc-03：禁隐式（Shared 面）——跨引擎访问 Shared 无 sync → McImplicit
    // Access；有 sync 记账 → 放行；主人自访 Shared 无 sync 放行（宽松面）。
    let v3 = book.check_access(AccessEngine::Npu, 3, &[]);
    let v4 = book.check_access(AccessEngine::Npu, 3, &[3]);
    let own_ok = book.check_access(AccessEngine::Gpu, 3, &[]).is_none();
    let implicit_ok = v3 == Some(McCode::McImplicitAccess)
        && v4.is_none()
        && own_ok
        && book.implicit_violations == 1;
    if implicit_ok {
        s.ok("K06-loc-03-禁隐式显式原则");
    } else {
        s.fail("K06-loc-03-禁隐式显式原则", "显式原则失效");
    }

    // --- 边界纪律（判据三）---
    // bnd-01：flush 时机规范——边界必 sync：skip_flush(边界) → McBoundary
    // Missed+计数；flush(边界) 放行留痕；非边界跳过不追责（宽松面）。
    let mut led = SyncLedger::new();
    let miss = led.skip_flush(true);
    let noboundary = led.skip_flush(false);
    let flushed = led.flush(7, true);
    let bnd_ok = miss == Some(McCode::McBoundaryMissed)
        && noboundary.is_none()
        && flushed.is_none()
        && led.boundary_misses == 1
        && led.is_flushed(7)
        && !led.is_flushed(8);
    if bnd_ok {
        s.ok("K06-bnd-01-交接边界必sync");
    } else {
        s.fail("K06-bnd-01-交接边界必sync", "边界纪律失效");
    }

    // --- USM 双路（判据四）---
    // usm-01：平台矩阵+双路裁决——Intel→USM；AMD→Copy（一期诚实降级）；
    // Unknown→Copy；矩阵三行两态互异口径自洽。
    let usm_ok = usm_supported(UsmPlatform::Intel)
        && !usm_supported(UsmPlatform::Amd)
        && !usm_supported(UsmPlatform::Unknown)
        && decide_access_path(UsmPlatform::Intel) == AccessPath::Usm
        && decide_access_path(UsmPlatform::Amd) == AccessPath::Copy
        && decide_access_path(UsmPlatform::Unknown) == AccessPath::Copy;
    if usm_ok {
        s.ok("K06-usm-01-平台矩阵双路裁决");
    } else {
        s.fail("K06-usm-01-平台矩阵双路裁决", "USM 面失效");
    }

    // --- 三路对拍（判据五）---
    // ord-01：写读序矩阵——三路同写 0xA5A5 读回全同 → 零码；一路读错 →
    // McOrderMismatch；一路自身写读不一致（读回≠写入）→ 同样失配。
    let good = OrderResult { written: 0xA5A5, read: 0xA5A5 };
    let none = triple_order_check(good, good, good);
    let bad_read = triple_order_check(good, OrderResult { written: 0xA5A5, read: 0x1234 }, good);
    let bad_self = triple_order_check(good, good, OrderResult { written: 0xA5A5, read: 0xBEEF });
    let ord_ok = none.is_none()
        && bad_read == Some(McCode::McOrderMismatch)
        && bad_self == Some(McCode::McOrderMismatch);
    if ord_ok {
        s.ok("K06-ord-01-三路对拍序一致");
    } else {
        s.fail("K06-ord-01-三路对拍序一致", "序对拍失效");
    }

    // --- 违规检测（判据六）---
    // dbg-01：位置可视化—— visualize 含各缓冲标签；空册平凡；违规计数
    // 与检出同步（检 2 次 violation 计 2——账实守恒）。
    let viz = book.visualize();
    let dbg_ok = viz.contains("b1=GPU私")
        && viz.contains("b2=NPU私")
        && viz.contains("b3=共享")
        && BufferBook::new().visualize() == "位置图:"
        && book.location_violations == 2
        && book.implicit_violations == 1;
    if dbg_ok {
        s.ok("K06-dbg-01-位置可视化账实守恒");
    } else {
        s.fail("K06-dbg-01-位置可视化账实守恒", "调试面失效");
    }

    // --- 判据元 ---
    // 元-01：四码互异+详情齐备+版本串非空。
    let codes = McCode::ALL;
    let mut codes_ok = codes.len() == 4;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if codes[i].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] || codes[i].detail() == codes[j].detail() {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let meta_ok = codes_ok && !K06_MEMCOHER_VERSION.is_empty();
    if meta_ok {
        s.ok("K06-元-01-四码互异版本齐备");
    } else {
        s.fail("K06-元-01-四码互异版本齐备", "码面漂移");
    }

    // 元-02：零 panic 面——自扫本文件+空册平凡（全查询/违规检测空态）。
    let src = include_str!("vck06_memcoher.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let mut empty_book = BufferBook::new();
    let empty_ok = empty_book.is_empty()
        && empty_book.location_of(1).is_none()
        && empty_book.check_access(AccessEngine::Gpu, 1, &[]).is_none()
        && empty_book.visualize() == "位置图:"
        && !K06_MEMCOHER_VERSION.is_empty();
    if hits == 0 && empty_ok {
        s.ok("K06-元-02-零panic空态平凡");
    } else {
        s.fail("K06-元-02-零panic空态平凡", "panic 面或空态失效");
    }

    s
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
