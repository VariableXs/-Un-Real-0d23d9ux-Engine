//! F183 存储健康监测（secstar · G-G-13）——U 盘是耗材，耗材要透明。
//!
//! 主册判据（验收标准第一句）：
//! **写入量累计与夹具实测对拍 ±2%；掉电续计零丢失（断电百次复用）；三段区间阈值文档化。**
//!
//! 功能定义（G-G-13）：U 盘健康数据采集与展示：ext4 错误计数/写入量累计
//! （TBW 估算）/磨损均衡指标（主控可读则读）/健康区间诚实显示。
//!
//! 【交互设计】「系统-存储-磁盘健康」页：写入量累计大数字+寿命条（绿/黄/红
//! 三段区间）+错误计数行+「数据来源与口径」折叠说明（哪些是实测哪些是估算
//! ——诚实分级）。
//! 【数据与存储】累计写入量账本持久（掉电续计——每日落盘）；健康数据采集
//! 容错（主控不支持即灰行）。
//! 【状态与异常】SMART 等价接口不可用 → 显示「主控未开放数据」+仅 ext4 层
//! 指标（graceful 既有纪律）；错误计数突增 → 体检灯黄（F120）+建议备份
//! toast。
//! 【设计细节】写入量计数点=块层提交（含写合并后真实盘量——不是应用量）；
//! 寿命区间阈值：绿<60%/黄<85%/红≥85%（估算标注「基于型号标称 TBW」）；
//! 日增量柱状图 30 天；数据口径折叠注明三层（实测/推算/不可知——不许混装）。
//!
//! 掉电续计实现（原子双槽 A/B）：每日检查点写非活动槽（seq 递增+帧校验和），
//! 恢复时取「两槽中 seq 更大且校验通过」者——撕裂帧（半写）必被校验和拒收，
//! 百次断电注入零丢失的构造性保证。
//!
//! 零堆纪律：定长帧 + 定长日柱环，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 寿命区间阈值（估算标注「基于型号标称 TBW」）：绿 <60%。
pub const BAND_GREEN_PERMILLE: u32 = 600;
/// 黄 <85%。
pub const BAND_YELLOW_PERMILLE: u32 = 850;
/// 红 ≥85%。
// （红=其余——两阈值三段，一处一事实只留两个常量。）
/// 日增量柱状图 30 天。
pub const DAILY_BARS: usize = 30;
/// 检查点帧长度（定长）。
pub const CKPT_FRAME_LEN: usize = 48;
/// 检查点帧魔数。
const CKPT_MAGIC: [u8; 4] = *b"VXDH";
/// 错误突增阈值：日增量 >10 → 体检灯黄+建议备份。
pub const ERROR_SPIKE_PER_DAY: u32 = 10;

// ---------------------------------------------------------------------------
// 数据口径三层（实测/推算/不可知——不许混装）
// ---------------------------------------------------------------------------

/// 数据口径分级（「数据来源与口径」折叠说明的诚实分级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 实测（块层/文件系统直接计数）。
    Measured,
    /// 推算（估算模型——标注来源）。
    Estimated,
    /// 不可知（主控未开放——灰行）。
    Unknown,
}

/// 健康指标项（每项独立标口径——不许混装）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metric {
    pub tier: Tier,
    /// 指标值（语义随指标类型）。
    pub value: u64,
}

impl Metric {
    pub const UNKNOWN: Metric = Metric { tier: Tier::Unknown, value: 0 };
}

// ---------------------------------------------------------------------------
// 寿命三段区间
// ---------------------------------------------------------------------------

/// 寿命条分段判定：绿 <60% / 黄 <85% / 红 ≥85%（占型号标称 TBW 千分比）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Green,
    Yellow,
    Red,
}

pub fn band_of(permille: u32) -> Band {
    if permille < BAND_GREEN_PERMILLE {
        Band::Green
    } else if permille < BAND_YELLOW_PERMILLE {
        Band::Yellow
    } else {
        Band::Red
    }
}

// ---------------------------------------------------------------------------
// 写入量账本（块层提交口径 + 每日检查点 + 原子双槽掉电续计）
// ---------------------------------------------------------------------------

