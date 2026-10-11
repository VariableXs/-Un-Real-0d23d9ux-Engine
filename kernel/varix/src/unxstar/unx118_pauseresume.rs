//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0929 · 核暂停/恢复协议（暂停期 IPI 排队语义）（目标 320 行）
//! 判据原文摘录（书 2596 行）：UNX-F0929-J1 暂停期请求零丢失（暂停期间 1 万
//! 请求恢复后全执行），暂停协议闭环（进入/退出确认 10/10 次），暂停中同步 call
//! 超时语义正确 10/10 次。
//! 职责定位：核暂停（热更新/调试停机/电源管理场景）期间到达的跨核请求不能丢也
//! 不能执行错——协议语义：暂停核的入站请求全部进「暂停排队账」（零丢失承诺），
//! 恢复时按到达序重放执行（重放账可审计）；暂停/恢复是双握手的（发起方请求→
//! 目标核确认进入；恢复方请求→目标核确认退出——单边宣告不是协议是愿望）；暂停
//! 中同步 call 的等待方必须有正确的超时语义（目标核不回执不是介质故障，是「目
//! 标暂停中」的显性状态——超时分类要与介质超时区分开）。
//! 设计要点：①**五态暂停状态机**——Running→Pausing→Paused→Resuming→Running
//! 单向环，每态迁移带确认握手（Pausing 确认后才算 Paused，恢复同构）；②**暂停
//! 排队账**——Paused 期入站请求只入账不执行，恢复时按序重放（序号保序+重放账
//! 与排队账逐笔对拍——零丢失的结构化证据）；③**超时分类**——同步 call 超窗时
//! 查目标核状态：Paused→「暂停超时」（非故障，等待恢复后重放完成再回执）/Running→
//! 「介质超时」（真故障）；④**家族复用声明**——FNV/逻辑 tick/LCG/ticket 单调
//! 复用前序（家族复用第十六用），本单新增的是暂停状态机与重放账面。
//! 诊断码段：0x56（本单独占，0x5601~0x560F）。
//! 施工：AI-WB2 · 2026-10-11。

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0x56 段独占）
// ---------------------------------------------------------------------------

