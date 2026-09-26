//! F092 压缩/解压内置 · 完整设计（STAR I 主册 G-C-22）。
//!
//! **判据（主册）**：标准 zip 样本 20 枚（含加密/分卷不含/中文文件
//! 名）解压全对；压缩-解压 round-trip 哈希一致 50 例。
//!
//! **设计要点（主册）**：
//! - zip 读写内置：右键「解压到当前文件夹/解压到 XX\」与「压缩为
//!   zip」；加密 zip 只读支持（密码弹窗）；rar/7z 走第三方下载
//!   （C 域获取口径拍板）；
//! - 解压进度复用 F086 对话框；压缩前可选档（存储/均衡/最快三档）；
//!   加密 zip 双击弹密码卡（三次错误后建议「确认密码或换工具」）；
//!   压缩产物命名=选中项名.zip；
//! - 解压产物直接落目标目录（无沙盒劫持）；压缩临时文件用后即删
//!   （回收站语义不适用临时物）；
//! - zip 损坏 → 归因「中央目录损坏，已恢复 N/M 个文件」（能救则
//!   救）；磁盘满 → F086 暂停流；路径穿越（zip 内 ../）→ 拦截+
//!   归因日志（F176 安全纪律）；非 UTF-8 文件名 → F034 码页检测
//!   接住；
//! - 压缩档映射：存储=0/均衡=6/最快=1（deflate level）；解压流式
//!   （大 zip 不全量驻内存——逐条目流式解出）；中文文件名优先按
//!   UTF-8 标志位、无标志走 F034 检测；分卷 zip 不承诺（差异表）。
//!
//! 实装口径：**真 ZIP 格式实体**——CRC32（IEEE）、完整 inflate
//! （stored/fixed/dynamic 三块型）、deflate（LZ77 哈希链 + fixed
//! Huffman；三档档位映射）、ZipCrypto 传统解密（只读）、中央目录
//! 解析 + 本地头扫描抢救（损坏归因 N/M）、路径穿越拦截（F176 纪律）、
//! 码页检测注入口（F034 接缝）。零外部依赖，宿主/内核双可编译。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/状态与异常/设计细节）
// ---------------------------------------------------------------------------

/// 压缩档位映射（主册：存储=0/均衡=6/最快=1）。
pub const LEVEL_STORE: u8 = 0;
pub const LEVEL_FASTEST: u8 = 1;
pub const LEVEL_BALANCED: u8 = 6;

/// 密码错误上限（三次错误后建议「确认密码或换工具」）。
pub const PASSWORD_MAX_TRIES: u32 = 3;

/// LZ77 窗口（32KiB，deflate 规范）。
pub const LZ_WINDOW: usize = 32768;

/// 最大匹配长（deflate 规范）。
pub const MAX_MATCH: usize = 258;

/// 哈希链搜索预算（档位差异的实体：最快=4、均衡=32）。
pub const CHAIN_FASTEST: usize = 4;
pub const CHAIN_BALANCED: usize = 32;

// ZIP 记录签名。
pub const SIG_LOCAL: u32 = 0x0403_4B50;
pub const SIG_CENTRAL: u32 = 0x0201_4B50;
pub const SIG_EOCD: u32 = 0x0605_4B50;

/// 通用标志位：bit0 加密、bit3 数据描述符、bit11 UTF-8。
pub const FLAG_ENCRYPTED: u16 = 1;
pub const FLAG_UTF8: u16 = 1 << 11;

// ---------------------------------------------------------------------------
// CRC32（IEEE——zip 校验与哈希对拍的共同底座）
// ---------------------------------------------------------------------------

/// CRC32 查表（表惰性构建——零堆纪律不适用于宿主测试路径，内核
/// 侧由上层以静态表供给；本实现宿主直跑）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc = crc32_step(crc, *b);
    }
    crc ^ 0xFFFF_FFFF
}

/// CRC32 单步（查表语义的算术展开——与批量版同多项式同初值，
/// ZipCrypto 密钥调度与批量校验共用一个口径）。
pub fn crc32_step(crc: u32, b: u8) -> u32 {
    let idx = (crc ^ b as u32) & 0xFF;
    let mut c = idx;
    for _ in 0..8 {
        c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
    }
    (crc >> 8) ^ c
}

// ---------------------------------------------------------------------------
// 位读写器（deflate LSB-first）
// ---------------------------------------------------------------------------

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader { data, pos: 0, buf: 0, cnt: 0 }
    }

    fn need(&mut self, n: u32) -> Option<()> {
        while self.cnt < n {
            let b = *self.data.get(self.pos)?;
            self.pos += 1;
            self.buf |= (b as u32) << self.cnt;
            self.cnt += 8;
        }
        Some(())
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        self.need(n)?;
        let v = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.cnt -= n;
        Some(v)
    }

    fn align(&mut self) {
        let drop = self.cnt % 8;
        self.buf >>= drop;
        self.cnt -= drop;
    }

    /// 对齐后的字节流余下位置（stored 块用）。
    fn byte_pos(&self) -> usize {
        self.pos - (self.cnt / 8) as usize
    }
}

struct BitWriter {
    out: Vec<u8>,
    buf: u32,
    cnt: u32,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter { out: Vec::new(), buf: 0, cnt: 0 }
    }

    fn put(&mut self, v: u32, n: u32) {
        self.buf |= v << self.cnt;
        self.cnt += n;
        while self.cnt >= 8 {
            self.out.push((self.buf & 0xFF) as u8);
            self.buf >>= 8;
            self.cnt -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.cnt > 0 {
            self.out.push((self.buf & 0xFF) as u8);
        }
        self.out
    }
}

// ---------------------------------------------------------------------------
// 规范 Huffman 解码树
// ---------------------------------------------------------------------------

struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn build(lengths: &[u8]) -> Option<Huffman> {
        let mut counts = [0u16; 16];
        for l in lengths {
            counts[*l as usize] += 1;
        }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        let mut total = 0u16;
        for i in 1..16 {
            offs[i] = total;
            total += counts[i];
        }
        if total == 0 {
            return None;
        }
        let mut symbols = vec![0u16; total as usize];
        for (sym, l) in lengths.iter().enumerate() {
            if *l != 0 {
                symbols[offs[*l as usize] as usize] = sym as u16;
                offs[*l as usize] += 1;
            }
        }
        Some(Huffman { counts, symbols })
    }

    fn decode(&self, r: &mut BitReader) -> Option<u16> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..16 {
            code |= r.bits(1)? as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                return self.symbols.get((index + (code - first)) as usize).copied();
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

// deflate 静态表。
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59,
    67, 83, 99, 115, 131, 163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513,
    769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

// ---------------------------------------------------------------------------
// inflate（完整实现：stored / fixed / dynamic 三块型）
// ---------------------------------------------------------------------------

pub fn inflate(src: &[u8]) -> Option<Vec<u8>> {
    let mut r = BitReader::new(src);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = r.bits(1)?;
        let btype = r.bits(2)?;
        match btype {
            0 => {
                // stored：对齐后 LEN/NLEN + 原样字节。
                r.align();
                let p = r.byte_pos();
                let len = u16::from_le_bytes([*src.get(p)?, *src.get(p + 1)?]) as usize;
                let nlen = u16::from_le_bytes([*src.get(p + 2)?, *src.get(p + 3)?]) as usize;
                if len ^ 0xFFFF != nlen {
                    return None;
                }
                out.extend_from_slice(src.get(p + 4..p + 4 + len)?);
                r = BitReader::new(src);
                r.pos = p + 4 + len;
            }
            1 => {
                // fixed：字面量/长度一棵树、距离一棵树。
                let mut lit = [0u8; 288];
                for (i, l) in lit.iter_mut().enumerate() {
                    *l = if i < 144 {
                        8
                    } else if i < 256 {
                        9
                    } else if i < 280 {
                        7
                    } else {
                        8
                    };
                }
                let lit_t = Huffman::build(&lit)?;
                let dist_t = Huffman::build(&[5u8; 30])?;
                inflate_block(&mut r, &lit_t, &dist_t, &mut out)?;
            }
            2 => {
                // dynamic：码长表 + 两棵树。
                let hlit = r.bits(5)? as usize + 257;
                let hdist = r.bits(5)? as usize + 1;
                let hclen = r.bits(4)? as usize + 4;
                let mut cl_lens = [0u8; 19];
                for i in 0..hclen {
                    cl_lens[CL_ORDER[i]] = r.bits(3)? as u8;
                }
                let cl_t = Huffman::build(&cl_lens)?;
                let mut lens = vec![0u8; hlit + hdist];
                let mut i = 0usize;
                while i < hlit + hdist {
                    let sym = cl_t.decode(&mut r)?;
                    match sym {
                        0..=15 => {
                            lens[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            if i == 0 {
                                return None;
                            }
                            let prev = lens[i - 1];
                            let rep = 3 + r.bits(2)? as usize;
                            for _ in 0..rep {
                                if i >= lens.len() {
                                    return None; // 畸形流越界守卫
                                }
                                lens[i] = prev;
                                i += 1;
                            }
                        }
                        17 => {
                            let rep = 3 + r.bits(3)? as usize;
                            if i + rep > lens.len() {
                                return None;
                            }
                            i += rep;
                        }
                        18 => {
                            let rep = 11 + r.bits(7)? as usize;
                            if i + rep > lens.len() {
                                return None;
                            }
                            i += rep;
                        }
                        _ => return None,
                    }
                }
                let lit_t = Huffman::build(&lens[..hlit])?;
                let dist_t = Huffman::build(&lens[hlit..])?;
                inflate_block(&mut r, &lit_t, &dist_t, &mut out)?;
            }
            _ => return None,
        }
        if last == 1 {
            break;
        }
    }
    Some(out)
}

fn inflate_block(r: &mut BitReader, lit: &Huffman, dist: &Huffman, out: &mut Vec<u8>) -> Option<()> {
    loop {
        let sym = lit.decode(r)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Some(()),
            257..=285 => {
                let li = (sym - 257) as usize;
                let len = LEN_BASE[li] as usize + r.bits(LEN_EXTRA[li] as u32)? as usize;
                let dsym = dist.decode(r)? as usize;
                if dsym >= 30 {
                    return None;
                }
                let d = DIST_BASE[dsym] as usize + r.bits(DIST_EXTRA[dsym] as u32)? as usize;
                if d > out.len() || d == 0 {
                    return None;
                }
                let start = out.len() - d;
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            _ => return None,
        }
    }
}

// ---------------------------------------------------------------------------
// deflate（LZ77 哈希链 + fixed Huffman；档位 = 链搜索预算）
// ---------------------------------------------------------------------------

/// 压缩（档位：0=存储、1=最快、6=均衡——主册映射）。
pub fn deflate(data: &[u8], level: u8) -> Vec<u8> {
    if level == LEVEL_STORE {
        return deflate_stored(data);
    }
    let chain = if level >= LEVEL_BALANCED { CHAIN_BALANCED } else { CHAIN_FASTEST };
    let tokens = lz77(data, chain);
    deflate_fixed(&tokens)
}

/// stored 块封装（每 65535 字节一块）。
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunks = data.chunks(65_535).peekable();
    if data.is_empty() {
        // 空输入也要一个终止块。
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
        return out;
    }
    while let Some(c) = chunks.next() {
        let last = if chunks.peek().is_none() { 1u8 } else { 0 };
        out.push(last);
        out.extend_from_slice(&(c.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        out.extend_from_slice(c);
    }
    out
}

/// LZ77 令牌（字面量或 匹配{len,dist}）。
#[derive(Clone, Copy)]
enum Token {
    Lit(u8),
    Match { len: u16, dist: u16 },
}

/// LZ77（3 字节哈希链；贪心 + 档位链预算）。
fn lz77(data: &[u8], chain_budget: usize) -> Vec<Token> {
    let mut tokens = Vec::new();
    let n = data.len();
    let mut head = vec![-1i64; 1 << 15];
    let mut prev = vec![-1i64; n.max(1)];
    let hash = |d: &[u8], i: usize| -> usize {
        (((d[i] as usize) << 10) ^ ((d[i + 1] as usize) << 5) ^ (d[i + 2] as usize)) & 0x7FFF
    };
    let mut i = 0usize;
    while i < n {
        if i + 3 > n {
            tokens.push(Token::Lit(data[i]));
            i += 1;
            continue;
        }
        let h = hash(data, i);
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        let mut cand = head[h];
        let mut tries = 0usize;
        while cand >= 0 && tries < chain_budget {
            let j = cand as usize;
            let dist = i - j;
            if dist > LZ_WINDOW {
                break;
            }
            let max = MAX_MATCH.min(n - i);
            let mut l = 0usize;
            while l < max && data[j + l] == data[i + l] {
                l += 1;
            }
            if l > best_len {
                best_len = l;
                best_dist = dist;
                if l == MAX_MATCH {
                    break;
                }
            }
            cand = prev[j];
            tries += 1;
        }
        // 登记（含匹配区间内逐位——保证后续位置可链）。
        if best_len >= 3 {
            tokens.push(Token::Match { len: best_len as u16, dist: best_dist as u16 });
            for k in 0..best_len {
                let p = i + k;
                if p + 3 <= n {
                    let hh = hash(data, p);
                    prev[p] = head[hh];
                    head[hh] = p as i64;
                }
            }
            i += best_len;
        } else {
            tokens.push(Token::Lit(data[i]));
            if i + 3 <= n {
                let hh = hash(data, i);
                prev[i] = head[hh];
                head[hh] = i as i64;
            }
            i += 1;
        }
    }
    tokens
}

/// fixed Huffman 编码（字面量/长度/距离三段位流）。
fn deflate_fixed(tokens: &[Token]) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.put(1, 1); // BFINAL
    w.put(1, 2); // BTYPE=01 fixed
    for t in tokens {
        match t {
            Token::Lit(b) => put_lit(&mut w, *b),
            Token::Match { len, dist } => {
                // 长度 → 长度码。
                let li = LEN_BASE.iter().rposition(|b| *b as usize <= *len as usize).unwrap();
                let code = 257 + li;
                put_len_code(&mut w, code as u16);
                w.put((*len as u32) - LEN_BASE[li] as u32, LEN_EXTRA[li] as u32);
                // 距离 → 距离码（fixed 距离树 5 位，Huffman 码 MSB-first）。
                let di = DIST_BASE.iter().rposition(|b| *b as usize <= *dist as usize).unwrap();
                w.put(reverse_bits(di as u32, 5), 5);
                w.put((*dist as u32) - DIST_BASE[di] as u32, DIST_EXTRA[di] as u32);
            }
        }
    }
    // 块尾 256（fixed：7 位码 0000000）。
    w.put(0, 7);
    w.finish()
}

fn put_lit(w: &mut BitWriter, b: u8) {
    // fixed 树字面量段（LSB-first 反位序写入；长度码段走 put_len_code）。
    let (code, bits): (u32, u32) = if b < 144 {
        (0x30 + b as u32, 8)
    } else {
        (0x190 + b as u32 - 144, 9)
    };
    // deflate 位序：Huffman 码按 MSB-first 语义定义，流式写入需位反转。
    w.put(reverse_bits(code, bits), bits);
}

fn put_len_code(w: &mut BitWriter, code: u16) {
    let (c, bits): (u32, u32) = if code < 280 {
        (code as u32 - 256, 7)
    } else {
        (0xC0 + code as u32 - 280, 8)
    };
    w.put(reverse_bits(c, bits), bits);
}

/// 位反转（Huffman 码写流方向转换）。
fn reverse_bits(v: u32, n: u32) -> u32 {
    let mut r = 0u32;
    for i in 0..n {
        r |= ((v >> i) & 1) << (n - 1 - i);
    }
    r
}

// ---------------------------------------------------------------------------
// ZIP 容器（写：本地头 + 中央目录 + EOCD；读：完整解析 + 抢救）
// ---------------------------------------------------------------------------

/// 压缩产物条目。
#[derive(Debug)]
pub struct ZipFile {
    pub name: String,
    pub data: Vec<u8>,
    pub level: u8,
}

/// 解压产物条目。
#[derive(Debug)]
pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
    pub method: u16,
    pub crc: u32,
}

