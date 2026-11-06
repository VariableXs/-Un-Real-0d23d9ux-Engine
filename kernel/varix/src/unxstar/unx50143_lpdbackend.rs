//! UNX-F50143 LPD 后端（LPR 协议 job 递交最小档）（0x43e00 段）
//!
//! 锚点：UNX-F50143-J1 递交 ×10 与模拟器对照一致；判据锚定 §AI-63 §2 专题五。
//!
//! LPD（RFC 1179）的 wire 格式是「命令字节 + 名字 + 载荷 + NUL」，看起来比
//! 任何自描述格式都简单，而它恰恰最容易在实现里埋三类坑：
//!
//! * **计数与载荷不一致**：控制文件声明 `count` 字节，实际多/少一个字节，
//!   后续所有帧全部错位。解析器必须按声明长度**精确切片**并在载荷后强制
//!   校验 NUL，任何偏移都不许「读到下一个命令字节再说」。
//! * **数据先于控制**：控制文件里的 `f`/`U` 行是数据文件名的唯一权威。
//!   若允许先收数据，收完才发现没有控制文件，作业就只剩一堆无法命名的
//!   字节。故顺序被状态机硬性固定：控制 → 数据 → 提交。
//! * **文件集合双向不一致**：控制文件登记了 `dfA` 却只来了 `dfB`，或来了
//!   未登记的文件。两个方向都必须显式失败（`NoData` / `DfMissing`），
//!   只查一个方向等于允许「偷偷丢页」。
//!
//! 数据结构：
//! * `ControlFile` / `DataFile` / `DecodedJob` —— 解析产物（控制行 + 数据摘要）；
//! * `LpdBackend` —— 四态递交状态机（Idle/Open/ControlHeld/DataHeld）；
//! * `JobLedger` —— 定容作业账本（queue/job 名去重、容量显式拒绝、首错留痕）。
//!
//! 错误路径：段内闭集 16 码，全部在注入核里被真实构造。
//! 本模块完全自包含（B22 之后不允许跨卡依赖）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43e00 独占 0x20）。
pub const SEG_BASE: u32 = 0x43e00;

pub const E_EMPTY: u16 = 1;
pub const E_OPCODE: u16 = 2;
pub const E_QUEUE: u16 = 3;
pub const E_QUEUE_LEN: u16 = 4;
pub const E_SUBCMD: u16 = 5;
pub const E_SUBNAME: u16 = 6;
pub const E_COUNT: u16 = 7;
pub const E_TRAILER: u16 = 8;
pub const E_CTL_ORDER: u16 = 9;
pub const E_CTL_LINE: u16 = 10;
pub const E_NO_CTL: u16 = 11;
pub const E_DF_MISSING: u16 = 12;
pub const E_NO_DATA: u16 = 13;
pub const E_CAPACITY: u16 = 14;
pub const E_DUP_JOB: u16 = 15;
pub const E_STATE: u16 = 16;

/// 闭集大小（账本按 slot 落槽）。
pub const ERR_SLOTS: usize = 16;

/// 队列名长度上限。RFC 1179 未规定，但打印机的队列名实际都很短；
/// 63 与 DNS 单标签上限一致，便于与发现结果互操作。
pub const MAX_QUEUE: usize = 63;

/// 子命令里数据文件名长度上限。
pub const MAX_SUBNAME: usize = 64;

/// 单个控制文件的载荷上限（1 MiB）。控制文件只是文本行，超过即视为攻击。
pub const MAX_CTL: usize = 1 << 20;

/// LPR 操作码。
pub const OP_PRINT_JOB: u8 = 0x02;
pub const OP_RECEIVE_CTL: u8 = 0x02;
pub const OP_RECEIVE_DATA: u8 = 0x03;

/// LPD 错误：`code()` = `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LpdError {
    Empty,
    Opcode,
    Queue,
    QueueLen,
    Subcmd,
    SubName,
    Count,
    Trailer,
    CtlOrder,
    CtlLine,
    NoCtl,
    DfMissing,
    NoData,
    Capacity,
    DupJob,
    State,
}

impl LpdError {
    /// 段内诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.slot() as u32 + 1
    }

    /// 账本槽位（0 基）。
    pub fn slot(self) -> usize {
        let n = match self {
            LpdError::Empty => E_EMPTY,
            LpdError::Opcode => E_OPCODE,
            LpdError::Queue => E_QUEUE,
            LpdError::QueueLen => E_QUEUE_LEN,
            LpdError::Subcmd => E_SUBCMD,
            LpdError::SubName => E_SUBNAME,
            LpdError::Count => E_COUNT,
            LpdError::Trailer => E_TRAILER,
            LpdError::CtlOrder => E_CTL_ORDER,
            LpdError::CtlLine => E_CTL_LINE,
            LpdError::NoCtl => E_NO_CTL,
            LpdError::DfMissing => E_DF_MISSING,
            LpdError::NoData => E_NO_DATA,
            LpdError::Capacity => E_CAPACITY,
            LpdError::DupJob => E_DUP_JOB,
            LpdError::State => E_STATE,
        };
        (n - 1) as usize
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            LpdError::Empty => "empty lpd script",
            LpdError::Opcode => "first byte is not 0x02",
            LpdError::Queue => "queue name empty or has illegal bytes",
            LpdError::QueueLen => "queue name exceeds 63 octets",
            LpdError::Subcmd => "unknown job subcommand",
            LpdError::SubName => "subcommand file name empty/illegal",
            LpdError::Count => "decimal count invalid or exceeds payload",
            LpdError::Trailer => "payload not terminated by NUL",
            LpdError::CtlOrder => "data before control, or duplicate control",
            LpdError::CtlLine => "control line tag/terminator invalid",
            LpdError::NoCtl => "job closed without control file",
            LpdError::DfMissing => "data file not registered in control file",
            LpdError::NoData => "control file lists a data file that never arrived",
            LpdError::Capacity => "ledger or control lines capacity exhausted",
            LpdError::DupJob => "same queue/job name submitted twice",
            LpdError::State => "illegal job phase transition",
        }
    }
}

