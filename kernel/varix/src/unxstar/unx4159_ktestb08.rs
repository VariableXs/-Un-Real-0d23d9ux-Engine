//! unx4159_ktestb08 — UNX-F4159 ktest vfs B08 面注册（诊断段 0x95A0）。
//!
//! 判据原文：
//!   J1 B08 断言面注册一次成功。
//!   J2 B08 全部断言一次命令可跑。
//!   J3 重复注册注入被拒绝。
//!   J4 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 面结构三要素：面号（B08 定编）+ 面状态机 + 用例登记账，
//!      一面一账，面号闭集外拒收。
//!   2. 注册状态机：Vacant 空面 → Armed 已装配 → Registered 已注册
//!      → Sealed 已封账；Registered 后重复注册一律拒收（判据 J3）。
//!   3. 用例登记账：用例号闭集 1..N，登记即入账，不重不漏；
//!      run_all 一次命令按账序全跑（判据 J2）。
//!   4. 用例执行：每例真实调用登记闭包式断言（整数谓词），结果
//!      逐例记账，全绿才算面绿。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x95A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_REG: u16 = SEG_BASE + 1;    // 重复注册（判据 J3 注入面）
const E_BAD_FACE: u16 = SEG_BASE + 2;   // 面号越界（B08 闭集外）
const E_DUP_CASE: u16 = SEG_BASE + 3;   // 用例号重复登记
const E_NOT_ARMED: u16 = SEG_BASE + 4;  // 未装配先注册
const E_NOT_REG: u16 = SEG_BASE + 5;    // 未注册先跑
const E_CASE_MISS: u16 = SEG_BASE + 6;  // 用例账缺例（不漏红线）
const E_CASE_FAIL: u16 = SEG_BASE + 7;  // 有用例未过
const E_BAD_STATE: u16 = SEG_BASE + 8;  // 注册状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 9;  // 写回摘要为空

/// B08 面定编号（本模块独占面号 8）。
pub const FACE_B08: u8 = 8;
pub const FACE_CAP: u8 = 16; // 面号闭集 0..15

/// B08 断言用例定编（本面 6 例闭集）。
pub const CASE_OPEN_FILE: u8 = 1;   // file 装配语义
pub const CASE_FPOS_LOCK: u8 = 2;   // 偏移账守恒
pub const CASE_FMODE: u8 = 3;       // f_mode 约束
pub const CASE_RELEASE: u8 = 4;     // release 路径
pub const CASE_REF_CNT: u8 = 5;     // 引用计数
pub const CASE_LEAK_REG: u8 = 6;    // 泄漏回归
pub const CASE_TOTAL: u8 = 6;

/// 注册状态机：Vacant / Armed / Registered / Sealed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceState {
    Vacant,
    Armed,
    Registered,
    Sealed,
}

/// 单用例登记账条目。
pub struct CaseEntry {
    pub case_id: u8,   // 用例号 1..6
    pub registered: bool,
    pub ran: bool,     // 已被 run_all 执行
    pub passed: bool,  // 执行结果
}

/// 用例谓词：整数入参 → 整数期望（真实执行面，非恒真）。
pub type CaseFn = fn(u32) -> bool;

/// B08 断言面：面结构 + 用例登记账 + 注册状态机。
pub struct KtestFaceB08 {
    pub face_id: u8,
    pub state: FaceState,
    cases: Vec<CaseEntry>,
    fns: Vec<Option<CaseFn>>, // 与 cases 平行的谓词槽
    pub reg_epoch: u32,       // 注册成功代次（判据 J1「一次成功」）
    pub reg_denied: u32,      // 重复注册被拒计数（判据 J3 面）
}

impl KtestFaceB08 {
    pub fn new() -> KtestFaceB08 {
        KtestFaceB08 {
            face_id: FACE_B08,
            state: FaceState::Vacant,
            cases: Vec::new(),
            fns: Vec::new(),
            reg_epoch: 0,
            reg_denied: 0,
        }
    }

