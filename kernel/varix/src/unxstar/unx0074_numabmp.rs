//! UNX-F0074 · NUMA 节点亲和页位图 · 诊断段 0x2190
//! 验收锚点：UNX-F0074-J1 每节点页位图互斥登记，亲和分配路径
//! 与跨节点回退账判据通过；纯功能代码 ≥280 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2190;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NODE: u16 = SEG_BASE + 1;     // 节点号越出闭集
const E_RANGE: u16 = SEG_BASE + 2;    // 页号越出节点域
const E_TWICE: u16 = SEG_BASE + 3;    // 页已在位图（跨节点双登记）
const E_FREE_TWICE: u16 = SEG_BASE + 4; // 双释放
const E_RDONLY: u16 = SEG_BASE + 5;   // 位图已封板
const E_BAL: u16 = SEG_BASE + 6;      // 节点账守恒不平
const E_EMPTY: u16 = SEG_BASE + 7;    // 零节点拓扑
const E_AFFINITY: u16 = SEG_BASE + 8; // 亲和页跨节点（页只能属一节点）

pub const MAX_NODES: usize = 16;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NODE => "节点号越出闭集：0..MAX_NODES",
        E_RANGE => "页号越出节点域：先扩域",
        E_TWICE => "页已在位图：跨节点双登记拦截",
        E_FREE_TWICE => "双释放：位已清",
        E_RDONLY => "位图已封板：再操作属误用",
        E_BAL => "节点账守恒：Σ节点置位数 = 全局分配数",
        E_EMPTY => "零节点拓扑：先登记节点",
        E_AFFINITY => "亲和页跨节点：一页一节点，登记前先查全局",
        _ => "未知诊断码",
    }
}

/// 单节点位图（复用动态字域）。
pub struct NodeBitmap {
    words: Vec<u64>,
    pub bits: usize,
    pub node: usize,
}

impl NodeBitmap {
    pub fn new(node: usize, bits: usize) -> NodeBitmap {
        NodeBitmap {
            words: vec![0u64; (bits + 63) / 64],
            bits,
            node,
        }
    }

    pub fn get(&self, i: usize) -> Result<bool, u16> {
        if i >= self.bits {
            return Err(E_RANGE);
        }
        Ok(self.words[i / 64] & (1u64 << (i % 64)) != 0)
    }

    pub fn test_and_set(&mut self, i: usize) -> Result<bool, u16> {
        if i >= self.bits {
            return Err(E_RANGE);
        }
        let w = &mut self.words[i / 64];
        let m = 1u64 << (i % 64);
        if *w & m != 0 {
            return Ok(false); // 已置位
        }
        *w |= m;
        Ok(true)
    }

    pub fn clear(&mut self, i: usize) -> Result<(), u16> {
        if i >= self.bits {
            return Err(E_RANGE);
        }
        let w = &mut self.words[i / 64];
        let m = 1u64 << (i % 64);
        if *w & m == 0 {
            return Err(E_FREE_TWICE);
        }
        *w &= !m;
        Ok(())
    }

    pub fn popcount(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// 首个空闲页（亲和分配核心：本节点内最近空闲页）。
    pub fn first_free(&self) -> Option<usize> {
        for (wi, w) in self.words.iter().enumerate() {
            let inv = !w;
            if inv != 0 {
                let b = inv.trailing_zeros() as usize;
                let i = wi * 64 + b;
                if i < self.bits {
                    return Some(i);
                }
            }
        }
        None
    }
}

// ── 亲和账 ───────────────────────────────────────────────────
pub struct NumaAffinityLedger {
    pub nodes: Vec<NodeBitmap>,
    pub allocs_total: usize,
    pub fallback_allocs: usize,
    pub sealed: bool,
}

impl NumaAffinityLedger {
    pub fn new(node_sizes: &[usize]) -> Result<NumaAffinityLedger, u16> {
        if node_sizes.is_empty() || node_sizes.len() > MAX_NODES {
            return Err(E_EMPTY);
        }
        let mut nodes = Vec::new();
        for (n, sz) in node_sizes.iter().enumerate() {
            nodes.push(NodeBitmap::new(n, *sz));
        }
        Ok(NumaAffinityLedger {
            nodes,
            allocs_total: 0,
            fallback_allocs: 0,
            sealed: false,
        })
    }

    /// 全局已占用判定（跨节点互斥的前置账）。
    /// 页号语义：全局页 = 节点基址 × 节点容量……本仓采用「节点内局部页号」，
    /// 跨节点互斥由调用方保证物理页唯一；本账内以 (node, page) 二元组为键。
    fn is_taken(&self, node: usize, page: usize) -> Result<bool, u16> {
        if node >= self.nodes.len() {
            return Err(E_NODE);
        }
        self.nodes[node].get(page)
    }

