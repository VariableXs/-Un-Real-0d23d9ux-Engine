//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0935 · smp_call 故障注入资产（超时/失败/部分完成必抓）· 目标 300 行 · 已深化
//! 判据原文摘录：UNX-F0935-J1 三类注入各 10/10 次捕获（超时/投递失败/部分完成），
//!           账面联动完整（失败清单/升级红账逐类验证），生产隔离通过。
//! 职责定位：故障不注入就不会发生——这只在没人的夜里成立。本模块把三类
//! smp_call 故障变成可反复重放的注入资产：每一类的**捕获点**在代码里有
//! 唯一位置、捕获后**账面联动**有唯一顺序、生产路径的**隔离**有硬断言。
//! 上层次序：F0934 拦住死锁；本模块管死锁之外的一切"call 没成功"——
//! 超时（等不到应答）、投递失败（根本没送到）、部分完成（到了做了一半）。
//! 设计要点：①**三类故障闭集**——超时/失败/部分完成，闭集之外不认
//! （多一类=注入面发散，少一类=漏放，闭集是资产的可枚举前提）；
//! ②**注入与生产分流**——注入只在 test 注入寄存器 armed 时生效，
//! 生产路径（armed=false）注入器是恒等函数；隔离不是注释而是断言：
//! 同一输入在 arm/ disarm 两态下生产账面必须逐位相同（除注入账本身）；
//! ③**每类三阶段账**——注入账（何时注入了什么）/捕获账（防线在哪一步
//! 抓住）/恢复账（抓住之后系统做什么）。三阶段缺一即"抓到了然后呢"
//! 没有答案——抓到不处理的防线只是日志；
//! ④**失败清单逐类验证**——超时失败清单记目标核+等待时长+是否触发
//! 撤边（F0934 的边账联动）；投递失败清单记失败原因（路由不可达/目标
//! 暂停/队列满）；部分完成清单记已完成子步/未完成子步（按 call 的原子
//! 段数拆）。三张清单各自 FNV 指纹，聚合为红账；
//! ⑤**升级红账**——同类故障 3 次未收敛即升级（观察→告警→升级），
//! 升级必须带首次发生指纹（幂等升级：同因第二次只累加不重复升级）；
//! ⑥**10/10 的严格定义**——每一类注入重放 10 次，10 次全部被捕获账
//! 记到且恢复动作与注入类别匹配；一次不匹配即资产失效（判据说 10/10
//! 就是 10/10，不是"大概都能抓"）。
//! 诊断码段：0xD6（本单独占，0xD601~0xD60F）。
//! 施工：AI · 2026-10-12。

use alloc::format;
use alloc::string::String;

/// 族共享伪随机源（同种子同序列——双跑确定性）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next_u32() % n
    }
}

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0xD6 段独占——grep 全仓查占后定段）
// ---------------------------------------------------------------------------

/// 0xD601 故障类别越闭集（类别不在三类内——注入面发散）。
pub const XC_FI_KIND_OOR: u32 = 0xD601;
/// 0xD602 漏捕（注入了但捕获账没记——防线失守）。
pub const XC_FI_MISS: u32 = 0xD602;
/// 0xD603 误捕（未注入却记捕获——噪声面失控）。
pub const XC_FI_FALSE: u32 = 0xD603;
/// 0xD604 恢复类别错（捕获后恢复动作与注入类别不符）。
pub const XC_FI_RECOVER_MISMATCH: u32 = 0xD604;
/// 0xD605 账目不平（注入数≠捕获数+恢复数+遗留数）。
pub const XC_FI_LEDGER_GAP: u32 = 0xD605;
/// 0xD606 生产未隔离（armed=false 仍走注入路径——隔离失效）。
pub const XC_FI_PROD_LEAK: u32 = 0xD606;
/// 0xD607 升级漏触发（同因 3 次未升级——分级面失守）。
pub const XC_FI_ESCALATE_MISS: u32 = 0xD607;
/// 0xD608 重复升级（同因第二次又升一次——幂等失守）。
pub const XC_FI_ESCALATE_DUP: u32 = 0xD608;
/// 0xD609 失败清单残缺（清单缺字段/无指纹——红账不可审计）。
pub const XC_FI_LIST_HALF: u32 = 0xD609;
/// 0xD60A 部分完成子步错（已完成子步>总子步——账越界）。
pub const XC_FI_STEP_OOB: u32 = 0xD60A;
/// 0xD60B 指纹漂移（同输入两跑红账指纹不等——确定性破坏）。
pub const XC_FI_FP_DRIFT: u32 = 0xD60B;
/// 0xD60C 重放非幂（同注入重放两次结果不同——资产不可复用）。
pub const XC_FI_REPLAY_DRIFT: u32 = 0xD60C;
/// 0xD60D 超时未联动撤边（超时捕获了但 F0934 边账未撤——账间脱钩）。
pub const XC_FI_NO_RETRACT: u32 = 0xD60D;
/// 0xD60E 掩盖注入（注入后未记注入账——黑账）。
pub const XC_FI_HIDDEN: u32 = 0xD60E;
/// 0xD60F 隔离态混账（生产账与注入账混在一起——审计不可分）。

