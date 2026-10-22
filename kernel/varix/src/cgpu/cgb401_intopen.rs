//! CGPU-F0401 · 集成组开工与预留清单核销（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 集成开工：预留清单盘点（C01-C05 全部"接口预留"逐项列出——F0332
//!   D 域/F0342 E 域/F0338 H 显存 I 带宽 J 功耗/F0346 K 异构/F0377 P 域/
//!   F0378 NPU/F0394 多用户）；本组核销计划（成熟一个核销一个——每项
//!   兑现后回写清单标记"已兑现+兑现批次"）；核销纪律（兑现=正式集成+
//!   回归矩阵+文档，不许只写代码不闭环）。
//! - 判据：清单盘点、核销计划、回写纪律、判据。
//!
//! 设计纪律：
//! - 预留清单是账不是备忘：每条预留（锚点号/对端域/预留语义/状态），
//!   状态机 Pending → InFlight → Redeemed，跃迁带证据（批次号）。
//! - 核销纪律机器化：兑现动作必须同时携带「集成件号+回归件号+文档件号」
//!   三证，缺一即拒——只写代码不闭环过不了核销闸。
//! - 清单盘点完整性：九条预留锚点（F0332/F0342/F0338/F0346/F0377/
//!   F0378/F0394 + H/I/J 三域细分）逐条在册，锚点重复即盘点红。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C40 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C40，全仓独占）。
pub const SEG_BASE: u16 = 0x0C40;

pub const E_RESV_DUP: u16 = SEG_BASE + 1; // 预留锚点重复
pub const E_RESV_STATE: u16 = SEG_BASE + 2; // 状态机非法跃迁
pub const E_RESV_EVID: u16 = SEG_BASE + 3; // 兑现三证缺一
pub const E_RESV_UNKNOWN: u16 = SEG_BASE + 4; // 锚点不在册
pub const E_PLAN_GAP: u16 = SEG_BASE + 5; // 核销计划批次缺口
pub const E_REDEEM_BATCH: u16 = SEG_BASE + 6; // 兑现批次号非法

/// 预留状态机：Pending（挂账）→ InFlight（本组在兑）→ Redeemed（已兑现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResvState {
    Pending,
    InFlight,
    Redeemed,
}

/// 状态机合法跃迁。
fn resv_legal(from: ResvState, to: ResvState) -> bool {
    matches!(
        (from, to),
        (ResvState::Pending, ResvState::InFlight)
            | (ResvState::InFlight, ResvState::Redeemed)
            | (ResvState::InFlight, ResvState::Pending) // 回滚（兑现失败退回挂账）
    )
}

/// 一条接口预留。
#[derive(Clone, Debug)]
pub struct Reservation {
    /// 预留锚点号（F0xxx）。
    pub anchor: u32,
    /// 对端域字母（D/E/H/I/J/K/P/N/U）。
    pub peer: u8,
    /// 预留语义（一句话）。
    pub semantic: String,
    pub state: ResvState,
    /// 兑现批次号（兑现后回写；0=未兑现）。
    pub redeemed_batch: u32,
    /// 兑现三证：集成件/回归件/文档件号。
    pub evid_integration: u32,
    pub evid_regress: u32,
    pub evid_doc: u32,
}

impl Reservation {
    pub fn pending(anchor: u32, peer: u8, semantic: &str) -> Self {
        Reservation {
            anchor,
            peer,
            semantic: semantic.to_string(),
            state: ResvState::Pending,
            redeemed_batch: 0,
            evid_integration: 0,
            evid_regress: 0,
            evid_doc: 0,
        }
    }

    pub fn is_redeemed(&self) -> bool {
        self.state == ResvState::Redeemed
    }
}

/// 预留清单（盘点 + 核销账）。
#[derive(Clone, Debug, Default)]
pub struct ResvLedger {
    pub items: Vec<Reservation>,
    /// 核销计划：批次号 → 本批计划核销的锚点集（扁平记录）。
    pub plan: Vec<(u32, u32)>,
}

/// 本组九条预留的标准盘点锚点。
pub const STANDARD_RESERVATIONS: [(u32, u8, &str); 9] = [
    (332, b'D', "D 域 80 帧计时反馈"),
    (342, b'E', "E 域表面可见性联动"),
    (338, b'H', "H 显存四维预算显存维"),
    (338, b'I', "I 带宽维度账户"),
    (338, b'J', "J 功耗标签位"),
    (346, b'K', "K 异构任务预算通道"),
    (377, b'P', "P 域预测预留接口"),
    (378, b'N', "NPU 预留接口"),
    (394, b'U', "多用户预留"),
];

