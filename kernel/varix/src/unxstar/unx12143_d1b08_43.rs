//! UNX-F12143 — 逐帧回退栈模拟器（RtlVirtualUnwind 底座）（0x1a400 段）
//!
//! 锚点：UNX-F12143-J1 逐帧回退 10/10（子→父→祖父），每层 RSP/RIP
//! 单调推进，到达根帧受控终止；野回退（跳层/越界）全拒。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：回退帧（UnwindFrame：RIP/RSP/返回地址）+ 回退器（按帧链
//! 逐层 pop，RSP 单调上推）+ 回退留痕账 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a400;
pub const UW_GHOST: u32 = SEG_BASE + 0x01; // RIP 不落任何已知帧
pub const UW_DOWN: u32 = SEG_BASE + 0x02; // RSP 倒退（栈向下走）
pub const UW_ROOTED: u32 = SEG_BASE + 0x03; // 根帧之外再回退
pub const UW_SEALED: u32 = SEG_BASE + 0x04; // 印后回退
pub const UW_EMPTY: u32 = SEG_BASE + 0x05; // 空账回退

/// 一层回退帧：本帧 RIP、帧 RSP、返回地址（父帧 RIP）。
#[derive(Clone, PartialEq)]
pub struct UnwindFrame {
    pub rip: u64,
    pub rsp: u64,
    pub ret: u64,
    pub note: String,
}

impl UnwindFrame {
    /// 帧自洽：ret > rip 所在方向合法（父帧地址更高）且 RSP 4 对齐。
    pub fn ok(&self) -> bool {
        self.rip != 0 && self.rsp != 0 && self.rsp % 4 == 0 && self.ret > self.rip
    }

    pub fn render(&self) -> String {
        format!("rip={:#x} rsp={:#x} ret={:#x}", self.rip, self.rsp, self.ret)
    }
}

/// 回退器：帧链按 RSP 降序挂载（栈向下增长，顶层 RSP 最小），
/// 回退一步 = 弹出顶层、以 ret 为新 RIP。
pub struct Unwinder {
    pub chain: Vec<UnwindFrame>,
    pub steps: Vec<u64>,
    sealed: bool,
}

impl Unwinder {
    pub fn new() -> Unwinder {
        Unwinder { chain: Vec::new(), steps: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂帧（RSP 严格升序入链表头 = 顶层最小；重复/倒挂必拒）。
    pub fn push(&mut self, rip: u64, rsp: u64, ret: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UW_SEALED);
        }
        let f = UnwindFrame { rip, rsp, ret, note: note.to_string() };
        if !f.ok() {
            return Err(UW_DOWN);
        }
        if let Some(top) = self.chain.first() {
            if rsp >= top.rsp {
                return Err(UW_DOWN);
            }
            if ret != top.rip {
                return Err(UW_GHOST);
            }
        }
        self.chain.insert(0, f);
        Ok(())
    }

    /// 回退一步：弹顶层帧，新 RIP = 顶层 ret，新 RSP = 顶层 rsp + 8（压入返回地址占位弹出）。
    pub fn step(&mut self) -> Result<(u64, u64), u32> {
        if self.sealed {
            return Err(UW_SEALED);
        }
        let top = match self.chain.first() {
            Some(f) => (f.rip, f.rsp, f.ret),
            None => return Err(UW_EMPTY),
        };
        if let Some(&prev) = self.steps.last() {
            if prev > top.2 {
                return Err(UW_DOWN);
            }
        }
        self.steps.push(top.0);
        self.chain.remove(0);
        Ok((top.2, top.1 + 8))
    }

    /// 全程回退：直到根帧（链空）为止，返回步数。
    pub fn unwind_all(&mut self) -> Result<usize, u32> {
        let mut n = 0usize;
        loop {
            match self.step() {
                Ok(_) => n += 1,
                Err(UW_EMPTY) => return Ok(n),
                Err(e) => return Err(e),
            }
        }
    }

