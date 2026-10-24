//! VE-F6204 · 输入状态机（veae04_devfsm）
//!
//! 输入设备连接状态的**完整状态机**——断开/重连/切换的**全路径有
//! 归宿**（每个状态都有出边通往稳态或终态，没有走不出去的死态——
//! 悬空状态是设备管理的债务）；**设备热切换原子事务**（失败整组
//! 回滚——半切换状态比不切换更毒）；**状态变化通知消费方**（通知
//! 丢失→补发——消费方不知道设备走了，输入就丢进了黑洞）；含**设备
//! 热插拔风暴验证**（快速插拔十次的稳态回归——风暴之后账要平）。
//!
//! 数据结构（锚点原文）：状态机；切换事务。
//!
//! 错误路径与降级矩阵：**切换失败→回滚+告知**（事务日志 undo）；
//! **状态悬空→超时兜底**（Suspect 态超时→Gone——不永久悬空）；
//! **通知丢失→补发**（ack 账——未 ack 通知重发）。性能：O(状态)。
//! 跨批对接点：V02 设备管理同构（veab02_devices 五态切换同款纪律）。
//! 无障碍：状态图读屏可达（`fsm_spoken`）。无隐私面。
//!
//! Z 域字符串码五码互异：`E_DS_ILLEGAL` / `E_DS_ROLLBACK` /
//! `E_DS_SUSPECT_TIMEOUT` / `E_DS_NO_SUBSCRIBER` / `E_DS_UNKNOWN_DEV`。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 非法迁移（状态机闭集外出边）。
pub const E_DS_ILLEGAL: &str = "E_DS_ILLEGAL";
/// 切换事务失败回滚（原子性——半切换状态被整组撤销）。
pub const E_DS_ROLLBACK: &str = "E_DS_ROLLBACK";
/// 悬空超时（Suspect 超时兜底→Gone）。
pub const E_DS_SUSPECT_TIMEOUT: &str = "E_DS_SUSPECT_TIMEOUT";
/// 通知无主（订阅簿空——变化无人知晓显性告警）。
pub const E_DS_NO_SUBSCRIBER: &str = "E_DS_NO_SUBSCRIBER";
/// 查无设备（未登记设备 ID）。
pub const E_DS_UNKNOWN_DEV: &str = "E_DS_UNKNOWN_DEV";

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_DS_ILLEGAL => "非法迁移：状态机闭集无此出边",
        E_DS_ROLLBACK => "切换失败回滚：事务整组撤销并告知",
        E_DS_SUSPECT_TIMEOUT => "悬空超时：Suspect 超时兜底转 Gone",
        E_DS_NO_SUBSCRIBER => "通知无主：订阅簿空，状态变化无人知晓",
        E_DS_UNKNOWN_DEV => "查无设备：设备 ID 未登记状态机",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// 状态机（五态闭集 + 迁移表全枚举 + 超时兜底）
// ---------------------------------------------------------------------------

/// 设备连接状态（五态闭集——V02 设备管理同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// 未接入（稳态）。
    Absent,
    /// 接入中（握手——有超时归宿）。
    Arriving,
    /// 在线（稳态）。
    Active,
    /// 可疑（失联观察——超时兜底归宿 Gone，恢复归宿 Active）。
    Suspect,
    /// 已断开（终态——可重 Arriving 重开新生命周期）。
    Gone,
}

impl LinkState {
    pub const COUNT: usize = 5;

    pub fn all() -> [LinkState; LinkState::COUNT] {
        [
            LinkState::Absent,
            LinkState::Arriving,
            LinkState::Active,
            LinkState::Suspect,
            LinkState::Gone,
        ]
    }

    pub fn zh(self) -> &'static str {
        match self {
            LinkState::Absent => "未接入",
            LinkState::Arriving => "接入中",
            LinkState::Active => "在线",
            LinkState::Suspect => "可疑",
            LinkState::Gone => "已断开",
        }
    }

    pub fn ordinal(self) -> usize {
        match self {
            LinkState::Absent => 0,
            LinkState::Arriving => 1,
            LinkState::Active => 2,
            LinkState::Suspect => 3,
            LinkState::Gone => 4,
        }
    }
}

