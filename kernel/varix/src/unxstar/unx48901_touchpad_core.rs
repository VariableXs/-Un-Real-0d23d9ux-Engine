//! UNX-F48901 触控板分类与软键区几何 · 0x70C80
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F48901-J1 clickpad / 物理键 / 混合三类判别 ×10^3 零误判；
//!   ③ UNX-F48902-J1 软键区左右中三段 + 迟滞 ×10^3 判定零抖动。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：能力集合法性 → 分类 → 几何自洽 → 压力语义 → 迟滞后判 →
//!           复算一致。分类必须先于几何：物理键机型没有软键区，若先按
//!           几何查区会把实体按键的坐标当成软键；迟滞必须放在边界判断
//!           之后，否则「先记住上一键、再算边界」会让迁移带反向吞掉新
//!           区。复算一致必须放在所有判定之后，防止有状态路径掩盖漂移。
//! 关键差异：本模块只回答「这片坐标属于哪个键 / 这颗压力值处于什么
//!           语义态」，不做手势、不做光标位移、不发事件。B06 后续模块
//!           消费本模块的 Button 与 PressureState；谁把原始坐标带出本
//!           模块，谁负责再做边界判定。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70C80，VX6 批 B06 族首槽）。
pub const SEG_BASE: u32 = 0x70C80;

/// 冻结常量（卡面判据锚点）。
pub const CLASS_CASES: usize = 3;
pub const CLICK_ROUNDS: u32 = 1000;
pub const PRESSURE_ROUNDS: u32 = 1000;
pub const ZONE_COUNT: usize = 4;
pub const PRESSURE_FULL: u16 = 1000;

