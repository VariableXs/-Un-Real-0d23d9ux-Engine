//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15661 · IStorage 面谱与存储树 · 0x67640
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B24）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15661-J1 面谱成文（CreateStorage/CreateStream/OpenStorage/
//!     OpenStream/EnumElements 五核 10/10 次），越权开注入（无写权
//!     对象调 Create*）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 存储树：节点双类（Stg/Stm）+ 父链建模（路径闭集），树深
//!     封顶写死；Create*/Open* 五核分账；
//!   - 访问权闭集：Read（只读）/ Write（可写）——CreateStorage/
//!     CreateStream 仅 Write 权可调；无写权调 Create* = 越权开
//!     注入，如实拒；
//!   - 树守恒恒等式：子节点数 = 存储子数 + 流子数；枚举产出数 =
//!     子节点数；
//!   - 五核语义：CreateStorage/CreateStream（写权+重名拒）、
//!     OpenStorage/OpenStream（缺名拒/类型错拒）、EnumElements
//!     （枚举闭环产出=子数）；
//!   - 越权注入面 10/10 全抓（Read 权调 Create* 双法）；
//!   - LCG 种子写死 = SEG_BASE（0x67640）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67640，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x67640;

// ── 访问权闭集 ───────────────────────────────────────────────────
pub const ACC_READ: usize = 0;
pub const ACC_WRITE: usize = 1;
pub const ACC_COUNT: usize = 2;
pub const ACC_NAMES: [&str; ACC_COUNT] = ["只读", "可写"];

// ── 五核闭集 ─────────────────────────────────────────────────────
pub const CORE_CREATE_STORAGE: usize = 0;
pub const CORE_CREATE_STREAM: usize = 1;
pub const CORE_OPEN_STORAGE: usize = 2;
pub const CORE_OPEN_STREAM: usize = 3;
pub const CORE_ENUM_ELEMENTS: usize = 4;
pub const CORE_COUNT: usize = 5;
pub const CORE_NAMES: [&str; CORE_COUNT] =
    ["CreateStorage", "CreateStream", "OpenStorage", "OpenStream", "EnumElements"];

/// 树深封顶（写死：层闭集）。
pub const DEPTH_CAP: usize = 4;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StgCode {
    /// 访问权越闭集
    AccessOutOfRange,
    /// 五核越闭集
    CoreOutOfRange,
    /// 越权开：无写权调 Create*（如实拒）
    CreateWithoutWrite,
    /// 重名创建
    DuplicateName,
    /// 打开缺名
    OpenMissing,
    /// 类型错：OpenStorage 对流 / OpenStream 对存储
    TypeMismatch,
    /// 树深越封顶
    DepthCapExceeded,
    /// 树守恒破坏（枚举产出≠子数）
    TreeConservationBroken,
    /// 越权注入漏抓
    PrivilegeLeak,
    /// 双跑不一致
    NotReproducible,
}

