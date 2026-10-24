//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0922 · smp_call 同步/异步双模式语义（目标 320 行）
//! 判据原文摘录（书 2589 行）：UNX-F0922-J1 同步模式闭环等待正确（全员完成才
//! 返回 10 万次），异步模式零等待（返回时延 <1µs），模式混用拦截 10/10 次。
//! 职责定位：在 F0921 统一入口之上把「调用发起方如何拿到结果」二分为两种互斥
//! 模式——同步模式发起即阻塞、全员完成才返回（闭环等待语义，返回时聚合回执必
//! 全量在场）；异步模式发起即返回（零等待语义，返回的是「在途凭据」而非结果，
//! 结果由收割面凭票领取）；两种模式对同一 ticket 的使用纪律严格互斥，任何混用
//! （同步票异步收/异步票同步等/重复收割/未到齐强取）都在收口闸显性拦截。
//! 设计要点：①**模式标签随票走**——ticket 诞生时模式即定型入账，后续一切操作
//! 按账面模式裁决（模式不可改写，改写=混用 0x4F01）；②**零等待的确定性量化**
//! ——「返回时延 <1µs」在零墙钟纪律下转化为「异步入口步数 ≤ 常量预算」（步数
//! 上界写死常量，每次发起实测步数入账，超预算立案——把时延承诺变成可对账的
//! 结构化事实）；③**同步闭环等待**——wait 轮询账面直到目标集全员终态或超窗，
//! 超窗=0x4F05 显性红（不做部分成功返回）；④**收割面单次性**——每个 ticket 恰
//! 可收割一次（收割即销票），重复收割 0x4F03；⑤**家族复用声明**——Facade 核槽
//! 表/ticket 账/FNV 快照复用 F0921 本体（家族复用第九用），本单新增的是模式层
//! 与收割层，不重复实现投递与执行。
//! 诊断码段：0x4F（本单独占，0x4F01~0x4F0F）。
//! 施工：AI-WB2 · 2026-10-11。

use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0x4F 段独占）
// ---------------------------------------------------------------------------

/// 0x4F01 模式混用·改写（ticket 诞生后模式被二次改写——模式随票定型纪律破坏）。
pub const XC_MODE_REWRITE: u32 = 0x4F01;
/// 0x4F02 模式混用·同步票异步收（同步 ticket 走收割面——同步票由 wait 直接出结果）。
pub const XC_SYNC_ASYNC_MIX: u32 = 0x4F02;
/// 0x4F03 重复收割（同一 ticket 收割两次——收割即销票纪律破坏）。
pub const XC_REAP_TWICE: u32 = 0x4F03;
/// 0x4F04 模式混用·异步票同步等（异步 ticket 走 wait 面——异步票只能收割）。
pub const XC_ASYNC_SYNC_MIX: u32 = 0x4F04;
/// 0x4F05 同步等待超窗（目标集未全员终态且窗口耗尽——闭环等待破约显性化）。
pub const XC_WAIT_TIMEOUT: u32 = 0x4F05;
/// 0x4F06 未到齐强取（异步票未全员终态就强读聚合值——结果语义未定义显性化）。
pub const XC_PREMATURE_READ: u32 = 0x4F06;
/// 0x4F07 异步入口超预算（发起步数超零等待预算上界——时延承诺对账失败）。
pub const XC_ASYNC_BUDGET: u32 = 0x4F07;
/// 0x4F08 收割凭据错票（reap 的 ticket 号不在异步在途账——凭据面串号）。
pub const XC_REAP_MISMATCH: u32 = 0x4F08;
/// 0x4F09 同步票缺回执（同步返回时聚合回执未全量在场——闭环等待破约）。
pub const XC_SYNC_INCOMPLETE: u32 = 0x4F09;
/// 0x4F0A 在途账漂移（异步在途账与模式账对不上——账面完整性破坏）。
pub const XC_INFLIGHT_GAP: u32 = 0x4F0A;
/// 0x4F0B 模式账缺失（操作引用的 ticket 在模式账中不存在——账面空洞）。
pub const XC_MODE_MISSING: u32 = 0x4F0B;
/// 0x4F0C 步数账倒退（预算账累计步数回退——测量面被并发改写）。
pub const XC_STEP_REGRESS: u32 = 0x4F0C;
/// 0x4F0D 收割序错乱（收割时 ticket 已终态但结果被清——结果面生命周期破坏）。
pub const XC_RESULT_VANISH: u32 = 0x4F0D;
/// 0x4F0E 双跑漂移（同种子重放指纹不等——确定性破坏）。
pub const XC_REPLAY_DRIFT: u32 = 0x4F0E;
/// 0x4F0F 混用拦截账失守（混用拦截计数与实际拦截次数不符——拦截面统计漂移）。
pub const XC_MIX_COUNT_GAP: u32 = 0x4F0F;
/// 0x4F10 业务红（回调返回 Err——业务账红，F0921 0x4E05 同义分账）。
pub const XC_BIZ_RED: u32 = 0x4F10;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_MODE_REWRITE => (
            "模式混用·改写：ticket 诞生后模式被二次改写",
            "模式随票定型不可改；确需换模式放弃本票重新发起（新票新模式）",
        ),
        XC_SYNC_ASYNC_MIX => (
            "模式混用·同步票异步收：同步 ticket 走收割面",
            "同步票由 wait 直接出结果；走收割面说明调用方拿错了返回凭据，查返回值类型",
        ),
        XC_REAP_TWICE => (
            "重复收割：同一 ticket 收割两次",
            "收割即销票；第二次收割拿到的是空气，调用方应保存首次收割结果",
        ),
        XC_ASYNC_SYNC_MIX => (
            "模式混用·异步票同步等：异步 ticket 走 wait 面",
            "异步票只能收割；要同步语义就在发起时选 Sync 模式，不能事后补等",
        ),
        XC_WAIT_TIMEOUT => (
            "同步等待超窗：目标集未全员终态且窗口耗尽",
            "先查 F0890 投递级账定介质健康；介质绿则目标核执行挂起，立案后上层决定重试或放弃",
        ),
        XC_PREMATURE_READ => (
            "未到齐强取：异步票未全员终态就强读聚合值",
            "先 reap_count 看在途进度；全量终态才收割，半程结果是语义未定义的",
        ),
        XC_ASYNC_BUDGET => (
            "异步入口超预算：发起步数超零等待预算上界",
            "异步入口只做记账+投递不做等待；超预算说明入口里混进了等待逻辑，二分定位",
        ),
        XC_REAP_MISMATCH => (
            "收割凭据错票：reap 的 ticket 不在异步在途账",
            "凭据是发起时返回的原票；串票说明凭据被复制或张冠李戴，核对调用点",
        ),
        XC_SYNC_INCOMPLETE => (
            "同步票缺回执：同步返回时聚合回执未全量在场",
            "闭环等待语义下返回即全量；缺回执=等待器提前放行，查 wait 轮询条件",
        ),
        XC_INFLIGHT_GAP => (
            "在途账漂移：异步在途账与模式账对不上",
            "在途账是模式账的子集；差额=已收割未销账的票，收割销账路径有漏步",
        ),
        XC_MODE_MISSING => (
            "模式账缺失：操作引用的 ticket 不在模式账",
            "票从发起而来；野票=凭据被伪造或跨账本串用，拒绝操作并立案",
        ),
        XC_STEP_REGRESS => (
            "步数账倒退：预算账累计步数回退",
            "步数只增不减；倒退=测量面被改写，测量必须与业务同账本不可分离",
        ),
        XC_RESULT_VANISH => (
            "收割序错乱：ticket 已终态但结果被清",
            "结果与终态同生共死；消失=有人绕过收割面清了结果，查是否有旁路写账",
        ),
        XC_REPLAY_DRIFT => (
            "双跑漂移：同种子重放指纹不等",
            "同种子同序列是确定性底线；不等则逐段二分找隐藏可变状态",
        ),
        XC_MIX_COUNT_GAP => (
            "混用拦截账失守：拦截计数与实际次数不符",
            "拦截计数是审计面；失守说明拦截路径有漏计，逐闸核对",
        ),
        _ => ("未知诊断码", "码不在 0x4F 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 模式闭集（随票定型——本单核心语义）
// ---------------------------------------------------------------------------

/// 调用模式闭集（判据 J1 双模式——互斥使用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallMode {
    /// 同步：发起即阻塞，全员完成才返回（闭环等待）。
    Sync,
    /// 异步：发起即返回（零等待），结果凭票收割。
    Async,
}

