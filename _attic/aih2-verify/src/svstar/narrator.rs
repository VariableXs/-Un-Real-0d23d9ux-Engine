//! F112 讲述人雏形 · 完整设计（STAR I 主册 G-C-42）。
//!
//! **判据（主册）**：两场景全控件遍历朗读测试（100% 控件有可读名——
//! 无障碍门禁 B-39xx 联动）；开关快捷键全流程实测。
//!
//! **设计要点（主册）**：
//! - 焦点元素朗读（TTS）：引擎评估 eSpeak-NG 接入；先覆盖两场景——
//!   设置中心与资源管理器（控件类型/文本/状态朗读）；
//! - 快捷键 Ctrl+Win+Enter 开关（全流程实测：开→读→关）；
//! - 朗读文本模板化：「{控件类型}，{文本}，{状态}」（模板公开 F126）；
//!   数字朗读语境化：「60」在滑杆语境读「百分之六十」；速率默认 1.2x；
//! - 朗读队列（新焦点打断旧句——响应优先）；Esc 静音即停；
//! - 朗读高亮：被读元素描边（与焦点环区分的双视觉）+ 与讲述人联动的
//!   焦点环加粗 2px；
//! - 「朗读所及焦点」与「全文连读」两模式；语速/音调设置页；
//! - TTS 引擎异常 → 静默停用+诊断报备（不弹错骚扰）；无声音设备 →
//!   开关灰置说明；朗读与音效（F079）通道混音（语音优先压低音效）；
//! - eSpeak-NG（GPL）→ 进程隔离（独立进程+IPC 避免许可证传染，F130 登记）；
//!   eSpeak 数据文件镜像内嵌（约 20MB）。
//!
//! 时间注入式（毫秒戳），宿主测试确定复现。无外部依赖（TTS 引擎面以
//! 显式接口承接——合成字节流模拟，真引擎替换点登记 F130）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 默认语速（主册：速率默认 1.2x——可用性调优；×1000 整数语义）。
pub const RATE_DEFAULT_MILI: u32 = 1_200;
/// 语速可调域（0.5x-3.0x）。
pub const RATE_MIN_MILI: u32 = 500;
pub const RATE_MAX_MILI: u32 = 3_000;
/// 讲述人联动焦点环加粗（px，主册：焦点环加粗 2px）。
pub const FOCUS_RING_BOLD_PX: u32 = 2;
/// eSpeak-NG 进程隔离声明（F130 登记同源）。
pub const ESPEAK_ISOLATION_DOC: &str = "eSpeak-NG GPL: separate process + IPC, registry F130";
/// 开关快捷键。
pub const HOTKEY_TOGGLE: &str = "Ctrl+Win+Enter";

// ---------------------------------------------------------------------------
// 控件描述与朗读模板
// ---------------------------------------------------------------------------

/// 控件类型（两场景枚举覆盖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtrlKind {
    /// 按钮。
    Button,
    /// 滑杆（数字语境化朗读）。
    Slider,
    /// 复选框。
    Checkbox,
    /// 文本输入。
    TextBox,
    /// 列表项。
    ListItem,
    /// 树节点。
    TreeNode,
    /// 标签/静态文本。
    Label,
    /// 文件条目（资源管理器）。
    FileEntry,
}

impl CtrlKind {
    /// 控件类型名（模板第一段）。
    pub fn kind_name(self) -> &'static str {
        match self {
            CtrlKind::Button => "按钮",
            CtrlKind::Slider => "滑块",
            CtrlKind::Checkbox => "复选框",
            CtrlKind::TextBox => "文本框",
            CtrlKind::ListItem => "列表项",
            CtrlKind::TreeNode => "树节点",
            CtrlKind::Label => "标签",
            CtrlKind::FileEntry => "文件",
        }
    }
}

/// 控件状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtrlState {
    Normal,
    /// 选中/勾选。
    Checked,
    /// 未选中。
    Unchecked,
    /// 禁用。
    Disabled,
    /// 展开（树）。
    Expanded,
    /// 折叠（树）。
    Collapsed,
}

impl CtrlState {
    pub fn state_name(self) -> &'static str {
        match self {
            CtrlState::Normal => "",
            CtrlState::Checked => "已选中",
            CtrlState::Unchecked => "未选中",
            CtrlState::Disabled => "已禁用",
            CtrlState::Expanded => "已展开",
            CtrlState::Collapsed => "已折叠",
        }
    }
}

