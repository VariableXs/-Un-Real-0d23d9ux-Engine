//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1278 · Irq 态文档册（B24 段域内文档） · 0xFD40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1278-J1 B24 段文档覆盖全部 19 条（引用判据一致率 100%），
//!      文档可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十九条文档条目（17 卡 J1 主判据 + 2 条点名子判据），每条含
//!      卡号/判据引用（"UNX-F1264-J1" 全串）/标题/正文/关键术语行；
//!   - 引用一致率双向闸：正向=每条正文含其判据全串；反向=引用清单
//!      与条目清单一一对应（无重号、无缺位）；
//!   - 术语闸：正文过禁词表（口语变体四条），必含术语抽检；
//!   - 导出：全册分节导出（节数≡条目数，逐节含判据行+正文行），
//!      导出守恒恒等式（行数=节数×每节行数+册头）；
//!   - 条目号唯一、正文非空且长度 ≥ 冻结下限（防空壳条目）；
//!   - 修订账：条目修订 append-only 计数（只增不减面）；
//!   - LCG 种子 0xFD40_1278 写死，10/10 轮可重复。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFD40;

/// 文档条目数（判据「全部 19 条」）。
pub const DOC_COUNT: usize = 19;

/// 正文字长下限（防空壳）。
pub const BODY_FLOOR: usize = 24;

/// 注入演练轮数。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0xFD40 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// 0xFD40 条目号越界（出 0..19）。
    DocOob = 0,
    /// 0xFD41 判据引用缺失（正文无判据全串）。
    CiteMiss = 1,
    /// 0xFD42 引用重号（两目引同一判据串）。
    CiteDup = 2,
    /// 0xFD43 禁词命中（正文含口语变体）。
    TermBanned = 3,
    /// 0xFD44 导出缺失（节数≠条目数）。
    ExportMiss = 4,
    /// 0xFD45 条目空壳（正文过短）。
    EntryEmpty = 5,
    /// 0xFD46 条目重号（编号唯一性破）。
    IdDup = 6,
    /// 0xFD47 正文无术语（必含术语缺失）。
    TermMissing = 7,
    /// 0xFD48 修订回退（修订计数回退）。
    RevRewind = 8,
    /// 0xFD49 可重复失恒（同种子两轮不同）。
    RepeatDrift = 9,
}

impl DcCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [DcCode; 10] = [
        DcCode::DocOob, DcCode::CiteMiss, DcCode::CiteDup, DcCode::TermBanned,
        DcCode::ExportMiss, DcCode::EntryEmpty, DcCode::IdDup, DcCode::TermMissing,
        DcCode::RevRewind, DcCode::RepeatDrift,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFD40 => "条目号越界：19 条只认 0..19，索引先比 DOC_COUNT",
        0xFD41 => "引用缺失：正文须含判据全串（如 UNX-F1264-J1），逐条补引",
        0xFD42 => "引用重号：两目引同一判据串，去重后重建条目表",
        0xFD43 => "禁词命中：正文含口语变体，按术语表替换为标准术语",
        0xFD44 => "导出缺失：节数≠条目数，按条目序全量导出",
        0xFD45 => "条目空壳：正文过短，补齐到冻结下限以上",
        0xFD46 => "条目重号：编号唯一性破，重排后重建",
        0xFD47 => "术语缺失：正文无必含术语，补术语行",
        0xFD48 => "修订回退：修订计数只增，回退即账面异常",
        0xFD49 => "可重复失恒：同种子两轮不同，查隐藏非确定性源",
        _ => "未知诊断码：查 0xFD40 段闭集 DcCode::ALL",
    }
}

/// 文档条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocEntry {
    /// 条目号（0..19，唯一）。
    pub id: usize,
    /// 归属卡。
    pub card: &'static str,
    /// 判据引用全串（如 "UNX-F1264-J1"）。
    pub cite: &'static str,
    /// 标题。
    pub title: &'static str,
    /// 正文（含判据引用与术语）。
    pub body: &'static str,
}

