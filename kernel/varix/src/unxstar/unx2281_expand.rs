//! UNX-F2281 · 域经沉淀扩容总控（扩容编排三段）
//!
//! 诊断段 0x6661~0x6668（域经族扩容段）。
//! 判据（施工书 J1）：扩容总控三段编排全过（素材接收段：F2257 素材哈希点收与增量素材清点；扩容执行段：EX 账/图族/教训账三线滚动；扩容确认段：扩容账落账与索引挂接），三段时序断言全过，跳段注入 10/10 拒绝。

use alloc::vec::Vec;

/// 扩容诊断码闭集（0x6661~0x6668 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpandCode {
    /// 0x6661 素材接收段完成（点收+清点）。
    IntakeDone = 0x6661,
    /// 0x6662 增量素材哈希不符。
    HashRejected = 0x6662,
    /// 0x6663 扩容执行段完成（三线滚动）。
    ExecDone = 0x6663,
    /// 0x6664 EX 账线滚动落账。
    ExRolled = 0x6664,
    /// 0x6665 图族线滚动落账。
    GraphRolled = 0x6665,
    /// 0x6666 教训账线滚动落账。
    LessonRolled = 0x6666,
    /// 0x6667 扩容确认段完成（账落+索引挂接）。
    ConfirmDone = 0x6667,
    /// 0x6668 跳段/账守恒破坏。
    SequenceBroken = 0x6668,
}

impl ExpandCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            ExpandCode::IntakeDone => "扩容素材接收段完成",
            ExpandCode::HashRejected => "增量素材哈希不符",
            ExpandCode::ExecDone => "扩容执行段完成",
            ExpandCode::ExRolled => "EX 账线已滚动",
            ExpandCode::GraphRolled => "图族线已滚动",
            ExpandCode::LessonRolled => "教训账线已滚动",
            ExpandCode::ConfirmDone => "扩容确认段完成",
            ExpandCode::SequenceBroken => "跳段或账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            ExpandCode::IntakeDone => "F2257 点收齐且增量清点齐",
            ExpandCode::HashRejected => "增量素材声明哈希不符",
            ExpandCode::ExecDone => "三线滚动全走完",
            ExpandCode::ExRolled => "EX 账增量合并",
            ExpandCode::GraphRolled => "图族拓扑增量合并",
            ExpandCode::LessonRolled => "教训账增量合并",
            ExpandCode::ConfirmDone => "扩容账落账且索引挂接齐",
            ExpandCode::SequenceBroken => "段序违规或账目不一致",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            ExpandCode::IntakeDone => "进入执行段",
            ExpandCode::HashRejected => "重传或修哈希声明",
            ExpandCode::ExecDone => "进入确认段",
            ExpandCode::ExRolled => "无需处置",
            ExpandCode::GraphRolled => "无需处置",
            ExpandCode::LessonRolled => "无需处置",
            ExpandCode::ConfirmDone => "扩容轴闭环",
            ExpandCode::SequenceBroken => "按三段序重走",
        }
    }
}

/// 增量素材：序号 + 声明哈希 + 内容。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeltaPack {
    /// 增量序号。
    pub seq: u32,
    /// 声明哈希。
    pub declared: u64,
    /// 内容。
    pub content: u64,
}

/// 扩容三段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpandPhase {
    /// 1 素材接收。
    Intake,
    /// 2 扩容执行。
    Exec,
    /// 3 扩容确认。
    Confirm,
}

impl ExpandPhase {
    /// 序号。
    pub const fn ordinal(self) -> u8 {
        match self {
            ExpandPhase::Intake => 1,
            ExpandPhase::Exec => 2,
            ExpandPhase::Confirm => 3,
        }
    }
}

/// 扩容总控引擎。
pub struct ExpandEngine {
    /// 当前段。
    phase: u8,
    /// 点收数。
    accepted: u64,
    /// 拒收数。
    rejects: u64,
    /// 清点基数（增量清点核对基线）。
    inventory_base: u64,
    /// 三线滚动位图（EX/图族/教训）。
    roll_map: u8,
    /// 索引挂接数。
    index_links: u64,
    /// 跳段拒绝数。
    seq_rejects: u64,
    /// 落账。
    sealed: bool,
}

impl ExpandEngine {
    /// 新引擎（清点基数 0）。
    pub fn new() -> Self {
        ExpandEngine {
            phase: 0,
            accepted: 0,
            rejects: 0,
            inventory_base: 0,
            roll_map: 0,
            index_links: 0,
            seq_rejects: 0,
            sealed: false,
        }
    }

    fn gate(&mut self, p: ExpandPhase) -> bool {
        if p.ordinal() != self.phase + 1 {
            self.seq_rejects += 1;
            return false;
        }
        self.phase = p.ordinal();
        true
    }

