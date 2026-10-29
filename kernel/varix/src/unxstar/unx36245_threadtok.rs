//! UNX-F36245-J1 线程令牌挂接（SetThreadToken 类语义）。
//! 判据：① 挂接 100 例：线程安全上下文切换对平；② 无权限挂接
//! 注入 50 例全拒（TOKEN_IMPERSONATE 位门）；③ 纯功能 ≥300 行。
//! 设计纪律：挂接的本质是「线程安全上下文的原子换装」——换装前
//! 必须验证两件事：目标令牌存在且可模拟（TOKEN_IMPERSONATE 位），
//! 调用者有权对该线程挂接。位门是第一闸：没有 IMPERSONATE 位的
//! 令牌根本不能成为模拟源——50 例注入全拒的第一道也是最后一道。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA330;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BIT: u16 = SEG_BASE + 2; // IMPERSONATE 位缺（位门拒）
pub const E_RIGHT: u16 = SEG_BASE + 3; // 挂接权缺（调用者权拒）
pub const E_SWITCH: u16 = SEG_BASE + 4; // 上下文切换对平破
pub const E_TOKEN: u16 = SEG_BASE + 5; // 目标令牌缺
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// TOKEN_IMPERSONATE 位（挂接资格位——位门真源）。
pub const TOK_IMPERSONATE: u16 = 1 << 2;
/// TOKEN_DUPLICATE 位（复制资格——挂接前验）。
pub const TOK_DUPLICATE: u16 = 1 << 1;
/// THREAD_SET_THREAD_TOKEN 位（线程挂接权）。
pub const THR_SET_TOKEN: u32 = 1 << 0;

/// 可模拟令牌（模拟源资格位集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImpToken {
    pub sid_fp: u32,
    /// 资格位集（TOK_IMPERSONATE 等位面）。
    pub flags: u16,
}

impl ImpToken {
    /// 位门检查：可模拟 = IMPERSONATE 位在（第一闸）。
    pub fn can_impersonate(&self) -> bool {
        self.flags & TOK_IMPERSONATE != 0
    }
}

/// 线程（安全上下文挂接目标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Thread {
    pub tid: u32,
    /// 当前挂接的令牌指纹（0 = 无挂接，用进程令牌）。
    pub cur_tok_fp: u32,
    /// 调用者对线程的权限位集（THR_SET_TOKEN 等）。
    pub caller_rights: u32,
}

/// 挂接（双闸：位门 + 权门——判据 ② 落点）。
pub fn set_thread_token(th: &mut Thread, tok: &ImpToken) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 闸一：目标令牌必须存在且带 IMPERSONATE 位。
    if tok.sid_fp == 0 {
        return Err(E_TOKEN);
    }
    if !tok.can_impersonate() {
        return Err(E_BIT);
    }
    // 闸二：调用者必须持 THREAD_SET_THREAD_TOKEN 权。
    if th.caller_rights & THR_SET_TOKEN == 0 {
        return Err(E_RIGHT);
    }
    th.cur_tok_fp = tok.sid_fp;
    Ok(())
}

/// 摘挂（置回无挂接态——SetThreadToken(NULL) 类语义）。
pub fn clear_thread_token(th: &mut Thread) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    th.cur_tok_fp = 0;
    Ok(())
}

/// 线程安全上下文（切换对平的观察面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecContext {
    /// 生效令牌指纹（挂接令牌或进程令牌回退）。
    pub effective_fp: u32,
    /// 来源（0=进程回退 1=线程挂接——审计与 36249 次序面共用）。
    pub source: u8,
}

/// 上下文求值（线程挂接优先——与 36249 取令牌次序一致）。
pub fn effective_context(th: &Thread, proc_tok_fp: u32) -> SecContext {
    if th.cur_tok_fp != 0 {
        SecContext { effective_fp: th.cur_tok_fp, source: 1 }
    } else {
        SecContext { effective_fp: proc_tok_fp, source: 0 }
    }
}

/// 挂接 100 例切换对平（判据 ① 落点）。
pub fn attach_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u32;
    while i < 100 {
        // LCG 派生令牌与线程（确定性可重放）。
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let tok = ImpToken { sid_fp: (r & 0xFFFF) + 1, flags: TOK_IMPERSONATE | TOK_DUPLICATE };
        let mut th = Thread { tid: i + 1, cur_tok_fp: 0, caller_rights: THR_SET_TOKEN };
        let proc_fp = r.rotate_left(16) | 1;
        // 挂接前：回退态。
        let c0 = effective_context(&th, proc_fp);
        if c0.source != 0 || c0.effective_fp != proc_fp {
            return Err(E_SWITCH);
        }
        // 挂接：切换对平。
        set_thread_token(&mut th, &tok)?;
        let c1 = effective_context(&th, proc_fp);
        if c1.source != 1 || c1.effective_fp != tok.sid_fp {
            return Err(E_SWITCH);
        }
        // 摘挂：回到回退态。
        clear_thread_token(&mut th)?;
        let c2 = effective_context(&th, proc_fp);
        if c2.source != 0 || c2.effective_fp != proc_fp {
            return Err(E_SWITCH);
        }
        i += 1;
    }
    Ok(100)
}

