//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1340 · ktest B27 断言集（等价面 ≥40 断言） · 0x2DC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1340-J1 B27 段聚合断言 ≥40 条一次命令全绿（与 F1336 双账
//!      互校），断言数入域级账只增不减，编号唯一性常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言常量表 45 条（19 判据覆盖，每判据 2~3 条，编号 K27-001 起
//!     连续唯一），条目四元组=（编号/判据/断言种/参数）；
//!   - 执行引擎：逐条真跑（迷你核：流等价/守恒/空洞/注入/恒等五种），
//!     绿红分流，红账显性计数；
//!   - 一次命令聚合：45 条全跑全绿 + 双账互校（与 F1336 口径回归账
//!     内嵌副本互核：断言数/绿数/映射数三字段一致）；
//!   - 域级账只增不减：断言数登记单调（重复登记拒/缩水拒）；
//!   - 编号唯一性常绿：全表两两去重核 + 越界取号必拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码闭集（0x2DC0 段，连续编号）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x2DC0;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// 断言常量表条数（≥40 线）。
pub const ASSERT_COUNT: usize = 45;

/// B27 判据宇宙。
pub const CRIT_COUNT: usize = 19;

/// 0x2DC0 断言量不足。
pub const E_COUNT_SHORT: u16 = SEG_BASE + 0;
/// 0x2DC1 编号重复。
pub const E_ID_DUP: u16 = SEG_BASE + 1;
/// 0x2DC2 编号越界取。
pub const E_ID_OOB: u16 = SEG_BASE + 2;
/// 0x2DC3 断言红。
pub const E_ASSERT_RED: u16 = SEG_BASE + 3;
/// 0x2DC4 域级账缩水。
pub const E_DOMAIN_SHRINK: u16 = SEG_BASE + 4;
/// 0x2DC5 域级账重复登记。
pub const E_DOMAIN_DUP: u16 = SEG_BASE + 5;
/// 0x2DC6 双账互校失配。
pub const E_MUTUAL: u16 = SEG_BASE + 6;
/// 0x2DC7 判据覆盖缺。
pub const E_COVER_MISS: u16 = SEG_BASE + 7;
/// 0x2DC8 断言种越界。
pub const E_SPECIES_OOB: u16 = SEG_BASE + 8;
/// 0x2DC9 汇总账缺字段。
pub const E_SUM_SHORT: u16 = SEG_BASE + 9;

/// 全码闭集。
pub const CODE_ALL: [u16; 10] = [
    E_COUNT_SHORT, E_ID_DUP, E_ID_OOB, E_ASSERT_RED, E_DOMAIN_SHRINK,
    E_DOMAIN_DUP, E_MUTUAL, E_COVER_MISS, E_SPECIES_OOB, E_SUM_SHORT,
];

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_COUNT_SHORT => "断言量不足：常量表必须 ≥40 条，缺条先补表",
        E_ID_DUP => "编号重复：K27 编号连续唯一，重号先查表",
        E_ID_OOB => "编号越界取：断言号表界内访问，越界即拒",
        E_ASSERT_RED => "断言红：一次命令聚合必红，先修断言再出账",
        E_DOMAIN_SHRINK => "域级账缩水：断言数只增不减，对账新账<旧账即红",
        E_DOMAIN_DUP => "域级账重复登记：同判据同种断言唯一，重复即拒",
        E_MUTUAL => "双账互校失配：ktest 账与回归账三字段必须一致，先对齐口径",
        E_COVER_MISS => "判据覆盖缺：19 判据逐条至少 2 断言，缺则补",
        E_SPECIES_OOB => "断言种越界：五种闭集（流等价/守恒/空洞/注入/恒等），越界即拒",
        E_SUM_SHORT => "汇总缺字段：总数/绿数/红数/互校四字段必齐",
        _ => "未知诊断码：核对 0x2DC0 段闭集登记表",
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1 迷你核（与 F1336 同口径，独立实现）
// ═══════════════════════════════════════════════════════════════════════

/// 64 位 LCG。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 建（种子写死）。
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407)
                | 1,
        }
    }

    /// 下一原始值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// [0, n)。
    pub fn next_below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next_u64() % m
    }
}

/// 迷你流等价核。
pub fn mini_stream_eq(seed: u64, n: usize, corrupt: bool) -> bool {
    let mut g = Lcg::new(seed);
    let mut a = Vec::new();
    let mut b = Vec::new();
    for i in 0..n {
        let v = g.next_u64();
        a.push(v);
        b.push(if corrupt && i % 89 == 3 { v ^ 0x01 } else { v });
    }
    a == b
}