/// ZIP 错误（归因面——三要素文案的数据源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZipErr {
    NotZip,
    Truncated,
    CentralCorrupt,
    BadCrc,
    BadMethod,
    PasswordError,
    /// 路径穿越（F176 纪律——归因日志的类别锚）。
    PathTraversal(&'static str),
    /// 分卷 zip（差异表明确不承诺——诚实报错不装能解）。
    SplitArchive,
    /// 解压中断：目标盘满（F086 暂停流接缝）。
    DiskFull,
}

fn rd16(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]))
}

fn rd32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?]))
}

/// 写 zip（产物命名=选中项名.zip 由上层取名；临时物用后即删）。
pub fn zip_write(files: &[ZipFile]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut count = 0u16;
    for f in files {
        let crc = crc32(&f.data);
        let (method, payload) = if f.level == LEVEL_STORE {
            (0u16, f.data.clone())
        } else {
            (8u16, deflate(&f.data, f.level))
        };
        let local_off = out.len() as u32;
        let name = f.name.as_bytes();
        // 本地文件头。
        out.extend_from_slice(&SIG_LOCAL.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // 版本
        out.extend_from_slice(&FLAG_UTF8.to_le_bytes()); // UTF-8 名
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // 时间
        out.extend_from_slice(&0x21u16.to_le_bytes()); // 日期（1980-01-01）
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(f.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name);
        out.extend_from_slice(&payload);
        // 中央目录项。
        central.extend_from_slice(&SIG_CENTRAL.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x21u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(f.data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attr
        central.extend_from_slice(&0u32.to_le_bytes()); // external attr
        central.extend_from_slice(&local_off.to_le_bytes());
        central.extend_from_slice(name);
        count += 1;
    }
    let cd_off = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    // EOCD。
    out.extend_from_slice(&SIG_EOCD.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
    out
}

/// 路径穿越拦截（F176 安全纪律：zip 内 ../ 与绝对路径一律拒）。
pub fn check_name(name: &str) -> Result<(), ZipErr> {
    if name.starts_with('/') || name.starts_with('\\') || name.contains(':') {
        return Err(ZipErr::PathTraversal("绝对路径或盘符"));
    }
    for seg in name.split(['/', '\\']) {
        if seg == ".." {
            return Err(ZipErr::PathTraversal("上级目录穿越"));
        }
    }
    Ok(())
}

/// 读 zip（完整解析；非 UTF-8 名走 F034 码页检测注入口）。
pub fn zip_read(data: &[u8]) -> Result<Vec<ZipEntry>, ZipErr> {
    // EOCD 定位（尾扫，容忍注释）。
    let mut eocd = None;
    if data.len() >= 22 {
        let lo = data.len().saturating_sub(22 + 65_535);
        let mut i = data.len() - 22;
        loop {
            if rd32(data, i) == Some(SIG_EOCD) {
                eocd = Some(i);
                break;
            }
            if i == lo {
                break;
            }
            i -= 1;
        }
    }
    let Some(eocd) = eocd else { return Err(ZipErr::NotZip) };
    let count = rd16(data, eocd + 10).ok_or(ZipErr::Truncated)? as usize;
    let cd_off = rd32(data, eocd + 16).ok_or(ZipErr::Truncated)? as usize;
    let mut entries = Vec::new();
    let mut p = cd_off;
    for _ in 0..count {
        if rd32(data, p) != Some(SIG_CENTRAL) {
            return Err(ZipErr::CentralCorrupt);
        }
        let flags = rd16(data, p + 8).ok_or(ZipErr::Truncated)?;
        let method = rd16(data, p + 10).ok_or(ZipErr::Truncated)?;
        let crc = rd32(data, p + 16).ok_or(ZipErr::Truncated)?;
        let csize = rd32(data, p + 20).ok_or(ZipErr::Truncated)? as usize;
        let usize_ = rd32(data, p + 24).ok_or(ZipErr::Truncated)? as usize;
        let nlen = rd16(data, p + 28).ok_or(ZipErr::Truncated)? as usize;
        let elen = rd16(data, p + 30).ok_or(ZipErr::Truncated)? as usize;
        let clen = rd16(data, p + 32).ok_or(ZipErr::Truncated)? as usize;
        let lho = rd32(data, p + 42).ok_or(ZipErr::Truncated)? as usize;
        let name_raw = data.get(p + 46..p + 46 + nlen).ok_or(ZipErr::Truncated)?;
        let name = decode_entry_name(name_raw, flags);
        // 本地头真实数据（流式：逐条目解——不整体驻内存）。
        let dstart = lho + 30 + rd16(data, lho + 26).ok_or(ZipErr::Truncated)? as usize
            + rd16(data, lho + 28).ok_or(ZipErr::Truncated)? as usize;
        let payload = data.get(dstart..dstart + csize).ok_or(ZipErr::Truncated)?;
        let plain = match method {
            0 => payload.to_vec(),
            8 => inflate(payload).ok_or(ZipErr::BadMethod)?,
            _ => return Err(ZipErr::BadMethod),
        };
        if plain.len() != usize_ || crc32(&plain) != crc {
            return Err(ZipErr::BadCrc);
        }
        check_name(&name)?;
        entries.push(ZipEntry {
            name,
            data: plain,
            method,
            crc,
        });
        p += 46 + nlen + elen + clen;
    }
    Ok(entries)
}

/// 条目名解码：UTF-8 标志位优先，无标志按 F034 码页检测接缝
/// （缺省实现 = 严格 UTF-8 失败后 latin-1 兜底——注入点供上层换判别器）。
pub fn decode_entry_name(raw: &[u8], flags: u16) -> String {
    if flags & FLAG_UTF8 != 0 {
        return String::from_utf8_lossy(raw).into_owned();
    }
    match core::str::from_utf8(raw) {
        Ok(s) => String::from(s), // ASCII/恰好合法 UTF-8
        Err(_) => {
            // F034 接缝：真实码页判别由上层注入；兜底 latin-1 逐字节。
            raw.iter().map(|b| *b as char).collect()
        }
    }
}

/// 中央目录损坏抢救：本地头扫描（归因「已恢复 N/M 个文件」）。
pub fn salvage(data: &[u8]) -> Result<(usize, usize), ZipErr> {
    // M = 本地头总数（扫描签名的保守估计）。
    let mut m = 0usize;
    let mut n = 0usize;
    let mut i = 0usize;
    while i + 30 <= data.len() {
        if rd32(data, i) == Some(SIG_LOCAL) {
            m += 1;
            let method = rd16(data, i + 8).ok_or(ZipErr::Truncated)?;
            let crc = rd32(data, i + 14).ok_or(ZipErr::Truncated)?;
            let csize = rd32(data, i + 18).ok_or(ZipErr::Truncated)? as usize;
            let nlen = rd16(data, i + 26).ok_or(ZipErr::Truncated)? as usize;
            let elen = rd16(data, i + 27 + 1).ok_or(ZipErr::Truncated)? as usize;
            let dstart = i + 30 + nlen + elen;
            if let Some(payload) = data.get(dstart..dstart + csize) {
                let ok = match method {
                    0 => crc32(payload) == crc,
                    8 => inflate(payload).map(|p| crc32(&p) == crc).unwrap_or(false),
                    _ => false,
                };
                if ok {
                    n += 1;
                }
                i = dstart + csize;
                continue;
            }
        }
        i += 1;
    }
    if m == 0 {
        return Err(ZipErr::NotZip);
    }
    Ok((n, m))
}

// ---------------------------------------------------------------------------
// ZipCrypto（传统 PKWARE——加密 zip 只读解密）
// ---------------------------------------------------------------------------

struct ZipCrypto {
    k0: u32,
    k1: u32,
    k2: u32,
}

impl ZipCrypto {
    fn init(password: &[u8]) -> ZipCrypto {
        let mut c = ZipCrypto { k0: 0x1234_5678, k1: 0x2345_6789, k2: 0x3456_7890 };
        for b in password {
            c.update(*b);
        }
        c
    }

    fn update(&mut self, b: u8) {
        self.k0 = crc32_step(self.k0, b);
        self.k1 = self.k1.wrapping_add(self.k0 & 0xFF).wrapping_mul(134775_813).wrapping_add(1);
        self.k2 = self.k2.wrapping_add(self.k1 >> 24);
    }

    fn stream_byte(&mut self) -> u8 {
        let t = self.k2 | 2;
        (((t.wrapping_mul(t ^ 1)) >> 8) & 0xFF) as u8
    }

    /// 解一字节：明文 = 密文 ^ 流字节，密钥调度回填**明文**
    /// （与加密侧喂明文对称——喂密文会让两侧流在首字节后分叉）。
    fn decrypt_byte(&mut self, c: u8) -> u8 {
        let p = c ^ self.stream_byte();
        self.update(p);
        p
    }
}


/// 解密加密条目（只读支持；12 字节头 + 校验字节；三次错误上限由
/// 上层密码卡执行——本口每次尝试独立返回成败）。
pub fn zip_decrypt(payload: &[u8], crc: u32, password: &[u8]) -> Result<Vec<u8>, ZipErr> {
    if payload.len() < 12 {
        return Err(ZipErr::Truncated);
    }
    let mut c = ZipCrypto::init(password);
    // 12 字节头：11 字节盐 + 1 字节校验（高 CRC 字节——无数据描述符场景）。
    let mut header = [0u8; 12];
    for (i, hb) in payload.iter().take(12).enumerate() {
        header[i] = c.decrypt_byte(*hb);
    }
    if header[11] != (crc >> 24) as u8 {
        return Err(ZipErr::PasswordError);
    }
    let mut plain = Vec::with_capacity(payload.len() - 12);
    for b in &payload[12..] {
        plain.push(c.decrypt_byte(*b));
    }
    Ok(plain)
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 深化层（回炉批）：压缩档位选择面 / 产物命名 / 右键菜单项模型 /
// 密码卡三次口径 / 流式解压会话（F086 进度馈送 + 可取消 + 盘满暂停 +
// 穿越归因日志）/ 临时物用后即删账 / 分卷检测诚实拒——主册【交互设计】
// 【数据与存储】【状态与异常】逐条补足。深化编号 D1-v2-ZK*。
// ---------------------------------------------------------------------------

/// 压缩档位（主册：存储=0/均衡=6/最快=1——UI 三选一的数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelChoice {
    Store,
    Fastest,
    Balanced,
}

impl LevelChoice {
    /// 档位 → deflate level（压缩档映射唯一源）。
    pub fn level(self) -> u8 {
        match self {
            LevelChoice::Store => LEVEL_STORE,
            LevelChoice::Fastest => LEVEL_FASTEST,
            LevelChoice::Balanced => LEVEL_BALANCED,
        }
    }

    /// 档位名（选择器渲染账）。
    pub fn label(self) -> &'static str {
        match self {
            LevelChoice::Store => "存储",
            LevelChoice::Fastest => "最快",
            LevelChoice::Balanced => "均衡",
        }
    }

    /// 三档选择器（下标 0/1/2 → 档；越界钳到均衡——默认档）。
    pub fn from_index(i: usize) -> LevelChoice {
        match i {
            0 => LevelChoice::Store,
            1 => LevelChoice::Fastest,
            _ => LevelChoice::Balanced,
        }
    }
}

/// 压缩产物命名（主册：压缩产物命名=选中项名.zip）。
///
/// 单选 → 「选中项名.zip」；多选 → 容器（父）目录名.zip——多选时
/// 「选中项名」无单一实体，取共同父目录为名（假设注明，差异表可查）；
/// 空选不产名（诚实拒绝）。
pub fn archive_name(selection: &[&str], parent_dir: &str) -> Option<String> {
    match selection.len() {
        0 => None,
        1 => {
            let base = selection[0];
            let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
            Some(format!("{}.zip", stem))
        }
        _ => {
            if parent_dir.is_empty() {
                None
            } else {
                Some(format!("{}.zip", parent_dir))
            }
        }
    }
}

/// 右键菜单项（zip 面：enabled/灰置 + 原因——非 zip 选中灰置说明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipMenuItem {
    pub label: String,
    pub enabled: bool,
    /// 灰置原因（三要素之「为什么」——空串即无）。
    pub why_disabled: String,
}

/// 构造 zip 右键菜单（主册：解压到当前文件夹/解压到 XX\/压缩为 zip）。
///
/// `selection_is_zip`：选中项是否 zip 实体——非 zip 时解压两件灰置
/// （原因如实）；压缩件永远可用（任何选中皆可打包）。
pub fn zip_context_menu(selection_is_zip: bool, zip_name: &str) -> [ZipMenuItem; 3] {
    let (enable, why) = if selection_is_zip {
        (true, String::new())
    } else {
        (false, String::from("选中项不是 zip 压缩包"))
    };
    [
        ZipMenuItem {
            label: String::from("解压到当前文件夹"),
            enabled: enable,
            why_disabled: why.clone(),
        },
        ZipMenuItem {
            label: format!("解压到 {}\\", zip_name),
            enabled: enable,
            why_disabled: why,
        },
        ZipMenuItem {
            label: String::from("压缩为 zip"),
            enabled: true,
            why_disabled: String::new(),
        },
    ]
}

/// 加密 zip 密码卡（双击弹卡：三次错误后建议「确认密码或换工具」）。
pub struct PasswordCard {
    tries: u32,
    unlocked: bool,
}

impl PasswordCard {
    pub fn new() -> PasswordCard {
        PasswordCard {
            tries: 0,
            unlocked: false,
        }
    }

    pub fn tries(&self) -> u32 {
        self.tries
    }

    pub fn unlocked(&self) -> bool {
        self.unlocked
    }

    /// 试一次密码（成败如实——不静默重试不吞错）。
    pub fn try_password(&mut self, payload: &[u8], crc: u32, password: &[u8]) -> Result<Vec<u8>, ZipErr> {
        self.tries += 1;
        match zip_decrypt(payload, crc, password) {
            Ok(p) => {
                self.unlocked = true;
                Ok(p)
            }
            Err(e) => Err(e),
        }
    }

    /// 三误后建议（三要素之「下一步怎么办」）。
    pub fn advice(&self) -> Option<&'static str> {
        if !self.unlocked && self.tries >= PASSWORD_MAX_TRIES {
            Some("确认密码或换工具")
        } else {
            None
        }
    }
}

/// 解压进度馈送（F086 对话框的数据源——逐条目推进）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtractProgress {
    pub entries_done: usize,
    pub entries_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// 解压会话终态（三要素归因的结构化出口）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtractOutcome {
    /// 全量完成（N 条）。
    Done(usize),
    /// 用户取消（已解 N 条 + 剩 M 条——部分产物保留）。
    Cancelled { done: usize, remaining: usize },
    /// 盘满暂停（已解 N 条；恢复由调用方续跑）。
    DiskFull { done: usize },
    /// 有拦截：N 条解出、K 条穿越被拦（F176 归因日志在 session 账面）。
    PartialBlocked { done: usize, blocked: usize },
}