impl ResvLedger {
    /// 盘点入册：锚点+对端唯一（同锚多对端合法——F0338 三细分）。
    pub fn stocktake(&mut self, anchor: u32, peer: u8, semantic: &str) -> Result<(), u16> {
        if self
            .items
            .iter()
            .any(|r| r.anchor == anchor && r.peer == peer)
        {
            return Err(E_RESV_DUP);
        }
        self.items.push(Reservation::pending(anchor, peer, semantic));
        Ok(())
    }

    /// 标准九条一键盘点。
    pub fn stocktake_standard(&mut self) -> Result<usize, u16> {
        let mut n = 0;
        for (a, p, s) in STANDARD_RESERVATIONS {
            self.stocktake(a, p, s)?;
            n += 1;
        }
        Ok(n)
    }

    /// 盘点完整性：标准九条逐一在册。
    pub fn stocktake_complete(&self) -> bool {
        STANDARD_RESERVATIONS.iter().all(|(a, p, _)| {
            self.items
                .iter()
                .any(|r| r.anchor == *a && r.peer == *p)
        })
    }

    /// 核销计划登记：批次号合法（≥1）且锚点在册。
    pub fn plan_batch(&mut self, batch: u32, anchor: u32) -> Result<(), u16> {
        if batch == 0 {
            return Err(E_REDEEM_BATCH);
        }
        if !self.items.iter().any(|r| r.anchor == anchor) {
            return Err(E_RESV_UNKNOWN);
        }
        self.plan.push((batch, anchor));
        Ok(())
    }

    /// 计划完整性：每条在册预留至少进一个批次。
    pub fn plan_covers_all(&self) -> Result<(), u16> {
        for r in &self.items {
            if !self.plan.iter().any(|(_, a)| *a == r.anchor) {
                return Err(E_PLAN_GAP);
            }
        }
        Ok(())
    }

    /// 开工：预留进入 InFlight。
    pub fn begin(&mut self, anchor: u32, peer: u8) -> Result<(), u16> {
        let it = self
            .items
            .iter_mut()
            .find(|r| r.anchor == anchor && r.peer == peer)
            .ok_or(E_RESV_UNKNOWN)?;
        if !resv_legal(it.state, ResvState::InFlight) {
            return Err(E_RESV_STATE);
        }
        it.state = ResvState::InFlight;
        Ok(())
    }

    /// 核销：InFlight → Redeemed，三证齐 + 批次号合法才放行。
    pub fn redeem(
        &mut self,
        anchor: u32,
        peer: u8,
        batch: u32,
        evid_integration: u32,
        evid_regress: u32,
        evid_doc: u32,
    ) -> Result<(), u16> {
        if batch == 0 {
            return Err(E_REDEEM_BATCH);
        }
        let it = self
            .items
            .iter_mut()
            .find(|r| r.anchor == anchor && r.peer == peer)
            .ok_or(E_RESV_UNKNOWN)?;
        if !resv_legal(it.state, ResvState::Redeemed) {
            return Err(E_RESV_STATE);
        }
        if evid_integration == 0 || evid_regress == 0 || evid_doc == 0 {
            return Err(E_RESV_EVID); // 只写代码不闭环 ⇒ 拒
        }
        it.state = ResvState::Redeemed;
        it.redeemed_batch = batch;
        it.evid_integration = evid_integration;
        it.evid_regress = evid_regress;
        it.evid_doc = evid_doc;
        Ok(())
    }

    /// 兑现失败回滚：InFlight → Pending（回到挂账池下批再试）。
    pub fn rollback(&mut self, anchor: u32, peer: u8) -> Result<(), u16> {
        let it = self
            .items
            .iter_mut()
            .find(|r| r.anchor == anchor && r.peer == peer)
            .ok_or(E_RESV_UNKNOWN)?;
        if !resv_legal(it.state, ResvState::Pending) {
            return Err(E_RESV_STATE);
        }
        it.state = ResvState::Pending;
        Ok(())
    }

    /// 已兑现回写视图：「锚点+对端 → 批次」名单（清单回写纪律的产物）。
    pub fn redeemed_view(&self) -> Vec<(u32, u8, u32)> {
        self.items
            .iter()
            .filter(|r| r.is_redeemed())
            .map(|r| (r.anchor, r.peer, r.redeemed_batch))
            .collect()
    }

