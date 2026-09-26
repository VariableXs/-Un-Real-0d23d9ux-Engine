//! F060 电量账本（perfstar2 · G-B-20）——看不懂的耗电才是焦虑，看得见的耗电是选择。
//!
//! 主册判据（验收标准第一句）：
//! **折算值与实测电量计曲线相关性 >0.8（同窗口对比）；排行前三与人工判断
//! 一致（抽查场景）。**
//!
//! 功能定义（G-B-20）：按应用能耗画像——CPU 时间 × 频率档 + 唤醒次数 +
//! IO 量折算为相对能耗单位；设置中心电池页显示「今天谁在耗电」排行。
//!
//! 【交互设计】应用排行条形图（相对单位，不伪精确瓦特数——诚实原则）+
//! 每应用明细（唤醒/IO/CPU 三分项）；时间范围切换（1h/今天/7 天）。
//! 【数据与存储】画像每分钟聚合入账本；保留 30 天；排行计算在设置页打开
//! 时现算（账本数据轻量）。
//! 【状态与异常】电量计不可读（部分机器）→ 页面显示「耗电相对值」并说明
//! 口径（诚实降级）；应用已卸载 → 画像归档不删除。
//! 【设计细节】权重初值：CPU 每毫秒核秒=1.0 / 唤醒每次=0.05 / IO 每 MB=
//! 0.02（参数进旋钮清单）；唤醒合并（F050）效果在画像里自动体现；屏幕亮度
//! 不计入应用（归系统项单列）；数据受隐私总闸（F036 开关）管理。
//!
//! 存储结构（一处一事实）：小时明细环 168 桶（7 天——覆盖 1h/今天/7 天三档
//! 视图）+ 日汇总环 30 桶（30 天留存口径）。每桶带**时间标签**（u16 环回
//! 模），查询先验标签再取数——空窗跳档后旧数据不可能冒充新数据（环历史
//! 查询的诚实性根基）。零堆：全定长，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 折算权重（主册明文初值，进旋钮清单）：CPU 每 ms（单核归一）= 1.0。
pub const W_CPU_PER_MS: u32 = 1_000; // ×1000 定点：1.0 → 1000
/// 唤醒每次 = 0.05（×1000 定点 = 50）。
pub const W_WAKEUP_EACH: u32 = 50;
/// IO 每 MB = 0.02（×1000 定点 = 20）。
pub const W_IO_PER_MB: u32 = 20;
/// 画像分钟聚合（主册：每分钟聚合入账本）。
pub const AGG_INTERVAL_MS: u64 = 60_000;
/// 小时明细环：7 天 × 24 = 168 桶（三档视图的最长档）。
pub const HOUR_BUCKETS: usize = 168;
/// 日汇总环：30 天（主册：保留 30 天）。
pub const DAY_BUCKETS: usize = 30;
/// 应用表容量（含归档位）。
pub const APP_CAP: usize = 32;
/// 频率档因子（×100 定点，Y7000 量级；_PSS 实表随闸门接线）。
pub const FREQ_FACTOR_PCT: [u32; 8] = [100, 95, 85, 72, 60, 45, 32, 20];
/// 相关性判据线（主册：>0.8），×1000 定点。
pub const CORRELATION_X1000_MIN: i64 = 800;
/// 桶值钳制（防单桶溢出撑爆账本）。
pub const PER_BUCKET_CAP: u32 = 1_000_000;

/// 应用能耗画像条目。
#[derive(Clone, Copy, Debug)]
pub struct AppEnergy {
    pub name: [u8; 24],
    pub name_len: u8,
    /// 已卸载归档旗标（画像归档不删除——主册纪律）。
    pub archived: bool,
    /// 小时明细环（CPU ms 折算 / 唤醒 / IO KB 三分项）。
    cpu_ms: [u32; HOUR_BUCKETS],
    wakeups: [u32; HOUR_BUCKETS],
    io_kb: [u32; HOUR_BUCKETS],
    hour_tag: [u16; HOUR_BUCKETS],
    /// 日汇总环（30 天留存）。
    day_cpu: [u32; DAY_BUCKETS],
    day_wake: [u32; DAY_BUCKETS],
    day_io: [u32; DAY_BUCKETS],
    day_tag: [u16; DAY_BUCKETS],
    /// 分钟内累计器。
    acc_cpu: u32,
    acc_wake: u32,
    acc_io: u32,
}