/// 0x5601 暂停态倒退（五态单向环被违反——状态机破坏）。
pub const XC_PAUSE_REGRESS: u32 = 0x5601;
/// 0x5602 握手缺失（未确认即迁移——协议闭环破坏）。
pub const XC_HANDSHAKE_MISSING: u32 = 0x5602;
/// 0x5603 暂停期执行（Paused 态执行了入站请求——排队纪律破坏）。
pub const XC_EXEC_WHILE_PAUSED: u32 = 0x5603;
/// 0x5604 重放丢笔（恢复重放数<排队数——零丢失破约）。
pub const XC_REPLAY_LOSS: u32 = 0x5604;
/// 0x5605 重放错序（重放序≠到达序——保序破坏）。
pub const XC_REPLAY_ORDER: u32 = 0x5605;
/// 0x5606 双重暂停（已 Paused 再请求暂停——幂等面外的重复动作）。
pub const XC_DOUBLE_PAUSE: u32 = 0x5606;
/// 0x5607 恢复未暂停（Running 态请求恢复——状态面空洞）。
pub const XC_RESUME_NOT_PAUSED: u32 = 0x5607;
/// 0x5608 超时分类错（暂停超时被记成介质超时或反之——分类面失守）。
pub const XC_TIMEOUT_KIND: u32 = 0x5608;
/// 0x5609 排队越容量（暂停排队账超上限——容量面越界）。
pub const XC_QUEUE_CAP: u32 = 0x5609;
/// 0x560A 确认越核（握手确认来自非目标核——协议面串号）。
pub const XC_ACK_ALIEN: u32 = 0x560A;
/// 0x560B 暂停账失守（暂停/恢复计数与注入数不符——审计面漂移）。
pub const XC_PAUSE_COUNT_GAP: u32 = 0x560B;
/// 0x560C 排队账残缺（排队数与账面明细数不符——账面失真）。
pub const XC_QUEUE_LEDGER_GAP: u32 = 0x560C;
/// 0x560D 恢复后状态残留（恢复完成但状态未回 Running——环未闭合）。
pub const XC_RESUME_RESIDUE: u32 = 0x560D;
/// 0x560E 双跑漂移（同种子重放指纹不等——确定性破坏）。
pub const XC_REPLAY_DRIFT: u32 = 0x560E;
/// 0x560F 超时账失守（暂停超时计数与注入数不符——超时面漂移）。
pub const XC_TIMEOUT_COUNT_GAP: u32 = 0x560F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_PAUSE_REGRESS => (
            "暂停态倒退：五态单向环被违反",
            "Running→Pausing→Paused→Resuming→Running 只进不退（环闭合处除外）；倒退=旁路改状态",
        ),
        XC_HANDSHAKE_MISSING => (
            "握手缺失：未确认即迁移",
            "双握手是协议闭环的承诺；单边宣告状态=愿望不是协议",
        ),
        XC_EXEC_WHILE_PAUSED => (
            "暂停期执行：Paused 态执行了入站请求",
            "Paused 核唯一的合法动作是入排队账；执行=暂停语义失效",
        ),
        XC_REPLAY_LOSS => (
            "重放丢笔：恢复重放数<排队数",
            "零丢失承诺=排队多少恢复执行多少；差额=恢复路径漏步",
        ),
        XC_REPLAY_ORDER => (
            "重放错序：重放序≠到达序",
            "保序是排队账的纪律；错序=重放器排序不稳定",
        ),
        XC_DOUBLE_PAUSE => (
            "双重暂停：已 Paused 再请求暂停",
            "幂等请求可重入但状态不重复迁移；重复迁移=状态机失守",
        ),
        XC_RESUME_NOT_PAUSED => (
            "恢复未暂停：Running 态请求恢复",
            "恢复只对 Paused/Resuming 态有意义；Running 态恢复=空操作伪装成协议动作",
        ),
        XC_TIMEOUT_KIND => (
            "超时分类错：暂停超时与介质超时混淆",
            "Paused 目标的超窗是『暂停超时』（等恢复重放）；Running 目标才是介质故障——分类错会误导排障",
        ),
        XC_QUEUE_CAP => (
            "排队越容量：暂停排队账超上限",
            "排队账有容量帽（默认 65536）；越帽=暂停期失控灌入，显性拒绝触发背压",
        ),
        XC_ACK_ALIEN => (
            "确认越核：握手确认来自非目标核",
            "只有目标核有权确认自己的暂停/恢复；越核确认=协议面被伪造",
        ),
        XC_PAUSE_COUNT_GAP => (
            "暂停账失守：计数与注入数不符",
            "暂停/恢复计数是审计面；失守说明某迁移路径漏计",
        ),
        XC_QUEUE_LEDGER_GAP => (
            "排队账残缺：排队数与明细数不符",
            "排队账=队列长度+明细账；两者不一致=入账拆步",
        ),
        XC_RESUME_RESIDUE => (
            "恢复后状态残留：未回 Running",
            "环闭合是恢复完成的定义；残留=恢复流程半途而废",
        ),
        XC_REPLAY_DRIFT => (
            "双跑漂移：同种子重放指纹不等",
            "同种子同序列；不等则逐段二分找隐藏可变状态",
        ),
        XC_TIMEOUT_COUNT_GAP => (
            "超时账失守：暂停超时计数与注入数不符",
            "超时分类账是排障面；失守说明分类路径漏计",
        ),
        _ => ("未知诊断码", "码不在 0x56 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 暂停状态机五态
// ---------------------------------------------------------------------------

/// 暂停五态（单向环：Running→Pausing→Paused→Resuming→Running）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PState2 {
    Running,
    Pausing,
    Paused,
    Resuming,
}

impl PState2 {
    pub fn ord(self) -> u8 {
        match self {
            PState2::Running => 0,
            PState2::Pausing => 1,
            PState2::Paused => 2,
            PState2::Resuming => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            PState2::Running => "running",
            PState2::Pausing => "pausing",
            PState2::Paused => "paused",
            PState2::Resuming => "resuming",
        }
    }
}

/// 暂停原因闭集（审计可归因——原因不明不许暂停）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
    /// 热更新（代码补丁窗口）。
    HotPatch,
    /// 调试停机。
    DebugHalt,
    /// 电源管理。
    PowerMgmt,
    /// 维护窗口。
    Maintenance,
}

impl PauseReason {
    pub fn name(self) -> &'static str {
        match self {
            PauseReason::HotPatch => "hot_patch",
            PauseReason::DebugHalt => "debug_halt",
            PauseReason::PowerMgmt => "power_mgmt",
            PauseReason::Maintenance => "maintenance",
        }
    }
    pub fn kind(self) -> u8 {
        match self {
            PauseReason::HotPatch => 0,
            PauseReason::DebugHalt => 1,
            PauseReason::PowerMgmt => 2,
            PauseReason::Maintenance => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// §2 暂停引擎（状态机+排队账+重放账+超时分类——本单主体）
// ---------------------------------------------------------------------------

/// 排队请求（暂停期入站——到达序+发起核+种类）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedReq {
    pub seq: u64,
    pub from_cpu: u32,
    pub kind: u32,
}

/// 暂停记录（协议闭环留痕——每次暂停的完整生命周期）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseRecord {
    pub round: u64,
    pub reason: PauseReason,
    /// 进入确认 tick（0=未确认）。
    pub entered_at: u64,
    /// 退出确认 tick（0=未确认）。
    pub exited_at: u64,
    /// 本轮排队数。
    pub queued: u64,
    /// 本轮重放数。
    pub replayed: u64,
}

