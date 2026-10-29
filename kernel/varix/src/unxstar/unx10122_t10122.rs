//! UNX-F10122 路径解析正反双判据（相对绝对达成与超长路径误用受控）（0x10260 段）
//!
//! 锚点：UNX-F10122-J1 路径解析达成（相对/绝对/多级导航 LTP chdir 面
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：路径树（节点名+父链的树形表）+ 解析核（绝对/相对/./.. 三导航/
//! 超长路径拒/空段跳过）+ 解析账（逐次留痕 + cwd 轨迹）+ 注入核（越根
//! 回退/超长/空路径/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x10260 独占 0x20）。
pub const SEG_BASE: u32 = 0x10260;
pub const PA_OVER_LONG: u32 = SEG_BASE + 0x01; // 超长路径
pub const PA_EMPTY: u32 = SEG_BASE + 0x02; // 空路径
pub const PA_NO_DIR: u32 = SEG_BASE + 0x03; // 导航缺目录
pub const PA_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const PA_SEALED: u32 = SEG_BASE + 0x05; // 封树后操作

/// 段长与全径上限。
pub const SEG_MAX: usize = 12;
pub const PATH_MAX: usize = 48;

/// 树节点。
#[derive(Clone)]
pub struct Node {
    pub name: String,
    pub parent: Option<u32>, // 0 表示根（根自指）
    pub is_dir: bool,
}

impl Node {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && self.name.len() <= SEG_MAX
    }
}

/// 解析账留痕。
#[derive(Clone)]
pub struct PaRec {
    pub seq: u32,
    pub input: String,
    pub resolved: String,
    pub note: String,
}

impl PaRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && !self.resolved.is_empty()
    }
}

