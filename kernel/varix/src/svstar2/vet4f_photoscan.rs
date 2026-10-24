#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xD880）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD880;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CD8BCode(pub u16);

impl CD8BCode {
    pub const SCAN_MISS: Self = Self(SEG_BASE);        // 违例漏扫
    pub const API_MISSING: Self = Self(SEG_BASE + 1);  // API 缺段
    pub const OVERHEAD_OVER: Self = Self(SEG_BASE + 2); // 开销超限
    pub const REPORT_FUZZY: Self = Self(SEG_BASE + 3);  // 定位模糊
    pub const GATE_FAIL: Self = Self(SEG_BASE + 4);     // 准入门失败

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::SCAN_MISS => "违例漏扫",
            Self::API_MISSING => "API缺段",
            Self::OVERHEAD_OVER => "开销超限",
            Self::REPORT_FUZZY => "定位模糊",
            Self::GATE_FAIL => "准入门失败",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::SCAN_MISS, Self::API_MISSING, Self::OVERHEAD_OVER,
        Self::REPORT_FUZZY, Self::GATE_FAIL,
    ];
}

// ── 光敏违例 ────────────────────────────────────────────────────────

/// 光敏违例定位（三维）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhotosensitiveViolation {
    pub id: u32,
    /// 闪烁频率 Hz。
    pub freq_hz: u16,
    /// 亮度差（0..=255）。
    pub brightness_delta: u8,
    /// 屏幕区域（归一化 0..100）。
    pub region_x: u8, pub region_y: u8,
}

impl PhotosensitiveViolation {
    /// 光敏风险判定：3..=30 Hz 且亮度差 >20。
    pub fn is_risk(&self) -> bool {
        (3..=30).contains(&self.freq_hz) && self.brightness_delta > 20
    }

    /// 定位有效。
    pub fn location_valid(&self) -> bool {
        self.region_x <= 100 && self.region_y <= 100
    }
}

// ── 双扫描引擎 ──────────────────────────────────────────────────────

/// 静态资源扫描。
#[derive(Debug, Clone)]
pub struct StaticScanner {
    pub violations: Vec<PhotosensitiveViolation>,
    pub scanned: bool,
}

impl StaticScanner {
    pub fn new() -> Self { Self { violations: Vec::new(), scanned: false } }

    /// 扫描资源。
    pub fn scan(&mut self) {
        self.scanned = true;
    }

    /// 违例清单（仅风险项）。
    pub fn risks(&self) -> Vec<&PhotosensitiveViolation> {
        self.violations.iter().filter(|v| v.is_risk()).collect()
    }
}

/// 运行时动态监控。
#[derive(Debug, Clone)]
pub struct RuntimeMonitor {
    pub samples: Vec<PhotosensitiveViolation>,
    pub active: bool,
}

impl RuntimeMonitor {
    pub fn new() -> Self { Self { samples: Vec::new(), active: false } }

    /// 启动监控。
    pub fn start(&mut self) { self.active = true; }

    /// 实时违例。
    pub fn live_risks(&self) -> Vec<&PhotosensitiveViolation> {
        self.samples.iter().filter(|v| v.is_risk()).collect()
    }
}

// ── 扫描报告 ────────────────────────────────────────────────────────

/// 扫描报告（违例清单+定位）。
#[derive(Debug, Clone)]
pub struct ScanReport {
    pub violations: Vec<PhotosensitiveViolation>,
    pub source_static: bool,
    pub source_runtime: bool,
}

impl ScanReport {
    /// 定位复述：所有违例必须有有效坐标。
    pub fn locations_valid(&self) -> bool {
        self.violations.iter().all(|v| v.location_valid())
    }

    /// 违例计数。
    pub fn risk_count(&self) -> usize {
        self.violations.iter().filter(|v| v.is_risk()).count()
    }
}

// ── 扫描 API（三段式）──────────────────────────────────────────────

/// 发布前扫描 API：三段（提交→扫描→放行/拒绝）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanApiStage { Submit, Scan, Verdict }

impl ScanApiStage {
    pub const ALL: [Self; 3] = [Self::Submit, Self::Scan, Self::Verdict];
}

/// 扫描 API 状态机。
#[derive(Debug, Clone)]
pub struct ScanApi {
    pub stage: Option<ScanApiStage>,
    pub report: Option<ScanReport>,
    pub approved: bool,
}

impl ScanApi {
    pub fn new() -> Self { Self { stage: None, report: None, approved: false } }

    /// 提交。
    pub fn submit(&mut self) { self.stage = Some(ScanApiStage::Submit); }

    /// 扫描。
    pub fn scan(&mut self, report: ScanReport) {
        if self.stage == Some(ScanApiStage::Submit) {
            self.stage = Some(ScanApiStage::Scan);
            self.report = Some(report);
        }
    }

    /// 裁决：零违例才放行（准入内联）。
    pub fn verdict(&mut self) -> bool {
        if self.stage != Some(ScanApiStage::Scan) {
            return false;
        }
        self.stage = Some(ScanApiStage::Verdict);
        self.approved = self.report.as_ref().map(|r| r.risk_count() == 0).unwrap_or(false);
        self.approved
    }

    /// API 三段完整。
    pub fn complete(&self) -> bool {
        self.stage == Some(ScanApiStage::Verdict)
    }
}

// ── 扫描性能 ────────────────────────────────────────────────────────

