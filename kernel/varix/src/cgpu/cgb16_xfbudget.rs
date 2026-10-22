//! cgb16_xfbudget — CGPU-F0229 预算跨帧记账。
//!
//! 判据原文（施工书锚点 CGPU-F0229）：预算记账：B04 预算体系的跨帧扩展（每
//! 帧预算含"偿还上一帧欠账"项——跨帧资源消耗诚实记账）；超预算跨帧策略（连
//! 续超支→降档建议→上游限流——与 B03 并行切分联动）；记账查询（每帧/滚动窗
//! 口资源账目可查——12 章资源透明）。判据：偿还项、降档联动、账目查询。
//!
//! 职责定位：跨帧预算诚实账。本帧花超的算力不是消失了，是变成欠账滚到下一
//! 帧——账不平的调度迟早爆。三件事：①偿还项——每帧预算先扣"上帧欠账"再
//! 分配（欠账优先于新消费，否则欠账永续）；②超支策略——连续超支帧数达门槛
//! → 产出降档建议（供 B03/B04 消费）→ 达上限则立案限流（上游减供而非账面
//! 掩盖）；③双粒度查询——逐帧账目 + 滚动窗聚合（8 章"资源透明"口径）。
//! 守恒恒等式：分配 = 消费 + 结转欠账，每帧核验，破恒等即错。诊断码段
//! 0xD460。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_BUDGET_VERSION: &str = "B16-xfbudget-v1";

// ---------------------------------------------------------------------------
// 诊断码（0xD460 槽）
// ---------------------------------------------------------------------------

/// 守恒破（分配 ≠ 消费 + 结转——账不平，拒绝收帧）。
pub const XC_CONSERVE_BROKEN: u32 = 0xD460;
/// 超支未立案（连续超支达门槛但无降档建议——诚实记账违约）。
pub const XC_OVERSPEND_SILENT: u32 = 0xD461;
/// 帧账重复收（同帧号二次记账）。
pub const XC_DUP_FRAME: u32 = 0xD462;
/// 帧号跳账（记账必须逐帧连续——跳帧即丢账）。
pub const XC_FRAME_GAP: u32 = 0xD463;
/// 消费超申请（单帧消费 > 分配 + 上帧结转的可透支余量）。
pub const XC_SPEND_OVER_GRANT: u32 = 0xD464;
/// 降档建议悬空（建议已产出但无人签收——资源透明闭环断）。
pub const XC_ADVICE_UNCLAIMED: u32 = 0xD465;
/// 窗口查询越界（滚动窗长非法）。
pub const XC_WINDOW_BAD: u32 = 0xD466;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 连续超支降档门槛（帧）。
pub const OVERSPEND_DOWNGRADE_AT: u32 = 3;
/// 连续超支限流门槛（帧）。
pub const OVERSPEND_LIMIT_AT: u32 = 8;
/// 透支余量：允许单帧透支上帧分配的 1/4（四舍五入向下——保守）。
pub const OVERDRAFT_NUM: u64 = 1;
pub const OVERDRAFT_DEN: u64 = 4;
/// 滚动窗长上限。
pub const WINDOW_CAP: usize = 256;
/// 逐帧账留存上限（超出滚出最旧——账目窗口）。
pub const LEDGER_CAP: usize = 1024;

// ---------------------------------------------------------------------------
// 帧账
// ---------------------------------------------------------------------------

/// 一帧的预算账。
#[derive(Clone, Debug)]
pub struct FrameBudget {
    pub frame: u64,
    /// 本帧总分配（微秒级预算单位——与 B03/B04 同口径，整数）。
    pub granted: u64,
    /// 偿还项：从分配中划给上帧欠账的额度。
    pub repay: u64,
    /// 实际消费。
    pub spent: u64,
    /// 收帧时的欠账结转（本帧未清的差额）。
    pub carried: u64,
    /// 本帧是否超支（spent > granted - repay 的净可花额）。
    pub overspent: bool,
    /// 收帧核验通过标记。
    pub sealed: bool,
}

// ---------------------------------------------------------------------------
// 跨帧预算引擎
// ---------------------------------------------------------------------------

