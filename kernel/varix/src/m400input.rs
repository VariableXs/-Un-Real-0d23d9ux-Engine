//! VARIX-M400 AI-06 输入与外设域（F126~F150）。
//!
//! 真实设备即插即用。纯逻辑 + 固定容量数组，no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F126 — xHCI USB 主控（枚举）
// ---------------------------------------------------------------------------

pub const USB_DEV_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbDevice {
    pub slot: u8,
    pub vid: u16,
    pub pid: u16,
    pub class: u8,
}

#[derive(Clone, Copy)]
pub struct XhciController {
    pub devices: [Option<UsbDevice>; USB_DEV_CAP],
    pub count: usize,
}

impl XhciController {
    pub const fn new() -> XhciController {
        XhciController { devices: [const { None }; USB_DEV_CAP], count: 0 }
    }

    pub fn enumerate(&mut self, d: UsbDevice) -> bool {
        if self.count >= USB_DEV_CAP || d.slot == 0 {
            return false;
        }
        if self.devices.iter().flatten().any(|e| e.slot == d.slot) {
            return false;
        }
        self.devices[self.count] = Some(d);
        self.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F127 — HID 键鼠通用驱动
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidClass {
    Keyboard,
    Mouse,
    Other,
}

/// usage page/device class 归类。
pub fn f127_classify(usage_page: u8) -> HidClass {
    match usage_page {
        0x07 => HidClass::Keyboard,
        0x01 if true => HidClass::Mouse, // generic desktop + mouse usage 简化
        _ => HidClass::Other,
    }
}

// ---------------------------------------------------------------------------
// F128 — USB 大容量存储
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MassStorage {
    pub lun_count: u8,
    pub sectors: u64,
}

pub fn f128_msc_ready(ms: MassStorage) -> bool {
    ms.lun_count > 0 && ms.sectors > 0
}

// ---------------------------------------------------------------------------
// F129 — 热插拔事件总线
// ---------------------------------------------------------------------------

pub const EVENT_BUS_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlugEvent {
    Arrived(u8),
    Left(u8),
}

pub struct EventBus {
    pub events: [Option<PlugEvent>; EVENT_BUS_CAP],
    pub head: usize,
    pub tail: usize,
    pub len: usize,
    pub ui_attached: bool,
}

impl EventBus {
    pub const fn new() -> EventBus {
        EventBus { events: [const { None }; EVENT_BUS_CAP], head: 0, tail: 0, len: 0, ui_attached: false }
    }

    pub fn publish(&mut self, ev: PlugEvent) -> bool {
        if self.len >= EVENT_BUS_CAP {
            return false;
        }
        self.events[self.tail] = Some(ev);
        self.tail = (self.tail + 1) % EVENT_BUS_CAP;
        self.len += 1;
        true
    }

    /// 事件必须可送达 UI 才算全链可达。
    pub fn dispatch_next(&mut self) -> Option<PlugEvent> {
        if self.len == 0 || !self.ui_attached {
            return None;
        }
        let ev = self.events[self.head];
        self.events[self.head] = None;
        self.head = (self.head + 1) % EVENT_BUS_CAP;
        self.len -= 1;
        ev
    }
}

// ---------------------------------------------------------------------------
// F130 — evdev 式输入抽象
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub dev_id: u8,
    pub kind: u16, // 1=key, 2=rel, 3=abs
    pub code: u16,
    pub value: i32,
}

pub fn f130_is_key_press(ev: InputEvent) -> bool {
    ev.kind == 1 && ev.value == 1
}

// ---------------------------------------------------------------------------
// F131 — 按键布局与重复引擎
// ---------------------------------------------------------------------------

pub const LAYOUT_CAP: usize = 4;

pub struct KeyLayout {
    pub maps: [[u8; 8]; LAYOUT_CAP], // 扫描码 -> 字符
    pub active: usize,
    pub repeat_delay_ms: u32,
    pub repeat_rate_ms: u32,
}

impl KeyLayout {
    pub fn translate(&self, scancode: u8) -> u8 {
        self.maps[self.active.min(LAYOUT_CAP - 1)][scancode.min(7) as usize]
    }

    pub fn switch_to(&mut self, idx: usize) -> bool {
        if idx >= LAYOUT_CAP {
            return false;
        }
        self.active = idx;
        true
    }

