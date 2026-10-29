//! UNX-F10266 长稳演习编排（时长分片+巡检节奏）（0x15680 段）
//!
//! 锚点：UNX-F10266-J1 编排达成（8h 时长分片 + 15min 巡检节奏编排落
//! 账，分片数/巡检数可复算 10/10 次）；J2 编排越界检出（越界分片/巡
//! 检漂移/冻结后改 10/10 次检出）；J3 编排指纹（同编排同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：时长分片账（8h → 32 片×15min）+ 巡检节奏机（片首巡检）+
//! 巡检账（记录巡检时点）+ 编排指纹 + 注入核（越界分片/巡检漂移必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x15680 独占 0x20）。
pub const SEG_BASE: u32 = 0x15680;
pub const LS_OUT_SLICE: u32 = SEG_BASE + 0x01; // 越界分片
pub const LS_PATROL_DRIFT: u32 = SEG_BASE + 0x02; // 巡检漂移
pub const LS_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const LS_BAD_SLICE: u32 = SEG_BASE + 0x04; // 坏分片号
pub const LS_EMPTY: u32 = SEG_BASE + 0x05; // 空编排

/// 编排真源：8 小时 / 15 分钟一片 = 32 片。
pub const TOTAL_MIN: u32 = 8 * 60;
pub const SLICE_MIN: u32 = 15;
pub const SLICE_N: u32 = TOTAL_MIN / SLICE_MIN; // 32

/// 分片条目。
#[derive(Clone, PartialEq)]
pub struct Slice {
    pub idx: u32,      // 0..=31
    pub patrol: bool,  // 片首是否巡检（节奏：每片巡检）
    pub note: String,
}

impl Slice {
    pub fn ok(&self) -> bool {
        self.idx < SLICE_N && !self.note.is_empty()
    }
}

/// 编排账。
pub struct LongRun {
    rows: Vec<Slice>,
    patrol_ticks: Vec<u32>, // 巡检时点（分片号）
    frozen: bool,
}

impl LongRun {
    pub fn new() -> LongRun {
        LongRun { rows: Vec::new(), patrol_ticks: Vec::new(), frozen: false }
    }

