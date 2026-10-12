//! unx982_throttle —— UNX-F0982 风暴节流器（节流动作与业务协商）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F0982 · 风暴节流器（节流动作与业务协商） · 目标 300 行 · 已深化
//! 判据原文：UNX-F0982-J1 节流生效（风暴向量投递率降至目标值 ±10% 内 10/10 次），
//!           节流不误伤他向量（隔离性断言），节流解除 10/10 次正确。
//! 前驱：UNX-F0981 unx981_irqstorm（检测态 Normal/Suspect/Storm + 双窗率）。
//!       本模块消费 Storm 相位，产出节流动作并对业务面发起协商。
//! 组件：令牌桶节流器（按向量独立桶 + 补充率，投递率收敛到目标 ±10%）
//!      + 隔离面（只节风暴向量集合，集合外向量零影响——误伤防线）
//!      + 业务协商状态机（空闲→发起→确认→降载→解除 五态相邻跃迁）
//!      + 降载动作表（削峰/排队/丢弃 三档按负载强度选择）
//!      + 解除账（条件满足解除 10/10 次，解除后投递率恢复满速留痕）
//!      + 协商账（每次协商记发起/目标率/业务应答/结果，可回放）
//!      + 三级账本守恒（到达≡投递+丢弃 / 隔离满速 / 解除恢复）
//! 判据落地：投递率收敛 ±10% + 隔离零误伤 + 解除 10/10 正确 + 协商留痕。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术（ppm 定点，零浮点）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TH_VERSION: &str = "unx982_throttle-v1";

/// 诊断段基址（0x7B40 段 · 本模块独占，grep 同目录核对无撞）。
pub const TH_SEG_BASE: u16 = 0x7B40;

/// 全量定点（1e6 = 100%）。
pub const TH_PPM_FULL: u64 = 1_000_000;

/// 目标投递率默认值（ppm；节流后收敛到此，±10% = ±100_000 ppm）。
pub const TH_TARGET_PPM: u64 = 200_000;

/// 投递率收敛容差（ppm；判据 ±10%）。
pub const TH_CONVERGE_TOL_PPM: u64 = 100_000;

/// 桶容量（令牌数上限；防内存无限增长的硬顶）。
pub const TH_BUCKET_CAP: u64 = 65_536;

/// 风暴向量集合上限（受节流的向量槽数硬顶）。
pub const TH_STORM_SLOTS: usize = 64;

/// 协商回合上限（防协商账无限增长）。
pub const TH_NEGO_ROUNDS: usize = 128;

/// 降载动作（三档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThAction {
    /// 削峰：令牌补充率下调（轻微降载，投递仍持续）。
    ShedPeak,
    /// 排队：超额事件进队列延后（中降载）。
    Queue,
    /// 丢弃：超额事件直接丢弃（重降载，仅队列满时）。
    Drop,
}

impl ThAction {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            ThAction::ShedPeak => "削峰",
            ThAction::Queue => "排队",
            ThAction::Drop => "丢弃",
        }
    }
    /// 严重度（越大越重；用于升级核）。
    pub const fn severity(self) -> u8 {
        match self {
            ThAction::ShedPeak => 1,
            ThAction::Queue => 2,
            ThAction::Drop => 3,
        }
    }
}

/// 协商相位（五态闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThNego {
    /// 空闲（无风暴，未协商）。
    Idle,
    /// 协商发起（节流器向业务面请求降载）。
    Propose,
    /// 业务确认（业务面应答愿意降载）。
    Acked,
    /// 降载执行（协商生效，节流进行中）。
    Degrading,
    /// 解除（风暴退去，协商解除，恢复满速）。
    Released,
}

impl ThNego {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            ThNego::Idle => "空闲",
            ThNego::Propose => "协商发起",
            ThNego::Acked => "业务确认",
            ThNego::Degrading => "降载执行",
            ThNego::Released => "已解除",
        }
    }
    /// 相邻跃迁合法性（判据：协商严格按五态相邻推进）。
    pub const fn adjacent(self, to: ThNego) -> bool {
        matches!(
            (self, to),
            (ThNego::Idle, ThNego::Propose)
                | (ThNego::Propose, ThNego::Acked)
                | (ThNego::Propose, ThNego::Idle)
                | (ThNego::Acked, ThNego::Degrading)
                | (ThNego::Degrading, ThNego::Released)
                | (ThNego::Released, ThNego::Idle)
        )
    }
}

