//! C15 · 内核键位事件消费循环（壳C 输入实现层）。
//!
//! 同一份键位配置（ca-core keymap 域），三壳仅注册目标不同：
//! 壳A 系统级窗口焦点 / 壳B vwm 转发 / 壳C **内核事件端口订阅**。
//!
//! 循环骨架与分发逻辑是纯逻辑，宿主可测；内核事件源
//! （[`varix_std::event::EventPort`]）只出现在 `target_os = "none"` 门后。
//!
//! 事件类别号镜像 `varix-std/src/abi.rs`（EVENT_KEY=1 等），
//! 漂移会让订阅掩码排不上队。

/// 内核事件类别镜像（对齐 `varix-std::abi`）。
pub const EVENT_KEY: u32 = 1;
pub const EVENT_TIMER: u32 = 2;
pub const EVENT_SIGNAL: u32 = 3;
pub const EVENT_IO: u32 = 4;
pub const EVENT_EXIT: u32 = 5;

/// 订阅掩码位（`event_bit` 同式）。
pub const fn event_bit(kind: u32) -> u64 {
    let k = if kind > 63 { 63 } else { kind };
    1u64 << k
}

/// 壳C 订阅掩码：键位 + 退出信号。别的一概不排（内核侧省队列）。
pub const SUBSCRIBE_MASK: u64 = event_bit(EVENT_KEY) | event_bit(EVENT_EXIT);

/// 一条已解码事件（与 `varix_std::event::Event` 同构，宿主可构造）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawEvent {
    pub kind: u32,
    /// 键事件：data[0] = 键码，data[1] = 修饰键位图。
    pub data: [u64; 2],
}

impl RawEvent {
    pub const fn key(keycode: u64, mods: u64) -> Self {
        RawEvent { kind: EVENT_KEY, data: [keycode, mods] }
    }

    pub const fn exit() -> Self {
        RawEvent { kind: EVENT_EXIT, data: [0, 0] }
    }
}

/// 事件源抽象：内核端口（none）或注入序列（宿主测试）。
pub trait EventSource {
    /// 取一条事件；队列空返回 `None`（不阻塞，与 `EventPort::poll` 语义对齐）。
    fn poll(&mut self) -> Option<RawEvent>;
}

/// 循环统计（C24 验收物：壳C 行为可量化）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoopStats {
    pub polled: u64,
    pub key_events: u64,
    pub quit_requested: bool,
}

/// 键位分发回调签名：返回 `false` 请求退出循环。
pub type KeyHandler<'a> = dyn FnMut(RawEvent) -> bool + 'a;

/// 事件循环：取空队列 → 键事件交处理器 → EXIT 置退出位。
/// 队列空即返回（单帧驱动；由外部决定是否继续下一帧）。
pub fn run_frame(src: &mut impl EventSource, on_key: &mut KeyHandler) -> LoopStats {
    let mut st = LoopStats::default();
    while let Some(ev) = src.poll() {
        st.polled += 1;
        match ev.kind {
            EVENT_KEY => {
                st.key_events += 1;
                if !on_key(ev) {
                    st.quit_requested = true;
                    return st;
                }
            }
            EVENT_EXIT => {
                st.quit_requested = true;
                return st;
            }
            _ => {} // 未订阅类别不该出现；出现即丢弃（防御内核侧掩码漂移）
        }
    }
    st
}

/// 简易帧驱动：直到退出位或队列连续空 `max_idle` 帧。
pub fn run_until_quit(
    src: &mut impl EventSource,
    on_key: &mut KeyHandler,
    max_idle: u32,
) -> LoopStats {
    let mut total = LoopStats::default();
    let mut idle = 0;
    while idle < max_idle {
        let st = run_frame(src, on_key);
        total.polled += st.polled;
        total.key_events += st.key_events;
        if st.quit_requested {
            total.quit_requested = true;
            return total;
        }
        idle += 1;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 注入序列源。
    struct Seq {
        events: Vec<RawEvent>,
        i: usize,
    }

    impl Seq {
        fn new(events: Vec<RawEvent>) -> Self {
            Seq { events, i: 0 }
        }
    }

    impl EventSource for Seq {
        fn poll(&mut self) -> Option<RawEvent> {
            if self.i < self.events.len() {
                let e = self.events[self.i];
                self.i += 1;
                Some(e)
            } else {
                None
            }
        }
    }

    #[test]
    fn dispatches_keys_and_counts() {
        let mut src = Seq::new(vec![
            RawEvent::key(65, 0), // 'A'
            RawEvent::key(83, 1), // Ctrl+S
            RawEvent::key(27, 0), // Esc
        ]);
        let mut keys = Vec::new();
        let st = run_frame(&mut src, &mut |ev| {
            keys.push(ev.data[0]);
            true
        });
        assert_eq!(st.polled, 3);
        assert_eq!(st.key_events, 3);
        assert!(!st.quit_requested);
        assert_eq!(keys, vec![65, 83, 27]);
    }

    #[test]
    fn handler_can_request_quit() {
        let mut src = Seq::new(vec![RawEvent::key(65, 0), RawEvent::key(27, 0), RawEvent::key(66, 0)]);
        let st = run_frame(&mut src, &mut |ev| ev.data[0] != 27);
        assert!(st.quit_requested);
        assert_eq!(st.polled, 2); // 第三条没被消费
    }

    #[test]
    fn exit_event_stops_loop() {
        let mut src = Seq::new(vec![RawEvent::key(65, 0), RawEvent::exit()]);
        let st = run_frame(&mut src, &mut |_| true);
        assert!(st.quit_requested);
        assert_eq!(st.key_events, 1);
    }

    #[test]
    fn unknown_kinds_are_dropped() {
        let mut src = Seq::new(vec![
            RawEvent { kind: 42, data: [0, 0] },
            RawEvent::key(65, 0),
        ]);
        let st = run_frame(&mut src, &mut |_| true);
        assert_eq!(st.polled, 2);
        assert_eq!(st.key_events, 1);
    }

    #[test]
    fn idle_frames_end_run_until_quit() {
        let mut src = Seq::new(vec![RawEvent::key(65, 0)]);
        let st = run_until_quit(&mut src, &mut |_| true, 3);
        assert!(!st.quit_requested);
        assert_eq!(st.polled, 1);
    }

    #[test]
    fn subscribe_mask_is_key_plus_exit() {
        assert_eq!(SUBSCRIBE_MASK, event_bit(EVENT_KEY) | event_bit(EVENT_EXIT));
        // 未订阅的类别位必须为 0（内核侧不排队的承诺）。
        assert_eq!(SUBSCRIBE_MASK & event_bit(EVENT_TIMER), 0);
        assert_eq!(SUBSCRIBE_MASK & event_bit(EVENT_IO), 0);
    }
}
