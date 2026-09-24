//! 交接状态机（篇 2.1）——五状态机七迁移，表驱动，零散落 if。
//!
//! 状态一到状态二的迁移由用户确认触发；preserving 可取消（分区里留
//! 废弃快照标记，下次交接清理）；flushing **不可取消**（文件系统日志
//! 冲刷一旦开始必须完成）；arming 任何一步失败迁移到 aborted 并给人话
//! 原因（武装失败等于没发生）；rebooting 只有一个出口——重启。
//!
//! 每次迁移写一条体验日志事件（谁触发、从哪到哪、耗时多少）——宪章
//! 第十三章的浮层生命周期要求在这里以状态机生命周期兑现。
//!
//! Q10（双向同时请求）：交接入口在 preserving 状态即锁定——第二次请求
//! 被拒绝并提示。在本实现里它不是特判，而是"没有这条表边"的自然结果
//! （表驱动状态机：表里没有的边就是拒绝），测试显式钉死这一语义。

/// 交接状态机的状态集。正向主干五步 + aborted 错误分支（见模块级文档
/// 的计数口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HState {
    /// 常态：一切照旧，交接入口按钮可见。
    Idle,
    /// 保全中：采集会话状态写交接分区，可取消。
    Preserving,
    /// 冲刷中：不可取消——界面显示"正在安全保存，请勿断电"。
    Flushing,
    /// 武装中：闸门三条件判定、写 OneShot 变量、校验写入。
    Arming,
    /// 重启中：显示交接画面直至内核交出最后控制权，唯一出口是重启。
    Rebooting,
    /// 武装失败：给人话原因与下一步指引，系统继续正常运行。
    Aborted,
}

impl HState {
    /// 人话名（诊断与日志用）。
    pub fn name(self) -> &'static str {
        match self {
            HState::Idle => "常态",
            HState::Preserving => "保全中",
            HState::Flushing => "冲刷中",
            HState::Arming => "武装中",
            HState::Rebooting => "重启中",
            HState::Aborted => "已中止",
        }
    }
}

/// 状态机的输入事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HEvent {
    /// 用户确认交接（点击"切换到 Windows"或反向按钮）。
    RequestHandoff,
    /// 保全中用户反悔（一切照旧，分区里留废弃快照标记）。
    CancelPreserve,
    /// 保全完成（快照写好、校验过）。
    PreserveDone,
    /// 冲刷完成（五步全部在预算内走完）。
    FlushDone,
    /// 武装成功（闸门过 + OneShot 写入且十次读回一致）。
    ArmOk,
    /// 武装失败（闸门拒绝 / 写读不一致且兜底也失败）。
    ArmFail,
    /// 用户确认中止（看完人话原因与指引，回到常态）。
    AbortAck,
}

impl HEvent {
    /// 人话名（体验日志的"谁触发"列）。
    pub fn name(self) -> &'static str {
        match self {
            HEvent::RequestHandoff => "用户确认交接",
            HEvent::CancelPreserve => "用户取消保全",
            HEvent::PreserveDone => "保全完成",
            HEvent::FlushDone => "冲刷完成",
            HEvent::ArmOk => "武装成功",
            HEvent::ArmFail => "武装失败",
            HEvent::AbortAck => "用户确认中止",
        }
    }
}

/// 一条转移表目：从哪、经什么事件、到哪、为什么。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Transition {
    pub from: HState,
    pub on: HEvent,
    pub to: HState,
    /// 这条边存在的设计理由（评审用，不是注释是数据）。
    pub why: &'static str,
}

