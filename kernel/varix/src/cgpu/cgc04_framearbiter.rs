//! cgc04_framearbiter — CGPU-F0005 帧预算仲裁器（80 帧承诺的执行中枢）。
//!
//! 判据原文（施工书锚点 CGPU-F0005）：帧预算 = 12.5ms（80fps）；本功能把预算
//! 切成账户制：合成器保底 4ms（不可侵占）、交互响应 2ms、网页表面池 4ms（按
//! 可见性与优先级再分配）、动态画面池 1.5ms、机动 1ms。申请制：任何渲染工作
//! 必须声明预算标签（预估耗时 + 优先级 + 可降级档位），仲裁器按账户水位批准、
//! 排队或降级。超时处置：任务超预算时先走自身降级档（如模糊半径减半），帧级
//! 超时触发降质链（CGPU-F0008），禁止静默吞帧。预算交易：低峰期（静止桌面）
//! 剩余预算自动存入"信用池"，高峰期信用池优先补给交互路径。全部仲裁决策（批
//! 准/排队/降级 + 归因）入帧日志。判据：保底账户侵占测试零失败、降质链触发
//! 即测、信用池补给有效、仲裁开销 ≤50μs/帧、决策日志全量。
//!
//! 职责定位：帧内预算切分与申请仲裁（执行中枢）；与 cgc02_budgetmodel（F0322
//! 四池比例模型）分工——彼是跨帧口径建模、此是单帧内逐申请裁定；与
//! cgc03_budgetaccount（F0323 系统级账户树）分工——彼是系统树配额继承、此是
//! 帧内微账户。降质链只发信号不执行（F0008 消费）。判定步数账把"仲裁开销
//! ≤50μs/帧"红线步数化（每步常数操作，64 步 @ 数十 ns/步 ≪ 50μs 红线）。
//! 诊断码段 0x3Cxx 独占。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本（口径冻结标识，消费方对账用）。
pub const ARBITER_VERSION: &str = "C04-framearbiter-v1";

/// 帧预算 12.5ms = 80fps（与 cgc02_budgetmodel::FRAME_BUDGET_US 同源对拍，
/// 两处独立写死——漂移由 vcw05_docsync 对账发现）。
pub const FRAME_BUDGET_US: u64 = 12_500;

/// 仲裁开销红线的步数化表达：单帧内 submit+统计类判定的总步数上限。
/// 每步为常数级操作（查表/比较/入账），64 步实测远低于 50μs 判据红线；
/// 超限即 Err(PcStepOverrun)——开销失控必须显性而非静默变慢。
pub const DECISION_STEP_BUDGET: u32 = 64;

/// 合成器保底 4ms（判据原文数字，不可侵占）。
pub const COMPOSER_FLOOR_US: u64 = 4_000;

/// 五账户预算表（µs）：合成器保底/交互响应/网页表面池/动态画面池/机动。
/// 和恒等于 FRAME_BUDGET_US（4000+2000+4000+1500+1000=12500，构造时校验）。
pub const ACCT_BUDGET_US: [u64; 5] = [4_000, 2_000, 4_000, 1_500, 1_000];

/// 信用池补给优先序：高峰期先补交互路径（判据原文），再补动态，其余不补。
pub const CREDIT_REFILL_ORDER: [usize; 2] = [1, 3];

/// 申请账户闭集（ordinal 对齐 ACCT_BUDGET_US 下标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcctKind {
    /// 合成器保底——不可侵占（判据原文）。
    Composer,
    /// 交互响应。
    Interactive,
    /// 网页表面池——按可见性与优先级再分配。
    WebSurfaces,
    /// 动态画面池。
    Dynamic,
    /// 机动池。
    Discretionary,
}

impl AcctKind {
    pub const ALL: [AcctKind; 5] = [
        AcctKind::Composer,
        AcctKind::Interactive,
        AcctKind::WebSurfaces,
        AcctKind::Dynamic,
        AcctKind::Discretionary,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            AcctKind::Composer => 0,
            AcctKind::Interactive => 1,
            AcctKind::WebSurfaces => 2,
            AcctKind::Dynamic => 3,
            AcctKind::Discretionary => 4,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            AcctKind::Composer => "合成器保底",
            AcctKind::Interactive => "交互响应",
            AcctKind::WebSurfaces => "网页表面池",
            AcctKind::Dynamic => "动态画面池",
            AcctKind::Discretionary => "机动",
        }
    }

    /// 是否保底不可侵占账户（判据：保底账户侵占测试零失败的判定对象）。
    pub const fn is_floor(self) -> bool {
        matches!(self, AcctKind::Composer)
    }
}

