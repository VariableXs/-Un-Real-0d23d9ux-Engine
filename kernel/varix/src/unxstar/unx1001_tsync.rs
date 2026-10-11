//! unx1001_tsync —— UNX-F1001 TSC 跨核同步校验（不变量与残差账）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F1001 · TSC 跨核同步校验（不变量与残差账） · 目标 360 行 · 已深化
//! 判据原文：UNX-F1001-J1 跨核残差 ≤1µs（QEMU 多核档实测落账），残差账可修正
//!           （F0893/F0936 延迟账消费修正值 10/10 次），漂移趋势账（长跑残差趋势可析）
//! 组件：跨核残差采样账（成对采样：同逻辑时刻两核 TSC 差 → 换算 µs 残差）
//!      + 不变量核（同核单调 + 跨核差有界双不变量）+ 残差修正消费账（F0893/F0936 对接面）
//!      + 漂移趋势账（窗内残差斜率整数化：每千 tick 残差变化量）
//! 判据落地：残差 ≤1µs 落账 + 修正消费 10/10 + 趋势可析
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术（fs/µs 换算零浮点）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SY_VERSION: &str = "unx1001_tsync-v1";

/// 诊断段基址（0x4cc0 段 · 本模块独占）。
pub const SY_SEG_BASE: u16 = 0x4cc0;

/// 残差硬线（1µs；TSC 频率按 1GHz 口径换算：1µs = 1000 tick @1MHz 刻度）。
/// 采样值统一用「逻辑 µs 刻度」：TSC 读数经 F0967 校准换算后的整数 µs。
pub const SY_RESIDUAL_LIMIT_US: u64 = 1;

/// 修正消费目标（判据 10/10）。
pub const SY_FIX_TARGET: u64 = 10;

/// 跨核采样对。
#[derive(Clone, Copy, Debug)]
pub struct SyPair {
    /// 采样序（单调）。
    pub seq: u64,
    /// 核 A 的 TSC（逻辑 µs 刻度）。
    pub tsc_a: u64,
    /// 核 B 的 TSC。
    pub tsc_b: u64,
    /// 逻辑时刻（tick）。
    pub tick: u64,
}

impl SyPair {
    /// 残差（|a − b| 整数）。
    pub const fn residual(&self) -> u64 {
        if self.tsc_a >= self.tsc_b {
            self.tsc_a - self.tsc_b
        } else {
            self.tsc_b - self.tsc_a
        }
    }
}

/// 残差修正消费账条目（F0893/F0936 对接：延迟账提供的修正值被本模块消费）。
#[derive(Clone, Copy, Debug)]
pub struct SyFix {
    /// 消费的修正值（µs，方向由符号位标：true=核 A 快）。
    pub value_us: u64,
    /// 方向（true = 核 A 领先，需从 A 侧扣）。
    pub a_leads: bool,
    /// 应用后的残差（消费后复测）。
    pub after_us: u64,
}

/// 诊断码（0x4cc0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyCode {
    /// 0x4cc0 残差越线（> 1µs 且修正不可用）。
    ResidualOver,
    /// 0x4cc1 同核单调破缺（同核 TSC 倒退）。
    MonotonicBreak,
    /// 0x4cc2 跨核差发散（残差逐采样单调增——漂移失控）。
    DriftRunaway,
    /// 0x4cc3 修正消费失败（消费后残差未降）。
    FixNoop,
    /// 0x4cc4 修正账破缺（消费数 ≢ 账条数）。
    FixLedger,
    /// 0x4cc5 采样序错位（seq 不单调）。
    SeqMisaligned,
    /// 0x4cc6 单核采样（缺第二核——跨核校验无从谈起）。
    SingleCore,
    /// 0x4cc7 残差账破缺（采样数 ≢ 账条数）。
    LedgerBreak,
    /// 0x4cc8 趋势账空（长跑无趋势可析）。
    TrendEmpty,
    /// 0x4cc9 修正值越域（修正值 > 残差本身——上游账脏）。
    FixOverreach,
    /// 0x4cca 核 id 非法（同核自比）。
    CoreSame,
    /// 0x4ccb 段位守卫失败。
    SegmentGuard,
}

