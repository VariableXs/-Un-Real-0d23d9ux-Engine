//! UNX-F2321 · 交接收官轴总控（联签编排三段）
//!
//! 诊断段 0x6651~0x6658（收官族联签段）。
//! 判据（施工书 J1）：联签三段编排全过（准备段：四接口签名与五户清单就绪核验；联签段：逐域逐件 fake 对拍；确认段：总账落账与保留项闭环），三段时序断言全过，跳段注入 10/10 拒绝。

use alloc::vec::Vec;

/// 联签诊断码闭集（0x6651~0x6658 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsignCode {
    /// 0x6651 准备段完成（四签名五户齐）。
    ReadyDone = 0x6651,
    /// 0x6652 接口签名缺失。
    SigMissing = 0x6652,
    /// 0x6653 户清单缺失。
    HouseMissing = 0x6653,
    /// 0x6654 联签段完成（逐域逐件对拍齐）。
    SignDone = 0x6654,
    /// 0x6655 fake 对拍不符。
    FakeMismatch = 0x6655,
    /// 0x6656 确认段完成（总账落+保留项闭环）。
    ConfirmDone = 0x6656,
    /// 0x6657 保留项未闭环。
    HoldoverOpen = 0x6657,
    /// 0x6658 跳段/账守恒破坏。
    SequenceBroken = 0x6658,
}

impl ConsignCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            ConsignCode::ReadyDone => "准备段就绪核验完成",
            ConsignCode::SigMissing => "四接口签名存在缺失",
            ConsignCode::HouseMissing => "五户清单存在缺失",
            ConsignCode::SignDone => "联签段逐域逐件对拍完成",
            ConsignCode::FakeMismatch => "fake 对拍不符",
            ConsignCode::ConfirmDone => "确认段总账落账完成",
            ConsignCode::HoldoverOpen => "保留项未闭环",
            ConsignCode::SequenceBroken => "跳段或账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            ConsignCode::ReadyDone => "四签名五户全就位",
            ConsignCode::SigMissing => "签名注册表不全",
            ConsignCode::HouseMissing => "户清单不全",
            ConsignCode::SignDone => "全部交付件 fake 双口径一致",
            ConsignCode::FakeMismatch => "交付件 fake 口径漂移",
            ConsignCode::ConfirmDone => "总账与保留项全部闭合",
            ConsignCode::HoldoverOpen => "保留项清单仍有开口",
            ConsignCode::SequenceBroken => "段序违规或账目不一致",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            ConsignCode::ReadyDone => "进入联签段",
            ConsignCode::SigMissing => "补齐接口签名",
            ConsignCode::HouseMissing => "补齐五户清单",
            ConsignCode::SignDone => "进入确认段",
            ConsignCode::FakeMismatch => "修交付件至双口径一致",
            ConsignCode::ConfirmDone => "收官轴闭环",
            ConsignCode::HoldoverOpen => "闭合保留项后重确认",
            ConsignCode::SequenceBroken => "按三段序重走",
        }
    }
}

/// 四接口签名闭集。
pub const INTERFACES: [&str; 4] = ["syscall.entry", "ve.render", "cgpu.submit", "unx.consult"];

/// 五户清单闭集。
pub const HOUSES: [&str; 5] = ["ktest", "shell", "daemon", "ve-app", "cgpu-app"];

/// 联签三段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsignPhase {
    /// 1 准备。
    Ready,
    /// 2 联签。
    Sign,
    /// 3 确认。
    Confirm,
}

impl ConsignPhase {
    /// 序号。
    pub const fn ordinal(self) -> u8 {
        match self {
            ConsignPhase::Ready => 1,
            ConsignPhase::Sign => 2,
            ConsignPhase::Confirm => 3,
        }
    }
}

/// 交付件对拍单元：域 + 件 + 双口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FakePair {
    /// 域号。
    pub domain: u8,
    /// 件号。
    pub item: u32,
    /// 实接口口径。
    pub real: u64,
    /// fake 口径。
    pub fake: u64,
}

impl FakePair {
    /// 对拍一致核。
    pub const fn matches(&self) -> bool {
        self.real == self.fake
    }
}

/// 联签总控引擎。
pub struct ConsignEngine {
    /// 当前段。
    phase: u8,
    /// 签名注册位图。
    sig_map: u8,
    /// 户清单位图。
    house_map: u8,
    /// 对拍通过件数。
    signed_items: u64,
    /// 对拍不符件数。
    mismatches: u64,
    /// 保留项开口数。
    holdovers: u64,
    /// 跳段拒绝数。
    seq_rejects: u64,
    /// 总账落账。
    sealed: bool,
}

impl ConsignEngine {
    /// 新引擎。
    pub fn new() -> Self {
        ConsignEngine {
            phase: 0,
            sig_map: 0,
            house_map: 0,
            signed_items: 0,
            mismatches: 0,
            holdovers: 0,
            seq_rejects: 0,
            sealed: false,
        }
    }

    fn gate(&mut self, p: ConsignPhase) -> bool {
        if p.ordinal() != self.phase + 1 {
            self.seq_rejects += 1;
            return false;
        }
        self.phase = p.ordinal();
        true
    }

    /// 段 1：签名与户清单就绪核验。
    pub fn ready(&mut self, sigs: &[&str], houses: &[&str]) -> ConsignCode {
        if !self.gate(ConsignPhase::Ready) {
            return ConsignCode::SequenceBroken;
        }
        for (i, want) in INTERFACES.iter().enumerate() {
            if sigs.contains(want) {
                self.sig_map |= 1 << i;
            }
        }
        for (i, want) in HOUSES.iter().enumerate() {
            if houses.contains(want) {
                self.house_map |= 1 << i;
            }
        }
        if self.sig_map == 0b1111 && self.house_map == 0b11111 {
            ConsignCode::ReadyDone
        } else if self.sig_map != 0b1111 {
            ConsignCode::SigMissing
        } else {
            ConsignCode::HouseMissing
        }
    }

