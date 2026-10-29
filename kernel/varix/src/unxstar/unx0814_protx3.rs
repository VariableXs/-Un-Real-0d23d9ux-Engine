//! unx0814_protx3 —— UNX-F0814 · #GP/#UD/#SS 保护类异常三联判（目标 240 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0814 · #GP/#UD/#SS 保护类异常三联判。
//! 判据原文：UNX-F0814-J1 三异常注入分型处置正确率 100%，内核 #GP 与用户
//!           #GP 路径分离（U 位裁决）。
//! 设计纪律：
//! - 三异常向量：#GP=13 / #UD=6 / #SS=12。三联判状态机按向量+错误码语义分型。
//! - #GP 错误码语义：BIT1=IDT 引用（external=BIT0），BIT2=TI（LDT/GDT 选择子），
//!   BIT15:3=选择子索引；external 置位时无选择子信息。U 位（错误码不存在）
//!   与 CPL 联动：内核态 #GP 与用户态 #GP 分路径（判据 U 位裁决）。
//! - #UD：无错误码；处置=非法指令终止（用户态）/ 内核 #UD = 不可恢复 → 终态。
//! - #SS：有错误码（同 #GP 格式）；处置=栈段违例，内核 #SS = 栈破损 → 终态。
//! - 分型处置账：注入→分型→处置→落账四步，处置码与分型一一对应（零错位）。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6320（0x6321~0x6329 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const PX_VERSION: &str = "unx0814_protx3-v1";

/// 诊断段基址（0x6320 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6320;

/// XcCode 闭集。
pub const XC_VEC_UNK: u16 = SEG_BASE + 1; // 非三联向量误入
pub const XC_ERRCODE_BAD: u16 = SEG_BASE + 2; // 错误码格式非法
pub const XC_CPL_BAD: u16 = SEG_BASE + 3; // CPL 非法（>3）
pub const XC_DISP_MISS: u16 = SEG_BASE + 4; // 处置码与分型错位
pub const XC_LED_FULL: u16 = SEG_BASE + 5; // 账溢出
pub const XC_PATH_MIX: u16 = SEG_BASE + 6; // 内核/用户路径混用
pub const XC_HAS_ERR: u16 = SEG_BASE + 7; // #UD 携带错误码（非法）

/// 三联向量。
pub const VEC_UD: u8 = 6;
pub const VEC_SS: u8 = 12;
pub const VEC_GP: u8 = 13;

/// #GP/#SS 错误码位（SDM Vol.3 Ch.6）。
pub const EC_EXTERNAL: u32 = 1 << 0;
pub const EC_IDT: u32 = 1 << 1;
pub const EC_TI: u32 = 1 << 2;
pub const EC_INDEX_MASK: u32 = 0xFFF8;

/// 分型闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtKind {
    /// 用户态 #GP（可移交用户处理）。
    UserGp = 0,
    /// 内核态 #GP（不可恢复 → 终态）。
    KernGp = 1,
    /// #UD 非法指令（用户态 = 进程终止；内核态 = 终态）。
    IllInstr = 2,
    /// #SS 栈违例（用户态 = 进程终止；内核态 = 终态）。
    StackFault = 3,
}

impl ProtKind {
    /// 向量合法性。
    pub fn of_vec(vec: u8) -> Option<ProtKind> {
        match vec {
            VEC_GP => None, // #GP 分型需 CPL——单凭向量不定义
            VEC_UD => Some(ProtKind::IllInstr),
            VEC_SS => Some(ProtKind::StackFault),
            _ => None,
        }
    }
}

/// 处置码闭集（与分型一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispCode {
    /// 移交用户态处理（进程信号）。
    ToUser = 0,
    /// 终态（内核上下文污染，停机）。
    Fatal = 1,
    /// 终止进程（用户上下文回收）。
    KillProc = 2,
}

