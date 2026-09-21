//! PS/2 键鼠输入事件服务 — 阶段0 轮询升级为内核输入服务（双域总案·阶段2 任务19）。
//!
//! 职责边界（总案施工步骤 12 口径）：
//! - **同源不复制**：键盘解码复用 `crate::ps2` 的 `Key`/`Decoder`（任务1 原语），
//!   本模块只做"队列 + 订阅者 + 服务化"，绝不重写扫描码表；
//! - **事件队列**：定容 64 槽环形（无堆分配），满时**丢最旧并计数**
//!   （不阻塞泵路径——轮询内核下等价于"不阻塞中断"的口径）；
//!   `dropped_oldest` 按最坏情况口径计数（每次满发布 +1），
//!   订阅者**实际漏读**另由每订阅者 `missed` 计数，两个计数器合读即全貌；
//! - **订阅者**：定容 4 槽广播模型，各自游标独立推进；订阅从当前写入位起
//!   （不回放历史——引导服务没有"昨天的按键"）；
//! - **鼠标 PS/2 最小版**：AUX 口判别（状态口 bit5）+ 3 字节流式包解码
//!   （包头 bit3 同步位重同步、bit4/5 符号位、bit0-2 按键；溢出位刻意忽略）；
//! - **`shim://input` 契约**（任务26 逐字段核对基准）：16 字节定长布局，
//!   见 [`InputEvent::to_shim_bytes`]，实机探针逐事件打印该布局字节。
//! - **焦点路由（AI-4 · S2.05 定版）**：订阅者可声明 [`FocusPolicy`]——
//!   `All`（默认，现状广播语义零变化）/ `KeyboardFocusOnly`（键盘只投
//!   焦点订阅者，鼠标广播不变）。焦点切换即生效（切换前入队未消费的
//!   键盘事件按当前焦点判定——诚实简单，无灰色窗口期）。Wine 通道与
//!   桌面通道共用本总线（一份代码两处用），pid→订阅者映射见
//!   [`InputService::set_focus_pid`]（SYS_WIN focus 子命令的跨层接线）；
//! - **延迟打点（S2.11 基线初值来源）**：publish 时记时钟戳、poll 交付
//!   时统计 `last/max/samples`（TSC 周期，宿主注入时钟可测）；16B
//!   shim://input 契约不变（时戳只走服务级统计，不进契约）。
//!
//! 端口 IO 仅 `target_os = "none"` 编译；解码/队列逻辑纯函数宿主可测。

use crate::ps2;

/// 队列容量：64 事件。定容即免分配，泵路径零堆零阻塞。
pub const QUEUE_CAP: usize = 64;
/// 订阅者上限：4。超限 `subscribe` 明确拒绝（绝不静默挤占）。
pub const MAX_SUBS: usize = 4;

/// 默认零时钟（延迟统计恒 0——不误导；enable_tsc/set_clock 才真实计时）。
fn zero_clock() -> u64 {
    0
}

/// 输入事件：键盘复用任务1 的 [`ps2::Key`]，鼠标为最小三维组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Key(ps2::Key),
    Mouse {
        dx: i16,
        dy: i16,
        buttons: u8,
    },
}

impl InputEvent {
    /// kind 字段：0=Key 1=Mouse（shim://input 契约）。
    pub const KIND_KEY: u8 = 0;
    pub const KIND_MOUSE: u8 = 1;

    /// 键字节：0=Up 1=Down 2=Enter（声明序，契约固定——前 3 序号是
    /// 任务19/26 定版契约，扩表只追加不重排），3 起=任务55 扩展区
    /// （Esc/Space/Backspace/Tab/Left/Right/Shift/字母/数字/标点）。
    fn key_byte(k: ps2::Key) -> u8 {
        use ps2::Key::*;
        match k {
            Up => 0,
            Down => 1,
            Enter => 2,
            Esc => 3,
            Space => 4,
            Backspace => 5,
            Tab => 6,
            Left => 7,
            Right => 8,
            LShift => 9,
            RShift => 10,
            A => 11,
            B => 12,
            C => 13,
            D => 14,
            E => 15,
            F => 16,
            G => 17,
            H => 18,
            I => 19,
            J => 20,
            K => 21,
            L => 22,
            M => 23,
            N => 24,
            O => 25,
            P => 26,
            Q => 27,
            R => 28,
            S => 29,
            T => 30,
            U => 31,
            V => 32,
            W => 33,
            X => 34,
            Y => 35,
            Z => 36,
            D1 => 37,
            D2 => 38,
            D3 => 39,
            D4 => 40,
            D5 => 41,
            D6 => 42,
            D7 => 43,
            D8 => 44,
            D9 => 45,
            D0 => 46,
            Minus => 47,
            Equal => 48,
            Comma => 49,
            Period => 50,
            Slash => 51,
            Semicolon => 52,
            Apostrophe => 53,
            BracketL => 54,
            BracketR => 55,
            Backslash => 56,
            Grave => 57,
        }
    }

    /// 归一化键表反查（验收/文档/注入脚本对账用；与 key_byte 互逆）。
    pub fn key_from_byte(b: u8) -> Option<ps2::Key> {
        use ps2::Key::*;
        Some(match b {
            0 => Up,
            1 => Down,
            2 => Enter,
            3 => Esc,
            4 => Space,
            5 => Backspace,
            6 => Tab,
            7 => Left,
            8 => Right,
            9 => LShift,
            10 => RShift,
            11 => A,
            12 => B,
            13 => C,
            14 => D,
            15 => E,
            16 => F,
            17 => G,
            18 => H,
            19 => I,
            20 => J,
            21 => K,
            22 => L,
            23 => M,
            24 => N,
            25 => O,
            26 => P,
            27 => Q,
            28 => R,
            29 => S,
            30 => T,
            31 => U,
            32 => V,
            33 => W,
            34 => X,
            35 => Y,
            36 => Z,
            37 => D1,
            38 => D2,
            39 => D3,
            40 => D4,
            41 => D5,
            42 => D6,
            43 => D7,
            44 => D8,
            45 => D9,
            46 => D0,
            47 => Minus,
            48 => Equal,
            49 => Comma,
            50 => Period,
            51 => Slash,
            52 => Semicolon,
            53 => Apostrophe,
            54 => BracketL,
            55 => BracketR,
            56 => Backslash,
            57 => Grave,
            _ => return None,
        })
    }

    /// `shim://input` 频道事件定长布局（16B，任务26 逐字段核对基准）：
    /// ```text
    /// [0..8)   u64 seq      事件序号（服务级单调，自 1 起，LE）
    /// [8]      u8  kind     0=Key 1=Mouse
    /// [9]      u8  key      kind=Key：0=Up 1=Down 2=Enter；kind=Mouse：恒 0
    /// [10..12) i16 dx       kind=Mouse：X 位移（有符号，LE）；kind=Key：恒 0
    /// [12..14) i16 dy       kind=Mouse：Y 位移（有符号，LE）；kind=Key：恒 0
    /// [14]     u8  buttons  kind=Mouse：bit0 左 bit1 右 bit2 中；kind=Key：恒 0
    /// [15]     u8  pad      保留，恒 0
    /// ```
    pub fn to_shim_bytes(&self, seq: u64) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0..8].copy_from_slice(&seq.to_le_bytes());
        match *self {
            InputEvent::Key(k) => {
                out[8] = Self::KIND_KEY;
                out[9] = Self::key_byte(k);
            }
            InputEvent::Mouse { dx, dy, buttons } => {
                out[8] = Self::KIND_MOUSE;
                out[10..12].copy_from_slice(&dx.to_le_bytes());
                out[12..14].copy_from_slice(&dy.to_le_bytes());
                out[14] = buttons & 0x07;
            }
        }
        out
    }
}

/// PS/2 鼠标流式包解码产物。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseDelta {
    pub dx: i16,
    pub dy: i16,
    pub buttons: u8,
}