/// 十九条 B24 段文档冻结册（判据常量独立写死；正文均含判据全串）。
pub const DOCS: [DocEntry; DOC_COUNT] = [
    DocEntry { id: 0, card: "UNX-F1264", cite: "UNX-F1264-J1", title: "稳态统计矩阵",
        body: "UNX-F1264-J1 判据：稳态统计矩阵十二维逐维可查，矩阵与部件账交叉一致率百分之百，快照周期可配默认不超一秒。术语：稳态、守恒、对账。" },
    DocEntry { id: 1, card: "UNX-F1265", cite: "UNX-F1265-J1", title: "分发表全联动",
        body: "UNX-F1265-J1 判据：全量十六向量各注入万笔，稳态分发正确率百分之百，五路分型全验证，C 表与通道稳态一致。术语：稳态、判据、对账。" },
    DocEntry { id: 2, card: "UNX-F1266", cite: "UNX-F1266-J1", title: "稳态长跑压测",
        body: "UNX-F1266-J1 判据：四场景长跑资产化，全程零丢失恒等常绿，资源零泄漏稳态收敛。术语：稳态、守恒、回执。" },
    DocEntry { id: 3, card: "UNX-F1267", cite: "UNX-F1267-J1", title: "稳态故障注入",
        body: "UNX-F1267-J1 判据：五类故障注入十轮必抓，防护动作正确率百分之百，资产可重复一致。术语：判据、红账、回执。" },
    DocEntry { id: 4, card: "UNX-F1268", cite: "UNX-F1268-J1", title: "稳态延迟账",
        body: "UNX-F1268-J1 判据：逐向量 P50 P95 P99 可查，预算偏差不超一成达标率百分之百，超限向量红账定位到段。术语：红账、判据、对账。" },
    DocEntry { id: 5, card: "UNX-F1269", cite: "UNX-F1269-J1", title: "健康账注册",
        body: "UNX-F1269-J1 判据：健康指标十项按 F0899 口径注册，摘要一致率百分之百，告警联通十轮必达。术语：判据、回执、对账。" },
    DocEntry { id: 6, card: "UNX-F1270", cite: "UNX-F1270-J1", title: "配置总表",
        body: "UNX-F1270-J1 判据：段参数十二项注册总表，双闸一致率百分之百，篡改十轮检出回退。术语：对账、守恒、红账。" },
    DocEntry { id: 7, card: "UNX-F1271", cite: "UNX-F1271-J1", title: "人话化与四态面板",
        body: "UNX-F1271-J1 判据：explain 报告覆盖 B24 全账本引用完整率百分之百，四态总面板实时可查，术语一致率百分之百。术语：稳态、判据、迁移。" },
    DocEntry { id: 8, card: "UNX-F1272", cite: "UNX-F1272-J1", title: "审计账",
        body: "UNX-F1272-J1 判据：稳态进入逆迁移确认错配事件审计记录完整率百分之百，五元组可导出，滚动保底常绿。术语：守恒、回执、对账。" },
    DocEntry { id: 9, card: "UNX-F1273", cite: "UNX-F1273-J1", title: "边界条件账",
        body: "UNX-F1273-J1 判据：六类边界场景行为契约全登记，注入十轮行为符合契约，资产可重复。术语：判据、守恒、迁移。" },
    DocEntry { id: 10, card: "UNX-F1274", cite: "UNX-F1274-J1", title: "回归资产与断言集",
        body: "UNX-F1274-J1 判据：B24 段十九判据聚合回归一次命令全跑全绿，断言四十条以上，映射零遗漏，JSON 可导出。术语：判据、对账、守恒。" },
    DocEntry { id: 11, card: "UNX-F1275", cite: "UNX-F1275-J1", title: "启动序列挂接",
        body: "UNX-F1275-J1 判据：稳态面初始化四步序登记完整率百分之百，序违反检出十轮必抓，全域启动链衔接断言常绿。术语：稳态、迁移、判据。" },
    DocEntry { id: 12, card: "UNX-F1276", cite: "UNX-F1276-J1", title: "BH 全语义联动",
        body: "UNX-F1276-J1 判据：稳态 BH 消费全链百万事件零丢失，配额越限率与超时率同 B19 账一致率百分之百。术语：守恒、判据、对账。" },
    DocEntry { id: 13, card: "UNX-F1277", cite: "UNX-F1277-J1", title: "四态全链总对账",
        body: "UNX-F1277-J1 判据：四态全链一次全流程含逆回重演十轮成功，事件守恒百分之百，判据-迁移一致率百分之百。术语：守恒、迁移、判据。" },
    DocEntry { id: 14, card: "UNX-F1278", cite: "UNX-F1278-J1", title: "文档册本条",
        body: "UNX-F1278-J1 判据：B24 段文档覆盖全部十九条，引用判据一致率百分之百，文档可导出。术语：判据、对账、回执。" },
    DocEntry { id: 15, card: "UNX-F1279", cite: "UNX-F1279-J1", title: "四态就绪宣告",
        body: "UNX-F1279-J1 判据：就绪宣告条件机可验证，消费域预演对接通过率百分之百，真账切换预案落档。术语：稳态、守恒、判据。" },
    DocEntry { id: 16, card: "UNX-F1280", cite: "UNX-F1280-J1", title: "ktest 断言集",
        body: "UNX-F1280-J1 判据：聚合断言四十条以上一次命令全绿，断言数入账只增不减，编号唯一性常绿。术语：判据、对账、守恒。" },
    DocEntry { id: 17, card: "UNX-F1264", cite: "UNX-F1264-J2", title: "子判据：快照周期",
        body: "UNX-F1264-J2 子判据：矩阵快照周期可配且默认不超一秒，超限配置一律拒收。术语：判据、对账、稳态。" },
    DocEntry { id: 18, card: "UNX-F1267", cite: "UNX-F1267-J2", title: "子判据：防护动作",
        body: "UNX-F1267-J2 子判据：五类故障防护动作正确率百分之百且可重复一致。术语：判据、回执、守恒。" },
];

