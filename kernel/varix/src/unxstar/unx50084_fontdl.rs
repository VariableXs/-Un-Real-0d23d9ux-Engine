//! UNX-F50084 字体下载通道（PCL/PS 载体两档）（0x436a0 段）
//!
//! 锚点：UNX-F50084-J1 下载 ×10 会话零残留断言；判据锚定 §AI-63 §2 专题二。
//!
//! 为什么下载通道必须自己记账：字体下载是「头声明长度 → 二进制/十六进制
//! 载荷 → 尾校验」的长事务，打印机的接收缓冲只有几 KiB，链路上一旦多一个
//! 字节或少一个字节，后续所有打印作业的字节边界都会错位（经典串流事故）。
//! 因此本模块把「声明长度 / 实收长度 / 分块数 / 校验和 / 尾后残留」五个量
//! 全部作为一等公民，`DlSession` 逐块泵动，任何一项不符都显式失败。
//!
//! 两档载体：
//! * PCL：`ESC(s<十进制长度>W` + 原始字节 + `ESC(s1Q<8位校验和>\n`；
//! * PS ：`%!PS…/FontSize <长度> def…currentfile eexec` + ASCII-hex +
//!   `%%Checksum: <8位>\n` + 512 个 `0` + `cleartomark`。
//! 两档的尾校验都必须是「载荷的 FNV-1a」，这样载体错位会在尾校验处
//! 立刻暴露，而不是等到光栅化时才发现字形是乱码。
//!
//! 结构清单：
//! * 状态机：`DlChannel`（Idle→Opened→Receiving→Sealed/Aborted）；
//! * 增量解析器：`DlSession`（Header→Payload→Trailer→Done，可跨块喂入）；
//! * 账本：`DlLedger`（定容 append-only）；
//! * 注入核：`injection_core` 10 类坏样本；
//! * 诊断码闭集：SEG_BASE = 0x436a0，n ∈ [1,12]。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x436a0 独占 0x20；F50085 从 0x436c0 起顺序占用）。
pub const SEG_BASE: u32 = 0x436a0;

pub const E_EMPTY: u16 = 1;
pub const E_CARRIER: u16 = 2;
pub const E_HEADER: u16 = 3;
pub const E_SIZE: u16 = 4;
pub const E_STATE: u16 = 5;
pub const E_SHORT: u16 = 6;
pub const E_OVER: u16 = 7;
pub const E_FOOTER: u16 = 8;
pub const E_CHECKSUM: u16 = 9;
pub const E_RESIDUAL: u16 = 10;
pub const E_CAP: u16 = 11;
pub const E_HEX: u16 = 12;

/// 单会话载荷上限（1 MiB）。打印链路单字体超过 1 MiB 就该走分片资源
/// 而不是单次下载；这里显式拒绝，避免无界缓冲。
pub const MAX_PAYLOAD: usize = 1 << 20;

const FNV_SEED: u32 = 2166136261;

/// 下载通道错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DlError {
    Empty,
    Carrier,
    Header,
    Size,
    State,
    Short,
    Over,
    Footer,
    Checksum,
    Residual,
    Cap,
    Hex,
}

impl DlError {
    fn ordinal(self) -> u16 {
        match self {
            DlError::Empty => E_EMPTY,
            DlError::Carrier => E_CARRIER,
            DlError::Header => E_HEADER,
            DlError::Size => E_SIZE,
            DlError::State => E_STATE,
            DlError::Short => E_SHORT,
            DlError::Over => E_OVER,
            DlError::Footer => E_FOOTER,
            DlError::Checksum => E_CHECKSUM,
            DlError::Residual => E_RESIDUAL,
            DlError::Cap => E_CAP,
            DlError::Hex => E_HEX,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            DlError::Empty => "empty chunk",
            DlError::Carrier => "unknown download carrier",
            DlError::Header => "carrier header malformed",
            DlError::Size => "declared payload size illegal",
            DlError::State => "illegal channel state transition",
            DlError::Short => "session closed before payload complete",
            DlError::Over => "payload exceeds declared size",
            DlError::Footer => "carrier trailer malformed",
            DlError::Checksum => "payload checksum mismatch",
            DlError::Residual => "bytes left after trailer",
            DlError::Cap => "download ledger capacity exhausted",
            DlError::Hex => "non-hex byte in PS payload",
        }
    }
}

/// 载体两档。PCL 走二进制，PS 走 ASCII-hex（7 位安全通道）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carrier {
    Pcl,
    Ps,
}

impl Carrier {
    pub fn code(self) -> u8 {
        match self {
            Carrier::Pcl => 0,
            Carrier::Ps => 1,
        }
    }

    pub fn from_code(c: u8) -> Result<Carrier, DlError> {
        match c {
            0 => Ok(Carrier::Pcl),
            1 => Ok(Carrier::Ps),
            _ => Err(DlError::Carrier),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Carrier::Pcl => "pcl",
            Carrier::Ps => "ps",
        }
    }
}

