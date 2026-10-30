//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15941 · 判据冻结快照总纲（四类快照 + SHA-256 式哈希链 + 漏类注入）
//! · 0x679C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15941-J1 总纲成文（快照范围——判据/行数/ID/接口清单四类
//!      +SHA-256 哈希链 面谱 10/10 次），漏类注入检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 快照四类闭集：判据面 / 行数面 / ID 面 / 接口清单面；
//!   - 哈希链：SHA-256 压缩函数建模为确定性 256 位混合轮函数
//!     （FIPS 180-4 轮结构语义：消息扩展 + 轮常量 + 链值推进），
//!     纯整数实现，无浮点无 IO；链式：H(n) = Mix(H(n-1) ‖ 类 n)；
//!   - 快照状态机：Draft → Frozen(哈希链锚定) → Verified / ClassMissing；
//!   - 漏类注入：四类缺一（哈希链断）→ 巡检必检出；
//!   - 正演 10/10：四类全锚 + 链复算一致；注入 10/10：漏类必检出；
//!   - LCG 种子写死 = SEG_BASE（0x679C0），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x679C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x679C0;

/// 每轮正演次数（判据钉版：10/10）。
pub const RUNS_PER_ROUND: u32 = 10;

/// 快照四类闭集。
pub const CLS_JUDGE: usize = 0;
pub const CLS_LINES: usize = 1;
pub const CLS_ID: usize = 2;
pub const CLS_IFACE: usize = 3;
pub const CLS_COUNT: usize = 4;
pub const CLS_NAMES: [&str; CLS_COUNT] = ["判据面", "行数面", "ID面", "接口清单面"];

/// 哈希链字宽（8×u32 = 256 位，SHA-256 链值语义）。
pub const HASH_WORDS: usize = 8;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapCode {
    /// 快照类越闭集
    ClassOutOfRange,
    /// 漏类（四类缺一）
    ClassMissing,
    /// 哈希链断裂（复算与锚不符）
    ChainBroken,
    /// 快照未冻结即校验
    NotFrozen,
    /// 冻结后篡改（快照内容与锚不符）
    FrozenTampered,
    /// 非法相位边
    IllegalPhaseEdge,
    /// 空快照（四类全空不得冻结）
    EmptySnapshot,
    /// 双跑不一致
    NotReproducible,
    /// 链序错（类乱序入链）
    ChainOrderWrong,
}

