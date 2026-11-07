//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12786 域闭账条（800 条集成账零悬空承继封账） · 0x82E80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行（契约 §2 深化下限 ≥550 行，本模块按 550+ 交付）；
//!   ② UNX-F12787-J1 800 判据 ID 全域唯一终验绿，
//!      dups 报告仅防重行自引属预期（非自引重即不绿）；
//!   ③ ID 唯一性 = 800 IDs（F12001..F12800）全互异 + 连续号段不重不漏。
//! 设计要点：
//!   - 扫描器模型：800 判据行互引全扫描 → dups 报告分两类——
//!     防重行自引（每行自引 1 次的防重护栏，属**预期**）与非自引重
//!     （真重号/错引，**不绿**）；绿判据 = 非自引重 = 0 且自引护栏恰 800；
//!   - ID 唯一性全域扫描：800 IDs 全互异 + 连续号段 F12001..F12800
//!     （位图扫描 + 号段游标双验，缺号/重号逐条记账）；
//!   - 注入面：非自引重行注入 10 轮 10/10 必抓（零静默漏检）；
//!   - 闭账状态机单调：开账 → 唯一性验证毕 → 闭账，倒退显性拒
//!     （未验证先闭账即拒、闭账后重验即拒），错误带可操作中文建议；
//!   - LCG（Numerical Recipes 32 位：st = st×1664525 + 1013904223），
//!     种子写死 = SEG_BASE（0x82E80），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic（测试区除外）；双跑一致 =
//!     同输入两次构建指纹逐域相等；中文注释与中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82E80，VX9 卡 12786 独占槽：0x80000 + 0x40 × (12786−12600)）。
pub const SEG_BASE: u32 = 0x82E80;

// ── 冻结常量表（判据原文锚定） ───────────────────────────────────
/// 判据行数冻结值：800 行。
pub const ROW_N: usize = 800;
/// 判据 ID 号段基（F12001）。
pub const ID_BASE: u32 = 12001;
/// 判据 ID 号段顶（F12800）。
pub const ID_TOP: u32 = 12800;
/// 防重行自引护栏预期总数（每行恰 1 次自引）。
pub const EXPECTED_SELF_REFS: u32 = 800;
/// 非自引重注入轮数（10/10 必抓判据）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseCode {
    /// 判据 ID 重号：全域唯一性破坏，逐行回查重号源
    IdDuplicate,
    /// 号段缺号/越段：F12001..F12800 连续段不完整
    IdGapOrRange,
    /// 非自引重：dups 报告出现防重护栏之外的引用重，属真重不绿
    NonSelfDup,
    /// 自引护栏缺失：预期 800 次自引不足额
    SelfRefMissing,
    /// 未做唯一性验证即闭账：先 verify_unique 再 close
    CloseBeforeUnique,
    /// 闭账状态倒退：已闭账禁止重验重闭，如需重开走新账期
    CloseRegress,
    /// 行缺位：800 行未建齐即扫描
    RegistryIncomplete,
    /// 扫描覆盖不足：互引全扫描覆盖量 ≠ 行数×护栏数
    ScanCoverageShort,
}

impl CloseCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CloseCode::IdDuplicate => "判据 ID 重号：全域唯一性破坏；逐行回查重号源并改号",
            CloseCode::IdGapOrRange => "号段缺号/越段：F12001..F12800 须连续不重不漏；先补缺号",
            CloseCode::NonSelfDup => "非自引重：dups 报告出现防重护栏外的引用重；修正错引行",
            CloseCode::SelfRefMissing => "自引护栏缺失：每行须恰 1 次防重自引（预期 800）",
            CloseCode::CloseBeforeUnique => "未验证唯一性即闭账：先 verify_unique 拿全域唯一结论再 close",
            CloseCode::CloseRegress => "闭账状态倒退：已闭账禁止重验重闭；重开走新账期",
            CloseCode::RegistryIncomplete => "行缺位：800 行未建齐即扫描；先补建再扫",
            CloseCode::ScanCoverageShort => "扫描覆盖不足：互引全扫描须覆盖 行数×护栏数 全量",
        }
    }
    pub const ALL: [CloseCode; 8] = [
        CloseCode::IdDuplicate,
        CloseCode::IdGapOrRange,
        CloseCode::NonSelfDup,
        CloseCode::SelfRefMissing,
        CloseCode::CloseBeforeUnique,
        CloseCode::CloseRegress,
        CloseCode::RegistryIncomplete,
        CloseCode::ScanCoverageShort,
    ];
}

