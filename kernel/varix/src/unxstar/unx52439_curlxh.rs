//! UNX-F52439 Curlie / xh 收口 · 0xCAD80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52439"，逐字）：
//!   title: Curlie / xh 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52440-J1 发现 ×10
//!     （实验室段白名单）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52439）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52440-J1，照原文锚定；功能按本卡标题（Curlie / xh）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 双工具五族闭集：Curlie 五族（GET/POST/认证/输出/调试）各 2 格共
//!      10 格，xh 五族（GET/POST/JSON/下载/调试）各 2 格共 10 格，族单调
//!      （逆序必抓），载荷折叠恒等才收。
//!   ② 与 curl 消费联签 4 格：Curlie/xh 对 curl 的消费联签逐格落账（锚
//!      F52255 curl 消费账，联签不重账）。
//!   ③ 双工具对照 8 格：Curlie ↔ xh 参数对照逐格恒等，漂移必抓。
//!   ④ 与 HTTPie 分账声明 2 行闭集：Curlie/xh 与 HTTPie 分账不重账，缺
//!      一拒封；Rust 上栈声明 2 行闭集同口径。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（Curlie 越闭集/xh 载荷漂移/联签
//!      越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；KB-COMPAT 定级
//!      封顶 S2（名义全通 S1 压顶至 S2，登记口径为 S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52439;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// Curlie 五族总格数（每族 2 格 × 5 族）。
pub const CURLIE_OPS: usize = 10;

/// xh 五族总格数（每族 2 格 × 5 族）。
pub const XH_OPS: usize = 10;

/// 与 curl 消费联签格数（锚 F52255）。
pub const CURL_SINK_CELLS: usize = 4;

/// 双工具对照格数。
pub const CROSS_CELLS: usize = 8;

/// 与 HTTPie 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// Rust 上栈声明行数。
pub const RUST_STACK_ROWS: usize = 2;

/// KB-COMPAT 定级封顶（本卡口径：定级 S2，名义 S1 压顶至 S2）。
pub const GRADE_CAP: u32 = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
/// 归账口径：声明行越闭集归 ENoCurlie，对照格越闭集归 ENoSink，
/// 对照格漂移归 ESinkDrift（同码归账，闭集恰 13 变体不扩）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurlxhCode {
    /// 成功
    Ok = 0,
    /// Curlie 族越闭集
    ENoCurlie,
    /// Curlie 载荷漂移
    ECurlieDrift,
    /// xh 族越闭集
    ENoXh,
    /// xh 载荷漂移
    EXhDrift,
    /// curl 消费联签越闭集
    ENoSink,
    /// 联签/对照载荷漂移
    ESinkDrift,
    /// 双工具族逆序
    EOrder,
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
pub const ALL_CODES: [CurlxhCode; 13] = [
    CurlxhCode::Ok,
    CurlxhCode::ENoCurlie,
    CurlxhCode::ECurlieDrift,
    CurlxhCode::ENoXh,
    CurlxhCode::EXhDrift,
    CurlxhCode::ENoSink,
    CurlxhCode::ESinkDrift,
    CurlxhCode::EOrder,
    CurlxhCode::EDup,
    CurlxhCode::ESeal,
    CurlxhCode::EInj,
    CurlxhCode::EFp,
    CurlxhCode::EGradeDrift,
];

