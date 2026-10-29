//! unx48219_clipdecl — UNX-F48219 Xwayland 与 F2 剪贴板桥边界声明（本域零剪贴板语义）（诊断段 0xBC40）。
//!
//! 判据原文：
//!   J1 边界声明入册+越界注入 ×10 零波及；判据锚定 §AI-61 §1 语义边界 F2/AI-27。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 语义边界三分：XIM 面、Xwayland 面声明零剪贴板语义（本域只管
//!      输入转换，不碰剪贴板）；F2 桥面声明为剪贴板唯一属主（语义在
//!      F2/AI-27 边界的另一侧）。
//!   2. 声明入册：三面各出一条声明（面号、语义位、指纹）入边界声明账；
//!      重复声明拒收（E_DECL_DUP），账本闭合（恰好 3 条、面号闭集）。
//!   3. 越界注入守卫：任何载荷向零语义面注入剪贴板事件必拒
//!      （E_CLIP_FORBIDDEN），拒收只记账不反弹——波及计数器恒 0。
//!   4. 零波及双证：注入前后 IM 侧状态指纹（预编辑量 + 状态摘要）
//!      逐位不变；守卫账恒等式 注入数 = 拒收数 + F2 转交数 闭合。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBC40;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CLIP_FORBIDDEN: u16 = SEG_BASE + 1; // 零语义面注入被拒
const E_UNDECLARED: u16 = SEG_BASE + 2; // 面未声明
const E_DECL_DUP: u16 = SEG_BASE + 3; // 重复声明
const E_RIPPLE: u16 = SEG_BASE + 4; // 波及计数非零
const E_GUARD_BAD: u16 = SEG_BASE + 5; // 守卫账恒等式破
const E_STATE_BAD: u16 = SEG_BASE + 6; // 守卫状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 面号：XIM 面 / Xwayland 面 / F2 桥面。
pub const SURF_XIM: u16 = 0;
pub const SURF_XWAYLAND: u16 = 1;
pub const SURF_F2BRIDGE: u16 = 2;

/// 语义位：0 = 零剪贴板语义（本域），1 = F2 桥属主语义。
pub const SEM_NONE: u8 = 0;
pub const SEM_F2_OWNER: u8 = 1;

/// 边界声明一条。
#[derive(Clone, Copy, Debug)]
pub struct Declaration {
    pub surface: u16,
    pub semantics: u8,
    pub fp: u64,
}

/// 声明指纹：确定性混合（面号 × 语义位）。
pub fn decl_fp(surface: u16, semantics: u8) -> u64 {
    (surface as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ ((semantics as u64) << 32).wrapping_add(0x1234_5678_9ABC_DEF0)
}

/// IM 侧状态快照：预编辑量 + 状态摘要（零波及双证的比对基线）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImSnapshot {
    pub preedit_len: u32,
    pub state_fp: u64,
}

/// 剪贴板边界守卫。
pub struct BoundaryGuard {
    decls: Vec<Declaration>,
    pub injected: u32,
    pub rejected: u32,
    pub forwarded_f2: u32,
    pub ripple: u32, // 注入对 IM 侧的波及数（纪律 3：必须恒 0）
    im_preedit: u32,
    im_state_fp: u64,
    pub f2_inbox: Vec<u64>, // F2 属主面收下的载荷（本域只转交不处置）
}

impl BoundaryGuard {
    pub fn new() -> BoundaryGuard {
        BoundaryGuard {
            decls: Vec::new(),
            injected: 0,
            rejected: 0,
            forwarded_f2: 0,
            ripple: 0,
            im_preedit: 0,
            im_state_fp: 0x1A2B,
            f2_inbox: Vec::new(),
        }
    }

    /// 声明入册：面号未声明过方可入册，重复声明拒收。
    pub fn declare(&mut self, surface: u16, semantics: u8) -> Result<(), u16> {
        if surface > SURF_F2BRIDGE {
            return Err(E_UNDECLARED);
        }
        let mut i = 0usize;
        while i < self.decls.len() {
            if self.decls[i].surface == surface {
                return Err(E_DECL_DUP);
            }
            i += 1;
        }
        self.decls.push(Declaration { surface, semantics, fp: decl_fp(surface, semantics) });
        Ok(())
    }

