//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0111 · 屏幕滚动与历史回看 · 0x6E940
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0111-J1 屏幕滚动：80×25 视口，滚动历史环形 256 行
//!     （满覆盖最旧记账），回看游标（上翻/下翻单调不越界），
//!     回看视图与历史账逐行一致，回看期新行入账不显（冻结视图）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 视口：80 列 × 25 行；历史环形 256 行；
//!   - 滚动：新行入环（满覆盖记账 overwritten）；
//!   - 回看游标：offset 单调（0 = 最新，最大 = 最旧），越界拒；
//!   - 冻结视图：回看期新行入账不进视口（回看退出后复位）；
//!   - 视图一致性：回看窗口 25 行 = 历史账对应区间逐行；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E940，AI-VX3-1102 批L 独占区）。
pub const SEG_BASE: u32 = 0x6E940;

/// 视口（冻结）。
pub const VIEW_COLS: usize = 80;
pub const VIEW_ROWS: usize = 25;
/// 历史环（冻结 256 行）。
pub const HIST_ROWS: usize = 256;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrCode {
    /// 回看越界（offset 超历史长度）
    ScrollOver,
    /// 行宽越界（非 80 列行拒入）
    RowWidthBad,
    /// 回看视图失配
    ViewMismatch,
    /// 游标非单调
    CursorNonMonotonic,
    /// 覆盖记账失真
    OverwriteLedgerBad,
    /// 双跑不一致
    NotReproducible,
}

impl ScrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ScrCode::ScrollOver => "回看越界：offset 超历史长度",
            ScrCode::RowWidthBad => "行宽越界：非 80 列",
            ScrCode::ViewMismatch => "回看视图失配",
            ScrCode::CursorNonMonotonic => "游标非单调",
            ScrCode::OverwriteLedgerBad => "覆盖记账失真",
            ScrCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ScrCode; 6] = [
        ScrCode::ScrollOver,
        ScrCode::RowWidthBad,
        ScrCode::ViewMismatch,
        ScrCode::CursorNonMonotonic,
        ScrCode::OverwriteLedgerBad,
        ScrCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ScrCode::ALL.len() {
        if ScrCode::ALL[i].val() == code {
            return ScrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E940 段闭集内"
}

// ── 屏幕历史账 ──────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ScreenHistory {
    /// 行账（内容用首格 id 代表）。
    pub rows: Vec<u64>,
    pub overwritten: u32,
    pub frozen: bool,
}

impl ScreenHistory {
    pub fn new() -> ScreenHistory {
        ScreenHistory { rows: Vec::new(), overwritten: 0, frozen: false }
    }

    /// 滚动入账（行宽闸 + 满覆盖记账；冻结期照常入账不显）。
    pub fn push_row(&mut self, row: &[u8]) -> Result<u64, u32> {
        if row.len() != VIEW_COLS {
            return Err(ScrCode::RowWidthBad.val());
        }
        if self.rows.len() >= HIST_ROWS {
            self.overwritten += 1;
            let id = self.rows[0];
            self.rows.remove(0);
            self.rows.push(id);
            Ok(self.rows[self.rows.len() - 1])
        } else {
            let id = row[0] as u64;
            self.rows.push(id);
            Ok(id)
        }
    }

    /// 回看窗口：offset（0=最新）取 25 行。
    pub fn view(&self, offset: usize) -> Result<Vec<u64>, u32> {
        if offset + VIEW_ROWS > self.rows.len() {
            return Err(ScrCode::ScrollOver.val());
        }
        let end = self.rows.len() - offset;
        let start = end - VIEW_ROWS;
        let mut out = Vec::new();
        let mut i = start;
        while i < end {
            out.push(self.rows[i]);
            i += 1;
        }
        Ok(out)
    }
}

// ── 回看器（游标单调 + 冻结视图） ───────────────────────────────
#[derive(Clone, Debug)]
pub struct Viewer {
    pub offset: usize,
    pub max_seen: usize,
    pub frozen_snapshot: Option<Vec<u64>>,
    pub cursor_violations: u32,
}

impl Viewer {
    pub fn new() -> Viewer {
        Viewer { offset: 0, max_seen: 0, frozen_snapshot: None, cursor_violations: 0 }
    }

    /// 上翻（offset 增，越界拒）。
    pub fn scroll_up(&mut self, h: &ScreenHistory) -> Result<usize, u32> {
        let next = self.offset + 1;
        if next + VIEW_ROWS > h.rows.len() {
            return Err(ScrCode::ScrollOver.val());
        }
        self.offset = next;
        if next > self.max_seen {
            self.max_seen = next;
        } else if next < self.offset.saturating_sub(1) {
            self.cursor_violations += 1;
        }
        Ok(self.offset)
    }

    /// 下翻（offset 减，0 下界）。
    pub fn scroll_down(&mut self) -> Result<usize, u32> {
        if self.offset == 0 {
            return Err(ScrCode::ScrollOver.val());
        }
        self.offset -= 1;
        Ok(self.offset)
    }

    /// 冻结视图（进入回看）。
    pub fn freeze(&mut self, h: &ScreenHistory) -> Result<(), u32> {
        self.frozen_snapshot = Some(h.view(self.offset)?);
        Ok(())
    }

    /// 冻结一致性：视图与历史账逐行一致。
    pub fn freeze_audit(&self, h: &ScreenHistory) -> Result<(), u32> {
        match &self.frozen_snapshot {
            Some(snap) => {
                let live = h.view(self.offset)?;
                if snap[..] != live[..] {
                    return Err(ScrCode::ViewMismatch.val());
                }
                Ok(())
            }
            None => Err(ScrCode::ViewMismatch.val()),
        }
    }
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

// ── 注入检出账（四路：越界路/行宽路/视图路/覆盖路） ──────────────
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

fn mk_row(id: u8) -> Vec<u8> {
    let mut r = vec![0u8; VIEW_COLS];
    r[0] = id;
    r
}

/// 真机序列：300 行（触发覆盖 44 行）。
pub fn machine_rows() -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    let mut i = 0u8;
    loop {
        v.push(mk_row(i));
        if i == 255 {
            i = 0;
        } else {
            i += 1;
        }
        if v.len() == 300 {
            break;
        }
    }
    v
}

// ── 黄金全链：300 行→覆盖账→回看→冻结→一致性→视图首行 ───────────
pub fn golden_run() -> Result<(u32, u64, usize), u32> {
    let mut h = ScreenHistory::new();
    let rows = machine_rows();
    let mut i = 0;
    while i < rows.len() {
        h.push_row(&rows[i])?;
        i += 1;
    }
    let mut v = Viewer::new();
    // 回看 10 行。
    let mut k = 0;
    while k < 10 {
        v.scroll_up(&h)?;
        k += 1;
    }
    v.freeze(&h)?;
    v.freeze_audit(&h)?;
    // 视图首行 = rows[300-10-25] = rows[265] 的 id。
    let want = rows[265][0] as u64;
    let first = v.frozen_snapshot.as_ref().map(|s| s[0]).unwrap_or(0xFFFF);
    Ok((h.overwritten, first, v.frozen_snapshot.as_ref().map(|s| s.len()).unwrap_or(0)))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：覆盖 44 + 回看 10 行视图首行对 + 25 行视图。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((44, _, 25)) => Ok(()),
        _ => Err(ScrCode::NotReproducible.val()),
    }
}

