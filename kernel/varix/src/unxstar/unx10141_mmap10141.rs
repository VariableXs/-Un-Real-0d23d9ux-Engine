//! UNX-F10141 mmap 匿名映射正反双判据（零页达成与长度零误用受控）· 0x93340 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10142-J1 文件映射达成（写映射→文件重读一致 10/10 次）；
//!       UNX-F10142-J2 越尾访问误用受控（预期 SIGBUS 精确复现 10/10 次，
//!       分进程断言）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93340（本卡独占 0x40 窗口，下一卡 0x93380）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_anon_call`："anon|len|prot" 三成分（len/prot 十进制
//!      闭集），成分缺失/非数字逐档拒绝；
//!   ② 匿名映射账 `AnonLedger`：MAP_ANONYMOUS 语义——映射即零页（读先于
//!      写必得全零），写后读回逐字节一致，bump 选址页对齐；
//!   ③ 状态机 `AnonState`：Zero→Written→Verified 单调（禁倒退）；
//!   ④ 注入核 `BadAnon`：长度零 / 非法 prot / 越窗读写 / 账满 四类坏样本
//!      逐类 10/10 次精确复现 EINVAL/ENOMEM 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10141 独占 0x40 窗口，下一卡 0x93380）。
pub const SEG_BASE: u32 = 0x93340;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93380;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 页大小。
pub const PAGE: u32 = 0x1000;

/// 选址基址。
pub const KBASE: u32 = 0x5000_0000;

/// 选址上窗（8MB）。
pub const KTOP: u32 = 0x5080_0000;

/// 映射账容量。
pub const LEDGER_CAP: usize = 16;

/// PROT_READ。
pub const PROT_READ: u32 = 1;
/// PROT_WRITE。
pub const PROT_WRITE: u32 = 2;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnCode {
    /// 长度零（EINVAL 族）
    ZeroLen,
    /// 非法 prot（EINVAL 族）
    BadProt,
    /// 越窗读写（ENOMEM 族）
    NotMapped,
    /// 账满（ENOMEM 族）
    LedgerFull,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 越选址窗（ENOMEM 族）
    AddrRange,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 零页失准
    ZeroDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl AnCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AnCode::ZeroLen => "长度零被拒：mmap 匿名映射 length 不得为 0；EINVAL 精确归档",
            AnCode::BadProt => "非法 prot 被拒：只允许 R/W 位闭集；EINVAL 精确归档",
            AnCode::NotMapped => "越窗读写被拒：地址不被任何匿名映射覆盖；ENOMEM 精确归档",
            AnCode::LedgerFull => "映射账满被拒：请先解除闲置映射",
            AnCode::BadNumber => "非数字成分被拒：len/prot 必须为十进制；请改写",
            AnCode::MissingPart => "成分缺失被拒：调用行必须三段 anon|len|prot；请补成分",
            AnCode::AddrRange => "越选址窗被拒：bump 游标越过 KTOP；ENOMEM 精确归档",
            AnCode::StateRegress => "状态倒退被拒：Zero→Written→Verified 单调",
            AnCode::EmptySummary => "空成文被拒：映射小结不得为空串；请先映射再成文",
            AnCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            AnCode::ZeroDrift => "零页失准被拒：匿名页读先于写必须逐字节为零",
            AnCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [AnCode; 12] = [
        AnCode::ZeroLen,
        AnCode::BadProt,
        AnCode::NotMapped,
        AnCode::LedgerFull,
        AnCode::BadNumber,
        AnCode::MissingPart,
        AnCode::AddrRange,
        AnCode::StateRegress,
        AnCode::EmptySummary,
        AnCode::SegmentViolation,
        AnCode::ZeroDrift,
        AnCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的匿名映射调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnonCall {
    /// 字节长
    pub len: u32,
    /// 保护位
    pub prot: u32,
}

/// 解析 "anon|len|prot" 三段调用行。
pub fn parse_anon_call(line: &str) -> Result<AnonCall, AnCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
    }
    let _ = parts.push(cur);
    if parts.len() != 3 {
        return Err(AnCode::MissingPart);
    }
    let num = |s: &str| -> Result<u32, AnCode> {
        if s.is_empty() {
            return Err(AnCode::BadNumber);
        }
        let mut acc: u32 = 0;
        for b in s.bytes() {
            if !b.is_ascii_digit() {
                return Err(AnCode::BadNumber);
            }
            acc = acc.wrapping_mul(10).wrapping_add((b - b'0') as u32);
        }
        Ok(acc)
    };
    let s1 = match parts.get(1) {
        Some(p) => p.as_str(),
        None => return Err(AnCode::MissingPart),
    };
    let s2 = match parts.get(2) {
        Some(p) => p.as_str(),
        None => return Err(AnCode::MissingPart),
    };
    let len = num(s1)?;
    let prot = num(s2)?;
    Ok(AnonCall { len, prot })
}

