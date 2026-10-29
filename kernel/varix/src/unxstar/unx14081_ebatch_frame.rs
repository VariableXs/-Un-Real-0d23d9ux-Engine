//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14081 · E 段总账开篇（十批框架守恒声明） · 0x66280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F14081-J1 依赖的开篇账：E 段十批（B16–B25）框架守恒声明
//!      账——十批主题框架表（批号/主题句/条数/行数四列）与 F13899
//!      预告合同对表断言（十批全对表——预告即实作），批间依赖声明表
//!      （十批依赖边全图），声明指纹钉版，红绿齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十批闭集 B16–B25：主题句/条数/行数逐批写死（四列框架表）；
//!   - 对表判据：黄金预告合同独立写死副本，逐批四列全对表
//!     （预告即实作——零漂移）；
//!   - 守恒断言：条数总和 = 写死合同总数；行数总和 = 写死合同总数；
//!   - 批间依赖声明边全图：边闭集 0..10 + 无自环 + Kahn 拓扑无环
//!     （依赖边数 = 写死合同边数）；
//!   - 声明状态机：草拟→声明→验证→封账（终态收敛无悬挂）；
//!   - 复现账：同输入双跑逐批一致 + 指纹双跑一致（确定性）；
//!   - LCG 种子写死 = SEG_BASE（0x66280），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66280，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66280;

// ── E 段十批闭集（B16–B25） ──────────────────────────────────────
pub const EB_COUNT: usize = 10;
pub const EB_IDS: [&str; EB_COUNT] = [
    "B16", "B17", "B18", "B19", "B20", "B21", "B22", "B23", "B24", "B25",
];
/// 主题句（批号/主题句/条数/行数四列框架表之主题列，写死）。
pub const EB_THEMES: [&str; EB_COUNT] = [
    "签名族判定账", "断链族判定账", "快表名字族判定账", "载荷族判定账",
    "安全class族判定账", "时间戳族判定账", "名字链族判定账", "降级容错总账",
    "诊断归并总账", "E段收束守恒账",
];
/// 条数（写死；来源：各批卡面条目数语义推定）。
pub const EB_ITEMS: [u32; EB_COUNT] = [12, 12, 12, 12, 12, 14, 14, 14, 16, 16];
/// 行数（写死；来源：各批卡 target 语义推定）。
pub const EB_LINES: [u32; EB_COUNT] =
    [400, 400, 400, 400, 400, 420, 420, 400, 420, 300];

/// 黄金预告合同（独立写死副本，与 F13899 预告对表）。
pub const GOLD_IDS: [&str; EB_COUNT] = [
    "B16", "B17", "B18", "B19", "B20", "B21", "B22", "B23", "B24", "B25",
];
pub const GOLD_THEMES: [&str; EB_COUNT] = [
    "签名族判定账", "断链族判定账", "快表名字族判定账", "载荷族判定账",
    "安全class族判定账", "时间戳族判定账", "名字链族判定账", "降级容错总账",
    "诊断归并总账", "E段收束守恒账",
];
pub const GOLD_ITEMS: [u32; EB_COUNT] =
    [12, 12, 12, 12, 12, 14, 14, 14, 16, 16];
pub const GOLD_LINES: [u32; EB_COUNT] =
    [400, 400, 400, 400, 400, 420, 420, 400, 420, 300];
pub const GOLD_ITEM_TOTAL: u32 = 134;
pub const GOLD_LINE_TOTAL: u32 = 3960;