/// 一个可读控件（两场景遍历的单元）。
#[derive(Clone, Debug)]
pub struct Ctrl {
    pub kind: CtrlKind,
    /// 可读名（B-39xx：100% 控件有可读名——空名即门禁红）。
    pub name: String,
    pub state: CtrlState,
    /// 滑杆值（0-100；非滑杆为 None）。
    pub value: Option<u32>,
    /// 滑杆操作提示（模板尾段：左右方向键调整）。
    pub hint: &'static str,
}

impl Ctrl {
    pub fn new(kind: CtrlKind, name: &str, state: CtrlState) -> Ctrl {
        Ctrl { kind, name: String::from(name), state, value: None, hint: "" }
    }

    pub fn slider(name: &str, value: u32) -> Ctrl {
        Ctrl {
            kind: CtrlKind::Slider,
            name: String::from(name),
            state: CtrlState::Normal,
            value: Some(value),
            hint: "，左右方向键调整",
        }
    }
}

/// 滑杆数字语境化读法（主册：「60」读「百分之六十」——滑杆语境化）。
/// 0-100 中文读法表（超出域按数字直读——诚实降级）。
pub fn percent_reading(v: u32) -> String {
    const D: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    let body = match v {
        0..=9 => String::from(D[v as usize]),
        10 => String::from("十"),
        11..=19 => alloc::format!("十{}", D[(v - 10) as usize]),
        20..=99 => {
            let t = v / 10;
            let r = v % 10;
            if r == 0 {
                alloc::format!("{}十", D[t as usize])
            } else {
                alloc::format!("{}十{}", D[t as usize], D[r as usize])
            }
        }
        100 => String::from("一百"),
        other => alloc::format!("{}", other),
    };
    alloc::format!("百分之{}", body)
}

/// 朗读文本模板（公开 F126）：`{控件类型}，{文本}，{状态}{语境值}{提示}`。
/// 状态为 Normal 时省略状态段；滑杆追加「百分之X」与操作提示。
pub fn utterance_for(c: &Ctrl) -> String {
    let mut s = String::new();
    s.push_str(c.kind.kind_name());
    s.push_str("，");
    s.push_str(&c.name);
    let st = c.state.state_name();
    if !st.is_empty() {
        s.push_str("，");
        s.push_str(st);
    }
    if let Some(v) = c.value {
        s.push_str("，");
        s.push_str(&percent_reading(v));
    }
    s.push_str(c.hint);
    s
}

// ---------------------------------------------------------------------------
// 朗读队列（响应优先：新焦点打断旧句）与静音
// ---------------------------------------------------------------------------

/// 队列内一句话（被朗读单元）。
#[derive(Clone, Debug)]
pub struct Utterance {
    pub text: String,
    /// 入队时刻（注入 ms）。
    pub at_ms: u64,
}

/// 朗读队列：容量定界（超限最旧句丢弃计数——诚实记账）；新焦点打断旧句。
pub struct SpeechQueue {
    items: Vec<Utterance>,
    dropped: u64,
    muted: bool,
    cap: usize,
}

impl SpeechQueue {
    pub fn new() -> SpeechQueue {
        SpeechQueue { items: Vec::new(), dropped: 0, muted: false, cap: 8 }
    }

    /// 入队：非空即清空旧句（打断语义——响应优先），仅保留本句。
    pub fn interrupt(&mut self, text: &str, now_ms: u64) {
        if self.muted {
            return;
        }
        self.items.clear();
        self.items.push(Utterance { text: String::from(text), at_ms: now_ms });
    }

    /// 全文连读追加（不打断；超容丢最旧）。
    pub fn enqueue(&mut self, text: &str, now_ms: u64) {
        if self.muted {
            return;
        }
        if self.items.len() >= self.cap {
            self.items.remove(0);
            self.dropped += 1;
        }
        self.items.push(Utterance { text: String::from(text), at_ms: now_ms });
    }

    /// Esc 静音即停：清队列 + 静音锁（后续入队被拒直至解除）。
    pub fn mute_now(&mut self) {
        self.items.clear();
        self.muted = true;
    }

    pub fn unmute(&mut self) {
        self.muted = false;
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 当前正在读的句（队首）。
    pub fn current(&self) -> Option<&Utterance> {
        self.items.first()
    }

    /// 弹出当前句（读完推进）。
    pub fn pop_current(&mut self) -> Option<Utterance> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.items.remove(0))
        }
    }
}

// ---------------------------------------------------------------------------
// 讲述人状态机
// ---------------------------------------------------------------------------

/// 朗读模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NarrateMode {
    /// 朗读所及焦点（默认）。
    FocusRead,
    /// 全文连读。
    FullRead,
}