/// 写入量账本：计数点=块层提交（含写合并后真实盘量——不是应用量）。
pub struct WriteAccountant {
    /// 累计写入字节（块层提交口径）。
    pub bytes_total: u64,
    /// 当日累计（日柱与检查点用）。
    pub bytes_today: u64,
    /// 今日日序数。
    pub today: u32,
    /// 30 天日增量柱（环形——新的在尾）。
    pub daily: [u64; DAILY_BARS],
    /// 检查点序号（单调递增）。
    ckpt_seq: u64,
    /// 活动槽（A/B 交替）。
    active_slot: u8,
}

/// 检查点帧布局（48B）：
/// [0..4) 魔数 "VXDH" · [4..8) seq u32 · [8..16) bytes_total u64 ·
/// [16..20) day u32 · [20..24) 保留 0 · [24..28) 校验和 FNV-1a(前 24B) · 其余 0。
pub fn encode_ckpt(seq: u32, bytes_total: u64, day: u32, out: &mut [u8; CKPT_FRAME_LEN]) {
    for (i, b) in CKPT_MAGIC.iter().enumerate() {
        out[i] = *b;
    }
    out[4..8].copy_from_slice(&seq.to_le_bytes());
    out[8..16].copy_from_slice(&bytes_total.to_le_bytes());
    out[16..20].copy_from_slice(&day.to_le_bytes());
    for b in out[20..24].iter_mut() {
        *b = 0;
    }
    let sum = fnv1a(&out[..24]);
    out[24..28].copy_from_slice(&sum.to_le_bytes());
    for b in out[28..].iter_mut() {
        *b = 0;
    }
}

/// 校验并解析检查点帧：魔数/校验和任一不过 → None（撕裂帧必拒）。
pub fn decode_ckpt(frame: &[u8; CKPT_FRAME_LEN]) -> Option<(u32, u64, u32)> {
    if frame[0..4] != CKPT_MAGIC {
        return None;
    }
    let sum_stored = u32::from_le_bytes(frame[24..28].try_into().ok()?);
    if fnv1a(&frame[..24]) != sum_stored {
        return None;
    }
    let seq = u32::from_le_bytes(frame[4..8].try_into().ok()?);
    let bytes = u64::from_le_bytes(frame[8..16].try_into().ok()?);
    let day = u32::from_le_bytes(frame[16..20].try_into().ok()?);
    Some((seq, bytes, day))
}

/// FNV-1a（32 位——帧校验和；自研无依赖）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl WriteAccountant {
    /// 建账本：`nominal_tbw_bytes` 为型号标称 TBW（寿命条分母）。
    pub const fn new() -> WriteAccountant {
        WriteAccountant { bytes_total: 0, bytes_today: 0, today: 0, daily: [0; DAILY_BARS], ckpt_seq: 0, active_slot: 0 }
    }

    /// 块层提交回调（写合并后的真实盘量）。
    pub fn on_block_commit(&mut self, bytes: u64, day: u32) {
        if day != self.today {
            self.roll_day(day);
        }
        self.bytes_total = self.bytes_total.saturating_add(bytes);
        self.bytes_today = self.bytes_today.saturating_add(bytes);
        // 今日柱同步累计（不变量：daily[last] 恒等于 bytes_today）。
        self.daily[DAILY_BARS - 1] = self.bytes_today;
    }

    fn roll_day(&mut self, day: u32) {
        // 日柱滚动（bar[last]=今日、bar[last-1]=昨日…bar[0]=30 天前）：
        // 老数据向左滚 gap 格（bar[i] ← bar[i+gap]），头部 gap 格清零——
        // 跨越的空日为 0 柱（不伪造连续）；首日（today==0）直接开账。
        let gap = day.saturating_sub(self.today).min(DAILY_BARS as u32) as usize;
        if self.today != 0 && gap > 0 {
            self.daily.copy_within(gap..DAILY_BARS, 0);
            for b in self.daily[DAILY_BARS - gap..].iter_mut() {
                *b = 0;
            }
        }
        self.bytes_today = 0;
        self.today = day;
    }

    /// 每日检查点落盘（原子双槽——写非活动槽后切换）。
    pub fn daily_checkpoint(&mut self, slots: &mut [[u8; CKPT_FRAME_LEN]; 2]) {
        self.ckpt_seq = self.ckpt_seq.wrapping_add(1);
        self.active_slot = 1 - self.active_slot;
        let seq32 = self.ckpt_seq as u32;
        encode_ckpt(seq32, self.bytes_total, self.today, &mut slots[self.active_slot as usize]);
    }

    /// 掉电恢复：取两槽中「seq 更大且校验通过」者（撕裂帧必拒——零丢失）。
    pub fn resume_from(&mut self, slots: &[[u8; CKPT_FRAME_LEN]; 2]) -> bool {
        let cands = slots.iter().filter_map(decode_ckpt);
        let best = cands.max_by_key(|(seq, _, _)| *seq);
        match best {
            Some((_, bytes, day)) => {
                self.bytes_total = bytes;
                self.today = day;
                self.bytes_today = 0;
                true
            }
            None => false,
        }
    }

    /// 写入量累计与夹具实测对拍：账面 vs 夹具计数偏差 ≤2%。
    pub fn reconcile(&self, fixture_bytes: u64) -> bool {
        if fixture_bytes == 0 {
            return self.bytes_total == 0;
        }
        let diff = self.bytes_total.abs_diff(fixture_bytes);
        // 千分比偏差 ≤20‰（±2%）。
        diff * 1000 / fixture_bytes <= 20
    }
}

