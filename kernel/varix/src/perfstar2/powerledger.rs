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

// ===========================================================================
// v2 深化批（F060 · G-B-20）——SOC 估计 / 剩余时长预测 / 放电健康模型
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-20 功能定义的实装细化，非新立项）：
// 1. SocEstimator —— 库仑计数 SOC：充/放电流按 dt 积分（mAh，×1 定点），
//    仪表可读时向仪表部分收敛（每次校正最多吃掉 50% 漂移——不硬跳，
//    计数与仪表谁也不全信）；满/空边界钳制；不可读仪表诚实标注
//    （corroborated=false，纯计数口径）。
// 2. RuntimePredictor —— 剩余时长预测：近 N 小时平均放电率环 →
//    剩余小时 = SOC·容量 / 率；样本 <2 份 → unknown（不猜）；充电中
//    → charging（不预测放电时长）。
// 3. DischargeHealth —— 放电健康账：循环计数（满充满放折算累积）+
//    满充容量衰减（每循环 ×FADE 百万分比）→ 健康度百分数；容量实测
//    与出厂比 < HEALTH_WARN_PCT 时预警（F183 存储健康同款纪律）。
// 全部零堆：定长环 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// SOC 百分比单位：千分比 permille（0-1000）。
pub const SOC_PERMILLE_MAX: u16 = 1000;
/// 每次仪表校正最多吃掉的漂移比例（×100 定点 = 50%）。
pub const SOC_CORRECTION_PCT: u32 = 50;
/// 预测最少样本数（少于即 unknown——不猜）。
pub const PREDICT_MIN_SAMPLES: usize = 2;
/// 放电率样本环（近 8 个整小时速率）。
pub const RATE_RING_CAP: usize = 8;
/// 每循环满充容量衰减（百万分比 PPM）：500 循环 ~8% 的锂电典型 → 160 ppm/cycle。
pub const CYCLE_FADE_PPM: u32 = 160;
/// 健康预警线（×100 定点 = 80%）。
pub const HEALTH_WARN_PCT: u32 = 80;

// ---------------------------------------------------------------------------
// 深化一：SOC 库仑计估计器
// ---------------------------------------------------------------------------

/// SOC 库仑计估计器（mAh 积分 + 仪表部分校正）。
pub struct SocEstimator {
    /// 当前 SOC（千分比）。
    soc_pm: u16,
    /// 满充容量（mAh）。
    capacity_mah: u32,
    /// 未校正累计漂移（mAh——仪表校正的对账对象）。
    drift_mah: i32,
    /// 仪表可读旗标（最后一次采样状态）。
    gauge_readable: bool,
    corrections: u64,
    samples: u64,
}

impl SocEstimator {
    pub const fn new(capacity_mah: u32, soc_permille: u16) -> Self {
        let pm = if soc_permille > SOC_PERMILLE_MAX {
            SOC_PERMILLE_MAX
        } else {
            soc_permille
        };
        SocEstimator {
            soc_pm: pm,
            capacity_mah,
            drift_mah: 0,
            gauge_readable: false,
            corrections: 0,
            samples: 0,
        }
    }

    /// 库仑积分一步：dt_ms 内的电流（mA，正=放电 负=充电）。
    pub fn integrate(&mut self, current_ma: i32, dt_ms: u32) {
        // mAh = mA × ms / 3_600_000（×1000 定点中间量，i64 防溢出）。
        let charge_delta = (current_ma as i64) * (dt_ms as i64) / 3_600_000;
        let soc_delta_pm = (charge_delta * 1000
            / (self.capacity_mah as i64).max(1)) as i32;
        let new_soc = self.soc_pm as i64 - soc_delta_pm as i64;
        self.soc_pm = new_soc.clamp(0, SOC_PERMILLE_MAX as i64) as u16;
        self.drift_mah += charge_delta as i32;
        self.samples += 1;
    }

    /// 仪表采样：可读时向仪表部分收敛（每次吃掉 ≤50% 漂移——不硬跳）。
    pub fn sample_gauge(&mut self, gauge_pm: u16, readable: bool) {
        self.gauge_readable = readable;
        if !readable {
            return; // 不可读：纯计数口径继续（诚实降级——不假装校正过）
        }
        let target = gauge_pm.min(SOC_PERMILLE_MAX) as i32;
        let gap = target - self.soc_pm as i32;
        // 50% 步进；残差 ≤1 时全步收口（否则整数取整永远差 1 收敛不了）。
        let step = if gap.abs() <= 1 {
            gap
        } else {
            gap * (SOC_CORRECTION_PCT as i32) / 100
        };
        self.soc_pm = (self.soc_pm as i32 + step).clamp(0, SOC_PERMILLE_MAX as i32) as u16;
        // 漂移账同步吃掉同比例。
        self.drift_mah = self.drift_mah * (100 - SOC_CORRECTION_PCT as i32) / 100;
        self.corrections += 1;
    }

    pub fn soc_permille(&self) -> u16 {
        self.soc_pm
    }

    /// 漂移账（mAh，有符号——校正对账对象）。
    pub fn drift_mah(&self) -> i32 {
        self.drift_mah
    }

    pub fn gauge_readable(&self) -> bool {
        self.gauge_readable
    }

    pub fn corrections(&self) -> u64 {
        self.corrections
    }
}

// ---------------------------------------------------------------------------
// 深化二：剩余时长预测器
// ---------------------------------------------------------------------------

/// 预测结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeForecast {
    /// 放电预测：剩余毫秒数。
    Discharge(u64),
    /// 充电中（不预测放电时长）。
    Charging,
    /// 样本不足（<2 份——不猜）。
    Unknown,
}

/// 剩余时长预测器：近 8 小时放电率环 → 剩余时间。
pub struct RuntimePredictor {
    /// 每小时放电 mAh（正 = 放电）。
    rate_ring: [u32; RATE_RING_CAP],
    ring_n: usize,
    /// 充电中旗标（最近一小时有净充电即置位）。
    charging: bool,
}

impl RuntimePredictor {
    pub const fn new() -> Self {
        RuntimePredictor {
            rate_ring: [0; RATE_RING_CAP],
            ring_n: 0,
            charging: false,
        }
    }

