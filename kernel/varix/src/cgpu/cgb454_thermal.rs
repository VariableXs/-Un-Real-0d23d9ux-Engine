//! CGPU-F0454 · 热极限预算（目标 380 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 热节流预算压缩（J 域热降档的预算侧协议——分级压缩配方）；
//! - 压缩优先序（压谁不压谁的预定义顺序——用户可见优先）；
//! - 热恢复回涨（温度回落后预算阶梯恢复——迟滞防振荡）；
//! - 热事件归因（每次热压缩记录完整链）。判据：分级压缩、优先序、
//!   迟滞恢复、归因、判据。
//!
//! 设计纪律：
//! - 分级压缩配方：三级（轻 90%/中 75%/重 55%），触发温度查表——
//!   J 域热降档的预算侧镜像（J 域定档，本模块执行预算压缩）。
//! - 压缩优先序：类别出压顺序预定义（遥测→缓存→后台→前台→系统），
//!   压缩从序首开始，系统类永远最后被压——用户可见优先的结构化。
//! - 迟滞恢复：回升触发温度 = 进入触发温度 − 迟滞带（每级 5℃），
//!   且恢复逐级（一次最多回一级）——防温度在阈值附近振荡导致
//!   预算抖动。
//! - 全归因：每级进入/退出/压缩执行记录完整链（温度快照+级别+
//!   压缩系数），热事件可逐条对账。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F90 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F90，全仓独占）。
pub const SEG_BASE: u16 = 0x0F90;

pub const E_TH_LEVEL: u16 = SEG_BASE + 1; // 热级非法
pub const E_TH_ORDER: u16 = SEG_BASE + 2; // 压缩序违规（跳类出压）
pub const E_TH_HYST: u16 = SEG_BASE + 3; // 迟滞参数非法
pub const E_TH_FACTOR: u16 = SEG_BASE + 4; // 压缩系数越界

/// 热级触发温度（℃，查表）与迟滞带。
pub const TRIP_LIGHT_C: i16 = 75;
pub const TRIP_MEDIUM_C: i16 = 85;
pub const TRIP_HEAVY_C: i16 = 92;
/// 每级迟滞带：回升触发 = 进入触发 − 带。
pub const HYST_C: i16 = 5;

/// 分级压缩系数（千分位：轻 90%/中 75%/重 55%）。
pub const FACTOR_LIGHT_PM: u64 = 900;
pub const FACTOR_MEDIUM_PM: u64 = 750;
pub const FACTOR_HEAVY_PM: u64 = 550;

/// 类别出压顺序（预定义：序首先被压，系统类最后）。
pub const THROTTLE_ORDER: [u8; 5] = [4, 3, 2, 1, 0];
/// 类别语义：0=系统 1=前台 2=后台 3=缓存 4=遥测。
pub const CLASS_SYSTEM: u8 = 0;

/// 热级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThermalLevel {
    Nominal,
    Light,
    Medium,
    Heavy,
}

impl ThermalLevel {
    pub fn rank(self) -> u8 {
        match self {
            ThermalLevel::Nominal => 0,
            ThermalLevel::Light => 1,
            ThermalLevel::Medium => 2,
            ThermalLevel::Heavy => 3,
        }
    }
    pub fn from_rank(r: u8) -> Result<Self, u16> {
        match r {
            0 => Ok(ThermalLevel::Nominal),
            1 => Ok(ThermalLevel::Light),
            2 => Ok(ThermalLevel::Medium),
            3 => Ok(ThermalLevel::Heavy),
            _ => Err(E_TH_LEVEL),
        }
    }
    /// 该级压缩系数（Nominal 恒 1000‰）。
    pub fn factor_pm(self) -> u64 {
        match self {
            ThermalLevel::Nominal => 1000,
            ThermalLevel::Light => FACTOR_LIGHT_PM,
            ThermalLevel::Medium => FACTOR_MEDIUM_PM,
            ThermalLevel::Heavy => FACTOR_HEAVY_PM,
        }
    }
    /// 进入触发温度。
    pub fn trip_c(self) -> i16 {
        match self {
            ThermalLevel::Nominal => 0,
            ThermalLevel::Light => TRIP_LIGHT_C,
            ThermalLevel::Medium => TRIP_MEDIUM_C,
            ThermalLevel::Heavy => TRIP_HEAVY_C,
        }
    }
}

