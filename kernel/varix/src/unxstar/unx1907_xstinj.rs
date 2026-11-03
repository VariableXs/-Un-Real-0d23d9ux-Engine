//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1907 · 扩展状态注入矩阵（第十二矩阵） · 0x72680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B16）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1907-J1 注入矩阵四族八形态全检出（区损坏/丢保存/
//!      恢复错位/XFD 错配），预期声明完备率 100%，与 F1788 组织法一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 扩展区模型：遗留区 512B（id0）+ 头 64B（XSTATE_BV/XCOMP_BV/
//!     全局和/保留零域）+ 恢复偏移表 10×u16 + 扩展组件区（64 对齐，
//!     每区 [区标 u8][保留3][区校验和 u32][载荷]）；
//!   · 四族八形态矩阵（F1788 组织法：族×形态二维冻结，格格带预期码）：
//!     族A 区损坏（A1 头损坏/A2 区校验和坏）、族B 丢保存（B1 整区缺/
//!     B2 截尾）、族C 恢复错位（C1 偏移错位/C2 区对调）、族D XFD 错配
//!     （D1 放行状态缺/D2 状态在 XFD 拒）；
//!   · 预期声明完备率：8 格每格必带预期诊断码（段内窗口、两两互异、
//!     族×形态满覆盖），声明率 <100% 即 MatrixGap 红；
//!   · 检出器逐层裁决：头（BV/保留零域/全局和）→ 表（恢复偏移 vs
//!     典型布局）→ 区（整区零/区标/区校验和/载荷对拍）→ XFD（放行
//!     缺状态/Deny 在状态），先账后区，逐层短路；
//!   · 矩阵机状态机：Idle→Armed→Injected→Adjudicated 单调推进，
//!     倒退/平移显性拒；注入 8 形态 8/8 必抓且逐格期望码对表。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72680，VX3-1103 注入矩阵窗口独占）。
pub const SEG_BASE: u32 = 0x72680;
/// 遗留区尺寸（id0，冻结 512）。
pub const LEGACY_SIZE: usize = 512;
/// 扩展头尺寸（冻结 64）。
pub const HDR_SIZE: usize = 64;
/// 头偏移（遗留区之后，冻结 512）。
pub const HDR_OFF: usize = LEGACY_SIZE;
/// 恢复偏移表条数（组件 id 0..=9，冻结 10）。
pub const RT_N: usize = 10;
/// 恢复偏移表偏移（头之后，冻结 576）。
pub const RT_OFF: usize = HDR_OFF + HDR_SIZE;
/// 扩展区基址（恢复表 20 字节对齐到 64，冻结 640）。
pub const REGION_BASE: usize = 640;
/// 组件 id 上沿（0..=9，冻结 9）。
pub const MAX_COMP_ID: usize = 9;
/// 扩展位掩码（id1..=9，bit0 归遗留区）。
pub const EXT_MASK: u64 = 0x3FE;
/// 缺席恢复表项哨兵值。
pub const RT_ABSENT: u16 = 0xFFFF;
/// 请求掩码（冻结：id1/id3/id5/id8 四组件在册）。
pub const REQUEST_MASK: u64 = (1 << 1) | (1 << 3) | (1 << 5) | (1 << 8);
/// 对齐粒度（每区 64 字节对齐，冻结）。
pub const ALIGN: usize = 64;
/// 矩形态数（四族×两形态，冻结 8）。
pub const MATRIX_CELLS: usize = 8;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XsCode {
    /// 头损坏：BV/保留零域/全局和/声明一致性任一破
    HdrBroken,
    /// 区校验和损坏：区标非在册值、校验和对不上或载荷对拍失真
    CompCrc,
    /// 整区缺：请求在册而区内容全零（保存器漏存特征）
    RegionLost,
    /// 截尾：区长不足以容纳头/恢复表/在册区，先补全再恢复
    Truncated,
    /// 恢复偏移错位：恢复表偏移与典型布局不符，按典型布局重排
    RestoreOff,
    /// 区对调：区标与组件 id 不符（两区整块互换的特征）
    Swap,
    /// XFD 放行但状态缺失：请求在册、XFD 放行而区缺席
    XfdGrantMiss,
    /// 状态在 XFD 拒：区在而 XFD 位拒恢复，先清 XFD 位或裁请求
    XfdDeny,
    /// 矩阵缺格：预期声明不完备或族×形态覆盖不满
    MatrixGap,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl XsCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            XsCode::HdrBroken => "头损坏：查 BV/保留零域/全局和/声明一致性，重出快照头",
            XsCode::CompCrc => "区校验和损坏：逐区定位区标/校验和/载荷失真点",
            XsCode::RegionLost => "整区缺：请求在册而区全零，查保存路径是否漏存该组件",
            XsCode::Truncated => "截尾：区长不足，按典型布局补齐到 REGION_BASE+全区",
            XsCode::RestoreOff => "恢复偏移错位：恢复表与典型布局不符，重算偏移表",
            XsCode::Swap => "区对调：区标与组件 id 不符，按区标回迁各区",
            XsCode::XfdGrantMiss => "XFD 放行但状态缺失：XFD 放行则保存器必须存区，查 rdsspq",
            XsCode::XfdDeny => "状态在 XFD 拒：清 XFD 位或把该组件移出请求掩码",
            XsCode::MatrixGap => "矩阵缺格：补齐预期声明并核查族×形态覆盖",
            XsCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [XsCode; 10] = [
        XsCode::HdrBroken,
        XsCode::CompCrc,
        XsCode::RegionLost,
        XsCode::Truncated,
        XsCode::RestoreOff,
        XsCode::Swap,
        XsCode::XfdGrantMiss,
        XsCode::XfdDeny,
        XsCode::MatrixGap,
        XsCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < XsCode::ALL.len() {
        if XsCode::ALL[i].val() == code {
            return XsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72680 段闭集内"
}

// ── FNV-1a 32 位（区校验和/全局和，整数实现） ───────────────────
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 字节级读写（LE，越界安全） ──────────────────────────────────
pub fn read_u16(b: &[u8], off: usize) -> Option<u16> {
    if off + 2 > b.len() {
        return None;
    }
    Some((b[off] as u16) | ((b[off + 1] as u16) << 8))
}

pub fn read_u32(b: &[u8], off: usize) -> Option<u32> {
    if off + 4 > b.len() {
        return None;
    }
    Some((b[off] as u32) | ((b[off + 1] as u32) << 8) | ((b[off + 2] as u32) << 16)
        | ((b[off + 3] as u32) << 24))
}

pub fn read_u64(b: &[u8], off: usize) -> Option<u64> {
    if off + 8 > b.len() {
        return None;
    }
    let mut v: u64 = 0;
    let mut k = 0usize;
    while k < 8 {
        v |= (b[off + k] as u64) << (k * 8);
        k += 1;
    }
    Some(v)
}

pub fn write_u16(b: &mut [u8], off: usize, v: u16) -> bool {
    if off + 2 > b.len() {
        return false;
    }
    b[off] = v as u8;
    b[off + 1] = (v >> 8) as u8;
    true
}

pub fn write_u32(b: &mut [u8], off: usize, v: u32) -> bool {
    if off + 4 > b.len() {
        return false;
    }
    let mut k = 0usize;
    while k < 4 {
        b[off + k] = (v >> (k * 8)) as u8;
        k += 1;
    }
    true
}

pub fn write_u64(b: &mut [u8], off: usize, v: u64) -> bool {
    if off + 8 > b.len() {
        return false;
    }
    let mut k = 0usize;
    while k < 8 {
        b[off + k] = (v >> (k * 8)) as u8;
        k += 1;
    }
    true
}

// ── 组件表（冻结，F1788 组织法同源） ────────────────────────────
/// 组件描述：(id, 区尺寸)。id0 遗留区恒在；区尺寸含 8 字节区头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompDesc {
    pub id: usize,
    pub size: usize,
}

/// 冻结组件表（id0..=9）。
pub const COMPS: [CompDesc; RT_N] = [
    CompDesc { id: 0, size: 512 },
    CompDesc { id: 1, size: 64 },
    CompDesc { id: 2, size: 64 },
    CompDesc { id: 3, size: 64 },
    CompDesc { id: 4, size: 256 },
    CompDesc { id: 5, size: 1024 },
    CompDesc { id: 6, size: 8 },
    CompDesc { id: 7, size: 16 },
    CompDesc { id: 8, size: 8192 },
    CompDesc { id: 9, size: 32 },
];

/// 组件尺寸查询（越界返回 0）。
pub fn comp_size(id: usize) -> usize {
    if id < RT_N {
        COMPS[id].size
    } else {
        0
    }
}

/// 64 对齐。
pub fn align64(n: usize) -> usize {
    (n + (ALIGN - 1)) & !(ALIGN - 1)
}

// ── 典型布局（恢复偏移的权威基准） ──────────────────────────────
/// 布局条目：(id, off, size)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutEntry {
    pub id: usize,
    pub off: usize,
    pub size: usize,
}

/// 典型布局：id0 遗留区在 0；present 位按 id 升序从 REGION_BASE 排布。
pub fn layout(present: u64) -> Vec<LayoutEntry> {
    let mut v: Vec<LayoutEntry> = Vec::new();
    v.push(LayoutEntry { id: 0, off: 0, size: LEGACY_SIZE });
    let mut off = REGION_BASE;
    let mut id = 1usize;
    while id <= MAX_COMP_ID {
        if present & (1u64 << id) != 0 {
            let sz = comp_size(id);
            v.push(LayoutEntry { id, off, size: sz });
            off = off.saturating_add(align64(sz));
        }
        id += 1;
    }
    v
}

/// 布局总长（含头与恢复表）。
pub fn layout_len(present: u64) -> usize {
    let mut off = REGION_BASE;
    let mut id = 1usize;
    while id <= MAX_COMP_ID {
        if present & (1u64 << id) != 0 {
            off = off.saturating_add(align64(comp_size(id)));
        }
        id += 1;
    }
    off
}

/// 典型布局中某组件偏移（不在册返回 None）。
pub fn layout_off(present: u64, id: usize) -> Option<usize> {
    if id == 0 {
        return Some(0);
    }
    let mut off = REGION_BASE;
    let mut k = 1usize;
    while k <= MAX_COMP_ID {
        if present & (1u64 << k) != 0 {
            if k == id {
                return Some(off);
            }
            off = off.saturating_add(align64(comp_size(k)));
        }
        k += 1;
    }
    None
}

// ── 区内容确定性生成（对拍基准） ────────────────────────────────
/// 遗留区字节模式。
pub fn legacy_byte(k: usize) -> u8 {
    ((k * 7 + 3) & 0xFF) as u8
}

/// 扩展区载荷字节模式（按组件 id 播种）。
pub fn payload_byte(id: usize, k: usize) -> u8 {
    ((id * 31 + k * 7 + 5) & 0xFF) as u8
}

/// 全局和口径：遗留区+头（0..576），除全局和字段自身（528..532）。
/// 恢复表与扩展区不在全局和内——它们的破损由表层/区层各自裁决，
/// 分层不串码（否则区注入全被头层抢先报 HdrBroken）。
fn global_sum(area: &[u8]) -> u32 {
    let lo = HDR_OFF + 16;
    let hi = HDR_OFF + 20;
    let end = HDR_OFF + HDR_SIZE;
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < end && i < area.len() {
        if i < lo || i >= hi {
            h = (h ^ (area[i] as u32)).wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

// ── 建区（确定性快照构造器） ────────────────────────────────────
/// 建扩展状态快照：requests=请求掩码（BV 宣称），present=实际保存位。
/// 正常态 present == requests；D1 注入态 present ⊊ requests。
pub fn build_area(requests: u64, present: u64) -> Result<Vec<u8>, u32> {
    // 请求掩码只允许扩展位（bit0 遗留区由建区器恒置，不在 requests 内）。
    if requests & !EXT_MASK & !1 != 0 {
        return Err(XsCode::HdrBroken.val());
    }
    if present & !requests != 0 {
        return Err(XsCode::HdrBroken.val());
    }
    let len = layout_len(present);
    let mut area: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < len {
        area.push(0);
        i += 1;
    }
    // 遗留区。
    let mut k = 0usize;
    while k < LEGACY_SIZE {
        area[k] = legacy_byte(k);
        k += 1;
    }
    // 头：BV / XCOMP_BV / 全局和 / 保留零域。
    if !write_u64(&mut area, HDR_OFF, requests | 1) {
        return Err(XsCode::Truncated.val());
    }
    if !write_u64(&mut area, HDR_OFF + 8, present) {
        return Err(XsCode::Truncated.val());
    }
    // 全局和先置零再算（此时和字段为 0，口径自洽）。
    let _ = write_u32(&mut area, HDR_OFF + 16, 0);
    let gs = global_sum(&area);
    if !write_u32(&mut area, HDR_OFF + 16, gs) {
        return Err(XsCode::Truncated.val());
    }
    // 恢复偏移表：id0=0；在册=典型偏移；缺席=哨兵。
    let mut id = 0usize;
    while id < RT_N {
        let v: u16 = if id == 0 {
            0
        } else if present & (1u64 << id) != 0 {
            match layout_off(present, id) {
                Some(o) => o as u16,
                None => RT_ABSENT,
            }
        } else {
            RT_ABSENT
        };
        if !write_u16(&mut area, RT_OFF + id * 2, v) {
            return Err(XsCode::Truncated.val());
        }
        id += 1;
    }
    // 扩展区：每区 [区标][保留3][区校验和][载荷]。
    let mut id = 1usize;
    while id <= MAX_COMP_ID {
        if present & (1u64 << id) != 0 {
            let off = match layout_off(present, id) {
                Some(o) => o,
                None => return Err(XsCode::RestoreOff.val()),
            };
            let sz = comp_size(id);
            if off + sz > len {
                return Err(XsCode::Truncated.val());
            }
            area[off] = id as u8;
            let mut p: Vec<u8> = Vec::new();
            let mut kk = 8usize;
            while kk < sz {
                p.push(payload_byte(id, kk - 8));
                kk += 1;
            }
            let crc = fnv1a32(&p);
            if !write_u32(&mut area, off + 4, crc) {
                return Err(XsCode::Truncated.val());
            }
            let mut w = 0usize;
            while w < p.len() {
                area[off + 8 + w] = p[w];
                w += 1;
            }
        }
        id += 1;
    }
    Ok(area)
}

// ── 检出器（逐层裁决：头→表→区→XFD） ────────────────────────────
/// 恢复校验：requests=请求掩码，xfd=XFD 拒绝位图。
/// 全过返回 Ok(裁决摘要串)；任何层破返回 Err(诊断码)。
pub fn verify_area(area: &[u8], requests: u64, xfd: u64) -> Result<String, u32> {
    if area.len() < REGION_BASE {
        return Err(XsCode::Truncated.val());
    }
    // ── 头层 ──
    let bv = match read_u64(area, HDR_OFF) {
        Some(v) => v,
        None => return Err(XsCode::HdrBroken.val()),
    };
    let present = match read_u64(area, HDR_OFF + 8) {
        Some(v) => v,
        None => return Err(XsCode::HdrBroken.val()),
    };
    let gs = match read_u32(area, HDR_OFF + 16) {
        Some(v) => v,
        None => return Err(XsCode::HdrBroken.val()),
    };
    if bv & 1 == 0 {
        return Err(XsCode::HdrBroken.val());
    }
    if bv & !EXT_MASK & !1 != 0 {
        return Err(XsCode::HdrBroken.val());
    }
    if bv & !1 != requests & EXT_MASK {
        // BV 宣称与请求不符（多宣/少宣都算头破）。
        return Err(XsCode::HdrBroken.val());
    }
    if present & !requests & EXT_MASK != 0 {
        // 存了没请求的区：BV/XCOMP_BV 声明不一致。
        return Err(XsCode::HdrBroken.val());
    }
    if gs != global_sum(area) {
        return Err(XsCode::HdrBroken.val());
    }
    // 保留零域 532..576 必须全零。
    let mut k = HDR_OFF + 20;
    while k < HDR_OFF + HDR_SIZE {
        if area[k] != 0 {
            return Err(XsCode::HdrBroken.val());
        }
        k += 1;
    }
    // ── 表层 + 区层 + XFD 层：逐在册组件过筛 ──
    let mut id = 1usize;
    while id <= MAX_COMP_ID {
        let req = requests & (1u64 << id) != 0;
        let has = present & (1u64 << id) != 0;
        let deny = xfd & (1u64 << id) != 0;
        if req && has && deny {
            // D2：状态在而 XFD 拒。
            return Err(XsCode::XfdDeny.val());
        }
        if req && !has && !deny {
            // D1：XFD 放行而状态缺失。
            return Err(XsCode::XfdGrantMiss.val());
        }
        if req && has && !deny {
            let off = match layout_off(present, id) {
                Some(o) => o,
                None => return Err(XsCode::RestoreOff.val()),
            };
            let sz = comp_size(id);
            // 表层：恢复表偏移必须等于典型布局偏移。
            let rt = match read_u16(area, RT_OFF + id * 2) {
                Some(v) => v,
                None => return Err(XsCode::Truncated.val()),
            };
            if rt as usize != off {
                return Err(XsCode::RestoreOff.val());
            }
            // 区层：越界 → 全零 → 区标 → 校验和 → 载荷对拍。
            if off + sz > area.len() {
                return Err(XsCode::Truncated.val());
            }
            let mut zero = true;
            let mut j = off;
            while j < off + sz {
                if area[j] != 0 {
                    zero = false;
                    break;
                }
                j += 1;
            }
            if zero {
                return Err(XsCode::RegionLost.val());
            }
            let tag = area[off] as usize;
            if tag != id {
                if tag >= 1 && tag <= MAX_COMP_ID {
                    return Err(XsCode::Swap.val());
                }
                return Err(XsCode::CompCrc.val());
            }
            let crc_stored = match read_u32(area, off + 4) {
                Some(v) => v,
                None => return Err(XsCode::Truncated.val()),
            };
            let mut p: Vec<u8> = Vec::new();
            let mut w = 8usize;
            while w < sz {
                p.push(area[off + w]);
                w += 1;
            }
            if crc_stored != fnv1a32(&p) {
                return Err(XsCode::CompCrc.val());
            }
            let mut w = 0usize;
            while w < p.len() {
                if p[w] != payload_byte(id, w) {
                    return Err(XsCode::CompCrc.val());
                }
                w += 1;
            }
        }
        id += 1;
    }
    // 遗留区对拍（恒在）。
    let mut k = 0usize;
    while k < LEGACY_SIZE {
        if area[k] != legacy_byte(k) {
            return Err(XsCode::CompCrc.val());
        }
        k += 1;
    }
    Ok(format!("头=清 表=清 区={} 清 XFD={:#x}", present & EXT_MASK, xfd))
}

// ── 矩阵（四族八形态，F1788 组织法） ────────────────────────────
/// 矩阵格：族×形态 + 预期声明（诊断码）+ 形态名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixCell {
    /// 族：0=区损坏 1=丢保存 2=恢复错位 3=XFD 错配。
    pub family: u8,
    /// 形态：族内序号 0/1。
    pub form: u8,
    /// 预期诊断码（声明完备率判据的锚）。
    pub expect: u32,
    pub name: &'static str,
}

/// 冻结矩阵：8 格两两互异，族×形态满覆盖。
pub const MATRIX: [MatrixCell; MATRIX_CELLS] = [
    MatrixCell { family: 0, form: 0, expect: XsCode::HdrBroken.val(), name: "A1 头损坏" },
    MatrixCell { family: 0, form: 1, expect: XsCode::CompCrc.val(), name: "A2 区校验和坏" },
    MatrixCell { family: 1, form: 0, expect: XsCode::RegionLost.val(), name: "B1 整区缺" },
    MatrixCell { family: 1, form: 1, expect: XsCode::Truncated.val(), name: "B2 截尾" },
    MatrixCell { family: 2, form: 0, expect: XsCode::RestoreOff.val(), name: "C1 偏移错位" },
    MatrixCell { family: 2, form: 1, expect: XsCode::Swap.val(), name: "C2 区对调" },
    MatrixCell { family: 3, form: 0, expect: XsCode::XfdGrantMiss.val(), name: "D1 放行状态缺" },
    MatrixCell { family: 3, form: 1, expect: XsCode::XfdDeny.val(), name: "D2 状态在XFD拒" },
];

/// 首在册扩展组件 id（注入定位用，恒为 1）。
pub fn first_present_ext(present: u64) -> usize {
    let mut id = 1usize;
    while id <= MAX_COMP_ID {
        if present & (1u64 << id) != 0 {
            return id;
        }
        id += 1;
    }
    MAX_COMP_ID + 1
}

/// 次在册扩展组件 id（对调注入用，恒为 3）。
pub fn second_present_ext(present: u64) -> usize {
    let first = first_present_ext(present);
    let mut id = first + 1;
    while id <= MAX_COMP_ID {
        if present & (1u64 << id) != 0 {
            return id;
        }
        id += 1;
    }
    MAX_COMP_ID + 1
}

/// 注入第 kind 形态（0..8），返回注入后待验三元组。
/// 返回 (注入区, 请求掩码, XFD 位图)；kind 6 直接重建区。
pub fn inject_cell(kind: usize) -> Result<(Vec<u8>, u64, u64), u32> {
    let area = build_area(REQUEST_MASK, REQUEST_MASK)?;
    let mut bad: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < area.len() {
        bad.push(area[i]);
        i += 1;
    }
    let id1 = first_present_ext(REQUEST_MASK);
    let id2 = second_present_ext(REQUEST_MASK);
    match kind % 8 {
        0 => {
            // A1 头损坏：保留零域写脏。
            bad[HDR_OFF + 24] = 0xA1;
            Ok((bad, REQUEST_MASK, 0))
        }
        1 => {
            // A2 区校验和坏：翻首个在册区载荷一字节。
            let off = match layout_off(REQUEST_MASK, id1) {
                Some(o) => o,
                None => return Err(XsCode::RestoreOff.val()),
            };
            bad[off + 8] ^= 0xFF;
            Ok((bad, REQUEST_MASK, 0))
        }
        2 => {
            // B1 整区缺：首在册区整块清零（含区标与校验和）。
            let off = match layout_off(REQUEST_MASK, id1) {
                Some(o) => o,
                None => return Err(XsCode::RestoreOff.val()),
            };
            let sz = comp_size(id1);
            let mut j = off;
            while j < off + sz && j < bad.len() {
                bad[j] = 0;
                j += 1;
            }
            Ok((bad, REQUEST_MASK, 0))
        }
        3 => {
            // B2 截尾：砍掉尾部在册区的大半载荷。
            let off = match layout_off(REQUEST_MASK, id2) {
                Some(o) => o,
                None => return Err(XsCode::RestoreOff.val()),
            };
            let cut = off + 8;
            bad.truncate(cut);
            Ok((bad, REQUEST_MASK, 0))
        }
        4 => {
            // C1 偏移错位：恢复表首在册项 +64。
            let off = match layout_off(REQUEST_MASK, id1) {
                Some(o) => o,
                None => return Err(XsCode::RestoreOff.val()),
            };
            if !write_u16(&mut bad, RT_OFF + id1 * 2, (off + 64) as u16) {
                return Err(XsCode::Truncated.val());
            }
            Ok((bad, REQUEST_MASK, 0))
        }
        5 => {
            // C2 区对调：两个等宽在册区整块互换。
            let o1 = match layout_off(REQUEST_MASK, id1) {
                Some(o) => o,
                None => return Err(XsCode::RestoreOff.val()),
            };
            let o2 = match layout_off(REQUEST_MASK, id2) {
                Some(o) => o,
                None => return Err(XsCode::RestoreOff.val()),
            };
            let sz = comp_size(id1);
            if sz != comp_size(id2) {
                return Err(XsCode::Swap.val());
            }
            let mut j = 0usize;
            while j < sz {
                let t = bad[o1 + j];
                bad[o1 + j] = bad[o2 + j];
                bad[o2 + j] = t;
                j += 1;
            }
            Ok((bad, REQUEST_MASK, 0))
        }
        6 => {
            // D1 放行状态缺：BV 仍宣称 id5，实际保存位缺 id5。
            let present = REQUEST_MASK & !(1u64 << 5);
            let rebuilt = build_area(REQUEST_MASK, present)?;
            Ok((rebuilt, REQUEST_MASK, 0))
        }
        _ => {
            // D2 状态在 XFD 拒：XFD 位图置 id1 拒位。
            Ok((bad, REQUEST_MASK, 1u64 << id1))
        }
    }
}

// ── 矩阵机状态机（Idle→Armed→Injected→Adjudicated，单调推进） ───
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixState {
    /// 空闲：矩阵未武装。
    Idle,
    /// 已武装：黄金区就绪。
    Armed,
    /// 已注入：八形态跑毕。
    Injected,
    /// 已裁决：检出账在册。
    Adjudicated,
}

impl MatrixState {
    pub fn rank(self) -> u8 {
        match self {
            MatrixState::Idle => 0,
            MatrixState::Armed => 1,
            MatrixState::Injected => 2,
            MatrixState::Adjudicated => 3,
        }
    }
    pub fn can_advance(self, to: MatrixState) -> bool {
        to.rank() > self.rank()
    }
}

/// 矩阵检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓/错码的格号（诊断用）。
    pub miss_cells: Vec<usize>,
    /// 预期声明完备格数（0..=8）。
    pub declared: u32,
}

impl MatrixLedger {
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
    /// 完备率（bps，10000=100%）。
    pub fn completeness_bps(&self) -> u64 {
        if self.tries == 0 {
            return 0;
        }
        let v = self.declared.saturating_mul(10_000) / self.tries;
        v as u64
    }
}

/// 预期声明完备审计（F1788 组织法一致性）：
/// 8 格齐、族×形态满覆盖、预期码段内互异、格名非空。
pub fn matrix_audit() -> Result<u32, u32> {
    if MATRIX.len() != MATRIX_CELLS {
        return Err(XsCode::MatrixGap.val());
    }
    let mut seen: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < MATRIX.len() {
        let c = MATRIX[i];
        if c.expect < SEG_BASE || c.expect >= SEG_BASE + 0x40 {
            return Err(XsCode::MatrixGap.val());
        }
        if c.family > 3 || c.form > 1 {
            return Err(XsCode::MatrixGap.val());
        }
        if c.name.is_empty() {
            return Err(XsCode::MatrixGap.val());
        }
        let mut k = 0usize;
        while k < seen.len() {
            if seen[k] == c.expect {
                return Err(XsCode::MatrixGap.val());
            }
            k += 1;
        }
        seen.push(c.expect);
        i += 1;
    }
    // 族×形态满覆盖：每族恰两形态。
    let mut fam = 0u8;
    while fam <= 3 {
        let mut hit = 0u32;
        let mut j = 0usize;
        while j < MATRIX.len() {
            if MATRIX[j].family == fam {
                hit += 1;
            }
            j += 1;
        }
        if hit != 2 {
            return Err(XsCode::MatrixGap.val());
        }
        fam += 1;
    }
    Ok(MATRIX.len() as u32)
}

/// 单格执行：黄金区自证 → 注入 → 检出，期望码逐格对表。
pub fn run_cell(kind: usize) -> Result<bool, u32> {
    // 黄金自证：未注入区必须全清（防注入器污染基准）。
    let gold = build_area(REQUEST_MASK, REQUEST_MASK)?;
    verify_area(&gold, REQUEST_MASK, 0)?;
    let (area, req, xfd) = inject_cell(kind)?;
    match verify_area(&area, req, xfd) {
        Err(e) => {
            if kind % 8 < MATRIX.len() {
                Ok(e == MATRIX[kind % 8].expect)
            } else {
                Ok(false)
            }
        }
        Ok(_) => Ok(false),
    }
}

/// 矩阵机：八形态全跑 + 声明完备审计 + 状态机封账。
pub fn run_matrix() -> Result<MatrixLedger, u32> {
    let mut m = MatrixMachine::new();
    m.arm()?;
    let declared = matrix_audit()?;
    let mut led = MatrixLedger { tries: 0, caught: 0, miss_cells: Vec::new(), declared };
    let mut k = 0usize;
    while k < MATRIX_CELLS {
        let hit = m.inject(k)?;
        led.tries += 1;
        if hit {
            led.caught += 1;
        } else {
            led.miss_cells.push(k);
        }
        k += 1;
    }
    m.adjudicate()?;
    Ok(led)
}

/// 矩阵机（状态机 + 检出账）。
#[derive(Clone, Debug)]
pub struct MatrixMachine {
    pub state: MatrixState,
    pub hits: u32,
    pub runs: u32,
}

impl MatrixMachine {
    pub fn new() -> MatrixMachine {
        MatrixMachine { state: MatrixState::Idle, hits: 0, runs: 0 }
    }

    pub fn advance(&mut self, to: MatrixState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(XsCode::NotReproducible.val());
        }
        self.state = to;
        Ok(())
    }

    /// 武装：黄金区自证（Idle→Armed）。
    pub fn arm(&mut self) -> Result<(), u32> {
        let gold = build_area(REQUEST_MASK, REQUEST_MASK)?;
        verify_area(&gold, REQUEST_MASK, 0)?;
        self.advance(MatrixState::Armed)
    }

    /// 单格注入+检出（Armed→Injected 首格时推进）。
    pub fn inject(&mut self, kind: usize) -> Result<bool, u32> {
        if self.state != MatrixState::Armed && self.state != MatrixState::Injected {
            return Err(XsCode::MatrixGap.val());
        }
        if self.state == MatrixState::Armed {
            self.advance(MatrixState::Injected)?;
        }
        let hit = run_cell(kind)?;
        self.runs += 1;
        if hit {
            self.hits += 1;
        }
        Ok(hit)
    }

    /// 封账（Injected→Adjudicated；未注入完即封 = MatrixGap 拒）。
    pub fn adjudicate(&mut self) -> Result<(u32, u32), u32> {
        if self.state != MatrixState::Injected {
            return Err(XsCode::MatrixGap.val());
        }
        self.advance(MatrixState::Adjudicated)?;
        Ok((self.runs, self.hits))
    }
}

// ── XFD 往返探针（放行/拒收/缺状态三态语义） ────────────────────
pub fn xfd_probe() -> Result<u32, u32> {
    let area = build_area(REQUEST_MASK, REQUEST_MASK)?;
    // 全放行 → 清。
    verify_area(&area, REQUEST_MASK, 0)?;
    // 拒 id1 → XfdDeny。
    if verify_area(&area, REQUEST_MASK, 1 << 1) != Err(XsCode::XfdDeny.val()) {
        return Err(XsCode::XfdDeny.val());
    }
    // 拒未请求位（幂等无害）→ 清。
    verify_area(&area, REQUEST_MASK, 1 << 6)?;
    // 状态缺 + 放行 → XfdGrantMiss。
    let thin = build_area(REQUEST_MASK, REQUEST_MASK & !(1u64 << 5))?;
    if verify_area(&thin, REQUEST_MASK, 0) != Err(XsCode::XfdGrantMiss.val()) {
        return Err(XsCode::XfdGrantMiss.val());
    }
    // 状态缺 + 拒 → 清（XFD 合法挡下）。
    verify_area(&thin, REQUEST_MASK, 1 << 5)?;
    Ok(4)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：黄金区清 → 八形态 8/8 → 声明完备率 100% → XFD 三态 →
/// 状态机三拒探。返回 (正旗数, 拒旗数, 检出格数, 完备率 bps)。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    // ① 黄金区自证。
    let gold = build_area(REQUEST_MASK, REQUEST_MASK)?;
    verify_area(&gold, REQUEST_MASK, 0)?;
    let clean_ok = true;
    // ② 矩阵八形态全检出。
    let led = run_matrix()?;
    let matrix_ok = led.all_caught();
    // ③ 声明完备率 100%。
    let complete_ok = led.completeness_bps() == 10_000 && led.declared == MATRIX_CELLS as u32;
    // ④ XFD 三态语义。
    let xfd_hits = xfd_probe()?;
    let xfd_ok = xfd_hits == 4;
    // 拒旗三路：矩阵机倒退 / 空区截尾 / 坏码查表。
    let mut m = MatrixMachine::new();
    let rej1 = m.adjudicate().is_err();
    let short: Vec<u8> = Vec::new();
    let rej2 = verify_area(&short, REQUEST_MASK, 0) == Err(XsCode::Truncated.val());
    let rej3 = detail(SEG_BASE + 0x40).starts_with("未知诊断码");
    let flags = clean_ok as u32 + matrix_ok as u32 + complete_ok as u32 + xfd_ok as u32;
    let rejects = rej1 as u32 + rej2 as u32 + rej3 as u32;
    Ok((flags, rejects, led.caught, led.completeness_bps()))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正三拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 3, 8, 10_000)) => Ok(()),
        _ => Err(XsCode::NotReproducible.val()),
    }
}

