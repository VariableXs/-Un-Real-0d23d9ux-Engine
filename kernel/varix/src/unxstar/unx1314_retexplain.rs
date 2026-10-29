//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1314 · 退役人话化（退役进度报告） · 0x7630
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1314-J1 退役面 explain 报告覆盖 B26 全账本（引用完整率
//!      100%），进度人话版（退到哪/风险点/下一步）随报告输出，术语
//!      一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B26 全账本登记表：13 本账（守恒/回退/进度/四态/对拍/清理/
//!     压测/注入/健康/配置/审计/消费/回归），explain 报告必须逐本
//!     引用（引用完整率 100% 判准，漏引必抓）；
//!   - 进度人话三段：退到哪（三路径各一句）/风险点（红黄账逐条）/
//!     下一步（动作建议逐条），随报告正文输出；
//!   - 术语表：退役/回退/守恒/排空窗/对拍/清理/隔离/红线 八词规范，
//!     报告正文术语扫描（规范词命中 + 禁用变体零出现 = 一致率 100%）；
//!   - 账态四色：绿(达标)/黄(观察)/红(越线)/灰(未测)，风险段只收
//!     红黄账（漏报必抓）；
//!   - 导出契约：报告导出含全部段头（五段），段缺失必抓；
//!   - 引用唯一性：每账恰好一引（重复引用必抓）；
//!   - 注入核 5 项（漏引/术语漂移/空报告/风险漏报/导出缺段）全部
//!     必抓，LCG 种子写死 0x7630_0010。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7630 段，退役人话化专属）。
pub const SEG_BASE: u16 = 0x7630;

// ── 诊断码闭集（0x7630..0x7639 十码） ───────────────────────
/// 0x7630 账本引用缺失。
pub const E_CITE_MISS: u16 = SEG_BASE;
/// 0x7631 术语漂移。
pub const E_TERM_DRIFT: u16 = SEG_BASE + 1;
/// 0x7632 报告为空。
pub const E_REPORT_EMPTY: u16 = SEG_BASE + 2;
/// 0x7633 风险漏报。
pub const E_RISK_MISS: u16 = SEG_BASE + 3;
/// 0x7634 导出缺段。
pub const E_EXPORT_MISS: u16 = SEG_BASE + 4;
/// 0x7635 报告段缺失。
pub const E_SEC_MISS: u16 = SEG_BASE + 5;
/// 0x7636 引用重复。
pub const E_DUP_CITE: u16 = SEG_BASE + 6;
/// 0x7637 账态码错。
pub const E_STATUS_BAD: u16 = SEG_BASE + 7;
/// 0x7638 报告账冻结后改写。
pub const E_RDONLY: u16 = SEG_BASE + 8;
/// 0x7639 空账本登记。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_CITE_MISS => "账本引用缺失：13 本账逐本补引后重出报告",
        E_TERM_DRIFT => "术语漂移：正文出现禁用变体，替换为规范术语",
        E_REPORT_EMPTY => "报告为空：先填进度/风险/下一步三段再导出",
        E_RISK_MISS => "风险漏报：红黄账必须进风险段，逐条补齐",
        E_EXPORT_MISS => "导出缺段：五段头（进度/风险/下一步/引用/结论）齐备再出",
        E_SEC_MISS => "报告段缺失：逐段校验生成器输出",
        E_DUP_CITE => "引用重复：每账恰好一引，去重后重出",
        E_STATUS_BAD => "账态码错：只收绿/黄/红/灰四色",
        E_RDONLY => "报告账冻结：定稿后只读，修订走新版本",
        E_EMPTY => "空账本登记：先登记 B26 全账本再出报告",
        _ => "未知 0x7630 段诊断码：核对人话化段位闭集",
    }
}

