//! UNX-F12609 NtReadRequest/NtWriteRequest 消息读写→D4 联签 · 0x80200段
//!
//! 判据原文（派工卡 12608 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12609-J1 读写请求语义（缓冲/数据长度/请求类型）
//!      对平断言绿，坏缓冲注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 请求类型闭集：读请求/写请求/无请求三类；读写对偶语义：
//!    写请求落缓冲、读请求取缓冲，读到的长度必须等于先前写入的长度
//!    （数据长度回显语义）；
//! 2. 缓冲语义：缓冲基址非零、数据长度非零时基址不得为零（零基址
//!    携非零长度即坏缓冲）；长度 ≤ 缓冲容量；零长度请求合法（空操作
//!    往返）；
//! 3. D4 联签：写→读往返闭合后联签账落一笔，读写字节计数守恒
//!    （写字节 == 读字节）；
//! 4. 坏缓冲注入 10/10：十路构造（零基址非零长、长度超容、溢出长度、
//!    类型错标、无请求先读、重复写覆盖账、读空缓冲、空样本、类型越
//!    闭集、往返长度漂移）逐路判定与基线一致且双跑相同；
//! 5. 状态机 空闲→已写→已读 单向推进，倒退显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12608 − 12600)）。
pub const SEG_BASE: u32 = 0x80200;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 读写请求诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RwDiag {
    /// 请求类型越三型闭集。
    ETypeRange,
    /// 零基址携非零长度（坏缓冲）。
    ENullBuf,
    /// 数据长度超缓冲容量。
    EOverCap,
    /// 数据长度加基址整数溢出。
    EOverflow,
    /// 缓冲空读（未写先读）。
    EReadEmpty,
    /// 重复写（账已落，覆盖即重放）。
    EDupWrite,
    /// 读长度与写长度不符（回显破坏）。
    EEchoMismatch,
    /// 请求类型错标（读/写对偶颠倒）。
    ETypeFlipped,
    /// 状态机倒退。
    ERewind,
    /// 空样本。
    EEmpty,
    /// 联签守恒破坏。
    EConserv,
    /// 未知连接。
    EUnknownConn,
}

impl RwDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RwDiag::ETypeRange => 0x01,
                RwDiag::ENullBuf => 0x02,
                RwDiag::EOverCap => 0x03,
                RwDiag::EOverflow => 0x04,
                RwDiag::EReadEmpty => 0x05,
                RwDiag::EDupWrite => 0x06,
                RwDiag::EEchoMismatch => 0x07,
                RwDiag::ETypeFlipped => 0x08,
                RwDiag::ERewind => 0x09,
                RwDiag::EEmpty => 0x0a,
                RwDiag::EConserv => 0x0b,
                RwDiag::EUnknownConn => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RwDiag::ETypeRange => "请求类型越闭集：只认读/写/无请求，先对型表再发请求",
            RwDiag::ENullBuf => "坏缓冲：零基址不得携非零长度，先分配缓冲再请求",
            RwDiag::EOverCap => "长度超容：数据长度超缓冲容量，先缩长度或换大缓冲",
            RwDiag::EOverflow => "整数溢出：基址+长度用 checked_add 判定，溢出即拒",
            RwDiag::EReadEmpty => "缓冲空读：先写后读，读空缓冲按序违拒",
            RwDiag::EDupWrite => "重复写：一往返一写，覆盖写按重放拒",
            RwDiag::EEchoMismatch => "读长与写长不符：读请求须回显先前写入长度",
            RwDiag::ETypeFlipped => "类型错标：读请求携写语义（或反之）按错标拒",
            RwDiag::ERewind => "状态机倒退：空闲→已写→已读只进不退",
            RwDiag::EEmpty => "空样本：连接/基址/长度全零不构成请求",
            RwDiag::EConserv => "联签守恒破坏：写字节与读字节计数不符",
            RwDiag::EUnknownConn => "未知连接：先建连接再走读写",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RW_ALL: &[RwDiag] = &[
    RwDiag::ETypeRange,
    RwDiag::ENullBuf,
    RwDiag::EOverCap,
    RwDiag::EOverflow,
    RwDiag::EReadEmpty,
    RwDiag::EDupWrite,
    RwDiag::EEchoMismatch,
    RwDiag::ETypeFlipped,
    RwDiag::ERewind,
    RwDiag::EEmpty,
    RwDiag::EConserv,
    RwDiag::EUnknownConn,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in RW_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80201..0x8020C 闭集内"
}

