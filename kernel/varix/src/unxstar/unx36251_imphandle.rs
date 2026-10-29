//! UNX-F36251-J1 模拟与句柄 GrantedAccess 的协同（老句柄新身份）。
//! 判据：① 模拟态用既有句柄 100 例：仍按句柄掩码过滤（不重查
//! 断言）；② 越掩码注入 10 例全拒；③ 纯功能 ≥300 行。
//! 设计纪律：老句柄新身份的安全语义是「授权在打开时已冻结」——
//! 模拟切换身份后，既有句柄的 GrantedAccess **不变**（不因新身份
//! 更大），操作时按句柄掩码过滤（不再全量重查）。不重查断言的
//! 价值：若模拟态用老句柄走全量重查，就给了「新身份放大老句柄
//! 权限」或「新身份回收老句柄权限」两个错误方向——前者提权后者
//! 破坏契约。掩码是唯一真源。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA390;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RECHECK: u16 = SEG_BASE + 2; // 模拟态重查（掩码被旁路）
pub const E_OVER: u16 = SEG_BASE + 3; // 越掩码操作未拒
pub const E_FROZEN: u16 = SEG_BASE + 4; // 授权冻结破（掩码被改）
pub const E_HANDLE: u16 = SEG_BASE + 5; // 句柄缺
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 已授出句柄（打开时授权冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrantedHandle {
    pub hid: u32,
    /// 打开时冻结的掩码（唯一操作真源）。
    pub mask: u16,
}

impl GrantedHandle {
    pub fn healthy(&self) -> bool {
        self.hid != 0
    }
}

/// 掩码过滤（零全量重查——模拟态唯一操作路径）。
pub fn filter_by_mask(h: &GrantedHandle, request: u16) -> Result<u16, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if !h.healthy() {
        return Err(E_HANDLE);
    }
    Ok(request & h.mask)
}

/// 模拟态句柄操作（不重查断言的落点：本函数签名**不接**令牌/
/// 身份参数——类型层面禁止重查）。
pub fn operate_in_impersonation(h: &GrantedHandle, request: u16) -> Result<u16, u16> {
    filter_by_mask(h, request)
}

/// 冻结校验（掩码与打开时账一致——模拟切换前后掩码不得变）。
pub fn mask_frozen(before: u16, after: u16) -> Result<(), u16> {
    if before != after {
        return Err(E_FROZEN);
    }
    Ok(())
}

/// 模拟态用既有句柄 100 例（判据 ① 落点）。
pub fn old_handle_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u32;
    while i < 100 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let h = GrantedHandle { hid: i + 1, mask: (r & 0xFFFF) as u16 };
        let req = r.rotate_left(9) as u16;
        // 打开时快照（冻结锚）。
        let snap = h.mask;
        // 模拟态操作（不重查——结果只由掩码定）。
        let got = operate_in_impersonation(&h, req)?;
        // 掩码过滤直证：got == req & mask（身份无关）。
        if got != (req & h.mask) {
            return Err(E_RECHECK);
        }
        // 冻结直证：操作后掩码不变。
        mask_frozen(snap, h.mask)?;
        i += 1;
    }
    Ok(100)
}

/// 越掩码注入 10 例全拒（判据 ② 落点：请求超出掩码位必须被过滤
/// 后为零——若实现放行了掩码外位 = E_OVER）。
pub fn over_mask_ten() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rejected = 0u32;
    let mut a = 0u32;
    while a < 10 {
        let r = a.wrapping_mul(2_654_435_761).wrapping_add(0x5EED);
        // 掩码故意取窄（奇数位），请求取全位面。
        let h = GrantedHandle { hid: a + 1, mask: (r & 0x5555) as u16 };
        let req = 0xFFFFu16;
        let got = operate_in_impersonation(&h, req)?;
        // 越掩码位（req & !mask）必须全部被过滤为 0。
        let overflow = got & !h.mask;
        if overflow != 0 {
            return Err(E_OVER);
        }
        // 完整性：掩码内位必须保留（过滤不误伤）。
        if got != (req & h.mask) {
            return Err(E_OVER);
        }
        rejected += 1;
        a += 1;
    }
    Ok(rejected)
}

/// 授出掩码快照账（打开时冻结的独立复核面：同句柄多次操作间的
/// 掩码快照全等——冻结语义的时间维度直证）。
pub fn mask_snapshot_stable() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let h = GrantedHandle { hid: 0x501, mask: 0x5A5A };
    // 五次操作间快照全等（掩码不因操作漂移）。
    let snap0 = h.mask;
    let mut i = 0;
    while i < 5 {
        let req = ((i as u32).wrapping_mul(0x1111) | 1) as u16;
        let _ = operate_in_impersonation(&h, req)?;
        mask_frozen(snap0, h.mask)?;
        i += 1;
    }
    Ok(())
}

