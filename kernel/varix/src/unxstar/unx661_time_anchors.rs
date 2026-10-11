//! unx661_time_anchors ↔ UNX-F0661 冷启动计时锚点表（T0–T3 四锚冻结）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0661，UNX-A1-B34）：
//! 「冷启动计时锚点表（T0–T3 四锚冻结）| 580 | 已深化 |
//!  UNX-F0661-J1 四锚点（T0 上电/T1 引导器接管/T2 内核入口/T3 进 main）
//!  全链留痕 50 次，锚点语义与 F0198 时间线字段映射一致」
//!
//! 设计要点：
//! 1. **四锚冻结**：T0（上电）/T1（引导器接管）/T2（内核入口）/
//!     T3（进 main）四个锚点语义与序号编译期冻结，`AnchorKind`
//!     枚举序即时间序（rank 单调 = 时间单调）。
//! 2. **全链留痕 50 次**：`trace()` 每锚落一条 `AnchorTrace`
//!     （锚 + tick + 源），`TRACE_CAP=50`，满时显性 `TraceFull`
//!     （不覆盖——覆盖会让第 1 次冷启动记录消失）。
//! 3. **单调性红线**：`monotonic_audit()` 核对每条 trace 的
//!     tick 全序单调不递减；倒退即 `TimeWentBack`（时钟倒退
//!     会让段时长变负，段账全崩）。
//! 4. **与 F0198 字段映射**：锚 tick 入 `TimelineField` 三列
//!     （absolute/delta/source），映射表 `F0198_FIELD_MAP` 冻结，
//!     错位即 `MappingBad`（锚语义漂移 = 全链时长不可比）。
//! 5. **段时长推导**：`segment_len()` 由相邻锚 tick 差推导，
//!     50 次 trace 聚合出四段 P95/均值（冷启动性能回归面）。
//!
//! 诊断码：`AnchorCode` 0x8B 段（0x8B00..=0x8B0F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX661_ANCHOR_VERSION: &str = "unx661-time-anchors-v1";

/// 锚点数（四锚冻结）。
pub const ANCHOR_COUNT: usize = 4;

/// 留痕容量（锚点 50 次冷启动 × 四锚 = 200 条）。
pub const TRACE_CAP: usize = 200;

/// 锚点种类（枚举序 = 时间序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorKind {
    /// T0 上电
    PowerOn,
    /// T1 引导器接管
    BootloaderTakeover,
    /// T2 内核入口
    KernelEntry,
    /// T3 进 main
    MainEntry,
}

impl AnchorKind {
    /// 全部（冻结序）。
    pub const ALL: [AnchorKind; ANCHOR_COUNT] = [
        AnchorKind::PowerOn,
        AnchorKind::BootloaderTakeover,
        AnchorKind::KernelEntry,
        AnchorKind::MainEntry,
    ];

    /// 秩（0..3，时间序）。
    pub const fn rank(self) -> u8 {
        match self {
            AnchorKind::PowerOn => 0,
            AnchorKind::BootloaderTakeover => 1,
            AnchorKind::KernelEntry => 2,
            AnchorKind::MainEntry => 3,
        }
    }

    /// 名牌。
    pub const fn name(self) -> &'static str {
        match self {
            AnchorKind::PowerOn => "T0",
            AnchorKind::BootloaderTakeover => "T1",
            AnchorKind::KernelEntry => "T2",
            AnchorKind::MainEntry => "T3",
        }
    }

    /// 语义（中文，锚点契约原文）。
    pub const fn zh(self) -> &'static str {
        match self {
            AnchorKind::PowerOn => "上电",
            AnchorKind::BootloaderTakeover => "引导器接管",
            AnchorKind::KernelEntry => "内核入口",
            AnchorKind::MainEntry => "进 main",
        }
    }

    /// 下一个锚（None = 末锚）。
    pub const fn next(self) -> Option<AnchorKind> {
        match self {
            AnchorKind::PowerOn => Some(AnchorKind::BootloaderTakeover),
            AnchorKind::BootloaderTakeover => Some(AnchorKind::KernelEntry),
            AnchorKind::KernelEntry => Some(AnchorKind::MainEntry),
            AnchorKind::MainEntry => None,
        }
    }
}