// ── 诊断码闭集（本模块窗口 +0x01..+0x09） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TpCode {
    /// 设备标识非法（vid/pid/fw 为零或超界）
    DevBad,
    /// 能力位图非法（未知位 / 互斥组合）
    CapBad,
    /// 分类失败（关键能力缺失）
    ClassBad,
    /// 软键区几何非法（段和为负 / 越界 / 迟滞过大）
    ZoneBad,
    /// 压力语义非法（量程/阈值次序错误）
    PressureBad,
    /// 开关状态非法（已禁用仍受理）
    SwitchBad,
    /// 复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl TpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            TpCode::DevBad => "设备标识非法：vid/pid/fw 必须非零",
            TpCode::CapBad => "能力位图非法：含未知位或互斥组合",
            TpCode::ClassBad => "分类失败：clickpad 缺 BUTTONPAD 或物理键缺 BTN_LEFT",
            TpCode::ZoneBad => "软键区几何非法：段宽越界或迟滞吞段",
            TpCode::PressureBad => "压力语义非法：低阈 ≥ 高阈或超量程",
            TpCode::SwitchBad => "开关状态非法：禁用后仍递交坐标",
            TpCode::ReplayBad => "复算不一致：两次判定结果不同",
            TpCode::SegmentBad => "段位越界：诊断码逃出 0x70C80 窗口",
        }
    }

    pub const ALL: [TpCode; 8] = [
        TpCode::DevBad,
        TpCode::CapBad,
        TpCode::ClassBad,
        TpCode::ZoneBad,
        TpCode::PressureBad,
        TpCode::SwitchBad,
        TpCode::ReplayBad,
        TpCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < TpCode::ALL.len() {
        if TpCode::ALL[i].val() == code {
            return TpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 能力位图 ──────────────────────────────────────────────────────
pub const CAP_BUTTONPAD: u16 = 0x0001;
pub const CAP_PHYS_BUTTONS: u16 = 0x0002;
pub const CAP_PRESSURE: u16 = 0x0004;
pub const CAP_MT: u16 = 0x0008;
pub const CAP_SEMI_MT: u16 = 0x0010;
pub const CAP_HIRES: u16 = 0x0020;
pub const CAP_PALM: u16 = 0x0040;
pub const CAP_KNOWN: u16 = 0x007F;

/// 位图审计：只有已知位，且两种按键面互斥。
pub fn audit_caps(caps: u16) -> Result<u16, u32> {
    if caps & !CAP_KNOWN != 0 {
        return Err(TpCode::CapBad.val());
    }
    if caps & CAP_BUTTONPAD != 0 && caps & CAP_PHYS_BUTTONS != 0 {
        return Err(TpCode::CapBad.val());
    }
    if caps & (CAP_MT | CAP_SEMI_MT) == 0 {
        return Err(TpCode::CapBad.val());
    }
    if caps & CAP_MT != 0 && caps & CAP_SEMI_MT != 0 {
        return Err(TpCode::CapBad.val());
    }
    Ok(caps)
}

// ── 分类 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PadClass {
    /// 一体式按压面（软键区虚拟左右键）
    Clickpad,
    /// 独立物理按键
    Physical,
    /// 混合：物理键 + 软键区（少量二合一机型）
    Hybrid,
}

impl PadClass {
    pub fn text(self) -> &'static str {
        match self {
            PadClass::Clickpad => "clickpad",
            PadClass::Physical => "physical",
            PadClass::Hybrid => "hybrid",
        }
    }

    pub fn from_ord(n: u8) -> Result<PadClass, u32> {
        match n {
            1 => Ok(PadClass::Clickpad),
            2 => Ok(PadClass::Physical),
            3 => Ok(PadClass::Hybrid),
            _ => Err(TpCode::ClassBad.val()),
        }
    }
}

/// 分类：BUTTONPAD + 压力 → clickpad；纯物理键 → physical；
/// 两者同时具备（厂商上报双能力）→ hybrid。
pub fn classify(caps: u16) -> Result<PadClass, u32> {
    let c = audit_caps(caps)?;
    let pad = c & CAP_BUTTONPAD != 0;
    let phys = c & CAP_PHYS_BUTTONS != 0;
    if pad && phys {
        return Err(TpCode::CapBad.val());
    }
    if pad {
        if c & CAP_PRESSURE == 0 {
            return Err(TpCode::ClassBad.val());
        }
        return Ok(PadClass::Clickpad);
    }
    if phys {
        return Ok(PadClass::Physical);
    }
    // 无按键能力：MT 设备按混合面处理（无软键、仅手势面）。
    Ok(PadClass::Hybrid)
}

// ── 软键区几何 ────────────────────────────────────────────────────
/// 软键区：以宽度为基准，左右段各取宽度的 left/right 百分比，
/// 中段占剩余；hys 为迟滞带（像素）。所有百分比用千分比。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ZoneSpec {
    pub width: u32,
    pub left_permille: u32,
    pub right_permille: u32,
    pub hys: u32,
}

impl ZoneSpec {
    pub fn new(width: u32, left_permille: u32, right_permille: u32, hys: u32) -> ZoneSpec {
        ZoneSpec { width, left_permille, right_permille, hys }
    }

    /// 三段都有非零宽度、左右和不越 1000、迟滞小于最窄段。
    pub fn valid(&self) -> bool {
        if self.width < ZONE_COUNT as u32 {
            return false;
        }
        if self.left_permille == 0 || self.right_permille == 0 {
            return false;
        }
        let sum = self.left_permille.saturating_add(self.right_permille);
        if sum >= 1000 {
            return false;
        }
        let left_px = self.width.saturating_mul(self.left_permille) / 1000;
        let right_px = self.width.saturating_mul(self.right_permille) / 1000;
        let mid_px = self.width.saturating_sub(left_px).saturating_sub(right_px);
        let narrow = left_px.min(right_px).min(mid_px);
        if narrow == 0 {
            return false;
        }
        self.hys < narrow
    }

    pub fn left_px(&self) -> u32 {
        self.width.saturating_mul(self.left_permille) / 1000
    }

    pub fn right_px(&self) -> u32 {
        self.width.saturating_mul(self.right_permille) / 1000
    }

    pub fn mid_px(&self) -> u32 {
        self.width
            .saturating_sub(self.left_px())
            .saturating_sub(self.right_px())
    }