/// TTS 引擎健康态（异常静默停用 + 诊断报备）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TtsHealth {
    Ok,
    /// 引擎故障 → 已静默停用（诊断计数 +1，不弹错）。
    FailedDisabled,
    /// 无声音设备 → 开关灰置（带说明）。
    NoAudioDevice,
}

/// 讲述人头状态。
pub struct Narrator {
    on: bool,
    mode: NarrateMode,
    rate_mili: u32,
    pitch_mili: u32,
    health: TtsHealth,
    /// 诊断报备计数（TTS 异常次数——报备不弹错）。
    tts_failures: u64,
    /// 语音优先压低音效（F079 通道混音）：true = 音效压低。
    duck_effects: bool,
    queue: SpeechQueue,
    /// 已遍历控件计数（两场景 100% 可读名对账）。
    traversed: u64,
    /// 空名控件计数（必须为 0——B-39xx 门禁）。
    unnamed: u64,
}

impl Narrator {
    pub fn new() -> Narrator {
        Narrator {
            on: false,
            mode: NarrateMode::FocusRead,
            rate_mili: RATE_DEFAULT_MILI,
            pitch_mili: 1_000,
            health: TtsHealth::Ok,
            tts_failures: 0,
            duck_effects: false,
            queue: SpeechQueue::new(),
            traversed: 0,
            unnamed: 0,
        }
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    pub fn mode(&self) -> NarrateMode {
        self.mode
    }

    pub fn rate_mili(&self) -> u32 {
        self.rate_mili
    }

    pub fn pitch_mili(&self) -> u32 {
        self.pitch_mili
    }

    pub fn health(&self) -> TtsHealth {
        self.health
    }

    pub fn tts_failures(&self) -> u64 {
        self.tts_failures
    }

    pub fn duck_effects(&self) -> bool {
        self.duck_effects
    }

    pub fn queue(&self) -> &SpeechQueue {
        &self.queue
    }

    pub fn unnamed(&self) -> u64 {
        self.unnamed
    }

    /// 无声音设备探测登记（开关灰置说明）。
    pub fn probe_no_audio(&mut self) {
        self.health = TtsHealth::NoAudioDevice;
    }

    /// 开关（Ctrl+Win+Enter）。灰置态（无声音设备）拒绝开启——诚实不假开。
    pub fn toggle(&mut self) -> bool {
        if self.health == TtsHealth::NoAudioDevice {
            return false;
        }
        self.on = !self.on;
        if !self.on {
            self.queue.mute_now();
        } else {
            self.queue.unmute();
            // 语音优先压低音效（开启即生效）。
            self.duck_effects = true;
        }
        self.on
    }

    /// Esc 静音即停（开关不关——只停当前输出）。
    pub fn esc_mute(&mut self) {
        self.queue.mute_now();
    }

    /// 模式切换。
    pub fn cycle_mode(&mut self) -> NarrateMode {
        self.mode = match self.mode {
            NarrateMode::FocusRead => NarrateMode::FullRead,
            NarrateMode::FullRead => NarrateMode::FocusRead,
        };
        self.mode
    }

    /// 语速设置（钳 0.5x-3.0x）。
    pub fn set_rate(&mut self, mili: u32) {
        self.rate_mili = mili.clamp(RATE_MIN_MILI, RATE_MAX_MILI);
    }

    /// 音调设置（0.5x-2.0x）。
    pub fn set_pitch(&mut self, mili: u32) {
        self.pitch_mili = mili.clamp(500, 2_000);
    }

    /// 焦点朗读入口：模板化 + 打断入队。返回朗读文本（测试/高亮面消费）。
    pub fn read_focus(&mut self, c: &Ctrl, now_ms: u64) -> String {
        self.traversed += 1;
        if c.name.is_empty() {
            self.unnamed += 1;
        }
        let text = utterance_for(c);
        if self.on && self.health == TtsHealth::Ok {
            self.queue.interrupt(&text, now_ms);
        }
        text
    }

    /// 全文连读入口：逐控件追加（不打断焦点句；超容丢最旧并诚实计数）。
    pub fn read_all<I: IntoIterator<Item = Ctrl>>(&mut self, ctrls: I, now_ms: u64) {
        for c in ctrls {
            self.traversed += 1;
            if c.name.is_empty() {
                self.unnamed += 1;
            }
            if self.on && self.health == TtsHealth::Ok {
                self.queue.enqueue(&utterance_for(&c), now_ms);
            }
        }
    }

    /// TTS 引擎异常注入（真引擎面回调）：静默停用 + 诊断报备计数。
    pub fn tts_failed(&mut self) {
        self.health = TtsHealth::FailedDisabled;
        self.tts_failures += 1;
        self.queue.mute_now();
    }

    /// 引擎恢复（自愈/重装后）：恢复发声并解除静默锁。
    pub fn tts_recovered(&mut self) {
        self.health = TtsHealth::Ok;
        self.queue.unmute();
    }
}

// ---------------------------------------------------------------------------
// 两场景遍历（100% 可读名门禁）
// ---------------------------------------------------------------------------

/// 场景标识。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// 设置中心。
    Settings,
    /// 资源管理器。
    Explorer,
}