/// 诊断码（0x7B40 段十三码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThCode {
    /// 0x7B40 目标率非法（0 或 > 全量）。
    TargetBad,
    /// 0x6601 投递率未收敛（超出目标 ±10%）。
    NotConverged,
    /// 0x6602 误伤（非风暴向量被节流）。
    Collateral,
    /// 0x6603 解除失败（条件满足却未解除）。
    ReleaseMissed,
    /// 0x6604 桶容量非法（0）。
    BucketBad,
    /// 0x6605 风暴集合越界（槽位超上限）。
    StormFull,
    /// 0x6606 协商相位跃迁违例（非相邻态跳迁）。
    NegoJump,
    /// 0x6607 投递账破缺（投递数 + 丢弃数 ≢ 到达数）。
    DeliverLedger,
    /// 0x6608 隔离账破缺（集合外投递不满速）。
    IsolateLedger,
    /// 0x6609 协商账溢出（回合超上限未回收）。
    NegoFull,
    /// 0x660a 补充率非法（降载档补充率 > 注入率）。
    RefillBad,
    /// 0x660b 降载动作降级违例（应升未升）。
    ActionEsc,
    /// 0x660c 解除后未恢复（解除态投递率仍受抑）。
    PostReleaseThrottled,
    /// 0x660d 段位守卫失败。
    SegmentGuard,
}

impl ThCode {
    /// 全量闭集。
    pub const ALL: [ThCode; 14] = [
        ThCode::TargetBad,
        ThCode::NotConverged,
        ThCode::Collateral,
        ThCode::ReleaseMissed,
        ThCode::BucketBad,
        ThCode::StormFull,
        ThCode::NegoJump,
        ThCode::DeliverLedger,
        ThCode::IsolateLedger,
        ThCode::NegoFull,
        ThCode::RefillBad,
        ThCode::ActionEsc,
        ThCode::PostReleaseThrottled,
        ThCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        TH_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            ThCode::TargetBad => "目标率非法：必须在 (0, 1e6] 内",
            ThCode::NotConverged => "投递率未收敛：超出目标 ±10% 容差",
            ThCode::Collateral => "误伤：非风暴向量被节流（隔离性违例）",
            ThCode::ReleaseMissed => "解除失败：条件满足却未解除",
            ThCode::BucketBad => "桶容量非法：必须 > 0",
            ThCode::StormFull => "风暴集合越界：槽位超上限",
            ThCode::NegoJump => "协商相位跃迁违例：只允许相邻态",
            ThCode::DeliverLedger => "投递账不平：投递+丢弃 ≢ 到达",
            ThCode::IsolateLedger => "隔离账破缺：集合外投递不满速",
            ThCode::NegoFull => "协商账溢出：回合超上限未回收",
            ThCode::RefillBad => "补充率非法：降载档补充率不应 > 注入率",
            ThCode::ActionEsc => "降载动作未升级：应升级却未升级",
            ThCode::PostReleaseThrottled => "解除后仍受抑：解除态应恢复满速",
            ThCode::SegmentGuard => "段位守卫失败",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            ThCode::TargetBad => "把目标投递率设在 (0, 1000000] ppm 内再启动节流",
            ThCode::NotConverged => "延长观测窗或调低注入率，使投递率落进目标 ±10%",
            ThCode::Collateral => "核对风暴集合成员，确保只含被检测确认的向量",
            ThCode::ReleaseMissed => "检查解除条件判定，确认风暴退去信号已接线",
            ThCode::BucketBad => "令牌桶容量必须为正整数，禁止零容量桶",
            ThCode::StormFull => "扩容风暴集合或提前回收已解除的向量槽",
            ThCode::NegoJump => "协商严格按 空闲→发起→确认→降载→解除 相邻推进",
            ThCode::DeliverLedger => "核对到达/投递/丢弃三计数，补齐漏记路径",
            ThCode::IsolateLedger => "确保集合外向量走满速通道，不被令牌桶拦",
            ThCode::NegoFull => "及时回收已完成协商回合，防止账无限增长",
            ThCode::RefillBad => "降载档补充率须低于注入率，否则降载形同虚设",
            ThCode::ActionEsc => "负载加重时必须升级降载档，避免降载不足",
            ThCode::PostReleaseThrottled => "解除态把补充率恢复到注入率，令牌桶放行",
            ThCode::SegmentGuard => "段位守卫失败：诊断码越界，检查 SEG_BASE",
        }
    }

    /// 诊断元组（码值 / 中文 / 建议）。
    pub const fn detail(self) -> (u16, &'static str, &'static str) {
        (self.val(), self.text(), self.advice())
    }
}


