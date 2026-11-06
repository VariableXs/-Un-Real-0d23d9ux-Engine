//! UNX-F50089 字体与 GDI 管线共用联签（B06 判据主轴）（0x43740 段）
//!
//! 锚点：UNX-F50089-J1 主轴联签登记；判据锚定 §AI-63 §3。
//!
//! 数据段 `SEG_BASE = 0x43740`：诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 10`，
//! 逐项见 [`GdiError`]。
//!
//! 字体栈与 GDI 管线各持半张签名，只有两半都落在同一条「主轴」上，联签才算
//! 成立。主轴由 [`AxisId`]（字体号 + epoch + 渲染模式）与 [`AxisSpec`]（缩放、
//! DPI、设计度量）共同定义；两半各自把主轴摘要 `joint` 写进签名，注册表再
//! 复算一遍——任何一栏被改写（字体表哈希、GDI 状态哈希、主轴字段）都会让
//! 摘要对不上，当场判 [`GdiError::Digest`] 并把该轴封为 Broken，不允许“先签
//! 后改”蒙混过关。
//!
//! 结构清单：
//! * [`RenderMode`]/[`AxisId`]/[`AxisSpec`]：主轴三要素与摘要输入；
//! * [`Side`]/[`HalfSign`]：两半签名（含 joint、payload 哈希、复算摘要、tick）；
//! * [`AxisState`] 状态机：Open→FontHalf→CoSigned，或任意校验失败→Broken；
//! * [`CoSignRegistry`] 注册表：登记、按轴推进、去重、容量、窗口期、封账审计；
//! * [`CoSign`] 联签记录：两半摘要 + 主轴摘要 + 时刻，可审计可重放；
//! * 注入核 [`injection_core`]：十类坏样本逐类断言具体诊断码。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43740 独占 0x20）。
pub const SEG_BASE: u32 = 0x43740;

pub const E_EMPTY: u16 = 1;
pub const E_UNKNOWN_AXIS: u16 = 2;
pub const E_OUT_OF_ORDER: u16 = 3;
pub const E_DUPLICATE: u16 = 4;
pub const E_DIGEST: u16 = 5;
pub const E_STALE: u16 = 6;
pub const E_FULL: u16 = 7;
pub const E_BADMODE: u16 = 8;
pub const E_NOT_COSIGNED: u16 = 9;
pub const E_RANGE: u16 = 10;

/// 联签错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GdiError {
    Empty,
    UnknownAxis,
    OutOfOrder,
    Duplicate,
    Digest,
    Stale,
    Full,
    BadMode,
    NotCosigned,
    Range,
}

impl GdiError {
    pub fn code(self) -> u32 {
        let n = match self {
            GdiError::Empty => E_EMPTY,
            GdiError::UnknownAxis => E_UNKNOWN_AXIS,
            GdiError::OutOfOrder => E_OUT_OF_ORDER,
            GdiError::Duplicate => E_DUPLICATE,
            GdiError::Digest => E_DIGEST,
            GdiError::Stale => E_STALE,
            GdiError::Full => E_FULL,
            GdiError::BadMode => E_BADMODE,
            GdiError::NotCosigned => E_NOT_COSIGNED,
            GdiError::Range => E_RANGE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            GdiError::Empty => "axis has no font id",
            GdiError::UnknownAxis => "axis not registered for co-sign",
            GdiError::OutOfOrder => "font half must precede gdi half",
            GdiError::Duplicate => "half already submitted for this axis",
            GdiError::Digest => "joint or half digest mismatch",
            GdiError::Stale => "signature older than co-sign window",
            GdiError::Full => "co-sign registry capacity exhausted",
            GdiError::BadMode => "render mode code outside 0..2",
            GdiError::NotCosigned => "axis has no completed co-sign",
            GdiError::Range => "axis spec outside legal range",
        }
    }
}

/// 渲染模式。三种模式的 GDI 状态不同，联签必须按模式分区。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    Text,
    GlyphRun,
    Path,
}

impl RenderMode {
    pub fn code(self) -> u8 {
        match self {
            RenderMode::Text => 0,
            RenderMode::GlyphRun => 1,
            RenderMode::Path => 2,
        }
    }

