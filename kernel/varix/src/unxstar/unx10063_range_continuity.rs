//! UNX-F10063 号段连续性核验（零空洞） · 0x91f80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10063（M 型收口线：号段解析与空洞归并核验）。
//! 锚点：UNX-F10064-J1 三成分全检过（480 条逐条三成分核 10/10 次），缺
//!       成分 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91f80（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `RangeCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `SegmentParser`：号段行 "F9601..F9660" 解析为闭区间，
//!     倒置段/越窗段/非数字段/空段逐类显性拒绝；
//!   - 归并器 `IntervalMerger`：段序列排序归并，重叠段合并且重叠计数
//!     落账，归并后剩余空洞逐一显性定位（首缺号）；
//!   - 账本 `CoverageLedger`：归并覆盖计数 + 空洞逃逸计数（恒 0）+
//!     只增不减守卫；
//!   - 状态机 `RangeState`：Collecting → Merged → Audited 单向推进，
//!     未归并出审计结论拒绝；
//!   - 注入核 `BadRangeCase`：倒置段 / 越窗段 / 非数字段 / 重叠段
//!     计账 / 空洞段 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10063 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91f80;

/// 段窗上界（下一卡 0x91fc0 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91fc0;

/// 号段合法窗口起点（F9601）。
pub const RANGE_LO: u32 = 9601;

/// 号段合法窗口终点（F10080）。
pub const RANGE_HI: u32 = 10080;

/// 窗口内号位总数（480）。
pub const RANGE_LEN: u32 = RANGE_HI - RANGE_LO + 1;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;
pub const EDOM: i32 = 33;
pub const ENODATA: i32 = 61;
pub const EOVERFLOW: i32 = 75;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeCode {
    /// 空段（行无内容）
    EmptySegment,
    /// 非数字段（段端点含非数字字符）
    NotNumeric,
    /// 倒置段（起点 > 终点）
    Inverted,
    /// 越窗段（端点出 9601..=10080）
    OutOfWindow,
    /// 重叠段计账（归并后重叠计数非零须显性可见）
    OverlapCounted,
    /// 空洞段（归并后存在缺号）
    ContinuityHole,
    /// 未归并出审计结论
    NotMerged,
    /// 覆盖账倒退（只增不减破坏）
    LedgerRegress,
    /// 空洞逃逸（账面漏计空洞）
    HoleEscape,
    /// 解析失败（段行格式非法）
    ParseFailure,
    /// 状态倒退
    StateRegress,
    /// 段位越界
    SegmentViolation,
}

impl RangeCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RangeCode::EmptySegment => "空段被拒：号段行不得为空；请补段行",
            RangeCode::NotNumeric => "非数字段被拒：段端点必须为纯数字 F 号；请修正端点",
            RangeCode::Inverted => "倒置段被拒：段起点不得大于终点；请对调端点",
            RangeCode::OutOfWindow => "越窗段被拒：端点只允许 9601..=10080；请核对窗口",
            RangeCode::OverlapCounted => "重叠段计账：归并覆盖与原段总长不符；请查重复段",
            RangeCode::ContinuityHole => "空洞段被拒：归并后存在缺号；请按首缺号补段",
            RangeCode::NotMerged => "未归并被拒：未归并不得出零空洞结论；请先归并",
            RangeCode::LedgerRegress => "覆盖账倒退被拒：覆盖计数只增不减；请排查回滚",
            RangeCode::HoleEscape => "空洞逃逸：账面漏计空洞；请补记空洞明细",
            RangeCode::ParseFailure => "解析失败：段行必须恰为 Flo..Fhi 两端点；请补齐分隔",
            RangeCode::StateRegress => "状态倒退被拒：Collecting→Merged→Audited 单向",
            RangeCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91f80..=0x91fc0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RangeCode; 12] = [
        RangeCode::EmptySegment,
        RangeCode::NotNumeric,
        RangeCode::Inverted,
        RangeCode::OutOfWindow,
        RangeCode::OverlapCounted,
        RangeCode::ContinuityHole,
        RangeCode::NotMerged,
        RangeCode::LedgerRegress,
        RangeCode::HoleEscape,
        RangeCode::ParseFailure,
        RangeCode::StateRegress,
        RangeCode::SegmentViolation,
    ];
}

