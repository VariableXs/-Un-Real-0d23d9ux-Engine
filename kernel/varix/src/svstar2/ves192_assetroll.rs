#![allow(dead_code)]
use alloc::vec;
// VE-F3938 · 无障碍性能资产汇总
//
// 资产总册（P09 模式六用）：成本模型/基准/门禁看板/降档矩阵/长稳档案五资产
// + 心流册 + 断链册 + 辅具功耗册——九资产指针+状态。
// 符合度快照（九源+断链率+下限——超限登记）；四热点档案（共享率/线性度/降档频度/重锚效果）
// ——断链热点替代说明（断链即 P0 不入热点——声明红线）。
// 错误路径：超限存量→两出口、指针失效→重链、断链入热点→台账修正、快照分歧→实测。


extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xD200）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDFCode(pub u16);

impl CDFCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const OVER_LIMIT: Self = Self(SEG_BASE + 1);
    pub const POINTER_DEAD: Self = Self(SEG_BASE + 2);
    pub const BREAK_IN_HOT: Self = Self(SEG_BASE + 3);
    pub const SNAPSHOT_DIVERGE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::OVER_LIMIT => "超限存量→两出口（降档/挂账）",
            Self::POINTER_DEAD => "指针失效→重链",
            Self::BREAK_IN_HOT => "断链入热点→台账修正（红线）",
            Self::SNAPSHOT_DIVERGE => "快照分歧→实测",
            _ => "未知",
        }
    }
}

// ── 九资产指针 ──────────────────────────────────────────────────────

/// 九资产。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    CostModel,
    Baseline,
    GateBoard,
    TierMatrix,
    LongRun,
    FlowBook,
    BreakBook,
    AtPower,
    Ledger,
}

impl AssetKind {
    pub const ALL: [AssetKind; 9] = [
        AssetKind::CostModel,
        AssetKind::Baseline,
        AssetKind::GateBoard,
        AssetKind::TierMatrix,
        AssetKind::LongRun,
        AssetKind::FlowBook,
        AssetKind::BreakBook,
        AssetKind::AtPower,
        AssetKind::Ledger,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::CostModel => "成本模型",
            Self::Baseline => "基准",
            Self::GateBoard => "门禁看板",
            Self::TierMatrix => "降档矩阵",
            Self::LongRun => "长稳档案",
            Self::FlowBook => "心流册",
            Self::BreakBook => "断链册",
            Self::AtPower => "辅具功耗册",
            Self::Ledger => "总账",
        }
    }
}

/// 资产指针。
#[derive(Debug, Clone, Copy)]
pub struct AssetPointer {
    pub kind: AssetKind,
    /// 指针有效。
    pub alive: bool,
    /// 资产状态就绪。
    pub ready: bool,
}

/// 资产总册。
#[derive(Debug, Clone)]
pub struct AssetBook {
    pub pointers: Vec<AssetPointer>,
}

impl AssetBook {
    /// 九指针全登记。
    pub fn full() -> Self {
        Self {
            pointers: AssetKind::ALL
                .iter()
                .map(|k| AssetPointer {
                    kind: *k,
                    alive: true,
                    ready: true,
                })
                .collect(),
        }
    }

    pub fn count(&self) -> usize {
        self.pointers.len()
    }

    /// 失效指针。
    pub fn dead(&self) -> Vec<AssetKind> {
        self.pointers
            .iter()
            .filter(|p| !p.alive)
            .map(|p| p.kind)
            .collect()
    }

    /// 重链：失效指针复活。
    pub fn relink(&mut self) -> usize {
        let mut n = 0;
        for p in self.pointers.iter_mut() {
            if !p.alive {
                p.alive = true;
                n += 1;
            }
        }
        n
    }
}

// ── 符合度快照（九源+断链率+下限）───────────────────────────────────

/// 单源快照。
#[derive(Debug, Clone, Copy)]
pub struct SourceSnap {
    pub kind: AssetKind,
    /// 达标率（0-100）。
    pub pct: u32,
}

/// 快照表。
#[derive(Debug, Clone)]
pub struct SnapTable {
    pub snaps: Vec<SourceSnap>,
    /// 断链率（万分比，硬门 0）。
    pub break_ppm: u32,
    /// 预算下限（µs，全部源不得低于）。
    pub floor_us: u32,
    /// 各源实测（µs）。
    pub measured_us: Vec<u32>,
}

impl SnapTable {
    /// 下限入快：全部实测不低于下限。
    pub fn floor_ok(&self) -> bool {
        self.measured_us.iter().all(|m| *m >= self.floor_us)
    }