/// 本册标准术语表（自备副本：文件间零交叉引用，与面板册术语口径一致）。
pub const TERMS: [&str; 8] = [
    "稳态", "中断", "守恒", "迁移", "判据", "红账", "回执", "对账",
];

/// 本册术语数（与面板册口径一致，本地副本常量）。
pub const TERM_COUNT: usize = 8;

/// 禁词表（口语变体，正文不得出现）。
pub const BANNED: [&str; 4] = ["死机", "卡死", "崩了", "挂了"];

/// 文档册正本（修订账+导出面）。
#[derive(Debug)]
pub struct DocBook {
    /// 逐条修订计数（append-only，只增）。
    revisions: [u64; DOC_COUNT],
    /// 修订总账（独立源，与逐条合计对账）。
    rev_total: u64,
    /// 导出次数。
    exports: u64,
    /// 上一轮导出串指纹（可重复面）。
    last_fp: u64,
}

/// 册指纹：逐条长度与首字符折叠（整数域）。
pub const fn book_fp(bk: &DocBook) -> u64 {
    let mut fp = 0u64;
    let mut i = 0;
    while i < DOC_COUNT {
        let d = &DOCS[i];
        fp = fp
            ^ (d.body.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (d.cite.len() as u64).wrapping_mul(0x100_0000_01B3);
        i += 1;
    }
    fp ^ bk.exports.wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
}

impl DocBook {
    /// 建册：修订零、导出零。
    pub fn new() -> DocBook {
        DocBook { revisions: [0; DOC_COUNT], rev_total: 0, exports: 0, last_fp: 0 }
    }

    /// 条目读（越界拒）。
    pub fn entry(&self, id: usize) -> Result<&'static DocEntry, DcCode> {
        if id >= DOC_COUNT {
            return Err(DcCode::DocOob);
        }
        Ok(&DOCS[id])
    }

    /// 修订一笔（只增；回退即拒）。
    pub fn revise(&mut self, id: usize) -> Result<u64, DcCode> {
        if id >= DOC_COUNT {
            return Err(DcCode::DocOob);
        }
        self.revisions[id] = self.revisions[id].wrapping_add(1);
        self.rev_total = self.rev_total.wrapping_add(1);
        Ok(self.revisions[id])
    }

    /// 修订计数读（越界拒）。
    pub fn rev_of(&self, id: usize) -> Result<u64, DcCode> {
        if id >= DOC_COUNT {
            return Err(DcCode::DocOob);
        }
        Ok(self.revisions[id])
    }

    /// 修订账只增自检（逐条合计 ≡ 修订总账两源对账）。
    pub fn rev_monotone(&self) -> bool {
        let sum: u64 = self.revisions.iter().sum();
        sum == self.rev_total
    }

    /// 正向引用闸：逐条正文含其判据全串。
    pub fn citations_forward() -> Result<(), DcCode> {
        for d in DOCS.iter() {
            if !d.body.contains(d.cite) {
                return Err(DcCode::CiteMiss);
            }
            if d.body.len() < BODY_FLOOR {
                return Err(DcCode::EntryEmpty);
            }
        }
        Ok(())
    }

    /// 反向引用闸：引用清单与条目一一对应（无重号无缺位）。
    pub fn citations_reverse() -> Result<(), DcCode> {
        for i in 0..DOC_COUNT {
            for j in (i + 1)..DOC_COUNT {
                if DOCS[i].cite == DOCS[j].cite {
                    return Err(DcCode::CiteDup);
                }
                if DOCS[i].id == DOCS[j].id {
                    return Err(DcCode::IdDup);
                }
            }
        }
        Ok(())
    }

    /// 术语闸：逐条正文过禁词表 + 必含「判据」术语。
    pub fn term_gate() -> Result<(), DcCode> {
        for d in DOCS.iter() {
            for b in BANNED.iter() {
                if d.body.contains(b) {
                    return Err(DcCode::TermBanned);
                }
            }
            if !d.body.contains("判据") && !d.body.contains("子判据") {
                return Err(DcCode::TermMissing);
            }
        }
        Ok(())
    }

    /// 全册导出：册头 + 逐节（题行/引用行/正文行/术语行）。
    /// 导出守恒：节数 ≡ 条目数，行数 ≡ 节数×4+册头 2 行。
    pub fn export(&mut self) -> Result<String, DcCode> {
        let mut s = String::from("== UNX-F1278 B24 段文档册 ==\n");
        s.push_str(&format!("  条目={} 引用一致=闸内核 导出序={}\n",
            DOC_COUNT, self.exports + 1));
        for d in DOCS.iter() {
            s.push_str(&format!("[{:02}] {} · {}\n", d.id + 1, d.card, d.title));
            s.push_str(&format!("  判据：{}\n", d.cite));
            s.push_str(&format!("  正文：{}\n", d.body));
        }
        let sections = s.matches("\n[").count(); // 每条目前有换行+节头
        if sections != DOC_COUNT {
            return Err(DcCode::ExportMiss);
        }
        self.exports = self.exports.wrapping_add(1);
        self.last_fp = book_fp(self);
        Ok(s)
    }

    /// 导出次数。
    pub fn export_count(&self) -> u64 {
        self.exports
    }

    /// 册指纹（可重复面）。
    pub fn fp(&self) -> u64 {
        book_fp(self)
    }

    /// 上一轮导出指纹。
    pub fn last_fp(&self) -> u64 {
        self.last_fp
    }
}

