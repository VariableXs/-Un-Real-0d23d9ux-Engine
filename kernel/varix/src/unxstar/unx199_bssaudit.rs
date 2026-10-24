//! UNX-F0199 · 早年数据段清零与初始化审计
//!
//! 锚点判据（施工书原文）：UNX-F0199-J1 .bss/.data 段初始化完整性
//! 扫描，未初始化访问检出（工具链 + 断言）。
//!
//! 设计：段清单（.bss 零化义务 / .data 预置值义务）→ 初始化扫描器
//! （逐页对拍预期值：.bss 页必须全零、.data 页必须等于预置指纹）→
//! 访问闸（未通过扫描的段地址一律拒访并记账）。三重红线：
//! ①.bss 残留非零字节必红；②.data 内容与预置指纹漂移必红；
//! ③闸未建先访问必红（防「零初始化缺位被静默吞掉」）。
//!
//! 错误码段：0x2BA0（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const F0199_VERSION: &str = "F0199-bssaudit-v1";

/// 诊断码段基址（UNX-F0199 独占）。
pub const SEG_BASE: u16 = 0x2BA0;

/// 页大小（4KiB 建模粒度）。
pub const PAGE: usize = 0x1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// .bss 残留非零字节。
    BssDirty = SEG_BASE + 1,
    /// .data 与预置指纹漂移。
    DataDrift = SEG_BASE + 2,
    /// 扫描未通过即放行访问。
    GateOpenEarly = SEG_BASE + 3,
    /// 段重复登记。
    DupSeg = SEG_BASE + 4,
    /// 非整页长度。
    BadLength = SEG_BASE + 5,
    /// 审计未跑即出报告。
    ReportNoScan = SEG_BASE + 6,
    /// 未初始化访问被拒后仍继续。
    UninitAccess = SEG_BASE + 7,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 段种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegKind {
    /// .bss：零化义务。
    Bss,
    /// .data：预置值义务（指纹由内容 FNV 而来）。
    Data,
}

/// 段登记项。
#[derive(Debug, Clone)]
pub struct SegEntry {
    /// 段名。
    pub name: &'static str,
    /// 种类。
    pub kind: SegKind,
    /// 页数。
    pub pages: usize,
    /// .data 预置指纹（.bss 恒 0）。
    pub preset_fp: u64,
}

/// FNV-1a 64 位（与仓内其他指纹同构）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// 初始化审计器 + 访问闸。
#[derive(Debug, Default)]
pub struct InitAuditor {
    /// 段清单。
    pub segs: Vec<SegEntry>,
    /// 逐段扫描结论（Some(true)=绿）。
    scanned: Vec<bool>,
    /// 闸已放行。
    gate_open: bool,
    /// 拒访账。
    pub denials: Vec<(&'static str, u16)>,
}

impl InitAuditor {
    /// 登记段。
    pub fn declare(&mut self, e: SegEntry) -> Result<(), u16> {
        if self.segs.iter().any(|s| s.name == e.name) {
            return Err(E::DupSeg.code());
        }
        if e.pages == 0 {
            return Err(E::BadLength.code());
        }
        self.scanned.push(false);
        self.segs.push(e);
        Ok(())
    }

    /// .bss 扫描：逐页对拍全零。返回违例页数。
    pub fn scan_bss(&mut self, name: &str, pages: &[Vec<u8>]) -> Result<usize, u16> {
        let i = self.segs.iter().position(|s| s.name == name).ok_or(E::DupSeg.code())?;
        if self.segs[i].kind != SegKind::Bss {
            return Err(E::BadLength.code());
        }
        let mut bad = 0usize;
        for p in pages.iter() {
            if p.len() != PAGE {
                return Err(E::BadLength.code());
            }
            if p.iter().any(|b| *b != 0) {
                bad += 1;
            }
        }
        self.scanned[i] = bad == 0 && pages.len() == self.segs[i].pages;
        self.refresh_gate();
        Ok(bad)
    }

    /// .data 扫描：逐页指纹对拍预置值。
    pub fn scan_data(&mut self, name: &str, pages: &[Vec<u8>]) -> Result<usize, u16> {
        let i = self.segs.iter().position(|s| s.name == name).ok_or(E::DupSeg.code())?;
        if self.segs[i].kind != SegKind::Data {
            return Err(E::BadLength.code());
        }
        let mut bad = 0usize;
        for p in pages.iter() {
            if p.len() != PAGE {
                return Err(E::BadLength.code());
            }
            if fnv1a(p) != self.segs[i].preset_fp {
                bad += 1;
            }
        }
        self.scanned[i] = bad == 0 && pages.len() == self.segs[i].pages;
        self.refresh_gate();
        Ok(bad)
    }