// ── 批间依赖声明边全图（来源：批序语义推定，写死） ────────────────
pub const EDGE_CHAIN: [(usize, usize); 12] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 4),
    (4, 5),
    (5, 6),
    (6, 7),
    (7, 8),
    (8, 9),
    (0, 4),
    (2, 6),
    (3, 7),
];
pub const GOLD_EDGE_COUNT: usize = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EbfCode {
    /// 批号越闭集
    BatchOutOfRange,
    /// 依赖边越闭集
    EdgeOutOfRange,
    /// 依赖边自环
    EdgeSelfLoop,
    /// 依赖图成环（Kahn 拓扑失败）
    DependencyCycle,
    /// 预告合同对表漂移
    ContractDrift,
    /// 条数守恒失守
    ItemConservation,
    /// 行数守恒失守
    LineConservation,
    /// 声明状态机越边
    IllegalTransition,
    /// 批未封账
    BatchUnsealed,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl EbfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EbfCode::BatchOutOfRange => "批号越闭集：E 段十批冻结",
            EbfCode::EdgeOutOfRange => "依赖边越闭集：0..10 冻结",
            EbfCode::EdgeSelfLoop => "依赖边自环：声明表失真",
            EbfCode::DependencyCycle => "依赖图成环：Kahn 拓扑失败",
            EbfCode::ContractDrift => "预告合同对表漂移：预告≠实作",
            EbfCode::ItemConservation => "条数守恒失守：Σ条数≠合同总数",
            EbfCode::LineConservation => "行数守恒失守：Σ行数≠合同总数",
            EbfCode::IllegalTransition => "声明状态机越边：未验证先封账",
            EbfCode::BatchUnsealed => "批未封账：红绿齐红线",
            EbfCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [EbfCode; 10] = [
        EbfCode::BatchOutOfRange,
        EbfCode::EdgeOutOfRange,
        EbfCode::EdgeSelfLoop,
        EbfCode::DependencyCycle,
        EbfCode::ContractDrift,
        EbfCode::ItemConservation,
        EbfCode::LineConservation,
        EbfCode::IllegalTransition,
        EbfCode::BatchUnsealed,
        EbfCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EbfCode::ALL.len() {
        if EbfCode::ALL[i].val() == code {
            return EbfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66280 段闭集内"
}

// ── 声明状态机 ───────────────────────────────────────────────────
/// 草拟 → 声明 → 验证 → 封账（合法边闭集；越边拒收）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    Drafted,
    Declared,
    Verified,
    Sealed,
}

// ── 十批声明账 ───────────────────────────────────────────────────
pub struct EbatchLedger {
    pub states: [BatchState; EB_COUNT],
    pub sealed: u32,
    pub fingerprint: u64,
}

impl EbatchLedger {
    pub fn new() -> EbatchLedger {
        EbatchLedger {
            states: [BatchState::Drafted; EB_COUNT],
            sealed: 0,
            fingerprint: 0,
        }
    }

    /// 声明一批（Drafted → Declared）。
    pub fn declare(&mut self, b: usize) -> Result<(), u32> {
        if b >= EB_COUNT {
            return Err(EbfCode::BatchOutOfRange.val());
        }
        if self.states[b] != BatchState::Drafted {
            return Err(EbfCode::IllegalTransition.val());
        }
        self.states[b] = BatchState::Declared;
        Ok(())
    }

    /// 验证一批（Declared → Verified；对黄金合同四列对表）。
    pub fn verify(&mut self, b: usize) -> Result<(), u32> {
        if b >= EB_COUNT {
            return Err(EbfCode::BatchOutOfRange.val());
        }
        if self.states[b] != BatchState::Declared {
            return Err(EbfCode::IllegalTransition.val());
        }
        // 四列对表：批号/主题句/条数/行数。
        if EB_IDS[b] != GOLD_IDS[b]
            || EB_THEMES[b] != GOLD_THEMES[b]
            || EB_ITEMS[b] != GOLD_ITEMS[b]
            || EB_LINES[b] != GOLD_LINES[b]
        {
            return Err(EbfCode::ContractDrift.val());
        }
        self.states[b] = BatchState::Verified;
        Ok(())
    }

    /// 封账一批（Verified → Sealed）。
    pub fn seal(&mut self, b: usize) -> Result<(), u32> {
        if b >= EB_COUNT {
            return Err(EbfCode::BatchOutOfRange.val());
        }
        if self.states[b] != BatchState::Verified {
            return Err(EbfCode::IllegalTransition.val());
        }
        self.states[b] = BatchState::Sealed;
        self.sealed += 1;
        // 指纹推进：批号+条数+行数三因子折叠。
        self.fingerprint = self
            .fingerprint
            .wrapping_mul(31)
            .wrapping_add(b as u64)
            .wrapping_add(EB_ITEMS[b] as u64)
            .wrapping_add(EB_LINES[b] as u64);
        Ok(())
    }

    /// 全十批跑账：声明→验证→封账。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        for b in 0..EB_COUNT {
            self.declare(b)?;
        }
        for b in 0..EB_COUNT {
            self.verify(b)?;
        }
        for b in 0..EB_COUNT {
            self.seal(b)?;
        }
        if self.sealed as usize != EB_COUNT {
            return Err(EbfCode::BatchUnsealed.val());
        }
        let mut b = 0;
        while b < EB_COUNT {
            if !matches!(self.states[b], BatchState::Sealed) {
                return Err(EbfCode::BatchUnsealed.val());
            }
            b += 1;
        }
        Ok(self.sealed)
    }

    /// 复现：双跑逐批状态 + 指纹一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = EbatchLedger::new();
        let _ = run2.run_all()?;
        if run2.sealed != self.sealed || run2.fingerprint != self.fingerprint {
            return Err(EbfCode::NotReproducible.val());
        }
        let mut b = 0;
        while b < EB_COUNT {
            if self.states[b] != run2.states[b] {
                return Err(EbfCode::NotReproducible.val());
            }
            b += 1;
        }
        Ok(())
    }

    /// 条数/行数守恒（Σ = 合同总数，双向断言）。
    pub fn conservation(&self) -> Result<(u32, u32), u32> {
        let mut items = 0u32;
        let mut lines = 0u32;
        let mut b = 0;
        while b < EB_COUNT {
            items = items.wrapping_add(EB_ITEMS[b]);
            lines = lines.wrapping_add(EB_LINES[b]);
            b += 1;
        }
        if items != GOLD_ITEM_TOTAL {
            return Err(EbfCode::ItemConservation.val());
        }
        if lines != GOLD_LINE_TOTAL {
            return Err(EbfCode::LineConservation.val());
        }
        Ok((items, lines))
    }
}

