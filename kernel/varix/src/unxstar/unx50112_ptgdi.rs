//! UNX-F50112 直通与 GDI 管线共用联签（B06 判据主轴）（0x43a20 段）
//!
//! 锚点：UNX-F50112-J1 主轴联签登记；判据锚定 §AI-63 §3。
//!
//! 本模块落地「同一张作业工单，直通管线与 GDI 回退管线各签一次，联签账落定」
//! 的共用面。直通管线原样消费工单（DPI/页数/色彩/介质全部照抄），GDI 管线
//! 需要走栅格化，因此不消费「矢量标记」位——但两者**共享字段**（作业号、页数、
//! DPI、色彩模式、介质尺寸）必须逐位一致。共享字段不一致 = 两条管线看到了
//! 不同的工单，联签必须显式失败（Diverged），而不是挑一个继续印。
//!
//! 结构清单：
//! * 工单票据解析器 `parse_ticket`：PTG1 定长 24 字节，尾部校验和；
//! * 双管线签署器 `Signer` / `Ack`：按角色生成签署回执，GDI 侧清除矢量位，
//!   由此产生「共享字段一致但整单摘要漂移」的合法漂移（drift）语义；
//! * 联签状态机 `SignSession`：Draft → PtSigned / GdiSigned → Cosigned →
//!   Sealed；共享字段分歧时进入 Diverged 终态，不可再签、不可封口；
//! * 联签账 `CoSignLedger`：容量 48，顺序敏感根校验和，逐条可审计；
//! * 文本编解码 `encode_ledger` / `parse_ledger`：根与条目分离，导入时重算根；
//! * 注入核：票据坏样本 8 类 + 账文本坏样本 3 类 + 重复登记 1 类，逐类断言码。
//!
//! 错误纪律：诊断码闭集 `0x43a20 + 1..=13`；内核路径零 panic——票据读取逐字节
//! 区间校验，文本解析全部走 `split` + `get`，不按字节下标切多字节字符串。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43a20 独占 0x20）。
pub const SEG_BASE: u32 = 0x43a20;

pub const E_FIELDS: u16 = 1;
pub const E_HEX: u16 = 2;
pub const E_MAGIC: u16 = 3;
pub const E_SHORT: u16 = 4;
pub const E_VERSION: u16 = 5;
pub const E_COLOR: u16 = 6;
pub const E_PAGES: u16 = 7;
pub const E_CHECKSUM: u16 = 8;
pub const E_DUP: u16 = 9;
pub const E_MISMATCH: u16 = 10;
pub const E_CAPACITY: u16 = 11;
pub const E_STATE: u16 = 12;
pub const E_RANGE: u16 = 13;

/// 联签面错误。`code()` 返回 `SEG_BASE + n`，与全仓诊断码闭集一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignErr {
    Fields,
    Hex,
    Magic,
    Short,
    Version,
    Color,
    Pages,
    Checksum,
    Dup,
    Mismatch,
    Capacity,
    State,
    Range,
}

impl SignErr {
    /// 诊断码（段基址 + 闭集内偏移）。
    pub fn code(self) -> u32 {
        let n = match self {
            SignErr::Fields => E_FIELDS,
            SignErr::Hex => E_HEX,
            SignErr::Magic => E_MAGIC,
            SignErr::Short => E_SHORT,
            SignErr::Version => E_VERSION,
            SignErr::Color => E_COLOR,
            SignErr::Pages => E_PAGES,
            SignErr::Checksum => E_CHECKSUM,
            SignErr::Dup => E_DUP,
            SignErr::Mismatch => E_MISMATCH,
            SignErr::Capacity => E_CAPACITY,
            SignErr::State => E_STATE,
            SignErr::Range => E_RANGE,
        };
        SEG_BASE + n as u32
    }

    /// 稳定文案（日志/诊断面板共用，不随本地化改动）。
    pub fn message(self) -> &'static str {
        match self {
            SignErr::Fields => "ticket/ledger field count mismatch",
            SignErr::Hex => "hex field malformed",
            SignErr::Magic => "PTG1 magic missing",
            SignErr::Short => "ticket shorter than 24 bytes",
            SignErr::Version => "ticket version unsupported",
            SignErr::Color => "color mode code invalid",
            SignErr::Pages => "page count is zero",
            SignErr::Checksum => "ticket or ledger checksum mismatch",
            SignErr::Dup => "duplicate job or duplicate role signature",
            SignErr::Mismatch => "pipelines disagree on shared plan fields",
            SignErr::Capacity => "co-sign ledger full",
            SignErr::State => "operation illegal in current sign state",
            SignErr::Range => "value outside legal range",
        }
    }
}

/// 色彩模式四档（与打印工单编码一致：0 单色 / 1 灰阶 / 2 RGB / 3 CMYK）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Mono,
    Gray,
    Rgb,
    Cmyk,
}

impl ColorMode {
    /// 工单码值。
    pub fn code(self) -> u8 {
        match self {
            ColorMode::Mono => 0,
            ColorMode::Gray => 1,
            ColorMode::Rgb => 2,
            ColorMode::Cmyk => 3,
        }
    }

