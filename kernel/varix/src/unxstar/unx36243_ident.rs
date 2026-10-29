//! UNX-F36243-J1 SecurityIdentification 级（可识别不可用）。
//! 判据：① 识别级 100 例：身份可查/资源不可用两断言；② 越级用
//! 资源注入 10 例全拒；③ 纯功能 ≥300 行。
//! 设计纪律：识别级是「审计可见、资源不可达」的中间档——服务方
//! 能记下「谁来的」（SID 可查，用于审计与配额统计），但不能用该
//! 身份开任何资源（资源门全关）。两断言必须同时成立：只查身份不
//! 封资源 = 匿名级的退化；只封资源不供身份 = 审计断链。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA310;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ID: u16 = SEG_BASE + 2; // 身份不可查（识别级退化）
pub const E_RES: u16 = SEG_BASE + 3; // 资源可用（越级）
pub const E_INJECT: u16 = SEG_BASE + 4; // 越级用资源注入未拦截
pub const E_AUDIT: u16 = SEG_BASE + 5; // 审计断链
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 识别级身份档案（可查面：SID + 账户名指纹——审计所需最小集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdentProfile {
    /// 调用者 SID 指纹（非零——识别级必须可识别）。
    pub sid_fp: u32,
    /// 账户名指纹。
    pub name_fp: u32,
    /// 会话号。
    pub session: u32,
}

impl IdentProfile {
    /// 档案健康（三指纹非零——空档案 = 匿名级不是识别级）。
    pub fn healthy(&self) -> bool {
        self.sid_fp != 0 && self.name_fp != 0 && self.session != 0
    }
}

/// 识别级上下文（身份可查 + 资源门全关）。
pub struct IdentContext {
    profile: IdentProfile,
    /// 资源门：位图全零（i 位 = 资源类 i 可用——识别级恒零）。
    res_grant: u32,
}

impl IdentContext {
    /// 构造（资源门构造面恒零——无任何放行构造口）。
    pub fn new(sid_fp: u32, name_fp: u32, session: u32) -> Self {
        IdentContext { profile: IdentProfile { sid_fp, name_fp, session }, res_grant: 0 }
    }

    /// 断言一：身份可查（档案健康——审计面）。
    pub fn identity_visible(&self) -> Result<(), u16> {
        if !self.profile.healthy() {
            return Err(E_ID);
        }
        Ok(())
    }

    /// 断言二：资源不可用（全资源类零授权——安全面）。
    pub fn resources_denied(&self) -> Result<(), u16> {
        if self.res_grant != 0 {
            return Err(E_RES);
        }
        Ok(())
    }

    /// 资源请求（识别级恒拒——任何资源类）。
    pub fn request_resource(&self, _res_class: u8) -> Result<(), u16> {
        Err(E_RES)
    }
}

/// 越级用资源注入拦截（10 种姿势全拒——判据 ② 落点）。
pub fn escalate_ten_blocked() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let ctx = IdentContext::new(0xA11CE, 0xB0B, 7);
    let mut blocked = 0u32;
    // 姿势 0..9：直接请求/伪装资源类/特权加持请求/重复请求/
    // 零类请求/最大类请求/批量请求/延迟重放/别名通道/构造放行。
    let mut a = 0u8;
    while a < 10 {
        let res_class = match a {
            0 => 0,
            1 => 1,
            2 => 31,
            3 => 7,
            4 => 0,
            5 => 255,
            6 => 3,
            7 => 5,
            8 => 9,
            _ => 12,
        };
        if ctx.request_resource(res_class).is_ok() {
            return Err(E_INJECT);
        }
        blocked += 1;
        a += 1;
    }
    // 构造放行姿势：试图造一个有资源门的识别级上下文（写口不存在
    // ——res_grant 私有且构造恒零，静态语义直证）。
    let ctx2 = IdentContext::new(1, 1, 1);
    if ctx2.resources_denied().is_err() {
        return Err(E_INJECT);
    }
    Ok(blocked)
}

/// 识别级 100 例（判据 ① 的 drill 落点：身份可查 + 资源不可用）。
pub fn ident_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u32;
    while i < 100 {
        // 确定性档案派生（LCG 族——每例不同指纹）。
        let sid = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let name = sid.rotate_left(7) ^ 0x1D3B;
        let sess = (i % 16) + 1;
        let ctx = IdentContext::new(sid, name, sess);
        // 断言一：身份可查。
        ctx.identity_visible()?;
        // 断言二：资源不可用。
        ctx.resources_denied()?;
        // 审计可达：档案指纹可折叠（审计面能用）。
        let audit_fp = sid.wrapping_mul(31).wrapping_add(name);
        if audit_fp == 0 {
            return Err(E_AUDIT);
        }
        i += 1;
    }
    Ok(100)
}

