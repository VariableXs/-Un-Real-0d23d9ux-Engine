//! VARIX-M500 · AI-11 能源与环境感知（F251~F275，M4）
//!
//! 使命：每一瓦电都有故事——预测、场景、舒适、离网。
//! 与 VARIX-500 的 `power.rs`（F251~F275 原始电源栈）零重复：
//! 本模块聚焦 v2 能源智能（画像、碳感知、预测、场景、舒适、离网）。
//! 纯逻辑 + 固定容量数组，无堆分配。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F251 能源仪表盘 v2 — 全系统能耗视图（分桶快照）
// ---------------------------------------------------------------------------

/// 能耗分桶（u32 毫瓦，固定槽位：CPU/GPU/显示/存储/网络/外设/其他）。
pub const ENERGY_BUCKETS: usize = 7;
pub const BUCKET_CPU: usize = 0;
pub const BUCKET_GPU: usize = 1;
pub const BUCKET_DISPLAY: usize = 2;
pub const BUCKET_STORAGE: usize = 3;
pub const BUCKET_NET: usize = 4;
pub const BUCKET_PERIPH: usize = 5;
pub const BUCKET_OTHER: usize = 6;

#[derive(Clone, Copy, Debug)]
pub struct EnergySnapshot {
    pub buckets: [u32; ENERGY_BUCKETS],
}

impl EnergySnapshot {
    pub const fn new() -> EnergySnapshot {
        EnergySnapshot { buckets: [0; ENERGY_BUCKETS] }
    }
    pub fn total_mw(&self) -> u32 {
        self.buckets.iter().fold(0u32, |a, &v| a.saturating_add(v))
    }
    /// 占比（千分比）。
    pub fn permille(&self, bucket: usize) -> u16 {
        let total = self.total_mw() as u64;
        if total == 0 {
            return 0;
        }
        ((self.buckets[bucket] as u64) * 1000 / total) as u16
    }
}

// F251 — 视图汇总：总计 + 最大桶。
pub fn dashboard_summary(s: &EnergySnapshot) -> (u32, usize, u16) {
    let mut max_bucket = 0usize;
    for i in 1..ENERGY_BUCKETS {
        if s.buckets[i] > s.buckets[max_bucket] {
            max_bucket = i;
        }
    }
    (s.total_mw(), max_bucket, s.permille(max_bucket))
}

// ---------------------------------------------------------------------------
// F252 碳感知充电 — 电网时段建议
// ---------------------------------------------------------------------------

/// 每小时电网碳强度（gCO2/kWh），24 槽。
pub const GRID_HOURS: usize = 24;
/// 碳强度低于该值视为清洁时段。
pub const CLEAN_CARBON_G: u16 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeAdvice {
    /// 现在就充（当前即清洁或必须充）。
    ChargeNow,
    /// 推迟到 `hour` 再充。
    WaitUntil(u8),
}

// F252 — 在窗口内选碳强度最低的小时；当前即最低且清洁则立即充。
pub fn carbon_charge_advice(grid: &[u16; GRID_HOURS], now_hour: u8, window_h: u8) -> ChargeAdvice {
    let now = now_hour as usize % GRID_HOURS;
    let span = (window_h as usize).clamp(1, GRID_HOURS);
    let mut best = now;
    for k in 0..span {
        let h = (now + k) % GRID_HOURS;
        if grid[h] < grid[best] {
            best = h;
        }
    }
    if best == now && grid[now] <= CLEAN_CARBON_G {
        ChargeAdvice::ChargeNow
    } else if best == now {
        ChargeAdvice::ChargeNow // 窗口内已是最优，别等
    } else {
        ChargeAdvice::WaitUntil(best as u8)
    }
}

// ---------------------------------------------------------------------------
// F253 续航预测器 — 按近期放电斜率学习剩余时间
// ---------------------------------------------------------------------------

/// 固定长度 SoC 采样环（百分数 * 100），配套毫秒时戳。
pub const SLOPE_SAMPLES: usize = 8;

#[derive(Clone, Copy)]
pub struct RuntimePredictor {
    soc_cs: [i32; SLOPE_SAMPLES],
    ts_ms: [u32; SLOPE_SAMPLES],
    head: usize,
    len: usize,
}

impl RuntimePredictor {
    pub const fn new() -> RuntimePredictor {
        RuntimePredictor { soc_cs: [0; SLOPE_SAMPLES], ts_ms: [0; SLOPE_SAMPLES], head: 0, len: 0 }
    }

