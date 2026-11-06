//! UNX-F49027 修饰键组合穷举抽样 + UNX-F49028 组合键生成序 · 0x71C00
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第四槽；任务板 J1 引用按原文保留）：
//!   UNX-F49027 修饰键组合穷举抽样（500 次随机组合终态一致——J-4 组成项复跑）:
//!          ① 纯功能代码 ≥320 行；
//!          ② 判据原文：UNX-F49028-J1 联签锚定行 + 序一致 ×10^3。
//!   UNX-F49028 组合键生成序（key_event 递交序与 D4 消费序一致联签）:
//!          ① 纯功能代码 ≥320 行；
//!          ② 判据原文：UNX-F49029-J1 边界声明落账 + 越界注入 ×10 零越权。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（修饰键抽样）：码表 → 解析 → 锁存 → 释放 → 终态。
//! 码表先于解析：修饰位必须先是 2 的幂且互不相交，否则 "held | extra"
//! 会把两个不同修饰揉成同一个位，抽样再多次也只是同一个错误重复 500 次。
//! 解析先于锁存：一次组合键必须先得到一个确定的解析结果（字母/数字/原样），
//! 才允许计入锁存。解析失败仍计入锁存，会让 "组合数" 虚高。
//! 锁存先于释放：按下的修饰位必须真的进入 held，释放才可验证。
//! 释放先于终态：每轮结束时 held 必须归零。终态不干净意味着修饰键卡住，
//! 下一轮的抽样就会带着上一轮的残留，所谓 "500 次独立组合" 不再成立。
//! 两条独立路径（算术快路径 + 表驱动慢路径）必须给出同一个 code：
//! 只让实现和自己对齐，等于没有对齐。
//! 闸门次序（生成序）：边界 → 递交 → 入队 → 消费 → 联签。
//! 边界先于递交：key 与 mods 的合法域必须先声明成可解析的行，再做校验。
//! 越界值绝不能进入队列——进了队列，消费端拿到的就是非法事件，
//! "零越权" 只能是事后统计，而这里要求事前拒绝。
//! 递交先于入队：递交产生单调递增的 seq，seq 是联签的唯一锚。
//! 入队先于消费：FIFO 队列保证递交序即消费序。任何乱序都会在 seq
//! 连续性上留下痕迹，因此 order_viol 是可复核的计数，而不是口头保证。
//! 消费先于联签：递交摘要与消费摘要必须在同一条事件序列上折出同一个值；
//! 两个摘要相等，才允许写联签行。联签行本身要能被 audit 原样复现。
//! 关键差异：本模块不处理 dead key（B12 第二槽），不碰 LED 灯态（B12
//! 第三槽），也不负责中文布局（B12 后续槽位）。本槽只回答两个问题：
//! 修饰键在 500 次随机组合后终态是否干净；事件从递交到消费的序是否一致。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71C00，B12 族第四槽）。
pub const SEG_BASE: u32 = 0x71C00;

/// 修饰位掩码（Shift/Ctrl/Alt/Super 四位）。
pub const MOD_MASK: u8 = 0x0F;
/// Shift 位。
pub const MOD_SHIFT: u8 = 0x01;
/// Ctrl 位。
pub const MOD_CTRL: u8 = 0x02;
/// Alt 位。
pub const MOD_ALT: u8 = 0x04;
/// Super（Win）位。
pub const MOD_SUPER: u8 = 0x08;
/// 修饰键数量。
pub const MOD_COUNT: usize = 4;

/// HID usage 下界（'a'）。
pub const KEY_MIN: u8 = 0x04;
/// HID usage 上界。
pub const KEY_MAX: u8 = 0xE7;

/// 抽样轮数（判据 ×500）。
pub const SAMPLE_ROUNDS: u32 = 500;
/// 抽样种子（固定，保证终态一致可复跑）。
pub const SAMPLE_SEED: u64 = 0x0000_0000_4902_7001;
/// 分块抽样块大小（用于分块不变性）。
pub const SAMPLE_CHUNK: u32 = 50;

/// 生成序轮数（判据 ×10^3）。
pub const ORDER_ROUNDS: u32 = 1000;
/// 生成序种子。
pub const ORDER_SEED: u64 = 0x0000_0000_4902_8001;
/// 每轮最多递交事件数。
pub const ORDER_BURST: u32 = 7;

/// 越界注入次数（判据 ×10）。
pub const BOUND_INJECT: u32 = 10;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 边界声明标签。
pub const BOUND_TAG: &str = "BOUND";
/// 联签标签（判据原文 UNX-F49028-J1）。
pub const SIGN_TAG: &str = "UNX-F49028-J1";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MkCode {
    /// 修饰位非法（越出 0x0F）
    ModBad,
    /// key 越出 [0x04,0xE7]
    KeyBad,
    /// 码表查询失败
    TableBad,
    /// 锁存状态不一致（释放未按下的修饰）
    LatchBad,
    /// 抽样轮数越界
    SampleBad,
    /// 抽样复跑不一致
    SampleReplayBad,
    /// 生成序不一致
    OrderBad,
    /// 联签行不可复现
    SignBad,
    /// 边界声明不可解析或越界值被接收
    BoundBad,
    /// 序号或计数溢出
    Overflow,
    /// 渲染字段不可解析
    RenderBad,
    /// 段位越窗
    SegmentBad,
    /// fuzz 断言失败
    FuzzBad,
    /// 覆盖轮数越界
    CoverBad,
}

