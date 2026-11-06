//! UNX-F11443 文件类型探测（0x20b70 段）
//!
//! 类型探测读的是文件头而不是扩展名。模型把九组魔数按固定优先级排列，对每个样本
//! 同时做两件事：逐字节比较已知签名，以及判断样本是否是某个签名的真前缀。三相
//! 结果由此产生：完整命中（KIND 为具体类型）、前缀（KIND=NEED，need 表示还差几
//! 个字节才能下结论）、以及确定性未知（KIND=UNKNOWN）。短缓冲区不能武断判成
//! 二进制，这正是探测接口必须区分 NEED 与 UNKNOWN 的原因。
//!
//! 优先级影响重叠签名的选择。PNG 的八字节头、ELF 的四字节头、JPEG 的三字节头、
//! GIF/ZIP 的四字节头、gzip 的两字节头、PDF 的五字节头、shebang 的两字节头和
//! UTF-8 BOM 的三字节头互不重叠，但顺序仍被固定下来，避免以后加入宽容签名时
//! 结果漂移。`%PDF` 四字节是 PDF 的真前缀，因此报告 NEED=1 而不是 TEXT；`GIF9`
//! 在第四字节失配，报告 UNKNOWN 而不是 GIF。
//!
//! 文本回退只在所有签名都不匹配且不是任何签名前缀时发生：每个字节属于可打印
//! ASCII、制表、换行或回车才算 TEXT，出现一个控制字节或高位字节即判 BINARY。
//! 空缓冲区返回 ENODATA，声明长度超过容量返回 EOVERFLOW，两者都发生在比较之前，
//! 且不改变任何计数器。拒绝路径只增加 rejects。
//!
//! 审计逐格重算：用保存的样本字节再次调用探测器，结果必须与登记时一致；同时
//! 检查所有 NEED 的缺口在 1..=7 之间、BINARY 样本至少含一个非文本字节、TEXT 样本
//! 全部可打印，并核对命中/前缀/未知三组计数与逐格统计相等。十六格账本十二成一败，
//! 前缀格与失配格成对出现，专门防止“看到开头就算命中”的懒惰实现。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b70;
/// 编号非法。
pub const FT_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const FT_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const FT_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const FT_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const FT_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const FT_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const FT_EBAL: u32 = SEG_BASE + 0x07;
/// 无数据。
pub const FT_ENODATA: u32 = SEG_BASE + 0x08;
/// 超长。
pub const FT_EOVERFLOW: u32 = SEG_BASE + 0x09;
/// 票据不一致。
pub const FT_ESEAL: u32 = SEG_BASE + 0x0a;

/// ELF。
pub const K_ELF: u8 = 1;
/// PNG。
pub const K_PNG: u8 = 2;
/// JPEG。
pub const K_JPEG: u8 = 3;
/// GIF。
pub const K_GIF: u8 = 4;
/// ZIP。
pub const K_ZIP: u8 = 5;
/// gzip。
pub const K_GZIP: u8 = 6;
/// PDF。
pub const K_PDF: u8 = 7;
/// 脚本。
pub const K_SCRIPT: u8 = 8;
/// 文本。
pub const K_TEXT: u8 = 9;
/// 二进制。
pub const K_BIN: u8 = 10;
/// 需要更多字节。
pub const K_NEED: u8 = 11;
/// 确定未知。
pub const K_UNKNOWN: u8 = 12;

/// 样本容量。
pub const CAP: usize = 24;
/// 签名组数。
pub const SIGS: usize = 9;
/// 账本格数。
pub const SAMPLES: usize = 16;