    pub fn semantics_of(&self, surface: u16) -> Option<u8> {
        let mut i = 0usize;
        while i < self.decls.len() {
            if self.decls[i].surface == surface {
                return Some(self.decls[i].semantics);
            }
            i += 1;
        }
        None
    }

    pub fn decl_count(&self) -> usize {
        self.decls.len()
    }

    /// IM 侧状态快照（对照基线）。
    pub fn im_snapshot(&self) -> ImSnapshot {
        ImSnapshot { preedit_len: self.im_preedit, state_fp: self.im_state_fp }
    }

    /// IM 侧正常打字（对照组：注入守卫不得影响此路径）。
    pub fn im_type(&mut self, ch: u16) {
        self.im_preedit += 1;
        self.im_state_fp = self.im_state_fp.wrapping_mul(31).wrapping_add(ch as u64);
    }

    /// 剪贴板事件注入守卫（J1 主体）：零语义面 → 拒收（零波及）；
    /// F2 属主面 → 转交 F2 收件匣；未声明面 → 拒收（E_UNDECLARED）。
    pub fn inject_clip(&mut self, surface: u16, payload: u64) -> Result<(), u16> {
        self.injected += 1;
        match self.semantics_of(surface) {
            None => {
                self.rejected += 1;
                Err(E_UNDECLARED)
            }
            Some(SEM_NONE) => {
                // 纪律 3：只记账、不反弹、零波及。
                self.rejected += 1;
                Err(E_CLIP_FORBIDDEN)
            }
            Some(_) => {
                self.forwarded_f2 += 1;
                self.f2_inbox.push(payload);
                Ok(())
            }
        }
    }

    /// 零波及审计：波及计数恒 0 + IM 侧状态指纹与基线逐位相等。
    pub fn assert_no_ripple(&self, baseline: &ImSnapshot) -> Result<(), u16> {
        if self.ripple != 0 {
            return Err(E_RIPPLE);
        }
        let now = self.im_snapshot();
        if now != *baseline {
            return Err(E_RIPPLE);
        }
        Ok(())
    }

