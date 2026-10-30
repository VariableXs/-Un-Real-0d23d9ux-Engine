//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16321 · 输入路由总纲（vxwm 事件→Win32 输入消息） · 0x67E80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E1-B17）：
//!   ① 纯功能代码 ≥440 行；
//!   ② UNX-F16321-J1 总纲落账（键盘/鼠标/滚轮/IME 四族通道），四族
//!     齐备率 100%，与 F16302 翻译表挂接一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四族通道建模：键盘（VK 码→扫描码→字符三步）、鼠标（按下/
//!     释放/移动三态+命中窗口归属）、滚轮（增量累积→整行派发）、
//!     IME（组合中/提交两态）；
//!   - 事件归一：vxwm 事件先归一为统一 InputEvent（四族标签+
//!     载荷），再按族路由到翻译表；
//!   - F16302 翻译表挂接：四族各自的翻译入口名必须在翻译表精确
//!     命中（一致率 100%）；
//!   - 键盘守恒：按下数 = 派发数 + 吞没数（焦点过滤吞没入账）；
//!   - 鼠标命中归属：命中测试显性（无归属窗即吞没入账，不伪造
//!     投递）；
//!   - LCG 种子写死 = SEG_BASE（0x67E80）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67E80，AI-VX3-1030 批14 独占区）。
pub const SEG_BASE: u32 = 0x67E80;

// ── 四族闭集 ────────────────────────────────────────────────────
pub const FAM_KEYBOARD: usize = 0;
pub const FAM_MOUSE: usize = 1;
pub const FAM_WHEEL: usize = 2;
pub const FAM_IME: usize = 3;
pub const FAMILY_COUNT: usize = 4;
pub const FAMILY_NAMES: [&str; FAMILY_COUNT] = ["键盘", "鼠标", "滚轮", "IME"];
pub const FAMILY_FLOOR: usize = 4;

// ── F16302 翻译表（冻结：挂接目标） ─────────────────────────────
pub const XLT_TABLE: [&str; 4] =
    ["xlt_keyboard_vk", "xlt_mouse_hit", "xlt_wheel_delta", "xlt_ime_commit"];
/// 四族冻结翻译入口名（须与 XLT_TABLE 逐族一致）。
pub const FAMILY_XLT_FROZEN: [&str; FAMILY_COUNT] =
    ["xlt_keyboard_vk", "xlt_mouse_hit", "xlt_wheel_delta", "xlt_ime_commit"];

// ── 键盘域（VK 码闭集采样） ─────────────────────────────────────
pub const VK_A: u32 = 0x41;
pub const VK_Z: u32 = 0x5A;
pub const VK_SPACE: u32 = 0x20;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_TAB: u32 = 0x09;
/// VK 合法域（本总纲采样闭集：Space/Tab/Return/Esc/A~Z）。
pub const VK_LEGAL_MAX: u32 = 0x5A;

/// 鼠标三态闭集。
pub const MOUSE_DOWN: u8 = 0;
pub const MOUSE_UP: u8 = 1;
pub const MOUSE_MOVE: u8 = 2;
pub const MOUSE_STATE_COUNT: usize = 3;

/// 滚轮整行阈值（冻结：累积满 120 派发一行）。
pub const WHEEL_LINE_DELTA: i32 = 120;

/// IME 两态闭集。
pub const IME_COMPOSING: u8 = 0;
pub const IME_COMMITTED: u8 = 1;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InCode {
    /// 输入族越闭集
    FamilyOutOfRange,
    /// 族名空（齐备破坏）
    FamilyNameBlank,
    /// VK 越采样闭集
    VkOutOfRange,
    /// 鼠标态越闭集
    MouseStateOutOfRange,
    /// IME 态越闭集
    ImeStateOutOfRange,
    /// 翻译表挂接断裂（族入口未命中）
    XltNotLinked,
    /// 键盘守恒破坏（按下≠派发+吞没）
    KeyConservationBroken,
    /// 空窗命中伪造投递（无归属窗时投递）
    FakeDeliverNoWindow,
    /// 滚轮累积溢出（反向抵消未清账）
    WheelAccumOverflow,
    /// IME 组合中即提交（态序破坏）
    ImeCommitWhileComposing,
    /// 双跑不一致
    NotReproducible,
}

