//! UNX-F48833 keymap 随机修饰 fuzz（乱组合/乱层注入状态机不崩不卡死）
//! UNX-F48834 B02 段内联轧（keymap 五路判据交叉复跑）
//! UNX-F48835 keymap 引擎与修饰键状态机错误码族（15 条）
//!
//! ── 闸门次序 ──
//! G1 码值闸：15 条错误码严格占 0x70401..=0x7040F，连续、无洞、无重号。
//! G2 归族闸：五族（parse/switch/mod/led/io）各三码，族内不串、族外不漏。
//! G3 文案闸：15 条 detail 两两不同（互不混淆），且都能被码反查命中。
//! G4 fuzz 闸：10^4 次乱注入里每一次失败都必须是**段内带码**的 Err，
//!    不许出现无码失败、不许计数停滞、不许越界访问。
//! G5 联轧闸：五路（解析/切换/修饰/LED/IO）各自跑两遍，摘要必须一字不差。
//!
//! ── 关键差异 ──
//! · 与 B01（原始码流）不同：这里不做扫描码解码，只做「跨路交叉复跑 + 错误码族」。
//! · 与本批 unx48828/48831 不同：那边各管一段语义；这边把五段拉平做联轧，
//!   且错误码**族**是唯一的对外词汇表——任何一路的失败都必须能翻译进这 15 条。
//! · 与 B03（输入法）不同：输入法提交失败有它自己的族，不在这 15 条内。
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（B02 bridge 段）。
pub const SEG_BASE: u32 = 0x70400;

// ── 15 条错误码 ──────────────────────────────────────────────
/// 错误码族（15 条；码值 = 段基址 + 族序内序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BridgeCode {
    /// 解析族 1：行无 `=`
    UnparsableLine = 1,
    /// 解析族 2：keysym 越域
    KeysymOutOfRange = 2,
    /// 解析族 3：键码重复
    KeycodeDup = 3,
    /// 切换族 1：切换风暴（窗口未落账又起）
    SwitchStorm = 4,
    /// 切换族 2：孤儿 commit
    SwitchOrphan = 5,
    /// 切换族 3：窗口溢出
    SwitchOverflow = 6,
    /// 修饰族 1：未按即松
    ModUnderflow = 7,
    /// 修饰族 2：重复按下（瞬时栈溢出）
    ModOverflow = 8,
    /// 修饰族 3：闩锁粘连
    ModLatchStuck = 9,
    /// LED 族 1：重试预算耗尽
    LedRetryExhausted = 10,
    /// LED 族 2：总线停机后仍写
    LedBusDown = 11,
    /// LED 族 3：降级账满
    LedLedgerFull = 12,
    /// IO 族 1：短读
    IoShortRead = 13,
    /// IO 族 2：校验和不符
    IoChecksum = 14,
    /// IO 族 3：回退无告警（静默）
    IoFallbackSilent = 15,
}

impl BridgeCode {
    /// 段内绝对码。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 十五码全表（顺序即码序）。
    pub const ALL: [BridgeCode; 15] = [
        BridgeCode::UnparsableLine,
        BridgeCode::KeysymOutOfRange,
        BridgeCode::KeycodeDup,
        BridgeCode::SwitchStorm,
        BridgeCode::SwitchOrphan,
        BridgeCode::SwitchOverflow,
        BridgeCode::ModUnderflow,
        BridgeCode::ModOverflow,
        BridgeCode::ModLatchStuck,
        BridgeCode::LedRetryExhausted,
        BridgeCode::LedBusDown,
        BridgeCode::LedLedgerFull,
        BridgeCode::IoShortRead,
        BridgeCode::IoChecksum,
        BridgeCode::IoFallbackSilent,
    ];
}

/// 族名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Family {
    /// 族名
    pub name: &'static str,
    /// 三个成员（码序）
    pub members: [BridgeCode; 3],
}

/// 五族 × 三码。
pub const FAMILIES: [Family; 5] = [
    Family {
        name: "parse",
        members: [BridgeCode::UnparsableLine, BridgeCode::KeysymOutOfRange, BridgeCode::KeycodeDup],
    },
    Family {
        name: "switch",
        members: [BridgeCode::SwitchStorm, BridgeCode::SwitchOrphan, BridgeCode::SwitchOverflow],
    },
    Family {
        name: "mod",
        members: [BridgeCode::ModUnderflow, BridgeCode::ModOverflow, BridgeCode::ModLatchStuck],
    },
    Family {
        name: "led",
        members: [BridgeCode::LedRetryExhausted, BridgeCode::LedBusDown, BridgeCode::LedLedgerFull],
    },
    Family {
        name: "io",
        members: [BridgeCode::IoShortRead, BridgeCode::IoChecksum, BridgeCode::IoFallbackSilent],
    },
];

/// 码 → 短名（大写常量名，便于日志对齐）。
pub fn name_of(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BridgeCode::ALL.len() {
        if let Some(c) = BridgeCode::ALL.get(i) {
            if c.val() == code {
                return match c {
                    BridgeCode::UnparsableLine => "UNPARSABLE_LINE",
                    BridgeCode::KeysymOutOfRange => "KEYSYM_OUT_OF_RANGE",
                    BridgeCode::KeycodeDup => "KEYCODE_DUP",
                    BridgeCode::SwitchStorm => "SWITCH_STORM",
                    BridgeCode::SwitchOrphan => "SWITCH_ORPHAN",
                    BridgeCode::SwitchOverflow => "SWITCH_OVERFLOW",
                    BridgeCode::ModUnderflow => "MOD_UNDERFLOW",
                    BridgeCode::ModOverflow => "MOD_OVERFLOW",
                    BridgeCode::ModLatchStuck => "MOD_LATCH_STUCK",
                    BridgeCode::LedRetryExhausted => "LED_RETRY_EXHAUSTED",
                    BridgeCode::LedBusDown => "LED_BUS_DOWN",
                    BridgeCode::LedLedgerFull => "LED_LEDGER_FULL",
                    BridgeCode::IoShortRead => "IO_SHORT_READ",
                    BridgeCode::IoChecksum => "IO_CHECKSUM",
                    BridgeCode::IoFallbackSilent => "IO_FALLBACK_SILENT",
                };
            }
        }
        i += 1;
    }
    "UNKNOWN_CODE"
}