impl AppEnergy {
    pub const fn new() -> Self {
        AppEnergy {
            name: [0; 24],
            name_len: 0,
            archived: false,
            cpu_ms: [0; HOUR_BUCKETS],
            wakeups: [0; HOUR_BUCKETS],
            io_kb: [0; HOUR_BUCKETS],
            hour_tag: [0xFFFF; HOUR_BUCKETS], // 0xFFFF = 从未写过
            day_cpu: [0; DAY_BUCKETS],
            day_wake: [0; DAY_BUCKETS],
            day_io: [0; DAY_BUCKETS],
            day_tag: [0xFFFF; DAY_BUCKETS],
            acc_cpu: 0,
            acc_wake: 0,
            acc_io: 0,
        }
    }

    fn set_name(&mut self, name: &[u8]) {
        let n = name.len().min(24);
        self.name[..n].copy_from_slice(&name[..n]);
        self.name_len = n as u8;
    }

    pub fn name_str(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
}

/// 折算单位能耗（×1000 定点相对单位）。
#[derive(Clone, Copy, Debug)]
pub struct EnergyUnit(pub u64);

/// 排行榜条目（设置页打开时现算）。
#[derive(Clone, Copy, Debug)]
pub struct RankRow {
    pub app_index: usize,
    pub energy: EnergyUnit,
    pub archived: bool,
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 电量账本。
pub struct PowerLedger {
    apps: [AppEnergy; APP_CAP],
    app_n: usize,
    /// 电量计可读性（诚实降级旗标）。
    gauge_readable: bool,
    /// 隐私总闸（F036）：关 → 停止记账（既有画像冻结不删）。
    privacy_gate_open: bool,
    /// 电量计曲线环（百分比 ×10 定点 0..=1000）。
    gauge_ring: [u16; 64],
    gauge_head: usize,
    gauge_n: usize,
    /// 系统项单列（屏幕亮度等——不计入应用）。
    sys_cpu_ms: u64,
    sys_wakeups: u64,
    sys_io_kb: u64,
    now_ms: u64,
}

impl PowerLedger {
    pub const fn new() -> Self {
        PowerLedger {
            apps: [AppEnergy::new(); APP_CAP],
            app_n: 0,
            gauge_readable: true,
            privacy_gate_open: true,
            gauge_ring: [0; 64],
            gauge_head: 0,
            gauge_n: 0,
            sys_cpu_ms: 0,
            sys_wakeups: 0,
            sys_io_kb: 0,
            now_ms: 0,
        }
    }

    /// 注册应用（无则建档；已卸载重装 → 复位归档旗标续用画像）。
    pub fn ensure_app(&mut self, name: &[u8]) -> usize {
        for i in 0..self.app_n {
            if self.apps[i].name_str() == name {
                self.apps[i].archived = false;
                return i;
            }
        }
        if self.app_n == APP_CAP {
            return usize::MAX; // 表满：诚实拒绝
        }
        let idx = self.app_n;
        self.apps[idx] = AppEnergy::new();
        self.apps[idx].set_name(name);
        self.app_n += 1;
        idx
    }

    /// 应用卸载：画像归档不删除。
    pub fn mark_uninstalled(&mut self, idx: usize) {
        if idx < self.app_n {
            self.apps[idx].archived = true;
        }
    }

    /// 记账入口：每分钟聚合落账（at_ms 对齐分钟界才提交）。
    pub fn record_minute(
        &mut self,
        idx: usize,
        cpu_ms: u32,
        freq_level: u8,
        wakeups: u32,
        io_kb: u32,
        at_ms: u64,
    ) {
        self.now_ms = at_ms;
        if !self.privacy_gate_open || idx >= self.app_n {
            return; // 隐私闸关闭 / 非法下标：不记（闸语义，非异常吞）
        }
        let app = &mut self.apps[idx];
        let f = FREQ_FACTOR_PCT[(freq_level as usize).min(7)] as u64;
        app.acc_cpu = app.acc_cpu.saturating_add(((cpu_ms as u64) * f / 100).min(PER_BUCKET_CAP as u64) as u32);
        app.acc_wake = app.acc_wake.saturating_add(wakeups);
        app.acc_io = app.acc_io.saturating_add(io_kb);
        if at_ms % AGG_INTERVAL_MS == 0 {
            Self::commit(app, at_ms);
        }
    }

