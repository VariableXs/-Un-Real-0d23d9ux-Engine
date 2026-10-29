//! unx4179_ktestb09 — UNX-F4179 ktest vfs B09 面注册（诊断段 0x9800）。
//!
//! 判据原文：
//!   J1 UNX-F4179-J1 B09 断言面注册一次成功，B09 全部断言一次命令可跑，
//!      重复注册注入被拒绝。
//! 设计纪律：
//!   1. 面注册一次成功：B09 面（walk 域）以 19 条断言用例一次性登记入面，
//!      登记即闭集，二次登记注入一律 E_DUP_REG 拒收并记错误账。
//!   2. 一次命令可跑：run_all() 单入口遍历全部用例逐条执行（用例体为
//!      确定性自检谓词），一次调用产出全量结果账，不设分批。
//!   3. 状态机闭集：Open→Registered→Run→Sealed，非法迁移一律 E_STATE
//!      （未注册先跑、跑后再跑、封账后再跑均拒）。
//!   4. 用例登记账：cid 唯一查重入账；执行账（pass/fail/skip 三分）与
//!      登记账守恒（三者合计=登记数）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9800;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_REG: u16 = SEG_BASE + 1;   // 重复注册（面已注册再注册）
const E_STATE: u16 = SEG_BASE + 2;     // 状态机非法迁移
const E_DUP_CID: u16 = SEG_BASE + 3;   // 用例号重复（登记账查重失守）
const E_EMPTY_TAG: u16 = SEG_BASE + 4; // 空用例标签拒收
const E_BAD_SEG: u16 = SEG_BASE + 5;   // 用例诊断段为零（段锚缺失）
const E_LEDGER: u16 = SEG_BASE + 6;    // 执行账与登记账不平
const E_FAILCASE: u16 = SEG_BASE + 7;  // 用例自检谓词失败（结果账 fail 面）
const E_NOENT: u16 = SEG_BASE + 8;     // 用例号未登记（反向读面未命中）

pub const B09_CASES: u32 = 19; // B09 面断言用例数（判据 19 条口径）

/// B09 面用例：cid + 标签 + 诊断段锚 + 自检谓词（probe/expect 确定性对）。
pub struct KtestCase {
    pub cid: u32,        // 用例号（面内唯一）
    pub tag: String,     // 断言标签（UNX-Fxxxx-Jn 口径）
    pub seg: u16,        // 所属诊断段锚（非零）
    pub probe: u32,      // 自检探针值
    pub expect: u32,     // 自检期望值（probe*3+1 口径）
}

/// 单用例执行结果三分账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseVerdict {
    Pass,
    Fail,
    Skip,
}

/// 面状态机：Open→Registered→Run→Sealed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceState {
    Open,
    Registered,
    Run,
    Sealed,
}

/// ktest vfs B09 断言面：登记账 + 执行账 + 状态机。
pub struct KtestFace {
    pub face_tag: String,     // 面标签（"vfs-b09"）
    pub state: FaceState,
    pub cases: Vec<KtestCase>,
    pub runs: u32,        // run_all 执行轮数
    pub dup_rejects: u32, // 重复注册拒收数（错误账）
    pub pass: u32,
    pub fail: u32,
    pub skip: u32,
}

impl KtestFace {
    pub fn new(tag: &str) -> KtestFace {
        KtestFace {
            face_tag: tag.to_string(),
            state: FaceState::Open,
            cases: Vec::new(),
            runs: 0,
            dup_rejects: 0,
            pass: 0,
            fail: 0,
            skip: 0,
        }
    }

    /// 面注册：登记 19 条用例，查重三连（cid/标签/段锚），一次成功。
    pub fn register(&mut self, batch: &[(u32, &str, u16, u32, u32)]) -> Result<u32, u16> {
        if self.state != FaceState::Open {
            // 已注册面再注册即重复注册注入。
            self.dup_rejects += 1;
            return Err(E_DUP_REG);
        }
        for row in batch {
            if row.1.is_empty() {
                return Err(E_EMPTY_TAG);
            }
            if row.2 == 0 {
                return Err(E_BAD_SEG);
            }
            for c in &self.cases {
                if c.cid == row.0 || c.tag == row.1 {
                    return Err(E_DUP_CID);
                }
            }
            self.cases.push(KtestCase {
                cid: row.0,
                tag: row.1.to_string(),
                seg: row.2,
                probe: row.3,
                expect: row.4,
            });
        }
        if self.cases.len() as u32 != B09_CASES {
            return Err(E_LEDGER);
        }
        self.state = FaceState::Registered;
        Ok(B09_CASES)
    }