/// 扫描开销声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanOverhead {
    /// 离线扫描时间 ms。
    pub offline_ms: u32,
    /// 在线采样率百分比（0..=100）。
    pub online_sample_pct: u8,
}

impl ScanOverhead {
    /// 开销诚实：在线采样 ≤10%（降级抽样）。
    pub fn online_ok(&self) -> bool { self.online_sample_pct <= 10 }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3868_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4f_photoscan");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 光敏风险判定
    {
        let v = PhotosensitiveViolation { id: 1, freq_hz: 10, brightness_delta: 50, region_x: 50, region_y: 50 };
        chk(&mut set, "C3868-01", v.is_risk(), "err");
    }
    // 2 非风险（低频）
    {
        let v = PhotosensitiveViolation { id: 1, freq_hz: 1, brightness_delta: 50, region_x: 50, region_y: 50 };
        chk(&mut set, "C3868-02", !v.is_risk(), "err");
    }
    // 3 非风险（低亮度差）
    {
        let v = PhotosensitiveViolation { id: 1, freq_hz: 10, brightness_delta: 5, region_x: 50, region_y: 50 };
        chk(&mut set, "C3868-03", !v.is_risk(), "err");
    }
    // 4 高频非风险
    {
        let v = PhotosensitiveViolation { id: 1, freq_hz: 60, brightness_delta: 50, region_x: 50, region_y: 50 };
        chk(&mut set, "C3868-04", !v.is_risk(), "err");
    }
    // 5 定位有效
    {
        let v = PhotosensitiveViolation { id: 1, freq_hz: 10, brightness_delta: 50, region_x: 0, region_y: 100 };
        chk(&mut set, "C3868-05", v.location_valid(), "err");
    }
    // 6 定位无效
    {
        let v = PhotosensitiveViolation { id: 1, freq_hz: 10, brightness_delta: 50, region_x: 150, region_y: 50 };
        chk(&mut set, "C3868-06", !v.location_valid(), "err");
    }
    // 7 静态扫描
    {
        let mut s = StaticScanner::new();
        s.scan();
        chk(&mut set, "C3868-07", s.scanned, "err");
    }
    // 8 静态风险过滤
    {
        let mut s = StaticScanner::new();
        s.violations.push(PhotosensitiveViolation { id: 1, freq_hz: 10, brightness_delta: 50, region_x: 0, region_y: 0 });
        s.violations.push(PhotosensitiveViolation { id: 2, freq_hz: 1, brightness_delta: 50, region_x: 0, region_y: 0 });
        chk(&mut set, "C3868-08", s.risks().len() == 1, "err");
    }
    // 9 运行时监控
    {
        let mut m = RuntimeMonitor::new();
        m.start();
        m.samples.push(PhotosensitiveViolation { id: 1, freq_hz: 20, brightness_delta: 30, region_x: 10, region_y: 10 });
        chk(&mut set, "C3868-09", m.live_risks().len() == 1, "err");
    }
    // 10 报告定位全有效
    {
        let r = ScanReport {
            violations: vec![
                PhotosensitiveViolation { id: 1, freq_hz: 10, brightness_delta: 50, region_x: 30, region_y: 40 },
            ],
            source_static: true,
            source_runtime: false,
        };
        chk(&mut set, "C3868-10", r.locations_valid(), "err");
    }
    // 11 API 三段
    {
        let mut api = ScanApi::new();
        api.submit();
        api.scan(ScanReport { violations: Vec::new(), source_static: true, source_runtime: false });
        chk(&mut set, "C3868-11", api.verdict() && api.complete(), "err");
    }
    // 12 API 有违例不放行
    {
        let mut api = ScanApi::new();
        api.submit();
        api.scan(ScanReport {
            violations: vec![PhotosensitiveViolation { id: 1, freq_hz: 10, brightness_delta: 50, region_x: 0, region_y: 0 }],
            source_static: true,
            source_runtime: false,
        });
        chk(&mut set, "C3868-12", !api.verdict(), "err");
    }
    // 13 API 跳段无效
    {
        let mut api = ScanApi::new();
        chk(&mut set, "C3868-13", !api.verdict(), "err");
    }
    // 14 API 三段枚举
    {
        chk(&mut set, "C3868-14", ScanApiStage::ALL.len() == 3, "err");
    }
    // 15 在线开销 OK
    {
        let o = ScanOverhead { offline_ms: 500, online_sample_pct: 5 };
        chk(&mut set, "C3868-15", o.online_ok(), "err");
    }
    // 16 在线开销超标
    {
        let o = ScanOverhead { offline_ms: 500, online_sample_pct: 20 };
        chk(&mut set, "C3868-16", !o.online_ok(), "err");
    }
    // 17 报告风险计数
    {
        let r = ScanReport {
            violations: vec![
                PhotosensitiveViolation { id: 1, freq_hz: 10, brightness_delta: 50, region_x: 0, region_y: 0 },
                PhotosensitiveViolation { id: 2, freq_hz: 60, brightness_delta: 50, region_x: 0, region_y: 0 },
            ],
            source_static: true,
            source_runtime: true,
        };
        chk(&mut set, "C3868-17", r.risk_count() == 1, "err");
    }
    // 18 零违例放行
    {
        let mut api = ScanApi::new();
        api.submit();
        api.scan(ScanReport { violations: Vec::new(), source_static: false, source_runtime: true });
        chk(&mut set, "C3868-18", api.verdict() && api.approved, "err");
    }

    set
}