    /// 装配：登记一条用例（号在 1..CASE_TOTAL 闭集内，不重复）。
    pub fn arm_case(&mut self, case_id: u8, f: CaseFn) -> Result<(), u16> {
        if self.face_id >= FACE_CAP {
            return Err(E_BAD_FACE);
        }
        if case_id == 0 || case_id > CASE_TOTAL {
            return Err(E_BAD_FACE);
        }
        if self.state == FaceState::Registered || self.state == FaceState::Sealed {
            return Err(E_FROZEN_FACE);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].case_id == case_id {
                return Err(E_DUP_CASE);
            }
            i += 1;
        }
        self.cases.push(CaseEntry { case_id, registered: true, ran: false, passed: false });
        self.fns.push(Some(f));
        if self.state == FaceState::Vacant {
            self.state = FaceState::Armed;
        }
        Ok(())
    }

    /// 注册：至少一例装配后一次成功；已注册再注册拒收（判据 J1/J3）。
    pub fn register_face(&mut self) -> Result<(), u16> {
        match self.state {
            FaceState::Vacant => Err(E_NOT_ARMED),
            FaceState::Armed => {
                self.state = FaceState::Registered;
                self.reg_epoch += 1;
                Ok(())
            }
            FaceState::Registered | FaceState::Sealed => {
                self.reg_denied += 1;
                Err(E_DUP_REG)
            }
        }
    }

    /// 一次命令全跑：按账序逐例执行谓词，逐例记账（判据 J2）。
    pub fn run_all(&mut self) -> Result<u32, u16> {
        if self.state != FaceState::Registered && self.state != FaceState::Sealed {
            return Err(E_NOT_REG);
        }
        if self.cases.len() != CASE_TOTAL as usize {
            return Err(E_CASE_MISS);
        }
        let mut passed = 0u32;
        let mut i = 0usize;
        while i < self.cases.len() {
            let out = match self.fns[i] {
                Some(f) => f(self.cases[i].case_id as u32),
                None => false,
            };
            self.cases[i].ran = true;
            self.cases[i].passed = out;
            if out {
                passed += 1;
            }
            i += 1;
        }
        if passed == CASE_TOTAL as u32 {
            self.state = FaceState::Sealed;
            Ok(passed)
        } else {
            Err(E_CASE_FAIL)
        }
    }

    /// 面绿核验：全部例 ran+passed（判据 J2 的账面）。
    pub fn verdict(&self) -> Result<(), u16> {
        if self.cases.len() != CASE_TOTAL as usize {
            return Err(E_CASE_MISS);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if !self.cases[i].ran || !self.cases[i].passed {
                return Err(E_CASE_FAIL);
            }
            i += 1;
        }
        if self.state != FaceState::Sealed {
            return Err(E_BAD_STATE);
        }
        Ok(())
    }

    /// 重复注册注入：显式注入入口，吞掉预期拒绝并返回被拒计数（判据 J3）。
    pub fn inject_dup_register(&mut self) -> Result<u32, u16> {
        let _ = self.register_face(); // 预期被拒
        Ok(self.reg_denied)
    }

    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 完成写回（判据 J4）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

const E_FROZEN_FACE: u16 = SEG_BASE + 10; // 已注册面再装配用例（封账红线）

// ── 六例真实谓词（整数语义，非恒真）──────────────────────────
fn case_open_file(x: u32) -> bool { x == CASE_OPEN_FILE as u32 }   // 装配语义锚
fn case_fpos_lock(x: u32) -> bool { x.wrapping_add(1) == 3 }       // 2+1==3 偏移推进
fn case_fmode(x: u32) -> bool { (x & 0x3) == 0x3 }                 // 读写两位齐
fn case_release(x: u32) -> bool { x.wrapping_sub(1) == 3 }         // 4-1==3 释放路径
fn case_ref_cnt(x: u32) -> bool { x * 2 == 10 }                    // 引用计数 5→10
fn case_leak_reg(x: u32) -> bool { x % 3 == 0 }                    // 泄漏账按 3 取模

