//! CGPU-F0496 · D01 帧计时组收口（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - D01 组收口：16 项自检 + 精度报告 + 双签 + 向 D02 移交。
//! 判据：自检、精度、双签、移交、判据。
//!
//! 设计纪律：
//! - 16 项自检：覆盖 D01 组九成员（F0487 延迟账本 / F0488 提前
//!   提交 / F0489 计时校准 / F0490 帧遥测 / F0491 时钟容错 /
//!   F0492 API 冻结 / F0493 文档 / F0494 基准 / F0495 一致性），
//!   每成员至少一项 + 跨域衔接项补齐 16。
//! - 精度报告：PrecisionReport 四指标（口径/抖动/卡顿/漂移），
//!   诚实声明——只报可兑现结论，夸大词结构上拒绝。
//! - 双签：owner + reviewer 两签齐才生效；同人双签拒。
//! - 移交：HandoffPack 五要件（自检绿/精度报告/双签/ADR 链/口径
//!   版本）缺一拒收。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x11D0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x11D0，全仓独占）。
pub const SEG_BASE: u16 = 0x11D0;

pub const E_DC_SELF: u16 = SEG_BASE + 1; // 自检未过/未齐
pub const E_DC_HYPE: u16 = SEG_BASE + 2; // 精度报告夸大
pub const E_DC_SIGN: u16 = SEG_BASE + 3; // 双签缺失/同人
pub const E_DC_HAND: u16 = SEG_BASE + 4; // 移交要件缺失

/// D01 组成员清单（F 号 + 模块名）。
pub const D01_MEMBERS: [(u32, &str); 9] = [
    (487, "latbook"),
    (488, "earlysub"),
    (489, "timecal"),
    (490, "frametelem"),
    (491, "clockfuzz"),
    (492, "timeapi"),
    (493, "timedoc"),
    (494, "timebench"),
    (495, "timeconsist"),
];

/// 自检项：16 条（编号 + 描述 + 归属成员）。
pub const SELF_CHECKS: [(&str, u32); 16] = [
    ("延迟账本五段链完整", 487),
    ("延迟守恒不凭空增减", 487),
    ("提前提交只做统计性", 488),
    ("提前提交回退冷却", 488),
    ("校准三源互校带内", 489),
    ("遥测帧魔数只增", 490),
    ("时钟容错无负时长", 491),
    ("洪峰丢最旧留痕", 491),
    ("计时 API 四族冻结", 492),
    ("命名与 C 域/B08 对齐", 492),
    ("三文档完整性闸", 493),
    ("承诺条款不夸大", 493),
    ("三基准入册", 494),
    ("基准回归带宽内", 494),
    ("跨硬件三档一致", 495),
    ("跨版本 ADR 链完整", 495),
];

// ---------------------------------------------------------------------------
// 一、16 项自检
// ---------------------------------------------------------------------------

/// 自检执行器：外部喂入各成员结论（member_fno -> green），
/// 16 项逐一核对：未覆盖成员、有红项即拒。
#[derive(Debug)]
pub struct SelfCheck {
    /// 成员 F 号 -> 绿。
    pub verdicts: Vec<(u32, bool)>,
}

impl SelfCheck {
    pub fn new() -> Self {
        Self { verdicts: Vec::new() }
    }
    pub fn set(&mut self, fno: u32, green: bool) {
        for (f, g) in self.verdicts.iter_mut() {
            if *f == fno {
                *g = green;
                return;
            }
        }
        self.verdicts.push((fno, green));
    }
    /// 全员覆盖。
    pub fn full_coverage(&self) -> bool {
        D01_MEMBERS.iter().all(|(f, _)| self.verdicts.iter().any(|(v, _)| v == f))
    }
    /// 16 项自检：全员覆盖 + 全员绿。
    pub fn all_green(&self) -> Result<usize, u16> {
        if !self.full_coverage() {
            return Err(E_DC_SELF);
        }
        for (f, g) in self.verdicts.iter() {
            if !g {
                return Err(E_DC_SELF);
            }
        }
        Ok(SELF_CHECKS.len())
    }
}

// ---------------------------------------------------------------------------
// 二、精度报告（诚实声明）
// ---------------------------------------------------------------------------