    pub fn push(&mut self, soc_cs: i32, ts_ms: u32) {
        self.soc_cs[self.head] = soc_cs;
        self.ts_ms[self.head] = ts_ms;
        self.head = (self.head + 1) % SLOPE_SAMPLES;
        if self.len < SLOPE_SAMPLES {
            self.len += 1;
        }
    }

    /// 放电速率：每小时的 SoC 百分数损耗（正数 = 在掉电）。
    pub fn drain_cs_per_hour(&self) -> u32 {
        if self.len < 2 {
            return 0;
        }
        let oldest = (self.head + SLOPE_SAMPLES - self.len) % SLOPE_SAMPLES;
        let newest = (self.head + SLOPE_SAMPLES - 1) % SLOPE_SAMPLES;
        let d_soc = self.soc_cs[oldest] - self.soc_cs[newest];
        let d_ms = self.ts_ms[newest].wrapping_sub(self.ts_ms[oldest]) as u64;
        if d_soc <= 0 || d_ms == 0 {
            return 0;
        }
        ((d_soc as u64) * 3_600_000 / d_ms) as u32
    }

    /// 剩余分钟数（基于当前电量与学习到的速率）。
    pub fn remaining_min(&self, soc_cs: i32) -> u32 {
        let rate = self.drain_cs_per_hour();
        if rate == 0 {
            return u32::MAX; // 未学习到放电趋势
        }
        ((soc_cs as u64) * 60 / (rate as u64)) as u32
    }
}

// ---------------------------------------------------------------------------
// F254 耗电排行 — 应用能耗榜（固定 8 槽）
// ---------------------------------------------------------------------------

pub const RANK_SLOTS: usize = 8;

#[derive(Clone, Copy)]
pub struct PowerRank {
    pub names: [[u8; 12]; RANK_SLOTS],
    pub mw: [u32; RANK_SLOTS],
    pub len: usize,
}

impl PowerRank {
    pub const fn new() -> PowerRank {
        PowerRank { names: [[0; 12]; RANK_SLOTS], mw: [0; RANK_SLOTS], len: 0 }
    }

    /// 记账式累加；不存在则占用一个空槽（满则并入最小者若新值更大）。
    pub fn record(&mut self, name: &[u8], mw: u32) {
        let mut found = RANK_SLOTS;
        for i in 0..self.len {
            if &self.names[i][..name.len()] == name && self.names[i][name.len()] == 0 {
                found = i;
                break;
            }
        }
        if found < RANK_SLOTS {
            self.mw[found] = self.mw[found].saturating_add(mw);
            return;
        }
        if self.len < RANK_SLOTS {
            let i = self.len;
            let n = name.len().min(12);
            self.names[i][..n].copy_from_slice(&name[..n]);
            self.mw[i] = mw;
            self.len += 1;
            return;
        }
        let mut min_i = 0;
        for i in 1..RANK_SLOTS {
            if self.mw[i] < self.mw[min_i] {
                min_i = i;
            }
        }
        if mw > self.mw[min_i] {
            let n = name.len().min(12);
            self.names[min_i] = [0; 12];
            self.names[min_i][..n].copy_from_slice(&name[..n]);
            self.mw[min_i] = mw;
        }
    }

    /// 返回按功耗降序排序后的索引表。
    pub fn order(&self) -> [usize; RANK_SLOTS] {
        let mut idx = [0usize; RANK_SLOTS];
        for (i, v) in idx.iter_mut().enumerate().take(self.len) {
            *v = i;
        }
        for a in 1..self.len {
            let mut b = a;
            while b > 0 && self.mw[idx[b]] > self.mw[idx[b - 1]] {
                idx.swap(b, b - 1);
                b -= 1;
            }
        }
        idx
    }
}

// ---------------------------------------------------------------------------
// F255 唤醒配额 — 后台唤醒预算
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct WakeQuota {
    pub limit_per_min: u16,
    pub used: u16,
}

impl WakeQuota {
    pub const fn new(limit: u16) -> WakeQuota {
        WakeQuota { limit_per_min: limit, used: 0 }
    }
    pub fn allowed(&self) -> bool {
        self.used < self.limit_per_min
    }
    pub fn consume(&mut self) -> bool {
        if self.allowed() {
            self.used += 1;
            true
        } else {
            false
        }
    }
    pub fn tick_minute(&mut self) {
        self.used = 0;
    }
}

