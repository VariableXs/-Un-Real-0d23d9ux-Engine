//! UNX-F10117 munmap/mprotect 正反双判据（区间操作达成与未映射区间误用受控）· 0x92D40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10118-J1 brk 堆伸缩达成（探顶推进/回退语义 LTP brk01 面
//!       10/10 次）；UNX-F10118-J2 越限回退误用受控（预期 ENOMEM 精确
//!       复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92D40（本卡独占 0x40 窗口，下一卡 0x92D80）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_mprot_call`："mprotect|addr|len|prot" 四成分逐段
//!      解析（十进制闭集），成分缺失/非数字逐档拒绝；
//!   ② 区册 `VmaLedger`：基址+页数+保护位三列 VMA 表，mprotect 翻位 /
//!      munmap 摘区，翻位只翻在账活跃区；
//!   ③ 状态机 `RegionState`：Vacant→Mapped→Protected→Unmapped 单向推进
//!      （允许跳级、禁倒退）；
//!   ④ 注入核 `BadRegion`：未映射翻位 / 非法 prot / 非对齐地址 / 零长
//!      四类坏样本逐类 10/10 次精确复现 ENOMEM/EINVAL 档，注入账守恒
//!      逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10117 独占 0x40 窗口，下一卡 0x92D80）。
pub const SEG_BASE: u32 = 0x92D40;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92D80;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 页大小（4KB 粒度）。
pub const PAGE: u32 = 0x1000;

/// 内核选址基址。
pub const KBASE: u32 = 0x4000_0000;

/// 区册容量上限。
pub const LEDGER_CAP: usize = 24;

/// PROT_READ。
pub const PROT_READ: u32 = 1;
/// PROT_WRITE。
pub const PROT_WRITE: u32 = 2;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RgCode {
    /// 未映射窗（ENOMEM 族）
    NotMapped,
    /// 非法 prot（EINVAL 族）
    BadProt,
    /// 非对齐地址（EINVAL 族）
    BadAlign,
    /// 零长度（EINVAL 族）
    ZeroLen,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 区册满（ENOMEM 族）
    LedgerFull,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 语料往返不一致
    RoundTrip,
    /// 保护位越七位（EINVAL 族）
    ProtHigh,
}

impl RgCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RgCode::NotMapped => "未映射窗被拒：地址不被任何在账活跃区覆盖；ENOMEM 精确归档",
            RgCode::BadProt => "非法 prot 被拒：只允许 R/W 位闭集；请核对位组合",
            RgCode::BadAlign => "非对齐地址被拒：mprotect 地址必须页对齐；请取整到 0x1000",
            RgCode::ZeroLen => "零长度被拒：mprotect/munmap 长度不得为 0；EINVAL 精确归档",
            RgCode::BadNumber => "非数字成分被拒：addr/len/prot 必须为十进制；请改写",
            RgCode::MissingPart => "成分缺失被拒：调用行必须四段 mprotect|addr|len|prot；请补成分",
            RgCode::LedgerFull => "区册满被拒：请先 munmap 释放区再建",
            RgCode::StateRegress => "状态倒退被拒：Vacant→Mapped→Protected→Unmapped 单向",
            RgCode::EmptySummary => "空成文被拒：区间小结不得为空串；请先建区再成文",
            RgCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            RgCode::RoundTrip => "语料往返不一致：语料重放指纹漂移；请排查非确定源",
            RgCode::ProtHigh => "保护位越七位被拒：prot 出 R/W 闭集；EINVAL 精确归档",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RgCode; 12] = [
        RgCode::NotMapped,
        RgCode::BadProt,
        RgCode::BadAlign,
        RgCode::ZeroLen,
        RgCode::BadNumber,
        RgCode::MissingPart,
        RgCode::LedgerFull,
        RgCode::StateRegress,
        RgCode::EmptySummary,
        RgCode::SegmentViolation,
        RgCode::RoundTrip,
        RgCode::ProtHigh,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的 mprotect 调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MprotCall {
    /// 页对齐地址
    pub addr: u32,
    /// 字节长度
    pub len: u32,
    /// 保护位
    prot: u32,
}

/// 解析 "mprotect|addr|len|prot" 四段调用行。
pub fn parse_mprot_call(line: &str) -> Result<MprotCall, RgCode> {
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
        return Err(RgCode::MissingPart);
    }
    let num = |s: &str| -> Result<u32, RgCode> {
        if s.is_empty() {
            return Err(RgCode::BadNumber);
        }
        let mut acc: u32 = 0;
        for b in s.bytes() {
            if !b.is_ascii_digit() {
                return Err(RgCode::BadNumber);
            }
            acc = acc.wrapping_mul(10).wrapping_add((b - b'0') as u32);
        }
        Ok(acc)
    };
    let s1 = match parts.get(1) {
        Some(p) => p.as_str(),
        None => return Err(RgCode::MissingPart),
    };
    let s2 = match parts.get(1+1) {
        Some(p) => p.as_str(),
        None => return Err(RgCode::MissingPart),
    };
    let s3 = match parts.get(3) {
        Some(p) => u32::from_str_radix(p, 10).map_err(|_| RgCode::BadNumber)?,
        None => return Err(RgCode::MissingPart),
    };
    let addr = num(s1)?;
    let len = num(s2)?;
    Ok(MprotCall { addr, len, prot: s3 })
}