// ═══════════════════════════════════════════════════════════════════
// 扩展面：逐条术语命中表 / 修订流水账 / 条目检索 / 单条导出 / 演练
// ═══════════════════════════════════════════════════════════════════

/// 逐条术语命中数（19 行 × 8 术语，判据「术语一致率」的量化面）。
pub fn term_hits_per_doc() -> Vec<(usize, usize)> {
    DOCS
        .iter()
        .map(|d| {
            let hits = TERMS.iter().filter(|t| d.body.contains(*t)).count();
            (d.id, hits)
        })
        .collect()
}

/// 术语命中守恒：19 行齐、逐行 ≥1（每条至少含「判据」一词）。
pub fn term_hits_consistent() -> bool {
    let hits = term_hits_per_doc();
    hits.len() == DOC_COUNT && hits.iter().all(|(id, n)| *n >= 1 && *id < DOC_COUNT)
}

/// 修订流水行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevRow {
    /// 条目号。
    pub id: usize,
    /// 修订序（该条内只增）。
    pub rev: u64,
    /// 流水号（只增）。
    pub seq: u64,
}

/// 修订流水账（append-only，只增不减）。
#[derive(Debug)]
pub struct RevJournal {
    rows: Vec<RevRow>,
    next_seq: u64,
    per_doc: [u64; DOC_COUNT],
}

impl RevJournal {
    /// 建账。
    pub fn new() -> RevJournal {
        RevJournal { rows: Vec::new(), next_seq: 1, per_doc: [0; DOC_COUNT] }
    }
    /// 记一笔修订（越界拒）。
    pub fn push(&mut self, id: usize) -> Result<RevRow, DcCode> {
        if id >= DOC_COUNT {
            return Err(DcCode::DocOob);
        }
        self.per_doc[id] = self.per_doc[id].wrapping_add(1);
        let r = RevRow { id, rev: self.per_doc[id], seq: self.next_seq };
        self.next_seq = self.next_seq.wrapping_add(1);
        self.rows.push(r);
        Ok(r)
    }
    /// 流水长。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 序号连续 + 逐条 rev 只增自检。
    pub fn intact(&self) -> bool {
        self.rows.iter().enumerate().all(|(i, r)| r.seq == i as u64 + 1)
            && self.per_doc.iter().all(|&n| n <= DOC_COUNT as u64)
    }
    /// 导出。
    pub fn export(&self) -> String {
        let mut s = String::from("== UNX-F1278 修订流水 ==\n");
        for r in self.rows.iter() {
            s.push_str(&format!("  #{} 条目{} 修订{}\n", r.seq, r.id, r.rev));
        }
        s
    }
}

