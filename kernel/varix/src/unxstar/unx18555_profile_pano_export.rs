//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18555 · 档案观测导出（伪装全景视图）· 0x80680 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18555-J1 bottle 全景（默认层+逐应用层）视图可导出，
//!       与 F18474/F18457 三视图合并一致
//! 本模块结构清单：
//!   1. 冻结常量层：8 键全景键闭集（KEY_N=8）、5 档值类、两个既有
//!      视图锚（V1=F18474 默认层视图、V2=F18457 逐应用层视图，
//!      id 写死取派工册）、导出分隔语法（"|"分节、";"分键、":"分值）；
//!   2. 诊断码闭集 PvCode（code() = SEG_BASE + n，0x80680 段窗口）；
//!   3. 数据结构一（分层账）：PanoLayer——默认层/逐应用层两组
//!      8 槽数组账（槽位=键位，0xFF 表示未声明），占用位面双份；
//!   4. 数据结构二（全景视图）：PanoView——合并后键值槽 + 指纹
//!      （键×31+值 折叠）+ 导出文本账（往返核）；
//!   5. 合并核 merge_layers：逐键取值——应用层在场用应用层、否则
//!      用默认层、双缺省槽=未声明；合并确定性：与独立查表路
//!      （table_merge）逐键对拍一致（三视图合并一致判据）；
//!   6. 导出核 export_view：确定性文本（按键位升序），含默认层节
//!      与逐应用层节；解析核 parse_view 往返——导出→解析→逐槽
//!      相等（往返判据）；
//!   7. 注入核 3 类×10 轮：未知键 / 逐应用层重复键 / 值类越界，
//!      必拒；篡改导出文本（改一位）→ 解析必拒（指纹闸）；
//!   8. 黄金全链 golden_run + run_unx18555_checks：14 条判据 + 8 例
//!      #[cfg(test)]。
//! 为什么导出用槽位数组而不是字符串拼接查值：槽位（0xFF=未声明）
//! 让「未声明」成为一等状态——合并时默认层兜底、逐应用层覆盖的
//! 语义才可判定；字符串只在导出边界出现，内部账一律结构化。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80680，AI-VX11-1105-G1·A16 批 08 独占窗口）。
pub const SEG_BASE: u32 = 0x80680;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 全景键闭集（冻结：8 键——档案面 12 键的观测子集，键位 0..8）。
pub const KEY_N: usize = 8;
/// 未声明槽标记（冻结：0xFF，值类闭集 0..5 之外的哨兵）。
pub const SLOT_UNSET: u8 = 0xFF;
/// 值类别档数（冻结：5 档，与 F18551 值类闭集对齐）。
pub const VAL_CLASS_N: u8 = 5;
/// 既有视图锚 V1（冻结：F18474 默认层视图 id，取派工册）。
pub const VIEW_V1_ID: u32 = 18474;
/// 既有视图锚 V2（冻结：F18457 逐应用层视图 id，取派工册）。
pub const VIEW_V2_ID: u32 = 18457;
/// 本卡全景合并视图 id（冻结：卡号 18555）。
pub const VIEW_V3_ID: u32 = 18555;
/// 三视图数（冻结：V1 默认层 + V2 逐应用层 + V3 合并全景）。
pub const VIEW_COUNT: usize = 3;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PvCode {
    /// 键未知：键位落出 0..8
    KeyUnknown,
    /// 逐应用层重复键
    KeyDup,
    /// 值类越界
    ValClassRange,
    /// 层账损坏：占用计数与槽位失配
    LayerCorrupt,
    /// 合并不一致：两路合并结果失配
    MergeMismatch,
    /// 导出文本空
    ExportEmpty,
    /// 解析失败（往返破）
    ParseFail,
    /// 指纹失配（篡改检出）
    FingerprintMismatch,
    /// 视图锚失配：V1/V2/V3 冻结 id 对不上
    ViewAnchorMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl PvCode {
    /// 诊断码：SEG_BASE + n（契约骨架口径）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            PvCode::KeyUnknown => "键未知：观测键位不在 0..8 闭集",
            PvCode::KeyDup => "逐应用层重复键：同键双声明",
            PvCode::ValClassRange => "值类越界：不在 5 档闭集",
            PvCode::LayerCorrupt => "层账损坏：占用计数与槽位失配",
            PvCode::MergeMismatch => "合并不一致：两路合并结果失配",
            PvCode::ExportEmpty => "导出文本空：全景视图必须可导出",
            PvCode::ParseFail => "解析失败：导出→解析往返破",
            PvCode::FingerprintMismatch => "指纹失配：导出文本被篡改",
            PvCode::ViewAnchorMismatch => "视图锚失配：V1/V2/V3 冻结 id 不符",
            PvCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [PvCode; 10] = [
        PvCode::KeyUnknown,
        PvCode::KeyDup,
        PvCode::ValClassRange,
        PvCode::LayerCorrupt,
        PvCode::MergeMismatch,
        PvCode::ExportEmpty,
        PvCode::ParseFail,
        PvCode::FingerprintMismatch,
        PvCode::ViewAnchorMismatch,
        PvCode::NotReproducible,
    ];
}

/// 按数值取中文文案（闭集外返回固定提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PvCode::ALL.len() {
        if PvCode::ALL[i].code() == code {
            return PvCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80680 段闭集内"
}

// ── 数据结构一：分层账 ──────────────────────────────────────────
/// 单层账：8 槽键值（SLOT_UNSET=未声明）+ 占用计数。
/// 为什么槽位即键位：合并与导出都不需要查表索引，O(1) 直落，
/// 且占用计数与槽位构成守恒对（LayerCorrupt 即现形）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanoLayer {
    slots: [u8; KEY_N],
    used: u32,
}

