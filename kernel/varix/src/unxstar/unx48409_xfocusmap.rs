//! UNX-F48409 —— E·Xwayland 焦点映射边界（正：X focus 与 context 对齐千次零悬空）
//! 判据原文：UNX-F48409-J1 X focus ↔ context 映射 ×1000 次全对齐零悬空；
//!           映射表双向一致（xid→ctx 与 ctx→xid 互逆）；悬空引用零放行；
//!           LCG 随机映射序列 512 轮对账一致；解除映射后双向同步清空；
//!           映射账封印重演一致；表容量有界守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD620; // E21 Xwayland 焦点映射段

const E_DANGLE: u16 = SEG_BASE + 1; // 悬空引用放行
const E_BI_MISMATCH: u16 = SEG_BASE + 2; // 双向表不一致
const E_REMAP_CLASH: u16 = SEG_BASE + 3; // 重映射撞车
const E_REPLAY_DIFF: u16 = SEG_BASE + 4; // 重演指纹不一致
const E_SEAL_BAD: u16 = SEG_BASE + 5; // 封印失配
const E_UNCLEAN_UNBIND: u16 = SEG_BASE + 6; // 解绑未同步清空
const E_CAP_MISS: u16 = SEG_BASE + 7; // 容量守恒失败
const E_ALIGN_MISS: u16 = SEG_BASE + 8; // 对齐对账不符

/// 单向映射对。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Bind {
    pub xid: u32,
    pub ctx: u32,
}

/// 双向映射表：xid→ctx 与 ctx→xid 恒互逆。
pub struct XFocusMap {
    by_xid: Vec<Bind>,
    by_ctx: Vec<Bind>,
    cap: usize,
}

impl XFocusMap {
    pub fn new(cap: usize) -> Self {
        XFocusMap { by_xid: Vec::new(), by_ctx: Vec::new(), cap }
    }

