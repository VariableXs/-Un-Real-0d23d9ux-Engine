//! veb400_final —— VE-F0400 B 域终验与 W1 中点里程碑。
//! 锚点（CoRun Unxreal 主册）：VE-F0400 · B 域终验与 W1 中点里程碑 · 目标 340 行
//! 判据原文：VE-F0400-J1 终验集五证：聚合自检卡绿、验收矩阵收、
//!           完成章在、移交包封、知识库非空；五证齐出 W1 中点
//!           里程碑章（W1-MID + 域号哈希）；缺证报证名；里程碑
//!           一经签发不可撤销；里程碑台账（签发时间 + 哈希核）。
//! 组件：五证收集器 + 里程碑签发器 + 不可撤销纪律 + 台账核。
//! 职责定位：终验是「B 域的中期大考」——五证是五份成绩单：聚合
//!           卡是平时分，验收是期末卷，完成章是毕业证，移交包
//!           是交接清单，知识库是学习档案；W1 中点里程碑是整条
//!           演进线的半程碑，签发不可撤销是里程碑的严肃性所在。
//! 性能逐项分解：证检 O(5)；签发 O(1)；台账 O(里程碑数)。
//! 诊断码段：0x30980（0x30981~0x3098A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FN_VERSION: &str = "veb400_final-v1";

/// 诊断段基址（0x30980 段 · 本模块独占）。
pub const FN_SEG_BASE: u32 = 0x30980;
pub const XC_MISS_AGG: u32 = 0x30981; // 缺聚合卡
pub const XC_MISS_ACC: u32 = 0x30982; // 缺验收
pub const XC_MISS_DONE: u32 = 0x30983; // 缺完成章
pub const XC_MISS_HAND: u32 = 0x30984; // 缺移交包
pub const XC_MISS_KB: u32 = 0x30985; // 缺知识库
pub const XC_MAGIC: u32 = 0x30986; // 账魔数破
pub const XC_IRREV: u32 = 0x30987; // 不可撤销
pub const XC_TWICE: u32 = 0x30988; // 重复签发
pub const XC_LEDGER: u32 = 0x3098A; // 台账核破

/// 五证。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveProof {
    pub agg_green: bool,
    pub accepted: bool,
    pub done_stamped: bool,
    pub handover_sealed: bool,
    pub kb_nonempty: bool,
}

fn ms_hash(domain: &str, ts: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in domain.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^= ts;
    h = h.wrapping_mul(0x0100_0193);
    h
}

/// 里程碑记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsRec {
    pub domain: &'static str,
    pub ts: u32,
    pub hash: u32,
}

/// 终验签发器。
pub struct FinalGate {
    recs: Vec<MsRec>,
    magic: u32,
}

impl FinalGate {
    /// 建。
    pub fn new() -> FinalGate {
        FinalGate { recs: Vec::new(), magic: 0x0980_F1A4 }
    }

    /// 五证检查：缺证报证名。
    pub fn check(&self, p: &FiveProof) -> Result<(), u32> {
        if self.magic != 0x0980_F1A4 {
            return Err(XC_MAGIC);
        }
        if !p.agg_green {
            return Err(XC_MISS_AGG);
        }
        if !p.accepted {
            return Err(XC_MISS_ACC);
        }
        if !p.done_stamped {
            return Err(XC_MISS_DONE);
        }
        if !p.handover_sealed {
            return Err(XC_MISS_HAND);
        }
        if !p.kb_nonempty {
            return Err(XC_MISS_KB);
        }
        Ok(())
    }

    /// 签发 W1 中点里程碑。
    pub fn issue(&mut self, domain: &'static str, p: &FiveProof, ts: u32) -> Result<u32, u32> {
        self.check(p)?;
        // 同域不可重复签发。
        for r in self.recs.iter() {
            if r.domain == domain {
                return Err(XC_TWICE);
            }
        }
        let h = ms_hash(domain, ts);
        self.recs.push(MsRec { domain, ts, hash: h });
        Ok(h)
    }

    /// 撤销拒（不可撤销纪律：接口只读，无撤销入口；此核验证账
    /// 面签发后不变）。
    pub fn verify(&self, domain: &str, ts: u32) -> Result<bool, u32> {
        for r in self.recs.iter() {
            if r.domain == domain {
                return Ok(r.hash == ms_hash(domain, r.ts) && r.ts == ts);
            }
        }
        Err(XC_LEDGER)
    }

    /// 台账核。
    pub fn audit(&self) -> Result<u32, u32> {
        for r in self.recs.iter() {
            if r.hash != ms_hash(r.domain, r.ts) {
                return Err(XC_LEDGER);
            }
        }
        Ok(self.recs.len() as u32)
    }

    /// 里程碑数。
    pub fn count(&self) -> usize {
        self.recs.len()
    }
}

pub fn run_veb400_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0400");
    let full = FiveProof {
        agg_green: true, accepted: true, done_stamped: true,
        handover_sealed: true, kb_nonempty: true,
    };
    // 族一：逐证缺报。
    let mut g = FinalGate::new();
    let p1 = FiveProof { agg_green: false, ..full };
    s.add("缺聚合卡", g.check(&p1) == Err(XC_MISS_AGG), "证一");
    let p2 = FiveProof { accepted: false, ..full };
    s.add("缺验收", g.check(&p2) == Err(XC_MISS_ACC), "证二");
    let p3 = FiveProof { done_stamped: false, ..full };
    s.add("缺完成章", g.check(&p3) == Err(XC_MISS_DONE), "证三");
    let p4 = FiveProof { handover_sealed: false, ..full };
    s.add("缺移交包", g.check(&p4) == Err(XC_MISS_HAND), "证四");
    let p5 = FiveProof { kb_nonempty: false, ..full };
    s.add("缺知识库", g.check(&p5) == Err(XC_MISS_KB), "证五");
    s.add("五证齐过", g.check(&full).is_ok(), "放行");
    // 族二：签发。
    let h1 = g.issue("VE-B", &full, 0x57A1_0001);
    s.add("签发出章", h1.is_ok(), "W1-MID");
    s.add("台账 1", g.count() == 1, "一碑");
    s.add("重复签发拒", g.issue("VE-B", &full, 2) == Err(XC_TWICE), "一域一碑");
    s.add("异域可签", g.issue("VE-C", &full, 0x57A1_0002).is_ok(), "并行域");
    s.add("台账 2", g.count() == 2, "两碑");
    // 族三：不可撤销（账面核）。
    s.add("章核真", g.verify("VE-B", 0x57A1_0001) == Ok(true), "哈希对");
    s.add("错时核伪", g.verify("VE-B", 7) == Ok(false), "时间戳不齐");
    s.add("野域查拒", g.verify("ghost", 1) == Err(XC_LEDGER), "无碑");
    // 族四：缺证签发拒。
    let mut k = FinalGate::new();
    let badp = FiveProof { accepted: false, ..full };
    s.add("缺证签发拒", k.issue("VE-B", &badp, 1) == Err(XC_MISS_ACC), "证先行");
    s.add("拒后无碑", k.count() == 0, "零台账");
    // 族五：审计。
    s.add("审计过", g.audit() == Ok(2), "两碑核真");
    s.add("魔数核", FN_SEG_BASE == 0x30980, "段基址");
    s
}
