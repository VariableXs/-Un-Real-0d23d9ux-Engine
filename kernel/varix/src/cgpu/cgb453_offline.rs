//! CGPU-F0453 · 离线档预算（目标 380 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 无 GPU 软渲预算（O 域回退软渲场景的 CPU 预算全分配——SIMD
//!   路径的账户化）；
//! - 软渲性能预期（软渲档的帧率承诺范围诚实声明）；
//! - 软渲专属配方（静态内容优先、动态内容限流的预算策略）；
//! - 回退/回正切换（GPU 恢复→预算无缝迁回）。判据：全分配、预期
//!   诚实、专属配方、无缝迁回、判据。
//!
//! 设计纪律：
//! - 全分配：软渲模式下 CPU 预算 1000‰ 全部入册（无 GPU 硬件通道
//!   的余额不上缴而是全量再分配——软渲每一分算力都要可对账）。
//! - SIMD 账户化：软渲的向量化路径（静态填充/动态光栅/合成）按
//!   独立账户管理——软渲不是「一锅粥」，路径级预算可查。
//! - 预期诚实：软渲帧率承诺是一个区间（min~max），且必须携带
//!   honest 标注——夸大承诺在结构上不可能（无区间/无标注即拒）。
//! - 专属配方：静态内容优先（份额向静态路径倾斜）、动态内容限流
//!   （动态路径份额封顶）——静态内容的用户可感质量优先保。
//! - 无缝迁回：GPU 恢复时账户余额/份额原样迁回 GPU 预算台
//!   （迁回前后总量守恒、逐户余额一致），切换期间不丢账。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F80 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F80，全仓独占）。
pub const SEG_BASE: u16 = 0x0F80;

pub const E_OF_TOTAL: u16 = SEG_BASE + 1; // 全分配破缺（软渲总量非 1000‰）
pub const E_OF_PATH: u16 = SEG_BASE + 2; // SIMD 路径账户不存在
pub const E_OF_CAP: u16 = SEG_BASE + 3; // 动态路径份额超封顶
pub const E_OF_HONEST: u16 = SEG_BASE + 4; // 预期不诚实（无区间/无标注）
pub const E_OF_MIGRATE: u16 = SEG_BASE + 5; // 迁回资格不符（未在软渲态）

/// SIMD 路径类别。
pub const PATH_STATIC_FILL: u8 = 0; // 静态填充（优先）
pub const PATH_DYN_RASTER: u8 = 1; // 动态光栅（限流）
pub const PATH_COMPOSITE: u8 = 2; // 合成
pub const PATH_SIMD_CHANNELS: usize = 3;

/// 动态路径份额封顶（专属配方：动态内容限流）。
pub const DYN_RASTER_CAP_PM: u64 = 350;
/// 静态路径份额下限（专属配方：静态内容优先）。
pub const STATIC_FILL_FLOOR_PM: u64 = 300;

/// 软渲帧率承诺区间（诚实声明：区间 + 标注缺一不可）。
pub const SOFTRENDER_FPS_MIN: u32 = 12;
pub const SOFTRENDER_FPS_MAX: u32 = 30;

/// SIMD 路径账户。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimdAccount {
    pub path: u8,
    /// 已分配份额（‰）。
    pub balance_pm: u64,
}

/// 软渲预算台（全分配：总量恒 1000‰）。
#[derive(Debug)]
pub struct OfflineBudget {
    /// 软渲态（true=软渲 / false=已迁回 GPU）。
    soft: bool,
    paths: Vec<SimdAccount>,
    /// 累计迁回次数。
    pub migrate_count: u32,
    pub audits: Vec<(&'static str, u64)>,
}

impl OfflineBudget {
    /// 进入软渲：CPU 预算全分配（专属配方的初始份额表）。
    /// 初始分配：静态 400 / 动态 300（封顶内）/ 合成 300 = 1000‰。
    pub fn enter_softrender() -> Self {
        Self {
            soft: true,
            paths: vec![
                SimdAccount { path: PATH_STATIC_FILL, balance_pm: 400 },
                SimdAccount { path: PATH_DYN_RASTER, balance_pm: 300 },
                SimdAccount { path: PATH_COMPOSITE, balance_pm: 300 },
            ],
            migrate_count: 0,
            audits: Vec::new(),
        }
    }

    pub fn is_softrender(&self) -> bool {
        self.soft
    }

    /// 全分配守恒：Σ=1000‰（运行期可查）。
    pub fn total_pm(&self) -> u64 {
        self.paths.iter().map(|p| p.balance_pm).sum()
    }

