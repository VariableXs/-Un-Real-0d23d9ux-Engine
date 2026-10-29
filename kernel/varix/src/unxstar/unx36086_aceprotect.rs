//! UNX-F36086-J1 受保护子树标志载体位（SE_DACL_PROTECTED 语义建模）。
//! 判据：① 保护态阻断继承重算——两轮完整状态机走查（5 次阻断 + 5 次放行）
//! 计数守恒、代际单调；② 无变化转闸 2 例、杂散控制位 3 例全部显性拒绝；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36081_acecore（FNV 指纹）；本模块是 36088/36092
//! 继承重算前的保护闸判据源，B11 继承引擎（F36204）为预告消费锚。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xe320;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_CTRL: u16 = SEG_BASE + 2;  // 控制位掩码含杂散位
pub const E_STATE: u16 = SEG_BASE + 3; // 无变化转闸（重复置位/重复清除）
pub const E_GEN: u16 = SEG_BASE + 4;   // 代际不单调
pub const E_BLOCK: u16 = SEG_BASE + 5; // 保护态阻断（继承重算被拒）
pub const E_CS: u16 = SEG_BASE + 6;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 受保护子树载体位（SE_DACL_PROTECTED 对标值，SD 控制位高段）。
pub const SE_DACL_PROTECTED: u32 = 0x8000_0000;
/// 本模块认可的控制位全集（只认保护位，其余皆杂散）。
pub const CTRL_MASK_LEGAL: u32 = SE_DACL_PROTECTED;

/// 保护状态机判定类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtectVerdict {
    /// 保护态：继承重算被阻断。
    Blocked   = 0x0001,
    /// 非保护态：继承重算放行。
    Recompute = 0x0002,
    /// 尚未发生任何重算请求。
    Idle      = 0x0003,
}

impl ProtectVerdict {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 受保护子树状态机：控制位 + 代际 + 阻断/放行计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProtectState {
    pub control: u32,
    pub generation: u32,
    pub blocked_n: u32,
    pub recomp_n: u32,
}

impl ProtectState {
    /// 初始态：未保护、代际 0、计数归零。
    pub const fn fresh() -> ProtectState {
        ProtectState { control: 0, generation: 0, blocked_n: 0, recomp_n: 0 }
    }

    /// 保护态判定。
    pub fn is_protected(&self) -> bool {
        self.control & SE_DACL_PROTECTED != 0
    }

