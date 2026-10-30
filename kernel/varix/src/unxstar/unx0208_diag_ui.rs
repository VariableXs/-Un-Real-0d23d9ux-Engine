//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0208 · 报告展示接口（诊断模式 UI） · 0x6C600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0208-J1 诊断模式分页浏览报告，键位（上下翻/跳转/退
//!     出）全可用（体验 14 章口径）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 分页浏览：报告行账 → 每页 PAGE_ROWS 行（冻结 20），页码闭
//!      集（0..=last），越页翻页拒（显性）；
//!    - 键位闭集：上翻/下翻/跳转/退出四键（体验 14 章口径冻结），
//!      每键语义账：上翻=页-1（0 页拒）、下翻=页+1（末页拒）、
//!      跳转=页号直达（越页拒）、退出=置退出态；
//!    - 页窗口渲染：当前页行账切片导出（渲染账 20 行定长，不足补
//!      空行），页脚行 = "page k/last"（冻结格式）；
//!    - 键序账：按键序列确定性重放，终态页码与逐步账一致；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C600，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C600;

// ── 分页冻结值 ──────────────────────────────────────────────────
/// 每页行数。
pub const PAGE_ROWS: usize = 20;
/// 键位闭集。
pub const KEY_UP: u8 = 0;
pub const KEY_DOWN: u8 = 1;
pub const KEY_GOTO: u8 = 2;
pub const KEY_QUIT: u8 = 3;
pub const KEY_COUNT: usize = 4;
pub const KEY_NAMES: [&str; KEY_COUNT] = ["上翻", "下翻", "跳转", "退出"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiCode {
    /// 键位越闭集
    KeyOutOfRange,
    /// 键名空（四键齐备破坏）
    KeyNameBlank,
    /// 首页上翻（越页拒）
    UpAtFirstPage,
    /// 末页下翻（越页拒）
    DownAtLastPage,
    /// 跳转越页
    GotoOutOfRange,
    /// 报告空（无行可分页）
    EmptyReport,
    /// 页窗口行数错（≠PAGE_ROWS）
    WindowRowsWrong,
    /// 页脚格式错
    FooterFormatWrong,
    /// 键序重放不一致
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl UiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            UiCode::KeyOutOfRange => "键位越闭集：四键冻结",
            UiCode::KeyNameBlank => "键名空：齐备破坏",
            UiCode::UpAtFirstPage => "首页上翻：越页拒",
            UiCode::DownAtLastPage => "末页下翻：越页拒",
            UiCode::GotoOutOfRange => "跳转越页",
            UiCode::EmptyReport => "报告空：无行可分页",
            UiCode::WindowRowsWrong => "页窗口行数错：≠20",
            UiCode::FooterFormatWrong => "页脚格式错",
            UiCode::ReplayMismatch => "键序重放不一致",
            UiCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [UiCode; 10] = [
        UiCode::KeyOutOfRange,
        UiCode::KeyNameBlank,
        UiCode::UpAtFirstPage,
        UiCode::DownAtLastPage,
        UiCode::GotoOutOfRange,
        UiCode::EmptyReport,
        UiCode::WindowRowsWrong,
        UiCode::FooterFormatWrong,
        UiCode::ReplayMismatch,
        UiCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < UiCode::ALL.len() {
        if UiCode::ALL[i].val() == code {
            return UiCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C600 段闭集内"
}

// ── 诊断报告浏览器 ─────────────────────────────────────────────
pub struct ReportViewer {
    pub lines: Vec<String>,
    pub page: usize,
    pub last_page: usize,
    pub quit: bool,
    pub event_log: Vec<(u8, usize)>, // (键, 落页)
}

impl ReportViewer {
    pub fn new(lines: Vec<String>) -> Result<ReportViewer, u32> {
        if lines.len() == 0 {
            return Err(UiCode::EmptyReport.val());
        }
        let last = (lines.len() - 1) / PAGE_ROWS;
        Ok(ReportViewer { lines, page: 0, last_page: last, quit: false, event_log: Vec::new() })
    }

    /// 按键处理（四键闭集）。
    pub fn key(&mut self, k: u8, target: usize) -> Result<(), u32> {
        if k as usize >= KEY_COUNT {
            return Err(UiCode::KeyOutOfRange.val());
        }
        match k {
            KEY_UP => {
                if self.page == 0 {
                    return Err(UiCode::UpAtFirstPage.val());
                }
                self.page -= 1;
            }
            KEY_DOWN => {
                if self.page >= self.last_page {
                    return Err(UiCode::DownAtLastPage.val());
                }
                self.page += 1;
            }
            KEY_GOTO => {
                if target > self.last_page {
                    return Err(UiCode::GotoOutOfRange.val());
                }
                self.page = target;
            }
            _ => {
                self.quit = true;
            }
        }
        self.event_log.push((k, self.page));
        Ok(())
    }

    /// 页窗口渲染：PAGE_ROWS 定长行 + 页脚（行内容取行账切片）。
    pub fn render_window(&self) -> Result<(Vec<String>, String), u32> {
        let start = self.page * PAGE_ROWS;
        let mut w = Vec::new();
        let mut i = 0;
        while i < PAGE_ROWS {
            let idx = start + i;
            if idx < self.lines.len() {
                w.push(self.lines[idx].clone());
            } else {
                w.push(String::new());
            }
            i += 1;
        }
        if w.len() != PAGE_ROWS {
            return Err(UiCode::WindowRowsWrong.val());
        }
        let footer = format!("page {}/{}", self.page, self.last_page);
        Ok((w, footer))
    }

    /// 页脚格式点验。
    pub fn footer_check(&self, footer: &str) -> Result<(), u32> {
        let expect = format!("page {}/{}", self.page, self.last_page);
        if footer != expect {
            return Err(UiCode::FooterFormatWrong.val());
        }
        Ok(())
    }

    /// 键序重放：同键序同终态（确定性）。
    pub fn replay_check(&self, keys: &[(u8, usize)]) -> Result<usize, u32> {
        let mut v = ReportViewer::new(self.lines.clone())?;
        let mut i = 0;
        while i < keys.len() {
            v.key(keys[i].0, keys[i].1)?;
            i += 1;
        }
        if v.page != self.page || v.quit != self.quit {
            return Err(UiCode::ReplayMismatch.val());
        }
        Ok(v.page)
    }
}

/// 四键齐备账。
pub fn keys_ready() -> bool {
    if KEY_NAMES.len() != KEY_COUNT {
        return false;
    }
    let mut i = 0;
    while i < KEY_COUNT {
        if KEY_NAMES[i].len() == 0 {
            return false;
        }
        i += 1;
    }
    true
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 样本报告（55 行确定性）。
pub fn sample_report(n: usize) -> Vec<String> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < n {
        v.push(format!("report line {:03}", i));
        i += 1;
    }
    v
}

/// 黄金全链：四键全走→窗口渲染→页脚点验→键序重放。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    if !keys_ready() {
        return Err(UiCode::KeyNameBlank.val());
    }
    let mut v = ReportViewer::new(sample_report(55))?;
    // 上翻在首页拒。
    if v.key(KEY_UP, 0).is_ok() {
        return Err(UiCode::UpAtFirstPage.val());
    }
    // 下翻×2 → 页 2。
    v.key(KEY_DOWN, 0)?;
    v.key(KEY_DOWN, 0)?;
    // 跳转回页 0。
    v.key(KEY_GOTO, 0)?;
    // 窗口渲染。
    let (w, footer) = v.render_window()?;
    v.footer_check(&footer)?;
    if w.len() != PAGE_ROWS {
        return Err(UiCode::WindowRowsWrong.val());
    }
    // 键序重放。
    let keys = [(KEY_DOWN, 0usize), (KEY_DOWN, 0)];
    let page = v.replay_check(&keys)?;
    // 退出。
    v.key(KEY_QUIT, 0)?;
    if !v.quit {
        return Err(UiCode::ReplayMismatch.val());
    }
    Ok((v.last_page, page, v.event_log.len()))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut v = ReportViewer::new(sample_report(45))?;
    v.key(KEY_DOWN, 0)?;
    v.render_window().map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 首页上翻 → 必拒。
    let mut v = ReportViewer::new(sample_report(45))?;
    v.key(KEY_UP, 0)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 跳转越页 → 必拒。
    let mut v = ReportViewer::new(sample_report(45))?;
    v.key(KEY_GOTO, 9)
}

/// 注入 10/10：首页上翻、末页下翻、越页跳转、坏键四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：首页上翻。
        let mut v0 = ReportViewer::new(sample_report(45)).ok();
        let h0 = match &mut v0 {
            Some(v) => v.key(KEY_UP, 0).is_err(),
            None => false,
        };
        led.record(0, h0);
        // 槽1：末页下翻。
        let mut v1 = ReportViewer::new(sample_report(45)).ok();
        let h1 = match &mut v1 {
            Some(v) => {
                v.key(KEY_GOTO, v.last_page).ok();
                v.key(KEY_DOWN, 0).is_err()
            }
            None => false,
        };
        led.record(1, h1);
        // 槽2：越页跳转。
        let mut v2 = ReportViewer::new(sample_report(45)).ok();
        let h2 = match &mut v2 {
            Some(v) => v.key(KEY_GOTO, 99).is_err(),
            None => false,
        };
        led.record(2, h2);
        // 槽3：坏键。
        let mut v3 = ReportViewer::new(sample_report(45)).ok();
        let h3 = match &mut v3 {
            Some(v) => v.key(9, 0).is_err(),
            None => false,
        };
        led.record(3, h3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(UiCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < UiCode::ALL.len() {
        let v = UiCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 四键全可用。
    v.push((
        "J1-四键全可用",
        keys_ready() && KEY_NAMES.len() == 4,
        format!("{:?}", KEY_NAMES),
    ));
    // 分页算术：55 行 → 3 页（0..2）。
    let vv = ReportViewer::new(sample_report(55));
    v.push((
        "J1-分页算术55行3页",
        match &vv {
            Ok(vv) => vv.last_page == 2,
            Err(_) => false,
        },
        format!("last {:?}", vv.as_ref().map(|x| x.last_page)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C600 窗口")));
    v
}

pub fn run_unx0208_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0208 报告展示接口（诊断模式 UI）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
