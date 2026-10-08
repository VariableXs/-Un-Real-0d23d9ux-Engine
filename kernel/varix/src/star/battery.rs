//! F060 电量账本 · 完整设计（STAR I 主册 G-B-20）。
//!
//! **判据（主册）**：折算值与实测电量计曲线相关性 >0.8（同窗口对比）；
//! 排行前三与人工判断一致（抽查场景）。
//!
//! **设计要点（主册）**：
//! - 按应用能耗画像：CPU 时间 × 频率档 + 唤醒次数 + IO 量折算为相对
//!   能耗单位（诚实原则：不伪精确瓦特数）；
//! - 权重初值：CPU 每毫秒核秒=1.0 / 唤醒每次=0.05 / IO 每 MB=0.02
//!   （参数进旋钮清单）；
//! - 屏幕亮度不计入应用（归系统项单列）；
//! - 电量计不可读（部分机器）→ 显示「耗电相对值」并说明口径（诚实降级）；
//! - 应用已卸载 → 画像归档不删除；
//! - 数据受隐私总闸（F036 开关）管理；
//! - 画像每分钟聚合入账本；保留 30 天；排行在设置页打开时现算。
//!
//! 能耗单位：毫单位 mu（milli-unit）。`energy_mu = cpu_ms_core × W_CPU
//! + wakes × W_WAKE + io_mb × W_IO`，权重以 mu 计（默认 1000/50/20）。

use crate::checks::CheckSet;
use crate::star::sbase::MinuteBook;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 应用画像容量（LRU 上限——活跃应用数远小于此）。
pub const APP_CAP: usize = 64;

/// 画像保留窗口（分钟）——30 天。
pub const RETENTION_MIN: u64 = 30 * 1440;

/// 权重：CPU 每毫秒核秒（mu）。
pub const W_CPU_MU: u64 = 1000;

/// 权重：每次唤醒（mu）。
pub const W_WAKE_MU: u64 = 50;

/// 权重：每 MB IO（mu）。
pub const W_IO_MU: u64 = 20;

/// 相关性判线（主册 >0.8）。
pub const CORRELATION_LINE_X100: i64 = 80;

// ---------------------------------------------------------------------------
// 权重（旋钮化）
// ---------------------------------------------------------------------------

/// 能耗权重表（可调——参数进旋钮清单；超出物理意义边界的输入钳制）。
#[derive(Clone, Copy, Debug)]
pub struct EnergyWeights {
    /// CPU 每毫秒核秒（mu）。
    pub cpu_mu: u64,
    /// 每次唤醒（mu）。
    pub wake_mu: u64,
    /// 每 MB IO（mu）。
    pub io_mb_mu: u64,
}

impl EnergyWeights {
    pub const DEFAULT: EnergyWeights = EnergyWeights { cpu_mu: W_CPU_MU, wake_mu: W_WAKE_MU, io_mb_mu: W_IO_MU };

    /// 单分钟折算（定点整数，零浮点——账本聚合确定性）。
    pub fn minute_mu(&self, cpu_ms_core: u64, wakes: u64, io_mb: u64) -> u64 {
        cpu_ms_core
            .saturating_mul(self.cpu_mu)
            .saturating_add(wakes.saturating_mul(self.wake_mu))
            .saturating_add(io_mb.saturating_mul(self.io_mb_mu))
    }
}

impl Default for EnergyWeights {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ---------------------------------------------------------------------------
// 应用画像
// ---------------------------------------------------------------------------

/// 单应用画像：分钟稀疏账本（stride 3：cpu_ms_core/wakes/io_mb）+ 状态。
pub struct AppProfile {
    pub app_id: u32,
    /// 归档标记：应用已卸载 → 画像归档不删除（主册状态与异常）。
    pub archived: bool,
    /// 分钟账本（稀疏：只存有活动的分钟）。
    book: MinuteBook,
    /// 累计账目（跨窗口总量，不随驱逐丢失）。
    pub total_mu: u64,
    pub total_minutes: u64,
}

impl AppProfile {
    fn new(app_id: u32) -> AppProfile {
        AppProfile {
            app_id,
            archived: false,
            book: MinuteBook::new(3, RETENTION_MIN),
            total_mu: 0,
            total_minutes: 0,
        }
    }