impl MkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            MkCode::ModBad => "修饰位越出 0x0F 掩码",
            MkCode::KeyBad => "key 越出 [0x04,0xE7]",
            MkCode::TableBad => "码表查询不到对应项",
            MkCode::LatchBad => "释放了未按下的修饰位",
            MkCode::SampleBad => "抽样轮数为零或越界",
            MkCode::SampleReplayBad => "抽样复跑终态不一致",
            MkCode::OrderBad => "递交序与消费序不一致",
            MkCode::SignBad => "联签锚定行不可原样复现",
            MkCode::BoundBad => "边界声明不可解析或越界值被接收",
            MkCode::Overflow => "序号或计数溢出",
            MkCode::RenderBad => "渲染字段不可解析",
            MkCode::SegmentBad => "诊断码越出段位窗口",
            MkCode::FuzzBad => "fuzz 断言失败",
            MkCode::CoverBad => "覆盖轮数越界",
        }
    }

    pub const ALL: [MkCode; 14] = [
        MkCode::ModBad,
        MkCode::KeyBad,
        MkCode::TableBad,
        MkCode::LatchBad,
        MkCode::SampleBad,
        MkCode::SampleReplayBad,
        MkCode::OrderBad,
        MkCode::SignBad,
        MkCode::BoundBad,
        MkCode::Overflow,
        MkCode::RenderBad,
        MkCode::SegmentBad,
        MkCode::FuzzBad,
        MkCode::CoverBad,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < MkCode::ALL.len() {
        if let Some(c) = MkCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知键盘组合诊断码"
}

// ── 修饰键码表 ────────────────────────────────────────────────────
/// 修饰键描述：位值、名称、声明序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Modifier {
    pub bit: u8,
    pub name: &'static str,
    pub order: u8,
}

pub const MODIFIERS: [Modifier; MOD_COUNT] = [
    Modifier { bit: MOD_SHIFT, name: "shift", order: 0 },
    Modifier { bit: MOD_CTRL, name: "ctrl", order: 1 },
    Modifier { bit: MOD_ALT, name: "alt", order: 2 },
    Modifier { bit: MOD_SUPER, name: "super", order: 3 },
];

/// 小写字母表（表驱动慢路径）。
pub const LETTERS: [u8; 26] = *b"abcdefghijklmnopqrstuvwxyz";
/// 大写字母表（Shift 后）。
pub const LETTERS_SHIFTED: [u8; 26] = *b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
/// 数字表（HID 0x1E..=0x27 对应 '1'..'0'）。
pub const DIGITS: [u8; 10] = *b"1234567890";
/// 数字 Shift 表。
pub const DIGITS_SHIFTED: [u8; 10] = [b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*', b'(', b')'];

/// 码表自洽：位值为 2 的幂、并集恰好等于掩码、声明序连续且不重复。
pub fn mod_table_ok() -> bool {
    let mut union: u8 = 0;
    let mut i = 0usize;
    while i < MODIFIERS.len() {
        let m = match MODIFIERS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if m.bit == 0 {
            return false;
        }
        if m.bit & (m.bit - 1) != 0 {
            return false;
        }
        if union & m.bit != 0 {
            return false;
        }
        if m.order != i as u8 {
            return false;
        }
        union |= m.bit;
        i += 1;
    }
    union == MOD_MASK
}

/// 数字段字母表长度必须与 HID 区间一致。
pub fn digit_table_ok() -> bool {
    let span = (0x27u8 - 0x1Eu8) as usize + 1;
    DIGITS.len() == span && DIGITS_SHIFTED.len() == span && LETTERS.len() == 26 && LETTERS_SHIFTED.len() == 26
}

// ── 解析（算术快路径） ────────────────────────────────────────────
/// 修饰组合解析结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    pub key: u8,
    pub mods: u8,
    pub code: u32,
}

/// 解析后的附加语义位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Resolved {
    pub chord: Chord,
    pub shifted: bool,
    pub altgr: bool,
    pub menu: bool,
}

/// 算术快路径：字母/数字/原样三种 payload。
pub fn payload(key: u8, shifted: bool) -> u8 {
    if key >= 0x04 && key <= 0x1D {
        let base = 0x61u8 + (key - 0x04);
        if shifted {
            base.wrapping_sub(0x20)
        } else {
            base
        }
    } else if key >= 0x1E && key <= 0x27 {
        let idx = (key - 0x1E) as usize;
        if shifted {
            match DIGITS_SHIFTED.get(idx) {
                Some(b) => *b,
                None => key,
            }
        } else if key == 0x27 {
            b'0'
        } else {
            0x31u8 + (key - 0x1E)
        }
    } else {
        key
    }
}

/// 表驱动慢路径（独立于算术快路径，用作终态比对）。
pub fn table_payload(key: u8, shifted: bool) -> Option<u8> {
    if key >= 0x04 && key <= 0x1D {
        let idx = (key - 0x04) as usize;
        if shifted {
            LETTERS_SHIFTED.get(idx).copied()
        } else {
            LETTERS.get(idx).copied()
        }
    } else if key >= 0x1E && key <= 0x27 {
        let idx = (key - 0x1E) as usize;
        if shifted {
            DIGITS_SHIFTED.get(idx).copied()
        } else {
            DIGITS.get(idx).copied()
        }
    } else {
        Some(key)
    }
}

/// code 编码：key | mods | payload 三段定界（便于联签与排障）。
pub fn encode(key: u8, mods: u8, payload: u8) -> u32 {
    ((key as u32) << 16) | ((mods as u32) << 8) | payload as u32
}

/// 解析一次组合键。越界修饰与越界 key 一律在入口拒绝。
pub fn resolve(key: u8, mods: u8) -> Result<Resolved, u32> {
    if mods & !MOD_MASK != 0 {
        return Err(MkCode::ModBad.val());
    }
    if key < KEY_MIN || key > KEY_MAX {
        return Err(MkCode::KeyBad.val());
    }
    let shifted = mods & MOD_SHIFT != 0;
    let altgr = mods & (MOD_CTRL | MOD_ALT) == (MOD_CTRL | MOD_ALT);
    let menu = mods & MOD_CTRL != 0 && !altgr;
    let p = payload(key, shifted);
    Ok(Resolved {
        chord: Chord { key, mods, code: encode(key, mods, p) },
        shifted,
        altgr,
        menu,
    })
}

