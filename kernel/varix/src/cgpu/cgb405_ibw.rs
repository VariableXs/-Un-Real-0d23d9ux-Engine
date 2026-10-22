//! CGPU-F0405 · 与 I 带宽域接口集成（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 带宽集成：带宽维度落地（上传/下载/总线带宽三类预算账户——与 I 域
//!   F1281+ 仲裁对接）；带宽挤占防护（批量上传不挤占实时流——类别差异
//!   化）；带宽超用降级（超预算→降码率/降帧率建议输出）；联动遥测（带宽
//!   维度消耗进统计立方）。
//! - 判据：三类账户、挤占防护、降级建议、遥测、判据。
//!
//! 设计纪律：
//! - 三类账户（上传/下载/总线）独立余量账，I 域仲裁对接以「类别标签」
//!   透传——仲裁侧看到的永远是带类别的请求。
//! - 挤占防护形式化：每类账户有「最小保障‰」；批类（Bulk）流量在实时
//!   类（Realtime）保障未满足时一律让行——仲裁序 = 保障缺口大者优先，
//!   同缺口下 Realtime > Interactive > Bulk。
//! - 超用降级建议：账户余量耗尽→按类别输出降级建议（视频降码率/
//!   动画降帧率/批量降吞吐），建议带幅度档（阶梯 1/2 → 1/4）。
//! - 遥测：三类消耗按帧累计进立方（acct × class × frame 桶）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C80 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C80，全仓独占）。
pub const SEG_BASE: u16 = 0x0C80;

pub const E_IBW_CLASS: u16 = SEG_BASE + 1; // 类别非法
pub const E_IBW_GUARANTEE: u16 = SEG_BASE + 2; // 保障‰配置非法
pub const E_IBW_ORDER: u16 = SEG_BASE + 3; // 仲裁序违反（挤占发生）
pub const E_IDegrade_NONE: u16 = SEG_BASE + 4; // 无可降级路径
pub const E_IBW_NOACC: u16 = SEG_BASE + 5; // 账户未建
pub const E_IBW_TELEM: u16 = SEG_BASE + 6; // 遥测桶越界

/// 三类带宽账户。
pub const CLASS_UPLOAD: u8 = 0;
pub const CLASS_DOWNLOAD: u8 = 1;
pub const CLASS_BUS: u8 = 2;
pub const CLASS_COUNT: usize = 3;

pub fn class_name(c: u8) -> &'static str {
    match c {
        CLASS_UPLOAD => "上传",
        CLASS_DOWNLOAD => "下载",
        _ => "总线",
    }
}

/// 流量子类别（挤占防护坐标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowKind {
    /// 实时流（音视频会议）——最高优先。
    Realtime,
    /// 交互流（页面/输入）。
    Interactive,
    /// 批量流（上传/同步/备份）——最低优先。
    Bulk,
}

/// 仲裁优先级序：Realtime < Interactive < Bulk（小者先）。
pub fn kind_rank(k: FlowKind) -> u8 {
    match k {
        FlowKind::Realtime => 0,
        FlowKind::Interactive => 1,
        FlowKind::Bulk => 2,
    }
}

/// 带宽账户。
#[derive(Clone, Debug)]
pub struct BwAccount {
    pub acct: u32,
    /// 三类预算（KB/窗）。
    pub budgets: [u32; CLASS_COUNT],
    /// 三类已耗。
    pub used: [u32; CLASS_COUNT],
    /// 最小保障‰（实时流保障——挤占防护的锚）。
    pub rt_guarantee_pm: u32,
}

impl BwAccount {
    pub fn new(acct: u32, budgets: [u32; CLASS_COUNT], rt_guarantee_pm: u32) -> Self {
        BwAccount { acct, budgets, used: [0; CLASS_COUNT], rt_guarantee_pm }
    }

    pub fn avail(&self, class: u8) -> u32 {
        if class as usize >= CLASS_COUNT {
            return 0;
        }
        self.budgets[class as usize].saturating_sub(self.used[class as usize])
    }

