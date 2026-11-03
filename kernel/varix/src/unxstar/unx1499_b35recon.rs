//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1499 · B35 批内对账（断言资产互联对账总成） · 0x76180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批6-N）：
//!   B35 批内 19 条判据互联对账（对角自核+互查矩阵），互联矩阵可查
//!   可导，断言 ≥40 大关
//! 设计要点：
//!   1. 判据数冻结 19（B35_J01..J19），互联矩阵 19×19 定容；
//!   2. 对角自核：M[i][i]=1 全对角常绿（自核破位即 DiagBreak）；
//!   3. 互查对称：M[i][j]=M[j][i] 全对称（互查失配即 SymBreak），
//!      行和与冻结口径逐行对拍（链式 ±1 邻接 + 冻结跨接对）；
//!   4. 断言账 ≥40 大关：判据 i 断言数 = 2 + (i mod 2)（奇 3 偶 2，
//!      共 48 ≥ 40），单面虚增（任一判据断言数多一枚）必破对账；
//!   5. 矩阵可查可导：query(i,j) 越界显性拒；导出文本 19 行逐行账，
//!      行文与转置列文逐字对拍（导出即互查的文本面复算）；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x76180，AI-VX7-1104 批6-N 独占槽）。
pub const SEG_BASE: u32 = 0x76180;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 判据数（冻结 19：B35_J01..J19）。
pub const CRIT_N: usize = 19;
/// 断言数大关（冻结 ≥40）。
pub const ASSERT_GATE: u32 = 40;
/// 断言数基线（判据 i 断言数 = 2 + (i mod 2)：奇 3 偶 2）。
pub const ASSERT_BASE: u32 = 2;
/// 断言键倍率（键 = 1499×1000 + 判据×10 + 序号）。
pub const KEY_SCALE: u32 = 14990000;
/// 断言键判据位倍率（判据 ×10 + 序号）。
pub const CRIT_SCALE: u32 = 10;
/// 冻结跨接对（链式 ±1 邻接之外的互查对，1 起）。
pub const EXTRA_PAIRS: [(usize, usize); 3] = [(1, 19), (5, 12), (9, 17)];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B35Code {
    /// 坐标越界（行列超出 19×19）
    CoordOver,
    /// 对角自核破位
    DiagBreak,
    /// 互查不对称
    SymBreak,
    /// 行和失配（与冻结口径对拍不齐）
    RowSumBreak,
    /// 断言单面虚增（判据断言数越冻结口径）
    CountMismatch,
    /// 断言数不足大关（<40）
    ThresholdUnmet,
    /// 断言键重复（批内唯一性破坏）
    LedgerDup,
    /// 导出对拍失配（行文 vs 转置列文）
    ExportBreach,
    /// 状态倒退
    StateRegress,
    /// 双跑不一致
    NotReproducible,
}

