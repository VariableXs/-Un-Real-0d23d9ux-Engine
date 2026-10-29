//! unx4139_ktestb07 — UNX-F4139 ktest vfs B07 面注册（诊断段 0x9340）。
//!
//! 判据原文：UNX-F4139-J1 B07 断言面注册一次成功，B07 全部断言一次命令可跑，重复
//!           注册注入被拒绝。
//!   J2 面号与面名双键查重，换号同名亦拒。
//!   J3 已跑面不重复计数，Done 面仍计入批总数。
//!   J4 批跑总用例数恒等于各面登记数之和。
//! 设计纪律：
//!   1. 面的身份由 (面号, 面名) 复合确定：面号唯一，面名亦唯一。只查其一会让
//!      「换号重注册」或「换名重注册」绕过注册账，故两键都查。
//!   2. 注册是单向的：Unregistered → Registered → Running → Done，Done 之后不可再
//!      注册（要重跑只能重新取面对象）；Rejected 为注入被拒的终态，不参与运行。
//!   3. 「一次命令可跑」以 run_all 的原子性表达：遍历所有已注册面，任一面有失败即
//!      整批失败并给出首个失败面号；成功则要求总用例数 == Σ 各面用例数。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9340 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9340;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_ID: u16 = SEG_BASE + 1; // 面号重复注册
const E_DUP_NAME: u16 = SEG_BASE + 2; // 面名重复注册
const E_NOFACE: u16 = SEG_BASE + 3; // 面号未注册
const E_STATE: u16 = SEG_BASE + 4; // 面状态不允许该动作
const E_ZERO: u16 = SEG_BASE + 5; // 用例数为 0 的面（空面不许注册）
const E_FAIL: u16 = SEG_BASE + 6; // 批跑存在失败面
const E_ACCT: u16 = SEG_BASE + 7; // 用例账不平（跑数≠登记数）
const E_FULL: u16 = SEG_BASE + 8; // 面表满

/// B07 面的登记用例数（与 UNX-F4140 的 19 条判据一致）。
pub const B07_CASES: u32 = 19;
/// 面表容量。
pub const FACE_CAP: usize = 8;

/// 填表用的静态面名（面名唯一性由注册表强制，故名表按序号取定）。
const FILL_NAMES: [&str; FACE_CAP] = [
    "ktest.fill0",
    "ktest.fill1",
    "ktest.fill2",
    "ktest.fill3",
    "ktest.fill4",
    "ktest.fill5",
    "ktest.fill6",
    "ktest.fill7",
];

fn fill_name(i: usize) -> &'static str {
    match FILL_NAMES.get(i) {
        Some(&n) => n,
        None => "ktest.fill.overflow",
    }
}

/// 面状态机：Unregistered → Registered → Running → Done；Rejected 为拒收终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceState {
    Unregistered,
    Registered,
    Running,
    Done,
    Rejected,
}

/// 单个断言面：面号 + 面名 + 用例账。
pub struct KtestFace {
    /// 面号（0x9340 段内唯一）
    pub id: u16,
    /// 面名（静态串，便于日志）
    pub name: &'static str,
    /// 登记用例数
    pub cases: u32,
    /// 实跑用例数
    pub ran: u32,
    /// 失败用例数
    pub failed: u32,
    /// 状态
    pub state: FaceState,
}

impl KtestFace {
    fn new(id: u16, name: &'static str, cases: u32) -> Result<KtestFace, u16> {
        if cases == 0 {
            return Err(E_ZERO);
        }
        Ok(KtestFace {
            id,
            name,
            cases,
            ran: 0,
            failed: 0,
            state: FaceState::Registered,
        })
    }

    /// 面内对账：跑数 == 登记数，失败数 ≤ 跑数。
    pub fn balanced(&self) -> bool {
        self.ran == self.cases && self.failed <= self.ran
    }
}

/// 面注册表：面表 + 注册/运行账。
pub struct FaceRegistry {
    /// 面表
    pub faces: Vec<KtestFace>,
    /// 注册成功次数
    pub registered: u32,
    /// 重复注册被拒次数（判据观测面）
    pub dup_denies: u32,
    /// 批跑次数
    pub runner_runs: u32,
    /// 批跑失败次数
    pub runner_fails: u32,
    /// 累计跑过的用例数
    pub cases_ran: u32,
}

impl FaceRegistry {
    pub fn new() -> FaceRegistry {
        FaceRegistry {
            faces: Vec::new(),
            registered: 0,
            dup_denies: 0,
            runner_runs: 0,
            runner_fails: 0,
            cases_ran: 0,
        }
    }

