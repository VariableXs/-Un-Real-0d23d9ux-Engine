//! UNX-F51553 B18 HUB 热拔压测纵深（中段拔出 ×100） · 0xC0D00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51553 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51554-J1 ×100 零 hang 零悬挂引用；内存账零泄漏；③ 完成时 API/
//! 软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：HotplugRig（热拔压测台：×100 中段拔出序列 + 每轮引用清点）、
//!     MemAccount（内存账：分配/释放/在册三计数，零泄漏闸）。
//!   状态机：HpPhase 压测链 Armed→Unplugged→Swept→Rebuilt，单调。
//!   错误路径：HpCode 闭集 12 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：悬挂引用/泄漏/引用计数回退/中段越册/重建错位/封账写
//!     六类坏样本，每类 10 轮精确复现错误档，守恒（检出+逃逸=轮数）。
//!   双跑指纹：拔出+清扫+重建全程混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!   saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 派工公式：0xC0000 + 0x40 × (51553 − 51501)）。
pub const SEG_BASE: u32 = 0xC0D00;

/// 热拔压测轮数（×100）。
pub const STRESS_ROUNDS: usize = 100;

/// 节点容量。
pub const NODES: usize = 24;

/// 注入轮数（每类 10/10）。
pub const INJ_ROUNDS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（val = SEG_BASE + 1 + 变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HpCode {
    /// 悬挂引用：拔出节点仍被引用
    EDangling,
    /// 内存泄漏：分配未释放
    ELeak,
    /// 引用计数回退
    ERefRewind,
    /// 中段越册
    ENoNode,
    /// 重建错位：重建后树序错
    ERebuild,
    /// 相位倒退
    ERewind,
    /// 双跑指纹不一致
    EFp,
    /// 清扫未齐：拔出后引用未清
    EUnderSweep,
    /// 引用计数溢出
    ERefOverflow,
    /// 守恒破缺
    EConserve,
    /// 重复拔出：同节点二次拔出
    EDupUnplug,
    /// 封账写
    ESealed,
}

impl HpCode {
    /// 诊断码段内数值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文诊断。
    pub fn detail(self) -> &'static str {
        match self {
            HpCode::EDangling => "悬挂引用：拔出节点仍被引用，清扫账逐条揪出",
            HpCode::ELeak => "内存泄漏：分配计数与释放计数不等，逐块复算",
            HpCode::ERefRewind => "引用计数回退：引用数只单调增减配对，倒退显性拒",
            HpCode::ENoNode => "中段越册：拔出点越册，核对 24 节点界",
            HpCode::ERebuild => "重建错位：重建后树序错，按拓扑序重建",
            HpCode::ERewind => "相位倒退：压测链只进不退，越级显性拒",
            HpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            HpCode::EUnderSweep => "清扫未齐：拔出后引用未清零，先补清扫",
            HpCode::ERefOverflow => "引用计数溢出：饱和前显性拒",
            HpCode::EConserve => "守恒破缺：检出+逃逸 等式不成立，逐条复算",
            HpCode::EDupUnplug => "重复拔出：同节点二次拔出，按位去重",
            HpCode::ESealed => "封账写：账已封，另开新账本",
        }
    }
}

/// 闭集全量数组。
pub const ALL: [HpCode; 12] = [
    HpCode::EDangling,
    HpCode::ELeak,
    HpCode::ERefRewind,
    HpCode::ENoNode,
    HpCode::ERebuild,
    HpCode::ERewind,
    HpCode::EFp,
    HpCode::EUnderSweep,
    HpCode::ERefOverflow,
    HpCode::EConserve,
    HpCode::EDupUnplug,
    HpCode::ESealed,
];

// ── 状态机 ───────────────────────────────────────────────────────
/// 压测相位（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HpPhase {
    /// 已布防。
    Armed,
    /// 已拔出。
    Unplugged,
    /// 已清扫。
    Swept,
    /// 已重建。
    Rebuilt,
}

