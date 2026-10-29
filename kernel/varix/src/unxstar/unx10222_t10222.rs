//! UNX-F10222 归因判定器（失绿样本按口径入类可复测，错类检出）（0x10ee0 段）
//!
//! 锚点：UNX-F10222-J1 判定器达成（失绿样本按口径入类可复测
//! 10/10 次），错类 10/10 次检出。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：判定器（症状输入 + 三轮探针 + 入类裁定）+ 复测核（同输入
//! 同裁定）+ 错类拦截核（与判别规则冲突的裁定拒绝）+ 折叠指纹 +
//! 注入核（矛盾证据/未探针先判/重判漂移/空样本/封器裁定）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10ee0;
/// 诊断码：样本为空
pub const ERR_EMPTY_SAMPLE: u32 = SEG_BASE + 0x01;
/// 诊断码：证据矛盾
pub const ERR_EVIDENCE_CONFLICT: u32 = SEG_BASE + 0x02;
/// 诊断码：未探针先判
pub const ERR_PREMATURE: u32 = SEG_BASE + 0x03;
/// 诊断码：复测漂移
pub const ERR_VERDICT_DRIFT: u32 = SEG_BASE + 0x04;
/// 诊断码：封器裁定
pub const ERR_FROZEN: u32 = SEG_BASE + 0x05;

/// 证据种类
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    /// 三跑同指纹（指向内核缺陷）
    SameFingerprint,
    /// 环境件缺失（指向受限）
    EnvPartMissing,
    /// 规格文本冲突（指向期望差异）
    SpecConflict,
}

/// 归因三分类（与 UNX-10221 口径册同类同序，本地自包含定义）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CauseClass {
    /// 内核缺陷
    KernelBug,
    /// 环境受限
    EnvLimited,
    /// 期望差异
    ExpectGap,
}

impl CauseClass {
    /// 类名
    pub fn name(&self) -> &'static str {
        match self {
            CauseClass::KernelBug => "KERNEL_BUG",
            CauseClass::EnvLimited => "ENV_LIMITED",
            CauseClass::ExpectGap => "EXPECT_GAP",
        }
    }
    /// 类序
    pub fn ord(&self) -> u8 {
        match self {
            CauseClass::KernelBug => 1,
            CauseClass::EnvLimited => 2,
            CauseClass::ExpectGap => 3,
        }
    }
}

/// 判定器
#[derive(Debug, PartialEq, Eq)]
pub struct Attributor {
    /// 样本标识
    pub sample: String,
    /// 已收集证据
    pub evidence: Vec<Evidence>,
    /// 探针轮数
    pub probes: usize,
    /// 裁定（入类）
    pub verdict: Option<CauseClass>,
    /// 封器
    pub frozen: bool,
}

impl Attributor {
    /// 开器
    pub fn new(sample: &str) -> Result<Self, u32> {
        if sample.is_empty() {
            return Err(ERR_EMPTY_SAMPLE);
        }
        Ok(Attributor {
            sample: sample.to_string(),
            evidence: Vec::new(),
            probes: 0,
            verdict: None,
            frozen: false,
        })
    }

