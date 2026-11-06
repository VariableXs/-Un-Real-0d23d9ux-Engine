//! UNX-F50113 直通冻结接口（spool 递交单点面）（0x43a40 段）
//!
//! 锚点：UNX-F50113-J1 接口冻结登记；判据锚定 §AI-63 §4。
//!
//! 直通管线对上层只暴露**一个递交点**：spool 闸门。要让「单点」成立，必须先
//! 把接口面冻结——参与直通的每条 API（名称/参数元数/版本/参数摘要）登记进
//! 清单，冻结后清单只读，根校验和固定。闸门只接受已冻结清单绑定的递交，
//! 且同一时刻只允许一个待递交票据；重复提交在闸门处显式拒绝，不做去重兜底。
//!
//! 结构清单：
//! * 接口登记表 `ApiSpec` + 冻结清单 `Manifest`：校验（名称/元数/版本/参数摘要）
//!   → 冻结（算顺序敏感根）→ 冻结后只读；
//! * 清单文本编解码 `encode_manifest` / `parse_manifest`：导入重算根，根不符整单拒；
//! * 递交票据 `Ticket`（作业号 / 源格式 / 字节数 / 内容摘要）与收据账
//!   `SubmitLedger`（容量 32，顺序敏感根）；
//! * 单点闸门状态机 `SubmitGate`：Idle → Armed → Committed；重复提交
//!   `DoubleSubmit`，未绑定/未冻结 `NotFrozen`，拒绝后回到 Idle 可重试；
//! * 注入核：清单坏样本 8 类 + 票据/闸门坏样本 5 类，逐类断言具体诊断码。
//!
//! 错误纪律：诊断码闭集 `0x43a40 + 1..=12`；内核路径零 panic——名称与十六进制
//! 解析全部按字符边界推进，文本切分只用 `split`，不按字节下标切 UTF-8 串。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43a40 独占 0x20）。
pub const SEG_BASE: u32 = 0x43a40;

pub const E_EMPTY: u16 = 1;
pub const E_DUP: u16 = 2;
pub const E_ARITY: u16 = 3;
pub const E_PARAMS: u16 = 4;
pub const E_NOTFROZEN: u16 = 5;
pub const E_ALREADYFROZEN: u16 = 6;
pub const E_STATE: u16 = 7;
pub const E_DOUBLESUBMIT: u16 = 8;
pub const E_CAPACITY: u16 = 9;
pub const E_RANGE: u16 = 10;
pub const E_CHECKSUM: u16 = 11;
pub const E_FORMAT: u16 = 12;

/// 冻结面错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeErr {
    Empty,
    Dup,
    Arity,
    Params,
    NotFrozen,
    AlreadyFrozen,
    State,
    DoubleSubmit,
    Capacity,
    Range,
    Checksum,
    Format,
}

impl FreezeErr {
    /// 诊断码（段基址 + 闭集内偏移）。
    pub fn code(self) -> u32 {
        let n = match self {
            FreezeErr::Empty => E_EMPTY,
            FreezeErr::Dup => E_DUP,
            FreezeErr::Arity => E_ARITY,
            FreezeErr::Params => E_PARAMS,
            FreezeErr::NotFrozen => E_NOTFROZEN,
            FreezeErr::AlreadyFrozen => E_ALREADYFROZEN,
            FreezeErr::State => E_STATE,
            FreezeErr::DoubleSubmit => E_DOUBLESUBMIT,
            FreezeErr::Capacity => E_CAPACITY,
            FreezeErr::Range => E_RANGE,
            FreezeErr::Checksum => E_CHECKSUM,
            FreezeErr::Format => E_FORMAT,
        };
        SEG_BASE + n as u32
    }

    /// 稳定文案。
    pub fn message(self) -> &'static str {
        match self {
            FreezeErr::Empty => "manifest has no api entry",
            FreezeErr::Dup => "duplicate api name",
            FreezeErr::Arity => "api arity outside 1..=8",
            FreezeErr::Params => "api param digest is zero",
            FreezeErr::NotFrozen => "manifest is not frozen yet",
            FreezeErr::AlreadyFrozen => "manifest already frozen",
            FreezeErr::State => "gate operation illegal in current state",
            FreezeErr::DoubleSubmit => "ticket already committed (double submit)",
            FreezeErr::Capacity => "submit ledger full",
            FreezeErr::Range => "value outside legal range",
            FreezeErr::Checksum => "manifest or receipt checksum mismatch",
            FreezeErr::Format => "source format tag unknown",
        }
    }
}

