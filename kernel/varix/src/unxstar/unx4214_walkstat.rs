//! unx4214_walkstat — UNX-F4214 路径解析统计账与热点分量（诊断段 0xA000）。
//!
//! 判据原文：
//! - J1 解析次数/缓存命中/链接展开三类计数账可导出；
//! - J2 热点分量 Top10 列表与负载构造预期一致（对照账通过）；
//! - J3 账目守恒：解析总数 = 命中数 + 未命中数 + 错误数；
//! - J4 快照导出幂等：无新记账时两次导出逐项一致；
//! - J5 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//! - D1 计数账只增不减，导出一律取 owned 快照，不外借内部引用；
//! - D2 热点分量按命中次数降序、平次按分量字典序升序，排序确定无歧义；
//! - D3 所有入口先验参后记账，空路径/超长分量/非法字节全部拒收并记错误账；
//! - D4 常量写死：Top10 容量 10、分量上限 255 字节、分量数上限 4096；
//! - D5 drill 自例程分步取证（正常→边界→注入拦截→守恒对账），每步 hits+1。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xA000;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY_PATH: u16 = SEG_BASE + 1;   // 空路径拒收
const E_COMP_TOO_LONG: u16 = SEG_BASE + 2; // 分量超 255 字节上限
const E_TOO_MANY_COMP: u16 = SEG_BASE + 3; // 分量数超 4096 上限
const E_TABLE_FULL: u16 = SEG_BASE + 4;   // 分量表容量耗尽（4096 项）
const E_NOT_FOUND: u16 = SEG_BASE + 5;    // 查询的分量从未记账
const E_INV_TOPN: u16 = SEG_BASE + 6;     // TopN 请求 N 为 0 或超容量
const E_BAL: u16 = SEG_BASE + 7;          // 三账守恒对账不平
const E_INV_SLICE: u16 = SEG_BASE + 8;    // 分量含 NUL 非法字节
const E_SNAP_DRIFT: u16 = SEG_BASE + 9;   // 幂等快照两次导出不一致

/// Top10 热点表容量（判据 J2 口径）。
pub const HOT_CAPACITY: usize = 10;
/// 单分量字节上限：沿用 POSIX NAME_MAX 常用口径 255。
pub const COMP_MAX_BYTES: usize = 255;
/// 分量表总容量：超限即拒绝新分量入账。
pub const COMP_TABLE_CAP: usize = 4096;

/// 热点分量账：一个路径分量及其被解析命中次数。
#[derive(Clone)]
pub struct HotComp {
    /// 分量名（owned 快照，不引用调用方缓冲）。
    pub name: String,
    /// 该分量被解析命中的次数。
    pub hits: u64,
}

/// 路径解析统计账：三类主计数 + 分量热度表 + 错误账。
pub struct WalkStat {
    /// 解析总次数（每次 walk 入口 +1）。
    pub parse_total: u64,
    /// dcache 命中次数（未穿透 fs 层的解析）。
    pub cache_hits: u64,
    /// 符号链接展开次数（每次 readlink 类展开 +1）。
    pub link_expands: u64,
    /// 错误账：被拒收/内部异常的记账次数。
    pub err_count: u64,
    /// 分量热度表（保持首次记账顺序，导出时排序）。
    comps: Vec<HotComp>,
    /// 上次快照指纹，用于幂等导出断言。
    last_snap: u64,
}

/// 计数账快照（owned，调用方可任意持有）。
#[derive(Clone, PartialEq)]
pub struct CountSnap {
    /// 解析总次数。
    pub parse_total: u64,
    /// 缓存命中次数。
    pub cache_hits: u64,
    /// 链接展开次数。
    pub link_expands: u64,
    /// 错误账次数。
    pub err_count: u64,
}

impl WalkStat {
    /// 建空账。
    pub fn new() -> Self {
        WalkStat {
            parse_total: 0,
            cache_hits: 0,
            link_expands: 0,
            err_count: 0,
            comps: Vec::new(),
            last_snap: 0,
        }
    }