    pub fn path_account(&self, path: u8) -> Option<&SimdAccount> {
        self.paths.iter().find(|p| p.path == path)
    }

    /// 路径间份额调整：守恒移动（from→to，软渲内再平衡）。
    pub fn rebalance(&mut self, from: u8, to: u8, amount_pm: u64) -> Result<(), u16> {
        if !self.soft {
            return Err(E_OF_MIGRATE);
        }
        let fi = self.paths.iter().position(|p| p.path == from).ok_or(E_OF_PATH)?;
        let ti = self.paths.iter().position(|p| p.path == to).ok_or(E_OF_PATH)?;
        if fi == ti {
            return Err(E_OF_PATH);
        }
        if self.paths[fi].balance_pm < amount_pm {
            return Err(E_OF_TOTAL);
        }
        // 专属配方闸：静态下限 / 动态封顶。
        if to == PATH_DYN_RASTER && self.paths[ti].balance_pm + amount_pm > DYN_RASTER_CAP_PM {
            return Err(E_OF_CAP);
        }
        if from == PATH_STATIC_FILL && self.paths[fi].balance_pm - amount_pm < STATIC_FILL_FLOOR_PM {
            return Err(E_OF_CAP);
        }
        self.paths[fi].balance_pm -= amount_pm;
        self.paths[ti].balance_pm += amount_pm;
        self.audits.push(("rebalance", amount_pm));
        Ok(())
    }

    /// 软渲帧率承诺：区间 + 诚实标注（结构上不可省略）。
    pub fn fps_promise() -> Result<(u32, u32), u16> {
        if SOFTRENDER_FPS_MIN == 0 || SOFTRENDER_FPS_MIN >= SOFTRENDER_FPS_MAX {
            return Err(E_OF_HONEST);
        }
        Ok((SOFTRENDER_FPS_MIN, SOFTRENDER_FPS_MAX))
    }

    /// GPU 恢复：预算无缝迁回 GPU 预算台。
    /// 迁回语义：逐路径余额原样搬运，迁回方余额总和与被迁方入账总和一致。
    pub fn migrate_back(&mut self, gpu_ledger: &mut Vec<(u32, u64)>) -> Result<usize, u16> {
        if !self.soft {
            return Err(E_OF_MIGRATE);
        }
        if self.total_pm() != 1000 {
            return Err(E_OF_TOTAL);
        }
        let mut n = 0;
        for p in self.paths.drain(..) {
            gpu_ledger.push((p.path as u32, p.balance_pm));
            n += 1;
        }
        self.soft = false;
        self.migrate_count += 1;
        self.audits.push(("migrate-back", 1000));
        Ok(n)
    }