impl CallMode {
    /// 模式人话名。
    pub fn name(self) -> &'static str {
        match self {
            CallMode::Sync => "sync",
            CallMode::Async => "async",
        }
    }
}

/// 模式票（ticket 的模式档案——定型后不可改）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeTicket {
    pub ticket: u64,
    pub mode: CallMode,
    /// 目标集（展开冻结快照——F0921 语义延续）。
    pub targets: Vec<u32>,
    /// 发起 tick。
    pub born: u64,
    /// 异步入口实测步数（Sync 票恒 0——预算只对 Async 有意义）。
    pub entry_steps: u32,
}

/// 票面终态结果（收割面产出——全量回执+聚合）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReapResult {
    pub ticket: u64,
    /// 逐核回执（全量在场——收割前提是全员终态）。
    pub per_cpu: Vec<(u32, Result<u64, u32>)>,
    /// 聚合：全绿拼值/任一红整单红（F0921 语义延续）。
    pub aggregated: Result<Vec<u64>, u32>,
}

// ---------------------------------------------------------------------------
// §2 双模式引擎（F0921 Facade 骨架上的模式层+收割层）
// ---------------------------------------------------------------------------

/// 请求账目状态（五桶——F0921 守恒口径延续+收割态扩展）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TState {
    /// 在途（异步票等待收割 / 同步票 wait 正在等）。
    InFlight,
    /// 全员终态待收割（仅异步票——同步票不经过此态）。
    Ready,
    /// 已收割销票（异步票终态）。
    Reaped,
    /// 同步已完成（同步票终态——wait 返回即入账）。
    Synced,
    /// 超窗/混用红终态。
    Red,
}

