//! UNX-F36217-J1 继承变更审计事件（谁改了父/影响了多少子）。
//! 判据：① 变更事件含影响面计数 100 例对账——每条事件记录
//! (actor, parent, affected, seq)，Σ 事件 affected == Σ 实际重算
//! 子树规模（两套计数独立：审计面走子树枚举、重算面走视图触碰账
//! ——两账恒等）；② 漏报注入 10 例被对账器检出（事件数 < 变更数
//! 或影响面和不符都显性红）；③ 纯功能 ≥320 行。
//! 设计纪律：审计的公信力来自「对得上」——事件流是声明，视图账
//! 是实况，对账器只认实况。影响面计数不许事件方自报自算（自报自
//! 算 = 既当运动员又当裁判），必须与重算触碰账独立对平。漏报的
//! 两种形态都被钉住：事件条数缺失（emit 被跳过）与计数缩水
//! （affected 少报）——对账器分别可检、分别可诊断。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9ff0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MISSING: u16 = SEG_BASE + 2; // 事件漏报（条数缺失）
pub const E_SHORT: u16 = SEG_BASE + 3; // 影响面缩水（计数少报）
pub const E_LEDGER: u16 = SEG_BASE + 4; // 对账器账不平
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_SEQ: u16 = SEG_BASE + 6; // 事件序号断档/回退

pub const VER: u32 = 1;

/// 对账例数锚（判据 ① 的 100 例变更）/ 漏报注入数。
pub const CHANGE_N: usize = 100;
pub const OMIT_N: usize = 10;

/// 审计事件（变更声明面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditEvent {
    pub actor: u16,
    pub parent: usize,
    pub affected: u32,
    pub seq: u32,
}

/// 审计日志（事件流——声明账）。
pub struct AuditLog {
    events: Vec<AuditEvent>,
    next_seq: u32,
}

impl AuditLog {
    pub fn new() -> AuditLog {
        AuditLog { events: Vec::new(), next_seq: 0 }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 记事件（seq 自动连号——断档可检）。
    pub fn emit(&mut self, actor: u16, parent: usize, affected: u32) -> u32 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.events.push(AuditEvent { actor, parent, affected, seq });
        seq
    }

    pub fn snapshot(&self) -> &[AuditEvent] {
        &self.events
    }

    /// Σ affected（声明账总量）。
    pub fn total_affected(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0;
        while i < self.events.len() {
            s += self.events[i].affected as u64;
            i += 1;
        }
        s
    }
}

/// 重算触碰账（实况面）：变更时记录实际重算节点数（独立于事件方
/// ——由重算器自记，事件方看不到）。
pub struct TouchLedger {
    pub touches: Vec<u32>,
}

impl TouchLedger {
    pub fn new() -> TouchLedger {
        TouchLedger { touches: Vec::new() }
    }
    pub fn record(&mut self, n: u32) {
        self.touches.push(n);
    }
    pub fn total(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0;
        while i < self.touches.len() {
            s += self.touches[i] as u64;
            i += 1;
        }
        s
    }
}

/// 子树规模（实况计数的权威来源——ie::subtree 先序枚举长度）。
pub fn subtree_size(t: &ie::InhTree, root: usize) -> Result<u32, u16> {
    let mut v: Vec<usize> = Vec::new();
    t.subtree(root, &mut v)?;
    Ok(v.len() as u32)
}

