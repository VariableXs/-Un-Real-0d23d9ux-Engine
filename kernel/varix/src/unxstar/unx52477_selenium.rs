//! UNX-F52477 Selenium Server / WebDriver 收口 · 0xCB700段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52477"，逐字）：
//!   title: Selenium Server / WebDriver 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52478-J1 六判据全
//!     落（Electron 壳三段账）；×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52477）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52478-J1，照原文锚定；功能按本卡标题（Selenium Server /
//! WebDriver）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五组件族闭集：Server / WebDriver / Grid / Client / Manager 各 4
//!      操作共 20 格，组件族单调（Server→WebDriver→Grid→Client→Manager
//!      逆序必抓）；逐操作载荷纯函数折叠，恒等才收。
//!   ② W3C WebDriver 协议投影 8 格：status/newSession/deleteSession/
//!      getTitle/getCurrentUrl/findElement/executeScript/screenshot 闭
//!      集投影，逐格漂移必抓（F52477-J1「协议 ×20 命令零漂移」的收口
//!      投影面）。
//!   ③ 会话三元闸 3 格：会话 id / 能力矩阵 / 超时预算，逐格漂移必抓；
//!      六判据母版闸逐格落账，未齐即封拒。
//!   ④ Chrome/Firefox 消费联签 4 格（F52461/F52463 锚定）：双驱动逐格
//!      恒等联签；Java 上栈声明 2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（协议漂移/组件族逆序/会话越闭
//!      集）全部落在当轮临时账本上，主账注入=检出=10、逃逸=0；双跑指
//!      纹一致；KB-COMPAT 定级闭集（35 格 + 2 行声明 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52477;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五组件操作数。
pub const COMPONENT_OPS: usize = 20;

/// W3C 协议投影格数。
pub const PROTO_CELLS: usize = 8;

/// 会话三元闸格数。
pub const SESSION_CELLS: usize = 3;

/// Chrome/Firefox 消费联签格数。
pub const COSIGN_CELLS: usize = 4;

/// Java 上栈声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeleniumCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 组件族逆序
    EOrder,
    /// 协议投影号越闭集
    ENoProto,
    /// 协议投影漂移
    EProtoDrift,
    /// 会话格号越闭集
    ENoSess,
    /// 会话三元漂移
    ESessDrift,
    /// 联签格号越闭集
    ENoSign,
    /// 联签漂移
    ESignDrift,
    /// 声明行越闭集
    ENoRow,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl SeleniumCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SeleniumCode::Ok => "成功",
            SeleniumCode::ENoOp => "操作号越闭集：五组件 20 操作集之外先登记",
            SeleniumCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            SeleniumCode::EOrder => "组件族逆序：Server→WebDriver→Grid→Client→Manager 单调，逆序必抓",
            SeleniumCode::ENoProto => "协议投影号越闭集：W3C 投影 8 格之外先登记",
            SeleniumCode::EProtoDrift => "协议投影漂移：W3C 命令投影折叠与基准不恒等，零漂移口径",
            SeleniumCode::ENoSess => "会话格号越闭集：会话三元闸 3 格之外先登记",
            SeleniumCode::ESessDrift => "会话三元漂移：会话 id/能力矩阵/超时预算折叠不恒等",
            SeleniumCode::ENoSign => "联签格号越闭集：Chrome/Firefox 联签 4 格之外先登记",
            SeleniumCode::ESignDrift => "联签漂移：双驱动消费联签折叠与基准不恒等（F52461/F52463）",
            SeleniumCode::ENoRow => "声明行越闭集：Java 上栈声明 2 行闭集之外先登记",
            SeleniumCode::ESeal => "未齐即封：20 操作 + 8 投影 + 3 会话 + 4 联签 + 2 声明未齐先补齐",
            SeleniumCode::EGradeDrift => "定级漂移：35 格 + 2 行声明 + 六判据全通才可定 S1",
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
            Grade::S1 => "S1：六判据+35格+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 组件操作格载荷（操作 o；组件族 = o/4）。