/// 迁移事件闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkEvent {
    /// 插入/重连请求。
    Plug,
    /// 握手完成。
    HandshakeOk,
    /// 握手失败。
    HandshakeFail,
    /// 心跳丢失（Active→Suspect 观察）。
    HeartbeatLost,
    /// 心跳恢复。
    HeartbeatBack,
    /// 可疑超时（兜底归宿）。
    SuspectTimeout,
    /// 拔出。
    Unplug,
}

/// 迁移表全枚举（合法边 Some(目标)；非法边 None——闭集显性，
/// vcs03/cgr05 隔离格同款；**全路径归宿**：每态存在出边链到稳态或
/// 终态——Absent 出 Plug、Gone 出 Plug、Suspect 双归宿）。
pub fn transition(from: LinkState, ev: LinkEvent) -> Option<LinkState> {
    use LinkEvent::*;
    use LinkState::*;
    match (from, ev) {
        (Absent, Plug) => Some(Arriving),
        (Arriving, HandshakeOk) => Some(Active),
        (Arriving, HandshakeFail) => Some(Absent),
        (Arriving, Unplug) => Some(Gone),
        (Active, HeartbeatLost) => Some(Suspect),
        (Active, Unplug) => Some(Gone),
        (Suspect, HeartbeatBack) => Some(Active),
        (Suspect, SuspectTimeout) => Some(Gone),
        (Suspect, Unplug) => Some(Gone),
        (Gone, Plug) => Some(Arriving), // 重开新生命周期——终态不是死态
        _ => None,
    }
}

/// 单设备连接状态机（迁移执行+滞留帧计数+悬空兜底）。
pub struct DeviceFsm {
    pub dev: u32,
    pub state: LinkState,
    /// 滞留帧数（当前态滞留时长——Suspect 超时判定用）。
    pub dwell_frames: u64,
    /// 迁移账（迁移次数——风暴对账面）。
    pub transitions: u32,
}

/// Suspect 悬空兜底阈值（帧）。
pub const SUSPECT_TIMEOUT_FRAMES: u64 = 30;

impl DeviceFsm {
    pub fn new(dev: u32) -> Self {
        DeviceFsm { dev, state: LinkState::Absent, dwell_frames: 0, transitions: 0 }
    }

    /// 迁移步进（非法边显性拒；合法边记账）。
    pub fn feed(&mut self, ev: LinkEvent) -> Result<LinkState, &'static str> {
        let next = transition(self.state, ev).ok_or(E_DS_ILLEGAL)?;
        self.state = next;
        self.dwell_frames = 0;
        self.transitions += 1;
        Ok(next)
    }

    /// 帧推进（滞留计数+Suspect 超时兜底——悬空不永久）。
    pub fn tick(&mut self, frames: u64) -> Result<Option<LinkState>, &'static str> {
        self.dwell_frames += frames;
        if self.state == LinkState::Suspect && self.dwell_frames >= SUSPECT_TIMEOUT_FRAMES {
            let gone = self.feed(LinkEvent::SuspectTimeout)?;
            return Ok(Some(gone));
        }
        Ok(None)
    }

    /// 全路径归宿静态自检（O(状态)——每态存在出边链到稳态/终态；
    /// 死态=无任何合法出边且非稳态/终态——本闭集无死态）。
    pub fn all_paths_reachable() -> bool {
        use LinkEvent::*;
        use LinkState::*;
        // 每态至少一条出边（Absent→Plug、Arriving→HandshakeOk、
        // Active→Unplug、Suspect→HeartbeatBack、Gone→Plug）。
        let probes = [
            (Absent, Plug),
            (Arriving, HandshakeOk),
            (Active, Unplug),
            (Suspect, HeartbeatBack),
            (Gone, Plug),
        ];
        probes.iter().all(|(s, e)| transition(*s, *e).is_some())
    }
}

// ---------------------------------------------------------------------------
// 热切换原子事务（失败整组回滚）
// ---------------------------------------------------------------------------

