//! unx721_limine_ledger ↔ UNX-F0721 Limine 版本台账（当前版本/上游跟踪/变更通知）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0721，UNX-A1-B37）：
//! 「Limine 版本台账（当前版本/上游跟踪/变更通知）| 590 | 已深化 |
//!  UNX-F0721-J1 版本台账三段齐（当前版本指纹/上游版本流/变更通知账）
//!  且当前版本指纹复算一致」
//!
//! 设计要点：
//! 1. **三段台账**：①当前版本（二进制指纹 + 版本四元组）
//!     ②上游版本流（跟踪到的上游版本序列，只增不减）
//!     ③变更通知账（每次版本变化的通知记录，append-only）。
//! 2. **当前版本指纹复算**：`binary_fingerprint()` 对引导器二进制
//!     内容做 FNV，与台账登记值比对；不一致 = 盘上二进制被换过
//!     （最常见事故：刷了新 Limine 忘了台账升版）。
//! 3. **上游版本流单调**：`UpstreamFlow` 只追加且版本号严格递增
//!     （降版本 = 回退，必须显性理由）；`append_upstream()`
//!     拒降级。
//! 4. **变更通知**：`notify()` 在版本切换时登记（旧版本/新版本/
//!     原因），通知账容量 32，满时显性。
//! 5. **落后判定**：`lag()` 计算当前版本与上游最新版本的差距
//!     （落后几个小版本），用于「何时该升级」的量化决策。
//!
//! 诊断码：`LimineCode` 0x8E 段（0x8E00..=0x8E0F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX721_LIMINE_VERSION: &str = "unx721-limine-ledger-v1";

/// 通知账容量。
pub const NOTIFY_CAP: usize = 32;

/// 上游版本流容量。
pub const UPSTREAM_CAP: usize = 16;

/// 版本四元组（major/minor/patch/build）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct SemVer {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub build: u32,
}

impl SemVer {
    /// 构造。
    pub const fn new(major: u16, minor: u16, patch: u16, build: u32) -> Self {
        SemVer {
            major,
            minor,
            patch,
            build,
        }
    }

    /// 序列化（线缆形态 X.Y.Z）。
    pub fn wire(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    /// 是否合法（非零大版本；0.0.0 拒收）。
    pub const fn is_valid(&self) -> bool {
        self.major != 0 || self.minor != 0 || self.patch != 0
    }

    /// 同主版本的次版本差（用于落后量化）。
    pub const fn minor_gap(self, other: SemVer) -> u16 {
        if self.major != other.major {
            return u16::MAX;
        }
        if other.minor > self.minor {
            other.minor - self.minor
        } else {
            0
        }
    }
}

/// 上游版本流条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UpstreamEntry {
    pub version: SemVer,
    /// 抓取 tick（注入）
    pub seen_tick: u64,
    /// 是否有安全公告（联动通知账）
    pub advisory: bool,
}

/// 上游版本流（单调递增，只增不减）。
pub struct UpstreamFlow {
    entries: Vec<UpstreamEntry>,
}

/// 变更通知一条。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NotifyEntry {
    /// 旧版本
    pub from: SemVer,
    /// 新版本
    pub to: SemVer,
    /// 变更原因（必填，禁空）
    pub reason: String,
    /// 通知 tick
    pub tick: u64,
}

/// 变更通知账（append-only）。
pub struct NotifyLedger {
    entries: Vec<NotifyEntry>,
}

/// 台账错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimineError {
    /// 版本非法（0.0.0）
    VersionInvalid,
    /// 指纹不符
    FingerprintMismatch { got: u64, want: u64 },
    /// 上游降级
    UpstreamDowngrade {
        got: SemVer,
        latest: SemVer,
    },
    /// 上游流满
    UpstreamFull { cap: usize },
    /// 通知账满
    NotifyFull { cap: usize },
    /// 通知原因为空
    ReasonEmpty,
    /// 变更版本相同
    NoOpChange,
    /// 台账三段不齐
    SegmentMissing { segment: &'static str },
}

/// 诊断码（0x8E 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimineCode {
    /// 0x8E00 版本非法
    VersionBad,
    /// 0x8E01 指纹不符
    FingerprintBad,
    /// 0x8E02 上游降级
    Downgrade,
    /// 0x8E03 上游流满
    UpstreamFull,
    /// 0x8E04 通知账满
    NotifyFull,
    /// 0x8E05 原因空
    ReasonEmpty,
    /// 0x8E06 空变更
    NoOp,
    /// 0x8E07 三段不齐
    SegmentBad,
    /// 0x8E08 台账通过
    Reconciled,
}

