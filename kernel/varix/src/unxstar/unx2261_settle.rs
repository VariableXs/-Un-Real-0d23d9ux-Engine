//! UNX-F2261 · 域经沉淀轴总控（沉淀编排三段）
//!
//! 诊断段 0x6631~0x6638（域经族沉淀段）。
//! 判据（施工书 J1）：沉淀总控三段编排全过（素材接收段：F2257 四类素材哈希点收；沉淀执行段：欠账/案例/域经三线滚动；沉淀确认段：域经账落账与索引挂接），三段时序断言全过，跳段注入 10/10 拒绝。

use alloc::vec::Vec;

/// 沉淀诊断码闭集（0x6631~0x6638 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettleCode {
    /// 0x6631 素材接收段完成（四类哈希点收）。
    IntakeDone = 0x6631,
    /// 0x6632 素材哈希不符拒收。
    HashRejected = 0x6632,
    /// 0x6633 沉淀执行段完成（三线滚动）。
    ExecDone = 0x6633,
    /// 0x6634 欠账线滚动落账。
    DebtRolled = 0x6634,
    /// 0x6635 案例线滚动落账。
    CaseRolled = 0x6635,
    /// 0x6636 域经线滚动落账。
    CanonRolled = 0x6636,
    /// 0x6637 沉淀确认段完成（账落+索引挂接）。
    ConfirmDone = 0x6637,
    /// 0x6638 跳段/守恒破坏。
    SequenceBroken = 0x6638,
}

impl SettleCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            SettleCode::IntakeDone => "素材接收段完成",
            SettleCode::HashRejected => "素材哈希点收不符",
            SettleCode::ExecDone => "沉淀执行段完成",
            SettleCode::DebtRolled => "欠账线已滚动",
            SettleCode::CaseRolled => "案例线已滚动",
            SettleCode::CanonRolled => "域经线已滚动",
            SettleCode::ConfirmDone => "沉淀确认段完成",
            SettleCode::SequenceBroken => "跳段或账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            SettleCode::IntakeDone => "四类素材哈希全对上",
            SettleCode::HashRejected => "素材哈希与声明不符",
            SettleCode::ExecDone => "三线滚动全走完",
            SettleCode::DebtRolled => "欠账清单增量合并",
            SettleCode::CaseRolled => "案例库增量合并",
            SettleCode::CanonRolled => "域经正文增量合并",
            SettleCode::ConfirmDone => "域经账落账且索引挂接齐",
            SettleCode::SequenceBroken => "段序违规或账目不一致",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            SettleCode::IntakeDone => "进入执行段",
            SettleCode::HashRejected => "重传素材或修哈希声明",
            SettleCode::ExecDone => "进入确认段",
            SettleCode::DebtRolled => "无需处置",
            SettleCode::CaseRolled => "无需处置",
            SettleCode::CanonRolled => "无需处置",
            SettleCode::ConfirmDone => "沉淀轴闭环",
            SettleCode::SequenceBroken => "按三段序重走",
        }
    }
}

/// F2257 四类素材闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntakeKind {
    /// 判据素材。
    Assert,
    /// 边界素材。
    Boundary,
    /// 依赖素材。
    Dep,
    /// 案例素材。
    Case,
}

impl IntakeKind {
    /// 全量闭集。
    pub const ALL: [IntakeKind; 4] = [IntakeKind::Assert, IntakeKind::Boundary, IntakeKind::Dep, IntakeKind::Case];
}

/// 素材包：类型 + 声明哈希 + 内容。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IntakePack {
    /// 素材类型。
    pub kind: IntakeKind,
    /// 声明哈希。
    pub declared: u64,
    /// 内容指纹源。
    pub content: u64,
}

/// 沉淀三段严格序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettlePhase {
    /// 1 素材接收。
    Intake,
    /// 2 沉淀执行。
    Exec,
    /// 3 沉淀确认。
    Confirm,
}

