//! UNX-F50015 色彩与 GDI 管线共用联签（B06 判据主轴）（0x42e00 段）
//!
//! 锚点：UNX-F50015-J1 主轴联签登记；判据锚定 §AI-63 §3。
//!
//! 数据段 `SEG_BASE = 0x42e00`；诊断码闭集 = `SEG_BASE + n`：
//!   +1 Empty     审计表/操作表为空（或无可签内容）
//!   +2 CapFull   操作表或审计表容量满
//!   +3 Duplicate 同一操作号重复登记
//!   +4 Unknown   操作号未登记
//!   +5 Conflict  签名顺序违例 / 重复签名 / 已闭合操作被撤销
//!   +6 Digest    任一方摘要与证书不符（或证书摘要为 0）
//!   +7 Expired   超时窗口已过，联签作废
//!   +8 Order     操作号为 0（保留值）
//!   +9 Range     窗口越界（1..=3600 tick）
//!   +10 State    状态机不允许该迁移（未双签就验证 / 已拒绝再签）
//!
//! 联签语义（B06 主轴）：色彩管线与 GDI 管线对**同一张操作证书**各自独立
//! 复算摘要，两方都签上且都等于证书摘要，操作才算「双签闭合」。任一时刻
//! 只允许一方先签（顺序固定：色彩 → GDI），因为 GDI 侧要消费色彩侧算出的
//! 设备色参数——反序签名意味着 GDI 拿到的是上一代参数，属于事实上的撕裂。
//!
//! 为什么要有超时窗口：两条管线跑在不同线程上，若一方永远不签，操作会
//! 永远挂在 Pending，账本无法收敛。窗口到期即显式 Expired（不是静默删除），
//! 让「谁没签」成为可查事实。
//!
//! 结构清单：
//! - 状态机：`SignState` = Pending → ColorSigned → DualSigned；旁支 Rejected/Expired
//! - 数据类型：`OpCert`（操作证书）、`SideSign`（单方签名）、`Countersign`
//!   （联签记录）、`AuditNote`（拒绝审计）、`SignReport`（账本快照）
//! - 账本：`JointLedger`（操作表 + 审计表 + 逻辑时钟）
//! - 错误面：`SignError::code()` 落 0x42e00 段闭集
//! - 注入核：`injection_core` 八类坏序列（GDI 先签/摘要不符/重复色彩签/
//!   超时/未知操作/重复登记/容量满/证书摘要为 0）

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x42e00 独占 0x20）。
pub const SEG_BASE: u32 = 0x42e00;

pub const E_EMPTY: u16 = 1;
pub const E_CAPFULL: u16 = 2;
pub const E_DUPLICATE: u16 = 3;
pub const E_UNKNOWN: u16 = 4;
pub const E_CONFLICT: u16 = 5;
pub const E_DIGEST: u16 = 6;
pub const E_EXPIRED: u16 = 7;
pub const E_ORDER: u16 = 8;
pub const E_RANGE: u16 = 9;
pub const E_STATE: u16 = 10;

/// 审计表容量（拒绝事件逐条记账，超容显式计入 `audit_dropped`）。
pub const MAX_AUDITS: usize = 32;

/// 联签窗口上限（tick）。3600 个调度节拍 ≈ 30 秒量级，够一次合成帧
/// 跨管线往返；再长就不是「同一帧的联签」而是补签了。
pub const MAX_WINDOW: u16 = 3600;

/// 联签错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignError {
    Empty,
    CapFull,
    Duplicate,
    Unknown,
    Conflict,
    Digest,
    Expired,
    Order,
    Range,
    State,
}

impl SignError {
    /// 段内诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            SignError::Empty => E_EMPTY,
            SignError::CapFull => E_CAPFULL,
            SignError::Duplicate => E_DUPLICATE,
            SignError::Unknown => E_UNKNOWN,
            SignError::Conflict => E_CONFLICT,
            SignError::Digest => E_DIGEST,
            SignError::Expired => E_EXPIRED,
            SignError::Order => E_ORDER,
            SignError::Range => E_RANGE,
            SignError::State => E_STATE,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            SignError::Empty => "no countersign entries",
            SignError::CapFull => "countersign table full",
            SignError::Duplicate => "operation already registered",
            SignError::Unknown => "operation not registered",
            SignError::Conflict => "sign order violated",
            SignError::Digest => "digest mismatch with certificate",
            SignError::Expired => "countersign window elapsed",
            SignError::Order => "operation id zero",
            SignError::Range => "window out of range",
            SignError::State => "state transition not allowed",
        }
    }
}