/// 申请优先级（四级；Queued 出队与信用补给按级排序）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl Priority {
    pub const fn rank(self) -> u8 {
        self as u8
    }
}

/// 预算标签申请（判据原文三要素：预估耗时 + 优先级 + 可降级档位）。
#[derive(Clone, Debug)]
pub struct Request {
    /// 申请账户（必须声明，未声明无法仲裁）。
    pub account: AcctKind,
    /// 预估耗时（µs，全档位后仍必须 >0）。
    pub est_us: u64,
    /// 优先级。
    pub prio: Priority,
    /// 可降级档位：逐档递减的耗时承诺（如模糊半径减半），升序下标即档位序。
    /// 空表 = 不可降级（超水位只能 Queued/Rejected）。
    pub degrade_ladder: Vec<u64>,
}

impl Request {
    /// 档位合法性：逐档递减且全部 < 预估、且末档 >0（0 耗时=静默吞帧等价物，
    /// 禁止）。
    pub fn ladder_valid(&self) -> bool {
        let l = &self.degrade_ladder;
        if l.is_empty() {
            return true; // 不可降级是合法声明
        }
        let mut prev = self.est_us;
        for &v in l {
            if v == 0 || v >= prev {
                return false;
            }
            prev = v;
        }
        true
    }
}

/// 诊断码（0x3Cxx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x3C01 保底账户侵占企图被拒（任何路径 grant 后 used 不得超 floor）。
    PcComposerImmune,
    /// 0x3C02 申请未带账户标签或账户越界。
    PcNoAccount,
    /// 0x3C03 账户水位不足且无信用可补——排队。
    PcAccountExhausted,
    /// 0x3C04 信用池不足以完成补给。
    PcCreditInsufficient,
    /// 0x3C05 仲裁步数超限——决策开销失控显性化（≤50μs 红线的步数表达）。
    PcStepOverrun,
    /// 0x3C06 帧级超时——降质链信号已发（F0008 消费），留痕。
    PcFrameOvertime,
    /// 0x3C07 降级档表非法（非递减/含零档——零耗时=静默吞帧等价物）。
    PcLadderInvalid,
    /// 0x3C08 帧日志序断裂（seq 不单调——账本完整性破坏）。
    PcLogBroken,
    /// 0x3C09 预算表构造非法（五账户和≠帧预算）。
    PcBudgetSum,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明——消费方直接呈现，不再二次解释）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::PcComposerImmune => "保底账户不可侵占：合成器 4ms 是呈现生命线，任何申请不得使其超额",
            XcCode::PcNoAccount => "申请缺少账户标签：渲染工作必须声明预算账户才能仲裁",
            XcCode::PcAccountExhausted => "账户水位不足且信用池不可补：申请转入排队（归因留痕）",
            XcCode::PcCreditInsufficient => "信用池余量不足以完成交互路径补给",
            XcCode::PcStepOverrun => "仲裁决策步数超限：开销红线失守，仲裁器必须显性上报而非静默变慢",
            XcCode::PcFrameOvertime => "帧级超时：降质链信号已发出（由 F0008 消费执行）",
            XcCode::PcLadderInvalid => "降级档表非法：档位必须逐档递减且不含零档（零耗时=静默吞帧等价物）",
            XcCode::PcLogBroken => "帧日志序断裂：决策账本 seq 必须单调，断裂即立案",
            XcCode::PcBudgetSum => "五账户预算和≠帧预算：账户表构造非法",
        }
    }
}

/// 仲裁裁定（四态闭集：批准/排队/降级/拒绝——判据原文"批准、排队或降级"
/// 外加显性拒绝；静默吞帧禁止=拒绝必须带归因码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 全额批准。
    Granted,
    /// 排队（出队序按优先级；带队列位次）。
    Queued,
    /// 降级批准（套用第几档，实际批准耗时）。
    Degraded,
    /// 显性拒绝（带归因码——不允许无因拒）。
    Rejected,
}

impl Verdict {
    pub const fn zh(self) -> &'static str {
        match self {
            Verdict::Granted => "批准",
            Verdict::Queued => "排队",
            Verdict::Degraded => "降级批准",
            Verdict::Rejected => "拒绝",
        }
    }
}