const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    let mut h = seed;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

fn fnv_u64(h: u64, x: u64) -> u64 {
    let mut acc = h;
    let mut shift = 0u32;
    while shift < 64 {
        let b = ((x >> shift) & 0xff) as u8;
        acc ^= b as u64;
        acc = acc.wrapping_mul(FNV_PRIME);
        shift += 8;
    }
    acc
}

fn parse_hex(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 16 {
        return None;
    }
    let mut v = 0u64;
    let mut i = 0usize;
    while i < s.len() {
        let c = s.get(i..i + 1)?;
        let ch = c.chars().next()?;
        v = (v << 4) | ch.to_digit(16)? as u64;
        i += 1;
    }
    Some(v)
}

/// API 名称合法性：1..=32 字节，只允许 `a-z 0-9 . _`。
///
/// 限制字符集不是为了好看：冻结后的名称进文本编解码与 grep 防重，
/// 一旦允许大写/空格/冒号，清单文本就会出现二义（大小写不同算不算重复）。
fn valid_name(n: &str) -> bool {
    if n.is_empty() || n.len() > 32 {
        return false;
    }
    let b = n.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let ok = (c >= b'a' && c <= b'z') || (c >= b'0' && c <= b'9') || c == b'.' || c == b'_';
        if !ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 单条冻结 API 登记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiSpec {
    pub name: String,
    pub arity: u8,
    pub version: u16,
    pub params: u64,
}

impl ApiSpec {
    /// 构造并校验（名称 / 参数元数 1..=8 / 版本 1..=3 / 参数摘要非 0）。
    pub fn new(name: &str, arity: u8, version: u16, params: u64) -> Result<ApiSpec, FreezeErr> {
        let s = ApiSpec {
            name: name.to_string(),
            arity,
            version,
            params,
        };
        s.validate()?;
        Ok(s)
    }

    /// 字段校验。
    pub fn validate(&self) -> Result<(), FreezeErr> {
        if !valid_name(&self.name) {
            return Err(FreezeErr::Range);
        }
        if self.arity < 1 || self.arity > 8 {
            return Err(FreezeErr::Arity);
        }
        if self.version < 1 || self.version > 3 {
            return Err(FreezeErr::Range);
        }
        if self.params == 0 {
            return Err(FreezeErr::Params);
        }
        Ok(())
    }

    /// 规范字节序：名称 + 0x00 分隔 + 元数 + 版本 + 参数摘要。
    pub fn canonical(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.name.len() + 12);
        v.extend_from_slice(self.name.as_bytes());
        v.push(0);
        v.push(self.arity);
        let ver = self.version.to_be_bytes();
        v.push(ver[0]);
        v.push(ver[1]);
        let p = self.params.to_be_bytes();
        let mut i = 0usize;
        while i < 8 {
            v.push(p[i]);
            i += 1;
        }
        v
    }

    /// 条目摘要（冻结根折叠单元）。
    pub fn digest(&self) -> u64 {
        fnv1a(&self.canonical(), FNV_BASIS)
    }

    /// 审计文本行：`name,arity,version,params_hex`。
    pub fn to_line(&self) -> String {
        format!("{},{},{},{:016x}", self.name, self.arity, self.version, self.params)
    }
}

/// 冻结状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    Draft,
    Frozen,
}

impl FreezeState {
    /// 稳定短名。
    pub fn label(self) -> &'static str {
        match self {
            FreezeState::Draft => "draft",
            FreezeState::Frozen => "frozen",
        }
    }
}

/// 冻结清单：Draft 期可追加，Frozen 后只读；`root` 在冻结时定稿。
#[derive(Clone, Debug)]
pub struct Manifest {
    apis: Vec<ApiSpec>,
    state: FreezeState,
    root: u64,
}

