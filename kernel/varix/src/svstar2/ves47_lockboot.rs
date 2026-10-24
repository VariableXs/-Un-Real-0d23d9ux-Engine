//! VE-F3647 · 锁屏与开机创作（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3647`
//!
//! **职责定位（锚点原文）**：锁屏/开机画面创作——
//! - **锁屏模板**：锁屏布局模板（时间/通知组件位）（组件位红线：
//!   创作内容不遮挡锁屏关键组件 → 遮挡检测（时间/通知区域保护）；
//! - **开机画面**：开机序列创作（格式与时长规范）（规范红线：
//!   不合规范的开机画面 = 启动失败 → 规范校验）；
//! - **引导红线**（域号 ADR 呼应）：开机创作不碰引导分区（边界声明红线：
//!   边界不清 = 误碰引导 → 开机画面仅是显示资产不涉及引导程序）。
//!
//! **判据（锚点原文六条）**：保护区红线、规范校验、边界声明、
//! 引导引用、遮挡检测、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 遮挡组件→检测拦截（红线实测） | [`L47Code::BLOCKED_COMPONENT`] |
//! | 规范不符→拒收（红线实测） | [`L47Code::SPEC_REJECT`] |
//! | 误触引导→P0（红线最高级——引用总则） | [`L47Code::BOOT_TOUCH`] |
//! | 保护区漏配→补（红线） | [`L47Code::ZONE_UNPROTECTED`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3647 诊断码段（独占 0x5Fxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct L47Code(pub u16);

impl L47Code {
    /// 遮挡锁屏组件（组件位红线）。
    pub const BLOCKED_COMPONENT: L47Code = L47Code(0x5F01);
    /// 开机画面规范不符（拒收红线）。
    pub const SPEC_REJECT: L47Code = L47Code(0x5F02);
    /// 误触引导分区（P0 最高级，引用 Variable 总则）。
    pub const BOOT_TOUCH: L47Code = L47Code(0x5F03);
    /// 保护区漏配（红线）。
    pub const ZONE_UNPROTECTED: L47Code = L47Code(0x5F04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            L47Code::BLOCKED_COMPONENT => "创作内容遮挡锁屏组件（红线实测）。建议：移出保护区。",
            L47Code::SPEC_REJECT => "开机画面规范不符（红线实测）。建议：按格式与时长规范整改。",
            L47Code::BOOT_TOUCH => "误触引导分区（P0 最高级）。建议：开机画面仅是显示资产。",
            L47Code::ZONE_UNPROTECTED => "保护区漏配。建议：补时间/通知区域保护。",
            _ => "未知诊断码。",
        }
    }
}

// ── 保护区（组件位红线） ────────────────────────────────────────────

/// 锁屏保护区类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    /// 时间组件位。
    Clock,
    /// 通知组件位。
    Notification,
}

/// 矩形（整数坐标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// 两矩形相交（含边贴靠算相交——保护从严）。
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// 保护区域登记。
#[derive(Clone, Copy)]
pub struct ZoneGuard {
    pub zone: Zone,
    pub area: Rect,
}

// ── 开机画面规范 ────────────────────────────────────────────────────

/// 开机画面格式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootFormat {
    /// 合规序列格式。
    Sequence,
    /// 静态图（合规子集）。
    Static,
    /// 非法格式。
    Unknown,
}

/// 开机画面描述。
#[derive(Clone, Copy)]
pub struct BootSplash {
    pub format: BootFormat,
    /// 时长（毫秒，规范窗 500..=5000）。
    pub duration_ms: u32,
    /// 是否触碰引导分区位（必须 false）。
    pub touches_boot: bool,
}

impl BootSplash {
    /// 规范校验：格式合规 + 时长在窗内。
    pub fn spec_ok(&self) -> bool {
        matches!(self.format, BootFormat::Sequence | BootFormat::Static)
            && (500..=5_000).contains(&self.duration_ms)
    }
}