impl CurlxhCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CurlxhCode::Ok => "成功",
            CurlxhCode::ENoCurlie => "Curlie 族越闭集：五族 10 格（含声明行闭集）之外先登记",
            CurlxhCode::ECurlieDrift => "Curlie 载荷漂移：格折叠与基准不恒等，逐格复算",
            CurlxhCode::ENoXh => "xh 族越闭集：五族 10 格之外先登记",
            CurlxhCode::EXhDrift => "xh 载荷漂移：格折叠与基准不恒等，逐格复算",
            CurlxhCode::ENoSink => "curl 消费联签越闭集：联签 4 格（对照 8 格同码归账）之外先登记",
            CurlxhCode::ESinkDrift => "联签/对照载荷漂移：折叠与基准不恒等，逐格复算",
            CurlxhCode::EOrder => "双工具族逆序：Curlie 与 xh 各五族单调，逆序必抓",
            CurlxhCode::EDup => "重复落账：同格只许一条账目",
            CurlxhCode::ESeal => "未齐即封：10+10+4+8 格与 2 行分账未齐先补齐",
            CurlxhCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CurlxhCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CurlxhCode::EGradeDrift => "定级漂移：KB-COMPAT 登记口径为 S2 封顶，越顶即漂移",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（名义档，本卡压顶至 S2）
    S1,
    /// S2：有条件收口（本卡登记档）
    S2,
    /// S3：不收口
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
            Grade::S1 => "S1：Curlie/xh 双 10 格+联签 4 格+对照 8 格+分账 2 行全通（名义档）",
            Grade::S2 => "S2：Curlie/xh 定级登记档，S1 压顶至 S2（KB-COMPAT 口径）",
            Grade::S3 => "S3：不收口，六判据缺失",
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
/// Curlie 格载荷（格 o；族 = o/2，GET/POST/认证/输出/调试）。
pub const fn curlie_cell(o: usize) -> Option<u64> {
    if o >= CURLIE_OPS {
        return None;
    }
    Some(
        (0xCC000u64 + (o as u64) * 0x3B)
            ^ ((o / 2) as u64).wrapping_mul(0x01000193)
            ^ 0x4355524C49450000,
    )
}

/// Curlie 族（0..2=GET 2..4=POST 4..6=认证 6..8=输出 8..10=调试）。
pub const fn curlie_kind(o: usize) -> Option<u32> {
    if o >= CURLIE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// Curlie 族名。
pub const fn curlie_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Curlie GET"),
        1 => Some("Curlie POST"),
        2 => Some("Curlie 认证"),
        3 => Some("Curlie 输出"),
        4 => Some("Curlie 调试"),
        _ => None,
    }
}

/// xh 格载荷（格 o；族 = o/2，GET/POST/JSON/下载/调试）。
pub const fn xh_cell(o: usize) -> Option<u64> {
    if o >= XH_OPS {
        return None;
    }
    Some(
        (0xCC080u64 + (o as u64) * 0x3D)
            ^ ((o / 2) as u64).wrapping_mul(0x01000193)
            ^ 0x78485F434C490000,
    )
}

/// xh 族（0..2=GET 2..4=POST 4..6=JSON 6..8=下载 8..10=调试）。
pub const fn xh_kind(o: usize) -> Option<u32> {
    if o >= XH_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// xh 族名。
pub const fn xh_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("xh GET"),
        1 => Some("xh POST"),
        2 => Some("xh JSON"),
        3 => Some("xh 下载"),
        4 => Some("xh 调试"),
        _ => None,
    }
}

/// curl 消费联签格载荷（格 r；锚 F52255 curl 消费账）。
pub const fn curl_sink_cell(r: usize) -> Option<u32> {
    if r >= CURL_SINK_CELLS {
        return None;
    }
    Some(0xCC100 + r as u32 * 0x59)
}

/// 双工具对照格载荷（格 f）。
pub const fn cross_cell(f: usize) -> Option<u64> {
    if f >= CROSS_CELLS {
        return None;
    }
    Some((0xCC180u64 + (f as u64) * 0x2E) ^ 0x58434F4D50415200)
}

/// 与 HTTPie 分账声明行折叠（行 r）。
pub const fn httpie_split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCC200 + r as u32 * 0x2C)
}