impl Manifest {
    /// 空清单（Draft，根为 0 表示尚未定稿）。
    pub fn new() -> Manifest {
        Manifest {
            apis: Vec::new(),
            state: FreezeState::Draft,
            root: 0,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> FreezeState {
        self.state
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.state == FreezeState::Frozen
    }

    /// 冻结根（Draft 期为 0）。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.apis.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.apis.is_empty()
    }

    /// 只读条目切片。
    pub fn apis(&self) -> &[ApiSpec] {
        &self.apis
    }

    /// 按名称查条目。
    pub fn find(&self, name: &str) -> Option<&ApiSpec> {
        let mut i = 0usize;
        while i < self.apis.len() {
            if let Some(a) = self.apis.get(i) {
                if a.name == name {
                    return Some(a);
                }
            }
            i += 1;
        }
        None
    }

    /// 追加一条 API。冻结后拒绝（`AlreadyFrozen`），重名拒绝（`Dup`）——
    /// 重名不是"后写覆盖"，因为覆盖会让冻结根与已发布文档脱钩。
    pub fn add(&mut self, spec: ApiSpec) -> Result<(), FreezeErr> {
        if self.state == FreezeState::Frozen {
            return Err(FreezeErr::AlreadyFrozen);
        }
        spec.validate()?;
        if self.find(&spec.name).is_some() {
            return Err(FreezeErr::Dup);
        }
        self.apis.push(spec);
        Ok(())
    }

    /// 冻结：非空才可冻结，根按登记顺序折叠；二次冻结显式拒绝且不动根。
    pub fn freeze(&mut self) -> Result<u64, FreezeErr> {
        if self.state == FreezeState::Frozen {
            return Err(FreezeErr::AlreadyFrozen);
        }
        if self.apis.is_empty() {
            return Err(FreezeErr::Empty);
        }
        let mut root = FNV_BASIS;
        let mut i = 0usize;
        while i < self.apis.len() {
            if let Some(a) = self.apis.get(i) {
                root = fnv_u64(root, a.digest());
            }
            i += 1;
        }
        self.root = root;
        self.state = FreezeState::Frozen;
        Ok(root)
    }
}

/// 导出清单文本：首行 `frozen <root_hex> <count>`，其后每行一条 API。
pub fn encode_manifest(m: &Manifest) -> String {
    let mut out = format!("{} {:016x} {}\n", m.state.label(), m.root(), m.len());
    let mut i = 0usize;
    while i < m.apis().len() {
        if let Some(a) = m.apis().get(i) {
            out.push_str(&a.to_line());
            out.push('\n');
        }
        i += 1;
    }
    out
}

/// 导入清单文本并重算根。只接受已冻结清单；根/条数不符整单拒绝。
pub fn parse_manifest(text: &str) -> Result<Manifest, FreezeErr> {
    if text.is_empty() {
        return Err(FreezeErr::Empty);
    }
    let mut lines = text.split('\n');
    let head = match lines.next() {
        Some(h) => h,
        None => return Err(FreezeErr::Empty),
    };
    let mut hp = head.split(' ');
    let tag = hp.next().unwrap_or("");
    let root_s = hp.next().unwrap_or("");
    let count_s = hp.next().unwrap_or("");
    if tag != "frozen" || hp.next().is_some() || root_s.is_empty() || count_s.is_empty() {
        return Err(FreezeErr::Range);
    }
    let declared_root = parse_hex(root_s).ok_or(FreezeErr::Checksum)?;
    let declared_count = parse_hex(count_s).ok_or(FreezeErr::Checksum)? as usize;
    if declared_count == 0 {
        return Err(FreezeErr::Empty);
    }
    let mut m = Manifest::new();
    let mut seen = 0usize;
    loop {
        let line = match lines.next() {
            Some(x) => x,
            None => break,
        };
        if line.is_empty() {
            continue;
        }
        let mut f = line.split(',');
        let name = f.next().unwrap_or("");
        let arity_s = f.next().unwrap_or("");
        let ver_s = f.next().unwrap_or("");
        let params_s = f.next().unwrap_or("");
        if f.next().is_some() || name.is_empty() || arity_s.is_empty() || ver_s.is_empty()
            || params_s.is_empty()
        {
            return Err(FreezeErr::Range);
        }
        let arity = parse_hex(arity_s).ok_or(FreezeErr::Checksum)?;
        let ver = parse_hex(ver_s).ok_or(FreezeErr::Checksum)?;
        let params = parse_hex(params_s).ok_or(FreezeErr::Checksum)?;
        if arity > 255 || ver > 65535 {
            return Err(FreezeErr::Arity);
        }
        let spec = ApiSpec::new(name, arity as u8, ver as u16, params)?;
        m.add(spec)?;
        seen += 1;
    }
    if seen != declared_count {
        return Err(FreezeErr::Checksum);
    }
    let root = m.freeze()?;
    if root != declared_root {
        return Err(FreezeErr::Checksum);
    }
    Ok(m)
}

/// 源格式两档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrcFormat {
    Pdf,
    Ps,
}