/// 签名表。
pub const SIG_TAB: [[u8; 8]; SIGS] = [
    [0x7f, b'E', b'L', b'F', 0, 0, 0, 0],
    [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
    [0xff, 0xd8, 0xff, 0, 0, 0, 0, 0],
    [b'G', b'I', b'F', b'8', 0, 0, 0, 0],
    [b'P', b'K', 0x03, 0x04, 0, 0, 0, 0],
    [0x1f, 0x8b, 0, 0, 0, 0, 0, 0],
    [b'%', b'P', b'D', b'F', b'-', 0, 0, 0],
    [b'#', b'!', 0, 0, 0, 0, 0, 0],
    [0xef, 0xbb, 0xbf, 0, 0, 0, 0, 0],
];

/// 签名长度。
pub const SIG_LEN: [u8; SIGS] = [4, 8, 3, 4, 4, 2, 5, 2, 3];
/// 签名类型。
pub const SIG_KIND: [u8; SIGS] = [K_ELF, K_PNG, K_JPEG, K_GIF, K_ZIP, K_GZIP, K_PDF, K_SCRIPT, K_BIN];

/// 探测结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub kind: u8,
    pub need: u8,
}

/// 文本字节判定。
pub fn is_text_byte(b: u8) -> bool {
    b == b'\t' || b == b'\n' || b == b'\r' || (b >= 0x20 && b <= 0x7e)
}

/// 主探测器。
pub fn classify(buf: &[u8]) -> Result<Hit, u32> {
    if buf.is_empty() {
        return Err(FT_ENODATA);
    }
    if buf.len() > CAP {
        return Err(FT_EOVERFLOW);
    }
    let mut need = 0u8;
    let mut i = 0usize;
    while i < SIGS {
        let slen = match SIG_LEN.get(i) {
            Some(v) => *v as usize,
            None => 0,
        };
        let sig = match SIG_TAB.get(i) {
            Some(v) => v,
            None => return Err(FT_EBAL),
        };
        let n = if buf.len() < slen { buf.len() } else { slen };
        let mut same = true;
        let mut k = 0usize;
        while k < n {
            let a = match buf.get(k) {
                Some(v) => *v,
                None => 0,
            };
            let b = match sig.get(k) {
                Some(v) => *v,
                None => 0,
            };
            if a != b {
                same = false;
                break;
            }
            k += 1;
        }
        if same {
            if buf.len() >= slen {
                let kind = match SIG_KIND.get(i) {
                    Some(v) => *v,
                    None => return Err(FT_EBAL),
                };
                return Ok(Hit { kind, need: 0 });
            }
            let gap = (slen - buf.len()) as u8;
            if need == 0 || gap < need {
                need = gap;
            }
        }
        i += 1;
    }
    if need > 0 {
        return Ok(Hit { kind: K_NEED, need });
    }
    let mut text = true;
    let mut j = 0usize;
    while j < buf.len() {
        if let Some(b) = buf.get(j) {
            if !is_text_byte(*b) {
                text = false;
                break;
            }
        }
        j += 1;
    }
    if text {
        return Ok(Hit { kind: K_TEXT, need: 0 });
    }
    if buf.len() < 2 {
        return Ok(Hit { kind: K_UNKNOWN, need: 0 });
    }
    Ok(Hit { kind: K_BIN, need: 0 })
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub data: [u8; CAP],
    pub len: u8,
}