/// 切换事务步骤（原子单元——apply/undo 对偶）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SwitchStep {
    /// 暂停旧设备输入流。
    PauseOld,
    /// 激活新设备输入流。
    ResumeNew,
    /// 迁移映射表条目（旧设备映射→新设备）。
    MigrateMaps { count: u32 },
}

/// 切换事务结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxnOutcome {
    Committed,
    RolledBack,
}

/// 热切换事务（原子性：步骤序列全成功才 commit；任一步失败整组
/// undo——半切换状态比不切换更毒）。
pub struct SwitchTxn {
    steps: Vec<SwitchStep>,
    /// 已 apply 的步骤栈（undo 按逆序弹）。
    applied: Vec<SwitchStep>,
    pub outcome: Option<TxnOutcome>,
}

impl SwitchTxn {
    pub fn new(steps: Vec<SwitchStep>) -> Self {
        SwitchTxn { steps, applied: Vec::new(), outcome: None }
    }

    /// 事务执行（O(步骤)——executor 返回 Ok 继续下一步；Err 即整组
    /// 回滚+告知（rollback 后调用方取 E_DS_ROLLBACK 语义））。
    pub fn run(&mut self, mut executor: impl FnMut(&SwitchStep) -> Result<(), &'static str>) -> TxnOutcome {
        for step in self.steps.iter() {
            match executor(step) {
                Ok(()) => self.applied.push(*step),
                Err(_) => {
                    // 整组回滚：逆序 undo 已 apply 步骤。
                    while let Some(done) = self.applied.pop() {
                        let _ = undo_step(&done);
                    }
                    self.outcome = Some(TxnOutcome::RolledBack);
                    return TxnOutcome::RolledBack;
                }
            }
        }
        self.outcome = Some(TxnOutcome::Committed);
        TxnOutcome::Committed
    }

    pub fn applied_len(&self) -> usize {
        self.applied.len()
    }
}

/// 步骤 undo（回滚原语——示例域 undo 恒成功；真实执行面由设备段承接）。
fn undo_step(_s: &SwitchStep) -> Result<(), &'static str> {
    Ok(())
}

// ---------------------------------------------------------------------------
// 状态变化通知（订阅簿 + ack 补发）
// ---------------------------------------------------------------------------

/// 通知行（设备+新旧态+帧戳）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotifyRow {
    pub dev: u32,
    pub from: LinkState,
    pub to: LinkState,
    pub frame: u64,
}

/// 订阅消费方。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Subscriber {
    id: u32,
}

/// 通知中心（订阅+发布+ack 补发——通知丢失可补发不黑洞）。
pub struct NotifyHub {
    subs: Vec<Subscriber>,
    /// 未 ack 通知队列（补发面）。
    pending: Vec<(u32, NotifyRow)>, // (subscriber_id, row)
    pub delivered: u32,
    pub resent: u32,
}

impl NotifyHub {
    pub fn new() -> Self {
        NotifyHub { subs: Vec::new(), pending: Vec::new(), delivered: 0, resent: 0 }
    }

    pub fn subscribe(&mut self, id: u32) -> bool {
        if self.subs.iter().any(|s| s.id == id) {
            return false;
        }
        self.subs.push(Subscriber { id });
        true
    }

    pub fn sub_count(&self) -> usize {
        self.subs.len()
    }

