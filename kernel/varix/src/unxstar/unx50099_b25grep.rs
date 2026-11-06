//! UNX-F50099 B25 批防重 grep 执行记录（关键词族：font/embed/subset/download）（0x43880 段）
//!
//! 锚点：UNX-F50099-J1 批收口防重留痕；判据锚定 §6.1。
//!
//! 数据段：`SEG_BASE = 0x43880`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 13`，
//! 逐项见 [`GrepError`]。
//!
//! B25 是字体批，收口前要证明「字体相关实现只落在一处、没有第二份平行
//! 拷贝」。做法不是人肉记笔记，而是把 grep 变成**可复核的执行记录**：
//!
//! ```text
//! 关键词族 --begin--> GrepRun(Running) --record_hit--> Hit 去重表
//!                                      --finish--> Done(scanned/matched)
//! 四族 Done --close--> GrepReceipt(digest, 只读)
//! ```
//!
//! 防重的三条硬规则：
//!
//! * **同族同位置**重复登记 → `DuplicateHit`（同一行不许报两次凑数）；
//! * **跨族同位置**登记 → `CrossFamily`（一行只能归一族，否则命中总数虚高，
//!   四族相加不再等于实际命中行数）；
//! * **摘要与登记顺序无关**：`digest()` 先按 (文件, 行) 排序再折叠，同一命中
//!   集无论扫描顺序如何都得到同一个摘要——这样「换个顺序重跑」骗不出新证据。
//!
//! 另外两条诚实约束：`finish` 不接受 `scanned == 0`（什么都没扫的 grep 不能
//! 算一次执行），`close` 只认四族全部 `Done`，缺一族就 `Incomplete`。
//!
//! 结构清单：
//! * [`KeywordFamily`]：四族关键词闭集（tag / slug / 模式表 / 优先级）；
//! * [`find_sub`] / [`line_matches`] / [`scan_text`]：大小写敏感的按行扫描器；
//! * [`GrepRun`] / [`RunState`]：单族执行状态机（Planned→Running→Done/Aborted）；
//! * [`GrepLedger`]：执行记录账（begin / record_hit / finish / abort / close）；
//! * [`GrepReceipt`]：收口回执（族数 / 命中数 / 扫描行数 / 顺序无关摘要）；
//! * [`injection_core`]：13 类坏样本注入核（逐类断言具体诊断码）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43880 独占 0x20）。
pub const SEG_BASE: u32 = 0x43880;

const E_EMPTY: u16 = 1;
const E_FAMILY: u16 = 2;
const E_STATE: u16 = 3;
const E_DUP_RUN: u16 = 4;
const E_FILE: u16 = 5;
const E_LINE: u16 = 6;
const E_DUP_HIT: u16 = 7;
const E_CROSS: u16 = 8;
const E_TALLY: u16 = 9;
const E_INCOMPLETE: u16 = 10;
const E_SEALED: u16 = 11;
const E_SCAN: u16 = 12;
const E_CAPACITY: u16 = 13;

/// 防重 grep 记录错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrepError {
    /// 关键词族计划为空。
    Empty,
    /// 族 tag 不在闭集内。
    Family,
    /// 该族不在 Running 状态（或已 Done），不允许当前操作。
    State,
    /// 同一族重复开跑。
    DuplicateRun,
    /// 文件号 0（0 是「未知文件」哨兵）。
    File,
    /// 行号 0（行号 1 起）。
    Line,
    /// 同族同位置重复命中。
    DuplicateHit,
    /// 跨族同位置命中（一行只能归一族）。
    CrossFamily,
    /// 扫描行数少于命中数，计数自相矛盾。
    Tally,
    /// 四族尚未全部收尾。
    Incomplete,
    /// 记录已封存，只读。
    Sealed,
    /// 扫描行数为 0（空扫描不算一次执行）。
    Scan,
    /// 命中表已满。
    Capacity,
}

