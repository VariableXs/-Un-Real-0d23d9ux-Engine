//! UNX-F0140 · ktest 引导面 B07 批断言集 · 诊断段 0x2530
//!
//! 判据原文（UNX-F0140-J1）：B07 批成员（F0125~F0139 十五模块）drill
//! 聚合一次全绿，断言账按成员闭集逐项记账，任一红即整批红并报成员名。
//!
//! 结构沿用 ktb03/ktb04/ktb05/ktb06 的聚合模式：成员闭集 + drill 表 +
//! 结果账（绿/红 + 诊断码），全绿输出一览，红项带成员定位。

use crate::alloc::{format, string::String, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2530;

const E_MEMBER: u16 = SEG_BASE + 1; // 成员 drill 红
const E_BAL: u16 = SEG_BASE + 2; // 断言账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 3; // 批已冻结拒重跑

/// B07 成员闭集（15 模块，与 mod.rs 注册闭集一致）。
pub const MEMBERS: [&str; 15] = [
    "unx0125_earlypanic",
    "unx0126_bootlogo",
    "unx0127_stagebudget",
    "unx0128_nmiwatch",
    "unx0129_recbootmenu",
    "unx0130_lastboot",
    "unx0131_bootcount",
    "unx0132_healthflag",
    "unx0133_badslot",
    "unx0134_abfallback",
    "unx0135_bootselect",
    "unx0136_diagentry",
    "unx0137_argedit",
    "unx0138_bootstat",
    "unx0139_wdhandoff",
];

/// 成员 drill 函数表（与 MEMBERS 严格同序——索引对齐由 drill_table 校验）。
type DrillFn = fn() -> Result<(), u16>;

/// 成员 drill 结果行。
#[derive(Clone)]
pub struct MemberRow {
    pub name: &'static str,
    pub ok: bool,
    pub err: Option<u16>,
}

/// B07 批断言册。
pub struct Ktb07Book {
    pub rows: Vec<MemberRow>,
    pub runs: u32,
    pub frozen: bool,
}

impl Ktb07Book {
    pub fn new() -> Result<Self, u16> {
        Ok(Ktb07Book {
            rows: Vec::new(),
            runs: 0,
            frozen: false,
        })
    }

    /// 跑整批：逐成员调 drill，红即记成员名 + 码，不短路（全景账）。
    pub fn run_batch(&mut self, drills: &[DrillFn]) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if drills.len() != MEMBERS.len() {
            return Err(E_BAL); // 表长与闭集不符——装配错误
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

    /// 全绿判定：任一红即 Err(成员定位码)。
    pub fn all_green(&self) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Err(E_BAL); // 未跑批即判绿——账面矛盾
        }
        for r in &self.rows {
            if !r.ok {
                return Err(E_MEMBER);
            }
        }
        Ok(())
    }

    /// 断言账守恒：行数 == 成员闭集长；红行必有码；绿行必无码。
    pub fn audit(&self) -> Result<(), u16> {
        if self.rows.len() != MEMBERS.len() {
            return Err(E_BAL);
        }
        for (i, r) in self.rows.iter().enumerate() {
            if r.name != MEMBERS[i] {
                return Err(E_BAL); // 行序与闭集错位
            }
            if r.ok == r.err.is_some() {
                return Err(E_BAL); // 绿红与码有无必须互补
            }
        }
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 全绿一览渲染。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let green = self.rows.iter().filter(|r| r.ok).count();
        out.push_str(&format!("B07 批 {}/{} 绿 跑{} 次\n", green, MEMBERS.len(), self.runs));
        for r in &self.rows {
            if r.ok {
                out.push_str(&format!("绿 {}\n", r.name));
            } else {
                out.push_str(&format!(
                    "红 {}({:04X})\n",
                    r.name,
                    r.err.unwrap_or(0)
                ));
            }
        }
        out
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 3 例 ─────────────────────────

// ── u32 返回成员的统一适配（旧批 drill 返回 Result<u32,u16>）──────
fn d125() -> Result<(), u16> { crate::unxstar::unx0125_earlypanic::drill().map(|_| ()) }
fn d126() -> Result<(), u16> { crate::unxstar::unx0126_bootlogo::drill().map(|_| ()) }
fn d127() -> Result<(), u16> { crate::unxstar::unx0127_stagebudget::drill().map(|_| ()) }
fn d128() -> Result<(), u16> { crate::unxstar::unx0128_nmiwatch::drill().map(|_| ()) }
fn d129() -> Result<(), u16> { crate::unxstar::unx0129_recbootmenu::drill().map(|_| ()) }
fn d130() -> Result<(), u16> { crate::unxstar::unx0130_lastboot::drill().map(|_| ()) }
fn d131() -> Result<(), u16> { crate::unxstar::unx0131_bootcount::drill().map(|_| ()) }
fn d132() -> Result<(), u16> { crate::unxstar::unx0132_healthflag::drill().map(|_| ()) }

/// 真实成员 drill 表（索引与 MEMBERS 严格同序）。
pub fn real_drills() -> Vec<DrillFn> {
    vec![
        d125,
        d126,
        d127,
        d128,
        d129,
        d130,
        d131,
        d132,
        crate::unxstar::unx0133_badslot::drill,
        crate::unxstar::unx0134_abfallback::drill,
        crate::unxstar::unx0135_bootselect::drill,
        crate::unxstar::unx0136_diagentry::drill,
        crate::unxstar::unx0137_argedit::drill,
        crate::unxstar::unx0138_bootstat::drill,
        crate::unxstar::unx0139_wdhandoff::drill,
    ]
}

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：真实成员整批跑——一次全绿。
    let mut b = Ktb07Book::new()?;
    let drills = real_drills();
    b.run_batch(&drills)?;
    b.all_green()?;
    b.audit()?;
    hits += 1;

    // L2：重跑账单调。
    b.run_batch(&drills)?;
    if b.runs != 2 {
        return Err(E_BAL);
    }
    b.all_green()?;
    hits += 1;

    // L3：渲染 15/15 绿。
    let rep = b.render();
    if !rep.contains("15/15 绿") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：表长不符（装配错误）拦截。
    let mut i1 = Ktb07Book::new()?;
    let short: Vec<DrillFn> = real_drills().into_iter().take(14).collect();
    if i1.run_batch(&short) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：注假红——一成员 drill 恒红，all_green 定位到成员。
    fn fake_red() -> Result<(), u16> {
        Err(0xBEEF)
    }
    let mut i2 = Ktb07Book::new()?;
    let mut fd = real_drills();
    fd[7] = fake_red as DrillFn; // unx0132 槽位注红
    i2.run_batch(&fd)?;
    if i2.all_green() != Err(E_MEMBER) {
        return Err(E_MEMBER);
    }
    if i2.rows[7].ok {
        return Err(E_BAL);
    }
    if i2.rows[7].err != Some(0xBEEF) {
        return Err(E_BAL);
    }
    // 其余 14 个仍绿。
    if i2.rows.iter().filter(|r| r.ok).count() != 14 {
        return Err(E_BAL);
    }
    i2.audit()?;
    hits += 1;

    // I3：未跑批判绿 / 冻结重跑 双拦截。
    let i3 = Ktb07Book::new()?;
    if i3.all_green() != Err(E_BAL) {
        return Err(E_BAL);
    }
    let mut i4 = Ktb07Book::new()?;
    i4.freeze();
    if i4.run_batch(&real_drills()) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