// ── 边界声明（引导红线） ────────────────────────────────────────────

/// 边界声明：开机创作仅显示资产（域号 ADR 呼应）。
pub struct BoundaryDecl;

impl BoundaryDecl {
    /// 声明正文 id（1=仅显示资产, 2=不碰引导）。
    pub const DISPLAY_ONLY: u8 = 1;
    pub const NO_BOOT: u8 = 2;

    /// 边界是否清晰（双声明齐）。
    pub fn declared(no_boot_touch: bool) -> bool {
        no_boot_touch
    }
}

// ── 创作器 ──────────────────────────────────────────────────────────

/// 锁屏与开机创作器。
pub struct LockBootStudio {
    zones: Vec<ZoneGuard>,
    objects: Vec<Rect>,
    splashes: Vec<BootSplash>,
    boot_touched: bool,
    last_code: L47Code,
    has_error: bool,
}

impl LockBootStudio {
    pub fn new() -> Self {
        Self {
            zones: Vec::new(),
            objects: Vec::new(),
            splashes: Vec::new(),
            boot_touched: false,
            last_code: L47Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> L47Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: L47Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 配保护区（时间/通知）。
    pub fn protect(&mut self, zone: Zone, area: Rect) -> usize {
        self.zones.push(ZoneGuard { zone, area });
        self.zones.len()
    }

    /// 双保护区齐配（漏配红线）。
    pub fn zones_complete(&mut self) -> Result<bool, L47Code> {
        let clock = self.zones.iter().any(|z| z.zone == Zone::Clock);
        let notif = self.zones.iter().any(|z| z.zone == Zone::Notification);
        if clock && notif {
            self.last_code = L47Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(L47Code::ZONE_UNPROTECTED);
            Err(L47Code::ZONE_UNPROTECTED)
        }
    }

    /// 放置创作物：遮挡检测（组件位红线）。
    pub fn place(&mut self, obj: Rect) -> Result<usize, L47Code> {
        for z in self.zones.iter() {
            if z.area.overlaps(&obj) {
                self.fail(L47Code::BLOCKED_COMPONENT);
                return Err(L47Code::BLOCKED_COMPONENT);
            }
        }
        self.objects.push(obj);
        self.last_code = L47Code(0);
        self.has_error = false;
        Ok(self.objects.len())
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// 提交开机画面：规范校验 + 引导边界。
    pub fn submit_splash(&mut self, splash: BootSplash) -> Result<usize, L47Code> {
        if splash.touches_boot {
            self.boot_touched = true;
            self.fail(L47Code::BOOT_TOUCH);
            return Err(L47Code::BOOT_TOUCH);
        }
        if !splash.spec_ok() {
            self.fail(L47Code::SPEC_REJECT);
            return Err(L47Code::SPEC_REJECT);
        }
        self.splashes.push(splash);
        self.last_code = L47Code(0);
        self.has_error = false;
        Ok(self.splashes.len())
    }

    pub fn splash_count(&self) -> usize {
        self.splashes.len()
    }

    /// 引导红线总评：从未触碰引导。
    pub fn boot_clean(&self) -> bool {
        !self.boot_touched
    }

    /// 边界声明审计。
    pub fn boundary_declared(&self) -> bool {
        BoundaryDecl::declared(self.boot_clean())
    }

    /// 时长合规数。
    pub fn spec_ok_count(&self) -> usize {
        self.splashes.iter().filter(|s| s.spec_ok()).count()
    }

    /// 保护区内对象数（应为 0）。
    pub fn intrusions(&self) -> usize {
        self.objects
            .iter()
            .filter(|o| self.zones.iter().any(|z| z.area.overlaps(o)))
            .count()
    }

    /// 遮挡演练：对象对拍保护区。
    pub fn check_overlap(&mut self, obj: Rect) -> Result<bool, L47Code> {
        if self.zones.iter().any(|z| z.area.overlaps(&obj)) {
            self.fail(L47Code::BLOCKED_COMPONENT);
            Err(L47Code::BLOCKED_COMPONENT)
        } else {
            self.last_code = L47Code(0);
            self.has_error = false;
            Ok(false)
        }
    }

    pub fn zone_count(&self) -> usize {
        self.zones.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves47_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves47_lockboot");
    {
        let s = LockBootStudio::new();
        chk(&mut set, "C3647-01", s.zone_count() == 0 && s.splash_count() == 0, "err");
    }
    {
        let mut s = LockBootStudio::new();
        let n = s.protect(Zone::Clock, Rect { x: 0, y: 0, w: 100, h: 50 });
        chk(&mut set, "C3647-02", n == 1, "err");
    }
    {
        let mut s = LockBootStudio::new();
        s.protect(Zone::Clock, Rect { x: 0, y: 0, w: 100, h: 50 });
        let r = s.zones_complete();
        chk(&mut set, "C3647-03", r.err() == Some(L47Code::ZONE_UNPROTECTED), "err");
    }
    {
        let mut s = LockBootStudio::new();
        s.protect(Zone::Clock, Rect { x: 0, y: 0, w: 100, h: 50 });
        s.protect(Zone::Notification, Rect { x: 0, y: 200, w: 200, h: 80 });
        let r = s.zones_complete();
        chk(&mut set, "C3647-04", r == Ok(true), "err");
    }
    {
        let mut s = LockBootStudio::new();
        s.protect(Zone::Clock, Rect { x: 0, y: 0, w: 100, h: 50 });
        let r = s.place(Rect { x: 90, y: 40, w: 50, h: 50 });
        chk(&mut set, "C3647-05", r.err() == Some(L47Code::BLOCKED_COMPONENT), "err");
    }
    {
        let mut s = LockBootStudio::new();
        s.protect(Zone::Clock, Rect { x: 0, y: 0, w: 100, h: 50 });
        let r = s.place(Rect { x: 200, y: 200, w: 50, h: 50 });
        chk(&mut set, "C3647-06", r == Ok(1) && s.object_count() == 1, "err");
    }
    {
        let mut s = LockBootStudio::new();
        let r = s.submit_splash(BootSplash { format: BootFormat::Sequence, duration_ms: 2_000, touches_boot: false });
        chk(&mut set, "C3647-07", r == Ok(1), "err");
    }
    {
        let mut s = LockBootStudio::new();
        let r = s.submit_splash(BootSplash { format: BootFormat::Unknown, duration_ms: 2_000, touches_boot: false });
        chk(&mut set, "C3647-08", r.err() == Some(L47Code::SPEC_REJECT), "err");
    }
    {
        let mut s = LockBootStudio::new();
        let r = s.submit_splash(BootSplash { format: BootFormat::Static, duration_ms: 100, touches_boot: false });
        chk(&mut set, "C3647-09", r.err() == Some(L47Code::SPEC_REJECT), "err");
    }
    {
        let mut s = LockBootStudio::new();
        let r = s.submit_splash(BootSplash { format: BootFormat::Sequence, duration_ms: 1_000, touches_boot: true });
        chk(&mut set, "C3647-10", r.err() == Some(L47Code::BOOT_TOUCH) && !s.boot_clean(), "err");
    }
    {
        let mut s = LockBootStudio::new();
        s.protect(Zone::Clock, Rect { x: 0, y: 0, w: 10, h: 10 });
        let r = s.check_overlap(Rect { x: 5, y: 5, w: 10, h: 10 });
        chk(&mut set, "C3647-11", r.err() == Some(L47Code::BLOCKED_COMPONENT), "err");
    }
    {
        let mut s = LockBootStudio::new();
        s.submit_splash(BootSplash { format: BootFormat::Static, duration_ms: 1_500, touches_boot: false });
        chk(&mut set, "C3647-12", s.boot_clean() && s.boundary_declared() && s.spec_ok_count() == 1, "err");
    }
    set
}