/// 控制文件（RFC 1179 §7.2 的最小可用子集）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlFile {
    pub host: String,
    pub owner: String,
    pub job_name: String,
    pub class: String,
    pub banner: String,
    pub src_name: String,
    /// `f`/`U`/`l`/`S` 行登记的数据文件名（保序、可重复登记一次即够）。
    pub df_names: Vec<String>,
    /// 实际解析的控制行数。
    pub lines: u16,
}

impl ControlFile {
    /// 建空控制文件。
    pub fn new() -> ControlFile {
        ControlFile {
            host: String::new(),
            owner: String::new(),
            job_name: String::new(),
            class: String::new(),
            banner: String::new(),
            src_name: String::new(),
            df_names: Vec::new(),
            lines: 0,
        }
    }

    /// 是否登记过某数据文件名。
    pub fn has_df(&self, name: &str) -> bool {
        let mut i = 0usize;
        while i < self.df_names.len() {
            if let Some(n) = self.df_names.get(i) {
                if n == name {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 作业身份：`queue/jobname`，用于账本去重。
    pub fn key(&self, queue: &str) -> String {
        format!("{}/{}", queue, self.job_name)
    }
}

/// 数据文件：只保留名字、字节数与摘要——原文由调用方保管，账本不驻留大缓冲。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataFile {
    pub name: String,
    pub len: usize,
    pub digest: u32,
}

/// 一次完整递交的解析产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedJob {
    pub queue: String,
    pub ctl: ControlFile,
    pub datas: Vec<DataFile>,
}

impl DecodedJob {
    /// 数据文件个数。
    pub fn data_count(&self) -> usize {
        self.datas.len()
    }

    /// 所有数据文件字节总数。
    pub fn total_bytes(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.datas.len() {
            if let Some(d) = self.datas.get(i) {
                n += d.len;
            }
            i += 1;
        }
        n
    }
}

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = 0x811c_9dc5u32;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 字节读取原语：每个都带显式边界，绝不裸下标
// ---------------------------------------------------------------------------

fn read_byte(d: &[u8], pos: &mut usize, err: LpdError) -> Result<u8, LpdError> {
    match d.get(*pos) {
        Some(v) => {
            *pos += 1;
            Ok(*v)
        }
        None => Err(err),
    }
}

/// 读一行（消费 `\n`，返回不含 `\n` 的名字）。行内只允许可打印非空格 ASCII。
/// 长度上限设 255 只防无限增长；业务上的 63/64 上限由调用方校验函数判定，
/// 这样「超长」才会落到 `QueueLen` 而不是笼统的 `Queue`。
fn read_name(d: &[u8], pos: &mut usize, err: LpdError) -> Result<String, LpdError> {
    let mut out = String::new();
    loop {
        let c = match d.get(*pos) {
            Some(v) => *v,
            None => return Err(err),
        };
        *pos += 1;
        if c == b'\n' {
            break;
        }
        if c < 0x21 || c > 0x7e {
            return Err(err);
        }
        out.push(c as char);
        if out.len() > 255 {
            return Err(err);
        }
    }
    Ok(out)
}

/// 读十进制计数（消费其后的空格）。位数与数值都设上限，防整数溢出。
fn read_dec(d: &[u8], pos: &mut usize) -> Result<usize, LpdError> {
    let mut n = 0usize;
    let mut digits = 0usize;
    loop {
        let c = match d.get(*pos) {
            Some(v) => *v,
            None => return Err(LpdError::Count),
        };
        if c == b' ' {
            *pos += 1;
            break;
        }
        if c < b'0' || c > b'9' {
            return Err(LpdError::Count);
        }
        n = n * 10 + (c - b'0') as usize;
        digits += 1;
        if digits > 9 || n > MAX_CTL {
            return Err(LpdError::Count);
        }
        *pos += 1;
    }
    if digits == 0 {
        return Err(LpdError::Count);
    }
    Ok(n)
}

/// 取 n 字节（严格按声明长度切片；不足即 Count）。
fn take<'a>(d: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], LpdError> {
    let end = *pos + n;
    if end > d.len() {
        return Err(LpdError::Count);
    }
    match d.get(*pos..end) {
        Some(s) => {
            *pos = end;
            Ok(s)
        }
        None => Err(LpdError::Count),
    }
}

fn validate_queue(q: &str, err: LpdError) -> Result<(), LpdError> {
    if q.is_empty() {
        return Err(LpdError::Queue);
    }
    if q.len() > MAX_QUEUE {
        return Err(LpdError::QueueLen);
    }
    let b = q.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c < 0x21 || *c > 0x7e {
                return Err(err);
            }
        }
        i += 1;
    }
    Ok(())
}

fn validate_subname(n: &str) -> Result<(), LpdError> {
    if n.is_empty() || n.len() > MAX_SUBNAME {
        return Err(LpdError::SubName);
    }
    let b = n.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c < 0x21 || *c > 0x7e {
                return Err(LpdError::SubName);
            }
        }
        i += 1;
    }
    Ok(())
}

/// 解析控制文件正文：逐行按首字母分派。
///
/// 末行必须带 `\n`：缺换行意味着上一帧的 NUL 校验其实已经失败，在这里
/// 再兜一层可以避免「半个控制行」被当成完整行入库。
pub fn parse_control(body: &[u8]) -> Result<ControlFile, LpdError> {
    let mut c = ControlFile::new();
    let mut pos = 0usize;
    while pos < body.len() {
        let start = pos;
        while pos < body.len() {
            let b = match body.get(pos) {
                Some(v) => *v,
                None => break,
            };
            if b == b'\n' {
                break;
            }
            pos += 1;
        }
        if pos >= body.len() {
            return Err(LpdError::CtlLine);
        }
        let line = match body.get(start..pos) {
            Some(s) => s,
            None => return Err(LpdError::CtlLine),
        };
        pos += 1; // 吃掉 '\n'
        let tag = match line.first() {
            Some(t) => *t,
            None => return Err(LpdError::CtlLine),
        };
        let val = match line.get(1..) {
            Some(v) => v,
            None => return Err(LpdError::CtlLine),
        };
        // 值里任何控制字节都说明编码损坏，文本化会掩盖真实问题。
        let mut k = 0usize;
        while k < val.len() {
            if let Some(b) = val.get(k) {
                if *b < 0x20 || *b == 0x7f {
                    return Err(LpdError::CtlLine);
                }
            }
            k += 1;
        }
        let text = ascii_text(val);
        match tag {
            b'H' => c.host = text,
            b'P' => c.owner = text,
            b'J' => c.job_name = text,
            b'C' => c.class = text,
            b'L' => c.banner = text,
            b'N' => c.src_name = text,
            b'f' | b'U' | b'l' | b'S' => {
                if text.is_empty() {
                    return Err(LpdError::CtlLine);
                }
                c.df_names.push(text);
            }
            _ => return Err(LpdError::CtlLine),
        }
        c.lines = c.lines.saturating_add(1);
        if c.lines as usize > ERR_SLOTS * 4 {
            return Err(LpdError::Capacity);
        }
    }
    // H 与 P 是后端记账的必需字段：缺任何一个，作业都无法归属。
    if c.host.is_empty() || c.owner.is_empty() {
        return Err(LpdError::CtlLine);
    }
    Ok(c)
}

