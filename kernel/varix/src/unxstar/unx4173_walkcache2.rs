//! unx4173_walkcache2 — UNX-F4173 walk 与 dentry 缓存联动（B05/F4090 接口消费）（诊断段 0x9740）。
//!
//! 判据原文：
//!   J1 UNX-F4173-J1 全命中负载下 fs 穿透=0（F4090 接口复用断言），部分
//!      命中负载穿透数=未命中分量数（逐分量对账一致）。
//! 设计纪律：
//!   1. dentry 缓存键=(父槽,分量名)，命中零穿透；未命中恰走一次 fs
//!      lookup（穿透 +1）并回填缓存——F4090 下游接口只此一条消费路。
//!   2. 逐分量对账：穿透数与未命中分量数必须相等（分量级对账，非路径级）。
//!   3. 缓存容量 64 槽定容，满则按轮转槽位逐出（旧槽标 Evicted），
//!      逐出后再问即重新穿透——账面可复演。
//!   4. 探测账守恒：hash 探测数 = 命中数 + 未命中数，一分量一探测。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9740;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_ABS: u16 = SEG_BASE + 1;   // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 2;  // 空分量
const E_NOENT: u16 = SEG_BASE + 3;     // fs 与缓存双未命中
const E_BAD_NODE: u16 = SEG_BASE + 4;  // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 5;  // 同目录重名
const E_NOT_DIR: u16 = SEG_BASE + 6;   // 中间分量非目录
const E_LEDGER: u16 = SEG_BASE + 7;    // 对账失平（穿透≠未命中）

pub const CACHE_CAP: usize = 64; // 缓存槽数（定容，防无限膨胀）
pub const ROOT: usize = 0;       // 根槽位

/// 下游假 fs 节点（F4090 接口消费的供给侧）。
pub struct FsNode {
    pub name: String,         // 分量名
    pub is_dir: bool,         // 目录位
    pub children: Vec<usize>, // 子槽位
}

/// 假 fs：arena 树 + lookup 穿透账（穿透数即判据 J1 核心账）。
pub struct FakeFs {
    pub nodes: Vec<FsNode>,
    pub fs_lookups: u32, // fs 穿透数
}

impl FakeFs {
    pub fn new() -> FakeFs {
        FakeFs {
            nodes: vec![FsNode {
                name: String::from("/"),
                is_dir: true,
                children: Vec::new(),
            }],
            fs_lookups: 0,
        }
    }