    /// 守卫账恒等式审计：注入 = 拒收 + F2 转交；声明账闭合（3 条、面号闭集）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.injected != self.rejected + self.forwarded_f2 {
            return Err(E_GUARD_BAD);
        }
        if self.decl_count() != 3 {
            return Err(E_GUARD_BAD);
        }
        let mut i = 0usize;
        while i < self.decls.len() {
            let d = self.decls[i];
            if d.surface > SURF_F2BRIDGE || d.semantics > SEM_F2_OWNER {
                return Err(E_GUARD_BAD);
            }
            if d.fp != decl_fp(d.surface, d.semantics) {
                return Err(E_GUARD_BAD); // 声明指纹留痕自洽
            }
            i += 1;
        }
        if self.f2_inbox.len() as u32 != self.forwarded_f2 {
            return Err(E_GUARD_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：零波及闭合 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.ripple != 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（边界声明入册 + 越界注入 ×10 零波及），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 声明入册：三面各一条（XIM/Xwayland 零语义、F2 桥属主语义），
    //    声明账闭合 3 条，指纹留痕自洽。
    let mut gd = BoundaryGuard::new();
    gd.declare(SURF_XIM, SEM_NONE)?;
    gd.declare(SURF_XWAYLAND, SEM_NONE)?;
    gd.declare(SURF_F2BRIDGE, SEM_F2_OWNER)?;
    if gd.decl_count() != 3 {
        return Err(E_GUARD_BAD);
    }
    if gd.semantics_of(SURF_XIM) != Some(SEM_NONE)
        || gd.semantics_of(SURF_F2BRIDGE) != Some(SEM_F2_OWNER)
    {
        return Err(E_GUARD_BAD);
    }
    gd.audit()?;
    hits += 1;

    // R2 越界注入 ×10 零波及（J1 主体）：10 个剪贴板载荷轮流打向
    //    XIM/Xwayland 两个零语义面，全部拒收（E_CLIP_FORBIDDEN），
    //    波及计数恒 0，IM 侧状态指纹与基线逐位不变。
    let mut gd2 = BoundaryGuard::new();
    gd2.declare(SURF_XIM, SEM_NONE)?;
    gd2.declare(SURF_XWAYLAND, SEM_NONE)?;
    gd2.declare(SURF_F2BRIDGE, SEM_F2_OWNER)?;
    gd2.im_type(0x4E00); // 对照组：IM 侧正常打字先落基线
    gd2.im_type(0x4E01);
    let baseline = gd2.im_snapshot();
    let mut i = 0u32;
    while i < 10 {
        let surf = if i % 2 == 0 { SURF_XIM } else { SURF_XWAYLAND };
        if gd2.inject_clip(surf, 0xC10C_0000 + i as u64).is_ok() {
            return Err(E_CLIP_FORBIDDEN); // 零语义面注入必须被拒
        }
        gd2.assert_no_ripple(&baseline)?;
        i += 1;
    }
    if gd2.rejected != 10 || gd2.ripple != 0 {
        return Err(E_CLIP_FORBIDDEN);
    }
    hits += 1;

    // R3 F2 属主面转交：同一载荷打向 F2 桥面 → 转交收件匣、不拒收、
    //    不计波及；账恒等式闭合（注入=拒收+转交）。
    gd2.inject_clip(SURF_F2BRIDGE, 0xBEEF)?;
    if gd2.forwarded_f2 != 1 || gd2.f2_inbox.len() != 1 || gd2.f2_inbox[0] != 0xBEEF {
        return Err(E_GUARD_BAD);
    }
    gd2.audit()?;
    if gd2.injected != gd2.rejected + gd2.forwarded_f2 {
        return Err(E_GUARD_BAD);
    }
    hits += 1;

    // R4 未声明面拒收：面号 7 从未声明 → E_UNDECLARED，计入拒收、零波及。
    if gd2.inject_clip(7, 0x1).is_ok() {
        return Err(E_UNDECLARED);
    }
    if gd2.rejected != 11 {
        return Err(E_GUARD_BAD);
    }
    gd2.assert_no_ripple(&baseline)?;
    hits += 1;

    // R5 重复声明拒收：同面二次声明 → E_DECL_DUP，账本长度不动。
    if gd2.declare(SURF_XIM, SEM_NONE).is_ok() {
        return Err(E_DECL_DUP);
    }
    if gd2.decl_count() != 3 {
        return Err(E_GUARD_BAD);
    }
    if gd2.declare(9, SEM_NONE).is_ok() {
        return Err(E_UNDECLARED); // 面号越界声明同拒
    }
    hits += 1;

    // R6 零波及终判（注入路径 vs 打字路径分离）：打字路径推进状态、
    //    注入路径纹丝不动——两路径状态账分离可查。
    let mut gd3 = BoundaryGuard::new();
    gd3.declare(SURF_XIM, SEM_NONE)?;
    let base3 = gd3.im_snapshot();
    gd3.im_type(0x4E10); // 打字推进
    if gd3.im_snapshot() == base3 {
        return Err(E_GUARD_BAD); // 打字必须推进状态（对照组有效）
    }
    let base3b = gd3.im_snapshot();
    if gd3.inject_clip(SURF_XIM, 0xD00D).is_ok() {
        return Err(E_CLIP_FORBIDDEN);
    }
    gd3.assert_no_ripple(&base3b)?; // 注入必须纹丝不动
    hits += 1;

    // R7 守卫账审计终判：恒等式、声明闭集、F2 匣账三方闭合。
    gd2.audit()?;
    if gd2.ripple != 0 {
        return Err(E_RIPPLE);
    }
    hits += 1;

    // R8 写回（判据 J2）：波及未清零不写回、空摘要拒、闭合+非空摘要放行。
    let mut gd4 = BoundaryGuard::new();
    if gd4.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD); // 零注入未跑不得写回
    }
    gd4.declare(SURF_XIM, SEM_NONE)?;
    if gd4.inject_clip(SURF_XIM, 1).is_ok() {
        return Err(E_CLIP_FORBIDDEN);
    }
    if gd4.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    gd4.writeback(true, 0x28)?;
    hits += 1;

    Ok(hits)
}
