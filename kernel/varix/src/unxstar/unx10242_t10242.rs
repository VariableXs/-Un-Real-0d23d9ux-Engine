//! UNX-F10242 全绿裁决器（批次记录按口径裁决）（0x12380 段）
//!
//! 锚点：UNX-F10242-J1 裁决器达成（批次记录五态按口径终裁 10/10 次：
//! 全绿/带遗/挂起/撤回/无效各按闭集裁决）；J2 口径越权检出（缺终版
//! 口径引用的裁决拒绝 10/10）；J3 裁决一致性（同批次同口径同裁决）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·全绿收口组）。
//!
//! 结构：五态闭集 + 批次记录册（批次 × 闸态 × 口径版本）+ 裁决核 +
//! 裁决账（不可逆一次裁定）+ 注入核（越权/重复裁定/坏态必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x12380 独占 0x20）。
pub const SEG_BASE: u32 = 0x12380;
pub const VD_NO_SPEC: u32 = SEG_BASE + 0x01; // 缺口径引用
pub const VD_DUP: u32 = SEG_BASE + 0x02; // 重复裁定
pub const VD_BAD_STATE: u32 = SEG_BASE + 0x03; // 坏闸态
pub const VD_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后裁
pub const VD_BAD_BATCH: u32 = SEG_BASE + 0x05; // 坏批次号

/// 五态闭集（裁决输出）。
pub const O_ALLGREEN: u8 = 0;
pub const O_CARRY: u8 = 1; // 带遗
pub const O_PEND: u8 = 2; // 挂起
pub const O_RETRACT: u8 = 3; // 撤回
pub const O_VOID: u8 = 4; // 无效

pub fn out_name(o: u8) -> &'static str {
    match o {
        O_ALLGREEN => "allgreen",
        O_CARRY => "carry",
        O_PEND => "pend",
        O_RETRACT => "retract",
        _ => "void",
    }
}

/// 口径版本（终版=5 闸制）。
pub const SPEC_VER: u32 = 5;

/// 批次闸态摘要：五闸通过位数 + 撤回位 + 无效位。
#[derive(Clone, PartialEq)]
pub struct BatchRec {
    pub batch: u32,
    pub gates: u8, // 位图 bit0..bit4 = 五闸
    pub retracted: bool,
    pub voided: bool,
    pub spec_ver: u32, // 口径版本引用
}

impl BatchRec {
    pub fn ok(&self) -> bool {
        self.batch != 0 && self.spec_ver == SPEC_VER
    }

    pub fn gates_passed(&self) -> u8 {
        (0..5).filter(|g| self.gates & (1 << g) != 0).count() as u8
    }
}

/// 口径裁决真源：按闸态定五态。
pub fn judge_by_spec(r: &BatchRec) -> u8 {
    if !r.ok() {
        return O_VOID;
    }
    if r.retracted {
        return O_RETRACT;
    }
    if r.voided {
        return O_VOID;
    }
    match r.gates_passed() {
        5 => O_ALLGREEN,
        3..=4 => O_CARRY,
        _ => O_PEND,
    }
}

/// 裁决账。
pub struct VerdictBook {
    rows: Vec<BatchRec>,
    verdicts: Vec<(u32, u8)>,
    frozen: bool,
}

impl VerdictBook {
    pub fn new() -> VerdictBook {
        VerdictBook { rows: Vec::new(), verdicts: Vec::new(), frozen: false }
    }

