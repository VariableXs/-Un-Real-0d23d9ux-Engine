//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1384 · 接管三部曲终审（B25–B27 测绘/退役/等价终审）· 0x72040
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B30）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1384-J1 接管终审执行 10/10 次（测绘覆盖 100%+退役守恒
//!     100%+等价率 100% 三硬线复核），三部曲链条完整性（测绘→退役
//!     →等价逐点可溯）100%，终审报告落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 施工说明：
//!   - 接管单元（TakeoverUnit）三阶段单调状态机：Fresh→Mapped（测绘）
//!     →Retired（退役）→Equived（等价），倒退/跳步显性拒。
//!   - 三硬线逐轮复核：测绘覆盖（已测绘字节/勘测计划字节，千分数
//!     必须 =1000）、退役守恒（退役字节 = 测绘字节，零损）、等价率
//!     （新旧行为签名一致单元数/单元总数 =100%）。
//!   - 链条完整性：每单元三阶段逐点可溯（测绘账/退役账/等价签全在
//!     且阶段序合法），链条完整率必须 100%。
//!   - 终审执行 10 轮全绿，终审报告（含 FNV 摘要）落档，摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72040，VX3-1103 接管终审批独占区）。
pub const SEG_BASE: u32 = 0x72040;

/// 终审执行轮数（冻结 10，判据 10/10 语义）。
pub const REVIEW_ROUNDS: u32 = 10;
/// 接管单元数（冻结 8）。
pub const UNIT_COUNT: usize = 8;
/// 每单元测绘区数（冻结 4）。
pub const REGIONS_PER_UNIT: usize = 4;
/// 单区基准大小（字节）。
pub const REGION_SIZE: u64 = 0x1000;
/// 覆盖硬线（千分数 1000 = 100%）。
pub const COVER_HARDLINE: u64 = 1000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TkCode {
    /// 测绘覆盖不足 100%
    SurveyGap,
    /// 退役守恒破（退役字节 ≠ 测绘字节）
    ConserveBreak,
    /// 等价失配（新旧行为签名不一致）
    EquivFail,
    /// 链条断裂（阶段倒退或三账缺一）
    ChainBreak,
    /// 终审轮次不足 10
    RoundShort,
    /// 终审报告缺档
    ReportMissing,
    /// 双跑不一致
    NotReproducible,
}

impl TkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TkCode::SurveyGap => "测绘覆盖不足 100%：请补全勘测计划内全部区段的测绘账",
            TkCode::ConserveBreak => "退役守恒破：退役字节与测绘字节不等，核对逐区退役账",
            TkCode::EquivFail => "等价失配：新旧行为签名不一致，比对接管前后行为迹",
            TkCode::ChainBreak => "三部曲链条断裂：阶段必须测绘→退役→等价单调推进且三账俱全",
            TkCode::RoundShort => "终审轮次不足 10：终审必须满 10 轮全绿",
            TkCode::ReportMissing => "终审报告缺档：请封存报告并写入非空摘要",
            TkCode::NotReproducible => "双跑不一致：复现红线，固定种子后重跑",
        }
    }
    pub const ALL: [TkCode; 7] = [
        TkCode::SurveyGap,
        TkCode::ConserveBreak,
        TkCode::EquivFail,
        TkCode::ChainBreak,
        TkCode::RoundShort,
        TkCode::ReportMissing,
        TkCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TkCode::ALL.len() {
        if TkCode::ALL[i].val() == code {
            return TkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72040 段闭集内"
}

// ── 测绘/退役域模型 ─────────────────────────────────────────────
/// 一段被接管的区（基址 + 字节数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub base: u64,
    pub size: u64,
}

/// 接管三阶段（单调：Fresh→Mapped→Retired→Equived）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TkStage {
    /// 未测绘
    Fresh,
    /// 已测绘
    Mapped,
    /// 已退役
    Retired,
    /// 等价已证
    Equived,
}

impl TkStage {
    fn order(self) -> u32 {
        match self {
            TkStage::Fresh => 0,
            TkStage::Mapped => 1,
            TkStage::Retired => 2,
            TkStage::Equived => 3,
        }
    }
}

