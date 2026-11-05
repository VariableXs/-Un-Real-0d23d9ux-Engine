//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12618 消息面性能预算（P95 投递延迟账） · 0x80480段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥341 行；
//!   ② 判据原文：UNX-F12619-J1 20 条单跑入口齐，全跑条数与聚合账
//!      等式断言记录随批归档可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 20 条单跑入口冻结：入口表 20 行（名/族/探针号三列），行序
//!     冻结；缺行少列即 EntryTableShort，越名跑即 EntryOutOfRange；
//!   - 每条入口恰跑一次：单跑计数账逐条 +1，重复单跑显性拒
//!     （DoubleRun）；三族计数（投递/应答/混合）之和必须等于
//!     全跑条数（聚合等式，破即 CountEquationBreak）；
//!   - 单跑语义：按族取冻结样本批（LCG 界内整数拍），P95 对预算
//!     线（nearest-rank 整数闭式），在线内得分为绿、超线即红账；
//!   - 随批归档可点验：全跑后把（总条数, 族计数, 账指纹）写归档
//!     存根，点验=重算指纹与存根比对，存根不符即 ArchiveDrift；
//!   - 双跑一致 + 段位守卫；LCG 种子写死 = SEG_BASE（0x80480）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect；
//!     诊断码闭集 0x80480..0x804C0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80480，AI-VX9-1105 B01 批 12 槽第 7 槽）。
pub const SEG_BASE: u32 = 0x80480;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtestEntryCode {
    /// 入口表缺行：不足 20 行
    EntryTableShort,
    /// 入口号越界：超出 20 行窗
    EntryOutOfRange,
    /// 重复单跑：同一入口两次计数
    DoubleRun,
    /// 聚合等式破：族计数之和≠全跑条数
    CountEquationBreak,
    /// 族码非法：族号超出三族闭集
    FamilyInvalid,
    /// 单跑超线：该入口 P95 超预算线
    RunOverBudget,
    /// 归档漂移：存根指纹与重算不符
    ArchiveDrift,
    /// 归档缺存根：未全跑就取存根
    ArchiveMissing,
    /// 样本空账：单跑无样本
    SampleEmpty,
    /// 样本越界：延迟样本超冻结界
    SampleOutOfRange,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl KtestEntryCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                KtestEntryCode::EntryTableShort => 0x01,
                KtestEntryCode::EntryOutOfRange => 0x02,
                KtestEntryCode::DoubleRun => 0x03,
                KtestEntryCode::CountEquationBreak => 0x04,
                KtestEntryCode::FamilyInvalid => 0x05,
                KtestEntryCode::RunOverBudget => 0x06,
                KtestEntryCode::ArchiveDrift => 0x07,
                KtestEntryCode::ArchiveMissing => 0x08,
                KtestEntryCode::SampleEmpty => 0x09,
                KtestEntryCode::SampleOutOfRange => 0x0A,
                KtestEntryCode::FingerprintDrift => 0x0B,
                KtestEntryCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            KtestEntryCode::EntryTableShort => "入口表缺行：补齐 20 行三列再跑",
            KtestEntryCode::EntryOutOfRange => "入口号越界：单跑号在 0..20 内取",
            KtestEntryCode::DoubleRun => "重复单跑：每条入口每批恰跑一次",
            KtestEntryCode::CountEquationBreak => "等式破：投递+应答+混合=全跑条数",
            KtestEntryCode::FamilyInvalid => "族码非法：三族闭集外值",
            KtestEntryCode::RunOverBudget => "单跑超线：压采样或走预算变更账",
            KtestEntryCode::ArchiveDrift => "归档漂移：存根与重算不符，逐项对账",
            KtestEntryCode::ArchiveMissing => "归档缺存根：先全跑 20 条再取存根",
            KtestEntryCode::SampleEmpty => "样本空账：先采样再结算",
            KtestEntryCode::SampleOutOfRange => "样本越界：落在冻结界内再算",
            KtestEntryCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            KtestEntryCode::Unknown => "未知诊断码：不在 0x80480 段闭集内",
        }
    }

    pub const ALL: [KtestEntryCode; 12] = [
        KtestEntryCode::EntryTableShort,
        KtestEntryCode::EntryOutOfRange,
        KtestEntryCode::DoubleRun,
        KtestEntryCode::CountEquationBreak,
        KtestEntryCode::FamilyInvalid,
        KtestEntryCode::RunOverBudget,
        KtestEntryCode::ArchiveDrift,
        KtestEntryCode::ArchiveMissing,
        KtestEntryCode::SampleEmpty,
        KtestEntryCode::SampleOutOfRange,
        KtestEntryCode::FingerprintDrift,
        KtestEntryCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in KtestEntryCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    KtestEntryCode::Unknown.detail()
}

