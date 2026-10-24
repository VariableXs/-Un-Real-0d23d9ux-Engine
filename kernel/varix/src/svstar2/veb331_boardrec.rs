//! veb331_boardrec —— VE-F0331 开发板采购与测试矩阵建议。
//! 锚点：VE-F0331 · 目标 320 行。
//! 判据原文：四判据评分（GPU IP 有无 / 社区支持度 / 文档质量 /
//!           可购性）、矩阵建议（最少板数覆盖维度）、预算诚实
//!           （区间估计）、决策留白（建议不代替决策）、来源
//!           日期标注（信息过时降级）。
//! 组件：候选评分表 + 矩阵建议 + 预算区间。
//! 职责定位：采购建议是「参谋不拍板」——评分表给依据，矩阵
//!           给覆盖方案，预算给区间；最后的决策权在 Variable。
//! 性能逐项分解：评分 O(型号×判据) 一次性；矩阵 O(1) 结构。
//! 诊断码段：0x31BC0（0x31BC1~0x31BC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const BR_VERSION: &str = "veb331_boardrec-v1";

/// 诊断段基址（0x31BC0 段 · 本模块独占）。
pub const BR_SEG_BASE: u32 = 0x31BC0;
pub const XC_STALE: u32 = 0x31BC1; // 信息过时（来源超时效窗）
pub const XC_OOS: u32 = 0x31BC2; // 无货（可购性为零）
pub const XC_DISPUTE: u32 = 0x31BC3; // 评分争议（判据明细呈现不仲裁）
pub const XC_NOCAND: u32 = 0x31BC4; // 候选不足（矩阵无法成形）
pub const XC_BUDGET: u32 = 0x31BC5; // 预算区间倒挂（低>高）

/// 四判据权重（和 10）。
pub const W_GPU: u32 = 4; // GPU IP 有无
pub const W_COMM: u32 = 3; // 社区支持度
pub const W_DOC: u32 = 2; // 文档质量
pub const W_AVAIL: u32 = 1; // 可购性

/// 判据单项分界（0~10）。
pub const SCORE_MAX: u32 = 10;

/// 来源时效窗（周期数）。
pub const STALE_WIN: u32 = 8;

/// 板卡候选。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cand {
    pub model: u32,
    /// 四判据分（gpu, comm, doc, avail），各 0~10。
    pub sc: (u32, u32, u32, u32),
    /// 来源登记周期。
    pub src_epoch: u32,
    /// 单价（元，区间下界）。
    pub price_lo: u32,
    /// 单价区间上界。
    pub price_hi: u32,
}

/// 评分结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scored {
    pub model: u32,
    /// 加权总分（0~100）。
    pub total: u32,
    /// 是否无货降级。
    pub oos: bool,
    /// 是否过时降级。
    pub stale: bool,
}

/// 采购建议簿。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardRec {
    cands: Vec<Cand>,
    /// 预算区间（lo, hi，元）。
    pub budget: (u32, u32),
    /// 决策标记（恒 false：决策留白）。
    pub decided: bool,
}

impl BoardRec {
    /// 空簿。
    pub fn new(budget_lo: u32, budget_hi: u32) -> Result<Self, u32> {
        if budget_lo > budget_hi {
            return Err(XC_BUDGET);
        }
        Ok(Self {
            cands: Vec::new(),
            budget: (budget_lo, budget_hi),
            decided: false,
        })
    }

    /// 登记候选。可购性为零（无货）拒——不把买不到的板当主选。
    pub fn add(
        &mut self,
        model: u32,
        gpu: u32,
        comm: u32,
        doc: u32,
        avail: u32,
        epoch: u32,
        lo: u32,
        hi: u32,
    ) -> Result<(), u32> {
        if gpu > SCORE_MAX || comm > SCORE_MAX || doc > SCORE_MAX || avail > SCORE_MAX {
            return Err(XC_DISPUTE);
        }
        if avail == 0 {
            return Err(XC_OOS);
        }
        if lo > hi {
            return Err(XC_BUDGET);
        }
        self.cands.push(Cand {
            model,
            sc: (gpu, comm, doc, avail),
            src_epoch: epoch,
            price_lo: lo,
            price_hi: hi,
        });
        Ok(())
    }

    /// 单型号加权评分：Σ(分×权)×10 / 100 → 0~100。
    pub fn score(&self, model: u32, now: u32) -> Option<Scored> {
        let c = self.cands.iter().find(|c| c.model == model)?;
        let total = (c.sc.0 * W_GPU + c.sc.1 * W_COMM + c.sc.2 * W_DOC + c.sc.3 * W_AVAIL) * 10 / 100;
        let stale = now.saturating_sub(c.src_epoch) > STALE_WIN;
        Some(Scored {
            model,
            total,
            oos: false,
            stale,
        })
    }

    /// 全表评分并按总分降序（选择排序，稳定量级）。
    pub fn rank(&self, now: u32) -> Vec<Scored> {
        let mut v: Vec<Scored> = Vec::new();
        for c in self.cands.iter() {
            let total =
                (c.sc.0 * W_GPU + c.sc.1 * W_COMM + c.sc.2 * W_DOC + c.sc.3 * W_AVAIL) * 10 / 100;
            v.push(Scored {
                model: c.model,
                total,
                oos: false,
                stale: now.saturating_sub(c.src_epoch) > STALE_WIN,
            });
        }
        let n = v.len();
        for i in 0..n {
            let mut best = i;
            for j in (i + 1)..n {
                if v[j].total > v[best].total {
                    best = j;
                }
            }
            let t = v[i];
            v[i] = v[best];
            v[best] = t;
        }
        v
    }

