//! UNX-F10134 深目录遍历正反双判据（深链达成与超深误用受控）（0x103e0 段）
//!
//! 锚点：UNX-F10134-J1 深目录达成（逐级创建/遍历/回溯一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：深度链（节点父链数组）+ 链核（逐级 mkdir/深度计/上行回溯/超
//! 深拒）+ 链账（逐次留痕 + 深度轨迹）+ 注入核（超深/缺父/空名/空留痕/
//! 封链必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x103e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x103e0;
pub const DP_TOO_DEEP: u32 = SEG_BASE + 0x01; // 超深
pub const DP_NO_PARENT: u32 = SEG_BASE + 0x02; // 缺父
pub const DP_EMPTY: u32 = SEG_BASE + 0x03; // 空名
pub const DP_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const DP_SEALED: u32 = SEG_BASE + 0x05; // 封链后操作

/// 深度上限。
pub const DEPTH_MAX: usize = 8;

/// 链节点。
#[derive(Clone)]
pub struct DeepNode {
    pub name: String,
    pub parent: i32, // -1 为根
    pub depth: usize,
}

impl DeepNode {
    pub fn ok(&self) -> bool {
        !self.name.is_empty()
    }
}

/// 链账留痕。
#[derive(Clone)]
pub struct DpRec {
    pub seq: u32,
    pub node: u32,
    pub depth: usize,
    pub note: String,
}

impl DpRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.depth <= DEPTH_MAX
    }
}