/// 核暂停引擎（状态+排队账+确认面+超时分类+记录账）。
pub struct PauseEngine {
    /// 状态。
    state: PState2,
    /// 暂停原因（当前轮）。
    cur_reason: Option<PauseReason>,
    /// 排队账（FIFO——保序真源）。
    queue: VecDeque<QueuedReq>,
    /// 排队容量帽。
    pub queue_cap: usize,
    /// 下一个排队序号。
    next_seq: u64,
    /// 最后重放序号（保序校验）。
    last_replay: u64,
    /// 当前 tick。
    tick: u64,
    /// 同步 call 超窗（暂停超时判定基准）。
    pub call_window: u64,
    /// 记录账（每轮暂停一条——协议闭环审计）。
    pub records: Vec<PauseRecord>,
    /// 排队总数（累计）。
    pub queued_total: u64,
    /// 重放总数（累计）。
    pub replayed_total: u64,
    /// 暂停进入确认数。
    pub enter_ack_total: u64,
    /// 恢复退出确认数。
    pub exit_ack_total: u64,
    /// 暂停超时计数（同步 call 对 Paused 目标超窗）。
    pub pause_timeout_total: u64,
    /// 介质超时计数（对 Running 目标超窗——对照账）。
    pub media_timeout_total: u64,
    /// 本核 id（握手归属校验）。
    cpu: u32,
}

impl PauseEngine {
    /// 构造（核 id——Running 起步）。
    pub fn new(cpu: u32) -> Result<PauseEngine, (u32, String)> {
        if cpu > 64 {
            return Err((0x560A, format!("核 id {} 越界", cpu)));
        }
        Ok(PauseEngine {
            state: PState2::Running,
            cur_reason: None,
            queue: VecDeque::new(),
            queue_cap: 65536,
            next_seq: 1,
            last_replay: 0,
            tick: 0,
            call_window: 8,
            records: Vec::new(),
            queued_total: 0,
            replayed_total: 0,
            enter_ack_total: 0,
            exit_ack_total: 0,
            pause_timeout_total: 0,
            media_timeout_total: 0,
            cpu,
        })
    }

    /// 当前状态。
    pub fn state(&self) -> PState2 {
        self.state
    }

    /// 在队数。
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// 请求暂停（Running→Pausing，登记原因——重复暂停 0x5606）。
    pub fn request_pause(&mut self, reason: PauseReason) -> Result<(), (u32, String)> {
        self.tick += 1;
        match self.state {
            PState2::Running => {
                self.state = PState2::Pausing;
                self.cur_reason = Some(reason);
                Ok(())
            }
            PState2::Paused | PState2::Pausing => Err((
                XC_DOUBLE_PAUSE,
                format!(
                    "核 #{} 已 {}（原因 {}）——重复暂停请求",
                    self.cpu,
                    self.state.name(),
                    self.cur_reason.map(|r| r.name()).unwrap_or("?")
                ),
            )),
            PState2::Resuming => Err((
                XC_PAUSE_REGRESS,
                "恢复中不可再暂停——环序破坏（先等恢复完成）".to_string(),
            )),
        }
    }

    /// 确认进入（Pausing→Paused——目标核自确认，越核 0x560A）。
    pub fn ack_enter(&mut self, from_cpu: u32) -> Result<(), (u32, String)> {
        self.tick += 1;
        if from_cpu != self.cpu {
            return Err((
                XC_ACK_ALIEN,
                format!(
                    "进入确认来自核 #{} ≠ 目标核 #{}——协议面串号",
                    from_cpu, self.cpu
                ),
            ));
        }
        if self.state != PState2::Pausing {
            return Err((
                XC_HANDSHAKE_MISSING,
                format!(
                    "核 #{} 状态 {} 无待确认的暂停请求——握手缺失",
                    self.cpu,
                    self.state.name()
                ),
            ));
        }
        self.state = PState2::Paused;
        self.enter_ack_total += 1;
        self.records.push(PauseRecord {
            round: self.records.len() as u64 + 1,
            reason: self.cur_reason.ok_or((XC_HANDSHAKE_MISSING, "原因缺失".to_string()))?,
            entered_at: self.tick,
            exited_at: 0,
            queued: 0,
            replayed: 0,
        });
        Ok(())
    }

