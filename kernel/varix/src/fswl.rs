//! fswl — WP-203 · B-701 ext4 特性白名单（判据实装层，MD2 篇 7.2 宪法）。
//!
//! 判据 B-701：未知特性挂载失败并人话提示。
//! MD2 宪法原文："ext4 的特性矩阵庞大，我们只开白名单内的特性：extents、日志、
//! 哈希树目录，其余特性遇到即挂载失败并人话提示，**绝不对未知特性硬解析**。"
//!
//! 语义分层（比 ext4 标准语义更严——数据红线立场，宪章第十二章落点）：
//! - ext4 标准语义：incompat 未知必须拒绝、ro_compat 未知可只读挂载；
//! - VARIX 宪法语义：三组旗标白名单外一律拒绝挂载（ro_compat 未知也拒），
//!   只读降级路径（MD1 行 1640：重放失败的盘转只读）由 B-703 断电恢复链承接，
//!   本模块只管"认不认"，不把"认不得"的盘放进任何写路径。
//!
//! 白名单三档：
//! - 基础集（missing 即拒绝）：解析器存在性假设，缺失说明卷不是可解析 ext4；
//! - 放行集（present 即允许）：解析器明确支持的特性（v1 初版随 ext4_rs 1.3.3
//!   接线实测校准，校准记录回写 MD2 篇 7 判据实装回写段）；
//! - 拒绝集（present 即拒绝 + 人话）：白名单外一切——inline_data、加密、
//!   casefold、verity、quota、bigalloc、mmp 等已知高危与一切未知位。
//!
//! 人话提示三要素（宪章第九章）：发生了什么 / 为什么 / 下一步。

use crate::checks::CheckSet;

// --- 超级块旗标偏移（ext4 布局，e2fsprogs 标准）---
pub const SB_COMPAT_OFF: usize = 0x2C;
pub const SB_INCOMPAT_OFF: usize = 0x30;
pub const SB_RO_COMPAT_OFF: usize = 0x34;
pub const SB_MIN_LEN: usize = SB_RO_COMPAT_OFF + 4;

// --- compat 组（0x2C）---
pub const C_HAS_JOURNAL: u32 = 0x4; // 日志（MD2 白名单明文）
pub const C_DIR_INDEX: u32 = 0x20; // 哈希树目录（MD2 白名单明文）

// --- incompat 组（0x30）---
pub const I_FILETYPE: u32 = 0x2; // 目录项文件类型——基础集
pub const I_RECOVER: u32 = 0x4; // 日志有未重放事务——挂载期重放（MD2 篇 7.2 阶段一）
pub const I_EXTENTS: u32 = 0x40; // extents（MD2 白名单明文）
pub const I_64BIT: u32 = 0x80; // 64 位卷
pub const I_FLEX_BG: u32 = 0x200; // flex 块组
pub const I_LARGEDIR: u32 = 0x4000; // 大目录

// --- ro_compat 组（0x34）---
pub const R_SPARSE_SUPER: u32 = 0x1; // 稀疏超级块
pub const R_LARGE_FILE: u32 = 0x2; // 大文件
pub const R_HUGE_FILE: u32 = 0x8; // 巨文件
pub const R_GDT_CSUM: u32 = 0x10; // 组描述符校验和
pub const R_DIR_NLINK: u32 = 0x20; // 目录子目录计数
pub const R_EXTRA_ISIZE: u32 = 0x40; // 扩展 inode 尺寸
pub const R_METADATA_CSUM: u32 = 0x400; // 元数据校验和

// --- 白名单（v1 初版：MD2 明文三项 + 解析器存在性基础集）---
/// 基础集：缺失即拒绝——卷形态超出解析器假设。
pub const BASE_COMPAT: u32 = 0;
pub const BASE_INCOMPAT: u32 = I_FILETYPE;
pub const BASE_RO_COMPAT: u32 = R_SPARSE_SUPER | R_LARGE_FILE;
/// 放行集：present 即允许。
pub const OK_COMPAT: u32 = C_HAS_JOURNAL | C_DIR_INDEX;
pub const OK_INCOMPAT: u32 = I_FILETYPE | I_RECOVER | I_EXTENTS | I_64BIT | I_FLEX_BG | I_LARGEDIR;
pub const OK_RO_COMPAT: u32 = R_SPARSE_SUPER | R_LARGE_FILE | R_HUGE_FILE | R_GDT_CSUM
    | R_DIR_NLINK | R_EXTRA_ISIZE | R_METADATA_CSUM;

