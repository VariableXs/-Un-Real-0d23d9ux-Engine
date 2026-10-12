//! unx584_unkparam —— UNX-F0584 未知参数账注（显性弃置）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B30）：
//!   UNX-F0584 · 未知参数账注（显性弃置） · 判据：未知键账注账齐
//!   （弃置簿 16 槽——原文/时点/位置），弃置率 100%（零执行断言）
//! · 组件：弃置簿 16 槽 + 原文留痕 + 时点/位置锚 + 弃置率账 + 零执行断言
//!
//! ## 这个模块解决什么真问题
//!
//! 参数解析层最危险的不是"解析失败"，而是**静默忽略**：
//! 调用方传了一个拼错的键（`timoutnanos` 少了 `e`），解析层跳过它、
//! 用默认值继续跑，于是超时变成 10 秒而不是 1 秒——**没有任何错误码、
//! 没有任何日志、没有任何返回值变化**，程序"成功"地跑完了，但行为全错。
//!
//! Windows 的做法是"未识别参数走 `Unknown` 桶，通常忽略"。这在
//! 应用层可以接受，但在内核/启动参数面不可接受：启动参数拼错一个字母，
//! 后果是 quiet 级别失效、时钟源选错、内存上限被静默改小，且**没有任何
//! 现场**可查。所以本模块做的是 Windows 做得浅的那一层：
//!
//! 1. **零静默**：每个未知键必须显式落账，账不满则整体拒（fail-closed）。
//! 2. **原文留痕**：不是记"有个键不认识"，而是记**原文字节**——
//!    截断到、但可复现。这是排障时唯一有用的东西。
//! 3. **时点锚**：弃置发生在第几个 tick、哪一轮解析。同一未知键在
//!    不同阶段出现，危险度完全不同（启动期 vs 运行期）。
//! 4. **位置锚**：指向来源缓冲区的偏移+长度，能定位到"是第几个参数
//!    写错的"，而不是"参数表里有个错的"。
//! 5. **零执行断言**：判据要求"弃置率 100%（零执行断言）"——
//!    意思是**没有一个未知键被执行过**。这是本模块的核心不变量：
//!    账记了不等于没发生，必须证明"记账的同时它没进到语义层"。
//!
//! ## 为什么 16 槽是硬上限
//!
//! 弃置簿是内核早期启动期就要用的结构，不能动态分配（那时堆还没建）。
//! 16 槽 = 定容。槽满之后**不是覆盖最旧的**，而是转`Overflowed`
//! 并让整体判定转为拒——因为"只留前 16 个"意味着第 17 个之后的
//! 未知键完全没留痕，此时给出"账齐"的结论是撒谎。

#![allow(clippy::too_many_arguments)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U584_VERSION: &str = "u584-unkparam-v1";

/// 诊断段基址（本单独占）。
pub const U584_SEG_BASE: u16 = 0xC100;

/// 弃置簿槽数（定容·判据口径）。
pub const SLOTS: usize = 16;

/// 单条原文留痕的字节上限（超长截断·记`truncated`）。
pub const RAW_CAP: usize = 48;

/// --------------------------------------------------------------------------
// 诊断码（十二码 · 段内连续）
// --------------------------------------------------------------------------

/// UNX-F0584 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum U584Code {
    /// 0xC100 簿未初始化（构造前禁止读）。
    NotInit,
    /// 0xC101 槽已满且仍有未知键待记（转溢出·整体拒）。
    Overflowed,
    /// 0xC102 原文超 RAW_CAP 被截断。
    RawTruncated,
    /// 0xC103 时点非法（tick 倒退）。
    TimeBack,
    /// 0xC104 位置越界（偏移+长度超出源缓冲）。
    PosRange,
    /// 0xC105 零长度原文（空键不算未知键）。
    EmptyRaw,
    /// 0xC106 键已弃置过（重复弃置去重）。
    DupKey,
    /// 0xC107 槽位非空（写入未清槽）。
    SlotDirty,
    /// 0xC108 阶段非法（不在闭集内）。
    BadPhase,
    /// 0xC109 原文非法 UTF-8 边界（多字节被切断）。
    RawSplit,
    /// 0xC10A 弃置后仍被取语义值（零执行断言破）。
    Leaked,
    /// 0xC10B 账目不守恒（记账数≠槽内有效数）。
    Unbalanced,
}