    /// 闸：全段扫描绿才放行。
    fn refresh_gate(&mut self) {
        self.gate_open = self.scanned.len() == self.segs.len() && self.scanned.iter().all(|g| *g);
    }

    /// 访问闸。
    pub fn access(&mut self, name: &str) -> Result<(), u16> {
        if !self.gate_open {
            let code = E::GateOpenEarly.code();
            if let Some(n) = self.segs.iter().find(|s| s.name == name).map(|s| s.name) {
                self.denials.push((n, code));
            }
            return Err(code);
        }
        Ok(())
    }

    /// 报告前置红线：审计必须全绿。
    pub fn report_ready(&self) -> Result<(), u16> {
        if self.scanned.len() != self.segs.len() {
            return Err(E::ReportNoScan.code());
        }
        if !self.scanned.iter().all(|g| *g) {
            return Err(E::ReportNoScan.code());
        }
        Ok(())
    }
}

/// 预置一页 .data 内容（构造可复现指纹的夹具）。
fn preset_page(seed: u8) -> Vec<u8> {
    let mut p = vec![0u8; PAGE];
    for (i, b) in p.iter_mut().enumerate() {
        *b = (seed as usize ^ i) as u8;
    }
    p
}

/// J1 自证（判据原文：.bss/.data 初始化完整性扫描，未初始化访问检出）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：.bss 两页全零 + .data 两页预置。
    let bss_pages = vec![vec![0u8; PAGE], vec![0u8; PAGE]];
    let dp = preset_page(0x5A);
    let fp = fnv1a(&dp);
    let data_pages = vec![dp.clone(), preset_page(0x5A)];

    let mut a = InitAuditor::default();
    a.declare(SegEntry {
        name: ".bss",
        kind: SegKind::Bss,
        pages: 2,
        preset_fp: 0,
    })?;
    a.declare(SegEntry {
        name: ".data",
        kind: SegKind::Data,
        pages: 2,
        preset_fp: fp,
    })?;

    // 二、闸未开先访问必拒（防静默吞掉零化缺位）。
    if a.access(".bss").is_ok() {
        return Err(E::GateOpenEarly.code());
    }
    if a.denials.len() != 1 {
        return Err(E::UninitAccess.code());
    }

    // 三、J1 主体：全绿扫描 → 闸开 → 报告就绪。
    if a.scan_bss(".bss", &bss_pages)? != 0 {
        return Err(E::BssDirty.code());
    }
    if a.scan_data(".data", &data_pages)? != 0 {
        return Err(E::DataDrift.code());
    }
    a.access(".bss")?;
    a.access(".data")?;
    a.report_ready()?;

    // 四、红线逐项：.bss 脏页 / .data 漂移 / 重复登记 / 未扫全出报告。
    let mut dirty = bss_pages.clone();
    dirty[1][0x10] = 0x77;
    let mut b = InitAuditor::default();
    b.declare(SegEntry {
        name: ".bss",
        kind: SegKind::Bss,
        pages: 2,
        preset_fp: 0,
    })?;
    if b.scan_bss(".bss", &dirty)? != 1 {
        return Err(E::BssDirty.code());
    }
    if b.report_ready().is_ok() {
        return Err(E::ReportNoScan.code());
    }

    let mut drifted = data_pages.clone();
    drifted[0][0] ^= 1;
    let mut c = InitAuditor::default();
    c.declare(SegEntry {
        name: ".data",
        kind: SegKind::Data,
        pages: 2,
        preset_fp: fp,
    })?;
    if c.scan_data(".data", &drifted)? != 1 {
        return Err(E::DataDrift.code());
    }

    let mut d = InitAuditor::default();
    let entry = SegEntry {
        name: ".bss",
        kind: SegKind::Bss,
        pages: 1,
        preset_fp: 0,
    };
    d.declare(entry)?;
    let entry2 = SegEntry {
        name: ".bss",
        kind: SegKind::Bss,
        pages: 1,
        preset_fp: 0,
    };
    if d.declare(entry2).is_ok() {
        return Err(E::DupSeg.code());
    }

    // 五、判别式段位对拍。
    if E::BssDirty.code() != SEG_BASE + 1 || E::UninitAccess.code() != SEG_BASE + 7 {
        return Err(E::BssDirty.code());
    }
    Ok(())
}