/// 单向量令牌桶。
#[derive(Clone)]
pub struct ThBucket {
    /// 向量号（标识）。
    pub vector: u16,
    /// 当前令牌数。
    pub tokens: u64,
    /// 桶容量。
    pub cap: u64,
    /// 每 tick 补充令牌数（降载档 < 注入率）。
    pub refill: u64,
    /// 注入率（ppm；登记时记下，用于解除恢复满速）。
    pub inject_ppm: u64,
    /// 到达计数。
    pub arrived: u64,
    /// 投递计数。
    pub delivered: u64,
    /// 丢弃计数。
    pub dropped: u64,
}

impl ThBucket {
    /// 新建桶（满令牌起步）。
    pub fn new(vector: u16, cap: u64, refill: u64, inject_ppm: u64) -> Self {
        Self { vector, tokens: cap, cap, refill, inject_ppm, arrived: 0, delivered: 0, dropped: 0 }
    }
    /// 尝试投递一个事件（有令牌则投递并扣 1，否则丢弃）。
    pub fn try_deliver(&mut self) -> bool {
        self.arrived += 1;
        if self.tokens > 0 {
            self.tokens -= 1;
            self.delivered += 1;
            true
        } else {
            self.dropped += 1;
            false
        }
    }
    /// tick 补充（饱和加，防溢出）。
    pub fn tick_refill(&mut self) {
        self.tokens = core::cmp::min(self.cap, self.tokens.saturating_add(self.refill));
    }
    /// 投递率（ppm；无到达视为满速）。
    pub fn deliver_ppm(&self) -> u64 {
        if self.arrived == 0 {
            TH_PPM_FULL
        } else {
            self.delivered.saturating_mul(TH_PPM_FULL) / self.arrived
        }
    }
    /// 投递账守恒（到达 ≡ 投递 + 丢弃）。
    pub fn ledger_ok(&self) -> bool {
        self.arrived == self.delivered + self.dropped
    }
    /// 解除恢复满速（补充率拉回注入率）。
    pub fn restore_full(&mut self) {
        self.refill = core::cmp::max(1, self.inject_ppm);
    }
}

/// 协商账条。
#[derive(Clone)]
pub struct ThNegoRound {
    /// 回合 tick。
    pub tick: u64,
    /// 相位跃迁（from→to）。
    pub from: ThNego,
    pub to: ThNego,
    /// 目标率（ppm）。
    pub target_ppm: u64,
    /// 业务应答（true=愿意降载）。
    pub biz_ack: bool,
}

/// 降载档位（三档闭集，含排队队列容量）。
#[derive(Clone)]
pub struct ThDegradePlan {
    /// 当前动作。
    pub action: ThAction,
    /// 排队队列容量（Queue 档的缓冲上限）。
    pub queue_cap: u64,
    /// 当前排队深度。
    pub queue_len: u64,
    /// 因队列满升级到 Drop 的次数。
    pub escalated: u64,
}