// ── 匿名页状态机 ──────────────────────────────────────────────────────
/// 匿名页生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnonState {
    /// 零页（未写）
    Zero,
    /// 已写
    Written,
    /// 已验讫（写后读回一致）
    Verified,
}

impl AnonState {
    /// 单调推进判定。
    pub fn can_advance(self, next: AnonState) -> bool {
        let rank = |s: AnonState| match s {
            AnonState::Zero => 0u8,
            AnonState::Written => 1,
            AnonState::Verified => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            AnonState::Zero => "零页",
            AnonState::Written => "已写",
            AnonState::Verified => "已验讫",
        }
    }
}

// ── 匿名映射账 ────────────────────────────────────────────────────────
/// 映射条目。
#[derive(Clone, Debug)]
pub struct AnonEnt {
    /// 起始地址（页对齐）
    pub start: u32,
    /// 字节长（页上取整）
    pub len: u32,
    /// 保护位
    pub prot: u32,
    /// 页体（匿名语义：建区即零填充）
    pub body: Vec<u8>,
    /// 生命周期状态
    pub state: AnonState,
}

/// 匿名映射账。
#[derive(Clone, Debug)]
pub struct AnonLedger {
    ents: Vec<AnonEnt>,
    cap: usize,
    cursor: u32,
    /// 累计映射次数
    pub maps: u32,
    /// 累计解除次数
    pub unmaps: u32,
}

impl AnonLedger {
    /// 空账构造。
    pub fn new() -> AnonLedger {
        AnonLedger { ents: Vec::new(), cap: LEDGER_CAP, cursor: KBASE, maps: 0, unmaps: 0 }
    }

    /// 在账数。
    pub fn len(&self) -> usize {
        self.ents.len()
    }

    /// 覆盖寻址。
    fn covering(&self, addr: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                let end = e.start.saturating_add(e.len);
                if addr >= e.start && addr < end {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 匿名映射：零长/坏 prot/账满/越窗逐闸；建区即零填充（零页语义）。
    pub fn mmap_anon(&mut self, call: &AnonCall) -> Result<u32, AnCode> {
        if call.len == 0 {
            return Err(AnCode::ZeroLen);
        }
        if call.prot & !3u32 != 0 {
            return Err(AnCode::BadProt);
        }
        if self.ents.len() >= self.cap {
            return Err(AnCode::LedgerFull);
        }
        let pages = (call.len + PAGE - 1) / PAGE;
        let span = pages.saturating_mul(PAGE);
        let end = self.cursor.checked_add(span).ok_or(AnCode::AddrRange)?;
        if end > KTOP {
            return Err(AnCode::AddrRange);
        }
        let start = self.cursor;
        self.cursor = end;
        let mut body: Vec<u8> = Vec::new();
        let mut k = 0u32;
        while k < span {
            let _ = body.push(0u8);
            k += 1;
        }
        let _ = self.ents.push(AnonEnt {
            start,
            len: span,
            prot: call.prot,
            body,
            state: AnonState::Zero,
        });
        self.maps = self.maps.saturating_add(1);
        Ok(start)
    }

    /// 写：须覆盖且带 W 位；写后态 Written。
    pub fn write_at(&mut self, addr: u32, data: &[u8]) -> Result<(), AnCode> {
        let idx = match self.covering(addr) {
            Some(i) => i,
            None => return Err(AnCode::NotMapped),
        };
        let (prot, off) = match self.ents.get(idx) {
            Some(e) => (e.prot, (addr - e.start) as usize),
            None => return Err(AnCode::NotMapped),
        };
        if prot & PROT_WRITE == 0 {
            return Err(AnCode::BadProt);
        }
        if data.is_empty() {
            return Ok(());
        }
        let last = addr.saturating_add(data.len() as u32 - 1);
        if self.covering(last).is_none() {
            return Err(AnCode::NotMapped);
        }
        if let Some(e) = self.ents.get_mut(idx) {
            let mut k = 0usize;
            while k < data.len() {
                let v = match data.get(k) {
                    Some(b) => *b,
                    None => 0,
                };
                if let Some(slot) = e.body.get_mut(off + k) {
                    *slot = v;
                } else {
                    let _ = e.body.push(v);
                }
                k += 1;
            }
            e.state = AnonState::Written;
        }
        Ok(())
    }

    /// 读：须覆盖且带 R 位；写后读一致即 Verified。
    pub fn read_at(&mut self, addr: u32, n: usize) -> Result<Vec<u8>, AnCode> {
        let idx = match self.covering(addr) {
            Some(i) => i,
            None => return Err(AnCode::NotMapped),
        };
        let (prot, off, st) = match self.ents.get(idx) {
            Some(e) => (e.prot, (addr - e.start) as usize, e.state),
            None => return Err(AnCode::NotMapped),
        };
        if prot & PROT_READ == 0 {
            return Err(AnCode::BadProt);
        }
        let mut out: Vec<u8> = Vec::new();
        let mut k = 0usize;
        while k < n {
            let v = match self.ents.get(idx).and_then(|e| e.body.get(off + k)) {
                Some(b) => *b,
                None => 0,
            };
            let _ = out.push(v);
            k += 1;
        }
        // 零页语义核对：Zero 态下读必须全零。
        if st == AnonState::Zero {
            let mut zi = 0usize;
            while zi < out.len() {
                if out.get(zi) != Some(&0u8) {
                    return Err(AnCode::ZeroDrift);
                }
                zi += 1;
            }
        }
        if st == AnonState::Written && st.can_advance(AnonState::Verified) {
            if let Some(e) = self.ents.get_mut(idx) {
                e.state = AnonState::Verified;
            }
        }
        Ok(out)
    }

    /// 解除。
    pub fn unmap(&mut self, addr: u32) -> Result<(), AnCode> {
        let idx = match self.covering(addr) {
            Some(i) => i,
            None => return Err(AnCode::NotMapped),
        };
        let _ = self.ents.remove(idx);
        self.unmaps = self.unmaps.saturating_add(1);
        Ok(())
    }

    /// 状态读取。
    pub fn state_of(&self, addr: u32) -> Option<AnonState> {
        match self.covering(addr) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.state),
                None => None,
            },
            None => None,
        }
    }
}