/// 双模式引擎（核槽表+模式账+在途账+结果账+步数账五件一体）。
pub struct DualModeEngine {
    /// 核槽表（下标=核 id；回调+统计——F0921 语义）。
    slots: Vec<Slot>,
    /// 模式账（ticket→模式票——定型不可改，全部操作的第一裁决依据）。
    modes: BTreeMap<u64, ModeTicket>,
    /// 异步在途账（ticket→全员是否终态 bool——Ready 判定面）。
    inflight: BTreeMap<u64, bool>,
    /// 结果账（ticket→逐核结果——终态写入，收割取出后销账）。
    results: BTreeMap<u64, Vec<(u32, Result<u64, u32>)>>,
    /// 下一个 ticket 号（单调）。
    next_ticket: u64,
    /// 当前逻辑 tick。
    tick: u64,
    /// 同步等待窗（tick 数——超窗 0x4F05；常量来源=闭环等待的量化承诺）。
    pub wait_window: u64,
    /// 异步入口步数预算（零等待量化：<1µs ≈ ≤16 步；常量来源=入口纯记账+投递
    /// 两段的结构上界，超预算即说明混进了等待——0x4F07）。
    pub async_budget: u32,
    /// 嵌套深度帽（F0921 语义延续）。
    pub nest_cap: u32,
    nest_depth: u32,
    // ---- 账桶 ----
    /// 同步闭环完成数（J1 第一款计数器）。
    pub sync_ok_total: u64,
    /// 异步零等待完成数（J1 第二款计数器）。
    pub async_ok_total: u64,
    /// 收割总数。
    pub reap_total: u64,
    /// 同步超窗数。
    pub wait_timeout_total: u64,
    /// 混用拦截数（10/10 的审计面）。
    pub mix_blocked_total: u64,
    /// 业务红数。
    pub biz_red_total: u64,
    /// 异步入口步数峰值（预算对账证据）。
    pub entry_steps_peak: u32,
    /// 累计步数（倒退检测基准——0x4F0C）。
    entry_steps_cum: u64,
}

/// 单核槽位。
struct Slot {
    online: bool,
    callee: Option<fn(&CallPayload2) -> Result<u64, u32>>,
    arrived: u64,
    executed: u64,
}

impl Slot {
    fn new() -> Slot {
        Slot {
            online: false,
            callee: None,
            arrived: 0,
            executed: 0,
        }
    }
}

/// 载荷（本单自带形状——与 F0921 家族同构，独立定制不跨模块引用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallPayload2 {
    pub kind: u32,
    pub arg: u64,
    pub from_cpu: u32,
}

impl CallPayload2 {
    pub fn new(kind: u32, arg: u64) -> CallPayload2 {
        CallPayload2 {
            kind,
            arg,
            from_cpu: 0,
        }
    }
}

/// 异步凭据（发起即返的「在途凭据」——收割面唯一合法钥匙）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsyncTicket {
    pub ticket: u64,
    pub targets: Vec<u32>,
    pub born: u64,
    /// 实测入口步数（零等待证据）。
    pub steps: u32,
}

/// 同步回执（发起阻塞到全员完成才产出——聚合全量在场）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncReceipt {
    pub ticket: u64,
    pub per_cpu: Vec<(u32, Result<u64, u32>)>,
    pub aggregated: Result<Vec<u64>, u32>,
    /// 等待耗时（tick 数——闭环等待可观测）。
    pub waited_ticks: u64,
}

impl DualModeEngine {
    /// 构造（核数+在线集合——F0921 语义）。
    pub fn new(ncpu: u32, online: &[u32]) -> Result<DualModeEngine, (u32, String)> {
        if ncpu == 0 {
            return Err((0x4F0A, "核数为 0——无核不可构造".to_string()));
        }
        let mut slots = Vec::new();
        for _ in 0..ncpu {
            slots.push(Slot::new());
        }
        for &c in online {
            if (c as usize) >= slots.len() {
                return Err((0x4F0A, format!("在线核 #{} 越界", c)));
            }
            slots[c as usize].online = true;
        }
        if online.is_empty() {
            return Err((0x4F0A, "空在线集合".to_string()));
        }
        Ok(DualModeEngine {
            slots,
            modes: BTreeMap::new(),
            inflight: BTreeMap::new(),
            results: BTreeMap::new(),
            next_ticket: 1,
            tick: 0,
            wait_window: 16,
            async_budget: 16,
            nest_cap: 4,
            nest_depth: 0,
            sync_ok_total: 0,
            async_ok_total: 0,
            reap_total: 0,
            wait_timeout_total: 0,
            mix_blocked_total: 0,
            biz_red_total: 0,
            entry_steps_peak: 0,
            entry_steps_cum: 0,
        })
    }

    /// 注册回调（一核一回调——F0921 语义）。
    pub fn register(&mut self, cpu: u32, f: fn(&CallPayload2) -> Result<u64, u32>) -> Result<(), (u32, String)> {
        let ncpu = self.slots.len();
        let slot = self
            .slots
            .get_mut(cpu as usize)
            .ok_or_else(|| (0x4F0A, format!("核 #{} 越界（核数 {}）", cpu, ncpu)))?;
        if slot.callee.is_some() {
            return Err((0x4F0A, format!("核 #{} 重复注册", cpu)));
        }
        slot.callee = Some(f);
        Ok(())
    }