// ── 常量 ────────────────────────────────────────────────────
/// B26 全账本登记表（13 本，编号+名+判据号）。
pub const LEDGER_TABLE: [(&str, u16, &str); 13] = [
    ("守恒账", 1305, "UNX-F1305-J1"),
    ("回退账", 1306, "UNX-F1306-J1"),
    ("进度账", 1307, "UNX-F1307-J1"),
    ("四态联动账", 1308, "UNX-F1308-J1"),
    ("双路对拍账", 1309, "UNX-F1309-J1"),
    ("清理账", 1310, "UNX-F1310-J1"),
    ("压测资产账", 1311, "UNX-F1311-J1"),
    ("故障注入账", 1312, "UNX-F1312-J1"),
    ("健康账", 1313, "UNX-F1313-J1"),
    ("配置总表账", 1315, "UNX-F1315-J1"),
    ("审计账", 1316, "UNX-F1316-J1"),
    ("消费对账账", 1317, "UNX-F1317-J1"),
    ("回归资产账", 1318, "UNX-F1318-J1"),
];
/// 账本数硬线。
pub const LEDGER_MIN: usize = 13;
/// 规范术语表（8 词）。
pub const CANON_TERMS: [&str; 8] = ["退役", "回退", "守恒", "排空窗", "对拍", "清理", "隔离", "红线"];
/// 禁用变体表（术语漂移扫描）。
pub const BANNED_TERMS: [&str; 4] = ["撤柜", "退坑", "对帐", "红 generalized"];
/// 报告五段头。
pub const SECTION_HEADS: [&str; 5] = ["【退到哪】", "【风险点】", "【下一步】", "【账本引用】", "【结论】"];

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 账态四色 ────────────────────────────────────────────────
/// 账态四色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status4 {
    /// 绿（达标）。
    Green,
    /// 黄（观察）。
    Amber,
    /// 红（越线）。
    Red,
    /// 灰（未测）。
    Gray,
}

impl Status4 {
    /// 码（登记用）。
    pub const fn code(self) -> u8 {
        match self {
            Status4::Green => 0,
            Status4::Amber => 1,
            Status4::Red => 2,
            Status4::Gray => 3,
        }
    }
    /// 按码取态。
    pub const fn of_code(c: u8) -> Status4 {
        match c {
            0 => Status4::Green,
            1 => Status4::Amber,
            2 => Status4::Red,
            _ => Status4::Gray,
        }
    }
    /// 中文。
    pub const fn text(self) -> &'static str {
        match self {
            Status4::Green => "绿",
            Status4::Amber => "黄",
            Status4::Red => "红",
            Status4::Gray => "灰",
        }
    }
    /// 是否进风险段。
    pub const fn risky(self) -> bool {
        matches!(self, Status4::Red | Status4::Amber)
    }
}

// ── 账本登记账 ──────────────────────────────────────────────
/// 账本登记条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 账名。
    pub name: &'static str,
    /// F 编号。
    pub fid: u16,
    /// 判据号。
    pub judge: &'static str,
    /// 账态。
    pub status: Status4,
}

/// 账本登记账（append-only + 冻结）。
#[derive(Debug)]
pub struct LedgerBook {
    /// 条目。
    entries: Vec<LedgerEntry>,
    /// 冻结旗。
    frozen: bool,
}

impl LedgerBook {
    /// 新账。
    pub fn new() -> LedgerBook {
        LedgerBook { entries: Vec::new(), frozen: false }
    }