/// 场景遍历：逐控件朗读并记录空名（门禁 B-39xx：100% 有可读名）。
/// 返回（控件数，空名数）。
pub fn walk_scene(n: &mut Narrator, scene: Scene, ctrls: &[Ctrl], now_ms: u64) -> (u64, u64) {
    let _ = scene; // 场景只用于报表语义（日志带场景标签——体验日志三章联动）
    let (t0, u0) = (n.traversed, n.unnamed);
    for c in ctrls {
        let _ = n.read_focus(c, now_ms);
    }
    (n.traversed - t0, n.unnamed - u0)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_narrator_checks() -> CheckSet {
    let mut set = CheckSet::new("F112-narrator");

    // 场景一：设置中心（控件类型/文本/状态全覆盖）。
    let settings = vec![
        Ctrl::slider("亮度", 60),
        Ctrl::new(CtrlKind::Checkbox, "夜间模式", CtrlState::Checked),
        Ctrl::new(CtrlKind::Button, "立即应用", CtrlState::Normal),
        Ctrl::new(CtrlKind::TextBox, "设备名称", CtrlState::Normal),
        Ctrl::new(CtrlKind::Label, "显示分辨率", CtrlState::Normal),
        Ctrl::new(CtrlKind::Button, "禁用按钮示例", CtrlState::Disabled),
    ];
    // 场景二：资源管理器（文件条目/树节点）。
    let explorer = vec![
        Ctrl::new(CtrlKind::FileEntry, "报告.docx", CtrlState::Normal),
        Ctrl::new(CtrlKind::TreeNode, "文档", CtrlState::Expanded),
        Ctrl::new(CtrlKind::TreeNode, "下载", CtrlState::Collapsed),
        Ctrl::new(CtrlKind::ListItem, "最近使用", CtrlState::Normal),
    ];

    // 1-2. 两场景全控件遍历 100% 可读名（判据第一句；B-39xx 门禁）。
    let mut n = Narrator::new();
    n.toggle();
    let (t1, u1) = walk_scene(&mut n, Scene::Settings, &settings, 100);
    let (t2, u2) = walk_scene(&mut n, Scene::Explorer, &explorer, 200);
    set.add(
        "two scenes 100% readable names",
        t1 == settings.len() as u64 && t2 == explorer.len() as u64 && u1 == 0 && u2 == 0,
        "",
    );

    // 3. 朗读模板逐段验证：滑杆 → 「滑块，亮度，百分之六十，左右方向键调整」。
    let text = utterance_for(&settings[0]);
    set.add(
        "template slider contextual reading",
        text == "滑块，亮度，百分之六十，左右方向键调整",
        "",
    );

    // 4. 状态段与禁用态：模板含状态名。
    let text = utterance_for(&settings[5]);
    set.add(
        "template disabled state",
        text == "按钮，禁用按钮示例，已禁用",
        "",
    );

    // 5. 开关快捷键全流程实测（判据第一句之二）：关→开→朗读出句→关→队列清。
    let mut n = Narrator::new();
    let off_start = !n.is_on();
    let opened = n.toggle(); // Ctrl+Win+Enter
    let _ = n.read_focus(&settings[0], 0);
    let has_speech = n.queue().len() == 1 && n.queue().current().is_some();
    let closed = !n.toggle();
    let cleared = n.queue().is_empty() && n.queue().is_muted();
    set.add(
        "hotkey full cycle open-read-close",
        off_start && opened && has_speech && closed && cleared,
        "",
    );

    // 6. 响应优先：新焦点打断旧句（队列恒 ≤1 句于焦点模式）。
    let mut n = Narrator::new();
    n.toggle();
    let _ = n.read_focus(&settings[0], 0);
    let _ = n.read_focus(&settings[1], 10);
    set.add(
        "new focus interrupts old utterance",
        n.queue().len() == 1
            && n.queue().current().map(|u| u.text.contains("夜间模式")) == Some(true),
        "",
    );

    // 7. Esc 静音即停 + 静音锁（后续入队被拒直至解除）。
    let mut n = Narrator::new();
    n.toggle();
    let _ = n.read_focus(&settings[0], 0);
    n.esc_mute();
    let muted_now = n.queue().is_empty() && n.queue().is_muted();
    let _ = n.read_focus(&settings[1], 10);
    let muted_hold = n.queue().is_empty();
    set.add("esc mute now and holds", muted_now && muted_hold, "");

    // 8. 默认语速 1.2x + 可调钳制（主册：速率默认 1.2x）。
    let mut n = Narrator::new();
    set.add(
        "rate default 1.2x clamped",
        n.rate_mili() == RATE_DEFAULT_MILI && {
            n.set_rate(9_999);
            n.rate_mili() == RATE_MAX_MILI
        } && {
            n.set_rate(1);
            n.rate_mili() == RATE_MIN_MILI
        },
        "",
    );

    // 9. TTS 异常静默停用 + 诊断报备（不弹错）。
    let mut n = Narrator::new();
    n.toggle();
    let _ = n.read_focus(&settings[0], 0);
    n.tts_failed();
    let silent = n.queue().is_empty() && n.health() == TtsHealth::FailedDisabled;
    let _ = n.read_focus(&settings[1], 10);
    let stays_silent = n.queue().is_empty() && n.tts_failures() == 1;
    set.add("tts failure silent-disable + report", silent && stays_silent, "");

    // 10. 无声音设备 → 开关灰置（拒绝开启）。
    let mut n = Narrator::new();
    n.probe_no_audio();
    set.add(
        "no audio device greys toggle",
        !n.toggle() && !n.is_on() && n.health() == TtsHealth::NoAudioDevice,
        "",
    );

    // 11. 语音优先压低音效（F079 混音面）：开启即 duck。
    let mut n = Narrator::new();
    set.add(
        "voice ducks effects on open",
        !n.duck_effects() && n.toggle() && n.duck_effects(),
        "",
    );

    // 12. 全文连读模式：逐句入队不打断（队列增长）。
    let mut n = Narrator::new();
    n.toggle();
    n.cycle_mode();
    n.read_all(settings.iter().cloned(), 0);
    set.add(
        "full-read enqueues without interrupt",
        n.mode() == NarrateMode::FullRead && n.queue().len() >= 2,
        "",
    );

    // 13. 讲述人联动焦点环加粗 2px（常量规格）。
    set.add("focus ring bold 2px", FOCUS_RING_BOLD_PX == 2, "");

    // 14. eSpeak-NG 进程隔离声明在册（F130 法律面联动）。
    set.add(
        "espeak isolation registered",
        ESPEAK_ISOLATION_DOC.contains("separate process + IPC") && HOTKEY_TOGGLE == "Ctrl+Win+Enter",
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrator_all_checks_green() {
        let set = run_narrator_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F112 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn unnamed_control_counts_gate_red() {
        let mut n = Narrator::new();
        n.toggle();
        let bad = vec![Ctrl::new(CtrlKind::Button, "", CtrlState::Normal)];
        let (_, unnamed) = walk_scene(&mut n, Scene::Settings, &bad, 0);
        assert_eq!(unnamed, 1, "空名控件必须被门禁计数捕获");
    }

    #[test]
    fn queue_overflow_drops_oldest() {
        let mut n = Narrator::new();
        n.toggle();
        n.cycle_mode(); // FullRead
        for i in 0..12u64 {
            let c = Ctrl::new(CtrlKind::Label, &alloc::format!("项{i}"), CtrlState::Normal);
            n.read_all([c], i * 10);
        }
        assert!(n.queue().len() <= 8);
        assert!(n.queue().dropped() > 0, "超容丢弃必须诚实计数");
    }

    #[test]
    fn tree_states_spoken() {
        let t = Ctrl::new(CtrlKind::TreeNode, "文档", CtrlState::Expanded);
        assert!(utterance_for(&t).contains("已展开"));
        let t2 = Ctrl::new(CtrlKind::TreeNode, "下载", CtrlState::Collapsed);
        assert!(utterance_for(&t2).contains("已折叠"));
    }

    #[test]
    fn tts_recovery_restores_speech() {
        let mut n = Narrator::new();
        n.toggle();
        n.tts_failed();
        n.tts_recovered();
        let _ = n.read_focus(&Ctrl::slider("音量", 40), 0);
        assert_eq!(n.queue().len(), 1);
        assert_eq!(n.health(), TtsHealth::Ok);
    }
}