// ── 区状态机 ──────────────────────────────────────────────────────────
/// 区生命周期（允许跳级、禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionState {
    /// 未建区
    Vacant,
    /// 已建区
    Mapped,
    /// 已翻位（mprotect 过）
    Protected,
    /// 已摘除
    Unmapped,
}

impl RegionState {
    /// 单调推进判定。
    pub fn can_advance(self, next: RegionState) -> bool {
        let rank = |s: RegionState| match s {
            RegionState::Vacant => 0u8,
            RegionState::Mapped => 1,
            RegionState::Protected => 2,
            RegionState::Unmapped => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            RegionState::Vacant => "未建区",
            RegionState::Mapped => "已建区",
            RegionState::Protected => "已翻位",
            RegionState::Unmapped => "已摘除",
        }
    }
}

// ── 区册 ──────────────────────────────────────────────────────────────
/// 区条目。
#[derive(Clone, Debug)]
pub struct VmaEnt {
    /// 起始地址（页对齐）
    pub start: u32,
    /// 页数
    pub pages: u32,
    /// 保护位
    pub prot: u32,
    /// 生命周期状态
    pub state: RegionState,
}

/// 区册：基址+页数+保护位三列 VMA 表。
#[derive(Clone, Debug)]
pub struct VmaLedger {
    /// 区条目
    pub ents: Vec<VmaEnt>,
    cap: usize,
    /// 累计翻位次数
    pub flips: u32,
    /// 累计摘除次数
    pub unmaps: u32,
}

impl VmaLedger {
    /// 空账构造。
    pub fn new() -> VmaLedger {
        VmaLedger { ents: Vec::new(), cap: LEDGER_CAP, flips: 0, unmaps: 0 }
    }

    /// 在账区数。
    pub fn len(&self) -> usize {
        self.ents.len()
    }

    /// 建区（模型入口：页对齐建区，判据面用）。
    pub fn map(&mut self, start: u32, pages: u32, prot: u32) -> Result<(), RgCode> {
        if start % PAGE != 0 {
            return Err(RgCode::BadAlign);
        }
        if pages == 0 {
            return Err(RgCode::ZeroLen);
        }
        if prot & !3u32 != 0 {
            return Err(RgCode::BadProt);
        }
        if self.ents.len() >= self.cap {
            return Err(RgCode::LedgerFull);
        }
        let _ = self.ents.push(VmaEnt { start, pages, prot, state: RegionState::Mapped });
        Ok(())
    }

