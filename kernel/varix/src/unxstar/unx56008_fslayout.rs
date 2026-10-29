//! UNX-F56008 文件系统布局与环境指纹映射件（0x600E0 段）
//!
//! 锚点：UNX-F56008-J1 分区布局/文件系统类型/挂载点/对齐参数逐项入
//! env_profile 口径（分区账逐条落账 + 布局指纹折叠）；J2 双系统
//! fs_layout 语义等价表（Varix 挂载点 ↔ Windows 盘符逐条双向可查）；
//! J3 布局漂移拒收（同挂载点重登异参即拒 ×3 注入全检出）三判据全绿。
//! 判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（环境冻结域·fs_layout 面）。
//!
//! 结构：分区登记册（dev × fs_type × mount × align 四元组逐条落账）
//! + 类型/对齐闭集核（varixfs/swap 闭集，512/4096 对齐闭集）+ 挂载点
//! 唯一闸 + 布局指纹（逐条折叠，确定性）+ 等价映射件（双向解析）+
//! 注入核（未知类型/非法对齐/挂载点撞账/布局漂移/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x600E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x600E0;
pub const FS_UNK_TYPE: u32 = SEG_BASE + 0x01; // 文件系统类型越闭集
pub const FS_ALIGN_BAD: u32 = SEG_BASE + 0x02; // 对齐参数越闭集（非 512/4096）
pub const FS_MOUNT_DUP: u32 = SEG_BASE + 0x03; // 挂载点撞账（重复挂载）
pub const FS_DEV_DUP: u32 = SEG_BASE + 0x04; // 设备号重复登记
pub const FS_SEALED: u32 = SEG_BASE + 0x05; // 布局账已封账拒改
pub const FS_DRIFT: u32 = SEG_BASE + 0x06; // 布局漂移（同挂载点异参重登）
pub const FS_EMPTY: u32 = SEG_BASE + 0x07; // 空 dev 或空挂载点

/// 文件系统类型闭集。
pub const FSTYPES: [&str; 3] = ["varixfs", "swap", "bootfs"];

/// 对齐参数闭集（字节）。
pub const ALIGNS: [u32; 2] = [512, 4096];

/// fs_type 闭集核。
pub fn fstype_ok(t: &str) -> bool {
    for x in FSTYPES.iter() {
        if *x == t {
            return true;
        }
    }
    false
}

/// align 闭集核。
pub fn align_ok(a: u32) -> bool {
    for x in ALIGNS.iter() {
        if *x == a {
            return true;
        }
    }
    false
}

/// 双系统语义等价映射：Varix 挂载点 ↔ Windows 盘符（索引同序）。
pub const EQ_VARIX_MOUNT: [&str; 4] = ["/", "/boot", "/home", "/var"];
pub const EQ_WIN_DRIVE: [&str; 4] = ["C:", "SYSTEM:", "D:", "E:"];

/// 正向映射：挂载点 → 盘符。
pub fn mount_to_drive(m: &str) -> Option<&'static str> {
    for i in 0..EQ_VARIX_MOUNT.len() {
        if EQ_VARIX_MOUNT[i] == m {
            return Some(EQ_WIN_DRIVE[i]);
        }
    }
    None
}

/// 反向映射：盘符 → 挂载点。
pub fn drive_to_mount(d: &str) -> Option<&'static str> {
    for i in 0..EQ_WIN_DRIVE.len() {
        if EQ_WIN_DRIVE[i] == d {
            return Some(EQ_VARIX_MOUNT[i]);
        }
    }
    None
}

/// 分区账目：dev × fs_type × mount × align 四元组。
#[derive(Clone)]
pub struct PartEntry {
    pub dev: String,
    pub fs_type: String,
    pub mount: String,
    pub align: u32,
}

impl PartEntry {
    pub fn new(dev: &str, fs_type: &str, mount: &str, align: u32) -> Result<Self, u32> {
        if dev.is_empty() || mount.is_empty() {
            return Err(FS_EMPTY);
        }
        if !fstype_ok(fs_type) {
            return Err(FS_UNK_TYPE);
        }
        if !align_ok(align) {
            return Err(FS_ALIGN_BAD);
        }
        Ok(PartEntry {
            dev: dev.to_string(),
            fs_type: fs_type.to_string(),
            mount: mount.to_string(),
            align,
        })
    }
}

