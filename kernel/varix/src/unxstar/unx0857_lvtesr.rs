//! UNX-F0857 · LVT 错误中断与 ESR 联动 EOI 路径 · 诊断段 0x6800（独占 0x6800~0x680F）
//!
//! **判据原文摘录**：错误中断 EOI 路径正确（错误 handler EOI 10/10 次），
//! ESR 分账与中断账对平。
//!
//! **设计纪律**：
//! 1. LVT 错误中断建模（SDM §10.5.3/§10.9）：LVT Error 表项向量 0x17 槽
//!    （模型取 0x53），错误源（internal/external，ESR bit0）置 ESR 位 →
//!    触发错误中断（向量 accept 进 ISR）→ 错误 handler 读 ESR 清源 →
//!    EOI 写清 ISR。EOI 缺失 ⇒ 下一次错误源无法再报（错误中断单次挂起）。
//! 2. ESR 二段清（SDM：写 1 清全部，再读得新错误——模型取「读清 + 复读
//!    验零」二段协议）：handler 必须读清 ESR 且复读为零，缺段即红。
//! 3. 对平恒等式（判据「ESR 分账与中断账对平」）：`esr_raised =
//!    error_isr_accepted + esr_pending_unreported`；`error_eoi_done =
//!    error_isr_cleared`；两账差 = 0 才对平。
//! 4. 注入面：10/10 次错误中断全走 handler→EOI 路径；注入 EOI 缺失
//!    面次次被「对平器」抓获。
//! 5. 零 unwrap/expect/panic/浮点/std；确定性；无墙钟。
//!
//! **诊断段声明**：0x6800~0x680F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

// ===========================================================================
// 一、诊断码（0x6800~0x680F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6800;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x680F;

/// 错误中断向量非法（必须为 LVT Error 槽 0x53）。
pub const X_LE_VEC_BAD: u16 = 0x6801;
/// ESR 读清缺段（handler 未读清 ESR 即 EOI）。
pub const X_LE_ESR_NOT_CLEARED: u16 = 0x6802;
/// ESR 复读非零（二段清失败——错误源残留）。
pub const X_LE_ESR_RESIDUE: u16 = 0x6803;
/// 错误中断无 EOI（ISR 残留——下轮错误无法上报）。
pub const X_LE_EOI_MISSING: u16 = 0x6804;
/// 对平失败（ESR 分账 ≠ 中断账）。
pub const X_LE_MISMATCH: u16 = 0x6805;
/// ESR 位越界（写入未定义位）。
pub const X_LE_ESR_BIT: u16 = 0x6806;
/// LVT 表项配置非法（掩码态下发错误中断）。
pub const X_LE_LVT_MASKED: u16 = 0x6807;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_LE_VEC_BAD => "错误中断向量非法",
        X_LE_ESR_NOT_CLEARED => "ESR读清缺段",
        X_LE_ESR_RESIDUE => "ESR复读非零",
        X_LE_EOI_MISSING => "错误中断缺EOI",
        X_LE_MISMATCH => "ESR与中断账不对平",
        X_LE_ESR_BIT => "ESR位越界",
        X_LE_LVT_MASKED => "LVT被屏蔽",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、LVT Error 表项与 ESR 寄存器模型
// ===========================================================================

/// LVT Error 槽向量号（模型约定 0x53，避开 0xFF spurious 与 32 保留区）。
pub const LVT_ERROR_VEC: u8 = 0x53;

/// ESR 位闭集（SDM §10.5.5.3 常用位子集）。
pub mod esr_bits {
    /// bit0：发送校验错（internal）。
    pub const SEND_CHECKSUM: u32 = 1 << 0;
    /// bit1：接收校验错。
    pub const RECV_CHECKSUM: u32 = 1 << 1;
    /// bit2：发送 accept 错。
    pub const SEND_ACCEPT: u32 = 1 << 2;
    /// bit3：接收 accept 错。
    pub const RECV_ACCEPT: u32 = 1 << 3;
    /// bit5：发送 ILLVEC。
    pub const SEND_ILLVEC: u32 = 1 << 5;
    /// bit6：接收 ILLVEC。
    pub const RECV_ILLVEC: u32 = 1 << 6;
    /// 合法位掩码。
    pub const LEGAL_MASK: u32 =
        SEND_CHECKSUM | RECV_CHECKSUM | SEND_ACCEPT | RECV_ACCEPT | SEND_ILLVEC | RECV_ILLVEC;
}