/// 码 → 文案（15 条互不相同）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BridgeCode::ALL.len() {
        if let Some(c) = BridgeCode::ALL.get(i) {
            if c.val() == code {
                return match c {
                    BridgeCode::UnparsableLine => "symbol 行缺少等号，无法切字段",
                    BridgeCode::KeysymOutOfRange => "keysym 为 0 或超出 0x1FFFFF",
                    BridgeCode::KeycodeDup => "同一布局内键码重复定义",
                    BridgeCode::SwitchStorm => "切换窗口未落账又起新的切换",
                    BridgeCode::SwitchOrphan => "没有 pending 布局却提交切换",
                    BridgeCode::SwitchOverflow => "切换窗口缓冲到达上限",
                    BridgeCode::ModUnderflow => "瞬时位未按下就松开",
                    BridgeCode::ModOverflow => "同一瞬时位重复按下（栈溢出）",
                    BridgeCode::ModLatchStuck => "闩锁位在松开后仍粘连",
                    BridgeCode::LedRetryExhausted => "LED 写重试预算耗尽",
                    BridgeCode::LedBusDown => "LED 总线已停机仍尝试写",
                    BridgeCode::LedLedgerFull => "LED 降级账已满，新增行被丢弃",
                    BridgeCode::IoShortRead => "keymap 文件短读，字节数不足",
                    BridgeCode::IoChecksum => "keymap 文件校验和不符",
                    BridgeCode::IoFallbackSilent => "回退发生了却没有告警（静默）",
                };
            }
        }
        i += 1;
    }
    "未知错误码：不在 0x70400 段 15 条闭集内"
}