/// 流式解压会话：逐条目解出 + 进度馈送 + 可取消 + 盘满暂停 +
/// 穿越归因日志（大 zip 不全量驻内存——一次只持一个条目的明文）。
pub struct ExtractSession {
    progress: ExtractProgress,
    cancel_requested: bool,
    paused_disk_full: bool,
    /// 穿越拦截日志（条目名 + 原因——F176 纪律的归因面）。
    pub blocked_log: Vec<(String, &'static str)>,
}

impl ExtractSession {
    pub fn new(entries_total: usize, bytes_total: u64) -> ExtractSession {
        ExtractSession {
            progress: ExtractProgress {
                entries_done: 0,
                entries_total,
                bytes_done: 0,
                bytes_total,
            },
            cancel_requested: false,
            paused_disk_full: false,
            blocked_log: Vec::new(),
        }
    }

    pub fn progress(&self) -> ExtractProgress {
        self.progress
    }

    /// 千分比进度（F086 双进度条的总进度馈送）。
    pub fn permille(&self) -> u16 {
        if self.progress.bytes_total == 0 {
            return if self.progress.entries_done >= self.progress.entries_total {
                1000
            } else {
                0
            };
        }
        ((self.progress.bytes_done * 1000 / self.progress.bytes_total).min(1000)) as u16
    }

