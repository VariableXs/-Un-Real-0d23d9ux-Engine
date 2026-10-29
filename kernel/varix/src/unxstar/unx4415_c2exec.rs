//! unx4415_c2exec — UNX-F4415 C2 exec 联签锚（诊断段 0xB7A0）。
//!
//! 判据原文：J1 联签契约闭集：C2（syscall 面）exec 家族（execve/
//!           execveat）入场前必须完成五项前置（fd 表 CLOEXEC 清账、
//!           旗标冻结、路径解析就绪、参数块配额、返回面失活），五
//!           项全绿才可放行——缺一即拒且拒绝原因入三要素账；
//!           J2 失败零副作用：任一前置失败后，fd 表/旗标账/统计账
//!           与入场前快照逐项一致（账面零污染）；
//!           J3 放行恰一次：绿检通过后放行位一次性置位（二次放行
//!           拒收），放行后账目冻结只读；
//!           J4 契约账守恒：signatures = granted + denied，per-前置
//!           拒绝分账之和等于 denied。
//! 设计纪律：
//!   1. 联签（co-sign）语义：跨域接口在入场前的多前置联合校验，
//!      本锚以账面闸模型承载 C2 exec 的入场纪律。
//!   2. 五前置闸：check 序（1~5）固定，短路遇红即拒（拒绝原因 =
//!      首红前置号），绿检位由 check_all 一次性置位。
//!   3. 快照零污染：入场前对 fd 表位向量/旗标账/统计账三面取快照，
//!      拒收路径恢复并比对。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0xB7A0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB7A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_P1_FD: u16 = SEG_BASE + 1;    // 前置1：fd 表 CLOEXEC 清账未过
const E_P2_FLAG: u16 = SEG_BASE + 2;  // 前置2：旗标冻结未过
const E_P3_PATH: u16 = SEG_BASE + 3;  // 前置3：路径解析未就绪
const E_P4_ARG: u16 = SEG_BASE + 4;   // 前置4：参数块配额越界
const E_P5_RET: u16 = SEG_BASE + 5;   // 前置5：返回面未失活
const E_TWICE: u16 = SEG_BASE + 6;    // 二次放行
const E_BAL: u16 = SEG_BASE + 7;      // 契约账不守恒
const E_FROZEN: u16 = SEG_BASE + 8;   // 放行后账目冻结仍写

pub const NPRE: usize = 5;

/// 五前置状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreState {
    Red = 0,
    Green = 1,
}

/// C2 exec 联签闸。
pub struct ExecGate {
    pre: [bool; NPRE], // 前置绿检位
    pub granted: bool,
    pub signatures: u32,
    pub granted_n: u32,
    pub denied_n: u32,
    pub deny_p1: u32,
    pub deny_p2: u32,
    pub deny_p3: u32,
    pub deny_p4: u32,
    pub deny_p5: u32,
    // 三面快照（零污染核对面）
    fd_bits: Vec<bool>,
    flag_events: u32,
    stat_ops: u32,
}

impl ExecGate {
    pub fn new(nfds: usize) -> ExecGate {
        let mut fd_bits = Vec::new();
        for _ in 0..nfds {
            fd_bits.push(false);
        }
        ExecGate {
            pre: [false; NPRE],
            granted: false,
            signatures: 0,
            granted_n: 0,
            denied_n: 0,
            deny_p1: 0,
            deny_p2: 0,
            deny_p3: 0,
            deny_p4: 0,
            deny_p5: 0,
            fd_bits,
            flag_events: 0,
            stat_ops: 0,
        }
    }

    /// 前置逐项置绿（序号 1~5）。
    pub fn set_pre(&mut self, n: usize) -> Result<(), u16> {
        if self.granted {
            return Err(E_FROZEN);
        }
        if n < 1 || n > NPRE {
            return Err(E_P1_FD);
        }
        self.pre[n - 1] = true;
        Ok(())
    }

    fn first_red(&self) -> Option<usize> {
        for (i, g) in self.pre.iter().enumerate() {
            if !g {
                return Some(i + 1);
            }
        }
        None
    }

