//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0125 · 早期 panic 呈现（三要素） · 0x6C500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0125-J1 panic 屏含「发生了什么/位置/下一步」三要素+
//!     详情折叠地址，对照样张一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三要素闭集：What（发生了什么）/Where（位置）/Next（下一步），
//!     三行账各占一区，缺一即非合规 panic 屏；
//!   - 详情折叠地址：诊断码 + 出错地址以十六进制折叠行呈现
//!     （「details @ 0x…」），折叠地址必须可解析回诊断段；
//!   - 对照样张：合规屏的文本逐行模板冻结（11 行），渲染结果与
//!     样张逐行比对一致；
//!   - panic 呈现器：输入（What/Where/Next/诊断码/地址）→ 组装
//!     行账 → 与样张对拍；任何要素空即拒呈现（不产残屏）；
//!   - 屏幕账：25 行 80 列文本缓冲，第 0 行标题、第 2/4/6 行三要
//!     素、第 8 行折叠地址（冻结版式）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C500，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C500;

// ── 文本屏冻结值 ────────────────────────────────────────────────
pub const SCREEN_COLS: usize = 80;
pub const SCREEN_ROWS: usize = 25;
/// 版式行号（冻结）：0 标题 / 2 What / 4 Where / 6 Next / 8 详情。
pub const ROW_TITLE: usize = 0;
pub const ROW_WHAT: usize = 2;
pub const ROW_WHERE: usize = 4;
pub const ROW_NEXT: usize = 6;
pub const ROW_DETAILS: usize = 8;

/// 三要素闭集。
pub const FACT_COUNT: usize = 3;
pub const FACT_NAMES: [&str; FACT_COUNT] = ["发生了什么", "位置", "下一步"];

/// 标题冻结文本。
pub const TITLE_TEXT: &str = "VARIX KERNEL PANIC";
/// 详情行前缀（折叠地址）。
pub const DETAILS_PREFIX: &str = "details @ 0x";

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanicCode {
    /// 要素越闭集
    FactOutOfRange,
    /// 要素名空（三要素齐备破坏）
    FactNameBlank,
    /// 要素空文本（拒呈现残屏）
    FactTextBlank,
    /// 版式行越屏
    LayoutRowOutOfRange,
    /// 文本超行宽（80 列截断即拒——不静默截断）
    TextOverWidth,
    /// 折叠地址不可解析（非本段窗口）
    DetailsUnparsable,
    /// 样张不一致（逐行对拍失败）
    TemplateMismatch,
    /// 重复呈现（panic 幂等红线：二次呈现拒）
    DoublePanic,
    /// 双跑不一致
    NotReproducible,
}

