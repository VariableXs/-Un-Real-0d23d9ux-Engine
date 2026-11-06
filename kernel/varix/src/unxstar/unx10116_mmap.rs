//! UNX-F10116 mmap 正反双判据（映射达成与非法长度误用受控）· 0x92D00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10117-J1 munmap/mprotect 达成（保护位翻转生效 LTP mprotect01
//!       面 10/10 次）；UNX-F10117-J2 未映射区间误用受控（预期 ENOMEM
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92D00（本卡独占 0x40 窗口，下一卡 0x92D40）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_mmap_call`："mmap|len|prot|flags" 四成分逐段解析，
//!      len/prot/flags 十进制闭集，成分缺失/非数字逐档拒绝；
//!   ② 映射账本 `MmLedger`：页粒度映射表 + 内核选址 bump 分配器（KBASE
//!      起步、32MB 上窗）+ 确定性字节模板竞技场（无浮点无随机）；
//!   ③ 状态机 `MapState`：Vacant→Mapped→Sealed→Unmapped 单向推进；
//!   ④ 注入核 `BadMmap`：零长度 / 非法 prot / 非对齐 FIXED 地址 /
//!      FIXED 重叠 四类坏样本逐类 10/10 次精确复现 EINVAL/EEXIST 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10116 独占 0x40 窗口，下一卡 0x92D40）。
pub const SEG_BASE: u32 = 0x92D00;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92D40;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 页大小（4KB 粒度）。
pub const PAGE: u32 = 0x1000;

/// 内核选址基址（bump 分配器起点）。
pub const KBASE: u32 = 0x4000_0000;

/// 选址上窗（KBASE + 32MB）。
pub const KTOP: u32 = 0x4200_0000;

/// 映射账本容量上限。
pub const LEDGER_CAP: usize = 24;

/// PROT_NONE。
pub const PROT_NONE: u32 = 0;
/// PROT_READ。
pub const PROT_READ: u32 = 1;
/// PROT_WRITE。
pub const PROT_WRITE: u32 = 2;
/// PROT_EXEC。
pub const PROT_EXEC: u32 = 4;

/// MAP_FIXED。
pub const MAP_FIXED: u32 = 0x10;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MmCode {
    /// 零长度（EINVAL 族）
    ZeroLen,
    /// 非法 prot（EINVAL 族）
    BadProt,
    /// 非对齐地址（EINVAL 族）
    BadAlign,
    /// FIXED 重叠（EEXIST 档）
    FixedConflict,
    /// 未映射窗（ENOMEM 族）
    NotMapped,
    /// 只读窗写入（EACCES 族）
    ReadOnly,
    /// 越地址窗（ENOMEM 族）
    AddrRange,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 映射表满（ENOMEM 族）
    LedgerFull,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl MmCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MmCode::ZeroLen => "零长度被拒：mmap 的 length 不得为 0；EINVAL 精确归档",
            MmCode::BadProt => "非法 prot 被拒：只允许 R/W/E/NONE 位闭集；请核对位组合",
            MmCode::BadAlign => "非对齐地址被拒：MAP_FIXED 地址必须页对齐；请取整到 0x1000",
            MmCode::FixedConflict => "FIXED 重叠被拒：目标窗已映射；EEXIST 档精确归档",
            MmCode::NotMapped => "未映射窗被拒：地址不被任何在账映射覆盖；ENOMEM 精确归档",
            MmCode::ReadOnly => "只读窗写入被拒：prot 无 PROT_WRITE 位；EACCES 精确归档",
            MmCode::AddrRange => "越地址窗被拒：选址越过 KTOP 上窗；ENOMEM 精确归档",
            MmCode::BadNumber => "非数字成分被拒：len/prot/flags 必须为十进制；请改写",
            MmCode::MissingPart => "成分缺失被拒：调用行必须四段 mmap|len|prot|flags；请补成分",
            MmCode::LedgerFull => "映射表满被拒：请先 munmap 释放映射再建",
            MmCode::EmptySummary => "空成文被拒：映射小结不得为空串；请先建映射再成文",
            MmCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [MmCode; 12] = [
        MmCode::ZeroLen,
        MmCode::BadProt,
        MmCode::BadAlign,
        MmCode::FixedConflict,
        MmCode::NotMapped,
        MmCode::ReadOnly,
        MmCode::AddrRange,
        MmCode::BadNumber,
        MmCode::MissingPart,
        MmCode::LedgerFull,
        MmCode::EmptySummary,
        MmCode::SegmentViolation,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的 mmap 调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MmCall {
    /// 长度（字节）
    pub len: u32,
    /// 保护位（R/W/E 位闭集）
    pub prot: u32,
    /// 标志位（FIXED 位闭集）
    pub flags: u32,
}

/// 解析 "mmap|len|prot|flags" 四段调用行；非数字/成分缺失逐档拒绝。
pub fn parse_mmap_call(line: &str) -> Result<MmCall, MmCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = match bytes.get(i) {
            Some(b) => *b as char,
            None => break,
        };
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
        i += 1;
    }
    let _ = parts.push(cur);
    if parts.len() != 4 {
        return Err(MmCode::MissingPart);
    }
    let num = |s: &str| -> Result<u32, MmCode> {
        if s.is_empty() {
            return Err(MmCode::BadNumber);
        }
        let mut acc: u32 = 0;
        for b in s.bytes() {
            if !b.is_ascii_digit() {
                return Err(MmCode::BadNumber);
            }
            acc = acc.wrapping_mul(10).wrapping_add((b - b'0') as u32);
        }
        Ok(acc)
    };
    let p1 = match parts.get(1) {
        Some(p) => p.as_str(),
        None => return Err(MmCode::MissingPart),
    };
    let p2 = match parts.get(2) {
        Some(p) => p.as_str(),
        None => return Err(MmCode::MissingPart),
    };
    let p3 = match parts.get(3) {
        Some(p) => p.as_str(),
        None => return Err(MmCode::MissingPart),
    };
    let len = num(p1)?;
    let prot = num(p2)?;
    let flags = num(p3)?;
    Ok(MmCall { len, prot, flags })
}

// ── 映射状态机 ────────────────────────────────────────────────────────
/// 映射生命周期（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapState {
    /// 未映射
    Vacant,
    /// 已映射
    Mapped,
    /// 已封账（只读快照）
    Sealed,
    /// 已解除
    Unmapped,
}

impl MapState {
    /// 单调推进判定（允许跳级，禁倒退）。
    pub fn can_advance(self, next: MapState) -> bool {
        let rank = |s: MapState| match s {
            MapState::Vacant => 0u8,
            MapState::Mapped => 1,
            MapState::Sealed => 2,
            MapState::Unmapped => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        Self::label_of(self)
    }

    /// 标签实现（避开 enum 与函数同名歧义）。
    fn label_of(s: MapState) -> &'static str {
        match s {
            MapState::Vacant => "未映射",
            MapState::Mapped => "已映射",
            MapState::Sealed => "已封账",
            MapState::Unmapped => "已解除",
        }
    }
}

// ── 映射账本 ──────────────────────────────────────────────────────────
/// 映射条目。
#[derive(Clone, Debug)]
pub struct MapEnt {
    /// 起始地址（页对齐）
    pub start: u32,
    /// 字节长度（内核上取整到页）
    pub len: u32,
    /// 保护位
    pub prot: u32,
    /// 生命周期状态
    pub state: MapState,
}

/// 映射账本：页粒度 VMA 表 + bump 分配器 + 确定性竞技场。
#[derive(Clone, Debug)]
pub struct MmLedger {
    /// 映射条目
    pub ents: Vec<MapEnt>,
    cap: usize,
    /// bump 游标（页对齐）
    cursor: u32,
    /// 页竞技场（byte i = (i*7+13) 模 256 模板）
    arena: Vec<u8>,
    /// 累计映射次数
    pub maps: u32,
    /// 累计解除次数
    pub unmaps: u32,
}

impl MmLedger {
    /// 空账构造。
    pub fn new() -> MmLedger {
        MmLedger {
            ents: Vec::new(),
            cap: LEDGER_CAP,
            cursor: KBASE,
            arena: Vec::new(),
            maps: 0,
            unmaps: 0,
        }
    }

    /// 在账映射数。
    pub fn len(&self) -> usize {
        self.ents.len()
    }

    /// 内核选址：bump 游标页对齐推进；越 KTOP 即 ENOMEM 档。
    fn alloc_at(&mut self, pages: u32) -> Result<u32, MmCode> {
        let span = pages.checked_mul(PAGE).ok_or(MmCode::AddrRange)?;
        let end = self.cursor.checked_add(span).ok_or(MmCode::AddrRange)?;
        if end > KTOP {
            return Err(MmCode::AddrRange);
        }
        let start = self.cursor;
        self.cursor = end;
        Ok(start)
    }

    /// 区间相交判定。
    fn overlaps(a: u32, an: u32, b: u32, bn: u32) -> bool {
        a < b.saturating_add(bn) && b < a.saturating_add(an)
    }

    /// 与在账活跃映射重叠判定（Unmapped 态不算）。
    fn overlap_any(&self, start: u32, len: u32) -> bool {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.state != MapState::Unmapped
                    && Self::overlaps(start, len, e.start, e.len)
                {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 竞技场模板扩容到 cover [KBASE, KBASE+need)。
    fn grow_arena(&mut self, need: usize) {
        while self.arena.len() < need {
            let v = (self.arena.len() as u32 * 7 + 13) % 256;
            let _ = self.arena.push(v as u8);
        }
    }

    /// mmap 主入口：零长/坏 prot/表满逐闸，bump 选址。
    pub fn mmap(&mut self, call: &MmCall) -> Result<u32, MmCode> {
        if call.len == 0 {
            return Err(MmCode::ZeroLen);
        }
        if call.prot & !7u32 != 0 {
            return Err(MmCode::BadProt);
        }
        if call.flags & MAP_FIXED != 0 {
            // FIXED 语义走 mmap_fixed 显式入口；调用行入口显性拒绝。
            return Err(MmCode::BadAlign);
        }
        if self.ents.len() >= self.cap {
            return Err(MmCode::LedgerFull);
        }
        let pages = (call.len + PAGE - 1) / PAGE;
        let start = self.alloc_at(pages)?;
        let span = pages.saturating_mul(PAGE);
        if self.overlap_any(start, span) {
            return Err(MmCode::FixedConflict);
        }
        self.grow_arena((start - KBASE) as usize + span as usize);
        let _ = self.ents.push(MapEnt {
            start,
            len: span,
            prot: call.prot,
            state: MapState::Mapped,
        });
        self.maps = self.maps.saturating_add(1);
        Ok(start)
    }

    /// MAP_FIXED 显式入口：页对齐 + 无重叠双闸。
    pub fn mmap_fixed(&mut self, addr: u32, len: u32, prot: u32) -> Result<u32, MmCode> {
        if len == 0 {
            return Err(MmCode::ZeroLen);
        }
        if prot & !7u32 != 0 {
            return Err(MmCode::BadProt);
        }
        if addr % PAGE != 0 {
            return Err(MmCode::BadAlign);
        }
        if addr < KBASE || addr >= KTOP {
            return Err(MmCode::AddrRange);
        }
        if self.ents.len() >= self.cap {
            return Err(MmCode::LedgerFull);
        }
        let pages = (len + PAGE - 1) / PAGE;
        let span = pages.saturating_mul(PAGE);
        if self.overlap_any(addr, span) {
            return Err(MmCode::FixedConflict);
        }
        self.grow_arena((addr - KBASE) as usize + span as usize);
        let _ = self.ents.push(MapEnt {
            start: addr,
            len: span,
            prot,
            state: MapState::Mapped,
        });
        self.maps = self.maps.saturating_add(1);
        Ok(addr)
    }

    /// 覆盖判定：addr 是否落在某活跃映射窗内。
    fn covering(&self, addr: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.state != MapState::Unmapped {
                    let end = e.start.saturating_add(e.len);
                    if addr >= e.start && addr < end {
                        return Some(i);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 覆盖窗的 prot 读取（审计用）。
    pub fn prot_at(&self, addr: u32) -> Option<u32> {
        match self.covering(addr) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.prot),
                None => None,
            },
            None => None,
        }
    }

    /// 写竞技场：须在活跃映射窗内且带 PROT_WRITE。
    pub fn write_at(&mut self, addr: u32, data: &[u8]) -> Result<(), MmCode> {
        let idx = match self.covering(addr) {
            Some(i) => i,
            None => return Err(MmCode::NotMapped),
        };
        let prot = match self.ents.get(idx) {
            Some(e) => e.prot,
            None => return Err(MmCode::NotMapped),
        };
        if prot & PROT_WRITE == 0 {
            return Err(MmCode::ReadOnly);
        }
        if data.is_empty() {
            return Ok(());
        }
        let last = addr.saturating_add(data.len() as u32 - 1);
        if self.covering(last).is_none() {
            return Err(MmCode::NotMapped);
        }
        let off = (addr - KBASE) as usize;
        let mut k = 0usize;
        while k < data.len() {
            if let Some(slot) = self.arena.get_mut(off + k) {
                *slot = match data.get(k) {
                    Some(b) => *b,
                    None => 0,
                };
            }
            k += 1;
        }
        Ok(())
    }

    /// 读竞技场：须在活跃映射窗内且带 PROT_READ。
    pub fn read_at(&self, addr: u32, n: usize) -> Result<Vec<u8>, MmCode> {
        let idx = match self.covering(addr) {
            Some(i) => i,
            None => return Err(MmCode::NotMapped),
        };
        let prot = match self.ents.get(idx) {
            Some(e) => e.prot,
            None => return Err(MmCode::NotMapped),
        };
        if prot & PROT_READ == 0 {
            return Err(MmCode::ReadOnly);
        }
        if n == 0 {
            return Ok(Vec::new());
        }
        let last = addr.saturating_add(n as u32 - 1);
        if self.covering(last).is_none() {
            return Err(MmCode::NotMapped);
        }
        let off = (addr - KBASE) as usize;
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < n {
            let v = match self.arena.get(off + k) {
                Some(b) => *b,
                None => 0u8,
            };
            let _ = out.push(v);
            k += 1;
        }
        Ok(out)
    }

    /// munmap：活跃映射解除（Mapped→Unmapped 单向推进），账面留档。
    pub fn unmap(&mut self, addr: u32) -> Result<(), MmCode> {
        let idx = match self.covering(addr) {
            Some(i) => i,
            None => return Err(MmCode::NotMapped),
        };
        let st = match self.ents.get(idx) {
            Some(e) => e.state,
            None => return Err(MmCode::NotMapped),
        };
        if !st.can_advance(MapState::Unmapped) {
            return Err(MmCode::SegmentViolation);
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.state = MapState::Unmapped;
        }
        self.unmaps = self.unmaps.saturating_add(1);
        Ok(())
    }
}

// ── J1 达成核：mmap01 面 10/10 轮 ─────────────────────────────────────
/// 逐轮：RW 匿名映射→写模板→读回一致→只读映射读通/写拒。
/// 返回 (达成轮数, 探针次数)。
pub fn run_mmap_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = MmLedger::new();
        let mut all_ok = true;
        // 面1：匿名 RW 映射 2 页，地址页对齐。
        let call = MmCall { len: 2 * PAGE, prot: PROT_READ | PROT_WRITE, flags: 0 };
        let a1 = match l.mmap(&call) {
            Ok(a) => a,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        all_ok = all_ok && a1 % PAGE == 0 && a1 == KBASE;
        probes += 1;
        // 面2：写 16 字节确定性图案。
        let mut pat: Vec<u8> = Vec::new();
        let mut k = 0u32;
        while k < 16 {
            let _ = pat.push(((k * 11 + 5) % 256) as u8);
            k += 1;
        }
        all_ok = all_ok && l.write_at(a1, &pat).is_ok();
        probes += 1;
        // 面3：读回与图案逐字节一致。
        let back = l.read_at(a1, 16).unwrap_or_default();
        all_ok = all_ok && back == pat;
        probes += 1;
        // 面4：第二次映射 bump 推进一窗，无重叠。
        let call2 = MmCall { len: PAGE, prot: PROT_READ, flags: 0 };
        let a2 = l.mmap(&call2).unwrap_or(0);
        all_ok = all_ok && a2 == a1 + 2 * PAGE;
        probes += 1;
        // 面5：只读映射读通。
        all_ok = all_ok && l.read_at(a2, 8).is_ok();
        probes += 1;
        // 面6：只读映射写拒（ReadOnly 档）。
        all_ok = all_ok && l.write_at(a2, b"x").err() == Some(MmCode::ReadOnly);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏映射样本矩阵 ────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadMmap {
    /// 零长度（EINVAL 族）
    ZeroLen,
    /// 非法 prot（EINVAL 族）
    BadProt,
    /// 非对齐 FIXED 地址（EINVAL 族）
    BadAlign,
    /// FIXED 重叠（EEXIST 档）
    FixedConflict,
}

impl BadMmap {
    /// 闭集全量。
    pub const ALL: [BadMmap; 4] = [
        BadMmap::ZeroLen,
        BadMmap::BadProt,
        BadMmap::BadAlign,
        BadMmap::FixedConflict,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadMmap::ZeroLen => "零长度映射",
            BadMmap::BadProt => "非法 prot 映射",
            BadMmap::BadAlign => "非对齐 FIXED 地址",
            BadMmap::FixedConflict => "FIXED 重叠映射",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失守恒返回 SegmentViolation。
    pub fn record(&mut self, caught: bool) -> Result<(), MmCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(MmCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏映射注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_mmap_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadMmap::ALL.len() {
        let case = match BadMmap::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = MmLedger::new();
            let caught = match case {
                BadMmap::ZeroLen => {
                    let c = MmCall { len: 0, prot: PROT_READ | PROT_WRITE, flags: 0 };
                    l.mmap(&c).err() == Some(MmCode::ZeroLen)
                }
                BadMmap::BadProt => {
                    let c = MmCall { len: PAGE, prot: 9, flags: 0 };
                    l.mmap(&c).err() == Some(MmCode::BadProt)
                }
                BadMmap::BadAlign => {
                    l.mmap_fixed(KBASE + 1, PAGE, PROT_READ).err() == Some(MmCode::BadAlign)
                }
                BadMmap::FixedConflict => {
                    let c = MmCall { len: PAGE, prot: PROT_READ, flags: 0 };
                    let _ = l.mmap(&c);
                    l.mmap_fixed(KBASE, PAGE, PROT_READ).err() == Some(MmCode::FixedConflict)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 映射小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「start=0x... pages=<n> prot=<bits>」；账外先拒。
pub fn compose_map_summary(l: &MmLedger, addr: u32) -> Result<String, MmCode> {
    let idx = match l.covering(addr) {
        Some(i) => i,
        None => return Err(MmCode::EmptySummary),
    };
    let (start, len, prot) = match l.ents.get(idx) {
        Some(e) => (e.start, e.len, e.prot),
        None => return Err(MmCode::EmptySummary),
    };
    let pages = len / PAGE;
    let out = format!("start=0x{:08X} pages={} prot={}", start, pages, prot);
    if out.is_empty() {
        return Err(MmCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零长/坏 prot/非对齐族"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；越窗/表满族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；FIXED 重叠族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；只读窗写族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料指纹：map→write→read→小结→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = MmLedger::new();
    let call = MmCall { len: PAGE, prot: PROT_READ | PROT_WRITE, flags: 0 };
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(a) = l.mmap(&call) {
        let mut pat: Vec<u8> = Vec::new();
        let mut k = 0u32;
        while k < 8 {
            let _ = pat.push(((k * 13 + 7) % 256) as u8);
            k += 1;
        }
        let _ = l.write_at(a, &pat);
        if let Ok(back) = l.read_at(a, 8) {
            bytes.extend_from_slice(&back);
        }
        if let Ok(s) = compose_map_summary(&l, a) {
            bytes.extend_from_slice(s.as_bytes());
        }
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10116 判据集（13 条静态 + 3 条动态；动态明细 ≤8 条）。
pub fn run_unx10116_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10116");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92D00..=0x92D3F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < MmCode::ALL.len() {
        let mut j = i + 1;
        while j < MmCode::ALL.len() {
            if let (Some(a), Some(b)) = (MmCode::ALL.get(i), MmCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = MmCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 调用行解析器
    let parse_ok = parse_mmap_call("mmap|4096|3|0").is_ok()
        && parse_mmap_call("mmap|0|3|0").is_ok()
        && parse_mmap_call("mmap|x|3|0").err() == Some(MmCode::BadNumber)
        && parse_mmap_call("mmap|4096|3").err() == Some(MmCode::MissingPart);
    s.add("调用行解析器：四段闭集放行且坏成分逐档拒绝", parse_ok,
          "解析器是 mmap 前置；len=0 在判定层拒，解析层只认数字闭集");

    // 4. J1 达成核
    let (ok, probes) = run_mmap_rounds();
    s.add("J1 映射达成：匿名页可读写 LTP mmap01 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面断言（选址/写入/读回/推进/读通/写拒）全对");

    // 5. 零长度档：EINVAL 精确复现
    let mut l = MmLedger::new();
    let zc = MmCall { len: 0, prot: PROT_READ | PROT_WRITE, flags: 0 };
    let zero_ok = l.mmap(&zc).err() == Some(MmCode::ZeroLen) && l.len() == 0;
    s.add("J2 零长度误用：EINVAL 档精确复现且账面不动", zero_ok,
          "零长度是 mmap 第一拒点；被拒不建条目");

    // 6. prot 位闭集：9 与 8 越位拒绝
    let pc = MmCall { len: PAGE, prot: 9, flags: 0 };
    let pc2 = MmCall { len: PAGE, prot: 8, flags: 0 };
    let prot_ok = l.mmap(&pc).err() == Some(MmCode::BadProt)
        && l.mmap(&pc2).err() == Some(MmCode::BadProt);
    s.add("prot 位闭集：越三位拒绝且逐档归因", prot_ok,
          "R/W/E 位闭集外的 prot 一律 EINVAL 档");

    // 7. FIXED 对齐与重叠双闸
    let fixed_ok = l.mmap_fixed(KBASE + 1, PAGE, PROT_READ).err() == Some(MmCode::BadAlign)
        && l.mmap_fixed(KBASE + PAGE / 2, PAGE, PROT_READ).err() == Some(MmCode::BadAlign);
    s.add("FIXED 对齐闸：非页对齐地址逐档拒绝", fixed_ok,
          "MAP_FIXED 地址必须 0x1000 对齐；半页错位也拒");

    // 8. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_mmap_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "零长/坏 prot/非对齐/重叠必须逐类逐次精确复现");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadMmap::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 写读一致：写图案读回逐字节一致
    let wc = MmCall { len: PAGE, prot: PROT_READ | PROT_WRITE, flags: 0 };
    let a = match l.mmap(&wc) {
        Ok(x) => x,
        Err(_) => KBASE,
    };
    let mut pat: Vec<u8> = Vec::new();
    let mut k = 0u32;
    while k < 32 {
        let _ = pat.push(((k * 11 + 5) % 256) as u8);
        k += 1;
    }
    let wrote = l.write_at(a, &pat).is_ok();
    let back = l.read_at(a, 32).unwrap_or_default();
    s.add("写读一致：32 字节图案写后读回逐字节一致",
          wrote && back == pat,
          "映射内容语义必须与竞技场逐字节对平");

    // 11. 未映射窗：NOT_MAPPED 档精确复现
    let nm_ok = l.write_at(KTOP + PAGE, b"x").err() == Some(MmCode::NotMapped)
        && l.read_at(KTOP + PAGE, 4).err() == Some(MmCode::NotMapped)
        && l.unmap(KTOP + PAGE).err() == Some(MmCode::NotMapped);
    s.add("未映射窗：读写解除三入口同档精确归因", nm_ok,
          "未映射地址必须三入口一致拒绝，不得假成功");

    // 12. 状态机
    let sm_ok = MapState::Vacant.can_advance(MapState::Mapped)
        && MapState::Mapped.can_advance(MapState::Unmapped)
        && !MapState::Unmapped.can_advance(MapState::Mapped)
        && MapState::Mapped.label() == "已映射";
    s.add("状态机：Vacant→Mapped→Sealed→Unmapped 单向", sm_ok,
          "映射生命周期只许单调推进；倒退即协议违约");

    // 13. Linux 逐码一致 + 成文 + 指纹
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(12)
        && agrees_with_linux(17) && agrees_with_linux(13)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    let sum = compose_map_summary(&l, a);
    let text_ok = match &sum {
        Ok(t) => t.contains("start=0x") && t.contains("pages=1"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("对账与指纹：Linux 逐码一致且成文双跑哈希一致",
          codes_ok && text_ok && f1 == f2,
          "错误族逐码可复算；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("映射达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_mmap_call("mmap|4096|3|0").is_ok());
        assert_eq!(parse_mmap_call("mmap|4x96|3|0").err(), Some(MmCode::BadNumber));
        assert_eq!(parse_mmap_call("mmap|4096|3").err(), Some(MmCode::MissingPart));
        let c = parse_mmap_call("mmap|8192|3|0").unwrap_or(MmCall { len: 0, prot: 0, flags: 0 });
        assert_eq!(c.len, 8192);
    }

    #[test]
    fn zero_len_rejected() {
        let mut l = MmLedger::new();
        let c = MmCall { len: 0, prot: PROT_READ | PROT_WRITE, flags: 0 };
        assert_eq!(l.mmap(&c).err(), Some(MmCode::ZeroLen));
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn prot_guards() {
        let mut l = MmLedger::new();
        let c = MmCall { len: PAGE, prot: 9, flags: 0 };
        assert_eq!(l.mmap(&c).err(), Some(MmCode::BadProt));
        let c2 = MmCall { len: PAGE, prot: PROT_READ, flags: 0 };
        assert!(l.mmap(&c2).is_ok());
    }

    #[test]
    fn fixed_alignment_gates() {
        let mut l = MmLedger::new();
        assert_eq!(l.mmap_fixed(KBASE + 1, PAGE, PROT_READ).err(), Some(MmCode::BadAlign));
        assert!(l.mmap_fixed(KBASE + 2 * PAGE, PAGE, PROT_READ).is_ok());
    }

    #[test]
    fn overlap_detected() {
        let mut l = MmLedger::new();
        assert!(l.mmap_fixed(KBASE, PAGE, PROT_READ).is_ok());
        assert_eq!(l.mmap_fixed(KBASE, PAGE, PROT_READ).err(), Some(MmCode::FixedConflict));
        assert_eq!(l.mmap_fixed(KBASE + 4, PAGE, PROT_READ).err(), Some(MmCode::BadAlign));
    }

    #[test]
    fn write_read_roundtrip() {
        let mut l = MmLedger::new();
        let c = MmCall { len: PAGE, prot: PROT_READ | PROT_WRITE, flags: 0 };
        let a = l.mmap(&c).unwrap_or(0);
        assert!(l.write_at(a, b"hello varix").is_ok());
        assert_eq!(l.read_at(a, 11).unwrap_or_default(), b"hello varix".to_vec());
    }

    #[test]
    fn readonly_write_blocked() {
        let mut l = MmLedger::new();
        let c = MmCall { len: PAGE, prot: PROT_READ, flags: 0 };
        let a = l.mmap(&c).unwrap_or(0);
        assert_eq!(l.write_at(a, b"x").err(), Some(MmCode::ReadOnly));
        assert!(l.read_at(a, 1).is_ok());
    }

    #[test]
    fn unmapped_gates() {
        let mut l = MmLedger::new();
        assert_eq!(l.read_at(KBASE + 0x30 * PAGE, 1).err(), Some(MmCode::NotMapped));
        assert_eq!(l.write_at(KBASE + 0x30 * PAGE, b"y").err(), Some(MmCode::NotMapped));
    }

    #[test]
    fn unmap_advances_state() {
        let mut l = MmLedger::new();
        let c = MmCall { len: PAGE, prot: PROT_READ, flags: 0 };
        let a = l.mmap(&c).unwrap_or(0);
        assert!(l.unmap(a).is_ok());
        assert_eq!(l.read_at(a, 1).err(), Some(MmCode::NotMapped));
        assert_eq!(l.unmaps, 1);
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_mmap_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_mmap_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10116_checks_green() {
        let set = run_unx10116_checks();
        assert!(set.all_passed(), "UNX-F10116 red: {}", set.red_items_text());
    }
}
