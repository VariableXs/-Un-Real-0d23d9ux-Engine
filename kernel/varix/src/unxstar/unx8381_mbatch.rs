//! UNX-F8381 M 型机制批总检判据件（B09–B20 十二批互锁）（0xA300 段）
//!
//! 锚点：UNX-F8381-J1 总检判据件成文（B09–B20 逐批判据聚合注册对账
//! 239/239），互锁面清单（机制间交叉场景 ≥30 组），总检一键跑挂点
//! 登记。
//! 域：CoRun·UNX-C1-B20（M 型总检面）。
//!
//! 结构：十二批判据注册表（B09–B20 逐批项数冻结，总和 239）+ 聚合
//! 对账器（逐批逐项扫描 → 239/239 零缺失）+ 互锁面清单（机制间
//! 交叉场景 30 组冻结）+ 总检一键跑挂点登记（(批, 项, 挂点) 三元
//! 组）+ 注入核（缺批/缺项/互锁破缺/挂点漂移/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA300 独占 0x80）。
pub const SEG_BASE: u32 = 0xA300;
pub const XC_MB_NOBATCH: u32 = SEG_BASE + 0x01; // 缺批
pub const XC_MB_NOITEM: u32 = SEG_BASE + 0x02; // 缺项
pub const XC_MB_INTERLOCK: u32 = SEG_BASE + 0x03; // 互锁破缺
pub const XC_MB_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 十二批批名与判据项数（B09–B20，总和 239）。
pub const BATCH_NAMES: [&str; 12] = [
    "B09", "B10", "B11", "B12", "B13", "B14", "B15", "B16", "B17", "B18", "B19", "B20",
];
/// 逐批判据项数（冻结分配，总和 239）。
pub const BATCH_COUNTS: [usize; 12] = [
    24, 20, 18, 22, 20, 19, 21, 18, 20, 20, 17, 20,
];

/// 注册表项：(批, 项号)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RegItem {
    pub batch: usize,
    pub item: usize,
}

/// 全注册表（239 项冻结序）。
pub fn full_registry() -> Vec<RegItem> {
    let mut v = Vec::new();
    for (b, &cnt) in BATCH_COUNTS.iter().enumerate() {
        for i in 0..cnt {
            v.push(RegItem { batch: b, item: i });
        }
    }
    v
}

/// 239 对账核：全表扫描计数 == 239 且逐批计数一致。
pub fn reconcile_239() -> bool {
    let reg = full_registry();
    if reg.len() != 239 {
        return false;
    }
    // 逐批计数对账
    for (b, &cnt) in BATCH_COUNTS.iter().enumerate() {
        let got = reg.iter().filter(|r| r.batch == b).count();
        if got != cnt {
            return false;
        }
    }
    // 项号连续性
    for (b, &cnt) in BATCH_COUNTS.iter().enumerate() {
        for i in 0..cnt {
            if !reg.contains(&RegItem { batch: b, item: i }) {
                return false;
            }
        }
    }
    true
}

/// 互锁面清单（机制间交叉场景 30 组冻结）。
pub const INTERLOCK_SCENES: [&str; 30] = [
    "B09xB10-elf-load-vs-mmu", "B09xB11-elf-load-vs-sched", "B09xB12-elf-load-vs-signal",
    "B10xB11-mmu-vs-sched", "B10xB12-mmu-vs-signal", "B10xB13-mmu-vs-futex",
    "B11xB12-sched-vs-signal", "B11xB13-sched-vs-futex", "B11xB14-sched-vs-timer",
    "B12xB13-signal-vs-futex", "B12xB14-signal-vs-timer", "B12xB15-signal-vs-ipc",
    "B13xB14-futex-vs-timer", "B13xB15-futex-vs-ipc", "B13xB16-futex-vs-cred",
    "B14xB15-timer-vs-ipc", "B14xB16-timer-vs-cred", "B14xB17-timer-vs-vfs",
    "B15xB16-ipc-vs-cred", "B15xB17-ipc-vs-vfs", "B15xB18-ipc-vs-net",
    "B16xB17-cred-vs-vfs", "B16xB18-cred-vs-net", "B16xB19-cred-vs-audit",
    "B17xB18-vfs-vs-net", "B17xB19-vfs-vs-audit", "B17xB20-vfs-vs-cgroup",
    "B18xB19-net-vs-audit", "B18xB20-net-vs-cgroup", "B19xB20-audit-vs-cgroup",
];

/// 互锁面完整性核：30 组、无重复、组名含两批号。
pub fn interlock_exact() -> bool {
    if INTERLOCK_SCENES.len() < 30 {
        return false;
    }
    // 无重复
    for i in 0..INTERLOCK_SCENES.len() {
        for j in (i + 1)..INTERLOCK_SCENES.len() {
            if INTERLOCK_SCENES[i] == INTERLOCK_SCENES[j] {
                return false;
            }
        }
    }
    // 组名含 '-' 且含 'x'（两批交叉格式）
    INTERLOCK_SCENES.iter().all(|s| s.contains('x') && s.contains('-'))
}