/// 白名单外一律拒绝（VARIX 宪法语义）：
/// 拒绝位 = 三组旗标 ∧ ¬放行集。
pub const REJECT_MASK_COMPAT: u32 = !OK_COMPAT;
pub const REJECT_MASK_INCOMPAT: u32 = !OK_INCOMPAT;
pub const REJECT_MASK_RO_COMPAT: u32 = !OK_RO_COMPAT;

/// 人话提示三要素（宪章第九章），挂载失败统一文案。
pub const MOUNT_FAIL_WHAT: &str = "挂载失败：卷启用了白名单外的 ext4 特性";
pub const MOUNT_FAIL_WHY: &str = "VARIX 只开已知安全的特性集，未知特性硬解析会腐坏数据";
pub const MOUNT_FAIL_NEXT: &str = "在源系统卸载该特性或更换卷";

/// 特性位名表（人话可 grep；未知名报位号）。
pub fn flag_name(group: u8, bit: u32) -> &'static str {
    match (group, bit) {
        (0, C_HAS_JOURNAL) => "has_journal",
        (0, C_DIR_INDEX) => "dir_index",
        (0, 0x1) => "dir_prealloc",
        (0, 0x2) => "imagic_inodes",
        (0, 0x8) => "ext_attr",
        (0, 0x10) => "resize_inode",
        (0, 0x40) => "dir_nlink_compat",
        (1, I_FILETYPE) => "filetype",
        (1, I_RECOVER) => "recover",
        (1, I_EXTENTS) => "extents",
        (1, I_64BIT) => "64bit",
        (1, I_FLEX_BG) => "flex_bg",
        (1, I_LARGEDIR) => "largedir",
        (1, 0x1) => "compression",
        (1, 0x8) => "journal_dev",
        (1, 0x10) => "meta_bg",
        (1, 0x100) => "mmp",
        (1, 0x400) => "ea_inode",
        (1, 0x1000) => "dirdata",
        (1, 0x2000) => "csum_seed",
        (1, 0x8000) => "inline_data",
        (1, 0x10000) => "orphan_present",
        (1, 0x20000) => "verity",
        (1, 0x40000) => "casefold",
        (2, R_SPARSE_SUPER) => "sparse_super",
        (2, R_LARGE_FILE) => "large_file",
        (2, R_HUGE_FILE) => "huge_file",
        (2, R_GDT_CSUM) => "gdt_csum",
        (2, R_DIR_NLINK) => "dir_nlink",
        (2, R_EXTRA_ISIZE) => "extra_isize",
        (2, R_METADATA_CSUM) => "metadata_csum",
        (2, 0x4) => "btree_dir",
        (2, 0x80) => "has_snapshot",
        (2, 0x100) => "quota",
        (2, 0x200) => "bigalloc",
        (2, 0x1000) => "readonly",
        (2, 0x2000) => "project",
        _ => "",
    }
}

/// 拒绝裁决：一位一个结构化拒绝（挂载失败报告逐位可列）。
#[derive(Clone, Copy)]
pub struct Reject {
    /// 组：0=compat 1=incompat 2=ro_compat
    pub group: u8,
    pub bit: u32,
    /// true = 基础集缺失（卷形态超出假设）；false = 白名单外拒绝位。
    pub missing_base: bool,
}

impl Reject {
    /// 人话一行（三要素压缩版；完整三要素常量见 MOUNT_FAIL_*）。
    pub fn human(&self) -> &'static str {
        if self.missing_base {
            "卷缺少解析器必需的基础特性，形态超出假设"
        } else {
            MOUNT_FAIL_WHAT
        }
    }
}