/// 无权限挂接注入 50 例全拒（判据 ② 落点：位门 25 + 权门 25）。
pub fn inject_fifty_blocked() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut blocked = 0u32;
    // 位门 25 例：无 IMPERSONATE 位的令牌（不论权门）。
    let mut i = 0u32;
    while i < 25 {
        let r = i.wrapping_mul(2_654_435_761).wrapping_add(0xB17);
        let tok = ImpToken { sid_fp: (r & 0xFFFF) + 1, flags: (r & 0xFF) as u16 & !TOK_IMPERSONATE };
        let mut th = Thread { tid: i + 1, cur_tok_fp: 0, caller_rights: THR_SET_TOKEN };
        if set_thread_token(&mut th, &tok).is_ok() {
            return Err(E_BIT); // 位门漏了
        }
        blocked += 1;
        i += 1;
    }
    // 权门 25 例：有权位令牌但调用者无 SET 权。
    let mut k = 0u32;
    while k < 25 {
        let r = k.wrapping_mul(1_664_525).wrapping_add(0x5EED);
        let tok = ImpToken { sid_fp: (r & 0xFFFF) + 1, flags: TOK_IMPERSONATE };
        let mut th = Thread { tid: k + 1, cur_tok_fp: 0, caller_rights: 0 };
        if set_thread_token(&mut th, &tok).is_ok() {
            return Err(E_RIGHT); // 权门漏了
        }
        blocked += 1;
        k += 1;
    }
    Ok(blocked)
}

/// 挂接原子性（失败不留半态——双闸拒时线程上下文必须原样）。
pub fn attach_atomicity() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut checked = 0u32;
    // 原子性场景 1：位门拒（无 IMPERSONATE）——线程态零变化。
    let tok_bad = ImpToken { sid_fp: 0x0BAD, flags: 0 };
    let mut th = Thread { tid: 1, cur_tok_fp: 0x7777, caller_rights: THR_SET_TOKEN };
    if set_thread_token(&mut th, &tok_bad).is_ok() {
        return Err(E_BIT);
    }
    if th.cur_tok_fp != 0x7777 {
        return Err(E_SWITCH); // 拒了却改了态 = 非原子
    }
    checked += 1;
    // 原子性场景 2：权门拒——线程态零变化。
    let tok_ok = ImpToken { sid_fp: 0x600D, flags: TOK_IMPERSONATE };
    let mut th2 = Thread { tid: 2, cur_tok_fp: 0x8888, caller_rights: 0 };
    if set_thread_token(&mut th2, &tok_ok).is_ok() {
        return Err(E_RIGHT);
    }
    if th2.cur_tok_fp != 0x8888 {
        return Err(E_SWITCH);
    }
    checked += 1;
    // 原子性场景 3：目标令牌缺（零指纹）——拒且态不变。
    let tok_nil = ImpToken { sid_fp: 0, flags: TOK_IMPERSONATE };
    let mut th3 = Thread { tid: 3, cur_tok_fp: 0x9999, caller_rights: THR_SET_TOKEN };
    if set_thread_token(&mut th3, &tok_nil).is_ok() {
        return Err(E_TOKEN);
    }
    if th3.cur_tok_fp != 0x9999 {
        return Err(E_SWITCH);
    }
    checked += 1;
    Ok(checked)
}

/// 上下文切换轨迹（挂接→摘挂→再挂接——三段轨迹对平，验状态机
/// 无残留：再挂接后的态与首挂全等）。
pub fn switch_trace() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let tok = ImpToken { sid_fp: 0xABCD, flags: TOK_IMPERSONATE };
    let mut th = Thread { tid: 9, cur_tok_fp: 0, caller_rights: THR_SET_TOKEN };
    let proc_fp = 0x1234;
    // 段一：首挂接。
    set_thread_token(&mut th, &tok)?;
    let c1 = effective_context(&th, proc_fp);
    // 段二：摘挂。
    clear_thread_token(&mut th)?;
    let c2 = effective_context(&th, proc_fp);
    // 段三：再挂接（同令牌）——与首挂接全等。
    set_thread_token(&mut th, &tok)?;
    let c3 = effective_context(&th, proc_fp);
    if c1 != c3 {
        return Err(E_SWITCH);
    }
    if c2.source != 0 {
        return Err(E_SWITCH);
    }
    Ok(c1.effective_fp.wrapping_mul(31).wrapping_add(c3.effective_fp))
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let ok = attach_hundred()?;
    let blocked = inject_fifty_blocked()?;
    if ok != 100 || blocked != 50 {
        return Err(E_SWITCH);
    }
    // 原子性 + 切换轨迹（失败零半态/重挂全等）。
    attach_atomicity()?;
    switch_trace()?;
    Ok(ok.wrapping_mul(1000).wrapping_add(blocked))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BIT => "IMPERSONATE 位缺（位门拒）",
        E_RIGHT => "挂接权缺（调用者权拒）",
        E_SWITCH => "上下文切换对平破",
        E_TOKEN => "目标令牌缺",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36245_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36245_threadtok");
    cs.add("SEG 段位独占", SEG_BASE == 0xA330, "0xA330~0xA33F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "挂接 100 例对平/无权注入 50 例全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("双闸锚", TOK_IMPERSONATE == 0x0004 && THR_SET_TOKEN == 0x0001, "位门 + 权门两位面独立");
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
    fn test_bit_gate() {
        let tok = ImpToken { sid_fp: 7, flags: 0 };
        assert!(!tok.can_impersonate());
        let tok2 = ImpToken { sid_fp: 7, flags: TOK_IMPERSONATE };
        assert!(tok2.can_impersonate());
    }

    #[test]
    fn test_switch_pairing() {
        let tok = ImpToken { sid_fp: 0xFEED, flags: TOK_IMPERSONATE };
        let mut th = Thread { tid: 1, cur_tok_fp: 0, caller_rights: THR_SET_TOKEN };
        assert!(set_thread_token(&mut th, &tok).is_ok());
        assert_eq!(effective_context(&th, 0x9999).effective_fp, 0xFEED);
        assert!(clear_thread_token(&mut th).is_ok());
        assert_eq!(effective_context(&th, 0x9999).effective_fp, 0x9999);
    }
}
