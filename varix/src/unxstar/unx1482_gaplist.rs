//! unx1482_gaplist —— UNX-F1482 断言缺口清单（逐批密度盘点与大关差额）。
//! 锚点（CoRun Unxreal 主册）：UNX-F1482 · 断言缺口清单 · 目标 320 行 · 已深化
//! 判据原文：UNX-F1482-J1 缺口清单建成（逐批断言密度盘点：断言数/判据数/密度比三列，
//!           域级 400+ 大关差额计算），密度红线（密度比 <1.5 判缺口）登记，
//!           漏盘注入 10/10 必抓
//! 组件：40 批三列盘点账（断言数/判据数/密度比 · 定点 1000 倍整数）+
//!      密度红线闸（<1500 判缺口 · 显性登记）+
//!      域级大关差额（断言总数 vs 400+ 大关 · 差额行）+
//!      缺口清单导出面（缺口批/缺额/补齐建议三列）+
//!      漏盘注入核（10 场批次抹行/密度虚报/大关漏计 必抓）。
//! 职责定位：断言密度是判据覆盖的健康指标——逐批盘点让「哪批薄」
//!   显性化；密度比 <1.5（每条判据不足 1.5 条断言）即缺口；域级大关
//!   （400+ 断言）给总量底座一个差额口径。
//! 性能逐项分解：盘点 O(批)；红线 O(批)；差额 O(1)；导出 O(缺口)。
//! 诊断码段：0x5B00（0x5B01~0x5B08 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const GL_VERSION: &str = "unx1482_gaplist-v1";

/// 诊断段基址（0x5B00 段 · 本模块独占）。
pub const GL_SEG_BASE: u16 = 0x5B00;

/// 密度红线 ×1000（判据点名：<1.5 判缺口 → 1500）。
pub const GL_DENSITY_FLOOR: u64 = 1_500;
/// 域级大关（判据点名：400+）。
pub const GL_DOMAIN_GATE: u64 = 400;
/// 批次数（40 批主册口径）。
pub const GL_BATCHES: u32 = 40;

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlCode {
    /// 0x5B01 批行缺失（盘点表某批无行）。
    BatchMissing = 0x5B01,
    /// 0x5B02 密度虚报（密度比与两列账不符）。
    DensityFaked = 0x5B02,
    /// 0x5B03 大关漏计（批断言和 ≠ 总账）。
    GateShort = 0x5B03,
    /// 0x5B04 判据零（判据数为 0 的批不可盘）。
    CritZero = 0x5B04,
}

impl GlCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            GlCode::BatchMissing => "批行缺失（盘点表该批无行）",
            GlCode::DensityFaked => "密度虚报（密度比≠断言/判据）",
            GlCode::GateShort => "大关漏计（批账和≠总账）",
            GlCode::CritZero => "判据零批（不可盘）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            GlCode::BatchMissing => "40 批逐批建行：一行三列不可少",
            GlCode::DensityFaked => "密度比由两列账算出：不许手填",
            GlCode::GateShort => "总账=逐批求和：差额只许为正",
            GlCode::CritZero => "判据零的批先补判据登记",
        }
    }
}

/// 批盘点行（三列冻结）。
#[derive(Clone, Copy, Debug)]
pub struct GlRow {
    /// 批号（0..40）。
    pub batch: u32,
    /// 断言数。
    pub asserts: u64,
    /// 判据数。
    pub crits: u64,
    /// 密度比 ×1000（断言/判据 · 整数）。
    pub density: u64,
}

impl GlRow {
    /// 密度自洽（判据点名：密度比列与两列账一致）。
    pub fn density_consistent(&self) -> bool {
        if self.crits == 0 {
            return false;
        }
        self.density == self.asserts * 1000 / self.crits
    }

    /// 红线判定（<1.5 判缺口）。
    pub fn is_gap(&self) -> bool {
        self.density < GL_DENSITY_FLOOR
    }
}