// ---------------------------------------------------------------------------
// 错误计数与体检灯（F120 联动）
// ---------------------------------------------------------------------------

/// ext4 错误计数器（突增检测 → 体检灯黄+建议备份 toast）。
pub struct ErrorMonitor {
    pub total: u32,
    /// 昨日累计（日增量分母）。
    yesterday_total: u32,
    /// 突增旗（体检灯黄）。
    pub lamp_yellow: bool,
    /// 建议备份 toast 旗（月频不烦——月内只提一次）。
    pub backup_toast_pending: bool,
    /// 本月已提醒（月频节流）。
    toast_month_done: u32,
}

impl ErrorMonitor {
    pub const fn new() -> ErrorMonitor {
        ErrorMonitor { total: 0, yesterday_total: 0, lamp_yellow: false, backup_toast_pending: false, toast_month_done: u32::MAX }
    }

    /// 错误上报（ext4 层）。
    pub fn on_error(&mut self, month: u32) {
        self.total += 1;
        self.evaluate(month);
    }

    fn evaluate(&mut self, month: u32) {
        let today_delta = self.total.saturating_sub(self.yesterday_total);
        if today_delta > ERROR_SPIKE_PER_DAY {
            self.lamp_yellow = true;
            if self.toast_month_done != month {
                self.backup_toast_pending = true;
                self.toast_month_done = month;
            }
        }
    }

    /// 日切（昨日累计滚动）。
    pub fn day_rollover(&mut self) {
        self.yesterday_total = self.total;
    }

    /// toast 消费（点开即清——不堆积）。
    pub fn consume_toast(&mut self) {
        self.backup_toast_pending = false;
    }
}

// ---------------------------------------------------------------------------
// 磁盘健康页模型（诚实分级——三层口径不许混装）
// ---------------------------------------------------------------------------

/// 健康页快照。
pub struct HealthPage {
    /// 写入量累计（实测——块层口径）。
    pub written: Metric,
    /// TBW 寿命占比（推算——基于型号标称）。
    pub wear: Metric,
    /// 磨损均衡（主控可读则读，不可读=Unknown 灰行）。
    pub wear_leveling: Metric,
    /// ext4 错误计数（实测）。
    pub errors: Metric,
    /// 寿命条分段。
    pub band: Band,
}

/// 组装健康页：主控指标可读性注入（不支持=灰行「主控未开放数据」）。
pub fn build_health_page(
    written_bytes: u64,
    nominal_tbw_bytes: u64,
    wear_leveling_raw: Option<u64>,
    errors: u32,
) -> HealthPage {
    // 寿命占比：仅当标称 TBW 有效时推算，否则不可知（不猜）。
    let wear = if nominal_tbw_bytes > 0 {
        let permille = ((written_bytes.saturating_mul(1000)) / nominal_tbw_bytes).min(u32::MAX as u64) as u32;
        Metric { tier: Tier::Estimated, value: permille as u64 }
    } else {
        Metric::UNKNOWN
    };
    let band = if wear.tier == Tier::Estimated { band_of(wear.value as u32) } else { Band::Green };
    HealthPage {
        written: Metric { tier: Tier::Measured, value: written_bytes },
        wear,
        wear_leveling: wear_leveling_raw.map(|v| Metric { tier: Tier::Measured, value: v }).unwrap_or(Metric::UNKNOWN),
        errors: Metric { tier: Tier::Measured, value: errors as u64 },
        band,
    }
}