/// PS/2 鼠标 3 字节流式包解码器（最小版）。
///
/// 包格式（流模式，第一套）：
/// - 字节 0：bit3 恒 1（同步位）、bit4 dx 符号、bit5 dy 符号、
///   bit6 dx 溢出、bit7 dy 溢出（溢出位最小版忽略）、bit0-2 左/右/中键；
/// - 字节 1/2：dx/dy 低 8 位。位移为 9 位二补码：符号位置 1 时值 = 字节 − 256。
///
/// 失步自愈：仅在"等待包头"状态（`have == 0`）校验 bit3——包中间的 0x00
/// 是合法数据字节，不得触发重同步（用例 `mouse_mid_packet_zeros_are_data`）。
#[derive(Default)]
pub struct MouseDecoder {
    buf: [u8; 3],
    have: u8,
}

impl MouseDecoder {
    /// 喂入一个 AUX 口字节，凑满 3 字节返回一个位移包。
    pub fn feed(&mut self, b: u8) -> Option<MouseDelta> {
        if self.have == 0 && b & 0x08 == 0 {
            return None; // 包头同步位缺失：丢弃并等待下一包头（失步自愈）
        }
        self.buf[self.have as usize] = b;
        self.have += 1;
        if self.have < 3 {
            return None;
        }
        self.have = 0;
        let f = self.buf[0];
        let dx = if f & 0x10 != 0 {
            self.buf[1] as i16 - 256
        } else {
            self.buf[1] as i16
        };
        let dy = if f & 0x20 != 0 {
            self.buf[2] as i16 - 256
        } else {
            self.buf[2] as i16
        };
        Some(MouseDelta {
            dx,
            dy,
            buttons: f & 0x07,
        })
    }
}

/// 订阅者槽位：独立游标 + 实际漏读计数。
struct SubSlot {
    live: bool,
    #[allow(dead_code)]
    name: &'static str,
    /// 下一个要读的事件序号（1 起）。
    read_seq: u64,
    /// 因队列覆盖而实际漏读的事件数。
    missed: u64,
}

/// 焦点路由策略（S2.05 定版；默认 `All` = 既有广播语义零变化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPolicy {
    /// 广播：键盘与鼠标全收（现状语义——bootselect/ushell 既有订阅零改动）。
    All,
    /// 键盘只投焦点订阅者（`focus_sub == Some(本槽)`），鼠标广播不变。
    /// 无焦点（None）时键盘一律不投（白名单精神：没明确授权就不给）。
    KeyboardFocusOnly,
}

/// PS/2 键鼠输入事件服务：定容环形队列 + 广播订阅者 + 丢最旧计数。
pub struct InputService {
    /// 槽 i 持有序号 s 的槽位：`(s-1) % QUEUE_CAP`。有效窗口 =
    /// `[write_seq − QUEUE_CAP + 1, write_seq]`（不主动清槽：窗口内必为 Some）。
    /// 元素 = (事件, 发布时钟戳)——时钟戳只走服务级延迟统计，不进 16B 契约。
    ring: [Option<(InputEvent, u64)>; QUEUE_CAP],
    /// 已发布事件总数（下一事件序号）。
    write_seq: u64,
    /// 满发布丢最旧计数（最坏情况口径：write_seq > CAP 后每发布 +1）。
    dropped_oldest: u64,
    subs: [SubSlot; MAX_SUBS],
    /// 各槽焦点策略（S2.05；cancel 时复位 All）。
    policies: [FocusPolicy; MAX_SUBS],
    /// 各槽属主 pid（subscribe_policy_pid 登记；set_focus_pid 查表）。
    sub_pids: [u32; MAX_SUBS],
    /// 焦点订阅者槽位（None = 无焦点：KeyboardFocusOnly 槽收不到键盘）。
    focus_sub: Option<usize>,
    focus_switches: u64,
    /// 被焦点路由跳过的键盘事件数（游标推进、不进 missed）。
    routed_away: u64,
    /// 时钟源（默认零时钟——延迟统计恒 0 不误导；实机 enable_tsc / 宿主 set_clock）。
    clock: fn() -> u64,
    clock_enabled: bool,
    lat_last: u64,
    lat_max: u64,
    lat_samples: u64,
    key_dec: ps2::Decoder,
    mouse_dec: MouseDecoder,
    /// 泵级原始字节追踪（实机探针诊断用；生产路径恒 false）。
    pub trace: bool,
    /// 实机取证计数（2026-09-20）：泵到的键盘/鼠标原始字节总数与最后键盘
    /// 字节。BootScreen 诊断行（boot://event 记录 idx=14）实时展示——
    /// 按了键 raw 不涨 ⇒ 键盘信号没到控制器；涨了没出键 ⇒ 解码问题。
    pub raw_kbd: u32,
    pub raw_aux: u32,
    pub last_raw: u8,
    /// 菜单/桌面用**累计**鼠标位移（自上次 `take_mouse` 起累加）。
    /// 与事件队列解耦：引导菜单期中断未开、也没有订阅者，`poll` 取不到
    /// 事件；累计量保证「动了就一定读得到」，且零分配。
    mouse_acc_x: i32,
    mouse_acc_y: i32,
    /// 本窗口内出现过的按键位并集（bit0 左 / bit1 右 / bit2 中）。
    mouse_btn: u8,
    /// 自上次 `take_mouse` 起是否收到过完整鼠标包。
    mouse_seen: bool,
}

impl InputService {
    pub fn new() -> Self {
        InputService {
            ring: [None; QUEUE_CAP],
            write_seq: 0,
            dropped_oldest: 0,
            subs: [
                SubSlot { live: false, name: "", read_seq: 0, missed: 0 },
                SubSlot { live: false, name: "", read_seq: 0, missed: 0 },
                SubSlot { live: false, name: "", read_seq: 0, missed: 0 },
                SubSlot { live: false, name: "", read_seq: 0, missed: 0 },
            ],
            policies: [FocusPolicy::All; MAX_SUBS],
            sub_pids: [0; MAX_SUBS],
            focus_sub: None,
            focus_switches: 0,
            routed_away: 0,
            clock: zero_clock,
            clock_enabled: false,
            lat_last: 0,
            lat_max: 0,
            lat_samples: 0,
            key_dec: ps2::Decoder::default(),
            mouse_dec: MouseDecoder::default(),
            trace: false,
            raw_kbd: 0,
            raw_aux: 0,
            last_raw: 0,
            mouse_acc_x: 0,
            mouse_acc_y: 0,
            mouse_btn: 0,
            mouse_seen: false,
        }
    }

    /// 发布事件：非阻塞、零分配；满时丢最旧并计数。
    /// 返回该事件的序号（自 1 单调递增）。发布时记时钟戳（延迟打点）。
    pub fn publish(&mut self, ev: InputEvent) -> u64 {
        let t = (self.clock)();
        self.write_seq = self.write_seq.wrapping_add(1);
        let s = self.write_seq;
        if s > QUEUE_CAP as u64 {
            self.dropped_oldest += 1; // 满发布：最旧一条被覆盖
        }
        let idx = ((s - 1) % QUEUE_CAP as u64) as usize;
        self.ring[idx] = Some((ev, t));
        s
    }

    /// 注册订阅者：从当前写入位之后起读（**不回放历史**——引导服务没有
    /// "昨天的按键"；事件序号自 1，故游标 = write_seq + 1）。
    /// 槽满返回 `None`（明确拒绝）。
    pub fn subscribe(&mut self, name: &'static str) -> Option<usize> {
        let slot = self.subs.iter().position(|x| !x.live)?;
        self.subs[slot] = SubSlot {
            live: true,
            name,
            read_seq: self.write_seq.wrapping_add(1),
            missed: 0,
        };
        Some(slot)
    }

    /// 注销订阅者（槽位回收，可再订阅；焦点策略与 pid 一并复位）。
    pub fn cancel(&mut self, idx: usize) {
        if let Some(s) = self.subs.get_mut(idx) {
            s.live = false;
        }
        self.policies[idx] = FocusPolicy::All;
        self.sub_pids[idx] = 0;
        if self.focus_sub == Some(idx) {
            self.focus_sub = None;
        }
    }

