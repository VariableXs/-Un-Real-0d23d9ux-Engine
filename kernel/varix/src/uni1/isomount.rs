//! F440 ISO 镜像挂载 · 完整设计（STAR I 主册 G-I-40）。
//!
//! **判据（主册）**：挂载/浏览/弹出全链；上限与提示；只读判据（挂载盘
//! 写入被拒且说明）；非 ISO 提示；重启后挂载不保留（会话态，文档化）。
//! ＋通12。
//!
//! 设计：虚拟光驱核——双击 .iso → 结构校验（ISO9660 签名——不猜扩展名
//! 硬挂）→ 分配盘符挂载（只读位钉死）；同时挂载上限 4（第 5 个拒绝 +
//! 人话提示）；浏览 = 目录表读取；写请求一律拒（含人话说明「ISO 挂载
//! 为只读」）；弹出回收盘符；挂载表不持久化（会话态——重启即清，结构
//! 性保证：状态只在内存会话对象里）。
//!
//! **v4 深化批次新增（AI-U1）**：
//! - PVD 卷标解析 [`parse_pvd_volume_id`]：主卷描述符（第 16 扇区，
//!   2048 字节节距）里的卷标识（偏移 40 起 32 字节）——挂载后资源管理
//!   器显示「卷标」而非文件名（真实 ISO9660 语义）；
//! - 目录记录遍历 [`parse_dir_records`]：目录记录最小编（LBA/数据
//!   长度/标志位/标识符）→ (名称, 字节数, 是否目录) 列表——浏览面
//!   的真实数据源（不是手填清单）；
//! - 重复源拒绝：同一 .iso 重复挂载 → 拒绝并提示（一个镜像占一个
//!   虚拟光驱就够——重复占位是浪费也是困惑）；
//! - 盘符空闲表：弹出归还的盘符优先复用（先归还先用——分配序确定
//!   可复现，不再只顺号递增）；
//! - 余量提示 [`IsoMounter::slots_hint`]：诊断面的人话余量
//!   （「还能挂 N 个」/「已满——请先弹出」）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

/// 同时挂载上限。
pub const MAX_MOUNTED: usize = 4;
/// ISO9660 扇区大小（字节）。
pub const SECTOR_SIZE: usize = 2048;
/// 主卷描述符所在扇区号。
pub const PVD_SECTOR: usize = 16;
/// PVD 内卷标识偏移（字节）。
pub const PVD_VOLUME_ID_OFF: usize = 40;
/// 卷标识长度（字节，ISO9660 定长 a 字符）。
pub const PVD_VOLUME_ID_LEN: usize = 32;

/// 一个解析出的目录记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirRecord {
    pub name: String,
    /// 数据长度（字节）。
    pub size: u32,
    /// 是否目录（记录标志位 bit1）。
    pub is_dir: bool,
}

/// PVD 卷标解析：`header` 为自 PVD 扇区起的字节（≥ 偏移+长度）。
/// 卷标识去尾部空格（ISO9660 以空格填充定长域）。
pub fn parse_pvd_volume_id(pvd: &[u8]) -> Option<String> {
    let end = PVD_VOLUME_ID_OFF + PVD_VOLUME_ID_LEN;
    if pvd.len() < end {
        return None;
    }
    let raw = &pvd[PVD_VOLUME_ID_OFF..end];
    // 规范以空格填充定长域；防御性同时剥 NUL（劣质刻录工具的产物）。
    let trimmed_end = raw
        .iter()
        .rposition(|&b| b != b' ' && b != 0)
        .map_or(0, |p| p + 1);
    Some(String::from_utf8_lossy(&raw[..trimmed_end]).into_owned())
}