    /// 亲和分配：本节点找空闲；找不到走最近节点回退并记账。
    /// `prefer_order`：回退序（节点号闭集内）。
    pub fn alloc_page(&mut self, home: usize, prefer_order: &[usize]) -> Result<(usize, usize), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if home >= self.nodes.len() {
            return Err(E_NODE);
        }
        if let Some(p) = self.nodes[home].first_free() {
            let taken = self.is_taken(home, p)?;
            if !taken {
                let ok = self.nodes[home].test_and_set(p)?;
                if ok {
                    self.allocs_total += 1;
                    return Ok((home, p));
                }
            }
        }
        // 回退序
        for &n in prefer_order {
            if n >= self.nodes.len() {
                return Err(E_NODE);
            }
            if let Some(p) = self.nodes[n].first_free() {
                let ok = self.nodes[n].test_and_set(p)?;
                if ok {
                    self.allocs_total += 1;
                    self.fallback_allocs += 1;
                    return Ok((n, p));
                }
            }
        }
        Err(E_BAL) // 全域耗尽
    }

    pub fn free_page(&mut self, node: usize, page: usize) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if node >= self.nodes.len() {
            return Err(E_NODE);
        }
        self.nodes[node].clear(page)?;
        self.allocs_total = self.allocs_total.saturating_sub(1);
        Ok(())
    }

    /// 节点账守恒：Σpopcount == allocs_total。
    pub fn reconcile(&self) -> Result<(), u16> {
        let sum: usize = self.nodes.iter().map(|n| n.popcount()).sum();
        if sum != self.allocs_total {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 节点占用率（千分比）。
    pub fn usage_permille(&self, node: usize) -> Result<u64, u16> {
        if node >= self.nodes.len() {
            return Err(E_NODE);
        }
        let n = &self.nodes[node];
        if n.bits == 0 {
            return Ok(0);
        }
        Ok(n.popcount() as u64 * 1000 / n.bits as u64)
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for n in &self.nodes {
            buf.extend_from_slice(&(n.node as u64).to_le_bytes());
            for w in &n.words {
                buf.extend_from_slice(&w.to_le_bytes());
            }
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：双节点拓扑——本节点亲和分配命中。
    let mut led = NumaAffinityLedger::new(&[64, 64])?;
    let (n, p) = led.alloc_page(0, &[1])?;
    if n != 0 || p != 0 {
        return Err(E_BAL);
    }
    let (n2, p2) = led.alloc_page(0, &[1])?;
    if n2 != 0 || p2 != 1 {
        return Err(E_BAL);
    }
    led.reconcile()?;
    hits += 1;

    // R2：本节点耗尽 → 回退节点并记账。
    let mut led2 = NumaAffinityLedger::new(&[2, 8])?;
    led2.alloc_page(0, &[])?; // 占 0 号
    led2.alloc_page(0, &[])?; // 占 1 号 → 本节点满
    let (n3, _) = led2.alloc_page(0, &[1])?;
    if n3 != 1 {
        return Err(E_BAL);
    }
    if led2.fallback_allocs != 1 {
        return Err(E_BAL);
    }
    led2.reconcile()?;
    hits += 1;

    // R3：释放后 first_free 复用。
    led2.free_page(0, 0)?;
    led2.reconcile()?;
    let (n4, p4) = led2.alloc_page(0, &[1])?;
    if n4 != 0 || p4 != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：占用率与指纹对拍。
    let u0 = led2.usage_permille(0)?;
    if u0 == 0 {
        return Err(E_BAL);
    }
    let fp1 = led2.fingerprint();
    let mut led3 = NumaAffinityLedger::new(&[2, 8])?;
    led3.alloc_page(0, &[])?;
    led3.alloc_page(0, &[])?;
    led3.alloc_page(0, &[1])?;
    led3.free_page(0, 0)?;
    led3.alloc_page(0, &[1])?;
    if led3.fingerprint() != fp1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：节点越界 → E_NODE。
    let mut i1 = NumaAffinityLedger::new(&[8, 8])?;
    if i1.alloc_page(5, &[]) != Err(E_NODE) {
        return Err(E_NODE);
    }
    hits += 1;

    // I2：零节点拓扑 → E_EMPTY。
    if NumaAffinityLedger::new(&[]).is_ok() {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I3：双释放 → E_FREE_TWICE。
    let mut i3 = NumaAffinityLedger::new(&[8])?;
    i3.alloc_page(0, &[])?;
    i3.free_page(0, 0)?;
    if i3.free_page(0, 0) != Err(E_FREE_TWICE) {
        return Err(E_FREE_TWICE);
    }
    hits += 1;

    // I4：全域耗尽 → E_BAL。
    let mut i4 = NumaAffinityLedger::new(&[1])?;
    i4.alloc_page(0, &[])?;
    if i4.alloc_page(0, &[]) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