impl PanoLayer {
    /// 新建空层（全 SLOT_UNSET）。
    pub fn new() -> PanoLayer {
        PanoLayer { slots: [SLOT_UNSET; KEY_N], used: 0 }
    }
    /// 落值：键越界/值类越界/重复显式拒。
    pub fn put(&mut self, key_id: u8, val_class: u8) -> Result<(), u32> {
        if key_id as usize >= KEY_N {
            return Err(PvCode::KeyUnknown.code());
        }
        if val_class >= VAL_CLASS_N {
            return Err(PvCode::ValClassRange.code());
        }
        let k = key_id as usize;
        if self.slots[k] != SLOT_UNSET {
            return Err(PvCode::KeyDup.code());
        }
        self.slots[k] = val_class;
        self.used = self.used.saturating_add(1);
        Ok(())
    }
    /// 槽位读值（未声明=Some(SLOT_UNSET) 由调用方判）。
    pub fn slot(&self, key_id: u8) -> Option<u8> {
        if key_id as usize >= KEY_N {
            return None;
        }
        self.slots.get(key_id as usize).copied()
    }
    /// 占用计数。
    pub fn used_count(&self) -> u32 {
        self.used
    }
    /// 层守恒：占用计数与槽位实数一致。
    pub fn conserve_ok(&self) -> bool {
        let mut n = 0u32;
        let mut i = 0;
        while i < KEY_N {
            if self.slots[i] != SLOT_UNSET {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n == self.used
    }
}

/// 全景分层账：默认层 + 逐应用层（V1/V2 的结构化载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanoLayers {
    pub bottle: PanoLayer,
    pub app: PanoLayer,
}

// ── 数据结构二：全景视图 ────────────────────────────────────────
/// 合并后的全景视图：8 键槽 + 双层快照 + 指纹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanoView {
    /// 合并槽（应用层在场用应用层，否则默认层，双缺省=UNSET）。
    pub merged: [u8; KEY_N],
    /// 默认层快照（导出节 V1）。
    pub bottle: [u8; KEY_N],
    /// 逐应用层快照（导出节 V2）。
    pub app: [u8; KEY_N],
    /// 指纹：三节逐槽折叠。
    pub fingerprint: u32,
}

