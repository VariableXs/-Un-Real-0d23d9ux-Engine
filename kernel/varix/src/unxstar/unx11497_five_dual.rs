//! UNX-F11497 五工具判据双机同码验证（0x20ee0 段）
//!
//! 同一份判据要在两台块设备不同的机器上跑出同一结论。帧格式是 [标签][长度]
//! [载荷]，尾部补零到机器块长的整数倍：A 机块长 8、标签 0xA1，B 机块长 16、
//! 标签 0xB2。原始帧因此必然不同——补零长度不同、标签不同——但把载荷抽出来做
//! 规范化折叠后必须逐位相等，这正是“同码”的含义。
//!
//! 解析口先验块长（4..=64 且为 2 的幂）、再验帧长（不短于头、能被块长整除）、
//! 再看标签、再看声明长度是否越过帧尾、最后验补零区是否全零。标签合法但不属于
//! 本机是 ESRC，载荷内容不同是 EBODY；两者都不许在规范化层被吞掉。
//!
//! 账本十四格：八格成功覆盖两机帧长、编码、解码、规范摘要与同码比较；六格拒绝
//! 依次是块长非法、帧长非法、标签非法、载荷越界、源机不符与载荷不符。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ee0;

/// 编号非法。
pub const FD_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const FD_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const FD_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const FD_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const FD_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const FD_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const FD_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const FD_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const FD_ESEAL: u32 = SEG_BASE + 0x09;
/// 块长非法。
pub const FD_ECHUNK: u32 = SEG_BASE + 0x0a;
/// 帧长非法。
pub const FD_EFRAME: u32 = SEG_BASE + 0x0b;
/// 标签非法。
pub const FD_ETAG: u32 = SEG_BASE + 0x0c;
/// 长度或载荷越界。
pub const FD_EOVER: u32 = SEG_BASE + 0x0d;
/// 源机不符。
pub const FD_ESRC: u32 = SEG_BASE + 0x0e;
/// 载荷不符。
pub const FD_EBODY: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 机器标签 A。
pub const TAG_A: u8 = 0xa1;
/// 机器标签 B。
pub const TAG_B: u8 = 0xb2;
/// 帧头长度。
pub const HEADER: usize = 2;
/// 载荷上限。
pub const PAY_CAP: usize = 64;
/// 块长下限。
pub const CHUNK_MIN: usize = 4;
/// 块长上限。
pub const CHUNK_MAX: usize = 64;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// 操作：帧长计算。
pub const OP_LEN: u8 = 1;
/// 操作：编码。
pub const OP_ENCODE: u8 = 2;
/// 操作：解码。
pub const OP_DECODE: u8 = 3;
/// 操作：规范化摘要。
pub const OP_CANON: u8 = 4;
/// 操作：同码比较。
pub const OP_SAME: u8 = 5;
/// 操作：按机器解码。
pub const OP_DECODE_AS: u8 = 6;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一台机器的块参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Machine {
    pub chunk: usize,
    pub tag: u8,
}

impl Machine {
    /// 构造。
    pub const fn new(chunk: usize, tag: u8) -> Machine {
        Machine { chunk, tag }
    }
}

/// A 机：块长 8，标签 0xA1。
pub const MACHINE_A: Machine = Machine::new(8, TAG_A);
/// B 机：块长 16，标签 0xB2。
pub const MACHINE_B: Machine = Machine::new(16, TAG_B);

/// 块长是否合法。
pub fn valid_chunk(chunk: usize) -> bool {
    chunk >= CHUNK_MIN && chunk <= CHUNK_MAX && chunk.is_power_of_two()
}

/// 标签是否合法。
pub fn valid_tag(tag: u8) -> bool {
    tag == TAG_A || tag == TAG_B
}

/// 补齐后的帧长。
pub fn frame_len(payload: usize, chunk: usize) -> Result<usize, u32> {
    if !valid_chunk(chunk) {
        return Err(FD_ECHUNK);
    }
    if payload > 255 {
        return Err(FD_EOVER);
    }
    let total = match HEADER.checked_add(payload) {
        Some(v) => v,
        None => return Err(FD_EOVER),
    };
    let rem = total % chunk;
    if rem == 0 {
        return Ok(total);
    }
    match total.checked_add(chunk - rem) {
        Some(v) => Ok(v),
        None => Err(FD_EOVER),
    }
}