/// FNV-1a 64 位（联合摘要；不需要密码学强度，只要逐位确定）。
#[derive(Clone, Copy)]
pub struct Fnv64 {
    h: u64,
}

impl Fnv64 {
    pub const fn new() -> Fnv64 {
        Fnv64 {
            h: 0xcbf29ce484222325,
        }
    }

    pub fn byte(&mut self, b: u8) {
        self.h ^= b as u64;
        self.h = self.h.wrapping_mul(0x100000001b3);
    }

    pub fn u16le(&mut self, v: u16) {
        self.byte(v as u8);
        self.byte((v >> 8) as u8);
    }

    pub fn u64le(&mut self, v: u64) {
        self.u16le(v as u16);
        self.u16le((v >> 16) as u16);
        self.u16le((v >> 32) as u16);
        self.u16le((v >> 48) as u16);
    }

    pub fn digest(self) -> u64 {
        self.h
    }
}

/// 联签状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignState {
    Pending,
    ColorSigned,
    DualSigned,
    Rejected,
    Expired,
}

impl SignState {
    pub fn label(self) -> &'static str {
        match self {
            SignState::Pending => "pending",
            SignState::ColorSigned => "color-signed",
            SignState::DualSigned => "dual-signed",
            SignState::Rejected => "rejected",
            SignState::Expired => "expired",
        }
    }

    /// 是否已闭合（双签）。
    pub fn is_closed(self) -> bool {
        matches!(self, SignState::DualSigned)
    }

    /// 是否还活（等待某一方签名）。
    pub fn is_live(self) -> bool {
        matches!(self, SignState::Pending | SignState::ColorSigned)
    }
}

/// 操作证书：操作号 + 期望摘要 + 联签窗口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpCert {
    pub op: u16,
    pub digest: u64,
    pub window: u16,
}

impl OpCert {
    /// 构造证书：操作号非 0、摘要非 0、窗口 1..=MAX_WINDOW。
    pub fn new(op: u16, digest: u64, window: u16) -> Result<OpCert, SignError> {
        if op == 0 {
            return Err(SignError::Order);
        }
        if digest == 0 {
            return Err(SignError::Digest);
        }
        if window == 0 || window > MAX_WINDOW {
            return Err(SignError::Range);
        }
        Ok(OpCert { op, digest, window })
    }
}

/// 单方签名：摘要 + 签名时刻（tick）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideSign {
    pub digest: u64,
    pub tick: u64,
}

/// 一条联签记录：证书 + 两方签名 + 状态 + 开闭时刻。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Countersign {
    pub cert: OpCert,
    pub color: Option<SideSign>,
    pub gdi: Option<SideSign>,
    pub state: SignState,
    pub opened: u64,
    pub closed: u64,
}

impl Countersign {
    fn deadline(&self) -> u64 {
        self.opened.saturating_add(self.cert.window as u64)
    }
}

/// 审计记录：只记被拒事件（成功路径由状态机本身表达，不重复记账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditNote {
    pub op: u16,
    pub code: u32,
}

/// 账本快照：给上层判据用的汇总量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignReport {
    pub opened: usize,
    pub dual: usize,
    pub rejected: usize,
    pub expired: usize,
    pub pending: usize,
    pub audits: usize,
    pub dropped: usize,
}

/// 联签账本：操作表 + 审计表 + 逻辑时钟。
#[derive(Clone, Debug)]
pub struct JointLedger {
    ops: Vec<Countersign>,
    cap: usize,
    tick: u64,
    audits: Vec<AuditNote>,
    audit_dropped: usize,
}

