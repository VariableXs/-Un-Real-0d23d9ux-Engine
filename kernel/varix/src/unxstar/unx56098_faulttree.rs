//! UNX-F56098 管线运维手册件（故障树全文）（0x60C20 段）
//!
//! 锚点：UNX-F56098-J1 故障树全量（故障树节点逐个落账：节点号 × 症状 ×
//! 处置三元组）；J2 故障树语义（子节点必须挂父节点、根唯一、叶子无子——
//! 树结构三闸逐闸可验）；J3 手册审计链（节点指纹链、封账只读、遍历闭
//! 式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·运维手册面）。
//!
//! 结构：故障树节点登记册（节点号 → 症状 × 处置 × 父节点逐个落账）+
//! 树结构三闸（父存在/根唯一/无环）+ 遍历核（前序遍历闭式、叶计数）
//! + 指纹链 + 封账语义 + 注入核（空症状/父不存在/双根/自环父/重复节点/
//! 封账改写/空树遍历必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60C20 独占 0x20）。
pub const SEG_BASE: u32 = 0x60C20;
pub const FT_SYM_EMPTY: u32 = SEG_BASE + 0x01; // 症状为空
pub const FT_PARENT_MISS: u32 = SEG_BASE + 0x02; // 父节点不存在
pub const FT_ROOT_DUP: u32 = SEG_BASE + 0x03; // 双根（多根树拒绝）
pub const FT_SELF_LOOP: u32 = SEG_BASE + 0x04; // 自环父（节点父指自身）
pub const FT_NODE_DUP: u32 = SEG_BASE + 0x05; // 重复节点号
pub const FT_SEALED: u32 = SEG_BASE + 0x06; // 手册已封账拒写
pub const FT_EMPTY: u32 = SEG_BASE + 0x07; // 空树遍历

/// 根哨兵父号（0 = 无父）。
pub const ROOT_PARENT: u32 = 0;

/// 故障树节点：节点号 × 症状 × 处置 × 父节点。
#[derive(Clone)]
pub struct FaultNode {
    pub no: u32,
    pub symptom: String,
    pub action: String,
    pub parent: u32,
}

/// 手册总账。
pub struct ManualBook {
    pub nodes: Vec<FaultNode>,
    pub sealed: bool,
}

impl ManualBook {
    pub fn new() -> Self {
        ManualBook { nodes: Vec::new(), sealed: false }
    }

    fn find(&self, no: u32) -> Option<usize> {
        for i in 0..self.nodes.len() {
            if self.nodes[i].no == no {
                return Some(i);
            }
        }
        None
    }