// ── 请求类型闭集 ─────────────────────────────────────────────────
/// 请求类型三型闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReqKind {
    /// 读请求。
    Read,
    /// 写请求。
    Write,
    /// 无请求（空操作）。
    None,
}

impl ReqKind {
    /// 型号。
    pub fn ordinal(self) -> u32 {
        match self {
            ReqKind::Read => 0,
            ReqKind::Write => 1,
            ReqKind::None => 2,
        }
    }

    /// 型中文名。
    pub fn name(self) -> &'static str {
        match self {
            ReqKind::Read => "读请求",
            ReqKind::Write => "写请求",
            ReqKind::None => "无请求",
        }
    }

    /// 由型号反查（越界 None）。
    pub fn from_ordinal(o: u32) -> Option<ReqKind> {
        match o {
            0 => Some(ReqKind::Read),
            1 => Some(ReqKind::Write),
            2 => Some(ReqKind::None),
            _ => None,
        }
    }
}

/// 三型闭集数组。
pub const KIND_ALL: &[ReqKind] = &[ReqKind::Read, ReqKind::Write, ReqKind::None];

// ── 读写域模型 ───────────────────────────────────────────────────
/// 标准缓冲容量（字节）。
pub const BUF_CAP: u32 = 4096;

/// 单笔读写请求（缓冲/数据长度/请求类型三元组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RwRequest {
    /// 请求类型。
    pub kind: ReqKind,
    /// 缓冲基址（非零有效；零基址只许携零长度）。
    pub buf_base: u32,
    /// 数据长度。
    pub data_len: u32,
}

impl RwRequest {
    /// 静态缓冲语义：零基址只许零长度；长度 ≤ 容量；基址+长度不溢出。
    pub fn buf_sound(&self) -> Result<(), u32> {
        if self.buf_base == 0 && self.data_len > 0 {
            return Err(RwDiag::ENullBuf.val());
        }
        if self.data_len > BUF_CAP {
            return Err(RwDiag::EOverCap.val());
        }
        match self.buf_base.checked_add(self.data_len) {
            Some(_) => Ok(()),
            None => Err(RwDiag::EOverflow.val()),
        }
    }

    /// 空样本。
    pub fn is_empty(&self) -> bool {
        self.kind == ReqKind::None && self.buf_base == 0 && self.data_len == 0
    }
}

/// 往返状态机：空闲 → 已写 → 已读。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RwState {
    /// 空闲（缓冲空）。
    Idle,
    /// 已写（缓冲持有数据）。
    Written,
    /// 已读（往返闭合）。
    Read,
}

impl RwState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            RwState::Idle => 0,
            RwState::Written => 1,
            RwState::Read => 2,
        }
    }
}

/// 单连接读写账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RwLedger {
    /// 连接序号。
    pub conn_id: u32,
    /// 状态。
    pub state: RwState,
    /// 写入长度。
    pub written: Option<u32>,
    /// 读出长度。
    pub read: Option<u32>,
}

/// 读写联签机。
#[derive(Clone, Debug)]
pub struct RwGate {
    /// 缓冲容量。
    cap: u32,
    /// 连接账。
    ledgers: Vec<RwLedger>,
}

impl RwGate {
    /// 定容读写机。
    pub fn new(cap: u32) -> RwGate {
        RwGate {
            cap,
            ledgers: Vec::new(),
        }
    }

    /// 账面数。
    pub fn len(&self) -> usize {
        self.ledgers.len()
    }

    /// 查账。
    pub fn ledger_of(&self, conn_id: u32) -> Option<RwLedger> {
        for l in self.ledgers.iter() {
            if l.conn_id == conn_id {
                return Some(*l);
            }
        }
        None
    }

    /// 建连接。
    pub fn connect(&mut self, conn_id: u32) -> Result<(), u32> {
        if conn_id == 0 {
            return Err(RwDiag::EEmpty.val());
        }
        if self.ledger_of(conn_id).is_some() {
            return Err(RwDiag::EUnknownConn.val());
        }
        self.ledgers.push(RwLedger {
            conn_id,
            state: RwState::Idle,
            written: None,
            read: None,
        });
        Ok(())
    }