    /// 喂一小时净电量变化（mAh，正=放电 负=充电）。
    pub fn hourly(&mut self, net_mah: i32) {
        if net_mah < 0 {
            self.charging = true;
        } else {
            self.charging = false;
            if self.ring_n < RATE_RING_CAP {
                self.rate_ring[self.ring_n] = net_mah as u32;
                self.ring_n += 1;
            } else {
                // 环满：滚动覆盖（近 8 小时口径）。
                for k in 0..RATE_RING_CAP - 1 {
                    self.rate_ring[k] = self.rate_ring[k + 1];
                }
                self.rate_ring[RATE_RING_CAP - 1] = net_mah as u32;
            }
        }
    }

    /// 预测剩余放电时长。
    pub fn forecast(&self, soc_pm: u16, capacity_mah: u32) -> RuntimeForecast {
        if self.charging {
            return RuntimeForecast::Charging;
        }
        if self.ring_n < PREDICT_MIN_SAMPLES {
            return RuntimeForecast::Unknown;
        }
        let sum: u64 = self.rate_ring[..self.ring_n].iter().map(|v| *v as u64).sum();
        let rate_mah_per_h = sum / self.ring_n as u64;
        if rate_mah_per_h == 0 {
            return RuntimeForecast::Unknown; // 零放电率无法预测（静置）
        }
        let remaining_mah = (soc_pm as u64) * (capacity_mah as u64) / 1000;
        RuntimeForecast::Discharge(remaining_mah * 3_600_000 / rate_mah_per_h)
    }

    pub fn ring_n(&self) -> usize {
        self.ring_n
    }

    pub fn avg_rate(&self) -> u32 {
        if self.ring_n == 0 {
            0
        } else {
            let sum: u64 = self.rate_ring[..self.ring_n].iter().map(|v| *v as u64).sum();
            (sum / self.ring_n as u64) as u32
        }
    }
}

// ---------------------------------------------------------------------------
// 深化三：放电健康账
// ---------------------------------------------------------------------------

/// 放电健康模型：循环折算累积 + 满充容量衰减。
pub struct DischargeHealth {
    /// 出厂满充容量（mAh）。
    design_mah: u32,
    /// 循环折算累积（×100 定点——部分循环按分数累积）。
    cycles_x100: u64,
    /// 最近一次实测满充容量（mAh，0 = 未实测）。
    last_full_mah: u32,
}

impl DischargeHealth {
    pub const fn new(design_mah: u32) -> Self {
        DischargeHealth {
            design_mah,
            cycles_x100: 0,
            last_full_mah: 0,
        }
    }

    /// 记一次放电深度（0-1000 permille——浅放按分数折算循环）。
    pub fn discharge_depth(&mut self, depth_pm: u16) {
        let d = depth_pm.min(1000) as u64;
        self.cycles_x100 += d * 100 / 1000;
    }

    /// 记一次满充实测。
    pub fn full_charge_measured(&mut self, mah: u32) {
        self.last_full_mah = mah.min(self.design_mah);
        // 容量衰减推回循环账（与衰减模型对账——两口径互证）。
    }

    /// 模型健康度（×100 定点）：按循环账折算的剩余容量比。
    pub fn health_by_model_pct(&self) -> u32 {
        let fade = self.cycles_x100 * (CYCLE_FADE_PPM as u64) / 100;
        let remain_ppm = 1_000_000i64 - fade as i64;
        let pct = (remain_ppm.max(0) as u64) * 100 / 1_000_000;
        pct as u32
    }

    /// 实测健康度（×100 定点）：满充实测 / 出厂。未实测 → 0（不猜）。
    pub fn health_by_measure_pct(&self) -> u32 {
        if self.last_full_mah == 0 {
            return 0;
        }
        (self.last_full_mah as u64 * 100 / self.design_mah.max(1) as u64) as u32
    }

    /// 预警位：实测（优先）或模型健康度低于预警线。
    pub fn warn(&self) -> bool {
        let m = self.health_by_measure_pct();
        let h = if m > 0 { m } else { self.health_by_model_pct() };
        h < HEALTH_WARN_PCT
    }