impl B35Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            B35Code::CoordOver => "坐标越界：行列须在 1..=19，建议按 B35_J01..J19 闭集取号",
            B35Code::DiagBreak => "对角自核破位：M[i][i] 须恒 1，建议恢复对角自核位",
            B35Code::SymBreak => "互查不对称：M[i][j] ≠ M[j][i]，建议按对称规则重铺矩阵",
            B35Code::RowSumBreak => "行和失配：与冻结口径（链式 ±1 + 跨接对）对拍不齐，建议重算行账",
            B35Code::CountMismatch => "断言单面虚增：判据断言数越冻结口径（奇3偶2），建议按口径裁账",
            B35Code::ThresholdUnmet => "断言数不足大关：须 ≥40，建议补齐冻结断言表",
            B35Code::LedgerDup => "断言键重复：批内唯一性红线，建议按 判据×10+序号 重编",
            B35Code::ExportBreach => "导出对拍失配：行文与转置列文逐字不齐，建议重导并复算",
            B35Code::StateRegress => "状态倒退：草拟→对账→封账单向单调，建议重开新一代批账",
            B35Code::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [B35Code; 10] = [
        B35Code::CoordOver,
        B35Code::DiagBreak,
        B35Code::SymBreak,
        B35Code::RowSumBreak,
        B35Code::CountMismatch,
        B35Code::ThresholdUnmet,
        B35Code::LedgerDup,
        B35Code::ExportBreach,
        B35Code::StateRegress,
        B35Code::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < B35Code::ALL.len() {
        if let Some(c) = B35Code::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x76180 段闭集内"
}

// ── 冻结口径谓词 ────────────────────────────────────────────────
/// 判据号合法（1..=19）。
pub fn crit_valid(c: usize) -> bool {
    c >= 1 && c <= CRIT_N
}

/// 判据 i 断言数冻结口径（奇 3 偶 2）。
pub fn assert_count(c: usize) -> u32 {
    if !crit_valid(c) {
        return 0;
    }
    ASSERT_BASE + (c % 2) as u32
}

/// 全批断言总数（冻结口径累计）。
pub fn assert_total_frozen() -> u32 {
    let mut s = 0u32;
    let mut i = 1;
    while i <= CRIT_N {
        s = s.saturating_add(assert_count(i));
        i += 1;
    }
    s
}

/// 互联规则（冻结）：对角恒联；链式 |i-j|=1；冻结跨接对。
pub fn link_expected(i: usize, j: usize) -> bool {
    if !crit_valid(i) || !crit_valid(j) {
        return false;
    }
    if i == j {
        return true;
    }
    let d = if i > j { i - j } else { j - i };
    if d == 1 {
        return true;
    }
    let mut k = 0;
    while k < EXTRA_PAIRS.len() {
        if let Some((a, b)) = EXTRA_PAIRS.get(k) {
            let (x, y) = (*a, *b);
            if (i == x && j == y) || (i == y && j == x) {
                return true;
            }
        }
        k += 1;
    }
    false
}

/// 判据 i 行和冻结口径。
pub fn row_sum_expected(i: usize) -> u32 {
    let mut s = 0u32;
    let mut j = 1;
    while j <= CRIT_N {
        if link_expected(i, j) {
            s = s.saturating_add(1);
        }
        j += 1;
    }
    s
}

// ── 互联矩阵（19×19 定容，0/1 格） ──────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconMatrix {
    /// 行主序 19×19 格。
    pub cells: Vec<u8>,
    pub n: usize,
}

impl ReconMatrix {
    /// 按冻结规则铺矩阵（对角 1 / 链式 / 跨接对）。
    pub fn build() -> ReconMatrix {
        let n = CRIT_N;
        let mut cells: Vec<u8> = Vec::new();
        let mut i = 1;
        while i <= n {
            let mut j = 1;
            while j <= n {
                cells.push(if link_expected(i, j) { 1 } else { 0 });
                j += 1;
            }
            i += 1;
        }
        ReconMatrix { cells, n }
    }

    /// 查格（坐标 1 起；越界显性拒）。
    pub fn query(&self, i: usize, j: usize) -> Result<u8, u32> {
        if !crit_valid(i) || !crit_valid(j) || self.n != CRIT_N {
            return Err(B35Code::CoordOver.val());
        }
        let idx = (i - 1) * self.n + (j - 1);
        match self.cells.get(idx) {
            Some(v) => Ok(*v),
            None => Err(B35Code::CoordOver.val()),
        }
    }

    /// 写格（坐标 1 起；越界显性拒；对角写 0 即时拒）。
    pub fn poke(&mut self, i: usize, j: usize, v: u8) -> Result<(), u32> {
        if !crit_valid(i) || !crit_valid(j) || self.n != CRIT_N {
            return Err(B35Code::CoordOver.val());
        }
        if i == j && v == 0 {
            return Err(B35Code::DiagBreak.val());
        }
        let idx = (i - 1) * self.n + (j - 1);
        match self.cells.get_mut(idx) {
            Some(slot) => {
                *slot = v;
                Ok(())
            }
            None => Err(B35Code::CoordOver.val()),
        }
    }

    /// 行和（坐标 1 起；越界回 0）。
    pub fn row_sum(&self, i: usize) -> u32 {
        if !crit_valid(i) {
            return 0;
        }
        let mut s = 0u32;
        let mut j = 1;
        while j <= self.n {
            if let Ok(v) = self.query(i, j) {
                s = s.saturating_add(v as u32);
            }
            j += 1;
        }
        s
    }
}