/// 把控制行值转成 ASCII 文本（上面已拒绝控制字节，这里只做字节→char）。
fn ascii_text(v: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < v.len() {
        if let Some(b) = v.get(i) {
            if *b < 0x80 {
                s.push(*b as char);
            } else {
                s.push('?');
            }
        }
        i += 1;
    }
    s
}

fn find_data<'a>(datas: &'a [DataFile], name: &str) -> Option<&'a DataFile> {
    let mut i = 0usize;
    while i < datas.len() {
        if let Some(d) = datas.get(i) {
            if d.name == name {
                return Some(d);
            }
        }
        i += 1;
    }
    None
}

/// 双向一致性：登记项必须都有落地文件，落地文件必须都被登记。
pub fn check_data_set(ctl: &ControlFile, datas: &[DataFile]) -> Result<(), LpdError> {
    let mut i = 0usize;
    while i < ctl.df_names.len() {
        if let Some(n) = ctl.df_names.get(i) {
            if find_data(datas, n.as_str()).is_none() {
                return Err(LpdError::NoData);
            }
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < datas.len() {
        if let Some(d) = datas.get(k) {
            if !ctl.has_df(d.name.as_str()) {
                return Err(LpdError::DfMissing);
            }
        }
        k += 1;
    }
    Ok(())
}

/// 解码一段完整的 LPR 递交脚本：`0x02 queue\n` + 控制帧 + 若干数据帧。
pub fn decode_job(data: &[u8]) -> Result<DecodedJob, LpdError> {
    if data.is_empty() {
        return Err(LpdError::Empty);
    }
    let mut pos = 0usize;
    let op = read_byte(data, &mut pos, LpdError::Opcode)?;
    if op != OP_PRINT_JOB {
        return Err(LpdError::Opcode);
    }
    let queue = read_name(data, &mut pos, LpdError::Queue)?;
    validate_queue(queue.as_str(), LpdError::Queue)?;
    let mut ctl: Option<ControlFile> = None;
    let mut datas: Vec<DataFile> = Vec::new();
    loop {
        if pos >= data.len() {
            break;
        }
        let sub = read_byte(data, &mut pos, LpdError::Subcmd)?;
        match sub {
            OP_RECEIVE_CTL => {
                // 控制文件只能有一个，且必须先于任何数据文件。
                if ctl.is_some() || !datas.is_empty() {
                    return Err(LpdError::CtlOrder);
                }
                let count = read_dec(data, &mut pos)?;
                let name = read_name(data, &mut pos, LpdError::SubName)?;
                validate_subname(name.as_str())?;
                let body = take(data, &mut pos, count)?;
                let nul = read_byte(data, &mut pos, LpdError::Trailer)?;
                if nul != 0 {
                    return Err(LpdError::Trailer);
                }
                let parsed = parse_control(body)?;
                ctl = Some(parsed);
            }
            OP_RECEIVE_DATA => {
                if ctl.is_none() {
                    return Err(LpdError::CtlOrder);
                }
                let count = read_dec(data, &mut pos)?;
                let name = read_name(data, &mut pos, LpdError::SubName)?;
                validate_subname(name.as_str())?;
                let body = take(data, &mut pos, count)?;
                let nul = read_byte(data, &mut pos, LpdError::Trailer)?;
                if nul != 0 {
                    return Err(LpdError::Trailer);
                }
                datas.push(DataFile {
                    name,
                    len: body.len(),
                    digest: fnv1a(body),
                });
            }
            _ => return Err(LpdError::Subcmd),
        }
    }
    let ctl = match ctl {
        Some(c) => c,
        None => return Err(LpdError::NoCtl),
    };
    check_data_set(&ctl, datas.as_slice())?;
    Ok(DecodedJob { queue, ctl, datas })
}

// ---------------------------------------------------------------------------
// 递交状态机 + 作业账本
// ---------------------------------------------------------------------------

/// 递交阶段。顺序是硬约束：Idle → Open → ControlHeld → DataHeld → Idle。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobPhase {
    Idle,
    Open,
    ControlHeld,
    DataHeld,
}

impl JobPhase {
    /// 阶段名（判据明细/日志用）。
    pub fn label(self) -> &'static str {
        match self {
            JobPhase::Idle => "idle",
            JobPhase::Open => "open",
            JobPhase::ControlHeld => "control",
            JobPhase::DataHeld => "data",
        }
    }
}

/// 账本条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobEntry {
    pub key: String,
    pub queue: String,
    pub owner: String,
    pub df: String,
    pub data_len: usize,
    pub digest: u32,
}

/// 定容作业账本。
#[derive(Clone, Debug)]
pub struct JobLedger {
    jobs: Vec<JobEntry>,
    cap: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub err_counts: [u16; ERR_SLOTS],
    pub first_err: u32,
}

impl JobLedger {
    /// 建账本（条数上限 cap）。
    pub fn new(cap: usize) -> JobLedger {
        JobLedger {
            jobs: Vec::new(),
            cap,
            accepted: 0,
            rejected: 0,
            err_counts: [0u16; ERR_SLOTS],
            first_err: 0,
        }
    }