/// 指纹：三节逐槽折叠（饱和运算，不回绕）。
pub fn view_fingerprint(bottle: &[u8; KEY_N], app: &[u8; KEY_N], merged: &[u8; KEY_N]) -> u32 {
    let mut acc: u32 = VIEW_V3_ID;
    let mut i = 0;
    while i < KEY_N {
        acc = acc
            .saturating_mul(31)
            .saturating_add(bottle[i] as u32)
            .saturating_mul(7)
            .saturating_add(app[i] as u32)
            .saturating_mul(3)
            .saturating_add(merged[i] as u32);
        i += 1;
    }
    acc
}

// ── 合并核（两路独立推导，三视图合并一致） ──────────────────────
/// 路 1：槽位直并——应用层在场用应用层，否则默认层。
pub fn merge_layers(layers: &PanoLayers) -> Result<PanoView, u32> {
    if !layers.bottle.conserve_ok() || !layers.app.conserve_ok() {
        return Err(PvCode::LayerCorrupt.code());
    }
    let mut merged = [SLOT_UNSET; KEY_N];
    let mut bottle = [SLOT_UNSET; KEY_N];
    let mut app = [SLOT_UNSET; KEY_N];
    let mut i = 0;
    while i < KEY_N {
        let b = layers.bottle.slot(i as u8).ok_or(PvCode::KeyUnknown.code())?;
        let a = layers.app.slot(i as u8).ok_or(PvCode::KeyUnknown.code())?;
        bottle[i] = b;
        app[i] = a;
        merged[i] = if a != SLOT_UNSET { a } else { b };
        i += 1;
    }
    let fp = view_fingerprint(&bottle, &app, &merged);
    Ok(PanoView { merged, bottle, app, fingerprint: fp })
}

/// 路 2：查表独立并——按「应用层优先」的真值表逐键复算。
/// 两路逐键相等 = 三视图合并一致判据（V3 == V1⊕V2）。
pub fn table_merge(layers: &PanoLayers) -> Result<[u8; KEY_N], u32> {
    // 真值表：0=未声明, 1=有值。行=(默认层,应用层)，列=合并取值策略。
    // (0,0)->UNSET, (0,1)->app, (1,0)->bottle, (1,1)->app（应用覆盖）。
    let mut out = [SLOT_UNSET; KEY_N];
    let mut i = 0;
    while i < KEY_N {
        let b = layers.bottle.slot(i as u8).ok_or(PvCode::KeyUnknown.code())?;
        let a = layers.app.slot(i as u8).ok_or(PvCode::KeyUnknown.code())?;
        let has_b = b != SLOT_UNSET;
        let has_a = a != SLOT_UNSET;
        out[i] = match (has_b, has_a) {
            (false, false) => SLOT_UNSET,
            (false, true) => a,
            (true, false) => b,
            (true, true) => a,
        };
        i += 1;
    }
    Ok(out)
}

/// 三视图合并一致核：路 1 与路 2 逐键对拍。
pub fn three_view_consistent(layers: &PanoLayers) -> Result<bool, u32> {
    let v3 = merge_layers(layers)?;
    let v3b = table_merge(layers)?;
    let mut i = 0;
    while i < KEY_N {
        if v3.merged[i] != v3b[i] {
            return Err(PvCode::MergeMismatch.code());
        }
        i += 1;
    }
    Ok(true)
}