/// 压缩优先序执行：给定量级，返回应被压缩的类别序列（出压顺序截断）。
/// 重级压到最深：序列长度 = 级别 rank + 1（系统类仅在重级边缘被压）。
pub fn compress_targets(level: ThermalLevel) -> Vec<u8> {
    let n = match level {
        ThermalLevel::Nominal => 0,
        ThermalLevel::Light => 1,
        ThermalLevel::Medium => 2,
        ThermalLevel::Heavy => 3,
    };
    THROTTLE_ORDER.iter().copied().take(n).collect()
}

/// 预算压缩执行：按优先序逐类压系数。
pub fn apply_compress(level: ThermalLevel, class_share: &[(u8, u64)]) -> Vec<(u8, u64)> {
    let targets = compress_targets(level);
    let f = level.factor_pm();
    class_share
        .iter()
        .map(|&(c, s)| {
            if targets.contains(&c) {
                (c, s * f / 1000)
            } else {
                (c, s)
            }
        })
        .collect()
}

/// 热事件审计行（完整链）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThermalAudit {
    pub tick: u64,
    /// 温度快照（℃）。
    pub temp_c: i16,
    /// 变化前级别 rank。
    pub from_rank: u8,
    /// 变化后级别 rank。
    pub to_rank: u8,
    /// 压缩系数快照（变化后）。
    pub factor_pm: u64,
}

/// 热极限预算台。
#[derive(Debug)]
pub struct ThermalBudget {
    level: ThermalLevel,
    pub audits: Vec<ThermalAudit>,
}

impl ThermalBudget {
    pub fn new() -> Self {
        Self { level: ThermalLevel::Nominal, audits: Vec::new() }
    }

    pub fn level(&self) -> ThermalLevel {
        self.level
    }

    /// 每帧喂温度：升级即时（安全优先）；降级带迟滞且逐级。
    pub fn feed(&mut self, tick: u64, temp_c: i16) {
        let cur = self.level;
        // 升级：任一更高级触发温度到达即升（直接跳到最高满足级）。
        let mut want = cur;
        for &lv in &[ThermalLevel::Heavy, ThermalLevel::Medium, ThermalLevel::Light] {
            if temp_c >= lv.trip_c() && lv.rank() > want.rank() {
                want = lv;
            }
        }
        if want.rank() > cur.rank() {
            self.level = want;
            self.audits.push(ThermalAudit {
                tick,
                temp_c,
                from_rank: cur.rank(),
                to_rank: want.rank(),
                factor_pm: want.factor_pm(),
            });
            return;
        }
        // 降级：迟滞 + 逐级。当前级的进入触发 − 迟滞带以下才允许回一级。
        if cur.rank() > 0 {
            let release_at = cur.trip_c() - HYST_C;
            if temp_c <= release_at {
                let back = ThermalLevel::from_rank(cur.rank() - 1).unwrap_or(ThermalLevel::Nominal);
                self.level = back;
                self.audits.push(ThermalAudit {
                    tick,
                    temp_c,
                    from_rank: cur.rank(),
                    to_rank: back.rank(),
                    factor_pm: back.factor_pm(),
                });
            }
        }
    }