    /// 落一笔作业：容量与重复键都显式失败。
    pub fn push(&mut self, job: &DecodedJob) -> Result<usize, LpdError> {
        let key = job.ctl.key(job.queue.as_str());
        let mut i = 0usize;
        while i < self.jobs.len() {
            if let Some(j) = self.jobs.get(i) {
                if j.key == key {
                    self.note_err(LpdError::DupJob);
                    return Err(LpdError::DupJob);
                }
            }
            i += 1;
        }
        if self.jobs.len() >= self.cap {
            self.note_err(LpdError::Capacity);
            return Err(LpdError::Capacity);
        }
        let df = match job.datas.first() {
            Some(d) => d.name.clone(),
            None => String::new(),
        };
        let mut h = 0x811c_9dc5u32;
        let mut k = 0usize;
        while k < job.datas.len() {
            if let Some(d) = job.datas.get(k) {
                h ^= d.digest;
                h = h.wrapping_mul(0x0100_0193);
                h ^= d.len as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            k += 1;
        }
        self.jobs.push(JobEntry {
            key,
            queue: job.queue.clone(),
            owner: job.ctl.owner.clone(),
            df,
            data_len: job.total_bytes(),
            digest: h,
        });
        self.accepted += 1;
        Ok(self.jobs.len() - 1)
    }

    /// 记一次拒绝。
    pub fn note_err(&mut self, e: LpdError) {
        self.rejected += 1;
        let slot = e.slot();
        if let Some(c) = self.err_counts.get_mut(slot) {
            *c = c.saturating_add(1);
        }
        if self.first_err == 0 {
            self.first_err = e.code();
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    /// 某类错误计数。
    pub fn err_count(&self, e: LpdError) -> u16 {
        match self.err_counts.get(e.slot()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 条目访问。
    pub fn entry(&self, i: usize) -> Option<&JobEntry> {
        self.jobs.get(i)
    }
}

/// LPD 后端：把「流式递交」与「整段解码」两条路径都收敛到同一账本。
#[derive(Clone, Debug)]
pub struct LpdBackend {
    phase: JobPhase,
    queue: String,
    ctl: Option<ControlFile>,
    datas: Vec<DataFile>,
    ledger: JobLedger,
    /// 合法迁移次数（可审计的活性指标）。
    pub steps_ok: usize,
    /// 被拒迁移次数。
    pub steps_rejected: usize,
}

impl LpdBackend {
    /// 建后端。
    pub fn new(cap: usize) -> LpdBackend {
        LpdBackend {
            phase: JobPhase::Idle,
            queue: String::new(),
            ctl: None,
            datas: Vec::new(),
            ledger: JobLedger::new(cap),
            steps_ok: 0,
            steps_rejected: 0,
        }
    }

    /// 当前阶段。
    pub fn phase(&self) -> JobPhase {
        self.phase
    }

    /// 账本只读访问。
    pub fn ledger(&self) -> &JobLedger {
        &self.ledger
    }

    /// 开作业：仅 Idle 可迁。
    pub fn begin(&mut self, queue: &str) -> Result<(), LpdError> {
        if self.phase != JobPhase::Idle {
            self.steps_rejected += 1;
            return Err(LpdError::State);
        }
        validate_queue(queue, LpdError::Queue)?;
        self.queue = queue.to_string();
        self.ctl = None;
        self.datas.clear();
        self.phase = JobPhase::Open;
        self.steps_ok += 1;
        Ok(())
    }

    /// 挂控制文件：仅 Open 可迁。
    pub fn control(&mut self, body: &[u8]) -> Result<(), LpdError> {
        if self.phase != JobPhase::Open {
            self.steps_rejected += 1;
            return Err(LpdError::State);
        }
        match parse_control(body) {
            Ok(c) => {
                self.ctl = Some(c);
                self.phase = JobPhase::ControlHeld;
                self.steps_ok += 1;
                Ok(())
            }
            Err(e) => {
                self.steps_rejected += 1;
                Err(e)
            }
        }
    }

    /// 挂数据文件：仅 ControlHeld/DataHeld 可迁（可多份）。
    pub fn data(&mut self, name: &str, body: &[u8]) -> Result<(), LpdError> {
        if self.phase != JobPhase::ControlHeld && self.phase != JobPhase::DataHeld {
            self.steps_rejected += 1;
            return Err(LpdError::State);
        }
        validate_subname(name)?;
        if find_data(self.datas.as_slice(), name).is_some() {
            // 同名数据文件重复：属于控制文件登记集合的一致性冲突。
            self.steps_rejected += 1;
            return Err(LpdError::DfMissing);
        }
        self.datas.push(DataFile {
            name: name.to_string(),
            len: body.len(),
            digest: fnv1a(body),
        });
        self.phase = JobPhase::DataHeld;
        self.steps_ok += 1;
        Ok(())
    }

    /// 提交：仅 DataHeld 可迁；成功返回账本下标。
    pub fn commit(&mut self) -> Result<usize, LpdError> {
        if self.phase != JobPhase::DataHeld {
            self.steps_rejected += 1;
            return Err(LpdError::State);
        }
        let ctl = match self.ctl.take() {
            Some(c) => c,
            None => {
                self.steps_rejected += 1;
                return Err(LpdError::NoCtl);
            }
        };
        // 一致性失败时把控制文件放回：作业并未消失，调用方可补数据后重试。
        if let Err(e) = check_data_set(&ctl, self.datas.as_slice()) {
            self.ctl = Some(ctl);
            self.steps_rejected += 1;
            return Err(e);
        }
        let job = DecodedJob {
            queue: self.queue.clone(),
            ctl,
            datas: self.datas.clone(),
        };
        let idx = match self.ledger.push(&job) {
            Ok(i) => i,
            Err(e) => {
                self.steps_rejected += 1;
                self.ctl = Some(job.ctl);
                return Err(e);
            }
        };
        self.queue = String::new();
        self.datas.clear();
        self.phase = JobPhase::Idle;
        self.steps_ok += 1;
        Ok(idx)
    }

    /// 整段递交：解码 + 落账（不走阶段机，供批处理路径使用）。
    pub fn accept(&mut self, script: &[u8]) -> Result<usize, LpdError> {
        let job = decode_job(script)?;
        self.ledger.push(&job)
    }
}

// ---------------------------------------------------------------------------
// 10 份递交模型（模拟器对照基准）
// ---------------------------------------------------------------------------

/// 一份递交模型的全部显式字段（对照时逐字段比较）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    pub queue: String,
    pub host: String,
    pub owner: String,
    pub job: String,
    pub class: String,
    pub banner: String,
    pub src: String,
    pub df: String,
    pub body: String,
}

fn sub(
    queue: &str,
    host: &str,
    owner: &str,
    job: &str,
    class: &str,
    banner: &str,
    src: &str,
    df: &str,
    body: &str,
) -> Submission {
    Submission {
        queue: queue.to_string(),
        host: host.to_string(),
        owner: owner.to_string(),
        job: job.to_string(),
        class: class.to_string(),
        banner: banner.to_string(),
        src: src.to_string(),
        df: df.to_string(),
        body: body.to_string(),
    }
}

/// 10 份模型：覆盖不同队列名、用户、类、横幅与载荷长度（含 0 字节载荷）。
pub fn submissions() -> Vec<Submission> {
    let mut v: Vec<Submission> = Vec::new();
    v.push(sub("lp", "pc-a", "alice", "job001", "text", "banner0", "note.txt", "dfA001", "hello world\n"));
    v.push(sub("lp", "pc-b", "bob", "job002", "text", "banner1", "img.ps", "dfA002", "%!PS\n"));
    v.push(sub("color", "pc-c", "carol", "job003", "raw", "banner2", "a.bin", "dfA003", "0123456789"));
    v.push(sub("color", "pc-d", "dave", "job004", "text", "banner3", "b.txt", "dfA004", ""));
    v.push(sub("st10", "pc-e", "erin", "job005", "text", "banner4", "c.txt", "dfA005", "123"));
    v.push(sub("st10", "pc-f", "frank", "job006", "raw", "banner5", "d.rst", "dfA006", "line1\nline2\n"));
    v.push(sub("lp2", "pc-g", "grace", "job007", "text", "banner6", "e.txt", "dfA007", "abcdefghijklmnop"));
    v.push(sub("lp2", "pc-h", "heidi", "job008", "text", "banner7", "f.txt", "dfA008", "\u{7f}"));
    v.push(sub("big", "pc-i", "ivan", "job009", "raw", "banner8", "g.pcl", "dfA009", "x"));
    v.push(sub("big", "pc-j", "judy", "job010", "text", "banner9", "h.txt", "dfA010", "final"));
    v
}

/// 编码一份递交：控制帧 + 数据帧，帧尾强制 NUL。
pub fn encode_job(s: &Submission) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(OP_PRINT_JOB);
    push_ascii(&mut v, s.queue.as_str());
    v.push(b'\n');
    // 控制文件正文
    let mut body: Vec<u8> = Vec::new();
    push_ctl_line(&mut body, b'H', s.host.as_str());
    push_ctl_line(&mut body, b'P', s.owner.as_str());
    push_ctl_line(&mut body, b'J', s.job.as_str());
    push_ctl_line(&mut body, b'C', s.class.as_str());
    push_ctl_line(&mut body, b'L', s.banner.as_str());
    push_ctl_line(&mut body, b'N', s.src.as_str());
    push_ctl_line(&mut body, b'f', s.df.as_str());
    v.push(OP_RECEIVE_CTL);
    push_dec(&mut v, body.len());
    v.push(b' ');
    push_ascii(&mut v, "cfA.job");
    v.push(b'\n');
    v.extend_from_slice(body.as_slice());
    v.push(0);
    // 数据帧
    v.push(OP_RECEIVE_DATA);
    push_dec(&mut v, s.body.len());
    v.push(b' ');
    push_ascii(&mut v, s.df.as_str());
    v.push(b'\n');
    push_ascii(&mut v, s.body.as_str());
    v.push(0);
    v
}

fn push_ascii(v: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            v.push(*c);
        }
        i += 1;
    }
}

fn push_ctl_line(v: &mut Vec<u8>, tag: u8, value: &str) {
    v.push(tag);
    push_ascii(v, value);
    v.push(b'\n');
}

fn push_dec(v: &mut Vec<u8>, mut n: usize) {
    let mut buf: [u8; 20] = [0u8; 20];
    let mut i = 0usize;
    if n == 0 {
        v.push(b'0');
        return;
    }
    while n > 0 && i < 20 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        if let Some(c) = buf.get(i) {
            v.push(*c);
        }
    }
}