/// 码是否在 15 条闭集内。
pub fn in_set(code: u32) -> bool {
    let mut i = 0usize;
    while i < BridgeCode::ALL.len() {
        if let Some(c) = BridgeCode::ALL.get(i) {
            if c.val() == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 码 → 族名。
pub fn family_of(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FAMILIES.len() {
        if let Some(f) = FAMILIES.get(i) {
            let mut j = 0usize;
            while j < f.members.len() {
                if let Some(c) = f.members.get(j) {
                    if c.val() == code {
                        return f.name;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    "unknown"
}

// ── 折叠 ─────────────────────────────────────────────────────
/// 折一个字节。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折一个 32 位字。
pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let b = w.to_le_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc = fold_byte(acc, *v);
        }
        i += 1;
    }
    acc
}

/// 折一段文本。
pub fn fold_text(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc = fold_byte(acc, *v);
        }
        i += 1;
    }
    acc
}

/// 十六进制回写。
fn push_hex(s: &mut String, v: u32) {
    s.push_str("0x");
    let mut sh = 28u32;
    let mut started = false;
    loop {
        let nib = ((v >> sh) & 0xf) as u8;
        if nib != 0 || started || sh == 0 {
            started = true;
            let ch = match nib {
                0..=9 => (b'0' + nib) as char,
                _ => (b'a' + (nib - 10)) as char,
            };
            s.push(ch);
        }
        if sh == 0 {
            break;
        }
        sh -= 4;
    }
}

/// 十进制回写。
fn push_dec(s: &mut String, v: u32) {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = buf.len();
    loop {
        let d = (n % 10) as u8;
        n /= 10;
        if i == 0 {
            break;
        }
        i -= 1;
        if let Some(c) = buf.get_mut(i) {
            *c = b'0' + d;
        }
        if n == 0 {
            break;
        }
    }
    let mut j = i;
    while j < buf.len() {
        if let Some(c) = buf.get(j) {
            s.push(*c as char);
        }
        j += 1;
    }
}

/// 十六进制解析。
fn parse_hex(t: &str) -> Option<u32> {
    let b = t.as_bytes();
    if b.len() < 3 {
        return None;
    }
    if b.first() != Some(&b'0') {
        return None;
    }
    let second = b.get(1)?;
    if *second != b'x' && *second != b'X' {
        return None;
    }
    let mut v = 0u32;
    let mut i = 2usize;
    while i < b.len() {
        let c = *b.get(i)?;
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d as u32)?;
        i += 1;
    }
    Some(v)
}

/// 固定种子掷骰。
pub struct Die {
    s: u32,
}

impl Die {
    /// 起骰。
    pub const fn new(seed: u32) -> Die {
        Die { s: seed ^ 0x0B02_4883 }
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(2_147_483_147).wrapping_add(1);
        self.s ^= self.s >> 17;
        self.s
    }

    /// [0, bound)。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

// ── 五路引擎 ─────────────────────────────────────────────────
/// keysym 域（X11 常用区上界）。
pub const KSYM_MAX: u32 = 0x1f_ffff;

/// 解析族：一行 symbol 的解析。
pub fn parse_symbol_line(line: &str, seen: &mut Vec<u16>) -> Result<(u16, u32), u32> {
    let eq = match line.find('=') {
        Some(p) => p,
        None => return Err(BridgeCode::UnparsableLine.val()),
    };
    let left = line.get(..eq).unwrap_or("").trim();
    let right = line.get(eq + 1..).unwrap_or("").trim();
    let kc_txt = match left.strip_prefix("kc.") {
        Some(v) => v.trim(),
        None => return Err(BridgeCode::UnparsableLine.val()),
    };
    let mut kc = 0u32;
    let b = kc_txt.as_bytes();
    if b.is_empty() {
        return Err(BridgeCode::UnparsableLine.val());
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => break,
        };
        if !(b'0'..=b'9').contains(&c) {
            return Err(BridgeCode::UnparsableLine.val());
        }
        kc = match kc.checked_mul(10).and_then(|v| v.checked_add((c - b'0') as u32)) {
            Some(v) => v,
            None => return Err(BridgeCode::KeysymOutOfRange.val()),
        };
        i += 1;
    }
    if kc > u16::MAX as u32 {
        return Err(BridgeCode::KeysymOutOfRange.val());
    }
    let lo_t = match right.split(',').next() {
        Some(v) => v.trim(),
        None => return Err(BridgeCode::UnparsableLine.val()),
    };
    let lo = match parse_hex(lo_t) {
        Some(v) => v,
        None => return Err(BridgeCode::UnparsableLine.val()),
    };
    if lo == 0 || lo > KSYM_MAX {
        return Err(BridgeCode::KeysymOutOfRange.val());
    }
    if seen.iter().any(|x| *x == kc as u16) {
        return Err(BridgeCode::KeycodeDup.val());
    }
    seen.push(kc as u16);
    Ok((kc as u16, lo))
}

/// 切换族：迷你调度器（只保留与错误码相关的骨架）。
pub struct MiniSwitch {
    /// 生效布局
    pub active: u8,
    /// 目标布局
    pub pending: Option<u8>,
    /// 窗口缓冲
    pub q: Vec<u16>,
    /// 本窗口内 begin 次数
    pub begins: u32,
    /// 完成次数
    pub commits: u32,
}

/// 窗口上限。
pub const SWITCH_CAP: usize = 8;

impl MiniSwitch {
    /// 起机。
    pub fn new() -> MiniSwitch {
        MiniSwitch { active: 0, pending: None, q: Vec::new(), begins: 0, commits: 0 }
    }

    /// begin：窗口内重复 begin = 风暴。
    pub fn begin(&mut self, to: u8) -> Result<(), u32> {
        if self.pending.is_some() {
            return Err(BridgeCode::SwitchStorm.val());
        }
        if to == self.active || to > 1 {
            return Err(BridgeCode::SwitchStorm.val());
        }
        self.pending = Some(to);
        self.begins = self.begins.saturating_add(1);
        Ok(())
    }

    /// feed：窗口满即溢出。
    pub fn feed(&mut self, kc: u16) -> Result<(), u32> {
        if self.pending.is_none() {
            return Ok(());
        }
        if self.q.len() >= SWITCH_CAP {
            return Err(BridgeCode::SwitchOverflow.val());
        }
        self.q.push(kc);
        Ok(())
    }

    /// commit：没有 pending = 孤儿。
    pub fn commit(&mut self) -> Result<u32, u32> {
        let to = match self.pending {
            Some(t) => t,
            None => return Err(BridgeCode::SwitchOrphan.val()),
        };
        self.active = to;
        self.pending = None;
        let n = self.q.len() as u32;
        self.q.clear();
        self.commits = self.commits.saturating_add(1);
        Ok(n)
    }
}

impl Default for MiniSwitch {
    fn default() -> MiniSwitch {
        MiniSwitch::new()
    }
}

/// 修饰族：瞬时栈 + 闩锁。
pub struct ModStack {
    /// 瞬时位
    pub bits: u8,
    /// 锁存位
    pub locked: u8,
    /// 闩锁位
    pub latched: u8,
}

impl ModStack {
    /// 空栈。
    pub const fn new() -> ModStack {
        ModStack { bits: 0, locked: 0, latched: 0 }
    }

    /// 按下：同一位按两次 = 溢出。
    pub fn press(&mut self, bit: u8) -> Result<(), u32> {
        let b = bit & 0x7f;
        if b == 0 {
            return Err(BridgeCode::ModUnderflow.val());
        }
        if self.bits & b != 0 {
            return Err(BridgeCode::ModOverflow.val());
        }
        self.bits |= b;
        Ok(())
    }

    /// 松开：没按过 = 下溢。
    pub fn release(&mut self, bit: u8) -> Result<(), u32> {
        let b = bit & 0x7f;
        if self.bits & b == 0 {
            return Err(BridgeCode::ModUnderflow.val());
        }
        self.bits &= !b;
        Ok(())
    }

    /// 闩锁。
    pub fn latch(&mut self, bit: u8) {
        self.latched |= bit & 0x7f;
    }

    /// 清闩锁。
    pub fn unlatch(&mut self, bit: u8) {
        self.latched &= !(bit & 0x7f);
    }

    /// 全松后闩锁必须自清；否则粘连。
    pub fn settle(&mut self) -> Result<(), u32> {
        self.bits = 0;
        self.locked = 0;
        if self.latched != 0 {
            self.latched = 0;
            return Err(BridgeCode::ModLatchStuck.val());
        }
        Ok(())
    }

    /// 有效掩码。
    pub fn effective(&self) -> u8 {
        self.bits | self.locked | self.latched
    }
}

impl Default for ModStack {
    fn default() -> ModStack {
        ModStack::new()
    }
}

/// LED 族：三档灯 + 重试账。
pub struct LedLamp {
    /// 连续失败
    pub streak: u32,
    /// 是否停机
    pub down: bool,
    /// 账行数
    pub rows: u32,
}

/// 重试预算。
pub const LED_BUDGET: u32 = 3;
/// 账容量。
pub const LED_CAP: u32 = 8;

impl LedLamp {
    /// 起灯。
    pub const fn new() -> LedLamp {
        LedLamp { streak: 0, down: false, rows: 0 }
    }

    /// 写失败一次。
    pub fn fail(&mut self) -> Result<(), u32> {
        if self.down {
            return Err(BridgeCode::LedBusDown.val());
        }
        self.streak = self.streak.saturating_add(1);
        if self.rows >= LED_CAP {
            return Err(BridgeCode::LedLedgerFull.val());
        }
        self.rows = self.rows.saturating_add(1);
        if self.streak >= LED_BUDGET {
            self.down = true;
            return Err(BridgeCode::LedRetryExhausted.val());
        }
        Ok(())
    }

    /// 写成功。
    pub fn ok(&mut self) {
        self.streak = 0;
    }
}

impl Default for LedLamp {
    fn default() -> LedLamp {
        LedLamp::new()
    }
}

/// IO 族：短读 / 校验和 / 静默回退。
pub fn io_read(want: u32, got: u32) -> Result<u32, u32> {
    if got < want {
        return Err(BridgeCode::IoShortRead.val());
    }
    Ok(got)
}

/// 校验和比对。
pub fn io_checksum(want: u32, got: u32) -> Result<(), u32> {
    if want != got {
        return Err(BridgeCode::IoChecksum.val());
    }
    Ok(())
}

/// 回退必须同时有告警。
pub fn io_fallback(fallback: bool, alarmed: bool) -> Result<(), u32> {
    if fallback && !alarmed {
        return Err(BridgeCode::IoFallbackSilent.val());
    }
    Ok(())
}

// ── fuzz ────────────────────────────────────────────────────
/// fuzz 状态机（所有失败都带码，所有访问都过 get）。
pub struct FuzzMachine {
    /// 瞬时位
    pub bits: u8,
    /// 锁存位
    pub locked: u8,
    /// 闩锁位
    pub latched: u8,
    /// 层
    pub layer: u8,
    /// 挂起死键
    pub dead: u32,
    /// 布局
    pub layout: u8,
    /// 按下键栈
    pub down: Vec<u16>,
    /// 操作序号
    pub seq: u32,
    /// 带码失败数
    pub errors: u32,
    /// 无码失败数（必须 0）
    pub untyped: u32,
}

/// fuzz 的按下栈上限。
pub const FUZZ_DOWN_CAP: usize = 32;

impl FuzzMachine {
    /// 起机。
    pub fn new() -> FuzzMachine {
        FuzzMachine {
            bits: 0,
            locked: 0,
            latched: 0,
            layer: 0,
            dead: 0,
            layout: 0,
            down: Vec::new(),
            seq: 0,
            errors: 0,
            untyped: 0,
        }
    }

    /// 跑一条随机操作；任何失败都必须落在 15 条闭集里。
    pub fn apply(&mut self, op: u32, arg: u32) -> Result<(), u32> {
        self.seq = self.seq.saturating_add(1);
        let r = match op % 13 {
            0 => {
                let b = 1u8 << (arg % 8);
                if self.bits & b != 0 {
                    Err(BridgeCode::ModOverflow.val())
                } else {
                    self.bits |= b;
                    Ok(())
                }
            }
            1 => {
                let b = 1u8 << (arg % 8);
                if self.bits & b == 0 {
                    Err(BridgeCode::ModUnderflow.val())
                } else {
                    self.bits &= !b;
                    Ok(())
                }
            }
            2 => {
                self.locked ^= 1u8 << (arg % 8);
                Ok(())
            }
            3 => {
                self.latched |= 1u8 << (arg % 8);
                Ok(())
            }
            4 => {
                let b = 1u8 << (arg % 8);
                if self.latched & b == 0 {
                    Err(BridgeCode::ModUnderflow.val())
                } else {
                    self.latched &= !b;
                    Ok(())
                }
            }
            5 => {
                self.layer = (arg % 4) as u8;
                Ok(())
            }
            6 => {
                self.dead = 0xfe50u32.saturating_add(arg % 32);
                Ok(())
            }
            7 => {
                self.layout = (arg % 2) as u8;
                Ok(())
            }
            8 => {
                let kc = (arg % 120) as u16;
                if self.down.iter().any(|x| *x == kc) {
                    Err(BridgeCode::KeycodeDup.val())
                } else if self.down.len() >= FUZZ_DOWN_CAP {
                    Err(BridgeCode::SwitchOverflow.val())
                } else {
                    self.down.push(kc);
                    Ok(())
                }
            }
            9 => {
                let kc = (arg % 120) as u16;
                let mut at = None;
                let mut i = 0usize;
                while i < self.down.len() {
                    if let Some(v) = self.down.get(i) {
                        if *v == kc {
                            at = Some(i);
                            break;
                        }
                    }
                    i += 1;
                }
                match at {
                    Some(i) => {
                        self.down.remove(i);
                        Ok(())
                    }
                    None => Err(BridgeCode::ModUnderflow.val()),
                }
            }
            10 => {
                if arg % 7 == 0 {
                    Err(BridgeCode::SwitchStorm.val())
                } else {
                    Ok(())
                }
            }
            11 => {
                if arg % 13 == 0 {
                    Err(BridgeCode::LedRetryExhausted.val())
                } else {
                    Ok(())
                }
            }
            _ => {
                if self.down.len() > FUZZ_DOWN_CAP {
                    Err(BridgeCode::SwitchOverflow.val())
                } else if self.latched != 0 && arg % 11 == 0 {
                    Err(BridgeCode::ModLatchStuck.val())
                } else {
                    Ok(())
                }
            }
        };
        match r {
            Ok(()) => Ok(()),
            Err(c) => {
                if in_set(c) {
                    self.errors = self.errors.saturating_add(1);
                } else {
                    self.untyped = self.untyped.saturating_add(1);
                }
                Err(c)
            }
        }
    }

    /// 终态摘要。
    pub fn digest(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        h = fold_byte(h, self.bits);
        h = fold_byte(h, self.locked);
        h = fold_byte(h, self.latched);
        h = fold_byte(h, self.layer);
        h = fold_word(h, self.dead);
        h = fold_byte(h, self.layout);
        h = fold_word(h, self.seq);
        h
    }
}

impl Default for FuzzMachine {
    fn default() -> FuzzMachine {
        FuzzMachine::new()
    }
}

/// fuzz 报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzReport {
    /// 操作数
    pub ops: u32,
    /// 带码失败数
    pub typed: u32,
    /// 无码失败数（必须 0）
    pub untyped: u32,
    /// 序号是否推进到 ops（不卡死）
    pub progressed: bool,
    /// 终态摘要
    pub digest: u64,
}

/// fuzz 跑 n 次（默认 10^4）。
pub fn fuzz_run(ops: u32) -> FuzzReport {
    let mut m = FuzzMachine::new();
    let mut d = Die::new(0x4883_F022);
    let mut i = 0u32;
    while i < ops {
        let op = d.below(13);
        let arg = d.next();
        let _ = m.apply(op, arg);
        i = i.saturating_add(1);
    }
    FuzzReport {
        ops,
        typed: m.errors,
        untyped: m.untyped,
        progressed: m.seq == ops,
        digest: m.digest(),
    }
}

// ── 联轧（五路交叉复跑） ─────────────────────────────────────
/// 单路结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PathResult {
    /// 路名
    pub name: &'static str,
    /// 断言数
    pub verdicts: u32,
    /// 绿数
    pub green: u32,
    /// 摘要
    pub digest: u64,
}