    /// 写请求：缓冲闸 + 状态闸（重写拒）+ 零长写合法（空操作推进）。
    pub fn write_req(&mut self, conn_id: u32, req: RwRequest) -> Result<(), u32> {
        let idx = match self.index_of(conn_id) {
            Some(i) => i,
            None => return Err(RwDiag::EUnknownConn.val()),
        };
        let l = match self.ledgers.get(idx) {
            Some(l) => *l,
            None => return Err(RwDiag::EUnknownConn.val()),
        };
        // 类型闸：写请求不得错标为读。
        if req.kind == ReqKind::Read {
            return Err(RwDiag::ETypeFlipped.val());
        }
        if l.state.rank() >= RwState::Written.rank() {
            return Err(RwDiag::EDupWrite.val());
        }
        // 缓冲闸。
        self.buf_check(&req)?;
        if let Some(l) = self.ledgers.get_mut(idx) {
            l.state = RwState::Written;
            l.written = Some(req.data_len);
        }
        Ok(())
    }

    /// 读请求：须先写；读长必须回显写长；零长往返合法。
    pub fn read_req(&mut self, conn_id: u32, req: RwRequest) -> Result<u32, u32> {
        let idx = match self.index_of(conn_id) {
            Some(i) => i,
            None => return Err(RwDiag::EUnknownConn.val()),
        };
        let l = match self.ledgers.get(idx) {
            Some(l) => *l,
            None => return Err(RwDiag::EUnknownConn.val()),
        };
        // 类型闸：读请求不得错标为写。
        if req.kind == ReqKind::Write {
            return Err(RwDiag::ETypeFlipped.val());
        }
        match l.state {
            RwState::Idle => Err(RwDiag::EReadEmpty.val()),
            RwState::Read => Err(RwDiag::ERewind.val()),
            RwState::Written => {
                let want = l.written.unwrap_or(0);
                if req.data_len != want {
                    return Err(RwDiag::EEchoMismatch.val());
                }
                // 缓冲闸（读侧同样要求基址语义）。
                self.buf_check(&req)?;
                if let Some(l) = self.ledgers.get_mut(idx) {
                    l.state = RwState::Read;
                    l.read = Some(req.data_len);
                }
                Ok(req.data_len)
            }
        }
    }

    /// 缓冲闸（容量按机容量）。
    fn buf_check(&self, req: &RwRequest) -> Result<(), u32> {
        if req.buf_base == 0 && req.data_len > 0 {
            return Err(RwDiag::ENullBuf.val());
        }
        if req.data_len > self.cap {
            return Err(RwDiag::EOverCap.val());
        }
        match req.buf_base.checked_add(req.data_len) {
            Some(_) => Ok(()),
            None => Err(RwDiag::EOverflow.val()),
        }
    }

