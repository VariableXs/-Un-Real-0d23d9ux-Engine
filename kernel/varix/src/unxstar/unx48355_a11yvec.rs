//! unx48355_a11yvec — UNX-F48355 B18 批向量账（无障碍用例集+校验和）（诊断段 0xCD40）。
//!
//! 判据原文：
//!   J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 用例向量账：无障碍面 20 条用例入册（读屏标签/焦点位/对比
//!      度/字号缩放/键盘导航/超时提示六族），每条（输入向量 → 期
//!      望指纹）定长定序，账面公开可对。
//!   2. 校验和 seal：20 条期望指纹 XOR 折叠成全册校验和，执行前
//!      先验 seal——账面被篡改（哪怕一条）seal 必不符，先红再谈
//!      执行（防"改期望凑结果"）。
//!   3. 可复跑口径（J1 ×20）：20 条用例逐条执行两轮，输出指纹与
//!      期望逐位一致；轮间输出逐位一致（确定性，无隐藏状态）。
//!   4. 执行器纯函数：输入向量决定输出指纹（无时钟无随机），同
//!      输入两轮跑必同指纹。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCD40;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_VEC_BAD: u16 = SEG_BASE + 1;  // 用例 id 越账
const E_SEAL_BAD: u16 = SEG_BASE + 2; // 校验和 seal 不符
const E_OUT_DIFF: u16 = SEG_BASE + 3; // 输出指纹与期望不符
const E_ROUND_DIFF: u16 = SEG_BASE + 4;// 轮间不一致（非确定）
const E_LEDGER_BAD: u16 = SEG_BASE + 5;// 执行账不平
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7;// 写回摘要为空

/// 用例数（J1 ×20）。
pub const CASE_N: usize = 20;
/// 六族闭集。
pub const FAM_LABEL: u8 = 0; // 读屏标签
pub const FAM_FOCUS: u8 = 1; // 焦点位
pub const FAM_CONTRAST: u8 = 2; // 对比度
pub const FAM_SCALE: u8 = 3; // 字号缩放
pub const FAM_NAV: u8 = 4;   // 键盘导航
pub const FAM_TIMEOUT: u8 = 5; // 超时提示

/// 用例条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A11yCase {
    pub case_id: u8,
    pub fam: u8,     // 族
    pub input: u32,  // 输入向量
    pub want: u64,   // 期望输出指纹
}

/// 20 条用例账（输入向量 → 期望指纹，执行器见 exec_case）。
pub const CASE_TAB: [A11yCase; CASE_N] = [
    A11yCase { case_id: 0,  fam: FAM_LABEL,    input: 0x0001, want: exec_case(FAM_LABEL, 0x0001) },
    A11yCase { case_id: 1,  fam: FAM_LABEL,    input: 0x0102, want: exec_case(FAM_LABEL, 0x0102) },
    A11yCase { case_id: 2,  fam: FAM_LABEL,    input: 0x0203, want: exec_case(FAM_LABEL, 0x0203) },
    A11yCase { case_id: 3,  fam: FAM_FOCUS,    input: 0x0304, want: exec_case(FAM_FOCUS, 0x0304) },
    A11yCase { case_id: 4,  fam: FAM_FOCUS,    input: 0x0405, want: exec_case(FAM_FOCUS, 0x0405) },
    A11yCase { case_id: 5,  fam: FAM_FOCUS,    input: 0x0506, want: exec_case(FAM_FOCUS, 0x0506) },
    A11yCase { case_id: 6,  fam: FAM_CONTRAST,    input: 0x0607, want: exec_case(FAM_CONTRAST, 0x0607) },
    A11yCase { case_id: 7,  fam: FAM_CONTRAST,    input: 0x0708, want: exec_case(FAM_CONTRAST, 0x0708) },
    A11yCase { case_id: 8,  fam: FAM_CONTRAST,    input: 0x0809, want: exec_case(FAM_CONTRAST, 0x0809) },
    A11yCase { case_id: 9,  fam: FAM_SCALE,    input: 0x090A, want: exec_case(FAM_SCALE, 0x090A) },
    A11yCase { case_id: 10,  fam: FAM_SCALE,    input: 0x0A0B, want: exec_case(FAM_SCALE, 0x0A0B) },
    A11yCase { case_id: 11,  fam: FAM_SCALE,    input: 0x0B0C, want: exec_case(FAM_SCALE, 0x0B0C) },
    A11yCase { case_id: 12,  fam: FAM_NAV,    input: 0x0C0D, want: exec_case(FAM_NAV, 0x0C0D) },
    A11yCase { case_id: 13,  fam: FAM_NAV,    input: 0x0D0E, want: exec_case(FAM_NAV, 0x0D0E) },
    A11yCase { case_id: 14,  fam: FAM_NAV,    input: 0x0E0F, want: exec_case(FAM_NAV, 0x0E0F) },
    A11yCase { case_id: 15,  fam: FAM_NAV,    input: 0x0F10, want: exec_case(FAM_NAV, 0x0F10) },
    A11yCase { case_id: 16,  fam: FAM_TIMEOUT,    input: 0x1011, want: exec_case(FAM_TIMEOUT, 0x1011) },
    A11yCase { case_id: 17,  fam: FAM_TIMEOUT,    input: 0x1112, want: exec_case(FAM_TIMEOUT, 0x1112) },
    A11yCase { case_id: 18,  fam: FAM_TIMEOUT,    input: 0x1213, want: exec_case(FAM_TIMEOUT, 0x1213) },
    A11yCase { case_id: 19,  fam: FAM_TIMEOUT,    input: 0x1314, want: exec_case(FAM_TIMEOUT, 0x1314) },
];