    pub fn from_code(c: u8) -> Result<RenderMode, GdiError> {
        match c {
            0 => Ok(RenderMode::Text),
            1 => Ok(RenderMode::GlyphRun),
            2 => Ok(RenderMode::Path),
            _ => Err(GdiError::BadMode),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RenderMode::Text => "text",
            RenderMode::GlyphRun => "glyph-run",
            RenderMode::Path => "path",
        }
    }
}

/// 主轴身份：换字体、换 epoch、换模式都必须重新联签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisId {
    pub font: u16,
    pub epoch: u32,
    pub mode: RenderMode,
}

impl AxisId {
    pub const fn new(font: u16, epoch: u32, mode: RenderMode) -> AxisId {
        AxisId { font, epoch, mode }
    }

    pub fn label(&self) -> String {
        format!("f{}@e{}:{}", self.font, self.epoch, self.mode.label())
    }
}

/// 主轴规格：两侧共同承认的渲染参数。摘要只由这里派生，避免各算各的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisSpec {
    pub id: AxisId,
    pub upem: u16,
    pub ascent: i16,
    pub descent: i16,
    pub glyphs: u16,
    pub dpi: u16,
    pub scale_num: u16,
    pub scale_den: u16,
}

impl AxisSpec {
    /// 构造并校验主轴。font=0 是无主轴哨兵；dpi/缩放为 0 会让摘要无法复算。
    pub fn new(
        id: AxisId,
        upem: u16,
        ascent: i16,
        descent: i16,
        glyphs: u16,
        dpi: u16,
        scale_num: u16,
        scale_den: u16,
    ) -> Result<AxisSpec, GdiError> {
        let spec = AxisSpec {
            id,
            upem,
            ascent,
            descent,
            glyphs,
            dpi,
            scale_num,
            scale_den,
        };
        spec.validate()?;
        Ok(spec)
    }

    /// 字段合法性。所有非法值都在登记前拒绝，摘要函数因此无需再防 0。
    pub fn validate(&self) -> Result<(), GdiError> {
        if self.id.font == 0 {
            return Err(GdiError::Empty);
        }
        if self.upem < 16 || self.upem > 16384 {
            return Err(GdiError::Range);
        }
        if self.ascent <= 0 || self.descent > 0 || self.glyphs == 0 {
            return Err(GdiError::Range);
        }
        if self.dpi == 0 || self.scale_num == 0 || self.scale_den == 0 {
            return Err(GdiError::Range);
        }
        Ok(())
    }

    /// 主轴摘要（FNV-1a over 固定小端字段序）。同一规格必须逐位一致。
    pub fn joint_digest(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::with_capacity(24);
        buf.extend_from_slice(&self.id.font.to_le_bytes());
        buf.extend_from_slice(&self.id.epoch.to_le_bytes());
        buf.push(self.id.mode.code());
        buf.extend_from_slice(&self.upem.to_le_bytes());
        buf.extend_from_slice(&self.ascent.to_le_bytes());
        buf.extend_from_slice(&self.descent.to_le_bytes());
        buf.extend_from_slice(&self.glyphs.to_le_bytes());
        buf.extend_from_slice(&self.dpi.to_le_bytes());
        buf.extend_from_slice(&self.scale_num.to_le_bytes());
        buf.extend_from_slice(&self.scale_den.to_le_bytes());
        fnv1a32(&buf)
    }
}

/// FNV-1a 32 位。内核不引入哈希库，固定向量可独立复算。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h = 0x811c9dc5u32;
    let mut i = 0usize;
    while i < data.len() {
        if let Some(b) = data.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        i += 1;
    }
    h
}

/// 签名方。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Font,
    Gdi,
}

impl Side {
    pub fn code(self) -> u8 {
        match self {
            Side::Font => b'F',
            Side::Gdi => b'G',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Font => "font",
            Side::Gdi => "gdi",
        }
    }
}

/// 半张签名。`payload` 是签名方自有的状态哈希（字体表 / GDI 状态），
/// `digest` 由 (side, joint, payload) 复算，注册表据此识别篡改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HalfSign {
    pub axis: AxisId,
    pub side: Side,
    pub joint: u32,
    pub payload: u32,
    pub digest: u32,
    pub tick: u64,
}