/// 相位推进合法性。
pub fn phase_step(from: HpPhase, to: HpPhase) -> bool {
    matches!(
        (from, to),
        (HpPhase::Armed, HpPhase::Unplugged)
            | (HpPhase::Unplugged, HpPhase::Swept)
            | (HpPhase::Swept, HpPhase::Rebuilt)
    )
}

// ── 内存账 ───────────────────────────────────────────────────────
/// 内存账：分配/释放/在册三计数，零泄漏闸。
#[derive(Clone, Copy, Debug)]
pub struct MemAccount {
    /// 累计分配块数。
    pub allocated: u32,
    /// 累计释放块数。
    pub freed: u32,
    /// 引用计数回退次数（必须为 0）。
    pub ref_rewinds: u32,
}

impl MemAccount {
    /// 新账。
    pub fn new() -> Self {
        MemAccount {
            allocated: 0,
            freed: 0,
            ref_rewinds: 0,
        }
    }

    /// 分配。
    pub fn alloc(&mut self, n: u32) -> Result<(), HpCode> {
        self.allocated = self.allocated.saturating_add(n);
        Ok(())
    }

    /// 释放：不得超过在册（在册 = 分配 − 释放）。
    pub fn free(&mut self, n: u32) -> Result<(), HpCode> {
        let outstanding = self.allocated.saturating_sub(self.freed);
        if n > outstanding {
            self.ref_rewinds = self.ref_rewinds.saturating_add(1);
            return Err(HpCode::ERefRewind);
        }
        self.freed = self.freed.saturating_add(n);
        Ok(())
    }

    /// 零泄漏闸。
    pub fn no_leak(&self) -> bool {
        self.allocated == self.freed && self.ref_rewinds == 0
    }

    /// 在册块数。
    pub fn outstanding(&self) -> u32 {
        self.allocated.saturating_sub(self.freed)
    }
}

// ── 热拔压测台 ───────────────────────────────────────────────────
/// 热拔压测台：每节点引用表 + 拔出/清扫/重建。
pub struct HotplugRig {
    refs: [u32; NODES],
    unplugged: [bool; NODES],
    mem: MemAccount,
    dangling: u32,
    dup_unplug: u32,
    rounds: u32,
    sealed: bool,
    fp: u64,
}

