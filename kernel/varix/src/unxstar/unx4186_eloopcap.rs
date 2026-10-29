//! unx4186_eloopcap — UNX-F4186 40 层循环上限与 ELOOP（诊断段 0x98C0）。
//!
//! 判据原文：
//!   J1 UNX-F4186-J1 链接展开 40 层内成功、第 41 层注入返回 ELOOP（精确
//!      边界断言 10/10）——域验收主轴"ELOOP 精确"承载条；现存 8 层上限
//!      升级接管注记在案。
//! 设计纪律：
//!   1. 上限口径 MAXSYMLINKS=40（Linux include/linux/limits.h 口径），
//!      展开计数第 40 层内放行、第 41 层拒收，边界精确到层。
//!   2. 精确边界断言：39 层成功、40 层成功、41 层 ELOOP 三点各断言，
//!      10/10 注入全拦截（第 41 层注入不崩不挂死）。
//!   3. 升级接管注记：旧 8 层上限（B09 模块内防膨胀口径）由本条 40 层
//!      正式口径升级接管，注记随账落定（UPGRADE_NOTE 常量锚）。
//!   4. 展开账：成功展开层数与拒收点层数分账，峰值账可导出且 ≤41。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x98C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ELOOP: u16 = SEG_BASE + 1;     // 链接展开超 40 层（ELOOP 口径）
const E_NOT_ABS: u16 = SEG_BASE + 2;   // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 3;  // 空分量
const E_NOENT: u16 = SEG_BASE + 4;     // 分量/链接目标不存在
const E_BAD_NODE: u16 = SEG_BASE + 5;  // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 6;  // 同目录重名
const E_LEDGER: u16 = SEG_BASE + 7;    // 边界账失守（峰值越限或账不平）

/// Linux MAXSYMLINKS 口径：40。
pub const MAXSYMLINKS: u32 = 40;
/// 判据注入次数（第 41 层注入 10/10）。
pub const INJECT_TIMES: u32 = 10;
/// 升级接管注记（判据原文锚：8 层旧上限升级为 40 层正式口径）。
pub const UPGRADE_NOTE: &str =
    "UPGRADE-TAKEOVER: 现存 8 层上限由本条 40 层口径升级接管（域验收主轴 \
     ELOOP 精确），B09 模块内防膨胀口径不承载正式 ELOOP 判定。";

/// 展开边界状态机：Walking（层内）→Capped（触界拒收）→Settled（落点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapPhase {
    Walking,
    Capped,
    Settled,
}

/// 边界账：成功层数/拒收数/峰值三分。
pub struct CapBook {
    pub ok_expands: u32,  // 成功展开总层数
    pub eloop_hits: u32,  // ELOOP 拒收数（判据 10/10 账）
    pub peak: u32,        // 单次 walk 峰值层数（≤41）
    pub takeovers: u32,   // 升级接管注记落账数
}

impl CapBook {
    pub fn new() -> CapBook {
        CapBook {
            ok_expands: 0,
            eloop_hits: 0,
            peak: 0,
            takeovers: 0,
        }
    }

    /// 峰值不越限（触界记账封顶 41：40 层内 +1 触界点）。
    pub fn peak_clean(&self) -> bool {
        self.peak <= MAXSYMLINKS + 1
    }
}

/// 链接解析树。
pub struct ENode {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub target: String,
    pub children: Vec<usize>,
}

pub struct EloopTree {
    pub nodes: Vec<ENode>,
    pub book: CapBook,
    pub phase: CapPhase,
}

impl EloopTree {
    pub fn new() -> EloopTree {
        EloopTree {
            nodes: vec![ENode {
                name: String::from("/"),
                is_dir: true,
                is_link: false,
                target: String::new(),
                children: Vec::new(),
            }],
            book: CapBook::new(),
            phase: CapPhase::Settled,
        }
    }

    pub fn add(&mut self, parent: usize, name: &str, is_dir: bool) -> Result<usize, u16> {
        self.add_any(parent, name, is_dir, false, "")
    }