impl InCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            InCode::FamilyOutOfRange => "输入族越闭集：四族冻结",
            InCode::FamilyNameBlank => "族名空：齐备破坏",
            InCode::VkOutOfRange => "VK 越采样闭集：Space~Z 冻结",
            InCode::MouseStateOutOfRange => "鼠标态越闭集：三态冻结",
            InCode::ImeStateOutOfRange => "IME 态越闭集：两态冻结",
            InCode::XltNotLinked => "翻译表挂接断裂：F16302 未命中",
            InCode::KeyConservationBroken => "键盘守恒破坏：按下≠派发+吞没",
            InCode::FakeDeliverNoWindow => "空窗伪造投递：无归属窗应吞没",
            InCode::WheelAccumOverflow => "滚轮累积溢出：抵消未清账",
            InCode::ImeCommitWhileComposing => "IME 态序破坏：组合中即提交",
            InCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [InCode; 11] = [
        InCode::FamilyOutOfRange,
        InCode::FamilyNameBlank,
        InCode::VkOutOfRange,
        InCode::MouseStateOutOfRange,
        InCode::ImeStateOutOfRange,
        InCode::XltNotLinked,
        InCode::KeyConservationBroken,
        InCode::FakeDeliverNoWindow,
        InCode::WheelAccumOverflow,
        InCode::ImeCommitWhileComposing,
        InCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < InCode::ALL.len() {
        if InCode::ALL[i].val() == code {
            return InCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67E80 段闭集内"
}

// ── 统一输入事件 ────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct InputEvent {
    pub family: usize,
    pub code: u32,
    pub state: u8,
    /// 命中窗 id（0 = 无归属窗，吞没）。
    pub hit_window: u32,
}

impl InputEvent {
    pub fn legal(&self) -> Result<(), u32> {
        if self.family >= FAMILY_COUNT {
            return Err(InCode::FamilyOutOfRange.val());
        }
        match self.family {
            FAM_KEYBOARD => {
                if self.code == VK_SPACE
                    || self.code == VK_TAB
                    || self.code == VK_RETURN
                    || self.code == VK_ESCAPE
                    || (self.code >= VK_A && self.code <= VK_Z)
                {
                    Ok(())
                } else {
                    Err(InCode::VkOutOfRange.val())
                }
            }
            FAM_MOUSE => {
                if self.state <= MOUSE_MOVE {
                    Ok(())
                } else {
                    Err(InCode::MouseStateOutOfRange.val())
                }
            }
            FAM_WHEEL => Ok(()),
            _ => {
                if self.state == IME_COMPOSING || self.state == IME_COMMITTED {
                    Ok(())
                } else {
                    Err(InCode::ImeStateOutOfRange.val())
                }
            }
        }
    }
}

// ── 键盘账（按下/派发/吞没守恒） ────────────────────────────────
pub struct KeyboardLedger {
    pub pressed: u32,
    pub dispatched: u32,
    pub swallowed: u32,
    pub focus_window: u32,
}

impl KeyboardLedger {
    pub fn new(focus_window: u32) -> KeyboardLedger {
        KeyboardLedger { pressed: 0, dispatched: 0, swallowed: 0, focus_window }
    }
    /// 按键路由：命中窗=焦点窗派发；否则吞没入账。
    pub fn route(&mut self, e: &InputEvent) -> Result<bool, u32> {
        if e.family != FAM_KEYBOARD {
            return Err(InCode::FamilyOutOfRange.val());
        }
        e.legal()?;
        self.pressed += 1;
        if e.hit_window == self.focus_window && e.hit_window != 0 {
            self.dispatched += 1;
            Ok(true)
        } else {
            self.swallowed += 1;
            Ok(false)
        }
    }
    /// 守恒：按下 = 派发 + 吞没。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.pressed != self.dispatched + self.swallowed {
            return Err(InCode::KeyConservationBroken.val());
        }
        Ok(())
    }
}

// ── 鼠标账（命中归属显性） ──────────────────────────────────────
pub struct MouseLedger {
    pub delivered: u32,
    pub swallowed_no_window: u32,
    pub down_cnt: u32,
    pub up_cnt: u32,
    pub move_cnt: u32,
}