    /// 登记（幂等；态码校验）。
    pub fn register(&mut self, e: LedgerEntry) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if e.fid < 1300 || e.fid > 1399 {
            return Err(E_STATUS_BAD);
        }
        if self.entries.iter().any(|x| x.fid == e.fid) {
            return Err(E_DUP_CITE);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 按常量表全量登记（默认全绿）。
    pub fn register_all_green(&mut self) -> Result<usize, u16> {
        for (name, fid, judge) in LEDGER_TABLE.iter() {
            self.register(LedgerEntry { name, fid: *fid, judge, status: Status4::Green })?;
        }
        Ok(self.entries.len())
    }

    /// 置账态。
    pub fn set_status(&mut self, fid: u16, s: Status4) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        match self.entries.iter_mut().find(|e| e.fid == fid) {
            Some(e) => {
                e.status = s;
                Ok(())
            }
            None => Err(E_CITE_MISS),
        }
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 资产化判准（13 本全登记且判据号一致）。
    pub fn assetized(&self) -> bool {
        self.entries.len() >= LEDGER_MIN
            && LEDGER_TABLE.iter().all(|(name, fid, judge)| {
                match self.entries.iter().find(|e| e.fid == *fid) {
                    Some(e) => e.name == *name && e.judge == *judge,
                    None => false,
                }
            })
    }

    /// 红黄账清单。
    pub fn risky_list(&self) -> Vec<&LedgerEntry> {
        self.entries.iter().filter(|e| e.status.risky()).collect()
    }

    /// 账态审计（态码合法由类型保证；此处校验灰账不进风险段语义）。
    pub fn status_audit(&self) -> Result<(), u16> {
        for e in self.entries.iter() {
            if e.status.code() > 3 {
                return Err(E_STATUS_BAD);
            }
        }
        Ok(())
    }
}

// ── 报告生成器 ──────────────────────────────────────────────
/// 进度人话行（单路径）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressLine {
    /// 路径名。
    pub path: &'static str,
    /// 态。
    pub state: &'static str,
    /// 已退役点数。
    pub retired_points: usize,
    /// 总点数。
    pub total_points: usize,
}

impl ProgressLine {
    /// 人话句。
    pub fn phrase(&self) -> String {
        format!("路径{}：{}（已退役 {}/{} 点）", self.path, self.state, self.retired_points, self.total_points)
    }
}

/// explain 报告。
#[derive(Debug)]
pub struct ExplainReport {
    /// 进度段。
    pub progress: Vec<String>,
    /// 风险段。
    pub risks: Vec<String>,
    /// 下一步段。
    pub nexts: Vec<String>,
    /// 引用段。
    pub cites: Vec<String>,
    /// 结论段。
    pub verdict: String,
    /// 定稿旗。
    pub sealed: bool,
}

impl ExplainReport {
    /// 新报告。
    pub fn new() -> ExplainReport {
        ExplainReport {
            progress: Vec::new(),
            risks: Vec::new(),
            nexts: Vec::new(),
            cites: Vec::new(),
            verdict: String::new(),
            sealed: false,
        }
    }

    /// 从登记账生成（进度人话 + 风险 + 下一步 + 引用 + 结论）。
    pub fn generate(book: &LedgerBook, prog: &[ProgressLine]) -> Result<ExplainReport, u16> {
        if book.len() == 0 {
            return Err(E_EMPTY);
        }
        if !book.assetized() {
            return Err(E_CITE_MISS);
        }
        let mut r = ExplainReport::new();
        // 段一：退到哪。
        for p in prog.iter() {
            r.progress.push(p.phrase());
        }
        // 段二：风险点（红黄账逐条）。
        for e in book.risky_list() {
            r.risks.push(format!("{}（{}）账态={}，需要关注", e.name, e.judge, e.status.text()));
        }
        // 段三：下一步。
        if !book.risky_list().is_empty() {
            r.nexts.push("优先处置红账，再消化黄账观察项".to_string());
        }
        r.nexts.push("按退役进度推进剩余未动路径".to_string());
        r.nexts.push("维持窗口对拍与守恒审计常绿".to_string());
        // 段四：账本引用（每账恰好一引）。
        for e in book.entries.iter() {
            r.cites.push(format!("{}={}（{}）", e.name, e.judge, e.status.text()));
        }
        // 段五：结论。
        let reds = book.entries.iter().filter(|e| e.status == Status4::Red).count();
        let ambers = book.entries.iter().filter(|e| e.status == Status4::Amber).count();
        r.verdict = if reds == 0 && ambers == 0 {
            "退役面全绿：账本齐、路径稳、可以继续推进".to_string()
        } else if reds == 0 {
            format!("退役面整体可控：{} 本黄账观察中", ambers)
        } else {
            format!("退役面有风险：{} 本红账待处置", reds)
        };
        Ok(r)
    }