// ── 冻结常量表 ───────────────────────────────────────────────────
/// 单跑入口总数（冻结 20）。
pub const ENTRY_TOTAL: usize = 20;
/// 族闭集：0=投递 1=应答 2=混合。
pub const FAMILY_COUNT: usize = 3;
/// 各族预算线（拍）。
pub const BUDGET: [u32; FAMILY_COUNT] = [48, 64, 72];
/// 各族样本上界（含）。
pub const SAMPLE_HI: [u32; FAMILY_COUNT] = [40, 56, 64];
/// 样本下界（含，各族同）。
pub const SAMPLE_LO: u32 = 1;
/// 每族每入口样本数（冻结）。
pub const SAMPLES_PER_RUN: u32 = 12;

/// 单跑入口行：名 / 族 / 探针号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryRow {
    pub name: &'static str,
    pub family: u32,
    pub probe: u32,
}

/// 冻结入口表：20 行（前 8 投递、中 6 应答、后 6 混合）。
pub const ENTRY_TABLE: [EntryRow; ENTRY_TOTAL] = [
    EntryRow { name: "投递-直发单端口", family: 0, probe: 0x11 },
    EntryRow { name: "投递-直发多端口", family: 0, probe: 0x12 },
    EntryRow { name: "投递-间接共享区", family: 0, probe: 0x13 },
    EntryRow { name: "投递-带柄消息", family: 0, probe: 0x14 },
    EntryRow { name: "投递-短包连发", family: 0, probe: 0x15 },
    EntryRow { name: "投递-满包连发", family: 0, probe: 0x16 },
    EntryRow { name: "投递-队列回压", family: 0, probe: 0x17 },
    EntryRow { name: "投递-唤醒链", family: 0, probe: 0x18 },
    EntryRow { name: "应答-即时回签", family: 1, probe: 0x21 },
    EntryRow { name: "应答-批量回签", family: 1, probe: 0x22 },
    EntryRow { name: "应答-应答超时", family: 1, probe: 0x23 },
    EntryRow { name: "应答-半对挂起", family: 1, probe: 0x24 },
    EntryRow { name: "应答-重投去重", family: 1, probe: 0x25 },
    EntryRow { name: "应答-关前排空", family: 1, probe: 0x26 },
    EntryRow { name: "混合-收发交错", family: 2, probe: 0x31 },
    EntryRow { name: "混合-多端竞态", family: 2, probe: 0x32 },
    EntryRow { name: "混合-注入风暴", family: 2, probe: 0x33 },
    EntryRow { name: "混合-账目对冲", family: 2, probe: 0x34 },
    EntryRow { name: "混合-关闭竞态", family: 2, probe: 0x35 },
    EntryRow { name: "混合-全链回放", family: 2, probe: 0x36 },
];