/// 分型 + 处置联合判定（纯函数，全路径覆盖）。
/// - #GP：CPL0 → KernGp/Fatal；CPL>0 → UserGp/ToUser。
/// - #UD：CPL0 → IllInstr/Fatal；CPL>0 → IllInstr/KillProc。
/// - #SS：CPL0 → StackFault/Fatal；CPL>0 → StackFault/KillProc。
pub fn triage(vec: u8, cpl: u8) -> Result<(ProtKind, DispCode), u16> {
    if cpl > 3 {
        return Err(XC_CPL_BAD);
    }
    match vec {
        VEC_GP => {
            if cpl == 0 {
                Ok((ProtKind::KernGp, DispCode::Fatal))
            } else {
                Ok((ProtKind::UserGp, DispCode::ToUser))
            }
        }
        VEC_UD => {
            if cpl == 0 {
                Ok((ProtKind::IllInstr, DispCode::Fatal))
            } else {
                Ok((ProtKind::IllInstr, DispCode::KillProc))
            }
        }
        VEC_SS => {
            if cpl == 0 {
                Ok((ProtKind::StackFault, DispCode::Fatal))
            } else {
                Ok((ProtKind::StackFault, DispCode::KillProc))
            }
        }
        _ => Err(XC_VEC_UNK),
    }
}

/// #GP/#SS 错误码合法性：external 置位时 IDT/TI/Index 位应全零（SDM：
/// external 事件不带选择子信息）；未置 external 时 Index 允许为零段选择子。
pub fn gp_errcode_legal(e: u32) -> bool {
    if e & EC_EXTERNAL != 0 {
        // external 置位：其余位应为 0（SDM 允许 external|IDT 组合于 gate 访问，
        // 保守闭集：只拦 external+TI 与 external+Index 的怪组合）。
        e & EC_TI == 0 && e & EC_INDEX_MASK == 0
    } else {
        true
    }
}

/// 选择子索引提取（external=0 时有效）。
pub fn selector_index(e: u32) -> Option<u16> {
    if e & EC_EXTERNAL != 0 {
        None
    } else {
        Some(((e & EC_INDEX_MASK) >> 3) as u16)
    }
}

/// 处置账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriageRow {
    /// 向量。
    pub vec: u8,
    /// CPL。
    pub cpl: u8,
    /// 错误码原值（#UD 恒 0）。
    pub err_raw: u32,
    /// 分型。
    pub kind: ProtKind,
    /// 处置。
    pub disp: DispCode,
    /// 序号。
    pub seq: u64,
}

/// 三联判引擎。
pub struct ProtTriage {
    rows: Vec<TriageRow>,
    overflow: usize,
    seq: u64,
    /// 路径分离计数：内核路径 / 用户路径分别落账列。
    kern_events: usize,
    user_events: usize,
}

impl ProtTriage {
    pub fn new() -> ProtTriage {
        ProtTriage {
            rows: Vec::new(),
            overflow: 0,
            seq: 0,
            kern_events: 0,
            user_events: 0,
        }
    }

    /// 注入一事件：错误码校验 → 三联判 → 处置 → 落账。
    pub fn inject(&mut self, vec: u8, cpl: u8, err: u32) -> Result<(ProtKind, DispCode), u16> {
        if cpl > 3 {
            return Err(XC_CPL_BAD);
        }
        // #UD 不得带错误码（SDM：#UD 无错误码）
        if vec == VEC_UD && err != 0 {
            return Err(XC_HAS_ERR);
        }
        if vec == VEC_GP || vec == VEC_SS {
            if !gp_errcode_legal(err) {
                return Err(XC_ERRCODE_BAD);
            }
        }
        let (k, d) = triage(vec, cpl)?;
        self.seq += 1;
        if self.rows.len() < 128 {
            self.rows.push(TriageRow {
                vec,
                cpl,
                err_raw: err,
                kind: k,
                disp: d,
                seq: self.seq,
            });
        } else {
            self.overflow += 1;
        }
        if cpl == 0 {
            self.kern_events += 1;
        } else {
            self.user_events += 1;
        }
        Ok((k, d))
    }