impl U584Code {
    /// 码值。
    pub fn val(self) -> u16 {
        U584_SEG_BASE
            + match self {
                U584Code::NotInit => 0x0,
                U584Code::Overflowed => 0x1,
                U584Code::RawTruncated => 0x2,
                U584Code::TimeBack => 0x3,
                U584Code::PosRange => 0x4,
                U584Code::EmptyRaw => 0x5,
                U584Code::DupKey => 0x6,
                U584Code::SlotDirty => 0x7,
                U584Code::BadPhase => 0x8,
                U584Code::RawSplit => 0x9,
                U584Code::Leaked => 0xA,
                U584Code::Unbalanced => 0xB,
            }
    }

    /// 全部码（顺序即码序 · 12 码）。
    pub const ALL: [U584Code; 12] = [
        U584Code::NotInit,
        U584Code::Overflowed,
        U584Code::RawTruncated,
        U584Code::TimeBack,
        U584Code::PosRange,
        U584Code::EmptyRaw,
        U584Code::DupKey,
        U584Code::SlotDirty,
        U584Code::BadPhase,
        U584Code::RawSplit,
        U584Code::Leaked,
        U584Code::Unbalanced,
    ];

    /// 可操作建议（错误路径必带修复指引）。
    pub fn remedy(self) -> &'static str {
        match self {
            U584Code::NotInit => "簿未初始化：先new_discard_book() 再 read，禁止读未建簿",
            U584Code::Overflowed => "弃置簿已满：未知键超过 16 个，先收敛参数面再重试，不得覆盖旧槽",
            U584Code::RawTruncated => "原文超 48 字节被截断：长参数应改走配置文件，别塞进命令行",
            U584Code::TimeBack => "时点倒退：tick 单调，须传不小于上一次弃置的时点",
            U584Code::PosRange => "位置越界：offset+len 必须落在源缓冲内，先校验源长度再报位置",
            U584Code::EmptyRaw => "零长度原文：空键不是未知键，解析层应直接跳过且不记账",
            U584Code::DupKey => "重复弃置：同一键已入簿，去重后仍计一次未知但只占一槽",
            U584Code::SlotDirty => "槽位非空：覆盖前必须 clear_slot，否则旧原文会与新键混账",
            U584Code::BadPhase => "阶段非法：只能是 boot/kernel/cmdline 三者之一",
            U584Code::RawSplit => "原文切断多字节字符：截断点应回退到 UTF-8 字符边界",
            U584Code::Leaked => "零执行断言破：被弃置的键仍取到了语义值，必须查语义层是否先查弃置表",
            U584Code::Unbalanced => "账目不守恒：recorded 应等于槽内有效条数，差值即丢账",
        }
    }

    /// 一行式描述。
    pub fn line(self) -> String {
        format!("[0x{:04X}] {}", self.val(), self.remedy())
    }
}

/// 段位守卫：全部诊断码必须落在本段 `[0xC100, 0xC10F]`。
pub fn segment_guard(c: u16) -> bool {
    c >= U584_SEG_BASE && c < U584_SEG_BASE + 0x10
}

// --------------------------------------------------------------------------
// 阶段闭集
// --------------------------------------------------------------------------

/// 弃置发生的阶段（三态闭集·不开放）。
///
/// 为什么闭集而不做成字符串：阶段是要进诊断码的，做成字符串就意味着
/// 运行时比较字节——那是热路径上不该有的事，也是"阶段名写错但没人发现"
/// 的来源。做成枚举，错只能在编译期暴露。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 固件/引导阶段。
    Boot,
    /// 内核早期初始化。
    Kernel,
    /// 命令行/最终参数面。
    Cmdline,
}

impl Phase {
    /// 序号（用于阶段序比较·启动早的序号小）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Boot => 0,
            Phase::Kernel => 1,
            Phase::Cmdline => 2,
        }
    }

    /// 阶段名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Boot => "boot",
            Phase::Kernel => "kernel",
            Phase::Cmdline => "cmdline",
        }
    }

    /// 由序号反解（越界返回 `None`）。
    pub fn from_ord(v: u8) -> Option<Phase> {
        match v {
            0 => Some(Phase::Boot),
            1 => Some(Phase::Kernel),
            2 => Some(Phase::Cmdline),
            _ => None,
        }
    }

    /// 全部阶段（顺序即阶段序）。
    pub const ALL: [Phase; 3] = [Phase::Boot, Phase::Kernel, Phase::Cmdline];
}