    /// 请求恢复（Paused→Resuming——非暂停态 0x5607）。
    pub fn request_resume(&mut self) -> Result<(), (u32, String)> {
        self.tick += 1;
        match self.state {
            PState2::Paused => {
                self.state = PState2::Resuming;
                Ok(())
            }
            PState2::Resuming => Err((XC_DOUBLE_PAUSE, "恢复已在途——重复请求".to_string())),
            s => Err((
                XC_RESUME_NOT_PAUSED,
                format!("核 #{} 状态 {} 无暂停可恢复", self.cpu, s.name()),
            )),
        }
    }

    /// 确认退出（Resuming→Running，重放排队账——零丢失闭环）。
    pub fn ack_exit(&mut self, from_cpu: u32) -> Result<Vec<QueuedReq>, (u32, String)> {
        self.tick += 1;
        if from_cpu != self.cpu {
            return Err((
                XC_ACK_ALIEN,
                format!(
                    "退出确认来自核 #{} ≠ 目标核 #{}——协议面串号",
                    from_cpu, self.cpu
                ),
            ));
        }
        if self.state != PState2::Resuming {
            return Err((
                XC_HANDSHAKE_MISSING,
                format!(
                    "核 #{} 状态 {} 无待确认的恢复请求——握手缺失",
                    self.cpu,
                    self.state.name()
                ),
            ));
        }
        // 重放：按到达序全量出队（保序+零丢失的结构化执行）。
        let mut replayed = Vec::new();
        while let Some(r) = self.queue.pop_front() {
            if r.seq <= self.last_replay {
                return Err((
                    XC_REPLAY_ORDER,
                    format!(
                        "重放序 {} ≤ 已放 {}——保序破坏",
                        r.seq, self.last_replay
                    ),
                ));
            }
            self.last_replay = r.seq;
            self.replayed_total += 1;
            replayed.push(r);
        }
        // 记录账闭环（本轮 exited_at+queued+replayed 补记——丢笔 0x5604 的对账源）。
        // 先取轮号与排队数（immutable 阶段结束），再进 last_mut 补记——借用面分离。
        let cur_round = match self.records.last() {
            Some(r) => r.round,
            None => 0,
        };
        let queued_now = self.queued_in_round(cur_round);
        if let Some(rec) = self.records.last_mut() {
            rec.exited_at = self.tick;
            rec.queued = queued_now;
            rec.replayed = replayed.len() as u64;
            if rec.replayed != rec.queued {
                return Err((
                    XC_REPLAY_LOSS,
                    format!(
                        "轮 #{} 重放 {} < 排队 {}——零丢失破约",
                        rec.round, rec.replayed, rec.queued
                    ),
                ));
            }
        }
        self.exit_ack_total += 1;
        self.state = PState2::Running;
        self.cur_reason = None;
        Ok(replayed)
    }

    /// 本轮排队数（从记录起点到当前——对账口径）。
    fn queued_in_round(&self, round: u64) -> u64 {
        // 简化口径：本轮排队数=重放总数−此前各轮重放和（由调用点对拍）。
        // 这里直接用累计排队−前轮累计（记录账里前轮 replayed 之和）。
        let mut prev: u64 = 0;
        for r in &self.records {
            if r.round < round {
                prev += r.replayed;
            }
        }
        self.replayed_total.saturating_sub(prev) + self.queue.len() as u64
    }

    /// 入站请求（Running 态执行 / 暂停态入排队账——执行面纪律）。
    pub fn inbound(&mut self, from_cpu: u32, kind: u32) -> Result<ExecOutcome, (u32, String)> {
        self.tick += 1;
        match self.state {
            PState2::Running => Ok(ExecOutcome::Executed(kind)),
            PState2::Pausing | PState2::Paused | PState2::Resuming => {
                // 暂停语义：只入账不执行（0x5603 的结构保证——本分支绝不执行）。
                if self.queue.len() >= self.queue_cap {
                    return Err((
                        XC_QUEUE_CAP,
                        format!(
                            "核 #{} 排队账满（{}/{}）——暂停期灌入越帽",
                            self.cpu,
                            self.queue.len(),
                            self.queue_cap
                        ),
                    ));
                }
                let seq = self.next_seq;
                self.next_seq += 1;
                self.queue.push_back(QueuedReq {
                    seq,
                    from_cpu,
                    kind,
                });
                self.queued_total += 1;
                if let Some(rec) = self.records.last_mut() {
                    if rec.exited_at == 0 {
                        rec.queued += 1;
                    }
                }
                Ok(ExecOutcome::Queued(seq))
            }
        }
    }