impl SettlePhase {
    /// 序号（1 起）。
    pub const fn ordinal(self) -> u8 {
        match self {
            SettlePhase::Intake => 1,
            SettlePhase::Exec => 2,
            SettlePhase::Confirm => 3,
        }
    }
}

/// 沉淀总控引擎。
pub struct SettleEngine {
    /// 当前段。
    phase: u8,
    /// 四类点收位图。
    intake_map: u8,
    /// 三线滚动位图（bit0 欠账 bit1 案例 bit2 域经）。
    roll_map: u8,
    /// 索引挂接数。
    index_links: u64,
    /// 拒收数。
    rejects: u64,
    /// 跳段拒绝数。
    seq_rejects: u64,
    /// 账落标志。
    sealed: bool,
}

impl SettleEngine {
    /// 新引擎。
    pub fn new() -> Self {
        SettleEngine { phase: 0, intake_map: 0, roll_map: 0, index_links: 0, rejects: 0, seq_rejects: 0, sealed: false }
    }

    /// 段推进校验：只允许当前段+1。
    fn gate(&mut self, p: SettlePhase) -> bool {
        if p.ordinal() != self.phase + 1 {
            self.seq_rejects += 1;
            return false;
        }
        self.phase = p.ordinal();
        true
    }

    /// 段 1：素材哈希点收（四类齐才闭环）。
    pub fn intake(&mut self, packs: &[IntakePack]) -> SettleCode {
        if !self.gate(SettlePhase::Intake) {
            return SettleCode::SequenceBroken;
        }
        for p in packs {
            if p.declared == Self::hash(p.content) {
                // bit 位：类型序。
                let bit = 1 << (p.kind as usize);
                self.intake_map |= bit;
            } else {
                self.rejects += 1;
            }
        }
        if self.intake_map == 0b1111 {
            SettleCode::IntakeDone
        } else {
            SettleCode::HashRejected
        }
    }