/// 复算半张签名的摘要。公开给调用方自检，注册表内部用同一函数。
pub fn half_digest(side: Side, joint: u32, payload: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::with_capacity(9);
    buf.push(side.code());
    buf.extend_from_slice(&joint.to_le_bytes());
    buf.extend_from_slice(&payload.to_le_bytes());
    fnv1a32(&buf)
}

/// 字体侧签名：载荷是字体表哈希。
pub fn font_half(spec: &AxisSpec, font_table_hash: u32, tick: u64) -> HalfSign {
    let joint = spec.joint_digest();
    HalfSign {
        axis: spec.id,
        side: Side::Font,
        joint,
        payload: font_table_hash,
        digest: half_digest(Side::Font, joint, font_table_hash),
        tick,
    }
}

/// GDI 侧签名：载荷是 GDI 管线状态哈希。
pub fn gdi_half(spec: &AxisSpec, gdi_state_hash: u32, tick: u64) -> HalfSign {
    let joint = spec.joint_digest();
    HalfSign {
        axis: spec.id,
        side: Side::Gdi,
        joint,
        payload: gdi_state_hash,
        digest: half_digest(Side::Gdi, joint, gdi_state_hash),
        tick,
    }
}

/// 单轴联签状态机。Broken 是吸收态：一次摘要不符就再也不放行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisState {
    Open,
    FontHalf,
    CoSigned,
    Broken,
}

impl AxisState {
    pub fn label(self) -> &'static str {
        match self {
            AxisState::Open => "open",
            AxisState::FontHalf => "font-half",
            AxisState::CoSigned => "cosigned",
            AxisState::Broken => "broken",
        }
    }
}

/// 完成后的联签记录。两半摘要都保留，便于事后复核是谁改了哪一侧。
#[derive(Clone, Debug, PartialEq)]
pub struct CoSign {
    pub axis: AxisId,
    pub joint: u32,
    pub font_digest: u32,
    pub gdi_digest: u32,
    pub font_tick: u64,
    pub gdi_tick: u64,
    pub mode: RenderMode,
}

/// 单轴槽位：规格 + 状态 + 两半签名。
#[derive(Clone, Debug)]
pub struct AxisSlot {
    pub spec: AxisSpec,
    pub state: AxisState,
    pub font: Option<HalfSign>,
    pub gdi: Option<HalfSign>,
    pub last_tick: u64,
}

/// 联签注册表。容量按轴计，联签记录与轴一一对应。
#[derive(Clone, Debug)]
pub struct CoSignRegistry {
    slots: Vec<AxisSlot>,
    cosigns: Vec<CoSign>,
    capacity: usize,
    deadline: u64,
    pub registered: u32,
    pub rejected: u32,
}

impl CoSignRegistry {
    pub fn new(capacity: usize, deadline: u64) -> CoSignRegistry {
        CoSignRegistry {
            slots: Vec::new(),
            cosigns: Vec::new(),
            capacity,
            deadline,
            registered: 0,
            rejected: 0,
        }
    }

    pub fn deadline(&self) -> u64 {
        self.deadline
    }

    /// 登记一条主轴。重复轴与超容都拒绝，失败不留半个槽位。
    pub fn register(&mut self, spec: AxisSpec) -> Result<(), GdiError> {
        if self.slots.len() >= self.capacity {
            self.rejected += 1;
            return Err(GdiError::Full);
        }
        if self.slot_index(&spec.id).is_some() {
            self.rejected += 1;
            return Err(GdiError::Duplicate);
        }
        self.slots.push(AxisSlot {
            spec,
            state: AxisState::Open,
            font: None,
            gdi: None,
            last_tick: 0,
        });
        self.registered += 1;
        Ok(())
    }

