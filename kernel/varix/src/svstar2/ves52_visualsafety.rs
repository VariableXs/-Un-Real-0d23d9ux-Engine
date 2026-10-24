//! VE-F3652 · 视觉创作安全（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3652`
//!
//! **职责定位（锚点原文）**：视觉创作安全（F3612 复刻细化）——
//! - **素材安全**：导入素材校验（复述同待遇）；
//! - **隐写检测**：图片隐写扫描（隐写红线：图片夹带数据 = 走私通道，
//!   隐写零容忍红线 P0）；
//! - **元数据清理**：导出资产元数据清理（EXIF 隐私）（元数据红线：
//!   导出带 GPS 等元数据 = 隐私泄露 → 清理强制 + 白名单保留版权字段）；
//! - **审计复用**（复述）。
//!
//! **判据（锚点原文六条）**：隐写 P0、元数据 P0、白名单平衡、
//! 同待遇复述、模式复刻、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 隐写检出→P0（红线实测演练——样本注入） | [`S52Code::STEG_DETECTED`] |
//! | 元数据带 GPS→P0（红线实测——导出扫描） | [`S52Code::GPS_LEAK`] |
//! | 绕检→P0（复述） | [`S52Code::SCAN_BYPASS`] |
//! | 清理误删版权→白名单修正（平衡显性） | [`S52Code::COPYRIGHT_STRIPPED`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3652 诊断码段（独占 0x64xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct S52Code(pub u16);

impl S52Code {
    /// 隐写检出（P0 零容忍）。
    pub const STEG_DETECTED: S52Code = S52Code(0x6401);
    /// 元数据带 GPS（P0 隐私红线）。
    pub const GPS_LEAK: S52Code = S52Code(0x6402);
    /// 绕过扫描（P0）。
    pub const SCAN_BYPASS: S52Code = S52Code(0x6403);
    /// 版权字段被误删（白名单修正）。
    pub const COPYRIGHT_STRIPPED: S52Code = S52Code(0x6404);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            S52Code::STEG_DETECTED => "隐写检出（P0 零容忍）。建议：拒收素材并立案。",
            S52Code::GPS_LEAK => "元数据含 GPS（P0 隐私红线）。建议：导出前强制清理。",
            S52Code::SCAN_BYPASS => "绕过扫描（P0）。建议：扫描为导出必经段。",
            S52Code::COPYRIGHT_STRIPPED => "版权字段被误删。建议：白名单保留版权字段。",
            _ => "未知诊断码。",
        }
    }
}

// ── 隐写扫描器 ──────────────────────────────────────────────────────

/// 素材样本（隐写检测口径）。
#[derive(Clone, Copy)]
pub struct Sample {
    /// LSB 异常分（0..=100，高=可疑）。
    pub lsb_score: u8,
    /// 尾部附加字节。
    pub trailing_bytes: u32,
}

impl Sample {
    /// 隐写嫌疑判定（零容忍：任一超阈即命中）。
    pub fn stego_suspect(&self) -> bool {
        self.lsb_score > 25 || self.trailing_bytes > 0
    }
}

/// 隐写扫描器。
pub struct StegoScanner {
    scanned: u64,
    hits: u32,
}

impl StegoScanner {
    pub fn new() -> Self {
        Self { scanned: 0, hits: 0 }
    }

    /// 扫描一个样本；命中即 P0。
    pub fn scan(&mut self, s: &Sample) -> Result<bool, S52Code> {
        self.scanned += 1;
        if s.stego_suspect() {
            self.hits += 1;
            return Err(S52Code::STEG_DETECTED);
        }
        Ok(false)
    }

    pub fn scanned(&self) -> u64 {
        self.scanned
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }
}

// ── 元数据清理（白名单） ────────────────────────────────────────────

/// 元数据字段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    /// GPS 坐标（隐私）。
    Gps,
    /// 拍摄设备序列（隐私）。
    Serial,
    /// 版权声明（白名单保留）。
    Copyright,
    /// 创作时间（白名单保留）。
    Created,
}