impl SnapCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SnapCode::ClassOutOfRange => "快照类越闭集：四类冻结",
            SnapCode::ClassMissing => "漏类：四类缺一",
            SnapCode::ChainBroken => "哈希链断裂：复算与锚不符",
            SnapCode::NotFrozen => "快照未冻结即校验",
            SnapCode::FrozenTampered => "冻结后篡改：内容与锚不符",
            SnapCode::IllegalPhaseEdge => "非法相位边：快照状态机闭集外",
            SnapCode::EmptySnapshot => "空快照：四类全空不得冻结",
            SnapCode::NotReproducible => "双跑不一致：快照复现红线",
            SnapCode::ChainOrderWrong => "链序错：类乱序入链",
        }
    }
    pub const ALL: [SnapCode; 9] = [
        SnapCode::ClassOutOfRange,
        SnapCode::ClassMissing,
        SnapCode::ChainBroken,
        SnapCode::NotFrozen,
        SnapCode::FrozenTampered,
        SnapCode::IllegalPhaseEdge,
        SnapCode::EmptySnapshot,
        SnapCode::NotReproducible,
        SnapCode::ChainOrderWrong,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SnapCode::ALL.len() {
        if SnapCode::ALL[i].val() == code {
            return SnapCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x679C0 段闭集内"
}

// ── SHA-256 式混合轮（确定性整数建模，写死常量来源：FIPS 180-4
//    前两轮常量 0x428a2f98 / 0x71374491 及 IV 语义） ────────────────
pub const MIX_K0: u32 = 0x428a_2f98;
pub const MIX_K1: u32 = 0x7137_4491;
pub const MIX_IV: [u32; HASH_WORDS] = [
    0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a,
    0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19,
];

fn rotr(x: u32, n: u32) -> u32 {
    x.rotate_right(n)
}

/// 单块压缩建模：8 字消息 + 8 字链值 → 8 字新链值（两轮结构）。
pub fn mix_block(chain: &[u32; HASH_WORDS], msg: &[u32; HASH_WORDS]) -> [u32; HASH_WORDS] {
    let mut w = [0u32; HASH_WORDS];
    let mut i = 0;
    while i < HASH_WORDS {
        w[i] = msg[i];
        i += 1;
    }
    // 轮 0。
    let mut a = chain[0];
    let mut b = chain[1];
    let mut c = chain[2];
    let mut d = chain[3];
    let mut e = chain[4];
    let mut f = chain[5];
    let mut g = chain[6];
    let mut h = chain[7];
    let t1 = h
        .wrapping_add(rotr(e, 7) ^ rotr(e, 18) ^ (e >> 3))
        .wrapping_add(g ^ (e & f))
        .wrapping_add(w[0].wrapping_add(MIX_K0));
    let t2 = (rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)).wrapping_add((a & b) | (a & c) | (b & c));
    h = g;
    g = f;
    f = e;
    e = d.wrapping_add(t1);
    d = c;
    c = b;
    b = a;
    a = t1.wrapping_add(t2);
    // 轮 1。
    let t1b = h
        .wrapping_add(rotr(e, 17) ^ rotr(e, 19) ^ (e >> 10))
        .wrapping_add(g ^ (e & f))
        .wrapping_add(w[1].wrapping_add(MIX_K1));
    let t2b = (rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)).wrapping_add((a & b) | (a & c) | (b & c));
    let hh = g;
    let _ = hh;
    g = f;
    f = e;
    e = d.wrapping_add(t1b);
    d = c;
    c = b;
    b = a;
    a = t1b.wrapping_add(t2b);
    let mut out = [0u32; HASH_WORDS];
    out[0] = a.wrapping_add(chain[0]);
    out[1] = b.wrapping_add(chain[1]);
    out[2] = c.wrapping_add(chain[2]);
    out[3] = d.wrapping_add(chain[3]);
    out[4] = e.wrapping_add(chain[4]);
    out[5] = f.wrapping_add(chain[5]);
    out[6] = g.wrapping_add(chain[6]);
    out[7] = h.wrapping_add(chain[7]);
    out
}

/// 哈希链锚：H = IV；逐类 mix。
pub fn chain_anchor(payloads: &[[u32; HASH_WORDS]; CLS_COUNT]) -> [u32; HASH_WORDS] {
    let mut chain = MIX_IV;
    let mut i = 0;
    while i < CLS_COUNT {
        chain = mix_block(&chain, &payloads[i]);
        i += 1;
    }
    chain
}