impl GrepError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            GrepError::Empty => E_EMPTY,
            GrepError::Family => E_FAMILY,
            GrepError::State => E_STATE,
            GrepError::DuplicateRun => E_DUP_RUN,
            GrepError::File => E_FILE,
            GrepError::Line => E_LINE,
            GrepError::DuplicateHit => E_DUP_HIT,
            GrepError::CrossFamily => E_CROSS,
            GrepError::Tally => E_TALLY,
            GrepError::Incomplete => E_INCOMPLETE,
            GrepError::Sealed => E_SEALED,
            GrepError::Scan => E_SCAN,
            GrepError::Capacity => E_CAPACITY,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断（日志与串口共用）。
    pub fn message(self) -> &'static str {
        match self {
            GrepError::Empty => "keyword family plan empty",
            GrepError::Family => "keyword family tag outside closed set",
            GrepError::State => "run state rejects this call",
            GrepError::DuplicateRun => "family already has a run",
            GrepError::File => "file id zero",
            GrepError::Line => "line number zero",
            GrepError::DuplicateHit => "hit already recorded for this family",
            GrepError::CrossFamily => "line already attributed to another family",
            GrepError::Tally => "scanned lines fewer than matched lines",
            GrepError::Incomplete => "not all four families finished",
            GrepError::Sealed => "grep ledger sealed (read only)",
            GrepError::Scan => "scan touched zero lines",
            GrepError::Capacity => "hit table full",
        }
    }
}

/// 关键词族数。
pub const FAMILY_COUNT: usize = 4;
/// 命中表容量。B25 是单批收口，超过 256 条说明关键词过宽，宁可报满也不静默丢。
pub const MAX_HITS: usize = 256;

/// 四族关键词。模式**区分大小写**：`embed` 与 `/EMBED` 是两种东西，
/// 放宽大小写会把注释里的 `Embedded` 也算命中，防重结论就失效了。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeywordFamily {
    /// 字体面登记（font-face / FontName / family）。
    Font,
    /// 嵌入（FontFile / /EmbeddedFile / embed）。
    Embed,
    /// 子集化（subset / ABCDEF+ / CIDSet）。
    Subset,
    /// 下载（download / fontdl / font-download）。
    Download,
}

impl KeywordFamily {
    /// 线上 tag（1..=4）。
    pub fn tag(self) -> u8 {
        match self {
            KeywordFamily::Font => 1,
            KeywordFamily::Embed => 2,
            KeywordFamily::Subset => 3,
            KeywordFamily::Download => 4,
        }
    }

    /// tag 反解，闭集之外显式拒绝。
    pub fn from_tag(t: u8) -> Result<KeywordFamily, GrepError> {
        match t {
            1 => Ok(KeywordFamily::Font),
            2 => Ok(KeywordFamily::Embed),
            3 => Ok(KeywordFamily::Subset),
            4 => Ok(KeywordFamily::Download),
            _ => Err(GrepError::Family),
        }
    }

    /// 报告用 slug（与备注里的族名一致，便于人机同读）。
    pub fn slug(self) -> &'static str {
        match self {
            KeywordFamily::Font => "font",
            KeywordFamily::Embed => "embed",
            KeywordFamily::Subset => "subset",
            KeywordFamily::Download => "download",
        }
    }

    /// 族内模式表（至少两条：单一模式漏检率太高）。
    pub fn patterns(self) -> &'static [&'static str] {
        match self {
            KeywordFamily::Font => &["font-face", "FontName", "family="],
            KeywordFamily::Embed => &["FontFile", "/EmbeddedFile", "embed"],
            KeywordFamily::Subset => &["subset", "ABCDEF+", "CIDSet"],
            KeywordFamily::Download => &["download", "fontdl", "font-download"],
        }
    }

    /// 四族标准顺序（收口回执按此序输出，保证报告可 diff）。
    pub fn all() -> [KeywordFamily; FAMILY_COUNT] {
        [
            KeywordFamily::Font,
            KeywordFamily::Embed,
            KeywordFamily::Subset,
            KeywordFamily::Download,
        ]
    }
}

