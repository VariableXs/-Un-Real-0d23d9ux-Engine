//! UNX-F36246-J1 模拟生命周期（开始/恢复/终止三态）。
//! 判据：① 生命周期 100 例三态账闭合；② 模拟中终止后回退线程
//! 令牌断言；③ 状态残留注入 10 例被自检修出；④ 纯功能 ≥300 行。
//! 设计纪律：生命周期的安全不变量是「状态机闭合」——开始/挂起
//! （恢复前态）/终止三态的每一次迁移都必须落账，终态必须等于
//! 「未模拟」。状态残留（模拟结束后仍持令牌指纹）是静默提权的
//! 温床：自检器必须能把任何残留检出并清扫。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA340;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_STATE: u16 = SEG_BASE + 2; // 非法状态迁移
pub const E_RESIDUE: u16 = SEG_BASE + 3; // 状态残留（终态≠未模拟）
pub const E_LEDGER: u16 = SEG_BASE + 4; // 三态账不闭合
pub const E_CLEAN: u16 = SEG_BASE + 5; // 自检清扫失败
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 生命周期三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum LifeState {
    /// 未模拟（线程用主令牌）。
    None = 0,
    /// 模拟中（线程挂接模拟令牌）。
    Active = 1,
    /// 已终止（刚退出模拟——待自检确认清零）。
    Terminated = 2,
}

/// 单线程生命周期账（三态 + 迁移计数）。
pub struct LifeLedger {
    pub tid: u32,
    pub state: LifeState,
    /// 挂接令牌指纹（Active 态非零，其余态恒零——残留检测面）。
    pub tok_fp: u32,
    /// 迁移计数：开始/终止各一（闭合校验用）。
    pub starts: u32,
    pub ends: u32,
}

impl LifeLedger {
    pub fn new(tid: u32) -> Self {
        LifeLedger { tid, state: LifeState::None, tok_fp: 0, starts: 0, ends: 0 }
    }

    /// 开始模拟（None 或 Terminated → Active——重开语义：终止后
    /// 可再开始；重复开始（Active 态）= E_STATE）。
    pub fn begin(&mut self, tok_fp: u32) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if self.state == LifeState::Active {
            return Err(E_STATE);
        }
        if tok_fp == 0 {
            return Err(E_STATE);
        }
        self.state = LifeState::Active;
        self.tok_fp = tok_fp;
        self.starts = self.starts.wrapping_add(1);
        Ok(())
    }

    /// 终止模拟（Active → Terminated；未模拟即终止 = E_STATE）。
    pub fn end(&mut self) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if self.state != LifeState::Active {
            return Err(E_STATE);
        }
        self.state = LifeState::Terminated;
        self.ends = self.ends.wrapping_add(1);
        // 终止即回退：令牌指纹立即清零（判据 ② 落点——终止后
        // 线程令牌回退 = tok_fp 归零）。
        self.tok_fp = 0;
        Ok(())
    }

    /// 自检（状态残留检出——判据 ③ 落点）。
    pub fn selfcheck(&self) -> Result<(), u16> {
        match self.state {
            LifeState::None => {
                if self.tok_fp != 0 {
                    return Err(E_RESIDUE);
                }
            }
            LifeState::Active => {
                if self.tok_fp == 0 {
                    return Err(E_RESIDUE);
                }
            }
            LifeState::Terminated => {
                if self.tok_fp != 0 {
                    return Err(E_RESIDUE);
                }
                // Terminated 终态：账闭合要求 starts == ends。
                if self.starts != self.ends {
                    return Err(E_LEDGER);
                }
            }
        }
        Ok(())
    }

    /// 账闭合（三态账：starts == ends 且终态清零）。
    pub fn closed(&self) -> bool {
        self.starts == self.ends && self.tok_fp == 0 && self.state == LifeState::Terminated
    }
}

/// 生命周期 100 例三态账闭合（判据 ① 落点）。
pub fn lifecycle_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u32;
    while i < 100 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut lg = LifeLedger::new(i + 1);
        // 初态自检：None 且零指纹。
        lg.selfcheck()?;
        // 开始模拟。
        let fp = (r & 0xFFFF) + 1;
        lg.begin(fp)?;
        lg.selfcheck()?;
        // 终止。
        lg.end()?;
        lg.selfcheck()?;
        // 闭合断言：终态 = 未模拟等价（指纹零 + 账平）。
        if !lg.closed() {
            return Err(E_LEDGER);
        }
        i += 1;
    }
    Ok(100)
}

/// 非法迁移注入（重复开始/未模拟终止——迁移面守卫）。
pub fn bad_transition_blocked() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut blocked = 0u32;
    let mut lg = LifeLedger::new(0xFFFF);
    // 1）未开始就终止：拒。
    if lg.end().is_ok() {
        return Err(E_STATE);
    }
    blocked += 1;
    // 2）开始后重复开始：拒。
    lg.begin(0x777)?;
    if lg.begin(0x888).is_ok() {
        return Err(E_STATE);
    }
    blocked += 1;
    // 3）终止后再终止：拒。
    lg.end().map_err(|_| E_STATE)?;
    if lg.end().is_ok() {
        return Err(E_STATE);
    }
    blocked += 1;
    Ok(blocked)
}