/// 白名单闸：三组旗标逐位过闸。
/// 返回 Ok(()) = 全部落入基础∪放行集；Err(拒绝位列表) = 挂载失败。
/// 空表不可达（至少一位违规才会 Err），上限 96 = 3 组 × 32 位穷举上界。
pub fn admit(compat: u32, incompat: u32, ro_compat: u32) -> Result<(), [Option<Reject>; 96]> {
    let mut out = [None; 96];
    let mut n = 0usize;
    // 基础集缺失检查（missing_base）——空必需集（BASE_COMPAT=0）短路跳过：
    // ext4 compat 旗标对只读挂载无强制必需位是域语义，恒零掩码不构成检查。
    if BASE_COMPAT != 0 && compat & BASE_COMPAT != BASE_COMPAT {
        out[n] = Some(Reject { group: 0, bit: BASE_COMPAT & !compat, missing_base: true });
        n += 1;
    }
    if incompat & BASE_INCOMPAT != BASE_INCOMPAT {
        out[n] = Some(Reject { group: 1, bit: BASE_INCOMPAT & !incompat, missing_base: true });
        n += 1;
    }
    if ro_compat & BASE_RO_COMPAT != BASE_RO_COMPAT {
        out[n] = Some(Reject { group: 2, bit: BASE_RO_COMPAT & !ro_compat, missing_base: true });
        n += 1;
    }
    // 白名单外拒绝位逐位列举（VARIX 宪法语义：ro_compat 未知也拒）
    for bit in 0u32..32 {
        let m = 1u32 << bit;
        if compat & m != 0 && m & OK_COMPAT == 0 {
            out[n] = Some(Reject { group: 0, bit: m, missing_base: false });
            n += 1;
        }
        if incompat & m != 0 && m & OK_INCOMPAT == 0 {
            out[n] = Some(Reject { group: 1, bit: m, missing_base: false });
            n += 1;
        }
        if ro_compat & m != 0 && m & OK_RO_COMPAT == 0 {
            out[n] = Some(Reject { group: 2, bit: m, missing_base: false });
            n += 1;
        }
    }
    if n == 0 {
        Ok(())
    } else {
        Err(out)
    }
}

/// 从超级块字节切片解析三组旗标并过闸（小端 u32 × 3）。
pub fn admit_sb(sb: &[u8]) -> Result<(), [Option<Reject>; 96]> {
    if sb.len() < SB_MIN_LEN {
        let mut out = [None; 96];
        out[0] = Some(Reject { group: 3, bit: 0, missing_base: true });
        return Err(out);
    }
    let rd = |off: usize| -> u32 {
        u32::from_le_bytes([sb[off], sb[off + 1], sb[off + 2], sb[off + 3]])
    };
    admit(rd(SB_COMPAT_OFF), rd(SB_INCOMPAT_OFF), rd(SB_RO_COMPAT_OFF))
}

/// 计数辅助：拒绝位个数（对练/报表用）。
pub fn reject_count(r: &[Option<Reject>; 96]) -> usize {
    r.iter().flatten().count()
}

// ---------------------------------------------------------------- 自检

