//! UNX-F48923 提高指针精确度开关与上报边界 · 0x70F40
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B07）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F48923-J1 开关行为对照账在册（L2 诚实登记）；
//!   ③ UNX-F48924-J1 分界 grep『光标绘制』本域零实现 + F1 联签锚定行。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：开关状态合法性 → 档位应用 → L2 对照账审计 → 联签分界审计 →
//!           上报账 → 复算。
//! 开关必须先于档位应用：重复设同一状态会让「用户点了一次」和「系统读了
//! 一次」无法区分，先拒掉重复设态，toggles 才是人手次数的真实账。L2 对
//! 照账必须诚实：Windows 侧行为未实测的条目一律登记为「L2待闸门补测」，
//! 不允许把未测写成已过。
//! 关键差异：本模块只回答「这个开关现在是什么态、应用后档位是什么、上报
//!           了什么坐标与键态」，不画像素（F1 光标绘制域）、不做双击消费
//!           （D4 USER32 联签）、不做拓扑映射（AI-35 显示域）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48921_pointer_core as p_core;

/// 诊断段基址（0x70F40，B07 第二槽）。
pub const SEG_BASE: u32 = 0x70F40;

/// L2 对照账最少条目数。
pub const L2_CASES: usize = 3;
/// 联签分界最少行数。
pub const SEAM_ROWS: usize = 3;
/// 上报账黄金轮数。
pub const REPORT_ROUNDS: u32 = 1000;
/// 开关往返轮数（卡面 ×10^2 载体）。
pub const ENHANCE_ROUNDS: u32 = 100;
/// 五键位图上限。
pub const BUTTON_MASK: u8 = 0x1F;
/// 上报坐标边界。
pub const POS_BOUND: i64 = 1_000_000;
/// L2 诚实登记状态（未实测条目唯一允许值）。
pub const L2_STATUS: &str = "L2待闸门补测";

// ── 诊断码闭集（本模块窗口 +0x01..+0x0A） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MnCode {
    /// 开关非法（重复设态）
    SwitchBad,
    /// L2 对照账非法（缺行/重复/状态越权）
    L2Bad,
    /// 联签分界非法（属主重复/越界声明）
    SeamBad,
    /// 上报非法（坐标/键位/零序号）
    ReportBad,
    /// 序号断档
    SeqBad,
    /// 时间回退
    TimeBad,
    /// 复算不一致
    ReplayBad,
    /// 渲染回读失败
    RenderBad,
    /// 覆盖不足
    CoverageBad,
    /// 段位越界
    SegmentBad,
}