    /// 分钟提交：写入带标签的小时桶 + 日桶（标签 = 环回模，空窗免疫）。
    fn commit(app: &mut AppEnergy, at_ms: u64) {
        let hour = at_ms / 3_600_000;
        let hi = (hour % HOUR_BUCKETS as u64) as usize;
        let htag = (hour % 0xFFFE) as u16;
        if app.hour_tag[hi] != htag {
            // 新小时：清桶再写（同小时重复提交为累加）。
            app.hour_tag[hi] = htag;
            app.cpu_ms[hi] = 0;
            app.wakeups[hi] = 0;
            app.io_kb[hi] = 0;
        }
        app.cpu_ms[hi] = app.cpu_ms[hi].saturating_add(app.acc_cpu).min(PER_BUCKET_CAP);
        app.wakeups[hi] = app.wakeups[hi].saturating_add(app.acc_wake).min(PER_BUCKET_CAP);
        app.io_kb[hi] = app.io_kb[hi].saturating_add(app.acc_io).min(PER_BUCKET_CAP);
        let day = at_ms / 86_400_000;
        let di = (day % DAY_BUCKETS as u64) as usize;
        let dtag = (day % 0xFFFE) as u16;
        if app.day_tag[di] != dtag {
            app.day_tag[di] = dtag;
            app.day_cpu[di] = 0;
            app.day_wake[di] = 0;
            app.day_io[di] = 0;
        }
        app.day_cpu[di] = app.day_cpu[di].saturating_add(app.acc_cpu).min(PER_BUCKET_CAP);
        app.day_wake[di] = app.day_wake[di].saturating_add(app.acc_wake).min(PER_BUCKET_CAP);
        app.day_io[di] = app.day_io[di].saturating_add(app.acc_io).min(PER_BUCKET_CAP);
        app.acc_cpu = 0;
        app.acc_wake = 0;
        app.acc_io = 0;
    }

    /// 系统项单列记账（屏幕亮度等）。
    pub fn record_system(&mut self, cpu_ms: u32, wakeups: u32, io_kb: u32, at_ms: u64) {
        if !self.privacy_gate_open {
            return;
        }
        self.sys_cpu_ms = self.sys_cpu_ms.saturating_add(cpu_ms as u64);
        self.sys_wakeups = self.sys_wakeups.saturating_add(wakeups as u64);
        self.sys_io_kb = self.sys_io_kb.saturating_add(io_kb as u64);
        self.now_ms = at_ms;
    }

    /// 电量计曲线采样（不可读 → 诚实降级旗标）。
    pub fn sample_gauge(&mut self, pct_permille: u16, readable: bool) {
        self.gauge_readable = readable;
        if readable {
            self.gauge_ring[self.gauge_head] = pct_permille.min(1000);
            self.gauge_head = (self.gauge_head + 1) % 64;
            self.gauge_n = (self.gauge_n + 1).min(64);
        }
    }

    pub fn gauge_readable(&self) -> bool {
        self.gauge_readable
    }

    pub fn set_privacy_gate(&mut self, open: bool) {
        self.privacy_gate_open = open;
    }

    /// 折算单应用能耗（相对单位 ×1000 定点；`since_ms` 起的窗口；0 = 全窗）。
    pub fn energy_since(&self, idx: usize, since_ms: u64, at_ms: u64) -> EnergyUnit {
        if idx >= self.app_n {
            return EnergyUnit(0);
        }
        let app = &self.apps[idx];
        let mut cpu = 0u64;
        let mut wk = 0u64;
        let mut io = 0u64;
        let at_hour = at_ms / 3_600_000;
        let at_mod = (at_hour % 0xFFFE) as u16;
        // 小时明细环（标签校验：标签不符 = 空窗/旧数据，跳过；
        // 标签大于查询时刻 = 桶属未来小时——同样跳过，不冒充历史）。
        for i in 0..HOUR_BUCKETS {
            if app.hour_tag[i] == 0xFFFF {
                continue;
            }
            let tag = app.hour_tag[i];
            if tag > at_mod {
                continue;
            }
            let hour = at_hour - (at_mod - tag) as u64;
            let t = hour * 3_600_000;
            if t >= since_ms && t <= at_ms {
                cpu += app.cpu_ms[i] as u64;
                wk += app.wakeups[i] as u64;
                io += app.io_kb[i] as u64;
            }
        }
        energy_from(cpu, wk, io)
    }

