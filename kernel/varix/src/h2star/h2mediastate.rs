//! H2 可移动介质状态机 · 深化批次五（F293 接入询问 × F294 安全弹出
//! 的全生命周期贯通——挂载状态机单点实现）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F293 接入询问**：接入即弹询问条（10s 静默收起——计时由调用
//!   方喂）、记住选择持久化、**Autorun 零执行**（状态机上不存在
//!   「自动运行」转移——安全红线的状态级保证）；
//! - **F294 安全弹出**：写入中弹出 = 拦截（Busy 态只出不进）；
//!   就绪弹出走冲刷→卸载→可拔三拍；强拔（未弹出即物理拔出）→
//!   重挂时损坏标注生命周期；
//! - **F184 车道（经 F293 锚）**：插入-写入-弹出-拔出全链录屏的
//!   状态序列由本机判产出——演练账即状态机轨迹。
//!
//! 状态机纪律：十态全遍历封闭性；每个事件在不合法状态下显式拒绝
//! （拒绝留因——零「静默吞事件」）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 状态机
// ---------------------------------------------------------------------------

/// 介质状态（十态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaState {
    /// 未接入。
    Absent,
    /// 已接入，询问条挂着（10s 窗口内）。
    Asking,
    /// 询问收起（用户没选——可从托盘/设置再进入）。
    Idle,
    /// 已挂载就绪（可读写）。
    Mounted,
    /// 写入中（弹出拦截窗口）。
    Busy,
    /// 冲刷中（弹出第一拍）。
    Flushing,
    /// 卸载中（弹出第二拍）。
    Unmounting,
    /// 可安全拔出（弹出第三拍——气泡已提示）。
    Ejectable,
    /// 强拔后的怀疑损坏（重挂时标注）。
    Suspect,
    /// 已拔出（终态——下次接入重新走机器）。
    Ejected,
}

/// 介质事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaEvent {
    PlugIn,
    /// 询问条超时收起（10s 静默——F293 判据）。
    AskTimeout,
    Mount,
    WriteStart,
    WriteEnd,
    /// 用户点「安全弹出」。
    EjectRequest,
    /// 三拍完成（冲刷+卸载成功）。
    FlushDone,
    /// 物理拔出（无论哪个状态——硬件事件不等人）。
    PhysicallyRemoved,
    /// 重挂（Suspect 态的标注检查点：完全重写→解除标注）。
    ReplugVerified,
    MountRefused,
}

/// 十秒询问窗口（F293 判据原值）。
pub const ASK_WINDOW_MS: u32 = 10_000;

/// 一块介质的状态机（单盘一实例——多盘多实例，无共享账）。
pub struct MediaDrive {
    pub label: String,
    pub state: MediaState,
    /// 轨迹（演练账=状态序列——F184 全链录屏的机判对偶）。
    pub trace: Vec<MediaEvent>,
    /// 被拒事件账（状态不合法时的显式拒绝——零静默）。
    pub rejected: Vec<(MediaEvent, &'static str)>,
    /// 被拦截的弹出次数（Busy 弹出——F294 拦截判据计数）。
    pub eject_blocks: u32,
}

impl MediaDrive {
    pub fn new(label: &str) -> MediaDrive {
        MediaDrive {
            label: label.into(),
            state: MediaState::Absent,
            trace: Vec::new(),
            rejected: Vec::new(),
            eject_blocks: 0,
        }
    }