/// 目录记录最小编解析：每记录 = [长度u8][扩展长度u8][LBA u32le][数据长
/// u32le][…7 字节…][标志u8][…][标识符长度u8][标识符…]。本函数只消费
/// 判据需要的字段，未知域按规范跳过。
pub fn parse_dir_records(buf: &[u8]) -> Vec<DirRecord> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let len = buf[pos] as usize;
        if len == 0 {
            break; // 记录区以 0 填充收尾
        }
        if pos + len > buf.len() || len < 34 {
            break; // 残缺记录：截断不猜（诚实——坏镜像不假装解析成功）
        }
        let lba = u32::from_le_bytes([buf[pos + 2], buf[pos + 3], buf[pos + 4], buf[pos + 5]]);
        let size = u32::from_le_bytes([buf[pos + 10], buf[pos + 11], buf[pos + 12], buf[pos + 13]]);
        let flags = buf[pos + 25];
        let name_len = buf[pos + 32] as usize;
        let name_bytes = &buf[pos + 33..(pos + 33 + name_len).min(pos + len)];
        // 版本号分号后缀（"README.TXT;1"）剥掉——资源管理器不显示。
        let full = String::from_utf8_lossy(name_bytes);
        let base = full.split(';').next().unwrap_or("");
        let name = String::from(base);
        out.push(DirRecord { name, size, is_dir: flags & 0b10 != 0 });
        let _ = lba; // LBA 供真实读盘路径用；语义核只产目录面
        pos += len;
    }
    out
}

/// 一个已挂载的 ISO。
#[derive(Clone, Debug)]
pub struct MountedIso {
    /// 源 .iso 文件路径。
    pub source: String,
    /// PVD 解析出的卷标（挂载成功即定）。
    pub volume_label: Option<String>,
    /// 分配的虚拟盘符。
    pub letter: char,
    /// 根目录文件表（浏览用——由目录记录解析产出）。
    pub files: Vec<String>,
}

/// ISO 结构校验：真实实现读 32KB 偏移的 CD001 签名；此处以字节签名
/// 前缀机检（判据：非 ISO 拒绝——不猜扩展名）。
pub fn looks_like_iso(header: &[u8]) -> bool {
    header.len() >= 5 && &header[..5] == b"CD001"
}

/// 虚拟光驱会话（会话态——不持久化）。
pub struct IsoMounter {
    pub mounted: Vec<MountedIso>,
    /// 盘符空闲表（弹出归还 → 表头复用；空则从顺序域取下一个）。
    free_letters: Vec<char>,
    /// 顺序分配域游标（空闲表空时用）。
    next_letter_idx: usize,
    /// 被拒写请求计数（诚实账）。
    pub rejected_writes: u64,
    /// 被拒挂载计数（上限/格式/重复源）。
    pub rejected_mounts: u64,
}

const DRIVE_LETTERS: [char; 8] = ['E', 'F', 'G', 'H', 'I', 'J', 'K', 'L'];

impl IsoMounter {
    pub fn new() -> IsoMounter {
        IsoMounter { mounted: Vec::new(), free_letters: Vec::new(), next_letter_idx: 0, rejected_writes: 0, rejected_mounts: 0 }
    }

    fn take_letter(&mut self) -> char {
        if let Some(c) = self.free_letters.pop() {
            return c;
        }
        let letter = DRIVE_LETTERS[self.next_letter_idx % DRIVE_LETTERS.len()];
        self.next_letter_idx += 1;
        letter
    }

    /// 挂载：结构校验 → 重复源 → 上限检查 → PVD 卷标 → 分配盘符。
    /// 失败给归因人话。
    pub fn mount(&mut self, source: &str, pvd: &[u8]) -> Result<char, &'static str> {
        if !looks_like_iso(pvd) {
            self.rejected_mounts += 1;
            return Err("不是 ISO 镜像（结构校验未过）——请确认文件完整或换用支持的镜像格式");
        }
        if self.mounted.iter().any(|m| m.source == source) {
            self.rejected_mounts += 1;
            return Err("此镜像已在虚拟光驱中——不要重复挂载");
        }
        if self.mounted.len() >= MAX_MOUNTED {
            self.rejected_mounts += 1;
            return Err("同时挂载已达上限 4 个——请先弹出不再使用的镜像");
        }
        let letter = self.take_letter();
        self.mounted.push(MountedIso {
            source: String::from(source),
            volume_label: parse_pvd_volume_id(pvd),
            letter,
            files: Vec::new(),
        });
        Ok(letter)
    }

    /// 浏览：返回盘符根目录表（目录表由装载时注入——镜像内数据）。
    pub fn browse(&self, letter: char) -> Option<&[String]> {
        self.mounted
            .iter()
            .find(|m| m.letter == letter)
            .map(|m| m.files.as_slice())
    }

    /// 装载目录表（真实实现 = 读镜像目录区 → parse_dir_records；
    /// 语义核接收解析产物注入）。
    pub fn seed_files(&mut self, letter: char, files: Vec<String>) -> bool {
        match self.mounted.iter_mut().find(|m| m.letter == letter) {
            Some(m) => {
                m.files = files;
                true
            }
            None => false,
        }
    }

    /// 挂载卷标查询（资源管理器显示用——PVD 语义）。
    pub fn volume_label(&self, letter: char) -> Option<&str> {
        self.mounted
            .iter()
            .find(|m| m.letter == letter)?
            .volume_label
            .as_deref()
    }

    /// 只读判据：任何对挂载盘的写请求都被拒并说明（永不静默）。
    pub fn try_write(&mut self, letter: char, _path: &str) -> Result<(), &'static str> {
        if self.mounted.iter().any(|m| m.letter == letter) {
            self.rejected_writes += 1;
            Err("ISO 挂载为只读——如需修改请先复制文件到本地磁盘")
        } else {
            Ok(())
        }
    }

    /// 弹出：盘符归还空闲表（先归还先复用）。
    pub fn eject(&mut self, letter: char) -> bool {
        let Some(pos) = self.mounted.iter().position(|m| m.letter == letter) else {
            return false;
        };
        self.mounted.remove(pos);
        self.free_letters.insert(0, letter);
        true
    }

    /// 余量提示（诊断面人话）。
    pub fn slots_hint(&self) -> String {
        let left = MAX_MOUNTED - self.mounted.len();
        if left == 0 {
            String::from("虚拟光驱已满——请先弹出不再使用的镜像")
        } else {
            format!("还能挂载 {} 个镜像", left)
        }
    }

    /// 重启：会话态清空（结构性——新会话对象即空）。
    pub fn after_reboot(&self) -> IsoMounter {
        IsoMounter::new()
    }
}