    /// 节点落账：空症状拒 + 重复节点拒 + 父存在 + 自环拒四闸。
    pub fn add(&mut self, no: u32, symptom: &str, action: &str, parent: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FT_SEALED);
        }
        if symptom.is_empty() || action.is_empty() {
            return Err(FT_SYM_EMPTY);
        }
        if self.find(no).is_some() {
            return Err(FT_NODE_DUP);
        }
        if parent != ROOT_PARENT {
            if parent == no {
                return Err(FT_SELF_LOOP);
            }
            if self.find(parent).is_none() {
                return Err(FT_PARENT_MISS);
            }
        }
        self.nodes.push(FaultNode {
            no,
            symptom: symptom.to_string(),
            action: action.to_string(),
            parent,
        });
        Ok(())
    }

    /// J2：根唯一核（恰一节点 parent=0）。
    pub fn single_root(&self) -> bool {
        let mut roots = 0usize;
        for n in self.nodes.iter() {
            if n.parent == ROOT_PARENT {
                roots += 1;
            }
        }
        roots == 1
    }

    /// J2：无环核（逐节点沿父链上溯，步数 ≤ 节点数即无环）。
    pub fn acyclic(&self) -> bool {
        for n in self.nodes.iter() {
            let mut cur = n.parent;
            let mut steps = 0usize;
            while cur != ROOT_PARENT {
                match self.find(cur) {
                    Some(i) => cur = self.nodes[i].parent,
                    None => return false,
                }
                steps += 1;
                if steps > self.nodes.len() {
                    return false;
                }
            }
        }
        true
    }

    /// J2：树结构三闸合核。
    pub fn tree_ok(&self) -> bool {
        !self.nodes.is_empty() && self.single_root() && self.acyclic()
    }

    /// J2：前序遍历（从根沿子链：按节点号升序访问，父先子后）。
    pub fn walk(&self) -> Result<Vec<u32>, u32> {
        if self.nodes.is_empty() {
            return Err(FT_EMPTY);
        }
        let mut root: Option<u32> = None;
        for n in self.nodes.iter() {
            if n.parent == ROOT_PARENT {
                root = Some(n.no);
            }
        }
        let root = match root {
            Some(r) => r,
            None => return Err(FT_EMPTY),
        };
        let mut out: Vec<u32> = Vec::new();
        self.visit(root, &mut out);
        Ok(out)
    }

    fn visit(&self, no: u32, out: &mut Vec<u32>) {
        out.push(no);
        // 子节点按号升序访问。
        let mut kids: Vec<u32> = Vec::new();
        for n in self.nodes.iter() {
            if n.parent == no {
                kids.push(n.no);
            }
        }
        for i in 1..kids.len() {
            let key = kids[i];
            let mut j = i;
            while j > 0 && kids[j - 1] > key {
                kids[j] = kids[j - 1];
                j -= 1;
            }
            kids[j] = key;
        }
        for k in kids.iter() {
            self.visit(*k, out);
        }
    }

    /// J1：手册全量（逐节点症状处置非空）。
    pub fn nodes_ok(&self) -> bool {
        !self.nodes.is_empty()
            && self.nodes.iter().all(|n| !n.symptom.is_empty() && !n.action.is_empty())
    }

    /// 叶计数（无子节点数）。
    pub fn leaf_count(&self) -> usize {
        let mut leaves = 0usize;
        for n in self.nodes.iter() {
            let mut has_child = false;
            for m in self.nodes.iter() {
                if m.parent == n.no {
                    has_child = true;
                }
            }
            if !has_child {
                leaves += 1;
            }
        }
        leaves
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 遍历指纹（前序遍历复合）。
    pub fn walk_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        if let Ok(w) = self.walk() {
            for no in w.iter() {
                h ^= *no;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 标准夹具：三层故障树（根 1 → 子 2,3 → 叶 4,5）。
pub fn standard_book() -> ManualBook {
    let mut b = ManualBook::new();
    let plan: [(u32, &str, &str, u32); 5] = [
        (1, "管线总异常", "按子树逐项排查", ROOT_PARENT),
        (2, "采集器掉线", "查心跳与重启采集器", 1),
        (3, "数据链断裂", "跑完整性校验器重放", 1),
        (4, "心跳连超带", "扩容窗并重标定", 2),
        (5, "指纹复算漂移", "锁定篡改批并回滚", 3),
    ];
    for p in plan.iter() {
        if b.add(p.0, p.1, p.2, p.3).is_err() {
            panic!("standard fixture add must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空症状/父不存在/自环父/重复节点/双根/封账改写/空树遍历）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空症状
    let mut b = ManualBook::new();
    if b.add(1, "", "act", ROOT_PARENT) == Err(FT_SYM_EMPTY) {
        caught += 1;
    }
    // 2) 父不存在
    if b.add(1, "s", "a", 9) == Err(FT_PARENT_MISS) {
        caught += 1;
    }
    // 3) 自环父
    if b.add(1, "s", "a", ROOT_PARENT).is_ok() && b.add(2, "s2", "a2", 2) == Err(FT_SELF_LOOP) {
        caught += 1;
    }
    // 4) 重复节点
    if b.add(2, "s2", "a2", 1).is_ok() && b.add(2, "s3", "a3", 1) == Err(FT_NODE_DUP) {
        caught += 1;
    }
    // 5) 双根
    let mut b2 = ManualBook::new();
    if b2.add(1, "s", "a", ROOT_PARENT).is_ok() && b2.add(2, "s2", "a2", ROOT_PARENT).is_ok() {
        if !b2.single_root() {
            caught += 1;
        }
    }
    // 6) 空树遍历
    if ManualBook::new().walk() == Err(FT_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    let sb = standard_book();
    let mut sb2 = standard_book();
    sb2.seal();
    let _ = sb;
    if sb2.add(6, "late", "act", 1) == Err(FT_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 手册渲染导出（逐节点一行）。
pub fn render_book(b: &ManualBook) -> String {
    let mut out = String::new();
    for n in b.nodes.iter() {
        let p = if n.parent == ROOT_PARENT { "root".to_string() } else { format!("n{}", n.parent) };
        out.push_str(&format!("n{}|{}|{}|{}\n", n.no, n.symptom, p, n.action));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.split('|').count() == 4)
}

/// 判据入口（CheckSet）。
pub fn run_unx56098_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56098");
    // J1-1 故障树全量（五节点逐个合法）
    let b = standard_book();
    let ok = b.nodes_ok() && b.nodes.len() == 5;
    s.add("56098-J1-故障树全量", ok, if ok { "" } else { "节点残缺" });
    // J1-2 根唯一
    let ok = b.single_root() && b.nodes[0].parent == ROOT_PARENT;
    s.add("56098-J1-根唯一", ok, if ok { "" } else { "双根破树" });
    // J1-3 无环
    let ok = b.acyclic();
    s.add("56098-J1-无环", ok, if ok { "" } else { "环穿透" });
    // J2-1 前序遍历闭式（父先子后：1,2,4,3,5）
    let w = b.walk();
    let ok = match w {
        Ok(w) => w.len() == 5 && w[0] == 1 && w[1] == 2 && w[2] == 4 && w[3] == 3 && w[4] == 5,
        Err(_) => false,
    };
    s.add("56098-J2-前序遍历", ok, if ok { "" } else { "遍历破序" });
    // J2-2 叶计数（叶 4,5 = 2）
    let ok = b.leaf_count() == 2;
    s.add("56098-J2-叶计数", ok, if ok { "" } else { "叶计数漂移" });
    // J2-3 父存在闸（挂空父拒）
    let mut b2 = standard_book();
    let ok = b2.add(9, "s", "a", 99) == Err(FT_PARENT_MISS);
    s.add("56098-J2-父存在闸", ok, if ok { "" } else { "父闸失效" });
    // J2-4 自环闸
    let ok = b2.add(2, "s", "a", 2) == Err(FT_NODE_DUP)
        && ManualBook::new().add(1, "s", "a", 1) == Err(FT_SELF_LOOP);
    s.add("56098-J2-自环闸", ok, if ok { "" } else { "自环穿透" });
    // J3-1 遍历指纹确定性
    let ok = standard_book().walk_fingerprint() == standard_book().walk_fingerprint();
    s.add("56098-J3-遍历指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 指纹敏感（换父链指纹变）
    let mut b3 = ManualBook::new();
    let _ = b3.add(1, "管线总异常", "x", ROOT_PARENT);
    let _ = b3.add(2, "采集器掉线", "y", 1);
    let _ = b3.add(3, "数据链断裂", "z", 1);
    let _ = b3.add(4, "心跳连超带", "p", 3);
    let _ = b3.add(5, "指纹复算漂移", "q", 3);
    let ok = b3.walk_fingerprint() != standard_book().walk_fingerprint();
    s.add("56098-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-3 空树指纹为零复合仍可算
    let ok = ManualBook::new().walk_fingerprint() == 0x811C_9DC5;
    s.add("56098-J3-空树指纹", ok, if ok { "" } else { "空树语义漂移" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.add(6, "late", "act", 1) == Err(FT_SEALED);
    s.add("56098-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出五行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 5) && text.contains("n1|管线总异常|root|按子树逐项排查");
    s.add("56098-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56098-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56098_all_checks_green() {
        let set = run_unx56098_checks();
        assert!(set.all_passed(), "UNX-F56098 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56098_walk_stable() {
        assert_eq!(standard_book().walk_fingerprint(), standard_book().walk_fingerprint());
    }
}