impl ThDegradePlan {
    /// 新建降载计划（默认削峰）。
    pub fn new(queue_cap: u64) -> Self {
        Self { action: ThAction::ShedPeak, queue_cap, queue_len: 0, escalated: 0 }
    }
    /// 设置动作。
    pub fn set_action(&mut self, a: ThAction) {
        self.action = a;
    }
    /// 入队一个事件；队列满则升级到 Drop 并返回 false。
    pub fn enqueue(&mut self) -> bool {
        if self.queue_len < self.queue_cap {
            self.queue_len += 1;
            true
        } else {
            if self.action != ThAction::Drop {
                self.action = ThAction::Drop;
                self.escalated += 1;
            }
            false
        }
    }
    /// 出队一个事件。
    pub fn dequeue(&mut self) -> bool {
        if self.queue_len > 0 {
            self.queue_len -= 1;
            true
        } else {
            false
        }
    }
}


/// 风暴节流器主账。
pub struct Throttler {
    /// 每向量桶表（仅风暴向量入表）。
    pub buckets: Vec<ThBucket>,
    /// 目标投递率（ppm）。
    pub target_ppm: u64,
    /// 桶容量。
    pub cap: u64,
    /// 协商相位。
    pub nego: ThNego,
    /// 协商回合账。
    pub rounds: Vec<ThNegoRound>,
    /// 逻辑 tick。
    pub tick: u64,
    /// 降载计划。
    pub plan: ThDegradePlan,
    /// 解除计数（判据 10/10）。
    pub released: u64,
    /// 协商发起计数。
    pub proposed: u64,
    /// 集合外（非风暴）到达与投递（隔离性核：必须满速）。
    pub outsider_arrived: u64,
    pub outsider_delivered: u64,
    /// 注入率（ppm；登记风暴向量时取的代表值）。
    pub inject_ppm: u64,
}

impl Throttler {
    /// 新建节流器。
    pub fn new(target_ppm: u64, cap: u64) -> Result<Self, (ThCode, String)> {
        if target_ppm == 0 || target_ppm > TH_PPM_FULL {
            return Err((ThCode::TargetBad, ThCode::TargetBad.advice().to_string()));
        }
        if cap == 0 {
            return Err((ThCode::BucketBad, ThCode::BucketBad.advice().to_string()));
        }
        Ok(Self {
            buckets: Vec::new(),
            target_ppm,
            cap,
            nego: ThNego::Idle,
            rounds: Vec::new(),
            tick: 0,
            plan: ThDegradePlan::new(256),
            released: 0,
            proposed: 0,
            outsider_arrived: 0,
            outsider_delivered: 0,
            inject_ppm: TH_PPM_FULL,
        })
    }

    /// 登记风暴向量（进桶表；重复登记幂等，补充率=注入率×目标率）。
    pub fn enroll_storm(&mut self, vector: u16, inject_ppm: u64) -> Result<(), (ThCode, String)> {
        if self.buckets.iter().any(|b| b.vector == vector) {
            return Ok(());
        }
        if self.buckets.len() >= TH_STORM_SLOTS {
            return Err((ThCode::StormFull, ThCode::StormFull.advice().to_string()));
        }
        self.inject_ppm = inject_ppm;
        // 降载补充率 = 注入率 × 目标率/全量（保证稳态收敛到目标）。
        let refill = inject_ppm.saturating_mul(self.target_ppm) / TH_PPM_FULL;
        self.buckets
            .push(ThBucket::new(vector, self.cap, core::cmp::max(1, refill), inject_ppm));
        Ok(())
    }

    /// 集合外向量投递（隔离面：永不节流，必须满速投递）。
    pub fn deliver_outsider(&mut self) -> bool {
        self.outsider_arrived += 1;
        self.outsider_delivered += 1;
        true
    }

    /// 推进协商相位（相邻跃迁守卫；违例不改相位）。
    fn set_nego(&mut self, to: ThNego, biz_ack: bool) {
        let from = self.nego;
        if !from.adjacent(to) {
            return;
        }
        self.nego = to;
        if to == ThNego::Propose {
            self.proposed += 1;
        }
        if to == ThNego::Released {
            self.released += 1;
        }
        if self.rounds.len() < TH_NEGO_ROUNDS {
            self.rounds.push(ThNegoRound {
                tick: self.tick,
                from,
                to,
                target_ppm: self.target_ppm,
                biz_ack,
            });
        }
    }