/// 编码：标签、声明长度、载荷、补零。
pub fn encode(payload: &[u8], chunk: usize, tag: u8, out: &mut [u8]) -> Result<usize, u32> {
    if !valid_chunk(chunk) {
        return Err(FD_ECHUNK);
    }
    if !valid_tag(tag) {
        return Err(FD_ETAG);
    }
    if payload.len() > PAY_CAP {
        return Err(FD_EOVER);
    }
    let flen = frame_len(payload.len(), chunk)?;
    if flen > out.len() {
        return Err(FD_EOVER);
    }
    let mut i = 0usize;
    while i < flen {
        match out.get_mut(i) {
            Some(d) => *d = 0,
            None => return Err(FD_EOVER),
        }
        i += 1;
    }
    match out.get_mut(0) {
        Some(d) => *d = tag,
        None => return Err(FD_EOVER),
    }
    match out.get_mut(1) {
        Some(d) => *d = payload.len() as u8,
        None => return Err(FD_EOVER),
    }
    let mut j = 0usize;
    while j < payload.len() {
        let b = match payload.get(j) {
            Some(&v) => v,
            None => return Err(FD_EOVER),
        };
        match out.get_mut(HEADER + j) {
            Some(d) => *d = b,
            None => return Err(FD_EOVER),
        }
        j += 1;
    }
    Ok(flen)
}

/// 解码：返回载荷长度，补零区必须为零。
pub fn decode(frame: &[u8], chunk: usize) -> Result<usize, u32> {
    if !valid_chunk(chunk) {
        return Err(FD_ECHUNK);
    }
    if frame.len() < HEADER || frame.len() % chunk != 0 {
        return Err(FD_EFRAME);
    }
    let tag = match frame.get(0) {
        Some(&v) => v,
        None => return Err(FD_EFRAME),
    };
    if !valid_tag(tag) {
        return Err(FD_ETAG);
    }
    let plen = match frame.get(1) {
        Some(&v) => v as usize,
        None => return Err(FD_EFRAME),
    };
    let end = match HEADER.checked_add(plen) {
        Some(v) => v,
        None => return Err(FD_EOVER),
    };
    if end > frame.len() {
        return Err(FD_EOVER);
    }
    let mut i = end;
    while i < frame.len() {
        match frame.get(i) {
            Some(&0) => {}
            Some(_) => return Err(FD_EFRAME),
            None => return Err(FD_EFRAME),
        }
        i += 1;
    }
    Ok(plen)
}

/// 按机器解码：标签必须属于本机。
pub fn decode_as(frame: &[u8], m: Machine) -> Result<usize, u32> {
    if !valid_chunk(m.chunk) {
        return Err(FD_ECHUNK);
    }
    if !valid_tag(m.tag) {
        return Err(FD_ETAG);
    }
    let tag = match frame.get(0) {
        Some(&v) => v,
        None => return Err(FD_EFRAME),
    };
    if tag != m.tag {
        return Err(FD_ESRC);
    }
    decode(frame, m.chunk)
}

/// 规范化摘要：只看载荷，不看标签与补零。
pub fn canonical(frame: &[u8], chunk: usize) -> Result<u64, u32> {
    let plen = decode(frame, chunk)?;
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < plen {
        let b = match frame.get(HEADER + i) {
            Some(&v) => v,
            None => return Err(FD_EOVER),
        };
        h = fold(h, b as u64);
        i += 1;
    }
    Ok(h)
}

/// 原始帧摘要：包含标签与补零。
pub fn raw_digest(frame: &[u8]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < frame.len() {
        if let Some(&b) = frame.get(i) {
            h = fold(h, b as u64);
        }
        i += 1;
    }
    h
}