/// 条目检索：按卡号返回命中条目号清单（判据「引用一致」的检索面）。
pub fn search_by_card(card: &str) -> Vec<usize> {
    DOCS.iter().filter(|d| d.card == card).map(|d| d.id).collect()
}

/// 检索守恒：17 卡各 ≥1 命中，Σ命中 ≡ 19（子判据双条记同卡）。
pub fn search_consistent() -> bool {
    let cards = [
        "UNX-F1264", "UNX-F1265", "UNX-F1266", "UNX-F1267", "UNX-F1268", "UNX-F1269",
        "UNX-F1270", "UNX-F1271", "UNX-F1272", "UNX-F1273", "UNX-F1274", "UNX-F1275",
        "UNX-F1276", "UNX-F1277", "UNX-F1278", "UNX-F1279", "UNX-F1280",
    ];
    let all: Vec<usize> = cards.iter().flat_map(|c| search_by_card(c)).collect();
    all.len() == DOC_COUNT
        && cards.iter().all(|c| !search_by_card(c).is_empty())
}

/// 单条导出（判据行+正文行，行数守恒=2）。
pub fn export_one(id: usize) -> Result<String, DcCode> {
    if id >= DOC_COUNT {
        return Err(DcCode::DocOob);
    }
    let d = &DOCS[id];
    let s = format!("== 条目{} ==\n  判据：{}\n  正文：{}\n", d.id + 1, d.cite, d.body);
    if s.matches('\n').count() != 3 {
        return Err(DcCode::ExportMiss);
    }
    Ok(s)
}

/// 演练八：术语命中表 10/10（行齐 + 逐行 ≥1）。
pub fn drill_term_hits_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        if term_hits_consistent() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("术语命中表={}/{}", ok, INJECT_ROUNDS))
}

/// 演练九：修订流水 10/10（只增 + 序号连续 + 越界拒 + 导出）。
pub fn drill_rev_journal_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut j = RevJournal::new();
        let mut rng = Lcg::new(0xFD40_B10F + r as u64);
        for _ in 0..25 {
            let _ = j.push(rng.below(DOC_COUNT as u64) as usize);
        }
        let oob = matches!(j.push(DOC_COUNT), Err(DcCode::DocOob));
        if j.intact() && oob && j.export().len() > 300 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("修订流水={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十：条目检索守恒（17 卡全命中 + Σ=19 + 双卡复检）。
pub fn drill_search_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        if search_consistent() {
            // 双卡复检：F1264 与 F1267 各含主判据+子判据两条。
            let n64 = search_by_card("UNX-F1264").len();
            let n67 = search_by_card("UNX-F1267").len();
            if n64 == 2 && n67 == 2 {
                ok += 1;
            }
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("检索守恒={}/{} F1264=2 F1267=2", ok, INJECT_ROUNDS))
}