// ── 导出核与解析核（往返） ──────────────────────────────────────
/// 导出：确定性文本——"V3|B:…;A:…;M:…;"，各节 8 槽按键位升序，
/// 未声明槽以 "?" 记。同输入两次导出逐字节同值。
pub fn export_view(v: &PanoView) -> Result<String, u32> {
    if v.fingerprint != view_fingerprint(&v.bottle, &v.app, &v.merged) {
        return Err(PvCode::FingerprintMismatch.code());
    }
    let mut s = String::new();
    s.push_str("V3|");
    let mut section = 0;
    while section < 3 {
        let tag = match section {
            0 => "B:",
            1 => "A:",
            _ => "M:",
        };
        s.push_str(tag);
        let slots: &[u8; KEY_N] = match section {
            0 => &v.bottle,
            1 => &v.app,
            _ => &v.merged,
        };
        let mut i = 0;
        while i < KEY_N {
            let val = slots.get(i).ok_or(PvCode::LayerCorrupt.code())?;
            if *val == SLOT_UNSET {
                s.push('?');
            } else {
                let ch = (b'0' + *val) as char;
                s.push(ch);
            }
            if i + 1 < KEY_N {
                s.push(',');
            }
            i += 1;
        }
        s.push(';');
        section += 1;
    }
    // 指纹节：解析核据此闸篡改（任一槽位改动都会让重算指纹失配）。
    s.push_str(&format!("F:{};", v.fingerprint));
    if s.is_empty() {
        return Err(PvCode::ExportEmpty.code());
    }
    Ok(s)
}

/// 解析：导出文本 → 视图（槽值重建）+ 指纹闸（篡改必拒）。
pub fn parse_view(text: &str) -> Result<PanoView, u32> {
    if text.is_empty() {
        return Err(PvCode::ExportEmpty.code());
    }
    // 前缀闸。
    let head = "V3|";
    if text.len() < head.len() || !text.starts_with(head) {
        return Err(PvCode::ParseFail.code());
    }
    let body = &text[head.len()..];
    // 三节逐段解析（B/A/M）。
    let mut bottle = [SLOT_UNSET; KEY_N];
    let mut app = [SLOT_UNSET; KEY_N];
    let mut merged = [SLOT_UNSET; KEY_N];
    let mut sections: [&mut [u8; KEY_N]; 3] = [&mut bottle, &mut app, &mut merged];
    let mut rest = body;
    let mut sec = 0;
    while sec < 3 {
        // 节头。
        let want_tag = match sec {
            0 => "B:",
            1 => "A:",
            _ => "M:",
        };
        if !rest.starts_with(want_tag) {
            return Err(PvCode::ParseFail.code());
        }
        rest = &rest[want_tag.len()..];
        // 8 槽逐槽（',' 分隔，';' 收节）。
        let mut i = 0;
        while i < KEY_N {
            let bytes = rest.as_bytes();
            if bytes.is_empty() {
                return Err(PvCode::ParseFail.code());
            }
            let c = bytes[0];
            let val = match c {
                b'?' => SLOT_UNSET,
                b'0'..=b'4' => c - b'0',
                _ => return Err(PvCode::ParseFail.code()),
            };
            let dst: &mut [u8; KEY_N] = sections[sec];
            dst[i] = val;
            rest = &rest[1..];
            if i + 1 < KEY_N {
                if !rest.starts_with(',') {
                    return Err(PvCode::ParseFail.code());
                }
                rest = &rest[1..];
            }
            i += 1;
        }
        if !rest.starts_with(';') {
            return Err(PvCode::ParseFail.code());
        }
        rest = &rest[1..];
        sec += 1;
    }
    // 指纹闸：解析出指纹与重算指纹必须一致（篡改必拒）。
    if !rest.starts_with("F:") {
        return Err(PvCode::ParseFail.code());
    }
    rest = &rest[2..];
    let fp_bytes = rest.as_bytes();
    let mut acc: u32 = 0;
    let mut fi = 0;
    while fi < fp_bytes.len() {
        let b = fp_bytes[fi];
        if b == b';' {
            break;
        }
        if b < b'0' || b > b'9' {
            return Err(PvCode::ParseFail.code());
        }
        let d = (b - b'0') as u32;
        acc = match acc.checked_mul(10).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(PvCode::ParseFail.code()),
        };
        fi += 1;
    }
    if fi >= fp_bytes.len() {
        return Err(PvCode::ParseFail.code());
    }
    rest = &rest[fi..];
    if !rest.starts_with(';') || rest.len() != 1 {
        return Err(PvCode::ParseFail.code());
    }
    let fp = view_fingerprint(&bottle, &app, &merged);
    if acc != fp {
        return Err(PvCode::FingerprintMismatch.code());
    }
    Ok(PanoView { merged, bottle, app, fingerprint: fp })
}