    /// 码值反解；越界返回 `Color`，不做静默兜底。
    pub fn from_code(c: u8) -> Result<ColorMode, SignErr> {
        match c {
            0 => Ok(ColorMode::Mono),
            1 => Ok(ColorMode::Gray),
            2 => Ok(ColorMode::Rgb),
            3 => Ok(ColorMode::Cmyk),
            _ => Err(SignErr::Color),
        }
    }

    /// 通道数（工单校验用：CMYK 必须有 4 个分色版）。
    pub fn planes(self) -> u8 {
        match self {
            ColorMode::Mono | ColorMode::Gray => 1,
            ColorMode::Rgb => 3,
            ColorMode::Cmyk => 4,
        }
    }
}

/// 管线角色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signer {
    Passthrough,
    Gdi,
}

impl Signer {
    /// 角色短名（落账文本用）。
    pub fn tag(self) -> &'static str {
        match self {
            Signer::Passthrough => "pt",
            Signer::Gdi => "gdi",
        }
    }

    /// GDI 需要栅格化，不消费工单的矢量标记位（bit1）。
    pub fn consumes_vector_flag(self) -> bool {
        matches!(self, Signer::Passthrough)
    }
}

/// 作业计划（工单的可执行视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobPlan {
    pub job: u32,
    pub pages: u16,
    pub dpi: u16,
    pub color: ColorMode,
    pub flags: u8,
    pub media_w_mm: u16,
    pub media_h_mm: u16,
}

impl JobPlan {
    /// 构造并校验。
    pub fn new(
        job: u32,
        pages: u16,
        dpi: u16,
        color: ColorMode,
        flags: u8,
        w: u16,
        h: u16,
    ) -> Result<JobPlan, SignErr> {
        let p = JobPlan {
            job,
            pages,
            dpi,
            color,
            flags,
            media_w_mm: w,
            media_h_mm: h,
        };
        p.validate()?;
        Ok(p)
    }

    /// 合法性：作业号非 0、页数非 0、DPI 四档、介质双边 10..=2000mm。
    pub fn validate(&self) -> Result<(), SignErr> {
        if self.job == 0 {
            return Err(SignErr::Range);
        }
        if self.pages == 0 {
            return Err(SignErr::Pages);
        }
        if !matches!(self.dpi, 150 | 300 | 600 | 1200) {
            return Err(SignErr::Range);
        }
        if self.media_w_mm < 10 || self.media_w_mm > 2000 {
            return Err(SignErr::Range);
        }
        if self.media_h_mm < 10 || self.media_h_mm > 2000 {
            return Err(SignErr::Range);
        }
        Ok(())
    }

    /// 规范字节序（大端定长 15 字节）；联签摘要的唯一口径。
    pub fn canonical(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(15);
        let j = self.job.to_be_bytes();
        v.push(j[0]);
        v.push(j[1]);
        v.push(j[2]);
        v.push(j[3]);
        let p = self.pages.to_be_bytes();
        v.push(p[0]);
        v.push(p[1]);
        let d = self.dpi.to_be_bytes();
        v.push(d[0]);
        v.push(d[1]);
        v.push(self.color.code());
        v.push(self.flags);
        let w = self.media_w_mm.to_be_bytes();
        v.push(w[0]);
        v.push(w[1]);
        let h = self.media_h_mm.to_be_bytes();
        v.push(h[0]);
        v.push(h[1]);
        v
    }

    /// 整单摘要（含管线私有标记）。
    pub fn digest(&self) -> u64 {
        fnv1a(&self.canonical(), FNV_BASIS)
    }

    /// 共享摘要：只覆盖两条管线都必须一致的五组字段（作业号/页数/DPI/色彩/介质）。
    pub fn shared_digest(&self) -> u64 {
        let mut v = Vec::with_capacity(12);
        let j = self.job.to_be_bytes();
        v.push(j[0]);
        v.push(j[1]);
        v.push(j[2]);
        v.push(j[3]);
        let p = self.pages.to_be_bytes();
        v.push(p[0]);
        v.push(p[1]);
        let d = self.dpi.to_be_bytes();
        v.push(d[0]);
        v.push(d[1]);
        v.push(self.color.code());
        let w = self.media_w_mm.to_be_bytes();
        v.push(w[0]);
        v.push(w[1]);
        let h = self.media_h_mm.to_be_bytes();
        v.push(h[0]);
        v.push(h[1]);
        fnv1a(&v, FNV_BASIS)
    }

