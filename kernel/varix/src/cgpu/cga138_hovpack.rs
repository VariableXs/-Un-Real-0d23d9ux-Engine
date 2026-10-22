//! cga138_hovpack — CGPU-F0138 A 域移交包（向 CGPU-B）。
//!
//! 判据原文（施工书锚点 CGPU-F0138）：A→B 移交包（任务图调度域开工
//! 包）：线程池冻结接口（B 域消费的执行底座）、任务图对接冻结书
//! （F0042 确认版）、确定性纪律文档（B 域调度必须遵守的确定性约束）、
//! 软渲参考（A 域算子作为 B 域调度的第一消费者范本）。B 域开工验证
//! （第一批次引用移交包立项）。判据：四件齐、验证、剩余计划、纪要、
//! 台账更新。
//!
//! **设计定盘**：与 F0124 组移交的深化差异：F0124 是「组间移交」，
//! 本件是「域级移交包」——四件各有版本语义（件版本三元），包 =
//! 四件 + 版本兼容矩阵（四件两两兼容核验：底座版本与对接书版本、
//! 纪律文档版本与算子清单版本必须落在兼容区间）。开工验证 = B 域
//! 首批次对包内件的逐件引用核验（四件全被引用才算开工凭包）。
//! 剩余计划 = 域级收尾项账；纪要 = 域级移交纪要；台账 = 包版本
//! 演进链（每次重打包留版本指纹链）。
//!
//! 诊断码段 0x0290~0x029F（0x02 段第 10 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA138_VERSION: &str = "cga138-hovpack-v1";

/// 诊断子区基值（0x02 段第 10 子区）。
pub const SEG_BASE: u16 = 0x0290;

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 包内四件闭集（判据"四件齐"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackPart {
    /// 线程池冻结接口。
    PoolIface,
    /// 任务图对接冻结书。
    GraphHandshake,
    /// 确定性纪律文档。
    DetDiscipline,
    /// 软渲参考。
    SoftRasterRef,
}