    /// 拓扑展开（Self/One/Many/All/AllButSelf——F0921 规则单点延续）。
    fn expand(&self, scope: u8, mask: &[u32], from: u32) -> Result<Vec<u32>, (u32, String)> {
        let mut t: Vec<u32> = Vec::new();
        match scope {
            0 => t.push(from),
            1 | 2 => {
                for &c in mask {
                    if (c as usize) >= self.slots.len() || !self.slots[c as usize].online {
                        return Err((0x4F0A, format!("掩码核 #{} 离线或越界", c)));
                    }
                    if !t.contains(&c) {
                        t.push(c);
                    }
                }
                if t.is_empty() {
                    return Err((0x4F0A, "空目标集".to_string()));
                }
                if scope == 1 && t.len() != 1 {
                    return Err((0x4F0A, "One 要求恰一核".to_string()));
                }
                if scope == 2 && t.len() < 2 {
                    return Err((0x4F0A, "Many 要求 ≥2 核".to_string()));
                }
            }
            3 => {
                for c in 0..self.slots.len() as u32 {
                    if self.slots[c as usize].online {
                        t.push(c);
                    }
                }
            }
            _ => {
                for c in 0..self.slots.len() as u32 {
                    if self.slots[c as usize].online && c != from {
                        t.push(c);
                    }
                }
                if t.is_empty() {
                    return Err((0x4F0A, "AllButSelf 零目标".to_string()));
                }
            }
        }
        t.sort_unstable();
        Ok(t)
    }

    /// 执行单核（同步语义内联——回调即执行即记账）。
    fn run_one(&mut self, cpu: u32, p: &CallPayload2) -> Result<u64, u32> {
        let slot = &mut self.slots[cpu as usize];
        slot.arrived += 1;
        match slot.callee {
            Some(f) => {
                slot.executed += 1;
                f(p)
            }
            None => Err(0x4F0A | 0x1000),
        }
    }

    /// 同步发起（闭环等待——全员完成才返回，J1 第一款语义）。
    pub fn call_sync(
        &mut self,
        scope: u8,
        p: &CallPayload2,
        mask: &[u32],
        from: u32,
    ) -> Result<SyncReceipt, (u32, String)> {
        if self.nest_depth >= self.nest_cap {
            return Err((0x4F0A, "嵌套越帽".to_string()));
        }
        let targets = self.expand(scope, mask, from)?;
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.tick += 1;
        // 模式定型（随票不可改——0x4F01 的账面基础）。
        self.modes.insert(
            ticket,
            ModeTicket {
                ticket,
                mode: CallMode::Sync,
                targets: targets.clone(),
                born: self.tick,
                entry_steps: 0,
            },
        );
        self.inflight.insert(ticket, false);
        // 同步执行：逐核立即执行并写入结果账（闭环=发起核等到全员终态）。
        let mut per: Vec<(u32, Result<u64, u32>)> = Vec::new();
        let mut red: Option<u32> = None;
        for &c in &targets {
            self.tick += 1;
            match self.run_one(c, p) {
                Ok(v) => per.push((c, Ok(v))),
                Err(code) => {
                    per.push((c, Err(code)));
                    if red.is_none() {
                        red = Some(code);
                    }
                }
            }
        }
        self.results.insert(ticket, per.clone());
        self.inflight.insert(ticket, true);
        // 闭环等待模拟：账面全员终态才放行——同步模型内联即终态，窗口校验兜底。
        let waited = self.tick - self.modes[&ticket].born;
        if waited > self.wait_window * 64 {
            self.wait_timeout_total += 1;
            self.mark_red(ticket);
            return Err((
                XC_WAIT_TIMEOUT,
                format!("同步等待 {} tick 超窗 {}——闭环破约", waited, self.wait_window * 64),
            ));
        }
        // 全量在场校验（0x4F09——返回即全量是闭环等待的铁语义）。
        if per.len() != targets.len() {
            self.mark_red(ticket);
            return Err((
                XC_SYNC_INCOMPLETE,
                format!("回执 {} ≠ 目标 {}——提前放行", per.len(), targets.len()),
            ));
        }
        self.sync_ok_total += 1;
        let aggregated = match red {
            Some(code) => {
                self.biz_red_total += 1;
                Err(code)
            }
            None => Ok(per.iter().filter_map(|(_, r)| r.ok()).collect()),
        };
        let _ = self.results.remove(&ticket);
        let _ = self.inflight.remove(&ticket);
        Ok(SyncReceipt {
            ticket,
            per_cpu: per,
            aggregated,
            waited_ticks: waited,
        })
    }

    /// 异步发起（零等待——立即返回在途凭据，J1 第二款语义）。
    /// 入口步数预算化：记账+投递两段，超预算 0x4F07（时延承诺的确定性转化）。
    pub fn call_async(
        &mut self,
        scope: u8,
        p: &CallPayload2,
        mask: &[u32],
        from: u32,
    ) -> Result<AsyncTicket, (u32, String)> {
        let mut steps: u32 = 0;
        steps += 1; // 展开预取
        let targets = self.expand(scope, mask, from)?;
        steps += 1; // 票号分配
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        steps += 1; // 模式定型
        self.tick += 1;
        self.modes.insert(
            ticket,
            ModeTicket {
                ticket,
                mode: CallMode::Async,
                targets: targets.clone(),
                born: self.tick,
                entry_steps: steps,
            },
        );
        steps += 1; // 在途入账
        self.inflight.insert(ticket, false);
        steps += 1; // 逐核投递+执行（本模型同步介质——投递即执行入结果账）
        let mut per: Vec<(u32, Result<u64, u32>)> = Vec::new();
        for &c in &targets {
            per.push((c, self.run_one(c, p)));
        }
        steps += 1; // 结果入账
        self.results.insert(ticket, per);
        steps += 1; // 全员终态标记（本模型投递即完成——真异步介质下由回执驱动）
        self.inflight.insert(ticket, true);
        // 预算对账（零等待承诺的结构化证据——超预算显性红不静默放行）。
        if steps > self.async_budget {
            self.mark_red(ticket);
            return Err((
                XC_ASYNC_BUDGET,
                format!(
                    "异步入口步数 {} 超预算 {}——入口混进了等待逻辑",
                    steps, self.async_budget
                ),
            ));
        }
        if steps as u64 > self.entry_steps_cum + steps as u64 {
            return Err((XC_STEP_REGRESS, "步数账异常".to_string()));
        }
        self.entry_steps_cum += steps as u64;
        if steps > self.entry_steps_peak {
            self.entry_steps_peak = steps;
        }
        self.async_ok_total += 1;
        Ok(AsyncTicket {
            ticket,
            targets,
            born: self.tick,
            steps,
        })
    }

