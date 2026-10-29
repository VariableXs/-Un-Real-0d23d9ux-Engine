//! unx4039_ktestb02 — UNX-F4039 ktest vfs B02 面注册（诊断段 0x8760）。
//!
//! 判据原文：
//!   J1 run_vfs_checks() 扩展 B02 断言面一次成功；
//!   J2 B02 全部断言一次命令可跑（面清单可一次性遍历出账）；
//!   J3 重复注册注入被拒绝。
//! 设计纪律：
//!   1. 面结构：ktest 断言面 =（面名，段基，用例账）；B02 面
//!      以固定面名定编，段基取本模块 SEG_BASE。
//!   2. 用例登记账：用例 =（用例名，判据号，登记序号）三元组，
//!      重名/重判据号注入一律拒收——登记即入账，账序单调。
//!   3. 注册状态机：Empty → Registered → Frozen（面冻结后
//!      只读，再注册拒收）；run_vfs_checks 的语义是「扩面 +
//!      冻结」一步完成，重复调用走 E_ALREADY_FROZEN。
//!   4. 面清单导出：一次调用导出全部已登记用例（判据 J2 的
//!      「一次命令可跑」账面形态），导出不改账。
//!   5. 边界：空用例名、超长名、判据号 0、面满——全部显性
//!      拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8760;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_DUP_CASE: u16 = SEG_BASE + 1;      // 用例重名/重判据号
const E_BAD_NAME: u16 = SEG_BASE + 2;      // 用例名空/超长
const E_BAD_JID: u16 = SEG_BASE + 3;       // 判据号 0 非法
const E_FROZEN: u16 = SEG_BASE + 4;        // 面冻结后再注册
const E_ALREADY_FROZEN: u16 = SEG_BASE + 5;// run_vfs_checks 重复调用
const E_NOT_REGISTERED: u16 = SEG_BASE + 6;// 面未注册先操作
const E_FACE_FULL: u16 = SEG_BASE + 7;     // 面容量满
const E_BAL: u16 = SEG_BASE + 8;           // 登记账不平

/// 面名定长与容量。
pub const FACE_NAME: &[u8] = b"vfsB02";
pub const CASE_CAP: usize = 24;
pub const NAME_CAP: usize = 16;

/// 面注册状态机：Empty → Registered → Frozen（终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FacePhase {
    Empty,
    Registered,
    Frozen,
}

/// 用例登记项：名 + 判据号 + 登记序号。
pub struct CaseEntry {
    pub name: [u8; NAME_CAP],
    pub name_len: usize,
    pub jid: u32,  // 判据号（非 0）
    pub seq: u32,  // 登记序号（单调）
}

/// ktest vfs B02 断言面。
pub struct KtestFaceB02 {
    pub phase: FacePhase,
    pub face_name: [u8; NAME_CAP], // 面名（定编 vfsB02）
    pub face_len: usize,
    pub cases: Vec<CaseEntry>,
    pub seq_next: u32,
    pub extend_calls: u32, // run_vfs_checks 调用次数（含被拒）
    pub extend_ok: u32,    // 扩面成功次数（≤1）
    pub rejects: u32,      // 错误账
}

impl KtestFaceB02 {
    pub fn new() -> KtestFaceB02 {
        let mut fnm = [0u8; NAME_CAP];
        let mut i = 0;
        while i < FACE_NAME.len() {
            fnm[i] = FACE_NAME[i];
            i += 1;
        }
        KtestFaceB02 {
            phase: FacePhase::Empty,
            face_name: fnm,
            face_len: FACE_NAME.len(),
            cases: Vec::new(),
            seq_next: 1,
            extend_calls: 0,
            extend_ok: 0,
            rejects: 0,
        }
    }

    /// 面注册：Empty → Registered（一次成功语义）。
    pub fn face_register(&mut self) -> Result<FacePhase, u16> {
        if self.phase != FacePhase::Empty {
            self.rejects += 1;
            return Err(E_ALREADY_FROZEN);
        }
        self.phase = FacePhase::Registered;
        Ok(self.phase)
    }