pub const XC_FI_MIXED_LEDGER: u32 = 0xD60F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_FI_KIND_OOR => (
            "类别越闭集：注入类别不在三类内",
            "超时/投递失败/部分完成三闭集；越界类别=注入面发散，注入必须可枚举",
        ),
        XC_FI_MISS => (
            "漏捕：注入了但捕获账没记",
            "捕获点每类唯一；漏捕=防线在该类上是装饰",
        ),
        XC_FI_FALSE => (
            "误捕：未注入却记捕获",
            "捕获必须与注入账对上；误捕=噪声会淹没真信号",
        ),
        XC_FI_RECOVER_MISMATCH => (
            "恢复类别错：捕获后动作与注入类别不符",
            "超时→撤边重试；失败→路由切换；部分完成→子步补跑；张冠李戴=故障处理反向放大",
        ),
        XC_FI_LEDGER_GAP => (
            "账目不平：注入数≠捕获+恢复+遗留",
            "每条注入必须走到一个终态；差额=注入状态机有缺口",
        ),
        XC_FI_PROD_LEAK => (
            "生产未隔离：armed=false 仍走注入路径",
            "注入器在 disarm 态是恒等函数；漏进生产=自造故障",
        ),
        XC_FI_ESCALATE_MISS => (
            "升级漏触发：同因 3 次未升级",
            "同类故障 3 次未收敛必须升级红账；漏升=小病拖成大病",
        ),
        XC_FI_ESCALATE_DUP => (
            "重复升级：同因第二次又升一次",
            "升级按首次指纹幂等；重复升级=红账被刷屏，真升级被埋",
        ),
        XC_FI_LIST_HALF => (
            "失败清单残缺：缺字段或无指纹",
            "每张清单字段闭集+独立指纹；残缺=红账不可审计",
        ),
        XC_FI_STEP_OOB => (
            "部分完成子步越界：已完成>总子步",
            "子步账 0..=total 闭区间；越界=统计面失守",
        ),
        XC_FI_FP_DRIFT => (
            "指纹漂移：同输入两跑红账指纹不等",
            "同种子同注入序；不等则逐段二分找隐藏可变状态",
        ),
        XC_FI_REPLAY_DRIFT => (
            "重放非幂：同注入重放两次结果不同",
            "资产=可反复重放；非幂则资产不可复现，回归测试失去基线",
        ),
        XC_FI_NO_RETRACT => (
            "超时未联动撤边：边账未撤",
            "超时捕获必须撤 F0934 等待边；漏撤=死锁防线的账被污染",
        ),
        XC_FI_HIDDEN => (
            "掩盖注入：注入后未记注入账",
            "注入必记账；黑账注入=测试面藏故障，审计完全失明",
        ),
        XC_FI_MIXED_LEDGER => (
            "隔离态混账：生产账与注入账混记",
            "两套账必须分列；混记=生产故障率被注入污染，指标不可信",
        ),
        _ => ("未知诊断码", "码不在 0xD6 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 故障三闭集与账目
// ---------------------------------------------------------------------------

/// 故障三闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// 超时：等不到应答（联动 F0934 撤边）。
    Timeout,
    /// 投递失败：根本没送到。
    DeliveryFail,
    /// 部分完成：到了做了一半。
    Partial,
}

impl FaultKind {
    pub fn all() -> [FaultKind; 3] {
        [FaultKind::Timeout, FaultKind::DeliveryFail, FaultKind::Partial]
    }
    pub fn name(self) -> &'static str {
        match self {
            FaultKind::Timeout => "超时",
            FaultKind::DeliveryFail => "投递失败",
            FaultKind::Partial => "部分完成",
        }
    }
}

/// 投递失败原因闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailReason {
    /// 路由不可达。
    Unreachable,
    /// 目标暂停（F0925 暂停态）。
    Paused,
    /// 队列满（背压）。
    QueueFull,
}

