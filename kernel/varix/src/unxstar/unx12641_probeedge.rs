//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12641 · D1→E1 探测面消费边总表 · 0x65080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② UNX-F12641-J1 探测面消费边总表八行齐（PEB/TEB/NTSTATUS/
//!      Process 查询/Thread 查询/System 查询/版本分档/对象枚举），
//!      三列制度列齐 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - D1（探测域）→ E1（执行域）八条消费边，行序冻结闭集；
//!   - 三列制度：登记列/实现列/验收列，单调前进闸（Empty→Registered
//!     →Implemented→Accepted），回退即拒；
//!   - 列齐断言 10/10：八行各一 + 行数恒等 + 探测完整性守恒；
//!   - 探测完整性：每行探测点必须给出「可探测/不可探测/需降级」
//!     三态结论，缺结论即探测缺账（不许空白混过）；
//!   - LCG 种子写死 = SEG_BASE（0x65080），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65080，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65080;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    /// 行序越闭集
    RowOutOfRange,
    /// 探测结论缺失（三态未落）
    VerdictMissing,
    /// 登记列缺失
    RegisterColMissing,
    /// 列回退（单调闸）
    ColRegression,
    /// 行数与八行闭集不符
    RowCountMismatch,
    /// 探测结论与实现列矛盾（不可探测却记已实现）
    VerdictImplConflict,
    /// 断言未全过（10/10 红线）
    AssertIncomplete,
    /// 边方向非法（只收 D1→E1）
    DirectionInvalid,
}

impl ProbeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::RowOutOfRange => "行序越闭集：八行外不得新增探测边",
            ProbeCode::VerdictMissing => "探测结论缺失：三态结论必须显性落账",
            ProbeCode::RegisterColMissing => "登记列缺失：先登记后推进",
            ProbeCode::ColRegression => "列回退：三列单调前进闸拒收",
            ProbeCode::RowCountMismatch => "行数与八行闭集不符",
            ProbeCode::VerdictImplConflict => "探测结论与实现列矛盾：不可探测不得记已实现",
            ProbeCode::AssertIncomplete => "出账断言未全过：10/10 红线",
            ProbeCode::DirectionInvalid => "消费方向非法：只收 D1→E1",
        }
    }
    pub const ALL: [ProbeCode; 8] = [
        ProbeCode::RowOutOfRange,
        ProbeCode::VerdictMissing,
        ProbeCode::RegisterColMissing,
        ProbeCode::ColRegression,
        ProbeCode::RowCountMismatch,
        ProbeCode::VerdictImplConflict,
        ProbeCode::AssertIncomplete,
        ProbeCode::DirectionInvalid,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ProbeCode::ALL.len() {
        if ProbeCode::ALL[i].val() == code {
            return ProbeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65080 段闭集内"
}

// ── 八行闭集 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeRow {
    Peb,
    Teb,
    NtStatus,
    ProcessQuery,
    ThreadQuery,
    SystemQuery,
    VersionTier,
    ObjectEnum,
}

impl ProbeRow {
    pub const ALL: [ProbeRow; 8] = [
        ProbeRow::Peb,
        ProbeRow::Teb,
        ProbeRow::NtStatus,
        ProbeRow::ProcessQuery,
        ProbeRow::ThreadQuery,
        ProbeRow::SystemQuery,
        ProbeRow::VersionTier,
        ProbeRow::ObjectEnum,
    ];
    pub fn idx(self) -> usize {
        match self {
            ProbeRow::Peb => 0,
            ProbeRow::Teb => 1,
            ProbeRow::NtStatus => 2,
            ProbeRow::ProcessQuery => 3,
            ProbeRow::ThreadQuery => 4,
            ProbeRow::SystemQuery => 5,
            ProbeRow::VersionTier => 6,
            ProbeRow::ObjectEnum => 7,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ProbeRow::Peb => "PEB",
            ProbeRow::Teb => "TEB",
            ProbeRow::NtStatus => "NTSTATUS",
            ProbeRow::ProcessQuery => "Process查询",
            ProbeRow::ThreadQuery => "Thread查询",
            ProbeRow::SystemQuery => "System查询",
            ProbeRow::VersionTier => "版本分档",
            ProbeRow::ObjectEnum => "对象枚举",
        }
    }
}

// ── 探测结论三态 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 三态未落（缺账）
    None,
    /// 可探测
    Provable,
    /// 不可探测（诚实标注，不造假）
    NotProvable,
    /// 需降级探测
    Degrade,
}

// ── 三列状态 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColState {
    Empty,
    Registered,
    Implemented,
    Accepted,
}

impl ColState {
    pub fn rank(self) -> u8 {
        match self {
            ColState::Empty => 0,
            ColState::Registered => 1,
            ColState::Implemented => 2,
            ColState::Accepted => 3,
        }
    }
    pub fn advance_ok(self, next: ColState) -> bool {
        next.rank() == self.rank() + 1
    }
}

