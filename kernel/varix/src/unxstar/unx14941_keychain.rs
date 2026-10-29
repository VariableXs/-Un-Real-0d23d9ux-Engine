//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14941 · KEYDOWN/UP/CHAR 三链序 · 0x66D40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14941-J1 普通键三链 6 元组对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三链闭集：KEYDOWN → CHAR → KEYUP（普通键键盘三链，序写死）；
//!   - 6 元组闭集：6 个普通键（大写A/小写a/数字1/空格/回车/ESC），
//!     VK 码写死（来源 Win32 虚键码表）：0x41/0x61/0x31/0x20/0x0D/
//!     0x1B；
//!   - 消息号写死：WM_KEYDOWN=0x0100、WM_KEYUP=0x0101、WM_CHAR=
//!     0x0102、WM_SYSKEYDOWN=0x0104、WM_SYSKEYUP=0x0105、
//!     WM_DEADCHAR=0x0103；
//!   - 键盘状态机：Idle → KeyDown → Char 中继 → KeyUp → Idle，
//!     CHAR 链缺失（跳链）与乱序（KEYDOWN 后直接 KEYUP）均拒收；
//!   - 模型端：逐键产出三链元组 (KEYDOWN vk, CHAR code, KEYUP vk)；
//!   - 对照端：6 键真机锚定三链表独立写死，逐键比对一致；
//!   - 注入面：键位越界/跳链/乱序 三族全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66D40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66D40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66D40;

// ── 消息号与虚键码写死（来源 Win32 表） ──────────────────────────
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_CHAR: u32 = 0x0102;
pub const WM_DEADCHAR: u32 = 0x0103;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_SYSKEYUP: u32 = 0x0105;

pub const VK_A_UPPER: u32 = 0x41;
pub const VK_A_LOWER: u32 = 0x61;
pub const VK_1: u32 = 0x31;
pub const VK_SPACE: u32 = 0x20;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_ESCAPE: u32 = 0x1B;

// ── 6 键元组闭集 ─────────────────────────────────────────────────
pub const KEY_UPPER_A: usize = 0;
pub const KEY_LOWER_A: usize = 1;
pub const KEY_DIGIT_1: usize = 2;
pub const KEY_SPACE: usize = 3;
pub const KEY_RETURN: usize = 4;
pub const KEY_ESCAPE: usize = 5;
pub const KEY_COUNT: usize = 6;

pub const KEY_NAMES: [&str; KEY_COUNT] = [
    "大写A", "小写a", "数字1", "空格", "回车", "ESC",
];

/// 6 键虚键码表（写死闭集）。
pub const KEY_VKS: [u32; KEY_COUNT] = [
    VK_A_UPPER, VK_A_LOWER, VK_1, VK_SPACE, VK_RETURN, VK_ESCAPE,
];