/// 单条仲裁决策（帧日志行——判据"决策日志全量可回放"的载体）。
#[derive(Clone, Debug)]
pub struct Decision {
    /// 帧号。
    pub frame_no: u64,
    /// 帧内序号（单调；断裂即 PcLogBroken 立案）。
    pub seq: u32,
    /// 申请账户。
    pub account: AcctKind,
    /// 申请优先级。
    pub prio: Priority,
    /// 原始预估耗时（µs）。
    pub est_us: u64,
    /// 裁定。
    pub verdict: Verdict,
    /// 实际批准耗时（Granted/Degraded 有效；Queued/Rejected 为 0）。
    pub grant_us: u64,
    /// 归因（人读；Rejected 时为诊断码 detail）。
    pub reason: &'static str,
}

/// 信用池（判据：低峰剩余预算自动存入，高峰期优先补给交互路径）。
#[derive(Clone, Debug, Default)]
pub struct CreditPool {
    /// 按账户累积的信用（µs；只增于存入、减于补给）。
    credits: [u64; 5],
    /// 累计存入总量（对账用）。
    deposited_total: u64,
    /// 累计补给总量（对账用；deposited_total ≥ refilled_total 守恒）。
    refilled_total: u64,
}

impl CreditPool {
    /// 低峰存入：把账户本帧剩余预算存为信用。
    pub fn deposit(&mut self, acct: AcctKind, remain_us: u64) {
        let i = acct.ordinal();
        self.credits[i] = self.credits[i].saturating_add(remain_us);
        self.deposited_total = self.deposited_total.saturating_add(remain_us);
    }

    /// 高峰补给：从指定账户信用中扣（补给上限=该账户信用存量）。
    pub fn refill(&mut self, acct: AcctKind, want_us: u64) -> u64 {
        let i = acct.ordinal();
        let give = want_us.min(self.credits[i]);
        self.credits[i] -= give;
        self.refilled_total = self.refilled_total.saturating_add(give);
        give
    }

    pub const fn balance(&self, acct: AcctKind) -> u64 {
        self.credits[acct.ordinal()]
    }

    /// 守恒：累计补给 ≤ 累计存入（信用不是凭空产生）。
    pub const fn conserves(&self) -> bool {
        self.refilled_total <= self.deposited_total
    }

    pub const fn deposited_total(&self) -> u64 {
        self.deposited_total
    }

    pub const fn refilled_total(&self) -> u64 {
        self.refilled_total
    }
}

/// 帧预算仲裁器（执行中枢本体）。
pub struct FrameArbiter {
    /// 五账户预算表（构造冻结）。
    budget: [u64; 5],
    /// 当前帧各账户已授出（µs）。
    used: [u64; 5],
    /// 信用池。
    credits: CreditPool,
    /// 帧日志（全量决策账）。
    log: Vec<Decision>,
    /// 帧内 seq 发生器。
    seq: u32,
    /// 当前帧号。
    frame_no: u64,
    /// 本帧已耗判定步数（开销红线）。
    steps: u32,
    /// 队列长度（Queued 申请计数，供位次与出队统计）。
    queued: u32,
    /// 降质链信号（帧级超时置位，F0008 消费后由消费方清）。
    pub degrade_chain_fired: bool,
    /// 累计立案（跨帧异常计数）。
    pub cases: u32,
}

impl FrameArbiter {
    /// 按判据默认五账户表构造（表和≠帧预算即拒绝构造——PcBudgetSum）。
    pub fn new() -> Result<Self, XcCode> {
        Self::with_budget(ACCT_BUDGET_US)
    }

    /// 显式预算表构造（账户化切分可调，但和必须恰等于帧预算）。
    pub fn with_budget(budget: [u64; 5]) -> Result<Self, XcCode> {
        let mut sum = 0u64;
        for &b in &budget {
            sum = sum.saturating_add(b);
        }
        if sum != FRAME_BUDGET_US {
            return Err(XcCode::PcBudgetSum);
        }
        if budget[AcctKind::Composer.ordinal()] != COMPOSER_FLOOR_US {
            return Err(XcCode::PcBudgetSum); // 保底额度被改动同样是构造非法
        }
        Ok(FrameArbiter {
            budget,
            used: [0; 5],
            credits: CreditPool::default(),
            log: Vec::new(),
            seq: 0,
            frame_no: 0,
            steps: 0,
            queued: 0,
            degrade_chain_fired: false,
            cases: 0,
        })
    }