impl SrcFormat {
    /// 文本标签。
    pub fn tag(self) -> &'static str {
        match self {
            SrcFormat::Pdf => "pdf",
            SrcFormat::Ps => "ps",
        }
    }

    /// 标签反解；未知标签返回 `Format`。
    pub fn from_tag(t: &str) -> Result<SrcFormat, FreezeErr> {
        match t {
            "pdf" => Ok(SrcFormat::Pdf),
            "ps" => Ok(SrcFormat::Ps),
            _ => Err(FreezeErr::Format),
        }
    }
}

/// 递交票据：单点面的唯一输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub job: u32,
    pub format: SrcFormat,
    pub size: u64,
    pub digest: u64,
}

impl Ticket {
    /// 构造并校验（作业号非 0 / 字节数 1..=1TiB / 摘要非 0）。
    pub fn new(job: u32, format: SrcFormat, size: u64, digest: u64) -> Result<Ticket, FreezeErr> {
        let t = Ticket {
            job,
            format,
            size,
            digest,
        };
        t.validate()?;
        Ok(t)
    }

    /// 字段校验。
    pub fn validate(&self) -> Result<(), FreezeErr> {
        if self.job == 0 || self.digest == 0 {
            return Err(FreezeErr::Range);
        }
        if self.size == 0 || self.size > (1u64 << 40) {
            return Err(FreezeErr::Range);
        }
        Ok(())
    }

    /// 票据校验和：作业号/格式/字节数/摘要的折叠。
    pub fn checksum(&self) -> u64 {
        let mut h = fnv_u64(FNV_BASIS, self.job as u64);
        h = fnv_u64(h, self.format.tag().len() as u64);
        h = fnv_u64(h, if self.format == SrcFormat::Pdf { 1 } else { 2 });
        h = fnv_u64(h, self.size);
        fnv_u64(h, self.digest)
    }
}

/// 递交收据（落账后不可改）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub job: u32,
    pub ticket: u64,
    pub size: u64,
    pub stamp: u32,
}

impl Receipt {
    /// 收据摘要（账根折叠单元）。
    pub fn digest(&self) -> u64 {
        let mut h = fnv_u64(FNV_BASIS, self.job as u64);
        h = fnv_u64(h, self.ticket);
        h = fnv_u64(h, self.size);
        fnv_u64(h, self.stamp as u64)
    }
}

/// 收据账容量：单点闸门一次回放 32 张票据足够覆盖 PDF/PS × 大小档，
/// 超出的生产流水按批清账（`reset` 只回闸门状态，不清账，账是审计底账）。
pub const LEDGER_CAP: usize = 32;

/// 收据账。
#[derive(Clone, Debug)]
pub struct SubmitLedger {
    receipts: Vec<Receipt>,
    root: u64,
    seq: u32,
}

impl SubmitLedger {
    /// 空账。
    pub fn new() -> SubmitLedger {
        SubmitLedger {
            receipts: Vec::with_capacity(LEDGER_CAP),
            root: FNV_BASIS,
            seq: 0,
        }
    }

    /// 已落账条数。
    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }

    /// 账根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 只读收据切片。
    pub fn receipts(&self) -> &[Receipt] {
        &self.receipts
    }

    fn push(&mut self, t: Ticket) -> Result<u64, FreezeErr> {
        if self.receipts.len() >= LEDGER_CAP {
            return Err(FreezeErr::Capacity);
        }
        let r = Receipt {
            job: t.job,
            ticket: t.checksum(),
            size: t.size,
            stamp: self.seq,
        };
        let d = r.digest();
        self.receipts.push(r);
        self.root = fnv_u64(self.root, d);
        self.seq = self.seq.wrapping_add(1);
        Ok(d)
    }
}

/// 单点闸门状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateState {
    Idle,
    Armed,
    Committed,
}

impl GateState {
    /// 稳定短名。
    pub fn label(self) -> &'static str {
        match self {
            GateState::Idle => "idle",
            GateState::Armed => "armed",
            GateState::Committed => "committed",
        }
    }
}