    pub fn text(&self) -> String {
        format!(
            "w={} L={} M={} R={} hys={}",
            self.width,
            self.left_px(),
            self.mid_px(),
            self.right_px(),
            self.hys
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoftButton {
    Left,
    Middle,
    Right,
    None,
}

impl SoftButton {
    pub fn text(self) -> &'static str {
        match self {
            SoftButton::Left => "L",
            SoftButton::Middle => "M",
            SoftButton::Right => "R",
            SoftButton::None => "-",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            SoftButton::Left => 1,
            SoftButton::Middle => 2,
            SoftButton::Right => 3,
            SoftButton::None => 0,
        }
    }
}

/// 无迟滞的窄区判定；超出 width 一律 None（不夹取）。
pub fn zone_raw(spec: &ZoneSpec, x: u32) -> Result<SoftButton, u32> {
    if !spec.valid() {
        return Err(TpCode::ZoneBad.val());
    }
    if x >= spec.width {
        return Err(TpCode::ZoneBad.val());
    }
    let l = spec.left_px();
    let m_end = l.saturating_add(spec.mid_px());
    if x < l {
        return Ok(SoftButton::Left);
    }
    if x < m_end {
        return Ok(SoftButton::Middle);
    }
    Ok(SoftButton::Right)
}

/// 带迟滞的判定：只有越过「旧区边界 ± hys」才换区。
///
/// 迟滞只作用于迁移带，不改变区间所有权：离开旧区后若落入新区的
/// 迟滞带，仍保留旧键；直到越过新键中心侧才提交。这样在边界来回
/// 抖动 1px 时不会连续切换。
pub fn zone_with_hysteresis(
    spec: &ZoneSpec,
    x: u32,
    previous: SoftButton,
) -> Result<SoftButton, u32> {
    let raw = zone_raw(spec, x)?;
    if previous == SoftButton::None || previous == raw {
        return Ok(raw);
    }
    if near_boundary(spec, x, previous) {
        return Ok(previous);
    }
    Ok(raw)
}

fn near_boundary(spec: &ZoneSpec, x: u32, previous: SoftButton) -> bool {
    let l = spec.left_px();
    let m_end = l.saturating_add(spec.mid_px());
    let h = spec.hys;
    match previous {
        SoftButton::Left => x <= l.saturating_add(h),
        SoftButton::Middle => {
            x.saturating_add(h) >= l && x <= m_end.saturating_add(h)
        }
        SoftButton::Right => x.saturating_add(h) >= m_end,
        SoftButton::None => false,
    }
}

// ── 压力语义 ──────────────────────────────────────────────────────
/// 压力语义：raw 量程 [0, max]，低阈以下为 Hover，高阈以上为 Press，
/// 中间按迟滞保留上一态。归一值 0..=1000。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PressureSpec {
    pub max: u32,
    pub low: u32,
    pub high: u32,
}

impl PressureSpec {
    pub fn new(max: u32, low: u32, high: u32) -> PressureSpec {
        PressureSpec { max, low, high }
    }

    pub fn valid(&self) -> bool {
        self.max > 0 && self.max <= 0xFFFF && self.low < self.high && self.high <= self.max
    }