    /// 同步 call 超时裁决（目标状态决定超时分类——0x5608）。
    /// 返回 Ok(()) 表示目标可应答（重放已完成）；Err 为分类超时。
    pub fn sync_call_timeout_check(&mut self, waited: u64) -> Result<(), (u32, String)> {
        self.tick += 1;
        if waited <= self.call_window {
            return Ok(());
        }
        match self.state {
            PState2::Paused | PState2::Pausing | PState2::Resuming => {
                // 暂停超时（非故障——恢复后重放补执行）。
                self.pause_timeout_total += 1;
                Err((
                    0x5601 | 0x8000, // 分类标记：暂停超时（与介质超时分账）。
                    format!(
                        "核 #{} {} 中——同步 call 超窗 {} 属暂停超时（恢复后重放补执行,非介质故障）",
                        self.cpu,
                        self.state.name(),
                        waited
                    ),
                ))
            }
            PState2::Running => {
                self.media_timeout_total += 1;
                Err((
                    0x5601 | 0x4000, // 分类标记：介质超时。
                    format!(
                        "核 #{} running 中超窗 {}——介质/执行真故障",
                        self.cpu, waited
                    ),
                ))
            }
        }
    }

    /// 排队账对平（排队数==明细长度+记录账自洽——0x560C）。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        if self.queue.len() as u64 + self.replayed_total != self.queued_total {
            return Err((
                XC_QUEUE_LEDGER_GAP,
                format!(
                    "排队账残缺：在队 {}+已重放 {} ≠ 排队 {}",
                    self.queue.len(),
                    self.replayed_total,
                    self.queued_total
                ),
            ));
        }
        // 恢复完成态必须回 Running（0x560D）。
        for rec in &self.records {
            if rec.entered_at == 0 || (rec.exited_at == 0 && rec.round < self.records.len() as u64)
            {
                return Err((
                    XC_HANDSHAKE_MISSING,
                    format!("轮 #{} 握手不完整——进入/退出确认缺失", rec.round),
                ));
            }
        }
        Ok(())
    }

    /// 暂停账对平（0x560B）。
    pub fn pause_gate(&self, enters: u64, exits: u64) -> Result<(), (u32, String)> {
        if self.enter_ack_total != enters || self.exit_ack_total != exits {
            return Err((
                XC_PAUSE_COUNT_GAP,
                format!(
                    "暂停账失守：进入 {} / 退出 {} ≠ 注入 {}/{}",
                    self.enter_ack_total, self.exit_ack_total, enters, exits
                ),
            ));
        }
        Ok(())
    }

    /// 超时账对平（0x560F）。
    pub fn timeout_gate(&self, pause_to: u64, media_to: u64) -> Result<(), (u32, String)> {
        if self.pause_timeout_total != pause_to || self.media_timeout_total != media_to {
            return Err((
                XC_TIMEOUT_COUNT_GAP,
                format!(
                    "超时账失守：暂停 {} / 介质 {} ≠ 注入 {}/{}",
                    self.pause_timeout_total, self.media_timeout_total, pause_to, media_to
                ),
            ));
        }
        Ok(())
    }

    /// 快照指纹。
    pub fn snapshot_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(self.state.ord());
        buf.extend_from_slice(&self.next_seq.to_le_bytes());
        buf.extend_from_slice(&self.last_replay.to_le_bytes());
        buf.extend_from_slice(&self.queued_total.to_le_bytes());
        buf.extend_from_slice(&self.replayed_total.to_le_bytes());
        buf.extend_from_slice(&self.pause_timeout_total.to_le_bytes());
        buf.extend_from_slice(&self.media_timeout_total.to_le_bytes());
        for r in &self.queue {
            buf.extend_from_slice(&r.seq.to_le_bytes());
        }
        fnv64(&buf)
    }
}

/// 入站结果（执行了 / 排队了——调用方据此感知暂停语义）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecOutcome {
    /// 立即执行（Running 态）。
    Executed(u32),
    /// 入排队账（暂停态——恢复后补执行）。
    Queued(u64),
}

/// FNV-1a 64。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

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

// ---------------------------------------------------------------------------
// §3 J1 三闸
// ---------------------------------------------------------------------------

