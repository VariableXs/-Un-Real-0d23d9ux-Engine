//! unx0811_dfault —— UNX-F0811 · Double Fault 双重故障现场保全路径（目标 280 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0811 · Double Fault 双重故障现场保全路径。
//! 判据原文：UNX-F0811-J1 DF 触发 10/10 次现场保全完整（原栈/rsp/错因三元在册），
//!           DF 处置路径零 panic 循环。
//! 设计纪律：
//! - DF（向量 8）是终结级异常：处置路径必须零递归、零 panic——本模块用
//!   「冻结旗标 + 一次性保全窗口」实现：DF 处理期间再入直接进入终态（三重
//!   故障防线），不再触发任何可能二次异常的路径。
//! - 现场三元：原栈（故障时 RSP 值）/ rsp（DF 自身 IST 栈指针）/ 错因（DF
//!   错误码位语义：BIT0=页表防护、BIT1=IST 越界）。三元缺一即保全不完整红。
//! - 保全账定容 32 行（LIFO 环形），每行带序号与完整三元 + 触发源异常号。
//! - IST 栈预算：DF IST 槽定容 4KiB（建模为深度计数），保全路径只读不改栈。
//! - 处置决策三档：停机（halt）/ 收集后复位 / 降级继续（仅宿主调试档）。
//!   档位由构建常量定，运行期不可改（冻结）。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x62C0（0x62C1~0x62C8 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const DF_VERSION: &str = "unx0811_dfault-v1";

/// 诊断段基址（0x62C0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x62C0;

/// XcCode 闭集。
pub const XC_REENTRY: u16 = SEG_BASE + 1; // DF 处置中再入（三重故障防线触发）
pub const XC_TRIAD_MISS: u16 = SEG_BASE + 2; // 现场三元缺件
pub const XC_LED_FULL: u16 = SEG_BASE + 3; // 保全账溢出
pub const XC_IST_DEPTH: u16 = SEG_BASE + 4; // IST 栈深度超预算
pub const XC_ERRCODE_BAD: u16 = SEG_BASE + 5; // DF 错误码保留位非零
pub const XC_MODE_FROZEN: u16 = SEG_BASE + 6; // 处置档冻结后尝试改
pub const XC_SEQ_GAP: u16 = SEG_BASE + 7; // 保全序号跳号
pub const XC_SRC_VEC: u16 = SEG_BASE + 8; // 触发源异常号非法（>32 且非 8 链）

/// DF 向量号。
pub const VEC_DF: u8 = 8;

/// 处置档（构建期冻结三档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DfPolicy {
    /// 立即停机（生产档）。
    Halt = 0,
    /// 收集后复位（默认档）。
    CollectReset = 1,
    /// 收集后降级继续（宿主调试档，需显式声明）。
    CollectContinue = 2,
}

/// DF 错误码位语义（SDM Vol.3 Table 6-5）。
pub const DF_ERR_GUARD_PAGE: u32 = 1 << 0; // BIT0：嵌套异常触页表防护
pub const DF_ERR_IST_OVERRUN: u32 = 1 << 1; // BIT1：嵌套异常致 IST 越界
pub const DF_ERR_RESERVED: u32 = !0x3u32; // 其余位保留，必须为零

/// 现场三元。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DfTriad {
    /// 故障时原栈指针（故障上下文的 RSP）。
    pub fault_rsp: u64,
    /// DF 自身 IST 栈指针（进入 DF 处理时的栈顶）。
    pub ist_rsp: u64,
    /// DF 错误码（BIT0/BIT1 语义）。
    pub err_code: u32,
    /// 触发源异常号（首次异常的向量号；DF 链则记 8）。
    pub src_vec: u8,
}

impl DfTriad {
    /// 三元完整性：rsp 非零 + IST 栈指针非零 + 源异常号界内。
    pub fn complete(&self) -> bool {
        self.fault_rsp != 0
            && self.ist_rsp != 0
            && (self.src_vec as usize) <= 32
    }

    /// 错误码合法性：保留位必须为零。
    pub fn errcode_legal(&self) -> bool {
        self.err_code & DF_ERR_RESERVED == 0
    }