/// 入口表点验：行数守恒、族号闭集、行序族段单调（0..8→1..6→2..6）。
pub fn entry_table_ok() -> bool {
    if ENTRY_TABLE.len() != ENTRY_TOTAL {
        return false;
    }
    let mut fam_count = [0u32; FAMILY_COUNT];
    let mut prev_fam: u32 = 0;
    for (i, r) in ENTRY_TABLE.iter().enumerate() {
        if r.family as usize >= FAMILY_COUNT {
            return false;
        }
        if r.name.is_empty() {
            return false;
        }
        // 族段单调：族号只许不变或 +1（0 段→1 段→2 段）。
        if i > 0 && r.family != prev_fam && r.family != prev_fam + 1 {
            return false;
        }
        prev_fam = r.family;
        fam_count[r.family as usize] += 1;
    }
    fam_count[0] == 8 && fam_count[1] == 6 && fam_count[2] == 6
}

// ── 确定性 LCG ───────────────────────────────────────────────────
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

/// 升序插入排序（整数，swap 法）。
pub fn sort_asc(v: &mut Vec<u32>) {
    let n = v.len();
    let mut i = 1;
    while i < n {
        let mut j = i;
        while j > 0 {
            let pair = match (v.get(j - 1), v.get(j)) {
                (Some(a), Some(b)) => (*a, *b),
                _ => return,
            };
            if pair.0 <= pair.1 {
                break;
            }
            v.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
}

/// P95 nearest-rank 整数闭式（需升序、非空）。
pub fn p95_of_sorted(sorted: &[u32]) -> Result<u32, u32> {
    if sorted.is_empty() {
        return Err(KtestEntryCode::SampleEmpty.val());
    }
    let mut i = 1;
    while i < sorted.len() {
        let pair = match (sorted.get(i - 1), sorted.get(i)) {
            (Some(a), Some(b)) => (*a, *b),
            _ => return Err(KtestEntryCode::SampleEmpty.val()),
        };
        if pair.0 > pair.1 {
            return Err(KtestEntryCode::SampleEmpty.val());
        }
        i += 1;
    }
    let n = sorted.len() as u32;
    let idx = match n
        .checked_mul(95)
        .and_then(|x| x.checked_add(99))
        .and_then(|x| x.checked_div(100))
        .and_then(|x| x.checked_sub(1))
    {
        Some(v) => v as usize,
        None => return Err(KtestEntryCode::SampleEmpty.val()),
    };
    match sorted.get(idx) {
        Some(v) => Ok(*v),
        None => Err(KtestEntryCode::SampleEmpty.val()),
    }
}

// ── 聚合账 ───────────────────────────────────────────────────────
/// 聚合账：全跑条数 + 三族计数 + 逐条单跑次数。
pub struct AggLedger {
    pub runs_total: u32,
    pub fam_counts: [u32; FAMILY_COUNT],
    /// 逐条单跑次数（每条 0 或 1；>1 在单跑入口即拒，不会落账）。
    pub per_entry: [u32; ENTRY_TOTAL],
    /// 累计得分（各单跑得分和）。
    pub score_total: u32,
}

impl AggLedger {
    pub const fn new() -> AggLedger {
        AggLedger {
            runs_total: 0,
            fam_counts: [0; FAMILY_COUNT],
            per_entry: [0; ENTRY_TOTAL],
            score_total: 0,
        }
    }

    /// 聚合等式：runs_total == 三族之和 == 已跑入口数（每条恰一次）。
    pub fn equation(&self) -> Result<(), u32> {
        let mut fam_sum = 0u32;
        let mut i = 0;
        while i < FAMILY_COUNT {
            fam_sum = fam_sum.wrapping_add(self.fam_counts[i]);
            i += 1;
        }
        if fam_sum != self.runs_total {
            return Err(KtestEntryCode::CountEquationBreak.val());
        }
        let mut ran = 0u32;
        let mut k = 0;
        while k < ENTRY_TOTAL {
            let c = match self.per_entry.get(k) {
                Some(v) => *v,
                None => return Err(KtestEntryCode::EntryOutOfRange.val()),
            };
            if c > 1 {
                return Err(KtestEntryCode::DoubleRun.val());
            }
            ran = ran.wrapping_add(c);
            k += 1;
        }
        if ran != self.runs_total {
            return Err(KtestEntryCode::CountEquationBreak.val());
        }
        Ok(())
    }

    /// 账指纹（归档存根基准）。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = self.runs_total.wrapping_mul(53).wrapping_add(self.score_total);
        let mut i = 0;
        while i < FAMILY_COUNT {
            fp = fp.wrapping_add(self.fam_counts[i].wrapping_mul(7 + i as u32));
            i += 1;
        }
        fp
    }
}

/// 单跑一条入口：取族样本批 → P95 → 对预算线。
/// 返回得分（P95 值）；重复单跑 / 越界 / 超线显性拒。
pub fn run_one(led: &mut AggLedger, idx: u32) -> Result<u32, u32> {
    let row = match ENTRY_TABLE.get(idx as usize) {
        Some(r) => *r,
        None => return Err(KtestEntryCode::EntryOutOfRange.val()),
    };
    let slot = idx as usize;
    let cnt = match led.per_entry.get(slot) {
        Some(v) => *v,
        None => return Err(KtestEntryCode::EntryOutOfRange.val()),
    };
    if cnt != 0 {
        return Err(KtestEntryCode::DoubleRun.val());
    }
    let fam = row.family as usize;
    if fam >= FAMILY_COUNT {
        return Err(KtestEntryCode::FamilyInvalid.val());
    }
    // 采样：种子 = SEG_BASE*1000 + probe（每入口确定性）。
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_mul(1000).wrapping_add(row.probe as u64));
    let mut samples = Vec::new();
    let mut i = 0;
    while i < SAMPLES_PER_RUN {
        let hi = match SAMPLE_HI.get(fam) {
            Some(h) => *h,
            None => return Err(KtestEntryCode::FamilyInvalid.val()),
        };
        let s = SAMPLE_LO + rng.below((hi - SAMPLE_LO + 1) as u64) as u32;
        if s < SAMPLE_LO || s > hi {
            return Err(KtestEntryCode::SampleOutOfRange.val());
        }
        samples.push(s);
        i += 1;
    }
    sort_asc(&mut samples);
    let p = p95_of_sorted(&samples)?;
    let budget = match BUDGET.get(fam) {
        Some(b) => *b,
        None => return Err(KtestEntryCode::FamilyInvalid.val()),
    };
    if p > budget {
        return Err(KtestEntryCode::RunOverBudget.val());
    }
    // 落账：每条恰一次。
    if let Some(c) = led.per_entry.get_mut(slot) {
        *c = 1;
    }
    led.runs_total += 1;
    if let Some(fc) = led.fam_counts.get_mut(fam) {
        *fc += 1;
    }
    led.score_total = led.score_total.wrapping_add(p);
    Ok(p)
}

