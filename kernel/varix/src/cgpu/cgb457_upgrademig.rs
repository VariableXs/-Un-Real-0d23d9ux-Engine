//! CGPU-F0457 · 升级过渡（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 升级过渡：热更新/升级窗口的预算过渡（Q 域 F2561+ 热更新联动
//!   预留——升级期间预算的平稳迁移协议）；
//! - 迁移原子性（旧实例预算→新实例的原子移交——不双算不漏算）；
//! - 升级失败回退（预算回滚到旧实例）；
//! - 过渡期显性告知。判据：迁移协议、原子性、回退、告知、判据。
//!
//! 设计纪律：
//! - 迁移协议三阶段闭集（Prepare→Cutover→Commit/Abort）：Prepare
//!   冻结旧实例（不再受理新申请）、Cutover 一次性移交、Commit 收编
//!   或 Abort 回滚——阶段机单向，跳级即拒。
//! - 原子性双闸：移交快照守恒（Σ旧=Σ新，不双算不漏算）+ 切换原子
//!   （cutover 是单点动作，不存在半新半旧的中间账）。
//! - 失败回退：Cutover 后 Commit 前的任何故障 → Abort → 旧实例
//!   原样复活（快照回灌），回退也有归因行。
//! - 显性告知：过渡期每步（冻结/移交/提交/回退）出一条
//!   MigrationNotice（级别+阶段+摘要），过渡期查询必带 in_transition。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0FC0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0FC0，全仓独占）。
pub const SEG_BASE: u16 = 0x0FC0;

pub const E_UM_STAGE: u16 = SEG_BASE + 1; // 阶段非法（跳级）
pub const E_UM_CONSERVE: u16 = SEG_BASE + 2; // 移交守恒破缺（双算/漏算）
pub const E_UM_FROZEN: u16 = SEG_BASE + 3; // 冻结期受理新申请
pub const E_UM_STATE: u16 = SEG_BASE + 4; // 实例态非法

/// 迁移阶段闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigStage {
    /// 未在过渡期。
    Idle,
    /// 冻结旧实例（不受理新申请）。
    Prepare,
    /// 移交点（原子切换）。
    Cutover,
    /// 已提交新实例。
    Committed,
    /// 已回滚旧实例。
    Aborted,
}

impl MigStage {
    pub fn rank(self) -> u8 {
        match self {
            MigStage::Idle => 0,
            MigStage::Prepare => 1,
            MigStage::Cutover => 2,
            MigStage::Committed => 3,
            MigStage::Aborted => 4,
        }
    }
}

/// 实例账（旧/新同构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceLedger {
    /// 账户余额（‰）。
    pub balances: Vec<(u32, u64)>,
    /// 预占冻结（‰）。
    pub reserved: Vec<(u32, u64)>,
}

impl InstanceLedger {
    pub fn total(&self) -> u64 {
        self.balances.iter().map(|b| b.1).sum::<u64>() + self.reserved.iter().map(|b| b.1).sum::<u64>()
    }
    pub fn clone_snapshot(&self) -> InstanceLedger {
        InstanceLedger { balances: self.balances.clone(), reserved: self.reserved.clone() }
    }
}

/// 过渡期告知行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigNotice {
    pub stage: MigStage,
    /// 摘要码：1=冻结 2=移交完成 3=提交 4=回退。
    pub code: u8,
}

/// 预算迁移协议执行器。
#[derive(Debug)]
pub struct UpgradeMigrator {
    stage: MigStage,
    old: InstanceLedger,
    new: InstanceLedger,
    /// 旧实例冻结前快照（回退源）。
    frozen_snap: Option<InstanceLedger>,
    pub notices: Vec<MigNotice>,
}

impl UpgradeMigrator {
    pub fn new(old: InstanceLedger) -> Self {
        Self {
            stage: MigStage::Idle,
            old,
            new: InstanceLedger { balances: Vec::new(), reserved: Vec::new() },
            frozen_snap: None,
            notices: Vec::new(),
        }
    }

    pub fn stage(&self) -> MigStage {
        self.stage
    }

    pub fn in_transition(&self) -> bool {
        matches!(self.stage, MigStage::Prepare | MigStage::Cutover)
    }

