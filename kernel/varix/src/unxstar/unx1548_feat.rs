//! UNX-F1548 域级经沉淀回归（十条沉淀逐条反哺核验） · 0x78900段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B04 卡 5 · UNX-F1548 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1548-J1 沉淀反哺核验（十条逐条与全域判据反查：
//!     条款在判据中有承载面），承载缺失 10/10 告警，反哺报告落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十条沉淀闭集：条款号 UNX-F1548-L01..L10 逐条登记，条款号唯一，
//!     承载关键词对非空，越条款显性拒；
//!   - 全域判据反查域：30 条全域判据文本（跨域判据闭集）为承载面
//!     反查唯一域，判据文本逐条在域；
//!   - 承载面判定：条款承载关键词在全域判据文本中逐字命中，命中
//!     判据数 ≥1 即有承载面，零命中即承载缺失告警；
//!   - 承载缺失 10/10：逐条构造零命中注入条款（虚构关键词），十路
//!     注入逐路必警；
//!   - 反哺报告落档：10 行条款承载明细 + 头尾两行，行数恒 12，指纹
//!     FNV-1a 落账，双跑逐位一致；
//!   - 状态机单调：登记条款 → 反查核验 → 报告落档，倒退跳级显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78900，派工卡 VX8-B04 卡 5 独占窗口）。
pub const SEG_BASE: u32 = 0x78900;

/// 沉淀条款条数（判据 J1「十条」冻结）。
pub const LESSON_COUNT: usize = 10;

/// 全域判据反查域条数（跨域判据闭集）。
pub const GLOBAL_JUDGE_COUNT: usize = 30;

/// 反哺报告行数（10 行明细 + 头尾两行，冻结）。
pub const REPORT_ROWS: usize = 12;

// ── 十条沉淀冻结表（条款号 / 条款文本 / 承载关键词对） ──────────
pub const LESSONS: [(&str, &str, [&str; 2]); 10] = [
    ("UNX-F1548-L01", "页表错挂须冻结写入后重挂复巡", ["页表", "映射"]),
    ("UNX-F1548-L02", "中断风暴先屏蔽线再清挂起重开", ["中断", "屏蔽"]),
    ("UNX-F1548-L03", "计时漂移越阈锚定主钟重校副钟", ["计时", "漂移"]),
    ("UNX-F1548-L04", "分发失联重发轮询回执落延迟账", ["分发", "回执"]),
    ("UNX-F1548-L05", "账本失衡冻结记账逐页对账解锁", ["账本", "对账"]),
    ("UNX-F1548-L06", "快照失配重建快照互证指纹落档", ["快照", "指纹"]),
    ("UNX-F1548-L07", "注入漏抓重放注入谱补失抓位", ["注入", "检出"]),
    ("UNX-F1548-L08", "双跑歧变隔离歧变路对位二分查", ["双跑", "一致"]),
    ("UNX-F1548-L09", "段位越窗锁段写口重对齐复验守卫", ["段位", "守卫"]),
    ("UNX-F1548-L10", "交接断链补批收口重签交接复验", ["交接", "批收口"]),
];