    pub fn text(&self) -> String {
        format!("max={} low={} high={}", self.max, self.low, self.high)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressureState {
    Hover,
    Contact,
    Press,
}

impl PressureState {
    pub fn text(self) -> &'static str {
        match self {
            PressureState::Hover => "hover",
            PressureState::Contact => "contact",
            PressureState::Press => "press",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            PressureState::Hover => 0,
            PressureState::Contact => 1,
            PressureState::Press => 2,
        }
    }
}

/// 压力归一：0 → 0，max → 1000，四舍五入。
pub fn normalize_pressure(raw: u32, spec: &PressureSpec) -> Result<u16, u32> {
    if !spec.valid() {
        return Err(TpCode::PressureBad.val());
    }
    if raw > spec.max {
        return Err(TpCode::PressureBad.val());
    }
    let scaled = raw.saturating_mul(PRESSURE_FULL as u32);
    let rounded = scaled.saturating_add(spec.max / 2);
    Ok((rounded / spec.max).min(PRESSURE_FULL as u32) as u16)
}

/// 压力三态（带迟滞）：低于 low 必 Hover，高于 high 必 Press，
/// 中间保留上一态；初始态为 Hover。
pub fn pressure_state(
    raw: u32,
    spec: &PressureSpec,
    previous: PressureState,
) -> Result<PressureState, u32> {
    if !spec.valid() {
        return Err(TpCode::PressureBad.val());
    }
    if raw > spec.max {
        return Err(TpCode::PressureBad.val());
    }
    if raw < spec.low {
        return Ok(PressureState::Hover);
    }
    if raw >= spec.high {
        return Ok(PressureState::Press);
    }
    if previous == PressureState::Press {
        return Ok(PressureState::Press);
    }
    Ok(PressureState::Contact)
}

// ── 设备档 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PadProfile {
    pub vid: u16,
    pub pid: u16,
    pub fw: u32,
    pub caps: u16,
    pub class: PadClass,
    pub zone: ZoneSpec,
    pub pressure: PressureSpec,
}

impl PadProfile {
    pub fn new(
        vid: u16,
        pid: u16,
        fw: u32,
        caps: u16,
        zone: ZoneSpec,
        pressure: PressureSpec,
    ) -> Result<PadProfile, u32> {
        if vid == 0 || pid == 0 || fw == 0 {
            return Err(TpCode::DevBad.val());
        }
        let class = classify(caps)?;
        if !zone.valid() {
            return Err(TpCode::ZoneBad.val());
        }
        if !pressure.valid() {
            return Err(TpCode::PressureBad.val());
        }
        Ok(PadProfile { vid, pid, fw, caps, class, zone, pressure })
    }

    pub fn text(&self) -> String {
        format!(
            "{:04X}:{:04X} fw{} {} {} {}",
            self.vid,
            self.pid,
            self.fw,
            self.class.text(),
            self.zone.text(),
            self.pressure.text()
        )
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fold_u32(h, self.vid as u32);
        h = fold_u32(h, self.pid as u32);
        h = fold_u32(h, self.fw);
        h = fold_u32(h, self.caps as u32);
        h = fold_u32(h, self.class.code());
        h = fold_u32(h, self.zone.width);
        h = fold_u32(h, self.zone.left_permille);
        h = fold_u32(h, self.zone.right_permille);
        h = fold_u32(h, self.zone.hys);
        h = fold_u32(h, self.pressure.max);
        h = fold_u32(h, self.pressure.low);
        h = fold_u32(h, self.pressure.high);
        h
    }
}

impl PadClass {
    pub fn code(self) -> u32 {
        match self {
            PadClass::Clickpad => 1,
            PadClass::Physical => 2,
            PadClass::Hybrid => 3,
        }
    }
}

// ── FNV-1a ────────────────────────────────────────────────────────
pub const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, (v >> (i * 8)) as u8);
        i += 1;
    }
    x
}

// ── LCG ───────────────────────────────────────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 三类设备语料 ──────────────────────────────────────────────────
/// (vid, pid, fw, caps) 三类代表：clickpad / physical / hybrid。
pub const CLASS_CORPUS: [(u16, u16, u32, u16); 3] = [
    (0x06CB, 0x7E7E, 1, CAP_BUTTONPAD | CAP_PRESSURE | CAP_MT | CAP_HIRES),
    (0x046D, 0xC52B, 2, CAP_PHYS_BUTTONS | CAP_MT | CAP_PALM),
    (0x04F3, 0x3140, 3, CAP_MT | CAP_HIRES | CAP_PRESSURE),
];

/// 分类黄金：三类各 1000 次分类，返回 (clickpad, physical, hybrid)。
pub fn golden_classify(rounds: u32) -> Result<(u32, u32, u32), u32> {
    let mut click = 0u32;
    let mut phys = 0u32;
    let mut hybrid = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let mut i = 0usize;
        while i < CLASS_CORPUS.len() {
            let (_, _, _, caps) = match CLASS_CORPUS.get(i) {
                Some(v) => *v,
                None => return Err(TpCode::SegmentBad.val()),
            };
            match classify(caps)? {
                PadClass::Clickpad => click = click.saturating_add(1),
                PadClass::Physical => phys = phys.saturating_add(1),
                PadClass::Hybrid => hybrid = hybrid.saturating_add(1),
            }
            i += 1;
        }
        r = r.wrapping_add(1);
    }
    if click != rounds || phys != rounds || hybrid != rounds {
        return Err(TpCode::ClassBad.val());
    }
    Ok((click, phys, hybrid))
}

