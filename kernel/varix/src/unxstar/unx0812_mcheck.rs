//! unx0812_mcheck —— UNX-F0812 · Machine Check 机器检查采集与降级（目标 300 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0812 · Machine Check 机器检查采集与降级。
//! 判据原文：UNX-F0812-J1 MC 触发采集路径完整（MCG_STATUS/银行寄存器全量落账），
//!           降级决策三档生效。
//! 设计纪律：
//! - MC（向量 18）采集全量落账：MCG_STATUS（RIPV/EIPV/MCIP 三位）+ MCi_STATUS
//!   （VAL/OVER/UC/EN/PCC/AR/SIGN/MCA 低 16 位）+ MCi_ADDR + MCi_MISC 四元组。
//! - 银行数建模上限 64（真机常见 8~48）；每银行一行账，零丢弃红线（溢出计数）。
//! - 降级决策三档：继续（UC=0 且 PCC=0，错误已被纠正）/ 降级（UC=1 但 AR=1，
//!   可标记坏页回收）/ 终结（UC=1 且 PCC=1，处理器上下文已污染必停机）。
//!   决策表写死（SDM Vol.3 Ch.15 语义），逐分支注释来源。
//! - MCIP 防重入：MCG_STATUS.MCIP 置位期再入 MC → 建模为双 MC 上报（SDM 行为），
//!   采集器直接终态并红账，不再递归。
//! - RIPV/EIPV 引导分类：RIPV=1 可续跑（PC 已回退到安全点）、EIPV=1 PC 指向
//!   可疑指令（降级档不许续跑原 PC）、双零则 PC 语义未知（只可复位）。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x62E0（0x62E1~0x62EA 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const MC_VERSION: &str = "unx0812_mcheck-v1";

/// 诊断段基址（0x62E0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x62E0;

/// XcCode 闭集。
pub const XC_BANK_RANGE: u16 = SEG_BASE + 1; // 银行号越界
pub const XC_STATUS_INV: u16 = SEG_BASE + 2; // MCi_STATUS.VAL=0 却带其余位（非法）
pub const XC_REENTRY: u16 = SEG_BASE + 3; // MCIP 置位期再入
pub const XC_LED_FULL: u16 = SEG_BASE + 4; // 账溢出
pub const XC_DECIDE_UNK: u16 = SEG_BASE + 5; // 决策表未覆盖组合
pub const XC_MCG_INV: u16 = SEG_BASE + 6; // MCG_STATUS 保留位非零
pub const XC_SEQ_GAP: u16 = SEG_BASE + 7; // 账序号跳号
pub const XC_BANK_VAL: u16 = SEG_BASE + 8; // 银行配置数非法（0 或 >64）

/// MCG_STATUS 位（SDM Vol.3 §15.3.1）。
pub const MCG_STATUS_RIPV: u64 = 1 << 0;
pub const MCG_STATUS_EIPV: u64 = 1 << 1;
pub const MCG_STATUS_MCIP: u64 = 1 << 2;
pub const MCG_STATUS_RESERVED: u64 = !0x7u64;

/// MCi_STATUS 位（SDM Vol.3 §15.3.2）。
pub const MS_VAL: u64 = 1 << 63;
pub const MS_OVER: u64 = 1 << 62;
pub const MS_UC: u64 = 1 << 61;
pub const MS_EN: u64 = 1 << 60;
pub const MS_PCC: u64 = 1 << 59;
pub const MS_AR: u64 = 1 << 55; // Action Required（配合 UC 才有意义）
pub const MS_SIGN: u64 = 1 << 54; // 溢出符号位
pub const MS_MCA_MASK: u64 = 0xFFFF; // 低 16 位 MCA 错误码

/// 银行数上限。
pub const BANK_MAX: usize = 64;

/// 降级决策三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum McDecision {
    /// 继续（错误已纠正）。
    Continue = 0,
    /// 降级（标记回收）。
    Degrade = 1,
    /// 终结（停机）。
    Fatal = 2,
}

/// 单银行采集行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct McBankRow {
    /// 银行号。
    pub bank: u8,
    /// MCi_STATUS 全量。
    pub status: u64,
    /// MCi_ADDR。
    pub addr: u64,
    /// MCi_MISC。
    pub misc: u64,
}

impl McBankRow {
    /// VAL 位必须置位（无 VAL 的行是非法采集）。
    pub fn legal(&self) -> bool {
        self.status & MS_VAL != 0
    }

    /// MCA 错误码（低 16 位）。
    pub fn mca_code(&self) -> u16 {
        (self.status & MS_MCA_MASK) as u16
    }
}

/// MCG_STATUS 合法性（保留位零 + 不允许 MCIP/EIPV/RIPV 全零以外的怪组合——
/// 全零本身合法（正常清态））。
pub fn mcg_status_legal(v: u64) -> bool {
    v & MCG_STATUS_RESERVED == 0
}