/// 跨帧预算记账引擎。
pub struct XfBudgetEngine {
    pub name: String,
    /// 基线帧预算（每帧的名义额度——B04 预算体系下发的口径）。
    pub base_grant: u64,
    /// 当前欠账（跨帧滚动的单一真源）。
    pub debt: u64,
    /// 逐帧账。
    ledger: Vec<FrameBudget>,
    /// 降档建议账（产出待签收）。
    advices: Vec<(u64, String)>,
    /// 限流立案账。
    limits: Vec<(u64, String)>,
    /// 连续超支计数。
    streak: u32,
    /// 已签收的建议数（闭环对账）。
    pub advices_claimed: u64,
    /// 统计。
    pub total_rejects: u64,
    rejections: Vec<(u32, String)>,
}

impl XfBudgetEngine {
    pub fn new(name: &str, base_grant: u64) -> Self {
        XfBudgetEngine {
            name: String::from(name),
            base_grant,
            debt: 0,
            ledger: Vec::new(),
            advices: Vec::new(),
            limits: Vec::new(),
            streak: 0,
            advices_claimed: 0,
            total_rejects: 0,
            rejections: Vec::new(),
        }
    }

    fn reject(&mut self, code: u32, why: &str) -> u32 {
        self.rejections.push((code, String::from(why)));
        if self.rejections.len() > 256 {
            self.rejections.remove(0);
        }
        self.total_rejects += 1;
        code
    }

    // ---------------- 记账主流程 ----------------

    /// 开帧：分配 + 偿还项计算。偿还项 = min(欠账, 分配×3/4)——至少留 1/4
    /// 给新消费，防止欠账把帧吃穿导致永久降档螺旋。
    pub fn open_frame(&mut self, frame: u64) -> Result<(u64, u64), u32> {
        if let Some(last) = self.ledger.last() {
            if frame <= last.frame {
                return Err(self.reject(XC_DUP_FRAME, "帧号不前进（重复或倒退）"));
            }
            if frame > last.frame + 1 {
                return Err(self.reject(XC_FRAME_GAP, "帧号跳账——记账必须逐帧连续"));
            }
        }
        let grant = self.base_grant;
        let repay_cap = grant * 3 / 4;
        let repay = if self.debt < repay_cap { self.debt } else { repay_cap };
        self.debt -= repay;
        self.ledger.push(FrameBudget {
            frame,
            granted: grant,
            repay,
            spent: 0,
            carried: 0,
            overspent: false,
            sealed: false,
        });
        if self.ledger.len() > LEDGER_CAP {
            self.ledger.remove(0);
        }
        Ok((grant, repay))
    }

    /// 记一笔消费。净可花额 = granted - repay + 透支余量（上帧分配的 1/4）。
    pub fn spend(&mut self, frame: u64, amount: u64) -> Result<(), u32> {
        let n = self.ledger.len();
        if n == 0 || self.ledger[n - 1].frame != frame {
            return Err(self.reject(XC_FRAME_GAP, "消费挂在不存在的当前帧"));
        }
        if self.ledger[n - 1].sealed {
            return Err(self.reject(XC_DUP_FRAME, "帧已封账再消费"));
        }
        let f = &mut self.ledger[n - 1];
        let overdraft = f.granted * OVERDRAFT_NUM / OVERDRAFT_DEN;
        let net = f.granted - f.repay;
        if f.spent + amount > net + overdraft {
            return Err(self.reject(XC_SPEND_OVER_GRANT, "消费超分配+透支余量"));
        }
        f.spent += amount;
        Ok(())
    }