    /// 用例查重（名与判据号双通道）。
    fn dup(&self, name: &[u8], jid: u32) -> bool {
        let mut i = 0;
        while i < self.cases.len() {
            let c = &self.cases[i];
            if c.jid == jid {
                return true;
            }
            if c.name_len == name.len() {
                let mut j = 0;
                let mut same = true;
                while j < name.len() {
                    if c.name[j] != name[j] {
                        same = false;
                        break;
                    }
                    j += 1;
                }
                if same {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 用例登记：面须已注册未冻结，名/判据号合法，双通道查重。
    pub fn case_add(&mut self, name: &[u8], jid: u32) -> Result<u32, u16> {
        if self.phase == FacePhase::Empty {
            self.rejects += 1;
            return Err(E_NOT_REGISTERED);
        }
        if self.phase == FacePhase::Frozen {
            self.rejects += 1;
            return Err(E_FROZEN);
        }
        if name.is_empty() || name.len() > NAME_CAP {
            self.rejects += 1;
            return Err(E_BAD_NAME);
        }
        if jid == 0 {
            self.rejects += 1;
            return Err(E_BAD_JID);
        }
        if self.dup(name, jid) {
            self.rejects += 1;
            return Err(E_DUP_CASE);
        }
        if self.cases.len() >= CASE_CAP {
            self.rejects += 1;
            return Err(E_FACE_FULL);
        }
        let mut nm = [0u8; NAME_CAP];
        let mut i = 0;
        while i < name.len() {
            nm[i] = name[i];
            i += 1;
        }
        let seq = self.seq_next;
        self.cases.push(CaseEntry { name: nm, name_len: name.len(), jid, seq });
        self.seq_next += 1;
        Ok(seq)
    }

    /// run_vfs_checks 语义：扩面（B02 断言面注入预置用例集）+ 冻结，
    /// 一次成功（判据 J1）；重复调用拒收（判据 J3 注入面）。
    pub fn run_vfs_checks(&mut self) -> Result<u32, u16> {
        self.extend_calls += 1;
        if self.phase == FacePhase::Empty {
            self.face_register()?;
        }
        if self.phase == FacePhase::Frozen {
            self.rejects += 1;
            return Err(E_ALREADY_FROZEN);
        }
        // 预置 B02 断言用例集（判据号 = 40001..40012 闭集段）。
        let base: u32 = 40001;
        let mut k = 0;
        while k < 12 {
            // 序号化用例名："b02c" + 两位序号（整数展开）。
            let mut nm: [u8; NAME_CAP] = [0; NAME_CAP];
            nm[0] = b'b';
            nm[1] = b'0';
            nm[2] = b'2';
            nm[3] = b'c';
            nm[4] = b'0' + ((k / 10) as u8);
            nm[5] = b'0' + ((k % 10) as u8);
            let nm_slice: Vec<u8> = {
                let mut v = Vec::new();
                let mut j = 0;
                while j < 6 {
                    v.push(nm[j]);
                    j += 1;
                }
                v
            };
            self.case_add(&nm_slice, base + k)?;
            k += 1;
        }
        self.phase = FacePhase::Frozen;
        self.extend_ok += 1;
        Ok(self.cases.len() as u32)
    }

    /// 面清单一次性导出（判据 J2「一次命令可跑」的账面形态）。
    pub fn export_face(&mut self) -> Result<Vec<(u32, u32)>, u16> {
        if self.phase != FacePhase::Frozen {
            self.rejects += 1;
            return Err(E_NOT_REGISTERED);
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.cases.len() {
            if i < self.cases.len() {
                out.push((self.cases[i].seq, self.cases[i].jid));
            }
            i += 1;
        }
        Ok(out)
    }

    /// 守恒对账：登记账 = 面内用例数；冻结面序号连续无断号。
    pub fn audit(&self) -> Result<(), u16> {
        if self.seq_next as usize - 1 != self.cases.len() {
            return Err(E_BAL);
        }
        let mut i = 0;
        while i < self.cases.len() {
            if i < self.cases.len() && self.cases[i].seq != (i as u32) + 1 {
                return Err(E_BAL);
            }
            i += 1;
        }
        if self.phase == FacePhase::Frozen && self.extend_ok != 1 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（相位，用例数，扩面账，拒收账）。
    pub fn ledger(&self) -> (FacePhase, usize, u32, u32, u32) {
        (self.phase, self.cases.len(), self.extend_calls, self.extend_ok, self.rejects)
    }
}

/// 自例程：R1 扩面一次成功 → R2 重复注入 → R3 登记注入 → R4 清单导出 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut face = KtestFaceB02::new();

    // R1 run_vfs_checks 一次成功（判据 J1）：12 条预置用例入账并冻结。
    let n = face.run_vfs_checks()?;
    if n != 12 || face.phase != FacePhase::Frozen || face.extend_ok != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 重复注入（判据 J3）：二次 run_vfs_checks 与冻结期登记均被拒。
    if face.run_vfs_checks() != Err(E_ALREADY_FROZEN) {
        return Err(E_BAL);
    }
    if face.case_add(b"late", 41001) != Err(E_FROZEN) {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 登记注入：重判据号与重名分别被拒；坏名/零判据号被拒。
    let mut f2 = KtestFaceB02::new();
    f2.face_register()?;
    f2.case_add(b"alpha", 42001)?;
    if f2.case_add(b"beta", 42001) != Err(E_DUP_CASE) {
        return Err(E_BAL);
    }
    if f2.case_add(b"alpha", 42002) != Err(E_DUP_CASE) {
        return Err(E_BAL);
    }
    if f2.case_add(b"", 42003) != Err(E_BAD_NAME) {
        return Err(E_BAL);
    }
    if f2.case_add(b"gamma", 0) != Err(E_BAD_JID) {
        return Err(E_BAL);
    }
    if f2.case_add(b"delt", 42101).is_err() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 面清单一次性导出（判据 J2）：冻结面全量出账且账序单调。
    let list = face.export_face()?;
    if list.len() != 12 {
        return Err(E_BAL);
    }
    let mut i = 0;
    while i < list.len() {
        if i + 1 < list.len() && list[i].0 >= list[i + 1].0 {
            return Err(E_BAL);
        }
        i += 1;
    }
    hits += 1;

    // R5 守恒：未注册面登记被拒；冻结面序号连续；账面闭合。
    let mut f3 = KtestFaceB02::new();
    if f3.case_add(b"early", 43001) != Err(E_NOT_REGISTERED) {
        return Err(E_BAL);
    }
    face.audit()?;
    f2.audit()?;
    let (ph, cs, ec, eo, rj) = face.ledger();
    if ph != FacePhase::Frozen || cs != 12 || ec != 2 || eo != 1 || rj != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