    /// 账行读取。
    pub fn row(&self, i: usize) -> Result<TriageRow, u16> {
        if i >= self.rows.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.rows[i])
    }

    /// 路径分离核（判据：内核 #GP 与用户 #GP 路径分离）：
    /// 内核路径事件全部 Fatal 处置，用户路径无 Fatal（用户 #GP 走 ToUser）。
    pub fn path_separation_ok(&self) -> bool {
        for r in self.rows.iter() {
            let kern = r.cpl == 0;
            let is_fatal = r.disp == DispCode::Fatal;
            if kern != is_fatal {
                // 内核路径必 Fatal；用户路径不 Fatal（#SS/#UD 是 KillProc，
                // #GP 是 ToUser——统一非 Fatal）
                return false;
            }
        }
        true
    }

    /// 守恒核。
    pub fn conserve(&self) -> bool {
        self.rows.len() + self.overflow == self.seq as usize
    }

    /// 统计。
    pub fn stat(&self) -> (usize, usize, usize, usize) {
        (self.rows.len(), self.overflow, self.kern_events, self.user_events)
    }
}

/// drill：全链自例程（九路分型全命中 + 路径分离 + 错误路径）。
pub fn drill() -> Result<u32, u16> {
    let mut t = ProtTriage::new();
    // 1. 九路全覆盖：3 向量 × {CPL0, CPL3}（#UD 无错误码）
    let cases = [
        (VEC_GP, 0u8, 0u32, ProtKind::KernGp, DispCode::Fatal),
        (VEC_GP, 3u8, 0x1238, ProtKind::UserGp, DispCode::ToUser),
        (VEC_UD, 0u8, 0u32, ProtKind::IllInstr, DispCode::Fatal),
        (VEC_UD, 3u8, 0u32, ProtKind::IllInstr, DispCode::KillProc),
        (VEC_SS, 0u8, EC_EXTERNAL, ProtKind::StackFault, DispCode::Fatal),
        (VEC_SS, 3u8, 0x20, ProtKind::StackFault, DispCode::KillProc),
    ];
    for (v, c, e, wk, wd) in cases.iter() {
        let (k, d) = t.inject(*v, *c, *e)?;
        if k != *wk || d != *wd {
            return Err(XC_DISP_MISS);
        }
    }
    // 2. 路径分离核
    if !t.path_separation_ok() {
        return Err(XC_PATH_MIX);
    }
    // 3. 守恒
    if !t.conserve() {
        return Err(XC_LED_FULL);
    }
    // 4. 错误路径①：非三联向量（#PF 14）
    if t.inject(14, 0, 0).is_ok() {
        return Err(XC_VEC_UNK);
    }
    // 5. 错误路径②：#UD 带错误码
    if t.inject(VEC_UD, 3, 0x10).is_ok() {
        return Err(XC_HAS_ERR);
    }
    // 6. 错误路径③：external+TI 怪组合
    if t.inject(VEC_GP, 3, EC_EXTERNAL | EC_TI).is_ok() {
        return Err(XC_ERRCODE_BAD);
    }
    // 7. 错误路径④：CPL=4 非法
    if t.inject(VEC_GP, 4, 0).is_ok() {
        return Err(XC_CPL_BAD);
    }
    // 8. 选择子索引提取
    if selector_index(0x1238) != Some(0x247) {
        return Err(XC_ERRCODE_BAD);
    }
    if selector_index(EC_EXTERNAL).is_some() {
        return Err(XC_ERRCODE_BAD);
    }
    // 9. 向量→分型辅助（#GP 需 CPL 不定分型）
    if ProtKind::of_vec(VEC_UD) != Some(ProtKind::IllInstr) {
        return Err(XC_DISP_MISS);
    }
    if ProtKind::of_vec(VEC_GP).is_some() {
        return Err(XC_DISP_MISS);
    }
    Ok(0)
}