    fn slot_index(&self, axis: &AxisId) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(slot) = self.slots.get(i) {
                if &slot.spec.id == axis {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn check_half(&self, slot: &AxisSlot, half: &HalfSign) -> Result<(), GdiError> {
        match slot.state {
            AxisState::Broken => return Err(GdiError::OutOfOrder),
            AxisState::CoSigned => return Err(GdiError::Duplicate),
            _ => {}
        }
        let expect_joint = slot.spec.joint_digest();
        if half.joint != expect_joint {
            return Err(GdiError::Digest);
        }
        if half.digest != half_digest(half.side, half.joint, half.payload) {
            return Err(GdiError::Digest);
        }
        if half.tick > self.deadline {
            return Err(GdiError::Stale);
        }
        match half.side {
            Side::Font => {
                if slot.font.is_some() {
                    return Err(GdiError::Duplicate);
                }
                if slot.state != AxisState::Open {
                    return Err(GdiError::OutOfOrder);
                }
            }
            Side::Gdi => {
                if slot.gdi.is_some() {
                    return Err(GdiError::Duplicate);
                }
                if slot.state != AxisState::FontHalf {
                    return Err(GdiError::OutOfOrder);
                }
                if let Some(f) = slot.font {
                    if half.tick < f.tick {
                        return Err(GdiError::OutOfOrder);
                    }
                } else {
                    return Err(GdiError::OutOfOrder);
                }
            }
        }
        Ok(())
    }

    /// 提交半张签名。校验全部通过才落状态；Digest 失败会把轴封为 Broken。
    pub fn submit(&mut self, half: HalfSign) -> Result<AxisState, GdiError> {
        let idx = match self.slot_index(&half.axis) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(GdiError::UnknownAxis);
            }
        };
        let verdict = match self.slots.get(idx) {
            Some(slot) => self.check_half(slot, &half),
            None => Err(GdiError::UnknownAxis),
        };
        if let Err(e) = verdict {
            self.rejected += 1;
            if e == GdiError::Digest {
                if let Some(slot) = self.slots.get_mut(idx) {
                    slot.state = AxisState::Broken;
                }
            }
            return Err(e);
        }
        let mut cosigned_now = false;
        let mut built: Option<CoSign> = None;
        if let Some(slot) = self.slots.get_mut(idx) {
            if half.tick > slot.last_tick {
                slot.last_tick = half.tick;
            }
            match half.side {
                Side::Font => {
                    slot.font = Some(half);
                    slot.state = AxisState::FontHalf;
                }
                Side::Gdi => {
                    slot.gdi = Some(half);
                    slot.state = AxisState::CoSigned;
                    cosigned_now = true;
                    if let (Some(f), Some(g)) = (slot.font, slot.gdi) {
                        built = Some(CoSign {
                            axis: slot.spec.id,
                            joint: slot.spec.joint_digest(),
                            font_digest: f.digest,
                            gdi_digest: g.digest,
                            font_tick: f.tick,
                            gdi_tick: g.tick,
                            mode: slot.spec.id.mode,
                        });
                    }
                }
            }
        }
        if let Some(c) = built {
            self.cosigns.push(c);
        }
        if cosigned_now {
            Ok(AxisState::CoSigned)
        } else {
            Ok(AxisState::FontHalf)
        }
    }

    /// 取联签记录。未完成（含 Broken）一律 NotCosigned，调用方不得自行拼装。
    pub fn cosign(&self, axis: &AxisId) -> Result<&CoSign, GdiError> {
        let mut i = 0usize;
        while i < self.cosigns.len() {
            if let Some(c) = self.cosigns.get(i) {
                if &c.axis == axis {
                    return Ok(c);
                }
            }
            i += 1;
        }
        Err(GdiError::NotCosigned)
    }

    pub fn cosign_count(&self) -> usize {
        self.cosigns.len()
    }

    /// 联签记录数必须与 CoSigned 槽位数相等（一一对应，不多不少）。
    pub fn cosigns_match_states(&self) -> bool {
        self.cosigns.len() == self.count_state(AxisState::CoSigned)
    }