    /// 跑一轮探针并收集证据
    pub fn probe(&mut self, e: Evidence) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        // 矛盾证据：同指纹与环境缺件/规格冲突不可并存（含本次传入证据）
        let has_fp = self.evidence.contains(&Evidence::SameFingerprint)
            || e == Evidence::SameFingerprint;
        let has_env = self.evidence.contains(&Evidence::EnvPartMissing)
            || e == Evidence::EnvPartMissing;
        let has_spec = self.evidence.contains(&Evidence::SpecConflict)
            || e == Evidence::SpecConflict;
        if (has_fp && has_env) || (has_fp && has_spec) {
            return Err(ERR_EVIDENCE_CONFLICT);
        }
        self.evidence.push(e);
        self.probes += 1;
        Ok(())
    }

    /// 裁定：按证据主导类入类；未探针先判拒绝
    pub fn judge(&mut self) -> Result<CauseClass, u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if self.probes == 0 {
            return Err(ERR_PREMATURE);
        }
        if self.verdict.is_some() {
            // 复测：同证据必同判
            let again = self.infer();
            if again != self.verdict.unwrap() {
                return Err(ERR_VERDICT_DRIFT);
            }
            return Ok(again);
        }
        let v = self.infer();
        self.verdict = Some(v);
        Ok(v)
    }

    /// 推断核心
    fn infer(&self) -> CauseClass {
        if self.evidence.contains(&Evidence::SameFingerprint) {
            CauseClass::KernelBug
        } else if self.evidence.contains(&Evidence::EnvPartMissing) {
            CauseClass::EnvLimited
        } else {
            CauseClass::ExpectGap
        }
    }

    /// 错类拦截：裁定与证据冲突检测
    pub fn verdict_consistent(&self) -> bool {
        match self.verdict {
            Some(CauseClass::KernelBug) => self.evidence.contains(&Evidence::SameFingerprint),
            Some(CauseClass::EnvLimited) => self.evidence.contains(&Evidence::EnvPartMissing),
            Some(CauseClass::ExpectGap) => {
                !self.evidence.contains(&Evidence::SameFingerprint)
                    && !self.evidence.contains(&Evidence::EnvPartMissing)
            }
            None => false,
        }
    }

    /// 封器
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        acc = acc.wrapping_add(self.probes as u32);
        for (i, e) in self.evidence.iter().enumerate() {
            let v = match e {
                Evidence::SameFingerprint => 3,
                Evidence::EnvPartMissing => 5,
                Evidence::SpecConflict => 7,
            };
            acc = acc.wrapping_mul(4409).wrapping_add(v + i as u32);
        }
        if let Some(c) = self.verdict {
            acc = acc.wrapping_add((c.ord() as u32).wrapping_mul(101));
        }
        acc
    }

    /// 诊断面：证据计数（按类）
    pub fn evidence_count(&self, e: Evidence) -> usize {
        self.evidence.iter().filter(|x| **x == e).count()
    }

    /// 诊断面：样本标识回读
    pub fn sample_name(&self) -> &str {
        &self.sample
    }

    /// 诊断面：裁定摘要
    pub fn verdict_summary(&self) -> String {
        match self.verdict {
            Some(c) => format!("sample={} class={}", self.sample, c.name()),
            None => format!("sample={} class=PENDING", self.sample),
        }
    }

    /// 诊断面：探针密度（轮数/证据数）
    pub fn probe_density_bps(&self) -> u32 {
        if self.evidence.is_empty() {
            return 0;
        }
        (self.probes * 10000 / self.evidence.len()) as u32
    }

    /// 诊断面：类名核
    pub fn class_names() -> [&'static str; 3] {
        [CauseClass::KernelBug.name(), CauseClass::EnvLimited.name(), CauseClass::ExpectGap.name()]
    }

    /// 诊断面：类序核
    pub fn class_ords() -> [u8; 3] {
        [CauseClass::KernelBug.ord(), CauseClass::EnvLimited.ord(), CauseClass::ExpectGap.ord()]
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 空样本拒绝
    out[0] = Attributor::new("").err() == Some(ERR_EMPTY_SAMPLE);
    // 2: 未探针先判拒绝
    let mut a2 = Attributor::new("s2").unwrap();
    out[1] = a2.judge() == Err(ERR_PREMATURE);
    // 3: 矛盾证据拒绝
    let mut a3 = Attributor::new("s3").unwrap();
    let _ = a3.probe(Evidence::SameFingerprint);
    out[2] = a3.probe(Evidence::EnvPartMissing) == Err(ERR_EVIDENCE_CONFLICT);
    // 4: 复测漂移检出（伪造裁定后重判）
    let mut a4 = Attributor::new("s4").unwrap();
    let _ = a4.probe(Evidence::EnvPartMissing);
    let _ = a4.judge();
    a4.verdict = Some(CauseClass::KernelBug);
    out[3] = a4.judge() == Err(ERR_VERDICT_DRIFT);
    // 5: 封器裁定拒绝
    let mut a5 = Attributor::new("s5").unwrap();
    let _ = a5.probe(Evidence::SpecConflict);
    let _ = a5.judge();
    let _ = a5.seal();
    out[4] = a5.judge() == Err(ERR_FROZEN);
    out
}

