//! unx4079_ktestb04 — UNX-F4079 ktest vfs B04 面注册（诊断段 0x8C20）。
//!
//! 判据原文：UNX-F4079-J1 B04 断言面注册一次成功，B04 全部断言一次命令可跑，
//!           重复注册注入被拒绝。
//! 可验性质（本模块自证面）：
//!   J1 面注册是一次成功动作：Empty → Registering → Registered，register_face 只
//!      成功一次，同一面号二次注册必须拒（E_FACE_DUP）且注册账不增；
//!   J2 一条命令可跑全：run_all 一次调用必须遍历全部已注册面的全部用例，
//!      Report.ran == 用例总数，不许分批、不许漏面；
//!   J3 用例登记账唯一：同一面内 case_id 重复必须拒（E_CASE_DUP），拒后用例表长度不变；
//!   J4 相位纪律：只有 Registering 能加用例，Registered 之后加用例拒（E_NOT_REG），
//!      Frozen 之后连 run 都被拒（E_FROZEN）——冻结是终态；
//!   J5 注册/运行分离账：registers 只记成功注册面数，runs 只记成功跑批次数，
//!      失败路径一律不入账。
//! 设计纪律：
//!   1. 面号写死闭集：B01=1、B02=2、B03=3、B04=4、B05=5（ktest 面号与批号同构），
//!      本模块只登记 B04，但面表按面号升序索引，便于后续面拼接。
//!   2. 用例名用 &'static str：ktest 用例表在编译期就定死，运行期不再分配字符串；
//!      这样"一次命令跑全"不会因为分配失败而半途中断。
//!   3. 面相位机单向：Empty → Registering → Registered → Frozen，任何回退都拒；
//!      Frozen 是"已归档"，回归仍可跑但不再接受新用例（本模块按判据取更严：冻结后
//!      连跑也拒，需要重跑必须先重注册）。
//!   4. 注册成功后才允许跑；跑之前逐面核用例数 > 0，空面单独记负样本账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8C20 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8C20;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_FACE_DUP: u16 = SEG_BASE + 1; // 同面号重复注册
const E_CASE_DUP: u16 = SEG_BASE + 2; // 同面内用例号重复
const E_NOT_REG: u16 = SEG_BASE + 3; // 相位不对（未注册即加用例/跑批）
const E_FROZEN: u16 = SEG_BASE + 4; // 已冻结，拒一切变更与跑批
const E_NO_FACE: u16 = SEG_BASE + 5; // 面号不存在
const E_NAME_EMPTY: u16 = SEG_BASE + 6; // 面名/用例名为空
const E_EMPTY_FACE: u16 = SEG_BASE + 7; // 空面跑批（无用例）
const E_BAL: u16 = SEG_BASE + 8; // 注册/跑批账不平

/// ktest 面号闭集（与批号同构，只列本册关心的五面）。
pub const FACE_B01: u16 = 1;
pub const FACE_B02: u16 = 2;
pub const FACE_B03: u16 = 3;
pub const FACE_B04: u16 = 4;
pub const FACE_B05: u16 = 5;

/// 面相位状态机（单向，见设计纪律 3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceState {
    Empty = 0,       // 面已建未注册
    Registering = 1, // 注册中，可加用例
    Registered = 2,  // 注册完成，可跑
    Frozen = 3,      // 已冻结，终态
}

/// 用例类型闭集：断言 / 回归 / 基准。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseKind {
    Assert = 0,  // 判据断言
    Regress = 1, // 注入回归
    Bench = 2,   // 基线对照
}

/// 单用例裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseVerdict {
    Pass = 0,
    Fail = 1,
    Skip = 2,
}

/// 用例声明：号 + 名 + 类型（全静态，运行期不分配）。
pub struct CaseDecl {
    pub case_id: u16,        // 面内用例号（唯一）
    pub name: &'static str,  // 用例名（编译期定死，非空）
    pub kind: CaseKind,      // 用例类型
    pub verdict: CaseVerdict, // 最近一次裁决（跑批后回填）
}