impl PackPart {
    /// 全集稳定序。
    pub const ALL: [PackPart; 4] = [
        PackPart::PoolIface,
        PackPart::GraphHandshake,
        PackPart::DetDiscipline,
        PackPart::SoftRasterRef,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            PackPart::PoolIface => 0,
            PackPart::GraphHandshake => 1,
            PackPart::DetDiscipline => 2,
            PackPart::SoftRasterRef => 3,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 件版本三元。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PartVer {
    /// 主版本。
    pub major: u32,
    /// 次版本。
    pub minor: u32,
    /// 修订。
    pub patch: u32,
}

impl PartVer {
    /// 稳定键。
    pub fn key(self) -> u64 {
        ((self.major as u64) << 40) | ((self.minor as u64) << 20) | self.patch as u64
    }

    /// 零版本即拒（未版本化件不能进包）。
    pub fn validate(self) -> Result<(), u16> {
        if self.major == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 域级移交包。
pub struct DomainHandover {
    /// 四件（指纹 + 版本）。
    parts: [Option<(u64, PartVer)>; 4],
    /// 兼容矩阵（判据"版本兼容"：件对 → 是否兼容的核验结论）。
    compat_checked: bool,
    /// 包指纹。
    package_fp: Option<u64>,
    /// 剩余计划（域级收尾项 1..=N）。
    plan: Vec<u32>,
    /// 纪要指纹。
    minutes_fp: Option<u64>,
    /// 打包代际（每次重打包 +1——台账演进链）。
    generation: u32,
}

impl DomainHandover {
    /// 空包。
    pub fn new() -> Self {
        DomainHandover {
            parts: [None; 4],
            compat_checked: false,
            package_fp: None,
            plan: Vec::new(),
            minutes_fp: None,
            generation: 0,
        }
    }

    /// 装件（判据"四件齐"：冻结后禁改）。
    pub fn put(&mut self, part: PackPart, body: &[u8], ver: PartVer) -> Result<(), u16> {
        if self.package_fp.is_some() {
            return Err(SEG_BASE + 6);
        }
        ver.validate()?;
        if body.is_empty() {
            return Err(SEG_BASE + 7);
        }
        let slot = &mut self.parts[part.ordinal() as usize];
        if slot.is_some() {
            return Err(SEG_BASE + 8);
        }
        *slot = Some((fnv1a64(body), ver));
        Ok(())
    }

    /// 兼容核验（判据"版本兼容"：底座（件 0）与对接书（件 1）主版本
    /// 必须一致；纪律文档（件 2）与软渲参考（件 3）主版本必须一致
    /// ——跨主版本引用是开工包的头号漂移源）。
    pub fn check_compat(&mut self) -> Result<(), u16> {
        if self.package_fp.is_some() {
            return Err(SEG_BASE + 6);
        }
        let v = |p: PackPart| -> Result<PartVer, u16> {
            self.parts[p.ordinal() as usize]
                .map(|(_, v)| v)
                .ok_or(p.code())
        };
        let a = v(PackPart::PoolIface)?;
        let b = v(PackPart::GraphHandshake)?;
        if a.major != b.major {
            return Err(SEG_BASE + 9);
        }
        let c = v(PackPart::DetDiscipline)?;
        let d = v(PackPart::SoftRasterRef)?;
        if c.major != d.major {
            return Err(SEG_BASE + 10);
        }
        self.compat_checked = true;
        Ok(())
    }

    /// 登记剩余计划项（项号 ≥ 1；重复即拒）。
    pub fn add_plan(&mut self, item: u32) -> Result<(), u16> {
        if self.package_fp.is_some() {
            return Err(SEG_BASE + 6);
        }
        if item == 0 || self.plan.contains(&item) {
            return Err(SEG_BASE + 11);
        }
        self.plan.push(item);
        Ok(())
    }

    /// 冻结打包（判据"四件齐 + 兼容核验"；打包代际 +1）。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.package_fp.is_some() {
            return Err(SEG_BASE + 12);
        }
        for p in PackPart::ALL {
            if self.parts[p.ordinal() as usize].is_none() {
                return Err(p.code());
            }
        }
        if !self.compat_checked {
            return Err(SEG_BASE + 13);
        }
        let mut fp = FNV_OFFSET;
        for p in &self.parts {
            let (f, v) = p.unwrap_or((0, PartVer { major: 0, minor: 0, patch: 0 }));
            fp = fp.wrapping_mul(FNV_PRIME) ^ f;
            fp = fp.wrapping_mul(FNV_PRIME) ^ v.key();
        }
        self.generation += 1;
        self.package_fp = Some(fp);
        Ok(fp)
    }

    /// 纪要归档（判据"纪要"）。
    pub fn archive_minutes(&mut self, minutes: &[u8]) -> Result<u64, u16> {
        if self.package_fp.is_none() {
            return Err(SEG_BASE + 14);
        }
        if self.minutes_fp.is_some() || minutes.is_empty() {
            return Err(SEG_BASE + 15);
        }
        let fp = fnv1a64(minutes);
        self.minutes_fp = Some(fp);
        Ok(fp)
    }

    /// 台账投影（判据"台账更新"：代际 + 包指纹 + 计划数）。
    pub fn ledger_row(&self) -> Option<(u32, u64, usize)> {
        Some((self.generation, self.package_fp?, self.plan.len()))
    }

    /// 包指纹。
    pub fn fingerprint(&self) -> Option<u64> {
        self.package_fp
    }
}

/// B 域开工验证账（判据"B 域开工验证"——四件逐件引用核验）。
pub struct KickoffCheck {
    /// 包指纹（对照真源）。
    package_fp: u64,
    /// 已核验件（件号序）。
    cited: [bool; 4],
    /// 开工判定。
    started: bool,
}

impl KickoffCheck {
    /// 以冻结包指纹开账。
    pub fn new(package_fp: u64) -> Self {
        KickoffCheck {
            package_fp,
            cited: [false; 4],
            started: false,
        }
    }

    /// 逐件引用核验（判据"第一批次引用移交包立项"：引用指纹必须
    /// 等于包指纹；同件重复核验即拒）。
    pub fn cite_part(&mut self, part: PackPart, cited_fp: u64) -> Result<(), u16> {
        if self.started {
            return Err(SEG_BASE + 16);
        }
        if cited_fp != self.package_fp {
            return Err(SEG_BASE + 17);
        }
        let slot = &mut self.cited[part.ordinal() as usize];
        if *slot {
            return Err(SEG_BASE + 18);
        }
        *slot = true;
        Ok(())
    }

    /// 开工判定（判据"四件全被引用才算开工凭包"）。
    pub fn start(&mut self) -> Result<(), u16> {
        if self.started {
            return Err(SEG_BASE + 19);
        }
        if !self.cited.iter().all(|c| *c) {
            return Err(SEG_BASE + 20);
        }
        self.started = true;
        Ok(())
    }

    /// 是否开工。
    pub fn is_started(&self) -> bool {
        self.started
    }
}

impl DomainHandover {
    /// 件版本投影（判据"版本兼容"的账面：四件各自 (指纹, 版本键)）。
    pub fn part_versions(&self) -> [Option<(u64, u64)>; 4] {
        let mut out = [None; 4];
        for (i, slot) in self.parts.iter().enumerate() {
            out[i] = slot.map(|(f, v)| (f, v.key()));
        }
        out
    }

    /// 缺件清单（判据"四件齐"的定位面）。
    pub fn missing_parts(&self) -> Vec<PackPart> {
        PackPart::ALL
            .iter()
            .copied()
            .filter(|p| self.parts[p.ordinal() as usize].is_none())
            .collect()
    }

    /// 计划进度（已登记计划项数）。
    pub fn plan_count(&self) -> usize {
        self.plan.len()
    }

    /// 打包代际。
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// 纪要在档。
    pub fn minutes_on_file(&self) -> bool {
        self.minutes_fp.is_some()
    }
}

impl Default for DomainHandover {
    fn default() -> Self {
        Self::new()
    }
}