/// 朴素子串查找（Haystack 小、Needle 短，且 no_std 无 memmem）。
///
/// 空 Needle 返回 `false`：空模式会匹配一切，那不是「命中」而是计数污染。
pub fn find_sub(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        let mut j = 0usize;
        let mut hit = true;
        while j < needle.len() {
            let a = match hay.get(i + j) {
                Some(v) => *v,
                None => return false,
            };
            let b = match needle.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a != b {
                hit = false;
                break;
            }
            j += 1;
        }
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 单行是否命中某族（任一模模式命中即为命中，逐字节、大小写敏感）。
pub fn line_matches(family: KeywordFamily, line: &str) -> bool {
    let pats = family.patterns();
    let hay = line.as_bytes();
    let mut i = 0usize;
    while i < pats.len() {
        if let Some(p) = pats.get(i) {
            if find_sub(hay, p.as_bytes()) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 一次文本扫描的结果：扫了多少行、哪几行命中（1 起、升序、无重复）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanResult {
    /// 扫描行数（按 `\n` 切分，末尾空行不计）。
    pub scanned: u32,
    /// 命中行号。
    pub hits: Vec<u32>,
}

/// 扫描一段文本。同一行命中多个模式只报一次——这正是「防重」在扫描层的落点。
pub fn scan_text(family: KeywordFamily, text: &str) -> ScanResult {
    let bytes = text.as_bytes();
    let mut scanned = 0u32;
    let mut hits = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= bytes.len() {
        let at_end = i == bytes.len();
        let nl = match bytes.get(i) {
            Some(b) => *b == b'\n',
            None => false,
        };
        if at_end || nl {
            if i > start {
                let seg = match text.get(start..i) {
                    Some(v) => v,
                    None => "",
                };
                scanned += 1;
                if line_matches(family, seg) {
                    hits.push(scanned);
                }
            }
            start = i + 1;
        }
        i += 1;
    }
    ScanResult { scanned, hits }
}

/// 单族执行状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    /// 计划中：还没开跑。
    Planned,
    /// 执行中：只此状态接受命中登记。
    Running,
    /// 已收尾：counts 已冻结。
    Done,
    /// 中止：本次不参与收口。
    Aborted,
}

/// 单族执行记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrepRun {
    /// 族。
    pub family: KeywordFamily,
    /// 状态。
    pub state: RunState,
    /// 已扫描行数（`finish` 时落定）。
    pub scanned: u32,
    /// 已登记命中数。
    pub matched: u32,
}

/// 一条命中：文件号 + 行号 + 归属族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 文件号（1 起）。
    pub file: u16,
    /// 行号（1 起）。
    pub line: u32,
    /// 归属族（一行只归一族）。
    pub family: KeywordFamily,
}

/// 收口回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrepReceipt {
    /// 收口族数（应为 4）。
    pub families: u8,
    /// 命中总数。
    pub hits: u32,
    /// 扫描总行数。
    pub lines: u32,
    /// 顺序无关摘要。
    pub digest: u32,
}

/// 防重 grep 执行记录账。
#[derive(Clone, Debug)]
pub struct GrepLedger {
    runs: Vec<GrepRun>,
    hits: Vec<Hit>,
    sealed: bool,
    receipt: Option<GrepReceipt>,
}

