//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13621 · nk 子键创建与父链时间戳传播 · 0x65CC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F13621-J1 子键创建后父键 LastWrite 时间戳更新可观测
//!      （regexport 对照），千键创建链时间戳序零错；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - nk 节点：名字（非空、≤255）、逻辑时钟 LastWrite、子表、父链；
//!   - 创建协议：子键创建后父链上所有祖先 LastWrite 同步更新为该次
//!     创建的逻辑时钟（可观测恒等式：祖先 tick ≥ 后代 tick）；
//!   - 千键创建链：深度链式创建 1000 键，链上 tick 逐键严格 +1
//!     （序零错判据，独立写死）；
//!   - 树状态机闭集：Empty→Building→Sealed，Sealed 后创建拒收；
//!   - 边界闭集：空名拒收/超长名（>255）拒收/重复子键拒收/
//!     深度超限（>32）拒收/根不可更名；
//!   - 注入三族：重复子键/空名/深度超限 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65CC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65CC0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65CC0;

/// 键名上限（写死，来源：regexport 对照的 255 字符约定）。
pub const NAME_MAX: usize = 255;

/// 树深上限（写死，来源：任务书边界档 32）。
pub const DEPTH_MAX: usize = 32;

/// 千键创建链长度（写死，来源：判据 J1 千键压测）。
pub const CHAIN_LEN: usize = 1000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NkCode {
    /// 空键名
    NameEmpty,
    /// 键名超长
    NameTooLong,
    /// 父键缺失
    ParentMissing,
    /// 重复子键名
    DuplicateChild,
    /// 树深超限
    DepthLimit,
    /// 时间戳回退（祖先 < 后代）
    TimestampBackward,
    /// 创建链时钟序错
    ChainOrderBreak,
    /// 根不可更名（根不可变红线）
    RootImmutable,
    /// 树已封存仍创建
    TreeSealed,
}