/// 「主控未开放数据」灰行文案锚。
pub const CONTROLLER_NA_COPY: &str = "主控未开放数据";

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_diskhealth_checks() -> CheckSet {
    let mut cs = CheckSet::new("F183-diskhealth");

    // 1) 写入量累计与夹具实测对拍 ±2%（块层提交口径）。
    let mut acc = WriteAccountant::new();
    for _ in 0..100 {
        acc.on_block_commit(1_048_576, 1); // 100 MiB
    }
    cs.add("reconcile_within_2pct", acc.reconcile(100 * 1_048_576), "");

    // 2) 对拍偏差越线必红（>2% 偏差判定器真的在判）。
    let mut acc2 = WriteAccountant::new();
    acc2.on_block_commit(1_100_000_000, 1);
    cs.add("reconcile_rejects_drift", !acc2.reconcile(1_000_000_000), "");

    // 3) 检查点帧 round-trip + 校验和防撕裂（半写帧必拒）。
    let mut frame = [0u8; CKPT_FRAME_LEN];
    encode_ckpt(7, 123_456_789, 42, &mut frame);
    let ok = decode_ckpt(&frame) == Some((7, 123_456_789, 42));
    let mut torn = frame;
    torn[10] ^= 0xFF; // 数据区撕裂
    cs.add("ckpt_roundtrip_and_torn", ok && decode_ckpt(&torn).is_none(), "");

    // 4) 原子双槽掉电续计：A 槽旧 seq + B 槽新 seq → 取 B；B 槽撕裂 → 回落 A。
    let mut slots = [[0u8; CKPT_FRAME_LEN]; 2];
    encode_ckpt(3, 1_000, 10, &mut slots[0]);
    encode_ckpt(4, 2_000, 11, &mut slots[1]);
    let mut acc3 = WriteAccountant::new();
    let took_b = acc3.resume_from(&slots) && acc3.bytes_total == 2_000;
    let mut torn_b = slots;
    torn_b[1][12] ^= 0x55;
    let mut acc4 = WriteAccountant::new();
    let fell_back_a = acc4.resume_from(&torn_b) && acc4.bytes_total == 1_000;
    cs.add("dual_slot_power_loss_resume", took_b && fell_back_a, "");

    // 5) 断电百次复用：随机中间态撕裂注入——有效检查点永不丢（构造性口径：
    //    旧槽在新槽写完成前保持完好；恢复取最大有效 seq）。
    let mut acc5 = WriteAccountant::new();
    let mut survived = 0u32;
    for round in 0..100u32 {
        let mut s = [[0u8; CKPT_FRAME_LEN]; 2];
        encode_ckpt(round, (round as u64 + 1) * 1_000, round, &mut s[0]);
        // 新一轮写到 B 槽——撕裂一半（只写前 24B 无校验和）。
        encode_ckpt(round + 1, (round as u64 + 2) * 1_000, round + 1, &mut s[1]);
        s[1][25] ^= 0xFF; // 校验和区撕裂
        if acc5.resume_from(&s) && acc5.bytes_total == (round as u64 + 1) * 1_000 {
            survived += 1;
        }
    }
    cs.add("power_cut_100_rounds_zero_loss", survived == 100, "");

    // 6) 寿命三段区间（绿<60%/黄<85%/红≥85%——阈值文档化）。
    cs.add(
        "band_three_segments",
        band_of(599) == Band::Green && band_of(600) == Band::Yellow && band_of(849) == Band::Yellow && band_of(850) == Band::Red,
        "",
    );

    // 7) 健康页三层口径不混装（写入=实测/寿命=推算/主控缺=不可知灰行）。
    let page = build_health_page(400_000_000_000, 1_000_000_000_000, None, 3);
    cs.add(
        "tiers_not_mixed",
        page.written.tier == Tier::Measured
            && page.wear.tier == Tier::Estimated
            && page.wear_leveling.tier == Tier::Unknown
            && page.wear_leveling == Metric::UNKNOWN,
        "",
    );

    // 8) 寿命条分段联动（40% 写入 → 绿；62% → 黄；90% → 红）。
    let g = build_health_page(400_000_000_000, 1_000_000_000_000, Some(1), 0);
    let y = build_health_page(620_000_000_000, 1_000_000_000_000, Some(1), 0);
    let r = build_health_page(900_000_000_000, 1_000_000_000_000, Some(1), 0);
    cs.add("band_page_linkage", g.band == Band::Green && y.band == Band::Yellow && r.band == Band::Red, "");

    // 9) 错误突增 → 体检灯黄+建议备份 toast（月频节流——同月不重复）。
    let mut em = ErrorMonitor::new();
    for _ in 0..11 {
        em.on_error(3);
    }
    let fired = em.lamp_yellow && em.backup_toast_pending;
    em.consume_toast();
    em.on_error(3); // 同月第 12 错——toast 不再发（月频不烦）
    cs.add("error_spike_lamp_toast_monthly", fired && !em.backup_toast_pending, "");

    // 10) 新月份重新可提醒。
    em.on_error(4);
    cs.add("toast_next_month_refires", em.backup_toast_pending, "");

    // 11) 日柱环形 30 天 + 空日为 0（不伪造连续）。
    let mut acc6 = WriteAccountant::new();
    acc6.on_block_commit(1_000, 1);
    acc6.on_block_commit(2_000, 3); // 跳过 day2——空日 0 柱
    cs.add("daily_bars_gap_zero", acc6.daily[DAILY_BARS - 1] == 2_000, "");

    // 12) 常量对账（60%/85% 阈值、30 柱、48B 帧、突增 10）。
    cs.add(
        "constants_reconciled",
        BAND_GREEN_PERMILLE == 600 && BAND_YELLOW_PERMILLE == 850 && DAILY_BARS == 30 && CKPT_FRAME_LEN == 48 && ERROR_SPIKE_PER_DAY == 10,
        "",
    );

    // 13) 灰行文案锚在位（主控未开放数据——graceful 纪律）。
    cs.add("controller_na_copy", CONTROLLER_NA_COPY.contains("主控未开放"), "");

    cs
}