/// 状态残留注入 10 例被自检修出（判据 ③ 落点：构造残留场景，
/// 自检必须报 E_RESIDUE——残留越权写口不存在，注入 = 构造非法
/// 中间态直证自检灵敏度）。
pub fn residue_ten_detected() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut detected = 0u32;
    let mut a = 0u32;
    while a < 10 {
        let r = a.wrapping_mul(2_654_435_761).wrapping_add(0x5EED);
        let mut lg = LifeLedger::new(a + 1);
        // 注入姿势 a：把账推到「非法中间态」再自检。
        match a % 5 {
            0 => {
                // None 态残留指纹（外部篡改语义——直接改字段模拟）。
                lg.tok_fp = (r & 0xFFFF) + 1;
            }
            1 => {
                // Active 态指纹被清（令牌丢失语义）。
                lg.begin((r & 0xFFFF) + 1).map_err(|_| E_STATE)?;
                lg.tok_fp = 0;
            }
            2 => {
                // Terminated 态指纹残留。
                lg.begin((r & 0xFFFF) + 1).map_err(|_| E_STATE)?;
                lg.ends = lg.ends.wrapping_add(1); // 绕过 end 的清零
                lg.state = LifeState::Terminated;
                // tok_fp 仍非零 = 残留。
            }
            3 => {
                // 账不平残留（starts != ends 的 Terminated）。
                lg.begin((r & 0xFFFF) + 1).map_err(|_| E_STATE)?;
                lg.state = LifeState::Terminated;
                lg.tok_fp = 0;
                lg.ends = lg.ends.wrapping_add(2); // 人为破平
            }
            _ => {
                // 双开始残留（starts 多计）。
                lg.begin((r & 0xFFFF) + 1).map_err(|_| E_STATE)?;
                lg.end().map_err(|_| E_STATE)?;
                lg.starts = lg.starts.wrapping_add(1); // 破平
            }
        }
        // 自检必须检出（残留场景自检恒红）。
        if lg.selfcheck().is_err() {
            detected += 1;
        } else {
            return Err(E_CLEAN); // 残留未被检出 = 自检失灵
        }
        a += 1;
    }
    Ok(detected)
}

/// 生命周期重开面（终止后可再开始——状态机可复用，不残留旧账）。
pub fn lifecycle_reopen() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut lg = LifeLedger::new(0x5EED);
    let mut rounds = 0u32;
    // 三轮全生命周期：开始 → 终止 →（重开）→ 开始 → 终止。
    let mut r = 0;
    while r < 3 {
        let fp = 0x1000 + r;
        lg.begin(fp)?;
        lg.selfcheck()?;
        lg.end()?;
        lg.selfcheck()?;
        if !lg.closed() {
            return Err(E_LEDGER);
        }
        rounds += 1;
        r += 1;
    }
    // 账闭合终态：starts == ends == 3。
    if lg.starts != 3 || lg.ends != 3 {
        return Err(E_LEDGER);
    }
    Ok(rounds)
}

/// 多线程生命周期账（3 线程并行各自闭合——账与线程一一对应，
/// 串行实现下验证账无串扰）。
pub fn lifecycle_threads() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut closed_n = 0u32;
    let mut t = 0u32;
    while t < 3 {
        let mut lg = LifeLedger::new(100 + t);
        lg.begin(0x2000 + t)?;
        lg.end()?;
        if lg.closed() && lg.selfcheck().is_ok() {
            closed_n += 1;
        }
        t += 1;
    }
    if closed_n != 3 {
        return Err(E_LEDGER);
    }
    Ok(closed_n)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = lifecycle_hundred()?;
    let t = bad_transition_blocked()?;
    let d = residue_ten_detected()?;
    // 重开面 + 多线程账。
    lifecycle_reopen()?;
    lifecycle_threads()?;
    if n != 100 || t != 3 || d != 10 {
        return Err(E_LEDGER);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(d.wrapping_mul(10)).wrapping_add(t))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_STATE => "非法状态迁移",
        E_RESIDUE => "状态残留（终态≠未模拟）",
        E_LEDGER => "三态账不闭合",
        E_CLEAN => "自检清扫失败",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36246_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36246_impcycle");
    cs.add("SEG 段位独占", SEG_BASE == 0xA340, "0xA340~0xA34F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "生命周期 100 例闭合/残留注入 10 例检出"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("三态闭集", LifeState::None as u8 == 0 && LifeState::Active as u8 == 1 && LifeState::Terminated as u8 == 2, "None/Active/Terminated 判别式 0/1/2");
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
    fn test_revert_on_end() {
        // 判据 ② 直证：终止后令牌回退（指纹归零）。
        let mut lg = LifeLedger::new(1);
        lg.begin(0xABCD).unwrap_or(());
        assert_eq!(lg.tok_fp, 0xABCD);
        lg.end().unwrap_or(());
        assert_eq!(lg.tok_fp, 0);
        assert!(lg.selfcheck().is_ok());
    }

    #[test]
    fn test_residue_detected() {
        assert_eq!(residue_ten_detected().ok(), Some(10));
        assert_eq!(bad_transition_blocked().ok(), Some(3));
    }
}
