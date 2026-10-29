//! CGPU-F0733 · 场景内容一致性（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0733，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：场景内容一致性——清单账（声明的内容计量）与实收账
//!    （实际渲染计量）的对账：逐类差额、容差判定、对账报告与
//!    计量序列化往返不变性。
//!
//! 设计纪律（与 F0716 的差异：F0716 以"双实现位置对拍"为核心，
//! 本单以"声明账 vs 实收账对账+序列化往返"为核心）：
// - 计量四类闭集（复用内容类别语义，独立定义序）：
//   Node/Text/Texture/Script。
// - 对账三态闭集：Exact（逐类相等）/WithinTol（每类差额
//   ≤ TOL‰ 且总差额 ≤ TOL_TOTAL）/Divergent。
// - 往返不变性：计量编码（u32x4 LE 拼包）→ 解码 → 逐类相等；
//   任何一次往返失配即 E_ROUNDTRIP。
// - 对账恒等式：declared_total == actual_total + net_delta
//   （net_delta 为逐类差额代数和，显式校验）。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x49e0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x49e0，批内 CGPU-F0733 独占）。
pub const SEG_BASE: u16 = 0x49e0;

/// 序列化往返失配。
pub const E_ROUNDTRIP: u16 = SEG_BASE + 1;
/// 对账发散（超容差）。
pub const E_DIVERGE: u16 = SEG_BASE + 2;
/// 对账恒等式被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 3;
/// 计量参数非法（负语义/超上限）。
pub const E_CENSUS: u16 = SEG_BASE + 4;
/// 对账无对象（空清单）。
pub const E_EMPTY: u16 = SEG_BASE + 5;

/// 单类计量上限（件）。
pub const COUNT_MAX: u32 = 1_000_000;
/// 单类容差（千分比，按声明值）。
pub const TOL_PM: u32 = 20;
/// 总差额容差（件）。
pub const TOL_TOTAL: u64 = 32;

/// 内容计量（四类，与 F0732 的评分视角不同：这里是账面对账视角）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Census {
    /// [Node, Text, Texture, Script] 件数。
    pub counts: [u32; 4],
}

impl Census {
    /// 合法性：每类 ≤ COUNT_MAX。
    pub fn validate(&self) -> Result<(), u16> {
        for c in &self.counts {
            if *c > COUNT_MAX {
                return Err(E_CENSUS);
            }
        }
        Ok(())
    }
    /// 总件数。
    pub fn total(&self) -> u64 {
        self.counts.iter().map(|c| *c as u64).sum()
    }
    /// 编码：4×u32 LE 拼包（16 字节）。
    pub fn encode(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        for i in 0..4 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.counts[i].to_le_bytes());
        }
        out
    }
    /// 解码。
    pub fn decode(b: &[u8; 16]) -> Census {
        let mut counts = [0u32; 4];
        for i in 0..4 {
            let mut v = [0u8; 4];
            v.copy_from_slice(&b[i * 4..i * 4 + 4]);
            counts[i] = u32::from_le_bytes(v);
        }
        Census { counts }
    }
    /// 往返自检。
    pub fn roundtrip(&self) -> Result<(), u16> {
        let enc = self.encode();
        let dec = Census::decode(&enc);
        if dec != *self {
            return Err(E_ROUNDTRIP);
        }
        Ok(())
    }
    /// 逐类差额（实收 - 声明，有符号）。
    pub fn delta_vs(&self, declared: &Census) -> [i64; 4] {
        let mut d = [0i64; 4];
        for i in 0..4 {
            d[i] = self.counts[i] as i64 - declared.counts[i] as i64;
        }
        d
    }
}

/// 对账三态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recon {
    /// 逐类精确相等。
    Exact,
    /// 容差内。
    WithinTol,
    /// 发散。
    Divergent,
}

/// 对账报告行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconRow {
    /// 类序。
    pub ord: u8,
    /// 声明件数。
    pub declared: u32,
    /// 实收件数。
    pub actual: u32,
    /// 差额（实收-声明）。
    pub delta: i64,
    /// 单类是否容差内。
    pub in_tol: bool,
}

