//! unx48317_cfge3 — UNX-F48317 B16 批错误呈现三要素抽查（配置错误用户可见可操作）（诊断段 0xC880）。
//!
//! 判据原文：
//!   J1 注入 ×10 三要素全齐；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 配置错误注入面：十类配置错误（值越界/键未知/类型不符/叠层冲突/
//!      扩容位越集/基线缺键/迁移残流/隐私红线/外联红线/闸门未开）逐类
//!      注入，每类必产一份用户可见呈现。
//!   2. 三要素闭域：呈现必含 ①错误码域 [0xC800,0xC900)（B16 族）②用户
//!      话术码域 [0xE000,0xF000) ③处理建议码域 [0xF000,0x10000)——
//!      三码任一缺位或越域即 E_ELEM_MISS / E_DOMAIN_MIX。
//!   3. 可操作性核：建议码必须落在可操作族（0xF0xx 修值/0xF1xx 回滚/
//!      0xF2xx 求助三族），不可操作建议即假建议（E_ADVICE_DEAD）。
//!   4. 注入即呈现守恒（J1）：×10 注入 ×10 呈现，逐条三要素在位；
//!      注入量与呈现账守恒，漏呈现即哑错误（E_SILENT_ERR）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC880;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ELEM_MISS: u16 = SEG_BASE + 1; // 三要素缺位
const E_DOMAIN_MIX: u16 = SEG_BASE + 2; // 呈现码越域
const E_ADVICE_DEAD: u16 = SEG_BASE + 3; // 建议不可操作
const E_SILENT_ERR: u16 = SEG_BASE + 4; // 注入无呈现（哑错误）
const E_STATE_BAD: u16 = SEG_BASE + 5; // 呈现器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 注入面十类闭集。
pub const INJ_VAL_RANGE: u8 = 0; // 值越界
pub const INJ_KEY_UNKNOWN: u8 = 1; // 键未知
pub const INJ_TYPE_MISMATCH: u8 = 2; // 类型不符
pub const INJ_OVERLAY_CONFLICT: u8 = 3; // 叠层冲突
pub const INJ_EXT_OOB: u8 = 4; // 扩容位越集
pub const INJ_BASE_MISS: u8 = 5; // 基线缺键
pub const INJ_MIG_PARTIAL: u8 = 6; // 迁移残流
pub const INJ_PRIVACY: u8 = 7; // 隐私红线
pub const INJ_PHONE_OUT: u8 = 8; // 外联红线
pub const INJ_GATE_LOCKED: u8 = 9; // 闸门未开
pub const INJ_N: usize = 10;

/// 呈现三码域。
pub const DOM_CODE_LO: u16 = 0xC800; // B16 错误码族
pub const DOM_CODE_HI: u16 = 0xC900;
pub const DOM_UTTR_LO: u16 = 0xE000;
pub const DOM_UTTR_HI: u16 = 0xF000;
pub const DOM_ADVICE_LO: u16 = 0xF000;
pub const DOM_ADVICE_HI: u32 = 0x1_0000;

/// 可操作建议族：修值 / 回滚 / 求助。
pub const ADV_FIX: u16 = 0xF000;
pub const ADV_ROLLBACK: u16 = 0xF100;
pub const ADV_ESCALATE: u16 = 0xF200;

/// 一份错误呈现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Present {
    pub code: u16,
    pub uttr: u16,
    pub advice: u16,
}

/// 呈现器。
pub struct CfgErrPresenter {
    pub presented: Vec<(u8, Present)>, // (注入类, 呈现)
    pub injected: u32,
}

impl CfgErrPresenter {
    pub fn new() -> CfgErrPresenter {
        CfgErrPresenter { presented: Vec::new(), injected: 0 }
    }