/// 表驱动独立期望值（与 resolve 走不同实现）。
pub fn expected_code(key: u8, mods: u8) -> Result<u32, u32> {
    if mods & !MOD_MASK != 0 {
        return Err(MkCode::ModBad.val());
    }
    if key < KEY_MIN || key > KEY_MAX {
        return Err(MkCode::KeyBad.val());
    }
    let shifted = (mods & MOD_SHIFT) != 0;
    let p = match table_payload(key, shifted) {
        Some(v) => v,
        None => return Err(MkCode::TableBad.val()),
    };
    Ok(((key as u32) << 16) | ((mods as u32) << 8) | p as u32)
}

/// 两条路径在所有合法输入上是否恒等（穷举）。
pub fn paths_agree() -> bool {
    let mut key = KEY_MIN;
    loop {
        let mut mods = 0u8;
        while mods <= MOD_MASK {
            let a = resolve(key, mods);
            let b = expected_code(key, mods);
            match (a, b) {
                (Ok(ra), Ok(rb)) => {
                    if ra.chord.code != rb {
                        return false;
                    }
                    if ra.shifted != (mods & MOD_SHIFT != 0) {
                        return false;
                    }
                }
                _ => return false,
            }
            if mods == MOD_MASK {
                break;
            }
            mods += 1;
        }
        if key == KEY_MAX {
            break;
        }
        key += 1;
    }
    true
}

// ── 锁存状态机 ────────────────────────────────────────────────────
/// 修饰键锁存：held 必须始终是 MOD_MASK 的子集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Latch {
    pub held: u8,
    pub chords: u32,
    pub rejected: u32,
}

impl Latch {
    pub fn new() -> Latch {
        Latch { held: 0, chords: 0, rejected: 0 }
    }
}

/// 按下修饰位。
pub fn press_mods(l: &mut Latch, mods: u8) -> Result<(), u32> {
    if mods & !MOD_MASK != 0 {
        l.rejected = l.rejected.saturating_add(1);
        return Err(MkCode::ModBad.val());
    }
    l.held |= mods;
    Ok(())
}

/// 释放修饰位（必须已按下）。
pub fn release_mods(l: &mut Latch, mods: u8) -> Result<(), u32> {
    if mods & !MOD_MASK != 0 {
        l.rejected = l.rejected.saturating_add(1);
        return Err(MkCode::ModBad.val());
    }
    if mods & !l.held != 0 {
        l.rejected = l.rejected.saturating_add(1);
        return Err(MkCode::LatchBad.val());
    }
    l.held &= !mods;
    Ok(())
}

/// 在锁存态之上产生一次组合键（额外修饰不进入 held）。
pub fn chord_with(l: &mut Latch, key: u8, extra: u8) -> Result<Resolved, u32> {
    if extra & !MOD_MASK != 0 {
        l.rejected = l.rejected.saturating_add(1);
        return Err(MkCode::ModBad.val());
    }
    let mods = l.held | extra;
    let r = resolve(key, mods)?;
    l.chords = l.chords.saturating_add(1);
    Ok(r)
}

/// 终态：没有任何修饰位残留。
pub fn terminal(l: &Latch) -> bool {
    l.held == 0
}

// ── 500 次随机组合抽样 ────────────────────────────────────────────
/// 抽样账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleLedger {
    pub rounds: u32,
    pub chorded: u32,
    pub terminal_all: bool,
    pub mismatches: u32,
    pub rejected: u32,
    pub digest: u64,
}

/// 抽样主循环。chunk 控制分块（块间重置锁存，块内同样每轮归零）。
pub fn sample_stream(seed: u64, rounds: u32, chunk: u32) -> Result<SampleLedger, u32> {
    if rounds == 0 || rounds > SAMPLE_ROUNDS {
        return Err(MkCode::CoverBad.val());
    }
    if chunk == 0 {
        return Err(MkCode::SampleBad.val());
    }
    let span = (KEY_MAX as u32) - (KEY_MIN as u32) + 1;
    let mut lcg = p_core::Lcg::new(seed);
    let mut digest = p_core::FNV_OFF;
    let mut chorded: u32 = 0;
    let mut mismatches: u32 = 0;
    let mut rejected: u32 = 0;
    let mut terminal_all = true;
    let mut done: u32 = 0;
    while done < rounds {
        let take = if chunk < rounds - done { chunk } else { rounds - done };
        let mut latch = Latch::new();
        let mut i = 0u32;
        while i < take {
            let _ = lcg.next();
            let key = KEY_MIN + lcg.below(span) as u8;
            let held = lcg.below(16) as u8;
            let extra = lcg.below(16) as u8;
            let mods = held | extra;
            press_mods(&mut latch, held)?;
            let r = chord_with(&mut latch, key, extra)?;
            let want = expected_code(key, mods)?;
            if r.chord.code != want {
                mismatches = mismatches.saturating_add(1);
            }
            // 终态比对的第二条路径：表驱动 payload 必须与 code 低字节一致。
            if let Some(p) = table_payload(key, mods & MOD_SHIFT != 0) {
                if (r.chord.code & 0xFF) as u8 != p {
                    mismatches = mismatches.saturating_add(1);
                }
            } else {
                mismatches = mismatches.saturating_add(1);
            }
            digest = p_core::fold_u32(digest, r.chord.code);
            digest = p_core::fold_u32(digest, mods as u32);
            digest = p_core::fold_u32(digest, key as u32);
            chorded = chorded.saturating_add(1);
            release_mods(&mut latch, held)?;
            if !terminal(&latch) {
                terminal_all = false;
            }
            i += 1;
        }
        rejected = rejected.saturating_add(latch.rejected);
        if !terminal(&latch) {
            terminal_all = false;
        }
        done += take;
    }
    Ok(SampleLedger {
        rounds,
        chorded,
        terminal_all,
        mismatches,
        rejected,
        digest,
    })
}

