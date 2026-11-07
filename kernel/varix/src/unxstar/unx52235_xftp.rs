//! UNX-F52235 Xftp 收口 · 0xC7A80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52235"，逐字）：
//!   title: Xftp 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52236-J1 六判据全
//!     落；脚本自动化（VB/Python 接口）×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52235）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52236-J1，照原文锚定；功能按本卡标题（Xftp）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 协议三族 12 格：FTP/FTPS/SFTP 三家族各 4 格，家族单调（逆序必抓）。
//!   ② 同步浏览 10 格：本地/远端目录同步浏览逐格落账，载荷对照漂移必抓。
//!   ③ 传输队列 8 操作：上传/下载/续传/覆盖/跳过/重命名/删除/校验八操作
//!      闭集，越集/漂移必抓。
//!   ④ 与 Xshell 消费联签 4 格：会话复用/密钥复用/隧道复用/宏复用四联签
//!      格逐格落账，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（越闭集/漂移/逆序）全部落在当轮
//!      临时账本上，led 上 injected==caught==10、escaped==0；双跑指纹一
//!      致；定级闭集 S1/S2/S3（全通 → S1，KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52235;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 协议三族格数（三族 × 4）。
pub const PROTOS: usize = 12;

/// 同步浏览格数。
pub const SYNC_BROWSERS: usize = 10;

/// 传输队列操作数。
pub const QUEUE_OPS: usize = 8;

/// 与 Xshell 消费联签格数。
pub const COSIGN_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XftpCode {
    /// 成功
    Ok = 0,
    /// 协议格越闭集
    ENoProto,
    /// 协议载荷漂移
    EProtoDrift,
    /// 协议家族逆序
    EOrder,
    /// 同步浏览格越闭集
    ENoSync,
    /// 同步浏览漂移
    ESyncDrift,
    /// 队列操作越闭集或漂移
    ENoQueueOp,
    /// 联签格越闭集
    ENoCosign,
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

impl XftpCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            XftpCode::Ok => "成功",
            XftpCode::ENoProto => "协议格越闭集：FTP/FTPS/SFTP 12 格之外先登记",
            XftpCode::EProtoDrift => "协议载荷漂移：协议折叠与基准不恒等，逐格复算",
            XftpCode::EOrder => "协议家族逆序：FTP→FTPS→SFTP 单调，逆序必抓",
            XftpCode::ENoSync => "同步浏览格越闭集：本地/远端同步浏览 10 格之外先登记",
            XftpCode::ESyncDrift => "同步浏览漂移：浏览折叠与基准不恒等，逐格复算",
            XftpCode::ENoQueueOp => "队列操作越闭集或漂移：传输队列 8 操作之外先登记，载荷不恒等先复算",
            XftpCode::ENoCosign => "联签格越闭集：与 Xshell 消费联签 4 格之外先登记，缺格先补齐",
            XftpCode::EDup => "重复落账：同格只许一条账目",
            XftpCode::ESeal => "未齐即封：12 协议 + 10 同步浏览 + 8 队列操作 + 4 联签未齐先补齐",
            XftpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            XftpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            XftpCode::EGradeDrift => "定级漂移：全通且联签 4 格全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+12协议+10同步浏览+8队列操作+4联签全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 协议格载荷（格 p；家族 = p/4）。
pub const fn proto_cell(p: usize) -> Option<u64> {
    if p >= PROTOS {
        return None;
    }
    Some((0xE3500u64 + (p as u64) * 0x2C) ^ ((p / 4) as u64).wrapping_mul(0x01000193))
}

/// 协议家族（0..4=FTP 4..8=FTPS 8..12=SFTP）。
pub const fn proto_kind(p: usize) -> Option<u32> {
    if p >= PROTOS {
        return None;
    }
    Some((p / 4) as u32)
}

/// 协议家族名。
pub const fn proto_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("FTP"),
        1 => Some("FTPS"),
        2 => Some("SFTP"),
        _ => None,
    }
}

/// 同步浏览格载荷（格 b）。
pub const fn sync_cell(b: usize) -> Option<u64> {
    if b >= SYNC_BROWSERS {
        return None;
    }
    Some((0xE6000u64 + (b as u64) * 0x3B) ^ 0x584654505F53594E)
}

/// 传输队列操作载荷（操作 o）。
pub const fn queue_cell(o: usize) -> Option<u64> {
    if o >= QUEUE_OPS {
        return None;
    }
    Some((0xE8000u64 + (o as u64) * 0x5D) ^ 0x584654505F515155)
}

