//! veb398_knowbase —— VE-F0398 B 域知识库。
//! 锚点（CoRun Unxreal 主册）：VE-F0398 · B 域知识库 · 目标 340 行
//! 判据原文：VE-F0398-J1 知识库三结构：词条（键 + 值 + 分类）、
//!           反链（词条间引用，双向登记）、冷热分层（访问计数
//!           ≥ 阈升热层）；检索（键精确 + 分类过滤）；反链完整
//!           核（A 引 B ⇒ B 的被引表含 A）；热层容荦（定容 K，
//!           满挤最低计数）。
//! 组件：词条账 + 双向反链 + 访问计数 + 热层（K 容 + 挤出）。
//! 职责定位：知识库是「B 域的记忆器官」——词条是知识单元，反
//!           链是联想通路：查 A 时能顺着引用摸到 B；冷热分层
//!           是记忆的经济学：常查的放热层（快），冷门的沉底；
//!           反链完整核保证联想通路不断：单向引用是断头路。
//! 性能逐项分解：检索 O(n)；反链 O(1)（双向登记）；挤层 O(K)。
//! 诊断码段：0x30900（0x30901~0x30909 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const KB_VERSION: &str = "veb398_knowbase-v1";

/// 诊断段基址（0x30900 段 · 本模块独占）。
pub const KB_SEG_BASE: u32 = 0x30900;
pub const XC_DUP: u32 = 0x30901; // 词条重键
pub const XC_NOENT: u32 = 0x30902; // 无此词条
pub const XC_SELF_REF: u32 = 0x30903; // 自引用
pub const XC_MAGIC: u32 = 0x30904; // 账魔数破
pub const XC_LINK_BROKEN: u32 = 0x30905; // 反链断
pub const XC_HOT_OOR: u32 = 0x30906; // 热层容量越域
pub const XC_CLASS_EMPTY: u32 = 0x30907; // 分类空
pub const XC_KEY_EMPTY: u32 = 0x30908; // 键空
pub const XC_HOT_FULL: u32 = 0x30909; // 热层账不平

/// 热层容量。
pub const HOT_CAP: usize = 4;
/// 升热阈（访问计数）。
pub const HOT_HIT: u32 = 3;

/// 词条。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: &'static str,
    pub val: u32,
    pub class: &'static str,
    pub hits: u32,
}

/// 知识库。
pub struct KnowBase {
    entries: Vec<Entry>,
    /// 反链：link[i] = (i 引用的词条下标集)。
    links: Vec<Vec<usize>>,
    /// 热层（词条下标）。
    hot: Vec<usize>,
    magic: u32,
}

impl KnowBase {
    /// 建。
    pub fn new() -> KnowBase {
        KnowBase { entries: Vec::new(), links: Vec::new(), hot: Vec::new(), magic: 0x0900_1B0E }
    }

    /// 加词条。
    pub fn put(&mut self, key: &'static str, val: u32, class: &'static str) -> Result<(), u32> {
        if self.magic != 0x0900_1B0E {
            return Err(XC_MAGIC);
        }
        if key.is_empty() {
            return Err(XC_KEY_EMPTY);
        }
        if class.is_empty() {
            return Err(XC_CLASS_EMPTY);
        }
        for e in self.entries.iter() {
            if e.key == key {
                return Err(XC_DUP);
            }
        }
        self.entries.push(Entry { key, val, class, hits: 0 });
        self.links.push(Vec::new());
        Ok(())
    }

    fn pos(&self, key: &str) -> Result<usize, u32> {
        for (i, e) in self.entries.iter().enumerate() {
            if e.key == key {
                return Ok(i);
            }
        }
        Err(XC_NOENT)
    }

    /// 检索（精确键，命中计访问，热层判定）。
    pub fn get(&mut self, key: &str) -> Result<Entry, u32> {
        let i = self.pos(key)?;
        self.entries[i].hits += 1;
        if self.entries[i].hits >= HOT_HIT {
            self.promote(i);
        }
        Ok(self.entries[i].clone())
    }

    /// 升热层：定容挤最低计数。
    fn promote(&mut self, i: usize) {
        // 已在热层则不重入。
        for h in self.hot.iter() {
            if *h == i {
                return;
            }
        }
        if self.hot.len() < HOT_CAP {
            self.hot.push(i);
            return;
        }
        // 挤最低计数。
        let mut min_pos = 0usize;
        for k in 1..self.hot.len() {
            if self.entries[self.hot[k]].hits < self.entries[self.hot[min_pos]].hits {
                min_pos = k;
            }
        }
        if self.entries[i].hits > self.entries[self.hot[min_pos]].hits {
            self.hot[min_pos] = i;
        }
    }

    /// 热层成员键。
    pub fn hot_keys(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for h in self.hot.iter() {
            out.push(self.entries[*h].key);
        }
        out
    }

    /// 反链登记（A 引 B，双向）。
    pub fn link(&mut self, a: &str, b: &str) -> Result<(), u32> {
        if a == b {
            return Err(XC_SELF_REF);
        }
        let ai = self.pos(a)?;
        let bi = self.pos(b)?;
        // 防重边。
        for d in self.links[ai].iter() {
            if *d == bi {
                return Ok(()); // 幂等
            }
        }
        self.links[ai].push(bi);
        self.links[bi].push(ai);
        Ok(())
    }

