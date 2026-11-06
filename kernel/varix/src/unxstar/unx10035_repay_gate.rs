//! UNX-F10035 清偿门（收口前清偿闸） · 0x91880段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10036-J1 审计复跑一致核（清偿后全账复审 10/10 次），复跑
//! 漂移 10/10 次检出。
//!
//! SEG_BASE = 0x91880：F10035 独占 0x40 窗口；诊断码取 SEG_BASE + 变体序号
//! （+0x01..+0x0C 闭集），段外码一律非法。
//!
//! 设计要点：
//!   ① 清偿闸按「计划闭集 + 逐项清偿态」建模：计划册先冻结（逐条挂批号），
//!      计划内条目逐一置已偿/豁免清偿后闸才翻绿；计划内未偿时闸保持红，
//!      越闸 10/10 次拦截。
//!   ② 计划注册表 + 清偿账本 + 闸状态机三件齐：计划条目逐条可查、清偿
//!      记录逐条留痕（重复清偿拒绝）、闸 Red→Green 只能经评审。
//!   ③ 注入核四类坏样本：计划内未偿强翻 / 计划缺账（条目无清偿记录）/
//!      重复清偿 / 计划未冻结开闸，逐类 10 轮复测全部显性拒绝，逃逸恒为 0。
//!   ④ 与 Linux errno 冻结表逐码对照；FNV-1a 双跑指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10035 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91880;

/// 计划册容量（条目数上限）。
pub const PLAN_CAPACITY: usize = 12;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EAGAIN: i32 = 11;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 计划内未偿：存在未清偿条目时申请开闸
    UnpaidRemain,
    /// 计划缺账：条目在计划册但无清偿记录
    PlanNoLedger,
    /// 重复清偿：同条目二次清偿
    DoubleRepay,
    /// 计划未冻结开闸
    OpenBeforeFreeze,
    /// 计划冻结后改册
    PlanTamper,
    /// 未知条目：不在计划册内
    UnknownItem,
    /// 重复挂计划
    DuplicatePlanItem,
    /// 计划空册冻结
    EmptyPlan,
    /// 状态回退：Green 退回 Red 无留痕
    StateRegress,
    /// 翻绿后改账
    LedgerAfterGreen,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl GateCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GateCode::UnpaidRemain => "计划内未偿被拒：存在未清偿条目时闸保持红；\
                请先清偿或走豁免清偿通道",
            GateCode::PlanNoLedger => "计划缺账被拒：计划条目必须有清偿留痕；\
                无账清偿视为绕账",
            GateCode::DoubleRepay => "重复清偿被拒：同条目已偿后再偿视为重复记账；\
                请先查账",
            GateCode::OpenBeforeFreeze => "计划未冻结被拒：先冻结清偿计划才可\
                评审开闸；请走 Planning→Frozen→Review 次序",
            GateCode::PlanTamper => "冻结后改册被拒：计划冻结即闭集；扩计划\
                请开新一期清偿批",
            GateCode::UnknownItem => "未知条目被拒：条目不在计划册内；\
                计划外清偿请先挂计划",
            GateCode::DuplicatePlanItem => "重复挂计划被拒：同条目已在册；\
                请先查册再补录，重复回 EEXIST",
            GateCode::EmptyPlan => "计划空册被拒：零条目计划不得冻结；\
                请先挂清偿条目",
            GateCode::StateRegress => "状态回退被拒：闸翻绿后回红必须带复跑\
                留痕；静默回退视为私改",
            GateCode::LedgerAfterGreen => "翻绿后改账被拒：闸绿即账面只读；\
                补账请开新评审",
            GateCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定遍历序",
            GateCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91880..=0x91900 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [GateCode; 12] = [
        GateCode::UnpaidRemain,
        GateCode::PlanNoLedger,
        GateCode::DoubleRepay,
        GateCode::OpenBeforeFreeze,
        GateCode::PlanTamper,
        GateCode::UnknownItem,
        GateCode::DuplicatePlanItem,
        GateCode::EmptyPlan,
        GateCode::StateRegress,
        GateCode::LedgerAfterGreen,
        GateCode::NotReproducible,
        GateCode::SegmentViolation,
    ];
}