// ---------------------------------------------------------------------------
// 注入核：16 类坏样本
// ---------------------------------------------------------------------------

fn header(queue: &str) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(OP_PRINT_JOB);
    push_ascii(&mut v, queue);
    v.push(b'\n');
    v
}

fn simple_ctl_body() -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    push_ctl_line(&mut b, b'H', "h");
    push_ctl_line(&mut b, b'P', "p");
    push_ctl_line(&mut b, b'J', "j");
    push_ctl_line(&mut b, b'f', "dfA");
    b
}

/// 拼一个控制帧（帧头 + 正文 + NUL）。
fn ctl_frame(body: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(OP_RECEIVE_CTL);
    push_dec(&mut v, body.len());
    v.push(b' ');
    push_ascii(&mut v, "cfA");
    v.push(b'\n');
    v.extend_from_slice(body);
    v.push(0);
    v
}

/// 拼一个数据帧。
fn data_frame(name: &str, body: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(OP_RECEIVE_DATA);
    push_dec(&mut v, body.len());
    v.push(b' ');
    push_ascii(&mut v, name);
    v.push(b'\n');
    v.extend_from_slice(body);
    v.push(0);
    v
}

/// 注入核返回（命中，总数，码位掩码）。
pub fn injection_core() -> (usize, usize, u32) {
    let total = 16usize;
    let mut caught = 0usize;
    let mut mask = 0u32;

    let mut good: Vec<u8> = header("lp");
    good.extend_from_slice(ctl_frame(simple_ctl_body().as_slice()).as_slice());
    good.extend_from_slice(data_frame("dfA", b"xy").as_slice());

    let mut cases: Vec<(Vec<u8>, LpdError)> = Vec::new();
    cases.push((Vec::new(), LpdError::Empty));
    cases.push((bad_opcode(), LpdError::Opcode));
    cases.push((bad_queue_name(), LpdError::Queue));
    cases.push((bad_queue_len(), LpdError::QueueLen));
    cases.push((bad_subcmd(&good), LpdError::Subcmd));
    cases.push((bad_subname(), LpdError::SubName));
    cases.push((bad_count(), LpdError::Count));
    cases.push((bad_trailer(&good), LpdError::Trailer));
    cases.push((data_first(), LpdError::CtlOrder));
    cases.push((bad_ctl_line(), LpdError::CtlLine));
    cases.push((no_ctl(), LpdError::NoCtl));
    cases.push((df_missing(), LpdError::DfMissing));
    cases.push((no_data(), LpdError::NoData));

    let mut i = 0usize;
    while i < cases.len() {
        if let Some((bytes, want)) = cases.get(i) {
            if decode_job(bytes).err() == Some(*want) {
                caught += 1;
                mask |= 1u32 << want.slot();
            }
        }
        i += 1;
    }
    // 容量：cap=1，第二笔必须换成另一个 job 名（否则先命中 DupJob）
    let mut led = JobLedger::new(1);
    let alt = encode_job(&sub("lp", "h", "p", "k", "c", "b", "s", "dfA", "xy"));
    let mut cap_hit = false;
    if let (Ok(j1), Ok(j2)) = (decode_job(good.as_slice()), decode_job(alt.as_slice())) {
        if led.push(&j1).is_ok() && led.push(&j2).err() == Some(LpdError::Capacity) {
            cap_hit = true;
        }
    }
    if cap_hit {
        caught += 1;
        mask |= 1u32 << LpdError::Capacity.slot();
    }
    // 重复作业：cap=8 下同一 key 第二笔
    let mut led2 = JobLedger::new(8);
    if let Ok(j) = decode_job(good.as_slice()) {
        let _ = led2.push(&j);
        if led2.push(&j).err() == Some(LpdError::DupJob) {
            caught += 1;
            mask |= 1u32 << LpdError::DupJob.slot();
        }
    }
    // 状态机非法迁移
    let mut be = LpdBackend::new(4);
    let mut state_hits = 0usize;
    if be.data("dfA", b"x").is_err() {
        state_hits += 1;
    }
    if be.begin("lp").is_ok() && be.begin("lp2").is_err() {
        state_hits += 1;
    }
    if be.commit().is_err() {
        state_hits += 1;
    }
    if state_hits == 3 {
        caught += 1;
        mask |= 1u32 << LpdError::State.slot();
    }
    (caught, total, mask)
}