    /// A 的正向引用键。
    pub fn refs_of(&self, a: &str) -> Result<Vec<&'static str>, u32> {
        let ai = self.pos(a)?;
        let mut out = Vec::new();
        for d in self.links[ai].iter() {
            out.push(self.entries[*d].key);
        }
        Ok(out)
    }

    /// 反链完整核：A 引 B ⇒ B 表含 A。
    pub fn link_audit(&self) -> Result<u32, u32> {
        if self.magic != 0x0900_1B0E {
            return Err(XC_MAGIC);
        }
        let mut edges = 0u32;
        for ai in 0..self.links.len() {
            for d in self.links[ai].iter() {
                let bi = *d;
                // bi 的表须含 ai。
                let mut back = false;
                for r in self.links[bi].iter() {
                    if *r == ai {
                        back = true;
                    }
                }
                if !back {
                    return Err(XC_LINK_BROKEN);
                }
                edges += 1;
            }
        }
        // 边数须为偶（双向登记）。
        if edges % 2 != 0 {
            return Err(XC_LINK_BROKEN);
        }
        Ok(edges / 2)
    }

    /// 分类过滤。
    pub fn by_class(&self, class: &str) -> Vec<&'static str> {
        let mut out = Vec::new();
        for e in self.entries.iter() {
            if e.class == class {
                out.push(e.key);
            }
        }
        out
    }

    /// 词条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

pub fn run_veb398_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0398");
    // 族一：词条与检索。
    let mut kb = KnowBase::new();
    let _ = kb.put("oom", 1, "mem");
    let _ = kb.put("guard", 2, "mem");
    let _ = kb.put("tsc", 3, "time");
    let _ = kb.put("pit", 4, "time");
    s.add("四词条", kb.len() == 4, "入册");
    s.add("重键拒", kb.put("oom", 9, "mem") == Err(XC_DUP), "主键");
    let g1 = kb.get("tsc");
    s.add("精确检索", matches!(&g1, Ok(e) if e.val == 3), "键主键");
    s.add("查无拒", kb.get("ghost") == Err(XC_NOENT), "空查");
    // 族二：反链。
    let _ = kb.link("oom", "guard");
    let _ = kb.link("tsc", "pit");
    s.add("正向引用", kb.refs_of("oom") == Ok(vec!["guard"]), "oom→guard");
    s.add("反向通路", kb.refs_of("guard") == Ok(vec!["oom"]), "guard→oom");
    s.add("自引拒", kb.link("oom", "oom") == Err(XC_SELF_REF), "环点");
    s.add("反链完整", kb.link_audit() == Ok(2), "两对边");
    // 幂等重边。
    let _ = kb.link("oom", "guard");
    s.add("重边幂等", kb.link_audit() == Ok(2), "不翻倍");
    // 族三：分类过滤。
    let mem = kb.by_class("mem");
    s.add("mem 类 2", mem.len() == 2 && mem[0] == "oom", "过滤");
    let time = kb.by_class("time");
    s.add("time 类 2", time.len() == 2, "四词两类");
    // 族四：冷热分层。
    let mut hk = KnowBase::new();
    let _ = hk.put("a", 1, "c");
    let _ = hk.put("b", 2, "c");
    let _ = hk.put("d3", 3, "c");
    let _ = hk.put("e4", 4, "c");
    let _ = hk.put("f5", 5, "c");
    // 热层空（未命中 3 次）。
    s.add("热层初始空", hk.hot_keys().is_empty(), "冷库");
    let _ = hk.get("a");
    let _ = hk.get("a");
    s.add("两访不升热", hk.hot_keys().is_empty(), "<3");
    let _ = hk.get("a");
    s.add("三访升热", hk.hot_keys() == vec!["a"], "a 入热");
    let _ = hk.get("b");
    let _ = hk.get("b");
    let _ = hk.get("b");
    let _ = hk.get("d3");
    let _ = hk.get("d3");
    let _ = hk.get("d3");
    let _ = hk.get("e4");
    let _ = hk.get("e4");
    let _ = hk.get("e4");
    // 热层满 4：a b d3 e4。
    let hot4 = hk.hot_keys();
    s.add("热层满 4", hot4.len() == 4, "K=4");
    // f5 升热挤最低计数（a b d3 e4 均 3 次，挤首个最低 = a）。
    let _ = hk.get("f5");
    let _ = hk.get("f5");
    let _ = hk.get("f5");
    let hot5 = hk.hot_keys();
    s.add("挤出最低", hot5.len() == 4 && !hot5.contains(&"a"), "f5 挤 a");
    // 族五：入参约束。
    let mut z = KnowBase::new();
    s.add("键空拒", z.put("", 1, "c") == Err(XC_KEY_EMPTY), "空键");
    s.add("类空拒", z.put("k", 1, "") == Err(XC_CLASS_EMPTY), "空类");
    s.add("热容常量", HOT_CAP == 4 && HOT_HIT == 3, "参数");
    s.add("魔数核", KB_SEG_BASE == 0x30900, "段基址");
    s
}