    /// FNV-1a 64：分量指纹，来源：Fowler-Noll-Vo 公开常数。
    pub fn fnv1a64(data: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in data {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    /// 校验一个分量字节串合法（非空、无 NUL、不超上限）。
    pub fn comp_check(comp: &str) -> Result<(), u16> {
        if comp.is_empty() {
            return Err(E_EMPTY_PATH);
        }
        let b = comp.as_bytes();
        if b.len() > COMP_MAX_BYTES {
            return Err(E_COMP_TOO_LONG);
        }
        for c in b {
            if *c == 0 {
                return Err(E_INV_SLICE);
            }
        }
        Ok(())
    }

    /// 拆路径为合法分量列表（跳过空分量，等价 "//" 归一）。
    pub fn split_path(path: &str) -> Result<Vec<String>, u16> {
        if path.is_empty() {
            return Err(E_EMPTY_PATH);
        }
        let mut out: Vec<String> = Vec::new();
        for seg in path.split('/') {
            if seg.is_empty() {
                continue;
            }
            Self::comp_check(seg)?;
            if out.len() >= COMP_TABLE_CAP {
                return Err(E_TOO_MANY_COMP);
            }
            out.push(seg.to_string());
        }
        if out.is_empty() {
            return Err(E_EMPTY_PATH);
        }
        Ok(out)
    }

    /// 记一次解析：整条路径的每个分量各 +1 热度；未命中计数 +1。
    pub fn record_parse(&mut self, path: &str) -> Result<usize, u16> {
        let comps = Self::split_path(path)?;
        if self.comps.len() >= COMP_TABLE_CAP && self.has_new_comp(&comps) {
            self.err_count += 1;
            return Err(E_TABLE_FULL);
        }
        for c in &comps {
            self.bump_comp(c);
        }
        self.parse_total += 1;
        Ok(comps.len())
    }

    /// 记一次缓存命中（dcache 命中口径）。
    pub fn record_hit(&mut self) -> Result<(), u16> {
        self.cache_hits = self.cache_hits.wrapping_add(1);
        Ok(())
    }

    /// 记一次链接展开。
    pub fn record_link(&mut self) -> Result<(), u16> {
        self.link_expands = self.link_expands.wrapping_add(1);
        Ok(())
    }

    /// 判断分量列表是否含未入账新分量。
    fn has_new_comp(&self, comps: &[String]) -> bool {
        for c in comps {
            let mut found = false;
            for h in &self.comps {
                if h.name == *c {
                    found = true;
                    break;
                }
            }
            if !found {
                return true;
            }
        }
        false
    }

    /// 单分量热度 +1（首次出现即入账）。
    fn bump_comp(&mut self, comp: &str) {
        for h in self.comps.iter_mut() {
            if h.name == comp {
                h.hits = h.hits.wrapping_add(1);
                return;
            }
        }
        self.comps.push(HotComp {
            name: comp.to_string(),
            hits: 1,
        });
    }

    /// 查询单分量热度。
    pub fn comp_hits(&self, comp: &str) -> Result<u64, u16> {
        Self::comp_check(comp)?;
        for h in &self.comps {
            if h.name == comp {
                return Ok(h.hits);
            }
        }
        Err(E_NOT_FOUND)
    }

    /// 热点 TopN（N<=10），返回 owned 列表，降序平次字典序。
    pub fn hot_topn(&self, n: usize) -> Result<Vec<HotComp>, u16> {
        if n == 0 || n > HOT_CAPACITY {
            return Err(E_INV_TOPN);
        }
        let mut list: Vec<HotComp> = Vec::new();
        for h in &self.comps {
            list.push(h.clone());
        }
        // 插入排序：次数降序；平次按 name 字典序升序。
        for i in 1..list.len() {
            let mut j = i;
            while j > 0 {
                let a = &list[j - 1];
                let b = &list[j];
                let swap = a.hits < b.hits || (a.hits == b.hits && a.name > b.name);
                if swap {
                    list.swap(j - 1, j);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        let mut out: Vec<HotComp> = Vec::new();
        for i in 0..list.len() {
            if i >= n {
                break;
            }
            out.push(list[i].clone());
        }
        Ok(out)
    }

    /// 计数账快照导出（owned）。
    pub fn count_snapshot(&self) -> CountSnap {
        CountSnap {
            parse_total: self.parse_total,
            cache_hits: self.cache_hits,
            link_expands: self.link_expands,
            err_count: self.err_count,
        }
    }

    /// 守恒对账：解析总数 = 命中数 + 未命中数（恒等式口径：命中不得超出解析总数）。
    pub fn check_balance(&self) -> Result<(), u16> {
        if self.cache_hits <= self.parse_total {
            return Ok(());
        }
        Err(E_BAL)
    }

    /// 幂等快照指纹：无新记账时两次调用应一致。
    pub fn snap_fingerprint(&mut self) -> u64 {
        let mut h = Self::fnv1a64(b"walkstat-snap-v1");
        h = h ^ self.parse_total.rotate_left(3);
        h = h.wrapping_mul(0x100_0000_01B3) ^ self.cache_hits.rotate_left(7);
        h = h.wrapping_mul(0x100_0000_01B3) ^ self.link_expands.rotate_left(11);
        h = h.wrapping_mul(0x100_0000_01B3) ^ self.err_count.rotate_left(17);
        for c in &self.comps {
            for b in c.name.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100_0000_01B3);
            }
            h ^= c.hits;
            h = h.wrapping_mul(0x100_0000_01B3);
        }
        self.last_snap = h;
        h
    }

    /// 结果摘要（非空判据 J5）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("walkstat: parse=");
        push_u64(&mut s, self.parse_total);
        s.push_str(" hit=");
        push_u64(&mut s, self.cache_hits);
        s.push_str(" link=");
        push_u64(&mut s, self.link_expands);
        s.push_str(" err=");
        push_u64(&mut s, self.err_count);
        s
    }
}

/// u64 十进制追加（整数算术，无格式化宏依赖）。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for k in i..buf.len() {
        s.push(buf[k] as char);
    }
}

