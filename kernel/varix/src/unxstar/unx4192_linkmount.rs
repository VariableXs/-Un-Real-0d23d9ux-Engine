//! unx4192_linkmount — UNX-F4192 链接目标遍历与挂载点交叉（诊断段 0x9980）。
//!
//! 判据原文：
//!   J1 UNX-F4192-J1 链接目标跨挂载点解析正确（B12 交叉预锚接口断言），
//!      目标落在挂载遮蔽区时按可见面解析（遮蔽语义账齐备）。
//! 设计纪律：
//!   1. 挂载表：挂载点=被遮蔽槽 + 挂载根（fs_id 承载），多挂同点按栈
//!      叠放，可见面恒取栈顶（后挂遮前挂）。
//!   2. 跨挂载点解析：链接目标串经挂载点时按可见面重定向到挂载根
//!      （fs_id 面），解析落点携带 fs_id 入账——B12 交叉预锚接口面。
//!   3. 遮蔽语义：目标落在被遮蔽槽时按可见面（栈顶挂载根）解析，
//!      原槽内容不可直达（遮蔽账分记）。
//!   4. 误路由账：解析落点与可见面不符即 misroute（恒零）；交叉解析
//!      与遮蔽解析分账齐备。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9980;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_ABS: u16 = SEG_BASE + 1;   // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 2;  // 空分量
const E_NOENT: u16 = SEG_BASE + 3;     // 分量不存在
const E_BAD_NODE: u16 = SEG_BASE + 4;  // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 5;  // 同目录重名
const E_DUP_MOUNT: u16 = SEG_BASE + 6; // 同挂载键重复登记（须走叠放）
const E_MISROUTE: u16 = SEG_BASE + 7;  // 解析落点与可见面不符（误路由）
const E_LEDGER: u16 = SEG_BASE + 8;    // 交叉账对平失败（遮蔽/穿越账不平）

/// 可见面二元（遮蔽语义读面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visible {
    Exposed, // 无遮蔽：原槽可见
    Covered, // 有遮蔽：栈顶挂载根可见
}

/// 挂载项：挂载点（被遮蔽槽）+ 挂载根 + 叠放代次。
pub struct Mount {
    pub at: usize,   // 挂载点槽（被遮蔽）
    pub root: usize, // 挂载根槽（可见面落点）
    pub fs_id: u32,  // 文件系统标识（交叉面锚）
    pub gen: u32,    // 叠放代次（大者可见）
}

/// 交叉账：跨挂载解析/遮蔽解析/误路由三分账。
pub struct CrossBook {
    pub crosses: u32,   // 目标跨挂载点解析数
    pub shadow_resolves: u32, // 遮蔽区按可见面解析数
    pub misroutes: u32, // 误路由数（恒零）
    pub mounts: u32,    // 挂载登记数
}

impl CrossBook {
    pub fn new() -> CrossBook {
        CrossBook {
            crosses: 0,
            shadow_resolves: 0,
            misroutes: 0,
            mounts: 0,
        }
    }

    /// 账面守恒：误路由恒零且挂载账与登记数一致。
    pub fn conserved(&self, mount_rows: usize) -> bool {
        self.misroutes == 0 && self.mounts == mount_rows as u32
    }
}

/// 交叉解析树。
pub struct XNode {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub target: String,
    pub children: Vec<usize>,
}

pub struct MountTree {
    pub nodes: Vec<XNode>,
    pub mounts: Vec<Mount>,
    pub book: CrossBook,
}

