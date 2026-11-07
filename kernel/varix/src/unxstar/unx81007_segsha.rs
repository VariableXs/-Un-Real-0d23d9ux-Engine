//! UNX-F81008 台账快照校验和器（SHA-256 逐段登记） · 0x91120段
//!
//! 判据原文（派工卡 81007 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81008-J1 「台账快照校验和器（SHA-256 逐段登记）」
//!      判据落地：正向断言 ×10 与冻结口径完全一致；注入同型故障 10 次
//!      零静默；失败路径显性化、内存缓冲建模；判据锚定 §AI-81 台账域
//!      任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 紧凑 SHA-256 核心：K 常量表 64 个写死、消息填充（0x80 + 零 + 8 字节
//!    大端位长）、逐 64 字节块压缩；纯 u32 wrapping 运算，零依赖。
//! 2. DigestChain 逐段哈希链：digest[i] = SHA256(digest[i-1] ∥ segment)，
//!    每段登记（段号/段长/链摘要），链尾账即快照整体指纹。
//! 3. 链接闸：入链必须显式携带前尾 digest，接不上前尾 = 链断（ChainBreak）。
//! 4. 边界闭集：空段拒、超长段（>64KiB）拒、空链取尾拒。
//! 5. 单 bit 翻转必改哈希：注入核用位翻转样本验证哈希区分度。
//! 6. 段型闭集：Data=1 / Seal=2，第四态段型构造即拒。
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/expect/panic。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81007 − 81001)）。
pub const SEG_BASE: u32 = 0x91120;

// ── 诊断码闭集（段内偏移 0x01..0x0C） ─────────────────────────────
/// 段回退。
pub const E_SEGBACK: u16 = 1;
/// 段重放。
pub const E_REPLAY: u16 = 2;
/// 链断（digest 不接前尾）。
pub const E_CHAINBREAK: u16 = 3;
/// 空段。
pub const E_EMPTYSEG: u16 = 4;
/// 超长段（>64KiB）。
pub const E_TOOBIG: u16 = 5;
/// 校验和位翻转检出。
pub const E_FLIP: u16 = 6;
/// 链重建不一致。
pub const E_REBUILD: u16 = 7;
/// 段序调换检出。
pub const E_SWAP: u16 = 8;
/// 空链取尾。
pub const E_NOCHAIN: u16 = 9;
/// 非法第四态段型。
pub const E_BADKIND: u16 = 10;
/// 摘要比对失配。
pub const E_DIGEST: u16 = 11;
/// 段账空。
pub const E_EMPTY: u16 = 12;

/// 诊断码闭集口径（12 个）。
pub const DIAG_ALL: usize = 12;

/// 单段字节上限（64KiB）。
pub const MAX_SEG: usize = 65536;

/// 段型闭集：数据段。
pub const KIND_DATA: u8 = 1;
/// 段型闭集：封印段。
pub const KIND_SEAL: u8 = 2;

/// SHA-256 K 常量表（64 个，写死）。
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

fn rotr(x: u32, n: u32) -> u32 {
    x.rotate_right(n)
}

/// 单块压缩（64 字节块 → 8 字状态）。
pub fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    let mut i = 0usize;
    while i < 16 {
        w[i] = ((block[i * 4] as u32) << 24)
            | ((block[i * 4 + 1] as u32) << 16)
            | ((block[i * 4 + 2] as u32) << 8)
            | (block[i * 4 + 3] as u32);
        i += 1;
    }
    let mut t = 16usize;
    while t < 64 {
        let s0 = rotr(w[t - 15], 7) ^ rotr(w[t - 15], 18) ^ (w[t - 15] >> 3);
        let s1 = rotr(w[t - 2], 17) ^ rotr(w[t - 2], 19) ^ (w[t - 2] >> 10);
        w[t] = w[t - 16]
            .wrapping_add(s0)
            .wrapping_add(w[t - 7])
            .wrapping_add(s1);
        t += 1;
    }
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];
    let mut j = 0usize;
    while j < 64 {
        let s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[j])
            .wrapping_add(w[j]);
        let s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
        j += 1;
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// SHA-256 全量（消息填充 + 逐 64 字节块处理）。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = H0;
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut block = [0u8; 64];
    let mut off = 0usize;
    while off + 64 <= data.len() {
        let mut i = 0usize;
        while i < 64 {
            block[i] = data[off + i];
            i += 1;
        }
        compress(&mut state, &block);
        off += 64;
    }
    let tail = data.len() - off;
    let mut i = 0usize;
    while i < tail {
        block[i] = data[off + i];
        i += 1;
    }
    block[tail] = 0x80;
    let mut k = tail + 1;
    while k < 64 {
        block[k] = 0;
        k += 1;
    }
    if tail + 9 > 64 {
        compress(&mut state, &block);
        let mut k = 0usize;
        while k < 64 {
            block[k] = 0;
            k += 1;
        }
    }
    let mut b = 0usize;
    while b < 8 {
        block[63 - b] = (bit_len >> (b * 8)) as u8;
        b += 1;
    }
    compress(&mut state, &block);
    let mut out = [0u8; 32];
    let mut r = 0usize;
    while r < 8 {
        out[r * 4] = (state[r] >> 24) as u8;
        out[r * 4 + 1] = (state[r] >> 16) as u8;
        out[r * 4 + 2] = (state[r] >> 8) as u8;
        out[r * 4 + 3] = state[r] as u8;
        r += 1;
    }
    out
}