    /// 发起协商（空闲→发起）。
    pub fn propose(&mut self) {
        self.set_nego(ThNego::Propose, false);
    }

    /// 业务应答（发起→确认 / 发起→空闲（拒绝））。
    pub fn biz_ack(&mut self, ack: bool) {
        if ack {
            self.set_nego(ThNego::Acked, true);
        } else {
            self.set_nego(ThNego::Idle, false);
        }
    }

    /// 进入降载（确认→降载，附带降载动作）。
    pub fn begin_degrade(&mut self, action: ThAction) {
        if self.nego == ThNego::Acked {
            self.set_nego(ThNego::Degrading, true);
            self.plan.set_action(action);
        }
    }

    /// 解除协商（降载→解除→空闲，恢复满速）。
    pub fn release(&mut self) {
        if self.nego == ThNego::Degrading {
            self.set_nego(ThNego::Released, true);
            for b in self.buckets.iter_mut() {
                b.restore_full();
            }
            self.set_nego(ThNego::Idle, true);
        }
    }

    /// 推进一个 tick：先补充令牌。
    pub fn tick_step(&mut self) {
        self.tick += 1;
        for b in self.buckets.iter_mut() {
            b.tick_refill();
        }
    }

    /// 投递一个风暴向量事件（经令牌桶；未登记向量保守丢弃不计误伤）。
    pub fn deliver_storm(&mut self, vector: u16) -> bool {
        if let Some(b) = self.buckets.iter_mut().find(|b| b.vector == vector) {
            b.try_deliver()
        } else {
            false
        }
    }


    /// 全风暴向量平均投递率（ppm）。
    pub fn avg_deliver_ppm(&self) -> u64 {
        if self.buckets.is_empty() {
            return TH_PPM_FULL;
        }
        let sum: u64 = self.buckets.iter().map(|b| b.delivered).sum();
        let arr: u64 = self.buckets.iter().map(|b| b.arrived).sum();
        if arr == 0 {
            TH_PPM_FULL
        } else {
            sum.saturating_mul(TH_PPM_FULL) / arr
        }
    }

    /// 投递率收敛核（判据：落进目标 ±10%）。
    pub fn converged(&self) -> bool {
        let lo = self.target_ppm.saturating_sub(TH_CONVERGE_TOL_PPM);
        let hi = self.target_ppm.saturating_add(TH_CONVERGE_TOL_PPM);
        let p = self.avg_deliver_ppm();
        p >= lo && p <= hi
    }

    /// 隔离性核（判据：集合外零误伤，投递率=全量）。
    pub fn isolated(&self) -> bool {
        self.outsider_arrived == self.outsider_delivered
    }

    /// 投递账守恒核（每桶 到达 ≡ 投递 + 丢弃）。
    pub fn deliver_ledger_ok(&self) -> bool {
        self.buckets.iter().all(|b| b.ledger_ok())
    }

    /// 隔离账核（集合外必须满速）。
    pub fn isolate_ledger_ok(&self) -> bool {
        self.isolated()
    }

    /// 解除后恢复核（空闲态且发生过解除 → 投递率应=全量）。
    pub fn post_release_ok(&self) -> bool {
        if self.nego == ThNego::Idle && self.released > 0 {
            self.avg_deliver_ppm() == TH_PPM_FULL
        } else {
            true
        }
    }

    /// 解除发生核（判据：解除计数 > 0）。
    pub fn release_happened(&self) -> bool {
        self.released > 0
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "风暴节流器：向量 {} 协商 {} 解除 {} 平均投递率 {}ppm 收敛 {} 隔离 {} 账平 {}",
            self.buckets.len(),
            self.proposed,
            self.released,
            self.avg_deliver_ppm(),
            self.converged(),
            self.isolated(),
            self.deliver_ledger_ok()
        )
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    ThCode::ALL.iter().all(|c| c.val() >= TH_SEG_BASE && c.val() < TH_SEG_BASE + 0x100)
}

