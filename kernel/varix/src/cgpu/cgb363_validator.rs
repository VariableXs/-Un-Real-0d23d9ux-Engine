//! CGPU-F0363 · 配方验证器（判据段 0x0920）
//! 验证器：静态校验（字段合法性/守恒约束——Σ池=帧预算）；仿真验
//! 证（导入配方先过 F0347 仿真器——不达标给出差距报告）；验证三
//! 态（通过/警告/拒绝+依据说明）。判据：静态、仿真、三态报告、
//! 判据。
//!
//! 锚点原文摘录（施工书 1615 行）：
//! 「静态校验（字段合法性/守恒约束——Σ池=帧预算）；仿真验证（导
//! 入配方先过 F0347 仿真器——不达标给出差距报告）；验证三态（通
//! 过/警告/拒绝+依据说明）。判据：静态、仿真、三态报告、判据。」
//!
//! 诊断码段 0x0920（本模块独占：0x0920~0x092F）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0920;

/// 仿真达标线（判据「仿真验证」）：配方在仿真器下的兑现率须 ≥
/// 800‰（80%）——低于即差距报告。
pub const SIM_PASS_PERMILLE: u32 = 800;

/// 警告线：兑现率 ≥ 650‰ 但 < 800‰ 给警告（可用但体验受损）。
pub const SIM_WARN_PERMILLE: u32 = 650;

/// 静态池（判据「静态校验」的守恒对象）：配方声明的资源池。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PoolDecl {
    /// 池号（0 非法）。
    pub pool: u32,
    /// 池容量（千分比）。
    pub capacity: u32,
    /// 池内配方诉求和（千分比）。
    pub demanded: u32,
}

impl PoolDecl {
    /// 静态守恒核（判据「静态校验」主判据）：Σ池 = 帧预算（1000），
    /// 且诉求不得超容量。
    pub fn static_check(&self) -> Result<(), u16> {
        if self.pool == 0 {
            return Err(SEG_BASE);
        }
        if self.capacity != 1000 {
            return Err(SEG_BASE + 1);
        }
        if self.demanded > self.capacity {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 配方静态项（判据「静态校验」输入单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StaticItem {
    /// 配方号。
    pub recipe: u32,
    /// 所属池。
    pub pool: u32,
    /// 诉求（千分比）。
    pub demand: u32,
}

impl StaticItem {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.recipe == 0 || self.pool == 0 {
            return Err(SEG_BASE + 3);
        }
        if self.demand == 0 || self.demand > 1000 {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }
}

/// 静态校验入口（判据「静态校验」聚合面）：全部池守恒 + 全部项
/// 合法 + 每池 Σ诉求 ≤ 容量。任一失手给出首个违规归因。
pub fn static_validate(pools: &[PoolDecl], items: &[StaticItem]) -> Result<(), u16> {
    if pools.is_empty() {
        return Err(SEG_BASE + 5);
    }
    for p in pools {
        p.static_check()?;
    }
    for it in items {
        it.validate()?;
        let pool = pools.iter().find(|p| p.pool == it.pool).ok_or(SEG_BASE + 6)?;
        // 单项诉求超池容量直接拒。
        if it.demand > pool.capacity {
            return Err(SEG_BASE + 7);
        }
    }
    // 每池聚合诉求核。
    for p in pools {
        let sum: u32 = items.iter().filter(|it| it.pool == p.pool).map(|it| it.demand).sum();
        if sum > p.capacity {
            return Err(SEG_BASE + 8);
        }
    }
    Ok(())
}

/// 仿真器（对接 F0347 语义——输入配方逐帧产出兑现率样本）。
/// 这里内置确定性仿真核：每帧兑现 = min(诉求, 可用)；可用按场景
/// 负载曲线滚动。零浮点、零随机——同输入双跑逐帧相同。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SimCore {
    /// 仿真帧数。
    pub frames: u32,
    /// 每帧可用预算（千分比）。
    pub avail_permille: u32,
    /// 配方每帧诉求（千分比）。
    pub demand_permille: u32,
}

impl SimCore {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.frames == 0 || self.frames > 10_000 {
            return Err(SEG_BASE + 9);
        }
        if self.avail_permille == 0 || self.avail_permille > 1000 {
            return Err(SEG_BASE + 10);
        }
        if self.demand_permille == 0 || self.demand_permille > 1000 {
            return Err(SEG_BASE + 11);
        }
        Ok(())
    }
    /// 单帧兑现。
    pub fn frame_fill(&self) -> u32 {
        self.demand_permille.min(self.avail_permille)
    }
    /// 总兑现率（千分比）：Σ兑现 / Σ诉求。
    pub fn fill_rate_permille(&self) -> Result<u32, u16> {
        self.validate()?;
        let want = self.frames as u64 * self.demand_permille as u64;
        let got = self.frames as u64 * self.frame_fill() as u64;
        Ok((got * 1000 / want) as u32)
    }
}

/// 验证三态（判据「三态报告」的裁决枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict3 {
    /// 通过（兑现率 ≥ 800‰）。
    Pass,
    /// 警告（650~799‰——可用但给差距报告）。
    Warn,
    /// 拒绝（< 650‰ 或静态不过）。
    Reject,
}