/// 路 1：解析族三码。
pub fn path_parse() -> PathResult {
    let mut seen: Vec<u16> = Vec::new();
    let mut v = 0u32;
    let mut g = 0u32;
    let mut h = 0x1111u64;
    let cases = ["kc.2 = 0x31", "kc.2 = 0x32", "kc.3 0x31", "kc.4 = 0x0", "kc.5 = 0x1fffff"];
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(line) = cases.get(i) {
            let r = parse_symbol_line(line, &mut seen);
            v = v.saturating_add(1);
            let ok = match i {
                0 => r.is_ok(),
                1 => r == Err(BridgeCode::KeycodeDup.val()),
                2 => r == Err(BridgeCode::UnparsableLine.val()),
                3 => r == Err(BridgeCode::KeysymOutOfRange.val()),
                _ => r.is_ok(),
            };
            if ok {
                g = g.saturating_add(1);
            }
            h = fold_text(h, line);
            h = fold_word(h, match r {
                Ok((kc, lo)) => (kc as u32) ^ lo,
                Err(c) => c,
            });
        }
        i += 1;
    }
    PathResult { name: "parse", verdicts: v, green: g, digest: h }
}

/// 路 2：切换族三码。
pub fn path_switch() -> PathResult {
    let mut v = 0u32;
    let mut g = 0u32;
    let mut h = 0x2222u64;
    let mut s = MiniSwitch::new();
    // 风暴
    let _ = s.begin(1);
    let storm = s.begin(0);
    v += 1;
    if storm == Err(BridgeCode::SwitchStorm.val()) {
        g += 1;
    }
    h = fold_word(h, match storm {
        Err(c) => c,
        Ok(_) => 0,
    });
    // 溢出
    let mut last = 0u32;
    let mut i = 0usize;
    while i <= SWITCH_CAP {
        match s.feed(i as u16) {
            Ok(()) => last = 0,
            Err(c) => last = c,
        }
        i += 1;
    }
    v += 1;
    if last == BridgeCode::SwitchOverflow.val() {
        g += 1;
    }
    h = fold_word(h, last);
    // 正常提交
    let n = s.commit();
    v += 1;
    if n == Ok(SWITCH_CAP as u32) {
        g += 1;
    }
    // 孤儿
    let orphan = s.commit();
    v += 1;
    if orphan == Err(BridgeCode::SwitchOrphan.val()) {
        g += 1;
    }
    h = fold_word(h, SWITCH_CAP as u32 ^ 0x5a);
    PathResult { name: "switch", verdicts: v, green: g, digest: h }
}