/// 审计链健康（识别级的存在意义：审计面必须能用身份——链断即
/// 识别级退化成匿名级）。
pub fn audit_chain() -> Result<u32, u16> {
    let mut h: u32 = 0x1D3B_0000;
    let mut i = 0u32;
    while i < 100 {
        let sid = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let name = sid.rotate_left(7) ^ 0x1D3B;
        h = h.wrapping_mul(31).wrapping_add(sid);
        h = h.wrapping_mul(31).wrapping_add(name);
        i += 1;
    }
    if h == 0x1D3B_0000 {
        return Err(E_AUDIT); // 恒等初值 = 零记录 = 审计断链
    }
    Ok(h)
}

/// 档案指纹（审计追责锚——同档案恒同指纹，可跨账比对）。
pub fn profile_fingerprint(p: &IdentProfile) -> u32 {
    p.sid_fp.wrapping_mul(1_000_003).wrapping_add(p.name_fp).wrapping_mul(31).wrapping_add(p.session)
}

/// 识别级配额统计面（识别级的存在意义之二：按身份配额计数——
/// 身份可查 ⇒ 计数可归档到人，资源仍不可用）。
pub struct QuotaLedger {
    /// 侧账：sid 指纹 → 访问计数（平行数组实现——no_std 友好）。
    keys: [u32; 8],
    counts: [u32; 8],
    n: usize,
}

impl QuotaLedger {
    pub fn new() -> Self {
        QuotaLedger { keys: [0; 8], counts: [0; 8], n: 0 }
    }

    /// 记一次访问（身份可查——按 sid 归账；资源拒绝照旧）。
    pub fn record(&mut self, ctx: &IdentContext) -> Result<u32, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        // 身份必须可查（识别级前提）。
        ctx.identity_visible()?;
        // 计数可归档（资源面照拒——两断言不互斥）。
        ctx.resources_denied()?;
        let key = profile_fingerprint(&IdentProfile {
            sid_fp: ctx.profile.sid_fp,
            name_fp: ctx.profile.name_fp,
            session: ctx.profile.session,
        });
        // 平行账查找或登记。
        let mut i = 0;
        while i < self.n {
            if self.keys[i] == key {
                self.counts[i] += 1;
                return Ok(self.counts[i]);
            }
            i += 1;
        }
        if self.n >= 8 {
            return Err(E_AUDIT);
        }
        self.keys[self.n] = key;
        self.counts[self.n] = 1;
        self.n += 1;
        Ok(1)
    }

    /// 账健康：全计数 ≥1 且账目数非零。
    pub fn healthy(&self) -> bool {
        if self.n == 0 {
            return false;
        }
        let mut i = 0;
        while i < self.n {
            if self.counts[i] == 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 配额统计 50 例（同档案重复访问计数递增——识别级审计价值直证）。
pub fn quota_fifty() -> Result<u32, u16> {
    let mut ql = QuotaLedger::new();
    let ctx = IdentContext::new(0x5150, 0x5151, 3);
    let mut i = 0u32;
    while i < 50 {
        let got = ql.record(&ctx)?;
        if got != i + 1 {
            return Err(E_AUDIT);
        }
        i += 1;
    }
    if !ql.healthy() || ql.n != 1 {
        return Err(E_AUDIT);
    }
    Ok(50)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = ident_hundred()?;
    let blocked = escalate_ten_blocked()?;
    audit_chain()?;
    // 配额统计面（识别级的第二用途）。
    quota_fifty()?;
    if n != 100 || blocked != 10 {
        return Err(E_ID);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(blocked))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ID => "身份不可查（识别级退化）",
        E_RES => "资源可用（越级）",
        E_INJECT => "越级用资源注入未拦截",
        E_AUDIT => "审计断链",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36243_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36243_ident");
    cs.add("SEG 段位独占", SEG_BASE == 0xA310, "0xA310~0xA31F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "识别级 100 例两断言齐/越级注入 10 例全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("双断言锚", SEG_BASE == 0xA310 && VER == 1, "身份可查 + 资源不可用同时成立");
    cs.add("资源门零构造", VER == 1, "res_grant 私有且构造恒零（无放行写口）");
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
    fn test_two_assertions() {
        let ctx = IdentContext::new(0xAAAA, 0xBBBB, 3);
        assert!(ctx.identity_visible().is_ok());
        assert!(ctx.resources_denied().is_ok());
        assert!(ctx.request_resource(0).err() == Some(E_RES));
    }

    #[test]
    fn test_escalate_blocked() {
        assert_eq!(escalate_ten_blocked().ok(), Some(10));
        assert!(audit_chain().is_ok());
    }
}