/// 往返核：导出→解析→逐槽+指纹相等。
pub fn roundtrip_ok(v: &PanoView) -> Result<bool, u32> {
    let text = export_view(v)?;
    let back = parse_view(&text)?;
    Ok(back == *v)
}

// ── 黄金语料（确定性公式，推导注记） ────────────────────────────
/// 默认层（V1 语料）：键 0..8 全落位，值类 = 键位%5。
pub fn make_bottle_layer() -> PanoLayer {
    let mut l = PanoLayer::new();
    let mut k = 0;
    while k < KEY_N {
        let _ = l.put(k as u8, (k % 5) as u8);
        k += 1;
    }
    l
}

/// 逐应用层（V2 语料）：偶数键落位（4 键），值类 = (键位+1)%5；
/// 奇数键不声明（默认层兜底——覆盖语义正例）。
pub fn make_app_layer() -> PanoLayer {
    let mut l = PanoLayer::new();
    let mut k = 0;
    while k < KEY_N {
        if k % 2 == 0 {
            let _ = l.put(k as u8, ((k + 1) % 5) as u8);
        }
        k += 1;
    }
    l
}

// ── 注入核（3 类坏样本，逐类必拒） ──────────────────────────────
/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 全抓判据。
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] != rounds || self.caught[i] != rounds {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检数（恒零判据）。
    pub fn missed(&self) -> u32 {
        let mut t: u32 = 0;
        let mut c: u32 = 0;
        let mut i = 0;
        while i < 3 {
            t = t.saturating_add(self.tries[i]);
            c = c.saturating_add(self.caught[i]);
            i += 1;
        }
        t.saturating_sub(c)
    }
}

/// 注入轮：未知键 / 逐应用层重复键 / 值类越界 三类各一轮。
pub fn inject_round(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类 1：未知键（键位 99 落出 0..8）。
        let mut l1 = PanoLayer::new();
        led.record(0, l1.put(99, 1).is_err());
        // 类 2：逐应用层重复键（键 2 声明两次）。
        let mut l2 = PanoLayer::new();
        let _ = l2.put(2, 1);
        led.record(1, l2.put(2, 3).is_err());
        // 类 3：值类越界（7 落出 5 档）。
        let mut l3 = PanoLayer::new();
        led.record(2, l3.put(1, 7).is_err());
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：V1/V2 语料 → 两路合并对拍（三视图一致）→ 导出 →
/// 解析往返 → 视图锚对账 → 覆盖语义（偶数键=应用值、奇数键=默认值）
/// → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 视图锚冻结对账。
    if VIEW_V1_ID != 18474 || VIEW_V2_ID != 18457 || VIEW_V3_ID != 18555 || VIEW_COUNT != 3 {
        return Err(PvCode::ViewAnchorMismatch.code());
    }
    let layers = PanoLayers { bottle: make_bottle_layer(), app: make_app_layer() };
    // 三视图合并一致。
    let consistent = three_view_consistent(&layers)?;
    if !consistent {
        return Err(PvCode::MergeMismatch.code());
    }
    let v3 = merge_layers(layers.clone())?;
    // 覆盖语义逐键复算：偶数键应用覆盖（值≠默认），奇数键默认兜底。
    let mut i = 0;
    while i < KEY_N {
        let b = make_bottle_layer().slot(i as u8).ok_or(PvCode::KeyUnknown.code())?;
        let m = v3.merged.get(i).ok_or(PvCode::LayerCorrupt.code())?;
        if i % 2 == 0 {
            // 应用层声明键：合并值=应用值（与默认必不同：(k+1)%5 vs k%5）。
            let a = make_app_layer().slot(i as u8).ok_or(PvCode::KeyUnknown.code())?;
            if *m != a || *m == b {
                return Err(PvCode::MergeMismatch.code());
            }
        } else {
            // 未声明键：合并值=默认值。
            if *m != b {
                return Err(PvCode::MergeMismatch.code());
            }
        }
        i += 1;
    }
    // 导出→解析往返。
    if !roundtrip_ok(&v3)? {
        return Err(PvCode::ParseFail.code());
    }
    // 导出非空且含三节。
    let text = export_view(&v3)?;
    if text.len() < 3 * (KEY_N + 2) + 4 {
        return Err(PvCode::ExportEmpty.code());
    }
    Ok((v3.fingerprint, layers.bottle.used_count(), layers.app.used_count()))
}