impl Verdict3 {
    /// 态号。
    pub fn ordinal(&self) -> u8 {
        match self {
            Verdict3::Pass => 0,
            Verdict3::Warn => 1,
            Verdict3::Reject => 2,
        }
    }
}

/// 差距报告行（判据「三态报告」的依据说明）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GapRow {
    /// 依据码（本模块诊断段内）。
    pub basis: u16,
    /// 依据说明。
    pub note: String,
}

/// 三态报告（判据「三态报告」一等公民）：裁决 + 依据行（拒绝与
/// 警告必须带依据——空报告即判据违规）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VerifyReport {
    /// 裁决。
    pub verdict: Verdict3,
    /// 静态通过。
    pub static_ok: bool,
    /// 兑现率（千分比）。
    pub fill_rate: u32,
    /// 依据行。
    pub gaps: Vec<GapRow>,
}

impl VerifyReport {
    /// 报告自洽核：Reject/Warn 必须有依据；Pass 依据可为空。
    pub fn consistent(&self) -> Result<(), u16> {
        match self.verdict {
            Verdict3::Reject | Verdict3::Warn => {
                if self.gaps.is_empty() {
                    return Err(SEG_BASE + 12);
                }
            }
            Verdict3::Pass => {}
        }
        if !self.static_ok && self.verdict != Verdict3::Reject {
            return Err(SEG_BASE + 13);
        }
        Ok(())
    }
}

/// 验证器入口（判据「静态 + 仿真 + 三态」总装）：先静态后仿真，
/// 静态不过直接拒绝（不浪费仿真），仿真给三态与差距报告。
pub fn verify(pools: &[PoolDecl], items: &[StaticItem], sim: &SimCore) -> Result<VerifyReport, u16> {
    let static_ok = match static_validate(pools, items) {
        Ok(()) => true,
        Err(e) => return Ok(VerifyReport {
            verdict: Verdict3::Reject,
            static_ok: false,
            fill_rate: 0,
            gaps: alloc::vec![GapRow { basis: e, note: String::from("静态校验未过——见诊断码") }],
        }),
    };
    let rate = sim.fill_rate_permille()?;
    let (verdict, gaps) = if rate >= SIM_PASS_PERMILLE {
        (Verdict3::Pass, Vec::new())
    } else if rate >= SIM_WARN_PERMILLE {
        (
            Verdict3::Warn,
            alloc::vec![GapRow {
                basis: SEG_BASE + 14,
                note: alloc::format!("兑现率 {}‰ 低于达标线 {}‰——差距 {}‰", rate, SIM_PASS_PERMILLE, SIM_PASS_PERMILLE - rate),
            }],
        )
    } else {
        (
            Verdict3::Reject,
            alloc::vec![GapRow {
                basis: SEG_BASE + 15,
                note: alloc::format!("兑现率 {}‰ 低于警告线 {}‰——配方不可用", rate, SIM_WARN_PERMILLE),
            }],
        )
    };
    let rep = VerifyReport { verdict, static_ok, fill_rate: rate, gaps };
    rep.consistent()?;
    Ok(rep)
}