/// 载荷同码比较。
pub fn same(a: &[u8], b: &[u8]) -> Result<(), u32> {
    if a == b {
        Ok(())
    } else {
        Err(FD_EBODY)
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    cap: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap: SAMPLES,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 编号 → 重复 → 顺序 → 容量。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FD_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(FD_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(FD_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(FD_EORDER);
        }
        if self.len >= self.cap {
            return Err(FD_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(FD_EID),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功计数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(FD_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(FD_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(FD_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(FD_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(FD_EMISMATCH);
                    }
                }
                None => return Err(FD_ENOBS),
            }
            i += 1;
        }
        let vals = [8u32, 16, 8, 6, 16, 6, 1, 0];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(FD_EMISMATCH);
                    }
                }
                _ => return Err(FD_EBAL),
            }
            k += 1;
        }
        let codes = [FD_ECHUNK, FD_EFRAME, FD_ETAG, FD_EOVER, FD_ESRC, FD_EBODY];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(FD_EMISMATCH);
                        }
                    }
                    None => return Err(FD_EBAL),
                },
                None => return Err(FD_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(FD_EBAL);
        }
        Ok(())
    }

    /// 熔断票号。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FD_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(FD_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(FD_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut fa = [0u8; 32];
    let mut fb = [0u8; 32];

    let _ = l.register(cell(0, OP_LEN, frame_len(6, 8).map(|n| n as u32)));
    let _ = l.register(cell(1, OP_LEN, frame_len(6, 16).map(|n| n as u32)));
    let _ = l.register(cell(2, OP_ENCODE, encode(b"abcdef", 8, TAG_A, &mut fa).map(|n| n as u32)));
    let _ = l.register(cell(3, OP_DECODE, decode(&fa, 8).map(|n| n as u32)));
    let _ = l.register(cell(4, OP_ENCODE, encode(b"abcdef", 16, TAG_B, &mut fb).map(|n| n as u32)));
    let _ = l.register(cell(5, OP_DECODE, decode(&fb, 16).map(|n| n as u32)));
    let ca = canonical(&fa, 8);
    let cb = canonical(&fb, 16);
    let eq = match (ca, cb) {
        (Ok(x), Ok(y)) => (x == y) as u32,
        _ => 0,
    };
    let _ = l.register(cell(6, OP_CANON, Ok(eq)));
    let _ = l.register(cell(7, OP_SAME, same(b"abcdef", b"abcdef").map(|_| 0)));

    let _ = l.register(cell(8, OP_LEN, frame_len(6, 0).map(|n| n as u32)));
    let odd = [TAG_A, 0u8, b'a', 0, 0, 0, 0];
    let _ = l.register(cell(9, OP_DECODE, decode(&odd, 8).map(|n| n as u32)));
    let bad = Machine::new(8, 0x55);
    let _ = l.register(cell(10, OP_ENCODE, encode(b"x", bad.chunk, bad.tag, &mut fa).map(|n| n as u32)));
    let big = [0u8; 65];
    let _ = l.register(cell(11, OP_ENCODE, encode(&big, 8, TAG_A, &mut fa).map(|n| n as u32)));
    let _ = l.register(cell(12, OP_DECODE_AS, decode_as(&fa, MACHINE_B).map(|n| n as u32)));
    let _ = l.register(cell(13, OP_SAME, same(b"abc", b"abd").map(|_| 0)));

    l
}

/// 四路注入：账本自洽、A 机往返、双机同码、拒绝后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(FD_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    let mut fa = [0u8; 32];
    let mut fb = [0u8; 32];
    let na = encode(b"abcdef", 8, TAG_A, &mut fa);
    let nb = encode(b"abcdef", 16, TAG_B, &mut fb);
    let body = fa.get(2) == Some(&b'a') && fa.get(7) == Some(&b'f');
    out[1] = na == Ok(8)
        && decode(&fa, 8) == Ok(6)
        && fa.get(0) == Some(&TAG_A)
        && fa.get(1) == Some(&6)
        && body;

    let ca = canonical(&fa, 8);
    let cb = canonical(&fb, 16);
    out[2] = nb == Ok(16)
        && decode(&fb, 16) == Ok(6)
        && ca.is_ok()
        && ca == cb
        && raw_digest(&fa) != raw_digest(&fb)
        && same(b"abcdef", b"abcdef").is_ok();

    let src = decode_as(&fa, MACHINE_B) == Err(FD_ESRC);
    let body = same(b"abc", b"abd") == Err(FD_EBODY);
    let chunk = frame_len(6, 0) == Err(FD_ECHUNK);
    let recovered = encode(b"xyz", 8, TAG_A, &mut fa) == Ok(8);
    out[3] = src && body && chunk && recovered;
    out
}

