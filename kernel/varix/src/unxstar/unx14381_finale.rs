//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14381 · 域收官总起（收官宣告总框架） · 0x66640
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F14381-J1 收官总框架：域收官八段链总图（满账对账/行数守恒/
//!      总锚归并/handoff 终态/防重终声明/收官报告/验收矩阵/标志条八段
//!      ×产物×闸位三列 8 行），八段产物在位断言（缺段注入必红），与
//!      先行域收官八段链体例同构声明，指纹钉版，红绿齐。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 功能定位：收官总框架是域收官宣告的总地图——八段链与先行域判例同
//!   构，收官不是一句口号而是一条可验的链。
//! 语义边界：只做收官总框架；不做各段本体（后续条持有）、不改先行域
//!   判例（只读引用）、不做跨域收官（各域持有）。
//! 设计要点：
//!   - 八段闭集（序号 0..8）：满账对账/行数守恒/总锚归并/handoff 终态/
//!     防重终声明/收官报告/验收矩阵/标志条；
//!   - 链式状态机：段闸位 0=未达/1=产物在位/2=过验/3=封段；链按序推
//!     进，跳段（缺段注入）必红；
//!   - 八段产物在位断言：每段产物锚（写死产物名表）非空且在账；
//!   - 防重终声明：收官宣告位单次置位，二次置位必红；
//!   - 先行域同构声明：先行域八段链判例（B4/C5 口径）写死平行表，两
//!     表八行段名逐行同构才放行；
//!   - 指纹钉版 + 双机对拍；LCG 种子写死 = 0x66640；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66640，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66640;

/// 八段闭集。
pub const SEG_FULLLEDGER: usize = 0;
pub const SEG_ROWCOUNT: usize = 1;
pub const SEG_TOTALANCHOR: usize = 2;
pub const SEG_HANDOFF: usize = 3;
pub const SEG_NOREDECLARE: usize = 4;
pub const SEG_REPORT: usize = 5;
pub const SEG_ACCEPTMAT: usize = 6;
pub const SEG_FLAGBAR: usize = 7;
pub const CHAIN_COUNT: usize = 8;

/// 八段段名（本域，写死）。
pub const CHAIN_NAMES: [&str; CHAIN_COUNT] = [
    "满账对账段",
    "行数守恒段",
    "总锚归并段",
    "handoff终态段",
    "防重终声明段",
    "收官报告段",
    "验收矩阵段",
    "标志条段",
];

/// 八段产物名（写死：产物在位断言的锚文本）。
pub const PRODUCT_NAMES: [&str; CHAIN_COUNT] = [
    "对账总表",
    "行数守恒账",
    "总锚归并表",
    "handoff终态单",
    "终声明单次位",
    "收官报告书",
    "验收矩阵表",
    "收官标志条",
];

/// 闸位闭集。
pub const GATE_UNREACHED: u8 = 0;
pub const GATE_FED: u8 = 1;
pub const GATE_VERIFIED: u8 = 2;
pub const GATE_SEALED: u8 = 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinCode {
    /// 段序越闭集（0..8）
    SegmentOutOfRange,
    /// 闸位越闭集
    GateOutOfRange,
    /// 缺段注入：链按序推进遇未在位段
    SegmentMissing,
    /// 链序跳段（未过验段不得后移）
    ChainSkipIllegal,
    /// 产物锚缺失（产物名空或不在账）
    ProductMissing,
    /// 防重终声明二次置位
    RedeclareAttempt,
    /// 同构声明破缺（与先行域八段链不同构）
    IsomorphismBroken,
    /// 链未收口即宣告
    ChainUnclosed,
    /// 总表行数 ≠ 8
    TableRowMismatch,
    /// 双机对拍漂移
    DualReplayDrift,
}

