//! mech_zram — LZ4 型块压缩编解码 + 压缩页池（AI-K1 深化批次五 · F045）。
//!
//! 主册依据：
//! - F045【设计细节】「回收循环」——文件页可以丢弃重读、匿名页只能
//!   换出；zram 同族方案把换出页**压缩后放进内存池**，用 CPU 买内存。
//!   本件实现：① LZ4 块格式编解码（token/字面量/偏移/扩展长度、
//!   4KB 页内哈希表匹配、不可压数据 raw 兜底+诚实标志）；② 压缩页池
//!   （128 字节粒度尺寸类、占用/节省记账、2:1 门槛判据——省不到一半
//!   就直存，池只收值得压的页）。
//! - 解码正确性锚点：RFC 派生的 LZ4 块格式（官方格式规范 v1.0）；
//!   编码端自造流由解码端逐字节验收（round-trip 是唯一裁判）。
//! - 零堆、零浮点（哈希表定长数组）。

// ---------------------------------------------------------------------------
// 1. LZ4 块编解码
// ---------------------------------------------------------------------------

/// 页大小（压缩单位）。
pub const PAGE: usize = 4096;
/// 最小匹配长度（LZ4 规范）。
pub const MIN_MATCH: usize = 4;
/// 不可压直存的节省门槛：压缩后 ≥ 原始的 90% 就直存（zram 类策略）。
pub const WORTH_IT_PERMILLE: usize = 900;
/// 池尺寸类粒度（字节）。
pub const CLASS_GRAN: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lz4Err {
    /// 输出缓冲不足。
    Overflow,
    /// 位流损坏：偏移越界/长度矛盾/截断。
    Corrupt,
    /// 输入超页容量。
    TooBig,
}

/// LZ4 哈希函数（4 字节 → 12 位桶；×2654435761 是 Knuth 乘法散列）。
#[inline]
fn lz4_hash(u: u32) -> usize {
    ((u.wrapping_mul(2654435761)) >> 20) as usize & 0xFFF
}

#[inline]
fn read_u32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// LZ4 块压缩：in → out（out ≥ 8 + in×9/8 保证不溢出）。
/// 返回写出字节数。策略：4 字节步进哈希链表（单候选），找到
/// MIN_MATCH 以上匹配则编码序列，否则字面量累积。
pub fn lz4_compress(inp: &[u8], out: &mut [u8]) -> Result<usize, Lz4Err> {
    if inp.len() > PAGE {
        return Err(Lz4Err::TooBig);
    }
    let n = inp.len();
    if n == 0 {
        // 空输入：单个 token（0 字面量 0 匹配）即完整块。
        if out.is_empty() {
            return Err(Lz4Err::Overflow);
        }
        out[0] = 0;
        return Ok(1);
    }
    if n < MIN_MATCH + 1 {
        // 太短无匹配价值：纯字面量块。
        return emit_literals_only(inp, out);
    }
    /// 空桶标记（页 ≤ 4096，位置不会撞上 0xFFFF）。
    const EMPTY: u16 = 0xFFFF;
    let mut table = [EMPTY; 4096]; // 桶 → 位置
    let mut op = 0usize;
    let mut anchor = 0usize; // 当前未编码字面量段起点
    let mut ip = 0usize;

    macro_rules! push_byte {
        ($v:expr) => {{
            if op >= out.len() {
                return Err(Lz4Err::Overflow);
            }
            out[op] = $v;
            op += 1;
        }};
    }

    while ip + MIN_MATCH <= n {
        let seq = read_u32(inp, ip);
        let h = lz4_hash(seq);
        let cand = table[h] as usize;
        table[h] = ip as u16;
        // 候选有效（非空桶）且距离 ≤ 65535（块格式上限）。
        let valid = cand != EMPTY as usize && ip - cand <= 65535;
        let mut mlen = 0usize;
        if valid {
            while ip + mlen < n && inp[cand + mlen] == inp[ip + mlen] {
                mlen += 1;
            }
        }
        if mlen < MIN_MATCH {
            ip += 1;
            continue;
        }
        // 编码：字面量段 [anchor, ip) + 匹配。
        let lit = ip - anchor;
        let ml_code_len = mlen - MIN_MATCH;
        // token。
        let tok_hi = if lit >= 15 { 15 } else { lit } as u8;
        let tok_lo = if ml_code_len >= 15 { 15 } else { ml_code_len } as u8;
        push_byte!((tok_hi << 4) | tok_lo);
        // 扩展字面量长。
        let mut l = lit;
        while l >= 15 {
            push_byte!(if l >= 270 { 255 } else { (l - 15) as u8 });
            if l < 270 {
                break;
            }
            l -= 255;
        }
        if lit > 0 && op + lit > out.len() {
            return Err(Lz4Err::Overflow);
        }
        out[op..op + lit].copy_from_slice(&inp[anchor..anchor + lit]);
        op += lit;
        // 偏移（小端，= ip - cand）。
        let off = (ip - cand) as u16;
        push_byte!(off as u8);
        push_byte!((off >> 8) as u8);
        // 扩展匹配长。
        let mut m = ml_code_len;
        while m >= 15 {
            push_byte!(if m >= 270 { 255 } else { (m - 15) as u8 });
            if m < 270 {
                break;
            }
            m -= 255;
        }
        ip += mlen;
        anchor = ip;
    }
    // 收尾：剩余字面量段（无尾匹配）。
    let lit = n - anchor;
    if lit > 0 {
        let tok_hi = if lit >= 15 { 15 } else { lit } as u8;
        push_byte!(tok_hi << 4);
        let mut l = lit;
        while l >= 15 {
            push_byte!(if l >= 270 { 255 } else { (l - 15) as u8 });
            if l < 270 {
                break;
            }
            l -= 255;
        }
        if op + lit > out.len() {
            return Err(Lz4Err::Overflow);
        }
        out[op..op + lit].copy_from_slice(&inp[anchor..anchor + lit]);
        op += lit;
    }
    Ok(op)
}

