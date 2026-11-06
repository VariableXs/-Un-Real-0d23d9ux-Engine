//! UNX-F50126 USB 与 AI-65 管道联签（上游接口零改写）（0x43be0 段）
//!
//! 锚点：UNX-F50126-J1 联签登记，AI-65 冻结接口消费留痕。
//!
//! USB 打印栈不是孤岛：报文最终要交给 AI-65 管道做版面/属性/沉降处理。AI-65 的
//! 接口是**冻结**的——字段名、字段顺序、版本号都不允许在本侧被改写。本模块因此
//! 只做两件事：
//!
//! 1. **联签登记**：每个上游生产者（role / attrib / sink / ledger）在把数据交给
//!    管道之前，先按「生产者 + 冻结接口摘要 + 负载令牌」生成一份联签；登记之后
//!    签名不再变化，消费多少次都不改写登记账。
//! 2. **消费留痕**：每一次消费都追加一条 `Trace`（序号、生产者、令牌、字节数、
//!    自身摘要）。留痕只增不改，`trace_digest()` 因而可以作为「消费确实发生过」
//!    的证据。
//!
//! 「零改写」的证据链有三条：
//! * `AI65_ABI` 是对冻结接口逐字节重算出的常量，任何字段/版本/顺序变动都会使
//!   `FrozenIface::digest()` 偏离该常量，`validate()` 直接拒绝；
//! * 登记账里保存的是 `AI65_ABI` 本身，消费不触碰它；
//! * `registry_digest()` 只覆盖接口摘要与登记项的核心字段，消费前后相等。
//!
//! 数据结构：
//! * `FrozenIface` —— 冻结接口描述（id / major / minor / fields）与自摘要；
//! * `JointSign` —— 联签（接口摘要 + 负载摘要 + 签名），可校验、可篡改检测；
//! * `RegEntry` / `Trace` —— 登记项与消费留痕（两本只增账）；
//! * `PipeRegistry` —— 登记 + 消费 + 审计 + 两个故障注入钩子；
//! * `PipeAudit` —— 登记/校验/消费/悬空/改写五项口径。
//!
//! 错误闭集：`SEG_BASE + n`（0x43be0 段），所有码值都被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43be0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43be0;

pub const E_EMPTY: u16 = 1;
pub const E_VERSION: u16 = 2;
pub const E_IFACE: u16 = 3;
pub const E_FIELD: u16 = 4;
pub const E_ALREADY: u16 = 5;
pub const E_ZERO_PAYLOAD: u16 = 6;
pub const E_SIG: u16 = 7;
pub const E_NO_SIGN: u16 = 8;
pub const E_CAPACITY: u16 = 9;
pub const E_REWRITE: u16 = 10;

/// 联签错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeError {
    Empty,
    Version,
    Iface,
    Field,
    Already,
    ZeroPayload,
    Sig,
    NoSign,
    Capacity,
    Rewrite,
}

impl PipeError {
    pub fn code(self) -> u32 {
        let n = match self {
            PipeError::Empty => E_EMPTY,
            PipeError::Version => E_VERSION,
            PipeError::Iface => E_IFACE,
            PipeError::Field => E_FIELD,
            PipeError::Already => E_ALREADY,
            PipeError::ZeroPayload => E_ZERO_PAYLOAD,
            PipeError::Sig => E_SIG,
            PipeError::NoSign => E_NO_SIGN,
            PipeError::Capacity => E_CAPACITY,
            PipeError::Rewrite => E_REWRITE,
        };
        SEG_BASE + n as u32
    }
}

// ---------------------------------------------------------------------------
// AI-65 冻结接口
// ---------------------------------------------------------------------------

pub const AI65_ID: &str = "ai65.print.pipeline";
pub const AI65_MAJOR: u16 = 1;
pub const AI65_MINOR: u16 = 4;
pub const AI65_FIELDS: [&str; 4] = ["role", "attrib", "sink", "ledger"];

/// 冻结接口的逐字节重算结果。
///
/// 重算口径（与 `FrozenIface::digest` 一一对应）：
/// FNV-1a 64 依次吞入 id 字节、major(u16 LE)、minor(u16 LE)、字段数(u16 LE)，
/// 然后每个字段吞入字段字节并以 0xff 作终止符。0xff 不在字段名的允许字符集内，
/// 因此 "role"+"attrib" 与 "roleattrib" 不会摘要相撞。
///
/// 该常量是联签侧的锚：所有登记项的 `iface_digest` 都必须等于它，否则视为
/// 上游接口被改写（`PipeError::Rewrite`）。
pub const AI65_ABI: u64 = 0x8be2_026c_f486_dbec;

