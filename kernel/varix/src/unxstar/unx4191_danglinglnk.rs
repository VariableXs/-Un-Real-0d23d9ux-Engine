//! unx4191_danglinglnk — UNX-F4191 悬空链接语义（ENOENT 时机）（诊断段 0x9960）。
//!
//! 判据原文：
//!   J1 UNX-F4191-J1 跟随悬空链接 stat 返回 ENOENT（不返回 ELOOP/其他），
//!      lstat 返回链接本体——两口径区分断言各 1,000 次。
//! 设计纪律：
//!   1. 悬空定义：链接目标串在命名空间无落点（目标链断裂或目标根本
//!      不存在），链接本体本身在场且合法。
//!   2. 两口径分立：跟随口径（stat 类）展开目标失败 → ENOENT（错误码
//!      恒 ENOENT，不得 ELOOP/EINVAL/其他）；不跟随口径（lstat 类）恒
//!      返回链接本体（成功，不带错误）。
//!   3. 错误码纯度账：跟随悬空出现的非 ENOENT 错误单独记账（wrong 账
//!      恒零——判据「不返回 ELOOP/其他」的账面落点）。
//!   4. 悬空允许创建：symlink 创建不校验目标存在性（POSIX 口径），
//!      悬空链接创建入账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9960;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ENOENT: u16 = SEG_BASE + 1;   // 跟随悬空：目标落点不存在（唯一合法错误码）
const E_NOT_ABS: u16 = SEG_BASE + 2;  // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 3; // 空分量
const E_BAD_NODE: u16 = SEG_BASE + 4; // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 5; // 同目录重名
const E_WRONG: u16 = SEG_BASE + 6;    // 错误码不纯（悬空跟随出了非 ENOENT）
const E_LEDGER: u16 = SEG_BASE + 7;   // 两口径账失守（区分断言账不平）

/// 判据断言次数（两口径各 1,000 次）。
pub const ASSERT_TIMES: u32 = 1000;

/// 两口径闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatSide {
    Follow, // stat 类：跟随展开
    NoStat, // lstat 类：停在链接本体
}

/// 悬空账：跟随 ENOENT/错误码不纯/本体命中三分账。
pub struct DangleBook {
    pub stat_enoent: u32,  // 跟随悬空返回 ENOENT 数（判据 1,000 账）
    pub wrong_err: u32,    // 跟随悬空返回非 ENOENT 数（恒零）
    pub lstat_body: u32,   // 不跟随命中链接本体数（判据 1,000 账）
    pub dangling_created: u32, // 悬空链接创建数
}

impl DangleBook {
    pub fn new() -> DangleBook {
        DangleBook {
            stat_enoent: 0,
            wrong_err: 0,
            lstat_body: 0,
            dangling_created: 0,
        }
    }

    /// 两口径区分账对平：两账各达判据次数且错误码纯度恒零。
    pub fn conserved(&self) -> bool {
        self.stat_enoent >= ASSERT_TIMES && self.lstat_body >= ASSERT_TIMES && self.wrong_err == 0
    }
}

/// 命名空间树（悬空链接载体）。
pub struct DNode {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub target: String,
    pub children: Vec<usize>,
}

pub struct DangleTree {
    pub nodes: Vec<DNode>,
    pub book: DangleBook,
}

impl DangleTree {
    pub fn new() -> DangleTree {
        DangleTree {
            nodes: vec![DNode {
                name: String::from("/"),
                is_dir: true,
                is_link: false,
                target: String::new(),
                children: Vec::new(),
            }],
            book: DangleBook::new(),
        }
    }

    /// 建链接（目标存在性不校验——悬空允许创建，POSIX 口径）。
    pub fn add_link(&mut self, parent: usize, name: &str, target: &str) -> Result<usize, u16> {
        let id = self.add_any(parent, name, false, true, target)?;
        // 目标落点不存在即悬空创建入账。
        if self.resolve_quiet(target).is_none() {
            self.book.dangling_created += 1;
        }
        Ok(id)
    }

    pub fn add(&mut self, parent: usize, name: &str, is_dir: bool) -> Result<usize, u16> {
        self.add_any(parent, name, is_dir, false, "")
    }