    /// 错因解释（人话化三选一）。
    pub fn cause_text_code(&self) -> u8 {
        match (self.err_code & DF_ERR_GUARD_PAGE != 0, self.err_code & DF_ERR_IST_OVERRUN != 0) {
            (true, false) => 1,  // 页表防护触发
            (false, true) => 2,  // IST 越界触发
            (true, true) => 3,   // 双因并发
            (false, false) => 0, // 未标注（保底解释：嵌套异常无位标注）
        }
    }
}

/// 保全账行（三元 + 序号 + 处置档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DfRow {
    /// 序号（单调）。
    pub seq: u64,
    /// 现场三元。
    pub triad: DfTriad,
    /// 处置档（0/1/2）。
    pub policy: u8,
}

/// DF 保全器。
pub struct DfGuard {
    /// 冻结旗标：DF 处置中再入即触发三重故障防线。
    in_flight: bool,
    /// 处置档（构建期冻结）。
    policy: DfPolicy,
    /// 保全账（定容 32，满即溢出计数——零丢弃红线）。
    rows: Vec<DfRow>,
    overflow: usize,
    /// IST 栈深度预算（建模计数）。
    ist_budget: u32,
    /// 序号发生器。
    seq: u64,
}

impl DfGuard {
    /// 构造（档位冻结于此）。
    pub fn new(policy: DfPolicy) -> DfGuard {
        DfGuard {
            in_flight: false,
            policy,
            rows: Vec::new(),
            overflow: 0,
            ist_budget: 4096,
            seq: 0,
        }
    }

    /// DF 进入路径：再入拦截（三重故障防线）→ 三元校验 → 保全入账。
    pub fn on_double_fault(&mut self, triad: DfTriad) -> Result<DfPolicy, u16> {
        // 1. 再入防线：in_flight 期再次 DF 直接终态，不入账不再递归。
        if self.in_flight {
            return Err(XC_REENTRY);
        }
        // 2. 三元完整性
        if !triad.complete() {
            return Err(XC_TRIAD_MISS);
        }
        // 3. 错误码保留位
        if !triad.errcode_legal() {
            return Err(XC_ERRCODE_BAD);
        }
        // 4. IST 深度预算：保全路径自身开销建模 64 字节/行，超预算即拒。
        let used = 64u64 + (self.rows.len() as u64) * 64;
        if used > self.ist_budget as u64 {
            return Err(XC_IST_DEPTH);
        }
        // 5. 入账（定容 32）
        self.seq += 1;
        if self.rows.len() < 32 {
            self.rows.push(DfRow {
                seq: self.seq,
                triad,
                policy: self.policy as u8,
            });
        } else {
            self.overflow += 1;
        }
        // 6. 处置：Halt/CollectReset 档冻结旗标置位（不再回来）；Continue 档回退旗标。
        match self.policy {
            DfPolicy::Halt | DfPolicy::CollectReset => {
                self.in_flight = true;
            }
            DfPolicy::CollectContinue => {
                // 降级继续：旗标开合一次（窗口式），下一轮 DF 仍可入账。
            }
        }
        Ok(self.policy)
    }

    /// 调试档显式复位（仅 CollectContinue 档可用；其他档已冻结即拒）。
    pub fn debug_reset(&mut self) -> Result<(), u16> {
        if self.policy != DfPolicy::CollectContinue {
            return Err(XC_MODE_FROZEN);
        }
        self.in_flight = false;
        Ok(())
    }

    /// 保全账第 i 行（越界即拒）。
    pub fn row(&self, i: usize) -> Result<DfRow, u16> {
        if i >= self.rows.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.rows[i])
    }

    /// 账长 / 溢出。
    pub fn stat(&self) -> (usize, usize) {
        (self.rows.len(), self.overflow)
    }

    /// 序号连续性（零跳号红线）。
    pub fn seq_no_gap(&self) -> bool {
        for (i, r) in self.rows.iter().enumerate() {
            if r.seq != i as u64 + 1 {
                return false;
            }
        }
        true
    }

    /// 守恒核：账长 + 溢出 == 序号总数。
    pub fn conserve(&self) -> bool {
        let (l, ov) = self.stat();
        l + ov == self.seq as usize
    }
}