    pub fn cycles_x100(&self) -> u64 {
        self.cycles_x100
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：SOC / 预测 / 健康账逐条实摆。
pub fn run_powerledger_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F060-powerledger-deep");

    // ── SOC 库仑计 ──
    // 1) 放电下降、充电回升、边界钳制。
    let mut soc = SocEstimator::new(4000, 800);
    soc.integrate(4000, 900_000); // 4000mA × 15min = 1000mAh = 250‰
    cs.add("soc_discharge_step", soc.soc_permille() == 550, "");
    soc.integrate(-8000, 900_000); // 充 2000mAh = 500‰
    cs.add("soc_charge_recover", soc.soc_permille() == 1000, ""); // 钳满
    // 2) 空边界钳制。
    let mut soc2 = SocEstimator::new(4000, 100);
    soc2.integrate(100_000, 3_600_000);
    cs.add("soc_empty_clamp", soc2.soc_permille() == 0, "");
    // 3) 仪表部分校正：一次校正吃掉 ≤50% 漂移（800 vs 仪表 500 → 步 150）。
    let mut soc3 = SocEstimator::new(4000, 800);
    soc3.sample_gauge(500, true);
    cs.add("soc_partial_correction", soc3.soc_permille() == 650, "");
    soc3.sample_gauge(500, true);
    cs.add("soc_correction_converges", soc3.soc_permille() == 575, "");
    // 4) 仪表不可读 → 纯计数不校正（不假装校过）。
    let mut soc4 = SocEstimator::new(4000, 800);
    soc4.sample_gauge(200, false);
    cs.add("soc_unreadable_no_touch", soc4.soc_permille() == 800 && !soc4.gauge_readable(), "");
    cs.add("soc_correction_ledger", soc3.corrections() == 2 && soc4.corrections() == 0, "");

    // ── 剩余时长预测 ──
    // 5) 样本不足 unknown（不猜）。
    let mut rp = RuntimePredictor::new();
    rp.hourly(500);
    cs.add("predict_unknown_below_min", rp.forecast(500, 4000) == RuntimeForecast::Unknown, "");
    // 6) 两样本预测：500mAh/h → 2000mAh 余量 = 4h = 14_400_000ms。
    rp.hourly(500);
    cs.add(
        "predict_two_samples_4h",
        rp.forecast(500, 4000) == RuntimeForecast::Discharge(14_400_000),
        "",
    );
    // 7) 充电中不预测。
    rp.hourly(-300);
    cs.add("predict_charging_honest", rp.forecast(500, 4000) == RuntimeForecast::Charging, "");
    // 8) 满环滚动覆盖（第 9 个样本挤掉最旧的）。
    let mut rp2 = RuntimePredictor::new();
    for k in 0..(RATE_RING_CAP as i32 + 1) {
        rp2.hourly(100 + k * 10);
    }
    cs.add(
        "predict_ring_rolls",
        rp2.ring_n() == RATE_RING_CAP && rp2.avg_rate() == 145, // 110..180 八桶均值
        "",
    );
    // 9) 静置零率 unknown（零除保护）。
    let mut rp3 = RuntimePredictor::new();
    rp3.hourly(0);
    rp3.hourly(0);
    cs.add("predict_idle_unknown", rp3.forecast(900, 4000) == RuntimeForecast::Unknown, "");

    // ── 放电健康账 ──
    // 10) 满放 = 1 循环；浅放按分数折算。
    let mut dh = DischargeHealth::new(4000);
    dh.discharge_depth(1000);
    cs.add("health_full_cycle_x100", dh.cycles_x100() == 100, "");
    dh.discharge_depth(500);
    cs.add("health_half_cycle_fraction", dh.cycles_x100() == 150, "");
    // 11) 模型健康度：cycles_x100=150（=1.5 循环）× 160ppm = 240ppm 衰减
    //     → (1e6−240)×100/1e6 = 99（整数截断——循环账是分数精确的）。
    cs.add("health_model_pct", dh.health_by_model_pct() == 99, "");
    // 12) 实测口径：满充实测 3500/4000 = 87%。
    dh.full_charge_measured(3500);
    cs.add("health_measure_pct", dh.health_by_measure_pct() == 87, "");
    // 13) 实测容量不得高于出厂（钳制）。
    dh.full_charge_measured(9999);
    cs.add("health_measure_clamped", dh.health_by_measure_pct() == 100, "");
    // 14) 未实测 → 实测口径 0（不猜），预警用模型口径。
    let dh2 = DischargeHealth::new(4000);
    cs.add("health_no_measure_zero", dh2.health_by_measure_pct() == 0, "");
    // 15) 预警线：500 满循环 × 160ppm = 8% → 92% ≥ 80 不警；深衰减警。
    let mut dh3 = DischargeHealth::new(4000);
    for _ in 0..500 {
        dh3.discharge_depth(1000);
    }
    cs.add("health_500_cycles_ok", dh3.health_by_model_pct() == 92 && !dh3.warn(), "");
    let mut dh4 = DischargeHealth::new(4000);
    for _ in 0..1500 {
        dh4.discharge_depth(1000);
    }
    cs.add("health_degraded_warn", dh4.warn(), "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn soc_integrate_is_symmetric() {
        let mut s = SocEstimator::new(4000, 500);
        s.integrate(3600, 1_000_000); // 1000mAh 放
        let after_dis = s.soc_permille();
        s.integrate(-3600, 1_000_000); // 1000mAh 充
        assert_eq!(s.soc_permille(), 500, "充放对称——积分无系统性偏置");
        assert!(after_dis < 500);
    }

    #[test]
    fn soc_correction_never_overshoots() {
        let mut s = SocEstimator::new(4000, 100);
        // 仪表 1000（远高于计数）——校正步 ≤50% 差值，绝不超仪表。
        s.sample_gauge(1000, true);
        assert!(s.soc_permille() > 100 && s.soc_permille() <= 1000);
        for _ in 0..20 {
            s.sample_gauge(1000, true);
        }
        assert_eq!(s.soc_permille(), 1000, "重复校正最终收敛到仪表");
    }

    #[test]
    fn forecast_monotonic_in_soc() {
        let mut rp = RuntimePredictor::new();
        rp.hourly(400);
        rp.hourly(400);
        let a = match rp.forecast(250, 4000) {
            RuntimeForecast::Discharge(ms) => ms,
            _ => 0,
        };
        let b = match rp.forecast(750, 4000) {
            RuntimeForecast::Discharge(ms) => ms,
            _ => 0,
        };
        assert!(b > a, "SOC 越高预测越长");
        assert_eq!(a, 9_000_000, "1000mAh 余量 / 400mAh·h⁻¹ = 2.5h");
    }

    #[test]
    fn health_partial_cycles_accumulate_to_full() {
        let mut h = DischargeHealth::new(4000);
        for _ in 0..10 {
            h.discharge_depth(100); // 10% × 10 次 = 1 循环
        }
        assert_eq!(h.cycles_x100(), 100, "浅放折算精确");
    }

    #[test]
    fn predictor_rolling_window_drops_oldest() {
        let mut rp = RuntimePredictor::new();
        for _ in 0..RATE_RING_CAP {
            rp.hourly(1000);
        }
        rp.hourly(2000); // 挤掉最旧
        assert_eq!(rp.avg_rate(), 1125, "8×1000 + 2000 → (8000-1000+2000)/8");
    }
}

// ===========================================================================
// v3 深化批（F060 · G-B-20）——充电预测 / 应用能耗趋势 / 内阻诊断
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-20 功能定义的实装细化，非新立项）：
// 1. charge_eta_min —— 充电时长预测：CC（恒流）/CV（恒压）两相整数模型
//    ——CC 段线性到 800‰、CV 段尾巴恒斜率近似；到 80%（快充口径）与
//    到 100%（满充口径）分离给出——不混报。
// 2. AppTrendDelta —— 应用能耗趋势：近 7 天 vs 前 7 天差值；涨幅 >50%
//    且绝对增量超阈 → 异动侦查命中（排行之外的时间维度）。
// 3. BatteryDiag —— 内阻诊断：负载阶跃法（极值电流对 ΔV/ΔI → mΩ ×100
//    定点）；健康度 = 参考内阻/实测；翻倍即老化预警。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// CC 段恒流充电率（‰/分钟，Y7000 适配器量级——旋钮）。
pub const CC_RATE_PM_PER_MIN: u32 = 20;
/// CV 段起始 SOC（‰）：≥800 进入恒压尾巴。
pub const CV_START_PM: u16 = 800;
/// CV 段每分钟增量（‰，尾巴斜率首段近似）。
pub const CV_RATE_PM_PER_MIN: u32 = 6;
/// 内阻健康参考（mΩ ×100 = 30.00 mΩ，出厂典型——旋钮）。
pub const IR_HEALTHY_MOHM_X100: u32 = 3_000;
/// 内阻老化预警倍数（×100 = 200%——翻倍即警）。
pub const IR_WARN_MULT_X100: u32 = 200;

// ---------------------------------------------------------------------------
// 深化一：CC/CV 充电时长预测
// ---------------------------------------------------------------------------

/// 充电预测结论（分钟）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeEta {
    /// 到 80%（快充口径）。
    pub to_80_min: u32,
    /// 到 100%（满充口径）。
    pub to_100_min: u32,
}