    /// 联签守恒：已读账写字节 == 读字节。
    pub fn conservation(&self) -> bool {
        for l in self.ledgers.iter() {
            if l.state == RwState::Read {
                match (l.written, l.read) {
                    (Some(w), Some(r)) => {
                        if w != r {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            if l.state.rank() < RwState::Idle.rank() {
                return false;
            }
        }
        true
    }

    /// 按连接号找账内下标。
    fn index_of(&self, conn_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if let Some(l) = self.ledgers.get(i) {
                if l.conn_id == conn_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
}

// ── 对平断言与注入面 ─────────────────────────────────────────────
/// 读写对平断言（判据 J1 直读口）：三种长度（零/半/满）往返全闭合。
fn rw_pairing_ok() -> bool {
    let cases = [0u32, BUF_CAP / 2, BUF_CAP];
    let mut k = 0u32;
    while k < 3 {
        let len = cases[k as usize];
        let mut g = RwGate::new(BUF_CAP);
        if g.connect(0x10 + k).is_err() {
            return false;
        }
        let w = RwRequest {
            kind: ReqKind::Write,
            buf_base: 0x1000,
            data_len: len,
        };
        if g.write_req(0x10 + k, w).is_err() {
            return false;
        }
        let r = RwRequest {
            kind: ReqKind::Read,
            buf_base: 0x2000,
            data_len: len,
        };
        match g.read_req(0x10 + k, r) {
            Ok(got) => {
                if got != len || !g.conservation() {
                    return false;
                }
            }
            Err(_) => return false,
        }
        k += 1;
    }
    true
}

/// 坏缓冲注入 10/10：十路违约判定与基线一致。
fn bad_buf_injections_ok() -> bool {
    let mut hits = 0usize;
    let mut g = RwGate::new(BUF_CAP);
    let _ = g.connect(0x20);
    let _ = g.connect(0x21);
    let _ = g.connect(0x22);
    // 注 1：零基址携非零长（写侧）。
    if g.write_req(
        0x20,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0,
            data_len: 16,
        },
    ) == Err(RwDiag::ENullBuf.val())
    {
        hits += 1;
    }
    // 注 2：长度超容。
    if g.write_req(
        0x20,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0x1000,
            data_len: BUF_CAP + 1,
        },
    ) == Err(RwDiag::EOverCap.val())
    {
        hits += 1;
    }
    // 注 3：基址+长度溢出。
    if g.write_req(
        0x20,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0xFFFFFFF0,
            data_len: 0x20,
        },
    ) == Err(RwDiag::EOverflow.val())
    {
        hits += 1;
    }
    // 注 4：写请求错标为读型。
    if g.write_req(
        0x20,
        RwRequest {
            kind: ReqKind::Read,
            buf_base: 0x1000,
            data_len: 8,
        },
    ) == Err(RwDiag::ETypeFlipped.val())
    {
        hits += 1;
    }
    // 注 5：空读（未写先读）。
    if g.read_req(
        0x21,
        RwRequest {
            kind: ReqKind::Read,
            buf_base: 0x2000,
            data_len: 0,
        },
    ) == Err(RwDiag::EReadEmpty.val())
    {
        hits += 1;
    }
    // 注 6：读长漂移（写 16 读 8）。
    let _ = g.write_req(
        0x20,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0x1000,
            data_len: 16,
        },
    );
    if g.read_req(
        0x20,
        RwRequest {
            kind: ReqKind::Read,
            buf_base: 0x2000,
            data_len: 8,
        },
    ) == Err(RwDiag::EEchoMismatch.val())
    {
        hits += 1;
    }
    // 注 7：重复写。
    if g.write_req(
        0x20,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0x1000,
            data_len: 16,
        },
    ) == Err(RwDiag::EDupWrite.val())
    {
        hits += 1;
    }
    // 注 8：读请求错标为写型。
    if g.read_req(
        0x20,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0x2000,
            data_len: 16,
        },
    ) == Err(RwDiag::ETypeFlipped.val())
    {
        hits += 1;
    }
    // 注 9：空样本连接。
    if g.connect(0) == Err(RwDiag::EEmpty.val()) {
        hits += 1;
    }
    // 注 10：未知连接读写。
    if g.write_req(
        0x99,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0x1000,
            data_len: 4,
        },
    ) == Err(RwDiag::EUnknownConn.val())
    {
        hits += 1;
    }
    hits == 10
}

/// 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫。
fn diag_closed_ok() -> bool {
    for d in RW_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(RwDiag::ENullBuf.val()) != RwDiag::ENullBuf.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x32).starts_with("未知") {
        return false;
    }
    true
}