/// 诊断码查表闭集入口。
pub fn detail(code: u32) -> &'static str {
    let mut found: &'static str = "未知诊断码：不在 0x82E80 段闭集内";
    for c in CloseCode::ALL.iter() {
        if c.val() == code {
            found = c.detail();
        }
    }
    found
}

// ── 闭账状态机（单调，倒退显性拒） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosePhase {
    /// 开账：行建齐、尚未唯一性验证
    Open,
    /// 唯一性验证毕：全域唯一 + 连续号段结论已落账
    UniqueVerified,
    /// 已闭账：唯一性绿 + dups 报告仅自引护栏两关全过
    Closed,
}

impl ClosePhase {
    pub fn rank(self) -> u8 {
        match self {
            ClosePhase::Open => 0,
            ClosePhase::UniqueVerified => 1,
            ClosePhase::Closed => 2,
        }
    }
    pub fn cn(self) -> &'static str {
        match self {
            ClosePhase::Open => "开账",
            ClosePhase::UniqueVerified => "唯一性验证毕",
            ClosePhase::Closed => "已闭账",
        }
    }
}

/// dups 报告（扫描器输出，逐项记账不静默）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DupReport {
    /// 防重行自引数（预期 = 800，属预期不扣绿）。
    pub self_refs: u32,
    /// 非自引重数（>0 即不绿）。
    pub non_self_dups: u32,
    /// 实扫引用总次数（全扫描覆盖量）。
    pub scanned_refs: u32,
    /// 重号 ID 计数（唯一性扫描产出）。
    pub dup_ids: u32,
}

impl DupReport {
    /// dups 报告绿判据：非自引重 = 0 且自引护栏恰 800。
    pub fn green(&self) -> bool {
        self.non_self_dups == 0 && self.self_refs == EXPECTED_SELF_REFS
    }
}

/// 判据行：ID + 防重自引护栏 + 额外引用集（黄金行额外引用恒空）。
#[derive(Clone, Debug)]
pub struct CriteriaRow {
    /// 判据 ID F12001..F12800。
    pub id: u32,
    /// 防重行自引护栏（true = 该行已挂自引护栏）。
    pub guard_self: bool,
    /// 额外引用目标（注入面/错引语料，黄金行恒空）。
    pub extra_refs: Vec<u32>,
}

/// 域闭账条账本。
pub struct CloseBar {
    /// 判据行（800 行，号连续）。
    pub rows: Vec<CriteriaRow>,
    /// 闭账状态机。
    pub phase: ClosePhase,
    /// 历次扫描累计非自引重数（显性记账）。
    pub nonself_seen: u32,
}

impl CloseBar {
    pub fn new() -> CloseBar {
        CloseBar {
            rows: Vec::new(),
            phase: ClosePhase::Open,
            nonself_seen: 0,
        }
    }

    /// 黄金账本：800 行判据 ID 连续不重，每行挂 1 次防重自引护栏。
    pub fn golden() -> CloseBar {
        let mut bar = CloseBar::new();
        let mut id = ID_BASE;
        let mut i = 0usize;
        while i < ROW_N {
            bar.rows.push(CriteriaRow {
                id,
                guard_self: true,
                extra_refs: Vec::new(),
            });
            id += 1;
            i += 1;
        }
        bar
    }

    /// 唯一性全域扫描（非变更口径：位图查重 + 游标查缺，只读）。
    pub fn scan_uniqueness(&self) -> Result<(u32, u32), u32> {
        if self.rows.len() != ROW_N {
            return Err(CloseCode::RegistryIncomplete.val());
        }
        // 位图查重：号段内逐号落位，重落即重号。
        let mut seen: Vec<bool> = Vec::new();
        let mut k = 0usize;
        while k < ROW_N {
            seen.push(false);
            k += 1;
        }
        let mut dup_ids = 0u32;
        for r in self.rows.iter() {
            match r.id.checked_sub(ID_BASE) {
                Some(off) => {
                    let idx = off as usize;
                    if idx >= ROW_N {
                        // 越段号按重号口径记账（号段外不允许）。
                        dup_ids += 1;
                    } else if let Some(slot) = seen.get_mut(idx) {
                        if *slot {
                            dup_ids += 1;
                        } else {
                            *slot = true;
                        }
                    }
                }
                None => {
                    dup_ids += 1;
                }
            }
        }
        // 号段游标查缺：位图逐位必满，缺位即缺号。
        let mut gaps = 0u32;
        for (i, slot) in seen.iter().enumerate() {
            if !*slot {
                let _ = i;
                gaps += 1;
            }
        }
        Ok((dup_ids, gaps))
    }

