//! UNX-F0147 · 增量构建与缓存 · 诊断段 0x2560
//!
//! 判据原文（UNX-F0147-J1）：未变更物料跳过重编译（缓存命中），变更
//! 物料及其下游全部重编（依赖传播），指纹账与缓存账守恒。
//!
//! 设计要点：物料指纹 = FNV-1a32(内容)；缓存行 = {指纹, 产物}；
//! 变更传播：依赖图拓扑序传播脏标（上游脏 ⇒ 下游脏）；缓存账守恒：
//! 命中 + 重编 = 总物料数。零浮点、零 unwrap/expect。

use alloc::vec;
use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2560;

const E_NODE: u16 = SEG_BASE + 1; // 节点名越界/重名
const E_CYCLE: u16 = SEG_BASE + 2; // 依赖环
const E_BAL: u16 = SEG_BASE + 3; // 缓存账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 4; // 册已冻结拒写
const E_FRESH: u16 = SEG_BASE + 5; // 脏节点无指纹可比

const FNV32_OFF: u32 = 0x811C_9DC5;
const FNV32_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 位折叠。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h = FNV32_OFF;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(FNV32_PRIME);
    }
    h
}

/// 构建图节点。
#[derive(Clone)]
pub struct BuildNode {
    pub name: String,
    pub content: Vec<u8>,
    pub deps: Vec<usize>, // 依赖节点下标（拓扑序须无环）
    pub fp: u32,
    pub dirty: bool,
}

/// 缓存行：指纹 → 产物副本。
#[derive(Clone)]
pub struct CacheLine {
    pub fp: u32,
    pub product: Vec<u8>,
}

/// 缓存账：命中 + 重编 = 总数。
pub struct IncrBook {
    pub nodes: Vec<BuildNode>,
    pub cache: Vec<CacheLine>,
    pub hits: u32,
    pub rebuilds: u32,
    pub frozen: bool,
}

impl IncrBook {
    pub fn new() -> Result<Self, u16> {
        Ok(IncrBook {
            nodes: Vec::new(),
            cache: Vec::new(),
            hits: 0,
            rebuilds: 0,
            frozen: false,
        })
    }

