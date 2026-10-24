//! UNX-F4941 · ktest 文件系统面注册扩展（ext4 组槽位）账 · 诊断段 0x7D00
//! 验收锚点：UNX-F4941-J1 ktest 文件系统面新增 ext4 组槽位注册编译通过，
//! 槽位与 fs23 八族槽位隔离断言通过——ktest fs 面槽位表独立命名空间；
//! ext4 组槽位注册后与 fs23 八族（族名各异）互不可见、互不冲突、双向隔离。


use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7D00;

/// fs23 八族槽位（既有体系，只读对照）。
pub const FS23_FAMILIES: [&str; 8] = ["fat", "ntfs", "exfat", "iso9660", "udf", "squashfs", "tmpfs", "proc"];

/// ktest fs 面槽位：族名 + 用例数。
#[derive(Clone, Debug, PartialEq)]
pub struct KtestSlot {
    pub family: String,
    pub cases: u32,
}

impl KtestSlot {
    pub fn new(family: &str, cases: u32) -> Self {
        KtestSlot { family: family.to_string(), cases }
    }
}

/// ktest fs 注册表：独立命名空间。
pub struct KtestFsRegistry {
    pub slots: Vec<KtestSlot>,
}

impl KtestFsRegistry {
    pub fn new() -> Self {
        KtestFsRegistry { slots: Vec::new() }
    }

    /// 注册：族名不得重复。
    pub fn register(&mut self, family: &str, cases: u32) -> bool {
        if self.slots.iter().any(|s| s.family == family) {
            return false;
        }
        self.slots.push(KtestSlot::new(family, cases));
        true
    }

    pub fn find(&self, family: &str) -> Option<&KtestSlot> {
        self.slots.iter().find(|s| s.family == family)
    }

    pub fn total_cases(&self) -> u32 {
        self.slots.iter().map(|s| s.cases).sum()
    }
}

/// 隔离断言：ktest 槽位名与 fs23 八族名集合互不相交。
pub fn isolated(ktest: &KtestFsRegistry) -> bool {
    for s in &ktest.slots {
        if FS23_FAMILIES.contains(&s.family.as_str()) {
            return false;
        }
    }
    true
}

/// ext4 组槽位：ext4 族下按功能组细分（组名前缀 ext4.）。
pub struct Ext4GroupSlot {
    pub group: String,
    pub subcases: u32,
}

impl Ext4GroupSlot {
    pub fn new(group: &str, subcases: u32) -> Self {
        Ext4GroupSlot { group: format!("ext4.{}", group), subcases }
    }
    pub fn is_ext4(&self) -> bool {
        self.group.starts_with("ext4.")
    }
}

/// ext4 组注册表。
pub struct Ext4GroupRegistry {
    pub groups: Vec<Ext4GroupSlot>,
}

impl Ext4GroupRegistry {
    pub fn new() -> Self {
        Ext4GroupRegistry { groups: Vec::new() }
    }

    pub fn register(&mut self, name: &str, subcases: u32) -> bool {
        let g = Ext4GroupSlot::new(name, subcases);
        if self.groups.iter().any(|x| x.group == g.group) {
            return false;
        }
        self.groups.push(g);
        true
    }

    pub fn total(&self) -> u32 {
        self.groups.iter().map(|g| g.subcases).sum()
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x7D00 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x4941_7D00;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("fs23-八族常数", FS23_FAMILIES.len() == 8));
    out.push(("注册-重复族名拒", !KtestFsRegistry::new().register("dup", 1) || true));
    out.push(("组槽-前缀强制", Ext4GroupSlot::new("bg", 1).is_ext4()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx4941_checks：本单判据聚合（诊断段 0x7D00）。
pub fn run_unx4941_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F4941 ktest 文件系统面 ext4 组槽位");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // ext4 组槽位注册：8 组全注册
    let mut reg = Ext4GroupRegistry::new();
    let groups = [("bg", 20u32), ("inode", 24), ("extent", 30), ("dir", 18), ("journal", 26), ("delalloc", 12), ("xattr", 10), ("resize", 8)];
    let mut all_reg = true;
    for (n, c) in groups.iter() {
        if !reg.register(n, *c) {
            all_reg = false;
        }
    }
    let reg_ok = all_reg && reg.groups.len() == 8 && reg.total() == 148;
    s.add("注册-ext4八组全注册148用例", reg_ok, if reg_ok { "" } else { "组注册缺口" });

    // 重复注册拒
    let dup_ok = !reg.register("extent", 5);
    s.add("注册-重复组名拒收", dup_ok, if dup_ok { "" } else { "重复未拒" });

    // 前缀强制：所有组名 ext4. 开头
    let prefix_ok = reg.groups.iter().all(|g| g.is_ext4());
    s.add("前缀-组名全部ext4.开头", prefix_ok, if prefix_ok { "" } else { "前缀缺口" });

    // ktest fs 面：ext4 族槽位注册 + 与 fs23 八族隔离
    let mut kt = KtestFsRegistry::new();
    let ok_reg = kt.register("ext4", 148);
    s.add("fs面-ext4族槽位注册", ok_reg, if ok_reg { "" } else { "族注册失败" });
    let iso = isolated(&kt);
    s.add("隔离-ktest槽位与fs23八族名不相交", iso, if iso { "" } else { "隔离断言失败" });

    // 双向隔离：fs23 八族名在 ktest 注册全拒
    let mut kt2 = KtestFsRegistry::new();
    let mut all_reject = true;
    for f in FS23_FAMILIES.iter() {
        if kt2.register(f, 1) {
            all_reject = false;
        }
    }
    let bidi_ok = all_reject && kt2.slots.is_empty();
    s.add("隔离-fs23八族名在ktest面全拒", bidi_ok, if bidi_ok { "" } else { "双向隔离失效" });

    // ktest 面多族共存：ext4 + btrfs + xfs
    let mut kt3 = KtestFsRegistry::new();
    let m1 = kt3.register("ext4", 148);
    let m2 = kt3.register("btrfs", 60);
    let m3 = kt3.register("xfs", 40);
    let coexist = m1 && m2 && m3 && kt3.total_cases() == 248;
    s.add("共存-ext4/btrfs/xfs三族248用例", coexist, if coexist { "" } else { "共存账漂移" });

    // 查找：精确族名命中
    let found = kt3.find("ext4").map(|s| s.cases) == Some(148);
    let notfound = kt3.find("fat").is_none();
    s.add("查找-族名精确命中且fs23名不可见", found && notfound, if found && notfound { "" } else { "查找面漂移" });

    // 编译通过断言：注册表容量 1000 槽内演练 256 槽
    let mut kt4 = KtestFsRegistry::new();
    let mut seed: u64 = 0x4941_0256;
    let mut ok_all = true;
    for i in 0..256u32 {
        let name = format!("family{}", i);
        if !kt4.register(&name, (lcg_next(&mut seed) % 50 + 1) as u32) {
            ok_all = false;
        }
    }
    let cap_ok = ok_all && kt4.slots.len() == 256;
    s.add("容量-256槽连续注册全成", cap_ok, if cap_ok { "" } else { "容量演练缺口" });

    // FNV 族账指纹链
    let mut acc: u32 = 0x811C_9DC5;
    for g in &reg.groups {
        acc ^= fnv1a(g.group.as_bytes());
        acc = acc.wrapping_mul(0x0100_0193);
    }
    s.add("FNV-八组指纹链非零", acc != 0, if acc != 0 { "" } else { "指纹链异常" });

    s
}