/// 标准抽样：整段一次跑完。
pub fn sample_run(seed: u64, rounds: u32) -> Result<SampleLedger, u32> {
    sample_stream(seed, rounds, rounds)
}

/// 复跑对：同一输入两次，终态摘要必须相等且非零。
pub fn sample_replay() -> Result<(SampleLedger, SampleLedger), u32> {
    let a = sample_run(SAMPLE_SEED, SAMPLE_ROUNDS)?;
    let b = sample_run(SAMPLE_SEED, SAMPLE_ROUNDS)?;
    Ok((a, b))
}

/// 分块不变性：分块抽样与整段抽样摘要一致（每轮终态干净的直接推论）。
pub fn sample_chunk_invariant() -> bool {
    let full = sample_run(SAMPLE_SEED, SAMPLE_ROUNDS);
    let chunked = sample_stream(SAMPLE_SEED, SAMPLE_ROUNDS, SAMPLE_CHUNK);
    match (full, chunked) {
        (Ok(a), Ok(b)) => a.digest == b.digest && a.digest != 0,
        _ => false,
    }
}

// ── 反例注入（修饰键面） ──────────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub attempts: u32,
    pub caught: u32,
}

impl InjectLog {
    pub fn new() -> InjectLog {
        InjectLog { attempts: 0, caught: 0 }
    }

    pub fn all_caught(&self) -> bool {
        self.attempts == self.caught && self.attempts > 0
    }
}

/// 十条越界输入（修饰位越界 + key 越界）。
pub const BAD_INPUTS: [(u8, u8); 10] = [
    (0x00, 0x00),
    (0x01, 0x00),
    (0x02, MOD_SHIFT),
    (0x03, MOD_CTRL),
    (0xE8, 0x00),
    (0xFE, MOD_SHIFT),
    (0x04, 0x10),
    (0x04, 0x20),
    (0x05, 0x40),
    (0x06, 0x80),
];

/// 越界注入必须全部被 resolve 与边界函数同时拒绝。
pub fn rejection_probe() -> InjectLog {
    let mut log = InjectLog::new();
    let mut i = 0usize;
    while i < BAD_INPUTS.len() {
        if let Some((k, m)) = BAD_INPUTS.get(i) {
            log.attempts = log.attempts.saturating_add(1);
            let r = resolve(*k, *m);
            if r.is_err() && !in_boundary(*k, *m) {
                log.caught = log.caught.saturating_add(1);
            }
        }
        i += 1;
    }
    log
}

/// 锁存反例：释放未按下的修饰位必须被拒。
pub fn latch_rejection() -> bool {
    let mut l = Latch::new();
    if press_mods(&mut l, MOD_SHIFT).is_err() {
        return false;
    }
    release_mods(&mut l, MOD_CTRL).is_err() && l.held == MOD_SHIFT
}

// ── 边界声明 ──────────────────────────────────────────────────────
/// 合法域声明（key 区间 + 修饰掩码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Boundary {
    pub key_lo: u8,
    pub key_hi: u8,
    pub mod_mask: u8,
}

pub const BOUNDARY: Boundary = Boundary { key_lo: KEY_MIN, key_hi: KEY_MAX, mod_mask: MOD_MASK };

/// 边界判定。
pub fn in_boundary(key: u8, mods: u8) -> bool {
    key >= BOUNDARY.key_lo && key <= BOUNDARY.key_hi && mods & !BOUNDARY.mod_mask == 0
}

/// 边界声明的摘要。
pub fn boundary_digest() -> u64 {
    let mut h = p_core::fold_u32(p_core::FNV_OFF, BOUNDARY.key_lo as u32);
    h = p_core::fold_u32(h, BOUNDARY.key_hi as u32);
    h = p_core::fold_u32(h, BOUNDARY.mod_mask as u32);
    p_core::fold_u32(h, 1)
}

/// 边界声明行（六字段，可被 audit 原样复现）。
pub fn boundary_row() -> String {
    render_row(
        BOUND_TAG,
        BOUNDARY.key_lo as u32,
        BOUNDARY.key_hi as u32,
        BOUNDARY.mod_mask as u32,
        1,
        boundary_digest(),
    )
}

/// 边界声明落账：行可解析、字段与常量一致。
pub fn boundary_ok() -> bool {
    let row = boundary_row();
    let (tag, vals, digest) = match row_fields(&row) {
        Some(v) => v,
        None => return false,
    };
    tag == BOUND_TAG
        && vals[0] == BOUNDARY.key_lo as u32
        && vals[1] == BOUNDARY.key_hi as u32
        && vals[2] == BOUNDARY.mod_mask as u32
        && vals[3] == 1
        && digest == boundary_digest()
        && audit_render(&row)
}

// ── 生成序：递交 → 队列 → 消费 ───────────────────────────────────
/// 一个键事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub seq: u32,
    pub key: u8,
    pub mods: u8,
    pub code: u32,
}

/// 事件摘要折入。
pub fn fold_event(h: u64, ev: Event) -> u64 {
    let mut x = p_core::fold_u32(h, ev.seq);
    x = p_core::fold_u32(x, ev.key as u32);
    x = p_core::fold_u32(x, ev.mods as u32);
    p_core::fold_u32(x, ev.code)
}

/// 递交/消费管道。
#[derive(Clone, Debug)]
pub struct Pipe {
    pub next_seq: u32,
    pub queue: VecDeque<Event>,
    pub submitted: u32,
    pub delivered: u32,
    pub rejected: u32,
    pub order_viol: u32,
    pub produce_digest: u64,
    pub consume_digest: u64,
}

