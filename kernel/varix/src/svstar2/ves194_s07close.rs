#![allow(dead_code)]
use alloc::vec;
// VE-F3940 · S07 组收口双签
//
// 接口总账：S07 冻结 API 九族签名表（成本模型/预算总表/基准/门禁/降档矩阵/
// 水位/功耗/归因/调试器）+ 映射断言 + 20 项自检。
// 联动核验：五前例模板对账（六版一致性——家族对拍）/六色瀑布证据/
// 四重最坏证据/低档一个不少证据/断链零证据。
// 错误路径：映射缺→阻断、模板漂移→对账、断链未修→阻断（P0 硬门）、联动分歧→两轮上限。


extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xD400）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDHCode(pub u16);

impl CDHCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const MAP_MISS: Self = Self(SEG_BASE + 1);
    pub const TEMPLATE_DRIFT: Self = Self(SEG_BASE + 2);
    pub const BREAK_OPEN: Self = Self(SEG_BASE + 3);
    pub const LINK_DIVERGE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::MAP_MISS => "映射缺→阻断",
            Self::TEMPLATE_DRIFT => "模板漂移→对账",
            Self::BREAK_OPEN => "断链未修→阻断（P0 硬门）",
            Self::LINK_DIVERGE => "联动分歧→两轮上限",
            _ => "未知",
        }
    }
}

// ── 九族签名表 ──────────────────────────────────────────────────────

/// 九族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    CostModel,
    BudgetTable,
    Baseline,
    Gate,
    TierMatrix,
    Watermark,
    Power,
    Attribution,
    Debugger,
}

impl Family {
    pub const ALL: [Family; 9] = [
        Family::CostModel,
        Family::BudgetTable,
        Family::Baseline,
        Family::Gate,
        Family::TierMatrix,
        Family::Watermark,
        Family::Power,
        Family::Attribution,
        Family::Debugger,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::CostModel => "成本模型",
            Self::BudgetTable => "预算总表",
            Self::Baseline => "基准",
            Self::Gate => "门禁",
            Self::TierMatrix => "降档矩阵",
            Self::Watermark => "水位",
            Self::Power => "功耗",
            Self::Attribution => "归因",
            Self::Debugger => "调试器",
        }
    }
}

/// 族签名：冻结 API 条目。
#[derive(Debug, Clone, Copy)]
pub struct FamilySign {
    pub fam: Family,
    /// 冻结函数数。
    pub fns: u32,
    /// 已冻结。
    pub frozen: bool,
}

/// 接口总账。
#[derive(Debug, Clone)]
pub struct ApiBook {
    pub signs: Vec<FamilySign>,
}

impl ApiBook {
    /// 九族全登记。
    pub fn full() -> Self {
        Self {
            signs: Family::ALL
                .iter()
                .map(|f| FamilySign {
                    fam: *f,
                    fns: 8,
                    frozen: true,
                })
                .collect(),
        }
    }

    pub fn count(&self) -> usize {
        self.signs.len()
    }

    /// 未冻结族。
    pub fn unfrozen(&self) -> Vec<Family> {
        self.signs
            .iter()
            .filter(|s| !s.frozen)
            .map(|s| s.fam)
            .collect()
    }

    /// 冻结函数总数。
    pub fn total_fns(&self) -> u32 {
        self.signs.iter().map(|s| s.fns).sum()
    }
}

// ── 映射断言 ────────────────────────────────────────────────────────

/// 映射断言：族 → 责任模块锚点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapEntry {
    pub fam: Family,
    /// 锚点模块号（0 = 缺映射）。
    pub anchor: u32,
}

/// 映射表核验：全部族有非零锚点。
pub fn mapping_ok(entries: &[MapEntry]) -> bool {
    Family::ALL
        .iter()
        .all(|f| entries.iter().any(|e| e.fam == *f && e.anchor != 0))
}

/// 缺映射族。
pub fn missing_maps(entries: &[MapEntry]) -> Vec<Family> {
    Family::ALL
        .iter()
        .filter(|&f| !entries.iter().any(|e| e.fam == *f && e.anchor != 0))
        .copied()
        .collect()
}

// ── 联动核验（四件证据）─────────────────────────────────────────────

/// 五前例模板版本（六版一致性：本版 + 五前例）。
pub const TEMPLATE_VERSIONS: [u32; 6] = [0x5_01_00, 0x5_02_00, 0x5_03_00, 0x5_04_00, 0x5_05_00, 0x5_07_00];