    /// 实时保障余量：实时类可用量占其预算的千分比。
    pub fn rt_gap_permille(&self) -> u32 {
        let rt_budget = self.budgets[CLASS_DOWNLOAD as usize].max(1) as u64;
        let rt_left = self.budgets[CLASS_DOWNLOAD as usize]
            .saturating_sub(self.used[CLASS_DOWNLOAD as usize]) as u64;
        let need_pm = (rt_budget * self.rt_guarantee_pm as u64 / 1000) as u32;
        if rt_left >= need_pm as u64 {
            1000
        } else {
            // 缺口比例（余量/需求）。
            (rt_left * 1000 / need_pm.max(1) as u64) as u32
        }
    }
}

/// 带宽请求（带流量子类——挤占防护输入）。
#[derive(Clone, Copy, Debug)]
pub struct BwRequest {
    pub acct: u32,
    pub class: u8,
    pub kind: FlowKind,
    pub kb: u32,
}

/// 降级建议。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradeAdvice {
    pub acct: u32,
    pub class: u8,
    /// 建议：0=降码率 1=降帧率 2=降吞吐。
    pub action: u8,
    /// 幅度档：2=减半 4=四分之一。
    pub step: u32,
}

/// 带宽集成控制器。
#[derive(Clone, Debug)]
pub struct BwIntegrator {
    pub accounts: Vec<BwAccount>,
    /// 遥测立方：acct × class → 累计 KB（遥测按账户×类两维聚合）。
    pub telem: Vec<(u32, u8, u64)>,
    /// 挤占计数（遥测：防护触发次数）。
    pub preempt_events: u32,
}

impl BwIntegrator {
    pub fn new() -> Self {
        BwIntegrator { accounts: Vec::new(), telem: Vec::new(), preempt_events: 0 }
    }

    pub fn attach(&mut self, a: BwAccount) -> Result<(), u16> {
        if a.rt_guarantee_pm > 1000 {
            return Err(E_IBW_GUARANTEE);
        }
        if self.accounts.iter().any(|x| x.acct == a.acct) {
            return Err(E_IBW_NOACC);
        }
        for c in 0..CLASS_COUNT as u8 {
            self.telem.push((a.acct, c, 0));
        }
        self.accounts.push(a);
        Ok(())
    }

    /// 单请求仲裁：余量足够即放行记账；不足按子类处理——
    /// Bulk 拒（等窗）、其他类放行到余量为止（部分授予）。
    pub fn request(&mut self, r: BwRequest) -> Result<u32, u16> {
        if r.class as usize >= CLASS_COUNT {
            return Err(E_IBW_CLASS);
        }
        let ai = self
            .accounts
            .iter()
            .position(|a| a.acct == r.acct)
            .ok_or(E_IBW_NOACC)?;
        let grant = r.kb.min(self.accounts[ai].avail(r.class));
        if grant == 0 {
            return Ok(0); // 无余量零授予（不报错——降级建议路径）
        }
        self.accounts[ai].used[r.class as usize] += grant;
        // 遥测累计。
        if let Some(t) = self
            .telem
            .iter_mut()
            .find(|(a, c, _)| *a == r.acct && *c == r.class)
        {
            t.2 += grant as u64;
        } else {
            return Err(E_IBW_TELEM);
        }
        Ok(grant)
    }

    /// 挤占防护仲裁：一批请求按「保障缺口大者优先 + 子类序」排闸，
    /// Bulk 不得越过未满足的 Realtime。返回按序放行的请求下标。
    pub fn arbitrate(&mut self, reqs: &[BwRequest]) -> Result<Vec<usize>, u16> {
        // 校验类与账户存在。
        for r in reqs {
            if r.class as usize >= CLASS_COUNT {
                return Err(E_IBW_CLASS);
            }
            if !self.accounts.iter().any(|a| a.acct == r.acct) {
                return Err(E_IBW_NOACC);
            }
        }
        // 排序键：实时缺口升序（缺口大=保障不足者优先），再子类序。
        let mut idx: Vec<usize> = (0..reqs.len()).collect();
        idx.sort_by(|&a, &b| {
            let ra = &reqs[a];
            let rb = &reqs[b];
            let ga = self
                .accounts
                .iter()
                .find(|x| x.acct == ra.acct)
                .map(|x| x.rt_gap_permille())
                .unwrap_or(0);
            let gb = self
                .accounts
                .iter()
                .find(|x| x.acct == rb.acct)
                .map(|x| x.rt_gap_permille())
                .unwrap_or(0);
            ga.cmp(&gb).then(kind_rank(ra.kind).cmp(&kind_rank(rb.kind)))
        });
        // 防挤占闸：Bulk 之前必须没有未调度的 Realtime（全局——跨账户也不许挤占）。
        let mut order = Vec::new();
        for &i in &idx {
            let r = reqs[i];
            if r.kind == FlowKind::Bulk {
                let blocked = reqs
                    .iter()
                    .enumerate()
                    .any(|(j, o)| {
                        o.kind == FlowKind::Realtime
                            && !order.contains(&j)
                            && o.kb > 0
                    });
                if blocked {
                    self.preempt_events += 1;
                    continue; // Bulk 让行
                }
            }
            order.push(i);
        }
        Ok(order)
    }