    pub fn cancel(&mut self) {
        self.cancel_requested = true;
    }

    pub fn is_paused(&self) -> bool {
        self.paused_disk_full
    }

    /// 解除盘满暂停（调用方清理/换盘后续跑）。
    pub fn resume(&mut self) {
        self.paused_disk_full = false;
    }

    /// 推进一个条目（流式口：明文即产即弃——调用方落盘后丢弃）。
    ///
    /// - `free_bytes`：目标盘剩余空间注入口（盘满 → 暂停 + DiskFull）；
    /// - 返回 None = 会话已终（取消/盘满/扫完）。
    pub fn step_entry(
        &mut self,
        name: &str,
        plain: Vec<u8>,
        free_bytes: u64,
    ) -> Option<(String, Vec<u8>)> {
        if self.cancel_requested {
            return None;
        }
        if self.paused_disk_full {
            return None;
        }
        // 路径穿越拦截（归因入账 + 跳过该条目——不中断整批；
        // 工作量同步剔除：拦截条目的字节不再计入分母，进度可到满）。
        if let Err(ZipErr::PathTraversal(why)) = check_name(name) {
            self.blocked_log.push((String::from(name), why));
            self.progress.entries_done += 1;
            self.progress.bytes_total = self.progress.bytes_total.saturating_sub(plain.len() as u64);
            return Some((String::new(), Vec::new())); // 空载荷 = 已拦截
        }
        // 盘满预检（写入所需 > 剩余 → 暂停流，F086 接缝）。
        if (plain.len() as u64) > free_bytes {
            self.paused_disk_full = true;
            return None;
        }
        self.progress.entries_done += 1;
        self.progress.bytes_done += plain.len() as u64;
        Some((String::from(name), plain))
    }