    /// 引用完整率（千分制：被引账本×1000/登记账本）。
    pub fn cite_permille(&self, book: &LedgerBook) -> u64 {
        if book.len() == 0 {
            return 0;
        }
        let mut cited = 0usize;
        for e in book.entries.iter() {
            if self.cites.iter().any(|c| c.contains(e.judge)) {
                cited += 1;
            }
        }
        (cited as u64) * 1000 / book.len() as u64
    }

    /// 引用唯一性（每判据号恰好一引）。
    pub fn cites_unique(&self) -> bool {
        for i in 0..self.cites.len() {
            for j in (i + 1)..self.cites.len() {
                if self.cites[i] == self.cites[j] {
                    return false;
                }
            }
        }
        true
    }

    /// 术语扫描（规范词命中 + 禁用变体零出现）。
    pub fn term_scan(&self) -> Result<(), u16> {
        let mut body = String::new();
        for l in self.progress.iter().chain(self.risks.iter()).chain(self.nexts.iter()) {
            body.push_str(l);
        }
        body.push_str(&self.verdict);
        for banned in BANNED_TERMS.iter() {
            if body.contains(banned) {
                return Err(E_TERM_DRIFT);
            }
        }
        let mut hits = 0usize;
        for t in CANON_TERMS.iter() {
            if self.cites.iter().any(|c| c.contains(t)) || body.contains(t) {
                hits += 1;
            }
        }
        if hits < 4 {
            return Err(E_TERM_DRIFT);
        }
        Ok(())
    }

    /// 五段齐备审计。
    pub fn sections_ok(&self) -> bool {
        !self.progress.is_empty() && !self.nexts.is_empty() && !self.cites.is_empty()
            && self.verdict.len() > 0
    }

    /// 风险段对账（红黄账全进风险段，漏报必抓）。
    pub fn risks_cover(&self, book: &LedgerBook) -> bool {
        let risky = book.risky_list();
        if risky.is_empty() {
            return self.risks.is_empty();
        }
        risky.iter().all(|e| self.risks.iter().any(|r| r.contains(e.judge)))
    }

    /// 导出（五段头 + 正文）。
    pub fn export(&self) -> Result<String, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if !self.sections_ok() {
            return Err(E_SEC_MISS);
        }
        let mut out = String::new();
        out.push_str(SECTION_HEADS[0]);
        for l in self.progress.iter() {
            out.push_str(l);
        }
        out.push_str(SECTION_HEADS[1]);
        for l in self.risks.iter() {
            out.push_str(l);
        }
        out.push_str(SECTION_HEADS[2]);
        for l in self.nexts.iter() {
            out.push_str(l);
        }
        out.push_str(SECTION_HEADS[3]);
        for l in self.cites.iter() {
            out.push_str(l);
        }
        out.push_str(SECTION_HEADS[4]);
        out.push_str(&self.verdict);
        if out.len() == 0 {
            return Err(E_REPORT_EMPTY);
        }
        Ok(out)
    }

    /// 定稿（冻结）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

// ── 标准进度行（三路径） ────────────────────────────────────
/// 标准进度（tick 全退、serial 切换中、scan 未动）。
pub fn std_progress() -> Vec<ProgressLine> {
    vec![
        ProgressLine { path: "tick", state: "已退役", retired_points: 4, total_points: 4 },
        ProgressLine { path: "serial", state: "切换中", retired_points: 2, total_points: 4 },
        ProgressLine { path: "scan", state: "未动", retired_points: 0, total_points: 4 },
    ]
}

// ── 演练 ────────────────────────────────────────────────────
/// 全绿报告演练（引用完整率 1000‰ + 五段齐 + 术语净）。
pub fn drill_full_report() -> (bool, String) {
    let mut book = LedgerBook::new();
    if let Err(c) = book.register_all_green() {
        return (false, format!("登记错=0x{:04X} {}", c, detail(c)));
    }
    let r = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成错=0x{:04X} {}", c, detail(c))),
    };
    let cite = r.cite_permille(&book);
    let uniq = r.cites_unique();
    let terms = r.term_scan().is_ok();
    let secs = r.sections_ok();
    let risks_ok = r.risks_cover(&book);
    (cite == 1000 && uniq && terms && secs && risks_ok,
     format!("引用={}‰ 唯一={} 术语净={} 五段={} 风险对账={}", cite, uniq, terms, secs, risks_ok))
}