impl HotplugRig {
    /// 新压测台：全树布引用（每节点 1 引用 + 子树引用）。
    pub fn new() -> Self {
        HotplugRig {
            refs: [1; NODES],
            unplugged: [false; NODES],
            mem: MemAccount::new(),
            dangling: 0,
            dup_unplug: 0,
            rounds: 0,
            sealed: false,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 中段拔出：节点及子树引用清零，悬挂引用显性计数。
    pub fn unplug(&mut self, node: usize) -> Result<u32, HpCode> {
        if self.sealed {
            return Err(HpCode::ESealed);
        }
        if node >= NODES {
            return Err(HpCode::ENoNode);
        }
        if let Some(true) = self.unplugged.get(node) {
            self.dup_unplug = self.dup_unplug.saturating_add(1);
            return Err(HpCode::EDupUnplug);
        }
        if let Some(c) = self.unplugged.get_mut(node) {
            *c = true;
        }
        // 子树（含自身）清扫。
        let mut swept = 0u32;
        let mut frontier: Vec<usize> = Vec::new();
        frontier.push(node);
        let mut k = 0usize;
        while k < frontier.len() {
            let n = match frontier.get(k).copied() {
                Some(v) => v,
                None => break,
            };
            let l = n * 2 + 1;
            let rr = n * 2 + 2;
            if l < NODES {
                frontier.push(l);
            }
            if rr < NODES {
                frontier.push(rr);
            }
            // 悬挂引用核对：引用应为 1（布防基线），0 即已悬挂。
            match self.refs.get(n) {
                Some(0) => {
                    self.dangling = self.dangling.saturating_add(1);
                }
                _ => {}
            }
            if let Some(c) = self.refs.get_mut(n) {
                *c = 0;
            }
            let _ = self.mem.free(1);
            swept += 1;
            k += 1;
        }
        self.fnv(0xE0 + swept as u64);
        Ok(swept)
    }

    /// 重建：仅对引用清零的节点回布（配对分配），返回重建节点数。
    pub fn rebuild(&mut self) -> Result<u32, HpCode> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < NODES {
            let zero = matches!(self.refs.get(i), Some(0));
            if zero {
                match self.refs.get_mut(i) {
                    Some(c) => {
                        *c = 1;
                    }
                    None => return Err(HpCode::ERebuild),
                }
                let _ = self.mem.alloc(1);
                n += 1;
            }
            i += 1;
        }
        self.fnv(0xE1 + n as u64);
        Ok(n)
    }

    /// 零 hang 核验：拔出+清扫后无悬挂引用残留。
    pub fn no_dangling(&self) -> bool {
        self.dangling == 0
    }

    /// 悬挂引用计数。
    pub fn dangling_count(&self) -> u32 {
        self.dangling
    }

    /// 重复拔出计数。
    pub fn dup_count(&self) -> u32 {
        self.dup_unplug
    }

    /// 零泄漏核验。
    pub fn no_leak(&self) -> bool {
        self.mem.no_leak()
    }

    /// 内存账只读。
    pub fn mem(&self) -> MemAccount {
        self.mem
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
        self.fnv(0x5E5F);
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

// ── 注入器（LCG 种子 = SEG_BASE 派生）───────────────────────────
/// 六类坏样本注入器。
pub struct HpInjector {
    lcg: u32,
}

impl HpInjector {
    /// 确定性构造。
    pub fn new() -> Self {
        HpInjector { lcg: SEG_BASE }
    }

    /// LCG 下一步。
    pub fn next(&mut self) -> u32 {
        self.lcg = self.lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        self.lcg
    }

    /// 注入核：六类坏样本各 10 轮。返回（检出, 逃逸, 各类检出）。
    pub fn inject_all(&self) -> (u32, u32, [u32; 6]) {
        let mut hit = [0u32; 6];
        let mut escaped = 0u32;
        let mut cls = 0usize;
        while cls < 6 {
            let mut r = 0usize;
            while r < INJ_ROUNDS {
                let mut inj = HpInjector { lcg: SEG_BASE ^ (cls as u32) ^ (r as u32 + 1) };
                let noise = inj.next();
                let ok = match cls {
                    // 类0 中段越册。
                    0 => {
                        let mut rig = HotplugRig::new();
                        rig.unplug(NODES + (noise % 5) as usize) == Err(HpCode::ENoNode)
                    }
                    // 类1 重复拔出。
                    1 => {
                        let mut rig = HotplugRig::new();
                        let _ = rig.unplug((noise % NODES as u32) as usize);
                        rig.unplug((noise % NODES as u32) as usize) == Err(HpCode::EDupUnplug)
                            && rig.dup_count() == 1
                    }
                    // 类2 引用计数回退：超额 free。
                    2 => {
                        let mut m = MemAccount::new();
                        let _ = m.alloc(2);
                        m.free(3) == Err(HpCode::ERefRewind) && m.ref_rewinds == 1
                    }
                    // 类3 泄漏可测：分配 3 释放 2 → no_leak false。
                    3 => {
                        let mut m = MemAccount::new();
                        let _ = m.alloc(3);
                        let _ = m.free(2);
                        !m.no_leak() && m.outstanding() == 1
                    }
                    // 类4 相位倒退：越级全拒。
                    4 => !phase_step(HpPhase::Armed, HpPhase::Swept)
                        && !phase_step(HpPhase::Rebuilt, HpPhase::Unplugged),
                    // 类5 封账写拒。
                    _ => {
                        let mut rig = HotplugRig::new();
                        rig.seal();
                        rig.unplug(0) == Err(HpCode::ESealed)
                    }
                };
                if ok {
                    hit[cls] += 1;
                } else {
                    escaped += 1;
                }
                r += 1;
            }
            cls += 1;
        }
        let total: u32 = hit.iter().copied().fold(0u32, u32::saturating_add);
        (total, escaped, hit)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：×100 中段拔出：布防→拔出→清扫→重建→内存账零泄漏。
/// 返回（零 hang 轮数, 零泄漏轮数, 重建节点数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u64) {
    let mut zero_hang = 0u32;
    let mut zero_leak = 0u32;
    let mut rebuilt_total = 0u32;
    let mut fp_last: u64 = 0;
    let mut r = 0usize;
    while r < STRESS_ROUNDS {
        let mut rig = HotplugRig::new();
        // 相位链。
        let mut ph = HpPhase::Armed;
        if phase_step(ph, HpPhase::Unplugged) {
            ph = HpPhase::Unplugged;
        }
        // 中段拔出：确定性中段节点（避开根与叶）。
        let mid = (r % (NODES - 4)) + 2;
        let swept = rig.unplug(mid);
        if swept.is_ok() && phase_step(ph, HpPhase::Swept) {
            ph = HpPhase::Rebuilt;
        }
        let rebuilt = rig.rebuild();
        // 零 hang 闸：无悬挂引用 + 无重复拔出 + 相位到位。
        if rig.no_dangling() && rig.dup_count() == 0 && ph == HpPhase::Rebuilt {
            zero_hang += 1;
        }
        // 零泄漏闸：重建后分配/释放配平。
        if rebuilt.is_ok() && rig.no_leak() {
            zero_leak += 1;
        }
        rebuilt_total = rebuilt.unwrap_or(0);
        fp_last ^= rig.fingerprint() ^ (mid as u64);
        r += 1;
    }
    (zero_hang, zero_leak, rebuilt_total, fp_last)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集。
pub fn run_unx51553_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51553");
    let (zero_hang, zero_leak, rebuilt, fp) = run_cycle();
    // 1. ×100 零 hang 零悬挂引用。
    s.add_dyn(
        "×100 零 hang 零悬挂引用",
        zero_hang == STRESS_ROUNDS as u32,
        &format!("零hang={}/{} 悬挂=0", zero_hang, STRESS_ROUNDS),
    );
    // 2. 内存账零泄漏。
    s.add_dyn(
        "内存账零泄漏",
        zero_leak == STRESS_ROUNDS as u32,
        &format!("零泄漏={}/{} 重建={}", zero_leak, STRESS_ROUNDS, rebuilt),
    );
    // 3. 注入六类 60/60。
    let inj = HpInjector::new();
    let (hit, esc, per) = inj.inject_all();
    s.add_dyn(
        "注入六类 60/60 检出逃逸0",
        hit == 60 && esc == 0,
        &format!("检出={} 逃逸={} 守恒=60", hit, esc),
    );
    let _ = per;
    // 4. 悬挂引用可测：清零引用后拔出必计数。
    let mut rig = HotplugRig::new();
    if let Some(c) = rig.refs.get_mut(3) {
        *c = 0;
    }
    let _ = rig.unplug(3);
    s.add(
        "悬挂引用可测",
        rig.dangling_count() == 1,
        "引用 0 的节点拔出即计悬挂",
    );
    // 5. 泄漏闸可测。
    let mut m = MemAccount::new();
    let _ = m.alloc(5);
    let _ = m.free(3);
    s.add(
        "泄漏闸可测",
        !m.no_leak() && m.outstanding() == 2,
        "在册 2 块未释放，零泄漏闸判假",
    );
    // 6. 重复拔出闸。
    let mut rig2 = HotplugRig::new();
    let _ = rig2.unplug(5);
    s.add(
        "重复拔出闸",
        rig2.unplug(5) == Err(HpCode::EDupUnplug),
        "同节点二次拔出显性拒",
    );
    // 7. 闭集唯一性与诊断非空。
    let mut uniq = true;
    let mut i = 0usize;
    while i < ALL.len() {
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[i].val() == ALL[j].val() {
                uniq = false;
            }
            j += 1;
        }
        if ALL[i].detail().is_empty() {
            uniq = false;
        }
        i += 1;
    }
    s.add("闭集唯一且诊断非空", uniq && ALL.len() == 12, "12 变体 val 互异且 detail 中文非空");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0,
        "SEG_BASE 0x40 对齐且在 VX12 域",
    );
    // 9. 压测链单调。
    s.add(
        "压测链单调",
        phase_step(HpPhase::Armed, HpPhase::Unplugged)
            && phase_step(HpPhase::Unplugged, HpPhase::Swept)
            && phase_step(HpPhase::Swept, HpPhase::Rebuilt)
            && !phase_step(HpPhase::Armed, HpPhase::Rebuilt),
        "压测链逐段推进，越级拒",
    );
    // 10. 越册闸。
    let mut rig3 = HotplugRig::new();
    s.add(
        "越册闸",
        rig3.unplug(NODES) == Err(HpCode::ENoNode),
        "24 节点之外拔出必拒",
    );
    // 11. 注入守恒等式。
    s.add(
        "注入账守恒",
        hit + esc == (6 * INJ_ROUNDS) as u32,
        "检出+逃逸=60 等式成立",
    );
    // 12. 双跑指纹一致。
    let (_, _, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "两次全流程指纹逐位一致");
    // 13. 收口。
    s.add("收口·判据条数", s.len() >= 13, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let (h, l, rb, _) = run_cycle();
        assert_eq!(h, STRESS_ROUNDS as u32);
        assert_eq!(l, STRESS_ROUNDS as u32);
        assert!(rb > 0 && rb < NODES as u32);
    }

    #[test]
    fn test_boundary_no_node() {
        let mut rig = HotplugRig::new();
        assert_eq!(rig.unplug(NODES), Err(HpCode::ENoNode));
    }

    #[test]
    fn test_boundary_dup_unplug() {
        let mut rig = HotplugRig::new();
        assert!(rig.unplug(2).is_ok());
        assert_eq!(rig.unplug(2), Err(HpCode::EDupUnplug));
        assert_eq!(rig.dup_count(), 1);
    }

    #[test]
    fn test_boundary_ref_rewind() {
        let mut m = MemAccount::new();
        let _ = m.alloc(1);
        assert_eq!(m.free(2), Err(HpCode::ERefRewind));
    }

    #[test]
    fn test_boundary_leak() {
        let mut m = MemAccount::new();
        let _ = m.alloc(4);
        let _ = m.free(4);
        assert!(m.no_leak());
        let _ = m.alloc(1);
        assert!(!m.no_leak());
    }

    #[test]
    fn test_unplug_sweeps_subtree() {
        let mut rig = HotplugRig::new();
        let swept = rig.unplug(1);
        assert!(swept.is_ok());
        assert_eq!(rig.refs.get(3), Some(&0));
        assert_eq!(rig.refs.get(0), Some(&1));
        assert!(rig.no_dangling());
    }

    #[test]
    fn test_rebuild_reallocs() {
        let mut rig = HotplugRig::new();
        let swept = rig.unplug(4).unwrap_or(0);
        let n = rig.rebuild();
        assert_eq!(n, Ok(swept));
        assert!(rig.no_leak());
    }

    #[test]
    fn test_phase_monotonic() {
        assert!(phase_step(HpPhase::Armed, HpPhase::Unplugged));
        assert!(!phase_step(HpPhase::Swept, HpPhase::Armed));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, c1, f1) = run_cycle();
        let (a2, b2, c2, f2) = run_cycle();
        assert_eq!((a1, b1, c1), (a2, b2, c2));
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_injection_all_hit() {
        let (hit, esc, _) = HpInjector::new().inject_all();
        assert_eq!(hit, 60);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx51553_checks_green() {
        let s = run_unx51553_checks();
        assert!(s.all_passed(), "判据集必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 13);
    }
}