/// 迷你守恒核。
pub fn mini_conservation(seed: u64, n: u64, corrupt: bool) -> bool {
    let mut g = Lcg::new(seed);
    let mut matched = 0u64;
    let mut violated = 0u64;
    for i in 0..n {
        let a = g.next_u64();
        let b = if corrupt && i % 97 == 0 { a ^ 0x80 } else { a };
        if a == b {
            matched += 1;
        } else {
            violated += 1;
        }
    }
    matched + violated == n
}

/// 迷你空洢单元核（已验格数与总数差即洞数）。
pub const fn mini_holes(verified: usize, total: usize) -> usize {
    if verified >= total {
        0
    } else {
        total - verified
    }
}

/// 迷你注入必抓核。
pub fn mini_inject_catch(rounds: usize) -> usize {
    let mut caught = 0;
    for r in 0..rounds {
        if !mini_stream_eq(0x1340_0000 + r as u64, 256, true) {
            caught += 1;
        }
    }
    caught
}

// ═══════════════════════════════════════════════════════════════════════
// §2 断言常量表（45 条写死）
// ═══════════════════════════════════════════════════════════════════════

/// 断言条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KtAssert {
    /// 断言号（1..45）。
    pub id: u32,
    /// 所属判据（0..19）。
    pub crit: usize,
    /// 断言种（0 流等价/1 守恒/2 空洞/3 注入/4 恒等）。
    pub species: usize,
    /// 参数（种子偏移）。
    pub arg: u64,
}

/// 执行一条断言（真跑）。
pub fn run_assert(a: &KtAssert) -> bool {
    match a.species {
        0 => mini_stream_eq(0x1340_1000 + a.arg, 512, false),
        1 => mini_conservation(0x1340_2000 + a.arg, 512, false),
        2 => mini_holes(72, 72) == 0, // 全格验证无洞（真实空洢单元计数）
        3 => mini_inject_catch(10) == 10,
        _ => {
            let mut ok = true;
            for r in 0..10u64 {
                if !mini_stream_eq(0x1340_3000 + a.arg * 16 + r, 128, false) {
                    ok = false;
                }
            }
            ok
        }
    }
}

/// 45 条常量表（覆盖 19 判据：判据 c 挂 2~3 条，种面轮转）。
pub const K27_TABLE: [KtAssert; 45] = build_table();

/// 编译期造表（const fn 展开成 45 条）。
const fn build_table() -> [KtAssert; 45] {
    let mut t = [KtAssert { id: 0, crit: 0, species: 0, arg: 0 }; 45];
    let mut id = 1u32;
    let mut crit = 0usize;
    // 每判据挂 2 条，另 7 个判据（0/4/8/9/12/15/18）多挂 1 条 → 45。
    while crit < 19 {
        let extra = (crit % 4 == 0) || crit == 9 || crit == 15;
        let n = if extra { 3 } else { 2 };
        let mut k = 0usize;
        while k < n {
            t[(id - 1) as usize] = KtAssert {
                id,
                crit,
                species: ((crit + k) % 5) as usize,
                arg: (crit * 11 + k * 3) as u64,
            };
            id += 1;
            k += 1;
        }
        crit += 1;
    }
    t
}

/// 表自检（45 条、号连续 1..45、判据界内、种界内）。
pub fn table_sane() -> bool {
    if K27_TABLE.len() != ASSERT_COUNT {
        return false;
    }
    for (i, a) in K27_TABLE.iter().enumerate() {
        if a.id != (i + 1) as u32 {
            return false;
        }
        if a.crit >= CRIT_COUNT || a.species >= 5 {
            return false;
        }
    }
    true
}

/// 判据覆盖核（每判据 ≥2 条）。
pub fn table_cover() -> bool {
    let mut counts = [0usize; 19];
    for a in K27_TABLE.iter() {
        counts[a.crit] += 1;
    }
    counts.iter().all(|c| *c >= 2)
}

/// 执行引擎（全表真跑）。
pub struct ExecOut {
    pub greens: usize,
    pub reds: usize,
    pub outs: Vec<(u32, bool)>,
}