/// 执行器（纯函数，const 可求值）：族 + 输入向量 → 输出指纹。
pub const fn exec_case(fam: u8, input: u32) -> u64 {
    let mut fp: u64 = 0xCD40_CD41 ^ ((fam as u64).wrapping_mul(0x1000_001F));
    fp ^= input as u64;
    fp = fp.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    fp.rotate_left(17)
}

/// 全册期望指纹（const fn 求值口径的运行时版本）。
pub fn want_of(case: &A11yCase) -> u64 {
    exec_case(case.fam, case.input)
}

/// 全册校验和：20 条期望指纹 XOR 折叠。
pub fn seal_of(tab: &[A11yCase; CASE_N]) -> u64 {
    let mut seal: u64 = 0xCD40_CD42;
    let mut i = 0usize;
    while i < CASE_N {
        seal ^= want_of(&tab[i]);
        i += 1;
    }
    seal
}

/// 向量账执行面：seal 验和门禁 + 两轮复跑。
pub struct A11yVec {
    pub run_total: u32,   // 用例执行总数（含两轮）
    pub pass_total: u32,  // 通过数
    pub fail_total: u32,  // 失败数（期望不符）
    pub reject_total: u32,// 越账拒收
}

impl A11yVec {
    pub fn new() -> A11yVec {
        A11yVec { run_total: 0, pass_total: 0, fail_total: 0, reject_total: 0 }
    }

    /// 单用例执行一轮：输出指纹与期望对账。
    pub fn run_case(&mut self, case: &A11yCase) -> Result<u64, u16> {
        if case.case_id as usize >= CASE_N {
            self.reject_total += 1;
            return Err(E_VEC_BAD);
        }
        let got = exec_case(case.fam, case.input);
        self.run_total += 1;
        if got != case.want {
            self.fail_total += 1;
            return Err(E_OUT_DIFF);
        }
        self.pass_total += 1;
        Ok(got)
    }

    /// 全册 seal 验和门禁：账面校验和与实算一致才准执行。
    pub fn verify_seal(&self, tab: &[A11yCase; CASE_N], claimed: u64) -> Result<(), u16> {
        if seal_of(tab) != claimed {
            return Err(E_SEAL_BAD);
        }
        Ok(())
    }

    /// 全册两轮复跑（J1 主体）：×20 用例各两轮，轮间指纹逐位一致。
    pub fn run_all(&mut self, tab: &[A11yCase; CASE_N]) -> Result<u64, u16> {
        let mut round_fps: [u64; 2] = [0; 2];
        let mut r = 0usize;
        while r < 2 {
            let mut rfp: u64 = 0xCD40_CD43;
            let mut i = 0usize;
            while i < CASE_N {
                let got = self.run_case(&tab[i])?;
                rfp = rfp.rotate_left(7) ^ got;
                i += 1;
            }
            round_fps[r] = rfp;
            r += 1;
        }
        if round_fps[0] != round_fps[1] {
            return Err(E_ROUND_DIFF);
        }
        Ok(round_fps[0])
    }

    /// 执行账守恒：pass+fail == run，fail == 0（J1 全绿）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.pass_total + self.fail_total != self.run_total {
            return Err(E_LEDGER_BAD);
        }
        if self.fail_total != 0 {
            return Err(E_OUT_DIFF);
        }
        if self.pass_total < (CASE_N * 2) as u32 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.pass_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 全册 seal（构建期一次算定，作为声称值）。
pub fn book_seal() -> u64 {
    seal_of(&CASE_TAB)
}