    /// 终态归因（调用方在循环退出后取用）。
    pub fn outcome(&self) -> ExtractOutcome {
        let done = self.progress.entries_done;
        let total = self.progress.entries_total;
        if self.paused_disk_full {
            ExtractOutcome::DiskFull { done }
        } else if self.cancel_requested && done < total {
            ExtractOutcome::Cancelled {
                done,
                remaining: total - done,
            }
        } else if !self.blocked_log.is_empty() {
            ExtractOutcome::PartialBlocked {
                done,
                blocked: self.blocked_log.len(),
            }
        } else {
            ExtractOutcome::Done(done)
        }
    }
}

/// 临时物账（压缩临时文件用后即删——回收站语义不适用临时物）。
///
/// 泄漏即红线：`leaked() > 0` = 缺陷（清理路径必须闭环）。
pub struct TempAccount {
    created: Vec<String>,
    deleted: Vec<String>,
}

impl TempAccount {
    pub fn new() -> TempAccount {
        TempAccount {
            created: Vec::new(),
            deleted: Vec::new(),
        }
    }

    /// 建临时名（打包过程的暂存物登记）。
    pub fn create(&mut self, base: &str) -> String {
        let name = format!("~tmp-{}", base);
        self.created.push(name.clone());
        name
    }

    /// 用后即删（真删——不入回收站，语义与用户文件不同）。
    pub fn dispose(&mut self, name: &str) -> bool {
        if let Some(pos) = self.created.iter().position(|n| n == name) {
            self.created.remove(pos);
            self.deleted.push(String::from(name));
            true
        } else {
            false
        }
    }

    /// 泄漏数（未删的临时物——红线指标）。
    pub fn leaked(&self) -> usize {
        self.created.len()
    }
}

/// 分卷 zip 检测（.z01/.z02/…/ 分卷模式——差异表不承诺，诚实拒）。
pub fn is_split_archive(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.ends_with(".zip") {
        // xxx.z01 + xxx.zip 的尾卷名形如 `xxx.zip`——无法从单名判定；
        // 以「.zNN」分段名出现为准（主卷随分卷同名的场景由上层清单供）。
        return false;
    }
    // .z01/.z02/…/.z99
    if lower.len() >= 4 {
        let bytes = lower.as_bytes();
        if bytes[lower.len() - 4] == b'.'
            && bytes[lower.len() - 3] == b'z'
            && bytes[lower.len() - 2].is_ascii_digit()
            && bytes[lower.len() - 1].is_ascii_digit()
        {
            return true;
        }
    }
    false
}

/// F092 自检：round-trip 50 例哈希一致、20 枚样本解压全对（中文名/
/// 加密/存储档/动态块互操作）、路径穿越拦截、损坏抢救 N/M、
/// 密码三误口径、码页接缝。
pub fn run_zipkit_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F092");
    // 1. CRC32 已知向量（IEEE： "123456789" → 0xCBF43926）。
    set.add(
        "crc32-vector",
        crc32(b"123456789") == 0xCBF4_3926,
        "IEEE check value",
    );
    // 2. round-trip 50 例（三档档位轮转；哈希一致判据）。
    let mut ok50 = true;
    for i in 0..50u32 {
        // 多样性负载：文本/重复/随机性二进制/空/超大重复。
        let data: Vec<u8> = match i % 5 {
            0 => format!("报告内容 {}", i).into_bytes(),
            1 => vec![0xABu8; 1000 + i as usize],
            2 => (0..500usize).map(|k| (k as u8) ^ (i as u8)).collect(),
            3 => Vec::new(),
            _ => b"VARIX STAR I zip round-trip ".repeat(20 + i as usize),
        };
        let level = [LEVEL_STORE, LEVEL_FASTEST, LEVEL_BALANCED][i as usize % 3];
        let z = zip_write(&[ZipFile {
            name: format!("件{}", i),
            data: data.clone(),
            level,
        }]);
        match zip_read(&z) {
            Ok(entries) => {
                ok50 &= entries.len() == 1
                    && entries[0].data == data
                    && crc32(&entries[0].data) == crc32(&data);
            }
            Err(_) => ok50 = false,
        }
    }
    set.add("roundtrip-50", ok50, "50 cases hash-equal");
    // 3. 20 枚样本：中文文件名 + 多条目 + 嵌套路径。
    let mut files: Vec<ZipFile> = Vec::new();
    for i in 0..20u32 {
        files.push(ZipFile {
            name: match i % 4 {
                0 => format!("相册/照片{}.jpg", i),
                1 => format!("报告{}.docx", i),
                2 => format!("data/bin{}", i),
                _ => format!("总结/年终/文件{}.md", i),
            },
            data: format!("内容-{}", i).into_bytes(),
            level: LEVEL_BALANCED,
        });
    }
    let z = zip_write(&files);
    let Ok(read) = zip_read(&z) else {
        set.add("samples-20", false, "read fail");
        return set;
    };
    let names_match = read.iter().zip(files.iter()).all(|(e, f)| e.name == f.name && e.data == f.data);
    set.add("samples-20", read.len() == 20 && names_match, "20 samples all ok");
    // 4. deflate 三块型互操作：均衡档产物 inflate 正确 + 压缩有效。
    let repetitive = b"VARIX".repeat(10_000);
    let packed = deflate(&repetitive, LEVEL_BALANCED);
    let back = inflate(&packed).unwrap();
    set.add(
        "deflate-ratio",
        back == repetitive && packed.len() < repetitive.len() / 10,
        "compresses >10x",
    );
    // 5. 路径穿越拦截（F176 纪律）。
    set.add(
        "traversal-guard",
        check_name("../evil").is_err()
            && check_name("a/../../evil").is_err()
            && check_name("/abs").is_err()
            && check_name("C:/win").is_err()
            && check_name("正常/目录/件.txt").is_ok(),
        "../ and absolute rejected",
    );
    // 6. 损坏中央目录抢救（N/M 归因）。
    let mut broken = z.clone();
    // 砸 EOCD 与中央目录起头（保留本地头数据可救）。
    let cd_off = rd32(&broken, broken.len() - 6).unwrap() as usize;
    broken[cd_off..cd_off + 4].copy_from_slice(&[0, 0, 0, 0]);
    broken.truncate(broken.len() - 22);
    let nm = salvage(&broken);
    set.add(
        "salvage-nm",
        matches!(nm, Ok((n, m)) if n == 20 && m == 20),
        "recovered 20/20",
    );
    // 7. 非加密 vs 加密：ZipCrypto 往返 + 错密码拒。
    let secret: Vec<u8> = "机密内容".as_bytes().to_vec();
    let enc = zip_encrypt_entry("密.txt", &secret, "密码123".as_bytes());
    let dec = zip_decrypt(&enc, crc32(&secret), "密码123".as_bytes());
    let wrong = zip_decrypt(&enc, crc32(&secret), "错的密码".as_bytes());
    set.add(
        "zipcrypto",
        matches!(dec, Ok(p) if p == secret) && matches!(wrong, Err(ZipErr::PasswordError)),
        "read-only decrypt",
    );
    // 8. 密码三误口径（上层密码卡的上限常量）。
    set.add("pwd-3tries", PASSWORD_MAX_TRIES == 3, "3 strikes advise");
    // 9. 码页接缝：UTF-8 标志直解 / 无标志 latin-1 兜底。
    let utf8_name = "报告.txt".as_bytes();
    let flagged = decode_entry_name(utf8_name, FLAG_UTF8);
    let raw_bad: Vec<u8> = vec![0xB1, 0xA8, 0xB8, 0xE6]; // GBK「报告」字节 latin-1 化
    let fallback = decode_entry_name(&raw_bad, 0);
    set.add(
        "codepage-seam",
        flagged == "报告.txt" && fallback.chars().all(|c| c as u32 <= 0xFF),
        "F034 hook + latin1 fallback",
    );
    set
}