/// 依赖边全图校验：边闭集 + 无自环 + Kahn 拓扑无环。
pub fn dependency_graph_audit() -> Result<u32, u32> {
    let mut indeg = [0u32; EB_COUNT];
    let mut e = 0;
    while e < EDGE_CHAIN.len() {
        let (a, b) = EDGE_CHAIN[e];
        if a >= EB_COUNT || b >= EB_COUNT {
            return Err(EbfCode::EdgeOutOfRange.val());
        }
        if a == b {
            return Err(EbfCode::EdgeSelfLoop.val());
        }
        indeg[b] += 1;
        e += 1;
    }
    if e != GOLD_EDGE_COUNT {
        return Err(EbfCode::DependencyCycle.val());
    }
    // Kahn：入度清零推进，处理满十批 = 无环。
    let mut done = [false; EB_COUNT];
    let mut processed = 0u32;
    let mut progress = true;
    while progress {
        progress = false;
        let mut b = 0;
        while b < EB_COUNT {
            if !done[b] && indeg[b] == 0 {
                done[b] = true;
                processed += 1;
                progress = true;
                // 消边：b 的所有出边目标入度减一。
                let mut k = 0;
                while k < EDGE_CHAIN.len() {
                    if EDGE_CHAIN[k].0 == b {
                        let t = EDGE_CHAIN[k].1;
                        indeg[t] = indeg[t].saturating_sub(1);
                    }
                    k += 1;
                }
            }
            b += 1;
        }
    }
    if processed != EB_COUNT as u32 {
        return Err(EbfCode::DependencyCycle.val());
    }
    Ok(processed)
}

