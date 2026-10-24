use alloc::vec;
// unx3542 —— UNX-F3542 采样栈回溯采集（帧指针回溯——OTP 回溯降级）
// 判据原文 J1：回溯语义（fp 链逐帧上溯——帧界校验失败即停）；
// M-300 回溯正确性（已知调用树函数栈回溯逐帧一致）。
// J2：降级声明（OTP = 一次性回溯深度上限——烂帧即断不猜）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xFD80 独占（0xFD80~0xFDFF）。
//
// 设计（纯功能 no_std）：
//   * fp 链模型：每帧（fp, ra, fn_id），回溯沿 fp 逐帧上溯；
//   * 帧界校验：fp 单调递增（栈向下生长 → 帧地址上溯递增）、帧距界内、
//     ra 落在代码段内——任一失败即停（不猜帧）；
//   * OTP 降级：OnceTrace 一次性回溯——烂帧即断，深度上限 OTP_MAX；
//   * 注入核：烂帧（fp 环/越界 ra）/OTP 超限/二次回溯 四类必抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0xFD80 独占） ─────────────────────────────────────────────
pub const XC_BT_BADFP: u32 = 0xFD81; // 烂帧（环/非单调）
pub const XC_BT_BADRA: u32 = 0xFD82; // ra 越码段
pub const XC_BT_OTP: u32 = 0xFD83; // OTP 深度超限
pub const XC_BT_REUSE: u32 = 0xFD84; // 一次性回溯被复用

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// OTP 上限（一次性回溯深度）。
pub const OTP_MAX: usize = 64;
/// 代码段界（模型）：0x1000_0000..0x1001_0000。
pub const CODE_LO: u64 = 0x1000_0000;
pub const CODE_HI: u64 = 0x1001_0000;

/// 栈帧：fp（帧指针）/ ra（返回址）/ fn_id。
#[derive(Clone, Copy)]
pub struct Frame {
    pub fp: u64,
    pub ra: u64,
    pub fn_id: u16,
}

/// 已知调用树（M-300 对拍真源）：主→中→叶 三层 + 尾帧。
pub struct CallTree {
    pub frames: Vec<Frame>,
    pub stack_top: u64,
}

impl CallTree {
    /// 构造合法调用栈：帧地址上溯递增（栈向下生长），ra 全在码段内。
    pub fn known() -> Self {
        let base: u64 = 0x7FF0_0000;
        let frames = vec![
            Frame { fp: base + 0x30, ra: CODE_LO + 0x40, fn_id: 3 }, // leaf
            Frame { fp: base + 0x60, ra: CODE_LO + 0x80, fn_id: 2 }, // mid
            Frame { fp: base + 0x90, ra: CODE_LO + 0xC0, fn_id: 1 }, // main
            Frame { fp: 0, ra: CODE_LO + 0x100, fn_id: 0 },          // 尾帧（fp=0 链终）
        ];
        CallTree { frames, stack_top: base }
    }
    /// 期望回溯序（fn_id 序列）。
    pub fn expect_seq(&self) -> Vec<u16> {
        self.frames.iter().map(|f| f.fn_id).collect()
    }
}

/// 帧界校验四闸。
pub fn frame_ok(prev_fp: u64, f: &Frame) -> Result<(), u32> {
    // 闸 1：链终（fp==0 合法终止）
    if f.fp == 0 && prev_fp != 0 {
        return Ok(()); // 终帧允许 fp=0
    }
    // 闸 2：fp 单调（上溯必须递增；首帧 prev_fp=0 跳过）
    if prev_fp != 0 && f.fp <= prev_fp {
        return Err(XC_BT_BADFP);
    }
    // 闸 3：帧距界内（≤0x1000，防环/跳帧）
    if prev_fp != 0 && f.fp - prev_fp > 0x1000 {
        return Err(XC_BT_BADFP);
    }
    // 闸 4：ra 落码段
    if f.ra < CODE_LO || f.ra >= CODE_HI {
        return Err(XC_BT_BADRA);
    }
    Ok(())
}