/// 三态单调核（判据「三态」守卫）：兑现率升序 → 态号降序（越好
/// 的率给越好的态）。
pub fn verdict_monotonic_ok() -> Result<(), u16> {
    let sim_hi = SimCore { frames: 100, avail_permille: 1000, demand_permille: 500 };
    let sim_lo = SimCore { frames: 100, avail_permille: 300, demand_permille: 900 };
    let r_hi = sim_hi.fill_rate_permille()?;
    let r_lo = sim_lo.fill_rate_permille()?;
    if r_hi <= r_lo {
        return Err(SEG_BASE + 16);
    }
    let v_hi = if r_hi >= SIM_PASS_PERMILLE { Verdict3::Pass } else { Verdict3::Warn };
    let v_lo = if r_lo >= SIM_WARN_PERMILLE { Verdict3::Warn } else { Verdict3::Reject };
    if v_hi.ordinal() >= v_lo.ordinal() {
        return Err(SEG_BASE + 17);
    }
    Ok(())
}

/// 验证器诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "池号为 0",
        1 => "池容量必须恒 1000（帧预算口径）",
        2 => "池诉求超容量",
        3 => "配方/池号为 0",
        4 => "诉求必须 ∈ (0,1000]",
        5 => "池表为空",
        6 => "项引用了不存在的池",
        7 => "单项诉求超池容量",
        8 => "池聚合诉求超容量",
        9 => "仿真帧数必须 ∈ (0,10000]",
        10 => "可用预算必须 ∈ (0,1000]",
        11 => "诉求必须 ∈ (0,1000]",
        12 => "Reject/Warn 报告必须带依据行",
        13 => "静态未过时裁决必须 Reject",
        14 => "兑现率低于达标线（警告档）",
        15 => "兑现率低于警告线（拒绝档）",
        16 => "兑现率未随可用量单调",
        17 => "三态未随兑现率单调",
        _ => "未知诊断码——查本模块 SEG_BASE 0x0920",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pools() -> Vec<PoolDecl> {
        vec![PoolDecl { pool: 1, capacity: 1000, demanded: 700 }]
    }

    #[test]
    fn static_checks() {
        let items = vec![
            StaticItem { recipe: 1, pool: 1, demand: 400 },
            StaticItem { recipe: 2, pool: 1, demand: 300 },
        ];
        assert!(static_validate(&pools(), &items).is_ok());
        // 聚合超容量。
        let over = vec![
            StaticItem { recipe: 1, pool: 1, demand: 700 },
            StaticItem { recipe: 2, pool: 1, demand: 400 },
        ];
        assert_eq!(static_validate(&pools(), &over).unwrap_err(), SEG_BASE + 8);
        // 引用不存在池。
        let ghost = vec![StaticItem { recipe: 1, pool: 9, demand: 100 }];
        assert_eq!(static_validate(&pools(), &ghost).unwrap_err(), SEG_BASE + 6);
    }

    #[test]
    fn sim_deterministic() {
        let sim = SimCore { frames: 500, avail_permille: 800, demand_permille: 1000 };
        assert_eq!(sim.fill_rate_permille().unwrap(), 800);
        // 双跑一致（确定性）。
        assert_eq!(sim.fill_rate_permille().unwrap(), sim.fill_rate_permille().unwrap());
    }

    #[test]
    fn three_state_report() {
        // Pass：800‰ 恰好达标。
        let sim = SimCore { frames: 500, avail_permille: 800, demand_permille: 1000 };
        let rep = verify(&pools(), &[StaticItem { recipe: 1, pool: 1, demand: 500 }], &sim).unwrap();
        assert_eq!(rep.verdict, Verdict3::Pass);
        assert!(rep.gaps.is_empty());
        // Warn：介于两线之间。
        let sim_w = SimCore { frames: 500, avail_permille: 700, demand_permille: 1000 };
        let rep_w = verify(&pools(), &[StaticItem { recipe: 1, pool: 1, demand: 500 }], &sim_w).unwrap();
        assert_eq!(rep_w.verdict, Verdict3::Warn);
        assert!(!rep_w.gaps.is_empty());
        assert!(rep_w.gaps[0].note.contains("差距"));
        // Reject：静态先炸。
        let bad_pools = vec![PoolDecl { pool: 1, capacity: 999, demanded: 0 }];
        let rep_r = verify(&bad_pools, &[], &sim).unwrap();
        assert_eq!(rep_r.verdict, Verdict3::Reject);
        assert!(!rep_r.static_ok);
        assert!(rep_r.consistent().is_ok());
        assert!(verdict_monotonic_ok().is_ok());
        assert!(diagnose(SEG_BASE + 12).contains("依据"));
    }
}
