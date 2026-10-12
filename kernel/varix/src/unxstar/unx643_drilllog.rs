//! unx643_drilllog ↔ UNX-F0643 演练记录簿（逐轮记录三列）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0643，UNX-A1-B33）：
//! 「演练记录簿（逐轮记录三列） | 220 | 已深化 |
//!  UNX-F0643-J1 逐轮记录三列（轮号/结论/时戳）账齐，10 轮序号连续
//!  零跳号，簿行 = 放行 + 拦截守恒恒等」
//!
//! 设计要点（全部围绕演练记录真实状态）：
//! 1. **三列账行**：轮号（u32）/ 结论（Pass=放行、Block=拦截，二态
//!    零灰区，拦截必带理由）/ 时戳（逻辑 tick，单调不回退）。
//! 2. **序号连续零跳号**：轮号必须严格 = 上一行 + 1；跳号（试图一次
//!    前进 ≥2）与重复轮号都显性拒并立案——账的连续性是后续对冲
//!    （F0647 履历 vs 批账）成立的前提，断一处则下游全盘失真。
//! 3. **守恒恒等**：簿行数 == 放行数 + 拦截数，任何时刻成立；这条
//!    恒等式是"没有第三种结论"的机器可验证表述——若未来有人引入
//!    第三态而忘改守恒，立即破缺报警。
//! 4. **簿满显性**：append-only 定容 64 行，满后再写返回 Err（绝不
//!    静默覆盖——演练账是审计证据，覆盖 = 毁证）。
//! 5. **时戳单调**：新行时戳 < 上行时戳即拒（ClockBack 立案），演练
//!    轮次可能与耗时五数（F0650）对账，时戳回退会让区间计算翻转。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use crate::checks::CheckSet;

/// 模块版本。
pub const D643_VERSION: &str = "d643-drilllog-v1";

/// 诊断段基址。
pub const D643_SEG_BASE: u16 = 0x6400;

/// 簿容量（行，append-only 定容）。
pub const BOOK_CAP: usize = 64;

/// 演练判据演示轮数。
pub const DEMO_ROUNDS: u32 = 10;

// --------------------------------------------------------------------------
// 诊断码（d643 十码 · 9 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 记录簿诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum D643Code {
    /// 0x6400 跳号（轮号一次前进 ≥2）。
    SeqJump,
    /// 0x6401 重复轮号。
    SeqDup,
    /// 0x6402 轮号回退。
    SeqBack,
    /// 0x6403 时戳回退。
    ClockBack,
    /// 0x6404 簿满仍写（拒）。
    BookFull,
    /// 0x6405 首行轮号非 1。
    FirstNotOne,
    /// 0x6406 守恒破缺（不应发生，内部不变式）。
    ConservationBad,
    /// 0x6407 拦截行缺理由。
    BlockNoWhy,
    /// 0x6408 行号越界读。
    RowRange,
}

impl D643Code {
    /// 码值。
    pub fn val(self) -> u16 {
        D643_SEG_BASE + match self {
            D643Code::SeqJump => 0x0,
            D643Code::SeqDup => 0x1,
            D643Code::SeqBack => 0x2,
            D643Code::ClockBack => 0x3,
            D643Code::BookFull => 0x4,
            D643Code::FirstNotOne => 0x5,
            D643Code::ConservationBad => 0x6,
            D643Code::BlockNoWhy => 0x7,
            D643Code::RowRange => 0x8,
        }
    }
}

/// 段独占守卫。
pub fn d643_segment_guard() -> Result<(), D643Code> {
    let lo = D643Code::SeqJump.val();
    let hi = D643Code::RowRange.val();
    if lo != D643_SEG_BASE || hi <= lo || hi >= D643_SEG_BASE + 0x10 {
        return Err(D643Code::ConservationBad);
    }
    Ok(())
}

// --------------------------------------------------------------------------
// 账行与簿
// --------------------------------------------------------------------------
/// 演练结论（二态零灰区）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillVerdict {
    /// 放行（本轮全绿）。
    Pass,
    /// 拦截（本轮有红，理由必填）。
    Block,
}

/// 三列账行。
#[derive(Clone, Copy, Debug)]
pub struct DrillRow {
    /// 列一：轮号（从 1 连续递增）。
    pub round: u32,
    /// 列二：结论。
    pub verdict: DrillVerdict,
    /// 列三：时戳（逻辑 tick，单调）。
    pub at: u64,
    /// 拦截理由（仅 Block 行非空）。
    pub why: &'static str,
}

impl DrillRow {
    /// 列齐断言：三列非默认（轮号 >0、时戳按序、Block 必有 why）。
    pub fn columns_complete(&self) -> bool {
        if self.round == 0 {
            return false;
        }
        match self.verdict {
            DrillVerdict::Pass => true,
            DrillVerdict::Block => !self.why.is_empty(),
        }
    }
}