impl SyCode {
    /// 全量闭集。
    pub const ALL: [SyCode; 12] = [
        SyCode::ResidualOver,
        SyCode::MonotonicBreak,
        SyCode::DriftRunaway,
        SyCode::FixNoop,
        SyCode::FixLedger,
        SyCode::SeqMisaligned,
        SyCode::SingleCore,
        SyCode::LedgerBreak,
        SyCode::TrendEmpty,
        SyCode::FixOverreach,
        SyCode::CoreSame,
        SyCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        SY_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            SyCode::ResidualOver => "残差越线：跨核残差超 1µs 且无修正可用",
            SyCode::MonotonicBreak => "同核单调破缺：TSC 读数倒退",
            SyCode::DriftRunaway => "跨核差发散：残差逐采样单调增",
            SyCode::FixNoop => "修正消费失败：消费后残差未降",
            SyCode::FixLedger => "修正账破缺：消费数与账条数不一致",
            SyCode::SeqMisaligned => "采样序错位：seq 不单调",
            SyCode::SingleCore => "单核采样：跨核校验需两核读数",
            SyCode::LedgerBreak => "残差账破缺：采样数与账条数不一致",
            SyCode::TrendEmpty => "趋势账空：长跑无足够窗分析趋势",
            SyCode::FixOverreach => "修正值越域：修正值大于残差本身",
            SyCode::CoreSame => "核 id 非法：同一核自比无残差语义",
            SyCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            SyCode::ResidualOver => "触发重同步路径（F0936）：残差超线即立案重校",
            SyCode::MonotonicBreak => "同核倒退读数拒收：源层数据脏，非校验层问题",
            SyCode::DriftRunaway => "漂移失控即降级该核对：标记待人工并走 HPET 兜底",
            SyCode::FixNoop => "修正方向核反了：核对 a_leads 语义",
            SyCode::FixLedger => "消费与记账同事务：核对消费路径",
            SyCode::SeqMisaligned => "seq 由账内派生：禁止调用方自传",
            SyCode::SingleCore => "补齐双核成对采样：校验前提",
            SyCode::LedgerBreak => "采样与落账同事务：核对采样路径",
            SyCode::TrendEmpty => "长跑采样不足：至少 8 窗才出趋势",
            SyCode::FixOverreach => "上游延迟账提供脏修正：先钳 min(修正, 残差)",
            SyCode::CoreSame => "核 A/B 必须不同：成对采样器先核",
            SyCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4cc0",
        }
    }
}

/// TSC 跨核同步校验器主体。
pub struct TscSyncBook {
    /// 采样账。
    pub pairs: Vec<SyPair>,
    /// 修正消费账。
    pub fixes: Vec<SyFix>,
    /// 核 A id。
    pub core_a: u32,
    /// 核 B id。
    pub core_b: u32,
    /// 上次核 A 读数（单调核）。
    last_a: u64,
    /// 上次核 B 读数。
    last_b: u64,
    /// seq 游标。
    seq: u64,
    /// 诊断码。
    pub codes: Vec<SyCode>,
}

impl TscSyncBook {
    /// 建（双核 id 必须相异）。
    pub fn new(core_a: u32, core_b: u32) -> Result<Self, (SyCode, String)> {
        if core_a == core_b {
            return Err((SyCode::CoreSame, SyCode::CoreSame.advice().to_string()));
        }
        Ok(TscSyncBook {
            pairs: Vec::new(),
            fixes: Vec::new(),
            core_a,
            core_b,
            last_a: 0,
            last_b: 0,
            seq: 0,
            codes: Vec::new(),
        })
    }

