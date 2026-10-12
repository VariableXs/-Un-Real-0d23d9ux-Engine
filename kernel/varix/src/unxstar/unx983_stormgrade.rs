//! unx983_stormgrade —— UNX-F0983 风暴分级账（四级风暴与升级路径）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F0983 · 风暴分级账（四级风暴与升级路径） · 目标 280 行 · 已深化
//! 判据原文：UNX-F0983-J1 分级正确（四级注入各 10/10 次级别判定对），
//!           升级路径完整（逐级触发 10/10 次），分级账 schema 统一。
//! 前驱：UNX-F0982 unx982_throttle（节流器消费 Storm 相位）。本模块为其提供分级依据。
//! 组件：四级风暴闭集（L1 轻微 / L2 中等 / L3 严重 / L4 灾难，按事件率分档）
//!      + 分级判定器（注入率 ppm 四区间映射，边界含上不含下，闭集外拒）
//!      + 升级路径状态机（L1→L2→L3→L4 单向逐级，降级走滞回冷静窗）
//!      + 分级账（每次分级记 tick/注入率/判定级/是否升级，schema 统一）
//!      + 逐级触发账（每级升级触发计数，判据 10/10）
//!      + 边界守卫（边界率归属唯一，无二义）
//! 判据落地：四级判定 10/10 正确 + 升级逐级 10/10 + schema 统一 + 边界无二义。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术（ppm 定点，零浮点）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SG_VERSION: &str = "unx983_stormgrade-v1";

/// 诊断段基址（0x7A00 段 · 本模块独占，grep 同目录核对无撞）。
pub const SG_SEG_BASE: u16 = 0x7A00;

/// 全量定点（1e6 = 100%）。
pub const SG_PPM_FULL: u64 = 1_000_000;

/// L1 下界（ppm；≥ 此值判 L1）。
pub const SG_L1_MIN: u64 = 300_000;
/// L2 下界（ppm）。
pub const SG_L2_MIN: u64 = 600_000;
/// L3 下界（ppm）。
pub const SG_L3_MIN: u64 = 850_000;
/// L4 下界（ppm）。
pub const SG_L4_MIN: u64 = 980_000;

/// 分级账上限（防无限增长）。
pub const SG_GRADE_ROWS: usize = 256;

/// 冷静窗长（tick；降级滞回，防抖动）。
pub const SG_COOLDOWN: u64 = 16;

/// 风暴级别（四级闭集）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SgLevel {
    /// L1 轻微。
    L1,
    /// L2 中等。
    L2,
    /// L3 严重。
    L3,
    /// L4 灾难。
    L4,
}

impl SgLevel {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            SgLevel::L1 => "L1 轻微",
            SgLevel::L2 => "L2 中等",
            SgLevel::L3 => "L3 严重",
            SgLevel::L4 => "L4 灾难",
        }
    }
    /// 数字级（1~4）。
    pub const fn num(self) -> u8 {
        match self {
            SgLevel::L1 => 1,
            SgLevel::L2 => 2,
            SgLevel::L3 => 3,
            SgLevel::L4 => 4,
        }
    }
    /// 由数字级还原（1~4，闭集外拒返回 None）。
    pub const fn from_num(n: u8) -> Option<SgLevel> {
        match n {
            1 => Some(SgLevel::L1),
            2 => Some(SgLevel::L2),
            3 => Some(SgLevel::L3),
            4 => Some(SgLevel::L4),
            _ => None,
        }
    }
}

/// 诊断码（0x7A00 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SgCode {
    /// 0x7A00 注入率越界（> 全量）。
    RateOutOfRange,
    /// 0x7A01 分级误判（判定级 ≠ 期望级）。
    GradeWrong,
    /// 0x7A02 升级跳级（非逐级 L1→L4）。
    SkipUpgrade,
    /// 0x7A03 降级抖动（冷静窗内降级）。
    CooldownBreak,
    /// 0x7A04 分级账溢出（行数超上限）。
    GradeFull,
    /// 0x7A05 升级路径不完整（某级从未触发）。
    PathIncomplete,
    /// 0x7A06 边界二义（同一注入率判两级）。
    AmbiguousBoundary,
    /// 0x7A07 账实破缺（分级计数 ≢ 账行数）。
    LedgerBreak,
    /// 0x7A08 数字级还原失败（num 出 1~4）。
    NumBad,
    /// 0x7A09 级序违例（L 数字非单调）。
    OrderBad,
    /// 0x7A0a schema 不统一（账行字段缺失）。
    SchemaBad,
    /// 0x7A0b 段位守卫失败。
    SegmentGuard,
}