    /// 唯一性全域扫描：位图查重 + 号段游标查缺，返回 (重号数, 缺号数)。
    ///
    /// 已闭账后重验即拒（状态倒退）；验证毕状态推进 Open→UniqueVerified。
    pub fn verify_unique(&mut self) -> Result<(u32, u32), u32> {
        if self.phase == ClosePhase::Closed {
            return Err(CloseCode::CloseRegress.val());
        }
        let res = self.scan_uniqueness()?;
        if self.phase == ClosePhase::Open {
            self.phase = ClosePhase::UniqueVerified;
        }
        Ok(res)
    }

    /// dups 报告全扫描：逐行收自引护栏与非自引重，覆盖量全记账。
    pub fn dup_scan(&mut self) -> Result<DupReport, u32> {
        if self.rows.len() != ROW_N {
            return Err(CloseCode::RegistryIncomplete.val());
        }
        let mut rep = DupReport::default();
        for r in self.rows.iter() {
            rep.scanned_refs += 1; // 自引护栏占 1 次扫描
            if r.guard_self {
                rep.self_refs += 1;
            }
            for t in r.extra_refs.iter() {
                rep.scanned_refs += 1;
                // 非自引：目标 ≠ 本行 ID 即非自引重（黄金账额外引用恒空）。
                if *t != r.id {
                    rep.non_self_dups += 1;
                }
            }
        }
        self.nonself_seen += rep.non_self_dups;
        Ok(rep)
    }

    /// 闭账：须已唯一性验证毕 + dups 报告绿；状态推进至 Closed。
    pub fn close(&mut self) -> Result<DupReport, u32> {
        if self.phase != ClosePhase::UniqueVerified {
            return Err(CloseCode::CloseBeforeUnique.val());
        }
        let (dups, gaps) = self.verify_unique()?;
        if dups != 0 {
            return Err(CloseCode::IdDuplicate.val());
        }
        if gaps != 0 {
            return Err(CloseCode::IdGapOrRange.val());
        }
        let rep = self.dup_scan()?;
        if !rep.green() {
            return Err(CloseCode::NonSelfDup.val());
        }
        self.phase = ClosePhase::Closed;
        Ok(rep)
    }

    /// 非自引重注入面：前 `n` 行各注入 1 个指向他行的错引。
    pub fn inject_nonself(&mut self, n: usize) {
        let mut k = 0usize;
        while k < n {
            if let Some(r) = self.rows.get_mut(k) {
                // 目标 = 下一行 ID（恒在号段内且 ≠ 本行，构成非自引重）。
                let next_id = if r.id == ID_TOP { ID_BASE } else { r.id + 1 };
                r.extra_refs.push(next_id);
            }
            k += 1;
        }
    }

    /// 账本指纹（双跑一致判据）。
    pub fn fingerprint(&self) -> u64 {
        let mut f: u64 = self.rows.len() as u64;
        f = f.wrapping_mul(31).wrapping_add(self.phase.rank() as u64);
        for r in self.rows.iter() {
            f = f.wrapping_mul(31).wrapping_add(r.id as u64);
            f = f.wrapping_mul(31).wrapping_add(r.guard_self as u64);
            for t in r.extra_refs.iter() {
                f = f.wrapping_mul(31).wrapping_add(*t as u64);
            }
        }
        f
    }