/// LVT Error 表项。
#[derive(Clone, Copy)]
pub struct LvtErrorEntry {
    /// 向量（0x53）。
    pub vector: u8,
    /// 掩码位（1=屏蔽）。
    pub masked: bool,
}

/// ESR + LVT + 错误中断联动核（单核面）。
pub struct ErrorEsrCore {
    pub lvt: LvtErrorEntry,
    /// ESR 现行错误位（读清前）。
    esr: u32,
    /// 错误中断在服（ISR 位）。
    pub error_isr: bool,
    /// 错误中断挂起（未 accept 的第二错误——SDM：单次挂起）。
    pub pending: bool,
    // —— 对平账 ——
    /// ESR 错误源置位次数（分账）。
    pub esr_raised: u64,
    /// 错误中断 accept 次数（中断账）。
    pub error_accepted: u64,
    /// 未上报挂起（accept 时已有在服——挂起累计）。
    pub pending_total: u64,
    /// 错误 handler EOI 完成次数。
    pub error_eoi_done: u64,
    /// ISR 残留次数（EOI 缺失被对平器抓获）。
    pub isr_residue_caught: u64,
}

impl ErrorEsrCore {
    pub fn new() -> Self {
        Self {
            lvt: LvtErrorEntry {
                vector: LVT_ERROR_VEC,
                masked: false,
            },
            esr: 0,
            error_isr: false,
            pending: false,
            esr_raised: 0,
            error_accepted: 0,
            pending_total: 0,
            error_eoi_done: 0,
            isr_residue_caught: 0,
        }
    }

    /// 错误源置位（ESR 位或运算；非法位拒绝）。
    pub fn raise(&mut self, bits: u32) -> Result<(), u16> {
        if bits & !esr_bits::LEGAL_MASK != 0 {
            return Err(X_LE_ESR_BIT);
        }
        if self.lvt.masked {
            return Err(X_LE_LVT_MASKED);
        }
        self.esr |= bits;
        self.esr_raised += 1;
        // 触发错误中断：无在服 → accept；有在服 → 挂起（单次）。
        if !self.error_isr {
            self.error_isr = true;
            self.error_accepted += 1;
        } else if !self.pending {
            self.pending = true;
            self.pending_total += 1;
        }
        Ok(())
    }

    /// handler 第 1 段：读清 ESR（返回读得位，随后清零）。
    pub fn esr_read_clear(&mut self) -> u32 {
        let v = self.esr;
        self.esr = 0;
        v
    }

    /// handler 第 2 段：复读 ESR（必须为零）。
    pub const fn esr_reread(&self) -> u32 {
        self.esr
    }

    /// EOI 写（错误 handler 尾）：要求 ESR 已读清（缺段拒绝）。
    /// EOI 后若挂起未上报，SDM 语义下挂起错误作为新错误中断立即投递
    /// （pending→accept 重入 ISR），需再一次 EOI 收口。
    pub fn error_eoi(&mut self) -> Result<(), u16> {
        if self.esr != 0 {
            // ESR 未读清即 EOI：SDM 违例——错误源信息丢失。
            return Err(X_LE_ESR_NOT_CLEARED);
        }
        if !self.error_isr {
            return Err(X_LE_EOI_MISSING);
        }
        self.error_isr = false;
        self.error_eoi_done += 1;
        if self.pending {
            // 挂起错误立即重投递（单次挂起语义收口）。
            self.pending = false;
            self.error_isr = true;
            self.error_accepted += 1;
        }
        Ok(())
    }

    /// 对平器：ESR 分账 vs 中断账。
    /// 恒等式 1：esr_raised = error_accepted + (挂起未上报?1:0)。
    /// 恒等式 2：error_eoi_done + (error_isr?1:0) = error_accepted
    ///           （每次 accept 必以 EOI 或现行在服收口）。
    pub const fn reconcile(&self) -> Result<(), u16> {
        let pend_now = if self.pending { 1 } else { 0 };
        if self.esr_raised != self.error_accepted + pend_now {
            return Err(X_LE_MISMATCH);
        }
        let isr_now = if self.error_isr { 1 } else { 0 };
        if self.error_eoi_done + isr_now != self.error_accepted {
            return Err(X_LE_MISMATCH);
        }
        Ok(())
    }

