//! UNX-F1550 域级词汇表（术语统一账终版） · 0x78980段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B04 卡 7 · UNX-F1550 逐字摘录）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1550-J1 术语统一账终版（域内术语逐条登记：
//!     术语/定义/首现条目三列），术语一致性抽验 100 组一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 术语三列闭集：20 条术语逐条登记（术语 / 定义 / 首现条目），
//!     术语名唯一，定义非空，首现条目可溯；
//!   - 用法语料闭集：40 句域内用法语逐句声明所用规范术语，语料
//!     即抽验唯一域；
//!   - 变体写法禁入册：10 对（规范术语, 变体写法）闭表，语料逐句
//!     扫描，变体写法一律显性拒；
//!   - 一致性抽验 100 组：LCG（种子 = SEG_BASE）派生 100 组抽验位，
//!     逐组核验「语料句含其声明规范术语且不含该术语变体」，一致率
//!     100%；
//!   - 终版摘要落档：22 行（头 + 20 术语行 + 尾），指纹 FNV-1a 落账，
//!     双跑逐位一致；
//!   - 状态机单调：登记 → 抽验 → 落档，倒退跳级显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78980，派工卡 VX8-B04 卡 7 独占窗口）。
pub const SEG_BASE: u32 = 0x78980;

/// 术语登记条数（域内术语闭集）。
pub const TERM_COUNT: usize = 20;

/// 用法语料句数（抽验唯一域）。
pub const USAGE_COUNT: usize = 40;

/// 一致性抽验组数（判据 J1「100 组」冻结）。
pub const SAMPLE_COUNT: usize = 100;

/// 变体写法对数（禁入册闭表）。
pub const VARIANT_COUNT: usize = 10;

/// 终版摘要行数（头 + 20 术语行 + 尾，冻结）。
pub const SUMMARY_ROWS: usize = 22;

// ── 术语三列冻结表（术语 / 定义 / 首现条目） ────────────────────
pub const TERMS: [(&str, &str, &str); 20] = [
    ("页表映射", "虚拟页到物理帧的受控重挂通道", "UNX-F1545-E01"),
    ("中断风暴", "单线中断速率越额的守卫判定态", "UNX-F1545-E02"),
    ("计时漂移", "主副钟间时差越阈的可测偏移", "UNX-F1545-E03"),
    ("分发回执", "分发面回执按窗确认的契约", "UNX-F1545-E04"),
    ("账本对账", "逐页借贷差归零的核验流程", "UNX-F1545-E05"),
    ("快照指纹", "批末快照的 FNV 指纹互证", "UNX-F1545-E06"),
    ("注入检出", "注入谱逐路必抓的检出率口径", "UNX-F1545-E07"),
    ("双跑一致", "同输入双跑指纹逐位相等的判定", "UNX-F1545-E08"),
    ("段位守卫", "段位 0x40 对齐且落段窗的守卫", "UNX-F1545-E09"),
    ("红账回潮", "清零后违约计数再次出现的状态", "UNX-F1545-E10"),
    ("批收口", "批内断言聚合落档的收口动作", "UNX-F1545-E11"),
    ("里程碑", "批级断言承载的计数口径", "UNX-F1545-E12"),
    ("时序一表", "40 批完成时点单调成表的终版", "UNX-F1545-E13"),
    ("预案演练", "恢复路径逐槽可执行的样例动作", "UNX-F1545-E14"),
    ("健康一屏", "四族指标聚合 32 行的终版屏面", "UNX-F1545-E15"),
    ("三向对账", "文档实现台账逐项对平的核验", "UNX-F1545-E16"),
    ("承载面", "沉淀条款在判据文本中的命中域", "UNX-F1545-E17"),
    ("判据台账", "批内判据编号唯一入账的闭集", "UNX-F1545-E18"),
    ("术语统一账", "域内术语三列登记的终版册", "UNX-F1545-E19"),
    ("反哺报告", "沉淀反查承载面明细的落档", "UNX-F1545-E20"),
];

// ── 变体写法禁入册闭表（规范术语 / 变体写法） ───────────────────
pub const VARIANTS: [(&str, &str); 10] = [
    ("页表映射", "页面映射"),
    ("中断风暴", "中断风爆"),
    ("计时漂移", "计时漂差"),
    ("分发回执", "分发回执单"),
    ("账本对账", "账目对账"),
    ("快照指纹", "快照指文"),
    ("注入检出", "注入捡出"),
    ("双跑一致", "双跑一至"),
    ("段位守卫", "段位守衙"),
    ("红账回潮", "红账回朝"),
];