// ── 断言账（判据 → 键列） ───────────────────────────────────────
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssertLedger {
    /// 判据 c（1..=19）的断言键列（键 = KEY_SCALE + c×10 + 序号）。
    pub by_crit: Vec<Vec<u32>>,
    pub dup_rejects: u32,
}

impl AssertLedger {
    /// 按冻结口径铺账。
    pub fn build() -> AssertLedger {
        let mut led = AssertLedger::default();
        let mut c = 1;
        while c <= CRIT_N {
            let mut col: Vec<u32> = Vec::new();
            let mut no = 1;
            while no <= assert_count(c) {
                col.push(KEY_SCALE + c as u32 * CRIT_SCALE + no);
                no += 1;
            }
            led.by_crit.push(col);
            c += 1;
        }
        led
    }

    /// 虚增一枚（单面注入面：判据 c 追加一枚越口径键）。
    pub fn inflate(&mut self, c: usize) -> Result<(), u32> {
        if !crit_valid(c) {
            return Err(B35Code::CoordOver.val());
        }
        let idx = c - 1;
        let col = match self.by_crit.get_mut(idx) {
            Some(x) => x,
            None => return Err(B35Code::CoordOver.val()),
        };
        let next_no = col.len() as u32 + 1;
        col.push(KEY_SCALE + c as u32 * CRIT_SCALE + next_no);
        Ok(())
    }

    /// 断言总数。
    pub fn total(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0;
        while i < self.by_crit.len() {
            if let Some(col) = self.by_crit.get(i) {
                s = s.saturating_add(col.len() as u32);
            }
            i += 1;
        }
        s
    }
}

/// 断言账对账（逐判据键数对冻结口径 + 批内键唯一 + ≥40 大关）。
pub fn ledger_recon(led: &AssertLedger) -> Result<u32, u32> {
    if led.total() < ASSERT_GATE {
        return Err(B35Code::ThresholdUnmet.val());
    }
    let mut c = 1;
    while c <= CRIT_N {
        let idx = c - 1;
        let col = match led.by_crit.get(idx) {
            Some(x) => x,
            None => return Err(B35Code::CountMismatch.val()),
        };
        if col.len() as u32 != assert_count(c) {
            return Err(B35Code::CountMismatch.val());
        }
        // 键闭集闸：键必须落在判据 c 的键域。
        let mut k = 0;
        while k < col.len() {
            let key = match col.get(k) {
                Some(x) => *x,
                None => return Err(B35Code::CountMismatch.val()),
            };
            let crit_part = (key - KEY_SCALE) / CRIT_SCALE;
            let no_part = (key - KEY_SCALE) % CRIT_SCALE;
            if crit_part != c as u32 || no_part == 0 || no_part > assert_count(c) {
                return Err(B35Code::CountMismatch.val());
            }
            k += 1;
        }
        c += 1;
    }
    // 批内键唯一（两两查重）。
    let mut seen: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < led.by_crit.len() {
        let col = match led.by_crit.get(i) {
            Some(x) => x,
            None => break,
        };
        let mut k = 0;
        while k < col.len() {
            let key = match col.get(k) {
                Some(x) => *x,
                None => break,
            };
            let mut s = 0;
            while s < seen.len() {
                if let Some(x) = seen.get(s) {
                    if *x == key {
                        return Err(B35Code::LedgerDup.val());
                    }
                }
                s += 1;
            }
            seen.push(key);
            k += 1;
        }
        i += 1;
    }
    Ok(led.total())
}

// ── 批内对账器（状态机：草拟→对账→封账） ────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconPhase {
    /// 草拟（矩阵与断言账可写）
    Draft,
    /// 对账毕（全绿）
    Reconciled,
    /// 封账
    Sealed,
}

impl ReconPhase {
    pub fn rank(self) -> u32 {
        match self {
            ReconPhase::Draft => 0,
            ReconPhase::Reconciled => 1,
            ReconPhase::Sealed => 2,
        }
    }
    pub fn can_advance(self, to: ReconPhase) -> bool {
        to.rank() > self.rank()
    }
    pub const ALL: [ReconPhase; 3] = [
        ReconPhase::Draft,
        ReconPhase::Reconciled,
        ReconPhase::Sealed,
    ];
}