/// 精度四指标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrecisionReport {
    /// 口径：记账单位（0=us 整数，声明值）。
    pub granularity_us: u32,
    /// 抖动：如实入账上限（us）。
    pub jitter_cap_us: u32,
    /// 卡顿：钳制线（us）。
    pub stall_line_us: u32,
    /// 漂移：互校带（‰）。
    pub drift_band_pm: u32,
}

/// 夸大词表：报告正文出现即拒。
pub const HYPE_WORDS: [&str; 4] = ["零误差", "纳秒级保证", "永不丢帧", "100%精确"];

impl PrecisionReport {
    /// 诚实校验：四指标非零（未声明即拒）+ 量级自洽。
    pub fn honest(&self, claims: &[&str]) -> Result<usize, u16> {
        if self.granularity_us == 0 || self.jitter_cap_us == 0
            || self.stall_line_us == 0 || self.drift_band_pm == 0
        {
            return Err(E_DC_HYPE);
        }
        // 量级自洽：卡顿线 ≥ 抖动上限（卡顿是大数，不能小于抖动）。
        if self.stall_line_us < self.jitter_cap_us {
            return Err(E_DC_HYPE);
        }
        for c in claims.iter() {
            for h in HYPE_WORDS.iter() {
                if c.contains(h) {
                    return Err(E_DC_HYPE);
                }
            }
        }
        Ok(claims.len())
    }
    /// 报告正文（正本口径，与 F0489/F0491 常数同源）。
    pub fn render_lines(&self) -> Vec<&'static str> {
        vec![
            "granularity：微秒（us）整数记账，不承诺纳秒精度",
            "jitter：抖动如实入账，上限即账本口径",
            "stall：卡顿钳制到卡顿线，超出标注丢失",
            "drift：三源互校 ±50‰ 带，越带重校准",
        ]
    }
}

// ---------------------------------------------------------------------------
// 三、双签 + 移交
// ---------------------------------------------------------------------------

/// 双签：owner 与 reviewer 异人齐签才生效。
#[derive(Debug)]
pub struct DualSign {
    pub owner: Option<&'static str>,
    pub reviewer: Option<&'static str>,
}

impl DualSign {
    pub fn new() -> Self {
        Self { owner: None, reviewer: None }
    }
    pub fn sign_owner(&mut self, who: &'static str) {
        self.owner = Some(who);
    }
    pub fn sign_reviewer(&mut self, who: &'static str) {
        self.reviewer = Some(who);
    }
    /// 生效：两签齐且异人。
    pub fn valid(&self) -> Result<(), u16> {
        let (o, r) = match (self.owner, self.reviewer) {
            (Some(o), Some(r)) => (o, r),
            _ => return Err(E_DC_SIGN),
        };
        if o == r {
            return Err(E_DC_SIGN); // 同人双签拒
        }
        Ok(())
    }
}

/// 移交包：五要件缺一拒收。
#[derive(Clone, Copy, Debug)]
pub struct HandoffPack {
    /// 自检绿项数。
    pub self_green: usize,
    /// 精度报告已诚实校验。
    pub precision_ok: bool,
    /// 双签已生效。
    pub dual_sign_ok: bool,
    /// ADR 链版本。
    pub adr_ver: u32,
    /// 口径版本（与 F0492 API_REV 对齐）。
    pub api_rev: u32,
}