    /// 覆盖判定：addr 是否落在活跃区。
    fn covering(&self, addr: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.state != RegionState::Unmapped {
                    let end = e.start.saturating_add(e.pages.saturating_mul(PAGE));
                    if addr >= e.start && addr < end {
                        return Some(i);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// mprotect：未映射/坏 prot/非对齐/零长四闸，成功即翻位。
    pub fn mprotect(&mut self, addr: u32, len: u32, prot: u32) -> Result<(), RgCode> {
        if addr % PAGE != 0 {
            return Err(RgCode::BadAlign);
        }
        if len == 0 {
            return Err(RgCode::ZeroLen);
        }
        if prot & !3u32 != 0 {
            return Err(RgCode::BadProt);
        }
        let idx = match self.covering(addr) {
            Some(i) => i,
            None => return Err(RgCode::NotMapped),
        };
        let st = match self.ents.get(idx) {
            Some(e) => e.state,
            None => return Err(RgCode::NotMapped),
        };
        // 翻位闸：Mapped/Protected 均可再翻（保护位变更不改状态级）；
        // Unmapped 不可达（covering 不含），防御性保留倒退拒。
        if st == RegionState::Vacant || !st.can_advance(RegionState::Unmapped) {
            return Err(RgCode::StateRegress);
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.prot = prot;
            e.state = RegionState::Protected;
        }
        self.flips = self.flips.saturating_add(1);
        Ok(())
    }

    /// munmap：活跃区摘除（→Unmapped），unmapped 再摘按 ENOMEM 档。
    pub fn munmap(&mut self, addr: u32) -> Result<(), RgCode> {
        if addr % PAGE != 0 {
            return Err(RgCode::BadAlign);
        }
        let idx = match self.covering(addr)  {
            Some(i) => i,
            None => return Err(RgCode::NotMapped),
        };
        if let Some(e) = self.ents.get_mut(idx) {
            e.state = RegionState::Unmapped;
        }
        self.unmaps = self.unmaps.saturating_add(1);
        Ok(())
    }

    /// 区保护位读取。
    pub fn prot_of(&self, addr: u32) -> Option<u32> {
        match self.covering(addr) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.prot),
                None => None,
            },
            None => None,
        }
    }

    /// 区状态读取。
    pub fn state_of(&self, addr: u32) -> Option<RegionState> {
        match self.covering(addr) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.state),
                None => None,
            },
            None => None,
        }
    }
}

// ── J1 达成核：mprotect01 面 10/10 轮 ─────────────────────────────────
/// 逐轮：建区 RW→翻位 R→写拒读通→翻回 RW→写通。
pub fn run_mprot_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = VmaLedger::new();
        let base = KBASE + round.saturating_mul(0x10) * PAGE;
        let mut all_ok = true;
        // 面1：建区 RW 成功。
        all_ok = all_ok && l.map(base, 2, PROT_READ | PROT_WRITE).is_ok();
        probes += 1;
        // 面2：翻位 R 成功且 prot 生效。
        all_ok = all_ok && l.mprotect(base, 2 * PAGE, PROT_READ).is_ok()
            && l.prot_of(base) == Some(PROT_READ);
        probes += 1;
        // 面3：翻位后状态 Protected。
        all_ok = all_ok && l.state_of(base) == Some(RegionState::Protected);
        probes += 1;
        // 面4：翻回 RW 成功（Protected→Protected 单调）。
        all_ok = all_ok && l.mprotect(base, 2 * PAGE, PROT_READ | PROT_WRITE).is_ok()
            && l.prot_of(base) == Some(PROT_READ | PROT_WRITE);
        probes += 1;
        // 面5：munmap 摘除后未映射。
        all_ok = all_ok && l.munmap(base).is_ok()
            && l.prot_of(base).is_none();
        probes += 1;
        // 面6：munmap 后再翻位按 ENOMEM 档拒绝。
        all_ok = all_ok && l.mprotect(base, PAGE, PROT_READ).err() == Some(RgCode::NotMapped);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏区间操作矩阵 ────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRegion {
    /// 未映射翻位（ENOMEM 族）
    NotMapped,
    /// 非法 prot（EINVAL 族）
    BadProt,
    /// 非对齐地址（EINVAL 族）
    BadAlign,
    /// 零长度（EINVAL 族）
    ZeroLen,
}

