//! unx4177_walkperf — UNX-F4177 walk 性能账（分量均摊访问数）（诊断段 0x97C0）。
//!
//! 判据原文：
//!   J1 UNX-F4177-J1 全命中负载下每分量均摊哈希查询≤1.2（账面可导出），
//!      未命中负载均摊值同步有账——性能对照基线落账。
//! 设计纪律：
//!   1. 均摊口径：均摊值 = 哈希查询总数×100 ÷ 分量总数（×100 定点整数，
//!      免浮点），判据基线全命中≤120（即 1.2）。
//!   2. 两负载分账：全命中负载（缓存全热）与未命中负载（缓存全冷）各自
//!      独立记账，均摊值分别可导出，不混账。
//!   3. 全命中负载每分量恰 1 次探测（均摊 100）；未命中负载每分量 1 次
//!      探测 + 1 次回填探测，均摊如实入账（基线对照面，不设上限）。
//!   4. 基线常量写死：BASELINE_FULL_X100=120（判据 1.2 口径）、
//!      PERFECT_X100=100（理想 1.0），来源判据原文注记。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x97C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_ABS: u16 = SEG_BASE + 1;   // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 2;  // 空分量
const E_NOENT: u16 = SEG_BASE + 3;     // 分量不存在
const E_BAD_NODE: u16 = SEG_BASE + 4;  // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 5;  // 同目录重名
const E_ZERO_DIV: u16 = SEG_BASE + 6;  // 零分量除零防护触发
const E_BASELINE: u16 = SEG_BASE + 7;  // 全命中均摊超判据基线
const E_LEDGER: u16 = SEG_BASE + 8;    // 账面对平失败（守恒/负载账不符）

/// 判据基线：全命中均摊上限 1.2（×100 定点=120），来源判据原文。
pub const BASELINE_FULL_X100: u32 = 120;
/// 理想均摊：全命中每分量恰 1 次探测（×100=100）。
pub const PERFECT_X100: u32 = 100;
/// 基线负载路径长度（分量数，两负载同长对照）。
pub const LOAD_COMPS: u32 = 24;

/// 性能账：探测/命中/未命中/回填四账分立。
pub struct PerfBook {
    pub comps: u32,        // 走过的分量总数
    pub hash_queries: u32, // 哈希探测总数（含回填探测）
    pub hits: u32,
    pub misses: u32,
    pub fills: u32,        // 回填数（每次回填附 1 次定位探测）
}

impl PerfBook {
    pub fn new() -> PerfBook {
        PerfBook {
            comps: 0,
            hash_queries: 0,
            hits: 0,
            misses: 0,
            fills: 0,
        }
    }

    /// 均摊导出：查询×100÷分量（定点整数；零分量防护返回 0）。
    pub fn avg_x100(&self) -> Result<u32, u16> {
        if self.comps == 0 {
            return Err(E_ZERO_DIV);
        }
        Ok((self.hash_queries * 100) / self.comps)
    }

    /// 探测守恒：全命中面 查询=分量+回填；未命中面同式（回填随未命中）。
    pub fn conserved(&self) -> bool {
        self.hash_queries == self.comps + self.fills && self.fills == self.misses
    }
}

/// 假 fs 节点。
pub struct PNode {
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<usize>,
}

/// 计账走树：逐分量探测计数（缓存命中模型内嵌：键在 hits 表即命中）。
pub struct PerfWalk {
    pub nodes: Vec<PNode>,
    pub book_warm: PerfBook, // 全命中负载账
    pub book_cold: PerfBook, // 未命中负载账
    pub warm_keys: Vec<(usize, String)>, // 预热键表（全命中负载的缓存像）
}