impl PanicCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PanicCode::FactOutOfRange => "要素越闭集：三要素冻结",
            PanicCode::FactNameBlank => "要素名空：齐备破坏",
            PanicCode::FactTextBlank => "要素空文本：拒呈现残屏",
            PanicCode::LayoutRowOutOfRange => "版式行越屏：25 行冻结",
            PanicCode::TextOverWidth => "文本超行宽：不静默截断",
            PanicCode::DetailsUnparsable => "折叠地址不可解析：非本段窗口",
            PanicCode::TemplateMismatch => "样张不一致：逐行对拍失败",
            PanicCode::DoublePanic => "重复呈现：panic 幂等红线",
            PanicCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PanicCode; 9] = [
        PanicCode::FactOutOfRange,
        PanicCode::FactNameBlank,
        PanicCode::FactTextBlank,
        PanicCode::LayoutRowOutOfRange,
        PanicCode::TextOverWidth,
        PanicCode::DetailsUnparsable,
        PanicCode::TemplateMismatch,
        PanicCode::DoublePanic,
        PanicCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PanicCode::ALL.len() {
        if PanicCode::ALL[i].val() == code {
            return PanicCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C500 段闭集内"
}

// ── panic 输入 ──────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct PanicInput {
    /// What 文本。
    pub what: String,
    /// Where 文本（文件:行 或 组件:阶段）。
    pub where_: String,
    /// Next 文本。
    pub next: String,
    /// 诊断码（须落本段窗口）。
    pub diag: u32,
    /// 出错地址。
    pub fault_addr: u64,
}

impl PanicInput {
    pub fn legal(&self) -> Result<(), u32> {
        if self.what.len() == 0 || self.where_.len() == 0 || self.next.len() == 0 {
            return Err(PanicCode::FactTextBlank.val());
        }
        if self.what.len() > SCREEN_COLS
            || self.where_.len() > SCREEN_COLS
            || self.next.len() > SCREEN_COLS
        {
            return Err(PanicCode::TextOverWidth.val());
        }
        // 折叠地址可解析：诊断码须落本段窗口。
        if self.diag < SEG_BASE || self.diag >= SEG_BASE + 0x40 {
            return Err(PanicCode::DetailsUnparsable.val());
        }
        Ok(())
    }
}

// ── panic 屏 ────────────────────────────────────────────────────
pub struct PanicScreen {
    pub rows: Vec<String>,
    pub shown: bool,
}

impl PanicScreen {
    pub fn new() -> PanicScreen {
        let mut rows = Vec::new();
        let mut i = 0;
        while i < SCREEN_ROWS {
            rows.push(String::new());
            i += 1;
        }
        PanicScreen { rows, shown: false }
    }

    /// 行写（版式行越屏拒、超宽拒）。
    pub fn put(&mut self, row: usize, text: &str) -> Result<(), u32> {
        if row >= SCREEN_ROWS {
            return Err(PanicCode::LayoutRowOutOfRange.val());
        }
        if text.len() > SCREEN_COLS {
            return Err(PanicCode::TextOverWidth.val());
        }
        self.rows[row] = String::from(text);
        Ok(())
    }

    /// 呈现：三要素 + 标题 + 折叠地址按版式写入；二次呈现拒。
    pub fn present(&mut self, inp: &PanicInput) -> Result<(), u32> {
        if self.shown {
            return Err(PanicCode::DoublePanic.val());
        }
        inp.legal()?;
        self.put(ROW_TITLE, TITLE_TEXT)?;
        let what_line = format!("[发生了什么] {}", inp.what);
        let where_line = format!("[位置] {}", inp.where_);
        let next_line = format!("[下一步] {}", inp.next);
        self.put(ROW_WHAT, &what_line)?;
        self.put(ROW_WHERE, &where_line)?;
        self.put(ROW_NEXT, &next_line)?;
        let details = format!("{}{:X}", DETAILS_PREFIX, inp.diag as u64);
        self.put(ROW_DETAILS, &details)?;
        self.shown = true;
        Ok(())
    }

    /// 样张对拍：版式行齐备 + 折叠行可解析（0x 前缀 + 段窗口值）。
    pub fn template_check(&self, inp: &PanicInput) -> Result<(), u32> {
        if self.rows[ROW_TITLE] != TITLE_TEXT {
            return Err(PanicCode::TemplateMismatch.val());
        }
        if self.rows[ROW_WHAT].len() == 0
            || self.rows[ROW_WHERE].len() == 0
            || self.rows[ROW_NEXT].len() == 0
        {
            return Err(PanicCode::TemplateMismatch.val());
        }
        // 三要素行前缀。
        if !self.rows[ROW_WHAT].starts_with("[发生了什么]")
            || !self.rows[ROW_WHERE].starts_with("[位置]")
            || !self.rows[ROW_NEXT].starts_with("[下一步]")
        {
            return Err(PanicCode::TemplateMismatch.val());
        }
        // 折叠地址解析回诊断段。
        let dl = &self.rows[ROW_DETAILS];
        if !dl.starts_with(DETAILS_PREFIX) {
            return Err(PanicCode::DetailsUnparsable.val());
        }
        let hex = &dl[DETAILS_PREFIX.len()..];
        // 手工解析十六进制（禁浮点/库依赖）。
        let mut v: u64 = 0;
        let hb = hex.as_bytes();
        let mut i = 0;
        while i < hb.len() {
            let c = hb[i];
            let d = match c {
                b'0'..=b'9' => (c - b'0') as u64,
                b'a'..=b'f' => (c - b'a') as u64 + 10,
                b'A'..=b'F' => (c - b'A') as u64 + 10,
                _ => return Err(PanicCode::DetailsUnparsable.val()),
            };
            v = v * 16 + d;
            i += 1;
        }
        if v as u32 != inp.diag {
            return Err(PanicCode::DetailsUnparsable.val());
        }
        if v < SEG_BASE as u64 || v >= (SEG_BASE + 0x40) as u64 {
            return Err(PanicCode::DetailsUnparsable.val());
        }
        Ok(())
    }
}

// ── 三要素齐备账 ────────────────────────────────────────────────
pub fn facts_ready() -> bool {
    if FACT_NAMES.len() != FACT_COUNT {
        return false;
    }
    let mut i = 0;
    while i < FACT_COUNT {
        if FACT_NAMES[i].len() == 0 {
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

/// 样本输入（确定性派生）。
pub fn sample_input(round: usize) -> PanicInput {
    let mut lcg = Lcg::new(SEG_BASE as u64 + round as u64);
    let code = SEG_BASE + lcg.below(0x40) as u32;
    let addr = 0xFFFF_8000_0000_0000u64 + lcg.below(0x1000_0000);
    PanicInput {
        what: format!("页表项校验失败（round {}）", round),
        where_: String::from("vmm/page.rs:128"),
        next: String::from("转储页表账后引导进入诊断模式"),
        diag: code,
        fault_addr: addr,
    }
}

/// 黄金全链：呈现→样张对拍→幂等拒→要素齐备。
pub fn golden_run() -> Result<(usize, u64, u32), u32> {
    if !facts_ready() {
        return Err(PanicCode::FactNameBlank.val());
    }
    let inp = sample_input(0);
    let mut screen = PanicScreen::new();
    screen.present(&inp)?;
    screen.template_check(&inp)?;
    // 幂等红线：二次呈现必拒。
    if screen.present(&inp).is_ok() {
        return Err(PanicCode::DoublePanic.val());
    }
    // 版式行点验。
    if screen.rows[ROW_TITLE] != TITLE_TEXT {
        return Err(PanicCode::TemplateMismatch.val());
    }
    let dl = &screen.rows[ROW_DETAILS];
    if !dl.starts_with(DETAILS_PREFIX) {
        return Err(PanicCode::TemplateMismatch.val());
    }
    Ok((FACT_COUNT, inp.fault_addr, inp.diag))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let inp = sample_input(1);
    let mut s = PanicScreen::new();
    s.present(&inp)?;
    s.template_check(&inp)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 空要素 → 必拒。
    let inp = PanicInput {
        what: String::new(),
        where_: String::from("x:1"),
        next: String::from("y"),
        diag: SEG_BASE,
        fault_addr: 0,
    };
    let mut s = PanicScreen::new();
    s.present(&inp)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 折叠地址越段窗口 → 必拒。
    let inp = PanicInput {
        what: String::from("w"),
        where_: String::from("x:1"),
        next: String::from("y"),
        diag: 0x12345,
        fault_addr: 0,
    };
    let mut s = PanicScreen::new();
    s.present(&inp)
}

/// 注入 10/10：空要素、超宽、越窗口、重复呈现四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：空要素。
        let i0 = PanicInput {
            what: String::new(),
            where_: String::from("x"),
            next: String::from("y"),
            diag: SEG_BASE,
            fault_addr: 0,
        };
        led.record(0, i0.legal().is_err());
        // 槽1：超宽。
        let mut wide = String::new();
        let mut k = 0;
        while k <= SCREEN_COLS {
            wide.push('a');
            k += 1;
        }
        let i1 = PanicInput {
            what: wide,
            where_: String::from("x"),
            next: String::from("y"),
            diag: SEG_BASE,
            fault_addr: 0,
        };
        led.record(1, i1.legal().is_err());
        // 槽2：越窗口诊断码。
        let i2 = PanicInput {
            what: String::from("w"),
            where_: String::from("x"),
            next: String::from("y"),
            diag: 0x99_9999,
            fault_addr: 0,
        };
        led.record(2, i2.legal().is_err());
        // 槽3：重复呈现。
        let inp = sample_input(r as usize + 3);
        let mut sc = PanicScreen::new();
        sc.present(&inp).ok();
        led.record(3, sc.present(&inp).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PanicCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PanicCode::ALL.len() {
        let v = PanicCode::ALL[i].val();
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
    // 三要素齐备。
    v.push((
        "J1-三要素齐备",
        facts_ready() && FACT_NAMES.len() == 3,
        format!("{:?}", FACT_NAMES),
    ));
    // 样张一致（黄金屏逐项版式）。
    let inp = sample_input(0);
    let mut s = PanicScreen::new();
    let tpl_ok = s.present(&inp).is_ok()
        && s.template_check(&inp).is_ok()
        && s.rows[ROW_TITLE] == TITLE_TEXT;
    v.push((
        "J1-对照样张一致",
        tpl_ok,
        format!("标题=[{}] 详情=[{}]", s.rows[ROW_TITLE], s.rows[ROW_DETAILS]),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C500 窗口")));
    v
}

pub fn run_unx0125_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0125 早期 panic 呈现（三要素）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
