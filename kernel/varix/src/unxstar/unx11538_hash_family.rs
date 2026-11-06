//! UNX-F11538 b2sum/sha1sum 哈希族补位
//!
//! 哈希工具的输出只有一个定长摘要，因此「对」与「错」之间没有中间态：算错一位，
//! 后面全错。这单补的是 B17 哈希族的两条腿——SHA-1（160 位）与 BLAKE2b
//! （可变长 8..64 字节）——以及贴着它们长的校验行格式。
//!
//! 三件基准机才在意的事：
//!
//! 1. **流式与一次性必须同值**。真实工具按块读文件，内核这里也必须能按任意
//!    块边界喂数据。分块只改变 `update` 的次数，不改变 `finish` 的结论；本模块
//!    把 `buffered`、`fill`、`total` 全部显式记账，任何一处差一字节都会在
//!    分块等价判据里显形。
//! 2. **填充是算法的一部分**。SHA-1 补 `0x80` 后补零到 56 mod 64，再补 64 位
//!    大端比特长度；BLAKE2b 把最后一块留到 `finish` 才压，计数器加上块内实际
//!    字节数，末块翻转 `v[14]`。两者的边界长度（55/56/57、127/128/129）是
//!    最容易写错的地方，所以判据直接拿这些长度对齐基准向量。
//! 3. **校验行是格式，也是语义**。GNU 的 `<hex>  <file>`、二进制标记
//!    `<hex> *<file>`、BSD 的 `ALG (file) = <hex>` 三种写法都收；摘要长度反推
//!    算法（40→sha1、64→blake2b-256、128→blake2b-512），长度对不上就拒绝，
//!    而不是「大概算了一个」。
//!
//! 闸门次序：算法字面（`HS_EALG`）→ 输入长度（`HS_ELEN`）→ 字节合法性
//! （`HS_EBYTE`）→ 摘要长度（`HS_ERANGE`）→ 记录不足（`HS_ESHORT`）→ 重复
//! （`HS_EDUP`）→ 状态冲突（`HS_ESTATE`）→ 内部不一致（`HS_EINVAL`）。
//!
//! 账本语义：十四格 = 八格成功（sha1 首字节、b2b512 首字节、b2b256 首字节、
//! 样例行解析数、样例核验通过数、b2b512 十六进制长度、报表渲染行数、标签行数）
//! + 六格失败（未知算法、坏十六进制、短记录、长度越界、重复算法、二次收尾）。
//! `audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，
//! `freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf2b0;

/// 算法字面非法。
pub const HS_EALG: u32 = SEG_BASE + 0x01;
/// 输入或名称过长。
pub const HS_ELEN: u32 = SEG_BASE + 0x02;
/// 字节非法（非十六进制、不可打印）。
pub const HS_EBYTE: u32 = SEG_BASE + 0x03;
/// 摘要长度越界。
pub const HS_ERANGE: u32 = SEG_BASE + 0x04;
/// 重复登记。
pub const HS_EDUP: u32 = SEG_BASE + 0x05;
/// 记录不足。
pub const HS_ESHORT: u32 = SEG_BASE + 0x06;
/// 状态冲突（收尾后再喂/再收尾）。
pub const HS_ESTATE: u32 = SEG_BASE + 0x07;
/// 内部不一致。
pub const HS_EINVAL: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const HS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const HS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const HS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const HS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const HS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// SHA-1 摘要字节数。
pub const SHA1_LEN: usize = 20;

/// BLAKE2b 最大摘要字节数。
pub const B2B_MAX: usize = 64;

/// 名称容量。
pub const NAME_CAP: usize = 40;

/// 批容量。
pub const BATCH_CAP: usize = 16;

/// 单次喂入上限。
pub const MAX_INPUT: usize = 1_048_576;

/// 操作码：取摘要。
pub const OP_DIGEST: u8 = 0x01;
/// 操作码：解析。
pub const OP_PARSE: u8 = 0x02;
/// 操作码：核验。
pub const OP_VERIFY: u8 = 0x03;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 0x04;
/// 操作码：算法。
pub const OP_ALG: u8 = 0x05;
/// 操作码：登记。
pub const OP_REG: u8 = 0x06;
/// 操作码：状态。
pub const OP_STATE: u8 = 0x07;
/// 操作码：越界。
pub const OP_RANGE: u8 = 0x08;

/// FNV-1a 偏移。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold(h: u64, b: u64) -> u64 {
    let mut v = h ^ (b & 0xff);
    let mut i = 0u32;
    while i < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    v
}

/// 十进制拼接。
fn push_dec(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut k = i;
    while k < 20 {
        if let Some(b) = buf.get(k) {
            s.push(*b as char);
        }
        k += 1;
    }
}

/// 单个十六进制字节（小写）。
fn push_hex(s: &mut String, b: u8) {
    const T: [u8; 16] = *b"0123456789abcdef";
    if let Some(c) = T.get((b >> 4) as usize) {
        s.push(*c as char);
    }
    if let Some(c) = T.get((b & 0x0f) as usize) {
        s.push(*c as char);
    }
}

/// 单个十六进制字节（大写）。
fn push_hex_upper(s: &mut String, b: u8) {
    const T: [u8; 16] = *b"0123456789ABCDEF";
    if let Some(c) = T.get((b >> 4) as usize) {
        s.push(*c as char);
    }
    if let Some(c) = T.get((b & 0x0f) as usize) {
        s.push(*c as char);
    }
}

/// 字节 → 可见串。
fn str_of(b: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    s
}

/// 取字节。
fn byte_at(b: &[u8], i: usize) -> Option<u8> {
    match b.get(i) {
        Some(v) => Some(*v),
        None => None,
    }
}

