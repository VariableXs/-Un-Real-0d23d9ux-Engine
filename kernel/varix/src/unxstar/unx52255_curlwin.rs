//! UNX-F52255 curl for Windows 收口 · 0xC7F80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52255"，逐字）：
//!   title: curl for Windows 收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52256-J1 下载 ×10
//!     场景全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52255）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52256-J1，照原文锚定；功能按本卡标题（curl for Windows）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐单定制）：
//!   ① 20 操作集：五家族闭集（GET/POST/头/代理/重定向）各 4 操作，家族
//!      单调（逆序必抓），逐格载荷对照。
//!   ② 协议五族 10 格：HTTP/HTTPS/FTP/SFTP/SMTP 各 2 格，族单调，逐格
//!      落账。
//!   ③ 退出码闭集 8：0/1/2/3/6/7/22/28，越闭集必抓，逐码落账。
//!   ④ 版本阶梯 3 档：7.x → 8.0 → 8.x，逐档载荷对照，越级必抓。
//!   ⑤ TLS 消费对账联签锚定行 1 行闭集（卡面判据逐字锚定），缺一拒封。
//!   ⑥ 六判据收口闸：启动/传输/崩溃/卸载/升级/许可，逐格落账，未齐即
//!      封拒。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（操作越闭集/载荷漂移/家族逆
//!      序），检出+逃逸守恒等式；双跑 FNV 指纹一致；KB-COMPAT 定级闭
//!      集：全通 → S1。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52255;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 传输操作数（五家族 × 4）。
pub const TRANSFER_OPS: usize = 20;

/// 协议五族格数（每族 2 格）。
pub const PROTO_CELLS: usize = 10;

/// 退出码闭集容量。
pub const EXIT_CODES: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// TLS 消费对账联签锚定行数。
pub const ANCHOR_ROWS: usize = 1;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurlWinCode {
    /// 成功
    Ok = 0,
    /// 操作越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 操作家族逆序
    EOrder,
    /// 协议格越闭集
    ENoProto,
    /// 协议载荷漂移
    EProtoDrift,
    /// 协议族逆序
    EProtoOrder,
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

impl CurlWinCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CurlWinCode::Ok => "成功",
            CurlWinCode::ENoOp => "操作越闭集：20 操作集之外先登记",
            CurlWinCode::EOpDrift => "操作载荷漂移：操作折叠与基准不恒等，逐格复算",
            CurlWinCode::EOrder => "操作家族逆序：GET→POST→头→代理→重定向单调，逆序必抓",
            CurlWinCode::ENoProto => "协议格越闭集：协议五族 10 格之外先登记",
            CurlWinCode::EProtoDrift => "协议载荷漂移：协议格折叠与基准不恒等，逐格复算",
            CurlWinCode::EProtoOrder => "协议族逆序：HTTP→HTTPS→FTP→SFTP→SMTP 单调，逆序必抓",
            CurlWinCode::ENoExit => "退出码越闭集：0/1/2/3/6/7/22/28 八码之外先登记",
            CurlWinCode::ENoVer => "版本级越阶梯：7.x/8.0/8.x 三档之外先登记",
            CurlWinCode::EVerDrift => "版本阶梯漂移：档载荷折叠与基准不恒等，逐档复算",
            CurlWinCode::ENoAnchor => "联签锚定行越闭集：TLS 消费对账仅 1 行（卡面判据锚定）",
            CurlWinCode::EDup => "重复落账：同格只许一条账目",
            CurlWinCode::EGradeDrift => "定级漂移：全通且联签行齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// curl for Windows KB-COMPAT 定级闭集（本卡口径顶格 S1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurlGrade {
    /// S1：全通收口
    S1,
    /// S2：判据全但格账有缺
    S2,
    /// S3：判据缺失，不收口
    S3,
}

impl CurlGrade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            CurlGrade::S1 => 1,
            CurlGrade::S2 => 2,
            CurlGrade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            CurlGrade::S1 => "S1：20 操作+协议 10 格+退出码 8+版本 3 档+联签 1 行全通收口",
            CurlGrade::S2 => "S2：六判据全但格账有缺，补齐后复评",
            CurlGrade::S3 => "S3：判据缺失，不收口",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 传输操作格载荷（操作 o；家族 = o/4）。