    pub fn energy_of(&self, idx: usize, at_ms: u64) -> EnergyUnit {
        self.energy_since(idx, 0, at_ms)
    }

    /// 30 天日汇总口径（留存证明用：小时环 7 天之外的窗口走日桶）。
    pub fn energy_30d(&self, idx: usize, at_ms: u64) -> EnergyUnit {
        if idx >= self.app_n {
            return EnergyUnit(0);
        }
        let app = &self.apps[idx];
        let mut cpu = 0u64;
        let mut wk = 0u64;
        let mut io = 0u64;
        let at_day = at_ms / 86_400_000;
        let at_mod = (at_day % 0xFFFE) as u16;
        for i in 0..DAY_BUCKETS {
            if app.day_tag[i] == 0xFFFF {
                continue;
            }
            let tag = app.day_tag[i];
            if tag > at_mod {
                continue;
            }
            let day = at_day - (at_mod - tag) as u64;
            let t = day * 86_400_000;
            if t <= at_ms {
                cpu += app.day_cpu[i] as u64;
                wk += app.day_wake[i] as u64;
                io += app.day_io[i] as u64;
            }
        }
        energy_from(cpu, wk, io)
    }

    /// 排行（现算；窗口钉在小时明细环真实跨度 7 天内——旧桶不冒充「今天」；
    /// 归档应用降位但保留）。
    pub fn rank(&self, at_ms: u64, out: &mut [RankRow]) -> usize {
        let since = at_ms.saturating_sub(HOUR_BUCKETS as u64 * 3_600_000);
        let mut n = 0usize;
        for i in 0..self.app_n {
            let e = self.energy_since(i, since, at_ms);
            let row = RankRow { app_index: i, energy: e, archived: self.apps[i].archived };
            let mut j = n;
            while j > 0
                && (out[j - 1].energy.0 < row.energy.0
                    || (out[j - 1].energy.0 == row.energy.0 && out[j - 1].archived && !row.archived))
            {
                if j < out.len() {
                    out[j] = out[j - 1];
                }
                j -= 1;
            }
            if n < out.len() {
                out[j] = row;
                n += 1;
            } else if j < out.len() {
                out[j] = row;
            }
        }
        n
    }

    /// 相关性自证：折算总量增量 vs 电量计掉电速率的皮尔逊 r（×1000 定点）。
    /// 采样轴线性等分（t_k = span×(k+1)——不随 prev 滚动累加，二次增长
    /// 的时间轴会让增量序列失真，见缺陷账本 K2-#3）。
    pub fn correlation_x1000(&self, at_ms: u64) -> i64 {
        if self.gauge_n < 3 || self.app_n == 0 {
            return 0; // 样本不足不评（诚实口径）
        }
        let n = self.gauge_n as usize;
        let mut xs = [0i64; 64];
        let mut ys = [0i64; 64];
        let mut cnt = 0usize;
        let span_ms = (at_ms / n as u64).max(1);
        let mut prev_t = 0u64;
        for k in 0..n {
            let t = span_ms * (k as u64 + 1);
            let e_prev = self.total_energy_at(prev_t);
            let e_cur = self.total_energy_at(t);
            xs[cnt] = e_cur.saturating_sub(e_prev) as i64;
            // 电量计侧：相邻采样掉电量（掉得越快消耗越大）。
            let idx = (self.gauge_head + 64 - n + k) % 64;
            ys[cnt] = if k == 0 { 0 } else {
                let prev_idx = (idx + 63) % 64;
                (self.gauge_ring[prev_idx] as i64 - self.gauge_ring[idx] as i64).max(0)
            };
            cnt += 1;
            prev_t = t;
        }
        pearson_x1000(&xs[..cnt], &ys[..cnt])
    }

    fn total_energy_at(&self, at_ms: u64) -> u64 {
        let mut total = 0u64;
        for i in 0..self.app_n {
            total += self.energy_of(i, at_ms).0;
        }
        total
    }