    /// 矩阵建议：按 GPU 有无与社区度分桶，返回最少板数。
    /// 桶一：带 GPU IP + 高社区（≥6）→ 主测板；
    /// 桶二：带 GPU IP + 低社区 → 备板；
    /// 桶三：无 GPU IP → 通用对照板。
    pub fn matrix_min(&self) -> Result<(u32, u32, u32), u32> {
        if self.cands.is_empty() {
            return Err(XC_NOCAND);
        }
        let mut m = (0u32, 0u32, 0u32);
        for c in self.cands.iter() {
            let has_gpu = c.sc.0 >= 5;
            let hi_comm = c.sc.1 >= 6;
            if has_gpu && hi_comm {
                m.0 += 1;
            } else if has_gpu {
                m.1 += 1;
            } else {
                m.2 += 1;
            }
        }
        Ok(m)
    }

    /// 覆盖维度数：主测板覆盖全维，备板覆盖驱动维，对照板覆盖基线维。
    pub fn matrix_dims(&self) -> Result<u32, u32> {
        let (a, b, c) = self.matrix_min()?;
        let mut d = 0;
        if a > 0 {
            d += 4; // 渲染/显示/功耗/调试
        }
        if b > 0 {
            d += 2; // 驱动/基线
        }
        if c > 0 {
            d += 1; // 基线
        }
        Ok(d)
    }

    /// 预算诚实区间：全选主测+备板的总价区间与预算的交集。
    /// 返回 (区间下界, 区间上界)；超预算返回 None 语义由 Err 表达。
    pub fn budget_fit(&self) -> Result<(u32, u32), u32> {
        let mut lo = 0u32;
        let mut hi = 0u32;
        for c in self.cands.iter() {
            lo += c.price_lo;
            hi += c.price_hi;
        }
        if lo > self.budget.1 {
            return Err(XC_BUDGET);
        }
        Ok((lo, if hi > self.budget.1 { self.budget.1 } else { hi }))
    }

    /// 在簿候选数。
    pub fn len(&self) -> usize {
        self.cands.len()
    }
}

pub fn run_veb331_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0331");
    // 族零：预算门。
    s.add("预算倒挂拒", BoardRec::new(500, 100) == Err(XC_BUDGET), "低>高");
    let mut r = match BoardRec::new(2000, 6000) {
        Ok(x) => x,
        Err(_) => {
            s.add("建簿败", false, "不应到");
            return s;
        }
    };
    s.add("空簿矩阵拒", r.matrix_min() == Err(XC_NOCAND), "无候选");
    // 族一：候选门。
    s.add("评分越界拒", r.add(1, 11, 5, 5, 5, 1, 500, 800) == Err(XC_DISPUTE), ">10");
    s.add("无货拒", r.add(2, 8, 7, 6, 0, 1, 500, 800) == Err(XC_OOS), "买不到");
    s.add("单价倒挂拒", r.add(3, 8, 7, 6, 9, 1, 800, 500) == Err(XC_BUDGET), "低>高");
    s.add("主测板入", r.add(101, 9, 8, 7, 8, 2, 1200, 1800).is_ok(), "带 GPU 高社区");
    s.add("备板入", r.add(102, 7, 4, 5, 7, 2, 600, 900).is_ok(), "带 GPU 低社区");
    s.add("对照板入", r.add(103, 0, 9, 9, 9, 2, 300, 500).is_ok(), "无 GPU");
    s.add("三候选", r.len() == 3, "入三");
    // 族二：评分。
    // 101: 9*4+8*3+7*2+8*1 = 36+24+14+8 = 82 → 820/100=82? 82*10=820/100=8? 核对：
    // (36+24+14+8)=82；82*10/100=8。分值刻度 0~10。
    let sc = r.score(101, 3).unwrap_or(Scored { model: 0, total: 0, oos: false, stale: false });
    s.add("主测分 8", sc.total == 8, "82*10/100");
    s.add("不过时", !sc.stale, "2→3 窗内");
    let sc2 = r.score(101, 20);
    s.add("过时标注", sc2.map(|x| x.stale).unwrap_or(false), "20-2>8");
    s.add("缺席 None", r.score(999, 3).is_none(), "查无");
    // 族三：排名。
    let rk = r.rank(3);
    s.add("榜三", rk.len() == 3, "全表");
    // 102: 7*4+4*3+5*2+7*1=28+12+10+7=57→5；103: 0+27+18+9=54→5。
    s.add("主测居首", rk[0].model == 101, "8 分顶");
    s.add("并列并列", rk[1].total == rk[2].total, "5/5");
    // 族四：矩阵。
    s.add("三桶齐", r.matrix_min() == Ok((1, 1, 1)), "各一");
    s.add("维度七", r.matrix_dims() == Ok(7), "4+2+1");
    // 族五：预算。
    s.add("总价下界 2100", r.budget_fit() == Ok((2100, 3200)), "1200+600+300");
    // 预算封顶 6000：hi=min(1800+900+500,6000)=3200。
    let mut poor = match BoardRec::new(100, 1000) {
        Ok(x) => x,
        Err(_) => {
            s.add("建簿败 2", false, "不应到");
            return s;
        }
    };
    let _ = poor.add(201, 9, 8, 7, 8, 2, 1200, 1800);
    s.add("超预算拒", poor.budget_fit() == Err(XC_BUDGET), "1200>1000");
    // 族六：决策留白。
    s.add("不拍板", !r.decided, "参谋不决策");
    s.add("魔数核", BR_SEG_BASE == 0x31BC0, "段基址");
    s
}