    /// GDI 视角：清掉矢量位（bit1），其余字段原样——这是合法漂移的唯一来源。
    pub fn gdi_view(&self) -> JobPlan {
        let mut p = *self;
        p.flags &= !0x02;
        p
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

/// 票据固定长度：4 魔数 + 1 版本 + 1 色彩 + 1 标记 + 1 保留 + 4 作业号
/// + 2 页数 + 2 DPI + 2 宽 + 2 高 + 4 校验和 = 24。
pub const TICKET_LEN: usize = 24;

/// 工单票据尾部校验和：前 20 字节按字节累加 mod 2^32（便宜且能抓单字节翻转）。
fn ticket_sum(data: &[u8]) -> u32 {
    let mut s = 0u32;
    let mut i = 0usize;
    while i < 20 {
        if let Some(b) = data.get(i) {
            s = s.wrapping_add(*b as u32);
        }
        i += 1;
    }
    s
}

fn read_u16(data: &[u8], at: usize) -> Result<u16, SignErr> {
    let a = match data.get(at) {
        Some(v) => *v as u16,
        None => return Err(SignErr::Short),
    };
    let b = match data.get(at + 1) {
        Some(v) => *v as u16,
        None => return Err(SignErr::Short),
    };
    Ok((a << 8) | b)
}

fn read_u32(data: &[u8], at: usize) -> Result<u32, SignErr> {
    let mut v = 0u32;
    let mut i = 0usize;
    while i < 4 {
        let b = match data.get(at + i) {
            Some(x) => *x as u32,
            None => return Err(SignErr::Short),
        };
        v = (v << 8) | b;
        i += 1;
    }
    Ok(v)
}

/// 解析 PTG1 工单票据。先判空/长度/魔数/版本，再逐字段反解并校验，
/// 最后核对尾部校验和——顺序固定，坏样本的诊断码因此可稳定断言。
pub fn parse_ticket(data: &[u8]) -> Result<JobPlan, SignErr> {
    if data.is_empty() {
        return Err(SignErr::Short);
    }
    if data.len() < TICKET_LEN {
        return Err(SignErr::Short);
    }
    if data.get(0..4) != Some(&b"PTG1"[..]) {
        return Err(SignErr::Magic);
    }
    let ver = match data.get(4) {
        Some(v) => *v,
        None => return Err(SignErr::Short),
    };
    if ver != 1 {
        return Err(SignErr::Version);
    }
    let color = ColorMode::from_code(match data.get(5) {
        Some(v) => *v,
        None => return Err(SignErr::Short),
    })?;
    let flags = match data.get(6) {
        Some(v) => *v,
        None => return Err(SignErr::Short),
    };
    let job = read_u32(data, 8)?;
    let pages = read_u16(data, 12)?;
    let dpi = read_u16(data, 14)?;
    let w = read_u16(data, 16)?;
    let h = read_u16(data, 18)?;
    let want = read_u32(data, 20)?;
    if ticket_sum(data) != want {
        return Err(SignErr::Checksum);
    }
    if pages == 0 {
        return Err(SignErr::Pages);
    }
    JobPlan::new(job, pages, dpi, color, flags, w, h)
}

/// 编码 PTG1 票据（校验和按前 20 字节累加）。
pub fn encode_ticket(p: &JobPlan) -> Vec<u8> {
    let mut v = Vec::with_capacity(TICKET_LEN);
    v.push(b'P');
    v.push(b'T');
    v.push(b'G');
    v.push(b'1');
    v.push(1);
    v.push(p.color.code());
    v.push(p.flags);
    v.push(0);
    let j = p.job.to_be_bytes();
    let mut i = 0usize;
    while i < 4 {
        v.push(j[i]);
        i += 1;
    }
    let pg = p.pages.to_be_bytes();
    v.push(pg[0]);
    v.push(pg[1]);
    let d = p.dpi.to_be_bytes();
    v.push(d[0]);
    v.push(d[1]);
    let w = p.media_w_mm.to_be_bytes();
    v.push(w[0]);
    v.push(w[1]);
    let h = p.media_h_mm.to_be_bytes();
    v.push(h[0]);
    v.push(h[1]);
    let sum = ticket_sum(&v);
    let sb = sum.to_be_bytes();
    i = 0;
    while i < 4 {
        v.push(sb[i]);
        i += 1;
    }
    v
}

/// 签署回执：`shared` 是共享字段摘要（联签判据），`plan` 是该管线实际
/// 看到的整单摘要（审计与漂移判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ack {
    pub role: Signer,
    pub job: u32,
    pub shared: u64,
    pub plan: u64,
}

impl Ack {
    /// 由计划生成回执；GDI 侧按角色规则清除矢量位后再取整单摘要。
    pub fn from_plan(role: Signer, plan: &JobPlan) -> Result<Ack, SignErr> {
        plan.validate()?;
        let seen = if role.consumes_vector_flag() {
            *plan
        } else {
            plan.gdi_view()
        };
        Ok(Ack {
            role,
            job: plan.job,
            shared: plan.shared_digest(),
            plan: seen.digest(),
        })
    }
}

/// 联签状态机状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignState {
    Draft,
    PtSigned,
    GdiSigned,
    Cosigned,
    Sealed,
    Diverged,
}

impl SignState {
    /// 稳定短名（账本审计文本用）。
    pub fn label(self) -> &'static str {
        match self {
            SignState::Draft => "draft",
            SignState::PtSigned => "pt",
            SignState::GdiSigned => "gdi",
            SignState::Cosigned => "cosigned",
            SignState::Sealed => "sealed",
            SignState::Diverged => "diverged",
        }
    }

    /// 是否已定格（不可再签）。
    pub fn terminal(self) -> bool {
        matches!(self, SignState::Sealed | SignState::Diverged)
    }
}