    /// 事件喂入：状态机唯一入口。返回是否接受。
    pub fn feed(&mut self, ev: MediaEvent) -> bool {
        let from = self.state;
        let next = match (from, ev) {
            (MediaState::Absent, MediaEvent::PlugIn) => Some(MediaState::Asking),
            (MediaState::Asking, MediaEvent::AskTimeout) => Some(MediaState::Idle),
            (MediaState::Asking | MediaState::Idle, MediaEvent::Mount) => Some(MediaState::Mounted),
            (MediaState::Mounted, MediaEvent::WriteStart) => Some(MediaState::Busy),
            (MediaState::Busy, MediaEvent::WriteEnd) => Some(MediaState::Mounted),
            (MediaState::Mounted, MediaEvent::EjectRequest) => Some(MediaState::Flushing),
            (MediaState::Busy, MediaEvent::EjectRequest) => {
                self.eject_blocks += 1;
                self.rejected
                    .push((ev, "写入中不能弹出——等写完或取消写入"));
                None
            }
            (MediaState::Flushing, MediaEvent::FlushDone) => Some(MediaState::Unmounting),
            (MediaState::Unmounting, MediaEvent::FlushDone) => Some(MediaState::Ejectable),
            (MediaState::Suspect, MediaEvent::ReplugVerified) => Some(MediaState::Mounted),
            (_, MediaEvent::PhysicallyRemoved) => {
                // 强拔：任何挂载族状态物理拔出 → 拔出态；此前在写入
                // 则留下损坏怀疑标记（重挂走 Suspect）。
                self.state = if matches!(
                    from,
                    MediaState::Busy | MediaState::Suspect
                ) {
                    MediaState::Suspect
                } else {
                    MediaState::Ejected
                };
                self.trace.push(ev);
                return true;
            }
            (MediaState::Suspect | MediaState::Ejected, MediaEvent::PlugIn) => {
                // 重挂：Suspect → 回到怀疑标注（等待验证）；已完好的
                // Ejected → 正常询问。
                self.state = if from == MediaState::Suspect {
                    MediaState::Suspect
                } else {
                    MediaState::Asking
                };
                self.trace.push(ev);
                return true;
            }
            (MediaState::Idle, MediaEvent::MountRefused) => Some(MediaState::Idle),
            _ => None,
        };
        match next {
            Some(s) => {
                self.state = s;
                self.trace.push(ev);
                true
            }
            None => {
                if self.rejected.last().map(|(e, _)| *e) != Some(ev) {
                    self.rejected
                        .push((ev, "当前状态下不接受该事件"));
                }
                false
            }
        }
    }

    /// 损坏怀疑是否解除（F294 强拔标注生命周期：完全重写→解除）。
    pub fn suspect_cleared(&self) -> bool {
        self.state == MediaState::Mounted && !self.trace.contains(&MediaEvent::ReplugVerified)
            || !matches!(self.state, MediaState::Suspect)
    }