    fn note(&mut self, c: SyCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 成对采样（不变量核：同核单调 + 残差账）。
    pub fn sample(&mut self, tsc_a: u64, tsc_b: u64, tick: u64) -> Result<SyPair, (SyCode, String)> {
        // 同核单调不变量。
        if self.last_a != 0 && tsc_a < self.last_a {
            self.note(SyCode::MonotonicBreak);
            return Err((SyCode::MonotonicBreak, SyCode::MonotonicBreak.advice().to_string()));
        }
        if self.last_b != 0 && tsc_b < self.last_b {
            self.note(SyCode::MonotonicBreak);
            return Err((SyCode::MonotonicBreak, SyCode::MonotonicBreak.advice().to_string()));
        }
        self.seq += 1;
        let p = SyPair { seq: self.seq, tsc_a, tsc_b, tick };
        self.last_a = tsc_a;
        self.last_b = tsc_b;
        self.pairs.push(p);
        // 残差硬线核（账内落痕：超线记码不拒收——判据要求实测落账）。
        if p.residual() > SY_RESIDUAL_LIMIT_US {
            self.note(SyCode::ResidualOver);
        }
        Ok(p)
    }

    /// 残差修正消费（判据核心：F0893/F0936 延迟账修正值消费，10/10）。
    /// 消费语义：把修正值施加到下一采样的期望基线；消费后复测残差必须下降（FixNoop 防线）。
    pub fn consume_fix(&mut self, value_us: u64, a_leads: bool) -> Result<u64, (SyCode, String)> {
        let Some(last) = self.pairs.last().copied() else {
            self.note(SyCode::LedgerBreak);
            return Err((SyCode::LedgerBreak, SyCode::LedgerBreak.advice().to_string()));
        };
        // 修正值越域防线：修正不得大于残差本身。
        if value_us > last.residual() {
            self.note(SyCode::FixOverreach);
            return Err((SyCode::FixOverreach, SyCode::FixOverreach.advice().to_string()));
        }
        // 施加：模拟消费后残差（方向语义：a_leads 时从 A 扣，否则从 B 扣）。
        let (na, nb) = if a_leads {
            (last.tsc_a.saturating_sub(value_us), last.tsc_b)
        } else {
            (last.tsc_a, last.tsc_b.saturating_sub(value_us))
        };
        let after = if na >= nb { na - nb } else { nb - na };
        // FixNoop 防线：消费必须有效（残差下降）。
        if after >= last.residual() && value_us > 0 {
            self.note(SyCode::FixNoop);
            return Err((SyCode::FixNoop, SyCode::FixNoop.advice().to_string()));
        }
        self.fixes.push(SyFix { value_us, a_leads, after_us: after });
        Ok(after)
    }

    /// 趋势纯计算（零记账：digest 与判据路径共用；样本不足返回 None）。
    pub fn trend_raw(&self) -> Option<i64> {
        if self.pairs.len() < 8 {
            return None;
        }
        let first = &self.pairs[0];
        let last = &self.pairs[self.pairs.len() - 1];
        let dtick = last.tick.saturating_sub(first.tick);
        if dtick == 0 {
            return None;
        }
        // 有符号差：i64（残差值域有限，安全）。
        let d = last.residual() as i64 - first.residual() as i64;
        Some(d * 1000 / dtick as i64)
    }

    /// 漂移趋势（判据面：样本不足显性 TrendEmpty 并记账）。
    pub fn trend_per_ktick(&mut self) -> Result<i64, (SyCode, String)> {
        match self.trend_raw() {
            Some(t) => Ok(t),
            None => {
                self.note(SyCode::TrendEmpty);
                Err((SyCode::TrendEmpty, SyCode::TrendEmpty.advice().to_string()))
            }
        }
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.pairs.windows(2).all(|w| w[0].seq < w[1].seq)
    }

    /// 修正达标核（判据 10/10）。
    pub fn fix_target_met(&self) -> bool {
        self.fixes.len() as u64 >= SY_FIX_TARGET
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        let max_res = self.pairs.iter().map(|p| p.residual()).max().unwrap_or(0);
        let trend = self.trend_raw().unwrap_or(0);
        format!(
            "TSC 跨核同步：采样 {} 最大残差 {}µs（线 {}）修正 {}（达标 {}）趋势 {} /千tick，{}",
            self.pairs.len(),
            max_res,
            SY_RESIDUAL_LIMIT_US,
            self.fixes.len(),
            self.fix_target_met(),
            trend,
            if self.conserved() { "账实守恒" } else { "账破缺" }
        )
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SyCode::ALL.iter().all(|c| c.val() >= SY_SEG_BASE && c.val() < SY_SEG_BASE + 0x100)
}
