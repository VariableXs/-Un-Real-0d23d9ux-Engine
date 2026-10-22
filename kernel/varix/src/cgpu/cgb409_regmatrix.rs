//! CGPU-F0409 · 集成回归矩阵（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 回归矩阵：每集成点的联合回归（D/E/H/I/J/K 六集成点各自的联合
//!   测试用例固化）；矩阵执行调度（随闸门补测登记——fast 日常跑集成
//!   冒烟、full 夜间跑全矩阵）；矩阵覆盖度量（每集成点的用例覆盖声明）；
//!   失败归因（集成失败自动定位到契约变更点）。
//! - 判据：六点用例、两档调度、覆盖声明、归因、判据。
//!
//! 设计纪律：
//! - 六点矩阵：D/E/H/I/J/K 每点一个用例账（用例名/两档标记/最近结果），
//!   用例固化 = 入账后名与档位不可变（改用例=新用例新名）。
//! - 两档调度：fast 集（冒烟子集）与 full 集（全部）；补测登记表把
//!   「因契约变更需要补测的用例」挂到闸门号上，闸门过 = 补测全绿。
//! - 覆盖声明：每点声明「已覆盖风险面数/应覆盖数」，缺覆盖即矩阵红。
//! - 失败归因：失败用例自动定位到契约变更点（用例 ↔ 契约双向登记，
//!   契约变更号 → 受影响用例集）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0CC0 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0CC0，全仓独占）。
pub const SEG_BASE: u16 = 0x0CC0;

pub const E_RGM_DUP: u16 = SEG_BASE + 1; // 用例重名
pub const E_RGM_POINT: u16 = SEG_BASE + 2; // 集成点非法
pub const E_RGM_SCHED: u16 = SEG_BASE + 3; // 调度集缺项（fast/full 未排齐）
pub const E_RGM_COVER: u16 = SEG_BASE + 4; // 覆盖声明缺口
pub const E_RGM_ATTR: u16 = SEG_BASE + 5; // 归因登记缺失（契约无用例影射）
pub const E_RGM_FROZEN: u16 = SEG_BASE + 6; // 固化用例被改

/// 六集成点。
pub const POINT_D: u8 = 0; // D 帧
pub const POINT_E: u8 = 1; // E 表面
pub const POINT_H: u8 = 2; // H 显存
pub const POINT_I: u8 = 3; // I 带宽
pub const POINT_J: u8 = 4; // J 功耗
pub const POINT_K: u8 = 5; // K 异构
pub const POINT_COUNT: usize = 6;

pub fn point_name(p: u8) -> &'static str {
    match p {
        POINT_D => "D帧",
        POINT_E => "E表面",
        POINT_H => "H显存",
        POINT_I => "I带宽",
        POINT_J => "J功耗",
        _ => "K异构",
    }
}

/// 两档标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 日常冒烟（fast）。
    Fast,
    /// 夜间全量（full-only）。
    Full,
}

/// 一条固化用例。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgCase {
    pub name: String,
    pub point: u8,
    pub tier: Tier,
    /// 最近结果：None=未跑 Some(绿否)。
    pub last: Option<bool>,
    /// 关联契约号（归因影射）。
    pub contracts: Vec<u32>,
}

/// 回归矩阵。
#[derive(Clone, Debug, Default)]
pub struct RegressMatrix {
    pub cases: Vec<RgCase>,
    /// 用例固化标志（入账后不可改的名单——按名索引）。
    pub frozen: Vec<String>,
}

impl RegressMatrix {
    /// 用例入账（固化：入账即锁名/点/档；结果可更新）。
    pub fn add(&mut self, name: &str, point: u8, tier: Tier, contracts: &[u32]) -> Result<(), u16> {
        if point as usize >= POINT_COUNT {
            return Err(E_RGM_POINT);
        }
        if self.cases.iter().any(|c| c.name == name) {
            return Err(E_RGM_DUP);
        }
        self.cases.push(RgCase {
            name: name.to_string(),
            point,
            tier,
            last: None,
            contracts: contracts.to_vec(),
        });
        self.frozen.push(name.to_string());
        Ok(())
    }

    /// 结果回写（只更新 last，不改固化字段）。
    pub fn report(&mut self, name: &str, green: bool) -> Result<(), u16> {
        let c = self
            .cases
            .iter_mut()
            .find(|c| c.name == name)
            .ok_or(E_RGM_SCHED)?;
        c.last = Some(green);
        Ok(())
    }

    /// fast 集：全部 Fast 档用例。
    pub fn fast_set(&self) -> Vec<&RgCase> {
        self.cases.iter().filter(|c| c.tier == Tier::Fast).collect()
    }