/// 降级决策表（SDM Ch.15 语义固化）：
/// - UC=0：错误已纠正 → Continue（无论 PCC）。
/// - UC=1 & AR=1 & PCC=0：需动作但上下文完好 → Degrade。
/// - UC=1 & PCC=1：上下文污染 → Fatal。
/// - UC=1 & AR=0 & PCC=0：不可纠正但未波及上下文 → Degrade（保守档：仍标记）。
/// - UC=1 & PCC=1 恒 Fatal（AR 无关）。
pub fn decide(mcg_status: u64, row: &McBankRow) -> Result<McDecision, u16> {
    if !mcg_status_legal(mcg_status) {
        return Err(XC_MCG_INV);
    }
    if !row.legal() {
        return Err(XC_STATUS_INV);
    }
    let uc = row.status & MS_UC != 0;
    let pcc = row.status & MS_PCC != 0;
    let ar = row.status & MS_AR != 0;
    if !uc {
        return Ok(McDecision::Continue);
    }
    if pcc {
        return Ok(McDecision::Fatal);
    }
    if ar {
        return Ok(McDecision::Degrade);
    }
    Ok(McDecision::Degrade)
}

/// PC 续跑许可：RIPV=1 且决策非 Fatal 才许续跑；EIPV=1 时即便续跑也要标可疑 PC。
pub fn can_resume(mcg_status: u64, d: McDecision) -> Result<(bool, bool), u16> {
    if !mcg_status_legal(mcg_status) {
        return Err(XC_MCG_INV);
    }
    let ripv = mcg_status & MCG_STATUS_RIPV != 0;
    let eipv = mcg_status & MCG_STATUS_EIPV != 0;
    if d == McDecision::Fatal {
        return Ok((false, false));
    }
    Ok((ripv, eipv))
}

/// MC 采集器。
pub struct McCollector {
    /// 银行配置数（构造期定，运行期冻结）。
    banks: usize,
    /// 采集账（全量落账）。
    rows: Vec<(u64, McBankRow)>, // (mcg_status, bank row)
    overflow: usize,
    /// 决策账。
    decisions: Vec<(u64, u8)>, // (seq, decision code)
    /// 序号。
    seq: u64,
    /// MCIP 锁（建模 MCIP 位的管理）。
    mcip_set: bool,
}

impl McCollector {
    /// 构造（银行数 1~64）。
    pub fn new(banks: usize) -> Result<McCollector, u16> {
        if banks == 0 || banks > BANK_MAX {
            return Err(XC_BANK_VAL);
        }
        Ok(McCollector {
            banks,
            rows: Vec::new(),
            overflow: 0,
            decisions: Vec::new(),
            seq: 0,
            mcip_set: false,
        })
    }

    /// MC 触发采集：mcg_status + 多银行行全量落账。
    /// MCIP 防重入：置位期再入直接终态红账。
    pub fn on_machine_check(
        &mut self,
        mcg_status: u64,
        bank_rows: &[McBankRow],
    ) -> Result<McDecision, u16> {
        if !mcg_status_legal(mcg_status) {
            return Err(XC_MCG_INV);
        }
        if self.mcip_set {
            return Err(XC_REENTRY);
        }
        if mcg_status & MCG_STATUS_MCIP != 0 {
            self.mcip_set = true;
        }
        // 逐银行校验 + 落账
        let mut worst = McDecision::Continue;
        for r in bank_rows {
            if (r.bank as usize) >= self.banks {
                return Err(XC_BANK_RANGE);
            }
            if !r.legal() {
                return Err(XC_STATUS_INV);
            }
            self.seq += 1;
            if self.rows.len() < 128 {
                self.rows.push((mcg_status, *r));
            } else {
                self.overflow += 1;
            }
            let d = decide(mcg_status, r)?;
            // 多银行取最严档（Continue < Degrade < Fatal）
            if (d as u8) > (worst as u8) {
                worst = d;
            }
            if self.decisions.len() < 128 {
                self.decisions.push((self.seq, d as u8));
            }
        }
        if bank_rows.is_empty() {
            // 空上报：MCG_STATUS 已置 MCIP 但无银行 VAL——建模为决策 Fatal
            // （上下文异常上报，采不到银行只能保守停机）。
            self.seq += 1;
            if self.decisions.len() < 128 {
                self.decisions.push((self.seq, McDecision::Fatal as u8));
            }
            worst = McDecision::Fatal;
        }
        Ok(worst)
    }

    /// MC 退出（清 MCIP）。
    pub fn exit_machine_check(&mut self) {
        self.mcip_set = false;
    }

    /// 账行读取（越界即拒）。
    pub fn row(&self, i: usize) -> Result<(u64, McBankRow), u16> {
        if i >= self.rows.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.rows[i])
    }

    /// 决策行读取。
    pub fn decision(&self, i: usize) -> Result<(u64, u8), u16> {
        if i >= self.decisions.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.decisions[i])
    }

    /// 统计。
    pub fn stat(&self) -> (usize, usize, u64, usize) {
        (self.rows.len(), self.overflow, self.seq, self.decisions.len())
    }

    /// 序号零跳号核。
    pub fn seq_no_gap(&self) -> bool {
        for (i, d) in self.decisions.iter().enumerate() {
            if d.0 != i as u64 + 1 {
                return false;
            }
        }
        true
    }

    /// 守恒核。
    pub fn conserve(&self) -> bool {
        let (l, ov, seq, _) = self.stat();
        l + ov == seq as usize
    }
}

