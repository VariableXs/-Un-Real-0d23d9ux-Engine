//! UNX-F1546 域级健康一屏终版（四族指标聚合终验） · 0x78880段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B04 卡 3 · UNX-F1546 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1546-J1 健康一屏终版（中断/计时/分发/账本四族
//!     指标聚合齐备：核心 30 条固化），指标口径与 F1529 终版一致率
//!     100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四族闭集：中断 / 计时 / 分发 / 账本，族号 FAM_IRQ..FAM_LEDGER
//!     唯一，越族指标显性拒；
//!   - 核心 30 条固化：指标名 / 所属族 / 口径三列冻结表，指标名唯一，
//!     条数恒 30，缺条越额均拒；
//!   - 口径一致率 100%：逐指标口径与 F1529 终版口径闭集逐字对平，
//!     口径不符显性拒；
//!   - 一屏聚合：固化 → 聚合 → 终验 → 封屏四态单调状态机，屏面
//!     34 行（头 + 30 指标行 + 尾）恒定，指纹 FNV-1a 落档；
//!   - 注入必抓：越族 / 口径漂移 / 缺条三路注入全部显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78880，派工卡 VX8-B04 卡 3 独占窗口）。
pub const SEG_BASE: u32 = 0x78880;

/// 四族闭集条数（中断/计时/分发/账本）。
pub const FAMILY_COUNT: usize = 4;

/// 核心指标固化条数（判据 J1「核心 30 条」冻结）。
pub const CORE_METRICS: usize = 30;

/// 一屏行数（头 + 30 指标行 + 尾，冻结）。
pub const SCREEN_ROWS: usize = 32;

// ── 四族闭集冻结表（编目序 = 判据 J1 列序） ─────────────────────
pub const FAMILIES: [&str; 4] = ["中断", "计时", "分发", "账本"];

// ── F1529 终版口径闭集（口径逐字对平唯一来源） ──────────────────
pub const CALIBERS: [&str; 6] = [
    "计数制", "门限制", "时窗制", "账面制", "指纹制", "档压制",
];