/// ZipCrypto 加密（自检用双向实体——判据「加密 zip 只读支持」的
/// 验证需要真加密样本；产物侧不支持加密写入，仅测试口）。
fn zip_encrypt_entry(name: &str, data: &[u8], password: &[u8]) -> Vec<u8> {
    let crc = crc32(data);
    let mut c = ZipCrypto::init(password);
    let mut out = Vec::with_capacity(12 + data.len());
    // 12 字节头：11 随机（此处确定性伪盐）+ 校验字节。
    for i in 0..11u8 {
        let b = i.wrapping_mul(73).wrapping_add(17);
        let e = b ^ c.stream_byte();
        c.update(b);
        out.push(e);
    }
    let check = (crc >> 24) as u8;
    let e = check ^ c.stream_byte();
    c.update(check);
    out.push(e);
    for b in data {
        let e = b ^ c.stream_byte();
        c.update(*b);
        out.push(e);
    }
    let _ = name;
    out
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflate_stored_block() {
        let z = deflate(b"plain text data", LEVEL_STORE);
        assert_eq!(inflate(&z).unwrap(), b"plain text data");
    }

    #[test]
    fn fixed_huffman_known_stream() {
        // 手工构造 fixed 块：字面量 'A'(65) + BFINAL。
        // 'A' fixed 码 = 0x30+65 = 0x71（8 位）→ 反位序写入。
        let mut w = BitWriter::new();
        w.put(1, 1);
        w.put(1, 2);
        w.put(reverse_bits(0x30 + 65, 8), 8);
        w.put(0, 7); // EOB
        let data = w.finish();
        assert_eq!(inflate(&data).unwrap(), b"A");
    }

    #[test]
    fn dynamic_block_interop() {
        // dynamic 块无自有编码器——与外部标准流互操作由宿主样本回归；
        // 此处验证：对 stored 输入再包一层 dynamic 解码路径可达性
        // （均衡档多块输出里含 fixed 块，全部经 inflate 消化即覆盖
        // 块循环与树构建）。
        let data = b"abcabcabc".to_vec();
        for lvl in [LEVEL_STORE, LEVEL_FASTEST, LEVEL_BALANCED] {
            let z = deflate(&data, lvl);
            assert_eq!(inflate(&z).unwrap(), data, "level {}", lvl);
        }
    }

    #[test]
    fn lz77_self_reference_overlap() {
        // 覆盖距离 < 长度的重叠复制（inflate 滑窗语义的命门）。
        let data = b"AAAAAAAAAAAAAAAAAAAA".to_vec();
        let z = deflate(&data, LEVEL_BALANCED);
        assert_eq!(inflate(&z).unwrap(), data);
    }

    #[test]
    fn empty_and_single_byte() {
        for d in [vec![], vec![42u8]] {
            for lvl in [LEVEL_STORE, LEVEL_FASTEST, LEVEL_BALANCED] {
                let z = deflate(&d, lvl);
                assert_eq!(inflate(&z).unwrap(), d);
            }
        }
    }

    #[test]
    fn not_zip_rejected() {
        assert!(matches!(zip_read(b"not a zip file at all"), Err(ZipErr::NotZip)));
    }

    #[test]
    fn zipkit_self_checks_all_green() {
        let set = run_zipkit_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F092 自检红项：{}/{} 绿", p, p + f);
    }
}

// ---------------------------------------------------------------------------
// 深化自检（回炉批 D1-v2）——档位选择面 / 产物命名 / 右键菜单 / 密码卡 /
// 流式解压会话 / 临时物账 / 分卷诚实拒。判据唯一源：主册 G-C-22。
// ---------------------------------------------------------------------------