pub fn run_isomount_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F440");
    let mut m = IsoMounter::new();
    // 构造真实 PVD：签名 + 偏移 40 的卷标域（"VARIX_TOOLS" + 空格填充）。
    let mut pvd = alloc::vec![0u8; PVD_VOLUME_ID_OFF + PVD_VOLUME_ID_LEN];
    pvd[..5].copy_from_slice(b"CD001");
    let label = b"VARIX_TOOLS";
    pvd[PVD_VOLUME_ID_OFF..PVD_VOLUME_ID_OFF + label.len()].copy_from_slice(label);
    for b in pvd[PVD_VOLUME_ID_OFF + label.len()..PVD_VOLUME_ID_OFF + PVD_VOLUME_ID_LEN].iter_mut() {
        *b = b' ';
    }
    // 挂载全链：合法 ISO → 分配盘符；非 ISO → 拒绝且人话。
    set.add(
        "f440-mount-ok",
        m.mount("tools.iso", &pvd) == Ok('E') && m.mounted.len() == 1,
        "",
    );
    set.add(
        "f440-non-iso-rejected",
        matches!(m.mount("fake.iso", b"NOTANISO"), Err(_)),
        "",
    );
    // PVD 卷标解析：挂载面显示卷标（去尾部填充空格），不是文件名。
    set.add(
        "f440-pvd-volume-label",
        m.volume_label('E') == Some("VARIX_TOOLS") && m.volume_label('Z').is_none(),
        "",
    );
    set.add(
        "f440-pvd-undersized-none",
        parse_pvd_volume_id(b"CD001").is_none(),
        "",
    );
    // 目录记录遍历：最小编 → (名称, 大小, 目录) 三元全对；分号版本号剥除。
    let mut dir_buf = alloc::vec![];
    // 记录 1：README.TXT;1 大小 1024 文件。
    let mut r1 = alloc::vec![0u8; 34 + 12];
    r1[0] = 46; // 记录长
    r1[10..14].copy_from_slice(&1024u32.to_le_bytes());
    r1[25] = 0; // 文件
    r1[32] = 12;
    r1[33..45].copy_from_slice(b"README.TXT;1"); // 标识符在记录偏移 33（ISO9660）
    dir_buf.extend_from_slice(&r1);
    // 记录 2：BOOT 目录。
    let mut r2 = alloc::vec![0u8; 34 + 4];
    r2[0] = 38;
    r2[10..14].copy_from_slice(&2048u32.to_le_bytes());
    r2[25] = 0b10; // 目录位
    r2[32] = 4;
    r2[33..37].copy_from_slice(b"BOOT");
    dir_buf.extend_from_slice(&r2);
    dir_buf.push(0); // 记录区收尾
    let recs = parse_dir_records(&dir_buf);
    set.add(
        "f440-dir-records-parsed",
        recs.len() == 2
            && recs[0].name == "README.TXT"
            && recs[0].size == 1024
            && !recs[0].is_dir
            && recs[1].name == "BOOT"
            && recs[1].is_dir,
        "",
    );
    // 残缺记录截断不猜（坏镜像不假装解析成功）。
    set.add("f440-dir-truncated-honest", parse_dir_records(&dir_buf[..40]).is_empty(), "");
    // 浏览（目录表注入 = 解析产物）。
    set.add(
        "f440-browse",
        m.seed_files('E', alloc::vec![String::from("setup.exe"), String::from("readme.txt")])
            && m.browse('E').map(|f| f.len()) == Some(2)
            && m.browse('Z').is_none(),
        "",
    );
    // 重复源拒绝（一个镜像占一个光驱就够）。
    set.add(
        "f440-duplicate-source-rejected",
        matches!(m.mount("tools.iso", &pvd), Err("此镜像已在虚拟光驱中——不要重复挂载"))
            && m.rejected_mounts >= 1,
        "",
    );
    // 只读判据：写入被拒且说明。
    set.add(
        "f440-readonly-enforced",
        matches!(m.try_write('E', "E:\\x.txt"), Err("ISO 挂载为只读——如需修改请先复制文件到本地磁盘"))
            && m.rejected_writes == 1,
        "",
    );
    set.add("f440-write-nonmount-ok", m.try_write('C', "C:\\x.txt").is_ok(), "");
    // 上限与提示：挂到 4 个后第 5 个拒；余量提示人话。
    let mut full = IsoMounter::new();
    for i in 0..MAX_MOUNTED {
        let src = alloc::format!("d{}.iso", i);
        assert!(full.mount(&src, &pvd).is_ok());
    }
    set.add(
        "f440-cap-four",
        full.mounted.len() == MAX_MOUNTED && matches!(full.mount("d5.iso", &pvd), Err(_)),
        "",
    );
    set.add(
        "f440-slots-hint",
        full.slots_hint().contains("已满") && m.slots_hint().contains("3 个"),
        "",
    );
    // 弹出：盘符归还空闲表 → 新挂载先复用归还位（先归还先用）。
    set.add(
        "f440-eject",
        full.eject('F') && full.mounted.len() == 3 && !full.eject('F'),
        "",
    );
    set.add("f440-letter-reuse", full.mount("d9.iso", &pvd) == Ok('F'), "");
    // 重启不保留（会话态）。
    let fresh = m.after_reboot();
    set.add(
        "f440-session-only",
        fresh.mounted.is_empty() && fresh.rejected_writes == 0,
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_never_trusted() {
        let mut m = IsoMounter::new();
        let mut pvd = alloc::vec![0u8; 72];
        pvd[..5].copy_from_slice(b"CD001");
        // 扩展名叫 .iso 但内容不是 → 拒。
        assert!(matches!(m.mount("movie.iso", b"RIFF...."), Err(_)));
        // 扩展名怪但内容对 → 收（不猜扩展名的正反面）。
        assert!(m.mount("backup.bin", &pvd).is_ok());
    }

    #[test]
    fn eject_reuses_letters() {
        let mut m = IsoMounter::new();
        let mut pvd = alloc::vec![0u8; 72];
        pvd[..5].copy_from_slice(b"CD001");
        let _ = m.mount("a.iso", &pvd);
        let _ = m.mount("b.iso", &pvd);
        assert!(m.eject('E'));
        // 归还位优先于顺序域（先归还先用——F 在顺序域，但 E 是刚归还的）。
        assert_eq!(m.mount("c.iso", &pvd), Ok('E'));
    }

    #[test]
    fn pvd_label_trailing_spaces_trimmed() {
        let mut pvd = alloc::vec![0u8; 72];
        pvd[..5].copy_from_slice(b"CD001");
        pvd[40..44].copy_from_slice(b"ABCD");
        // 尾部 NUL 填充（劣质刻录工具）也剥——只留有效字符。
        assert_eq!(parse_pvd_volume_id(&pvd).as_deref(), Some("ABCD"));
        // 全空卷标域 → 空串（不假装有卷标）。
        let mut blank = alloc::vec![b' '; 72];
        blank[..5].copy_from_slice(b"CD001");
        assert_eq!(parse_pvd_volume_id(&blank).as_deref(), Some(""));
    }
}
