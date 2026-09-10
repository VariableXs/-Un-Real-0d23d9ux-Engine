//! AI-11 电源与热管理域（F251~F275）。
//!
//! Sleep states, wake accounting, battery/charge policy, CPU frequency
//! governance, thermal control, power profiles and the domain self-test.
//! Every policy is a pure function or a fixed-capacity structure so the
//! host test suite can exercise the whole domain without hardware; the
//! target only supplies TSC stamps and MMIO writes at the very edge.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F251 — ACPI 电源态 (S0~S5)
// ---------------------------------------------------------------------------

/// PM1x_CNT `SLP_EN` bit (ACPI spec: writing 1 enters the SLP_TYP state).
pub const SLP_EN: u16 = 1 << 13;
/// SLP_TYP field position inside PM1x_CNT (bits 10..12).
pub const SLP_TYP_SHIFT: u16 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerState {
    S0,
    S1,
    S2,
    S3,
    S4,
    S5,
}

impl PowerState {
    pub fn id(self) -> u8 {
        match self {
            PowerState::S0 => 0,
            PowerState::S1 => 1,
            PowerState::S2 => 2,
            PowerState::S3 => 3,
            PowerState::S4 => 4,
            PowerState::S5 => 5,
        }
    }

    pub fn from_id(id: u8) -> Option<PowerState> {
        match id {
            0 => Some(PowerState::S0),
            1 => Some(PowerState::S1),
            2 => Some(PowerState::S2),
            3 => Some(PowerState::S3),
            4 => Some(PowerState::S4),
            5 => Some(PowerState::S5),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PowerState::S0 => "S0",
            PowerState::S1 => "S1",
            PowerState::S2 => "S2",
            PowerState::S3 => "S3",
            PowerState::S4 => "S4",
            PowerState::S5 => "S5",
        }
    }

    /// Memory content survives (S1~S4 keep RAM, S5 does not).
    pub fn keeps_memory(self) -> bool {
        !matches!(self, PowerState::S5)
    }

    /// CPU context survives without restore-from-disk (S1~S3).
    pub fn keeps_context(self) -> bool {
        matches!(self, PowerState::S0 | PowerState::S1 | PowerState::S2 | PowerState::S3)
    }
}

/// Minimal FADT view: only the fields the power domain touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fadt {
    pub pm1a_cnt_blk: u32,
    pub pm1b_cnt_blk: u32,
    pub pm1_cnt_len: u8,
    pub sci_int: u16,
    pub smi_cmd: u32,
    pub profile: u8,
}

/// Parse the fixed fields of a FADT body (header included). Offsets follow
/// ACPI 6.x: SCI_INT@46, SMI_CMD@48, PM1a_CNT_BLK@64, PM1b_CNT_BLK@68,
/// PM1_CNT_LEN@89, PreferredPMProfile@45.
pub fn parse_fadt(table: &[u8]) -> Option<Fadt> {
    if table.len() < 92 {
        return None;
    }
    if &table[0..4] != b"FACP" {
        return None;
    }
    let u32_at = |off: usize| -> u32 {
        u32::from_le_bytes([table[off], table[off + 1], table[off + 2], table[off + 3]])
    };
    Some(Fadt {
        profile: table[45],
        sci_int: u16::from_le_bytes([table[46], table[47]]),
        smi_cmd: u32_at(48),
        pm1a_cnt_blk: u32_at(64),
        pm1b_cnt_blk: u32_at(68),
        pm1_cnt_len: table[89],
    })
}

/// Build the PM1x_CNT value that enters `slp_typ`.
pub fn slp_typ_value(slp_typ: u8) -> u16 {
    (((slp_typ & 0x7) as u16) << SLP_TYP_SHIFT) | SLP_EN
}

/// Registers required to enter a sleep state (F251/F252).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SleepRegs {
    pub pm1a: u32,
    pub pm1b: u32,
    pub slp_typa: u8,
    pub slp_typb: u8,
}

impl SleepRegs {
    pub fn from_fadt(fadt: &Fadt, slp_typa: u8, slp_typb: u8) -> SleepRegs {
        SleepRegs {
            pm1a: fadt.pm1a_cnt_blk,
            pm1b: fadt.pm1b_cnt_blk,
            slp_typa,
            slp_typb,
        }
    }

    /// (PM1a_CNT, PM1b_CNT) values for `state` — b is 0 when absent.
    pub fn value_pair(self, typ_a: u8, typ_b: u8) -> (u16, u16) {
        let a = slp_typ_value(typ_a);
        let b = if self.pm1b != 0 { slp_typ_value(typ_b) } else { 0 };
        (a, b)
    }

    pub fn has_b(self) -> bool {
        self.pm1b != 0
    }
}

// ---------------------------------------------------------------------------
// F252 — 睡眠 S3
// ---------------------------------------------------------------------------

/// Ordered bring-down steps for suspend-to-RAM.
pub const S3_STEPS: [&'static str; 7] = [
    "freeze user tasks",
    "suspend devices",
    "save wake vector",
    "disable non-wake IRQs",
    "flush caches",
    "write PM1x_CNT",
    "wait-for-STS",
];

#[derive(Clone, Copy, Debug)]
pub struct SuspendPlan {
    pub step_count: usize,
    pub wake_vector: u32,
    pub pm1a_value: u16,
    pub pm1b_value: u16,
    pub crc: u32,
}

/// Small CRC-32 (reflected poly 0xEDB88320) used by the power domain for
/// wake-vector and hibernation image headers.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

pub fn plan_s3(regs: SleepRegs, typ_a: u8, typ_b: u8, wake_vector: u32) -> SuspendPlan {
    let (a, b) = regs.value_pair(typ_a, typ_b);
    let mut payload = [0u8; 8];
    payload[0..4].copy_from_slice(&wake_vector.to_le_bytes());
    payload[4..6].copy_from_slice(&a.to_le_bytes());
    payload[6..8].copy_from_slice(&b.to_le_bytes());
    SuspendPlan {
        step_count: S3_STEPS.len(),
        wake_vector,
        pm1a_value: a,
        pm1b_value: b,
        crc: crc32(&payload),
    }
}

// ---------------------------------------------------------------------------
// F253 — 休眠 S4
// ---------------------------------------------------------------------------

pub const HIBERNATION_MAGIC: [u8; 4] = *b"VRXH";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HibernationHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub page_count: u32,
    pub image_bytes: u64,
    pub crc: u32,
    pub flags: u16,
}

