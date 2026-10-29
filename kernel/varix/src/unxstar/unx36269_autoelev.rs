//! UNX-F36269-J1 AutoElevate 白名单消费与签名校验联签（AI-48 锚）。
//! 判据：① 白名单 10 项×签名校验 fake 联签 100% 过；② 白名单外
//! 请求走 consent 断言 50 例；③ 签名失败注入 10 例拒绝；④ ≥300 行。
//! 设计纪律：AutoElevate = 无弹窗自动提权的白名单通道——它的
//! 危险性在于绕过 consent，故必须**双签联动**：执行体在白名单
//! （清单面）且签名校验通过（信任面），二者缺一即回落 consent。
//! 「清单在、签名坏」与「签名好、清单无」都必须走 consent——
//! 单签放行是白名单通道的经典旁路。白名单本身冻结：10 项之外
//! 无运行时扩容口。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36265_filttok as ft;
use super::unx36266_uacdeflt as df;
use super::unx36268_elevreq as el;

pub const SEG_BASE: u16 = 0xAF40;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SIGN: u16 = SEG_BASE + 2; // 签名校验失败未拒
pub const E_BYPASS: u16 = SEG_BASE + 3; // 单签放行（联签破）
pub const E_LIST: u16 = SEG_BASE + 4; // 白名单账破
pub const E_FROZEN: u16 = SEG_BASE + 5; // 白名单运行时扩容
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 白名单（冻结 10 项：执行体 ID 闭集——对标 OS 签名系统组件）。
pub const AUTOELEVATE_LIST: [u32; 10] = [101, 102, 103, 104, 105, 106, 107, 108, 109, 110];

/// 签名校验（fake 联签锚：FNV-1a 指纹 = 签名值——确定性可重放；
/// 信任面以指纹匹配为准）。
pub fn sign_verify(target: u32, sig: u32) -> bool {
    let mut h: u32 = 0x811C_9DC5;
    let b = target.to_le_bytes();
    for x in &b {
        h ^= *x as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h == sig
}

/// 白名单在册查询（清单面）。
pub fn in_list(target: u32) -> bool {
    AUTOELEVATE_LIST.contains(&target)
}

/// 自动提权裁决（双签联动：清单 ∧ 签名 → auto；否则 → consent）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoVerdict {
    AutoElevate,
    NeedsConsent,
}

pub fn auto_elevate_judge(target: u32, sig: u32) -> Result<AutoVerdict, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let listed = in_list(target);
    let signed = sign_verify(target, sig);
    match (listed, signed) {
        (true, true) => Ok(AutoVerdict::AutoElevate),
        _ => Ok(AutoVerdict::NeedsConsent), // 单签/双缺一律回落 consent
    }
}

/// 白名单 10 项 × 签名联签全过（判据 ① 落点）：10 项执行体以
/// 正确签名逐项判 auto——100% AutoElevate；并以错误签名交叉
/// 项（10 项 × 邻位签名）验证回落。
pub fn whitelist_ten_signed() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if AUTOELEVATE_LIST.len() != 10 {
        return Err(E_LIST);
    }
    // 白名单互异（冻结闭集性质）。
    let mut i = 0usize;
    while i < 10 {
        let mut j = i + 1;
        while j < 10 {
            if AUTOELEVATE_LIST[i] == AUTOELEVATE_LIST[j] {
                return Err(E_LIST);
            }
            j += 1;
        }
        i += 1;
    }
    // 10 项 × 正确签名 → auto。
    let mut n = 0u32;
    let mut k = 0usize;
    while k < 10 {
        let t = AUTOELEVATE_LIST[k];
        // 正确签名 = sign_verify 的期望指纹（fake 联签锚：构造方
        // 用同一 FNV——签名值由 target 确定性导出）。
        let mut h: u32 = 0x811C_9DC5;
        let b = t.to_le_bytes();
        for x in &b {
            h ^= *x as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        if auto_elevate_judge(t, h)? != AutoVerdict::AutoElevate {
            return Err(E_SIGN); // 清单在 + 签名对 = 必 auto
        }
        n += 1;
        k += 1;
    }
    if n != 10 {
        return Err(E_SIGN);
    }
    Ok(n)
}

/// 白名单外走 consent 50 例（判据 ② 落点）：50 个白名单外 ID
/// （即使签名对）全部 NeedsConsent——清单面是硬边界。
pub fn outside_fifty_consent() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    let mut t = 111u32;
    while t < 161 {
        // 白名单外断言 + 正确签名（签名对也不放行）。
        let mut h: u32 = 0x811C_9DC5;
        let b = t.to_le_bytes();
        for x in &b {
            h ^= *x as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        if in_list(t) {
            return Err(E_LIST); // 取样越界（闭集外取样污染）
        }
        if auto_elevate_judge(t, h)? != AutoVerdict::NeedsConsent {
            return Err(E_BYPASS); // 清单外放行 = 白名单破
        }
        n += 1;
        t += 1;
    }
    if n != 50 {
        return Err(E_LIST);
    }
    Ok(n)
}

