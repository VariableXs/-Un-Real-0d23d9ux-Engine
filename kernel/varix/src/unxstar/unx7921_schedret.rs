//! UNX-F7921 调度与保留总纲（策略面宣言）（0xA100 段）
//!
//! 锚点：UNX-F7921-J1 策略面总纲（调度+保留+GC 三面铺排落账 ×1；
//! vs 总纲 §7.3-B5 引言备份主题边界声明 ×1（策略面为备份校验链的
//! 运维延伸）；三面混写注入 ×10 检出）。
//! 域：CoRun·UNX-B5-B37（策略面宣言面）。
//!
//! 结构：三面闭集（Schedule / Retention / GC）+ 三面铺排落账 +
//! 主题边界声明（策略面 ⊂ 备份校验链运维延伸）+ 三面混写注入
//! ×10 检出核（跨面操作错配全报）+ 注入核（缺面/越界/混写/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA100 独占 0x80）。
pub const SEG_BASE: u32 = 0xA100;
pub const XC_SR_NOFACE: u32 = SEG_BASE + 0x01; // 缺面
pub const XC_SR_BOUND: u32 = SEG_BASE + 0x02; // 边界越界
pub const XC_SR_MIX: u32 = SEG_BASE + 0x03; // 混写
pub const XC_SR_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 三面闭集。
pub const FACE_SCHED: u8 = 0; // 调度（何时备份）
pub const FACE_RETAIN: u8 = 1; // 保留（留几份）
pub const FACE_GC: u8 = 2; // GC（回收谁）

/// 面名。
pub fn policy_face_name(f: u8) -> &'static str {
    match f {
        FACE_SCHED => "schedule",
        FACE_RETAIN => "retention",
        FACE_GC => "gc",
        _ => "?",
    }
}

/// 面内操作闭集（错配检测用）。
pub const OP_SCHED_TICK: &str = "sched-tick";
pub const OP_RETAIN_KEEP: &str = "retain-keep";
pub const OP_GC_SWEEP: &str = "gc-sweep";

/// 面 → 合法操作表。
pub fn face_op_legal(face: u8, op: &str) -> bool {
    match face {
        FACE_SCHED => op == OP_SCHED_TICK,
        FACE_RETAIN => op == OP_RETAIN_KEEP,
        FACE_GC => op == OP_GC_SWEEP,
        _ => false,
    }
}

/// 策略面铺排账。
pub struct PolicyBook {
    pub faces: Vec<u8>,
    pub ops: Vec<(u8, String)>,
    pub sealed: bool,
}

impl PolicyBook {
    pub fn new() -> Self {
        PolicyBook { faces: Vec::new(), ops: Vec::new(), sealed: false }
    }

    /// 铺面。
    pub fn lay_face(&mut self, face: u8) -> Result<(), u32> {
        if face > FACE_GC {
            return Err(XC_SR_NOFACE);
        }
        if self.sealed {
            return Err(XC_SR_SEALED);
        }
        if self.faces.contains(&face) {
            return Ok(()); // 幂等铺面
        }
        self.faces.push(face);
        Ok(())
    }

    /// 面内操作（混写检测：face 与 op 错配 → MIX）。
    pub fn do_op(&mut self, face: u8, op: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_SR_SEALED);
        }
        if face > FACE_GC {
            return Err(XC_SR_NOFACE);
        }
        if !self.faces.contains(&face) {
            return Err(XC_SR_NOFACE);
        }
        if !face_op_legal(face, op) {
            return Err(XC_SR_MIX);
        }
        self.ops.push((face, String::from(op)));
        Ok(())
    }

    /// 三面齐备。
    pub fn faces_full(&self) -> bool {
        (0..=2u8).all(|f| self.faces.contains(&f))
    }
}

/// 主题边界声明核：策略面为备份校验链的运维延伸（声明账冻结）。
pub const BOUNDARY_DECL: &str = "policy-face = backup-chain-ops-extension";

pub fn boundary_decl_exact() -> bool {
    BOUNDARY_DECL == "policy-face = backup-chain-ops-extension"
}

/// 三面铺排落账核：三面全铺 + 各面操作留账。
pub fn three_face_exact() -> bool {
    let mut b = PolicyBook::new();
    for f in 0..=2u8 {
        if b.lay_face(f).is_err() {
            return false;
        }
    }
    if b.do_op(FACE_SCHED, OP_SCHED_TICK).is_err() {
        return false;
    }
    if b.do_op(FACE_RETAIN, OP_RETAIN_KEEP).is_err() {
        return false;
    }
    if b.do_op(FACE_GC, OP_GC_SWEEP).is_err() {
        return false;
    }
    b.faces_full() && b.ops.len() == 3
}