    /// 残留扫描：EOI 缺失即抓获（错误中断在服未收口——纯漏 EOI）。
    pub const fn scan_residue(&mut self) -> bool {
        if self.error_isr {
            self.isr_residue_caught += 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// 三、注入演练（10/10 错误中断 EOI 正确 + EOI 缺失必抓）
// ===========================================================================

/// 演练结论。
pub struct ErrVerdict {
    pub injected: u64,
    pub eoi_correct: u64,
    pub missing_caught: u64,
    pub reconcile_ok: bool,
}

/// 注入演练：
/// · 前 10 轮：完整路径 raise→handler(读清+复读)→EOI，10/10 正确；
/// · 后 2 轮：注入 EOI 缺失（raise 后不 eoi），对平器/残留扫描必须抓获。
pub fn run_error_drill() -> Result<ErrVerdict, u16> {
    let mut core = ErrorEsrCore::new();
    let mut eoi_correct = 0u64;
    // —— 正确路径 10 轮。
    let mut k = 0;
    while k < 10 {
        // 错误源轮换注入（6 位闭集轮转）。
        let bits = match k % 6 {
            0 => esr_bits::SEND_CHECKSUM,
            1 => esr_bits::RECV_CHECKSUM,
            2 => esr_bits::SEND_ACCEPT,
            3 => esr_bits::RECV_ACCEPT,
            4 => esr_bits::SEND_ILLVEC,
            _ => esr_bits::RECV_ILLVEC,
        };
        core.raise(bits)?;
        // handler：读清 + 复读验零。
        let got = core.esr_read_clear();
        if got != bits {
            return Err(X_LE_ESR_RESIDUE);
        }
        if core.esr_reread() != 0 {
            return Err(X_LE_ESR_RESIDUE);
        }
        // EOI。
        core.error_eoi()?;
        eoi_correct += 1;
        k += 1;
    }
    // —— EOI 缺失注入 2 轮：必须被残留扫描/对平器抓获。
    // 每轮注两源：第一源 accept、第二源撞在服挂起（SDM 单次挂起语义），
    // 不 EOI ⇒ 残留扫描抓获；随后 handler 读清 + EOI 收口。
    let mut missing_caught = 0u64;
    let mut k = 0;
    while k < 2 {
        core.raise(esr_bits::SEND_CHECKSUM)?; // accept（在服置位）。
        core.raise(esr_bits::RECV_CHECKSUM)?; // 挂起（单次）。
        // 故意不 EOI。
        if core.scan_residue() {
            missing_caught += 1;
        }
        // 抓获后 handler 收口（读清 + EOI；挂起重投递后再补一次 EOI）。
        core.esr_read_clear();
        core.error_eoi()?;
        if core.error_isr {
            // 挂起错误重投递入 ISR：第二次 EOI 收口。
            core.error_eoi()?;
        }
        k += 1;
    }
    if eoi_correct != 10 {
        return Err(X_LE_MISMATCH);
    }
    if missing_caught != 2 {
        return Err(X_LE_EOI_MISSING);
    }
    core.reconcile()?;
    Ok(ErrVerdict {
        injected: 10,
        eoi_correct,
        missing_caught,
        reconcile_ok: true,
    })
}

// ===========================================================================
// 四、drill 自例程
// ===========================================================================

/// 自例程：10/10 EOI 正确 + 缺失必抓 + 对平 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) ESR 位越界拒绝。
    let mut core = ErrorEsrCore::new();
    if core.raise(1 << 20).is_ok() {
        return Ok(1);
    }
    // 2) LVT 屏蔽态下发错误中断拒绝。
    core.lvt.masked = true;
    if core.raise(esr_bits::SEND_CHECKSUM).is_ok() {
        return Ok(2);
    }
    core.lvt.masked = false;
    // 3) ESR 未读清即 EOI 拒绝。
    core.raise(esr_bits::SEND_CHECKSUM)?;
    if core.error_eoi().is_ok() {
        return Ok(3);
    }
    // 4) 主路径：10/10 正确 + 缺失必抓。
    let v = run_error_drill()?;
    if v.eoi_correct != 10 || v.missing_caught != 2 {
        return Err(X_LE_MISMATCH);
    }
    // 5) 对平账：14 源 raise = 14 accept（含 2 挂起重投递），挂起累计 2。
    if core.esr_raised != 14 || core.error_accepted != 14 || core.pending_total != 2 {
        return Ok(5);
    }
    // 6) 挂起未上报已全部重投递收口：pending 清零、ISR 无残留、对平过。
    if core.pending || core.error_isr {
        return Ok(6);
    }
    core.reconcile()?;
    // 7) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_LE_VEC_BAD, X_LE_ESR_NOT_CLEARED, X_LE_ESR_RESIDUE, X_LE_EOI_MISSING,
        X_LE_MISMATCH, X_LE_ESR_BIT, X_LE_LVT_MASKED,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(7);
        }
    }
    Ok(0)
}
