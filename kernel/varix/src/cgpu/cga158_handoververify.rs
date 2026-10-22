//! cga158_handoververify.rs —— CGPU-F0158 · A 域移交完成核验
//!
//! F0124 移交的完成核验：B 域是否真凭移交包开工（B 域首批内容
//! 引用移交包文档/接口的验证）、移交包缺项的补充（B 域开工时
//! 发现缺什么——补进移交包供后续域复用）。核验即移交闭环。
//! 与 F0124（cga124_handover 移交包构建）配对：那边管「包的
//! 构建」，这边管「包被真实消费」的闭环。
//! 诊断码段 0x03D0（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x03D0;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 开工引用行（判据「开工验证」）：B 域首批内容对移交包的
/// 一次真实引用。
#[derive(Clone, Debug)]
pub struct UsageRow {
    /// B 域消费文件指纹。
    pub consumer_fp: u64,
    /// 所引移交包件号（0..PACKAGE_PARTS）。
    pub part: usize,
    /// 引用形态：0=文档引用 1=接口引用 2=范例引用。
    pub usage: u8,
}

impl UsageRow {
    /// 自检。
    pub fn validate(&self, parts: usize) -> Result<(), u16> {
        if self.consumer_fp == 0 {
            return Err(SEG_BASE + 1);
        }
        if self.part >= parts {
            return Err(SEG_BASE + 2);
        }
        if self.usage > 2 {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 缺项补充行（判据「缺项补充」）。
#[derive(Clone, Debug)]
pub struct GapRow {
    /// 缺项描述指纹。
    pub gap_fp: u64,
    /// 发现方：0=B域开工 1=核验过程。
    pub found_by: u8,
    /// 补充状态：false=待补 true=已补。
    pub filled: bool,
    /// 补充件指纹（已补时非零）。
    pub fill_fp: u64,
}

/// 移交核验台账。
pub struct HandoverVerify {
    /// 包件数（与 F0124 四件对齐，但核验册按通用件数管理）。
    parts: usize,
    /// 包指纹（F0124 冻结签章，None = 包未冻结即开工是违规）。
    package_fp: Option<u64>,
    /// 开工引用行。
    usages: Vec<UsageRow>,
    /// 缺项行。
    gaps: Vec<GapRow>,
    /// B 域确认（判据「B 域确认」）：B 域侧签章指纹。
    b_confirm: Option<u64>,
    /// 归档。
    archived: Option<u64>,
}

impl HandoverVerify {
    /// 建账：件数与包指纹必填（无包指纹 = 未冻结包，核验无从谈起）。
    pub fn new(parts: usize, package_fp: u64) -> Result<Self, u16> {
        if parts == 0 || parts > 16 {
            return Err(SEG_BASE + 4);
        }
        if package_fp == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(HandoverVerify {
            parts,
            package_fp: Some(package_fp),
            usages: Vec::new(),
            gaps: Vec::new(),
            b_confirm: None,
            archived: None,
        })
    }

    /// 件数。
    pub fn parts(&self) -> usize {
        self.parts
    }

    /// 包指纹。
    pub fn package_fp(&self) -> Option<u64> {
        self.package_fp
    }

    /// 登记开工引用（判据「开工验证」）：B 域文件真实引用包件。
    pub fn record_usage(&mut self, u: UsageRow) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 6);
        }
        u.validate(self.parts)?;
        self.usages.push(u);
        Ok(())
    }

    /// 每件都被引用验证（判据「B 域是否真凭移交包开工」）：
    /// 包内每件至少被引用一次——零引用件 = 死文档。
    pub fn all_parts_used(&self) -> Result<(), u16> {
        if self.usages.is_empty() {
            return Err(SEG_BASE + 7);
        }
        for p in 0..self.parts {
            if !self.usages.iter().any(|u| u.part == p) {
                return Err(SEG_BASE + 8);
            }
        }
        Ok(())
    }

    /// 引用形态覆盖（判据支撑）：三种引用形态至少各一次
    /// （只用文档不用接口 = 包开工不完整）。
    pub fn usage_kinds_covered(&self) -> Result<(), u16> {
        for k in 0u8..=2 {
            if !self.usages.iter().any(|u| u.usage == k) {
                return Err(SEG_BASE + 9);
            }
        }
        Ok(())
    }