impl MnCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            MnCode::SwitchBad => "开关非法：同一状态被重复设置",
            MnCode::L2Bad => "L2 对照账非法：缺行/属主重复/状态越权",
            MnCode::SeamBad => "联签分界非法：属主重复或越界声明",
            MnCode::ReportBad => "上报非法：坐标越界或键位含未知位",
            MnCode::SeqBad => "序号断档：上报序号必须逐条加一",
            MnCode::TimeBad => "时间回退：上报 tick 必须非递减",
            MnCode::ReplayBad => "复算不一致：两次黄金账不同",
            MnCode::RenderBad => "渲染回读失败：字段缺失或数值越界",
            MnCode::CoverageBad => "覆盖不足：账本条数低于下限",
            MnCode::SegmentBad => "段位越界：诊断码逃出 0x70F40 窗口",
        }
    }

    pub const ALL: [MnCode; 10] = [
        MnCode::SwitchBad,
        MnCode::L2Bad,
        MnCode::SeamBad,
        MnCode::ReportBad,
        MnCode::SeqBad,
        MnCode::TimeBad,
        MnCode::ReplayBad,
        MnCode::RenderBad,
        MnCode::CoverageBad,
        MnCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < MnCode::ALL.len() {
        if MnCode::ALL[i].val() == code {
            return MnCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 开关 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnhanceState {
    Off,
    On,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnhanceSwitch {
    pub state: EnhanceState,
    pub toggles: u32,
}

impl EnhanceSwitch {
    pub fn new(initial_on: bool) -> EnhanceSwitch {
        EnhanceSwitch {
            state: if initial_on {
                EnhanceState::On
            } else {
                EnhanceState::Off
            },
            toggles: 0,
        }
    }

    pub fn is_on(&self) -> bool {
        self.state == EnhanceState::On
    }

    /// 设态：与当前态相同的设置一律拒绝（SwitchBad），toggles 只记真实变化。
    pub fn set(&mut self, on: bool) -> Result<EnhanceState, u32> {
        let want = if on { EnhanceState::On } else { EnhanceState::Off };
        if want == self.state {
            return Err(MnCode::SwitchBad.val());
        }
        self.state = want;
        self.toggles = self.toggles.saturating_add(1);
        Ok(want)
    }

    /// 应用到设备档：开 → 曲线档，关 → 平坦档；档里没有曲线则 SwitchBad。
    pub fn apply(&self, profile: &mut p_core::PtrProfile) -> Result<(), u32> {
        let mode = if self.is_on() {
            p_core::Mode::Curve
        } else {
            p_core::Mode::Flat
        };
        match profile.set_mode(mode) {
            Ok(()) => Ok(()),
            Err(_) => Err(MnCode::SwitchBad.val()),
        }
    }
}

/// 开关往返 ×10^2：每次开→曲线语义、关→平坦语义，档位即时生效。
pub fn enhance_rounds(rounds: u32) -> Result<(), u32> {
    let curve = p_core::Curve::new(p_core::curve_of(&[(0, 1000), (400, 2000), (1600, 4000)]))?;
    let mut prof = p_core::PtrProfile::curved(p_core::DEFAULT_DPI, curve)?;
    let mut sw = EnhanceSwitch::new(false);
    let mut i = 0u32;
    while i < rounds {
        sw.set(true)?;
        sw.apply(&mut prof)?;
        if prof.mode != p_core::Mode::Curve {
            return Err(MnCode::ReplayBad.val());
        }
        if p_core::gain_for(&prof, 400)? != 2000 {
            return Err(MnCode::SwitchBad.val());
        }
        if p_core::gain_for(&prof, 0)? != 1000 {
            return Err(MnCode::SwitchBad.val());
        }
        sw.set(false)?;
        sw.apply(&mut prof)?;
        if prof.mode != p_core::Mode::Flat {
            return Err(MnCode::ReplayBad.val());
        }
        if p_core::gain_for(&prof, 400)? != 1000 {
            return Err(MnCode::SwitchBad.val());
        }
        if sw.toggles != i.saturating_mul(2).saturating_add(2) {
            return Err(MnCode::SwitchBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(())
}

// ── L2 对照账（诚实登记） ─────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct L2Row {
    pub item: &'static str,
    pub windows_side: &'static str,
    pub kernel_side: &'static str,
    pub status: &'static str,
}

pub fn l2_rows() -> Vec<L2Row> {
    let mut v = Vec::new();
    v.push(L2Row {
        item: "提高指针精确度开关",
        windows_side: "设置页开关 ON/OFF 读回",
        kernel_side: "EnhanceSwitch 拒绝重复设态",
        status: L2_STATUS,
    });
    v.push(L2Row {
        item: "开启后的加速语义",
        windows_side: "低速增益低、高速增益高",
        kernel_side: "Curve 采样点精确 + 段内半值进位",
        status: L2_STATUS,
    });
    v.push(L2Row {
        item: "关闭后的平坦语义",
        windows_side: "增益与速度无关",
        kernel_side: "Flat 档恒速，曲线档保留不丢",
        status: L2_STATUS,
    });
    v
}

/// 对照账审计：至少三行、字段非空、属主（item）两两不同、状态只许诚实值。
pub fn audit_l2(rows: &[L2Row]) -> Result<u32, u32> {
    if rows.len() < L2_CASES {
        return Err(MnCode::L2Bad.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(MnCode::L2Bad.val()),
        };
        if r.item.is_empty() || r.windows_side.is_empty() || r.kernel_side.is_empty() {
            return Err(MnCode::L2Bad.val());
        }
        if r.status != L2_STATUS {
            return Err(MnCode::L2Bad.val());
        }
        let mut j = i + 1;
        while j < rows.len() {
            let o = match rows.get(j) {
                Some(v) => v,
                None => return Err(MnCode::L2Bad.val()),
            };
            if o.item == r.item {
                return Err(MnCode::L2Bad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(rows.len() as u32)
}

// ── 联签分界（F1 光标绘制 / D4 USER32 / AI-35 显示） ──────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeamRow {
    pub owner: &'static str,
    pub seam: &'static str,
    pub this_side: &'static str,
}

pub fn seam_rows() -> Vec<SeamRow> {
    let mut v = Vec::new();
    v.push(SeamRow {
        owner: "F1",
        seam: "光标绘制像素",
        this_side: "本域只输出坐标与键态，不绘制像素",
    });
    v.push(SeamRow {
        owner: "D4/USER32",
        seam: "双击消费序",
        this_side: "本域只做阈值判定，不做消费分发",
    });
    v.push(SeamRow {
        owner: "AI-35/显示",
        seam: "多显示器拓扑映射",
        this_side: "本域只提供衔接位，不做拓扑裁决",
    });
    v
}

/// 「光标绘制」类词元：本域自己的词元表不允许出现其中任何一个。
pub const DRAW_TOKENS: [&str; 3] = ["光标绘制", "draw_cursor", "blit_cursor"];
/// 本域词元：只描述上报与合成，不描述绘制。
pub const OWN_TOKENS: [&str; 4] = ["坐标上报", "键态上报", "指针合成", "位移账本"];
/// 分界声明（逐字可审计）。
pub const BOUNDARY_STMT: &str = "本域只输出坐标与键态，不绘制像素，不做消费分发";

pub fn token_overlap(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || (a.len() >= 2 && b.len() >= 2 && (a.contains(b) || b.contains(a)))
}

/// 词元分界审计：本域词元与绘制词元零重叠，且声明含「不绘制」。
pub fn audit_tokens() -> bool {
    if !BOUNDARY_STMT.contains("不绘制") {
        return false;
    }
    let mut i = 0usize;
    while i < OWN_TOKENS.len() {
        let own = match OWN_TOKENS.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = 0usize;
        while j < DRAW_TOKENS.len() {
            let draw = match DRAW_TOKENS.get(j) {
                Some(v) => v,
                None => return false,
            };
            if token_overlap(own, draw) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 分界审计：三行起、属主两两不同、每行 this_side 必须声明「只」而不越权。
pub fn audit_seam(rows: &[SeamRow]) -> Result<u32, u32> {
    if rows.len() < SEAM_ROWS {
        return Err(MnCode::SeamBad.val());
    }
    if !audit_tokens() {
        return Err(MnCode::SeamBad.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(MnCode::SeamBad.val()),
        };
        if r.owner.is_empty() || r.seam.is_empty() || r.this_side.is_empty() {
            return Err(MnCode::SeamBad.val());
        }
        if !r.this_side.contains("只") {
            return Err(MnCode::SeamBad.val());
        }
        let mut j = i + 1;
        while j < rows.len() {
            let o = match rows.get(j) {
                Some(v) => v,
                None => return Err(MnCode::SeamBad.val()),
            };
            if o.owner == r.owner {
                return Err(MnCode::SeamBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(rows.len() as u32)
}

// ── 上报账 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointerReport {
    pub seq: u32,
    pub x: i64,
    pub y: i64,
    pub buttons: u8,
    pub tick: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReportBook {
    pub rows: u32,
    pub last_seq: u32,
    pub last_tick: u32,
    pub buttons_seen: u8,
    pub digest: u64,
}

impl ReportBook {
    pub fn new() -> ReportBook {
        ReportBook {
            rows: 0,
            last_seq: 0,
            last_tick: 0,
            buttons_seen: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn push(&mut self, r: PointerReport) -> Result<(), u32> {
        if r.buttons & !BUTTON_MASK != 0 {
            return Err(MnCode::ReportBad.val());
        }
        if r.x.unsigned_abs() > POS_BOUND as u64 || r.y.unsigned_abs() > POS_BOUND as u64 {
            return Err(MnCode::ReportBad.val());
        }
        if r.tick == 0 || r.tick < self.last_tick {
            return Err(MnCode::TimeBad.val());
        }
        if self.rows == 0 {
            if r.seq != 1 {
                return Err(MnCode::SeqBad.val());
            }
        } else if r.seq != self.last_seq.saturating_add(1) {
            return Err(MnCode::SeqBad.val());
        }
        self.rows = self.rows.saturating_add(1);
        self.last_seq = r.seq;
        self.last_tick = r.tick;
        self.buttons_seen |= r.buttons;
        self.digest = p_core::fold_u32(self.digest, r.seq);
        self.digest = p_core::fold_u32(self.digest, r.x as u32);
        self.digest = p_core::fold_u32(self.digest, r.y as u32);
        self.digest = p_core::fold_u32(self.digest, r.buttons as u32);
        self.digest = p_core::fold_u32(self.digest, r.tick);
        Ok(())
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.rows == 0 {
            return Err(MnCode::CoverageBad.val());
        }
        if self.last_seq != self.rows {
            return Err(MnCode::SeqBad.val());
        }
        Ok(())
    }
}

/// 黄金上报账：1000 条，键位覆盖五键全图，tick 非递减。
pub fn golden_book() -> Result<ReportBook, u32> {
    let mut book = ReportBook::new();
    let mut lcg = p_core::Lcg::new(0x4892_3001);
    let mut i = 0u32;
    while i < REPORT_ROUNDS {
        let r = PointerReport {
            seq: i.saturating_add(1),
            x: lcg.below(10_001) as i64 - 5_000,
            y: lcg.below(10_001) as i64 - 5_000,
            buttons: lcg.below(32) as u8,
            tick: lcg.below(10).saturating_add(1).saturating_add(i.saturating_mul(10)),
        };
        book.push(r)?;
        i = i.saturating_add(1);
    }
    // 强制五键全覆盖：末五条各点亮一位，保证 buttons_seen == 0x1F。
    let mut b = 0u32;
    while b < 5 {
        let bit = 1u8 << b;
        let r = PointerReport {
            seq: REPORT_ROUNDS.saturating_add(b).saturating_add(1),
            x: 0,
            y: 0,
            buttons: bit,
            tick: book.last_tick.saturating_add(1),
        };
        book.push(r)?;
        b = b.saturating_add(1);
    }
    if book.buttons_seen != BUTTON_MASK {
        return Err(MnCode::CoverageBad.val());
    }
    Ok(book)
}

// ── 渲染与回读 ────────────────────────────────────────────────────
pub fn render_book(b: &ReportBook) -> String {
    format!(
        "{}|{}|{}|0x{:016X}",
        b.rows, b.last_seq, b.buttons_seen, b.digest
    )
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 4 {
        return Err(MnCode::RenderBad.val());
    }
    let rows = p_core::parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(MnCode::RenderBad.val()),
    })
    .map_err(|_| MnCode::RenderBad.val())?;
    let last_seq = p_core::parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(MnCode::RenderBad.val()),
    })
    .map_err(|_| MnCode::RenderBad.val())?;
    let buttons = p_core::parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(MnCode::RenderBad.val()),
    })
    .map_err(|_| MnCode::RenderBad.val())?;
    let digest = p_core::parse_hex16(match parts.get(3) {
        Some(v) => v,
        None => return Err(MnCode::RenderBad.val()),
    })
    .map_err(|_| MnCode::RenderBad.val())?;
    if rows == 0 || last_seq != rows || buttons > BUTTON_MASK as u32 || digest == 0 {
        return Err(MnCode::RenderBad.val());
    }
    Ok(rows)
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
        let mut sw = EnhanceSwitch::new(true);
        if sw.set(true).err() == Some(MnCode::SwitchBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        let mut rows = l2_rows();
        if let Some(first) = rows.get_mut(0) {
            first.status = "已实测";
        }
        if audit_l2(&rows).err() == Some(MnCode::L2Bad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        let mut seam = seam_rows();
        if let Some(first) = seam.get_mut(0) {
            first.owner = "D4/USER32";
        }
        if audit_seam(&seam).err() == Some(MnCode::SeamBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        let mut book = ReportBook::new();
        let _ = book.push(PointerReport {
            seq: 1,
            x: 0,
            y: 0,
            buttons: 0,
            tick: 1,
        });
        let gap = book.push(PointerReport {
            seq: 3,
            x: 0,
            y: 0,
            buttons: 0,
            tick: 2,
        });
        if gap.err() == Some(MnCode::SeqBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    Ok(log)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 探针 A：开关重复设态必拒、往返计数精确、应用后语义即时。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut sw = EnhanceSwitch::new(true);
    if sw.set(true).err() != Some(MnCode::SwitchBad.val()) {
        return Err(MnCode::SwitchBad.val());
    }
    if sw.set(false) != Ok(EnhanceState::Off) {
        return Err(MnCode::SwitchBad.val());
    }
    if sw.set(false).err() != Some(MnCode::SwitchBad.val()) {
        return Err(MnCode::SwitchBad.val());
    }
    if sw.set(true) != Ok(EnhanceState::On) {
        return Err(MnCode::SwitchBad.val());
    }
    if sw.toggles != 2 {
        return Err(MnCode::SwitchBad.val());
    }
    let curve = p_core::Curve::new(p_core::curve_of(&[(0, 1000), (400, 2000), (1600, 4000)]))?;
    let mut prof = p_core::PtrProfile::curved(p_core::DEFAULT_DPI, curve)?;
    sw.set(false)?;
    sw.apply(&mut prof)?;
    if prof.mode != p_core::Mode::Flat {
        return Err(MnCode::ReplayBad.val());
    }
    sw.set(true)?;
    sw.apply(&mut prof)?;
    if prof.mode != p_core::Mode::Curve {
        return Err(MnCode::ReplayBad.val());
    }
    Ok(())
}

/// 探针 B：L2 三行在册、联签三行在册、词元零重叠、篡改必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let rows = l2_rows();
    if audit_l2(&rows)? != L2_CASES as u32 {
        return Err(MnCode::L2Bad.val());
    }
    let mut dup = rows.clone();
    if let Some(first) = dup.get_mut(1) {
        first.item = "提高指针精确度开关";
    }
    if audit_l2(&dup).err() != Some(MnCode::L2Bad.val()) {
        return Err(MnCode::ReplayBad.val());
    }
    let mut forged = rows.clone();
    if let Some(first) = forged.get_mut(2) {
        first.status = "已实测";
    }
    if audit_l2(&forged).err() != Some(MnCode::L2Bad.val()) {
        return Err(MnCode::ReplayBad.val());
    }
    let seam = seam_rows();
    if audit_seam(&seam)? != SEAM_ROWS as u32 {
        return Err(MnCode::SeamBad.val());
    }
    if !audit_tokens() {
        return Err(MnCode::SeamBad.val());
    }
    let mut crossed = seam.clone();
    if let Some(first) = crossed.get_mut(0) {
        first.this_side = "本域绘制像素并报表";
    }
    if audit_seam(&crossed).err() != Some(MnCode::SeamBad.val()) {
        return Err(MnCode::ReplayBad.val());
    }
    Ok(())
}

/// 探针 C：黄金账双跑一致、键位全覆盖、渲染回读一致、序号断档必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let a = golden_book()?;
    let b = golden_book()?;
    if a != b {
        return Err(MnCode::ReplayBad.val());
    }
    if a.buttons_seen != BUTTON_MASK {
        return Err(MnCode::CoverageBad.val());
    }
    a.audit()?;
    let text = render_book(&a);
    if audit_render(&text)? != a.rows {
        return Err(MnCode::RenderBad.val());
    }
    let mut book = ReportBook::new();
    book.push(PointerReport {
        seq: 1,
        x: 1,
        y: 2,
        buttons: 1,
        tick: 5,
    })?;
    if book
        .push(PointerReport {
            seq: 3,
            x: 1,
            y: 2,
            buttons: 1,
            tick: 6,
        })
        .err()
        != Some(MnCode::SeqBad.val())
    {
        return Err(MnCode::SeqBad.val());
    }
    if book
        .push(PointerReport {
            seq: 2,
            x: 1,
            y: 2,
            buttons: 1,
            tick: 4,
        })
        .err()
        != Some(MnCode::TimeBad.val())
    {
        return Err(MnCode::TimeBad.val());
    }
    if book
        .push(PointerReport {
            seq: 2,
            x: 1,
            y: 2,
            buttons: 0x80,
            tick: 7,
        })
        .err()
        != Some(MnCode::ReportBad.val())
    {
        return Err(MnCode::ReportBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_book()?;
    let b = golden_book()?;
    if a != b {
        return Err(MnCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 1;
    let hi = SEG_BASE + MnCode::ALL.len() as u32;
    let mut i = 0usize;
    while i < MnCode::ALL.len() {
        match MnCode::ALL.get(i) {
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
    let sw = enhance_rounds(ENHANCE_ROUNDS);
    v.push((
        "F48923-J1-开关往返×10^2",
        sw.is_ok(),
        match &sw {
            Ok(()) => format!("rounds={} 开↔关语义即时", ENHANCE_ROUNDS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let rows = l2_rows();
    let l2 = audit_l2(&rows);
    v.push((
        "F48923-J1-L2对照账诚实在册",
        l2 == Ok(L2_CASES as u32),
        match &l2 {
            Ok(n) => format!("{} 行，状态逐字 {}", n, L2_STATUS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let seam = seam_rows();
    let sm = audit_seam(&seam);
    v.push((
        "F48924-J1-F1联签锚定行",
        sm == Ok(SEAM_ROWS as u32),
        match &sm {
            Ok(n) => format!("{} 行联签（F1/D4/AI-35）", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48924-J1-光标绘制词元零本域",
        audit_tokens(),
        format!(
            "本域词元 {:?} × 绘制词元 {:?} 零重叠",
            OWN_TOKENS, DRAW_TOKENS
        ),
    ));
    let book = golden_book();
    v.push((
        "F48924-J1-上报账×10^3键位全覆盖",
        match &book {
            Ok(b) => b.audit().is_ok() && b.buttons_seen == BUTTON_MASK,
            Err(_) => false,
        },
        match &book {
            Ok(b) => format!("rows={} buttons_seen=0x{:02X}", b.rows, b.buttons_seen),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48923-J1-重复设态必拒",
        {
            let mut s = EnhanceSwitch::new(false);
            s.set(false).err() == Some(MnCode::SwitchBad.val())
                && s.toggles == 0
                && s.set(true).is_ok()
                && s.toggles == 1
        },
        String::from("SwitchBad，toggles 只记真实变化"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48923-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let render_ok = match golden_book() {
        Ok(b) => {
            let text = render_book(&b);
            match audit_render(&text) {
                Ok(n) => n == b.rows,
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    v.push((
        "F48924-J1-渲染四字段回读",
        render_ok,
        String::from("rows|last_seq|buttons_seen|digest"),
    ));
    v.push((
        "F48923-J1-探针三序与双跑",
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
        "F48923-J1-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 {} 码", SEG_BASE, MnCode::ALL.len()),
    ));
    v
}

pub fn run_unx48923_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48923 指针精确度开关与上报边界");
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
    fn switch_rejects_same_state() {
        let mut s = EnhanceSwitch::new(true);
        assert_eq!(s.set(true).err(), Some(MnCode::SwitchBad.val()));
        assert_eq!(s.toggles, 0);
        assert_eq!(s.set(false), Ok(EnhanceState::Off));
        assert_eq!(s.toggles, 1);
        assert_eq!(s.set(false).err(), Some(MnCode::SwitchBad.val()));
        assert_eq!(s.set(true), Ok(EnhanceState::On));
        assert_eq!(s.toggles, 2);
    }

    #[test]
    fn switch_applies_profile_mode() {
        match p_core::Curve::new(p_core::curve_of(&[(0, 1000), (400, 2000)])) {
            Ok(c) => match p_core::PtrProfile::curved(1000, c) {
                Ok(mut p) => {
                    let mut s = EnhanceSwitch::new(false);
                    assert!(s.apply(&mut p).is_ok());
                    assert_eq!(p.mode, p_core::Mode::Flat);
                    assert!(s.set(true).is_ok());
                    assert!(s.apply(&mut p).is_ok());
                    assert_eq!(p.mode, p_core::Mode::Curve);
                }
                Err(_) => assert!(false),
            },
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn switch_apply_without_curve_fails() {
        match p_core::PtrProfile::flat(1000, 1000) {
            Ok(mut p) => {
                let s = EnhanceSwitch::new(true);
                assert_eq!(s.apply(&mut p), Err(MnCode::SwitchBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn l2_rows_are_honest() {
        let rows = l2_rows();
        assert_eq!(audit_l2(&rows), Ok(3));
        assert_eq!(rows.len(), L2_CASES);
        let mut forged = rows.clone();
        if let Some(r) = forged.get_mut(0) {
            r.status = "已实测";
        }
        assert_eq!(audit_l2(&forged), Err(MnCode::L2Bad.val()));
    }

    #[test]
    fn seam_rows_are_disjoint() {
        let rows = seam_rows();
        assert_eq!(audit_seam(&rows), Ok(3));
        assert!(audit_tokens());
        assert!(!token_overlap("指针合成", "draw_cursor"));
        assert!(token_overlap("光标绘制像素", "光标绘制"));
    }

    #[test]
    fn report_seq_and_time_enforced() {
        let mut b = ReportBook::new();
        assert_eq!(
            b.push(PointerReport {
                seq: 2,
                x: 0,
                y: 0,
                buttons: 0,
                tick: 1
            })
            .err(),
            Some(MnCode::SeqBad.val())
        );
        assert!(b
            .push(PointerReport {
                seq: 1,
                x: 0,
                y: 0,
                buttons: 0,
                tick: 1
            })
            .is_ok());
        assert_eq!(
            b.push(PointerReport {
                seq: 2,
                x: 0,
                y: 0,
                buttons: 0,
                tick: 0
            })
            .err(),
            Some(MnCode::TimeBad.val())
        );
        assert_eq!(
            b.push(PointerReport {
                seq: 2,
                x: 0,
                y: 0,
                buttons: 0xFF,
                tick: 2
            })
            .err(),
            Some(MnCode::ReportBad.val())
        );
        assert!(b
            .push(PointerReport {
                seq: 2,
                x: -5,
                y: 6,
                buttons: 0x1F,
                tick: 2
            })
            .is_ok());
        assert_eq!(b.last_seq, 2);
        assert_eq!(b.buttons_seen, 0x1F);
    }

    #[test]
    fn golden_books_are_deterministic() {
        let a = golden_book();
        let b = golden_book();
        assert_eq!(a, b);
        match a {
            Ok(book) => {
                assert!(book.audit().is_ok());
                assert_eq!(book.rows, REPORT_ROUNDS + 5);
                assert_eq!(book.buttons_seen, BUTTON_MASK);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_pass() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(enhance_rounds(ENHANCE_ROUNDS).is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn injection_catches_all_four() {
        match inject_rounds(5) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.route(0), 5);
                assert_eq!(l.route(3), 5);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        match golden_book() {
            Ok(b) => {
                let text = render_book(&b);
                assert_eq!(audit_render(&text), Ok(b.rows));
                let bad = text.replacen("|", "!", 1);
                assert!(audit_render(&bad).is_err());
                let zero = "0|0|0|0x0000000000000000";
                assert!(audit_render(zero).is_err());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn codes_inside_window() {
        assert!(segment_guard());
        assert_eq!(MnCode::ALL.len(), 10);
        assert_eq!(detail(MnCode::L2Bad.val()), MnCode::L2Bad.detail());
        assert_eq!(detail(0), "未知诊断码");
    }
}