/// 单作业联签会话：持有计划、两方回执与状态。
#[derive(Clone, Debug)]
pub struct SignSession {
    pub plan: JobPlan,
    pub state: SignState,
    pub pt: Option<Ack>,
    pub gdi: Option<Ack>,
    pub entry: Option<u64>,
}

impl SignSession {
    /// 新会话（计划必须先过校验）。
    pub fn new(plan: JobPlan) -> Result<SignSession, SignErr> {
        plan.validate()?;
        Ok(SignSession {
            plan,
            state: SignState::Draft,
            pt: None,
            gdi: None,
            entry: None,
        })
    }

    /// 以本会话内计划签署指定角色。
    pub fn sign(&mut self, role: Signer) -> Result<SignState, SignErr> {
        let ack = Ack::from_plan(role, &self.plan)?;
        self.sign_with(ack)
    }

    /// 以外部回执签署（用于接收对端管线递交的回执，含陈旧票据注入）。
    pub fn sign_with(&mut self, ack: Ack) -> Result<SignState, SignErr> {
        if self.state.terminal() {
            return Err(SignErr::State);
        }
        if ack.job != self.plan.job {
            self.state = SignState::Diverged;
            return Err(SignErr::Mismatch);
        }
        match ack.role {
            Signer::Passthrough => {
                if self.pt.is_some() {
                    return Err(SignErr::Dup);
                }
                self.pt = Some(ack);
            }
            Signer::Gdi => {
                if self.gdi.is_some() {
                    return Err(SignErr::Dup);
                }
                self.gdi = Some(ack);
            }
        }
        let (pt, gdi) = match (self.pt, self.gdi) {
            (Some(p), Some(g)) => (p, g),
            (Some(_), None) => {
                self.state = SignState::PtSigned;
                return Ok(self.state);
            }
            (None, Some(_)) => {
                self.state = SignState::GdiSigned;
                return Ok(self.state);
            }
            (None, None) => return Err(SignErr::State),
        };
        if pt.shared != gdi.shared {
            self.state = SignState::Diverged;
            return Err(SignErr::Mismatch);
        }
        self.state = SignState::Cosigned;
        Ok(self.state)
    }

    /// 两方整单摘要是否漂移（共享字段一致但管线私有视图不同）。
    pub fn drift(&self) -> bool {
        match (self.pt, self.gdi) {
            (Some(p), Some(g)) => p.plan != g.plan,
            _ => false,
        }
    }

    /// 封口：只有 Cosigned 可封；封口后会话不可再改。
    pub fn seal(&mut self) -> Result<(), SignErr> {
        if self.state != SignState::Cosigned {
            return Err(SignErr::State);
        }
        self.state = SignState::Sealed;
        Ok(())
    }
}

/// 联签账容量：一次批量回放 48 张工单已覆盖双管线 × 四色彩 × 四 DPI 的
/// 关键组合；再大属于生产流水，应走 spool 递交账而不是联签账。
pub const LEDGER_CAP: usize = 48;

/// 联签账条目（不可变快照，`stamp` 为落账序号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoSignEntry {
    pub job: u32,
    pub plan: u64,
    pub pt: u64,
    pub gdi: u64,
    pub shared: u64,
    pub drift: bool,
    pub stamp: u32,
}

impl CoSignEntry {
    /// 条目摘要：联签账根校验和的折叠单元。
    pub fn digest(&self) -> u64 {
        let mut h = fnv_u64(FNV_BASIS, self.job as u64);
        h = fnv_u64(h, self.plan);
        h = fnv_u64(h, self.pt);
        h = fnv_u64(h, self.gdi);
        h = fnv_u64(h, self.shared);
        h = fnv_u64(h, if self.drift { 1 } else { 0 });
        fnv_u64(h, self.stamp as u64)
    }

    /// 审计文本（无根头，parse_ledger 逐行解析）。
    pub fn to_line(&self) -> String {
        format!(
            "{:x}:{:016x}:{:016x}:{:016x}:{:016x}:{}:{}",
            self.job, self.plan, self.pt, self.gdi, self.shared, self.drift as u8, self.stamp
        )
    }
}

/// 联签账：只追加，根校验和对条目顺序敏感。
#[derive(Clone, Debug)]
pub struct CoSignLedger {
    entries: Vec<CoSignEntry>,
    root: u64,
    seq: u32,
}

impl CoSignLedger {
    /// 空账（根取 FNV 基值，便于与「有账但全零」区分）。
    pub fn new() -> CoSignLedger {
        CoSignLedger {
            entries: Vec::with_capacity(LEDGER_CAP),
            root: FNV_BASIS,
            seq: 0,
        }
    }

    /// 当前根校验和。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 已落账条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 账是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 只读条目切片（审计导出用）。
    pub fn entries(&self) -> &[CoSignEntry] {
        &self.entries
    }