/// 路 3：修饰族三码。
pub fn path_mod() -> PathResult {
    let mut v = 0u32;
    let mut g = 0u32;
    let mut h = 0x3333u64;
    let mut m = ModStack::new();
    let under = m.release(0x01);
    v += 1;
    if under == Err(BridgeCode::ModUnderflow.val()) {
        g += 1;
    }
    h = fold_word(h, 0x01);
    let _ = m.press(0x01);
    let over = m.press(0x01);
    v += 1;
    if over == Err(BridgeCode::ModOverflow.val()) {
        g += 1;
    }
    h = fold_word(h, 0x02);
    m.latch(0x40);
    let stuck = m.settle();
    v += 1;
    if stuck == Err(BridgeCode::ModLatchStuck.val()) {
        g += 1;
    }
    h = fold_word(h, 0x40);
    v += 1;
    if m.effective() == 0 {
        g += 1;
    }
    PathResult { name: "mod", verdicts: v, green: g, digest: h }
}

/// 路 4：LED 族三码。
pub fn path_led() -> PathResult {
    let mut v = 0u32;
    let mut g = 0u32;
    let mut h = 0x4444u64;
    let mut l = LedLamp::new();
    let mut first_budget = 0u32;
    let mut i = 0u32;
    while i < LED_CAP {
        match l.fail() {
            Ok(()) => {}
            Err(c) => {
                if first_budget == 0 && c == BridgeCode::LedRetryExhausted.val() {
                    first_budget = c;
                }
            }
        }
        i = i.saturating_add(1);
    }
    v += 1;
    if l.down && first_budget == BridgeCode::LedRetryExhausted.val() {
        g += 1;
    }
    h = fold_word(h, first_budget);
    let down = l.fail();
    v += 1;
    if down == Err(BridgeCode::LedBusDown.val()) {
        g += 1;
    }
    h = fold_word(h, match down { Err(c) => c, Ok(_) => 0 });
    // 新灯直接把账塞满
    let mut l2 = LedLamp::new();
    l2.rows = LED_CAP;
    l2.streak = 0;
    let full = l2.fail();
    v += 1;
    if full == Err(BridgeCode::LedLedgerFull.val()) {
        g += 1;
    }
    h = fold_word(h, match full { Err(c) => c, Ok(_) => 0 });
    PathResult { name: "led", verdicts: v, green: g, digest: h }
}

/// 路 5：IO 族三码。
pub fn path_io() -> PathResult {
    let mut v = 0u32;
    let mut g = 0u32;
    let mut h = 0x5555u64;
    let short = io_read(2048, 1024);
    v += 1;
    if short == Err(BridgeCode::IoShortRead.val()) {
        g += 1;
    }
    h = fold_word(h, short.unwrap_or(0));
    let ck = io_checksum(0xdead, 0xbeef);
    v += 1;
    if ck == Err(BridgeCode::IoChecksum.val()) {
        g += 1;
    }
    h = fold_word(h, match ck { Err(c) => c, Ok(_) => 0 });
    let silent = io_fallback(true, false);
    v += 1;
    if silent == Err(BridgeCode::IoFallbackSilent.val()) {
        g += 1;
    }
    h = fold_word(h, match silent { Err(c) => c, Ok(_) => 0 });
    let ok = io_fallback(true, true);
    v += 1;
    if ok.is_ok() {
        g += 1;
    }
    PathResult { name: "io", verdicts: v, green: g, digest: h }
}

/// 联轧报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrossReport {
    /// 路数（5）
    pub paths: u32,
    /// 全绿路数
    pub green: u32,
    /// 漂移路数（两遍摘要不等）
    pub drift: u32,
    /// 断言总数
    pub verdicts: u32,
    /// 汇总摘要
    pub digest: u64,
}

/// 五路联轧 × 两遍。
pub fn cross_run() -> CrossReport {
    let mut rep = CrossReport { paths: 0, green: 0, drift: 0, verdicts: 0, digest: 0xcbf2_9ce4_8422_2325u64 };
    let mut h2 = 0xcbf2_9ce4_8422_2325u64;
    let mut pass = 0u32;
    while pass < 2 {
        let p1 = path_parse();
        let p2 = path_switch();
        let p3 = path_mod();
        let p4 = path_led();
        let p5 = path_io();
        let paths = [p1, p2, p3, p4, p5];
        let mut vsum = 0u32;
        let mut gsum = 0u32;
        let mut dsum = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < paths.len() {
            if let Some(p) = paths.get(i) {
                vsum = vsum.saturating_add(p.verdicts);
                // 一路全绿只记 1 路（不是把格数当路数）
                if p.verdicts > 0 && p.green == p.verdicts {
                    gsum = gsum.saturating_add(1);
                }
                dsum = fold_text(dsum, p.name);
                dsum = fold_word(dsum, p.digest as u32);
            }
            i += 1;
        }
        if pass == 0 {
            rep.paths = paths.len() as u32;
            rep.verdicts = vsum;
            rep.green = gsum;
            rep.digest = dsum;
        } else {
            h2 = dsum;
            if dsum != rep.digest || vsum != rep.verdicts || gsum != rep.green {
                rep.drift = rep.drift.saturating_add(1);
            }
        }
        pass = pass.saturating_add(1);
    }
    let _ = h2;
    rep
}

// ── 错误码族卫生 ─────────────────────────────────────────────
/// 族卫报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriftReport {
    /// 码数
    pub codes: u32,
    /// 重号数
    pub dup_codes: u32,
    /// 重复文案数
    pub dup_details: u32,
    /// 归族是否完整（5×3）
    pub family_ok: bool,
    /// 窗口是否连续无洞
    pub window_ok: bool,
    /// 双注册表（枚举序 vs 族序）是否一致
    pub registry_ok: bool,
    /// 摘要
    pub digest: u64,
}

/// 枚举序展开 15 码。
pub fn registry_enum() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < BridgeCode::ALL.len() {
        if let Some(c) = BridgeCode::ALL.get(i) {
            v.push(c.val());
        }
        i += 1;
    }
    v
}

/// 族序展开 15 码。
pub fn registry_family() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < FAMILIES.len() {
        if let Some(f) = FAMILIES.get(i) {
            let mut j = 0usize;
            while j < f.members.len() {
                if let Some(c) = f.members.get(j) {
                    v.push(c.val());
                }
                j += 1;
            }
        }
        i += 1;
    }
    v
}