    /// 注入 → 呈现（确定性映射：类号驱动三码，同源编码）。
    pub fn present(&mut self, inj: u8) -> Result<Present, u16> {
        self.injected += 1;
        if inj as usize >= INJ_N {
            return Err(E_STATE_BAD); // 注入类越闭集不呈现
        }
        // 逐类呈现表（码族内连续、话术/建议按类配对）。
        let p = match inj {
            INJ_VAL_RANGE => Present { code: 0xC801, uttr: 0xE001, advice: ADV_FIX + 1 },
            INJ_KEY_UNKNOWN => Present { code: 0xC802, uttr: 0xE002, advice: ADV_FIX + 2 },
            INJ_TYPE_MISMATCH => Present { code: 0xC803, uttr: 0xE003, advice: ADV_FIX + 3 },
            INJ_OVERLAY_CONFLICT => Present { code: 0xC804, uttr: 0xE004, advice: ADV_ROLLBACK + 1 },
            INJ_EXT_OOB => Present { code: 0xC805, uttr: 0xE005, advice: ADV_ROLLBACK + 2 },
            INJ_BASE_MISS => Present { code: 0xC806, uttr: 0xE006, advice: ADV_FIX + 6 },
            INJ_MIG_PARTIAL => Present { code: 0xC807, uttr: 0xE007, advice: ADV_ROLLBACK + 3 },
            INJ_PRIVACY => Present { code: 0xC808, uttr: 0xE008, advice: ADV_ESCALATE + 1 },
            INJ_PHONE_OUT => Present { code: 0xC809, uttr: 0xE009, advice: ADV_ESCALATE + 2 },
            _ => Present { code: 0xC80A, uttr: 0xE00A, advice: ADV_ESCALATE + 3 },
        };
        self.presented.push((inj, p));
        Ok(p)
    }

    /// 三要素单条核验：三码在位 + 三域各归其位 + 建议可操作。
    pub fn check_present(p: &Present) -> Result<(), u16> {
        if p.code == 0 || p.uttr == 0 || p.advice == 0 {
            return Err(E_ELEM_MISS);
        }
        if p.code < DOM_CODE_LO || p.code >= DOM_CODE_HI {
            return Err(E_DOMAIN_MIX);
        }
        if p.uttr < DOM_UTTR_LO || p.uttr >= DOM_UTTR_HI {
            return Err(E_DOMAIN_MIX);
        }
        if p.advice < DOM_ADVICE_LO || (p.advice as u32) >= DOM_ADVICE_HI {
            return Err(E_DOMAIN_MIX);
        }
        // 可操作族核：0xF0xx 修值 / 0xF1xx 回滚 / 0xF2xx 求助。
        let fam = p.advice & 0xFF00;
        if fam != ADV_FIX && fam != ADV_ROLLBACK && fam != ADV_ESCALATE {
            return Err(E_ADVICE_DEAD);
        }
        Ok(())
    }

    /// 注入即呈现守恒（J1 主体）：×10 注入 ×10 呈现、注入类闭集
    /// 全覆盖、逐条三要素在位。
    pub fn sweep(&mut self) -> Result<(), u16> {
        let mut inj = 0u8;
        while (inj as usize) < INJ_N {
            let p = self.present(inj)?;
            CfgErrPresenter::check_present(&p)?;
            inj += 1;
        }
        if self.presented.len() as u32 != self.injected || self.injected != INJ_N as u32 {
            return Err(E_SILENT_ERR);
        }
        Ok(())
    }