/// 冻结接口描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenIface {
    pub id: &'static str,
    pub major: u16,
    pub minor: u16,
    pub fields: &'static [&'static str],
}

impl FrozenIface {
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv_bytes(h, self.id.as_bytes());
        h = fnv_u16(h, self.major);
        h = fnv_u16(h, self.minor);
        h = fnv_u16(h, self.fields.len() as u16);
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some(f) = self.fields.get(i) {
                h = fnv_bytes(h, f.as_bytes());
                h = fnv_u8(h, 0xff);
            }
            i += 1;
        }
        h
    }

    pub fn same_fields(&self, other: &[&str]) -> bool {
        if self.fields.len() != other.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.fields.len() {
            let a = self.fields.get(i);
            let b = other.get(i);
            match (a, b) {
                (Some(x), Some(y)) => {
                    if x != y {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 冻结接口自检：字段、id、版本、摘要四道闸。
    pub fn validate(&self) -> Result<(), PipeError> {
        if self.id.is_empty() {
            return Err(PipeError::Empty);
        }
        if !self.same_fields(&AI65_FIELDS) {
            return Err(PipeError::Field);
        }
        if self.id != AI65_ID {
            return Err(PipeError::Iface);
        }
        if self.major != AI65_MAJOR || self.minor != AI65_MINOR {
            return Err(PipeError::Version);
        }
        if self.digest() != AI65_ABI {
            return Err(PipeError::Iface);
        }
        Ok(())
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(self.id);
        out.push_str(&format!("/{}.{}", self.major, self.minor));
        out.push('[');
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some(f) = self.fields.get(i) {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(f);
            }
            i += 1;
        }
        out.push(']');
        out
    }
}

pub fn ai65_frozen() -> FrozenIface {
    FrozenIface {
        id: AI65_ID,
        major: AI65_MAJOR,
        minor: AI65_MINOR,
        fields: &AI65_FIELDS,
    }
}

// ---------------------------------------------------------------------------
// 联签
// ---------------------------------------------------------------------------

fn payload_digest(producer: &str, token: u64) -> u64 {
    let mut h = FNV_OFFSET;
    h = fnv_bytes(h, producer.as_bytes());
    h = fnv_u64(h, token);
    h
}

fn sign_of(producer: &str, iface_digest: u64, payload_digest: u64) -> u64 {
    let mut h = SIGN_SEED;
    h = fnv_bytes(h, producer.as_bytes());
    h = fnv_u64(h, iface_digest);
    h = fnv_u64(h, payload_digest);
    h
}

fn trace_digest_of(seq: u32, producer: &str, token: u64, bytes: u32) -> u64 {
    let mut h = FNV_OFFSET;
    h = fnv_u32(h, seq);
    h = fnv_bytes(h, producer.as_bytes());
    h = fnv_u64(h, token);
    h = fnv_u32(h, bytes);
    h
}

/// 一份联签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointSign {
    pub producer: &'static str,
    pub iface_digest: u64,
    pub payload_digest: u64,
    pub sig: u64,
}

impl JointSign {
    pub fn compute(producer: &'static str, iface_digest: u64, payload_digest: u64) -> JointSign {
        JointSign {
            producer,
            iface_digest,
            payload_digest,
            sig: sign_of(producer, iface_digest, payload_digest),
        }
    }

    pub fn verify(&self) -> bool {
        sign_of(self.producer, self.iface_digest, self.payload_digest) == self.sig
    }

    /// 用给定接口复核联签：接口摘要不符 → Iface；签名不符 → Sig。
    pub fn check(&self, iface: &FrozenIface) -> Result<(), PipeError> {
        if iface.digest() != self.iface_digest {
            return Err(PipeError::Iface);
        }
        if !self.verify() {
            return Err(PipeError::Sig);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 登记账与留痕账
// ---------------------------------------------------------------------------

/// 登记项：一个生产者一条，`seq` 是登记顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegEntry {
    pub seq: u32,
    pub producer: &'static str,
    pub iface_digest: u64,
    pub token: u64,
    pub payload_digest: u64,
    pub sign_digest: u64,
    pub consumed: u32,
    pub bytes: u64,
}

/// 消费留痕：只增不改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trace {
    pub seq: u32,
    pub producer: &'static str,
    pub token: u64,
    pub bytes: u32,
    pub digest: u64,
}

/// 审计口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipeAudit {
    pub registered: u32,
    pub verified: u32,
    pub consumed: u64,
    pub dangling: u32,
    pub rewrites: u32,
    pub digest_ok: bool,
}

impl PipeAudit {
    pub fn clean(&self) -> bool {
        self.dangling == 0 && self.rewrites == 0 && self.digest_ok && self.verified == self.registered
    }
}

// ---------------------------------------------------------------------------
// 联签登记表
// ---------------------------------------------------------------------------

pub const MAX_ENTRIES: usize = 64;

#[derive(Clone, Debug)]
pub struct PipeRegistry {
    iface: FrozenIface,
    entries: Vec<RegEntry>,
    traces: Vec<Trace>,
    capacity: usize,
    next_seq: u32,
    trace_seq: u32,
    rewrites: u32,
    forced_sig: Option<u64>,
    frozen_override: Option<u64>,
}

impl PipeRegistry {
    pub fn new(capacity: usize) -> Result<PipeRegistry, PipeError> {
        if capacity == 0 {
            return Err(PipeError::Empty);
        }
        if capacity > MAX_ENTRIES {
            return Err(PipeError::Capacity);
        }
        Ok(PipeRegistry {
            iface: ai65_frozen(),
            entries: Vec::new(),
            traces: Vec::new(),
            capacity,
            next_seq: 0,
            trace_seq: 0,
            rewrites: 0,
            forced_sig: None,
            frozen_override: None,
        })
    }

    pub fn iface(&self) -> &FrozenIface {
        &self.iface
    }

    pub fn effective_iface_digest(&self) -> u64 {
        match self.frozen_override {
            Some(v) => v,
            None => self.iface.digest(),
        }
    }

    pub fn rewrites(&self) -> u32 {
        self.rewrites
    }

    fn find(&self, producer: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.producer == producer {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn entry(&self, producer: &str) -> Option<&RegEntry> {
        match self.find(producer) {
            Some(i) => self.entries.get(i),
            None => None,
        }
    }

    pub fn trace_at(&self, i: usize) -> Option<&Trace> {
        self.traces.get(i)
    }

    pub fn trace_len(&self) -> usize {
        self.traces.len()
    }

    pub fn registered_len(&self) -> usize {
        self.entries.len()
    }

    /// 联签登记。返回登记序号（从 0 起）。
    pub fn register(&mut self, producer: &'static str, token: u64) -> Result<u32, PipeError> {
        if producer.is_empty() {
            return Err(PipeError::Empty);
        }
        if token == 0 {
            return Err(PipeError::ZeroPayload);
        }
        let eff = self.effective_iface_digest();
        if eff != AI65_ABI {
            self.rewrites = self.rewrites.saturating_add(1);
            return Err(PipeError::Rewrite);
        }
        if self.find(producer).is_some() {
            return Err(PipeError::Already);
        }
        if self.entries.len() >= self.capacity {
            return Err(PipeError::Capacity);
        }
        let pd = payload_digest(producer, token);
        let mut sig = sign_of(producer, AI65_ABI, pd);
        if let Some(v) = self.forced_sig {
            sig = v;
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(RegEntry {
            seq,
            producer,
            iface_digest: AI65_ABI,
            token,
            payload_digest: pd,
            sign_digest: sig,
            consumed: 0,
            bytes: 0,
        });
        Ok(seq)
    }

    /// 消费一次：令牌必须与登记一致，签名必须复核通过，然后追加留痕。
    pub fn consume(
        &mut self,
        producer: &'static str,
        token: u64,
        bytes: u32,
    ) -> Result<u32, PipeError> {
        if producer.is_empty() {
            return Err(PipeError::Empty);
        }
        if token == 0 || bytes == 0 {
            return Err(PipeError::ZeroPayload);
        }
        if self.effective_iface_digest() != AI65_ABI {
            self.rewrites = self.rewrites.saturating_add(1);
            return Err(PipeError::Rewrite);
        }
        let idx = match self.find(producer) {
            Some(i) => i,
            None => return Err(PipeError::NoSign),
        };
        let (stored_pd, stored_sig) = match self.entries.get(idx) {
            Some(e) => (e.payload_digest, e.sign_digest),
            None => return Err(PipeError::NoSign),
        };
        let pd = payload_digest(producer, token);
        if pd != stored_pd {
            return Err(PipeError::Sig);
        }
        if sign_of(producer, AI65_ABI, pd) != stored_sig {
            return Err(PipeError::Sig);
        }
        let tseq = self.trace_seq;
        self.trace_seq = self.trace_seq.saturating_add(1);
        let d = trace_digest_of(tseq, producer, token, bytes);
        if let Some(e) = self.entries.get_mut(idx) {
            e.consumed = e.consumed.saturating_add(1);
            e.bytes = e.bytes.saturating_add(bytes as u64);
        }
        self.traces.push(Trace {
            seq: tseq,
            producer,
            token,
            bytes,
            digest: d,
        });
        Ok(tseq)
    }

    /// 全量复核：接口被改写 → Rewrite；任一签名不符 → Sig。
    pub fn verify_all(&mut self) -> Result<usize, PipeError> {
        if self.effective_iface_digest() != AI65_ABI {
            self.rewrites = self.rewrites.saturating_add(1);
            return Err(PipeError::Rewrite);
        }
        let mut ok = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.iface_digest != AI65_ABI {
                    return Err(PipeError::Sig);
                }
                if sign_of(e.producer, e.iface_digest, e.payload_digest) != e.sign_digest {
                    return Err(PipeError::Sig);
                }
                ok += 1;
            }
            i += 1;
        }
        Ok(ok)
    }

    pub fn registry_digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv_u64(h, self.iface.digest());
        h = fnv_u16(h, self.entries.len() as u16);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv_u32(h, e.seq);
                h = fnv_bytes(h, e.producer.as_bytes());
                h = fnv_u64(h, e.iface_digest);
                h = fnv_u64(h, e.payload_digest);
                h = fnv_u64(h, e.sign_digest);
            }
            i += 1;
        }
        h
    }

    pub fn trace_digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv_u16(h, self.traces.len() as u16);
        let mut i = 0usize;
        while i < self.traces.len() {
            if let Some(t) = self.traces.get(i) {
                h = fnv_u64(h, t.digest);
            }
            i += 1;
        }
        h
    }

    pub fn audit(&self) -> PipeAudit {
        let mut verified = 0u32;
        let mut consumed = 0u64;
        let mut dangling = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if sign_of(e.producer, e.iface_digest, e.payload_digest) == e.sign_digest {
                    verified = verified.saturating_add(1);
                }
                consumed = consumed.saturating_add(e.consumed as u64);
                if e.consumed == 0 {
                    dangling = dangling.saturating_add(1);
                }
            }
            i += 1;
        }
        let digest_ok = self.effective_iface_digest() == AI65_ABI
            && verified as usize == self.entries.len();
        PipeAudit {
            registered: self.entries.len() as u32,
            verified,
            consumed,
            dangling,
            rewrites: self.rewrites,
            digest_ok,
        }
    }

    /// 故障注入：强制下一次登记的签名值（用于验证 Sig 分支）。
    pub fn force_sign(&mut self, v: Option<u64>) {
        self.forced_sig = v;
    }

    /// 故障注入：覆盖「当前看到的接口摘要」，模拟上游接口被改写。
    pub fn override_frozen_digest(&mut self, v: Option<u64>) {
        self.frozen_override = v;
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 64
// ---------------------------------------------------------------------------

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const SIGN_SEED: u64 = 0x8422_2325_cbf2_9ce4;

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_u8(fnv_u8(h, (v & 0xff) as u8), (v >> 8) as u8)
}

