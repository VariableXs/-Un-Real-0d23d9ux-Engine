//! unx0818_isttree —— UNX-F0818 · 中断栈切换协议（rsp0/IST 选用决策树）（目标 260 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0818 · 中断栈切换协议（rsp0/IST 选用决策树）。
//! 判据原文：UNX-F0818-J1 决策树全覆盖（内核态中断不切栈/用户态中断切
//!           rsp0/IST 向量走 IST），注入验证 10/10 次走对路径。
//! 设计纪律：
//! - 决策树三叶（SDM Vol.3 Ch.6.14.5 固化）：
//!   ① IST 向量（门描述符 IST≠0）→ 切 IST 槽（无条件，最高优先）；
//!   ② CPL 变更（CPL3→CPL0 中断）→ 切 TSS.rsp0；
//!   ③ CPL 不变（内核态中断）→ 不切栈（沿用当前栈）。
//!   优先级：IST 判断先于 CPL 判断（门配了 IST 就走 IST，与 CPL 无关）。
//! - 决策结果五元：目标栈源（IST 槽号/rsp0/当前栈）/ 是否压 SS:RSP /
//!   栈顶对齐要求（16B）/ 切换账行 / 路径标签（0/1/2）。
//! - 切换账：每次决策落账（路径标签 + 向量 + CPL from/to），10 次注入全走对
//!   路径由账回放核验。
//! - 不变量：IST 槽 1~7 供关键向量（DF=#8 必走 IST1——决策器强制断言）；
//!   内核态中断（路径③）绝不压 SS:RSP。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x63A0（0x63A1~0x63A9 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const IT_VERSION: &str = "unx0818_isttree-v1";

/// 诊断段基址（0x63A0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x63A0;

/// XcCode 闭集。
pub const XC_IST_RANGE: u16 = SEG_BASE + 1; // IST 槽越界
pub const XC_PATH_UNK: u16 = SEG_BASE + 2; // 决策路径未定义
pub const XC_CPL_BAD: u16 = SEG_BASE + 3; // CPL 非法
pub const XC_DF_IST: u16 = SEG_BASE + 4; // DF 未走 IST1（强制断言红）
pub const XC_PUSH_MIX: u16 = SEG_BASE + 5; // 路径③压栈（不变量破）
pub const XC_LED_FULL: u16 = SEG_BASE + 6; // 账溢出
pub const XC_ALIGN_BAD: u16 = SEG_BASE + 7; // 栈顶未 16B 对齐
pub const XC_VEC_BAD: u16 = SEG_BASE + 8; // 向量越界
pub const XC_RSP0_ZERO: u16 = SEG_BASE + 9; // rsp0 未配置（路径②缺件）

/// 决策路径标签。
pub const PATH_IST: u8 = 0;
pub const PATH_RSP0: u8 = 1;
pub const PATH_SAME: u8 = 2;

/// DF 向量（强制 IST1）。
pub const VEC_DF: u8 = 8;

/// 栈源闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackSrc {
    /// IST 槽（1~7）。
    Ist(u8),
    /// TSS.rsp0。
    Rsp0,
    /// 沿用当前栈。
    Same,
}

impl StackSrc {
    /// 源标签（账用）。
    pub fn tag(&self) -> u8 {
        match self {
            StackSrc::Ist(_) => PATH_IST,
            StackSrc::Rsp0 => PATH_RSP0,
            StackSrc::Same => PATH_SAME,
        }
    }
}

/// 决策结果五元。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchPlan {
    /// 目标栈源。
    pub src: StackSrc,
    /// 是否压 SS:RSP（仅 CPL 变更路径）。
    pub push_ssrsp: bool,
    /// 16B 对齐要求（压栈后）。
    pub align16: bool,
    /// 路径标签。
    pub path: u8,
}

impl SwitchPlan {
    /// 不变量自检：Same 路径绝不压 SS:RSP。
    pub fn invariants_ok(&self) -> bool {
        if self.src.tag() == PATH_SAME && self.push_ssrsp {
            return false;
        }
        true
    }
}

/// 决策树纯函数（SDM 三叶固化）：
/// ist > 0 → IST 路径（无条件）；否则 CPL from 3 → to 0 → rsp0；否则 Same。
pub fn decide(vec: u8, cpl_from: u8, cpl_to: u8, gate_ist: u8, rsp0: u64) -> Result<SwitchPlan, u16> {
    if cpl_from > 3 || cpl_to > 3 {
        return Err(XC_CPL_BAD);
    }
    if gate_ist > 7 {
        return Err(XC_IST_RANGE);
    }
    // 强制断言：DF 必须配 IST1（缺配即拒）
    if vec == VEC_DF && gate_ist != 1 {
        return Err(XC_DF_IST);
    }
    if gate_ist > 0 {
        return Ok(SwitchPlan {
            src: StackSrc::Ist(gate_ist),
            push_ssrsp: cpl_from != cpl_to, // IST 路径 CPL 变更仍压 SS:RSP（SDM）
            align16: true,
            path: PATH_IST,
        });
    }
    if cpl_from == 3 && cpl_to == 0 {
        if rsp0 == 0 {
            return Err(XC_RSP0_ZERO);
        }
        return Ok(SwitchPlan {
            src: StackSrc::Rsp0,
            push_ssrsp: true,
            align16: true,
            path: PATH_RSP0,
        });
    }
    Ok(SwitchPlan {
        src: StackSrc::Same,
        push_ssrsp: false,
        align16: false,
        path: PATH_SAME,
    })
}