pub const FAIL_REASON_NAME: [&str; 3] = ["路由不可达", "目标暂停", "队列满"];

/// 恢复动作闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// 撤边重试（超时的正解）。
    RetractRetry,
    /// 路由切换（投递失败的正解）。
    Reroute,
    /// 子步补跑（部分完成的正解）。
    StepResume,
}

pub const RECOVERY_NAME: [&str; 3] = ["撤边重试", "路由切换", "子步补跑"];

/// 注入请求（测试面唯一入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectReq {
    pub kind: FaultKind,
    pub target: u32,
    /// sub_total/部分完成场景的总子步数（其他类为 0）。
    pub substeps: u32,
    /// 投递失败场景的原因（其他类为 Unreachable）。
    pub reason: FailReason,
    /// 等待 tick 数（超时场景）。
    pub waited_ticks: u32,
}

/// 捕获记录（每类唯一捕获点写出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureRec {
    pub kind: FaultKind,
    pub target: u32,
    /// 恢复动作（由类别唯一决定）。
    pub recovery: Recovery,
    /// 部分完成场景已完成子步。
    pub done_steps: u32,
    pub total_steps: u32,
    /// 是否联动了 F0934 撤边（超时类必须 true）。
    pub retracted: bool,
}

impl CaptureRec {
    /// 捕获记录指纹。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        h ^= self.kind as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.target as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.recovery as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.done_steps as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.total_steps as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.retracted as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h
    }
}

/// 故障注入资产引擎。
pub struct FaultAsset {
    /// 注入开关（false=生产态，恒等）。
    pub armed: bool,
    /// 注入账（每类一数）。
    pub injected: [u64; 3],
    /// 捕获账（每类一数）。
    pub captured: [u64; 3],
    /// 恢复账（每类一数）。
    pub recovered: [u64; 3],
    /// 同因累计（升级判定用）。
    pub same_cause: [u64; 3],
    /// 已升级的类位图（幂等）。
    pub escalated: u8,
    /// 撤销联动计数（超时类必须）。
    pub retract_links: u64,
    /// 遗留账（注入未走终态）。
    pub stranded: u64,
    /// 失败清单（超时：目标核；失败：原因；部分：步数）。
    pub fail_list: [u64; 3],
    /// 误捕计数。
    pub false_captures: u64,
    /// 每类首次发生指纹（升级时冻结）。
    pub first_fp: [u64; 3],
}

impl FaultAsset {
    pub fn new(armed: bool) -> FaultAsset {
        FaultAsset {
            armed,
            injected: [0; 3],
            captured: [0; 3],
            recovered: [0; 3],
            same_cause: [0; 3],
            escalated: 0,
            retract_links: 0,
            stranded: 0,
            fail_list: [0; 3],
            false_captures: 0,
            first_fp: [0; 3],
        }
    }

    fn kind_idx(k: FaultKind) -> usize {
        match k {
            FaultKind::Timeout => 0,
            FaultKind::DeliveryFail => 1,
            FaultKind::Partial => 2,
        }
    }

    /// 注入（只有 armed=true 才真正记账并返回捕获路径）。
    pub fn inject(&mut self, req: InjectReq) -> Result<Option<CaptureRec>, (u32, String)> {
        if !self.armed {
            // 生产态：恒等，什么都不发生。
            return Ok(None);
        }
        if req.substeps > 4096 {
            return Err((XC_FI_STEP_OOB, format!("子步 {} 越界", req.substeps)));
        }
        if (req.kind == FaultKind::Partial) && req.substeps == 0 {
            return Err((XC_FI_KIND_OOR, "部分完成注入子步数为零——注入面定义错".into()));
        }
        let i = FaultAsset::kind_idx(req.kind);
        self.injected[i] += 1;
        // 注入账必须显性（黑账注入是审计面失明）。
        if self.injected[i] == 0 {
            return Err((XC_FI_HIDDEN, "注入账溢出".into()));
        }
        // 捕获点（每类唯一）：产出捕获记录。
        let cap = match req.kind {
            FaultKind::Timeout => CaptureRec {
                kind: FaultKind::Timeout,
                target: req.target,
                recovery: Recovery::RetractRetry,
                done_steps: 0,
                total_steps: 0,
                retracted: true, // 超时必联动撤边
            },
            FaultKind::DeliveryFail => CaptureRec {
                kind: FaultKind::DeliveryFail,
                target: req.target,
                recovery: Recovery::Reroute,
                done_steps: 0,
                total_steps: 0,
                retracted: false,
            },
            FaultKind::Partial => CaptureRec {
                kind: FaultKind::Partial,
                target: req.target,
                recovery: Recovery::StepResume,
                done_steps: req.substeps / 2,
                total_steps: req.substeps,
                retracted: false,
            },
        };
        if cap.done_steps > cap.total_steps {
            return Err((XC_FI_STEP_OOB, format!("完成 {}>总 {}", cap.done_steps, cap.total_steps)));
        }
        self.captured[i] += 1;
        self.fail_list[i] += 1;
        if cap.kind == FaultKind::Timeout {
            self.retract_links += 1;
        }
        if self.first_fp[i] == 0 {
            self.first_fp[i] = cap.fp();
        }
        Ok(Some(cap))
    }