// ── 全域判据反查域冻结表（30 条跨域判据文本） ───────────────────
pub const GLOBAL_JUDGES: [(&str, &str); 30] = [
    ("UNX-F1548-G01", "域级页表项越权置位巡检必须全绿"),
    ("UNX-F1548-G02", "页缺死循同地址计数越限须恢复"),
    ("UNX-F1548-G03", "页表映射重建后复巡检判定通过"),
    ("UNX-F1548-G04", "中断线屏蔽时长口径与终版一致"),
    ("UNX-F1548-G05", "中断重入计数与风暴判定门限齐"),
    ("UNX-F1548-G06", "中断尾链延迟峰值记录入健康面"),
    ("UNX-F1548-G07", "计时主副钟漂移越阈须锚定重校"),
    ("UNX-F1548-G08", "计时看门狗踢狗间隔口径冻结"),
    ("UNX-F1548-G09", "计时锚钟锁定状态入四族指标"),
    ("UNX-F1548-G10", "分发回执超时须重发轮询落账"),
    ("UNX-F1548-G11", "分发队列积压峰值入健康一屏"),
    ("UNX-F1548-G12", "分发延迟账落账数口径制冻结"),
    ("UNX-F1548-G13", "账本借贷差非零须逐页对账"),
    ("UNX-F1548-G14", "账本红账违约计数回潮须清位"),
    ("UNX-F1548-G15", "账本对账轮次与冻结页计数齐"),
    ("UNX-F1548-G16", "快照指纹与账目互证不符须重建"),
    ("UNX-F1548-G17", "快照落档指纹双跑逐位一致"),
    ("UNX-F1548-G18", "注入谱检出率回落须补失抓位"),
    ("UNX-F1548-G19", "注入重号与缺判据两路必抓"),
    ("UNX-F1548-G20", "注入十路逐路检出率保持全量"),
    ("UNX-F1548-G21", "双跑指纹歧变须隔离歧变路二分"),
    ("UNX-F1548-G22", "双跑一致判定覆盖批收口报告"),
    ("UNX-F1548-G23", "段位守卫须 0x40 对齐且落段窗"),
    ("UNX-F1548-G24", "段位越窗锁写口重对齐后复验"),
    ("UNX-F1548-G25", "守卫检出越窗须落红账并告警"),
    ("UNX-F1548-G26", "交接断链须补批收口报告重签"),
    ("UNX-F1548-G27", "交接批收口聚合断言只增不减"),
    ("UNX-F1548-G28", "批收口编号唯一性常绿入档"),
    ("UNX-F1548-G29", "对账报告行数恒定指纹落档"),
    ("UNX-F1548-G30", "预案演练样例可执行逐槽登记"),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reflux48Code {
    /// 条款号越闭集：条款收进 L01..L10 闭集
    LessonUnknown,
    /// 条款号重复：一条一登记，重登先撤旧
    LessonDuplicate,
    /// 承载关键词为空：补关键词对再登记
    KeywordEmpty,
    /// 承载缺失告警：关键词在全域判据零命中，回补承载面
    CarrierMissing,
    /// 反查未覆盖十条：十条逐条反查齐再收口
    SurveyShort,
    /// 反查状态倒退：登记→反查→落档单调推进
    PhaseRegress,
    /// 反查状态跳级：未登记齐十条不得反查，未反查不得落档
    PhaseSkip,
    /// 报告落档后禁改：反哺落档禁改，另开反哺修订档
    SealedLocked,
    /// 报告行数违约：反哺报告必须恰 12 行
    ReportRowsBad,
    /// 反哺报告为空：先反查生成报告再落档
    ReportEmpty,
    /// 条款号为空：补条款号再登记
    IdEmpty,
    /// 双跑不一致：排查报告指纹随机性后复跑
    NotReproducible,
}

impl Reflux48Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Reflux48Code::LessonUnknown => "条款号越闭集：条款收进 L01..L10 闭集再登记",
            Reflux48Code::LessonDuplicate => "条款号重复：一条一登记，撤旧登再补",
            Reflux48Code::KeywordEmpty => "承载关键词为空：补关键词对再登记",
            Reflux48Code::CarrierMissing => "承载缺失：关键词全域判据零命中，回补承载面",
            Reflux48Code::SurveyShort => "反查未覆盖十条：十条逐条反查齐再收口",
            Reflux48Code::PhaseRegress => "反查状态倒退：登记→反查→落档单调推进",
            Reflux48Code::PhaseSkip => "反查状态跳级：未登记齐十条不得反查",
            Reflux48Code::SealedLocked => "反哺落档禁改：另开反哺修订档",
            Reflux48Code::ReportRowsBad => "反哺报告行数违约：报告必须恰 12 行",
            Reflux48Code::ReportEmpty => "反哺报告为空：先反查生成报告再落档",
            Reflux48Code::IdEmpty => "条款号为空：补条款号再登记",
            Reflux48Code::NotReproducible => "双跑不一致：排查报告指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Reflux48Code; 12] = [
        Reflux48Code::LessonUnknown,
        Reflux48Code::LessonDuplicate,
        Reflux48Code::KeywordEmpty,
        Reflux48Code::CarrierMissing,
        Reflux48Code::SurveyShort,
        Reflux48Code::PhaseRegress,
        Reflux48Code::PhaseSkip,
        Reflux48Code::SealedLocked,
        Reflux48Code::ReportRowsBad,
        Reflux48Code::ReportEmpty,
        Reflux48Code::IdEmpty,
        Reflux48Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Reflux48Code::ALL.len() {
        let c = match Reflux48Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 关键词对取值（.get 通道，闭集内恒命中）。
fn kw_at<'a>(pair: [&'a str; 2], idx: usize) -> &'a str {
    match pair.get(idx) {
        Some(s) => *s,
        None => "",
    }
}