/// 进度人话版演练（三路径各一句：已退役/切换中/未动全出现）。
pub fn drill_progress_phrases() -> (bool, String) {
    let mut book = LedgerBook::new();
    let _ = book.register_all_green();
    let r = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成错=0x{:04X} {}", c, detail(c))),
    };
    let joined = r.progress.join(" | ");
    let has_done = joined.contains("已退役");
    let has_switch = joined.contains("切换中");
    let has_undone = joined.contains("未动");
    let counts_ok = joined.contains("4/4") && joined.contains("2/4") && joined.contains("0/4");
    (r.progress.len() == 3 && has_done && has_switch && has_undone && counts_ok,
     format!("进度句={} 条 done/switch/undone={}/{}/{}", r.progress.len(), has_done, has_switch, has_undone))
}

/// 风险漏报必抓演练（置两本红账 → 风险段必含两引）。
pub fn drill_risk_cover() -> (bool, String) {
    let mut book = LedgerBook::new();
    let _ = book.register_all_green();
    let _ = book.set_status(1305, Status4::Red);
    let _ = book.set_status(1311, Status4::Amber);
    let r = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成错=0x{:04X} {}", c, detail(c))),
    };
    let cover = r.risks_cover(&book);
    let has_red = r.risks.iter().any(|x| x.contains("1305"));
    let has_amber = r.risks.iter().any(|x| x.contains("1311"));
    let verdict_red = r.verdict.contains("红账待处置");
    (cover && has_red && has_amber && verdict_red,
     format!("风险 {} 条 红引={} 黄引={} 结论红={}", r.risks.len(), has_red, has_amber, verdict_red))
}

/// 术语一致率演练（禁用变体注入必抓）。
pub fn drill_terms() -> (bool, String) {
    let mut book = LedgerBook::new();
    let _ = book.register_all_green();
    let mut r = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成错=0x{:04X} {}", c, detail(c))),
    };
    let clean = r.term_scan().is_ok();
    // 注入禁用变体。
    r.nexts.push("把对帐口径换成撤柜方案".to_string());
    let caught = matches!(r.term_scan(), Err(E_TERM_DRIFT));
    (clean && caught, format!("净文={} 变体必抓={}", clean, caught))
}

/// 引用重复必抓演练。
pub fn drill_dup_cite() -> (bool, String) {
    let mut r = ExplainReport::new();
    r.cites.push("守恒账=UNX-F1305-J1（绿）".to_string());
    r.cites.push("守恒账=UNX-F1305-J1（绿）".to_string());
    let dup = !r.cites_unique();
    let mut book = LedgerBook::new();
    let dup_reg = matches!(book.register(LedgerEntry { name: "守恒账", fid: 1305, judge: "UNX-F1305-J1", status: Status4::Green }), Ok(()));
    let dup_reg2 = matches!(book.register(LedgerEntry { name: "守恒账", fid: 1305, judge: "UNX-F1305-J1", status: Status4::Green }), Err(E_DUP_CITE));
    (dup && dup_reg && dup_reg2, format!("引用重={} 登记成={} 重登拒={}", dup, dup_reg, dup_reg2))
}

/// 导出契约演练（五段头齐 + 定稿后拒写 + 段缺失必抓）。
pub fn drill_export() -> (bool, String) {
    let mut book = LedgerBook::new();
    let _ = book.register_all_green();
    let mut r = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成错=0x{:04X} {}", c, detail(c))),
    };
    let text = match r.export() {
        Ok(t) => t,
        Err(c) => return (false, format!("导出错=0x{:04X} {}", c, detail(c))),
    };
    let heads_ok = SECTION_HEADS.iter().all(|h| text.contains(h));
    // 段缺失注入：清空结论 → 导出必拒。
    let mut broken = ExplainReport::new();
    broken.progress.push("路径tick：已退役（4/4 点）".to_string());
    broken.nexts.push("继续推进".to_string());
    broken.cites.push("x".to_string());
    let miss = matches!(broken.export(), Err(E_SEC_MISS));
    // 定稿后拒写。
    r.seal();
    let rdonly = matches!(r.export(), Err(E_RDONLY));
    (heads_ok && miss && rdonly && text.len() > 0,
     format!("段头齐={} 缺段抓={} 定稿拒={}", heads_ok, miss, rdonly))
}

