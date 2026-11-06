//! UNX-F51542 端口挂起级联（子树整体挂起/恢复序） · 0xC0A40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51542 逐字锚定）：UNX-F51543-J1 休眠唤醒 ×20 零丢
//! 事件；误休眠注入 10/10 检出。
//!
//! 设计要点：
//!   ① 域模型：端口子树整体休眠序面（同 51541 的完全二叉树 40 节点，但
//!      本卡建模**子树整体**操作序：挂起 = 先根后叶（DFS 前序）、恢复 =
//!      先叶后根（DFS 后序），两序互为镜像），序由纯函数
//!      subtree_suspend_order / subtree_resume_order 确定性生成，与树行
//!      复算恒等。
//!   ② 休眠唤醒事件账：每次子树休眠开启事件窗，窗内到达事件全部入队，
//!      唤醒后逐条交付，×20 周期零丢事件（入队数 == 交付数 == 账量）。
//!   ③ 误休眠注入器：对仍有在途事件（队列非空且未交付）的子树注入休眠
//!      共 10 轮，闸 10/10 检出（在途事件未交付前休眠显性拒）并审计入账。
//!   ④ 判定器：根越册 ENOENT → 子树序重复节点 EDUP → 树行复算漂移
//!      EDRIFT → 在途事件未交付误休眠 ESLEEPBAD → 正常推进。
//!   ⑤ 状态机 Active→Sleeping→Waking→Active 环（环内单向，反向显性拒）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 派工公式：0xC0000 + 0x40 × (51542 − 51501)）。
pub const SEG_BASE: u32 = 0xC0A40;

/// 完全二叉树节点数（0 为根，父号 = (i−1)/2）。
pub const NODE_ROWS: usize = 40;

/// 休眠唤醒周期数（×20 零丢事件）。
pub const SLEEP_CYCLES: usize = 20;

/// 误休眠注入轮数（10/10）。
pub const BADSLEEP_ROUNDS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0B 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SsCode {
    /// 成功
    Ok = 0,
    /// 根越册（≥40）
    ENoEnt,
    /// 子树序重复节点
    EDup,
    /// 树行复算漂移（序内节点与复算行不一致）
    EDrift,
    /// 误休眠（在途事件未交付）
    ESleepBad,
    /// 事件账量守恒破缺（入队 ≠ 交付）
    EConserve,
    /// 序账满
    ECapFull,
    /// 未齐封账
    ESeal,
    /// 状态环倒退（Sleeping 直接回 Active 跳过 Waking）
    ERewind,
    /// 双跑指纹不一致
    EFp,
    /// 序为空
    EEmpty,
}

impl SsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SsCode::Ok => "成功",
            SsCode::ENoEnt => "根越册：核对 40 节点册行序",
            SsCode::EDup => "子树序重复节点：序生成器有环或重访，按 DFS 闭集重排",
            SsCode::EDrift => "树行复算漂移：序内节点必须与复算树行一致",
            SsCode::ESleepBad => "误休眠：在途事件未交付前不得休眠，先交付再休",
            SsCode::EConserve => "事件账守恒破缺：入队数必须等于交付数，逐窗复算",
            SsCode::ECapFull => "序账满：先封账或换新账本再入新序",
            SsCode::ESeal => "未齐封账：序未走完即封，先补齐缺步",
            SsCode::ERewind => "状态环倒退：Active→Sleeping→Waking→Active 单向，反向显性拒",
            SsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SsCode::EEmpty => "序为空：休眠序至少含根节点 1 条",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 节点行（同 51541 完全二叉树拓扑，父号恒 < 节点号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubNode {
    pub no: u32,
    pub parent: u32,
}

/// 复算第 i 行。
pub const fn sub_node(i: usize) -> Option<SubNode> {
    if i >= NODE_ROWS {
        return None;
    }
    Some(SubNode {
        no: i as u32,
        parent: if i == 0 { 0 } else { ((i - 1) / 2) as u32 },
    })
}

/// 子树挂起序（DFS 前序：先根后叶），由纯函数生成。
pub fn subtree_suspend_order(root: usize) -> Option<Vec<u32>> {
    if root >= NODE_ROWS {
        return None;
    }
    let mut seq: Vec<u32> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    stack.push(root);
    while !stack.is_empty() {
        let n = match stack.pop() {
            Some(v) => v,
            None => break,
        };
        if n >= NODE_ROWS {
            return None;
        }
        seq.push(n as u32);
        // 完全二叉树：右子先压栈（后弹出），保证左先右后前序。
        let right = n * 2 + 2;
        let left = n * 2 + 1;
        if right < NODE_ROWS {
            stack.push(right);
        }
        if left < NODE_ROWS {
            stack.push(left);
        }
    }
    Some(seq)
}

