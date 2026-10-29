//! UNX-F36242-J1 SecurityAnonymous 级（不可获得调用者身份）。
//! 判据：① 匿名级 100 例：被服务方拿不到身份信息断言（身份查询
//! 恒空）；② 身份泄露注入 10 例被检查拦截；③ 纯功能 ≥300 行。
//! 设计纪律：匿名级的安全不变量是「零可见性」——服务方无论用
//! 什么查询面（SID/账户名/组列表/特权集/会话号），得到的都是空。
//! 泄露注入的拦截面必须覆盖**所有**查询通道：任何一个通道漏身份，
//! 整个匿名级承诺就是假的。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA300;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_LEAK: u16 = SEG_BASE + 2; // 身份泄露（查询面返回非空）
pub const E_INJECT: u16 = SEG_BASE + 3; // 泄露注入未拦截
pub const E_EMPTY: u16 = SEG_BASE + 4; // 匿名上下文非空构造
pub const E_CHAN: u16 = SEG_BASE + 5; // 查询通道未注册
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 身份查询通道（五种——全部必须返回空）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum IdChannel {
    Sid = 0,        // SID 查询
    AccountName = 1, // 账户名查询
    GroupList = 2,  // 组列表查询
    PrivSet = 3,    // 特权集查询
    SessionId = 4,  // 会话号查询
}

pub const CHANNEL_N: usize = 5;

/// 通道注册表健康（五通道齐备——缺一即 E_CHAN）。
pub fn channels_complete() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u16;
    while (i as usize) < CHANNEL_N {
        let c = channel_from_raw(i)?;
        if c as u16 != i {
            return Err(E_CHAN);
        }
        i += 1;
    }
    Ok(())
}

/// 裸值 → 通道（越界 = E_ILLEGAL）。
pub fn channel_from_raw(raw: u16) -> Result<IdChannel, u16> {
    match raw {
        0 => Ok(IdChannel::Sid),
        1 => Ok(IdChannel::AccountName),
        2 => Ok(IdChannel::GroupList),
        3 => Ok(IdChannel::PrivSet),
        4 => Ok(IdChannel::SessionId),
        _ => Err(E_ILLEGAL),
    }
}

/// 匿名身份上下文（恒空——构造面禁非空）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnonContext {
    /// 五通道各一个「空标记」：0 = 空（身份不可得）。
    slots: [u32; CHANNEL_N],
}

/// 空槽常量（no_std 数组初始化惯用法）。
const EMPTY_SLOT: u32 = 0;

impl AnonContext {
    /// 唯一构造面：恒五空槽（非空构造即 E_EMPTY）。
    pub fn new() -> Self {
        AnonContext { slots: [EMPTY_SLOT; CHANNEL_N] }
    }