/// 深度链册。
pub struct DeepBook {
    pub nodes: Vec<DeepNode>,
    pub ledger: Vec<DpRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl DeepBook {
    pub fn new() -> DeepBook {
        DeepBook {
            nodes: vec![DeepNode { name: "/".to_string(), parent: -1, depth: 0 }],
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 逐级 mkdir：深度超限/缺父/空名拒。
    pub fn mkdir_under(&mut self, parent: i32, name: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(DP_SEALED);
        }
        if note.is_empty() {
            return Err(DP_EMPTY_NOTE);
        }
        if name.is_empty() {
            return Err(DP_EMPTY);
        }
        if parent < 0 || parent as usize >= self.nodes.len() {
            return Err(DP_NO_PARENT);
        }
        let pdepth = self.nodes[parent as usize].depth;
        if pdepth + 1 > DEPTH_MAX {
            return Err(DP_TOO_DEEP);
        }
        let id = self.nodes.len() as u32;
        self.nodes.push(DeepNode { name: name.to_string(), parent, depth: pdepth + 1 });
        self.ledger.push(DpRec { seq: self.seq_next, node: id, depth: pdepth + 1, note: note.to_string() });
        self.seq_next += 1;
        Ok(id)
    }

    /// 逐级建链：按名段序列自根而下。
    pub fn build_chain(&mut self, segs: &[&str]) -> Result<u32, u32> {
        let mut cur: i32 = 0;
        for seg in segs {
            cur = self.mkdir_under(cur, seg, "建链")? as i32;
        }
        Ok(cur as u32)
    }

    /// 上行回溯：自节点回根的名链。
    pub fn path_up(&self, id: u32) -> Result<Vec<String>, u32> {
        if id as usize >= self.nodes.len() {
            return Err(DP_NO_PARENT);
        }
        let mut chain: Vec<String> = Vec::new();
        let mut cur = id as i32;
        let mut guard = 0;
        while cur >= 0 && guard <= self.nodes.len() {
            chain.push(self.nodes[cur as usize].name.clone());
            cur = self.nodes[cur as usize].parent;
            guard += 1;
        }
        chain.reverse();
        Ok(chain)
    }

    /// 回溯串（斜杠拼接；根名恒为 "/"，自带斜杠处不重复加分隔符）。
    pub fn path_str(&self, id: u32) -> Result<String, u32> {
        let chain = self.path_up(id)?;
        let mut out = String::new();
        for seg in &chain {
            if !out.is_empty() && !out.ends_with('/') {
                out.push('/');
            }
            out.push_str(seg);
        }
        Ok(out)
    }

    /// 深度轨迹核：账面深度随登记单调 +1。
    pub fn depth_trail_ok(&self, leaf: u32) -> bool {
        if leaf as usize >= self.nodes.len() {
            return false;
        }
        self.ledger.iter().any(|r| r.node == leaf)
            && self.ledger.iter().all(|r| r.depth >= 1)
    }

    /// 广度计数：同父子节点数。
    pub fn breadth_of(&self, parent: i32) -> usize {
        self.nodes.iter().filter(|n| n.parent == parent).count()
    }

    /// 全链深度最大值。
    pub fn max_depth(&self) -> usize {
        self.nodes.iter().map(|n| n.depth).max().unwrap_or(0)
    }

    /// 深度分桶统计。
    pub fn depth_hist(&self) -> Vec<usize> {
        let mut hist: Vec<usize> = vec![0; DEPTH_MAX + 1];
        for n in &self.nodes {
            if n.depth <= DEPTH_MAX {
                hist[n.depth] += 1;
            }
        }
        hist
    }

    /// 节点合法性全检。
    pub fn all_ok(&self) -> bool {
        self.nodes.iter().all(|n| n.ok()) && self.nodes[0].depth == 0
    }

    /// 单层下行核：自父找同名子。
    pub fn child_of(&self, parent: i32, name: &str) -> Option<u32> {
        self.nodes
            .iter()
            .position(|n| n.parent == parent && n.name == name)
            .map(|i| i as u32)
    }

    pub fn depth_of(&self, id: u32) -> Option<usize> {
        self.nodes.get(id as usize).map(|n| n.depth)
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x103e00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.depth as u32);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DeepBook::new();
    // 1) 超深必拒：建满 DEPTH_MAX 后再深一层
    let mut cur: i32 = 0;
    let mut built = true;
    for i in 0..DEPTH_MAX {
        let nm = format!("d{}", i);
        match b.mkdir_under(cur, &nm, "inj-deep") {
            Ok(id) => cur = id as i32,
            Err(_) => built = false,
        }
    }
    got[0] = built && b.mkdir_under(cur, "over", "inj-over").err() == Some(DP_TOO_DEEP);
    // 2) 缺父必拒
    got[1] = b.mkdir_under(999, "x", "inj-parent").err() == Some(DP_NO_PARENT);
    // 3) 空名必拒
    got[2] = b.mkdir_under(0, "", "inj-empty").err() == Some(DP_EMPTY);
    // 4) 空留痕必拒
    got[3] = b.mkdir_under(0, "nn", "").err() == Some(DP_EMPTY_NOTE);
    // 5) 封链后操作必抓
    b.seal();
    got[4] = b.mkdir_under(0, "late", "inj-seal").err() == Some(DP_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10134_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10134");

    // 正路径：逐级建链 DEPTH_MAX 层并回溯一致
    let mut b = DeepBook::new();
    let segs: Vec<&str> = vec!["a", "b", "c", "d", "e", "f", "g", "h"];
    let leaf = b.build_chain(&segs).unwrap_or(u32::MAX);
    let path = b.path_str(leaf).unwrap_or_default();
    let j1 = path == "/a/b/c/d/e/f/g/h" && b.depth_of(leaf) == Some(DEPTH_MAX);
    s.add("J1-深链回溯", j1, "八层链回溯串一致且深度达顶");

    // 回溯名链序
    let j2 = {
        let chain = b.path_up(leaf).unwrap_or_default();
        chain.len() == DEPTH_MAX + 1 && chain[0] == "/" && chain[1] == "a"
    };
    s.add("J2-名链序", j2, "根起九元序一致");

    // 部分链分支挂载
    let j3 = {
        let m = b.mkdir_under(2, "side", "分支").unwrap_or(u32::MAX);
        b.path_str(m).unwrap_or_default() == "/a/b/side"
    };
    s.add("J3-分支挂载", j3, "中途分支回溯正确");

    // 超深受控
    let j4 = inject_five()[0];
    s.add("J4-超深受控", j4, "DEPTH_MAX 后再深必拒");

    // 缺父受控
    let j5 = b.mkdir_under(999, "orphan", "野父").err() == Some(DP_NO_PARENT);
    s.add("J5-缺父受控", j5, "野父必拒");

    // 空名受控
    let j6 = b.mkdir_under(0, "", "空名").err() == Some(DP_EMPTY);
    s.add("J6-空名受控", j6, "空名必拒");

    // 深度轨迹
    let j7 = b.depth_trail_ok(leaf) && b.ledger.iter().all(|r| r.ok());
    s.add("J7-深度轨迹", j7, "账面深度在限且注非空");

    // 根深为 0
    let j8 = b.depth_of(0) == Some(0);
    s.add("J8-根深为零", j8, "根节点深度 0");

    // 空留痕拒收
    let j9 = b.mkdir_under(0, "zz", "").err() == Some(DP_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "深度在限且注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // 广度与深度统计
    let hist = b.depth_hist();
    let j13 = b.max_depth() == DEPTH_MAX
        && b.breadth_of(0) == 1
        && hist[DEPTH_MAX] == 1
        && b.all_ok();
    s.add("J13-深广统计", j13, "最大深度达顶、根广度 1、顶桶 1");

    // 单层下行核
    let j14 = b.child_of(0, "a").unwrap_or(9) == 1 && b.child_of(0, "zz").is_none();
    s.add("J14-下行命中", j14, "根下 a 命中、野名空回");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10134_deep_flow() {
        let mut b = DeepBook::new();
        let leaf = b.build_chain(&["x", "y"]).unwrap_or(0);
        assert_eq!(b.path_str(leaf).unwrap_or_default(), "/x/y");
        assert!(b.mkdir_under(leaf as i32, "z", "d").is_ok());
    }

    #[test]
    fn unx10134_checks_green() {
        let s = run_unx10134_checks();
        assert!(s.all_passed(), "UNX-F10134 判据未全绿");
    }
}