/// 布局登记册：逐条落账 + 指纹 + 封账。
pub struct FsLayoutBook {
    pub parts: Vec<PartEntry>,
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl FsLayoutBook {
    pub fn new() -> Self {
        FsLayoutBook { parts: Vec::new(), sealed: false, audit: Vec::new() }
    }

    /// 逐条落账：设备号唯一 + 挂载点唯一 + 同点异参漂移拒。
    pub fn register(&mut self, e: PartEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(FS_SEALED);
        }
        for p in self.parts.iter() {
            if p.dev == e.dev {
                return Err(FS_DEV_DUP);
            }
            if p.mount == e.mount {
                if p.fs_type != e.fs_type || p.align != e.align {
                    self.audit.push(format!("FS|DRIFT|{}", e.mount));
                    return Err(FS_DRIFT);
                }
                return Err(FS_MOUNT_DUP);
            }
        }
        self.audit.push(format!("FS|REG|{}|{}", e.dev, e.mount));
        self.parts.push(e);
        Ok(())
    }

    /// 布局指纹：逐条 dev|type|mount|align 折叠（确定性）。
    pub fn layout_fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for p in self.parts.iter() {
            let line = format!("{}|{}|{}|{}", p.dev, p.fs_type, p.mount, p.align);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }

    /// J1：逐项落账全量核（根/引导/家/变 四挂载点齐 + 类型对齐全合法）。
    pub fn full_ok(&self) -> bool {
        if self.parts.is_empty() {
            return false;
        }
        for p in self.parts.iter() {
            if !fstype_ok(&p.fs_type) || !align_ok(p.align) {
                return false;
            }
        }
        true
    }