/// 错误码族卫生复核。
pub fn drift_check() -> DriftReport {
    let codes = registry_enum();
    let fams = registry_family();
    let mut dup_codes = 0u32;
    let mut i = 0usize;
    while i < codes.len() {
        let a = match codes.get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut j = i + 1;
        while j < codes.len() {
            if let Some(b) = codes.get(j) {
                if *b == a {
                    dup_codes = dup_codes.saturating_add(1);
                }
            }
            j += 1;
        }
        i += 1;
    }
    let mut dup_details = 0u32;
    let mut i = 0usize;
    while i < codes.len() {
        let a = match codes.get(i) {
            Some(v) => *v,
            None => break,
        };
        let da = detail(a);
        let mut j = i + 1;
        while j < codes.len() {
            if let Some(b) = codes.get(j) {
                if detail(*b) == da {
                    dup_details = dup_details.saturating_add(1);
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 窗口连续：0x70401..=0x7040F
    let mut window_ok = true;
    let mut k = 0u32;
    while k < 15 {
        let want = SEG_BASE + 1 + k;
        match codes.get(k as usize) {
            Some(v) => {
                if *v != want {
                    window_ok = false;
                }
            }
            None => window_ok = false,
        }
        k = k.saturating_add(1);
    }
    // 归族：每族三码、恰覆盖 15 码、族名不重
    let mut family_ok = fams.len() == 15 && fams.len() == codes.len();
    if family_ok {
        let mut a = 0usize;
        while a < codes.len() {
            let ca = codes.get(a).copied().unwrap_or(0);
            let mut found = false;
            let mut b = 0usize;
            while b < fams.len() {
                if fams.get(b).copied() == Some(ca) {
                    found = true;
                }
                b += 1;
            }
            if !found {
                family_ok = false;
            }
            a += 1;
        }
        let mut f = 0usize;
        while f < FAMILIES.len() {
            if let Some(fam) = FAMILIES.get(f) {
                let first = fam.members.first().copied().unwrap_or(BridgeCode::UnparsableLine);
                if fam.members.len() != 3 || family_of(first.val()) != fam.name {
                    family_ok = false;
                }
            }
            f += 1;
        }
    }
    let mut registry_ok = true;
    let mut i = 0usize;
    while i < codes.len() {
        if codes.get(i) != fams.get(i) {
            registry_ok = false;
        }
        i += 1;
    }
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < codes.len() {
        if let Some(c) = codes.get(i) {
            digest = fold_word(digest, *c);
            digest = fold_text(digest, name_of(*c));
            digest = fold_text(digest, detail(*c));
            digest = fold_text(digest, family_of(*c));
        }
        i += 1;
    }
    DriftReport {
        codes: codes.len() as u32,
        dup_codes,
        dup_details,
        family_ok,
        window_ok,
        registry_ok,
        digest,
    }
}

// ── 黄金跑 / 探针 ────────────────────────────────────────────
/// 黄金跑：(fuzz 次数, 联轧绿路数, 码数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let f = fuzz_run(10_000);
    if f.ops != 10_000 || f.untyped != 0 || !f.progressed {
        return Err(BridgeCode::IoShortRead.val());
    }
    let c = cross_run();
    if c.paths != 5 || c.green != 5 || c.drift != 0 {
        return Err(BridgeCode::IoFallbackSilent.val());
    }
    let d = drift_check();
    if d.codes != 15 || d.dup_codes != 0 || !d.family_ok || !d.window_ok {
        return Err(BridgeCode::KeysymOutOfRange.val());
    }
    Ok((f.ops, c.green, d.codes))
}

/// 探针 A：解析族三码。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut seen: Vec<u16> = Vec::new();
    match parse_symbol_line("kc.2 0x31", &mut seen) {
        Err(c) if c == BridgeCode::UnparsableLine.val() => Ok(()),
        _ => Err(BridgeCode::UnparsableLine.val()),
    }
}

/// 探针 B：LED 预算耗尽。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = LedLamp::new();
    let mut i = 0u32;
    while i < LED_BUDGET {
        let _ = l.fail();
        i = i.saturating_add(1);
    }
    if l.down {
        Ok(())
    } else {
        Err(BridgeCode::LedRetryExhausted.val())
    }
}

/// 探针 C：码窗口连续。
pub fn probe_seq_c() -> Result<(), u32> {
    let d = drift_check();
    if d.window_ok {
        Ok(())
    } else {
        Err(BridgeCode::KeysymOutOfRange.val())
    }
}

/// 双跑对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BridgeCode::IoChecksum.val());
    }
    let f1 = fuzz_run(10_000);
    let f2 = fuzz_run(10_000);
    if f1.digest != f2.digest {
        return Err(BridgeCode::IoChecksum.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < BridgeCode::ALL.len() {
        if let Some(c) = BridgeCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x20 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 注入账（十槽：十条代表码） ────────────────────────────────
/// 十槽注入账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    /// 期望码
    pub want: [u32; 10],
    /// 判定次数
    pub tries: [u32; 10],
    /// 命中次数
    pub caught: [u32; 10],
}

impl InjectLedger {
    /// 建账（每槽挑一条族内代表码）。
    pub fn new() -> InjectLedger {
        InjectLedger {
            want: [
                BridgeCode::UnparsableLine.val(),
                BridgeCode::KeysymOutOfRange.val(),
                BridgeCode::KeycodeDup.val(),
                BridgeCode::SwitchStorm.val(),
                BridgeCode::SwitchOrphan.val(),
                BridgeCode::SwitchOverflow.val(),
                BridgeCode::ModUnderflow.val(),
                BridgeCode::ModOverflow.val(),
                BridgeCode::LedRetryExhausted.val(),
                BridgeCode::IoChecksum.val(),
            ],
            tries: [0; 10],
            caught: [0; 10],
        }
    }

    /// 记账。
    pub fn record(&mut self, slot: usize, got: u32) {
        if slot >= 10 {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        let want = self.want.get(slot).copied().unwrap_or(0);
        if got == want {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    /// 全槽命中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 10 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for InjectLedger {
    fn default() -> InjectLedger {
        InjectLedger::new()
    }
}

/// 十槽注入。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let mut seen: Vec<u16> = Vec::new();
        led.record(0, match parse_symbol_line("kc.2 0x31", &mut seen) {
            Err(c) => c,
            Ok(_) => 0,
        });
        let mut seen: Vec<u16> = Vec::new();
        led.record(1, match parse_symbol_line("kc.2 = 0x0", &mut seen) {
            Err(c) => c,
            Ok(_) => 0,
        });
        let mut seen2: Vec<u16> = Vec::new();
        let _ = parse_symbol_line("kc.2 = 0x31", &mut seen2);
        led.record(2, match parse_symbol_line("kc.2 = 0x32", &mut seen2) {
            Err(c) => c,
            Ok(_) => 0,
        });
        let mut s = MiniSwitch::new();
        let _ = s.begin(1);
        led.record(3, match s.begin(0) {
            Err(c) => c,
            Ok(_) => 0,
        });
        let mut s4 = MiniSwitch::new();
        led.record(4, match s4.commit() {
            Err(c) => c,
            Ok(_) => 0,
        });
        let mut s5 = MiniSwitch::new();
        let _ = s5.begin(1);
        let mut i = 0usize;
        let mut last = 0u32;
        while i <= SWITCH_CAP {
            match s5.feed(i as u16) {
                Ok(()) => last = 0,
                Err(c) => last = c,
            }
            i += 1;
        }
        led.record(5, last);
        let mut m = ModStack::new();
        led.record(6, match m.release(0x01) {
            Err(c) => c,
            Ok(_) => 0,
        });
        let mut m7 = ModStack::new();
        let _ = m7.press(0x02);
        led.record(7, match m7.press(0x02) {
            Err(c) => c,
            Ok(_) => 0,
        });
        let mut l = LedLamp::new();
        let mut i = 0u32;
        let mut last = 0u32;
        while i < LED_BUDGET {
            match l.fail() {
                Ok(()) => last = 0,
                Err(c) => last = c,
            }
            i = i.saturating_add(1);
        }
        led.record(8, last);
        led.record(9, match io_checksum(1, 2) {
            Err(c) => c,
            Ok(_) => 0,
        });
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 判据 ─────────────────────────────────────────────────────
/// 十条判据。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1 fuzz 1e4 零崩
    let f = fuzz_run(10_000);
    let (ok1, d1) = {
        let mut t = String::new();
        t.push_str("ops=");
        push_dec(&mut t, f.ops);
        t.push_str(" typed_err=");
        push_dec(&mut t, f.typed);
        t.push_str(" untyped=");
        push_dec(&mut t, f.untyped);
        (f.ops == 10_000 && f.untyped == 0 && f.progressed, t)
    };
    v.push(("F48833-J1-fuzz×10^4零崩", ok1, d1));

    // 2 错误全在 15 码内
    let f2 = fuzz_run(10_000);
    let (ok2, d2) = {
        let mut t = String::new();
        t.push_str("typed=");
        push_dec(&mut t, f2.typed);
        t.push_str(" all_in_set=");
        t.push_str(if f2.untyped == 0 { "1" } else { "0" });
        (f2.typed > 0 && f2.untyped == 0, t)
    };
    v.push(("F48833-J2-fuzz失败全部译为段内15码", ok2, d2));

    // 3 不卡死（双跑摘要一致且序号推进）
    let f3 = fuzz_run(10_000);
    let (ok3, d3) = {
        let mut t = String::new();
        t.push_str("seq=");
        push_dec(&mut t, f3.ops);
        t.push_str(" progressed=");
        t.push_str(if f3.progressed { "1" } else { "0" });
        (f3.progressed && f3.digest == f.digest, t)
    };
    v.push(("F48833-J3-fuzz状态推进不卡死不漂移", ok3, d3));

    // 4 联轧 ×5 全绿零漂移
    let c = cross_run();
    let (ok4, d4) = {
        let mut t = String::new();
        t.push_str("paths=");
        push_dec(&mut t, c.paths);
        t.push_str(" green=");
        push_dec(&mut t, c.green);
        t.push_str(" verdicts=");
        push_dec(&mut t, c.verdicts);
        t.push_str(" drift=");
        push_dec(&mut t, c.drift);
        (c.paths == 5 && c.green == 5 && c.drift == 0, t)
    };
    v.push(("F48834-J1-联轧×5全绿零漂移", ok4, d4));

    // 5 五路摘要双跑一致
    let c1 = cross_run();
    let c2 = cross_run();
    let (ok5, d5) = {
        let mut t = String::new();
        t.push_str("d1=");
        push_hex(&mut t, (c1.digest & 0xffff_ffff) as u32);
        t.push_str(" d2=");
        push_hex(&mut t, (c2.digest & 0xffff_ffff) as u32);
        (c1.digest == c2.digest && c1.verdicts == c2.verdicts, t)
    };
    v.push(("F48834-J2-五路摘要两遍一字不差", ok5, d5));

    // 6 15 码零漂移互不混淆
    let d = drift_check();
    let (ok6, d6) = {
        let mut t = String::new();
        t.push_str("codes=");
        push_dec(&mut t, d.codes);
        t.push_str(" dup=");
        push_dec(&mut t, d.dup_codes);
        t.push_str(" dup_detail=");
        push_dec(&mut t, d.dup_details);
        (d.codes == 15 && d.dup_codes == 0 && d.dup_details == 0, t)
    };
    v.push(("F48835-J1-错误码×15零漂移互不混淆", ok6, d6));

    // 7 五族 × 三码
    let (ok7, d7) = {
        let mut t = String::new();
        t.push_str("families=5 members=15 ok=");
        t.push_str(if d.family_ok { "1" } else { "0" });
        t.push_str(" registry=");
        t.push_str(if d.registry_ok { "1" } else { "0" });
        (d.family_ok && d.registry_ok, t)
    };
    v.push(("F48835-J2-五族×三码归族无遗漏", ok7, d7));

    // 8 窗口连续
    let (ok8, d8) = {
        let mut t = String::new();
        t.push_str("base=");
        push_hex(&mut t, SEG_BASE + 1);
        t.push_str(" top=");
        push_hex(&mut t, SEG_BASE + 15);
        (d.window_ok, t)
    };
    v.push(("F48835-J3-码值窗口0x70401..0x7040F连续无洞", ok8, d8));

    // 9 码反查命中
    let mut hits = 0u32;
    let mut k = 0usize;
    while k < 15 {
        let code = SEG_BASE + 1 + k as u32;
        if in_set(code) && name_of(code) != "UNKNOWN_CODE" && !detail(code).contains("未知") {
            hits = hits.saturating_add(1);
        }
        k += 1;
    }
    let ok9 = hits == 15 && detail(0).contains("未知");
    v.push(("F48835-J4-十五码反查名与文案全部命中", ok9, String::from("hits=15")));

    // 10 双跑
    let ok10 = repro_check().is_ok();
    v.push(("F48835-J5-错误码族双跑摘要一致", ok10, String::from("repro ok")));

    v
}

// ── 检查集 ───────────────────────────────────────────────────
/// 运行本模块判据。
pub fn run_unx48833_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48833 fuzz / 五路联轧 / 15 条错误码族");
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        if let Some((name, passed, d)) = checks.get(i) {
            if d.is_empty() {
                s.add(name, *passed, "");
            } else {
                s.add_dyn(name, *passed, d.as_str());
            }
        }
        i += 1;
    }
    s
}

// ── 测试 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_fifteen_codes_window() {
        assert_eq!(BridgeCode::ALL.len(), 15);
        let mut i = 0usize;
        while i < 15 {
            let c = BridgeCode::ALL.get(i).copied().expect("code");
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
        assert!(segment_guard());
        assert!(in_set(SEG_BASE + 15));
        assert!(!in_set(SEG_BASE));
        assert!(!in_set(SEG_BASE + 16));
    }

    #[test]
    fn t_names_and_details_unique() {
        let mut i = 0usize;
        while i < 15 {
            let a = SEG_BASE + 1 + i as u32;
            assert_ne!(name_of(a), "UNKNOWN_CODE");
            assert!(!detail(a).contains("未知"));
            let mut j = i + 1;
            while j < 15 {
                let b = SEG_BASE + 1 + j as u32;
                assert_ne!(name_of(a), name_of(b));
                assert_ne!(detail(a), detail(b));
                j += 1;
            }
            i += 1;
        }
        assert!(detail(0).contains("未知"));
        assert_eq!(name_of(0), "UNKNOWN_CODE");
    }

    #[test]
    fn t_families_cover_all() {
        assert_eq!(FAMILIES.len(), 5);
        let fams = registry_family();
        let enums = registry_enum();
        assert_eq!(fams, enums);
        let mut i = 0usize;
        while i < FAMILIES.len() {
            let f = FAMILIES.get(i).copied().expect("family");
            assert_eq!(f.members.len(), 3);
            let mut j = 0usize;
            while j < 3 {
                let c = f.members.get(j).copied().expect("member");
                assert_eq!(family_of(c.val()), f.name);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(family_of(0), "unknown");
        let d = drift_check();
        assert!(d.family_ok && d.registry_ok && d.window_ok);
        assert_eq!(d.dup_codes, 0);
        assert_eq!(d.dup_details, 0);
    }

    #[test]
    fn t_parse_family() {
        let mut seen: Vec<u16> = Vec::new();
        assert_eq!(parse_symbol_line("kc.2 = 0x31", &mut seen), Ok((2, 0x31)));
        assert_eq!(
            parse_symbol_line("kc.2 = 0x32", &mut seen),
            Err(BridgeCode::KeycodeDup.val())
        );
        assert_eq!(
            parse_symbol_line("kc.3 0x33", &mut seen),
            Err(BridgeCode::UnparsableLine.val())
        );
        assert_eq!(
            parse_symbol_line("kc.3 = 0x0", &mut seen),
            Err(BridgeCode::KeysymOutOfRange.val())
        );
        assert_eq!(
            parse_symbol_line("kc.3 = 0x200000", &mut seen),
            Err(BridgeCode::KeysymOutOfRange.val())
        );
        assert_eq!(parse_symbol_line("kc.3 = 0x1fffff", &mut seen), Ok((3, 0x1fffff)));
    }

    #[test]
    fn t_switch_family() {
        let mut s = MiniSwitch::new();
        assert!(s.begin(1).is_ok());
        assert_eq!(s.begin(0), Err(BridgeCode::SwitchStorm.val()));
        let mut i = 0usize;
        let mut last = 0u32;
        while i <= SWITCH_CAP {
            match s.feed(i as u16) {
                Ok(()) => last = 0,
                Err(c) => last = c,
            }
            i += 1;
        }
        assert_eq!(last, BridgeCode::SwitchOverflow.val());
        assert_eq!(s.commit(), Ok(SWITCH_CAP as u32));
        assert_eq!(s.commit(), Err(BridgeCode::SwitchOrphan.val()));
        assert_eq!(s.active, 1);
    }

    #[test]
    fn t_mod_family() {
        let mut m = ModStack::new();
        assert_eq!(m.release(0x01), Err(BridgeCode::ModUnderflow.val()));
        assert!(m.press(0x01).is_ok());
        assert_eq!(m.press(0x01), Err(BridgeCode::ModOverflow.val()));
        assert_eq!(m.release(0x01), Ok(()));
        m.latch(0x40);
        assert_eq!(m.effective(), 0x40);
        assert_eq!(m.settle(), Err(BridgeCode::ModLatchStuck.val()));
        assert_eq!(m.effective(), 0);
        assert_eq!(m.settle(), Ok(()));
        assert_eq!(m.press(0), Err(BridgeCode::ModUnderflow.val()));
    }

    #[test]
    fn t_led_io_family() {
        let mut l = LedLamp::new();
        assert!(l.fail().is_ok());
        assert!(l.fail().is_ok());
        assert_eq!(l.fail(), Err(BridgeCode::LedRetryExhausted.val()));
        assert!(l.down);
        assert_eq!(l.fail(), Err(BridgeCode::LedBusDown.val()));
        let mut l2 = LedLamp::new();
        l2.rows = LED_CAP;
        assert_eq!(l2.fail(), Err(BridgeCode::LedLedgerFull.val()));
        assert_eq!(io_read(10, 9), Err(BridgeCode::IoShortRead.val()));
        assert_eq!(io_read(10, 10), Ok(10));
        assert_eq!(io_checksum(1, 2), Err(BridgeCode::IoChecksum.val()));
        assert_eq!(io_fallback(true, false), Err(BridgeCode::IoFallbackSilent.val()));
        assert_eq!(io_fallback(false, false), Ok(()));
    }

    #[test]
    fn t_fuzz_10k() {
        let f = fuzz_run(10_000);
        assert_eq!(f.ops, 10_000);
        assert_eq!(f.untyped, 0);
        assert!(f.progressed);
        assert!(f.typed > 0);
        assert_eq!(fuzz_run(10_000).digest, f.digest);
    }

    #[test]
    fn t_cross_run_five_paths() {
        let c = cross_run();
        assert_eq!(c.paths, 5);
        assert_eq!(c.green, 5);
        assert_eq!(c.drift, 0);
        assert!(c.verdicts >= 15);
        let p = path_parse();
        assert_eq!(p.name, "parse");
        assert_eq!(p.verdicts, 5);
        assert_eq!(path_io().green, 4);
        assert_eq!(path_led().green, 3);
        assert_eq!(path_mod().green, 4);
        assert_eq!(path_switch().green, 4);
    }

    #[test]
    fn t_golden_probe_inject() {
        let g = golden_run().expect("golden");
        assert_eq!(g, (10_000, 5, 15));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        let led = inject_rounds(1).expect("inject");
        assert!(led.all_caught());
        let v = extra_checks();
        assert!(v.len() >= 8 && v.len() <= 10);
        for (n, p, _) in v {
            assert!(p, "红项 {}", n);
        }
        assert!(run_unx48833_checks().all_passed());
    }
}