impl LimineCode {
    pub const fn code(self) -> u16 {
        match self {
            LimineCode::VersionBad => 0x8E00,
            LimineCode::FingerprintBad => 0x8E01,
            LimineCode::Downgrade => 0x8E02,
            LimineCode::UpstreamFull => 0x8E03,
            LimineCode::NotifyFull => 0x8E04,
            LimineCode::ReasonEmpty => 0x8E05,
            LimineCode::NoOp => 0x8E06,
            LimineCode::SegmentBad => 0x8E07,
            LimineCode::Reconciled => 0x8E08,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            LimineCode::VersionBad => "LIMINE-VERSION-BAD",
            LimineCode::FingerprintBad => "LIMINE-FINGERPRINT-BAD",
            LimineCode::Downgrade => "LIMINE-DOWNGRADE",
            LimineCode::UpstreamFull => "LIMINE-UPSTREAM-FULL",
            LimineCode::NotifyFull => "LIMINE-NOTIFY-FULL",
            LimineCode::ReasonEmpty => "LIMINE-REASON-EMPTY",
            LimineCode::NoOp => "LIMINE-NO-OP",
            LimineCode::SegmentBad => "LIMINE-SEGMENT-BAD",
            LimineCode::Reconciled => "LIMINE-RECONCILED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            LimineCode::VersionBad => "Limine 版本非法：0.0.0 不收",
            LimineCode::FingerprintBad => "二进制指纹不符：盘上引导器与台账登记不一致",
            LimineCode::Downgrade => "上游版本降级：只允许递增（回退必须显性理由）",
            LimineCode::UpstreamFull => "上游版本流满：16 条已满",
            LimineCode::NotifyFull => "变更通知账满：32 条已满",
            LimineCode::ReasonEmpty => "变更原因为空：版本切换必须带原因",
            LimineCode::NoOp => "空变更：新旧版本相同，无通知意义",
            LimineCode::SegmentBad => "台账三段不齐：当前版本/上游流/通知账缺一",
            LimineCode::Reconciled => "Limine 台账通过：三段齐 + 当前指纹复算一致",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx721_limine_ledger.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8E00 && (c & 0x00FF) < 0x10
    }
}

impl LimineError {
    pub const fn code(self) -> LimineCode {
        match self {
            LimineError::VersionInvalid => LimineCode::VersionBad,
            LimineError::FingerprintMismatch { .. } => LimineCode::FingerprintBad,
            LimineError::UpstreamDowngrade { .. } => LimineCode::Downgrade,
            LimineError::UpstreamFull { .. } => LimineCode::UpstreamFull,
            LimineError::NotifyFull { .. } => LimineCode::NotifyFull,
            LimineError::ReasonEmpty => LimineCode::ReasonEmpty,
            LimineError::NoOpChange => LimineCode::NoOp,
            LimineError::SegmentMissing { .. } => LimineCode::SegmentBad,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// FNV-1a 64（字节序列）。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl UpstreamFlow {
    pub const fn new() -> Self {
        UpstreamFlow { entries: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 追加一个上游版本（拒降级；同版本幂等）。
    pub fn append_upstream(&mut self, e: UpstreamEntry) -> Result<(), LimineError> {
        if !e.version.is_valid() {
            return Err(LimineError::VersionInvalid);
        }
        if let Some(last) = self.entries.last() {
            if e.version < last.version {
                return Err(LimineError::UpstreamDowngrade {
                    got: e.version,
                    latest: last.version,
                });
            }
            if e.version == last.version {
                return Ok(());
            }
        }
        if self.entries.len() >= UPSTREAM_CAP {
            return Err(LimineError::UpstreamFull { cap: UPSTREAM_CAP });
        }
        self.entries.push(e);
        Ok(())
    }

    /// 上游最新版本（None = 空流）。
    pub fn latest(&self) -> Option<SemVer> {
        match self.entries.last() {
            Some(e) => Some(e.version),
            None => None,
        }
    }

    /// 安全公告条数。
    pub fn advisory_count(&self) -> u32 {
        let mut n = 0u32;
        for e in self.entries.iter() {
            if e.advisory {
                n += 1;
            }
        }
        n
    }
}

impl NotifyLedger {
    pub const fn new() -> Self {
        NotifyLedger { entries: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 记一条变更通知（空变更/空原因/账满三门）。
    pub fn notify(&mut self, e: NotifyEntry) -> Result<(), LimineError> {
        if e.from == e.to {
            return Err(LimineError::NoOpChange);
        }
        if e.reason.is_empty() {
            return Err(LimineError::ReasonEmpty);
        }
        if self.entries.len() >= NOTIFY_CAP {
            return Err(LimineError::NotifyFull { cap: NOTIFY_CAP });
        }
        self.entries.push(e);
        Ok(())
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for e in self.entries.iter() {
            for b in e.reason.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h ^= e.from.minor as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= e.to.minor as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= e.tick;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// Limine 版本台账（三段合一）。
pub struct LimineLedger {
    /// 当前版本
    pub current: SemVer,
    /// 当前版本二进制指纹（登记值）
    pub current_fp: u64,
    /// 上游版本流
    pub upstream: UpstreamFlow,
    /// 变更通知账
    pub notify: NotifyLedger,
    /// 三段齐否
    segments_ok: bool,
}

impl LimineLedger {
    pub fn new(current: SemVer, current_fp: u64) -> Result<Self, LimineError> {
        if !current.is_valid() {
            return Err(LimineError::VersionInvalid);
        }
        Ok(LimineLedger {
            current,
            current_fp,
            upstream: UpstreamFlow::new(),
            notify: NotifyLedger::new(),
            segments_ok: false,
        })
    }

    /// 盘上二进制指纹复算核对。
    pub fn verify_binary(&self, bytes: &[u8]) -> Result<(), LimineError> {
        let got = fnv1a64(bytes);
        if got != self.current_fp {
            return Err(LimineError::FingerprintMismatch {
                got,
                want: self.current_fp,
            });
        }
        Ok(())
    }

    /// 切换当前版本（必须走通知账）。
    pub fn switch_to(
        &mut self,
        to: SemVer,
        new_fp: u64,
        reason: &str,
        tick: u64,
    ) -> Result<(), LimineError> {
        if !to.is_valid() {
            return Err(LimineError::VersionInvalid);
        }
        let from = self.current;
        self.notify.notify(NotifyEntry {
            from,
            to,
            reason: String::from(reason),
            tick,
        })?;
        self.current = to;
        self.current_fp = new_fp;
        Ok(())
    }

    /// 落后量化（当前 vs 上游最新的次版本差；None = 无上游数据）。
    pub fn lag(&self) -> Option<u16> {
        match self.upstream.latest() {
            Some(u) => Some(self.current.minor_gap(u)),
            None => None,
        }
    }

    /// 三段齐判定。
    pub fn mark_segments(&mut self) -> Result<(), LimineError> {
        if self.current_fp == 0 {
            return Err(LimineError::SegmentMissing {
                segment: "current",
            });
        }
        if self.upstream.is_empty() {
            return Err(LimineError::SegmentMissing {
                segment: "upstream",
            });
        }
        self.segments_ok = true;
        Ok(())
    }

    pub const fn segments_ok(&self) -> bool {
        self.segments_ok
    }
}

/// 域自检。
pub fn run_unx721_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx721");

    // C721-01 四元组合法性与线缆形态
    {
        let v = SemVer::new(8, 2, 1, 5);
        let good = v.is_valid() && v.wire() == "8.2.1";
        s.add("C721-01-版本形态", good, if good { "" } else { "版本序列化错" });
    }

    // C721-02 指纹复算一致
    {
        let bytes = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let fp = fnv1a64(&bytes);
        let good = match LimineLedger::new(SemVer::new(8, 2, 1, 0), fp) {
            Ok(l) => l.verify_binary(&bytes).is_ok(),
            Err(_) => false,
        };
        s.add("C721-02-指纹复算一致", good, if good { "" } else { "复算不符" });
    }

    // C721-03 指纹不符被拦
    {
        let bytes = [1u8, 2, 3];
        let fp = fnv1a64(&bytes);
        let bad = [9u8, 9, 9];
        let good = match LimineLedger::new(SemVer::new(8, 2, 1, 0), fp) {
            Ok(l) => l.verify_binary(&bad).is_err(),
            Err(_) => false,
        };
        s.add("C721-03-指纹不符拦截", good, if good { "" } else { "不符被放行" });
    }

    // C721-04 0.0.0 拒收
    {
        let l = LimineLedger::new(SemVer::new(0, 0, 0, 0), 1);
        let good = matches!(l, Err(LimineError::VersionInvalid));
        s.add("C721-04-零版本拒收", good, if good { "" } else { "零版本被放行" });
    }

    // C721-05 上游流单调递增 + 降级拒
    {
        let mut f = UpstreamFlow::new();
        let r1 = f.append_upstream(UpstreamEntry {
            version: SemVer::new(8, 1, 0, 0),
            seen_tick: 0,
            advisory: false,
        });
        let r2 = f.append_upstream(UpstreamEntry {
            version: SemVer::new(8, 2, 0, 0),
            seen_tick: 10,
            advisory: true,
        });
        let r3 = f.append_upstream(UpstreamEntry {
            version: SemVer::new(8, 1, 0, 0),
            seen_tick: 20,
            advisory: false,
        });
        let good = r1.is_ok()
            && r2.is_ok()
            && matches!(
                r3,
                Err(LimineError::UpstreamDowngrade {
                    got: SemVer {
                        major: 8,
                        minor: 1,
                        patch: 0,
                        build: 0
                    },
                    ..
                })
            )
            && f.latest() == Some(SemVer::new(8, 2, 0, 0))
            && f.advisory_count() == 1;
        s.add("C721-05-上游单调与降级拦", good, if good { "" } else { "上游流纪律失效" });
    }

    // C721-06 上游流满显性
    {
        let mut f = UpstreamFlow::new();
        let mut i = 0u16;
        let mut last = Ok(());
        while i < UPSTREAM_CAP as u16 + 3 {
            last = f.append_upstream(UpstreamEntry {
                version: SemVer::new(8, i, 0, 0),
                seen_tick: 0,
                advisory: false,
            });
            i += 1;
        }
        let good = matches!(last, Err(LimineError::UpstreamFull { cap: 16 })) && f.len() == 16;
        s.add("C721-06-上游流满显性", good, if good { "" } else { "满流被吞" });
    }

    // C721-07 变更通知（空变更/空原因拒）
    {
        let bytes = [7u8; 8];
        let fp = fnv1a64(&bytes);
        let mut l = match LimineLedger::new(SemVer::new(8, 2, 1, 0), fp) {
            Ok(l) => l,
            Err(_) => {
                s.add("C721-07-构造失败", false, "ledger new failed");
                return s;
            }
        };
        let r1 = l.switch_to(SemVer::new(8, 2, 1, 0), fp, "noop", 0);
        let r2 = l.switch_to(SemVer::new(8, 3, 0, 0), 12345, "security", 5);
        let r3 = l.switch_to(SemVer::new(8, 4, 0, 0), 12346, "", 6);
        let good = matches!(r1, Err(LimineError::NoOpChange))
            && r2.is_ok()
            && matches!(r3, Err(LimineError::ReasonEmpty))
            && l.notify.len() == 1
            && l.current == SemVer::new(8, 3, 0, 0);
        s.add("C721-07-变更通知三门", good, if good { "" } else { "变更通知失效" });
    }

    // C721-08 通知账满显性
    {
        let mut n = NotifyLedger::new();
        let mut i = 0u16;
        let mut last = Ok(());
        while i < NOTIFY_CAP as u16 + 2 {
            last = n.notify(NotifyEntry {
                from: SemVer::new(8, i, 0, 0),
                to: SemVer::new(8, i + 1, 0, 0),
                reason: String::from("tick"),
                tick: i as u64,
            });
            i += 1;
        }
        let good = matches!(last, Err(LimineError::NotifyFull { cap: 32 })) && n.len() == 32;
        s.add("C721-08-通知账满显性", good, if good { "" } else { "满账被吞" });
    }

    // C721-09 落后量化
    {
        let mut l = match LimineLedger::new(SemVer::new(8, 2, 0, 0), fnv1a64(&[1u8])) {
            Ok(l) => l,
            Err(_) => {
                s.add("C721-09-构造失败", false, "ledger new failed");
                return s;
            }
        };
        let _ = l.upstream.append_upstream(UpstreamEntry {
            version: SemVer::new(8, 5, 0, 0),
            seen_tick: 0,
            advisory: false,
        });
        let good = l.lag() == Some(3);
        s.add("C721-09-落后量化", good, if good { "" } else { "落后度算错" });
    }

    // C721-10 三段齐判定（缺上游被拒）
    {
        let mut l0 = match LimineLedger::new(SemVer::new(8, 2, 0, 0), 123) {
            Ok(l) => l,
            Err(_) => {
                s.add("C721-10-构造失败", false, "ledger new failed");
                return s;
            }
        };
        let r0 = l0.mark_segments();
        let mut l2 = match LimineLedger::new(SemVer::new(8, 2, 0, 0), 123) {
            Ok(l) => l,
            Err(_) => {
                s.add("C721-10-构造失败2", false, "ledger new failed");
                return s;
            }
        };
        let _ = l2.upstream.append_upstream(UpstreamEntry {
            version: SemVer::new(8, 2, 0, 0),
            seen_tick: 0,
            advisory: false,
        });
        let r1 = l2.mark_segments();
        let good = matches!(
            r0,
            Err(LimineError::SegmentMissing {
                segment: "upstream"
            })
        ) && r1.is_ok()
            && l2.segments_ok();
        s.add("C721-10-三段齐判定", good, if good { "" } else { "三段门失效" });
    }

    // C721-11 诊断码段独占
    {
        let codes = [
            LimineCode::VersionBad,
            LimineCode::FingerprintBad,
            LimineCode::Downgrade,
            LimineCode::UpstreamFull,
            LimineCode::NotifyFull,
            LimineCode::ReasonEmpty,
            LimineCode::NoOp,
            LimineCode::SegmentBad,
            LimineCode::Reconciled,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C721-11-诊断码 0x8E 段独占", all, if all { "" } else { "码值越出 0x8E 段" });
    }

    s
}