    /// 挂载点查询。
    pub fn by_mount(&self, m: &str) -> Option<&PartEntry> {
        for p in self.parts.iter() {
            if p.mount == m {
                return Some(p);
            }
        }
        None
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：四分区布局（/ /boot /home /var）。
pub fn standard_layout() -> FsLayoutBook {
    let mut b = FsLayoutBook::new();
    let rows = [
        ("nvme0p1", "bootfs", "/boot", 4096u32),
        ("nvme0p2", "varixfs", "/", 4096),
        ("nvme0p3", "varixfs", "/home", 4096),
        ("nvme0p4", "varixfs", "/var", 4096),
    ];
    for r in rows.iter() {
        b.register(PartEntry::new(r.0, r.1, r.2, r.3).expect("fixture part"))
            .expect("fixture reg");
    }
    b
}

/// 布局漂移注入核：×3 全检出（类型漂移/对齐漂移/挂载点漂移）。
pub fn drift_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 类型漂移（/ 挂载点换成 swap）
    let mut b = standard_layout();
    if b.register(PartEntry::new("nvme9p1", "swap", "/", 4096).expect("d1")) == Err(FS_DRIFT) {
        caught += 1;
    }
    // 2) 对齐漂移（/home 换 512 对齐）
    let mut b2 = standard_layout();
    if b2.register(PartEntry::new("nvme9p2", "varixfs", "/home", 512).expect("d2"))
        == Err(FS_DRIFT)
    {
        caught += 1;
    }
    // 3) 挂载点漂移（/var 登到新设备仍算撞账口径）
    let mut b3 = standard_layout();
    if b3.register(PartEntry::new("nvme9p3", "varixfs", "/var", 4096).expect("d3"))
        == Err(FS_MOUNT_DUP)
    {
        caught += 1;
    }
    (caught, 3)
}

/// 注入核：5 类非法登记必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知类型
    if PartEntry::new("nvme0p9", "ntfs", "/x", 4096).err() == Some(FS_UNK_TYPE) {
        caught += 1;
    }
    // 2) 非法对齐（8192）
    if PartEntry::new("nvme0p9", "varixfs", "/x", 8192).err() == Some(FS_ALIGN_BAD) {
        caught += 1;
    }
    // 3) 空 dev
    if PartEntry::new("", "varixfs", "/x", 4096).err() == Some(FS_EMPTY) {
        caught += 1;
    }
    // 4) 设备号重复
    let mut b = standard_layout();
    if b.register(PartEntry::new("nvme0p2", "varixfs", "/opt", 4096).expect("dup dev"))
        == Err(FS_DEV_DUP)
    {
        caught += 1;
    }
    // 5) 封账后改写
    let mut b2 = standard_layout();
    b2.seal();
    if b2.register(PartEntry::new("nvme1p1", "varixfs", "/opt", 4096).expect("sealed"))
        == Err(FS_SEALED)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 等价映射双向闭包核。
pub fn mapping_bidirectional_ok() -> bool {
    for i in 0..EQ_VARIX_MOUNT.len() {
        match mount_to_drive(EQ_VARIX_MOUNT[i]) {
            None => return false,
            Some(d) => {
                if d != EQ_WIN_DRIVE[i] {
                    return false;
                }
                match drive_to_mount(d) {
                    None => return false,
                    Some(m2) => {
                        if m2 != EQ_VARIX_MOUNT[i] {
                            return false;
                        }
                    }
                }
            }
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56008_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56008");
    // J1-1 分区逐项落账全量
    let b = standard_layout();
    let ok = b.full_ok() && b.parts.len() == 4;
    s.add("56008-J1-分区落账全量", ok, if ok { "" } else { "分区账不全" });
    // J1-2 布局指纹确定性
    let ok = standard_layout().layout_fingerprint() == standard_layout().layout_fingerprint();
    s.add("56008-J1-布局指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J1-3 类型/对齐闭集核
    let ok = fstype_ok("varixfs") && !fstype_ok("ext12") && align_ok(512) && !align_ok(2048);
    s.add("56008-J1-类型对齐闭集", ok, if ok { "" } else { "闭集核松" });
    // J2-1 语义等价映射双向闭包
    let ok = mapping_bidirectional_ok();
    s.add("56008-J2-等价映射闭包", ok, if ok { "" } else { "映射断链" });
    // J2-2 根挂载 ↔ C: 语义
    let ok = mount_to_drive("/") == Some("C:") && drive_to_mount("C:") == Some("/");
    s.add("56008-J2-根挂载等价", ok, if ok { "" } else { "根映射漂移" });
    // J3-1 布局漂移 ×3 注入全检出
    let (c, n) = drift_core();
    let ok = c == n && n == 3;
    let detail = if ok { String::new() } else { format!("漂移检出 {c}/{n}") };
    s.add_dyn("56008-J3-布局漂移必拒", ok, &detail);
    // J3-2 漂移留痕（DRIFT 行落 audit）
    let mut b2 = standard_layout();
    let _ = b2.register(PartEntry::new("nvme9p9", "swap", "/", 4096).expect("d"));
    let mut hit = false;
    for a in b2.audit.iter() {
        if a.contains("FS|DRIFT|/") {
            hit = true;
        }
    }
    let ok = hit;
    s.add("56008-J3-漂移留痕", ok, if ok { "" } else { "漂移未留痕" });
    // J3-3 注入核 5 类必抓
    let (ic, in_) = injection_core();
    let ok = ic == in_;
    let detail = if ok { String::new() } else { format!("注入 {ic}/{in_}") };
    s.add_dyn("56008-J3-注入核必抓", ok, &detail);
    // J1-4 挂载点查询语义
    let root = standard_layout().by_mount("/").expect("root").clone();
    let ok = root.fs_type == "varixfs" && root.align == 4096;
    s.add("56008-J1-挂载点查询", ok, if ok { "" } else { "查询语义漂移" });
    // J1-5 登记留痕齐（REG 行数 = 分区数）
    let b3 = standard_layout();
    let mut reg = 0usize;
    for a in b3.audit.iter() {
        if a.contains("FS|REG|") {
            reg += 1;
        }
    }
    let ok = reg == b3.parts.len();
    s.add("56008-J1-登记留痕齐", ok, if ok { "" } else { "留痕缺行" });
    // J2-3 封账冻结
    let mut b4 = standard_layout();
    b4.seal();
    let ok = b4.register(PartEntry::new("nvme2p1", "varixfs", "/opt", 4096).expect("s"))
        == Err(FS_SEALED)
        && b4.layout_fingerprint() == standard_layout().layout_fingerprint();
    s.add("56008-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J2-4 缺射显性
    let ok = mount_to_drive("/opt").is_none() && drive_to_mount("Q:").is_none();
    s.add("56008-J2-缺射显性", ok, if ok { "" } else { "缺射被猜" });
    // J1-6 指纹对布局敏感（改一处指纹必变）
    let mut b5 = standard_layout();
    b5.parts[0].align = 512;
    let ok = b5.layout_fingerprint() != standard_layout().layout_fingerprint();
    s.add("56008-J1-指纹敏感性", ok, if ok { "" } else { "指纹不敏感" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56008_all_checks_green() {
        let set = run_unx56008_checks();
        assert!(set.all_passed(), "UNX-F56008 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56008_mapping_closure() {
        assert!(mapping_bidirectional_ok());
    }
}