/// 找字节。
fn find_byte(b: &[u8], want: u8) -> Option<usize> {
    let mut i = 0usize;
    while i < b.len() {
        if byte_at(b, i) == Some(want) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 自起找字节。
fn find_from(b: &[u8], start: usize, want: u8) -> Option<usize> {
    let mut i = start;
    while i < b.len() {
        if byte_at(b, i) == Some(want) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 跳过空格。
fn skip_spaces(b: &[u8], mut p: usize) -> usize {
    while byte_at(b, p) == Some(b' ') {
        p += 1;
    }
    p
}

/// 去两端空格。
fn trim(b: &[u8]) -> &[u8] {
    let mut lo = 0usize;
    let mut hi = b.len();
    while lo < hi && byte_at(b, lo) == Some(b' ') {
        lo += 1;
    }
    while hi > lo && byte_at(b, hi - 1) == Some(b' ') {
        hi -= 1;
    }
    b.get(lo..hi).unwrap_or(&[])
}

/// 是否十六进制字符。
fn is_hex(b: u8) -> bool {
    (b >= b'0' && b <= b'9') || (b >= b'a' && b <= b'f') || (b >= b'A' && b <= b'F')
}

/// 是否字母数字。
fn is_alnum(b: u8) -> bool {
    (b >= b'0' && b <= b'9') || (b >= b'a' && b <= b'z') || (b >= b'A' && b <= b'Z')
}

/// 十六进制字符值。
fn hexval(b: u8) -> Result<u8, u32> {
    if b >= b'0' && b <= b'9' {
        return Ok(b - b'0');
    }
    if b >= b'a' && b <= b'f' {
        return Ok(b - b'a' + 10);
    }
    if b >= b'A' && b <= b'F' {
        return Ok(b - b'A' + 10);
    }
    Err(HS_EBYTE)
}

/// 小写化。
fn lower(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

/// 大小写不敏感相等。
fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i) {
            Some(v) => lower(*v),
            None => return false,
        };
        let y = match b.get(i) {
            Some(v) => lower(*v),
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

/// 大端 u32。
fn be32(b: &[u8], at: usize) -> u32 {
    let x0 = byte_at(b, at).unwrap_or(0) as u32;
    let x1 = byte_at(b, at + 1).unwrap_or(0) as u32;
    let x2 = byte_at(b, at + 2).unwrap_or(0) as u32;
    let x3 = byte_at(b, at + 3).unwrap_or(0) as u32;
    (x0 << 24) | (x1 << 16) | (x2 << 8) | x3
}

/// 小端 u64。
fn le64(b: &[u8], at: usize) -> u64 {
    let mut v = 0u64;
    let mut i = 0usize;
    while i < 8 {
        v |= (byte_at(b, at + i).unwrap_or(0) as u64) << (8 * i);
        i += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// SHA-1
// ---------------------------------------------------------------------------

/// SHA-1 初始寄存器。
const SHA1_IV: [u32; 5] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476, 0xc3d2_e1f0];

/// SHA-1 流式状态。
#[derive(Clone, Copy, Debug)]
pub struct Sha1 {
    h: [u32; 5],
    buf: [u8; 64],
    fill: usize,
    total: u64,
    done: bool,
}

impl Sha1 {
    /// 新建。
    pub const fn new() -> Sha1 {
        Sha1 { h: SHA1_IV, buf: [0u8; 64], fill: 0, total: 0, done: false }
    }

    /// 喂入一段。
    pub fn update(&mut self, data: &[u8]) -> Result<(), u32> {
        if self.done {
            return Err(HS_ESTATE);
        }
        if data.len() > MAX_INPUT {
            return Err(HS_ELEN);
        }
        self.total = self.total.saturating_add(data.len() as u64);
        let mut i = 0usize;
        while i < data.len() {
            let space = 64 - self.fill;
            let rest = data.len() - i;
            let n = if rest < space { rest } else { space };
            let mut k = 0usize;
            while k < n {
                if let (Some(dst), Some(src)) = (self.buf.get_mut(self.fill + k), data.get(i + k)) {
                    *dst = *src;
                }
                k += 1;
            }
            self.fill += n;
            i += n;
            if self.fill == 64 {
                let block = self.buf;
                self.compress(&block);
                self.fill = 0;
            }
        }
        Ok(())
    }

    /// 收尾，返回二十字节摘要。
    pub fn finish(&mut self) -> Result<[u8; 20], u32> {
        if self.done {
            return Err(HS_ESTATE);
        }
        let bits = self.total.saturating_mul(8);
        self.push_raw(0x80);
        while self.fill != 56 {
            self.push_raw(0x00);
        }
        let mut lenb = [0u8; 8];
        let mut v = bits;
        let mut i = 8usize;
        while i > 0 {
            i -= 1;
            lenb[i] = (v & 0xff) as u8;
            v >>= 8;
        }
        let mut k = 0usize;
        while k < 8 {
            if let Some(b) = lenb.get(k).copied() {
                self.push_raw(b);
            }
            k += 1;
        }
        let mut out = [0u8; 20];
        let mut j = 0usize;
        while j < 5 {
            let w = self.h.get(j).copied().unwrap_or(0);
            if let Some(d) = out.get_mut(j * 4) {
                *d = (w >> 24) as u8;
            }
            if let Some(d) = out.get_mut(j * 4 + 1) {
                *d = (w >> 16) as u8;
            }
            if let Some(d) = out.get_mut(j * 4 + 2) {
                *d = (w >> 8) as u8;
            }
            if let Some(d) = out.get_mut(j * 4 + 3) {
                *d = w as u8;
            }
            j += 1;
        }
        self.done = true;
        Ok(out)
    }

    /// 追加一个填充字节。
    fn push_raw(&mut self, b: u8) {
        if let Some(dst) = self.buf.get_mut(self.fill) {
            *dst = b;
        }
        self.fill += 1;
        if self.fill == 64 {
            let block = self.buf;
            self.compress(&block);
            self.fill = 0;
        }
    }

    /// 压一个 64 字节块。
    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        let mut i = 0usize;
        while i < 16 {
            w[i] = be32(block, i * 4);
            i += 1;
        }
        while i < 80 {
            let a = w.get(i - 3).copied().unwrap_or(0);
            let b = w.get(i - 8).copied().unwrap_or(0);
            let c = w.get(i - 14).copied().unwrap_or(0);
            let d = w.get(i - 16).copied().unwrap_or(0);
            w[i] = (a ^ b ^ c ^ d).rotate_left(1);
            i += 1;
        }
        let mut a = self.h.get(0).copied().unwrap_or(0);
        let mut b = self.h.get(1).copied().unwrap_or(0);
        let mut c = self.h.get(2).copied().unwrap_or(0);
        let mut d = self.h.get(3).copied().unwrap_or(0);
        let mut e = self.h.get(4).copied().unwrap_or(0);
        let mut r = 0usize;
        while r < 80 {
            let (f, k) = if r < 20 {
                ((b & c) | ((!b) & d), 0x5a82_7999u32)
            } else if r < 40 {
                (b ^ c ^ d, 0x6ed9_eba1u32)
            } else if r < 60 {
                ((b & c) | (b & d) | (c & d), 0x8f1b_bcdcu32)
            } else {
                (b ^ c ^ d, 0xca62_c1d6u32)
            };
            let wi = w.get(r).copied().unwrap_or(0);
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
            r += 1;
        }
        if let Some(x) = self.h.get_mut(0) {
            *x = x.wrapping_add(a);
        }
        if let Some(x) = self.h.get_mut(1) {
            *x = x.wrapping_add(b);
        }
        if let Some(x) = self.h.get_mut(2) {
            *x = x.wrapping_add(c);
        }
        if let Some(x) = self.h.get_mut(3) {
            *x = x.wrapping_add(d);
        }
        if let Some(x) = self.h.get_mut(4) {
            *x = x.wrapping_add(e);
        }
    }

    /// 已喂字节数。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 缓冲占用。
    pub fn buffered(&self) -> usize {
        self.fill
    }

    /// 是否已收尾。
    pub fn is_done(&self) -> bool {
        self.done
    }
}

// ---------------------------------------------------------------------------
// BLAKE2b
// ---------------------------------------------------------------------------

/// BLAKE2b 初始向量。
const B2B_IV: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

/// BLAKE2b 置换表（十行循环用两轮）。
const SIGMA: [[u8; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

/// 取混合字。
fn mword(m: &[u64; 16], i: u8) -> u64 {
    m.get(i as usize).copied().unwrap_or(0)
}

/// 取状态字。
fn vget(v: &[u64; 16], i: usize) -> u64 {
    v.get(i).copied().unwrap_or(0)
}

/// 写状态字。
fn vput(v: &mut [u64; 16], i: usize, x: u64) {
    if let Some(d) = v.get_mut(i) {
        *d = x;
    }
}

/// BLAKE2b 的 G 函数。
fn g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    let mut va = vget(v, a);
    let mut vb = vget(v, b);
    let mut vc = vget(v, c);
    let mut vd = vget(v, d);
    va = va.wrapping_add(vb).wrapping_add(x);
    vd = (vd ^ va).rotate_right(32);
    vc = vc.wrapping_add(vd);
    vb = (vb ^ vc).rotate_right(24);
    va = va.wrapping_add(vb).wrapping_add(y);
    vd = (vd ^ va).rotate_right(16);
    vc = vc.wrapping_add(vd);
    vb = (vb ^ vc).rotate_right(63);
    vput(v, a, va);
    vput(v, b, vb);
    vput(v, c, vc);
    vput(v, d, vd);
}

/// BLAKE2b 流式状态。
#[derive(Clone, Copy, Debug)]
pub struct Blake2b {
    h: [u64; 8],
    t_lo: u64,
    t_hi: u64,
    buf: [u8; 128],
    fill: usize,
    outlen: usize,
    done: bool,
}

impl Blake2b {
    /// 新建；`outlen` 取 1..=64。
    pub fn new(outlen: usize) -> Result<Blake2b, u32> {
        if outlen == 0 || outlen > B2B_MAX {
            return Err(HS_ERANGE);
        }
        let mut h = B2B_IV;
        let param = 0x0101_0000u64 ^ (outlen as u64);
        if let Some(d) = h.get_mut(0) {
            *d ^= param;
        }
        Ok(Blake2b {
            h,
            t_lo: 0,
            t_hi: 0,
            buf: [0u8; 128],
            fill: 0,
            outlen,
            done: false,
        })
    }

    /// 喂入一段。
    pub fn update(&mut self, data: &[u8]) -> Result<(), u32> {
        if self.done {
            return Err(HS_ESTATE);
        }
        if data.len() > MAX_INPUT {
            return Err(HS_ELEN);
        }
        let mut i = 0usize;
        while i < data.len() {
            if self.fill == 128 {
                let block = self.buf;
                self.add_t(128);
                self.compress(&block, false);
                self.fill = 0;
            }
            let space = 128 - self.fill;
            let rest = data.len() - i;
            let n = if rest < space { rest } else { space };
            let mut k = 0usize;
            while k < n {
                if let (Some(dst), Some(src)) = (self.buf.get_mut(self.fill + k), data.get(i + k)) {
                    *dst = *src;
                }
                k += 1;
            }
            self.fill += n;
            i += n;
        }
        Ok(())
    }

    /// 收尾。
    pub fn finish(&mut self) -> Result<Digest, u32> {
        if self.done {
            return Err(HS_ESTATE);
        }
        self.add_t(self.fill as u64);
        let mut i = self.fill;
        while i < 128 {
            if let Some(d) = self.buf.get_mut(i) {
                *d = 0;
            }
            i += 1;
        }
        let block = self.buf;
        self.compress(&block, true);
        self.done = true;
        let mut out = Digest::blank();
        let mut k = 0usize;
        while k < self.outlen {
            let wi = k / 8;
            let sh = ((k % 8) * 8) as u32;
            let w = self.h.get(wi).copied().unwrap_or(0);
            if let Some(d) = out.bytes.get_mut(k) {
                *d = ((w >> sh) & 0xff) as u8;
            }
            k += 1;
        }
        out.len = self.outlen;
        Ok(out)
    }

    /// 计数加上块长。
    fn add_t(&mut self, n: u64) {
        let (lo, carry) = self.t_lo.overflowing_add(n);
        self.t_lo = lo;
        if carry {
            self.t_hi = self.t_hi.wrapping_add(1);
        }
    }

    /// 压一个 128 字节块。
    fn compress(&mut self, block: &[u8; 128], last: bool) {
        let mut m = [0u64; 16];
        let mut i = 0usize;
        while i < 16 {
            m[i] = le64(block, i * 8);
            i += 1;
        }
        let mut v = [0u64; 16];
        i = 0;
        while i < 8 {
            v[i] = self.h.get(i).copied().unwrap_or(0);
            i += 1;
        }
        i = 0;
        while i < 8 {
            v[8 + i] = B2B_IV.get(i).copied().unwrap_or(0);
            i += 1;
        }
        if let Some(x) = v.get_mut(12) {
            *x ^= self.t_lo;
        }
        if let Some(x) = v.get_mut(13) {
            *x ^= self.t_hi;
        }
        if last {
            if let Some(x) = v.get_mut(14) {
                *x = !*x;
            }
        }
        let mut r = 0usize;
        while r < 12 {
            let s = SIGMA.get(r).copied().unwrap_or([0u8; 16]);
            g(&mut v, 0, 4, 8, 12, mword(&m, s[0]), mword(&m, s[1]));
            g(&mut v, 1, 5, 9, 13, mword(&m, s[2]), mword(&m, s[3]));
            g(&mut v, 2, 6, 10, 14, mword(&m, s[4]), mword(&m, s[5]));
            g(&mut v, 3, 7, 11, 15, mword(&m, s[6]), mword(&m, s[7]));
            g(&mut v, 0, 5, 10, 15, mword(&m, s[8]), mword(&m, s[9]));
            g(&mut v, 1, 6, 11, 12, mword(&m, s[10]), mword(&m, s[11]));
            g(&mut v, 2, 7, 8, 13, mword(&m, s[12]), mword(&m, s[13]));
            g(&mut v, 3, 4, 9, 14, mword(&m, s[14]), mword(&m, s[15]));
            r += 1;
        }
        i = 0;
        while i < 8 {
            let a = self.h.get(i).copied().unwrap_or(0);
            let b = vget(&v, i);
            let c = vget(&v, 8 + i);
            if let Some(d) = self.h.get_mut(i) {
                *d = a ^ b ^ c;
            }
            i += 1;
        }
    }

    /// 摘要长度。
    pub fn outlen(&self) -> usize {
        self.outlen
    }

    /// 是否已收尾。
    pub fn is_done(&self) -> bool {
        self.done
    }
}

// ---------------------------------------------------------------------------
// 摘要与算法
// ---------------------------------------------------------------------------

/// 摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Digest {
    bytes: [u8; B2B_MAX],
    len: usize,
}

impl Digest {
    /// 空白摘要。
    pub const fn blank() -> Digest {
        Digest { bytes: [0u8; B2B_MAX], len: 0 }
    }

    /// 从字节切片构造（截到 64）。
    pub fn from_bytes(b: &[u8]) -> Digest {
        let mut d = Digest::blank();
        let n = if b.len() > B2B_MAX { B2B_MAX } else { b.len() };
        let mut i = 0usize;
        while i < n {
            if let (Some(dst), Some(src)) = (d.bytes.get_mut(i), b.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        d.len = n;
        d
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取第 i 字节。
    pub fn byte(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.bytes.get(i).copied()
    }

    /// 逐字节相等。
    pub fn equals(&self, other: &Digest) -> bool {
        if self.len != other.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            let a = self.bytes.get(i).copied().unwrap_or(0);
            let b = other.bytes.get(i).copied().unwrap_or(0);
            if a != b {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 与小写十六进制串相等。
    pub fn hex_is(&self, want: &[u8]) -> bool {
        let s = self.hex();
        s.as_str().as_bytes() == want
    }

    /// 小写十六进制。
    pub fn hex(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bytes.get(i) {
                push_hex(&mut s, *b);
            }
            i += 1;
        }
        s
    }

    /// 大写十六进制。
    pub fn upper(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bytes.get(i) {
                push_hex_upper(&mut s, *b);
            }
            i += 1;
        }
        s
    }

    /// 十六进制长度。
    pub fn hexlen(&self) -> usize {
        self.len.saturating_mul(2)
    }
}

/// 算法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    /// SHA-1（160 位）。
    Sha1,
    /// BLAKE2b-256。
    Blake2b256,
    /// BLAKE2b-512。
    Blake2b512,
}

impl Algorithm {
    /// 全部。
    pub fn all() -> [Algorithm; 3] {
        [Algorithm::Sha1, Algorithm::Blake2b256, Algorithm::Blake2b512]
    }

    /// 序号。
    pub fn index(&self) -> u8 {
        match self {
            Algorithm::Sha1 => 0,
            Algorithm::Blake2b256 => 1,
            Algorithm::Blake2b512 => 2,
        }
    }

    /// 小写名。
    pub fn text(&self) -> &'static str {
        match self {
            Algorithm::Sha1 => "sha1",
            Algorithm::Blake2b256 => "blake2b-256",
            Algorithm::Blake2b512 => "blake2b-512",
        }
    }

    /// 大写标签名。
    pub fn tag_text(&self) -> &'static str {
        match self {
            Algorithm::Sha1 => "SHA1",
            Algorithm::Blake2b256 => "BLAKE2b-256",
            Algorithm::Blake2b512 => "BLAKE2b-512",
        }
    }

    /// 摘要字节数。
    pub fn outlen(&self) -> usize {
        match self {
            Algorithm::Sha1 => SHA1_LEN,
            Algorithm::Blake2b256 => 32,
            Algorithm::Blake2b512 => B2B_MAX,
        }
    }

    /// 十六进制长度。
    pub fn hexlen(&self) -> usize {
        self.outlen().saturating_mul(2)
    }

    /// 解析名字。
    pub fn parse(name: &[u8]) -> Result<Algorithm, u32> {
        if eq_ci(name, b"sha1") || eq_ci(name, b"sha1sum") {
            return Ok(Algorithm::Sha1);
        }
        if eq_ci(name, b"blake2b-256") || eq_ci(name, b"b2-256") {
            return Ok(Algorithm::Blake2b256);
        }
        if eq_ci(name, b"blake2b-512")
            || eq_ci(name, b"blake2b")
            || eq_ci(name, b"b2sum")
            || eq_ci(name, b"b2-512")
        {
            return Ok(Algorithm::Blake2b512);
        }
        Err(HS_EALG)
    }

    /// 由十六进制长度反推。
    pub fn from_hexlen(n: usize) -> Result<Algorithm, u32> {
        if n == 40 {
            return Ok(Algorithm::Sha1);
        }
        if n == 64 {
            return Ok(Algorithm::Blake2b256);
        }
        if n == 128 {
            return Ok(Algorithm::Blake2b512);
        }
        Err(HS_ERANGE)
    }

    /// 一次性摘要。
    pub fn digest(&self, data: &[u8]) -> Result<Digest, u32> {
        let mut h = Hasher::new(*self)?;
        h.update(data)?;
        h.finish()
    }

    /// 十六进制摘要。
    pub fn hex_digest(&self, data: &[u8]) -> Result<String, u32> {
        let d = self.digest(data)?;
        Ok(d.hex())
    }
}

/// 流式哈希器。
#[derive(Clone, Copy, Debug)]
pub enum Hasher {
    /// SHA-1。
    S(Sha1),
    /// BLAKE2b。
    B(Blake2b),
}

impl Hasher {
    /// 按算法新建。
    pub fn new(alg: Algorithm) -> Result<Hasher, u32> {
        match alg {
            Algorithm::Sha1 => Ok(Hasher::S(Sha1::new())),
            Algorithm::Blake2b256 => Ok(Hasher::B(Blake2b::new(32)?)),
            Algorithm::Blake2b512 => Ok(Hasher::B(Blake2b::new(64)?)),
        }
    }

    /// 喂入。
    pub fn update(&mut self, data: &[u8]) -> Result<(), u32> {
        match self {
            Hasher::S(h) => h.update(data),
            Hasher::B(h) => h.update(data),
        }
    }

    /// 收尾。
    pub fn finish(&mut self) -> Result<Digest, u32> {
        match self {
            Hasher::S(h) => {
                let b = h.finish()?;
                Ok(Digest::from_bytes(&b))
            }
            Hasher::B(h) => h.finish(),
        }
    }

    /// 是否已收尾。
    pub fn is_done(&self) -> bool {
        match self {
            Hasher::S(h) => h.is_done(),
            Hasher::B(h) => h.is_done(),
        }
    }
}

/// 十六进制解回摘要。
pub fn parse_hex(hex: &[u8], want: usize) -> Result<Digest, u32> {
    if hex.len() != want.saturating_mul(2) {
        return Err(HS_ERANGE);
    }
    let mut d = Digest::blank();
    let mut i = 0usize;
    while i < want {
        let hi = hexval(byte_at(hex, i * 2).unwrap_or(0));
        let lo = match hi {
            Ok(_) => hexval(byte_at(hex, i * 2 + 1).unwrap_or(0)),
            Err(c) => Err(c),
        };
        let hv = match hi {
            Ok(v) => v,
            Err(c) => return Err(c),
        };
        let lv = match lo {
            Ok(v) => v,
            Err(c) => return Err(c),
        };
        if let Some(dst) = d.bytes.get_mut(i) {
            *dst = (hv << 4) | lv;
        }
        i += 1;
    }
    d.len = want;
    Ok(d)
}

// ---------------------------------------------------------------------------
// 校验行
// ---------------------------------------------------------------------------

/// 一条校验记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    alg: Algorithm,
    digest: Digest,
    name: [u8; NAME_CAP],
    name_len: usize,
    tagged: bool,
}

impl Entry {
    /// 空白记录。
    pub const fn blank() -> Entry {
        Entry {
            alg: Algorithm::Sha1,
            digest: Digest::blank(),
            name: [0u8; NAME_CAP],
            name_len: 0,
            tagged: false,
        }
    }

    /// 构造并校验。
    pub fn make(alg: Algorithm, digest: Digest, name: &[u8], tagged: bool) -> Result<Entry, u32> {
        if digest.len() != alg.outlen() {
            return Err(HS_ERANGE);
        }
        if name.is_empty() {
            return Err(HS_ESHORT);
        }
        let mut blank = true;
        let mut q = 0usize;
        while q < name.len() {
            if name.get(q).copied() != Some(b' ') {
                blank = false;
            }
            q += 1;
        }
        if blank {
            return Err(HS_ESHORT);
        }
        if name.len() > NAME_CAP {
            return Err(HS_ELEN);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < name.len() {
            let b = match name.get(i) {
                Some(v) => *v,
                None => return Err(HS_EBYTE),
            };
            if b < 0x20 || b > 0x7e {
                return Err(HS_EBYTE);
            }
            if let Some(d) = buf.get_mut(i) {
                *d = b;
            }
            i += 1;
        }
        Ok(Entry { alg, digest, name: buf, name_len: name.len(), tagged })
    }

    /// 解析一行；末尾的换行/回车先剥掉。
    pub fn parse(line: &[u8]) -> Result<Entry, u32> {
        let mut end = line.len();
        loop {
            if end == 0 {
                break;
            }
            let b = byte_at(line, end - 1).unwrap_or(0);
            if b == 10u8 || b == 13u8 {
                end -= 1;
            } else {
                break;
            }
        }
        let line = line.get(0..end).unwrap_or(&[]);
        if line.is_empty() {
            return Err(HS_ESHORT);
        }
        if let Some(open) = find_byte(line, b'(') {
            if let Some(close) = find_from(line, open + 1, b')') {
                let head = line.get(0..open).unwrap_or(&[]);
                let alg = Algorithm::parse(trim(head))?;
                let mut p = close + 1;
                p = skip_spaces(line, p);
                if byte_at(line, p) != Some(b'=') {
                    return Err(HS_EBYTE);
                }
                p = skip_spaces(line, p + 1);
                let hex = line.get(p..).unwrap_or(&[]);
                let digest = parse_hex(hex, alg.outlen())?;
                let name = line.get(open + 1..close).unwrap_or(&[]);
                return Entry::make(alg, digest, name, true);
            }
        }
        let mut hlen = 0usize;
        while hlen < line.len() {
            match line.get(hlen) {
                Some(b) => {
                    if is_hex(*b) {
                        hlen += 1;
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
        if hlen == 0 {
            return Err(if byte_at(line, 0) == Some(b' ') {
                HS_ESHORT
            } else {
                HS_EBYTE
            });
        }
        if hlen == line.len() {
            return Err(HS_ESHORT);
        }
        let sep = byte_at(line, hlen).unwrap_or(0);
        if sep != b' ' {
            return Err(if is_alnum(sep) { HS_EBYTE } else { HS_ESHORT });
        }
        let alg = Algorithm::from_hexlen(hlen)?;
        let hex = line.get(0..hlen).unwrap_or(&[]);
        let digest = parse_hex(hex, alg.outlen())?;
        let mut p = hlen + 1;
        if byte_at(line, p) == Some(b'*') {
            p += 1;
        } else if byte_at(line, p) == Some(b' ') {
            p += 1;
        }
        if p >= line.len() {
            return Err(HS_ESHORT);
        }
        let name = line.get(p..).unwrap_or(&[]);
        Entry::make(alg, digest, name, false)
    }

    /// 由内容直接构造。
    pub fn from_content(
        alg: Algorithm,
        content: &[u8],
        name: &[u8],
        tagged: bool,
    ) -> Result<Entry, u32> {
        let d = alg.digest(content)?;
        Entry::make(alg, d, name, tagged)
    }

    /// 算法。
    pub fn alg(&self) -> Algorithm {
        self.alg
    }

    /// 摘要。
    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    /// 名称字节。
    pub fn name(&self) -> &[u8] {
        self.name.get(0..self.name_len).unwrap_or(&[])
    }

    /// 名称串。
    pub fn name_text(&self) -> String {
        str_of(self.name())
    }

    /// 是否标签格式。
    pub fn tagged(&self) -> bool {
        self.tagged
    }

    /// 名称长度。
    pub fn name_len(&self) -> usize {
        self.name_len
    }

    /// 与内容比对。
    pub fn matches(&self, content: &[u8]) -> Result<bool, u32> {
        let d = self.alg.digest(content)?;
        if d.len != self.digest.len {
            return Ok(false);
        }
        let mut i = 0usize;
        while i < d.len {
            let a = d.bytes.get(i).copied().unwrap_or(0);
            let b = self.digest.bytes.get(i).copied().unwrap_or(0);
            if a != b {
                return Ok(false);
            }
            i += 1;
        }
        Ok(true)
    }

    /// 自检。
    pub fn validate(&self) -> Result<(), u32> {
        if self.name_len == 0 || self.name_len > NAME_CAP {
            return Err(HS_EINVAL);
        }
        if self.digest.len != self.alg.outlen() {
            return Err(HS_EINVAL);
        }
        Ok(())
    }

    /// GNU 校验行（`<hex>  <name>`），可被 `parse` 原样读回。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(self.digest.hex().as_str());
        s.push(' ');
        s.push(' ');
        s.push_str(self.name_text().as_str());
        s
    }

    /// BSD 标签一行。
    pub fn tag_line(&self) -> String {
        let mut s = String::new();
        s.push_str(self.alg.tag_text());
        s.push(' ');
        s.push('(');
        s.push_str(self.name_text().as_str());
        s.push(')');
        s.push(' ');
        s.push('=');
        s.push(' ');
        s.push_str(self.digest.hex().as_str());
        s
    }
}

/// 判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 一致。
    Good,
    /// 不一致。
    Bad,
    /// 无法判定。
    Unreadable,
}

impl Verdict {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Verdict::Good => "OK",
            Verdict::Bad => "FAILED",
            Verdict::Unreadable => "UNREADABLE",
        }
    }
}

/// 对一条记录与内容给出判定。
pub fn verdict(e: &Entry, content: &[u8]) -> Verdict {
    match e.matches(content) {
        Ok(true) => Verdict::Good,
        Ok(false) => Verdict::Bad,
        Err(_) => Verdict::Unreadable,
    }
}

/// 解析统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Summary {
    /// 总数。
    pub total: usize,
    /// 标签格式数。
    pub tagged: usize,
    /// 普通格式数。
    pub plain: usize,
    /// sha1 条数。
    pub sha1: usize,
    /// blake2b-256 条数。
    pub b256: usize,
    /// blake2b-512 条数。
    pub b512: usize,
}

/// 解析批。
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    rows: [Entry; BATCH_CAP],
    len: usize,
}

impl Batch {
    /// 空批。
    pub const fn new() -> Batch {
        Batch { rows: [Entry::blank(); BATCH_CAP], len: 0 }
    }

    /// 登记一条。
    pub fn register(&mut self, e: Entry) -> Result<usize, u32> {
        if e.validate().is_err() {
            return Err(HS_EINVAL);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(x) = self.rows.get(i) {
                if x.name() == e.name() {
                    return Err(HS_EDUP);
                }
            }
            i += 1;
        }
        if self.len >= BATCH_CAP {
            return Err(HS_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = e;
        }
        self.len += 1;
        Ok(self.len)
    }

    /// 解析一行并登记。
    pub fn parse_line(&mut self, line: &[u8]) -> Result<usize, u32> {
        let e = Entry::parse(line)?;
        self.register(e)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一条。
    pub fn at(&self, i: usize) -> Option<&Entry> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i)
    }

    /// 切片。
    pub fn as_slice(&self) -> &[Entry] {
        self.rows.get(0..self.len).unwrap_or(&[])
    }

    /// 统计。
    pub fn summary(&self) -> Summary {
        let mut s = Summary::default();
        s.total = self.len;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if e.tagged {
                    s.tagged += 1;
                } else {
                    s.plain += 1;
                }
                match e.alg {
                    Algorithm::Sha1 => s.sha1 += 1,
                    Algorithm::Blake2b256 => s.b256 += 1,
                    Algorithm::Blake2b512 => s.b512 += 1,
                }
            }
            i += 1;
        }
        s
    }
}

/// 渲染报表：表头 + 每条一行 + 合计。
pub fn render(entries: &[Entry]) -> Result<String, u32> {
    if entries.len() > BATCH_CAP {
        return Err(HS_ELEN);
    }
    let mut s = String::new();
    s.push_str("unx11538 hash report\n");
    let mut i = 0usize;
    while i < entries.len() {
        if let Some(e) = entries.get(i) {
            s.push_str(e.render().as_str());
            s.push('\n');
        }
        i += 1;
    }
    s.push_str("total ");
    push_dec(&mut s, entries.len() as u64);
    s.push('\n');
    Ok(s)
}

// ---------------------------------------------------------------------------
// 算法目录（重复检测）
// ---------------------------------------------------------------------------

/// 算法目录。
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    seen: [u8; 3],
    len: usize,
    frozen: bool,
    rejects: u32,
}