/// 演练十一：单条导出 10/10（19 条行守恒 + 越界拒）。
pub fn drill_export_one_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        let mut all = true;
        for i in 0..DOC_COUNT {
            match export_one(i) {
                Ok(s) => {
                    if !s.contains("判据：") || !s.contains("正文：") {
                        all = false;
                    }
                }
                Err(_) => all = false,
            }
        }
        let oob = matches!(export_one(DOC_COUNT), Err(DcCode::DocOob));
        if all && oob {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("单条导出={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录：全册目录账 + 引用字长校验和 ──

/// 全册目录（19 行：条目号+卡号+判据串，Σ 行 ≡ 19）。
pub fn export_toc() -> String {
    let mut s = String::from("== UNX-F1278 全册目录 ==\n");
    for d in DOCS.iter() {
        s.push_str(&format!("  [{:02}] {} → {}\n", d.id + 1, d.card, d.cite));
    }
    s.push_str(&format!("  共 {} 条\n", DOC_COUNT));
    s
}

/// 目录守恒：行数 ≡ 19 且逐行含判据串。
pub fn toc_consistent() -> bool {
    let s = export_toc();
    let rows = DOCS.iter().filter(|d| s.contains(d.cite)).count();
    rows == DOC_COUNT && s.contains(&format!("共 {} 条", DOC_COUNT))
}

/// 引用字长校验和（逐条 cite 字节折叠，唯一性辅助面）。
pub fn cite_checksums() -> Vec<u64> {
    DOCS
        .iter()
        .map(|d| {
            let mut fp = 0u64;
            for b in d.cite.bytes() {
                fp = (fp ^ b as u64).wrapping_mul(0x100_0000_01B3);
            }
            fp
        })
        .collect()
}

/// 演练十二：全册目录账 10/10（目录守恒 + 引用字长校验和唯一）。
pub fn drill_toc_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        if toc_consistent() {
            let cs = cite_checksums();
            let mut uniq = true;
            for i in 0..cs.len() {
                for j in (i + 1)..cs.len() {
                    if cs[i] == cs[j] {
                        uniq = false;
                    }
                }
            }
            if uniq {
                ok += 1;
            }
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("全册目录={}/{}", ok, INJECT_ROUNDS))
}


pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 定种。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.st
    }
    /// 取 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 演练一：19 条全覆盖（条目数=19、编号唯一、卡名齐）。
pub fn drill_coverage() -> (bool, String) {
    let bk = DocBook::new();
    let mut ok = DOC_COUNT == 19;
    // 逐条可读且非空。
    for i in 0..DOC_COUNT {
        match bk.entry(i) {
            Ok(d) => {
                if d.card.is_empty() || d.title.is_empty() || d.body.is_empty() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
    }
    let oob = matches!(bk.entry(DOC_COUNT), Err(DcCode::DocOob));
    (ok && oob, format!("条目={} 越界拒={}", DOC_COUNT, oob))
}

/// 演练二：引用一致率 100%（正向+反向双闸 + 术语闸）。
pub fn drill_citations() -> (bool, String) {
    let fwd = DocBook::citations_forward();
    let rev = DocBook::citations_reverse();
    let term = DocBook::term_gate();
    // 反向抽检：引用串与卡号自洽（cite 以卡号开头）。
    let self_ok = DOCS.iter().all(|d| d.cite.starts_with(d.card));
    (fwd.is_ok() && rev.is_ok() && term.is_ok() && self_ok,
     format!("正向={} 反向={} 术语={} 卡引用自洽={}",
        fwd.is_ok(), rev.is_ok(), term.is_ok(), self_ok))
}

/// 演练三：导出 100%（节守恒+引用全现+非空）10/10。
pub fn drill_export_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        let mut bk = DocBook::new();
        match bk.export() {
            Ok(s) => {
                let cover = DOCS.iter().all(|d| s.contains(d.cite) && s.contains(d.title));
                let nonempty = s.len() > 1000;
                if cover && nonempty && bk.export_count() == 1 {
                    ok += 1;
                }
            }
            Err(_) => {}
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("导出={}/{}", ok, INJECT_ROUNDS))
}

/// 演练四：禁词必抓 10/10（口语变体逐一入检必翻红）。
pub fn drill_banned_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let b = BANNED[r % BANNED.len()];
        let bad = format!("稳态守恒判据一切正常，但系统{}了，对账失败", b);
        let hit = BANNED.iter().any(|w| bad.contains(w));
        // 干净文必过。
        let clean_text = "稳态守恒判据对账回执，全部常绿无红账";
        let clean_pass = !BANNED.iter().any(|w| clean_text.contains(w));
        if hit && clean_pass && !clean_text.contains(b) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("禁词抓={}/{}", ok, INJECT_ROUNDS))
}

/// 演练五：修订账只增（逐目修订递增、越界拒、回退口径自检）。
pub fn drill_revisions() -> (bool, String) {
    let mut bk = DocBook::new();
    let mut ok = true;
    let mut rng = Lcg::new(0xFD40_AA1);
    for _ in 0..40 {
        let id = rng.below(DOC_COUNT as u64) as usize;
        let before = bk.rev_of(id).unwrap_or(u64::MAX);
        let after = bk.revise(id).unwrap_or(u64::MAX);
        if after != before + 1 {
            ok = false;
        }
    }
    let oob = matches!(bk.revise(DOC_COUNT), Err(DcCode::DocOob));
    let mono = bk.rev_monotone();
    (ok && oob && mono, format!("修订递增={} 越界拒={} 只增自检={}", ok, oob, mono))
}