/// 阶段序倒退诊断：同一未知键若在更晚阶段又出现，返回该阶段以示警告。
pub fn phase_regress(later: Phase, earlier: Phase) -> bool {
    later.ord() < earlier.ord()
}

// --------------------------------------------------------------------------
// 槽
// --------------------------------------------------------------------------

/// 弃置槽（原文 / 时点 / 位置 三件套齐）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Slot {
    /// 原文（截断到 [`RAW_CAP`]·键本身，不是整条命令行）。
    pub raw: String,
    /// 是否被截断。
    pub truncated: bool,
    /// 弃置时点（逻辑 tick·零墙钟）。
    pub at_tick: u64,
    /// 阶段。
    pub phase: Phase,
    /// 源缓冲内偏移。
    pub off: u32,
    /// 源缓冲内长度（**原始**长度·即便被截断也是原长）。
    pub len: u32,
    /// 原文指纹（FNV-1a64·对原文改动敏感）。
    pub raw_fp: u64,
    /// 是否已被去重合并（重复弃置只更新此项与时间）。
    pub merged: u32,
}

impl Slot {
    /// 空槽（`raw` 空即视为未占）。
    pub fn empty() -> Slot {
        Slot {
            raw: String::new(),
            truncated: false,
            at_tick: 0,
            phase: Phase::Boot,
            off: 0,
            len: 0,
            raw_fp: 0,
            merged: 0,
        }
    }

    /// 是否空槽。
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// 三件套齐（原文非空 + 时点有值 + 长度非零）。
    ///
    /// 位置为 0 是合法的（源缓冲首部），所以不能用 `off != 0` 当"有位置"。
    pub fn triad_complete(&self) -> bool {
        !self.raw.is_empty() && self.len != 0
    }

    /// 位置区间（`off..off+len`·checked）。
    pub fn span(&self) -> Option<(u32, u32)> {
        let end = self.off.checked_add(self.len)?;
        Some((self.off, end))
    }

    /// 覆盖到源缓冲长度内。
    pub fn within(&self, src_len: u32) -> bool {
        match self.span() {
            Some((a, b)) => b <= src_len,
            None => false,
        }
    }
}

/// 槽指纹（对整槽敏感·含原文与位置）。
pub fn slot_fp(s: &Slot) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.raw.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^= s.at_tick;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    h ^= s.off as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    h ^= s.len as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    h ^ (s.phase.ord() as u64)
}

// --------------------------------------------------------------------------
// UTF-8 边界回退（截断不得切断字符）
// --------------------------------------------------------------------------

/// 回退到 UTF-8 字符边界（返回合法截断长度）。
///
/// 判据要求留原文，而留原文最容易被忽略的坑就是：从字节中间切断一个
/// 多字节字符，存下来的"原文"已经不是原文了——排障时照着它去 grep
/// 一定grep 不到，因为那个字符已经不在里面了。
pub fn utf8_safe_len(bytes: &[u8], cap: usize) -> usize {
    let mut n = if cap < bytes.len() { cap } else { bytes.len() };
    // 回退：若第 n 字节是后续字节（10xxxxxx），说明 n 落在字符中间，
    // 须退到下一个首字节之前。注意 n 可能等于 bytes.len()，此时无此字节。
    while n > 0 && n < bytes.len() && (bytes[n] & 0xC0) == 0x80 {
        n -= 1;
    }
    n
}

/// 是否会切断多字节字符（判据用·不做实际截断）。
pub fn would_split(bytes: &[u8], cap: usize) -> bool {
    if cap >= bytes.len() {
        return false;
    }
    if cap == 0 {
        return bytes.len() > 0;
    }
    (bytes[cap] & 0xC0) == 0x80
}

// --------------------------------------------------------------------------
// 零执行面（记账与执行的隔离）
// --------------------------------------------------------------------------

/// 零执行面：语义取值必须先查此面。
///
/// 这是"零执行断言"能成立的关键。弃置簿只**记账**；语义层取值必须先
/// 问 `shadowed(key)`。若语义层绕过此面直接取默认值，弃置就只是
/// 一张纸——所以本面是**可查的**：账上有的键，`shadowed` 必为真，
/// 语义层拿不到它。
///
/// 这里刻意不提供"取默认值并记账"的组合 API：一旦提供，
/// 就会有人图省事直接用它，零执行断言立刻变成一句空话。
#[derive(Clone, Debug, Default)]
pub struct ZeroExec {
    /// 已执行的键（正常参数·不进弃置簿）。
    executed: Vec<String>,
    /// 逻辑 tick。
    tick: u64,
    /// 执行计数。
    exec_n: u64,
}