/// 模板对账：六版字段数一致。
pub fn template_consistent(field_counts: &[usize; 6]) -> bool {
    field_counts.iter().all(|c| *c == field_counts[0])
}

/// 联动证据四件。
#[derive(Debug, Clone, Copy)]
pub struct LinkEvidence {
    /// 六色瀑布证据在册。
    pub waterfall6: bool,
    /// 四重最坏证据在册。
    pub worst_quad: bool,
    /// 低档一个不少证据在册。
    pub lowtier_complete: bool,
    /// 断链零证据在册。
    pub break_zero: bool,
}

impl LinkEvidence {
    /// 四件齐。
    pub fn complete(&self) -> bool {
        self.waterfall6 && self.worst_quad && self.lowtier_complete && self.break_zero
    }

    /// 缺件数。
    pub fn gaps(&self) -> u32 {
        [self.waterfall6, self.worst_quad, self.lowtier_complete, self.break_zero]
            .iter()
            .filter(|b| !**b)
            .count() as u32
    }
}

// ── 双签 ────────────────────────────────────────────────────────────

/// 双签结构：组负责人 + 独立复核。
#[derive(Debug, Clone, Copy)]
pub struct DualSign {
    pub owner: bool,
    pub reviewer: bool,
    /// 联动对账轮次（两轮上限）。
    pub rounds: u32,
}

/// 两轮上限。
pub const ROUND_LIMIT: u32 = 2;

impl DualSign {
    /// 双签成立：两签齐且轮次不超限。
    pub fn done(&self) -> bool {
        self.owner && self.reviewer && self.rounds <= ROUND_LIMIT
    }

    /// 轮次超限判定。
    pub fn exhausted(&self) -> bool {
        self.rounds > ROUND_LIMIT
    }
}

// ── 20 项自检 ───────────────────────────────────────────────────────

/// 自检清单：20 项布尔账。
#[derive(Debug, Clone)]
pub struct SelfCheck {
    pub items: [bool; 20],
}

impl SelfCheck {
    pub fn new_passing() -> Self {
        Self { items: [true; 20] }
    }

    pub fn all_pass(&self) -> bool {
        self.items.iter().all(|b| *b)
    }

    pub fn fail_count(&self) -> usize {
        self.items.iter().filter(|b| !**b).count()
    }

    /// 逐项置位。
    pub fn set(&mut self, idx: usize, v: bool) -> bool {
        if idx < 20 {
            self.items[idx] = v;
            true
        } else {
            false
        }
    }
}

// ── 收口会话（总控）─────────────────────────────────────────────────

/// S07 组收口会话。
#[derive(Debug, Clone)]
pub struct S07Close {
    pub api: ApiBook,
    pub maps: Vec<MapEntry>,
    pub evidence: LinkEvidence,
    pub sign: DualSign,
    pub checks: SelfCheck,
    /// 断链未修数（P0 硬门）。
    pub breaks_open: u32,
    /// 对账轮次。
    pub reconcile_rounds: u32,
}

impl S07Close {
    pub fn new() -> Self {
        Self {
            api: ApiBook::full(),
            maps: Family::ALL
                .iter()
                .enumerate()
                .map(|(i, f)| MapEntry {
                    fam: *f,
                    anchor: 0x7_00_00 + i as u32,
                })
                .collect(),
            evidence: LinkEvidence {
                waterfall6: true,
                worst_quad: true,
                lowtier_complete: true,
                break_zero: true,
            },
            sign: DualSign {
                owner: true,
                reviewer: true,
                rounds: 0,
            },
            checks: SelfCheck::new_passing(),
            breaks_open: 0,
            reconcile_rounds: 0,
        }
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CDHCode> {
        if self.breaks_open > 0 {
            return Err(CDHCode::BREAK_OPEN);
        }
        if !mapping_ok(&self.maps) {
            return Err(CDHCode::MAP_MISS);
        }
        if self.api.unfrozen().len() < self.api.count() && !self.api.unfrozen().is_empty() {
            return Err(CDHCode::MAP_MISS);
        }
        if !self.evidence.complete() {
            return Err(CDHCode::LINK_DIVERGE);
        }
        if self.sign.exhausted() {
            return Err(CDHCode::LINK_DIVERGE);
        }
        if !self.checks.all_pass() {
            return Err(CDHCode::MAP_MISS);
        }
        Ok(())
    }

    /// 双签完成判定。
    pub fn signed(&self) -> bool {
        self.sign.done() && self.checks.all_pass() && self.breaks_open == 0
    }
}

impl Default for S07Close {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves194_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves194_s07close");