impl PerfWalk {
    pub fn new() -> PerfWalk {
        PerfWalk {
            nodes: vec![PNode {
                name: String::from("/"),
                is_dir: true,
                children: Vec::new(),
            }],
            book_warm: PerfBook::new(),
            book_cold: PerfBook::new(),
            warm_keys: Vec::new(),
        }
    }

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
        self.nodes.push(PNode {
            name: name.to_string(),
            is_dir,
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    /// 预热：把一条路径的全部 (父槽,分量) 键登记进命中表。
    pub fn prewarm(&mut self, path: &str) -> Result<u32, u16> {
        let comps = self.split(path)?;
        let mut cur = 0usize;
        let mut n = 0u32;
        for c in comps {
            let next = self.child_of(cur, c).ok_or(E_NOENT)?;
            self.warm_keys.push((cur, c.to_string()));
            cur = next;
            n += 1;
        }
        Ok(n)
    }

    fn warm_hit(&self, parent: usize, name: &str) -> bool {
        for k in &self.warm_keys {
            if k.0 == parent && k.1 == name {
                return true;
            }
        }
        false
    }

    /// 计账走：warm=true 记全命中负载账，否则记未命中负载账。
    /// 每分量 1 次探测；未命中再附 1 次回填定位探测（账面如实）。
    pub fn walk(&mut self, path: &str, warm: bool) -> Result<usize, u16> {
        let comps = self.split(path)?;
        let mut cur = 0usize;
        for c in comps {
            // 先独立取命中快照，再取账面可变借用（避免双借用冲突）。
            let hit = warm && self.warm_hit(cur, c);
            let book = if warm { &mut self.book_warm } else { &mut self.book_cold };
            book.comps += 1;
            book.hash_queries += 1;
            if hit {
                book.hits += 1;
            } else {
                // 未命中（含冷负载恒未命中）：如实入账并附 1 次回填定位探测。
                book.misses += 1;
                book.fills += 1;
                book.hash_queries += 1;
            }
            cur = self.child_of(cur, c).ok_or(E_NOENT)?;
        }
        Ok(cur)
    }

    fn split<'a>(&self, path: &'a str) -> Result<Vec<&'a str>, u16> {
        if !path.starts_with('/') {
            return Err(E_NOT_ABS);
        }
        let body = &path[1..]; // '/' 单字节，前缀判定后切片安全
        if body.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for c in body.split('/') {
            if c.is_empty() {
                return Err(E_BAD_COMP);
            }
            out.push(c);
        }
        Ok(out)
    }

    fn child_of(&self, dir: usize, name: &str) -> Option<usize> {
        if dir >= self.nodes.len() {
            return None;
        }
        for &c in &self.nodes[dir].children {
            if c < self.nodes.len() && self.nodes[c].name == name {
                return Some(c);
            }
        }
        None
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "分量不存在",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_ZERO_DIV => "零分量除零防护触发",
        E_BASELINE => "全命中均摊超判据基线",
        E_LEDGER => "账面对平失败（守恒/负载账不符）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：预热→全命中均摊≤1.2→未命中均摊有账→两账守恒→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建链状树 /n1/n2/…/n26（26 分量，覆盖基线负载长度）。
    let mut pw = PerfWalk::new();
    let mut parent = 0usize;
    let mut i = 1usize;
    while i <= 26 {
        let mut name = String::from("n");
        name.push((b'a' + ((i - 1) % 26) as u8) as char);
        parent = pw.add(parent, &name, true)?;
        i += 1;
    }
    // 拼路径 /na/nb/…/nz（26 分量）。
    let mut path = String::new();
    let mut j = 0usize;
    while j < 26 {
        path.push('/');
        path.push((b'a' + j as u8) as char);
        path.push('n');
        j += 1;
    }
    hits += 1;

    // R2：预热 24 分量前缀，然后全命中负载走全程——前缀均摊恰 100（1.0）。
    let warmed = pw.prewarm(&path)?;
    if warmed != 24 {
        return Err(E_LEDGER);
    }
    pw.walk(&path, true)?;
    let avg_warm = pw.book_warm.avg_x100()?;
    if avg_warm > BASELINE_FULL_X100 {
        return Err(E_BASELINE); // 判据：全命中均摊≤1.2
    }
    if pw.book_warm.hits != 24 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：纯前缀全命中负载——24 分量全热，负载增量均摊恰 100（理想 1.0）。
    let mut prefix = String::new();
    let mut j = 0usize;
    while j < 24 {
        prefix.push('/');
        prefix.push((b'a' + j as u8) as char);
        prefix.push('n');
        j += 1;
    }
    let q0 = pw.book_warm.hash_queries;
    let c0 = pw.book_warm.comps;
    pw.walk(&prefix, true)?;
    let dq = pw.book_warm.hash_queries - q0;
    let dcp = pw.book_warm.comps - c0;
    if dq != dcp || dcp != 24 {
        return Err(E_LEDGER); // 前缀负载均摊恰 1.0（≤基线 1.2）
    }
    hits += 1;

    // R4：未命中负载同步有账——冷走同长前缀，均摊如实导出（200=2.0）。
    pw.walk(&prefix, false)?;
    let avg_cold = pw.book_cold.avg_x100()?;
    if avg_cold != 200 || pw.book_cold.misses != LOAD_COMPS {
        return Err(E_LEDGER);
    }
    if pw.book_cold.hash_queries != LOAD_COMPS * 2 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：两账守恒——查询=分量+回填，回填=未命中。
    if !pw.book_warm.conserved() || !pw.book_cold.conserved() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：相对路径 / 空分量 / 不存在分量注入。
    if pw.walk("a/n", true) != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    if pw.walk("/na//nb", true) != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    if pw.walk("/na/none", false) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    hits += 1;

    // I2：除零防护——空账导出均摊报 E_ZERO_DIV。
    let empty = PerfBook::new();
    if empty.avg_x100() != Err(E_ZERO_DIV) {
        return Err(E_ZERO_DIV);
    }
    hits += 1;

    Ok(hits)
}