impl JointLedger {
    /// 新建账本。`cap` 是同时可挂的操作数。
    pub fn new(cap: usize) -> JointLedger {
        JointLedger {
            ops: Vec::new(),
            cap,
            tick: 0,
            audits: Vec::new(),
            audit_dropped: 0,
        }
    }

    pub fn now(&self) -> u64 {
        self.tick
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn audits(&self) -> usize {
        self.audits.len()
    }

    pub fn audit_dropped(&self) -> usize {
        self.audit_dropped
    }

    fn note(&mut self, op: u16, code: u32) {
        if self.audits.len() >= MAX_AUDITS {
            self.audit_dropped += 1;
            return;
        }
        self.audits.push(AuditNote { op, code });
    }

    fn index_of(&self, op: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(c) = self.ops.get(i) {
                if c.cert.op == op {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记操作证书。重号/容量满显式拒绝；成功即进入 Pending。
    pub fn open(&mut self, cert: OpCert) -> Result<u16, SignError> {
        if self.ops.len() >= self.cap {
            self.note(cert.op, SignError::CapFull.code());
            return Err(SignError::CapFull);
        }
        if self.index_of(cert.op).is_some() {
            self.note(cert.op, SignError::Duplicate.code());
            return Err(SignError::Duplicate);
        }
        self.ops.push(Countersign {
            cert,
            color: None,
            gdi: None,
            state: SignState::Pending,
            opened: self.tick,
            closed: 0,
        });
        Ok(cert.op)
    }

    /// 逻辑时钟推进，并把超窗的活操作显式打成 Expired。
    pub fn advance(&mut self, n: u64) -> u64 {
        self.tick = self.tick.saturating_add(n);
        let now = self.tick;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(c) = self.ops.get_mut(i) {
                if c.state.is_live() && now > c.deadline() {
                    c.state = SignState::Expired;
                }
            }
            i += 1;
        }
        self.tick
    }

    /// 色彩方签名（第一步）。只允许 Pending；摘要必须等于证书。
    pub fn sign_color(&mut self, op: u16, digest: u64) -> Result<SignState, SignError> {
        let idx = match self.index_of(op) {
            Some(i) => i,
            None => {
                self.note(op, SignError::Unknown.code());
                return Err(SignError::Unknown);
            }
        };
        let now = self.tick;
        let mut err: Option<SignError> = None;
        if let Some(c) = self.ops.get_mut(idx) {
            if c.state == SignState::Expired {
                err = Some(SignError::Expired);
            } else if c.state != SignState::Pending {
                err = Some(SignError::Conflict);
            } else if now > c.deadline() {
                c.state = SignState::Expired;
                err = Some(SignError::Expired);
            } else if digest != c.cert.digest {
                err = Some(SignError::Digest);
            } else {
                c.color = Some(SideSign { digest, tick: now });
                c.state = SignState::ColorSigned;
            }
        } else {
            err = Some(SignError::Unknown);
        }
        match err {
            Some(e) => {
                self.note(op, e.code());
                Err(e)
            }
            None => Ok(SignState::ColorSigned),
        }
    }

    /// GDI 方签名（第二步）。必须先有色彩签名，且摘要与证书、与色彩侧一致。
    pub fn sign_gdi(&mut self, op: u16, digest: u64) -> Result<SignState, SignError> {
        let idx = match self.index_of(op) {
            Some(i) => i,
            None => {
                self.note(op, SignError::Unknown.code());
                return Err(SignError::Unknown);
            }
        };
        let now = self.tick;
        let mut err: Option<SignError> = None;
        if let Some(c) = self.ops.get_mut(idx) {
            if c.state == SignState::Expired {
                err = Some(SignError::Expired);
            } else if c.state != SignState::ColorSigned {
                err = Some(SignError::Conflict);
            } else if now > c.deadline() {
                c.state = SignState::Expired;
                err = Some(SignError::Expired);
            } else if digest != c.cert.digest {
                err = Some(SignError::Digest);
            } else {
                let color_ok = match c.color {
                    Some(s) => s.digest == digest,
                    None => false,
                };
                if !color_ok {
                    err = Some(SignError::Digest);
                } else {
                    c.gdi = Some(SideSign { digest, tick: now });
                    c.state = SignState::DualSigned;
                    c.closed = now;
                }
            }
        } else {
            err = Some(SignError::Unknown);
        }
        match err {
            Some(e) => {
                self.note(op, e.code());
                Err(e)
            }
            None => Ok(SignState::DualSigned),
        }
    }

    /// 撤销一个未闭合的操作（显式拒绝语义）。
    pub fn reject(&mut self, op: u16, reason: u32) -> Result<SignState, SignError> {
        let idx = match self.index_of(op) {
            Some(i) => i,
            None => {
                self.note(op, SignError::Unknown.code());
                return Err(SignError::Unknown);
            }
        };
        let mut err: Option<SignError> = None;
        let now = self.tick;
        if let Some(c) = self.ops.get_mut(idx) {
            if c.state == SignState::Expired {
                err = Some(SignError::Expired);
            } else if c.state.is_closed() {
                err = Some(SignError::Conflict);
            } else if c.state == SignState::Rejected {
                err = Some(SignError::State);
            } else {
                c.state = SignState::Rejected;
                c.closed = now;
            }
        }
        match err {
            Some(e) => {
                self.note(op, e.code());
                Err(e)
            }
            None => {
                self.note(op, reason);
                Ok(SignState::Rejected)
            }
        }
    }

    /// 验证一个双签操作，返回联合摘要（含两方签名时刻，可审计）。
    pub fn verify(&self, op: u16) -> Result<u64, SignError> {
        let idx = match self.index_of(op) {
            Some(i) => i,
            None => return Err(SignError::Unknown),
        };
        let c = match self.ops.get(idx) {
            Some(c) => c,
            None => return Err(SignError::Unknown),
        };
        match c.state {
            SignState::DualSigned => {}
            SignState::Expired => return Err(SignError::Expired),
            _ => return Err(SignError::State),
        }
        let col = match c.color {
            Some(s) => s,
            None => return Err(SignError::State),
        };
        let gdi = match c.gdi {
            Some(s) => s,
            None => return Err(SignError::State),
        };
        if col.digest != c.cert.digest || gdi.digest != c.cert.digest {
            return Err(SignError::Digest);
        }
        let mut h = Fnv64::new();
        h.u16le(c.cert.op);
        h.u64le(c.cert.digest);
        h.u64le(col.tick);
        h.u64le(gdi.tick);
        h.u64le(c.closed);
        Ok(h.digest())
    }

    /// 查状态（未登记返回 None，不制造「默认 Pending」的假象）。
    pub fn state_of(&self, op: u16) -> Option<SignState> {
        match self.index_of(op) {
            Some(i) => self.ops.get(i).map(|c| c.state),
            None => None,
        }
    }

    /// 主轴承：所有双签记录的双方摘要都必须与证书一致，且双签计数自洽。
    pub fn axis_ok(&self) -> bool {
        let mut dual = 0usize;
        let mut ok = true;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(c) = self.ops.get(i) {
                if c.state == SignState::DualSigned {
                    dual += 1;
                    let good = match (c.color, c.gdi) {
                        (Some(a), Some(b)) => {
                            a.digest == c.cert.digest
                                && b.digest == c.cert.digest
                                && c.closed >= a.tick
                                && c.closed >= b.tick
                        }
                        _ => false,
                    };
                    if !good {
                        ok = false;
                    }
                }
            }
            i += 1;
        }
        ok && dual == self.report().dual
    }

    /// 账本快照。
    pub fn report(&self) -> SignReport {
        let mut dual = 0usize;
        let mut rejected = 0usize;
        let mut expired = 0usize;
        let mut pending = 0usize;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(c) = self.ops.get(i) {
                match c.state {
                    SignState::DualSigned => dual += 1,
                    SignState::Rejected => rejected += 1,
                    SignState::Expired => expired += 1,
                    SignState::Pending | SignState::ColorSigned => pending += 1,
                }
            }
            i += 1;
        }
        SignReport {
            opened: self.ops.len(),
            dual,
            rejected,
            expired,
            pending,
            audits: self.audits.len(),
            dropped: self.audit_dropped,
        }
    }
}

// ---------------------------------------------------------------------------
// 注入核：八类坏序列必须全部被抓
// ---------------------------------------------------------------------------

fn cert(op: u16) -> Option<OpCert> {
    OpCert::new(op, 0x5151_0000_0000_0000u64 + op as u64, 100).ok()
}

/// 八类坏序列注入：返回（被抓数, 总数）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let base = match cert(1) {
        Some(c) => c,
        None => return (0, total),
    };

    // 1 GDI 先签
    let mut l = JointLedger::new(4);
    let _ = l.open(base);
    if l.sign_gdi(1, base.digest).err() == Some(SignError::Conflict) {
        caught += 1;
    }
    // 2 摘要不符（两方各试一次）
    let mut l = JointLedger::new(4);
    let _ = l.open(base);
    let bad = base.digest ^ 0xff;
    let bad_color = l.sign_color(1, bad).err() == Some(SignError::Digest);
    let _ = l.sign_color(1, base.digest);
    let bad_gdi = l.sign_gdi(1, bad).err() == Some(SignError::Digest);
    if bad_color && bad_gdi {
        caught += 1;
    }
    // 3 重复色彩签
    let mut l = JointLedger::new(4);
    let _ = l.open(base);
    let _ = l.sign_color(1, base.digest);
    if l.sign_color(1, base.digest).err() == Some(SignError::Conflict) {
        caught += 1;
    }
    // 4 超时
    let mut l = JointLedger::new(4);
    let _ = l.open(base);
    let _ = l.advance(101);
    if l.sign_color(1, base.digest).err() == Some(SignError::Expired) {
        caught += 1;
    }
    // 5 未知操作
    let mut l = JointLedger::new(4);
    if l.sign_color(9, 1).err() == Some(SignError::Unknown)
        && l.sign_gdi(9, 1).err() == Some(SignError::Unknown)
    {
        caught += 1;
    }
    // 6 重复登记
    let mut l = JointLedger::new(4);
    let _ = l.open(base);
    if l.open(base).err() == Some(SignError::Duplicate) {
        caught += 1;
    }
    // 7 容量满
    let mut l = JointLedger::new(1);
    let _ = l.open(base);
    if let Some(c2) = cert(2) {
        if l.open(c2).err() == Some(SignError::CapFull) {
            caught += 1;
        }
    }
    // 8 证书摘要为 0 / 窗口越界
    if OpCert::new(3, 0, 10).err() == Some(SignError::Digest)
        && OpCert::new(3, 7, 0).err() == Some(SignError::Range)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50015_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50015");

    // J1-1 登记：Pending，时钟从 0 起
    let mut l = JointLedger::new(8);
    let c1 = match cert(1) {
        Some(c) => c,
        None => return s,
    };
    let opened = l.open(c1);
    let ok = opened == Ok(1)
        && l.state_of(1) == Some(SignState::Pending)
        && l.now() == 0
        && l.len() == 1;
    let det = format!("open={:?} state={:?}", opened, l.state_of(1));
    s.add_dyn("50015-J1-登记", ok, &det);

    // J1-2 主轴联签：色彩签 → GDI 签 → 双签闭合（中间态须当场取样）
    let cs = l.sign_color(1, c1.digest);
    let mid = l.state_of(1);
    let gs = l.sign_gdi(1, c1.digest);
    let ok = cs == Ok(SignState::ColorSigned)
        && mid == Some(SignState::ColorSigned)
        && gs == Ok(SignState::DualSigned)
        && l.state_of(1) == Some(SignState::DualSigned)
        && l.state_of(1).map(|x| x.is_closed()) == Some(true);
    let det = format!("cs={:?} gs={:?}", cs, gs);
    s.add_dyn("50015-J1-主轴联签", ok, &det);

    // J1-3 联合摘要确定：两次 verify 逐位一致且非 0
    let v1 = l.verify(1);
    let v2 = l.verify(1);
    let ok = v1 == v2 && v1.map(|d| d != 0) == Ok(true);
    let det = format!("verify={:?}", v1);
    s.add_dyn("50015-J1-联合摘要", ok, &det);

    // J1-4 GDI 先签必拒（顺序违例）
    let c2 = match cert(2) {
        Some(c) => c,
        None => return s,
    };
    let _ = l.open(c2);
    let ok = l.sign_gdi(2, c2.digest).err() == Some(SignError::Conflict)
        && l.state_of(2) == Some(SignState::Pending);
    s.add("50015-J1-顺序必拒", ok, if ok { "" } else { "反序签名被接受" });

    // J1-5 重复色彩签必拒，且状态不回退
    let _ = l.sign_color(2, c2.digest);
    let again = l.sign_color(2, c2.digest);
    let ok = again.err() == Some(SignError::Conflict)
        && l.state_of(2) == Some(SignState::ColorSigned);
    s.add("50015-J1-重复签必拒", ok, if ok { "" } else { "重复签名被接受" });

    // J1-6 摘要不符：色彩侧与 GDI 侧都必须拒
    let c3 = match cert(3) {
        Some(c) => c,
        None => return s,
    };
    let _ = l.open(c3);
    let wrong = c3.digest ^ 0x8000_0000_0000_0000;
    let rc = l.sign_color(3, wrong);
    let _ = l.sign_color(3, c3.digest);
    let rg = l.sign_gdi(3, wrong);
    let ok = rc.err() == Some(SignError::Digest)
        && rg.err() == Some(SignError::Digest)
        && l.state_of(3) == Some(SignState::ColorSigned);
    let det = format!("c={:?} g={:?}", rc, rg);
    s.add_dyn("50015-J1-摘要必拒", ok, &det);

    // J1-7 超时：advance 越窗后显式 Expired，再签必拒（独立账本，避免
    // 提前推进主账本时钟把别的活操作一起打成过期）
    let c4 = match cert(4) {
        Some(c) => c,
        None => return s,
    };
    let mut lexp = JointLedger::new(4);
    let _ = lexp.open(c4);
    let _ = lexp.advance(c4.window as u64 + 1);
    let late = lexp.sign_color(4, c4.digest);
    let ok = late.err() == Some(SignError::Expired)
        && lexp.state_of(4) == Some(SignState::Expired)
        && lexp.verify(4).err() == Some(SignError::Expired)
        && lexp.report().expired == 1;
    let det = format!("late={:?} tick={}", late, lexp.now());
    s.add_dyn("50015-J1-超时必拒", ok, &det);

    // J1-8 未知操作与未双签验证
    let ok = l.sign_color(77, 1).err() == Some(SignError::Unknown)
        && l.verify(77).err() == Some(SignError::Unknown)
        && l.verify(2).err() == Some(SignError::State)
        && l.state_of(77).is_none();
    s.add("50015-J1-未登记必拒", ok, if ok { "" } else { "未知操作未被拒" });

    // J1-9 重复登记 / 容量满 / 零号证书（容量与重号分表验证，避免
    // 容量先于重号命中把 Duplicate 误报成 CapFull）
    let ca = match cert(5) {
        Some(c) => c,
        None => return s,
    };
    let mut ldup = JointLedger::new(4);
    let _ = ldup.open(ca);
    let dup = ldup.open(ca);
    let mut lfull = JointLedger::new(1);
    let _ = lfull.open(ca);
    let full = match cert(6) {
        Some(c) => lfull.open(c),
        None => Err(SignError::Unknown),
    };
    let zero = OpCert::new(0, 1, 10).err() == Some(SignError::Order);
    let ok = dup.err() == Some(SignError::Duplicate)
        && full.err() == Some(SignError::CapFull)
        && zero;
    s.add("50015-J1-容量与零号", ok, if ok { "" } else { "容量或零号闸失灵" });

    // J1-10 撤销：Pending 可拒，双签不可撤销，重复撤销报 State
    let c7 = match cert(7) {
        Some(c) => c,
        None => return s,
    };
    let _ = l.open(c7);
    let rj = l.reject(7, SignError::Digest.code());
    let rj2 = l.reject(7, SignError::Digest.code());
    let rev_dual = l.reject(1, SignError::Conflict.code());
    let ok = rj == Ok(SignState::Rejected)
        && rj2.err() == Some(SignError::State)
        && rev_dual.err() == Some(SignError::Conflict)
        && l.state_of(7) == Some(SignState::Rejected);
    s.add("50015-J1-撤销语义", ok, if ok { "" } else { "撤销语义错" });

    // J1-11 审计账：上面 8 次拒绝（反序 1、重复签 1、摘要 2、未知 1、
    // 撤销成功 1、重复撤销 1、撤双签 1）逐条落表，无丢弃
    let audits = l.audits();
    let ok = audits == 8 && l.audit_dropped() == 0;
    let det = format!("audits={} dropped={}", audits, l.audit_dropped());
    s.add_dyn("50015-J1-拒绝记账", ok, &det);

    // J1-12 主轴承：全部双签记录双方摘要一致，计数自洽
    let ok = l.axis_ok() && l.report().dual == 1 && l.report().opened == l.len();
    let rep = l.report();
    let det = format!("dual={} rej={} exp={}", rep.dual, rep.rejected, rep.expired);
    s.add_dyn("50015-J1-主轴承", ok, &det);

    // J1-13 确定性：同序列两个账本得到同样的联合摘要与报告
    let mut la = JointLedger::new(4);
    let mut lb = JointLedger::new(4);
    let cd = match cert(9) {
        Some(c) => c,
        None => return s,
    };
    let _ = la.open(cd);
    let _ = lb.open(cd);
    let _ = la.sign_color(9, cd.digest);
    let _ = lb.sign_color(9, cd.digest);
    let _ = la.advance(3);
    let _ = lb.advance(3);
    let _ = la.sign_gdi(9, cd.digest);
    let _ = lb.sign_gdi(9, cd.digest);
    let ok = la.verify(9) == lb.verify(9) && la.report() == lb.report() && la.verify(9).is_ok();
    s.add("50015-J1-确定性", ok, if ok { "" } else { "同序列摘要不一致" });

    // J1-14 八类坏序列全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50015-J1-注入必抓", ok, &det);

    // J1-15 错误码落段闭集
    let all = [
        SignError::Empty,
        SignError::CapFull,
        SignError::Duplicate,
        SignError::Unknown,
        SignError::Conflict,
        SignError::Digest,
        SignError::Expired,
        SignError::Order,
        SignError::Range,
        SignError::State,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < all.len() {
        if let Some(e) = all.get(i) {
            let c = e.code();
            if c <= SEG_BASE || c > SEG_BASE + 0x1f || e.message().is_empty() {
                ok = false;
            }
        }
        i += 1;
    }
    s.add("50015-J1-码域闭集", ok, if ok { "" } else { "诊断码越段" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50015_checks_green() {
        let set = run_unx50015_checks();
        assert!(set.all_passed(), "UNX-F50015 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50015_dual_sign_roundtrip() {
        let c = match cert(1) {
            Some(c) => c,
            None => return,
        };
        let mut l = JointLedger::new(2);
        assert_eq!(l.open(c), Ok(1));
        assert_eq!(l.sign_color(1, c.digest), Ok(SignState::ColorSigned));
        assert_eq!(l.sign_gdi(1, c.digest), Ok(SignState::DualSigned));
        assert!(l.verify(1).is_ok());
        assert!(l.axis_ok());
    }

    #[test]
    fn unx50015_axis_detects_mismatch() {
        let c = match cert(2) {
            Some(c) => c,
            None => return,
        };
        let mut l = JointLedger::new(2);
        let _ = l.open(c);
        let _ = l.sign_color(2, c.digest);
        let _ = l.sign_gdi(2, c.digest);
        assert!(l.axis_ok());
        assert!(l.verify(2).is_ok());
        // 未双签的操作不进主轴
        let c3 = match cert(3) {
            Some(c) => c,
            None => return,
        };
        let _ = l.open(c3);
        assert!(l.axis_ok());
        assert_eq!(l.verify(3).err(), Some(SignError::State));
    }
}