    /// 帧开始：重置帧内水位与步数；seq 跨帧单调（日志连续性以全局 seq 表达）。
    pub fn begin_frame(&mut self, frame_no: u64) {
        self.used = [0; 5];
        self.steps = 0;
        self.queued = 0;
        self.frame_no = frame_no;
        self.degrade_chain_fired = false;
    }

    /// 判定步数计费（内部统一入口——超限显性报错）。
    fn step(&mut self) -> Result<(), XcCode> {
        self.steps = self.steps.saturating_add(1);
        if self.steps > DECISION_STEP_BUDGET {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::PcStepOverrun);
        }
        Ok(())
    }

    /// 账户剩余水位。
    pub const fn remaining(&self, acct: AcctKind) -> u64 {
        let i = acct.ordinal();
        if self.used[i] >= self.budget[i] {
            0
        } else {
            self.budget[i] - self.used[i]
        }
    }

    /// 提交申请（判据主路径：按账户水位批准、排队或降级）。
    ///
    /// 裁定序：①步数红线 → ②降级档合法性 → ③保底侵占防线 → ④全额批准
    /// → ⑤降级档逐档试 → ⑥信用池补给（仅交互） → ⑦排队/显性拒绝。
    pub fn submit(&mut self, req: &Request) -> Result<Verdict, XcCode> {
        self.step()?;
        if !req.ladder_valid() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::PcLadderInvalid);
        }
        let i = req.account.ordinal();
        let remain = self.remaining(req.account);

        // ③ 保底侵占防线：保底账户超 floor 的任何路径都不给授权。
        if req.account.is_floor() {
            let used = self.used[i];
            if used.saturating_add(req.est_us) > COMPOSER_FLOOR_US {
                // 先试降级档（保底内降级合法——侵占的是"超额部分"）
                if let Some((idx, g)) = self.ladder_fit(req, remain) {
                    return self.grant_degraded(req, idx, g);
                }
                return self.queue_or_reject(req, XcCode::PcComposerImmune);
            }
        }

        // ④ 全额批准。
        if req.est_us <= remain {
            return self.grant_full(req);
        }

        // ⑤ 降级档逐档试（判据：任务超预算时先走自身降级档）。
        if let Some((idx, g)) = self.ladder_fit(req, remain) {
            return self.grant_degraded(req, idx, g);
        }

        // ⑥ 信用池补给——判据"高峰期信用池优先补给交互路径"。
        if matches!(req.account, AcctKind::Interactive) {
            let need = req.est_us - remain;
            let got = self.credits.refill(AcctKind::Interactive, need);
            if got == need {
                return self.grant_full(req);
            }
            // 部分补给也回退：信用守恒优先，退还已扣部分（不半拉子授权）
            self.credits.deposit(AcctKind::Interactive, got);
            self.cases = self.cases.saturating_add(1);
            return self.queue_or_reject(req, XcCode::PcCreditInsufficient);
        }

        // ⑦ 排队或显性拒绝（禁止静默吞帧：两态都留痕带归因）。
        self.queue_or_reject(req, XcCode::PcAccountExhausted)
    }

    /// 找第一个能塞进剩余水位的降级档（返回 (档位序, 批准耗时)）。
    fn ladder_fit(&self, req: &Request, remain: u64) -> Option<(usize, u64)> {
        for (idx, &v) in req.degrade_ladder.iter().enumerate() {
            if v <= remain && v > 0 {
                return Some((idx, v));
            }
        }
        None
    }

    /// 全额批准入账。
    fn grant_full(&mut self, req: &Request) -> Result<Verdict, XcCode> {
        self.commit(req, Verdict::Granted, req.est_us, "水位足够全额批准")?;
        Ok(Verdict::Granted)
    }

    /// 降级批准入账。
    fn grant_degraded(&mut self, req: &Request, ladder_idx: usize, grant: u64) -> Result<Verdict, XcCode> {
        let _ = ladder_idx; // 档位序由消费方按 ladder 数组下标对回（log 行带 grant 值）
        self.commit(req, Verdict::Degraded, grant, "超水位：套用降级档")?;
        Ok(Verdict::Degraded)
    }

    /// 排队/拒绝裁定：Critical/High 排队，Normal/Low 显性拒绝（归因必带）。
    fn queue_or_reject(&mut self, req: &Request, why: XcCode) -> Result<Verdict, XcCode> {
        let v = match req.prio {
            Priority::Critical | Priority::High => {
                self.queued = self.queued.saturating_add(1);
                Verdict::Queued
            }
            Priority::Normal | Priority::Low => Verdict::Rejected,
        };
        self.commit(req, v, 0, why.detail())?;
        Ok(v)
    }

    /// 统一入账（判定步数在 submit 已计，commit 只记账）。
    fn commit(&mut self, req: &Request, v: Verdict, grant: u64, reason: &'static str) -> Result<(), XcCode> {
        let i = req.account.ordinal();
        self.used[i] = self.used[i].saturating_add(grant);
        self.seq = self.seq.saturating_add(1);
        self.log.push(Decision {
            frame_no: self.frame_no,
            seq: self.seq,
            account: req.account,
            prio: req.prio,
            est_us: req.est_us,
            verdict: v,
            grant_us: grant,
            reason,
        });
        Ok(())
    }

    /// 帧级超时上报（判据：帧级超时触发降质链 F0008，禁止静默吞帧）。
    pub fn report_frame_overtime(&mut self, actual_us: u64) -> Result<(), XcCode> {
        self.step()?;
        if actual_us > FRAME_BUDGET_US {
            self.degrade_chain_fired = true;
            self.cases = self.cases.saturating_add(1);
            self.seq = self.seq.saturating_add(1);
            self.log.push(Decision {
                frame_no: self.frame_no,
                seq: self.seq,
                account: AcctKind::Discretionary,
                prio: Priority::Critical,
                est_us: FRAME_BUDGET_US,
                verdict: Verdict::Rejected,
                grant_us: 0,
                reason: XcCode::PcFrameOvertime.detail(),
            });
        }
        Ok(())
    }

    /// 低峰收尾：把各账户剩余预算存入信用池（判据"低峰期剩余预算自动存入"）。
    /// 静止桌面判定由调用方（场景层）给出——本模块只管存。
    pub fn deposit_all_remain(&mut self) {
        for k in AcctKind::ALL {
            let r = self.remaining(k);
            if r > 0 {
                self.used[k.ordinal()] = self.used[k.ordinal()].saturating_add(r);
                self.credits.deposit(k, r);
            }
        }
    }

    /// 信用池只读访问（消费方查询补给有效性）。
    pub const fn credit(&self) -> &CreditPool {
        &self.credits
    }

    /// 本帧队列长度。
    pub const fn queued(&self) -> u32 {
        self.queued
    }

    /// 帧日志全量（判据"决策日志全量"）。
    pub fn log(&self) -> &[Decision] {
        &self.log
    }

    /// 帧日志连续性核验：seq 严格单调递增（断裂=PcLogBroken）。
    /// 立案计数随核验写账（&mut self——立案即状态变更）。
    pub fn log_sequential(&mut self) -> Result<(), XcCode> {
        let mut prev = 0u32;
        for d in &self.log {
            if d.seq <= prev {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::PcLogBroken);
            }
            prev = d.seq;
        }
        Ok(())
    }

    /// 不变量核验（判据"保底账户侵占测试零失败"的机械化形态）：
    /// ① 各账户 used ≤ budget；② 保底 used ≤ COMPOSER_FLOOR_US；
    /// ③ 信用守恒（补给 ≤ 存入）。任一破坏即 Err 并立案。
    pub fn invariants_hold(&mut self) -> Result<(), XcCode> {
        for k in AcctKind::ALL {
            let i = k.ordinal();
            if self.used[i] > self.budget[i] {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::PcBudgetSum);
            }
        }
        if self.used[AcctKind::Composer.ordinal()] > COMPOSER_FLOOR_US {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::PcComposerImmune);
        }
        if !self.credits.conserves() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::PcCreditInsufficient);
        }
        Ok(())
    }

    /// 本帧判定步数（开销对账用；红线 DECISION_STEP_BUDGET）。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 单行摘要（帧号/步数/队列/降质链——上级日志消费）。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "帧{} 步数{}/{} 排队{} 降质链{} 立案{} 信用存{}补{}",
            self.frame_no,
            self.steps,
            DECISION_STEP_BUDGET,
            self.queued,
            self.degrade_chain_fired,
            self.cases,
            self.credits.deposited_total(),
            self.credits.refilled_total(),
        )
    }
}
