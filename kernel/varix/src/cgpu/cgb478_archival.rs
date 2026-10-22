//! CGPU-F0478 · 基线归档（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 基线归档：证据三件套归档（fuzz/基准/验收记录）；
//! - 三级索引（域/组/项）；双签（组签+域签）；完整性校验。
//!   判据：归档、索引、双签、校验、判据。
//!
//! 设计纪律：
//! - 三件套结构化：fuzz 记录（案例数+全绿）、基准记录（指标数+冻结
//!   版本）、验收记录（维度数+总分）——任一件缺失即归档不完整。
//! - 三级索引：域级（C 域唯一）→组级（9 组）→项级（N 项），索引
//!   完整=每组有项、每项有指针。
//! - 双签：组签+域签指纹互异+时间戳非零（与 F0464 双签同构）。
//! - 完整性校验：三件套 CRC 快照一致——归档后内容变更可检出。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1110 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1110，全仓独占）。
pub const SEG_BASE: u16 = 0x1110;

pub const E_AR_PIECE: u16 = SEG_BASE + 1; // 三件套缺失
pub const E_AR_INDEX: u16 = SEG_BASE + 2; // 索引不完整
pub const E_AR_SIGN: u16 = SEG_BASE + 3; // 双签不成立
pub const E_AR_TAMPER: u16 = SEG_BASE + 4; // 完整性校验失败

/// 三件套。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceBundle {
    /// fuzz：案例数（≥1）。
    pub fuzz_cases: u32,
    /// 基准：指标数（=6）。
    pub bench_metrics: u32,
    /// 验收：维度数（=20）与总分（‰）。
    pub accept_dims: u32,
    pub accept_total_pm: u64,
}

impl EvidenceBundle {
    pub fn complete(&self) -> bool {
        self.fuzz_cases >= 1 && self.bench_metrics == 6 && self.accept_dims == 20 && self.accept_total_pm >= 850
    }
    fn digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for v in [self.fuzz_cases, self.bench_metrics, self.accept_dims] {
            h = (h ^ v).wrapping_mul(0x0100_0193);
        }
        h = (h ^ (self.accept_total_pm as u32)).wrapping_mul(0x0100_0193);
        h
    }
}

/// 三级索引（域→组→项指针）。
#[derive(Debug, Default, Clone)]
pub struct ArchivalIndex {
    /// 每组的项指针数（域级唯一，组级 9，项级任意）。
    pub group_items: Vec<(u8, u32)>,
}

impl ArchivalIndex {
    /// 索引完整：九组全在册且每组至少一项。
    pub fn complete(&self) -> bool {
        if self.group_items.len() != 9 {
            return false;
        }
        self.group_items.iter().all(|(_, n)| *n >= 1)
    }
    pub fn total_items(&self) -> u32 {
        self.group_items.iter().map(|(_, n)| n).sum()
    }
}

/// 双签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualSign {
    pub group_fp: u32,
    pub domain_fp: u32,
    pub at_tick: u64,
}

fn fp(tag: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in tag.as_bytes() {
        h = (h ^ *b as u32).wrapping_mul(0x0100_0193);
    }
    h
}

impl DualSign {
    pub fn make(group_tag: &str, domain_tag: &str, at_tick: u64) -> Result<Self, u16> {
        let g = fp(group_tag);
        let d = fp(domain_tag);
        if g == d || at_tick == 0 {
            return Err(E_AR_SIGN);
        }
        Ok(Self { group_fp: g, domain_fp: d, at_tick })
    }
    pub fn complete(&self) -> bool {
        self.group_fp != 0 && self.domain_fp != 0 && self.group_fp != self.domain_fp && self.at_tick > 0
    }
}

/// 归档包。
#[derive(Debug)]
pub struct Archive {
    pub bundle: EvidenceBundle,
    /// 归档时快照摘要（完整性基准）。
    pub sealed_digest: u32,
    pub sign: DualSign,
    pub index: ArchivalIndex,
}