    /// 记一分钟活动。
    fn sample(&mut self, stamp_min: u64, cpu_ms_core: u64, wakes: u64, io_mb: u64, w: &EnergyWeights) {
        self.book.record_minute(stamp_min, &[cpu_ms_core, wakes, io_mb]);
        self.book.evict_by_now(stamp_min);
        self.total_mu += w.minute_mu(cpu_ms_core, wakes, io_mb);
        self.total_minutes += 1;
    }

    /// 窗口能耗（mu）：[from_min, to_min]。
    fn window_mu(&self, from_min: u64, to_min: u64, w: &EnergyWeights) -> u64 {
        let s = self.book.range_sum(from_min, to_min);
        w.minute_mu(s[0], s[1], s[2])
    }

    fn window_breakdown(&self, from_min: u64, to_min: u64) -> (u64, u64, u64) {
        let s = self.book.range_sum(from_min, to_min);
        (s[0], s[1], s[2])
    }
}

// ---------------------------------------------------------------------------
// 排行
// ---------------------------------------------------------------------------

/// 排行条目（设置页「今天谁在耗电」一行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RankRow {
    pub app_id: u32,
    /// 窗口能耗（mu）。
    pub energy_mu: u64,
    /// 占系统总折算的 ppm（诚实相对值——不伪瓦特）。
    pub share_ppm: u64,
    pub archived: bool,
}

/// 排行窗口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RankWindow {
    /// 最近 1 小时。
    Hour,
    /// 今天（当日 0 点起）。
    Today,
    /// 最近 7 天。
    Week,
}

impl RankWindow {
    pub fn span_min(self, now_min: u64) -> (u64, u64) {
        match self {
            RankWindow::Hour => (now_min.saturating_sub(60), now_min),
            RankWindow::Today => (now_min / 1440 * 1440, now_min),
            RankWindow::Week => (now_min.saturating_sub(7 * 1440), now_min),
        }
    }
}

// ---------------------------------------------------------------------------
// 电量账本主体
// ---------------------------------------------------------------------------

/// 电量账本。
pub struct BatteryLedger {
    apps: Vec<AppProfile>,
    /// 系统项（屏幕亮度等）分钟账本（单列 stride 1，mu 直记）。
    system_book: MinuteBook,
    weights: EnergyWeights,
    /// 隐私总闸（F036）：关 → 停止记录（历史保留）。
    pub privacy_on: bool,
    /// 电量计可读性：false → 页面显示「耗电相对值」并说明口径。
    pub gauge_readable: bool,
    /// 实测电量曲线（电量计读数，电量 µAh 或千分比——口径由上层定，
    /// 相关性只看形状）：(stamp_min, level_permille)。
    gauge: Vec<(u64, u64)>,
    pub clamped_apps: u64,
}

impl BatteryLedger {
    pub fn new() -> BatteryLedger {
        BatteryLedger {
            apps: Vec::new(),
            system_book: MinuteBook::new(1, RETENTION_MIN),
            weights: EnergyWeights::DEFAULT,
            privacy_on: true,
            gauge_readable: true,
            gauge: Vec::new(),
            clamped_apps: 0,
        }
    }

    pub fn weights(&self) -> &EnergyWeights {
        &self.weights
    }

    /// 调权（钳制：CPU ≥100mu、wake ≥1mu、IO ≥1mu，防呆）。
    pub fn set_weights(&mut self, cpu_mu: u64, wake_mu: u64, io_mb_mu: u64) {
        self.weights = EnergyWeights {
            cpu_mu: cpu_mu.max(100),
            wake_mu: wake_mu.max(1),
            io_mb_mu: io_mb_mu.max(1),
        };
    }