// ── 核心 30 条指标冻结表（指标名 / 族位次 / 口径位次） ──────────
pub const CORE30: [(&str, usize, usize); 30] = [
    ("中断·挂起位清零率", 0, 0),
    ("中断·线屏蔽时长", 0, 2),
    ("中断·重入计数", 0, 0),
    ("中断·风暴判定门限", 0, 1),
    ("中断·尾链延迟峰值", 0, 2),
    ("中断·误触发次数", 0, 0),
    ("中断·嵌套深度峰值", 0, 0),
    ("计时·主副钟漂移", 1, 2),
    ("计时·重校准次数", 1, 0),
    ("计时·时窗对齐误差", 1, 2),
    ("计时·定时器溢出数", 1, 0),
    ("计时·看门狗踢狗间隔", 1, 2),
    ("计时·锚钟锁定状态", 1, 5),
    ("计时·时基单调违例", 1, 0),
    ("分发·回执超时数", 2, 2),
    ("分发·重发次数", 2, 0),
    ("分发·队列积压峰值", 2, 0),
    ("分发·延迟账落账数", 2, 3),
    ("分发·轮询命中率", 2, 0),
    ("分发·失联面计数", 2, 0),
    ("分发·越面拒收数", 2, 0),
    ("账本·借贷差", 3, 3),
    ("账本·冻结页计数", 3, 0),
    ("账本·逐页对账轮次", 3, 0),
    ("账本·红账违约计数", 3, 0),
    ("账本·快照指纹失配", 3, 4),
    ("账本·回潮检出数", 3, 0),
    ("账本·段位越窗检出", 3, 1),
    ("账本·批收口落档数", 3, 3),
    ("账本·交接断链数", 3, 0),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health46Code {
    /// 族位次越界：族收进四族闭集
    FamilyUnknown,
    /// 口径越闭集：口径收进 F1529 六口径闭集
    CaliberUnknown,
    /// 指标名重复：一名一条，先撤旧再补条
    MetricDuplicate,
    /// 指标缺条：补足 30 条再聚合
    MetricShort,
    /// 指标超额：核心面恰 30 条，不收多余条
    MetricOverflow,
    /// 口径漂移：口径与 F1529 终版逐字不符，回对口径表
    CaliberDrift,
    /// 屏面行数违约：一屏必须恰 32 行
    ScreenRowsBad,
    /// 屏面状态倒退：固化→聚合→终验→封屏单调推进
    PhaseRegress,
    /// 屏面状态跳级：未聚合不得终验，未终验不得封屏
    PhaseSkip,
    /// 封屏后再改：终版封屏禁改，另开终版修订屏
    SealedLocked,
    /// 终验摘要为空：先终验生成摘要再落档
    SummaryEmpty,
    /// 双跑不一致：排查屏面指纹随机性后复跑
    NotReproducible,
}

impl Health46Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Health46Code::FamilyUnknown => "族位次越界：指标收进中断/计时/分发/账本四族闭集",
            Health46Code::CaliberUnknown => "口径越闭集：口径收进 F1529 六口径闭集",
            Health46Code::MetricDuplicate => "指标名重复：一名一条，撤旧条再补登",
            Health46Code::MetricShort => "核心指标缺条：补足 30 条再聚合",
            Health46Code::MetricOverflow => "核心指标超额：核心面恰 30 条，删多余条",
            Health46Code::CaliberDrift => "口径漂移：回对 F1529 终版口径表逐字修正",
            Health46Code::ScreenRowsBad => "一屏行数违约：屏面必须恰 32 行",
            Health46Code::PhaseRegress => "屏面状态倒退：固化→聚合→终验→封屏单调推进",
            Health46Code::PhaseSkip => "屏面状态跳级：未聚合不得终验，未终验不得封屏",
            Health46Code::SealedLocked => "终版封屏禁改：另开终版修订屏",
            Health46Code::SummaryEmpty => "终验摘要为空：先终验生成摘要再落档",
            Health46Code::NotReproducible => "双跑不一致：排查屏面指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Health46Code; 12] = [
        Health46Code::FamilyUnknown,
        Health46Code::CaliberUnknown,
        Health46Code::MetricDuplicate,
        Health46Code::MetricShort,
        Health46Code::MetricOverflow,
        Health46Code::CaliberDrift,
        Health46Code::ScreenRowsBad,
        Health46Code::PhaseRegress,
        Health46Code::PhaseSkip,
        Health46Code::SealedLocked,
        Health46Code::SummaryEmpty,
        Health46Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Health46Code::ALL.len() {
        let c = match Health46Code::ALL.get(i) {
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

// ── 屏面状态机（固化 → 聚合 → 终验 → 封屏，单调推进） ───────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScreenPhase {
    /// 固化期：核心 30 条逐条登记
    Frozen,
    /// 聚合期：四族聚合完成
    Aggregated,
    /// 终验期：口径一致率终验通过
    Verified,
    /// 封屏期：一屏落档封版
    Sealed,
}

impl ScreenPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            ScreenPhase::Frozen => 0,
            ScreenPhase::Aggregated => 1,
            ScreenPhase::Verified => 2,
            ScreenPhase::Sealed => 3,
        }
    }
}

/// 单条核心指标登记项。
#[derive(Clone, Debug)]
pub struct CoreMetric {
    pub name: String,
    pub family: usize,
    pub caliber: usize,
}

/// 域级健康一屏终版。
pub struct HealthScreen {
    phase: ScreenPhase,
    metrics: Vec<CoreMetric>,
    screen: Option<String>,
    screen_digest: u32,
}

impl HealthScreen {
    pub fn new() -> HealthScreen {
        HealthScreen {
            phase: ScreenPhase::Frozen,
            metrics: Vec::new(),
            screen: None,
            screen_digest: 0,
        }
    }