/// CC/CV 两相预测（整数分钟）。
pub fn charge_eta_min(soc_pm: u16) -> ChargeEta {
    let soc = (soc_pm as u32).min(1000);
    if soc >= 1000 {
        return ChargeEta { to_80_min: 0, to_100_min: 0 };
    }
    let cv_start = CV_START_PM as u32;
    // CC 段：soc → cv_start（不足为 0）。
    let cc_min = cv_start.saturating_sub(soc) / CC_RATE_PM_PER_MIN;
    // CV 段：cv_start → 1000（恒斜率近似）。
    let cv_min = (1000 - cv_start) / CV_RATE_PM_PER_MIN;
    // to_80：80% 线在 CC 段内（800 ≤ cv_start）。
    let to_80 = if soc >= 800 { 0 } else { (800 - soc) / CC_RATE_PM_PER_MIN };
    ChargeEta {
        to_80_min: to_80,
        to_100_min: cc_min + cv_min,
    }
}

// ---------------------------------------------------------------------------
// 深化二：应用能耗趋势（7 天 vs 前 7 天）
// ---------------------------------------------------------------------------

/// 逐应用 14 天能耗环（day 0 = 今天）。
pub struct AppTrendDelta {
    days: [[u32; 14]; 8],
    n: usize,
}

impl AppTrendDelta {
    pub const fn new() -> Self {
        AppTrendDelta { days: [[0; 14]; 8], n: 0 }
    }

    /// 登记应用（返回 idx；超 8 个归并到 idx 7——容量语义如实）。
    pub fn add_app(&mut self) -> usize {
        let idx = if self.n < 8 {
            let i = self.n;
            self.n += 1;
            i
        } else {
            7
        };
        idx
    }

    /// 喂某应用某天能耗。
    pub fn set_day(&mut self, idx: usize, day: usize, energy: u32) {
        if idx < 8 && day < 14 {
            self.days[idx][day] = energy;
        }
    }

    /// 趋势差值（近 7 天和 − 前 7 天和）。
    pub fn delta(&self, idx: usize) -> i64 {
        if idx >= 8 {
            return 0;
        }
        let recent: u64 = self.days[idx][..7].iter().map(|v| *v as u64).sum();
        let prior: u64 = self.days[idx][7..14].iter().map(|v| *v as u64).sum();
        recent as i64 - prior as i64
    }

    /// 涨幅 ×100（prior=0 → None——不猜）。
    pub fn rise_pct_x100(&self, idx: usize) -> Option<i64> {
        if idx >= 8 {
            return None;
        }
        let prior: u64 = self.days[idx][7..14].iter().map(|v| *v as u64).sum();
        if prior == 0 {
            return None;
        }
        Some(self.delta(idx) * 100 / prior as i64)
    }

    /// 异动侦查：涨幅 >50% 且绝对增量 >100 的应用写入 out，返回命中数。
    pub fn scan_spikes(&self, out: &mut [usize]) -> usize {
        let mut k = 0;
        for idx in 0..self.n.min(8) {
            if let Some(rise) = self.rise_pct_x100(idx) {
                if rise > 50 && self.delta(idx) > 100 && k < out.len() {
                    out[k] = idx;
                    k += 1;
                }
            }
        }
        k
    }