/// spool 递交单点闸门。
#[derive(Clone, Debug)]
pub struct SubmitGate {
    frozen: bool,
    state: GateState,
    armed: Option<Ticket>,
    ledger: SubmitLedger,
    rejected: u32,
}

impl SubmitGate {
    /// 新建闸门（未绑定清单，任何递交都会被 `NotFrozen` 拒绝）。
    pub fn new() -> SubmitGate {
        SubmitGate {
            frozen: false,
            state: GateState::Idle,
            armed: None,
            ledger: SubmitLedger::new(),
            rejected: 0,
        }
    }

    /// 绑定已冻结清单；未冻清单一律拒绝。绑定是幂等的。
    pub fn bind(&mut self, m: &Manifest) -> Result<(), FreezeErr> {
        if !m.is_frozen() || m.root() == 0 {
            return Err(FreezeErr::NotFrozen);
        }
        self.frozen = true;
        Ok(())
    }

    /// 当前状态。
    pub fn state(&self) -> GateState {
        self.state
    }

    /// 只读收据账。
    pub fn ledger(&self) -> &SubmitLedger {
        &self.ledger
    }

    /// 已拒绝票据计数（显性记账，不静默吞）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 挂起一张票据：只有 Idle 可挂，且必须已绑定冻结清单。
    pub fn arm(&mut self, t: Ticket) -> Result<(), FreezeErr> {
        if !self.frozen {
            return Err(FreezeErr::NotFrozen);
        }
        if self.state != GateState::Idle {
            return Err(FreezeErr::State);
        }
        t.validate()?;
        self.armed = Some(t);
        self.state = GateState::Armed;
        Ok(())
    }

    /// 提交挂起票据：Armed → Committed，落一条收据并返回收据摘要。
    /// Committed 上再次提交是 `DoubleSubmit`（单点面不允许重复收件）。
    pub fn commit(&mut self) -> Result<u64, FreezeErr> {
        if !self.frozen {
            return Err(FreezeErr::NotFrozen);
        }
        match self.state {
            GateState::Armed => {}
            GateState::Committed => return Err(FreezeErr::DoubleSubmit),
            GateState::Idle => return Err(FreezeErr::State),
        }
        let t = match self.armed {
            Some(v) => v,
            None => return Err(FreezeErr::State),
        };
        let d = self.ledger.push(t)?;
        self.armed = None;
        self.state = GateState::Committed;
        Ok(d)
    }

    /// 拒绝挂起票据（校验失败/上层取消）：回到 Idle，收据账分毫不动。
    pub fn reject(&mut self) -> Result<(), FreezeErr> {
        if self.state != GateState::Armed {
            return Err(FreezeErr::State);
        }
        self.armed = None;
        self.state = GateState::Idle;
        self.rejected = self.rejected.wrapping_add(1);
        Ok(())
    }

    /// 收件后归位（Committed → Idle），下一票才能进闸门。
    pub fn reset(&mut self) -> Result<(), FreezeErr> {
        if self.state != GateState::Committed {
            return Err(FreezeErr::State);
        }
        self.state = GateState::Idle;
        Ok(())
    }
}

fn sample_manifest() -> Manifest {
    let mut m = Manifest::new();
    let _ = m.add(ApiSpec {
        name: "pt.submit".to_string(),
        arity: 1,
        version: 1,
        params: 0x1111,
    });
    let _ = m.add(ApiSpec {
        name: "pt.abort".to_string(),
        arity: 2,
        version: 1,
        params: 0x2222,
    });
    let _ = m.add(ApiSpec {
        name: "gdi.blit8".to_string(),
        arity: 4,
        version: 2,
        params: 0x3333,
    });
    m
}

fn ticket_for(job: u32) -> Result<Ticket, FreezeErr> {
    Ticket::new(job, SrcFormat::Pdf, 4096, 0xabc0 + job as u64)
}