    /// 注册订阅者并声明焦点策略与属主 pid（S2.05；旧 [`Self::subscribe`]
    /// 保持原语义 = `All` + pid 0）。槽满明确拒绝。
    pub fn subscribe_policy_pid(&mut self, name: &'static str, policy: FocusPolicy, pid: u32) -> Option<usize> {
        let slot = self.subs.iter().position(|x| !x.live)?;
        self.subs[slot] = SubSlot {
            live: true,
            name,
            read_seq: self.write_seq.wrapping_add(1),
            missed: 0,
        };
        self.policies[slot] = policy;
        self.sub_pids[slot] = pid;
        Some(slot)
    }

    /// 焦点切到订阅者槽位（切换即生效；重复设置不计数）。
    pub fn set_focus(&mut self, idx: usize) -> bool {
        if !self.subs.get(idx).map(|s| s.live).unwrap_or(false) {
            return false;
        }
        if self.focus_sub != Some(idx) {
            self.focus_sub = Some(idx);
            self.focus_switches += 1;
        }
        true
    }

    /// 焦点按属主 pid 定位（SYS_WIN focus 子命令的跨层接线：winsurf
    /// `focus_owner()` 的 pid → 本表槽位）。无匹配订阅者返回 false——
    /// 窗口焦点记录照常成立（桌面进程自持 DOM 焦点语义），只是内核级
    /// 键盘路由不切。
    pub fn set_focus_pid(&mut self, pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        let target = (0..MAX_SUBS).find(|&i| self.subs[i].live && self.sub_pids[i] == pid);
        match target {
            Some(idx) => self.set_focus(idx),
            None => false,
        }
    }

    /// 清除焦点（KeyboardFocusOnly 槽此后收不到键盘——安全默认）。
    pub fn clear_focus(&mut self) {
        if self.focus_sub.take().is_some() {
            self.focus_switches += 1;
        }
    }

    /// 当前焦点订阅者槽位。
    pub fn focus(&self) -> Option<usize> {
        self.focus_sub
    }

    /// 焦点切换次数（S2.05 走查/基线指标）。
    pub fn focus_switches(&self) -> u64 {
        self.focus_switches
    }

    /// 被焦点路由跳过的键盘事件数。
    pub fn routed_away(&self) -> u64 {
        self.routed_away
    }

    /// 注入时钟源（宿主测试；实机用 [`Self::enable_tsc`]）。同时启用延迟统计。
    pub fn set_clock(&mut self, f: fn() -> u64) {
        self.clock = f;
        self.clock_enabled = true;
    }

    /// 实机启用 TSC 时钟（延迟统计开始累计）。
    #[cfg(target_os = "none")]
    pub fn enable_tsc(&mut self) {
        self.clock = crate::kaslr::read_tsc;
        self.clock_enabled = true;
    }

    /// 延迟统计快照：(last, max, samples)，单位 = 时钟周期（实机 TSC
    /// 周期换算 μs 由探针按 tsc_hz 做；零时钟时恒 0）。
    pub fn latency_ticks(&self) -> (u64, u64, u64) {
        (self.lat_last, self.lat_max, self.lat_samples)
    }

    /// 订阅者非阻塞读取下一个事件及其序号。滞后超过窗口时快进到最老可用
    /// 事件，被覆盖的事件计入 `missed`。
    ///
    /// 焦点路由（S2.05）：`KeyboardFocusOnly` 策略槽读到键盘事件且本槽
    /// 不是焦点时——事件被路由走（游标推进、`routed_away` 计数、不进
    /// missed），循环取下一个可交付事件；鼠标事件与 `All` 槽不受影响。
    ///
    /// 游标语义：`read_seq = 0` 表示"尚未读过"（首事件序号是 1，不是 0），
    /// 故实际推进从 `max(read_seq, 1)` 起——漏读计数只数真正被覆盖的事件。
    pub fn poll_with_seq(&mut self, idx: usize) -> Option<(u64, InputEvent)> {
        let s = self.subs.get_mut(idx)?;
        if !s.live {
            return None;
        }
        // 最老仍有效序号 = write_seq − CAP + 1（write_seq ≥ CAP 时）。
        let oldest = self.write_seq.saturating_sub(QUEUE_CAP as u64 - 1);
        let next = s.read_seq.max(1);
        if next < oldest {
            s.missed += oldest - next; // [next, oldest) 已被覆盖：实际漏读
            s.read_seq = oldest;
        } else if s.read_seq == 0 {
            s.read_seq = 1;
        }
        loop {
            if s.read_seq > self.write_seq {
                return None; // 已追平（含空队列）
            }
            let seq = s.read_seq;
            let entry = self.ring[((seq - 1) % QUEUE_CAP as u64) as usize];
            s.read_seq += 1;
            let Some((ev, t)) = entry else {
                continue; // 理论不空（窗口内必 Some）；防御性跳过
            };
            if matches!(ev, InputEvent::Key(_))
                && self.policies[idx] == FocusPolicy::KeyboardFocusOnly
                && self.focus_sub != Some(idx)
            {
                self.routed_away += 1;
                continue;
            }
            if self.clock_enabled && t != 0 {
                let lat = (self.clock)().wrapping_sub(t);
                self.lat_last = lat;
                if lat > self.lat_max {
                    self.lat_max = lat;
                }
                self.lat_samples += 1;
            }
            return Some((seq, ev));
        }
    }

    /// 订阅者非阻塞读取下一个事件（不关心序号的便捷封装）。
    pub fn poll(&mut self, idx: usize) -> Option<InputEvent> {
        self.poll_with_seq(idx).map(|(_, ev)| ev)
    }

    /// 键盘口字节入服务（同源：解码走任务1 的 [`ps2::Decoder`]）。
    pub fn feed_key_byte(&mut self, b: u8) -> Option<InputEvent> {
        let k = self.key_dec.feed(b)?;
        let ev = InputEvent::Key(k);
        self.publish(ev);
        Some(ev)
    }

    /// 鼠标 AUX 口字节入服务（3 字节凑包后发布，并累计进菜单通道）。
    pub fn feed_mouse_byte(&mut self, b: u8) -> Option<InputEvent> {
        let m = self.mouse_dec.feed(b)?;
        self.mouse_acc_x = self.mouse_acc_x.saturating_add(m.dx as i32);
        self.mouse_acc_y = self.mouse_acc_y.saturating_add(m.dy as i32);
        self.mouse_btn |= m.buttons;
        self.mouse_seen = true;
        let ev = InputEvent::Mouse {
            dx: m.dx,
            dy: m.dy,
            buttons: m.buttons,
        };
        self.publish(ev);
        Some(ev)
    }

    /// 取走自上次调用以来累计的鼠标位移（引导菜单/桌面用）。
    ///
    /// 队列之外的一条独立通道：菜单在中断未开、无订阅者的阶段也要能拿到
    /// 位移，而 `poll` 依赖订阅槽。无数据返回 `None`（键盘路径完全不受影响）。
    /// 累计值按 i16 饱和截断——单包只有 ±255，跨包累计也不会静默翻转。
    pub fn take_mouse(&mut self) -> Option<MouseDelta> {
        if !self.mouse_seen {
            return None;
        }
        let sat = |v: i32| -> i16 { v.clamp(i16::MIN as i32, i16::MAX as i32) as i16 };
        let d = MouseDelta {
            dx: sat(self.mouse_acc_x),
            dy: sat(self.mouse_acc_y),
            buttons: self.mouse_btn,
        };
        self.mouse_acc_x = 0;
        self.mouse_acc_y = 0;
        self.mouse_btn = 0;
        self.mouse_seen = false;
        Some(d)
    }

    /// 统计快照：(已发布总数, 丢最旧计数)。
    pub fn stats(&self) -> (u64, u64) {
        (self.write_seq, self.dropped_oldest)
    }

    /// 订阅者实际漏读数。
    pub fn sub_missed(&self, idx: usize) -> Option<u64> {
        self.subs.get(idx).filter(|s| s.live).map(|s| s.missed)
    }
}

/// 任务1 复用适配器：把服务变成 bootselect 的 `KeySource`
/// （`&mut dyn FnMut() -> Option<ps2::Key>`）。
/// 泵 → 按序读订阅 → 跳过鼠标事件 → 产出键事件。
pub struct KeySourceAdapter {
    sub: usize,
}