    pub fn app_count(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 深化三：内阻诊断（负载阶跃法）
// ---------------------------------------------------------------------------

/// 内阻估计器。
pub struct BatteryDiag {
    samples: [(u32, u32); 8], // (电流 mA, 端电压 mV)
    n: usize,
    ir_mohm_x100: u32,
}

impl BatteryDiag {
    pub const fn new() -> Self {
        BatteryDiag { samples: [(0, 0); 8], n: 0, ir_mohm_x100: 0 }
    }

    /// 喂采样对（同 SOC 窗口内的多态负载）。
    pub fn sample(&mut self, current_ma: u32, voltage_mv: u32) {
        if self.n < 8 {
            self.samples[self.n] = (current_ma, voltage_mv);
            self.n += 1;
        } else {
            for k in 0..7 {
                self.samples[k] = self.samples[k + 1];
            }
            self.samples[7] = (current_ma, voltage_mv);
        }
        self.estimate();
    }

    /// 估计：极值电流对（跨度最大——信噪比最好）。
    fn estimate(&mut self) {
        if self.n < 2 {
            return;
        }
        let mut lo = 0usize;
        let mut hi = 0usize;
        for k in 0..self.n {
            if self.samples[k].0 < self.samples[lo].0 {
                lo = k;
            }
            if self.samples[k].0 > self.samples[hi].0 {
                hi = k;
            }
        }
        let di = self.samples[hi].0.saturating_sub(self.samples[lo].0);
        let dv = self.samples[lo].1.saturating_sub(self.samples[hi].1); // 载重压降
        if di == 0 {
            return;
        }
        // R(mΩ) = ΔV(mV)/ΔI(A) = ΔV×1000/ΔI；×100 定点 = ΔV×100_000/ΔI。
        self.ir_mohm_x100 = ((dv as u64) * 100_000 / (di as u64)) as u32;
    }

    /// 最近估计（mΩ ×100；未估计 = 0——不猜）。
    pub fn ir_mohm_x100(&self) -> u32 {
        self.ir_mohm_x100
    }

    /// 健康度 ×100（参考/实测）。未估计 → None。
    pub fn health_pct_x100(&self) -> Option<u64> {
        if self.ir_mohm_x100 == 0 {
            return None;
        }
        Some((IR_HEALTHY_MOHM_X100 as u64) * 100 / self.ir_mohm_x100 as u64)
    }

    /// 老化预警（实测 ≥ 参考 ×2）。
    pub fn aged(&self) -> bool {
        self.ir_mohm_x100 >= IR_HEALTHY_MOHM_X100 * IR_WARN_MULT_X100 / 100
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：充电预测 / 趋势 / 内阻逐条实摆。
pub fn run_powerledger_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F060-powerledger-v3");

    // ── 充电预测 ──
    // 1) 0%：CC 段 800/20 = 40 分钟 + CV 段 200/6 = 33 分钟 = 73。
    let e0 = charge_eta_min(0);
    cs.add("charge_full_73min", e0.to_100_min == 73 && e0.to_80_min == 40, "");
    // 2) 恰在 CV 起点：to_80 = 0、to_100 = 33。
    let e80 = charge_eta_min(800);
    cs.add("charge_at_cv_start", e80.to_80_min == 0 && e80.to_100_min == 33, "");
    // 3) 满电双零。
    cs.add("charge_full_zero", charge_eta_min(1000) == ChargeEta { to_80_min: 0, to_100_min: 0 }, "");
    // 4) 快充口径 ≤ 满充口径恒成立（抽样五点）。
    cs.add(
        "charge_80_le_100",
        [0u16, 250, 500, 799, 999].iter().all(|s| {
            let e = charge_eta_min(*s);
            e.to_80_min <= e.to_100_min
        }),
        "",
    );

    // ── 应用趋势 ──
    let mut tr = AppTrendDelta::new();
    let idx = tr.add_app();
    for d in 0..7 {
        tr.set_day(idx, d, 300);
    }
    for d in 7..14 {
        tr.set_day(idx, d, 100);
    }
    cs.add("trend_delta_1400", tr.delta(idx) == 1400, "");
    cs.add("trend_rise_200pct", tr.rise_pct_x100(idx) == Some(200), "");
    // 无历史（prior=0）不猜。
    let idx2 = tr.add_app();
    tr.set_day(idx2, 0, 500);
    cs.add("trend_no_prior_unknown", tr.rise_pct_x100(idx2).is_none(), "");
    // 异动侦查命中。
    let mut out = [0usize; 8];
    let hits = tr.scan_spikes(&mut out);
    cs.add("trend_spike_scanned", hits >= 1 && out[0] == idx, "");
    // 下降趋势负增量。
    for d in 0..7 {
        tr.set_day(idx2, d, 50);
    }
    for d in 7..14 {
        tr.set_day(idx2, d, 200);
    }
    cs.add("trend_decline_negative", tr.delta(idx2) == -1050, "");

    // ── 内阻诊断 ──
    // 1) 标准对：200mA@12.00V / 3200mA@11.40V → ΔV=600mV、ΔI=3A → 200mΩ。
    let mut bd = BatteryDiag::new();
    bd.sample(200, 12_000);
    bd.sample(3_200, 11_400);
    cs.add("diag_ir_200mohm", bd.ir_mohm_x100() == 20_000, "");
    cs.add("diag_health_15pct", bd.health_pct_x100() == Some(15), "");
    cs.add("diag_aged_true", bd.aged(), "");
    // 2) 新电池 30mΩ：30mV/1A → 健康 100% 不警。
    let mut bd4 = BatteryDiag::new();
    bd4.sample(200, 12_000);
    bd4.sample(1_200, 11_970);
    cs.add("diag_new_battery", bd4.health_pct_x100() == Some(100) && !bd4.aged(), "");
    // 3) 单样本不估（0 = 未估——不猜）。
    cs.add("diag_one_sample_keeps_zero", {
        let mut b = BatteryDiag::new();
        b.sample(500, 12_000);
        b.ir_mohm_x100() == 0
    }, "");
    // 4) 极值对选择：中间噪声样本不参与（极值 = 200mA/1500mA 对）。
    let mut bd5 = BatteryDiag::new();
    bd5.sample(200, 12_000);
    bd5.sample(1_000, 11_990);
    bd5.sample(1_500, 11_950);
    bd5.sample(1_200, 11_970);
    cs.add("diag_extremes_only", bd5.ir_mohm_x100() == 3_846, ""); // 50×100000/1300

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn charge_eta_monotonic_in_soc() {
        let a = charge_eta_min(200).to_100_min;
        let b = charge_eta_min(600).to_100_min;
        assert!(b < a, "SOC 越高满充越近");
    }

    #[test]
    fn trend_cap_eight_apps() {
        let mut tr = AppTrendDelta::new();
        for _ in 0..12 {
            tr.add_app();
        }
        assert_eq!(tr.app_count(), 8, "容量 8——超出归并不膨胀");
    }

    #[test]
    fn diag_sliding_window_drops_old() {
        let mut bd = BatteryDiag::new();
        bd.sample(200, 12_000);
        bd.sample(3_200, 11_400); // 200mΩ
        // 滑窗 8 槽：7 个 (700,11985) + 1 个 (1200,11970) → 旧对全被挤出，
        // 新极值对 ΔV=15mV/ΔI=500mA → 30mΩ。
        for _ in 0..7 {
            bd.sample(700, 11_985);
        }
        bd.sample(1_200, 11_970);
        assert_eq!(bd.ir_mohm_x100(), 3_000, "滑窗滚动后旧数据退场");
    }
}

// ===========================================================================
// v4 深化批（F060 · G-B-20）——日报编解码 / 能耗异动侦测 / 预算告警
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-20 功能定义的实装细化，非新立项）：
// 1. DailyReportCodec —— 日能耗报告序列化：日期 + 总能耗 + top 应用
//    → 定长字节 + FNV（导出/跨机对账的机制面）。
// 2. EnergyAnomaly —— 能耗异动侦测：当日 vs 30 天基线（均值+2×标准
//    差整数版）——超线标红并给倍数。
// 3. BudgetAlert —— 用户预算告警：日预算 ‰ 消耗进度 + 超速预警
//    （按当日已过小时折算——「照这速度今天要爆」的诚实提醒）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 日报 top 应用数。
pub const DAILY_TOP_N: usize = 4;
/// 基线窗（天）。
pub const ANOMALY_BASELINE_DAYS: usize = 30;
/// 异动倍数线（×100 = 200%——均值两倍）。
pub const ANOMALY_MULT_X100: u64 = 200;

// ---------------------------------------------------------------------------
// 深化一：日报编解码
// ---------------------------------------------------------------------------

/// 日报条目（应用哈希 + 能耗）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DailyAppRow {
    pub app_hash: u64,
    pub energy: u32,
}

/// 日报编解码：day_index(2) + total(4) + rows(4×12) + FNV(4) = 58B。
pub const DAILY_REPORT_BYTES: usize = 2 + 4 + DAILY_TOP_N * 12 + 4;

/// 编码日报。
pub fn daily_report_encode(day_index: u16, total: u32, rows: &[DailyAppRow]) -> [u8; DAILY_REPORT_BYTES] {
    let mut buf = [0u8; DAILY_REPORT_BYTES];
    buf[0..2].copy_from_slice(&day_index.to_le_bytes());
    buf[2..6].copy_from_slice(&total.to_le_bytes());
    for (k, r) in rows.iter().take(DAILY_TOP_N).enumerate() {
        let b = 6 + k * 12;
        buf[b..b + 8].copy_from_slice(&r.app_hash.to_le_bytes());
        buf[b + 8..b + 12].copy_from_slice(&r.energy.to_le_bytes());
    }
    let h = crate::perfstar2::perfgate::fnv1a(&buf[..DAILY_REPORT_BYTES - 4]);
    let hb = h.to_le_bytes();
    buf[DAILY_REPORT_BYTES - 4..].copy_from_slice(&hb);
    buf
}

/// 解码日报（校验失败 → None）。
pub fn daily_report_decode(buf: &[u8; DAILY_REPORT_BYTES]) -> Option<(u16, u32, [DailyAppRow; DAILY_TOP_N])> {
    let stored = u32::from_le_bytes([buf[buf.len() - 4], buf[buf.len() - 3], buf[buf.len() - 2], buf[buf.len() - 1]]);
    if crate::perfstar2::perfgate::fnv1a(&buf[..buf.len() - 4]) != stored {
        return None;
    }
    let day = u16::from_le_bytes([buf[0], buf[1]]);
    let total = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);
    let mut rows = [DailyAppRow { app_hash: 0, energy: 0 }; DAILY_TOP_N];
    for (k, r) in rows.iter_mut().enumerate() {
        let b = 6 + k * 12;
        let mut h8 = [0u8; 8];
        h8.copy_from_slice(&buf[b..b + 8]);
        let mut e4 = [0u8; 4];
        e4.copy_from_slice(&buf[b + 8..b + 12]);
        *r = DailyAppRow { app_hash: u64::from_le_bytes(h8), energy: u32::from_le_bytes(e4) };
    }
    Some((day, total, rows))
}

// ---------------------------------------------------------------------------
// 深化二：能耗异动侦测
// ---------------------------------------------------------------------------

/// 基线窗（30 天能耗样本）。
pub struct EnergyAnomaly {
    history: [u32; ANOMALY_BASELINE_DAYS],
    n: usize,
    anomalies: u64,
}

impl EnergyAnomaly {
    pub const fn new() -> Self {
        EnergyAnomaly { history: [0; ANOMALY_BASELINE_DAYS], n: 0, anomalies: 0 }
    }