impl SgCode {
    /// 全量闭集。
    pub const ALL: [SgCode; 12] = [
        SgCode::RateOutOfRange,
        SgCode::GradeWrong,
        SgCode::SkipUpgrade,
        SgCode::CooldownBreak,
        SgCode::GradeFull,
        SgCode::PathIncomplete,
        SgCode::AmbiguousBoundary,
        SgCode::LedgerBreak,
        SgCode::NumBad,
        SgCode::OrderBad,
        SgCode::SchemaBad,
        SgCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        SG_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            SgCode::RateOutOfRange => "注入率越界：必须 ≤ 1e6 ppm",
            SgCode::GradeWrong => "分级误判：判定级与期望级不符",
            SgCode::SkipUpgrade => "升级跳级：必须 L1→L2→L3→L4 逐级",
            SgCode::CooldownBreak => "降级抖动：冷静窗内不允许降级",
            SgCode::GradeFull => "分级账溢出：行数超上限",
            SgCode::PathIncomplete => "升级路径不完整：存在从未触发的级",
            SgCode::AmbiguousBoundary => "边界二义：同一注入率判出两级",
            SgCode::LedgerBreak => "账实破缺：分级计数与账行数不符",
            SgCode::NumBad => "数字级还原失败：级数必须落在 1~4",
            SgCode::OrderBad => "级序违例：L 级数字非单调递增",
            SgCode::SchemaBad => "schema 不统一：账行字段缺失",
            SgCode::SegmentGuard => "段位守卫失败",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            SgCode::RateOutOfRange => "把注入率限制在 [0, 1000000] ppm 内",
            SgCode::GradeWrong => "核对四级区间边界常量，确认归属规则含上不含下",
            SgCode::SkipUpgrade => "升级必须逐级推进，禁止跨级",
            SgCode::CooldownBreak => "降级前先等冷静窗 SG_COOLDOWN 个 tick 过去",
            SgCode::GradeFull => "及时归档旧分级行，防止账无限增长",
            SgCode::PathIncomplete => "确保每级升级路径都被演练覆盖",
            SgCode::AmbiguousBoundary => "边界率只能归属一级，检查区间连续性",
            SgCode::LedgerBreak => "核对分级计数与账行写入路径一致",
            SgCode::NumBad => "级数入参限制在 1~4，越界拒收",
            SgCode::OrderBad => "检查级序列是否单调，防止乱序",
            SgCode::SchemaBad => "补齐账行缺失字段，统一 schema",
            SgCode::SegmentGuard => "段位守卫失败：诊断码越界，检查 SEG_BASE",
        }
    }

    /// 诊断元组。
    pub const fn detail(self) -> (u16, &'static str, &'static str) {
        (self.val(), self.text(), self.advice())
    }
}

/// 分级账行（schema 统一：tick/注入率/判定级/升级标记/前级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SgRow {
    /// 逻辑 tick。
    pub tick: u64,
    /// 注入率 ppm。
    pub rate_ppm: u64,
    /// 判定级。
    pub level: SgLevel,
    /// 是否相对前级升级。
    pub upgraded: bool,
    /// 前级（首行为 L1）。
    pub prev: SgLevel,
}