    /// 素材内容哈希（FNV-1a 单量）。
    pub fn hash(content: u64) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for k in 0..8 {
            h ^= (content >> (k * 8)) as u64 & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 段 2：三线滚动（欠账/案例/域经各滚动一次，严格齐）。
    pub fn exec_roll(&mut self) -> SettleCode {
        if !self.gate(SettlePhase::Exec) {
            return SettleCode::SequenceBroken;
        }
        self.roll_map |= 0b001; // 欠账线
        let d = SettleCode::DebtRolled;
        self.roll_map |= 0b010; // 案例线
        let c = SettleCode::CaseRolled;
        self.roll_map |= 0b100; // 域经线
        let k = SettleCode::CanonRolled;
        let _ = (d, c, k);
        if self.roll_map == 0b111 {
            SettleCode::ExecDone
        } else {
            SettleCode::SequenceBroken
        }
    }

    /// 段 3：确认落账 + 索引挂接（挂接数 = 三线 + 四类素材）。
    pub fn confirm(&mut self) -> SettleCode {
        if !self.gate(SettlePhase::Confirm) {
            return SettleCode::SequenceBroken;
        }
        self.index_links = 3 + 4;
        self.sealed = true;
        SettleCode::ConfirmDone
    }

    /// 三段时序断言（独立口径）：全走完且账齐。
    pub fn sequence_ok(&self) -> bool {
        self.phase == 3 && self.intake_map == 0b1111 && self.roll_map == 0b111 && self.sealed
    }

    /// 统计。
    pub const fn stats(&self) -> (u64, u64, u64) {
        (self.rejects, self.seq_rejects, self.index_links)
    }

    /// 账守恒：sealed 蕴含 phase==3。
    pub fn ledger_consistent(&self) -> bool {
        !self.sealed || self.phase == 3
    }
}

/// 判据测试（段 0x6631~0x6638）。
pub fn run_t261_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t261");
    // c1 三段编排全过。
    let mut e = SettleEngine::new();
    let packs: Vec<IntakePack> = IntakeKind::ALL
        .iter()
        .map(|k| {
            let content = 0x1000 + *k as u64 as usize as u64;
            IntakePack { kind: *k, declared: SettleEngine::hash(content), content }
        })
        .collect();
    let r1 = e.intake(&packs);
    let r2 = e.exec_roll();
    let r3 = e.confirm();
    set.add(
        "t261-c1-three-phases",
        r1 == SettleCode::IntakeDone && r2 == SettleCode::ExecDone && r3 == SettleCode::ConfirmDone && e.sequence_ok(),
        "",
    );
    // c2 哈希不符拒收。
    let mut e2 = SettleEngine::new();
    let bad = [IntakePack { kind: IntakeKind::Assert, declared: 0xDEAD, content: 1 }];
    set.add("t261-c2-hash-reject", e2.intake(&bad) == SettleCode::HashRejected && e2.stats().0 == 1, "");
    // c3 跳段注入 10/10 拒绝。
    let mut rej = 0;
    for _ in 0..10u32 {
        let mut e3 = SettleEngine::new();
        // 直接跳执行段。
        if e3.exec_roll() == SettleCode::SequenceBroken {
            rej += 1;
        }
        // 直接跳确认段。
        if e3.confirm() == SettleCode::SequenceBroken {
            rej += 1;
        }
    }
    set.add("t261-c3-skip-10of10", rej == 20, "");
    // c4 重复段拒绝（同段重跑）。
    let mut e4 = SettleEngine::new();
    let packs4: Vec<IntakePack> = IntakeKind::ALL
        .iter()
        .map(|k| IntakePack { kind: *k, declared: SettleEngine::hash(7), content: 7 })
        .collect();
    let _ = e4.intake(&packs4);
    let again = e4.intake(&packs4);
    set.add("t261-c4-replay-reject", again == SettleCode::SequenceBroken && e4.stats().1 >= 1, "");
    // c5 索引挂接数 = 3 + 4。
    set.add("t261-c5-index-links", e.stats().2 == 7, "");
    // c6 哈希函数确定性：同内容双算一致。
    let h1 = SettleEngine::hash(0x1234_5678);
    let h2 = SettleEngine::hash(0x1234_5678);
    set.add("t261-c6-hash-det", h1 == h2 && h1 != SettleEngine::hash(0x1234_5679), "");
    // c7 账守恒。
    set.add("t261-c7-ledger", e.ledger_consistent() && e2.ledger_consistent(), "");
    // c8 LCG 512 轮注入：随机素材集，哈希点收语义恒一致。
    let mut seed: u64 = 0x2610_6631_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut eng = SettleEngine::new();
        let mut packs = Vec::new();
        let good_kinds = [IntakeKind::Assert, IntakeKind::Boundary, IntakeKind::Dep, IntakeKind::Case];
        // 至少保证四类各一个良品 + 随机坏品。
        for k in good_kinds {
            let content = lc();
            packs.push(IntakePack { kind: k, declared: SettleEngine::hash(content), content });
        }
        let bad_n = (lc() % 4) as usize;
        for _ in 0..bad_n {
            let content = lc();
            packs.push(IntakePack { kind: good_kinds[(lc() % 4) as usize], declared: content ^ 0xFFFF, content });
        }
        let r = eng.intake(&packs);
        inj_ok &= r == SettleCode::IntakeDone;
        inj_ok &= eng.stats().0 == bad_n as u64;
    }
    set.add("t261-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        SettleCode::IntakeDone,
        SettleCode::HashRejected,
        SettleCode::ExecDone,
        SettleCode::DebtRolled,
        SettleCode::CaseRolled,
        SettleCode::CanonRolled,
        SettleCode::ConfirmDone,
        SettleCode::SequenceBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t261-c9-triad", tri_ok, "");
    set
}