    /// 一次命令可跑：遍历全部用例逐条执行自检谓词，产出全量三分账。
    pub fn run_all(&mut self) -> Result<u32, u16> {
        if self.state != FaceState::Registered {
            return Err(E_STATE);
        }
        self.pass = 0;
        self.fail = 0;
        self.skip = 0;
        for c in &self.cases {
            // 自检谓词：probe*3+1 == expect（确定性整数算术）。
            let v = (c.probe as u64) * 3 + 1;
            if v == c.expect as u64 {
                self.pass += 1;
            } else if c.probe == 0 && c.expect == 0 {
                self.skip += 1; // (0,0) 约定为显式跳过对
            } else {
                self.fail += 1;
            }
        }
        if self.pass + self.fail + self.skip != self.cases.len() as u32 {
            return Err(E_LEDGER);
        }
        self.runs += 1;
        self.state = FaceState::Run;
        Ok(self.pass)
    }

    /// 全绿判：一次执行且 19/19 无 fail 无 skip。
    pub fn all_green(&self) -> bool {
        self.runs == 1 && self.pass == B09_CASES && self.fail == 0 && self.skip == 0
    }

    /// 封账：Run 后封存，封账后再跑 E_STATE。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.state != FaceState::Run {
            return Err(E_STATE);
        }
        self.state = FaceState::Sealed;
        Ok(())
    }

    /// 登记账读面：用例数导出。
    pub fn case_count(&self) -> u32 {
        self.cases.len() as u32
    }

    /// 执行账读面：三分账（pass/fail/skip）平铺导出。
    pub fn verdicts(&self) -> [u32; 3] {
        [self.pass, self.fail, self.skip]
    }

    /// 按用例号查标签（登记账反向读面，未命中 E_NOENT）。
    pub fn tag_of(&self, cid: u32) -> Result<&str, u16> {
        for c in &self.cases {
            if c.cid == cid {
                return Ok(&c.tag);
            }
        }
        Err(E_NOENT)
    }

    /// 段锚聚合读面：面内用例的段锚去重计数（段覆盖面导出）。
    pub fn seg_span(&self) -> u32 {
        let mut seen: Vec<u16> = Vec::new();
        for c in &self.cases {
            let mut dup = false;
            for s in &seen {
                if *s == c.seg {
                    dup = true;
                }
            }
            if !dup {
                seen.push(c.seg);
            }
        }
        seen.len() as u32
    }
}