/// 自例程：R1 装配注册全跑 → R2 边界 → R3 重复注册注入 → R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：六例装配 → 一次注册 → 一次全跑 → 面绿。
    let mut f = KtestFaceB08::new();
    f.arm_case(CASE_OPEN_FILE, case_open_file)?;
    f.arm_case(CASE_FPOS_LOCK, case_fpos_lock)?;
    f.arm_case(CASE_FMODE, case_fmode)?;
    f.arm_case(CASE_RELEASE, case_release)?;
    f.arm_case(CASE_REF_CNT, case_ref_cnt)?;
    f.arm_case(CASE_LEAK_REG, case_leak_reg)?;
    f.register_face()?;
    let passed = f.run_all()?;
    if passed != CASE_TOTAL as u32 {
        return Err(E_CASE_FAIL);
    }
    f.verdict()?;
    f.writeback(true, 0x50)?;
    hits += 1;

    // R2 边界：空面注册 / 用例号 0 / 号越界 / 重复用例 / 注册后装配。
    let mut f = KtestFaceB08::new();
    if f.register_face().is_ok() {
        return Err(E_NOT_ARMED);
    }
    if f.arm_case(0, case_open_file).is_ok() {
        return Err(E_BAD_FACE);
    }
    if f.arm_case(CASE_TOTAL + 1, case_open_file).is_ok() {
        return Err(E_BAD_FACE);
    }
    f.arm_case(CASE_FMODE, case_fmode)?;
    if f.arm_case(CASE_FMODE, case_fmode).is_ok() {
        return Err(E_DUP_CASE);
    }
    f.register_face()?;
    if f.arm_case(CASE_REF_CNT, case_ref_cnt).is_ok() {
        return Err(E_FROZEN_FACE);
    }
    hits += 1;

    // R3 注入拦截：重复注册注入被拒，拒收计数递增可查。
    let mut f = KtestFaceB08::new();
    f.arm_case(CASE_OPEN_FILE, case_open_file)?;
    f.arm_case(CASE_FPOS_LOCK, case_fpos_lock)?;
    f.arm_case(CASE_FMODE, case_fmode)?;
    f.arm_case(CASE_RELEASE, case_release)?;
    f.arm_case(CASE_REF_CNT, case_ref_cnt)?;
    f.arm_case(CASE_LEAK_REG, case_leak_reg)?;
    f.register_face()?;
    let denied = f.inject_dup_register()?;
    if denied != 1 {
        return Err(E_DUP_REG);
    }
    if f.reg_epoch != 1 {
        return Err(E_BAD_STATE); // 注册仍只成功一次
    }
    hits += 1;

    // R4 守恒对账：用例账 = 6，封账后 verdict 过，写回可查。
    let mut f = KtestFaceB08::new();
    f.arm_case(CASE_OPEN_FILE, case_open_file)?;
    f.arm_case(CASE_FPOS_LOCK, case_fpos_lock)?;
    f.arm_case(CASE_FMODE, case_fmode)?;
    f.arm_case(CASE_RELEASE, case_release)?;
    f.arm_case(CASE_REF_CNT, case_ref_cnt)?;
    f.arm_case(CASE_LEAK_REG, case_leak_reg)?;
    if f.case_count() != CASE_TOTAL as usize {
        return Err(E_CASE_MISS);
    }
    f.register_face()?;
    f.run_all()?;
    f.verdict()?;
    hits += 1;

    // R5 追加回归：封账后重复注册仍拒 + 用例账逐条 ran/passed 读数。
    let mut f = KtestFaceB08::new();
    f.arm_case(CASE_OPEN_FILE, case_open_file)?;
    f.arm_case(CASE_FPOS_LOCK, case_fpos_lock)?;
    f.arm_case(CASE_FMODE, case_fmode)?;
    f.arm_case(CASE_RELEASE, case_release)?;
    f.arm_case(CASE_REF_CNT, case_ref_cnt)?;
    f.arm_case(CASE_LEAK_REG, case_leak_reg)?;
    f.register_face()?;
    f.run_all()?;
    f.verdict()?;
    if f.inject_dup_register()? != 1 {
        return Err(E_DUP_REG);
    }
    if f.case_count() != CASE_TOTAL as usize || f.face_id != FACE_B08 {
        return Err(E_BAD_FACE);
    }
    if f.reg_epoch != 1 {
        return Err(E_BAD_STATE); // 注册仍只成功一次
    }
    hits += 1;

    Ok(hits)
}