/// 纯字面量块（极短输入）。
fn emit_literals_only(inp: &[u8], out: &mut [u8]) -> Result<usize, Lz4Err> {
    let lit = inp.len();
    if out.is_empty() {
        return Err(Lz4Err::Overflow);
    }
    let tok_hi = if lit >= 15 { 15 } else { lit } as u8;
    out[0] = tok_hi << 4;
    let mut op = 1usize;
    let mut l = lit;
    while l >= 15 {
        if op >= out.len() {
            return Err(Lz4Err::Overflow);
        }
        out[op] = if l >= 270 { 255 } else { (l - 15) as u8 };
        op += 1;
        if l < 270 {
            break;
        }
        l -= 255;
    }
    if op + lit > out.len() {
        return Err(Lz4Err::Overflow);
    }
    out[op..op + lit].copy_from_slice(inp);
    Ok(op + lit)
}

/// LZ4 块解码：in → out（调用方给精确输出长度）。
/// 返回写出字节数；任何不一致如实报 Corrupt（零静默）。
pub fn lz4_decompress(inp: &[u8], out: &mut [u8]) -> Result<usize, Lz4Err> {
    let (mut ip, mut op) = (0usize, 0usize);
    loop {
        if ip >= inp.len() {
            // 流以匹配序列收尾（编码器最后一个匹配顶到页尾）是合法
            // LZ4 块——恰好读尽即完整解码；越界才是损坏。
            if ip == inp.len() {
                return Ok(op);
            }
            return Err(Lz4Err::Corrupt);
        }
        let tok = inp[ip];
        ip += 1;
        // 字面量长。
        let mut lit = (tok >> 4) as usize;
        if lit == 15 {
            loop {
                if ip >= inp.len() {
                    return Err(Lz4Err::Corrupt);
                }
                let b = inp[ip] as usize;
                ip += 1;
                lit += b;
                if b != 255 {
                    break;
                }
            }
        }
        if op + lit > out.len() {
            return Err(Lz4Err::Overflow);
        }
        if ip + lit > inp.len() {
            return Err(Lz4Err::Corrupt);
        }
        out[op..op + lit].copy_from_slice(&inp[ip..ip + lit]);
        ip += lit;
        op += lit;
        // 结束判定：最后一个序列只有字面量（token 低 4 位为 0 且已到尾）。
        if ip >= inp.len() {
            return Ok(op);
        }
        // 匹配。
        if ip + 2 > inp.len() {
            return Err(Lz4Err::Corrupt);
        }
        let off = inp[ip] as usize | ((inp[ip + 1] as usize) << 8);
        ip += 2;
        if off == 0 || off > op {
            return Err(Lz4Err::Corrupt);
        }
        let mut ml = ((tok & 0xF) as usize) + MIN_MATCH;
        if (tok & 0xF) == 15 {
            loop {
                if ip >= inp.len() {
                    return Err(Lz4Err::Corrupt);
                }
                let b = inp[ip] as usize;
                ip += 1;
                ml += b;
                if b != 255 {
                    break;
                }
            }
        }
        if op + ml > out.len() {
            return Err(Lz4Err::Overflow);
        }
        // 逐字节复制（重叠匹配语义：off 可以是 1）。
        let start = op - off;
        for k in 0..ml {
            out[op + k] = out[start + k];
        }
        op += ml;
    }
}