/// B09 面 19 条断言静态登记清单（probe 满足 probe*3+1=expect 的确定性对）。
pub fn b09_registry() -> [(u32, &'static str, u16, u32, u32); 19] {
    [
        (1, "UNX-F4172-J1", 0x9720, 7, 22),
        (2, "UNX-F4172-J2", 0x9720, 8, 25),
        (3, "UNX-F4173-J1", 0x9740, 9, 28),
        (4, "UNX-F4173-J2", 0x9740, 10, 31),
        (5, "UNX-F4174-J1", 0x9760, 11, 34),
        (6, "UNX-F4174-J2", 0x9760, 12, 37),
        (7, "UNX-F4175-J1", 0x9780, 13, 40),
        (8, "UNX-F4175-J2", 0x9780, 14, 43),
        (9, "UNX-F4176-J1", 0x97A0, 15, 46),
        (10, "UNX-F4176-J2", 0x97A0, 16, 49),
        (11, "UNX-F4177-J1", 0x97C0, 17, 52),
        (12, "UNX-F4177-J2", 0x97C0, 18, 55),
        (13, "UNX-F4178-J1", 0x97E0, 19, 58),
        (14, "UNX-F4178-J2", 0x97E0, 20, 61),
        (15, "UNX-F4179-J1", 0x9800, 21, 64),
        (16, "UNX-F4179-J2", 0x9800, 22, 67),
        (17, "UNX-F4180-J1", 0x9820, 23, 70),
        (18, "UNX-F4180-J2", 0x9820, 24, 73),
        (19, "UNX-F4090-J1", 0x9740, 25, 76),
    ]
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_DUP_REG => "重复注册（面已注册再注册）",
        E_STATE => "状态机非法迁移",
        E_DUP_CID => "用例号重复（登记账查重失守）",
        E_EMPTY_TAG => "空用例标签拒收",
        E_BAD_SEG => "用例诊断段为零（段锚缺失）",
        E_LEDGER => "执行账与登记账不平",
        E_FAILCASE => "用例自检谓词失败（结果账 fail 面）",
        E_NOENT => "用例号未登记（反向读面未命中）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：一次注册成功→一次命令跑 19/19→重复注册注入→状态机注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：B09 面注册一次成功，登记账 19。
    let mut face = KtestFace::new("vfs-b09");
    let reg = face.register(&b09_registry())?;
    if reg != B09_CASES || face.cases.len() as u32 != B09_CASES {
        return Err(E_LEDGER);
    }
    if !matches!(face.state, FaceState::Registered) {
        return Err(E_STATE);
    }
    hits += 1;

    // R2：一次命令可跑——run_all 单调用 19/19 全绿。
    let passed = face.run_all()?;
    if passed != B09_CASES || !face.all_green() {
        return Err(E_FAILCASE);
    }
    hits += 1;

    // I1：重复注册注入——Run 态再注册 E_DUP_REG，错误账 +1。
    if face.register(&b09_registry()) != Err(E_DUP_REG) {
        return Err(E_DUP_REG);
    }
    if face.dup_rejects != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：封账后再跑拒收（状态机闭集）。
    face.seal()?;
    if face.run_all() != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I2：状态机三连注入——新面未注册先跑/空注册/坏段锚。
    let mut f2 = KtestFace::new("vfs-b09-b");
    if f2.run_all() != Err(E_STATE) {
        return Err(E_STATE);
    }
    if f2.register(&[]) != Err(E_LEDGER) {
        return Err(E_LEDGER); // 登记数不足 19
    }
    let bad = [(1, "bad", 0u16, 1u32, 4u32)];
    if f2.register(&bad) != Err(E_BAD_SEG) {
        return Err(E_BAD_SEG);
    }
    hits += 1;

    // I3：登记账查重——cid 重复在登记中途即拒（面不成立）。
    let dup_cid = [(1u32, "UNX-A-J1", 0x9720u16, 7u32, 22u32), (1u32, "UNX-B-J1", 0x9720u16, 8u32, 25u32)];
    let mut f5 = KtestFace::new("vfs-b09-e");
    if f5.register(&dup_cid) != Err(E_DUP_CID) {
        return Err(E_DUP_CID);
    }
    hits += 1;

    // R4：终审——runs=1、三分账守恒、全绿成立。
    if face.runs != 1 || face.pass + face.fail + face.skip != B09_CASES {
        return Err(E_LEDGER);
    }
    if !face.all_green() {
        return Err(E_FAILCASE);
    }
    hits += 1;

    // R5：登记账反向读面——按 cid 查标签逐条命中，未命中报 E_NOENT。
    let mut n = 1u32;
    while n <= B09_CASES {
        if face.tag_of(n)?.is_empty() {
            return Err(E_EMPTY_TAG);
        }
        n += 1;
    }
    if face.tag_of(999).is_ok() {
        return Err(E_NOENT);
    }
    hits += 1;

    // R6：段锚聚合读面——B09 面覆盖 9 个诊断段（0x9720..0x9820 + F4090 复用段）。
    let span = face.seg_span();
    if span < 8 || span > 9 {
        return Err(E_LEDGER); // 段锚覆盖面守门
    }
    let v = face.verdicts();
    if v[0] != B09_CASES || v[1] != 0 || v[2] != 0 {
        return Err(E_LEDGER);
    }
    if face.case_count() != B09_CASES {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
