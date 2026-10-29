//! unx4155_errpass — UNX-F4155 错误码透传契约（fs 错误→errno 不改写）（诊断段 0x9520）。
//!
//! 判据原文：
//!   J1 fs 层返回的 20 类错误码经 VFS 透传后数值零改写（逐类断言）。
//!   J2 非法错误码注入被规范化为 EIO 并留账。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 合法错误码闭集 20 类（POSIX errno 标准值），透传层只做闭集
//!      判定，不做任何数值改写、映射或包装。
//!   2. 零改写断言：透传出参 == 入参逐类核对，差一即契约失守。
//!   3. 非法码注入（0、超闭集值）一律规范化为 EIO(5) 并逐次留账，
//!      规范化次数与留账条数守恒。
//!   4. 透传状态机：Idle → Passing → Passed/EioNorm，一码一账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9520;

// ── 诊断码闭集（透传层自身诊断）─────────────────────────────
const E_ILLEGAL_CODE: u16 = SEG_BASE + 1; // 非法错误码注入（已规范化 EIO）
const E_REWRITE: u16 = SEG_BASE + 2;      // 透传零改写失守
const E_NORM_BAL: u16 = SEG_BASE + 3;     // 规范化账与留账不平
const E_BAD_STATE: u16 = SEG_BASE + 4;    // 透传状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 5;    // 写回摘要为空
const E_PASS_MISS: u16 = SEG_BASE + 6;    // 20 类透传断言缺类

/// POSIX errno 标准值（Linux 口径），20 类合法闭集。
pub const EPERM: u16 = 1;   // 操作不允许
pub const ENOENT: u16 = 2;  // 无此文件或目录
pub const ESRCH: u16 = 3;   // 无此进程
pub const EINTR: u16 = 4;   // 系统调用被中断
pub const EIO: u16 = 5;     // I/O 错误（非法码规范化的目标值）
pub const ENXIO: u16 = 6;   // 无此设备或地址
pub const E2BIG: u16 = 7;   // 参数表过长
pub const ENOEXEC: u16 = 8; // 可执行文件格式错误
pub const EBADF: u16 = 9;   // 坏文件描述符
pub const ECHILD: u16 = 10; // 无子进程
pub const EAGAIN: u16 = 11; // 资源暂不可用
pub const ENOMEM: u16 = 12; // 内存不足
pub const EACCES: u16 = 13; // 权限不足
pub const EFAULT: u16 = 14; // 坏地址
pub const EBUSY: u16 = 16;  // 设备或资源忙
pub const EEXIST: u16 = 17; // 文件已存在
pub const EXDEV: u16 = 18;  // 跨设备链接
pub const ENODEV: u16 = 19; // 无此设备
pub const ENOTDIR: u16 = 20;// 非目录
pub const EISDIR: u16 = 21; // 是目录

/// 20 类合法闭集表（常量写死，来源：POSIX/Linux errno 编号）。
pub const LEGAL_CODES: [u16; 20] = [
    EPERM, ENOENT, ESRCH, EINTR, EIO, ENXIO, E2BIG, ENOEXEC, EBADF,
    ECHILD, EAGAIN, ENOMEM, EACCES, EFAULT, EBUSY, EEXIST, EXDEV,
    ENODEV, ENOTDIR, EISDIR,
];

/// 透传状态机：Idle 待命 / Passing 透传中 / Passed 零改写出 / EioNorm 已规范化。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PassState {
    Idle,
    Passing,
    Passed,
    EioNorm,
}

/// 一条透传账：入码 / 出码 / 是否规范化。
pub struct PassRecord {
    pub code_in: u16,   // fs 层原始返回
    pub code_out: u16,  // VFS 透传出参
    pub normalized: bool, // 是否走了 EIO 规范化
}

/// 透传契约枢纽：闭集判定 + 零改写 + 规范化留账。
pub struct ErrPass {
    pub state: PassState,
    ledger: Vec<PassRecord>,
    pub passed_legal: u32,   // 合法码透传计数
    pub normalized_total: u32, // 非法码规范化计数
}

impl ErrPass {
    pub fn new() -> ErrPass {
        ErrPass {
            state: PassState::Idle,
            ledger: Vec::new(),
            passed_legal: 0,
            normalized_total: 0,
        }
    }