impl Catalog {
    /// 空目录。
    pub const fn new() -> Catalog {
        Catalog { seen: [0u8; 3], len: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, a: Algorithm) -> Result<usize, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HS_ESLOT);
        }
        let idx = a.index();
        let mut i = 0usize;
        while i < self.len {
            if self.seen.get(i).copied() == Some(idx) {
                self.rejects = self.rejects.saturating_add(1);
                return Err(HS_EDUP);
            }
            i += 1;
        }
        if self.len >= 3 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HS_ESLOT);
        }
        if let Some(d) = self.seen.get_mut(self.len) {
            *d = idx;
        }
        self.len += 1;
        Ok(self.len)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否含某算法。
    pub fn contains(&self, a: Algorithm) -> bool {
        let idx = a.index();
        let mut i = 0usize;
        while i < self.len {
            if self.seen.get(i).copied() == Some(idx) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 三种算法是否齐备。
    pub fn complete(&self) -> bool {
        self.len == 3 && self.contains(Algorithm::Sha1) && self.contains(Algorithm::Blake2b256)
            && self.contains(Algorithm::Blake2b512)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HS_ESLOT);
        }
        self.frozen = true;
        Ok(())
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HS_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
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

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(HS_EBAL);
        }
        const VALS: [u32; 8] = [169, 186, 189, 4, 4, 128, 6, 1];
        const CODES: [u32; 6] = [HS_EALG, HS_EBYTE, HS_ESHORT, HS_ERANGE, HS_EDUP, HS_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(HS_EBAL),
            };
            if s.slot as usize != i {
                return Err(HS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(HS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(HS_EVALUE);
                        }
                    }
                    None => return Err(HS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(HS_EVALUE);
                        }
                    }
                    None => return Err(HS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(HS_EBAL);
        }
        Ok(())
    }

    /// 指纹。
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
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HS_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(HS_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 样例校验行。
pub const SAMPLE_LINES: [&[u8]; 4] = [
    b"a9993e364706816aba3e25717850c26c9cd0d89d  abc.txt",
    b"ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923 *beta.bin",
    b"bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319  gamma.txt",
    b"SHA1 (delta.txt) = da39a3ee5e6b4b0d3255bfef95601890afd80709",
];

/// 样例内容（与 `SAMPLE_LINES` 对应）。
pub const SAMPLE_CONTENT: [&[u8]; 4] = [b"abc", b"abc", b"abc", b""];

/// 样例批。
pub fn sample_batch() -> Result<Batch, u32> {
    let mut b = Batch::new();
    let mut i = 0usize;
    while i < SAMPLE_LINES.len() {
        if let Some(line) = SAMPLE_LINES.get(i) {
            b.parse_line(line)?;
        }
        i += 1;
    }
    Ok(b)
}

/// 样例核验通过数。
pub fn sample_verified() -> Result<usize, u32> {
    let b = sample_batch()?;
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        let e = match b.at(i) {
            Some(v) => v,
            None => return Err(HS_ENOBS),
        };
        let c = match SAMPLE_CONTENT.get(i) {
            Some(v) => *v,
            None => return Err(HS_ENOBS),
        };
        if e.matches(c)? {
            n += 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 样例报表行数。
pub fn sample_render_lines() -> Result<usize, u32> {
    let b = sample_batch()?;
    let s = render(b.as_slice())?;
    Ok(s.as_str().matches('\n').count())
}

/// 首字节观测。
fn first_byte(alg: Algorithm, data: &[u8]) -> Result<u32, u32> {
    match alg.digest(data) {
        Ok(d) => match d.byte(0) {
            Some(b) => Ok(b as u32),
            None => Err(HS_ENOBS),
        },
        Err(c) => Err(c),
    }
}

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_DIGEST, first_byte(Algorithm::Sha1, b"abc")));

    let _ = l.register(word_cell(1, OP_DIGEST, first_byte(Algorithm::Blake2b512, b"abc")));

    let _ = l.register(word_cell(2, OP_DIGEST, first_byte(Algorithm::Blake2b256, b"abc")));

    let _ = l.register(count_cell(3, OP_PARSE, sample_batch().map(|b| b.len())));

    let _ = l.register(count_cell(4, OP_VERIFY, sample_verified()));

    let _ = l.register(count_cell(5, OP_RENDER, Ok(Algorithm::Blake2b512.hexlen())));

    let _ = l.register(count_cell(6, OP_RENDER, sample_render_lines()));

    let _ = l.register(count_cell(7, OP_PARSE, sample_batch().map(|b| b.summary().tagged)));

    let _ = l.register(word_cell(8, OP_ALG, Algorithm::parse(b"md5").map(|a| a.index() as u32)));

    let _ = l.register(word_cell(
        9,
        OP_PARSE,
        Entry::parse(b"zz93e364706816aba3e25717850c26c9cd0d89d  x").map(|e| e.name_len as u32),
    ));

    let _ = l.register(word_cell(10, OP_PARSE, Entry::parse(b"abc").map(|e| e.name_len as u32)));

    let _ = l.register(word_cell(11, OP_RANGE, Algorithm::from_hexlen(88).map(|a| a.index() as u32)));

    let _ = l.register(word_cell(12, OP_REG, {
        let mut c = Catalog::new();
        let a = c.register(Algorithm::Sha1);
        let b = c.register(Algorithm::Sha1);
        match a {
            Ok(_) => b.map(|_| 1),
            Err(e) => Err(e),
        }
    }));

    let _ = l.register(word_cell(13, OP_STATE, {
        let mut h = Sha1::new();
        let first = h.finish();
        let second = h.finish();
        match first {
            Ok(_) => second.map(|_| 1),
            Err(e) => Err(e),
        }
    }));

    l
}

