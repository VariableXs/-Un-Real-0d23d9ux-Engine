//! UNX-F48925 五键语义 / 双击阈值 / 加速参数表 · 0x70F80
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B07）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F48925-J1 五键 ×10^2 键位零串；
//!   ③ UNX-F48926-J1 双击 ×10^2 阈值判定一致；
//!   ④ UNX-F48927-J1 参数表 ×5 组校验一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：键位掩码合法性 → 边沿求解 → 双击判定 → 参数表校验 → 账本。
//! 先校验掩码再求边沿：未知位一旦混入，pressed/released 会把「不存在的键」
//! 当成真键递送。双击必须看时间与位移双阈值，只看时间会把两次相隔很远的
//! 单击合成双击；只看位移会把慢速双击吞掉。参数表必须先整体校验再查表，
//! 半坏的表查出来的增益没有任何意义。
//! 关键差异：本模块只回答「键位怎么变、这次是不是双击、这个速度查多少
//!           增益」，不做坐标合成（B07-CORE）、不做拖拽状态机（B07-EDGE）、
//!           不做滚轮（B08）。X1/X2 的逻辑重映射只在掩码层完成，不改物理
//!           键号，物理键号由调用方保管。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70F80，B07 第三槽）。
pub const SEG_BASE: u32 = 0x70F80;

/// 五键键位。
pub const BTN_LEFT: u8 = 0x01;
pub const BTN_MIDDLE: u8 = 0x02;
pub const BTN_RIGHT: u8 = 0x04;
pub const BTN_X1: u8 = 0x08;
pub const BTN_X2: u8 = 0x10;
pub const BUTTON_MASK: u8 = 0x1F;
/// 五键表行数。
pub const FIVE_KEYS: usize = 5;
/// 双击轮数（卡面 ×10^2 载体）。
pub const CLICK_ROUNDS: u32 = 100;
/// 参数表组数（卡面 ×5 组载体）。
pub const TABLE_GROUPS: usize = 5;
/// 双击默认阈值（毫秒 / 像素）。
pub const DC_DEFAULT_MS: u32 = 500;
pub const DC_DEFAULT_DIST: u32 = 4;
pub const DC_MIN_MS: u32 = 100;
pub const DC_MAX_MS: u32 = 1000;
pub const DC_MAX_DIST: u32 = 64;
/// 增益区间与速度封顶（与 B07-CORE 同口径）。
pub const GAIN_MIN: u32 = 250;
pub const GAIN_MAX: u32 = 4000;
pub const SPEED_CAP: u32 = 8_000;
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GdCode {
    /// 键位非法（多键或未知位）
    BtnBad,
    /// 掩码非法（含未定义位）
    MaskBad,
    /// X 键映射非法（重复或越界）
    XMapBad,
    /// 双击判定非法（时距/位移口径破坏）
    ClickBad,
    /// 时间回退
    TimeBad,
    /// 参数表非法（组数/次序/单调）
    TableBad,
    /// 次序非法（账本累计不一致）
    OrderBad,
    /// 复算不一致
    ReplayBad,
    /// 渲染回读失败
    RenderBad,
    /// 覆盖不足
    CoverageBad,
    /// 账不平
    LedgerBad,
    /// 段位越界
    SegmentBad,
}