    fn is_legal(code: u16) -> bool {
        let mut i = 0usize;
        while i < LEGAL_CODES.len() {
            if LEGAL_CODES[i] == code {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 透传入口：合法码原样出（零改写），非法码规范化 EIO 并留账。
    pub fn pass(&mut self, code: u16) -> Result<u16, u16> {
        self.state = PassState::Passing;
        if Self::is_legal(code) {
            self.ledger.push(PassRecord {
                code_in: code,
                code_out: code, // 零改写
                normalized: false,
            });
            self.passed_legal += 1;
            self.state = PassState::Passed;
            Ok(code)
        } else {
            // 非法码注入：规范化为 EIO 并留账（判据 J2）。
            self.ledger.push(PassRecord {
                code_in: code,
                code_out: EIO,
                normalized: true,
            });
            self.normalized_total += 1;
            self.state = PassState::EioNorm;
            Err(E_ILLEGAL_CODE)
        }
    }

    /// 零改写逐类断言：20 类逐一透传并核对出参==入参（判据 J1）。
    pub fn assert_all_legal(&mut self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < LEGAL_CODES.len() {
            let c = LEGAL_CODES[i];
            let out = self.pass(c)?; // 合法码必须 Ok
            if out != c {
                return Err(E_REWRITE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒对账：合法账 + 规范化账 = 留账总条数。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ledger.len() as u32 != self.passed_legal + self.normalized_total {
            return Err(E_NORM_BAL);
        }
        for r in &self.ledger {
            if r.normalized && r.code_out != EIO {
                return Err(E_REWRITE);
            }
            if !r.normalized && r.code_out != r.code_in {
                return Err(E_REWRITE);
            }
        }
        Ok(())
    }

    /// 20 类覆盖核验：合法码账必须覆盖闭集全 20 类（判据 J1 无缺类）。
    pub fn coverage_check(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < LEGAL_CODES.len() {
            let want = LEGAL_CODES[i];
            let mut hit = false;
            for r in &self.ledger {
                if !r.normalized && r.code_in == want {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_PASS_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
}

/// 自例程：R1 20 类零改写 → R2 边界 → R3 非法码注入规范化 → R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：20 类逐一透传零改写 + 覆盖核验。
    let mut p = ErrPass::new();
    p.assert_all_legal()?;
    p.coverage_check()?;
    if p.passed_legal != 20 {
        return Err(E_PASS_MISS);
    }
    p.writeback(true, 0x40)?;
    hits += 1;

    // R2 边界：码 0 与码 0xFFFF 均非法（闭集外），各自规范化。
    let mut p = ErrPass::new();
    if p.pass(0).is_ok() {
        return Err(E_ILLEGAL_CODE);
    }
    if p.pass(0xFFFF).is_ok() {
        return Err(E_ILLEGAL_CODE);
    }
    if p.normalized_total != 2 {
        return Err(E_NORM_BAL);
    }
    hits += 1;

    // R3 注入拦截：闭集缝隙值（如 15、22）注入必拦并规范化 EIO。
    let mut p = ErrPass::new();
    if p.pass(15).is_ok() {
        return Err(E_ILLEGAL_CODE);
    }
    if p.pass(22).is_ok() {
        return Err(E_ILLEGAL_CODE);
    }
    p.audit()?;
    let ok_out = p.pass(EIO)?; // EIO 本身在闭集内，合法透传
    if ok_out != EIO {
        return Err(E_REWRITE);
    }
    hits += 1;

    // R4 守恒对账：合法+规范化=总账，逐条回放核对。
    let mut p = ErrPass::new();
    let mut i = 0usize;
    while i < LEGAL_CODES.len() {
        p.pass(LEGAL_CODES[i])?;
        i += 1;
    }
    p.pass(0xFFFF).is_err();
    p.audit()?;
    if p.ledger_len() != 21 {
        return Err(E_NORM_BAL);
    }
    hits += 1;

    // R5 追加回归：倒序 20 类透传 + 非法码三条注入 + 覆盖复核。
    let mut p = ErrPass::new();
    let mut i = LEGAL_CODES.len();
    while i > 0 {
        i -= 1;
        let c = LEGAL_CODES[i];
        if p.pass(c)? != c {
            return Err(E_REWRITE);
        }
    }
    p.pass(0).is_err();
    p.pass(15).is_err();
    p.pass(22).is_err();
    if p.normalized_total != 3 {
        return Err(E_NORM_BAL);
    }
    p.coverage_check()?;
    p.audit()?;
    if p.ledger_len() != 23 {
        return Err(E_NORM_BAL);
    }
    p.writeback(true, 1)?;
    hits += 1;

    // R6 回归：重复合法码透传不查重（透传层无状态红线）。
    let mut p2 = ErrPass::new();
    p2.pass(EACCES)?;
    p2.pass(EACCES)?;
    if p2.passed_legal != 2 {
        return Err(E_NORM_BAL);
    }
    if p2.pass(EACCES)? != EACCES {
        return Err(E_REWRITE);
    }
    p2.pass(0xFFFF).is_err();
    p2.audit()?;
    p2.writeback(true, 0xFFFF)?;
    hits += 1;

    // R7 回归：闭集缝隙定位（14 与 16 之间缺 15，21 之后 22 起非法）。
    let mut p3 = ErrPass::new();
    if p3.pass(14).is_err() {
        return Err(E_REWRITE); // 14 在闭集内
    }
    if p3.pass(15).is_ok() {
        return Err(E_ILLEGAL_CODE);
    }
    if p3.pass(16).is_err() {
        return Err(E_REWRITE); // 16 在闭集内
    }
    if p3.pass(21).is_err() {
        return Err(E_REWRITE); // 21 在闭集内
    }
    if p3.pass(22).is_ok() {
        return Err(E_ILLEGAL_CODE);
    }
    p3.audit()?;
    hits += 1;

    Ok(hits)
}