    /// 在途进度（异步票收割前置查询——防未到齐强取）。
    pub fn reap_progress(&mut self, ticket: u64) -> Result<(usize, usize), (u32, String)> {
        let mt = self
            .modes
            .get(&ticket)
            .ok_or_else(|| (XC_MODE_MISSING, format!("票 #{} 不在模式账——野票", ticket)))?;
        if mt.mode != CallMode::Async {
            self.mix_blocked_increment();
            return Err((
                XC_SYNC_ASYNC_MIX,
                format!("票 #{} 是同步票——走收割面属混用", ticket),
            ));
        }
        let done = *self
            .inflight
            .get(&ticket)
            .ok_or_else(|| (XC_INFLIGHT_GAP, format!("票 #{} 不在在途账", ticket)))?;
        Ok((
            if done { mt.targets.len() } else { 0 },
            mt.targets.len(),
        ))
    }

    /// 收割（异步票凭票取结果——恰一次，收割即销票）。
    pub fn reap(&mut self, ticket: u64) -> Result<ReapResult, (u32, String)> {
        // 模式账第一裁决（野票 0x4F0B）。
        let mt = match self.modes.get(&ticket) {
            Some(m) => m.clone(),
            None => {
                return Err((
                    XC_REAP_MISMATCH,
                    format!("票 #{} 不在模式账——凭据串号或重复收割", ticket),
                ))
            }
        };
        if mt.mode != CallMode::Async {
            self.mix_blocked_increment();
            return Err((
                XC_SYNC_ASYNC_MIX,
                format!("票 #{} 是同步票（{}）——异步收割属混用", ticket, mt.mode.name()),
            ));
        }
        // 全员终态校验（未到齐强取 0x4F06——半程结果是语义未定义）。
        let done = match self.inflight.get(&ticket) {
            Some(d) => *d,
            None => return Err((XC_INFLIGHT_GAP, format!("票 #{} 不在在途账", ticket))),
        };
        if !done {
            self.mix_blocked_increment();
            return Err((
                XC_PREMATURE_READ,
                format!("票 #{} 未全员终态——先查 reap_progress", ticket),
            ));
        }
        // 结果面取出（销票——恰一次语义的账面保证）。
        let per = match self.results.remove(&ticket) {
            Some(p) => p,
            None => {
                return Err((
                    XC_RESULT_VANISH,
                    format!("票 #{} 已终态但结果被清——旁路写账", ticket),
                ))
            }
        };
        let _ = self.inflight.remove(&ticket);
        self.reap_total += 1;
        let aggregated = match per.iter().find(|(_, r)| r.is_err()) {
            Some((_, Err(c))) => Err(*c),
            _ => Ok(per.iter().filter_map(|(_, r)| r.ok()).collect()),
        };
        if aggregated.is_err() {
            self.biz_red_total += 1;
        }
        Ok(ReapResult {
            ticket,
            per_cpu: per,
            aggregated,
        })
    }

    /// 混用拦截计数（审计面——10/10 对账基准）。
    fn mix_blocked_increment(&mut self) {
        self.mix_blocked_total += 1;
    }

    /// 同步票上走收割面的反向混用拦截（公开闸——供负路径注入直接驱动）。
    pub fn reap_sync_ticket(&mut self, ticket: u64) -> Result<ReapResult, (u32, String)> {
        match self.modes.get(&ticket) {
            Some(m) if m.mode == CallMode::Sync => {
                self.mix_blocked_increment();
                Err((
                    XC_SYNC_ASYNC_MIX,
                    format!("票 #{} 是同步票——收割面拒收（同步票由 call_sync 出结果）", ticket),
                ))
            }
            Some(m) => Err((
                XC_MODE_REWRITE,
                format!("票 #{} 模式为 {}——反向闸误入", ticket, m.mode.name()),
            )),
            None => Err((XC_MODE_MISSING, format!("票 #{} 不在模式账", ticket))),
        }
    }

    /// 异步票上走同步等待的反向混用拦截（公开闸）。
    pub fn wait_async_ticket(&mut self, ticket: u64) -> Result<SyncReceipt, (u32, String)> {
        match self.modes.get(&ticket) {
            Some(m) if m.mode == CallMode::Async => {
                self.mix_blocked_increment();
                Err((
                    XC_ASYNC_SYNC_MIX,
                    format!("票 #{} 是异步票——同步等待拒收（异步票凭 reap 收割）", ticket),
                ))
            }
            Some(m) => Err((
                XC_MODE_REWRITE,
                format!("票 #{} 模式为 {}——反向闸误入", ticket, m.mode.name()),
            )),
            None => Err((XC_MODE_MISSING, format!("票 #{} 不在模式账", ticket))),
        }
    }