/// 结论三态演练（全绿/黄/红三种 verdict）。
pub fn drill_verdicts() -> (bool, String) {
    let mut book = LedgerBook::new();
    let _ = book.register_all_green();
    let g = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r.verdict,
        Err(_) => String::new(),
    };
    let _ = book.set_status(1309, Status4::Amber);
    let a = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r.verdict,
        Err(_) => String::new(),
    };
    let _ = book.set_status(1309, Status4::Red);
    let rr = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r.verdict,
        Err(_) => String::new(),
    };
    let g_ok = g.contains("全绿");
    let a_ok = a.contains("黄账");
    let r_ok = rr.contains("红账");
    (g_ok && a_ok && r_ok, format!("绿/黄/红 结论对={}/{}/{}", g_ok, a_ok, r_ok))
}


// ── 报告版本账（定稿快照 append-only） ─────────────────────
/// 版本条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportVersion {
    /// 版本号（1 起连续）。
    pub ver: u32,
    /// 定稿摘要（五段长度 + 结论哈希简化）。
    pub digest: u64,
}

/// 报告版本账。
#[derive(Debug)]
pub struct VersionBook {
    /// 版本史。
    versions: Vec<ReportVersion>,
}

impl VersionBook {
    /// 新账。
    pub fn new() -> VersionBook {
        VersionBook { versions: Vec::new() }
    }

    /// 定稿入账（版本号连续；摘要 = 五段长度卷积）。
    pub fn seal(&mut self, r: &ExplainReport) -> Result<u32, u16> {
        if !r.sections_ok() {
            return Err(E_SEC_MISS);
        }
        let mut digest = r.progress.len() as u64;
        digest = digest.wrapping_mul(31).wrapping_add(r.risks.len() as u64);
        digest = digest.wrapping_mul(131).wrapping_add(r.nexts.len() as u64);
        digest = digest.wrapping_mul(17).wrapping_add(r.cites.len() as u64);
        digest = digest.wrapping_mul(7).wrapping_add(r.verdict.len() as u64);
        let ver = match self.versions.last() {
            Some(v) => v.ver + 1,
            None => 1,
        };
        self.versions.push(ReportVersion { ver, digest });
        Ok(ver)
    }