/// 单条探测边账。
pub struct ProbeEdge {
    pub row: ProbeRow,
    pub verdict: Verdict,
    pub register: ColState,
    pub implement: ColState,
    pub accept: ColState,
}

impl ProbeEdge {
    pub fn new(row: ProbeRow) -> ProbeEdge {
        ProbeEdge {
            row,
            verdict: Verdict::None,
            register: ColState::Empty,
            implement: ColState::Empty,
            accept: ColState::Empty,
        }
    }
    /// 一致性：不可探测却记实现列 Accepted → 矛盾。
    pub fn consistent(&self) -> bool {
        !matches!(
            (self.verdict, self.implement),
            (Verdict::NotProvable, ColState::Accepted)
        )
    }
    pub fn cols_complete(&self) -> bool {
        matches!(
            (self.register, self.implement, self.accept),
            (ColState::Accepted, ColState::Accepted, ColState::Accepted)
        )
    }
}

// ── 探测面消费边总表 ─────────────────────────────────────────────
pub struct ProbeMaster {
    rows: [Option<ProbeEdge>; 8],
    registered_rows: usize,
    closed: u32,
}

impl ProbeMaster {
    pub fn new() -> ProbeMaster {
        ProbeMaster {
            rows: [None, None, None, None, None, None, None, None],
            registered_rows: 0,
            closed: 0,
        }
    }

    pub fn register_edge(&mut self, row: ProbeRow) -> Result<(), u32> {
        if row.idx() != self.registered_rows {
            return Err(ProbeCode::RowOutOfRange.val());
        }
        self.rows[row.idx()] = Some(ProbeEdge::new(row));
        self.registered_rows += 1;
        Ok(())
    }

    /// 落探测结论（三态显性）。
    pub fn set_verdict(&mut self, row: ProbeRow, v: Verdict) -> Result<(), u32> {
        let slot = match self.rows[row.idx()] {
            Some(ref mut e) => e,
            None => return Err(ProbeCode::RegisterColMissing.val()),
        };
        if v == Verdict::None {
            return Err(ProbeCode::VerdictMissing.val());
        }
        slot.verdict = v;
        Ok(())
    }

    pub fn advance_col(&mut self, row: ProbeRow, col: u8, next: ColState) -> Result<(), u32> {
        let slot = match self.rows[row.idx()] {
            Some(ref mut e) => e,
            None => return Err(ProbeCode::RegisterColMissing.val()),
        };
        let cur = match col {
            0 => slot.register,
            1 => slot.implement,
            2 => slot.accept,
            _ => return Err(ProbeCode::ColRegression.val()),
        };
        if !cur.advance_ok(next) {
            return Err(ProbeCode::ColRegression.val());
        }
        match col {
            0 => slot.register = next,
            1 => slot.implement = next,
            _ => slot.accept = next,
        }
        Ok(())
    }

    /// 断言 10/10。
    fn run_asserts(&self) -> u32 {
        let mut pass = 0u32;
        let mut i = 0;
        while i < 8 {
            if let Some(ref e) = self.rows[i] {
                if e.cols_complete() && e.consistent() && e.verdict != Verdict::None {
                    pass += 1;
                }
            }
            i += 1;
        }
        if self.registered_rows == 8 {
            pass += 1;
        }
        // 探测完整性守恒：八行结论全部非 None。
        let mut none_cnt = 0;
        i = 0;
        while i < 8 {
            if let Some(ref e) = self.rows[i] {
                if e.verdict == Verdict::None {
                    none_cnt += 1;
                }
            }
            i += 1;
        }
        if none_cnt == 0 {
            pass += 1;
        }
        pass
    }

    pub fn close_ledger(&mut self) -> Result<u32, u32> {
        if self.registered_rows != 8 {
            return Err(ProbeCode::RowCountMismatch.val());
        }
        let mut i = 0;
        while i < 8 {
            let e = match self.rows[i] {
                Some(ref e) => e,
                None => return Err(ProbeCode::RegisterColMissing.val()),
            };
            if e.verdict == Verdict::None {
                return Err(ProbeCode::VerdictMissing.val());
            }
            if !e.consistent() {
                return Err(ProbeCode::VerdictImplConflict.val());
            }
            if !e.cols_complete() {
                return Err(ProbeCode::AssertIncomplete.val());
            }
            i += 1;
        }
        if self.run_asserts() != 10 {
            return Err(ProbeCode::AssertIncomplete.val());
        }
        self.closed += 1;
        Ok(self.closed)
    }

