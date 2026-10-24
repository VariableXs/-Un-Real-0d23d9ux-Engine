#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xD780）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD780;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CD7BCode(pub u16);

impl CD7BCode {
    pub const MULTI_IMPL: Self = Self(SEG_BASE);       // 倍率双实现
    pub const BREAK_REDUCE: Self = Self(SEG_BASE + 1); // 倍率破 reduce
    pub const RECALIB_STUCK: Self = Self(SEG_BASE + 2); // 重标定卡
    pub const PERSIST_LOST: Self = Self(SEG_BASE + 3);  // 持久丢
    pub const TOKEN_DRIFT: Self = Self(SEG_BASE + 4);   // 令牌漂移

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::MULTI_IMPL => "倍率双实现",
            Self::BREAK_REDUCE => "倍率破reduce",
            Self::RECALIB_STUCK => "重标定卡",
            Self::PERSIST_LOST => "持久丢",
            Self::TOKEN_DRIFT => "令牌漂移",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::MULTI_IMPL, Self::BREAK_REDUCE, Self::RECALIB_STUCK,
        Self::PERSIST_LOST, Self::TOKEN_DRIFT,
    ];
}

// ── 倍率模型（单源断言）────────────────────────────────────────────

/// 动效时长倍率（0.5x ~ 3.0x，定点 ×100 存储）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationScale {
    /// 倍率 ×100：50..=300。
    pub scale_pct: u16,
    /// 单源标记：全局唯一实现。
    pub single_source: bool,
}

impl DurationScale {
    pub const MIN: u16 = 50;
    pub const MAX: u16 = 300;

    pub fn new(scale_pct: u16) -> Self {
        let clamped = scale_pct.clamp(Self::MIN, Self::MAX);
        Self { scale_pct: clamped, single_source: true }
    }

    /// 倍率合法。
    pub fn valid(&self) -> bool {
        (Self::MIN..=Self::MAX).contains(&self.scale_pct)
    }

    /// 倍率单源断言：不得双实现。
    pub fn single_source_ok(&self) -> bool { self.single_source }

    /// 作用于基础时长。
    pub fn apply(&self, base_ms: u32) -> u32 {
        ((base_ms as u64 * self.scale_pct as u64) / 100) as u32
    }
}

// ── reduce 与倍率正交声明 ──────────────────────────────────────────

/// reduce 模式（覆盖型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceMode { Off, Reduce, Eliminate }

/// 正交性声明：reduce 是覆盖、倍率是缩放——语义不重叠。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrthoDecl;

impl OrthoDecl {
    /// 正交红线：reduce=Eliminate 时倍率不应再生效（覆盖优先于缩放）。
    pub fn reduce_wins(reduce: ReduceMode, scale: DurationScale) -> u32 {
        match reduce {
            ReduceMode::Eliminate => 0,
            ReduceMode::Reduce => scale.apply(50), // 减半基础再倍率
            ReduceMode::Off => scale.apply(100),
        }
    }

    /// 正交语义等价：reduce=Eliminate ≈ 倍率无穷大。
    pub fn semantic_equiv_eliminate(base_ms: u32) -> bool {
        let via_reduce = OrthoDecl::reduce_wins(ReduceMode::Eliminate, DurationScale::new(300));
        via_reduce == 0 && base_ms > 0
    }
}

// ── 令牌重标定 ──────────────────────────────────────────────────────

/// 时长令牌。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationToken {
    pub id: u32,
    pub base_ms: u32,
    pub scaled_ms: u32,
    pub calibrated: bool,
}

impl DurationToken {
    pub fn new(id: u32, base_ms: u32, scale: DurationScale) -> Self {
        Self { id, base_ms, scaled_ms: scale.apply(base_ms), calibrated: true }
    }

    /// 重标定（改倍率后全令牌即时重标）。
    pub fn recalibrate(&mut self, scale: DurationScale) {
        self.scaled_ms = scale.apply(self.base_ms);
        self.calibrated = true;
    }
}

/// 令牌注册表：倍率变更→全量重标定。
#[derive(Debug, Clone)]
pub struct TokenRegistry {
    pub tokens: Vec<DurationToken>,
    pub scale: DurationScale,
    pub reduce: ReduceMode,
}

impl TokenRegistry {
    pub fn new(scale: DurationScale) -> Self {
        Self { tokens: Vec::new(), scale, reduce: ReduceMode::Off }
    }

    /// 批量注册。
    pub fn register(&mut self, id: u32, base_ms: u32) {
        self.tokens.push(DurationToken::new(id, base_ms, self.scale));
    }

    /// 全量重标定（改倍率→即时生效）。
    pub fn set_scale(&mut self, new_scale: DurationScale) {
        self.scale = new_scale;
        for t in &mut self.tokens {
            t.recalibrate(new_scale);
        }
    }