pub fn run_fswl_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-701 ext4 特性白名单");
    {
        // 纯白名单卷（MD2 明文三项 + 基础集）通过
        let compat = OK_COMPAT;
        let incompat = OK_INCOMPAT;
        let ro = OK_RO_COMPAT;
        set.add("B-701 白名单内卷全通过", admit(compat, incompat, ro).is_ok(), "三组旗标全在放行集");
    }
    {
        // MD2 明文三项最小卷
        let compat = C_HAS_JOURNAL | C_DIR_INDEX;
        let incompat = I_FILETYPE | I_EXTENTS;
        let ro = BASE_RO_COMPAT;
        set.add("B-701 MD2 明文三项最小卷通过", admit(compat, incompat, ro).is_ok(), "extents+日志+哈希树+基础集");
    }
    {
        // 已知高危特性拒绝：inline_data
        let r = admit(0, I_FILETYPE | 0x8000, BASE_RO_COMPAT);
        let n = r.as_ref().err().map(reject_count).unwrap_or(0);
        set.add("B-701 inline_data 拒绝", r.is_err() && n == 1, "白名单外 incompat 位即拒");
    }
    {
        // ro_compat 未知位也拒（VARIX 宪法语义，严于 ext4 标准）
        let r = admit(0, I_FILETYPE, BASE_RO_COMPAT | 0x200); // bigalloc
        set.add("B-701 bigalloc(ro_compat) 拒绝", r.is_err(), "未知 ro_compat 不只读放行");
    }
    {
        // 基础集缺失拒绝：无 filetype
        let r = admit(0, 0, BASE_RO_COMPAT);
        let missing = r.as_ref().err().and_then(|t| t.iter().flatten().find(|x| x.missing_base).map(|_| ()));
        set.add("B-701 基础集缺失拒绝", r.is_err() && missing.is_some(), "缺 filetype 即卷形态超假设");
    }
    {
        // 多违规位逐位列举
        let r = admit(0x8, I_FILETYPE | 0x8000 | 0x100, BASE_RO_COMPAT | 0x200);
        let n = r.as_ref().err().map(reject_count).unwrap_or(0);
        set.add("B-701 多违规位逐位列举", n == 4, "ext_attr+inline_data+mmp+bigalloc=4 位");
    }
    {
        // 超级块字节切片解析：小端三组旗标
        let mut sb = [0u8; SB_MIN_LEN];
        sb[SB_COMPAT_OFF..SB_COMPAT_OFF + 4].copy_from_slice(&OK_COMPAT.to_le_bytes());
        sb[SB_INCOMPAT_OFF..SB_INCOMPAT_OFF + 4].copy_from_slice(&OK_INCOMPAT.to_le_bytes());
        sb[SB_RO_COMPAT_OFF..SB_RO_COMPAT_OFF + 4].copy_from_slice(&OK_RO_COMPAT.to_le_bytes());
        set.add("B-701 超级块解析过闸", admit_sb(&sb).is_ok(), "0x2C/0x30/0x34 小端三组读出全放行");
    }
    {
        // 短切片拒绝（卷形态不足）
        let sb = [0u8; SB_MIN_LEN - 1];
        let r = admit_sb(&sb);
        set.add("B-701 短超级块拒绝", r.is_err(), "旗标区不完整即拒不硬读");
    }
    {
        // 特性名表：白名单核心位有名可报
        set.add(
            "B-701 特性名表核心位",
            !flag_name(1, I_EXTENTS).is_empty() && !flag_name(0, C_HAS_JOURNAL).is_empty(),
            "extents/has_journal 人话可 grep",
        );
    }
    {
        // 人话三要素常量齐
        set.add(
            "B-701 人话三要素",
            !MOUNT_FAIL_WHAT.is_empty() && !MOUNT_FAIL_WHY.is_empty() && !MOUNT_FAIL_NEXT.is_empty(),
            "发生了什么/为什么/下一步",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f501_admit_semantics() {
        // 全放行
        assert!(admit(OK_COMPAT, OK_INCOMPAT, OK_RO_COMPAT).is_ok());
        // MD2 明文三项
        assert!(admit(C_HAS_JOURNAL | C_DIR_INDEX, I_FILETYPE | I_EXTENTS, BASE_RO_COMPAT).is_ok());
        // inline_data 拒
        assert!(admit(0, I_FILETYPE | 0x8000, BASE_RO_COMPAT).is_err());
        // bigalloc（ro_compat 白名单外）拒——VARIX 语义严于 ext4 标准
        assert!(admit(0, I_FILETYPE, BASE_RO_COMPAT | 0x200).is_err());
        // 基础集缺失拒
        assert!(admit(0, 0, BASE_RO_COMPAT).is_err());
    }

    #[test]
    fn f501_reject_listing() {
        let r = admit(0x8, I_FILETYPE | 0x8000 | 0x100, BASE_RO_COMPAT | 0x200);
        let table = r.err().expect("必拒");
        assert_eq!(reject_count(&table), 4);
        // 拒绝位带组号与位号，人话非空
        let first = table.iter().flatten().next().expect("至少一位");
        assert!(first.group <= 2 && first.bit != 0 || first.missing_base);
        assert!(!first.human().is_empty());
    }

    #[test]
    fn f501_sb_parse() {
        let mut sb = [0u8; SB_MIN_LEN];
        sb[SB_COMPAT_OFF..SB_COMPAT_OFF + 4].copy_from_slice(&OK_COMPAT.to_le_bytes());
        sb[SB_INCOMPAT_OFF..SB_INCOMPAT_OFF + 4].copy_from_slice(&OK_INCOMPAT.to_le_bytes());
        sb[SB_RO_COMPAT_OFF..SB_RO_COMPAT_OFF + 4].copy_from_slice(&OK_RO_COMPAT.to_le_bytes());
        assert!(admit_sb(&sb).is_ok());
        // 污染一位 → 拒
        sb[SB_INCOMPAT_OFF] |= 0x01; // compression
        assert!(admit_sb(&sb).is_err());
        // 短切片
        assert!(admit_sb(&[0u8; 16]).is_err());
    }

    #[test]
    fn f501_flag_names() {
        assert_eq!(flag_name(1, I_EXTENTS), "extents");
        assert_eq!(flag_name(0, C_HAS_JOURNAL), "has_journal");
        assert_eq!(flag_name(0, C_DIR_INDEX), "dir_index");
        assert_eq!(flag_name(1, 0x8000), "inline_data");
        assert_eq!(flag_name(2, R_METADATA_CSUM), "metadata_csum");
        assert_eq!(flag_name(9, 0x1234), ""); // 未知组静默
    }

    #[test]
    fn f501_self_checks_pass() {
        let set = run_fswl_checks();
        assert!(set.all_passed(), "B-701 自检全绿");
    }
}