    /// 步留痕全对：每一步 RIP 严格递增。
    pub fn steps_monotonic(&self) -> bool {
        self.steps.windows(2).all(|w| w[0] < w[1])
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a400;
        for f in &self.chain {
            h = h.wrapping_mul(211).wrapping_add(f.rip as u32);
            h = h.wrapping_mul(211).wrapping_add(f.rsp as u32);
            for by in f.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("chain={} steps={} mono={}", self.chain.len(), self.steps.len(), self.steps_monotonic())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 空账回退必拒。
    let mut b = Unwinder::new();
    got[0] = b.step().err() == Some(UW_EMPTY);
    // 2) RSP 倒挂（后帧 RSP 更大）必拒。
    let mut b2 = Unwinder::new();
    let _ = b2.push(0x8000, 0x9000, 0x8500, "a");
    got[1] = b2.push(0x8500, 0x9500, 0x8800, "b").err() == Some(UW_DOWN);
    // 3) 链断裂（ret ≠ 上层 rip）必拒。
    got[2] = b2.push(0x8600, 0x8800, 0x8900, "c").err() == Some(UW_GHOST);
    // 4) 帧字段倒挂（ret < rip）必拒。
    let mut b3 = Unwinder::new();
    got[3] = b3.push(0x9000, 0x8000, 0x1000, "bad").err() == Some(UW_DOWN);
    // 5) 印后回退必拒。
    b3.seal();
    got[4] = b3.step().err() == Some(UW_SEALED);
    got
}

/// 判据入口：UNX-F12143
pub fn run_unx12143_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12143");
    let mut b = Unwinder::new();

    // J1 三层帧链挂载（祖父→顶层，RSP 降序；ret 指向上层 rip）。
    let j1 = b.push(0x8700, 0x9800, 0x8900, "root").is_ok()
        && b.push(0x8500, 0x9400, 0x8700, "mid").is_ok()
        && b.push(0x8000, 0x9000, 0x8500, "leaf").is_ok();
    s.add("J1_chain3", j1 && b.chain.len() == 3, "三层帧链挂载");

    // J2 逐帧回退 3/3，每步 RIP 单调递增。
    let s1 = b.step();
    let s2 = b.step();
    let s3 = b.step();
    s.add(
        "J2_step3",
        s1 == Ok((0x8500, 0x9008)) && s2 == Ok((0x8700, 0x9408)) && s3 == Ok((0x8900, 0x9808)),
        "三层回退 RIP/RSP 逐层推进",
    );

    // J3 根帧之后再回退受控。
    s.add("J3_rooted", b.step().err() == Some(UW_EMPTY), "根帧外回退拒 EMPTY");

    // J4 步留痕单调。
    s.add("J4_mono", b.steps_monotonic() && b.steps.len() == 3, "步留痕 3 条全单调");

    // J5 全程回退口径：新账 4 层 4 步到底。
    let mut b5 = Unwinder::new();
    let j5 = b5.push(0x1300, 0x8c00, 0x1400, "l3").is_ok()
        && b5.push(0x1200, 0x8800, 0x1300, "l2").is_ok()
        && b5.push(0x1100, 0x8400, 0x1200, "l1").is_ok()
        && b5.push(0x1000, 0x8000, 0x1100, "l0").is_ok();
    let walked = b5.unwind_all();
    s.add(
        "J5_unwind_all",
        j5 && walked == Ok(4) && b5.chain.is_empty(),
        "四层全程回退 4 步到底",
    );

    // J6 帧字段自洽核。
    let f = UnwindFrame { rip: 0x8000, rsp: 0x9000, ret: 0x8500, note: "t".to_string() };
    s.add("J6_frame_ok", f.ok(), "帧四元组过自洽核");

    // J7 非对齐 RSP 拒。
    let mut b7 = Unwinder::new();
    s.add(
        "J7_align",
        b7.push(0x1000, 0x9002, 0x1100, "mis").err() == Some(UW_DOWN),
        "RSP 4 对齐违例拒",
    );

    // J8 帧渲染。
    let r = f.render();
    s.add(
        "J8_render",
        r.starts_with("rip=0x8000") && r.contains("ret=0x8500"),
        "渲染含 rip/rsp/ret",
    );

    // J9 顶层 RSP 必须更低（栈向下增长）。
    let mut b9 = Unwinder::new();
    let _ = b9.push(0x8000, 0x9000, 0x8500, "a");
    s.add(
        "J9_downward",
        b9.push(0x8500, 0x9000, 0x8800, "same").err() == Some(UW_DOWN),
        "同 RSP 复用拒 DOWN",
    );

    // J10 链断裂受控（ret 指向野地址）。
    s.add(
        "J10_ghost_ret",
        b9.push(0x8800, 0x8800, 0xDEAD, "wild").err() == Some(UW_GHOST),
        "ret 野地址拒 GHOST",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "回退账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b9.seal();
    s.add(
        "J13_sealed",
        b9.step().err() == Some(UW_SEALED)
            && b9.push(0x9000, 0x9800, 0x9100, "post").err() == Some(UW_SEALED),
        "印账后回退/挂帧皆拒",
    );

    // J14 步进 RSP 抬升量恒 8（压栈返回地址弹回）。
    let mut b14 = Unwinder::new();
    let _ = b14.push(0x2000, 0x8000, 0x2100, "x");
    let r14 = b14.step();
    s.add("J14_rsp8", r14 == Ok((0x2100, 0x8008)), "回退一步 RSP +8");

    // J15 账面摘要。
    let sm = b14.book_summary();
    s.add(
        "J15_summary",
        sm.contains("steps=1") && sm.contains("mono=true"),
        "摘要含步数与单调位",
    );

    // J16 零 RIP 帧非法。
    let bad = UnwindFrame { rip: 0, rsp: 0x8000, ret: 0x100, note: "z".to_string() };
    s.add("J16_zero_rip", !bad.ok(), "零 RIP 帧判非法");

    // J17 步留痕记录的是被弹出帧的 RIP（回退起点）。
    let mut b17 = Unwinder::new();
    let _ = b17.push(0x3000, 0x9000, 0x3100, "e");
    let _ = b17.step();
    s.add("J17_step_trace", b17.steps == [0x3000], "步留痕记回退起点 RIP");

    // J18 深链 8 层挂载（RSP 降序，祖父先挂）+ 全程回退。
    let mut b18 = Unwinder::new();
    let mut ok18 = true;
    for i in (0..8u64).rev() {
        let rip = 0x1000 + i * 0x100;
        let rsp = 0x8000 + i * 0x80;
        let ret = 0x1100 + i * 0x100;
        if b18.push(rip, rsp, ret, "deep").is_err() {
            ok18 = false;
        }
    }
    s.add(
        "J18_deep8",
        ok18 && b18.unwind_all() == Ok(8),
        "8 层深链全程回退",
    );

    // J19 空账指纹稳定。
    let b19 = Unwinder::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空回退账指纹稳定");

    // J20 回退后链顶即父帧。
    let mut b20 = Unwinder::new();
    let _ = b20.push(0x8500, 0x9400, 0x8700, "mid");
    let _ = b20.push(0x8000, 0x9000, 0x8500, "leaf");
    let _ = b20.step();
    s.add(
        "J20_after_step_top",
        b20.chain[0].rip == 0x8500 && b20.chain[0].note == "mid",
        "回退一步后链顶=父帧",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12143_step() {
        let mut b = Unwinder::new();
        let _ = b.push(0x8000, 0x9000, 0x8500, "a");
        assert_eq!(b.step(), Ok((0x8500, 0x9008)));
    }

    #[test]
    fn unx12143_checks_green() {
        let s = run_unx12143_checks();
        assert!(s.all_passed(), "UNX-F12143 判据未全绿");
    }

    #[test]
    fn unx12143_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