impl ZeroExec {
    /// 造空面。
    pub fn new() -> ZeroExec {
        ZeroExec {
            executed: Vec::new(),
            tick: 0,
            exec_n: 0,
        }
    }

    /// 推进逻辑 tick（零墙钟注入）。
    pub fn tick_to(&mut self, t: u64) -> bool {
        if t < self.tick {
            return false;
        }
        self.tick = t;
        true
    }

    /// 正常参数执行（**不进**弃置簿）。
    pub fn exec(&mut self, key: &str) -> bool {
        if key.is_empty() {
            return false;
        }
        self.executed.push(String::from(key));
        self.exec_n = self.exec_n.wrapping_add(1);
        true
    }

    /// 是否被弃置面遮蔽（语义层取它必须返回"不可用"）。
    ///
    /// 查的是**弃置簿**而非执行集——语义层的正确问法是"这个键是不是
    /// 已经被判为未知了"，而不是"它执行过没有"。查错对象就会出现
    /// "执行集里有它所以可取"的反向漏洞。
    pub fn shadowed(&self, book: &DiscardBook, key: &str) -> bool {
        if key.is_empty() {
            return false;
        }
        book.find(key).is_some()
    }

    /// 可取语义值的键（未被弃置面遮蔽）。
    ///
    /// 语义层必须走这个入口：它把"默认直取"改成"先问弃置面"，
    /// 零执行断言才不是一句空话。
    pub fn semantic(&self, book: &DiscardBook, key: &str) -> Option<&'static str> {
        if self.shadowed(book, key) {
            return None;
        }
        Some("default-ok")
    }

    /// 执行集（读出·渲染用）。
    pub fn executed(&self) -> &[String] {
        &self.executed
    }

    /// 执行总数。
    pub fn exec_count(&self) -> u64 {
        self.exec_n
    }

    /// 当前 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }
}

// --------------------------------------------------------------------------
// 弃置簿
// --------------------------------------------------------------------------

/// 弃置簿（16 槽定容·append-only 语义）。
#[derive(Clone, Debug)]
pub struct DiscardBook {
    slots: Vec<Slot>,
    initialized: bool,
    overflowed: bool,
    /// 累计收到未知键次数（含去重命中）。
    seen: u64,
    /// 上一条弃置时点（单调锚）。
    last_tick: u64,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
    /// 源缓冲长度（位置校验基准）。
    src_len: u32,
    /// 全簿指纹。
    pub fp: u64,
}

impl DiscardBook {
    /// 造簿（16 空槽）。
    pub fn new_discard_book() -> DiscardBook {
        DiscardBook {
            slots: alloc::vec![Slot::empty(); SLOTS],
            initialized: false,
            overflowed: false,
            seen: 0,
            last_tick: 0,
            stats: [0; 12],
            src_len: 0,
            fp: 0,
        }
    }

    /// 开簿（记源缓冲长度·启用记账）。
    pub fn open(&mut self, src_len: u32) -> Result<(), U584Code> {
        self.src_len = src_len;
        self.initialized = true;
        self.fp = self.compute_fp();
        Ok(())
    }

    /// 是否已开簿。
    pub fn is_open(&self) -> bool {
        self.initialized
    }

    /// 立案计数自增。
    fn bump(&mut self, c: U584Code) {
        let i = (c.val() - U584_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: U584Code) -> u64 {
        let i = (c.val() - U584_SEG_BASE) as usize;
        match self.stats.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 重算全簿指纹（纯查询·不改簿）。
    pub fn compute_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for s in self.slots.iter() {
            h ^= slot_fp(s);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= self.overflowed as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^ self.seen
    }

    /// 簿指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 指纹自检（外部改槽后应检出）。
    pub fn fp_intact(&self) -> bool {
        self.fp == self.compute_fp()
    }

    /// 清单槽（供外部误用演示"非清槽覆盖"的后果）。
    pub fn clear_slot(&mut self, idx: usize) {
        if let Some(s) = self.slots.get_mut(idx) {
            *s = Slot::empty();
        }
    }

    /// 查键是否已入簿（去重用）。
    pub fn find(&self, key: &str) -> Option<usize> {
        self.slots.iter().position(|s| s.raw == key)
    }

    /// 有效条数（槽内`raw` 非空）。
    pub fn live(&self) -> usize {
        self.slots.iter().filter(|s| !s.is_empty()).count()
    }

    /// 只读槽视图（判据与渲染用）。
    pub fn slots_ref(&self) -> &[Slot] {
        &self.slots
    }

    /// 首次空槽下标（满则 `None`）。
    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_empty())
    }