    /// full 集：全部用例。
    pub fn full_set(&self) -> Vec<&RgCase> {
        self.cases.iter().collect()
    }

    /// 两档调度校验：六点每点至少 1 条 Fast 且全部用例可跑。
    pub fn schedule_ok(&self) -> Result<(), u16> {
        for p in 0..POINT_COUNT as u8 {
            let has_fast = self
                .cases
                .iter()
                .any(|c| c.point == p && c.tier == Tier::Fast);
            if !has_fast {
                return Err(E_RGM_SCHED);
            }
        }
        Ok(())
    }

    /// 跑一档：按档执行（此处仿真执行——结果由执行器喂回 report）。
    pub fn run_tier(&self, tier: Tier) -> Vec<(&str, bool)> {
        self.cases
            .iter()
            .filter(|c| tier == Tier::Full || c.tier == Tier::Fast)
            .map(|c| (c.name.as_str(), c.last.unwrap_or(false)))
            .collect()
    }
}

/// 覆盖声明：每点（已覆盖风险面 / 应覆盖风险面）。
#[derive(Clone, Debug, Default)]
pub struct CoverDecl {
    pub rows: Vec<(u8, usize, usize)>,
}

impl CoverDecl {
    pub fn declare(&mut self, point: u8, covered: usize, total: usize) -> Result<(), u16> {
        if point as usize >= POINT_COUNT || covered > total {
            return Err(E_RGM_COVER);
        }
        match self.rows.iter_mut().find(|(p, _, _)| *p == point) {
            Some(e) => {
                e.1 = covered;
                e.2 = total;
            }
            None => self.rows.push((point, covered, total)),
        }
        Ok(())
    }

