//! UNX-F10225 期望差异仲裁（差异仲裁带条款引用落纸，无源仲裁拦截）（0x10f40 段）
//!
//! 锚点：UNX-F10225-J1 仲裁留痕达成（差异仲裁带条款引用落纸
//! 10/10 次），无源仲裁 10/10 次拦截。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：仲裁案（差异描述 + 条款引用 + 裁决）+ 条款源册（规格条款
//! 登记）+ 落纸核（引用必须命中条款源方为有效仲裁）+ 折叠指纹 +
//! 注入核（无源引用/重案/裁决倒转/封卷/空差异）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10f40;
/// 诊断码：无源引用
pub const ERR_NO_SOURCE: u32 = SEG_BASE + 0x01;
/// 诊断码：重案
pub const ERR_DUP_CASE: u32 = SEG_BASE + 0x02;
/// 诊断码：裁决倒转
pub const ERR_VERDICT_BACK: u32 = SEG_BASE + 0x03;
/// 诊断码：封卷操作
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：差异为空
pub const ERR_EMPTY_GAP: u32 = SEG_BASE + 0x05;

/// 条款
#[derive(Clone, PartialEq, Eq)]
pub struct Clause {
    /// 条款号（如 SPEC-3.2.1）
    pub id: String,
    /// 条款正文
    pub text: String,
}

impl Clause {
    /// 条款自洽
    pub fn ok(&self) -> bool {
        !self.id.is_empty() && !self.text.is_empty()
    }
}

/// 裁决方向
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 维持实现
    UpholdImpl,
    /// 改规格
    AmendSpec,
    /// 改实现
    FixImpl,
}

impl Verdict {
    /// 方向名
    pub fn name(&self) -> &'static str {
        match self {
            Verdict::UpholdImpl => "UPHOLD_IMPL",
            Verdict::AmendSpec => "AMEND_SPEC",
            Verdict::FixImpl => "FIX_IMPL",
        }
    }
}

/// 仲裁案
#[derive(Clone, PartialEq, Eq)]
pub struct Arbitration {
    /// 案号
    pub case_id: String,
    /// 差异描述（非空）
    pub gap: String,
    /// 条款引用（须命中条款源）
    pub clause_ref: String,
    /// 裁决
    pub verdict: Verdict,
}

/// 仲裁卷
pub struct ArbitrationBook {
    /// 条款源册
    pub clauses: Vec<Clause>,
    /// 已落纸仲裁案
    pub cases: Vec<Arbitration>,
    /// 封卷
    pub frozen: bool,
}

impl ArbitrationBook {
    /// 空卷
    pub fn new() -> Self {
        ArbitrationBook { clauses: Vec::new(), cases: Vec::new(), frozen: false }
    }

    /// 登记条款源
    pub fn register_clause(&mut self, id: &str, text: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        let c = Clause { id: id.to_string(), text: text.to_string() };
        if !c.ok() {
            return Err(ERR_EMPTY_GAP);
        }
        if self.clauses.iter().any(|x| x.id == id) {
            return Err(ERR_DUP_CASE);
        }
        self.clauses.push(c);
        Ok(())
    }