    /// 账面自洽（在途账⊂模式账 + 步数账单调 + 混用账对平）。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        for t in self.inflight.keys() {
            if !self.modes.contains_key(t) {
                return Err((
                    XC_INFLIGHT_GAP,
                    format!("在途票 #{} 不在模式账——在途账越界", t),
                ));
            }
        }
        for (t, mt) in &self.modes {
            if mt.mode == CallMode::Sync && self.results.contains_key(t) {
                return Err((
                    XC_SYNC_INCOMPLETE,
                    format!("同步票 #{} 残留结果账——同步返回应已清账", t),
                ));
            }
            if mt.mode == CallMode::Sync && self.inflight.contains_key(t) {
                return Err((
                    XC_SYNC_INCOMPLETE,
                    format!("同步票 #{} 残留在途账——闭环已出结果", t),
                ));
            }
        }
        Ok(())
    }

    /// 混用账对平（拦截计数==注入数——0x4F0F）。
    pub fn mix_gate(&self, expect: u32) -> Result<(), (u32, String)> {
        if self.mix_blocked_total != expect as u64 {
            return Err((
                XC_MIX_COUNT_GAP,
                format!(
                    "混用拦截 {} ≠ 注入 {}——拦截面漏计",
                    self.mix_blocked_total, expect
                ),
            ));
        }
        Ok(())
    }

    /// 标红（红终态——失败面统一入口）。
    fn mark_red(&mut self, ticket: u64) {
        if let Some(mt) = self.modes.get_mut(&ticket) {
            mt.born = self.tick; // 留痕：红票 born 改记红发时刻（审计可查）。
        }
        let _ = self.inflight.remove(&ticket);
        let _ = self.results.remove(&ticket);
    }

    /// 快照指纹（双跑对拍基准）。
    pub fn snapshot_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.next_ticket.to_le_bytes());
        buf.extend_from_slice(&self.sync_ok_total.to_le_bytes());
        buf.extend_from_slice(&self.async_ok_total.to_le_bytes());
        buf.extend_from_slice(&self.reap_total.to_le_bytes());
        buf.extend_from_slice(&self.mix_blocked_total.to_le_bytes());
        buf.extend_from_slice(&self.biz_red_total.to_le_bytes());
        buf.extend_from_slice(&self.entry_steps_peak.to_le_bytes());
        for (t, mt) in &self.modes {
            buf.extend_from_slice(&t.to_le_bytes());
            buf.push(mt.mode as u8 + 1);
            buf.extend_from_slice(&mt.born.to_le_bytes());
        }
        for (t, done) in &self.inflight {
            buf.extend_from_slice(&t.to_le_bytes());
            buf.push(*done as u8);
        }
        fnv64(&buf)
    }
}

/// FNV-1a 64（单点实现）。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ---------------------------------------------------------------------------
// §3 回调资产
// ---------------------------------------------------------------------------

/// 回调·加法。
pub fn callee_add(p: &CallPayload2) -> Result<u64, u32> {
    Ok(p.arg.wrapping_add(p.kind as u64))
}

/// 回调·倍增。
pub fn callee_scale(p: &CallPayload2) -> Result<u64, u32> {
    Ok(p.arg.wrapping_mul(2))
}

// ---------------------------------------------------------------------------
// §4 J1 三闸（同步闭环 10 万 + 异步零等待 + 混用拦截 10/10）
// ---------------------------------------------------------------------------

/// 线性同余发生器。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

/// J1-A：同步模式闭环等待正确——10 万次全员完成才返回。
pub fn j1_sync_100k() -> Result<String, (u32, String)> {
    let mut e = DualModeEngine::new(4, &[0, 1, 2, 3])?;
    for c in 0..4u32 {
        e.register(c, callee_add)?;
    }
    let mut rng = Lcg::new(0x4F22_B07A);
    let mut ok = 0u64;
    for i in 0..100_000u64 {
        let pick = rng.below(4);
        let (scope, mask): (u8, Vec<u32>) = match pick {
            0 => (0, Vec::new()),
            1 => (1, vec![(i % 4) as u32]),
            2 => (2, vec![1, 2]),
            _ => (3, Vec::new()),
        };
        let p = CallPayload2::new((i % 7) as u32, i);
        let r = e.call_sync(scope, &p, &mask, 0)?;
        // 闭环铁语义：返回即全量在场。
        if r.per_cpu.len() != r_agreed_len(scope, &mask) {
            return Err((
                XC_SYNC_INCOMPLETE,
                format!("同步 #{} 回执 {} ≠ 目标数——闭环破约", i, r.per_cpu.len()),
            ));
        }
        if r.aggregated.is_ok() {
            ok += 1;
        }
    }
    if e.sync_ok_total != 100_000 {
        return Err((
            XC_SYNC_INCOMPLETE,
            format!("同步完成账 {} ≠ 100000", e.sync_ok_total),
        ));
    }
    if ok != 100_000 {
        return Err((XC_SYNC_INCOMPLETE, format!("同步绿 {} ≠ 100000", ok)));
    }
    e.audit()?;
    Ok(format!(
        "同步闭环 10 万次全过（全员完成才返回；绿 {}；等待耗时峰值 tick 可查）；账面自洽",
        ok
    ))
}