/// 通道状态：Opened 是「已解析头、等待载荷」，Receiving 是「已有载荷」。
/// Sealed 之后任何写入都是 State 错误——下载事务不可续写。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DlState {
    Idle,
    Opened,
    Receiving,
    Sealed,
    Aborted,
}

/// 低层字节会计通道：只管「声明 / 实收 / 分块 / 校验和」，不解析载体。
/// 解析器 `DlSession` 把解出的载荷字节喂进这里，两者可独立测试。
#[derive(Clone, Debug)]
pub struct DlChannel {
    pub session_id: u16,
    pub carrier: Carrier,
    pub state: DlState,
    pub declared: usize,
    pub received: usize,
    pub chunks: usize,
    checksum: u32,
}

impl DlChannel {
    /// 开通道：会话号 0 或长度非法都显式拒绝。
    pub fn open(session_id: u16, carrier: Carrier, declared: usize) -> Result<DlChannel, DlError> {
        if session_id == 0 {
            return Err(DlError::State);
        }
        if declared == 0 || declared > MAX_PAYLOAD {
            return Err(DlError::Size);
        }
        Ok(DlChannel {
            session_id,
            carrier,
            state: DlState::Opened,
            declared,
            received: 0,
            chunks: 0,
            checksum: FNV_SEED,
        })
    }

    /// 喂一块载荷。超出声明长度立即 `Over`，绝不「先收下再说」。
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), DlError> {
        if chunk.is_empty() {
            return Err(DlError::Empty);
        }
        match self.state {
            DlState::Opened | DlState::Receiving => {}
            _ => return Err(DlError::State),
        }
        if chunk.len() > self.declared - self.received {
            return Err(DlError::Over);
        }
        self.checksum = fnv32(chunk, self.checksum);
        self.received += chunk.len();
        self.chunks += 1;
        self.state = DlState::Receiving;
        Ok(())
    }

    /// 封口：长度与校验和双门都过才允许 Sealed。
    pub fn seal(&mut self, announced: u32) -> Result<u32, DlError> {
        match self.state {
            DlState::Opened | DlState::Receiving => {}
            _ => return Err(DlError::State),
        }
        if self.received != self.declared {
            return Err(DlError::Short);
        }
        if self.checksum != announced {
            return Err(DlError::Checksum);
        }
        self.state = DlState::Sealed;
        Ok(self.checksum)
    }

    pub fn abort(&mut self) {
        if self.state != DlState::Sealed {
            self.state = DlState::Aborted;
        }
    }

    pub fn checksum(&self) -> u32 {
        self.checksum
    }
}

/// 32 位 FNV-1a。逐字节累积，才能对「块切分方式」不敏感：同样的载荷
/// 无论被拆成 1 字节还是 4 KiB 一块，终值都必须一致。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(16777619);
        i += 1;
    }
    h
}

fn hex_val(b: u8) -> Result<u8, DlError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(DlError::Hex),
    }
}