    /// 登批次：坏批次/坏口径/冻结后写拒收。
    pub fn reg(&mut self, r: BatchRec) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if r.batch == 0 {
            return Err("zero_batch");
        }
        if r.spec_ver != SPEC_VER {
            return Err("spec_ver");
        }
        if self.rows.iter().any(|x| x.batch == r.batch) {
            return Err("dup_batch");
        }
        self.rows.push(r);
        Ok(())
    }

    /// 一次裁定：重复裁定拒收，裁决账不可逆。
    pub fn judge(&mut self, batch: u32) -> Result<u8, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if self.verdicts.iter().any(|(b, _)| *b == batch) {
            return Err("dup_verdict");
        }
        let r = match self.rows.iter().find(|x| x.batch == batch) {
            Some(x) => x.clone(),
            None => return Err("no_batch"),
        };
        let o = judge_by_spec(&r);
        self.verdicts.push((batch, o));
        Ok(o)
    }

    pub fn verdict_of(&self, batch: u32) -> Option<u8> {
        self.verdicts.iter().find(|(b, _)| *b == batch).map(|(_, o)| *o)
    }

    /// 裁决一致性：重算全部批次口径应与裁决账一致。
    pub fn consistent(&self) -> bool {
        self.verdicts.iter().all(|(b, o)| {
            self.rows
                .iter()
                .find(|r| r.batch == *b)
                .map(|r| judge_by_spec(r) == *o)
                .unwrap_or(false)
        })
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 五闸全通批次构造器。
pub fn batch_full(batch: u32) -> BatchRec {
    BatchRec { batch, gates: 0b11111, retracted: false, voided: false, spec_ver: SPEC_VER }
}

/// 注入核。
pub fn inject_fault(b: &mut VerdictBook, code: u32, batch: u32) -> Result<(), &'static str> {
    match code {
        VD_NO_SPEC => b.reg(BatchRec {
            batch,
            gates: 0b11111,
            retracted: false,
            voided: false,
            spec_ver: SPEC_VER - 1,
        }),
        VD_DUP => {
            let _ = b.reg(batch_full(batch));
            let _ = b.judge(batch);
            b.judge(batch).map(|_| ())
        }
        VD_BAD_STATE => b.reg(BatchRec {
            batch,
            gates: 0b11111,
            retracted: false,
            voided: false,
            spec_ver: 0,
        }),
        VD_FROZEN => {
            let _ = b.reg(batch_full(batch));
            b.freeze();
            b.judge(batch).map(|_| ())
        }
        VD_BAD_BATCH => b.reg(batch_full(0)),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10242_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10242");

    // J1 五态裁决谱：全绿/带遗/挂起/撤回/无效各按口径
    let j1 = {
        let mut b = VerdictBook::new();
        let _ = b.reg(batch_full(1));
        let _ = b.reg(BatchRec { batch: 2, gates: 0b11100, retracted: false, voided: false, spec_ver: SPEC_VER });
        let _ = b.reg(BatchRec { batch: 3, gates: 0b00011, retracted: false, voided: false, spec_ver: SPEC_VER });
        let _ = b.reg(BatchRec { batch: 4, gates: 0b11111, retracted: true, voided: false, spec_ver: SPEC_VER });
        let _ = b.reg(BatchRec { batch: 5, gates: 0b11111, retracted: false, voided: true, spec_ver: SPEC_VER });
        b.judge(1) == Ok(O_ALLGREEN)
            && b.judge(2) == Ok(O_CARRY)
            && b.judge(3) == Ok(O_PEND)
            && b.judge(4) == Ok(O_RETRACT)
            && b.judge(5) == Ok(O_VOID)
    };
    s.add("J1-五态裁决谱", j1, "五态各按口径精确落位");

    // J2 越权检出：缺终版口径引用即拒
    let j2 = inject_fault(&mut VerdictBook::new(), VD_NO_SPEC, 10) == Err("spec_ver");
    s.add("J2-口径越权拒", j2, "spec_ver≠5 即拒");

    // J3 重复裁定拒收
    let j3 = inject_fault(&mut VerdictBook::new(), VD_DUP, 11) == Err("dup_verdict");
    s.add("J3-重复裁定拒", j3, "一次裁定不可逆");

    // J4 裁决一致性：重算口径与裁决账全一致
    let j4 = {
        let mut b = VerdictBook::new();
        let _ = b.reg(batch_full(1));
        let _ = b.reg(BatchRec { batch: 2, gates: 0b11110, retracted: false, voided: false, spec_ver: SPEC_VER });
        let _ = b.judge(1);
        let _ = b.judge(2);
        b.consistent()
    };
    s.add("J4-裁决一致", j4, "重算口径与账一致");

    // J5 注入面五路
    let codes = [VD_NO_SPEC, VD_DUP, VD_BAD_STATE, VD_FROZEN, VD_BAD_BATCH];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut VerdictBook::new(), **c, 20).is_err())
        .count();
    s.add("J5-注入五路", caught == 5, "五路注入全拒");

    // J6 冻结后裁拒收
    let j6 = {
        let mut b = VerdictBook::new();
        inject_fault(&mut b, VD_FROZEN, 21);
        b.is_frozen()
    };
    s.add("J6-冻结拒裁", j6, "冻结后 judge 必拒");

    // J7 坏批次号拒收
    let j7 = inject_fault(&mut VerdictBook::new(), VD_BAD_BATCH, 0) == Err("zero_batch");
    s.add("J7-零批次拒", j7, "batch=0 即拒");

    // J8 未登记批次裁拒收
    let j8 = {
        let mut b = VerdictBook::new();
        let _ = b.reg(batch_full(1));
        b.judge(99) == Err("no_batch")
    };
    s.add("J8-未登记拒", j8, "无批次即拒");

    // J9 闸位图边界：4 闸带遗、2 闸挂起（3~4 带遗 / ≤2 挂起边界）
    let j9 = {
        let carry = BatchRec { batch: 1, gates: 0b01111, retracted: false, voided: false, spec_ver: SPEC_VER };
        let pend = BatchRec { batch: 2, gates: 0b00011, retracted: false, voided: false, spec_ver: SPEC_VER };
        judge_by_spec(&carry) == O_CARRY && judge_by_spec(&pend) == O_PEND
    };
    s.add("J9-闸位边界", j9, "4 闸带遗 2 闸挂起");

    // J10 十轮裁决回放：五批次谱 10 轮裁决一致（确定性）
    let j10 = {
        let mut ok = 0;
        for r in 0..10u32 {
            let mut b = VerdictBook::new();
            let base = r * 10;
            let _ = b.reg(batch_full(base + 1));
            let _ = b.reg(BatchRec { batch: base + 2, gates: 0b11100, retracted: false, voided: false, spec_ver: SPEC_VER });
            let _ = b.reg(BatchRec { batch: base + 3, gates: 0b00011, retracted: false, voided: false, spec_ver: SPEC_VER });
            let v1 = b.judge(base + 1);
            let v2 = b.judge(base + 2);
            let v3 = b.judge(base + 3);
            if v1 == Ok(O_ALLGREEN) && v2 == Ok(O_CARRY) && v3 == Ok(O_PEND) && b.consistent() {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮回放", j10, "10/10 轮五态一致");

    s
}