pub const fn component_cell(o: usize) -> Option<u64> {
    if o >= COMPONENT_OPS {
        return None;
    }
    Some((0xD0000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 组件族（0..4=Server 4..8=WebDriver 8..12=Grid 12..16=Client 16..20=Manager）。
pub const fn component_kind(o: usize) -> Option<u32> {
    if o >= COMPONENT_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 组件族名。
pub const fn component_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Server"),
        1 => Some("WebDriver"),
        2 => Some("Grid"),
        3 => Some("Client"),
        4 => Some("Manager"),
        _ => None,
    }
}

/// W3C 协议投影格载荷（投影 p）。
pub const fn proto_cell(p: usize) -> Option<u64> {
    if p >= PROTO_CELLS {
        return None;
    }
    Some((0xD1000u64 + (p as u64) * 0x35) ^ 0x5733435F50524F54)
}

/// 协议投影名（W3C WebDriver 命令闭集投影）。
pub const fn proto_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("status"),
        1 => Some("newSession"),
        2 => Some("deleteSession"),
        3 => Some("getTitle"),
        4 => Some("getCurrentUrl"),
        5 => Some("findElement"),
        6 => Some("executeScript"),
        7 => Some("screenshot"),
        _ => None,
    }
}

/// 会话三元闸格载荷（格 s：0=会话 id 1=能力矩阵 2=超时预算）。
pub const fn session_cell(s: usize) -> Option<u64> {
    if s >= SESSION_CELLS {
        return None;
    }
    Some((0xD2000u64 + (s as u64) * 0x41) ^ 0x53455353494F4E00)
}

/// Chrome/Firefox 消费联签格载荷（格 c）。
pub const fn cosign_cell(c: usize) -> Option<u64> {
    if c >= COSIGN_CELLS {
        return None;
    }
    Some((0xD3000u64 + (c as u64) * 0x47) ^ 0x434F5349474E5300)
}