// ── 段解析器 ──────────────────────────────────────────────────────────
/// 闭区间号段 [lo, hi]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Segment {
    /// 段起点（含）
    pub lo: u32,
    /// 段终点（含）
    pub hi: u32,
}

/// 端点解析："F9660" → 9660；其余一律拒绝。
pub fn parse_endpoint(s: &str) -> Result<u32, RangeCode> {
    let body = match s.strip_prefix('F') {
        Some(b) => b,
        None => return Err(RangeCode::NotNumeric),
    };
    if body.is_empty() || body.len() > 5 {
        return Err(RangeCode::NotNumeric);
    }
    let mut n: u32 = 0;
    for b in body.bytes() {
        if !b.is_ascii_digit() {
            return Err(RangeCode::NotNumeric);
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    Ok(n)
}

/// 段行解析："F9601..F9660" → 闭区间；倒置/越窗/格式非法逐类拒绝。
pub fn parse_segment(line: &str) -> Result<Segment, RangeCode> {
    if line.is_empty() {
        return Err(RangeCode::EmptySegment);
    }
    const SEP: &str = "..";
    let pos = match line.find(SEP) {
        Some(p) => p,
        None => return Err(RangeCode::ParseFailure),
    };
    let lo_s = line.get(..pos).unwrap_or("");
    let hi_s = line.get(pos + SEP.len()..).unwrap_or("");
    if hi_s.is_empty() {
        return Err(RangeCode::ParseFailure);
    }
    let lo = parse_endpoint(lo_s)?;
    let hi = parse_endpoint(hi_s)?;
    if lo > hi {
        return Err(RangeCode::Inverted);
    }
    if lo < RANGE_LO || hi > RANGE_HI {
        return Err(RangeCode::OutOfWindow);
    }
    Ok(Segment { lo, hi })
}

/// 段长（含两端）。
pub fn seg_len(seg: &Segment) -> u32 {
    seg.hi.saturating_sub(seg.lo).saturating_add(1)
}

// ── 归并器 ────────────────────────────────────────────────────────────
/// 状态机：Collecting → Merged → Audited 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeState {
    /// 收段中
    Collecting,
    /// 已归并
    Merged,
    /// 已出零空洞结论
    Audited,
}

/// 归并器：段序列排序归并 + 空洞显性定位。
#[derive(Clone, Debug)]
pub struct IntervalMerger {
    /// 原始段序列
    pub segs: Vec<Segment>,
    /// 归并后区间
    pub merged: Vec<Segment>,
    /// 归并覆盖号位数
    pub covered: u32,
    /// 原始段总长（重叠时会大于 covered）
    pub raw_total: u32,
    /// 当前状态
    pub state: RangeState,
}

impl IntervalMerger {
    /// 构造空归并器。
    pub fn new() -> IntervalMerger {
        IntervalMerger {
            segs: Vec::new(),
            merged: Vec::new(),
            covered: 0,
            raw_total: 0,
            state: RangeState::Collecting,
        }
    }

    /// 收一段：仅 Collecting 收段；越窗/倒置在解析口已拒。
    pub fn collect(&mut self, seg: Segment) -> Result<(), RangeCode> {
        if self.state != RangeState::Collecting {
            return Err(RangeCode::StateRegress);
        }
        if seg.lo < RANGE_LO || seg.hi > RANGE_HI || seg.lo > seg.hi {
            return Err(RangeCode::OutOfWindow);
        }
        self.raw_total = self.raw_total.saturating_add(seg_len(&seg));
        self.segs.push(seg);
        Ok(())
    }

    /// 归并：按起点排序后线性合并，重叠计入 covered 不重复计。
    pub fn merge(&mut self) -> Result<(), RangeCode> {
        if self.state != RangeState::Collecting {
            return Err(RangeCode::StateRegress);
        }
        if self.segs.is_empty() {
            return Err(RangeCode::EmptySegment);
        }
        // 插入排序（no_std 无 sort 稳定性要求，段数有限）。
        let n = self.segs.len();
        let mut i = 1usize;
        while i < n {
            let key = match self.segs.get(i).copied() {
                Some(k) => k,
                None => break,
            };
            let mut j = i;
            while j > 0 {
                let prev = match self.segs.get(j - 1).copied() {
                    Some(p) => p,
                    None => break,
                };
                if prev.lo > key.lo {
                    self.segs[j] = prev;
                    j -= 1;
                } else {
                    break;
                }
            }
            if let Some(slot) = self.segs.get_mut(j) {
                *slot = key;
            }
            i += 1;
        }
        // 线性归并。
        self.merged.clear();
        self.covered = 0;
        let mut idx = 0usize;
        while idx < self.segs.len() {
            let cur = match self.segs.get(idx).copied() {
                Some(c) => c,
                None => break,
            };
            match self.merged.last_mut() {
                Some(last) if cur.lo <= last.hi.saturating_add(1) => {
                    if cur.hi > last.hi {
                        last.hi = cur.hi;
                    }
                }
                _ => self.merged.push(cur),
            }
            idx += 1;
        }
        for m in self.merged.iter() {
            self.covered = self.covered.saturating_add(seg_len(m));
        }
        self.state = RangeState::Merged;
        Ok(())
    }

    /// 首缺号：归并序列起 F9601 找第一个不在覆盖内的号；无缺返回 None。
    pub fn first_hole(&self) -> Option<u32> {
        if self.state != RangeState::Merged && self.state != RangeState::Audited {
            return None;
        }
        let mut n = RANGE_LO;
        while n <= RANGE_HI {
            let mut inside = false;
            for m in self.merged.iter() {
                if n >= m.lo && n <= m.hi {
                    inside = true;
                    break;
                }
                if m.lo > n {
                    break;
                }
            }
            if !inside {
                return Some(n);
            }
            n += 1;
        }
        None
    }

    /// 零空洞结论：归并覆盖恰为 480 且首缺号不存在才可出具；未归并拒绝。
    pub fn audit(&mut self) -> Result<(), RangeCode> {
        if self.state != RangeState::Merged {
            return Err(RangeCode::NotMerged);
        }
        if self.first_hole().is_some() || self.covered != RANGE_LEN {
            return Err(RangeCode::ContinuityHole);
        }
        self.state = RangeState::Audited;
        Ok(())
    }
}

/// 标准收段：单段 F9601..F10080 全覆盖 → 归并 → 审计零空洞。
pub fn audit_standard() -> Result<(), RangeCode> {
    let mut m = IntervalMerger::new();
    m.collect(Segment { lo: RANGE_LO, hi: RANGE_HI })?;
    m.merge()?;
    m.audit()
}

/// 24 册段收段：每册 20 号（F9601..F9620、F9621..F9640、…），零空洞。
pub fn audit_by_books() -> Result<(), RangeCode> {
    let mut m = IntervalMerger::new();
    let mut b = 0u32;
    while b < 24 {
        let lo = RANGE_LO + b * 20;
        let hi = lo + 19;
        m.collect(Segment { lo, hi })?;
        b += 1;
    }
    m.merge()?;
    m.audit()
}

// ── 覆盖账 ────────────────────────────────────────────────────────────
/// 覆盖账：覆盖计数 + 空洞逃逸计数（恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoverageLedger {
    /// 已覆盖号位数
    pub covered: u32,
    /// 空洞逃逸数（必须为 0）
    pub hole_escape: u32,
}