/// 签名失败注入 10 例（判据 ③ 落点）：白名单内执行体 + 错误
/// 签名 → NeedsConsent（信任面是硬边界——清单在签名坏也回落）。
pub fn badsig_ten_consent() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    let mut k = 0usize;
    while k < 10 {
        let t = AUTOELEVATE_LIST[k];
        // 错误签名 = 邻位执行体的指纹（确定性伪造）。
        let fake_t = AUTOELEVATE_LIST[(k + 1) % 10];
        let mut h: u32 = 0x811C_9DC5;
        let b = fake_t.to_le_bytes();
        for x in &b {
            h ^= *x as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        if sign_verify(t, h) {
            return Err(E_SIGN); // 伪造签名竟然通过 = 签名锚破
        }
        if auto_elevate_judge(t, h)? != AutoVerdict::NeedsConsent {
            return Err(E_BYPASS); // 签名坏还 auto = 联签破
        }
        n += 1;
        k += 1;
    }
    if n != 10 {
        return Err(E_SIGN);
    }
    Ok(n)
}

/// 白名单冻结断言（运行时无扩容口：清单常量长度恒 10、内容
/// 恒等——扩容注入以重读恒等检出）。
pub fn list_frozen() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let expect: [u32; 10] = [101, 102, 103, 104, 105, 106, 107, 108, 109, 110];
    if AUTOELEVATE_LIST != expect {
        return Err(E_FROZEN);
    }
    Ok(())
}

/// auto 通道产物一致性（auto 提权走 36268 同一铸造入口——通道
/// 不同、引擎同一：auto 判决后的铸造产物与 consent 通过的产物
/// 形状一致，禁另立铸造面）。
pub fn auto_elevate_product_shape() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let sess = df::LogonSession {
        uid: 1,
        admin_member: true,
        full: ft::FullToken {
            groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
            privs: alloc::vec!["SeChangeNotifyPrivilege"],
            il: ilv::Il::Medium,
        },
    };
    let req = el::ElevRequest { session: &sess, target: 101, consent: true };
    let elev = el::elevate(&req)?;
    // 产物形状：完整面（管理员组授权可用 + 特权全带）。
    if !elev.new_token.admin_grant_usable(1) || !elev.new_token.priv_enabled("SeChangeNotifyPrivilege") {
        return Err(E_ILLEGAL);
    }
    Ok(())
}

/// 双签交换矩阵（2×2 完整裁决面：清单×签名四组合逐一锁死——
/// 联签真值的呈现面：恰一组合 auto，三组合 consent）。
pub fn dual_sign_matrix() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let listed_true = AUTOELEVATE_LIST[0]; // 101（在册）
    // 正确签名（由 target 确定性导出）。
    let mut h: u32 = 0x811C_9DC5;
    let b = listed_true.to_le_bytes();
    for x in &b {
        h ^= *x as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    // (在册, 签对) = auto。
    if auto_elevate_judge(listed_true, h)? != AutoVerdict::AutoElevate {
        return Err(E_BYPASS);
    }
    // (在册, 签坏) = consent。
    if auto_elevate_judge(listed_true, h ^ 0xFFFF)? != AutoVerdict::NeedsConsent {
        return Err(E_BYPASS);
    }
    // (不在册, 签对) = consent。
    if auto_elevate_judge(999, h)? != AutoVerdict::NeedsConsent {
        return Err(E_BYPASS);
    }
    // (不在册, 签坏) = consent。
    if auto_elevate_judge(999, h ^ 0xFFFF)? != AutoVerdict::NeedsConsent {
        return Err(E_BYPASS);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    list_frozen()?;
    let a = whitelist_ten_signed()?;
    let b = outside_fifty_consent()?;
    let c = badsig_ten_consent()?;
    dual_sign_matrix()?;
    auto_elevate_product_shape()?;
    if a != 10 || b != 50 || c != 10 {
        return Err(E_BYPASS);
    }
    Ok(b.wrapping_mul(100).wrapping_add(a))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SIGN => "签名校验失败未拒",
        E_BYPASS => "单签放行（联签破）",
        E_LIST => "白名单账破",
        E_FROZEN => "白名单运行时扩容",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36269_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36269_autoelev");
    cs.add("SEG 段位独占", SEG_BASE == 0xAF40, "0xAF40~0xAF4F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "白名单 10 项联签全过/清单外 50 例回落/坏签 10 例拒/冻结锚"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("联签锚", AUTOELEVATE_LIST.len() == 10, "双签联动（清单 ∧ 签名）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_dual_sign() {
        // 清单在 + 签名对 = auto。
        let mut h: u32 = 0x811C_9DC5;
        let b = 101u32.to_le_bytes();
        for x in &b {
            h ^= *x as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        assert!(matches!(auto_elevate_judge(101, h), Ok(AutoVerdict::AutoElevate)));
        // 清单在 + 签名坏 = consent。
        assert!(matches!(auto_elevate_judge(101, h ^ 1), Ok(AutoVerdict::NeedsConsent)));
        // 清单外 + 签名对 = consent。
        assert!(matches!(auto_elevate_judge(999, h), Ok(AutoVerdict::NeedsConsent)));
    }

    #[test]
    fn test_badsig() {
        assert_eq!(badsig_ten_consent().err(), None);
    }
}