    /// 弃置一个未知键（核心写入口）。
    ///
    /// 参数口径：`raw` 是键原文，`off`/`len` 是它在源缓冲里的位置，
    /// `at_tick` 是时点。返回 `Ok(true)`=新入簿，`Ok(false)`=去重合并，
    /// `Err` 是显性拒（不留痕就拒，不静默丢）。
    pub fn discard(
        &mut self,
        raw: &str,
        off: u32,
        len: u32,
        at_tick: u64,
        phase: Phase,
    ) -> Result<bool, U584Code> {
        if !self.initialized {
            self.bump(U584Code::NotInit);
            return Err(U584Code::NotInit);
        }
        if raw.is_empty() {
            self.bump(U584Code::EmptyRaw);
            return Err(U584Code::EmptyRaw);
        }
        // 位置校验：越界即拒——记一个查不回源缓冲的位置等于没记。
        let end = match off.checked_add(len) {
            Some(e) => e,
            None => {
                self.bump(U584Code::PosRange);
                return Err(U584Code::PosRange);
            }
        };
        if end > self.src_len {
            self.bump(U584Code::PosRange);
            return Err(U584Code::PosRange);
        }
        // 时点单调：倒退即拒（乱序账无法解释因果）。
        if at_tick < self.last_tick {
            self.bump(U584Code::TimeBack);
            return Err(U584Code::TimeBack);
        }
        self.last_tick = at_tick;
        self.seen = self.seen.wrapping_add(1);

        // 去重：同键合并，不占新槽，但把"又见一次"这件事也留下。
        if let Some(idx) = self.find(raw) {
            if let Some(s) = self.slots.get_mut(idx) {
                s.merged = s.merged.wrapping_add(1);
                s.at_tick = at_tick;
            }
            self.bump(U584Code::DupKey);
            self.fp = self.compute_fp();
            return Ok(false);
        }

        // 槽满：显性溢出 + 整体拒（不覆盖最旧的）。
        let idx = match self.free_slot() {
            Some(i) => i,
            None => {
                self.overflowed = true;
                self.bump(U584Code::Overflowed);
                self.fp = self.compute_fp();
                return Err(U584Code::Overflowed);
            }
        };

        // 原文截断到 RAW_CAP，且必须落在 UTF-8 边界上。
        let bytes = raw.as_bytes();
        let cut = utf8_safe_len(bytes, RAW_CAP);
        let truncated = cut < bytes.len();
        if truncated {
            self.bump(U584Code::RawTruncated);
            if would_split(bytes, RAW_CAP) {
                self.bump(U584Code::RawSplit);
            }
        }
        // 存回校验过的长度：截断后的字符串长度不是原长。
        let kept = String::from_utf8_lossy(&bytes[..cut]).into_owned();
        let kept_len = kept.len() as u32;
        let mut fp: u64 = 0xcbf2_9ce4_8422_2325;
        for b in kept.as_bytes() {
            fp ^= *b as u64;
            fp = fp.wrapping_mul(0x0000_0100_0000_01b3);
        }

        let s = Slot {
            raw: kept,
            truncated,
            at_tick,
            phase,
            off,
            len,
            raw_fp: fp,
            merged: 0,
        };
        // 写前必清槽：非空槽直接写会把两笔账混在一起。
        let dirty = match self.slots.get(idx) {
            Some(c) => !c.is_empty(),
            None => true,
        };
        if dirty {
            self.bump(U584Code::SlotDirty);
        }
        if let Some(cell) = self.slots.get_mut(idx) {
            *cell = s;
        }
        self.fp = self.compute_fp();
        Ok(true)
    }

    /// 整体判定：账齐且未溢出才算通过（fail-closed）。
    pub fn verdict(&self) -> bool {
        self.initialized && !self.overflowed && self.fp_intact() && self.conserved()
    }

    /// 弃置率（`有效条数 / 见键次数`，整数 ppm·零浮点）。
    pub fn discard_rate_ppm(&self) -> u64 {
        if self.seen == 0 {
            return 0;
        }
        (self.live() as u64).saturating_mul(1_000_000) / self.seen
    }