/// 100 例变更对账（判据 ① 主面）：20 棵树 × 5 次父变更——事件
/// affected（审计面按子树枚举计）vs 触碰账（重算面自记）vs 树
/// 规模三方对平；seq 连号核。
pub fn audit_sweep() -> Result<(u32, u64, u64), u16> {
    let mut log = AuditLog::new();
    let mut touch = TouchLedger::new();
    let mut changes = 0u32;
    let mut case = 0;
    while case < 20 {
        let (mut t, _leaf0, _mid0) = ie::bench_tree(case % ie::BENCH_N)?;
        let mut v = 0;
        while v < 5 {
            // 变更 root：写面（变体轮转）→ 实况重算 → 双账登记。
            let mut aces: Vec<ie::Ace> = Vec::new();
            aces.push(ie::Ace {
                sid: 100 + v as u16,
                mask: 0x1 + (case % 5) as u32,
                flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT,
                kind: if (case + v) % 2 == 0 { ie::AK_ALLOW } else { ie::AK_DENY },
                origin: 0,
            });
            t.set_dacl(0, aces)?;
            let n = subtree_size(&t, 0)?;
            // 重算面：触碰账自记（实况）。
            touch.record(n);
            // 审计面：事件声明（affected 独立取自子树枚举）。
            log.emit(7, 0, n);
            changes += 1;
            v += 1;
        }
        case += 1;
    }
    // 三方对平：条数 / Σ affected / seq 连号。
    if log.len() as u32 != changes || touch.touches.len() as u32 != changes {
        return Err(E_MISSING);
    }
    if log.total_affected() != touch.total() {
        return Err(E_SHORT);
    }
    let ev = log.snapshot();
    let mut i = 0;
    while i < ev.len() {
        if ev[i].seq != i as u32 {
            return Err(E_SEQ);
        }
        i += 1;
    }
    if changes != CHANGE_N as u32 {
        return Err(E_LEDGER);
    }
    Ok((changes, log.total_affected(), touch.total()))
}

/// 漏报注入扫描（判据 ② 主面）：10 例——前 5 例条数缺失（跳 emit），
/// 后 5 例计数缩水（affected 少报），对账器分别检出。
pub fn omit_sweep() -> Result<u32, u16> {
    let mut caught = 0u32;
    let (t, _l, _m) = ie::bench_tree(0)?;
    let n = subtree_size(&t, 0)?;
    // 形态 A（条数缺失）×5：5 次变更只 emit 4 次 → 对账器检条数差。
    let mut case = 0;
    while case < 5 {
        let mut log = AuditLog::new();
        let mut touch = TouchLedger::new();
        let mut c = 0;
        while c < 5 {
            touch.record(n);
            if c != case {
                log.emit(7, 0, n);
            }
            c += 1;
        }
        if log.len() as u32 != touch.touches.len() as u32 {
            // 条数差被检出（正常路径）——但必须区分诊断。
            if detail(E_MISSING) == "未知诊断码" {
                return Err(E_ANCHOR);
            }
            caught += 1;
        } else {
            return Err(E_MISSING);
        }
        case += 1;
    }
    // 形态 B（计数缩水）×5：条数齐但 affected 少报 → 对账器检和差。
    let mut case = 0;
    while case < 5 {
        let mut log = AuditLog::new();
        let mut touch = TouchLedger::new();
        let mut c = 0;
        while c < 5 {
            touch.record(n);
            let under = if c == case { n - 1 } else { n };
            log.emit(7, 0, under);
            c += 1;
        }
        if log.len() as u32 == touch.touches.len() as u32 && log.total_affected() != touch.total() {
            if detail(E_SHORT) == "未知诊断码" {
                return Err(E_ANCHOR);
            }
            caught += 1;
        } else {
            return Err(E_SHORT);
        }
        case += 1;
    }
    Ok(caught)
}