/// 切换账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchRow {
    pub vec: u8,
    pub cpl_from: u8,
    pub cpl_to: u8,
    pub path: u8,
    pub seq: u64,
}

/// 协议引擎：决策 + 账 + 路径回放核。
pub struct IstProtocol {
    /// TSS.rsp0 配置。
    rsp0: u64,
    ledger: Vec<SwitchRow>,
    overflow: usize,
    seq: u64,
    /// 注入正确计数（10/10 目标）。
    correct: usize,
    wrong: usize,
}

impl IstProtocol {
    pub fn new(rsp0: u64) -> IstProtocol {
        IstProtocol {
            rsp0,
            ledger: Vec::new(),
            overflow: 0,
            seq: 0,
            correct: 0,
            wrong: 0,
        }
    }

    /// 注入一次中断事件：决策 → 不变量核 → 落账 → 正确性标记（期望路径显式传入）。
    pub fn fire(
        &mut self,
        vec: u8,
        cpl_from: u8,
        cpl_to: u8,
        gate_ist: u8,
        expect_path: u8,
    ) -> Result<SwitchPlan, u16> {
        let plan = decide(vec, cpl_from, cpl_to, gate_ist, self.rsp0)?;
        if !plan.invariants_ok() {
            return Err(XC_PUSH_MIX);
        }
        if plan.path != expect_path {
            self.wrong += 1;
            return Err(XC_PATH_UNK);
        }
        self.correct += 1;
        self.seq += 1;
        if self.ledger.len() < 128 {
            self.ledger.push(SwitchRow {
                vec,
                cpl_from,
                cpl_to,
                path: plan.path,
                seq: self.seq,
            });
        } else {
            self.overflow += 1;
        }
        Ok(plan)
    }

    /// 账回放核：账内 path 与 (vec, cpl_from, cpl_to) 重算一致（判据「走对路径」）。
    pub fn replay_verify(&self) -> Result<usize, u16> {
        let mut n = 0;
        for r in self.ledger.iter() {
            // 回放：按账行参数重决策（gate_ist 无法从账还原——账只存结果路径；
            // 重算用「路径标签自洽」：IST 行的 cpl 变更 push 语义一致核）。
            let consistent = match r.path {
                PATH_IST => true, // IST 与 CPL 无关
                PATH_RSP0 => r.cpl_from == 3 && r.cpl_to == 0,
                PATH_SAME => r.cpl_from == r.cpl_to,
                _ => false,
            };
            if !consistent {
                return Err(XC_PATH_UNK);
            }
            n += 1;
        }
        Ok(n)
    }

    /// 10/10 正确率核。
    pub fn ten_of_ten(&self, total: usize) -> bool {
        self.correct == total && self.wrong == 0
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.ledger.len() + self.overflow == self.seq as usize
    }
}

/// 栈顶对齐核：16B 对齐检查（切换后新栈顶建模）。
pub fn stack_top_aligned(top: u64) -> Result<(), u16> {
    if top % 16 != 0 {
        return Err(XC_ALIGN_BAD);
    }
    Ok(())
}

/// drill：全链自例程（决策树三叶 10/10 + DF 强制 IST + 不变量 + 错误路径）。
pub fn drill() -> Result<u32, u16> {
    let rsp0 = 0xFEE2_0000u64;
    let mut p = IstProtocol::new(rsp0);
    // 1. 路径③ Same：内核态中断（CPL0→0，无 IST）×4
    for _ in 0..4 {
        p.fire(50, 0, 0, 0, PATH_SAME)?;
    }
    // 2. 路径② rsp0：用户态中断（CPL3→0，无 IST）×3
    for _ in 0..3 {
        p.fire(60, 3, 0, 0, PATH_RSP0)?;
    }
    // 3. 路径① IST：IST 向量（CPL 任意）×2
    p.fire(70, 0, 0, 4, PATH_IST)?;
    p.fire(71, 3, 0, 5, PATH_IST)?;
    // 4. DF 强制 IST1
    p.fire(VEC_DF, 0, 0, 1, PATH_IST)?;
    // 5. 10/10 核
    if !p.ten_of_ten(10) {
        return Err(XC_PATH_UNK);
    }
    // 6. 账回放
    if p.replay_verify()? != 10 {
        return Err(XC_PATH_UNK);
    }
    // 7. 守恒
    if !p.conserve() {
        return Err(XC_LED_FULL);
    }
    // 8. 错误路径①：DF 缺 IST1
    if decide(VEC_DF, 0, 0, 0, rsp0).is_ok() {
        return Err(XC_DF_IST);
    }
    // 9. 错误路径②：rsp0 未配置走 rsp0 路径
    if decide(60, 3, 0, 0, 0).is_ok() {
        return Err(XC_RSP0_ZERO);
    }
    // 10. 错误路径③：CPL 非法
    if decide(50, 4, 0, 0, rsp0).is_ok() {
        return Err(XC_CPL_BAD);
    }
    // 11. 错误路径④：IST 越界
    if decide(50, 0, 0, 8, rsp0).is_ok() {
        return Err(XC_IST_RANGE);
    }
    // 12. Same 路径不压栈（不变量）
    let plan = decide(50, 0, 0, 0, rsp0)?;
    if plan.push_ssrsp || !plan.invariants_ok() {
        return Err(XC_PUSH_MIX);
    }
    // 13. 对齐核
    if stack_top_aligned(0xFEE2_0008).is_ok() {
        return Err(XC_ALIGN_BAD);
    }
    stack_top_aligned(0xFEE2_0010)?;
    Ok(0)
}