// ---------------------------------------------------------------------------
// 参考向量
// ---------------------------------------------------------------------------

/// 模式：第 i 字节 = (7*i + 3) & 0xff。
fn fill_pattern(buf: &mut [u8], n: usize) {
    let mut i = 0usize;
    while i < n && i < buf.len() {
        if let Some(d) = buf.get_mut(i) {
            *d = ((i as u32 * 7 + 3) & 0xff) as u8;
        }
        i += 1;
    }
}

/// 对模式取摘要。
fn digest_pattern(alg: Algorithm, n: usize) -> Result<Digest, u32> {
    let mut buf = [0u8; 300];
    fill_pattern(&mut buf, n);
    alg.digest(buf.get(0..n).unwrap_or(&[]))
}

/// SHA-1 向量表。
const SHA1_TABLE: [(usize, &[u8]); 12] = [
    (0, b"da39a3ee5e6b4b0d3255bfef95601890afd80709"),
    (1, b"9842926af7ca0a8cca12604f945414f07b01e13d"),
    (3, b"4201de9f98cb0a9b8cf52398be7802b55d45266a"),
    (55, b"ddf57317ef34bfee3b6df83d359098930eb278bc"),
    (56, b"a0d492bb0fc889d0eca3bc137066ab6f4f74f369"),
    (57, b"11a02dcf95859677a62e75024067c22b165d890f"),
    (63, b"c55856749bef509bdfe6bfebfc7bf4e793e82132"),
    (64, b"bede92be29c3874e1b54ddc77988d606fc857a8e"),
    (65, b"b05a80522b053d6dc7e0a517d0e70212c7dad11f"),
    (127, b"34d5e582029e9b9b85b2febe31da3db7cdabaaea"),
    (128, b"a09133e6730ffe899efb70204cb5646cd5dc24ee"),
    (129, b"808aea332ce367541d37adae7f94e59c5c1a934e"),
];