fn family_index(runs: &[GrepRun], family: KeywordFamily) -> Option<usize> {
    let mut i = 0usize;
    while i < runs.len() {
        if let Some(r) = runs.get(i) {
            if r.family == family {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

impl GrepLedger {
    /// 空账。
    pub fn new() -> GrepLedger {
        GrepLedger {
            runs: Vec::with_capacity(FAMILY_COUNT),
            hits: Vec::with_capacity(16),
            sealed: false,
            receipt: None,
        }
    }

    /// 按族计划建账并逐族开跑。空计划显式拒绝，四族顺序不受入参顺序影响。
    pub fn from_plan(families: &[KeywordFamily]) -> Result<GrepLedger, GrepError> {
        if families.is_empty() {
            return Err(GrepError::Empty);
        }
        let mut led = GrepLedger::new();
        let mut i = 0usize;
        while i < families.len() {
            if let Some(f) = families.get(i) {
                led.begin(*f)?;
            }
            i += 1;
        }
        Ok(led)
    }

    /// 四族标准计划（收口流程默认值）。
    pub fn from_standard_plan() -> Result<GrepLedger, GrepError> {
        GrepLedger::from_plan(&KeywordFamily::all())
    }

    /// 开跑一族。同族二次开跑报 `DuplicateRun`，不让「重跑一遍」把命中翻倍。
    pub fn begin(&mut self, family: KeywordFamily) -> Result<(), GrepError> {
        if self.sealed {
            return Err(GrepError::Sealed);
        }
        if family_index(&self.runs, family).is_some() {
            return Err(GrepError::DuplicateRun);
        }
        self.runs.push(GrepRun {
            family,
            state: RunState::Running,
            scanned: 0,
            matched: 0,
        });
        Ok(())
    }

    /// 登记一条命中。去重规则见模块头（同族重复 / 跨族同位置都拒）。
    pub fn record_hit(&mut self, family: KeywordFamily, file: u16, line: u32) -> Result<(), GrepError> {
        if self.sealed {
            return Err(GrepError::Sealed);
        }
        if file == 0 {
            return Err(GrepError::File);
        }
        if line == 0 {
            return Err(GrepError::Line);
        }
        let idx = match family_index(&self.runs, family) {
            Some(v) => v,
            None => return Err(GrepError::State),
        };
        match self.runs.get(idx) {
            Some(r) => {
                if r.state != RunState::Running {
                    return Err(GrepError::State);
                }
            }
            None => return Err(GrepError::State),
        }
        if self.hits.len() >= MAX_HITS {
            return Err(GrepError::Capacity);
        }
        let mut i = 0usize;
        while i < self.hits.len() {
            if let Some(h) = self.hits.get(i) {
                if h.file == file && h.line == line {
                    return if h.family == family {
                        Err(GrepError::DuplicateHit)
                    } else {
                        Err(GrepError::CrossFamily)
                    };
                }
            }
            i += 1;
        }
        self.hits.push(Hit { file, line, family });
        if let Some(r) = self.runs.get_mut(idx) {
            r.matched += 1;
        }
        Ok(())
    }

    /// 收尾一族：`scanned` 必须 ≥ 本族命中数且 > 0（空扫描不算执行）。
    pub fn finish(&mut self, family: KeywordFamily, scanned: u32) -> Result<u32, GrepError> {
        if self.sealed {
            return Err(GrepError::Sealed);
        }
        let idx = match family_index(&self.runs, family) {
            Some(v) => v,
            None => return Err(GrepError::State),
        };
        let matched = match self.runs.get(idx) {
            Some(r) => {
                if r.state != RunState::Running {
                    return Err(GrepError::State);
                }
                if scanned == 0 {
                    return Err(GrepError::Scan);
                }
                if scanned < r.matched {
                    return Err(GrepError::Tally);
                }
                r.matched
            }
            None => return Err(GrepError::State),
        };
        if let Some(r) = self.runs.get_mut(idx) {
            r.scanned = scanned;
            r.state = RunState::Done;
        }
        Ok(matched)
    }

    /// 中止一族（设备掉线 / 关键词改口径时用）。中止的族不参与收口。
    pub fn abort(&mut self, family: KeywordFamily) -> Result<(), GrepError> {
        if self.sealed {
            return Err(GrepError::Sealed);
        }
        let idx = match family_index(&self.runs, family) {
            Some(v) => v,
            None => return Err(GrepError::State),
        };
        match self.runs.get(idx) {
            Some(r) => {
                if r.state != RunState::Running {
                    return Err(GrepError::State);
                }
            }
            None => return Err(GrepError::State),
        }
        if let Some(r) = self.runs.get_mut(idx) {
            r.state = RunState::Aborted;
        }
        Ok(())
    }

    /// 收口：四族全部 Done 才发回执，随后整本只读。
    pub fn close(&mut self) -> Result<GrepReceipt, GrepError> {
        if self.sealed {
            return Err(GrepError::Sealed);
        }
        let mut done = 0u8;
        let mut lines = 0u32;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                if r.state == RunState::Done {
                    done += 1;
                    lines += r.scanned;
                }
            }
            i += 1;
        }
        if done as usize != FAMILY_COUNT {
            return Err(GrepError::Incomplete);
        }
        let receipt = GrepReceipt {
            families: done,
            hits: self.hits.len() as u32,
            lines,
            digest: self.digest(),
        };
        self.receipt = Some(receipt);
        self.sealed = true;
        Ok(receipt)
    }

    /// 顺序无关摘要：先按 (文件, 行) 排序再折叠。
    ///
    /// 排序副本用插入排序（≤256 条、单批一次），避免在 no_std 里为了一个
    /// 摘要引入通用排序；命中行号在同一文件内基本已升序，插入排序接近线性。
    pub fn digest(&self) -> u32 {
        let mut order: Vec<Hit> = Vec::with_capacity(self.hits.len());
        let mut i = 0usize;
        while i < self.hits.len() {
            if let Some(h) = self.hits.get(i) {
                order.push(*h);
            }
            i += 1;
        }
        let mut a = 1usize;
        while a < order.len() {
            let mut b = a;
            while b > 0 {
                let prev = match order.get(b - 1) {
                    Some(v) => *v,
                    None => break,
                };
                let cur = match order.get(b) {
                    Some(v) => *v,
                    None => break,
                };
                // 键序：文件号 → 行号 → 族 tag（族只在同位置时参与，正常不触发）。
                let key_prev = (prev.file as u32, prev.line, prev.family.tag());
                let key_cur = (cur.file as u32, cur.line, cur.family.tag());
                if key_prev <= key_cur {
                    break;
                }
                if let Some(slot) = order.get_mut(b - 1) {
                    *slot = cur;
                }
                if let Some(slot) = order.get_mut(b) {
                    *slot = prev;
                }
                b -= 1;
            }
            a += 1;
        }
        let mut h: u32 = 0x811c_9dc5;
        let mut k = 0usize;
        while k < order.len() {
            if let Some(x) = order.get(k) {
                let bytes = [
                    (x.file >> 8) as u8,
                    x.file as u8,
                    (x.line >> 24) as u8,
                    (x.line >> 16) as u8,
                    (x.line >> 8) as u8,
                    x.line as u8,
                    x.family.tag(),
                ];
                let mut j = 0usize;
                while j < bytes.len() {
                    if let Some(bt) = bytes.get(j) {
                        h = (h ^ *bt as u32).wrapping_mul(0x0100_0193);
                    }
                    j += 1;
                }
            }
            k += 1;
        }
        h
    }

    /// 族当前状态。
    pub fn run_state(&self, family: KeywordFamily) -> Option<RunState> {
        match family_index(&self.runs, family) {
            Some(i) => self.runs.get(i).map(|r| r.state),
            None => None,
        }
    }

    /// 族命中数。
    pub fn matched(&self, family: KeywordFamily) -> Option<u32> {
        match family_index(&self.runs, family) {
            Some(i) => self.runs.get(i).map(|r| r.matched),
            None => None,
        }
    }

    /// 账本上的族数。
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// 命中总数。
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// 是否已封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 回执。
    pub fn receipt(&self) -> Option<GrepReceipt> {
        self.receipt
    }

    /// 一行报告（结果摘要非空，供软件写回）。
    pub fn report(&self) -> String {
        let mut hits = 0u32;
        let mut lines = 0u32;
        let mut done = 0u8;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                hits += r.matched;
                lines += r.scanned;
                if r.state == RunState::Done {
                    done += 1;
                }
            }
            i += 1;
        }
        format!(
            "B25 grep runs={} done={} hits={} lines={} sealed={} digest={:08x}",
            self.runs.len(),
            done,
            hits,
            lines,
            self.sealed as u8,
            self.digest()
        )
    }
}