/// 对账核心：声明账 vs 实收账。
/// 恒等式（显式）：actual_total == declared_total + Σdelta。
pub fn reconcile(declared: &Census, actual: &Census) -> Result<(Recon, Vec<ReconRow>), u16> {
    declared.validate()?;
    actual.validate()?;
    // 恒等式：actual == declared + net_delta。
    let mut net: i64 = 0;
    for i in 0..4 {
        net += actual.counts[i] as i64 - declared.counts[i] as i64;
    }
    if actual.total() as i64 != declared.total() as i64 + net {
        return Err(E_LEDGER);
    }
    let d = actual.delta_vs(declared);
    let mut rows = Vec::with_capacity(4);
    let mut exact = true;
    let mut total_abs: u64 = 0;
    for i in 0..4 {
        let di = d[i];
        if di != 0 {
            exact = false;
        }
        total_abs += di.unsigned_abs();
        // 单类容差：|Δ| ≤ declared*TOL_PM/1000（声明 0 时容差为 0）。
        let tol = (declared.counts[i] as u64 * TOL_PM as u64) / 1000;
        let in_tol = di.unsigned_abs() as u64 <= tol;
        rows.push(ReconRow {
            ord: i as u8,
            declared: declared.counts[i],
            actual: actual.counts[i],
            delta: di,
            in_tol,
        });
    }
    let verdict = if exact {
        Recon::Exact
    } else if total_abs <= TOL_TOTAL && rows.iter().all(|r| r.in_tol) {
        Recon::WithinTol
    } else {
        Recon::Divergent
    };
    Ok((verdict, rows))
}

/// 对账门：Divergent 即拒绝（返回错误码）。
pub fn recon_gate(declared: &Census, actual: &Census) -> Result<Recon, u16> {
    let (v, _) = reconcile(declared, actual)?;
    if v == Recon::Divergent {
        return Err(E_DIVERGE);
    }
    Ok(v)
}

/// 批量对账：多场景（声明, 实收）对，全部非 Divergent 才过。
/// 返回（场景数, Exact 数, WithinTol 数）。
pub fn batch_recon(pairs: &[(Census, Census)]) -> Result<(usize, usize, usize), u16> {
    if pairs.is_empty() {
        return Err(E_EMPTY);
    }
    let mut exact = 0usize;
    let mut within = 0usize;
    for (d, a) in pairs {
        match recon_gate(d, a)? {
            Recon::Exact => exact += 1,
            Recon::WithinTol => within += 1,
            Recon::Divergent => return Err(E_DIVERGE),
        }
    }
    Ok((pairs.len(), exact, within))
}

/// 对账汇总（报告头部）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconSummary {
    /// 判定。
    pub verdict: Recon,
    /// 声明总件数。
    pub declared_total: u64,
    /// 实收总件数。
    pub actual_total: u64,
    /// 净差额（实收-声明）。
    pub net_delta: i64,
    /// 容差内类数。
    pub in_tol_classes: u32,
    /// 总差额（绝对值）。
    pub total_abs_delta: u64,
}

/// 汇总化对账：先 reconcile 再折叠头部字段。
/// 恒等式复核（显式）：actual_total == declared_total + net_delta。
pub fn reconcile_summary(declared: &Census, actual: &Census) -> Result<ReconSummary, u16> {
    let (v, rows) = reconcile(declared, actual)?;
    let net: i64 = rows.iter().map(|r| r.delta).sum();
    let total_abs: u64 = rows.iter().map(|r| r.delta.unsigned_abs() as u64).sum();
    let in_tol = rows.iter().filter(|r| r.in_tol).count() as u32;
    let s = ReconSummary {
        verdict: v,
        declared_total: declared.total(),
        actual_total: actual.total(),
        net_delta: net,
        in_tol_classes: in_tol,
        total_abs_delta: total_abs,
    };
    if s.actual_total as i64 != s.declared_total as i64 + s.net_delta {
        return Err(E_LEDGER);
    }
    if s.net_delta != 0 && s.total_abs_delta < s.net_delta.unsigned_abs() as u64 {
        // 三角不等式口径：各差绝对值之和 ≥ 净差绝对值。
        return Err(E_LEDGER);
    }
    Ok(s)
}

