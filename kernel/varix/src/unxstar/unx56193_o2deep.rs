//! UNX-F56193 启动 O2 深化标定件（0x61820 段）
//!
//! 锚点：UNX-F56193-J1 标定账（启动域 O2 三元组深化标定——标定点 ×
//! 三元组分量 × 偏差值逐点落账）；J2 标定语义（三元组守恒 + 偏差在
//! 阈 + 标定点闭集全覆盖三性质）；J3 标定指纹（标定账折叠指纹）三判
//! 据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·O2 深化标定面）。
//!
//! 结构：O2 三元组 + 标定点闭集 + 标定簿（点 → 偏差）+ 标定核（点
//! 闸 + 偏差阈闸 + 覆盖闸）+ 标定指纹 + 注入核（未知点/偏差超阈/重
//! 点位/覆盖缺失/印后标定必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61820 独占 0x20）。
pub const SEG_BASE: u32 = 0x61820;
pub const O2_UNK_POINT: u32 = SEG_BASE + 0x01; // 未知标定点
pub const O2_DEV_OVER: u32 = SEG_BASE + 0x02; // 偏差超阈
pub const O2_DUP_POINT: u32 = SEG_BASE + 0x03; // 重点位
pub const O2_COVER_MISS: u32 = SEG_BASE + 0x04; // 覆盖缺失
pub const O2_FROZEN: u32 = SEG_BASE + 0x05; // 印后标定

/// 启动域标定点闭集。
pub const OP_BOOT_TOTAL: usize = 0; // 引导总时长
pub const OP_KERNEL_TOTAL: usize = 1; // 内核总时长
pub const OP_LOGIN_TOTAL: usize = 2; // 登录桌面总时长
pub const OP_INTERACT_TOTAL: usize = 3; // 交互就绪总时长
pub const OP_GRAND_TOTAL: usize = 4; // 启动全链总时长
pub const OP_COUNT: usize = 5;

pub const POINT_NAMES: [&str; OP_COUNT] = ["boot", "kernel", "login", "interact", "grand"];

/// O2 三元组（启动域深化标定分量）。
#[derive(Clone, Copy, PartialEq)]
pub struct O2Triple {
    /// 基线值（ms）。
    pub baseline_ms: u64,
    /// 实测值（ms）。
    pub measured_ms: u64,
    /// 允差（ms）。
    pub tolerance_ms: u64,
}

impl O2Triple {
    /// 偏差绝对值。
    pub fn deviation(&self) -> u64 {
        self.baseline_ms.abs_diff(self.measured_ms)
    }

    /// 偏差在阈。
    pub fn within(&self) -> bool {
        self.deviation() <= self.tolerance_ms
    }

    /// 三元组守恒：基线/实测/允差全非零。
    pub fn triple_conserved(&self) -> bool {
        self.baseline_ms > 0 && self.measured_ms > 0 && self.tolerance_ms > 0
    }
}

/// 深化标定簿。
pub struct O2Calibration {
    entries: Vec<(usize, O2Triple)>,
    sealed: bool,
}

impl O2Calibration {
    pub fn new() -> Self {
        O2Calibration {
            entries: Vec::new(),
            sealed: false,
        }
    }

    /// 标定一点：点闸 + 偏差阈闸 + 重点拒 + 印后拒。
    pub fn calibrate(&mut self, point: usize, t: O2Triple) -> Result<(), u32> {
        if self.sealed {
            return Err(O2_FROZEN);
        }
        if point >= OP_COUNT {
            return Err(O2_UNK_POINT);
        }
        if !t.within() {
            return Err(O2_DEV_OVER);
        }
        if self.entries.iter().any(|(p, _)| *p == point) {
            return Err(O2_DUP_POINT);
        }
        self.entries.push((point, t));
        Ok(())
    }