/// fp 链回溯器。
pub struct Backtracer {
    frames: Vec<Frame>,
    cursor: usize,
    done: bool,
}

impl Backtracer {
    pub fn new(frames: Vec<Frame>) -> Self {
        Backtracer { frames, cursor: 0, done: false }
    }

    /// 逐帧上溯：帧界校验失败即停（不猜帧）。
    pub fn walk(&mut self) -> Vec<Frame> {
        let mut out: Vec<Frame> = Vec::new();
        let mut prev_fp: u64 = 0;
        while self.cursor < self.frames.len() {
            let f = self.frames[self.cursor];
            if let Err(_e) = frame_ok(prev_fp, &f) {
                self.done = true; // 烂帧即断
                break;
            }
            out.push(f);
            prev_fp = f.fp;
            self.cursor += 1;
            if f.fp == 0 {
                break; // 链终
            }
        }
        out
    }

    /// 回溯序（fn_id 序列）。
    pub fn walk_seq(&mut self) -> Vec<u16> {
        self.walk().iter().map(|f| f.fn_id).collect()
    }
}

/// OTP 一次性回溯器：深度上限 + 单次消费。
pub struct OnceTracer {
    frames: Vec<Frame>,
    consumed: bool,
}

impl OnceTracer {
    pub fn new(frames: Vec<Frame>) -> Self {
        OnceTracer { frames, consumed: false }
    }

    /// 一次性回溯：烂帧即断不猜，深度 ≤OTP_MAX；二次调用拒绝。
    pub fn trace_once(&mut self) -> Result<Vec<Frame>, u32> {
        if self.consumed {
            return Err(XC_BT_REUSE);
        }
        self.consumed = true;
        let mut out: Vec<Frame> = Vec::new();
        let mut prev_fp: u64 = 0;
        for f in &self.frames {
            if f.fp == 0 && prev_fp != 0 {
                // 链终帧不计 OTP 深度
                if let Err(e) = frame_ok(prev_fp, f) {
                    return Err(e);
                }
                out.push(*f);
                break;
            }
            if out.len() >= OTP_MAX {
                return Err(XC_BT_OTP); // 深度超限降级
            }
            if let Err(e) = frame_ok(prev_fp, f) {
                return Err(e); // 烂帧即断
            }
            out.push(*f);
            prev_fp = f.fp;
        }
        Ok(out)
    }
    pub fn consumed(&self) -> bool {
        self.consumed
    }
}

/// 构造烂帧栈（fp 环）。
pub fn poisoned_loop_stack() -> Vec<Frame> {
    let base: u64 = 0x7EE0_0000;
    vec![
        Frame { fp: base + 0x40, ra: CODE_LO + 0x10, fn_id: 10 },
        Frame { fp: base + 0x40, ra: CODE_LO + 0x20, fn_id: 11 }, // fp 不增（环）
        Frame { fp: base + 0x80, ra: CODE_LO + 0x30, fn_id: 12 },
    ]
}

/// 构造 ra 越码段栈。
pub fn poisoned_ra_stack() -> Vec<Frame> {
    let base: u64 = 0x7DD0_0000;
    vec![
        Frame { fp: base + 0x40, ra: CODE_LO + 0x10, fn_id: 20 },
        Frame { fp: base + 0x80, ra: 0xDEAD_0000, fn_id: 21 }, // ra 出码段
    ]
}

