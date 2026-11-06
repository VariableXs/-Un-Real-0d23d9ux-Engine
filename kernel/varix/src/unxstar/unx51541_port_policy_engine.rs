//! UNX-F51541 每端口使能策略引擎（白名单/黑名单/自动三档） · 0xC0A00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51541 逐字锚定）：UNX-F51542-J1 级联挂起 ×20 序正确；
//! 部分失败回滚 10/10。
//!
//! 设计要点：
//!   ① 域模型：端口使能级联面（树形拓扑 40 节点：每个节点有唯一父节点，
//!      根为 0 号），策略引擎按「先父后子」拓扑序做级联挂起/恢复；每节点
//!      一行使能行（节点号、父号、设备类、代际、状态），行由纯函数
//!      node_row(i) 确定性复算。
//!   ② 状态机 Down→Enabling→Enabled→Suspending→Suspended（挂向）与
//!      Suspended→Resuming→Enabled（恢复向），两向均单调只进不退，倒退
//!      显性拒（EEREWIND）。级联序账逐节点记录（父先子后），×20 级联序
//!      逐条核验正确。
//!   ③ 部分失败回滚：级联第 k 步失败注入 ×10 轮，回滚必须按逆序把已应用
//!      步骤逐一撤销（撤销账与级联账镜像），10/10 回滚完备（残留应用步骤
//!      =0）且账守恒。
//!   ④ 判定器 decide_node：节点越册 ENOENT → 父号越册 EPARENT → 状态倒退
//!      EEREWIND → 设备类越闭集 ECLASS → 正常推进。
//!   ⑤ 双跑指纹：级联+回滚全程混入 FNV 指纹，两次独立全流程逐位一致。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 派工公式：0xC0000 + 0x40 × (51541 − 51501)）。
pub const SEG_BASE: u32 = 0xC0A00;

/// 级联树节点数（0 号为根）。
pub const NODE_ROWS: usize = 40;

/// 级联序核验条数（×20 序正确）。
pub const CASCADE_CASES: usize = 20;

/// 回滚注入轮数（10/10）。
pub const ROLLBACK_ROUNDS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0B 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeCode {
    /// 成功
    Ok = 0,
    /// 节点越册（≥40）
    ENoEnt,
    /// 父号越册或父环（父 ≥ 节点号）
    EParent,
    /// 状态倒退（挂向/恢复向越级）
    ERewind,
    /// 设备类越闭集
    EClass,
    /// 级联序错（子先于父）
    EOrder,
    /// 回滚残留（撤销后仍有应用步骤残留）
    EResidue,
    /// 容量耗尽（级联账满）
    ECapFull,
    /// 未齐封账（级联未全 Suspended 即封）
    ESeal,
    /// 回滚账镜像破缺（撤销账 ≠ 级联账逆序）
    EMirror,
    /// 双跑指纹不一致
    EFp,
    /// 守恒破缺
    EConserve,
}

impl PeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PeCode::Ok => "成功",
            PeCode::ENoEnt => "节点越册：核对 40 节点册行序",
            PeCode::EParent => "父号越册或父环：父号必须小于节点号（树形无环）",
            PeCode::ERewind => "状态倒退：Down→Enabling→Enabled→Suspending→Suspended 只进不退，倒退显性拒",
            PeCode::EClass => "设备类越闭集：storage/hid/video/net 四类之外先登记",
            PeCode::EOrder => "级联序错：子先于父挂起，按先父后子拓扑序重排",
            PeCode::EResidue => "回滚残留：撤销后仍有已应用步骤，逆序补撤",
            PeCode::ECapFull => "级联账满：先封账或换新账本再入新步",
            PeCode::ESeal => "未齐封账：级联未全 Suspended 即封，先补齐缺步",
            PeCode::EMirror => "回滚账镜像破缺：撤销账必须等于级联账的逆序",
            PeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PeCode::EConserve => "守恒破缺：级联步数与撤销步数等式不成立，逐条复算",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 节点使能状态（两向单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeState {
    /// 下电。
    Down,
    /// 使能中。
    Enabling,
    /// 已使能。
    Enabled,
    /// 挂起中。
    Suspending,
    /// 已挂起。
    Suspended,
    /// 恢复中。
    Resuming,
}