// F255 — 配额决策：允许 / 丢弃（超额不唤醒）。
pub fn wake_gate(q: &mut WakeQuota) -> bool {
    q.consume()
}

// ---------------------------------------------------------------------------
// F256 场景电源计划 — 会议/创作/旅途/安静
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Meeting,
    Creation,
    Travel,
    Quiet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenePlan {
    pub screen_permille: u16,
    pub cpu_boost: bool,
    pub radio_on: bool,
    pub silence: bool,
}

// F256 — 场景 → 电源计划映射（纯函数，含"会议必静音"等约束）。
pub fn scene_plan(scene: Scene, battery_cs: i32) -> ScenePlan {
    let low = battery_cs < 2000;
    match scene {
        Scene::Meeting => ScenePlan { screen_permille: 700, cpu_boost: false, radio_on: true, silence: true },
        Scene::Creation => ScenePlan { screen_permille: 900, cpu_boost: true, radio_on: true, silence: false },
        Scene::Travel => ScenePlan {
            screen_permille: if low { 250 } else { 500 },
            cpu_boost: false,
            radio_on: !low,
            silence: low,
        },
        Scene::Quiet => ScenePlan { screen_permille: 400, cpu_boost: false, radio_on: false, silence: true },
    }
}

// ---------------------------------------------------------------------------
// F257 电源自动化 — 低电自动动作链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LowPowerAction {
    DimScreen,
    KillBackground,
    SaveWork,
    Hibernate,
}

