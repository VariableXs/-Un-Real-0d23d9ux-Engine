//! CGPU-F0455 · 电池临界预算（目标 380 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 电量分级紧缩（20%/10%/5% 三级预算紧缩配方——续航保护）；
//! - 临界公告（紧缩内容三要素告知用户）；
//! - 用户豁免（用户可临时豁免——知情选择后恢复）；
//! - 与 J 域续航模式联动（模式统一不重复）。判据：三级紧缩、公告、
//!   豁免、联动、判据。
//!
//! 设计纪律：
//! - 三级紧缩：20%/10%/5% 电量查表触发，紧缩配方常量入册
//!   （预算压到 80%/60%/40%——续航保护优先于性能）。
//! - 公告三要素：紧缩内容必须同时携带（级别、压缩比、预计续航
//!   延长档位）——三要素缺一即公告不成立（结构闸）。
//! - 用户豁免：知情选择——豁免只对当前级别生效一次，电量再降级
//!   豁免自动失效（每级重新知情）；豁免期间紧缩解除但持续归因。
//! - J 域联动：模式单一真源——本模块声明「预算侧紧缩级别」，
//!   J 域续航模式为「系统侧模式」，两者通过 rank 对齐函数联动，
//!   不各自为政（模式不一致在联动检查中显性暴露）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0FA0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0FA0，全仓独占）。
pub const SEG_BASE: u16 = 0x0FA0;

pub const E_BT_LEVEL: u16 = SEG_BASE + 1; // 紧缩级非法
pub const E_BT_NOTICE: u16 = SEG_BASE + 2; // 公告三要素缺失
pub const E_BT_EXEMPT: u16 = SEG_BASE + 3; // 豁免资格不符（未知情/重复豁免）
pub const E_BT_LINK: u16 = SEG_BASE + 4; // J 域模式联动不一致

/// 电量触发阈值（%）与紧缩配方（预算压缩到千分位）。
pub const TRIP_20_PM: u64 = 200; // 电量 ≤20%
pub const TRIP_10_PM: u64 = 100; // ≤10%
pub const TRIP_5_PM: u64 = 50; // ≤5%
pub const FACTOR_L1_PM: u64 = 800; // 一级紧缩：压到 80%
pub const FACTOR_L2_PM: u64 = 600; // 二级：60%
pub const FACTOR_L3_PM: u64 = 400; // 三级：40%

/// 续航延长档位（公告第三要素）。
pub const EXTEND_L1: u32 = 30; // 约延长 30%
pub const EXTEND_L2: u32 = 60;
pub const EXTEND_L3: u32 = 100;

/// 紧缩级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SaverLevel {
    Off,
    L1,
    L2,
    L3,
}

impl SaverLevel {
    pub fn rank(self) -> u8 {
        match self {
            SaverLevel::Off => 0,
            SaverLevel::L1 => 1,
            SaverLevel::L2 => 2,
            SaverLevel::L3 => 3,
        }
    }
    pub fn from_rank(r: u8) -> Result<Self, u16> {
        match r {
            0 => Ok(SaverLevel::Off),
            1 => Ok(SaverLevel::L1),
            2 => Ok(SaverLevel::L2),
            3 => Ok(SaverLevel::L3),
            _ => Err(E_BT_LEVEL),
        }
    }
    pub fn factor_pm(self) -> u64 {
        match self {
            SaverLevel::Off => 1000,
            SaverLevel::L1 => FACTOR_L1_PM,
            SaverLevel::L2 => FACTOR_L2_PM,
            SaverLevel::L3 => FACTOR_L3_PM,
        }
    }
    pub fn extend_pct(self) -> u32 {
        match self {
            SaverLevel::Off => 0,
            SaverLevel::L1 => EXTEND_L1,
            SaverLevel::L2 => EXTEND_L2,
            SaverLevel::L3 => EXTEND_L3,
        }
    }
}

/// 电量→级别查表（J 域联动对齐函数）。
pub fn level_for_battery_pct(pct_pm: u64) -> SaverLevel {
    if pct_pm <= TRIP_5_PM {
        SaverLevel::L3
    } else if pct_pm <= TRIP_10_PM {
        SaverLevel::L2
    } else if pct_pm <= TRIP_20_PM {
        SaverLevel::L1
    } else {
        SaverLevel::Off
    }
}