/// 注入核：13 类坏样本，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 13usize;

    // 1 空清单冻结 → Empty
    let mut m = Manifest::new();
    if m.freeze().err() == Some(FreezeErr::Empty) {
        caught += 1;
    }
    // 2 重名 → Dup
    let mut m = sample_manifest();
    if m.add(ApiSpec {
        name: "pt.submit".to_string(),
        arity: 1,
        version: 1,
        params: 0x9999,
    })
    .err()
        == Some(FreezeErr::Dup)
    {
        caught += 1;
    }
    // 3 元数 0 → Arity
    if ApiSpec::new("pt.bad", 0, 1, 0x44).err() == Some(FreezeErr::Arity) {
        caught += 1;
    }
    // 4 参数摘要 0 → Params
    if ApiSpec::new("pt.bad", 1, 1, 0).err() == Some(FreezeErr::Params) {
        caught += 1;
    }
    // 5 名称非法（大写）→ Range
    if ApiSpec::new("Pt.Bad", 1, 1, 0x44).err() == Some(FreezeErr::Range) {
        caught += 1;
    }
    // 6 清单文本头字段错 → Range
    if parse_manifest("draft 0 1\n").err() == Some(FreezeErr::Range) {
        caught += 1;
    }
    // 7 清单文本摘要错（参数非十六进制）→ Checksum
    if parse_manifest("frozen 1 1\npt.a,1,1,zz\n").err() == Some(FreezeErr::Checksum) {
        caught += 1;
    }
    // 8 清单根不符 → Checksum（合法清单头换成 0 根，条目原样）
    let mut good_m = sample_manifest();
    let _ = good_m.freeze();
    let lines = encode_manifest(&good_m);
    let mut wrong = format!("frozen 0000000000000000 {}\n", good_m.len());
    if let Some(pos) = lines.find('\n') {
        if let Some(rest) = lines.get(pos + 1..) {
            wrong.push_str(rest);
        }
    }
    if parse_manifest(&wrong).err() == Some(FreezeErr::Checksum) {
        caught += 1;
    }
    // 9 未知源格式 → Format
    if SrcFormat::from_tag("tiff").err() == Some(FreezeErr::Format) {
        caught += 1;
    }
    // 10 票据摘要 0 → Range
    if Ticket::new(1, SrcFormat::Ps, 10, 0).err() == Some(FreezeErr::Range) {
        caught += 1;
    }
    // 11 未绑定冻结清单就挂票 → NotFrozen
    let mut g = SubmitGate::new();
    let t = match ticket_for(9) {
        Ok(v) => v,
        Err(_) => Ticket {
            job: 1,
            format: SrcFormat::Pdf,
            size: 1,
            digest: 1,
        },
    };
    if g.arm(t).err() == Some(FreezeErr::NotFrozen) {
        caught += 1;
    }
    // 12 已绑定但未挂票就提交 → State
    let mut g1 = SubmitGate::new();
    let _ = g1.bind(&good_m);
    if g1.commit().err() == Some(FreezeErr::State) {
        caught += 1;
    }
    // 13 重复提交 → DoubleSubmit
    let mut m = sample_manifest();
    let _ = m.freeze();
    let mut g2 = SubmitGate::new();
    let _ = g2.bind(&m);
    let ok = g2.arm(t).is_ok() && g2.commit().is_ok();
    if ok && g2.commit().err() == Some(FreezeErr::DoubleSubmit) {
        caught += 1;
    }
    (caught, total)
}