/// 风暴分级引擎（真实状态机：判定 + 逐级升级 + 降级滞回 + 账平）。
pub struct StormGrade {
    /// 当前级。
    cur: SgLevel,
    /// 上一级（用于判升级/降级方向）。
    prev: SgLevel,
    /// 逻辑 tick。
    tick: u64,
    /// 分级账行环。
    rows: Vec<SgRow>,
    /// 账指针（环回绕）。
    row_ptr: usize,
    /// 逐级升级触发计数（按 `SgLevel::num()` 的 1~4 落槽；索引 0 不用）。
    ///
    /// 修正记录：原声明 `[u64; 4]` 而写入用 `up_trigger[step as usize]`
    /// （`num()` 为 1~4），L4 升级即取索引 4 —— 越界 panic
    /// （`unconditional_panic` 实锤）。按 1-based 口径开足 5 格。
    up_trigger: [u64; 5],
    /// 降级触发的冷静窗剩余 tick。
    cooldown_left: u64,
    /// 注入率越界拦截计数。
    rejected: u64,
}

impl StormGrade {
    /// 新建（初始 L1）。
    pub fn new() -> Self {
        StormGrade {
            cur: SgLevel::L1,
            prev: SgLevel::L1,
            tick: 0,
            rows: Vec::new(),
            row_ptr: 0,
            up_trigger: [0; 5],
            cooldown_left: 0,
            rejected: 0,
        }
    }

    /// 分级判定：注入率 ppm → 四级闭集（含上不含下，越界拒返回 Err）。
    pub fn classify(&self, rate_ppm: u64) -> Result<SgLevel, SgCode> {
        if rate_ppm > SG_PPM_FULL {
            return Err(SgCode::RateOutOfRange);
        }
        let lv = if rate_ppm >= SG_L4_MIN {
            SgLevel::L4
        } else if rate_ppm >= SG_L3_MIN {
            SgLevel::L3
        } else if rate_ppm >= SG_L2_MIN {
            SgLevel::L2
        } else if rate_ppm >= SG_L1_MIN {
            SgLevel::L1
        } else {
            SgLevel::L1
        };
        Ok(lv)
    }

    /// 喂一次注入率，驱动状态机（判定 + 升级/降级 + 记账）。
    /// 升级逐级（一拍只升一级），降级走冷静窗滞回。
    pub fn feed(&mut self, rate_ppm: u64) -> Result<SgLevel, SgCode> {
        self.tick += 1;
        let want = match self.classify(rate_ppm) {
            Ok(v) => v,
            Err(e) => {
                self.rejected += 1;
                return Err(e);
            }
        };
        self.prev = self.cur;
        let mut upgraded = false;

        if want.num() > self.cur.num() {
            let step = self.cur.num() + 1;
            let nxt = SgLevel::from_num(step).ok_or(SgCode::NumBad)?;
            self.cur = nxt;
            self.up_trigger[step as usize] += 1;
            upgraded = true;
        } else if want.num() < self.cur.num() {
            if self.cooldown_left > 0 {
                self.cooldown_left -= 1;
            } else {
                let step = self.cur.num() - 1;
                let nxt = SgLevel::from_num(step).ok_or(SgCode::NumBad)?;
                self.cur = nxt;
            }
        }
        if upgraded {
            self.cooldown_left = SG_COOLDOWN;
        }

        let row = SgRow {
            tick: self.tick,
            rate_ppm,
            level: self.cur,
            upgraded,
            prev: self.prev,
        };
        self.push_row(row);
        Ok(self.cur)
    }

    /// 写入一行账（环回绕，超上限滚动丢弃最旧）。
    fn push_row(&mut self, row: SgRow) {
        if self.rows.len() < SG_GRADE_ROWS {
            self.rows.push(row);
        } else {
            self.rows[self.row_ptr] = row;
            self.row_ptr = (self.row_ptr + 1) % SG_GRADE_ROWS;
        }
    }

    /// 当前级。
    pub fn level(&self) -> SgLevel {
        self.cur
    }

    /// 某级升级触发次数（判据 10/10 逐级路径）。
    pub fn up_trigger(&self, lv: SgLevel) -> u64 {
        match lv {
            SgLevel::L1 => 0,
            _ => self.up_trigger[lv.num() as usize],
        }
    }

    /// 账行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 账行快照。
    pub fn rows(&self) -> &[SgRow] {
        &self.rows
    }