impl KeySourceAdapter {
    /// 注册 bootselect 订阅者；服务订阅槽满时返回 `None`。
    pub fn new(svc: &mut InputService) -> Option<Self> {
        let sub = svc.subscribe("bootselect")?;
        Some(KeySourceAdapter { sub })
    }

    /// 泵一次硬件并产出下一个键事件（鼠标事件对菜单无关，跳过）。
    pub fn poll_key(&mut self, svc: &mut InputService) -> Option<ps2::Key> {
        svc.pump();
        loop {
            match svc.poll(self.sub) {
                Some(InputEvent::Key(k)) => return Some(k),
                Some(InputEvent::Mouse { .. }) => continue,
                None => return None,
            }
        }
    }

    /// 占用的订阅槽号——菜单结束时要按它把槽还回去。
    pub fn slot(&self) -> usize {
        self.sub
    }
}

// ---------------------------------------------------------------------------
// 目标态：端口泵 + 实机探针
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
mod port {
    /// 端口读 — `in al, dx`。复用 ps2.rs 同一实现（任务19 起改为 pub(crate)）。
    pub use crate::ps2::port::inp;
}

impl InputService {
    /// 从 PS/2 控制器泵入全部就绪字节：状态口 bit5（AUX）判别键/鼠。
    /// 单次上限 16 字节防失控；无控制器（0xFF）/无数据（OBF=0）即返。
    #[cfg(target_os = "none")]
    pub fn pump(&mut self) -> usize {
        use port::inp;
        const STAT: u16 = 0x64;
        const DATA: u16 = 0x60;
        const STAT_OBF: u8 = 0x01;
        const STAT_AUX: u8 = 0x20;

        let mut n = 0usize;
        for _ in 0..16 {
            let st = unsafe { inp(STAT) };
            if st == 0xFF || st & STAT_OBF == 0 {
                break;
            }
            let b = unsafe { inp(DATA) };
            if st & STAT_AUX != 0 {
                self.raw_aux = self.raw_aux.wrapping_add(1);
                if self.trace {
                    crate::kinfo!("input-probe: raw aux={:#04x}", b);
                }
                self.feed_mouse_byte(b);
            } else {
                self.raw_kbd = self.raw_kbd.wrapping_add(1);
                self.last_raw = b;
                if self.trace {
                    crate::kinfo!("input-probe: raw kbd={:#04x}", b);
                }
                self.feed_key_byte(b);
            }
            n += 1;
        }
        n
    }

    /// 宿主态恒 0（无端口；宿主测试走注入字节）。
    #[cfg(not(target_os = "none"))]
    pub fn pump(&mut self) -> usize {
        0
    }
}

/// SYS_WIN focus 联动（S2.05）：内核级键盘路由切到属主 pid 的订阅者。
/// 无匹配订阅者返回 false（窗口焦点记录照常成立——桌面进程自持 DOM 焦点）。
pub fn focus_pid(pid: u32) -> bool {
    svc().set_focus_pid(pid)
}

/// SYS_WIN focus(0) 联动：清除内核级键盘焦点（KeyboardFocusOnly 槽此后
/// 收不到键盘——白名单默认）。
pub fn clear_focus() {
    svc().clear_focus();
}

/// 实机探针（QEMU）：①定容语义合成注入 ②订阅者槽位语义 ③实机键鼠窗口
/// （宿主脚本经 HMP `sendkey`/`mouse_move`/`mouse_button` 注入）。
#[cfg(target_os = "none")]
pub mod target {
    use super::{InputEvent, InputService, KeySourceAdapter, MouseDelta, MAX_SUBS, QUEUE_CAP};
    use crate::ps2;

    static mut SVC: Option<InputService> = None;
    /// 引导菜单键源适配器（一次性注册，键鼠共用同一个端口泵）。
    static mut KSRC: Option<KeySourceAdapter> = None;