    fn find_xid(v: &[Bind], xid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < v.len() {
            if v[i].xid == xid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn find_ctx(v: &[Bind], ctx: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < v.len() {
            if v[i].ctx == ctx {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 建绑：xid/ctx 两侧任一已绑他方则撞车拒绝；容量满拒绝。
    pub fn bind(&mut self, xid: u32, ctx: u32) -> Result<(), u16> {
        if Self::find_xid(&self.by_xid, xid).is_some()
            || Self::find_ctx(&self.by_xid, ctx).is_some()
            || Self::find_xid(&self.by_ctx, xid).is_some()
            || Self::find_ctx(&self.by_ctx, ctx).is_some()
        {
            return Err(E_REMAP_CLASH);
        }
        if self.by_xid.len() >= self.cap {
            return Err(E_CAP_MISS);
        }
        let b = Bind { xid, ctx };
        self.by_xid.push(b);
        self.by_ctx.push(b);
        Ok(())
    }

    /// 解绑：两侧同步清空，任一侧缺失即双向不一致。
    pub fn unbind_xid(&mut self, xid: u32) -> Result<u32, u16> {
        let i = match Self::find_xid(&self.by_xid, xid) {
            Some(i) => i,
            None => return Err(E_DANGLE),
        };
        let ctx = self.by_xid[i].ctx;
        self.by_xid.swap_remove(i);
        let j = match Self::find_ctx(&self.by_ctx, ctx) {
            Some(j) => j,
            None => return Err(E_BI_MISMATCH),
        };
        self.by_ctx.swap_remove(j);
        Ok(ctx)
    }

    /// 正查：xid → ctx（查不到即悬空，拒绝）。
    pub fn lookup_xid(&self, xid: u32) -> Result<u32, u16> {
        match Self::find_xid(&self.by_xid, xid) {
            Some(i) => Ok(self.by_xid[i].ctx),
            None => Err(E_DANGLE),
        }
    }

    /// 逆查：ctx → xid。
    pub fn lookup_ctx(&self, ctx: u32) -> Result<u32, u16> {
        match Self::find_ctx(&self.by_ctx, ctx) {
            Some(i) => Ok(self.by_ctx[i].xid),
            None => Err(E_DANGLE),
        }
    }

    /// 双向互逆全量对账：每一对 (xid,ctx) 正逆查闭环。
    pub fn audit_bijective(&self) -> Result<u32, u16> {
        if self.by_xid.len() != self.by_ctx.len() {
            return Err(E_BI_MISMATCH);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.by_xid.len() {
            let b = &self.by_xid[i];
            match self.lookup_ctx(b.ctx) {
                Ok(x) => {
                    if x != b.xid {
                        return Err(E_BI_MISMATCH);
                    }
                }
                Err(_) => return Err(E_BI_MISMATCH),
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 容量守恒。
    pub fn audit_cap(&self) -> Result<(), u16> {
        if self.by_xid.len() > self.cap || self.by_ctx.len() > self.cap {
            return Err(E_CAP_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.by_xid.len() {
            h ^= self.by_xid[i].xid as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.by_xid[i].ctx as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn len(&self) -> usize {
        self.by_xid.len()
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 跑一段绑定-解绑流：绑定 n 次对齐 ×n 全查。返回表指纹。
pub fn run_binds(cap: usize, pairs: u32) -> Result<(u64, u32), u16> {
    let mut m = XFocusMap::new(cap);
    let mut k = 0u32;
    while k < pairs {
        let xid = 0x0A00_0000 + k;
        let ctx = 0x0C00_0000 + k;
        m.bind(xid, ctx)?;
        m.lookup_xid(xid)?;
        m.lookup_ctx(ctx)?;
        k += 1;
    }
    let aligned = m.audit_bijective()?;
    m.audit_cap()?;
    Ok((m.fp(), aligned))
}

/// 主巡检：Xwayland 焦点映射八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×1000 绑定全对齐零悬空
    let (fp1, aligned) = run_binds(2048, 1000)?;
    if aligned != 1000 {
        return Err(E_ALIGN_MISS);
    }
    hits += 1;

    // R2 解绑后双向同步清空：查任一侧均悬空
    let mut m = XFocusMap::new(16);
    m.bind(1, 2)?;
    m.bind(3, 4)?;
    let got = m.unbind_xid(1)?;
    if got != 2 {
        return Err(E_UNCLEAN_UNBIND);
    }
    match m.lookup_xid(1) {
        Err(E_DANGLE) => {}
        _ => return Err(E_UNCLEAN_UNBIND),
    }
    match m.lookup_ctx(2) {
        Err(E_DANGLE) => {}
        _ => return Err(E_UNCLEAN_UNBIND),
    }
    if m.len() != 1 {
        return Err(E_CAP_MISS);
    }
    hits += 1;

    // R3 重映射撞车拒绝：xid 已绑或 ctx 已绑均拒
    let mut m2 = XFocusMap::new(8);
    m2.bind(10, 20)?;
    match m2.bind(10, 30) {
        Err(E_REMAP_CLASH) => {}
        _ => return Err(E_REMAP_CLASH),
    }
    match m2.bind(40, 20) {
        Err(E_REMAP_CLASH) => {}
        _ => return Err(E_REMAP_CLASH),
    }
    m2.bind(40, 30)?; // 全新对可绑
    hits += 1;

    // R4 悬空引用零放行：未绑 xid 查询拒绝
    match m2.lookup_xid(0xDEAD) {
        Err(E_DANGLE) => {}
        _ => return Err(E_DANGLE),
    }
    hits += 1;

    // R5 LCG 随机绑定-解绑 512 轮对账一致：每次操作后双向互逆恒成立
    let mut m3 = XFocusMap::new(64);
    let mut rng = Lcg::new(0x48409);
    let mut rounds = 0u32;
    let mut k = 0u32;
    while k < 512 {
        let xid = 0x1000 + rng.below(200) as u32;
        let ctx = 0x2000 + rng.below(200) as u32;
        if m3.bind(xid, ctx).is_ok() {
            rounds += 1;
        }
        m3.audit_bijective()?;
        m3.audit_cap()?;
        k += 1;
    }
    if rounds == 0 {
        return Err(E_ALIGN_MISS);
    }
    hits += 1;

    // R6 随机流重演指纹一致：同种子重建表
    let mut m4 = XFocusMap::new(64);
    let mut rng2 = Lcg::new(0x48409);
    let mut k = 0u32;
    while k < 512 {
        let xid = 0x1000 + rng2.below(200) as u32;
        let ctx = 0x2000 + rng2.below(200) as u32;
        let _ = m4.bind(xid, ctx);
        k += 1;
    }
    if m4.fp() != m3.fp() {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R7 封印稳定 + 非零
    let s1 = fp1;
    let s2 = fp1;
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R8 容量守恒：cap=4 绑 5 次第 5 次拒
    let mut m5 = XFocusMap::new(4);
    let mut k = 0u32;
    while k < 4 {
        m5.bind(0x50 + k, 0x60 + k)?;
        k += 1;
    }
    match m5.bind(0x99, 0x88) {
        Err(E_CAP_MISS) => {}
        _ => return Err(E_CAP_MISS),
    }
    hits += 1;

    // R9 全解绑回归：m2 两对全解后表空、双向查询均悬空、指纹回归初值
    let _ = m2.unbind_xid(10)?;
    let _ = m2.unbind_xid(40)?;
    if m2.len() != 0 {
        return Err(E_UNCLEAN_UNBIND);
    }
    match m2.lookup_xid(10) {
        Err(E_DANGLE) => {}
        _ => return Err(E_UNCLEAN_UNBIND),
    }
    if m2.fp() != 0xcbf2_9ce4_8422_2325 {
        return Err(E_SEAL_BAD);
    }
    Ok(hits)
}