/// J 域续航模式 rank 对齐：系统侧模式（0=正常 1=省电 2=超省 3=极省）
/// 与预算侧紧缩级一致即联动成立。
pub fn link_check(j_mode_rank: u8, saver: SaverLevel) -> Result<(), u16> {
    if j_mode_rank == saver.rank() {
        Ok(())
    } else {
        Err(E_BT_LINK)
    }
}

/// 临界公告（三要素结构：缺一即不成立）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaverNotice {
    /// 要素一：级别。
    pub level: SaverLevel,
    /// 要素二：压缩比（‰）。
    pub factor_pm: u64,
    /// 要素三：预计续航延长（%）。
    pub extend_pct: u32,
}

impl SaverNotice {
    /// 构造公告：三要素自动从配方取，保证一致性。
    pub fn make(level: SaverLevel) -> Self {
        Self { level, factor_pm: level.factor_pm(), extend_pct: level.extend_pct() }
    }
    /// 三要素完整性校验（结构闸：任何一项缺失/错配即不成立）。
    pub fn valid(&self) -> bool {
        self.level.factor_pm() == self.factor_pm
            && self.level.extend_pct() == self.extend_pct
            && (self.level != SaverLevel::Off || (self.factor_pm == 1000 && self.extend_pct == 0))
    }
}

/// 豁免状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exemption {
    /// 豁免针对的级别 rank（只对该级有效）。
    pub level_rank: u8,
    /// 豁免生效时的电量快照（%）。
    pub at_pct: u64,
}

/// 电池临界预算台。
#[derive(Debug)]
pub struct BatteryBudget {
    level: SaverLevel,
    exemption: Option<Exemption>,
    /// 实际生效系数（豁免期间回 1000‰）。
    pub notices: Vec<SaverNotice>,
    pub audits: Vec<(&'static str, u64)>,
}

impl BatteryBudget {
    pub fn new() -> Self {
        Self { level: SaverLevel::Off, exemption: None, notices: Vec::new(), audits: Vec::new() }
    }

    pub fn level(&self) -> SaverLevel {
        self.level
    }

    pub fn exempted(&self) -> bool {
        self.exemption.is_some() && self.exemption.map(|e| e.level_rank) == Some(self.level.rank())
    }

    /// 实际生效压缩系数（豁免期间不压）。
    pub fn effective_factor_pm(&self) -> u64 {
        if self.exempted() {
            1000
        } else {
            self.level.factor_pm()
        }
    }

    /// 每帧喂电量：升级即时；降级即时（Off 恢复无需迟滞——电量
    /// 不会振荡回充除非用户插电，插电即安全解除）。
    pub fn feed(&mut self, tick: u64, pct_pm: u64) {
        let want = level_for_battery_pct(pct_pm);
        if want != self.level {
            let from = self.level.rank();
            self.level = want;
            // 级别变化 → 旧豁免失效（每级重新知情）。
            self.exemption = None;
            self.notices.push(SaverNotice::make(want));
            self.audits.push(("level-change", tick));
            let _ = from;
        }
    }

    /// 公告查询：当前公告必须三要素齐全。
    pub fn current_notice(&self) -> Result<SaverNotice, u16> {
        let n = SaverNotice::make(self.level);
        if !n.valid() {
            return Err(E_BT_NOTICE);
        }
        Ok(n)
    }

    /// 用户豁免：知情选择后解除当前级紧缩（每级一次，电量再降自动失效）。
    pub fn exempt(&mut self, tick: u64, pct_pm: u64) -> Result<(), u16> {
        if self.level == SaverLevel::Off {
            return Err(E_BT_EXEMPT);
        }
        if self.exempted() {
            return Err(E_BT_EXEMPT);
        }
        self.exemption = Some(Exemption { level_rank: self.level.rank(), at_pct: pct_pm });
        self.audits.push(("exempt", tick));
        Ok(())
    }