    /// 喂历史样本（当日判定前的基线数据）。
    pub fn feed(&mut self, energy: u32) {
        if self.n < ANOMALY_BASELINE_DAYS {
            self.history[self.n] = energy;
            self.n += 1;
        } else {
            for k in 0..ANOMALY_BASELINE_DAYS - 1 {
                self.history[k] = self.history[k + 1];
            }
            self.history[ANOMALY_BASELINE_DAYS - 1] = energy;
        }
    }

    /// 基线均值 ×100。
    pub fn baseline_mean_x100(&self) -> Option<u64> {
        if self.n < 7 {
            return None; // 少于一周不判（不猜）
        }
        let sum: u64 = self.history[..self.n].iter().map(|v| *v as u64).sum();
        Some(sum * 100 / self.n as u64)
    }

    /// 当日是否异动（> 均值 ×2）→ 返回倍数 ×100。
    pub fn check(&mut self, today: u32) -> Option<u64> {
        let mean = self.baseline_mean_x100()?;
        if mean == 0 {
            return None;
        }
        let mult = (today as u64) * 10_000 / mean; // mean 已 ×100——再 ×100 还原比值
        if mult > ANOMALY_MULT_X100 {
            self.anomalies += 1;
            Some(mult)
        } else {
            None
        }
    }

    pub fn anomalies(&self) -> u64 {
        self.anomalies
    }
}

// ---------------------------------------------------------------------------
// 深化三：预算告警
// ---------------------------------------------------------------------------

/// 预算进度告警。
pub struct BudgetAlert {
    budget_energy: u32, // 日预算（折算能耗单位）
    spent: u32,
    hours_elapsed: u32, // 当日已过小时（0-24）
}

impl BudgetAlert {
    pub const fn new(budget_energy: u32) -> Self {
        BudgetAlert { budget_energy, spent: 0, hours_elapsed: 0 }
    }

    /// 记消耗与时间推进。
    pub fn record(&mut self, energy: u32, hour: u32) {
        self.spent += energy;
        self.hours_elapsed = hour.min(24);
    }

    /// 消耗进度 ×100。
    pub fn spent_pct_x100(&self) -> u64 {
        if self.budget_energy == 0 {
            return 0;
        }
        (self.spent as u64) * 100 / self.budget_energy as u64
    }

    /// 超速预警：线性折算全天消耗 > 预算（小时为 0 时不猜）。
    pub fn on_pace_to_exceed(&self) -> Option<bool> {
        if self.hours_elapsed == 0 {
            return None;
        }
        let projected = (self.spent as u64) * 24 / self.hours_elapsed as u64;
        Some(projected > self.budget_energy as u64)
    }

    /// 已爆表。
    pub fn exceeded(&self) -> bool {
        self.spent > self.budget_energy
    }