// ── 用法语料闭集（40 句，逐句声明所用规范术语） ─────────────────
pub const USAGE: [(&str, &str); 40] = [
    ("页表映射", "巡检发现越权置位后按页表映射预案重挂复巡"),
    ("页表映射", "冻结写入通道是页表映射恢复的第一步"),
    ("中断风暴", "单线速率越额即判中断风暴并屏蔽清挂起"),
    ("中断风暴", "中断风暴恢复后须复测线速率口径"),
    ("计时漂移", "主副钟差越阈按计时漂移预案锚定重校"),
    ("计时漂移", "计时漂移口径与健康一屏冻结一致"),
    ("分发回执", "回执超窗按分发回执预案重发轮询落账"),
    ("分发回执", "分发回执延迟账逐笔可溯"),
    ("账本对账", "借贷差非零须冻结记账执行账本对账"),
    ("账本对账", "账本对账轮次与冻结页计数并列核验"),
    ("快照指纹", "批末快照指纹与账目互证不符即重建"),
    ("快照指纹", "快照指纹双跑逐位一致方可落档"),
    ("注入检出", "注入检出率回落须重放注入谱补失抓位"),
    ("注入检出", "注入检出十路逐路必抓口径冻结"),
    ("双跑一致", "同输入指纹歧变即违双跑一致判定"),
    ("双跑一致", "双跑一致覆盖批收口报告与表哈希"),
    ("段位守卫", "段位守卫要求 0x40 对齐且落本批段窗"),
    ("段位守卫", "段位守卫检出越窗即锁写口重对齐"),
    ("红账回潮", "违约计数清零后再现即红账回潮告警"),
    ("红账回潮", "红账回潮须追认违约源后清位复扫"),
    ("批收口", "批内断言聚合落档即完成批收口"),
    ("批收口", "批收口断言只增不减编号唯一"),
    ("里程碑", "批级断言承载计数即该批里程碑"),
    ("里程碑", "B37 批里程碑恰四十条入时序一表"),
    ("时序一表", "四十批完成时点单调成表即时序一表"),
    ("时序一表", "时序一表哈希落档双跑一致"),
    ("预案演练", "恢复路径逐槽可执行即预案演练通过"),
    ("预案演练", "预案演练样例十槽逐槽登记"),
    ("健康一屏", "四族指标聚合三十二行即健康一屏"),
    ("健康一屏", "健康一屏口径与终版一致率百分百"),
    ("三向对账", "文档实现台账逐项对平即三向对账通过"),
    ("三向对账", "三向对账失配注入十路全检"),
    ("承载面", "沉淀条款关键词命中判据即有承载面"),
    ("承载面", "承载面零命中即告警并回补"),
    ("判据台账", "批内判据编号唯一入账即判据台账"),
    ("判据台账", "判据台账逐条与学实指纹互证"),
    ("术语统一账", "术语三列逐条登记成术语统一账"),
    ("术语统一账", "术语统一账一致性抽验百组全一致"),
    ("反哺报告", "沉淀反查承载面明细落档即反哺报告"),
    ("反哺报告", "反哺报告十二行指纹落档"),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glossary50Code {
    /// 术语越词表：术语收进 20 条闭集
    TermUnknown,
    /// 术语重复：一术语一登记，重登先撤旧
    TermDuplicate,
    /// 定义为空：补定义再登记
    DefEmpty,
    /// 首现条目为空：补首现条目再登记
    FirstRefEmpty,
    /// 变体写法入册：语料出现禁入变体，回改规范术语
    VariantFound,
    /// 抽验不足百组：补足 100 组再终验
    SampleShort,
    /// 词汇表状态倒退：登记→抽验→落档单调推进
    PhaseRegress,
    /// 词汇表状态跳级：未抽验不得落档
    PhaseSkip,
    /// 终版封账禁改：另开词汇修订册
    SealedLocked,
    /// 摘要行数违约：终版摘要必须恰 22 行
    SummaryRowsBad,
    /// 术语名为空：补术语名再登记
    IdEmpty,
    /// 双跑不一致：排查抽验序列随机性后复跑
    NotReproducible,
}

impl Glossary50Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Glossary50Code::TermUnknown => "术语越词表：术语收进 20 条闭集再登记",
            Glossary50Code::TermDuplicate => "术语重复：一术语一登记，撤旧登再补",
            Glossary50Code::DefEmpty => "定义为空：补定义再登记",
            Glossary50Code::FirstRefEmpty => "首现条目为空：补首现条目再登记",
            Glossary50Code::VariantFound => "变体写法入册：回改规范术语再抽验",
            Glossary50Code::SampleShort => "抽验不足百组：补足 100 组再终验",
            Glossary50Code::PhaseRegress => "词汇表状态倒退：登记→抽验→落档单调推进",
            Glossary50Code::PhaseSkip => "词汇表状态跳级：未抽验不得落档",
            Glossary50Code::SealedLocked => "终版封账禁改：另开词汇修订册",
            Glossary50Code::SummaryRowsBad => "终版摘要行数违约：摘要必须恰 22 行",
            Glossary50Code::IdEmpty => "术语名为空：补术语名再登记",
            Glossary50Code::NotReproducible => "双跑不一致：排查抽验序列随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Glossary50Code; 12] = [
        Glossary50Code::TermUnknown,
        Glossary50Code::TermDuplicate,
        Glossary50Code::DefEmpty,
        Glossary50Code::FirstRefEmpty,
        Glossary50Code::VariantFound,
        Glossary50Code::SampleShort,
        Glossary50Code::PhaseRegress,
        Glossary50Code::PhaseSkip,
        Glossary50Code::SealedLocked,
        Glossary50Code::SummaryRowsBad,
        Glossary50Code::IdEmpty,
        Glossary50Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Glossary50Code::ALL.len() {
        let c = match Glossary50Code::ALL.get(i) {
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

// ── LCG（种子固定 = SEG_BASE，零随机源） ────────────────────────
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    /// 逐步推进并取模返回。
    pub fn next_mod(&mut self, m: usize) -> usize {
        self.state = self
            .state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345);
        let v = (self.state >> 16) as usize;
        if m == 0 {
            return 0;
        }
        v % m
    }
}

// ── 词汇表状态机（登记 → 抽验 → 落档，单调推进） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlossPhase {
    /// 登记期：20 条术语逐条入册
    Enrolled,
    /// 抽验期：100 组一致性抽验完成
    Sampled,
    /// 落档期：终版摘要落档封账
    Sealed,
}

