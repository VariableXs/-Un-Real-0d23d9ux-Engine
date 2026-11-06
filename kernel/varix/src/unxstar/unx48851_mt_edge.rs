//! UNX-F48851 触控设备 quirk 表骨架 · quirk 外降级口径 · 0x70640
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48851 ① 纯功能代码 ≥280 行；
//!          ② UNX-F48851-J1 骨架表 ×10 项命中正确；
//!   F48852 ① 纯功能代码 ≥280 行；
//!          ② UNX-F48852-J1 降级声明落账 ×10 设备如实记账零虚报。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：行自洽 → 区间不叠 → 三键精确命中 → 参数覆盖在界 →
//!           降级声明前置 → 手势承诺核验 → 账本对账。
//!   次序不可倒：行未自洽就查表，fw 区间倒挂会让任何版本都命中；
//!   区间叠不拒绝，后一行会静默遮蔽前一行；降级声明必须先于手势
//!   承诺核验，否则「没声明过就放行」会变成默认承诺。
//! 关键差异：本模块只决定「这台设备用哪套参数」与「对 quirk 外设备
//!   承诺到什么程度」。真实的轴值解析在 unx48845，手势判定在 B10；
//!   本模块不读事件流、不拥有触点，只输出参数表与降级声明。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70640，VX6 批 B03 第五槽）。
pub const SEG_BASE: u32 = 0x70640;

/// quirk 表行数（判据 ×10）。
pub const QUIRK_ROWS: usize = 10;
/// 参数覆盖上限：单帧并发触点。
pub const TOUCH_CAP: u8 = 5;
/// 归一压力上限（0..=1000，与 unx48845 的 SCALE_FULL 同口径）。
pub const PRESSURE_FULL: u16 = 1000;
/// slot 步进合法值。
pub const STRIDE_MAX: u8 = 2;
/// 降级声明台数（判据 ×10）。
pub const DEGRADE_DEVICES: u32 = 10;

// ── 诊断码闭集（+0x01..+0x0C） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QkCode {
    /// quirk 行自相矛盾：vid/pid 为零或 fw 区间倒挂
    RowBad,
    /// 同型号 fw 区间重叠
    RowOverlap,
    /// 参数覆盖越界
    ParamBad,
    /// 查表键非法（vid/pid 为零）
    KeyBad,
    /// 降级声明重复
    DeclareDup,
    /// 已命中 quirk 的设备不该走降级
    DeclareMatched,
    /// 识别不可算却声明降级
    DeclareBlind,
    /// 降级设备被虚报手势承诺
    Overclaim,
    /// 未声明设备被查询
    NotDeclared,
    /// 账本对账不平
    LedgerBad,
    /// 复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl QkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            QkCode::RowBad => "quirk 行非法：vid/pid 为零或 fw_min > fw_max",
            QkCode::RowOverlap => "quirk 区间重叠：同 vid/pid 的 fw 区间互相遮蔽",
            QkCode::ParamBad => "参数覆盖越界：触点上限 1..=5、步进 1..=2、压力阈值 ≤1000",
            QkCode::KeyBad => "查表键非法：vid/pid 均不得为零",
            QkCode::DeclareDup => "降级声明重复：同一 boot_id 只允许声明一次",
            QkCode::DeclareMatched => "已命中 quirk：命中设备不走降级口径",
            QkCode::DeclareBlind => "识别不可算：不得为盲设备声明降级",
            QkCode::Overclaim => "虚报手势：quirk 外设备不承诺手势，只承诺识别",
            QkCode::NotDeclared => "未声明设备：quirk 外设备必须先落降级声明",
            QkCode::LedgerBad => "账本对账不平：声明数 ≠ 诚实条目数 + 异常数",
            QkCode::ReplayBad => "复算不一致：两次查表结果不同",
            QkCode::SegmentBad => "段位越界：诊断码逃出 0x70640 窗口",
        }
    }

    pub const ALL: [QkCode; 12] = [
        QkCode::RowBad,
        QkCode::RowOverlap,
        QkCode::ParamBad,
        QkCode::KeyBad,
        QkCode::DeclareDup,
        QkCode::DeclareMatched,
        QkCode::DeclareBlind,
        QkCode::Overclaim,
        QkCode::NotDeclared,
        QkCode::LedgerBad,
        QkCode::ReplayBad,
        QkCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QkCode::ALL.len() {
        if QkCode::ALL[i].val() == code {
            return QkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70640 段闭集内"
}

// ── FNV-1a（本域基础） ────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut x = h;
    x = fold_byte(x, (w & 0xFF) as u8);
    x = fold_byte(x, ((w >> 8) & 0xFF) as u8);
    x = fold_byte(x, ((w >> 16) & 0xFF) as u8);
    fold_byte(x, ((w >> 24) & 0xFF) as u8)
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── 参数覆盖 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuirkParams {
    /// 单帧并发触点上限（1..=5）。
    pub max_touch: u8,
    /// X 轴方向反转（归一值 0..=1000）。
    pub invert_x: bool,
    /// Y 轴方向反转。
    pub invert_y: bool,
    /// 压力下限：低于该值的接触视为噪声丢弃。
    pub pressure_floor: u16,
    /// 协议 B 的 slot 步进（1 或 2；2 表示固件隔槽上报）。
    pub slot_stride: u8,
}

impl QuirkParams {
    pub fn new(
        max_touch: u8,
        invert_x: bool,
        invert_y: bool,
        pressure_floor: u16,
        slot_stride: u8,
    ) -> QuirkParams {
        QuirkParams { max_touch, invert_x, invert_y, pressure_floor, slot_stride }
    }

    pub fn valid(&self) -> bool {
        self.max_touch >= 1
            && self.max_touch <= TOUCH_CAP
            && self.pressure_floor <= PRESSURE_FULL
            && self.slot_stride >= 1
            && self.slot_stride <= STRIDE_MAX
    }

    pub fn text(&self) -> String {
        format!(
            "touch{} {} {} floor{} stride{}",
            self.max_touch,
            if self.invert_x { "invX" } else { "x+" },
            if self.invert_y { "invY" } else { "y+" },
            self.pressure_floor,
            self.slot_stride
        )
    }

    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.max_touch);
        h = fold_byte(h, if self.invert_x { 1 } else { 0 });
        h = fold_byte(h, if self.invert_y { 1 } else { 0 });
        h = fold_word(h, self.pressure_floor as u32);
        fold_byte(h, self.slot_stride)
    }
}