    /// 撤销豁免（用户主动恢复紧缩）。
    pub fn revoke_exempt(&mut self, tick: u64) -> Result<(), u16> {
        if self.exemption.is_none() {
            return Err(E_BT_EXEMPT);
        }
        self.exemption = None;
        self.audits.push(("revoke", tick));
        Ok(())
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、三级紧缩：查表触发 + 配方分级。
    {
        let mut bb = BatteryBudget::new();
        bb.feed(1, TRIP_20_PM);
        let l1 = bb.level();
        bb.feed(2, TRIP_10_PM);
        let l2 = bb.level();
        bb.feed(3, TRIP_5_PM);
        let l3 = bb.level();
        v.push((
            "F0455-三级紧缩-逐级触发",
            l1 == SaverLevel::L1 && l2 == SaverLevel::L2 && l3 == SaverLevel::L3,
        ));
        // 配方系数分级。
        v.push((
            "F0455-三级紧缩-系数逐级递减",
            FACTOR_L1_PM > FACTOR_L2_PM && FACTOR_L2_PM > FACTOR_L3_PM && FACTOR_L3_PM >= 400,
        ));
        // 插电恢复：直接回 Off。
        bb.feed(4, 900);
        v.push(("F0455-三级紧缩-插电即恢复", bb.level() == SaverLevel::Off && bb.effective_factor_pm() == 1000));
    }
    // 二、公告三要素。
    {
        let mut bb = BatteryBudget::new();
        bb.feed(1, TRIP_20_PM);
        let n = bb.current_notice();
        v.push((
            "F0455-公告-三要素成立",
            n == Ok(SaverNotice { level: SaverLevel::L1, factor_pm: FACTOR_L1_PM, extend_pct: EXTEND_L1 }),
        ));
        // 三要素结构自校验。
        v.push(("F0455-公告-结构闸通过", n.map(|x| x.valid()) == Ok(true)));
        // 每次级别变化自动出新公告。
        bb.feed(2, TRIP_10_PM);
        v.push(("F0455-公告-级别变化出新公告", bb.notices.len() == 2 && bb.notices[1].level == SaverLevel::L2));
        // 续航延长档随级别单调。
        v.push(("F0455-公告-延长档单调", EXTEND_L1 < EXTEND_L2 && EXTEND_L2 < EXTEND_L3));
    }
    // 三、豁免：知情解除 + 每级一次 + 再降失效。
    {
        let mut bb = BatteryBudget::new();
        // Off 级豁免拒绝（无紧缩可豁免）。
        let off = bb.exempt(1, 900);
        v.push(("F0455-豁免-常态豁免拒绝", off == Err(E_BT_EXEMPT)));
        bb.feed(1, TRIP_20_PM);
        let e1 = bb.exempt(2, 20);
        v.push(("F0455-豁免-知情解除生效", e1.is_ok() && bb.effective_factor_pm() == 1000));
        // 同级重复豁免拒绝。
        let e2 = bb.exempt(3, 19);
        v.push(("F0455-豁免-同级重复拒绝", e2 == Err(E_BT_EXEMPT)));
        // 电量再降：豁免自动失效，紧缩恢复。
        bb.feed(4, TRIP_10_PM);
        v.push((
            "F0455-豁免-再降级自动失效",
            bb.level() == SaverLevel::L2 && !bb.exempted() && bb.effective_factor_pm() == FACTOR_L2_PM,
        ));
        // 主动撤销。
        let _ = bb.exempt(5, 10);
        let r = bb.revoke_exempt(6);
        v.push(("F0455-豁免-主动撤销恢复紧缩", r.is_ok() && bb.effective_factor_pm() == FACTOR_L2_PM));
        // 无豁免撤销拒绝。
        let r2 = bb.revoke_exempt(7);
        v.push(("F0455-豁免-无豁免撤销拒绝", r2 == Err(E_BT_EXEMPT)));
    }
    // 四、J 域联动：模式对齐。
    {
        let mut bb = BatteryBudget::new();
        bb.feed(1, TRIP_20_PM);
        // 对齐：J 域省电模式 rank 1 == 预算侧 L1。
        v.push(("F0455-联动-模式对齐通过", link_check(1, bb.level()) == Ok(())));
        // 不一致显性暴露。
        v.push(("F0455-联动-不一致暴露", link_check(0, bb.level()) == Err(E_BT_LINK)));
        // 三级全对齐。
        let mut ok = true;
        for r in 0..4u8 {
            let lv = SaverLevel::from_rank(r).unwrap_or(SaverLevel::Off);
            ok = ok && link_check(r, lv) == Ok(());
        }
        v.push(("F0455-联动-全级对齐", ok));
    }
    // 五、判据自检。
    {
        v.push(("F0455-判据-级别码非法被拒", SaverLevel::from_rank(9) == Err(E_BT_LEVEL)));
        v.push(("F0455-判据-触发阈值单调", TRIP_20_PM > TRIP_10_PM && TRIP_10_PM > TRIP_5_PM));
        // Off 公告也三要素自洽（压缩 1000‰/延长 0）。
        let n_off = SaverNotice::make(SaverLevel::Off);
        v.push(("F0455-判据-常态公告自洽", n_off.valid() && n_off.factor_pm == 1000 && n_off.extend_pct == 0));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0455_battery_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