    /// 呈现账审计：呈现账与注入账守恒、类号闭集、呈现码两两互异
    /// （十类不撞呈现）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.presented.len() as u32 != self.injected {
            return Err(E_SILENT_ERR);
        }
        let mut i = 0usize;
        while i < self.presented.len() {
            let (inj, _) = self.presented[i];
            if inj as usize >= INJ_N {
                return Err(E_STATE_BAD);
            }
            i += 1;
        }
        let mut a = 0usize;
        while a < self.presented.len() {
            let mut b = a + 1;
            while b < self.presented.len() {
                if self.presented[a].1.code == self.presented[b].1.code {
                    return Err(E_STATE_BAD); // 十类撞呈现码
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：呈现全量 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.presented.len() as u32 != INJ_N as u32 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（注入 ×10 三要素全齐），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：十类注入逐类呈现，逐条三要素在位、三域各归其位、
    //    建议可操作。
    let mut pr = CfgErrPresenter::new();
    pr.sweep()?;
    if pr.presented.len() != 10 {
        return Err(E_SILENT_ERR);
    }
    pr.audit()?;
    hits += 1;

    // R2 呈现表自洽终判：十类呈现码 = 0xC801..0xC80A 连续族内（同源
    //    编码零漂移），话术码 = 0xE001..0xE00A 同步。
    let mut inj = 0u8;
    while (inj as usize) < INJ_N {
        let (_, p) = pr.presented[inj as usize];
        if p.code != 0xC801 + inj as u16 {
            return Err(E_DOMAIN_MIX);
        }
        if p.uttr != 0xE001 + inj as u16 {
            return Err(E_DOMAIN_MIX);
        }
        inj += 1;
    }
    hits += 1;

    // R3 三要素缺位防线：构造缺位呈现（话术零）→ 核验必报
    //    E_ELEM_MISS（三要素口径非恒绿）。
    let bare = Present { code: 0xC801, uttr: 0, advice: ADV_FIX };
    if CfgErrPresenter::check_present(&bare).is_ok() {
        return Err(E_ELEM_MISS);
    }
    let bare2 = Present { code: 0xC801, uttr: 0xE001, advice: 0 };
    if CfgErrPresenter::check_present(&bare2).is_ok() {
        return Err(E_ELEM_MISS);
    }
    hits += 1;

    // R4 域错位防线：错误码塞话术域值 / 建议越上域 → E_DOMAIN_MIX。
    let mixed = Present { code: 0xE001, uttr: 0xE001, advice: ADV_FIX };
    if CfgErrPresenter::check_present(&mixed).is_ok() {
        return Err(E_DOMAIN_MIX);
    }
    let mixed2 = Present { code: 0xC801, uttr: 0xE001, advice: (0x10001u32 >> 1) as u16 };
    if CfgErrPresenter::check_present(&mixed2).is_ok() {
        return Err(E_DOMAIN_MIX);
    }
    hits += 1;

    // R5 假建议防线：建议族落在不可操作区（0xF3xx）→ E_ADVICE_DEAD。
    let dead = Present { code: 0xC801, uttr: 0xE001, advice: 0xF301 };
    if CfgErrPresenter::check_present(&dead).is_ok() {
        return Err(E_ADVICE_DEAD);
    }
    hits += 1;

    // R6 注入类越界拒收：inj=10 → E_STATE_BAD、不呈现不入账。
    let mut pr2 = CfgErrPresenter::new();
    if pr2.present(10).is_ok() {
        return Err(E_STATE_BAD);
    }
    if pr2.presented.len() != 0 || pr2.injected != 1 {
        return Err(E_SILENT_ERR);
    }
    hits += 1;

    // R7 呈现可操作族分布：十类建议覆盖三族（修值 4 / 回滚 3 / 求助 3），
    //    无一假建议。
    let mut fix_n = 0u32;
    let mut rb_n = 0u32;
    let mut esc_n = 0u32;
    let mut i = 0usize;
    while i < pr.presented.len() {
        let fam = pr.presented[i].1.advice & 0xFF00;
        if fam == ADV_FIX {
            fix_n += 1;
        } else if fam == ADV_ROLLBACK {
            rb_n += 1;
        } else if fam == ADV_ESCALATE {
            esc_n += 1;
        }
        i += 1;
    }
    if fix_n != 4 || rb_n != 3 || esc_n != 3 {
        return Err(E_ADVICE_DEAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：呈现不全不写回、空摘要拒、全量+非空摘要放行。
    let mut pr3 = CfgErrPresenter::new();
    if pr3.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    pr3.sweep()?;
    if pr3.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    pr3.writeback(true, 0x0A)?;
    hits += 1;

    Ok(hits)
}