    /// 段 2：逐域逐件 fake 对拍。
    pub fn sign(&mut self, pairs: &[FakePair]) -> ConsignCode {
        if !self.gate(ConsignPhase::Sign) {
            return ConsignCode::SequenceBroken;
        }
        for p in pairs {
            if p.matches() {
                self.signed_items += 1;
            } else {
                self.mismatches += 1;
            }
        }
        if self.mismatches == 0 {
            ConsignCode::SignDone
        } else {
            ConsignCode::FakeMismatch
        }
    }

    /// 段 3：总账落账 + 保留项闭环核。
    pub fn confirm(&mut self, holdovers_open: u64) -> ConsignCode {
        if !self.gate(ConsignPhase::Confirm) {
            return ConsignCode::SequenceBroken;
        }
        self.holdovers = holdovers_open;
        if holdovers_open == 0 {
            self.sealed = true;
            ConsignCode::ConfirmDone
        } else {
            ConsignCode::HoldoverOpen
        }
    }

    /// 三段时序断言。
    pub fn sequence_ok(&self) -> bool {
        self.phase == 3 && self.sealed && self.sig_map == 0b1111 && self.house_map == 0b11111
    }

    /// 统计。
    pub const fn stats(&self) -> (u64, u64, u64) {
        (self.signed_items, self.mismatches, self.seq_rejects)
    }

    /// 账守恒。
    pub fn ledger_consistent(&self) -> bool {
        !self.sealed || (self.phase == 3 && self.mismatches == 0 && self.holdovers == 0)
    }
}

/// 判据测试（段 0x6651~0x6658）。
pub fn run_t321_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t321");
    // c1 三段编排全过。
    let mut e = ConsignEngine::new();
    let r1 = e.ready(&INTERFACES, &HOUSES);
    let pairs: Vec<FakePair> = (0..8)
        .map(|i: u64| FakePair { domain: (i % 3) as u8, item: i as u32, real: 0x5000 + i, fake: 0x5000 + i })
        .collect();
    let r2 = e.sign(&pairs);
    let r3 = e.confirm(0);
    set.add(
        "t321-c1-three-phases",
        r1 == ConsignCode::ReadyDone && r2 == ConsignCode::SignDone && r3 == ConsignCode::ConfirmDone && e.sequence_ok(),
        "",
    );
    // c2 签名缺失检出。
    let mut e2 = ConsignEngine::new();
    let r = e2.ready(&["syscall.entry", "ve.render"], &HOUSES);
    set.add("t321-c2-sig-missing", r == ConsignCode::SigMissing, "");
    // c3 户清单缺失检出。
    let mut e3 = ConsignEngine::new();
    let r3 = e3.ready(&INTERFACES, &["ktest", "shell"]);
    set.add("t321-c3-house-missing", r3 == ConsignCode::HouseMissing, "");
    // c4 fake 对拍不符。
    let mut e4 = ConsignEngine::new();
    let _ = e4.ready(&INTERFACES, &HOUSES);
    let bad = [FakePair { domain: 1, item: 1, real: 0xA, fake: 0xB }];
    set.add("t321-c4-fake-mismatch", e4.sign(&bad) == ConsignCode::FakeMismatch && e4.stats().1 == 1, "");
    // c5 保留项未闭环 → 确认拒。
    let mut e5 = ConsignEngine::new();
    let _ = e5.ready(&INTERFACES, &HOUSES);
    let _ = e5.sign(&[]);
    set.add("t321-c5-holdover-open", e5.confirm(3) == ConsignCode::HoldoverOpen, "");
    // c6 跳段注入 10/10 拒绝。
    let mut rej = 0;
    for _ in 0..10u32 {
        let mut e6 = ConsignEngine::new();
        if e6.sign(&[]) == ConsignCode::SequenceBroken {
            rej += 1;
        }
        if e6.confirm(0) == ConsignCode::SequenceBroken {
            rej += 1;
        }
    }
    set.add("t321-c6-skip-10of10", rej == 20, "");
    // c7 账守恒。
    set.add("t321-c7-ledger", e.ledger_consistent() && e.ledger_consistent(), "");
    // c8 LCG 512 轮注入：随机交付件集，对拍语义恒一致。
    let mut seed: u64 = 0x3210_6651_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut eng = ConsignEngine::new();
        let _ = eng.ready(&INTERFACES, &HOUSES);
        let n = (lc() % 12) as usize;
        let mut good = 0;
        let mut bad = 0;
        let mut pairs = Vec::new();
        for i in 0..n {
            let v = lc();
            let corrupt = lc() % 4 == 0;
            let p = FakePair {
                domain: (lc() % 5) as u8,
                item: i as u32,
                real: v,
                fake: if corrupt { v ^ 0xFF } else { v },
            };
            if p.matches() {
                good += 1;
            } else {
                bad += 1;
            }
            pairs.push(p);
        }
        let r = eng.sign(&pairs);
        let expect = if bad == 0 { ConsignCode::SignDone } else { ConsignCode::FakeMismatch };
        inj_ok &= r == expect && eng.stats().0 == good as u64 && eng.stats().1 == bad as u64;
    }
    set.add("t321-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        ConsignCode::ReadyDone,
        ConsignCode::SigMissing,
        ConsignCode::HouseMissing,
        ConsignCode::SignDone,
        ConsignCode::FakeMismatch,
        ConsignCode::ConfirmDone,
        ConsignCode::HoldoverOpen,
        ConsignCode::SequenceBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t321-c9-triad", tri_ok, "");
    set
}