    pub fn count_state(&self, state: AxisState) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(slot) = self.slots.get(i) {
                if slot.state == state {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 审计：登记数 = 四态之和；每条联签都能在槽位里找到同名主轴且摘要一致。
    pub fn audit(&self) -> bool {
        let mut sum = 0usize;
        sum += self.count_state(AxisState::Open);
        sum += self.count_state(AxisState::FontHalf);
        sum += self.count_state(AxisState::CoSigned);
        sum += self.count_state(AxisState::Broken);
        if sum != self.slots.len() || sum != self.registered as usize {
            return false;
        }
        if self.cosigns.len() != self.count_state(AxisState::CoSigned) {
            return false;
        }
        let mut i = 0usize;
        while i < self.cosigns.len() {
            if let Some(c) = self.cosigns.get(i) {
                let idx = match self.slot_index(&c.axis) {
                    Some(v) => v,
                    None => return false,
                };
                let slot = match self.slots.get(idx) {
                    Some(v) => v,
                    None => return false,
                };
                if c.joint != slot.spec.joint_digest() || slot.state != AxisState::CoSigned {
                    return false;
                }
                match (slot.font, slot.gdi) {
                    (Some(f), Some(g)) => {
                        if c.font_digest != f.digest || c.gdi_digest != g.digest {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            i += 1;
        }
        true
    }

    pub fn summary(&self) -> String {
        format!(
            "registered={} cosigned={} open={} broken={} rejected={}",
            self.registered,
            self.count_state(AxisState::CoSigned),
            self.count_state(AxisState::Open),
            self.count_state(AxisState::Broken),
            self.rejected
        )
    }
}

// ---------------------------------------------------------------------------
// 注入核：十类坏样本
// ---------------------------------------------------------------------------

fn spec_for(font: u16) -> AxisSpec {
    match AxisSpec::new(AxisId::new(font, 1, RenderMode::Text), 1000, 800, -200, 512, 96, 1, 1) {
        Ok(s) => s,
        Err(_) => AxisSpec {
            id: AxisId::new(1, 1, RenderMode::Text),
            upem: 1000,
            ascent: 800,
            descent: -200,
            glyphs: 1,
            dpi: 96,
            scale_num: 1,
            scale_den: 1,
        },
    }
}

/// 注入核。返回（被抓数, 总数），每条对照具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    // 1. 未知主轴
    let mut r = CoSignRegistry::new(8, 1_000);
    let ghost = spec_for(77);
    if r.submit(font_half(&ghost, 0x11, 10)).err() == Some(GdiError::UnknownAxis) {
        caught += 1;
    }
    // 2. GDI 先于字体
    let mut order = CoSignRegistry::new(8, 1_000);
    let s1 = spec_for(1);
    let reg1 = order.register(s1).is_ok();
    if reg1 && order.submit(gdi_half(&s1, 0x22, 10)).err() == Some(GdiError::OutOfOrder) {
        caught += 1;
    }
    // 3. 字体半张重复
    let mut dup = CoSignRegistry::new(8, 1_000);
    let s2 = spec_for(2);
    let rd = dup.register(s2).is_ok();
    let f = font_half(&s2, 0x33, 10);
    let first = dup.submit(f).is_ok();
    if rd && first && dup.submit(f).err() == Some(GdiError::Duplicate) {
        caught += 1;
    }
    // 4. 半张摘要被篡改
    let mut tamper = CoSignRegistry::new(8, 1_000);
    let s3 = spec_for(3);
    let rt = tamper.register(s3).is_ok();
    let mut fh = font_half(&s3, 0x44, 10);
    fh.digest ^= 0xffff_ffff;
    if rt && tamper.submit(fh).err() == Some(GdiError::Digest) {
        caught += 1;
    }
    // 5. 主轴摘要被篡改（两半都错）
    let mut joint = CoSignRegistry::new(8, 1_000);
    let s4 = spec_for(4);
    let rj = joint.register(s4).is_ok();
    let mut jh = font_half(&s4, 0x55, 10);
    jh.joint ^= 1;
    if rj && joint.submit(jh).err() == Some(GdiError::Digest) {
        caught += 1;
    }
    // 6. 超窗期签名
    let mut stale = CoSignRegistry::new(8, 100);
    let s5 = spec_for(5);
    let rs = stale.register(s5).is_ok();
    if rs && stale.submit(font_half(&s5, 0x66, 500)).err() == Some(GdiError::Stale) {
        caught += 1;
    }
    // 7. 容量满
    let mut full = CoSignRegistry::new(1, 1_000);
    let rf = full.register(spec_for(6)).is_ok();
    if rf && full.register(spec_for(7)).err() == Some(GdiError::Full) {
        caught += 1;
    }
    // 8. 模式码越界
    if RenderMode::from_code(7).err() == Some(GdiError::BadMode) {
        caught += 1;
    }
    // 9. 无字体号
    let empty = AxisSpec::new(AxisId::new(0, 1, RenderMode::Text), 1000, 800, -200, 8, 96, 1, 1);
    if empty.err() == Some(GdiError::Empty) {
        caught += 1;
    }
    // 10. DPI 为 0
    let range = AxisSpec::new(AxisId::new(9, 1, RenderMode::Text), 1000, 800, -200, 8, 0, 1, 1);
    if range.err() == Some(GdiError::Range) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

const MODES: [RenderMode; 3] = [RenderMode::Text, RenderMode::GlyphRun, RenderMode::Path];

fn axis_spec(i: u16) -> AxisSpec {
    match AxisSpec::new(
        AxisId::new(100 + i, 1, MODES[(i as usize) % 3]),
        1000 + i,
        800,
        -200,
        256 + i,
        96 + (i % 2),
        1,
        1,
    ) {
        Ok(s) => s,
        Err(_) => spec_for(1),
    }
}

pub fn run_unx50089_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50089");
    let mut registry = CoSignRegistry::new(16, 10_000);
    let mut specs: Vec<AxisSpec> = Vec::with_capacity(8);
    let mut i = 0u16;
    while i < 8 {
        let spec = axis_spec(i);
        let _ = registry.register(spec);
        let f = font_half(&spec, 0x1000 + i as u32, 10 + i as u64);
        let first = registry.submit(f);
        let g = gdi_half(&spec, 0x2000 + i as u32, 20 + i as u64);
        let second = registry.submit(g);
        if first.is_ok() && second == Ok(AxisState::CoSigned) {
            specs.push(spec);
        }
        i += 1;
    }

    // J1-1 主轴联签登记：8 轴全部完成，记录与主轴一一对应
    let mut all_ok = specs.len() == 8;
    let mut j = 0usize;
    while j < specs.len() {
        if let Some(spec) = specs.get(j) {
            match registry.cosign(&spec.id) {
                Ok(c) => {
                    if c.joint != spec.joint_digest() || c.mode != spec.id.mode {
                        all_ok = false;
                    }
                }
                Err(_) => all_ok = false,
            }
        }
        j += 1;
    }
    let ok = all_ok && registry.cosign_count() == 8 && registry.audit();
    let det = format!(
        "cosigned={} registered={}",
        registry.cosign_count(),
        registry.registered
    );
    s.add_dyn("50089-J1-主轴联签登记", ok, &det);

    // J1-2 两半摘要都入档，且与各自半张签名逐位一致
    let mut pair_ok = true;
    let mut k = 0usize;
    while k < specs.len() {
        if let Some(spec) = specs.get(k) {
            if let Ok(c) = registry.cosign(&spec.id) {
                let f = font_half(spec, 0x1000 + k as u32, 10 + k as u64);
                let g = gdi_half(spec, 0x2000 + k as u32, 20 + k as u64);
                if c.font_digest != f.digest || c.gdi_digest != g.digest || c.font_tick != f.tick {
                    pair_ok = false;
                }
            }
        }
        k += 1;
    }
    s.add("50089-J1-两半入档", pair_ok, if pair_ok { "" } else { "联签摘要不符" });

    // J1-3 主轴摘要：确定性 + 8 轴互不碰撞
    let mut distinct = 0usize;
    let mut a = 0usize;
    while a < specs.len() {
        if let Some(spec) = specs.get(a) {
            let d1 = spec.joint_digest();
            let d2 = spec.joint_digest();
            let mut collide = false;
            let mut b = 0usize;
            while b < specs.len() {
                if b != a {
                    if let Some(other) = specs.get(b) {
                        if other.joint_digest() == d1 {
                            collide = true;
                        }
                    }
                }
                b += 1;
            }
            if d1 == d2 && !collide {
                distinct += 1;
            }
        }
        a += 1;
    }
    let ok = distinct == 8;
    let det = format!("distinct={}", distinct);
    s.add_dyn("50089-J1-主轴摘要", ok, &det);

    // J1-4 FNV-1a 独立参考向量
    let ok = fnv1a32(b"") == 0x811c9dc5
        && fnv1a32(b"a") == 0xe40c292c
        && fnv1a32(b"foobar") == 0xbf9cf968;
    let det = format!(
        "{:08x}/{:08x}/{:08x}",
        fnv1a32(b""),
        fnv1a32(b"a"),
        fnv1a32(b"foobar")
    );
    s.add_dyn("50089-J1-FNV向量", ok, &det);

    // J1-5 GDI 先签拒绝，轴停在 Open，不留半张
    let mut early = CoSignRegistry::new(4, 1_000);
    let es = spec_for(21);
    let er = early.register(es);
    let eg = early.submit(gdi_half(&es, 0x77, 5));
    let ok = er.is_ok()
        && eg.err() == Some(GdiError::OutOfOrder)
        && early.count_state(AxisState::Open) == 1
        && early.rejected == 1
        && early.audit();
    s.add("50089-J1-GDI先签拒", ok, if ok { "" } else { "先签被收" });

    // J1-6 重复半张拒绝，状态与计数守恒
    let mut dup = CoSignRegistry::new(4, 1_000);
    let ds = spec_for(22);
    let dr = dup.register(ds);
    let f = font_half(&ds, 0x88, 5);
    let f1 = dup.submit(f);
    let f2 = dup.submit(f);
    let ok = dr.is_ok()
        && f1 == Ok(AxisState::FontHalf)
        && f2.err() == Some(GdiError::Duplicate)
        && dup.count_state(AxisState::FontHalf) == 1
        && dup.rejected == 1
        && dup.audit();
    s.add("50089-J1-重复半张拒", ok, if ok { "" } else { "重复半张被收" });

    // J1-7 已联签后再提交 → Duplicate，记录数不变
    let target = match specs.first() {
        Some(v) => *v,
        None => spec_for(1),
    };
    let before = registry.cosign_count();
    let err = registry.submit(font_half(&target, 0x1000, 30));
    let ok = err.err() == Some(GdiError::Duplicate) && registry.cosign_count() == before;
    s.add("50089-J1-联签后拒再签", ok, if ok { "" } else { "联签后仍被改写" });

    // J1-8 字体侧摘要篡改 → Digest + Broken + NotCosigned
    let mut tam = CoSignRegistry::new(4, 1_000);
    let ts = spec_for(23);
    let tr = tam.register(ts);
    let mut tf = font_half(&ts, 0x99, 5);
    tf.payload ^= 0x1;
    let terr = tam.submit(tf);
    let ok = tr.is_ok()
        && terr.err() == Some(GdiError::Digest)
        && tam.count_state(AxisState::Broken) == 1
        && tam.cosign(&ts.id).err() == Some(GdiError::NotCosigned)
        && tam.submit(gdi_half(&ts, 0xaa, 6)).err() == Some(GdiError::OutOfOrder);
    s.add("50089-J1-摘要篡改封轴", ok, if ok { "" } else { "篡改未被封轴" });

    // J1-9 主轴摘要篡改 → Digest（两半自洽也没用，必须与规格一致）
    let mut jt = CoSignRegistry::new(4, 1_000);
    let js = spec_for(24);
    let jr = jt.register(js);
    let mut jf = font_half(&js, 0xbb, 5);
    jf.joint ^= 0x8000_0000;
    let jg = gdi_half(&js, 0xcc, 6);
    let ok = jr.is_ok()
        && jt.submit(jf).err() == Some(GdiError::Digest)
        && jt.submit(jg).err() == Some(GdiError::OutOfOrder)
        && jt.count_state(AxisState::CoSigned) == 0;
    s.add("50089-J1-主轴篡改拒", ok, if ok { "" } else { "主轴篡改被放过" });

    // J1-10 未知主轴与超窗各自可判
    let mut unk = CoSignRegistry::new(4, 1_000);
    let us = spec_for(25);
    let u1 = unk.submit(font_half(&us, 0xdd, 1)).err();
    let mut st = CoSignRegistry::new(4, 50);
    let ss = spec_for(26);
    let sr = st.register(ss);
    let s1 = st.submit(font_half(&ss, 0xee, 60)).err();
    let ok = u1 == Some(GdiError::UnknownAxis) && sr.is_ok() && s1 == Some(GdiError::Stale);
    s.add("50089-J1-未知与超窗", ok, if ok { "" } else { "未知/超窗未判" });

    // J1-11 容量满与规格校验
    let mut cap = CoSignRegistry::new(1, 1_000);
    let c1 = cap.register(spec_for(27)).is_ok();
    let c2 = cap.register(spec_for(28)).err();
    let empty = AxisSpec::new(AxisId::new(0, 1, RenderMode::Text), 1000, 800, -200, 8, 96, 1, 1).err();
    let range = AxisSpec::new(AxisId::new(9, 1, RenderMode::Text), 1000, 800, -200, 8, 96, 0, 1).err();
    let badmode = RenderMode::from_code(9).err();
    let ok = c1
        && c2 == Some(GdiError::Full)
        && empty == Some(GdiError::Empty)
        && range == Some(GdiError::Range)
        && badmode == Some(GdiError::BadMode)
        && cap.registered == 1
        && cap.audit();
    s.add("50089-J1-容量与规格", ok, if ok { "" } else { "容量/规格校验失效" });

    // J1-12 状态计数守恒：registered = 四态之和，且联签数 = CoSigned 数
    let ok = registry.audit()
        && registry.registered as usize
            == registry.count_state(AxisState::Open)
                + registry.count_state(AxisState::FontHalf)
                + registry.count_state(AxisState::CoSigned)
                + registry.count_state(AxisState::Broken)
        && registry.cosigns_match_states();
    let det = format!(
        "reg={} cosign={}",
        registry.registered,
        registry.cosign_count()
    );
    s.add_dyn("50089-J1-状态守恒", ok, &det);

    // J1-13 重放确定性：同一提交序列在两个注册表产生相同记录
    let mut ra = CoSignRegistry::new(4, 1_000);
    let mut rb = CoSignRegistry::new(4, 1_000);
    let rspec = spec_for(29);
    let _ = ra.register(rspec);
    let _ = rb.register(rspec);
    let _ = ra.submit(font_half(&rspec, 0x1234, 3));
    let _ = ra.submit(gdi_half(&rspec, 0x5678, 4));
    let _ = rb.submit(font_half(&rspec, 0x1234, 3));
    let _ = rb.submit(gdi_half(&rspec, 0x5678, 4));
    let ok = match (ra.cosign(&rspec.id), rb.cosign(&rspec.id)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    };
    s.add("50089-J1-重放确定", ok, if ok { "" } else { "重放结果不一致" });

    // J1-14 摘要与审计文本非空
    let text = registry.summary();
    let ok = !text.is_empty()
        && text.contains("registered=8")
        && text.contains("cosigned=8")
        && text.contains("rejected=");
    s.add("50089-J1-摘要非空", ok, if ok { "" } else { "摘要缺字段" });

    // J1-15 错误码闭集：十码连续、消息非空
    let all = [
        GdiError::Empty,
        GdiError::UnknownAxis,
        GdiError::OutOfOrder,
        GdiError::Duplicate,
        GdiError::Digest,
        GdiError::Stale,
        GdiError::Full,
        GdiError::BadMode,
        GdiError::NotCosigned,
        GdiError::Range,
    ];
    let mut e = 0usize;
    let mut ok = true;
    while e < all.len() {
        if let Some(err) = all.get(e) {
            if err.code() != SEG_BASE + (e as u32) + 1 || err.message().is_empty() {
                ok = false;
            }
        }
        e += 1;
    }
    s.add("50089-J1-码值闭集", ok, if ok { "" } else { "码值漂移" });

    // J1-16 注入核十类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50089-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50089_checks_green() {
        let set = run_unx50089_checks();
        assert!(set.all_passed(), "UNX-F50089 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50089_cosign_binds_both_halves() {
        let spec = axis_spec(0);
        let mut r = CoSignRegistry::new(2, 1_000);
        assert!(r.register(spec).is_ok());
        assert_eq!(r.submit(font_half(&spec, 1, 1)), Ok(AxisState::FontHalf));
        assert_eq!(r.submit(gdi_half(&spec, 2, 2)), Ok(AxisState::CoSigned));
        match r.cosign(&spec.id) {
            Ok(c) => {
                assert_eq!(c.joint, spec.joint_digest());
                assert_eq!(c.mode, spec.id.mode);
            }
            Err(_) => panic!("co-sign must exist"),
        }
        assert!(r.audit());
    }

    #[test]
    fn unx50089_tamper_breaks_axis() {
        let spec = axis_spec(1);
        let mut r = CoSignRegistry::new(2, 1_000);
        assert!(r.register(spec).is_ok());
        let mut f = font_half(&spec, 1, 1);
        f.digest = 0;
        assert_eq!(r.submit(f).err(), Some(GdiError::Digest));
        assert_eq!(r.count_state(AxisState::Broken), 1);
    }
}