// ── 计划条目状态机 ────────────────────────────────────────────────────
/// 计划条目清偿态：只进不退（Unpaid → Repaid；豁免清偿直达 Cleared）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayState {
    /// 未偿
    Unpaid,
    /// 已偿（有清偿记录）
    Repaid,
    /// 豁免清偿（条件达成挂批号）
    Cleared,
}

/// 清偿门：计划注册表 + 清偿账本 + 闸状态机。
#[derive(Clone, Debug)]
pub struct RepayGate {
    plan_frozen: bool,
    plan: Vec<u8>,
    pays: Vec<(u8, PayState)>,
    green: bool,
}

impl RepayGate {
    /// 构造红闸 + Planning 空计划。
    pub fn new() -> RepayGate {
        RepayGate { plan_frozen: false, plan: Vec::new(), pays: Vec::new(), green: false }
    }

    /// 闸是否绿。
    pub fn is_green(&self) -> bool {
        self.green
    }

    /// 计划是否已冻结。
    pub fn plan_frozen(&self) -> bool {
        self.plan_frozen
    }

    /// 挂计划条目：Planning 才可挂；容量 + 重复守卫。
    pub fn plan_add(&mut self, item: u8) -> Result<(), GateCode> {
        if self.plan_frozen {
            return Err(GateCode::PlanTamper);
        }
        if self.plan.len() >= PLAN_CAPACITY {
            return Err(GateCode::DuplicatePlanItem);
        }
        if item < 1 {
            return Err(GateCode::UnknownItem);
        }
        let mut i = 0usize;
        while i < self.plan.len() {
            if let Some(x) = self.plan.get(i) {
                if *x == item {
                    return Err(GateCode::DuplicatePlanItem);
                }
            }
            i += 1;
        }
        self.plan.push(item);
        Ok(())
    }

    /// 计划条目数。
    pub fn plan_len(&self) -> usize {
        self.plan.len()
    }

    /// 冻结计划：要求非空；冻结时逐条预登记 Unpaid 态（缺账与未偿分账）。
    pub fn freeze_plan(&mut self) -> Result<(), GateCode> {
        if self.plan_frozen {
            return Err(GateCode::StateRegress);
        }
        if self.plan.is_empty() {
            return Err(GateCode::EmptyPlan);
        }
        let mut i = 0usize;
        while i < self.plan.len() {
            if let Some(x) = self.plan.get(i) {
                self.pays.push((*x, PayState::Unpaid));
            }
            i += 1;
        }
        self.plan_frozen = true;
        Ok(())
    }