fn bad_opcode() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(0x01);
    push_ascii(&mut v, "lp\n");
    v
}

fn bad_queue_name() -> Vec<u8> {
    header("lp bad")
}

fn bad_queue_len() -> Vec<u8> {
    let mut q = String::new();
    let mut i = 0usize;
    while i < 70 {
        q.push('q');
        i += 1;
    }
    header(q.as_str())
}

fn bad_subcmd(good: &[u8]) -> Vec<u8> {
    let mut v = good.to_vec();
    // 在数据帧之后追加一个未知子命令
    v.push(0x07);
    v
}

fn bad_subname() -> Vec<u8> {
    let mut v = header("lp");
    v.push(OP_RECEIVE_CTL);
    push_dec(&mut v, 8);
    v.push(b' ');
    v.push(b'\n'); // 空文件名
    v.extend_from_slice(b"H h\nP p\n");
    v.push(0);
    v
}

fn bad_count() -> Vec<u8> {
    let mut v = header("lp");
    v.push(OP_RECEIVE_CTL);
    v.push(b'x'); // 非十进制
    v.push(b' ');
    push_ascii(&mut v, "cfA\n");
    v
}

fn bad_trailer(good: &[u8]) -> Vec<u8> {
    let mut v = good.to_vec();
    if let Some(last) = v.last_mut() {
        *last = b'X'; // 末 NUL 改成普通字节
    }
    v
}

fn data_first() -> Vec<u8> {
    let mut v = header("lp");
    v.extend_from_slice(data_frame("dfA", b"xy").as_slice());
    v
}

fn bad_ctl_line() -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    push_ctl_line(&mut body, b'H', "h");
    push_ctl_line(&mut body, b'P', "p");
    push_ctl_line(&mut body, b'Z', "bogus");
    let mut v = header("lp");
    v.extend_from_slice(ctl_frame(body.as_slice()).as_slice());
    v
}

fn no_ctl() -> Vec<u8> {
    header("lp")
}

fn df_missing() -> Vec<u8> {
    let mut v = header("lp");
    v.extend_from_slice(ctl_frame(simple_ctl_body().as_slice()).as_slice());
    // dfA 已登记且有落地，dfB 是未登记的额外文件 → DfMissing
    v.extend_from_slice(data_frame("dfA", b"xy").as_slice());
    v.extend_from_slice(data_frame("dfB", b"z").as_slice());
    v
}