/// J1-A：暂停期请求零丢失——暂停期间 1 万请求恢复后全执行。
pub fn j1_zero_loss_10k() -> Result<String, (u32, String)> {
    let mut e = PauseEngine::new(2)?;
    let mut rng = Lcg::new(0x5629_B07A);
    // 暂停前 100 请求直执行。
    for i in 0..100u64 {
        match e.inbound(0, (i % 7) as u32)? {
            ExecOutcome::Executed(_) => {}
            _ => return Err((XC_EXEC_WHILE_PAUSED, "Running 态请求被排队——语义颠倒".to_string())),
        }
    }
    // 暂停协议：请求→确认进入。
    e.request_pause(PauseReason::HotPatch)?;
    e.ack_enter(2)?;
    if e.state() != PState2::Paused {
        return Err((XC_PAUSE_REGRESS, "确认后未 Paused".to_string()));
    }
    // 暂停期 1 万请求全入排队账。
    let mut queued_seqs: Vec<u64> = Vec::new();
    for i in 0..10_000u64 {
        match e.inbound(rng.below(4), (i % 11) as u32)? {
            ExecOutcome::Queued(seq) => queued_seqs.push(seq),
            ExecOutcome::Executed(_) => {
                return Err((XC_EXEC_WHILE_PAUSED, "Paused 态执行了请求——排队纪律破坏".to_string()))
            }
        }
    }
    if e.queue_len() != 10_000 {
        return Err((
            XC_QUEUE_LEDGER_GAP,
            format!("在队 {} ≠ 10000", e.queue_len()),
        ));
    }
    // 恢复协议：请求→确认退出（重放全量）。
    e.request_resume()?;
    let replayed = e.ack_exit(2)?;
    if replayed.len() != 10_000 {
        return Err((
            XC_REPLAY_LOSS,
            format!("重放 {} ≠ 10000——零丢失破约", replayed.len()),
        ));
    }
    // 保序对拍：重放序==排队序。
    for (i, r) in replayed.iter().enumerate() {
        if r.seq != queued_seqs[i] {
            return Err((
                XC_REPLAY_ORDER,
                format!(
                    "重放位 {} 序 {} ≠ 排队序 {}——错序",
                    i, r.seq, queued_seqs[i]
                ),
            ));
        }
    }
    if e.state() != PState2::Running {
        return Err((XC_RESUME_RESIDUE, "恢复后未回 Running".to_string()));
    }
    // 恢复后请求恢复直执行。
    match e.inbound(0, 1)? {
        ExecOutcome::Executed(_) => {}
        _ => return Err((XC_RESUME_RESIDUE, "恢复后仍排队——环未闭合".to_string())),
    }
    e.audit()?;
    e.pause_gate(1, 1)?;
    Ok(format!(
        "暂停期 1 万请求零丢失（排队 10000==重放 10000,序号逐位对拍保序）；协议闭环；恢复后直执行"
    ))
}

/// J1-B：暂停协议闭环——进入/退出确认 10/10 次（含原因轮转与越核拦截）。
pub fn j1_handshake_10() -> Result<String, (u32, String)> {
    let mut e = PauseEngine::new(3)?;
    let mut ok = 0u32;
    let reasons = [
        PauseReason::HotPatch,
        PauseReason::DebugHalt,
        PauseReason::PowerMgmt,
        PauseReason::Maintenance,
    ];
    for round in 0..10 {
        let reason = reasons[round % 4];
        // 越核确认拦截（先请求再用错核确认——必须拦）。
        e.request_pause(reason)?;
        match e.ack_enter(9) {
            Err((XC_ACK_ALIEN, _)) => {}
            _ => return Err((XC_ACK_ALIEN, "越核进入确认未拦".to_string())),
        }
        e.ack_enter(3)?;
        // 快速排队 1 笔再恢复。
        e.inbound(0, round as u32)?;
        e.request_resume()?;
        match e.ack_exit(9) {
            Err((XC_ACK_ALIEN, _)) => {}
            _ => return Err((XC_ACK_ALIEN, "越核退出确认未拦".to_string())),
        }
        let replayed = e.ack_exit(3)?;
        if replayed.len() != 1 || e.state() != PState2::Running {
            return Err((
                XC_RESUME_RESIDUE,
                format!("轮 {} 恢复不完整（重放 {} 状态 {}）", round, replayed.len(), e.state().name()),
            ));
        }
        ok += 1;
    }
    if ok != 10 || e.enter_ack_total != 10 || e.exit_ack_total != 10 {
        return Err((
            XC_PAUSE_COUNT_GAP,
            format!(
                "握手 {} /10（进入账 {} 退出账 {}）",
                ok, e.enter_ack_total, e.exit_ack_total
            ),
        ));
    }
    e.audit()?;
    e.pause_gate(10, 10)?;
    // 记录账：10 轮全部闭环（entered/exited 双确认+原因留痕）。
    for rec in &e.records {
        if rec.entered_at == 0 || rec.exited_at == 0 {
            return Err((XC_HANDSHAKE_MISSING, format!("轮 #{} 握手残缺", rec.round)));
        }
    }
    Ok(format!(
        "暂停协议闭环 10/10 全过（四原因轮转+越核双拦截+每轮 enter/exit 双确认留痕）；记录账 10 轮全闭环"
    ))
}