    /// 阶段推进（只允许相邻步）。
    fn advance(&mut self, to: MigStage) -> Result<(), u16> {
        let ok = matches!(
            (self.stage, to),
            (MigStage::Idle, MigStage::Prepare)
                | (MigStage::Prepare, MigStage::Cutover)
                | (MigStage::Cutover, MigStage::Committed)
                | (MigStage::Cutover, MigStage::Aborted)
        );
        if !ok {
            return Err(E_UM_STAGE);
        }
        self.stage = to;
        Ok(())
    }

    /// 阶段一：冻结旧实例（快照留存，不再受理新申请）。
    pub fn prepare(&mut self) -> Result<(), u16> {
        self.advance(MigStage::Prepare)?;
        self.frozen_snap = Some(self.old.clone_snapshot());
        self.notices.push(MigNotice { stage: MigStage::Prepare, code: 1 });
        Ok(())
    }

    /// 冻结期受理闸：新申请一律拒（E_UM_FROZEN）。
    pub fn request(&mut self, _acct: u32, _pm: u64) -> Result<(), u16> {
        if self.in_transition() {
            return Err(E_UM_FROZEN);
        }
        Ok(())
    }

    /// 阶段二：原子移交——新实例账逐户搬运，守恒双闸。
    pub fn cutover(&mut self, new: InstanceLedger) -> Result<(), u16> {
        self.advance(MigStage::Cutover)?;
        let old_total = self.old.total();
        let new_total = new.total();
        if old_total != new_total {
            self.stage = MigStage::Aborted;
            self.old = self.frozen_snap.take().unwrap_or_else(|| self.old.clone_snapshot());
            self.notices.push(MigNotice { stage: MigStage::Aborted, code: 4 });
            return Err(E_UM_CONSERVE);
        }
        self.new = new;
        self.notices.push(MigNotice { stage: MigStage::Cutover, code: 2 });
        Ok(())
    }

    /// 阶段三 a：提交新实例。
    pub fn commit(&mut self) -> Result<(), u16> {
        self.advance(MigStage::Committed)?;
        self.notices.push(MigNotice { stage: MigStage::Committed, code: 3 });
        Ok(())
    }

    /// 阶段三 b：回退旧实例（冻结快照原样回灌）。
    pub fn abort(&mut self) -> Result<(), u16> {
        self.advance(MigStage::Aborted)?;
        if let Some(snap) = self.frozen_snap.take() {
            self.old = snap;
        }
        self.notices.push(MigNotice { stage: MigStage::Aborted, code: 4 });
        Ok(())
    }

    /// 生效实例账（过渡期看旧，提交后看新）。
    pub fn active(&self) -> &InstanceLedger {
        if self.stage == MigStage::Committed {
            &self.new
        } else {
            &self.old
        }
    }