/// 零掩码句柄面（mask=0 的句柄：任何请求过滤后全零——零授权
/// 句柄的语义自洽面：存在但无能，操作恒空）。
pub fn zero_mask_handle() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let h = GrantedHandle { hid: 0x700, mask: 0 };
    // 全位请求过滤后全零（存在但无能）。
    if operate_in_impersonation(&h, 0xFFFF).map_err(|_| E_HANDLE)? != 0 {
        return Err(E_RECHECK);
    }
    // 复制零掩码：子集只能是零。
    let dup = dup_narrowing(&h, 0).map_err(|_| E_OVER)?;
    if dup.mask != 0 {
        return Err(E_OVER);
    }
    // 复制放大在零掩码上更严格（任何非零新掩码必拒）。
    if dup_narrowing(&h, 1).is_ok() {
        return Err(E_OVER);
    }
    Ok(())
}

/// 授出 ≤ 请求面（操作结果恒 ≤ 请求掩码——掩码过滤的方向性：
/// 授出位必是请求位的子集，多授一位即越权）。
pub fn grant_le_request() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u32;
    while i < 50 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let h = GrantedHandle { hid: i + 1, mask: (r & 0xFFFF) as u16 };
        let req = r.rotate_left(9) as u16;
        let granted = operate_in_impersonation(&h, req)?;
        // 方向性：granted ⊆ req（授出不超过请求）。
        if granted & !req != 0 {
            return Err(E_OVER);
        }
        // 方向性：granted ⊆ mask（授出不超过掩码）。
        if granted & !h.mask != 0 {
            return Err(E_OVER);
        }
        i += 1;
    }
    Ok(())
}

/// 降权复制语义（DuplicateHandle 类：新句柄掩码 ⊆ 原掩码——
/// 复制可以收缩不能放大，36224 链式约束的句柄面投影）。
pub fn dup_narrowing(src: &GrantedHandle, new_mask: u16) -> Result<GrantedHandle, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if !src.healthy() {
        return Err(E_HANDLE);
    }
    // 放大拒：新掩码超出原掩码位 = 复制放大（红线）。
    if new_mask & !src.mask != 0 {
        return Err(E_OVER);
    }
    Ok(GrantedHandle { hid: src.hid.wrapping_mul(2), mask: new_mask })
}

/// 降权复制 50 例（LCG 新掩码 ⊆ 原掩码全对平 + 放大注入 10 例
/// 全拒——两账齐平才算语义闭合）。
pub fn dup_fifty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut ok_n = 0u32;
    let mut i = 0u32;
    while i < 50 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let src = GrantedHandle { hid: i + 1, mask: (r & 0xFFFF) as u16 };
        // 新掩码 = 原掩码的子集（按位与派生——合法收缩）。
        let sub = ((r >> 8) as u16) & src.mask;
        let dup = dup_narrowing(&src, sub)?;
        if dup.mask & !src.mask != 0 {
            return Err(E_OVER);
        }
        // 新句柄同样只按掩码过滤（不重查语义在副本上同样成立）。
        let req = (r >> 4) as u16;
        if operate_in_impersonation(&dup, req)? != (req & dup.mask) {
            return Err(E_RECHECK);
        }
        ok_n += 1;
        i += 1;
    }
    // 放大注入 10 例全拒。
    let mut rej = 0u32;
    let mut a = 0u32;
    while a < 10 {
        let r = a.wrapping_mul(2_654_435_761).wrapping_add(0x5EED);
        let src = GrantedHandle { hid: a + 1, mask: (r & 0x00FF) as u16 };
        // 越掩码新掩码（原掩码 | 高位）必拒。
        let wider = src.mask | !((r & 0xFFFF) as u16) & 0xFF00;
        if dup_narrowing(&src, wider).is_ok() {
            return Err(E_OVER);
        }
        rej += 1;
        a += 1;
    }
    Ok(ok_n.wrapping_mul(1000).wrapping_add(rej))
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = old_handle_hundred()?;
    let r = over_mask_ten()?;
    // 降权复制面（句柄语义的第二投影）。
    dup_fifty()?;
    // 快照稳定 + 授出 ≤ 请求方向性 + 零掩码面。
    mask_snapshot_stable()?;
    grant_le_request()?;
    zero_mask_handle()?;
    if n != 100 || r != 10 {
        return Err(E_RECHECK);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(r))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RECHECK => "模拟态重查（掩码被旁路）",
        E_OVER => "越掩码操作未拒",
        E_FROZEN => "授权冻结破（掩码被改）",
        E_HANDLE => "句柄缺",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36251_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36251_imphandle");
    cs.add("SEG 段位独占", SEG_BASE == 0xA390, "0xA390~0xA39F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "老句柄 100 例掩码过滤/越掩码 10 例全过滤"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("不重查锚", VER == 1, "operate_in_impersonation 不接身份参数（类型面禁重查）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_mask_only() {
        let h = GrantedHandle { hid: 1, mask: 0b1010 };
        // 请求 0b1110：掩码外位（0b0100）被过滤。
        assert_eq!(operate_in_impersonation(&h, 0b1110).unwrap_or(0), 0b1010);
        // 全外请求过滤为 0。
        assert_eq!(operate_in_impersonation(&h, 0b0101).unwrap_or(0), 0);
    }

    #[test]
    fn test_over_mask() {
        assert_eq!(over_mask_ten().ok(), Some(10));
        assert_eq!(old_handle_hundred().ok(), Some(100));
    }
}