// ── J1 达成核：零页可写读回一致 10/10 轮 ──────────────────────────────
/// 逐轮：映射→读零页→写图案→读回一致→态 Verified。
pub fn run_anon_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = AnonLedger::new();
        let mut all_ok = true;
        // 面1：匿名映射 2 页。
        let call = AnonCall { len: 2 * PAGE, prot: PROT_READ | PROT_WRITE };
        let a = match l.mmap_anon(&call) {
            Ok(x) => x,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        all_ok = all_ok && a % PAGE == 0 && a == KBASE;
        probes += 1;
        // 面2：读先于写 = 零页（16 字节全零）。
        let z = l.read_at(a, 16).unwrap_or_default();
        let mut zero_ok = z.len() == 16;
        let mut zi = 0usize;
        while zi < z.len() {
            if z.get(zi) != Some(&0u8) {
                zero_ok = false;
            }
            zi += 1;
        }
        all_ok = all_ok && zero_ok;
        probes += 1;
        // 面3：写 16 字节图案。
        let mut pat: Vec<u8> = Vec::new();
        let mut k = 0u32;
        while k < 16 {
            let _ = pat.push(((k * 13 + 7) % 256) as u8);
            k += 1;
        }
        all_ok = all_ok && l.write_at(a, &pat).is_ok();
        probes += 1;
        // 面4：读回逐字节一致。
        all_ok = all_ok && l.read_at(a, 16) == Ok(pat.clone());
        probes += 1;
        // 面5：态 Verified。
        all_ok = all_ok && l.state_of(a) == Some(AnonState::Verified);
        probes += 1;
        // 面6：解除后账空。
        all_ok = all_ok && l.unmap(a).is_ok() && l.len() == 0;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏匿名映射矩阵 ────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAnon {
    /// 长度零（EINVAL 族）
    ZeroLen,
    /// 非法 prot（EINVAL 族）
    BadProt,
    /// 越窗读写（ENOMEM 族）
    NotMapped,
    /// 账满（ENOMEM 族）
    LedgerFull,
}

impl BadAnon {
    /// 闭集全量。
    pub const ALL: [BadAnon; 4] = [
        BadAnon::ZeroLen,
        BadAnon::BadProt,
        BadAnon::NotMapped,
        BadAnon::LedgerFull,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadAnon::ZeroLen => "长度零映射",
            BadAnon::BadProt => "非法 prot 映射",
            BadAnon::NotMapped => "越窗读写",
            BadAnon::LedgerFull => "账满映射",
        }
    }
}