    /// 按作业号查条目（重复作业在登记阶段已拒绝，故最多一条）。
    pub fn find(&self, job: u32) -> Option<&CoSignEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.job == job {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 把一张已双签的会话落账，返回条目摘要。
    ///
    /// 拒绝条件按「便宜到贵」排序：状态 → 容量 → 重复 → 回执缺失。
    /// 任何一次拒绝都不改动账本（根与长度均不变），这是守恒判据的基础。
    pub fn register(&mut self, s: &SignSession) -> Result<u64, SignErr> {
        if s.state != SignState::Cosigned {
            return Err(SignErr::State);
        }
        if self.entries.len() >= LEDGER_CAP {
            return Err(SignErr::Capacity);
        }
        if self.find(s.plan.job).is_some() {
            return Err(SignErr::Dup);
        }
        let (pt, gdi) = match (s.pt, s.gdi) {
            (Some(p), Some(g)) => (p, g),
            _ => return Err(SignErr::State),
        };
        let e = CoSignEntry {
            job: s.plan.job,
            plan: s.plan.digest(),
            pt: pt.plan,
            gdi: gdi.plan,
            shared: pt.shared,
            drift: pt.plan != gdi.plan,
            stamp: self.seq,
        };
        let d = e.digest();
        self.entries.push(e);
        self.root = fnv_u64(self.root, d);
        self.seq += 1;
        Ok(d)
    }
}

fn parse_hex(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 16 {
        return None;
    }
    let mut v = 0u64;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match s.get(i..i + 1) {
            Some(x) => x,
            None => return None,
        };
        let ch = match c.chars().next() {
            Some(x) => x,
            None => return None,
        };
        let d = match ch.to_digit(16) {
            Some(x) => x as u64,
            None => return None,
        };
        v = (v << 4) | d;
        i += 1;
    }
    Some(v)
}

/// 导出联签账文本：首行 `root <hex> <count>`，其后每行一条条目。
pub fn encode_ledger(l: &CoSignLedger) -> String {
    let mut out = format!("root {:016x} {}\n", l.root(), l.len());
    let mut i = 0usize;
    while i < l.entries().len() {
        if let Some(e) = l.entries().get(i) {
            out.push_str(&e.to_line());
            out.push('\n');
        }
        i += 1;
    }
    out
}

/// 导入联签账文本并重算根。根不符即整账拒绝（`Checksum`），不半信半疑。
pub fn parse_ledger(text: &str) -> Result<CoSignLedger, SignErr> {
    if text.is_empty() {
        return Err(SignErr::Fields);
    }
    let mut lines = text.split('\n');
    let head = match lines.next() {
        Some(h) => h,
        None => return Err(SignErr::Fields),
    };
    let mut hp = head.split(' ');
    let tag = hp.next().unwrap_or("");
    let root_s = hp.next().unwrap_or("");
    let count_s = hp.next().unwrap_or("");
    if tag != "root" || hp.next().is_some() {
        return Err(SignErr::Fields);
    }
    let declared_root = parse_hex(root_s).ok_or(SignErr::Hex)?;
    let declared_count = parse_hex(count_s).ok_or(SignErr::Hex)? as usize;
    let mut ledger = CoSignLedger::new();
    let mut seen = 0usize;
    let mut it = lines;
    loop {
        let line = match it.next() {
            Some(x) => x,
            None => break,
        };
        if line.is_empty() {
            continue;
        }
        if seen >= LEDGER_CAP {
            return Err(SignErr::Capacity);
        }
        let mut f = line.split(':');
        let job_s = f.next().unwrap_or("");
        let plan_s = f.next().unwrap_or("");
        let pt_s = f.next().unwrap_or("");
        let gdi_s = f.next().unwrap_or("");
        let shared_s = f.next().unwrap_or("");
        let drift_s = f.next().unwrap_or("");
        let stamp_s = f.next().unwrap_or("");
        if f.next().is_some() {
            return Err(SignErr::Fields);
        }
        if job_s.is_empty() || plan_s.is_empty() || pt_s.is_empty() || gdi_s.is_empty()
            || shared_s.is_empty() || drift_s.is_empty() || stamp_s.is_empty()
        {
            return Err(SignErr::Fields);
        }
        let job = parse_hex(job_s).ok_or(SignErr::Hex)? as u32;
        let plan = parse_hex(plan_s).ok_or(SignErr::Hex)?;
        let pt = parse_hex(pt_s).ok_or(SignErr::Hex)?;
        let gdi = parse_hex(gdi_s).ok_or(SignErr::Hex)?;
        let shared = parse_hex(shared_s).ok_or(SignErr::Hex)?;
        let drift = parse_hex(drift_s).ok_or(SignErr::Hex)?;
        let stamp = parse_hex(stamp_s).ok_or(SignErr::Hex)? as u32;
        if drift > 1 {
            return Err(SignErr::Range);
        }
        if ledger.find(job).is_some() {
            return Err(SignErr::Dup);
        }
        let e = CoSignEntry {
            job,
            plan,
            pt,
            gdi,
            shared,
            drift: drift == 1,
            stamp,
        };
        let d = e.digest();
        ledger.entries.push(e);
        ledger.root = fnv_u64(ledger.root, d);
        ledger.seq = ledger.seq.wrapping_add(1);
        seen += 1;
    }
    if seen != declared_count {
        return Err(SignErr::Checksum);
    }
    if ledger.root != declared_root {
        return Err(SignErr::Checksum);
    }
    Ok(ledger)
}