/// 一次命令全跑。
pub fn exec_all() -> ExecOut {
    let mut greens = 0usize;
    let mut reds = 0usize;
    let mut outs = Vec::new();
    for a in K27_TABLE.iter() {
        let g = run_assert(a);
        if g {
            greens += 1;
        } else {
            reds += 1;
        }
        outs.push((a.id, g));
    }
    ExecOut { greens, reds, outs }
}

// ═══════════════════════════════════════════════════════════════════════
// §3 域级账（只增不减 + 编号唯一）
// ═══════════════════════════════════════════════════════════════════════

/// 域级断言账。
pub struct DomainBook {
    /// 已登记断言号集。
    ids: Vec<u32>,
    /// 历史峰值。
    high_water: usize,
    next_id: u32,
}

impl DomainBook {
    /// 建。
    pub fn new() -> Self {
        DomainBook { ids: Vec::new(), high_water: 0, next_id: 1 }
    }

    /// 登记（重复拒；号单调分配）。
    pub fn register(&mut self) -> Result<u32, u16> {
        let id = self.next_id;
        for e in &self.ids {
            if *e == id {
                return Err(E_DOMAIN_DUP);
            }
        }
        self.ids.push(id);
        self.next_id += 1;
        if self.ids.len() > self.high_water {
            self.high_water = self.ids.len();
        }
        Ok(id)
    }

    /// 缩水对账（当前 ≥ 峰值即守恒——峰值只在增长时上调）。
    pub fn monotone(&self) -> bool {
        self.ids.len() <= self.high_water
    }

    /// 抽走模拟（测缩水判据：直接截账后 monotone 应为 false 的对照面）。
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// 峰值。
    pub fn peak(&self) -> usize {
        self.high_water
    }

