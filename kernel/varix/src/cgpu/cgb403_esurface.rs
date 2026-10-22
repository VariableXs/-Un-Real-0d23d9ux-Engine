//! CGPU-F0403 · 与 E 域表面配额正式集成（目标 380 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - E 域集成：表面可见性联动（F0342/F0358 预留兑现——表面可见性变化→
//!   预算权重实时调整 <1 帧）；30 网页配额执行（E 域配额决策→本域账户
//!   额度执行的完整链路）；滚动提权协议（可见滚动表面临时提权的正式
//!   协议——提权幅度/时长/回退）；冲突对账（E 域配额与 C 域账户的定期
//!   一致性对账）。
//! - 判据：实时联动、完整链路、提权协议、对账、判据。
//!
//! 设计纪律：
//! - 实时联动 <1 帧：可见性事件同步折算权重（同帧生效——无排队），
//!   提权/降权都以「帧序号」记生效点，回查可复算。
//! - 30 网页两层分配链路：E 域配额决策（两层比例）→ 本域账户额度
//!   生成 → 执行反馈 → 差额回写。链路四步缺一即不闭环。
//! - 滚动提权协议三要素强制：幅度（上限封顶）/时长（到期自动回退）/
//!   回退（提前撤销或到期回退都要留痕）。同时最多一个活跃提权。
//! - 冲突对账：E 侧配额与 C 侧账户额度逐项比对，差异 > 容差即对账红，
//!   对账差异计数入遥测。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C60 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C60，全仓独占）。
pub const SEG_BASE: u16 = 0x0C60;

pub const E_EVIS_WEIGHT: u16 = SEG_BASE + 1; // 权重和越界
pub const E_ELINK_STATE: u16 = SEG_BASE + 2; // 链路状态机非法
pub const E_ELEV_CAP: u16 = SEG_BASE + 3; // 提权幅度超封顶
pub const E_ELEV_ACTIVE: u16 = SEG_BASE + 4; // 已有活跃提权
pub const E_ERECON_DIFF: u16 = SEG_BASE + 5; // 对账差异超容差
pub const E_EPAGE_COUNT: u16 = SEG_BASE + 6; // 网页数非 30

/// 30 网页两层分配的网页数。
pub const WEB_PAGES: usize = 30;
/// 提权幅度封顶（基线的 50%——协议上限）。
pub const ELEVATE_CAP_PM: u32 = 500;
/// 默认提权时长（帧）。
pub const ELEVATE_SPAN_FRAMES: u32 = 240;
/// 对账容差（额度千分位）。
pub const RECON_TOLERANCE_PM: u32 = 20;

/// 可见性事件：表面 → 可见/不可见（帧序号携带——同帧生效）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisEvent {
    pub surface: u32,
    pub visible: bool,
    pub frame: u64,
}

/// 权重联动表：表面 → 权重‰。
#[derive(Clone, Debug, Default)]
pub struct VisWeights {
    pub w: Vec<(u32, u32)>,
}

impl VisWeights {
    /// 事件同步折算：可见 +x‰（封顶单表和 ≤1000），不可见 →0。
    /// 返回生效帧（=事件帧，<1 帧的形式化）。
    pub fn apply_event(&mut self, e: VisEvent, gain_pm: u32) -> Result<u64, u16> {
        let exists = self.w.iter().any(|(s, _)| *s == e.surface);
        if exists {
            // 先算其余和（不可变借），再定点改（可变借）——借用不交叠。
            let others: u32 = self
                .w
                .iter()
                .filter(|(s, _)| *s != e.surface)
                .map(|(_, x)| x)
                .sum();
            let entry = self
                .w
                .iter_mut()
                .find(|(s, _)| *s == e.surface)
                .ok_or(E_EVIS_WEIGHT)?;
            if e.visible {
                let nw = entry.1 + gain_pm;
                entry.1 = if nw + others > 1000 { 1000 - others } else { nw };
            } else {
                entry.1 = 0;
            }
        } else {
            let others: u32 = self.w.iter().map(|(_, x)| x).sum();
            if e.visible {
                let nw = gain_pm.min(1000 - others);
                self.w.push((e.surface, nw));
            } else {
                self.w.push((e.surface, 0));
            }
        }
        Ok(e.frame)
    }

    pub fn weight_sum(&self) -> u32 {
        self.w.iter().map(|(_, x)| x).sum()
    }