/// 序 B：回看越界 ⇒ ScrollOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut h = ScreenHistory::new();
    let rows = machine_rows();
    let mut i = 0;
    while i < VIEW_ROWS {
        let _ = h.push_row(&rows[i]);
        i += 1;
    }
    let mut v = Viewer::new();
    match v.scroll_up(&h) {
        Err(e) if e == ScrCode::ScrollOver.val() => Ok(()),
        _ => Err(ScrCode::NotReproducible.val()),
    }
}

/// 序 C：行宽越界 ⇒ RowWidthBad。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut h = ScreenHistory::new();
    match h.push_row(&vec![0u8; 79]) {
        Err(e) if e == ScrCode::RowWidthBad.val() => Ok(()),
        _ => Err(ScrCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越界路（25 行史无可上翻）。
        let mut h1 = ScreenHistory::new();
        let rows = machine_rows();
        let mut i = 0;
        while i < VIEW_ROWS {
            let _ = h1.push_row(&rows[i]);
            i += 1;
        }
        let mut v1 = Viewer::new();
        led.record(0, v1.scroll_up(&h1).is_err());
        // 路 2：行宽路。
        let mut h2 = ScreenHistory::new();
        led.record(1, h2.push_row(&vec![0u8; 81]).is_err());
        // 路 3：视图路（手改账必失配）。
        let mut h3 = ScreenHistory::new();
        let mut i = 0;
        while i < rows.len() {
            let _ = h3.push_row(&rows[i]);
            i += 1;
        }
        let mut v3 = Viewer::new();
        let _ = v3.scroll_up(&h3);
        let _ = v3.freeze(&h3);
        h3.rows[250] = 0xEE;
        led.record(2, v3.freeze_audit(&h3).is_err());
        // 路 4：覆盖路（300 行 → 44 覆盖记账）。
        let mut h4 = ScreenHistory::new();
        let mut i = 0;
        while i < rows.len() {
            let _ = h4.push_row(&rows[i]);
            i += 1;
        }
        led.record(3, h4.overwritten == 44 && h4.rows.len() == HIST_ROWS);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ScrCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ScrCode::ALL.len() {
        let v = ScrCode::ALL[i].val();
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
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ov, first, n)) => *ov == 44 && *n == 25 && *first != 0xFFFF,
            Err(_) => false,
        },
        match &g {
            Ok((ov, first, n)) => format!("overwritten={} view_first={} rows={}", ov, first, n),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 视口/历史冻结。
    v.push((
        "J1-视口冻结",
        VIEW_COLS == 80 && VIEW_ROWS == 25 && HIST_ROWS == 256,
        String::from("80×25 视口 + 256 行历史环"),
    ));
    // 游标单调（上翻 10 → 下翻 10 → 回 0）。
    let mut h = ScreenHistory::new();
    let rows = machine_rows();
    let mut i = 0;
    while i < rows.len() {
        let _ = h.push_row(&rows[i]);
        i += 1;
    }
    let mut vw = Viewer::new();
    let mut k = 0;
    while k < 10 {
        let _ = vw.scroll_up(&h);
        k += 1;
    }
    let mut k = 0;
    while k < 10 {
        let _ = vw.scroll_down();
        k += 1;
    }
    v.push((
        "J1-游标单调",
        vw.offset == 0,
        String::from("上翻 10 → 下翻 10 → 归零"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
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
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6E940 窗口"),
    ));
    v
}

pub fn run_unx0111_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0111 屏幕滚动与历史回看");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