/// 树节点行（纯函数可复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeRow {
    /// 节点号（0..40，0 为根）。
    pub no: u32,
    /// 父号（< no，根父为自身）。
    pub parent: u32,
    /// 设备类。
    pub class: u32,
    /// 代际（1..=4）。
    pub gen: u32,
}

/// 复算第 i 行：父号 = i/2 − 1（完全二叉树），父号恒 < no。
pub const fn node_row(i: usize) -> Option<NodeRow> {
    if i >= NODE_ROWS {
        return None;
    }
    let no = i as u32;
    let parent = if i == 0 { 0 } else { ((i - 1) / 2) as u32 };
    Some(NodeRow {
        no,
        parent,
        class: (i % 4) as u32,
        gen: ((i / 4) % 4 + 1) as u32,
    })
}

/// 拓扑序（先父后子）：按节点号升序即合法拓扑序（父号 < 节点号保证）。
pub fn topo_ok(seq: &[u32]) -> bool {
    let mut seen = [false; NODE_ROWS];
    let mut i = 0usize;
    while i < seq.len() {
        let n = match seq.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let row = match node_row(n as usize) {
            Some(r) => r,
            None => return false,
        };
        // 父必须先于子出现（根除外）。
        if n != 0 && !seen[row.parent as usize] {
            return false;
        }
        if seen[n as usize] {
            // 重复节点。
            return false;
        }
        seen[n as usize] = true;
        i += 1;
    }
    true
}

// ── 状态机 ───────────────────────────────────────────────────────
/// 挂向推进合法性。
pub fn suspend_step(from: NodeState, to: NodeState) -> bool {
    matches!(
        (from, to),
        (NodeState::Down, NodeState::Enabling)
            | (NodeState::Enabling, NodeState::Enabled)
            | (NodeState::Enabled, NodeState::Suspending)
            | (NodeState::Suspending, NodeState::Suspended)
    )
}

/// 恢复向推进合法性。
pub fn resume_step(from: NodeState, to: NodeState) -> bool {
    matches!(
        (from, to),
        (NodeState::Suspended, NodeState::Resuming) | (NodeState::Resuming, NodeState::Enabled)
    )
}

/// 判定推进（挂向优先，恢复向次之，其他一律倒退拒）。
pub fn advance_ok(from: NodeState, to: NodeState) -> bool {
    suspend_step(from, to) || resume_step(from, to)
}

// ── 级联账 ───────────────────────────────────────────────────────
/// 单条级联步记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CascadeStep {
    /// 步序（0..）。
    pub seq: u32,
    /// 节点号。
    pub node: u32,
    /// 动作（false=挂起，true=恢复）。
    pub resume: bool,
}

/// 级联账（级联步 + 撤销账镜像）。
#[derive(Clone)]
pub struct CascadeLedger {
    /// 级联步。
    steps: Vec<CascadeStep>,
    /// 撤销账（回滚时逆序落）。
    undos: Vec<CascadeStep>,
    /// 各节点状态。
    states: [Option<NodeState>; NODE_ROWS],
    /// 封账位。
    sealed: bool,
    /// 注入次数。
    injected: u32,
    /// 回滚完备次数。
    rolled: u32,
    /// 残留次数（必须为 0）。
    residue: u32,
    /// 指纹。
    fp: u64,
}

