//! 任务34 · 剪贴板/拖放白名单化（双域总案阶段4·步骤6）。
//!
//! 职责分工（复用不重造）：
//! - **路径裁决**（FileRef/拖放）复用 [`vfsguard`]（任务30 的唯一入口）；
//! - **审计**复用 [`vfsguard::AuditJournal`]（journal 断电不丢）；
//! - 本模块新增的只有两件事：**剪贴板格式白名单 + 真实性校验**，以及
//!   **pid 级剪贴板授权位图**（VFS 规则是路径维度的，剪贴板是对象维度
//!   的——两维度正交，如实声明；任务36 设置页 UI 接管授权增删）。
//!
//! 验收对齐（总案步骤6）：
//! - 未授权进程读剪贴板得空 + 审计（不报错、不泄漏存在性）；
//! - 格式白名单三类起步（文本/图片引用/文件引用），白名单外拒绝；
//! - 伪装扩展名/魔数不符拒绝（声明与内容不一致 = Spoofed）；
//! - 拖放等价为文件读写走同一裁决（`dragdrop` → `adjudicate`）。
//! - 与 Windows 侧剪贴板行为差异公示见
//!   `docs/双域-任务34-剪贴板语义差异清单-2026-09-17.md`。

use alloc::vec::Vec;
use crate::drivers::blk::BlockDevice;
use crate::vfsguard::{self, AuditJournal, Decision, Op};

/// 格式白名单（三类起步；开放性=新增格式只扩本枚举与嗅探规则）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipFormat {
    /// UTF-8 文本。
    Text,
    /// 图片引用（png/jpeg/gif/bmp 魔数校验）。
    ImageRef,
    /// 共享分区文件引用（路径须过 VFS 白名单）。
    FileRef,
}