    pub fn app_name(&self, idx: usize) -> &[u8] {
        self.apps.get(idx).map(|a| a.name_str()).unwrap_or(&[])
    }

    pub fn is_archived(&self, idx: usize) -> bool {
        self.apps.get(idx).map_or(false, |a| a.archived)
    }

    pub fn app_count(&self) -> usize {
        self.app_n
    }

    /// 系统项单列能耗。
    pub fn system_energy(&self) -> EnergyUnit {
        energy_from(self.sys_cpu_ms, self.sys_wakeups, self.sys_io_kb)
    }
}

/// 权重公式（×1000 定点相对单位）。
fn energy_from(cpu_ms: u64, wakeups: u64, io_kb: u64) -> EnergyUnit {
    let io_mb_x1000 = io_kb * 1000 / 1024;
    EnergyUnit(
        cpu_ms * W_CPU_PER_MS as u64
            + wakeups * W_WAKEUP_EACH as u64
            + io_mb_x1000 * W_IO_PER_MB as u64 / 1000,
    )
}

/// 皮尔逊 r ×1000 定点（零方差 → 0 = 不相关，诚实保守口径）。
fn pearson_x1000(xs: &[i64], ys: &[i64]) -> i64 {
    let n = xs.len().min(ys.len());
    if n < 3 {
        return 0;
    }
    let mut sx = 0i64;
    let mut sy = 0i64;
    for i in 0..n {
        sx += xs[i];
        sy += ys[i];
    }
    let mx = sx / n as i64;
    let my = sy / n as i64;
    let mut sxy = 0i64;
    let mut sxx = 0i64;
    let mut syy = 0i64;
    for i in 0..n {
        let dx = xs[i] - mx;
        let dy = ys[i] - my;
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    if sxx == 0 || syy == 0 {
        return 0;
    }
    let denom = (sxx as f64 * syy as f64).sqrt();
    let r = sxy as f64 / denom;
    (r * 1000.0).round() as i64
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_powerledger_checks() -> CheckSet {
    let mut cs = CheckSet::new("F060-powerledger");
    // 1) 权重公式：CPU 1000ms 满频 + 20 唤醒 + 1MB IO。
    let mut l = PowerLedger::new();
    let a = l.ensure_app(b"sync-tool");
    l.record_minute(a, 1_000, 0, 20, 1024, 60_000);
    let e = l.energy_of(a, 3_600_000);
    // cpu 1000×100/100=1000ms→1_000_000；wake 20×50=1_000；io 1MB→20。
    cs.add("weight_formula_cpu_wake_io", e.0 == 1_000_000 + 1_000 + 20, "");
    // 2) 频率档折算：同 CPU 时间低频档能耗更低（×100 因子）。
    let mut l2 = PowerLedger::new();
    let b = l2.ensure_app(b"downloader");
    l2.record_minute(b, 1_000, 0, 0, 0, 60_000);
    let e_full = l2.energy_of(b, 3_600_000);
    l2.record_minute(b, 1_000, 7, 0, 0, 120_000);
    let e_low = l2.energy_of(b, 3_600_000);
    cs.add("freq_factor_scales", e_low.0 == e_full.0 + 200_000, "");
        // 3) 排行与投入量一致（重载应用列前）。
        let mut l3 = PowerLedger::new();
        let h1 = l3.ensure_app(b"heavy-game");
        let h2 = l3.ensure_app(b"idle-widget");
        let h3 = l3.ensure_app(b"mid-editor");
        l3.record_minute(h1, 60_000, 0, 300, 0, 60_000);
        l3.record_minute(h2, 100, 7, 2, 0, 60_000);
        l3.record_minute(h3, 10_000, 3, 30, 0, 60_000);
        let mut rows = [RankRow { app_index: 0, energy: EnergyUnit(0), archived: false }; APP_CAP];
        let n = l3.rank(3_600_000, &mut rows);
        cs.add(
            "rank_top3_by_effort",
            n == 3
                && l3.app_name(rows[0].app_index) == b"heavy-game"
                && l3.app_name(rows[1].app_index) == b"mid-editor"
                && l3.app_name(rows[2].app_index) == b"idle-widget",
            "",
        );
        // 4) 归档不删除：卸载应用仍在账；同能耗时归档降位（活跃在前）。
        let mut l3b = PowerLedger::new();
        let p1 = l3b.ensure_app(b"active-app");
        let p2 = l3b.ensure_app(b"archived-app");
        l3b.record_minute(p1, 10_000, 3, 30, 0, 60_000);
        l3b.record_minute(p2, 10_000, 3, 30, 0, 60_000); // 与活跃应用同能耗
        l3b.mark_uninstalled(p2);
        cs.add("archived_kept_in_ledger", l3b.is_archived(p2) && l3b.app_count() == 2, "");
        let mut rows4 = [RankRow { app_index: 0, energy: EnergyUnit(0), archived: false }; APP_CAP];
        let n4 = l3b.rank(3_600_000, &mut rows4);
        cs.add(
            "archived_ranked_last",
            n4 == 2
                && l3b.app_name(rows4[0].app_index) == b"active-app"
                && l3b.app_name(rows4[1].app_index) == b"archived-app",
            "",
        );
    // 5) 电量计不可读 → 诚实降级旗标。
    let mut l5 = PowerLedger::new();
    l5.sample_gauge(500, false);
    cs.add("gauge_unreadable_degrades", !l5.gauge_readable(), "");
    // 6) 隐私总闸关闭 → 停止记账（既有画像冻结）。
    let mut l6 = PowerLedger::new();
    let c = l6.ensure_app(b"private-app");
    l6.record_minute(c, 1_000, 0, 1, 0, 60_000);
    l6.set_privacy_gate(false);
    l6.record_minute(c, 50_000, 0, 100, 0, 120_000);
    cs.add("privacy_gate_freezes", l6.energy_of(c, 3_600_000).0 == 1_000_000 + 50, "");
    // 7) 系统项单列：亮度类系统耗电不计入任何应用。
    let mut l7 = PowerLedger::new();
    let d = l7.ensure_app(b"normal-app");
    l7.record_minute(d, 1_000, 0, 1, 0, 60_000);
    l7.record_system(100_000, 500, 0, 60_000);
    cs.add("system_item_separate", l7.energy_of(d, 3_600_000).0 < l7.system_energy().0, "");
    // 8) 相关性自证：能耗升 → 电量计同步加速掉，r > 0.8（小时粒度对齐）。
    let mut l8 = PowerLedger::new();
    let s = l8.ensure_app(b"corr-app");
    let mut cum_drop = 0i64;
    for k in 0..10u64 {
        let t = (k + 1) * 3_600_000;
        l8.record_minute(s, 1_000 * (k as u32 + 1), 0, 10, 0, t);
        cum_drop += 10 * (k as i64 + 1); // 掉电速率随能耗同步升
        l8.sample_gauge((1000 - cum_drop as u16).max(0), true);
    }
    let r = l8.correlation_x1000(10 * 3_600_000);
    cs.add("correlation_above_08", r > CORRELATION_X1000_MIN, "");
    // 9) 恒速掉电 vs 波动能耗：r < 0.8（判据不是摆设）。
    let mut l9 = PowerLedger::new();
    let s9 = l9.ensure_app(b"noise-app");
    let sq = [10u32, 90, 30, 70, 50, 60, 40, 80, 20, 100];
    for (k, v) in sq.iter().enumerate() {
        l9.record_minute(s9, *v * 100, 0, 10, 0, (k as u64 + 1) * 3_600_000);
    }
    for k in 0..10u16 {
        l9.sample_gauge(1000 - (k + 1) * 50, true);
    }
    cs.add("uncorrelated_scores_low", l9.correlation_x1000(10 * 3_600_000) < CORRELATION_X1000_MIN, "");
    // 10) 空窗跳档：中间空 5 小时后旧桶不冒充新数据（标签校验）。
    let mut l10 = PowerLedger::new();
    let t = l10.ensure_app(b"gap-app");
    l10.record_minute(t, 5_000, 0, 5, 0, 3_600_000); // hour 1
    l10.record_minute(t, 1_000, 0, 1, 0, 6 * 3_600_000); // hour 6（跳 4 小时）
    // 查 hour 2-5 窗口：应只含 hour 1 之外为零（hour 1 桶不在窗口内）。
    let e_gap = l10.energy_since(t, 2 * 3_600_000, 5 * 3_600_000);
    cs.add("gap_hours_honest_zero", e_gap.0 == 0, "");
    let e_all = l10.energy_of(t, 6 * 3_600_000);
    // hour1: 5000ms→5_000_000 + 5×50=250；hour6: 1000ms→1_000_000+50。
    cs.add("gap_windows_sum_correct", e_all.0 == 5_000_000 + 250 + 1_000_000 + 50, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_ring_survives_30_day_roll() {
        let mut l = PowerLedger::new();
        let a = l.ensure_app(b"roller");
        // 打 800 小时点（>168 桶环）：标签校验下查询只回最近 168h 有数据桶。
        for h in 0..800u64 {
            l.record_minute(a, 1_000, 0, 1, 0, h * 3_600_000);
        }
        // 最近 168 小时（h 632..800）应有 168 桶 × (1000ms CPU + 1 唤醒)。
        let e_recent = l.energy_since(a, 632 * 3_600_000, 800 * 3_600_000);
        assert_eq!(e_recent.0, 168 * (1_000_000 + 50), "168h 环精确窗口");
        // 全窗查询：环外桶的标签已过期（>168h 前的桶被新数据覆盖或标签不符），
        // 只有现存桶计入——不会重复计 800 桶。
        let e_all = l.energy_of(a, 800 * 3_600_000);
        assert!(e_all.0 <= 168 * (1_000_000 + 50), "环外旧桶不冒充");
    }

    #[test]
    fn day_bucket_retains_beyond_hour_window() {
        let mut l = PowerLedger::new();
        let a = l.ensure_app(b"old-data");
        // 第 1 天记账。
        l.record_minute(a, 10_000, 0, 10, 0, 3_600_000);
        // 第 29 天再记账（小时环已远超 168h 滚走）。
        l.record_minute(a, 2_000, 0, 2, 0, 29 * 86_400_000);
        // 30 天日桶口径下两段都在。
        let e = l.energy_30d(a, 30 * 86_400_000);
        let expect = 10_000_000 + 10 * 50 + 2_000_000 + 2 * 50;
        assert_eq!(e.0, expect as u64, "日桶 30 天留存");
    }

    #[test]
    fn reinstall_resumes_profile() {
        let mut l = PowerLedger::new();
        let a = l.ensure_app(b"reinstall-app");
        l.record_minute(a, 5_000, 0, 5, 0, 60_000);
        l.mark_uninstalled(a);
        let b = l.ensure_app(b"reinstall-app");
        assert_eq!(a, b, "同名复用槽位");
        assert!(!l.is_archived(b));
        l.record_minute(b, 1_000, 0, 1, 0, 120_000);
        assert!(l.energy_of(b, 3_600_000).0 > 5_000_000, "两段画像累计");
    }

    #[test]
    fn app_cap_honest_rejection() {
        let mut l = PowerLedger::new();
        for i in 0..APP_CAP {
            // 32 个互异名（base/offset 双字母编码）。
            let name: [u8; 2] = [b'a' + (i / 26) as u8, b'a' + (i % 26) as u8];
            assert_ne!(l.ensure_app(&name), usize::MAX);
        }
        assert_eq!(l.ensure_app(b"overflow"), usize::MAX, "表满诚实拒绝");
    }

    #[test]
    fn freq_factor_table_bounded() {
        assert_eq!(FREQ_FACTOR_PCT.len(), 8);
        assert!(FREQ_FACTOR_PCT[0] == 100 && FREQ_FACTOR_PCT[7] == 20);
        for w in FREQ_FACTOR_PCT.windows(2) {
            assert!(w[0] >= w[1], "因子单调不升");
        }
    }

    #[test]
    fn correlation_zero_variance_scores_zero() {
        let mut l = PowerLedger::new();
        let a = l.ensure_app(b"flat");
        for k in 0..5u64 {
            l.record_minute(a, 1_000, 0, 1, 0, (k + 1) * 60_000);
            l.sample_gauge(1000 - (k as u16 + 1) * 10, true);
        }
        // 能耗恒定（零方差）→ r = 0：不相关性诚实保守。
        assert_eq!(l.correlation_x1000(300_000), 0);
    }
}