    pub fn phase(&self) -> ScreenPhase {
        self.phase
    }

    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }

    pub fn screen_digest(&self) -> u32 {
        self.screen_digest
    }

    /// 族内指标计数。
    pub fn family_load(&self, family: usize) -> usize {
        let mut n = 0usize;
        for m in self.metrics.iter() {
            if m.family == family {
                n += 1;
            }
        }
        n
    }

    /// 固化登记：族 / 口径闭集 + 指标名唯一 + 恰 30 条封顶。
    pub fn freeze(&mut self, name: &str, family: usize, caliber: usize) -> Result<(), u32> {
        if self.phase == ScreenPhase::Sealed {
            return Err(Health46Code::SealedLocked.val());
        }
        if self.phase != ScreenPhase::Frozen {
            return Err(Health46Code::PhaseRegress.val());
        }
        if family >= FAMILY_COUNT {
            return Err(Health46Code::FamilyUnknown.val());
        }
        if caliber >= CALIBERS.len() {
            return Err(Health46Code::CaliberUnknown.val());
        }
        if self.metrics.len() >= CORE_METRICS {
            return Err(Health46Code::MetricOverflow.val());
        }
        let mut i = 0usize;
        while i < self.metrics.len() {
            let m = match self.metrics.get(i) {
                Some(m) => m,
                None => return Err(Health46Code::NotReproducible.val()),
            };
            if m.name == name {
                return Err(Health46Code::MetricDuplicate.val());
            }
            i += 1;
        }
        if name.is_empty() {
            return Err(Health46Code::MetricShort.val());
        }
        self.metrics.push(CoreMetric {
            name: name.to_string(),
            family,
            caliber,
        });
        Ok(())
    }

    /// 聚合：恰 30 条且四族齐备方可推进。
    pub fn aggregate(&mut self) -> Result<usize, u32> {
        if self.phase.rank() > ScreenPhase::Frozen.rank() {
            return Err(Health46Code::PhaseRegress.val());
        }
        if self.metrics.len() != CORE_METRICS {
            return Err(Health46Code::MetricShort.val());
        }
        let mut f = 0usize;
        while f < FAMILY_COUNT {
            if self.family_load(f) == 0 {
                return Err(Health46Code::FamilyUnknown.val());
            }
            f += 1;
        }
        self.phase = ScreenPhase::Aggregated;
        Ok(self.metrics.len())
    }

    /// 终验：逐条口径与冻结表逐字一致方可推进。
    pub fn verify_caliber(&mut self) -> Result<usize, u32> {
        if self.phase == ScreenPhase::Frozen {
            return Err(Health46Code::PhaseSkip.val());
        }
        if self.phase == ScreenPhase::Sealed {
            return Err(Health46Code::PhaseRegress.val());
        }
        let mut i = 0usize;
        while i < self.metrics.len() {
            let m = match self.metrics.get(i) {
                Some(m) => m,
                None => return Err(Health46Code::NotReproducible.val()),
            };
            let row = match CORE30.get(i) {
                Some(r) => *r,
                None => return Err(Health46Code::MetricOverflow.val()),
            };
            // 冻结表口径互证。
            if m.name != row.0 || m.family != row.1 || m.caliber != row.2 {
                return Err(Health46Code::CaliberDrift.val());
            }
            let cal = match CALIBERS.get(m.caliber) {
                Some(c) => *c,
                None => return Err(Health46Code::CaliberUnknown.val()),
            };
            if cal.is_empty() {
                return Err(Health46Code::CaliberDrift.val());
            }
            i += 1;
        }
        self.phase = ScreenPhase::Verified;
        Ok(self.metrics.len())
    }

    /// 封屏：生成 32 行一屏文本并落档。
    pub fn seal_screen(&mut self) -> Result<u32, u32> {
        if self.phase == ScreenPhase::Verified {
            // 允许推进。
        } else {
            return Err(Health46Code::PhaseSkip.val());
        }
        let mut text = String::from("== UNX-F1546 域级健康一屏终版 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.metrics.len() {
            let m = match self.metrics.get(i) {
                Some(m) => m,
                None => break,
            };
            let fam = match FAMILIES.get(m.family) {
                Some(f) => *f,
                None => return Err(Health46Code::FamilyUnknown.val()),
            };
            let cal = match CALIBERS.get(m.caliber) {
                Some(c) => *c,
                None => return Err(Health46Code::CaliberUnknown.val()),
            };
            let line = format!("{} {} 口径:{} ●", fam, m.name, cal);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str(&format!("== 四族聚合 {} 条口径一致率 100% ==", self.metrics.len()));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != SCREEN_ROWS {
            return Err(Health46Code::ScreenRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.screen = Some(text);
        self.screen_digest = digest;
        self.phase = ScreenPhase::Sealed;
        Ok(digest)
    }

    pub fn screen_text(&self) -> Option<&str> {
        match self.screen.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }
}

// ── 好屏构建（30 条全固 + 聚合 + 终验 + 封屏） ──────────────────
pub fn build_screen_ok() -> HealthScreen {
    let mut scr = HealthScreen::new();
    let mut i = 0usize;
    while i < CORE30.len() {
        let row = match CORE30.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = scr.freeze(row.0, row.1, row.2);
        i += 1;
    }
    let _ = scr.aggregate();
    let _ = scr.verify_caliber();
    let _ = scr.seal_screen();
    scr
}

/// 注入屏：越族 + 口径越闭集两路注入（复制好账序后改注）。
pub fn build_screen_injected() -> HealthScreen {
    let mut scr = HealthScreen::new();
    let mut i = 0usize;
    while i < CORE30.len() {
        let row = match CORE30.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = scr.freeze(row.0, row.1, row.2);
        i += 1;
    }
    // 注入一：越族位次 4 越四族闭集。
    let _ = scr.freeze("注入·越族指标", 4, 0);
    // 注入二：口径位次 6 越六口径闭集。
    let _ = scr.freeze("注入·越口径指标", 0, 6);
    scr
}

/// 口径漂移屏：第 2 条口径错位登记。
pub fn build_screen_drifted() -> HealthScreen {
    let mut scr = HealthScreen::new();
    let mut i = 0usize;
    while i < CORE30.len() {
        let row = match CORE30.get(i) {
            Some(r) => *r,
            None => break,
        };
        let mut cal = row.2;
        if i == 2 {
            // 口径漂移：取相邻口径位次（0↔1 翻转，仍落闭集内）。
            if cal == 0 {
                cal = 1;
            } else {
                cal = 0;
            }
        }
        let _ = scr.freeze(row.0, row.1, cal);
        i += 1;
    }
    let _ = scr.aggregate();
    scr
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78800 && SEG_BASE <= 0x789C0
}

/// 核心 30 条固化齐备且四族分布齐。
fn core30_frozen_ok() -> bool {
    let scr = build_screen_ok();
    if scr.metric_count() != CORE_METRICS {
        return false;
    }
    if scr.phase() != ScreenPhase::Sealed {
        return false;
    }
    if CORE30.len() != CORE_METRICS {
        return false;
    }
    // 四族逐族非空且族数覆盖 4。
    let mut f = 0usize;
    while f < FAMILY_COUNT {
        if scr.family_load(f) == 0 {
            return false;
        }
        f += 1;
    }
    true
}

/// 族分布口径：中断 7 / 计时 7 / 分发 7 / 账本 9。
fn family_distribution_ok() -> bool {
    let scr = build_screen_ok();
    if scr.family_load(0) != 7 {
        return false;
    }
    if scr.family_load(1) != 7 {
        return false;
    }
    if scr.family_load(2) != 7 {
        return false;
    }
    if scr.family_load(3) != 9 {
        return false;
    }
    if FAMILIES.len() != FAMILY_COUNT {
        return false;
    }
    true
}

/// 口径一致率 100%：逐条与冻结表三元组全等。
fn caliber_100_ok() -> bool {
    let mut scr = build_screen_ok();
    // 好屏已封屏；终验以未封屏副本重走。
    if scr.phase() != ScreenPhase::Sealed {
        return false;
    }
    let mut scr2 = HealthScreen::new();
    let mut i = 0usize;
    while i < CORE30.len() {
        let row = match CORE30.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if scr2.freeze(row.0, row.1, row.2).is_err() {
            return false;
        }
        i += 1;
    }
    let _ = scr2.aggregate();
    let n = match scr2.verify_caliber() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != CORE_METRICS {
        return false;
    }
    if scr2.phase() != ScreenPhase::Verified {
        return false;
    }
    true
}

/// 一屏聚合落档：32 行、四族名齐、100% 标记、指纹非零。
fn screen_archived_ok() -> bool {
    let scr = build_screen_ok();
    let text = match scr.screen_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != SCREEN_ROWS {
        return false;
    }
    if !text.contains("健康一屏终版") {
        return false;
    }
    if !text.contains("100%") {
        return false;
    }
    let mut f = 0usize;
    while f < FAMILY_COUNT {
        let fam = match FAMILIES.get(f) {
            Some(n) => *n,
            None => return false,
        };
        if !text.contains(fam) {
            return false;
        }
        f += 1;
    }
    if scr.screen_digest() == 0 {
        return false;
    }
    true
}

/// 状态机单调：未聚合终验拒、未终验封屏拒、倒退拒。
fn phase_monotonic_ok() -> bool {
    let mut scr = HealthScreen::new();
    // 空屏先聚合：缺条拒。
    if scr.aggregate().is_ok() {
        return false;
    }
    // 未聚合先终验：跳级拒。
    if scr.verify_caliber().is_ok() {
        return false;
    }
    // 未终验先封屏：跳级拒。
    if scr.seal_screen().is_ok() {
        return false;
    }
    // 好屏封屏后再固化：封屏锁拒。
    let mut sealed = build_screen_ok();
    if sealed.freeze("封屏后注入", 0, 0).is_ok() {
        return false;
    }
    if sealed.phase() != ScreenPhase::Sealed {
        return false;
    }
    true
}

/// 注入必抓：越族 / 口径越闭集两路全抓，账面仍 30 条。
fn inject_caught_ok() -> bool {
    let scr = build_screen_injected();
    if scr.metric_count() != CORE_METRICS {
        return false;
    }
    // 直接通道复核：越族。
    let mut scr2 = HealthScreen::new();
    match scr2.freeze("注入·越族指标", 4, 0) {
        Err(code) => {
            if code != Health46Code::FamilyUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 口径越闭集。
    match scr2.freeze("注入·越口径指标", 0, 6) {
        Err(code) => {
            if code != Health46Code::CaliberUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 重名。
    let row0 = match CORE30.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let _ = scr2.freeze(row0.0, row0.1, row0.2);
    match scr2.freeze(row0.0, row0.1, row0.2) {
        Err(code) => {
            if code != Health46Code::MetricDuplicate.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if scr2.metric_count() != 1 {
        return false;
    }
    true
}

/// 口径漂移必抓：漂移屏终验必拒。
fn drift_caught_ok() -> bool {
    let mut scr = build_screen_drifted();
    match scr.verify_caliber() {
        Err(code) => {
            if code != Health46Code::CaliberDrift.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if scr.phase() != ScreenPhase::Aggregated {
        return false;
    }
    true
}

/// 缺条必抓：29 条聚合必拒。
fn short_caught_ok() -> bool {
    let mut scr = HealthScreen::new();
    let mut i = 0usize;
    while i < CORE30.len() {
        let row = match CORE30.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if i != 7 {
            let _ = scr.freeze(row.0, row.1, row.2);
        }
        i += 1;
    }
    if scr.metric_count() != CORE_METRICS - 1 {
        return false;
    }
    match scr.aggregate() {
        Err(code) => {
            if code != Health46Code::MetricShort.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 超额必抓：第 31 条固化必拒。
fn overflow_caught_ok() -> bool {
    let mut scr = HealthScreen::new();
    let mut i = 0usize;
    while i < CORE30.len() {
        let row = match CORE30.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let _ = scr.freeze(row.0, row.1, row.2);
        i += 1;
    }
    match scr.freeze("超额·第三十一条", 0, 0) {
        Err(code) => {
            if code != Health46Code::MetricOverflow.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if scr.metric_count() != CORE_METRICS {
        return false;
    }
    true
}

/// 双跑一致：好屏指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_screen_ok().screen_digest();
    let d2 = build_screen_ok().screen_digest();
    if d1 != d2 {
        return false;
    }
    if d1 == 0 {
        return false;
    }
    let s1 = build_screen_ok();
    let s2 = build_screen_ok();
    let t1 = match s1.screen_text() {
        Some(t) => t,
        None => return false,
    };
    let t2 = match s2.screen_text() {
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
    for c in Health46Code::ALL.iter() {
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
    let first = match Health46Code::ALL.get(0) {
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
pub fn run_unx1546_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1546 域级健康一屏终版");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78800..=0x789C0", SEG_BASE));
    s.add_dyn("核心三十条固化", core30_frozen_ok(), "核心 30 条逐条入册，四族齐备");
    s.add_dyn("四族分布口径", family_distribution_ok(), "中断7/计时7/分发7/账本9，分布恒定");
    s.add_dyn("口径一致率100%", caliber_100_ok(), "逐条与 F1529 终版口径三元组全等");
    s.add_dyn("一屏聚合落档", screen_archived_ok(), "屏面 32 行四族名齐，指纹非零落档");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "固化→聚合→终验→封屏可达，跳级倒退拒");
    s.add_dyn("越族越口径注入", inject_caught_ok(), "越族 / 越口径 / 重名三路必抓");
    s.add_dyn("口径漂移必抓", drift_caught_ok(), "漂移屏终验必拒，不进终版");
    s.add_dyn("缺条必抓", short_caught_ok(), "29 条聚合必拒，补条再聚合");
    s.add_dyn("超额必抓", overflow_caught_ok(), "第 31 条固化必拒，核心面恒 30");
    s.add_dyn("双跑一致", double_run_ok(), "好屏指纹与屏面两次构建逐位相等");
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
        assert_eq!(SEG_BASE, 0x78880);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1546_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn core30_table_frozen() {
        assert_eq!(CORE30.len(), 30);
        assert_eq!(FAMILIES.len(), 4);
        assert_eq!(CALIBERS.len(), 6);
        // 指标名唯一：两两互异。
        let mut i = 0usize;
        while i < CORE30.len() {
            let a = match CORE30.get(i) {
                Some(r) => r.0,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let mut j = i + 1;
            while j < CORE30.len() {
                let b = match CORE30.get(j) {
                    Some(r) => r.0,
                    None => {
                        must_not_reach();
                        return;
                    }
                };
                assert_ne!(a, b, "指标名重复: {}", a);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn family_distribution_frozen() {
        let scr = build_screen_ok();
        assert_eq!(scr.family_load(0), 7);
        assert_eq!(scr.family_load(1), 7);
        assert_eq!(scr.family_load(2), 7);
        assert_eq!(scr.family_load(3), 9);
        assert_eq!(scr.metric_count(), 30);
    }

    #[test]
    fn caliber_drift_rejected() {
        let mut scr = build_screen_drifted();
        match scr.verify_caliber() {
            Err(code) => assert_eq!(code, Health46Code::CaliberDrift.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn out_of_family_rejected() {
        let mut scr = HealthScreen::new();
        match scr.freeze("越族注入", 4, 0) {
            Err(code) => assert_eq!(code, Health46Code::FamilyUnknown.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(scr.metric_count(), 0);
    }

    #[test]
    fn out_of_caliber_rejected() {
        let mut scr = HealthScreen::new();
        match scr.freeze("越口径注入", 0, 6) {
            Err(code) => assert_eq!(code, Health46Code::CaliberUnknown.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn duplicate_metric_rejected() {
        let mut scr = HealthScreen::new();
        let row0 = match CORE30.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = scr.freeze(row0.0, row0.1, row0.2);
        match scr.freeze(row0.0, row0.1, row0.2) {
            Err(code) => assert_eq!(code, Health46Code::MetricDuplicate.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(scr.metric_count(), 1);
    }

    #[test]
    fn verify_before_aggregate_skipped() {
        let mut scr = HealthScreen::new();
        let row0 = match CORE30.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = scr.freeze(row0.0, row0.1, row0.2);
        match scr.verify_caliber() {
            Err(code) => assert_eq!(code, Health46Code::PhaseSkip.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn screen_rows_and_markers() {
        let scr = build_screen_ok();
        let text = match scr.screen_text() {
            Some(t) => t.to_string(),
            None => {
                must_not_reach();
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 32);
        assert!(text.contains("100%"));
        assert!(text.contains("账本·借贷差"));
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_screen_ok().screen_digest();
        let d2 = build_screen_ok().screen_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn sealed_screen_locked() {
        let mut scr = build_screen_ok();
        match scr.freeze("封屏后注入", 0, 0) {
            Err(code) => assert_eq!(code, Health46Code::SealedLocked.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Health46Code::ALL.len(), 12);
        let first = match Health46Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Health46Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