/// 自例程：主路径（精确/容差内/往返不变）+ 错误路径（发散/账不平）。
pub fn drill() -> Result<u32, u16> {
    // 主路径 0：往返不变性（含边界值）。
    let c0 = Census { counts: [0, 1, 123_456, COUNT_MAX] };
    c0.roundtrip()?;
    let c1 = Census { counts: [500, 300, 20, 5] };
    c1.roundtrip()?;
    // 主路径 1：精确对账。
    let (v, rows) = reconcile(&c1, &Census { counts: [500, 300, 20, 5] })?;
    if v != Recon::Exact {
        return Err(E_DIVERGE);
    }
    if rows.len() != 4 || rows[0].delta != 0 {
        return Err(E_LEDGER);
    }
    // 主路径 2：容差内——Node 多 8（500*20‰=10 ≥ 8），Texture 少 0；
    // 总差额 8 ≤ 32 → WithinTol。
    let actual2 = Census { counts: [508, 300, 20, 5] };
    let (v2, _) = reconcile(&c1, &actual2)?;
    if v2 != Recon::WithinTol {
        return Err(E_DIVERGE);
    }
    recon_gate(&c1, &actual2)?;
    // 汇总化对账：容差内例——净差 +8，容差内类 4（Node in_tol，
    // 其余零差 in_tol）。
    let s2 = reconcile_summary(&c1, &actual2)?;
    if s2.net_delta != 8 || s2.in_tol_classes != 4 || s2.total_abs_delta != 8 {
        return Err(E_LEDGER);
    }
    if s2.declared_total != 825 || s2.actual_total != 833 {
        return Err(E_LEDGER);
    }
    // 汇总化对账：精确例——净差 0。
    let s_exact = reconcile_summary(&c1, &Census { counts: [500, 300, 20, 5] })?;
    if s_exact.verdict != Recon::Exact || s_exact.net_delta != 0 {
        return Err(E_LEDGER);
    }
    // 汇总化对账：发散例——净差 +100，总差 100。
    let s_div = reconcile_summary(&c1, &Census { counts: [500, 300, 20, 105] })?;
    if s_div.verdict != Recon::Divergent || s_div.net_delta != 100 {
        return Err(E_LEDGER);
    }
    // 主路径 3：批对账（1 exact + 1 within）。
    let (n, e, w) = batch_recon(&[
        (c1, Census { counts: [500, 300, 20, 5] }),
        (c1, actual2),
    ])?;
    if n != 2 || e != 1 || w != 1 {
        return Err(E_LEDGER);
    }
    // 错误路径 0：发散——Script 多 100（容差 5*20‰=0），总差 100 > 32。
    let actual3 = Census { counts: [500, 300, 20, 105] };
    if recon_gate(&c1, &actual3) != Err(E_DIVERGE) {
        return Err(E_DIVERGE);
    }
    let (v3, rows3) = reconcile(&c1, &actual3)?;
    if v3 != Recon::Divergent || rows3[3].delta != 100 || rows3[3].in_tol {
        return Err(E_DIVERGE);
    }
    // 错误路径 1：单类容差外但总差小——声明 0 的类多 1 件
    // （容差 0）→ Divergent。
    let decl0 = Census { counts: [0, 0, 0, 0] };
    let act1 = Census { counts: [0, 0, 0, 1] };
    if recon_gate(&decl0, &act1) != Err(E_DIVERGE) {
        return Err(E_DIVERGE);
    }
    // 错误路径 2：计量越上限。
    let bad = Census { counts: [COUNT_MAX + 1, 0, 0, 0] };
    if bad.validate() != Err(E_CENSUS) {
        return Err(E_CENSUS);
    }
    if reconcile(&bad, &bad) != Err(E_CENSUS) {
        return Err(E_CENSUS);
    }
    // 错误路径 3：空批对账。
    if batch_recon(&[]) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 4：往返不变性负验证——手工破坏编码字节。
    let mut enc = c1.encode();
    enc[0] = enc[0].wrapping_add(1);
    let dec = Census::decode(&enc);
    if dec == c1 {
        return Err(E_ROUNDTRIP); // 破坏后应不等
    }
    // 错误路径 5：对账恒等式自检（对合法输入恒成立，负验证：
    // 构造 delta 代数和 == 总差 的平凡成立性已隐含；此处校验
    // total() 恒等）。
    if actual2.total() as i64 != c1.total() as i64 + 8 {
        return Err(E_LEDGER);
    }
    Ok(0)
}