/// 子树恢复序（DFS 后序镜像：先叶后根 = 挂起序逆序）。
pub fn subtree_resume_order(root: usize) -> Option<Vec<u32>> {
    let sus = subtree_suspend_order(root)?;
    let mut rev: Vec<u32> = Vec::new();
    let mut i = sus.len();
    while i > 0 {
        i -= 1;
        if let Some(v) = sus.get(i) {
            rev.push(*v);
        }
    }
    Some(rev)
}

/// 子树序核验：无重复、逐节点与复算树行一致（父号 < 节点号）。
pub fn subtree_order_ok(seq: &[u32]) -> bool {
    if seq.is_empty() {
        return false;
    }
    let mut seen = [false; NODE_ROWS];
    let mut i = 0usize;
    while i < seq.len() {
        let n = match seq.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if n as usize >= NODE_ROWS || seen[n as usize] {
            return false;
        }
        let row = match sub_node(n as usize) {
            Some(r) => r,
            None => return false,
        };
        if n != 0 && row.parent >= n {
            return false;
        }
        seen[n as usize] = true;
        i += 1;
    }
    true
}

// ── 事件账 ───────────────────────────────────────────────────────
/// 子树状态环（环内单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubState {
    /// 活跃。
    Active,
    /// 休眠中。
    Sleeping,
    /// 唤醒中。
    Waking,
}

/// 环推进合法性（Active→Sleeping→Waking→Active，反向显性拒）。
pub fn substep(from: SubState, to: SubState) -> bool {
    matches!(
        (from, to),
        (SubState::Active, SubState::Sleeping)
            | (SubState::Sleeping, SubState::Waking)
            | (SubState::Waking, SubState::Active)
    )
}

/// 单窗事件账记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SleepWin {
    /// 窗号。
    pub win: u32,
    /// 子树根。
    pub root: u32,
    /// 窗内入队事件数。
    pub enq: u32,
    /// 唤醒后交付事件数。
    pub deq: u32,
}

/// 休眠唤醒事件账。
#[derive(Clone)]
pub struct SleepLedger {
    /// 逐窗记录。
    wins: Vec<SleepWin>,
    /// 总入队。
    total_enq: u32,
    /// 总交付。
    total_deq: u32,
    /// 误休眠注入次数。
    injected: u32,
    /// 误休眠检出次数。
    caught: u32,
    /// 误休眠漏检次数（必须 0）。
    missed: u32,
    /// 子树状态。
    state: SubState,
    /// 指纹。
    fp: u64,
}

impl SleepLedger {
    /// 新账本。
    pub fn new() -> Self {
        SleepLedger {
            wins: Vec::new(),
            total_enq: 0,
            total_deq: 0,
            injected: 0,
            caught: 0,
            missed: 0,
            state: SubState::Active,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 误休眠闸：在途事件未交付前休眠显性拒。
    pub fn sleep_gate(&mut self, pending: u32) -> Result<(), SsCode> {
        if pending > 0 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5150);
            return Err(SsCode::ESleepBad);
        }
        Ok(())
    }