    pub fn entry(&self, point: usize) -> Option<&O2Triple> {
        self.entries.iter().find(|(p, _)| *p == point).map(|(_, t)| t)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 覆盖核：标定点闭集全覆盖。
    pub fn covers_all(&self) -> bool {
        (0..OP_COUNT).all(|p| self.entry(p).is_some())
    }

    /// 全偏差和。
    pub fn total_deviation(&self) -> u64 {
        self.entries.iter().map(|(_, t)| t.deviation()).sum()
    }

    /// 标定指纹。
    pub fn calib_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for (p, t) in &self.entries {
            h = h.wrapping_mul(0x0100_0193) ^ *p as u32;
            h = h.wrapping_mul(0x0100_0193) ^ (t.baseline_ms as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (t.measured_ms as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (t.tolerance_ms as u32);
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封簿印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.covers_all() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_calib(&self) -> String {
        let mut out = String::new();
        for (p, t) in &self.entries {
            out.push_str(&format!(
                "{}|base={}|meas={}|dev={}\n",
                POINT_NAMES[*p],
                t.baseline_ms,
                t.measured_ms,
                t.deviation()
            ));
        }
        if self.sealed {
            out.push_str("O2CAL-SEAL\n");
        }
        out
    }
}

impl Default for O2Calibration {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：五点标定（偏差 ≤ 允差）。
fn standard_calib() -> O2Calibration {
    let mut c = O2Calibration::new();
    let _ = c.calibrate(OP_BOOT_TOTAL, O2Triple { baseline_ms: 800, measured_ms: 795, tolerance_ms: 20 });
    let _ = c.calibrate(OP_KERNEL_TOTAL, O2Triple { baseline_ms: 1800, measured_ms: 1812, tolerance_ms: 30 });
    let _ = c.calibrate(OP_LOGIN_TOTAL, O2Triple { baseline_ms: 2600, measured_ms: 2588, tolerance_ms: 40 });
    let _ = c.calibrate(OP_INTERACT_TOTAL, O2Triple { baseline_ms: 800, measured_ms: 806, tolerance_ms: 20 });
    let _ = c.calibrate(OP_GRAND_TOTAL, O2Triple { baseline_ms: 6000, measured_ms: 6001, tolerance_ms: 60 });
    c
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知点
    let mut c = O2Calibration::new();
    if c.calibrate(9, O2Triple { baseline_ms: 1, measured_ms: 1, tolerance_ms: 1 }) == Err(O2_UNK_POINT) {
        caught += 1;
    }
    // 注 2：偏差超阈
    let mut c = O2Calibration::new();
    if c.calibrate(OP_BOOT_TOTAL, O2Triple { baseline_ms: 800, measured_ms: 900, tolerance_ms: 20 }) == Err(O2_DEV_OVER) {
        caught += 1;
    }
    // 注 3：重点位
    let mut c = standard_calib();
    if c.calibrate(OP_BOOT_TOTAL, O2Triple { baseline_ms: 800, measured_ms: 800, tolerance_ms: 20 }) == Err(O2_DUP_POINT) {
        caught += 1;
    }
    // 注 4：覆盖缺失封印拒
    let mut c = O2Calibration::new();
    let _ = c.calibrate(OP_BOOT_TOTAL, O2Triple { baseline_ms: 800, measured_ms: 800, tolerance_ms: 20 });
    if !c.seal() && !c.sealed() {
        caught += 1;
    }
    // 注 5：印后标定
    let mut c = standard_calib();
    let _ = c.seal();
    if c.calibrate(OP_BOOT_TOTAL, O2Triple { baseline_ms: 1, measured_ms: 1, tolerance_ms: 1 }) == Err(O2_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 标定账；J2 标定语义；J3 标定指纹。
pub fn run_unx56193_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56193");
    // J1-1 五点全标定
    let c = standard_calib();
    let ok = c.len() == OP_COUNT && c.covers_all();
    s.add("56193-J1-五点全标定", ok, if ok { "" } else { "标定账不全" });
    // J1-2 渲染导出齐
    let mut c2 = standard_calib();
    let _ = c2.seal();
    let text = c2.render_calib();
    let ok = text.contains("boot|base=800|meas=795|dev=5")
        && text.contains("grand|base=6000|meas=6001|dev=1")
        && text.contains("O2CAL-SEAL");
    s.add("56193-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 三元组守恒（全部）
    let ok = (0..OP_COUNT).all(|p| c.entry(p).map(|t| t.triple_conserved()).unwrap_or(false));
    s.add("56193-J2-三元组守恒", ok, if ok { "" } else { "守恒漂移" });
    // J2-2 全偏差在阈
    let ok = (0..OP_COUNT).all(|p| c.entry(p).map(|t| t.within()).unwrap_or(false));
    s.add("56193-J2-偏差在阈", ok, if ok { "" } else { "偏差超阈" });
    // J2-3 偏差值抽查（boot 5 / grand 1）
    let ok = c.entry(OP_BOOT_TOTAL).map(|t| t.deviation() == 5).unwrap_or(false)
        && c.entry(OP_GRAND_TOTAL).map(|t| t.deviation() == 1).unwrap_or(false);
    s.add("56193-J2-偏差抽查", ok, if ok { "" } else { "偏差值漂移" });
    // J2-4 覆盖核
    let ok = c.covers_all();
    s.add("56193-J2-覆盖核", ok, if ok { "" } else { "覆盖缺失" });
    // J2-5 总偏差和（5+12+12+6+1 = 36）
    let ok = c.total_deviation() == 36;
    s.add("56193-J2-总偏差和", ok, if ok { "" } else { "总偏差漂移" });
    // J2-6 偏差超阈拒
    let mut c3 = O2Calibration::new();
    let ok = c3.calibrate(OP_KERNEL_TOTAL, O2Triple { baseline_ms: 1800, measured_ms: 2000, tolerance_ms: 30 }) == Err(O2_DEV_OVER);
    s.add("56193-J2-超阈拒", ok, if ok { "" } else { "阈闸失效" });
    // J3-1 标定指纹确定
    let ok = standard_calib().calib_fp() == standard_calib().calib_fp();
    s.add("56193-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 实测变签变
    let mut c4 = standard_calib();
    let fp0 = c4.calib_fp();
    if let Some(t) = c4.entries.first_mut() {
        t.1.measured_ms += 1;
    }
    let ok = c4.calib_fp() != fp0;
    s.add("56193-J3-实测变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封簿印
    let mut c5 = standard_calib();
    let ok = c5.seal() && c5.sealed();
    s.add("56193-J3-封簿印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒标定
    let ok = c5.calibrate(OP_BOOT_TOTAL, O2Triple { baseline_ms: 1, measured_ms: 1, tolerance_ms: 1 }) == Err(O2_FROZEN);
    s.add("56193-J3-印后拒标定", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56193-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56193_all_checks_green() {
        let set = run_unx56193_checks();
        assert!(set.all_passed(), "UNX-F56193 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56193_calib_stable() {
        assert!(standard_calib().covers_all());
    }
}