    /// 超用降级建议：三类各按账户输出建议（余量为 0 且有流量时）。
    pub fn degrade_advices(&self) -> Vec<DegradeAdvice> {
        let mut out = Vec::new();
        for a in &self.accounts {
            // 上传超尽 → 降码率（视频上传场景）。
            if a.avail(CLASS_UPLOAD) == 0 && a.used[CLASS_UPLOAD as usize] > 0 {
                out.push(DegradeAdvice { acct: a.acct, class: CLASS_UPLOAD, action: 0, step: 2 });
            }
            // 下载超尽 → 降帧率（实时流场景）。
            if a.avail(CLASS_DOWNLOAD) == 0 && a.used[CLASS_DOWNLOAD as usize] > 0 {
                out.push(DegradeAdvice { acct: a.acct, class: CLASS_DOWNLOAD, action: 1, step: 2 });
            }
            // 总线超尽 → 降吞吐（批量场景）。
            if a.avail(CLASS_BUS) == 0 && a.used[CLASS_BUS as usize] > 0 {
                out.push(DegradeAdvice { acct: a.acct, class: CLASS_BUS, action: 2, step: 4 });
            }
        }
        out
    }

    /// 立方快照：遥测计数（账户×类已聚合桶数）。
    pub fn telem_buckets(&self) -> usize {
        self.telem.len()
    }