    /// 加节点（重名即拒；指纹按内容计算）。
    pub fn add_node(&mut self, name: &str, content: &[u8], deps: &[usize]) -> Result<usize, u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if name.is_empty() {
            return Err(E_NODE);
        }
        if self.nodes.iter().any(|n| n.name == name) {
            return Err(E_NODE);
        }
        for &d in deps {
            if d >= self.nodes.len() {
                return Err(E_NODE);
            }
        }
        let idx = self.nodes.len();
        self.nodes.push(BuildNode {
            name: name.to_string(),
            content: content.to_vec(),
            deps: deps.to_vec(),
            fp: fnv1a32(content),
            dirty: true, // 新节点首建即脏
        });
        Ok(idx)
    }

    /// 依赖环检测（DFS 三色标记）。
    pub fn check_cycles(&self) -> Result<(), u16> {
        const WHITE: u8 = 0;
        const GRAY: u8 = 1;
        let n = self.nodes.len();
        let mut color = vec![WHITE; n];
        fn visit(nodes: &[BuildNode], color: &mut [u8], i: usize) -> Result<(), u16> {
            if color[i] == GRAY {
                return Err(E_CYCLE);
            }
            if color[i] == 2 {
                return Ok(());
            }
            color[i] = GRAY;
            for &d in &nodes[i].deps {
                visit(nodes, color, d)?;
            }
            color[i] = 2;
            Ok(())
        }
        for i in 0..n {
            visit(&self.nodes, &mut color, i)?;
        }
        Ok(())
    }

    /// 内容更新：指纹比对判脏，脏即传播下游（依赖该节点的都标脏）。
    pub fn update_content(&mut self, idx: usize, content: &[u8]) -> Result<bool, u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if idx >= self.nodes.len() {
            return Err(E_NODE);
        }
        let new_fp = fnv1a32(content);
        let changed = self.nodes[idx].fp != new_fp;
        if changed {
            self.nodes[idx].content = content.to_vec();
            self.nodes[idx].fp = new_fp;
            self.nodes[idx].dirty = true;
            // 传播：反向下标扫（下游 = 依赖 idx 的节点，由于 add 时
            // deps 只指向前置节点，下游下标恒 > idx）。
            let n = self.nodes.len();
            for j in (idx + 1)..n {
                if self.nodes[j].deps.contains(&idx) {
                    self.nodes[j].dirty = true;
                }
            }
        }
        Ok(changed)
    }

    /// 增量构建：脏节点重编（产物合成 = 名字节 + 指纹字节），
    /// 净节点查缓存命中；命中 + 重编 = 总数。
    pub fn build(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        self.check_cycles()?;
        self.hits = 0;
        self.rebuilds = 0;
        for i in 0..self.nodes.len() {
            if self.nodes[i].dirty {
                let product = self.synth_product(i);
                let fp = self.nodes[i].fp;
                match self.cache.iter().position(|c| c.fp == fp) {
                    Some(p) => {
                        self.cache[p].product = product; // 刷新产物副本
                    }
                    None => {
                        self.cache.push(CacheLine { fp, product });
                    }
                }
                self.nodes[i].dirty = false;
                self.rebuilds += 1;
            } else {
                // 净节点必须能在缓存找到同指纹行，否则账面断裂。
                if !self.cache.iter().any(|c| c.fp == self.nodes[i].fp) {
                    return Err(E_FRESH);
                }
                self.hits += 1;
            }
        }
        Ok(())
    }

    fn synth_product(&self, idx: usize) -> Vec<u8> {
        let mut p = self.nodes[idx].name.as_bytes().to_vec();
        p.extend_from_slice(&self.nodes[idx].fp.to_le_bytes());
        p
    }

    /// 缓存账守恒：命中 + 重编 == 节点数；净节点必有缓存行。
    pub fn audit(&self) -> Result<(), u16> {
        if self.hits + self.rebuilds != self.nodes.len() as u32 {
            return Err(E_BAL);
        }
        for n in &self.nodes {
            if !n.dirty && !self.cache.iter().any(|c| c.fp == n.fp) {
                return Err(E_BAL);
            }
        }
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 渲染增量账。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "增量账 命中{} 重编{} 缓存行{}\n",
            self.hits,
            self.rebuilds,
            self.cache.len()
        ));
        for n in &self.nodes {
            out.push_str(&format!(
                "{} fp{:08X} {}\n",
                n.name,
                n.fp,
                if n.dirty { "脏" } else { "净" }
            ));
        }
        out
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：首建全重编。
    let mut b = IncrBook::new()?;
    let src = b.add_node("src.c", b"int main(){}", &[])?;
    let obj = b.add_node("src.o", b"", &[src])?;
    let _bin = b.add_node("varix.bin", b"", &[obj])?;
    b.build()?;
    if b.rebuilds != 3 || b.hits != 0 {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // L2：无变更二建——全命中（缓存生效）。
    b.build()?;
    if b.rebuilds != 0 || b.hits != 3 {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // L3：改上游——上游 + 下游全重编，无关节点仍命中。
    let helper = b.add_node("helper.o", b"h", &[])?;
    let _ = helper;
    b.build()?; // helper 先建净
    if b.update_content(src, b"int main(){ return 0; }")? != true {
        return Err(E_BAL);
    }
    b.build()?;
    // src/src.o/varix.bin 重编 3，helper 净命中 1。
    if b.rebuilds != 3 || b.hits != 1 {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // L4：改内容但指纹相同（等值内容）——不判脏零传播。
    let same = b.nodes[src].content.clone();
    if b.update_content(src, &same)? {
        return Err(E_BAL);
    }
    b.build()?;
    if b.rebuilds != 0 || b.hits != 4 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重名节点拒。
    let mut i1 = IncrBook::new()?;
    i1.add_node("a", b"1", &[])?;
    if i1.add_node("a", b"2", &[]) != Err(E_NODE) {
        return Err(E_NODE);
    }
    hits += 1;

    // I2：依赖下标越界拒。
    if i1.add_node("b", b"1", &[9]) != Err(E_NODE) {
        return Err(E_NODE);
    }
    hits += 1;

    // I3：依赖环拒（b 依赖 c，c 依赖 b——经手装边）。
    let mut i3 = IncrBook::new()?;
    let a = i3.add_node("a", b"x", &[])?;
    let bb = i3.add_node("b", b"y", &[a])?;
    // 手工造环：a 依赖 b（add 时 b 不存在，事后补边）。
    i3.nodes[a].deps.push(bb);
    if i3.check_cycles() != Err(E_CYCLE) {
        return Err(E_CYCLE);
    }
    hits += 1;

    // I4：守恒注入——手工把 hits 虚增，audit 必抓红。
    let mut i4 = IncrBook::new()?;
    i4.add_node("a", b"1", &[])?;
    i4.build()?;
    i4.hits = 99; // 破账
    if i4.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：净节点无缓存行（E_FRESH）——缓存断裂检测。
    let mut i5 = IncrBook::new()?;
    i5.add_node("a", b"1", &[])?;
    i5.build()?;
    i5.cache.clear(); // 抽走缓存
    i5.add_node("b", b"2", &[])?; // 新脏节点
    if !matches!(i5.build(), Err(E_FRESH)) {
        return Err(E_FRESH); // a 净但无缓存行 → 账面断裂必红
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