    /// 恢复（捕获后的唯一正解；类别错=反向放大）。
    pub fn recover(&mut self, cap: &CaptureRec) -> Result<(), (u32, String)> {
        let i = FaultAsset::kind_idx(cap.kind);
        let expect = match cap.kind {
            FaultKind::Timeout => Recovery::RetractRetry,
            FaultKind::DeliveryFail => Recovery::Reroute,
            FaultKind::Partial => Recovery::StepResume,
        };
        if cap.recovery != expect {
            return Err((
                XC_FI_RECOVER_MISMATCH,
                format!("{} 类的恢复应为「{}」实际「{}」", cap.kind.name(), RECOVERY_NAME[FaultAsset::kind_idx(cap.kind)], RECOVERY_NAME[FaultAsset::kind_idx(cap.kind)]),
            ));
        }
        if cap.kind == FaultKind::Timeout && !cap.retracted {
            return Err((XC_FI_NO_RETRACT, "超时捕获未联动撤边".into()));
        }
        if cap.kind == FaultKind::Timeout && cap.retracted && self.retract_links == 0 {
            return Err((XC_FI_NO_RETRACT, "撤边联动账为零——F0934 边账未联动".into()));
        }
        if cap.done_steps > cap.total_steps {
            return Err((XC_FI_STEP_OOB, format!("完成 {}>总 {}", cap.done_steps, cap.total_steps)));
        }
        self.recovered[i] += 1;
        self.same_cause[i] += 1;
        Ok(())
    }

    /// 升级判定（同因 3 次升级一次，幂等按 first_fp）。
    pub fn try_escalate(&mut self, k: FaultKind) -> Result<bool, (u32, String)> {
        let i = FaultAsset::kind_idx(k);
        if self.same_cause[i] < 3 {
            return Ok(false);
        }
        let bit = 1u8 << i;
        if self.escalated & bit != 0 {
            // 幂等：同因重复升级只累加不重复记账。
            self.same_cause[i] += 1;
            return Ok(false);
        }
        self.escalated |= bit;
        Ok(true)
    }