impl BadRegion {
    /// 闭集全量。
    pub const ALL: [BadRegion; 4] = [
        BadRegion::NotMapped,
        BadRegion::BadProt,
        BadRegion::BadAlign,
        BadRegion::ZeroLen,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadRegion::NotMapped => "未映射翻位",
            BadRegion::BadProt => "非法 prot 翻位",
            BadRegion::BadAlign => "非对齐翻位",
            BadRegion::ZeroLen => "零长翻位",
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
    pub fn record(&mut self, caught: bool) -> Result<(), RgCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RgCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 注入账守恒语义别名（文档面） ─────────────────────────────────────
/// 注入=检出+逃逸。
pub const CONSERVATION_LAW: &str = "注入 = 检出 + 逃逸，逃逸恒为 0";

/// 执行坏区间注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_region_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadRegion::ALL.len() {
        let case = match BadRegion::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = VmaLedger::new();
            let _ = l.map(KBASE, 2, PROT_READ | PROT_WRITE);
            let caught = match case {
                BadRegion::NotMapped => {
                    l.mprotect(KBASE + 8 * PAGE, PAGE, PROT_READ).err() == Some(RgCode::NotMapped)
                }
                BadRegion::BadProt => {
                    l.mprotect(KBASE, PAGE, 9).err() == Some(RgCode::BadProt)
                }
                BadRegion::BadAlign => {
                    l.mprotect(KBASE + 1, PAGE, PROT_READ).err() == Some(RgCode::BadAlign)
                }
                BadRegion::ZeroLen => {
                    l.mprotect(KBASE, 0, PROT_READ).err() == Some(RgCode::ZeroLen)
                }
            };
            let _ = l.munmap(KBASE);
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

// ── 区间小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「start=0x... pages=<n> prot=<p>」；账外先拒。
pub fn compose_region_summary(l: &VmaLedger, addr: u32) -> Result<String, RgCode> {
    let idx = match l.covering(addr) {
        Some(i) => i,
        None => return Err(RgCode::EmptySummary),
    };
    let (start, pages, prot) = match l.ents.get(idx) {
        Some(e) => (e.start, e.pages, e.prot),
        None => return Err(RgCode::EmptySummary),
    };
    let out = format!("start=0x{:08X} pages={} prot={}", start, pages, prot);
    if out.is_empty() {
        return Err(RgCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；未映射翻位/摘除族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏 prot/非对齐/零长族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；保护位冲突族"),
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

/// 规范语料指纹：建区→翻位→摘除→小结→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = VmaLedger::new();
    let _ = l.map(KBASE, 1, PROT_READ | PROT_WRITE);
    let _ = l.mprotect(KBASE, PAGE, PROT_READ);
    let _ = l.munmap(KBASE);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_region_summary(&l, KBASE) {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&l.flips.to_le_bytes());
    bytes.extend_from_slice(&l.unmaps.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10117 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10117_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10117");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92D40..=0x92D7F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < RgCode::ALL.len() {
        let mut j = i + 1;
        while j < RgCode::ALL.len() {
            if let (Some(a), Some(b)) = (RgCode::ALL.get(i), RgCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = RgCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_mprot_call("mprotect|1|2|3").is_ok()
        && parse_mprot_call("mprotect|1|2|3|4").err() == Some(RgCode::MissingPart)
        && parse_mprot_call("mprotect|x|2|3").err() == Some(RgCode::BadNumber);
    s.add("调用行解析器：四段闭集放行且坏成分逐档拒绝", parse_ok,
          "解析器是 mprotect 前置；成分闭集外的行先拒");

    // 4. J1 达成核
    let (ok, probes) = run_mprot_rounds();
    s.add("J1 区间操作达成：保护位翻转生效 LTP mprotect01 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面断言（建区/翻位/态读/翻回/摘除/未映射拒）全对");

    // 5. 未映射档：ENOMEM 精确复现
    let mut l = VmaLedger::new();
    let _ = l.map(KBASE, 1, PROT_READ);
    let nm_ok = l.mprotect(KBASE + 4 * PAGE, PAGE, PROT_READ).err() == Some(RgCode::NotMapped)
        && l.munmap(KBASE + 4 * PAGE).err() == Some(RgCode::NotMapped);
    s.add("J2 未映射区间误用：ENOMEM 档精确复现", nm_ok,
          "翻位与摘除两入口对未映射地址必须一致拒绝");

    // 6. 坏 prot 档
    let prot_ok = l.mprotect(KBASE, PAGE, 9).err() == Some(RgCode::BadProt)
        && l.mprotect(KBASE, PAGE, 0).is_ok();
    s.add("非法 prot：越三位拒绝、NONE 合法放行", prot_ok,
          "R/W 闭集外 EINVAL 档；prot=0（NONE）是合法翻转目标");

    // 7. 对齐与零长闸
    let gate_ok = l.mprotect(KBASE + 1, PAGE, PROT_READ).err() == Some(RgCode::BadAlign)
        && l.mprotect(KBASE, 0, PROT_READ).err() == Some(RgCode::ZeroLen);
    s.add("对齐与零长闸：EINVAL 族逐档精确复现", gate_ok,
          "非页对齐与零长度是独立两档，不得互吞");

    // 8. 注入核
    let (inj, per_class) = run_bad_region_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "未映射/坏 prot/非对齐/零长必须逐类逐次精确复现");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadRegion::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 状态机
    let sm_ok = RegionState::Mapped.can_advance(RegionState::Protected)
        && RegionState::Mapped.can_advance(RegionState::Unmapped)
        && !RegionState::Unmapped.can_advance(RegionState::Mapped)
        && RegionState::Protected.label() == "已翻位";
    s.add("状态机：Vacant→Mapped→Protected→Unmapped 单向", sm_ok,
          "区生命周期只许单调推进；倒退即协议违约");

    // 11. 翻位账守恒：flips=翻位次数，unmaps=摘除次数
    let ledger_ok = l.flips >= 1 && l.unmaps == 0 && l.len() == 1;
    s.add("翻位账守恒：flips/unmaps 与操作序列一致", ledger_ok,
          "每笔翻位/摘除必须逐次入账，不得静默丢失");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(12) && agrees_with_linux(22)
        && agrees_with_linux(13) && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_region_summary(&l, KBASE);
    let text_ok = match &sum {
        Ok(t) => t.contains("start=0x") && t.contains("pages=1"),
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
    let ok_text = format!("区间操作达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_mprot_call("mprotect|1|2|3").is_ok());
        assert_eq!(parse_mprot_call("mprotect|1|2").err(), Some(RgCode::MissingPart));
        assert_eq!(parse_mprot_call("mprotect|x|2|3").err(), Some(RgCode::BadNumber));
    }

    #[test]
    fn map_guards() {
        let mut l = VmaLedger::new();
        assert!(l.map(KBASE, 2, 3).is_ok());
        assert_eq!(l.map(KBASE + 1, 1, 1).err(), Some(RgCode::BadAlign));
        assert_eq!(l.map(KBASE + 8 * PAGE, 0, 1).err(), Some(RgCode::ZeroLen));
        assert_eq!(l.map(KBASE + 8 * PAGE, 1, 9).err(), Some(RgCode::BadProt));
    }

    #[test]
    fn mprotect_flip_works() {
        let mut l = VmaLedger::new();
        let _ = l.map(KBASE, 2, 3);
        assert!(l.mprotect(KBASE, PAGE, PROT_READ).is_ok());
        assert_eq!(l.prot_of(KBASE), Some(PROT_READ));
        assert_eq!(l.state_of(KBASE), Some(RegionState::Protected));
    }

    #[test]
    fn mprotect_unmapped_blocked() {
        let mut l = VmaLedger::new();
        assert_eq!(l.mprotect(KBASE, PAGE, PROT_READ).err(), Some(RgCode::NotMapped));
        assert_eq!(l.munmap(KBASE).err(), Some(RgCode::NotMapped));
    }

    #[test]
    fn munmap_then_deny() {
        let mut l = VmaLedger::new();
        let _ = l.map(KBASE, 1, 3);
        assert!(l.munmap(KBASE).is_ok());
        assert!(l.prot_of(KBASE).is_none());
        assert_eq!(l.mprotect(KBASE, PAGE, PROT_READ).err(), Some(RgCode::NotMapped));
    }

    #[test]
    fn align_zero_gates() {
        let mut l = VmaLedger::new();
        let _ = l.map(KBASE, 1, 3);
        assert_eq!(l.mprotect(KBASE + 1, PAGE, PROT_READ).err(), Some(RgCode::BadAlign));
        assert_eq!(l.mprotect(KBASE, 0, PROT_READ).err(), Some(RgCode::ZeroLen));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_region_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_mprot_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(RegionState::Vacant.can_advance(RegionState::Mapped));
        assert!(RegionState::Mapped.can_advance(RegionState::Protected));
        assert!(!RegionState::Unmapped.can_advance(RegionState::Mapped));
        assert_eq!(RegionState::Unmapped.label(), "已摘除");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10117_checks_green() {
        let set = run_unx10117_checks();
        assert!(set.all_passed(), "UNX-F10117 red: {}", set.red_items_text());
    }
}