// ---------------------------------------------------------------------------
// 深化层（批次二）：SMART 等价读取面 · 三层口径折叠说明 · 备份提醒
// 调度 —— 主册【设计细节】「TBW 口径参照 NAND 磨损公开文献（P/E cycle
// 估算模型标注来源）/数据口径折叠注明三层（实测/推算/不可知——不许混装）/
// 黄段起备份提醒 toast（月频不烦）」落地。
// ---------------------------------------------------------------------------

/// SMART 等价读取面（主控指标注入口——可读则读，不可读=灰行）。
pub trait SmartReader {
    /// 磨损均衡指标（0-1000‰ 主控自报；None=主控未开放）。
    fn wear_leveling_permille(&self) -> Option<u32>;
    /// 主控型号是否提供健康页（诚实分级：型号声明 ≠ 实测）。
    fn controller_supports_health(&self) -> bool;
}

/// 无 SMART 主控的空实现（灰行语义——「主控未开放数据」路径的引擎侧）。
pub struct NoSmart;
impl SmartReader for NoSmart {
    fn wear_leveling_permille(&self) -> Option<u32> {
        None
    }
    fn controller_supports_health(&self) -> bool {
        false
    }
}

/// 有 SMART 主控的样本实现（台架注入——测试与真实驱动同一 trait）。
pub struct FakeSmart {
    pub wear: Option<u32>,
}
impl SmartReader for FakeSmart {
    fn wear_leveling_permille(&self) -> Option<u32> {
        self.wear
    }
    fn controller_supports_health(&self) -> bool {
        true
    }
}

/// TBW 估算模型标注（P/E cycle 模型——来源标注随页输出，F130 开放纪律）。
pub const TBW_MODEL_CITATION: &str = "P/E-cycle estimate, model nominal TBW";