/// 断言面：号 + 名 + 用例表 + 相位 + 跑批账。
pub struct Face {
    pub face_id: u16,       // 面号
    pub name: &'static str, // 面名
    pub cases: Vec<CaseDecl>, // 用例表
    pub state: FaceState,   // 相位
    pub runs: u32,          // 该面成功跑批次数
}

impl Face {
    fn new(face_id: u16, name: &'static str) -> Face {
        Face {
            face_id,
            name,
            cases: Vec::new(),
            state: FaceState::Empty,
            runs: 0,
        }
    }

    /// 面内用例号查重（线性，用例数十条量级）。
    pub fn has_case(&self, case_id: u16) -> bool {
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].case_id == case_id {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 面内通过/失败计数（跑批后读面）。
    pub fn tally(&self) -> (u32, u32, u32) {
        let mut p = 0u32;
        let mut f = 0u32;
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.cases.len() {
            match self.cases[i].verdict {
                CaseVerdict::Pass => p += 1,
                CaseVerdict::Fail => f += 1,
                CaseVerdict::Skip => s += 1,
            }
            i += 1;
        }
        (p, f, s)
    }
}

/// 跑批报告：一次命令的总账。
pub struct RunReport {
    pub faces: u32,   // 参与面数
    pub total: u32,   // 用例总数
    pub ran: u32,     // 实际跑数（应等于 total）
    pub passed: u32,  // 通过数
    pub failed: u32,  // 失败数
    pub skipped: u32, // 跳过数
}

impl RunReport {
    fn new() -> RunReport {
        RunReport {
            faces: 0,
            total: 0,
            ran: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
        }
    }

    /// 全覆盖判定：跑数等于总数，且三类裁决之和等于跑数（一用例一裁决）。
    pub fn complete(&self) -> bool {
        self.ran == self.total && self.passed + self.failed + self.skipped == self.ran
    }

    /// 全绿：全覆盖且零失败零跳过。
    pub fn green(&self) -> bool {
        self.complete() && self.failed == 0 && self.skipped == 0
    }
}

/// 注册枢纽：面表 + 注册账 + 跑批账 + 负样本账。
pub struct KtestReg {
    pub faces: Vec<Face>,   // 面表（按注册顺序）
    pub registers: u32,     // 成功注册面数
    pub runs: u32,          // 成功跑批次数
    pub dup_rejects: u32,   // 重复注册被拒次数
    pub case_rejects: u32,  // 重复用例被拒次数
}

impl KtestReg {
    pub fn new() -> KtestReg {
        KtestReg {
            faces: Vec::new(),
            registers: 0,
            runs: 0,
            dup_rejects: 0,
            case_rejects: 0,
        }
    }

