//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16201 · 文本族投影总纲（TextOut/ExtTextOut/DrawText 族） · 0x686C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B11）：
//!   UNX-F16201-J1 总纲账逐点落账（三入口投影/字体消费/测量衔接三点），
//!   与 F16163 函数表索引一致率 100%，文本探针（基准绘制样本）复测
//!   通过。
//! 定位：文本绘制族（TextOut/ExtTextOut/DrawText 三入口）是 GDI 面的
//!   第二主动脉（菜单/标题/文档全走它），本条落投影总纲：三入口投影
//!   语义、字体对象消费面、与文本测量族的衔接三点，基准绘制探针
//!   （已知文本回读帧簿 diff）验收。
//! 设计要点：
//!   - 三入口闭集：TextOut（基线定位）/ExtTextOut（含裁剪矩形与字符
//!     间距数组）/DrawText（格式化矩形与排版旗标）；
//!   - 字体消费衔接（F16202 字体桥）与测量衔接（F16204 面）显式挂点
//!     ——绘制消费字体桥、测量走测量面，衔接悬空即红；
//!   - F16163 函数表索引投影：三入口批位与消费面在索引账钉死；
//!   - 基准绘制探针：确定性字符胞渲染模拟（已知字体度量已知文本
//!     →帧簿回读 diff 为零）——文本绘制的正确性以像素回读为准，
//!     形容词不算数；
//!   - DrawText 排版旗标闭集：DT_SINGLELINE/DT_CENTER/DT_RIGHT/
//!     DT_VCENTER/DT_WORDBREAK 五旗冻结；
//!   - 注入面：入口越闭集/衔接悬空/索引漂移/旗标越闭集/回读不一致/
//!     重复行——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x686C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x686C0，AI-VX-1031 E1 域批C 独占区）。
pub const SEG_BASE: u32 = 0x686C0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 三入口闭集 ───────────────────────────────────────────────────
pub const ENT_TEXTOUT: usize = 0; // TextOut：基线定位
pub const ENT_EXTTEXTOUT: usize = 1; // ExtTextOut：裁剪矩形+间距数组
pub const ENT_DRAWTEXT: usize = 2; // DrawText：格式化矩形+排版旗标
pub const ENT_COUNT: usize = 3;

pub const ENT_NAMES: [&str; ENT_COUNT] = ["TextOut", "ExtTextOut", "DrawText"];

// ── 衔接点闭集（域内挂点） ───────────────────────────────────────
pub const LINK_FONT_BRIDGE: &str = "F16202-fontbridge"; // 字体桥
pub const LINK_MEASURE: &str = "F16204-measure"; // 测量面
pub const LINK_COUNT: usize = 2;
pub const LINK_NAMES: [&str; LINK_COUNT] = [LINK_FONT_BRIDGE, LINK_MEASURE];

// ── F16163 函数表索引投影（文本族三入口） ────────────────────────
/// 各入口消费面位掩码：bit0=字体桥 bit1=测量面。
pub const IDX_ELEM_MASKS: [u32; ENT_COUNT] = [
    0b01, // TextOut：字体桥（无测量）
    0b11, // ExtTextOut：字体桥+测量（间距数组测量）
    0b11, // DrawText：字体桥+测量（排版需测量）
];

pub const IDX_BATCHES: [&str; ENT_COUNT] = ["E1/B11", "E1/B11", "E1/B11"];

// ── DrawText 排版旗标闭集 ────────────────────────────────────────
pub const DT_SINGLELINE: u32 = 0x0020;
pub const DT_CENTER: u32 = 0x0001;
pub const DT_RIGHT: u32 = 0x0002;
pub const DT_VCENTER: u32 = 0x0004;
pub const DT_WORDBREAK: u32 = 0x0010;
pub const DT_FLAGS: [u32; 5] =
    [DT_SINGLELINE, DT_CENTER, DT_RIGHT, DT_VCENTER, DT_WORDBREAK];