    /// 三件套齐率（ppm·判据"账齐"的量化口径）。
    pub fn triad_rate_ppm(&self) -> u64 {
        let live = self.live();
        if live == 0 {
            return 0;
        }
        let ok = self
            .slots
            .iter()
            .filter(|s| !s.is_empty() && s.triad_complete() && s.within(self.src_len))
            .count();
        (ok as u64).saturating_mul(1_000_000) / (live as u64)
    }

    /// 守恒恒等式：有效条数 ≤ 已见次数，且有效条数 ≤ 槽数。
    pub fn conserved(&self) -> bool {
        let l = self.live() as u64;
        l <= self.seen && l <= SLOTS as u64
    }

    /// 零执行断言：簿内每键都不得出现在执行集里。
    ///
    /// 这是判据"弃置率 100%（零执行断言）"的落点。检查方向是
    /// **簿 → 执行集**：对簿内每一条，去执行集里找同键；找到即破。
    pub fn zero_exec_ok(&self, zx: &ZeroExec) -> bool {
        for s in self.slots.iter() {
            if s.is_empty() {
                continue;
            }
            if zx.executed().iter().any(|k| k == &s.raw) {
                return false;
            }
        }
        true
    }

    /// 破零执行断言的键（无则空 Vec）。
    pub fn leaked_keys(&self, zx: &ZeroExec) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for s in self.slots.iter() {
            if s.is_empty() {
                continue;
            }
            if zx.executed().iter().any(|k| k == &s.raw) {
                v.push(s.raw.clone());
            }
        }
        v
    }

    /// 显式破零执行断言（用于判据制造失败样本）。
    pub fn force_leak(&mut self, zx: &mut ZeroExec) -> bool {
        let first = match self.slots.iter().find(|s| !s.is_empty()) {
            Some(s) => s.raw.clone(),
            None => return false,
        };
        zx.exec(&first)
    }

    /// 按阶段分组计数（渲染/趋势用）。
    pub fn by_phase(&self) -> [u64; 3] {
        let mut out = [0u64; 3];
        for s in self.slots.iter() {
            if s.is_empty() {
                continue;
            }
            let i = s.phase.ord() as usize;
            if let Some(c) = out.get_mut(i) {
                *c = c.wrapping_add(1);
            }
        }
        out
    }

    /// 阶段倒退告警数（同一簿里出现阶段逆序·提示参数面跨阶段复用）。
    pub fn phase_regressions(&self) -> u64 {
        let mut n = 0u64;
        let mut last = 0u8;
        let mut first = true;
        for s in self.slots.iter() {
            if s.is_empty() {
                continue;
            }
            let o = s.phase.ord();
            if !first && o < last {
                n = n.wrapping_add(1);
            }
            last = o;
            first = false;
        }
        n
    }

    /// 渲染（看板真源·零浮点）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "unknown-params open={} seen={} live={} overflowed={} seg=0x{:04X}\n",
            self.initialized,
            self.seen,
            self.live(),
            self.overflowed,
            U584_SEG_BASE
        ));
        s.push_str(&format!(
            "rate discard_ppm={} triad_ppm={} rate_ok={} conserved={}\n",
            self.discard_rate_ppm(),
            self.triad_rate_ppm(),
            self.discard_rate_ppm() == 1_000_000,
            self.conserved()
        ));
        s.push_str("idx phase      tick       off      len  trunc mrg  raw\n");
        for (i, sl) in self.slots.iter().enumerate() {
            if sl.is_empty() {
                continue;
            }
            s.push_str(&format!(
                "{:<3} {:<10} {:<9} {:<8} {:<4} {:<5} {:<3}  {}\n",
                i,
                sl.phase.name(),
                sl.at_tick,
                sl.off,
                sl.len,
                sl.truncated,
                sl.merged,
                sl.raw
            ));
        }
        s.push_str(&format!(
            "book fp=0x{:016X} intact={} seg_ok={}\n",
            self.fp,
            self.fp_intact(),
            segment_guard(U584Code::Leaked.val())
        ));
        s
    }
}

// --------------------------------------------------------------------------
// 判据（12 条）
// --------------------------------------------------------------------------