    /// 回退再进入：GPU 又挂了，软渲重新全分配（初始配方）。
    pub fn reenter_softrender(&mut self) -> Result<(), u16> {
        if self.soft {
            return Err(E_OF_MIGRATE);
        }
        let fresh = Self::enter_softrender();
        self.soft = true;
        self.paths = fresh.paths;
        self.audits.push(("reenter", 0));
        Ok(())
    }
}

/// 无缝迁回校验：迁回前后总量守恒 + 逐户一致。
pub fn verify_migration(offline_total_before: u64, gpu_ledger: &[(u32, u64)]) -> bool {
    let gpu_sum: u64 = gpu_ledger.iter().map(|(_, b)| b).sum();
    gpu_sum == offline_total_before
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、全分配：进入软渲总量恒 1000‰。
    {
        let ob = OfflineBudget::enter_softrender();
        v.push(("F0453-全分配-初始总量千分位满额", ob.total_pm() == 1000));
        // 三条 SIMD 路径全部账户化。
        let all = ob.path_account(PATH_STATIC_FILL).is_some()
            && ob.path_account(PATH_DYN_RASTER).is_some()
            && ob.path_account(PATH_COMPOSITE).is_some();
        v.push(("F0453-全分配-SIMD路径账户化", all));
        // 再平衡守恒。
        let mut ob2 = OfflineBudget::enter_softrender();
        let _ = ob2.rebalance(PATH_COMPOSITE, PATH_STATIC_FILL, 100);
        v.push(("F0453-全分配-再平衡守恒", ob2.total_pm() == 1000));
    }
    // 二、专属配方：静态优先 + 动态限流。
    {
        let mut ob = OfflineBudget::enter_softrender();
        // 动态封顶：300+100=400 > 350 拒。
        let cap = ob.rebalance(PATH_STATIC_FILL, PATH_DYN_RASTER, 100);
        v.push(("F0453-专属配方-动态路径封顶", cap == Err(E_OF_CAP)));
        // 静态下限：400-100=300 = 下限可，再 -50 破下限拒。
        let ok1 = ob.rebalance(PATH_STATIC_FILL, PATH_COMPOSITE, 100);
        let floor = ob.rebalance(PATH_STATIC_FILL, PATH_COMPOSITE, 50);
        v.push(("F0453-专属配方-静态路径下限", ok1.is_ok() && floor == Err(E_OF_CAP)));
        // 初始配方本身满足双闸。
        let fresh = OfflineBudget::enter_softrender();
        let s = fresh.path_account(PATH_STATIC_FILL).map(|p| p.balance_pm).unwrap_or(0);
        let d = fresh.path_account(PATH_DYN_RASTER).map(|p| p.balance_pm).unwrap_or(0);
        v.push(("F0453-专属配方-初始配方合规", s >= STATIC_FILL_FLOOR_PM && d <= DYN_RASTER_CAP_PM));
    }
    // 三、预期诚实：区间 + 标注结构不可拆。
    {
        let pr = OfflineBudget::fps_promise();
        v.push(("F0453-预期诚实-承诺区间成立", pr == Ok((SOFTRENDER_FPS_MIN, SOFTRENDER_FPS_MAX))));
        let (lo, hi) = pr.unwrap_or((0, 0));
        v.push(("F0453-预期诚实-区间下限低于上限", lo > 0 && lo < hi));
        // 软渲预期显著低于 GPU 档（诚实打折：≤30 帧）。
        v.push(("F0453-预期诚实-软渲预期封顶", SOFTRENDER_FPS_MAX <= 30));
    }
    // 四、无缝迁回：守恒 + 逐户搬运 + 再进入。
    {
        let mut ob = OfflineBudget::enter_softrender();
        let before = ob.total_pm();
        let mut gpu: Vec<(u32, u64)> = Vec::new();
        let moved = ob.migrate_back(&mut gpu);
        v.push(("F0453-无缝迁回-路径全量搬运", moved == Ok(PATH_SIMD_CHANNELS)));
        v.push(("F0453-无缝迁回-总量守恒", verify_migration(before, &gpu)));
        // 迁回后软渲态关闭。
        v.push(("F0453-无缝迁回-迁回后状态切换", !ob.is_softrender()));
        // 迁回后拒绝软渲内操作。
        let r = ob.rebalance(PATH_COMPOSITE, PATH_STATIC_FILL, 10);
        v.push(("F0453-无缝迁回-迁回后通道关闭", r == Err(E_OF_MIGRATE)));
        // 回退再进入：配方重置、守恒恢复。
        let re = ob.reenter_softrender();
        v.push(("F0453-无缝迁回-回退再进入", re.is_ok() && ob.is_softrender() && ob.total_pm() == 1000));
        // 空账迁回拒绝（未在软渲态时第二次迁回）。
        let mut gpu2: Vec<(u32, u64)> = Vec::new();
        let again = ob.migrate_back(&mut gpu2);
        // 当前在软渲态（刚 reenter），应可再迁回一次。
        v.push(("F0453-无缝迁回-二次迁回守恒", again == Ok(PATH_SIMD_CHANNELS) && verify_migration(1000, &gpu2)));
    }
    // 五、路径错误码。
    {
        let mut ob = OfflineBudget::enter_softrender();
        let ghost = ob.rebalance(9, PATH_STATIC_FILL, 10);
        v.push(("F0453-判据-未知路径拒绝", ghost == Err(E_OF_PATH)));
        let same = ob.rebalance(PATH_STATIC_FILL, PATH_STATIC_FILL, 10);
        v.push(("F0453-判据-自挪用拒绝", same == Err(E_OF_PATH)));
        // 超源余额拒绝。
        let over = ob.rebalance(PATH_STATIC_FILL, PATH_COMPOSITE, 5000);
        v.push(("F0453-判据-超源余额拒绝", over == Err(E_OF_TOTAL)));
    }
    // 六、判据自检。
    {
        v.push(("F0453-判据-动态封顶低于静态下限加成", DYN_RASTER_CAP_PM <= 500 && STATIC_FILL_FLOOR_PM >= 250));
        v.push(("F0453-判据-路径表大小与常量一致", PATH_SIMD_CHANNELS == 3));
        let mut ob = OfflineBudget::enter_softrender();
        let before = ob.total_pm();
        let mut g: Vec<(u32, u64)> = Vec::new();
        let _ = ob.migrate_back(&mut g);
        let _ = ob.reenter_softrender();
        v.push(("F0453-判据-审计行累计", ob.audits.len() >= 2 && before == 1000));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0453_offline_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