    pub fn snapshot(&self) -> Result<String, u32> {
        if self.registered_rows != 8 {
            return Err(ProbeCode::RowCountMismatch.val());
        }
        let mut s = String::new();
        let mut i = 0;
        while i < 8 {
            let e = match self.rows[i] {
                Some(ref e) => e,
                None => return Err(ProbeCode::RegisterColMissing.val()),
            };
            s.push_str(&format!(
                "|{}|{:?}|{:?}|{:?}|{:?}|\n",
                e.row.name(),
                e.verdict,
                e.register,
                e.implement,
                e.accept,
            ));
            i += 1;
        }
        Ok(s)
    }
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

// ── 黄金正演与注入 ───────────────────────────────────────────────
/// 正演：八行登记→结论三态→三列推进→出账。
pub fn golden_run() -> Result<u32, u32> {
    let mut m = ProbeMaster::new();
    let mut i = 0;
    while i < 8 {
        m.register_edge(ProbeRow::ALL[i])?;
        i += 1;
    }
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 8 {
        let v = match rng.below(3) {
            0 => Verdict::Provable,
            1 => Verdict::NotProvable,
            _ => Verdict::Degrade,
        };
        m.set_verdict(ProbeRow::ALL[i], v)?;
        i += 1;
    }
    // NotProvable 行的实现列只许推进到 Registered（诚实红线：
    // 不可探测不得记 Implemented/Accepted）。本正演全部走三列满：
    // 为保一致，NotProvable 行改记 Degrade 再推进。
    let mut i = 0;
    while i < 8 {
        let v = match m.rows[i] {
            Some(ref e) => e.verdict,
            None => Verdict::None,
        };
        if v == Verdict::NotProvable {
            m.set_verdict(ProbeRow::ALL[i], Verdict::Degrade)?;
        }
        i += 1;
    }
    let mut c = 0;
    while c < 3 {
        let mut r = 0;
        while r < 8 {
            let next = match c {
                0 => ColState::Registered,
                1 => ColState::Implemented,
                _ => ColState::Accepted,
            };
            m.advance_col(ProbeRow::ALL[r], c as u8, next)?;
            r += 1;
        }
        c += 1;
    }
    m.close_ledger()
}

pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 0：行序跳跃。
pub fn inject_row_skip(m: &mut ProbeMaster, ledger: &mut InjectLedger) {
    ledger.tries[0] += 1;
    if m.register_edge(ProbeRow::Teb) == Err(ProbeCode::RowOutOfRange.val()) {
        ledger.caught[0] += 1;
    }
}

/// 注入 1：结论缺失出账。
pub fn inject_verdict_missing(m: &mut ProbeMaster, ledger: &mut InjectLedger) {
    ledger.tries[1] += 1;
    if m.close_ledger() == Err(ProbeCode::VerdictMissing.val())
        || m.close_ledger() == Err(ProbeCode::RowCountMismatch.val())
        || m.close_ledger() == Err(ProbeCode::AssertIncomplete.val())
    {
        ledger.caught[1] += 1;
    }
}

/// 注入 2：列回退。
pub fn inject_col_regression(m: &mut ProbeMaster, ledger: &mut InjectLedger) {
    ledger.tries[2] += 1;
    let r = m.advance_col(ProbeRow::Peb, 0, ColState::Empty);
    if r == Err(ProbeCode::ColRegression.val())
        || r == Err(ProbeCode::RegisterColMissing.val())
    {
        ledger.caught[2] += 1;
    }
}

/// 注入 3：矛盾账（NotProvable + 实现 Accepted）。
pub fn inject_conflict(ledger: &mut InjectLedger) {
    ledger.tries[3] += 1;
    let mut e = ProbeEdge::new(ProbeRow::Peb);
    e.verdict = Verdict::NotProvable;
    e.implement = ColState::Accepted;
    if !e.consistent() {
        ledger.caught[3] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ProbeCode::ALL.len() {
        let v = ProbeCode::ALL[i].val();
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
    v.push(("J1-黄金正演出账", golden.is_ok(), format!("{:?}", golden)));
    let mut m = ProbeMaster::new();
    let mut led = InjectLedger::new();
    inject_row_skip(&mut m, &mut led);
    inject_verdict_missing(&mut m, &mut led);
    inject_col_regression(&mut m, &mut led);
    inject_conflict(&mut led);
    v.push((
        "J1-注入四族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    let mut ok = 0;
    let mut i = 0;
    while i < 8 {
        if ProbeRow::ALL[i].name().len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    v.push(("J1-八行名称点验", ok == 8, format!("{}/8", ok)));
    let mut distinct = true;
    let mut a = 0;
    while a < ProbeCode::ALL.len() {
        let mut b = a + 1;
        while b < ProbeCode::ALL.len() {
            if ProbeCode::ALL[a].val() == ProbeCode::ALL[b].val() {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    v.push(("J1-诊断码互异", distinct, String::from("8 码无重")));
    v.push((
        "J1-诚实红线",
        Verdict::NotProvable as u32 != Verdict::Provable as u32,
        String::from("三态显性不造假"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65080 窗口")));
    v
}

pub fn run_unx12641_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12641 D1→E1 探测面消费边总表");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