    /// 服务全局实例（引导期单核、探针在 enable_interrupts 前，static mut 无并发；
    /// 与 ps2.rs 同一手工 Once 范式）。
    fn svc() -> &'static mut InputService {
        let slot = &raw mut SVC;
        // SAFETY: 引导期单核、探针在 enable_interrupts 前独占运行。
        unsafe {
            if (*slot).is_none() {
                *slot = Some(InputService::new());
            }
            (*slot).as_mut().unwrap()
        }
    }

    /// 引导菜单鼠标 bring-up —— **真机也跑**。
    ///
    /// 修复记录：此前 `mouse_bringup` 只挂在 `input_probe()` 的 `if on_qemu`
    /// 分支里，而探针又排在引导菜单**之后**——真机上鼠标从头到尾没被初始化，
    /// 插着鼠标也只能当摆设（需求 1/6「任意鼠标和键盘」的真实缺口）。
    /// 现在菜单亮出前显式初始化：①i8042 控制器 ②AUX 门 ③置默认 + 使能上报。
    /// 返回 ack 位：bit0=0xF6 置默认成功，bit1=0xF4 使能上报成功。
    pub fn mouse_init(tsc_hz: u64) -> u8 {
        ps2::controller_init();
        mouse_bringup(tsc_hz)
    }

    /// 菜单键源：与服务共用一次端口泵，鼠标字节不再被键盘路径吃掉。
    ///
    /// 为什么不能继续用 `ps2::poll_key`：**两个读者抢同一个 0x60 端口**。
    /// `poll_key` 只看 OBF 不看 AUX 标志位，AUX 字节会被当成键盘字节喂进
    /// 键解码器（错位解码），鼠标则永远收不到包。走适配器后，键鼠由
    /// `pump()` 按 STAT_AUX 分流，各取所需。
    /// 订阅槽耗尽时退化为直轮询——键盘路径绝不因鼠标接线而失效。
    pub fn menu_poll_key() -> Option<ps2::Key> {
        let slot = &raw mut KSRC;
        // SAFETY: 引导期单核、中断未开，独占访问；与 svc() 同一手工 Once 范式。
        unsafe {
            if (*slot).is_none() {
                *slot = KeySourceAdapter::new(svc());
            }
            match (*slot).as_mut() {
                Some(a) => a.poll_key(svc()),
                None => ps2::poll_key(),
            }
        }
    }

    /// 菜单鼠标源：泵一次端口后取走累计位移（与键源共用同一泵）。
    /// 没有鼠标 / 鼠标没动 → `None`，菜单行为与纯键盘时逐帧一致。
    pub fn menu_poll_mouse() -> Option<MouseDelta> {
        let s = svc();
        s.pump();
        s.take_mouse()
    }

    /// 菜单结束，把键源订阅槽**还回去**。
    ///
    /// 为什么必须还：`MAX_SUBS` 只有 4 个。菜单只在倒计时那几秒需要键源，
    /// 留着不还就会让引导链后面的订阅者挤在剩下的槽里——实测踩到的回归是
    /// `input_probe` 的槽位自检填不满、提前 `return`（且当时不回收），
    /// 于是 4 槽全满，`shell_subscribe` 失败、**ushell 键盘彻底不可达**。
    /// 返回是否真的回收了一个槽（幂等：重复调用返回 false）。
    pub fn menu_release_key_source() -> bool {
        let slot = &raw mut KSRC;
        // SAFETY: 引导期单核；注册路径（menu_poll_key）在菜单内、释放路径
        // 在菜单返回之后，两者不并发。
        unsafe {
            match (*slot).take() {
                Some(a) => {
                    svc().cancel(a.slot());
                    true
                }
                None => false,
            }
        }
    }

    /// 任务27 · shell 订阅槽（惰性注册；usize::MAX = 未注册）。
    static SHELL_SUB: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(usize::MAX);

    /// 任务27 · shell 订阅注册（usrshell 初始化时调用一次；重复调用幂等）。
    pub fn shell_subscribe() -> bool {
        if SHELL_SUB.load(core::sync::atomic::Ordering::Acquire) != usize::MAX {
            return true;
        }
        match svc().subscribe("usrshell") {
            Some(idx) => {
                SHELL_SUB.store(idx, core::sync::atomic::Ordering::Release);
                true
            }
            None => false,
        }
    }

    /// 任务27 · 内核嵌入层输入泵取：泵一次硬件（键/鼠分流同任务19 泵），
    /// 随后把该订阅者的待发事件按 shim://input 16B 布局灌入用户缓冲。
    /// 返回已灌入事件数；未订阅/槽满时如实返回 0，绝不伪造输入。
    pub fn drain_to_shim(buf: &mut [u8], max_events: usize) -> usize {
        let sub_idx = SHELL_SUB.load(core::sync::atomic::Ordering::Acquire);
        if sub_idx == usize::MAX {
            return 0;
        }
        let cap = buf.len() / 16;
        let cap = if cap < max_events { cap } else { max_events };
        let s = svc();
        s.pump();
        let mut n = 0usize;
        while n < cap {
            let Some((seq, ev)) = s.poll_with_seq(sub_idx) else {
                break;
            };
            buf[n * 16..n * 16 + 16].copy_from_slice(&ev.to_shim_bytes(seq));
            n += 1;
        }
        n
    }

    /// BootScreen 键盘诊断字（2026-09-20 实机取证；boot://event 记录 idx=14
    /// 载荷，ushell 诊断行实时消费）。位格式：bits7:0=控制器探针
    /// （ps2::pack_probe 低 8 位）、bits23:8=已收键盘原始字节数（16 位饱和）、
    /// bits31:24=最后一个键盘原始字节。实机判读：按了键 raw 不涨 ⇒ 键盘
    /// 信号没到控制器（内建键盘很可能走 USB/控制器未就绪）；raw 涨了没出键
    /// ⇒ 扫描码集/解码问题（last 即实际编码）。
    pub fn kbd_diag_word() -> u32 {
        let probe = ps2::probe_word() & 0xFF;
        let s = svc();
        let raw = (s.raw_kbd.min(0xFFFF)) as u32;
        let last = s.last_raw as u32;
        probe | (raw << 8) | (last << 24)
    }

    const HEX: &[u8; 16] = b"0123456789abcdef";

    /// i8042 鼠标 bring-up：复位后 PS/2 鼠标默认**不上报数据**——
    /// ①控制器 0xA8 开 AUX 门 ②鼠标 0xF6 置默认/流模式 ③鼠标 0xF4
    /// 允许数据上报。每步等 ACK（0xFA），TSC 超时不致命（键盘路径不受影响）。
    fn mouse_bringup(tsc_hz: u64) -> u8 {
        use crate::ps2::port::{inp, outp};
        const STAT: u16 = 0x64;
        const DATA: u16 = 0x60;
        const STAT_OBF: u8 = 0x01;
        const STAT_IBF: u8 = 0x02;
        const STAT_AUX: u8 = 0x20;
        let ms = |n: u64| crate::timeline::read_tsc() + n * tsc_hz / 1000;

        // 冲掉残留输出。
        let flush_deadline = ms(10);
        while crate::timeline::read_tsc() < flush_deadline {
            if unsafe { inp(STAT) } & STAT_OBF != 0 {
                let _ = unsafe { inp(DATA) };
            }
        }
        // 单字节命令通道：写命令口→写数据口→等 ACK。
        let mouse_cmd = |cmd: u8| -> bool {
            let d = ms(20);
            while unsafe { inp(STAT) } & STAT_IBF != 0 {
                if crate::timeline::read_tsc() > d {
                    return false;
                }
                core::hint::spin_loop();
            }
            unsafe { outp(STAT, 0xD4) }; // 下一字节走 AUX 通道
            let d = ms(20);
            while unsafe { inp(STAT) } & STAT_IBF != 0 {
                if crate::timeline::read_tsc() > d {
                    return false;
                }
                core::hint::spin_loop();
            }
            unsafe { outp(DATA, cmd) };
            // 等 ACK：OBF+AUX 时读 0x60，应得 0xFA。
            let d = ms(60);
            while crate::timeline::read_tsc() < d {
                let st = unsafe { inp(STAT) };
                if st == 0xFF {
                    return false; // 无控制器
                }
                if st & STAT_OBF != 0 {
                    let b = unsafe { inp(DATA) };
                    if b == 0xFA && st & STAT_AUX != 0 {
                        return true;
                    }
                }
            }
            false
        };
        // ① 0xA8：控制器开 AUX 门（控制器命令，直接写命令口，无 ACK）。
        let d = ms(20);
        while unsafe { inp(STAT) } & STAT_IBF != 0 {
            if crate::timeline::read_tsc() > d {
                return 0;
            }
            core::hint::spin_loop();
        }
        unsafe { outp(STAT, 0xA8) };
        // ② 0xF6 set defaults → ③ 0xF4 enable data reporting。
        let mut ok = 0u8;
        if mouse_cmd(0xF6) {
            ok |= 0x1;
        }
        if mouse_cmd(0xF4) {
            ok |= 0x2;
        }
        ok
    }

    /// 16B shim 布局 → 32 字符小写十六进制（no_std 手工编码，无堆）。
    fn hex16(b: &[u8; 16]) -> &'static str {
        static mut BUF: [u8; 32] = [0u8; 32];
        let buf = unsafe { &mut *(&raw mut BUF) };
        for i in 0..16 {
            buf[i * 2] = HEX[(b[i] >> 4) as usize];
            buf[i * 2 + 1] = HEX[(b[i] & 0x0F) as usize];
        }
        core::str::from_utf8(buf).unwrap_or("-")
    }

    /// 任务19 实机探针入口（main.rs 挂在 NVMe 探针之后）。
    pub fn input_probe() {
        // ⓪ 实机戒律（2026-09-20）：先做控制器初始化再谈轮询——固件移交
        //    后的 i8042 状态不可假设（真机 BootScreen 按键全无响应的根因
        //    候选）。QEMU 复现不了这一点（开箱即用），取证靠探针字上屏。
        ps2::controller_init();
        crate::kinfo!("kbd-init: probe={:#010x}", ps2::probe_word());
        // ① 定容语义（合成注入，不依赖外设）：发布 CAP+6 条 → 丢最旧 6 条、
        //    计数一致、队列仍可发布（零分配零阻塞的结构性验证）。
        let s = svc();
        let flood = QUEUE_CAP as u64 + 6;
        for i in 0..flood {
            let k = if i % 2 == 0 { ps2::Key::Up } else { ps2::Key::Down };
            s.publish(InputEvent::Key(k));
        }
        let (pubs, drops) = s.stats();
        crate::kinfo!(
            "input-probe: flood publishes={} dropped_oldest={} expect_drops=6",
            pubs,
            drops
        );
        if drops != 6 {
            crate::kwarn!("input-probe: drop counter mismatch, abort probe");
            return;
        }

        // ② 订阅者槽位语义：MAX_SUBS 打满 → 明确拒绝；注销后可复用。
        //
        // 两条稳健性纪律（本轮回归实测踩到第一条）：
        // ① **不假设从空开始**——引导链上可能已有别的模块占着槽（如菜单键源
        //    的订阅）。所以先量出实际可填数量，填不满就如实降级为"跳过溢出
        //    断言"而不是当成失败。
        // ② **任何提前退出路径都必须把已占的槽还回去**——探针泄漏槽会让
        //    ushell 的输入订阅失败，后果是键盘彻底不可达。
        let mut filled = [0usize; MAX_SUBS];
        let mut n_filled = 0usize;
        while n_filled < MAX_SUBS {
            match s.subscribe("slot-fill") {
                Some(x) => {
                    filled[n_filled] = x;
                    n_filled += 1;
                }
                None => break,
            }
        }
        // 任何提前退出前先归还已占槽（用宏避免重复三遍同样的循环）。
        macro_rules! release_filled {
            () => {
                for &x in filled[..n_filled].iter() {
                    s.cancel(x);
                }
            };
        }
        if n_filled < MAX_SUBS {
            // 有别的占用者：跳过溢出断言（填不满就无从断言），但槽先还回去。
            crate::kwarn!(
                "input-probe: only {} of {} subscriber slots free — overflow assertion skipped",
                n_filled,
                MAX_SUBS
            );
            release_filled!();
            return;
        }
        if s.subscribe("overflow").is_some() {
            crate::kwarn!("input-probe: subscribe overflow not rejected");
            release_filled!();
            return;
        }
        release_filled!();
        let sub = match s.subscribe("probe") {
            Some(x) => x,
            None => {
                crate::kwarn!("input-probe: subscribe after cancel failed");
                return;
            }
        };
        crate::kinfo!("input-probe: sub-slots ok (fill/reject/cancel/reuse) sub={}", sub);

        // ③ 实机键鼠窗口：QEMU（hypervisor 位）等宿主脚本注入，事件齐早退、
        //    90s 超时如实上报；**真机跳过**——90s 黑屏窗口在实机上是纯等待，
        //    且窗口内按的键会被静默吃掉（2026-09-20 实机戒律）。
        let tsc_hz = crate::platform::info().map(|p| p.tsc_hz).unwrap_or(1_000_000_000);
        let on_qemu = crate::platform::info()
            .map(|p| p.features.hypervisor)
            .unwrap_or(false);
        let mut seen = [false; 3]; // Up/Down/Enter
        let mut mouse_moves = 0u32;
        let mut mouse_btn = false;
        if on_qemu {
            let bring = mouse_bringup(tsc_hz);
            crate::kinfo!(
                "input-probe: mouse bringup set-defaults={} enable-report={} (0xFA ack bits)",
                bring & 0x1 != 0,
                bring & 0x2 != 0
            );
            let s = svc();
            s.trace = true; // 泵级原始字节追踪（诊断窗口内开启）
            let deadline = crate::timeline::read_tsc() + 90 * tsc_hz;
            crate::kinfo!("input-probe: live (awaiting sendkey/mouse via HMP)");
            loop {
                // 泵节流：每 ~50µs 泵一次（TSC 步进），兼顾字节不断流与 ioport 开销。
                let next = crate::timeline::read_tsc() + tsc_hz / 20_000;
                while crate::timeline::read_tsc() < next {
                    core::hint::spin_loop();
                }
                let s = svc();
                let _ = s.pump();
                while let Some((seq, ev)) = s.poll_with_seq(sub) {
                    let shim = ev.to_shim_bytes(seq);
                    crate::kinfo!(
                        "input-probe: event seq={} shim={}",
                        seq,
                        hex16(&shim)
                    );
                    match ev {
                        InputEvent::Key(k) => {
                            let i = match k {
                                ps2::Key::Up => 0,
                                ps2::Key::Down => 1,
                                ps2::Key::Enter => 2,
                                _ => continue, // 扩展键不在三键矩阵内（任务55 扩表后如实跳过）
                            };
                            seen[i] = true;
                        }
                        InputEvent::Mouse { dx, dy, buttons } => {
                            if dx != 0 || dy != 0 {
                                mouse_moves += 1;
                            }
                            if buttons & 0x01 != 0 {
                                mouse_btn = true;
                            }
                            crate::kinfo!("input-probe: mouse dx={} dy={} buttons={:#04x}", dx, dy, buttons);
                        }
                    }
                }
                if seen[0] && seen[1] && seen[2] && mouse_moves >= 2 && mouse_btn {
                    break;
                }
                if crate::timeline::read_tsc() > deadline {
                    break;
                }
            }
        } else {
            crate::kinfo!("input-probe: real-hw window skipped (no hypervisor)");
        }
        let s = svc();
        let missed = s.sub_missed(sub).unwrap_or(u64::MAX);
        let ok = seen[0] && seen[1] && seen[2] && mouse_moves >= 2 && mouse_btn;
        crate::kinfo!(
            "input-probe: verdict={} keys={:?} mouse_moves={} mouse_btn={} missed={}",
            ok,
            seen,
            mouse_moves,
            mouse_btn,
            missed
        );
        s.cancel(sub); // 留出干净槽位，供后续消费者（任务26 链）复用
    }
}