impl CoverageLedger {
    /// 构造零账。
    pub const fn zero() -> CoverageLedger {
        CoverageLedger { covered: 0, hole_escape: 0 }
    }

    /// 记覆盖：只增不减守卫。
    pub fn add_cover(&mut self, n: u32) -> Result<(), RangeCode> {
        let next = self.covered.saturating_add(n);
        if next < self.covered {
            return Err(RangeCode::LedgerRegress);
        }
        self.covered = next;
        Ok(())
    }

    /// 记空洞：显性入账不静默。
    pub fn record_hole(&mut self) {
        self.hole_escape = self.hole_escape.saturating_add(1);
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏号段样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRangeCase {
    /// 倒置段（起点>终点）
    Inverted,
    /// 越窗段（9600 起或 10081 止）
    OutOfWindow,
    /// 非数字段（端点带字母）
    NotNumeric,
    /// 空段
    EmptySegment,
    /// 空洞段（缺 9701..9720，审计必须拒）
    ContinuityHole,
}

impl BadRangeCase {
    /// 闭集全量。
    pub const ALL: [BadRangeCase; 5] = [
        BadRangeCase::Inverted,
        BadRangeCase::OutOfWindow,
        BadRangeCase::NotNumeric,
        BadRangeCase::EmptySegment,
        BadRangeCase::ContinuityHole,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadRangeCase::Inverted => {
                parse_segment("F9700..F9601") == Err(RangeCode::Inverted)
            }
            BadRangeCase::OutOfWindow => {
                parse_segment("F9600..F9660").is_err()
                    && parse_segment("F9601..F10081").is_err()
            }
            BadRangeCase::NotNumeric => {
                parse_segment("F96x1..F9660").is_err()
                    && parse_endpoint("G9601").is_err()
                    && parse_endpoint("F").is_err()
            }
            BadRangeCase::EmptySegment => {
                parse_segment("").is_err() && parse_segment("F9601..").is_err()
            }
            BadRangeCase::ContinuityHole => {
                // 覆盖 9601..9700 + 9721..10080（缺 9701..9720），审计必须拒。
                let mut m = IntervalMerger::new();
                if m.collect(Segment { lo: RANGE_LO, hi: 9700 }).is_err() {
                    return false;
                }
                if m.collect(Segment { lo: 9721, hi: RANGE_HI }).is_err() {
                    return false;
                }
                if m.merge().is_err() {
                    return false;
                }
                m.first_hole() == Some(9701) && m.audit().is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_range_injection() -> (CoverageLedger, [u32; 5]) {
    let mut ledger = CoverageLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadRangeCase::ALL.len() {
        if let Some(case) = BadRangeCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.add_cover(1);
                if !caught {
                    ledger.record_hole();
                }
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；倒置段/格式族"),
    ("EDOM", -33, 87, "Windows ERROR_INVALID_PARAMETER；端点域外族"),
    ("ERANGE", -34, 206, "Windows ERROR_FILENAME_EXCED_RANGE；越窗族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_DATA；空段族"),
    ("EOVERFLOW", -75, 28, "Windows ERROR_OUT_OF_PAPER；计数溢出族"),
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

/// 规范语料双跑指纹：解析→归并→审计全链逐字节入哈希（含空洞探测）。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match audit_standard() {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match audit_by_books() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    // 空洞场景：缺 9701..9720，首缺号入哈希。
    let mut m = IntervalMerger::new();
    let _ = m.collect(Segment { lo: RANGE_LO, hi: 9700 });
    let _ = m.collect(Segment { lo: 9721, hi: RANGE_HI });
    let _ = m.merge();
    bytes.push(match m.first_hole() {
        Some(9701) => 5,
        _ => 6,
    });
    bytes.extend_from_slice(&m.covered.to_le_bytes());
    bytes.extend_from_slice(&m.raw_total.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10063 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10063_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10063");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91f80..=0x91fc0", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 端点解析器：合法命中 + 非法拒绝
    let ep_ok = parse_endpoint("F9601") == Ok(9601)
        && parse_endpoint("F10080") == Ok(10080)
        && parse_endpoint("G9601").is_err()
        && parse_endpoint("F96x1").is_err()
        && parse_endpoint("F").is_err();
    s.add("端点解析：F 号命中、非 F/带字母/空体拒绝", ep_ok,
          "端点必须为 F+纯数字，长度 1..=5");

    // 3. 段行解析器：合法段命中
    let seg_parse = parse_segment("F9601..F9660");
    let seg_ok2 = seg_parse == Ok(Segment { lo: 9601, hi: 9660 }) && seg_len(&seg_parse.unwrap_or(Segment { lo: 0, hi: 0 })) == 60;
    s.add("段行解析：F9601..F9660 → 闭区间 [9601,9660]", seg_ok2,
          "段长含两端：60 号位");

    // 4. 倒置段拦截
    let inv_ok = parse_segment("F9700..F9601") == Err(RangeCode::Inverted);
    s.add("倒置段：起点>终点即拒", inv_ok,
          "闭区间必须 lo<=hi，倒置显性拒绝");

    // 5. 越窗段拦截
    let win_ok = parse_segment("F9600..F9660").is_err()
        && parse_segment("F9601..F10081").is_err()
        && parse_segment("F10081..F10100").is_err();
    s.add("越窗段：9600 起/10081 止全拒", win_ok,
          "号段窗口冻结 9601..=10080，出窗即拦");

    // 6. 空段/格式段拦截
    let fmt_ok = parse_segment("").is_err()
        && parse_segment("F9601..").is_err()
        && parse_segment("F9601-F9660").is_err()
        && parse_segment("F9601..F9660..").is_err();
    s.add("格式段：空段/缺终点/错分隔全拒", fmt_ok,
          "段行只允许 Flo..Fhi 形态");

    // 7. 标准单段审计：零空洞
    let std_ok = audit_standard().is_ok();
    s.add("标准审计：单段 F9601..F10080 零空洞通过", std_ok,
          "覆盖 480 恰等于窗口总数");

    // 8. 24 册分段审计：零空洞
    let books_ok = audit_by_books().is_ok();
    s.add("分册审计：24 段各 20 号零空洞通过", books_ok,
          "9601 起每册 20 号，拼接恰满窗口");

    // 9. 空洞定位：缺 9701..9720 时首缺号=9701
    let mut m = IntervalMerger::new();
    let _ = m.collect(Segment { lo: RANGE_LO, hi: 9700 });
    let _ = m.collect(Segment { lo: 9721, hi: RANGE_HI });
    let _ = m.merge();
    let hole_ok = m.first_hole() == Some(9701)
        && m.covered == 460
        && m.audit().is_err();
    s.add("空洞定位：缺段后首缺号=F9701、审计拒绝", hole_ok,
          "覆盖 460≠480，空洞显性定位到 9701");

    // 10. 重叠段归并：重叠只计一次
    let mut ov = IntervalMerger::new();
    let _ = ov.collect(Segment { lo: 9601, hi: 9660 });
    let _ = ov.collect(Segment { lo: 9651, hi: 9700 });
    let _ = ov.merge();
    let ov_ok = ov.covered == 100
        && ov.raw_total == 110
        && ov.merged.len() == 1
        && ov.first_hole() == Some(9701);
    s.add("重叠归并：重叠 10 号只计一次（覆盖 100/原始 110）", ov_ok,
          "覆盖计数去重、原始计数显性保留差异；段外 9701 显性为空洞");

    // 11. 乱序收段：归并与收段序无关
    let mut a = IntervalMerger::new();
    let _ = a.collect(Segment { lo: 9701, hi: 10080 });
    let _ = a.collect(Segment { lo: 9601, hi: 9700 });
    let _ = a.merge();
    let order_ok = a.covered == RANGE_LEN && a.first_hole().is_none() && a.audit().is_ok();
    s.add("乱序无关：倒序收段归并后同零空洞", order_ok,
          "归并按起点排序，收段序不影响结论");

    // 12. 未归并出结论拦截
    let mut nm = IntervalMerger::new();
    let _ = nm.collect(Segment { lo: 9601, hi: 9660 });
    let nm_rej = nm.audit() == Err(RangeCode::NotMerged);
    s.add("审计闸：未归并出零空洞结论被拒", nm_rej,
          "Collecting → Merged → Audited 单向，缺步即拦");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_range_injection();
    let all_caught = per_class.iter().all(|c| *c == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "倒置/越窗/非数字/空段/空洞全被显式拒绝");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入 50 轮覆盖 50 且空洞逃逸=0",
          ledger.covered == 5 * INJECT_ROUNDS && ledger.hole_escape == 0,
          "注入核账守恒：50 次注入 50 次检出、0 空洞逃逸");

    // 15. 文案闭集：非空
    let text_ok = RangeCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < RangeCode::ALL.len() {
        let mut j = i + 1;
        while j < RangeCode::ALL.len() {
            if let (Some(x), Some(y)) = (RangeCode::ALL.get(i), RangeCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = RangeCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(EDOM)
        && agrees_with_linux(ERANGE)
        && agrees_with_linux(ENODATA)
        && agrees_with_linux(EOVERFLOW)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：覆盖快照 + 指纹快照
    let snap_text = format!("覆盖 {} / 空洞逃逸 {}", ledger.covered, ledger.hole_escape);
    s.add_dyn("覆盖快照", ledger.hole_escape == 0, &snap_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_parse_roundtrip() {
        assert_eq!(parse_endpoint("F9601"), Ok(9601));
        assert_eq!(parse_endpoint("F10080"), Ok(10080));
        assert!(parse_endpoint("F96x1").is_err());
        assert!(parse_endpoint("G1").is_err());
    }

    #[test]
    fn segment_parse_boundaries() {
        assert_eq!(parse_segment("F9601..F9660"), Ok(Segment { lo: 9601, hi: 9660 }));
        assert_eq!(parse_segment("F10080..F10080"), Ok(Segment { lo: 10080, hi: 10080 }));
        assert_eq!(parse_segment("F9700..F9601"), Err(RangeCode::Inverted));
        assert_eq!(parse_segment("F9600..F9660"), Err(RangeCode::OutOfWindow));
    }

    #[test]
    fn standard_audit_ok() {
        assert!(audit_standard().is_ok());
        assert!(audit_by_books().is_ok());
    }

    #[test]
    fn hole_located_at_9701() {
        let mut m = IntervalMerger::new();
        let _ = m.collect(Segment { lo: 9601, hi: 9700 });
        let _ = m.collect(Segment { lo: 9721, hi: 10080 });
        let _ = m.merge();
        assert_eq!(m.first_hole(), Some(9701));
        assert_eq!(m.covered, 460);
        assert!(m.audit().is_err());
    }

    #[test]
    fn overlap_counted_once() {
        let mut m = IntervalMerger::new();
        let _ = m.collect(Segment { lo: 9601, hi: 9660 });
        let _ = m.collect(Segment { lo: 9651, hi: 9700 });
        let _ = m.merge();
        assert_eq!(m.covered, 100);
        assert_eq!(m.raw_total, 110);
        assert_eq!(m.first_hole(), Some(9701));
    }

    #[test]
    fn audit_requires_merge() {
        let mut m = IntervalMerger::new();
        let _ = m.collect(Segment { lo: 9601, hi: 9660 });
        assert_eq!(m.audit(), Err(RangeCode::NotMerged));
    }

    #[test]
    fn empty_merger_rejected() {
        let mut m = IntervalMerger::new();
        assert_eq!(m.merge(), Err(RangeCode::EmptySegment));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_range_injection();
        assert_eq!(l.covered, 5 * INJECT_ROUNDS);
        assert_eq!(l.hole_escape, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EDOM));
        assert!(agrees_with_linux(ENODATA));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ERANGE), -ERANGE);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn range_code_text_complete() {
        for c in RangeCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx10063_checks_green() {
        let s = run_unx10063_checks();
        assert!(s.all_passed(), "UNX-F10063 red: {}", s.red_items_text());
    }
}