    /// 断链率硬门。
    pub fn break_ok(&self) -> bool {
        self.break_ppm == 0
    }

    /// 超限源（达标率低于 90）。
    pub fn over_limit(&self) -> Vec<AssetKind> {
        self.snaps
            .iter()
            .filter(|s| s.pct < 90)
            .map(|s| s.kind)
            .collect()
    }
}

/// 超限两出口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// 降档（当前档位可承受）。
    TierDown,
    /// 挂账（限期修复）。
    Track,
}

pub fn pick_exit(pct: u32) -> Exit {
    if pct >= 70 {
        Exit::TierDown
    } else {
        Exit::Track
    }
}

// ── 四热点档案（断链除外声明）───────────────────────────────────────

/// 热点维度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotDim {
    /// 共享率。
    Sharing,
    /// 线性度。
    Linearity,
    /// 降档频度。
    TierFreq,
    /// 重锚效果。
    Reanchor,
}

impl HotDim {
    pub const ALL: [HotDim; 4] = [
        HotDim::Sharing,
        HotDim::Linearity,
        HotDim::TierFreq,
        HotDim::Reanchor,
    ];
}

/// 热点条目。
#[derive(Debug, Clone, Copy)]
pub struct HotSpot {
    pub dim: HotDim,
    /// 分值（0-100）。
    pub score: u32,
}

/// 热点档案（断链即 P0 不入热点——声明红线）。
#[derive(Debug, Clone)]
pub struct HotBook {
    pub spots: Vec<HotSpot>,
    /// 声明：断链热点被排除的次数（红线实测）。
    pub excluded_breaks: u32,
}

impl HotBook {
    pub fn new() -> Self {
        Self {
            spots: Vec::new(),
            excluded_breaks: 0,
        }
    }

    /// 入档：断链类条目拒绝入档。
    pub fn admit(&mut self, spot: HotSpot, is_break: bool) -> bool {
        if is_break {
            self.excluded_breaks += 1;
            return false;
        }
        self.spots.push(spot);
        true
    }

    /// 四维齐备。
    pub fn dims_full(&self) -> bool {
        HotDim::ALL
            .iter()
            .all(|d| self.spots.iter().any(|s| s.dim == *d))
    }
}

impl Default for HotBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 曲线健康 ────────────────────────────────────────────────────────

/// 曲线健康：长稳采样单调不劣化（漂移带内）。
pub fn curve_healthy(samples: &[u32], drift_ppm: u32) -> bool {
    if samples.len() < 2 {
        return true;
    }
    for w in samples.windows(2) {
        if w[1] > w[0] {
            // u64 全程：u32 差值 × 1_000_000 会溢出 u32，导致大样本误判「健康」。
            let grow = (w[1] - w[0]) as u64 * 1_000_000 / (w[0].max(1) as u64);
            if grow > drift_ppm as u64 {
                return false;
            }
        }
    }
    true
}

// ── 汇总会话（总控）─────────────────────────────────────────────────

/// 资产汇总会话。
#[derive(Debug, Clone)]
pub struct AssetSession {
    pub book: AssetBook,
    pub snaps: SnapTable,
    pub hot: HotBook,
}

impl AssetSession {
    pub fn new() -> Self {
        Self {
            book: AssetBook::full(),
            snaps: SnapTable {
                snaps: Vec::new(),
                break_ppm: 0,
                floor_us: 300,
                measured_us: Vec::new(),
            },
            hot: HotBook::new(),
        }
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CDFCode> {
        if !self.book.dead().is_empty() {
            return Err(CDFCode::POINTER_DEAD);
        }
        if self.snaps.over_limit().len() >= self.snaps.snaps.len() && !self.snaps.snaps.is_empty() {
            return Err(CDFCode::OVER_LIMIT);
        }
        if !self.snaps.break_ok() {
            return Err(CDFCode::OVER_LIMIT);
        }
        if !self.snaps.floor_ok() {
            return Err(CDFCode::SNAPSHOT_DIVERGE);
        }
        if self.hot.excluded_breaks > 0 && self.hot.dims_full() {
            return Err(CDFCode::BREAK_IN_HOT);
        }
        Ok(())
    }
}

impl Default for AssetSession {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves192_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves192_assetroll");

