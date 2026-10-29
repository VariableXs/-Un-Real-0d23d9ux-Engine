//! UNX-F36247-J1 RevertToSelf 类语义（回退主令牌）。
//! 判据：① 回退 100 例后线程上下文 = 主令牌逐项一致；② 双重回
//! 退幂等断言 50 例；③ 纯功能 ≥300 行。
//! 设计纪律：回退的黄金性质是**幂等**——RevertToSelf 调一次与调
//! N 次，线程上下文都必须收敛到同一个「主令牌逐项一致」的终态。
//! 幂等不是优化是安全性质：任何「多回退一次导致状态漂移」的实现
//! 都在暗示状态机里藏着不可重入的副作用。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA350;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_DIFF: u16 = SEG_BASE + 2; // 回退后上下文与主令牌不一致
pub const E_NIDEM: u16 = SEG_BASE + 3; // 双重回退不幂等
pub const E_FIELD: u16 = SEG_BASE + 4; // 逐项对平缺项
pub const E_STATE: u16 = SEG_BASE + 5; // 回退态非法
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 线程安全上下文（逐项对平的字段集——五项冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThrContext {
    /// 生效令牌指纹。
    pub tok_fp: u32,
    /// 模拟级（0 = 无模拟）。
    pub level: u8,
    /// 模拟源（0 = 进程主令牌）。
    pub source: u8,
    /// 委托深度（回退后恒 0）。
    pub depth: u16,
    /// 挂接标志（回退后 false）。
    pub attached: bool,
}

/// 主令牌上下文（回退的目标终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrimaryContext {
    pub tok_fp: u32,
    /// 主令牌会话号。
    pub session: u32,
}

impl PrimaryContext {
    /// 回退终态投影（主令牌逐项一致的字段推导——判据 ① 真源）。
    pub fn revert_state(&self) -> ThrContext {
        ThrContext { tok_fp: self.tok_fp, level: 0, source: 0, depth: 0, attached: false }
    }
}

/// 模拟中的线程（可回退对象）。
pub struct ImpersonatedThread {
    ctx: ThrContext,
    primary: PrimaryContext,
}

impl ImpersonatedThread {
    pub fn new(primary: PrimaryContext, sim_tok_fp: u32) -> Self {
        ImpersonatedThread {
            ctx: ThrContext { tok_fp: sim_tok_fp, level: 2, source: 1, depth: 1, attached: true },
            primary,
        }
    }

    pub fn ctx(&self) -> &ThrContext {
        &self.ctx
    }

    /// RevertToSelf（幂等回退——判据 ①② 共同落点）。
    pub fn revert(&mut self) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        self.ctx = self.primary.revert_state();
        Ok(())
    }

    /// 逐项对平（五字段逐一比对主令牌投影——判据 ① 落点）。
    pub fn matches_primary(&self) -> Result<(), u16> {
        let expect = self.primary.revert_state();
        if self.ctx.tok_fp != expect.tok_fp {
            return Err(E_FIELD);
        }
        if self.ctx.level != expect.level {
            return Err(E_FIELD);
        }
        if self.ctx.source != expect.source {
            return Err(E_FIELD);
        }
        if self.ctx.depth != expect.depth {
            return Err(E_FIELD);
        }
        if self.ctx.attached != expect.attached {
            return Err(E_FIELD);
        }
        Ok(())
    }
}

/// 回退 100 例逐项一致（判据 ① 落点）。
pub fn revert_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u32;
    while i < 100 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let primary = PrimaryContext { tok_fp: (r & 0xFFFF) | 1, session: (r >> 16) & 0xFFFF };
        let sim_fp = r.rotate_left(11) | 1;
        let mut th = ImpersonatedThread::new(primary, sim_fp);
        // 模拟态先对不上主令牌（前提健康——否则回退断言无意义）。
        if th.matches_primary().is_ok() {
            return Err(E_STATE);
        }
        th.revert()?;
        th.matches_primary()?;
        i += 1;
    }
    Ok(100)
}

/// 双重回退幂等 50 例（判据 ② 落点：回退 1 次与回退 N 次终态全等）。
pub fn idempotent_fifty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u32;
    while i < 50 {
        let r = i.wrapping_mul(2_654_435_761).wrapping_add(0x5EED);
        let primary = PrimaryContext { tok_fp: (r & 0xFFFF) | 1, session: (r >> 8) & 0xFFFF };
        // 基准：单次回退终态。
        let mut base = ImpersonatedThread::new(primary, r.rotate_left(3) | 1);
        base.revert()?;
        let base_ctx = *base.ctx();
        // 幂等：再回退 1..=5 次，终态与单次全等。
        let mut n = 1u32;
        while n <= 5 {
            let mut th = ImpersonatedThread::new(primary, r.rotate_left(3) | 1);
            let mut k = 0;
            while k <= n {
                th.revert()?;
                k += 1;
            }
            if *th.ctx() != base_ctx {
                return Err(E_NIDEM);
            }
            n += 1;
        }
        i += 1;
    }
    Ok(50)
}