impl GdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            GdCode::BtnBad => "键位非法：必须恰为单个已知键位",
            GdCode::MaskBad => "掩码非法：含未定义的键位比特",
            GdCode::XMapBad => "X 键映射非法：逻辑码重复或越出 1..=2",
            GdCode::ClickBad => "双击判定非法：时距/位移口径被破坏",
            GdCode::TimeBad => "时间回退：观察 tick 小于上次 tick",
            GdCode::TableBad => "参数表非法：组数/速度次序/增益单调不合法",
            GdCode::OrderBad => "次序非法：账本累计与轮数不一致",
            GdCode::ReplayBad => "复算不一致：两次判定结果不同",
            GdCode::RenderBad => "渲染回读失败：字段缺失或数值越界",
            GdCode::CoverageBad => "覆盖不足：五键或参数组未全覆盖",
            GdCode::LedgerBad => "账不平：合成分与拒绝分之和不等于轮数",
            GdCode::SegmentBad => "段位越界：诊断码逃出 0x70F80 窗口",
        }
    }

    pub const ALL: [GdCode; 12] = [
        GdCode::BtnBad,
        GdCode::MaskBad,
        GdCode::XMapBad,
        GdCode::ClickBad,
        GdCode::TimeBad,
        GdCode::TableBad,
        GdCode::OrderBad,
        GdCode::ReplayBad,
        GdCode::RenderBad,
        GdCode::CoverageBad,
        GdCode::LedgerBad,
        GdCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < GdCode::ALL.len() {
        if GdCode::ALL[i].val() == code {
            return GdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 键位掩码 ──────────────────────────────────────────────────────
pub fn audit_mask(mask: u8) -> Result<u8, u32> {
    if mask & !BUTTON_MASK != 0 {
        return Err(GdCode::MaskBad.val());
    }
    Ok(mask)
}

pub fn single_bit(mask: u8) -> bool {
    mask != 0 && (mask & mask.wrapping_sub(1)) == 0
}

pub fn button_name(mask: u8) -> &'static str {
    match mask {
        BTN_LEFT => "左键",
        BTN_MIDDLE => "中键",
        BTN_RIGHT => "右键",
        BTN_X1 => "X1",
        BTN_X2 => "X2",
        0x00 => "无",
        _ => "组合或多键",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyRow {
    pub mask: u8,
    pub name: &'static str,
    pub x_code: u8,
}

pub fn five_key_table() -> Vec<KeyRow> {
    let mut v = Vec::new();
    v.push(KeyRow {
        mask: BTN_LEFT,
        name: "左键",
        x_code: 0,
    });
    v.push(KeyRow {
        mask: BTN_MIDDLE,
        name: "中键",
        x_code: 0,
    });
    v.push(KeyRow {
        mask: BTN_RIGHT,
        name: "右键",
        x_code: 0,
    });
    v.push(KeyRow {
        mask: BTN_X1,
        name: "X1",
        x_code: 1,
    });
    v.push(KeyRow {
        mask: BTN_X2,
        name: "X2",
        x_code: 2,
    });
    v
}

/// 五键表审计：恰五行、掩码恰为 1/2/4/8/16、名字互异、X 码恰 1/2。
pub fn audit_five(rows: &[KeyRow]) -> Result<u32, u32> {
    if rows.len() != FIVE_KEYS {
        return Err(GdCode::CoverageBad.val());
    }
    let mut seen_mask = 0u8;
    let mut nonzero_codes = 0u32;
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(GdCode::CoverageBad.val()),
        };
        if !single_bit(r.mask) || r.mask & !BUTTON_MASK != 0 {
            return Err(GdCode::BtnBad.val());
        }
        if seen_mask & r.mask != 0 {
            return Err(GdCode::CoverageBad.val());
        }
        seen_mask |= r.mask;
        if r.name.is_empty() {
            return Err(GdCode::BtnBad.val());
        }
        if r.x_code != 0 {
            nonzero_codes = nonzero_codes.saturating_add(1);
        }
        let mut j = i + 1;
        while j < rows.len() {
            let o = match rows.get(j) {
                Some(v) => v,
                None => return Err(GdCode::CoverageBad.val()),
            };
            if o.name == r.name {
                return Err(GdCode::BtnBad.val());
            }
            if o.x_code != 0 && r.x_code != 0 && o.x_code == r.x_code {
                return Err(GdCode::XMapBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    if seen_mask != BUTTON_MASK {
        return Err(GdCode::CoverageBad.val());
    }
    if nonzero_codes != 2 {
        return Err(GdCode::XMapBad.val());
    }
    Ok(FIVE_KEYS as u32)
}

/// 边沿求解：返回 (按下位, 释放位)，无变化判 BtnBad。
pub fn transition(old: u8, new: u8) -> Result<(u8, u8), u32> {
    audit_mask(old)?;
    audit_mask(new)?;
    if old == new {
        return Err(GdCode::BtnBad.val());
    }
    let pressed = new & !old;
    let released = old & !new;
    if pressed == 0 && released == 0 {
        return Err(GdCode::BtnBad.val());
    }
    Ok((pressed, released))
}

/// X 键逻辑重映射：[物理 X1, 物理 X2] → 逻辑 XBUTTON 码，码值 1/2 互异。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct XButtonMap {
    pub slots: [u8; 2],
}

impl XButtonMap {
    pub fn new(a: u8, b: u8) -> Result<XButtonMap, u32> {
        if a == 0 || a > 2 || b == 0 || b > 2 || a == b {
            return Err(GdCode::XMapBad.val());
        }
        Ok(XButtonMap { slots: [a, b] })
    }

    pub fn default_map() -> XButtonMap {
        XButtonMap { slots: [1, 2] }
    }

    pub fn swapped() -> XButtonMap {
        XButtonMap { slots: [2, 1] }
    }

    /// 物理键 1/2 → 逻辑掩码（X1 或 X2）。
    pub fn resolve(&self, physical: u8) -> Result<u8, u32> {
        if self.slots[0] == 0
            || self.slots[0] > 2
            || self.slots[1] == 0
            || self.slots[1] > 2
            || self.slots[0] == self.slots[1]
        {
            return Err(GdCode::XMapBad.val());
        }
        let logical = match physical {
            1 => self.slots[0],
            2 => self.slots[1],
            _ => return Err(GdCode::XMapBad.val()),
        };
        if logical == 1 {
            Ok(BTN_X1)
        } else {
            Ok(BTN_X2)
        }
    }
}

// ── 双击 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DoubleClickSpec {
    pub threshold_ms: u32,
    pub distance: u32,
}

impl DoubleClickSpec {
    pub fn new(threshold_ms: u32, distance: u32) -> Result<DoubleClickSpec, u32> {
        if threshold_ms < DC_MIN_MS || threshold_ms > DC_MAX_MS {
            return Err(GdCode::ClickBad.val());
        }
        if distance > DC_MAX_DIST {
            return Err(GdCode::ClickBad.val());
        }
        Ok(DoubleClickSpec {
            threshold_ms,
            distance,
        })
    }

    pub fn default_spec() -> DoubleClickSpec {
        DoubleClickSpec {
            threshold_ms: DC_DEFAULT_MS,
            distance: DC_DEFAULT_DIST,
        }
    }

    pub fn limit_sq(&self) -> u64 {
        (self.distance as u64).saturating_mul(self.distance as u64)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClickKind {
    Single,
    Double,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DcTracker {
    pub spec: DoubleClickSpec,
    last_tick: Option<u32>,
    last_x: i64,
    last_y: i64,
    last_btn: u8,
}

impl DcTracker {
    pub fn new(spec: DoubleClickSpec) -> DcTracker {
        DcTracker {
            spec,
            last_tick: None,
            last_x: 0,
            last_y: 0,
            last_btn: 0,
        }
    }

    pub fn with_default() -> DcTracker {
        DcTracker::new(DoubleClickSpec::default_spec())
    }

    /// 观察一次按下：时间回退判 TimeBad；左键在双阈值内判 Double 并清账。
    pub fn observe(&mut self, btn: u8, tick: u32, x: i64, y: i64) -> Result<ClickKind, u32> {
        if !single_bit(btn) || btn & !BUTTON_MASK != 0 {
            return Err(GdCode::BtnBad.val());
        }
        if let Some(last) = self.last_tick {
            if tick < last {
                return Err(GdCode::TimeBad.val());
            }
            if btn == BTN_LEFT && self.last_btn == BTN_LEFT {
                let dt = tick.saturating_sub(last);
                let dx = x.saturating_sub(self.last_x).unsigned_abs();
                let dy = y.saturating_sub(self.last_y).unsigned_abs();
                let d2 = dx
                    .saturating_mul(dx)
                    .saturating_add(dy.saturating_mul(dy));
                if dt >= 1 && dt <= self.spec.threshold_ms && d2 <= self.spec.limit_sq() {
                    self.last_tick = None;
                    self.last_btn = 0;
                    return Ok(ClickKind::Double);
                }
            }
        }
        self.last_tick = Some(tick);
        self.last_x = x;
        self.last_y = y;
        self.last_btn = btn;
        Ok(ClickKind::Single)
    }
}

/// 双击 ×10^2：阈值内 Double、阈值外 Single、位移边界精确、右键不双击。
pub fn click_rounds(rounds: u32) -> Result<(), u32> {
    let mut i = 0u32;
    while i < rounds {
        let mut t = DcTracker::with_default();
        if t.observe(BTN_LEFT, 0, 0, 0)? != ClickKind::Single {
            return Err(GdCode::ClickBad.val());
        }
        if t.observe(BTN_LEFT, DC_DEFAULT_MS, 4, 0)? != ClickKind::Double {
            return Err(GdCode::ClickBad.val());
        }
        if t.observe(BTN_LEFT, 900, 0, 0)? != ClickKind::Single {
            return Err(GdCode::ClickBad.val());
        }
        if t.observe(BTN_LEFT, 1300, 0, 0)? != ClickKind::Double {
            return Err(GdCode::ClickBad.val());
        }
        let mut u = DcTracker::with_default();
        if u.observe(BTN_LEFT, 0, 0, 0)? != ClickKind::Single {
            return Err(GdCode::ClickBad.val());
        }
        if u.observe(BTN_LEFT, DC_DEFAULT_MS + 1, 0, 0)? != ClickKind::Single {
            return Err(GdCode::ClickBad.val());
        }
        if u.observe(BTN_RIGHT, 600, 0, 0)? != ClickKind::Single {
            return Err(GdCode::ClickBad.val());
        }
        if u.observe(BTN_RIGHT, 700, 0, 0)? != ClickKind::Single {
            return Err(GdCode::ClickBad.val());
        }
        let mut w = DcTracker::with_default();
        if w.observe(BTN_LEFT, 0, 0, 0)? != ClickKind::Single {
            return Err(GdCode::ClickBad.val());
        }
        if w.observe(BTN_LEFT, 10, 5, 0)? != ClickKind::Single {
            return Err(GdCode::ClickBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(())
}

// ── 加速参数表（五组） ────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GainRow {
    pub speed: u32,
    pub gain: u32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GainTable {
    rows: Vec<GainRow>,
}

impl GainTable {
    pub fn new(rows: Vec<GainRow>) -> Result<GainTable, u32> {
        let t = GainTable { rows };
        t.validate()?;
        Ok(t)
    }

    pub fn validate(&self) -> Result<(), u32> {
        if self.rows.len() != TABLE_GROUPS {
            return Err(GdCode::TableBad.val());
        }
        let first = match self.rows.first() {
            Some(v) => v,
            None => return Err(GdCode::TableBad.val()),
        };
        if first.speed != 0 || first.gain < GAIN_MIN || first.gain > GAIN_MAX {
            return Err(GdCode::TableBad.val());
        }
        let mut i = 1usize;
        while i < self.rows.len() {
            let a = match self.rows.get(i - 1) {
                Some(v) => v,
                None => return Err(GdCode::TableBad.val()),
            };
            let b = match self.rows.get(i) {
                Some(v) => v,
                None => return Err(GdCode::TableBad.val()),
            };
            if b.speed <= a.speed || b.speed > SPEED_CAP {
                return Err(GdCode::TableBad.val());
            }
            if b.gain < a.gain || b.gain < GAIN_MIN || b.gain > GAIN_MAX {
                return Err(GdCode::TableBad.val());
            }
            i += 1;
        }
        Ok(())
    }

    pub fn rows(&self) -> &[GainRow] {
        &self.rows
    }

    /// 分段线性查表：采样点精确，段内半值进位，末点以上封顶。
    pub fn lookup(&self, speed: u32) -> Result<u32, u32> {
        if self.rows.len() != TABLE_GROUPS {
            return Err(GdCode::TableBad.val());
        }
        let first = match self.rows.first() {
            Some(v) => v,
            None => return Err(GdCode::TableBad.val()),
        };
        if speed <= first.speed {
            return Ok(first.gain);
        }
        let mut i = 1usize;
        while i < self.rows.len() {
            let lo = match self.rows.get(i - 1) {
                Some(v) => v,
                None => return Err(GdCode::TableBad.val()),
            };
            let hi = match self.rows.get(i) {
                Some(v) => v,
                None => return Err(GdCode::TableBad.val()),
            };
            if speed <= hi.speed {
                let ds = hi.speed - lo.speed;
                if ds == 0 {
                    return Err(GdCode::TableBad.val());
                }
                let dg = hi.gain - lo.gain;
                let off = speed - lo.speed;
                let num = (dg as u64)
                    .saturating_mul(off as u64)
                    .saturating_add((ds as u64) / 2);
                let add = num / (ds as u64);
                return Ok(lo.gain.saturating_add(add as u32));
            }
            i += 1;
        }
        match self.rows.last() {
            Some(v) => Ok(v.gain),
            None => Err(GdCode::TableBad.val()),
        }
    }
}

/// 第 g 组参数表（g in 0..5）：基增益 1000+200g，五点单调。
pub fn table_of_group(g: u32) -> Result<GainTable, u32> {
    if g >= TABLE_GROUPS as u32 {
        return Err(GdCode::TableBad.val());
    }
    let base = 1000u32.saturating_add(200u32.saturating_mul(g));
    let mut rows = Vec::new();
    rows.push(GainRow { speed: 0, gain: base });
    rows.push(GainRow {
        speed: 250,
        gain: base.saturating_add(200).min(GAIN_MAX),
    });
    rows.push(GainRow {
        speed: 750,
        gain: base.saturating_add(600).min(GAIN_MAX),
    });
    rows.push(GainRow {
        speed: 2000,
        gain: base.saturating_add(1200).min(GAIN_MAX),
    });
    rows.push(GainRow {
        speed: 6000,
        gain: base.saturating_add(2000).min(GAIN_MAX),
    });
    GainTable::new(rows)
}

/// 独立复算（u128 有理式），与查表实现交叉印证。
pub fn probe_gain(table: &GainTable, speed: u32) -> Result<u32, u32> {
    if table.rows.len() != TABLE_GROUPS {
        return Err(GdCode::TableBad.val());
    }
    let first = match table.rows.first() {
        Some(v) => v,
        None => return Err(GdCode::TableBad.val()),
    };
    if speed <= first.speed {
        return Ok(first.gain);
    }
    let mut i = 1usize;
    while i < table.rows.len() {
        let lo = match table.rows.get(i - 1) {
            Some(v) => v,
            None => return Err(GdCode::TableBad.val()),
        };
        let hi = match table.rows.get(i) {
            Some(v) => v,
            None => return Err(GdCode::TableBad.val()),
        };
        if speed <= hi.speed {
            let ds = (hi.speed - lo.speed) as u128;
            let dg = (hi.gain - lo.gain) as u128;
            let off = (speed - lo.speed) as u128;
            let num = dg.saturating_mul(off).saturating_mul(2).saturating_add(ds);
            let den = ds.saturating_mul(2);
            let add = (num / den) as u32;
            return Ok(lo.gain.saturating_add(add));
        }
        i += 1;
    }
    match table.rows.last() {
        Some(v) => Ok(v.gain),
        None => Err(GdCode::TableBad.val()),
    }
}

/// 五组参数表一致性：每组 201 个速度点查表与独立复算一致。
pub fn table_rounds() -> Result<u32, u32> {
    let mut checks = 0u32;
    let mut g = 0u32;
    while g < TABLE_GROUPS as u32 {
        let t = table_of_group(g)?;
        let mut s = 0u32;
        while s <= SPEED_CAP {
            let a = t.lookup(s)?;
            let b = probe_gain(&t, s)?;
            if a != b {
                return Err(GdCode::TableBad.val());
            }
            checks = checks.saturating_add(1);
            s = s.saturating_add(40);
        }
        let mut r = 0usize;
        while r < t.rows.len() {
            let row = match t.rows.get(r) {
                Some(v) => v,
                None => return Err(GdCode::TableBad.val()),
            };
            if t.lookup(row.speed)? != row.gain {
                return Err(GdCode::TableBad.val());
            }
            r += 1;
        }
        g = g.saturating_add(1);
    }
    Ok(checks)
}

// ── 账本 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuardLedger {
    pub lookups: u32,
    pub rejects: u32,
    pub tables: u8,
    pub digest: u64,
}

impl GuardLedger {
    pub fn new() -> GuardLedger {
        GuardLedger {
            lookups: 0,
            rejects: 0,
            tables: 0,
            digest: FNV_OFF,
        }
    }

    pub fn step(&mut self, table: &GainTable, speed: u32, group: u8) -> Result<u32, u32> {
        match table.lookup(speed) {
            Ok(g) => {
                self.lookups = self.lookups.saturating_add(1);
                self.digest = fold_u32(self.digest, speed);
                self.digest = fold_u32(self.digest, g);
                self.digest = fold_u32(self.digest, group as u32);
                self.tables |= 1u8 << (group & 7);
                Ok(g)
            }
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                self.digest = fold_u32(self.digest, e);
                Err(e)
            }
        }
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.lookups == 0 {
            return Err(GdCode::LedgerBad.val());
        }
        if self.tables & 0x1F != 0x1F {
            return Err(GdCode::CoverageBad.val());
        }
        Ok(())
    }
}

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

/// 黄金账：五组 × 200 次查表，返回 (lookups, digest, sum)。
pub fn golden_run() -> Result<(u32, u64, u64), u32> {
    let mut led = GuardLedger::new();
    let mut lcg = Lcg::new(0x4892_5001);
    let mut sum = 0u64;
    let mut i = 0u32;
    while i < 1000 {
        let g = (lcg.below(TABLE_GROUPS as u32)) as u8;
        let table = table_of_group(g as u32)?;
        let speed = lcg.below(SPEED_CAP.saturating_add(1));
        let gain = led.step(&table, speed, g)?;
        sum = sum.saturating_add(gain as u64);
        i = i.saturating_add(1);
    }
    led.audit()?;
    Ok((led.lookups, led.digest, sum))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next() % (n as u64)) as u32
        }
    }
}

// ── 渲染与回读 ────────────────────────────────────────────────────
pub fn render_ledger(l: &GuardLedger) -> String {
    format!(
        "{}|{}|{}|0x{:016X}",
        l.lookups, l.rejects, l.tables, l.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(GdCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(GdCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(GdCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(GdCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(GdCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(GdCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(GdCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(GdCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 4 {
        return Err(GdCode::RenderBad.val());
    }
    let lookups = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(GdCode::RenderBad.val()),
    })?;
    let rejects = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(GdCode::RenderBad.val()),
    })?;
    let tables = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(GdCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(3) {
        Some(v) => v,
        None => return Err(GdCode::RenderBad.val()),
    })?;
    if lookups == 0 || lookups > 10_000_000 || rejects > lookups || tables > 0x1F {
        return Err(GdCode::RenderBad.val());
    }
    if digest == 0 {
        return Err(GdCode::RenderBad.val());
    }
    Ok(lookups)
}

// ── 注入（四路） ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn route(&self, i: usize) -> u32 {
        match self.caught.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    let mut log = InjectLog {
        tries: [rounds; 4],
        caught: [0; 4],
    };
    let mut r = 0u32;
    while r < rounds {
        if audit_mask(0x80).err() == Some(GdCode::MaskBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        if XButtonMap::new(1, 1).err() == Some(GdCode::XMapBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        let mut t = DcTracker::with_default();
        let _ = t.observe(BTN_LEFT, 100, 0, 0);
        if t.observe(BTN_LEFT, 99, 0, 0).err() == Some(GdCode::TimeBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        let mut rows = Vec::new();
        rows.push(GainRow { speed: 0, gain: 1000 });
        rows.push(GainRow {
            speed: 750,
            gain: 1200,
        });
        rows.push(GainRow {
            speed: 250,
            gain: 1400,
        });
        rows.push(GainRow {
            speed: 2000,
            gain: 2000,
        });
        rows.push(GainRow {
            speed: 6000,
            gain: 3000,
        });
        if GainTable::new(rows).err() == Some(GdCode::TableBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    Ok(log)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 探针 A：五键表全覆盖、边沿求解、X 键默认与重映射。
pub fn probe_seq_a() -> Result<(), u32> {
    let rows = five_key_table();
    if audit_five(&rows)? != FIVE_KEYS as u32 {
        return Err(GdCode::CoverageBad.val());
    }
    let (p, r) = transition(0, BTN_LEFT | BTN_X1)?;
    if p != BTN_LEFT | BTN_X1 || r != 0 {
        return Err(GdCode::BtnBad.val());
    }
    let (p2, r2) = transition(BTN_LEFT | BTN_X1, BTN_X2)?;
    if p2 != BTN_X2 || r2 != BTN_LEFT | BTN_X1 {
        return Err(GdCode::BtnBad.val());
    }
    if transition(BTN_LEFT, BTN_LEFT).err() != Some(GdCode::BtnBad.val()) {
        return Err(GdCode::BtnBad.val());
    }
    if audit_mask(0x40).err() != Some(GdCode::MaskBad.val()) {
        return Err(GdCode::MaskBad.val());
    }
    let def = XButtonMap::default_map();
    if def.resolve(1)? != BTN_X1 || def.resolve(2)? != BTN_X2 {
        return Err(GdCode::XMapBad.val());
    }
    let sw = XButtonMap::swapped();
    if sw.resolve(1)? != BTN_X2 || sw.resolve(2)? != BTN_X1 {
        return Err(GdCode::XMapBad.val());
    }
    if sw.resolve(3).err() != Some(GdCode::XMapBad.val()) {
        return Err(GdCode::XMapBad.val());
    }
    if button_name(BTN_LEFT) == button_name(BTN_RIGHT) {
        return Err(GdCode::BtnBad.val());
    }
    Ok(())
}

/// 探针 B：双击双阈值边界、右键不双击、时间回退必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    click_rounds(CLICK_ROUNDS)?;
    let mut t = DcTracker::new(DoubleClickSpec::new(200, 0)?);
    if t.observe(BTN_LEFT, 0, 10, 10)? != ClickKind::Single {
        return Err(GdCode::ClickBad.val());
    }
    if t.observe(BTN_LEFT, 200, 10, 10)? != ClickKind::Double {
        return Err(GdCode::ClickBad.val());
    }
    let mut u = DcTracker::new(DoubleClickSpec::new(200, 0)?);
    if u.observe(BTN_LEFT, 0, 10, 10)? != ClickKind::Single {
        return Err(GdCode::ClickBad.val());
    }
    if u.observe(BTN_LEFT, 0, 10, 10)? != ClickKind::Single {
        return Err(GdCode::ClickBad.val());
    }
    let mut w = DcTracker::new(DoubleClickSpec::new(200, 3)?);
    if w.observe(BTN_LEFT, 0, 0, 0)? != ClickKind::Single {
        return Err(GdCode::ClickBad.val());
    }
    if w.observe(BTN_LEFT, 50, 3, 0)? != ClickKind::Double {
        return Err(GdCode::ClickBad.val());
    }
    let mut v = DcTracker::with_default();
    if v.observe(BTN_LEFT, 10, 0, 0)? != ClickKind::Single {
        return Err(GdCode::ClickBad.val());
    }
    if v.observe(BTN_MIDDLE, 20, 0, 0)? != ClickKind::Single {
        return Err(GdCode::ClickBad.val());
    }
    if v.observe(BTN_MIDDLE, 30, 0, 0)? != ClickKind::Single {
        return Err(GdCode::ClickBad.val());
    }
    if v.observe(BTN_LEFT, 5, 0, 0).err() != Some(GdCode::TimeBad.val()) {
        return Err(GdCode::TimeBad.val());
    }
    if DoubleClickSpec::new(DC_MIN_MS - 1, 0).err() != Some(GdCode::ClickBad.val()) {
        return Err(GdCode::ClickBad.val());
    }
    if DoubleClickSpec::new(DC_DEFAULT_MS, DC_MAX_DIST + 1).err() != Some(GdCode::ClickBad.val()) {
        return Err(GdCode::ClickBad.val());
    }
    Ok(())
}

/// 探针 C：五组参数表交叉复算、黄金双跑一致、渲染回读一致。
pub fn probe_seq_c() -> Result<(), u32> {
    let checks = table_rounds()?;
    if checks != (TABLE_GROUPS as u32).saturating_mul(201) {
        return Err(GdCode::CoverageBad.val());
    }
    let g1 = golden_run()?;
    let g2 = golden_run()?;
    if g1 != g2 {
        return Err(GdCode::ReplayBad.val());
    }
    let mut led = GuardLedger::new();
    let mut gi = 0u8;
    while gi < TABLE_GROUPS as u8 {
        let t = table_of_group(gi as u32)?;
        led.step(&t, 750, gi)?;
        gi = gi.saturating_add(1);
    }
    led.audit()?;
    let text = render_ledger(&led);
    if audit_render(&text)? != led.lookups {
        return Err(GdCode::RenderBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GdCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 1;
    let hi = SEG_BASE + GdCode::ALL.len() as u32;
    let mut i = 0usize;
    while i < GdCode::ALL.len() {
        match GdCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v < lo || v > hi {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let rows = five_key_table();
    let five = audit_five(&rows);
    v.push((
        "F48925-J1-五键×10^2零串",
        five == Ok(FIVE_KEYS as u32) && click_rounds(CLICK_ROUNDS).is_ok(),
        match &five {
            Ok(n) => format!("{} 键全覆盖；双击往返 {} 轮零串", n, CLICK_ROUNDS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48925-J1-边沿求解",
        {
            let a = transition(0, BTN_LEFT | BTN_RIGHT);
            let b = transition(BTN_LEFT | BTN_RIGHT, BTN_MIDDLE);
            a == Ok((BTN_LEFT | BTN_RIGHT, 0)) && b == Ok((BTN_MIDDLE, BTN_LEFT | BTN_RIGHT))
        },
        String::from("按下/释放位互补"),
    ));
    v.push((
        "F48925-J1-X键重映射",
        {
            let d = XButtonMap::default_map();
            let s = XButtonMap::swapped();
            d.resolve(1) == Ok(BTN_X1)
                && s.resolve(1) == Ok(BTN_X2)
                && XButtonMap::new(2, 2).is_err()
                && XButtonMap::new(0, 1).is_err()
        },
        String::from("默认 X1/X2，交换后互异"),
    ));
    let cl = click_rounds(CLICK_ROUNDS);
    v.push((
        "F48926-J1-双击×10^2阈值一致",
        cl.is_ok(),
        match &cl {
            Ok(()) => format!("{} 轮：500ms/4px 内 Double，外 Single", CLICK_ROUNDS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48926-J1-双阈值边界精确",
        {
            let mut t = DcTracker::with_default();
            let s0 = t.observe(BTN_LEFT, 0, 0, 0);
            let d1 = t.observe(BTN_LEFT, DC_DEFAULT_MS, 4, 0);
            let mut u = DcTracker::with_default();
            let s2 = u.observe(BTN_LEFT, 0, 0, 0);
            let s3 = u.observe(BTN_LEFT, DC_DEFAULT_MS + 1, 0, 0);
            s0 == Ok(ClickKind::Single)
                && d1 == Ok(ClickKind::Double)
                && s2 == Ok(ClickKind::Single)
                && s3 == Ok(ClickKind::Single)
        },
        String::from("500ms 与 4px 边界含，501ms 与 5px 不含"),
    ));
    v.push((
        "F48926-J1-右键不双击",
        {
            let mut t = DcTracker::with_default();
            let a = t.observe(BTN_RIGHT, 0, 0, 0);
            let b = t.observe(BTN_RIGHT, 10, 0, 0);
            a == Ok(ClickKind::Single) && b == Ok(ClickKind::Single)
        },
        String::from("仅左键参与双击"),
    ));
    let tr = table_rounds();
    v.push((
        "F48927-J1-参数表×5组校验一致",
        tr == Ok((TABLE_GROUPS as u32).saturating_mul(201)),
        match &tr {
            Ok(n) => format!("5 组 × 201 点 = {} 次复算一致", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let golden = golden_run();
    v.push((
        "F48927-J1-黄金账×10^3",
        match &golden {
            Ok((n, _, _)) => *n == 1000,
            Err(_) => false,
        },
        match &golden {
            Ok((n, d, sum)) => format!("lookups={} digest=0x{:016X} sum={}", n, d, sum),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48925-J1-注入四路10/10",
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
        "F48925-J1-探针三序与双跑",
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
        "F48925-J1-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 {} 码", SEG_BASE, GdCode::ALL.len()),
    ));
    v
}

pub fn run_unx48925_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48925 五键双击与加速参数表");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_keys_cover_exactly() {
        let rows = five_key_table();
        assert_eq!(audit_five(&rows), Ok(5));
        assert_eq!(button_name(BTN_LEFT), "左键");
        assert_eq!(button_name(BTN_X2), "X2");
        assert_eq!(button_name(0x03), "组合或多键");
    }

    #[test]
    fn five_keys_reject_bad_tables() {
        let mut rows = five_key_table();
        if let Some(r) = rows.get_mut(3) {
            r.x_code = 2;
        }
        if let Some(r) = rows.get_mut(4) {
            r.x_code = 2;
        }
        assert_eq!(audit_five(&rows), Err(GdCode::XMapBad.val()));
        let mut short = five_key_table();
        short.pop();
        assert_eq!(audit_five(&short), Err(GdCode::CoverageBad.val()));
    }

    #[test]
    fn transitions_are_exact() {
        assert_eq!(transition(0, BTN_LEFT), Ok((BTN_LEFT, 0)));
        assert_eq!(transition(BTN_LEFT, 0), Ok((0, BTN_LEFT)));
        assert_eq!(
            transition(BTN_LEFT, BTN_LEFT).err(),
            Some(GdCode::BtnBad.val())
        );
        assert_eq!(transition(0, 0).err(), Some(GdCode::BtnBad.val()));
    }

    #[test]
    fn xmap_swaps_logical_codes() {
        let d = XButtonMap::default_map();
        assert_eq!(d.resolve(1), Ok(BTN_X1));
        assert_eq!(d.resolve(2), Ok(BTN_X2));
        let s = XButtonMap::swapped();
        assert_eq!(s.resolve(1), Ok(BTN_X2));
        assert_eq!(s.resolve(2), Ok(BTN_X1));
        assert_eq!(s.resolve(0).err(), Some(GdCode::XMapBad.val()));
    }

    #[test]
    fn double_click_thresholds_are_inclusive() {
        let mut t = DcTracker::with_default();
        assert_eq!(t.observe(BTN_LEFT, 0, 0, 0), Ok(ClickKind::Single));
        assert_eq!(
            t.observe(BTN_LEFT, DC_DEFAULT_MS, 4, 0),
            Ok(ClickKind::Double)
        );
        let mut u = DcTracker::with_default();
        assert_eq!(u.observe(BTN_LEFT, 0, 0, 0), Ok(ClickKind::Single));
        assert_eq!(
            u.observe(BTN_LEFT, DC_DEFAULT_MS, 5, 0),
            Ok(ClickKind::Single)
        );
    }

    #[test]
    fn double_click_only_left() {
        let mut t = DcTracker::with_default();
        assert_eq!(t.observe(BTN_RIGHT, 0, 0, 0), Ok(ClickKind::Single));
        assert_eq!(t.observe(BTN_RIGHT, 10, 0, 0), Ok(ClickKind::Single));
    }

    #[test]
    fn ticks_cannot_go_back() {
        let mut t = DcTracker::with_default();
        assert_eq!(t.observe(BTN_LEFT, 100, 0, 0), Ok(ClickKind::Single));
        assert_eq!(
            t.observe(BTN_LEFT, 99, 0, 0).err(),
            Some(GdCode::TimeBad.val())
        );
    }

    #[test]
    fn tables_reject_bad_shapes() {
        let mut rows = Vec::new();
        rows.push(GainRow { speed: 0, gain: 1000 });
        rows.push(GainRow {
            speed: 750,
            gain: 1200,
        });
        rows.push(GainRow {
            speed: 250,
            gain: 1400,
        });
        rows.push(GainRow {
            speed: 2000,
            gain: 2000,
        });
        rows.push(GainRow {
            speed: 6000,
            gain: 3000,
        });
        assert_eq!(GainTable::new(rows).err(), Some(GdCode::TableBad.val()));
        let mut short = Vec::new();
        short.push(GainRow { speed: 0, gain: 1000 });
        assert!(GainTable::new(short).is_err());
    }

    #[test]
    fn table_groups_cross_check() {
        assert_eq!(table_rounds(), Ok(5 * 201));
        match table_of_group(0) {
            Ok(t) => {
                assert_eq!(t.lookup(0), Ok(1000));
                assert_eq!(t.lookup(250), Ok(1200));
                assert_eq!(t.lookup(750), Ok(1600));
                assert_eq!(t.lookup(10_000), Ok(3000));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_is_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn injection_catches_all_four() {
        match inject_rounds(6) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [6, 6, 6, 6]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_pass() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn render_roundtrip() {
        let mut led = GuardLedger::new();
        match table_of_group(1) {
            Ok(t) => {
                let _ = led.step(&t, 300, 1);
                let text = render_ledger(&led);
                assert!(audit_render(&text).is_ok());
                assert!(led.audit().is_err());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn codes_inside_window() {
        assert!(segment_guard());
        assert_eq!(GdCode::ALL.len(), 12);
        assert_eq!(detail(GdCode::TableBad.val()), GdCode::TableBad.detail());
        assert_eq!(detail(0), "未知诊断码");
    }
}