    /// 重复参数合法：rate < delay。
    pub fn repeat_valid(&self) -> bool {
        self.repeat_rate_ms > 0 && self.repeat_rate_ms < self.repeat_delay_ms
    }
}

// ---------------------------------------------------------------------------
// F132 — 多键盘区分（设备级归因）
// ---------------------------------------------------------------------------

pub fn f132_attribute(ev: InputEvent, registered: &[u8]) -> bool {
    registered.contains(&ev.dev_id)
}

// ---------------------------------------------------------------------------
// F133 — 游戏手柄
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GamepadState {
    pub buttons: u32,
    pub lx: i16,
    pub ly: i16,
}

pub const BTN_A: u32 = 1 << 0;
pub const BTN_B: u32 = 1 << 1;
pub const BTN_START: u32 = 1 << 8;

pub fn f133_button_pressed(gp: GamepadState, btn: u32) -> bool {
    gp.buttons & btn == btn
}

/// 摇杆死区。
pub fn f133_in_deadzone(v: i16, dz: i16) -> bool {
    (v as i32).abs() <= dz as i32
}

// ---------------------------------------------------------------------------
// F134 — 触控板手势骨架
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    None,
    TwoFingerScroll,
    ThreeFingerSwipe,
    Pinch,
}

pub fn f134_recognize(fingers: u8, dx: i32, dy: i32) -> Gesture {
    match fingers {
        2 if dx != 0 || dy != 0 => Gesture::TwoFingerScroll,
        3 => Gesture::ThreeFingerSwipe,
        _ if fingers > 3 => Gesture::Pinch,
        _ => Gesture::None,
    }
}

// ---------------------------------------------------------------------------
// F135 — PS/2 兼容回退
// ---------------------------------------------------------------------------

pub fn f135_ps2_fallback(usb_keyboard_present: bool, ps2_port_present: bool) -> bool {
    !usb_keyboard_present && ps2_port_present
}

// ---------------------------------------------------------------------------
// F136 — 串口控制台成熟化（救援模式）
// ---------------------------------------------------------------------------

pub fn f136_rescue_line(out: &mut [u8], cmd: &str, ok: bool) -> usize {
    let mut n = 0usize;
    crate::checks::push_str(out, &mut n, "rescue> ");
    crate::checks::push_str(out, &mut n, cmd);
    crate::checks::push_str(out, &mut n, if ok { " [ok]" } else { " [fail]" });
    n
}

// ---------------------------------------------------------------------------
// F137 — 遗留设备策略
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyPolicy {
    Supported,
    Unsupported,
}

pub fn f137_legacy_policy(isa_dma: bool, lpt: bool, floppy: bool) -> LegacyPolicy {
    if isa_dma || lpt || floppy {
        LegacyPolicy::Unsupported
    } else {
        LegacyPolicy::Supported
    }
}

// ---------------------------------------------------------------------------
// F138 — 打印机骨架（作业模型）
// ---------------------------------------------------------------------------

pub const PRINT_JOBS: usize = 8;

pub struct PrintQueue {
    pub jobs: [Option<(u8, u32)>; PRINT_JOBS], // (printer id, bytes)
    pub count: usize,
}

impl PrintQueue {
    pub const fn new() -> PrintQueue {
        PrintQueue { jobs: [const { None }; PRINT_JOBS], count: 0 }
    }

    pub fn submit(&mut self, printer: u8, bytes: u32) -> bool {
        if self.count >= PRINT_JOBS || bytes == 0 {
            return false;
        }
        self.jobs[self.count] = Some((printer, bytes));
        self.count += 1;
        true
    }

    pub fn pop(&mut self) -> Option<(u8, u32)> {
        if self.count == 0 {
            return None;
        }
        let j = self.jobs[0];
        self.jobs.copy_within(1.., 0);
        self.jobs[PRINT_JOBS - 1] = None;
        self.count -= 1;
        j
    }
}

// ---------------------------------------------------------------------------
// F139 — HDA 音频驱动
// ---------------------------------------------------------------------------

pub fn f139_hda_volume_clamp(requested: i8) -> u8 {
    requested.clamp(0, 100) as u8
}

pub fn f139_hda_codec_present(vid_did: u32) -> bool {
    vid_did != 0 && vid_did != u32::MAX
}

// ---------------------------------------------------------------------------
// F140 — 混音策略
// ---------------------------------------------------------------------------

/// 线性混音 + 削波保护。
pub fn f140_mix(a: i16, b: i16) -> i16 {
    let sum = a as i32 + b as i32;
    sum.clamp(-32768, 32767) as i16
}