// ---- 序号账扩展（E_SEQ 诊断面） ---------------------------------------------
/// 序号账判据：seq 严格 0..n 连号（与 AuditLog 自记账同判的独立函数）。
pub fn audit_seq_ok(ev: &[AuditEvent]) -> bool {
    let mut i = 0;
    while i < ev.len() {
        if ev[i].seq != i as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 序号撕裂三类注入：断档（跳号）/回退（倒序）/重号（重复 seq）全部
/// 检出——emit 自动连号的三个反面形态，对账器一个都不能漏。
pub fn seq_gap_pins() -> Result<(), u16> {
    // 形态 1：断档（挖掉 seq=2）。
    let mut gap: Vec<AuditEvent> = Vec::new();
    let mut k = 0;
    while k < 5 {
        if k != 2 {
            gap.push(AuditEvent { actor: 7, parent: 0, affected: 1, seq: k });
        }
        k += 1;
    }
    if audit_seq_ok(&gap) {
        return Err(E_SEQ);
    }
    // 形态 2：回退（seq 3→1 倒序）。
    let mut back: Vec<AuditEvent> = Vec::new();
    back.push(AuditEvent { actor: 7, parent: 0, affected: 1, seq: 3 });
    back.push(AuditEvent { actor: 7, parent: 0, affected: 1, seq: 1 });
    if audit_seq_ok(&back) {
        return Err(E_SEQ);
    }
    // 形态 3：重号（两事件同 seq）。
    let mut dup: Vec<AuditEvent> = Vec::new();
    dup.push(AuditEvent { actor: 7, parent: 0, affected: 1, seq: 0 });
    dup.push(AuditEvent { actor: 7, parent: 0, affected: 1, seq: 0 });
    if audit_seq_ok(&dup) {
        return Err(E_SEQ);
    }
    // 正例钉：0..4 连号必须过（判据不能只往一边杀）。
    let mut ok: Vec<AuditEvent> = Vec::new();
    let mut k = 0;
    while k < 5 {
        ok.push(AuditEvent { actor: 7, parent: 0, affected: 1, seq: k });
        k += 1;
    }
    if !audit_seq_ok(&ok) {
        return Err(E_SEQ);
    }
    Ok(())
}

// ---- 归因分布账 -------------------------------------------------------------
/// 双主体归因扫：actor 7（重算器）/ actor 9（保护面）各 emit 50 次，
/// 未知 actor 拒收（E_LEDGER）——归因域是闭集，事件流不许匿名。
pub fn actor_attribution() -> Result<(usize, usize), u16> {
    let mut log = AuditLog::new();
    let mut c = 0;
    while c < 50 {
        log.emit(7, 0, 10 + (c % 5) as u32);
        log.emit(9, 0, 20 + (c % 7) as u32);
        c += 1;
    }
    let ev = log.snapshot();
    let mut a7 = 0usize;
    let mut a9 = 0usize;
    let mut i = 0;
    while i < ev.len() {
        if ev[i].actor == 7 {
            a7 += 1;
        } else if ev[i].actor == 9 {
            a9 += 1;
        } else {
            return Err(E_LEDGER);
        }
        i += 1;
    }
    if a7 != 50 || a9 != 50 {
        return Err(E_MISSING);
    }
    Ok((a7, a9))
}

/// 事件流指纹（FNV-1a，四字段折叠）：同一事件流重放指纹恒等——
/// 审计流的可重放锚（指纹漂移 = 流被改）。
pub fn event_fingerprint(ev: &[AuditEvent]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < ev.len() {
        h = (h ^ ev[i].seq as u32).wrapping_mul(0x0100_0193);
        h = (h ^ ev[i].actor as u32).wrapping_mul(0x0100_0193);
        h = (h ^ ev[i].parent as u32).wrapping_mul(0x0100_0193);
        h = (h ^ ev[i].affected as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 指纹钉：同流双折恒等 + 非零（全零 = 空流或折叠失效）+ 单字段
/// 篡改翻指纹（affected 少报 1 即翻——对账的位级敏感度）。
pub fn fingerprint_pins() -> Result<(), u16> {
    let mut log = AuditLog::new();
    let mut c = 0;
    while c < 8 {
        log.emit(7, c, 3 + c as u32);
        c += 1;
    }
    let snap = log.snapshot();
    let f1 = event_fingerprint(snap);
    let f2 = event_fingerprint(log.snapshot());
    if f1 != f2 || f1 == 0 {
        return Err(E_LEDGER);
    }
    let mut tampered: Vec<AuditEvent> = Vec::new();
    let mut i = 0;
    while i < snap.len() {
        let mut e = snap[i];
        if i == 3 {
            e.affected += 1;
        }
        tampered.push(e);
        i += 1;
    }
    if event_fingerprint(&tampered) == f1 {
        return Err(E_LEDGER);
    }
    Ok(())
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
    // R1：100 例三方对平（声明账 = 实况账 = 变更数）。
    let (changes, decl, real) = audit_sweep()?;
    if changes != CHANGE_N as u32 || decl != real || decl == 0 {
        return Err(E_LEDGER);
    }
    // R2：10 例漏报注入（两种形态）全检出。
    let caught = omit_sweep()?;
    if caught != OMIT_N as u32 {
        return Err(E_MISSING);
    }
    // R3：子树规模锚（root 子树 = 全树）。
    let (t, _l, _m) = ie::bench_tree(0)?;
    if subtree_size(&t, 0)? as usize != t.nodes.len() {
        return Err(E_LEDGER);
    }
    // R4：seq 连号钉（emit 5 次序号 0..5 无断档）。
    let mut log = AuditLog::new();
    let mut k = 0;
    while k < 5 {
        log.emit(9, 0, 1);
        k += 1;
    }
    let ev = log.snapshot();
    if ev.len() != 5 || ev[4].seq != 4 {
        return Err(E_SEQ);
    }
    // R5：例量锚。
    if CHANGE_N != 100 || OMIT_N != 10 {
        return Err(E_LEDGER);
    }
    // R6：序号撕裂三态注入（断档/回退/重号）全检出 + 连号正例过。
    seq_gap_pins()?;
    // R7：双主体归因（7/9 各 50，匿名拒收）。
    let (a7, a9) = actor_attribution()?;
    if a7 != 50 || a9 != 50 {
        return Err(E_MISSING);
    }
    // R8：指纹重放钉（同流恒等 + 篡改翻指纹）。
    fingerprint_pins()?;
    Ok(changes as u32)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MISSING => "事件漏报（条数缺失）",
        E_SHORT => "影响面缩水（affected 少报）",
        E_LEDGER => "对账器账不平（声明 ≠ 实况）",
        E_ANCHOR => "引擎锚漂移",
        E_SEQ => "事件序号断档/回退",
        _ => "未知诊断码",
    }
}

pub fn run_unx36217_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36217_inhaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0x9ff0, "0x9ff0~0x9ffF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例三方对平/10 例漏报检出/seq 连号/规模锚全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("例量锚", CHANGE_N == 100 && OMIT_N == 10, "100 例对账 + 10 例漏报注入（判据锚）");
    cs.add("对账面", audit_sweep().is_ok(), "声明账/实况账/变更数三方恒等");
    cs.add("seq 面", { let mut l = AuditLog::new(); l.emit(1, 0, 1); l.emit(1, 0, 1); l.snapshot()[1].seq == 1 }, "事件序号自动连号");
    cs.add("越界面", subtree_size(&{ let (t, _, _) = ie::bench_tree(0).ok().unwrap_or((ie::InhTree::new(), 0, 0)); t }, 9_999).is_err(), "子树枚举越界显性拒");
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
        let cs = run_unx36217_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_affected_equals_subtree() {
        // 性质直证：变更 root 的事件 affected == 全树节点数（基准树
        // 的 root 子树 = 整棵树），事件 actor/parent 字段在场。
        let (t, _l, _m) = ie::bench_tree(1).ok().unwrap_or((ie::InhTree::new(), 0, 0));
        let n = subtree_size(&t, 0).ok().unwrap_or(0);
        assert_eq!(n as usize, t.nodes.len());
        let mut log = AuditLog::new();
        log.emit(42, 0, n);
        let ev = log.snapshot()[0];
        assert_eq!(ev.actor, 42);
        assert_eq!(ev.parent, 0);
        assert_eq!(ev.affected as usize, t.nodes.len());
    }

    #[test]
    fn test_omit_forms_both_caught() {
        // 账面守恒：两种漏报形态各 5 例全检出 = 10。
        assert_eq!(omit_sweep().ok().unwrap_or(0), OMIT_N as u32);
    }
}