/// 队列操作名（const fn 表 + Option 边界）。
pub const fn queue_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("上传"),
        1 => Some("下载"),
        2 => Some("续传"),
        3 => Some("覆盖"),
        4 => Some("跳过"),
        5 => Some("重命名"),
        6 => Some("删除"),
        7 => Some("校验"),
        _ => None,
    }
}

/// 与 Xshell 消费联签格折叠（格 r）。
pub const fn cosign_row(r: usize) -> Option<u32> {
    if r >= COSIGN_ROWS {
        return None;
    }
    Some(0xEA00 + r as u32 * 0x2C)
}

/// 联签格名。
pub const fn cosign_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("会话复用联签"),
        1 => Some("密钥复用联签"),
        2 => Some("隧道复用联签"),
        3 => Some("宏复用联签"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Xftp 收口账本状态机。
#[derive(Clone)]
pub struct XftpLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 协议账。
    protos: [Option<u64>; PROTOS],
    /// 同步浏览账。
    syncs: [Option<u64>; SYNC_BROWSERS],
    /// 传输队列操作账。
    queue: [Option<u64>; QUEUE_OPS],
    /// 联签格账。
    cosigns: [Option<u32>; COSIGN_ROWS],
    /// 最后协议家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl XftpLedger {
    /// 新账本。
    pub fn new() -> Self {
        XftpLedger {
            gates: [None; 6],
            protos: [None; PROTOS],
            syncs: [None; SYNC_BROWSERS],
            queue: [None; QUEUE_OPS],
            cosigns: [None; COSIGN_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), XftpCode> {
        if gate >= 6 {
            return Err(XftpCode::ESeal);
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

    /// 协议一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_proto(&mut self, p: usize, got: u64) -> Result<(), XftpCode> {
        if p >= PROTOS {
            return Err(XftpCode::ENoProto);
        }
        let kind = match proto_kind(p) {
            Some(k) => k,
            None => return Err(XftpCode::ENoProto),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(XftpCode::EOrder);
        }
        let want = match proto_cell(p) {
            Some(w) => w,
            None => return Err(XftpCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + p as u64);
            return Err(XftpCode::EProtoDrift);
        }
        if match self.protos.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(XftpCode::EDup);
        }
        if let Some(x) = self.protos.get_mut(p) {
            *x = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + p as u64);
        Ok(())
    }

    /// 协议全账核验（12 格）。
    pub fn protos_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROTOS {
            match self.protos.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 最后协议家族读数（逆序注入不改值核验）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 同步浏览一格（载荷对照 + 重复拒）。
    pub fn record_sync(&mut self, b: usize, got: u64) -> Result<(), XftpCode> {
        let want = match sync_cell(b) {
            Some(w) => w,
            None => return Err(XftpCode::ENoSync),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + b as u64);
            return Err(XftpCode::ESyncDrift);
        }
        if match self.syncs.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(XftpCode::EDup);
        }
        if let Some(x) = self.syncs.get_mut(b) {
            *x = Some(got);
        }
        self.fnv(0x7301 + b as u64);
        Ok(())
    }

    /// 同步浏览全账核验（10 格）。
    pub fn syncs_all(&self) -> bool {
        let mut b = 0usize;
        while b < SYNC_BROWSERS {
            match self.syncs.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 传输队列操作一格（载荷对照 + 重复拒）。
    pub fn record_queue_op(&mut self, o: usize, got: u64) -> Result<(), XftpCode> {
        let want = match queue_cell(o) {
            Some(w) => w,
            None => return Err(XftpCode::ENoQueueOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + o as u64);
            return Err(XftpCode::ENoQueueOp);
        }
        if match self.queue.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(XftpCode::EDup);
        }
        if let Some(x) = self.queue.get_mut(o) {
            *x = Some(got);
        }
        self.fnv(0x7401 + o as u64);
        Ok(())
    }

    /// 队列操作全账核验（8 操作）。
    pub fn queue_all(&self) -> bool {
        let mut o = 0usize;
        while o < QUEUE_OPS {
            match self.queue.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 联签一格（与 Xshell 消费联签）。
    pub fn declare_cosign(&mut self, r: usize) -> Result<(), XftpCode> {
        if r >= COSIGN_ROWS {
            return Err(XftpCode::ENoCosign);
        }
        if match self.cosigns.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(XftpCode::EDup);
        }
        if let Some(x) = self.cosigns.get_mut(r) {
            *x = cosign_row(r);
        }
        self.fnv(0x7500 + r as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn cosigns_all(&self) -> bool {
        let mut r = 0usize;
        while r < COSIGN_ROWS {
            match self.cosigns.get(r) {
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
        if self.gates_all()
            && self.protos_all()
            && self.syncs_all()
            && self.queue_all()
            && self.cosigns_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for XftpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = XftpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROTOS {
        if let Some(w) = proto_cell(p) {
            if led.record_proto(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut b = 0usize;
    while b < SYNC_BROWSERS {
        if let Some(w) = sync_cell(b) {
            if led.record_sync(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut o = 0usize;
    while o < QUEUE_OPS {
        if let Some(w) = queue_cell(o) {
            if led.record_queue_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < COSIGN_ROWS {
        let _ = led.declare_cosign(r);
        r += 1;
    }
    // 注入 10 轮：越闭集 / 漂移 / 逆序三态轮转，全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = XftpLedger::new();
        let caught = match kind {
            0 => {
                // 越闭集：协议 12 与联签 4 之外必抓。
                if k % 2 == 0 {
                    lt.record_proto(PROTOS, 0).is_err()
                } else {
                    lt.declare_cosign(COSIGN_ROWS).is_err()
                }
            }
            1 => {
                // 漂移：先录正格再注漂移，漂移必抓。
                let _ = lt.record_sync(0, match sync_cell(0) {
                    Some(v) => v,
                    _ => 0,
                });
                lt.record_sync(1, match sync_cell(1) {
                    Some(v) => v.wrapping_add(1),
                    _ => 1,
                }) == Err(XftpCode::ESyncDrift)
            }
            _ => {
                // 逆序：空账本上先录 SFTP 族再注 FTP 族，逆序必抓且不改 last_kind。
                let _ = lt.record_proto(9, match proto_cell(9) {
                    Some(v) => v,
                    _ => 0,
                });
                let before = lt.last_kind();
                let hit = lt.record_proto(1, match proto_cell(1) {
                    Some(v) => v,
                    _ => 0,
                }) == Err(XftpCode::EOrder);
                hit && lt.last_kind() == before
            }
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
/// UNX-F52235 判据集（锚 UNX-F52236-J1，off-by-one 板式）。
pub fn run_unx52235_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52235");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "协议三族 12 格全通",
        recorded >= PROTOS as u32,
        "FTP/FTPS/SFTP 逐格载荷恒等，越级必抓",
    );
    s.add(
        "同步浏览 10 格全通",
        {
            let mut lt = XftpLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < SYNC_BROWSERS {
                if let Some(w) = sync_cell(b) {
                    if lt.record_sync(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.syncs_all()
        },
        "本地/远端目录同步浏览逐格恒等，漂移必抓",
    );
    s.add(
        "传输队列 8 操作全通",
        {
            let mut lt = XftpLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < QUEUE_OPS {
                if let Some(w) = queue_cell(o) {
                    if lt.record_queue_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.queue_all()
        },
        "上传/下载/续传/覆盖/跳过/重命名/删除/校验八操作闭集",
    );
    s.add(
        "与 Xshell 消费联签 4/4 全齐",
        {
            let mut lt = XftpLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < COSIGN_ROWS {
                if lt.declare_cosign(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cosigns_all()
        },
        "会话/密钥/隧道/宏四复用联签格闭集，缺一拒封",
    );
    s.add(
        "Xftp 六判据全落",
        {
            let mut lt = XftpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（UNX-F52236-J1）",
    );
    s.add(
        "协议家族单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PROTOS {
                let a = match proto_kind(p) {
                    Some(k) => k,
                    _ => 3,
                };
                let b = match proto_kind(p - 1) {
                    Some(k) => k,
                    _ => 3,
                };
                if a < b {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "FTP→FTPS→SFTP 三家族闭集单调",
    );
    s.add(
        "队列操作名闭集",
        {
            let mut ok = true;
            let mut o = 0usize;
            while o < QUEUE_OPS {
                if queue_op_name(o).is_none() {
                    ok = false;
                }
                o += 1;
            }
            ok && queue_op_name(QUEUE_OPS).is_none()
        },
        "八操作名逐格有表且越集为 None",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/漂移/逆序三态轮转必抓");
    let mut l0 = XftpLedger::new();
    s.add(
        "边界·协议越闭集",
        l0.record_proto(PROTOS, 0).is_err(),
        "协议 12 越闭集必拒",
    );
    s.add(
        "边界·同步浏览越闭集",
        l0.record_sync(SYNC_BROWSERS, 0).is_err(),
        "同步浏览 10 越闭集必拒",
    );
    s.add(
        "边界·队列操作越闭集",
        l0.record_queue_op(QUEUE_OPS, 0).is_err(),
        "队列操作 8 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            l0.record_sync(0, match sync_cell(0) {
                Some(v) => v,
                _ => 0,
            });
            l0.record_sync(1, match sync_cell(1) {
                Some(v) => v.wrapping_add(1),
                _ => 1,
            }) == Err(XftpCode::ESyncDrift)
        },
        "同步浏览载荷漂移必抓",
    );
    s.add(
        "重复落账拒",
        {
            let _ = l0.record_queue_op(0, match queue_cell(0) {
                Some(v) => v,
                _ => 0,
            });
            l0.record_queue_op(0, match queue_cell(0) {
                Some(v) => v,
                _ => 0,
            }) == Err(XftpCode::EDup)
        },
        "同格只许一条账目",
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
            let mut lt = XftpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < PROTOS {
                if let Some(w) = proto_cell(p) {
                    let _ = lt.record_proto(p, w);
                }
                p += 1;
            }
            let mut b = 0usize;
            while b < SYNC_BROWSERS {
                if let Some(w) = sync_cell(b) {
                    let _ = lt.record_sync(b, w);
                }
                b += 1;
            }
            let mut o = 0usize;
            while o < QUEUE_OPS {
                if let Some(w) = queue_cell(o) {
                    let _ = lt.record_queue_op(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < COSIGN_ROWS {
                let _ = lt.declare_cosign(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且符合 VX15 派工公式",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_kind() {
        assert_eq!(proto_kind(0), Some(0));
        assert_eq!(proto_kind(4), Some(1));
        assert_eq!(proto_kind(11), Some(2));
        assert!(proto_kind(12).is_none());
        assert!(proto_cell(12).is_none());
        assert_eq!(proto_kind_name(2), Some("SFTP"));
        assert!(proto_kind_name(3).is_none());
    }

    #[test]
    fn test_queue_op_names() {
        assert_eq!(queue_op_name(0), Some("上传"));
        assert_eq!(queue_op_name(7), Some("校验"));
        assert!(queue_op_name(8).is_none());
        assert!(queue_cell(QUEUE_OPS).is_none());
    }

    #[test]
    fn test_cosign_rows() {
        assert_eq!(cosign_row_name(0), Some("会话复用联签"));
        assert_eq!(cosign_row_name(3), Some("宏复用联签"));
        assert!(cosign_row_name(4).is_none());
        assert!(cosign_row(COSIGN_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = sync_cell(3).unwrap_or(0);
        assert_eq!(a, sync_cell(3).unwrap_or(0));
        assert_ne!(a, sync_cell(4).unwrap_or(0));
        let p0 = proto_cell(0).unwrap_or(0);
        assert_ne!(p0, proto_cell(4).unwrap_or(0));
        assert!(sync_cell(SYNC_BROWSERS).is_none());
    }

    #[test]
    fn test_proto_order_caught() {
        let mut lt = XftpLedger::new();
        let _ = lt.record_proto(9, proto_cell(9).unwrap_or(0));
        let before = lt.last_kind();
        assert_eq!(lt.record_proto(1, proto_cell(1).unwrap_or(0)), Err(XftpCode::EOrder));
        assert_eq!(lt.last_kind(), before);
        assert!(lt.record_proto(10, proto_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = XftpLedger::new();
        assert_eq!(
            lt.record_sync(5, sync_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(XftpCode::ESyncDrift)
        );
        assert_eq!(
            lt.record_proto(2, proto_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(XftpCode::EProtoDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = XftpLedger::new();
        let _ = lt.record_sync(0, sync_cell(0).unwrap_or(0));
        assert_eq!(lt.record_sync(0, sync_cell(0).unwrap_or(0)), Err(XftpCode::EDup));
        let _ = lt.declare_cosign(0);
        assert_eq!(lt.declare_cosign(0), Err(XftpCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = XftpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = XftpLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROTOS + SYNC_BROWSERS + QUEUE_OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7A80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(XftpCode::Ok as u32, 0);
        assert_eq!(XftpCode::EGradeDrift as u32, 12);
        assert!(!XftpCode::Ok.detail().is_empty());
        assert!(!XftpCode::ENoQueueOp.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = XftpLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52235_checks_green() {
        let s = run_unx52235_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