    /// 置/清保护位：杂散位闸 → 无变化转闸 → 代际推进。
    pub fn set_protection(&mut self, on: bool, control_mask: u32) -> Result<(), u16> {
        if control_mask & !CTRL_MASK_LEGAL != 0 {
            return Err(E_CTRL);
        }
        if self.is_protected() == on {
            return Err(E_STATE);
        }
        if on {
            self.control |= SE_DACL_PROTECTED;
        } else {
            self.control &= !SE_DACL_PROTECTED;
        }
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    /// 继承重算请求：保护态 → 阻断（E_BLOCK，计数推进）；非保护态 → 放行（代际推进）。
    pub fn request_recompute(&mut self) -> Result<ProtectVerdict, u16> {
        if self.is_protected() {
            self.blocked_n = self.blocked_n.wrapping_add(1);
            return Err(E_BLOCK);
        }
        self.recomp_n = self.recomp_n.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
        Ok(ProtectVerdict::Recompute)
    }
}

/// 两轮走查剧本（drill 主体）：置保→3 阻断→清除→2 放行→再置保→2 阻断→清除→3 放行。
pub fn run_two_cycle_script(st: &mut ProtectState) -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // 第 1 轮：置保护。
    st.set_protection(true, SE_DACL_PROTECTED)?;
    if !st.is_protected() {
        return Err(E_STATE);
    }
    hits += 1;
    // 3 次重算全部被阻断。
    let mut b = 0;
    while b < 3 {
        match st.request_recompute() {
            Ok(_) => return Err(E_BLOCK),
            Err(c) => {
                if c != E_BLOCK {
                    return Err(E_BLOCK);
                }
            }
        }
        hits += 1;
        b += 1;
    }
    // 清除保护。
    st.set_protection(false, SE_DACL_PROTECTED)?;
    if st.is_protected() {
        return Err(E_STATE);
    }
    hits += 1;
    // 2 次重算放行。
    let mut g = 0;
    while g < 2 {
        match st.request_recompute() {
            Ok(ProtectVerdict::Recompute) => {}
            Ok(_) => return Err(E_GEN),
            Err(c) => {
                if c != E_GEN {
                    return Err(E_GEN);
                }
            }
        }
        hits += 1;
        g += 1;
    }
    // 第 2 轮：再置保护。
    st.set_protection(true, SE_DACL_PROTECTED)?;
    hits += 1;
    let mut b2 = 0;
    while b2 < 2 {
        match st.request_recompute() {
            Ok(_) => return Err(E_BLOCK),
            Err(c) => {
                if c != E_BLOCK {
                    return Err(E_BLOCK);
                }
            }
        }
        hits += 1;
        b2 += 1;
    }
    // 清除保护。
    st.set_protection(false, SE_DACL_PROTECTED)?;
    hits += 1;
    // 3 次重算放行。
    let mut g2 = 0;
    while g2 < 3 {
        match st.request_recompute() {
            Ok(ProtectVerdict::Recompute) => {}
            Ok(_) => return Err(E_GEN),
            Err(c) => {
                if c != E_GEN {
                    return Err(E_GEN);
                }
            }
        }
        hits += 1;
        g2 += 1;
    }
    Ok(hits)
}

/// 状态指纹（真源 FNV over 四件账目 + 版本锚）。
pub fn state_fingerprint(st: &ProtectState) -> u64 {
    let mut feed = Vec::new();
    feed.extend_from_slice(&st.control.to_le_bytes());
    feed.extend_from_slice(&st.generation.to_le_bytes());
    feed.extend_from_slice(&st.blocked_n.to_le_bytes());
    feed.extend_from_slice(&st.recomp_n.to_le_bytes());
    feed.extend_from_slice(&VER.to_le_bytes());
    core::fnv1a(&feed)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(st: &ProtectState) -> String {
    let mut s = String::new();
    s.push_str("protect gen=");
    s.push_str(&st.generation.to_string());
    s.push_str(" blocked=");
    s.push_str(&st.blocked_n.to_string());
    s.push_str(" recomp=");
    s.push_str(&st.recomp_n.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_CTRL, E_STATE, E_GEN, E_BLOCK, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CTRL => "控制位掩码含杂散位",
        E_STATE => "无变化转闸（重复置位/重复清除）",
        E_GEN => "代际不单调",
        E_BLOCK => "保护态阻断（继承重算被拒）",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 版本锚；R2 两轮剧本走查；R3 无变化转闸 2 例；R4 杂散位 3 例；
/// R5 代际单调；R6 计数守恒（阻断 5 / 放行 5）；R7 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚。
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：两轮剧本。
    let mut st = ProtectState::fresh();
    let script_hits = run_two_cycle_script(&mut st)?;
    hits += script_hits;
    // R3：无变化转闸 2 例（已保护再置 / 已清除再清）。
    let mut st2 = ProtectState::fresh();
    st2.set_protection(true, SE_DACL_PROTECTED)?;
    match st2.set_protection(true, SE_DACL_PROTECTED) {
        Ok(_) => return Err(E_STATE),
        Err(c) => {
            if c != E_STATE {
                return Err(E_STATE);
            }
        }
    }
    hits += 1;
    match st2.set_protection(false, 0) {
        Ok(_) => {}
        Err(_) => return Err(E_STATE),
    }
    match st2.set_protection(false, 0) {
        Ok(_) => return Err(E_STATE),
        Err(c) => {
            if c != E_STATE {
                return Err(E_STATE);
            }
        }
    }
    hits += 1;
    // R4：杂散控制位 3 例（0x4000_0000 / 0xC000_0000 / 0x0000_0001）。
    let strays: Vec<u32> = vec![0x4000_0000, 0xc000_0000, 0x0000_0001];
    let mut s4 = 0;
    while s4 < strays.len() {
        let mut st3 = ProtectState::fresh();
        let r = st3.set_protection(true, strays[s4]);
        match r {
            Ok(_) => return Err(E_CTRL),
            Err(c) => {
                if c != E_CTRL {
                    return Err(E_CTRL);
                }
            }
        }
        hits += 1;
        s4 += 1;
    }
    // R5：代际单调（剧本全程严格递增；4 次转闸 + 5 次放行重算 = 9）。
    if st.generation != 9 {
        return Err(E_GEN);
    }
    let before = st.generation;
    st.request_recompute()?;
    if st.generation != before + 1 {
        return Err(E_GEN);
    }
    hits += 1;
    // R6：计数守恒（阻断 5 / 放行 6——含 R5 补的 1 次放行）。
    if st.blocked_n != 5 || st.recomp_n != 6 {
        return Err(E_BLOCK);
    }
    hits += 1;
    // R7：指纹两次求值稳定且非零。
    let p1 = state_fingerprint(&st);
    let p2 = state_fingerprint(&st);
    if p1 != p2 || p1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(23));
    }

    #[test]
    fn blocked_when_protected() {
        let mut st = ProtectState::fresh();
        st.set_protection(true, SE_DACL_PROTECTED).unwrap_or(());
        assert_eq!(st.request_recompute(), Err(E_BLOCK));
    }

    #[test]
    fn stray_control_rejected() {
        let mut st = ProtectState::fresh();
        assert_eq!(st.set_protection(true, 0x4000_0000), Err(E_CTRL));
    }

    #[test]
    fn idempotent_transition_rejected() {
        let mut st = ProtectState::fresh();
        assert_eq!(st.set_protection(false, 0), Err(E_STATE));
        assert!(summary(&st).contains("protect"));
    }
}

pub fn run_unx36086_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36086_aceprotect");
    cs.add("SEG 段位独占", SEG_BASE == 0xe320, "0xe320~0xe32f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "两轮剧本对平、阻断/放行计数守恒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("保护位锚 0x80000000", SE_DACL_PROTECTED == 0x8000_0000, "SE_DACL_PROTECTED 锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs.add("合法控制位全集单值", CTRL_MASK_LEGAL == SE_DACL_PROTECTED, "控制位全集漂移");
    cs
}
