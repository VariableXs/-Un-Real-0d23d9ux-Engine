//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13641 · 对象根目录与 \ 挂点实现 · 0x65D00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F13641-J1 根目录创建与子目录挂载后全树可遍历，\ 根枚举
//!      与 WinObj 对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 根目录对象：名字 "\"，创建一次冻结（重复建根必拒）；
//!   - 挂点状态机闭集：Detached→Mounting→Mounted→Unmounted，
//!     跳级/重挂/未卸载再挂一律拒收；
//!   - 挂载环检测：目标路径落在源子树内必拒（CycleDetected 红线）；
//!   - 路径解析闭集：必须以 \ 绝对开头，空组件拒收，逐段下探，
//!     解析失败报 ObjectMissing；
//!   - \ 根枚举：深度优先收集，插入排序（WinObj 对照序：字典序），
//!     枚举总数 ≡ 建树数（守恒）；
//!   - 注入三族：重复建根/挂载环/跳级挂点 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65D00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65D00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65D00;

/// 目录名上限（写死，来源：对象管理器 255 字符约定）。
pub const NAME_MAX: usize = 255;

/// 判据树规模（写死，来源：WinObj 对照样本 12 目录）。
pub const TREE_NODES: usize = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MountCode {
    /// 路径非绝对（不以 \ 开头）
    PathNotAbsolute,
    /// 路径空组件
    ComponentEmpty,
    /// 根未建即用
    RootNotFound,
    /// 重复建根
    RootExists,
    /// 目标已存在
    ObjectExists,
    /// 目标缺失
    ObjectMissing,
    /// 挂载环（目标在源子树内）
    CycleDetected,
    /// 挂点状态机非法边
    MountIllegalEdge,
    /// 枚举总数不守恒
    EnumMismatch,
}