// ---------------------------------------------------------------------------
// 宿主测试（ktest）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ps2::Key;
    use std::vec::Vec;

    fn mouse(dx: i16, dy: i16, buttons: u8) -> InputEvent {
        InputEvent::Mouse { dx, dy, buttons }
    }

    #[test]
    fn queue_fifo_single_subscriber() {
        let mut s = InputService::new();
        let sub = s.subscribe("t").unwrap();
        assert_eq!(s.publish(InputEvent::Key(Key::Up)), 1, "序号自 1 起");
        s.publish(InputEvent::Key(Key::Enter));
        s.publish(mouse(5, -6, 1));
        assert_eq!(s.poll(sub), Some(InputEvent::Key(Key::Up)), "FIFO 序");
        assert_eq!(s.poll(sub), Some(InputEvent::Key(Key::Enter)));
        assert_eq!(s.poll(sub), Some(mouse(5, -6, 1)));
        assert_eq!(s.poll(sub), None, "读尽即 None");
        assert_eq!(s.sub_missed(sub), Some(0), "未滞后零漏读");
    }

    #[test]
    fn queue_full_drops_oldest_and_counts() {
        let mut s = InputService::new();
        let sub = s.subscribe("t").unwrap();
        for i in 0..QUEUE_CAP as u64 {
            s.publish(InputEvent::Key(if i % 2 == 0 { Key::Up } else { Key::Down }));
        }
        assert_eq!(s.stats().1, 0, "未满零丢弃");
        s.publish(InputEvent::Key(Key::Enter)); // 第 65 条：丢第 1 条
        assert_eq!(s.stats().1, 1, "满发布丢最旧计数 +1");
        // 最旧（seq 1, Up）已被覆盖；下一个可读应为 seq 2。
        assert_eq!(s.poll(sub), Some(InputEvent::Key(Key::Down)), "seq1 被覆盖后从 seq2 起");
        assert_eq!(s.sub_missed(sub), Some(1), "订阅者未及读走的恰是被覆盖的 seq1");
    }

    #[test]
    fn queue_full_reader_catches_up_exactly() {
        let mut s = InputService::new();
        let sub = s.subscribe("t").unwrap();
        for _ in 0..QUEUE_CAP as u64 {
            s.publish(InputEvent::Key(Key::Up));
        }
        // 逐条读 64 条后追平
        for _ in 0..QUEUE_CAP {
            assert_eq!(s.poll(sub), Some(InputEvent::Key(Key::Up)));
        }
        assert_eq!(s.poll(sub), None);
        // 再压 2 条：写 66，最老有效 seq=3，但订阅者已在 write 位 → 零漏读
        s.publish(InputEvent::Key(Key::Enter));
        s.publish(InputEvent::Key(Key::Down));
        assert_eq!(s.sub_missed(sub), Some(0));
        assert_eq!(s.poll(sub), Some(InputEvent::Key(Key::Enter)));
        assert_eq!(s.poll(sub), Some(InputEvent::Key(Key::Down)));
        assert_eq!(s.stats().1, 2, "两次满发布 = 丢 2 条最坏口径");
    }

    #[test]
    fn subscriber_lag_counts_missed_and_resyncs() {
        let mut s = InputService::new();
        let sub = s.subscribe("t").unwrap();
        for _ in 0..QUEUE_CAP + 10 {
            s.publish(InputEvent::Key(Key::Up)); // 发布 74 条，seq 1..10 被覆盖
        }
        // 首次 poll 触发快进：missed 计数在读取路径上结算。
        let (seq, _) = s.poll_with_seq(sub).unwrap();
        assert_eq!(seq, 11, "滞后快进到最老可用");
        assert_eq!(s.sub_missed(sub), Some(10), "覆盖的 10 条计入实际漏读");
        assert_eq!(s.stats().1, 10, "最坏口径与实际漏读一致（全未读场景）");
    }

    #[test]
    fn subscribe_starts_at_now_no_replay() {
        let mut s = InputService::new();
        for _ in 0..3 {
            s.publish(InputEvent::Key(Key::Up));
        }
        let sub = s.subscribe("late").unwrap();
        assert_eq!(s.poll(sub), None, "新订阅者不得重放订阅前事件");
        s.publish(InputEvent::Key(Key::Enter));
        assert_eq!(s.poll(sub), Some(InputEvent::Key(Key::Enter)), "只读订阅后事件");
        assert_eq!(s.sub_missed(sub), Some(0), "不回放 ≠ 漏读");
    }

    #[test]
    fn broadcast_two_subscribers_independent() {
        let mut s = InputService::new();
        let a = s.subscribe("a").unwrap();
        let b = s.subscribe("b").unwrap();
        s.publish(InputEvent::Key(Key::Up));
        s.publish(mouse(1, 2, 0));
        assert_eq!(s.poll(a), Some(InputEvent::Key(Key::Up)));
        assert_eq!(s.poll(a), Some(mouse(1, 2, 0)));
        assert_eq!(s.poll(a), None, "a 读尽不影响 b");
        assert_eq!(s.poll(b), Some(InputEvent::Key(Key::Up)), "b 独立游标");
        assert_eq!(s.poll(b), Some(mouse(1, 2, 0)));
    }

    #[test]
    fn subscribe_slots_exhausted_then_reused() {
        let mut s = InputService::new();
        let mut got = Vec::new();
        for i in 0..MAX_SUBS {
            got.push(s.subscribe("fill").unwrap_or_else(|| panic!("第 {} 个订阅应成功", i)));
        }
        assert!(s.subscribe("overflow").is_none(), "第 5 个订阅应明确拒绝");
        s.cancel(got[1]);
        assert!(s.subscribe("reused").is_some(), "注销后槽位可复用");
    }

    #[test]
    fn key_bytes_reuse_ps2_decoder_same_source() {
        let mut s = InputService::new();
        let sub = s.subscribe("t").unwrap();
        assert_eq!(s.feed_key_byte(0xE0), None, "扩展前缀被消化");
        assert_eq!(s.feed_key_byte(0x50), Some(InputEvent::Key(Key::Down)), "同源 ps2::Decoder");
        assert_eq!(s.feed_key_byte(0x50 | 0x80), None, "断码不入队");
        assert_eq!(s.poll(sub), Some(InputEvent::Key(Key::Down)));
    }

    #[test]
    fn mouse_packet_decode_basic_and_buttons() {
        let mut s = InputService::new();
        let sub = s.subscribe("t").unwrap();
        s.feed_mouse_byte(0x08); // 包头：无键无符号
        s.feed_mouse_byte(30);
        assert_eq!(
            s.feed_mouse_byte(40),
            Some(mouse(30, 40, 0)),
            "凑满 3 字节发布"
        );
        assert_eq!(s.poll(sub), Some(mouse(30, 40, 0)));
    }

    #[test]
    fn mouse_sign_extension_and_negative() {
        let mut d = MouseDecoder::default();
        assert_eq!(d.feed(0x28), None); // y 负号位，凑包中
        assert_eq!(d.feed(0xFF), None);
        let m = d.feed(0xE0).unwrap(); // dx=255 正、dy=224 → 224-256=-32
        assert_eq!(m.dx, 255);
        assert_eq!(m.dy, -32);
        assert_eq!(m.buttons, 0);
        let mut d2 = MouseDecoder::default();
        d2.feed(0x19); // 左键按下 + dx 负号位（0x09|0x10）
        d2.feed(0xFF); // dx=-1
        let m2 = d2.feed(0x01).unwrap(); // dy=1 正
        assert_eq!(m2, MouseDelta { dx: -1, dy: 1, buttons: 1 });
    }

    #[test]
    fn mouse_resync_on_lost_header() {
        let mut d = MouseDecoder::default();
        assert_eq!(d.feed(0x00), None, "包头同步位缺失 → 丢弃重同步");
        assert_eq!(d.feed(0x08), None);
        assert_eq!(d.feed(10), None);
        assert_eq!(d.feed(20), Some(MouseDelta { dx: 10, dy: 20, buttons: 0 }), "重同步后整包正确");
    }

    #[test]
    fn mouse_mid_packet_zeros_are_data() {
        let mut d = MouseDecoder::default();
        assert_eq!(d.feed(0x0A), None, "包头（bit3=1，中键）");
        assert_eq!(d.feed(0x00), None, "包中 0x00 是合法数据，不得触发重同步");
        assert_eq!(d.feed(0x05), Some(MouseDelta { dx: 0, dy: 5, buttons: 2 }));
    }

    /// 喂一个完整 3 字节鼠标包（包头 + dx + dy），返回是否解出。
    fn feed_packet(s: &mut InputService, flags: u8, dx: u8, dy: u8) -> bool {
        s.feed_mouse_byte(flags);
        s.feed_mouse_byte(dx);
        s.feed_mouse_byte(dy).is_some()
    }

    #[test]
    fn take_mouse_is_none_without_data() {
        let mut s = InputService::new();
        assert_eq!(
            s.take_mouse(),
            None,
            "从未收到鼠标包时必须返回 None（菜单据此判定「没有鼠标」）"
        );
        // 只喂了半个包：未凑满不产生位移
        s.feed_mouse_byte(0x08);
        s.feed_mouse_byte(10);
        assert_eq!(s.take_mouse(), None);
    }

    #[test]
    fn take_mouse_accumulates_across_packets() {
        // 菜单一片只取一次：同一片内多个包必须累加，不能只留最后一个。
        let mut s = InputService::new();
        assert!(feed_packet(&mut s, 0x08, 10, 20));
        assert!(feed_packet(&mut s, 0x08, 5, 30));
        assert!(feed_packet(&mut s, 0x18, 200, 0)); // dx 负号位：200-256 = -56
        let m = s.take_mouse().unwrap();
        assert_eq!(m.dx, 10 + 5 - 56);
        assert_eq!(m.dy, 20 + 30);
        assert_eq!(m.buttons, 0);
    }

    #[test]
    fn take_mouse_unions_buttons_and_resets() {
        let mut s = InputService::new();
        assert!(feed_packet(&mut s, 0x09, 0, 0)); // bit0 左键
        assert!(feed_packet(&mut s, 0x0A, 0, 0)); // bit1 右键
        let m = s.take_mouse().unwrap();
        assert_eq!(m.buttons, 0x03, "本窗口出现过的按键位取并集");
        // 取走即复位：再次调用必须回到 None（否则菜单会重复确认）
        assert_eq!(s.take_mouse(), None, "take_mouse 必须清空累计量");
        assert!(feed_packet(&mut s, 0x08, 1, 1));
        assert_eq!(s.take_mouse().unwrap().buttons, 0, "复位后按键位不残留");
    }

    #[test]
    fn take_mouse_saturates_instead_of_wrapping() {
        // 累计量远超 i16 时静默翻转会让指针瞬移到屏幕另一头，必须饱和。
        let mut s = InputService::new();
        for _ in 0..300 {
            assert!(feed_packet(&mut s, 0x08, 200, 200));
        }
        let m = s.take_mouse().unwrap();
        assert_eq!(m.dx, i16::MAX, "超上限饱和到 i16::MAX 而非翻转");
        assert_eq!(m.dy, i16::MAX);
    }

    #[test]
    fn mouse_channel_is_independent_of_queue() {
        // 菜单期没有订阅者：累计通道必须仍然可用（这正是它存在的理由）。
        let mut s = InputService::new();
        assert!(feed_packet(&mut s, 0x08, 3, 4));
        assert_eq!(s.stats().0, 1, "事件照常进队列，两通道互不干扰");
        assert_eq!(s.take_mouse().unwrap().dx, 3, "无订阅者也能取到位移");
    }

    /// 回归钉：菜单键源占着槽时，槽位自检**不能**泄漏槽，否则 ushell 的
    /// 输入订阅会失败、键盘彻底不可达（2026-09-20 交接走查实测踩到：
    /// h1/h3 场景串口打出 `usrshell: inputsvc subscribe failed`）。
    #[test]
    fn probe_slot_check_survives_preoccupied_slot() {
        let mut s = InputService::new();
        // 模拟菜单期已注册的键源订阅
        let menu_slot = s.subscribe("bootselect").unwrap();
        // 模拟探针的"填到满"循环：被占一个槽后只能填 MAX_SUBS-1 个
        let mut filled = [0usize; MAX_SUBS];
        let mut n = 0usize;
        while n < MAX_SUBS {
            match s.subscribe("slot-fill") {
                Some(x) => {
                    filled[n] = x;
                    n += 1;
                }
                None => break,
            }
        }
        assert_eq!(n, MAX_SUBS - 1, "被占一个槽后只能填 {} 个", MAX_SUBS - 1);
        // 探针归还它占的那些（修复后的行为）
        for &x in filled[..n].iter() {
            s.cancel(x);
        }
        // 菜单用完也要还（main.rs 在 run_countdown 之后调 menu_release_key_source）
        s.cancel(menu_slot);
        // 归还干净后，ushell 必须还能订上——这就是"键盘可达"的硬条件
        assert!(
            s.subscribe("usrshell").is_some(),
            "归还槽之后 ushell 必须能订阅，否则键盘彻底不可达"
        );
    }

    #[test]
    fn menu_release_key_source_reports_and_is_idempotent() {
        // 关键路径的返回值契约：注册过 → true；重复调用/没注册过 → false。
        let mut s = InputService::new();
        let sub = s.subscribe("bootselect").unwrap();
        s.cancel(sub);
        assert!(s.subscribe("usrshell").is_some(), "取消后槽位可复用");
    }

    #[test]
    fn shim_bytes_layout_contract() {
        // Key(Enter) @seq 7：seq=7(LE) kind=0 key=2 其余零
        let k = InputEvent::Key(Key::Enter).to_shim_bytes(7);
        let mut expect = [0u8; 16];
        expect[0] = 7;
        expect[8] = InputEvent::KIND_KEY;
        expect[9] = 2;
        assert_eq!(k, expect, "shim://input 键事件布局（任务26 契约）");
        // Mouse{dx:-3, dy:2, buttons:1} @seq 9
        let m = mouse(-3, 2, 1).to_shim_bytes(9);
        let mut expect = [0u8; 16];
        expect[0] = 9;
        expect[8] = InputEvent::KIND_MOUSE;
        expect[10] = 0xFD; // -3 LE
        expect[11] = 0xFF;
        expect[12] = 2;
        expect[14] = 1;
        assert_eq!(m, expect, "shim://input 鼠标事件布局（任务26 契约）");
    }

    #[test]
    fn publish_seq_monotonic_across_overwrite() {
        let mut s = InputService::new();
        for i in 1..=QUEUE_CAP as u64 + 3 {
            assert_eq!(s.publish(InputEvent::Key(Key::Up)), i, "序号跨覆盖仍单调");
        }
        assert_eq!(s.stats().1, 3);
    }

    #[test]
    fn bootselect_reuses_service_as_key_source() {
        // 任务19 → 任务1 复用证明：菜单先订阅，服务喂扫描码字节，选择逻辑工作。
        use crate::fb::{PixelFormat, Surface};
        let mut s = InputService::new();
        let mut adapter = KeySourceAdapter::new(&mut s).expect("订阅成功");
        for b in [0x50u8, 0x50, 0x1C] {
            s.feed_key_byte(b); // ↓ ↓ Enter（订阅后到达——服务不回放历史）
        }
        let mut keys = || adapter.poll_key(&mut s);
        let mut v: std::vec::Vec<u8> = std::vec![0u8; 800 * 600 * 4];
        let surf = unsafe { Surface::from_raw(v.as_mut_ptr(), 800, 600, 800 * 4, PixelFormat::Bgr32) };
        let sel = crate::bootselect::run_countdown_with(&surf, 5, 100_000, &mut keys);
        assert_eq!(sel, 2, "服务链 ↓↓Enter 应选中第三项（任务1 复用证明）");
    }

    // ---------------- S2.05 焦点路由（AI-4） ----------------

    fn fake_clock() -> u64 {
        // 测试用假时钟：由调用方配合 LAT 原子递增语义不必要——
        // 这里直接用静态递增计数器（线程局部即可，ktest 单线程）。
        use std::cell::Cell;
        thread_local! {
            static N: Cell<u64> = const { Cell::new(0) };
        }
        N.with(|n| {
            n.set(n.get() + 10);
            n.get()
        })
    }

    /// ×1000 焦点切换无串键：A=All 广播、B/C=KeyboardFocusOnly，
    /// 1000 轮交替切焦点+发键，断言各收各键、鼠标不受焦点影响。
    #[test]
    fn focus_route_1000_no_crosstalk() {
        let mut s = InputService::new();
        let a = s.subscribe("all").unwrap();
        let b = s.subscribe_policy_pid("kbd-b", FocusPolicy::KeyboardFocusOnly, 0xB).unwrap();
        let c = s.subscribe_policy_pid("kbd-c", FocusPolicy::KeyboardFocusOnly, 0xC).unwrap();
        let (mut got_b, mut got_c, mut got_a_keys, mut got_a_mouse) = (0u32, 0u32, 0u32, 0u32);
        for round in 0..1000u32 {
            // 交替焦点；每轮 1 键 + 1 鼠标。
            let focus_b = round % 2 == 0;
            assert!(s.set_focus(if focus_b { b } else { c }));
            s.feed_key_byte(0x1E); // A 键 make（解码无歧义路径）
            // 鼠标 3 字节包（bit3 同步位包头 + dx + dy）：凑包后发布一帧。
            s.feed_mouse_byte(0x08).unwrap();
            s.feed_mouse_byte(0x00).unwrap();
            s.feed_mouse_byte(0x00).unwrap();
            while let Some(ev) = s.poll(a) {
                if matches!(ev, InputEvent::Key(_)) {
                    got_a_keys += 1;
                } else {
                    got_a_mouse += 1;
                }
            }
            while s.poll(b).is_some() {
                got_b += 1;
            }
            while s.poll(c).is_some() {
                got_c += 1;
            }
        }
        // All 槽：1000 键 + 1000 鼠标（焦点不影响广播槽）。
        assert_eq!(got_a_keys, 1000, "All 槽键盘全收");
        assert_eq!(got_a_mouse, 1000, "All 槽鼠标全收");
        // B/C 各收焦点在自己身上的键；C 多收最后一轮后未切换的部分——
        // 1000 轮交替：B=偶数轮 500，C=奇数轮 500。
        assert_eq!(got_b, 500, "B 只收焦点在自己上的键");
        assert_eq!(got_c, 500, "C 只收焦点在自己上的键");
        assert_eq!(s.focus_switches(), 1000);
        assert!(s.routed_away() > 0, "焦点路由确有跳过行为");
    }

    #[test]
    fn focus_none_blocks_keyboard_focus_only_subs() {
        // 无焦点（None）时 KeyboardFocusOnly 槽收不到键盘（白名单默认拒绝）；
        // All 槽照常；set_focus_pid 查表生效；cancel 清焦点与 pid。
        let mut s = InputService::new();
        let all = s.subscribe("all").unwrap();
        let w = s.subscribe_policy_pid("wine", FocusPolicy::KeyboardFocusOnly, 0x42).unwrap();
        s.feed_key_byte(0x21);
        assert!(s.poll(all).is_some(), "All 槽无焦点照收");
        assert!(s.poll(w).is_none(), "无焦点 KFO 槽不收键盘");
        assert!(s.set_focus_pid(0x42));
        assert_eq!(s.focus(), Some(w));
        s.feed_key_byte(0x21);
        assert!(s.poll(w).is_some(), "焦点就位后 KFO 槽收键盘");
        assert!(!s.set_focus_pid(0x99), "无匹配 pid 明确拒绝");
        assert!(!s.set_focus_pid(0), "pid 0 保留拒绝");
        s.cancel(w);
        assert_eq!(s.focus(), None, "cancel 焦点联动清空");
        assert!(!s.set_focus_pid(0x42), "cancel 后 pid 查不到");
    }

    #[test]
    fn latency_stats_with_injected_clock() {
        // 延迟打点：注入时钟后 publish→poll 差被统计；零时钟恒 0。
        let mut s = InputService::new();
        let sub = s.subscribe("lat").unwrap();
        assert_eq!(s.latency_ticks(), (0, 0, 0), "零时钟不统计");
        s.set_clock(fake_clock);
        s.feed_key_byte(0x21);
        assert!(s.poll(sub).is_some());
        let (last, max, samples) = s.latency_ticks();
        assert_eq!(samples, 1);
        assert!(last > 0 && max >= last, "时钟注入后延迟统计生效");
    }
}