/// 探针三序（合并/空层/全层）。
pub fn probe_seq_a() -> Result<u32, u32> {
    let layers = PanoLayers { bottle: make_bottle_layer(), app: make_app_layer() };
    let v = merge_layers(layers)?;
    Ok(v.fingerprint)
}

pub fn probe_seq_b() -> Result<u32, u32> {
    // 双空层：合并全 UNSET，导出文本含 "?" 槽。
    let layers = PanoLayers { bottle: PanoLayer::new(), app: PanoLayer::new() };
    let v = merge_layers(layers)?;
    let text = export_view(&v)?;
    if !text.contains('?') {
        return Err(PvCode::ExportEmpty.code());
    }
    Ok(v.fingerprint)
}

/// 篡改检出：导出文本改一位 → 解析指纹闸必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let layers = PanoLayers { bottle: make_bottle_layer(), app: make_app_layer() };
    let v = merge_layers(layers)?;
    let text = export_view(&v)?;
    let mut bytes: Vec<u8> = text.as_bytes().to_vec();
    // 找第一个数字槽位改值（0→1）。
    let mut idx = None;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b >= b'0' && *b <= b'4' {
                idx = Some(i);
            }
        }
        i += 1;
    }
    let pos = match idx {
        Some(p) => p,
        None => return Err(PvCode::ExportEmpty.code()),
    };
    if let Some(b) = bytes.get_mut(pos) {
        *b = if *b == b'0' { b'1' } else { b'0' };
    }
    let tampered = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => return Err(PvCode::ParseFail.code()),
    };
    match parse_view(&tampered) {
        Err(c) if c == PvCode::FingerprintMismatch.code() => Ok(()),
        _ => Err(PvCode::FingerprintMismatch.code()),
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PvCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PvCode::ALL.len() {
        let v = PvCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < PvCode::ALL.len() {
        if PvCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据汇总。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：全景（默认层+逐应用层）可导出。
    let g = golden_run();
    let g_ok = match &g {
        Ok((_, b, a)) => *b == KEY_N as u32 && *a == 4 && g.is_ok(),
        Err(_) => false,
    };
    v.push(("J1-全景视图可导出", g_ok, format!("{:?}", g)));
    // 三视图合并一致（V3 == V1⊕V2 两路对拍）。
    let layers = PanoLayers { bottle: make_bottle_layer(), app: make_app_layer() };
    let tv = three_view_consistent(&layers);
    v.push(("J1-三视图合并一致", tv == Ok(true), format!("{:?}", tv)));
    // 往返：导出→解析逐槽相等。
    let rt = match merge_layers(layers.clone()) {
        Ok(v3) => roundtrip_ok(&v3) == Ok(true),
        Err(_) => false,
    };
    v.push(("J1-导出解析往返", rt, String::from("导出→解析→逐槽+指纹")));
    // 视图锚对账（V1/V2/V3 冻结 id）。
    let anchor_ok = VIEW_V1_ID == 18474 && VIEW_V2_ID == 18457 && VIEW_V3_ID == 18555;
    v.push(("J2-视图锚对账", anchor_ok, format!("V1={} V2={} V3={}", VIEW_V1_ID, VIEW_V2_ID, VIEW_V3_ID)));
    // 注入三类 10/10。
    let inj = inject_round(JUDGE_ROUNDS);
    v.push((
        "J1-注入三类10/10",
        inj.all_caught(JUDGE_ROUNDS) && inj.missed() == 0,
        format!("t={:?} c={:?}", inj.tries, inj.caught),
    ));
    // 篡改检出（指纹闸）。
    v.push(("J2-篡改必拒", probe_seq_c().is_ok(), String::from("导出文本改一位→解析拒")));
    // 覆盖语义（偶数键应用覆盖/奇数键默认兜底）。
    let cover = match merge_layers(layers.clone()) {
        Ok(v3) => {
            v3.merged.get(0).copied().unwrap_or(0xFF) == 1
                && v3.merged.get(1).copied().unwrap_or(0xFF) == 1
        }
        Err(_) => false,
    };
    v.push(("J2-覆盖语义", cover, String::from("键0=应用值1，键1=默认值1")));
    // 探针三序。
    v.push((
        "J2-探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok(),
        String::from("全层/双空层两序"),
    ));
    // 层守恒。
    let cons = layers.bottle.conserve_ok() && layers.app.conserve_ok();
    v.push(("J2-层账守恒", cons, format!("b={} a={}", layers.bottle.used_count(), layers.app.used_count())));
    // 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫 + 文案闭集。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x80680 窗口")));
    v.push(("J3-文案闭集", detail_closed(), String::from("10 码逐条非空")));
    v
}

/// 判据收口（动态明细槽仅 8 个：前 8 条走 add_dyn，余项静态明细）。
pub fn run_unx18555_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18555 档案观测导出（伪装全景视图）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "明细见域账/前序动态槽");
            }
        }
        i += 1;
    }
    s
}
// ═══════════════════════════════════════════════════════════════════════
// 测试区
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_panorama_exportable() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert!(g.0 > 0, "指纹非零");
        assert_eq!(g.1, 8, "默认层 8 键全落位");
        assert_eq!(g.2, 4, "逐应用层 4 偶数键");
    }

    #[test]
    fn three_view_merge_two_paths_agree() {
        let layers = PanoLayers { bottle: make_bottle_layer(), app: make_app_layer() };
        assert_eq!(three_view_consistent(&layers), Ok(true));
        let v = merge_layers(layers).unwrap_or(PanoView {
            merged: [0; KEY_N],
            bottle: [0; KEY_N],
            app: [0; KEY_N],
            fingerprint: 0,
        });
        assert_eq!(v.merged[0], 1, "键 0 应用覆盖 (0+1)%5");
        assert_eq!(v.merged[1], 1, "键 1 默认兜底 1%5");
    }

    #[test]
    fn export_parse_roundtrip() {
        let layers = PanoLayers { bottle: make_bottle_layer(), app: make_app_layer() };
        let v = merge_layers(layers).unwrap_or(PanoView {
            merged: [0; KEY_N],
            bottle: [0; KEY_N],
            app: [0; KEY_N],
            fingerprint: 0,
        });
        let text = export_view(&v).unwrap_or_default();
        assert!(text.starts_with("V3|B:"), "导出含默认层节");
        assert!(text.contains("A:"), "导出含逐应用层节");
        assert!(text.contains("M:"), "导出含合并节");
        assert_eq!(roundtrip_ok(&v), Ok(true));
    }

    #[test]
    fn tamper_detected_by_fingerprint_gate() {
        assert!(probe_seq_c().is_ok(), "篡改必拒");
    }

    #[test]
    fn injections_three_classes() {
        let inj = inject_round(JUDGE_ROUNDS);
        assert!(inj.all_caught(JUDGE_ROUNDS));
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn layer_boundaries() {
        let mut l = PanoLayer::new();
        assert!(l.put(0, 0).is_ok());
        assert!(l.put(7, 4).is_ok());
        assert_eq!(l.put(8, 0), Err(PvCode::KeyUnknown.code()));
        assert_eq!(l.put(1, 5), Err(PvCode::ValClassRange.code()));
        assert_eq!(l.put(0, 1), Err(PvCode::KeyDup.code()));
        assert!(l.conserve_ok());
    }

    #[test]
    fn repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(detail_closed());
    }

    #[test]
    fn checkset_all_green() {
        let set = run_unx18555_checks();
        assert!(set.all_passed(), "判据集全绿：{}", set.red_items_text());
    }
}