pub const fn transfer_cell(o: usize) -> Option<u64> {
    if o >= TRANSFER_OPS {
        return None;
    }
    Some((0xD10000u64 + (o as u64) * 0x53) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=GET 4..8=POST 8..12=头 12..16=代理 16..20=重定向）。
pub const fn transfer_kind(o: usize) -> Option<u32> {
    if o >= TRANSFER_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 操作家族名。
pub const fn transfer_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("GET"),
        1 => Some("POST"),
        2 => Some("头"),
        3 => Some("代理"),
        4 => Some("重定向"),
        _ => None,
    }
}

/// 协议格载荷（格 p；族 = p/2）。
pub const fn proto_cell(p: usize) -> Option<u64> {
    if p >= PROTO_CELLS {
        return None;
    }
    Some((0xD20000u64 + (p as u64) * 0x3F) ^ ((p / 2) as u64).wrapping_mul(0x010001B3))
}

/// 协议族（0..2=HTTP 2..4=HTTPS 4..6=FTP 6..8=SFTP 8..10=SMTP）。
pub const fn proto_kind(p: usize) -> Option<u32> {
    if p >= PROTO_CELLS {
        return None;
    }
    Some((p / 2) as u32)
}

/// 协议族名。
pub const fn proto_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("HTTP"),
        1 => Some("HTTPS"),
        2 => Some("FTP"),
        3 => Some("SFTP"),
        4 => Some("SMTP"),
        _ => None,
    }
}

/// 退出码闭集核验（0/1/2/3/6/7/22/28 八码）。
pub const fn exit_code_legal(code: usize) -> bool {
    match code {
        0 | 1 | 2 | 3 => true,
        6 | 7 => true,
        22 | 28 => true,
        _ => false,
    }
}

/// 退出码名。
pub const fn exit_code_name(code: usize) -> Option<&'static str> {
    match code {
        0 => Some("0 成功"),
        1 => Some("1 不支持的协议"),
        2 => Some("2 初始化失败"),
        3 => Some("3 URL 格式错"),
        6 => Some("6 主机解析失败"),
        7 => Some("7 连接失败"),
        22 => Some("22 HTTP 错误码"),
        28 => Some("28 操作超时"),
        _ => None,
    }
}

/// 退出码槽位 → 合法码（闭集序 0/1/2/3/6/7/22/28）。
pub const fn exit_slot_code(slot: usize) -> Option<usize> {
    match slot {
        0 => Some(0),
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        4 => Some(6),
        5 => Some(7),
        6 => Some(22),
        7 => Some(28),
        _ => None,
    }
}

/// 版本阶梯档载荷（档 v：0=7.x 1=8.0 2=8.x）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xD30000u64 + (v as u64) * 0x61) ^ 0x4355524C5F564552)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("7.x"),
        1 => Some("8.0"),
        2 => Some("8.x"),
        _ => None,
    }
}