fn plan_at(dpi: u16) -> Result<JobPlan, SignErr> {
    JobPlan::new(4001, 2, dpi, ColorMode::Rgb, 0x02 | 0x04, 210, 297)
}

fn cosigned_session(job: u32, drift: bool) -> Result<SignSession, SignErr> {
    let flags = if drift { 0x02 } else { 0x00 };
    let p = JobPlan::new(job, 1, 600, ColorMode::Cmyk, flags, 210, 297)?;
    let mut s = SignSession::new(p)?;
    s.sign(Signer::Passthrough)?;
    if drift {
        // 外部回执由 GDI 视角生成：共享字段一致、整单摘要不同。
        let ack = Ack::from_plan(Signer::Gdi, &p)?;
        s.sign_with(ack)?;
    } else {
        s.sign(Signer::Gdi)?;
    }
    Ok(s)
}

/// 注入核：12 类坏样本，逐类断言具体诊断码。
///
/// 覆盖票据解析 8 类、账文本 3 类、重复登记 1 类；返回 (抓住数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;

    // 1 空票据 → Short
    if parse_ticket(&[]).err() == Some(SignErr::Short) {
        caught += 1;
    }
    // 2 截断 → Short
    if parse_ticket(&[b'P', b'T', b'G', b'1', 1, 2, 0, 0, 0, 0]).err() == Some(SignErr::Short) {
        caught += 1;
    }
    // 3 魔数错
    let mut t = match plan_at(600) {
        Ok(p) => encode_ticket(&p),
        Err(_) => Vec::new(),
    };
    if let Some(b) = t.get_mut(0) {
        *b = b'X';
    }
    if parse_ticket(&t).err() == Some(SignErr::Magic) {
        caught += 1;
    }
    // 4 版本错
    let mut t = match plan_at(600) {
        Ok(p) => encode_ticket(&p),
        Err(_) => Vec::new(),
    };
    if let Some(b) = t.get_mut(4) {
        *b = 9;
    }
    if parse_ticket(&t).err() == Some(SignErr::Version) {
        caught += 1;
    }
    // 5 色彩码错
    let mut t = match plan_at(600) {
        Ok(p) => encode_ticket(&p),
        Err(_) => Vec::new(),
    };
    if let Some(b) = t.get_mut(5) {
        *b = 7;
    }
    if parse_ticket(&t).err() == Some(SignErr::Color) {
        caught += 1;
    }
    // 6 页数为 0（校验和同步修正，确保先撞 Pages）
    let zero = JobPlan {
        job: 4001,
        pages: 0,
        dpi: 600,
        color: ColorMode::Rgb,
        flags: 0,
        media_w_mm: 210,
        media_h_mm: 297,
    };
    if parse_ticket(&encode_ticket(&zero)).err() == Some(SignErr::Pages) {
        caught += 1;
    }
    // 7 校验和错（翻一字节不修尾部）
    let mut t = match plan_at(600) {
        Ok(p) => encode_ticket(&p),
        Err(_) => Vec::new(),
    };
    if let Some(b) = t.get_mut(6) {
        *b ^= 0x40;
    }
    if parse_ticket(&t).err() == Some(SignErr::Checksum) {
        caught += 1;
    }
    // 8 DPI 非法（校验和正确）
    if parse_ticket(&encode_ticket(&JobPlan {
        job: 4001,
        pages: 1,
        dpi: 999,
        color: ColorMode::Mono,
        flags: 0,
        media_w_mm: 210,
        media_h_mm: 297,
    }))
    .err()
        == Some(SignErr::Range)
    {
        caught += 1;
    }
    // 9 账文本字段数错
    if parse_ledger("root 0 0\n1:2:3\n").err() == Some(SignErr::Fields) {
        caught += 1;
    }
    // 10 账文本十六进制错
    if parse_ledger("root 0 1\nzz:2:3:4:5:0:0\n").err() == Some(SignErr::Hex) {
        caught += 1;
    }
    // 11 账根不符
    if parse_ledger("root 0 0\n").err() == Some(SignErr::Checksum) {
        caught += 1;
    }
    // 12 重复作业登记
    let mut ledger = CoSignLedger::new();
    let s1 = match cosigned_session(4100, false) {
        Ok(v) => v,
        Err(_) => SignSession {
            plan: match plan_at(600) {
                Ok(p) => p,
                Err(_) => JobPlan {
                    job: 1,
                    pages: 1,
                    dpi: 600,
                    color: ColorMode::Rgb,
                    flags: 0,
                    media_w_mm: 210,
                    media_h_mm: 297,
                },
            },
            state: SignState::Draft,
            pt: None,
            gdi: None,
            entry: None,
        },
    };
    if ledger.register(&s1).is_ok() && ledger.register(&s1).err() == Some(SignErr::Dup) {
        caught += 1;
    }
    (caught, total)
}