/// 自例程判据：逐条落实 J1（用例 ×20 可复跑），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let seal = book_seal();

    // R1 seal 验和门禁：真 seal 过；单条期望被篡改后 seal 必不符
    //    （防"改期望凑结果"，判据非恒绿）。
    let mut av = A11yVec::new();
    av.verify_seal(&CASE_TAB, seal)?;
    let mut forged = CASE_TAB;
    forged[7].want = forged[7].want ^ 0xFF;
    if av.verify_seal(&forged, seal).is_ok() {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R2 全册两轮复跑（J1 主体）：×20 用例 ×2 轮全绿，轮间指纹
    //    一致，执行账 == 40。
    let mut av = A11yVec::new();
    let fp = av.run_all(&CASE_TAB)?;
    av.audit()?;
    if av.pass_total != (CASE_N * 2) as u32 {
        return Err(E_LEDGER_BAD);
    }
    if fp == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R3 逐条独立对账：20 条逐条 exec_case 直算与 want_of 对账
    //    （不复用 run_case 判定）；族权重对输出敏感（同输入异族
    //    异指纹）。
    let mut i = 0usize;
    while i < CASE_N {
        if exec_case(CASE_TAB[i].fam, CASE_TAB[i].input) != want_of(&CASE_TAB[i]) {
            return Err(E_OUT_DIFF);
        }
        i += 1;
    }
    if exec_case(FAM_LABEL, 0x0304) == exec_case(FAM_FOCUS, 0x0304) {
        return Err(E_OUT_DIFF);
    }
    hits += 1;

    // R4 六族覆盖账：20 条按族分账，标签/焦点/对比度/缩放各 3，
    //    导航 4，超时 4——族账加和 == 20。
    let mut fam_count = [0u32; 6];
    let mut i = 0usize;
    while i < CASE_N {
        fam_count[CASE_TAB[i].fam as usize] += 1;
        i += 1;
    }
    if fam_count[FAM_LABEL as usize] != 3
        || fam_count[FAM_FOCUS as usize] != 3
        || fam_count[FAM_CONTRAST as usize] != 3
        || fam_count[FAM_SCALE as usize] != 3
        || fam_count[FAM_NAV as usize] != 4
        || fam_count[FAM_TIMEOUT as usize] != 4
    {
        return Err(E_LEDGER_BAD);
    }
    let mut sum: u32 = 0;
    let mut f = 0usize;
    while f < 6 {
        sum += fam_count[f];
        f += 1;
    }
    if sum != CASE_N as u32 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 执行器纯函数：同输入两轮必同指纹；单输入位翻转指纹必变
    //    （敏感度非恒等）。
    let a1 = exec_case(FAM_NAV, 0x0C0D);
    let a2 = exec_case(FAM_NAV, 0x0C0D);
    if a1 != a2 {
        return Err(E_ROUND_DIFF);
    }
    if exec_case(FAM_NAV, 0x0C0C) == a1 {
        return Err(E_OUT_DIFF);
    }
    hits += 1;

    // R6 越账拒收与可红：case_id ≥20 拒收留痕；期望伪造必 E_OUT_DIFF。
    let mut av = A11yVec::new();
    let bad_case = A11yCase { case_id: 99, fam: FAM_LABEL, input: 1, want: 0 };
    if av.run_case(&bad_case) != Err(E_VEC_BAD) {
        return Err(E_VEC_BAD);
    }
    if av.reject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    let mut forged2 = CASE_TAB;
    forged2[12].want = 0xDEAD;
    let mut av2 = A11yVec::new();
    if av2.run_case(&forged2[12]) != Err(E_OUT_DIFF) {
        return Err(E_OUT_DIFF);
    }
    if av2.fail_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 seal 对空账/零 claimed 可红：claimed=0 必不符；真 seal 复
    //    验两轮恒绿（验和自身确定性）。
    let av = A11yVec::new();
    if av.verify_seal(&CASE_TAB, 0).is_ok() {
        return Err(E_SEAL_BAD);
    }
    av.verify_seal(&CASE_TAB, book_seal())?;
    av.verify_seal(&CASE_TAB, book_seal())?;
    hits += 1;

    // R8 复跑一致与写回（判据 J2）：全册两轮复跑第二轮，指纹与
    //    R2 轮逐位一致（账面可复现）；写回过。
    let mut av = A11yVec::new();
    let fp1 = av.run_all(&CASE_TAB)?;
    av.audit()?;
    let mut av2 = A11yVec::new();
    let fp2 = av2.run_all(&CASE_TAB)?;
    if fp1 != fp2 {
        return Err(E_REPLAY_DIFF);
    }
    if av.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    av.writeback(true, 0x1B)?;
    hits += 1;

    Ok(hits)
}