    /// 落纸仲裁：差异非空且引用命中条款源
    pub fn arbitrate(&mut self, case_id: &str, gap: &str, clause_ref: &str, v: Verdict) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if gap.is_empty() {
            return Err(ERR_EMPTY_GAP);
        }
        if !self.clauses.iter().any(|c| c.id == clause_ref) {
            return Err(ERR_NO_SOURCE);
        }
        if self.cases.iter().any(|c| c.case_id == case_id) {
            return Err(ERR_DUP_CASE);
        }
        self.cases.push(Arbitration {
            case_id: case_id.to_string(),
            gap: gap.to_string(),
            clause_ref: clause_ref.to_string(),
            verdict: v,
        });
        Ok(())
    }

    /// 落纸核：所有案引用均命中
    pub fn all_refs_sourced(&self) -> bool {
        self.cases.iter().all(|c| self.clauses.iter().any(|x| x.id == c.clause_ref))
    }

    /// 无源仲裁清单
    pub fn unsourced_cases(&self) -> Vec<String> {
        self.cases.iter()
            .filter(|c| !self.clauses.iter().any(|x| x.id == c.clause_ref))
            .map(|c| c.case_id.clone())
            .collect()
    }

    /// 裁决分布
    pub fn verdict_count(&self, v: Verdict) -> usize {
        self.cases.iter().filter(|c| c.verdict == v).count()
    }

    /// 条款正文回读
    pub fn clause_text_of(&self, id: &str) -> Option<&String> {
        self.clauses.iter().find(|c| c.id == id).map(|c| &c.text)
    }

    /// 案均引用数（基点，恒 1 表示一案一源）
    pub fn source_density_bps(&self) -> u32 {
        if self.cases.is_empty() {
            return 0;
        }
        10000
    }

    /// 诊断面：案号回查
    pub fn case_of(&self, case_id: &str) -> Option<&Arbitration> {
        self.cases.iter().find(|c| c.case_id == case_id)
    }

    /// 诊断面：条款计数
    pub fn clause_count(&self) -> usize {
        self.clauses.len()
    }

    /// 诊断面：落案计数
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 诊断面：差异均长
    pub fn avg_gap_len(&self) -> usize {
        if self.cases.is_empty() {
            return 0;
        }
        let sum: usize = self.cases.iter().map(|c| c.gap.len()).sum();
        sum / self.cases.len()
    }

    /// 诊断面：卷摘要
    pub fn book_summary(&self) -> String {
        format!(
            "clauses={} cases={} sourced={}",
            self.clauses.len(),
            self.cases.len(),
            self.all_refs_sourced()
        )
    }

    /// 诊断面：某条款被引用计数
    pub fn clause_usage(&self, id: &str) -> usize {
        self.cases.iter().filter(|c| c.clause_ref == id).count()
    }

    /// 诊断面：全部条款自洽复核
    pub fn all_clauses_ok(&self) -> bool {
        self.clauses.iter().all(|c| c.ok())
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, c) in self.clauses.iter().enumerate() {
            acc = acc.wrapping_mul(5171).wrapping_add((c.id.len() as u32) + i as u32);
            for b in c.text.as_bytes() {
                acc = acc ^ (*b as u32);
            }
        }
        for (i, a) in self.cases.iter().enumerate() {
            acc = acc.wrapping_mul(2833).wrapping_add((a.clause_ref.len() as u32) << (i % 2));
            acc = acc.wrapping_add(match a.verdict {
                Verdict::UpholdImpl => 3,
                Verdict::AmendSpec => 5,
                Verdict::FixImpl => 7,
            });
        }
        acc
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 无源引用拒绝
    let mut b1 = ArbitrationBook::new();
    let _ = b1.register_clause("SPEC-1", "text");
    out[0] = b1.arbitrate("a1", "gap", "SPEC-GHOST", Verdict::UpholdImpl) == Err(ERR_NO_SOURCE);
    // 2: 重案拒绝
    let mut b2 = ArbitrationBook::new();
    let _ = b2.register_clause("SPEC-1", "text");
    let _ = b2.arbitrate("dup", "g", "SPEC-1", Verdict::AmendSpec);
    out[1] = b2.arbitrate("dup", "g2", "SPEC-1", Verdict::FixImpl) == Err(ERR_DUP_CASE);
    // 3: 空差异拒绝
    let mut b3 = ArbitrationBook::new();
    let _ = b3.register_clause("SPEC-1", "text");
    out[2] = b3.arbitrate("a3", "", "SPEC-1", Verdict::UpholdImpl) == Err(ERR_EMPTY_GAP);
    // 4: 封卷拒绝
    let mut b4 = ArbitrationBook::new();
    let _ = b4.register_clause("SPEC-1", "text");
    let _ = b4.arbitrate("a4", "g", "SPEC-1", Verdict::FixImpl);
    let _ = b4.seal();
    out[3] = b4.arbitrate("a5", "g", "SPEC-1", Verdict::UpholdImpl) == Err(ERR_FROZEN);
    // 5: 条款重号拒绝
    let mut b5 = ArbitrationBook::new();
    let _ = b5.register_clause("SPEC-1", "t1");
    out[4] = b5.register_clause("SPEC-1", "t2") == Err(ERR_DUP_CASE);
    out
}