    // 01 九资产齐备
    {
        let b = AssetBook::full();
        chk(&mut set, "C3938-01", b.count() == 9, "err");
    }
    // 02 资产定名
    {
        chk(
            &mut set,
            "C3938-02",
            AssetKind::ALL.iter().all(|k| !k.name().is_empty()),
            "err",
        );
    }
    // 03 指针失效检出与重链
    {
        let mut b = AssetBook::full();
        b.pointers[2].alive = false;
        b.pointers[5].alive = false;
        let dead = b.dead();
        let n = b.relink();
        chk(
            &mut set,
            "C3938-03",
            dead.len() == 2 && n == 2 && b.dead().is_empty(),
            "err",
        );
    }
    // 04 断链率硬门
    {
        let mut s = AssetSession::new();
        s.snaps.break_ppm = 0;
        chk(&mut set, "C3938-04", s.snaps.break_ok(), "err");
    }
    // 05 断链非零→超限
    {
        let mut s = AssetSession::new();
        s.snaps.break_ppm = 12;
        chk(
            &mut set,
            "C3938-05",
            !s.snaps.break_ok() && s.full_check() == Err(CDFCode::OVER_LIMIT),
            "err",
        );
    }
    // 06 超限源检出
    {
        let mut s = AssetSession::new();
        s.snaps.snaps.push(SourceSnap { kind: AssetKind::Baseline, pct: 95 });
        s.snaps.snaps.push(SourceSnap { kind: AssetKind::GateBoard, pct: 82 });
        let over = s.snaps.over_limit();
        chk(
            &mut set,
            "C3938-06",
            over == vec![AssetKind::GateBoard],
            "err",
        );
    }
    // 07 超限两出口：≥70 降档
    {
        chk(&mut set, "C3938-07", pick_exit(75) == Exit::TierDown, "err");
    }
    // 08 超限两出口：<70 挂账
    {
        chk(&mut set, "C3938-08", pick_exit(50) == Exit::Track, "err");
    }
    // 09 下限入快：全过
    {
        let mut s = AssetSession::new();
        s.snaps.measured_us = vec![320, 410, 300];
        chk(&mut set, "C3938-09", s.snaps.floor_ok(), "err");
    }
    // 10 下限破→快照分歧
    {
        let mut s = AssetSession::new();
        s.snaps.measured_us = vec![320, 299];
        chk(
            &mut set,
            "C3938-10",
            !s.snaps.floor_ok() && s.full_check() == Err(CDFCode::SNAPSHOT_DIVERGE),
            "err",
        );
    }
    // 11 断链不入热点（声明红线）
    {
        let mut h = HotBook::new();
        let ok = h.admit(
            HotSpot {
                dim: HotDim::Sharing,
                score: 88,
            },
            false,
        );
        let bad = h.admit(
            HotSpot {
                dim: HotDim::Linearity,
                score: 99,
            },
            true,
        );
        chk(
            &mut set,
            "C3938-11",
            ok && !bad && h.excluded_breaks == 1 && h.spots.len() == 1,
            "err",
        );
    }
    // 12 断链入热点→台账修正路径
    {
        let mut s = AssetSession::new();
        let _ = s.hot.admit(HotSpot { dim: HotDim::Sharing, score: 1 }, true);
        chk(
            &mut set,
            "C3938-12",
            s.hot.excluded_breaks == 1 && s.full_check() == Ok(()),
            "err",
        );
    }
    // 13 四维热点齐备
    {
        let mut h = HotBook::new();
        for (i, d) in HotDim::ALL.iter().enumerate() {
            h.admit(
                HotSpot {
                    dim: *d,
                    score: 80 + i as u32,
                },
                false,
            );
        }
        chk(&mut set, "C3938-13", h.dims_full(), "err");
    }
    // 14 曲线健康：平稳
    {
        chk(&mut set, "C3938-14", curve_healthy(&[300, 301, 300], 10_000), "err");
    }
    // 15 曲线健康：漂移超带
    {
        chk(&mut set, "C3938-15", !curve_healthy(&[300, 330], 5_000), "err");
    }
    // 16 曲线健康：改善不判劣
    {
        chk(&mut set, "C3938-16", curve_healthy(&[330, 300], 0), "err");
    }
    // 17 会话总检全绿
    {
        let mut s = AssetSession::new();
        s.snaps.snaps.push(SourceSnap { kind: AssetKind::Baseline, pct: 96 });
        s.snaps.measured_us = vec![350, 400];
        chk(&mut set, "C3938-17", s.full_check() == Ok(()), "err");
    }
    // 18 诊断码互异
    {
        let codes = [
            CDFCode::OVER_LIMIT,
            CDFCode::POINTER_DEAD,
            CDFCode::BREAK_IN_HOT,
            CDFCode::SNAPSHOT_DIVERGE,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        chk(&mut set, "C3938-18", distinct, "err");
    }

    set
}