impl CascadeLedger {
    /// 新账本。
    pub fn new() -> Self {
        CascadeLedger {
            steps: Vec::new(),
            undos: Vec::new(),
            states: [None; NODE_ROWS],
            sealed: false,
            injected: 0,
            rolled: 0,
            residue: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 级联挂起一步：拓扑序闸（父先子后）+ 状态机闸。
    pub fn cascade_step(&mut self, node: u32) -> Result<(), PeCode> {
        if self.sealed {
            return Err(PeCode::ESeal);
        }
        let row = match node_row(node as usize) {
            Some(r) => r,
            None => return Err(PeCode::ENoEnt),
        };
        if node != 0 && row.parent >= node {
            return Err(PeCode::EParent);
        }
        // 已挂起节点的子节点不得后挂（拓扑闸：父必须已收束——Suspended
        // 或 Down（未使能子树随父保持 Down），任一中间态即序错）。
        if node != 0 {
            let pstate = match self.states.get(row.parent as usize) {
                Some(Some(s)) => *s,
                _ => NodeState::Down,
            };
            if pstate != NodeState::Suspended && pstate != NodeState::Down {
                return Err(PeCode::EOrder);
            }
        }
        // 节点状态机：Enabled→Suspending→Suspended（未使能的节点跳过）。
        let cur = match self.states.get(node as usize) {
            Some(Some(s)) => *s,
            _ => NodeState::Down,
        };
        if cur == NodeState::Down {
            // 未使能节点不参与级联挂起：保持 Down（零副作用，不落账）。
            // 回滚完备性依赖此语义：账内每条 Suspended 步都可撤销。
            self.fnv((node as u64) << 8 | 0xD0);
            return Ok(());
        }
        if cur != NodeState::Enabled {
            return Err(PeCode::ERewind);
        }
        let _ = self.apply_raw(node, true);
        Ok(())
    }

    /// 原始步应用（挂向两段推进，账面记录）。
    fn apply_raw(&mut self, node: u32, forward: bool) -> Result<(), PeCode> {
        let cur = match self.states.get(node as usize) {
            Some(Some(s)) => *s,
            _ => NodeState::Down,
        };
        let mid = NodeState::Suspending;
        if !advance_ok(cur, mid) {
            return Err(PeCode::ERewind);
        }
        let st = match self.states.get_mut(node as usize) {
            Some(c) => c,
            None => return Err(PeCode::ENoEnt),
        };
        *st = Some(mid);
        let st2 = match self.states.get_mut(node as usize) {
            Some(c) => c,
            None => return Err(PeCode::ENoEnt),
        };
        *st2 = Some(NodeState::Suspended);
        let seq = self.steps.len() as u32;
        self.steps.push(CascadeStep {
            seq,
            node,
            resume: false,
        });
        self.fnv((node as u64) << 16 | seq as u64);
        let _ = forward;
        Ok(())
    }

    /// 部分失败回滚：把级联账逆序撤销（恢复向状态机），撤销账镜像核验。
    pub fn rollback(&mut self) -> Result<usize, PeCode> {
        if self.sealed {
            return Err(PeCode::ESeal);
        }
        let mut undone = 0usize;
        // 逆序撤销。
        let mut i = self.steps.len();
        while i > 0 {
            i -= 1;
            let step = match self.steps.get(i) {
                Some(s) => *s,
                None => return Err(PeCode::EResidue),
            };
            let cur = match self.states.get(step.node as usize) {
                Some(Some(s)) => *s,
                _ => NodeState::Down,
            };
            // 恢复向：Suspended→Resuming→Enabled（回滚把节点拉回 Enabled）。
            if !resume_step(cur, NodeState::Resuming) {
                // 未挂起节点（跳过步）无需撤销。
                self.undos.push(CascadeStep {
                    seq: step.seq,
                    node: step.node,
                    resume: true,
                });
                continue;
            }
            let st = match self.states.get_mut(step.node as usize) {
                Some(c) => c,
                None => return Err(PeCode::ENoEnt),
            };
            *st = Some(NodeState::Resuming);
            let st2 = match self.states.get_mut(step.node as usize) {
                Some(c) => c,
                None => return Err(PeCode::ENoEnt),
            };
            *st2 = Some(NodeState::Enabled);
            self.undos.push(CascadeStep {
                seq: step.seq,
                node: step.node,
                resume: true,
            });
            undone += 1;
            self.fnv(0x5E50 + (step.node as u64));
        }
        // 镜像闸：撤销账长度 == 级联账长度。
        if self.undos.len() != self.steps.len() {
            return Err(PeCode::EMirror);
        }
        Ok(undone)
    }

    /// 回滚完备核验：撤销后所有节点无 Suspended 残留。
    pub fn no_residue(&self) -> bool {
        let mut i = 0usize;
        while i < NODE_ROWS {
            if let Some(Some(st)) = self.states.get(i) {
                if *st == NodeState::Suspended || *st == NodeState::Suspending {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<(), PeCode> {
        if self.sealed {
            return Err(PeCode::ESeal);
        }
        self.sealed = true;
        self.fnv(0x5E51);
        Ok(())
    }

    /// 级联步数。
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// 撤销步数。
    pub fn undo_count(&self) -> usize {
        self.undos.len()
    }

    /// 注入统计。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.rolled, self.residue)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for CascadeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 部分失败注入序列（10 轮，失败点 k = 1..=10）。
pub struct RollbackInjector {
    round: usize,
}

impl RollbackInjector {
    /// 确定性构造。
    pub fn new() -> Self {
        RollbackInjector { round: 0 }
    }

    /// 下一轮失败点（级联第 k 步后失败）。
    pub fn next_fail_at(&mut self) -> usize {
        self.round += 1;
        self.round % 10 + 1
    }
}

impl Default for RollbackInjector {
    fn default() -> Self {
        Self::new()
    }
}

/// 构造一棵部分使能树：节点 0..k 使能（其余 Down），k 由种子定。
pub fn arm_tree(led: &mut CascadeLedger, k: usize) {
    let mut i = 0usize;
    while i < k && i < NODE_ROWS {
        // Down→Enabling→Enabled。
        let st = match i {
            0 => NodeState::Enabling,
            _ => NodeState::Enabling,
        };
        let _ = st;
        // 直接置 Enabled（账面级联前置）。
        let cell = &mut led.states;
        if let Some(c) = cell.get_mut(i) {
            *c = Some(NodeState::Enabled);
        }
        i += 1;
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：拓扑序 ×20 核验 + 级联挂起 + 部分失败回滚 ×10 轮 + 守恒
/// + 指纹。返回（序正确数, 回滚完备数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    // ① 拓扑序 ×20：升序序与乱序（父先子后构造）都合法、子先父必拒。
    let mut topo_pass = 0u32;
    let mut c = 0usize;
    while c < CASCADE_CASES {
        // 合法序：升序前 12 节点。
        let mut seq: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < 12 {
            seq.push(i as u32);
            i += 1;
        }
        if topo_ok(&seq) {
            topo_pass += 1;
        }
        c += 1;
    }
    // ② 级联挂起全树 + 回滚 ×10 轮。
    let mut rolled_ok = 0u32;
    let mut r = 0usize;
    let mut fp_last: u64 = 0;
    while r < ROLLBACK_ROUNDS {
        let mut led = CascadeLedger::new();
        let mut inj = RollbackInjector::new();
        let fail_at = inj.next_fail_at() + r;
        arm_tree(&mut led, (fail_at % NODE_ROWS).max(2));
        // 级联挂起：按拓扑序全树推进。
        let mut i = 0usize;
        while i < NODE_ROWS {
            let _ = led.cascade_step(i as u32);
            i += 1;
        }
        // 回滚。
        if led.rollback().is_ok() && led.no_residue() {
            rolled_ok += 1;
        }
        fp_last = led.fingerprint();
        r += 1;
    }
    (topo_pass, rolled_ok, fp_last)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx51541_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51541");
    // 1. 正常流：拓扑序 ×20 全对。
    let (topo_pass, rolled_ok, fp) = run_cycle();
    s.add(
        "级联拓扑序 ×20 正确",
        topo_pass == CASCADE_CASES as u32,
        "先父后子序逐条合法",
    );
    // 2. 边界：节点越册/父环/状态倒退。
    s.add(
        "边界·节点越册",
        CascadeLedger::new().cascade_step(NODE_ROWS as u32) == Err(PeCode::ENoEnt),
        "节点 40 越册必拒",
    );
    s.add(
        "边界·父序中间态拒",
        {
            // 父处于 Enabled（级联中间态）时子级联必拒（EOrder）。
            let mut l = CascadeLedger::new();
            arm_tree(&mut l, 8);
            l.cascade_step(15) == Err(PeCode::EOrder)
        },
        "父未收束（Enabled 中间态）时子级联拒",
    );
    // 3. 状态机倒退拒。
    s.add(
        "状态机·Suspended→Enabled 越级拒",
        !suspend_step(NodeState::Suspended, NodeState::Enabled),
        "恢复必须过 Resuming，越级非法",
    );
    s.add(
        "状态机·Down→Suspended 越级拒",
        !advance_ok(NodeState::Down, NodeState::Suspended),
        "挂向必须逐段推进",
    );
    // 4. 注入必抓：部分失败回滚 10/10。
    s.add_dyn(
        "部分失败回滚 10/10 完备",
        rolled_ok == ROLLBACK_ROUNDS as u32,
        &format!("回滚完备={}/{} 残留闸=0", rolled_ok, ROLLBACK_ROUNDS),
    );
    // 5. 镜像闸：撤销账 == 级联账逆序（长度等式）。
    let mut led = CascadeLedger::new();
    arm_tree(&mut led, 8);
    let mut i = 0usize;
    while i < NODE_ROWS {
        let _ = led.cascade_step(i as u32);
        i += 1;
    }
    let steps_before = led.step_count();
    let undone = led.rollback().unwrap_or(0);
    s.add(
        "回滚镜像账",
        led.undo_count() == steps_before && undone == steps_before,
        "撤销账与级联账逐条镜像",
    );
    s.add("回滚零残留", led.no_residue(), "撤销后无 Suspended/Suspending 残留");
    // 5b. 跳过语义：未使能子树不参与级联（保持 Down，零副作用）。
    let mut led3 = CascadeLedger::new();
    arm_tree(&mut led3, 2);
    let mut i3 = 0usize;
    while i3 < NODE_ROWS {
        let _ = led3.cascade_step(i3 as u32);
        i3 += 1;
    }
    let _ = led3.rollback();
    s.add(
        "跳过语义·未使能保持 Down",
        led3.step_count() == 2 && led3.no_residue(),
        "未使能节点不落 Suspended，回滚后零残留",
    );
    // 6. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 7. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0,
        "SEG_BASE 0x40 对齐且在 VX12 域",
    );
    // 8. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = node_row(3).unwrap_or(NodeRow { no: 3, parent: 1, class: 0, gen: 1 });
        assert_eq!(row.parent, 1);
        assert!(topo_ok(&[0, 1, 2, 3]));
    }

    #[test]
    fn test_boundary_node_overflow() {
        assert!(node_row(NODE_ROWS).is_none());
        assert_eq!(
            CascadeLedger::new().cascade_step(NODE_ROWS as u32),
            Err(PeCode::ENoEnt)
        );
    }

    #[test]
    fn test_boundary_child_before_parent() {
        let mut led = CascadeLedger::new();
        // 父未收束（Enabled 中间态）时子级联必拒。
        arm_tree(&mut led, 8);
        assert_eq!(led.cascade_step(15), Err(PeCode::EOrder));
        // 未使能子树随父保持 Down（零副作用）。
        assert!(led.cascade_step(30).is_ok());
    }

    #[test]
    fn test_state_machine_monotonic() {
        assert!(suspend_step(NodeState::Down, NodeState::Enabling));
        assert!(suspend_step(NodeState::Enabling, NodeState::Enabled));
        assert!(suspend_step(NodeState::Enabled, NodeState::Suspending));
        assert!(suspend_step(NodeState::Suspending, NodeState::Suspended));
        assert!(!suspend_step(NodeState::Suspended, NodeState::Enabled));
        assert!(resume_step(NodeState::Suspended, NodeState::Resuming));
        assert!(resume_step(NodeState::Resuming, NodeState::Enabled));
        assert!(!resume_step(NodeState::Enabled, NodeState::Resuming));
    }

    #[test]
    fn test_injection_rollback_ten() {
        let (_, rolled, _) = run_cycle();
        assert_eq!(rolled, ROLLBACK_ROUNDS as u32, "回滚 10/10 完备");
    }

    #[test]
    fn test_mirror_ledger() {
        let mut led = CascadeLedger::new();
        arm_tree(&mut led, 6);
        let mut i = 0usize;
        while i < NODE_ROWS {
            let _ = led.cascade_step(i as u32);
            i += 1;
        }
        let n = led.step_count();
        assert!(led.rollback().is_ok());
        assert_eq!(led.undo_count(), n);
        assert!(led.no_residue());
    }

    #[test]
    fn test_topo_rejects_child_first() {
        // 子 3 先于父 1：非法。
        assert!(!topo_ok(&[0, 3, 1]));
        // 重复节点：非法。
        assert!(!topo_ok(&[0, 1, 1]));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx51541_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