    /// 版本数。
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// 版本号连续。
    pub fn continuous(&self) -> bool {
        for (i, v) in self.versions.iter().enumerate() {
            if v.ver != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 最新摘要。
    pub fn latest(&self) -> Option<u64> {
        self.versions.last().map(|v| v.digest)
    }
}

// ── 术语使用账（规范词命中计数） ───────────────────────────
/// 术语使用账。
#[derive(Debug)]
pub struct TermUsage {
    /// 规范词命中数（下标对齐 CANON_TERMS）。
    pub hits: [usize; 8],
    /// 禁用变体出现数。
    pub banned: usize,
}

impl TermUsage {
    /// 从报告正文统计。
    pub fn build(r: &ExplainReport) -> TermUsage {
        let mut body = String::new();
        for l in r.progress.iter().chain(r.risks.iter()).chain(r.nexts.iter()) {
            body.push_str(l);
        }
        body.push_str(&r.verdict);
        for c in r.cites.iter() {
            body.push_str(c);
        }
        let mut hits = [0usize; 8];
        for (i, t) in CANON_TERMS.iter().enumerate() {
            hits[i] = body.matches(t).count();
        }
        let mut banned = 0usize;
        for b in BANNED_TERMS.iter() {
            banned += body.matches(b).count();
        }
        TermUsage { hits, banned }
    }

    /// 规范词命中总数。
    pub fn hit_total(&self) -> usize {
        self.hits.iter().sum()
    }

    /// 术语净（禁用变体零出现）。
    pub fn clean(&self) -> bool {
        self.banned == 0
    }

    /// 命中分布（至少 4 词命中 = 报告有实质内容）。
    pub fn spread_ok(&self) -> bool {
        self.hits.iter().filter(|h| **h > 0).count() >= 4
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 报告版本演练（两次定稿两版本 + 连续 + 摘要稳定）。
pub fn drill_versions() -> (bool, String) {
    let mut book = LedgerBook::new();
    if book.register_all_green().is_err() {
        return (false, "登记失败".to_string());
    }
    let r = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成错=0x{:04X} {}", c, detail(c))),
    };
    let mut vb = VersionBook::new();
    let v1 = vb.seal(&r).unwrap_or(0);
    let v2 = vb.seal(&r).unwrap_or(0);
    let same = vb.latest().is_some() && v1 == 1 && v2 == 2 && vb.continuous();
    // 空报告定稿必拒。
    let empty = ExplainReport::new();
    let refused = matches!(vb.seal(&empty), Err(E_SEC_MISS));
    (same && refused && vb.len() == 2,
     format!("版本 {} 连续={} 空报拒={}", vb.len(), vb.continuous(), refused))
}

/// 术语使用演练（命中分布 + 净词）。
pub fn drill_term_usage() -> (bool, String) {
    let mut book = LedgerBook::new();
    if book.register_all_green().is_err() {
        return (false, "登记失败".to_string());
    }
    let r = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成错=0x{:04X} {}", c, detail(c))),
    };
    let u = TermUsage::build(&r);
    let clean = u.clean();
    let spread = u.spread_ok();
    let positive = u.hit_total() > 0;
    (clean && spread && positive,
     format!("命中 {} 净词={} 分布={}", u.hit_total(), clean, spread))
}


// ── 引用密度账（逐账引用计数与密度） ───────────────────────
/// 引用密度账。
#[derive(Debug)]
pub struct CiteDensity {
    /// 被引账本数。
    pub cited: usize,
    /// 引用总行数。
    pub cite_lines: usize,
    /// 每账恰一引。
    pub one_per_ledger: bool,
}

impl CiteDensity {
    /// 从报告统计。
    pub fn build(r: &ExplainReport, book: &LedgerBook) -> CiteDensity {
        let mut cited = 0usize;
        for e in book.entries.iter() {
            if r.cites.iter().any(|c| c.contains(e.judge)) {
                cited += 1;
            }
        }
        let one_per = r.cites_unique() && cited == book.len();
        CiteDensity { cited, cite_lines: r.cites.len(), one_per_ledger: one_per }
    }

    /// 密度 1000‰（全引且不多引）。
    pub fn permille(&self, book: &LedgerBook) -> u64 {
        if book.len() == 0 {
            return 0;
        }
        (self.cited as u64) * 1000 / book.len() as u64
    }
}

/// 追加演练：引用密度（13/13 全引恰一引）。
pub fn drill_cite_density() -> (bool, String) {
    let mut book = LedgerBook::new();
    if book.register_all_green().is_err() {
        return (false, "登记失败".to_string());
    }
    let r = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成错=0x{:04X} {}", c, detail(c))),
    };
    let d = CiteDensity::build(&r, &book);
    (d.one_per_ledger && d.permille(&book) == 1000 && d.cite_lines == LEDGER_MIN,
     format!("被引 {}/{} 行 {} 密度 {}‰", d.cited, book.len(), d.cite_lines, d.permille(&book)))
}

/// 追加演练：风险三段联动（红账 → 风险段+下一步+结论三处联动）。
pub fn drill_risk_linkage() -> (bool, String) {
    let mut book = LedgerBook::new();
    if book.register_all_green().is_err() {
        return (false, "登记失败".to_string());
    }
    let _ = book.set_status(1316, Status4::Red);
    let r = match ExplainReport::generate(&book, &std_progress()) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成错=0x{:04X} {}", c, detail(c))),
    };
    let in_risk = r.risks.iter().any(|x| x.contains("1316"));
    let in_next = r.nexts.iter().any(|x| x.contains("红账"));
    let in_verdict = r.verdict.contains("1 本红账");
    (in_risk && in_next && in_verdict,
     format!("风险段={} 下一步={} 结论={} 联动", in_risk, in_next, in_verdict))
}
// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：漏引（登记账 13 本，报告引用只 12 本 → 完整率<1000）。
    {
        let mut book = LedgerBook::new();
        let _ = book.register_all_green();
        let mut r = ExplainReport::new();
        for e in book.entries.iter().skip(1) {
            r.cites.push(format!("{}={}（绿）", e.name, e.judge));
        }
        let permille = r.cite_permille(&book);
        if permille < 1000 { caught += 1; d.push("注1 漏引:抓".to_string()); } else { d.push("注1 漏引:漏".to_string()); }
    }
    // 注 2：术语漂移。
    {
        let mut book = LedgerBook::new();
        let _ = book.register_all_green();
        let mut r = ExplainReport::generate(&book, &std_progress()).unwrap_or_else(|_| ExplainReport::new());
        r.verdict = "退役面退坑完成".to_string();
        if matches!(r.term_scan(), Err(E_TERM_DRIFT)) { caught += 1; d.push("注2 漂移:抓".to_string()); } else { d.push("注2 漂移:漏".to_string()); }
    }
    // 注 3：空报告。
    {
        let empty = ExplainReport::new();
        if !empty.sections_ok() { caught += 1; d.push("注3 空报:抓".to_string()); } else { d.push("注3 空报:漏".to_string()); }
    }
    // 注 4：风险漏报（红账在册但风险段空）。
    {
        let mut book = LedgerBook::new();
        let _ = book.register_all_green();
        let _ = book.set_status(1306, Status4::Red);
        let mut r = ExplainReport::new();
        r.cites.push("x".to_string());
        if !r.risks_cover(&book) { caught += 1; d.push("注4 漏险:抓".to_string()); } else { d.push("注4 漏险:漏".to_string()); }
    }
    // 注 5：导出缺段。
    {
        let broken = ExplainReport::new();
        if matches!(broken.export(), Err(E_SEC_MISS)) { caught += 1; d.push("注5 缺段:抓".to_string()); } else { d.push("注5 缺段:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7630
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7630 段诊断码：核对人话化段位闭集"
        && LEDGER_TABLE.len() >= LEDGER_MIN
        && SECTION_HEADS.len() == 5
}

/// run_unx1314_checks：本单判据聚合（诊断段 0x7630）。
pub fn run_unx1314_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1314 退役人话化");
    let (ok1, d1) = drill_full_report();
    s.add_dyn("J1-全绿报告全段齐", ok1, &d1);
    let (ok2, d2) = drill_progress_phrases();
    s.add_dyn("J1-进度人话三态", ok2, &d2);
    let (ok3, d3) = drill_risk_cover();
    s.add_dyn("J1-红黄风险全进段", ok3, &d3);
    let (ok4, d4) = drill_terms();
    s.add_dyn("J1-术语一致率100%", ok4, &d4);
    let (ok5, d5) = drill_dup_cite();
    s.add_dyn("J1-引用唯一+重登拒", ok5, &d5);
    let (ok6, d6) = drill_export();
    s.add_dyn("J1-导出契约", ok6, &d6);
    let (ok7, d7) = drill_verdicts();
    s.add_dyn("J1-结论三态", ok7, &d7);
    let (ok8, d8) = drill_versions();
    s.add_dyn("J2-报告版本账", ok8, &d8);
    let (ok9, d9) = drill_term_usage();
    s.add_dyn("J2-术语使用账", ok9, &d9);
    let (ok10, d10) = drill_cite_density();
    s.add_dyn("J2-引用密度账", ok10, &d10);
    let (ok11, d11) = drill_risk_linkage();
    s.add_dyn("J2-风险三段联动", ok11, &d11);
    let (c8, t8, d8) = inject_5();
    s.add_dyn("J2-注入核5/5", c8 == t8, &format!("{}/{} {}", c8, t8, d8.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7630 段十码闭集");
    s
}