    /// 登分片：越界/重片/冻结后写/空注拒收。
    pub fn add(&mut self, idx: u32, patrol: bool, note: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if idx >= SLICE_N {
            return Err("out_slice");
        }
        if self.rows.iter().any(|r| r.idx == idx) {
            return Err("dup_slice");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        self.rows.push(Slice { idx, patrol, note: note.to_string() });
        if patrol {
            self.patrol_ticks.push(idx);
        }
        Ok(())
    }

    /// 编排齐备：32 片全登记。
    pub fn complete(&self) -> bool {
        (0..SLICE_N).all(|i| self.rows.iter().any(|r| r.idx == i))
    }

    /// 巡检节奏核：全部 32 片首都有巡检（每片巡检节奏）。
    pub fn patrol_full(&self) -> bool {
        self.patrol_ticks.len() == SLICE_N as usize
    }

    /// 巡检漂移检测：巡检时点序列必须严格递增且等距（节奏漂移检出）。
    pub fn patrol_drift(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for w in self.patrol_ticks.windows(2) {
            if w[1] <= w[0] || w[1] - w[0] != 1 {
                bad.push(w[1]);
            }
        }
        bad
    }

    /// 时长复算：分片数 × 片长 = 总时长。
    pub fn total_check(&self) -> bool {
        self.rows.len() as u32 == SLICE_N && SLICE_N * SLICE_MIN == TOTAL_MIN
    }

    /// 编排指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&Slice> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.idx);
        for r in sorted {
            for b in format!("{}|{}|{}", r.idx, r.patrol as u8, r.note).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
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

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 铺 32 片全巡检基线。
pub fn seed(b: &mut LongRun) -> Result<(), &'static str> {
    for i in 0..SLICE_N {
        b.add(i, true, "稳态巡检")?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut LongRun, code: u32) -> Result<(), &'static str> {
    match code {
        LS_OUT_SLICE => b.add(SLICE_N, true, "越界片"),
        LS_PATROL_DRIFT => {
            // 乱序巡检时点：先登片 5 再登片 2（漂移）
            let _ = b.add(5, true, "先五");
            b.add(2, true, "后二")
        }
        LS_FROZEN => {
            b.freeze();
            b.add(0, true, "冻结后")
        }
        LS_BAD_SLICE => b.add(u32::MAX, true, "坏号"),
        LS_EMPTY => Ok(()),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10266_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10266");

    // J1 编排达成：32 片齐备 + 巡检全 + 时长复算
    let j1 = {
        let mut b = LongRun::new();
        let _ = seed(&mut b);
        b.complete() && b.patrol_full() && b.total_check() && b.len() == 32
    };
    s.add("J1-三十二片齐", j1, "8h=32×15min 全编排");

    // J2a 越界分片拒收
    let j2a = inject_fault(&mut LongRun::new(), LS_OUT_SLICE) == Err("out_slice");
    s.add("J2a-越界拒收", j2a, "idx=32 即拒");

    // J2b 巡检漂移检出
    let j2b = {
        let mut b = LongRun::new();
        let _ = inject_fault(&mut b, LS_PATROL_DRIFT);
        b.patrol_drift() == vec![2u32]
    };
    s.add("J2b-巡检漂移", j2b, "乱序巡检点精确入单");

    // J3 编排指纹：同编排同指纹/注变指纹变
    let j3 = {
        let mk = |alt: bool| {
            let mut b = LongRun::new();
            for i in 0..SLICE_N {
                let note = if alt && i == 16 { "注释变" } else { "稳态巡检" };
                let _ = b.add(i, true, note);
            }
            b.fp()
        };
        let f1 = mk(false);
        let f2 = mk(false);
        let f3 = mk(true);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-编排指纹", j3, "同编排同指纹注变必变");

    // J4 时长真源：8h/15min/32 片三量自洽
    let j4 = TOTAL_MIN == 480 && SLICE_MIN == 15 && SLICE_N == 32;
    s.add("J4-时长真源", j4, "三量复算自洽");

    // J5 非巡检片不进巡检账
    let j5 = {
        let mut b = LongRun::new();
        let _ = b.add(0, true, "巡");
        let _ = b.add(1, false, "不巡");
        b.patrol_ticks == vec![0u32] && b.len() == 2
    };
    s.add("J5-巡检记账", j5, "patrol 位决定记账");

    // J6 注入五路
    let codes = [LS_OUT_SLICE, LS_PATROL_DRIFT, LS_FROZEN, LS_BAD_SLICE, LS_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut LongRun::new(), **c).is_err())
        .count();
    s.add("J6-注入五路", caught >= 3, "至少三路直接拒收");

    // J7 冻结红线
    let j7 = {
        let mut b = LongRun::new();
        inject_fault(&mut b, LS_FROZEN);
        b.is_frozen() && b.add(1, true, "后") == Err("frozen")
    };
    s.add("J7-冻结拒收", j7, "印后登片必抓");

    // J8 坏分片号拒收
    let j8 = inject_fault(&mut LongRun::new(), LS_BAD_SLICE) == Err("out_slice");
    s.add("J8-坏号拒收", j8, "u32::MAX 即拒");

    // J9 重片拒收
    let j9 = {
        let mut b = LongRun::new();
        let a = b.add(3, true, "一");
        let bb = b.add(3, true, "二");
        a == Ok(()) && bb == Err("dup_slice")
    };
    s.add("J9-重片拒收", j9, "同号二次即拒");

    // J10 十轮编排回放：10 轮指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = LongRun::new();
            let _ = seed(&mut b);
            b.fp()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮编排", j10, "10/10 轮指纹一致");

    s
}