    /// LCG 抽检：抽中行 ID 在号段内且自引护栏在位。
    pub fn lcg_spot_check(&self, rounds: u32) -> bool {
        let mut rng = Lcg::new(SEG_BASE);
        let mut i = 0u32;
        while i < rounds {
            let idx = rng.below(ROW_N as u32) as usize;
            match self.rows.get(idx) {
                Some(r) => {
                    if r.id < ID_BASE || r.id > ID_TOP || !r.guard_self {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 矩阵可点验 10/10（A1..A10）。
    pub fn run_asserts(&mut self) -> u32 {
        let mut pass = 0u32;
        // A1：行数恰 800。
        if self.rows.len() == ROW_N {
            pass += 1;
        }
        // A2：全域唯一终验绿（重号 0、缺号 0；非变更口径，闭账后亦可点验）。
        let (dups, gaps) = self.scan_uniqueness().unwrap_or((u32::MAX, u32::MAX));
        if dups == 0 && gaps == 0 {
            pass += 1;
        }
        // A3：dups 报告仅防重行自引属预期（自引恰 800、非自引 0）。
        let rep = self.dup_scan().unwrap_or_default();
        if rep.green() {
            pass += 1;
        }
        // A4：全扫描覆盖量 = 800（每行护栏 1 次）。
        if rep.scanned_refs == ROW_N as u32 {
            pass += 1;
        }
        // A5：闭账前拒（未验证先闭账必拒）。
        let mut probe = CloseBar::golden();
        if matches!(probe.close(), Err(c) if c == CloseCode::CloseBeforeUnique.val()) {
            pass += 1;
        }
        // A6：非自引重注入 1 必抓。
        let mut probe2 = CloseBar::golden();
        let _ = probe2.verify_unique();
        probe2.inject_nonself(1);
        if probe2.dup_scan().map(|r| r.non_self_dups) == Ok(1) {
            pass += 1;
        }
        // A7：闭账后重验倒退显性拒。
        let mut probe3 = CloseBar::golden();
        let _ = probe3.verify_unique();
        if probe3.close().is_ok() && probe3.verify_unique().is_err() {
            pass += 1;
        }
        // A8：连续号段端点冻结（首 F12001、末 F12800）。
        let first = self.rows.first().map(|r| r.id);
        let last = self.rows.last().map(|r| r.id);
        if first == Some(ID_BASE) && last == Some(ID_TOP) {
            pass += 1;
        }
        // A9：LCG 确定性抽检 16 行。
        if self.lcg_spot_check(16) {
            pass += 1;
        }
        // A10：闭账终态恰 Closed。
        let mut probe4 = CloseBar::golden();
        let _ = probe4.verify_unique();
        if probe4.close().is_ok() && probe4.phase == ClosePhase::Closed {
            pass += 1;
        }
        pass
    }
}

// ── 确定性 LCG（Numerical Recipes 32 位，种子=SEG_BASE） ─────────
pub struct Lcg(u32);

impl Lcg {
    pub const fn new(seed: u32) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演：建账→全域唯一→dups 报告绿→闭账→点验 10/10 ─────────
pub fn golden_run() -> Result<u32, u32> {
    let mut bar = CloseBar::golden();
    let (dups, gaps) = bar.verify_unique()?;
    if dups != 0 || gaps != 0 {
        return Err(CloseCode::IdDuplicate.val());
    }
    let rep = bar.close()?;
    if !rep.green() {
        return Err(CloseCode::NonSelfDup.val());
    }
    Ok(bar.run_asserts())
}

// ── 非自引重注入：10 轮注入 10/10 必抓（零静默） ─────────────────
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 注入轮数。
    pub tries: u32,
    /// 必抓轮数。
    pub caught: u32,
    /// 末轮注入数。
    pub last_injected: usize,
    /// 末轮扫描报告的非自引重数。
    pub last_reported: u32,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger {
            tries: 0,
            caught: 0,
            last_injected: 0,
            last_reported: 0,
        }
    }
    /// 10/10 全抓判定。
    pub fn all_caught(&self) -> bool {
        self.tries == INJECT_ROUNDS && self.caught == INJECT_ROUNDS
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut bar = CloseBar::golden();
        let _ = bar.verify_unique();
        let n = 1usize + (r as usize % 5);
        bar.inject_nonself(n);
        ledger.last_injected = n;
        let reported = bar.dup_scan().map(|x| x.non_self_dups).unwrap_or(u32::MAX);
        ledger.last_reported = reported;
        ledger.tries += 1;
        if reported == n as u32 {
            ledger.caught += 1;
        }
        r += 1;
    }
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn unique_full_domain_ok() -> bool {
    let mut bar = CloseBar::golden();
    matches!(bar.verify_unique(), Ok((0, 0)))
}

fn dup_report_only_self_refs_ok() -> bool {
    let mut bar = CloseBar::golden();
    match bar.dup_scan() {
        Ok(rep) => rep.green(),
        Err(_) => false,
    }
}

fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

// ── 检查面（≥12 条） ─────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 判据主锚：800 判据 ID 全域唯一终验绿。
    v.push((
        "J2-800判据ID全域唯一终验绿",
        unique_full_domain_ok(),
        format!("重号0 缺号0 连续段 F{}..F{}", ID_BASE, ID_TOP),
    ));
    // 2. dups 报告仅防重行自引属预期。
    v.push((
        "J2-dups报告仅防重行自引属预期",
        dup_report_only_self_refs_ok(),
        format!("自引恰{} 非自引0", EXPECTED_SELF_REFS),
    ));
    // 3. 非自引重注入 10/10 必抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J2-非自引重注入10/10必抓",
        inj.all_caught(),
        format!("try/catch {}/{} 末轮注入{}报{}", inj.tries, inj.caught, inj.last_injected, inj.last_reported),
    ));
    // 4. 黄金正演点验 10/10。
    let golden = golden_run();
    v.push((
        "正常流-黄金正演点验10/10",
        golden == Ok(10),
        format!("{:?}", golden),
    ));
    // 5. 闭账状态机单调推进。
    let mut sm = CloseBar::golden();
    let _ = sm.verify_unique();
    let closed = sm.close().is_ok();
    v.push((
        "正常流-闭账状态机开账→验证毕→闭账",
        closed && sm.phase == ClosePhase::Closed,
        format!("终态={}", sm.phase.cn()),
    ));
    // 6. 注入必抓：未验证先闭账即拒。
    let mut p1 = CloseBar::golden();
    let before = matches!(p1.close(), Err(c) if c == CloseCode::CloseBeforeUnique.val());
    v.push((
        "注入必抓-未验证先闭账即拒",
        before,
        String::from("先 verify_unique 再 close"),
    ));
    // 7. 注入必抓：闭账后重验倒退显性拒。
    let mut p2 = CloseBar::golden();
    let _ = p2.verify_unique();
    let _ = p2.close();
    let regress = matches!(p2.verify_unique(), Err(c) if c == CloseCode::CloseRegress.val());
    v.push((
        "注入必抓-闭账后重验倒退显性拒",
        regress,
        String::from("已闭账禁止重验；重开走新账期"),
    ));
    // 8. 双跑一致：两次构建指纹逐域相等。
    let f1 = CloseBar::golden().fingerprint();
    let f2 = CloseBar::golden().fingerprint();
    v.push((
        "双跑一致-黄金账指纹相等",
        f1 == f2,
        format!("{:#x} vs {:#x}", f1, f2),
    ));
    // 9. 边界：号段端点与行数冻结。
    let g = CloseBar::golden();
    let bounds_ok = g.rows.len() == ROW_N
        && g.rows.first().map(|r| r.id) == Some(ID_BASE)
        && g.rows.last().map(|r| r.id) == Some(ID_TOP);
    v.push((
        "边界-号段F12001..F12800与行数800冻结",
        bounds_ok,
        format!("{}行", g.rows.len()),
    ));
    // 10. 重号注入必抓：人为换重一号，位图扫描必报。
    let mut p3 = CloseBar::golden();
    if let Some(r) = p3.rows.get_mut(1) {
        r.id = ID_BASE;
    }
    let dup_caught = matches!(p3.verify_unique(), Ok((d, _)) if d == 1);
    v.push((
        "注入必抓-重号1枚位图扫描必报",
        dup_caught,
        String::from("换重 F12001→重号计 1"),
    ));
    // 11. 缺号注入必抓：人为抽掉一行，行缺位/缺号两关任一必报。
    let mut p4 = CloseBar::golden();
    let _ = p4.rows.pop();
    let gap_caught = match p4.verify_unique() {
        Ok((_, gp)) => gp >= 1,
        Err(c) => c == CloseCode::RegistryIncomplete.val(),
    };
    v.push((
        "注入必抓-缺号1枚扫描必报",
        gap_caught,
        String::from("抽末行→行缺位/缺号必报其一"),
    ));
    // 12. LCG 确定性：同种子双序列逐拍相等。
    let mut r1 = Lcg::new(SEG_BASE);
    let mut r2 = Lcg::new(SEG_BASE);
    let mut lcg_eq = true;
    let mut k = 0;
    while k < 32 {
        if r1.next() != r2.next() {
            lcg_eq = false;
        }
        k += 1;
    }
    v.push((
        "LCG-同种子双序列逐拍相等",
        lcg_eq,
        format!("种子=0x{:X}", SEG_BASE),
    ));
    // 13. 诊断码闭集查表全可回。
    let mut codes_ok = true;
    for c in CloseCode::ALL.iter() {
        if detail(c.val()) != c.detail() {
            codes_ok = false;
        }
    }
    v.push((
        "诊断码-闭集查表全可回",
        codes_ok && CloseCode::ALL.len() == 8,
        format!("{}码全查表通过", CloseCode::ALL.len()),
    ));
    // 14. 段位守卫。
    v.push((
        "段位守卫-0x82E80",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    v
}

/// 判据入口：UNX-F12786 域闭账条自检集。
pub fn run_unx12786_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12786 域闭账条（800 条集成账零悬空承继封账）");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "");
            }
        }
        i += 1;
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_unique_and_closes() {
        let mut bar = CloseBar::golden();
        assert_eq!(bar.verify_unique(), Ok((0, 0)));
        let rep = bar.close();
        assert!(rep.is_ok(), "全域唯一+报告绿须能闭账");
        assert_eq!(bar.phase, ClosePhase::Closed);
    }

    #[test]
    fn dup_report_green_on_golden() {
        let mut bar = CloseBar::golden();
        let rep = bar.dup_scan().unwrap_or_default();
        assert!(rep.green());
        assert_eq!(rep.self_refs, EXPECTED_SELF_REFS);
        assert_eq!(rep.non_self_dups, 0);
    }

    #[test]
    fn nonself_injection_10_of_10() {
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert!(inj.all_caught());
        assert_eq!(inj.tries, INJECT_ROUNDS);
    }

    #[test]
    fn close_before_unique_rejected() {
        let mut bar = CloseBar::golden();
        assert_eq!(bar.close(), Err(CloseCode::CloseBeforeUnique.val()));
    }

    #[test]
    fn regress_after_close_rejected() {
        let mut bar = CloseBar::golden();
        let _ = bar.verify_unique();
        assert!(bar.close().is_ok());
        assert_eq!(bar.verify_unique(), Err(CloseCode::CloseRegress.val()));
    }

    #[test]
    fn dup_id_injection_caught() {
        let mut bar = CloseBar::golden();
        if let Some(r) = bar.rows.get_mut(2) {
            // 把第 3 行改重为 F12001：F12001 重号 1，同时 F12003 缺号 1
            //（改号既造重又造缺，两关都须报）。
            r.id = ID_BASE;
        }
        assert_eq!(bar.verify_unique(), Ok((1, 1)), "重号1+缺号1 两关同报");
    }

    #[test]
    fn gap_injection_caught() {
        let mut bar = CloseBar::golden();
        let _ = bar.rows.pop();
        match bar.verify_unique() {
            Ok((_, gaps)) => assert!(gaps >= 1, "缺号必报"),
            Err(e) => assert_eq!(e, CloseCode::RegistryIncomplete.val(), "行缺位也可拒扫"),
        }
    }

    #[test]
    fn double_run_fingerprint_equal() {
        let a = CloseBar::golden().fingerprint();
        let b = CloseBar::golden().fingerprint();
        assert_eq!(a, b);
    }

    #[test]
    fn detail_lookup_closed_set() {
        for c in CloseCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
    }

    #[test]
    fn segment_guard_pass() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x82E80);
    }

    #[test]
    fn golden_asserts_full_score() {
        let mut bar = CloseBar::golden();
        let _ = bar.verify_unique();
        assert_eq!(bar.run_asserts(), 10, "可点验须 10/10");
    }

    #[test]
    fn lcg_deterministic_same_seed() {
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        for _ in 0..32 {
            assert_eq!(a.next(), b.next());
        }
    }
}