impl ClipFormat {
    /// 线上格式编号（稳定值，设置页展示共用）。
    pub fn code(self) -> u8 {
        match self {
            ClipFormat::Text => 1,
            ClipFormat::ImageRef => 2,
            ClipFormat::FileRef => 3,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        match c {
            1 => Some(ClipFormat::Text),
            2 => Some(ClipFormat::ImageRef),
            3 => Some(ClipFormat::FileRef),
            _ => None,
        }
    }
    fn slot(self) -> usize {
        match self {
            ClipFormat::Text => 0,
            ClipFormat::ImageRef => 1,
            ClipFormat::FileRef => 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipError {
    /// 白名单外格式编号（线上未知格式，安全路径：不猜）。
    UnknownFormat,
    /// 声明与内容不符（伪装扩展名/魔数）。
    Spoofed,
    /// FileRef 路径未过 VFS 白名单。
    PathDenied,
    /// pid 未获剪贴板授权（写方向）。
    NotAllowedWrite,
}

/// 一条剪贴板内容。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClipItem {
    pub format: ClipFormat,
    /// 文本内容 / 图片头部 / 规范化路径。
    pub data: Vec<u8>,
    pub from_pid: u32,
}

/// pid 级剪贴板授权位图（对象维度授权；bit i = pid i）。
#[derive(Clone, Copy)]
pub struct ClipAcl {
    grants: u64,
}

impl ClipAcl {
    pub const fn new() -> Self {
        ClipAcl { grants: 0 }
    }
    pub fn grant(&mut self, pid: u32) {
        if pid < 64 {
            self.grants |= 1u64 << pid;
        }
    }
    pub fn revoke(&mut self, pid: u32) {
        if pid < 64 {
            self.grants &= !(1u64 << pid);
        }
    }
    pub fn allowed(&self, pid: u32) -> bool {
        pid < 64 && (self.grants >> pid) & 1 == 1
    }
}

/// 剪贴板每格式单槽（新写入覆盖）。
pub struct ClipStore {
    slots: [Option<ClipItem>; 3],
    /// 覆盖次数（前主回收对照）。
    pub overwrites: u64,
}

impl ClipStore {
    pub const fn new() -> Self {
        ClipStore { slots: [None, None, None], overwrites: 0 }
    }

    /// 写入：授权 → 格式真实性 → FileRef 过 VFS 裁决 → 落槽。
    /// Err 时零副作用（不产生半条目）。
    pub fn set<B: BlockDevice>(
        &mut self,
        acl: &ClipAcl,
        rules: &vfsguard::RuleSet,
        audit: &mut AuditJournal<B>,
        pid: u32,
        format: ClipFormat,
        data: &[u8],
    ) -> Result<(), ClipError> {
        if !acl.allowed(pid) {
            audit.record(pid, false, true, b"clipboard:write");
            return Err(ClipError::NotAllowedWrite);
        }
        match format {
            ClipFormat::Text => {
                if core::str::from_utf8(data).is_err() {
                    audit.record(pid, false, true, b"clipboard:text-spoofed");
                    return Err(ClipError::Spoofed);
                }
            }
            ClipFormat::ImageRef => {
                if !looks_like_image(data) {
                    audit.record(pid, false, true, b"clipboard:image-spoofed");
                    return Err(ClipError::Spoofed);
                }
            }
            ClipFormat::FileRef => {
                if !rules.adjudicate(data, Op::Read).allow {
                    audit.record(pid, false, true, data);
                    return Err(ClipError::PathDenied);
                }
            }
        }
        let i = format.slot();
        if self.slots[i].is_some() {
            self.overwrites += 1;
        }
        self.slots[i] = Some(ClipItem { format, data: data.to_vec(), from_pid: pid });
        audit.record(pid, true, true, b"clipboard:ok");
        Ok(())
    }

    /// 读取：未授权 → `Ok(None)` + 审计（得空不报错）。
    /// FileRef 读取端复核路径（写端已滤，此处兜底——规则可能已收紧）。
    pub fn get<B: BlockDevice>(
        &mut self,
        acl: &ClipAcl,
        rules: &vfsguard::RuleSet,
        audit: &mut AuditJournal<B>,
        pid: u32,
        format: ClipFormat,
    ) -> Result<Option<ClipItem>, ClipError> {
        if !acl.allowed(pid) {
            audit.record(pid, false, false, b"clipboard:read");
            return Ok(None);
        }
        let i = format.slot();
        let Some(item) = self.slots[i].clone() else {
            return Ok(None);
        };
        if item.format == ClipFormat::FileRef && !rules.adjudicate(&item.data, Op::Read).allow {
            audit.record(pid, false, false, &item.data);
            return Err(ClipError::PathDenied);
        }
        audit.record(pid, true, false, b"clipboard:ok");
        Ok(Some(item))
    }

    /// 拖放 = 文件读写走同一裁决（返回值即裁决结论，调用方执行搬运）。
    pub fn dragdrop<B: BlockDevice>(
        rules: &vfsguard::RuleSet,
        audit: &mut AuditJournal<B>,
        pid: u32,
        path: &[u8],
        write: bool,
    ) -> Decision {
        let op = if write { Op::Write } else { Op::Read };
        let d = rules.adjudicate(path, op);
        audit.record(pid, d.allow, write, path);
        d
    }

    /// 线上格式编号 → 白名单格式（未知拒绝）。
    pub fn format_from_wire(code: u8) -> Result<ClipFormat, ClipError> {
        ClipFormat::from_code(code).ok_or(ClipError::UnknownFormat)
    }
}

/// 图片魔数嗅探（png/jpeg/gif/bmp 四类起步；不认识 = 拒绝，诚实口径）。
fn looks_like_image(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }
    let png = &data[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let jpeg = data[0] == 0xFF && data[1] == 0xD8;
    let gif = &data[..3] == b"GIF";
    let bmp = data[0] == b'B' && data[1] == b'M';
    png || jpeg || gif || bmp
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfsguard::{AuditJournal, DenyReason, RuleSet};

    struct FakeDisk {
        data: Vec<u8>,
    }
    impl FakeDisk {
        fn new(blocks: u64) -> Self {
            FakeDisk { data: alloc::vec![0u8; blocks as usize * 512] }
        }
    }
    impl BlockDevice for FakeDisk {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            (self.data.len() / 512) as u64
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), crate::drivers::blk::BlockError> {
            let off = lba as usize * 512;
            dst.copy_from_slice(&self.data[off..off + dst.len()]);
            Ok(())
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), crate::drivers::blk::BlockError> {
            let off = lba as usize * 512;
            self.data[off..off + src.len()].copy_from_slice(src);
            Ok(())
        }
        fn flush(&mut self) -> Result<(), crate::drivers::blk::BlockError> {
            Ok(())
        }
    }

    fn fixture() -> (RuleSet, AuditJournal<FakeDisk>, ClipStore, ClipAcl) {
        let mut rules = RuleSet::new();
        assert!(rules.add(true, true, b"/docs/*").is_ok());
        assert!(rules.add(true, true, b"/dropzone/*").is_ok());
        let mut disk = FakeDisk::new(2048);
        AuditJournal::<FakeDisk>::format(&mut disk, 100).expect("fmt");
        let (journal, _) = AuditJournal::open(disk, 100).expect("open");
        (rules, journal, ClipStore::new(), ClipAcl::new())
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    #[test]
    fn acl_grant_revoke_dimensions() {
        let mut acl = ClipAcl::new();
        assert!(!acl.allowed(5));
        acl.grant(5);
        assert!(acl.allowed(5));
        assert!(!acl.allowed(64), "pid 空间 64 之外恒拒");
        acl.revoke(5);
        assert!(!acl.allowed(5));
    }

    #[test]
    fn set_get_roundtrip_all_formats() {
        let (rules, mut audit, mut clip, mut acl) = fixture();
        acl.grant(5);
        clip.set(&acl, &rules, &mut audit, 5, ClipFormat::Text, b"hello").expect("text");
        clip.set(&acl, &rules, &mut audit, 5, ClipFormat::ImageRef, PNG).expect("image");
        clip.set(&acl, &rules, &mut audit, 5, ClipFormat::FileRef, b"/docs/report.txt").expect("fileref");
        assert_eq!(clip.get(&acl, &rules, &mut audit, 5, ClipFormat::Text).expect("get").unwrap().data, b"hello");
        assert_eq!(
            clip.get(&acl, &rules, &mut audit, 5, ClipFormat::FileRef).unwrap().unwrap().data,
            b"/docs/report.txt"
        );
    }

    #[test]
    fn unauthorized_read_gets_empty_with_audit() {
        // 总案验收原句：未授权进程读剪贴板得空 + 审计。
        let (rules, mut audit, mut clip, acl) = fixture(); // acl 全空 = 无人授权
        let got = clip.get(&acl, &rules, &mut audit, 9, ClipFormat::Text).expect("Ok(None) 不报错");
        assert!(got.is_none(), "得空");
        // 未授权写同样被拒。
        assert_eq!(
            clip.set(&acl, &rules, &mut audit, 9, ClipFormat::Text, b"secret"),
            Err(ClipError::NotAllowedWrite)
        );
        // 审计留痕（deny 记录在 journal 中可读回）。
        let rec = audit.read_record(0).expect("审计记录存在");
        assert!(!rec.allow);
    }

    #[test]
    fn spoofed_and_unknown_formats_rejected_zero_side_effect() {
        let (rules, mut audit, mut clip, mut acl) = fixture();
        acl.grant(5);
        // 声明 ImageRef 但无图片魔数。
        assert_eq!(
            clip.set(&acl, &rules, &mut audit, 5, ClipFormat::ImageRef, b"MZfake-not-image"),
            Err(ClipError::Spoofed)
        );
        // 声明 Text 但非 UTF-8。
        assert_eq!(clip.set(&acl, &rules, &mut audit, 5, ClipFormat::Text, &[0xFF, 0xFE, 0x00]), Err(ClipError::Spoofed));
        // FileRef 指向白名单外路径。
        assert_eq!(
            clip.set(&acl, &rules, &mut audit, 5, ClipFormat::FileRef, b"/etc/shadow"),
            Err(ClipError::PathDenied)
        );
        // 线上未知格式码。
        assert_eq!(ClipStore::format_from_wire(99), Err(ClipError::UnknownFormat));
        // 拒绝后零副作用：三槽仍空。
        for f in [ClipFormat::Text, ClipFormat::ImageRef, ClipFormat::FileRef] {
            assert!(clip.get(&acl, &rules, &mut audit, 5, f).unwrap().is_none());
        }
    }

    #[test]
    fn dragdrop_uses_same_adjudication() {
        let (rules, mut audit, _clip, _acl) = fixture();
        let d1 = ClipStore::dragdrop(&rules, &mut audit, 5, b"/dropzone/x.txt", true);
        assert!(d1.allow);
        let d2 = ClipStore::dragdrop(&rules, &mut audit, 5, b"/etc/passwd", true);
        assert!(!d2.allow);
        // 逃逸路径经拖放同样被路径层拦截（Escape/规范化失败）。
        let d3 = ClipStore::dragdrop(&rules, &mut audit, 5, b"../etc/passwd", true);
        assert!(!d3.allow);
        assert!(matches!(d3.deny, Some(DenyReason::BadPath(_))));
    }
}