    /// 弹出进度三拍位（F294 进度说明：1 冲刷 / 2 卸载 / 3 可拔）。
    pub fn eject_phase(&self) -> Option<u8> {
        match self.state {
            MediaState::Flushing => Some(1),
            MediaState::Unmounting => Some(2),
            MediaState::Ejectable => Some(3),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2mediastate_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2mediastate");
    // 正常全链：接入→询问→挂载→写→完成→弹出三拍→拔出。
    let mut d = MediaDrive::new("VARIX-KEY");
    for ev in [
        MediaEvent::PlugIn,
        MediaEvent::Mount,
        MediaEvent::WriteStart,
        MediaEvent::WriteEnd,
        MediaEvent::EjectRequest,
        MediaEvent::FlushDone,
        MediaEvent::FlushDone,
        MediaEvent::PhysicallyRemoved,
    ] {
        assert!(d.feed(ev));
    }
    set.add(
        "h2mediastate full chain",
        d.state == MediaState::Ejected && d.trace.len() == 8,
        "plug→mount→write→eject→remove",
    );
    // 询问超时：10s 静默收起（AskTimeout → Idle）。
    let mut d2 = MediaDrive::new("备件盘");
    d2.feed(MediaEvent::PlugIn);
    d2.feed(MediaEvent::AskTimeout);
    set.add(
        "h2mediastate ask collapse",
        d2.state == MediaState::Idle,
        "10s silent collapse",
    );
    // Busy 弹出拦截：计数显性化 + 状态不动 + 拒绝留因。
    let mut d3 = MediaDrive::new("写入中");
    d3.feed(MediaEvent::PlugIn);
    d3.feed(MediaEvent::Mount);
    d3.feed(MediaEvent::WriteStart);
    let blocked = d3.feed(MediaEvent::EjectRequest);
    set.add(
        "h2mediastate busy block",
        !blocked
            && d3.state == MediaState::Busy
            && d3.eject_blocks == 1
            && d3.rejected[0].1.contains("写入中"),
        "eject blocked with reason",
    );
    // 强拔：写入中物理拔出 → Suspect；完好盘拔出 → Ejected。
    d3.feed(MediaEvent::PhysicallyRemoved);
    set.add(
        "h2mediastate hard remove suspect",
        d3.state == MediaState::Suspect,
        "write-interrupt marks suspect",
    );
    let mut d4 = MediaDrive::new("完好盘");
    d4.feed(MediaEvent::PlugIn);
    d4.feed(MediaEvent::PhysicallyRemoved);
    set.add(
        "h2mediastate clean remove",
        d4.state == MediaState::Ejected,
        "no write no suspect",
    );
    // Suspect 生命周期：重挂保持怀疑 → 验证解除。
    d3.feed(MediaEvent::PlugIn);
    set.add(
        "h2mediastate replug keeps suspect",
        d3.state == MediaState::Suspect,
        "label survives replug",
    );
    d3.feed(MediaEvent::ReplugVerified);
    set.add(
        "h2mediastate suspect cleared",
        d3.state == MediaState::Mounted && d3.suspect_cleared(),
        "full rewrite clears label",
    );
    // 状态不合法事件显式拒绝：Absent 态不给 Mount。
    let mut d5 = MediaDrive::new("空位");
    let refused = d5.feed(MediaEvent::Mount);
    set.add(
        "h2mediastate illegal visible",
        !refused && d5.state == MediaState::Absent && !d5.rejected.is_empty(),
        "no silent swallow",
    );
    // 三拍位查询 + 询问窗口常量。
    let mut d6 = MediaDrive::new("拍位");
    d6.feed(MediaEvent::PlugIn);
    d6.feed(MediaEvent::Mount);
    d6.feed(MediaEvent::EjectRequest);
    let p1 = d6.eject_phase();
    d6.feed(MediaEvent::FlushDone);
    let p2 = d6.eject_phase();
    d6.feed(MediaEvent::FlushDone);
    set.add(
        "h2mediastate eject phases",
        p1 == Some(1) && p2 == Some(2) && d6.eject_phase() == Some(3),
        "flush→unmount→ejectable",
    );
    set.add(
        "h2mediastate ask window",
        ASK_WINDOW_MS == 10_000,
        "10s line",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2mediastate_all_green() {
        let set = run_h2mediastate_checks();
        assert!(set.all_passed(), "h2mediastate 自检有红项");
        assert!(!set.truncated(), "h2mediastate 自检溢出");
    }

    #[test]
    fn all_states_total_function() {
        // 十态 × 十一事件全遍历：任何喂入不 panic、状态恒在十态之内
        // （状态机封闭性——十四章公理的机判形态）。
        let states = [
            MediaState::Absent, MediaState::Asking, MediaState::Idle, MediaState::Mounted,
            MediaState::Busy, MediaState::Flushing, MediaState::Unmounting, MediaState::Ejectable,
            MediaState::Suspect, MediaState::Ejected,
        ];
        let events = [
            MediaEvent::PlugIn, MediaEvent::AskTimeout, MediaEvent::Mount, MediaEvent::WriteStart,
            MediaEvent::WriteEnd, MediaEvent::EjectRequest, MediaEvent::FlushDone,
            MediaEvent::PhysicallyRemoved, MediaEvent::ReplugVerified, MediaEvent::MountRefused,
        ];
        for st in states {
            let mut d = MediaDrive::new("遍历");
            d.state = st;
            for ev in events {
                d.feed(ev);
                assert!(matches!(
                    d.state,
                    MediaState::Absent | MediaState::Asking | MediaState::Idle
                        | MediaState::Mounted | MediaState::Busy | MediaState::Flushing
                        | MediaState::Unmounting | MediaState::Ejectable
                        | MediaState::Suspect | MediaState::Ejected
                ));
                d.state = st; // 重置回归
            }
        }
    }
}