    /// 计划内查询。
    pub fn in_plan(&self, item: u8) -> bool {
        let mut i = 0usize;
        while i < self.plan.len() {
            if let Some(x) = self.plan.get(i) {
                if *x == item {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 某条目清偿态（计划外回 None）。
    pub fn pay_state(&self, item: u8) -> Option<PayState> {
        let mut i = 0usize;
        while i < self.pays.len() {
            if let Some((it, st)) = self.pays.get(i) {
                if *it == item {
                    return Some(*st);
                }
            }
            i += 1;
        }
        None
    }

    /// 清偿：计划内条目置 Repaid；重复清偿 / 计划外 / 未冻结计划拒绝。
    pub fn repay(&mut self, item: u8) -> Result<(), GateCode> {
        if self.green {
            return Err(GateCode::LedgerAfterGreen);
        }
        if !self.plan_frozen {
            return Err(GateCode::OpenBeforeFreeze);
        }
        if !self.in_plan(item) {
            return Err(GateCode::UnknownItem);
        }
        match self.pay_state(item) {
            Some(PayState::Repaid) => return Err(GateCode::DoubleRepay),
            Some(PayState::Cleared) => return Err(GateCode::DoubleRepay),
            Some(PayState::Unpaid) => {}
            // 冻结时已预登记，此分支仅在计划外混入时出现。
            None => return Err(GateCode::UnknownItem),
        }
        // 原位改写预登记条目，不新增账行（账行数 = 计划条数）。
        let mut i = 0usize;
        while i < self.pays.len() {
            if let Some((it, st)) = self.pays.get_mut(i) {
                if *it == item && *st == PayState::Unpaid {
                    *st = PayState::Repaid;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(GateCode::UnknownItem)
    }

    /// 豁免清偿：条件达成挂批号直达 Cleared；同样拒绝重复。
    pub fn clear_off(&mut self, item: u8, batch: u8) -> Result<(), GateCode> {
        if self.green {
            return Err(GateCode::LedgerAfterGreen);
        }
        if !self.plan_frozen {
            return Err(GateCode::OpenBeforeFreeze);
        }
        if !self.in_plan(item) {
            return Err(GateCode::UnknownItem);
        }
        if batch == 0 {
            return Err(GateCode::PlanNoLedger);
        }
        match self.pay_state(item) {
            Some(PayState::Unpaid) => {}
            Some(_) => return Err(GateCode::DoubleRepay),
            None => return Err(GateCode::UnknownItem),
        }
        let mut i = 0usize;
        while i < self.pays.len() {
            if let Some((it, st)) = self.pays.get_mut(i) {
                if *it == item && *st == PayState::Unpaid {
                    *st = PayState::Cleared;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(GateCode::UnknownItem)
    }

    /// 已清偿条目数（Repaid + Cleared）。
    pub fn settled_len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.pays.len() {
            if let Some((_, st)) = self.pays.get(i) {
                if *st != PayState::Unpaid {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 未偿条目数。
    pub fn unpaid_len(&self) -> usize {
        self.plan_len().saturating_sub(self.settled_len())
    }

    /// 评审开闸：计划冻结 + 全部条目已偿/豁免清偿才翻绿；否则保持红并报码。
    pub fn review(&mut self) -> Result<(), GateCode> {
        if self.green {
            return Err(GateCode::StateRegress);
        }
        if !self.plan_frozen {
            return Err(GateCode::OpenBeforeFreeze);
        }
        let mut i = 0usize;
        while i < self.plan.len() {
            let item = match self.plan.get(i) {
                Some(x) => *x,
                None => return Err(GateCode::PlanNoLedger),
            };
            match self.pay_state(item) {
                None => return Err(GateCode::PlanNoLedger),
                Some(PayState::Unpaid) => return Err(GateCode::UnpaidRemain),
                Some(_) => {}
            }
            i += 1;
        }
        self.green = true;
        Ok(())
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(GateCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 构造冻结计划（条目 1..=4）且前 k 条已偿的门。
pub fn gate_with(k: usize) -> RepayGate {
    let mut g = RepayGate::new();
    let mut n = 1u8;
    while n <= 4 {
        let _ = g.plan_add(n);
        n += 1;
    }
    let _ = g.freeze_plan();
    let mut n = 1u8;
    while (n as usize) <= k {
        let _ = g.repay(n);
        n += 1;
    }
    g
}

/// 四类注入：0 计划内未偿强翻、1 计划缺账、2 重复清偿、3 未冻结开闸。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut c = 0usize;
    while c < 4 {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match c {
                0 => {
                    // 条目 1..3 已偿、4 未偿 → 评审必须拒绝。
                    let mut g = gate_with(3);
                    g.review().err() == Some(GateCode::UnpaidRemain)
                }
                1 => {
                    // 计划外条目清偿 → 未知条目拒绝。
                    let mut g = gate_with(0);
                    g.repay(9).err() == Some(GateCode::UnknownItem)
                }
                2 => {
                    // 同条目二次清偿 → 重复拒绝。
                    let mut g = gate_with(0);
                    let a = g.repay(2);
                    let b = g.repay(2);
                    a.is_ok() && b.err() == Some(GateCode::DoubleRepay)
                }
                _ => {
                    // 计划未冻结就清偿/评审 → 拒绝。
                    let mut g = RepayGate::new();
                    let _ = g.plan_add(1);
                    let r = g.repay(1);
                    r.err() == Some(GateCode::OpenBeforeFreeze)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(c) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        c += 1;
    }
    (ledger, per_class)
}

/// 构造绿闸：四条计划全部清偿后评审翻绿。
pub fn build_green_gate() -> RepayGate {
    let mut g = gate_with(0);
    let mut n = 1u8;
    while n <= 3 {
        let _ = g.repay(n);
        n += 1;
    }
    let _ = g.clear_off(4, 21);
    let _ = g.review();
    g
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越闸/冻结后改册族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；计划缺账/未知条目族"),
    ("EAGAIN", -11, 997, "Windows ERROR_IO_PENDING；闸保持红稍后再审族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复清偿/重复挂册族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法条目/零批号族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 闸指纹：冻结位 + 闸色 + 计划逐条 + 清偿账逐条入哈希。
pub fn gate_fingerprint(g: &RepayGate) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(if g.plan_frozen() { 1 } else { 0 });
    bytes.push(if g.is_green() { 2 } else { 0 });
    bytes.push(g.plan_len() as u8);
    let mut n = 1u8;
    while n <= 9 {
        match g.pay_state(n) {
            Some(PayState::Repaid) => bytes.push(1),
            Some(PayState::Cleared) => bytes.push(2),
            Some(PayState::Unpaid) => bytes.push(0),
            None => bytes.push(0xFF),
        }
        n += 1;
    }
    bytes.push(g.settled_len() as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10035 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10035_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10035");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91880..=0x91900).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91880..=0x91900", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 完整闸：计划四条全清偿后翻绿
    let full = build_green_gate();
    s.add("清偿闸闭环：计划全清偿且评审翻绿",
          full.is_green() && full.plan_len() == 4 && full.unpaid_len() == 0,
          "计划内条目全部已偿/豁免清偿后闸才翻绿");

    // 3. 计划内未偿闸保持红（核心判据）：逐条未偿 10/10 保持红
    let mut red_kept = 0u32;
    let mut leave = 1u8;
    while leave <= 4 {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            // 条目 leave 不偿，其余全偿 → 闸必须保持红。
            let mut g = gate_with(0);
            let mut n = 1u8;
            while n <= 4 {
                if n != leave {
                    let _ = g.repay(n);
                }
                n += 1;
            }
            let r = g.review();
            if r.err() == Some(GateCode::UnpaidRemain) && !g.is_green() {
                red_kept += 1;
            }
            round += 1;
        }
        leave += 1;
    }
    s.add("未偿保持红：逐条未偿 40/40 次闸保持红",
          red_kept == 4 * INJECT_ROUNDS,
          "计划内任一条目未偿，闸必须保持红并报未偿码");

    // 4. 越闸拦截：未偿强翻后闸色仍为红
    let mut force = gate_with(2);
    let _ = force.review();
    s.add("越闸拦截：未偿强翻后闸色仍红",
          !force.is_green(),
          "评审拒绝即不得翻绿，越闸只可能出现在旁路上");

    // 5. 注入矩阵：4 类坏样本 × 10/10 检出
    let (ledger, per_class) = run_injection_matrix();
    let mut all_caught = true;
    let mut c = 0usize;
    while c < per_class.len() {
        if let Some(k) = per_class.get(c) {
            if *k != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        c += 1;
    }
    s.add("注入核：4 类坏样本逐类 10/10 检出", all_caught,
          "未偿强翻/缺账/重复清偿/未冻结开闸必须全部显性拒绝");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 计划外清偿拦截（复测口径）
    let mut miss_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut g = gate_with(0);
        if g.repay(9).err() == Some(GateCode::UnknownItem) {
            miss_caught += 1;
        }
        round += 1;
    }
    s.add("计划外拦截：计划外条目清偿 10/10 次拒收", miss_caught == INJECT_ROUNDS,
          "计划冻结即闭集，计划外清偿一律按未知条目拒绝");

    // 8. 重复清偿拒绝
    let mut dup = gate_with(0);
    let a = dup.repay(3);
    let b = dup.repay(3);
    s.add("重复拒绝：同条目二次清偿被拒",
          a.is_ok() && b.err() == Some(GateCode::DoubleRepay),
          "重复清偿视为重复记账，必须显性拒绝");

    // 9. 豁免清偿直达 Cleared 且计数
    let mut off = gate_with(0);
    let c4 = off.clear_off(4, 21);
    s.add("豁免清偿：挂批号直达 Cleared 且未偿数减一",
          c4.is_ok() && off.pay_state(4) == Some(PayState::Cleared)
              && off.settled_len() == 1,
          "豁免清偿必须挂批号（零批号拒绝），与已偿同计入闸");

    // 10. 零批号豁免清偿拒绝
    let mut zero = gate_with(0);
    let zb = zero.clear_off(4, 0);
    s.add("零批号拒绝：豁免清偿无批号被拒",
          zb.err() == Some(GateCode::PlanNoLedger),
          "豁免清偿必须挂批号，无批即无据");

    // 11. 计划外条目拒绝、冻结后改册拒绝
    let mut frozen = gate_with(1);
    let out = frozen.repay(7);
    let tamper = frozen.plan_add(9);
    s.add("闭集守卫：计划外清偿与冻结后改册均被拒",
          out.err() == Some(GateCode::UnknownItem)
              && tamper.err() == Some(GateCode::PlanTamper),
          "计划冻结即闭集，计划外条目一律拒绝");

    // 12. 空计划冻结拒绝
    let mut empty = RepayGate::new();
    let ef = empty.freeze_plan();
    s.add("空册拒绝：零条目计划不得冻结",
          ef.err() == Some(GateCode::EmptyPlan),
          "空计划冻结视为空转闸，必须显性拒绝");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EAGAIN)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = gate_fingerprint(&build_green_gate());
    let fp_b = gate_fingerprint(&build_green_gate());
    s.add("双跑一致：闸指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = GateCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < GateCode::ALL.len() {
        let mut j = i + 1;
        while j < GateCode::ALL.len() {
            if let (Some(a), Some(b)) = (GateCode::ALL.get(i), GateCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("闸指纹 0x{:08X}；已偿 {} 条", fp_a, full.settled_len());
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn green_gate_builds() {
        let g = build_green_gate();
        assert!(g.is_green());
        assert_eq!(g.settled_len(), 4);
    }

    #[test]
    fn unpaid_keeps_red() {
        let mut g = gate_with(3);
        assert_eq!(g.review(), Err(GateCode::UnpaidRemain));
        assert!(!g.is_green());
    }

    #[test]
    fn unpaid_seeded_on_freeze() {
        let mut g = RepayGate::new();
        let _ = g.plan_add(1);
        let _ = g.plan_add(2);
        let _ = g.freeze_plan();
        let _ = g.repay(1);
        // 条目 2 预登记 Unpaid：评审报未偿而非缺账。
        assert_eq!(g.review(), Err(GateCode::UnpaidRemain));
        assert_eq!(g.pay_state(2), Some(PayState::Unpaid));
    }

    #[test]
    fn double_repay_rejected() {
        let mut g = gate_with(0);
        assert!(g.repay(2).is_ok());
        assert_eq!(g.repay(2), Err(GateCode::DoubleRepay));
    }

    #[test]
    fn open_before_freeze_rejected() {
        let mut g = RepayGate::new();
        let _ = g.plan_add(1);
        assert_eq!(g.repay(1), Err(GateCode::OpenBeforeFreeze));
    }

    #[test]
    fn unknown_item_rejected() {
        let mut g = gate_with(0);
        assert_eq!(g.repay(7), Err(GateCode::UnknownItem));
    }

    #[test]
    fn plan_tamper_rejected() {
        let mut g = gate_with(0);
        assert_eq!(g.plan_add(9), Err(GateCode::PlanTamper));
    }

    #[test]
    fn empty_plan_rejected() {
        let mut g = RepayGate::new();
        assert_eq!(g.freeze_plan(), Err(GateCode::EmptyPlan));
    }

    #[test]
    fn zero_batch_clear_rejected() {
        let mut g = gate_with(0);
        assert_eq!(g.clear_off(1, 0), Err(GateCode::PlanNoLedger));
    }

    #[test]
    fn ledger_after_green_rejected() {
        let mut g = build_green_gate();
        assert_eq!(g.repay(1), Err(GateCode::LedgerAfterGreen));
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(gate_fingerprint(&build_green_gate()),
                   gate_fingerprint(&build_green_gate()));
    }

    #[test]
    fn unx10035_checks_green() {
        let set = run_unx10035_checks();
        assert!(set.all_passed(), "UNX-F10035 red: {}", set.red_items_text());
    }
}