/// BLAKE2b-512 向量表。
const B2B512_TABLE: [(usize, &[u8]); 12] = [
    (0, b"786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce"),
    (1, b"4fe4da61bcc756071b226843361d74944c72245d23e8245ea678c13fdcd7fe2ae529cf999ad99cc24f7a73416a18ba53e76c0afef83b16a568b12fbfc1a2674d"),
    (3, b"76ce7e90c577d436a65b691e51f0a1ad536130d70a18df3bf5368551327258b6f706f4a47c45f2d0d8147cfbb5623077e798247972e4f9338603a13abc05a4db"),
    (55, b"cd3d304c5fc217e84778b997f89dc654a6a44a88b1fd43c4668e4adfbbe2b0d169002a6f98b22ab98dde4603477dcdaeac3e6ff320b1059531fd8b6c36738e67"),
    (56, b"b84c95e7cb1bab5b0f5acfdd426cd90091fe5f98b2f9c047ea71542ffe6e757c29b47ec42c9fdbe423b43083f711fb546f0c3237dd42fe311ff4544d627f295a"),
    (57, b"f853027535ea376cf522c95f8bb8150ac82d3772a1dc684bbe79bff8f7117ce7920f680a9646b204ec2e371c8d930d9f75ecd935865d439da87286a161000d59"),
    (127, b"71546bbf9110ad184cc60f2eb120fcfd9b4dbbca7a7f1270045b8a23a6a4f4330f65c1f030dd2f5fabc6c57617242c37cf427bd90407fac5b9deffd3ae888c39"),
    (128, b"2d9e329f42afa3601d646692b81c13e87fcaff5bf15972e9813d7373cb6d181f9599f4d513d4af4fd6ebd37497aceb29aba5ee23ed764d8510b552bd088814fb"),
    (129, b"47889df9eb4d717afc5019df5c6a83df00a0b8677395e078cd5778ace0f338a618e68b7d9afb065d9e6a01ccd31d109447e7fae771c3ee3e105709194122ba2b"),
    (255, b"1a5199ac66a00e8a87ad1c7fbad30b33137dd8312bf6d98602dacf8f40ea2cb623a7fbc63e5a6bfa434d337ae7da5ca1a52502a215a3fe0297a151be85d88789"),
    (256, b"91019c558584980249ca43eceed27e19f1c3c24161b93eed1eee2a6a774f60bf8a81b43750870bee1698feac9c5336ae4d5c842e7ead159bf3916387e8ded9ae"),
    (257, b"9f1975efca45e7b74b020975d4d2c22802906ed8bfefca51ac497bd23147fc8f303890d8e5471ab6caaa02362e831a9e8d3435279912ccd4842c7806b096c348"),
];