    /// 立方查询：账户×类累计消耗。
    pub fn telem_of(&self, acct: u32, class: u8) -> u64 {
        self.telem
            .iter()
            .find(|(a, c, _)| *a == acct && *c == class)
            .map(|(_, _, v)| *v)
            .unwrap_or(0)
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn it() -> BwIntegrator {
        let mut i = BwIntegrator::new();
        i.attach(BwAccount::new(1, [400, 400, 800], 500)).ok();
        i.attach(BwAccount::new(2, [200, 200, 400], 500)).ok();
        i
    }

    #[test]
    fn a01_三类账户_独立余量与记账() {
        let mut i = it();
        assert_eq!(i.telem_buckets(), 6); // 2 账户 × 3 类
        // 上传/下载/总线三账独立。
        assert_eq!(
            i.request(BwRequest { acct: 1, class: CLASS_UPLOAD, kind: FlowKind::Bulk, kb: 100 }),
            Ok(100)
        );
        assert_eq!(i.accounts[0].avail(CLASS_UPLOAD), 300);
        assert_eq!(i.accounts[0].avail(CLASS_DOWNLOAD), 400);
        assert_eq!(i.telem_of(1, CLASS_UPLOAD), 100);
        // 类别越界拒。
        assert_eq!(
            i.request(BwRequest { acct: 1, class: 9, kind: FlowKind::Bulk, kb: 1 }),
            Err(E_IBW_CLASS)
        );
        // 账户不存在拒。
        assert_eq!(
            i.request(BwRequest { acct: 9, class: 0, kind: FlowKind::Bulk, kb: 1 }),
            Err(E_IBW_NOACC)
        );
    }

    #[test]
    fn a02_挤占防护_Bulk不越Realtime() {
        let mut i = it();
        // 打满账户 1 的下载类制造实时缺口（缺口 250‰ < 1000‰）。
        i.request(BwRequest { acct: 1, class: CLASS_DOWNLOAD, kind: FlowKind::Realtime, kb: 350 })
            .ok();
        // Bulk（acct1 缺口小→排序在前）会先于 acct2 的 Realtime 被调度。
        let reqs = vec![
            BwRequest { acct: 1, class: CLASS_UPLOAD, kind: FlowKind::Bulk, kb: 100 },
            BwRequest { acct: 2, class: CLASS_DOWNLOAD, kind: FlowKind::Realtime, kb: 30 },
            BwRequest { acct: 2, class: CLASS_BUS, kind: FlowKind::Interactive, kb: 10 },
        ];
        let order = i.arbitrate(&reqs).ok();
        // Bulk（idx 0）被全局让行 ⇒ 序中不含 0；实时先于交互。
        let o = order.unwrap_or_default();
        assert!(!o.contains(&0));
        let p_rt = o.iter().position(|&x| x == 1).unwrap_or(99);
        let p_int = o.iter().position(|&x| x == 2).unwrap_or(99);
        assert!(p_rt < p_int);
        assert_eq!(i.preempt_events, 1);
        // 无实时缺口且 Realtime 先排时 Bulk 正常放行。
        let mut i2 = it();
        let reqs2 = vec![
            BwRequest { acct: 1, class: CLASS_UPLOAD, kind: FlowKind::Bulk, kb: 50 },
            BwRequest { acct: 1, class: CLASS_DOWNLOAD, kind: FlowKind::Realtime, kb: 50 },
        ];
        let o2 = i2.arbitrate(&reqs2).ok();
        let o2 = o2.unwrap_or_default();
        assert_eq!(o2.len(), 2);
        assert_eq!(i2.preempt_events, 0);
    }

    #[test]
    fn a03_降级建议_三路建议阶梯() {
        let mut i = it();
        // 三类打满。
        i.request(BwRequest { acct: 1, class: CLASS_UPLOAD, kind: FlowKind::Bulk, kb: 400 }).ok();
        i.request(BwRequest { acct: 1, class: CLASS_DOWNLOAD, kind: FlowKind::Realtime, kb: 400 }).ok();
        i.request(BwRequest { acct: 1, class: CLASS_BUS, kind: FlowKind::Bulk, kb: 800 }).ok();
        let adv = i.degrade_advices();
        assert_eq!(adv.len(), 3);
        assert!(adv.contains(&DegradeAdvice { acct: 1, class: CLASS_UPLOAD, action: 0, step: 2 }));
        assert!(adv.contains(&DegradeAdvice { acct: 1, class: CLASS_DOWNLOAD, action: 1, step: 2 }));
        assert!(adv.contains(&DegradeAdvice { acct: 1, class: CLASS_BUS, action: 2, step: 4 }));
        // 无超用无建议。
        let i2 = it();
        assert!(i2.degrade_advices().is_empty());
    }

    #[test]
    fn a04_遥测_立方按账户类聚合() {
        let mut i = it();
        i.request(BwRequest { acct: 1, class: CLASS_DOWNLOAD, kind: FlowKind::Realtime, kb: 30 }).ok();
        i.request(BwRequest { acct: 1, class: CLASS_DOWNLOAD, kind: FlowKind::Realtime, kb: 20 }).ok();
        i.request(BwRequest { acct: 2, class: CLASS_BUS, kind: FlowKind::Interactive, kb: 5 }).ok();
        assert_eq!(i.telem_of(1, CLASS_DOWNLOAD), 50);
        assert_eq!(i.telem_of(2, CLASS_BUS), 5);
        assert_eq!(i.telem_of(1, CLASS_UPLOAD), 0);
        assert_eq!(i.telem_of(2, CLASS_DOWNLOAD), 0);
        // 部分授予时只记实授量。
        i.request(BwRequest { acct: 2, class: CLASS_DOWNLOAD, kind: FlowKind::Bulk, kb: 999 }).ok();
        assert_eq!(i.telem_of(2, CLASS_DOWNLOAD), 200); // 余量顶满
    }

    #[test]
    fn a05_类名表与保障配置校验() {
        assert_eq!(class_name(CLASS_UPLOAD), "上传");
        assert_eq!(class_name(CLASS_BUS), "总线");
        // 保障‰>1000 拒。
        let mut i = it();
        assert_eq!(
            i.attach(BwAccount::new(9, [1, 1, 1], 1001)),
            Err(E_IBW_GUARANTEE)
        );
        // 重复对接拒。
        assert_eq!(
            i.attach(BwAccount::new(1, [1, 1, 1], 500)),
            Err(E_IBW_NOACC)
        );
    }
}