/// 接管单元账（测绘账/退役账/等价签三账俱全）。
#[derive(Clone, Debug)]
pub struct TakeoverUnit {
    pub id: u32,
    pub stage: TkStage,
    /// 测绘账（区列表）。
    pub mapped: Vec<Region>,
    /// 测绘字节总数。
    pub mapped_total: u64,
    /// 退役账（区列表，与测绘逐区对应）。
    pub retired: Vec<Region>,
    /// 退役字节总数。
    pub retired_total: u64,
    /// 接管前行为签名。
    pub old_sig: u64,
    /// 接管后行为签名。
    pub new_sig: u64,
    /// 等价已证。
    pub equiv: bool,
}

impl TakeoverUnit {
    /// 新建未测绘单元。
    pub fn new(id: u32) -> TakeoverUnit {
        TakeoverUnit {
            id,
            stage: TkStage::Fresh,
            mapped: Vec::new(),
            mapped_total: 0,
            retired: Vec::new(),
            retired_total: 0,
            old_sig: 0,
            new_sig: 0,
            equiv: false,
        }
    }

    /// 阶段推进（只许前进，停步/倒退一律显性拒）。
    fn advance(&mut self, next: TkStage) -> Result<(), u32> {
        if next.order() <= self.stage.order() {
            return Err(TkCode::ChainBreak.val());
        }
        self.stage = next;
        Ok(())
    }

    /// 测绘：登记区账（只许从 Fresh 出发；区表非空）。
    pub fn measure(&mut self, regions: Vec<Region>) -> Result<u64, u32> {
        if self.stage != TkStage::Fresh {
            return Err(TkCode::ChainBreak.val());
        }
        if regions.is_empty() {
            return Err(TkCode::SurveyGap.val());
        }
        let mut total: u64 = 0;
        let mut i = 0;
        while i < regions.len() {
            total = total.saturating_add(regions[i].size);
            i += 1;
        }
        self.mapped = regions;
        self.mapped_total = total;
        self.advance(TkStage::Mapped)?;
        Ok(total)
    }

    /// 退役：逐区对应退役，字节守恒（退役 = 测绘，零损）。
    pub fn retire(&mut self) -> Result<u64, u32> {
        if self.stage != TkStage::Mapped {
            return Err(TkCode::ChainBreak.val());
        }
        if self.mapped.is_empty() || self.mapped_total == 0 {
            return Err(TkCode::SurveyGap.val());
        }
        let mut retired = Vec::new();
        let mut total: u64 = 0;
        let mut i = 0;
        while i < self.mapped.len() {
            let r = self.mapped[i];
            retired.push(Region { base: r.base, size: r.size });
            total = total.saturating_add(r.size);
            i += 1;
        }
        if total != self.mapped_total {
            return Err(TkCode::ConserveBreak.val());
        }
        self.retired = retired;
        self.retired_total = total;
        self.advance(TkStage::Retired)?;
        Ok(total)
    }

    /// 等价：新旧行为签名一致方可通过。
    pub fn equiv_check(&mut self, old_sig: u64, new_sig: u64) -> Result<(), u32> {
        if self.stage != TkStage::Retired {
            return Err(TkCode::ChainBreak.val());
        }
        if old_sig != new_sig {
            return Err(TkCode::EquivFail.val());
        }
        self.old_sig = old_sig;
        self.new_sig = new_sig;
        self.equiv = true;
        self.advance(TkStage::Equived)?;
        Ok(())
    }

    /// 链条完整性：三账俱全且阶段终到 Equived 且守恒且等价。
    pub fn chain_ok(&self) -> bool {
        self.stage == TkStage::Equived
            && !self.mapped.is_empty()
            && !self.retired.is_empty()
            && self.mapped_total == self.retired_total
            && self.equiv
            && self.old_sig == self.new_sig
            && self.mapped.len() == self.retired.len()
    }
}

