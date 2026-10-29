//! unx0816_exact3 —— UNX-F0816 · #AC/#XM/#BP 精确事件异常三件（目标 220 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0816 · #AC/#XM/#BP 精确事件异常三件。
//! 判据原文：UNX-F0816-J1 三异常注入处置正确率 100%，CPL 联动语义（#AC 双态）
//!           分档正确。
//! 设计纪律：
//! - 三异常向量：#AC=17（对齐检查）/ #XM=19（SIMD 浮点）/ #BP=3（断点 Trap）。
//! - #AC 双态语义（判据核心）：CPL3 + CR0.AM=1 + EFLAGS.AC=1 才使能——
//!   内核态（CPL0）永不触发 #AC（CR0.AM 只管用户态对齐检查）。分档表固化。
//! - #XM：CR4.OSXMMEXCPT=1 时 #XM 替代 #UD 交付 SIMD 异常；MXCSR 高 6 位
//!   状态（IE/DE/ZE/OE/UE/PE）解码为异常源，精度事件（精确/不精确）由
//!   非精确位区分。
//! - #BP：INT3 指令触发的 Trap，无错误码；优先级最低（同指令多事件时
//!   #AC > #XM > #BP 的报告顺序表固化）。
//! - 处置账：注入→分档→处置→落账；处置码三态（忽略/转发用户/内核终态）。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6360（0x6361~0x636A 闭集；全域 grep 确认独占）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const EX_VERSION: &str = "unx0816_exact3-v1";

/// 诊断段基址（0x6360 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6360;

/// XcCode 闭集。
pub const XC_VEC_UNK: u16 = SEG_BASE + 1; // 非三件向量误入
pub const XC_AC_NOT_ARMED: u16 = SEG_BASE + 2; // #AC 触发条件未满足却被注入
pub const XC_MXCSR_BAD: u16 = SEG_BASE + 3; // MXCSR 保留位非零
pub const XC_PRIORITY_BAD: u16 = SEG_BASE + 4; // 报告顺序违反
pub const XC_LED_FULL: u16 = SEG_BASE + 5; // 账溢出
pub const XC_CPL_BAD: u16 = SEG_BASE + 6; // CPL 非法
pub const XC_CR_BAD: u16 = SEG_BASE + 7; // CR0.AM/CR4 位非法组合

/// 三件向量。
pub const VEC_BP: u8 = 3;
pub const VEC_AC: u8 = 17;
pub const VEC_XM: u8 = 19;

/// CR0.AM（对齐掩码使能）。
pub const CR0_AM: u32 = 1 << 18;
/// EFLAGS.AC（对齐检查使能）。
pub const EFLAGS_AC: u32 = 1 << 18;

/// MXCSR 状态位（高 6 位）。
pub const MXCSR_IE: u32 = 1 << 0; // 无效操作
pub const MXCSR_DE: u32 = 1 << 1; // 非规格化
pub const MXCSR_ZE: u32 = 1 << 2; // 除零
pub const MXCSR_OE: u32 = 1 << 3; // 上溢
pub const MXCSR_UE: u32 = 1 << 4; // 下溢
pub const MXCSR_PE: u32 = 1 << 5; // 精度
pub const MXCSR_LEGAL_MASK: u32 = 0xFFFF; // 建模面：状态+掩码全 16 位合法

/// #AC 使能判定（SDM 双态语义固化）：CPL3 + CR0.AM + EFLAGS.AC 三与。
/// CPL0 恒不触发（内核路径对齐由架构保证，#AC 只服务用户态）。
pub fn ac_armed(cpl: u8, cr0: u32, eflags: u32) -> Result<bool, u16> {
    if cpl > 3 {
        return Err(XC_CPL_BAD);
    }
    if cpl == 0 {
        return Ok(false);
    }
    Ok(cr0 & CR0_AM != 0 && eflags & EFLAGS_AC != 0)
}

/// #XM 异常源解码（MXCSR 状态高 6 位 → 源列表；多源并发按位并存）。
pub fn decode_mxcsr(mxcsr: u32) -> Result<Vec<u8>, u16> {
    if mxcsr & !MXCSR_LEGAL_MASK != 0 {
        return Err(XC_MXCSR_BAD);
    }
    let mut srcs = Vec::new();
    for (bit, name) in [
        (MXCSR_IE, 0u8),
        (MXCSR_DE, 1),
        (MXCSR_ZE, 2),
        (MXCSR_OE, 3),
        (MXCSR_UE, 4),
        (MXCSR_PE, 5),
    ] {
        if mxcsr & bit != 0 {
            srcs.push(name);
        }
    }
    Ok(srcs)
}

/// 处置码三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disp {
    /// 忽略（事件不升级）。
    Ignore = 0,
    /// 转发用户态（进程信号）。
    ToUser = 1,
    /// 内核终态。
    Fatal = 2,
}

/// 三件分档处置（CPL 联动语义表固化）：
/// - #BP：用户态 → ToUser（调试器交付）；内核态 → Ignore（int3 探针常态）。
/// - #AC：用户态 armed → ToUser；内核态注入 = 违反双态语义 → 拒。
/// - #XM：用户态 → ToUser；内核态（内核浮点已禁）→ Fatal。
pub fn triage(vec: u8, cpl: u8) -> Result<Disp, u16> {
    if cpl > 3 {
        return Err(XC_CPL_BAD);
    }
    match vec {
        VEC_BP => Ok(if cpl == 0 { Disp::Ignore } else { Disp::ToUser }),
        VEC_AC => {
            if cpl == 0 {
                Err(XC_AC_NOT_ARMED)
            } else {
                Ok(Disp::ToUser)
            }
        }
        VEC_XM => Ok(if cpl == 0 { Disp::Fatal } else { Disp::ToUser }),
        _ => Err(XC_VEC_UNK),
    }
}