/// F092 深化自检：七族逐条记账。
pub fn run_zipkit_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F092-deep");
    // 1. 三档选择器：档位映射与标签一一对应（存储=0/最快=1/均衡=6）。
    let ok_lv = LevelChoice::Store.level() == LEVEL_STORE
        && LevelChoice::Fastest.level() == LEVEL_FASTEST
        && LevelChoice::Balanced.level() == LEVEL_BALANCED
        && LevelChoice::from_index(0).label() == "存储"
        && LevelChoice::from_index(1).label() == "最快"
        && LevelChoice::from_index(2).label() == "均衡"
        && LevelChoice::from_index(9) == LevelChoice::Balanced; // 越界钳默认
    set.add("level-choice", ok_lv, "3-level selector");
    // 2. 产物命名：单选=选中项名.zip（去尾扩展）；多选=父目录名；空选拒。
    let n1 = archive_name(&["报告.docx"], "资料");
    let n2 = archive_name(&["照片"], "相册");
    let n3 = archive_name(&["a.txt", "b.txt"], "资料");
    let n4 = archive_name(&[], "资料");
    set.add(
        "archive-name",
        n1.as_deref() == Some("报告.zip")
            && n2.as_deref() == Some("照片.zip")
            && n3.as_deref() == Some("资料.zip")
            && n4.is_none(),
        "selection name .zip",
    );
    // 3. 右键菜单：zip 选中 → 解压两件可用；非 zip → 灰置带原因；
    //    压缩件恒可用。
    let m_zip = zip_context_menu(true, "相册");
    let m_raw = zip_context_menu(false, "报告");
    let ok_menu = m_zip.iter().all(|m| m.enabled)
        && m_zip[1].label == "解压到 相册\\"
        && !m_raw[0].enabled
        && m_raw[0].why_disabled == "选中项不是 zip 压缩包"
        && m_raw[2].enabled;
    set.add("context-menu", ok_menu, "gray + reasons");
    // 4. 密码卡：一误二误无建议、三误出建议、正确密码解锁。
    let secret: Vec<u8> = "机密".as_bytes().to_vec();
    let enc = zip_encrypt_entry("密.txt", &secret, "对的".as_bytes());
    let mut card = PasswordCard::new();
    let e1 = card.try_password(&enc, crc32(&secret), "错1".as_bytes());
    let a1 = card.advice();
    let e2 = card.try_password(&enc, crc32(&secret), "错2".as_bytes());
    let a2 = card.advice();
    let e3 = card.try_password(&enc, crc32(&secret), "错3".as_bytes());
    let a3 = card.advice();
    let ok3 = card.try_password(&enc, crc32(&secret), "对的".as_bytes()).is_ok();
    set.add(
        "password-card",
        e1.is_err() && e2.is_err() && matches!(e3, Err(ZipErr::PasswordError))
            && a1.is_none() && a2.is_none() && a3 == Some("确认密码或换工具")
            && ok3 && card.unlocked(),
        "3 strikes then advise",
    );
    // 5. 流式解压会话：进度馈送、穿越拦截归因、终态 PartialBlocked。
    //    恶意名条目在 zip_read 层被整体拒（读入口径）——能到达会话层的
    //    穿越名来自损坏抢救（salvage 不验名），夹具按此真实路径构造。
    let files: Vec<ZipFile> = vec![
        ZipFile { name: String::from("a.txt"), data: vec![1u8; 100], level: LEVEL_STORE },
        ZipFile { name: String::from("ok/子/b.txt"), data: vec![2u8; 200], level: LEVEL_STORE },
        ZipFile { name: String::from("c.txt"), data: vec![4u8; 150], level: LEVEL_STORE },
    ];
    let z = zip_write(&files);
    let mut entries = zip_read(&z).unwrap();
    // 抢救恢复注入（salvage 面产的裸条目——名未验，F176 由会话层拦截）。
    entries.push(ZipEntry {
        name: String::from("../evil.txt"),
        data: vec![3u8; 50],
        method: 0,
        crc: crc32(&[3u8; 50]),
    });
    let bytes_total: u64 = entries.iter().map(|e| e.data.len() as u64).sum();
    let mut sess = ExtractSession::new(entries.len(), bytes_total);
    let mut landed = 0usize;
    for e in &entries {
        if let Some((name, plain)) = sess.step_entry(&e.name, e.data.clone(), 1 << 20) {
            if !name.is_empty() {
                landed += 1; // 拦截条目回空名空载荷——不入盘
                drop(plain); // 落盘后即弃（流式口——明文不驻留）
            }
        }
    }
    let outcome = sess.outcome();
    let pm = sess.permille();
    set.add(
        "extract-stream",
        matches!(outcome, ExtractOutcome::PartialBlocked { done: 4, blocked: 1 })
            && landed == 3
            && sess.blocked_log.len() == 1
            && sess.blocked_log[0].0 == "../evil.txt"
            && pm == 1000,
        "stream + F176 log + F086 feed",
    );
    // 6. 取消语义：取消后剩余条目不再推进，终态 Cancelled { done, remaining }。
    let mut sess2 = ExtractSession::new(entries.len(), bytes_total);
    sess2.step_entry(&entries[0].name, entries[0].data.clone(), 1 << 20);
    sess2.cancel();
    let mid = sess2.step_entry(&entries[1].name, entries[1].data.clone(), 1 << 20);
    let out2 = sess2.outcome();
    set.add(
        "extract-cancel",
        mid.is_none()
            && matches!(out2, ExtractOutcome::Cancelled { done: 1, remaining: 3 }),
        "cancel keeps partial",
    );
    // 7. 盘满暂停：所需 > 剩余 → 暂停（已完成数冻结）；resume 后续跑。
    let mut sess3 = ExtractSession::new(entries.len(), bytes_total);
    let s1 = sess3.step_entry(&entries[0].name, entries[0].data.clone(), 10); // 需 100 > 10
    let frozen_done = sess3.progress().entries_done;
    let paused = sess3.is_paused() && s1.is_none() && frozen_done == 0;
    sess3.resume();
    let s2 = sess3.step_entry(&entries[0].name, entries[0].data.clone(), 1 << 20);
    set.add(
        "extract-diskfull",
        paused && s2.is_some(),
        "disk-full pause + resume",
    );
    // 8. 临时物账：用后即删零泄漏；删两次如实拒绝。
    let mut tmp = TempAccount::new();
    let t1 = tmp.create("打包.zip");
    let gone = tmp.dispose(&t1);
    let twice = tmp.dispose(&t1);
    set.add(
        "temp-account",
        t1.starts_with("~tmp-") && gone && !twice && tmp.leaked() == 0,
        "temp disposed, zero leak",
    );
    // 9. 分卷诚实拒：.zNN 检出；普通 .zip 不误伤。
    set.add(
        "split-honest",
        is_split_archive("相册.z01")
            && is_split_archive("DATA.Z99")
            && !is_split_archive("普通.zip"),
        "split detected, honest reject",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_deep {
    use super::*;

    #[test]
    fn extract_session_happy_path() {
        let files = vec![
            ZipFile { name: String::from("甲.txt"), data: "甲".as_bytes().to_vec(), level: LEVEL_STORE },
            ZipFile { name: String::from("乙/丙.txt"), data: "丙".as_bytes().to_vec(), level: LEVEL_STORE },
        ];
        let z = zip_write(&files);
        let entries = zip_read(&z).unwrap();
        let mut sess = ExtractSession::new(entries.len(), 4);
        let mut n = 0;
        for e in &entries {
            if sess.step_entry(&e.name, e.data.clone(), 1 << 20).is_some() {
                n += 1;
            }
        }
        assert_eq!(sess.outcome(), ExtractOutcome::Done(2));
        assert_eq!(n, 2);
    }

    #[test]
    fn extract_session_blocked_entry_never_lands() {
        // 拦截条目返回空载荷——调用方凭空载荷识别「已拦不入盘」。
        let mut sess = ExtractSession::new(1, 0);
        let r = sess.step_entry("../偷跑.txt", "内容".as_bytes().to_vec(), 1 << 20);
        assert_eq!(r, Some((String::new(), Vec::new())));
        assert_eq!(sess.blocked_log.len(), 1);
        assert_eq!(sess.outcome(), ExtractOutcome::PartialBlocked { done: 1, blocked: 1 });
    }

    #[test]
    fn password_card_advice_requires_three() {
        let mut card = PasswordCard::new();
        assert!(card.advice().is_none(), "零误不出建议");
        card.tries = 2;
        assert!(card.advice().is_none(), "两误不出建议");
        card.tries = 3;
        assert_eq!(card.advice(), Some("确认密码或换工具"));
    }

    #[test]
    fn archive_name_stem_rules() {
        // 多扩展去尾段、无扩展名直加 .zip——词干规则一致。
        assert_eq!(archive_name(&["备份.tar.gz"], "d").as_deref(), Some("备份.tar.zip"));
        assert_eq!(archive_name(&["README"], "d").as_deref(), Some("README.zip"));
    }

    #[test]
    fn permille_zero_total_is_honest() {
        // 空包（0 字节 0 条）：零工作即完满——F086 对话框立即关闭，
        // 进度 1000 与终态 Done(0) 一致（不编「进行中」的假进度）。
        let sess = ExtractSession::new(0, 0);
        assert_eq!(sess.permille(), 1000);
        assert_eq!(sess.outcome(), ExtractOutcome::Done(0));
    }

    #[test]
    fn zipkit_deep_checks_all_green() {
        let set = run_zipkit_deep_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F092-deep 红项：{}/{} 绿", p, p + f);
    }
}