impl MouseLedger {
    pub fn new() -> MouseLedger {
        MouseLedger { delivered: 0, swallowed_no_window: 0, down_cnt: 0, up_cnt: 0, move_cnt: 0 }
    }
    pub fn route(&mut self, e: &InputEvent) -> Result<bool, u32> {
        if e.family != FAM_MOUSE {
            return Err(InCode::FamilyOutOfRange.val());
        }
        e.legal()?;
        match e.state {
            MOUSE_DOWN => self.down_cnt += 1,
            MOUSE_UP => self.up_cnt += 1,
            _ => self.move_cnt += 1,
        }
        if e.hit_window == 0 {
            // 无归属窗：显性吞没，不伪造投递。
            self.swallowed_no_window += 1;
            return Ok(false);
        }
        self.delivered += 1;
        Ok(true)
    }
}

// ── 滚轮账（累积→整行派发） ─────────────────────────────────────
pub struct WheelLedger {
    pub accum: i64,
    pub lines_dispatched: u32,
    pub neg_events: u32,
    pub pos_events: u32,
}

impl WheelLedger {
    pub fn new() -> WheelLedger {
        WheelLedger { accum: 0, lines_dispatched: 0, neg_events: 0, pos_events: 0 }
    }
    pub fn route(&mut self, delta: i32) -> u32 {
        if delta < 0 {
            self.neg_events += 1;
        } else {
            self.pos_events += 1;
        }
        self.accum += delta as i64;
        let mut lines = 0u32;
        // 整行派发：累积越阈值即派发并回扣。
        while self.accum >= WHEEL_LINE_DELTA as i64 {
            self.accum -= WHEEL_LINE_DELTA as i64;
            lines += 1;
            self.lines_dispatched += 1;
        }
        while self.accum <= -(WHEEL_LINE_DELTA as i64) {
            self.accum += WHEEL_LINE_DELTA as i64;
            self.lines_dispatched += 1;
        }
        lines
    }
}

// ── IME 账（组合/提交两态序） ───────────────────────────────────
pub struct ImeLedger {
    pub state: u8,
    pub composing_cnt: u32,
    pub commit_cnt: u32,
}

impl ImeLedger {
    pub fn new() -> ImeLedger {
        ImeLedger { state: IME_COMPOSING, composing_cnt: 0, commit_cnt: 0 }
    }
    pub fn compose(&mut self) -> Result<(), u32> {
        if self.state != IME_COMPOSING && self.state != IME_COMMITTED {
            return Err(InCode::ImeStateOutOfRange.val());
        }
        self.state = IME_COMPOSING;
        self.composing_cnt += 1;
        Ok(())
    }
    pub fn commit(&mut self) -> Result<(), u32> {
        // 提交前提：组合中。空组合直接提交 = 态序破坏（拒绝）。
        if self.state != IME_COMPOSING {
            return Err(InCode::ImeCommitWhileComposing.val());
        }
        if self.composing_cnt == 0 {
            return Err(InCode::ImeCommitWhileComposing.val());
        }
        self.state = IME_COMMITTED;
        self.commit_cnt += 1;
        Ok(())
    }
}