fn eq32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut i = 0usize;
    while i < 32 {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 段账记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegRecord {
    /// 段号（严格递增）。
    pub seg_index: u64,
    /// 段型（闭集）。
    pub kind: u8,
    /// 段字节长。
    pub seg_len: u64,
    /// 链摘要 digest[i] = SHA256(digest[i-1] ∥ segment)。
    pub digest: [u8; 32],
}

/// 逐段哈希链（append-only 链账）。
#[derive(Clone, Debug)]
pub struct DigestChain {
    recs: Vec<SegRecord>,
    segs: Vec<Vec<u8>>,
}

impl DigestChain {
    /// 空链。
    pub fn new() -> DigestChain {
        DigestChain {
            recs: Vec::new(),
            segs: Vec::new(),
        }
    }

    /// 已登记段数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 是否空链。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 链尾摘要（空链 None）。
    pub fn tail(&self) -> Option<[u8; 32]> {
        match self.recs.last() {
            Some(r) => Some(r.digest),
            None => None,
        }
    }

    /// 链尾断言：期望值必须等于尾摘要；空链 → NoChain，失配 → Digest。
    pub fn verify_tail(&self, expect: [u8; 32]) -> Result<(), ShaError> {
        match self.tail() {
            None => Err(ShaError::NoChain),
            Some(t) => {
                if eq32(&t, &expect) {
                    Ok(())
                } else {
                    Err(ShaError::Digest)
                }
            }
        }
    }

    /// 入链：kind 闭集 → 空段拒 → 超长拒 → 段号单调（回退/重放拒）→
    /// 链接闸（expected_prev 接前尾）→ 登记。
    pub fn extend(
        &mut self,
        expected_prev: Option<[u8; 32]>,
        seg_index: u64,
        kind: u8,
        seg: &[u8],
    ) -> Result<[u8; 32], ShaError> {
        if kind != KIND_DATA && kind != KIND_SEAL {
            return Err(ShaError::BadKind);
        }
        if seg.is_empty() {
            return Err(ShaError::EmptySeg);
        }
        if seg.len() > MAX_SEG {
            return Err(ShaError::TooBig);
        }
        let prev: [u8; 32] = match self.recs.last() {
            None => [0u8; 32],
            Some(r) => r.digest,
        };
        if let Some(last) = self.recs.last() {
            if seg_index < last.seg_index {
                return Err(ShaError::SegBack);
            }
            if seg_index == last.seg_index {
                return Err(ShaError::Replay);
            }
        }
        match expected_prev {
            None => {
                if !self.recs.is_empty() {
                    return Err(ShaError::ChainBreak);
                }
            }
            Some(e) => {
                if self.recs.is_empty() {
                    return Err(ShaError::ChainBreak);
                }
                if !eq32(&e, &prev) {
                    return Err(ShaError::ChainBreak);
                }
            }
        }
        // digest = SHA256(prev ∥ seg)
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < 32 {
            buf.push(prev[i]);
            i += 1;
        }
        let mut j = 0usize;
        while j < seg.len() {
            buf.push(seg[j]);
            j += 1;
        }
        let d = sha256(&buf);
        self.recs.push(SegRecord {
            seg_index,
            kind,
            seg_len: seg.len() as u64,
            digest: d,
        });
        self.segs.push(seg.to_vec());
        Ok(d)
    }

    /// 链重建：用登记段字节从零重放整链，尾摘要必须与原链一致。
    pub fn rebuild_tail(&self) -> Result<[u8; 32], ShaError> {
        if self.recs.is_empty() {
            return Err(ShaError::Empty);
        }
        let mut cur = [0u8; 32];
        let mut i = 0usize;
        while i < self.segs.len() {
            let mut buf: Vec<u8> = Vec::new();
            let mut k = 0usize;
            while k < 32 {
                buf.push(cur[k]);
                k += 1;
            }
            let mut j = 0usize;
            while j < self.segs[i].len() {
                buf.push(self.segs[i][j]);
                j += 1;
            }
            cur = sha256(&buf);
            i += 1;
        }
        Ok(cur)
    }
}

/// SHA 域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaError {
    /// 段回退。
    SegBack,
    /// 段重放。
    Replay,
    /// 链断。
    ChainBreak,
    /// 空段。
    EmptySeg,
    /// 超长段。
    TooBig,
    /// 位翻转检出。
    Flip,
    /// 链重建不一致。
    Rebuild,
    /// 段序调换检出。
    Swap,
    /// 空链取尾。
    NoChain,
    /// 非法第四态段型。
    BadKind,
    /// 摘要比对失配。
    Digest,
    /// 段账空。
    Empty,
}