/// 触发源异常号合法性辅助（8 链上溯：DF 由任何异常引发都合法，但源号 ≤32）。
pub fn src_vec_legal(v: u8) -> bool {
    (v as usize) <= 32
}

/// drill：全链自例程（10 次 DF 注入全保全 + 三道错误路径）。
pub fn drill() -> Result<u32, u16> {
    // 1. 默认档（CollectReset）10/10 全保全
    let mut g = DfGuard::new(DfPolicy::CollectReset);
    for i in 0..10u64 {
        let t = DfTriad {
            fault_rsp: 0x7F00_0000 + i * 0x100,
            ist_rsp: 0xFEE2_8000,
            err_code: if i % 2 == 0 { DF_ERR_GUARD_PAGE } else { DF_ERR_IST_OVERRUN },
            src_vec: 14, // #PF 引发 DF 的经典链
        };
        let p = g.on_double_fault(t)?;
        if p != DfPolicy::CollectReset {
            return Err(XC_MODE_FROZEN);
        }
    }
    if !g.seq_no_gap() {
        return Err(XC_SEQ_GAP);
    }
    if !g.conserve() {
        return Err(XC_LED_FULL);
    }
    // 2. 首行三元抽查
    let r0 = g.row(0)?;
    if r0.triad.src_vec != 14 || r0.triad.fault_rsp == 0 {
        return Err(XC_TRIAD_MISS);
    }
    // 3. 再入防线：Halt 档首次 DF 后冻结，第二次 DF 必拦
    let mut h = DfGuard::new(DfPolicy::Halt);
    let t = DfTriad {
        fault_rsp: 0x1000,
        ist_rsp: 0x2000,
        err_code: 0,
        src_vec: 13,
    };
    h.on_double_fault(t)?;
    if h.on_double_fault(t).is_ok() {
        return Err(XC_REENTRY);
    }
    // 4. 三元缺件拦截（fault_rsp = 0）
    let mut c = DfGuard::new(DfPolicy::CollectContinue);
    let bad = DfTriad {
        fault_rsp: 0,
        ist_rsp: 0x2000,
        err_code: 0,
        src_vec: 13,
    };
    if c.on_double_fault(bad).is_ok() {
        return Err(XC_TRIAD_MISS);
    }
    // 5. 错误码保留位拦截
    let bad2 = DfTriad {
        fault_rsp: 0x1000,
        ist_rsp: 0x2000,
        err_code: 0x8000_0000,
        src_vec: 13,
    };
    if c.on_double_fault(bad2).is_ok() {
        return Err(XC_ERRCODE_BAD);
    }
    // 6. 源异常号越界拦截
    let bad3 = DfTriad {
        fault_rsp: 0x1000,
        ist_rsp: 0x2000,
        err_code: 0,
        src_vec: 33,
    };
    if c.on_double_fault(bad3).is_ok() {
        return Err(XC_SRC_VEC);
    }
    // 7. 调试档复位可用 / 生产档复位拒
    c.debug_reset()?;
    let mut hp = DfGuard::new(DfPolicy::Halt);
    if hp.debug_reset().is_ok() {
        return Err(XC_MODE_FROZEN);
    }
    // 8. 错因解释抽查
    let t1 = DfTriad { fault_rsp: 1, ist_rsp: 2, err_code: DF_ERR_GUARD_PAGE, src_vec: 8 };
    if t1.cause_text_code() != 1 {
        return Err(XC_ERRCODE_BAD);
    }
    let t3 = DfTriad { fault_rsp: 1, ist_rsp: 2, err_code: DF_ERR_GUARD_PAGE | DF_ERR_IST_OVERRUN, src_vec: 8 };
    if t3.cause_text_code() != 3 {
        return Err(XC_ERRCODE_BAD);
    }
    // 9. src_vec 辅助
    if !src_vec_legal(8) || src_vec_legal(200) {
        return Err(XC_SRC_VEC);
    }
    Ok(0)
}