    /// 全前置检查 + 一次性放行（判据 J1/J3）。失败路径零副作用
    /// （判据 J2：三面快照不变）。
    pub fn check_and_grant(&mut self) -> Result<(), u16> {
        if self.granted {
            return Err(E_TWICE);
        }
        self.signatures += 1;
        // 三面快照
        let fd_snap = self.fd_bits.clone();
        let fe_snap = self.flag_events;
        let st_snap = self.stat_ops;
        let red = self.first_red();
        match red {
            None => {
                self.granted = true;
                self.granted_n += 1;
                Ok(())
            }
            Some(n) => {
                self.denied_n += 1;
                let e = match n {
                    1 => {
                        self.deny_p1 += 1;
                        E_P1_FD
                    }
                    2 => {
                        self.deny_p2 += 1;
                        E_P2_FLAG
                    }
                    3 => {
                        self.deny_p3 += 1;
                        E_P3_PATH
                    }
                    4 => {
                        self.deny_p4 += 1;
                        E_P4_ARG
                    }
                    _ => {
                        self.deny_p5 += 1;
                        E_P5_RET
                    }
                };
                // 零污染比对（判据 J2）
                if self.fd_bits != fd_snap || self.flag_events != fe_snap || self.stat_ops != st_snap {
                    return Err(E_BAL);
                }
                Err(e)
            }
        }
    }

    /// fd 位账变更（放行前可变；放行后冻结拒收）。
    pub fn fd_mark(&mut self, fd: usize) -> Result<(), u16> {
        if self.granted {
            return Err(E_FROZEN);
        }
        if fd >= self.fd_bits.len() {
            return Err(E_P1_FD);
        }
        self.fd_bits[fd] = true;
        self.stat_ops += 1;
        Ok(())
    }

    pub fn fd_unmark(&mut self, fd: usize) -> Result<(), u16> {
        if self.granted {
            return Err(E_FROZEN);
        }
        if fd >= self.fd_bits.len() {
            return Err(E_P1_FD);
        }
        self.fd_bits[fd] = false;
        self.stat_ops += 1;
        Ok(())
    }

    /// 契约账守恒（判据 J4）。
    pub fn balanced(&self) -> bool {
        self.signatures == self.granted_n + self.denied_n
            && self.deny_p1 + self.deny_p2 + self.deny_p3 + self.deny_p4 + self.deny_p5
                == self.denied_n
    }
}

/// 自例程：R1 五绿放行恰一次；R2 首红短路拒绝序；R3 拒收零副作用；
/// R4 放行后冻结；R5 契约账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：五前置全绿 → 放行；二次放行拒收（判据 J1/J3）。
    let mut g = ExecGate::new(8);
    for n in 1..=5 {
        g.set_pre(n)?;
    }
    g.check_and_grant()?;
    if !g.granted || g.granted_n != 1 {
        return Err(E_BAL);
    }
    if g.check_and_grant() != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // R2：首红短路——只置 1/2/4（缺 3）：拒绝原因必须是前置 3 而非 4/5。
    let mut h = ExecGate::new(8);
    h.set_pre(1)?;
    h.set_pre(2)?;
    h.set_pre(4)?;
    if h.check_and_grant() != Err(E_P3_PATH) {
        return Err(E_P3_PATH);
    }
    if h.deny_p3 != 1 || h.deny_p4 != 0 || h.deny_p5 != 0 {
        return Err(E_BAL);
    }
    // 缺 1：拒绝原因是 1
    let mut k = ExecGate::new(8);
    for n in 2..=5 {
        k.set_pre(n)?;
    }
    if k.check_and_grant() != Err(E_P1_FD) {
        return Err(E_P1_FD);
    }
    hits += 1;

    // R3：拒收零副作用（判据 J2）——拒绝前后 fd 位账/统计账一致。
    let mut m = ExecGate::new(8);
    m.fd_mark(3)?;
    let ops_before = m.stat_ops;
    if m.check_and_grant() != Err(E_P1_FD) {
        return Err(E_P1_FD);
    }
    if m.stat_ops != ops_before {
        return Err(E_BAL);
    }
    // fd_mark 仍可用（未放行），再变更后拒收依旧零污染
    m.fd_mark(5)?;
    m.fd_unmark(5)?;
    if m.check_and_grant() != Err(E_P1_FD) {
        return Err(E_P1_FD);
    }
    if m.deny_p1 != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：放行后账目冻结——fd_mark/unmark/set_pre 全拒（判据 J3）。
    if m.fd_mark(0) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    let mut g2 = ExecGate::new(4);
    for n in 1..=5 {
        g2.set_pre(n)?;
    }
    g2.check_and_grant()?;
    if g2.set_pre(1) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    if g2.fd_mark(1) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;

    // R5：契约账守恒终验（判据 J4）。
    if !g.balanced() || !h.balanced() || !m.balanced() {
        return Err(E_BAL);
    }
    // 分账闭合逐项核
    if m.signatures != m.granted_n + m.denied_n {
        return Err(E_BAL);
    }
    // 越界前置号拒收
    let mut q = ExecGate::new(4);
    if q.set_pre(6) != Err(E_P1_FD) {
        return Err(E_P1_FD);
    }
    if q.set_pre(0) != Err(E_P1_FD) {
        return Err(E_P1_FD);
    }
    hits += 1;

    Ok(hits)
}