    // 01 九族齐备定名
    {
        let ok = Family::ALL.len() == 9
            && Family::ALL.iter().all(|f| !f.name().is_empty());
        chk(&mut set, "C3940-01", ok, "err");
    }
    // 02 九族签名表全冻结
    {
        let b = ApiBook::full();
        chk(
            &mut set,
            "C3940-02",
            b.count() == 9 && b.unfrozen().is_empty(),
            "err",
        );
    }
    // 03 冻结函数总数
    {
        let b = ApiBook::full();
        chk(&mut set, "C3940-03", b.total_fns() == 72, "err");
    }
    // 04 未冻结族检出
    {
        let mut b = ApiBook::full();
        b.signs[4].frozen = false;
        chk(&mut set, "C3940-04", b.unfrozen() == vec![Family::TierMatrix], "err");
    }
    // 05 映射断言：全锚点通过
    {
        let s = S07Close::new();
        chk(&mut set, "C3940-05", mapping_ok(&s.maps), "err");
    }
    // 06 映射缺→阻断
    {
        let mut s = S07Close::new();
        s.maps[2].anchor = 0;
        chk(
            &mut set,
            "C3940-06",
            missing_maps(&s.maps) == vec![Family::Baseline]
                && s.full_check() == Err(CDHCode::MAP_MISS),
            "err",
        );
    }
    // 07 模板六版一致
    {
        let counts = [12usize, 12, 12, 12, 12, 12];
        chk(&mut set, "C3940-07", template_consistent(&counts), "err");
    }
    // 08 模板漂移检出
    {
        let counts = [12usize, 12, 11, 12, 12, 12];
        chk(&mut set, "C3940-08", !template_consistent(&counts), "err");
    }
    // 09 模板版本六版齐
    {
        chk(&mut set, "C3940-09", TEMPLATE_VERSIONS.len() == 6, "err");
    }
    // 10 联动证据四件齐
    {
        let e = LinkEvidence {
            waterfall6: true,
            worst_quad: true,
            lowtier_complete: true,
            break_zero: true,
        };
        chk(&mut set, "C3940-10", e.complete() && e.gaps() == 0, "err");
    }
    // 11 联动缺件计位
    {
        let e = LinkEvidence {
            waterfall6: true,
            worst_quad: false,
            lowtier_complete: true,
            break_zero: false,
        };
        chk(&mut set, "C3940-11", !e.complete() && e.gaps() == 2, "err");
    }
    // 12 断链未修→P0 阻断
    {
        let mut s = S07Close::new();
        s.breaks_open = 1;
        chk(
            &mut set,
            "C3940-12",
            s.full_check() == Err(CDHCode::BREAK_OPEN),
            "err",
        );
    }
    // 13 两轮上限：内通过
    {
        let sgn = DualSign {
            owner: true,
            reviewer: true,
            rounds: 2,
        };
        chk(&mut set, "C3940-13", sgn.done() && !sgn.exhausted(), "err");
    }
    // 14 两轮上限：超限失效
    {
        let sgn = DualSign {
            owner: true,
            reviewer: true,
            rounds: 3,
        };
        chk(&mut set, "C3940-14", !sgn.done() && sgn.exhausted(), "err");
    }
    // 15 单签不成双签
    {
        let sgn = DualSign {
            owner: true,
            reviewer: false,
            rounds: 1,
        };
        chk(&mut set, "C3940-15", !sgn.done(), "err");
    }
    // 16 自检 20 项全过
    {
        let c = SelfCheck::new_passing();
        chk(&mut set, "C3940-16", c.all_pass() && c.fail_count() == 0, "err");
    }
    // 17 自检置位与越界防御
    {
        let mut c = SelfCheck::new_passing();
        let ok = c.set(7, false);
        let oob = c.set(20, true);
        chk(
            &mut set,
            "C3940-17",
            ok && !oob && c.fail_count() == 1 && !c.all_pass(),
            "err",
        );
    }
    // 18 会话总检全绿
    {
        let s = S07Close::new();
        chk(&mut set, "C3940-18", s.full_check() == Ok(()), "err");
    }
    // 19 双签完成判定
    {
        let s = S07Close::new();
        chk(&mut set, "C3940-19", s.signed(), "err");
    }
    // 20 双签未完（自检挂红）
    {
        let mut s = S07Close::new();
        let _ = s.checks.set(13, false);
        chk(&mut set, "C3940-20", !s.signed(), "err");
    }

    set
}