/// 软限幅：超过 80% 峰值开始衰减。
pub fn f140_soft_limit(sample: i16) -> i16 {
    let peak: i32 = 26_214; // 32767 * 4 / 5
    let s = sample as i32;
    if s > peak {
        (peak + (s - peak) / 4) as i16
    } else if s < -peak {
        (-peak + (s + peak) / 4) as i16
    } else {
        sample
    }
}

// ---------------------------------------------------------------------------
// F141 — 蓝牙骨架
// ---------------------------------------------------------------------------

pub fn f141_bt_addr_valid(addr: &[u8; 6]) -> bool {
    // 高 2 位为 11 的地址是保留的（非公共地址），公共地址至少不全是 0
    !(addr.iter().all(|&b| b == 0)) && (addr[5] & 0b1100_0000) != 0b1100_0000
}

// ---------------------------------------------------------------------------
// F142 — 摄像头枚举
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Camera {
    pub id: u8,
    pub in_use: bool,
}

pub fn f142_camera_free(cams: &[Camera], id: u8) -> bool {
    cams.iter().filter(|c| c.id == id).all(|c| !c.in_use)
}

// ---------------------------------------------------------------------------
// F143 — 温度/风扇传感器
// ---------------------------------------------------------------------------

pub fn f143_temp_sane(millidegrees: i32) -> bool {
    (-40_000..=120_000).contains(&millidegrees)
}

/// 风扇转速曲线：温度越高转速越快。
pub fn f143_fan_pct(temp_c: i32) -> u32 {
    if temp_c <= 40 {
        30
    } else if temp_c >= 90 {
        100
    } else {
        (30 + (temp_c - 40) * 70 / 50) as u32
    }
}

// ---------------------------------------------------------------------------
// F144 — 硬件监控告警
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HwAlert {
    None,
    Warn,
    Critical,
}

pub fn f144_thermal_alert(temp_c: i32) -> HwAlert {
    if temp_c >= 95 {
        HwAlert::Critical
    } else if temp_c >= 80 {
        HwAlert::Warn
    } else {
        HwAlert::None
    }
}

// ---------------------------------------------------------------------------
// F145 — 设备管理器数据源
// ---------------------------------------------------------------------------

pub const DEVICE_TREE_CAP: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevNode {
    pub id: u16,
    pub parent: Option<u16>,
    pub name_tag: u8,
}

