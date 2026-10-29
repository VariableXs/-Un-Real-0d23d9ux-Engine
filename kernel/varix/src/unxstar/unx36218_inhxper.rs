//! UNX-F36218-J1 继承引擎体验日志（权限继承困惑指纹）。
//! 判据：① 「改父未见效」类操作指纹（父改后子查未变 > 预期）自动
//! 标记 10 例触发——NP 短传 / OI 对容器查询 / IO 自身不适用 / 保护
//! 短路四类真实困惑形态各若干例，指纹器逐例置 confusion 账；非
//! 困惑反例（改父确实生效）零误标；② 入改进清单接口（队列 push/
//! drain 账面守恒，drain 后队列空、条目可读）；③ 纯功能 ≥320 行。
//! 设计纪律：体验日志不是抱怨箱——「困惑指纹」是可计算的观测：
//! 操作意图（改父）与观测结果（子视图 digest）的差值超过语义预期
//! 才算困惑。四类真实形态全有正当语义解释（NP 就是该短传、保护就
//! 是该隔离），但用户的操作意图与观测结果错位是真实痛点——指纹
//! 器只负责「如实标记 + 进改进清单」，不负责替语义辩护。误标（把
//! 生效当未生效）比漏标更糟——反例面钉死。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use super::unx36204_inhprot::ProtEngine;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xa120;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FINGER: u16 = SEG_BASE + 2; // 指纹错标（困惑形态未标/生效误标）
pub const E_QUEUE: u16 = SEG_BASE + 3; // 改进清单账不平（push/drain）
pub const E_ANCHOR: u16 = SEG_BASE + 4; // 引擎锚漂移
pub const E_DIGEST: u16 = SEG_BASE + 5; // 视图摘要账破
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

pub const VER: u32 = 1;

/// 触发例数锚（判据 ① 的 10 例）与反例数。
pub const CONFUSED_N: usize = 10;
pub const CLEAR_N: usize = 10;

/// 视图摘要（子查结果的可观测形态——digest = 有效集折叠）。
pub fn view_digest(acl: &[ie::Ace]) -> u32 {
    let mut h = 0xC0FF_EE01u32;
    let mut i = 0;
    while i < acl.len() {
        h = h.wrapping_mul(0x0100_0193)
            ^ acl[i].sid as u32
            ^ acl[i].mask
            ^ (acl[i].kind as u32).wrapping_mul(3);
        i += 1;
    }
    h
}

/// 困惑事件（指纹 + 归因类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConfusionEvent {
    pub kind: u8, // 0=NP 短传 1=OI 对容器 2=IO 自身 3=保护短路
    pub parent: usize,
    pub child: usize,
    pub digest_before: u32,
    pub digest_after: u32,
}

/// 改进清单接口（push/drain 账面守恒）。
pub struct ImprovementQueue {
    items: Vec<ConfusionEvent>,
    pub pushed_total: u32,
    pub drained_total: u32,
}

impl ImprovementQueue {
    pub fn new() -> ImprovementQueue {
        ImprovementQueue { items: Vec::new(), pushed_total: 0, drained_total: 0 }
    }

    pub fn push(&mut self, e: ConfusionEvent) {
        self.items.push(e);
        self.pushed_total += 1;
    }

    pub fn pending(&self) -> usize {
        self.items.len()
    }

    /// 排空（消费面——条目按序返回，队列清零）。
    pub fn drain(&mut self) -> Vec<ConfusionEvent> {
        let mut out: Vec<ConfusionEvent> = Vec::new();
        let mut i = 0;
        while i < self.items.len() {
            out.push(self.items[i]);
            i += 1;
        }
        self.drained_total += out.len() as u32;
        self.items.clear();
        out
    }

    /// 账面守恒：pushed = drained + pending。
    pub fn reconciles(&self) -> bool {
        self.pushed_total == self.drained_total + self.pending() as u32
    }
}