    fn app_slot(&mut self, app_id: u32) -> Option<&mut AppProfile> {
        if let Some(pos) = self.apps.iter().position(|a| a.app_id == app_id) {
            return self.apps.get_mut(pos);
        }
        if self.apps.len() >= APP_CAP {
            // LRU 逐出：优先逐出已归档者，其次 total_minutes 最小者。
            let victim = self
                .apps
                .iter()
                .enumerate()
                .min_by_key(|(_, a)| (u8::from(!a.archived), a.total_minutes))
                .map(|(i, _)| i)?;
            let was_archived = self.apps[victim].archived;
            self.apps.remove(victim);
            if !was_archived {
                self.clamped_apps += 1;
            }
        }
        self.apps.push(AppProfile::new(app_id));
        self.apps.last_mut()
    }

    /// 记一分钟应用活动（隐私总闸关 → 拒记并返回 false——零静默）。
    pub fn sample_app(
        &mut self,
        stamp_min: u64,
        app_id: u32,
        cpu_ms_core: u64,
        wakes: u64,
        io_mb: u64,
    ) -> bool {
        if !self.privacy_on {
            return false;
        }
        let w = self.weights;
        let slot = self.app_slot(app_id).expect("app cap enforced");
        slot.sample(stamp_min, cpu_ms_core, wakes, io_mb, &w);
        true
    }

    /// 记一分钟系统项能耗（屏幕亮度等，mu 直记——不入应用排行）。
    pub fn sample_system(&mut self, stamp_min: u64, energy_mu: u64) {
        self.system_book.record_minute(stamp_min, &[energy_mu]);
        self.system_book.evict_by_now(stamp_min);
    }

    /// 记电量计读数（permille 0-1000，越界钳制）。
    pub fn note_gauge(&mut self, stamp_min: u64, level_permille: u64) {
        let lvl = level_permille.min(1000);
        if let Some(last) = self.gauge.last() {
            if last.0 == stamp_min {
                // 同分钟覆写最后读数。
                self.gauge.last_mut().unwrap().1 = lvl;
                return;
            }
        }
        self.gauge.push((stamp_min, lvl));
        if self.gauge.len() > 43200 {
            self.gauge.remove(0);
        }
    }

    /// 应用排行（窗口内现算——账本数据轻量，设置页打开时调用）。
    ///
    /// 行序按窗口能耗降序；尾部恒追加一条系统项单列（app_id=0，屏幕
    /// 亮度等——主册「不计入应用」）。share_ppm 以「应用合计+系统项」
    /// 为分母，诚实相对值。
    pub fn ranking(&self, window: RankWindow, now_min: u64, top: usize) -> Vec<RankRow> {
        let (from, to) = window.span_min(now_min);
        let sys_total = self.system_book.range_sum(from, to)[0];
        let mut rows: Vec<(u32, u64, bool)> = Vec::new();
        for a in &self.apps {
            let mu = a.window_mu(from, to, &self.weights);
            if mu > 0 {
                rows.push((a.app_id, mu, a.archived));
            }
        }
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let app_sum: u64 = rows.iter().map(|r| r.1).sum();
        let grand = app_sum.saturating_add(sys_total).max(1);
        let mut out: Vec<RankRow> = rows
            .into_iter()
            .take(top.max(1))
            .map(|(id, mu, archived)| RankRow {
                app_id: id,
                energy_mu: mu,
                share_ppm: mu * 1_000_000 / grand,
                archived,
            })
            .collect();
        out.push(RankRow {
            app_id: 0,
            energy_mu: sys_total,
            share_ppm: sys_total * 1_000_000 / grand,
            archived: false,
        });
        out
    }

    /// 应用三分项（唤醒/IO/CPU 明细——设置页行展开）。
    pub fn breakdown(&self, app_id: u32, window: RankWindow, now_min: u64) -> Option<(u64, u64, u64)> {
        let (from, to) = window.span_min(now_min);
        self.apps
            .iter()
            .find(|a| a.app_id == app_id)
            .map(|a| a.window_breakdown(from, to))
    }