impl Pipe {
    pub fn new() -> Pipe {
        Pipe {
            next_seq: 0,
            queue: VecDeque::new(),
            submitted: 0,
            delivered: 0,
            rejected: 0,
            order_viol: 0,
            produce_digest: p_core::FNV_OFF,
            consume_digest: p_core::FNV_OFF,
        }
    }

    pub fn pending(&self) -> u32 {
        self.queue.len() as u32
    }

    pub fn linked(&self) -> bool {
        self.produce_digest == self.consume_digest
            && self.pending() == 0
            && self.submitted == self.delivered
            && self.order_viol == 0
            && self.submitted > 0
    }
}

/// 递交一个事件：先边界，后解析，再入队。
pub fn submit(p: &mut Pipe, key: u8, mods: u8) -> Result<Event, u32> {
    if !in_boundary(key, mods) {
        p.rejected = p.rejected.saturating_add(1);
        return Err(MkCode::BoundBad.val());
    }
    let r = resolve(key, mods)?;
    let seq = p.next_seq;
    let next = match seq.checked_add(1) {
        Some(v) => v,
        None => return Err(MkCode::Overflow.val()),
    };
    let ev = Event { seq, key, mods, code: r.chord.code };
    p.queue.push_back(ev);
    p.next_seq = next;
    p.submitted = p.submitted.saturating_add(1);
    p.produce_digest = fold_event(p.produce_digest, ev);
    Ok(ev)
}

/// 消费一个事件（FIFO）；空队列返回 None。
pub fn consume(p: &mut Pipe) -> Result<Option<Event>, u32> {
    let ev = match p.queue.pop_front() {
        Some(v) => v,
        None => return Ok(None),
    };
    if ev.seq != p.delivered {
        p.order_viol = p.order_viol.saturating_add(1);
    }
    p.delivered = p.delivered.saturating_add(1);
    p.consume_digest = fold_event(p.consume_digest, ev);
    Ok(Some(ev))
}

/// 生成序账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrderLedger {
    pub rounds: u32,
    pub submitted: u32,
    pub delivered: u32,
    pub order_viol: u32,
    pub rejected: u32,
    pub produce_digest: u64,
    pub consume_digest: u64,
    pub pending: u32,
}

impl OrderLedger {
    pub fn linked(&self) -> bool {
        self.produce_digest == self.consume_digest
            && self.submitted == self.delivered
            && self.pending == 0
            && self.order_viol == 0
            && self.submitted > 0
    }
}

/// 生成序 ×rounds（每轮随机递交 1..=7 条，随机消费 0..=队列长）。
pub fn order_rounds(seed: u64, rounds: u32) -> Result<OrderLedger, u32> {
    if rounds == 0 || rounds > ORDER_ROUNDS {
        return Err(MkCode::CoverBad.val());
    }
    let span = (KEY_MAX as u32) - (KEY_MIN as u32) + 1;
    let mut lcg = p_core::Lcg::new(seed);
    let mut p = Pipe::new();
    let mut i = 0u32;
    while i < rounds {
        let burst = 1 + lcg.below(ORDER_BURST);
        let mut j = 0u32;
        while j < burst {
            let key = KEY_MIN + lcg.below(span) as u8;
            let mods = lcg.below(16) as u8;
            let _ = submit(&mut p, key, mods)?;
            j += 1;
        }
        let qlen = p.pending();
        let take = if qlen == 0 { 0 } else { lcg.below(qlen + 1) };
        let mut t = 0u32;
        while t < take {
            let _ = consume(&mut p)?;
            t += 1;
        }
        i += 1;
    }
    while !p.queue.is_empty() {
        let _ = consume(&mut p)?;
    }
    Ok(OrderLedger {
        rounds,
        submitted: p.submitted,
        delivered: p.delivered,
        order_viol: p.order_viol,
        rejected: p.rejected,
        produce_digest: p.produce_digest,
        consume_digest: p.consume_digest,
        pending: p.pending(),
    })
}

/// 生成序复跑对。
pub fn order_replay() -> Result<(OrderLedger, OrderLedger), u32> {
    let a = order_rounds(ORDER_SEED, ORDER_ROUNDS)?;
    let b = order_rounds(ORDER_SEED, ORDER_ROUNDS)?;
    Ok((a, b))
}

// ── 联签 ──────────────────────────────────────────────────────────
/// 联签结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Signature {
    pub submitted: u32,
    pub delivered: u32,
    pub order_viol: u32,
    pub joint: u64,
    pub row: String,
}

/// 由生成序账导出联签行（两个摘要相等才联签）。
pub fn joint_signature(l: &OrderLedger) -> Result<Signature, u32> {
    if l.produce_digest != l.consume_digest {
        return Err(MkCode::SignBad.val());
    }
    let joint = p_core::fold_u32(
        p_core::fold_u32(l.produce_digest, l.consume_digest as u32),
        l.submitted,
    );
    let row = render_row(
        SIGN_TAG,
        ORDER_ROUNDS,
        l.delivered,
        l.order_viol,
        l.submitted,
        joint,
    );
    Ok(Signature {
        submitted: l.submitted,
        delivered: l.delivered,
        order_viol: l.order_viol,
        joint,
        row,
    })
}

/// 联签锚定行校验：可 audit、标签原文、字段与账一致。
pub fn signature_ok(l: &OrderLedger, sig: &Signature) -> bool {
    let (tag, vals, digest) = match row_fields(&sig.row) {
        Some(v) => v,
        None => return false,
    };
    tag == SIGN_TAG
        && vals[0] == ORDER_ROUNDS
        && vals[1] == l.delivered
        && vals[2] == l.order_viol
        && vals[3] == l.submitted
        && digest == sig.joint
        && sig.submitted == l.submitted
        && sig.delivered == l.delivered
        && audit_render(&sig.row)
}