    fn add_any(
        &mut self,
        parent: usize,
        name: &str,
        is_dir: bool,
        is_link: bool,
        target: &str,
    ) -> Result<usize, u16> {
        if parent >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        if name.is_empty() || name.contains('/') {
            return Err(E_BAD_COMP);
        }
        if self.child_of(parent, name).is_some() {
            return Err(E_DUP_NAME);
        }
        let id = self.nodes.len();
        self.nodes.push(DNode {
            name: name.to_string(),
            is_dir,
            is_link,
            target: target.to_string(),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        Ok(id)
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

    /// 静默解析（不记账，用于悬空判定）。
    fn resolve_quiet(&self, path: &str) -> Option<usize> {
        let comps = self.split(path).ok()?;
        let mut cur = 0usize;
        for c in comps {
            cur = self.child_of(cur, c)?;
        }
        Some(cur)
    }

    /// 跟随展开：目标落点不存在 → ENOENT（错误码纯度由账面守卫）。
    fn follow(&self, mut node: usize) -> Result<usize, u16> {
        let mut layers = 0u32;
        while self.nodes[node].is_link {
            layers += 1;
            if layers > 8 {
                // 上限守门不越界：悬空判定的边界面（正式口径归 F4186）。
                return Err(E_WRONG);
            }
            match self.resolve_quiet(&self.nodes[node].target.clone()) {
                Some(t) => node = t,
                None => return Err(E_ENOENT), // 悬空：唯一合法错误码
            }
        }
        Ok(node)
    }

    /// 双口径主入口：Follow→展开（悬空 ENOENT）；NoStat→链接本体。
    pub fn stat_like(&mut self, path: &str, side: StatSide) -> Result<usize, u16> {
        let comps = self.split(path)?;
        if comps.is_empty() {
            return Ok(0);
        }
        let mut cur = 0usize;
        let n = comps.len();
        let mut i = 0usize;
        while i + 1 < n {
            // 中间分量恒跟随（两口径共享中间语义）。
            let next = self.child_of(cur, comps[i]).ok_or(E_ENOENT)?;
            cur = self.follow(next)?;
            i += 1;
        }
        let last = self.child_of(cur, comps[n - 1]).ok_or(E_ENOENT)?;
        match side {
            StatSide::Follow => {
                let r = self.follow(last)?;
                Ok(r)
            }
            StatSide::NoStat => {
                self.book.lstat_body += 1;
                Ok(last) // 恒返回链接本体
            }
        }
    }

    /// 跟随口径记账包装：ENOENT 入正账，其他错误码入 wrong 账（判据纯度）。
    pub fn stat_follow(&mut self, path: &str) -> Result<usize, u16> {
        match self.stat_like(path, StatSide::Follow) {
            Ok(n) => Ok(n),
            Err(e) => {
                if e == E_ENOENT {
                    self.book.stat_enoent += 1;
                } else {
                    self.book.wrong_err += 1;
                }
                Err(e)
            }
        }
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ENOENT => "跟随悬空：目标落点不存在（唯一合法错误码）",
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_WRONG => "错误码不纯（悬空跟随出了非 ENOENT）",
        E_LEDGER => "两口径账失守（区分断言账不平）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：悬空创建→跟随 ENOENT 1,000→本体 1,000→错误码纯度→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut t = DangleTree::new();

    // R1：建面——真身 /real、悬空链接 dang→/no/such、在位链接 ok→/real。
    let real = t.add(0, "real", true)?;
    t.add_link(0, "dang", "/no/such")?;
    t.add_link(0, "ok", "/real")?;
    if t.book.dangling_created != 1 {
        return Err(E_LEDGER); // 仅 dang 悬空
    }
    hits += 1;

    // R2：跟随口径 1,000 次——悬空链接 stat 恒 ENOENT。
    let mut n = 0u32;
    while n < ASSERT_TIMES {
        if t.stat_follow("/dang") != Err(E_ENOENT) {
            return Err(E_ENOENT);
        }
        n += 1;
    }
    if t.book.stat_enoent != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：不跟随口径 1,000 次——lstat 恒返回链接本体。
    let mut n = 0u32;
    while n < ASSERT_TIMES {
        let node = t.stat_like("/dang", StatSide::NoStat)?;
        if t.nodes[node].name != "dang" || !t.nodes[node].is_link {
            return Err(E_LEDGER);
        }
        n += 1;
    }
    if t.book.lstat_body != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：非悬空对照——在位链接跟随落真身；不跟随落本体。
    let r = t.stat_follow("/ok")?;
    if r != real {
        return Err(E_LEDGER);
    }
    let b = t.stat_like("/ok", StatSide::NoStat)?;
    if t.nodes[b].name != "ok" {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：错误码纯度——跟随悬空经中间分量同样 ENOENT（/dang/x 的
    // 中间位悬空即断，两口径皆 ENOENT 面）。
    if t.stat_follow("/dang/x") != Err(E_ENOENT) {
        return Err(E_ENOENT);
    }
    if t.book.wrong_err != 0 {
        return Err(E_WRONG);
    }
    hits += 1;

    // I2：相对路径与空分量注入（形错不入悬空账）。
    if t.stat_follow("dang") != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    if t.stat_like("//dang", StatSide::NoStat) != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    hits += 1;

    // R5：终审——两口径账各 1,000、wrong 恒零、对平。
    if !t.book.conserved() {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