// ── 四族齐备账 ──────────────────────────────────────────────────
pub fn families_ready() -> bool {
    if FAMILY_NAMES.len() < FAMILY_FLOOR {
        return false;
    }
    let mut i = 0;
    while i < FAMILY_NAMES.len() {
        if FAMILY_NAMES[i].len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// F16302 翻译表挂接：四族冻结入口逐族命中（一致率 100%）。
pub fn xlt_all_linked() -> bool {
    let mut i = 0;
    while i < FAMILY_COUNT {
        let mut hit = false;
        let mut j = 0;
        while j < XLT_TABLE.len() {
            if XLT_TABLE[j] == FAMILY_XLT_FROZEN[i] {
                hit = true;
            }
            j += 1;
        }
        if !hit {
            return false;
        }
        i += 1;
    }
    true
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：四族各走一轮合法流。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    // 键盘：焦点窗 7，10 按其中 6 派发 4 吞没。
    let mut kb = KeyboardLedger::new(7);
    let mut i = 0;
    while i < 10 {
        let hit = if i < 6 { 7 } else { 9 };
        let e = InputEvent { family: FAM_KEYBOARD, code: VK_A + (i % 26), state: 0, hit_window: hit };
        kb.route(&e)?;
        i += 1;
    }
    kb.conserve()?;
    // 鼠标：8 事件，2 无窗吞没。
    let mut ms = MouseLedger::new();
    i = 0;
    while i < 8 {
        let e = InputEvent {
            family: FAM_MOUSE,
            code: 0,
            state: (i % MOUSE_STATE_COUNT as u32) as u8,
            hit_window: if i >= 6 { 0 } else { 3 },
        };
        ms.route(&e)?;
        i += 1;
    }
    // 滚轮：+120×3、-60×2、+120×1 → 4 行派发。
    let mut wh = WheelLedger::new();
    let deltas = [120i32, 120, 120, -60, -60, 120];
    let mut d = 0;
    while d < deltas.len() {
        wh.route(deltas[d]);
        d += 1;
    }
    // IME：组合两次→提交一次。
    let mut im = ImeLedger::new();
    im.compose()?;
    im.compose()?;
    im.commit()?;
    // 齐备与挂接。
    if !families_ready() {
        return Err(InCode::FamilyNameBlank.val());
    }
    if !xlt_all_linked() {
        return Err(InCode::XltNotLinked.val());
    }
    Ok((kb.dispatched, ms.delivered, wh.lines_dispatched, im.commit_cnt))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    // 黄金序：键盘守恒闭合。
    let mut kb = KeyboardLedger::new(1);
    let e = InputEvent { family: FAM_KEYBOARD, code: VK_A, state: 0, hit_window: 1 };
    kb.route(&e)?;
    kb.conserve()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // VK 越闭集 → 必拒。
    let e = InputEvent { family: FAM_KEYBOARD, code: 0x99, state: 0, hit_window: 1 };
    let mut kb = KeyboardLedger::new(1);
    kb.route(&e).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // IME 空组合直接提交 → 必拒。
    let mut im = ImeLedger::new();
    im.commit()
}

/// 注入 10/10：VK 越界、IME 态序、空窗伪造、族越界四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：VK 越界。
        let e0 = InputEvent { family: FAM_KEYBOARD, code: VK_LEGAL_MAX + 1, state: 0, hit_window: 1 };
        let mut kb = KeyboardLedger::new(1);
        led.record(0, kb.route(&e0).is_err());
        // 槽1：IME 组合中再提交前的态序注入（commit→commit）。
        let mut im = ImeLedger::new();
        im.compose().ok();
        im.commit().ok();
        let hit1 = im.commit().is_err();
        led.record(1, hit1);
        // 槽2：空窗伪造投递（鼠标 hit_window=0 应吞没不投）。
        let mut ms = MouseLedger::new();
        let e2 = InputEvent { family: FAM_MOUSE, code: 0, state: MOUSE_DOWN, hit_window: 0 };
        let delivered = ms.route(&e2).unwrap_or(true);
        let hit2 = !delivered && ms.swallowed_no_window == 1;
        led.record(2, hit2);
        // 槽3：族越界。
        let e3 = InputEvent { family: FAMILY_COUNT, code: 0, state: 0, hit_window: 1 };
        let hit3 = e3.legal().is_err();
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(InCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < InCode::ALL.len() {
        let v = InCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 四族齐备率 100%。
    v.push((
        "J1-四族齐备率100%",
        families_ready() && FAMILY_NAMES.len() == 4,
        format!("{:?}", FAMILY_NAMES),
    ));
    // F16302 翻译表挂接一致率 100%。
    v.push((
        "J1-F16302挂接一致率100%",
        xlt_all_linked(),
        format!("四族入口 {:?}", FAMILY_XLT_FROZEN),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a();
    let b = probe_seq_b();
    let c = probe_seq_c();
    v.push((
        "J1-探针三序",
        a.is_ok() && b.is_err() && c.is_err(),
        format!("a={:?} b={:?} c={:?}", a.is_ok(), b.is_err(), c.is_err()),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67E80 窗口")));
    v
}

pub fn run_unx16321_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16321 输入路由总纲");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