    pub fn weight_of(&self, surface: u32) -> u32 {
        self.w
            .iter()
            .find(|(s, _)| *s == surface)
            .map(|(_, x)| *x)
            .unwrap_or(0)
    }
}

/// 30 网页两层分配链路状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkPhase {
    /// E 域配额决策已到。
    QuotaIn,
    /// 本域账户额度已生成。
    QuotaApplied,
    /// 执行反馈已收。
    Executed,
    /// 差额已回写 E 域（链路闭环）。
    Written,
}

/// 网页两层分配决策（E 域→本域的输入）。
#[derive(Clone, Debug)]
pub struct TwoLayerQuota {
    /// 第一层（前 N 个活跃页）总额度‰。
    pub layer1_pm: u32,
    /// 第二层（其余页）= 1000 − layer1。
    pub layer1_count: usize,
}

impl TwoLayerQuota {
    /// 两层合计恒 1000。
    pub fn layer2_pm(&self) -> u32 {
        1000 - self.layer1_pm
    }

    /// 每页额度（千分位）：第一层均分、第二层均分。
    pub fn per_page_pm(&self) -> Result<Vec<u32>, u16> {
        if self.layer1_count == 0 || self.layer1_count >= WEB_PAGES {
            return Err(E_EPAGE_COUNT);
        }
        let mut v = Vec::new();
        let l1 = self.layer1_pm / self.layer1_count as u32;
        let l2 = self.layer2_pm() / (WEB_PAGES - self.layer1_count) as u32;
        for i in 0..WEB_PAGES {
            v.push(if i < self.layer1_count { l1 } else { l2 });
        }
        Ok(v)
    }
}

/// 链路执行器：决策→额度→执行→回写四步。
#[derive(Clone, Debug)]
pub struct QuotaLink {
    pub phase: LinkPhase,
    pub quota: Option<TwoLayerQuota>,
    pub quotas_pm: Vec<u32>,
    pub executed: Vec<u32>,
    pub written_delta: i32,
}

impl QuotaLink {
    pub fn new() -> Self {
        QuotaLink {
            phase: LinkPhase::QuotaIn,
            quota: None,
            quotas_pm: Vec::new(),
            executed: Vec::new(),
            written_delta: 0,
        }
    }

    /// 第一步：E 域决策入链。
    pub fn ingest(&mut self, q: TwoLayerQuota) -> Result<(), u16> {
        if self.phase != LinkPhase::Written && self.phase != LinkPhase::QuotaIn {
            return Err(E_ELINK_STATE);
        }
        if q.layer1_pm > 1000 || q.layer1_count == 0 || q.layer1_count >= WEB_PAGES {
            return Err(E_EPAGE_COUNT);
        }
        self.quota = Some(q);
        self.phase = LinkPhase::QuotaIn;
        Ok(())
    }

    /// 第二步：生成本域账户额度。
    pub fn apply(&mut self) -> Result<(), u16> {
        if self.phase != LinkPhase::QuotaIn {
            return Err(E_ELINK_STATE);
        }
        let q = self.quota.as_ref().ok_or(E_ELINK_STATE)?;
        self.quotas_pm = q.per_page_pm()?;
        self.phase = LinkPhase::QuotaApplied;
        Ok(())
    }

    /// 第三步：执行反馈（每页实际消耗‰）。
    pub fn feedback(&mut self, used: Vec<u32>) -> Result<(), u16> {
        if self.phase != LinkPhase::QuotaApplied || used.len() != WEB_PAGES {
            return Err(E_ELINK_STATE);
        }
        self.executed = used;
        self.phase = LinkPhase::Executed;
        Ok(())
    }

    /// 第四步：差额回写（执行和 − 额度和）。
    pub fn writeback(&mut self) -> Result<i32, u16> {
        if self.phase != LinkPhase::Executed {
            return Err(E_ELINK_STATE);
        }
        let q: i64 = self.quotas_pm.iter().map(|v| *v as i64).sum();
        let u: i64 = self.executed.iter().map(|v| *v as i64).sum();
        self.written_delta = (u - q) as i32;
        self.phase = LinkPhase::Written;
        Ok(self.written_delta)
    }
}