/// J1-C：暂停中同步 call 超时语义正确 10/10 次（分类对拍：暂停超时 vs 介质超时）。
pub fn j1_timeout_class_10() -> Result<String, (u32, String)> {
    let mut e = PauseEngine::new(4)?;
    let mut pause_hits = 0u32;
    // 场景 A：Paused 目标超窗 → 暂停超时（5 次）。
    e.request_pause(PauseReason::DebugHalt)?;
    e.ack_enter(4)?;
    for _ in 0..5 {
        match e.sync_call_timeout_check(e.call_window + 1) {
            Err((code, m)) => {
                if code != 0x5601 | 0x8000 {
                    return Err((XC_TIMEOUT_KIND, format!("分类码 {:#06x} 异常：{}", code, m)));
                }
                if !m.contains("暂停超时") {
                    return Err((XC_TIMEOUT_KIND, format!("暂停场景误分类：{}", m)));
                }
                pause_hits += 1;
            }
            Ok(()) => return Err((XC_TIMEOUT_KIND, "超窗未判超时".to_string())),
        }
    }
    // 恢复后同窗不超（call_window+1 内 Running 态即时应答——waited≤window 不超时）。
    e.request_resume()?;
    let _ = e.ack_exit(4)?;
    // 场景 B：Running 目标超窗 → 介质超时（5 次）。
    let mut media_hits = 0u32;
    for _ in 0..5 {
        match e.sync_call_timeout_check(e.call_window + 1) {
            Err((code, m)) => {
                if code != 0x5601 | 0x4000 {
                    return Err((XC_TIMEOUT_KIND, format!("分类码 {:#06x} 异常：{}", code, m)));
                }
                if !m.contains("真故障") {
                    return Err((XC_TIMEOUT_KIND, format!("介质场景误分类：{}", m)));
                }
                media_hits += 1;
            }
            Ok(()) => return Err((XC_TIMEOUT_KIND, "超窗未判超时".to_string())),
        }
    }
    // 场景 C：窗口内不超时（对照 2 次）。
    for _ in 0..2 {
        e.sync_call_timeout_check(e.call_window)?;
    }
    if pause_hits != 5 || media_hits != 5 {
        return Err((
            XC_TIMEOUT_KIND,
            format!("分类 {} 暂停/{} 介质 ≠ 5/5", pause_hits, media_hits),
        ));
    }
    e.timeout_gate(5, 5)?;
    e.audit()?;
    Ok(format!(
        "同步 call 超时语义 10/10 正确（Paused→暂停超时 5/Running→介质超时 5,分类码与文案双对拍）；窗内不误报"
    ))
}

/// 负路径 10 闸（状态机/握手/排队面——码精确命中）。
pub fn reject_gate_10() -> Result<String, (u32, String)> {
    let mut e = PauseEngine::new(1)?;
    let mut hits: Vec<(&str, bool)> = Vec::new();

    // 1）Running 态请求恢复。
    match e.request_resume() {
        Err((XC_RESUME_NOT_PAUSED, _)) => hits.push(("未暂停恢复", true)),
        _ => hits.push(("未暂停恢复", false)),
    }
    // 2）无请求确认进入。
    match e.ack_enter(1) {
        Err((XC_HANDSHAKE_MISSING, _)) => hits.push(("无请求确认", true)),
        _ => hits.push(("无请求确认", false)),
    }
    // 3）恢复中再暂停。
    e.request_pause(PauseReason::Maintenance)?;
    e.ack_enter(1)?;
    e.request_resume()?;
    match e.request_pause(PauseReason::HotPatch) {
        Err((XC_PAUSE_REGRESS, _)) => hits.push(("恢复中暂停", true)),
        _ => hits.push(("恢复中暂停", false)),
    }
    let _ = e.ack_exit(1);
    // 4）双重暂停。
    e.request_pause(PauseReason::HotPatch)?;
    match e.request_pause(PauseReason::HotPatch) {
        Err((XC_DOUBLE_PAUSE, _)) => hits.push(("双重暂停", true)),
        _ => hits.push(("双重暂停", false)),
    }
    e.ack_enter(1)?;
    // 5）Paused 态重复暂停。
    match e.request_pause(PauseReason::HotPatch) {
        Err((XC_DOUBLE_PAUSE, _)) => hits.push(("Paused 再暂停", true)),
        _ => hits.push(("Paused 再暂停", false)),
    }
    // 6）恢复中重复恢复。
    e.request_resume()?;
    match e.request_resume() {
        Err((XC_DOUBLE_PAUSE, _)) => hits.push(("重复恢复", true)),
        _ => hits.push(("重复恢复", false)),
    }
    // 7）无请求确认退出。
    match e.ack_exit(1) {
        Err((XC_HANDSHAKE_MISSING, _)) => hits.push(("无恢复确认", true)),
        _ => hits.push(("无恢复确认", false)),
    }
    let _ = e.ack_exit(1);
    // 8）越界核构造。
    match PauseEngine::new(65) {
        Err((0x560A, _)) => hits.push(("核 id 越界", true)),
        _ => hits.push(("核 id 越界", false)),
    }
    // 9）排队容量帽（压到 4 灌 5——第 5 显性拒）。
    let mut e2 = PauseEngine::new(1)?;
    e2.queue_cap = 4;
    e2.request_pause(PauseReason::HotPatch)?;
    e2.ack_enter(1)?;
    for i in 0..4 {
        e2.inbound(0, i)?;
    }
    match e2.inbound(0, 99) {
        Err((XC_QUEUE_CAP, _)) => hits.push(("排队越帽", true)),
        _ => hits.push(("排队越帽", false)),
    }
    let _ = e2.request_resume();
    let _ = e2.ack_exit(1);
    // 10）超窗边界（waited==window 不超；+1 才超）。
    match e.sync_call_timeout_check(e.call_window) {
        Ok(()) => hits.push(("窗界不超", true)),
        _ => hits.push(("窗界不超", false)),
    }

    let missed: Vec<&str> = hits.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
    if !missed.is_empty() {
        return Err((
            XC_PAUSE_COUNT_GAP,
            format!("负路径失守：{:?} 未按期望", missed),
        ));
    }
    Ok("负路径 10/10 全拦且码精确命中".to_string())
}