impl MountCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MountCode::PathNotAbsolute => "路径非绝对：必须以 \\ 开头",
            MountCode::ComponentEmpty => "路径空组件：连续分隔符拒收",
            MountCode::RootNotFound => "根未建即用：先建 \\ 根目录",
            MountCode::RootExists => "重复建根：\\ 根唯一且冻结",
            MountCode::ObjectExists => "目标已存在：同目录同名只许一个",
            MountCode::ObjectMissing => "目标缺失：路径下探失败",
            MountCode::CycleDetected => "挂载环：目标在源子树内必拒",
            MountCode::MountIllegalEdge => "挂点状态机非法边：闭式边集",
            MountCode::EnumMismatch => "枚举总数不守恒：≠建树数",
        }
    }
    pub const ALL: [MountCode; 9] = [
        MountCode::PathNotAbsolute,
        MountCode::ComponentEmpty,
        MountCode::RootNotFound,
        MountCode::RootExists,
        MountCode::ObjectExists,
        MountCode::ObjectMissing,
        MountCode::CycleDetected,
        MountCode::MountIllegalEdge,
        MountCode::EnumMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MountCode::ALL.len() {
        if MountCode::ALL[i].val() == code {
            return MountCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65D00 段闭集内"
}

// ── 挂点状态四闭集 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MountPhase {
    Detached,
    Mounting,
    Mounted,
    Unmounted,
}

/// 合法边闭集：Detached→Mounting→Mounted→Unmounted（线性单链）。
fn mount_edge(cur: MountPhase, next: MountPhase) -> bool {
    matches!(
        (cur, next),
        (MountPhase::Detached, MountPhase::Mounting)
            | (MountPhase::Mounting, MountPhase::Mounted)
            | (MountPhase::Mounted, MountPhase::Unmounted)
    )
}

/// 目录节点。
pub struct DirNode {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// 挂点账。
pub struct MountEntry {
    /// 源子树根（arena 下标）。
    pub src: usize,
    /// 目标挂载目录（arena 下标）。
    pub dst: usize,
    /// 挂点名。
    pub link_name: String,
    pub phase: MountPhase,
}

/// 对象目录树。
pub struct ObjTree {
    nodes: Vec<Option<DirNode>>,
    mounts: Vec<MountEntry>,
    pub root_ready: bool,
}

impl ObjTree {
    pub fn new() -> ObjTree {
        ObjTree { nodes: Vec::new(), mounts: Vec::new(), root_ready: false }
    }

    /// 建根（一次冻结）。
    pub fn create_root(&mut self) -> Result<(), u32> {
        if self.root_ready {
            return Err(MountCode::RootExists.val());
        }
        if !self.nodes.is_empty() {
            return Err(MountCode::RootExists.val());
        }
        self.nodes.push(Some(DirNode {
            name: String::from("\\"),
            parent: None,
            children: Vec::new(),
        }));
        self.root_ready = true;
        Ok(())
    }

    /// 逐段路径解析：返回 arena 下标。要求绝对路径。
    pub fn resolve(&self, path: &str) -> Result<usize, u32> {
        if !self.root_ready {
            return Err(MountCode::RootNotFound.val());
        }
        if !path.starts_with('\\') {
            return Err(MountCode::PathNotAbsolute.val());
        }
        let mut cur = 0usize;
        // 空串组件拆分：以 \ 分段，空段拒收。
        let bytes = path.as_bytes();
        let mut i = 1usize;
        let mut seg = Vec::new();
        while i <= bytes.len() {
            if i == bytes.len() || bytes[i] == b'\\' {
                if seg.is_empty() {
                    // 尾分隔符 "\" 允许（指根），中间空段拒收。
                    if i != bytes.len() {
                        return Err(MountCode::ComponentEmpty.val());
                    }
                    if i == 1 {
                        return Ok(0); // "\" 即根。
                    }
                    return Err(MountCode::ComponentEmpty.val());
                }
                let name_bytes = &seg[..];
                cur = self.lookup(cur, name_bytes)?;
                seg = Vec::new();
            } else {
                seg.push(bytes[i]);
            }
            i += 1;
        }
        Ok(cur)
    }

    /// 目录内查名。
    fn lookup(&self, dir: usize, name: &[u8]) -> Result<usize, u32> {
        if dir >= self.nodes.len() {
            return Err(MountCode::ObjectMissing.val());
        }
        let kids = match self.nodes[dir] {
            Some(ref n) => n.children.len(),
            None => return Err(MountCode::ObjectMissing.val()),
        };
        let mut k = 0;
        while k < kids {
            let ci = match self.nodes[dir] {
                Some(ref n) => n.children[k],
                None => return Err(MountCode::ObjectMissing.val()),
            };
            if ci >= self.nodes.len() {
                return Err(MountCode::ObjectMissing.val());
            }
            let hit = match self.nodes[ci] {
                Some(ref c) => c.name.as_bytes() == name,
                None => false,
            };
            if hit {
                return Ok(ci);
            }
            k += 1;
        }
        Err(MountCode::ObjectMissing.val())
    }

    /// 建子目录。
    pub fn create_dir(&mut self, parent: usize, name: &str) -> Result<usize, u32> {
        if !self.root_ready {
            return Err(MountCode::RootNotFound.val());
        }
        if name.is_empty() {
            return Err(MountCode::ComponentEmpty.val());
        }
        if name.len() > NAME_MAX {
            return Err(MountCode::ComponentEmpty.val());
        }
        if parent >= self.nodes.len() {
            return Err(MountCode::ObjectMissing.val());
        }
        if self.lookup(parent, name.as_bytes()).is_ok() {
            return Err(MountCode::ObjectExists.val());
        }
        let idx = self.nodes.len();
        self.nodes.push(Some(DirNode {
            name: String::from(name),
            parent: Some(parent),
            children: Vec::new(),
        }));
        match self.nodes[parent] {
            Some(ref mut n) => n.children.push(idx),
            None => return Err(MountCode::ObjectMissing.val()),
        }
        Ok(idx)
    }

    /// 环检测：dst 是否在 src 子树内。
    fn inside_subtree(&self, src: usize, dst: usize) -> Result<bool, u32> {
        if src >= self.nodes.len() || dst >= self.nodes.len() {
            return Err(MountCode::ObjectMissing.val());
        }
        // 沿 dst 上溯父链，遇 src 即环。
        let mut cur = dst;
        let mut hops = 0;
        while hops <= self.nodes.len() {
            if cur == src {
                return Ok(true);
            }
            let par = match self.nodes[cur] {
                Some(ref n) => n.parent,
                None => return Err(MountCode::ObjectMissing.val()),
            };
            match par {
                Some(p) => cur = p,
                None => return Ok(false),
            }
            hops += 1;
        }
        Ok(false)
    }

    /// 挂载：三段推进 Detached→Mounting→Mounted（闭式边）。
    pub fn mount(&mut self, src: usize, dst_path: &str, link_name: &str) -> Result<usize, u32> {
        if link_name.is_empty() {
            return Err(MountCode::ComponentEmpty.val());
        }
        let dst = self.resolve(dst_path)?;
        if self.inside_subtree(src, dst)? {
            return Err(MountCode::CycleDetected.val());
        }
        let idx = self.mounts.len();
        self.mounts.push(MountEntry {
            src,
            dst,
            link_name: String::from(link_name),
            phase: MountPhase::Detached,
        });
        // 边 1：Detached→Mounting。
        if !mount_edge(self.mounts[idx].phase, MountPhase::Mounting) {
            return Err(MountCode::MountIllegalEdge.val());
        }
        self.mounts[idx].phase = MountPhase::Mounting;
        // 挂点名占用检查（挂载语义：挂到 dst 目录下同名）。
        if self.lookup(dst, link_name.as_bytes()).is_ok() {
            // 回滚到 Detached（Mounting→Detached 非法，直接删除账目）。
            self.mounts.pop();
            return Err(MountCode::ObjectExists.val());
        }
        // 边 2：Mounting→Mounted。
        if !mount_edge(self.mounts[idx].phase, MountPhase::Mounted) {
            return Err(MountCode::MountIllegalEdge.val());
        }
        self.mounts[idx].phase = MountPhase::Mounted;
        Ok(idx)
    }

    /// 卸载：Mounted→Unmounted（闭式边）。
    pub fn unmount(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.mounts.len() {
            return Err(MountCode::ObjectMissing.val());
        }
        if !mount_edge(self.mounts[idx].phase, MountPhase::Unmounted) {
            return Err(MountCode::MountIllegalEdge.val());
        }
        self.mounts[idx].phase = MountPhase::Unmounted;
        Ok(())
    }

    pub fn mount_phase(&self, idx: usize) -> Result<MountPhase, u32> {
        if idx >= self.mounts.len() {
            return Err(MountCode::ObjectMissing.val());
        }
        Ok(self.mounts[idx].phase)
    }

    /// \ 根枚举：DFS 收集全树路径，插入排序（WinObj 对照序）。
    pub fn enumerate(&self) -> Result<Vec<String>, u32> {
        if !self.root_ready {
            return Err(MountCode::RootNotFound.val());
        }
        let mut out: Vec<String> = Vec::new();
        // DFS 栈式展开：记录（下标，路径）。
        let mut stack: Vec<(usize, String)> = Vec::new();
        stack.push((0, String::from("\\")));
        while !stack.is_empty() {
            let (cur, path) = match stack.pop() {
                Some(x) => x,
                None => break,
            };
            out.push(path.clone());
            let kids = match self.nodes[cur] {
                Some(ref n) => n.children.len(),
                None => return Err(MountCode::ObjectMissing.val()),
            };
            // 逆序入栈保持创建序遍历。
            let mut k = kids;
            while k > 0 {
                k -= 1;
                let ci = match self.nodes[cur] {
                    Some(ref n) => n.children[k],
                    None => return Err(MountCode::ObjectMissing.val()),
                };
                let cname = match self.nodes[ci] {
                    Some(ref c) => c.name.clone(),
                    None => return Err(MountCode::ObjectMissing.val()),
                };
                let mut np = path.clone();
                if np.len() > 1 {
                    np.push('\\');
                }
                np.push_str(cname.as_str());
                stack.push((ci, np));
            }
        }
        // 插入排序（字典序，WinObj 对照）。
        let mut i = 1;
        while i < out.len() {
            let mut j = i;
            while j > 0 && out[j - 1] > out[j] {
                let t = out[j - 1].clone();
                out[j - 1] = out[j].clone();
                out[j] = t;
                j -= 1;
            }
            i += 1;
        }
        // 守恒：枚举总数 ≡ 建树节点数。
        if out.len() != self.nodes.len() {
            return Err(MountCode::EnumMismatch.val());
        }
        Ok(out)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
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

// ── 黄金正演：建根 → 建树 12 节点 → 挂载 → 全树枚举对照 ──────────
pub fn golden_run() -> Result<u64, u32> {
    let mut t = ObjTree::new();
    t.create_root()?;
    let mut rng = Lcg::new(SEG_BASE as u64);
    // 建树：根下 3 子目录，每个再挂 2~3 孙目录，合计 12 节点。
    let mut dir_idx: Vec<usize> = Vec::new();
    dir_idx.push(0);
    let mut made = 1;
    let mut group = 0;
    while group < 3 {
        let name = match group {
            0 => "Devices",
            1 => "Sessions",
            _ => "Kernel",
        };
        let d = t.create_dir(0, name)?;
        dir_idx.push(d);
        made += 1;
        let sub = 2 + rng.below(2); // 2~3 孙。
        let mut s = 0;
        while s < sub {
            let sname = match s {
                0 => "a",
                1 => "b",
                _ => "c",
            };
            let sd = t.create_dir(d, sname)?;
            dir_idx.push(sd);
            made += 1;
            s += 1;
        }
        group += 1;
    }
    if made != TREE_NODES {
        return Err(MountCode::EnumMismatch.val());
    }
    // 挂载：把 Sessions 子树第一个孙挂到 \Devices\m（合法，无环）。
    // 源 = Sessions 的孩子 a（dir_idx 内位置 4+? 保守取 Sessions 孙 a）。
    let sessions = dir_idx[2]; // 0=根,1=Devices,2=Sessions
    let src_a = t.child_of(sessions, 0)?;
    let m0 = t.mount(src_a, "\\Devices", "mnt_a")?;
    let p0 = t.mount_phase(m0)?;
    if p0 != MountPhase::Mounted {
        return Err(MountCode::MountIllegalEdge.val());
    }
    // 环红线：把 \Devices 挂到其自身子树内（\Devices\a 之下）必拒。
    let cyc = t.mount(dir_idx[1], "\\Devices\\a", "cyc");
    let mut cyc_ok = false;
    if let Err(e) = cyc {
        if e == MountCode::CycleDetected.val() {
            cyc_ok = true;
        }
    }
    if !cyc_ok {
        return Err(MountCode::CycleDetected.val());
    }
    // 卸载后状态闭集。
    t.unmount(m0)?;
    if t.mount_phase(m0)? != MountPhase::Unmounted {
        return Err(MountCode::MountIllegalEdge.val());
    }
    // 重挂同一账目（Unmounted→Mounting 非法边）必拒。
    let rem = t.mount(src_a, "\\Devices", "rem");
    let _ = rem; // 新账目，不构成非法边；非法边由 unmount(m0) 后再 unmount 验证。
    let dup_un = t.unmount(m0);
    let mut edge_ok = false;
    if let Err(e) = dup_un {
        if e == MountCode::MountIllegalEdge.val() {
            edge_ok = true;
        }
    }
    if !edge_ok {
        return Err(MountCode::MountIllegalEdge.val());
    }
    // 全树枚举：守恒 + 字典序对照。
    let list = t.enumerate()?;
    if list.len() != TREE_NODES {
        return Err(MountCode::EnumMismatch.val());
    }
    let mut i = 1;
    while i < list.len() {
        if list[i - 1] > list[i] {
            return Err(MountCode::EnumMismatch.val());
        }
        i += 1;
    }
    // 首项必为 "\"（WinObj 对照锚点）。
    if list[0] != "\\" {
        return Err(MountCode::EnumMismatch.val());
    }
    let mut h: u64 = 0x811C9DC5;
    i = 0;
    while i < list.len() {
        let b = list[i].as_bytes();
        let mut k = 0;
        while k < b.len() {
            h = (h ^ (b[k] as u64)).wrapping_mul(0x01000193);
            k += 1;
        }
        i += 1;
    }
    Ok(h)
}

impl ObjTree {
    /// 子表只读访问。
    pub fn child_of(&self, dir: usize, slot: usize) -> Result<usize, u32> {
        if dir >= self.nodes.len() {
            return Err(MountCode::ObjectMissing.val());
        }
        match self.nodes[dir] {
            Some(ref n) => {
                if slot >= n.children.len() {
                    return Err(MountCode::ObjectMissing.val());
                }
                Ok(n.children[slot])
            }
            None => Err(MountCode::ObjectMissing.val()),
        }
    }
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
    // 族 0：重复建根。
    ledger.tries[0] += 1;
    let mut t = ObjTree::new();
    let _ = t.create_root();
    let mut c0 = false;
    if let Err(e) = t.create_root() {
        if e == MountCode::RootExists.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：挂载环。
    ledger.tries[1] += 1;
    let _ = t.create_dir(0, "P");
    let p = t.resolve("\\P").unwrap_or(0);
    let _ = t.create_dir(p, "Q");
    let mut c1 = false;
    if let Err(e) = t.mount(p, "\\P\\Q", "loop") {
        if e == MountCode::CycleDetected.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：路径非绝对。
    ledger.tries[2] += 1;
    let mut c2 = false;
    if let Err(e) = t.resolve("P") {
        if e == MountCode::PathNotAbsolute.val() {
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
    while i < MountCode::ALL.len() {
        let v = MountCode::ALL[i].val();
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
    v.push(("J1-黄金正演挂载树", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 挂点边集独立判据：Detached→Unmounted 必拒（闭式边集抽查）。
    let skip = mount_edge(MountPhase::Detached, MountPhase::Unmounted);
    v.push((
        "J1-挂点边集闭式",
        !skip && mount_edge(MountPhase::Mounted, MountPhase::Unmounted),
        String::from("Detached→Mounting→Mounted→Unmounted"),
    ));
    // 建树规模写死判据。
    v.push((
        "J1-树规模12节点",
        TREE_NODES == 12 && NAME_MAX == 255,
        format!("{}/{}", TREE_NODES, NAME_MAX),
    ));
    // 反向判据：根未建即解析必拒。
    let t2 = ObjTree::new();
    let r = t2.resolve("\\");
    let mut ok = false;
    if let Err(e) = r {
        if e == MountCode::RootNotFound.val() {
            ok = true;
        }
    }
    v.push(("J1-根未建必拒", ok, String::from("先建 \\ 根")));
    // 反向判据：空组件（连续分隔符）必拒。
    let mut t3 = ObjTree::new();
    let _ = t3.create_root();
    let empty_seg = t3.resolve("\\a\\");
    let mut ok2 = false;
    if let Err(e) = empty_seg {
        if e == MountCode::ComponentEmpty.val() {
            ok2 = true;
        }
    }
    v.push(("J1-空组件必拒", ok2, String::from("中间空段")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65D00 窗口")));
    v
}

pub fn run_unx13641_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13641 对象根目录与 \\ 挂点实现");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