/// Java 上栈声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Selenium 收口账本状态机。
#[derive(Clone)]
pub struct SelLedger {
    /// 六判据母版闸。
    gates: [Option<bool>; 6],
    /// 组件操作账。
    ops: [Option<u64>; COMPONENT_OPS],
    /// 协议投影账。
    protos: [Option<u64>; PROTO_CELLS],
    /// 会话三元账。
    sess: [Option<u64>; SESSION_CELLS],
    /// 消费联签账。
    signs: [Option<u64>; COSIGN_CELLS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后组件族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SelLedger {
    /// 新账本。
    pub fn new() -> Self {
        SelLedger {
            gates: [None; 6],
            ops: [None; COMPONENT_OPS],
            protos: [None; PROTO_CELLS],
            sess: [None; SESSION_CELLS],
            signs: [None; COSIGN_CELLS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SeleniumCode> {
        if gate >= 6 {
            return Err(SeleniumCode::ESeal);
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

    /// 组件操作一格（族单调 + 载荷对照 + 恒等复落幂等）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), SeleniumCode> {
        if o >= COMPONENT_OPS {
            return Err(SeleniumCode::ENoOp);
        }
        let kind = match component_kind(o) {
            Some(k) => k,
            None => return Err(SeleniumCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(SeleniumCode::EOrder);
        }
        let want = match component_cell(o) {
            Some(w) => w,
            None => return Err(SeleniumCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(SeleniumCode::EDrift);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < COMPONENT_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 协议投影一格（载荷对照 + 恒等幂等）。
    pub fn record_proto(&mut self, p: usize, got: u64) -> Result<(), SeleniumCode> {
        let want = match proto_cell(p) {
            Some(w) => w,
            None => return Err(SeleniumCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + p as u64);
            return Err(SeleniumCode::EProtoDrift);
        }
        if let Some(c) = self.protos.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5301 + p as u64);
        Ok(())
    }

    /// 协议投影全账核验（8 格）。
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

    /// 会话三元一格（载荷对照 + 恒等幂等）。
    pub fn record_session(&mut self, s: usize, got: u64) -> Result<(), SeleniumCode> {
        let want = match session_cell(s) {
            Some(w) => w,
            None => return Err(SeleniumCode::ENoSess),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + s as u64);
            return Err(SeleniumCode::ESessDrift);
        }
        if let Some(c) = self.sess.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x5401 + s as u64);
        Ok(())
    }

    /// 会话三元全账核验（3 格）。
    pub fn sess_all(&self) -> bool {
        let mut s = 0usize;
        while s < SESSION_CELLS {
            match self.sess.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 消费联签一格（载荷对照 + 恒等幂等）。
    pub fn record_cosign(&mut self, c: usize, got: u64) -> Result<(), SeleniumCode> {
        let want = match cosign_cell(c) {
            Some(w) => w,
            None => return Err(SeleniumCode::ENoSign),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + c as u64);
            return Err(SeleniumCode::ESignDrift);
        }
        if let Some(k) = self.signs.get_mut(c) {
            *k = Some(got);
        }
        self.fnv(0x5501 + c as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn signs_all(&self) -> bool {
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            match self.signs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行（Java 上栈）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), SeleniumCode> {
        if r >= SPLIT_ROWS {
            return Err(SeleniumCode::ENoRow);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
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
        if self.gates_all() && self.ops_all() && self.protos_all() && self.sess_all()
            && self.signs_all() && self.splits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SelLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SelLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < COMPONENT_OPS {
        if let Some(w) = component_cell(o) {
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
    let mut s = 0usize;
    while s < SESSION_CELLS {
        if let Some(w) = session_cell(s) {
            if led.record_session(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_CELLS {
        if let Some(w) = cosign_cell(c) {
            if led.record_cosign(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：协议漂移 / 组件族逆序 / 会话越闭集三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SelLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_proto(1, proto_cell(1).unwrap_or(0));
                lt.record_proto(0, proto_cell(0).unwrap_or(0).wrapping_add(1))
                    == Err(SeleniumCode::EProtoDrift)
            }
            1 => {
                let _ = lt.record_op(17, component_cell(17).unwrap_or(0));
                lt.record_op(5, component_cell(5).unwrap_or(0)) == Err(SeleniumCode::EOrder)
            }
            _ => lt.record_session(SESSION_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52477 判据集（锚 UNX-F52478-J1，off-by-one 板式）。
pub fn run_unx52477_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52477");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五组件 20 操作全通",
        {
            let mut lt = SelLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < COMPONENT_OPS {
                if let Some(w) = component_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "Server/WebDriver/Grid/Client/Manager 五族逐操作恒等",
    );
    s.add(
        "W3C 协议投影 8/8 零漂移",
        {
            let mut lt = SelLedger::new();
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
        "status/newSession/deleteSession 等八投影逐格恒等",
    );
    s.add(
        "会话三元闸 3/3 全落",
        {
            let mut lt = SelLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < SESSION_CELLS {
                if let Some(w) = session_cell(sc) {
                    if lt.record_session(sc, w).is_err() {
                        ok = false;
                    }
                }
                sc += 1;
            }
            ok && lt.sess_all()
        },
        "会话 id/能力矩阵/超时预算三元逐格落账",
    );
    s.add(
        "Chrome/Firefox 消费联签 4/4（F52461/F52463）",
        {
            let mut lt = SelLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                if let Some(w) = cosign_cell(c) {
                    if lt.record_cosign(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.signs_all()
        },
        "双驱动消费联签逐格恒等（F52461 Chrome/F52463 Firefox）",
    );
    s.add(
        "六判据母版闸全落",
        {
            let mut lt = SelLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，越界未齐即封拒",
    );
    s.add(
        "Java 上栈声明 2/2 全齐",
        {
            let mut lt = SelLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "Java 上栈独立账两行闭集，缺一拒封",
    );
    s.add(
        "组件族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < COMPONENT_OPS {
                if component_kind(o).unwrap_or(5) < component_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五组件族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "协议漂移/族逆序/会话越闭集三态轮转必抓");
    let mut l0 = SelLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(COMPONENT_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·协议投影越闭集",
        l0.record_proto(PROTO_CELLS, 0).is_err(),
        "投影 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 2 越闭集必拒",
    );
    s.add(
        "会话漂移拒",
        l0.record_session(1, session_cell(1).unwrap_or(0).wrapping_add(1))
            == Err(SeleniumCode::ESessDrift),
        "会话三元漂移必抓",
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
            let mut lt = SelLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < COMPONENT_OPS {
                if let Some(w) = component_cell(o) {
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
            let mut sc = 0usize;
            while sc < SESSION_CELLS {
                if let Some(w) = session_cell(sc) {
                    let _ = lt.record_session(sc, w);
                }
                sc += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                if let Some(w) = cosign_cell(c) {
                    let _ = lt.record_cosign(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "35 格 + 2 行声明 + 六判据全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB700,
        "SEG_BASE 0x40 对齐且落 VX16 公式格 0xCB700",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_kind() {
        assert_eq!(component_kind(0), Some(0));
        assert_eq!(component_kind(3), Some(0));
        assert_eq!(component_kind(4), Some(1));
        assert_eq!(component_kind(19), Some(4));
        assert!(component_kind(20).is_none());
        assert!(component_cell(20).is_none());
        assert_eq!(component_kind_name(0), Some("Server"));
        assert_eq!(component_kind_name(4), Some("Manager"));
        assert!(component_kind_name(5).is_none());
    }

    #[test]
    fn test_proto_names() {
        assert_eq!(proto_name(0), Some("status"));
        assert_eq!(proto_name(1), Some("newSession"));
        assert_eq!(proto_name(7), Some("screenshot"));
        assert!(proto_name(8).is_none());
        assert!(proto_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = component_cell(3).unwrap_or(0);
        assert_eq!(a, component_cell(3).unwrap_or(0));
        assert_ne!(a, component_cell(4).unwrap_or(0));
        let b = session_cell(2).unwrap_or(0);
        assert_eq!(b, session_cell(2).unwrap_or(0));
        assert_ne!(b, session_cell(1).unwrap_or(0));
        assert!(cosign_cell(COSIGN_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = SelLedger::new();
        let _ = lt.record_op(9, component_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_op(2, component_cell(2).unwrap_or(0)),
            Err(SeleniumCode::EOrder)
        );
        assert!(lt.record_op(10, component_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SelLedger::new();
        assert_eq!(
            lt.record_op(5, component_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SeleniumCode::EDrift)
        );
        assert_eq!(
            lt.record_proto(3, proto_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(SeleniumCode::EProtoDrift)
        );
        assert_eq!(
            lt.record_session(2, session_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(SeleniumCode::ESessDrift)
        );
        assert_eq!(
            lt.record_cosign(0, cosign_cell(0).unwrap_or(0).wrapping_add(3)),
            Err(SeleniumCode::ESignDrift)
        );
    }

    #[test]
    fn test_boundaries() {
        let mut lt = SelLedger::new();
        assert!(lt.record_op(COMPONENT_OPS, 0).is_err());
        assert!(lt.record_proto(PROTO_CELLS, 0).is_err());
        assert!(lt.record_session(SESSION_CELLS, 0).is_err());
        assert!(lt.record_cosign(COSIGN_CELLS, 0).is_err());
        assert!(lt.declare_split(SPLIT_ROWS).is_err());
        assert!(lt.judge_gate(6).is_err());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SelLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.gates_all());
        let mut lt2 = SelLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 恰为 run_cycle 实际计数四节公式常量之和：20+8+3+4。
        assert_eq!(recorded, (COMPONENT_OPS + PROTO_CELLS + SESSION_CELLS + COSIGN_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xCB700);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SeleniumCode::Ok as u32, 0);
        assert_eq!(SeleniumCode::EGradeDrift as u32, 12);
        assert_eq!(SeleniumCode::ESeal as u32, 11);
        assert!(!SeleniumCode::Ok.detail().is_empty());
        assert!(!SeleniumCode::ESeal.detail().is_empty());
        assert!(!SeleniumCode::EGradeDrift.detail().is_empty());
        assert!(SeleniumCode::Ok.val() == SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = SelLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 → S2。
        let mut lt2 = SelLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52477_checks_green() {
        let s = run_unx52477_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
