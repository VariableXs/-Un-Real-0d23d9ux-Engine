//! UNX-F0160 · ktest 引导面 B08 批断言集 · 诊断段 0x26D0
//!
//! 判据原文（UNX-F0160-J1）：B08 批成员（F0145~F0159 十五模块，构建
//! 工具链面）drill 聚合一次全绿；断言账按成员闭集逐项记账，红项带
//! 成员定位；与 B07（unx0140）同构但闭集独立。
//!
//! 结构：成员闭集 + drill 表 + 结果账（绿/红 + 码）+ 全绿判定 +
//! 守恒审计。零浮点、零 unwrap/expect。

use crate::alloc::{format, string::String, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x26E0;

const E_MEMBER: u16 = SEG_BASE + 1; // 成员 drill 红
const E_BAL: u16 = SEG_BASE + 2; // 断言账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 3; // 批已冻结拒重跑

/// B08 成员闭集（15 模块）。
pub const MEMBERS: [&str; 15] = [
    "unx0145_makeentry",
    "unx0146_reprobuild",
    "unx0147_incbuild",
    "unx0148_verinject",
    "unx0149_sbom",
    "unx0150_imgsplit",
    "unx0151_writetool",
    "unx0152_wverify",
    "unx0153_multitarget",
    "unx0154_qsmoke",
    "unx0155_checksum",
    "unx0156_builddiag",
    "unx0157_imgstats",
    "unx0158_archnaming",
    "unx0159_slowmedia",
];

type DrillFn = fn() -> Result<(), u16>;

/// 成员结果行。
#[derive(Clone)]
pub struct MemberRow {
    pub name: &'static str,
    pub ok: bool,
    pub err: Option<u16>,
}

/// B08 批断言册。
pub struct Ktb08Book {
    pub rows: Vec<MemberRow>,
    pub runs: u32,
    pub frozen: bool,
}

impl Ktb08Book {
    pub fn new() -> Result<Self, u16> {
        Ok(Ktb08Book { rows: Vec::new(), runs: 0, frozen: false })
    }

    /// 整批跑：逐成员 drill，不短路（全景账）。
    pub fn run_batch(&mut self, drills: &[DrillFn]) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if drills.len() != MEMBERS.len() {
            return Err(E_BAL);
        }
        self.rows.clear();
        self.runs += 1;
        for (i, d) in drills.iter().enumerate() {
            let r = match d() {
                Ok(()) => MemberRow { name: MEMBERS[i], ok: true, err: None },
                Err(c) => MemberRow { name: MEMBERS[i], ok: false, err: Some(c) },
            };
            self.rows.push(r);
        }
        Ok(())
    }

    /// 全绿判定。
    pub fn all_green(&self) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Err(E_BAL);
        }
        for r in &self.rows {
            if !r.ok {
                return Err(E_MEMBER);
            }
        }
        Ok(())
    }

    /// 守恒：行数 == 闭集长；行序对齐；绿红与码互补。
    pub fn audit(&self) -> Result<(), u16> {
        if self.rows.len() != MEMBERS.len() {
            return Err(E_BAL);
        }
        for (i, r) in self.rows.iter().enumerate() {
            if r.name != MEMBERS[i] {
                return Err(E_BAL);
            }
            if r.ok == r.err.is_some() {
                return Err(E_BAL);
            }
        }
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 渲染一览。
    pub fn render(&self) -> String {
        let green = self.rows.iter().filter(|r| r.ok).count();
        let mut out = format!("B08 批 {}/{} 绿 跑{} 次\n", green, MEMBERS.len(), self.runs);
        for r in &self.rows {
            if r.ok {
                out.push_str(&format!("绿 {}\n", r.name));
            } else {
                out.push_str(&format!("红 {}({:04X})\n", r.name, r.err.unwrap_or(0)));
            }
        }
        out
    }
}

/// 真实成员 drill 表（与 MEMBERS 严格同序）。
pub fn real_drills() -> Vec<DrillFn> {
    vec![
        crate::unxstar::unx0145_makeentry::drill,
        crate::unxstar::unx0146_reprobuild::drill,
        crate::unxstar::unx0147_incbuild::drill,
        crate::unxstar::unx0148_verinject::drill,
        crate::unxstar::unx0149_sbom::drill,
        crate::unxstar::unx0150_imgsplit::drill,
        crate::unxstar::unx0151_writetool::drill,
        crate::unxstar::unx0152_wverify::drill,
        crate::unxstar::unx0153_multitarget::drill,
        crate::unxstar::unx0154_qsmoke::drill,
        crate::unxstar::unx0155_checksum::drill,
        crate::unxstar::unx0156_builddiag::drill,
        crate::unxstar::unx0157_imgstats::drill,
        crate::unxstar::unx0158_archnaming::drill,
        crate::unxstar::unx0159_slowmedia::drill,
    ]
}

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：真实成员整批跑——一次全绿。
    let mut b = Ktb08Book::new()?;
    let drills = real_drills();
    b.run_batch(&drills)?;
    b.all_green()?;
    b.audit()?;
    hits += 1;

    // L2：重跑账单调 + 渲染 15/15。
    b.run_batch(&drills)?;
    if b.runs != 2 {
        return Err(E_BAL);
    }
    let rep = b.render();
    if !rep.contains("15/15 绿") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：表长不符拦截。
    let mut i1 = Ktb08Book::new()?;
    let short: Vec<DrillFn> = real_drills().into_iter().take(10).collect();
    if i1.run_batch(&short) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：注假红——定位成员，其余 14 绿。
    fn fake_red() -> Result<(), u16> {
        Err(0x60EF)
    }
    let mut i2 = Ktb08Book::new()?;
    let mut fd = real_drills();
    fd[11] = fake_red as DrillFn; // unx0156 槽位注红
    i2.run_batch(&fd)?;
    if i2.all_green() != Err(E_MEMBER) {
        return Err(E_MEMBER);
    }
    if i2.rows[11].err != Some(0x60EF) {
        return Err(E_BAL);
    }
    if i2.rows.iter().filter(|r| r.ok).count() != 14 {
        return Err(E_BAL);
    }
    i2.audit()?;
    hits += 1;

    // I3：未跑批判绿 / 冻结重跑双拦截。
    let i3 = Ktb08Book::new()?;
    if i3.all_green() != Err(E_BAL) {
        return Err(E_BAL);
    }
    let mut i4 = Ktb08Book::new()?;
    i4.freeze();
    if i4.run_batch(&real_drills()) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