/// J1 判据：接口冻结登记（14 条）。
pub fn run_unx50113_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50113");

    // J1-1 冻结根稳定：两次冻结根一致，第二次 AlreadyFrozen。
    let mut m = sample_manifest();
    let r1 = m.freeze();
    let r2 = m.freeze();
    let ok = match (r1, r2) {
        (Ok(a), Err(FreezeErr::AlreadyFrozen)) => a == m.root() && a != 0,
        _ => false,
    };
    let det = format!("root={:016x}", m.root());
    s.add_dyn("50113-J1-冻结根稳定", ok, &det);

    // J1-2 冻结后追加拒绝且根/长度不变。
    let len_before = m.len();
    let root_before = m.root();
    let added = m.add(ApiSpec {
        name: "pt.late".to_string(),
        arity: 1,
        version: 1,
        params: 0x5555,
    });
    let ok = added.err() == Some(FreezeErr::AlreadyFrozen)
        && m.len() == len_before
        && m.root() == root_before;
    s.add("50113-J1-冻结只读", ok, if ok { "" } else { "冻结后仍可改" });

    // J1-3 空清单不可冻结，状态留 Draft。
    let mut empty = Manifest::new();
    let f = empty.freeze();
    let ok = f.err() == Some(FreezeErr::Empty)
        && empty.state() == FreezeState::Draft
        && empty.root() == 0;
    s.add("50113-J1-空单拒冻", ok, if ok { "" } else { "空单被冻结" });

    // J1-4 元数/参数非法分别被拒（Arity / Params 两码不混用）。
    let a = ApiSpec::new("pt.x", 0, 1, 0x77);
    let p = ApiSpec::new("pt.x", 1, 1, 0);
    let ok = a.err() == Some(FreezeErr::Arity) && p.err() == Some(FreezeErr::Params);
    s.add("50113-J1-登记校验", ok, if ok { "" } else { "非法登记放行" });

    // J1-5 冻结清单文本往返：根、条数、逐条摘要逐位一致。
    let text = encode_manifest(&m);
    let back = parse_manifest(&text);
    let ok = match back {
        Ok(b) => b.is_frozen() && b.root() == m.root() && b.apis() == m.apis(),
        Err(_) => false,
    };
    let det = format!("len={} root={:016x}", m.len(), m.root());
    s.add_dyn("50113-J1-清单往返", ok, &det);

    // J1-6 未冻结清单不得绑定闸门（NotFrozen）。
    let draft = sample_manifest();
    let mut g0 = SubmitGate::new();
    let ok = g0.bind(&draft).err() == Some(FreezeErr::NotFrozen) && g0.state() == GateState::Idle;
    s.add("50113-J1-未冻不绑", ok, if ok { "" } else { "草案被绑定" });

    // J1-7 单点递交：arm→commit 一次成功，账落一条、状态 Committed。
    let mut g = SubmitGate::new();
    let bound = g.bind(&m);
    let t = match ticket_for(101) {
        Ok(v) => v,
        Err(_) => Ticket {
            job: 1,
            format: SrcFormat::Pdf,
            size: 1,
            digest: 1,
        },
    };
    let armed = g.arm(t);
    let committed = g.commit();
    let ok = bound.is_ok()
        && armed.is_ok()
        && committed.is_ok()
        && g.ledger().len() == 1
        && g.state() == GateState::Committed;
    let det = format!("len={} state={}", g.ledger().len(), g.state().label());
    s.add_dyn("50113-J1-单点递交", ok, &det);

    // J1-8 重复提交拒绝且账/根守恒。
    let root_before = g.ledger().root();
    let again = g.commit();
    let ok = again.err() == Some(FreezeErr::DoubleSubmit)
        && g.ledger().len() == 1
        && g.ledger().root() == root_before;
    s.add("50113-J1-重复递交", ok, if ok { "" } else { "重复递交入账" });

    // J1-9 未挂票直接提交 → State；reset 只在 Committed 可用。
    let mut fresh = SubmitGate::new();
    let b = fresh.bind(&m);
    let c = fresh.commit();
    let r = fresh.reset();
    let ok = b.is_ok() && c.err() == Some(FreezeErr::State) && r.err() == Some(FreezeErr::State);
    s.add("50113-J1-空投拒绝", ok, if ok { "" } else { "空投被接受" });

    // J1-10 拒绝后可重挂：被拒票据不入账，rejected 计数显式 +1。
    let mut g2 = SubmitGate::new();
    let _ = g2.bind(&m);
    let t2 = match ticket_for(202) {
        Ok(v) => v,
        Err(_) => t,
    };
    let a1 = g2.arm(t2);
    let rej = g2.reject();
    let len_after_reject = g2.ledger().len();
    let a2 = g2.arm(t2);
    let c2 = g2.commit();
    let ok = a1.is_ok()
        && rej.is_ok()
        && len_after_reject == 0
        && g2.rejected() == 1
        && a2.is_ok()
        && c2.is_ok()
        && g2.ledger().len() == 1;
    let det = format!("rej={} len={}", g2.rejected(), g2.ledger().len());
    s.add_dyn("50113-J1-拒后重试", ok, &det);

    // J1-11 票据大小 0 / 超 1TiB 拒绝（同一 Range 码，边界对称）。
    let z = Ticket::new(7, SrcFormat::Ps, 0, 0x11);
    let big = Ticket::new(7, SrcFormat::Ps, (1u64 << 40) + 1, 0x11);
    let ok = z.err() == Some(FreezeErr::Range) && big.err() == Some(FreezeErr::Range);
    s.add("50113-J1-票面边界", ok, if ok { "" } else { "票面边界漏判" });

    // J1-12 收据账顺序敏感：同一对票据换序 → 根不同。
    let mut ga = SubmitGate::new();
    let mut gb = SubmitGate::new();
    let _ = ga.bind(&m);
    let _ = gb.bind(&m);
    let t1 = match ticket_for(301) {
        Ok(v) => v,
        Err(_) => t,
    };
    let t2 = match Ticket::new(302, SrcFormat::Ps, 8192, 0x99) {
        Ok(v) => v,
        Err(_) => t,
    };
    let mut ok = false;
    if ga.arm(t1).is_ok() && ga.commit().is_ok() && ga.reset().is_ok() {
        if ga.arm(t2).is_ok() && ga.commit().is_ok() {
            if gb.arm(t2).is_ok() && gb.commit().is_ok() && gb.reset().is_ok() {
                if gb.arm(t1).is_ok() && gb.commit().is_ok() {
                    ok = ga.ledger().root() != gb.ledger().root() && ga.ledger().len() == 2;
                }
            }
        }
    }
    let det = format!("ra={:016x} rb={:016x}", ga.ledger().root(), gb.ledger().root());
    s.add_dyn("50113-J1-收据换序", ok, &det);

    // J1-13 确定性：两闸门同序递交 → 根逐位一致。
    let mut gc = SubmitGate::new();
    let mut gd = SubmitGate::new();
    let _ = gc.bind(&m);
    let _ = gd.bind(&m);
    let mut ok = false;
    if gc.arm(t1).is_ok() && gc.commit().is_ok() && gd.arm(t1).is_ok() && gd.commit().is_ok() {
        ok = gc.ledger().root() == gd.ledger().root()
            && gc.ledger().receipts() == gd.ledger().receipts();
    }
    let det = format!("r={:016x}", gc.ledger().root());
    s.add_dyn("50113-J1-收据确定", ok, &det);

    // J1-14 账本容量 32：第 33 票 Capacity 且根不变。
    let mut gcap = SubmitGate::new();
    let _ = gcap.bind(&m);
    let mut acc = 0usize;
    let mut i = 0u32;
    while i < LEDGER_CAP as u32 {
        let tk = match Ticket::new(9000 + i, SrcFormat::Pdf, 1024, 0x1000 + i as u64) {
            Ok(v) => v,
            Err(_) => t,
        };
        if gcap.arm(tk).is_ok() && gcap.commit().is_ok() && gcap.reset().is_ok() {
            acc += 1;
        }
        i += 1;
    }
    let root_before = gcap.ledger().root();
    let over_t = match Ticket::new(9999, SrcFormat::Pdf, 1024, 0x77) {
        Ok(v) => v,
        Err(_) => t,
    };
    let over = match gcap.arm(over_t) {
        Ok(_) => gcap.commit(),
        Err(e) => Err(e),
    };
    let ok = acc == LEDGER_CAP
        && over.err() == Some(FreezeErr::Capacity)
        && gcap.ledger().root() == root_before
        && gcap.ledger().len() == LEDGER_CAP;
    let det = format!("acc={} len={}", acc, gcap.ledger().len());
    s.add_dyn("50113-J1-账满封顶", ok, &det);

    // J1-15 注入核 13/13。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50113-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50113_checks_green() {
        let set = run_unx50113_checks();
        assert!(set.all_passed(), "UNX-F50113 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50113_manifest_roundtrip() {
        let mut m = sample_manifest();
        assert!(m.freeze().is_ok());
        let text = encode_manifest(&m);
        match parse_manifest(&text) {
            Ok(b) => assert_eq!(b.root(), m.root()),
            Err(_) => panic!("manifest rejected"),
        }
    }

    #[test]
    fn unx50113_gate_single_point() {
        let mut m = sample_manifest();
        assert!(m.freeze().is_ok());
        let mut g = SubmitGate::new();
        assert!(g.bind(&m).is_ok());
        if let Ok(t) = ticket_for(5) {
            assert!(g.arm(t).is_ok());
            assert!(g.commit().is_ok());
            assert_eq!(g.commit().err(), Some(FreezeErr::DoubleSubmit));
            assert_eq!(g.ledger().len(), 1);
        }
    }
}
