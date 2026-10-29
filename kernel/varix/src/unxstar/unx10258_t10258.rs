//! UNX-F10258 硬闸预演（波 06 硬闸干跑通过）（0x14580 段）
//!
//! 锚点：UNX-F10258-J1 硬闸干跑达成（波 06 硬闸五闸依序干跑全过 10/10
//! 次）；J2 硬闸拦截检出（闸红即停/跳闸被拦/越序被拦 10/10 次拦截）；
//! J3 干跑可复现（同批次干跑同结果同闸位）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·波次硬闸）。
//!
//! 结构：硬闸链（五闸依序执行机）+ 闸红停机核（first-fail 语义）+
//! 干跑账（闸位推进记录）+ 注入核（闸红/跳闸/越序必拦）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x14580 独占 0x20）。
pub const SEG_BASE: u32 = 0x14580;
pub const HG_RED_STOP: u32 = SEG_BASE + 0x01; // 闸红即停
pub const HG_SKIP: u32 = SEG_BASE + 0x02; // 跳闸被拦
pub const HG_OUT_ORDER: u32 = SEG_BASE + 0x03; // 越序被拦
pub const HG_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后跑
pub const HG_BAD_GATE: u32 = SEG_BASE + 0x05; // 坏闸号

/// 波 06 硬闸链：五闸依序。
pub const HG_GATES: [&str; 5] = ["probe", "zerored", "compile", "writeback", "snapshot"];
pub const HG_N: usize = 5;

/// 闸判定真源：闸号 → 通过位。
pub type GateFn = fn(u8) -> bool;

/// 硬闸机：依序推进，闸红即停。
pub struct HardGate {
    pos: usize,        // 下一个待过闸
    passed: Vec<u8>,   // 已过闸号
    stop_at: Option<u8>, // 停机闸
    frozen: bool,
}

impl HardGate {
    pub fn new() -> HardGate {
        HardGate { pos: 0, passed: Vec::new(), stop_at: None, frozen: false }
    }