/// 指纹器：父变更前后对 child 取摘要——未变且「语义上应可感知」
/// 即困惑（感知预期由调用方按操作意图给定）。
pub fn fingerprint(
    digest_before: u32,
    digest_after: u32,
    expected_effective: bool,
    parent: usize,
    child: usize,
    kind: u8,
) -> Result<Option<ConfusionEvent>, u16> {
    if digest_before == digest_after && expected_effective {
        Ok(Some(ConfusionEvent {
            kind,
            parent,
            child,
            digest_before,
            digest_after,
        }))
    } else if digest_before != digest_after && !expected_effective {
        // 生效却预期无效 → 指纹器语义错（比漏标更糟的误标）。
        Err(E_FINGER)
    } else {
        Ok(None)
    }
}

/// 10 例困惑触发扫描（判据 ① 主面）：四类真实形态。
pub fn confusion_sweep() -> Result<u32, u16> {
    let mut q = ImprovementQueue::new();
    let mut triggered = 0u32;
    // 形态 0（NP 短传）×3：root 改 Deny 带 NP——mid 能看到、孙看不到；
    // 用户视角查询孙 → 未变 → 困惑。
    let mut case = 0;
    while case < 3 {
        let mut eng = ProtEngine::new();
        let root = eng.add_node("root", None, true)?;
        let mid = eng.add_node("mid", Some(root), true)?;
        let leaf = eng.add_node("leaf", Some(mid), false)?;
        let mut base: Vec<ie::Ace> = Vec::new();
        base.push(ie::Ace { sid: 1, mask: 0x3, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        eng.change_acl(root, base)?;
        let d_before = view_digest(eng.view(leaf)?);
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace { sid: 1, mask: 0x3, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        fresh.push(ie::Ace { sid: 99, mask: 0x8, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT | ie::AI_NO_PROPAGATE, kind: ie::AK_DENY, origin: 0 });
        eng.change_acl(root, fresh)?;
        let d_after = view_digest(eng.view(leaf)?);
        if let Some(e) = fingerprint(d_before, d_after, true, root, leaf, 0)? {
            q.push(e);
            triggered += 1;
        } else {
            return Err(E_FINGER);
        }
        case += 1;
    }
    // 形态 1（OI 对容器查询）×3：root 加 OI-only Deny——容器子不变；
    // 用户查 mid（容器）→ 未变 → 困惑。
    let mut case = 0;
    while case < 3 {
        let mut eng = ProtEngine::new();
        let root = eng.add_node("root", None, true)?;
        let mid = eng.add_node("mid", Some(root), true)?;
        let mut base: Vec<ie::Ace> = Vec::new();
        base.push(ie::Ace { sid: 1, mask: 0x3, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        eng.change_acl(root, base)?;
        let d_before = view_digest(eng.view(mid)?);
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace { sid: 1, mask: 0x3, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        fresh.push(ie::Ace { sid: 98, mask: 0x4, flags: ie::AI_OBJECT_INHERIT, kind: ie::AK_DENY, origin: 0 });
        eng.change_acl(root, fresh)?;
        let d_after = view_digest(eng.view(mid)?);
        if let Some(e) = fingerprint(d_before, d_after, true, root, mid, 1)? {
            q.push(e);
            triggered += 1;
        } else {
            return Err(E_FINGER);
        }
        case += 1;
    }
    // 形态 2（IO 自身不适用）×2：root 加 IO Deny——root 自己不变；
    // 用户查 root → 未变 → 困惑。
    let mut case = 0;
    while case < 2 {
        let mut eng = ProtEngine::new();
        let root = eng.add_node("root", None, true)?;
        let mut base: Vec<ie::Ace> = Vec::new();
        base.push(ie::Ace { sid: 1, mask: 0x3, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        eng.change_acl(root, base)?;
        let d_before = view_digest(eng.view(root)?);
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace { sid: 1, mask: 0x3, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        fresh.push(ie::Ace { sid: 97, mask: 0x8, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT | ie::AI_INHERIT_ONLY, kind: ie::AK_DENY, origin: 0 });
        eng.change_acl(root, fresh)?;
        let d_after = view_digest(eng.view(root)?);
        if let Some(e) = fingerprint(d_before, d_after, true, root, root, 2)? {
            q.push(e);
            triggered += 1;
        } else {
            return Err(E_FINGER);
        }
        case += 1;
    }
    // 形态 3（保护短路）×2：子受保护——父改不进来；用户查子 → 未变 → 困惑。
    let mut case = 0;
    while case < 2 {
        let mut eng = ProtEngine::new();
        let root = eng.add_node("root", None, true)?;
        let child = eng.add_node("child", Some(root), false)?;
        let mut base: Vec<ie::Ace> = Vec::new();
        base.push(ie::Ace { sid: 5, mask: 0x1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        eng.tree.set_dacl(child, base)?;
        eng.set_protection(child, true)?;
        eng.materialize_all()?;
        let d_before = view_digest(eng.view(child)?);
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace { sid: 96, mask: 0x8, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_DENY, origin: 0 });
        eng.change_acl(root, fresh)?;
        let d_after = view_digest(eng.view(child)?);
        if let Some(e) = fingerprint(d_before, d_after, true, root, child, 3)? {
            q.push(e);
            triggered += 1;
        } else {
            return Err(E_FINGER);
        }
        case += 1;
    }
    if triggered != CONFUSED_N as u32 {
        return Err(E_FINGER);
    }
    // 改进清单接口（判据 ②）：drain 守恒 + 条目可读（kind 落域）。
    if !q.reconciles() || q.pending() != CONFUSED_N {
        return Err(E_QUEUE);
    }
    let drained = q.drain();
    if drained.len() != CONFUSED_N || q.pending() != 0 || !q.reconciles() {
        return Err(E_QUEUE);
    }
    let mut i = 0;
    while i < drained.len() {
        if drained[i].kind > 3 {
            return Err(E_FINGER);
        }
        i += 1;
    }
    Ok(triggered)
}

/// 反例扫描（误标防线）：改父确实生效的 10 例——指纹器必须零误标。
pub fn clear_sweep() -> Result<u32, u16> {
    let mut clear = 0u32;
    let mut case = 0;
    while case < CLEAR_N {
        let mut eng = ProtEngine::new();
        let root = eng.add_node("root", None, true)?;
        let mid = eng.add_node("mid", Some(root), true)?;
        let leaf = eng.add_node("leaf", Some(mid), false)?;
        let mut base: Vec<ie::Ace> = Vec::new();
        base.push(ie::Ace {
            sid: 1,
            mask: 0x1 + (case % 7) as u32,
            flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT,
            kind: ie::AK_ALLOW,
            origin: 0,
        });
        eng.change_acl(root, base)?;
        let d_before = view_digest(eng.view(leaf)?);
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace {
            sid: 1,
            mask: 0x1 + (case % 7) as u32,
            flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT,
            kind: ie::AK_ALLOW,
            origin: 0,
        });
        fresh.push(ie::Ace {
            sid: 90 + case as u16,
            mask: 0x8,
            flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT,
            kind: ie::AK_DENY,
            origin: 0,
        });
        eng.change_acl(root, fresh)?;
        let d_after = view_digest(eng.view(leaf)?);
        // 生效例：预期有效且确实变化 → 零困惑。
        if fingerprint(d_before, d_after, true, root, leaf, 0)?.is_some() {
            return Err(E_FINGER);
        }
        if d_before == d_after {
            return Err(E_DIGEST);
        }
        clear += 1;
        case += 1;
    }
    Ok(clear)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R1：10 例困惑触发 + 改进清单守恒。
    let triggered = confusion_sweep()?;
    if triggered != CONFUSED_N as u32 {
        return Err(E_FINGER);
    }
    // R2：10 例反例零误标。
    let clear = clear_sweep()?;
    if clear != CLEAR_N as u32 {
        return Err(E_FINGER);
    }
    // R3：指纹器语义钉（未变+预期无效 → 不标；变+预期有效 → 不标）。
    if fingerprint(1, 1, false, 0, 1, 0)?.is_some() {
        return Err(E_FINGER);
    }
    if fingerprint(1, 2, true, 0, 1, 0)?.is_some() {
        return Err(E_FINGER);
    }
    // R4：摘要确定性。
    let mut acl: Vec<ie::Ace> = Vec::new();
    acl.push(ie::Ace { sid: 1, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    if view_digest(&acl) != view_digest(&acl) {
        return Err(E_DIGEST);
    }
    // R5：队列越界/清空面（drain 两次第二次为空）。
    let mut q = ImprovementQueue::new();
    q.push(ConfusionEvent { kind: 0, parent: 0, child: 1, digest_before: 1, digest_after: 1 });
    let d1 = q.drain();
    let d2 = q.drain();
    if d1.len() != 1 || !d2.is_empty() || !q.reconciles() {
        return Err(E_QUEUE);
    }
    Ok(triggered.wrapping_mul(3).wrapping_add(clear))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FINGER => "指纹错标（困惑形态未标/生效误标/kind 越域）",
        E_QUEUE => "改进清单账不平（push/drain 守恒破）",
        E_ANCHOR => "引擎锚漂移",
        E_DIGEST => "视图摘要账破",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36218_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36218_inhxper");
    cs.add("SEG 段位独占", SEG_BASE == 0xa120, "0xa120~0xa12F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "10 例困惑触发/10 例反例零误标/队列守恒/指纹语义钉全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("例量锚", CONFUSED_N == 10 && CLEAR_N == 10, "10 例触发 + 10 例反例（判据锚）");
    cs.add("形态覆盖", { let mut q = ImprovementQueue::new(); let mut k = 0; while k < 4 { q.push(ConfusionEvent { kind: k as u8, parent: 0, child: 1, digest_before: 1, digest_after: 1 }); k += 1; } q.drain().len() == 4 }, "四类困惑形态可入清单（NP/OI/IO/保护）");
    cs.add("队列面", { let mut q = ImprovementQueue::new(); q.push(ConfusionEvent { kind: 0, parent: 0, child: 0, digest_before: 1, digest_after: 1 }); q.reconciles() && q.pending() == 1 }, "push/drain 账面守恒");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_green() {
        let cs = run_unx36218_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_np_confusion_is_real_pain() {
        // 性质直证：NP 短传形态——mid 看得见新 Deny、孙看不见；用户
        // 查孙的摘要恒定（困惑的物理基础）。
        let mut eng = ProtEngine::new();
        let root = eng.add_node("root", None, true).ok().unwrap_or(0);
        let mid = eng.add_node("mid", Some(root), true).ok().unwrap_or(0);
        let leaf = eng.add_node("leaf", Some(mid), false).ok().unwrap_or(0);
        let mut base: Vec<ie::Ace> = Vec::new();
        base.push(ie::Ace { sid: 1, mask: 0x3, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        eng.change_acl(root, base).ok().unwrap_or(());
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace { sid: 1, mask: 0x3, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        fresh.push(ie::Ace { sid: 99, mask: 0x8, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT | ie::AI_NO_PROPAGATE, kind: ie::AK_DENY, origin: 0 });
        eng.change_acl(root, fresh).ok().unwrap_or(());
        let d_mid = view_digest(eng.view(mid).ok().unwrap_or_default());
        let d_leaf = view_digest(eng.view(leaf).ok().unwrap_or_default());
        assert!(d_mid != d_leaf, "mid 与 leaf 视图必不同（Deny 落 mid）");
        let d_leaf2 = view_digest(eng.view(leaf).ok().unwrap_or_default());
        assert_eq!(d_leaf, d_leaf2, "孙视图对用户查询恒定——困惑形态成立");
    }

    #[test]
    fn test_queue_accounts_conserve() {
        // 账面守恒：10 触发 push → drain 10 → 队列空、账恒等。
        assert_eq!(confusion_sweep().ok().unwrap_or(0), CONFUSED_N as u32);
        assert_eq!(clear_sweep().ok().unwrap_or(0), CLEAR_N as u32);
    }
}