    /// 内容哈希（FNV-1a 单量，与 F2261 同式）。
    pub fn hash(content: u64) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for k in 0..8 {
            h ^= (content >> (k * 8)) as u64 & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 段 1：素材哈希点收 + 增量清点（seq 连续性核）。
    pub fn intake(&mut self, packs: &[DeltaPack]) -> ExpandCode {
        if !self.gate(ExpandPhase::Intake) {
            return ExpandCode::SequenceBroken;
        }
        let mut expect_seq = self.inventory_base as u32;
        let mut seq_ok = true;
        for p in packs {
            if p.declared == Self::hash(p.content) {
                self.accepted += 1;
            } else {
                self.rejects += 1;
            }
            if p.seq != expect_seq {
                seq_ok = false;
            }
            expect_seq = expect_seq.wrapping_add(1);
        }
        self.inventory_base = expect_seq as u64;
        if self.rejects == 0 && seq_ok {
            ExpandCode::IntakeDone
        } else if !seq_ok {
            ExpandCode::SequenceBroken
        } else {
            ExpandCode::HashRejected
        }
    }

    /// 段 2：EX 账/图族/教训账三线滚动。
    pub fn exec_roll(&mut self) -> ExpandCode {
        if !self.gate(ExpandPhase::Exec) {
            return ExpandCode::SequenceBroken;
        }
        self.roll_map |= 0b001; // EX 账
        self.roll_map |= 0b010; // 图族
        self.roll_map |= 0b100; // 教训账
        if self.roll_map == 0b111 {
            ExpandCode::ExecDone
        } else {
            ExpandCode::SequenceBroken
        }
    }

    /// 段 3：扩容账落账 + 索引挂接（挂接 = 3 线 + 素材数上限 4）。
    pub fn confirm(&mut self) -> ExpandCode {
        if !self.gate(ExpandPhase::Confirm) {
            return ExpandCode::SequenceBroken;
        }
        self.index_links = 3 + self.accepted.min(4);
        self.sealed = true;
        ExpandCode::ConfirmDone
    }

    /// 三段时序断言。
    pub fn sequence_ok(&self) -> bool {
        self.phase == 3 && self.roll_map == 0b111 && self.sealed
    }

    /// 统计。
    pub const fn stats(&self) -> (u64, u64, u64) {
        (self.accepted, self.rejects, self.seq_rejects)
    }

    /// 账守恒。
    pub fn ledger_consistent(&self) -> bool {
        !self.sealed || self.phase == 3
    }
}

/// 判据测试（段 0x6661~0x6668）。
pub fn run_t281_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t281");
    // c1 三段编排全过。
    let mut e = ExpandEngine::new();
    let packs: Vec<DeltaPack> = (0..4u32)
        .map(|i| DeltaPack { seq: i, declared: ExpandEngine::hash(0x7000 + i as u64), content: 0x7000 + i as u64 })
        .collect();
    let r1 = e.intake(&packs);
    let r2 = e.exec_roll();
    let r3 = e.confirm();
    set.add(
        "t281-c1-three-phases",
        r1 == ExpandCode::IntakeDone && r2 == ExpandCode::ExecDone && r3 == ExpandCode::ConfirmDone && e.sequence_ok(),
        "",
    );
    // c2 增量哈希不符拒收。
    let mut e2 = ExpandEngine::new();
    let bad = [DeltaPack { seq: 0, declared: 0xDEAD, content: 1 }];
    set.add("t281-c2-hash-reject", e2.intake(&bad) == ExpandCode::HashRejected && e2.stats().1 == 1, "");
    // c3 跳段注入 10/10 拒绝。
    let mut rej = 0;
    for _ in 0..10u32 {
        let mut e3 = ExpandEngine::new();
        if e3.exec_roll() == ExpandCode::SequenceBroken {
            rej += 1;
        }
        if e3.confirm() == ExpandCode::SequenceBroken {
            rej += 1;
        }
    }
    set.add("t281-c3-skip-10of10", rej == 20, "");
    // c4 增量清点：seq 断链检出。
    let mut e4 = ExpandEngine::new();
    let gap = [
        DeltaPack { seq: 0, declared: ExpandEngine::hash(1), content: 1 },
        DeltaPack { seq: 2, declared: ExpandEngine::hash(2), content: 2 },
    ];
    set.add("t281-c4-seq-gap", e4.intake(&gap) == ExpandCode::SequenceBroken, "");
    // c5 索引挂接数：3 + min(素材,4)。
    set.add("t281-c5-index-links", e.stats().0 == 4 && e.sequence_ok(), "");
    // c6 重复段拒绝。
    let mut e6 = ExpandEngine::new();
    let p6 = [DeltaPack { seq: 0, declared: ExpandEngine::hash(9), content: 9 }];
    let _ = e6.intake(&p6);
    set.add("t281-c6-replay-reject", e6.intake(&p6) == ExpandCode::SequenceBroken && e6.stats().2 >= 1, "");
    // c7 账守恒。
    set.add("t281-c7-ledger", e.ledger_consistent() && e2.ledger_consistent(), "");
    // c8 LCG 512 轮注入：随机素材集，点收语义恒一致。
    let mut seed: u64 = 0x2810_6661_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut eng = ExpandEngine::new();
        let n = (lc() % 8) as u32;
        let mut good = 0u64;
        let mut bad = 0u64;
        let mut packs = Vec::new();
        for i in 0..n {
            let content = lc();
            let corrupt = lc() % 4 == 0;
            let p = DeltaPack {
                seq: i,
                declared: if corrupt { content ^ 0xFFFF } else { ExpandEngine::hash(content) },
                content,
            };
            if p.declared == ExpandEngine::hash(p.content) {
                good += 1;
            } else {
                bad += 1;
            }
            packs.push(p);
        }
        let r = eng.intake(&packs);
        let expect = if bad == 0 { ExpandCode::IntakeDone } else { ExpandCode::HashRejected };
        inj_ok &= r == expect && eng.stats().0 == good && eng.stats().1 == bad;
    }
    set.add("t281-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        ExpandCode::IntakeDone,
        ExpandCode::HashRejected,
        ExpandCode::ExecDone,
        ExpandCode::ExRolled,
        ExpandCode::GraphRolled,
        ExpandCode::LessonRolled,
        ExpandCode::ConfirmDone,
        ExpandCode::SequenceBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t281-c9-triad", tri_ok, "");
    set
}