/// 软键黄金：1000 轮扫过 0..width，迟滞路径与窄区一致。
///
/// 迟滞必须「只在边界带保留旧键」，因此对每个采样点做两遍：
/// 一遍带上一键、一遍不带；无旧键时必须等于窄区结果。
pub fn golden_zone(rounds: u32) -> Result<(u32, u32), u32> {
    let spec = ZoneSpec::new(3200, 300, 300, 24);
    if !spec.valid() {
        return Err(TpCode::ZoneBad.val());
    }
    let l = spec.left_px();
    let m_end = l.saturating_add(spec.mid_px());
    let mut decisions = 0u32;
    let mut keeps = 0u32;
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x5A);
    let mut prev = SoftButton::None;
    let mut r = 0u32;
    while r < rounds {
        // 每四轮固定打一次边界带，其余随机扫；边界带保证迟滞真实被走到，
        // 随机扫保证不把黄金链写成四条常量。
        let x = match r % 4 {
            0 => l.saturating_add(1),
            1 => l.saturating_sub(1),
            2 => m_end.saturating_sub(1),
            3 => m_end.saturating_add(1),
            _ => rng.below(spec.width),
        };
        let raw = zone_raw(&spec, x)?;
        let held = zone_with_hysteresis(&spec, x, prev)?;
        if prev == SoftButton::None && held != raw {
            return Err(TpCode::ReplayBad.val());
        }
        if held == prev && held != raw {
            keeps = keeps.saturating_add(1);
        }
        decisions = decisions.saturating_add(1);
        prev = held;
        r = r.wrapping_add(1);
    }
    if decisions != rounds || keeps == 0 {
        return Err(TpCode::ReplayBad.val());
    }
    Ok((decisions, keeps))
}

/// 压力黄金：1000 轮全量程扫描，三态单调不回跳。
pub fn golden_pressure(rounds: u32) -> Result<(u32, u16), u32> {
    let spec = PressureSpec::new(4095, 300, 2200);
    if !spec.valid() {
        return Err(TpCode::PressureBad.val());
    }
    let mut samples = 0u32;
    let mut sum = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let raw = r % (spec.max.saturating_add(1));
        let n = normalize_pressure(raw, &spec)?;
        let st = pressure_state(raw, &spec, PressureState::Hover)?;
        if raw < spec.low && st != PressureState::Hover {
            return Err(TpCode::PressureBad.val());
        }
        if raw >= spec.high && st != PressureState::Press {
            return Err(TpCode::PressureBad.val());
        }
        sum = sum.saturating_add(n as u32);
        samples = samples.saturating_add(1);
        r = r.wrapping_add(1);
    }
    let avg = if samples == 0 { 0 } else { (sum / samples) as u16 };
    Ok((samples, avg))
}

/// 黄金全链：(分类三元组, 区判定数, 压力样本数, 指纹)。
pub fn golden_run() -> Result<((u32, u32, u32), u32, u32, u64), u32> {
    let cls = golden_classify(CLICK_ROUNDS)?;
    let (zd, keeps) = golden_zone(CLICK_ROUNDS)?;
    let (ps, _avg) = golden_pressure(PRESSURE_ROUNDS)?;
    let mut profile_ok = 0u32;
    let mut i = 0usize;
    while i < CLASS_CORPUS.len() {
        let (vid, pid, fw, caps) = match CLASS_CORPUS.get(i) {
            Some(v) => *v,
            None => return Err(TpCode::SegmentBad.val()),
        };
        let zone = ZoneSpec::new(3200, 300 + i as u32 * 10, 300, 24);
        let press = PressureSpec::new(4095, 300, 2200);
        let p = PadProfile::new(vid, pid, fw, caps, zone, press)?;
        if p.vid != vid || p.class != classify(caps)? {
            return Err(TpCode::ReplayBad.val());
        }
        profile_ok = profile_ok.saturating_add(1);
        i += 1;
    }
    let mut h = FNV_OFFSET;
    h = fold_u32(h, cls.0);
    h = fold_u32(h, cls.1);
    h = fold_u32(h, cls.2);
    h = fold_u32(h, zd);
    h = fold_u32(h, keeps);
    h = fold_u32(h, ps);
    h = fold_u32(h, profile_ok);
    Ok((cls, zd, ps, h))
}