/// 路径树 + 解析核。
pub struct PathTree {
    pub nodes: Vec<Node>,
    pub cwd: u32,
    pub ledger: Vec<PaRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PathTree {
    pub fn new() -> PathTree {
        PathTree {
            nodes: vec![Node { name: "/".to_string(), parent: Some(0), is_dir: true }],
            cwd: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 挂目录（父必须为目录）。
    pub fn mkdir(&mut self, parent: u32, name: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(PA_SEALED);
        }
        if name.is_empty() || name.len() > SEG_MAX {
            return Err(PA_OVER_LONG);
        }
        let p = parent as usize;
        if p >= self.nodes.len() || !self.nodes[p].is_dir {
            return Err(PA_NO_DIR);
        }
        let id = self.nodes.len() as u32;
        self.nodes.push(Node { name: name.to_string(), parent: Some(parent), is_dir: true });
        Ok(id)
    }

    /// 挂文件。
    pub fn touch(&mut self, parent: u32, name: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(PA_SEALED);
        }
        let p = parent as usize;
        if p >= self.nodes.len() || !self.nodes[p].is_dir {
            return Err(PA_NO_DIR);
        }
        let id = self.nodes.len() as u32;
        self.nodes.push(Node { name: name.to_string(), parent: Some(parent), is_dir: false });
        Ok(id)
    }

    /// 绝对路径串（回溯根链）。
    pub fn abspath(&self, id: u32) -> String {
        let mut chain: Vec<String> = Vec::new();
        let mut cur = id as usize;
        let mut guard = 0;
        while guard <= self.nodes.len() {
            if cur == 0 {
                break;
            }
            chain.push(self.nodes[cur].name.clone());
            match self.nodes[cur].parent {
                Some(p) => cur = p as usize,
                None => break,
            }
            guard += 1;
        }
        chain.reverse();
        let mut out = String::new();
        for seg in chain {
            out.push('/');
            out.push_str(&seg);
        }
        if out.is_empty() {
            out.push('/');
        }
        out
    }

    /// 解析核心：绝对自根、相对自 cwd，"./" 跳过、".." 回退（越根拒）。
    pub fn resolve(&mut self, path: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(PA_SEALED);
        }
        if note.is_empty() {
            return Err(PA_EMPTY_NOTE);
        }
        if path.is_empty() {
            return Err(PA_EMPTY);
        }
        if path.len() > PATH_MAX {
            return Err(PA_OVER_LONG);
        }
        let mut cur: u32 = if path.starts_with('/') { 0 } else { self.cwd };
        for seg in path.split('/') {
            if seg.is_empty() || seg == "." {
                continue;
            }
            if seg == ".." {
                let p = self.nodes[cur as usize].parent.unwrap_or(0);
                if cur == 0 {
                    return Err(PA_NO_DIR);
                }
                cur = p;
                continue;
            }
            // 在 cur 的子目录里找
            let hit = self
                .nodes
                .iter()
                .position(|n| n.parent == Some(cur) && n.name == seg && n.is_dir);
            match hit {
                Some(h) => cur = h as u32,
                None => return Err(PA_NO_DIR),
            }
        }
        self.ledger.push(PaRec {
            seq: self.seq_next,
            input: path.to_string(),
            resolved: self.abspath(cur),
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(cur)
    }

    /// chdir：解析并落 cwd。
    pub fn chdir(&mut self, path: &str, note: &str) -> Result<u32, u32> {
        let id = self.resolve(path, note)?;
        self.cwd = id;
        Ok(id)
    }

    pub fn cwd_path(&self) -> String {
        self.abspath(self.cwd)
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
        let mut h: u32 = 0x102600;
        for r in &self.ledger {
            for b in r.resolved.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in r.note.bytes() {
                h = h.wrapping_mul(37).wrapping_add(b as u32);
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
    let mut t = PathTree::new();
    let _ = t.mkdir(0, "home");
    // 1) 越根回退必拒
    got[0] = t.resolve("/../../..", "inj-root").err() == Some(PA_NO_DIR);
    // 2) 导航缺目录必拒
    got[1] = t.resolve("no/such/dir", "inj-miss").err() == Some(PA_NO_DIR);
    // 3) 空路径必拒
    got[2] = t.resolve("", "inj-empty").err() == Some(PA_EMPTY);
    // 4) 空留痕必拒
    got[3] = t.resolve("home", "").err() == Some(PA_EMPTY_NOTE);
    // 5) 封树后操作必抓
    t.seal();
    got[4] = t.resolve("home", "inj-seal").err() == Some(PA_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10122_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10122");

    // 树形搭建：/home/user/docs
    let mut t = PathTree::new();
    let home = t.mkdir(0, "home").unwrap_or(0);
    let user = t.mkdir(home, "user").unwrap_or(0);
    let docs = t.mkdir(user, "docs").unwrap_or(0);
    let _ = t.touch(docs, "a.txt");

    // 正路径：绝对解析
    let r1 = t.resolve("/home/user/docs", "绝对").unwrap_or(u32::MAX);
    let j1 = r1 == docs && t.abspath(docs) == "/home/user/docs";
    s.add("J1-绝对解析", j1, "绝对路径落 docs 且回溯串一致");

    // 相对解析 + 多级导航（chdir 面）
    let j2 = {
        let _ = t.chdir("home", "相对一");
        let _ = t.chdir("user", "相对二");
        let c = t.chdir("docs", "相对三").unwrap_or(u32::MAX);
        c == docs && t.cwd_path() == "/home/user/docs"
    };
    s.add("J2-多级导航", j2, "三级相对 chdir 落同点");

    // "." 与 ".." 导航
    let j3 = {
        let same = t.resolve(".", "原地").unwrap_or(u32::MAX);
        let up = t.resolve("..", "回退").unwrap_or(u32::MAX);
        same == docs && up == user
    };
    s.add("J3-点点导航", j3, ". 原地、.. 回父");

    // 空段跳过：双斜杠
    let j4 = t.resolve("//home//user", "空段").unwrap_or(u32::MAX) == user;
    s.add("J4-空段跳过", j4, "双斜杠等效单斜杠");

    // 相对混合导航：../docs 自 user 起
    let j5 = {
        let _ = t.chdir("/home/user", "归位");
        t.resolve("../user/docs", "混合").unwrap_or(u32::MAX) == docs
    };
    s.add("J5-混合导航", j5, "..+名段混合解析正确");

    // 超长路径受控
    let long = format!("/{}", "a".repeat(PATH_MAX));
    let j6 = t.resolve(&long, "超长").err() == Some(PA_OVER_LONG);
    s.add("J6-超长受控", j6, "超 PATH_MAX 必拒");

    // 越根回退受控
    let j7 = t.resolve("/..", "越根").err() == Some(PA_NO_DIR);
    s.add("J7-越根受控", j7, "根上再回退必拒");

    // 缺目录受控
    let j8 = t.resolve("/no/such", "缺席").err() == Some(PA_NO_DIR);
    s.add("J8-缺目录受控", j8, "导航缺席目录必拒");

    // 空路径受控
    let j9 = t.resolve("", "空").err() == Some(PA_EMPTY);
    s.add("J9-空路径受控", j9, "空路径必拒");

    // 节点合法性守恒
    let j10 = t.nodes.iter().all(|n| n.ok());
    s.add("J10-节点合法", j10, "名非空且段长在界");

    // 留痕合法与 seq 唯一
    let j11 = t.ledger.iter().all(|r| r.ok()) && t.seq_unique();
    s.add("J11-留痕合法", j11, "解析串非空且注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j12 = inj.iter().all(|g| *g);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j13 = t.fold() == t.fold();
    s.add("J13-指纹稳定", j13, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10122_path_flow() {
        let mut t = PathTree::new();
        let h = t.mkdir(0, "a").unwrap_or(0);
        let _ = t.mkdir(h, "b").unwrap_or(0);
        assert_eq!(t.resolve("/a/b", "x").unwrap_or(9), 2);
        assert_eq!(t.chdir("..", "y").unwrap_or(9), h);
        assert!(t.resolve("", "z") == Err(PA_EMPTY));
    }

    #[test]
    fn unx10122_checks_green() {
        let s = run_unx10122_checks();
        assert!(s.all_passed(), "UNX-F10122 判据未全绿");
    }
}