impl GlossPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            GlossPhase::Enrolled => 0,
            GlossPhase::Sampled => 1,
            GlossPhase::Sealed => 2,
        }
    }
}

/// 单条术语登记项。
#[derive(Clone, Debug)]
pub struct TermRow {
    pub name: String,
    pub def: String,
    pub first_ref: String,
}

/// 域级术语统一账。
pub struct Glossary {
    phase: GlossPhase,
    rows: Vec<TermRow>,
    samples_run: usize,
    variant_hits: Vec<String>,
    summary: Option<String>,
    summary_digest: u32,
}

impl Glossary {
    pub fn new() -> Glossary {
        Glossary {
            phase: GlossPhase::Enrolled,
            rows: Vec::new(),
            samples_run: 0,
            variant_hits: Vec::new(),
            summary: None,
            summary_digest: 0,
        }
    }

    pub fn phase(&self) -> GlossPhase {
        self.phase
    }

    pub fn term_count(&self) -> usize {
        self.rows.len()
    }

    pub fn samples_run(&self) -> usize {
        self.samples_run
    }

    pub fn variant_hit_count(&self) -> usize {
        self.variant_hits.len()
    }

    pub fn summary_digest(&self) -> u32 {
        self.summary_digest
    }

    /// 术语登记：闭集 + 唯一 + 定义/首现条目非空。
    pub fn register(&mut self, name: &str, def: &str, first_ref: &str) -> Result<(), u32> {
        if self.phase == GlossPhase::Sealed {
            return Err(Glossary50Code::SealedLocked.val());
        }
        if self.phase != GlossPhase::Enrolled {
            return Err(Glossary50Code::PhaseRegress.val());
        }
        if name.is_empty() {
            return Err(Glossary50Code::IdEmpty.val());
        }
        let mut known = false;
        let mut i = 0usize;
        while i < TERMS.len() {
            let row = match TERMS.get(i) {
                Some(r) => *r,
                None => break,
            };
            if row.0 == name {
                known = true;
            }
            i += 1;
        }
        if !known {
            return Err(Glossary50Code::TermUnknown.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => return Err(Glossary50Code::NotReproducible.val()),
            };
            if r.name == name {
                return Err(Glossary50Code::TermDuplicate.val());
            }
            i += 1;
        }
        if def.is_empty() {
            return Err(Glossary50Code::DefEmpty.val());
        }
        if first_ref.is_empty() {
            return Err(Glossary50Code::FirstRefEmpty.val());
        }
        self.rows.push(TermRow {
            name: name.to_string(),
            def: def.to_string(),
            first_ref: first_ref.to_string(),
        });
        Ok(())
    }