/// BLAKE2b-256 向量表。
const B2B256_TABLE: [(usize, &[u8]); 6] = [
    (0, b"0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8"),
    (1, b"e88bd757ad5b9bedf372d8d3f0cf6c962a469db61a265f6418e1ffed86da29ec"),
    (3, b"9facd640130c81e7f318a8d5c660bc1b0f5d03861ffbd4cc1358e0e904593c28"),
    (55, b"03d4607f1e3d3c6054d276c53eab4ee4de04446370d6aaab103b24060349bf47"),
    (128, b"f0501d06597880592bc49234eef100ec1ff349058d0e9d9b753504e24af86dd6"),
    (257, b"4ce481b24d387422d2bc2baa03d1afd55a1327939ff537c71eb9b38709268649"),
];

/// 对表逐项比对。
fn table_ok(alg: Algorithm, table: &[(usize, &[u8])]) -> bool {
    let mut i = 0usize;
    while i < table.len() {
        if let Some((n, want)) = table.get(i) {
            let d = match digest_pattern(alg, *n) {
                Ok(v) => v,
                Err(_) => return false,
            };
            if !d.hex_is(want) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 经典向量。
fn classic_ok() -> bool {
    let a = match Algorithm::Sha1.digest(b"") {
        Ok(d) => d.hex_is(b"da39a3ee5e6b4b0d3255bfef95601890afd80709"),
        Err(_) => false,
    };
    let b = match Algorithm::Sha1.digest(b"abc") {
        Ok(d) => d.hex_is(b"a9993e364706816aba3e25717850c26c9cd0d89d"),
        Err(_) => false,
    };
    let c = match Algorithm::Sha1.digest(b"hello world\n") {
        Ok(d) => d.hex_is(b"22596363b3de40b06f981fb85d82312e8c0ed511"),
        Err(_) => false,
    };
    let e = match Algorithm::Sha1.digest(
        b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
    ) {
        Ok(d) => d.hex_is(b"a49b2446a02c645bf419f995b67091253a04a259"),
        Err(_) => false,
    };
    let f = match Algorithm::Blake2b512.digest(b"") {
        Ok(d) => d.hex_is(b"786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce"),
        Err(_) => false,
    };
    let g = match Algorithm::Blake2b512.digest(b"abc") {
        Ok(d) => d.hex_is(b"ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"),
        Err(_) => false,
    };
    let h = match Algorithm::Blake2b512.digest(b"hello world\n") {
        Ok(d) => d.hex_is(b"fec91c70284c72d0d4e3684788a90de9338a5b2f47f01fedbe203cafd68708718ae5672d10eca804a8121904047d40d1d6cf11e7a76419357a9469af41f22d01"),
        Err(_) => false,
    };
    let i = match Algorithm::Blake2b256.digest(b"") {
        Ok(d) => d.hex_is(b"0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8"),
        Err(_) => false,
    };
    let j = match Algorithm::Blake2b256.digest(b"abc") {
        Ok(d) => d.hex_is(b"bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319"),
        Err(_) => false,
    };
    let k = match Algorithm::Blake2b256.digest(b"hello world\n") {
        Ok(d) => d.hex_is(b"c71b05fd1d1c7bf7e928ff18e58db5193e9316416cc26ba9cc9094da80d7011e"),
        Err(_) => false,
    };
    a && b && c && e && f && g && h && i && j && k
}

/// 分块等价（三种算法 × 十五种长度 × 九种块长）。
fn streaming_ok() -> bool {
    const SIZES: [usize; 15] = [0, 1, 3, 55, 56, 57, 63, 64, 65, 127, 128, 129, 255, 256, 257];
    const CHUNKS: [usize; 9] = [1, 2, 3, 7, 16, 63, 64, 65, 127];
    let algs = Algorithm::all();
    let mut si = 0usize;
    while si < SIZES.len() {
        let n = SIZES.get(si).copied().unwrap_or(0);
        let mut buf = [0u8; 300];
        fill_pattern(&mut buf, n);
        let data = buf.get(0..n).unwrap_or(&[]);
        let mut ai = 0usize;
        while ai < algs.len() {
            let alg = match algs.get(ai) {
                Some(v) => *v,
                None => return false,
            };
            let whole = match alg.digest(data) {
                Ok(d) => d,
                Err(_) => return false,
            };
            let mut ci = 0usize;
            while ci < CHUNKS.len() {
                let cs = CHUNKS.get(ci).copied().unwrap_or(1);
                let mut h = match Hasher::new(alg) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                let mut off = 0usize;
                while off < data.len() {
                    let rest = data.len() - off;
                    let end = if rest < cs { data.len() } else { off + cs };
                    if h.update(data.get(off..end).unwrap_or(&[])).is_err() {
                        return false;
                    }
                    off = end;
                }
                let part = match h.finish() {
                    Ok(d) => d,
                    Err(_) => return false,
                };
                if !part.equals(&whole) {
                    return false;
                }
                ci += 1;
            }
            ai += 1;
        }
        si += 1;
    }
    true
}

/// 账本自洽。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit().is_err() {
        return false;
    }
    let s1 = l.seal();
    if build_ledger().seal() != s1 {
        return false;
    }
    let mut l3 = build_ledger();
    let r = match l3.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r != s1 || !l3.is_frozen() {
        return false;
    }
    let replay = match l3.replay() {
        Ok(v) => v == r,
        Err(_) => false,
    };
    if !replay {
        return false;
    }
    let probe = Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 };
    if l3.register(probe) != Err(HS_ESLOT) {
        return false;
    }
    l3.rejects() == 1
}