    /// 应用卸载 → 归档（不删除——主册纪律）。
    pub fn archive_app(&mut self, app_id: u32) -> bool {
        match self.apps.iter_mut().find(|a| a.app_id == app_id) {
            Some(a) => {
                a.archived = true;
                true
            }
            None => false,
        }
    }

    /// 实测窗口耗电（permille 下降量）。
    ///
    /// 基线取「from_min 前最后一个读数」；窗口前无读数时回退到窗内
    /// 首个读数（首测窗口也能算——口径在诊断面如实标注）。
    pub fn measured_drain_permille(&self, from_min: u64, to_min: u64) -> Option<u64> {
        let mut last_before: Option<u64> = None;
        let mut first_in: Option<u64> = None;
        let mut last_after: Option<u64> = None;
        for (t, lvl) in &self.gauge {
            if *t <= from_min {
                last_before = Some(*lvl);
            } else if *t <= to_min && first_in.is_none() {
                first_in = Some(*lvl);
            }
            if *t <= to_min {
                last_after = Some(*lvl);
            }
        }
        let b = last_before.or(first_in)?;
        let a = last_after?;
        Some(b.saturating_sub(a))
    }

    /// 窗口模型折算总量（mu）。
    pub fn modeled_mu(&self, from_min: u64, to_min: u64) -> u64 {
        let apps_mu: u64 = self.apps.iter().map(|a| a.window_mu(from_min, to_min, &self.weights)).sum();
        let sys_mu = self.system_book.range_sum(from_min, to_min)[0];
        apps_mu.saturating_add(sys_mu)
    }

    /// 相关性审计：把观察期切 N 段，逐段 (模型 mu, 实测下降 permille)
    /// 求 Pearson r×100。样本 <3 段或实测全平 → None（不伪装达标）。
    pub fn correlation_audit(&self, from_min: u64, to_min: u64, segments: usize) -> Option<i64> {
        let n = segments.max(3);
        let span = to_min.saturating_sub(from_min);
        if span == 0 {
            return None;
        }
        let step = (span / n as u64).max(1);
        let mut pairs: Vec<(u64, u64)> = Vec::new();
        let mut seg = from_min;
        while seg + step <= to_min {
            let x = self.modeled_mu(seg, seg + step);
            if let Some(drain) = self.measured_drain_permille(seg, seg + step) {
                pairs.push((x, drain));
            }
            seg += step;
        }
        if pairs.len() < 3 {
            return None;
        }
        pearson_x100(&pairs)
    }
}

impl Default for BatteryLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Pearson 相关系数 ×100——**纯整数实现**（i128 中间量 + 整数平方根）。
///
/// r = (n·Σxy − Σx·Σy) / √[(n·Σx² − (Σx)²)·(n·Σy² − (Σy)²)]
/// 输入 (x, y) 对；样本 <2 或任一方向零方差 → None（诚实不可判）。
/// 负相关合法（模型说耗电、电量计却回升 → r<0，判据自然红）。
fn pearson_x100(pairs: &[(u64, u64)]) -> Option<i64> {
    let n = pairs.len();
    if n < 2 {
        return None;
    }
    let mut sx = 0i128;
    let mut sy = 0i128;
    let mut sxy = 0i128;
    let mut sx2 = 0i128;
    let mut sy2 = 0i128;
    for &(x, y) in pairs {
        let (x, y) = (x as i128, y as i128);
        sx += x;
        sy += y;
        sxy += x * y;
        sx2 += x * x;
        sy2 += y * y;
    }
    let nf = n as i128;
    let num = nf * sxy - sx * sy;
    let dx = nf * sx2 - sx * sx;
    let dy = nf * sy2 - sy * sy;
    if dx <= 0 || dy <= 0 {
        return None;
    }
    let den = isqrt_u128(dx as u128) as i128 * isqrt_u128(dy as u128) as i128;
    if den == 0 {
        return None;
    }
    let r_x100 = (num * 100 / den) as i64;
    Some(r_x100.clamp(-100, 100))
}