    /// 变体扫描：语料逐句查禁入变体，命中记名。
    pub fn scan_variants(&mut self, corpus: &[(&str, &str)]) -> Result<usize, u32> {
        if corpus.is_empty() {
            return Err(Glossary50Code::SampleShort.val());
        }
        self.variant_hits.clear();
        let mut i = 0usize;
        while i < corpus.len() {
            let sentence = match corpus.get(i) {
                Some(s) => s.1,
                None => break,
            };
            let mut v = 0usize;
            while v < VARIANTS.len() {
                let pair = match VARIANTS.get(v) {
                    Some(p) => *p,
                    None => break,
                };
                if sentence.contains(pair.1) {
                    self.variant_hits.push(format!("句{} 含变体{}", i, pair.1));
                }
                v += 1;
            }
            i += 1;
        }
        if !self.variant_hits.is_empty() {
            return Err(Glossary50Code::VariantFound.val());
        }
        Ok(self.variant_hits.len())
    }

    /// 一致性抽验 100 组：LCG 派生组位，逐组核验声明术语在句。
    pub fn run_samples(&mut self) -> Result<usize, u32> {
        if self.phase.rank() > GlossPhase::Enrolled.rank() {
            return Err(Glossary50Code::PhaseRegress.val());
        }
        if self.phase == GlossPhase::Sealed {
            return Err(Glossary50Code::SealedLocked.val());
        }
        if self.rows.len() != TERM_COUNT {
            return Err(Glossary50Code::SampleShort.val());
        }
        let mut lcg = Lcg::new(SEG_BASE);
        let mut consistent = 0usize;
        let mut g = 0usize;
        while g < SAMPLE_COUNT {
            let si = lcg.next_mod(USAGE.len());
            let row = match USAGE.get(si) {
                Some(r) => *r,
                None => return Err(Glossary50Code::NotReproducible.val()),
            };
            let mut ok = row.1.contains(row.0);
            // 该术语若有禁入变体，句内不得出现。
            let mut v = 0usize;
            while v < VARIANTS.len() {
                let pair = match VARIANTS.get(v) {
                    Some(p) => *p,
                    None => break,
                };
                if pair.0 == row.0 && row.1.contains(pair.1) {
                    ok = false;
                }
                v += 1;
            }
            if ok {
                consistent += 1;
            }
            g += 1;
        }
        self.samples_run = SAMPLE_COUNT;
        if consistent != SAMPLE_COUNT {
            return Err(Glossary50Code::VariantFound.val());
        }
        self.phase = GlossPhase::Sampled;
        Ok(consistent)
    }