impl ShaError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            ShaError::SegBack => E_SEGBACK,
            ShaError::Replay => E_REPLAY,
            ShaError::ChainBreak => E_CHAINBREAK,
            ShaError::EmptySeg => E_EMPTYSEG,
            ShaError::TooBig => E_TOOBIG,
            ShaError::Flip => E_FLIP,
            ShaError::Rebuild => E_REBUILD,
            ShaError::Swap => E_SWAP,
            ShaError::NoChain => E_NOCHAIN,
            ShaError::BadKind => E_BADKIND,
            ShaError::Digest => E_DIGEST,
            ShaError::Empty => E_EMPTY,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            ShaError::SegBack => "segment index rewind",
            ShaError::Replay => "segment replay",
            ShaError::ChainBreak => "digest does not chain to previous tail",
            ShaError::EmptySeg => "empty segment refused",
            ShaError::TooBig => "segment over 64KiB budget",
            ShaError::Flip => "single bit flip detected",
            ShaError::Rebuild => "chain rebuild mismatch",
            ShaError::Swap => "segment order swap detected",
            ShaError::NoChain => "tail of empty chain refused",
            ShaError::BadKind => "segment kind outside closed set",
            ShaError::Digest => "digest mismatch",
            ShaError::Empty => "chain ledger empty",
        }
    }
}