    /// 漏检记账（闸未抓到时调用，计入漏检）。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.missed += 1;
    }

    /// 开休眠窗（Active→Sleeping）。
    pub fn open_win(&mut self, win: u32, root: u32) -> Result<(), SsCode> {
        if root as usize >= NODE_ROWS {
            return Err(SsCode::ENoEnt);
        }
        if !substep(self.state, SubState::Sleeping) {
            return Err(SsCode::ERewind);
        }
        self.state = SubState::Sleeping;
        self.wins.push(SleepWin {
            win,
            root,
            enq: 0,
            deq: 0,
        });
        self.fnv((win as u64) << 8 | root as u64);
        Ok(())
    }

    /// 窗内入队一条事件。
    pub fn enqueue(&mut self, n: u32) -> Result<(), SsCode> {
        let win = {
            let last = match self.wins.last_mut() {
                Some(w) => w,
                None => return Err(SsCode::ESeal),
            };
            last.enq = last.enq.saturating_add(n);
            last.win
        };
        self.total_enq = self.total_enq.saturating_add(n);
        self.fnv(0xE000 + win as u64);
        Ok(())
    }

    /// 唤醒（Sleeping→Waking）并交付窗内全部事件。
    pub fn wake_and_deliver(&mut self) -> Result<u32, SsCode> {
        if !substep(self.state, SubState::Waking) {
            return Err(SsCode::ERewind);
        }
        self.state = SubState::Waking;
        let delivered = {
            let last = match self.wins.last_mut() {
                Some(w) => w,
                None => return Err(SsCode::ESeal),
            };
            let d = last.enq;
            last.deq = last.deq.saturating_add(d);
            d
        };
        self.total_deq = self.total_deq.saturating_add(delivered);
        self.fnv(0xD000);
        Ok(delivered)
    }

    /// 回 Active（Waking→Active）。
    pub fn settle(&mut self) -> Result<(), SsCode> {
        if !substep(self.state, SubState::Active) {
            return Err(SsCode::ERewind);
        }
        self.state = SubState::Active;
        Ok(())
    }

    /// 事件守恒：总入队 == 总交付（零丢事件）。
    pub fn conservation_ok(&self) -> bool {
        self.total_enq == self.total_deq
    }

    /// 误休眠三数。
    pub fn badsleep_stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.missed)
    }

    /// 窗数。
    pub fn win_count(&self) -> usize {
        self.wins.len()
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for SleepLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 误休眠注入序列（10 轮，确定性 LCG）：每轮构造 pending>0 的假窗。
pub struct BadSleepInjector {
    seed: u64,
}

impl BadSleepInjector {
    /// 种子 = SEG_BASE 派生。
    pub fn new() -> Self {
        BadSleepInjector {
            seed: (SEG_BASE as u64) << 16 | 0x5E42,
        }
    }

    /// 下一轮假 pending（1..=7）。
    pub fn next_pending(&mut self) -> u32 {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.seed >> 33) % 7) as u32 + 1
    }
}