impl Sample {
    pub fn bytes(&self) -> &[u8] {
        let n = self.len as usize;
        if n > CAP {
            return &[];
        }
        match self.data.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 探测器统计。
pub struct Detector {
    samples: Vec<Sample>,
    hits: u32,
    needs: u32,
    unknown: u32,
    rejects: u32,
}

impl Detector {
    pub fn new() -> Detector {
        Detector { samples: Vec::new(), hits: 0, needs: 0, unknown: 0, rejects: 0 }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn sample(&self, i: usize) -> Option<Sample> {
        match self.samples.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }

    pub fn needs(&self) -> u32 {
        self.needs
    }

    pub fn unknown(&self) -> u32 {
        self.unknown
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 登记并探测一个样本。
    pub fn probe(&mut self, data: &[u8]) -> Result<Hit, u32> {
        if data.is_empty() {
            self.rejects += 1;
            return Err(FT_ENODATA);
        }
        if data.len() > CAP {
            self.rejects += 1;
            return Err(FT_EOVERFLOW);
        }
        let h = classify(data)?;
        let mut s = Sample { data: [0; CAP], len: data.len() as u8 };
        let mut i = 0usize;
        while i < data.len() {
            if let (Some(d), Some(v)) = (s.data.get_mut(i), data.get(i)) {
                *d = *v;
            }
            i += 1;
        }
        self.samples.push(s);
        if h.kind == K_NEED {
            self.needs += 1;
        } else if h.kind == K_UNKNOWN {
            self.unknown += 1;
        } else {
            self.hits += 1;
        }
        Ok(h)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.is_empty() || self.samples.len() > SAMPLES {
            return Err(FT_EBAL);
        }
        let mut hc = 0u32;
        let mut nc = 0u32;
        let mut uc = 0u32;
        let mut i = 0usize;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(FT_ENOBS),
            };
            let h = match classify(s.bytes()) {
                Ok(v) => v,
                Err(_) => return Err(FT_EBAL),
            };
            if h.kind == K_NEED {
                if h.need == 0 || h.need > 7 {
                    return Err(FT_EBAL);
                }
                nc += 1;
            } else if h.kind == K_UNKNOWN {
                uc += 1;
            } else {
                hc += 1;
                if h.kind == K_TEXT {
                    let mut k = 0usize;
                    while k < s.bytes().len() {
                        if let Some(b) = s.bytes().get(k) {
                            if !is_text_byte(*b) {
                                return Err(FT_EBAL);
                            }
                        }
                        k += 1;
                    }
                }
                if h.kind == K_BIN {
                    let mut bad = false;
                    let mut k = 0usize;
                    while k < s.bytes().len() {
                        if let Some(b) = s.bytes().get(k) {
                            if !is_text_byte(*b) {
                                bad = true;
                            }
                        }
                        k += 1;
                    }
                    if !bad {
                        return Err(FT_EBAL);
                    }
                }
            }
            i += 1;
        }
        if hc != self.hits || nc != self.needs || uc != self.unknown {
            return Err(FT_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                h = fold(h, s.len as u64);
                let mut k = 0usize;
                while k < s.len as usize {
                    if let Some(b) = s.data.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.hits as u64);
        h = fold(h, self.needs as u64);
        h = fold(h, self.unknown as u64);
        fold(h, self.rejects as u64)
    }
}

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

fn cut(a: &[u8], n: u8) -> &[u8] {
    let k = n as usize;
    if k > a.len() {
        return &[];
    }
    match a.get(..k) {
        Some(v) => v,
        None => &[],
    }
}

fn put(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub data: [u8; CAP],
    pub len: u8,
    pub hit: Hit,
    pub ok: bool,
}

impl Case {
    pub fn bytes(&self) -> &[u8] {
        cut(&self.data, self.len)
    }
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub hit: Hit,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    d: Detector,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { d: Detector::new(), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, data: &[u8], kind: u8, need: u8, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FT_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(FT_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(FT_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(FT_EORDER);
        }
        let r = self.d.probe(data);
        let end = match r {
            Ok(h) => {
                if !ok || h.kind != kind || h.need != need {
                    self.rejects += 1;
                    return Err(FT_EMISMATCH);
                }
                Ending { hit: h, code: 0 }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(FT_EMISMATCH);
                }
                Ending { hit: Hit { kind: 0, need: 0 }, code: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { data: [0; CAP], len: 0, hit: end.hit, ok };
        c.len = put(&mut c.data, data);
        self.cases.push(c);
        self.ends.push(end);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn case(&self, i: usize) -> Option<Case> {
        match self.cases.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn ending(&self, i: usize) -> Option<Ending> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn detector(&self) -> &Detector {
        &self.d
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.ends.len() {
            if let Some(e) = self.ends.get(i) {
                if e.code == 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.ends.len() - self.ok_count()
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(FT_EBAL);
        }
        self.d.audit()
    }

    pub fn seal(&self) -> u64 {
        self.d.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FT_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(FT_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十六格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, b"\x89PNG\r\n\x1a\n", K_PNG, 0, true);
    let _ = l.register(1, b"\x89PNG", K_NEED, 4, true);
    let _ = l.register(2, b"\x7fELF\x02\x01", K_ELF, 0, true);
    let _ = l.register(3, b"\x7fEL", K_NEED, 1, true);
    let _ = l.register(4, b"\xff\xd8", K_NEED, 1, true);
    let _ = l.register(5, b"\xff\xd8\xff\xe0", K_JPEG, 0, true);
    let _ = l.register(6, b"GIF89a", K_GIF, 0, true);
    let _ = l.register(7, b"\x00", K_UNKNOWN, 0, true);
    let _ = l.register(8, b"PK\x03\x04rest", K_ZIP, 0, true);
    let _ = l.register(9, b"\x1f\x8b\x08", K_GZIP, 0, true);
    let _ = l.register(10, b"%PDF", K_NEED, 1, true);
    let _ = l.register(11, b"#!/bin/sh", K_SCRIPT, 0, true);
    let _ = l.register(12, b"hello\tworld\r\n", K_TEXT, 0, true);
    let _ = l.register(13, b"\xff\x00\x01", K_BIN, 0, true);
    let _ = l.register(14, b"", 0, 0, false);
    let _ = l.register(15, b"0123456789abcdefghijklmno", 0, 0, false);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 14 && l.err_count() == 2 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    out[1] = classify(b"\x89PN").map(|h| (h.kind, h.need)) == Ok((K_NEED, 5))
        && classify(b"\x89PNX").map(|h| (h.kind, h.need)) == Ok((K_BIN, 0))
        && classify(b"GIF").map(|h| (h.kind, h.need)) == Ok((K_NEED, 1))
        && classify(b"GIF9").map(|h| h.kind) == Ok(K_TEXT)
        && classify(b"\x00").map(|h| h.kind) == Ok(K_UNKNOWN);

    let mut d = Detector::new();
    let a = d.probe(b"%PDF-1.7");
    let b = d.probe(b"%PDF");
    let c = d.probe(b"PK");
    out[2] = a == Ok(Hit { kind: K_PDF, need: 0 }) && b == Ok(Hit { kind: K_NEED, need: 1 })
        && c == Ok(Hit { kind: K_NEED, need: 2 }) && d.hits() == 1 && d.needs() == 2;

    let mut e = Detector::new();
    let n = e.probe(b"");
    let o = e.probe(b"0123456789abcdefghijklmnop");
    let h = e.probe(b"#!/bin/sh");
    out[3] = n == Err(FT_ENODATA) && o == Err(FT_EOVERFLOW) && h.map(|x| x.kind) == Ok(K_SCRIPT)
        && e.rejects() == 2 && e.hits() == 1 && e.len() == 1;
    out
}

/// 十三判据。
pub fn run_unx11443_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11443.filetype");
    let l = build_ledger();
    s.add("FT1-十六格齐备", l.len() == SAMPLES, "static");
    s.add_dyn("FT2-十四成二拒", l.ok_count() == 14 && l.err_count() == 2 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("FT3-PNG前后缀", l.ending(0).map(|e| e.hit) == Some(Hit { kind: K_PNG, need: 0 }) && l.ending(1).map(|e| e.hit) == Some(Hit { kind: K_NEED, need: 4 }), "static");
    s.add("FT4-ELF前后缀", l.ending(2).map(|e| e.hit.kind) == Some(K_ELF) && l.ending(3).map(|e| e.hit) == Some(Hit { kind: K_NEED, need: 1 }), "static");
    s.add("FT5-JPEG", l.ending(4).map(|e| e.hit) == Some(Hit { kind: K_NEED, need: 1 }) && l.ending(5).map(|e| e.hit.kind) == Some(K_JPEG), "static");
    s.add("FT6-GIF失配", l.ending(6).map(|e| e.hit.kind) == Some(K_GIF) && l.ending(7).map(|e| e.hit.kind) == Some(K_UNKNOWN), "static");
    s.add("FT7-ZIP与gzip", l.ending(8).map(|e| e.hit.kind) == Some(K_ZIP) && l.ending(9).map(|e| e.hit.kind) == Some(K_GZIP), "static");
    s.add("FT8-PDF真前缀", l.ending(10).map(|e| e.hit) == Some(Hit { kind: K_NEED, need: 1 }), "static");
    s.add("FT9-脚本与文本", l.ending(11).map(|e| e.hit.kind) == Some(K_SCRIPT) && l.ending(12).map(|e| e.hit.kind) == Some(K_TEXT), "static");
    s.add("FT10-二进制", l.ending(13).map(|e| e.hit.kind) == Some(K_BIN) && l.case(13).map(|c| (c.len, c.data.get(0).copied())) == Some((3, Some(0xff))), "static");
    s.add("FT11-空与超长", l.ending(14).map(|e| e.code) == Some((FT_ENODATA & 0xffff) as u16) && l.ending(15).map(|e| e.code) == Some((FT_EOVERFLOW & 0xffff) as u16), "static");
    s.add("FT12-计数守恒", l.detector().hits() == 9 && l.detector().needs() == 4 && l.detector().unknown() == 1 && l.detector().rejects() == 2 && l.detector().len() == 14 && l.audit().is_ok() && l.case(1).map(|c| classify(c.bytes())) == Some(Ok(Hit { kind: K_NEED, need: 4 })), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(FT_EFROZEN);
    let inj = inject_four();
    s.add_dyn("FT13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_prefix_and_full() {
        assert_eq!(classify(b"\x89PNG"), Ok(Hit { kind: K_NEED, need: 4 }));
        assert_eq!(classify(b"\x89PNG\r\n\x1a\n"), Ok(Hit { kind: K_PNG, need: 0 }));
    }

    #[test]
    fn elf_prefix() {
        assert_eq!(classify(b"\x7fE"), Ok(Hit { kind: K_NEED, need: 2 }));
        assert_eq!(classify(b"\x7fELF"), Ok(Hit { kind: K_ELF, need: 0 }));
    }

    #[test]
    fn jpeg_needs_third_byte() {
        assert_eq!(classify(b"\xff\xd8"), Ok(Hit { kind: K_NEED, need: 1 }));
        assert_eq!(classify(b"\xff\xd8\xff"), Ok(Hit { kind: K_JPEG, need: 0 }));
    }

    #[test]
    fn mismatch_is_unknown() {
        assert_eq!(classify(b"GIF9"), Ok(Hit { kind: K_TEXT, need: 0 }));
        assert_eq!(classify(b"\x00"), Ok(Hit { kind: K_UNKNOWN, need: 0 }));
        assert_eq!(classify(b"%PDX"), Ok(Hit { kind: K_TEXT, need: 0 }));
        assert_eq!(classify(b"PK\x03\x05"), Ok(Hit { kind: K_BIN, need: 0 }));
    }

    #[test]
    fn text_and_binary() {
        assert_eq!(classify(b"plain text\n"), Ok(Hit { kind: K_TEXT, need: 0 }));
        assert_eq!(classify(b"a\x00b"), Ok(Hit { kind: K_BIN, need: 0 }));
    }

    #[test]
    fn limits() {
        assert_eq!(classify(b""), Err(FT_ENODATA));
        assert_eq!(classify(b"0123456789abcdefghijklmno"), Err(FT_EOVERFLOW));
        assert_eq!(classify(b"0123456789abcdefghijklm"), Ok(Hit { kind: K_TEXT, need: 0 }));
    }

    #[test]
    fn ledger_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11443_checks().failed_count(), 0);
    }
}