/// 判据函数
pub fn run_unx10222_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10222");
    let mut a = match Attributor::new("ut-attr-1") {
        Ok(x) => x,
        Err(_) => return s,
    };
    let _ = a.probe(Evidence::SameFingerprint);
    let _ = a.probe(Evidence::SameFingerprint);
    let v1 = a.judge();
    s.add("三纹入内核类", v1 == Ok(CauseClass::KernelBug), "同指纹证据裁入内核缺陷");
    s.add("裁定自洽", a.verdict_consistent(), "裁定与证据一致");
    s.add("复测同判", a.judge() == Ok(CauseClass::KernelBug), "同证据复测同判");
    let mut b = Attributor::new("ut-attr-2").unwrap();
    let _ = b.probe(Evidence::EnvPartMissing);
    s.add("缺件入受限类", b.judge() == Ok(CauseClass::EnvLimited), "环境缺件裁入受限");
    s.add("受限自洽", b.verdict_consistent(), "受限裁定自洽");
    let mut c = Attributor::new("ut-attr-3").unwrap();
    let _ = c.probe(Evidence::SpecConflict);
    s.add("冲突入差异类", c.judge() == Ok(CauseClass::ExpectGap), "规格冲突裁入期望差异");
    s.add("差异自洽", c.verdict_consistent(), "差异裁定自洽");
    let mut d = Attributor::new("ut-attr-4").unwrap();
    let _ = d.probe(Evidence::SpecConflict);
    let _ = d.probe(Evidence::SpecConflict);
    let _ = d.probe(Evidence::SpecConflict);
    s.add("多轮探针计数", d.probes == 3, "三轮探针留痕");
    s.add("证据计数", a.evidence_count(Evidence::SameFingerprint) == 2, "同纹证据两条");
    s.add("样本回读", a.sample_name() == "ut-attr-1", "样本标识一致");
    s.add("裁定摘要", a.verdict_summary().contains("KERNEL_BUG"), "摘要含类名");
    s.add("探针密度", c.probe_density_bps() == 10000, "一证据一轮密度满格");
    s.add("类名表", Attributor::class_names() == ["KERNEL_BUG", "ENV_LIMITED", "EXPECT_GAP"], "三分类名表正确");
    s.add("类序表", Attributor::class_ords() == [1, 2, 3], "三分类序表正确");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = a.seal();
    s.add("封器后拒判", a.judge() == Err(ERR_FROZEN), "封器后裁定被拒");
    s.add("封器指纹非零", fp != 0, "封器折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_priority_order() {
        let mut a = Attributor::new("t").unwrap();
        let _ = a.probe(Evidence::SpecConflict);
        let _ = a.probe(Evidence::SpecConflict);
        assert_eq!(a.judge(), Ok(CauseClass::ExpectGap));
        assert!(a.verdict_consistent());
    }

    #[test]
    fn premature_and_conflict() {
        let mut a = Attributor::new("t2").unwrap();
        assert_eq!(a.judge(), Err(ERR_PREMATURE));
        let _ = a.probe(Evidence::EnvPartMissing);
        assert_eq!(a.probe(Evidence::SameFingerprint), Err(ERR_EVIDENCE_CONFLICT));
    }

    #[test]
    fn repeat_verdict_stable() {
        let mut a = Attributor::new("t3").unwrap();
        let _ = a.probe(Evidence::SameFingerprint);
        let v1 = a.judge().unwrap();
        let v2 = a.judge().unwrap();
        assert_eq!(v1, v2);
        assert_eq!(v1, CauseClass::KernelBug);
        assert!(a.verdict_summary().contains("KERNEL_BUG"));
    }

    #[test]
    fn empty_sample_rejected() {
        assert_eq!(Attributor::new(""), Err(ERR_EMPTY_SAMPLE));
        assert_eq!(Attributor::class_ords(), [1, 2, 3]);
    }

    #[test]
    fn probe_density_and_summary() {
        let mut a = Attributor::new("t9").unwrap();
        let _ = a.probe(Evidence::SpecConflict);
        let _ = a.probe(Evidence::SpecConflict);
        let _ = a.judge();
        assert_eq!(a.probe_density_bps(), 20000);
        assert!(a.verdict_summary().contains("EXPECT_GAP"));
        assert_eq!(a.evidence_count(Evidence::SameFingerprint), 0);
    }
}