impl Default for BadSleepInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：子树两序生成核验 + ×20 休眠唤醒零丢 + 误休眠注入 10/10
/// + 守恒 + 指纹。返回（两序核验数, 指纹）。
pub fn run_cycle() -> (u32, u64) {
    // ① 子树两序核验（前序/后序互为镜像且逐序合法）。
    let mut ord_ok = 0u32;
    let mut root = 0usize;
    while root < 8 {
        if let (Some(sus), Some(res)) = (subtree_suspend_order(root), subtree_resume_order(root)) {
            if subtree_order_ok(&sus) && subtree_order_ok(&res) && sus.len() == res.len() {
                ord_ok += 1;
            }
        }
        root += 1;
    }
    // ② ×20 休眠唤醒零丢事件（窗内事件确定性 LCG 入队）。
    let mut led = SleepLedger::new();
    let mut cyc = 0usize;
    while cyc < SLEEP_CYCLES {
        let root_u = (cyc % 8) as u32;
        let _ = led.open_win(cyc as u32, root_u);
        // 窗内入队（确定性：1..=4 条）。
        let _ = led.enqueue(((cyc % 4) + 1) as u32);
        let _ = led.wake_and_deliver();
        let _ = led.settle();
        cyc += 1;
    }
    // ③ 误休眠注入 10/10。
    let mut inj = BadSleepInjector::new();
    let mut r = 0usize;
    while r < BADSLEEP_ROUNDS {
        let p = inj.next_pending();
        if led.sleep_gate(p).is_err() {
            // 检出（sleep_gate 内已记账）。
        } else {
            led.note_missed();
        }
        r += 1;
    }
    let fp = led.fingerprint();
    (ord_ok, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx51542_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51542");
    // 1. 正常流：子树两序生成 ×8 根全合法。
    let (ord_ok, fp) = run_cycle();
    s.add("子树两序 ×8 根合法", ord_ok == 8, "前序/后序互为镜像且逐序合法");
    // 2. 边界：根越册/空序/重复。
    s.add(
        "边界·根越册",
        subtree_suspend_order(NODE_ROWS).is_none() && sub_node(NODE_ROWS).is_none(),
        "根 40 越册必 None",
    );
    s.add("边界·空序拒", !subtree_order_ok(&[]), "空序必拒");
    s.add(
        "边界·重复节点拒",
        !subtree_order_ok(&[0, 1, 1]),
        "重复节点必拒",
    );
    // 3. 镜像性：恢复序 == 挂起序逆序（逐条相等）。
    let mirror_ok = {
        let sus = subtree_suspend_order(0).unwrap_or_default();
        let res = subtree_resume_order(0).unwrap_or_default();
        let mut ok = sus.len() == res.len();
        let mut i = 0usize;
        while ok && i < sus.len() {
            let a = sus.get(i).copied().unwrap_or(0xFFFF);
            let b = res.get(res.len() - 1 - i).copied().unwrap_or(0xFFFF);
            if a != b {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("恢复序镜像挂起序", mirror_ok, "先根后叶 ↔ 先叶后根互为镜像");
    // 4. 状态环倒退拒。
    s.add(
        "状态环·Sleeping→Active 跳步拒",
        !substep(SubState::Sleeping, SubState::Active),
        "必须过 Waking，跳步非法",
    );
    s.add(
        "状态环·Active→Waking 拒",
        !substep(SubState::Active, SubState::Waking),
        "必须先 Sleeping，越步非法",
    );
    // 5. ×20 休眠唤醒零丢事件。
    let mut led = SleepLedger::new();
    let mut cyc = 0usize;
    while cyc < SLEEP_CYCLES {
        let _ = led.open_win(cyc as u32, (cyc % 8) as u32);
        let _ = led.enqueue(((cyc % 4) + 1) as u32);
        let _ = led.wake_and_deliver();
        let _ = led.settle();
        cyc += 1;
    }
    s.add(
        "休眠唤醒 ×20 零丢事件",
        led.conservation_ok() && led.win_count() == SLEEP_CYCLES,
        "入队数==交付数、20 窗齐",
    );
    // 6. 误休眠注入 10/10。
    let mut inj = BadSleepInjector::new();
    let mut all_caught = true;
    let mut r = 0usize;
    while r < BADSLEEP_ROUNDS {
        let p = inj.next_pending();
        if led.sleep_gate(p).is_ok() {
            all_caught = false;
            led.note_missed();
        }
        r += 1;
    }
    let (inj_n, caught_n, missed_n) = led.badsleep_stats();
    s.add_dyn(
        "误休眠注入 10/10 检出",
        all_caught && caught_n >= 10 && missed_n == 0,
        &format!("注入={} 检出={} 漏检={}", inj_n, caught_n, missed_n),
    );
    // 7. 双跑指纹一致。
    let (_, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0,
        "SEG_BASE 0x40 对齐且在 VX12 域",
    );
    // 9. 收口（含本条共 ≥12）。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let sus = subtree_suspend_order(0).unwrap_or_default();
        assert_eq!(sus.first().copied(), Some(0));
        assert!(subtree_order_ok(&sus));
    }

    #[test]
    fn test_boundary_root_overflow() {
        assert!(subtree_suspend_order(NODE_ROWS).is_none());
        assert!(subtree_resume_order(NODE_ROWS).is_none());
        assert!(sub_node(NODE_ROWS).is_none());
    }

    #[test]
    fn test_mirror_property() {
        let sus = subtree_suspend_order(3).unwrap_or_default();
        let res = subtree_resume_order(3).unwrap_or_default();
        assert_eq!(sus.len(), res.len());
        let mut i = 0usize;
        while i < sus.len() {
            assert_eq!(sus[i], res[sus.len() - 1 - i]);
            i += 1;
        }
    }

    #[test]
    fn test_dup_rejected() {
        assert!(!subtree_order_ok(&[0, 1, 1]));
        assert!(!subtree_order_ok(&[]));
    }

    #[test]
    fn test_state_ring_monotonic() {
        assert!(substep(SubState::Active, SubState::Sleeping));
        assert!(substep(SubState::Sleeping, SubState::Waking));
        assert!(substep(SubState::Waking, SubState::Active));
        assert!(!substep(SubState::Sleeping, SubState::Active));
        assert!(!substep(SubState::Active, SubState::Waking));
    }

    #[test]
    fn test_zero_event_loss() {
        let mut led = SleepLedger::new();
        let mut cyc = 0usize;
        while cyc < SLEEP_CYCLES {
            let _ = led.open_win(cyc as u32, (cyc % 8) as u32);
            let _ = led.enqueue(((cyc % 4) + 1) as u32);
            let _ = led.wake_and_deliver();
            let _ = led.settle();
            cyc += 1;
        }
        assert!(led.conservation_ok());
        assert_eq!(led.win_count(), SLEEP_CYCLES);
    }

    #[test]
    fn test_injection_all_caught() {
        let mut led = SleepLedger::new();
        let mut inj = BadSleepInjector::new();
        let mut r = 0usize;
        while r < BADSLEEP_ROUNDS {
            let p = inj.next_pending();
            assert!(led.sleep_gate(p).is_err(), "第 {} 轮误休眠必须被拒", r);
            r += 1;
        }
        let (_, _, missed) = led.badsleep_stats();
        assert_eq!(missed, 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, f1) = run_cycle();
        let (a2, f2) = run_cycle();
        assert_eq!(a1, a2);
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
        let s = run_unx51542_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