impl Default for GrepLedger {
    fn default() -> GrepLedger {
        GrepLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 样例语料与注入核
// ---------------------------------------------------------------------------

/// 样例语料：三份「源码」按行给出，覆盖四族关键词与若干干扰行（`Embedded`
/// 大写变体、注释里的 `font-face` 拼写错误）。
pub const SAMPLE_A: &str = "use crate::face::Kind;\n@font-face { family=Latin }\n// Embedded is not a hit\nFontFile2 present\n";
/// 样例语料 B：子集 + 下载。
pub const SAMPLE_B: &str = "subset tags rebuilt\nglyph ABCDEF+LATIN\nfontdl channel ready\n";
/// 样例语料 C：仅干扰行（故意一条都不命中）。
pub const SAMPLE_C: &str = "plain text\nno keywords here\n";

/// 坏样本注入核：13 类错误必须逐类命中具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 13usize;

    // 1 空计划
    if GrepLedger::from_plan(&[]).err() == Some(GrepError::Empty) {
        caught += 1;
    }
    // 2 族 tag 越界
    if KeywordFamily::from_tag(9).err() == Some(GrepError::Family) {
        caught += 1;
    }
    // 3 未开跑就登记命中
    let mut led = GrepLedger::new();
    if led.record_hit(KeywordFamily::Font, 1, 1).err() == Some(GrepError::State) {
        caught += 1;
    }
    // 4 同族重复开跑
    let mut led = GrepLedger::new();
    let _ = led.begin(KeywordFamily::Font);
    if led.begin(KeywordFamily::Font).err() == Some(GrepError::DuplicateRun) {
        caught += 1;
    }
    // 5 文件号为 0
    let mut led = GrepLedger::new();
    let _ = led.begin(KeywordFamily::Font);
    if led.record_hit(KeywordFamily::Font, 0, 7).err() == Some(GrepError::File) {
        caught += 1;
    }
    // 6 行号为 0
    let mut led = GrepLedger::new();
    let _ = led.begin(KeywordFamily::Font);
    if led.record_hit(KeywordFamily::Font, 1, 0).err() == Some(GrepError::Line) {
        caught += 1;
    }
    // 7 同族同位置重复
    let mut led = GrepLedger::new();
    let _ = led.begin(KeywordFamily::Font);
    let _ = led.record_hit(KeywordFamily::Font, 1, 3);
    if led.record_hit(KeywordFamily::Font, 1, 3).err() == Some(GrepError::DuplicateHit) {
        caught += 1;
    }
    // 8 跨族同位置
    let mut led = GrepLedger::new();
    let _ = led.begin(KeywordFamily::Font);
    let _ = led.begin(KeywordFamily::Embed);
    let _ = led.record_hit(KeywordFamily::Font, 1, 3);
    if led.record_hit(KeywordFamily::Embed, 1, 3).err() == Some(GrepError::CrossFamily) {
        caught += 1;
    }
    // 9 扫描行数少于命中数
    let mut led = GrepLedger::new();
    let _ = led.begin(KeywordFamily::Font);
    let _ = led.record_hit(KeywordFamily::Font, 1, 3);
    let _ = led.record_hit(KeywordFamily::Font, 1, 4);
    if led.finish(KeywordFamily::Font, 1).err() == Some(GrepError::Tally) {
        caught += 1;
    }
    // 10 未跑满四族就收口
    let mut led = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };
    let _ = led.finish(KeywordFamily::Font, 10);
    let _ = led.finish(KeywordFamily::Embed, 10);
    if led.close().err() == Some(GrepError::Incomplete) {
        caught += 1;
    }
    // 11 封存后开跑
    let mut led = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };
    let mut f = 0usize;
    while f < FAMILY_COUNT {
        if let Some(fam) = KeywordFamily::all().get(f).copied() {
            let _ = led.finish(fam, 4);
        }
        f += 1;
    }
    let _ = led.close();
    if led.begin(KeywordFamily::Font).err() == Some(GrepError::Sealed) {
        caught += 1;
    }
    // 12 空扫描收尾
    let mut led = GrepLedger::new();
    let _ = led.begin(KeywordFamily::Subset);
    if led.finish(KeywordFamily::Subset, 0).err() == Some(GrepError::Scan) {
        caught += 1;
    }
    // 13 命中表满
    let mut led = GrepLedger::new();
    let _ = led.begin(KeywordFamily::Download);
    let mut n = 0u32;
    while (n as usize) < MAX_HITS {
        let _ = led.record_hit(KeywordFamily::Download, 1, n + 1);
        n += 1;
    }
    if led.record_hit(KeywordFamily::Download, 1, n + 1).err() == Some(GrepError::Capacity) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑 UNX-F50099 的 14 条判据（含 J1 批收口防重留痕与注入核）。