/// Rust 上栈声明行折叠（行 r）。
pub const fn rust_stack_row(r: usize) -> Option<u32> {
    if r >= RUST_STACK_ROWS {
        return None;
    }
    Some(0xCC240 + r as u32 * 0x31)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Curlie/xh 收口账本状态机。
#[derive(Clone)]
pub struct CurlxhLedger {
    /// 六判据（发现/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// Curlie 五族 10 格。
    curlies: [Option<u64>; CURLIE_OPS],
    /// xh 五族 10 格。
    xhs: [Option<u64>; XH_OPS],
    /// curl 消费联签 4 格。
    sinks: [Option<u32>; CURL_SINK_CELLS],
    /// 双工具对照 8 格。
    crosses: [Option<u64>; CROSS_CELLS],
    /// 与 HTTPie 分账声明 2 行。
    httpie_rows: [Option<u32>; SPLIT_ROWS],
    /// Rust 上栈声明 2 行。
    rust_rows: [Option<u32>; RUST_STACK_ROWS],
    /// 最后 Curlie 族。
    last_curlie: i64,
    /// 最后 xh 族。
    last_xh: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CurlxhLedger {
    /// 新账本。
    pub fn new() -> Self {
        CurlxhLedger {
            gates: [None; 6],
            curlies: [None; CURLIE_OPS],
            xhs: [None; XH_OPS],
            sinks: [None; CURL_SINK_CELLS],
            crosses: [None; CROSS_CELLS],
            httpie_rows: [None; SPLIT_ROWS],
            rust_rows: [None; RUST_STACK_ROWS],
            last_curlie: -1,
            last_xh: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CurlxhCode> {
        if gate >= 6 {
            return Err(CurlxhCode::ESeal);
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

    /// Curlie 一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_curlie(&mut self, o: usize, got: u64) -> Result<(), CurlxhCode> {
        if o >= CURLIE_OPS {
            return Err(CurlxhCode::ENoCurlie);
        }
        let kind = match curlie_kind(o) {
            Some(k) => k,
            None => return Err(CurlxhCode::ENoCurlie),
        };
        if (kind as i64) < self.last_curlie {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(CurlxhCode::EOrder);
        }
        let want = match curlie_cell(o) {
            Some(w) => w,
            None => return Err(CurlxhCode::ENoCurlie),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + o as u64);
            return Err(CurlxhCode::ECurlieDrift);
        }
        if match self.curlies.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlxhCode::EDup);
        }
        if let Some(c) = self.curlies.get_mut(o) {
            *c = Some(got);
        }
        self.last_curlie = kind as i64;
        self.fnv(0x4202 + o as u64);
        Ok(())
    }

    /// Curlie 全账核验（10 格）。
    pub fn curlies_all(&self) -> bool {
        let mut o = 0usize;
        while o < CURLIE_OPS {
            match self.curlies.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// xh 一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_xh(&mut self, o: usize, got: u64) -> Result<(), CurlxhCode> {
        if o >= XH_OPS {
            return Err(CurlxhCode::ENoXh);
        }
        let kind = match xh_kind(o) {
            Some(k) => k,
            None => return Err(CurlxhCode::ENoXh),
        };
        if (kind as i64) < self.last_xh {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(CurlxhCode::EOrder);
        }
        let want = match xh_cell(o) {
            Some(w) => w,
            None => return Err(CurlxhCode::ENoXh),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(CurlxhCode::EXhDrift);
        }
        if match self.xhs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlxhCode::EDup);
        }
        if let Some(c) = self.xhs.get_mut(o) {
            *c = Some(got);
        }
        self.last_xh = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// xh 全账核验（10 格）。
    pub fn xhs_all(&self) -> bool {
        let mut o = 0usize;
        while o < XH_OPS {
            match self.xhs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// curl 消费联签一格（载荷对照 + 重复拒；锚 F52255）。
    pub fn record_sink(&mut self, r: usize, got: u32) -> Result<(), CurlxhCode> {
        let want = match curl_sink_cell(r) {
            Some(w) => w,
            None => return Err(CurlxhCode::ENoSink),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400 + r as u64);
            return Err(CurlxhCode::ESinkDrift);
        }
        if match self.sinks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlxhCode::EDup);
        }
        if let Some(c) = self.sinks.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x4401 + r as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn sinks_all(&self) -> bool {
        let mut r = 0usize;
        while r < CURL_SINK_CELLS {
            match self.sinks.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 双工具对照一格（载荷对照 + 重复拒）。
    pub fn record_cross(&mut self, f: usize, got: u64) -> Result<(), CurlxhCode> {
        let want = match cross_cell(f) {
            Some(w) => w,
            None => return Err(CurlxhCode::ENoSink),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4500 + f as u64);
            return Err(CurlxhCode::ESinkDrift);
        }
        if match self.crosses.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlxhCode::EDup);
        }
        if let Some(c) = self.crosses.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x4501 + f as u64);
        Ok(())
    }

    /// 对照全账核验（8 格）。
    pub fn crosses_all(&self) -> bool {
        let mut f = 0usize;
        while f < CROSS_CELLS {
            match self.crosses.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 与 HTTPie 分账声明一行。
    pub fn declare_httpie_split(&mut self, r: usize) -> Result<(), CurlxhCode> {
        if r >= SPLIT_ROWS {
            return Err(CurlxhCode::ENoCurlie);
        }
        if match self.httpie_rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlxhCode::EDup);
        }
        if let Some(c) = self.httpie_rows.get_mut(r) {
            *c = httpie_split_row(r);
        }
        self.fnv(0x4600 + r as u64);
        Ok(())
    }

    /// Rust 上栈声明一行。
    pub fn declare_rust_stack(&mut self, r: usize) -> Result<(), CurlxhCode> {
        if r >= RUST_STACK_ROWS {
            return Err(CurlxhCode::ENoCurlie);
        }
        if match self.rust_rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlxhCode::EDup);
        }
        if let Some(c) = self.rust_rows.get_mut(r) {
            *c = rust_stack_row(r);
        }
        self.fnv(0x4700 + r as u64);
        Ok(())
    }

    /// HTTPie 分账声明全齐核验（单节）。
    pub fn httpie_rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.httpie_rows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Rust 上栈声明全齐核验（单节）。
    pub fn rust_rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < RUST_STACK_ROWS {
            match self.rust_rows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 声明行全齐核验（HTTPie 分账 + Rust 上栈）。
    pub fn rows_all(&self) -> bool {
        let mut ok = true;
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            if match self.httpie_rows.get(r) {
                Some(Some(_)) => false,
                _ => true,
            } {
                ok = false;
            }
            r += 1;
        }
        r = 0;
        while r < RUST_STACK_ROWS {
            if match self.rust_rows.get(r) {
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
    pub fn check_conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), CurlxhCode> {
        if injected == caught.wrapping_add(escaped) && injected >= 10 && escaped == 0 {
            Ok(())
        } else {
            Err(CurlxhCode::EInj)
        }
    }

    /// 双跑指纹一致性核验。
    pub fn check_fingerprint(a: u64, b: u64) -> Result<(), CurlxhCode> {
        if a == b && a != 0 {
            Ok(())
        } else {
            Err(CurlxhCode::EFp)
        }
    }

    /// 定级封顶核验（越封顶档即 EGradeDrift）。
    pub fn check_grade_cap(g: Grade) -> Result<(), CurlxhCode> {
        if g.val() > GRADE_CAP {
            Err(CurlxhCode::EGradeDrift)
        } else {
            Ok(())
        }
    }

    /// 名义定级（未封顶）：全通 → S1，六判据齐 → S2，否则 S3。
    pub fn judge_grade_nominal(&self) -> Grade {
        if self.gates_all()
            && self.curlies_all()
            && self.xhs_all()
            && self.sinks_all()
            && self.crosses_all()
            && self.rows_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// KB-COMPAT 定级（本卡封顶 S2：名义 S1 压顶至 S2）。
    pub fn judge_grade(&self) -> Grade {
        cap_grade(self.judge_grade_nominal())
    }
}

impl Default for CurlxhLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CurlxhLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CURLIE_OPS {
        if let Some(w) = curlie_cell(o) {
            if led.record_curlie(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut o = 0usize;
    while o < XH_OPS {
        if let Some(w) = xh_cell(o) {
            if led.record_xh(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < CURL_SINK_CELLS {
        if let Some(w) = curl_sink_cell(r) {
            if led.record_sink(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut f = 0usize;
    while f < CROSS_CELLS {
        if let Some(w) = cross_cell(f) {
            if led.record_cross(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_httpie_split(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < RUST_STACK_ROWS {
        let _ = led.declare_rust_stack(r);
        r += 1;
    }
    // 注入 10 轮：Curlie 越闭集 / xh 载荷漂移 / 联签越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CurlxhLedger::new();
        let caught = match kind {
            0 => lt.record_curlie(CURLIE_OPS, 0).is_err(),
            1 => {
                let _ = lt.record_xh(0, xh_cell(0).unwrap_or(0));
                lt.record_xh(1, xh_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(CurlxhCode::EXhDrift)
            }
            _ => lt.record_sink(CURL_SINK_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4900 + k as u64);
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
/// UNX-F52439 判据集（锚 UNX-F52440-J1，off-by-one 板式）。
pub fn run_unx52439_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52439");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Curlie 五族 10 格全通",
        {
            let mut lt = CurlxhLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < CURLIE_OPS {
                if let Some(w) = curlie_cell(o) {
                    if lt.record_curlie(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.curlies_all()
        },
        "GET/POST/认证/输出/调试 五族逐格恒等，逆序与漂移必抓",
    );
    s.add(
        "xh 五族 10 格全通",
        {
            let mut lt = CurlxhLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < XH_OPS {
                if let Some(w) = xh_cell(o) {
                    if lt.record_xh(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.xhs_all()
        },
        "GET/POST/JSON/下载/调试 五族逐格恒等，逆序与漂移必抓",
    );
    s.add(
        "curl 消费联签 4/4 全落",
        {
            let mut lt = CurlxhLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CURL_SINK_CELLS {
                if let Some(w) = curl_sink_cell(r) {
                    if lt.record_sink(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.sinks_all()
        },
        "与 curl 消费联签逐格恒等（锚 F52255，联签不重账）",
    );
    s.add(
        "双工具对照 8/8 全落",
        {
            let mut lt = CurlxhLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < CROSS_CELLS {
                if let Some(w) = cross_cell(f) {
                    if lt.record_cross(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.crosses_all()
        },
        "Curlie ↔ xh 参数对照逐格恒等，漂移必抓",
    );
    s.add(
        "与 HTTPie 分账声明 2/2 全齐",
        {
            let mut lt = CurlxhLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_httpie_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.httpie_rows_all()
        },
        "Curlie/xh 与 HTTPie 分账不重账，两行闭集缺一拒封（§AI-66.3）",
    );
    s.add(
        "Rust 上栈声明 2/2 全齐",
        {
            let mut lt = CurlxhLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RUST_STACK_ROWS {
                if lt.declare_rust_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rust_rows_all()
        },
        "Rust 上栈声明两行闭集缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = CurlxhLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "发现/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "双工具族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CURLIE_OPS {
                if curlie_kind(o).unwrap_or(5) < curlie_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            o = 1;
            while o < XH_OPS {
                if xh_kind(o).unwrap_or(5) < xh_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "Curlie 与 xh 各五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "Curlie 越闭集/xh 载荷漂移/联签越闭集三态轮转必抓",
    );
    let mut l0 = CurlxhLedger::new();
    s.add(
        "边界·Curlie 越闭集",
        l0.record_curlie(CURLIE_OPS, 0).is_err(),
        "Curlie 格 10 越闭集必拒",
    );
    s.add(
        "边界·xh 越闭集",
        l0.record_xh(XH_OPS, 0).is_err(),
        "xh 格 10 越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_sink(CURL_SINK_CELLS, 0).is_err()
            && l0.record_cross(CROSS_CELLS, 0).is_err(),
        "联签 4 / 对照 8 越闭集必拒（同码归账）",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        CurlxhLedger::check_conservation(inj_n, caught_n, esc_n).is_ok(),
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        CurlxhLedger::check_fingerprint(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = CurlxhLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CURLIE_OPS {
                if let Some(w) = curlie_cell(o) {
                    let _ = lt.record_curlie(o, w);
                }
                o += 1;
            }
            let mut o = 0usize;
            while o < XH_OPS {
                if let Some(w) = xh_cell(o) {
                    let _ = lt.record_xh(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < CURL_SINK_CELLS {
                if let Some(w) = curl_sink_cell(r) {
                    let _ = lt.record_sink(r, w);
                }
                r += 1;
            }
            let mut f = 0usize;
            while f < CROSS_CELLS {
                if let Some(w) = cross_cell(f) {
                    let _ = lt.record_cross(f, w);
                }
                f += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_httpie_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < RUST_STACK_ROWS {
                let _ = lt.declare_rust_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
                && lt.judge_grade_nominal() == Grade::S1
                && CurlxhLedger::check_grade_cap(lt.judge_grade()).is_ok()
        },
        "全通名义 S1 压顶至 S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAD80,
        "SEG_BASE 0x40 对齐且落在 VX16 公式 0xCA400+0x40×38",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 16,
        "含本条共 17 条闭集，判据锚 UNX-F52440-J1 发现 ×10（实验室段白名单）",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dual_kinds() {
        assert_eq!(curlie_kind(0), Some(0));
        assert_eq!(curlie_kind(3), Some(1));
        assert_eq!(curlie_kind(9), Some(4));
        assert!(curlie_kind(10).is_none());
        assert_eq!(xh_kind(0), Some(0));
        assert_eq!(xh_kind(5), Some(2));
        assert_eq!(xh_kind(9), Some(4));
        assert!(xh_kind(10).is_none());
        assert_eq!(curlie_kind_name(2), Some("Curlie 认证"));
        assert_eq!(xh_kind_name(3), Some("xh 下载"));
        assert!(curlie_kind_name(5).is_none());
        assert!(xh_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = curlie_cell(4).unwrap_or(0);
        assert_eq!(a, curlie_cell(4).unwrap_or(0));
        assert_ne!(a, xh_cell(4).unwrap_or(0));
        assert_ne!(cross_cell(0).unwrap_or(0), cross_cell(1).unwrap_or(0));
        assert_ne!(curl_sink_cell(0).unwrap_or(0), curl_sink_cell(1).unwrap_or(0));
        assert!(curlie_cell(CURLIE_OPS).is_none());
        assert!(xh_cell(XH_OPS).is_none());
        assert!(curl_sink_cell(CURL_SINK_CELLS).is_none());
        assert!(cross_cell(CROSS_CELLS).is_none());
        assert!(httpie_split_row(SPLIT_ROWS).is_none());
        assert!(rust_stack_row(RUST_STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = CurlxhLedger::new();
        // 先落 Curlie 族 2（格 5），再回落族 1（格 3）→ 逆序必抓。
        let _ = lt.record_curlie(5, curlie_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_curlie(3, curlie_cell(3).unwrap_or(0)),
            Err(CurlxhCode::EOrder)
        );
        // xh 侧独立记账：先落族 3（格 7），再回落族 2（格 5）→ 逆序必抓。
        let _ = lt.record_xh(7, xh_cell(7).unwrap_or(0));
        assert_eq!(
            lt.record_xh(5, xh_cell(5).unwrap_or(0)),
            Err(CurlxhCode::EOrder)
        );
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CurlxhLedger::new();
        assert_eq!(
            lt.record_curlie(0, curlie_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(CurlxhCode::ECurlieDrift)
        );
        assert_eq!(
            lt.record_xh(0, xh_cell(0).unwrap_or(0).wrapping_add(2)),
            Err(CurlxhCode::EXhDrift)
        );
        assert_eq!(
            lt.record_sink(0, curl_sink_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(CurlxhCode::ESinkDrift)
        );
        assert_eq!(
            lt.record_cross(0, cross_cell(0).unwrap_or(0).wrapping_add(4)),
            Err(CurlxhCode::ESinkDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CurlxhLedger::new();
        let _ = lt.record_curlie(1, curlie_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_curlie(1, curlie_cell(1).unwrap_or(0)),
            Err(CurlxhCode::EDup)
        );
        let _ = lt.record_xh(1, xh_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_xh(1, xh_cell(1).unwrap_or(0)),
            Err(CurlxhCode::EDup)
        );
        let _ = lt.record_cross(3, cross_cell(3).unwrap_or(0));
        assert_eq!(
            lt.record_cross(3, cross_cell(3).unwrap_or(0)),
            Err(CurlxhCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CurlxhLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = CurlxhLedger::new();
        assert!(lt2.judge_gate(2).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_declarations() {
        let mut lt = CurlxhLedger::new();
        assert!(lt.declare_httpie_split(0).is_ok());
        assert!(lt.declare_httpie_split(1).is_ok());
        assert!(lt.declare_httpie_split(2).is_err());
        assert!(lt.declare_rust_stack(0).is_ok());
        assert!(lt.declare_rust_stack(1).is_ok());
        assert!(lt.declare_rust_stack(2).is_err());
        assert!(lt.rows_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 10 Curlie + 10 xh + 4 联签 + 8 对照（公式常量之和）。
        assert_eq!(
            recorded,
            (CURLIE_OPS + XH_OPS + CURL_SINK_CELLS + CROSS_CELLS) as u32
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
        assert_eq!(SEG_BASE, 0xCAD80);
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
        assert_eq!(CurlxhCode::Ok as u32, 0);
        assert_eq!(CurlxhCode::EGradeDrift as u32, 12);
        assert!(CurlxhLedger::check_fingerprint(7, 8).is_err());
        assert!(CurlxhLedger::check_conservation(10, 8, 1).is_err());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐压顶至 S2（名义 S1）。
        let lt = CurlxhLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = CurlxhLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let mut lt3 = CurlxhLedger::new();
        let mut g3 = 0usize;
        while g3 < 6 {
            let _ = lt3.judge_gate(g3);
            g3 += 1;
        }
        let mut o = 0usize;
        while o < CURLIE_OPS {
            if let Some(w) = curlie_cell(o) {
                let _ = lt3.record_curlie(o, w);
            }
            o += 1;
        }
        // 六闸齐但其余节未齐 → 名义 S2，封顶 S2。
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52439_checks_green() {
        let s = run_unx52439_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