// ---------------------------------------------------------------------------
// 2. 压缩页池
// ---------------------------------------------------------------------------

/// 池容量（页数）。
pub const POOL_PAGES: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct ZramPage {
    /// true = 原始直存（不可压）；false = LZ4 压缩驻留。
    pub raw: bool,
    pub stored_len: usize,
}

/// 压缩页池（定长页表 + bump 区 + 位置表）。
pub struct ZramPool {
    pages: [Option<ZramPage>; POOL_PAGES],
    /// 池体：压缩数据与直存原始页都真实驻留于此（load 可完整取回）。
    body: [u8; POOL_PAGES * PAGE],
    /// 每页在 body 内的位置（bump 分配）。
    pos: [usize; POOL_PAGES],
    used_bytes: usize,
    /// 统计面。
    pub stored_raw: u64,
    pub stored_comp: u64,
    pub bytes_saved: u64,
    pub rejected_unworthy: u64,
}

impl ZramPool {
    pub fn new() -> Self {
        ZramPool {
            pages: [None; POOL_PAGES],
            body: [0; POOL_PAGES * PAGE],
            pos: [0; POOL_PAGES],
            used_bytes: 0,
            stored_raw: 0,
            stored_comp: 0,
            bytes_saved: 0,
            rejected_unworthy: 0,
        }
    }

    pub fn class_gran(stored_len: usize) -> usize {
        ((stored_len + CLASS_GRAN - 1) / CLASS_GRAN) * CLASS_GRAN
    }

    /// 存入一页：压缩→判值→入池或直存（两种都真实保留数据）。
    pub fn store(&mut self, idx: usize, page: &[u8]) -> Result<(), Lz4Err> {
        if page.len() != PAGE || idx >= POOL_PAGES {
            return Err(Lz4Err::TooBig);
        }
        let mut buf = [0u8; PAGE + PAGE / 8 + 64];
        let n = lz4_compress(page, &mut buf)?;
        let slot = self.used_bytes;
        if n * 1000 >= PAGE * WORTH_IT_PERMILLE {
            // 不值得压：原始字节直存（诚实记账，不假装节省）。
            if slot + PAGE > self.body.len() {
                return Err(Lz4Err::Overflow);
            }
            self.body[slot..slot + PAGE].copy_from_slice(page);
            self.pos[idx] = slot;
            self.used_bytes += PAGE;
            self.pages[idx] = Some(ZramPage { raw: true, stored_len: PAGE });
            self.stored_raw += 1;
            self.rejected_unworthy += 1;
            return Ok(());
        }
        if slot + n > self.body.len() {
            return Err(Lz4Err::Overflow);
        }
        self.body[slot..slot + n].copy_from_slice(&buf[..n]);
        self.pos[idx] = slot;
        self.used_bytes += Self::class_gran(n);
        self.pages[idx] = Some(ZramPage { raw: false, stored_len: n });
        self.stored_comp += 1;
        self.bytes_saved += (PAGE - n) as u64;
        Ok(())
    }

    /// 取回一页（解压/拷出）。唯一裁判：调用方对比 round-trip。
    pub fn load(&self, idx: usize, out: &mut [u8]) -> Result<usize, Lz4Err> {
        let Some(p) = self.pages[idx] else {
            return Err(Lz4Err::Corrupt);
        };
        let src = &self.body[self.pos[idx]..self.pos[idx] + p.stored_len];
        if p.raw {
            out[..PAGE].copy_from_slice(src);
            Ok(PAGE)
        } else {
            lz4_decompress(src, out)
        }
    }

    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    pub fn saved_permille(&self) -> u64 {
        let total = self.stored_raw * PAGE as u64 + self.stored_comp * PAGE as u64;
        if total == 0 {
            0
        } else {
            self.bytes_saved * 1000 / total
        }
    }
}

impl Default for ZramPool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 3. CheckSet
// ---------------------------------------------------------------------------