/// 越界注入结果。
#[derive(Clone, Debug)]
pub struct BoundProbe {
    pub attempts: u32,
    pub accepted_outside: u32,
    pub pending_before: u32,
    pub pending_after: u32,
    pub log: InjectLog,
}

/// 十条越界输入逐一递交，必须在入队前被拒。
pub fn boundary_inject() -> Result<BoundProbe, u32> {
    let mut p = Pipe::new();
    let _ = submit(&mut p, KEY_MIN, 0)?;
    let pending_before = p.pending();
    let mut log = InjectLog::new();
    let mut accepted_outside = 0u32;
    let mut i = 0usize;
    while i < BAD_INPUTS.len() {
        if let Some((k, m)) = BAD_INPUTS.get(i) {
            log.attempts = log.attempts.saturating_add(1);
            match submit(&mut p, *k, *m) {
                Err(_) => {
                    log.caught = log.caught.saturating_add(1);
                }
                Ok(_) => {
                    accepted_outside = accepted_outside.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    let pending_after = p.pending();
    Ok(BoundProbe {
        attempts: log.attempts,
        accepted_outside,
        pending_before,
        pending_after,
        log,
    })
}

// ── 段位守卫 ──────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < MkCode::ALL.len() {
        if let Some(c) = MkCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段渲染：tag | params | schedule | fails | keys | digest。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

/// 六字段解析：返回标签、四个数值与摘要。
pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.get(0).and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return None,
        };
        if sp.next().is_some() {
            return None;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => {
            render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
        }
        None => false,
    }
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49027 ①：修饰表自洽（位值/并集/顺序 + 字母数字表长度 + 双路径恒等）。
    let table = mod_table_ok() && digit_table_ok();
    let agree = paths_agree();
    out.push((
        "F49027/修饰表自洽",
        table && agree,
        format!("table={} agree={} mask=0x{:02X}", table, agree, MOD_MASK),
    ));

    // F49027 ②：500 次抽样终态干净（每轮 held 归零、双路径零分歧）。
    let sample = sample_run(SAMPLE_SEED, SAMPLE_ROUNDS);
    let sample_ok = match &sample {
        Ok(l) => {
            l.rounds == SAMPLE_ROUNDS
                && l.chorded == SAMPLE_ROUNDS
                && l.terminal_all
                && l.mismatches == 0
                && l.rejected == 0
        }
        Err(_) => false,
    };
    out.push((
        "F49027/终态一致",
        sample_ok,
        format!(
            "rounds={:?} terminal={:?} mismatches={:?}",
            sample.as_ref().map(|l| l.rounds),
            sample.as_ref().map(|l| l.terminal_all),
            sample.as_ref().map(|l| l.mismatches)
        ),
    ));

    // F49027 ③：复跑零漂移（同种子两次摘要相等 + 分块不变）。
    let replay = sample_replay();
    let replay_ok = match &replay {
        Ok((a, b)) => a.digest == b.digest && a.digest != 0 && a.terminal_all && b.terminal_all,
        Err(_) => false,
    };
    let chunk = sample_chunk_invariant();
    out.push((
        "F49027/复跑零漂移",
        replay_ok && chunk,
        format!(
            "digest={:016X} chunk={}",
            replay.as_ref().map(|(a, _)| a.digest).unwrap_or_else(|_| 0),
            chunk
        ),
    ));

    // F49027 ④：反例必拒（10 条越界 + 锁存误释放 + 段位）。
    let rej = rejection_probe();
    let latch = latch_rejection();
    let seg = segment_guard();
    out.push((
        "F49027/反例必拒",
        rej.all_caught() && rej.attempts == BOUND_INJECT && latch && seg,
        format!(
            "attempts={} caught={} latch={} seg={}",
            rej.attempts, rej.caught, latch, seg
        ),
    ));

    // F49028 ①：递交序与消费序一致（×1000，零乱序、零丢失）。
    let order = order_rounds(ORDER_SEED, ORDER_ROUNDS);
    let order_ok = match &order {
        Ok(l) => l.rounds == ORDER_ROUNDS && l.linked(),
        Err(_) => false,
    };
    out.push((
        "F49028/序一致",
        order_ok,
        format!(
            "submitted={:?} delivered={:?} viol={:?} pending={:?}",
            order.as_ref().map(|l| l.submitted),
            order.as_ref().map(|l| l.delivered),
            order.as_ref().map(|l| l.order_viol),
            order.as_ref().map(|l| l.pending)
        ),
    ));

    // F49028 ②：联签锚定行可原样复现。
    let sig_ok = match &order {
        Ok(l) => match joint_signature(l) {
            Ok(sig) => signature_ok(l, &sig),
            Err(_) => false,
        },
        Err(_) => false,
    };
    out.push((
        "F49028/联签锚定",
        sig_ok,
        format!("tag={} audited={}", SIGN_TAG, sig_ok),
    ));

    // F49028 ③：边界声明落账 + 越界注入 ×10 零越权。
    let bound = boundary_inject();
    let bound_ok = match &bound {
        Ok(b) => {
            b.attempts == BOUND_INJECT
                && b.accepted_outside == 0
                && b.log.all_caught()
                && b.pending_before == b.pending_after
        }
        Err(_) => false,
    };
    let decl = boundary_ok();
    out.push((
        "F49028/边界零越权",
        bound_ok && decl,
        format!(
            "declared={} attempts={} accepted_outside={:?}",
            decl,
            BOUND_INJECT,
            bound.as_ref().map(|b| b.accepted_outside)
        ),
    ));

    // F49028 ④：生成序复跑一致。
    let replay2 = order_replay();
    let rep_ok = match &replay2 {
        Ok((a, b)) => {
            a.produce_digest == b.produce_digest
                && a.consume_digest == b.consume_digest
                && a.produce_digest != 0
                && a.linked()
                && b.linked()
        }
        Err(_) => false,
    };
    out.push((
        "F49028/复跑一致",
        rep_ok,
        format!(
            "digest={:016X} stable={}",
            replay2.as_ref().map(|(a, _)| a.produce_digest).unwrap_or_else(|_| 0),
            rep_ok
        ),
    ));

    out
}

pub fn run_unx49027_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49028 修饰键抽样与组合键生成序");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_table_is_coherent() {
        assert!(mod_table_ok());
        assert!(digit_table_ok());
    }

    #[test]
    fn mask_covers_all_modifiers() {
        let mut u = 0u8;
        let mut i = 0usize;
        while i < MODIFIERS.len() {
            if let Some(m) = MODIFIERS.get(i) {
                u |= m.bit;
            }
            i += 1;
        }
        assert_eq!(u, MOD_MASK);
        assert_eq!(MOD_MASK, MOD_SHIFT | MOD_CTRL | MOD_ALT | MOD_SUPER);
    }

    #[test]
    fn letter_payload_both_cases() {
        assert_eq!(payload(0x04, false), b'a');
        assert_eq!(payload(0x04, true), b'A');
        assert_eq!(payload(0x1D, false), b'z');
        assert_eq!(payload(0x1D, true), b'Z');
        assert_eq!(table_payload(0x11, false), Some(b'n'));
        assert_eq!(table_payload(0x11, true), Some(b'N'));
    }

    #[test]
    fn digit_payload_both_cases() {
        assert_eq!(payload(0x1E, false), b'1');
        assert_eq!(payload(0x1E, true), b'!');
        assert_eq!(payload(0x27, false), b'0');
        assert_eq!(payload(0x27, true), b')');
        assert_eq!(table_payload(0x23, true), Some(b'^'));
    }

    #[test]
    fn non_printable_key_passes_through() {
        assert_eq!(payload(0x29, false), 0x29);
        assert_eq!(payload(0xE7, false), 0xE7);
        assert_eq!(table_payload(0xE7, true), Some(0xE7));
    }

    #[test]
    fn resolve_rejects_bad_mods() {
        assert_eq!(resolve(0x04, 0x10), Err(MkCode::ModBad.val()));
        assert_eq!(resolve(0x04, 0xFF), Err(MkCode::ModBad.val()));
    }

    #[test]
    fn resolve_rejects_bad_keys() {
        assert_eq!(resolve(0x00, 0), Err(MkCode::KeyBad.val()));
        assert_eq!(resolve(0x03, MOD_SHIFT), Err(MkCode::KeyBad.val()));
        assert_eq!(resolve(0xE8, 0), Err(MkCode::KeyBad.val()));
    }

    #[test]
    fn altgr_is_not_menu_accelerator() {
        let r = match resolve(0x04, MOD_CTRL | MOD_ALT) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert!(r.altgr);
        assert!(!r.menu);
        let m = match resolve(0x04, MOD_CTRL) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert!(m.menu);
        assert!(!m.altgr);
    }

    #[test]
    fn super_and_shift_combine() {
        let r = match resolve(0x04, MOD_SUPER | MOD_SHIFT) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(r.chord.mods, MOD_SUPER | MOD_SHIFT);
        assert_eq!((r.chord.code & 0xFF) as u8, b'A');
    }

    #[test]
    fn paths_agree_everywhere() {
        assert!(paths_agree());
    }

    #[test]
    fn latch_terminal_after_release() {
        let mut l = Latch::new();
        assert!(press_mods(&mut l, MOD_SHIFT | MOD_CTRL).is_ok());
        assert!(!terminal(&l));
        assert!(release_mods(&mut l, MOD_SHIFT).is_ok());
        assert!(release_mods(&mut l, MOD_CTRL).is_ok());
        assert!(terminal(&l));
        assert_eq!(l.rejected, 0);
    }

    #[test]
    fn latch_rejects_unknown_release() {
        let mut l = Latch::new();
        assert!(press_mods(&mut l, MOD_SHIFT).is_ok());
        assert_eq!(release_mods(&mut l, MOD_CTRL), Err(MkCode::LatchBad.val()));
        assert_eq!(l.held, MOD_SHIFT);
        assert_eq!(l.rejected, 1);
        assert!(latch_rejection());
    }

    #[test]
    fn chord_counts_only_on_success() {
        let mut l = Latch::new();
        assert!(press_mods(&mut l, MOD_CTRL).is_ok());
        assert!(chord_with(&mut l, 0x04, 0).is_ok());
        assert_eq!(l.chords, 1);
        assert_eq!(chord_with(&mut l, 0x02, 0), Err(MkCode::KeyBad.val()));
        assert_eq!(l.chords, 1);
    }

    #[test]
    fn sample_500_is_clean() {
        let l = match sample_run(SAMPLE_SEED, SAMPLE_ROUNDS) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.rounds, SAMPLE_ROUNDS);
        assert_eq!(l.chorded, SAMPLE_ROUNDS);
        assert!(l.terminal_all);
        assert_eq!(l.mismatches, 0);
        assert_eq!(l.rejected, 0);
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn sample_replay_is_stable() {
        let (a, b) = match sample_replay() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(a.digest, b.digest);
        assert_ne!(a.digest, 0);
        assert!(a.terminal_all && b.terminal_all);
    }

    #[test]
    fn sample_chunk_invariant_holds() {
        assert!(sample_chunk_invariant());
    }

    #[test]
    fn sample_rejects_bad_cover() {
        assert_eq!(sample_run(SAMPLE_SEED, 0), Err(MkCode::CoverBad.val()));
        assert_eq!(sample_run(SAMPLE_SEED, SAMPLE_ROUNDS + 1), Err(MkCode::CoverBad.val()));
        assert_eq!(sample_stream(SAMPLE_SEED, 10, 0), Err(MkCode::SampleBad.val()));
    }

    #[test]
    fn ten_bad_inputs_all_rejected() {
        let log = rejection_probe();
        assert_eq!(log.attempts, BOUND_INJECT);
        assert_eq!(log.caught, BOUND_INJECT);
        assert!(log.all_caught());
    }

    #[test]
    fn boundary_row_is_auditable() {
        let row = boundary_row();
        assert!(audit_render(&row));
        let (tag, vals, _) = match row_fields(&row) {
            Some(v) => v,
            None => return assert!(false),
        };
        assert_eq!(tag, BOUND_TAG);
        assert_eq!(vals[0], BOUNDARY.key_lo as u32);
        assert_eq!(vals[1], BOUNDARY.key_hi as u32);
        assert_eq!(vals[2], BOUNDARY.mod_mask as u32);
        assert!(boundary_ok());
    }

    #[test]
    fn boundary_inject_never_accepted() {
        let b = match boundary_inject() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(b.attempts, BOUND_INJECT);
        assert_eq!(b.accepted_outside, 0);
        assert!(b.log.all_caught());
        assert_eq!(b.pending_before, b.pending_after);
    }

    #[test]
    fn pipe_fifo_order_is_exact() {
        let mut p = Pipe::new();
        assert!(submit(&mut p, 0x04, 0).is_ok());
        assert!(submit(&mut p, 0x05, MOD_SHIFT).is_ok());
        assert!(submit(&mut p, 0x06, MOD_CTRL).is_ok());
        assert_eq!(p.pending(), 3);
        let a = match consume(&mut p) {
            Ok(Some(e)) => e,
            _ => return assert!(false),
        };
        let b = match consume(&mut p) {
            Ok(Some(e)) => e,
            _ => return assert!(false),
        };
        let c = match consume(&mut p) {
            Ok(Some(e)) => e,
            _ => return assert!(false),
        };
        assert_eq!((a.seq, b.seq, c.seq), (0, 1, 2));
        assert_eq!(a.code, match resolve(0x04, 0) { Ok(r) => r.chord.code, Err(_) => 0 });
        assert_eq!(b.code, match resolve(0x05, MOD_SHIFT) { Ok(r) => r.chord.code, Err(_) => 0 });
        assert!(consume(&mut p).is_ok());
        assert!(p.linked());
    }

    #[test]
    fn pipe_detects_sequence_gap() {
        let mut p = Pipe::new();
        p.next_seq = 5;
        assert!(submit(&mut p, 0x04, 0).is_ok());
        assert!(consume(&mut p).is_ok());
        assert_eq!(p.order_viol, 1);
        assert!(!p.linked());
    }

    #[test]
    fn order_1000_is_linked() {
        let l = match order_rounds(ORDER_SEED, ORDER_ROUNDS) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.rounds, ORDER_ROUNDS);
        assert!(l.linked());
        assert_eq!(l.order_viol, 0);
        assert_eq!(l.pending, 0);
        assert!(l.submitted >= ORDER_ROUNDS);
    }

    #[test]
    fn order_rejects_bad_cover() {
        assert_eq!(order_rounds(ORDER_SEED, 0), Err(MkCode::CoverBad.val()));
        assert_eq!(order_rounds(ORDER_SEED, ORDER_ROUNDS + 1), Err(MkCode::CoverBad.val()));
    }

    #[test]
    fn signature_is_anchored_and_audited() {
        let l = match order_rounds(ORDER_SEED, ORDER_ROUNDS) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let sig = match joint_signature(&l) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert!(sig.row.starts_with(SIGN_TAG));
        assert!(signature_ok(&l, &sig));
        let (tag, vals, digest) = match row_fields(&sig.row) {
            Some(v) => v,
            None => return assert!(false),
        };
        assert_eq!(tag, "UNX-F49028-J1");
        assert_eq!(vals[0], ORDER_ROUNDS);
        assert_eq!(vals[1], l.delivered);
        assert_eq!(vals[2], l.order_viol);
        assert_eq!(vals[3], l.submitted);
        assert_eq!(digest, sig.joint);
    }

    #[test]
    fn signature_rejects_broken_row() {
        let l = match order_rounds(ORDER_SEED, ORDER_ROUNDS) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let sig = match joint_signature(&l) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let mut row = sig.row.clone();
        row.push('|');
        assert!(!audit_render(&row));
        let broken = Signature { row, ..sig };
        assert!(!signature_ok(&l, &broken));
    }

    #[test]
    fn signature_requires_equal_digests() {
        let mut l = match order_rounds(ORDER_SEED, ORDER_ROUNDS) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        l.consume_digest = l.consume_digest ^ 0xFFFF;
        assert_eq!(joint_signature(&l), Err(MkCode::SignBad.val()));
    }

    #[test]
    fn render_parsers_roundtrip() {
        let row = render_row("T", 1, 2, 3, 4, 0xAB);
        assert!(audit_render(&row));
        assert!(parse_u32("0").is_some());
        assert!(parse_u32("4294967295").is_some());
        assert!(parse_u32("4294967296").is_none());
        assert!(parse_u32("").is_none());
        assert!(parse_u32("12a").is_none());
        assert!(parse_hex16("00000000000000AB").is_some());
        assert!(parse_hex16("0000000000000AB").is_none());
        assert!(!audit_render(""));
        assert!(!audit_render("T|params=1|schedule=2|fails=3|keys=4"));
        assert!(!audit_render("T|params=1|schedule=2|fails=3|keys=4|digest=ZZZZZZZZZZZZZZZZ"));
    }

    #[test]
    fn code_window_and_detail() {
        assert!(segment_guard());
        assert_eq!(MkCode::ModBad.val(), SEG_BASE + 1);
        assert_eq!(MkCode::CoverBad.val(), SEG_BASE + 14);
        assert_eq!(detail(MkCode::SignBad.val()), MkCode::SignBad.detail());
        assert_eq!(detail(0), "未知键盘组合诊断码");
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let s = run_unx49027_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }
}