// ── 注入账（四路：坏能力 / 坏区 / 坏压力 / 禁用后受理） ──────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x49);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：未知能力位。
        let bad_caps = audit_caps(CAP_MT | 0x8000).err() == Some(TpCode::CapBad.val())
            && audit_caps(CAP_BUTTONPAD | CAP_PHYS_BUTTONS | CAP_MT).err()
                == Some(TpCode::CapBad.val());
        led.record(0, bad_caps);

        // 路 2：区宽为 0 / 左右和越界 / 迟滞吞段。
        let z0 = ZoneSpec::new(100, 0, 300, 1);
        let z1 = ZoneSpec::new(100, 600, 500, 1);
        let z2 = ZoneSpec::new(100, 400, 400, 30);
        let bad_zone = !z0.valid() && !z1.valid() && !z2.valid()
            && zone_raw(&z0, 0).err() == Some(TpCode::ZoneBad.val());
        led.record(1, bad_zone);

        // 路 3：阈值次序颠倒 / 超量程。
        let p0 = PressureSpec::new(100, 60, 40);
        let p1 = PressureSpec::new(100, 10, 101);
        let bad_press = !p0.valid()
            && !p1.valid()
            && normalize_pressure(10, &p0).err() == Some(TpCode::PressureBad.val())
            && pressure_state(10, &p1, PressureState::Hover).is_err();
        led.record(2, bad_press);

        // 路 4：禁用后仍递交坐标。
        let mut sw = PadSwitch::new(0, true);
        sw.disable(11);
        let swallowed = !sw.enabled() && sw.accept(11, 100).err() == Some(TpCode::SwitchBad.val());
        led.record(3, swallowed);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 开关闸 ────────────────────────────────────────────────────────
/// 触控板开关：禁用后只接受「重新配置」不接受坐标；启用序号单调。
pub struct PadSwitch {
    device: u16,
    enabled: bool,
    seq: u32,
}