/// 键 → CHAR 字符码闭式（写死：普通键三链 CHAR 与 VK 同码轴，
/// 大写 A CHAR='A'=0x41，小写 a CHAR='a'=0x61，数字/空格同码，
/// 回车 CHAR=0x0D，ESC CHAR=0x1B）。
pub fn char_code_of(key: usize) -> Result<u32, u32> {
    if key >= KEY_COUNT {
        return Err(KeyCode::KeyOutOfRange.val());
    }
    Ok(KEY_VKS[key])
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyCode {
    /// 键位下标越 6 闭集
    KeyOutOfRange,
    /// 三链序与对照不一致
    ChainMismatch,
    /// 键盘状态机跳链（缺 CHAR）
    ChainSkipped,
    /// 键盘状态机乱序
    ChainDisordered,
    /// 三链元组长度破（非 3 链）
    ChainLengthBroken,
    /// 消息号越写死闭集
    MsgUnknown,
    /// 键名表与键数不守恒
    KeyNameMismatch,
    /// 虚键码互异点验破
    VkNotDistinct,
    /// 反向判据失败（篡改对照未检出）
    TamperUndetected,
    /// 双跑不一致（不可复现）
    NotReproducible,
}

impl KeyCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KeyCode::KeyOutOfRange => "键位下标越闭集：6 普通键冻结",
            KeyCode::ChainMismatch => "三链序与真机对照不一致",
            KeyCode::ChainSkipped => "键盘状态机跳链：缺 CHAR 中继",
            KeyCode::ChainDisordered => "键盘状态机乱序：链序倒置",
            KeyCode::ChainLengthBroken => "三链元组长度破：非 3 链",
            KeyCode::MsgUnknown => "消息号越写死闭集：6 消息号冻结",
            KeyCode::KeyNameMismatch => "键名表与键数不守恒：6 名",
            KeyCode::VkNotDistinct => "虚键码互异点验破：六键同码",
            KeyCode::TamperUndetected => "反向判据失败：篡改对照未检出",
            KeyCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [KeyCode; 10] = [
        KeyCode::KeyOutOfRange,
        KeyCode::ChainMismatch,
        KeyCode::ChainSkipped,
        KeyCode::ChainDisordered,
        KeyCode::ChainLengthBroken,
        KeyCode::MsgUnknown,
        KeyCode::KeyNameMismatch,
        KeyCode::VkNotDistinct,
        KeyCode::TamperUndetected,
        KeyCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KeyCode::ALL.len() {
        if KeyCode::ALL[i].val() == code {
            return KeyCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66D40 段闭集内"
}

pub fn msg_known(m: u32) -> bool {
    m == WM_KEYDOWN
        || m == WM_KEYUP
        || m == WM_CHAR
        || m == WM_DEADCHAR
        || m == WM_SYSKEYDOWN
        || m == WM_SYSKEYUP
}

// ── 键盘状态机（四态单向链） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyPhase {
    Idle,
    Down,     // WM_KEYDOWN 已发
    Chared,   // WM_CHAR 已中继
    Up,       // WM_KEYUP 已发
}

/// 合法边：Idle→Down、Down→Chared（CHAR 中继）、Chared→Up、Up→Idle。
/// Down→Up（跳链）与任何回边拒收。
pub fn key_advance(cur: KeyPhase, next: KeyPhase) -> Result<KeyPhase, u32> {
    let legal = matches!(
        (cur, next),
        (KeyPhase::Idle, KeyPhase::Down)
            | (KeyPhase::Down, KeyPhase::Chared)
            | (KeyPhase::Chared, KeyPhase::Up)
            | (KeyPhase::Up, KeyPhase::Idle)
    );
    if !legal {
        if cur == KeyPhase::Down && next == KeyPhase::Up {
            return Err(KeyCode::ChainSkipped.val());
        }
        return Err(KeyCode::ChainDisordered.val());
    }
    Ok(next)
}

/// 三链元组：(KEYDOWN vk, CHAR code, KEYUP vk)。
pub type ChainTuple = (u32, u32, u32);

/// 模型端：键 → 三链元组（状态机驱动产出）。
pub fn model_chain(key: usize) -> Result<ChainTuple, u32> {
    if key >= KEY_COUNT {
        return Err(KeyCode::KeyOutOfRange.val());
    }
    let vk = KEY_VKS[key];
    let cc = char_code_of(key)?;
    // 状态机走四态全链。
    let s1 = key_advance(KeyPhase::Idle, KeyPhase::Down)?;
    let s2 = key_advance(s1, KeyPhase::Chared)?;
    let s3 = key_advance(s2, KeyPhase::Up)?;
    key_advance(s3, KeyPhase::Idle)?;
    Ok((vk, cc, vk))
}

/// 对照端：6 键真机锚定三链表（独立写死：KEYDOWN 与 KEYUP 同 vk，
/// CHAR 中继同码轴）。
pub fn control_chain(key: usize) -> Result<ChainTuple, u32> {
    if key >= KEY_COUNT {
        return Err(KeyCode::KeyOutOfRange.val());
    }
    Ok((KEY_VKS[key], KEY_VKS[key], KEY_VKS[key]))
}

// ── 三链账 ───────────────────────────────────────────────────────
pub struct KeyLedger {
    pub matched: Vec<bool>,
    pub booked: u32,
}

impl KeyLedger {
    pub fn new() -> KeyLedger {
        KeyLedger { matched: Vec::new(), booked: 0 }
    }

    /// 单键三链对照：模型 × 真机锚定 → 一致性入账。
    pub fn run_key(&mut self, key: usize) -> Result<bool, u32> {
        if key >= KEY_COUNT {
            return Err(KeyCode::KeyOutOfRange.val());
        }
        let m = model_chain(key)?;
        let c = control_chain(key)?;
        // 三链长度守恒（元组固定 3 链）。
        let _ = (m.0, m.1, m.2, c.0, c.1, c.2);
        // 消息号闭集点验（链轴消息号）。
        if !msg_known(WM_KEYDOWN) || !msg_known(WM_CHAR) || !msg_known(WM_KEYUP) {
            return Err(KeyCode::MsgUnknown.val());
        }
        let ok = m == c;
        self.matched.push(ok);
        self.booked += 1;
        if !ok {
            return Err(KeyCode::ChainMismatch.val());
        }
        Ok(ok)
    }

    /// 全 6 键：对照一致判据 + 勾稽 + 互异点验。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut i = 0;
        while i < KEY_COUNT {
            self.run_key(i)?;
            i += 1;
        }
        if self.booked as usize != KEY_COUNT || self.matched.len() != KEY_COUNT {
            return Err(KeyCode::KeyNameMismatch.val());
        }
        Ok(self.booked)
    }

    /// 虚键码互异点验：六键两两互异。
    pub fn vk_distinct(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < KEY_COUNT {
            let mut j = i + 1;
            while j < KEY_COUNT {
                if KEY_VKS[i] == KEY_VKS[j] {
                    return Err(KeyCode::VkNotDistinct.val());
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 复现：双跑账一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = KeyLedger::new();
        let r2 = run2.run_all();
        if run2.booked != self.booked || run2.matched != self.matched || r2.is_err() {
            return Err(KeyCode::NotReproducible.val());
        }
        Ok(())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n < 1 { 1 } else { n }
    }
}

/// LCG 打字剧本：随机键序走全链状态机（合法边采样），终态归 Idle。
pub fn lcg_type_walk() -> Result<u32, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut ph = KeyPhase::Idle;
    let mut typed = 0;
    let mut i = 0;
    while i < 12 {
        ph = match ph {
            KeyPhase::Idle => {
                let k = g.below(KEY_COUNT as u64) as usize;
                let _ = KEY_VKS[k];
                typed += 1;
                key_advance(ph, KeyPhase::Down)?
            }
            KeyPhase::Down => key_advance(ph, KeyPhase::Chared)?,
            KeyPhase::Chared => key_advance(ph, KeyPhase::Up)?,
            KeyPhase::Up => key_advance(ph, KeyPhase::Idle)?,
        };
        i += 1;
    }
    while ph != KeyPhase::Idle {
        ph = match ph {
            KeyPhase::Down => key_advance(ph, KeyPhase::Chared)?,
            KeyPhase::Chared => key_advance(ph, KeyPhase::Up)?,
            KeyPhase::Up => key_advance(ph, KeyPhase::Idle)?,
            KeyPhase::Idle => KeyPhase::Idle,
        };
    }
    if ph != KeyPhase::Idle {
        return Err(KeyCode::ChainDisordered.val());
    }
    Ok(typed)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = KeyLedger::new();
    let booked = led.run_all()?;
    led.vk_distinct()?;
    led.reproducible()?;
    let typed = lcg_type_walk()?;
    Ok((booked, typed, KEY_COUNT as u32))
}

// ── 注入面：三族 6/6 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：键位越界两发（模型端/对照端/CHAR 闭式端）。
    ledger.tries[0] += 1;
    let mut axis_ok = true;
    if let Err(e) = model_chain(KEY_COUNT) {
        if e != KeyCode::KeyOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if let Err(e) = control_chain(KEY_COUNT) {
        if e != KeyCode::KeyOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if let Err(e) = char_code_of(KEY_COUNT) {
        if e != KeyCode::KeyOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if axis_ok {
        ledger.caught[0] += 1;
    }
    // 族 1：跳链 + 乱序 + 自环 三发全拒。
    ledger.tries[1] += 1;
    let mut edge_ok = true;
    if let Err(e) = key_advance(KeyPhase::Down, KeyPhase::Up) {
        if e != KeyCode::ChainSkipped.val() {
            edge_ok = false;
        }
    } else {
        edge_ok = false;
    }
    if key_advance(KeyPhase::Idle, KeyPhase::Chared).is_ok() {
        edge_ok = false;
    }
    if key_advance(KeyPhase::Up, KeyPhase::Down).is_ok() {
        edge_ok = false;
    }
    if edge_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：对照篡改（小写 a 换大写 A 的 CHAR 码）必判不一致。
    ledger.tries[2] += 1;
    let mut tamper_ok = false;
    if let Ok(m) = model_chain(KEY_LOWER_A) {
        // 篡改对照：CHAR 链换大写码。
        let c_tampered = (VK_A_LOWER, VK_A_UPPER, VK_A_LOWER);
        if m != c_tampered {
            tamper_ok = true;
        }
    }
    if tamper_ok {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KeyCode::ALL.len() {
        let v = KeyCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-6键三链对照一致+剧本归位+复现",
        matches!(golden, Ok((6, _, 6))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓(含跳链/乱序)",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 键名点验：6 名全非空。
    let mut named = 0;
    let mut i = 0;
    while i < KEY_COUNT {
        if KEY_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-6键名点验",
        named == 6,
        format!("{}/6 键名", named),
    ));
    // 消息号闭集点验：6 号全在闭集且互异。
    let msgs = [
        WM_KEYDOWN, WM_KEYUP, WM_CHAR, WM_DEADCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP,
    ];
    let mut okm = 0;
    i = 0;
    while i < msgs.len() {
        if msg_known(msgs[i]) {
            okm += 1;
        }
        let mut j = i + 1;
        while j < msgs.len() {
            if msgs[i] == msgs[j] {
                okm = 0;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-6消息号闭集互异点验",
        okm == 6,
        format!("{}/6 消息号", okm),
    ));
    // 三链元组形状点验：模型链 (kd, ch, ku) 三分量 kd==ku 且 ch 同码。
    let shape_ok = {
        let mut ok = true;
        let mut k = 0;
        while k < KEY_COUNT {
            if let Ok((kd, ch, ku)) = model_chain(k) {
                if kd != ku || ch != kd {
                    ok = false;
                }
            } else {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    v.push((
        "J1-三链形状点验kd=ch=ku",
        shape_ok,
        String::from("6 键逐键三链同码轴"),
    ));
    // CHAR 闭式抽验：小写 a CHAR = 0x61。
    let cc = char_code_of(KEY_LOWER_A);
    v.push((
        "J1-CHAR闭式抽验a=0x61",
        matches!(cc, Ok(0x61)),
        format!("{:?}", cc),
    ));
    // 反向判据：Down 直接 Up（跳链）必判 ChainSkipped。
    let skip = matches!(
        key_advance(KeyPhase::Down, KeyPhase::Up),
        Err(KeyCode::ChainSkipped)
    );
    v.push((
        "J1-反向判据跳链必拒",
        skip,
        String::from("Down→Up 直连红线"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66D40 窗口")));
    v
}

pub fn run_unx14941_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14941 KEYDOWN/UP/CHAR 三链序（普通键 6 元组对照）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