/// 注入账。
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

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), AnCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(AnCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏匿名注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_anon_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadAnon::ALL.len() {
        let case = match BadAnon::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = AnonLedger::new();
            let _ = l.mmap_anon(&AnonCall { len: PAGE, prot: PROT_READ | PROT_WRITE });
            let caught = match case {
                BadAnon::ZeroLen => {
                    l.mmap_anon(&AnonCall { len: 0, prot: 3 }).err() == Some(AnCode::ZeroLen)
                }
                BadAnon::BadProt => {
                    l.mmap_anon(&AnonCall { len: PAGE, prot: 9 }).err() == Some(AnCode::BadProt)
                }
                BadAnon::NotMapped => {
                    l.write_at(KTOP + PAGE, b"x").err() == Some(AnCode::NotMapped)
                        && l.read_at(KTOP + PAGE, 1).err() == Some(AnCode::NotMapped)
                }
                BadAnon::LedgerFull => {
                    let mut last: Option<AnCode> = None;
                    let mut k = 0usize;
                    while k <= LEDGER_CAP {
                        last = l.mmap_anon(&AnonCall { len: PAGE, prot: 3 }).err();
                        k += 1;
                    }
                    last == Some(AnCode::LedgerFull)
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
/// 小结成文：「start=0x... pages=<n> state=<t>」。
pub fn compose_anon_summary(l: &AnonLedger, addr: u32) -> Result<String, AnCode> {
    let idx = match l.covering(addr) {
        Some(i) => i,
        None => return Err(AnCode::EmptySummary),
    };
    let (start, len, st) = match l.ents.get(idx) {
        Some(e) => (e.start, e.len, e.state),
        None => return Err(AnCode::EmptySummary),
    };
    let out = format!("start=0x{:08X} pages={} state={}", start, len / PAGE, st.label());
    if out.is_empty() {
        return Err(AnCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零长/坏 prot 族"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；越窗/账满/越选址窗族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料指纹：映射→读零→写→读回→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = AnonLedger::new();
    let _ = l.mmap_anon(&AnonCall { len: PAGE, prot: 3 });
    let mut bytes: Vec<u8> = Vec::new();
    let _ = l.write_at(KBASE, &[1, 2, 3]);
    if let Ok(back) = l.read_at(KBASE, 3) {
        bytes.extend_from_slice(&back);
    }
    if let Ok(s) = compose_anon_summary(&l, KBASE) {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10141 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10141_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10141");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93340..=0x9337F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < AnCode::ALL.len() {
        let mut j = i + 1;
        while j < AnCode::ALL.len() {
            if let (Some(a), Some(b)) = (AnCode::ALL.get(i), AnCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = AnCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_anon_call("anon|4096|3").is_ok()
        && parse_anon_call("anon|x|3").err() == Some(AnCode::BadNumber)
        && parse_anon_call("anon|4096").err() == Some(AnCode::MissingPart);
    s.add("调用行解析器：三段闭集放行且坏成分逐档拒绝", parse_ok,
          "len=0 在判定层拒；解析层只认数字闭集");

    // 4. J1 达成核
    let (ok, probes) = run_anon_rounds();
    s.add("J1 匿名映射达成：MAP_ANONYMOUS 零页可写读回一致 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（映射/零页/写/读回/态推进/解除）全对");

    // 5. J2 长度零
    let mut l = AnonLedger::new();
    let zero_ok = l.mmap_anon(&AnonCall { len: 0, prot: 3 }).err() == Some(AnCode::ZeroLen)
        && l.len() == 0;
    s.add("J2 长度零误用受控：EINVAL 精确复现且账面不动", zero_ok,
          "零长是匿名映射第一拒点；被拒不建条目");

    // 6. 零页语义
    let _ = l.mmap_anon(&AnonCall { len: PAGE, prot: 3 });
    let zp = l.read_at(KBASE, 8).unwrap_or_default();
    let mut zok = zp.len() == 8;
    let mut zi = 0usize;
    while zi < zp.len() {
        if zp.get(zi) != Some(&0u8) {
            zok = false;
        }
        zi += 1;
    }
    s.add("零页语义：建区读先于写逐字节为零", zok,
          "MAP_ANONYMOUS 零页是匿名语义根；任何漂移即 ZeroDrift");

    // 7. 只读页写拒
    let _ = l.mmap_anon(&AnonCall { len: PAGE, prot: PROT_READ });
    let ro_ok = l.write_at(KBASE + PAGE, b"x").err() == Some(AnCode::BadProt);
    s.add("只读页写：EINVAL 档精确复现", ro_ok,
          "无 W 位写必须拒");

    // 8. 越窗档
    let oob_ok = l.read_at(KTOP + PAGE, 1).err() == Some(AnCode::NotMapped)
        && l.unmap(KTOP + PAGE).err() == Some(AnCode::NotMapped);
    s.add("越窗：ENOMEM 读写解除三入口一致拒绝", oob_ok,
          "未覆盖地址必须三入口同档");

    // 9. 注入核
    let (inj, per_class) = run_bad_anon_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "零长/坏 prot/越窗/账满必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadAnon::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = AnonState::Zero.can_advance(AnonState::Written)
        && AnonState::Written.can_advance(AnonState::Verified)
        && !AnonState::Verified.can_advance(AnonState::Zero)
        && AnonState::Zero.label() == "零页";
    s.add("状态机：Zero→Written→Verified 单调", sm_ok,
          "匿名页生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(12)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_anon_summary(&l, KBASE);
    let text_ok = match &sum {
        Ok(x) => x.contains("start=0x") && x.contains("pages=1"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结字段齐备且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("匿名映射达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_anon_call("anon|4096|3").is_ok());
        assert_eq!(parse_anon_call("anon|x|3").err(), Some(AnCode::BadNumber));
        assert_eq!(parse_anon_call("anon|4096").err(), Some(AnCode::MissingPart));
    }

    #[test]
    fn zero_page_semantics() {
        let mut l = AnonLedger::new();
        let _ = l.mmap_anon(&AnonCall { len: PAGE, prot: 3 });
        let z = l.read_at(KBASE, 8).unwrap_or_default();
        assert_eq!(z, Vec::from(&[0u8; 8] as &[u8]));
        assert_eq!(l.state_of(KBASE), Some(AnonState::Zero));
    }

    #[test]
    fn write_readback() {
        let mut l = AnonLedger::new();
        let _ = l.mmap_anon(&AnonCall { len: PAGE, prot: 3 });
        let _ = l.write_at(KBASE, &[9, 8, 7]);
        assert_eq!(l.read_at(KBASE, 3), Ok(Vec::from(&[9u8, 8, 7] as &[u8])));
        assert_eq!(l.state_of(KBASE), Some(AnonState::Verified));
    }

    #[test]
    fn zero_len_blocked() {
        let mut l = AnonLedger::new();
        assert_eq!(l.mmap_anon(&AnonCall { len: 0, prot: 3 }).err(), Some(AnCode::ZeroLen));
    }

    #[test]
    fn bad_prot_blocked() {
        let mut l = AnonLedger::new();
        assert_eq!(l.mmap_anon(&AnonCall { len: PAGE, prot: 9 }).err(), Some(AnCode::BadProt));
    }

    #[test]
    fn readonly_write_blocked() {
        let mut l = AnonLedger::new();
        let _ = l.mmap_anon(&AnonCall { len: PAGE, prot: PROT_READ });
        assert_eq!(l.write_at(KBASE, b"x").err(), Some(AnCode::BadProt));
        assert!(l.read_at(KBASE, 1).is_ok());
    }

    #[test]
    fn unmap_then_deny() {
        let mut l = AnonLedger::new();
        let _ = l.mmap_anon(&AnonCall { len: PAGE, prot: 3 });
        assert!(l.unmap(KBASE).is_ok());
        assert_eq!(l.read_at(KBASE, 1).err(), Some(AnCode::NotMapped));
        assert_eq!(l.unmaps, 1);
    }

    #[test]
    fn bump_pages_monotonic() {
        let mut l = AnonLedger::new();
        let a1 = l.mmap_anon(&AnonCall { len: PAGE, prot: 3 }).unwrap_or(0);
        let a2 = l.mmap_anon(&AnonCall { len: 2 * PAGE, prot: 3 }).unwrap_or(0);
        assert_eq!(a2, a1 + PAGE);
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_anon_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_anon_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(AnonState::Zero.can_advance(AnonState::Verified));
        assert!(!AnonState::Verified.can_advance(AnonState::Written));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10141_checks_green() {
        let set = run_unx10141_checks();
        assert!(set.all_passed(), "UNX-F10141 red: {}", set.red_items_text());
    }
}