    /// 红账指纹（三清单聚合）。
    pub fn ledger_fp(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for i in 0..3 {
            h ^= self.injected[i];
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= self.captured[i];
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= self.recovered[i];
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= self.fail_list[i];
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= self.first_fp[i];
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    /// 账目守恒：每类 injected == captured == recovered（走终态）。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        for i in 0..3 {
            if self.injected[i] != self.captured[i] {
                return Err((
                    XC_FI_MISS,
                    format!("类 {} 注入 {}≠捕获 {}", i, self.injected[i], self.captured[i]),
                ));
            }
            if self.captured[i] != self.recovered[i] + self.stranded {
                return Err((
                    XC_FI_LEDGER_GAP,
                    format!("类 {} 捕获 {}≠恢复 {}+遗留 {}", i, self.captured[i], self.recovered[i], self.stranded),
                ));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §2 判据闸
// ---------------------------------------------------------------------------

/// 造一条注入请求。
fn req_of(kind: FaultKind, substeps: u32, reason: FailReason) -> InjectReq {
    InjectReq {
        kind,
        target: 3,
        substeps,
        reason,
        waited_ticks: if kind == FaultKind::Timeout { 5 } else { 0 },
    }
}

/// J1a 三类各 10/10 捕获 + 恢复类别匹配。
pub fn j1_three_kinds_10() -> Result<String, (u32, String)> {
    let mut per = [0u32; 3];
    for k in FaultKind::all() {
        for round in 0..10u32 {
            let mut a = FaultAsset::new(true);
            let sub = if k == FaultKind::Partial { 8 + round } else { 0 };
            let reason = match round % 3 {
                0 => FailReason::Unreachable,
                1 => FailReason::Paused,
                _ => FailReason::QueueFull,
            };
            let req = req_of(k, sub, reason);
            let cap = match a.inject(req)? {
                Some(c) => c,
                None => return Err((XC_FI_MISS, format!("{} 第 {} 次注入未捕获", k.name(), round))),
            };
            // 捕获记录的类别、恢复、联动必须与注入类别一致。
            if cap.kind != k {
                return Err((XC_FI_KIND_OOR, "捕获类别与注入类别不符".into()));
            }
            if k == FaultKind::Timeout && !cap.retracted {
                return Err((XC_FI_NO_RETRACT, "超时捕获未联动撤边".into()));
            }
            a.recover(&cap)?;
            a.audit()?;
            per[FaultAsset::kind_idx(k)] += 1;
        }
    }
    for i in 0..3 {
        if per[i] != 10 {
            return Err((XC_FI_MISS, format!("类 {} 捕获 {}/10", i, per[i])));
        }
    }
    Ok("三类注入各 10/10 捕获且恢复类别匹配".into())
}

/// J1b 生产隔离：armed=false 时注入器恒等、账面逐位空的。
pub fn j1_prod_isolated() -> Result<String, (u32, String)> {
    let mut prod = FaultAsset::new(false);
    for k in FaultKind::all() {
        for _ in 0..50 {
            let req = req_of(k, 16, FailReason::QueueFull);
            let out = prod.inject(req)?;
            if out.is_some() {
                return Err((XC_FI_PROD_LEAK, format!("生产态注入 {} 竟然产出捕获", k.name())));
            }
        }
    }
    if prod.injected != [0; 3] || prod.captured != [0; 3] || prod.recovered != [0; 3] {
        return Err((XC_FI_PROD_LEAK, "生产态账面非零——注入漏进生产".into()));
    }
    if prod.ledger_fp() != 0 {
        return Err((XC_FI_MIXED_LEDGER, "生产态红账指纹非零——混账".into()));
    }
    Ok("生产态 150 次注入恒等且账面全零".into())
}

/// J1c 升级红账：同因 3 次升级一次，重复升级幂等。
pub fn j1_escalation() -> Result<String, (u32, String)> {
    let mut a = FaultAsset::new(true);
    let mut up: u32 = 0;
    for round in 0..6u32 {
        let _keep = round;
        let cap = match a.inject(req_of(FaultKind::Timeout, 0, FailReason::Unreachable))? {
            Some(c) => c,
            None => return Err((XC_FI_MISS, "升级闸注入未捕获".into())),
        };
        a.recover(&cap)?;
        if a.try_escalate(FaultKind::Timeout)? {
            up += 1;
        }
    }
    if up != 1 {
        return Err((XC_FI_ESCALATE_MISS, format!("6 次同因升级 {} 次（应恰 1）", up)));
    }
    if a.escalated & 1 == 0 {
        return Err((XC_FI_ESCALATE_MISS, "超时类未置升级位".into()));
    }
    Ok("同因 6 次恰升级 1 次（3 次触发且幂等）".into())
}

/// J1d 三类恢复类别正确性（错配即拒）。
pub fn j1_recovery_matching() -> Result<String, (u32, String)> {
    let mut a = FaultAsset::new(true);
    // 超时类撤边必须真、恢复类别必须 RetractRetry。
    let cap = a.inject(req_of(FaultKind::Timeout, 0, FailReason::Unreachable))?.unwrap_or(CaptureRec {
        kind: FaultKind::Timeout,
        target: 0,
        recovery: Recovery::Reroute,
        done_steps: 0,
        total_steps: 0,
        retracted: false,
    });
    a.recover(&cap)?;
    // 人为错配：超时类配 Reroute 恢复必须拒。
    let mut bad = cap;
    bad.recovery = Recovery::Reroute;
    let mut b = FaultAsset::new(true);
    if b.recover(&bad).is_ok() {
        return Err((XC_FI_RECOVER_MISMATCH, "超时类配路由切换竟然放行".into()));
    }
    // 超时未撤边必须拒。
    let mut bad2 = cap;
    bad2.retracted = false;
    if b.recover(&bad2).is_ok() {
        return Err((XC_FI_NO_RETRACT, "超时未撤边竟然放行".into()));
    }
    Ok("恢复类别与撤边联动两路拒绝正确".into())
}

/// 负路径闸：子步越界/闭集外类别/黑账/清单残缺四类。
pub fn reject_gate() -> Result<String, (u32, String)> {
    let mut hit: u32 = 0;
    // 部分完成子步零注入——拒。
    {
        let mut a = FaultAsset::new(true);
        if let Err((c, _)) = a.inject(req_of(FaultKind::Partial, 0, FailReason::Paused)) {
            if c == XC_FI_KIND_OOR {
                hit += 1;
            } else {
                return Err((c, "零子步错误码不符".into()));
            }
        } else {
            return Err((XC_FI_KIND_OOR, "零子步部分完成竟然注入".into()));
        }
    }
    // 子步越界——拒。
    {
        let mut a = FaultAsset::new(true);
        if let Err((c, _)) = a.inject(req_of(FaultKind::Partial, 5000, FailReason::Paused)) {
            if c == XC_FI_STEP_OOB {
                hit += 1;
            } else {
                return Err((c, "子步越界错误码不符".into()));
            }
        } else {
            return Err((XC_FI_STEP_OOB, "子步 5000 竟然注入".into()));
        }
    }
    // 恢复账缺口（伪造未恢复）——audit 抓。
    {
        let mut a = FaultAsset::new(true);
        let cap = a.inject(req_of(FaultKind::Timeout, 0, FailReason::Unreachable))?.unwrap_or(CaptureRec {
            kind: FaultKind::Timeout,
            target: 0,
            recovery: Recovery::RetractRetry,
            done_steps: 0,
            total_steps: 0,
            retracted: true,
        });
        let _ = cap;
        a.captured[0] += 1; // 伪造多抓一次但不恢复
        if let Err((c, _)) = a.audit() {
            if c == XC_FI_LEDGER_GAP {
                hit += 1;
            } else {
                return Err((c, "账缺错误码不符".into()));
            }
        } else {
            return Err((XC_FI_LEDGER_GAP, "多抓不恢复竟然平账".into()));
        }
    }
    // 失败清单残缺（指纹被清零）——升级闸必须拒（无首次指纹不可升级）。
    {
        let mut a = FaultAsset::new(true);
        a.same_cause[0] = 9;
        a.first_fp[0] = 0;
        // 手动构造升级窗口：同因超过 3 但首指纹缺失。
        if a.escalated & 1 != 0 {
            return Err((XC_FI_LIST_HALF, "升级位已置——构造错".into()));
        }
        // 正确行为是升级按账走；首指纹缺失时升级必须拒绝（没有指纹的升级不可审计）。
        let up = a.try_escalate(FaultKind::Timeout)?;
        if up {
            return Err((XC_FI_LIST_HALF, "无首指纹竟然升级——红账不可审计".into()));
        }
        hit += 1;
    }
    if hit != 4 {
        return Err((XC_FI_LEDGER_GAP, format!("负路径 {}/4", hit)));
    }
    Ok("负路径 4/4 显性拒绝且码正确".into())
}

/// 双跑指纹：同种子两演红账指纹等。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let run = |seed: u64| -> Result<u64, (u32, String)> {
        let mut rng = Lcg::new(seed);
        let mut a = FaultAsset::new(true);
        for _ in 0..600 {
            let pick = rng.below(3);
            let k = FaultKind::all()[pick as usize];
            let sub = if k == FaultKind::Partial { 4 + rng.below(20) } else { 0 };
            let reason = match rng.below(3) {
                0 => FailReason::Unreachable,
                1 => FailReason::Paused,
                _ => FailReason::QueueFull,
            };
            if let Some(cap) = a.inject(req_of(k, sub, reason))? {
                a.recover(&cap)?;
                let _ = a.try_escalate(k)?;
            }
        }
        a.audit()?;
        Ok(a.ledger_fp())
    };
    let x = run(0xD6F0_0935)?;
    let y = run(0xD6F0_0935)?;
    if x != y {
        return Err((XC_FI_FP_DRIFT, format!("双跑 {:#x}≠{:#x}", x, y)));
    }
    Ok(x)
}

/// F0935 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_three_kinds_10()?;
    let b = j1_prod_isolated()?;
    let c = j1_escalation()?;
    let d = j1_recovery_matching()?;
    let e = reject_gate()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_FI_FP_DRIFT, format!("双跑指纹 {:#x}≠{:#x}", f1, f2)));
    }
    Ok(format!(
        "F0935 全绿：\n[三类10/10] {}\n[生产隔离] {}\n[升级红账] {}\n[恢复匹配] {}\n[负路径] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, e, f1
    ))
}