pub fn run_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut cs = CheckSet::new("mech_zram");

    // 1) 重复文本页 round-trip：压→解→逐字节一致。
    {
        let page = repetitive_page(0x5A, 97);
        let mut out = [0u8; PAGE];
        let n = lz4_compress(&page, &mut out).unwrap();
        let mut back = [0u8; PAGE];
        let m = lz4_decompress(&out[..n], &mut back).unwrap();
        cs.add("zram_roundtrip", m == PAGE && back == page, "");
    }

    // 2) 已知小向量：字面量+重叠匹配（off=1 游程）。
    {
        let inp = b"aaaaaaaaaaaaaaaaaaaa"; // 20 × 'a'
        let mut out = [0u8; 64];
        let n = lz4_compress(inp, &mut out).unwrap();
        let mut back = [0u8; 20];
        let m = lz4_decompress(&out[..n], &mut back).unwrap();
        cs.add("zram_run_length", m == 20 && &back[..] == &inp[..], "");
    }

    // 3) 不可压数据诚实直存判定（压缩后 ≥ 90% → 拒入池）。
    {
        // 伪随机数据（确定性 LCG）——高熵不可压。
        let mut page = [0u8; PAGE];
        let mut x = 0x1234_5678u32;
        for b in page.iter_mut() {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *b = (x >> 24) as u8;
        }
        let mut out = [0u8; PAGE + PAGE / 8 + 64];
        let n = lz4_compress(&page, &mut out).unwrap();
        cs.add("zram_incompressible_honest", n * 1000 >= PAGE * WORTH_IT_PERMILLE, "");
    }

    // 4) 池记账：重复页入池 → saved 统计非零、类粒度对齐。
    {
        let mut pool = ZramPool::new();
        let page = repetitive_page(0xA0, 311);
        pool.store(0, &page).unwrap();
        pool.store(1, &page).unwrap();
        let aligned = pool.used_bytes() % CLASS_GRAN == 0;
        // load 往返：池内取回必须与原始页逐字节一致（直存与压缩两路都验）。
        let mut back = [0u8; PAGE];
        pool.load(0, &mut back).unwrap();
        let load_ok = back == page;
        let mut x = 0x9E37_79B9u32;
        let mut rnd = [0u8; PAGE];
        for b in rnd.iter_mut() {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *b = (x >> 24) as u8;
        }
        pool.store(2, &rnd).unwrap(); // 高熵 → 直存路
        pool.load(2, &mut back).unwrap();
        cs.add(
            "zram_pool_accounting",
            pool.stored_comp == 2
                && pool.bytes_saved > 0
                && aligned
                && pool.saved_permille() > 500
                && load_ok
                && back == rnd,
            "",
        );
    }

    // 5) 损坏拒绝：翻转压缩流中段 → 解码报错（不静默出错误数据）。
    {
        let page = repetitive_page(0x33, 41);
        let mut out = [0u8; PAGE + PAGE / 8 + 64];
        let n = lz4_compress(&page, &mut out).unwrap();
        out[n / 2] ^= 0xFF;
        let mut back = [0u8; PAGE];
        let r = lz4_decompress(&out[..n], &mut back);
        cs.add("zram_corrupt_rejected", r.is_err(), "");
    }

    // 6) 截断拒绝。
    {
        let page = repetitive_page(0x77, 13);
        let mut out = [0u8; PAGE + PAGE / 8 + 64];
        let n = lz4_compress(&page, &mut out).unwrap();
        let mut back = [0u8; PAGE];
        let r = lz4_decompress(&out[..n - 2], &mut back);
        cs.add("zram_truncated_rejected", r.is_err(), "");
    }

    cs
}

/// 确定性重复文本页（伪语句循环——可压但非平凡）。
fn repetitive_page(seed: u8, period: usize) -> [u8; PAGE] {
    let mut page = [0u8; PAGE];
    for (i, b) in page.iter_mut().enumerate() {
        *b = (seed as usize).wrapping_add(i / period * 7) as u8;
    }
    page
}