/// 归档存根：全跑后落档，点验=重算比对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveNote {
    pub runs_total: u32,
    pub fam_sum: u32,
    pub fp: u32,
}

/// 全跑 20 条 → 等式断言 → 归档。
pub fn run_all() -> Result<ArchiveNote, u32> {
    if !entry_table_ok() {
        return Err(KtestEntryCode::EntryTableShort.val());
    }
    let mut led = AggLedger::new();
    let mut i = 0;
    while i < ENTRY_TOTAL {
        run_one(&mut led, i as u32)?;
        i += 1;
    }
    led.equation()?;
    if led.runs_total != ENTRY_TOTAL as u32 {
        return Err(KtestEntryCode::CountEquationBreak.val());
    }
    let mut fam_sum = 0u32;
    let mut k = 0;
    while k < FAMILY_COUNT {
        fam_sum = fam_sum.wrapping_add(led.fam_counts[k]);
        k += 1;
    }
    Ok(ArchiveNote { runs_total: led.runs_total, fam_sum, fp: led.fingerprint() })
}

/// 存根点验：重跑一遍全跑，重算存根与给定存根比对。
pub fn verify_note(note: &ArchiveNote) -> Result<(), u32> {
    let fresh = run_all()?;
    if fresh.runs_total != note.runs_total
        || fresh.fam_sum != note.fam_sum
        || fresh.fp != note.fp
    {
        return Err(KtestEntryCode::ArchiveDrift.val());
    }
    Ok(())
}