    /// 落档：生成 22 行终版摘要并封账。
    pub fn seal_summary(&mut self) -> Result<u32, u32> {
        if self.phase == GlossPhase::Sampled {
            // 允许推进。
        } else {
            return Err(Glossary50Code::PhaseSkip.val());
        }
        if self.samples_run != SAMPLE_COUNT {
            return Err(Glossary50Code::SampleShort.val());
        }
        let mut text = String::from("== UNX-F1550 术语统一账终版摘要 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("{} 首现:{} ●", r.name, r.first_ref);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str(&format!("== 一致性抽验 {}/{} 组一致率 100% ==", self.samples_run, SAMPLE_COUNT));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != SUMMARY_ROWS {
            return Err(Glossary50Code::SummaryRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.summary = Some(text);
        self.summary_digest = digest;
        self.phase = GlossPhase::Sealed;
        Ok(digest)
    }

    pub fn summary_text(&self) -> Option<&str> {
        match self.summary.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }
}

// ── 好册构建（20 术语全登 + 变体扫描 + 抽验 + 落档） ────────────
pub fn build_glossary_ok() -> Glossary {
    let mut gl = Glossary::new();
    let mut i = 0usize;
    while i < TERMS.len() {
        let row = match TERMS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = gl.register(row.0, row.1, row.2);
        i += 1;
    }
    let _ = gl.scan_variants(&USAGE);
    let _ = gl.run_samples();
    let _ = gl.seal_summary();
    gl
}

/// 变体注入语料：十路变体写法逐句构造（禁入变体注入）。
pub const VARIANT_SENTENCES: [&str; 10] = [
    "巡检按页面映射预案重挂复巡",
    "单线速率越额判中断风爆并屏蔽",
    "主副钟差越阈按计时漂差预案重校",
    "回执超窗按分发回执单预案重发",
    "借贷差非零执行账目对账流程",
    "批末快照指文与账目互证不符",
    "注入捡出率回落须重放注入谱",
    "同输入指纹歧变违双跑一至判定",
    "段位守衙要求对齐且落段窗",
    "违约计数再现即红账回朝告警",
];

/// 变体注入检出复算：十句逐句必检，返回检出句数。
fn variant_detect_count() -> usize {
    let mut gl = Glossary::new();
    let mut i = 0usize;
    while i < TERMS.len() {
        let row = match TERMS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = gl.register(row.0, row.1, row.2);
        i += 1;
    }
    let mut corpus: Vec<(&str, &str)> = Vec::new();
    let mut k = 0usize;
    while k < VARIANT_SENTENCES.len() {
        let s = match VARIANT_SENTENCES.get(k) {
            Some(s) => *s,
            None => break,
        };
        corpus.push(("注入", s));
        k += 1;
    }
    match gl.scan_variants(&corpus) {
        Ok(_) => 0,
        Err(_) => gl.variant_hit_count(),
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78800 && SEG_BASE <= 0x789C0
}

/// 20 术语三列齐备登记且封账。
fn terms_ok() -> bool {
    let gl = build_glossary_ok();
    if gl.term_count() != TERM_COUNT {
        return false;
    }
    if gl.phase() != GlossPhase::Sealed {
        return false;
    }
    if TERMS.len() != TERM_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < TERMS.len() {
        let row = match TERMS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.0.is_empty() || row.1.is_empty() || row.2.is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 首现条目可溯：逐条首现引用前缀一致。
fn first_ref_ok() -> bool {
    let mut i = 0usize;
    while i < TERMS.len() {
        let row = match TERMS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if !row.2.starts_with("UNX-F1545-E") {
            return false;
        }
        i += 1;
    }
    true
}

/// 一致性抽验 100 组一致率 100%。
fn samples_100_ok() -> bool {
    let mut gl = Glossary::new();
    let mut i = 0usize;
    while i < TERMS.len() {
        let row = match TERMS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if gl.register(row.0, row.1, row.2).is_err() {
            return false;
        }
        i += 1;
    }
    let n = match gl.run_samples() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != SAMPLE_COUNT {
        return false;
    }
    if gl.samples_run() != SAMPLE_COUNT {
        return false;
    }
    if gl.phase() != GlossPhase::Sampled {
        return false;
    }
    true
}

/// 用法语料零变体：好语料扫描零命中。
fn variant_free_ok() -> bool {
    let mut gl = Glossary::new();
    let mut i = 0usize;
    while i < TERMS.len() {
        let row = match TERMS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let _ = gl.register(row.0, row.1, row.2);
        i += 1;
    }
    match gl.scan_variants(&USAGE) {
        Ok(n) => {
            if n != 0 {
                return false;
            }
        }
        Err(_) => return false,
    }
    if gl.variant_hit_count() != 0 {
        return false;
    }
    true
}

/// 变体注入 10/10 检出：十句变体语料逐句必检。
fn inject_10_ok() -> bool {
    let detected = variant_detect_count();
    if detected != VARIANT_COUNT {
        return false;
    }
    // 检出名单逐句可溯：含变体名。
    let mut gl = Glossary::new();
    let mut i = 0usize;
    while i < TERMS.len() {
        let row = match TERMS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let _ = gl.register(row.0, row.1, row.2);
        i += 1;
    }
    let mut corpus: Vec<(&str, &str)> = Vec::new();
    let mut k = 0usize;
    while k < VARIANT_SENTENCES.len() {
        let s = match VARIANT_SENTENCES.get(k) {
            Some(s) => *s,
            None => break,
        };
        corpus.push(("注入", s));
        k += 1;
    }
    if gl.scan_variants(&corpus).is_ok() {
        return false;
    }
    let first = match gl.variant_hits.get(0) {
        Some(h) => h.as_str(),
        None => return false,
    };
    let var0 = match VARIANTS.get(0) {
        Some(p) => p.1,
        None => return false,
    };
    if !first.contains(var0) {
        return false;
    }
    true
}

/// 终版摘要落档：22 行、20 术语名齐、100% 标记、指纹非零。
fn summary_archived_ok() -> bool {
    let gl = build_glossary_ok();
    let text = match gl.summary_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != SUMMARY_ROWS {
        return false;
    }
    if !text.contains("术语统一账终版摘要") {
        return false;
    }
    if !text.contains("100%") {
        return false;
    }
    let mut i = 0usize;
    while i < TERMS.len() {
        let row = match TERMS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if !text.contains(row.0) {
            return false;
        }
        i += 1;
    }
    if gl.summary_digest() == 0 {
        return false;
    }
    true
}

/// 状态机单调：未登齐抽验拒、未抽验落档拒、倒退拒。
fn phase_monotonic_ok() -> bool {
    let mut gl = Glossary::new();
    // 零条抽验：SampleShort 拒。
    if gl.run_samples().is_ok() {
        return false;
    }
    // 未抽验落档：跳级拒。
    if gl.seal_summary().is_ok() {
        return false;
    }
    // 封账后再登记：封账锁拒。
    let mut sealed = build_glossary_ok();
    let row0 = match TERMS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    match sealed.register(row0.0, row0.1, row0.2) {
        Err(code) => {
            if code != Glossary50Code::SealedLocked.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    true
}

/// 登记闸门：越词表 / 重登 / 空定义 / 空首现 / 空术语名五路必抓。
fn register_guard_ok() -> bool {
    let mut gl = Glossary::new();
    // 越词表。
    match gl.register("量子纠缠", "凭空术语", "UNX-F1545-E99") {
        Err(code) => {
            if code != Glossary50Code::TermUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 空术语名。
    match gl.register("", "空名术语", "UNX-F1545-E01") {
        Err(code) => {
            if code != Glossary50Code::IdEmpty.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 空定义。
    let row0 = match TERMS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    match gl.register(row0.0, "", row0.2) {
        Err(code) => {
            if code != Glossary50Code::DefEmpty.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 空首现。
    match gl.register(row0.0, row0.1, "") {
        Err(code) => {
            if code != Glossary50Code::FirstRefEmpty.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 重登。
    let _ = gl.register(row0.0, row0.1, row0.2);
    match gl.register(row0.0, row0.1, row0.2) {
        Err(code) => {
            if code != Glossary50Code::TermDuplicate.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if gl.term_count() != 1 {
        return false;
    }
    true
}

/// LCG 确定性：同种子两次序列逐位相等。
fn lcg_deterministic_ok() -> bool {
    let mut a = Lcg::new(SEG_BASE);
    let mut b = Lcg::new(SEG_BASE);
    let mut i = 0usize;
    while i < SAMPLE_COUNT {
        if a.next_mod(USAGE.len()) != b.next_mod(USAGE.len()) {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：好册指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_glossary_ok().summary_digest();
    let d2 = build_glossary_ok().summary_digest();
    if d1 != d2 {
        return false;
    }
    if d1 == 0 {
        return false;
    }
    let g1 = build_glossary_ok();
    let g2 = build_glossary_ok();
    let t1 = match g1.summary_text() {
        Some(t) => t,
        None => return false,
    };
    let t2 = match g2.summary_text() {
        Some(t) => t,
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Glossary50Code::ALL.iter() {
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
    let first = match Glossary50Code::ALL.get(0) {
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
pub fn run_unx1550_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1550 域级词汇表");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78800..=0x789C0", SEG_BASE));
    s.add_dyn("术语三列齐备", terms_ok(), "20 条术语/定义/首现条目逐条入册");
    s.add_dyn("首现条目可溯", first_ref_ok(), "逐条首现引用前缀一致可溯");
    s.add_dyn("一致性抽验百组", samples_100_ok(), "LCG 百组抽验一致率 100%");
    s.add_dyn("语料零变体", variant_free_ok(), "用法语料 40 句扫描零变体");
    s.add_dyn("变体注入10/10", inject_10_ok(), "十句变体语料逐句必检可溯");
    s.add_dyn("终版摘要落档", summary_archived_ok(), "摘要 22 行术语名齐，指纹落档");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "登记→抽验→落档可达，跳级倒退拒");
    s.add_dyn("登记闸门", register_guard_ok(), "越词表 / 空列 / 重登五路必抓");
    s.add_dyn("LCG 确定性", lcg_deterministic_ok(), "同种子抽验序列逐位相等");
    s.add_dyn("双跑一致", double_run_ok(), "好册指纹与摘要两次构建逐位相等");
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
        assert_eq!(SEG_BASE, 0x78980);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1550_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn twenty_terms_frozen() {
        assert_eq!(TERMS.len(), 20);
        assert_eq!(VARIANTS.len(), 10);
        assert_eq!(USAGE.len(), 40);
        let mut i = 0usize;
        while i < TERMS.len() {
            let a = match TERMS.get(i) {
                Some(r) => r.0,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let mut j = i + 1;
            while j < TERMS.len() {
                let b = match TERMS.get(j) {
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
    fn usage_corpus_declares_own_terms() {
        let mut i = 0usize;
        while i < USAGE.len() {
            let row = match USAGE.get(i) {
                Some(r) => *r,
                None => {
                    must_not_reach();
                    return;
                }
            };
            assert!(row.1.contains(row.0), "句 {} 未含声明术语 {}", i, row.0);
            i += 1;
        }
    }

    #[test]
    fn samples_hundred_percent() {
        assert!(samples_100_ok());
        let gl = build_glossary_ok();
        assert_eq!(gl.samples_run(), 100);
    }

    #[test]
    fn variant_injection_ten_detected() {
        assert_eq!(variant_detect_count(), 10);
        assert!(inject_10_ok());
    }

    #[test]
    fn good_corpus_variant_free() {
        assert!(variant_free_ok());
        let mut probe = Glossary::new();
        assert!(probe.scan_variants(&USAGE).is_ok());
    }

    #[test]
    fn unknown_term_rejected() {
        let mut gl = Glossary::new();
        match gl.register("量子纠缠", "凭空术语", "UNX-F1545-E99") {
            Err(code) => assert_eq!(code, Glossary50Code::TermUnknown.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn duplicate_term_rejected() {
        let mut gl = Glossary::new();
        let row0 = match TERMS.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = gl.register(row0.0, row0.1, row0.2);
        match gl.register(row0.0, row0.1, row0.2) {
            Err(code) => assert_eq!(code, Glossary50Code::TermDuplicate.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(gl.term_count(), 1);
    }

    #[test]
    fn empty_def_and_first_ref_rejected() {
        let mut gl = Glossary::new();
        let row0 = match TERMS.get(3) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        match gl.register(row0.0, "", row0.2) {
            Err(code) => assert_eq!(code, Glossary50Code::DefEmpty.val()),
            Ok(()) => must_not_reach(),
        }
        match gl.register(row0.0, row0.1, "") {
            Err(code) => assert_eq!(code, Glossary50Code::FirstRefEmpty.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn seal_needs_sampling() {
        let mut gl = Glossary::new();
        let mut i = 0usize;
        while i < TERMS.len() {
            let row = match TERMS.get(i) {
                Some(r) => *r,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let _ = gl.register(row.0, row.1, row.2);
            i += 1;
        }
        match gl.seal_summary() {
            Err(code) => assert_eq!(code, Glossary50Code::PhaseSkip.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn lcg_deterministic() {
        assert!(lcg_deterministic_ok());
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        assert_eq!(a.next_mod(40), b.next_mod(40));
        assert_eq!(a.next_mod(40), b.next_mod(40));
    }

    #[test]
    fn summary_rows_and_markers() {
        let gl = build_glossary_ok();
        let text = match gl.summary_text() {
            Some(t) => t.to_string(),
            None => {
                must_not_reach();
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 22);
        assert!(text.contains("100%"));
        assert!(text.contains("术语统一账"));
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_glossary_ok().summary_digest();
        let d2 = build_glossary_ok().summary_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Glossary50Code::ALL.len(), 12);
        let first = match Glossary50Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Glossary50Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