fn fnv_u32(h: u64, v: u32) -> u64 {
    fnv_u16(fnv_u16(h, (v & 0xffff) as u16), (v >> 16) as u16)
}

fn fnv_u64(h: u64, v: u64) -> u64 {
    fnv_u32(fnv_u32(h, (v & 0xffff_ffff) as u32), (v >> 32) as u32)
}

fn fnv_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fnv_u8(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 注入核：十类坏联签/坏消费必须全部被拒
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    let mut r = match PipeRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return (0, total),
    };

    // 1 空生产者登记
    if r.register("", 7).err() == Some(PipeError::Empty) {
        caught += 1;
    }
    // 2 零令牌登记
    if r.register("role", 0).err() == Some(PipeError::ZeroPayload) {
        caught += 1;
    }
    // 3 登记成功后的重复登记
    if r.register("role", 0x11).is_ok() && r.register("role", 0x11).err() == Some(PipeError::Already)
    {
        caught += 1;
    }
    // 4 未登记生产者的消费
    if r.consume("ghost", 0x11, 64).err() == Some(PipeError::NoSign) {
        caught += 1;
    }
    // 5 登记表满（容量 1 的独立表）
    let mut small = match PipeRegistry::new(1) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if small.register("role", 0x21).is_ok()
        && small.register("attrib", 0x22).err() == Some(PipeError::Capacity)
    {
        caught += 1;
    }
    // 6 字段被改写（多一个字段）
    const F_INJ_EXTRA: [&str; 5] = ["role", "attrib", "sink", "ledger", "shadow"];
    let f_field = FrozenIface {
        id: AI65_ID,
        major: AI65_MAJOR,
        minor: AI65_MINOR,
        fields: &F_INJ_EXTRA,
    };
    if f_field.validate().err() == Some(PipeError::Field) {
        caught += 1;
    }
    // 7 版本被抬高
    let f_ver = FrozenIface {
        id: AI65_ID,
        major: AI65_MAJOR,
        minor: AI65_MINOR + 1,
        fields: &AI65_FIELDS,
    };
    if f_ver.validate().err() == Some(PipeError::Version) {
        caught += 1;
    }
    // 8 接口 id 被改写
    let f_id = FrozenIface {
        id: "ai65.print.pipeline.v2",
        major: AI65_MAJOR,
        minor: AI65_MINOR,
        fields: &AI65_FIELDS,
    };
    if f_id.validate().err() == Some(PipeError::Iface) {
        caught += 1;
    }
    // 9 签名被篡改后消费
    let mut tampered = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    tampered.force_sign(Some(0xdead_beef_0bad_f00d));
    if tampered.register("sink", 0x31).is_ok()
        && tampered.consume("sink", 0x31, 64).err() == Some(PipeError::Sig)
    {
        caught += 1;
    }
    // 10 上游接口摘要被覆盖（改写检测）
    let mut rewritten = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    rewritten.override_frozen_digest(Some(0x0123_4567_89ab_cdef));
    if rewritten.register("ledger", 0x41).err() == Some(PipeError::Rewrite) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50126_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50126");

    // J1-1 四生产者联签登记：role/attrib/sink/ledger，序号 0..3，全量复核通过
    let mut reg = match PipeRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let names = ["role", "attrib", "sink", "ledger"];
    let tokens = [0x101u64, 0x102, 0x103, 0x104];
    let mut seq_ok = 0usize;
    let mut i = 0usize;
    while i < names.len() {
        let name = match names.get(i) {
            Some(v) => *v,
            None => "",
        };
        let token = match tokens.get(i) {
            Some(v) => *v,
            None => 0,
        };
        match reg.register(name, token) {
            Ok(seq) => {
                if seq as usize == i {
                    seq_ok += 1;
                }
            }
            Err(_) => {}
        }
        i += 1;
    }
    let verified = match reg.verify_all() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let ok = seq_ok == 4 && verified == 4 && reg.registered_len() == 4;
    s.add(
        "50126-J1-四生产者登记",
        ok,
        if ok { "" } else { "登记序号或复核数不符" },
    );

    // J1-2 联签确定性：同一生产者 + 同一令牌，两次独立登记签名一致
    let mut d1 = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut d2 = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = d1.register("attrib", 0xa1);
    let _ = d2.register("attrib", 0xa1);
    let sig_a = match d1.entry("attrib") {
        Some(e) => e.sign_digest,
        None => 0,
    };
    let sig_b = match d2.entry("attrib") {
        Some(e) => e.sign_digest,
        None => 0,
    };
    let ok = sig_a != 0 && sig_a == sig_b && d1.registry_digest() == d2.registry_digest();
    s.add(
        "50126-J1-联签确定性",
        ok,
        if ok { "" } else { "同输入签名漂移" },
    );

    // J1-3 冻结常量：运行期逐字节重算必须等于硬编码的 AI65_ABI
    let frozen = ai65_frozen();
    let ok = frozen.digest() == AI65_ABI && AI65_ABI != 0;
    s.add(
        "50126-J1-冻结常量",
        ok,
        if ok { "" } else { "AI65_ABI 与重算不一致" },
    );

    // J1-4 冻结接口形状：id / 版本 / 字段表与常量一致，且自检通过
    let ok = frozen.id == AI65_ID
        && frozen.major == AI65_MAJOR
        && frozen.minor == AI65_MINOR
        && frozen.same_fields(&AI65_FIELDS)
        && frozen.validate().is_ok();
    s.add(
        "50126-J1-接口形状",
        ok,
        if ok { "" } else { "冻结接口形状漂移" },
    );

    // J1-5 五类接口改写必须被逐类拒绝
    const F_EXTRA: [&str; 5] = ["role", "attrib", "sink", "ledger", "tail"];
    const F_REORDER: [&str; 4] = ["attrib", "role", "sink", "ledger"];
    const F_RENAME: [&str; 4] = ["role", "attrib", "sink", "ledger2"];
    let mut mutated = 0usize;
    let c_extra = FrozenIface {
        id: AI65_ID,
        major: AI65_MAJOR,
        minor: AI65_MINOR,
        fields: &F_EXTRA,
    };
    if c_extra.validate().err() == Some(PipeError::Field) {
        mutated += 1;
    }
    let c_reorder = FrozenIface {
        id: AI65_ID,
        major: AI65_MAJOR,
        minor: AI65_MINOR,
        fields: &F_REORDER,
    };
    if c_reorder.validate().err() == Some(PipeError::Field) {
        mutated += 1;
    }
    let c_rename = FrozenIface {
        id: AI65_ID,
        major: AI65_MAJOR,
        minor: AI65_MINOR,
        fields: &F_RENAME,
    };
    if c_rename.validate().err() == Some(PipeError::Field) {
        mutated += 1;
    }
    let c_ver = FrozenIface {
        id: AI65_ID,
        major: AI65_MAJOR,
        minor: AI65_MINOR + 1,
        fields: &AI65_FIELDS,
    };
    if c_ver.validate().err() == Some(PipeError::Version) {
        mutated += 1;
    }
    let c_id = FrozenIface {
        id: "ai65.print.pipeline.beta",
        major: AI65_MAJOR,
        minor: AI65_MINOR,
        fields: &AI65_FIELDS,
    };
    if c_id.validate().err() == Some(PipeError::Iface) {
        mutated += 1;
    }
    let ok = mutated == 5;
    let det = format!("改写拦截 {}/5", mutated);
    s.add_dyn("50126-J1-改写拦截", ok, &det);

    // J1-6 消费留痕：10 次消费，留痕序号 0..9 连续，逐生产者计数正确
    let mut trace_reg = match PipeRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let plan: [(&'static str, u64, u32, u32); 4] = [
        ("role", 0x201, 3, 96),
        ("attrib", 0x202, 2, 48),
        ("sink", 0x203, 2, 512),
        ("ledger", 0x204, 3, 24),
    ];
    let mut p = 0usize;
    while p < plan.len() {
        if let Some(row) = plan.get(p) {
            let _ = trace_reg.register(row.0, row.1);
        }
        p += 1;
    }
    let mut p = 0usize;
    let mut accepted = 0usize;
    while p < plan.len() {
        if let Some(row) = plan.get(p) {
            let mut k = 0u32;
            while k < row.2 {
                if trace_reg.consume(row.0, row.1, row.3).is_ok() {
                    accepted += 1;
                }
                k += 1;
            }
        }
        p += 1;
    }
    let mut seq_contig = true;
    let mut t = 0usize;
    while t < trace_reg.trace_len() {
        match trace_reg.trace_at(t) {
            Some(tr) => {
                if tr.seq as usize != t || tr.bytes == 0 {
                    seq_contig = false;
                }
            }
            None => seq_contig = false,
        }
        t += 1;
    }
    let counts_ok = trace_reg.entry("role").map(|e| e.consumed) == Some(3)
        && trace_reg.entry("attrib").map(|e| e.consumed) == Some(2)
        && trace_reg.entry("sink").map(|e| e.consumed) == Some(2)
        && trace_reg.entry("ledger").map(|e| e.consumed) == Some(3);
    let ok = accepted == 10 && trace_reg.trace_len() == 10 && seq_contig && counts_ok;
    let det = format!(
        "留痕 {} 条/接收 {}",
        trace_reg.trace_len(),
        accepted
    );
    s.add_dyn("50126-J1-消费留痕", ok, &det);

    // J1-7 零改写证据：消费 5 次前后登记账摘要与接口摘要不变
    let mut zero_reg = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = zero_reg.register("role", 0x301);
    let _ = zero_reg.register("sink", 0x302);
    let rd_before = zero_reg.registry_digest();
    let if_before = zero_reg.iface().digest();
    let mut k = 0u32;
    while k < 5 {
        let _ = zero_reg.consume("role", 0x301, 64);
        k += 1;
    }
    let rd_after = zero_reg.registry_digest();
    let if_after = zero_reg.iface().digest();
    let ok = rd_before == rd_after
        && if_before == if_after
        && if_after == AI65_ABI
        && zero_reg.rewrites() == 0
        && zero_reg.trace_len() == 5;
    let det = format!(
        "reg {:016x}->{:016x} iface {:016x}",
        rd_before, rd_after, if_after
    );
    s.add_dyn("50126-J1-零改写", ok, &det);

    // J1-8 未登记消费：拒绝且不产生留痕
    let mut ghost_reg = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = ghost_reg.register("role", 0x401);
    let before_len = ghost_reg.trace_len();
    let rejected = ghost_reg.consume("ghost", 0x401, 32).err() == Some(PipeError::NoSign);
    let ok = rejected && ghost_reg.trace_len() == before_len;
    s.add(
        "50126-J1-未登记拒",
        ok,
        if ok { "" } else { "未登记消费被放行或留下痕迹" },
    );

    // J1-9 重复登记：拒绝且登记账长度不变
    let mut dup_reg = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = dup_reg.register("sink", 0x501);
    let len_before = dup_reg.registered_len();
    let rejected = dup_reg.register("sink", 0x502).err() == Some(PipeError::Already);
    let ok = rejected && dup_reg.registered_len() == len_before;
    s.add(
        "50126-J1-重复登记",
        ok,
        if ok { "" } else { "重复登记改动了登记账" },
    );

    // J1-10 空输入：空生产者 / 零令牌
    let mut empty_reg = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = empty_reg.register("", 1).err() == Some(PipeError::Empty)
        && empty_reg.register("role", 0).err() == Some(PipeError::ZeroPayload)
        && empty_reg.registered_len() == 0;
    s.add(
        "50126-J1-空输入拒",
        ok,
        if ok { "" } else { "空输入被登记" },
    );

    // J1-11 容量边界：0 拒、超上限拒、满表拒
    let zero_cap = PipeRegistry::new(0).err() == Some(PipeError::Empty);
    let big_cap = PipeRegistry::new(MAX_ENTRIES + 1).err() == Some(PipeError::Capacity);
    let mut cap_reg = match PipeRegistry::new(2) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = cap_reg.register("role", 0x601);
    let _ = cap_reg.register("attrib", 0x602);
    let full = cap_reg.register("sink", 0x603).err() == Some(PipeError::Capacity);
    let ok = zero_cap && big_cap && full;
    s.add(
        "50126-J1-容量边界",
        ok,
        if ok { "" } else { "容量边界错放/错拒" },
    );

    // J1-12 签名篡改：强制坏签名登记，消费时必须报 Sig
    let mut tamper_reg = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    tamper_reg.force_sign(Some(0x0bad_f00d_dead_beef));
    let reg_ok = tamper_reg.register("ledger", 0x701).is_ok();
    let rejected = tamper_reg.consume("ledger", 0x701, 128).err() == Some(PipeError::Sig);
    let ok = reg_ok && rejected && tamper_reg.trace_len() == 0;
    s.add(
        "50126-J1-签名篡改拒",
        ok,
        if ok { "" } else { "坏签名被消费" },
    );

    // J1-13 消费参数：零字节拒；令牌不符拒
    let mut param_reg = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = param_reg.register("role", 0x801);
    let zero_bytes = param_reg.consume("role", 0x801, 0).err() == Some(PipeError::ZeroPayload);
    let wrong_token = param_reg.consume("role", 0x802, 64).err() == Some(PipeError::Sig);
    let ok = zero_bytes && wrong_token && param_reg.trace_len() == 0;
    s.add(
        "50126-J1-消费参数拒",
        ok,
        if ok { "" } else { "零字节或错令牌被消费" },
    );

    // J1-14 改写检测钩子：覆盖接口摘要后登记/复核都报 Rewrite，复位后恢复
    let mut rw_reg = match PipeRegistry::new(4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    rw_reg.override_frozen_digest(Some(0x1111_2222_3333_4444));
    let reg_rejected = rw_reg.register("role", 0x901).err() == Some(PipeError::Rewrite);
    let verify_rejected = rw_reg.verify_all().err() == Some(PipeError::Rewrite);
    let audit_bad = rw_reg.audit();
    rw_reg.override_frozen_digest(None);
    let recovered = rw_reg.register("role", 0x901).is_ok() && rw_reg.audit().digest_ok;
    let ok = reg_rejected
        && verify_rejected
        && audit_bad.rewrites == 2
        && !audit_bad.digest_ok
        && recovered;
    s.add(
        "50126-J1-改写检测",
        ok,
        if ok { "" } else { "改写检测未闭合" },
    );

    // J1-15 审计口径：登记 4、消费 2 时悬空 2；补消费后 clean
    let mut audit_reg = match PipeRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = audit_reg.register("role", 0xb01);
    let _ = audit_reg.register("attrib", 0xb02);
    let _ = audit_reg.register("sink", 0xb03);
    let _ = audit_reg.register("ledger", 0xb04);
    let _ = audit_reg.consume("role", 0xb01, 64);
    let _ = audit_reg.consume("attrib", 0xb02, 128);
    let mid = audit_reg.audit();
    let mid_ok = mid.registered == 4
        && mid.verified == 4
        && mid.consumed == 2
        && mid.dangling == 2
        && mid.digest_ok
        && !mid.clean();
    let _ = audit_reg.consume("sink", 0xb03, 256);
    let _ = audit_reg.consume("ledger", 0xb04, 512);
    let end = audit_reg.audit();
    let ok = mid_ok
        && end.dangling == 0
        && end.consumed == 4
        && end.rewrites == 0
        && end.clean();
    let det = format!(
        "悬空 {}->{} 消费 {}->{}",
        mid.dangling, end.dangling, mid.consumed, end.consumed
    );
    s.add_dyn("50126-J1-审计口径", ok, &det);

    // J1-16 确定性：同一脚本两次执行，登记账/留痕账/审计三方一致
    let mut s1 = match PipeRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut s2 = match PipeRegistry::new(8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = s1.register("role", 0xc01);
    let _ = s1.register("sink", 0xc02);
    let _ = s2.register("role", 0xc01);
    let _ = s2.register("sink", 0xc02);
    let _ = s1.consume("role", 0xc01, 640);
    let _ = s1.consume("sink", 0xc02, 32);
    let _ = s2.consume("role", 0xc01, 640);
    let _ = s2.consume("sink", 0xc02, 32);
    let ok = s1.registry_digest() == s2.registry_digest()
        && s1.trace_digest() == s2.trace_digest()
        && s1.audit() == s2.audit();
    s.add(
        "50126-J1-确定性",
        ok,
        if ok { "" } else { "同脚本两账漂移" },
    );

    // J1-17 注入核：十类坏联签/坏消费全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50126-J1-注入必抓", ok, &det);

    // J1-18 错误码闭集：10 个码全部落在本段且互不相同
    let codes = [
        PipeError::Empty.code(),
        PipeError::Version.code(),
        PipeError::Iface.code(),
        PipeError::Field.code(),
        PipeError::Already.code(),
        PipeError::ZeroPayload.code(),
        PipeError::Sig.code(),
        PipeError::NoSign.code(),
        PipeError::Capacity.code(),
        PipeError::Rewrite.code(),
    ];
    let mut in_seg = 0usize;
    let mut i = 0usize;
    while i < codes.len() {
        if let Some(c) = codes.get(i) {
            let mut j = i + 1;
            let mut unique = true;
            while j < codes.len() {
                if codes.get(j) == Some(c) {
                    unique = false;
                }
                j += 1;
            }
            if *c == SEG_BASE + (i as u32 + 1) && unique {
                in_seg += 1;
            }
        }
        i += 1;
    }
    let ok = in_seg == 10;
    s.add(
        "50126-J1-错误码闭集",
        ok,
        if ok { "" } else { "错误码越段或重复" },
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50126_checks_green() {
        let set = run_unx50126_checks();
        assert!(set.all_passed(), "UNX-F50126 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50126_sign_is_deterministic() {
        let a = JointSign::compute("role", AI65_ABI, 0x1234_5678);
        let b = JointSign::compute("role", AI65_ABI, 0x1234_5678);
        assert_eq!(a, b);
        assert!(a.verify());
        assert_eq!(a.check(&ai65_frozen()), Ok(()));
    }

    #[test]
    fn unx50126_frozen_abi_stable() {
        let f = ai65_frozen();
        assert_eq!(f.digest(), AI65_ABI);
        assert!(f.validate().is_ok());
        assert!(AI65_ABI != 0);
    }

    #[test]
    fn unx50126_error_codes_in_segment() {
        assert_eq!(PipeError::Empty.code(), SEG_BASE + 1);
        assert_eq!(PipeError::Rewrite.code(), SEG_BASE + 10);
    }
}