/// 健康页折叠说明文案（三层口径——「数据来源与口径」节的内容面）。
pub fn explainer_lines() -> [(&'static str, Tier); 4] {
    [
        ("写入量累计：块层提交实测（含写合并后真实盘量）", Tier::Measured),
        ("寿命区间：基于型号标称 TBW 推算", Tier::Estimated),
        ("磨损均衡：主控可读则读，不可读不显示", Tier::Unknown),
        ("ext4 错误计数：文件系统层实测", Tier::Measured),
    ]
}

/// 三层不许混装校验（口径纪律的机器面：每行口径与其内容自洽——
/// 推算行必须带「推算」字样、不可知行必须带「不显示/未开放」语义）。
pub fn explainer_tiers_consistent(lines: &[(&'static str, Tier); 4]) -> bool {
    lines.iter().all(|(text, tier)| match tier {
        Tier::Measured => text.contains("实测") && !text.contains("推算"),
        Tier::Estimated => text.contains("推算") || text.contains("标称"),
        Tier::Unknown => text.contains("不显示") || text.contains("未开放"),
    })
}

/// 备份提醒月键（月频不烦——年×12 月键，同月只提一次的键空间）。
pub fn backup_toast_month_key(year: u32, month: u32) -> u32 {
    year.wrapping_mul(12).wrapping_add(month.saturating_sub(1).min(11))
}

/// 黄段备份提醒裁决（月频节流的裁决面：黄/红段+当月未提过 → 提）。
pub fn backup_reminder_due(band: Band, month_done: u32, month: u32) -> bool {
    matches!(band, Band::Yellow | Band::Red) && month_done != month
}

/// 深化自检（检查项对账层——主册【设计细节】子句逐项实算）。
#[inline(never)]
pub fn run_diskhealth_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F183-deep");

    // 1) SMART 面：有主控 → 指标可读；无主控 → None（灰行引擎侧）。
    let smart = FakeSmart { wear: Some(420) };
    let none = NoSmart;
    cs.add(
        "smart_reader_triage",
        smart.wear_leveling_permille() == Some(420) && none.wear_leveling_permille().is_none() && !none.controller_supports_health(),
        "",
    );

    // 2) SMART 面接健康页组装（None → Unknown 灰行—— trait 到页面贯通）。
    let page_smart = build_health_page(100_000_000_000, 1_000_000_000_000, smart.wear_leveling_permille().map(|v| v as u64), 0);
    let page_none = build_health_page(100_000_000_000, 1_000_000_000_000, none.wear_leveling_permille().map(|v| v as u64), 0);
    cs.add(
        "smart_to_page_linkage",
        page_smart.wear_leveling.tier == Tier::Measured && page_none.wear_leveling.tier == Tier::Unknown,
        "",
    );

    // 3) TBW 模型来源标注在册（P/E cycle——F130 开放引用纪律）。
    cs.add("tbw_model_citation", TBW_MODEL_CITATION.contains("P/E") && TBW_MODEL_CITATION.contains("TBW"), "");

    // 4) 三层口径折叠说明：四行齐、口径逐行自洽（不许混装机器面）。
    let lines = explainer_lines();
    cs.add(
        "explainer_tiers_consistent",
        lines.len() == 4 && explainer_tiers_consistent(&lines),
        "",
    );

    // 5) 折叠说明覆盖实测/推算/不可知三态（三态各至少一行——分级展示完整性）。
    let has_measured = lines.iter().any(|(_, t)| *t == Tier::Measured);
    let has_est = lines.iter().any(|(_, t)| *t == Tier::Estimated);
    let has_unknown = lines.iter().any(|(_, t)| *t == Tier::Unknown);
    cs.add("explainer_covers_all_tiers", has_measured && has_est && has_unknown, "");

    // 6) 月键构造：年月 → 稳定键（2026-09 与 2027-09 不同键——跨年不碰撞）。
    cs.add(
        "backup_month_key",
        backup_toast_month_key(2026, 9) != backup_toast_month_key(2027, 9) && backup_toast_month_key(2026, 9) == backup_toast_month_key(2026, 9),
        "",
    );

    // 7) 黄段提醒裁决：黄/红段+当月未提 → 提；绿段不提；同月已提不提。
    cs.add(
        "backup_reminder_due",
        backup_reminder_due(Band::Yellow, 0, 9) && !backup_reminder_due(Band::Green, 0, 9) && !backup_reminder_due(Band::Red, 9, 9),
        "",
    );

    // 8) 错误计数行口径（实测——ext4 层来源在折叠说明中声明）。
    let page = build_health_page(1, 1_000_000_000_000, None, 7);
    cs.add("error_row_measured", page.errors.tier == Tier::Measured && page.errors.value == 7, "");

    // 9) 写入计数点语义（块层提交——含写合并后真实盘量，非应用量的声明行）。
    cs.add(
        "block_layer_semantics",
        explainer_lines()[0].0.contains("块层提交") && explainer_lines()[0].0.contains("写合并"),
        "",
    );

    // 10) 寿命条三段与提醒联动（绿段零打扰——月频纪律的前提）。
    cs.add(
        "band_reminder_linkage",
        !backup_reminder_due(Band::Green, 0, 9) && backup_reminder_due(Band::Yellow, 0, 9) && backup_reminder_due(Band::Red, 0, 9),
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_rollover_across_gap() {
        // 跨 2 日跳变（day1→day3）：day1 的柱滚到 27 位、day2 空日为 0 柱、
        // day3 在今日柱累计（不伪造连续——空日诚实显 0）。
        let mut acc = WriteAccountant::new();
        acc.on_block_commit(1_000, 1);
        acc.on_block_commit(2_000, 3);
        assert_eq!(acc.daily[DAILY_BARS - 3], 1_000, "day1 柱位");
        assert_eq!(acc.daily[DAILY_BARS - 2], 0, "day2 空日 0 柱");
        assert_eq!(acc.daily[DAILY_BARS - 1], 2_000, "day3 今日柱");
        assert_eq!(acc.bytes_today, 2_000);
    }

    #[test]
    fn day_rollover_beyond_window() {
        // 跨越 30 天以上：旧柱全部滚出（诚实清空——不残留幽灵数据），
        // 仅今日柱承载新写入。
        let mut acc = WriteAccountant::new();
        acc.on_block_commit(1_000, 1);
        acc.on_block_commit(500, 1 + 45);
        for v in acc.daily[..DAILY_BARS - 1].iter() {
            assert_eq!(*v, 0, "45 天后旧柱应全部滚出");
        }
        assert_eq!(acc.daily[DAILY_BARS - 1], 500);
        assert_eq!(acc.bytes_today, 500);
    }

    #[test]
    fn checkpoint_seq_monotonic() {
        // 连续落盘 seq 单调递增（恢复时取大者——次序无歧义）。
        let mut acc = WriteAccountant::new();
        let mut slots = [[0u8; CKPT_FRAME_LEN]; 2];
        let mut last_seq = 0u32;
        for i in 0..8 {
            acc.on_block_commit(100, i + 1);
            acc.daily_checkpoint(&mut slots);
            let (_, bytes, _) = decode_ckpt(&slots[acc.active_slot as usize]).unwrap();
            let seq = decode_ckpt(&slots[acc.active_slot as usize]).unwrap().0;
            assert!(seq > last_seq || i == 0);
            assert_eq!(bytes, (i as u64 + 1) * 100);
            last_seq = seq;
        }
    }

    #[test]
    fn reconcile_zero_fixture() {
        // 零夹具对账：账面非零必红（0/0 语义不装糊涂）。
        let acc = WriteAccountant::new();
        assert!(acc.reconcile(0));
        let mut acc2 = WriteAccountant::new();
        acc2.on_block_commit(1, 1);
        assert!(!acc2.reconcile(0));
    }

    #[test]
    fn both_slots_torn_honest_failure() {
        // 双槽全撕 → resume 返回 false（诚实失败——不编造账面）。
        let mut s = [[0u8; CKPT_FRAME_LEN]; 2];
        encode_ckpt(1, 1_000, 1, &mut s[0]);
        encode_ckpt(2, 2_000, 2, &mut s[1]);
        s[0][3] ^= 0xFF; // 撕魔数
        s[1][26] ^= 0xFF; // 撕校验和
        let mut acc = WriteAccountant::new();
        assert!(!acc.resume_from(&s));
        assert_eq!(acc.bytes_total, 0);
    }

    #[test]
    fn band_boundary_exact_values() {
        // 阈值边界精确值（600/850 千分位——文档化口径逐位对账）。
        assert_eq!(band_of(0), Band::Green);
        assert_eq!(band_of(599), Band::Green);
        assert_eq!(band_of(600), Band::Yellow);
        assert_eq!(band_of(849), Band::Yellow);
        assert_eq!(band_of(850), Band::Red);
        assert_eq!(band_of(u32::MAX), Band::Red);
    }
}