/// 无 quirk 命中时的默认参数（不做任何覆盖）。
pub const DEFAULT_PARAMS: QuirkParams = QuirkParams {
    max_touch: TOUCH_CAP,
    invert_x: false,
    invert_y: false,
    pressure_floor: 0,
    slot_stride: 1,
};

// ── quirk 行 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuirkRow {
    pub id: u8,
    pub vid: u16,
    pub pid: u16,
    pub fw_min: u32,
    pub fw_max: u32,
    pub params: QuirkParams,
}

impl QuirkRow {
    pub fn new(
        id: u8,
        vid: u16,
        pid: u16,
        fw_min: u32,
        fw_max: u32,
        params: QuirkParams,
    ) -> QuirkRow {
        QuirkRow { id, vid, pid, fw_min, fw_max, params }
    }

    pub fn covers(&self, vid: u16, pid: u16, fw: u32) -> bool {
        self.vid == vid && self.pid == pid && fw >= self.fw_min && fw <= self.fw_max
    }

    pub fn text(&self) -> String {
        format!(
            "q{} {:04X}:{:04X} fw{}..={} {}",
            self.id, self.vid, self.pid, self.fw_min, self.fw_max, self.params.text()
        )
    }
}

/// 十项骨架表（覆盖 B03 语料的十台设备，含两类反向覆盖）。
pub const QUIRKS: [QuirkRow; QUIRK_ROWS] = [
    QuirkRow { id: 1, vid: 0x04F3, pid: 0x3100, fw_min: 0, fw_max: 55, params: QuirkParams { max_touch: 5, invert_x: false, invert_y: true, pressure_floor: 0, slot_stride: 1 } },
    QuirkRow { id: 2, vid: 0x04F3, pid: 0x3101, fw_min: 0, fw_max: 56, params: QuirkParams { max_touch: 5, invert_x: false, invert_y: false, pressure_floor: 0, slot_stride: 2 } },
    QuirkRow { id: 3, vid: 0x06CB, pid: 0x7E10, fw_min: 0, fw_max: 9, params: QuirkParams { max_touch: 5, invert_x: false, invert_y: false, pressure_floor: 50, slot_stride: 1 } },
    QuirkRow { id: 4, vid: 0x06CB, pid: 0x7E11, fw_min: 0, fw_max: 10, params: QuirkParams { max_touch: 4, invert_x: false, invert_y: false, pressure_floor: 0, slot_stride: 1 } },
    QuirkRow { id: 5, vid: 0x2575, pid: 0x0A01, fw_min: 0, fw_max: 1, params: QuirkParams { max_touch: 5, invert_x: true, invert_y: false, pressure_floor: 0, slot_stride: 1 } },
    QuirkRow { id: 6, vid: 0x1A2B, pid: 0x0011, fw_min: 0, fw_max: 100, params: QuirkParams { max_touch: 5, invert_x: false, invert_y: false, pressure_floor: 10, slot_stride: 1 } },
    QuirkRow { id: 7, vid: 0x1A2B, pid: 0x0012, fw_min: 0, fw_max: 101, params: QuirkParams { max_touch: 5, invert_x: false, invert_y: false, pressure_floor: 0, slot_stride: 2 } },
    QuirkRow { id: 8, vid: 0x0EEF, pid: 0x2001, fw_min: 0, fw_max: 7, params: QuirkParams { max_touch: 2, invert_x: false, invert_y: false, pressure_floor: 0, slot_stride: 1 } },
    QuirkRow { id: 9, vid: 0x0EEF, pid: 0x2002, fw_min: 0, fw_max: 8, params: QuirkParams { max_touch: 5, invert_x: false, invert_y: true, pressure_floor: 0, slot_stride: 1 } },
    QuirkRow { id: 10, vid: 0x0534, pid: 0x0300, fw_min: 0, fw_max: 3, params: QuirkParams { max_touch: 1, invert_x: false, invert_y: false, pressure_floor: 0, slot_stride: 1 } },
];