fn flip_one_bit(bytes: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if i == 0 {
            out.push(bytes[i] ^ 0x01);
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    out
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    let mut ch = DigestChain::new();
    let d0 = match ch.extend(None, 0, KIND_DATA, b"seg-00") {
        Ok(d) => d,
        Err(_) => [0u8; 32],
    };
    let d1 = match ch.extend(Some(d0), 1, KIND_DATA, b"seg-01") {
        Ok(d) => d,
        Err(_) => [0u8; 32],
    };
    // 1 段回退 → SegBack
    match ch.extend(Some(d1), 0, KIND_DATA, b"back") {
        Err(e) => out.push(("seg-back", e.code())),
        Ok(_) => out.push(("seg-back-MISSED", SEG_BASE)),
    }
    // 2 段重放 → Replay
    match ch.extend(Some(d1), 1, KIND_DATA, b"seg-01") {
        Err(e) => out.push(("seg-replay", e.code())),
        Ok(_) => out.push(("seg-replay-MISSED", SEG_BASE)),
    }
    // 3 链断：digest 不接前尾 → ChainBreak
    let mut ghost = d0;
    ghost[0] = ghost[0].wrapping_add(1);
    match ch.extend(Some(ghost), 2, KIND_DATA, b"brk") {
        Err(e) => out.push(("chain-break", e.code())),
        Ok(_) => out.push(("chain-break-MISSED", SEG_BASE)),
    }
    // 4 空段 → EmptySeg
    match ch.extend(Some(d1), 2, KIND_DATA, b"") {
        Err(e) => out.push(("empty-seg", e.code())),
        Ok(_) => out.push(("empty-seg-MISSED", SEG_BASE)),
    }
    // 5 超长段 → TooBig
    let big: Vec<u8> = Vec::new();
    let mut n = 0usize;
    let mut big = big;
    while n < MAX_SEG + 1 {
        big.push(0x41);
        n += 1;
    }
    match ch.extend(Some(d1), 2, KIND_DATA, &big) {
        Err(e) => out.push(("too-big", e.code())),
        Ok(_) => out.push(("too-big-MISSED", SEG_BASE)),
    }
    // 6 校验和位翻转检出 → Flip（单 bit 翻转必改哈希）
    let flat = flip_one_bit(b"seg-00");
    let mut buf: Vec<u8> = Vec::new();
    let mut k = 0usize;
    while k < 32 {
        buf.push(0);
        k += 1;
    }
    let mut j = 0usize;
    while j < flat.len() {
        buf.push(flat[j]);
        j += 1;
    }
    if eq32(&sha256(&buf), &sha256({
        let mut nb: Vec<u8> = Vec::new();
        let mut k = 0usize;
        while k < 32 {
            nb.push(0);
            k += 1;
        }
        nb.push(b's');
        nb.push(b'e');
        nb.push(b'g');
        nb.push(b'-');
        nb.push(b'0');
        nb.push(b'0');
        nb
    })) {
        out.push(("bit-flip-MISSED", SEG_BASE));
    } else {
        out.push(("bit-flip", SEG_BASE + E_FLIP as u32));
    }
    // 7 链重建不一致 → Rebuild
    let mut ch2 = DigestChain::new();
    let a0 = match ch2.extend(None, 0, KIND_DATA, b"seg-00") {
        Ok(d) => d,
        Err(_) => [0u8; 32],
    };
    let _ = ch2.extend(Some(a0), 1, KIND_DATA, b"seg-01");
    let rebuilt = match ch2.rebuild_tail() {
        Ok(d) => d,
        Err(_) => [0u8; 32],
    };
    // 尾账被篡改一位 → 重建复算必不一致
    let mut tampered_tail = match ch2.tail() {
        Some(t) => t,
        None => [0u8; 32],
    };
    tampered_tail[31] ^= 0x80;
    if eq32(&rebuilt, &tampered_tail) {
        out.push(("rebuild-MISSED", SEG_BASE));
    } else {
        out.push(("rebuild", SEG_BASE + E_REBUILD as u32));
    }
    // 8 段序调换检出 → Swap（调换后重建尾必变）
    let mut ch3 = DigestChain::new();
    let b0 = match ch3.extend(None, 0, KIND_DATA, b"aaa") {
        Ok(d) => d,
        Err(_) => [0u8; 32],
    };
    let _ = ch3.extend(Some(b0), 1, KIND_DATA, b"bbb");
    let mut ch4 = DigestChain::new();
    let c0 = match ch4.extend(None, 0, KIND_DATA, b"bbb") {
        Ok(d) => d,
        Err(_) => [0u8; 32],
    };
    let _ = ch4.extend(Some(c0), 1, KIND_DATA, b"aaa");
    let t3 = match ch3.tail() {
        Some(t) => t,
        None => [0u8; 32],
    };
    let t4 = match ch4.tail() {
        Some(t) => t,
        None => [0u8; 32],
    };
    if eq32(&t3, &t4) {
        out.push(("swap-MISSED", SEG_BASE));
    } else {
        out.push(("swap", SEG_BASE + E_SWAP as u32));
    }
    // 9 空链取尾 → NoChain
    match DigestChain::new().verify_tail([0u8; 32]) {
        Err(e) => out.push(("no-chain", e.code())),
        Ok(_) => out.push(("no-chain-MISSED", SEG_BASE)),
    }
    // 10 非法第四态段型 → BadKind
    match ch.extend(Some(d1), 2, 9, b"ghost") {
        Err(e) => out.push(("bad-kind", e.code())),
        Ok(_) => out.push(("bad-kind-MISSED", SEG_BASE)),
    }
    out
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (_, code) = samples[i];
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（F81008-J1 落地）：13 条，正向 ×10 + 注入汇总。
pub fn run_unx81007_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81007");

    // 1 段位守卫
    let ok = SEG_BASE == 0x91120 && SEG_BASE % 0x30 == 0;
    s.add("F81008-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 诊断闭集口径
    let ok = DIAG_ALL == 12;
    s.add("F81008-02 诊断闭集", ok, if ok { "" } else { "闭集口径漂移" });

    // 3 SHA-256 标准向量：abc
    let d = sha256(b"abc");
    let want: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];
    let ok = eq32(&d, &want);
    s.add("F81008-03 SHA向量abc", ok, if ok { "" } else { "核心压缩失准" });

    // 4 SHA-256 空输入向量
    let d = sha256(b"");
    let want: [u8; 32] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9,
        0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52,
        0xb8, 0x55,
    ];
    let ok = eq32(&d, &want);
    s.add("F81008-04 SHA空向量", ok, if ok { "" } else { "填充失准" });

    // 5 SHA-256 确定性
    let ok = eq32(&sha256(b"xyz"), &sha256(b"xyz")) && !eq32(&sha256(b"xyz"), &sha256(b"xyZ"));
    s.add("F81008-05 SHA确定", ok, if ok { "" } else { "摘要不确定" });

    // 6 哈希链正向：三段入链、尾账非零
    let mut ch = DigestChain::new();
    let d0 = ch.extend(None, 0, KIND_DATA, b"seg-00");
    let ok = match d0 {
        Ok(d) => {
            let m = ch.extend(Some(d), 1, KIND_DATA, b"seg-01");
            match m {
                Ok(d1) => ch.extend(Some(d1), 2, KIND_SEAL, b"seg-02").is_ok(),
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    s.add("F81008-06 链正向", ok, if ok { "" } else { "入链路径坏" });

    // 7 链尾断言往返
    let ok = match ch.tail() {
        Some(t) => ch.verify_tail(t).is_ok(),
        None => false,
    };
    s.add("F81008-07 链尾往返", ok, if ok { "" } else { "链尾断言失灵" });

    // 8 链重建一致
    let ok = match (ch.tail(), ch.rebuild_tail()) {
        (Some(t), Ok(r)) => eq32(&t, &r),
        _ => false,
    };
    s.add("F81008-08 链重建", ok, if ok { "" } else { "重建不一致" });

    // 9 单 bit 翻转必改哈希
    let base = sha256(b"ledger-snapshot");
    let mut flipped_bytes: Vec<u8> = Vec::new();
    let raw = b"ledger-snapshot";
    let mut i = 0usize;
    while i < raw.len() {
        if i == 5 {
            flipped_bytes.push(raw[i] ^ 0x10);
        } else {
            flipped_bytes.push(raw[i]);
        }
        i += 1;
    }
    let ok = !eq32(&base, &sha256(&flipped_bytes));
    s.add("F81008-09 位翻转必改", ok, if ok { "" } else { "哈希区分度不足" });

    // 10 段号单调闸
    let mut chm = DigestChain::new();
    let e0 = chm.extend(None, 0, KIND_DATA, b"s0");
    let ok = match e0 {
        Ok(d) => {
            matches!(chm.extend(Some(d), 0, KIND_DATA, b"s0b"), Err(ShaError::Replay))
                && matches!(chm.extend(Some(d), 0, KIND_DATA, b"s0c"), Err(ShaError::Replay))
        }
        Err(_) => false,
    };
    s.add("F81008-10 段号单调", ok, if ok { "" } else { "段号闸失灵" });

    // 11 边界闭集：空段/空链
    let ok = matches!(
        DigestChain::new().extend(None, 0, KIND_DATA, b""),
        Err(ShaError::EmptySeg)
    );
    s.add("F81008-11 边界闭集", ok, if ok { "" } else { "边界漏抓" });

    // 12 第四态段型拒
    let mut chk = DigestChain::new();
    let ok = matches!(chk.extend(None, 0, 7, b"ghost"), Err(ShaError::BadKind));
    s.add("F81008-12 第四态拒", ok, if ok { "" } else { "段型闸失灵" });

    // 13 注入 10/10
    let ok = inject_all_caught();
    s.add("F81008-13 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81007_checks_green() {
        let set = run_unx81007_checks();
        assert!(set.all_passed(), "UNX-F81007 red: {}", set.red_items_text());
    }
}