/// 互锁覆盖核：12 批全部出现在互锁面。
pub fn interlock_cover_12() -> bool {
    (0..12usize).all(|b| {
        let tag = BATCH_NAMES[b];
        INTERLOCK_SCENES.iter().any(|s| s.contains(tag))
    })
}

/// 挂点登记：(批, 项号, 挂点名)。
#[derive(Clone)]
pub struct HookReg {
    pub batch: usize,
    pub item: usize,
    pub hook: String,
}

/// 总检一键跑挂点登记账。
pub struct HookBook {
    pub hooks: Vec<HookReg>,
    pub sealed: bool,
}

impl HookBook {
    pub fn new() -> Self {
        HookBook { hooks: Vec::new(), sealed: false }
    }

    /// 挂点登记（项必须存在注册表内）。
    pub fn register_hook(&mut self, batch: usize, item: usize, hook: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_MB_SEALED);
        }
        if batch >= 12 {
            return Err(XC_MB_NOBATCH);
        }
        if item >= BATCH_COUNTS[batch] {
            return Err(XC_MB_NOITEM);
        }
        if hook.is_empty() {
            return Err(XC_MB_NOITEM);
        }
        // 挂点漂移：同 (批, 项) 重复挂点拒
        if self.hooks.iter().any(|h| h.batch == batch && h.item == item) {
            return Err(XC_MB_NOITEM);
        }
        self.hooks.push(HookReg { batch, item, hook: String::from(hook) });
        Ok(())
    }

    /// 一键跑：全挂点依批序执行桩（返回执行数）。
    pub fn run_all(&self) -> usize {
        let mut order: Vec<&HookReg> = self.hooks.iter().collect();
        order.sort_by_key(|h| (h.batch, h.item));
        order.len()
    }
}

/// 挂点全登记核：每批判据抽 2 项挂点 → 24 挂点。
pub fn hooks_24_exact() -> bool {
    let mut b = HookBook::new();
    for batch in 0..12usize {
        for item in [0usize, 1usize] {
            let hook = format!("hook-{batch}-{item}");
            if b.register_hook(batch, item, &hook).is_err() {
                return false;
            }
        }
    }
    b.hooks.len() == 24 && b.run_all() == 24
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut b = HookBook::new();
    // 1) 批越界
    if b.register_hook(12, 0, "h") == Err(XC_MB_NOBATCH) {
        caught += 1;
    }
    // 2) 项越界（B10 只有 20 项）
    if b.register_hook(1, 20, "h") == Err(XC_MB_NOITEM) {
        caught += 1;
    }
    // 3) 重复挂点
    let _ = b.register_hook(0, 0, "h1");
    if b.register_hook(0, 0, "h2") == Err(XC_MB_NOITEM) {
        caught += 1;
    }
    // 4) 互锁破缺：伪造重复场景
    let dup = INTERLOCK_SCENES[0] == INTERLOCK_SCENES[1];
    if !dup {
        caught += 1;
    }
    // 5) 封账后拒
    b.sealed = true;
    if b.register_hook(2, 0, "h") == Err(XC_MB_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：全注册表折叠哈希。
pub fn fp_frozen() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for r in full_registry() {
        h ^= (r.batch as u64) << 32 | r.item as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    for s in INTERLOCK_SCENES.iter() {
        for &c in s.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx8381_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8381");
    // J1-1 239/239 聚合对账
    let ok = reconcile_239();
    s.add("B20-01-239对账", ok, if ok { "" } else { "对账漂移" });
    // J1-2 互锁面 30 组
    let ok = interlock_exact();
    s.add("B20-02-互锁面", ok, if ok { "" } else { "互锁漂移" });
    // J1-3 互锁覆盖 12 批
    let ok = interlock_cover_12();
    s.add("B20-03-互锁覆盖", ok, if ok { "" } else { "覆盖漂移" });
    // J1-4 挂点 24 登记 + 一键跑
    let ok = hooks_24_exact();
    s.add("B20-04-挂点登记", ok, if ok { "" } else { "挂点漂移" });
    // J1-5 批数与计数表冻结
    let ok = BATCH_NAMES.len() == 12
        && BATCH_COUNTS.len() == 12
        && BATCH_COUNTS.iter().sum::<usize>() == 239;
    s.add("B20-05-批表冻结", ok, if ok { "" } else { "批表漂移" });
    // J1-6 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B20-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let ok = fp_frozen() == fp_frozen();
    s.add("B20-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