/// 滚动提权协议账。
#[derive(Clone, Debug)]
pub struct Elevation {
    pub surface: u32,
    /// 提权幅度‰（≤ 封顶）。
    pub gain_pm: u32,
    /// 生效帧与到期帧。
    pub from_frame: u64,
    pub until_frame: u64,
    pub active: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ElevationBook {
    pub current: Option<Elevation>,
}

impl ElevationBook {
    /// 提权申请：幅度封顶 + 单活跃限制 + 时长强制。
    pub fn request(&mut self, surface: u32, gain_pm: u32, from: u64) -> Result<(), u16> {
        if gain_pm == 0 || gain_pm > ELEVATE_CAP_PM {
            return Err(E_ELEV_CAP);
        }
        if self.current.as_ref().map(|e| e.active).unwrap_or(false) {
            return Err(E_ELEV_ACTIVE);
        }
        self.current = Some(Elevation {
            surface,
            gain_pm,
            from_frame: from,
            until_frame: from + ELEVATE_SPAN_FRAMES as u64,
            active: true,
        });
        Ok(())
    }

    /// 到期自动回退：过期即失活（回退帧 = until_frame）。
    pub fn tick(&mut self, frame: u64) -> Option<u64> {
        let mut fell = None;
        if let Some(e) = self.current.as_mut() {
            if e.active && frame >= e.until_frame {
                e.active = false;
                fell = Some(e.until_frame);
            }
        }
        fell
    }

    /// 提前撤销：回退即时生效（留痕 = 撤销帧）。
    pub fn revoke(&mut self, frame: u64) -> Result<u64, u16> {
        match self.current.as_mut() {
            Some(e) if e.active => {
                e.active = false;
                Ok(frame)
            }
            _ => Err(E_ELEV_ACTIVE),
        }
    }