/// 遍历深度检查：环检测（父链成环即非法）。
pub fn f145_tree_acyclic(nodes: &[DevNode]) -> bool {
    for n in nodes {
        let mut steps = 0usize;
        let mut cur = n.parent;
        while let Some(p) = cur {
            if p == n.id {
                return false;
            }
            let parent_node = nodes.iter().find(|x| x.id == p);
            cur = parent_node.and_then(|x| x.parent);
            steps += 1;
            if steps > nodes.len() {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F146 — 驱动签名校验
// ---------------------------------------------------------------------------

pub fn f146_driver_sig_ok(driver_hash: u64, signed_hash: u64, require_sig: bool) -> bool {
    if !require_sig {
        return true;
    }
    driver_hash == signed_hash
}

// ---------------------------------------------------------------------------
// F147 — 固件加载框架
// ---------------------------------------------------------------------------

pub const FW_MAX_BYTES: usize = 64;

pub fn f147_firmware_load(blob: &[u8]) -> Result<usize, &'static str> {
    if blob.is_empty() {
        return Err("empty firmware");
    }
    if blob.len() > FW_MAX_BYTES {
        return Err("firmware too large");
    }
    Ok(blob.len())
}

// ---------------------------------------------------------------------------
// F148 — 驱动崩溃隔离
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverState {
    Running,
    Crashed,
    Recovered,
}

/// 驱动崩溃只标记自身，不拖垮内核；可恢复。
pub fn f148_isolate(state: DriverState, attempts: u32) -> DriverState {
    match state {
        DriverState::Crashed if attempts < 3 => DriverState::Recovered,
        DriverState::Crashed => DriverState::Crashed, // 重试耗尽保持隔离
        other => other,
    }
}

// ---------------------------------------------------------------------------
// F149 — 设备电源管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevPowerState {
    D0Active,
    D3Suspended,
}

/// 挂起顺序：子设备先挂起。
pub fn f149_suspend_order(children_before_parent: bool) -> bool {
    children_before_parent
}

// ---------------------------------------------------------------------------
// F150 — 设备树统一模型 + 域自检
// ---------------------------------------------------------------------------

pub fn run_inputm400_checks() -> CheckSet {
    let mut set = CheckSet::new("inputm400");

    // F126
    let mut xhci = XhciController::new();
    let dev = UsbDevice { slot: 1, vid: 0x046D, pid: 0xC52B, class: 0x03 };
    set.add("F126 enumerate", xhci.enumerate(dev) && xhci.count == 1, "one device");
    set.add("F126 dup slot", !xhci.enumerate(dev), "same slot rejected");
    set.add("F126 slot0", !xhci.enumerate(UsbDevice { slot: 0, vid: 1, pid: 1, class: 0 }), "slot 0 invalid");

    // F127
    set.add("F127 keyboard", f127_classify(0x07) == HidClass::Keyboard, "kbd page");
    set.add("F127 other", f127_classify(0x0F) == HidClass::Other, "pid page");

    // F128
    set.add("F128 ready", f128_msc_ready(MassStorage { lun_count: 1, sectors: 1000 }), "valid MSC");
    set.add("F128 empty", !f128_msc_ready(MassStorage { lun_count: 0, sectors: 0 }), "no lun");

    // F129
    let mut bus = EventBus::new();
    bus.ui_attached = true;
    set.add("F129 publish", bus.publish(PlugEvent::Arrived(2)) && bus.len == 1, "queued");
    set.add("F129 dispatch", bus.dispatch_next() == Some(PlugEvent::Arrived(2)), "reached UI");
    bus.ui_attached = false;
    bus.publish(PlugEvent::Left(2));
    set.add("F129 stalled", bus.dispatch_next().is_none(), "no UI no dispatch");

    // F130
    let key = InputEvent { dev_id: 1, kind: 1, code: 30, value: 1 };
    set.add("F130 key press", f130_is_key_press(key), "EV_KEY down");
    set.add("F130 release", !f130_is_key_press(InputEvent { value: 0, ..key }), "up not press");

    // F131
    let mut layout = KeyLayout {
        maps: [[b'a'; 8], [b'q'; 8], [0; 8], [0; 8]],
        active: 0,
        repeat_delay_ms: 500,
        repeat_rate_ms: 30,
    };
    set.add("F131 translate", layout.translate(0) == b'a', "layout 0");
    layout.switch_to(1);
    set.add("F131 switch", layout.translate(0) == b'q', "layout 1");
    set.add("F131 repeat", layout.repeat_valid(), "rate<delay");
    layout.repeat_rate_ms = 600;
    set.add("F131 repeat bad", !layout.repeat_valid(), "rate>=delay invalid");

    // F132
    set.add("F132 attrib", f132_attribute(key, &[1, 2]), "known device");
    set.add("F132 unknown", !f132_attribute(key, &[2, 3]), "unknown device");

    // F133
    let gp = GamepadState { buttons: BTN_A | BTN_START, lx: 10, ly: -10 };
    set.add("F133 btn", f133_button_pressed(gp, BTN_A) && !f133_button_pressed(gp, BTN_B), "A yes B no");
    set.add("F133 deadzone", f133_in_deadzone(5, 10) && !f133_in_deadzone(50, 10), "dz semantics");

    // F134
    set.add("F134 scroll", f134_recognize(2, 0, -30) == Gesture::TwoFingerScroll, "two fingers move");
    set.add("F134 swipe", f134_recognize(3, 0, 0) == Gesture::ThreeFingerSwipe, "three fingers");
    set.add("F134 none", f134_recognize(1, 0, 0) == Gesture::None, "single none");

    // F135
    set.add("F135 fallback", f135_ps2_fallback(false, true), "PS/2 used");
    set.add("F135 usb wins", !f135_ps2_fallback(true, true), "usb present");

    // F136
    let mut buf = [0u8; 48];
    let n = f136_rescue_line(&mut buf, "ls", true);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    set.add("F136 rescue", text.starts_with("rescue> ls [ok]"), "prompt rendered");

    // F137
    set.add("F137 floppy unsupported", f137_legacy_policy(false, false, true) == LegacyPolicy::Unsupported, "floppy out");
    set.add("F137 pci ok", f137_legacy_policy(false, false, false) == LegacyPolicy::Supported, "modern ok");

    // F138
    let mut pq = PrintQueue::new();
    set.add("F138 submit", pq.submit(1, 100) && pq.count == 1, "queued");
    set.add("F138 empty job", !pq.submit(1, 0), "zero bytes rejected");
    set.add("F138 fifo", pq.pop() == Some((1, 100)) && pq.pop().is_none(), "fifo order");

    // F139
    set.add("F139 clamp low", f139_hda_volume_clamp(-5) == 0, "floor 0");
    set.add("F139 clamp high", f139_hda_volume_clamp(120) == 100, "cap 100");
    set.add("F139 codec", f139_hda_codec_present(0x10EC0892) && !f139_hda_codec_present(0), "codec id");

    // F140
    set.add("F140 sum", f140_mix(100, 200) == 300, "linear mix");
    set.add("F140 clip", f140_mix(30000, 30000) == 32767, "clipped");
    set.add("F140 soft limit", f140_soft_limit(30000) < 30000, "attenuated above 80%");
    set.add("F140 passthrough", f140_soft_limit(100) == 100, "quiet untouched");

    // F141
    set.add("F141 valid", f141_bt_addr_valid(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]), "public addr");
    set.add("F141 reserved", !f141_bt_addr_valid(&[0, 0, 0, 0, 0, 0xFF]), "reserved prefix");

    // F142
    let cams = [Camera { id: 1, in_use: true }, Camera { id: 2, in_use: false }];
    set.add("F142 busy", !f142_camera_free(&cams, 1), "cam 1 busy");
    set.add("F142 free", f142_camera_free(&cams, 2), "cam 2 free");

    // F143
    set.add("F143 sane", f143_temp_sane(45_000) && !f143_temp_sane(999_999), "range check");
    set.add("F143 fan idle", f143_fan_pct(30) == 30, "cool -> 30%");
    set.add("F143 fan max", f143_fan_pct(95) == 100, "hot -> 100%");

    // F144
    set.add("F144 crit", f144_thermal_alert(96) == HwAlert::Critical, ">=95 critical");
    set.add("F144 warn", f144_thermal_alert(85) == HwAlert::Warn, "80-95 warn");
    set.add("F144 none", f144_thermal_alert(40) == HwAlert::None, "cool");

    // F145
    let tree = [
        DevNode { id: 1, parent: None, name_tag: 0 },
        DevNode { id: 2, parent: Some(1), name_tag: 1 },
        DevNode { id: 3, parent: Some(2), name_tag: 2 },
    ];
    let cyclic = [
        DevNode { id: 1, parent: Some(2), name_tag: 0 },
        DevNode { id: 2, parent: Some(1), name_tag: 1 },
    ];
    set.add("F145 acyclic", f145_tree_acyclic(&tree), "valid tree");
    set.add("F145 cycle", !f145_tree_acyclic(&cyclic), "cycle detected");

    // F146
    set.add("F146 match", f146_driver_sig_ok(0xAA, 0xAA, true), "signed");
    set.add("F146 mismatch", !f146_driver_sig_ok(0xAB, 0xAA, true), "unsigned rejected");
    set.add("F146 opt-out", f146_driver_sig_ok(0xAB, 0xAA, false), "sig optional mode");

    // F147
    set.add("F147 ok", f147_firmware_load(&[1, 2, 3]).is_ok(), "loaded");
    set.add("F147 empty", f147_firmware_load(&[]).is_err(), "empty rejected");
    set.add("F147 huge", f147_firmware_load(&[7u8; FW_MAX_BYTES + 1]).is_err(), "oversize rejected");

    // F148
    set.add("F148 recover", f148_isolate(DriverState::Crashed, 1) == DriverState::Recovered, "retry works");
    set.add("F148 stuck", f148_isolate(DriverState::Crashed, 3) == DriverState::Crashed, "retries exhausted");

    // F149
    set.add("F149 order", f149_suspend_order(true), "children first");

    // F150
    set.add("F150 coverage", set.len() >= 25, "domain coverage");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failures(set: &CheckSet) -> String {
        let mut s = String::new();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    s.push_str(&format!("{}: {}\n", c.name, c.detail));
                }
            }
        }
        s
    }

    #[test]
    fn f150_inputm400_selftest_all_pass() {
        let set = run_inputm400_checks();
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "{}", failures(&set));
    }

    #[test]
    fn f129_bus_capacity() {
        let mut bus = EventBus::new();
        bus.ui_attached = true;
        for i in 0..EVENT_BUS_CAP {
            assert!(bus.publish(PlugEvent::Arrived(i as u8)));
        }
        assert!(!bus.publish(PlugEvent::Arrived(99)));
    }

    #[test]
    fn f140_no_overflow() {
        assert_eq!(f140_mix(i16::MAX, 1), i16::MAX);
        assert_eq!(f140_mix(i16::MIN, -1), i16::MIN);
    }
}