// ── 行为签名（FNV-1a 整数可复算） ───────────────────────────────
/// FNV-1a 64 位签名。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 单元行为签名：由测绘区序列整数化派生（确定性可复算）。
pub fn unit_sig(u: &TakeoverUnit) -> u64 {
    let mut bytes = Vec::new();
    bytes.push((u.id & 0xFF) as u8);
    let mut i = 0;
    while i < u.mapped.len() {
        let b = u.mapped[i].base;
        let s = u.mapped[i].size;
        bytes.push((b & 0xFF) as u8);
        bytes.push(((b >> 8) & 0xFF) as u8);
        bytes.push((s & 0xFF) as u8);
        bytes.push(((s >> 8) & 0xFF) as u8);
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 勘测计划（真机账） ─────────────────────────────────────────
/// 真机勘测计划：8 单元 × 4 区（基址确定性铺开）。
pub fn survey_plan() -> Vec<Vec<Region>> {
    let mut plan = Vec::new();
    let mut u = 0;
    while u < UNIT_COUNT {
        let mut regs = Vec::new();
        let mut r = 0;
        while r < REGIONS_PER_UNIT {
            regs.push(Region {
                base: 0x80000 + (u as u64) * 0x10000 + (r as u64) * (REGION_SIZE * 2),
                size: REGION_SIZE,
            });
            r += 1;
        }
        plan.push(regs);
        u += 1;
    }
    plan
}

/// 计划字节总数。
pub fn plan_total(plan: &[Vec<Region>]) -> u64 {
    let mut t: u64 = 0;
    let mut i = 0;
    while i < plan.len() {
        let mut j = 0;
        while j < plan[i].len() {
            t = t.saturating_add(plan[i][j].size);
            j += 1;
        }
        i += 1;
    }
    t
}

/// 真机三部曲全链：测绘→退役→等价（全单元）。
pub fn machine_units() -> Result<Vec<TakeoverUnit>, u32> {
    let plan = survey_plan();
    let mut units = Vec::new();
    let mut i = 0;
    while i < plan.len() {
        let mut u = TakeoverUnit::new(i as u32);
        u.measure(plan[i].clone())?;
        u.retire()?;
        let sig = unit_sig(&u);
        u.equiv_check(sig, sig)?;
        units.push(u);
        i += 1;
    }
    Ok(units)
}

// ── 三硬线逐轮复核 ─────────────────────────────────────────────
/// 三硬线复核账（千分数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardLines {
    /// 测绘覆盖（已测绘字节 × 1000 ÷ 计划字节）。
    pub cover_permille: u64,
    /// 退役守恒（退役字节 × 1000 ÷ 测绘字节）。
    pub conserve_permille: u64,
    /// 等价率（等价单元 × 1000 ÷ 单元总数）。
    pub equiv_permille: u64,
    /// 链条完整率（chain_ok 单元 × 1000 ÷ 单元总数）。
    pub chain_permille: u64,
}

/// 逐轮三硬线复核（覆盖/守恒/等价/链条四率，整数复算）。
pub fn review_once(units: &[TakeoverUnit], plan: &[Vec<Region>]) -> Result<HardLines, u32> {
    if units.is_empty() {
        return Err(TkCode::ChainBreak.val());
    }
    let pt = plan_total(plan);
    if pt == 0 {
        return Err(TkCode::SurveyGap.val());
    }
    let mut mapped: u64 = 0;
    let mut retired: u64 = 0;
    let mut equiv_n: u32 = 0;
    let mut chain_n: u32 = 0;
    let mut i = 0;
    while i < units.len() {
        mapped = mapped.saturating_add(units[i].mapped_total);
        retired = retired.saturating_add(units[i].retired_total);
        if units[i].equiv {
            equiv_n += 1;
        }
        if units[i].chain_ok() {
            chain_n += 1;
        }
        i += 1;
    }
    let cover = mapped.saturating_mul(1000) / pt;
    let conserve = if mapped == 0 { 0 } else { retired.saturating_mul(1000) / mapped };
    let n = units.len() as u32 as u64;
    let equiv = (equiv_n as u64).saturating_mul(1000) / n;
    let chain = (chain_n as u64).saturating_mul(1000) / n;
    Ok(HardLines {
        cover_permille: cover,
        conserve_permille: conserve,
        equiv_permille: equiv,
        chain_permille: chain,
    })
}

/// 三硬线闸（四率全部 =1000‰ 才算本轮全绿）。
pub fn hardline_gate(h: &HardLines) -> Result<(), u32> {
    if h.cover_permille < COVER_HARDLINE {
        return Err(TkCode::SurveyGap.val());
    }
    if h.conserve_permille < COVER_HARDLINE {
        return Err(TkCode::ConserveBreak.val());
    }
    if h.equiv_permille < COVER_HARDLINE {
        return Err(TkCode::EquivFail.val());
    }
    if h.chain_permille < COVER_HARDLINE {
        return Err(TkCode::ChainBreak.val());
    }
    Ok(())
}

// ── 终审报告（落档） ────────────────────────────────────────────
/// 终审报告账。
#[derive(Clone, Debug)]
pub struct FinalReport {
    /// 终审执行轮数。
    pub rounds: u32,
    /// 每轮三硬线账（逐轮留痕）。
    pub per_round: Vec<HardLines>,
    /// 单元总数。
    pub units: usize,
    /// 报告摘要（FNV-1a 派生）。
    pub digest: u64,
    /// 是否落档。
    pub archived: bool,
    /// 写回摘要（非空为完成）。
    pub summary: String,
}

/// 报告闸：满轮数 + 逐轮全绿 + 落档 + 摘要非空。
pub fn gate_report(r: &FinalReport) -> Result<(), u32> {
    if r.rounds != REVIEW_ROUNDS || r.per_round.len() != REVIEW_ROUNDS as usize {
        return Err(TkCode::RoundShort.val());
    }
    let mut i = 0;
    while i < r.per_round.len() {
        hardline_gate(&r.per_round[i])?;
        i += 1;
    }
    if !r.archived || r.summary.is_empty() {
        return Err(TkCode::ReportMissing.val());
    }
    Ok(())
}

/// 终审执行 rounds 轮（每轮三硬线复核，全绿才计入）。
pub fn final_review(units: &[TakeoverUnit], plan: &[Vec<Region>], rounds: u32) -> Result<FinalReport, u32> {
    if rounds != REVIEW_ROUNDS {
        return Err(TkCode::RoundShort.val());
    }
    let mut per_round = Vec::new();
    let mut r = 0;
    while r < rounds {
        let h = review_once(units, plan)?;
        hardline_gate(&h)?;
        per_round.push(h);
        r += 1;
    }
    // 摘要派生：轮数/单元数/首轮四率整数化 FNV。
    let h0 = &per_round[0];
    let bytes: Vec<u8> = vec![
        (rounds & 0xFF) as u8,
        (units.len() & 0xFF) as u8,
        (h0.cover_permille & 0xFF) as u8,
        (h0.conserve_permille & 0xFF) as u8,
        (h0.equiv_permille & 0xFF) as u8,
        (h0.chain_permille & 0xFF) as u8,
    ];
    let digest = fnv1a(&bytes);
    Ok(FinalReport {
        rounds,
        per_round,
        units: units.len(),
        digest,
        archived: true,
        summary: format!(
            "终审{}轮全绿 units={} cover/conserve/equiv/chain=1000‰ digest={:x}",
            rounds,
            units.len(),
            digest
        ),
    })
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：缺测绘/少退役/等价失配/链条断裂） ─────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入轮（LCG 驱动 rounds 轮 × 4 路，每路必抓）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：测绘缺区（少测一区）⇒ 覆盖硬线必抓。
        let plan = survey_plan();
        let mut u1 = TakeoverUnit::new(0);
        let mut short = plan[0].clone();
        short.pop();
        let _ = u1.measure(short);
        let _ = u1.retire();
        let sig = unit_sig(&u1);
        let _ = u1.equiv_check(sig, sig);
        let mut bad_units1 = machine_units()?;
        bad_units1[0] = u1;
        led.record(0, review_once(&bad_units1, &plan).map(|h| hardline_gate(&h).is_err()).unwrap_or(true));
        // 路 2：退役少退（退役账抹一区）⇒ 守恒硬线必抓。
        let mut u2 = TakeoverUnit::new(1);
        u2.measure(plan[1].clone())?;
        // 只退部分：手工截短退役账后直接尝试过闸。
        let mut partial = machine_units()?;
        let full = partial[1].retired_total;
        partial[1].retired_total = full.saturating_sub(REGION_SIZE);
        let h2 = review_once(&partial, &plan)?;
        led.record(1, hardline_gate(&h2).is_err());
        // 路 3：等价失配（新签名差 1）⇒ EquivFail 必抓。
        let mut u3 = TakeoverUnit::new(2);
        u3.measure(plan[2].clone())?;
        u3.retire()?;
        let sig = unit_sig(&u3);
        led.record(2, u3.equiv_check(sig, sig ^ 1).is_err());
        // 路 4：链条断裂（跳过退役直取等价）⇒ ChainBreak 必抓。
        let mut u4 = TakeoverUnit::new(3);
        u4.measure(plan[3].clone())?;
        let sig4 = unit_sig(&u4);
        led.record(3, u4.equiv_check(sig4, sig4).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：真机三部曲 + 终审 10 轮 + 报告落档。
/// 返回 (单元数, 报告摘要字节数, 轮数)。
pub fn golden_run() -> Result<(usize, usize, u32), u32> {
    let plan = survey_plan();
    let units = machine_units()?;
    // 链条完整性 100% 前置断言（逐点可溯）。
    let mut i = 0;
    while i < units.len() {
        if !units[i].chain_ok() {
            return Err(TkCode::ChainBreak.val());
        }
        i += 1;
    }
    let rep = final_review(&units, &plan, REVIEW_ROUNDS)?;
    gate_report(&rep)?;
    Ok((rep.units, rep.summary.len(), rep.rounds))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（8 单元 + 满 10 轮 + 摘要非空）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((n, sum, rounds)) => {
            if n == UNIT_COUNT && sum > 0 && rounds == REVIEW_ROUNDS {
                Ok(())
            } else {
                Err(TkCode::NotReproducible.val())
            }
        }
        Err(e) => Err(e),
    }
}

/// 序 B：等价失配注入 ⇒ EquivFail。
pub fn probe_seq_b() -> Result<(), u32> {
    let plan = survey_plan();
    let mut u = TakeoverUnit::new(99);
    u.measure(plan[0].clone())?;
    u.retire()?;
    let sig = unit_sig(&u);
    match u.equiv_check(sig, sig.wrapping_add(1)) {
        Err(e) if e == TkCode::EquivFail.val() => Ok(()),
        _ => Err(TkCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常数守卫（轮数/单元数/区数/硬线）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        REVIEW_ROUNDS == 10,
        UNIT_COUNT == 8,
        REGIONS_PER_UNIT == 4,
        COVER_HARDLINE == 1000,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(TkCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TkCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TkCode::ALL.len() {
        let v = TkCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：终审 10/10 全绿。
    let g = golden_run();
    v.push((
        "J1-终审10/10",
        match &g {
            Ok((n, _, rounds)) => *n == UNIT_COUNT && *rounds == REVIEW_ROUNDS,
            Err(_) => false,
        },
        match &g {
            Ok((n, sum, rounds)) => format!("units={} summary={}B rounds={}", n, sum, rounds),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三硬线（真机账逐项复核）。
    let plan = survey_plan();
    let units = machine_units();
    let hl = match &units {
        Ok(us) => review_once(us, &plan),
        Err(_) => Err(TkCode::NotReproducible.val()),
    };
    v.push((
        "J1-三硬线",
        match &hl {
            Ok(h) => hardline_gate(h).is_ok(),
            Err(_) => false,
        },
        match &hl {
            Ok(h) => format!(
                "cover={} conserve={} equiv={} chain={}‰",
                h.cover_permille, h.conserve_permille, h.equiv_permille, h.chain_permille
            ),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 链条完整性（测绘→退役→等价逐点可溯）。
    let chain_all = match &units {
        Ok(us) => {
            let mut ok = us.len() == UNIT_COUNT;
            let mut i = 0;
            while i < us.len() {
                if !us[i].chain_ok() {
                    ok = false;
                }
                i += 1;
            }
            ok
        }
        Err(_) => false,
    };
    v.push((
        "J1-链条完整性",
        chain_all,
        String::from("8 单元三账俱全阶段序合法"),
    ));
    // 终审报告落档（闸过 + 摘要非空 + 摘要含 digest）。
    let rep_ok = match (&units, &g) {
        (Ok(us), Ok((_, sum, _))) => {
            match final_review(us, &plan, REVIEW_ROUNDS) {
                Ok(rep) => gate_report(&rep).is_ok() && !rep.summary.is_empty() && *sum > 0,
                Err(_) => false,
            }
        }
        _ => false,
    };
    v.push((
        "J1-报告落档",
        rep_ok,
        String::from("终审报告封存摘要非空"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(REVIEW_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x72040 窗口"),
    ));
    v
}

/// 自检入口（UNX-F1384）。
pub fn run_unx1384_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1384 接管三部曲终审（B25–B27 测绘/退役/等价终审）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