/// 依次检查阈值（CS：5%→500，10%→1000，20%→2000），返回首个触发的动作。
pub fn low_power_chain(soc_cs: i32, done: &mut [bool; 4]) -> Option<LowPowerAction> {
    let chain: [(i32, LowPowerAction); 4] = [
        (2000, LowPowerAction::DimScreen),
        (1000, LowPowerAction::KillBackground),
        (500, LowPowerAction::SaveWork),
        (200, LowPowerAction::Hibernate),
    ];
    for (i, (thr, act)) in chain.iter().enumerate() {
        if soc_cs <= *thr && !done[i] {
            done[i] = true;
            return Some(*act);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// F258 充电学习 — 按用户作息推迟满充保养
// ---------------------------------------------------------------------------

/// 学到的用户起床/离桌时刻（0~23）。满充保养：夜里只充到 80%，起床前 1h 充满。
pub fn smart_charge_target(soc_cs: i32, hour: u8, wake_hour: u8, user_active: bool) -> u8 {
    let pre_wake = (wake_hour + 23) % 24; // 起床前 1 小时
    if user_active || hour == pre_wake || hour == wake_hour {
        100
    } else if soc_cs < 8000 {
        80
    } else {
        80
    }
}

// ---------------------------------------------------------------------------
// F259 散热画像 — 机型散热档案（温升常数学习）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ThermalProfile {
    /// 历史负载% → 达到稳态温度的 EMA。
    pub steady_mc: [i32; 5], // 负载档 0/25/50/75/100
    pub samples: [u16; 5],
}

impl ThermalProfile {
    pub const fn new() -> ThermalProfile {
        ThermalProfile { steady_mc: [0; 5], samples: [0; 5] }
    }
    pub fn load_slot(load_pct: u8) -> usize {
        ((load_pct as usize) / 25).min(4)
    }
    pub fn observe(&mut self, load_pct: u8, temp_mc: i32) {
        let s = Self::load_slot(load_pct);
        if self.samples[s] == 0 {
            self.steady_mc[s] = temp_mc;
        } else {
            self.steady_mc[s] = (self.steady_mc[s] * 7 + temp_mc * 3) / 10;
        }
        self.samples[s] = self.samples[s].saturating_add(1);
    }
    pub fn predict_mc(&self, load_pct: u8) -> i32 {
        self.steady_mc[Self::load_slot(load_pct)]
    }
}

// ---------------------------------------------------------------------------
// F260 热舒适模式 — 表面温度优先的降频策略
// ---------------------------------------------------------------------------

/// 表面舒适上限（默认 42 °C），超出则按超额度数压 P-state。
pub fn comfort_pstate(skin_mc: i32, comfort_mc: i32, current_p: u8) -> u8 {
    if skin_mc <= comfort_mc {
        return current_p;
    }
    let over_c = (skin_mc - comfort_mc) / 1000;
    let drop = (over_c as u8).saturating_mul(2);
    current_p.saturating_sub(drop).max(1)
}

// ---------------------------------------------------------------------------
// F261 声学风扇曲线 — 噪声优先
// ---------------------------------------------------------------------------

/// 在满足温度底线的前提下取最安静转速（RPM）。
pub fn acoustic_fan_rpm(temp_mc: i32, quiet_floor_mc: i32, max_rpm: u16) -> u16 {
    if temp_mc <= quiet_floor_mc {
        return 0; // 被动散热
    }
    let over_c = ((temp_mc - quiet_floor_mc) / 1000) as u16;
    let rpm = over_c.saturating_mul(400).saturating_add(600);
    rpm.min(max_rpm)
}

// ---------------------------------------------------------------------------
// F262 环境光策略 — 亮度自适应（含夜间压制）
// ---------------------------------------------------------------------------

/// lux(0~10000) → 背光千分比；夜间（hour<6||hour>=22）上限 40%。
pub fn ambient_brightness(lux: u16, hour: u8) -> u16 {
    let base = ((lux as u32) * 900 / 10_000).saturating_add(100) as u16; // 10%~100%
    let capped = base.min(1000);
    if hour < 6 || hour >= 22 {
        capped.min(400)
    } else {
        capped
    }
}

// ---------------------------------------------------------------------------
// F263 多源测温仲裁 — 传感器融合
// ---------------------------------------------------------------------------

/// 取中位数（去掉离群：与中位差 >10°C 的样本被剔除后重算）。
pub fn fuse_temps(mc: &[i32]) -> Option<i32> {
    if mc.is_empty() {
        return None;
    }
    let mut v: [i32; 8] = [0; 8];
    let n = mc.len().min(8);
    v[..n].copy_from_slice(&mc[..n]);
    v[..n].sort_unstable();
    let mut med = v[n / 2];
    // 剔除离群后重算一次
    let mut kept = [0i32; 8];
    let mut k = 0;
    for &x in mc.iter().take(8) {
        if (x - med).abs() <= 10_000 {
            kept[k] = x;
            k += 1;
        }
    }
    if k > 0 {
        kept[..k].sort_unstable();
        med = kept[k / 2];
    }
    Some(med)
}

// ---------------------------------------------------------------------------
// F264 能耗回归基线 — 功耗自动比对
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerDelta {
    Improved,
    Within,
    Regressed,
}

/// 基线偏差 > +5% 判回归，< -2% 判改善。
pub fn power_regression(baseline_mw: u32, measured_mw: u32) -> PowerDelta {
    if baseline_mw == 0 {
        return PowerDelta::Within;
    }
    let delta_permille = ((measured_mw as i64 - baseline_mw as i64) * 1000 / baseline_mw as i64) as i32;
    if delta_permille > 50 {
        PowerDelta::Regressed
    } else if delta_permille < -20 {
        PowerDelta::Improved
    } else {
        PowerDelta::Within
    }
}

// ---------------------------------------------------------------------------
// F265 充电效率分析 — 电源路径优化
// ---------------------------------------------------------------------------

/// 效率 permille = 实入电量 / 插头电量；低于 750 告警。
pub fn charge_efficiency_permille(plug_mah: u32, gained_mah: u32) -> u16 {
    if plug_mah == 0 {
        return 0;
    }
    let eff = ((gained_mah as u64) * 1000 / plug_mah as u64) as u16;
    eff.min(1000)
}

pub fn charge_efficiency_alert(eff_permille: u16) -> bool {
    eff_permille > 0 && eff_permille < 750
}

// ---------------------------------------------------------------------------
// F266 睡眠成功率追踪 — 失眠自诊断
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SleepTracker {
    pub attempts: u16,
    pub ok: u16,
    /// 最近 8 次失败原因（0=无，1=设备拒绝，2=唤醒风暴，3=超时）。
    pub fail_codes: [u8; 8],
    pub head: usize,
}

impl SleepTracker {
    pub const fn new() -> SleepTracker {
        SleepTracker { attempts: 0, ok: 0, fail_codes: [0; 8], head: 0 }
    }
    pub fn record_success(&mut self) {
        self.attempts += 1;
        self.ok += 1;
    }
    pub fn record_fail(&mut self, code: u8) {
        self.attempts += 1;
        self.fail_codes[self.head] = code;
        self.head = (self.head + 1) % 8;
    }
    pub fn rate_permille(&self) -> u16 {
        if self.attempts == 0 {
            return 0;
        }
        ((self.ok as u32) * 1000 / self.attempts as u32) as u16
    }
    /// 唤醒风暴占比（code==2）。
    pub fn wake_storm_permille(&self) -> u16 {
        let total = self.attempts - self.ok;
        if total == 0 {
            return 0;
        }
        let storms = self.fail_codes.iter().filter(|&&c| c == 2).count() as u32;
        (storms * 1000 / total as u32).min(1000) as u16
    }
}

// ---------------------------------------------------------------------------
// F267 网络唤醒控制台 — WoL 管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WolEntry {
    /// MAC 低 48 位。
    pub mac48: u64,
    pub enabled: bool,
}

/// 构造 Magic Packet 前 6 字节头（FF×6）。
pub fn wol_magic_header(out: &mut [u8; 6]) {
    out.fill(0xFF);
}

pub fn wol_filter(entries: &[WolEntry]) -> usize {
    entries.iter().filter(|e| e.enabled).count()
}

// ---------------------------------------------------------------------------
// F268 假死侦测 — 睡眠卡死自救
// ---------------------------------------------------------------------------

/// 进入睡眠后看门狗到期仍未置 DONE 位 → 判假死。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepHealth {
    Ok,
    SuspectedStuck,
}