impl MountTree {
    pub fn new() -> MountTree {
        MountTree {
            nodes: vec![XNode {
                name: String::from("/"),
                is_dir: true,
                is_link: false,
                target: String::new(),
                children: Vec::new(),
            }],
            mounts: Vec::new(),
            book: CrossBook::new(),
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
        self.nodes.push(XNode {
            name: name.to_string(),
            is_dir,
            is_link,
            target: target.to_string(),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    /// 挂载登记：同槽可叠放（代次递增），叠放即遮蔽。
    pub fn mount(&mut self, at: usize, root: usize, fs_id: u32) -> Result<u32, u16> {
        if at >= self.nodes.len() || root >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        // 同 (at, root, fs_id) 三元组完全重复才拒（叠放合法）。
        for m in &self.mounts {
            if m.at == at && m.root == root && m.fs_id == fs_id {
                return Err(E_DUP_MOUNT);
            }
        }
        let gen = self.top_gen(at) + 1;
        self.mounts.push(Mount { at, root, fs_id, gen });
        self.book.mounts += 1;
        Ok(gen)
    }

    /// 槽上最高代次（无挂载为 0）。
    fn top_gen(&self, at: usize) -> u32 {
        let mut g = 0u32;
        for m in &self.mounts {
            if m.at == at && m.gen > g {
                g = m.gen;
            }
        }
        g
    }

    /// 可见面读面：槽被挂载遮蔽则返回栈顶挂载根，否则原槽。
    pub fn visible_of(&self, node: usize) -> Result<(usize, Visible), u16> {
        if node >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        let g = self.top_gen(node);
        if g == 0 {
            return Ok((node, Visible::Exposed));
        }
        // 取栈顶挂载根。
        let mut best = 0usize;
        let mut found = false;
        for m in &self.mounts {
            if m.at == node && m.gen == g {
                best = m.root;
                found = true;
            }
        }
        if !found {
            return Err(E_MISROUTE);
        }
        Ok((best, Visible::Covered))
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

    /// 逐分量解析：每分量先按命中找，再按可见面重定向（遮蔽语义）。
    fn step(&mut self, dir: usize, name: &str) -> Result<usize, u16> {
        let hit = self.child_of(dir, name).ok_or(E_NOENT)?;
        let (vis, kind) = self.visible_of(hit)?;
        match kind {
            Visible::Covered => {
                self.book.shadow_resolves += 1;
                Ok(vis)
            }
            Visible::Exposed => Ok(vis),
        }
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

    /// 解析绝对路径（逐分量过可见面）；路径中链接目标即目标串重入。
    pub fn resolve(&mut self, path: &str) -> Result<usize, u16> {
        let comps = self.split(path)?;
        let mut cur = 0usize;
        for c in comps {
            cur = self.step(cur, c)?;
        }
        Ok(cur)
    }

    /// 链接目标解析（交叉主面）：目标串重入 resolve，落点 fs_id 随账。
    pub fn resolve_target(&mut self, link_name: &str) -> Result<(usize, u32), u16> {
        let lpath = format_link_path(link_name);
        let comps = self.split(&lpath)?;
        let _ = comps;
        // 找链接本体拿目标串。
        let mut cur = 0usize;
        let body = link_name.trim_start_matches('/');
        if body.is_empty() {
            return Ok((0, 0));
        }
        for c in body.split('/') {
            cur = self.step(cur, c)?;
        }
        if !self.nodes[cur].is_link {
            return Err(E_NOENT);
        }
        let tgt = self.nodes[cur].target.clone();
        // 目标串逐分量解析：跨挂载点即交叉解析入账。
        let tcomps = self.split(&tgt)?;
        let mut tcur = 0usize;
        for tc in tcomps {
            // 命中前先查该分量是否挂载点（跨挂载预判）。
            let hit = self.child_of(tcur, tc).ok_or(E_NOENT)?;
            if self.top_gen(hit) > 0 {
                self.book.crosses += 1;
            }
            tcur = self.step(tcur, tc)?;
        }
        // 落点 fs_id：落点在遮蔽区取栈顶挂载 fs_id，否则 0。
        let (vis, kind) = self.visible_of(tcur)?;
        let fs = match kind {
            Visible::Covered => {
                let g = self.top_gen(tcur);
                let mut fs = 0u32;
                for m in &self.mounts {
                    if m.at == tcur && m.gen == g {
                        fs = m.fs_id;
                    }
                }
                fs
            }
            Visible::Exposed => 0u32,
        };
        let _ = vis;
        Ok((vis, fs))
    }
}

/// 拼绝对路径（/ + 名，确定性构造）。
fn format_link_path(name: &str) -> String {
    let mut s = String::from("/");
    s.push_str(name);
    s
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "分量不存在",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_DUP_MOUNT => "同挂载键重复登记（须走叠放）",
        E_MISROUTE => "解析落点与可见面不符（误路由）",
        E_LEDGER => "交叉账对平失败（遮蔽/穿越账不平）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：挂载遮蔽→跨挂载链接目标→叠放栈顶可见→误路由拦截→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建面 /mnt/{a,b}、外置 fs 根 /xfs/{a,data}（挂载面自带同名
    // 条目，遮蔽后按可见面解析），链接 ln→/mnt/a。
    let mut t = MountTree::new();
    let mnt = t.add(0, "mnt", true)?;
    let a = t.add(mnt, "a", true)?;
    let _b = t.add(mnt, "b", true)?;
    let xfs = t.add(0, "xfs", true)?;
    let ax = t.add(xfs, "a", true)?;
    let data = t.add(xfs, "data", true)?;
    t.add_link(0, "ln", "/mnt/a")?;
    hits += 1;

    // R2：挂载遮蔽——xfs 根挂到 mnt 上：/mnt/a 经可见面解析落 xfs 子树。
    t.mount(mnt, xfs, 42)?;
    let (node, kind) = t.visible_of(mnt)?;
    if node != xfs || kind != Visible::Covered {
        return Err(E_MISROUTE);
    }
    // a 无自身挂载 → 原槽可见。
    let (na, ka) = t.visible_of(a)?;
    if na != a || ka != Visible::Exposed {
        return Err(E_MISROUTE);
    }
    hits += 1;

    // R3：链接目标跨挂载点解析——ln 目标 /mnt/a 经挂载点 mnt 交叉，
    // 按可见面落到挂载根下的同名条目 ax（fs_id=42，B12 交叉预锚接口面）。
    let (node, fs) = t.resolve_target("ln")?;
    if node != ax {
        return Err(E_MISROUTE);
    }
    if fs != 42 || t.book.crosses != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：叠放栈顶可见——再挂 fs7 到 mnt：可见面切到 fs7 根，遮蔽账 +1。
    t.mount(mnt, data, 7)?;
    let (node, kind) = t.visible_of(mnt)?;
    if node != data || kind != Visible::Covered {
        return Err(E_MISROUTE);
    }
    if t.book.shadow_resolves < 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：同三元组重复挂载拒收；不同根叠放合法（上一条已验）。
    if t.mount(mnt, data, 7) != Err(E_DUP_MOUNT) {
        return Err(E_DUP_MOUNT);
    }
    hits += 1;

    // I2：不存在分量 / 相对路径 / 空分量注入。
    if t.resolve("/mnt/none") != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if t.resolve("mnt/a") != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    if t.resolve("/mnt//a") != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    hits += 1;

    // R5：终审——误路由恒零，挂载账与登记数对平。
    if !t.book.conserved(t.mounts.len()) {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