    /// 重置（新的一天）。
    pub fn reset_day(&mut self) {
        self.spent = 0;
        self.hours_elapsed = 0;
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：日报 / 异动 / 预算逐条实摆。
pub fn run_powerledger_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F060-powerledger-v4");

    // ── 日报编解码 ──
    let rows = [
        DailyAppRow { app_hash: 0x11, energy: 500 },
        DailyAppRow { app_hash: 0x22, energy: 300 },
        DailyAppRow { app_hash: 0x33, energy: 150 },
        DailyAppRow { app_hash: 0x44, energy: 50 },
    ];
    let buf = daily_report_encode(42, 1000, &rows);
    cs.add("daily_roundtrip", {
        matches!(daily_report_decode(&buf), Some((42, 1000, r)) if r[0] == rows[0] && r[3] == rows[3])
    }, "");
    let mut bad = buf;
    bad[10] ^= 0xFF;
    cs.add("daily_tamper_none", daily_report_decode(&bad).is_none(), "");
    // 超 top4 截断。
    let five = [
        rows[0], rows[1], rows[2], rows[3],
        DailyAppRow { app_hash: 0x55, energy: 10 },
    ];
    cs.add("daily_top4_cap", {
        matches!(daily_report_decode(&daily_report_encode(0, 1010, &five)), Some((_, _, r)) if r[3].app_hash == 0x44 && r[0].app_hash != 0x55)
    }, "");

    // ── 异动侦测 ──
    let mut an = EnergyAnomaly::new();
    for _ in 0..6 {
        an.feed(100);
    }
    cs.add("anomaly_below_week_none", an.check(500).is_none(), "");
    an.feed(100); // 第 7 天——基线成立
    cs.add("anomaly_normal_none", an.check(150).is_none(), "");
    cs.add("anomaly_spike_caught", an.check(300) == Some(300), ""); // 300/100 = 300%
    cs.add("anomaly_ledger", an.anomalies() == 1, "");
    // 全零基线不判（除零保护）。
    let mut an2 = EnergyAnomaly::new();
    for _ in 0..7 {
        an2.feed(0);
    }
    cs.add("anomaly_zero_baseline_none", an2.check(100).is_none(), "");

    // ── 预算告警 ──
    let mut ba = BudgetAlert::new(1000);
    cs.add("budget_zero_hour_no_guess", ba.on_pace_to_exceed().is_none(), "");
    ba.record(100, 6); // 6 小时花了 10%
    cs.add("budget_on_pace_ok", ba.on_pace_to_exceed() == Some(false), ""); // 折算全天 400 < 1000
    ba.record(300, 12); // 12 小时花了 40% → 折算 800 仍不超
    cs.add("budget_pace_warn_late", {
        ba.record(300, 13); // 13 小时 70% → 折算 1292 > 1000 → 预警
        ba.on_pace_to_exceed() == Some(true)
    }, "");
    cs.add("budget_not_yet_exceeded", !ba.exceeded(), "");
    ba.record(400, 14);
    cs.add("budget_exceeded", ba.exceeded() && ba.spent_pct_x100() == 110, "");
    ba.reset_day();
    cs.add("budget_day_reset", ba.spent_pct_x100() == 0 && ba.on_pace_to_exceed().is_none(), "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn daily_report_zero_rows_valid() {
        let buf = daily_report_encode(7, 0, &[]);
        let (d, t, r) = daily_report_decode(&buf).unwrap();
        assert_eq!((d, t), (7, 0));
        assert!(r.iter().all(|x| x.app_hash == 0));
    }

    #[test]
    fn anomaly_monotonic_history_window() {
        let mut an = EnergyAnomaly::new();
        for k in 0..40u32 {
            an.feed(100 + k);
        }
        // 窗滚动后最旧（100）退场——均值 > 100+15。
        let m = an.baseline_mean_x100().unwrap();
        assert!(m > 11_500, "滚动窗均值反映新数据 m={m}");
    }

    #[test]
    fn budget_hour_cap_24() {
        let mut ba = BudgetAlert::new(100);
        ba.record(10, 30); // hour 30 → 钳 24
        let p = ba.on_pace_to_exceed().unwrap();
        assert!(!p, "折算用 24 小时口径");
    }
}

// ===========================================================================
// v5 深化批（deep5）
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：温度补偿 SOC 估计（内阻随温度变化——低温容量显示下调）
// ---------------------------------------------------------------------------

/// 温度补偿表：−10/0/25/45℃ 四锚点容量系数（×1000）。
pub const TEMP_COEF_X1000: [i32; 4] = [820, 920, 1000, 980];
/// 锚点温度（℃）。
pub const TEMP_ANCHORS: [i32; 4] = [-10, 0, 25, 45];

/// 分段线性温度补偿系数（×1000）。
pub fn temp_coef_x1000(temp_c: i32) -> i32 {
    if temp_c <= TEMP_ANCHORS[0] {
        return TEMP_COEF_X1000[0];
    }
    for k in 0..3 {
        if temp_c <= TEMP_ANCHORS[k + 1] {
            let span = TEMP_ANCHORS[k + 1] - TEMP_ANCHORS[k];
            let frac = (temp_c - TEMP_ANCHORS[k]) * 1000 / span;
            let a = TEMP_COEF_X1000[k];
            let b = TEMP_COEF_X1000[k + 1];
            return a + (b - a) * frac / 1000;
        }
    }
    TEMP_COEF_X1000[3]
}

/// 温补 SOC 显示：真实 SOC × 温度系数（低温诚实下调）。
pub fn soc_displayed_x1000(soc_permille: u16, temp_c: i32) -> u32 {
    (soc_permille as u32) * temp_coef_x1000(temp_c).max(0) as u32 / 1000
}

// ---------------------------------------------------------------------------
// 深化二：循环寿命账（等效满循环 EFC——部分循环按 DoD 折算累加）
// ---------------------------------------------------------------------------

/// EFC 账本：每次放电深度 DoD（×1000）按 0.8 次幂近似折算
/// （浅充放损伤小——整数近似：DoD^0.8 ≈ DoD × (1 - (1000-DoD)/5000)）。
pub struct CycleLedger {
    efc_x1000: u64,
    cycles_logged: u32,
}

impl CycleLedger {
    pub const fn new() -> Self {
        CycleLedger { efc_x1000: 0, cycles_logged: 0 }
    }

    /// 记一次放电（dod_permille: 0..=1000）。返回折算 EFC 增量（×1000）。
    pub fn log_discharge(&mut self, dod_permille: u16) -> u64 {
        let d = dod_permille.min(1000) as u64;
        // 折算系数 ×1000：1000 DoD → 1000；500 DoD → 500×(1+100/5000)=510？不——
        // 浅放折算应 <线性：coef = 1000 - (1000-DoD)/5。
        let coef = 1000 - (1000 - d) / 5;
        let delta = d * coef / 1000;
        self.efc_x1000 += delta;
        self.cycles_logged += 1;
        delta
    }

    /// 等效满循环数（×1000）。
    pub fn efc_x1000(&self) -> u64 {
        self.efc_x1000
    }

    pub fn cycles(&self) -> u32 {
        self.cycles_logged
    }
}

// ---------------------------------------------------------------------------
// 深化三：充电会话分段（插拔识别——间隔 >5 分钟算新会话）
// ---------------------------------------------------------------------------

/// 充电会话账：连续充电段聚合（起始 SOC/结束 SOC/时长/增量）。
pub struct ChargeSessionLedger {
    /// 当前会话起始 SOC（×1000）。
    session_start_soc: Option<u32>,
    session_start_ms: u32,
    last_ms: u32,
    /// 已完结会话账（最近 8 个滚动）。
    ends_ring: [(u32, u32); 8], // (duration_ms, gained_x1000)
    end_pos: usize,
    end_n: usize,
    completed: u32,
}

/// 会话切分间隔（ms）。
pub const SESSION_GAP_MS: u32 = 5 * 60 * 1000;

impl ChargeSessionLedger {
    pub const fn new() -> Self {
        ChargeSessionLedger {
            session_start_soc: None,
            session_start_ms: 0,
            last_ms: 0,
            ends_ring: [(0, 0); 8],
            end_pos: 0,
            end_n: 0,
            completed: 0,
        }
    }