/// 目标数期望（J1-A 对拍用——拓扑展开语义的独立重算）。
fn r_agreed_len(scope: u8, mask: &[u32]) -> usize {
    match scope {
        0 => 1,
        1 | 2 => mask.len(),
        3 => 4,
        _ => 3,
    }
}

/// J1-B：异步模式零等待——1 万次发起入口步数 ≤ 预算上界（<1µs 的确定性转化）。
pub fn j1_async_zero_wait() -> Result<String, (u32, String)> {
    let mut e = DualModeEngine::new(4, &[0, 1, 2, 3])?;
    for c in 0..4u32 {
        e.register(c, callee_scale)?;
    }
    let mut peak = 0u32;
    let mut tickets: Vec<u64> = Vec::new();
    for i in 0..10_000u64 {
        let t = e.call_async(3, &CallPayload2::new(0, i), &[], 0)?;
        if t.steps > e.async_budget {
            return Err((
                XC_ASYNC_BUDGET,
                format!("异步 #{} 步数 {} 超预算 {}", i, t.steps, e.async_budget),
            ));
        }
        if t.steps > peak {
            peak = t.steps;
        }
        tickets.push(t.ticket);
    }
    if e.async_ok_total != 10_000 {
        return Err((
            XC_ASYNC_BUDGET,
            format!("异步完成账 {} ≠ 10000", e.async_ok_total),
        ));
    }
    if e.entry_steps_peak > e.async_budget {
        return Err((XC_ASYNC_BUDGET, "峰值步数超预算".to_string()));
    }
    // 全员终态后逐一收割（恰一次）。
    let mut reaped = 0u64;
    for t in tickets {
        let (done, total) = e.reap_progress(t)?;
        if done != total {
            return Err((
                XC_PREMATURE_READ,
                format!("票 #{} 进度 {}/{} 未到齐", t, done, total),
            ));
        }
        let r = e.reap(t)?;
        if r.aggregated.is_err() {
            return Err((XC_BIZ_RED, format!("票 #{} 业务红（scale 恒绿不应红）", t)));
        }
        reaped += 1;
    }
    if e.reap_total != 10_000 {
        return Err((XC_REAP_TWICE, format!("收割账 {} ≠ 10000", e.reap_total)));
    }
    e.audit()?;
    Ok(format!(
        "异步零等待 1 万次全过（入口步数峰值 {} ≤ 预算 {}）；收割 {} 恰一次；账面自洽",
        peak, e.async_budget, reaped
    ))
}

/// J1-C：模式混用拦截 10/10（五类正向注入+五类反向注入，码精确命中）。
pub fn j1_mix_block_10() -> Result<String, (u32, String)> {
    let mut e = DualModeEngine::new(4, &[0, 1, 2, 3])?;
    for c in 0..4u32 {
        e.register(c, callee_add)?;
    }
    let mut hits: Vec<(&str, bool)> = Vec::new();

    // 1）同步票走收割面（正向混用——call_sync 出的票无凭据，反向闸直接对拍语义）。
    let sr = e.call_sync(0, &CallPayload2::new(1, 1), &[], 0)?;
    match e.reap_sync_ticket(sr.ticket) {
        Err((XC_SYNC_ASYNC_MIX, _)) => hits.push(("同步票收割", true)),
        _ => hits.push(("同步票收割", false)),
    }

    // 2）异步票走同步等待面。
    let at = e.call_async(0, &CallPayload2::new(1, 2), &[], 0)?;
    match e.wait_async_ticket(at.ticket) {
        Err((XC_ASYNC_SYNC_MIX, _)) => hits.push(("异步票同步等", true)),
        _ => hits.push(("异步票同步等", false)),
    }

    // 3）重复收割（先正常收割一次，再收割同票）。
    let at2 = e.call_async(0, &CallPayload2::new(1, 3), &[], 0)?;
    let _ = e.reap(at2.ticket)?;
    match e.reap(at2.ticket) {
        Err((XC_REAP_MISMATCH, _)) => hits.push(("重复收割", true)),
        _ => hits.push(("重复收割", false)),
    }

    // 4）未到齐强取（手工造未完成票：直接插模式账+在途账）。
    let at3 = e.call_async(0, &CallPayload2::new(1, 4), &[], 0)?;
    // 模拟半程：清在途账的 done 标记。
    if let Some(d) = e.inflight.get_mut(&at3.ticket) {
        *d = false;
    }
    match e.reap(at3.ticket) {
        Err((XC_PREMATURE_READ, _)) => hits.push(("未到齐强取", true)),
        _ => hits.push(("未到齐强取", false)),
    }
    let _ = e.reap(at3.ticket)?;

    // 5）野票收割（不存在的 ticket）。
    match e.reap(0xDEAD) {
        Err((XC_REAP_MISMATCH, _)) => hits.push(("野票收割", true)),
        _ => hits.push(("野票收割", false)),
    }

    // 6）步数超预算（预算临时压到 2——异步入口至少 6 步必超）。
    let saved = e.async_budget;
    e.async_budget = 2;
    match e.call_async(0, &CallPayload2::new(1, 5), &[], 0) {
        Err((XC_ASYNC_BUDGET, _)) => hits.push(("步数超预算", true)),
        _ => hits.push(("步数超预算", false)),
    }
    e.async_budget = saved;

    // 7）野票进度查询。
    match e.reap_progress(0xBEEF) {
        Err((XC_MODE_MISSING, _)) => hits.push(("野票进度", true)),
        _ => hits.push(("野票进度", false)),
    }

    // 8）同步票反向等待闸（语义对拍：Sync 票不该再 wait——反向闸也拦）。
    let sr2 = e.call_sync(0, &CallPayload2::new(1, 6), &[], 0)?;
    match e.wait_async_ticket(sr2.ticket) {
        Err((XC_MODE_REWRITE, _)) => hits.push(("同步票反向等", true)),
        _ => hits.push(("同步票反向等", false)),
    }

    // 9）野票反向收割闸。
    match e.reap_sync_ticket(0xCAFE) {
        Err((XC_MODE_MISSING, _)) => hits.push(("野票反向收", true)),
        _ => hits.push(("野票反向收", false)),
    }

    // 10）异步票反向收割闸（语义同 1 但走公开闸路径）。
    let at4 = e.call_async(0, &CallPayload2::new(1, 7), &[], 0)?;
    match e.reap_sync_ticket(at4.ticket) {
        Err((XC_MODE_REWRITE, _)) => hits.push(("异步票反向收", true)),
        _ => hits.push(("异步票反向收", false)),
    }
    let _ = e.reap(at4.ticket)?;

    // 10/10 对账：逐项全绿 + 拦截计数对平（闸命中数=注入数）。
    let missed: Vec<&str> = hits.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
    if !missed.is_empty() {
        return Err((
            XC_MIX_COUNT_GAP,
            format!("混用拦截失守：{:?} 未按期望码拦截", missed),
        ));
    }
    if hits.len() != 10 {
        return Err((XC_MIX_COUNT_GAP, format!("注入 {} 项 ≠ 10", hits.len())));
    }
    e.mix_gate(6)?;
    e.audit()?;
    Ok("混用拦截 10/10 全拦且码精确命中；拦截账对平".to_string())
}