/// UNX-F0584 判据全集（12 条）。
pub fn run_unx584_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx584");

    // ---- F584-01 诊断段闭集与建议完备 ----
    let seg_ok = U584Code::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == U584_SEG_BASE + i as u16)
        && U584Code::ALL.iter().all(|c| segment_guard(c.val()));
    let remedy_ok = U584Code::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add("F584-01 诊断段闭集", seg_ok && remedy_ok, "12 码连续落段+建议完备");

    // ---- F584-02 未开簿拒记（无簿不记账） ----
    let mut k2 = DiscardBook::new_discard_book();
    let e2 = k2.discard("timeoutnanos", 0, 13, 10, Phase::Boot);
    cs.add(
        "F584-02 未开簿拒",
        e2 == Err(U584Code::NotInit) && k2.live() == 0 && !k2.is_open(),
        "构造前弃置显性拒且不落痕",
    );

    // ---- F584-03 三件套齐（原文/时点/位置） ----
    let mut k3 = DiscardBook::new_discard_book();
    k3.open(256);
    let r3 = k3.discard("timeoutnanos", 16, 13, 100, Phase::Boot);
    let triad = k3.slots_ref();
    let t3 = triad
        .first()
        .map(|s| s.raw == "timeoutnanos" && s.at_tick == 100 && s.span() == Some((16, 29)))
        .unwrap_or(false);
    cs.add(
        "F584-03 三件套齐",
        r3 == Ok(true) && t3 && k3.triad_rate_ppm() == 1_000_000,
        "原文+时点+位置全留痕且齐率 100%",
    );

    // ---- F584-04 弃置率 100%（seen == live） ----
    let mut k4 = DiscardBook::new_discard_book();
    k4.open(1024);
    for i in 0..8u32 {
        let key = format!("unknown_{}", i);
        let _ = k4.discard(&key, i * 16, 10, 200 + i as u64, Phase::Boot);
    }
    cs.add(
        "F584-04 弃置率100",
        k4.live() == 8 && k4.seen == 8 && k4.discard_rate_ppm() == 1_000_000,
        "8 见 8 记·弃置率百万分位满",
    );

    // ---- F584-05 零执行断言（记账者未被执行） ----
    let mut k5 = DiscardBook::new_discard_book();
    k5.open(512);
    let mut zx5 = ZeroExec::new();
    let _ = zx5.tick_to(0);
    for i in 0..6u32 {
        let key = format!("typo_{}", i);
        let _ = k5.discard(&key, i * 8, 6, 300 + i as u64, Phase::Kernel);
    }
    // 正常参数执行，与弃置键不相交。
    for i in 0..4u32 {
        let _ = zx5.exec(&format!("good_{}", i));
    }
    let leak5 = k5.leaked_keys(&zx5);
    cs.add(
        "F584-05 零执行断言",
        k5.zero_exec_ok(&zx5) && leak5.is_empty() && zx5.exec_count() == 4,
        "弃置键零执行·不相交",
    );

    // ---- F584-06 破零执行断言必被检出 ----
    let mut k6 = DiscardBook::new_discard_book();
    k6.open(512);
    let _ = k6.discard("quietlevel", 0, 10, 400, Phase::Cmdline);
    let mut zx6 = ZeroExec::new();
    let leaked = k6.force_leak(&mut zx6);
    cs.add(
        "F584-06 断言破检出",
        leaked && !k6.zero_exec_ok(&zx6) && k6.leaked_keys(&zx6).len() == 1,
        "弃置键被执行即破且可定位",
    );

    // ---- F584-07 槽满显性溢出（不覆盖） ----
    let mut k7 = DiscardBook::new_discard_book();
    k7.open(4096);
    for i in 0..SLOTS as u32 {
        let key = format!("k{}", i);
        let _ = k7.discard(&key, i * 8, 4, 500 + i as u64, Phase::Boot);
    }
    let before7 = k7.live();
    let e7 = k7.discard("overflowkey", 900, 11, 900, Phase::Boot);
    cs.add(
        "F584-07 槽满拒",
        before7 == SLOTS
            && e7 == Err(U584Code::Overflowed)
            && k7.live() == SLOTS
            && !k7.verdict(),
        "第 17 键显性拒且不覆盖旧槽",
    );

    // ---- F584-08 位置越界与空键拒 ----
    let mut k8 = DiscardBook::new_discard_book();
    k8.open(64);
    let e_p = k8.discard("k", 60, 10, 600, Phase::Boot);
    let e_o = k8.discard("k", u32::MAX, 4, 601, Phase::Boot);
    let e_e = k8.discard("", 0, 0, 602, Phase::Boot);
    cs.add(
        "F584-08 位置空键拒",
        e_p == Err(U584Code::PosRange)
            && e_o == Err(U584Code::PosRange)
            && e_e == Err(U584Code::EmptyRaw)
            && k8.live() == 0,
        "越界/溢出/空键均拒且不留痕",
    );

    // ---- F584-09 时点倒退拒 ----
    let mut k9 = DiscardBook::new_discard_book();
    k9.open(256);
    let _ = k9.discard("first", 0, 5, 700, Phase::Boot);
    let e9 = k9.discard("second", 8, 6, 600, Phase::Boot);
    cs.add(
        "F584-09 时点倒退拒",
        e9 == Err(U584Code::TimeBack) && k9.live() == 1,
        "tick 倒退显性拒·账不乱序",
    );

    // ---- F584-10 去重合并（同键只占一槽·合并计数留痕） ----
    let mut k10 = DiscardBook::new_discard_book();
    k10.open(256);
    let a = k10.discard("dupkey", 0, 6, 800, Phase::Boot);
    let b = k10.discard("dupkey", 32, 6, 801, Phase::Cmdline);
    let c = k10.discard("otherkey", 40, 8, 802, Phase::Cmdline);
    let merged10 = k10.slots_ref().first().map(|s| s.merged).unwrap_or(0);
    cs.add(
        "F584-10 去重合并",
        a == Ok(true) && b == Ok(false) && c == Ok(true) && k10.live() == 2 && merged10 == 1,
        "同键并槽+合并次数留痕",
    );

    // ---- F584-11 原文超长截断且落UTF-8 边界 ----
    let long = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ0123456789";
    let multi = "键参数名超长中文测试内容一二三四五六七八九十甲乙丙丁戊己";
    let mut k11 = DiscardBook::new_discard_book();
    k11.open(512);
    let r11 = k11.discard(long, 0, 60, 900, Phase::Boot);
    let r11b = k11.discard(multi, 64, 64, 901, Phase::Boot);
    let rows = k11.slots_ref();
    let kept = k11.live();
    let all_utf8 = rows.iter().all(|s| s.is_empty() || std_ok(&s.raw));
    let trunc_seen = rows.iter().filter(|s| s.truncated).count();
    cs.add(
        "F584-11 原文截断",
        r11 == Ok(true) && r11b == Ok(true) && kept == 2 && trunc_seen == 2 && all_utf8,
        "超长截断到合法 UTF-8 边界且标 truncated",
    );

    // ---- F584-12 阶段闭集 + 指纹链式敏感 + 渲染完备 ----
    let mut k12 = DiscardBook::new_discard_book();
    k12.open(512);
    for (i, p) in Phase::ALL.iter().enumerate() {
        let key = format!("ph{}", i);
        let _ = k12.discard(&key, i as u32 * 8, 3, 1000 + i as u64, *p);
    }
    let fp_a = k12.fingerprint();
    let mut k12b = k12.clone();
    k12b.clear_slot(0);
    let fp_b = k12b.compute_fp();
    let txt = k12.render();
    let render_ok = txt.contains("unknown-params") && txt.contains("rate ")
        && txt.contains("book fp=")
        && txt.contains("boot");
    let by_phase = k12.by_phase();
    let chain = fp_a != fp_b && !k12b.fp_intact();
    cs.add(
        "F584-12 阶段与指纹链",
        render_ok
            && chain
            && by_phase == [1, 1, 1]
            && k12.phase_regressions() == 0
            && Phase::from_ord(3).is_none()
            && Phase::from_ord(0) == Some(Phase::Boot)
            && !phase_regress(Phase::Cmdline, Phase::Boot)
            && phase_regress(Phase::Boot, Phase::Kernel),
        "三阶段各一+指纹对清槽敏感+阶段序可判",
    );

    cs
}

/// 无 std 依赖的 UTF-8 有效性校验（自写最小实现·零依赖）。
fn std_ok(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c < 0x80 {
            i += 1;
            continue;
        }
        let need = if (c & 0xE0) == 0xC0 {
            2usize
        } else if (c & 0xF0) == 0xE0 {
            3usize
        } else if (c & 0xF8) == 0xF0 {
            4usize
        } else {
            return false;
        };
        if i + need > b.len() {
            return false;
        }
        for k in 1..need {
            if (b[i + k] & 0xC0) != 0x80 {
                return false;
            }
        }
        i += need;
    }
    true
}

/// 未知参数账注锚点串（跨批引用防漂移）。
#[must_use]
pub fn u584_anchor() -> &'static str {
    U584_VERSION
}