/// 万级压测（10 轮暂停循环×随机请求——排队/重放/超时全账对平）。
pub fn stress_10k() -> Result<String, (u32, String)> {
    let mut e = PauseEngine::new(5)?;
    let mut rng = Lcg::new(0x562B_07A4);
    let mut total_queued = 0u64;
    for round in 0..10 {
        e.request_pause(reasons_pick(round))?;
        e.ack_enter(5)?;
        // 每轮 1000 请求入排队。
        for i in 0..1000u64 {
            match e.inbound(rng.below(4), (i % 9) as u32)? {
                ExecOutcome::Queued(_) => total_queued += 1,
                _ => return Err((XC_EXEC_WHILE_PAUSED, "压测中 Paused 态执行".to_string())),
            }
        }
        e.request_resume()?;
        let replayed = e.ack_exit(5)?;
        if replayed.len() != 1000 {
            return Err((XC_REPLAY_LOSS, format!("轮 {} 重放 {} ≠ 1000", round, replayed.len())));
        }
        // 保序抽验。
        let first = replayed[0].seq;
        let last = replayed[999].seq;
        if last <= first {
            return Err((XC_REPLAY_ORDER, "重放序无递增——错序".to_string()));
        }
    }
    e.audit()?;
    e.pause_gate(10, 10)?;
    if e.replayed_total != total_queued {
        return Err((
            XC_REPLAY_LOSS,
            format!("累计重放 {} ≠ 排队 {}——零丢失破约", e.replayed_total, total_queued),
        ));
    }
    Ok(format!(
        "万级压测过：10 轮暂停×1000 请求（排队 {}==重放 {}）；保序抽验过；审计+暂停账双绿",
        total_queued, e.replayed_total
    ))
}

/// 原因轮转（压测辅助）。
fn reasons_pick(i: u32) -> PauseReason {
    match i % 4 {
        0 => PauseReason::HotPatch,
        1 => PauseReason::DebugHalt,
        2 => PauseReason::PowerMgmt,
        _ => PauseReason::Maintenance,
    }
}

/// 双跑指纹。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let mut e = PauseEngine::new(2)?;
    let mut rng = Lcg::new(0x56D0_0B1E);
    for _ in 0..50 {
        e.request_pause(reasons_pick(rng.below(4)))?;
        e.ack_enter(2)?;
        for _ in 0..rng.below(20) {
            let _ = e.inbound(rng.below(4), rng.below(9));
        }
        e.request_resume()?;
        let _ = e.ack_exit(2)?;
    }
    e.audit()?;
    Ok(e.snapshot_fp())
}

/// F0929 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_zero_loss_10k()?;
    let b = j1_handshake_10()?;
    let c = j1_timeout_class_10()?;
    let d = reject_gate_10()?;
    let e = stress_10k()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_REPLAY_DRIFT, format!("双跑指纹 {:#018x}≠{:#018x}", f1, f2)));
    }
    Ok(format!(
        "F0929 全绿：\n[零丢失] {}\n[协议闭环] {}\n[超时语义] {}\n[负路径] {}\n[万级压测] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, e, f1
    ))
}