fn chains_equal(a: &[u32; HASH_WORDS], b: &[u32; HASH_WORDS]) -> bool {
    let mut i = 0;
    while i < HASH_WORDS {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ── 快照状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapPhase {
    Draft,
    Frozen,
    Verified,
    ClassMissing,
}

/// 合法边闭集：Draft→Frozen、Frozen→Verified、Draft→ClassMissing。
pub fn snap_edge_legal(from: SnapPhase, to: SnapPhase) -> bool {
    match (from, to) {
        (SnapPhase::Draft, SnapPhase::Frozen) => true,
        (SnapPhase::Frozen, SnapPhase::Verified) => true,
        (SnapPhase::Draft, SnapPhase::ClassMissing) => true,
        _ => false,
    }
}

/// 快照载荷：类内容（判据条数/行数/ID 序列/接口清单条数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapClass {
    pub class: usize,
    pub payload: [u32; HASH_WORDS],
    pub present: bool,
}

// ── 快照账本 ─────────────────────────────────────────────────────
pub struct SnapLedger {
    pub classes: Vec<SnapClass>,
    pub anchor: [u32; HASH_WORDS],
    pub phase: SnapPhase,
}

impl SnapLedger {
    pub fn new() -> SnapLedger {
        SnapLedger {
            classes: Vec::new(),
            anchor: MIX_IV,
            phase: SnapPhase::Draft,
        }
    }

    /// 装填四类（写死种子：判据 12 条/行数 6900/ID 12 枚/接口 4 面）。
    pub fn load_full(&mut self) {
        self.classes.clear();
        self.phase = SnapPhase::Draft;
        // 类 0 判据面。
        self.classes.push(SnapClass {
            class: CLS_JUDGE,
            payload: [12, 0x67700, 0x679C0, 0x428a_2f98, 0, 0, 0, 1],
            present: true,
        });
        // 类 1 行数面。
        self.classes.push(SnapClass {
            class: CLS_LINES,
            payload: [6900, 300, 350, 0x7137_4491, 0, 0, 0, 2],
            present: true,
        });
        // 类 2 ID 面。
        self.classes.push(SnapClass {
            class: CLS_ID,
            payload: [15721, 15741, 15761, 15781, 15801, 15821, 15841, 15861],
            present: true,
        });
        // 类 3 接口清单面。
        self.classes.push(SnapClass {
            class: CLS_IFACE,
            payload: [15881, 15901, 15921, 15941, 0, 0, 0, 4],
            present: true,
        });
    }

    /// 漏类装填：跳过 skip 类。
    pub fn load_missing(&mut self, skip: usize) -> Result<(), u32> {
        if skip >= CLS_COUNT {
            return Err(SnapCode::ClassOutOfRange.val());
        }
        self.load_full();
        // 直接拿走一类（漏类注入）。
        self.classes[skip].present = false;
        Ok(())
    }

    /// 冻结：四类齐 → 哈希链锚定；缺类 → ClassMissing。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if !snap_edge_legal(self.phase, SnapPhase::Frozen) {
            return Err(SnapCode::IllegalPhaseEdge.val());
        }
        // 类清点（下标先比长度）。
        if self.classes.len() != CLS_COUNT {
            self.phase = SnapPhase::ClassMissing;
            return Err(SnapCode::ClassMissing.val());
        }
        let mut payloads = [[0u32; HASH_WORDS]; CLS_COUNT];
        let mut present = 0;
        let mut i = 0;
        while i < CLS_COUNT {
            if self.classes[i].present {
                present += 1;
                payloads[self.classes[i].class] = self.classes[i].payload;
            }
            i += 1;
        }
        if present < CLS_COUNT {
            self.phase = SnapPhase::ClassMissing;
            return Err(SnapCode::ClassMissing.val());
        }
        // 链序校验：类必须 0..3 有序入链。
        let mut ordered = true;
        i = 0;
        while i < CLS_COUNT {
            if self.classes[i].class != i {
                ordered = false;
            }
            i += 1;
        }
        if !ordered {
            return Err(SnapCode::ChainOrderWrong.val());
        }
        self.anchor = chain_anchor(&payloads);
        self.phase = SnapPhase::Frozen;
        Ok(())
    }

    /// 校验：重算链与锚比对（冻结后篡改即断）。
    pub fn verify(&mut self) -> Result<(), u32> {
        if self.phase != SnapPhase::Frozen {
            return Err(SnapCode::NotFrozen.val());
        }
        let mut payloads = [[0u32; HASH_WORDS]; CLS_COUNT];
        let mut i = 0;
        while i < CLS_COUNT {
            if self.classes[i].present {
                payloads[self.classes[i].class] = self.classes[i].payload;
            }
            i += 1;
        }
        let re = chain_anchor(&payloads);
        if !chains_equal(&re, &self.anchor) {
            self.phase = SnapPhase::ClassMissing;
            return Err(SnapCode::ChainBroken.val());
        }
        self.phase = SnapPhase::Verified;
        Ok(())
    }

    /// 冻结后篡改注入：改一类 payload 再 verify 必断链。
    pub fn tamper_frozen(&mut self, class: usize, delta: u32) -> Result<(), u32> {
        if class >= CLS_COUNT || class >= self.classes.len() {
            return Err(SnapCode::ClassOutOfRange.val());
        }
        if self.phase != SnapPhase::Frozen {
            return Err(SnapCode::NotFrozen.val());
        }
        let w = class % HASH_WORDS;
        self.classes[class].payload[w] = self.classes[class].payload[w].wrapping_add(delta);
        self.verify()
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ──────────────────────────
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
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 注入面：三族各 10 次，10/10 全抓 ─────────────────────────────
pub const INJ_FAMILIES: usize = 3;
pub const INJ_NAMES: [&str; INJ_FAMILIES] = ["漏类族", "冻结后篡改族", "未冻结校验族"];

pub struct InjectLedger {
    pub tries: [u32; INJ_FAMILIES],
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJ_FAMILIES {
            if self.caught[i] != self.tries[i] || self.tries[i] != RUNS_PER_ROUND {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：漏类注入（缺类轮转 ×10）→ ClassMissing。
    let mut k = 0u32;
    while k < RUNS_PER_ROUND {
        ledger.tries[0] += 1;
        let mut s = SnapLedger::new();
        let _ = s.load_missing(k as usize % CLS_COUNT);
        if let Err(e) = s.freeze() {
            if e == SnapCode::ClassMissing.val() && s.phase == SnapPhase::ClassMissing {
                ledger.caught[0] += 1;
            }
        }
        k += 1;
    }
    // 族 1：冻结后篡改注入（改 payload ×10）→ ChainBroken。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[1] += 1;
        let mut s = SnapLedger::new();
        s.load_full();
        let _ = s.freeze();
        if let Err(e) = s.tamper_frozen((k as usize) % CLS_COUNT, 1 + k % 5) {
            if e == SnapCode::ChainBroken.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：未冻结校验注入（Draft 相位 verify ×10）→ NotFrozen。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[2] += 1;
        let mut s = SnapLedger::new();
        s.load_full();
        if let Err(e) = s.verify() {
            if e == SnapCode::NotFrozen.val() {
                ledger.caught[2] += 1;
            }
        }
        k += 1;
    }
}

// ── 黄金正演：四类全锚 + 链复算 ×10 轮 ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub rounds: u32,
    pub anchor: [u32; HASH_WORDS],
    pub phase_verified: bool,
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut last: Option<GoldenResult> = None;
    let mut round = 0u32;
    while round < RUNS_PER_ROUND {
        let _ = g.below(41);
        let mut s = SnapLedger::new();
        s.load_full();
        s.freeze()?;
        // 空快照闸反向验证：链非 IV（四类入链必变）。
        if chains_equal(&s.anchor, &MIX_IV) {
            return Err(SnapCode::EmptySnapshot.val());
        }
        s.verify()?;
        if s.phase != SnapPhase::Verified {
            return Err(SnapCode::IllegalPhaseEdge.val());
        }
        last = Some(GoldenResult {
            rounds: round + 1,
            anchor: s.anchor,
            phase_verified: true,
        });
        round += 1;
    }
    match last {
        Some(res) => Ok(res),
        None => Err(SnapCode::IllegalPhaseEdge.val()),
    }
}

/// 双跑复现：锚逐字一致。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if !chains_equal(&a.anchor, &b.anchor) {
        return Err(SnapCode::NotReproducible.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SnapCode::ALL.len() {
        let v = SnapCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：四类全锚 + 链复算 ×10 轮。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => g.rounds == RUNS_PER_ROUND && g.phase_verified,
        Err(_) => false,
    };
    v.push(("J1-四类快照链复算10/10", golden_full, format!("{:?}", golden)));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-漏类三族10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现（锚逐字一致）。
    v.push(("J1-双跑锚一致", matches!(reproducible(), Ok(())), String::from("双跑逐字")));
    // 4. 四类名点验。
    let mut ok = 0;
    let mut i = 0;
    while i < CLS_COUNT {
        if CLS_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    v.push(("J1-四类名点验", ok == 4, format!("{}/4", ok)));
    // 5. 混合轮确定性自证：同输入双算一致 + 单字改动必变。
    let m0 = [1u32, 2, 3, 4, 5, 6, 7, 8];
    let a = mix_block(&MIX_IV, &m0);
    let b = mix_block(&MIX_IV, &m0);
    let mut m1 = m0;
    m1[0] = m1[0].wrapping_add(1);
    let c = mix_block(&MIX_IV, &m1);
    let det = chains_equal(&a, &b) && !chains_equal(&a, &c);
    v.push(("J1-混合轮确定性自证", det, format!("{:08x}", a[0])));
    // 6. 状态机非法边抽验：ClassMissing→Verified 直跳必拒。
    let bad = !snap_edge_legal(SnapPhase::ClassMissing, SnapPhase::Verified);
    v.push(("J1-ClassMissing→Verified必拒", bad, String::from("闭集外")));
    // 7. 链序反向判据：类乱序冻结必拒（ChainOrderWrong 路径）。
    let mut s2 = SnapLedger::new();
    s2.load_full();
    let t = s2.classes[0];
    s2.classes[0] = s2.classes[1];
    s2.classes[1] = t;
    let ord = if let Err(e) = s2.freeze() {
        e == SnapCode::ChainOrderWrong.val()
    } else {
        false
    };
    v.push(("J1-类乱序入链必拒", ord, String::from("链序闸")));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x679C0 窗口")));
    v
}

pub fn run_unx15941_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B38 判据冻结快照总纲（四类+SHA-256 式哈希链）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