/// 转移表——**七条边，一表定协议**。任何不在表里的 (状态, 事件) 组合
/// 一律拒绝（返回 [`step`] 的 Err），拒绝永远不改变状态。
pub const TRANSITIONS: [Transition; 7] = [
    Transition {
        from: HState::Idle,
        on: HEvent::RequestHandoff,
        to: HState::Preserving,
        why: "状态一到状态二的迁移由用户确认触发",
    },
    Transition {
        from: HState::Preserving,
        on: HEvent::CancelPreserve,
        to: HState::Idle,
        why: "可取消（用户反悔则一切照旧，分区里留下废弃快照标记）",
    },
    Transition {
        from: HState::Preserving,
        on: HEvent::PreserveDone,
        to: HState::Flushing,
        why: "保全完成才允许冲刷——顺序是硬性的",
    },
    Transition {
        from: HState::Flushing,
        on: HEvent::FlushDone,
        to: HState::Arming,
        why: "最后才允许进入武装状态（篇 2.4 冲刷顺序）",
    },
    Transition {
        from: HState::Arming,
        on: HEvent::ArmOk,
        to: HState::Rebooting,
        why: "写入并读回校验通过才进入 rebooting",
    },
    Transition {
        from: HState::Arming,
        on: HEvent::ArmFail,
        to: HState::Aborted,
        why: "任何一步失败迁移到 aborted 并给出人话原因与下一步指引",
    },
    Transition {
        from: HState::Aborted,
        on: HEvent::AbortAck,
        to: HState::Idle,
        why: "武装失败等于没发生——确认后一切照旧",
    },
];

/// 表驱动的纯转移函数。表里没有的边一律拒绝，拒绝给出人话原因。
pub fn step(state: HState, event: HEvent) -> Result<HState, Rejected> {
    for t in TRANSITIONS.iter() {
        if t.from == state && t.on == event {
            return Ok(t.to);
        }
    }
    Err(Rejected { state, event, reason: reject_reason(state, event) })
}

/// 拒绝的人话原因词表。特化常被点名的组合，兜底给出通用指引。
fn reject_reason(state: HState, event: HEvent) -> &'static str {
    match (state, event) {
        // Q10：交接入口在 preserving 状态即锁定，第二次请求被拒绝并提示。
        (HState::Preserving, HEvent::RequestHandoff) => {
            "交接正在进行中——请等这次交接走完再发起新的"
        }
        // flushing 不可取消：冲刷一旦开始必须完成。
        (HState::Flushing, HEvent::CancelPreserve) => {
            "正在安全保存，请勿断电——此步骤不可取消"
        }
        (HState::Rebooting, _) => "交接已进入重启，此刻没有可响应的操作",
        (HState::Idle, HEvent::CancelPreserve) => "当前没有正在进行的交接",
        (HState::Idle, HEvent::AbortAck) => "当前没有需要确认的中止",
        _ => "当前状态下这个操作没有意义",
    }
}

/// 被拒绝的事件（原状态原样保留——拒绝永远不是迁移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rejected {
    pub state: HState,
    pub event: HEvent,
    pub reason: &'static str,
}

/// 体验日志事件（篇 2.1：谁触发、从哪到哪、耗时多少）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogEvent {
    /// 迁移时刻（毫秒，调用方注入的钟——状态机不持有钟）。
    pub at_ms: u64,
    /// 这一步耗时（毫秒，进入上一状态到本次迁移的间隔）。
    pub took_ms: u64,
    pub from: HState,
    pub to: HState,
    pub event: HEvent,
}

/// 交接状态机会话。状态转移全走 [`TRANSITIONS`] 表，每次成功迁移落
/// 一条体验日志；拒绝事件也落日志（结果=拒绝，状态不变）——日志面
/// "全程说实话"。
#[derive(Debug)]
pub struct HandoffMachine {
    state: HState,
    entered_at_ms: u64,
    log: alloc::vec::Vec<LogEvent>,
}

impl HandoffMachine {
    /// 新会话：从 idle 开局，`now_ms` 是本会话的时钟零点。
    pub fn new(now_ms: u64) -> HandoffMachine {
        HandoffMachine { state: HState::Idle, entered_at_ms: now_ms, log: alloc::vec::Vec::new() }
    }

    pub fn state(&self) -> HState {
        self.state
    }