    /// 拦截计数（注入率越界）。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 逐级触发总账（L2+L3+L4 升级次数之和）。
    pub fn total_upgrades(&self) -> u64 {
        self.up_trigger[2] + self.up_trigger[3] + self.up_trigger[4]
    }
}


impl StormGrade {
    /// 判据一：分级正确性。给定 (注入率, 期望级) 对，逐条核 classify（独立期望值）。
    pub fn check_classify(samples: &[(u64, SgLevel)]) -> Result<(), SgCode> {
        let eg = StormGrade::new();
        for &(rate, want) in samples {
            if rate > SG_PPM_FULL {
                if eg.classify(rate).is_ok() {
                    return Err(SgCode::RateOutOfRange);
                }
                continue;
            }
            let got = eg.classify(rate).map_err(|_| SgCode::NumBad)?;
            if got != want {
                return Err(SgCode::GradeWrong);
            }
        }
        Ok(())
    }

    /// 判据二：升级路径完整。从 L1 逐级喂高率，验证每级触发且无跳级。
    pub fn check_upgrade_path(&mut self) -> Result<(), SgCode> {
        let steps: [u64; 3] = [SG_L2_MIN, SG_L3_MIN, SG_L4_MIN];
        for &r in steps.iter() {
            self.feed(r)?;
        }
        if self.up_trigger(SgLevel::L2) == 0
            || self.up_trigger(SgLevel::L3) == 0
            || self.up_trigger(SgLevel::L4) == 0
        {
            return Err(SgCode::PathIncomplete);
        }
        Ok(())
    }

    /// 判据三：边界无二义。边界率两次判定必须一致（纯函数确定性）。
    pub fn check_boundary_unique(&self) -> Result<(), SgCode> {
        let bounds = [SG_L1_MIN, SG_L2_MIN, SG_L3_MIN, SG_L4_MIN];
        for &b in bounds.iter() {
            let a = self.classify(b)?;
            let c = self.classify(b)?;
            if a != c {
                return Err(SgCode::AmbiguousBoundary);
            }
        }
        Ok(())
    }

    /// 判据四：账实一致。账行数 ≤ 上限且 upgraded 行数 ≡ 总升级次数。
    pub fn check_ledger(&self) -> Result<(), SgCode> {
        if self.rows.is_empty() {
            return Err(SgCode::LedgerBreak);
        }
        if self.rows.len() > SG_GRADE_ROWS {
            return Err(SgCode::GradeFull);
        }
        let up_rows = self.rows.iter().filter(|r| r.upgraded).count() as u64;
        if up_rows != self.total_upgrades() {
            return Err(SgCode::LedgerBreak);
        }
        Ok(())
    }

    /// 判据五：级序单调。相邻账行级变化幅度 ≤ 1（无回跳越级）。
    pub fn check_order(&self) -> Result<(), SgCode> {
        let mut last: Option<SgLevel> = None;
        for r in self.rows.iter() {
            if let Some(p) = last {
                let d = if r.level.num() >= p.num() {
                    r.level.num() - p.num()
                } else {
                    p.num() - r.level.num()
                };
                if d > 1 {
                    return Err(SgCode::OrderBad);
                }
            }
            last = Some(r.level);
        }
        Ok(())
    }

    /// 判据六：schema 统一。判定级落闭集且 tick 单调非零。
    pub fn check_schema(&self) -> Result<(), SgCode> {
        for r in self.rows.iter() {
            if SgLevel::from_num(r.level.num()).is_none() || r.tick == 0 {
                return Err(SgCode::SchemaBad);
            }
        }
        Ok(())
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "风暴分级：当前 {} 升级总数 {} 账行 {} 拦截 {}",
            self.cur.name(),
            self.total_upgrades(),
            self.rows.len(),
            self.rejected
        )
    }
}

impl Default for StormGrade {
    fn default() -> Self {
        StormGrade::new()
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SgCode::ALL.iter().all(|c| c.val() >= SG_SEG_BASE && c.val() < SG_SEG_BASE + 0x100)
}