impl Field {
    /// 白名单：允许保留的字段。
    pub fn whitelisted(self) -> bool {
        matches!(self, Field::Copyright | Field::Created)
    }

    /// 隐私字段（必须清理）。
    pub fn is_private(self) -> bool {
        matches!(self, Field::Gps | Field::Serial)
    }
}

/// 元数据清理器。
pub struct MetaCleaner {
    /// 来源字段清单。
    present: Vec<Field>,
    stripped: Vec<Field>,
    kept: Vec<Field>,
}

impl MetaCleaner {
    pub fn new() -> Self {
        Self { present: Vec::new(), stripped: Vec::new(), kept: Vec::new() }
    }

    /// 登记源元数据。
    pub fn ingest(&mut self, f: Field) -> usize {
        self.present.push(f);
        self.present.len()
    }

    /// 清理：隐私字段必删，白名单保留，误删版权报码。
    pub fn clean(&mut self) -> Result<usize, S52Code> {
        let mut err = None;
        for f in self.present.iter().copied() {
            if f.is_private() {
                self.stripped.push(f);
            } else if f.whitelisted() {
                self.kept.push(f);
            }
        }
        if self.present.contains(&Field::Copyright) && !self.kept.contains(&Field::Copyright) {
            err = Some(S52Code::COPYRIGHT_STRIPPED);
        }
        self.present.clear();
        match err {
            Some(c) => Err(c),
            None => Ok(self.stripped.len()),
        }
    }

    /// 导出扫描：仍有隐私字段即 P0。
    pub fn export_scan(&self) -> Result<bool, S52Code> {
        if self.present.iter().any(|f| f.is_private()) {
            Err(S52Code::GPS_LEAK)
        } else {
            Ok(true)
        }
    }

    pub fn stripped_count(&self) -> usize {
        self.stripped.len()
    }

    pub fn kept_count(&self) -> usize {
        self.kept.len()
    }
}

// ── 同待遇导入校验 ──────────────────────────────────────────────────

/// 导入校验（与 F3608 同待遇）。
pub struct ImportCheck {
    pub virus_scan: bool,
    pub format_ok: bool,
}

impl ImportCheck {
    pub fn pass(&self) -> bool {
        self.virus_scan && self.format_ok
    }
}

// ── 安全面板 ────────────────────────────────────────────────────────

/// 视觉创作安全面板。
pub struct VisualSafety {
    scanner: StegoScanner,
    cleaner: MetaCleaner,
    /// 扫描是否为导出必经段（防绕检）。
    scan_mandatory: bool,
    bypass_hits: u32,
    last_code: S52Code,
    has_error: bool,
}