impl Archive {
    pub fn seal(bundle: EvidenceBundle, sign: DualSign, index: ArchivalIndex) -> Result<Archive, u16> {
        if !bundle.complete() {
            return Err(E_AR_PIECE);
        }
        if !sign.complete() {
            return Err(E_AR_SIGN);
        }
        if !index.complete() {
            return Err(E_AR_INDEX);
        }
        Ok(Archive { sealed_digest: bundle.digest(), bundle, sign, index })
    }
    /// 完整性校验：重算摘要与封存值比对。
    pub fn verify(&self) -> Result<(), u16> {
        if self.bundle.digest() != self.sealed_digest {
            return Err(E_AR_TAMPER);
        }
        Ok(())
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、三件套完整性。
    {
        let good = EvidenceBundle { fuzz_cases: 12, bench_metrics: 6, accept_dims: 20, accept_total_pm: 902 };
        v.push(("F0478-三件套-齐备通过", good.complete()));
        let no_fuzz = EvidenceBundle { fuzz_cases: 0, bench_metrics: 6, accept_dims: 20, accept_total_pm: 902 };
        v.push(("F0478-三件套-fuzz缺失拒", !no_fuzz.complete()));
        let low = EvidenceBundle { fuzz_cases: 3, bench_metrics: 6, accept_dims: 20, accept_total_pm: 800 };
        v.push(("F0478-三件套-验收总分不足拒", !low.complete()));
    }
    // 二、三级索引。
    {
        let full = ArchivalIndex { group_items: vec![(1, 2), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1), (7, 1), (8, 1), (9, 3)] };
        v.push(("F0478-索引-九组全在册", full.complete() && full.total_items() == 12));
        let miss = ArchivalIndex { group_items: vec![(1, 2), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1), (7, 1), (8, 1)] };
        v.push(("F0478-索引-缺组拒", !miss.complete()));
        let empty_group = ArchivalIndex { group_items: vec![(1, 0), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1), (7, 1), (8, 1), (9, 1)] };
        v.push(("F0478-索引-零项组拒", !empty_group.complete()));
    }
    // 三、双签。
    {
        let s = DualSign::make("C10-lead", "domain-c", 5000);
        v.push(("F0478-双签-成立", s.as_ref().map(|x| x.complete()) == Ok(true)));
        v.push(("F0478-双签-同角色拒", DualSign::make("x", "x", 1) == Err(E_AR_SIGN)));
        v.push(("F0478-双签-零时间戳拒", DualSign::make("a", "b", 0) == Err(E_AR_SIGN)));
    }
    // 四、归档封存+完整性校验。
    {
        let good = EvidenceBundle { fuzz_cases: 9, bench_metrics: 6, accept_dims: 20, accept_total_pm: 880 };
        let sign = DualSign::make("g9", "dc", 7000).unwrap_or(DualSign { group_fp: 1, domain_fp: 2, at_tick: 7000 });
        let idx = ArchivalIndex { group_items: vec![(1, 1), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1), (7, 1), (8, 1), (9, 2)] };
        let arc = Archive::seal(good, sign, idx.clone());
        v.push(("F0478-归档-封存通过", arc.is_ok()));
        let a = arc.unwrap_or(Archive {
            sealed_digest: 0,
            bundle: EvidenceBundle { fuzz_cases: 1, bench_metrics: 6, accept_dims: 20, accept_total_pm: 900 },
            sign,
            index: idx,
        });
        v.push(("F0478-归档-完整性通过", a.verify().is_ok()));
        // 篡改检出：改 bundle 后 digest 不匹配。
        let mut tampered = Archive {
            sealed_digest: a.sealed_digest,
            bundle: a.bundle.clone(),
            sign: a.sign,
            index: a.index,
        };
        tampered.bundle.accept_total_pm = 999;
        v.push(("F0478-归档-篡改检出", tampered.verify() == Err(E_AR_TAMPER)));
    }
    // 五、判据自检。
    {
        v.push(("F0478-判据-摘要确定性", {
            let b1 = EvidenceBundle { fuzz_cases: 2, bench_metrics: 6, accept_dims: 20, accept_total_pm: 900 };
            let b2 = EvidenceBundle { fuzz_cases: 2, bench_metrics: 6, accept_dims: 20, accept_total_pm: 900 };
            b1.digest() == b2.digest()
        }));
        v.push(("F0478-判据-三件套红线", !EvidenceBundle { fuzz_cases: 1, bench_metrics: 5, accept_dims: 20, accept_total_pm: 900 }.complete()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0478_archival_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