/// u128 整数平方根（牛顿迭代，向下取整）。
fn isqrt_u128(v: u128) -> u128 {
    if v < 2 {
        return v;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F060 自检（聚合进 star 域）。
pub fn run_battery_checks() -> CheckSet {
    let mut set = CheckSet::new("F060-battery");

    // 权重折算数学。
    let w = EnergyWeights::DEFAULT;
    set.add("weight math", w.minute_mu(1000, 10, 5) == 1000 * 1000 + 10 * 50 + 5 * 20, "");

    // 记账与排行。
    let mut led = BatteryLedger::new();
    for m in 0..120u64 {
        led.sample_app(m, 1, 30_000, 20, 2); // 高耗应用
        led.sample_app(m, 2, 3_000, 1, 0); // 低耗应用
    }
    let rank = led.ranking(RankWindow::Hour, 120, 10);
    set.add("rank order", rank[0].app_id == 1 && rank[1].app_id == 2, "");
    set.add("rank share", rank[0].share_ppm > rank[1].share_ppm, "");

    // 归档不删除。
    set.add("archive ok", led.archive_app(2), "");
    let rank2 = led.ranking(RankWindow::Hour, 120, 10);
    let arch = rank2.iter().find(|r| r.app_id == 2);
    set.add("archived still ranked", arch.map(|r| r.archived) == Some(true), "");

    // 隐私总闸：关 → 拒记（返回 false），历史保留。
    led.privacy_on = false;
    set.add("privacy blocks", !led.sample_app(130, 3, 1000, 1, 0), "");
    led.privacy_on = true;

    // 相关性：合成同形状曲线（模型升 → 每段掉电量随之升）→ 高相关。
    // 幅度控制在 permille 量程内（总掉电 186‰ < 900 起点）。
    let mut led = BatteryLedger::new();
    let mut level = 900u64;
    for seg in 0..12u64 {
        let base = 10_000 + seg * 5_000;
        for m in 0..60u64 {
            let t = seg * 60 + m;
            led.sample_app(t, 1, base, 10, 1);
        }
        level -= 10 + seg; // 段掉电量随 seg 线性增 → 与模型同形状
        led.note_gauge(seg * 60 + 59, level);
    }
    let corr = led.correlation_audit(0, 720, 12);
    set.add("correlation high", corr.map(|c| c > CORRELATION_LINE_X100) == Some(true), "");

    // 平坦实测 → None（不伪装）。
    let mut led = BatteryLedger::new();
    for seg in 0..12u64 {
        for m in 0..60u64 {
            let t = seg * 60 + m;
            led.sample_app(t, 1, 10_000, 1, 0);
            led.note_gauge(t, 500);
        }
    }
    set.add("flat gauge none", led.correlation_audit(0, 720, 12).is_none(), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f060_ranking_three_windows() {
        let mut led = BatteryLedger::new();
        // 今天：应用 7 高耗；昨天：应用 8 高耗。
        let today0 = 2 * 1440;
        for m in 0..60u64 {
            led.sample_app(today0 + m, 7, 50_000, 30, 5);
        }
        let yesterday = today0 - 1440;
        for m in 0..60u64 {
            led.sample_app(yesterday + m, 8, 90_000, 40, 9);
        }
        let hour = led.ranking(RankWindow::Hour, today0 + 60, 5);
        assert_eq!(hour[0].app_id, 7);
        let today = led.ranking(RankWindow::Today, today0 + 60, 5);
        assert_eq!(today[0].app_id, 7);
        let week = led.ranking(RankWindow::Week, today0 + 60, 5);
        // 周窗里两者都在；90k 权重下的应用 8 总量更大。
        let ids: Vec<u32> = week.iter().map(|r| r.app_id).collect();
        assert!(ids.contains(&7) && ids.contains(&8));
        assert_eq!(week[0].app_id, 8);
    }

    #[test]
    fn f060_breakdown_columns() {
        let mut led = BatteryLedger::new();
        // 采样落在 Hour 窗 (now-60, now] 内。
        led.sample_app(90, 5, 2000, 4, 3);
        let (cpu, wake, io) = led.breakdown(5, RankWindow::Hour, 100).unwrap();
        assert_eq!((cpu, wake, io), (2000, 4, 3));
        assert!(led.breakdown(99, RankWindow::Hour, 100).is_none());
    }

    #[test]
    fn f060_system_item_separate() {
        let mut led = BatteryLedger::new();
        led.sample_app(5, 1, 1000, 0, 0);
        led.sample_system(5, 500_000); // 屏幕亮度 mu 直记
        let rank = led.ranking(RankWindow::Hour, 10, 10);
        // 系统项在排行尾部单列（app_id=0 行）。
        let sys_row = rank.last().unwrap();
        assert_eq!(sys_row.app_id, 0);
        assert_eq!(sys_row.energy_mu, 500_000);
        // 模型总量含系统项。
        assert_eq!(led.modeled_mu(0, 100), 1000 * 1000 + 500_000);
    }

    #[test]
    fn f060_gauge_clamp_and_same_minute() {
        let mut led = BatteryLedger::new();
        led.note_gauge(10, 1500); // 钳到 1000
        led.note_gauge(10, 900); // 同分钟覆写
        led.note_gauge(70, 800);
        assert_eq!(led.measured_drain_permille(0, 100), Some(100));
        assert_eq!(led.measured_drain_permille(80, 100), Some(0));
    }

    #[test]
    fn f060_weight_clamp() {
        let mut led = BatteryLedger::new();
        led.set_weights(0, 0, 0); // 全被钳到下限
        assert_eq!(led.weights().cpu_mu, 100);
        assert_eq!(led.weights().wake_mu, 1);
        assert_eq!(led.weights().io_mb_mu, 1);
    }

    #[test]
    fn f060_app_cap_lru_archive_first() {
        let mut led = BatteryLedger::new();
        // 填满 + 一个长活应用。
        for id in 0..(APP_CAP - 1) as u32 {
            led.sample_app(id as u64, id, 100, 0, 0);
        }
        led.sample_app(999, 999, 1_000_000, 0, 0); // 长活
        led.archive_app(0); // 归档最老
        // 再挤一个进来：归档者先逐出。
        led.sample_app(APP_CAP as u64, APP_CAP as u32, 100, 0, 0);
        assert_eq!(led.apps.len(), APP_CAP);
        assert!(led.apps.iter().all(|a| a.app_id != 0), "archived evicted first");
        assert!(led.apps.iter().any(|a| a.app_id == 999), "heavy app survives");
        assert!(led.clamped_apps == 0, "archived eviction not counted as clamp");
    }

    #[test]
    fn f060_pearson_edges() {
        // 完全线性正相关 → 100。
        let lin = [(1u64, 2u64), (2, 4), (3, 6), (4, 8)];
        assert_eq!(pearson_x100(&lin), Some(100));
        // 完全线性负相关 → -100。
        let inv = [(1u64, 8u64), (2, 6), (3, 4), (4, 2)];
        assert_eq!(pearson_x100(&inv), Some(-100));
        // y 零方差 → None。
        let flat = [(1u64, 5u64), (2, 5), (3, 5)];
        assert_eq!(pearson_x100(&flat), None);
        // 样本不足。
        assert_eq!(pearson_x100(&[(1u64, 1u64)]), None);
        // 整数平方根。
        assert_eq!(isqrt_u128(0), 0);
        assert_eq!(isqrt_u128(1), 1);
        assert_eq!(isqrt_u128(99), 9);
        assert_eq!(isqrt_u128(100), 10);
    }

    #[test]
    fn f060_run_checks_pass() {
        assert!(run_battery_checks().all_passed());
    }
}