impl HibernationHeader {
    pub fn encode(self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out[0..4].copy_from_slice(&self.magic);
        out[4..6].copy_from_slice(&self.version.to_le_bytes());
        out[6..10].copy_from_slice(&self.page_count.to_le_bytes());
        out[10..18].copy_from_slice(&self.image_bytes.to_le_bytes());
        out[18..22].copy_from_slice(&self.crc.to_le_bytes());
        out[22..24].copy_from_slice(&self.flags.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<HibernationHeader> {
        if bytes.len() < 32 {
            return None;
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&bytes[0..4]);
        if magic != HIBERNATION_MAGIC {
            return None;
        }
        let mut page_count = [0u8; 4];
        page_count.copy_from_slice(&bytes[6..10]);
        let mut image_bytes = [0u8; 8];
        image_bytes.copy_from_slice(&bytes[10..18]);
        let mut crc = [0u8; 4];
        crc.copy_from_slice(&bytes[18..22]);
        let mut flags = [0u8; 2];
        flags.copy_from_slice(&bytes[22..24]);
        Some(HibernationHeader {
            magic,
            version: u16::from_le_bytes([bytes[4], bytes[5]]),
            page_count: u32::from_le_bytes(page_count),
            image_bytes: u64::from_le_bytes(image_bytes),
            crc: u32::from_le_bytes(crc),
            flags: u16::from_le_bytes(flags),
        })
    }

    pub fn valid(self) -> bool {
        self.magic == HIBERNATION_MAGIC && self.version >= 1 && self.page_count > 0
    }
}

/// Backing store needed for an image: pages + 8% slack + header.
pub fn required_storage_bytes(total_pages: u64, page_size: u64) -> u64 {
    let image = total_pages.saturating_mul(page_size);
    image.saturating_add(image / 8).saturating_add(4096)
}

pub fn can_hibernate(free_bytes: u64, needed: u64) -> bool {
    free_bytes >= needed && needed > 0
}

pub fn build_hibernation_header(pages: u32, page_size: u64, crc: u32) -> HibernationHeader {
    HibernationHeader {
        magic: HIBERNATION_MAGIC,
        version: 1,
        page_count: pages,
        image_bytes: (pages as u64).saturating_mul(page_size),
        crc,
        flags: 0,
    }
}

// ---------------------------------------------------------------------------
// F254 — 唤醒源管理
// ---------------------------------------------------------------------------

pub const MAX_WAKE_SOURCES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeKind {
    PowerButton = 0,
    Lid = 1,
    Keyboard = 2,
    Mouse = 3,
    Rtc = 4,
    Nic = 5,
    Usb = 6,
}

impl WakeKind {
    pub fn mask_bit(self) -> u32 {
        1u32 << (self as u32)
    }

    pub fn from_index(i: usize) -> Option<WakeKind> {
        match i {
            0 => Some(WakeKind::PowerButton),
            1 => Some(WakeKind::Lid),
            2 => Some(WakeKind::Keyboard),
            3 => Some(WakeKind::Mouse),
            4 => Some(WakeKind::Rtc),
            5 => Some(WakeKind::Nic),
            6 => Some(WakeKind::Usb),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WakeSource {
    pub name: &'static str,
    pub kind: WakeKind,
    pub gsi: u8,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct WakeRegistry {
    sources: [Option<WakeSource>; MAX_WAKE_SOURCES],
    count: usize,
}

impl WakeRegistry {
    pub const fn new() -> WakeRegistry {
        WakeRegistry {
            sources: [None; MAX_WAKE_SOURCES],
            count: 0,
        }
    }

    pub fn register(&mut self, source: WakeSource) -> bool {
        if self.count >= MAX_WAKE_SOURCES {
            return false;
        }
        self.sources[self.count] = Some(source);
        self.count += 1;
        true
    }

    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        for i in 0..self.count {
            if let Some(mut s) = self.sources[i] {
                if s.name == name {
                    s.enabled = enabled;
                    self.sources[i] = Some(s);
                    return true;
                }
            }
        }
        false
    }

    pub fn mask(&self) -> u32 {
        let mut m = 0u32;
        for i in 0..self.count {
            if let Some(s) = self.sources[i] {
                if s.enabled {
                    m |= s.kind.mask_bit();
                }
            }
        }
        m
    }

    pub fn enabled_count(&self) -> usize {
        (0..self.count)
            .filter(|i| self.sources[*i].map(|s| s.enabled).unwrap_or(false))
            .count()
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

pub fn should_wake(mask: u32, kind: WakeKind) -> bool {
    mask & kind.mask_bit() != 0
}

// ---------------------------------------------------------------------------
// F255 — 快速唤醒
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct WakeBudget {
    pub firmware_ms: u32,
    pub device_ms: u32,
    pub display_ms: u32,
    /// Hand-feel red line: screen content must be live within this.
    pub redline_ms: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeVerdict {
    Fast,
    Nominal,
    Slow,
}

impl WakeBudget {
    pub fn total_ms(&self) -> u32 {
        self.firmware_ms + self.device_ms + self.display_ms
    }

    /// 0..100 score against the red line (100 = half the red line or better).
    pub fn score(&self) -> u8 {
        let total = self.total_ms().max(1);
        let redline = self.redline_ms.max(1);
        let ratio = (redline as u64) * 100 / (total as u64 * 2);
        ratio.min(100) as u8
    }
}

pub fn wake_verdict(measured_ms: u32, redline_ms: u32) -> WakeVerdict {
    if measured_ms <= redline_ms / 2 {
        WakeVerdict::Fast
    } else if measured_ms <= redline_ms {
        WakeVerdict::Nominal
    } else {
        WakeVerdict::Slow
    }
}

// ---------------------------------------------------------------------------
// F256 — 电池仪表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryState {
    pub present: bool,
    pub charge_now_mah: u32,
    pub charge_full_mah: u32,
    pub design_mah: u32,
    pub voltage_mv: u32,
    /// Positive = charging, negative = discharging.
    pub current_ma: i32,
}

impl BatteryState {
    /// State of charge in percent (clamped 0..100).
    pub fn soc_percent(&self) -> u8 {
        if !self.present || self.charge_full_mah == 0 {
            return 0;
        }
        let pct = (self.charge_now_mah as u64 * 100) / self.charge_full_mah as u64;
        pct.min(100) as u8
    }

    pub fn health_percent(&self) -> u8 {
        if self.design_mah == 0 {
            return 0;
        }
        let pct = (self.charge_full_mah as u64 * 100) / self.design_mah as u64;
        pct.min(100) as u8
    }

    pub fn is_charging(&self) -> bool {
        self.current_ma > 0
    }

    /// Minutes until empty/full, or `None` when idle/unknown.
    pub fn minutes_remaining(&self) -> Option<u32> {
        if self.current_ma == 0 || !self.present || self.charge_full_mah == 0 {
            return None;
        }
        let remaining = if self.current_ma < 0 {
            self.charge_now_mah as u64
        } else {
            self.charge_full_mah.saturating_sub(self.charge_now_mah) as u64
        };
        let ma = self.current_ma.unsigned_abs() as u64;
        if ma == 0 {
            return None;
        }
        Some(((remaining * 60) / ma).min(u32::MAX as u64) as u32)
    }

    /// Instantaneous power draw in milliwatts (positive = charging in).
    pub fn milliwatts(&self) -> i32 {
        let ma = self.current_ma;
        ((ma as i64) * (self.voltage_mv as i64) / 1000) as i32
    }
}

/// Display smoothing: never let the shown value jump more than 1%/tick and
/// never increase while discharging (no phantom charge on the gauge).
pub fn soc_smooth(shown: u8, measured: u8, charging: bool) -> u8 {
    if measured == shown {
        return shown;
    }
    if measured > shown {
        if !charging {
            return shown;
        }
        shown.saturating_add(1)
    } else {
        shown.saturating_sub(1)
    }
}

// ---------------------------------------------------------------------------
// F257/F258 — 电量保养与充电策略
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeThreshold {
    pub start_below: u8,
    pub stop_at: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeAction {
    Charge,
    Hold,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeProfile {
    /// 0~100 always — travel/field use.
    Full,
    /// 40~80 battery care (F257).
    Care,
    /// Docked all day: hold near 60.
    Desk,
    /// Fast to 100 before leaving.
    Travel,
}

pub fn profile_thresholds(profile: ChargeProfile) -> ChargeThreshold {
    match profile {
        ChargeProfile::Full => ChargeThreshold { start_below: 95, stop_at: 100 },
        ChargeProfile::Care => ChargeThreshold { start_below: 40, stop_at: 80 },
        ChargeProfile::Desk => ChargeThreshold { start_below: 50, stop_at: 60 },
        ChargeProfile::Travel => ChargeThreshold { start_below: 90, stop_at: 100 },
    }
}

/// Hysteresis decision: keep the previous state until a threshold is crossed.
pub fn charge_decision(soc: u8, charging: bool, thr: ChargeThreshold) -> ChargeAction {
    if thr.stop_at == 0 || thr.start_below >= thr.stop_at {
        return ChargeAction::Charge;
    }
    if charging {
        if soc >= thr.stop_at {
            ChargeAction::Stop
        } else {
            ChargeAction::Charge
        }
    } else if soc <= thr.start_below {
        ChargeAction::Charge
    } else {
        ChargeAction::Hold
    }
}

/// Current limit in mA: derate with temperature and near-full SoC.
pub fn charge_current_limit_ma(profile: ChargeProfile, temp_c: i8, soc: u8) -> u32 {
    let base = match profile {
        ChargeProfile::Full => 3000,
        ChargeProfile::Care => 2000,
        ChargeProfile::Desk => 1200,
        ChargeProfile::Travel => 4000,
    };
    let mut limit = base;
    if temp_c >= 45 {
        limit /= 4;
    } else if temp_c >= 40 {
        limit /= 2;
    } else if temp_c <= 0 {
        limit /= 4;
    }
    if soc >= 90 {
        limit = limit.min(800);
    } else if soc >= 80 {
        limit = limit.min(1500);
    }
    limit
}

// ---------------------------------------------------------------------------
// F259 — CPU 调频
// ---------------------------------------------------------------------------

pub const MAX_PSTATES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PState {
    pub index: u8,
    pub mhz: u32,
    pub milliwatts: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Governor {
    Performance,
    Powersave,
    Ondemand,
    Schedutil,
}

#[derive(Clone, Copy, Debug)]
pub struct PStateTable {
    states: [PState; MAX_PSTATES],
    count: usize,
}

impl PStateTable {
    pub const fn new() -> PStateTable {
        PStateTable {
            states: [PState { index: 0, mhz: 0, milliwatts: 0 }; MAX_PSTATES],
            count: 0,
        }
    }

    /// Table is stored fastest-first (index 0 = max performance).
    pub fn push(&mut self, state: PState) -> bool {
        if self.count >= MAX_PSTATES {
            return false;
        }
        self.states[self.count] = state;
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, index: usize) -> Option<PState> {
        if index < self.count {
            Some(self.states[index])
        } else {
            None
        }
    }

    pub fn slowest_index(&self) -> u8 {
        self.count.saturating_sub(1) as u8
    }

    /// Pick a P-state index for the measured load (0..1000 permille).
    pub fn pick(&self, load_permille: u16, gov: Governor, current: u8) -> u8 {
        if self.count == 0 {
            return 0;
        }
        let last = self.slowest_index();
        match gov {
            Governor::Performance => 0,
            Governor::Powersave => last,
            Governor::Ondemand => {
                if load_permille >= 800 {
                    0
                } else if load_permille >= 400 {
                    current.saturating_sub(1).min(last)
                } else {
                    (current + 1).min(last)
                }
            }
            Governor::Schedutil => {
                // Utilization maps linearly onto the table.
                let target = ((1000u16.saturating_sub(load_permille)) as u32
                    * (last as u32 + 1)
                    / 1000) as u8;
                target.min(last)
            }
        }
    }

    /// Estimate package power for a P-state at a given utilization.
    pub fn estimate_milliwatts(&self, index: u8, util_permille: u16) -> u32 {
        match self.get(index as usize) {
            Some(p) => {
                let base = p.milliwatts / 4; // idle floor
                base + ((p.milliwatts as u64 * util_permille as u64) / 4000) as u32
            }
            None => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// F260 — 温度传感器
// ---------------------------------------------------------------------------

pub const MAX_SENSORS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThermalSensor {
    pub name: &'static str,
    pub millideg_c: i32,
    pub critical_millideg: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct ThermalZone {
    pub name: &'static str,
    sensors: [Option<ThermalSensor>; MAX_SENSORS],
    count: usize,
}

impl ThermalZone {
    pub const fn new(name: &'static str) -> ThermalZone {
        ThermalZone {
            name,
            sensors: [None; MAX_SENSORS],
            count: 0,
        }
    }

    pub fn upsert(&mut self, sensor: ThermalSensor) {
        for i in 0..self.count {
            if let Some(mut s) = self.sensors[i] {
                if s.name == sensor.name {
                    s.millideg_c = sensor.millideg_c;
                    s.critical_millideg = sensor.critical_millideg;
                    self.sensors[i] = Some(s);
                    return;
                }
            }
        }
        if self.count < MAX_SENSORS {
            self.sensors[self.count] = Some(sensor);
            self.count += 1;
        }
    }

    pub fn max_millideg(&self) -> i32 {
        let mut max = i32::MIN;
        for i in 0..self.count {
            if let Some(s) = self.sensors[i] {
                if s.millideg_c > max {
                    max = s.millideg_c;
                }
            }
        }
        if max == i32::MIN {
            0
        } else {
            max
        }
    }

    pub fn is_critical(&self) -> bool {
        (0..self.count).any(|i| {
            self.sensors[i]
                .map(|s| s.millideg_c >= s.critical_millideg)
                .unwrap_or(false)
        })
    }

    pub fn hottest(&self) -> Option<&'static str> {
        let max = self.max_millideg();
        for i in 0..self.count {
            if let Some(s) = self.sensors[i] {
                if s.millideg_c == max {
                    return Some(s.name);
                }
            }
        }
        None
    }
}

pub fn millideg_to_c(millideg: i32) -> i32 {
    millideg / 1000
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThermalVerdict {
    Cool,
    Warm,
    Hot,
    Critical,
}

pub fn thermal_verdict(millideg: i32, passive_millideg: i32, critical_millideg: i32) -> ThermalVerdict {
    if millideg >= critical_millideg {
        ThermalVerdict::Critical
    } else if millideg >= critical_millideg - 5000 {
        ThermalVerdict::Hot
    } else if millideg >= passive_millideg {
        ThermalVerdict::Warm
    } else {
        ThermalVerdict::Cool
    }
}

// ---------------------------------------------------------------------------
// F261 — 风扇曲线
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FanCurvePoint {
    pub temp_c: i8,
    pub duty_percent: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct FanCurve {
    points: [FanCurvePoint; MAX_SENSORS],
    count: usize,
    /// Minimum duty change before the fan reacts (hysteresis / no hunting).
    pub hysteresis_c: i8,
}

impl FanCurve {
    pub const fn new() -> FanCurve {
        FanCurve {
            points: [FanCurvePoint { temp_c: 0, duty_percent: 0 }; MAX_SENSORS],
            count: 0,
            hysteresis_c: 3,
        }
    }

    /// Points may be added in any order; lookup sorts on the fly.
    pub fn push(&mut self, point: FanCurvePoint) -> bool {
        if self.count >= MAX_SENSORS || point.duty_percent > 100 {
            return false;
        }
        self.points[self.count] = point;
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// Duty for `temp_c`, interpolated between the two nearest points and
    /// held steady inside the hysteresis band around `current_duty`.
    pub fn duty(&self, temp_c: i8, current_duty: u8) -> u8 {
        if self.count == 0 {
            return 0;
        }
        let target = self.raw_duty(temp_c);
        if target.abs_diff(current_duty) < self.hysteresis_c as u8 {
            current_duty
        } else {
            target
        }
    }

    fn raw_duty(&self, temp_c: i8) -> u8 {
        let mut lo: Option<FanCurvePoint> = None;
        let mut hi: Option<FanCurvePoint> = None;
        for i in 0..self.count {
            let p = self.points[i];
            if p.temp_c <= temp_c && (lo.is_none() || p.temp_c > lo.unwrap().temp_c) {
                lo = Some(p);
            }
            if p.temp_c >= temp_c && (hi.is_none() || p.temp_c < hi.unwrap().temp_c) {
                hi = Some(p);
            }
        }
        match (lo, hi) {
            (Some(l), Some(h)) if l.temp_c == h.temp_c => l.duty_percent,
            (Some(l), Some(h)) => {
                let span = (h.temp_c - l.temp_c) as i32;
                let off = (temp_c - l.temp_c) as i32;
                let d = l.duty_percent as i32
                    + ((h.duty_percent as i32 - l.duty_percent as i32) * off) / span;
                d.clamp(0, 100) as u8
            }
            (Some(l), None) => l.duty_percent,
            (None, Some(h)) => h.duty_percent,
            (None, None) => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// F262 — 过热降频
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThermalMitigation {
    pub passive_c: i8,
    pub hot_c: i8,
    pub critical_c: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MitigationLevel {
    None,
    Passive,
    Heavy,
    Critical,
}

pub fn mitigation_level(temp_c: i8, m: ThermalMitigation) -> MitigationLevel {
    if temp_c >= m.critical_c {
        MitigationLevel::Critical
    } else if temp_c >= m.hot_c {
        MitigationLevel::Heavy
    } else if temp_c >= m.passive_c {
        MitigationLevel::Passive
    } else {
        MitigationLevel::None
    }
}

/// Cap the P-state index (table is fastest-first) for a mitigation level.
pub fn capped_pstate(current: u8, level: MitigationLevel, slowest: u8) -> u8 {
    let cap = match level {
        MitigationLevel::None => current.min(slowest),
        MitigationLevel::Passive => 2u8.min(slowest),
        MitigationLevel::Heavy => 4u8.min(slowest),
        MitigationLevel::Critical => slowest,
    };
    current.max(cap).min(slowest)
}

// ---------------------------------------------------------------------------
// F263 — 电源按钮事件
// ---------------------------------------------------------------------------

pub const LONG_PRESS_MS: u32 = 1500;
pub const FORCED_OFF_MS: u32 = 8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerButtonEvent {
    None,
    ShortPress,
    LongPress,
    ForcedOff,
}

#[derive(Clone, Copy, Debug)]
pub struct PowerButtonState {
    pub held: bool,
    pub hold_ms: u32,
    long_fired: bool,
    forced_fired: bool,
}

impl PowerButtonState {
    pub const fn new() -> PowerButtonState {
        PowerButtonState { held: false, hold_ms: 0, long_fired: false, forced_fired: false }
    }

    /// Advance the debounced button state machine by `dt_ms`.
    pub fn tick(&mut self, pressed: bool, dt_ms: u32) -> PowerButtonEvent {
        if pressed {
            self.hold_ms = self.hold_ms.saturating_add(dt_ms);
            self.held = true;
            if !self.forced_fired && self.hold_ms >= FORCED_OFF_MS {
                self.forced_fired = true;
                return PowerButtonEvent::ForcedOff;
            }
            if !self.long_fired && self.hold_ms >= LONG_PRESS_MS {
                self.long_fired = true;
                return PowerButtonEvent::LongPress;
            }
            return PowerButtonEvent::None;
        }
        if !self.held {
            return PowerButtonEvent::None;
        }
        let was_long = self.long_fired;
        self.held = false;
        self.hold_ms = 0;
        self.long_fired = false;
        self.forced_fired = false;
        if was_long {
            PowerButtonEvent::None
        } else {
            PowerButtonEvent::ShortPress
        }
    }
}

// ---------------------------------------------------------------------------
// F264 — 合盖事件
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LidState {
    Open,
    Closed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LidAction {
    Nothing,
    BlankScreen,
    Suspend,
    Hibernate,
}

pub fn lid_action(lid: LidState, on_battery: bool, external_display: bool) -> LidAction {
    match lid {
        LidState::Open | LidState::Unknown => LidAction::Nothing,
        LidState::Closed => {
            if external_display {
                LidAction::BlankScreen
            } else if on_battery {
                LidAction::Suspend
            } else {
                LidAction::Suspend
            }
        }
    }
}

// ---------------------------------------------------------------------------
// F265 — 低电量应急
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LowBatteryPolicy {
    pub warn_percent: u8,
    pub critical_percent: u8,
    pub hibernate_percent: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LowAction {
    Normal,
    Warn,
    Critical,
    Hibernate,
}

impl Default for LowBatteryPolicy {
    fn default() -> LowBatteryPolicy {
        LowBatteryPolicy { warn_percent: 15, critical_percent: 8, hibernate_percent: 3 }
    }
}

pub fn low_battery_action(soc: u8, on_battery: bool, policy: LowBatteryPolicy) -> LowAction {
    if !on_battery || soc > policy.warn_percent {
        return LowAction::Normal;
    }
    if soc <= policy.hibernate_percent {
        LowAction::Hibernate
    } else if soc <= policy.critical_percent {
        LowAction::Critical
    } else {
        LowAction::Warn
    }
}

// ---------------------------------------------------------------------------
// F266 — 唤醒原因记录
// ---------------------------------------------------------------------------

const WAKE_LOG_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct WakeRecord {
    pub kind: WakeKind,
    pub stamp: u64,
    pub battery_percent: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct WakeLog {
    records: [WakeRecord; WAKE_LOG_CAP],
    head: usize,
    count: usize,
}

impl WakeLog {
    pub const fn new() -> WakeLog {
        WakeLog {
            records: [WakeRecord { kind: WakeKind::PowerButton, stamp: 0, battery_percent: 0 };
                WAKE_LOG_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn record(&mut self, record: WakeRecord) {
        self.records[self.head] = record;
        self.head = (self.head + 1) % WAKE_LOG_CAP;
        if self.count < WAKE_LOG_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// Newest first.
    pub fn get(&self, index: usize) -> Option<WakeRecord> {
        if index >= self.count {
            return None;
        }
        let pos = (self.head + WAKE_LOG_CAP - 1 - index) % WAKE_LOG_CAP;
        Some(self.records[pos])
    }

    pub fn count_for(&self, kind: WakeKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|r| r.kind == kind).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// F267 — 功耗仪表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct PowerMeter {
    /// EMA of system power, Q0 milliwatts.
    pub ema_milliwatts: u32,
    pub peak_milliwatts: u32,
    pub redline_milliwatts: u32,
    samples: u32,
}

impl PowerMeter {
    pub const fn new(redline_milliwatts: u32) -> PowerMeter {
        PowerMeter { ema_milliwatts: 0, peak_milliwatts: 0, redline_milliwatts, samples: 0 }
    }

    /// EMA with 1/8 weight — smooth enough for a gauge, fast enough to react.
    pub fn update(&mut self, milliwatts: u32) {
        self.samples += 1;
        if self.samples == 1 {
            self.ema_milliwatts = milliwatts;
        } else {
            let prev = self.ema_milliwatts as u64;
            let next = (prev * 7 + milliwatts as u64) / 8;
            self.ema_milliwatts = next as u32;
        }
        if milliwatts > self.peak_milliwatts {
            self.peak_milliwatts = milliwatts;
        }
    }

    pub fn over_redline(&self) -> bool {
        self.ema_milliwatts > self.redline_milliwatts
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }
}

pub fn estimate_system_mw(cpu_mw: u32, display_mw: u32, disk_mw: u32, nic_mw: u32) -> u32 {
    cpu_mw.saturating_add(display_mw).saturating_add(disk_mw).saturating_add(nic_mw)
}

// ---------------------------------------------------------------------------
// F268 — 能耗预算
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessEnergy {
    pub pid: u32,
    pub millijoules: u64,
    pub budget_millijoules: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetVerdict {
    Ok,
    Warn,
    Throttle,
}

pub fn energy_verdict(p: ProcessEnergy) -> BudgetVerdict {
    if p.budget_millijoules == 0 {
        return BudgetVerdict::Ok;
    }
    let used_permille = (p.millijoules as u64 * 1000) / p.budget_millijoules;
    if used_permille >= 1000 {
        BudgetVerdict::Throttle
    } else if used_permille >= 800 {
        BudgetVerdict::Warn
    } else {
        BudgetVerdict::Ok
    }
}

/// Keep-fraction (0..1000) handed to the scheduler: linear past 80%.
pub fn throttle_keep_permille(p: ProcessEnergy) -> u16 {
    if p.budget_millijoules == 0 {
        return 1000;
    }
    let used_permille = ((p.millijoules as u64 * 1000) / p.budget_millijoules).min(2000);
    if used_permille <= 800 {
        return 1000;
    }
    // Linear from 100% at 80% of budget down to a 20% floor at 180%+.
    let over = (used_permille - 800) as u32;
    let keep = 1000u32.saturating_sub(over * 800 / 1000).max(200);
    keep as u16
}

// ---------------------------------------------------------------------------
// F269/F270 — 省电模式 / 性能模式
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerProfile {
    Saver,
    Balanced,
    Performance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerKnobs {
    pub governor: Governor,
    pub max_pstate_index: u8,
    pub dim_percent: u8,
    pub timer_slack_ms: u32,
    pub usb_autosuspend: bool,
    pub turbo: bool,
}

pub fn profile_knobs(profile: PowerProfile, slowest_index: u8) -> PowerKnobs {
    match profile {
        PowerProfile::Saver => PowerKnobs {
            governor: Governor::Powersave,
            max_pstate_index: slowest_index,
            dim_percent: 30,
            timer_slack_ms: 50,
            usb_autosuspend: true,
            turbo: false,
        },
        PowerProfile::Balanced => PowerKnobs {
            governor: Governor::Schedutil,
            max_pstate_index: (slowest_index / 2).max(1),
            dim_percent: 0,
            timer_slack_ms: 10,
            usb_autosuspend: true,
            turbo: true,
        },
        PowerProfile::Performance => PowerKnobs {
            governor: Governor::Performance,
            max_pstate_index: 0,
            dim_percent: 0,
            timer_slack_ms: 1,
            usb_autosuspend: false,
            turbo: true,
        },
    }
}

// ---------------------------------------------------------------------------
// F271 — 电源事件日志
// ---------------------------------------------------------------------------

const POWER_EVENT_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerEventKind {
    Suspend,
    Resume,
    Hibernate,
    Thermal,
    Battery,
    AcPlug,
    AcUnplug,
    Lid,
    Button,
}

#[derive(Clone, Copy, Debug)]
pub struct PowerEvent {
    pub kind: PowerEventKind,
    pub stamp: u64,
    /// Kind-specific payload (temperature, soc, lid state...).
    pub arg: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct PowerEventLog {
    events: [PowerEvent; POWER_EVENT_CAP],
    head: usize,
    count: usize,
}

impl PowerEventLog {
    pub const fn new() -> PowerEventLog {
        PowerEventLog {
            events: [PowerEvent { kind: PowerEventKind::Suspend, stamp: 0, arg: 0 };
                POWER_EVENT_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, event: PowerEvent) {
        self.events[self.head] = event;
        self.head = (self.head + 1) % POWER_EVENT_CAP;
        if self.count < POWER_EVENT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, index: usize) -> Option<PowerEvent> {
        if index >= self.count {
            return None;
        }
        let pos = (self.head + POWER_EVENT_CAP - 1 - index) % POWER_EVENT_CAP;
        Some(self.events[pos])
    }

    pub fn count_of(&self, kind: PowerEventKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| e.kind == kind).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// F272 — 电池健康档案
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryHealth {
    pub design_mah: u32,
    pub full_mah: u32,
    pub cycles: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthVerdict {
    Good,
    Fair,
    Degraded,
    Replace,
}

impl BatteryHealth {
    /// Wear in percent (0 = as-new).
    pub fn wear_percent(&self) -> u8 {
        if self.design_mah == 0 {
            return 0;
        }
        let full = self.full_mah.min(self.design_mah);
        let wear = ((self.design_mah - full) as u64 * 100) / self.design_mah as u64;
        wear.min(100) as u8
    }

    pub fn verdict(&self) -> HealthVerdict {
        let wear = self.wear_percent();
        if wear >= 40 || self.cycles >= 1200 {
            HealthVerdict::Replace
        } else if wear >= 25 || self.cycles >= 800 {
            HealthVerdict::Degraded
        } else if wear >= 12 || self.cycles >= 400 {
            HealthVerdict::Fair
        } else {
            HealthVerdict::Good
        }
    }
}

/// Accumulate cycle count from charged-through energy (mAh).
pub fn accumulate_cycles(cycles: u32, charged_mah: u32, full_mah: u32) -> u32 {
    if full_mah == 0 {
        return cycles;
    }
    cycles.saturating_add(charged_mah / full_mah)
}

// ---------------------------------------------------------------------------
// F273 — 外设断电策略
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeripheralPolicy {
    pub idle_timeout_ms: u32,
    /// Never suspend the keyboard: it is the primary wake device.
    pub keep_keyboard_awake: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeripheralAction {
    Active,
    Idle,
    Suspend,
}

pub fn peripheral_action(
    idle_ms: u32,
    is_keyboard: bool,
    policy: PeripheralPolicy,
) -> PeripheralAction {
    if is_keyboard && policy.keep_keyboard_awake {
        return PeripheralAction::Active;
    }
    if idle_ms >= policy.idle_timeout_ms {
        PeripheralAction::Suspend
    } else if idle_ms >= policy.idle_timeout_ms / 2 {
        PeripheralAction::Idle
    } else {
        PeripheralAction::Active
    }
}

// ---------------------------------------------------------------------------
// F274 — 唤醒动画
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WakeAnimation {
    pub frames: u16,
    pub frame_ms: u16,
    pub reduce_motion: bool,
}

impl WakeAnimation {
    pub fn total_ms(&self) -> u32 {
        self.frames as u32 * self.frame_ms as u32
    }

    /// Frame index at `ms` (clamped to the last frame).
    pub fn frame_at(&self, ms: u32) -> u16 {
        if self.frame_ms == 0 || self.frames == 0 {
            return 0;
        }
        let f = ms / self.frame_ms as u32;
        f.min(self.frames as u32 - 1) as u16
    }

    /// `reduce-motion` (F423) collapses the animation to a single frame.
    pub fn effective(&self) -> WakeAnimation {
        if self.reduce_motion {
            WakeAnimation { frames: 1, frame_ms: 1, reduce_motion: true }
        } else {
            *self
        }
    }
}

// ---------------------------------------------------------------------------
// F275 — 电源自检
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_power_checks() -> CheckSet {
    let mut set = CheckSet::new("power");

    set.add("F251 s-states", PowerState::from_id(3) == Some(PowerState::S3)
        && PowerState::S5.keeps_memory() == false
        && PowerState::S3.keeps_context(), "state table");
    set.add("F251 pm1 value", slp_typ_value(5) == (5u16 << 10) | SLP_EN, "slp_typ encode");

    let plan = plan_s3(
        SleepRegs { pm1a: 0x1804, pm1b: 0, slp_typa: 3, slp_typb: 3 },
        3,
        3,
        0x8000,
    );
    set.add("F252 s3 plan", plan.step_count == S3_STEPS.len() && plan.pm1b_value == 0, "plan");
    set.add("F252 crc", crc32(b"varix") == crc32(b"varix") && crc32(b"a") != crc32(b"b"), "crc");

    let header = build_hibernation_header(1024, 4096, 0x1234_5678);
    let encoded = header.encode();
    let decoded = HibernationHeader::decode(&encoded).expect("decode");
    set.add(
        "F253 s4 header",
        decoded.valid() && decoded.page_count == 1024 && decoded.image_bytes == 1024 * 4096,
        "header round-trip",
    );
    set.add(
        "F253 storage",
        can_hibernate(required_storage_bytes(1000, 4096), required_storage_bytes(1000, 4096))
            && !can_hibernate(1024, required_storage_bytes(1000, 4096)),
        "capacity math",
    );

    let mut reg = WakeRegistry::new();
    reg.register(WakeSource { name: "kbd", kind: WakeKind::Keyboard, gsi: 1, enabled: true });
    reg.register(WakeSource { name: "rtc", kind: WakeKind::Rtc, gsi: 8, enabled: false });
    set.add(
        "F254 wake sources",
        reg.len() == 2 && reg.enabled_count() == 1 && should_wake(reg.mask(), WakeKind::Keyboard)
            && !should_wake(reg.mask(), WakeKind::Rtc),
        "wake mask",
    );
    set.add(
        "F254 wake enable",
        reg.set_enabled("rtc", true) && reg.enabled_count() == 2,
        "toggle",
    );

    let budget = WakeBudget { firmware_ms: 120, device_ms: 180, display_ms: 60, redline_ms: 500 };
    set.add(
        "F255 fast wake",
        budget.total_ms() == 360
            && wake_verdict(200, 500) == WakeVerdict::Fast
            && wake_verdict(501, 500) == WakeVerdict::Slow,
        "wake budget",
    );

    let bat = BatteryState {
        present: true,
        charge_now_mah: 2400,
        charge_full_mah: 4000,
        design_mah: 5000,
        voltage_mv: 11400,
        current_ma: -1500,
    };
    set.add(
        "F256 battery gauge",
        bat.soc_percent() == 60 && bat.health_percent() == 80 && !bat.is_charging()
            && bat.minutes_remaining() == Some(96),
        "soc/health",
    );
    set.add(
        "F256 smoothing",
        soc_smooth(60, 61, false) == 60 && soc_smooth(60, 59, false) == 59,
        "no phantom charge",
    );

    let care = profile_thresholds(ChargeProfile::Care);
    set.add(
        "F257 care mode",
        charge_decision(85, true, care) == ChargeAction::Stop
            && charge_decision(50, false, care) == ChargeAction::Hold
            && charge_decision(35, false, care) == ChargeAction::Charge,
        "hysteresis",
    );
    set.add(
        "F258 charge profile",
        charge_current_limit_ma(ChargeProfile::Travel, 25, 20) == 4000
            && charge_current_limit_ma(ChargeProfile::Travel, 50, 20) < 4000
            && charge_current_limit_ma(ChargeProfile::Desk, 25, 95) <= 800,
        "current limit",
    );

    let mut table = PStateTable::new();
    table.push(PState { index: 0, mhz: 3600, milliwatts: 45000 });
    table.push(PState { index: 1, mhz: 2800, milliwatts: 28000 });
    table.push(PState { index: 2, mhz: 2000, milliwatts: 15000 });
    table.push(PState { index: 3, mhz: 1200, milliwatts: 6000 });
    set.add(
        "F259 p-states",
        table.pick(900, Governor::Performance, 3) == 0
            && table.pick(900, Governor::Powersave, 0) == 3
            && table.pick(1000, Governor::Schedutil, 0) == 0
            && table.pick(0, Governor::Schedutil, 0) == 3,
        "governor",
    );

    let mut zone = ThermalZone::new("pkg");
    zone.upsert(ThermalSensor { name: "cpu", millideg_c: 78_000, critical_millideg: 100_000 });
    zone.upsert(ThermalSensor { name: "ssd", millideg_c: 45_000, critical_millideg: 85_000 });
    set.add(
        "F260 thermal zone",
        zone.max_millideg() == 78_000 && !zone.is_critical() && zone.hottest() == Some("cpu")
            && millideg_to_c(zone.max_millideg()) == 78,
        "sensors",
    );
    set.add(
        "F260 verdicts",
        thermal_verdict(101_000, 70_000, 100_000) == ThermalVerdict::Critical
            && thermal_verdict(30_000, 70_000, 100_000) == ThermalVerdict::Cool,
        "thresholds",
    );

    let mut fan = FanCurve::new();
    fan.push(FanCurvePoint { temp_c: 40, duty_percent: 20 });
    fan.push(FanCurvePoint { temp_c: 70, duty_percent: 70 });
    fan.push(FanCurvePoint { temp_c: 90, duty_percent: 100 });
    set.add(
        "F261 fan curve",
        fan.duty(55, 0) == 45 && fan.duty(90, 0) == 100 && fan.duty(30, 0) == 20,
        "interpolation",
    );
    set.add("F261 hysteresis", fan.duty(41, 20) == 20, "no hunting");

    let mitig = ThermalMitigation { passive_c: 70, hot_c: 85, critical_c: 95 };
    set.add(
        "F262 thermal throttle",
        mitigation_level(96, mitig) == MitigationLevel::Critical
            && capped_pstate(0, MitigationLevel::Critical, 3) == 3
            && capped_pstate(0, MitigationLevel::None, 3) == 0,
        "throttle",
    );

    let mut btn = PowerButtonState::new();
    let mut long_press_seen = false;
    for _ in 0..20 {
        if btn.tick(true, 100) == PowerButtonEvent::LongPress {
            long_press_seen = true;
        }
    }
    // A long press must not also emit a short press on release.
    let release = btn.tick(false, 100);
    set.add(
        "F263 power button",
        long_press_seen && release == PowerButtonEvent::None,
        "press states",
    );
    let mut btn2 = PowerButtonState::new();
    set.add(
        "F263 short press",
        btn2.tick(true, 100) == PowerButtonEvent::None
            && btn2.tick(false, 100) == PowerButtonEvent::ShortPress,
        "short",
    );

    set.add(
        "F264 lid",
        lid_action(LidState::Closed, true, false) == LidAction::Suspend
            && lid_action(LidState::Closed, false, true) == LidAction::BlankScreen
            && lid_action(LidState::Open, true, false) == LidAction::Nothing,
        "lid policy",
    );

    let low = LowBatteryPolicy::default();
    set.add(
        "F265 low battery",
        low_battery_action(2, true, low) == LowAction::Hibernate
            && low_battery_action(6, true, low) == LowAction::Critical
            && low_battery_action(50, true, low) == LowAction::Normal,
        "emergency",
    );

    let mut wl = WakeLog::new();
    wl.record(WakeRecord { kind: WakeKind::Lid, stamp: 1, battery_percent: 80 });
    wl.record(WakeRecord { kind: WakeKind::Lid, stamp: 2, battery_percent: 70 });
    set.add(
        "F266 wake log",
        wl.get(0).unwrap().kind == WakeKind::Lid && wl.count_for(WakeKind::Lid) == 2
            && wl.get(0).unwrap().stamp == 2,
        "newest first",
    );

    let mut meter = PowerMeter::new(30_000);
    meter.update(20_000);
    meter.update(40_000);
    set.add(
        "F267 power meter",
        meter.ema_milliwatts > 20_000 && meter.peak_milliwatts == 40_000 && !meter.over_redline(),
        "ema",
    );
    set.add("F267 budget sum", estimate_system_mw(1000, 2000, 300, 400) == 3700, "sum");

    set.add(
        "F268 energy budget",
        energy_verdict(ProcessEnergy { pid: 1, millijoules: 700, budget_millijoules: 1000 })
            == BudgetVerdict::Ok
            && energy_verdict(ProcessEnergy { pid: 1, millijoules: 1000, budget_millijoules: 1000 })
                == BudgetVerdict::Throttle
            && throttle_keep_permille(ProcessEnergy {
                pid: 1,
                millijoules: 1000,
                budget_millijoules: 1000,
            }) == 840,
        "budget",
    );

    set.add(
        "F269 saver profile",
        profile_knobs(PowerProfile::Saver, 3).governor == Governor::Powersave
            && profile_knobs(PowerProfile::Saver, 3).max_pstate_index == 3
            && !profile_knobs(PowerProfile::Saver, 3).turbo,
        "saver knobs",
    );
    set.add(
        "F270 performance profile",
        profile_knobs(PowerProfile::Performance, 3).max_pstate_index == 0
            && profile_knobs(PowerProfile::Performance, 3).timer_slack_ms == 1
            && profile_knobs(PowerProfile::Balanced, 3).governor == Governor::Schedutil,
        "performance knobs",
    );

    let mut log = PowerEventLog::new();
    log.push(PowerEvent { kind: PowerEventKind::Suspend, stamp: 1, arg: 3 });
    log.push(PowerEvent { kind: PowerEventKind::Resume, stamp: 2, arg: 3 });
    set.add(
        "F271 event log",
        log.len() == 2 && log.get(0).unwrap().kind == PowerEventKind::Resume
            && log.count_of(PowerEventKind::Suspend) == 1,
        "events",
    );

    let health = BatteryHealth { design_mah: 5000, full_mah: 4200, cycles: 500 };
    set.add(
        "F272 battery health",
        health.wear_percent() == 16 && health.verdict() == HealthVerdict::Fair
            && accumulate_cycles(100, 4000, 4000) == 101,
        "wear",
    );

    let per = PeripheralPolicy { idle_timeout_ms: 10_000, keep_keyboard_awake: true };
    set.add(
        "F273 peripherals",
        peripheral_action(60_000, true, per) == PeripheralAction::Active
            && peripheral_action(60_000, false, per) == PeripheralAction::Suspend
            && peripheral_action(1_000, false, per) == PeripheralAction::Active,
        "idle policy",
    );

    let anim = WakeAnimation { frames: 12, frame_ms: 24, reduce_motion: false };
    set.add(
        "F274 wake animation",
        anim.total_ms() == 288 && anim.frame_at(100) == 4
            && anim.effective().frames == 12
            && WakeAnimation { frames: 12, frame_ms: 24, reduce_motion: true }.effective().frames
                == 1,
        "animation",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fadt_body() -> [u8; 256] {
        let mut t = [0u8; 256];
        t[0..4].copy_from_slice(b"FACP");
        t[45] = 2; // mobile profile
        t[46..48].copy_from_slice(&9u16.to_le_bytes());
        t[48..52].copy_from_slice(&0xB2u32.to_le_bytes());
        t[64..68].copy_from_slice(&0x1804u32.to_le_bytes());
        t[68..72].copy_from_slice(&0u32.to_le_bytes());
        t[89] = 2;
        t
    }

    #[test]
    fn f251_fadt_and_sleep_values() {
        let f = parse_fadt(&fadt_body()).expect("fadt");
        assert_eq!(f.pm1a_cnt_blk, 0x1804);
        assert_eq!(f.pm1_cnt_len, 2);
        assert_eq!(f.sci_int, 9);
        assert!(!SleepRegs::from_fadt(&f, 3, 3).has_b());
        assert_eq!(slp_typ_value(3), (3 << 10) | SLP_EN);
        assert!(PowerState::S4.keeps_memory());
        assert!(!PowerState::S5.keeps_context());
        assert_eq!(PowerState::from_id(9), None);
    }

    #[test]
    fn f251_bad_fadt_rejected() {
        assert!(parse_fadt(&[0u8; 8]).is_none());
        let mut t = fadt_body();
        t[0] = b'X';
        assert!(parse_fadt(&t).is_none());
    }

    #[test]
    fn f252_suspend_plan_is_stable() {
        let regs = SleepRegs { pm1a: 0x1804, pm1b: 0x1804, slp_typa: 3, slp_typb: 3 };
        let a = plan_s3(regs, 3, 3, 0x10_0000);
        let b = plan_s3(regs, 3, 3, 0x10_0000);
        assert_eq!(a.crc, b.crc);
        assert!(a.pm1b_value != 0);
        assert_eq!(a.step_count, 7);
    }

    #[test]
    fn f253_hibernation_round_trip() {
        let h = build_hibernation_header(0, 4096, 7);
        assert!(!h.valid()); // zero pages is not a valid image
        let h = build_hibernation_header(64, 4096, 7);
        let bytes = h.encode();
        assert_eq!(&bytes[0..4], b"VRXH");
        let back = HibernationHeader::decode(&bytes).unwrap();
        assert_eq!(back.crc, 7);
        assert!(HibernationHeader::decode(&bytes[..16]).is_none());
        assert!(HibernationHeader::decode(b"XXXX").is_none());
    }

    #[test]
    fn f254_wake_registry_bounds() {
        let mut r = WakeRegistry::new();
        for i in 0..MAX_WAKE_SOURCES + 4 {
            let ok = r.register(WakeSource {
                name: "x",
                kind: WakeKind::from_index(i % 7).unwrap(),
                gsi: i as u8,
                enabled: i % 2 == 0,
            });
            if i >= MAX_WAKE_SOURCES {
                assert!(!ok);
            }
        }
        assert_eq!(r.len(), MAX_WAKE_SOURCES);
        assert_eq!(r.enabled_count(), MAX_WAKE_SOURCES / 2);
        assert!(!r.set_enabled("missing", true));
    }

    #[test]
    fn f255_wake_scoring() {
        let fast = WakeBudget { firmware_ms: 50, device_ms: 50, display_ms: 16, redline_ms: 500 };
        let slow = WakeBudget { firmware_ms: 400, device_ms: 400, display_ms: 200, redline_ms: 500 };
        assert!(fast.score() > slow.score());
        assert_eq!(fast.score(), 100);
        assert_eq!(wake_verdict(0, 500), WakeVerdict::Fast);
        assert_eq!(wake_verdict(500, 500), WakeVerdict::Nominal);
    }

    #[test]
    fn f256_battery_math() {
        let b = BatteryState {
            present: true,
            charge_now_mah: 4000,
            charge_full_mah: 4000,
            design_mah: 4000,
            voltage_mv: 12_000,
            current_ma: 2000,
        };
        assert_eq!(b.soc_percent(), 100);
        assert!(b.is_charging());
        assert_eq!(b.minutes_remaining(), Some(0));
        assert_eq!(b.milliwatts(), 24_000);
        let absent = BatteryState { present: false, ..b };
        assert_eq!(absent.soc_percent(), 0);
        assert_eq!(absent.minutes_remaining(), None);
        let idle = BatteryState { current_ma: 0, ..b };
        assert_eq!(idle.minutes_remaining(), None);
    }

    #[test]
    fn f257_charge_hysteresis_full_profile() {
        let full = profile_thresholds(ChargeProfile::Full);
        assert_eq!(charge_decision(99, true, full), ChargeAction::Charge);
        assert_eq!(charge_decision(100, true, full), ChargeAction::Stop);
        assert_eq!(charge_decision(96, false, full), ChargeAction::Hold);
        let broken = ChargeThreshold { start_below: 90, stop_at: 50 };
        assert_eq!(charge_decision(10, true, broken), ChargeAction::Charge);
    }

    #[test]
    fn f258_temperature_derating() {
        assert_eq!(charge_current_limit_ma(ChargeProfile::Care, 25, 50), 2000);
        assert_eq!(charge_current_limit_ma(ChargeProfile::Care, 45, 50), 500);
        assert_eq!(charge_current_limit_ma(ChargeProfile::Care, 40, 50), 1000);
        assert_eq!(charge_current_limit_ma(ChargeProfile::Care, -5, 50), 500);
    }

    #[test]
    fn f259_governor_paths() {
        let mut t = PStateTable::new();
        // Fastest first: index 0 is the top P-state.
        for i in 0..5 {
            t.push(PState {
                index: i,
                mhz: 3600 - 500 * i as u32,
                milliwatts: 45000 - 8000 * i as u32,
            });
        }
        assert_eq!(t.len(), 5);
        assert_eq!(t.pick(999, Governor::Performance, 4), 0);
        assert_eq!(t.pick(0, Governor::Ondemand, 4), 4);
        assert_eq!(t.pick(900, Governor::Ondemand, 4), 0); // above 80%: jump to max
        assert_eq!(t.pick(500, Governor::Ondemand, 4), 3); // mid band: step down
        assert_eq!(t.pick(100, Governor::Ondemand, 0), 1); // idle: step up
        assert!(t.estimate_milliwatts(0, 1000) > t.estimate_milliwatts(4, 1000));
        let empty = PStateTable::new();
        assert_eq!(empty.pick(500, Governor::Schedutil, 0), 0);
    }

    #[test]
    fn f260_zone_updates_replace_in_place() {
        let mut z = ThermalZone::new("pkg");
        z.upsert(ThermalSensor { name: "cpu", millideg_c: 50_000, critical_millideg: 100_000 });
        z.upsert(ThermalSensor { name: "cpu", millideg_c: 99_000, critical_millideg: 100_000 });
        assert_eq!(z.max_millideg(), 99_000);
        assert!(!z.is_critical());
        z.upsert(ThermalSensor { name: "gpu", millideg_c: 105_000, critical_millideg: 100_000 });
        assert!(z.is_critical());
        let empty = ThermalZone::new("none");
        assert_eq!(empty.max_millideg(), 0);
        assert_eq!(empty.hottest(), None);
    }

    #[test]
    fn f261_fan_curve_edges() {
        let mut f = FanCurve::new();
        assert_eq!(f.duty(90, 0), 0);
        f.push(FanCurvePoint { temp_c: 50, duty_percent: 0 });
        f.push(FanCurvePoint { temp_c: 60, duty_percent: 100 });
        assert_eq!(f.duty(55, 0), 50);
        assert_eq!(f.duty(20, 0), 0);
        assert_eq!(f.duty(99, 0), 100);
        assert!(!f.push(FanCurvePoint { temp_c: 70, duty_percent: 101 }));
        for i in 0..MAX_SENSORS {
            f.push(FanCurvePoint { temp_c: 70 + i as i8, duty_percent: 50 });
        }
        assert_eq!(f.len(), MAX_SENSORS);
    }

    #[test]
    fn f262_mitigation_caps() {
        let m = ThermalMitigation { passive_c: 70, hot_c: 85, critical_c: 95 };
        assert_eq!(mitigation_level(69, m), MitigationLevel::None);
        assert_eq!(mitigation_level(70, m), MitigationLevel::Passive);
        assert_eq!(mitigation_level(88, m), MitigationLevel::Heavy);
        assert_eq!(capped_pstate(0, MitigationLevel::Heavy, 3), 3);
        assert_eq!(capped_pstate(3, MitigationLevel::Passive, 3), 3);
    }

    #[test]
    fn f263_button_forced_off() {
        let mut b = PowerButtonState::new();
        let mut last = PowerButtonEvent::None;
        for _ in 0..100 {
            let e = b.tick(true, 100);
            if e != PowerButtonEvent::None {
                last = e;
            }
        }
        assert_eq!(last, PowerButtonEvent::ForcedOff);
        assert_eq!(b.tick(false, 100), PowerButtonEvent::None);
        assert_eq!(b.tick(false, 100), PowerButtonEvent::None);
    }

    #[test]
    fn f264_lid_matrix() {
        assert_eq!(lid_action(LidState::Unknown, true, false), LidAction::Nothing);
        assert_eq!(lid_action(LidState::Closed, true, true), LidAction::BlankScreen);
        assert_eq!(lid_action(LidState::Closed, false, false), LidAction::Suspend);
    }

    #[test]
    fn f265_low_battery_edges() {
        let p = LowBatteryPolicy { warn_percent: 15, critical_percent: 8, hibernate_percent: 3 };
        assert_eq!(low_battery_action(16, true, p), LowAction::Normal);
        assert_eq!(low_battery_action(15, true, p), LowAction::Warn);
        assert_eq!(low_battery_action(1, false, p), LowAction::Normal); // on AC
    }

    #[test]
    fn f266_ring_wraps() {
        let mut l = WakeLog::new();
        for i in 0..WAKE_LOG_CAP + 3 {
            l.record(WakeRecord { kind: WakeKind::Rtc, stamp: i as u64, battery_percent: 50 });
        }
        assert_eq!(l.len(), WAKE_LOG_CAP);
        assert_eq!(l.get(0).unwrap().stamp, (WAKE_LOG_CAP + 2) as u64);
        assert_eq!(l.count_for(WakeKind::Rtc), WAKE_LOG_CAP);
        assert!(l.get(WAKE_LOG_CAP).is_none());
    }

    #[test]
    fn f267_meter_reacts_and_peaks() {
        let mut m = PowerMeter::new(25_000);
        m.update(10_000);
        assert_eq!(m.ema_milliwatts, 10_000);
        for _ in 0..32 {
            m.update(40_000);
        }
        assert!(m.ema_milliwatts > 35_000);
        assert!(m.over_redline());
        assert_eq!(m.peak_milliwatts, 40_000);
        assert_eq!(m.samples(), 33);
    }

    #[test]
    fn f268_budget_curve() {
        let p = ProcessEnergy { pid: 7, millijoules: 1800, budget_millijoules: 1000 };
        assert_eq!(energy_verdict(p), BudgetVerdict::Throttle);
        assert_eq!(throttle_keep_permille(p), 200); // 180% of budget → floor
        let mid = ProcessEnergy { pid: 7, millijoules: 1300, budget_millijoules: 1000 };
        assert_eq!(energy_verdict(mid), BudgetVerdict::Throttle);
        assert_eq!(throttle_keep_permille(mid), 600);
        let over = ProcessEnergy { pid: 7, millijoules: 5000, budget_millijoules: 1000 };
        assert_eq!(throttle_keep_permille(over), 200);
        let none = ProcessEnergy { pid: 7, millijoules: 100, budget_millijoules: 0 };
        assert_eq!(energy_verdict(none), BudgetVerdict::Ok);
        assert_eq!(throttle_keep_permille(none), 1000);
    }

    #[test]
    fn f269_profiles_differ() {
        let saver = profile_knobs(PowerProfile::Saver, 5);
        let perf = profile_knobs(PowerProfile::Performance, 5);
        assert_eq!(saver.max_pstate_index, 5);
        assert_eq!(perf.max_pstate_index, 0);
        assert!(saver.dim_percent > perf.dim_percent);
        assert!(saver.timer_slack_ms > perf.timer_slack_ms);
        assert!(saver.usb_autosuspend && !perf.usb_autosuspend);
    }

    #[test]
    fn f271_event_ring_wraps() {
        let mut l = PowerEventLog::new();
        for i in 0..POWER_EVENT_CAP + 5 {
            l.push(PowerEvent { kind: PowerEventKind::Thermal, stamp: i as u64, arg: 70 });
        }
        assert_eq!(l.len(), POWER_EVENT_CAP);
        assert_eq!(l.get(0).unwrap().stamp, (POWER_EVENT_CAP + 4) as u64);
        assert_eq!(l.count_of(PowerEventKind::Thermal), POWER_EVENT_CAP);
    }

    #[test]
    fn f272_health_verdicts() {
        assert_eq!(BatteryHealth { design_mah: 5000, full_mah: 5000, cycles: 10 }.verdict(), HealthVerdict::Good);
        assert_eq!(BatteryHealth { design_mah: 5000, full_mah: 4000, cycles: 100 }.verdict(), HealthVerdict::Fair);
        assert_eq!(BatteryHealth { design_mah: 5000, full_mah: 3500, cycles: 100 }.verdict(), HealthVerdict::Degraded);
        assert_eq!(BatteryHealth { design_mah: 5000, full_mah: 2500, cycles: 100 }.verdict(), HealthVerdict::Replace);
        assert_eq!(BatteryHealth { design_mah: 0, full_mah: 100, cycles: 0 }.wear_percent(), 0);
    }

    #[test]
    fn f273_peripheral_idle_bands() {
        let p = PeripheralPolicy { idle_timeout_ms: 10_000, keep_keyboard_awake: false };
        assert_eq!(peripheral_action(0, false, p), PeripheralAction::Active);
        assert_eq!(peripheral_action(6_000, false, p), PeripheralAction::Idle);
        assert_eq!(peripheral_action(10_000, false, p), PeripheralAction::Suspend);
    }

    #[test]
    fn f274_animation_frames() {
        let a = WakeAnimation { frames: 10, frame_ms: 16, reduce_motion: false };
        assert_eq!(a.total_ms(), 160);
        assert_eq!(a.frame_at(0), 0);
        assert_eq!(a.frame_at(15), 0);
        assert_eq!(a.frame_at(16), 1);
        assert_eq!(a.frame_at(10_000), 9);
        let zero = WakeAnimation { frames: 0, frame_ms: 0, reduce_motion: false };
        assert_eq!(zero.frame_at(100), 0);
        assert_eq!(zero.effective().frames, 0);
    }

    #[test]
    fn f275_self_test_passes() {
        let set = run_power_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("power self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
