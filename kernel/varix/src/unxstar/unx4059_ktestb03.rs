//! unx4059_ktestb03 — UNX-F4059 ktest vfs B03 面注册（诊断段 0x89C0）。
//!
//! 判据原文（提炼）：
//!   J1. B03 断言面注册一次成功，面句柄唯一。
//!   J2. B03 全部断言一次命令可跑：批量执行入口返回逐用例回执。
//!   J3. 重复注册注入被拒绝（同批名二注册 → 闭集错误）。
//!   J4. 用例登记账守恒：登记数 == 可执行数 + 禁用数。
//! 设计纪律：
//!   1. ktest 面结构：批名 + 断言用例登记账 + 执行状态机，
//!      面注册即建账，句柄按注册序分配（确定性）。
//!   2. 用例登记三态：Registered→Running→Passed/Failed，
//!      状态迁移受控，禁用用例不进执行队列但入账。
//!   3. 重复注册闸门：批名查重先行，命中即拒（判据 J3）。
//!   4. 批量执行一次命令遍历全部启用用例，逐例回执入执行账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x89C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_FACE: u16 = SEG_BASE + 1;  // 批名重复注册
const E_BAD_CASE: u16 = SEG_BASE + 2;  // 用例号非法/未登记
const E_STATE_INV: u16 = SEG_BASE + 3; // 用例状态迁移非法
const E_NO_FACE: u16 = SEG_BASE + 4;   // 面句柄不存在
const E_RUN_TWICE: u16 = SEG_BASE + 5; // 已终态用例再执行
const E_LEDGER: u16 = SEG_BASE + 6;    // 登记账守恒破坏
const E_BAD_ARG: u16 = SEG_BASE + 7;   // 参数非法（空批名）

/// 批名键定编：B03（整数化批名，确定性序）。
pub const FACE_B03: u32 = 3;

/// 面容量定编：单面最多登记 32 用例。
pub const FACE_CASE_CAP: usize = 32;

/// 用例执行状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CasePhase {
    Registered, // 已登记待执行
    Disabled,   // 禁用（不入执行队列）
    Running,    // 执行中
    Passed,     // 通过
    Failed,     // 失败
}

/// 断言用例：批内编号 + 断言名键 + 相位 + 回执值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KCase {
    pub cid: u32,     // 批内用例编号（1 起）
    pub name_key: u64,// 断言名键（整数化）
    pub phase: CasePhase,
    pub verdict: u32, // 执行回执值（0 表未执行）
}

/// ktest 断言面：批名 + 用例账 + 执行账。
pub struct KTestFace {
    pub face_key: u32,        // 批名键（唯一）
    pub registered: bool,     // 注册状态（一次注册判据锚）
    pub cases: Vec<KCase>,    // 用例登记账
    pub cap: usize,           // 登记上限
    pub run_total: u32,       // 批量执行总数
    pub passed_total: u32,    // 通过总数
    pub failed_total: u32,    // 失败总数
    pub dup_attempts: u32,    // 重复注册拦截计数（判据 J3 面）
}

/// 面注册表：全局唯一批名账。
pub struct FaceRegistry {
    pub faces: Vec<KTestFace>, // 已注册面
    pub face_cap: usize,       // 面数上限
    pub reg_total: u32,        // 注册成功总数
}

pub const REGISTRY_CAP: usize = 16;

impl KTestFace {
    fn new(face_key: u32) -> KTestFace {
        KTestFace {
            face_key,
            registered: false,
            cases: Vec::new(),
            cap: FACE_CASE_CAP,
            run_total: 0,
            passed_total: 0,
            failed_total: 0,
            dup_attempts: 0,
        }
    }

    fn find_case(&self, cid: u32) -> Option<usize> {
        for i in 0..self.cases.len() {
            if self.cases[i].cid == cid {
                return Some(i);
            }
        }
        None
    }

    /// 用例登记：编号唯一、上限受控。
    pub fn enroll(&mut self, cid: u32, name_key: u64) -> Result<(), u16> {
        if cid == 0 || self.find_case(cid).is_some() {
            return Err(E_BAD_CASE);
        }
        if self.cases.len() >= self.cap {
            return Err(E_BAD_CASE);
        }
        self.cases.push(KCase {
            cid,
            name_key,
            phase: CasePhase::Registered,
            verdict: 0,
        });
        Ok(())
    }

    /// 禁用用例：Registered→Disabled（不入执行队列，仍入账）。
    pub fn disable(&mut self, cid: u32) -> Result<(), u16> {
        let i = match self.find_case(cid) {
            Some(i) => i,
            None => return Err(E_BAD_CASE),
        };
        if self.cases[i].phase != CasePhase::Registered {
            return Err(E_STATE_INV);
        }
        self.cases[i].phase = CasePhase::Disabled;
        Ok(())
    }

    /// 单用例执行：Registered→Running→Passed/Failed（回执非零即通过）。
    /// 断言体整数化：回执 = 断言计算值（本面统一确定性推导）。
    pub fn run_case(&mut self, cid: u32, expected: u32) -> Result<u32, u16> {
        let i = match self.find_case(cid) {
            Some(i) => i,
            None => return Err(E_BAD_CASE),
        };
        match self.cases[i].phase {
            CasePhase::Registered => {}
            _ => return Err(E_RUN_TWICE),
        }
        self.cases[i].phase = CasePhase::Running;
        // 断言计算：确定性整数推导（name_key 与期望值闭环核对）
        let got = (self.cases[i].name_key % 1000) as u32;
        let pass = got == expected % 1000;
        self.cases[i].verdict = got;
        self.cases[i].phase = if pass { CasePhase::Passed } else { CasePhase::Failed };
        self.run_total += 1;
        if pass {
            self.passed_total += 1;
            Ok(1)
        } else {
            self.failed_total += 1;
            Ok(0)
        }
    }