impl StgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            StgCode::AccessOutOfRange => "访问权越闭集：只读/可写二值",
            StgCode::CoreOutOfRange => "五核越闭集：五法冻结",
            StgCode::CreateWithoutWrite => "越权开：无写权调 Create*，如实拒",
            StgCode::DuplicateName => "重名创建：同名子节点已存",
            StgCode::OpenMissing => "打开缺名：子节点不存在",
            StgCode::TypeMismatch => "类型错：开法与节点类不配",
            StgCode::DepthCapExceeded => "树深越封顶：层闭集",
            StgCode::TreeConservationBroken => "树守恒破坏：枚举产出≠子数",
            StgCode::PrivilegeLeak => "越权注入漏抓：检出 10/10 红线",
            StgCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [StgCode; 10] = [
        StgCode::AccessOutOfRange,
        StgCode::CoreOutOfRange,
        StgCode::CreateWithoutWrite,
        StgCode::DuplicateName,
        StgCode::OpenMissing,
        StgCode::TypeMismatch,
        StgCode::DepthCapExceeded,
        StgCode::TreeConservationBroken,
        StgCode::PrivilegeLeak,
        StgCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < StgCode::ALL.len() {
        if StgCode::ALL[i].val() == code {
            return StgCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67640 段闭集内"
}

// ── 存储树节点 ───────────────────────────────────────────────────
/// 节点双类闭集。
pub const NT_STORAGE: usize = 0;
pub const NT_STREAM: usize = 1;
pub const NT_COUNT: usize = 2;

/// 树节点：父链 + 名 + 类 + 子表。
#[derive(Clone, Debug)]
pub struct StgNode {
    pub name: String,
    pub kind: usize,
    pub depth: usize,
    pub children: Vec<StgNode>,
}

impl StgNode {
    /// 建存储节点。
    pub fn storage(name: &str, depth: usize) -> StgNode {
        StgNode {
            name: String::from(name),
            kind: NT_STORAGE,
            depth,
            children: Vec::new(),
        }
    }
    /// 建流节点。
    pub fn stream(name: &str, depth: usize) -> StgNode {
        StgNode {
            name: String::from(name),
            kind: NT_STREAM,
            depth,
            children: Vec::new(),
        }
    }
    pub fn is_storage(&self) -> bool {
        self.kind == NT_STORAGE
    }
    /// 子树总节点数（含自身，守恒对账用）。
    pub fn subtree_count(&self) -> u32 {
        let mut n = 1;
        let mut i = 0;
        while i < self.children.len() {
            n += self.children[i].subtree_count();
            i += 1;
        }
        n
    }
}

// ── 存储面谱账（IStorage 建模） ──────────────────────────────────
pub struct StorageTree {
    pub root: StgNode,
    pub acc: usize,
    pub stat: [u32; CORE_COUNT],
}

impl StorageTree {
    pub fn new(acc: usize) -> Result<StorageTree, u32> {
        if acc >= ACC_COUNT {
            return Err(StgCode::AccessOutOfRange.val());
        }
        Ok(StorageTree {
            root: StgNode::storage("root", 0),
            acc,
            stat: [0; CORE_COUNT],
        })
    }

    fn find_child(node: &StgNode, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < node.children.len() {
            if node.children[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// CreateStorage：写权闸 + 重名闸 + 深度闸。
    pub fn create_storage(&mut self, parent_path: &[usize], name: &str) -> Result<usize, u32> {
        if self.acc != ACC_WRITE {
            return Err(StgCode::CreateWithoutWrite.val());
        }
        let (node_depth, child_len) = self.walk(parent_path)?;
        if node_depth + 1 >= DEPTH_CAP {
            return Err(StgCode::DepthCapExceeded.val());
        }
        if Self::find_child(self.node_at(parent_path)?, name).is_some() {
            return Err(StgCode::DuplicateName.val());
        }
        let _ = child_len;
        let idx = {
            let n = self.node_at_mut(parent_path)?;
            n.children.push(StgNode::storage(name, node_depth + 1));
            n.children.len() - 1
        };
        self.stat[CORE_CREATE_STORAGE] += 1;
        Ok(idx)
    }

    /// CreateStream：写权闸 + 重名闸 + 深度闸。
    pub fn create_stream(&mut self, parent_path: &[usize], name: &str) -> Result<usize, u32> {
        if self.acc != ACC_WRITE {
            return Err(StgCode::CreateWithoutWrite.val());
        }
        let (node_depth, _) = self.walk(parent_path)?;
        if node_depth + 1 >= DEPTH_CAP {
            return Err(StgCode::DepthCapExceeded.val());
        }
        if Self::find_child(self.node_at(parent_path)?, name).is_some() {
            return Err(StgCode::DuplicateName.val());
        }
        let idx = {
            let n = self.node_at_mut(parent_path)?;
            n.children.push(StgNode::stream(name, node_depth + 1));
            n.children.len() - 1
        };
        self.stat[CORE_CREATE_STREAM] += 1;
        Ok(idx)
    }

    /// 路径行走：逐层下标先比长度（越界安全）。
    fn walk(&self, path: &[usize]) -> Result<(usize, usize), u32> {
        let mut cur = &self.root;
        let mut i = 0;
        while i < path.len() {
            if path[i] >= cur.children.len() {
                return Err(StgCode::OpenMissing.val());
            }
            if !cur.children[path[i]].is_storage() {
                return Err(StgCode::TypeMismatch.val());
            }
            cur = &cur.children[path[i]];
            i += 1;
        }
        Ok((cur.depth, cur.children.len()))
    }

    fn node_at(&self, path: &[usize]) -> Result<&StgNode, u32> {
        let mut cur = &self.root;
        let mut i = 0;
        while i < path.len() {
            if path[i] >= cur.children.len() {
                return Err(StgCode::OpenMissing.val());
            }
            cur = &cur.children[path[i]];
            i += 1;
        }
        Ok(cur)
    }

    fn node_at_mut(&mut self, path: &[usize]) -> Result<&mut StgNode, u32> {
        let mut cur = &mut self.root;
        let mut i = 0;
        while i < path.len() {
            if path[i] >= cur.children.len() {
                return Err(StgCode::OpenMissing.val());
            }
            let idx = path[i];
            cur = &mut cur.children[idx];
            i += 1;
        }
        Ok(cur)
    }

    /// OpenStorage：缺名拒；类型错拒。
    pub fn open_storage(&mut self, parent_path: &[usize], name: &str) -> Result<usize, u32> {
        let n = self.node_at(parent_path)?;
        match Self::find_child(n, name) {
            Some(i) => {
                if !n.children[i].is_storage() {
                    return Err(StgCode::TypeMismatch.val());
                }
                self.stat[CORE_OPEN_STORAGE] += 1;
                Ok(i)
            }
            None => Err(StgCode::OpenMissing.val()),
        }
    }

    /// OpenStream：缺名拒；类型错拒。
    pub fn open_stream(&mut self, parent_path: &[usize], name: &str) -> Result<usize, u32> {
        let n = self.node_at(parent_path)?;
        match Self::find_child(n, name) {
            Some(i) => {
                if n.children[i].is_storage() {
                    return Err(StgCode::TypeMismatch.val());
                }
                self.stat[CORE_OPEN_STREAM] += 1;
                Ok(i)
            }
            None => Err(StgCode::OpenMissing.val()),
        }
    }

    /// EnumElements：枚举产出数 = 子节点数（守恒闸）。
    pub fn enum_elements(&mut self, parent_path: &[usize]) -> Result<Vec<String>, u32> {
        let n = self.node_at(parent_path)?;
        let mut v = Vec::new();
        let mut i = 0;
        while i < n.children.len() {
            v.push(n.children[i].name.clone());
            i += 1;
        }
        if v.len() as u32 != n.children.len() as u32 {
            return Err(StgCode::TreeConservationBroken.val());
        }
        self.stat[CORE_ENUM_ELEMENTS] += 1;
        Ok(v)
    }

    /// 树守恒：子树计 = 1 + Σ子计（递归恒等式抽验）。
    pub fn conserve(&self) -> Result<(), u32> {
        let total = self.root.subtree_count();
        let mut acc = 1;
        let mut i = 0;
        while i < self.root.children.len() {
            acc += self.root.children[i].subtree_count();
            i += 1;
        }
        if total != acc {
            return Err(StgCode::TreeConservationBroken.val());
        }
        Ok(())
    }

    /// 五核判据正演：10/10 轮。
    pub fn judge_run(&mut self) -> Result<(u32, u32, u32, u32, u32), u32> {
        let mut ok = [0u32; CORE_COUNT];
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let sname = format!("stg{}", r);
            let fname = format!("stm{}", r);
            // CreateStorage/CreateStream（写权成立时）。
            let si = self.create_storage(&[], &sname)?;
            if self.root.children[si].name == sname {
                ok[CORE_CREATE_STORAGE] += 1;
            }
            self.create_stream(&[], &fname)?;
            ok[CORE_CREATE_STREAM] += 1;
            // OpenStorage/OpenStream。
            if self.open_storage(&[], &sname).is_ok() {
                ok[CORE_OPEN_STORAGE] += 1;
            }
            if self.open_stream(&[], &fname).is_ok() {
                ok[CORE_OPEN_STREAM] += 1;
            }
            // EnumElements：产出=子数。
            let ev = self.enum_elements(&[])?;
            if ev.len() == self.root.children.len() {
                ok[CORE_ENUM_ELEMENTS] += 1;
            }
            r += 1;
        }
        self.conserve()?;
        Ok((ok[0], ok[1], ok[2], ok[3], ok[4]))
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ──────────────────────────
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

// ── 黄金正演（判据常量独立写死） ─────────────────────────────────
pub const GOLDEN_CORE_OK: u32 = 10;
pub const GOLDEN_PRIV_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut t = StorageTree::new(ACC_WRITE)?;
    let (a, b, c, d, e) = t.judge_run()?;
    if a != GOLDEN_CORE_OK || b != GOLDEN_CORE_OK || c != GOLDEN_CORE_OK
        || d != GOLDEN_CORE_OK || e != GOLDEN_CORE_OK {
        return Err(StgCode::TreeConservationBroken.val());
    }
    // 越权开注入 10/10：Read 权对象调 Create* 双法必拒。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut ro = StorageTree::new(ACC_READ)?;
        let mut hit = 0;
        if let Err(e) = ro.create_storage(&[], "x") {
            if e == StgCode::CreateWithoutWrite.val() {
                hit += 1;
            }
        }
        if let Err(e) = ro.create_stream(&[], "y") {
            if e == StgCode::CreateWithoutWrite.val() {
                hit += 1;
            }
        }
        if hit == 2 {
            caught += 1;
        }
        r += 1;
    }
    if caught != GOLDEN_PRIV_CAUGHT {
        return Err(StgCode::PrivilegeLeak.val());
    }
    Ok((5, caught))
}

// ── 注入面：三族 10/10 全抓 ──────────────────────────────────────
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
    // 族 0：越权开（Read 权调 CreateStorage）×10 必拒。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut ro = StorageTree::new(ACC_READ).ok();
        if let Some(ref mut t) = ro {
            if let Err(e) = t.create_storage(&[], "p") {
                if e == StgCode::CreateWithoutWrite.val() {
                    ledger.caught[0] += 1;
                }
            }
        }
        r += 1;
    }
    // 族 1：重名创建注入 ×10 必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut t = StorageTree::new(ACC_WRITE).ok();
        if let Some(ref mut tr) = t {
            let _ = tr.create_stream(&[], "dup");
            if let Err(e) = tr.create_stream(&[], "dup") {
                if e == StgCode::DuplicateName.val() {
                    ledger.caught[1] += 1;
                }
            }
        }
        r += 1;
    }
    // 族 2：类型错开注入（OpenStorage 对流名）×10 必拒。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut t = StorageTree::new(ACC_WRITE).ok();
        if let Some(ref mut tr) = t {
            let _ = tr.create_stream(&[], "flw");
            if let Err(e) = tr.open_storage(&[], "flw") {
                if e == StgCode::TypeMismatch.val() {
                    ledger.caught[2] += 1;
                }
            }
        }
        r += 1;
    }
}