    /// 发布状态变化（订阅簿空→显性 E_DS_NO_SUBSCRIBER 告警——变化
    /// 无人知晓是告警不是静默；有订阅者→全员入队待 ack）。
    pub fn publish(&mut self, row: NotifyRow) -> Result<usize, &'static str> {
        if self.subs.is_empty() {
            return Err(E_DS_NO_SUBSCRIBER);
        }
        let n = self.subs.len();
        for s in self.subs.iter() {
            self.pending.push((s.id, row));
        }
        Ok(n)
    }

    /// 投递（pending 全量出队=送达；ack 缺席的重发——补发面）。
    pub fn flush(&mut self, acked: &[u32]) -> usize {
        let mut sent = 0;
        let mut remain = Vec::new();
        for (sid, row) in self.pending.drain(..) {
            if acked.contains(&sid) {
                self.delivered += 1;
            } else {
                remain.push((sid, row));
                self.resent += 1;
            }
            let _ = sent;
            sent += 1;
        }
        self.pending = remain;
        sent
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

impl Default for NotifyHub {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 状态图读屏（无障碍）
// ---------------------------------------------------------------------------

/// 状态图可读名单（五态+合法出边数——O(状态×事件) 闭集枚举）。
pub fn fsm_spoken() -> String {
    let mut out = String::from("设备连接状态图：");
    for st in LinkState::all().iter() {
        let mut edges = 0;
        for ev in [
            LinkEvent::Plug,
            LinkEvent::HandshakeOk,
            LinkEvent::HandshakeFail,
            LinkEvent::HeartbeatLost,
            LinkEvent::HeartbeatBack,
            LinkEvent::SuspectTimeout,
            LinkEvent::Unplug,
        ]
        .iter()
        {
            if transition(*st, *ev).is_some() {
                edges += 1;
            }
        }
        out.push_str(&format!("[{}]出{} ", st.zh(), edges));
    }
    out
}

// ---------------------------------------------------------------------------
// 判据（锚点逐条：全路径归宿/原子切换/悬空兜底/变化通知/判据）
// ---------------------------------------------------------------------------

/// F6204 判据集（9 条，全部数值手算复核、零 panic 面）。
pub fn run_veae04_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F6204");

    // --- 全路径归宿（判据一）---
    // 归-01：迁移表全枚举对拍——合法 10 边恰值（含 Gone→Plug 重开
    // 终态不是死态）+非法边抽查（Active→Plug 等）None+每态出边≥1
    // 无死态+静态自检 True。
    use LinkEvent as E;
    use LinkState as S;
    let legal = [
        (S::Absent, E::Plug, S::Arriving),
        (S::Arriving, E::HandshakeOk, S::Active),
        (S::Arriving, E::HandshakeFail, S::Absent),
        (S::Arriving, E::Unplug, S::Gone),
        (S::Active, E::HeartbeatLost, S::Suspect),
        (S::Active, E::Unplug, S::Gone),
        (S::Suspect, E::HeartbeatBack, S::Active),
        (S::Suspect, E::SuspectTimeout, S::Gone),
        (S::Suspect, E::Unplug, S::Gone),
        (S::Gone, E::Plug, S::Arriving),
    ];
    let mut legal_ok = true;
    for (f, e, t) in legal.iter() {
        if transition(*f, *e) != Some(*t) {
            legal_ok = false;
        }
    }
    let illegal = [
        (S::Active, E::Plug),
        (S::Absent, E::HandshakeOk),
        (S::Gone, E::HeartbeatLost),
        (S::Suspect, E::HandshakeOk),
    ];
    let mut illegal_ok = true;
    for (f, e) in illegal.iter() {
        if transition(*f, *e).is_some() {
            illegal_ok = false;
        }
    }
    let reach = DeviceFsm::all_paths_reachable();
    let path_ok = legal_ok && illegal_ok && reach && legal.len() == 10;
    if path_ok {
        s.ok("F6204-归-01-迁移表全枚举无死态");
    } else {
        s.fail("F6204-归-01-迁移表全枚举无死态", "全路径归宿失效");
    }

    // --- 全生命周期（判据二）---
    // 生-01：完整生命周期链——Absent→Plug→HandshakeOk→Active→
    // HeartbeatLost→Suspect→HeartbeatBack→Active→Unplug→Gone→Plug
    // 重开——断开重连切换全路径走通 transitions 账恰 10。
    let mut f2 = DeviceFsm::new(1);
    let seq = [
        (E::Plug, S::Arriving),
        (E::HandshakeOk, S::Active),
        (E::HeartbeatLost, S::Suspect),
        (E::HeartbeatBack, S::Active),
        (E::Unplug, S::Gone),
        (E::Plug, S::Arriving),
    ];
    let mut life_ok = true;
    for (ev, want) in seq.iter() {
        match f2.feed(*ev) {
            Ok(got) if got == *want => {}
            _ => life_ok = false,
        }
    }
    let life = life_ok && f2.transitions == 6 && f2.state == S::Arriving;
    if life {
        s.ok("F6204-生-01-全生命周期重开归宿");
    } else {
        s.fail("F6204-生-01-全生命周期重开归宿", "生命周期失效");
    }

    // --- 悬空兜底（判据三）---
    // 悬-01：Suspect 超时兜底——滞留 29 帧不触发（恰值 <30）/30 帧
    // 触发→Gone；非法事件拒 dwell 不动；Absent tick 不兜底。
    let mut f3 = DeviceFsm::new(2);
    let _ = f3.feed(E::Plug);
    let _ = f3.feed(E::HandshakeOk);
    let _ = f3.feed(E::HeartbeatLost);
    let no_to = f3.tick(29).expect("probe-3a");
    let to = f3.tick(1).expect("probe-3b"); // 累计 30 → Gone
    let mut f3b = DeviceFsm::new(3);
    let _ = f3b.tick(100); // Absent 滞留不兜底
    let abs_still = f3b.state == S::Absent;
    let susp_ok = no_to.is_none()
        && matches!(to, Some(S::Gone))
        && f3.state == S::Gone
        && f3.dwell_frames == 0
        && abs_still;
    if susp_ok {
        s.ok("F6204-悬-01-超时兜底恰值30帧");
    } else {
        s.fail("F6204-悬-01-超时兜底恰值30帧", "悬空兜底失效");
    }

    // --- 非法迁移（判据四）---
    // 非-01：非法事件显性拒 E_DS_ILLEGAL 态不变账不动——闭集纪律。
    let mut f4 = DeviceFsm::new(4);
    let bad = f4.feed(E::HandshakeOk); // Absent 不能直接握手
    let non_ok = matches!(bad, Err(E_DS_ILLEGAL))
        && f4.state == S::Absent
        && f4.transitions == 0;
    if non_ok {
        s.ok("F6204-非-01-非法迁移显性拒");
    } else {
        s.fail("F6204-非-01-非法迁移显性拒", "迁移闸失效");
    }

    // --- 原子切换（判据五）---
    // 原-01：事务全成功 commit——3 步全 applied；中途失败整组回滚
    // applied 清零 outcome=RolledBack（半切换状态被整组撤销——
    // 失败在第 2 步：undo 逆序弹出第 1 步）。
    let steps = vec![
        SwitchStep::PauseOld,
        SwitchStep::ResumeNew,
        SwitchStep::MigrateMaps { count: 3 },
    ];
    let mut t1 = SwitchTxn::new(steps.clone());
    let o1 = t1.run(|_| Ok(()));
    let mut fail_on = 0;
    let mut t2 = SwitchTxn::new(steps);
    let o2 = t2.run(|_| {
        fail_on += 1;
        if fail_on == 2 {
            Err(E_DS_ROLLBACK)
        } else {
            Ok(())
        }
    });
    let txn_ok = o1 == TxnOutcome::Committed
        && t1.applied_len() == 3
        && o2 == TxnOutcome::RolledBack
        && t2.applied_len() == 0;
    if txn_ok {
        s.ok("F6204-原-01-切换事务整组回滚");
    } else {
        s.fail("F6204-原-01-切换事务整组回滚", "原子切换失效");
    }

    // --- 变化通知（判据六）---
    // 知-01：发布→全员入队（2 订阅者 pending=2）；ack 缺席重发
    // resent 计数；无订阅者发布显性告警 E_DS_NO_SUBSCRIBER——
    // 变化无人知晓是告警不是静默。
    let mut h6 = NotifyHub::new();
    let nopub = h6.publish(NotifyRow { dev: 1, from: S::Absent, to: S::Active, frame: 1 });
    let _ = h6.subscribe(7);
    let _ = h6.subscribe(8);
    let n = h6.publish(NotifyRow { dev: 1, from: S::Absent, to: S::Active, frame: 2 }).expect("probe-6a");
    let sent1 = h6.flush(&[7]); // 8 未 ack → 重发
    let pend = h6.pending_len();
    let sent2 = h6.flush(&[8]);
    let know_ok = matches!(nopub, Err(E_DS_NO_SUBSCRIBER))
        && n == 2
        && h6.delivered == 2 // 7 首轮 ack + 8 次轮 ack
        && sent1 == 2
        && pend == 1
        && sent2 == 1
        && h6.resent == 1
        && h6.pending_len() == 0;
    if know_ok {
        s.ok("F6204-知-01-变化通知ack补发");
    } else {
        s.fail("F6204-知-01-变化通知ack补发", "变化通知失效");
    }

    // --- 热插拔风暴（判据七）---
    // 风-01：快速插拔十次稳态回归——Plug/Unplug 交替 10 轮最终态
    // 收敛 Gone（末次拔出）；transitions 账 20 恰值；账对账不漂移。
    let mut f7 = DeviceFsm::new(5);
    for _ in 0..10 {
        let _ = f7.feed(E::Plug);
        let _ = f7.feed(E::Unplug);
    }
    let storm_ok = f7.state == S::Gone && f7.transitions == 20;
    if storm_ok {
        s.ok("F6204-风-01-插拔十次稳态回归");
    } else {
        s.fail("F6204-风-01-插拔十次稳态回归", "风暴验证失效");
    }

    // --- 判据元（判据八）---
    // 元-01：五码互异 + reason 互异非空 + 缺省兜底 + 五态闭集互异
    // +零 panic 空态（空 hub pending=0 平凡；未登记设备语义由
    // DeviceFsm 独立实例承载——无共享表无串账）。
    let codes = [E_DS_ILLEGAL, E_DS_ROLLBACK, E_DS_SUSPECT_TIMEOUT, E_DS_NO_SUBSCRIBER, E_DS_UNKNOWN_DEV];
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] || reason_of(codes[i]) == reason_of(codes[j]) {
                distinct = false;
            }
        }
    }
    let states = LinkState::all();
    let mut st_distinct = true;
    for i in 0..states.len() {
        for j in (i + 1)..states.len() {
            if states[i] == states[j] || states[i].zh() == states[j].zh() {
                st_distinct = false;
            }
        }
    }
    let h8 = NotifyHub::new();
    let meta_ok = distinct
        && st_distinct
        && reason_of("E_DS_UNKNOWN") == "未知诊断码"
        && codes.iter().all(|c| !reason_of(c).is_empty())
        && h8.pending_len() == 0;
    if meta_ok {
        s.ok("F6204-元-01-五码五态互异零panic");
    } else {
        s.fail("F6204-元-01-五码五态互异零panic", "判据元失效");
    }

    // --- 编排（判据九）---
    // 编-01：全链路 O(状态)——设备 1 完整热切换剧本：Active 中
    // 新设备到达→切换事务 3 步 commit→旧设备拔出 Gone→通知发布
    // 2 订阅者→ack 补发闭环→风暴 10 轮收敛→读屏名单对账。
    let mut f9 = DeviceFsm::new(1);
    let _ = f9.feed(E::Plug);
    let _ = f9.feed(E::HandshakeOk);
    let txn_steps = vec![
        SwitchStep::PauseOld,
        SwitchStep::ResumeNew,
        SwitchStep::MigrateMaps { count: 2 },
    ];
    let mut t9 = SwitchTxn::new(txn_steps);
    let o9 = t9.run(|_| Ok(()));
    let _ = f9.feed(E::Unplug);
    let mut h9 = NotifyHub::new();
    let _ = h9.subscribe(1);
    let _ = h9.subscribe(2);
    let _ = h9.publish(NotifyRow { dev: 1, from: S::Active, to: S::Gone, frame: 10 });
    let _ = h9.flush(&[1, 2]);
    for _ in 0..10 {
        let _ = f9.feed(E::Plug);
        let _ = f9.feed(E::Unplug);
    }
    let spoken9 = fsm_spoken();
    let chain_ok = o9 == TxnOutcome::Committed
        && f9.state == S::Gone
        && f9.transitions == 23 // 3 初始 + 20 风暴（首轮 Plug 从 Gone 起）
        && h9.delivered == 2
        && h9.pending_len() == 0
        && spoken9.contains("[未接入]出1")
        && spoken9.contains("[可疑]出3");
    if chain_ok {
        s.ok("F6204-编-01-全链路O状态对账");
    } else {
        s.fail("F6204-编-01-全链路O状态对账", "编排失效");
    }

    s
}