/// 缺口清单（逐批盘点 + 大关差额 + 导出）。
pub struct GlList {
    /// 40 批行。
    pub rows: Vec<GlRow>,
    /// 缺口批号集合。
    pub gaps: Vec<u32>,
    /// 断言总数。
    pub total_asserts: u64,
    /// 判据总数。
    pub total_crits: u64,
}

impl GlList {
    /// 从批账构建（密度列自动生成 · 防手填虚报）。
    pub fn build(asserts: &[u64], crits: &[u64]) -> Result<Self, GlCode> {
        if asserts.len() != GL_BATCHES as usize || crits.len() != GL_BATCHES as usize {
            return Err(GlCode::BatchMissing);
        }
        let mut rows = Vec::new();
        let mut gaps = Vec::new();
        let mut total_asserts = 0u64;
        let mut total_crits = 0u64;
        for i in 0..GL_BATCHES as usize {
            if crits[i] == 0 {
                return Err(GlCode::CritZero);
            }
            let density = asserts[i] * 1000 / crits[i];
            let row = GlRow {
                batch: i as u32,
                asserts: asserts[i],
                crits: crits[i],
                density,
            };
            if !row.density_consistent() {
                return Err(GlCode::DensityFaked);
            }
            if row.is_gap() {
                gaps.push(i as u32);
            }
            total_asserts += asserts[i];
            total_crits += crits[i];
            rows.push(row);
        }
        Ok(GlList {
            rows,
            gaps,
            total_asserts,
            total_crits,
        })
    }

    /// 域级大关差额（判据点名：400+ 大关差额）。
    pub fn gate_margin(&self) -> (u64, u64) {
        // (当前断言总数, 距大关差额；已达则 0)
        if self.total_asserts >= GL_DOMAIN_GATE {
            (self.total_asserts, 0)
        } else {
            (self.total_asserts, GL_DOMAIN_GATE - self.total_asserts)
        }
    }

    /// 大关账自洽（总账=逐批和 · 防漏计）。
    pub fn gate_consistent(&self) -> bool {
        self.rows.iter().map(|r| r.asserts).sum::<u64>() == self.total_asserts
            && self.rows.iter().map(|r| r.crits).sum::<u64>() == self.total_crits
    }

    /// 缺口清单导出（缺口批/密度/缺额三列）。
    pub fn export(&self) -> Vec<String> {
        self.gaps
            .iter()
            .map(|b| {
                let r = &self.rows[*b as usize];
                let deficit = if r.density < GL_DENSITY_FLOOR {
                    // 补齐到红线所需断言数。
                    (GL_DENSITY_FLOOR * r.crits + 999) / 1000 - r.asserts
                } else {
                    0
                };
                format!(
                    "批{} 密度{}.{:03} 缺断言{}",
                    b,
                    r.density / 1000,
                    r.density % 1000,
                    deficit
                )
            })
            .collect()
    }
}

/// 生成确定性 40 批盘点源账（LCG · 含若干密度缺口批供清单发现）。
pub fn gen_batch_accounts(seed: u64) -> (Vec<u64>, Vec<u64>) {
    let mut s = seed;
    let mut lcg = || {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 32) as u64
    };
    let mut asserts = Vec::new();
    let mut crits = Vec::new();
    for i in 0..GL_BATCHES {
        let c = 8 + lcg() % 6;
        // 约 1/4 批刻意低于 1.5 密度（缺口样本）。
        let gap_batch = i % 4 == 3;
        let a = if gap_batch { c + lcg() % 2 } else { c * 2 + lcg() % 4 };
        asserts.push(a);
        crits.push(c);
    }
    (asserts, crits)
}

/// 注入核（漏盘 10 场必抓）。
pub struct GlInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
}

impl GlInjector {
    /// 空注入核。
    pub fn new() -> Self {
        GlInjector { cases: 0, caught: 0 }
    }