    /// 收帧封账：算结转、核守恒、跑超支策略。判据"偿还项""降档联动"在此落地。
    pub fn close_frame(&mut self, frame: u64) -> Result<FrameBudget, u32> {
        let n = self.ledger.len();
        if n == 0 || self.ledger[n - 1].frame != frame {
            return Err(self.reject(XC_FRAME_GAP, "封账帧不存在"));
        }
        // 守恒：分配 = 偿还 + 净花 + 结转 + 退回（本帧未花完的回收抵欠账）。
        let f = &mut self.ledger[n - 1];
        let net_spend = f.spent;
        let refund = if net_spend + f.repay <= f.granted {
            f.granted - f.repay - net_spend
        } else {
            0
        };
        let over = if net_spend + f.repay > f.granted {
            net_spend + f.repay - f.granted
        } else {
            0
        };
        // 退回先抵欠账。
        if refund > 0 && self.debt >= refund {
            self.debt -= refund;
        } else if refund > 0 {
            self.debt = 0;
        }
        f.carried = over;
        self.debt += over;
        f.overspent = over > 0;
        // 守恒核验：granted == repay + spent - refund + carried（整数恒等式）。
        let lhs = f.granted as i64;
        let rhs = f.repay as i64 + f.spent as i64 - refund as i64 + f.carried as i64;
        if lhs != rhs {
            f.sealed = false;
            let c = self.reject(XC_CONSERVE_BROKEN, "帧账守恒破（分配≠偿还+消费-退回+结转）");
            return Err(c);
        }
        f.sealed = true;
        // 超支策略：连续超支 → 降档建议 → 限流。
        if f.overspent {
            self.streak += 1;
            if self.streak == OVERSPEND_DOWNGRADE_AT {
                let mut s = String::from("连续超支 ");
                s.push_str(itoa(OVERSPEND_DOWNGRADE_AT as u64));
                s.push_str(" 帧——建议上游降档一档（B03/B04 消费）");
                self.advices.push((frame, s));
            }
            if self.streak == OVERSPEND_LIMIT_AT {
                let mut s = String::from("连续超支 ");
                s.push_str(itoa(OVERSPEND_LIMIT_AT as u64));
                s.push_str(" 帧——立案限流：上游减供，欠账 ");
                s.push_str(itoa(self.debt));
                self.limits.push((frame, s));
            }
        } else {
            self.streak = 0;
        }
        Ok(self.ledger[n - 1].clone())
    }

    // ---------------- 查询（判据"账目查询"） ----------------

    /// 逐帧账只读投影。
    pub fn frames(&self) -> &[FrameBudget] {
        &self.ledger
    }

    /// 滚动窗聚合（最近 win 帧的分配/消费/欠账总和——8 章资源透明口径）。
    pub fn window_summary(&self, win: usize) -> Result<(u64, u64, u64), u32> {
        if win == 0 || win > WINDOW_CAP {
            return Err(self.reject(XC_WINDOW_BAD, "滚动窗长非法"));
        }
        let n = self.ledger.len();
        let take = if win < n { win } else { n };
        let mut g = 0;
        let mut s = 0;
        let mut c = 0;
        let mut i = n - take;
        while i < n {
            g += self.ledger[i].granted;
            s += self.ledger[i].spent;
            c += self.ledger[i].carried;
            i += 1;
        }
        Ok((g, s, c))
    }

    /// 建议签收：资源透明闭环（判据"降档联动"的下游确认）。
    pub fn claim_advice(&mut self) -> Result<String, u32> {
        if self.advices.is_empty() {
            return Err(self.reject(XC_ADVICE_UNCLAIMED, "无在册建议可签收"));
        }
        let (_, s) = self.advices.remove(0);
        self.advices_claimed += 1;
        Ok(s)
    }

    /// 建议账只读投影。
    pub fn pending_advices(&self) -> &[(u64, String)] {
        &self.advices
    }

    /// 限流立案账只读投影。
    pub fn limit_cases(&self) -> &[(u64, String)] {
        &self.limits
    }

    /// 引擎核验：账内帧全部封账 + 守恒 + 欠账与逐帧结转一致。
    pub fn validate(&self) -> Result<(), u32> {
        let mut carried_total: u64 = 0;
        for f in self.ledger.iter() {
            if !f.sealed {
                return Err(XC_DUP_FRAME);
            }
            carried_total += f.carried;
        }
        // 欠账来源只有逐帧结转（退款抵扣在引擎内，这里做上界核验）。
        if self.debt > carried_total {
            return Err(XC_CONSERVE_BROKEN);
        }
        if self.streak >= OVERSPEND_DOWNGRADE_AT && self.advices.is_empty() && self.limits.is_empty()
        {
            return Err(XC_OVERSPEND_SILENT);
        }
        Ok(())
    }