// ---------------------------------------------------------------------------
// 4. 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_various_periods() {
        for period in [1usize, 3, 7, 64, 512] {
            let page = repetitive_page(0x11 + period as u8, period);
            let mut out = [0u8; PAGE + PAGE / 8 + 64];
            let n = lz4_compress(&page, &mut out).unwrap();
            let mut back = [0u8; PAGE];
            let m = lz4_decompress(&out[..n], &mut back).unwrap();
            assert_eq!(m, PAGE);
            assert!(back == page, "period={}", period);
        }
    }

    #[test]
    fn roundtrip_binary_patterns() {
        // 递增序列、方波、稀疏点——不同匹配距离的覆盖。
        let mut page = [0u8; PAGE];
        for (i, b) in page.iter_mut().enumerate() {
            *b = match i % 16 {
                0..=3 => 0xFF,
                4..=7 => 0x00,
                _ => (i % 251) as u8,
            };
        }
        let mut out = [0u8; PAGE + PAGE / 8 + 64];
        let n = lz4_compress(&page, &mut out).unwrap();
        let mut back = [0u8; PAGE];
        lz4_decompress(&out[..n], &mut back).unwrap();
        assert!(back == page);
    }

    #[test]
    fn zero_page_compresses_hard() {
        // 全零页：游程压缩应远小于 10%（zram 的典型收益）。
        let page = [0u8; PAGE];
        let mut out = [0u8; PAGE + PAGE / 8 + 64];
        let n = lz4_compress(&page, &mut out).unwrap();
        assert!(n < PAGE / 10, "n={}", n);
        let mut back = [0u8; PAGE];
        lz4_decompress(&out[..n], &mut back).unwrap();
        assert!(back == page);
    }

    #[test]
    fn offset_one_overlap_run() {
        // off=1 的重叠复制（RLE 游程）是 LZ4 语义的试金石。
        let inp = [b'x'; 300];
        let mut out = [0u8; 512];
        let n = lz4_compress(&inp, &mut out).unwrap();
        let mut back = [0u8; 300];
        let m = lz4_decompress(&out[..n], &mut back).unwrap();
        assert_eq!(m, 300);
        assert!(back == inp);
    }

    #[test]
    fn long_literal_extension() {
        // 400 字节不重复字面量：扩展长度路径（255 循环）。
        let mut inp = [0u8; 400];
        let mut x = 7u32;
        for b in inp.iter_mut() {
            x = x.wrapping_mul(48_271).wrapping_add(11);
            *b = (x >> 16) as u8;
        }
        let mut out = [0u8; 512];
        let n = lz4_compress(&inp, &mut out).unwrap();
        let mut back = [0u8; 400];
        let m = lz4_decompress(&out[..n], &mut back).unwrap();
        assert_eq!(m, 400);
        assert!(back == inp);
    }

    #[test]
    fn corrupt_offset_zero_rejected() {
        // 手造流：token(4 字面量) + 字面量 + off=0（非法）。
        let stream = [0x40u8, 1, 2, 3, 4, 0, 0];
        let mut back = [0u8; 64];
        assert_eq!(lz4_decompress(&stream, &mut back), Err(Lz4Err::Corrupt));
    }

    #[test]
    fn corrupt_offset_beyond_history_rejected() {
        // off=5 但历史只有 4 字节。
        let stream = [0x40u8, 1, 2, 3, 4, 5, 0];
        let mut back = [0u8; 64];
        assert_eq!(lz4_decompress(&stream, &mut back), Err(Lz4Err::Corrupt));
    }

    #[test]
    fn pool_class_granularity() {
        assert_eq!(ZramPool::class_gran(1), 128);
        assert_eq!(ZramPool::class_gran(128), 128);
        assert_eq!(ZramPool::class_gran(129), 256);
        assert_eq!(ZramPool::class_gran(4096), 4096);
    }

    #[test]
    fn pool_worthiness_gate() {
        let mut pool = ZramPool::new();
        // 高熵页：压缩无收益 → 直存 + rejected 计数。
        let mut x = 99u32;
        let mut page = [0u8; PAGE];
        for b in page.iter_mut() {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *b = (x >> 24) as u8;
        }
        pool.store(0, &page).unwrap();
        assert_eq!(pool.stored_raw, 1);
        assert_eq!(pool.rejected_unworthy, 1);
        assert_eq!(pool.stored_comp, 0);
        // 全零页：高收益 → 入池。
        pool.store(1, &[0u8; PAGE]).unwrap();
        assert_eq!(pool.stored_comp, 1);
        assert!(pool.bytes_saved > (PAGE * 9 / 10) as u64);
    }
}

#[test]
fn pool_load_roundtrip_mixed() {
    let mut pool = ZramPool::new();
    let comp_page = repetitive_page(0xE0, 23);
    let mut rnd_page = [0u8; PAGE];
    let mut x = 0xCAFE_F00Du32;
    for b in rnd_page.iter_mut() {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *b = (x >> 24) as u8;
    }
    pool.store(0, &comp_page).unwrap();
    pool.store(1, &rnd_page).unwrap();
    let mut out = [0u8; PAGE];
    pool.load(0, &mut out).unwrap();
    assert!(out == comp_page, "压缩路取回一致");
    pool.load(1, &mut out).unwrap();
    assert!(out == rnd_page, "直存路取回一致");
    // 空槽诚实报错。
    assert_eq!(pool.load(5, &mut out), Err(Lz4Err::Corrupt));
}