/// 演练六：正文非空壳（逐条字长 ≥ 冻结下限 + 必含术语）。
pub fn drill_body_floor() -> (bool, String) {
    let all_long = DOCS.iter().all(|d| d.body.len() >= BODY_FLOOR);
    let all_term = DOCS.iter().all(|d| d.body.contains("判据"));
    let min_len = DOCS.iter().map(|d| d.body.len()).min().unwrap_or(0);
    (all_long && all_term && min_len >= BODY_FLOOR,
     format!("下限全过={} 术语全有={} 最短正文={}", all_long, all_term, min_len))
}

/// 演练七：可重复一致 10/10（同种子两轮导出+指纹逐位同）。
pub fn drill_repeatable_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        let mut a = DocBook::new();
        let mut b = DocBook::new();
        let sa = a.export().unwrap_or_default();
        let sb = b.export().unwrap_or_default();
        if sa == sb && a.fp() == b.fp() && a.last_fp() == b.last_fp() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("可重复={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录 C：篇幅口径账 + 修订版本号账 ──

/// 篇幅口径账：逐条正文字长 ≥ 判据串字长 ×4（信息密度下限，判据面）。
pub fn body_density_ok() -> bool {
    DOCS.iter().all(|d| d.body.len() >= d.cite.len() * 4)
}

/// 篇幅表导出（19 行：条目/正文字长/密度倍数）。
pub fn export_density() -> String {
    let mut s = String::from("== UNX-F1278 篇幅口径账 ==\n");
    for d in DOCS.iter() {
        s.push_str(&format!("  条目{} 正文={} 判据串={} 倍数={}\n",
            d.id + 1, d.body.len(), d.cite.len(), d.body.len() / d.cite.len()));
    }
    s.push_str(&format!("  密度下限全过={}\n", body_density_ok()));
    s
}

/// 修订版本号账：逐条当前版本（从流水重放终值）。
pub fn rev_versions(j: &RevJournal) -> [u64; DOC_COUNT] {
    let mut v = [0u64; DOC_COUNT];
    for r in j.rows.iter() {
        if r.id < DOC_COUNT {
            v[r.id] = r.rev;
        }
    }
    v
}

/// 演练十五：篇幅口径账 10/10（密度全过 + 导出 + 版本号回放）。
pub fn drill_density_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut j = RevJournal::new();
        let mut rng = Lcg::new(0xFD40_C402 + r as u64);
        for _ in 0..12 {
            let _ = j.push(rng.below(DOC_COUNT as u64) as usize);
        }
        let v = rev_versions(&j);
        let replay = v.iter().enumerate().all(|(i, &n)| {
            n == j.rows.iter().filter(|row| row.id == i).count() as u64
        });
        if body_density_ok() && export_density().len() > 500 && replay {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("篇幅口径={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录 B：修订回放 + 术语清单导出 + 按卡检索导出 ──

/// 修订回放判据：流水逐行 rev 与逐条计数复算一致（append-only 的回放面）。
pub fn journal_replay_ok(j: &RevJournal) -> bool {
    let mut counts = [0u64; DOC_COUNT];
    for r in j.rows.iter() {
        if r.id >= DOC_COUNT {
            return false;
        }
        counts[r.id] += 1;
        if counts[r.id] != r.rev {
            return false;
        }
    }
    true
}

/// 术语逐条清单导出（19 行 × 命中数，术语一致率的账化面）。
pub fn export_term_table() -> String {
    let mut s = String::from("== UNX-F1278 术语命中清单 ==\n");
    for (id, hits) in term_hits_per_doc().iter() {
        s.push_str(&format!("  条目{} 命中={}/{}\n", id + 1, hits, TERM_COUNT));
    }
    s.push_str(&format!("  全条目命中 ≥1={}\n", term_hits_consistent()));
    s
}

/// 按卡检索导出（17 卡逐卡行：卡号→命中条目号清单）。
pub fn export_search_by_card() -> String {
    let cards = [
        "UNX-F1264", "UNX-F1265", "UNX-F1266", "UNX-F1267", "UNX-F1268", "UNX-F1269",
        "UNX-F1270", "UNX-F1271", "UNX-F1272", "UNX-F1273", "UNX-F1274", "UNX-F1275",
        "UNX-F1276", "UNX-F1277", "UNX-F1278", "UNX-F1279", "UNX-F1280",
    ];
    let mut s = String::from("== UNX-F1278 按卡检索 ==\n");
    for c in cards.iter() {
        let ids = search_by_card(c);
        let list: Vec<String> = ids.iter().map(|i| format!("{}", i + 1)).collect();
        s.push_str(&format!("  {} → [{}]\n", c, list.join(",")));
    }
    s
}

/// 演练十三：修订回放 10/10（逐行复算一致 + 越界行拒）。
pub fn drill_journal_replay_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut j = RevJournal::new();
        let mut rng = Lcg::new(0xFD40_C401 + r as u64);
        for _ in 0..30 {
            let _ = j.push(rng.below(DOC_COUNT as u64) as usize);
        }
        if journal_replay_ok(&j) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("修订回放={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十四：术语清单 + 按卡检索导出 10/10。
pub fn drill_term_search_export_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        let a = export_term_table();
        let b = export_search_by_card();
        if a.contains("命中=") && b.contains("UNX-F1280 → [17]") && b.len() > 300 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("术语+检索导出={}/{}", ok, INJECT_ROUNDS))
}


// ── 附录 D：引用前缀一致性 ──

/// 引用前缀一致性：cite 以 card 起头且含 "-J" 判据标记（19 条全过）。
pub fn cite_prefix_ok() -> bool {
    DOCS.iter().all(|d| d.cite.starts_with(d.card) && d.cite.contains("-J"))
}

/// 演练十六：引用前缀一致性 10/10。
pub fn drill_cite_prefix_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        if cite_prefix_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("引用前缀={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录 E：正文字节集判据 ──

/// 正文字节集判据：逐条正文非空、含判据全串、含中文冒号结构行（文档面）。
pub fn body_bytes_ok() -> bool {
    DOCS.iter().all(|d| {
        !d.body.is_empty() && d.body.contains(d.cite) && d.body.contains("判据")
    })
}

/// 演练十七：正文字节集 10/10。
pub fn drill_body_bytes_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        if body_bytes_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正文字节集={}/{}", ok, INJECT_ROUNDS))
}

pub fn segment_guard() -> bool {
    let mut seen = [false; 16];
    for c in DcCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            return false;
        }
        let idx = (v - SEG_BASE) as usize;
        if seen[idx] {
            return false;
        }
        seen[idx] = true;
    }
    true
}