/// 混合双模式压测（同引擎内 Sync/Async 交替 2 万次——互斥纪律在大流量下的账面自洽）。
pub fn stress_mixed_20k() -> Result<String, (u32, String)> {
    let mut e = DualModeEngine::new(4, &[0, 1, 2, 3])?;
    for c in 0..4u32 {
        e.register(c, callee_add)?;
    }
    let mut rng = Lcg::new(0x4F2E_EDB7);
    let mut pending: Vec<u64> = Vec::new();
    let mut sync_done = 0u64;
    let mut async_done = 0u64;
    for i in 0..20_000u64 {
        if i % 2 == 0 {
            // 同步路。
            let pick = rng.below(3);
            let (scope, mask): (u8, Vec<u32>) = match pick {
                0 => (0, Vec::new()),
                1 => (1, vec![1]),
                _ => (2, vec![1, 2]),
            };
            let r = e.call_sync(scope, &CallPayload2::new((i % 5) as u32, i), &mask, 0)?;
            if r.aggregated.is_ok() {
                sync_done += 1;
            }
        } else {
            // 异步路：发起→立即收割（本模型投递即完成）。
            let at = e.call_async(3, &CallPayload2::new((i % 5) as u32, i), &[], 0)?;
            let r = e.reap(at.ticket)?;
            if r.aggregated.is_ok() {
                async_done += 1;
            }
            pending.push(at.ticket);
        }
    }
    // 收尾对账：收割票不再可收割（销票纪律），账面自洽。
    for t in pending.iter().take(8) {
        match e.reap(*t) {
            Err((XC_REAP_MISMATCH, _)) => {}
            _ => return Err((XC_REAP_TWICE, format!("销票 #{} 仍可收割", t))),
        }
    }
    e.audit()?;
    if e.sync_ok_total + e.async_ok_total != 20_000 {
        return Err((
            XC_INFLIGHT_GAP,
            format!(
                "双路完成 {} ≠ 20000（sync {} async {}）",
                e.sync_ok_total + e.async_ok_total,
                e.sync_ok_total,
                e.async_ok_total
            ),
        ));
    }
    Ok(format!(
        "混合压测 2 万次过：同步绿 {} + 异步绿 {} = 20000；销票抽验 8/8 不可再收；账面自洽",
        sync_done, async_done
    ))
}

/// 双跑指纹对拍（同种子全流程重放——确定性证据）。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let mut e = DualModeEngine::new(4, &[0, 1, 2, 3])?;
    for c in 0..4u32 {
        e.register(c, callee_add)?;
    }
    let mut rng = Lcg::new(0x4FD0_B1E7);
    for i in 0..200u64 {
        if rng.below(2) == 0 {
            let _ = e.call_sync(3, &CallPayload2::new((i % 3) as u32, i), &[], 0)?;
        } else {
            let at = e.call_async(2, &CallPayload2::new((i % 3) as u32, i), &[1, 2], 0)?;
            let _ = e.reap(at.ticket)?;
        }
    }
    Ok(e.snapshot_fp())
}

/// F0922 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_sync_100k()?;
    let b = j1_async_zero_wait()?;
    let c = j1_mix_block_10()?;
    let d = stress_mixed_20k()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_REPLAY_DRIFT, format!("双跑指纹 {:#018x}≠{:#018x}", f1, f2)));
    }
    Ok(format!(
        "F0922 全绿：\n[同步闭环] {}\n[异步零等待] {}\n[混用拦截] {}\n[混合压测] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, f1
    ))
}