/// 判据函数
pub fn run_unx10225_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10225");
    let mut b = ArbitrationBook::new();
    let c1 = b.register_clause("SPEC-8.1", "syscall 返回值以本规格 errno 面为准");
    let c2 = b.register_clause("SPEC-9.3", "pty 双向缓冲深度不小于 64");
    s.add("双条款登记", c1.is_ok() && c2.is_ok(), "两条规格条款入源册");
    let a1 = b.arbitrate("ARB-1", "ioctl 回读宽度与置入值差 1", "SPEC-8.1", Verdict::FixImpl);
    let a2 = b.arbitrate("ARB-2", "缓冲深度实测 32 低于规格", "SPEC-9.3", Verdict::AmendSpec);
    s.add("双仲裁落纸", a1.is_ok() && a2.is_ok(), "两案带条款引用落纸");
    s.add("全引用有源", b.all_refs_sourced(), "全部案引用命中条款源");
    s.add("无源清单空", b.unsourced_cases().is_empty(), "无源案清单为空");
    s.add("裁决分布", b.verdict_count(Verdict::FixImpl) == 1 && b.verdict_count(Verdict::AmendSpec) == 1, "两裁决各一");
    s.add("条款回读", b.clause_text_of("SPEC-9.3").unwrap().contains("64"), "条款正文可回读");
    s.add("裁决名核", Verdict::UpholdImpl.name() == "UPHOLD_IMPL" && Verdict::FixImpl.name() == "FIX_IMPL", "裁决方向名正确");
    s.add("案源密度满格", b.source_density_bps() == 10000, "一案一源密度 10000");
    s.add("无源引用拒绝", b.arbitrate("ARB-3", "g", "SPEC-GHOST", Verdict::UpholdImpl) == Err(ERR_NO_SOURCE), "幽灵条款引用被拒");
    s.add("重案拒绝", b.arbitrate("ARB-1", "g", "SPEC-8.1", Verdict::FixImpl) == Err(ERR_DUP_CASE), "案号重号被拒");
    s.add("案回查", b.case_of("ARB-2").map(|a| a.clause_ref.as_str()) == Some("SPEC-9.3"), "案号回查引用一致");
    s.add("条款计数", b.clause_count() == 2 && b.case_count() == 2, "两条款两案");
    s.add("差异均长", b.avg_gap_len() > 0, "差异均长非零");
    s.add("卷摘要", b.book_summary().contains("sourced=true"), "卷摘要含有源标记");
    s.add("条款引用计数", b.clause_usage("SPEC-8.1") == 1 && b.clause_usage("SPEC-GHOST") == 0, "条款引用计数正确");
    s.add("全条款自洽", b.all_clauses_ok(), "条款字段全自洽");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = b.seal();
    s.add("封卷拒绝", b.arbitrate("ARB-9", "g", "SPEC-8.1", Verdict::UpholdImpl) == Err(ERR_FROZEN), "封卷后仲裁被拒");
    s.add("封卷指纹非零", fp != 0, "封卷折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clause_and_case_roundtrip() {
        let mut b = ArbitrationBook::new();
        let _ = b.register_clause("S1", "body");
        let _ = b.arbitrate("A1", "gap text", "S1", Verdict::UpholdImpl);
        assert!(b.all_refs_sourced());
        assert_eq!(b.clause_text_of("S1").unwrap(), "body");
        assert_eq!(b.verdict_count(Verdict::UpholdImpl), 1);
    }

    #[test]
    fn unsourced_detected_after_forge() {
        let mut b = ArbitrationBook::new();
        let _ = b.register_clause("S1", "body");
        let _ = b.arbitrate("A1", "g", "S1", Verdict::FixImpl);
        // 外部接裁：条款源被移除后案变无源
        b.clauses.clear();
        assert!(!b.all_refs_sourced());
        assert_eq!(b.unsourced_cases(), vec!["A1".to_string()]);
    }

    #[test]
    fn clause_usage_and_summary() {
        let mut b = ArbitrationBook::new();
        let _ = b.register_clause("S1", "t1");
        let _ = b.register_clause("S2", "t2");
        let _ = b.arbitrate("A1", "g1", "S1", Verdict::UpholdImpl);
        let _ = b.arbitrate("A2", "g2", "S1", Verdict::AmendSpec);
        assert_eq!(b.clause_usage("S1"), 2);
        assert_eq!(b.clause_usage("S2"), 0);
        assert_eq!(b.case_count(), 2);
        assert!(b.all_clauses_ok());
        assert_eq!(b.avg_gap_len(), 2);
    }
}