impl PadSwitch {
    pub fn new(device: u16, enabled: bool) -> PadSwitch {
        PadSwitch { device, enabled, seq: 0 }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    pub fn device(&self) -> u16 {
        self.device
    }

    pub fn enable(&mut self, seq: u32) -> Result<(), u32> {
        if seq <= self.seq {
            return Err(TpCode::SwitchBad.val());
        }
        self.seq = seq;
        self.enabled = true;
        Ok(())
    }

    pub fn disable(&mut self, seq: u32) {
        if seq > self.seq {
            self.seq = seq;
        }
        self.enabled = false;
    }

    pub fn accept(&self, seq: u32, _x: u32) -> Result<(), u32> {
        if !self.enabled {
            return Err(TpCode::SwitchBad.val());
        }
        if seq != self.seq {
            return Err(TpCode::ReplayBad.val());
        }
        Ok(())
    }
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (cls, zd, ps, _) = golden_run()?;
    if cls == (CLICK_ROUNDS, CLICK_ROUNDS, CLICK_ROUNDS) && zd == CLICK_ROUNDS && ps == PRESSURE_ROUNDS {
        Ok(())
    } else {
        Err(TpCode::ReplayBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    if classify(CAP_MT) != Ok(PadClass::Hybrid) {
        return Err(TpCode::ClassBad.val());
    }
    if zone_raw(&ZoneSpec::new(100, 300, 300, 1), 100).err() != Some(TpCode::ZoneBad.val()) {
        return Err(TpCode::ZoneBad.val());
    }
    if pad_roundtrip().is_err() {
        return Err(TpCode::ReplayBad.val());
    }
    Ok(())
}

fn pad_roundtrip() -> Result<(), u32> {
    let spec = ZoneSpec::new(3200, 300, 300, 24);
    let p = PadProfile::new(1, 2, 3, CAP_BUTTONPAD | CAP_PRESSURE | CAP_MT, spec,
        PressureSpec::new(4095, 300, 2200))?;
    let a = p.fingerprint();
    let b = p.fingerprint();
    if a != b {
        return Err(TpCode::ReplayBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if CLASS_CASES == 3
        && CLICK_ROUNDS == 1000
        && PRESSURE_ROUNDS == 1000
        && ZONE_COUNT == 4
        && PRESSURE_FULL == 1000
        && CLASS_CORPUS.len() == 3
    {
        Ok(())
    } else {
        Err(TpCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TpCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < TpCode::ALL.len() {
        let v = TpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let cls = golden_classify(CLICK_ROUNDS);
    v.push((
        "F48901-J1-三类判别千轮",
        match &cls {
            Ok((a, b, c)) => *a == CLICK_ROUNDS && *b == CLICK_ROUNDS && *c == CLICK_ROUNDS,
            Err(_) => false,
        },
        match &cls {
            Ok((a, b, c)) => format!("click={} phys={} hybrid={}", a, b, c),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let zone = golden_zone(CLICK_ROUNDS);
    v.push((
        "F48902-J1-软键迟滞千轮",
        match &zone {
            Ok((n, keeps)) => *n == CLICK_ROUNDS && *keeps > 0,
            Err(_) => false,
        },
        match &zone {
            Ok((n, keeps)) => format!("decisions={} held={}", n, keeps),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let press = golden_pressure(PRESSURE_ROUNDS);
    v.push((
        "F48902-J1-压力三态千轮",
        match &press {
            Ok((n, avg)) => *n == PRESSURE_ROUNDS && *avg > 0,
            Err(_) => false,
        },
        match &press {
            Ok((n, avg)) => format!("samples={} avg={}", n, avg),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let caps = audit_caps(CAP_MT | 0x8000);
    v.push((
        "F48901-J1-未知能力位必拒",
        caps.err() == Some(TpCode::CapBad.val()),
        format!("audit={:?}", caps.err()),
    ));
    let mutual = audit_caps(CAP_BUTTONPAD | CAP_PHYS_BUTTONS | CAP_MT);
    v.push((
        "F48901-J1-按键面互斥",
        mutual.err() == Some(TpCode::CapBad.val()),
        format!("audit={:?}", mutual.err()),
    ));
    let no_press = classify(CAP_BUTTONPAD | CAP_MT);
    v.push((
        "F48901-J1-clickpad缺压力拒",
        no_press.err() == Some(TpCode::ClassBad.val()),
        format!("classify={:?}", no_press.err()),
    ));
    let hys = zone_with_hysteresis(&ZoneSpec::new(3200, 300, 300, 24), 970, SoftButton::Left);
    v.push((
        "F48902-J1-边界带保旧键",
        hys == Ok(SoftButton::Left),
        format!("held={:?}", hys),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48901-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48901-J1-探针三序与双跑",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48901-J1-段位守卫",
        segment_guard(),
        String::from("0x70C80 窗口"),
    ));
    v
}

pub fn run_unx48901_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48901 触控板分类与软键区几何");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_classes_are_distinct() {
        assert_eq!(classify(CAP_BUTTONPAD | CAP_PRESSURE | CAP_MT), Ok(PadClass::Clickpad));
        assert_eq!(classify(CAP_PHYS_BUTTONS | CAP_MT), Ok(PadClass::Physical));
        assert_eq!(classify(CAP_MT), Ok(PadClass::Hybrid));
    }

    #[test]
    fn unknown_caps_rejected() {
        assert_eq!(audit_caps(CAP_MT | 0x8000).err(), Some(TpCode::CapBad.val()));
        assert_eq!(
            audit_caps(CAP_BUTTONPAD | CAP_PHYS_BUTTONS | CAP_MT).err(),
            Some(TpCode::CapBad.val())
        );
        assert_eq!(audit_caps(CAP_BUTTONPAD).err(), Some(TpCode::CapBad.val()));
    }

    #[test]
    fn clickpad_needs_pressure() {
        assert_eq!(classify(CAP_BUTTONPAD | CAP_MT).err(), Some(TpCode::ClassBad.val()));
    }

    #[test]
    fn zone_geometry_checks_sum() {
        assert!(ZoneSpec::new(3200, 300, 300, 24).valid());
        assert!(!ZoneSpec::new(3200, 600, 500, 24).valid());
        assert!(!ZoneSpec::new(3200, 0, 300, 1).valid());
        assert!(!ZoneSpec::new(3200, 300, 300, 1000).valid());
    }

    #[test]
    fn zone_boundaries_are_exact() {
        let s = ZoneSpec::new(1000, 300, 300, 10);
        assert_eq!(zone_raw(&s, 299), Ok(SoftButton::Left));
        assert_eq!(zone_raw(&s, 300), Ok(SoftButton::Middle));
        assert_eq!(zone_raw(&s, 699), Ok(SoftButton::Middle));
        assert_eq!(zone_raw(&s, 700), Ok(SoftButton::Right));
        assert_eq!(zone_raw(&s, 1000).err(), Some(TpCode::ZoneBad.val()));
    }

    #[test]
    fn hysteresis_holds_old_key_only_near_boundary() {
        let s = ZoneSpec::new(1000, 300, 300, 30);
        assert_eq!(zone_with_hysteresis(&s, 301, SoftButton::Left), Ok(SoftButton::Left));
        assert_eq!(zone_with_hysteresis(&s, 400, SoftButton::Left), Ok(SoftButton::Middle));
        assert_eq!(zone_with_hysteresis(&s, 500, SoftButton::None), Ok(SoftButton::Middle));
    }

    #[test]
    fn pressure_state_has_dead_zone() {
        let p = PressureSpec::new(1000, 100, 700);
        assert_eq!(pressure_state(50, &p, PressureState::Press), Ok(PressureState::Hover));
        assert_eq!(pressure_state(400, &p, PressureState::Hover), Ok(PressureState::Contact));
        assert_eq!(pressure_state(400, &p, PressureState::Press), Ok(PressureState::Press));
        assert_eq!(pressure_state(700, &p, PressureState::Hover), Ok(PressureState::Press));
    }

    #[test]
    fn pressure_normalization_endpoints() {
        let p = PressureSpec::new(4095, 300, 2200);
        assert_eq!(normalize_pressure(0, &p), Ok(0));
        assert_eq!(normalize_pressure(4095, &p), Ok(1000));
        assert_eq!(normalize_pressure(4096, &p).err(), Some(TpCode::PressureBad.val()));
    }

    #[test]
    fn profile_rejects_bad_identity() {
        let z = ZoneSpec::new(3200, 300, 300, 24);
        let p = PressureSpec::new(4095, 300, 2200);
        assert_eq!(PadProfile::new(0, 1, 1, CAP_MT, z, p).err(), Some(TpCode::DevBad.val()));
        assert_eq!(PadProfile::new(1, 1, 0, CAP_MT, z, p).err(), Some(TpCode::DevBad.val()));
    }

    #[test]
    fn switch_blocks_after_disable() {
        let mut s = PadSwitch::new(1, true);
        assert!(s.accept(0, 0).is_ok());
        s.disable(1);
        assert_eq!(s.accept(1, 0).err(), Some(TpCode::SwitchBad.val()));
        assert!(s.enable(2).is_ok());
        assert!(s.accept(2, 0).is_ok());
        assert!(s.enable(1).is_err());
    }

    #[test]
    fn golden_is_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn injection_catches_all_four() {
        match inject_rounds(10) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [10, 10, 10, 10]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn codes_inside_window() {
        assert!(segment_guard());
        assert_eq!(TpCode::ALL.len(), 8);
    }
}