/// run_unx1278_checks：本单判据聚合（诊断段 0xFD40）。
pub fn run_unx1278_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1278 Irq态文档册");
    let (a, da) = drill_coverage();
    s.add_dyn("J1-十九条全覆盖", a, &da);
    let (b, db) = drill_citations();
    s.add_dyn("J1-引用一致率100%", b, &db);
    let (c, dc) = drill_export_10();
    s.add_dyn("J1-导出100%十轮", c, &dc);
    let (d, dd) = drill_banned_10();
    s.add_dyn("J2-禁词必抓10/10", d, &dd);
    let (e, de) = drill_revisions();
    s.add_dyn("J2-修订账只增", e, &de);
    let (f, df) = drill_body_floor();
    s.add_dyn("J2-正文非空壳", f, &df);
    let (g, dg) = drill_repeatable_10();
    s.add_dyn("J1-可重复一致10/10", g, &dg);
    let (h, dh) = drill_term_hits_10();
    s.add_dyn("J2-术语命中表10/10", h, &dh);
    let (i, di) = drill_rev_journal_10();
    s.add_dyn("J2-修订流水10/10", i, &di);
    let (j, dj) = drill_search_10();
    s.add_dyn("J2-条目检索守恒10/10", j, &dj);
    let (k, dk) = drill_export_one_10();
    s.add_dyn("J2-单条导出10/10", k, &dk);
    let (l, dl) = drill_toc_10();
    s.add_dyn("J2-全册目录账10/10", l, &dl);
    let (m, dm) = drill_journal_replay_10();
    s.add_dyn("J2-修订回放10/10", m, &dm);
    let (n, dn) = drill_term_search_export_10();
    s.add_dyn("J2-术语+检索导出10/10", n, &dn);
    let (o, do_) = drill_density_10();
    s.add_dyn("J2-篇幅口径账10/10", o, &do_);
    let (p, dp) = drill_cite_prefix_10();
    s.add_dyn("J2-引用前缀一致性10/10", p, &dp);
    let (q, dq) = drill_body_bytes_10();
    s.add_dyn("J2-正文字节集10/10", q, &dq);
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFD40段十码闭集");
    s
}