    /// 快照。
    pub fn snapshot(&self) -> BudgetSnap {
        BudgetSnap {
            version: String::from(XF_BUDGET_VERSION),
            engine: self.name.clone(),
            base: self.base_grant,
            debt: self.debt,
            frames: self.ledger.len() as u64,
            overspent_frames: self.ledger.iter().filter(|f| f.overspent).count() as u64,
            advices: self.advices.len() as u64,
            limits: self.limits.len() as u64,
            rejects: self.total_rejects,
        }
    }
}

/// 快照。
#[derive(Clone, Debug)]
pub struct BudgetSnap {
    pub version: String,
    pub engine: String,
    pub base: u64,
    pub debt: u64,
    pub frames: u64,
    pub overspent_frames: u64,
    pub advices: u64,
    pub limits: u64,
    pub rejects: u64,
}

impl BudgetSnap {
    pub fn detail(&self) -> String {
        let mut s = String::from("[跨帧预算 ");
        s.push_str(&self.engine);
        s.push_str(" 基线=");
        s.push_str(itoa(self.base));
        s.push_str(" 欠账=");
        s.push_str(itoa(self.debt));
        s.push_str(" 帧=");
        s.push_str(itoa(self.frames));
        s.push_str(" 超支=");
        s.push_str(itoa(self.overspent_frames));
        s.push_str(" 建议=");
        s.push_str(itoa(self.advices));
        s.push_str(" 限流=");
        s.push_str(itoa(self.limits));
        s.push_str("]");
        s
    }
}

fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    match core::str::from_utf8(&buf[i..]) {
        Ok(s) => String::from(s),
        Err(_) => String::from("?"),
    }
}

// ---------------------------------------------------------------------------
// 场景化构造（纯函数，确定性）
// ---------------------------------------------------------------------------

/// 场景 A：超支→欠账→偿还闭环。三帧：首帧超支，次帧偿还，第三帧清零。
pub fn scenario_repay_loop() -> Result<BudgetSnap, u32> {
    let mut e = XfBudgetEngine::new("B16-偿还闭环", 1000);
    e.open_frame(1)?;
    e.spend(1, 1150)?; // 净可花 1000 + 透支 250 → 合法超支 150。
    e.close_frame(1)?;
    if e.debt != 150 {
        return Err(XC_CONSERVE_BROKEN);
    }
    e.open_frame(2)?; // 偿还项 = min(150, 750) = 150。
    e.spend(2, 800)?; // 净可花 850 → 不超支。
    e.close_frame(2)?;
    if e.debt != 0 {
        return Err(XC_CONSERVE_BROKEN);
    }
    e.open_frame(3)?;
    e.spend(3, 1000)?;
    e.close_frame(3)?;
    e.validate()?;
    Ok(e.snapshot())
}

/// 场景 B：降档联动——连续三帧超支产出建议并可签收。
pub fn scenario_downgrade_advice() -> Result<BudgetSnap, u32> {
    let mut e = XfBudgetEngine::new("B16-降档", 1000);
    let mut f = 1;
    while f <= 3 {
        e.open_frame(f)?;
        e.spend(f, 1240)?; // 顶格透支。
        e.close_frame(f)?;
        f += 1;
    }
    if e.pending_advices().is_empty() {
        return Err(XC_OVERSPEND_SILENT);
    }
    e.claim_advice()?;
    e.validate()?;
    Ok(e.snapshot())
}

/// 场景 C：守恒负向——封账前引擎内部守恒检查捕获不平账（用 spend 越权构造）。
pub fn scenario_conserve_guard() -> Result<(), u32> {
    let mut e = XfBudgetEngine::new("B16-守恒拒", 1000);
    e.open_frame(1)?;
    match e.spend(1, 2000) {
        Err(code) if code == XC_SPEND_OVER_GRANT => Ok(()),
        Ok(()) => Err(XC_SPEND_OVER_GRANT),
        Err(other) => Err(other),
    }
}

/// 场景 D：跳账拒绝——帧号跳跃记账必须被拒。
pub fn scenario_frame_gap() -> Result<(), u32> {
    let mut e = XfBudgetEngine::new("B16-跳账拒", 1000);
    e.open_frame(1)?;
    e.spend(1, 10)?;
    e.close_frame(1)?;
    match e.open_frame(5) {
        Err(code) if code == XC_FRAME_GAP => Ok(()),
        Ok(_) => Err(XC_FRAME_GAP),
        Err(other) => Err(other),
    }
}