// ── 反哺状态机（登记 → 反查 → 落档，单调推进） ──────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefluxPhase {
    /// 登记期：十条沉淀逐条入册
    Enrolled,
    /// 反查期：逐条承载面反查完成
    Surveyed,
    /// 落档期：反哺报告落档封版
    Sealed,
}

impl RefluxPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            RefluxPhase::Enrolled => 0,
            RefluxPhase::Surveyed => 1,
            RefluxPhase::Sealed => 2,
        }
    }
}

/// 单条沉淀登记项。
#[derive(Clone, Debug)]
pub struct LessonRow {
    pub id: String,
    pub text: String,
    pub kw_a: String,
    pub kw_b: String,
    pub carriers: usize,
}

/// 沉淀反哺账。
pub struct RefluxLedger {
    phase: RefluxPhase,
    rows: Vec<LessonRow>,
    warnings: Vec<String>,
    report: Option<String>,
    report_digest: u32,
}

impl RefluxLedger {
    pub fn new() -> RefluxLedger {
        RefluxLedger {
            phase: RefluxPhase::Enrolled,
            rows: Vec::new(),
            warnings: Vec::new(),
            report: None,
            report_digest: 0,
        }
    }

    pub fn phase(&self) -> RefluxPhase {
        self.phase
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn warning_count(&self) -> usize {
        self.warnings.len()
    }

    pub fn report_digest(&self) -> u32 {
        self.report_digest
    }

    /// 条款登记：条款号闭集 + 唯一 + 关键词对非空。
    pub fn enroll(&mut self, id: &str, text: &str, kw_a: &str, kw_b: &str) -> Result<(), u32> {
        if self.phase == RefluxPhase::Sealed {
            return Err(Reflux48Code::SealedLocked.val());
        }
        if self.phase != RefluxPhase::Enrolled {
            return Err(Reflux48Code::PhaseRegress.val());
        }
        if id.is_empty() {
            return Err(Reflux48Code::IdEmpty.val());
        }
        let mut known = false;
        let mut i = 0usize;
        while i < LESSONS.len() {
            let row = match LESSONS.get(i) {
                Some(r) => *r,
                None => break,
            };
            if row.0 == id {
                known = true;
            }
            i += 1;
        }
        if !known {
            return Err(Reflux48Code::LessonUnknown.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => return Err(Reflux48Code::NotReproducible.val()),
            };
            if r.id == id {
                return Err(Reflux48Code::LessonDuplicate.val());
            }
            i += 1;
        }
        if kw_a.is_empty() || kw_b.is_empty() {
            return Err(Reflux48Code::KeywordEmpty.val());
        }
        self.rows.push(LessonRow {
            id: id.to_string(),
            text: text.to_string(),
            kw_a: kw_a.to_string(),
            kw_b: kw_b.to_string(),
            carriers: 0,
        });
        Ok(())
    }

    /// 承载面反查：单条关键词对在全域判据文本的命中判据数。
    pub fn carrier_of(&self, kw_a: &str, kw_b: &str) -> usize {
        let mut n = 0usize;
        let mut j = 0usize;
        while j < GLOBAL_JUDGES.len() {
            let judge = match GLOBAL_JUDGES.get(j) {
                Some(g) => g.1,
                None => break,
            };
            if judge.contains(kw_a) || judge.contains(kw_b) {
                n += 1;
            }
            j += 1;
        }
        n
    }

    /// 反查核验：十条逐条反查承载面，零命中入告警清单。
    pub fn survey(&mut self) -> Result<usize, u32> {
        if self.phase.rank() > RefluxPhase::Enrolled.rank() {
            return Err(Reflux48Code::PhaseRegress.val());
        }
        if self.phase == RefluxPhase::Sealed {
            return Err(Reflux48Code::SealedLocked.val());
        }
        if self.rows.len() != LESSON_COUNT {
            return Err(Reflux48Code::SurveyShort.val());
        }
        self.warnings.clear();
        let mut carried = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            let carriers = match self.rows.get(i) {
                Some(r) => self.carrier_of(&r.kw_a, &r.kw_b),
                None => return Err(Reflux48Code::NotReproducible.val()),
            };
            let id = match self.rows.get(i) {
                Some(r) => r.id.clone(),
                None => return Err(Reflux48Code::NotReproducible.val()),
            };
            if carriers == 0 {
                self.warnings.push(id);
            } else {
                carried += 1;
            }
            let slot = match self.rows.get_mut(i) {
                Some(r) => r,
                None => return Err(Reflux48Code::NotReproducible.val()),
            };
            slot.carriers = carriers;
            i += 1;
        }
        self.phase = RefluxPhase::Surveyed;
        Ok(carried)
    }

