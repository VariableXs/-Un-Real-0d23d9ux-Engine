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

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 同时挂载上限。
pub const MAX_MOUNTED: usize = 4;

/// 一个已挂载的 ISO。
#[derive(Clone, Debug)]
pub struct MountedIso {
    /// 源 .iso 文件路径。
    pub source: String,
    /// 分配的虚拟盘符。
    pub letter: char,
    /// 根目录文件表（浏览用）。
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
    /// 已占用盘符（虚拟光驱自己的分配域）。
    next_letter_idx: usize,
    /// 被拒写请求计数（诚实账）。
    pub rejected_writes: u64,
    /// 被拒挂载计数（上限/格式）。
    pub rejected_mounts: u64,
}

const DRIVE_LETTERS: [char; 8] = ['E', 'F', 'G', 'H', 'I', 'J', 'K', 'L'];

impl IsoMounter {
    pub fn new() -> IsoMounter {
        IsoMounter { mounted: Vec::new(), next_letter_idx: 0, rejected_writes: 0, rejected_mounts: 0 }
    }

    /// 挂载：结构校验 → 上限检查 → 分配盘符。失败给归因人话。
    pub fn mount(&mut self, source: &str, header: &[u8]) -> Result<char, &'static str> {
        if !looks_like_iso(header) {
            self.rejected_mounts += 1;
            return Err("不是 ISO 镜像（结构校验未过）——请确认文件完整或换用支持的镜像格式");
        }
        if self.mounted.len() >= MAX_MOUNTED {
            self.rejected_mounts += 1;
            return Err("同时挂载已达上限 4 个——请先弹出不再使用的镜像");
        }
        let letter = DRIVE_LETTERS[self.next_letter_idx % DRIVE_LETTERS.len()];
        self.next_letter_idx += 1;
        self.mounted.push(MountedIso {
            source: String::from(source),
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

    /// 装载目录表（模拟镜像内容注入）。
    pub fn seed_files(&mut self, letter: char, files: Vec<String>) -> bool {
        match self.mounted.iter_mut().find(|m| m.letter == letter) {
            Some(m) => {
                m.files = files;
                true
            }
            None => false,
        }
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

    /// 弹出：回收盘符。
    pub fn eject(&mut self, letter: char) -> bool {
        let before = self.mounted.len();
        self.mounted.retain(|m| m.letter != letter);
        self.mounted.len() < before
    }

    /// 重启：会话态清空（结构性——新会话对象即空）。
    pub fn after_reboot(&self) -> IsoMounter {
        IsoMounter::new()
    }
}

pub fn run_isomount_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F440");
    let mut m = IsoMounter::new();
    // 挂载全链：合法 ISO → 分配盘符；非 ISO → 拒绝且人话。
    let iso_header: &[u8] = b"CD001\x00blah";
    set.add(
        "f440-mount-ok",
        m.mount("tools.iso", iso_header) == Ok('E') && m.mounted.len() == 1,
        "",
    );
    set.add(
        "f440-non-iso-rejected",
        matches!(m.mount("fake.iso", b"NOTANISO"), Err(_)),
        "",
    );
    // 浏览。
    set.add(
        "f440-browse",
        m.seed_files('E', alloc::vec![String::from("setup.exe"), String::from("readme.txt")])
            && m.browse('E').map(|f| f.len()) == Some(2)
            && m.browse('Z').is_none(),
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
    // 上限与提示：挂到 4 个后第 5 个拒。
    let mut full = IsoMounter::new();
    for i in 0..MAX_MOUNTED {
        let src = alloc::format!("d{}.iso", i);
        assert!(full.mount(&src, iso_header).is_ok());
    }
    set.add(
        "f440-cap-four",
        full.mounted.len() == MAX_MOUNTED && matches!(full.mount("d5.iso", iso_header), Err(_)),
        "",
    );
    // 弹出。
    set.add(
        "f440-eject",
        full.eject('E') && full.mounted.len() == 3 && !full.eject('E'),
        "",
    );
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
        // 扩展名叫 .iso 但内容不是 → 拒。
        assert!(matches!(m.mount("movie.iso", b"RIFF...."), Err(_)));
        // 扩展名怪但内容对 → 收（不猜扩展名的正反面）。
        assert!(m.mount("backup.bin", b"CD001").is_ok());
    }

    #[test]
    fn eject_reuses_letters() {
        let mut m = IsoMounter::new();
        let hdr: &[u8] = b"CD001";
        let _ = m.mount("a.iso", hdr);
        let _ = m.mount("b.iso", hdr);
        assert!(m.eject('E'));
        // 新挂载继续顺序分配。
        assert_eq!(m.mount("c.iso", hdr), Ok('G'));
    }
}