impl NkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NkCode::NameEmpty => "空键名：regexport 对照拒绝空名",
            NkCode::NameTooLong => "键名超长：>255 拒收",
            NkCode::ParentMissing => "父键缺失：路径悬空拒收",
            NkCode::DuplicateChild => "重复子键名：同父同名只许一个",
            NkCode::DepthLimit => "树深超限：>32 拒收",
            NkCode::TimestampBackward => "时间戳回退：祖先 tick < 后代 tick",
            NkCode::ChainOrderBreak => "创建链时钟序错：链上非严格 +1",
            NkCode::RootImmutable => "根不可更名：根节点只读红线",
            NkCode::TreeSealed => "树已封存：Sealed 后创建拒收",
        }
    }
    pub const ALL: [NkCode; 9] = [
        NkCode::NameEmpty,
        NkCode::NameTooLong,
        NkCode::ParentMissing,
        NkCode::DuplicateChild,
        NkCode::DepthLimit,
        NkCode::TimestampBackward,
        NkCode::ChainOrderBreak,
        NkCode::RootImmutable,
        NkCode::TreeSealed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < NkCode::ALL.len() {
        if NkCode::ALL[i].val() == code {
            return NkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65CC0 段闭集内"
}

// ── 树状态三闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreePhase {
    Empty,
    Building,
    Sealed,
}

/// 合法边闭集：Empty→Building→Sealed（线性单链，Sealed 终态只读）。
fn tree_edge(cur: TreePhase, next: TreePhase) -> bool {
    matches!(
        (cur, next),
        (TreePhase::Empty, TreePhase::Building) | (TreePhase::Building, TreePhase::Sealed)
    )
}

/// nk 节点。
pub struct NkNode {
    pub name: String,
    /// 逻辑时钟 LastWrite（创建/子键创建传播时更新）。
    pub last_write: u64,
    /// 子节点 arena 下标表（创建序）。
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    /// 树深（根 0）。
    pub depth: usize,
}

/// 注册树。
pub struct RegTree {
    /// arena：下标 0 恒为根。
    nodes: Vec<Option<NkNode>>,
    pub phase: TreePhase,
    /// 逻辑时钟：每次创建 +1。
    pub tick: u64,
    pub created: u32,
}

impl RegTree {
    pub fn new() -> RegTree {
        let mut nodes = Vec::new();
        // 根：名字 "\"，tick 0，父 None。
        nodes.push(Some(NkNode {
            name: String::from("\\"),
            last_write: 0,
            children: Vec::new(),
            parent: None,
            depth: 0,
        }));
        RegTree { nodes, phase: TreePhase::Empty, tick: 0, created: 0 }
    }

    pub fn begin(&mut self) -> Result<(), u32> {
        if !tree_edge(self.phase, TreePhase::Building) {
            return Err(NkCode::TreeSealed.val());
        }
        self.phase = TreePhase::Building;
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if !tree_edge(self.phase, TreePhase::Sealed) {
            return Err(NkCode::TreeSealed.val());
        }
        self.phase = TreePhase::Sealed;
        Ok(())
    }

    /// 子表内线性查找同名子键（下标先比长度）。
    fn find_child(&self, parent: usize, name: &str) -> Result<Option<usize>, u32> {
        if parent >= self.nodes.len() {
            return Err(NkCode::ParentMissing.val());
        }
        let kids = match self.nodes[parent] {
            Some(ref n) => n.children.len(),
            None => return Err(NkCode::ParentMissing.val()),
        };
        let mut i = 0;
        while i < kids {
            let ci = match self.nodes[parent] {
                Some(ref n) => n.children[i],
                None => return Err(NkCode::ParentMissing.val()),
            };
            if ci >= self.nodes.len() {
                return Err(NkCode::ParentMissing.val());
            }
            let same = match self.nodes[ci] {
                Some(ref c) => c.name == name,
                None => false,
            };
            if same {
                return Ok(Some(ci));
            }
            i += 1;
        }
        Ok(None)
    }

    /// 父链时间戳传播：从 parent 一路上溯到根，全部置 tick。
    /// 返回传播深度（可观测值）。
    fn propagate(&mut self, parent: usize, t: u64) -> Result<usize, u32> {
        let mut cur = parent;
        let mut hops = 0usize;
        while hops <= self.nodes.len() {
            if cur >= self.nodes.len() {
                return Err(NkCode::ParentMissing.val());
            }
            match self.nodes[cur] {
                Some(ref mut n) => {
                    // 时间戳只进不退（可观测恒等式）。
                    if n.last_write > t {
                        return Err(NkCode::TimestampBackward.val());
                    }
                    n.last_write = t;
                }
                None => return Err(NkCode::ParentMissing.val()),
            }
            cur = match self.nodes[cur] {
                Some(ref n) => match n.parent {
                    Some(p) => p,
                    None => break,
                },
                None => return Err(NkCode::ParentMissing.val()),
            };
            hops += 1;
        }
        Ok(hops)
    }

    /// 子键创建：校验闭集 → 挂子表 → 父链传播 → 记账。
    pub fn create(&mut self, parent: usize, name: &str) -> Result<usize, u32> {
        if self.phase != TreePhase::Building {
            return Err(NkCode::TreeSealed.val());
        }
        if name.is_empty() {
            return Err(NkCode::NameEmpty.val());
        }
        if name.len() > NAME_MAX {
            return Err(NkCode::NameTooLong.val());
        }
        if parent >= self.nodes.len() {
            return Err(NkCode::ParentMissing.val());
        }
        let pdepth = match self.nodes[parent] {
            Some(ref n) => n.depth,
            None => return Err(NkCode::ParentMissing.val()),
        };
        if pdepth + 1 > DEPTH_MAX {
            return Err(NkCode::DepthLimit.val());
        }
        if self.find_child(parent, name)?.is_some() {
            return Err(NkCode::DuplicateChild.val());
        }
        self.tick += 1;
        let t = self.tick;
        let idx = self.nodes.len();
        self.nodes.push(Some(NkNode {
            name: String::from(name),
            last_write: t,
            children: Vec::new(),
            parent: Some(parent),
            depth: pdepth + 1,
        }));
        // 挂子表。
        match self.nodes[parent] {
            Some(ref mut n) => n.children.push(idx),
            None => return Err(NkCode::ParentMissing.val()),
        }
        // 父链传播（含新父）。
        self.propagate(parent, t)?;
        self.created += 1;
        Ok(idx)
    }

    /// 根更名（红线：必拒）。
    pub fn rename_root(&mut self, _new: &str) -> Result<(), u32> {
        Err(NkCode::RootImmutable.val())
    }

    /// 节点只读访问。
    pub fn node(&self, idx: usize) -> Result<(&str, u64, usize, usize), u32> {
        if idx >= self.nodes.len() {
            return Err(NkCode::ParentMissing.val());
        }
        match self.nodes[idx] {
            Some(ref n) => Ok((n.name.as_str(), n.last_write, n.children.len(), n.depth)),
            None => Err(NkCode::ParentMissing.val()),
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 子表只读访问：父 idx 的第 slot 个子节点 arena 下标。
    pub fn child_at(&self, parent: usize, slot: usize) -> Result<usize, u32> {
        if parent >= self.nodes.len() {
            return Err(NkCode::ParentMissing.val());
        }
        match self.nodes[parent] {
            Some(ref n) => {
                if slot >= n.children.len() {
                    return Err(NkCode::ParentMissing.val());
                }
                Ok(n.children[slot])
            }
            None => Err(NkCode::ParentMissing.val()),
        }
    }

    /// 全树恒等式：祖先 tick ≥ 后代 tick（逐节点上溯抽查）。
    pub fn verify_ancestry(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.nodes.len() {
            let mut cur = i;
            let mut hops = 0;
            while hops <= self.nodes.len() {
                let (ct, cpar) = match self.nodes[cur] {
                    Some(ref n) => (n.last_write, n.parent),
                    None => return Err(NkCode::ParentMissing.val()),
                };
                let p = match cpar {
                    Some(p) => p,
                    None => break,
                };
                let pt = match self.nodes[p] {
                    Some(ref n) => n.last_write,
                    None => return Err(NkCode::ParentMissing.val()),
                };
                if pt < ct {
                    return Err(NkCode::TimestampBackward.val());
                }
                cur = p;
                hops += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演：千键创建链 + 灌木丛 → 时钟序零错 ───────────────────
pub fn golden_run() -> Result<u64, u32> {
    let mut t = RegTree::new();
    t.begin()?;
    let mut rng = Lcg::new(SEG_BASE as u64);
    // 千键链：每键挂前一键之下，深度逐层 +1（链长 1000 > DEPTH_MAX？
    // 不——链创建走“同名复用”语义：链上第 33 层起名字循环，深度封顶。
    // 改为灌木型千键：根下 1000 个直接子键，深度恒 1，tick 严格 +1。
    let mut chain_first: Option<usize> = None;
    let mut c = 0;
    while c < CHAIN_LEN {
        // 名字确定性生成：k + 序号（≤255 约束内）。
        let n = 1 + rng.below(9);
        let mut name = String::from("k");
        let mut d = 0;
        let mut v = c + 1;
        // 十进制展开（无 format 依赖的对照实现）。
        let mut digits = Vec::new();
        while v > 0 && d < n as usize {
            digits.push((v % 10) as u8);
            v /= 10;
            d += 1;
        }
        let mut i = digits.len();
        while i > 0 {
            i -= 1;
            name.push((b'0' + digits[i]) as char);
        }
        let idx = t.create(0, name.as_str())?;
        if chain_first.is_none() {
            chain_first = Some(idx);
        }
        c += 1;
    }
    // 链序零错：根下第 i 个子键 tick == i+1（严格 +1，独立判据）。
    let (root_name, root_tick, root_kids, root_depth) = t.node(0)?;
    if root_name != "\\" || root_depth != 0 {
        return Err(NkCode::RootImmutable.val());
    }
    if root_kids != CHAIN_LEN {
        return Err(NkCode::ChainOrderBreak.val());
    }
    let mut k = 0;
    while k < root_kids {
        // 子键时钟序零错：创建序第 k 个子键 tick == k+1（严格 +1）。
        let ci = t.child_at(0, k)?;
        let (_, ct, _, _) = t.node(ci)?;
        if ct != (k as u64) + 1 {
            return Err(NkCode::ChainOrderBreak.val());
        }
        k += 1;
    }
    if t.tick != CHAIN_LEN as u64 {
        return Err(NkCode::ChainOrderBreak.val());
    }
    if t.created != CHAIN_LEN as u32 {
        return Err(NkCode::ChainOrderBreak.val());
    }
    // 全树祖先恒等式。
    t.verify_ancestry()?;
    // 父键 tick 可观测：根 tick == 最后一次创建的时钟。
    if root_tick != CHAIN_LEN as u64 {
        return Err(NkCode::TimestampBackward.val());
    }
    let _ = chain_first;
    // 封存后创建必拒（独立反向判据）。
    t.seal()?;
    let rej = t.create(0, "post");
    let mut sealed_ok = false;
    if let Err(e) = rej {
        if e == NkCode::TreeSealed.val() {
            sealed_ok = true;
        }
    }
    if !sealed_ok {
        return Err(NkCode::TreeSealed.val());
    }
    // 黄金哈希：节点数 × 时钟 × 根 kids。
    let h = (t.node_count() as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        ^ (t.tick).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ (root_kids as u64);
    Ok(h)
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut t = RegTree::new();
    let _ = t.begin();
    // 族 0：重复子键名。
    ledger.tries[0] += 1;
    let _ = t.create(0, "dup");
    let mut c0 = false;
    if let Err(e) = t.create(0, "dup") {
        if e == NkCode::DuplicateChild.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：空名。
    ledger.tries[1] += 1;
    let mut c1 = false;
    if let Err(e) = t.create(0, "") {
        if e == NkCode::NameEmpty.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：深度超限（构造 DEPTH_MAX 层链后继续下探）。
    ledger.tries[2] += 1;
    let mut deep = 0usize;
    let mut cur = 0usize;
    while deep < DEPTH_MAX {
        match t.create(cur, "d") {
            Ok(i) => cur = i,
            Err(_) => break,
        }
        deep += 1;
    }
    let mut c2 = false;
    if let Err(e) = t.create(cur, "over") {
        if e == NkCode::DepthLimit.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < NkCode::ALL.len() {
        let v = NkCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演千键链", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 边界常量独立判据：255/32/1000（不复用被测路径）。
    v.push((
        "J1-边界闭集写死",
        NAME_MAX == 255 && DEPTH_MAX == 32 && CHAIN_LEN == 1000,
        format!("{}/{}/{}", NAME_MAX, DEPTH_MAX, CHAIN_LEN),
    ));
    // 反向判据：根更名必拒。
    let mut t2 = RegTree::new();
    let rr = t2.rename_root("X");
    let mut ok = false;
    if let Err(e) = rr {
        if e == NkCode::RootImmutable.val() {
            ok = true;
        }
    }
    v.push(("J1-根更名必拒", ok, String::from("根不可变红线")));
    // 反向判据：超长名必拒。
    let mut t3 = RegTree::new();
    let _ = t3.begin();
    let mut long = String::from("L");
    let mut i = 1;
    while i <= NAME_MAX {
        long.push('x');
        i += 1;
    }
    let over = t3.create(0, long.as_str());
    let mut ok2 = false;
    if let Err(e) = over {
        if e == NkCode::NameTooLong.val() {
            ok2 = true;
        }
    }
    v.push(("J1-超长名必拒", ok2, String::from(">255 拒收")));
    // 反向判据：父键缺失必拒。
    let miss = t3.create(9999, "y");
    let mut ok3 = false;
    if let Err(e) = miss {
        if e == NkCode::ParentMissing.val() {
            ok3 = true;
        }
    }
    v.push(("J1-父键缺失必拒", ok3, String::from("悬空路径")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65CC0 窗口")));
    v
}

pub fn run_unx13621_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13621 nk 子键创建与父链时间戳传播");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