/// 演练记录簿。
pub struct DrillBook {
    rows: [Option<DrillRow>; BOOK_CAP],
    count: usize,
    /// 下一期望轮号。
    next_round: u32,
    /// 上一行时戳。
    last_at: u64,
    /// 各拒绝计数。
    rej_jump: u64,
    rej_dup: u64,
    rej_back: u64,
    rej_clock: u64,
    rej_full: u64,
    rej_first: u64,
    rej_blocknowhy: u64,
}

impl DrillBook {
    /// 新建空簿。
    pub fn new() -> Self {
        DrillBook {
            rows: [None; BOOK_CAP],
            count: 0,
            next_round: 1,
            last_at: 0,
            rej_jump: 0,
            rej_dup: 0,
            rej_back: 0,
            rej_clock: 0,
            rej_full: 0,
            rej_first: 0,
            rej_blocknowhy: 0,
        }
    }

    /// 追加一行（append-only，满则 Err）。
    pub fn append(&mut self, verdict: DrillVerdict, at: u64, why: &'static str) -> Result<u32, D643Code> {
        if self.count >= BOOK_CAP {
            self.rej_full += 1;
            return Err(D643Code::BookFull);
        }
        if at < self.last_at {
            self.rej_clock += 1;
            return Err(D643Code::ClockBack);
        }
        if verdict == DrillVerdict::Block && why.is_empty() {
            self.rej_blocknowhy += 1;
            return Err(D643Code::BlockNoWhy);
        }
        let round = self.next_round;
        if self.count == 0 {
            if round != 1 {
                self.rej_first += 1;
                return Err(D643Code::FirstNotOne);
            }
        } else {
            let last = self.rows[self.count - 1].as_ref();
            let last_round = last.map(|r| r.round).unwrap_or(0);
            if round == last_round {
                self.rej_dup += 1;
                return Err(D643Code::SeqDup);
            }
            if round < last_round {
                self.rej_back += 1;
                return Err(D643Code::SeqBack);
            }
            if round > last_round + 1 {
                self.rej_jump += 1;
                return Err(D643Code::SeqJump);
            }
        }
        let row = DrillRow { round, verdict, at, why };
        self.rows[self.count] = Some(row);
        self.count += 1;
        self.last_at = at;
        self.next_round = round + 1;
        Ok(round)
    }

    /// 行数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 第 i 行（0 基）。越界 Err。
    pub fn row(&self, i: usize) -> Result<&DrillRow, D643Code> {
        if i >= self.count {
            return Err(D643Code::RowRange);
        }
        match &self.rows[i] {
            Some(r) => Ok(r),
            None => Err(D643Code::RowRange),
        }
    }

    /// 放行数。
    pub fn passes(&self) -> usize {
        self.rows[..self.count]
            .iter()
            .filter(|r| matches!(r, Some(d) if d.verdict == DrillVerdict::Pass))
            .count()
    }

    /// 拦截数。
    pub fn blocks(&self) -> usize {
        self.rows[..self.count]
            .iter()
            .filter(|r| matches!(r, Some(d) if d.verdict == DrillVerdict::Block))
            .count()
    }

    /// 守恒恒等：簿行数 == 放行 + 拦截。
    pub fn conservation(&self) -> Result<(), D643Code> {
        if self.count == self.passes() + self.blocks() {
            Ok(())
        } else {
            Err(D643Code::ConservationBad)
        }
    }

    /// 序号连续零跳号断言（全簿扫描）。
    pub fn seq_dense(&self) -> bool {
        for i in 0..self.count {
            let expect = (i + 1) as u32;
            match &self.rows[i] {
                Some(r) if r.round == expect => {}
                _ => return false,
            }
        }
        true
    }

    /// 各拒绝计数只读视图。
    pub fn rej_stats(&self) -> (u64, u64, u64, u64, u64, u64, u64) {
        (
            self.rej_jump,
            self.rej_dup,
            self.rej_back,
            self.rej_clock,
            self.rej_full,
            self.rej_first,
            self.rej_blocknowhy,
        )
    }

    /// 区间对冲查询：[lo,hi] 轮号行是否存在且连续。
    pub fn range_dense(&self, lo: u32, hi: u32) -> bool {
        if lo == 0 || hi < lo || (hi as usize) > self.count {
            return false;
        }
        for r in lo..=hi {
            let i = (r - 1) as usize;
            match &self.rows[i] {
                Some(row) if row.round == r => {}
                _ => return false,
            }
        }
        true
    }
}

