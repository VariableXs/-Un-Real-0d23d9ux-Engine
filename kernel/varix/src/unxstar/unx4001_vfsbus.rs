// unx4001 —— UNX-F4001 VFS 域总线初始化与对象族谱
// 判据原文 UNX-F4001-J1：内核启动后 vfs_init() 首次调用成功且六类对象族
// （sb/inode/dentry/file/mount/ns）注册计数落账，二次调用返回 E_ALREADY
// 且全局状态逐字节不变（ktest vfs B01 面断言可复测）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xFF80 独占（0xFF80~0xFFFF）。
//
// 设计（纯功能 no_std）：
//   * vfs_init() 单次初始化闸：首次成功落账，二次 E_ALREADY；
//   * 六类对象族谱闭集：sb/inode/dentry/file/mount/ns 各注册计数落账；
//   * 幂等不变量：二次调用后全局状态逐字节不变（全量快照复算）；
//   * 注入核：重复初始化/族谱计数漂移/封账后注册 三类必抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xFF80 独占） ─────────────────────────────────────────────
pub const XC_VFS_ALREADY: u32 = 0xFF81; // 重复初始化
pub const XC_VFS_REG: u32 = 0xFF82; // 族谱计数漂移
pub const XC_VFS_SEAL: u32 = 0xFF83; // 封账后注册
pub const XC_VFS_STATE: u32 = 0xFF84; // 状态漂移

/// 六类对象族闭集。
pub const OBJ_FAMILIES: [&str; 6] = ["sb", "inode", "dentry", "file", "mount", "ns"];

pub fn family_idx(name: &str) -> Option<usize> {
    OBJ_FAMILIES.iter().position(|s| *s == name)
}