/// 注入核：烂帧环/ra 越界/OTP 超限/二次回溯 四类必抓。
pub struct BtInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl BtInjectCore {
    pub fn new() -> Self {
        BtInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_fp_loop(&mut self) {
        self.total += 1;
        let mut bt = Backtracer::new(poisoned_loop_stack());
        let seq = bt.walk_seq();
        // 烂帧即断：只采到第一帧
        if seq == vec![10] {
            self.caught += 1;
        }
    }
    pub fn inject_ra_oob(&mut self) {
        self.total += 1;
        let mut ot = OnceTracer::new(poisoned_ra_stack());
        let r = ot.trace_once();
        if matches!(r, Err(XC_BT_BADRA)) {
            self.caught += 1;
        }
    }
    pub fn inject_otp_over(&mut self) {
        self.total += 1;
        // OTP_MAX+1 帧全合法 → 深度超限必拒
        let mut frames: Vec<Frame> = Vec::new();
        let mut fp = 0x7CC0_0000u64;
        for i in 0..(OTP_MAX + 1) {
            fp += 0x20;
            frames.push(Frame { fp, ra: CODE_LO + (i as u64), fn_id: (i % 200) as u16 });
        }
        frames.push(Frame { fp: 0, ra: CODE_LO, fn_id: 0 });
        let mut ot = OnceTracer::new(frames);
        let r = ot.trace_once();
        if matches!(r, Err(XC_BT_OTP)) {
            self.caught += 1;
        }
    }
    pub fn inject_double_trace(&mut self) {
        self.total += 1;
        let mut ot = OnceTracer::new(CallTree::known().frames);
        let t1 = ot.trace_once();
        let t2 = ot.trace_once();
        if t1.is_ok() && matches!(t2, Err(XC_BT_REUSE)) {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx3542_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx3542");

    // C42-01 已知调用树逐帧一致（M-300）
    {
        let tree = CallTree::known();
        let mut bt = Backtracer::new(tree.frames.clone());
        let seq = bt.walk_seq();
        let ok = seq == tree.expect_seq();
        s.add("C42-01-逐帧一致", ok, if ok { "" } else { "回溯序漂移" });
    }
    // C42-02 帧界校验四闸（合法栈全过）
    {
        let tree = CallTree::known();
        let mut all_ok = true;
        let mut prev = 0u64;
        for f in &tree.frames {
            if frame_ok(prev, f).is_err() {
                all_ok = false;
            }
            prev = f.fp;
        }
        s.add("C42-02-帧界四闸", all_ok, if all_ok { "" } else { "合法帧被拒" });
    }
    // C42-03 OTP 一次性回溯（合法栈成功）
    {
        let mut ot = OnceTracer::new(CallTree::known().frames);
        let t = ot.trace_once();
        let ok = t.is_ok() && ot.consumed();
        s.add("C42-03-OTP 成功", ok, if ok { "" } else { "OTP 断" });
    }
    // C42-04 OTP 烂帧即断（fp 环只出一帧）
    {
        let mut ot = OnceTracer::new(poisoned_loop_stack());
        let r = ot.trace_once();
        match r {
            Ok(v) => {
                s.add("C42-04-烂帧即断", false, "环帧被采出多帧");
                let _ = v;
            }
            Err(XC_BT_BADFP) => s.add("C42-04-烂帧即断", true, ""),
            Err(_) => s.add("C42-04-烂帧即断", false, "错码"),
        }
    }
    // C42-05 OTP 深度上限守恒（恰 64 帧成功）
    {
        let mut frames: Vec<Frame> = Vec::new();
        let mut fp = 0x7BB0_0000u64;
        for i in 0..OTP_MAX {
            fp += 0x20;
            frames.push(Frame { fp, ra: CODE_LO + (i as u64), fn_id: (i % 200) as u16 });
        }
        frames.push(Frame { fp: 0, ra: CODE_LO, fn_id: 0 });
        let mut ot = OnceTracer::new(frames);
        let r = ot.trace_once();
        let ok = matches!(r, Ok(v) if v.len() == OTP_MAX + 1);
        s.add("C42-05-OTP 界内", ok, if ok { "" } else { "深度断" });
    }
    // C42-06 回溯序指纹（同树同序恒等）
    {
        let mut a = Backtracer::new(CallTree::known().frames.clone());
        let mut b = Backtracer::new(CallTree::known().frames);
        let ok = a.walk_seq() == b.walk_seq();
        s.add("C42-06-指纹恒等", ok, if ok { "" } else { "序漂移" });
    }
    // C42-07 注入核 4/4
    {
        let mut inj = BtInjectCore::new();
        inj.inject_fp_loop();
        inj.inject_ra_oob();
        inj.inject_otp_over();
        inj.inject_double_trace();
        s.add(
            "C42-07-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    s
}