impl VisualSafety {
    pub fn new() -> Self {
        Self {
            scanner: StegoScanner::new(),
            cleaner: MetaCleaner::new(),
            scan_mandatory: true,
            bypass_hits: 0,
            last_code: S52Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> S52Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: S52Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 导入同待遇校验。
    pub fn import_gate(&mut self, check: ImportCheck) -> Result<bool, S52Code> {
        if check.pass() {
            self.last_code = S52Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(S52Code::SCAN_BYPASS);
            Err(S52Code::SCAN_BYPASS)
        }
    }

    /// 隐写扫描（P0）。
    pub fn scan_stego(&mut self, s: &Sample) -> Result<bool, S52Code> {
        self.scanner.scan(s).map_err(|c| {
            self.fail(c);
            c
        })
    }

    /// 登记元数据字段。
    pub fn ingest_meta(&mut self, f: Field) -> usize {
        self.cleaner.ingest(f)
    }

    /// 清理元数据。
    pub fn clean_meta(&mut self) -> Result<usize, S52Code> {
        self.cleaner.clean().map_err(|c| {
            self.fail(c);
            c
        })
    }

    /// 导出前强制扫描路径（防绕检）。
    pub fn export_path(&mut self, scanned: bool) -> Result<bool, S52Code> {
        if self.scan_mandatory && !scanned {
            self.bypass_hits += 1;
            self.fail(S52Code::SCAN_BYPASS);
            return Err(S52Code::SCAN_BYPASS);
        }
        self.cleaner.export_scan().map_err(|c| {
            self.fail(c);
            c
        })
    }

    pub fn stego_hits(&self) -> u32 {
        self.scanner.hits()
    }

    pub fn scanned(&self) -> u64 {
        self.scanner.scanned()
    }

    pub fn bypass_hits(&self) -> u32 {
        self.bypass_hits
    }

    pub fn stripped(&self) -> usize {
        self.cleaner.stripped_count()
    }

    pub fn kept(&self) -> usize {
        self.cleaner.kept_count()
    }

    /// 模式复刻总评：扫描必经 + 无隐写 + 无绕检。
    pub fn safety_green(&self) -> bool {
        self.scan_mandatory && self.scanner.hits() == 0 && self.bypass_hits == 0
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves52_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves52_visualsafety");
    {
        let s = VisualSafety::new();
        chk(&mut set, "C3652-01", s.stego_hits() == 0 && s.bypass_hits() == 0, "err");
    }
    {
        let mut s = VisualSafety::new();
        let r = s.import_gate(ImportCheck { virus_scan: true, format_ok: true });
        chk(&mut set, "C3652-02", r == Ok(true), "err");
    }
    {
        let mut s = VisualSafety::new();
        let r = s.import_gate(ImportCheck { virus_scan: false, format_ok: true });
        chk(&mut set, "C3652-03", r.err() == Some(S52Code::SCAN_BYPASS), "err");
    }
    {
        let mut s = VisualSafety::new();
        let r = s.scan_stego(&Sample { lsb_score: 5, trailing_bytes: 0 });
        chk(&mut set, "C3652-04", r == Ok(false) && s.scanned() == 1, "err");
    }
    {
        let mut s = VisualSafety::new();
        let r = s.scan_stego(&Sample { lsb_score: 80, trailing_bytes: 0 });
        chk(&mut set, "C3652-05", r.err() == Some(S52Code::STEG_DETECTED) && s.stego_hits() == 1, "err");
    }
    {
        let mut s = VisualSafety::new();
        let r = s.scan_stego(&Sample { lsb_score: 5, trailing_bytes: 100 });
        chk(&mut set, "C3652-06", r.err() == Some(S52Code::STEG_DETECTED), "err");
    }
    {
        let mut s = VisualSafety::new();
        s.ingest_meta(Field::Gps);
        s.ingest_meta(Field::Copyright);
        let r = s.clean_meta();
        chk(&mut set, "C3652-07", r == Ok(1) && s.kept() == 1, "err");
    }
    {
        let mut s = VisualSafety::new();
        s.ingest_meta(Field::Gps);
        let r = s.export_path(true);
        chk(&mut set, "C3652-08", r.err() == Some(S52Code::GPS_LEAK), "err");
    }
    {
        let mut s = VisualSafety::new();
        s.ingest_meta(Field::Gps);
        s.clean_meta();
        let r = s.export_path(true);
        chk(&mut set, "C3652-09", r == Ok(true), "err");
    }
    {
        let mut s = VisualSafety::new();
        let r = s.export_path(false);
        chk(&mut set, "C3652-10", r.err() == Some(S52Code::SCAN_BYPASS) && s.bypass_hits() == 1, "err");
    }
    {
        let f = Field::Copyright;
        chk(&mut set, "C3652-11", f.whitelisted() && !f.is_private(), "err");
    }
    {
        let mut s = VisualSafety::new();
        s.scan_stego(&Sample { lsb_score: 1, trailing_bytes: 0 }).unwrap();
        chk(&mut set, "C3652-12", s.safety_green(), "err");
    }
    set
}