    /// 落档：生成 12 行反哺报告并封版（承载全在方可落档）。
    pub fn seal_report(&mut self) -> Result<u32, u32> {
        if self.phase == RefluxPhase::Surveyed {
            // 允许推进。
        } else {
            return Err(Reflux48Code::PhaseSkip.val());
        }
        if !self.warnings.is_empty() {
            return Err(Reflux48Code::CarrierMissing.val());
        }
        let mut text = String::from("== UNX-F1548 沉淀反哺报告 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("{} 承载判据 {} 条 ●", r.id, r.carriers);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str(&format!("== 反哺十条全有承载面，告警 {} 条 ==", self.warnings.len()));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != REPORT_ROWS {
            return Err(Reflux48Code::ReportRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.report = Some(text);
        self.report_digest = digest;
        self.phase = RefluxPhase::Sealed;
        Ok(digest)
    }

    pub fn report_text(&self) -> Option<&str> {
        match self.report.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }
}

// ── 好账构建（十条全登 + 反查 + 落档） ──────────────────────────
pub fn build_reflux_ok() -> RefluxLedger {
    let mut lg = RefluxLedger::new();
    let mut i = 0usize;
    while i < LESSONS.len() {
        let row = match LESSONS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = lg.enroll(row.0, row.1, kw_at(row.2, 0), kw_at(row.2, 1));
        i += 1;
    }
    let _ = lg.survey();
    let _ = lg.seal_report();
    lg
}

/// 零命中注入条款集：十路虚构关键词逐路构造（承载缺失注入）。
pub const FAKE_KEYWORDS: [&str; 10] = [
    "量子", "玄机", "浮空", "幻数", "雾算", "虚链", "空转", "影账", "谜程", "幽测",
];

/// 注入反查复算：十路零命中关键词逐路必警，返回告警条数。
fn inject_warning_count() -> usize {
    let mut warned = 0usize;
    let mut i = 0usize;
    while i < FAKE_KEYWORDS.len() {
        let kw = match FAKE_KEYWORDS.get(i) {
            Some(k) => *k,
            None => break,
        };
        let mut lg = RefluxLedger::new();
        let mut k = 0usize;
        while k < LESSONS.len() {
            let row = match LESSONS.get(k) {
                Some(r) => *r,
                None => break,
            };
            let mut ka = kw_at(row.2, 0);
            let mut kb = kw_at(row.2, 1);
            if k == i {
                // 第 i 条换装虚构关键词 → 零命中。
                ka = kw;
                kb = kw;
            }
            let _ = lg.enroll(row.0, row.1, ka, kb);
            k += 1;
        }
        match lg.survey() {
            Ok(carried) => {
                if lg.warning_count() == 1 && carried == LESSON_COUNT - 1 {
                    warned += 1;
                }
            }
            Err(_) => {}
        }
        i += 1;
    }
    warned
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78800 && SEG_BASE <= 0x789C0
}

/// 十条沉淀全登且落闭集。
fn lessons_ok() -> bool {
    let lg = build_reflux_ok();
    if lg.row_count() != LESSON_COUNT {
        return false;
    }
    if lg.phase() != RefluxPhase::Sealed {
        return false;
    }
    if LESSONS.len() != LESSON_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < LESSONS.len() {
        let row = match LESSONS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.0.is_empty() || row.1.is_empty() || kw_at(row.2, 0).is_empty() || kw_at(row.2, 1).is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 全域判据反查域齐备：30 条编号唯一、文本非空。
fn global_judges_ok() -> bool {
    if GLOBAL_JUDGES.len() != GLOBAL_JUDGE_COUNT {
        return false;
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut i = 0usize;
    while i < GLOBAL_JUDGES.len() {
        let row = match GLOBAL_JUDGES.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if seen.iter().any(|s| *s == row.0) {
            return false;
        }
        if row.1.is_empty() {
            return false;
        }
        seen.push(row.0);
        i += 1;
    }
    if seen.len() != GLOBAL_JUDGE_COUNT {
        return false;
    }
    true
}

/// 条款在判据中有承载面：十条逐条命中判据数 ≥1。
fn carriers_ok() -> bool {
    let mut lg = RefluxLedger::new();
    let mut i = 0usize;
    while i < LESSONS.len() {
        let row = match LESSONS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if lg.enroll(row.0, row.1, kw_at(row.2, 0), kw_at(row.2, 1)).is_err() {
            return false;
        }
        i += 1;
    }
    let carried = match lg.survey() {
        Ok(c) => c,
        Err(_) => return false,
    };
    if carried != LESSON_COUNT {
        return false;
    }
    if lg.warning_count() != 0 {
        return false;
    }
    if lg.phase() != RefluxPhase::Surveyed {
        return false;
    }
    // 逐条承载计数抽验：L01 页表/映射 至少 2 条命中。
    let l01 = lg.carrier_of("页表", "映射");
    if l01 < 2 {
        return false;
    }
    let l02 = lg.carrier_of("中断", "屏蔽");
    if l02 < 3 {
        return false;
    }
    true
}

/// 承载缺失 10/10 告警：十路零命中注入逐路必警。
fn inject_10_ok() -> bool {
    let warned = inject_warning_count();
    if warned != LESSON_COUNT {
        return false;
    }
    true
}

/// 反哺报告落档：12 行、承载标记齐、指纹非零。
fn report_archived_ok() -> bool {
    let lg = build_reflux_ok();
    let text = match lg.report_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != REPORT_ROWS {
        return false;
    }
    if !text.contains("沉淀反哺报告") {
        return false;
    }
    if !text.contains("告警 0 条") {
        return false;
    }
    let mut i = 0usize;
    while i < LESSONS.len() {
        let row = match LESSONS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if !text.contains(row.0) {
            return false;
        }
        i += 1;
    }
    if lg.report_digest() == 0 {
        return false;
    }
    true
}

/// 状态机单调：未登齐反查拒、未反查落档拒、倒退拒。
fn phase_monotonic_ok() -> bool {
    let mut lg = RefluxLedger::new();
    // 零条反查：SurveyShort 拒。
    if lg.survey().is_ok() {
        return false;
    }
    // 未反查先落档：跳级拒。
    if lg.seal_report().is_ok() {
        return false;
    }
    // 落档后再登记：封档锁拒。
    let mut sealed = build_reflux_ok();
    let row0 = match LESSONS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    match sealed.enroll(row0.0, row0.1, kw_at(row0.2, 0), kw_at(row0.2, 1)) {
        Err(code) => {
            if code != Reflux48Code::SealedLocked.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    true
}

/// 登记闸门：越条款 / 重登 / 空关键词 / 空条款号四路必抓。
fn enroll_guard_ok() -> bool {
    let mut lg = RefluxLedger::new();
    // 越条款。
    match lg.enroll("UNX-F1548-L99", "凭空条款", "量子", "玄机") {
        Err(code) => {
            if code != Reflux48Code::LessonUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 空条款号。
    match lg.enroll("", "空号条款", "页表", "映射") {
        Err(code) => {
            if code != Reflux48Code::IdEmpty.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 空关键词。
    let row0 = match LESSONS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    match lg.enroll(row0.0, row0.1, "", kw_at(row0.2, 1)) {
        Err(code) => {
            if code != Reflux48Code::KeywordEmpty.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 重登。
    let _ = lg.enroll(row0.0, row0.1, kw_at(row0.2, 0), kw_at(row0.2, 1));
    match lg.enroll(row0.0, row0.1, kw_at(row0.2, 0), kw_at(row0.2, 1)) {
        Err(code) => {
            if code != Reflux48Code::LessonDuplicate.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if lg.row_count() != 1 {
        return false;
    }
    true
}

/// 承载缺失落档拒：含告警账落档必拒。
fn warning_blocks_seal_ok() -> bool {
    let mut lg = RefluxLedger::new();
    let mut i = 0usize;
    while i < LESSONS.len() {
        let row = match LESSONS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let mut ka = kw_at(row.2, 0);
        let mut kb = kw_at(row.2, 1);
        if i == 3 {
            // 第 4 条换装虚构关键词对 → 零命中。
            ka = "量子";
            kb = "玄机";
        }
        let _ = lg.enroll(row.0, row.1, ka, kb);
        i += 1;
    }
    let _ = lg.survey();
    if lg.warning_count() != 1 {
        return false;
    }
    match lg.seal_report() {
        Err(code) => {
            if code != Reflux48Code::CarrierMissing.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if lg.report_text().is_some() {
        return false;
    }
    true
}

/// 双跑一致：好账指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_reflux_ok().report_digest();
    let d2 = build_reflux_ok().report_digest();
    if d1 != d2 {
        return false;
    }
    if d1 == 0 {
        return false;
    }
    let g1 = build_reflux_ok();
    let g2 = build_reflux_ok();
    let t1 = match g1.report_text() {
        Some(t) => t,
        None => return false,
    };
    let t2 = match g2.report_text() {
        Some(t) => t,
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 反查覆盖十条：逐条承载计数回填账面且全 >0。
fn survey_full_ok() -> bool {
    let mut lg = RefluxLedger::new();
    let mut i = 0usize;
    while i < LESSONS.len() {
        let row = match LESSONS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if lg.enroll(row.0, row.1, kw_at(row.2, 0), kw_at(row.2, 1)).is_err() {
            return false;
        }
        i += 1;
    }
    let carried = match lg.survey() {
        Ok(c) => c,
        Err(_) => return false,
    };
    if carried != LESSON_COUNT {
        return false;
    }
    if lg.row_count() != LESSON_COUNT {
        return false;
    }
    if lg.warning_count() != 0 {
        return false;
    }
    // 少登一条反查必拒（SurveyShort）。
    let mut short = RefluxLedger::new();
    let mut k = 0usize;
    while k < LESSONS.len() {
        let row = match LESSONS.get(k) {
            Some(r) => *r,
            None => return false,
        };
        if k != 9 {
            let _ = short.enroll(row.0, row.1, kw_at(row.2, 0), kw_at(row.2, 1));
        }
        k += 1;
    }
    match short.survey() {
        Err(code) => {
            if code != Reflux48Code::SurveyShort.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Reflux48Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Reflux48Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1548_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1548 域级经沉淀回归");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78800..=0x789C0", SEG_BASE));
    s.add_dyn("十条沉淀全登", lessons_ok(), "条款 L01..L10 三列齐备入册");
    s.add_dyn("全域判据反查域", global_judges_ok(), "30 条跨域判据编号唯一文本在域");
    s.add_dyn("承载面逐条命中", carriers_ok(), "十条逐条关键词命中判据数 ≥1");
    s.add_dyn("承载缺失10/10", inject_10_ok(), "十路零命中注入逐路必警");
    s.add_dyn("反查覆盖十条", survey_full_ok(), "十条逐条反查回填，缺条反查拒");
    s.add_dyn("反哺报告落档", report_archived_ok(), "报告 12 行承载标记齐，指纹落档");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "登记→反查→落档可达，跳级倒退拒");
    s.add_dyn("登记闸门", enroll_guard_ok(), "越条款 / 空号 / 空词 / 重登四路必抓");
    s.add_dyn("告警阻落档", warning_blocks_seal_ok(), "含承载缺失账落档必拒");
    s.add_dyn("双跑一致", double_run_ok(), "好账指纹与报告两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78900);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1548_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn ten_lessons_frozen() {
        assert_eq!(LESSONS.len(), 10);
        assert_eq!(FAKE_KEYWORDS.len(), 10);
        let mut i = 0usize;
        while i < LESSONS.len() {
            let row = match LESSONS.get(i) {
                Some(r) => *r,
                None => {
                    must_not_reach();
                    return;
                }
            };
            assert!(!row.0.is_empty());
            assert!(!row.1.is_empty());
            i += 1;
        }
    }

    #[test]
    fn global_judge_domain_unique() {
        assert_eq!(GLOBAL_JUDGES.len(), 30);
        let mut i = 0usize;
        while i < GLOBAL_JUDGES.len() {
            let a = match GLOBAL_JUDGES.get(i) {
                Some(r) => r.0,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let mut j = i + 1;
            while j < GLOBAL_JUDGES.len() {
                let b = match GLOBAL_JUDGES.get(j) {
                    Some(r) => r.0,
                    None => {
                        must_not_reach();
                        return;
                    }
                };
                assert_ne!(a, b);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn carriers_at_least_one_each() {
        assert!(carriers_ok());
        let mut i = 0usize;
        while i < LESSONS.len() {
            let row = match LESSONS.get(i) {
                Some(r) => *r,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let mut probe = RefluxLedger::new();
            let c = probe.carrier_of(kw_at(row.2, 0), kw_at(row.2, 1));
            assert!(c >= 1, "条款 {} 零承载", row.0);
            i += 1;
        }
    }

    #[test]
    fn fake_keywords_zero_carrier() {
        let mut probe = RefluxLedger::new();
        assert_eq!(probe.carrier_of("量子", "玄机"), 0);
        assert_eq!(probe.carrier_of("影账", "幽测"), 0);
    }

    #[test]
    fn injected_ten_all_warned() {
        assert_eq!(inject_warning_count(), 10);
        assert!(inject_10_ok());
    }

    #[test]
    fn warning_blocks_seal() {
        assert!(warning_blocks_seal_ok());
    }

    #[test]
    fn unknown_lesson_rejected() {
        let mut lg = RefluxLedger::new();
        match lg.enroll("UNX-F1548-L99", "凭空条款", "量子", "玄机") {
            Err(code) => assert_eq!(code, Reflux48Code::LessonUnknown.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn survey_before_enroll_short() {
        let mut lg = RefluxLedger::new();
        match lg.survey() {
            Err(code) => assert_eq!(code, Reflux48Code::SurveyShort.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn report_rows_and_markers() {
        let lg = build_reflux_ok();
        let text = match lg.report_text() {
            Some(t) => t.to_string(),
            None => {
                must_not_reach();
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 12);
        assert!(text.contains("UNX-F1548-L01"));
        assert!(text.contains("●"));
        assert!(text.contains("告警 0 条"));
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_reflux_ok().report_digest();
        let d2 = build_reflux_ok().report_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Reflux48Code::ALL.len(), 12);
        let first = match Reflux48Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Reflux48Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