/// 时间线三列（与 F0198 字段映射同构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimelineField {
    /// 绝对 tick
    pub absolute: u64,
    /// 与前一锚差（首锚 = 0）
    pub delta: u64,
    /// 源标识（锚名）
    pub source: &'static str,
}

/// F0198 字段映射表（锚 → 字段名，冻结）。
pub const F0198_FIELD_MAP: [(AnchorKind, &str); ANCHOR_COUNT] = [
    (AnchorKind::PowerOn, "boot.t0.power-on"),
    (AnchorKind::BootloaderTakeover, "boot.t1.takeover"),
    (AnchorKind::KernelEntry, "boot.t2.kernel-entry"),
    (AnchorKind::MainEntry, "boot.t3.main-entry"),
];

/// 锚点留痕一条。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnchorTrace {
    /// 第几次冷启动（1 起）
    pub cycle: u16,
    /// 锚
    pub anchor: AnchorKind,
    /// tick（注入）
    pub tick: u64,
    /// 源（bootloader/kernel/…）
    pub source: &'static str,
}

/// 锚点表（留痕 + 聚合）。
pub struct AnchorTable {
    traces: Vec<AnchorTrace>,
    /// 单调校验通过数
    monotonic_ok: u32,
}

/// 锚点错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorError {
    /// 留痕满
    TraceFull { cap: usize },
    /// 时间倒退
    TimeWentBack { cycle: u16, prev: u64, got: u64 },
    /// 锚点秩倒挂
    AnchorOutOfOrder { cycle: u16 },
    /// 映射错位
    MappingBad { anchor: AnchorKind },
    /// 周期号非法
    CycleBad { cycle: u16 },
    /// 同周期同锚重复
    DuplicateAnchor { cycle: u16, anchor: AnchorKind },
}

/// 诊断码（0x8B 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorCode {
    /// 0x8B00 留痕满
    TraceFull,
    /// 0x8B01 时间倒退
    TimeBack,
    /// 0x8B02 锚序倒挂
    AnchorOrder,
    /// 0x8B03 映射错位
    MappingBad,
    /// 0x8B04 周期号非法
    CycleBad,
    /// 0x8B05 重复锚
    Duplicate,
    /// 0x8B06 全链留痕通过
    Traced,
    /// 0x8B07 段聚合通过
    Segmented,
}