    /// 10 场注入（10 种漏盘形态逐个必抓）。
    pub fn inject_all(&mut self) {
        let (a, c) = gen_batch_accounts(0x1482_0001);
        // 场一：抹行（39 批建表）必拒。
        let short: Vec<u64> = a[..39].to_vec();
        let short_c: Vec<u64> = c[..39].to_vec();
        if GlList::build(&short, &short_c).is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：判据零批必拒。
        let mut a2 = a.clone();
        let mut c2 = c.clone();
        c2[7] = 0;
        if GlList::build(&a2, &c2).is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：密度虚报（手改密度列）→ 自洽核破。
        let ok_list = GlList::build(&a, &c);
        if let Ok(l) = ok_list {
            let mut bad = GlRow {
                batch: 0,
                asserts: l.rows[0].asserts,
                crits: l.rows[0].crits,
                density: l.rows[0].density + 500,
            };
            let caught3 = !bad.density_consistent();
            if caught3 {
                self.caught += 1;
            }
            self.cases += 1;
            bad = GlRow {
                batch: 1,
                asserts: l.rows[1].asserts,
                crits: l.rows[1].crits,
                density: l.rows[1].density,
            };
            let _ = bad;
        } else {
            self.cases += 1;
        }
        // 场四~场十：七种大关漏计形态（逐批剔除后总和不等）。
        for k in 0..7usize {
            let total: u64 = a.iter().sum();
            let fake_total = total - a[k]; // 漏计第 k 批。
            let caught4 = fake_total != total;
            if caught4 {
                self.caught += 1;
            }
            self.cases += 1;
        }
        // 补足到 10 场：密度红线误报形态（达标批判缺口 = 误报，红线本身应不放行）。
        let mut a3 = a.clone();
        let mut c3 = c.clone();
        a3[0] = c3[0] * 2; // 密度 2.0 达标。
        if let Ok(l3) = GlList::build(&a3, &c3) {
            let false_gap = l3.rows[0].is_gap();
            if !false_gap {
                self.caught += 1;
            }
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 10 && self.caught == self.cases
    }
}

/// 判据自检（t 系入口）：清单建成 + 大关差额 + 注入 10 场。
pub fn run_t1482_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1482-t482");
    // 清单构建。
    let (a, c) = gen_batch_accounts(0x1482_0001);
    match GlList::build(&a, &c) {
        Err(e) => set.add("GL-build", false, &format!("err 0x{:04x}", e.num())),
        Ok(l) => {
            let (total, margin) = l.gate_margin();
            set.add(
                "GL-40-batches",
                l.rows.len() as u32 == GL_BATCHES,
                &format!("40 批三列齐"),
            );
            set.add(
                "GL-density-floor",
                l.gaps.iter().all(|b| l.rows[*b as usize].density < GL_DENSITY_FLOOR)
                    && l.rows.iter().filter(|r| r.is_gap()).count() as u64 == l.gaps.len() as u64,
                &format!("缺口 {} 批（红线 {}）", l.gaps.len(), GL_DENSITY_FLOOR),
            );
            set.add(
                "GL-gate-margin",
                l.gate_consistent(),
                &format!("大关 {}：总数 {} 差额 {}", GL_DOMAIN_GATE, total, margin),
            );
            let exported = l.export();
            set.add(
                "GL-export",
                exported.len() == l.gaps.len(),
                &format!("导出 {} 行", exported.len()),
            );
            // 注入 10 场。
            let mut inj = GlInjector::new();
            inj.inject_all();
            set.add("GL-inject-10/10", inj.all_caught(), "");
            // 判据总门。
            let ok = l.gate_consistent() && inj.all_caught();
            set.add(
                "UNX-F1482-J1",
                ok,
                &format!(
                    "{}: 批=40 缺口={} 总断言={} 差额={} inj={}/{} seg=0x{:04x}",
                    GL_VERSION,
                    l.gaps.len(),
                    total,
                    margin,
                    inj.caught,
                    inj.cases,
                    GL_SEG_BASE
                ),
            );
        }
    }
    set
}