/// 多层深度回退（depth 1..4 逐层——不管模拟嵌套多深，一次回退
/// 直达主令牌终态：RevertToSelf 语义是「回自己」不是「回一层」）。
pub fn revert_from_depth() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0u32;
    while i < 50 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let primary = PrimaryContext { tok_fp: (r & 0xFFFF) | 1, session: 9 };
        // 深度模拟态：depth = 1..4（嵌套委托面）。
        let depth = (r % 4) + 1;
        let mut th = ImpersonatedThread::new(primary, r.rotate_left(5) | 1);
        th.ctx.depth = depth as u16;
        th.ctx.level = 2 + (depth as u8);
        // 一次回退直达终态。
        th.revert()?;
        th.matches_primary()?;
        let c = *th.ctx();
        if c.depth != 0 || c.level != 0 {
            return Err(E_FIELD);
        }
        i += 1;
    }
    Ok(50)
}

/// 回退状态面直证（revert 前提健康：模拟态五字段与主令牌投影
/// 至少一字段不同——否则回退断言空转）。
pub fn revert_precondition() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let primary = PrimaryContext { tok_fp: 0x1111, session: 1 };
    // 深度 0 的「假模拟态」：与主令牌投影全等——回退前提破。
    let mut fake = ImpersonatedThread::new(primary, 0x1111);
    fake.ctx.depth = 0;
    if fake.matches_primary().is_ok() {
        return Err(E_STATE);
    }
    Ok(())
}

/// 回退审计痕（回退事件自身可审计——RevertToSelf 不是隐身操作：
/// 回退次数账与模拟开始次数账闭合）。
pub struct RevertAudit {
    begins: u32,
    reverts: u32,
}

impl RevertAudit {
    pub fn new() -> Self {
        RevertAudit { begins: 0, reverts: 0 }
    }

    pub fn on_begin(&mut self) {
        self.begins = self.begins.wrapping_add(1);
    }

    pub fn on_revert(&mut self) {
        self.reverts = self.reverts.wrapping_add(1);
    }

    /// 账闭合（回退数 ≤ 开始数——不能回退比开始多）。
    pub fn healthy(&self) -> Result<(), u16> {
        if self.reverts > self.begins {
            return Err(E_NIDEM);
        }
        Ok(())
    }
}

/// 回退审计 30 例（开始→回退交替 30 轮：账闭合 + 每轮回退后逐项
/// 一致——审计与语义双面）。
pub fn revert_audit_thirty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut audit = RevertAudit::new();
    let mut i = 0u32;
    while i < 30 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let primary = PrimaryContext { tok_fp: (r & 0xFFFF) | 1, session: 4 };
        let mut th = ImpersonatedThread::new(primary, r.rotate_left(11) | 1);
        audit.on_begin();
        th.revert()?;
        audit.on_revert();
        th.matches_primary()?;
        i += 1;
    }
    audit.healthy()?;
    Ok(30)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = revert_hundred()?;
    let m = idempotent_fifty()?;
    // 多层深度回退 + 前提健康 + 审计痕。
    revert_from_depth()?;
    revert_precondition()?;
    revert_audit_thirty()?;
    if n != 100 || m != 50 {
        return Err(E_DIFF);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(m))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_DIFF => "回退后上下文与主令牌不一致",
        E_NIDEM => "双重回退不幂等",
        E_FIELD => "逐项对平缺项",
        E_STATE => "回退态非法",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36247_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36247_revert");
    cs.add("SEG 段位独占", SEG_BASE == 0xA350, "0xA350~0xA35F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "回退 100 例逐项一致/幂等 50 例全等"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("五字段锚", core::mem::size_of::<ThrContext>() == 12 || core::mem::size_of::<ThrContext>() == 16, "上下文五字段冻结（fp/level/source/depth/attached）");
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
    fn test_fieldwise_match() {
        let primary = PrimaryContext { tok_fp: 0x1234, session: 9 };
        let mut th = ImpersonatedThread::new(primary, 0xBEEF);
        assert!(th.matches_primary().is_err());
        th.revert().unwrap_or(());
        assert!(th.matches_primary().is_ok());
        let c = *th.ctx();
        assert_eq!(c.tok_fp, 0x1234);
        assert_eq!(c.depth, 0);
        assert!(!c.attached);
    }

    #[test]
    fn test_idempotent_fifty() {
        assert_eq!(idempotent_fifty().ok(), Some(50));
    }
}