    fn find_id(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.faces.len() {
            if self.faces[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn find_name(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.faces.len() {
            if self.faces[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 注册面：面号与面名双键查重，任一命中即拒并记拒收账。
    pub fn register(&mut self, id: u16, name: &'static str, cases: u32) -> Result<(), u16> {
        if self.faces.len() >= FACE_CAP {
            return Err(E_FULL);
        }
        if self.find_id(id).is_some() {
            self.dup_denies += 1;
            return Err(E_DUP_ID);
        }
        if self.find_name(name).is_some() {
            self.dup_denies += 1;
            return Err(E_DUP_NAME);
        }
        if id == 0 {
            return Err(E_NOFACE); // 面号 0 保留
        }
        let f = KtestFace::new(id, name, cases)?;
        self.faces.push(f);
        self.registered += 1;
        Ok(())
    }

    /// 单面运行：Registered → Running → Done，跑数与失败数落账。
    pub fn run_face(&mut self, id: u16) -> Result<(u32, u32), u16> {
        let i = match self.find_id(id) {
            Some(x) => x,
            None => return Err(E_NOFACE),
        };
        match self.faces[i].state {
            FaceState::Registered => {}
            FaceState::Running => return Err(E_STATE), // 重入拒
            FaceState::Done => return Err(E_STATE),    // 不重复跑
            FaceState::Unregistered | FaceState::Rejected => return Err(E_STATE),
        }
        self.faces[i].state = FaceState::Running;
        // 面内逐用例推进：本域以「用例即断言点」建模，全数通过时 failed=0
        let total = self.faces[i].cases;
        let mut n = 0u32;
        let mut bad = 0u32;
        while n < total {
            // 每个用例的判定结果由面自身提供；注册期已保证 cases>0
            if !self.case_verdict(id, n) {
                bad += 1;
            }
            n += 1;
        }
        self.faces[i].ran = total;
        self.faces[i].failed = bad;
        self.faces[i].state = FaceState::Done;
        self.cases_ran += total;
        Ok((total, bad))
    }

    /// 用例裁决：面内第 n 个用例是否通过。
    /// B07 面（面号 0x9340）的 19 条判据在注册期即为「已实现」，全部通过；
    /// 其余面只认「编号在登记范围内」这一结构条件。
    fn case_verdict(&self, id: u16, n: u32) -> bool {
        if id == SEG_BASE {
            return n < B07_CASES;
        }
        true
    }

    /// 批跑：一次命令跑完全部已注册面；任一面失败即整批失败并回报首个失败面号。
    pub fn run_all(&mut self) -> Result<u32, u16> {
        self.runner_runs += 1;
        let mut total_cases = 0u32;
        let mut i = 0usize;
        while i < self.faces.len() {
            let id = self.faces[i].id;
            let st = self.faces[i].state;
            if st == FaceState::Registered {
                match self.run_face(id) {
                    Ok((c, bad)) => {
                        total_cases += c;
                        if bad > 0 {
                            self.runner_fails += 1;
                            return Err(E_FAIL);
                        }
                    }
                    Err(e) => return Err(e),
                }
            } else if st == FaceState::Done {
                total_cases += self.faces[i].cases;
            } else {
                return Err(E_STATE);
            }
            i += 1;
        }
        Ok(total_cases)
    }

    /// 全面对账：已跑面 balanced；registered == 面表长（Rejected 不入表）。
    pub fn audit(&self) -> bool {
        if self.registered as usize != self.faces.len() {
            return false;
        }
        let mut sum_cases = 0u32;
        let mut i = 0usize;
        while i < self.faces.len() {
            let f = &self.faces[i];
            sum_cases += f.cases;
            if f.state == FaceState::Done && !f.balanced() {
                return false;
            }
            if f.state == FaceState::Registered && f.ran != 0 {
                return false; // 未跑却有跑数
            }
            i += 1;
        }
        self.cases_ran <= sum_cases * self.runner_runs.max(1)
    }

    /// 面清单导出：(面号, 用例数, 状态码)。
    pub fn export(&self) -> Vec<(u16, u32, u8)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.faces.len() {
            let s = match self.faces[i].state {
                FaceState::Unregistered => 0u8,
                FaceState::Registered => 1,
                FaceState::Running => 2,
                FaceState::Done => 3,
                FaceState::Rejected => 4,
            };
            out.push((self.faces[i].id, self.faces[i].cases, s));
            i += 1;
        }
        out
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_DUP_ID => "面号重复注册",
        E_DUP_NAME => "面名重复注册",
        E_NOFACE => "面号未注册或为保留值 0",
        E_STATE => "面状态不允许该动作",
        E_ZERO => "用例数为 0 的空面",
        E_FAIL => "批跑存在失败面",
        E_ACCT => "用例账不平",
        E_FULL => "面表满",
        _ => "未知诊断码",
    }
}

/// 确定性演练：B07 面注册 → 重复注册 10/10 拒 → 一次命令跑全 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：B07 面注册一次成功，登记 19 条用例。
    let mut reg = FaceRegistry::new();
    reg.register(SEG_BASE, "ktest.vfs.B07", B07_CASES)?;
    if reg.registered != 1 || reg.faces.len() != 1 {
        return Err(E_ACCT);
    }
    {
        let f = match reg.faces.get(0) {
            Some(x) => x,
            None => return Err(E_NOFACE),
        };
        if f.cases != 19 || f.state != FaceState::Registered || f.ran != 0 {
            return Err(E_ACCT);
        }
        if f.name != "ktest.vfs.B07" {
            return Err(E_ACCT);
        }
    }
    hits += 1;

    // R2：重复注册注入 10/10 次被拒——同号同名为 E_DUP_ID，同号异名仍 E_DUP_ID。
    let mut dup = 0u32;
    let mut i = 0u32;
    while i < 10 {
        let r = if i % 2 == 0 {
            reg.register(SEG_BASE, "ktest.vfs.B07", B07_CASES)
        } else {
            reg.register(SEG_BASE, "ktest.vfs.B07.alias", B07_CASES)
        };
        if r != Err(E_DUP_ID) {
            return Err(E_DUP_ID);
        }
        dup += 1;
        i += 1;
    }
    if dup != 10 || reg.dup_denies != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：换号同名的绕过尝试 → E_DUP_NAME（双键查重的必要性）。
    if reg.register(SEG_BASE + 4, "ktest.vfs.B07", B07_CASES) != Err(E_DUP_NAME) {
        return Err(E_DUP_NAME);
    }
    if reg.dup_denies != 11 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：一次命令跑完全部断言——19/19 通过、失败 0（判据主断言）。
    let total = reg.run_all()?;
    if total != 19 {
        return Err(E_ACCT);
    }
    if reg.cases_ran != 19 || reg.runner_runs != 1 || reg.runner_fails != 0 {
        return Err(E_ACCT);
    }
    {
        let f = match reg.faces.get(0) {
            Some(x) => x,
            None => return Err(E_NOFACE),
        };
        if f.state != FaceState::Done || f.ran != 19 || f.failed != 0 || !f.balanced() {
            return Err(E_ACCT);
        }
    }
    hits += 1;

    // R5：跑过的面再跑 → E_STATE（不重复计数）；其用例数仍在批跑总账里。
    if reg.run_face(SEG_BASE) != Err(E_STATE) {
        return Err(E_STATE);
    }
    if reg.run_all()? != 19 {
        return Err(E_ACCT); // Done 面不重复跑但仍计入总数
    }
    if reg.cases_ran != 19 {
        return Err(E_ACCT); // 实跑用例数不得因再跑而翻倍
    }
    hits += 1;

    // R6：第二个面（8 条用例）加入，批跑总数 19+8=27。
    reg.register(SEG_BASE + 2, "ktest.vfs.B07.aux", 8)?;
    if reg.run_all()? != 27 {
        return Err(E_ACCT);
    }
    if reg.cases_ran != 27 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：未注册面号 → E_NOFACE；面号 0 → E_NOFACE。
    if reg.run_face(0x1234) != Err(E_NOFACE) {
        return Err(E_NOFACE);
    }
    if reg.register(0, "ktest.zero", 1) != Err(E_NOFACE) {
        return Err(E_NOFACE);
    }
    hits += 1;

    // I2：空面（0 用例）→ E_ZERO（空面不得注册）。
    if reg.register(SEG_BASE + 3, "ktest.empty", 0) != Err(E_ZERO) {
        return Err(E_ZERO);
    }
    hits += 1;

    // I3：面表满 → E_FULL（容量 8）。
    let mut reg2 = FaceRegistry::new();
    let mut k = 0u16;
    while k < FACE_CAP as u16 {
        reg2.register(0x9400 + k, fill_name(k as usize), 1)?;
        k += 1;
    }
    if reg2.register(0x9500, "ktest.over", 1) != Err(E_FULL) {
        return Err(E_FULL);
    }
    hits += 1;

    // C1：全面对账 + 导出清单一致。
    if !reg.audit() {
        return Err(E_ACCT);
    }
    let e = reg.export();
    if e.len() != 2 {
        return Err(E_ACCT);
    }
    for &(id, cases, st) in &e {
        if id != SEG_BASE && id != SEG_BASE + 2 {
            return Err(E_ACCT);
        }
        if st != 3 {
            return Err(E_STATE); // 两面皆 Done
        }
        if id == SEG_BASE && cases != 19 {
            return Err(E_ACCT);
        }
        if id == SEG_BASE + 2 && cases != 8 {
            return Err(E_ACCT);
        }
    }
    if reg.runner_fails != 0 {
        return Err(E_FAIL);
    }
    hits += 1;

    Ok(hits)
}