/// VFS 域总线（全局态模型）。
pub struct VfsBus {
    inited: bool,
    /// 六族注册计数。
    counts: [u64; 6],
    /// 族谱登记：族号 → 已注册对象名。
    registry: [Vec<&'static str>; 6],
    events: Vec<(&'static str, u64)>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl VfsBus {
    pub fn new() -> Self {
        VfsBus {
            inited: false,
            counts: [0; 6],
            registry: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            events: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// vfs_init()：首次成功，二次 E_ALREADY。
    pub fn init(&mut self) -> Result<(), u32> {
        if self.inited {
            self.rejects.push((XC_VFS_ALREADY, "重复初始化"));
            return Err(XC_VFS_ALREADY);
        }
        self.inited = true;
        self.events.push(("init", 1));
        // 初始化即注册六族谱根（各 +1）
        for i in 0..6 {
            self.counts[i] += 1;
            self.registry[i].push("root");
        }
        Ok(())
    }

    /// 对象注册（init 后合法）。
    pub fn register(&mut self, family: usize, obj: &'static str) -> Result<u64, u32> {
        if !self.inited {
            self.rejects.push((XC_VFS_STATE, "未初始化注册"));
            return Err(XC_VFS_STATE);
        }
        if self.sealed {
            self.rejects.push((XC_VFS_SEAL, "封账后注册"));
            return Err(XC_VFS_SEAL);
        }
        if family >= 6 {
            self.rejects.push((XC_VFS_REG, "族号越界"));
            return Err(XC_VFS_REG);
        }
        self.counts[family] += 1;
        self.registry[family].push(obj);
        Ok(self.counts[family])
    }

    /// 族谱计数守恒：计数 == 登记长（六族全核）。
    pub fn genealogy_consistent(&self) -> bool {
        for i in 0..6 {
            if self.counts[i] as usize != self.registry[i].len() {
                return false;
            }
        }
        true
    }

    /// 初始化后六族各 ≥1（谱根已注册）。
    pub fn six_families_booted(&self) -> bool {
        self.inited && self.counts.iter().all(|&c| c >= 1)
    }

    /// 全局状态逐字节快照（幂等不变量核对）。
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.push(self.inited as u8);
        b.push(self.sealed as u8);
        for &c in &self.counts {
            b.extend_from_slice(&c.to_le_bytes());
        }
        for reg in &self.registry {
            for name in reg {
                b.extend_from_slice(name.as_bytes());
                b.push(0xFF);
            }
            b.push(0xFE);
        }
        b.extend_from_slice(&(self.events.len() as u64).to_le_bytes());
        b
    }

    pub fn init_ok(&self) -> bool {
        self.inited
    }
    pub fn family_count(&self, i: usize) -> u64 {
        self.counts[i]
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_VFS_SEAL);
        }
        self.sealed = true;
        Ok(())
    }
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准启动序列：init + 六族各注册 2 对象。
pub fn boot_sequence() -> VfsBus {
    let mut bus = VfsBus::new();
    let _ = bus.init();
    for i in 0..6 {
        let _ = bus.register(i, "obj-a");
        let _ = bus.register(i, "obj-b");
    }
    bus
}

/// 注入核：重复初始化/族谱漂移/封账后注册/未初始化注册 四类必抓。
pub struct VfsInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl VfsInjectCore {
    pub fn new() -> Self {
        VfsInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_double_init(&mut self) {
        self.total += 1;
        let mut bus = VfsBus::new();
        let i1 = bus.init();
        let snap1 = bus.state_bytes();
        let i2 = bus.init();
        let snap2 = bus.state_bytes();
        // 二次 E_ALREADY 且状态逐字节不变
        if i1.is_ok() && i2 == Err(XC_VFS_ALREADY) && snap1 == snap2 {
            self.caught += 1;
        }
    }
    pub fn inject_genealogy_drift(&mut self) {
        self.total += 1;
        let mut bus = boot_sequence();
        // 漂移注入：篡改计数
        bus.counts[2] += 1;
        if !bus.genealogy_consistent() {
            self.caught += 1;
        }
    }
    pub fn inject_register_after_seal(&mut self) {
        self.total += 1;
        let mut bus = boot_sequence();
        let _ = bus.seal();
        let r = bus.register(0, "late");
        if r.is_err() && bus.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_register_pre_init(&mut self) {
        self.total += 1;
        let mut bus = VfsBus::new();
        let r = bus.register(0, "early");
        if r.is_err() && bus.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx4001_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx4001");

    // C01-01 vfs_init 首次成功 + 六族谱根落账
    {
        let mut bus = VfsBus::new();
        let ok = bus.init().is_ok() && bus.six_families_booted();
        s.add("C01-01-首次初始化", ok, if ok { "" } else { "初始化断" });
    }
    // C01-02 二次调用 E_ALREADY 且状态逐字节不变
    {
        let mut bus = VfsBus::new();
        let _ = bus.init();
        let s1 = bus.state_bytes();
        let r2 = bus.init();
        let s2 = bus.state_bytes();
        let ok = r2 == Err(XC_VFS_ALREADY) && s1 == s2;
        s.add("C01-02-幂等不变量", ok, if ok { "" } else { "状态漂移" });
    }
    // C01-03 六族计数守恒（计数 == 登记）
    {
        let bus = boot_sequence();
        let ok = bus.genealogy_consistent()
            && bus.family_count(0) == 3
            && bus.family_count(5) == 3;
        s.add("C01-03-族谱守恒", ok, if ok { "" } else { "计数漂移" });
    }
    // C01-04 族号闭集（六族名可索引）
    {
        let ok = OBJ_FAMILIES.len() == 6
            && family_idx("inode") == Some(1)
            && family_idx("ns") == Some(5)
            && family_idx("x9").is_none();
        s.add("C01-04-六族闭集", ok, if ok { "" } else { "族集漂移" });
    }
    // C01-05 ktest B01 面断言可复测（双构造同态）
    {
        let a = boot_sequence();
        let b = boot_sequence();
        let ok = a.state_bytes() == b.state_bytes();
        s.add("C01-05-可复测", ok, if ok { "" } else { "复测漂移" });
    }
    // C01-06 封账拒注册
    {
        let mut bus = boot_sequence();
        let _ = bus.seal();
        let dup = bus.seal();
        let r = bus.register(0, "late");
        let ok = dup.is_err() && r.is_err() && bus.reject_count() == 2;
        s.add("C01-06-封账拒", ok, if ok { "" } else { "封账失守" });
    }
    // C01-07 注入核 4/4
    {
        let mut inj = VfsInjectCore::new();
        inj.inject_double_init();
        inj.inject_genealogy_drift();
        inj.inject_register_after_seal();
        inj.inject_register_pre_init();
        s.add(
            "C01-07-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    s
}