/// 序 B：矩阵八形态逐格全抓 + 漏账必空。
pub fn probe_seq_b() -> Result<(), u32> {
    match run_matrix() {
        Ok(l) if l.all_caught() && l.miss_cells.is_empty() => Ok(()),
        _ => Err(XsCode::MatrixGap.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72680 && XsCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = LEGACY_SIZE == 512 && HDR_SIZE == 64 && HDR_OFF == 512 && RT_OFF == 576
        && REGION_BASE == 640 && RT_N == 10 && MAX_COMP_ID == 9 && REQUEST_MASK == 0x12A
        && MATRIX_CELLS == 8 && in_window;
    if ok {
        Ok(())
    } else {
        Err(XsCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(XsCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < XsCode::ALL.len() {
        let v = XsCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 布局对拍探针（两条代码路径互证） ────────────────────────────
pub fn layout_probe() -> bool {
    // layout() 与 layout_off() 逐组件互证。
    let mut ok = true;
    let mut id = 1usize;
    while id <= MAX_COMP_ID {
        let l = layout(REQUEST_MASK);
        let a = l.iter().any(|e| e.id == id && Some(e.off) == layout_off(REQUEST_MASK, id));
        if REQUEST_MASK & (1u64 << id) != 0 {
            ok = ok && a;
        }
        id += 1;
    }
    // 总长与最末区端一致。
    let l = layout(REQUEST_MASK);
    let last = l[l.len() - 1];
    ok = ok && layout_len(REQUEST_MASK) == last.off + align64(last.size);
    // 对齐约束：每区偏移 64 对齐。
    let mut j = 0usize;
    while j < l.len() {
        ok = ok && l[j].off % ALIGN == 0;
        j += 1;
    }
    ok
}

// ── 诊断码闭集探针 ──────────────────────────────────────────────
pub fn code_table_probe() -> bool {
    let mut ok = XsCode::ALL.len() == 10;
    let mut i = 0;
    while i < XsCode::ALL.len() {
        let c = XsCode::ALL[i];
        ok = ok && c.val() == SEG_BASE + i as u32 && !detail(c.val()).is_empty()
            && detail(c.val()) == c.detail();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x40).starts_with("未知诊断码");
    ok
}

// ── 状态机守卫探针（四态推进 + 倒退/平移拒） ────────────────────
pub fn state_machine_probe() -> bool {
    let mut m = MatrixMachine::new();
    let m1 = m.advance(MatrixState::Armed).is_ok();
    let b1 = m.advance(MatrixState::Idle).is_err();
    let b2 = m.advance(MatrixState::Armed).is_err();
    let m2 = m.advance(MatrixState::Injected).is_ok();
    let m3 = m.advance(MatrixState::Adjudicated).is_ok();
    let b3 = m.advance(MatrixState::Idle).is_err();
    m1 && b1 && b2 && m2 && m3 && b3
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let (flags, rejects, caught, bps) = golden_run()?;
    if flags != 4 || rejects != 3 || caught != MATRIX_CELLS as u32 || bps != 10_000 {
        return Err(XsCode::NotReproducible.val());
    }
    Ok(format!(
        "状态=已完成 第十二矩阵收官 flags={} rejects={} caught={}/{} 完备率={}bps",
        flags, rejects, caught, MATRIX_CELLS, bps
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正三拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, c, b)) => (*f == 4 && *r == 3,
            format!("flags={} rejects={} caught={} bps={}", f, r, c, b)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链四正三拒", gp, gd));
    // J1 四族八形态全检出。
    let led = run_matrix();
    let (mp, md) = match &led {
        Ok(l) => (l.all_caught(),
            format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_cells)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-四族八形态全检出", mp, md));
    // J1 预期声明完备率 100% + F1788 组织法审计。
    let audit = matrix_audit();
    let (ap, ad) = match (&led, &audit) {
        (Ok(l), Ok(n)) => (l.completeness_bps() == 10_000 && *n == MATRIX_CELLS as u32,
            format!("declared={}/8 bps={} 审计格={}", l.declared, l.completeness_bps(), n)),
        (_, Err(e)) => (false, format!("err=0x{:x} {}", e, detail(*e))),
        _ => (false, String::from("矩阵未跑")),
    };
    v.push(("J1-声明完备率100%", ap, ad));
    // J1 四族点名：区损坏/丢保存/恢复错位/XFD 错配逐族必在册。
    let fam_ok = MATRIX.iter().any(|c| c.expect == XsCode::HdrBroken.val())
        && MATRIX.iter().any(|c| c.expect == XsCode::RegionLost.val())
        && MATRIX.iter().any(|c| c.expect == XsCode::RestoreOff.val())
        && MATRIX.iter().any(|c| c.expect == XsCode::XfdGrantMiss.val());
    v.push(("J1-四族点名在册", fam_ok, String::from("区损坏/丢保存/恢复错位/XFD错配")));
    // J1 XFD 三态语义。
    let xp = xfd_probe();
    v.push(("J1-XFD三态语义", matches!(&xp, Ok(n) if *n == 4), format!("{:?}", xp)));
    // J1 布局对拍（layout/layout_off/layout_len 三路互证）。
    v.push(("J1-布局对拍", layout_probe(), String::from("三路互证 + 64 对齐")));
    // J2 状态机单调：四态推进 + 倒退/平移拒。
    v.push(("J2-状态机单调", state_machine_probe(), String::from("Idle→Armed→Injected→Adjudicated")));
    // J2 诊断码闭集。
    v.push(("J2-诊断码闭集", code_table_probe(), String::from("10 码 val/detail/ALL 查表一致")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), "0x72680 窗口".to_string()));
    v
}

/// 自检入口（契约命名：run_unxXXXX_checks，XXXX=1907）。
pub fn run_unx1907_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1907 扩展状态注入矩阵（第十二矩阵）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