/// 解析与核验。
fn parse_ok() -> bool {
    let b = match sample_batch() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if b.len() != 4 {
        return false;
    }
    let s = b.summary();
    if s.tagged != 1 || s.plain != 3 {
        return false;
    }
    if s.sha1 != 2 || s.b256 != 1 || s.b512 != 1 {
        return false;
    }
    match sample_verified() {
        Ok(n) => n == 4,
        Err(_) => false,
    }
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = classic_ok() && table_ok(Algorithm::Sha1, &SHA1_TABLE);
    out[1] = streaming_ok();
    out[2] = ledger_ok();
    out[3] = parse_ok()
        && table_ok(Algorithm::Blake2b512, &B2B512_TABLE)
        && table_ok(Algorithm::Blake2b256, &B2B256_TABLE);
    out
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 判据集合。
pub fn run_unx11538_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11538.hash");

    s.add(
        "HS01-SHA1已知向量",
        {
            let a = match Algorithm::Sha1.digest(b"") {
                Ok(d) => d.hex_is(b"da39a3ee5e6b4b0d3255bfef95601890afd80709"),
                Err(_) => false,
            };
            let b = match Algorithm::Sha1.digest(b"abc") {
                Ok(d) => d.hex_is(b"a9993e364706816aba3e25717850c26c9cd0d89d"),
                Err(_) => false,
            };
            let c = match Algorithm::Sha1.digest(b"hello world\n") {
                Ok(d) => d.hex_is(b"22596363b3de40b06f981fb85d82312e8c0ed511"),
                Err(_) => false,
            };
            let e = match Algorithm::Sha1.digest(b"abc") {
                Ok(d) => d.hexlen() == 40 && d.len() == SHA1_LEN && d.byte(19).is_some(),
                Err(_) => false,
            };
            a && b && c && e
        },
        "static",
    );

    s.add(
        "HS02-分块等价",
        {
            streaming_ok()
        },
        "static",
    );

    s.add(
        "HS03-SHA1填充边界",
        {
            table_ok(Algorithm::Sha1, &SHA1_TABLE)
        },
        "static",
    );

    s.add(
        "HS04-BLAKE2b向量",
        {
            classic_ok()
        },
        "static",
    );

    s.add(
        "HS05-BLAKE2b变长",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i <= B2B_MAX {
                let mut h = match Blake2b::new(i) {
                    Ok(v) => v,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                };
                if h.update(b"abc").is_err() {
                    ok = false;
                    break;
                }
                let d = match h.finish() {
                    Ok(v) => v,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                };
                if d.len() != i {
                    ok = false;
                }
                let mut h2 = match Blake2b::new(i) {
                    Ok(v) => v,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                };
                let _ = h2.update(b"abc");
                let d2 = match h2.finish() {
                    Ok(v) => v,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                };
                if !d.equals(&d2) {
                    ok = false;
                }
                i += 1;
            }
            let a = Blake2b::new(0).err() == Some(HS_ERANGE);
            let b = Blake2b::new(65).err() == Some(HS_ERANGE);
            ok && a && b
        },
        "static",
    );

    s.add(
        "HS06-BLAKE2b填充边界",
        {
            table_ok(Algorithm::Blake2b512, &B2B512_TABLE)
                && table_ok(Algorithm::Blake2b256, &B2B256_TABLE)
        },
        "static",
    );

    s.add(
        "HS07-十六进制",
        {
            let d = match Algorithm::Sha1.digest(b"abc") {
                Ok(v) => v,
                Err(_) => Digest::blank(),
            };
            let lower = d.hex_is(b"a9993e364706816aba3e25717850c26c9cd0d89d");
            let upper = d.upper().as_str().as_bytes()
                == b"A9993E364706816ABA3E25717850C26C9CD0D89D";
            let round = match parse_hex(b"a9993e364706816aba3e25717850c26c9cd0d89d", SHA1_LEN) {
                Ok(p) => p.equals(&d),
                Err(_) => false,
            };
            let bad = parse_hex(b"0g", 1) == Err(HS_EBYTE);
            let len = parse_hex(b"00", 2) == Err(HS_ERANGE);
            let h0 = hexval(b'0') == Ok(0);
            let hf = hexval(b'f') == Ok(15);
            let h_f = hexval(b'F') == Ok(15);
            let hg = hexval(b'g') == Err(HS_EBYTE);
            lower && upper && round && bad && len && h0 && hf && h_f && hg
        },
        "static",
    );

    s.add(
        "HS08-校验行解析",
        {
            let mut ok = true;
            let l0 = match Entry::parse(SAMPLE_LINES[0]) {
                Ok(v) => v,
                Err(_) => {
                    ok = false;
                    Entry::blank()
                }
            };
            if l0.alg() != Algorithm::Sha1 || l0.tagged() || l0.name_text().as_str() != "abc.txt" {
                ok = false;
            }
            let l1 = match Entry::parse(SAMPLE_LINES[1]) {
                Ok(v) => v,
                Err(_) => {
                    ok = false;
                    Entry::blank()
                }
            };
            if l1.alg() != Algorithm::Blake2b512 || l1.tagged() || l1.name_text().as_str() != "beta.bin" {
                ok = false;
            }
            let l2 = match Entry::parse(SAMPLE_LINES[2]) {
                Ok(v) => v,
                Err(_) => {
                    ok = false;
                    Entry::blank()
                }
            };
            if l2.alg() != Algorithm::Blake2b256 || l2.tagged() || l2.name_text().as_str() != "gamma.txt" {
                ok = false;
            }
            let l3 = match Entry::parse(SAMPLE_LINES[3]) {
                Ok(v) => v,
                Err(_) => {
                    ok = false;
                    Entry::blank()
                }
            };
            if l3.alg() != Algorithm::Sha1 || !l3.tagged() || l3.name_text().as_str() != "delta.txt" {
                ok = false;
            }
            let m = match Algorithm::parse(b"BLAKE2b-512") {
                Ok(v) => v,
                Err(_) => Algorithm::Sha1,
            };
            ok && m == Algorithm::Blake2b512
        },
        "static",
    );

    s.add(
        "HS09-解析闸门",
        {
            let a = Entry::parse(b"zz93e364706816aba3e25717850c26c9cd0d89d  x") == Err(HS_EBYTE);
            let b = Entry::parse(b"abc") == Err(HS_ESHORT);
            let c = Entry::parse(b"md5 (x) = 00") == Err(HS_EALG);
            let d = Algorithm::from_hexlen(88) == Err(HS_ERANGE);
            let e = Entry::parse(b"a9993e364706816aba3e25717850c26c9cd0d89d  ") == Err(HS_ESHORT);
            let f = {
                let mut buf = [0u8; 96];
                let hex = b"a9993e364706816aba3e25717850c26c9cd0d89d";
                let mut i = 0usize;
                while i < 40 {
                    if let (Some(dst), Some(src)) = (buf.get_mut(i), hex.get(i)) {
                        *dst = *src;
                    }
                    i += 1;
                }
                if let Some(dst) = buf.get_mut(40) {
                    *dst = b' ';
                }
                if let Some(dst) = buf.get_mut(41) {
                    *dst = b' ';
                }
                let mut k = 0usize;
                while k < 41 {
                    if let Some(dst) = buf.get_mut(42 + k) {
                        *dst = b'n';
                    }
                    k += 1;
                }
                Entry::parse(buf.get(0..83).unwrap_or(&[])) == Err(HS_ELEN)
            };
            let g = match Entry::parse(b"a9993e364706816aba3e25717850c26c9cd0d89d *x") {
                Ok(v) => !v.tagged() && v.name_text().as_str() == "x",
                Err(_) => false,
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "HS10-校验判定",
        {
            let e = match Entry::from_content(Algorithm::Sha1, b"hello world\n", b"h.txt", false) {
                Ok(v) => v,
                Err(_) => Entry::blank(),
            };
            let good = e.matches(b"hello world\n") == Ok(true);
            let bad = e.matches(b"hello world!") == Ok(false);
            let vg = verdict(&e, b"hello world\n") == Verdict::Good;
            let vb = verdict(&e, b"hello world!") == Verdict::Bad;
            let e2 = match Entry::from_content(Algorithm::Blake2b256, b"abc", b"b.txt", true) {
                Ok(v) => v,
                Err(_) => Entry::blank(),
            };
            let good2 = e2.matches(b"abc") == Ok(true);
            let crossed = {
                let d = match Algorithm::Sha1.digest(b"abc") {
                    Ok(v) => v,
                    Err(_) => Digest::blank(),
                };
                Entry::make(Algorithm::Blake2b256, d, b"x", false) == Err(HS_ERANGE)
            };
            let neq = Digest::from_bytes(b"ab") != Digest::from_bytes(b"abc");
            good && bad && vg && vb && good2 && crossed && neq
        },
        "static",
    );

    s.add(
        "HS11-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES;
            let b = l.ok_count() == 8;
            let c = l.err_count() == 6;
            let d = l.audit() == Ok(());
            let e = l.sample(0).map(|s| s.value).unwrap_or(0) == 169;
            let f = l.sample(13).map(|s| s.code).unwrap_or(0) == HS_ESTATE;
            let g = {
                let mut m = Ledger::new();
                let s = Sample { slot: 3, op: 0, ok: false, code: 0, value: 0 };
                m.register(s) == Err(HS_ESLOT)
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "HS12-冻结重放",
        {
            let mut l = build_ledger();
            let same = l.seal() == build_ledger().seal();
            let root = match l.freeze() {
                Ok(v) => v,
                Err(_) => 0,
            };
            let frozen = l.is_frozen() && root == l.seal();
            let replay = match l.replay() {
                Ok(v) => v == root,
                Err(_) => false,
            };
            let probe = Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 };
            let rejected = l.register(probe) == Err(HS_ESLOT) && l.rejects() == 1;
            let again = l.freeze() == Err(HS_EBAL) && l.rejects() == 2;
            same && frozen && replay && rejected && again
        },
        "static",
    );

    s.add(
        "HS13-冻结重放四注入",
        {
            let four = inject_four();
            four[0] && four[1] && four[2] && four[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_known_vectors() {
        assert!(Algorithm::Sha1
            .digest(b"abc")
            .map(|d| d.hex_is(b"a9993e364706816aba3e25717850c26c9cd0d89d"))
            .unwrap_or(false));
        assert!(Algorithm::Sha1
            .digest(b"")
            .map(|d| d.hex_is(b"da39a3ee5e6b4b0d3255bfef95601890afd80709"))
            .unwrap_or(false));
    }

    #[test]
    fn blake2b_known_vectors() {
        assert!(Algorithm::Blake2b512
            .digest(b"abc")
            .map(|d| d.hex_is(b"ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"))
            .unwrap_or(false));
        assert!(Algorithm::Blake2b256
            .digest(b"")
            .map(|d| d.hex_is(b"0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8"))
            .unwrap_or(false));
    }

    #[test]
    fn streaming_matches_one_shot() {
        assert!(streaming_ok());
    }

    #[test]
    fn vectors_tables() {
        assert!(table_ok(Algorithm::Sha1, &SHA1_TABLE));
        assert!(table_ok(Algorithm::Blake2b512, &B2B512_TABLE));
        assert!(table_ok(Algorithm::Blake2b256, &B2B256_TABLE));
    }

    #[test]
    fn parse_forms() {
        assert!(parse_ok());
        let e = Entry::parse(SAMPLE_LINES[3]).expect("tag line");
        assert!(e.tagged());
        assert_eq!(e.name_text().as_str(), "delta.txt");
    }

    #[test]
    fn verdicts() {
        let e = Entry::from_content(Algorithm::Sha1, b"abc", b"a", false).expect("entry");
        assert_eq!(verdict(&e, b"abc"), Verdict::Good);
        assert_eq!(verdict(&e, b"abd"), Verdict::Bad);
        assert_eq!(Verdict::Good.text(), "OK");
    }

    #[test]
    fn ledger_is_audited() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
    }

    #[test]
    fn freeze_and_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.register(Sample::blank()), Err(HS_ESLOT));
    }

    #[test]
    fn render_report() {
        let b = sample_batch().expect("batch");
        let text = render(b.as_slice()).expect("render");
        assert_eq!(text.as_str().matches('\n').count(), 6);
        assert!(text.as_str().starts_with("unx11538 hash report"));
        assert!(text.as_str().ends_with("total 4\n"));
    }

    #[test]
    fn entries_round_trip() {
        let e = Entry::from_content(Algorithm::Sha1, b"abc", b"abc.txt", false).expect("entry");
        let line = e.render();
        let back = Entry::parse(line.as_str().as_bytes()).expect("parse");
        assert_eq!(back.alg(), Algorithm::Sha1);
        assert_eq!(back.name_text().as_str(), "abc.txt");
        assert!(back.matches(b"abc").unwrap_or(false));
        let tag = e.tag_line();
        let back2 = Entry::parse(tag.as_str().as_bytes()).expect("parse tag");
        assert!(back2.tagged());
    }

    #[test]
    fn gates_reject_bad_input() {
        assert_eq!(Entry::parse(b""), Err(HS_ESHORT));
        assert_eq!(Entry::parse(b"abc"), Err(HS_ESHORT));
        assert_eq!(Entry::parse(b"md5 (x) = 00"), Err(HS_EALG));
        assert_eq!(Algorithm::from_hexlen(42), Err(HS_ERANGE));
        assert_eq!(Blake2b::new(0).err(), Some(HS_ERANGE));
        assert_eq!(Blake2b::new(65).err(), Some(HS_ERANGE));
    }

    #[test]
    fn state_conflicts() {
        let mut h = Sha1::new();
        assert!(h.finish().is_ok());
        assert_eq!(h.finish(), Err(HS_ESTATE));
        assert_eq!(h.update(b"x"), Err(HS_ESTATE));
        let mut b = Blake2b::new(32).expect("blake");
        assert!(b.finish().is_ok());
        assert_eq!(b.finish(), Err(HS_ESTATE));
    }

    #[test]
    fn catalog_duplicates() {
        let mut c = Catalog::new();
        assert_eq!(c.register(Algorithm::Sha1), Ok(1));
        assert_eq!(c.register(Algorithm::Sha1), Err(HS_EDUP));
        assert_eq!(c.register(Algorithm::Blake2b256), Ok(2));
        assert_eq!(c.register(Algorithm::Blake2b512), Ok(3));
        assert!(c.complete());
        assert_eq!(c.register(Algorithm::Sha1), Err(HS_EDUP));
        assert_eq!(c.rejects(), 2);
    }

    #[test]
    fn batch_duplicates() {
        let mut b = Batch::new();
        let e = Entry::from_content(Algorithm::Sha1, b"abc", b"same", false).expect("entry");
        assert_eq!(b.register(e), Ok(1));
        assert_eq!(b.register(e), Err(HS_EDUP));
        assert_eq!(b.summary().total, 1);
    }

    #[test]
    fn lengths_and_buffers() {
        let mut h = Sha1::new();
        assert_eq!(h.buffered(), 0);
        assert!(h.update(&[0u8; 65]).is_ok());
        assert_eq!(h.buffered(), 1);
        assert_eq!(h.total(), 65);
        let mut b = Blake2b::new(64).expect("blake");
        assert!(b.update(&[7u8; 129]).is_ok());
        assert_eq!(b.outlen(), 64);
    }

    #[test]
    fn four_injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11538_checks();
        assert!(s.all_passed());
    }
}