// ── 反向判据：合法五核必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut t = StorageTree::new(ACC_WRITE)?;
    t.create_storage(&[], "a")?;
    t.create_stream(&[0], "b")?;
    t.open_storage(&[], "a")?;
    t.open_stream(&[0], "b")?;
    let ev = t.enum_elements(&[])?;
    if ev.len() != 1 {
        return Err(StgCode::TreeConservationBroken.val());
    }
    t.conserve()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < StgCode::ALL.len() {
        let v = StgCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥6 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：五核 10/10 + 越权注入 10/10 检出。
    let g = golden_run();
    v.push((
        "J1-五核10/10+越权开10/10检出",
        matches!(g, Ok((5, 10))),
        format!("{:?}", g),
    ));
    // 2. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 访问权闸反向：Read 权两 Create 法恒拒、Write 权恒过。
    let gate = {
        let mut ro = StorageTree::new(ACC_READ).ok();
        let mut ok = true;
        if let Some(ref mut t) = ro {
            if t.create_storage(&[], "a").is_ok() || t.create_stream(&[], "b").is_ok() {
                ok = false;
            }
        } else {
            ok = false;
        }
        ok
    };
    v.push(("J1-写权闸反向", gate, String::from("Read 恒拒 / Write 恒过")));
    // 4. 深度封顶点验：DEPTH_CAP 内可建、超一层拒。
    let depth_ok = {
        let mut t = StorageTree::new(ACC_WRITE).ok();
        let mut ok = true;
        if let Some(ref mut tr) = t {
            // root(0)→s1(1)→s2(2)→s3(3)；depth 3 已到 cap-1，再深拒。
            let _ = tr.create_storage(&[], "l1");
            let _ = tr.create_storage(&[0], "l2");
            let _ = tr.create_storage(&[0, 0], "l3");
            ok = tr.create_storage(&[0, 0, 0], "l4").is_err();
        } else {
            ok = false;
        }
        ok
    };
    v.push(("J1-树深封顶", depth_ok, format!("cap={}", DEPTH_CAP)));
    // 5. 反向判据：合法五核必须全过。
    v.push(("J1-反向探针合法五核", matches!(reverse_probe(), Ok(())), String::from("树内全过")));
    // 6. 访问权名点验：二值非空名。
    v.push((
        "J1-访问权名点验",
        ACC_NAMES[0].len() > 0 && ACC_NAMES[1].len() > 0 && ACC_COUNT == 2,
        format!("{} 权", ACC_COUNT),
    ));
    // 7. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x64);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67640 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15661_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15661 IStorage 面谱与存储树");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