    /// 建点：重名拒收（树账防线）。
    pub fn add(&mut self, parent: usize, name: &str, is_dir: bool) -> Result<usize, u16> {
        if parent >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        if name.is_empty() || name.contains('/') {
            return Err(E_BAD_COMP);
        }
        for &c in &self.nodes[parent].children {
            if c < self.nodes.len() && self.nodes[c].name == name {
                return Err(E_DUP_NAME);
            }
        }
        let id = self.nodes.len();
        self.nodes.push(FsNode {
            name: name.to_string(),
            is_dir,
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    /// fs lookup：唯一穿透口，进入 fs 即穿透 +1（命中与否都算）。
    pub fn lookup(&mut self, parent: usize, name: &str) -> Result<usize, u16> {
        self.fs_lookups += 1;
        if parent >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        for &c in &self.nodes[parent].children {
            if c < self.nodes.len() && self.nodes[c].name == name {
                return Ok(c);
            }
        }
        Err(E_NOENT)
    }
}

/// 缓存槽状态机：Empty→Valid→（逐出）Evicted→（槽位复用）Valid。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntState {
    Empty,
    Valid,
    Evicted,
}

/// 缓存槽：键=(parent, name)，值=解析落点。
#[derive(Clone)]
pub struct CacheEnt {
    pub parent: usize,
    pub name: String,
    pub node: usize,
    pub state: EntState,
}

/// 缓存面状态机：Cold（零问询）→Warming（只有未命中）→Warm（出现过命中）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceState {
    Cold,
    Warming,
    Warm,
}

/// dentry 缓存：定容轮转槽 + 探测/命中/未命中三分账。
pub struct WalkCache {
    pub ents: Vec<CacheEnt>,
    pub fill_seq: u32,    // 回填序号（轮转逐出用）
    pub hash_probes: u32, // hash 探测数（一分量一探测）
    pub hits: u32,
    pub misses: u32,
    pub evictions: u32,
}

impl WalkCache {
    pub fn new() -> WalkCache {
        WalkCache {
            ents: vec![
                CacheEnt {
                    parent: 0,
                    name: String::new(),
                    node: 0,
                    state: EntState::Empty,
                };
                CACHE_CAP
            ],
            fill_seq: 0,
            hash_probes: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    /// 缓存探测：键全等才算命中（父槽 + 分量名双键）。
    fn probe(&self, parent: usize, name: &str) -> Option<usize> {
        for i in 0..self.ents.len() {
            if self.ents[i].state == EntState::Valid
                && self.ents[i].parent == parent
                && self.ents[i].name == name
            {
                return Some(i);
            }
        }
        None
    }

    /// 回填：轮转槽位，撞 Valid 槽即逐出（标 Evicted 由复用覆盖）。
    fn fill(&mut self, parent: usize, name: &str, node: usize) {
        let slot = (self.fill_seq as usize) % CACHE_CAP;
        if self.ents[slot].state == EntState::Valid {
            self.evictions += 1;
        }
        self.ents[slot] = CacheEnt {
            parent,
            name: name.to_string(),
            node,
            state: EntState::Valid,
        };
        self.fill_seq += 1;
    }

    /// 缓存面状态机读口。
    pub fn face(&self) -> FaceState {
        if self.hits == 0 && self.misses == 0 {
            FaceState::Cold
        } else if self.hits == 0 {
            FaceState::Warming
        } else {
            FaceState::Warm
        }
    }

    /// 带缓存 walk：逐分量先探缓存，未命中恰穿透一次 fs 并回填。
    pub fn walk(&mut self, fs: &mut FakeFs, path: &str) -> Result<usize, u16> {
        if !path.starts_with('/') {
            return Err(E_NOT_ABS);
        }
        let body = &path[1..]; // '/' 单字节，前缀判定后切片安全
        let mut cur = ROOT;
        if body.is_empty() {
            return Ok(cur);
        }
        for comp in body.split('/') {
            if comp.is_empty() {
                return Err(E_BAD_COMP);
            }
            if cur >= fs.nodes.len() || !fs.nodes[cur].is_dir {
                return Err(E_NOT_DIR);
            }
            self.hash_probes += 1;
            match self.probe(cur, comp) {
                Some(slot) => {
                    self.hits += 1;
                    cur = self.ents[slot].node;
                }
                None => {
                    self.misses += 1;
                    let node = fs.lookup(cur, comp)?; // 唯一穿透口
                    self.fill(cur, comp, node);
                    cur = node;
                }
            }
        }
        Ok(cur)
    }

    /// 守恒：探测=命中+未命中（逐分量对账的账面基座）。
    pub fn probes_conserved(&self) -> bool {
        self.hits + self.misses == self.hash_probes
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "fs 与缓存双未命中",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_NOT_DIR => "中间分量非目录",
        E_LEDGER => "对账失平（穿透≠未命中）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：冷走穿透对账→全命中零穿透→部分命中逐分量对账→逐出复演→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建 fs 树 /a/(b/(c,d))，缓存独立成账，初始面 Cold。
    let mut fs = FakeFs::new();
    let a = fs.add(ROOT, "a", true)?;
    let b = fs.add(a, "b", true)?;
    let c = fs.add(b, "c", false)?;
    let d = fs.add(b, "d", false)?;
    let mut wc = WalkCache::new();
    if !matches!(wc.face(), FaceState::Cold) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2：冷走 /a/b/c——三分量全未命中，穿透增量=3=未命中增量（逐分量对账）。
    let p0 = fs.fs_lookups;
    let m0 = wc.misses;
    let r = wc.walk(&mut fs, "/a/b/c")?;
    if r != c {
        return Err(E_LEDGER);
    }
    if fs.fs_lookups - p0 != 3 || wc.misses - m0 != 3 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：全命中负载——同路径重走，穿透增量=0（F4090 接口复用断言）。
    let p1 = fs.fs_lookups;
    let r = wc.walk(&mut fs, "/a/b/c")?;
    if r != c {
        return Err(E_LEDGER);
    }
    if fs.fs_lookups != p1 || wc.hits < 3 {
        return Err(E_LEDGER);
    }
    if !matches!(wc.face(), FaceState::Warm) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：部分命中负载——/a/b/d 前缀两格命中，仅 d 未命中：穿透增量=1=未命中增量。
    let p2 = fs.fs_lookups;
    let m2 = wc.misses;
    let r = wc.walk(&mut fs, "/a/b/d")?;
    if r != d {
        return Err(E_LEDGER);
    }
    if fs.fs_lookups - p2 != 1 || wc.misses - m2 != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：轮转逐出——回填 64 个杂散分量挤转全槽，旧路径重走重新穿透 3 次。
    let mut i = 0usize;
    while i < CACHE_CAP {
        let mut name = String::from("z");
        // 两位十进制序号（纯整数转字符，免动态格式化）
        name.push((b'0' + (i / 10) as u8) as char);
        name.push((b'0' + (i % 10) as u8) as char);
        fs.add(ROOT, &name, false)?;
        let mut p = String::from("/");
        p.push_str(&name);
        wc.walk(&mut fs, &p)?;
        i += 1;
    }
    let p3 = fs.fs_lookups;
    let r = wc.walk(&mut fs, "/a/b/c")?;
    if r != c {
        return Err(E_LEDGER);
    }
    if fs.fs_lookups - p3 != 3 || wc.evictions == 0 {
        return Err(E_LEDGER); // 逐出后必须重新穿透
    }
    hits += 1;

    // R6：守恒终审——探测=命中+未命中。
    if !wc.probes_conserved() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：相对路径拒收。
    if wc.walk(&mut fs, "a/b") != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    // I2：空分量拒收。
    if wc.walk(&mut fs, "/a//b") != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    // I3：不存在分量——未命中也真实穿透 1 次后报 E_NOENT（对账含失败路）。
    let p4 = fs.fs_lookups;
    if wc.walk(&mut fs, "/a/none") != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if fs.fs_lookups - p4 != 1 {
        return Err(E_NOENT);
    }
    // I4：普通文件作中间目录用 → E_NOTDIR。
    if wc.walk(&mut fs, "/a/b/c/x") != Err(E_NOT_DIR) {
        return Err(E_NOT_DIR);
    }
    hits += 1;

    Ok(hits)
}
