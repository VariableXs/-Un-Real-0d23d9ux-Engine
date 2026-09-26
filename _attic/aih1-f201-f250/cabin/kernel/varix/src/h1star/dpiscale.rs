//! F224 显示缩放用户档位 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F224「显示缩放用户档位」。
//!
//! **验收标准（主册第一句）**：四档全界面走查截图各一套；跨屏拖窗重
//! 采样清晰度比对（放大镜取证）；免重启生效实测（切换到可用 <1s）；
//! 双 DPI 配置用例。
//!
//! **设计要点**：
//! - 四档 100%/125%/150%/200%（×100 整数定点），设置中心即时预览、
//!   免重启生效（切换到可用 <1s：重排+重栅格化的成本核算门）；
//! - 混合 DPI 双显示器**各自独立取档**（per-monitor 表）；
//! - 跨屏拖窗重采样：矢量资产直接重栅格化、位图按 4K 管线高清源缩放
//!   （禁止低清源上采样——不模糊的结构保证）；
//! - 档位持久化到用户配置（换 U 盘宿主机跟着走）。
//!
//! **依赖锚点**：F068（4K 资产管线）、F214（最小尺寸自适应）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量（一处一事实）
// ---------------------------------------------------------------------------

/// 免重启生效预算——主册 F224：「切换到可用 <1s」。
pub const HOT_APPLY_BUDGET_MS: u32 = 1000;

/// 四档走查样张套数——主册 F224：「四档全界面走查截图各一套」。
pub const WALK_TIER_SETS: usize = 4;

// ---------------------------------------------------------------------------
// 缩放档位
// ---------------------------------------------------------------------------

/// DPI 缩放档位（×100 整数定点：100 = 100%）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScaleTier {
    S100,
    S125,
    S150,
    S200,
}

impl ScaleTier {
    /// 缩放系数（×100 定点）。
    pub fn factor(self) -> u32 {
        match self {
            ScaleTier::S100 => 100,
            ScaleTier::S125 => 125,
            ScaleTier::S150 => 150,
            ScaleTier::S200 => 200,
        }
    }

    /// 全部档位（走查矩阵用）。
    pub const ALL: [ScaleTier; 4] = [ScaleTier::S100, ScaleTier::S125, ScaleTier::S150, ScaleTier::S200];
}

/// 几何换算：物理尺寸 = 逻辑尺寸 × 系数（四舍五入，±0.5px 误差内）。
pub fn scale_px(logical: i32, tier: ScaleTier) -> i32 {
    (logical as i64 * tier.factor() as i64 + 50) as i32 / 100
}

/// 逻辑回算：物理尺寸 → 逻辑尺寸（跨屏重排用，同精度约定）。
pub fn unscale_px(physical: i32, tier: ScaleTier) -> i32 {
    (physical as i64 * 100 + tier.factor() as i64 / 2) as i32 / tier.factor() as i32
}

// ---------------------------------------------------------------------------
// 每显示器独立档位（双 DPI 配置）
// ---------------------------------------------------------------------------

/// 显示器档位表：每屏独立取档（混合 DPI 判据的落点）。
#[derive(Clone, Debug, Default)]
pub struct MonitorScaleMap {
    scales: Vec<(String, ScaleTier)>,
}

impl MonitorScaleMap {
    pub fn new() -> MonitorScaleMap {
        MonitorScaleMap { scales: Vec::new() }
    }

    /// 设置某屏档位（显示器按稳定标识 EDID 字符串索引）。
    pub fn set(&mut self, monitor: &str, tier: ScaleTier) {
        if let Some(slot) = self.scales.iter_mut().find(|(m, _)| m == monitor) {
            slot.1 = tier;
        } else {
            self.scales.push((monitor.to_string(), tier));
        }
    }

    /// 读取某屏档位（未登记默认 100%——不猜）。
    pub fn get(&self, monitor: &str) -> ScaleTier {
        self.scales
            .iter()
            .find(|(m, _)| m == monitor)
            .map(|(_, t)| *t)
            .unwrap_or(ScaleTier::S100)
    }