    /// 报缺项（判据「缺项补充」）：发现方闭集、指纹非零。
    pub fn report_gap(&mut self, gap_fp: u64, found_by: u8) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 6);
        }
        if gap_fp == 0 {
            return Err(SEG_BASE + 10);
        }
        if found_by > 1 {
            return Err(SEG_BASE + 11);
        }
        self.gaps.push(GapRow {
            gap_fp,
            found_by,
            filled: false,
            fill_fp: 0,
        });
        Ok(())
    }

    /// 补缺项：补充件指纹必填。
    pub fn fill_gap(&mut self, gap_fp: u64, fill_fp: u64) -> Result<(), u16> {
        let g = self
            .gaps
            .iter_mut()
            .find(|g| g.gap_fp == gap_fp)
            .ok_or(SEG_BASE + 12)?;
        if fill_fp == 0 {
            return Err(SEG_BASE + 13);
        }
        g.filled = true;
        g.fill_fp = fill_fp;
        Ok(())
    }

    /// 缺项全补判（判据「缺项补充」闭环）。
    pub fn gaps_filled(&self) -> Result<(), u16> {
        if self.gaps.iter().any(|g| !g.filled) {
            return Err(SEG_BASE + 14);
        }
        Ok(())
    }

    /// B 域确认（判据「B 域确认」）：确认材料指纹化落账。
    pub fn confirm_by_b(&mut self, material: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 6);
        }
        if self.b_confirm.is_some() {
            return Err(SEG_BASE + 15);
        }
        if material.is_empty() {
            return Err(SEG_BASE + 16);
        }
        let fp = fnv1a64(material);
        self.b_confirm = Some(fp);
        Ok(fp)
    }

    /// B 域确认投影。
    pub fn b_confirm_fp(&self) -> Option<u64> {
        self.b_confirm
    }

    /// 归档前置总检（判据「闭环记录/归档」）：包冻结 + 每件被用 +
    /// 形态覆盖 + 缺项全补 + B 域确认，五关全过才许归档。
    pub fn pre_archive(&mut self, canon: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 17);
        }
        if self.package_fp.is_none() {
            return Err(SEG_BASE + 5);
        }
        self.all_parts_used()?;
        self.usage_kinds_covered()?;
        self.gaps_filled()?;
        if self.b_confirm.is_none() {
            return Err(SEG_BASE + 18);
        }
        let mut fp = self.package_fp.unwrap_or(0);
        for u in &self.usages {
            fp = fp.wrapping_mul(0x100000001b3) ^ u.consumer_fp;
        }
        for g in &self.gaps {
            fp = fp.wrapping_mul(0x100000001b3) ^ g.gap_fp;
        }
        for &b in canon {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        if fp == 0 {
            return Err(SEG_BASE + 19);
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }

    /// 闭环记录投影（判据「闭环记录」）：引用数/缺项数/确认态
    /// 三元组。
    pub fn closure_summary(&self) -> (usize, usize, bool) {
        (
            self.usages.len(),
            self.gaps.len(),
            self.b_confirm.is_some(),
        )
    }
}

/// 引用真实性抽查（独立函数面）：同消费者文件对同件多次引用
/// 形态必须一致（同文件一会儿文档引用一会儿接口引用 = 引用造假）。
pub fn usage_consistency(rows: &[UsageRow]) -> Result<(), u16> {
    for (i, a) in rows.iter().enumerate() {
        for (j, b) in rows.iter().enumerate() {
            if i < j && a.consumer_fp == b.consumer_fp && a.part == b.part
                && a.usage != b.usage
            {
                return Err(SEG_BASE + 20);
            }
        }
    }
    Ok(())
}

impl HandoverVerify {
    /// 引用密度（判据「开工验证」量化面）：每件平均被引次数
    /// （千分倍——乘 1000 保持整数域）。
    pub fn usage_density_permille(&self) -> Option<u32> {
        if self.parts == 0 || self.usages.is_empty() {
            return None;
        }
        Some(((self.usages.len() * 1000) / self.parts) as u32)
    }

    /// B 域开工缺项占比（判据「缺项补充」量化面）：B 域开工时
    /// 发现的缺项占全部缺项的比例（发现方=0）。
    pub fn b_found_permille(&self) -> Option<u32> {
        if self.gaps.is_empty() {
            return None;
        }
        let b = self.gaps.iter().filter(|g| g.found_by == 0).count();
        Some(((b * 1000) / self.gaps.len()) as u32)
    }

    /// 已补缺项指纹册（后续域复用判据「补进移交包供后续域
    /// 复用」的投影）：全部已补件指纹列表。
    pub fn filled_fingerprints(&self) -> Vec<u64> {
        self.gaps
            .iter()
            .filter(|g| g.filled && g.fill_fp != 0)
            .map(|g| g.fill_fp)
            .collect()
    }
}