pub fn sleep_health(watchdog_ms: u32, done_bit: bool, elapsed_ms: u32) -> SleepHealth {
    if done_bit {
        SleepHealth::Ok
    } else if elapsed_ms > watchdog_ms {
        SleepHealth::SuspectedStuck
    } else {
        SleepHealth::Ok
    }
}

/// 假死自救路径：回滚到 S0 并记录。
pub fn stuck_recovery(h: SleepHealth) -> bool {
    h == SleepHealth::SuspectedStuck
}

// ---------------------------------------------------------------------------
// F269 电源 fuzz — 电源状态对抗
// ---------------------------------------------------------------------------

/// 对任意 (soc_cs, temp_mc, load) 组合保证返回合法档位且不 panic。
pub fn fuzz_power_state(soc_cs: i32, temp_mc: i32, load_pct: u8) -> u8 {
    let soc = soc_cs.clamp(0, 10_000);
    let temp = temp_mc.clamp(-40000, 125_000);
    let p = if temp > 90_000 {
        1
    } else if soc < 500 && load_pct > 200 {
        1
    } else if load_pct > 80 {
        8
    } else {
        4
    };
    p.clamp(1, 8)
}

// ---------------------------------------------------------------------------
// F270 电源 API 版本化
// ---------------------------------------------------------------------------

pub const ENERGY_API_VERSION: u32 = 2;
pub const ENERGY_API_MIN: u32 = 1;

pub fn energy_api_ok(requested: u32) -> bool {
    requested >= ENERGY_API_MIN && requested <= ENERGY_API_VERSION
}

// ---------------------------------------------------------------------------
// F271 能耗周报 — 人话报告（字节渲染）
// ---------------------------------------------------------------------------

/// 生成一行周报：`ENERGY week 1234mW top=display 42%`。
pub fn weekly_report(out: &mut [u8], s: &EnergySnapshot) -> usize {
    use crate::checks::{push_str, push_usize};
    let mut n = 0;
    let (total, top, top_pm) = dashboard_summary(s);
    push_str(out, &mut n, "ENERGY week ");
    push_usize(out, &mut n, total as usize);
    push_str(out, &mut n, "mW top=bucket");
    push_usize(out, &mut n, top);
    push_str(out, &mut n, " ");
    push_usize(out, &mut n, top_pm as usize);
    push_str(out, &mut n, "permille");
    n
}

// ---------------------------------------------------------------------------
// F272 USB-PD 协商 — 供电档位预留
// ---------------------------------------------------------------------------

/// PD PPS 档位表（mV, mA）。
pub const PD_PROFILES: [(u16, u16); 6] = [
    (5000, 3000),
    (9000, 3000),
    (15000, 3000),
    (20000, 3250),
    (20000, 5000),
    (28000, 5000),
];

/// 给定需求功率(mW)，选最小满足的档；无则 0xFFFF。
pub fn pd_pick(mw_needed: u32) -> usize {
    for (i, (mv, ma)) in PD_PROFILES.iter().enumerate() {
        if (*mv as u32) * (*ma as u32) / 1000 >= mw_needed {
            return i;
        }
    }
    0xFFFF
}