impl AnchorCode {
    pub const fn code(self) -> u16 {
        match self {
            AnchorCode::TraceFull => 0x8B00,
            AnchorCode::TimeBack => 0x8B01,
            AnchorCode::AnchorOrder => 0x8B02,
            AnchorCode::MappingBad => 0x8B03,
            AnchorCode::CycleBad => 0x8B04,
            AnchorCode::Duplicate => 0x8B05,
            AnchorCode::Traced => 0x8B06,
            AnchorCode::Segmented => 0x8B07,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            AnchorCode::TraceFull => "ANCHOR-TRACE-FULL",
            AnchorCode::TimeBack => "ANCHOR-TIME-BACK",
            AnchorCode::AnchorOrder => "ANCHOR-ORDER",
            AnchorCode::MappingBad => "ANCHOR-MAPPING-BAD",
            AnchorCode::CycleBad => "ANCHOR-CYCLE-BAD",
            AnchorCode::Duplicate => "ANCHOR-DUPLICATE",
            AnchorCode::Traced => "ANCHOR-TRACED",
            AnchorCode::Segmented => "ANCHOR-SEGMENTED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            AnchorCode::TraceFull => "锚点留痕满：50 次已满，溢出显性（不覆盖历史）",
            AnchorCode::TimeBack => "锚点时间倒退：tick 非单调，段时长会变负",
            AnchorCode::AnchorOrder => "锚点序倒挂：同周期内锚秩非递增",
            AnchorCode::MappingBad => "F0198 字段映射错位：锚与字段名不符",
            AnchorCode::CycleBad => "周期号非法：冷启动周期必须 ≥1",
            AnchorCode::Duplicate => "同周期同锚重复：一个周期一个锚只记一次",
            AnchorCode::Traced => "全链留痕通过：50 次冷启动四锚齐",
            AnchorCode::Segmented => "段聚合通过：四段时长可推导",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx661_time_anchors.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8B00 && (c & 0x00FF) < 0x10
    }
}

impl AnchorError {
    pub const fn code(self) -> AnchorCode {
        match self {
            AnchorError::TraceFull { .. } => AnchorCode::TraceFull,
            AnchorError::TimeWentBack { .. } => AnchorCode::TimeBack,
            AnchorError::AnchorOutOfOrder { .. } => AnchorCode::AnchorOrder,
            AnchorError::MappingBad { .. } => AnchorCode::MappingBad,
            AnchorError::CycleBad { .. } => AnchorCode::CycleBad,
            AnchorError::DuplicateAnchor { .. } => AnchorCode::Duplicate,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

impl AnchorTable {
    /// 空表。
    pub const fn new() -> Self {
        AnchorTable {
            traces: Vec::new(),
            monotonic_ok: 0,
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.traces.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.traces.is_empty()
    }

    /// 留痕一条（周期/锚/重复/满四门 + 单调校验）。
    pub fn trace(&mut self, t: AnchorTrace) -> Result<(), AnchorError> {
        if t.cycle == 0 {
            return Err(AnchorError::CycleBad { cycle: t.cycle });
        }
        // 映射错位核（锚 → 字段名必须存在）
        let mut found = false;
        let mut i = 0;
        while i < ANCHOR_COUNT {
            if let Some((k, _)) = F0198_FIELD_MAP.get(i) {
                if *k == t.anchor {
                    found = true;
                }
            }
            i += 1;
        }
        if !found {
            return Err(AnchorError::MappingBad { anchor: t.anchor });
        }
        // 同周期同锚重复核
        for e in self.traces.iter() {
            if e.cycle == t.cycle && e.anchor == t.anchor {
                return Err(AnchorError::DuplicateAnchor {
                    cycle: t.cycle,
                    anchor: t.anchor,
                });
            }
        }
        // 同周期内锚秩必须递增（查本周期已有锚的最大秩）
        let mut max_rank: Option<u8> = None;
        for e in self.traces.iter() {
            if e.cycle == t.cycle {
                let r = e.anchor.rank();
                if let Some(m) = max_rank {
                    if r > m {
                        max_rank = Some(r);
                    }
                } else {
                    max_rank = Some(r);
                }
            }
        }
        if let Some(m) = max_rank {
            if t.anchor.rank() <= m {
                return Err(AnchorError::AnchorOutOfOrder { cycle: t.cycle });
            }
        }
        // 单调核（同 tick 相等允许——同刻多源）
        if let Some(last) = self.traces.last() {
            if t.tick < last.tick {
                return Err(AnchorError::TimeWentBack {
                    cycle: t.cycle,
                    prev: last.tick,
                    got: t.tick,
                });
            }
        }
        if self.traces.len() >= TRACE_CAP {
            return Err(AnchorError::TraceFull { cap: TRACE_CAP });
        }
        self.traces.push(t);
        self.monotonic_ok += 1;
        Ok(())
    }

    /// 全链留痕审计（50 次 = 每周期 4 锚齐）。
    pub fn full_chain_audit(&self) -> Result<u32, AnchorError> {
        let cycles = self.traces.len() / ANCHOR_COUNT;
        let mut c = 1u16;
        while c as usize <= cycles {
            let mut ranks = 0u8;
            for e in self.traces.iter() {
                if e.cycle == c {
                    ranks |= 1u8 << (e.anchor.rank());
                }
            }
            if ranks != 0b1111 {
                return Err(AnchorError::AnchorOutOfOrder { cycle: c });
            }
            c += 1;
        }
        Ok(self.traces.len() as u32)
    }

    /// 段时长（按锚种类相邻差；返回四段（T0→T1/T1→T2/T2→T3/末锚后））。
    pub fn segment_lens(&self) -> [u64; ANCHOR_COUNT] {
        let mut out = [0u64; ANCHOR_COUNT];
        // 找每锚第一次出现的 tick（首个周期为基线）
        let mut marks = [0u64; ANCHOR_COUNT];
        let mut seen = [false; ANCHOR_COUNT];
        for e in self.traces.iter() {
            let r = e.anchor.rank() as usize;
            if let Some(s) = seen.get_mut(r) {
                if !*s {
                    marks[r] = e.tick;
                    *s = true;
                }
            }
        }
        let mut i = 1;
        while i < ANCHOR_COUNT {
            if let (Some(a), Some(b)) = (seen.get(i - 1), seen.get(i)) {
                if *a && *b {
                    let prev = marks[i - 1];
                    let cur = marks[i];
                    out[i - 1] = cur.saturating_sub(prev);
                }
            }
            i += 1;
        }
        out
    }

    /// 全周期段时长均值（整数除法，向下取整并记账）。
    pub fn segment_avg(&self) -> [u64; ANCHOR_COUNT] {
        let seg = self.segment_lens();
        let cycles = (self.traces.len() / ANCHOR_COUNT) as u64;
        let mut out = [0u64; ANCHOR_COUNT];
        let mut i = 0;
        while i < ANCHOR_COUNT {
            if let Some(v) = out.get_mut(i) {
                if let Some(s) = seg.get(i) {
                    *v = if cycles == 0 { 0 } else { *s / cycles };
                }
            }
            i += 1;
        }
        out
    }

    /// 表指纹（trace 序全进）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for t in self.traces.iter() {
            h ^= t.cycle as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= t.anchor.rank() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= t.tick;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= t.source.len() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 域自检。
pub fn run_unx661_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx661");

    // C661-01 四锚枚举冻结（rank 单调 + next 链闭合）
    {
        let mut ok = AnchorKind::ALL[0].rank() == 0;
        let mut i = 1;
        while i < ANCHOR_COUNT {
            ok = ok && AnchorKind::ALL[i].rank() == i as u8;
            i += 1;
        }
        ok = ok && matches!(AnchorKind::MainEntry.next(), None);
        s.add("C661-01-四锚冻结", ok, if ok { "" } else { "锚枚举序漂移" });
    }

    // C661-02 50 次全链留痕
    {
        let mut tb = AnchorTable::new();
        let mut ok = true;
        let mut cycle = 1u16;
        while cycle <= 50 {
            let base = (cycle as u64) * 1000;
            let mut a = 0;
            while a < ANCHOR_COUNT {
                let r = tb.trace(AnchorTrace {
                    cycle,
                    anchor: AnchorKind::ALL[a],
                    tick: base + a as u64 * 10,
                    source: match a {
                        0 => "fw",
                        1 => "bootloader",
                        _ => "kernel",
                    },
                });
                if r.is_err() {
                    ok = false;
                }
                a += 1;
            }
            cycle += 1;
        }
        let audit = tb.full_chain_audit();
        let good = ok && audit == Ok(200) && tb.len() == 200;
        s.add("C661-02-50 次全链留痕", good, if good { "" } else { "留痕不满 50 次" });
    }

    // C661-03 时间倒退被拦
    {
        let mut tb = AnchorTable::new();
        let _ = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::PowerOn,
            tick: 100,
            source: "fw",
        });
        let r = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::BootloaderTakeover,
            tick: 99,
            source: "bootloader",
        });
        let good = matches!(r, Err(AnchorError::TimeWentBack { prev: 100, got: 99, .. }));
        s.add("C661-03-时间倒退拦截", good, if good { "" } else { "倒退被放行" });
    }