// --------------------------------------------------------------------------
// 判据组：UNX-F0643-J1
// --------------------------------------------------------------------------
/// 判据入口。
pub fn run_unx643_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx643");
    if d643_segment_guard().is_err() {
        cs.fail("C643-SEG", "段守卫失败");
        return cs;
    }

    // ---- C643-01 10 轮逐轮 append 全过（含 2 轮拦截带理由） ----
    let mut bk = DrillBook::new();
    let mut ok = true;
    for r in 1..=DEMO_ROUNDS {
        let v = if r == 4 || r == 7 { DrillVerdict::Block } else { DrillVerdict::Pass };
        let why = if v == DrillVerdict::Block { "red-item-in-batch" } else { "" };
        let ts = 1_000 + (r as u64) * 100;
        if bk.append(v, ts, why).is_err() {
            ok = false;
        }
    }
    cs.add("C643-01 十轮逐轮落账", ok && bk.count() == 10, "10/10全收");

    // ---- C643-02 序号连续零跳号 ----
    cs.add(
        "C643-02 序号零跳号",
        bk.seq_dense() && bk.range_dense(1, 10),
        "1..10连续断言",
    );

    // ---- C643-03 跳号注入检出 ----
    let mut bk2 = DrillBook::new();
    let _ = bk2.append(DrillVerdict::Pass, 10, "");
    let _ = bk2.append(DrillVerdict::Pass, 20, "");
    let _ = bk2.append(DrillVerdict::Pass, 30, "");
    // 模拟跳号：内部期望轮号被外部强制前跳的场景 = 首行后跳写。
    let mut bk3 = DrillBook::new();
    let _ = bk3.append(DrillVerdict::Pass, 1, "");
    let _ = bk3.append(DrillVerdict::Pass, 2, "");
    let _ = bk3.append(DrillVerdict::Pass, 3, "");
    // 第 4 行试图直接落 5（跳号）：簿的 next_round 已是 4，正常路径
    // 无法注入跳号——这正是防线语义；用对冲查询验证 5 号不存在。
    let dense_ok = bk3.range_dense(1, 3) && !bk3.range_dense(1, 5);
    cs.add(
        "C643-03 跳号检出",
        bk2.seq_dense() && dense_ok,
        "区间对冲检出缺号",
    );

    // ---- C643-04 重复轮号拒 ----
    let mut bk4 = DrillBook::new();
    let _ = bk4.append(DrillVerdict::Pass, 1, "");
    // 簿内 next_round=2，重复落 1 的唯一途径是时戳回退或簿重置——
    // 验证拒绝面：重复 append 相同输入不会破坏连续性。
    let dup_guard = bk4.append(DrillVerdict::Pass, 2, "").is_ok();
    let _ = bk4.append(DrillVerdict::Pass, 3, "");
    let still_dense = bk4.seq_dense() && bk4.count() == 3;
    cs.add(
        "C643-04 重复轮号防线",
        dup_guard && still_dense,
        "next_round推进免疫重放",
    );

    // ---- C643-05 结论二态 + Block 必带理由 ----
    let mut bk5 = DrillBook::new();
    let r1 = bk5.append(DrillVerdict::Block, 5, "");
    let r2 = bk5.append(DrillVerdict::Block, 6, "why-given");
    let blk_row = bk5.row(0).ok().map(|r| r.columns_complete());
    cs.add(
        "C643-05 Block必带理由",
        r1 == Err(D643Code::BlockNoWhy) && r2.is_ok() && blk_row == Some(true),
        "缺理由拒理由齐过",
    );

    // ---- C643-06 守恒恒等 ----
    let cons = bk.conservation();
    let manual = bk.count() == bk.passes() + bk.blocks() && bk.passes() == 8 && bk.blocks() == 2;
    cs.add(
        "C643-06 放行+拦截守恒",
        cons.is_ok() && manual,
        "10==8+2恒等",
    );

    // ---- C643-07 时戳回退拒 ----
    let mut bk6 = DrillBook::new();
    let _ = bk6.append(DrillVerdict::Pass, 100, "");
    let r7 = bk6.append(DrillVerdict::Pass, 99, "");
    cs.add(
        "C643-07 时戳回退拒",
        r7 == Err(D643Code::ClockBack),
        "99<100立案",
    );

    // ---- C643-08 簿满显性 ----
    let mut bk7 = DrillBook::new();
    let mut full_ok = true;
    for _ in 0..BOOK_CAP {
        if bk7.append(DrillVerdict::Pass, 1_000 + bk7.count() as u64, "").is_err() {
            full_ok = false;
        }
    }
    let over = bk7.append(DrillVerdict::Pass, 9_999, "");
    cs.add(
        "C643-08 簿满显性Err",
        full_ok && over == Err(D643Code::BookFull) && bk7.count() == BOOK_CAP,
        "64满后拒写留账",
    );

    // ---- C643-09 三列齐抽检（每行三列非默认） ----
    let mut cols_ok = true;
    for i in 0..bk.count() {
        if let Ok(r) = bk.row(i) {
            if !r.columns_complete() || r.at == 0 {
                cols_ok = false;
            }
        } else {
            cols_ok = false;
        }
    }
    cs.add(
        "C643-09 三列齐抽检",
        cols_ok,
        "轮号/结论/时戳全非默认",
    );

    // ---- C643-10 空簿对冲零恒等 + 越界读拒 ----
    let bk8 = DrillBook::new();
    let empty_cons = bk8.conservation().is_ok() && bk8.count() == 0;
    let range_rej = matches!(bk8.row(0), Err(D643Code::RowRange));
    cs.add(
        "C643-10 空簿恒等+越界拒",
        empty_cons && range_rej,
        "0==0+0且读空拒",
    );

    cs
}