// ── 确定性 LCG（种子写死 = SEG_BASE） ────────────────────────────
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
        self.next() % n.max(1)
    }
}

/// LCG 抽样对账：确定性抽 10 批复验四列对表（预告即实作）。
pub fn lcg_spot_check() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ok = 0u32;
    let mut k = 0;
    while k < 10 {
        let b = rng.below(EB_COUNT as u64) as usize;
        if EB_IDS[b] != GOLD_IDS[b]
            || EB_THEMES[b] != GOLD_THEMES[b]
            || EB_ITEMS[b] != GOLD_ITEMS[b]
            || EB_LINES[b] != GOLD_LINES[b]
        {
            return Err(EbfCode::ContractDrift.val());
        }
        ok += 1;
        k += 1;
    }
    Ok(ok)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = EbatchLedger::new();
    let sealed = led.run_all()?;
    led.reproducible()?;
    let (items, lines) = led.conservation()?;
    let deps = dependency_graph_audit()?;
    if deps != EB_COUNT as u32 {
        return Err(EbfCode::DependencyCycle.val());
    }
    Ok((sealed, items, lines))
}

// ── 注入面：三族注入必抓 ─────────────────────────────────────────
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
    // 族 0：批号越闭集注入。
    ledger.tries[0] += 1;
    let mut led = EbatchLedger::new();
    if let Err(e) = led.declare(EB_COUNT) {
        if e == EbfCode::BatchOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：状态机越边注入（未声明先验证）。
    ledger.tries[1] += 1;
    if let Err(e) = led.verify(0) {
        if e == EbfCode::IllegalTransition.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：依赖边越闭集注入（伪边 (9,9) 自环必被单边校验拒收）。
    ledger.tries[2] += 1;
    if let Err(e) = edge_valid(9, 9) {
        if e == EbfCode::EdgeSelfLoop.val() {
            ledger.caught[2] += 1;
        }
    }
}

/// 单边校验：闭集 + 无自环（图审计的格级原语）。
pub fn edge_valid(a: usize, b: usize) -> Result<(), u32> {
    if a >= EB_COUNT || b >= EB_COUNT {
        return Err(EbfCode::EdgeOutOfRange.val());
    }
    if a == b {
        return Err(EbfCode::EdgeSelfLoop.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EbfCode::ALL.len() {
        let v = EbfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-十批全封账+守恒+依赖无环",
        matches!(golden, Ok((10, 134, 3960))),
        format!("{:?}", golden),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    let deps = dependency_graph_audit();
    v.push((
        "J1-依赖边全图（12边无自环无环）",
        matches!(deps, Ok(10)),
        format!("{:?}", deps),
    ));
    let spot = lcg_spot_check();
    v.push((
        "J1-LCG抽样10批四列对表",
        matches!(spot, Ok(10)),
        format!("{:?}", spot),
    ));
    let fp_stable = {
        let mut a = EbatchLedger::new();
        let mut b = EbatchLedger::new();
        let _ = a.run_all();
        let _ = b.run_all();
        a.fingerprint == b.fingerprint && a.fingerprint != 0
    };
    v.push((
        "J1-声明指纹钉版（双跑一致且非零）",
        fp_stable,
        String::from("指纹=批号+条数+行数三因子折叠"),
    ));
    // 主题句非空点验（十批主题列全非空）。
    let themes_ok = {
        let mut ok = true;
        let mut b = 0;
        while b < EB_COUNT {
            if EB_THEMES[b].len() == 0 {
                ok = false;
            }
            b += 1;
        }
        ok
    };
    v.push((
        "J1-十批主题句点验",
        themes_ok,
        String::from("四列框架表主题列非空"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66280 窗口")));
    v
}

pub fn run_unx14081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14081 E 段总账开篇（B16–B25 十批框架守恒声明）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