fn hex_upper(v: u8) -> u8 {
    if v < 10 {
        b'0' + v
    } else {
        b'A' + (v - 10)
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let limit = hay.len() - needle.len();
    let mut i = 0usize;
    while i <= limit {
        let mut j = 0usize;
        let mut ok = true;
        while j < needle.len() {
            if hay[i + j] != needle[j] {
                ok = false;
                break;
            }
            j += 1;
        }
        if ok {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn parse_hex8(b: &[u8]) -> Result<u32, DlError> {
    if b.len() < 8 {
        return Err(DlError::Footer);
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < 8 {
        v = (v << 4) | hex_val(b[i])? as u32;
        i += 1;
    }
    Ok(v)
}

/// 解析阶段。Done 之后 `feed` 只返回 State，保证「一帧一事务」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Header,
    Payload,
    Trailer,
    Done,
}

/// 增量下载会话：可把任意切分的字节块依次喂入，内部维护头/载荷/尾三段。
#[derive(Clone, Debug)]
pub struct DlSession {
    session_id: u16,
    phase: Phase,
    channel: Option<DlChannel>,
    inbuf: Vec<u8>,
    payload: Vec<u8>,
    nibble: Option<u8>,
    residual: usize,
    done: bool,
}

impl DlSession {
    pub fn new(session_id: u16) -> Result<DlSession, DlError> {
        if session_id == 0 {
            return Err(DlError::State);
        }
        Ok(DlSession {
            session_id,
            phase: Phase::Header,
            channel: None,
            inbuf: Vec::new(),
            payload: Vec::new(),
            nibble: None,
            residual: 0,
            done: false,
        })
    }

    /// 喂一块原始字节（可以只是半截头）。空块显式拒绝。
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), DlError> {
        if chunk.is_empty() {
            return Err(DlError::Empty);
        }
        if self.done {
            return Err(DlError::State);
        }
        self.inbuf.extend_from_slice(chunk);
        self.advance()
    }

    /// 结束会话：只有解析到 Done 才算完整；否则 Short。
    pub fn finish(&mut self) -> Result<(), DlError> {
        if self.done {
            Ok(())
        } else {
            Err(DlError::Short)
        }
    }

    fn advance(&mut self) -> Result<(), DlError> {
        let mut guard = 0usize;
        loop {
            guard += 1;
            if guard > 64 {
                return Err(DlError::Header);
            }
            match self.phase {
                Phase::Header => {
                    if !self.try_header()? {
                        return Ok(());
                    }
                }
                Phase::Payload => {
                    self.pump_payload()?;
                    // 载荷未满就说明这一轮已把现有字节吃干，等下一块。
                    // 不在这里退出会让外层循环空转到 guard 上限。
                    if self.phase == Phase::Payload {
                        return Ok(());
                    }
                }
                Phase::Trailer => {
                    if !self.try_trailer()? {
                        return Ok(());
                    }
                }
                Phase::Done => return Ok(()),
            }
        }
    }

    fn try_header(&mut self) -> Result<bool, DlError> {
        if self.inbuf.len() < 3 {
            return Ok(false);
        }
        if self.inbuf[0] == 0x1B {
            if self.inbuf[1] != b'(' || self.inbuf[2] != b's' {
                return Err(DlError::Carrier);
            }
            let mut w = 3usize;
            let mut found = false;
            while w < self.inbuf.len() && w < 16 {
                let b = self.inbuf[w];
                if b == b'W' {
                    found = true;
                    break;
                }
                if !b.is_ascii_digit() {
                    return Err(DlError::Header);
                }
                w += 1;
            }
            if !found {
                if self.inbuf.len() >= 16 {
                    return Err(DlError::Header);
                }
                return Ok(false);
            }
            let mut decl = 0usize;
            let mut k = 3usize;
            while k < w {
                decl = decl * 10 + (self.inbuf[k] - b'0') as usize;
                k += 1;
            }
            if decl == 0 || decl > MAX_PAYLOAD {
                return Err(DlError::Size);
            }
            let ch = DlChannel::open(self.session_id, Carrier::Pcl, decl)?;
            self.inbuf.drain(..=w);
            self.channel = Some(ch);
            self.phase = Phase::Payload;
            Ok(true)
        } else if self.inbuf[0] == b'%' || self.inbuf[0] == b'/' {
            let start_ok = self.inbuf.starts_with(b"%!PS") || self.inbuf.starts_with(b"/F");
            if !start_ok {
                return Err(DlError::Carrier);
            }
            let ex = match find(&self.inbuf, b"eexec\n") {
                Some(v) => v,
                None => {
                    if self.inbuf.len() > 512 {
                        return Err(DlError::Header);
                    }
                    return Ok(false);
                }
            };
            let hdr_end = ex + 6;
            let head = match self.inbuf.get(..hdr_end) {
                Some(h) => h,
                None => return Ok(false),
            };
            let fs_at = match find(head, b"/FontSize ") {
                Some(v) => v,
                None => return Err(DlError::Header),
            };
            let mut i = fs_at + 10;
            while i < head.len() && head[i] == b' ' {
                i += 1;
            }
            let mut decl = 0usize;
            let mut digits = 0usize;
            while i < head.len() && head[i].is_ascii_digit() {
                decl = decl * 10 + (head[i] - b'0') as usize;
                i += 1;
                digits += 1;
            }
            if digits == 0 {
                return Err(DlError::Header);
            }
            if decl == 0 || decl > MAX_PAYLOAD {
                return Err(DlError::Size);
            }
            let ch = DlChannel::open(self.session_id, Carrier::Ps, decl)?;
            self.inbuf.drain(..hdr_end);
            self.channel = Some(ch);
            self.phase = Phase::Payload;
            Ok(true)
        } else {
            Err(DlError::Carrier)
        }
    }

    fn pump_payload(&mut self) -> Result<(), DlError> {
        let mut complete = false;
        if let Some(ch) = self.channel.as_mut() {
            let remaining = ch.declared - ch.received;
            match ch.carrier {
                Carrier::Pcl => {
                    let take = if self.inbuf.len() < remaining {
                        self.inbuf.len()
                    } else {
                        remaining
                    };
                    if take > 0 {
                        let chunk: Vec<u8> = self.inbuf.drain(..take).collect();
                        ch.feed(&chunk)?;
                        self.payload.extend_from_slice(&chunk);
                    }
                }
                Carrier::Ps => {
                    let mut decoded: Vec<u8> = Vec::new();
                    let mut i = 0usize;
                    let mut consumed = 0usize;
                    while i < self.inbuf.len() && decoded.len() < remaining {
                        let b = self.inbuf[i];
                        if b == b' ' || b == b'\n' || b == b'\r' || b == b'\t' {
                            i += 1;
                            consumed = i;
                            continue;
                        }
                        let v = hex_val(b)?;
                        match self.nibble {
                            None => {
                                self.nibble = Some(v);
                                i += 1;
                                consumed = i;
                            }
                            Some(hi) => {
                                decoded.push((hi << 4) | v);
                                self.nibble = None;
                                i += 1;
                                consumed = i;
                            }
                        }
                    }
                    if !decoded.is_empty() {
                        ch.feed(&decoded)?;
                        self.payload.extend_from_slice(&decoded);
                    }
                    if consumed > 0 {
                        self.inbuf.drain(..consumed);
                    }
                }
            }
            complete = ch.received == ch.declared;
        }
        if complete {
            self.phase = Phase::Trailer;
        }
        Ok(())
    }

    fn try_trailer(&mut self) -> Result<bool, DlError> {
        let carrier = match self.channel.as_ref() {
            Some(c) => c.carrier,
            None => return Err(DlError::State),
        };
        match carrier {
            Carrier::Pcl => {
                if self.inbuf.len() < 14 {
                    if self.inbuf.len() > 2048 {
                        return Err(DlError::Footer);
                    }
                    return Ok(false);
                }
                let marker = [0x1B, b'(', b's', b'1', b'Q'];
                let mut i = 0usize;
                while i < 5 {
                    if self.inbuf[i] != marker[i] {
                        return Err(DlError::Footer);
                    }
                    i += 1;
                }
                let cs = parse_hex8(&self.inbuf[5..13])?;
                if self.inbuf[13] != b'\n' {
                    return Err(DlError::Footer);
                }
                let ch = match self.channel.as_mut() {
                    Some(c) => c,
                    None => return Err(DlError::State),
                };
                ch.seal(cs)?;
                let residual = self.inbuf.len() - 14;
                if residual > 0 {
                    return Err(DlError::Residual);
                }
                self.inbuf.clear();
                self.residual = 0;
                self.done = true;
                self.phase = Phase::Done;
                Ok(true)
            }
            Carrier::Ps => {
                let cs_at = match find(&self.inbuf, b"%%Checksum: ") {
                    Some(v) => v,
                    None => {
                        if self.inbuf.len() > 4096 {
                            return Err(DlError::Footer);
                        }
                        return Ok(false);
                    }
                };
                let hex_at = cs_at + 12;
                if self.inbuf.len() < hex_at + 8 {
                    return Ok(false);
                }
                let cs = parse_hex8(&self.inbuf[hex_at..hex_at + 8])?;
                let mut nl = hex_at + 8;
                if self.inbuf.get(nl) == Some(&b'\n') {
                    nl += 1;
                }
                let clear = match find(&self.inbuf, b"cleartomark") {
                    Some(v) => v,
                    None => {
                        if self.inbuf.len() > 8192 {
                            return Err(DlError::Footer);
                        }
                        return Ok(false);
                    }
                };
                if clear < nl {
                    return Err(DlError::Footer);
                }
                let pad = match self.inbuf.get(nl..clear) {
                    Some(p) => p,
                    None => return Err(DlError::Footer),
                };
                let mut zeros = 0usize;
                let mut k = 0usize;
                while k < pad.len() {
                    let b = pad[k];
                    if b == b'0' {
                        zeros += 1;
                    } else if b == b'\n' || b == b'\r' || b == b' ' || b == b'\t' {
                        // 排版空白不计入零填充。
                    } else {
                        return Err(DlError::Footer);
                    }
                    k += 1;
                }
                // Type1 规范要求 eexec 之后至少 512 个零字节；不足即视为
                // 载体被截断，而不是「差不多就行」。
                if zeros < 512 {
                    return Err(DlError::Footer);
                }
                let ch = match self.channel.as_mut() {
                    Some(c) => c,
                    None => return Err(DlError::State),
                };
                ch.seal(cs)?;
                let end = clear + 11;
                if end > self.inbuf.len() {
                    return Ok(false);
                }
                let residual = self.inbuf.len() - end;
                if residual > 0 {
                    return Err(DlError::Residual);
                }
                self.inbuf.clear();
                self.residual = 0;
                self.done = true;
                self.phase = Phase::Done;
                Ok(true)
            }
        }
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn residual(&self) -> usize {
        self.residual
    }

    pub fn buffered_len(&self) -> usize {
        self.inbuf.len()
    }

    pub fn done(&self) -> bool {
        self.done
    }

    pub fn state(&self) -> DlState {
        match &self.channel {
            Some(c) => c.state,
            None => DlState::Idle,
        }
    }

    pub fn declared(&self) -> usize {
        match &self.channel {
            Some(c) => c.declared,
            None => 0,
        }
    }

    pub fn received(&self) -> usize {
        match &self.channel {
            Some(c) => c.received,
            None => 0,
        }
    }

    pub fn chunks(&self) -> usize {
        match &self.channel {
            Some(c) => c.chunks,
            None => 0,
        }
    }

    pub fn checksum(&self) -> u32 {
        match &self.channel {
            Some(c) => c.checksum,
            None => FNV_SEED,
        }
    }

    /// 会话收据：只有 Done 才能开票，半截会话不允许进账。
    pub fn to_record(&self) -> Result<DlRecord, DlError> {
        if !self.done {
            return Err(DlError::Short);
        }
        let ch = match &self.channel {
            Some(c) => c,
            None => return Err(DlError::State),
        };
        Ok(DlRecord {
            session_id: self.session_id,
            carrier: ch.carrier,
            declared: ch.declared,
            received: ch.received,
            chunks: ch.chunks,
            checksum: ch.checksum,
            residual: self.residual,
        })
    }
}

/// 会话收据。`received == declared` 与 `residual == 0` 是零残留的两条硬判据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DlRecord {
    pub session_id: u16,
    pub carrier: Carrier,
    pub declared: usize,
    pub received: usize,
    pub chunks: usize,
    pub checksum: u32,
    pub residual: usize,
}

impl DlRecord {
    /// 零残留：声明=实收、无尾后字节、有分块且校验和非零。
    pub fn zero_residual(&self) -> bool {
        self.declared == self.received && self.residual == 0 && self.chunks > 0
    }
}

/// 下载账本：定容 append-only。
#[derive(Clone, Debug)]
pub struct DlLedger {
    recs: Vec<DlRecord>,
    cap: usize,
}

impl DlLedger {
    pub fn new(cap: usize) -> DlLedger {
        DlLedger {
            recs: Vec::new(),
            cap,
        }
    }

    pub fn push(&mut self, r: DlRecord) -> Result<(), DlError> {
        if self.recs.len() >= self.cap {
            return Err(DlError::Cap);
        }
        self.recs.push(r);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn record(&self, i: usize) -> Option<&DlRecord> {
        self.recs.get(i)
    }

    /// 全账残留合计：J1 的「零残留」在这里做总账口径的复核。
    pub fn total_residual(&self) -> usize {
        let mut t = 0usize;
        let mut i = 0usize;
        while i < self.recs.len() {
            t += self.recs[i].residual;
            i += 1;
        }
        t
    }
}

// ---------------------------------------------------------------------------
// 载体生成器（自检与注入核共用；真实链路里由调用方拼装）
// ---------------------------------------------------------------------------

/// 生成一帧合法 PCL 下载（头 + 原生载荷 + 尾校验）。
pub fn build_pcl(payload: &[u8]) -> Vec<u8> {
    let cs = fnv32(payload, FNV_SEED);
    let mut v = Vec::with_capacity(payload.len() + 24);
    v.extend_from_slice(&[0x1B, b'(', b's']);
    let digits = format!("{}", payload.len());
    v.extend_from_slice(digits.as_bytes());
    v.push(b'W');
    v.extend_from_slice(payload);
    v.extend_from_slice(&[0x1B, b'(', b's', b'1', b'Q']);
    v.extend_from_slice(format!("{:08X}", cs).as_bytes());
    v.push(b'\n');
    v
}

/// 生成一帧合法 PS 下载（DSC 头 + ASCII-hex + 零填充 + cleartomark）。
pub fn build_ps(session_id: u16, payload: &[u8]) -> Vec<u8> {
    let cs = fnv32(payload, FNV_SEED);
    let mut s = String::new();
    s.push_str("%!PS-AdobeFont-1.0\n");
    s.push_str(&format!("/F{} /FontType 1 def\n", session_id));
    s.push_str(&format!("/FontSize {} def\n", payload.len()));
    s.push_str("currentfile eexec\n");
    let mut i = 0usize;
    while i < payload.len() {
        let b = payload[i];
        s.push(hex_upper(b >> 4) as char);
        s.push(hex_upper(b & 0x0F) as char);
        if (i + 1) % 64 == 0 {
            s.push('\n');
        }
        i += 1;
    }
    s.push_str("\n%%Checksum: ");
    s.push_str(&format!("{:08X}", cs));
    s.push('\n');
    let mut z = 0usize;
    while z < 512 {
        s.push('0');
        z += 1;
    }
    s.push_str("\ncleartomark");
    s.into_bytes()
}

fn sample_payload(len: usize, tag: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        v.push((i as u32 * 31 + tag as u32 + 7) as u8);
        i += 1;
    }
    v
}

/// 一站式下载：整帧一次喂入并落账。分块路径由 `DlSession::feed` 覆盖。
pub fn download_once(
    session_id: u16,
    carrier: Carrier,
    frame: &[u8],
    ledger: &mut DlLedger,
) -> Result<DlRecord, DlError> {
    let mut sess = DlSession::new(session_id)?;
    sess.feed(frame)?;
    sess.finish()?;
    let rec = sess.to_record()?;
    if rec.carrier != carrier {
        return Err(DlError::Carrier);
    }
    ledger.push(rec)?;
    Ok(rec)
}

// ---------------------------------------------------------------------------
// 注入核：10 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;
    let payload = sample_payload(64, 3);

    if DlSession::new(1).and_then(|mut s| s.feed(&[])).err() == Some(DlError::Empty) {
        caught += 1;
    }
    if DlSession::new(1).and_then(|mut s| s.feed(b"XYZ")).err() == Some(DlError::Carrier) {
        caught += 1;
    }
    if DlSession::new(1)
        .and_then(|mut s| s.feed(b"\x1b(sABCDEFGHIJKLMNOP"))
        .err()
        == Some(DlError::Header)
    {
        caught += 1;
    }
    if DlChannel::open(1, Carrier::Pcl, 0).err() == Some(DlError::Size) {
        caught += 1;
    }
    // 提前结束：只喂头就 finish
    let short = DlSession::new(2).and_then(|mut s| {
        let mut hdr = Vec::new();
        hdr.extend_from_slice(&[0x1B, b'(', b's']);
        hdr.extend_from_slice(b"64");
        hdr.push(b'W');
        let _ = s.feed(&hdr);
        s.finish()
    });
    if short.err() == Some(DlError::Short) {
        caught += 1;
    }
    // 超量：通道直接多喂一字节
    let over = DlChannel::open(3, Carrier::Pcl, 4).and_then(|mut c| {
        let buf = [1u8, 2, 3, 4, 5];
        c.feed(&buf)
    });
    if over.err() == Some(DlError::Over) {
        caught += 1;
    }
    // 坏尾校验：合法帧改尾部标记
    let mut bad_footer = build_pcl(&payload);
    let flen = bad_footer.len();
    if flen > 14 {
        bad_footer[flen - 14] = b'X';
    }
    if DlSession::new(4)
        .and_then(|mut s| s.feed(&bad_footer))
        .err()
        == Some(DlError::Footer)
    {
        caught += 1;
    }
    // 校验和错：PS 帧改 8 位校验和中的一位
    let mut bad_cs = build_ps(5, &payload);
    let cs_at = match find(&bad_cs, b"%%Checksum: ") {
        Some(v) => v + 12,
        None => 0,
    };
    if cs_at + 8 < bad_cs.len() {
        bad_cs[cs_at] = if bad_cs[cs_at] == b'0' { b'1' } else { b'0' };
    }
    if DlSession::new(5)
        .and_then(|mut s| s.feed(&bad_cs))
        .err()
        == Some(DlError::Checksum)
    {
        caught += 1;
    }
    // 尾后残留：合法帧后再加一字节
    let mut extra = build_pcl(&payload);
    extra.push(0x7F);
    if DlSession::new(6)
        .and_then(|mut s| s.feed(&extra))
        .err()
        == Some(DlError::Residual)
    {
        caught += 1;
    }
    // 十六进制坏字符
    let mut bad_hex = build_ps(7, &payload);
    let ex = match find(&bad_hex, b"eexec\n") {
        Some(v) => v + 6,
        None => 0,
    };
    if ex + 2 < bad_hex.len() {
        bad_hex[ex] = b'Z';
    }
    if DlSession::new(7)
        .and_then(|mut s| s.feed(&bad_hex))
        .err()
        == Some(DlError::Hex)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50084_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50084");
    let mut ledger = DlLedger::new(16);

    // J1-1 十会话分块下载：5 PCL + 5 PS，零残留
    let mut ok = true;
    let mut pcl_ok = 0usize;
    let mut ps_ok = 0usize;
    let mut total_payload = 0usize;
    let mut session = 1u16;
    while session <= 10 {
        let carrier = if session % 2 == 1 { Carrier::Pcl } else { Carrier::Ps };
        let payload = sample_payload(40 + session as usize * 3, session as u8);
        let frame = match carrier {
            Carrier::Pcl => build_pcl(&payload),
            Carrier::Ps => build_ps(session, &payload),
        };
        let mut sess = match DlSession::new(session) {
            Ok(v) => v,
            Err(_) => {
                ok = false;
                session += 1;
                continue;
            }
        };
        // 用奇数块长切分，专门打「头/半字节跨块」的边界
        let widths = [1usize, 3, 7, 2, 11, 5];
        let mut at = 0usize;
        let mut wi = 0usize;
        while at < frame.len() {
            let w = widths[wi % widths.len()];
            let end = if at + w > frame.len() { frame.len() } else { at + w };
            if sess.feed(&frame[at..end]).is_err() {
                ok = false;
                break;
            }
            at = end;
            wi += 1;
        }
        if ok {
            if sess.finish().is_err() {
                ok = false;
            }
        }
        if sess.payload() != payload.as_slice() {
            ok = false;
        }
        if sess.residual() != 0 || sess.buffered_len() != 0 || sess.received() != sess.declared() {
            ok = false;
        }
        if sess.state() != DlState::Sealed {
            ok = false;
        }
        let rec = match sess.to_record() {
            Ok(r) => r,
            Err(_) => {
                ok = false;
                session += 1;
                continue;
            }
        };
        if !rec.zero_residual() || rec.checksum != fnv32(&payload, FNV_SEED) {
            ok = false;
        }
        if rec.chunks <= 1 {
            ok = false;
        }
        if ledger.push(rec).is_err() {
            ok = false;
        }
        if carrier == Carrier::Pcl {
            pcl_ok += 1;
        } else {
            ps_ok += 1;
        }
        total_payload += payload.len();
        session += 1;
    }
    let ok = ok
        && pcl_ok == 5
        && ps_ok == 5
        && ledger.len() == 10
        && ledger.total_residual() == 0
        && total_payload > 0;
    let det = format!(
        "ok={} pcl={} ps={} rec={} resid={}",
        ok,
        pcl_ok,
        ps_ok,
        ledger.len(),
        ledger.total_residual()
    );
    s.add_dyn("50084-J1-十会话零残留", ok, &det);

    // J1-2 载荷逐字节往返（十会话在账面上的长度与校验和自洽）
    let mut bytes_ok = ledger.len() == 10;
    let mut i = 0usize;
    while i < ledger.len() {
        match ledger.record(i) {
            Some(r) => {
                if r.received != r.declared || r.checksum == FNV_SEED {
                    bytes_ok = false;
                }
            }
            None => bytes_ok = false,
        }
        i += 1;
    }
    s.add("50084-J1-载荷对账", bytes_ok, if bytes_ok { "" } else { "长度或校验和异常" });

    // J1-3 两档载体头判别
    let payload = sample_payload(48, 9);
    let pcl = build_pcl(&payload);
    let ps = build_ps(11, &payload);
    let a = DlSession::new(11).and_then(|mut x| x.feed(&pcl));
    let b = DlSession::new(12).and_then(|mut x| x.feed(&ps));
    let ok = a.is_ok() && b.is_ok();
    s.add("50084-J1-双档载体识别", ok, if ok { "" } else { "载体头解析失败" });

    // J1-4 块切分不影响校验和（1 字节 vs 整帧）
    let one = {
        let mut sess = match DlSession::new(20) {
            Ok(v) => v,
            Err(_) => {
                s.add("50084-J2-切分无关", false, "开会话失败");
                return s;
            }
        };
        let mut k = 0usize;
        while k < pcl.len() {
            if sess.feed(&pcl[k..k + 1]).is_err() {
                break;
            }
            k += 1;
        }
        sess.to_record()
    };
    let whole = {
        let mut sess = match DlSession::new(21) {
            Ok(v) => v,
            Err(_) => {
                s.add("50084-J2-切分无关", false, "开会话失败");
                return s;
            }
        };
        let _ = sess.feed(&pcl);
        sess.to_record()
    };
    let ok = match (one, whole) {
        (Ok(x), Ok(y)) => x.checksum == y.checksum && x.received == y.received,
        _ => false,
    };
    s.add("50084-J2-切分无关", ok, if ok { "" } else { "切分改变校验和" });

    // J1-5 状态机：封口后不可再写；重复封口拒绝
    let mut ch = match DlChannel::open(30, Carrier::Pcl, 4) {
        Ok(v) => v,
        Err(_) => {
            s.add("50084-J2-通道状态机", false, "开通道失败");
            return s;
        }
    };
    let half = ch.feed(&[1, 2]);
    let cs = fnv32(&[1, 2, 3, 4], FNV_SEED);
    let short = ch.seal(cs).err() == Some(DlError::Short);
    let over = ch.feed(&[9, 9, 9]).err() == Some(DlError::Over);
    let full = ch.feed(&[3, 4]).is_ok();
    let sealed = ch.seal(cs).is_ok();
    let again = ch.seal(cs).err() == Some(DlError::State);
    let late = ch.feed(&[5]).err() == Some(DlError::State);
    let ok = half.is_ok() && short && over && full && sealed && again && late;
    s.add("50084-J2-通道状态机", ok, if ok { "" } else { "通道状态机漏拦" });

    // J1-6 提前 finish / 完成后 feed
    let mut sess = match DlSession::new(31) {
        Ok(v) => v,
        Err(_) => {
            s.add("50084-J2-会话终止", false, "开会话失败");
            return s;
        }
    };
    let early = sess.finish().err() == Some(DlError::Short);
    let feed = sess.feed(&pcl).is_ok();
    let fin = sess.finish().is_ok();
    let late = sess.feed(&pcl).err() == Some(DlError::State);
    let ok = early && feed && fin && late;
    s.add("50084-J2-会话终止", ok, if ok { "" } else { "终止语义错误" });

    // J1-7 坏校验和必须拒绝（PCL / PS 各一）
    let mut bad_pcl = build_pcl(&payload);
    if bad_pcl.len() > 3 {
        let at = bad_pcl.len() - 9;
        bad_pcl[at] = if bad_pcl[at] == b'0' { b'1' } else { b'0' };
    }
    let pcl_err = DlSession::new(40)
        .and_then(|mut x| x.feed(&bad_pcl))
        .err();
    let mut bad_ps = build_ps(41, &payload);
    let cs_at = match find(&bad_ps, b"%%Checksum: ") {
        Some(v) => v + 12,
        None => 0,
    };
    if cs_at + 8 < bad_ps.len() {
        bad_ps[cs_at] = if bad_ps[cs_at] == b'0' { b'1' } else { b'0' };
    }
    let ps_err = DlSession::new(41).and_then(|mut x| x.feed(&bad_ps)).err();
    let ok = pcl_err == Some(DlError::Checksum) && ps_err == Some(DlError::Checksum);
    s.add("50084-J2-校验和拒绝", ok, if ok { "" } else { "坏校验和未拒" });

    // J1-8 尾后残留拒绝（PCL / PS 各一）
    let mut extra = build_pcl(&payload);
    extra.push(0x00);
    let pcl_err = DlSession::new(50).and_then(|mut x| x.feed(&extra)).err();
    let mut ps_extra = build_ps(51, &payload);
    ps_extra.push(b'\n');
    let ps_err = DlSession::new(51).and_then(|mut x| x.feed(&ps_extra)).err();
    let ok = pcl_err == Some(DlError::Residual) && ps_err == Some(DlError::Residual);
    s.add("50084-J2-残留拒绝", ok, if ok { "" } else { "尾后字节未拒" });

    // J1-9 尾结构损坏拒绝（PCL 尾标记 + PS 零填充不足）
    let mut bad_tail = build_pcl(&payload);
    let t = bad_tail.len();
    if t > 14 {
        bad_tail[t - 14] = b'X';
    }
    let tail_err = DlSession::new(60).and_then(|mut x| x.feed(&bad_tail)).err();
    let mut few_zeros = build_ps(61, &payload);
    if let Some(z) = find(&few_zeros, b"cleartomark") {
        let start = if z > 100 { z - 100 } else { 0 };
        let mut k = start;
        while k < z {
            few_zeros[k] = b' '; // 抹掉零填充
            k += 1;
        }
    }
    let zero_err = DlSession::new(61).and_then(|mut x| x.feed(&few_zeros)).err();
    let ok = tail_err == Some(DlError::Footer) && zero_err == Some(DlError::Footer);
    s.add("50084-J2-尾结构拒绝", ok, if ok { "" } else { "坏尾未拒" });

    // J1-10 账本容量
    let mut tiny = DlLedger::new(1);
    let rec = DlRecord {
        session_id: 1,
        carrier: Carrier::Pcl,
        declared: 4,
        received: 4,
        chunks: 1,
        checksum: 7,
        residual: 0,
    };
    let first = tiny.push(rec).is_ok();
    let second = tiny.push(rec).err() == Some(DlError::Cap);
    let ok = first && second;
    s.add("50084-J2-账本容量", ok, if ok { "" } else { "容量未拦" });

    // J1-11 一站式下载确定性
    let mut l1 = DlLedger::new(4);
    let mut l2 = DlLedger::new(4);
    let r1 = download_once(70, Carrier::Ps, &ps, &mut l1);
    let r2 = download_once(70, Carrier::Ps, &ps, &mut l2);
    let ok = match (r1, r2) {
        (Ok(x), Ok(y)) => x == y && x.zero_residual(),
        _ => false,
    };
    s.add("50084-J2-下载确定", ok, if ok { "" } else { "两次结果不同" });

    // J1-12 载体档位码往返
    let mut ok = Carrier::from_code(0).ok() == Some(Carrier::Pcl)
        && Carrier::from_code(1).ok() == Some(Carrier::Ps)
        && Carrier::from_code(2).is_err();
    if Carrier::Pcl.code() != 0 || Carrier::Ps.code() != 1 {
        ok = false;
    }
    s.add("50084-J2-档位往返", ok, if ok { "" } else { "档位码不闭合" });

    // J1-13 注入核 10 类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50084-J1-注入必抓", ok, &det);

    // J1-14 码值闭集
    let codes = [
        DlError::Empty,
        DlError::Carrier,
        DlError::Header,
        DlError::Size,
        DlError::State,
        DlError::Short,
        DlError::Over,
        DlError::Footer,
        DlError::Checksum,
        DlError::Residual,
        DlError::Cap,
        DlError::Hex,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < codes.len() {
        let c = codes[i].code();
        if c <= SEG_BASE || c > SEG_BASE + 12 {
            ok = false;
        }
        i += 1;
    }
    s.add("50084-J1-码值闭集", ok, if ok { "" } else { "码值越段" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50084_checks_green() {
        let set = run_unx50084_checks();
        assert!(set.all_passed(), "UNX-F50084 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50084_zero_residual_ten() {
        let mut ledger = DlLedger::new(16);
        let mut i = 1u16;
        while i <= 10 {
            let payload = sample_payload(32 + i as usize, i as u8);
            let frame = if i % 2 == 1 {
                build_pcl(&payload)
            } else {
                build_ps(i, &payload)
            };
            let r = download_once(i, if i % 2 == 1 { Carrier::Pcl } else { Carrier::Ps }, &frame, &mut ledger);
            assert!(r.is_ok());
            if let Ok(rec) = r {
                assert!(rec.zero_residual());
            }
            i += 1;
        }
        assert_eq!(ledger.len(), 10);
        assert_eq!(ledger.total_residual(), 0);
    }

    #[test]
    fn unx50084_injection() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
    }
}