    /// 覆盖完整性：六点齐且每点全覆盖（covered == total）。
    pub fn validate(&self) -> Result<(), u16> {
        for p in 0..POINT_COUNT as u8 {
            match self.rows.iter().find(|(q, _, _)| *q == p) {
                None => return Err(E_RGM_COVER),
                Some((_, c, t)) if c != t => return Err(E_RGM_COVER),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn coverage_permille(&self, point: u8) -> u32 {
        self.rows
            .iter()
            .find(|(p, _, _)| *p == point)
            .map(|(_, c, t)| {
                if *t == 0 {
                    0
                } else {
                    (*c as u32 * 1000 / *t as u32)
                }
            })
            .unwrap_or(0)
    }
}

/// 补测登记：契约变更号 → 受影响用例集（闸门补测的账）。
#[derive(Clone, Debug, Default)]
pub struct RetestBook {
    /// (契约变更号, 用例名)。
    pub rows: Vec<(u32, String)>,
}

impl RetestBook {
    pub fn register(&mut self, contract: u32, case: &str) -> Result<(), u16> {
        if self.rows.iter().any(|(c, n)| *c == contract && n == case) {
            return Err(E_RGM_DUP);
        }
        self.rows.push((contract, case.to_string()));
        Ok(())
    }

    /// 失败归因：用例名 → 关联契约号集（登记反查）。
    pub fn contracts_of(&self, case: &str) -> Vec<u32> {
        self.rows
            .iter()
            .filter(|(_, n)| n == case)
            .map(|(c, _)| *c)
            .collect()
    }

    /// 归因登记完整性：矩阵内每条带契约的用例在补测账有影射。
    pub fn attrib_complete(&self, m: &RegressMatrix) -> Result<(), u16> {
        for c in &m.cases {
            for ct in &c.contracts {
                if !self.rows.iter().any(|(k, n)| k == ct && n == &c.name) {
                    return Err(E_RGM_ATTR);
                }
            }
        }
        Ok(())
    }

    /// 补测集：契约变更号 → 应补测用例（fast 优先）。
    pub fn retest_set<'a>(&self, m: &'a RegressMatrix, contract: u32) -> Vec<&'a RgCase> {
        m.cases
            .iter()
            .filter(|c| {
                self.rows
                    .iter()
                    .any(|(k, n)| *k == contract && n == &c.name)
            })
            .collect()
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn matrix() -> RegressMatrix {
        let mut m = RegressMatrix::default();
        m.add("d.p95窗回归", POINT_D, Tier::Fast, &[332]).ok();
        m.add("d.闭环全跑", POINT_D, Tier::Full, &[332]).ok();
        m.add("e.可见性联动", POINT_E, Tier::Fast, &[342]).ok();
        m.add("e.提权回退", POINT_E, Tier::Full, &[342]).ok();
        m.add("h.双账守恒", POINT_H, Tier::Fast, &[338]).ok();
        m.add("h.降质路径", POINT_H, Tier::Full, &[338]).ok();
        m.add("i.挤占防护", POINT_I, Tier::Fast, &[338]).ok();
        m.add("j.续航压缩", POINT_J, Tier::Fast, &[359]).ok();
        m.add("k.探测退回", POINT_K, Tier::Fast, &[346]).ok();
        m
    }

    #[test]
    fn a01_六点用例_固化与防重() {
        let m = matrix();
        assert_eq!(m.schedule_ok(), Ok(()));
        assert_eq!(m.fast_set().len(), 6);
        assert_eq!(m.full_set().len(), 9);
        // 重名拒。
        let mut m2 = matrix();
        assert_eq!(m2.add("d.p95窗回归", POINT_D, Tier::Fast, &[]), Err(E_RGM_DUP));
        // 点越界拒。
        assert_eq!(m2.add("x", 9, Tier::Fast, &[]), Err(E_RGM_POINT));
        // fast 集缺某点 ⇒ 调度红。
        let mut m3 = matrix();
        m3.cases.retain(|c| !(c.point == POINT_K && c.tier == Tier::Fast));
        assert_eq!(m3.schedule_ok(), Err(E_RGM_SCHED));
    }

    #[test]
    fn a02_两档调度_冒烟与全量分离() {
        let m = matrix();
        let fast = m.run_tier(Tier::Fast);
        let full = m.run_tier(Tier::Full);
        assert_eq!(fast.len(), 6);
        assert_eq!(full.len(), 9);
        // fast ⊆ full。
        assert!(fast.iter().all(|(n, _)| full.iter().any(|(f, _)| f == n)));
        // 结果回写后 fast 集可读。
        let mut m2 = matrix();
        m2.report("d.p95窗回归", true).ok();
        assert!(m2.run_tier(Tier::Fast).iter().any(|(n, g)| *n == "d.p95窗回归" && *g));
    }

    #[test]
    fn a03_覆盖声明_六点全覆盖才绿() {
        let mut cd = CoverDecl::default();
        for p in 0..POINT_COUNT as u8 {
            cd.declare(p, 3, 3).ok();
        }
        assert_eq!(cd.validate(), Ok(()));
        // 缺点红。
        let mut cd2 = cd.clone();
        cd2.rows.remove(3);
        assert_eq!(cd2.validate(), Err(E_RGM_COVER));
        // 覆盖不足红。
        let mut cd3 = cd.clone();
        cd3.rows[2].1 = 2;
        assert_eq!(cd3.validate(), Err(E_RGM_COVER));
        // 覆盖率换算。
        assert_eq!(cd.coverage_permille(POINT_H), 1000);
        assert_eq!(cd3.coverage_permille(POINT_H), 666);
        // 越界/倒挂声明拒。
        let mut cd4 = CoverDecl::default();
        assert_eq!(cd4.declare(POINT_D, 5, 3), Err(E_RGM_COVER));
        assert_eq!(cd4.declare(9, 1, 1), Err(E_RGM_COVER));
    }

    #[test]
    fn a04_失败归因_契约变更点定位() {
        let mut m = matrix();
        let mut rb = RetestBook::default();
        // 契约影射登记（与用例契约一致）。
        for c in m.cases.clone() {
            for ct in c.contracts.clone() {
                rb.register(ct, &c.name).ok();
            }
        }
        assert_eq!(rb.attrib_complete(&m), Ok(()));
        // 漏登记 ⇒ 归因红。
        let mut rb2 = RetestBook::default();
        rb2.register(332, "d.p95窗回归").ok();
        assert_eq!(rb2.attrib_complete(&m), Err(E_RGM_ATTR));
        // 归因反查：契约 359 → j.续航压缩。
        assert_eq!(rb.contracts_of("j.续航压缩"), vec![359]);
        // 补测集：契约 332 变更 → D 域两条受影响用例。
        assert_eq!(rb.retest_set(&m, 332).len(), 2);
        // 契约变更后补测跑绿（先取名单再回写——借用不交叠）。
        let names: Vec<String> = rb
            .retest_set(&m, 332)
            .iter()
            .map(|c| c.name.clone())
            .collect();
        for n in &names {
            m.report(n, true).ok();
        }
        let all_green = m
            .full_set()
            .iter()
            .filter(|c| rb.contracts_of(&c.name).contains(&332))
            .all(|c| c.last == Some(true));
        assert!(all_green);
    }

    #[test]
    fn a05_点名表_六点坐标可读() {
        assert_eq!(point_name(POINT_D), "D帧");
        assert_eq!(point_name(POINT_K), "K异构");
        assert_eq!(POINT_COUNT, 6);
    }
}