#[derive(Clone, Debug)]
pub struct B35Recon {
    pub matrix: Option<ReconMatrix>,
    pub ledger: Option<AssertLedger>,
    pub phase: ReconPhase,
    pub rejects: u32,
}

impl B35Recon {
    pub fn new() -> B35Recon {
        B35Recon {
            matrix: Some(ReconMatrix::build()),
            ledger: Some(AssertLedger::build()),
            phase: ReconPhase::Draft,
            rejects: 0,
        }
    }

    /// 对账（对角自核 + 对称互查 + 行和对拍 + 断言账对账；全绿进对账态）。
    pub fn reconcile(&mut self) -> Result<(u32, u32), u32> {
        if !self.phase.can_advance(ReconPhase::Reconciled) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(B35Code::StateRegress.val());
        }
        let m = match &self.matrix {
            Some(x) => x.clone(),
            None => return Err(B35Code::CoordOver.val()),
        };
        // 对角自核。
        let mut i = 1;
        while i <= CRIT_N {
            if m.query(i, i)? != 1 {
                return Err(B35Code::DiagBreak.val());
            }
            i += 1;
        }
        // 对称互查 + 行和对拍。
        let mut i = 1;
        while i <= CRIT_N {
            let mut j = i;
            while j <= CRIT_N {
                let a = m.query(i, j)?;
                let b = m.query(j, i)?;
                if a != b {
                    return Err(B35Code::SymBreak.val());
                }
                j += 1;
            }
            if m.row_sum(i) != row_sum_expected(i) {
                return Err(B35Code::RowSumBreak.val());
            }
            i += 1;
        }
        // 断言账对账（≥40 大关 + 口径逐判据）。
        let led = match &self.ledger {
            Some(x) => x.clone(),
            None => return Err(B35Code::CountMismatch.val()),
        };
        let total = ledger_recon(&led)?;
        self.phase = ReconPhase::Reconciled;
        Ok((total, CRIT_N as u32))
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<(), u32> {
        if !self.phase.can_advance(ReconPhase::Sealed) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(B35Code::StateRegress.val());
        }
        self.phase = ReconPhase::Sealed;
        Ok(())
    }
}

// ── 导出文本（19 行逐行账） ─────────────────────────────────────
/// 导出（行文：`B35_Jii 格×19 和=N`）。
pub fn export_text(m: &ReconMatrix) -> Result<String, u32> {
    if m.n != CRIT_N || m.cells.len() != CRIT_N * CRIT_N {
        return Err(B35Code::CoordOver.val());
    }
    let mut out = String::new();
    let mut i = 1;
    while i <= CRIT_N {
        out.push_str(&format!("B35_J{:02}", i));
        out.push_str(" |");
        let mut j = 1;
        while j <= CRIT_N {
            let v = m.query(i, j)?;
            out.push(' ');
            out.push(if v == 1 { '1' } else { '0' });
            j += 1;
        }
        out.push_str(&format!(" | 和={}", m.row_sum(i)));
        out.push('\n');
        i += 1;
    }
    Ok(out)
}