    pub fn is_active(&self) -> bool {
        self.current.as_ref().map(|e| e.active).unwrap_or(false)
    }
}

/// 冲突对账：E 侧配额 vs C 侧账户额度逐项比。
#[derive(Clone, Debug, Default)]
pub struct Reconciler {
    pub diffs: u32,
    pub last_bad: Vec<u32>,
}

impl Reconciler {
    /// 对账：逐项差异 ≤ 容差（基线额度的容差‰）才绿。
    pub fn reconcile(&mut self, e_side: &[(u32, u32)], c_side: &[(u32, u32)]) -> Result<(), u16> {
        self.last_bad.clear();
        for (acct, ev) in e_side {
            let cv = c_side
                .iter()
                .find(|(a, _)| a == acct)
                .map(|(_, v)| *v)
                .unwrap_or(0);
            let base = (*ev).max(1) as u64;
            let diff: u64 = if cv > *ev {
                (cv - *ev) as u64
            } else {
                (*ev - cv) as u64
            };
            if diff * 1000 > base * RECON_TOLERANCE_PM as u64 {
                self.last_bad.push(*acct);
            }
        }
        if !self.last_bad.is_empty() {
            self.diffs += 1;
            return Err(E_ERECON_DIFF);
        }
        Ok(())
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_实时联动_可见性同帧折权重() {
        let mut vw = VisWeights::default();
        // 首个表面可见 → 生效帧 = 事件帧（<1 帧形式化）。
        assert_eq!(
            vw.apply_event(VisEvent { surface: 7, visible: true, frame: 100 }, 300),
            Ok(100)
        );
        assert_eq!(vw.weight_of(7), 300);
        // 再来一个，总和钳 1000。
        vw.apply_event(VisEvent { surface: 8, visible: true, frame: 101 }, 900)
            .ok();
        assert_eq!(vw.weight_sum(), 1000);
        assert_eq!(vw.weight_of(8), 700);
        // 不可见 → 0，权重让出。
        vw.apply_event(VisEvent { surface: 7, visible: false, frame: 102 }, 0)
            .ok();
        assert_eq!(vw.weight_of(7), 0);
        assert_eq!(vw.weight_sum(), 700);
        // 未注册表面不可见也入册（0 权重）。
        vw.apply_event(VisEvent { surface: 9, visible: false, frame: 103 }, 0)
            .ok();
        assert_eq!(vw.weight_of(9), 0);
    }

    #[test]
    fn a02_完整链路_四步闭环与差额回写() {
        let mut link = QuotaLink::new();
        let q = TwoLayerQuota { layer1_pm: 600, layer1_count: 10 };
        link.ingest(q).ok();
        link.apply().ok();
        assert_eq!(link.quotas_pm.len(), WEB_PAGES);
        // 第一层每页 60‰、第二层每页 20‰（400‰/20 页）。
        assert_eq!(link.quotas_pm[0], 60);
        assert_eq!(link.quotas_pm[29], 20);
        // 执行恰好等于额度 ⇒ 差额 0 闭环。
        let used = link.quotas_pm.clone();
        link.feedback(used).ok();
        assert_eq!(link.writeback(), Ok(0));
        assert_eq!(link.phase, LinkPhase::Written);
        // 状态机跳步拒。
        let mut l2 = QuotaLink::new();
        assert_eq!(l2.apply(), Err(E_ELINK_STATE));
        // 超额执行 ⇒ 正差额回写。
        let mut l3 = QuotaLink::new();
        l3.ingest(TwoLayerQuota { layer1_pm: 600, layer1_count: 10 }).ok();
        l3.apply().ok();
        let mut over = l3.quotas_pm.clone();
        over[0] += 20;
        l3.feedback(over).ok();
        assert_eq!(l3.writeback(), Ok(20));
    }

    #[test]
    fn a03_提权协议_幅度封顶时长到期回退() {
        let mut b = ElevationBook::default();
        // 超封顶拒。
        assert_eq!(b.request(7, ELEVATE_CAP_PM + 1, 100), Err(E_ELEV_CAP));
        assert_eq!(b.request(7, 0, 100), Err(E_ELEV_CAP));
        // 正常提权：幅度/时长三要素齐。
        assert_eq!(b.request(7, 300, 100), Ok(()));
        assert!(b.is_active());
        let e = b.current.clone();
        assert_eq!(e.map(|x| (x.gain_pm, x.until_frame)), Some((300, 100 + ELEVATE_SPAN_FRAMES as u64)));
        // 单活跃限制。
        assert_eq!(b.request(8, 200, 101), Err(E_ELEV_ACTIVE));
        // 到期自动回退。
        assert_eq!(b.tick(99), None);
        assert_eq!(b.tick(100 + ELEVATE_SPAN_FRAMES as u64), Some(100 + ELEVATE_SPAN_FRAMES as u64));
        assert!(!b.is_active());
        // 失活后可再提权；提前撤销留痕。
        b.request(8, 200, 500).ok();
        assert_eq!(b.revoke(520), Ok(520));
        assert!(!b.is_active());
        assert_eq!(b.revoke(521), Err(E_ELEV_ACTIVE));
    }

    #[test]
    fn a04_对账_容差内绿差异计数() {
        let mut r = Reconciler::default();
        let e_side = vec![(1u32, 500u32), (2, 300)];
        // 完全一致绿。
        let c_same = vec![(1u32, 500u32), (2, 300)];
        assert_eq!(r.reconcile(&e_side, &c_same), Ok(()));
        // 差 1%（20‰ 容差内）绿。
        let c_close = vec![(1u32, 495u32), (2, 305)];
        assert_eq!(r.reconcile(&e_side, &c_close), Ok(()));
        // 差 5% 越容差红且点名。
        let c_far = vec![(1u32, 470u32), (2, 300)];
        assert_eq!(r.reconcile(&e_side, &c_far), Err(E_ERECON_DIFF));
        assert_eq!(r.last_bad, vec![1]);
        assert_eq!(r.diffs, 1);
        // C 侧缺项按 0 差额处理（缺账户必红）。
        let c_missing = vec![(1u32, 500u32)];
        assert_eq!(r.reconcile(&e_side, &c_missing), Err(E_ERECON_DIFF));
        assert_eq!(r.last_bad, vec![2]);
        assert_eq!(r.diffs, 2);
    }

    #[test]
    fn a05_两层分配_页数与额度守恒() {
        // 层一 10 页拿 600‰、层二 20 页拿 400‰。
        let q = TwoLayerQuota { layer1_pm: 600, layer1_count: 10 };
        let per = q.per_page_pm().unwrap();
        let l1_sum: u32 = per[..10].iter().sum();
        let l2_sum: u32 = per[10..].iter().sum();
        assert_eq!(l1_sum, 600);
        assert_eq!(l2_sum, 400);
        assert_eq!(per.len(), WEB_PAGES);
        // 非法页数拒。
        assert_eq!(
            TwoLayerQuota { layer1_pm: 600, layer1_count: 0 }.per_page_pm(),
            Err(E_EPAGE_COUNT)
        );
        assert_eq!(
            TwoLayerQuota { layer1_pm: 600, layer1_count: 30 }.per_page_pm(),
            Err(E_EPAGE_COUNT)
        );
    }
}