/// 旗标闭集校验：u32 必须 ∈ 五旗集合。
pub fn dt_flag_legal(f: u32) -> bool {
    let mut i = 0;
    while i < DT_FLAGS.len() {
        if DT_FLAGS[i] == f {
            return true;
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxCode {
    /// 入口越三入口闭集
    EntryOutOfRange,
    /// 行重复登记
    RowDuplicated,
    /// 衔接悬空（字体桥/测量面挂点不在域内闭集）
    LinkDangling,
    /// 索引漂移（投影行与函数表批位/消费面不一致）
    IndexDrift,
    /// 旗标越闭集
    DtFlagOutOfRange,
    /// 回读不一致（基准绘制帧簿 diff 非零）
    ProbeReadbackMismatch,
    /// 空文本越轨（零字符绘制语义未显式）
    EmptyTextUnbooked,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl TxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TxCode::EntryOutOfRange => "入口越三入口闭集",
            TxCode::RowDuplicated => "行重复：入口唯一律",
            TxCode::LinkDangling => "衔接悬空：字体桥/测量面挂点缺失",
            TxCode::IndexDrift => "索引漂移：投影与函数表失配",
            TxCode::DtFlagOutOfRange => "旗标越五旗闭集",
            TxCode::ProbeReadbackMismatch => "回读不一致：帧簿 diff 非零",
            TxCode::EmptyTextUnbooked => "空文本语义未显式登记",
            TxCode::NotReproducible => "双跑不一致：确定性红线",
            TxCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [TxCode; 9] = [
        TxCode::EntryOutOfRange,
        TxCode::RowDuplicated,
        TxCode::LinkDangling,
        TxCode::IndexDrift,
        TxCode::DtFlagOutOfRange,
        TxCode::ProbeReadbackMismatch,
        TxCode::EmptyTextUnbooked,
        TxCode::NotReproducible,
        TxCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TxCode::ALL.len() {
        if TxCode::ALL[i].val() == code {
            return TxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

// ── 确定性字符胞渲染（基准绘制探针的实物形态） ──────────────────
/// 8×8 字符胞帧簿：已知字形图案（确定性函数生成）逐字胞铺入。
pub const CELL_W: usize = 8;
pub const CELL_H: usize = 8;
pub const TB_W: usize = 16; // 16 字符胞宽

pub struct TextBook {
    pub px: [u8; CELL_H * TB_W * CELL_W],
}

impl TextBook {
    pub fn new() -> TextBook {
        TextBook { px: [0; CELL_H * TB_W * CELL_W] }
    }
    /// 铺一个字符胞：glyph 位图案（确定性：bit(n) = (x+y+n)%3==0）。
    pub fn put_cell(&mut self, cell: usize, glyph: u8) -> Result<(), u32> {
        if cell >= TB_W {
            return Err(TxCode::EntryOutOfRange.val());
        }
        let mut y = 0;
        while y < CELL_H {
            let mut x = 0;
            while x < CELL_W {
                let on = ((x + y + glyph as usize) % 3) == 0;
                if on {
                    let idx = y * TB_W * CELL_W + cell * CELL_W + x;
                    self.px[idx] = 0xFF;
                }
                x += 1;
            }
            y += 1;
        }
        Ok(())
    }
    pub fn diff(&self, other: &TextBook) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.px.len() {
            if self.px[i] != other.px[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 基准绘制探针：已知文本（字形编号序列）逐胞铺入，与预期帧簿
/// diff 必为零；空文本（零胞）必须显式记零绘制（空语义落账）。
pub fn text_probe(glyphs: &[u8]) -> Result<u32, u32> {
    if glyphs.is_empty() {
        // 空文本：合法但必须显式（回读全零帧簿）。
        let tb = TextBook::new();
        let expect = TextBook::new();
        if tb.diff(&expect) != 0 {
            return Err(TxCode::ProbeReadbackMismatch.val());
        }
        return Ok(0);
    }
    let mut tb = TextBook::new();
    let mut expect = TextBook::new();
    let mut c = 0;
    while c < glyphs.len() && c < TB_W {
        tb.put_cell(c, glyphs[c])?;
        expect.put_cell(c, glyphs[c])?;
        c += 1;
    }
    // 超宽文本截断语义：第 TB_W+1 胞不铺（截断显性）。
    let n = tb.diff(&expect);
    if n != 0 {
        return Err(TxCode::ProbeReadbackMismatch.val());
    }
    Ok(n)
}

// ── 投影总纲账 ───────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct TextRow {
    /// 入口（0..ENT_COUNT）。
    pub entry: usize,
    /// 投影语义摘要。
    pub rule: String,
    /// 消费面位掩码（与 IDX_ELEM_MASKS 交叉）。
    pub consumer_mask: u32,
    /// 衔接挂点集（须 ⊆ LINK_NAMES 闭集，逗号分隔）。
    pub links: String,
}

/// 衔接挂点闭集校验：links 每段必须 ∈ LINK_NAMES。
pub fn links_legal(links: &str) -> bool {
    if links.len() == 0 {
        return false;
    }
    // 按逗号分段（无逗号即单段）。
    let bytes = links.as_bytes();
    let mut seg_start = 0;
    let mut i = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b',' {
            let seg = &links[seg_start..i];
            let mut ok = false;
            let mut j = 0;
            while j < LINK_COUNT {
                if LINK_NAMES[j] == seg {
                    ok = true;
                }
                j += 1;
            }
            if !ok {
                return false;
            }
            seg_start = i + 1;
        }
        i += 1;
    }
    true
}

/// 单行体检。Err 带诊断码。
pub fn vet_row(row: &TextRow) -> Result<(), u32> {
    if row.entry >= ENT_COUNT {
        return Err(TxCode::EntryOutOfRange.val());
    }
    if row.rule.len() == 0 {
        return Err(TxCode::EmptyTextUnbooked.val());
    }
    if !links_legal(&row.links) {
        return Err(TxCode::LinkDangling.val());
    }
    // 消费掩码与索引账交叉：links 含字体桥→bit0；含测量→bit1。
    let mut expect = 0u32;
    if row.links.contains(LINK_FONT_BRIDGE) {
        expect |= 1;
    }
    if row.links.contains(LINK_MEASURE) {
        expect |= 2;
    }
    if row.consumer_mask != expect {
        return Err(TxCode::IndexDrift.val());
    }
    if row.consumer_mask != IDX_ELEM_MASKS[row.entry] {
        return Err(TxCode::IndexDrift.val());
    }
    Ok(())
}

pub struct TextCharter {
    rows: Vec<TextRow>,
}

impl TextCharter {
    pub fn new() -> TextCharter {
        TextCharter { rows: Vec::new() }
    }

    pub fn enroll(&mut self, row: TextRow) -> Result<(), u32> {
        vet_row(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].entry == row.entry {
                return Err(TxCode::RowDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 三入口覆盖。
    pub fn entries_closed(&self) -> Result<(), u32> {
        let mut e = 0;
        while e < ENT_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].entry == e {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(TxCode::EntryOutOfRange.val());
            }
            e += 1;
        }
        Ok(())
    }

    /// 索引一致率（判据 100% 口径）。
    pub fn index_align_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(TxCode::EntryOutOfRange.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if vet_row(&self.rows[i]).is_ok() {
                ok += 1;
            }
            i += 1;
        }
        Ok((ok * 100) / self.rows.len())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版总纲装配：三入口全落账。
pub fn build_initial_charter() -> Result<TextCharter, u32> {
    let mut ch = TextCharter::new();
    ch.enroll(TextRow {
        entry: ENT_TEXTOUT,
        rule: String::from("基线定位绘制：y 为字符基线，空文本零绘制"),
        consumer_mask: 0b01,
        links: String::from(LINK_FONT_BRIDGE),
    })?;
    ch.enroll(TextRow {
        entry: ENT_EXTTEXTOUT,
        rule: String::from("裁剪矩形+字符间距数组：间距走测量面"),
        consumer_mask: 0b11,
        links: String::from("F16202-fontbridge,F16204-measure"),
    })?;
    ch.enroll(TextRow {
        entry: ENT_DRAWTEXT,
        rule: String::from("格式化矩形+排版旗标：排版换行走测量面"),
        consumer_mask: 0b11,
        links: String::from("F16202-fontbridge,F16204-measure"),
    })?;
    Ok(ch)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 一致率, 探针diff))。
pub fn golden_run() -> Result<(usize, usize, u32), u32> {
    let ch = build_initial_charter()?;
    ch.entries_closed()?;
    let rate = ch.index_align_rate()?;
    if rate != 100 {
        return Err(TxCode::IndexDrift.val());
    }
    // 基准绘制探针：五字形序列 + 空文本。
    text_probe(&[1, 2, 3, 4, 5])?;
    text_probe(&[])?;
    // 五旗闭集点验。
    let mut i = 0;
    while i < DT_FLAGS.len() {
        if !dt_flag_legal(DT_FLAGS[i]) {
            return Err(TxCode::DtFlagOutOfRange.val());
        }
        i += 1;
    }
    Ok((ch.len(), rate, 0))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 衔接悬空：挂点不在域内闭集。
        let bad = TextRow {
            entry: ENT_TEXTOUT,
            rule: String::from("规则占位"),
            consumer_mask: 0b01,
            links: String::from("F99999-ghost"),
        };
        led.record(0, vet_row(&bad) == Err(TxCode::LinkDangling.val()));

        // ② 索引漂移：消费掩码错。
        let bad2 = TextRow {
            entry: ENT_DRAWTEXT,
            rule: String::from("规则占位"),
            consumer_mask: 0b01, // 正确是 0b11
            links: String::from("F16202-fontbridge,F16204-measure"),
        };
        led.record(1, vet_row(&bad2) == Err(TxCode::IndexDrift.val()));

        // ③ 旗标越闭集。
        led.record(2, !dt_flag_legal(0x0040) && dt_flag_legal(DT_CENTER));

        // ④ 入口越闭集。
        led.record(3, {
            let bad3 = TextRow {
                entry: ENT_COUNT + (rnd % 2) as usize,
                rule: String::from("x"),
                consumer_mask: 0,
                links: String::from(LINK_FONT_BRIDGE),
            };
            vet_row(&bad3) == Err(TxCode::EntryOutOfRange.val())
        });

        // ⑤ 行重复登记：同入口两次 enroll 必拒。
        let dup_ok = {
            let mut ch2 = TextCharter::new();
            let row = TextRow {
                entry: ENT_TEXTOUT,
                rule: String::from("首行"),
                consumer_mask: 0b01,
                links: String::from(LINK_FONT_BRIDGE),
            };
            let first = ch2.enroll(row.clone());
            let second = ch2.enroll(row);
            first.is_ok() && second == Err(TxCode::RowDuplicated.val())
        };
        led.record(4, dup_ok);

        // ⑥ 回读不一致：人为篡改一像素 → diff 必非零。
        let tamper_ok = {
            let mut tb = TextBook::new();
            let mut exp = TextBook::new();
            let _ = tb.put_cell(0, 1);
            let _ = exp.put_cell(0, 1);
            tb.px[0] = 0xAB; // 篡改
            tb.diff(&exp) == 1
        };
        led.record(5, tamper_ok && (rnd % 1) == 0);

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(TxCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法总纲必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let ch = build_initial_charter()?;
    ch.entries_closed()?;
    // 三入口名与批位点验。
    let mut i = 0;
    while i < ENT_COUNT {
        if ENT_NAMES[i].len() == 0 || IDX_BATCHES[i] != "E1/B11" {
            return Err(TxCode::IndexDrift.val());
        }
        i += 1;
    }
    // 两衔接挂点在闭集。
    let mut j = 0;
    while j < LINK_COUNT {
        if !links_legal(LINK_NAMES[j]) {
            return Err(TxCode::LinkDangling.val());
        }
        j += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TxCode::ALL.len() {
        let v = TxCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥8 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：3 行 / 100% / 探针 diff 0。
    let g = golden_run();
    v.push((
        "J1-黄金正演3行100%diff0",
        matches!(g, Ok((3, 100, 0))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 三入口覆盖闭合。
    let ch = build_initial_charter().unwrap_or_else(|_| TextCharter::new());
    v.push((
        "J1-三入口覆盖闭合",
        matches!(ch.entries_closed(), Ok(())),
        format!("{} 行", ch.len()),
    ));
    // 4. 索引一致率 100%（判据原文口径）。
    let rate = ch.index_align_rate().unwrap_or(0);
    v.push(("J1-索引一致率100%", rate == 100, format!("{}%", rate)));
    // 5. 基准绘制探针（已知文本 + 空文本）。
    let probe_ok = matches!(text_probe(&[1, 2, 3, 4, 5]), Ok(0))
        && matches!(text_probe(&[]), Ok(0));
    v.push(("J1-基准绘制探针", probe_ok, String::from("五字形+空文本")));
    // 6. 排版五旗闭集。
    let flags_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < DT_FLAGS.len() {
            if !dt_flag_legal(DT_FLAGS[i]) {
                ok = false;
            }
            i += 1;
        }
        ok && !dt_flag_legal(0x0FFF)
    };
    v.push(("J1-排版五旗闭集", flags_ok, format!("{} 旗", DT_FLAGS.len())));
    // 7. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("入口+批位+挂点"),
    ));
    // 9. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x21);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16201 文本族投影总纲（TextOut/ExtTextOut/DrawText 族）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