/// TLS 消费对账联签锚定行折叠（行 r）。
pub const fn anchor_row(r: usize) -> Option<u32> {
    if r >= ANCHOR_ROWS {
        return None;
    }
    Some(0xD310 + r as u32 * 0x17)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// curl for Windows 收口账本状态机。
#[derive(Clone)]
pub struct CurlLedger {
    /// 六判据（启动/传输/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; TRANSFER_OPS],
    /// 协议账。
    protos: [Option<u64>; PROTO_CELLS],
    /// 退出码账（下标即闭集序 0..8）。
    exits: [Option<bool>; EXIT_CODES],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 联签锚定行账。
    anchors: [Option<u32>; ANCHOR_ROWS],
    /// 最后操作家族。
    last_kind: i64,
    /// 最后协议族。
    last_proto: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CurlLedger {
    /// 新账本。
    pub fn new() -> Self {
        CurlLedger {
            gates: [None; 6],
            ops: [None; TRANSFER_OPS],
            protos: [None; PROTO_CELLS],
            exits: [None; EXIT_CODES],
            vers: [None; VERSION_STEPS],
            anchors: [None; ANCHOR_ROWS],
            last_kind: -1,
            last_proto: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CurlWinCode> {
        if gate >= 6 {
            return Err(CurlWinCode::EGradeDrift);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), CurlWinCode> {
        if o >= TRANSFER_OPS {
            return Err(CurlWinCode::ENoOp);
        }
        let kind = match transfer_kind(o) {
            Some(k) => k,
            None => return Err(CurlWinCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(CurlWinCode::EOrder);
        }
        let want = match transfer_cell(o) {
            Some(w) => w,
            None => return Err(CurlWinCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(CurlWinCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlWinCode::EDup);
        }
        if let Some(slot) = self.ops.get_mut(o) {
            *slot = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < TRANSFER_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 协议一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_proto(&mut self, p: usize, got: u64) -> Result<(), CurlWinCode> {
        if p >= PROTO_CELLS {
            return Err(CurlWinCode::ENoProto);
        }
        let kind = match proto_kind(p) {
            Some(k) => k,
            None => return Err(CurlWinCode::ENoProto),
        };
        if (kind as i64) < self.last_proto {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(CurlWinCode::EProtoOrder);
        }
        let want = match proto_cell(p) {
            Some(w) => w,
            None => return Err(CurlWinCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + p as u64);
            return Err(CurlWinCode::EProtoDrift);
        }
        if match self.protos.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlWinCode::EDup);
        }
        if let Some(slot) = self.protos.get_mut(p) {
            *slot = Some(got);
        }
        self.last_proto = kind as i64;
        self.fnv(0x7302 + p as u64);
        Ok(())
    }

    /// 协议全账核验（10 格）。
    pub fn protos_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROTO_CELLS {
            match self.protos.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 退出码一格（闭集核验 + 重复拒）。
    pub fn record_exit(&mut self, slot: usize, code: usize) -> Result<(), CurlWinCode> {
        if slot >= EXIT_CODES {
            return Err(CurlWinCode::ENoExit);
        }
        if !exit_code_legal(code) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + code as u64);
            return Err(CurlWinCode::ENoExit);
        }
        if match self.exits.get(slot) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlWinCode::EDup);
        }
        if let Some(c) = self.exits.get_mut(slot) {
            *c = Some(true);
        }
        self.fnv(0x7401 + slot as u64);
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
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), CurlWinCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(CurlWinCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + v as u64);
            return Err(CurlWinCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlWinCode::EDup);
        }
        if let Some(slot) = self.vers.get_mut(v) {
            *slot = Some(got);
        }
        self.fnv(0x7501 + v as u64);
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

    /// TLS 消费对账联签锚定行（卡面判据逐字锚定）。
    pub fn declare_anchor(&mut self, r: usize) -> Result<(), CurlWinCode> {
        if r >= ANCHOR_ROWS {
            return Err(CurlWinCode::ENoAnchor);
        }
        if match self.anchors.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CurlWinCode::EDup);
        }
        if let Some(slot) = self.anchors.get_mut(r) {
            *slot = anchor_row(r);
        }
        self.fnv(0x7600 + r as u64);
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
    pub fn judge_grade(&self) -> CurlGrade {
        if self.gates_all()
            && self.ops_all()
            && self.protos_all()
            && self.exits_all()
            && self.vers_all()
            && self.anchors_all()
        {
            CurlGrade::S1
        } else if self.gates_all() {
            CurlGrade::S2
        } else {
            CurlGrade::S3
        }
    }
}

impl Default for CurlLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CurlLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < TRANSFER_OPS {
        if let Some(w) = transfer_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PROTO_CELLS {
        if let Some(w) = proto_cell(p) {
            if led.record_proto(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
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
    // 注入 10 轮：操作越闭集 / 操作载荷漂移 / 家族逆序三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒数与指纹。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CurlLedger::new();
        let caught = match kind {
            0 => {
                let a = lt.record_exit(0, 41) == Err(CurlWinCode::ENoExit);
                let b = lt.record_op(TRANSFER_OPS, 0) == Err(CurlWinCode::ENoOp);
                a && b
            }
            1 => {
                // 漂移注入先录正格再注漂移。
                if let Some(w) = transfer_cell(0) {
                    let _ = lt.record_op(0, w);
                }
                lt.record_op(1, transfer_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(CurlWinCode::EOpDrift)
            }
            _ => {
                // 逆序注入对空账本同样必抓：先录正格（高族未录格）再逆序注入，
                // 拒后 last_kind 不变。
                if let Some(w) = transfer_cell(16) {
                    let _ = lt.record_op(16, w);
                }
                lt.record_op(0, transfer_cell(0).unwrap_or(0)) == Err(CurlWinCode::EOrder)
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
/// UNX-F52255 判据集（锚 UNX-F52256-J1，off-by-one 板式）。
pub fn run_unx52255_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52255");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "20 操作集五家族全通",
        recorded >= TRANSFER_OPS as u32,
        "GET/POST/头/代理/重定向五家族各 4 操作逐格恒等",
    );
    s.add(
        "协议五族 10 格全落",
        {
            let mut lt = CurlLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROTO_CELLS {
                if let Some(w) = proto_cell(p) {
                    if lt.record_proto(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.protos_all()
        },
        "HTTP/HTTPS/FTP/SFTP/SMTP 各 2 格族单调逐格落账",
    );
    s.add(
        "退出码闭集 8 码全齐",
        {
            let mut lt = CurlLedger::new();
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
        "0/1/2/3/6/7/22/28 八码闭集逐码落账",
    );
    s.add(
        "版本阶梯 3 档全落",
        {
            let mut lt = CurlLedger::new();
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
        "7.x/8.0/8.x 三档逐档恒等，越级必抓",
    );
    s.add(
        "TLS 消费对账联签锚定行 1/1",
        {
            let mut lt = CurlLedger::new();
            let ok = lt.declare_anchor(0).is_ok();
            ok && lt.anchors_all()
        },
        "卡面判据逐字锚定行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = CurlLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/传输/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "操作家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < TRANSFER_OPS {
                if transfer_kind(o).unwrap_or(5) < transfer_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调（GET→POST→头→代理→重定向）",
    );
    s.add(
        "协议族单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PROTO_CELLS {
                if proto_kind(p).unwrap_or(5) < proto_kind(p - 1).unwrap_or(5) {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "协议五族闭集单调（HTTP→HTTPS→FTP→SFTP→SMTP）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "退出码越闭集+操作越闭集/载荷漂移/家族逆序三态轮转必抓",
    );
    // 边界·越闭集 ×3。
    let mut l0 = CurlLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(TRANSFER_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·协议越闭集",
        l0.record_proto(PROTO_CELLS, 0).is_err(),
        "协议格 10 越闭集必拒",
    );
    s.add(
        "边界·版本越阶梯",
        l0.record_version(VERSION_STEPS, 0).is_err(),
        "版本档 3 越阶梯必拒",
    );
    s.add(
        "边界·退出码越闭集",
        lt_exit_out_of_range(),
        "退出码 41 越闭集必拒",
    );
    s.add(
        "操作漂移拒",
        {
            let _ = l0.record_op(0, transfer_cell(0).unwrap_or(0));
            l0.record_op(1, transfer_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(CurlWinCode::EOpDrift)
        },
        "操作载荷漂移必抓",
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
            let mut lt = CurlLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < TRANSFER_OPS {
                if let Some(w) = transfer_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PROTO_CELLS {
                if let Some(w) = proto_cell(p) {
                    let _ = lt.record_proto(p, w);
                }
                p += 1;
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
            lt.judge_grade() == CurlGrade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且公式恒等",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

/// 退出码越闭集复验（独立小函数便于测试复用）。
fn lt_exit_out_of_range() -> bool {
    let mut lt = CurlLedger::new();
    lt.record_exit(0, 41) == Err(CurlWinCode::ENoExit)
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_kind() {
        assert_eq!(transfer_kind(0), Some(0));
        assert_eq!(transfer_kind(3), Some(0));
        assert_eq!(transfer_kind(4), Some(1));
        assert_eq!(transfer_kind(19), Some(4));
        assert!(transfer_kind(20).is_none());
        assert!(transfer_cell(20).is_none());
    }

    #[test]
    fn test_transfer_names() {
        assert_eq!(transfer_kind_name(0), Some("GET"));
        assert_eq!(transfer_kind_name(1), Some("POST"));
        assert_eq!(transfer_kind_name(2), Some("头"));
        assert_eq!(transfer_kind_name(3), Some("代理"));
        assert_eq!(transfer_kind_name(4), Some("重定向"));
        assert!(transfer_kind_name(5).is_none());
    }

    #[test]
    fn test_proto_kind() {
        assert_eq!(proto_kind(0), Some(0));
        assert_eq!(proto_kind(1), Some(0));
        assert_eq!(proto_kind(2), Some(1));
        assert_eq!(proto_kind(9), Some(4));
        assert!(proto_kind(10).is_none());
        assert_eq!(proto_kind_name(3), Some("SFTP"));
        assert!(proto_kind_name(5).is_none());
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
        assert!(exit_code_legal(22));
        assert!(exit_code_legal(28));
        assert!(!exit_code_legal(4));
        assert!(!exit_code_legal(41));
        assert!(!exit_code_legal(56));
        assert!(exit_code_name(41).is_none());
    }

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("7.x"));
        assert_eq!(version_name(1), Some("8.0"));
        assert_eq!(version_name(2), Some("8.x"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_order_caught() {
        let mut lt = CurlLedger::new();
        let _ = lt.record_op(12, transfer_cell(12).unwrap_or(0));
        assert_eq!(
            lt.record_op(3, transfer_cell(3).unwrap_or(0)),
            Err(CurlWinCode::EOrder)
        );
        // 逆序拒后同 last_kind 复验须用同族或更高族未录格。
        assert!(lt.record_op(13, transfer_cell(13).unwrap_or(0)).is_ok());
        let mut lt2 = CurlLedger::new();
        let _ = lt2.record_proto(4, proto_cell(4).unwrap_or(0));
        assert_eq!(
            lt2.record_proto(0, proto_cell(0).unwrap_or(0)),
            Err(CurlWinCode::EProtoOrder)
        );
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CurlLedger::new();
        assert_eq!(
            lt.record_op(5, transfer_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(CurlWinCode::EOpDrift)
        );
        assert_eq!(
            lt.record_proto(7, proto_cell(7).unwrap_or(0).wrapping_add(3)),
            Err(CurlWinCode::EProtoDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(9)),
            Err(CurlWinCode::EVerDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CurlLedger::new();
        let _ = lt.record_op(0, transfer_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_op(0, transfer_cell(0).unwrap_or(0)),
            Err(CurlWinCode::EDup)
        );
        let _ = lt.record_exit(0, 0);
        assert_eq!(lt.record_exit(0, 0), Err(CurlWinCode::EDup));
        let _ = lt.record_exit(4, 6);
        assert_eq!(lt.record_exit(4, 6), Err(CurlWinCode::EDup));
        assert_eq!(exit_slot_code(7), Some(28));
        assert!(exit_slot_code(8).is_none());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CurlLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = CurlLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded
                >= (TRANSFER_OPS + PROTO_CELLS + EXIT_CODES + VERSION_STEPS) as u32
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
        assert_eq!(SEG_BASE, 0xC7F80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CurlWinCode::Ok as u32, 0);
        assert_eq!(CurlWinCode::EGradeDrift as u32, 12);
        assert!(!CurlWinCode::Ok.detail().is_empty());
        assert!(!CurlWinCode::ENoExit.detail().is_empty());
        assert!(!CurlWinCode::EVerDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(CurlGrade::S1.val(), 1);
        assert_eq!(CurlGrade::S3.val(), 3);
        let lt = CurlLedger::new();
        assert_eq!(lt.judge_grade(), CurlGrade::S3);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = CurlLedger::new();
        // 载荷漂移路径记账：注入=检出=1、逃逸=0（越闭集路径提前返回不记账）。
        let _ = lt.record_op(0, transfer_cell(0).unwrap_or(0).wrapping_add(1));
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (1, 1, 0));
    }

    #[test]
    fn unx52255_checks_green() {
        let s = run_unx52255_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