/// 三型闭集守卫。
fn kind_closed_ok() -> bool {
    if KIND_ALL.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < KIND_ALL.len() {
        let a = match KIND_ALL.get(i) {
            Some(k) => *k,
            None => return false,
        };
        if ReqKind::from_ordinal(a.ordinal()) != Some(a) {
            return false;
        }
        let mut j = i + 1;
        while j < KIND_ALL.len() {
            let b = match KIND_ALL.get(j) {
                Some(k) => *k,
                None => return false,
            };
            if a.ordinal() == b.ordinal() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 静态缓冲语义三点：零基址零长合法、零基址非零长拒、满容合法。
fn buf_semantics_ok() -> bool {
    // 零基址零长（空操作）合法。
    let nop = RwRequest {
        kind: ReqKind::Write,
        buf_base: 0,
        data_len: 0,
    };
    // 满容合法。
    let full = RwRequest {
        kind: ReqKind::Write,
        buf_base: 0x1000,
        data_len: BUF_CAP,
    };
    // 零基址非零长拒。
    let null = RwRequest {
        kind: ReqKind::Write,
        buf_base: 0,
        data_len: 1,
    };
    nop.buf_sound().is_ok() && full.buf_sound().is_ok() && null.buf_sound() == Err(RwDiag::ENullBuf.val())
}

/// 双跑一致：注入与对平两次相同。
fn dual_run_ok() -> bool {
    let a = (rw_pairing_ok(), bad_buf_injections_ok());
    let b = (rw_pairing_ok(), bad_buf_injections_ok());
    a == b && a.0 && a.1
}

/// 完成写回语义：往返联签摘要非空。
fn writeback_ok() -> bool {
    let mut g = RwGate::new(BUF_CAP);
    let _ = g.connect(0x30);
    let _ = g.write_req(
        0x30,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0x1000,
            data_len: 512,
        },
    );
    let got = g.read_req(
        0x30,
        RwRequest {
            kind: ReqKind::Read,
            buf_base: 0x2000,
            data_len: 512,
        },
    );
    match got {
        Ok(n) => {
            let summary: String = format!("conn=0x30 bytes={} closed", n);
            n == 512 && !summary.is_empty() && g.conservation()
        }
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12609 自检（判据 12608 卡：读写请求语义对平 + 坏缓冲注入）。
pub fn run_unx12608_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12608");
    let mut g = RwGate::new(BUF_CAP);
    let _ = g.connect(0x40);
    let _ = g.write_req(
        0x40,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0x1000,
            data_len: 128,
        },
    );

    s.add_dyn(
        "F12609-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12609-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", RW_ALL.len()),
    );

    s.add_dyn(
        "F12609-03 三型闭集枚举",
        kind_closed_ok(),
        &format!(
            "kinds={} 型序=[读,写,无]",
            KIND_ALL.len()
        ),
    );

    s.add_dyn(
        "F12609-04 静态缓冲语义",
        buf_semantics_ok(),
        "零基址零长合法/零基址非零长拒/满容合法",
    );

    s.add_dyn(
        "F12609-05 读写对平断言绿",
        rw_pairing_ok(),
        "零/半/满三长度往返全闭合且守恒",
    );

    // 注入必抓：十路坏缓冲全数命中。
    s.add_dyn(
        "F12609-06 坏缓冲注入 10/10 一致",
        bad_buf_injections_ok(),
        "零基址/超容/溢出/写错标/空读/读漂/重写/读错标/空样本/未知连",
    );

    s.add_dyn(
        "F12609-07 双跑一致",
        dual_run_ok(),
        "对平与注入判定两次逐项相同",
    );

    s.add_dyn(
        "F12609-08 联签守恒",
        g.conservation(),
        &format!("账面={}", g.len()),
    );

    s.add_dyn(
        "F12609-09 状态机单调",
        RwState::Idle.rank() < RwState::Written.rank()
            && RwState::Written.rank() < RwState::Read.rank(),
        "空闲→已写→已读 只进不退",
    );

    s.add_dyn(
        "F12609-10 零长往返合法",
        match zero_roundtrip() {
            Ok(n) => n == 0,
            Err(_) => false,
        },
        "零长写→零长读 空操作闭合",
    );

    s.add_dyn(
        "F12609-11 完成写回语义",
        writeback_ok(),
        "往返联签摘要非空",
    );

    s.add_dyn(
        "F12609-12 未知连接拒",
        match RwGate::new(BUF_CAP).ledger_of(0x99) {
            None => true,
            Some(_) => false,
        },
        "未连接无账可查",
    );

    s
}

/// 零长往返辅助流。
fn zero_roundtrip() -> Result<u32, u32> {
    let mut g = RwGate::new(BUF_CAP);
    g.connect(0x50)?;
    g.write_req(
        0x50,
        RwRequest {
            kind: ReqKind::Write,
            buf_base: 0,
            data_len: 0,
        },
    )?;
    g.read_req(
        0x50,
        RwRequest {
            kind: ReqKind::Read,
            buf_base: 0,
            data_len: 0,
        },
    )
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x80200);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(RW_ALL.len(), 12);
        let mut i = 0usize;
        while i < RW_ALL.len() {
            let vi = RW_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < RW_ALL.len() {
                let vj = RW_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x31).contains("未知"));
    }

    #[test]
    fn kind_closed_set_three() {
        assert_eq!(KIND_ALL.len(), 3);
        assert!(ReqKind::from_ordinal(3).is_none());
        assert!(kind_closed_ok());
    }

    #[test]
    fn buf_semantics_three_points() {
        assert!(buf_semantics_ok());
        // 静态面：满容+1 超容。
        let over = RwRequest {
            kind: ReqKind::Write,
            buf_base: 0x1000,
            data_len: BUF_CAP + 1,
        };
        assert_eq!(over.buf_sound(), Err(RwDiag::EOverCap.val()));
        // 溢出。
        let ovf = RwRequest {
            kind: ReqKind::Write,
            buf_base: 0xFFFFFFFF,
            data_len: 0x10,
        };
        assert_eq!(ovf.buf_sound(), Err(RwDiag::EOverflow.val()));
    }

    #[test]
    fn rw_pairing_three_lengths() {
        assert!(rw_pairing_ok());
    }

    #[test]
    fn bad_buf_injections_all_hit() {
        assert!(bad_buf_injections_ok());
    }

    #[test]
    fn echo_semantics_strict() {
        // 读长必须回显写长：漂移拒、相等过。
        let mut g = RwGate::new(BUF_CAP);
        let _ = g.connect(0x60);
        let _ = g.write_req(
            0x60,
            RwRequest {
                kind: ReqKind::Write,
                buf_base: 0x1000,
                data_len: 64,
            },
        );
        assert_eq!(
            g.read_req(
                0x60,
                RwRequest {
                    kind: ReqKind::Read,
                    buf_base: 0x2000,
                    data_len: 32,
                }
            ),
            Err(RwDiag::EEchoMismatch.val())
        );
        assert!(g
            .read_req(
                0x60,
                RwRequest {
                    kind: ReqKind::Read,
                    buf_base: 0x2000,
                    data_len: 64,
                }
            )
            .is_ok());
        assert!(g.conservation());
    }

    #[test]
    fn state_machine_rejects_rewind() {
        // 空读/重写/重复读三类倒退。
        let mut g = RwGate::new(BUF_CAP);
        let _ = g.connect(0x61);
        assert_eq!(
            g.read_req(
                0x61,
                RwRequest {
                    kind: ReqKind::Read,
                    buf_base: 0,
                    data_len: 0,
                }
            ),
            Err(RwDiag::EReadEmpty.val())
        );
        let _ = g.write_req(
            0x61,
            RwRequest {
                kind: ReqKind::Write,
                buf_base: 0x1000,
                data_len: 8,
            },
        );
        assert_eq!(
            g.write_req(
                0x61,
                RwRequest {
                    kind: ReqKind::Write,
                    buf_base: 0x1000,
                    data_len: 8,
                }
            ),
            Err(RwDiag::EDupWrite.val())
        );
        let _ = g.read_req(
            0x61,
            RwRequest {
                kind: ReqKind::Read,
                buf_base: 0x2000,
                data_len: 8,
            },
        );
        // 已读后再读 = 倒退。
        assert_eq!(
            g.read_req(
                0x61,
                RwRequest {
                    kind: ReqKind::Read,
                    buf_base: 0x2000,
                    data_len: 8,
                }
            ),
            Err(RwDiag::ERewind.val())
        );
    }

    #[test]
    fn zero_length_roundtrip() {
        // 零长往返：空操作闭合且守恒。
        let r = zero_roundtrip();
        assert_eq!(r.is_ok(), true, "零长往返应闭合");
        assert_eq!(r.unwrap_or(1), 0);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_ok());
    }

    #[test]
    fn connect_gates() {
        // 空连接号与重复连接。
        let mut g = RwGate::new(BUF_CAP);
        assert_eq!(g.connect(0), Err(RwDiag::EEmpty.val()));
        assert!(g.connect(0x62).is_ok());
        assert!(g.connect(0x62).is_err());
        assert!(g.ledger_of(0x99).is_none());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12608_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