// ---------------------------------------------------------------------------
// F273 离网电源预留 — UPS/太阳能
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffGridMode {
    Grid,
    Ups,
    Solar,
}

/// 离网策略：UPS 上切省电台账，太阳能下尽量白天重载。
pub fn offgrid_policy(mode: OffGridMode, soc_cs: i32, hour: u8) -> u8 {
    match mode {
        OffGridMode::Grid => 4,
        OffGridMode::Ups => {
            if soc_cs < 3000 {
                1
            } else {
                2
            }
        }
        OffGridMode::Solar => {
            if (6..18).contains(&hour) {
                8
            } else {
                2
            }
        }
    }
}

// ---------------------------------------------------------------------------
// F274 静音服务器档 — 机架化部署
// ---------------------------------------------------------------------------

/// 机架模式：风扇封顶、LED 灭、指示蜂鸣禁用；温度超临界才允许升fan。
pub fn rack_silent_fan(temp_mc: i32, critical_mc: i32, max_rpm: u16) -> u16 {
    if temp_mc >= critical_mc {
        max_rpm
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// F275 能源域自检（M500）
// ---------------------------------------------------------------------------

pub fn run_energy_checks() -> CheckSet {
    let mut set = CheckSet::new("energy-m500");

    // F251
    let mut s = EnergySnapshot::new();
    s.buckets[BUCKET_DISPLAY] = 4200;
    s.buckets[BUCKET_CPU] = 2600;
    let (total, top, top_pm) = dashboard_summary(&s);
    set.add("F251 dashboard total", total == 6800, "sum");
    set.add("F251 dashboard top", top == BUCKET_DISPLAY && top_pm == 617, "display dominant");

    // F252
    let mut grid = [900u16; 24];
    grid[2] = 120;
    grid[14] = 60;
    set.add("F252 charge now clean", carbon_charge_advice(&grid, 2, 6) == ChargeAdvice::ChargeNow, "2h clean");
    set.add("F252 wait for solar", carbon_charge_advice(&grid, 20, 24) == ChargeAdvice::WaitUntil(14), "pick 14h");
    set.add("F252 now is best", carbon_charge_advice(&grid, 14, 4) == ChargeAdvice::ChargeNow, "already lowest");

    // F253
    let mut rp = RuntimePredictor::new();
    rp.push(10_000, 0);
    rp.push(9_000, 3_600_000);
    set.add("F253 slope 10%/h", rp.drain_cs_per_hour() == 1000, "learned 1000cs/h");
    set.add("F253 remaining 600min", rp.remaining_min(10_000) == 600, "10h at 10%/h");

    // F254
    let mut rk = PowerRank::new();
    rk.record(b"app_a", 900);
    rk.record(b"app_b", 1200);
    rk.record(b"app_a", 100);
    let idx = rk.order();
    set.add("F254 accumulate", rk.mw[0] == 1000, "app_a merged");
    set.add("F254 rank order", idx[0] == 1 && idx[1] == 0, "b first");
    rk.record(b"app_c", 5);
    set.add("F254 evict small", rk.len == 3 && rk.mw.iter().take(3).all(|&v| v >= 5), "app_c in slot 3");

    // F255
    let mut q = WakeQuota::new(2);
    let a1 = wake_gate(&mut q);
    let a2 = wake_gate(&mut q);
    let used_before_tick = q.used;
    let a3 = wake_gate(&mut q);
    set.add("F255 quota 2 ok", a1 && a2, "first two pass");
    set.add("F255 quota deny", !a3, "third denied");
    q.tick_minute();
    set.add("F255 quota reset", wake_gate(&mut q), "after tick");

    // F256
    let meet = scene_plan(Scene::Meeting, 9000);
    let trav_low = scene_plan(Scene::Travel, 1000);
    set.add("F256 meeting silent", meet.silence && !meet.cpu_boost, "meeting plan");
    set.add("F256 travel low", trav_low.screen_permille == 250 && !trav_low.radio_on, "low battery plan");
    set.add("F256 creation boost", scene_plan(Scene::Creation, 9000).cpu_boost, "creation");

    // F257
    let mut done = [false; 4];
    let first = low_power_chain(1800, &mut done);
    let second = low_power_chain(1800, &mut done);
    set.add("F257 chain first dim", first == Some(LowPowerAction::DimScreen), "20% -> dim");
    set.add("F257 chain once", second.is_none(), "no repeat");
    let mut done2 = [true, true, true, false];
    set.add("F257 chain hibernate", low_power_chain(100, &mut done2) == Some(LowPowerAction::Hibernate), "2% -> hibernate");

    // F258
    set.add("F258 night 80", smart_charge_target(5000, 1, 7, false) == 80, "preserve battery");
    set.add("F258 prewake 100", smart_charge_target(5000, 6, 7, false) == 100, "ready by wake");
    set.add("F258 active 100", smart_charge_target(5000, 13, 7, true) == 100, "user active");

    // F259
    let mut tp = ThermalProfile::new();
    tp.observe(100, 80_000);
    tp.observe(100, 82_000);
    let mid = tp.steady_mc[4];
    set.add("F259 ema", mid == 80_600, "first*0.7+second*0.3");
    set.add("F259 predict", tp.predict_mc(100) == mid, "slot 4");
    set.add("F259 slot map", ThermalProfile::load_slot(255) == 4 && ThermalProfile::load_slot(0) == 0, "clamp");

    // F260
    set.add("F260 comfort ok", comfort_pstyle_held(41_000, 42_000, 8) == 8, "no drop");
    set.add("F260 comfort drop", comfort_pstyle_held(45_000, 42_000, 8) == 2, "3C over -> -6");
    set.add("F260 floor 1", comfort_pstyle_held(60_000, 42_000, 8) >= 1, "never off");

    // F261
    set.add("F261 passive", acoustic_fan_rpm(50_000, 60_000, 4000) == 0, "under floor");
    set.add("F261 step", acoustic_fan_rpm(63_000, 60_000, 4000) == 1800, "600+3*400");
    set.add("F261 cap", acoustic_fan_rpm(99_000, 60_000, 4000) == 4000, "clamped");

    // F262
    set.add("F262 bright day", ambient_brightness(10_000, 12) == 1000, "full");
    set.add("F262 night cap", ambient_brightness(10_000, 23) == 400, "night 40%");
    set.add("F262 dark min", ambient_brightness(0, 12) == 100, "floor 10%");

    // F263
    let med = fuse_temps(&[45_000, 46_000, 90_000]);
    set.add("F263 outlier rejected", med == Some(46_000), "90C dropped");
    set.add("F263 empty", fuse_temps(&[]).is_none(), "no sensors");

    // F264
    set.add("F264 regressed", power_regression(10_000, 10_600) == PowerDelta::Regressed, "+6%");
    set.add("F264 improved", power_regression(10_000, 9_700) == PowerDelta::Improved, "-3%");
    set.add("F264 within", power_regression(10_000, 10_200) == PowerDelta::Within, "+2%");

    // F265
    set.add("F265 eff 900", charge_efficiency_permille(1000, 900) == 900, "ratio");
    set.add("F265 alert", charge_efficiency_alert(700) && !charge_efficiency_alert(900), "threshold 750");

    // F266
    let mut st = SleepTracker::new();
    st.record_success();
    st.record_success();
    st.record_fail(2);
    set.add("F266 rate 667", st.rate_permille() == 666 || st.rate_permille() == 667, "2/3");
    set.add("F266 storm 1000", st.wake_storm_permille() == 1000, "only fail is storm");
    st.record_success();
    set.add("F266 rate 750", st.rate_permille() == 750, "3/4");

    // F267
    let mut hdr = [0u8; 6];
    wol_magic_header(&mut hdr);
    let entries = [
        WolEntry { mac48: 0xAABBCCDD0011, enabled: true },
        WolEntry { mac48: 0xAABBCCDD0022, enabled: false },
        WolEntry { mac48: 0xAABBCCDD0033, enabled: true },
    ];
    set.add("F267 magic ff", hdr.iter().all(|&b| b == 0xFF), "header");
    set.add("F267 enabled 2", wol_filter(&entries) == 2, "filter");

    // F268
    set.add("F268 done ok", sleep_health(5000, true, 9000) == SleepHealth::Ok, "done bit wins");
    set.add("F268 stuck", sleep_health(5000, false, 6000) == SleepHealth::SuspectedStuck, "timeout");
    set.add("F268 recovery", stuck_recovery(SleepHealth::SuspectedStuck), "rollback path");

    // F269
    set.add("F269 fuzz hot", fuzz_power_state(5000, 95_000, 100) == 1, "critical -> p1");
    set.add("F269 fuzz clamp", fuzz_power_state(-50, 300_000, 255) == 1, "clamped inputs");
    set.add("F269 fuzz busy", fuzz_power_state(9000, 60_000, 90) == 8, "high load");

    // F270
    set.add("F270 api ok", energy_api_ok(1) && energy_api_ok(2), "1~2 valid");
    set.add("F270 api deny", !energy_api_ok(0) && !energy_api_ok(3), "out of range");

    // F271
    let mut buf = [0u8; 64];
    let n = weekly_report(&mut buf, &s);
    let text = core::str::from_utf8(&buf[..n]).unwrap();
    set.add("F271 report renders", text.starts_with("ENERGY week 6800mW"), "format");

    // F272
    set.add("F272 pd 5V", pd_pick(14_000) == 0, "15W covers 14W");
    set.add("F272 pd 100W", pd_pick(99_000) == 4, "20V*5A=100W");

    // F273
    set.add("F273 ups low", offgrid_policy(OffGridMode::Ups, 1000, 12) == 1, "conserve");
    set.add("F273 solar noon", offgrid_policy(OffGridMode::Solar, 9000, 12) == 8, "day power");
    set.add("F273 solar night", offgrid_policy(OffGridMode::Solar, 9000, 23) == 2, "night");

    // F274
    set.add("F274 silent", rack_silent_fan(60_000, 85_000, 6000) == 0, "fan off");
    set.add("F274 critical", rack_silent_fan(86_000, 85_000, 6000) == 6000, "emergency");

    // F275 自检闭环自身
    set.add("F275 self count", set.len() >= 25, "25+ checks");

    set
}

/// F260 的包装（避免与命名歧义，行为同 `comfort_pstate`）。
pub fn comfort_pstyle_held(skin_mc: i32, comfort_mc: i32, current_p: u8) -> u8 {
    comfort_pstate(skin_mc, comfort_mc, current_p)
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f251_buckets_and_summary() {
        let mut s = EnergySnapshot::new();
        s.buckets[BUCKET_GPU] = 1000;
        s.buckets[BUCKET_CPU] = 3000;
        assert_eq!(s.total_mw(), 4000);
        assert_eq!(s.permille(BUCKET_CPU), 750);
        assert_eq!(dashboard_summary(&s).1, BUCKET_CPU);
    }

    #[test]
    fn f252_carbon_window_wraps() {
        let mut grid = [800u16; 24];
        grid[23] = 100;
        grid[0] = 200;
        assert_eq!(carbon_charge_advice(&grid, 22, 3), ChargeAdvice::WaitUntil(23));
    }

    #[test]
    fn f253_charging_is_not_drain() {
        let mut rp = RuntimePredictor::new();
        rp.push(5000, 0);
        rp.push(6000, 3_600_000);
        assert_eq!(rp.drain_cs_per_hour(), 0); // 电量上升不算放电
        assert_eq!(rp.remaining_min(5000), u32::MAX);
    }

    #[test]
    fn f254_full_rank_eviction() {
        let mut rk = PowerRank::new();
        for i in 0..8u32 {
            rk.record(&[b'a' + i as u8], (i + 1) * 100);
        }
        rk.record(b"zz", 5); // 最小不会被顶掉
        assert_eq!(rk.len, 8);
        assert_eq!(rk.order()[0], 7); // 800 最大
    }

    #[test]
    fn f257_chain_order_is_monotonic() {
        let mut done = [false; 4];
        let mut acts = Vec::new();
        while let Some(a) = low_power_chain(0, &mut done) {
            acts.push(a);
        }
        assert_eq!(acts.len(), 4);
        assert_eq!(acts[0], LowPowerAction::DimScreen);
        assert_eq!(acts[3], LowPowerAction::Hibernate);
    }

    #[test]
    fn f263_two_samples_average_median() {
        assert_eq!(fuse_temps(&[40_000, 42_000]), Some(42_000));
    }

    #[test]
    fn f272_pd_boundary() {
        assert_eq!(pd_pick(15_000), 0); // 恰好 5V*3A=15W
        assert_eq!(pd_pick(15_001), 1);
    }

    #[test]
    fn domain_self_test_passes() {
        let set = run_energy_checks();
        assert!(!set.truncated());
        assert!(set.all_passed(), "energy-m500 self-test: {} checks", set.len());
    }
}