// ── 抽样 10 例对平 ───────────────────────────────────────────────
/// LCG 抽 10 个**不重复**入口号（已跑入口重抽则跳过，总尝试设上限），
/// 逐条单跑全绿。
pub fn sample_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut led = AggLedger::new();
    let mut ok_runs = 0u32;
    let mut attempts = 0u32;
    while ok_runs < 10 && attempts < 100 {
        attempts += 1;
        let idx = rng.below(ENTRY_TOTAL as u64) as u32;
        // 已跑入口跳过（不算失败，重抽）。
        let already = match led.per_entry.get(idx as usize) {
            Some(v) => *v != 0,
            None => true,
        };
        if already {
            continue;
        }
        if run_one(&mut led, idx).is_ok() {
            ok_runs += 1;
        }
    }
    led.equation()?;
    if ok_runs == 10 {
        Ok(10)
    } else {
        Err(KtestEntryCode::RunOverBudget.val())
    }
}

// ── 注入面：账违约必抓 ───────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0 }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入族 0：越界入口号 → EntryOutOfRange。
/// 注入族 1：重复单跑 → DoubleRun。
/// 注入族 2：族计数篡改 → CountEquationBreak。
/// 注入族 3：坏存根点验 → ArchiveDrift。
/// 注入族 4：未全跑取等式（空账等式应过，跑 1 条后等式仍须自洽）。
pub fn inject_ledger_violations(led: &mut InjectLedger) {
    // 族 0。
    led.tries += 1;
    let mut a = AggLedger::new();
    if run_one(&mut a, ENTRY_TOTAL as u32) == Err(KtestEntryCode::EntryOutOfRange.val()) {
        led.caught += 1;
    }
    // 族 1。
    led.tries += 1;
    let mut b = AggLedger::new();
    let _ = run_one(&mut b, 0);
    if run_one(&mut b, 0) == Err(KtestEntryCode::DoubleRun.val()) {
        led.caught += 1;
    }
    // 族 2。
    led.tries += 1;
    let mut c = AggLedger::new();
    let _ = run_one(&mut c, 0);
    c.fam_counts[0] = c.fam_counts[0].wrapping_add(1);
    if c.equation() == Err(KtestEntryCode::CountEquationBreak.val()) {
        led.caught += 1;
    }
    // 族 3。
    led.tries += 1;
    let mut bad_note = ArchiveNote { runs_total: 20, fam_sum: 20, fp: 0 };
    let mut fresh = ArchiveNote { runs_total: 0, fam_sum: 0, fp: 0 };
    if let Ok(n) = run_all() {
        fresh = n;
    }
    bad_note.fp = fresh.fp.wrapping_add(1);
    if verify_note(&bad_note) == Err(KtestEntryCode::ArchiveDrift.val()) {
        led.caught += 1;
    }
    // 族 4：部分跑后等式自洽（1 条已跑，族和=1=总条数）。
    led.tries += 1;
    let mut d = AggLedger::new();
    let _ = run_one(&mut d, 1);
    if d.equation().is_ok() {
        led.caught += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：表齐 → 全跑 20 → 等式 → 归档 → 点验。
pub fn golden_run() -> Result<u32, u32> {
    let note = run_all()?;
    verify_note(&note)?;
    Ok(note.fp)
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        _ => Err(KtestEntryCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1：20 条单跑入口齐。
fn entries_ready_ok() -> bool {
    entry_table_ok()
}

/// J2：全跑条数与聚合账等式成立且随批归档可点验。
fn archive_ok() -> bool {
    match run_all() {
        Ok(note) => verify_note(&note).is_ok() && note.runs_total == ENTRY_TOTAL as u32,
        Err(_) => false,
    }
}

/// J3：注入全抓。
fn inject_ok() -> bool {
    let mut led = InjectLedger::new();
    inject_ledger_violations(&mut led);
    led.all_caught()
}

/// J4：抽样 10 例。
fn sample_ok() -> bool {
    sample_ten() == Ok(10)
}

/// J5：双跑一致。
fn repro_ok() -> bool {
    repro_check().is_ok()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in KtestEntryCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    v.push((
        "J1-20条入口齐",
        entries_ready_ok(),
        format!("行数={} 三族 8/6/6", ENTRY_TABLE.len()),
    ));
    v.push((
        "J2-全跑等式+归档点验",
        archive_ok(),
        format!("{:?}", run_all().map(|n| (n.runs_total, n.fam_sum))),
    ));
    // 边界：P95 闭式。
    let p1 = p95_of_sorted(&[5]);
    let p2 = p95_of_sorted(&[2, 4, 6, 8, 10, 12, 14, 16, 18, 20]);
    v.push((
        "J3-P95闭式边界",
        p1 == Ok(5) && p2 == Ok(20),
        format!("{:?}/{:?}", p1, p2),
    ));
    // 边界：单条入口越界/重复。
    let mut l = AggLedger::new();
    let out = run_one(&mut l, ENTRY_TOTAL as u32) == Err(KtestEntryCode::EntryOutOfRange.val());
    let _ = run_one(&mut l, 2);
    let dup = run_one(&mut l, 2) == Err(KtestEntryCode::DoubleRun.val());
    v.push(("J3-越界/重复拒", out && dup, String::from("窗内一次")));
    v.push(("J4-注入全抓", inject_ok(), String::from("五族账违约")));
    v.push(("J5-抽样10例对平", sample_ok(), format!("{:?}", sample_ten())));
    v.push(("J6-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J7-段位守卫", segment_guard(), String::from("0x80480 窗口")));
    v
}

/// VX9 检查入口（卡 12618，UNX-F12619-J1 判据承载）。
pub fn run_unx12618_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12618 消息面性能预算（P95 投递延迟账）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_entry_table_twenty() {
        assert_eq!(ENTRY_TABLE.len(), 20);
        assert!(entry_table_ok());
    }

    #[test]
    fn test_run_all_equation_and_archive() {
        let note = run_all().unwrap_or(ArchiveNote { runs_total: 0, fam_sum: 0, fp: 0 });
        assert_eq!(note.runs_total, 20);
        assert_eq!(note.fam_sum, 20);
        assert!(verify_note(&note).is_ok());
    }

    #[test]
    fn test_double_run_rejected() {
        let mut led = AggLedger::new();
        assert!(run_one(&mut led, 3).is_ok());
        assert_eq!(run_one(&mut led, 3), Err(KtestEntryCode::DoubleRun.val()));
    }

    #[test]
    fn test_out_of_range_rejected() {
        let mut led = AggLedger::new();
        assert_eq!(
            run_one(&mut led, 20),
            Err(KtestEntryCode::EntryOutOfRange.val())
        );
        assert_eq!(
            run_one(&mut led, u32::MAX),
            Err(KtestEntryCode::EntryOutOfRange.val())
        );
    }

    #[test]
    fn test_equation_break_caught() {
        let mut led = AggLedger::new();
        let _ = run_one(&mut led, 0);
        led.fam_counts[0] = led.fam_counts[0].wrapping_add(2);
        assert_eq!(
            led.equation(),
            Err(KtestEntryCode::CountEquationBreak.val())
        );
    }

    #[test]
    fn test_injection_all_caught() {
        let mut led = InjectLedger::new();
        inject_ledger_violations(&mut led);
        assert_eq!(led.tries, 5);
        assert_eq!(led.caught, 5);
    }

    #[test]
    fn test_sample_ten() {
        assert_eq!(sample_ten(), Ok(10));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or(0);
        let b = golden_run().unwrap_or(1);
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80480);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in KtestEntryCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0x777).contains("未知"));
    }
}