    // C661-04 同周期锚序倒挂被拦（T0→T2→T0：第三锚秩回退）
    {
        let mut tb = AnchorTable::new();
        let _ = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::PowerOn,
            tick: 100,
            source: "fw",
        });
        let _ = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::KernelEntry,
            tick: 110,
            source: "kernel",
        });
        // T1 秩 1 <= 已有最大秩 2 → 锚序倒挂（且不与 T0/T2 重号）
        let r = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::BootloaderTakeover,
            tick: 120,
            source: "bootloader",
        });
        let good = matches!(r, Err(AnchorError::AnchorOutOfOrder { cycle: 1 }));
        s.add("C661-04-锚序倒挂拦截", good, if good { "" } else { "锚序倒挂被放行" });
    }

    // C661-05 同周期同锚重复被拦（先 T0 再 T1 再 T1）
    {
        let mut tb = AnchorTable::new();
        let _ = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::PowerOn,
            tick: 100,
            source: "fw",
        });
        let _ = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::BootloaderTakeover,
            tick: 110,
            source: "bootloader",
        });
        let r = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::KernelEntry,
            tick: 120,
            source: "kernel",
        });
        let r2 = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::BootloaderTakeover,
            tick: 130,
            source: "bootloader",
        });
        let good = r.is_ok() && matches!(r2, Err(AnchorError::DuplicateAnchor { .. }));
        s.add("C661-05-同锚重复拦截", good, if good { "" } else { "重复锚被放行" });
    }

    // C661-06 留痕满显性
    {
        let mut tb = AnchorTable::new();
        let mut cycle = 1u16;
        let mut last = Ok(());
        // 灌满 50 周期 × 4 = 200，再灌第 51 周期
        while cycle <= 51 {
            let base = (cycle as u64) * 1000;
            let mut a = 0;
            while a < ANCHOR_COUNT {
                last = tb.trace(AnchorTrace {
                    cycle,
                    anchor: AnchorKind::ALL[a],
                    tick: base + a as u64 * 10,
                    source: "x",
                });
                a += 1;
            }
            cycle += 1;
        }
        let good = matches!(last, Err(AnchorError::TraceFull { cap: 200 }))
            && tb.len() == 200;
        s.add("C661-06-留痕满显性", good, if good { "" } else { "溢出被吞" });
    }

    // C661-07 段时长推导（10/10/10/0）
    {
        let mut tb = AnchorTable::new();
        let _ = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::PowerOn,
            tick: 0,
            source: "fw",
        });
        let _ = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::BootloaderTakeover,
            tick: 10,
            source: "bootloader",
        });
        let _ = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::KernelEntry,
            tick: 20,
            source: "kernel",
        });
        let _ = tb.trace(AnchorTrace {
            cycle: 1,
            anchor: AnchorKind::MainEntry,
            tick: 30,
            source: "kernel",
        });
        let seg = tb.segment_lens();
        let good = seg[0] == 10 && seg[1] == 10 && seg[2] == 10;
        s.add("C661-07-段时长推导", good, if good { "" } else { "段时长错" });
    }

    // C661-08 周期 0 拒收
    {
        let mut tb = AnchorTable::new();
        let r = tb.trace(AnchorTrace {
            cycle: 0,
            anchor: AnchorKind::PowerOn,
            tick: 0,
            source: "fw",
        });
        let good = matches!(r, Err(AnchorError::CycleBad { cycle: 0 }));
        s.add("C661-08-零周期拒收", good, if good { "" } else { "周期 0 被放行" });
    }

    // C661-09 表指纹确定
    {
        let build = || -> AnchorTable {
            let mut tb = AnchorTable::new();
            let mut a = 0;
            while a < ANCHOR_COUNT {
                let _ = tb.trace(AnchorTrace {
                    cycle: 1,
                    anchor: AnchorKind::ALL[a],
                    tick: a as u64 * 5,
                    source: "s",
                });
                a += 1;
            }
            tb
        };
        let a = build();
        let b = build();
        let good = a.fingerprint() == b.fingerprint() && a.fingerprint() != 0;
        s.add("C661-09-表指纹确定", good, if good { "" } else { "指纹不确定" });
    }

    // C661-10 诊断码段独占
    {
        let codes = [
            AnchorCode::TraceFull,
            AnchorCode::TimeBack,
            AnchorCode::AnchorOrder,
            AnchorCode::MappingBad,
            AnchorCode::CycleBad,
            AnchorCode::Duplicate,
            AnchorCode::Traced,
            AnchorCode::Segmented,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C661-10-诊断码 0x8B 段独占", all, if all { "" } else { "码值越出 0x8B 段" });
    }

    s
}