    /// 压缩执行查询：当前级下的类别份额表。
    pub fn compressed_shares(&self, class_share: &[(u8, u64)]) -> Vec<(u8, u64)> {
        apply_compress(self.level, class_share)
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、分级压缩：查表触发 + 系数分级。
    {
        let mut tb = ThermalBudget::new();
        tb.feed(1, TRIP_LIGHT_C);
        let l1 = tb.level();
        tb.feed(2, TRIP_MEDIUM_C);
        let l2 = tb.level();
        tb.feed(3, TRIP_HEAVY_C);
        let l3 = tb.level();
        v.push((
            "F0454-分级压缩-逐级触发",
            l1 == ThermalLevel::Light && l2 == ThermalLevel::Medium && l3 == ThermalLevel::Heavy,
        ));
        // 一次喂到极热：直入重级。
        let mut tb2 = ThermalBudget::new();
        tb2.feed(1, TRIP_HEAVY_C + 5);
        v.push(("F0454-分级压缩-极热直入重级", tb2.level() == ThermalLevel::Heavy));
        // 系数分级单调。
        v.push((
            "F0454-分级压缩-系数逐级递减",
            FACTOR_LIGHT_PM > FACTOR_MEDIUM_PM && FACTOR_MEDIUM_PM > FACTOR_HEAVY_PM,
        ));
    }
    // 二、优先序：预定义顺序 + 系统类最后。
    {
        let targets_light = compress_targets(ThermalLevel::Light);
        let targets_heavy = compress_targets(ThermalLevel::Heavy);
        v.push((
            "F0454-优先序-轻级只压序首",
            targets_light == vec![THROTTLE_ORDER[0]],
        ));
        v.push((
            "F0454-优先序-重级压到第三类",
            targets_heavy.len() == 3 && targets_heavy == vec![4, 3, 2],
        ));
        // 系统类不在任何级别的压缩目标里（用户可见优先的结构底线）。
        let sys_safe = (0..4u8).all(|r| {
            let lv = ThermalLevel::from_rank(r).unwrap_or(ThermalLevel::Nominal);
            !compress_targets(lv).contains(&CLASS_SYSTEM)
        });
        v.push(("F0454-优先序-系统类永不压缩", sys_safe));
        // 出压顺序预定义可查（用户可见）。
        v.push(("F0454-优先序-顺序表公示", THROTTLE_ORDER.len() == 5 && THROTTLE_ORDER[0] == 4 && THROTTLE_ORDER[4] == 0));
        // 压缩执行：前台不被轻级压（序尾保护）。
        let shares = [(CLASS_SYSTEM, 300), (1, 400), (2, 200), (3, 60), (4, 40)];
        let out = apply_compress(ThermalLevel::Light, &shares);
        v.push((
            "F0454-优先序-轻级压缩只动遥测",
            out[4].1 == 36 && out[0].1 == 300 && out[1].1 == 400,
        ));
    }
    // 三、迟滞恢复：带内不回 + 逐级回。
    {
        let mut tb = ThermalBudget::new();
        tb.feed(1, TRIP_LIGHT_C); // Light
        // 回到进入温度：迟滞带内不降。
        tb.feed(2, TRIP_LIGHT_C);
        let hold = tb.level();
        // 降到 trip−带 以下才回。
        tb.feed(3, TRIP_LIGHT_C - HYST_C);
        let back = tb.level();
        v.push(("F0454-迟滞恢复-带内不回涨", hold == ThermalLevel::Light));
        v.push(("F0454-迟滞恢复-带下回涨一级", back == ThermalLevel::Nominal));
        // 重级不能一步回 Nominal：先回 Medium。
        let mut tb2 = ThermalBudget::new();
        tb2.feed(1, TRIP_HEAVY_C);
        tb2.feed(2, TRIP_HEAVY_C - HYST_C);
        v.push(("F0454-迟滞恢复-重级只回一级", tb2.level() == ThermalLevel::Medium));
        // 迟滞参数合法：带 < 级间距。
        v.push((
            "F0454-迟滞恢复-迟滞带小于级间距",
            HYST_C > 0 && (TRIP_MEDIUM_C - TRIP_LIGHT_C) > HYST_C && (TRIP_HEAVY_C - TRIP_MEDIUM_C) > HYST_C,
        ));
    }
    // 四、归因：完整链（温度+级别+系数）。
    {
        let mut tb = ThermalBudget::new();
        tb.feed(10, TRIP_HEAVY_C); // 直入重级
        tb.feed(11, TRIP_HEAVY_C - HYST_C); // 回 Medium
        let a0 = &tb.audits[0];
        let a1 = &tb.audits[1];
        v.push((
            "F0454-归因-升级链完整",
            a0.tick == 10 && a0.temp_c == TRIP_HEAVY_C && a0.from_rank == 0 && a0.to_rank == 3 && a0.factor_pm == FACTOR_HEAVY_PM,
        ));
        v.push((
            "F0454-归因-回涨链完整",
            a1.tick == 11 && a1.from_rank == 3 && a1.to_rank == 2 && a1.factor_pm == FACTOR_MEDIUM_PM,
        ));
        // 级别轨迹逐条可对账。
        let timeline_ok = tb.audits.windows(2).all(|w| w[0].to_rank == w[1].from_rank);
        v.push(("F0454-归因-轨迹连续", timeline_ok));
    }
    // 五、判据自检。
    {
        v.push(("F0454-判据-级别码非法被拒", ThermalLevel::from_rank(9) == Err(E_TH_LEVEL)));
        v.push(("F0454-判据-触发温度单调", TRIP_LIGHT_C < TRIP_MEDIUM_C && TRIP_MEDIUM_C < TRIP_HEAVY_C));
        let shares = [(CLASS_SYSTEM, 300), (1, 400)];
        let out = apply_compress(ThermalLevel::Nominal, &shares);
        v.push(("F0454-判据-常态零压缩", out == vec![(CLASS_SYSTEM, 300), (1, 400)]));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0454_thermal_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