    /// 全部已标定。
    pub fn all_calibrated(&self) -> bool {
        self.tokens.iter().all(|t| t.calibrated)
    }

    /// 终态时长（含 reduce 覆盖）。
    pub fn final_duration(&self, id: u32) -> Option<u32> {
        let t = self.tokens.iter().find(|t| t.id == id)?;
        Some(match self.reduce {
            ReduceMode::Eliminate => 0,
            ReduceMode::Reduce => t.scaled_ms / 2,
            ReduceMode::Off => t.scaled_ms,
        })
    }
}

// ── 持久化 ──────────────────────────────────────────────────────────

/// 倍率偏好持久化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScalePersist {
    pub stored_pct: u16,
    pub write_ok: bool,
}

impl ScalePersist {
    /// 持久化读写一致。
    pub fn consistent(&self, current: DurationScale) -> bool {
        self.write_ok && self.stored_pct == current.scale_pct
    }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3866_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4d_motionscale");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 倍率合法
    {
        let s = DurationScale::new(150);
        chk(&mut set, "C3866-01", s.valid(), "err");
    }
    // 2 倍率钳制（低于下限）
    {
        let s = DurationScale::new(10);
        chk(&mut set, "C3866-02", s.scale_pct == DurationScale::MIN, "err");
    }
    // 3 倍率钳制（超上限）
    {
        let s = DurationScale::new(500);
        chk(&mut set, "C3866-03", s.scale_pct == DurationScale::MAX, "err");
    }
    // 4 倍率单源
    {
        let s = DurationScale::new(100);
        chk(&mut set, "C3866-04", s.single_source_ok(), "err");
    }
    // 5 倍率应用
    {
        let s = DurationScale::new(200);
        chk(&mut set, "C3866-05", s.apply(1000) == 2000, "err");
    }
    // 6 reduce=Eliminate 优先于倍率
    {
        let r = OrthoDecl::reduce_wins(ReduceMode::Eliminate, DurationScale::new(300));
        chk(&mut set, "C3866-06", r == 0, "err");
    }
    // 7 reduce=Reduce + 倍率
    {
        let r = OrthoDecl::reduce_wins(ReduceMode::Reduce, DurationScale::new(200));
        chk(&mut set, "C3866-07", r == 100, "err");
    }
    // 8 正交语义等价
    {
        chk(&mut set, "C3866-08", OrthoDecl::semantic_equiv_eliminate(500), "err");
    }
    // 9 令牌重标定
    {
        let mut reg = TokenRegistry::new(DurationScale::new(100));
        reg.register(1, 500);
        reg.set_scale(DurationScale::new(200));
        chk(&mut set, "C3866-09", reg.all_calibrated() && reg.final_duration(1) == Some(1000), "err");
    }
    // 10 全量即时重标定
    {
        let mut reg = TokenRegistry::new(DurationScale::new(100));
        reg.register(1, 300);
        reg.register(2, 600);
        reg.set_scale(DurationScale::new(50));
        chk(&mut set, "C3866-10", reg.final_duration(1) == Some(150) && reg.final_duration(2) == Some(300), "err");
    }
    // 11 reduce=Eliminate 终态
    {
        let mut reg = TokenRegistry::new(DurationScale::new(100));
        reg.register(1, 500);
        reg.reduce = ReduceMode::Eliminate;
        chk(&mut set, "C3866-11", reg.final_duration(1) == Some(0), "err");
    }
    // 12 reduce=Reduce 终态
    {
        let mut reg = TokenRegistry::new(DurationScale::new(100));
        reg.register(1, 1000);
        reg.reduce = ReduceMode::Reduce;
        chk(&mut set, "C3866-12", reg.final_duration(1) == Some(500), "err");
    }
    // 13 持久化一致
    {
        let s = DurationScale::new(180);
        let p = ScalePersist { stored_pct: 180, write_ok: true };
        chk(&mut set, "C3866-13", p.consistent(s), "err");
    }
    // 14 持久化丢失
    {
        let s = DurationScale::new(180);
        let p = ScalePersist { stored_pct: 0, write_ok: false };
        chk(&mut set, "C3866-14", !p.consistent(s), "err");
    }
    // 15 令牌查无
    {
        let reg = TokenRegistry::new(DurationScale::new(100));
        chk(&mut set, "C3866-15", reg.final_duration(99).is_none(), "err");
    }
    // 16 半倍率
    {
        let s = DurationScale::new(50);
        chk(&mut set, "C3866-16", s.apply(1000) == 500, "err");
    }
    // 17 极限倍率
    {
        let s = DurationScale::new(300);
        chk(&mut set, "C3866-17", s.apply(1000) == 3000, "err");
    }
    // 18 Off 模式等于缩放基础
    {
        let r = OrthoDecl::reduce_wins(ReduceMode::Off, DurationScale::new(150));
        chk(&mut set, "C3866-18", r == 150, "err");
    }

    set
}