pub fn run_unx50099_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50099");

    // J1-1 族闭集自洽：4 族、tag 1..=4 双射、slug 唯一、每族 ≥2 个模式。
    let all = KeywordFamily::all();
    let mut ok = all.len() == FAMILY_COUNT;
    let mut i = 0usize;
    while i < all.len() {
        if let Some(f) = all.get(i) {
            if f.tag() as usize != i + 1 {
                ok = false;
            }
            if f.patterns().len() < 2 {
                ok = false;
            }
            if KeywordFamily::from_tag(f.tag()) != Ok(*f) {
                ok = false;
            }
            let mut j = i + 1;
            while j < all.len() {
                if let Some(g) = all.get(j) {
                    if g.slug() == f.slug() {
                        ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("50099-J1-族闭集", ok, if ok { "" } else { "族表不自洽" });

    // J1-2 扫描器口径：区分大小写，正例命中、干扰行不命中。
    let a = scan_text(KeywordFamily::Font, SAMPLE_A);
    let b = scan_text(KeywordFamily::Embed, SAMPLE_A);
    let c = scan_text(KeywordFamily::Subset, SAMPLE_C);
    let ok = a.scanned == 4
        && a.hits.first() == Some(&2)
        && b.hits.first() == Some(&4)
        && !b.hits.contains(&3)
        && c.scanned == 2
        && c.hits.is_empty();
    let det = format!("font={:?} embed={:?}", a.hits, b.hits);
    s.add_dyn("50099-J1-扫描口径", ok, &det);

    // J1-3 扫描行号 1 起、升序、同一行多模式只报一次。
    let mixed = "font-face family=FontName\nsubset ABCDEF+ CIDSet\n";
    let m = scan_text(KeywordFamily::Font, mixed);
    let sub = scan_text(KeywordFamily::Subset, mixed);
    let ok = m.scanned == 2
        && m.hits.len() == 1
        && m.hits.first() == Some(&1)
        && sub.hits.len() == 1
        && sub.hits.first() == Some(&2);
    let det = format!("font={:?} subset={:?}", m.hits, sub.hits);
    s.add_dyn("50099-J1-行号与去重", ok, &det);

    // 建账：四族全部开跑。
    let mut led = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };

    // J1-4 状态机：开跑后 Running；登记被接受；收尾后拒绝登记与二次收尾。
    let running = led.run_state(KeywordFamily::Font) == Some(RunState::Running);
    let ok_rec = led.record_hit(KeywordFamily::Font, 2, 7).is_ok();
    let fin = led.finish(KeywordFamily::Font, 40);
    let after_rec = led.record_hit(KeywordFamily::Font, 2, 8);
    let after_fin = led.finish(KeywordFamily::Font, 41);
    let ok = running
        && ok_rec
        && fin == Ok(1)
        && after_rec.err() == Some(GrepError::State)
        && after_fin.err() == Some(GrepError::State)
        && led.run_state(KeywordFamily::Font) == Some(RunState::Done);
    let det = format!("fin={:?} after={:?}", fin, after_rec.err().map(|e| e.code()));
    s.add_dyn("50099-J1-状态机", ok, &det);

    // J1-5 同族重复命中拒（同一行不许报两次）。
    let mut d = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };
    let _ = d.record_hit(KeywordFamily::Embed, 3, 11);
    let dup = d.record_hit(KeywordFamily::Embed, 3, 11);
    let ok = dup.err() == Some(GrepError::DuplicateHit)
        && d.matched(KeywordFamily::Embed) == Some(1)
        && d.hit_count() == 1;
    s.add("50099-J1-同位置防重", ok, if ok { "" } else { "同族重复命中被接受" });

    // J1-6 跨族同位置拒：一行只能归一族，否则四族相加会虚高。
    let cross = d.record_hit(KeywordFamily::Subset, 3, 11);
    let ok = cross.err() == Some(GrepError::CrossFamily) && d.matched(KeywordFamily::Subset) == Some(0);
    s.add("50099-J1-跨族防重", ok, if ok { "" } else { "跨族重复被接受" });

    // J1-7 计数守恒：matched 之和 == 命中表条数；finish 的 scanned ≥ matched。
    let mut rec = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };
    // 用真实语料扫描后逐条登记：文件 1 用 A，文件 2 用 B。
    let fa = scan_text(KeywordFamily::Font, SAMPLE_A);
    let ea = scan_text(KeywordFamily::Embed, SAMPLE_A);
    let sb = scan_text(KeywordFamily::Subset, SAMPLE_B);
    let db = scan_text(KeywordFamily::Download, SAMPLE_B);
    let mut k = 0usize;
    while k < fa.hits.len() {
        if let Some(l) = fa.hits.get(k) {
            let _ = rec.record_hit(KeywordFamily::Font, 1, *l);
        }
        k += 1;
    }
    let mut k = 0usize;
    while k < ea.hits.len() {
        if let Some(l) = ea.hits.get(k) {
            let _ = rec.record_hit(KeywordFamily::Embed, 1, *l);
        }
        k += 1;
    }
    let mut k = 0usize;
    while k < sb.hits.len() {
        if let Some(l) = sb.hits.get(k) {
            let _ = rec.record_hit(KeywordFamily::Subset, 2, *l);
        }
        k += 1;
    }
    let mut k = 0usize;
    while k < db.hits.len() {
        if let Some(l) = db.hits.get(k) {
            let _ = rec.record_hit(KeywordFamily::Download, 2, *l);
        }
        k += 1;
    }
    let mut sum = 0u32;
    let mut q = 0usize;
    let fams = KeywordFamily::all();
    while q < fams.len() {
        if let Some(f) = fams.get(q) {
            if let Some(n) = rec.matched(*f) {
                sum += n;
            }
        }
        q += 1;
    }
    let f1 = rec.finish(KeywordFamily::Font, fa.scanned);
    let f2 = rec.finish(KeywordFamily::Embed, ea.scanned);
    let f3 = rec.finish(KeywordFamily::Subset, sb.scanned);
    let f4 = rec.finish(KeywordFamily::Download, db.scanned);
    let ok = sum as usize == rec.hit_count()
        && rec.hit_count() == 5
        && f1 == Ok(1)
        && f2 == Ok(1)
        && f3 == Ok(2)
        && f4 == Ok(1);
    let det = format!("sum={} hits={} f={:?}/{:?}/{:?}/{:?}", sum, rec.hit_count(), f1, f2, f3, f4);
    s.add_dyn("50099-J1-计数守恒", ok, &det);

    // J1-8 收口防重留痕：四族跑满 → 回执 4 族、5 命中、摘要非零且整本只读。
    let receipt = rec.close();
    let ok = match &receipt {
        Ok(r) => {
            r.families == 4
                && r.hits == 5
                && r.lines == fa.scanned + ea.scanned + sb.scanned + db.scanned
                && r.digest != 0
                && rec.is_sealed()
                && rec.receipt().is_some()
                && rec.report().contains("B25 grep")
        }
        Err(_) => false,
    };
    let det = match &receipt {
        Ok(r) => format!("fam={} hits={} dg={:08x}", r.families, r.hits, r.digest),
        Err(e) => format!("err={}", e.code()),
    };
    s.add_dyn("50099-J1-收口留痕", ok, &det);

    // J1-9 收口后只读：begin / record / close / abort 全部 Sealed。
    let b = rec.begin(KeywordFamily::Font);
    let h = rec.record_hit(KeywordFamily::Font, 9, 9);
    let c = rec.close();
    let ab = rec.abort(KeywordFamily::Font);
    let ok = b.err() == Some(GrepError::Sealed)
        && h.err() == Some(GrepError::Sealed)
        && c.err() == Some(GrepError::Sealed)
        && ab.err() == Some(GrepError::Sealed);
    s.add("50099-J1-收口只读", ok, if ok { "" } else { "封存后仍可写" });

    // J1-10 摘要与登记顺序无关：倒序登记同一命中集，摘要逐位相同。
    let mut forward = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };
    let mut backward = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };
    let pts = [(2u16, 30u32), (1, 9), (3, 4), (2, 7)];
    let mut i = 0usize;
    while i < pts.len() {
        if let Some(p) = pts.get(i) {
            let _ = forward.record_hit(KeywordFamily::Subset, p.0, p.1);
        }
        i += 1;
    }
    let mut r = pts.len();
    while r > 0 {
        r -= 1;
        if let Some(p) = pts.get(r) {
            let _ = backward.record_hit(KeywordFamily::Subset, p.0, p.1);
        }
    }
    let ok = forward.digest() == backward.digest() && forward.digest() != 0;
    let det = format!("df={:08x} db={:08x}", forward.digest(), backward.digest());
    s.add_dyn("50099-J1-摘要顺序无关", ok, &det);

    // J1-11 未跑满四族不可收口：只 Done 两族 → Incomplete。
    let mut partial = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };
    let _ = partial.finish(KeywordFamily::Font, 5);
    let _ = partial.finish(KeywordFamily::Embed, 5);
    let ok = partial.close().err() == Some(GrepError::Incomplete);
    s.add("50099-J1-未跑满不收口", ok, if ok { "" } else { "缺族仍收口" });

    // J1-12 中止的族不算 Done：中止后仍不可收口，且不再接受命中。
    let mut aborted = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };
    let _ = aborted.finish(KeywordFamily::Font, 5);
    let _ = aborted.finish(KeywordFamily::Embed, 5);
    let _ = aborted.finish(KeywordFamily::Subset, 5);
    let ab = aborted.abort(KeywordFamily::Download);
    let after = aborted.record_hit(KeywordFamily::Download, 4, 2);
    let ok = ab.is_ok()
        && aborted.run_state(KeywordFamily::Download) == Some(RunState::Aborted)
        && after.err() == Some(GrepError::State)
        && aborted.close().err() == Some(GrepError::Incomplete);
    s.add("50099-J1-中止语义", ok, if ok { "" } else { "中止态语义错" });

    // J1-13 空扫描不算执行：scanned=0 在 finish 就被拒。
    let mut zero = match GrepLedger::from_standard_plan() {
        Ok(v) => v,
        Err(_) => GrepLedger::new(),
    };
    let ok = zero.finish(KeywordFamily::Font, 0).err() == Some(GrepError::Scan)
        && zero.run_state(KeywordFamily::Font) == Some(RunState::Running);
    s.add("50099-J1-空扫描必拒", ok, if ok { "" } else { "空扫描被当成执行" });

    // 注入核：13 类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50099-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50099_checks_green() {
        let set = run_unx50099_checks();
        assert!(set.all_passed(), "UNX-F50099 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50099_digest_ignores_insertion_order() {
        let mut a = match GrepLedger::from_standard_plan() {
            Ok(v) => v,
            Err(_) => GrepLedger::new(),
        };
        let mut b = match GrepLedger::from_standard_plan() {
            Ok(v) => v,
            Err(_) => GrepLedger::new(),
        };
        let _ = a.record_hit(KeywordFamily::Font, 1, 5);
        let _ = a.record_hit(KeywordFamily::Font, 1, 9);
        let _ = b.record_hit(KeywordFamily::Font, 1, 9);
        let _ = b.record_hit(KeywordFamily::Font, 1, 5);
        assert_eq!(a.digest(), b.digest());
    }
}