fn no_data() -> Vec<u8> {
    let mut v = header("lp");
    v.extend_from_slice(ctl_frame(simple_ctl_body().as_slice()).as_slice());
    v
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50143 自检：10 份递交与模型逐字段对照 + 状态机 + 16 类注入。
pub fn run_unx50143_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50143");
    let models = submissions();

    // J1-1 10 份递交全部解码成功（整段路径）
    let mut decoded: Vec<DecodedJob> = Vec::new();
    let mut ok_count = 0usize;
    let mut i = 0usize;
    while i < models.len() {
        if let Some(m) = models.get(i) {
            let bytes = encode_job(m);
            match decode_job(bytes.as_slice()) {
                Ok(j) => {
                    ok_count += 1;
                    decoded.push(j);
                }
                Err(_) => {}
            }
        }
        i += 1;
    }
    let ok = ok_count == 10 && decoded.len() == 10;
    let det = format!("decoded={}/10", ok_count);
    s.add_dyn("50143-J1-十份可解", ok, &det);

    // J1-2 逐字段对照：10 份 × 10 字段 = 100 项
    let mut matched = 0usize;
    let mut compared = 0usize;
    let mut k = 0usize;
    while k < models.len() {
        if let (Some(m), Some(j)) = (models.get(k), decoded.get(k)) {
            let df = match j.datas.first() {
                Some(d) => d,
                None => {
                    compared += 1;
                    k += 1;
                    continue;
                }
            };
            let checks: [bool; 10] = [
                j.queue == m.queue,
                j.ctl.host == m.host,
                j.ctl.owner == m.owner,
                j.ctl.job_name == m.job,
                j.ctl.class == m.class,
                j.ctl.banner == m.banner,
                j.ctl.src_name == m.src,
                df.name == m.df,
                df.len == m.body.len(),
                df.digest == fnv1a(m.body.as_bytes()),
            ];
            let mut c = 0usize;
            while c < checks.len() {
                if let Some(passed) = checks.get(c) {
                    if *passed {
                        matched += 1;
                    }
                    compared += 1;
                }
                c += 1;
            }
        }
        k += 1;
    }
    let ok = compared == 100 && matched == 100;
    let det = format!("matched={}/{}", matched, compared);
    s.add_dyn("50143-J1-逐字段一致", ok, &det);

    // J1-3 零字节载荷也是合法作业：第 4 份 data len = 0
    let mut zero_ok = false;
    if let Some(j) = decoded.get(3) {
        if let Some(d) = j.datas.first() {
            zero_ok = d.len == 0 && j.data_count() == 1;
        }
    }
    s.add("50143-J1-零字节载荷", zero_ok, if zero_ok { "" } else { "空数据文件被拒或长度错" });

    // J1-4 控制行保真：J 行与 f 行跨 10 份逐份一致
    let mut ctl_ok = 0usize;
    let mut ci = 0usize;
    while ci < models.len() {
        if let (Some(m), Some(j)) = (models.get(ci), decoded.get(ci)) {
            if j.ctl.has_df(m.df.as_str()) && j.ctl.lines >= 7 {
                ctl_ok += 1;
            }
        }
        ci += 1;
    }
    let ok = ctl_ok == 10;
    let det = format!("ctl_ok={}/10", ctl_ok);
    s.add_dyn("50143-J1-控制行保真", ok, &det);

    // J1-5 队列名边界：63 通过名字校验（仅缺控制文件 → NoCtl）、64 拒绝
    let mut max_q = String::new();
    let mut n = 0usize;
    while n < MAX_QUEUE {
        max_q.push('q');
        n += 1;
    }
    let max_ok = decode_job(header(max_q.as_str()).as_slice()).err() == Some(LpdError::NoCtl);
    let mut over = String::new();
    let mut m2 = 0usize;
    while m2 <= MAX_QUEUE {
        over.push('q');
        m2 += 1;
    }
    let over_rej = decode_job(header(over.as_str()).as_slice()).err() == Some(LpdError::QueueLen);
    let ok = max_ok && over_rej;
    s.add("50143-J1-队列名边界", ok, if ok { "" } else { "63/64 边界判错" });

    // J1-6 数据先于控制必拒
    let ok = decode_job(data_first().as_slice()).err() == Some(LpdError::CtlOrder)
        && decode_job(no_ctl().as_slice()).err() == Some(LpdError::NoCtl);
    s.add("50143-J1-控制必须先到", ok, if ok { "" } else { "顺序约束失效" });

    // J1-7 数据集合双向一致：缺落地 / 多未登记
    let ok = decode_job(no_data().as_slice()).err() == Some(LpdError::NoData)
        && decode_job(df_missing().as_slice()).err() == Some(LpdError::DfMissing);
    s.add("50143-J1-双向一致", ok, if ok { "" } else { "单方向校验" });

    // J1-8 计数与载荷：声明越界 / 非十进制 / 缺 NUL
    let sample_xy = encode_job(&sub("lp", "h", "p", "j", "c", "b", "s", "dfA", "xy"));
    let trunc = bad_trailer(sample_xy.as_slice());
    let ok = decode_job(bad_count().as_slice()).err() == Some(LpdError::Count)
        && decode_job(trunc.as_slice()).err() == Some(LpdError::Trailer);
    s.add("50143-J1-计数与NUL", ok, if ok { "" } else { "计数/NUL 未校验" });

    // J1-9 未知子命令与非法控制行
    let with_junk = bad_subcmd(sample_xy.as_slice());
    let ok = decode_job(with_junk.as_slice()).err() == Some(LpdError::Subcmd)
        && decode_job(bad_ctl_line().as_slice()).err() == Some(LpdError::CtlLine);
    s.add("50143-J1-未知与非法行", ok, if ok { "" } else { "未知字节被容忍" });

    // J1-10 状态机合法链：Idle→Open→ControlHeld→DataHeld→Idle
    let mut be = LpdBackend::new(4);
    let mut chain = 0usize;
    if be.begin("lp").is_ok() && be.phase() == JobPhase::Open {
        chain += 1;
    }
    if be.control(simple_ctl_body().as_slice()).is_ok() && be.phase() == JobPhase::ControlHeld {
        chain += 1;
    }
    if be.data("dfA", b"xy").is_ok() && be.phase() == JobPhase::DataHeld {
        chain += 1;
    }
    let committed = be.commit();
    if committed.is_ok() && be.phase() == JobPhase::Idle {
        chain += 1;
    }
    let ok = chain == 4 && be.ledger().len() == 1;
    let det = format!("chain={}/4 depth={}", chain, be.ledger().len());
    s.add_dyn("50143-J1-状态机链", ok, &det);

    // J1-11 状态机非法迁移：未开始就送数据 / 重复 begin / 未收数据就 commit
    let mut be2 = LpdBackend::new(4);
    let a = be2.data("dfA", b"x").err() == Some(LpdError::State);
    let b = {
        let first = be2.begin("lp").is_ok();
        first && be2.begin("lp2").err() == Some(LpdError::State)
    };
    let c = be2.commit().err() == Some(LpdError::State);
    let phase_kept = be2.phase() == JobPhase::Open;
    let ok = a && b && c && phase_kept;
    s.add("50143-J1-非法迁移", ok, if ok { "" } else { "非法迁移放行或阶段被污染" });

    // J1-12 账本容量与重复作业
    let sample = encode_job(&sub("lp", "h", "p", "j", "c", "b", "s", "dfA", "xy"));
    let alt = encode_job(&sub("lp", "h", "p", "k", "c", "b", "s", "dfA", "xy"));
    let mut led = JobLedger::new(1);
    let mut cap_ok = false;
    if let (Ok(j1), Ok(j2)) = (decode_job(sample.as_slice()), decode_job(alt.as_slice())) {
        if led.push(&j1).is_ok() {
            cap_ok = led.push(&j2).err() == Some(LpdError::Capacity) && led.len() == 1;
        }
    }
    let mut led2 = JobLedger::new(8);
    let mut dup_ok = false;
    if let Ok(j) = decode_job(sample.as_slice()) {
        if led2.push(&j).is_ok() {
            dup_ok = led2.push(&j).err() == Some(LpdError::DupJob) && led2.accepted == 1;
        }
    }
    let ok = cap_ok && dup_ok;
    let det = format!("cap={} dup={}", cap_ok, dup_ok);
    s.add_dyn("50143-J1-账本边界", ok, &det);

    // J1-13 账本摘要守恒：10 份递交的数据摘要与模型各自独立计算一致
    let mut led3 = JobLedger::new(16);
    let mut pushed = 0usize;
    let mut ii = 0usize;
    while ii < models.len() {
        if let Some(m) = models.get(ii) {
            if let Ok(j) = decode_job(encode_job(m).as_slice()) {
                if led3.push(&j).is_ok() {
                    pushed += 1;
                }
            }
        }
        ii += 1;
    }
    let mut digest_ok = pushed == 10;
    let mut d = 0usize;
    while d < models.len() {
        if let (Some(m), Some(e)) = (models.get(d), led3.entry(d)) {
            let mut h = 0x811c_9dc5u32;
            h ^= fnv1a(m.body.as_bytes());
            h = h.wrapping_mul(0x0100_0193);
            h ^= m.body.len() as u32;
            h = h.wrapping_mul(0x0100_0193);
            if e.digest != h || e.data_len != m.body.len() {
                digest_ok = false;
            }
        }
        d += 1;
    }
    s.add("50143-J1-摘要守恒", digest_ok, if digest_ok { "" } else { "账本摘要与模型不符" });

    // J1-14 坏包记账：5 类逐类计数
    let mut bad = JobLedger::new(4);
    let bads: [LpdError; 5] = [
        LpdError::Empty,
        LpdError::Opcode,
        LpdError::Queue,
        LpdError::CtlLine,
        LpdError::NoData,
    ];
    let bad_bytes: [Vec<u8>; 5] = [
        Vec::new(),
        bad_opcode(),
        bad_queue_name(),
        bad_ctl_line(),
        no_data(),
    ];
    let mut b = 0usize;
    while b < bad_bytes.len() {
        if let (Some(bytes), Some(want)) = (bad_bytes.get(b), bads.get(b)) {
            match decode_job(bytes.as_slice()) {
                Err(e) if e == *want => bad.note_err(e),
                _ => {}
            }
        }
        b += 1;
    }
    let mut per_class = true;
    let mut cc = 0usize;
    while cc < bads.len() {
        if let Some(e) = bads.get(cc) {
            if bad.err_count(*e) != 1 {
                per_class = false;
            }
        }
        cc += 1;
    }
    let ok = bad.rejected == 5 && bad.accepted == 0 && per_class;
    let det = format!("rej={} first={:x}", bad.rejected, bad.first_err);
    s.add_dyn("50143-J1-坏包记账", ok, &det);

    // J1-15 递交确定性：同脚本两次解码结果逐字段一致
    let mut det_ok = true;
    if let Some(m) = models.first() {
        let bytes = encode_job(m);
        match (decode_job(bytes.as_slice()), decode_job(bytes.as_slice())) {
            (Ok(a), Ok(b)) => {
                if a != b {
                    det_ok = false;
                }
            }
            _ => det_ok = false,
        }
    }
    s.add("50143-J1-递交确定", det_ok, if det_ok { "" } else { "两次解码不一致" });

    // J1-16 注入核：16/16 且码位全覆盖
    let (caught, total, mask) = injection_core();
    let full = (1u32 << ERR_SLOTS) - 1;
    let ok = caught == total && mask == full;
    let det = format!("注入 {}/{} mask={:x}", caught, total, mask);
    s.add_dyn("50143-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50143_checks_green() {
        let set = run_unx50143_checks();
        assert!(set.all_passed(), "UNX-F50143 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50143_submission_roundtrip() {
        let models = submissions();
        let mut i = 0usize;
        while i < models.len() {
            let m = match models.get(i) {
                Some(v) => v,
                None => return,
            };
            let bytes = encode_job(m);
            let j = match decode_job(bytes.as_slice()) {
                Ok(v) => v,
                Err(e) => {
                    assert!(false, "decode failed at {}: {:?}", i, e);
                    return;
                }
            };
            assert_eq!(j.queue, m.queue);
            assert_eq!(j.ctl.owner, m.owner);
            assert_eq!(j.ctl.job_name, m.job);
            assert_eq!(j.total_bytes(), m.body.len());
            i += 1;
        }
    }

    #[test]
    fn unx50143_injection_covers_all_codes() {
        let (caught, total, mask) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(mask, (1u32 << ERR_SLOTS) - 1);
    }

    #[test]
    fn unx50143_phase_machine_rejects_skip() {
        let mut be = LpdBackend::new(2);
        assert_eq!(be.data("dfA", b"x").err(), Some(LpdError::State));
        assert_eq!(be.commit().err(), Some(LpdError::State));
        assert!(be.begin("lp").is_ok());
        assert_eq!(be.phase(), JobPhase::Open);
    }
}