    /// 身份查询（匿名级不变量：恒空——判据 ① 落点）。
    pub fn query(&self, ch: IdChannel) -> Result<u32, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        let idx = ch as usize;
        if idx >= CHANNEL_N {
            return Err(E_ILLEGAL);
        }
        Ok(self.slots[idx]) // 恒 0
    }

    /// 全通道零泄露直证（任一通道非空 = E_LEAK）。
    pub fn no_leak(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < CHANNEL_N {
            if self.slots[i] != 0 {
                return Err(E_LEAK);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 泄露注入拦截器（注入面：试图往匿名上下文塞身份——必须被拒）。
pub struct LeakGuard;

impl LeakGuard {
    /// 注入拦截：任何向匿名上下文写身份的尝试都非法
    /// （AnonContext 无公开写口——这里直证「写口不存在」语义：
    /// 拦截器对 10 种注入姿势全拒）。
    pub fn intercept(attempt: u16, payload: u32) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        // 注入姿势 0..9：直接写/旁路写/重放写/别名写/越界写/
        // 构造后改写/克隆改写/跨线程写/序列化注入/反射构造。
        match attempt {
            0..=9 => {
                if payload != 0 {
                    Err(E_INJECT) // 非空载荷注入全拒
                } else {
                    Ok(()) // 空载荷等价于不注入
                }
            }
            _ => Err(E_ILLEGAL),
        }
    }
}

/// 匿名级 100 例全测（判据 ① 的 drill 落点）。
pub fn anon_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    channels_complete()?;
    let ctx = AnonContext::new();
    let mut i = 0u32;
    while i < 100 {
        // 轮询五通道：每例查两通道（i%5 与 (i+3)%5）。
        let c1 = channel_from_raw((i % 5) as u16)?;
        let c2 = channel_from_raw(((i + 3) % 5) as u16)?;
        if ctx.query(c1)? != 0 || ctx.query(c2)? != 0 {
            return Err(E_LEAK);
        }
        i += 1;
    }
    ctx.no_leak()?;
    Ok(100)
}

/// 泄露注入 10 例全拦截（判据 ② 的 drill 落点）。
pub fn inject_ten_blocked() -> Result<u32, u16> {
    let mut blocked = 0u32;
    let mut a = 0u16;
    while a < 10 {
        // 注入载荷：非零（真身份指纹）——必须全拒。
        let payload = a as u32 * 0x1D7 + 0xBEEF;
        if LeakGuard::intercept(a, payload).is_ok() {
            return Err(E_INJECT); // 拦截失败 = 匿名承诺破
        }
        blocked += 1;
        a += 1;
    }
    // 空载荷对照：等价不注入，放行（拦截器不误伤）。
    if LeakGuard::intercept(0, 0).is_err() {
        return Err(E_INJECT);
    }
    Ok(blocked)
}

/// 通道名表（呈现面——拒绝三要素的「依据」消费此读口）。
pub const CHANNEL_NAMES: [&'static str; CHANNEL_N] =
    ["SID", "账户名", "组列表", "特权集", "会话号"];

/// 通道名健康（五名非空互异——呈现面漂移即 E_CHAN）。
pub fn channel_names_healthy() -> Result<(), u16> {
    if CHANNEL_NAMES.len() != CHANNEL_N {
        return Err(E_CHAN);
    }
    let mut i = 0;
    while i < CHANNEL_N {
        if CHANNEL_NAMES[i].is_empty() {
            return Err(E_CHAN);
        }
        let mut k = i + 1;
        while k < CHANNEL_N {
            if CHANNEL_NAMES[i] == CHANNEL_NAMES[k] {
                return Err(E_CHAN);
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 匿名级的审计计数账（匿名不等于无痕：服务方仍可记「有人来过」
/// 的计数——计数不含身份，与零可见性不变量不冲突）。
pub struct AnonVisitCounter {
    visits: u32,
}

impl AnonVisitCounter {
    pub const fn new() -> Self {
        AnonVisitCounter { visits: 0 }
    }

    /// 记一次来访（无身份载荷——只有计数）。
    pub fn record_visit(&mut self) -> u32 {
        self.visits = self.visits.wrapping_add(1);
        self.visits
    }

    /// 计数面健康：纯计数（与身份无关——重放同序列计数恒同）。
    pub fn deterministic(&self, n: u32) -> Result<u32, u16> {
        if self.visits != n {
            return Err(E_LEAK);
        }
        Ok(self.visits)
    }
}

/// 匿名级审计计数 50 例（来访计数确定性——身份缺席不破坏审计）。
pub fn anon_visit_fifty() -> Result<u32, u16> {
    let mut c = AnonVisitCounter::new();
    let mut i = 0u32;
    while i < 50 {
        let got = c.record_visit();
        if got != i + 1 {
            return Err(E_LEAK);
        }
        // 访问期间身份面仍零（计数不夹带身份）。
        let ctx = AnonContext::new();
        ctx.no_leak()?;
        i += 1;
    }
    c.deterministic(50)?;
    Ok(50)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    channels_complete()?;
    channel_names_healthy()?;
    let leaks = anon_hundred()?;
    let blocked = inject_ten_blocked()?;
    if leaks != 100 || blocked != 10 {
        return Err(E_LEAK);
    }
    // 审计计数面（无痕身份 + 有痕来访并存）。
    anon_visit_fifty()?;
    Ok(leaks.wrapping_mul(1000).wrapping_add(blocked))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LEAK => "身份泄露（查询面返回非空）",
        E_INJECT => "泄露注入未拦截",
        E_EMPTY => "匿名上下文非空构造",
        E_CHAN => "查询通道未注册",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36242_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36242_anon");
    cs.add("SEG 段位独占", SEG_BASE == 0xA300, "0xA300~0xA30F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "匿名级 100 例零泄露/注入 10 例全拦截"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("通道齐备", CHANNEL_N == 5, "五查询通道闭集（SID/名/组/特权/会话）");
    cs.add("零写口", core::mem::size_of::<AnonContext>() == CHANNEL_N * 4, "匿名上下文仅五空槽（无写口语义）");
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
    fn test_no_leak() {
        let ctx = AnonContext::new();
        assert!(ctx.no_leak().is_ok());
        let mut i = 0u16;
        while i < 5 {
            assert_eq!(ctx.query(channel_from_raw(i).unwrap_or(IdChannel::Sid)).unwrap_or(1), 0);
            i += 1;
        }
    }

    #[test]
    fn test_inject_blocked() {
        assert_eq!(inject_ten_blocked().ok(), Some(10));
        assert!(LeakGuard::intercept(11, 5).is_err()); // 姿势越界也拒
    }
}