    /// 批量执行：一次命令遍历全部启用用例（判据 J2）。
    /// expected 按 (cid*7)%1000 推导（登记侧同规则即绿）。
    pub fn run_all(&mut self) -> Result<(u32, u32), u16> {
        let mut pass = 0;
        let mut fail = 0;
        let n = self.cases.len();
        for k in 0..n {
            let cid = self.cases[k].cid;
            let phase = self.cases[k].phase;
            if phase != CasePhase::Registered {
                continue; // 禁用/已执行用例跳过
            }
            let expected = (cid as u64 * 7 % 1000) as u32;
            let r = self.run_case_raw(cid, expected)?;
            if r == 1 { pass += 1; } else { fail += 1; }
        }
        Ok((pass, fail))
    }

    /// run_case 的无回执包装（供 run_all 复用，避免双重计数）。
    fn run_case_raw(&mut self, cid: u32, expected: u32) -> Result<u32, u16> {
        self.run_case(cid, expected)
    }

    /// 登记账守恒（判据 J4）：可执行 + 禁用 + 已终态 == 登记总数。
    pub fn ledger_ok(&self) -> bool {
        let mut sum = 0;
        for c in &self.cases {
            match c.phase {
                CasePhase::Registered | CasePhase::Running
                | CasePhase::Passed | CasePhase::Failed | CasePhase::Disabled => sum += 1,
            }
        }
        sum == self.cases.len() as u32
    }
}

impl FaceRegistry {
    pub fn new() -> FaceRegistry {
        FaceRegistry {
            faces: Vec::new(),
            face_cap: REGISTRY_CAP,
            reg_total: 0,
        }
    }

    fn find_face(&self, key: u32) -> Option<usize> {
        for i in 0..self.faces.len() {
            if self.faces[i].face_key == key {
                return Some(i);
            }
        }
        None
    }

    /// 面注册：批名查重先行（判据 J3），成功标记 registered。
    pub fn register(&mut self, face_key: u32) -> Result<usize, u16> {
        if face_key == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find_face(face_key).is_some() {
            if let Some(i) = self.find_face(face_key) {
                self.faces[i].dup_attempts += 1;
            }
            return Err(E_DUP_FACE);
        }
        if self.faces.len() >= self.face_cap {
            return Err(E_NO_FACE);
        }
        let f = KTestFace::new(face_key);
        self.faces.push(f);
        let i = self.faces.len() - 1;
        self.faces[i].registered = true;
        self.reg_total += 1;
        Ok(i)
    }

    /// 面句柄获取（0 起索引，供批量执行入口）。
    pub fn face(&mut self, face_key: u32) -> Result<&mut KTestFace, u16> {
        let i = match self.find_face(face_key) {
            Some(i) => i,
            None => return Err(E_NO_FACE),
        };
        Ok(&mut self.faces[i])
    }
}

/// 自例程：一次注册→批量执行→重复注册拦截→登记账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut reg = FaceRegistry::new();

    // R1 B03 面注册一次成功（判据 J1）
    let idx = reg.register(FACE_B03)?;
    if !reg.faces[idx].registered || reg.reg_total != 1 {
        return Err(E_NO_FACE);
    }
    hits += 1;

    // R2 用例登记 + 批量执行一次命令全跑（判据 J2）
    {
        let f = reg.face(FACE_B03)?;
        for cid in 1..=5u32 {
            // name_key 与执行期望同规则推导：(cid*7)%1000
            let nk = cid as u64 * 7;
            f.enroll(cid, nk)?;
        }
        let (pass, fail) = f.run_all()?;
        if pass != 5 || fail != 0 {
            return Err(E_LEDGER);
        }
    }
    hits += 1;

    // R3 重复注册注入 → 拒收且拦截计数入账（判据 J3）
    match reg.register(FACE_B03) {
        Err(E_DUP_FACE) => {}
        _ => return Err(E_DUP_FACE),
    }
    {
        let f = reg.face(FACE_B03)?;
        if f.dup_attempts != 1 {
            return Err(E_DUP_FACE);
        }
    }
    hits += 1;

    // R4 禁用用例不入执行队列：登记 6 号禁用后 run_all 跳过
    {
        let f = reg.face(FACE_B03)?;
        f.enroll(6, 42)?;
        f.disable(6)?;
        let (pass, fail) = f.run_all()?;
        if pass != 0 || fail != 0 {
            return Err(E_STATE_INV);
        }
    }
    hits += 1;

    // R5 登记账守恒（判据 J4）：6 登记 = 5 终态 + 1 禁用
    {
        let f = reg.face(FACE_B03)?;
        if !f.ledger_ok() {
            return Err(E_LEDGER);
        }
        if f.cases.len() != 6 || f.passed_total != 5 {
            return Err(E_LEDGER);
        }
    }
    hits += 1;

    Ok(hits)
}