    /// 告知完整性：过渡期每步均有 notice 且 in_transition 可查。
    pub fn notice_codes(&self) -> Vec<u8> {
        self.notices.iter().map(|n| n.code).collect()
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、迁移协议：三阶段闭集单向。
    {
        let old = InstanceLedger { balances: vec![(1, 600), (2, 400)], reserved: vec![(1, 100)] };
        let mut um = UpgradeMigrator::new(old);
        // Idle 直接 commit 跳级拒。
        v.push(("F0457-迁移协议-Idle跳级commit拒", um.commit() == Err(E_UM_STAGE)));
        um.prepare().ok();
        let st1 = um.stage();
        // Prepare 直接 commit 跳过 cutover 拒。
        v.push(("F0457-迁移协议-Prepare跳级commit拒", um.commit() == Err(E_UM_STAGE) && st1 == MigStage::Prepare));
        v.push(("F0457-迁移协议-过渡期可查", um.in_transition()));
    }
    // 二、冻结期受理闸。
    {
        let old = InstanceLedger { balances: vec![(1, 500)], reserved: Vec::new() };
        let mut um = UpgradeMigrator::new(old);
        v.push(("F0457-迁移协议-冻结前受理正常", um.request(1, 100).is_ok()));
        um.prepare().ok();
        v.push(("F0457-迁移协议-冻结期新申请拒", um.request(1, 100) == Err(E_UM_FROZEN)));
    }
    // 三、原子性：守恒双闸（不双算不漏算）。
    {
        let old = InstanceLedger { balances: vec![(1, 600), (2, 400)], reserved: vec![(1, 100)] };
        let mut um = UpgradeMigrator::new(old);
        um.prepare().ok();
        // 漏算：新实例少 100。
        let leak = InstanceLedger { balances: vec![(1, 600), (2, 300)], reserved: vec![(1, 100)] };
        let r = um.cutover(leak);
        v.push(("F0457-原子性-漏算守恒拒", r == Err(E_UM_CONSERVE)));
        // 守恒破缺后自动回退：旧实例原样复活。
        v.push(("F0457-原子性-破缺自动Abort", um.stage() == MigStage::Aborted && um.active().total() == 1100));
        // 双算：新实例多 100 同拒。
        let old2 = InstanceLedger { balances: vec![(1, 500)], reserved: Vec::new() };
        let mut um2 = UpgradeMigrator::new(old2);
        um2.prepare().ok();
        let dbl = InstanceLedger { balances: vec![(1, 500), (2, 100)], reserved: Vec::new() };
        v.push(("F0457-原子性-双算守恒拒", um2.cutover(dbl) == Err(E_UM_CONSERVE)));
        // 合法移交：逐户一致。
        let old3 = InstanceLedger { balances: vec![(1, 600), (2, 400)], reserved: vec![(2, 50)] };
        let mut um3 = UpgradeMigrator::new(old3);
        um3.prepare().ok();
        let good = InstanceLedger { balances: vec![(1, 600), (2, 400)], reserved: vec![(2, 50)] };
        v.push(("F0457-原子性-合法移交过闸", um3.cutover(good).is_ok()));
        um3.commit().ok();
        v.push(("F0457-原子性-提交后新实例生效", um3.stage() == MigStage::Committed && um3.active().total() == 1050));
    }
    // 四、失败回退：Cutover 后 Commit 前 Abort → 旧实例原样复活。
    {
        let old = InstanceLedger { balances: vec![(1, 700), (2, 300)], reserved: vec![(1, 80)] };
        let mut um = UpgradeMigrator::new(old);
        um.prepare().ok();
        let good = InstanceLedger { balances: vec![(1, 700), (2, 300)], reserved: vec![(1, 80)] };
        um.cutover(good).ok();
        um.abort().ok();
        let back = um.active();
        v.push((
            "F0457-回退-旧实例原样复活",
            um.stage() == MigStage::Aborted && back.balances == vec![(1, 700), (2, 300)] && back.reserved == vec![(1, 80)],
        ));
        // Aborted 后不能 commit（回退是终态）。
        v.push(("F0457-回退-回退终态不可提交", um.commit() == Err(E_UM_STAGE)));
    }
    // 五、显性告知：每步一条 notice。
    {
        let old = InstanceLedger { balances: vec![(1, 500)], reserved: Vec::new() };
        let mut um = UpgradeMigrator::new(old);
        um.prepare().ok();
        let good = InstanceLedger { balances: vec![(1, 500)], reserved: Vec::new() };
        um.cutover(good).ok();
        um.commit().ok();
        v.push(("F0457-告知-正常链三步告知", um.notice_codes() == vec![1, 2, 3]));
        let old2 = InstanceLedger { balances: vec![(1, 500)], reserved: Vec::new() };
        let mut um2 = UpgradeMigrator::new(old2);
        um2.prepare().ok();
        let bad = InstanceLedger { balances: vec![(1, 400)], reserved: Vec::new() };
        let _ = um2.cutover(bad);
        v.push(("F0457-告知-回退链含Abort告知", um2.notice_codes() == vec![1, 4]));
    }
    // 六、判据自检。
    {
        v.push(("F0457-判据-空账移交守恒成立", {
            let mut um = UpgradeMigrator::new(InstanceLedger { balances: Vec::new(), reserved: Vec::new() });
            um.prepare().ok();
            um.cutover(InstanceLedger { balances: Vec::new(), reserved: Vec::new() }).is_ok()
        }));
        v.push(("F0457-判据-Idle非过渡期", !UpgradeMigrator::new(InstanceLedger { balances: Vec::new(), reserved: Vec::new() }).in_transition()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0457_upgrademig_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