    /// 核销进度：已兑/总数。
    pub fn progress(&self) -> (usize, usize) {
        (self.items.iter().filter(|r| r.is_redeemed()).count(), self.items.len())
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_清单盘点_标准九条齐且防重() {
        let mut l = ResvLedger::default();
        assert_eq!(l.stocktake_standard(), Ok(9));
        assert!(l.stocktake_complete());
        // 同锚同对端重复盘点红。
        assert_eq!(l.stocktake(332, b'D', "重复"), Err(E_RESV_DUP));
        // 同锚不同对端合法（F0338 三细分已入册）。
        assert!(l.items.iter().filter(|r| r.anchor == 338).count() >= 3);
        // 状态初值全挂账。
        assert!(l.items.iter().all(|r| r.state == ResvState::Pending));
    }

    #[test]
    fn a02_核销计划_批覆盖全部预留() {
        let mut l = ResvLedger::default();
        l.stocktake_standard().ok();
        // 批次 1：D/E。
        for a in [332u32, 342] {
            l.plan_batch(1, a).ok();
        }
        // 尚有锚点未进计划 ⇒ 缺口红。
        assert_eq!(l.plan_covers_all(), Err(E_PLAN_GAP));
        // 补齐其余锚点（F038 同锚一条即覆盖三细分）⇒ 全绿。
        for a in [338u32, 346, 377, 378, 394] {
            l.plan_batch(3, a).ok();
        }
        assert_eq!(l.plan_covers_all(), Ok(()));
        // 非法批次号。
        assert_eq!(l.plan_batch(0, 332), Err(E_REDEEM_BATCH));
        // 计划引用不在册锚点。
        assert_eq!(l.plan_batch(4, 9999), Err(E_RESV_UNKNOWN));
    }

    #[test]
    fn a03_回写纪律_三证缺一即拒() {
        let mut l = ResvLedger::default();
        l.stocktake_standard().ok();
        l.begin(332, b'D').ok();
        // 只写代码（缺回归+文档证）⇒ 拒。
        assert_eq!(
            l.redeem(332, b'D', 1, 101, 0, 0),
            Err(E_RESV_EVID)
        );
        // 三证齐 ⇒ 放行并回写批次。
        assert_eq!(l.redeem(332, b'D', 1, 101, 202, 303), Ok(()));
        let view = l.redeemed_view();
        assert_eq!(view, vec![(332, b'D', 1)]);
        // 批次号 0 拒。
        l.begin(342, b'E').ok();
        assert_eq!(
            l.redeem(342, b'E', 0, 1, 1, 1),
            Err(E_REDEEM_BATCH)
        );
        // 在册外锚点拒。
        assert_eq!(l.redeem(9999, b'X', 1, 1, 1, 1), Err(E_RESV_UNKNOWN));
    }

    #[test]
    fn a04_状态机_Pending到InFlight到Redeemed及回滚() {
        let mut l = ResvLedger::default();
        l.stocktake(332, b'D', "D 域").ok();
        // Pending 直接 Redeemed ⇒ 跳级拒。
        assert_eq!(l.redeem(332, b'D', 1, 1, 1, 1), Err(E_RESV_STATE));
        l.begin(332, b'D').ok();
        // InFlight 重复开工拒。
        assert_eq!(l.begin(332, b'D'), Err(E_RESV_STATE));
        // 兑现失败回滚 → Pending 再开工。
        assert_eq!(l.rollback(332, b'D'), Ok(()));
        assert_eq!(l.rollback(332, b'D'), Err(E_RESV_STATE)); // 双回滚拒
        l.begin(332, b'D').ok();
        assert_eq!(l.redeem(332, b'D', 1, 1, 1, 1), Ok(()));
        // Redeemed 再回滚拒（已兑现不可逆）。
        assert_eq!(l.rollback(332, b'D'), Err(E_RESV_STATE));
        assert_eq!(l.progress(), (1, 1));
    }

    #[test]
    fn a05_进度视图_多批核销统计() {
        let mut l = ResvLedger::default();
        l.stocktake_standard().ok();
        // 批 1 兑三条。
        for (a, p) in [(332u32, b'D'), (342, b'E'), (338, b'H')] {
            l.begin(a, p).ok();
            l.redeem(a, p, 1, 1, 1, 1).ok();
        }
        assert_eq!(l.progress(), (3, 9));
        // 未兑明细不进视图。
        let view = l.redeemed_view();
        assert_eq!(view.len(), 3);
        assert!(view.iter().all(|(_, _, b)| *b == 1));
    }
}