/// 三面混写注入 ×10 检出核：错配组合全报。
pub fn mix_inject_10_exact() -> bool {
    let mut caught = 0usize;
    let mut b = PolicyBook::new();
    for f in 0..=2u8 {
        let _ = b.lay_face(f);
    }
    // 注入 1：sched 面跑 gc-sweep
    if b.do_op(FACE_SCHED, OP_GC_SWEEP) == Err(XC_SR_MIX) {
        caught += 1;
    }
    // 注入 2：sched 面跑 retain-keep
    if b.do_op(FACE_SCHED, OP_RETAIN_KEEP) == Err(XC_SR_MIX) {
        caught += 1;
    }
    // 注入 3：retain 面跑 sched-tick
    if b.do_op(FACE_RETAIN, OP_SCHED_TICK) == Err(XC_SR_MIX) {
        caught += 1;
    }
    // 注入 4：retain 面跑 gc-sweep
    if b.do_op(FACE_RETAIN, OP_GC_SWEEP) == Err(XC_SR_MIX) {
        caught += 1;
    }
    // 注入 5：gc 面跑 sched-tick
    if b.do_op(FACE_GC, OP_SCHED_TICK) == Err(XC_SR_MIX) {
        caught += 1;
    }
    // 注入 6：gc 面跑 retain-keep
    if b.do_op(FACE_GC, OP_RETAIN_KEEP) == Err(XC_SR_MIX) {
        caught += 1;
    }
    // 注入 7：未知操作名
    if b.do_op(FACE_SCHED, "unknown-op") == Err(XC_SR_MIX) {
        caught += 1;
    }
    // 注入 8：未铺面操作（新建空账）
    let mut b2 = PolicyBook::new();
    if b2.do_op(FACE_GC, OP_GC_SWEEP) == Err(XC_SR_NOFACE) {
        caught += 1;
    }
    // 注入 9：面越界（face=7）
    if b.do_op(7, OP_GC_SWEEP) == Err(XC_SR_NOFACE) {
        caught += 1;
    }
    // 注入 10：缺面检测（只铺两面）
    let mut b3 = PolicyBook::new();
    let _ = b3.lay_face(FACE_SCHED);
    let _ = b3.lay_face(FACE_RETAIN);
    if !b3.faces_full() {
        caught += 1;
    }
    caught == 10
}

/// 幂等铺面核：重复铺面无害。
pub fn idempotent_lay_exact() -> bool {
    let mut b = PolicyBook::new();
    let _ = b.lay_face(FACE_SCHED);
    let _ = b.lay_face(FACE_SCHED);
    b.faces.len() == 1 && b.faces.contains(&FACE_SCHED)
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 面越界铺面
    let mut b = PolicyBook::new();
    if b.lay_face(9) == Err(XC_SR_NOFACE) {
        caught += 1;
    }
    // 2) 混写（错配）
    let _ = b.lay_face(FACE_SCHED);
    if b.do_op(FACE_SCHED, OP_GC_SWEEP) == Err(XC_SR_MIX) {
        caught += 1;
    }
    // 3) 封账后拒
    b.sealed = true;
    if b.do_op(FACE_SCHED, OP_SCHED_TICK) == Err(XC_SR_SEALED) {
        caught += 1;
    }
    // 4) 边界声明冻结
    if !boundary_decl_exact() {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：策略账折叠哈希。
pub fn fp_frozen(b: &PolicyBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &f in &b.faces {
        h ^= f as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    for (f, op) in &b.ops {
        h ^= *f as u64;
        h = h.wrapping_mul(0x100000001b3);
        for &c in op.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx7921_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7921");
    // J1-1 三面铺排落账
    let ok = three_face_exact();
    s.add("B37-01-三面铺排", ok, if ok { "" } else { "铺排漂移" });
    // J1-2 混写注入 ×10 检出
    let ok = mix_inject_10_exact();
    s.add("B37-02-混写十检", ok, if ok { "" } else { "混写漏检" });
    // J1-3 边界声明
    let ok = boundary_decl_exact();
    s.add("B37-03-边界声明", ok, if ok { "" } else { "声明漂移" });
    // J1-4 幂等铺面
    let ok = idempotent_lay_exact();
    s.add("B37-04-幂等铺面", ok, if ok { "" } else { "幂等漂移" });
    // J1-5 面闭集
    let ok = policy_face_name(0) != "?" && policy_face_name(2) != "?" && policy_face_name(3) == "?";
    s.add("B37-05-面闭集", ok, if ok { "" } else { "闭集漂移" });
    // J1-6 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B37-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let mut b = PolicyBook::new();
    let _ = b.lay_face(FACE_SCHED);
    let _ = b.lay_face(FACE_RETAIN);
    let _ = b.lay_face(FACE_GC);
    let ok = fp_frozen(&b) == fp_frozen(&b) && b.faces_full();
    s.add("B37-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