    /// 体验日志（成功迁移 + 被拒绝事件都有记录）。
    pub fn log(&self) -> &[LogEvent] {
        &self.log
    }

    /// 提交一个事件。成功迁移返回新状态；被拒绝返回 Err 且状态原样。
    pub fn submit(&mut self, event: HEvent, now_ms: u64) -> Result<HState, Rejected> {
        match step(self.state, event) {
            Ok(to) => {
                let took = now_ms.saturating_sub(self.entered_at_ms);
                self.log.push(LogEvent {
                    at_ms: now_ms,
                    took_ms: took,
                    from: self.state,
                    to,
                    event,
                });
                self.state = to;
                self.entered_at_ms = now_ms;
                Ok(to)
            }
            Err(r) => {
                // 拒绝也入账：体验日志记"谁在什么状态试了什么、被什么话挡回"。
                self.log.push(LogEvent {
                    at_ms: now_ms,
                    took_ms: 0,
                    from: self.state,
                    to: self.state,
                    event,
                });
                Err(r)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // B-201 主判据：七迁移全覆盖（表驱动测试含异常分支）。
    // 表里的每条边逐一走通；表外组合逐一被拒。
    #[test]
    fn b201_seven_transitions_all_reachable() {
        // 1. idle --RequestHandoff--> preserving
        // 2. preserving --CancelPreserve--> idle
        // 3. preserving --PreserveDone--> flushing
        // 4. flushing --FlushDone--> arming
        // 5. arming --ArmOk--> rebooting
        // 6. arming --ArmFail--> aborted
        // 7. aborted --AbortAck--> idle
        assert_eq!(step(HState::Idle, HEvent::RequestHandoff), Ok(HState::Preserving));
        assert_eq!(step(HState::Preserving, HEvent::CancelPreserve), Ok(HState::Idle));
        assert_eq!(step(HState::Preserving, HEvent::PreserveDone), Ok(HState::Flushing));
        assert_eq!(step(HState::Flushing, HEvent::FlushDone), Ok(HState::Arming));
        assert_eq!(step(HState::Arming, HEvent::ArmOk), Ok(HState::Rebooting));
        assert_eq!(step(HState::Arming, HEvent::ArmFail), Ok(HState::Aborted));
        assert_eq!(step(HState::Aborted, HEvent::AbortAck), Ok(HState::Idle));
        // 转移表本身恰好七条，一条不多不少。
        assert_eq!(TRANSITIONS.len(), 7);
    }

    #[test]
    fn b201_happy_path_walks_the_spine() {
        let mut m = HandoffMachine::new(0);
        assert_eq!(m.submit(HEvent::RequestHandoff, 100), Ok(HState::Preserving));
        assert_eq!(m.submit(HEvent::PreserveDone, 900), Ok(HState::Flushing));
        assert_eq!(m.submit(HEvent::FlushDone, 9_500), Ok(HState::Arming));
        assert_eq!(m.submit(HEvent::ArmOk, 11_000), Ok(HState::Rebooting));
        // 正向主干恰好五步状态、四条迁移边；每步耗时入账。
        let log = m.log();
        assert_eq!(log.len(), 4);
        assert_eq!(log[2].took_ms, 9_500 - 900, "冲刷步耗时=进入时刻差");
        assert_eq!(log[3].took_ms, 11_000 - 9_500);
    }

    #[test]
    fn b201_exception_branch_cancel_and_abort() {
        // 取消分支：preserving 反悔回 idle，可再次发起。
        let mut m = HandoffMachine::new(0);
        m.submit(HEvent::RequestHandoff, 10).unwrap();
        assert_eq!(m.submit(HEvent::CancelPreserve, 500), Ok(HState::Idle));
        assert_eq!(m.submit(HEvent::RequestHandoff, 600), Ok(HState::Preserving));
        // 失败分支：arming 失败进 aborted，确认后回 idle，一切照旧。
        let mut m2 = HandoffMachine::new(0);
        m2.submit(HEvent::RequestHandoff, 10).unwrap();
        m2.submit(HEvent::PreserveDone, 20).unwrap();
        m2.submit(HEvent::FlushDone, 30).unwrap();
        assert_eq!(m2.submit(HEvent::ArmFail, 40), Ok(HState::Aborted));
        assert_eq!(m2.submit(HEvent::AbortAck, 999), Ok(HState::Idle));
    }

    #[test]
    fn b201_q10_double_request_locked_at_preserving() {
        // Q10：交接入口在 preserving 状态即锁定，第二次请求被拒绝并提示。
        let mut m = HandoffMachine::new(0);
        m.submit(HEvent::RequestHandoff, 10).unwrap();
        let r = m.submit(HEvent::RequestHandoff, 20);
        assert!(r.is_err(), "preserving 中的第二次请求必须被拒");
        let r = r.unwrap_err();
        assert_eq!(r.state, HState::Preserving, "拒绝不是迁移：状态原样");
        assert!(r.reason.contains("正在进行"), "拒绝要有提示语: {}", r.reason);
        // 日志也如实记了这次被拒的尝试。
        assert_eq!(m.log().last().unwrap().event, HEvent::RequestHandoff);
        assert_eq!(m.state(), HState::Preserving);
    }

    #[test]
    fn b201_flushing_cannot_be_cancelled() {
        // 冲刷不可取消：表里没有 (Flushing, CancelPreserve) 这条边。
        let mut m = HandoffMachine::new(0);
        m.submit(HEvent::RequestHandoff, 0).unwrap();
        m.submit(HEvent::PreserveDone, 0).unwrap();
        let r = m.submit(HEvent::CancelPreserve, 100);
        assert!(r.is_err());
        assert!(r.unwrap_err().reason.contains("请勿断电"));
        assert_eq!(m.state(), HState::Flushing, "冲刷中取消被拒后状态原样");
    }

    #[test]
    fn b201_rebooting_has_no_event_exit() {
        // rebooting 唯一出口是物理重启（协议终点事件），任何事件都拒绝。
        for e in [
            HEvent::RequestHandoff,
            HEvent::CancelPreserve,
            HEvent::PreserveDone,
            HEvent::FlushDone,
            HEvent::ArmOk,
            HEvent::ArmFail,
            HEvent::AbortAck,
        ] {
            assert!(step(HState::Rebooting, e).is_err(), "rebooting 必须拒绝 {:?}", e);
        }
    }

    #[test]
    fn b201_zero_dead_end_states() {
        // 零个无出口状态：每个状态要么有事件出口，要么有协议明文的
        // 物理出口（rebooting=重启）。
        for s in [
            HState::Idle,
            HState::Preserving,
            HState::Flushing,
            HState::Arming,
            HState::Rebooting,
            HState::Aborted,
        ] {
            let has_event_exit = TRANSITIONS.iter().any(|t| t.from == s);
            let physical_exit = s == HState::Rebooting; // 出口=重启（协议终点）
            assert!(has_event_exit || physical_exit, "{:?} 是无出口状态", s);
        }
        // aborted 有且仅有 AbortAck 一条事件出口。
        let aborted_edges: alloc::vec::Vec<HEvent> =
            TRANSITIONS.iter().filter(|t| t.from == HState::Aborted).map(|t| t.on).collect();
        assert_eq!(aborted_edges, alloc::vec![HEvent::AbortAck]);
    }

    #[test]
    fn b201_rejects_never_mutate_state_and_log() {
        // 任意状态提交无意义事件：状态不变 + 日志入账。
        let mut m = HandoffMachine::new(0);
        assert!(m.submit(HEvent::ArmOk, 5).is_err(), "idle 直接 ArmOk 必须被拒");
        assert_eq!(m.state(), HState::Idle);
        assert_eq!(m.log().len(), 1, "拒绝也入账");
        assert_eq!(m.log()[0].to, HState::Idle);
    }
}