/// 导出对账：行数 19、行文与转置列文逐字对齐（导出即互查的文本面复算）。
pub fn export_recon(m: &ReconMatrix) -> Result<u32, u32> {
    let text = export_text(m)?;
    // 行数账。
    let mut lines: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let bytes = text.as_bytes();
    let mut p = 0usize;
    while p < bytes.len() {
        if let Some(b) = bytes.get(p) {
            if *b == b'\n' {
                lines.push(text.get(start..p).unwrap_or(""));
                start = p + 1;
            }
        }
        p += 1;
    }
    if start < bytes.len() {
        lines.push(text.get(start..bytes.len()).unwrap_or(""));
    }
    if lines.len() != CRIT_N {
        return Err(B35Code::ExportBreach.val());
    }
    // 逐行：行头 + 对角位 '1' + 和账。
    let mut i = 1;
    while i <= CRIT_N {
        let line = match lines.get(i - 1) {
            Some(x) => *x,
            None => return Err(B35Code::ExportBreach.val()),
        };
        let head = format!("B35_J{:02}", i);
        if !line.starts_with(&head) {
            return Err(B35Code::ExportBreach.val());
        }
        // 对角格文本面必须 '1'。
        let d = m.query(i, i)?;
        if d != 1 {
            return Err(B35Code::DiagBreak.val());
        }
        // 和账逐行对拍。
        let want_sum = row_sum_expected(i);
        let got_sum = m.row_sum(i);
        if want_sum != got_sum {
            return Err(B35Code::RowSumBreak.val());
        }
        // 行文含和尾账。
        let tail = format!("和={}", got_sum);
        if !line.ends_with(&tail) {
            return Err(B35Code::ExportBreach.val());
        }
        i += 1;
    }
    // 转置列文对拍：列 j 的格序列与行 j 的格序列逐位一致（对称文本面）。
    let mut j = 1;
    while j <= CRIT_N {
        let mut k = 1;
        while k <= CRIT_N {
            if m.query(j, k)? != m.query(k, j)? {
                return Err(B35Code::ExportBreach.val());
            }
            k += 1;
        }
        j += 1;
    }
    Ok(lines.len() as u32)
}

// ── LCG（种子写死 = SEG_BASE） ─────────────────────────────────
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

// ── 注入检出账（四路：对角破位/互查不对称/单面虚增/行和破账） ────
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
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：矩阵+账铺设→对账全绿→导出对拍→封账→断言大关 ───────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut r = B35Recon::new();
    let (total, crits) = r.reconcile()?;
    if total < ASSERT_GATE {
        return Err(B35Code::ThresholdUnmet.val());
    }
    let m = match &r.matrix {
        Some(x) => x.clone(),
        None => return Err(B35Code::CoordOver.val()),
    };
    let lines = export_recon(&m)?;
    if lines != CRIT_N as u32 {
        return Err(B35Code::ExportBreach.val());
    }
    r.seal()?;
    if r.phase != ReconPhase::Sealed {
        return Err(B35Code::StateRegress.val());
    }
    Ok((total, crits, lines))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（断言 48 / 判据 19 / 导出 19 行）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((48, 19, 19)) => Ok(()),
        _ => Err(B35Code::NotReproducible.val()),
    }
}

/// 序 B：越界查询显性拒（行 0 / 行 20 / 行 19 列 20）。
pub fn probe_seq_b() -> Result<(), u32> {
    let m = ReconMatrix::build();
    let r1 = m.query(0, 1);
    let r2 = m.query(20, 1);
    let r3 = m.query(19, 20);
    match (r1, r2, r3) {
        (Err(e1), Err(e2), Err(e3))
            if e1 == B35Code::CoordOver.val()
                && e2 == B35Code::CoordOver.val()
                && e3 == B35Code::CoordOver.val() =>
        {
            Ok(())
        }
        _ => Err(B35Code::NotReproducible.val()),
    }
}

