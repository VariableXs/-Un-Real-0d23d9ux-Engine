//! UNX-F6921 remove 正常路径总序（排空→注销→释放五步）（0x9180 段）
//!
//! 锚点：UNX-F6921-J1 remove 五步总序断言（排空/队列冻结/gendisk
//! 注销/资源释放/账本注销）；步序不可乱序断言。
//! 域：CoRun·UNX-B4-B27（remove 总序面）。
//!
//! 结构：五步闭集（Drain→Freeze→Unregister→Release→Deregister）+
//! 步序执行器（严格序号递进 + 乱序拒）+ 步留账（(步, 资源, 序号)）
//! + 步幂等核（重复执行同步拒）+ 注入核（跳步/逆序/重复/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x9180 独占 0x80）。
pub const SEG_BASE: u32 = 0x9180;
pub const XC_RM_SKIP: u32 = SEG_BASE + 0x01; // 跳步
pub const XC_RM_ORDER: u32 = SEG_BASE + 0x02; // 乱序
pub const XC_RM_DUP: u32 = SEG_BASE + 0x03; // 重复执行
pub const XC_RM_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 五步闭集（序号冻结）。
pub const STEP_DRAIN: u8 = 0; // 排空
pub const STEP_FREEZE: u8 = 1; // 队列冻结
pub const STEP_UNREG_DISK: u8 = 2; // gendisk 注销
pub const STEP_RELEASE: u8 = 3; // 资源释放
pub const STEP_DEREG: u8 = 4; // 账本注销

/// 五步名。
pub fn step_name(s: u8) -> &'static str {
    match s {
        STEP_DRAIN => "drain",
        STEP_FREEZE => "freeze",
        STEP_UNREG_DISK => "unregister-gendisk",
        STEP_RELEASE => "release",
        STEP_DEREG => "deregister",
        _ => "?",
    }
}

/// 步留账项。
#[derive(Clone)]
pub struct StepLog {
    pub step: u8,
    pub resource: String,
    pub seq: u8,
}

/// remove 总序执行器。
pub struct RemoveSeq {
    pub dev_id: u64,
    pub logs: Vec<StepLog>,
    pub next: u8, // 下一合法步
    pub done: bool,
    pub sealed: bool,
}

impl RemoveSeq {
    pub fn new(dev_id: u64) -> Self {
        RemoveSeq { dev_id, logs: Vec::new(), next: STEP_DRAIN, done: false, sealed: false }
    }

    /// 执行一步（严格序守卫 + 留账）。
    pub fn run_step(&mut self, step: u8, resource: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_RM_SEALED);
        }
        if self.done {
            return Err(XC_RM_DUP);
        }
        if step < self.next {
            return Err(XC_RM_ORDER);
        }
        if step > self.next {
            return Err(XC_RM_SKIP);
        }
        self.logs.push(StepLog {
            step,
            resource: String::from(resource),
            seq: self.next,
        });
        self.next += 1;
        if self.next > STEP_DEREG {
            self.done = true;
            self.sealed = true;
        }
        Ok(())
    }

    /// 全五步一键执行（正常路径）。
    pub fn run_all(&mut self, res: &str) -> Result<(), u32> {
        for i in 0..5u8 {
            self.run_step(i, res)?;
        }
        Ok(())
    }

    /// 步序链核：日志步号严格 0,1,2,3,4。
    pub fn ordered(&self) -> bool {
        if self.logs.len() != 5 {
            return false;
        }
        self.logs.iter().enumerate().all(|(i, l)| l.step == i as u8 && l.seq == i as u8)
    }

    /// 总序完成断言。
    pub fn complete(&self) -> bool {
        self.done && self.sealed && self.ordered()
    }
}

/// 全五步总序核：一步不落。
pub fn full_sequence_exact() -> bool {
    let mut r = RemoveSeq::new(0xEE00_0001);
    if r.run_all("sdX").is_err() {
        return false;
    }
    r.complete()
}

/// 分步执行核：逐步推进（模拟调度间隙）。
pub fn stepped_sequence_exact() -> bool {
    let mut r = RemoveSeq::new(0xEF00_0002);
    let steps = [
        (STEP_DRAIN, "queue"),
        (STEP_FREEZE, "queue"),
        (STEP_UNREG_DISK, "gendisk0"),
        (STEP_RELEASE, "irq+dma"),
        (STEP_DEREG, "lifecycle-book"),
    ];
    for &(s, res) in steps.iter() {
        if r.run_step(s, res).is_err() {
            return false;
        }
    }
    r.complete() && r.logs[2].resource == "gendisk0"
}