/// J1 判据：主轴联签登记（13 条，全部性质断言）。
pub fn run_unx50112_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50112");

    // J1-1 票据往返：编码→解析字段逐位一致，且规范摘要相同。
    let plan = match plan_at(600) {
        Ok(p) => p,
        Err(_) => JobPlan {
            job: 1,
            pages: 1,
            dpi: 600,
            color: ColorMode::Rgb,
            flags: 0,
            media_w_mm: 210,
            media_h_mm: 297,
        },
    };
    let back = parse_ticket(&encode_ticket(&plan));
    let ok = match back {
        Ok(b) => b == plan && b.canonical() == plan.canonical() && b.digest() == plan.digest(),
        Err(_) => false,
    };
    s.add("50112-J1-票据往返", ok, if ok { "" } else { "票据字段漂移" });

    // J1-2 双签一致 → Cosigned，且账本落一条。
    let mut session = match cosigned_session(4201, false) {
        Ok(v) => v,
        Err(_) => match SignSession::new(plan) {
            Ok(v) => v,
            Err(_) => SignSession {
                plan,
                state: SignState::Draft,
                pt: None,
                gdi: None,
                entry: None,
            },
        },
    };
    let mut ledger = CoSignLedger::new();
    let reg = ledger.register(&session);
    let ok = session.state == SignState::Cosigned && reg.is_ok() && ledger.len() == 1;
    let det = format!("state={} len={}", session.state.label(), ledger.len());
    s.add_dyn("50112-J1-双签登记", ok, &det);

    // J1-3 单签不可封口：State 拒绝且状态保持 PtSigned。
    let mut one = match SignSession::new(match plan_at(300) {
        Ok(p) => p,
        Err(_) => plan,
    }) {
        Ok(v) => v,
        Err(_) => SignSession {
            plan,
            state: SignState::Draft,
            pt: None,
            gdi: None,
            entry: None,
        },
    };
    let sig = one.sign(Signer::Passthrough);
    let sealed = one.seal();
    let ok = sig.is_ok() && sealed.err() == Some(SignErr::State) && one.state == SignState::PtSigned;
    let det = format!("state={} code={:x}", one.state.label(), sealed.err().map(|e| e.code()).unwrap_or(0));
    s.add_dyn("50112-J1-单签拒封", ok, &det);

    // J1-4 陈旧回执（GDI 看到 300dpi，工单 600dpi）→ Mismatch 且终态 Diverged。
    let mut stale = match SignSession::new(match plan_at(600) {
        Ok(p) => p,
        Err(_) => plan,
    }) {
        Ok(v) => v,
        Err(_) => SignSession {
            plan,
            state: SignState::Draft,
            pt: None,
            gdi: None,
            entry: None,
        },
    };
    let _ = stale.sign(Signer::Passthrough);
    let old = match plan_at(300) {
        Ok(p) => Ack::from_plan(Signer::Gdi, &p),
        Err(_) => Err(SignErr::Range),
    };
    let mut stale_code = 0u32;
    if let Ok(a) = old {
        if let Err(e) = stale.sign_with(a) {
            stale_code = e.code();
        }
    }
    let ok = stale.state == SignState::Diverged && stale_code == SEG_BASE + E_MISMATCH as u32;
    let det = format!("state={} code={:x}", stale.state.label(), stale_code);
    s.add_dyn("50112-J1-分歧拒签", ok, &det);

    // J1-5 私有位漂移但共享一致 → Cosigned + drift 标记（不是失败）。
    let drift_s = cosigned_session(4202, true);
    let ok = match drift_s {
        Ok(v) => v.state == SignState::Cosigned && v.drift(),
        Err(_) => false,
    };
    s.add("50112-J1-漂移登记", ok, if ok { "" } else { "合法漂移被误杀" });

    // J1-6 账根顺序敏感：同两条不同顺序 → 根不同。
    let a = cosigned_session(4301, false);
    let b = cosigned_session(4302, false);
    let mut l1 = CoSignLedger::new();
    let mut l2 = CoSignLedger::new();
    let mut ok = false;
    if let (Ok(sa), Ok(sb)) = (&a, &b) {
        let r1 = l1.register(sa).is_ok() && l1.register(sb).is_ok();
        let r2 = l2.register(sb).is_ok() && l2.register(sa).is_ok();
        ok = r1 && r2 && l1.root() != l2.root() && l1.len() == 2 && l2.len() == 2;
    }
    let det = format!("r1={:016x} r2={:016x}", l1.root(), l2.root());
    s.add_dyn("50112-J1-根顺序敏感", ok, &det);

    // J1-7 重复作业拒绝且账不动（守恒）。
    let mut l = CoSignLedger::new();
    let mut ok = false;
    if let Ok(sa) = &a {
        let first = l.register(sa);
        let root_before = l.root();
        let len_before = l.len();
        let again = l.register(sa);
        ok = first.is_ok()
            && again.err() == Some(SignErr::Dup)
            && l.root() == root_before
            && l.len() == len_before;
    }
    s.add("50112-J1-重复守恒", ok, if ok { "" } else { "重复登记改了账" });

    // J1-8 容量：48 条可落，第 49 条 Capacity 且根不变。
    let mut full = CoSignLedger::new();
    let mut accepted = 0usize;
    let mut i = 0u32;
    while i < LEDGER_CAP as u32 {
        if let Ok(sess) = cosigned_session(5000 + i, false) {
            if full.register(&sess).is_ok() {
                accepted += 1;
            }
        }
        i += 1;
    }
    let root_before = full.root();
    let over = match cosigned_session(6000, false) {
        Ok(v) => full.register(&v),
        Err(e) => Err(e),
    };
    let ok = accepted == LEDGER_CAP
        && over.err() == Some(SignErr::Capacity)
        && full.root() == root_before
        && full.len() == LEDGER_CAP;
    let det = format!("acc={} len={}", accepted, full.len());
    s.add_dyn("50112-J1-容量封顶", ok, &det);

    // J1-9 账文本往返：根、条数、逐条摘要不变。
    let mut l3 = CoSignLedger::new();
    let mut built = false;
    if let (Ok(sa), Ok(sb)) = (&a, &b) {
        built = l3.register(sa).is_ok() && l3.register(sb).is_ok();
    }
    let text = encode_ledger(&l3);
    let parsed = parse_ledger(&text);
    let ok = built
        && match parsed {
            Ok(p) => p.root() == l3.root() && p.len() == l3.len() && p.entries() == l3.entries(),
            Err(_) => false,
        };
    let det = format!("len={} root={:016x}", l3.len(), l3.root());
    s.add_dyn("50112-J1-账本往返", ok, &det);

    // J1-10 确定性：同一流程两次构建根逐位一致。
    let x = cosigned_session(7001, false);
    let y = cosigned_session(7001, false);
    let mut la = CoSignLedger::new();
    let mut lb = CoSignLedger::new();
    let mut ok = false;
    if let (Ok(sx), Ok(sy)) = (&x, &y) {
        let ra = la.register(sx);
        let rb = lb.register(sy);
        ok = ra.is_ok() && rb.is_ok() && ra == rb && la.root() == lb.root();
    }
    let det = format!("ra={:016x} rb={:016x}", la.root(), lb.root());
    s.add_dyn("50112-J1-联签确定", ok, &det);

    // J1-11 未双签不得登记：落账拒绝且账保持为空。
    let mut l = CoSignLedger::new();
    let mut only_pt = match SignSession::new(match plan_at(1200) {
        Ok(p) => p,
        Err(_) => plan,
    }) {
        Ok(v) => v,
        Err(_) => SignSession {
            plan,
            state: SignState::Draft,
            pt: None,
            gdi: None,
            entry: None,
        },
    };
    let _ = only_pt.sign(Signer::Gdi);
    let rejected = l.register(&only_pt);
    let ok = rejected.err() == Some(SignErr::State) && l.is_empty() && l.root() == FNV_BASIS;
    s.add("50112-J1-未签不入账", ok, if ok { "" } else { "半签被记账" });

    // J1-12 封口后不可再签：State 拒绝，Sealed 保持。
    let mut sealed = match cosigned_session(7201, false) {
        Ok(v) => v,
        Err(_) => match SignSession::new(plan) {
            Ok(v) => v,
            Err(_) => SignSession {
                plan,
                state: SignState::Draft,
                pt: None,
                gdi: None,
                entry: None,
            },
        },
    };
    let did = sealed.seal();
    let after = sealed.sign(Signer::Gdi);
    let ok = did.is_ok() && after.err() == Some(SignErr::State) && sealed.state == SignState::Sealed;
    s.add("50112-J1-封口幂等", ok, if ok { "" } else { "封口后可改" });

    // J1-13 注入核 12/12。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50112-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50112_checks_green() {
        let set = run_unx50112_checks();
        assert!(set.all_passed(), "UNX-F50112 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50112_ticket_roundtrip() {
        if let Ok(p) = plan_at(300) {
            let bytes = encode_ticket(&p);
            assert_eq!(bytes.len(), TICKET_LEN);
            match parse_ticket(&bytes) {
                Ok(b) => assert_eq!(b, p),
                Err(_) => panic!("ticket rejected"),
            }
        }
    }

    #[test]
    fn unx50112_ledger_roundtrip() {
        if let (Ok(a), Ok(b)) = (cosigned_session(11, false), cosigned_session(12, true)) {
            let mut l = CoSignLedger::new();
            assert!(l.register(&a).is_ok());
            assert!(l.register(&b).is_ok());
            let text = encode_ledger(&l);
            match parse_ledger(&text) {
                Ok(p) => assert_eq!(p.root(), l.root()),
                Err(_) => panic!("ledger rejected"),
            }
        }
    }
}