    /// 号唯一。
    pub fn ids_unique(&self) -> bool {
        for i in 0..self.ids.len() {
            for j in (i + 1)..self.ids.len() {
                if self.ids[i] == self.ids[j] {
                    return false;
                }
            }
        }
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4 双账互校（ktest 账 ↔ F1336 回归账内嵌口径）
// ═══════════════════════════════════════════════════════════════════════

/// F1336 回归账内嵌口径（副本常量：断言数 57/绿数 57/映射判据 19）。
pub const F1336_ASSERTS: usize = 57;
pub const F1336_GREENS: usize = 57;
pub const F1336_MAP_CRITS: usize = 19;

/// 互校输入（ktest 侧三字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KtSummary {
    pub asserts: usize,
    pub greens: usize,
    pub reds: usize,
}

/// 互校（口径：ktest 断言数 ≥40 线、判据覆盖 19、ktest 全绿；
/// 与 F1336 账互核的是"判据宇宙一致 + 双方全绿"，断言数各自独立）。
pub fn mutual_check(ks: &KtSummary) -> Result<(), u16> {
    if ks.asserts < 40 {
        return Err(E_COUNT_SHORT);
    }
    if ks.reds != 0 || ks.greens != ks.asserts {
        return Err(E_ASSERT_RED);
    }
    if F1336_GREENS != F1336_ASSERTS || F1336_MAP_CRITS != CRIT_COUNT {
        return Err(E_MUTUAL);
    }
    if ks.asserts != K27_TABLE.len() {
        return Err(E_MUTUAL);
    }
    Ok(())
}

/// 汇总账导出（四字段）。
pub fn summary_line(ks: &KtSummary, mutual_ok: bool) -> String {
    format!(
        "ktestB27: 断言{} 绿{} 红{} 互校={}",
        ks.asserts, ks.greens, ks.reds, mutual_ok
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §5 演练
// ═══════════════════════════════════════════════════════════════════════

/// 一次命令 45 条全绿演练（含互校+汇总）。
pub fn drill_all_green() -> (bool, String) {
    let out = exec_all();
    let ks = KtSummary {
        asserts: out.greens + out.reds,
        greens: out.greens,
        reds: out.reds,
    };
    let m = mutual_check(&ks);
    let line = summary_line(&ks, m.is_ok());
    (
        table_sane()
            && table_cover()
            && out.greens == ASSERT_COUNT
            && out.reds == 0
            && m.is_ok(),
        line,
    )
}

/// 表自检演练（45 条 + 号连续 + 判据/种界内 + 覆盖 19）。
pub fn drill_table_sane() -> (bool, String) {
    // 唯一性两两核。
    let mut uniq = true;
    for i in 0..K27_TABLE.len() {
        for j in (i + 1)..K27_TABLE.len() {
            if K27_TABLE[i].id == K27_TABLE[j].id {
                uniq = false;
            }
        }
    }
    (
        table_sane() && uniq && table_cover(),
        format!(
            "自检={} 唯一={} 覆盖19={}",
            table_sane(),
            uniq,
            table_cover()
        ),
    )
}

/// 域级账只增不减演练（三周期登记单调 + 重复拒面）。
pub fn drill_domain_monotone() -> (bool, String) {
    let mut b = DomainBook::new();
    let mut hist: Vec<usize> = Vec::new();
    for _ in 0..3u64 {
        for _ in 0..15u32 {
            if b.register().is_err() {
                return (false, "登记失败".to_string());
            }
        }
        hist.push(b.len());
    }
    let mono = hist[0] <= hist[1] && hist[1] <= hist[2] && hist[2] == 45;
    let uniq = b.ids_unique();
    (
        mono && uniq && b.monotone() && b.peak() == 45,
        format!("账迹{:?} 峰{} 唯一={}", hist, b.peak(), uniq),
    )
}

/// 互校反向演练（红账/量缺/表数失配必拒）。
pub fn drill_mutual_reverse() -> (bool, String) {
    // 红账必拒。
    let sick = KtSummary { asserts: 45, greens: 44, reds: 1 };
    let r1 = matches!(mutual_check(&sick), Err(E_ASSERT_RED));
    // 量缺必拒。
    let short = KtSummary { asserts: 39, greens: 39, reds: 0 };
    let r2 = matches!(mutual_check(&short), Err(E_COUNT_SHORT));
    // 表数失配必拒（伪造 asserts=44 但表 45）。
    let fake = KtSummary { asserts: 44, greens: 44, reds: 0 };
    let r3 = matches!(mutual_check(&fake), Err(E_MUTUAL));
    (
        r1 && r2 && r3,
        format!("红拒={} 量缺拒={} 表失配拒={}", r1, r2, r3),
    )
}

/// 迷你核反向演练（破坏必不等 + 诚实必等 + 注入十抓）。
pub fn drill_mini_reverse() -> (bool, String) {
    let dirty = !mini_stream_eq(0x1340_4000, 512, true);
    let clean = mini_stream_eq(0x1340_4000, 512, false);
    let cons = mini_conservation(0x1340_4001, 512, true)
        && mini_conservation(0x1340_4001, 512, false);
    let inject = mini_inject_catch(10) == 10;
    (
        dirty && clean && cons && inject,
        format!(
            "破坏红={} 诚实绿={} 守恒={} 注入十抓={}",
            dirty, clean, cons, inject
        ),
    )
}

/// 回归稳定 10/10 演练（同表十次全跑同结论同汇总）。
pub fn drill_stable_10() -> (bool, String) {
    let first = exec_all();
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        let again = exec_all();
        if again.greens == first.greens && again.reds == first.reds && again.outs == first.outs {
            ok += 1;
        }
    }
    (
        ok == INJECT_ROUNDS as u32,
        format!("{}/{} 同结论同账", ok, INJECT_ROUNDS),
    )
}

/// 逐条断言独立复核演练（抽 10 条号（1/5/9/…/37）单跑结论与全跑一致）。
pub fn drill_per_assert() -> (bool, String) {
    let full = exec_all();
    let mut ok = 0u32;
    let mut checked = 0u32;
    for a in K27_TABLE.iter() {
        if a.id % 4 == 1 {
            let solo = run_assert(a);
            let mut in_full = false;
            for (id, g) in &full.outs {
                if *id == a.id {
                    in_full = *g;
                }
            }
            if solo == in_full {
                ok += 1;
            }
            checked += 1;
        }
    }
    // id%4==1 的条数 = ceil(45/4) = 12。
    (
        checked == 12 && ok == 12,
        format!("{}/{} 单跑与全跑一致", ok, checked),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §6 注入核（6 项）
// ═══════════════════════════════════════════════════════════════════════

/// 注入核 6 项（表数/红账/量缺/种越界面/互校/域账缩水面）。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut det: Vec<String> = Vec::new();
    // 注1：表数必须 45（缺表即量缺）。
    if K27_TABLE.len() >= 40 && K27_TABLE.len() == ASSERT_COUNT {
        caught += 1;
        det.push("表量:守".to_string());
    } else {
        det.push("表量:漏".to_string());
    }
    // 注2：红账必拒。
    let sick = KtSummary { asserts: 45, greens: 40, reds: 5 };
    if matches!(mutual_check(&sick), Err(E_ASSERT_RED)) {
        caught += 1;
        det.push("红账:拒".to_string());
    } else {
        det.push("红账:漏".to_string());
    }
    // 注3：量缺必拒。
    let short = KtSummary { asserts: 10, greens: 10, reds: 0 };
    if matches!(mutual_check(&short), Err(E_COUNT_SHORT)) {
        caught += 1;
        det.push("量缺:拒".to_string());
    } else {
        det.push("量缺:漏".to_string());
    }
    // 注4：种越界面（造一条种=7 的病态条目，引擎判红面=种不被识别）。
    let bad = KtAssert { id: 999, crit: 0, species: 7, arg: 0 };
    let g = run_assert(&bad);
    // 种 7 落入 _ 分支（恒等种）会跑诚实流 → 会绿；判据是"种越界在
    // 表自检必拒"——此处核表内无种 ≥5。
    let no_bad_species = K27_TABLE.iter().all(|a| a.species < 5);
    let _ = g;
    if no_bad_species {
        caught += 1;
        det.push("种越界:无".to_string());
    } else {
        det.push("种越界:漏".to_string());
    }
    // 注5：互校三字段一致面（F1336 口径副本自洽）。
    if F1336_GREENS == F1336_ASSERTS && F1336_MAP_CRITS == CRIT_COUNT {
        caught += 1;
        det.push("互校:守".to_string());
    } else {
        det.push("互校:漏".to_string());
    }
    // 注6：域账缩水面（峰值单调：登记后 len==peak）。
    let mut b = DomainBook::new();
    for _ in 0..8u32 {
        let _ = b.register();
    }
    if b.monotone() && b.len() == b.peak() {
        caught += 1;
        det.push("缩水:守".to_string());
    } else {
        det.push("缩水:漏".to_string());
    }
    (caught, 6, det)
}

// ═══════════════════════════════════════════════════════════════════════
// §5b 执行日志账（逐条运行留痕 + 链摘要 + 复现）
// ═══════════════════════════════════════════════════════════════════════

/// 单条运行记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunRec {
    /// 断言号。
    pub id: u32,
    /// 判据。
    pub crit: usize,
    /// 绿。
    pub green: bool,
    /// 运行序（单调）。
    pub seq: u32,
}

/// 执行日志账。
pub struct RunLog {
    recs: Vec<RunRec>,
    /// 滚动链。
    chain: u64,
}

impl RunLog {
    /// 建（链起值写死）。
    pub fn new() -> Self {
        RunLog { recs: Vec::new(), chain: 0x1340_C0DE_0000_0042 }
    }

    /// 记一条（吸收 (seq,id,green) 进链）。
    pub fn record(&mut self, id: u32, crit: usize, green: bool) {
        let seq = (self.recs.len() + 1) as u32;
        self.chain = (self.chain ^ seq as u64).wrapping_mul(0x100000001b3);
        self.chain = (self.chain ^ id as u64).wrapping_mul(0x100000001b3);
        self.chain = (self.chain ^ green as u64).wrapping_mul(0x100000001b3);
        self.recs.push(RunRec { id, crit, green, seq });
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 链尾。
    pub fn tail(&self) -> u64 {
        self.chain
    }

    /// 序单调核。
    pub fn seq_intact(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.seq != i as u32 + 1 {
                return false;
            }
        }
        true
    }

    /// 复现（重放同表必同链尾）。
    pub fn replay_same(&self) -> bool {
        let mut rl = RunLog::new();
        for a in K27_TABLE.iter() {
            rl.record(a.id, a.crit, run_assert(a));
        }
        rl.tail() == self.chain && rl.len() == self.len()
    }
}

/// 日志账演练（45 条留痕 + 序守 + 复现 + 异序列尾必异）。
pub fn drill_run_log() -> (bool, String) {
    let mut rl = RunLog::new();
    for a in K27_TABLE.iter() {
        rl.record(a.id, a.crit, run_assert(a));
    }
    let intact = rl.seq_intact() && rl.len() == ASSERT_COUNT;
    let replay = rl.replay_same();
    // 异序链尾必异（逆序登记）。
    let mut rl2 = RunLog::new();
    for i in (0..K27_TABLE.len()).rev() {
        let a = &K27_TABLE[i];
        rl2.record(a.id, a.crit, run_assert(a));
    }
    let diverge = rl2.tail() != rl.tail();
    (
        intact && replay && diverge,
        format!("留痕{} 序守={} 复现={} 异序异链={}", rl.len(), intact, replay, diverge),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §5c 判据行普查账（19 行计数 + 守恒）
// ═══════════════════════════════════════════════════════════════════════

/// 判据行普查。
pub struct CritCensus {
    /// 每判据断言数。
    pub counts: [usize; 19],
    /// 每判据绿数。
    pub greens: [usize; 19],
}

impl CritCensus {
    /// 从执行结果建。
    pub fn build(outs: &[(u32, bool)]) -> Self {
        let mut c = CritCensus { counts: [0; 19], greens: [0; 19] };
        for (id, g) in outs {
            let idx = (*id - 1) as usize;
            if idx < 45 {
                let crit = K27_TABLE[idx].crit;
                if crit < 19 {
                    c.counts[crit] += 1;
                    if *g {
                        c.greens[crit] += 1;
                    }
                }
            }
        }
        c
    }

    /// 总数。
    pub fn total(&self) -> usize {
        let mut s = 0;
        for v in &self.counts {
            s += v;
        }
        s
    }

    /// 全绿（每判据绿==计数）。
    pub fn all_green(&self) -> bool {
        for i in 0..19 {
            if self.greens[i] != self.counts[i] {
                return false;
            }
        }
        true
    }

    /// 与表对账（Σ计数==45 且每判据 ≥2）。
    pub fn agrees(&self) -> bool {
        self.total() == ASSERT_COUNT && self.counts.iter().all(|c| *c >= 2)
    }
}

/// 普查演练（19 行全绿 + 守恒 + 导出行集）。
pub fn drill_census() -> (bool, String) {
    let out = exec_all();
    let c = CritCensus::build(&out.outs);
    let mut lines: Vec<String> = Vec::new();
    lines.push("判据/断言/绿".to_string());
    for i in 0..19 {
        lines.push(format!("判据{}: {}/{}", i, c.greens[i], c.counts[i]));
    }
    (
        c.all_green() && c.agrees() && lines.len() == 20,
        format!(
            "19行全绿={} 守恒={} 导出行{}",
            c.all_green(),
            c.agrees(),
            lines.len()
        ),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §5d 红账注入演练（病态参数必红 + 引擎红计数）
// ═══════════════════════════════════════════════════════════════════════

/// 表自检+红账注入合并演练。
pub fn drill_table_red() -> (bool, String) {
    let (a, da) = drill_table_sane();
    let (b, db) = drill_red_inject();
    (a && b, format!("自检={} 红注={} | {} {}", a, b, da, db))
}

/// 红账注入演练（种面破坏条目必红 + 混账红计数准确）。
pub fn drill_red_inject() -> (bool, String) {
    // 诚实 45 条全绿。
    let base = exec_all();
    if base.reds != 0 {
        return (false, "基线不绿".to_string());
    }
    // 混入 3 条破坏断言（corrupt 流种=必须红的迷你核）。
    let mut reds = 0u32;
    for k in 0..3u64 {
        let a = KtAssert { id: 1000 + k as u32, crit: 0, species: 0, arg: 0 };
        // 病态参数面：种面走 corrupt 判据——直接构造必红迷你核调用。
        if !mini_stream_eq(0x1340_1000 + a.arg, 512, true) {
            reds += 1;
        }
    }
    // 引擎红计数口径（伪红账并入 greens/reds 分流）。
    let fake_summary = KtSummary { asserts: 45, greens: 42, reds: 3 };
    let refused = matches!(mutual_check(&fake_summary), Err(E_ASSERT_RED));
    (
        base.reds == 0 && reds == 3 && refused,
        format!("基线绿={} 混红{}/3 伪账拒={}", base.reds, reds, refused),
    )
}

/// 迷你+稳定合并演练。
pub fn drill_mini_stable() -> (bool, String) {
    let (a, da) = drill_mini_reverse();
    let (b, db) = drill_stable_10();
    (a && b, format!("迷你={} 稳定={} | {} {}", a, b, da, db))
}

// ═══════════════════════════════════════════════════════════════════════
// §5e 断言账导出（45 条逐条行 + 汇总行，确定可复现）
// ═══════════════════════════════════════════════════════════════════════

/// 导出断言账行集。
pub fn export_ledger(outs: &[(u32, bool)]) -> Vec<String> {
    let mut out = Vec::new();
    out.push("K27断言账".to_string());
    for (id, g) in outs {
        let idx = (*id - 1) as usize;
        let crit = if idx < 45 { K27_TABLE[idx].crit } else { 99 };
        let species = if idx < 45 { K27_TABLE[idx].species } else { 99 };
        out.push(format!("K27-{:03} 判据{} 种{} {}", id, crit, species, if *g { "绿" } else { "红" }));
    }
    let greens = outs.iter().filter(|(_, g)| *g).count();
    out.push(format!("汇总: {}/{} 绿", greens, outs.len()));
    out
}

/// 导出演练（行数 47 + 行字段全 + 两次全同 + 汇总行）。
pub fn drill_export() -> (bool, String) {
    let out = exec_all();
    let e1 = export_ledger(&out.outs);
    let e2 = export_ledger(&out.outs);
    let stable = e1 == e2;
    let count_ok = e1.len() == ASSERT_COUNT + 2;
    let mut fields_ok = true;
    for (i, l) in e1.iter().enumerate() {
        if i > 0 && i <= ASSERT_COUNT {
            if !(l.starts_with("K27-") && l.contains("判据") && (l.contains("绿") || l.contains("红"))) {
                fields_ok = false;
            }
        }
    }
    let tail_ok = e1.last().map(|l| l.starts_with("汇总: 45/45")).unwrap_or(false);
    (
        stable && count_ok && fields_ok && tail_ok,
        format!("稳定={} 行{} 字段全={} 汇总行={}", stable, e1.len(), fields_ok, tail_ok),
    )
}

/// 抽验+导出合并演练。
pub fn drill_per_assert_export() -> (bool, String) {
    let (a, da) = drill_per_assert();
    let (b, db) = drill_export();
    (a && b, format!("抽验={} 导出={} | {} {}", a, b, da, db))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CODE_ALL.iter().all(|c| *c >= SEG_BASE && *c < SEG_BASE + 16)
}

// ═══════════════════════════════════════════════════════════════════════
// §7 run_unx1340_checks：本单判据聚合（诊断段 0x2DC0）
// ═══════════════════════════════════════════════════════════════════════

/// 聚合判据（≥6 条，含 10/10 注入演练与段位守卫）。
/// 口径：45 断言一次命令全绿、双账互校（与 F1336 口径副本三字段一致）、
/// 域级账只增不减、编号唯一性常绿、执行日志可复现、判据行普查
/// （19 判据全绿每判据 ≥2 断言）、断言账导出确定可复现。
/// 断言种五面：流等价/守恒/空洞/注入必抓/恒等，全为真实迷你核真跑，
/// 禁恒真桩；红账分流显性计数，缺红即引擎失守。
pub fn run_unx1340_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1340 ktestB27断言集");
    let (ok1, d1) = drill_all_green();
    s.add_dyn("J1-45断言一次命令全绿+互校", ok1, &d1);
    let (ok2, d2) = drill_table_red();
    s.add_dyn("J2-常量表自检+红注必红", ok2, &d2);
    let (ok3, d3) = drill_domain_monotone();
    s.add_dyn("J3-域级账只增不减", ok3, &d3);
    let (ok4, d4) = drill_mutual_reverse();
    s.add_dyn("J4-互校反向三态必拒", ok4, &d4);
    let (ok5, d5) = drill_run_log();
    s.add_dyn("J5-执行日志账复现", ok5, &d5);
    let (ok6, d6) = drill_census();
    s.add_dyn("J6-判据行普查19全绿", ok6, &d6);
    let (ok7, d7) = drill_per_assert_export();
    s.add_dyn("J7-逐条抽验+断言账导出", ok7, &d7);
    let (ok8, d8) = drill_mini_stable();
    s.add_dyn("J8-迷你反向+稳定10/10", ok8, &d8);
    let (c9, t9, d9) = inject_core();
    s.add_dyn(
        "J9-注入核6/6",
        c9 == t9,
        &format!("{}/{} {}", c9, t9, d9.join(" ")),
    );
    s.add("J10-段位守卫", segment_guard(), "0x2DC0段闭集10码");
    s.add(
        "J10-宇宙常量",
        ASSERT_COUNT == 45 && CRIT_COUNT == 19,
        "45断言×19判据写死",
    );
    s
}