/// 单行自洽。
pub fn validate_row(row: &QuirkRow) -> Result<(), u32> {
    if row.vid == 0 || row.pid == 0 {
        return Err(QkCode::RowBad.val());
    }
    if row.fw_min > row.fw_max {
        return Err(QkCode::RowBad.val());
    }
    if !row.params.valid() {
        return Err(QkCode::ParamBad.val());
    }
    Ok(())
}

/// 全表自洽：逐行通过 + 同型号区间不叠。
pub fn validate_table(rows: &[QuirkRow]) -> Result<usize, u32> {
    let mut i = 0usize;
    while i < rows.len() {
        let a = match rows.get(i) {
            Some(r) => *r,
            None => return Err(QkCode::SegmentBad.val()),
        };
        validate_row(&a)?;
        let mut j = i + 1;
        while j < rows.len() {
            let b = match rows.get(j) {
                Some(r) => *r,
                None => return Err(QkCode::SegmentBad.val()),
            };
            if a.vid == b.vid && a.pid == b.pid {
                let disjoint = a.fw_max < b.fw_min || b.fw_max < a.fw_min;
                if !disjoint {
                    return Err(QkCode::RowOverlap.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(rows.len())
}

/// 三键精确查表：vid/pid 完全相等 + fw 落在闭区间。
pub fn lookup(rows: &[QuirkRow], vid: u16, pid: u16, fw: u32) -> Result<Option<QuirkRow>, u32> {
    if vid == 0 || pid == 0 {
        return Err(QkCode::KeyBad.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => *v,
            None => return Err(QkCode::SegmentBad.val()),
        };
        if r.covers(vid, pid, fw) {
            return Ok(Some(r));
        }
        i += 1;
    }
    Ok(None)
}

/// 参数解析：命中行 → 行参数；未命中 → 默认参数。
pub fn params_for(rows: &[QuirkRow], vid: u16, pid: u16, fw: u32) -> Result<(QuirkParams, bool), u32> {
    match lookup(rows, vid, pid, fw)? {
        Some(r) => Ok((r.params, true)),
        None => Ok((DEFAULT_PARAMS, false)),
    }
}

// ── 参数执行（反向覆盖 + 压力下限） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quirked {
    pub x: u16,
    pub y: u16,
    pub pressure: u16,
    pub suppressed: bool,
}

/// 执行覆盖：输入为已归一到 0..=1000 的采样。
///
/// - invert_x/invert_y：1000 − v（端点保持 0/1000 对调）。
/// - pressure_floor：压力低于阈值 → suppressed=true 且压力归零；
///   采样本身仍返回，由调用方决定是否丢弃（如实记账，不静默）。
pub fn apply_params(p: &QuirkParams, x: u16, y: u16, pressure: u16) -> Result<Quirked, u32> {
    if !p.valid() {
        return Err(QkCode::ParamBad.val());
    }
    if x > PRESSURE_FULL || y > PRESSURE_FULL || pressure > PRESSURE_FULL {
        return Err(QkCode::ParamBad.val());
    }
    let ox = if p.invert_x { PRESSURE_FULL - x } else { x };
    let oy = if p.invert_y { PRESSURE_FULL - y } else { y };
    let suppressed = pressure < p.pressure_floor;
    let op = if suppressed { 0 } else { pressure };
    Ok(Quirked { x: ox, y: oy, pressure: op, suppressed })
}

// ── 降级声明账（quirk 外设备） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeDecl {
    pub boot_id: u32,
    pub vid: u16,
    pub pid: u16,
    pub fw: u32,
    /// 识别可算（协议与轴量纲可判定）。
    pub recognition_ok: bool,
    /// 手势不承诺（quirk 外一律 false）。
    pub gesture_promised: bool,
    pub matched: bool,
}

impl DegradeDecl {
    pub fn text(&self) -> String {
        format!(
            "degrade dev{} {:04X}:{:04X} fw{} recognition={} gesture={}",
            self.boot_id,
            self.vid,
            self.pid,
            self.fw,
            self.recognition_ok,
            self.gesture_promised
        )
    }
}

pub struct DegradeLedger {
    decls: Vec<DegradeDecl>,
    overclaim_rejects: u32,
    blind_rejects: u32,
    dup_rejects: u32,
}

impl DegradeLedger {
    pub fn new() -> DegradeLedger {
        DegradeLedger {
            decls: Vec::new(),
            overclaim_rejects: 0,
            blind_rejects: 0,
            dup_rejects: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.decls.len()
    }

    pub fn overclaim_rejects(&self) -> u32 {
        self.overclaim_rejects
    }

    pub fn blind_rejects(&self) -> u32 {
        self.blind_rejects
    }

    pub fn dup_rejects(&self) -> u32 {
        self.dup_rejects
    }

    fn find(&self, boot_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.decls.len() {
            if let Some(d) = self.decls.get(i) {
                if d.boot_id == boot_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn get(&self, boot_id: u32) -> Result<DegradeDecl, u32> {
        match self.find(boot_id) {
            Some(i) => match self.decls.get(i) {
                Some(d) => Ok(*d),
                None => Err(QkCode::SegmentBad.val()),
            },
            None => Err(QkCode::NotDeclared.val()),
        }
    }

    /// 落声明：quirk 外 + 识别可算 + 未声明过。
    pub fn declare(
        &mut self,
        rows: &[QuirkRow],
        boot_id: u32,
        vid: u16,
        pid: u16,
        fw: u32,
        recognition_ok: bool,
    ) -> Result<(), u32> {
        if vid == 0 || pid == 0 || fw == 0 {
            return Err(QkCode::KeyBad.val());
        }
        if self.find(boot_id).is_some() {
            self.dup_rejects = self.dup_rejects.saturating_add(1);
            return Err(QkCode::DeclareDup.val());
        }
        let matched = lookup(rows, vid, pid, fw)?.is_some();
        if matched {
            return Err(QkCode::DeclareMatched.val());
        }
        if !recognition_ok {
            self.blind_rejects = self.blind_rejects.saturating_add(1);
            return Err(QkCode::DeclareBlind.val());
        }
        self.decls.push(DegradeDecl {
            boot_id,
            vid,
            pid,
            fw,
            recognition_ok: true,
            gesture_promised: false,
            matched: false,
        });
        Ok(())
    }

    /// 手势承诺核验：quirk 外设备一律拒绝。
    ///
    /// 这里不是「报错就算完」：拒绝次数入账，声明本身不变。调用方若
    /// 要继续做手势，必须自己承担「未承诺」的语义，而不是把拒绝吞掉。
    pub fn claim_gesture(&mut self, boot_id: u32) -> Result<(), u32> {
        let idx = match self.find(boot_id) {
            Some(i) => i,
            None => return Err(QkCode::NotDeclared.val()),
        };
        let d = match self.decls.get(idx) {
            Some(v) => *v,
            None => return Err(QkCode::SegmentBad.val()),
        };
        if !d.gesture_promised {
            self.overclaim_rejects = self.overclaim_rejects.saturating_add(1);
            return Err(QkCode::Overclaim.val());
        }
        Ok(())
    }

    /// 对账：每条声明必须 recognition_ok=true 且 gesture_promised=false。
    pub fn audit(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.decls.len() {
            match self.decls.get(i) {
                Some(d) => {
                    if !d.recognition_ok || d.gesture_promised || d.matched {
                        return Err(QkCode::LedgerBad.val());
                    }
                }
                None => return Err(QkCode::SegmentBad.val()),
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.decls.len() {
            if let Some(d) = self.decls.get(i) {
                h = fold_word(h, d.boot_id);
                h = fold_word(h, d.vid as u32);
                h = fold_word(h, d.pid as u32);
                h = fold_word(h, d.fw);
                h = fold_byte(h, if d.recognition_ok { 1 } else { 0 });
                h = fold_byte(h, if d.gesture_promised { 2 } else { 0 });
            }
            i += 1;
        }
        h = fold_word(h, self.overclaim_rejects);
        h = fold_word(h, self.blind_rejects);
        fold_word(h, self.dup_rejects)
    }
}

// ── 黄金语料 ──────────────────────────────────────────────────────
/// 十项命中：逐行用自己的 vid/pid/fw_min 查表，必须命中同一行。
pub fn golden_hits() -> Result<u32, u32> {
    validate_table(&QUIRKS)?;
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < QUIRKS.len() {
        let r = match QUIRKS.get(i) {
            Some(v) => *v,
            None => return Err(QkCode::SegmentBad.val()),
        };
        match lookup(&QUIRKS, r.vid, r.pid, r.fw_min)? {
            Some(found) => {
                if found.id != r.id || found.params != r.params {
                    return Err(QkCode::ReplayBad.val());
                }
                hits += 1;
            }
            None => return Err(QkCode::RowBad.val()),
        }
        // fw_max 仍命中；fw_max + 1 必须落空（开放区间上界）。
        if lookup(&QUIRKS, r.vid, r.pid, r.fw_max)?.is_none() {
            return Err(QkCode::RowBad.val());
        }
        if lookup(&QUIRKS, r.vid, r.pid, r.fw_max.saturating_add(1))?.is_some() {
            return Err(QkCode::RowOverlap.val());
        }
        i += 1;
    }
    Ok(hits)
}

/// 十台 quirk 外设备：全部落降级声明，全部拒绝手势承诺。
pub fn golden_degrade() -> Result<DegradeLedger, u32> {
    let mut led = DegradeLedger::new();
    let mut i = 0u32;
    while i < DEGRADE_DEVICES {
        let boot = 9000u32.saturating_add(i);
        let vid = 0x9000u16.saturating_add(i as u16);
        let pid = 0x8F00u16.saturating_add(i as u16);
        let fw = 40u32.saturating_add(i);
        // 预确认这些键不在表内。
        if lookup(&QUIRKS, vid, pid, fw)?.is_some() {
            return Err(QkCode::RowOverlap.val());
        }
        led.declare(&QUIRKS, boot, vid, pid, fw, true)?;
        if led.claim_gesture(boot) != Err(QkCode::Overclaim.val()) {
            return Err(QkCode::Overclaim.val());
        }
        i += 1;
    }
    led.audit()?;
    Ok(led)
}

/// 参数执行四例：原样、双反向、压力压制、阈值边界。
pub fn golden_apply() -> Result<u32, u32> {
    let plain = QuirkParams::new(5, false, false, 0, 1);
    let a = apply_params(&plain, 250, 750, 500)?;
    if a.x != 250 || a.y != 750 || a.suppressed {
        return Err(QkCode::ParamBad.val());
    }
    let invert = QuirkParams::new(5, true, true, 0, 1);
    let b = apply_params(&invert, 250, 750, 500)?;
    if b.x != 750 || b.y != 250 {
        return Err(QkCode::ParamBad.val());
    }
    let floor = QuirkParams::new(5, false, false, 100, 1);
    let c = apply_params(&floor, 100, 100, 99)?;
    if !c.suppressed || c.pressure != 0 {
        return Err(QkCode::ParamBad.val());
    }
    let d = apply_params(&floor, 100, 100, 100)?;
    if d.suppressed || d.pressure != 100 {
        return Err(QkCode::ParamBad.val());
    }
    Ok(4)
}

/// 全链：十项命中 → 十台降级 → 四例执行。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let hits = golden_hits()?;
    let deg = golden_degrade()?;
    let applied = golden_apply()?;
    if hits != QUIRK_ROWS as u32 || deg.len() != DEGRADE_DEVICES as usize {
        return Err(QkCode::LedgerBad.val());
    }
    if deg.overclaim_rejects() != DEGRADE_DEVICES || applied != 4 {
        return Err(QkCode::Overclaim.val());
    }
    let mut h = FNV_OFF;
    h = fold_word(h, hits);
    h = fold_word(h, deg.len() as u32);
    h = fold_word(h, (deg.seal() & 0xFFFF_FFFF) as u32);
    h = fold_word(h, applied);
    Ok((hits, deg.len() as u32, deg.overclaim_rejects(), h))
}

// ── LCG ───────────────────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入账（四路：倒挂行/重叠行/越界参数/虚报手势） ──────────────
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
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：fw 区间倒挂。
        let bad = QuirkRow::new(1, 0x1111, 0x2222, 10, 5, DEFAULT_PARAMS);
        led.record(0, validate_row(&bad) == Err(QkCode::RowBad.val()));
        // 路 2：同型号区间重叠。
        let a = QuirkRow::new(1, 0x1111, 0x2222, 0, 10, DEFAULT_PARAMS);
        let b = QuirkRow::new(2, 0x1111, 0x2222, 10, 20, DEFAULT_PARAMS);
        let pair = [a, b];
        led.record(1, validate_table(&pair) == Err(QkCode::RowOverlap.val()));
        // 路 3：触点上限 0。
        let p = QuirkParams::new(0, false, false, 0, 1);
        led.record(2, !p.valid() && apply_params(&p, 0, 0, 0) == Err(QkCode::ParamBad.val()));
        // 路 4：quirk 外设备虚报手势。
        let mut d = DegradeLedger::new();
        let declared = d.declare(&QUIRKS, 1, 0x9001, 0x8F01, 40, true).is_ok();
        let rejected = d.claim_gesture(1) == Err(QkCode::Overclaim.val());
        led.record(3, declared && rejected);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (hits, deg, over, _) = golden_run()?;
    if hits == 10 && deg == 10 && over == 10 {
        Ok(())
    } else {
        Err(QkCode::LedgerBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 未知键必须落空。
    if lookup(&QUIRKS, 0xDEAD, 0xBEEF, 1)?.is_some() {
        return Err(QkCode::RowOverlap.val());
    }
    // 零 vid 必须拒。
    if lookup(&QUIRKS, 0, 1, 1) != Err(QkCode::KeyBad.val()) {
        return Err(QkCode::KeyBad.val());
    }
    // 识别不可算的盲设备不得声明。
    let mut d = DegradeLedger::new();
    if d.declare(&QUIRKS, 2, 0x9002, 0x8F02, 41, false) != Err(QkCode::DeclareBlind.val()) {
        return Err(QkCode::DeclareBlind.val());
    }
    // 已命中设备不得走降级。
    if d.declare(&QUIRKS, 3, 0x04F3, 0x3100, 1, true) != Err(QkCode::DeclareMatched.val()) {
        return Err(QkCode::DeclareMatched.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if QUIRK_ROWS == 10
        && TOUCH_CAP == 5
        && PRESSURE_FULL == 1000
        && STRIDE_MAX == 2
        && DEGRADE_DEVICES == 10
        && QUIRKS.len() == 10
    {
        Ok(())
    } else {
        Err(QkCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(QkCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < QkCode::ALL.len() {
        let v = QkCode::ALL[i].val();
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
    let hits = golden_hits();
    v.push((
        "F48851-J1-十项命中正确",
        hits.ok() == Some(10),
        format!("hits={:?}", hits.ok()),
    ));
    let deg = golden_degrade();
    v.push((
        "F48852-J1-十台降级如实落账",
        match &deg {
            Ok(d) => d.len() == 10 && d.audit().is_ok() && d.overclaim_rejects() == 10,
            Err(_) => false,
        },
        match &deg {
            Ok(d) => format!(
                "decls={} overclaim={} blind={}",
                d.len(),
                d.overclaim_rejects(),
                d.blind_rejects()
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let apply = golden_apply();
    v.push((
        "F48851-J1-参数执行四例",
        apply.ok() == Some(4),
        format!("cases={:?}", apply.ok()),
    ));
    v.push((
        "F48851-J1-区间上界开放",
        {
            let r = QUIRKS.first().copied();
            match r {
                Some(row) => lookup(&QUIRKS, row.vid, row.pid, row.fw_max).ok().flatten().is_some()
                    && lookup(&QUIRKS, row.vid, row.pid, row.fw_max + 1).ok().flatten().is_none(),
                None => false,
            }
        },
        String::from("fw_max 命中、fw_max+1 落空"),
    ));
    v.push((
        "F48852-J1-未声明设备拒查询",
        {
            let d = DegradeLedger::new();
            d.get(424242) == Err(QkCode::NotDeclared.val())
        },
        String::from("NotDeclared"),
    ));
    v.push((
        "F48852-J1-盲设备不得声明",
        {
            let mut d = DegradeLedger::new();
            d.declare(&QUIRKS, 5, 0x9100, 0x8F00, 44, false) == Err(QkCode::DeclareBlind.val())
        },
        String::from("recognition_ok=false → DeclareBlind"),
    ));
    v.push((
        "F48851-J1-已命中不得降级",
        {
            let mut d = DegradeLedger::new();
            d.declare(&QUIRKS, 6, 0x06CB, 0x7E10, 3, true) == Err(QkCode::DeclareMatched.val())
        },
        String::from("quirk 命中 → DeclareMatched"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48852-J1-注入四路10/10",
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
        "F48851-J1-探针三序与双跑",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok() && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48852-J1-段位守卫",
        segment_guard(),
        String::from("0x70640 窗口 12 码"),
    ));
    v
}

pub fn run_unx48851_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48851 触控设备 quirk 表骨架与降级口径");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_valid_and_ten_rows() {
        assert_eq!(validate_table(&QUIRKS), Ok(10));
    }

    #[test]
    fn exact_key_hits_own_row() {
        let mut i = 0usize;
        while i < QUIRKS.len() {
            if let Some(r) = QUIRKS.get(i) {
                match lookup(&QUIRKS, r.vid, r.pid, r.fw_min) {
                    Ok(Some(f)) => assert_eq!(f.id, r.id),
                    _ => assert!(false),
                }
            }
            i += 1;
        }
    }

    #[test]
    fn upper_bound_is_inclusive() {
        let row = QuirkRow::new(1, 0x1234, 0x5678, 2, 6, DEFAULT_PARAMS);
        assert!(row.covers(0x1234, 0x5678, 2));
        assert!(row.covers(0x1234, 0x5678, 6));
        assert!(!row.covers(0x1234, 0x5678, 7));
        assert!(!row.covers(0x1234, 0x5678, 1));
    }

    #[test]
    fn unknown_key_falls_back_to_default() {
        assert_eq!(params_for(&QUIRKS, 0xDEAD, 0xBEEF, 1).ok(), Some((DEFAULT_PARAMS, false)));
    }

    #[test]
    fn known_key_returns_row_params() {
        let p = params_for(&QUIRKS, 0x04F3, 0x3100, 10);
        assert!(p.is_ok());
        if let Ok((params, matched)) = p {
            assert!(matched);
            assert!(params.invert_y);
        }
    }

    #[test]
    fn inverted_row_rejects_bad_range() {
        let bad = QuirkRow::new(1, 0x1, 0x2, 9, 3, DEFAULT_PARAMS);
        assert_eq!(validate_row(&bad), Err(QkCode::RowBad.val()));
    }

    #[test]
    fn overlapping_rows_are_rejected() {
        let a = QuirkRow::new(1, 0x1, 0x2, 0, 10, DEFAULT_PARAMS);
        let b = QuirkRow::new(2, 0x1, 0x2, 5, 15, DEFAULT_PARAMS);
        assert_eq!(validate_table(&[a, b]), Err(QkCode::RowOverlap.val()));
    }

    #[test]
    fn adjacent_rows_are_allowed() {
        let a = QuirkRow::new(1, 0x1, 0x2, 0, 10, DEFAULT_PARAMS);
        let b = QuirkRow::new(2, 0x1, 0x2, 11, 20, DEFAULT_PARAMS);
        assert_eq!(validate_table(&[a, b]).ok(), Some(2));
    }

    #[test]
    fn zero_key_is_rejected() {
        assert_eq!(lookup(&QUIRKS, 0, 1, 1), Err(QkCode::KeyBad.val()));
        assert_eq!(lookup(&QUIRKS, 1, 0, 1), Err(QkCode::KeyBad.val()));
    }

    #[test]
    fn bad_params_are_rejected() {
        assert!(!QuirkParams::new(0, false, false, 0, 1).valid());
        assert!(!QuirkParams::new(6, false, false, 0, 1).valid());
        assert!(!QuirkParams::new(5, false, false, 1001, 1).valid());
        assert!(!QuirkParams::new(5, false, false, 0, 0).valid());
        assert!(!QuirkParams::new(5, false, false, 0, 3).valid());
    }

    #[test]
    fn apply_inverts_and_clamps() {
        let p = QuirkParams::new(5, true, false, 0, 1);
        let q = apply_params(&p, 0, 1000, 500);
        assert_eq!(q.ok().map(|v| (v.x, v.y)), Some((1000, 1000)));
    }

    #[test]
    fn pressure_floor_suppresses_below_only() {
        let p = QuirkParams::new(5, false, false, 200, 1);
        let below = apply_params(&p, 1, 1, 199);
        let at = apply_params(&p, 1, 1, 200);
        assert_eq!(below.ok().map(|v| v.suppressed), Some(true));
        assert_eq!(at.ok().map(|v| v.suppressed), Some(false));
    }

    #[test]
    fn apply_rejects_out_of_scale() {
        let p = QuirkParams::new(5, false, false, 0, 1);
        assert_eq!(apply_params(&p, 1001, 0, 0), Err(QkCode::ParamBad.val()));
    }

    #[test]
    fn degrade_requires_recognizable_device() {
        let mut d = DegradeLedger::new();
        assert_eq!(
            d.declare(&QUIRKS, 1, 0x9001, 0x8F01, 40, false),
            Err(QkCode::DeclareBlind.val())
        );
    }

    #[test]
    fn degrade_rejects_matched_device() {
        let mut d = DegradeLedger::new();
        assert_eq!(
            d.declare(&QUIRKS, 2, 0x04F3, 0x3100, 1, true),
            Err(QkCode::DeclareMatched.val())
        );
    }

    #[test]
    fn degrade_records_and_refuses_gesture() {
        let mut d = DegradeLedger::new();
        assert!(d.declare(&QUIRKS, 3, 0x9003, 0x8F03, 42, true).is_ok());
        assert_eq!(d.claim_gesture(3), Err(QkCode::Overclaim.val()));
        assert_eq!(d.overclaim_rejects(), 1);
        assert!(d.audit().is_ok());
        let decl = d.get(3);
        assert_eq!(decl.ok().map(|x| x.recognition_ok), Some(true));
        assert_eq!(decl.ok().map(|x| x.gesture_promised), Some(false));
    }

    #[test]
    fn duplicate_declaration_is_rejected() {
        let mut d = DegradeLedger::new();
        assert!(d.declare(&QUIRKS, 4, 0x9004, 0x8F04, 43, true).is_ok());
        assert_eq!(
            d.declare(&QUIRKS, 4, 0x9004, 0x8F04, 43, true),
            Err(QkCode::DeclareDup.val())
        );
    }

    #[test]
    fn ten_degraded_devices_are_honest() {
        let d = golden_degrade();
        assert!(d.is_ok());
        if let Ok(dd) = d {
            assert_eq!(dd.len(), 10);
            assert_eq!(dd.overclaim_rejects(), 10);
            assert_eq!(dd.blind_rejects(), 0);
            assert_eq!(dd.dup_rejects(), 0);
            assert!(dd.audit().is_ok());
        }
    }

    #[test]
    fn golden_run_is_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(QkCode::ALL.len(), 12);
        assert_eq!(QkCode::RowBad.val(), SEG_BASE + 1);
        assert_eq!(QkCode::SegmentBad.val(), SEG_BASE + 12);
    }
}