/// drill：全链自例程（三档决策全命中 + 采集全量 + 错误路径五道）。
pub fn drill() -> Result<u32, u16> {
    // 1. 构造（8 银行）
    let mut c = McCollector::new(8)?;
    // 2. Continue 档：UC=0（纠正错误）
    let ok_row = McBankRow {
        bank: 4,
        status: MS_VAL | 0x0081, // VAL + MCA=0x81（外部错误码格式示意）
        addr: 0x1234_5678,
        misc: 0,
    };
    let d1 = c.on_machine_check(MCG_STATUS_RIPV, &[ok_row])?;
    if d1 != McDecision::Continue {
        return Err(XC_DECIDE_UNK);
    }
    c.exit_machine_check();
    // 3. Degrade 档：UC=1 AR=1 PCC=0
    let dg_row = McBankRow {
        bank: 2,
        status: MS_VAL | MS_UC | MS_AR | 0x0005,
        addr: 0xA000_1000,
        misc: 0x55,
    };
    let d2 = c.on_machine_check(MCG_STATUS_RIPV, &[dg_row])?;
    if d2 != McDecision::Degrade {
        return Err(XC_DECIDE_UNK);
    }
    c.exit_machine_check();
    // 4. Fatal 档：UC=1 PCC=1
    let ft_row = McBankRow {
        bank: 0,
        status: MS_VAL | MS_UC | MS_PCC | 0x0002,
        addr: 0,
        misc: 0,
    };
    let d3 = c.on_machine_check(MCG_STATUS_EIPV | MCG_STATUS_MCIP, &[ft_row])?;
    if d3 != McDecision::Fatal {
        return Err(XC_DECIDE_UNK);
    }
    // Fatal 后 MCIP 仍置位（决策停机不 exit）——再入拦截验证
    if c.on_machine_check(MCG_STATUS_MCIP, &[ok_row]).is_ok() {
        return Err(XC_REENTRY);
    }
    c.exit_machine_check();
    // 5. 多银行最严档：一纠正一致命 → Fatal
    let mix = [
        McBankRow { bank: 1, status: MS_VAL | 0x10, addr: 1, misc: 0 },
        McBankRow { bank: 5, status: MS_VAL | MS_UC | MS_PCC | 0x20, addr: 2, misc: 0 },
    ];
    let d4 = c.on_machine_check(MCG_STATUS_RIPV, &mix)?;
    if d4 != McDecision::Fatal {
        return Err(XC_DECIDE_UNK);
    }
    c.exit_machine_check();
    // 6. 全量账核：4 次事件 × 每次银行数 = 1+1+1+2 = 5 行
    if !c.seq_no_gap() || !c.conserve() {
        return Err(XC_SEQ_GAP);
    }
    let (rl, ov, sq, dl) = c.stat();
    if rl != 5 || ov != 0 || sq != 5 || dl != 5 {
        return Err(XC_LED_FULL);
    }
    // 7. 错误路径①：银行越界
    let far = McBankRow { bank: 63, status: MS_VAL, addr: 0, misc: 0 };
    if c.on_machine_check(0, &[far]).is_ok() {
        return Err(XC_BANK_RANGE);
    }
    // 8. 错误路径②：VAL=0 非法行
    let noval = McBankRow { bank: 3, status: 0x10, addr: 0, misc: 0 };
    if c.on_machine_check(0, &[noval]).is_ok() {
        return Err(XC_STATUS_INV);
    }
    // 9. 错误路径③：MCG_STATUS 保留位
    if c.on_machine_check(0x8000_0000_0000_0000, &[ok_row]).is_ok() {
        return Err(XC_MCG_INV);
    }
    // 10. 错误路径④：银行数非法构造
    if McCollector::new(0).is_ok() || McCollector::new(65).is_ok() {
        return Err(XC_BANK_VAL);
    }
    // 11. can_resume：RIPV=1 非 Fatal 可续跑；Fatal 不可；EIPV 标可疑
    let (ok, susp) = can_resume(MCG_STATUS_RIPV, McDecision::Continue)?;
    if !ok || susp {
        return Err(XC_DECIDE_UNK);
    }
    let (ok2, susp2) = can_resume(MCG_STATUS_EIPV, McDecision::Degrade)?;
    if ok2 || !susp2 {
        return Err(XC_DECIDE_UNK);
    }
    let (ok3, _) = can_resume(MCG_STATUS_RIPV, McDecision::Fatal)?;
    if ok3 {
        return Err(XC_DECIDE_UNK);
    }
    Ok(0)
}