/// 序 C：口径冻结（19 判据 / 大关 40 / 总数 48 / 跨接对 3）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        CRIT_N == 19,
        ASSERT_GATE == 40,
        assert_total_frozen() == 48,
        EXTRA_PAIRS.len() == 3,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(B35Code::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 0：对角破位（poke 对角 0 即时拒；绕开 poke 直接改账再对账必抓）。
        let mut m0 = ReconMatrix::build();
        let pick = 1 + rng.below(CRIT_N as u64) as usize;
        let idx = (pick - 1) * CRIT_N + (pick - 1);
        let direct_reject = match m0.poke(pick, pick, 0) {
            Err(e) => e == B35Code::DiagBreak.val(),
            Ok(()) => false,
        };
        // 绕闸注入（对账面兜底必抓）。
        let mut bypass = B35Recon::new();
        if let Some(mm) = bypass.matrix.as_mut() {
            if let Some(slot) = mm.cells.get_mut(idx) {
                *slot = 0;
            }
        }
        let bypass_caught = match bypass.reconcile() {
            Err(e) => e == B35Code::DiagBreak.val(),
            Ok(_) => false,
        };
        led.record(0, direct_reject && bypass_caught);
        // 路 1：互查不对称（单侧改格 → 对账必抓）。
        let mut r1 = B35Recon::new();
        if let Some(mm) = r1.matrix.as_mut() {
            if let Some(slot) = mm.cells.get_mut(0 * CRIT_N + 2) {
                *slot = 1;
            }
        }
        let hit1 = match r1.reconcile() {
            Err(e) => e == B35Code::SymBreak.val() || e == B35Code::RowSumBreak.val(),
            Ok(_) => false,
        };
        led.record(1, hit1);
        // 路 2：断言单面虚增（任一判据多一枚 → 对账必抓）。
        let pick2 = 1 + rng.below(CRIT_N as u64) as usize;
        let mut r2 = B35Recon::new();
        if let Some(led2) = r2.ledger.as_mut() {
            let _ = led2.inflate(pick2);
        }
        let hit2 = match r2.reconcile() {
            Err(e) => e == B35Code::CountMismatch.val(),
            Ok(_) => false,
        };
        led.record(2, hit2);
        // 路 3：断言不足大关（裁账至 <40 → 对账必抓）。
        let mut r3 = B35Recon::new();
        if let Some(led3) = r3.ledger.as_mut() {
            let mut c = 1;
            while c <= CRIT_N {
                if let Some(col) = led3.by_crit.get_mut(c - 1) {
                    col.clear();
                    col.push(KEY_SCALE + c as u32 * CRIT_SCALE + 1);
                }
                c += 1;
            }
        }
        let hit3 = match r3.reconcile() {
            Err(e) => e == B35Code::ThresholdUnmet.val() || e == B35Code::CountMismatch.val(),
            Ok(_) => false,
        };
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(B35Code::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < B35Code::ALL.len() {
        if let Some(c) = B35Code::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集对账 ─────────────────────────────────────────────
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < B35Code::ALL.len() {
        if let Some(c) = B35Code::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

/// LCG 确定性。
fn lcg_deterministic_ok() -> bool {
    let mut x = Lcg::new(SEG_BASE as u64);
    let mut y = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 16 {
        if x.next() != y.next() {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入账 0 号槽取值（.get 取，不裸下标）。
fn slot0(t: &[u32; 4]) -> u32 {
    match t.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 口径冻结。
    v.push((
        "J1-口径冻结",
        CRIT_N == 19 && ASSERT_GATE == 40 && assert_total_frozen() == 48,
        format!("{} 判据 断言口径总数{} ≥大关{}", CRIT_N, assert_total_frozen(), ASSERT_GATE),
    ));
    // 2. 断言大关。
    let led = AssertLedger::build();
    v.push((
        "J1-断言≥40大关",
        led.total() >= ASSERT_GATE && ledger_recon(&led).is_ok(),
        format!("断言账 {} 枚（口径 48）", led.total()),
    ));
    // 3. 矩阵对角自核。
    let m = ReconMatrix::build();
    let mut diag_ok = true;
    let mut i = 1;
    while i <= CRIT_N {
        if m.query(i, i) != Ok(1) {
            diag_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-对角自核",
        diag_ok,
        format!("{}×{} 全对角 1", CRIT_N, CRIT_N),
    ));
    // 4. 互查对称。
    let mut sym_ok = true;
    let mut i = 1;
    while i <= CRIT_N && sym_ok {
        let mut j = i + 1;
        while j <= CRIT_N {
            if m.query(i, j) != m.query(j, i) {
                sym_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-互查对称",
        sym_ok,
        String::from("M[i][j]=M[j][i] 全对称"),
    ));
    // 5. 行和对拍（冻结口径逐行）。
    let mut rowsum_ok = true;
    let mut first_bad = 0usize;
    let mut i = 1;
    while i <= CRIT_N {
        if m.row_sum(i) != row_sum_expected(i) {
            rowsum_ok = false;
            if first_bad == 0 {
                first_bad = i;
            }
        }
        i += 1;
    }
    v.push((
        "J1-行和对拍",
        rowsum_ok,
        format!("19 行逐行对拍 失配行={}", first_bad),
    ));
    // 6. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((t, c, l)) => *t == 48 && *c == 19 && *l == 19,
            Err(_) => false,
        },
        match &g {
            Ok((t, c, l)) => format!("断言{} 判据{} 导出{}行", t, c, l),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 7. 矩阵可查（越界显性拒）。
    v.push((
        "J1-矩阵可查越界拒",
        probe_seq_b().is_ok(),
        String::from("行 0/列 0/行 20/列 20 全显性拒"),
    ));
    // 8. 矩阵可导（导出对拍 19 行）。
    let exp = export_recon(&ReconMatrix::build());
    v.push((
        "J1-矩阵可导",
        match &exp {
            Ok(n) => *n == CRIT_N as u32,
            Err(_) => false,
        },
        match &exp {
            Ok(n) => format!("导出{}行 行文=转置列文", n),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 9. 导出文本逐行账抽核（J01 行按冻结规则重算逐字对拍）。
    let txt = export_text(&ReconMatrix::build());
    let mut want_j01 = String::from("B35_J01 |");
    let mut j = 1;
    while j <= CRIT_N {
        want_j01.push(' ');
        want_j01.push(if link_expected(1, j) { '1' } else { '0' });
        j += 1;
    }
    let j01_ok = match &txt {
        Ok(t) => t.contains(&want_j01),
        Err(_) => false,
    };
    v.push((
        "J1-导出逐行抽核",
        j01_ok,
        match &txt {
            Ok(t) => {
                let first = match t.lines().next() {
                    Some(x) => x,
                    None => "",
                };
                format!("J01 行={}", first)
            }
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 10. 探针三序。
    v.push((
        "J1-探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a全链 b越界拒 c口径"),
    ));
    // 11. 注入四路 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught() && slot0(&l.tries) == JUDGE_ROUNDS,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 12. 状态机单调（对账后封账；封账后重复对账显性拒）。
    let mono_ok = {
        let mut r = B35Recon::new();
        r.reconcile().ok();
        let sealed = r.seal().is_ok();
        let after = match r.reconcile() {
            Err(e) => {
                e == B35Code::StateRegress.val() && r.rejects >= 1
            }
            Ok(_) => false,
        };
        sealed && after && ReconPhase::Draft.rank() < ReconPhase::Reconciled.rank()
    };
    v.push((
        "J1-状态机单调",
        mono_ok,
        String::from("草拟→对账→封账 只许前进 重复对账拒"),
    ));
    // 13. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 14. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 15. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", B35Code::ALL.len()),
    ));
    // 16. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x76180 窗口 0x40 对齐区间三闸"),
    ));
    v
}

pub fn run_unx1499_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1499 B35 批内对账（断言资产互联对账总成）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_green() {
        match golden_run() {
            Ok((t, c, l)) => {
                assert_eq!(t, 48);
                assert_eq!(c, 19);
                assert_eq!(l, 19);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_consts() {
        assert_eq!(CRIT_N, 19);
        assert_eq!(ASSERT_GATE, 40);
        // 断言口径：奇 3 偶 2 → 10×3 + 9×2 = 48。
        assert_eq!(assert_count(1), 3);
        assert_eq!(assert_count(2), 2);
        assert_eq!(assert_count(19), 3);
        assert_eq!(assert_total_frozen(), 48);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn matrix_rules() {
        let m = ReconMatrix::build();
        // 对角全 1。
        let mut i = 1;
        while i <= CRIT_N {
            assert_eq!(m.query(i, i), Ok(1), "对角 J{:02}", i);
            i += 1;
        }
        // 链式邻接。
        assert_eq!(m.query(1, 2), Ok(1));
        assert_eq!(m.query(2, 1), Ok(1));
        assert_eq!(m.query(18, 19), Ok(1));
        // 非邻接非跨接为 0。
        assert_eq!(m.query(3, 15), Ok(0));
        // 跨接对（对称双向）。
        assert_eq!(m.query(1, 19), Ok(1));
        assert_eq!(m.query(19, 1), Ok(1));
        assert_eq!(m.query(5, 12), Ok(1));
        assert_eq!(m.query(12, 5), Ok(1));
        assert_eq!(m.query(9, 17), Ok(1));
        assert_eq!(m.query(17, 9), Ok(1));
        // 越界拒。
        assert!(m.query(0, 1).is_err());
        assert!(m.query(1, 20).is_err());
        // 对角写 0 即时拒。
        let mut m2 = m.clone();
        match m2.poke(7, 7, 0) {
            Err(e) => assert_eq!(e, B35Code::DiagBreak.val()),
            Ok(()) => assert!(false, "对角写 0 未拒"),
        }
    }

    #[test]
    fn row_sums_match_frozen() {
        let m = ReconMatrix::build();
        let mut i = 1;
        while i <= CRIT_N {
            assert_eq!(m.row_sum(i), row_sum_expected(i), "行 J{:02}", i);
            i += 1;
        }
        // J01 行和：自核 + J02 + J19 = 3。
        assert_eq!(row_sum_expected(1), 3);
        // J02 行和：自核 + J01 + J03 = 3。
        assert_eq!(row_sum_expected(2), 3);
    }

    #[test]
    fn ledger_counts_and_uniqueness() {
        let led = AssertLedger::build();
        assert_eq!(led.total(), 48);
        assert!(ledger_recon(&led).is_ok());
        // 单面虚增一枚必破。
        let mut led2 = led.clone();
        led2.inflate(7).ok();
        assert_eq!(led2.total(), 49);
        match ledger_recon(&led2) {
            Err(e) => assert_eq!(e, B35Code::CountMismatch.val()),
            Ok(_) => assert!(false, "单面虚增未破"),
        }
        // 不足大关必破。
        let mut led3 = AssertLedger::build();
        let mut c = 1;
        while c <= CRIT_N {
            if let Some(col) = led3.by_crit.get_mut(c - 1) {
                col.clear();
                col.push(KEY_SCALE + c as u32 * CRIT_SCALE + 1);
            }
            c += 1;
        }
        match ledger_recon(&led3) {
            // 裁账后总数 19 < 大关 40：大关闸先于口径闸回因。
            Err(e) => assert_eq!(e, B35Code::ThresholdUnmet.val()),
            Ok(_) => assert!(false, "裁账未破"),
        }
    }

    #[test]
    fn inject_rounds_tally() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(e) => {
                assert!(false, "inject err=0x{:x}", e);
                return;
            }
        };
        assert!(led.all_caught(), "四路注入 10/10 全抓");
        assert_eq!(slot0(&led.tries), JUDGE_ROUNDS);
    }

    #[test]
    fn export_lines_and_diagonal() {
        let m = ReconMatrix::build();
        match export_recon(&m) {
            Ok(n) => assert_eq!(n, 19),
            Err(e) => assert!(false, "export err=0x{:x}", e),
        }
        let txt = match export_text(&m) {
            Ok(t) => t,
            Err(e) => {
                assert!(false, "text err=0x{:x}", e);
                return;
            }
        };
        // 19 行、每行 19 格。
        let mut line_n = 0;
        let mut it = txt.lines();
        while let Some(l) = it.next() {
            line_n += 1;
            // 只数格段（首个 | 与末个 | 之间），行头 J01 与和尾数字不入格账。
            let cells_seg = l.split('|').nth(1).unwrap_or("");
            let cells = cells_seg.matches('1').count() + cells_seg.matches('0').count();
            assert_eq!(cells, 19, "行{} 格数", line_n);
            assert!(l.starts_with("B35_J"), "行{} 行头", line_n);
        }
        assert_eq!(line_n, 19);
        // J01 行按冻结规则重算逐字对拍（对角 1 + J02 链接 + J19 跨接）。
        let mut want_j01 = String::from("B35_J01 |");
        let mut j = 1;
        while j <= CRIT_N {
            want_j01.push(' ');
            want_j01.push(if link_expected(1, j) { '1' } else { '0' });
            j += 1;
        }
        assert!(txt.contains(&want_j01), "J01 行文对拍");
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert!(lcg_deterministic_ok());
        assert_eq!(detail(B35Code::SymBreak.val()), B35Code::SymBreak.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }

    #[test]
    fn checks_face_full_green() {
        let s = run_unx1499_checks();
        assert!(s.len() >= 12, "检查条数须 ≥12");
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
    }
}