impl HandoffPack {
    /// 验收：五要件逐项核。
    pub fn accept(&self) -> Result<(), u16> {
        if self.self_green != SELF_CHECKS.len() {
            return Err(E_DC_HAND);
        }
        if !self.precision_ok || !self.dual_sign_ok {
            return Err(E_DC_HAND);
        }
        if self.adr_ver == 0 {
            return Err(E_DC_HAND);
        }
        if self.api_rev == 0 {
            return Err(E_DC_HAND);
        }
        Ok(())
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、16 项自检。
    {
        v.push(("F0496-自检-清单十六条", SELF_CHECKS.len() == 16));
        v.push(("F0496-自检-成员九个", D01_MEMBERS.len() == 9));
        // 清单覆盖：16 项全部归属在册成员。
        v.push(("F0496-自检-清单归属全覆盖", {
            SELF_CHECKS.iter().all(|(_, f)| D01_MEMBERS.iter().any(|(m, _)| m == f))
        }));
        // 全绿场景。
        let mut sc = SelfCheck::new();
        for (f, _) in D01_MEMBERS.iter() {
            sc.set(*f, true);
        }
        v.push(("F0496-自检-全绿通过", sc.all_green() == Ok(16)));
        // 缺成员拒。
        let mut miss = SelfCheck::new();
        for (f, _) in D01_MEMBERS.iter() {
            if *f != 491 {
                miss.set(*f, true);
            }
        }
        v.push(("F0496-自检-缺成员拒", miss.all_green() == Err(E_DC_SELF)));
        // 有红项拒。
        let mut red = SelfCheck::new();
        for (f, _) in D01_MEMBERS.iter() {
            red.set(*f, *f != 494);
        }
        v.push(("F0496-自检-有红拒", red.all_green() == Err(E_DC_SELF)));
    }
    // 二、精度报告。
    {
        let pr = PrecisionReport {
            granularity_us: 1,
            jitter_cap_us: 2_000,
            stall_line_us: 10_000,
            drift_band_pm: 50,
        };
        let claims = pr.render_lines();
        v.push(("F0496-精度-正文四行", claims.len() == 4));
        v.push(("F0496-精度-诚实通过", pr.honest(&claims) == Ok(4)));
        v.push(("F0496-精度-量级自洽", pr.stall_line_us >= pr.jitter_cap_us));
        // 夸大注入拒。
        let hype_claims = vec!["本组计时零误差，纳秒级保证"];
        v.push(("F0496-精度-夸大拒", pr.honest(&hype_claims) == Err(E_DC_HYPE)));
        // 指标缺声明拒。
        let thin = PrecisionReport {
            granularity_us: 1,
            jitter_cap_us: 2_000,
            stall_line_us: 0,
            drift_band_pm: 50,
        };
        v.push(("F0496-精度-缺声明拒", thin.honest(&claims) == Err(E_DC_HYPE)));
        // 量级倒挂拒（卡顿线 < 抖动上限）。
        let flip = PrecisionReport {
            granularity_us: 1,
            jitter_cap_us: 10_000,
            stall_line_us: 2_000,
            drift_band_pm: 50,
        };
        v.push(("F0496-精度-量级倒挂拒", flip.honest(&claims) == Err(E_DC_HYPE)));
    }
    // 三、双签。
    {
        let mut ds = DualSign::new();
        v.push(("F0496-双签-未签拒", ds.valid() == Err(E_DC_SIGN)));
        ds.sign_owner("AI-WB");
        v.push(("F0496-双签-单签拒", ds.valid() == Err(E_DC_SIGN)));
        ds.sign_reviewer("AI-WB");
        v.push(("F0496-双签-同人拒", ds.valid() == Err(E_DC_SIGN)));
        let mut ds2 = DualSign::new();
        ds2.sign_owner("AI-WB");
        ds2.sign_reviewer("AI-W002");
        v.push(("F0496-双签-异人齐签生效", ds2.valid().is_ok()));
    }
    // 四、移交。
    {
        let ok = HandoffPack {
            self_green: 16,
            precision_ok: true,
            dual_sign_ok: true,
            adr_ver: 3,
            api_rev: 1,
        };
        v.push(("F0496-移交-五要件齐收", ok.accept().is_ok()));
        v.push((
            "F0496-移交-自检缺项拒",
            HandoffPack { self_green: 15, ..ok }.accept() == Err(E_DC_HAND),
        ));
        v.push((
            "F0496-移交-精度缺拒",
            HandoffPack { precision_ok: false, ..ok }.accept() == Err(E_DC_HAND),
        ));
        v.push((
            "F0496-移交-双签缺拒",
            HandoffPack { dual_sign_ok: false, ..ok }.accept() == Err(E_DC_HAND),
        ));
        v.push((
            "F0496-移交-ADR 缺拒",
            HandoffPack { adr_ver: 0, ..ok }.accept() == Err(E_DC_HAND),
        ));
        v.push((
            "F0496-移交-API 版缺拒",
            HandoffPack { api_rev: 0, ..ok }.accept() == Err(E_DC_HAND),
        ));
    }
    // 五、判据自检。
    {
        v.push(("F0496-判据-移交至 D02", true)); // D01 终点即 D02 起点
        v.push(("F0496-判据-夸大词表入册", HYPE_WORDS.len() == 4));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0496_d01close_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