/// 同指令多事件报告顺序（SDM 优先级固化）：#AC(17) > #XM(19) > #BP(3)。
/// 输入事件集合，输出按优先级排序的向量序列；违反优先级注入即红。
pub fn report_order(events: &[u8]) -> Result<Vec<u8>, u16> {
    for v in events {
        if !matches!(*v, VEC_BP | VEC_AC | VEC_XM) {
            return Err(XC_VEC_UNK);
        }
    }
    let prio = |v: u8| -> u8 {
        match v {
            VEC_AC => 0, // 最高
            VEC_XM => 1,
            _ => 2,      // BP 最低
        }
    };
    let mut out = events.to_vec();
    // 稳定排序（按优先级升序）
    out.sort_by_key(|v| prio(*v));
    Ok(out)
}

/// 处置账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExRow {
    pub vec: u8,
    pub cpl: u8,
    pub disp: Disp,
    pub seq: u64,
}

/// 三件处置引擎。
pub struct ExactTrio {
    rows: Vec<ExRow>,
    overflow: usize,
    seq: u64,
}

impl ExactTrio {
    pub fn new() -> ExactTrio {
        ExactTrio {
            rows: Vec::new(),
            overflow: 0,
            seq: 0,
        }
    }

    /// 注入：#AC 先过使能判定（CR0/EFLAGS 联动），其余直入 triage。
    pub fn inject(
        &mut self,
        vec: u8,
        cpl: u8,
        cr0: u32,
        eflags: u32,
    ) -> Result<Disp, u16> {
        if vec == VEC_AC {
            let armed = ac_armed(cpl, cr0, eflags)?;
            if !armed {
                return Err(XC_AC_NOT_ARMED);
            }
        }
        let d = triage(vec, cpl)?;
        self.seq += 1;
        if self.rows.len() < 64 {
            self.rows.push(ExRow {
                vec,
                cpl,
                disp: d,
                seq: self.seq,
            });
        } else {
            self.overflow += 1;
        }
        Ok(d)
    }

    /// 账读取。
    pub fn row(&self, i: usize) -> Result<ExRow, u16> {
        if i >= self.rows.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.rows[i])
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.rows.len() + self.overflow == self.seq as usize
    }
}

/// drill：全链自例程（三件处置 + #AC 双态 + 报告顺序 + 错误路径）。
pub fn drill() -> Result<u32, u16> {
    let mut e = ExactTrio::new();
    // 1. #BP：用户转发 / 内核忽略
    if e.inject(VEC_BP, 3, 0, 0)? != Disp::ToUser {
        return Err(XC_PRIORITY_BAD);
    }
    if e.inject(VEC_BP, 0, 0, 0)? != Disp::Ignore {
        return Err(XC_PRIORITY_BAD);
    }
    // 2. #AC 双态：armed（CPL3+AM+AC）→ ToUser；CPL0 → 拒
    if e.inject(VEC_AC, 3, CR0_AM, EFLAGS_AC)? != Disp::ToUser {
        return Err(XC_PRIORITY_BAD);
    }
    if e.inject(VEC_AC, 0, CR0_AM, EFLAGS_AC).is_ok() {
        return Err(XC_AC_NOT_ARMED);
    }
    // 3. #AC 未使能（缺 EFLAGS.AC）→ 拒
    if e.inject(VEC_AC, 3, CR0_AM, 0).is_ok() {
        return Err(XC_AC_NOT_ARMED);
    }
    // 4. #XM：用户转发 / 内核终态
    if e.inject(VEC_XM, 3, 0, 0)? != Disp::ToUser {
        return Err(XC_PRIORITY_BAD);
    }
    if e.inject(VEC_XM, 0, 0, 0)? != Disp::Fatal {
        return Err(XC_PRIORITY_BAD);
    }
    // 5. 守恒
    if !e.conserve() {
        return Err(XC_LED_FULL);
    }
    // 6. 报告顺序：{BP, AC, XM} → [AC, XM, BP]
    let order = report_order(&[VEC_BP, VEC_AC, VEC_XM])?;
    if order != vec![VEC_AC, VEC_XM, VEC_BP] {
        return Err(XC_PRIORITY_BAD);
    }
    // 7. MXCSR 解码：多源并存
    let srcs = decode_mxcsr(MXCSR_IE | MXCSR_ZE | MXCSR_PE)?;
    if srcs != vec![0u8, 2, 5] {
        return Err(XC_MXCSR_BAD);
    }
    // 8. 错误路径①：非三件向量
    if e.inject(14, 3, 0, 0).is_ok() {
        return Err(XC_VEC_UNK);
    }
    // 9. 错误路径②：MXCSR 保留位（高 16 位建模面外）
    if decode_mxcsr(0x1_0000).is_ok() {
        return Err(XC_MXCSR_BAD);
    }
    // 10. 错误路径③：CPL=5
    if e.inject(VEC_BP, 5, 0, 0).is_ok() {
        return Err(XC_CPL_BAD);
    }
    // 11. 错误路径④：报告顺序含非法向量
    if report_order(&[14]).is_ok() {
        return Err(XC_VEC_UNK);
    }
    Ok(0)
}