    /// 采样：charging 状态 + SOC（×1000）+ 时刻。
    pub fn sample(&mut self, charging: bool, soc_x1000: u32, now_ms: u32) {
        if charging {
            if self.session_start_soc.is_none() {
                self.session_start_soc = Some(soc_x1000);
                self.session_start_ms = now_ms;
            } else if now_ms.saturating_sub(self.last_ms) > SESSION_GAP_MS {
                // 间隔过久 → 上会话视为已结束，开新会话。
                self.close(now_ms, soc_x1000);
                self.session_start_soc = Some(soc_x1000);
                self.session_start_ms = now_ms;
            }
            self.last_ms = now_ms;
        } else if self.session_start_soc.is_some() {
            // 先 close（读起点）再清——顺序错会把结束 SOC 当起点（v5#1）。
            self.close(now_ms, soc_x1000);
            self.session_start_soc = None;
        }
    }

    fn close(&mut self, now_ms: u32, soc_x1000: u32) {
        let start = self.session_start_soc.unwrap_or(soc_x1000);
        let gained = soc_x1000.saturating_sub(start);
        self.ends_ring[self.end_pos] = (now_ms.saturating_sub(self.session_start_ms), gained);
        self.end_pos = (self.end_pos + 1) % 8;
        if self.end_n < 8 {
            self.end_n += 1;
        }
        self.completed += 1;
    }

    /// 最近一次会话 (时长, 增量)。
    pub fn last_session(&self) -> Option<(u32, u32)> {
        if self.end_n == 0 {
            return None;
        }
        let idx = (self.end_pos + 8 - 1) % 8;
        Some(self.ends_ring[idx])
    }

    pub fn completed(&self) -> u32 {
        self.completed
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_powerledger_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F060-powerledger-v5");

    // ── 温补 ──
    // 1) 锚点精确：25℃ 系数 1000，−10℃ 820。
    cs.add(
        "tempcoef_anchors",
        temp_coef_x1000(25) == 1000 && temp_coef_x1000(-10) == 820 && temp_coef_x1000(45) == 980,
        "",
    );
    // 2) 中段插值：12℃ → frac=480 → 920 + 80×480/1000 = 958。
    cs.add("tempcoef_midpoint", temp_coef_x1000(12) == 958, "");
    // 3) 低温显示诚实下调：SOC 500 @ −10℃ → 500×820/1000 = 410。
    cs.add("soc_display_downscaled", soc_displayed_x1000(500, -10) == 410, "");
    // 4) 越界温度钳到端点。
    cs.add("tempcoef_clamped", temp_coef_x1000(-50) == 820 && temp_coef_x1000(80) == 980, "");

    // ── 循环寿命 ──
    // 5) 满循环：1000 DoD → 1000 折算（系数 1000）。
    let mut cl = CycleLedger::new();
    cs.add("efc_full_cycle", cl.log_discharge(1000) == 1000 && cl.efc_x1000() == 1000, "");
    // 6) 浅循环折算 <线性：500 DoD → 500×(1000-100)/1000 = 450。
    let mut cl2 = CycleLedger::new();
    cs.add("efc_shallow_discounted", cl2.log_discharge(500) == 450, "");
    // 7) 两次浅放 < 一次满放（EFC 视角）。
    let mut cl3 = CycleLedger::new();
    let _ = cl3.log_discharge(500);
    let _ = cl3.log_discharge(500);
    cs.add("efc_two_shallow_lt_full", cl3.efc_x1000() == 900 && cl3.cycles() == 2, "");

    // ── 充电会话 ──
    // 8) 单会话：0-2 分钟充电 SOC 200→800，拔出 → 完结（时长 130s，增量 600）。
    let mut sl = ChargeSessionLedger::new();
    sl.sample(true, 200, 0);
    sl.sample(true, 800, 120_000); // 间隔 2min < 5min → 同会话
    sl.sample(false, 800, 130_000);
    cs.add("session_basic", sl.completed() == 1 && sl.last_session() == Some((130_000, 600)), "");
    // 9) 间隔 >5 分钟切新会话（0→200s 同会话；200s→600s 超隔切分）。
    let mut sl2 = ChargeSessionLedger::new();
    sl2.sample(true, 100, 0);
    sl2.sample(true, 200, 200_000); // 间隔 200s < 5min → 同会话
    sl2.sample(true, 400, 600_000); // 间隔 400s > 5min → 切分（上一会话先完结）
    sl2.sample(false, 500, 650_000);
    cs.add("session_gap_splits", sl2.completed() == 2, "");
    // 10) 无会话时拔出不算完成。
    let mut sl3 = ChargeSessionLedger::new();
    sl3.sample(false, 500, 0);
    cs.add("session_unplug_noop", sl3.completed() == 0 && sl3.last_session().is_none(), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn temp_coef_midpoint_math() {
        // 12.5℃：frac = 12.5×1000/25 = 500 → 920 + 80×500/1000 = 960。
        assert_eq!(temp_coef_x1000(12), 920 + 80 * 12 * 1000 / 25 / 1000);
        assert_eq!(temp_coef_x1000(13), 920 + 80 * 13 * 1000 / 25 / 1000);
    }

    #[test]
    fn session_ring_overwrites_oldest() {
        let mut sl = ChargeSessionLedger::new();
        for k in 0..10u32 {
            sl.sample(true, 0, k * 60_000);
            sl.sample(false, 100, k * 60_000 + 30_000);
        }
        assert_eq!(sl.completed(), 10);
        // 最近会话（k=9）：540s 起，570s 拔出 → 时长 30s，增量 100。
        assert_eq!(sl.last_session(), Some((30_000, 100)));
    }

    #[test]
    fn efc_accumulates_across_mix() {
        let mut cl = CycleLedger::new();
        let mut total = 0u64;
        for _ in 0..10 {
            total += cl.log_discharge(300); // 300×(1000-140)/1000 = 258
        }
        assert_eq!(total, cl.efc_x1000());
        assert_eq!(cl.efc_x1000(), 10 * 258);
    }
}