    pub fn len(&self) -> usize {
        self.scales.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scales.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 跨屏重采样策略（不模糊的结构保证）
// ---------------------------------------------------------------------------

/// 资产类型（重采样通路分派依据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// 矢量（图标轮廓/字体）——直接按目标档重栅格化。
    Vector,
    /// 位图（有 4K 高清源——按高清源缩放）。
    BitmapWithHiRes,
    /// 位图（无高清源——**只许降采样不许上采样**，上采样=模糊）。
    BitmapOnly,
}

/// 重采样决策：矢量重栅格 ✓ / 高清源缩放 ✓ / 无源位图上采样 ✗（拒绝，
/// 如实保留原始尺寸并标注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResampleDecision {
    ReRasterize,
    ScaleFromHiRes,
    RefuseUpscale,
}

/// 重采样判定（策略唯一实现点）。
pub fn resample_decision(kind: AssetKind, from: ScaleTier, to: ScaleTier) -> ResampleDecision {
    match kind {
        AssetKind::Vector => ResampleDecision::ReRasterize,
        AssetKind::BitmapWithHiRes => ResampleDecision::ScaleFromHiRes,
        AssetKind::BitmapOnly => {
            if to.factor() > from.factor() {
                ResampleDecision::RefuseUpscale
            } else {
                ResampleDecision::ScaleFromHiRes // 降采样走同一通路（高质量）
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 免重启生效（成本核算门）
// ---------------------------------------------------------------------------

/// 切换生效的重排操作（耗时由调用方按实测表注入）。
pub struct ApplyOp {
    pub name: &'static str,
    pub cost_ms: u32,
}

/// 切换成本核算：全部重排+重栅格操作总耗时 < 1s 才允许热切换
/// （超预算 → 走注销重登路径并如实告知——绝不假装即时）。
pub fn hot_apply_within_budget(ops: &[ApplyOp]) -> bool {
    let total: u32 = ops.iter().map(|o| o.cost_ms).sum();
    total > 0 && total < HOT_APPLY_BUDGET_MS
}

// ---------------------------------------------------------------------------
// 持久化（换 U 盘宿主机跟着走）
// ---------------------------------------------------------------------------

/// 档位持久化往返：序列化为 `monitor=tier` 行集（开放格式，可手工编辑）。
pub fn persist_lines(map: &MonitorScaleMap) -> Vec<String> {
    (0..map.len())
        .filter_map(|i| {
            map.scales.get(i).map(|(m, t)| format!("{}={}", m, tier_tag(*t)))
        })
        .collect()
}

fn tier_tag(t: ScaleTier) -> &'static str {
    match t {
        ScaleTier::S100 => "100",
        ScaleTier::S125 => "125",
        ScaleTier::S150 => "150",
        ScaleTier::S200 => "200",
    }
}

/// 从行集恢复（未知行如实跳过——损坏容错不炸）。
pub fn restore_lines(lines: &[String], map: &mut MonitorScaleMap) -> usize {
    let mut n = 0;
    for l in lines {
        let Some((m, t)) = l.split_once('=') else { continue };
        let tier = match t {
            "100" => ScaleTier::S100,
            "125" => ScaleTier::S125,
            "150" => ScaleTier::S150,
            "200" => ScaleTier::S200,
            _ => continue,
        };
        map.set(m, tier);
        n += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F224 自检（判据面：四档矩阵 + 双 DPI + 重采样策略 + 热切换预算 + 持久化）。
pub fn run_dpiscale_checks() -> CheckSet {
    let mut set = CheckSet::new("F224-dpiscale");

    // 1. 四档全量在册（走查矩阵 4 套）。
    set.add(
        "four tiers registered",
        ScaleTier::ALL.len() == WALK_TIER_SETS
            && ScaleTier::ALL.iter().map(|t| t.factor()).eq([100, 125, 150, 200]),
        "",
    );

    // 2. 几何换算往返误差 ≤0.5px（100/150/200 三档抽样）。
    set.add(
        "scale roundtrip within half px",
        scale_px(200, ScaleTier::S100) == 200
            && scale_px(200, ScaleTier::S150) == 300
            && scale_px(200, ScaleTier::S200) == 400
            && unscale_px(300, ScaleTier::S150) == 200
            && unscale_px(scale_px(137, ScaleTier::S125), ScaleTier::S125) >= 136
            && unscale_px(scale_px(137, ScaleTier::S125), ScaleTier::S125) <= 137,
        "",
    );

    // 3. 双 DPI 配置：两屏各取各档、互不串扰。
    let mut m = MonitorScaleMap::new();
    m.set("EDID-A", ScaleTier::S150);
    m.set("EDID-B", ScaleTier::S100);
    set.add(
        "dual dpi independent tiers",
        m.len() == 2 && m.get("EDID-A") == ScaleTier::S150 && m.get("EDID-B") == ScaleTier::S100,
        "",
    );

    // 4. 未登记屏默认 100%（不猜档）。
    set.add("unregistered monitor defaults 100", m.get("EDID-C") == ScaleTier::S100, "");

    // 5. 重采样策略：矢量重栅格、高清源缩放、无源位图拒绝上采样。
    set.add(
        "resample policy refuses upscale",
        resample_decision(AssetKind::Vector, ScaleTier::S100, ScaleTier::S200) == ResampleDecision::ReRasterize
            && resample_decision(AssetKind::BitmapWithHiRes, ScaleTier::S100, ScaleTier::S200)
                == ResampleDecision::ScaleFromHiRes
            && resample_decision(AssetKind::BitmapOnly, ScaleTier::S100, ScaleTier::S125)
                == ResampleDecision::RefuseUpscale
            && resample_decision(AssetKind::BitmapOnly, ScaleTier::S200, ScaleTier::S100)
                == ResampleDecision::ScaleFromHiRes,
        "",
    );

    // 6. 热切换预算：典型重排表 <1s 放行、超预算拒绝（诚实走重登）。
    let ok_ops = [
        ApplyOp { name: "relayout", cost_ms: 300 },
        ApplyOp { name: "re-raster", cost_ms: 400 },
        ApplyOp { name: "wallpaper", cost_ms: 100 },
    ];
    let slow_ops = [ApplyOp { name: "relayout", cost_ms: 900 }, ApplyOp { name: "re-raster", cost_ms: 300 }];
    set.add(
        "hot apply budget 1s gate",
        hot_apply_within_budget(&ok_ops) && !hot_apply_within_budget(&slow_ops),
        "",
    );

    // 7. 持久化往返：保存后恢复逐屏等值；未知行跳过不炸。
    let mut m = MonitorScaleMap::new();
    m.set("EDID-A", ScaleTier::S200);
    m.set("EDID-B", ScaleTier::S125);
    let lines = persist_lines(&m);
    let mut r = MonitorScaleMap::new();
    let mut dirty = lines.clone();
    dirty.push("垃圾行".into());
    let restored = restore_lines(&dirty, &mut r);
    set.add(
        "persist roundtrip and garbage tolerant",
        restored == 2 && r.get("EDID-A") == ScaleTier::S200 && r.get("EDID-B") == ScaleTier::S125,
        "",
    );

    // 8. 热切换预算常量（一处一事实：<1s）。
    set.add("hot apply budget constant 1000ms", HOT_APPLY_BUDGET_MS == 1000, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factor_table_exact() {
        assert_eq!(ScaleTier::S100.factor(), 100);
        assert_eq!(ScaleTier::S125.factor(), 125);
        assert_eq!(ScaleTier::S150.factor(), 150);
        assert_eq!(ScaleTier::S200.factor(), 200);
    }

    #[test]
    fn scale_math() {
        // 125% 下 16px 逻辑 → 20px 物理。
        assert_eq!(scale_px(16, ScaleTier::S125), 20);
        // 150% 下 33px → 50px（49.5 四舍五入）。
        assert_eq!(scale_px(33, ScaleTier::S150), 50);
        // 回算一致。
        assert_eq!(unscale_px(20, ScaleTier::S125), 16);
    }

    #[test]
    fn monitor_map_overwrite() {
        let mut m = MonitorScaleMap::new();
        m.set("A", ScaleTier::S100);
        m.set("A", ScaleTier::S200);
        assert_eq!(m.len(), 1);
        assert_eq!(m.get("A"), ScaleTier::S200);
    }

    #[test]
    fn dpiscale_selfcheck_all_green() {
        let set = run_dpiscale_checks();
        assert!(set.all_passed(), "F224 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 6 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

/// 持久化版本（格式变更递增；旧版本拒绝读——不猜格式）。
pub const DPISCALE_PERSIST_VERSION: u8 = 1;
/// 显示器持久化槽位上限——判据是「双 DPI 配置」，4 槽覆盖双屏 + 余量
/// （定容纪律：槽位在册，第 5 屏不落盘、capture 如实拒绝）。
pub const MONITOR_PERSIST_CAP: usize = 4;
/// 显示器名（EDID 标识）缓冲上限（字节）。
pub const MONITOR_NAME_CAP: usize = 16;
/// 定长记录 = 4 magic + 1 版本 + 载荷 73（全局档 1 + 4 槽 ×(名 16+名长 1
/// +档码 1)）+ 4 校验 = 82B。
pub const DPISCALE_RECORD_LEN: usize = 5 + 1 + MONITOR_PERSIST_CAP * (MONITOR_NAME_CAP + 2) + 4;
/// v2 记录魔数（AI-H1 二次对账批统一 b"VXH1"）。
const VXH1_MAGIC: [u8; 4] = *b"VXH1";

/// FNV-1a 32 位校验和（与 h2persist fnv1a64 同族异宽，域内自足实现）。
fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 持久化错误枚举：四类损坏输入全拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DpiscalePersistError { BadMagic, BadVersion, BadChecksum, BadLen }

/// 档位 ↔ 码（持久化面的唯一映射点）：0=100 / 1=125 / 2=150 / 3=200。
pub fn tier_to_code(t: ScaleTier) -> u8 {
    match t {
        ScaleTier::S100 => 0,
        ScaleTier::S125 => 1,
        ScaleTier::S150 => 2,
        ScaleTier::S200 => 3,
    }
}

pub fn tier_from_code(c: u8) -> Option<ScaleTier> {
    match c {
        0 => Some(ScaleTier::S100),
        1 => Some(ScaleTier::S125),
        2 => Some(ScaleTier::S150),
        3 => Some(ScaleTier::S200),
        _ => None,
    }
}

/// 档位持久化记录：全局档 + 每显示器档表（空槽名长 0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScalePersist {
    pub global: ScaleTier,
    pub names: [[u8; MONITOR_NAME_CAP]; MONITOR_PERSIST_CAP],
    pub name_lens: [u8; MONITOR_PERSIST_CAP],
    pub tiers: [ScaleTier; MONITOR_PERSIST_CAP],
}

impl ScalePersist {
    /// 从内存档表捕获（同名覆盖语义与 MonitorScaleMap::set 一致；
    /// 超 MONITOR_PERSIST_CAP 槽或名单超容 → None 如实拒绝）。
    pub fn capture(map: &MonitorScaleMap, global: ScaleTier) -> Option<ScalePersist> {
        if map.len() > MONITOR_PERSIST_CAP {
            return None;
        }
        let mut rec = ScalePersist {
            global,
            names: [[0; MONITOR_NAME_CAP]; MONITOR_PERSIST_CAP],
            name_lens: [0; MONITOR_PERSIST_CAP],
            tiers: [ScaleTier::S100; MONITOR_PERSIST_CAP],
        };
        for i in 0..map.len() {
            if let Some((m, t)) = map.scales.get(i) {
                let b = m.as_bytes();
                if b.len() > MONITOR_NAME_CAP {
                    return None;
                }
                rec.names[i][..b.len()].copy_from_slice(b);
                rec.name_lens[i] = b.len() as u8;
                rec.tiers[i] = *t;
            }
        }
        Some(rec)
    }

    /// 编码：[0..4]=magic、[4]=版本、[5]=全局档、4 槽、尾 4B=校验（LE）。
    pub fn to_bytes(&self) -> [u8; DPISCALE_RECORD_LEN] {
        let mut out = [0u8; DPISCALE_RECORD_LEN];
        out[0..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = DPISCALE_PERSIST_VERSION;
        out[5] = tier_to_code(self.global);
        for i in 0..MONITOR_PERSIST_CAP {
            let o = 6 + i * (MONITOR_NAME_CAP + 2);
            out[o..o + MONITOR_NAME_CAP].copy_from_slice(&self.names[i]);
            out[o + MONITOR_NAME_CAP] = self.name_lens[i];
            out[o + MONITOR_NAME_CAP + 1] = tier_to_code(self.tiers[i]);
        }
        let n = DPISCALE_RECORD_LEN;
        let sum = fnv1a32(&out[5..n - 4]);
        out[n - 4..n].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：四类损坏全拒绝 + 档位码在册复核（表外值 = 记录损坏）。
    pub fn from_bytes(b: &[u8]) -> Result<ScalePersist, DpiscalePersistError> {
        if b.len() != DPISCALE_RECORD_LEN {
            return Err(DpiscalePersistError::BadLen);
        }
        if b[0..4] != VXH1_MAGIC {
            return Err(DpiscalePersistError::BadMagic);
        }
        if b[4] != DPISCALE_PERSIST_VERSION {
            return Err(DpiscalePersistError::BadVersion);
        }
        let n = b.len();
        let sum = u32::from_le_bytes([b[n - 4], b[n - 3], b[n - 2], b[n - 1]]);
        if fnv1a32(&b[5..n - 4]) != sum {
            return Err(DpiscalePersistError::BadChecksum);
        }
        let global = match tier_from_code(b[5]) {
            Some(t) => t,
            None => return Err(DpiscalePersistError::BadChecksum),
        };
        let mut rec = ScalePersist {
            global,
            names: [[0; MONITOR_NAME_CAP]; MONITOR_PERSIST_CAP],
            name_lens: [0; MONITOR_PERSIST_CAP],
            tiers: [ScaleTier::S100; MONITOR_PERSIST_CAP],
        };
        for i in 0..MONITOR_PERSIST_CAP {
            let o = 6 + i * (MONITOR_NAME_CAP + 2);
            rec.names[i].copy_from_slice(&b[o..o + MONITOR_NAME_CAP]);
            rec.name_lens[i] = b[o + MONITOR_NAME_CAP];
            rec.tiers[i] = match tier_from_code(b[o + MONITOR_NAME_CAP + 1]) {
                Some(t) => t,
                None => return Err(DpiscalePersistError::BadChecksum),
            };
        }
        Ok(rec)
    }

    /// 恢复进 MonitorScaleMap（逐屏等值；名解码失败的槽如实跳过）。
    pub fn restore_into(&self, map: &mut MonitorScaleMap) -> usize {
        let mut n = 0;
        for i in 0..MONITOR_PERSIST_CAP {
            if self.name_lens[i] == 0 || self.name_lens[i] as usize > MONITOR_NAME_CAP {
                continue;
            }
            if let Ok(name) = core::str::from_utf8(&self.names[i][..self.name_lens[i] as usize]) {
                map.set(name, self.tiers[i]);
                n += 1;
            }
        }
        n
    }
}

// --- v2 UI 壳接线面：窗口矩形跨档换算 + 缩放生效时序判定 ---

/// 窗口矩形按新档换算（左上角锚定，尺寸走「物理→逻辑→新物理」两步，
/// 四舍五入复用 scale_px/unscale_px 口径——跨屏重排的基本操作）。
pub fn rescale_rect(
    r: crate::h1star::h1base::Rect,
    from: ScaleTier,
    to: ScaleTier,
) -> crate::h1star::h1base::Rect {
    let w = scale_px(unscale_px(r.w, from), to);
    let h = scale_px(unscale_px(r.h, from), to);
    crate::h1star::h1base::Rect::new(r.x, r.y, w.max(1), h.max(1))
}

/// 缩放生效时序判定（主册 F224「免重启生效实测（切换到可用 <1s）」）：
/// 切换发起 ts0 → 重排完成 → 重栅格完成，三戳必须不倒序，且总时长
/// 严格 < HOT_APPLY_BUDGET_MS（=1s）。
pub fn apply_sequence_in_budget(ts0: u64, relayout_done: u64, reraster_done: u64) -> bool {
    ts0 <= relayout_done
        && relayout_done <= reraster_done
        && reraster_done.saturating_sub(ts0) < HOT_APPLY_BUDGET_MS as u64
}

// --- v2 判定面扩展 ---

/// F224 v2 自检（首条必为持久化 round-trip）。
pub fn run_dpiscale_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F224-dpiscale-v2");

    // 1. round-trip：双屏档位编码→解码→恢复逐屏等值——验主册 F224
    //    「档位持久化到用户配置」与「双 DPI 各自独立取档」。
    let mut map = MonitorScaleMap::new();
    map.set("EDID-A", ScaleTier::S150);
    map.set("EDID-B", ScaleTier::S200);
    let rt = match ScalePersist::capture(&map, ScaleTier::S125) {
        Some(rec) => match ScalePersist::from_bytes(&rec.to_bytes()) {
            Ok(back) => {
                let mut out = MonitorScaleMap::new();
                back.restore_into(&mut out) == 2
                    && out.get("EDID-A") == ScaleTier::S150
                    && out.get("EDID-B") == ScaleTier::S200
                    && back.global == ScaleTier::S125
            }
            Err(_) => false,
        },
        None => false,
    };
    set.add("v2 persist roundtrip (dual monitor)", rt, "");

    // 2. 四类损坏全拒绝 + 档位码表外拒读——验十二查「损坏输入明错误」。
    let mut rec = ScalePersist {
        global: ScaleTier::S100,
        names: [[0; MONITOR_NAME_CAP]; MONITOR_PERSIST_CAP],
        name_lens: [0; MONITOR_PERSIST_CAP],
        tiers: [ScaleTier::S100; MONITOR_PERSIST_CAP],
    };
    let name = b"EDID-A";
    rec.names[0][..name.len()].copy_from_slice(&name[..]);
    rec.name_lens[0] = name.len() as u8;
    rec.tiers[0] = ScaleTier::S150;
    let bytes = rec.to_bytes();
    let mut m = bytes;
    m[0] = b'X';
    let mut v = bytes;
    v[4] = 9;
    let mut s = bytes;
    s[10] ^= 0xFF;
    let mut c = bytes;
    c[6 + MONITOR_NAME_CAP + 1] = 7;
    let fixed = fnv1a32(&c[5..DPISCALE_RECORD_LEN - 4]);
    c[DPISCALE_RECORD_LEN - 4..DPISCALE_RECORD_LEN].copy_from_slice(&fixed.to_le_bytes());
    set.add(
        "v2 persist rejects 4 corrupt classes + wild tier code",
        ScalePersist::from_bytes(&m) == Err(DpiscalePersistError::BadMagic)
            && ScalePersist::from_bytes(&v) == Err(DpiscalePersistError::BadVersion)
            && ScalePersist::from_bytes(&s) == Err(DpiscalePersistError::BadChecksum)
            && ScalePersist::from_bytes(&c) == Err(DpiscalePersistError::BadChecksum)
            && ScalePersist::from_bytes(&bytes[..bytes.len() - 1]) == Err(DpiscalePersistError::BadLen),
        "",
    );

    // 3. 档位码封闭：四档双向映射、表外拒绝——验主册 F224「四档
    //    100%/125%/150/200%」×100 整数定点的持久化承载。
    set.add(
        "v2 tier codes closed set",
        [ScaleTier::S100, ScaleTier::S125, ScaleTier::S150, ScaleTier::S200]
            .iter()
            .enumerate()
            .all(|(i, t)| tier_to_code(*t) == i as u8)
            && tier_from_code(4).is_none(),
        "",
    );

    // 4. 窗口矩形跨档换算：100→200 尺寸翻倍、200→100 减半、恒非零——
    //    验主册 F224「跨屏拖窗重采样」的几何面。
    let r = crate::h1star::h1base::Rect::new(10, 20, 200, 100);
    let up = rescale_rect(r, ScaleTier::S100, ScaleTier::S200);
    let down = rescale_rect(up, ScaleTier::S200, ScaleTier::S100);
    set.add(
        "v2 rect rescale doubles then halves, size >= 1",
        up.w == 400 && up.h == 200 && down.w == 200 && down.h == 100,
        "",
    );

    // 5. 生效时序判定：<1s 放行、倒序拒、恰 1s 拒——验主册 F224
    //    「免重启生效实测（切换到可用 <1s）」。
    set.add(
        "v2 apply sequence in 1s budget",
        apply_sequence_in_budget(1_000, 1_400, 1_900)
            && !apply_sequence_in_budget(1_000, 1_500, 1_400)
            && !apply_sequence_in_budget(1_000, 1_000, 2_000),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_empty_map_global_only() {
        let rec = ScalePersist::capture(&MonitorScaleMap::new(), ScaleTier::S200).unwrap();
        assert_eq!(rec.global, ScaleTier::S200);
        assert!(rec.name_lens.iter().all(|l| *l == 0));
        let mut out = MonitorScaleMap::new();
        assert_eq!(rec.restore_into(&mut out), 0);
    }

    #[test]
    fn v2_unscale_rescale_chain_125() {
        let up = rescale_rect(crate::h1star::h1base::Rect::new(0, 0, 33, 33), ScaleTier::S100, ScaleTier::S125);
        assert_eq!(up.w, 41);
        let down = rescale_rect(up, ScaleTier::S125, ScaleTier::S100);
        assert_eq!(down.w, 33, "125% 往返不漂移");
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_dpiscale_v2_checks();
        assert!(set.all_passed(), "F224 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