    /// 面定位（面号 → 下标），查前比 len。
    pub fn find_face(&self, face_id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.faces.len() {
            if self.faces[i].face_id == face_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 注册一次面：Empty → Registering。重复面号拒（J1）。
    pub fn register_face(&mut self, face_id: u16, name: &'static str) -> Result<usize, u16> {
        if face_id == 0 || face_id > FACE_B05 {
            return Err(E_NO_FACE); // 面号闭集外，不建面（防野生面号污染面表）
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if self.find_face(face_id).is_some() {
            self.dup_rejects += 1;
            return Err(E_FACE_DUP);
        }
        let mut f = Face::new(face_id, name);
        f.state = FaceState::Registering;
        self.faces.push(f);
        self.registers += 1;
        Ok(self.faces.len() - 1)
    }

    /// 加用例：相位必须 Registering，用例号面内唯一（J3）。
    pub fn add_case(
        &mut self,
        face_id: u16,
        case_id: u16,
        name: &'static str,
        kind: CaseKind,
    ) -> Result<(), u16> {
        let idx = match self.find_face(face_id) {
            Some(i) => i,
            None => return Err(E_NO_FACE),
        };
        match self.faces[idx].state {
            FaceState::Registering => {}
            FaceState::Empty => return Err(E_NOT_REG),
            FaceState::Registered => return Err(E_NOT_REG), // 注册完成后不再收用例
            FaceState::Frozen => return Err(E_FROZEN),
        }
        if case_id == 0 || name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if self.faces[idx].has_case(case_id) {
            self.case_rejects += 1;
            return Err(E_CASE_DUP);
        }
        self.faces[idx].cases.push(CaseDecl {
            case_id,
            name,
            kind,
            verdict: CaseVerdict::Skip, // 未跑默认 Skip，跑批后回填
        });
        Ok(())
    }

    /// 收面（封注册）：Registering → Registered，要求用例数 > 0。
    pub fn finish_face(&mut self, face_id: u16) -> Result<u32, u16> {
        let idx = match self.find_face(face_id) {
            Some(i) => i,
            None => return Err(E_NO_FACE),
        };
        match self.faces[idx].state {
            FaceState::Registering => {}
            FaceState::Empty => return Err(E_NOT_REG),
            FaceState::Registered => return Err(E_NOT_REG),
            FaceState::Frozen => return Err(E_FROZEN),
        }
        if self.faces[idx].cases.is_empty() {
            return Err(E_EMPTY_FACE);
        }
        self.faces[idx].state = FaceState::Registered;
        Ok(self.faces[idx].cases.len() as u32)
    }

    /// 跑单面：Registered 才可跑，用例逐条回填 Pass（模型里断言全过由用例自身决定，
    /// 此处只有"跑到了"这一件事，裁决回填由 set_verdict 单独供 drill 注入失败面）。
    pub fn run_face(&mut self, face_id: u16) -> Result<RunReport, u16> {
        let idx = match self.find_face(face_id) {
            Some(i) => i,
            None => return Err(E_NO_FACE),
        };
        match self.faces[idx].state {
            FaceState::Registered => {}
            FaceState::Empty | FaceState::Registering => return Err(E_NOT_REG),
            FaceState::Frozen => return Err(E_FROZEN),
        }
        if self.faces[idx].cases.is_empty() {
            return Err(E_EMPTY_FACE);
        }
        let mut rep = RunReport::new();
        rep.faces = 1;
        let n = self.faces[idx].cases.len() as u32;
        rep.total = n;
        rep.ran = n;
        let mut i = 0usize;
        while i < self.faces[idx].cases.len() {
            match self.faces[idx].cases[i].verdict {
                CaseVerdict::Pass => rep.passed += 1,
                CaseVerdict::Fail => rep.failed += 1,
                CaseVerdict::Skip => rep.skipped += 1,
            }
            i += 1;
        }
        self.faces[idx].runs += 1;
        self.runs += 1;
        Ok(rep)
    }

    /// 一次命令跑全（J2 主面）：遍历全部已注册面，任一面相位不对整批拒。
    pub fn run_all(&mut self) -> Result<RunReport, u16> {
        if self.faces.is_empty() {
            return Err(E_NO_FACE);
        }
        // 先全量预检，避免跑到一半才发现某面未注册（半批失败会污染跑批账）
        let mut i = 0usize;
        while i < self.faces.len() {
            match self.faces[i].state {
                FaceState::Registered => {}
                FaceState::Frozen => return Err(E_FROZEN),
                _ => return Err(E_NOT_REG),
            }
            if self.faces[i].cases.is_empty() {
                return Err(E_EMPTY_FACE);
            }
            i += 1;
        }
        let mut rep = RunReport::new();
        let mut k = 0usize;
        while k < self.faces.len() {
            let n = self.faces[k].cases.len() as u32;
            rep.faces += 1;
            rep.total += n;
            rep.ran += n;
            let (p, f, s) = self.faces[k].tally();
            rep.passed += p;
            rep.failed += f;
            rep.skipped += s;
            self.faces[k].runs += 1;
            k += 1;
        }
        self.runs += 1;
        Ok(rep)
    }

    /// 裁决回填（drill 造失败面用；只允许在 Registered 面内改）。
    pub fn set_verdict(
        &mut self,
        face_id: u16,
        case_id: u16,
        v: CaseVerdict,
    ) -> Result<(), u16> {
        let idx = match self.find_face(face_id) {
            Some(i) => i,
            None => return Err(E_NO_FACE),
        };
        if self.faces[idx].state == FaceState::Frozen {
            return Err(E_FROZEN);
        }
        let mut i = 0usize;
        while i < self.faces[idx].cases.len() {
            if self.faces[idx].cases[i].case_id == case_id {
                self.faces[idx].cases[i].verdict = v;
                return Ok(());
            }
            i += 1;
        }
        Err(E_NO_FACE)
    }

    /// 冻结：Registered → Frozen（单向终态）。
    pub fn freeze(&mut self, face_id: u16) -> Result<(), u16> {
        let idx = match self.find_face(face_id) {
            Some(i) => i,
            None => return Err(E_NO_FACE),
        };
        match self.faces[idx].state {
            FaceState::Registered => {
                self.faces[idx].state = FaceState::Frozen;
                Ok(())
            }
            FaceState::Frozen => Err(E_FROZEN),
            _ => Err(E_NOT_REG),
        }
    }

    /// 注册账复核：注册数 == 面表长度，跑批数 == 各面 runs 之和（J5）。
    pub fn registers_balanced(&self) -> bool {
        if self.registers as usize != self.faces.len() {
            return false;
        }
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.faces.len() {
            s += self.faces[i].runs;
            i += 1;
        }
        s == self.runs
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_FACE_DUP => "同面号重复注册",
        E_CASE_DUP => "同面内用例号重复",
        E_NOT_REG => "面相位不对（未注册即加用例或跑批）",
        E_FROZEN => "面已冻结，拒一切变更与跑批",
        E_NO_FACE => "面号不存在（或闭集外）",
        E_NAME_EMPTY => "面名/用例名为空或用例号为 0",
        E_EMPTY_FACE => "空面跑批（无用例）",
        E_BAL => "注册/跑批账不平",
        _ => "未知诊断码",
    }
}

/// 用例名表：B04 面的 19 条判据用例（静态，不分配）。
const B04_CASES: [&str; 19] = [
    "F4075-J1-inode-compare-100",
    "F4075-J2-tamper-detect",
    "F4075-J3-missing-split",
    "F4076-J1-pseudo-isolated",
    "F4076-J2-tenk-zero-leak",
    "F4076-J3-no-reuse",
    "F4077-J1-eight-stats",
    "F4077-J2-snap-idempotent",
    "F4077-J3-change-visible",
    "F4078-J1-order-zero-violation",
    "F4078-J2-cross-100",
    "F4078-J3-legal-not-hit",
    "F4079-J1-register-once",
    "F4079-J2-one-command-all",
    "F4079-J3-case-dup-reject",
    "F4080-J1-19-criteria",
    "F4080-J2-rate-100",
    "F4080-J3-no-missing",
    "F4080-J4-green-gate",
];

/// 确定性演练：注册一次成功 → 一次命令跑全 → 重复注册拦截 → 账平。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：B04 面注册一次成功，相位 Empty→Registering，注册账 +1（J1 头一段）。
    let mut reg = KtestReg::new();
    let idx = reg.register_face(FACE_B04, "vfs-B04-inode-cache")?;
    if idx != 0 || reg.registers != 1 || reg.faces.len() != 1 {
        return Err(E_BAL);
    }
    if reg.faces[idx].state != FaceState::Registering {
        return Err(E_NOT_REG);
    }
    hits += 1;

    // R2：19 条用例逐条登记，面内号 1..19 唯一，表长 19（J3 正面）。
    let mut i = 0usize;
    while i < B04_CASES.len() {
        let kind = if i % 3 == 1 {
            CaseKind::Regress
        } else if i % 7 == 6 {
            CaseKind::Bench
        } else {
            CaseKind::Assert
        };
        reg.add_case(FACE_B04, (i + 1) as u16, B04_CASES[i], kind)?;
        i += 1;
    }
    if reg.faces[idx].cases.len() != 19 {
        return Err(E_BAL);
    }
    if reg.faces[idx].has_case(0) {
        return Err(E_CASE_DUP); // 0 号不许存在
    }
    hits += 1;

    // R3：收面 Registering→Registered，返回用例数 19。
    let n = reg.finish_face(FACE_B04)?;
    if n != 19 || reg.faces[idx].state != FaceState::Registered {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：裁决全 Pass 后一次命令跑全：ran==total==19，全覆盖且全绿（J2 主面）。
    let mut k = 1u16;
    while k <= 19 {
        reg.set_verdict(FACE_B04, k, CaseVerdict::Pass)?;
        k += 1;
    }
    let rep = reg.run_all()?;
    if rep.faces != 1 || rep.total != 19 || rep.ran != 19 {
        return Err(E_BAL);
    }
    if !rep.complete() || !rep.green() {
        return Err(E_BAL);
    }
    if rep.passed != 19 || rep.failed != 0 {
        return Err(E_BAL);
    }
    if !reg.registers_balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：失败面可见——回填一条 Fail 后重跑，绿判据必须转假（跑批不许掩败）。
    reg.set_verdict(FACE_B04, 7, CaseVerdict::Fail)?;
    let rep2 = reg.run_all()?;
    if rep2.failed != 1 || rep2.green() {
        return Err(E_BAL);
    }
    if rep2.passed != 18 {
        return Err(E_BAL);
    }
    reg.set_verdict(FACE_B04, 7, CaseVerdict::Pass)?;
    hits += 1;

    // R6：冻结后跑批（按本模块更严纪律）必须拒，且冻结不可重入。
    reg.freeze(FACE_B04)?;
    if reg.faces[idx].state != FaceState::Frozen {
        return Err(E_FROZEN);
    }
    if !matches!(reg.run_all(), Err(E_FROZEN)) {
        return Err(E_FROZEN);
    }
    if reg.freeze(FACE_B04) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;

    // I1：重复注册同面号 → E_FACE_DUP，注册账不增、面表长度不变（J1 主面）。
    let reg_before = reg.registers;
    let faces_before = reg.faces.len();
    if reg.register_face(FACE_B04, "vfs-B04-dup") != Err(E_FACE_DUP) {
        return Err(E_FACE_DUP);
    }
    if reg.registers != reg_before || reg.faces.len() != faces_before {
        return Err(E_BAL);
    }
    if reg.dup_rejects != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：重复用例号 → E_CASE_DUP，用例表长度不变；且拒后计数 +1（J3 负面）。
    let mut reg3 = KtestReg::new();
    reg3.register_face(FACE_B05, "vfs-B05-dentry")?;
    reg3.add_case(FACE_B05, 1, "B05-J1-dhash", CaseKind::Assert)?;
    let len_before = reg3.faces[0].cases.len();
    if reg3.add_case(FACE_B05, 1, "B05-J1-dup", CaseKind::Assert) != Err(E_CASE_DUP) {
        return Err(E_CASE_DUP);
    }
    if reg3.faces[0].cases.len() != len_before || reg3.case_rejects != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I3：未收面即跑 → E_NOT_REG；空面收面 → E_EMPTY_FACE。
    if !matches!(reg3.run_face(FACE_B05), Err(E_NOT_REG)) {
        return Err(E_NOT_REG);
    }
    let mut reg4 = KtestReg::new();
    reg4.register_face(FACE_B03, "vfs-B03-empty")?;
    if reg4.finish_face(FACE_B03) != Err(E_EMPTY_FACE) {
        return Err(E_EMPTY_FACE);
    }
    hits += 1;

    // I4：面号闭集外（0 / 6 / 65535）→ E_NO_FACE；空面名 → E_NAME_EMPTY。
    if reg4.register_face(0, "zero") != Err(E_NO_FACE) {
        return Err(E_NO_FACE);
    }
    if reg4.register_face(6, "over") != Err(E_NO_FACE) {
        return Err(E_NO_FACE);
    }
    if reg4.register_face(0xFFFF, "huge") != Err(E_NO_FACE) {
        return Err(E_NO_FACE);
    }
    if reg4.register_face(FACE_B01, "") != Err(E_NAME_EMPTY) {
        return Err(E_NAME_EMPTY);
    }
    hits += 1;

    // I5：空枢纽跑批 → E_NO_FACE；查不存在的面 → None。
    let mut reg5 = KtestReg::new();
    if !matches!(reg5.run_all(), Err(E_NO_FACE)) {
        return Err(E_NO_FACE);
    }
    if reg4.find_face(FACE_B02).is_some() {
        return Err(E_NO_FACE);
    }
    hits += 1;

    Ok(hits)
}