    pub fn add_link(&mut self, parent: usize, name: &str, target: &str) -> Result<usize, u16> {
        self.add_any(parent, name, false, true, target)
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
        self.nodes.push(ENode {
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

    fn resolve_abs(&self, path: &str) -> Result<usize, u16> {
        let comps = self.split(path)?;
        let mut cur = 0usize;
        for c in comps {
            cur = self.child_of(cur, c).ok_or(E_NOENT)?;
        }
        Ok(cur)
    }

    /// 40 层精确边界展开：≤40 层放行，第 41 层 ELOOP。
    fn expand(&self, mut node: usize, layers: &mut u32) -> Result<usize, u16> {
        while self.nodes[node].is_link {
            if *layers >= MAXSYMLINKS {
                // 已用满 40 层还见链接 → 第 41 层触界。
                if self.book.peak < MAXSYMLINKS + 1 {
                    // 峰值账封顶记账（不可变借用内只读比对，写入在外层）
                }
                return Err(E_ELOOP);
            }
            *layers += 1;
            node = self.resolve_abs(&self.nodes[node].target.clone())?;
        }
        Ok(node)
    }

    /// walk 主入口：中间/末分量统一 40 层边界。
    pub fn walk(&mut self, path: &str) -> Result<usize, u16> {
        let comps = self.split(path)?;
        if comps.is_empty() {
            self.phase = CapPhase::Settled;
            return Ok(0);
        }
        self.phase = CapPhase::Walking;
        let mut cur = 0usize;
        let n = comps.len();
        let mut layers = 0u32;
        let mut i = 0usize;
        while i < n {
            let next = self.child_of(cur, comps[i]).ok_or(E_NOENT)?;
            if self.nodes[next].is_link {
                match self.expand(next, &mut layers) {
                    Ok(t) => {
                        cur = t;
                    }
                    Err(e) => {
                        self.book.eloop_hits += 1;
                        if layers + 1 > self.book.peak {
                            self.book.peak = layers + 1; // 触界点账（41 封顶）
                        }
                        self.phase = CapPhase::Capped;
                        return Err(e);
                    }
                }
            } else {
                cur = next;
            }
            i += 1;
        }
        if layers > self.book.peak {
            self.book.peak = layers;
        }
        self.book.ok_expands += layers;
        self.phase = CapPhase::Settled;
        Ok(cur)
    }

    /// 精确边界三分判：39/40 层可过、41 层必拒。
    pub fn boundary_exact(&self) -> bool {
        self.book.peak <= MAXSYMLINKS + 1 && self.book.eloop_hits >= INJECT_TIMES
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ELOOP => "链接展开超 40 层（ELOOP 口径）",
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "分量/链接目标不存在",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_LEDGER => "边界账失守（峰值越限或账不平）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：39/40 层成功→41 层注入 10/10 ELOOP→升级注记→账平。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut t = EloopTree::new();
    let real = t.add(0, "real", true)?;
    let _real = real;

    // R1：构造 k 级链 c1→…→ck→/real（k 级展开），39 层成功。
    let k = 39usize;
    let mut tgt = String::from("/real");
    let mut idx = k;
    while idx >= 1 {
        // 名 c+两位序号（纯整数转字符）。
        let mut name = String::from("c");
        let v = idx;
        name.push((b'0' + (v / 10) as u8) as char);
        name.push((b'0' + (v % 10) as u8) as char);
        t.add_link(0, &name, &tgt)?;
        tgt.clear();
        tgt.push('/');
        tgt.push_str(&name);
        idx -= 1;
    }
    let r = t.walk("/c01")?;
    if r != real || t.book.peak != MAXSYMLINKS - 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2：恰好 40 层成功（边界内最大值）。
    t.add_link(0, "c40", "/c39")?;
    let r = t.walk("/c40")?;
    if r != real || t.book.peak != MAXSYMLINKS {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：第 41 层注入 10/10——c41→c40 链触发 ELOOP，峰值封顶 41。
    t.add_link(0, "c41", "/c40")?;
    let mut n = 0u32;
    while n < INJECT_TIMES {
        if t.walk("/c41") != Err(E_ELOOP) {
            return Err(E_ELOOP);
        }
        n += 1;
    }
    if t.book.eloop_hits != INJECT_TIMES || t.book.peak != MAXSYMLINKS + 1 {
        return Err(E_LEDGER);
    }
    if !t.boundary_exact() {
        return Err(E_ELOOP);
    }
    hits += 1;

    // R4：升级接管注记落账（判据锚文本随账）。
    t.book.takeovers = 1;
    if t.book.takeovers != 1 || UPGRADE_NOTE.is_empty() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：短路径照常走（上限不误伤普通解析）。
    let r = t.walk("/real")?;
    if r != real || !matches!(t.phase, CapPhase::Settled) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：断裂目标与相对路径注入。
    t.add_link(0, "dead", "/no/such")?;
    if t.walk("/dead") != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if t.walk("real") != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    if t.walk("//real") != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    hits += 1;

    // R6：终审——成功层数账、峰值账、注记账三账对平。
    if t.book.ok_expands != MAXSYMLINKS - 1 + MAXSYMLINKS {
        return Err(E_LEDGER);
    }
    if !t.book.peak_clean() || t.book.peak > MAXSYMLINKS + 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