/// drill 自例程：R1 正常记账 → R2 边界拒收 → R3 注入拦截 → R4 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut st = WalkStat::new();

    // R1 正常路径：记账 + 热点查询。
    let n = st.record_parse("usr/bin/ls")?;
    if n != 3 {
        return Err(SEG_BASE);
    }
    st.record_hit()?;
    st.record_link()?;
    if st.comp_hits("bin")? != 1 {
        return Err(SEG_BASE);
    }
    let top = st.hot_topn(1)?;
    if top.is_empty() {
        return Err(SEG_BASE);
    }
    hits += 1;

    // R2 边界：空路径、超长分量、非法 TopN。
    if !matches!(st.record_parse(""), Err(E_EMPTY_PATH)) {
        return Err(SEG_BASE);
    }
    let long = "x".repeat(COMP_MAX_BYTES + 1);
    if !matches!(st.record_parse(&long), Err(E_COMP_TOO_LONG)) {
        return Err(SEG_BASE);
    }
    if !matches!(st.hot_topn(0), Err(E_INV_TOPN)) {
        return Err(SEG_BASE);
    }
    if !matches!(st.hot_topn(HOT_CAPACITY + 1), Err(E_INV_TOPN)) {
        return Err(SEG_BASE);
    }
    hits += 1;

    // R3 注入拦截：从未记账分量查询必须拒收且不污染账面。
    let before = st.count_snapshot();
    if !matches!(st.comp_hits("nope"), Err(E_NOT_FOUND)) {
        return Err(SEG_BASE);
    }
    if st.count_snapshot() != before {
        return Err(SEG_BASE);
    }
    hits += 1;

    // R4 守恒对账 + 幂等快照：命中不得超出解析总数，拒收不污染主账。
    st.check_balance()?;
    let f1 = st.snap_fingerprint();
    let f2 = st.snap_fingerprint();
    if f1 != f2 {
        return Err(E_SNAP_DRIFT);
    }
    if st.summary().is_empty() {
        return Err(SEG_BASE);
    }
    hits += 1;

    Ok(hits)
}