/// 十三判据。
pub fn run_unx11497_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11497.dual");
    let l = build_ledger();
    let mut fa = [0u8; 32];
    let mut fb = [0u8; 32];
    let _ = encode(b"abcdef", 8, TAG_A, &mut fa);
    let _ = encode(b"abcdef", 16, TAG_B, &mut fb);

    s.add(
        "FD1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_LEN),
        "static",
    );
    s.add_dyn(
        "FD2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "FD3-两种块长",
        frame_len(6, 8) == Ok(8)
            && frame_len(6, 16) == Ok(16)
            && frame_len(0, 8) == Ok(8)
            && frame_len(14, 16) == Ok(16),
        "static",
    );
    s.add(
        "FD4-A 机编码",
        encode(b"abcdef", 8, TAG_A, &mut fa) == Ok(8)
            && fa.get(0) == Some(&TAG_A)
            && fa.get(1) == Some(&6)
            && fa.get(2) == Some(&b'a')
            && fa.get(7) == Some(&b'f'),
        "static",
    );
    s.add(
        "FD5-A 机解码",
        decode(&fa, 8) == Ok(6)
            && l.sample(3).map(|x| x.value) == Some(6)
            && canonical(&fa, 8).is_ok(),
        "static",
    );
    s.add(
        "FD6-B 机往返",
        encode(b"abcdef", 16, TAG_B, &mut fb) == Ok(16)
            && decode(&fb, 16) == Ok(6)
            && fb.get(0) == Some(&TAG_B)
            && fb.get(15) == Some(&0),
        "static",
    );
    s.add(
        "FD7-规范化摘要相等",
        canonical(&fa, 8) == canonical(&fb, 16)
            && raw_digest(&fa) != raw_digest(&fb)
            && l.sample(6).map(|x| x.value) == Some(1),
        "static",
    );
    s.add(
        "FD8-同码比较",
        same(b"abcdef", b"abcdef") == Ok(())
            && same(b"abcdef", b"abcde") == Err(FD_EBODY)
            && l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );
    s.add(
        "FD9-块长拒绝",
        frame_len(6, 0) == Err(FD_ECHUNK)
            && frame_len(6, 3) == Err(FD_ECHUNK)
            && frame_len(6, 128) == Err(FD_ECHUNK)
            && decode(&fa, 0) == Err(FD_ECHUNK)
            && l.sample(8).map(|x| x.code) == Some(FD_ECHUNK),
        "static",
    );
    s.add(
        "FD10-帧长拒绝",
        decode(&[], 8) == Err(FD_EFRAME)
            && decode(&[TAG_A, 0u8, b'a', 0, 0, 0, 0], 8) == Err(FD_EFRAME)
            && decode(&[TAG_A, 0u8], 8) == Err(FD_EFRAME)
            && l.sample(9).map(|x| x.code) == Some(FD_EFRAME),
        "static",
    );
    s.add(
        "FD11-标签拒绝",
        decode(&[0x55u8, 0, 0, 0, 0, 0, 0, 0], 8) == Err(FD_ETAG)
            && encode(b"x", 8, 0x55, &mut fa) == Err(FD_ETAG)
            && l.sample(10).map(|x| x.code) == Some(FD_ETAG),
        "static",
    );
    s.add(
        "FD12-载荷越界",
        encode(&[0u8; 65], 8, TAG_A, &mut fa) == Err(FD_EOVER)
            && frame_len(300, 8) == Err(FD_EOVER)
            && l.sample(11).map(|x| x.code) == Some(FD_EOVER),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(FD_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "FD13-源机载荷与冻结重放",
        decode_as(&fa, MACHINE_B) == Err(FD_ESRC)
            && decode_as(&fa, MACHINE_A) == Ok(6)
            && same(b"abc", b"abd") == Err(FD_EBODY)
            && l.sample(12).map(|x| x.code) == Some(FD_ESRC)
            && l.sample(13).map(|x| x.code) == Some(FD_EBODY)
            && root != 0
            && rep == Ok(root)
            && again
            && f.root() == root
            && inj == [true; 4],
        &format!("root={:016x} inj={:?}", root, inj),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_lengths_round_up() {
        assert_eq!(frame_len(6, 8), Ok(8));
        assert_eq!(frame_len(6, 16), Ok(16));
        assert_eq!(frame_len(14, 8), Ok(16));
        assert_eq!(frame_len(6, 0), Err(FD_ECHUNK));
    }

    #[test]
    fn two_machines_roundtrip() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        assert_eq!(encode(b"abcdef", 8, TAG_A, &mut a), Ok(8));
        assert_eq!(encode(b"abcdef", 16, TAG_B, &mut b), Ok(16));
        assert_eq!(decode(&a, 8), Ok(6));
        assert_eq!(decode(&b, 16), Ok(6));
        assert_eq!(canonical(&a, 8), canonical(&b, 16));
        assert_ne!(raw_digest(&a), raw_digest(&b));
    }

    #[test]
    fn decode_checks_frame_shape() {
        assert_eq!(decode(&[], 8), Err(FD_EFRAME));
        assert_eq!(decode(&[TAG_A, 0, 0, 0, 0, 0, 0], 8), Err(FD_EFRAME));
        assert_eq!(decode(&[0x55, 0, 0, 0, 0, 0, 0, 0], 8), Err(FD_ETAG));
        assert_eq!(decode(&[TAG_A, 9, 0, 0, 0, 0, 0, 0], 8), Err(FD_EOVER));
    }

    #[test]
    fn source_machine_must_match() {
        let mut a = [0u8; 32];
        let _ = encode(b"abc", 8, TAG_A, &mut a);
        assert_eq!(decode_as(&a, MACHINE_A), Ok(3));
        assert_eq!(decode_as(&a, MACHINE_B), Err(FD_ESRC));
    }

    #[test]
    fn payload_limits() {
        let mut a = [0u8; 32];
        assert_eq!(encode(&[0u8; 65], 8, TAG_A, &mut a), Err(FD_EOVER));
        assert_eq!(encode(b"abc", 8, 0x55, &mut a), Err(FD_ETAG));
    }

    #[test]
    fn body_comparison() {
        assert_eq!(same(b"abc", b"abc"), Ok(()));
        assert_eq!(same(b"abc", b"abd"), Err(FD_EBODY));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11497_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