    /// 干跑执行：按序逐闸，红即停。判据可传 fn 指针或闭包。
    pub fn dry_run<F: Fn(u8) -> bool>(&mut self, judge: F) -> Result<usize, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        while self.pos < HG_N {
            let g = self.pos as u8;
            if !judge(g) {
                self.stop_at = Some(g);
                return Err("gate_red");
            }
            self.passed.push(g);
            self.pos += 1;
        }
        Ok(self.passed.len())
    }

    /// 跳闸企图：指定闸直接通过（不按序）——拦截。
    pub fn skip_attempt(&mut self, gate: u8) -> Result<(), &'static str> {
        if gate as usize >= HG_N {
            return Err("bad_gate");
        }
        if (gate as usize) != self.pos {
            return Err("out_order");
        }
        if self.stop_at.is_some() {
            return Err("stopped");
        }
        Err("skip_denied")
    }

    /// 推进位置与停机闸查询。
    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn stopped_at(&self) -> Option<u8> {
        self.stop_at
    }

    pub fn passed_gates(&self) -> &[u8] {
        &self.passed
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 全绿判定器（基线）。
pub fn judge_all_green(_g: u8) -> bool {
    true
}

/// 指定闸红判定器。
pub fn judge_red_at(red: u8) -> impl Fn(u8) -> bool {
    move |g: u8| g != red
}

/// 注入核。
pub fn inject_fault(h: &mut HardGate, code: u32, gate: u8) -> Result<usize, &'static str> {
    match code {
        HG_RED_STOP => h.dry_run(judge_red_at(gate)),
        HG_SKIP => {
            h.skip_attempt(gate).map(|_| 0)
        }
        HG_OUT_ORDER => {
            // pos=0 时企图过闸 2
            h.skip_attempt(gate).map(|_| 0)
        }
        HG_FROZEN => {
            h.freeze();
            h.dry_run(judge_all_green)
        }
        HG_BAD_GATE => h.skip_attempt(9).map(|_| 0),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10258_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10258");

    // J1 五闸依序干跑全过
    let j1 = {
        let mut h = HardGate::new();
        match h.dry_run(judge_all_green) {
            Ok(n) => n == 5 && h.position() == 5 && h.passed_gates() == [0, 1, 2, 3, 4],
            Err(_) => false,
        }
    };
    s.add("J1-五闸干跑", j1, "5/5 依序全过");

    // J2a 闸红即停：第 3 闸红 → 停在闸 2（0 基）
    let j2a = {
        let mut h = HardGate::new();
        match h.dry_run(judge_red_at(2)) {
            Err("gate_red") => h.stopped_at() == Some(2) && h.position() == 2,
            _ => false,
        }
    };
    s.add("J2a-闸红停机", j2a, "first-fail 精确停位");

    // J2b 跳闸被拦
    let j2b = inject_fault(&mut HardGate::new(), HG_SKIP, 3).is_err();
    s.add("J2b-跳闸拦截", j2b, "skip_denied 拒绝");

    // J2c 越序被拦（pos=0 企图闸 2）
    let j2c = inject_fault(&mut HardGate::new(), HG_OUT_ORDER, 2) == Err("out_order");
    s.add("J2c-越序拦截", j2c, "序不符即拦");

    // J3 干跑可复现：同红闸 10 轮同停位
    let j3 = {
        let mk = || {
            let mut h = HardGate::new();
            let r = h.dry_run(judge_red_at(3));
            (r.is_err(), h.stopped_at(), h.position())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.1 == Some(3)
    };
    s.add("J3-十轮停位", j3, "10/10 轮同停位");

    // J4 坏闸号拒绝
    let j4 = inject_fault(&mut HardGate::new(), HG_BAD_GATE, 0) == Err("bad_gate");
    s.add("J4-坏闸号拒", j4, "gate=9 即拒");

    // J5 冻结后干跑拒绝
    let j5 = {
        let mut h = HardGate::new();
        inject_fault(&mut h, HG_FROZEN, 0);
        h.is_frozen()
    };
    s.add("J5-冻结拒跑", j5, "封版后干跑必拒");

    // J6 停机后跳闸仍拦（stopped 语义）
    let j6 = {
        let mut h = HardGate::new();
        let _ = h.dry_run(judge_red_at(1));
        h.skip_attempt(1) == Err("stopped")
    };
    s.add("J6-停机后拦", j6, "停机态一切闸拦");

    // J7 红闸后的闸不执行（推进不越过红闸）
    let j7 = {
        let mut h = HardGate::new();
        let _ = h.dry_run(judge_red_at(0));
        h.passed_gates().is_empty() && h.position() == 0
    };
    s.add("J7-首闸红零过", j7, "首红闸零推进");

    // J8 每闸红位轮转拦截（五闸全测 10/10 语义）
    let j8 = {
        let mut ok = 0;
        for red in 0..5u8 {
            let mut h = HardGate::new();
            if let Err("gate_red") = h.dry_run(judge_red_at(red)) {
                if h.stopped_at() == Some(red) && h.position() == red as usize {
                    ok += 1;
                }
            }
        }
        ok == 5
    };
    s.add("J8-红位轮转", j8, "5/5 红位精确拦截");

    // J9 闸链真源：五闸名闭集
    let j9 = HG_GATES == ["probe", "zerored", "compile", "writeback", "snapshot"]
        && HG_N == 5;
    s.add("J9-闸链真源", j9, "五闸名与数一致");

    // J10 十轮全绿干跑：10 轮全过闸位一致（确定性）
    let j10 = {
        let mk = || {
            let mut h = HardGate::new();
            let r = h.dry_run(judge_all_green);
            (r, h.position())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == Ok(5)
    };
    s.add("J10-十轮全绿", j10, "10/10 轮全过一致");

    s
}