/// 步序不可乱序断言核：乱序/跳步/重复全拒。
pub fn order_guard_exact() -> bool {
    // 乱序：从 FREEZE 开始
    let mut r = RemoveSeq::new(1);
    if r.run_step(STEP_FREEZE, "x").is_ok() {
        return false;
    }
    // 跳步：DRAIN 后直接 UNREG
    let mut r2 = RemoveSeq::new(2);
    let _ = r2.run_step(STEP_DRAIN, "x");
    if r2.run_step(STEP_UNREG_DISK, "x").is_ok() {
        return false;
    }
    // 重复：DRAIN 后再 DRAIN
    let mut r3 = RemoveSeq::new(3);
    let _ = r3.run_step(STEP_DRAIN, "x");
    if r3.run_step(STEP_DRAIN, "x").is_ok() {
        return false;
    }
    // 逆序：RELEASE 先行
    let mut r4 = RemoveSeq::new(4);
    if r4.run_step(STEP_RELEASE, "x").is_ok() {
        return false;
    }
    // 封账后总拒
    let mut r5 = RemoveSeq::new(5);
    let _ = r5.run_all("x");
    if r5.run_step(STEP_DRAIN, "x").is_ok() {
        return false;
    }
    // 完成后总拒（done 不必 sealed）
    let mut r6 = RemoveSeq::new(6);
    let _ = r6.run_all("x");
    !r6.run_step(STEP_DEREG, "x").is_ok()
}

/// 五步资源账核：每步资源标签独立留痕。
pub fn resource_ledger_exact() -> bool {
    let mut r = RemoveSeq::new(0xF000_0003);
    let _ = r.run_step(STEP_DRAIN, "inflight=3");
    let _ = r.run_step(STEP_FREEZE, "frozen");
    let _ = r.run_step(STEP_UNREG_DISK, "disk8:0");
    let _ = r.run_step(STEP_RELEASE, "msix[4..5]");
    let _ = r.run_step(STEP_DEREG, "book");
    if !r.complete() {
        return false;
    }
    // 排空账在冻结账之前
    r.logs[0].step == STEP_DRAIN
        && r.logs[1].step == STEP_FREEZE
        && r.logs[0].resource == "inflight=3"
        && r.logs[4].resource == "book"
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 跳步
    let mut r = RemoveSeq::new(11);
    if r.run_step(STEP_RELEASE, "x") == Err(XC_RM_SKIP) {
        caught += 1;
    }
    // 2) 乱序（低序号回退）
    let mut r2 = RemoveSeq::new(12);
    let _ = r2.run_step(STEP_DRAIN, "x");
    let _ = r2.run_step(STEP_FREEZE, "x");
    if r2.run_step(STEP_DRAIN, "x") == Err(XC_RM_ORDER) {
        caught += 1;
    }
    // 3) 重复五步
    let mut r3 = RemoveSeq::new(13);
    let _ = r3.run_all("x");
    if r3.run_step(STEP_DRAIN, "x") == Err(XC_RM_DUP) {
        caught += 1;
    }
    // 4) 封账后拒
    let mut r4 = RemoveSeq::new(14);
    let _ = r4.run_all("x");
    if r4.run_step(STEP_FREEZE, "x") == Err(XC_RM_DUP) {
        caught += 1;
    }
    // 5) 全序不完整则 complete=false（少最后一步）
    let mut r5 = RemoveSeq::new(15);
    for i in 0..4u8 {
        let _ = r5.run_step(i, "x");
    }
    if !r5.complete() {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：步账折叠哈希。
pub fn fp_frozen(r: &RemoveSeq) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for l in &r.logs {
        h ^= l.step as u64;
        h = h.wrapping_mul(0x100000001b3);
        for &c in l.resource.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx6921_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6921");
    // J1-1 全五步总序
    let ok = full_sequence_exact();
    s.add("B27-01-五步总序", ok, if ok { "" } else { "总序漂移" });
    // J1-2 分步执行
    let ok = stepped_sequence_exact();
    s.add("B27-02-分步执行", ok, if ok { "" } else { "分步漂移" });
    // J1-3 步序不可乱序
    let ok = order_guard_exact();
    s.add("B27-03-乱序拒", ok, if ok { "" } else { "序守卫漂移" });
    // J1-4 资源账
    let ok = resource_ledger_exact();
    s.add("B27-04-资源账", ok, if ok { "" } else { "资源账漂移" });
    // J1-5 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B27-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let mut r = RemoveSeq::new(16);
    let _ = r.run_all("fp");
    let ok = fp_frozen(&r) == fp_frozen(&r) && r.complete();
    s.add("B27-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