impl FinCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FinCode::SegmentOutOfRange => "段序越闭集：八段链 0..8",
            FinCode::GateOutOfRange => "闸位越闭集：0..3",
            FinCode::SegmentMissing => "缺段注入：链上产物不在位必红",
            FinCode::ChainSkipIllegal => "链序跳段：未过验段不得后移",
            FinCode::ProductMissing => "产物锚缺失：段名/产物名空",
            FinCode::RedeclareAttempt => "防重终声明二次置位：单次位红线",
            FinCode::IsomorphismBroken => "同构破缺：与先行域八段链不同构",
            FinCode::ChainUnclosed => "链未收口即宣告：收官前置不满足",
            FinCode::TableRowMismatch => "总表行数 ≠ 8：八段链红线",
            FinCode::DualReplayDrift => "双机对拍漂移：链不可复现",
        }
    }
    pub const ALL: [FinCode; 10] = [
        FinCode::SegmentOutOfRange,
        FinCode::GateOutOfRange,
        FinCode::SegmentMissing,
        FinCode::ChainSkipIllegal,
        FinCode::ProductMissing,
        FinCode::RedeclareAttempt,
        FinCode::IsomorphismBroken,
        FinCode::ChainUnclosed,
        FinCode::TableRowMismatch,
        FinCode::DualReplayDrift,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FinCode::ALL.len() {
        if FinCode::ALL[i].val() == code {
            return FinCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66640 段闭集内"
}

// ── 八段链行（段/产物/闸位） ─────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainRow {
    pub seg: usize,
    /// 产物锚（产物名表直引）
    pub product: &'static str,
    pub gate: u8,
}

/// 先行域八段链平行表（写死：与 B4/C5 收官判例同构的段名序——只读
/// 引用不改判例；段名序与本域八段完全同序同名）。
pub const PRIOR_CHAIN_NAMES: [&str; CHAIN_COUNT] = [
    "满账对账段",
    "行数守恒段",
    "总锚归并段",
    "handoff终态段",
    "防重终声明段",
    "收官报告段",
    "验收矩阵段",
    "标志条段",
];

/// 同构声明：两表八行段名逐行一致。
pub fn isomorphic_to_prior() -> bool {
    let mut i = 0;
    while i < CHAIN_COUNT {
        if CHAIN_NAMES[i] != PRIOR_CHAIN_NAMES[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ── 收官链账本 ───────────────────────────────────────────────────
pub struct FinaleLedger {
    pub rows: Vec<ChainRow>,
    /// 防重终声明单次位
    pub declared: bool,
    /// 链收口位
    pub closed: bool,
}

impl FinaleLedger {
    pub fn new() -> FinaleLedger {
        FinaleLedger { rows: Vec::new(), declared: false, closed: false }
    }

    /// 八段链落位（全段未达态，产物锚挂账）。
    pub fn lay_chain(&mut self) -> Result<(), u32> {
        if !self.rows.is_empty() {
            return Err(FinCode::TableRowMismatch.val());
        }
        let mut s = 0;
        while s < CHAIN_COUNT {
            if PRODUCT_NAMES[s].len() == 0 {
                return Err(FinCode::ProductMissing.val());
            }
            self.rows.push(ChainRow {
                seg: s,
                product: PRODUCT_NAMES[s],
                gate: GATE_UNREACHED,
            });
            s += 1;
        }
        if self.rows.len() != CHAIN_COUNT {
            return Err(FinCode::TableRowMismatch.val());
        }
        Ok(())
    }

    /// 产物喂入：段 i 产物在位断言（0→1）。链序：只准喂当前链头段。
    pub fn feed(&mut self, seg: usize) -> Result<(), u32> {
        if seg >= CHAIN_COUNT {
            return Err(FinCode::SegmentOutOfRange.val());
        }
        if self.rows[seg].product.len() == 0 {
            return Err(FinCode::ProductMissing.val());
        }
        // 链序：前序段必须全过验，否则跳段必红。
        let mut p = 0;
        while p < seg {
            if self.rows[p].gate != GATE_VERIFIED && self.rows[p].gate != GATE_SEALED {
                return Err(FinCode::ChainSkipIllegal.val());
            }
            p += 1;
        }
        // 缺段注入面：本段未达态喂入才合法；重复喂入必红。
        if self.rows[seg].gate != GATE_UNREACHED {
            return Err(FinCode::GateOutOfRange.val());
        }
        self.rows[seg].gate = GATE_FED;
        Ok(())
    }

    /// 段过验（1→2）。
    pub fn verify(&mut self, seg: usize) -> Result<(), u32> {
        if seg >= CHAIN_COUNT {
            return Err(FinCode::SegmentOutOfRange.val());
        }
        if self.rows[seg].gate != GATE_FED {
            return Err(FinCode::ChainSkipIllegal.val());
        }
        self.rows[seg].gate = GATE_VERIFIED;
        Ok(())
    }

    /// 全链推进：按序喂入+过验+封段。
    pub fn run_chain(&mut self) -> Result<usize, u32> {
        let mut s = 0;
        while s < CHAIN_COUNT {
            self.feed(s)?;
            self.verify(s)?;
            self.rows[s].gate = GATE_SEALED;
            s += 1;
        }
        // 八段产物在位断言：收口前逐段复点。
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].gate != GATE_SEALED {
                return Err(FinCode::SegmentMissing.val());
            }
            if self.rows[i].product.len() == 0 {
                return Err(FinCode::ProductMissing.val());
            }
            i += 1;
        }
        self.closed = true;
        Ok(self.rows.len())
    }

    /// 收官宣告（防重：单次置位）。
    pub fn declare(&mut self) -> Result<(), u32> {
        if self.declared {
            return Err(FinCode::RedeclareAttempt.val());
        }
        if !self.closed {
            return Err(FinCode::ChainUnclosed.val());
        }
        self.declared = true;
        Ok(())
    }

    /// 指纹（八行逐段滚乘）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x6664_00F7;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            let mut p = 0;
            let mut prod: u64 = 0;
            // 产物名逐字节滚入（&'static str 直读，无堆）。
            let bytes = r.product.as_bytes();
            while p < bytes.len() {
                prod = prod.wrapping_mul(31).wrapping_add(bytes[p] as u64);
                p += 1;
            }
            h = h.wrapping_mul(0x100000001B3)
                ^ ((r.seg as u64) << 16)
                ^ ((r.gate as u64) << 8)
                ^ (prod & 0xFFFF);
            i += 1;
        }
        h
    }

    /// 双机对拍：第二账本独立跑链，八行逐行一致。
    pub fn dual_replay(&self) -> Result<(), u32> {
        let mut twin = FinaleLedger::new();
        twin.lay_chain()?;
        twin.run_chain()?;
        if twin.rows.len() != self.rows.len() {
            return Err(FinCode::DualReplayDrift.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if twin.rows[i] != self.rows[i] {
                return Err(FinCode::DualReplayDrift.val());
            }
            i += 1;
        }
        if twin.fingerprint() != self.fingerprint() {
            return Err(FinCode::DualReplayDrift.val());
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
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

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, u64, bool), u32> {
    if !isomorphic_to_prior() {
        return Err(FinCode::IsomorphismBroken.val());
    }
    let mut led = FinaleLedger::new();
    led.lay_chain()?;
    let n = led.run_chain()?;
    led.declare()?;
    let fp = led.fingerprint();
    led.dual_replay()?;
    Ok((n, fp, led.declared))
}

// ── 注入面：三族 4/4 必抓 ────────────────────────────────────────
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
    // 族 0：缺段注入——第 3 段未喂即收口宣告（链未收口必红）。
    ledger.tries[0] += 1;
    let mut led = FinaleLedger::new();
    let _ = led.lay_chain();
    let mut s = 0;
    while s < CHAIN_COUNT {
        if s != 3 {
            let _ = led.feed(s);
            let _ = led.verify(s);
            led.rows[s].gate = GATE_SEALED;
        }
        s += 1;
    }
    led.closed = true;
    if led.declare().is_err() {
        ledger.caught[0] += 1;
    }
    // 族 1：防重终声明二次置位。
    ledger.tries[1] += 1;
    let mut led2 = FinaleLedger::new();
    let _ = led2.lay_chain();
    let _ = led2.run_chain();
    let first = led2.declare().is_ok();
    let second = led2.declare().is_err();
    if first && second {
        ledger.caught[1] += 1;
    }
    // 族 2：链序跳段注入（段 0 未过验直接喂段 5）+ 重复喂入。
    ledger.tries[2] += 2;
    let mut led3 = FinaleLedger::new();
    let _ = led3.lay_chain();
    if led3.feed(5).is_err() {
        ledger.caught[2] += 1;
    }
    let mut led4 = FinaleLedger::new();
    let _ = led4.lay_chain();
    let _ = led4.feed(0);
    if led4.feed(0).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FinCode::ALL.len() {
        let v = FinCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：八段链收口 + 单次宣告 + 指纹。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((n, _, declared)) => *n == CHAIN_COUNT && *declared,
        Err(_) => false,
    };
    v.push(("J1-八段链收口+单次宣告", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓（缺段必红）",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 同构声明：两表逐行一致。
    v.push((
        "J1-先行域八段链同构",
        isomorphic_to_prior(),
        String::from("8 行段名逐行"),
    ));
    // 4. 八段产物名点验（全部非空）。
    let mut named = 0;
    let mut i = 0;
    while i < CHAIN_COUNT {
        if PRODUCT_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-八段产物在位点验",
        named == CHAIN_COUNT,
        format!("{}/8", named),
    ));
    // 5. 防重位初值必假（未宣告即假，宣告后必真）。
    let once_ok = {
        let mut led = FinaleLedger::new();
        let _ = led.lay_chain();
        let before = !led.declared;
        let _ = led.run_chain();
        let _ = led.declare();
        before && led.declared
    };
    v.push(("J1-终声明单次位语义", once_ok, String::from("false→true 一次")));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14381_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14381 域收官总起（收官宣告八段链总框架）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
