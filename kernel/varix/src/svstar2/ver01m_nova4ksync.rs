//! VE-F3413 · 与 nova4k 的同步协议 —— 导出/导入双向 + 映射审计 + 4K 精度校验 + 版本兼容矩阵。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3413`
//!
//! # 职责（锚点原文拆解）
//!
//! - **双向同步**：令牌基线 → nova4k 导出（含 4K 精度闸与映射登记）、
//!   nova4k → 基线导入（565 反量化 + 映射存在性核查）；往返一遍即
//!   比对，失真如实标注不静默；
//! - **映射审计**：每条 令牌路径 ↔ nova4k 资产号 的映射入账，一号
//!   多路径即拒（审计账一对一，坏账比慢同步致命得多）；
//! - **4K 精度校验**：令牌产出的素材宽高须达 4K（3840×2160）——
//!   高分屏上糊是素材像素不够，不是缩放算法的锅，源头驳回；
//! - **版本兼容矩阵**：nova4k 工具版本 × 令牌格式版本二维**实测表**
//!   （Compatible/Degraded/Incompatible 三态），矩阵外组合一律
//!   漂移拦截——没实测过的组合不许放行（放行=替工具方撒谎）；
//! - **降级矩阵**：往返失真→标注（`RoundTripLoss` 非阻断）；精度
//!   不达标→驳回（`PrecisionLow` 阻断）；漂移→版本拦截
//!   （`ToolDrift`/`FormatDrift` 阻断）；
//! - **跨批对接点 E07 开放格式**：联接触手（方言闭集），越界即拒；
//! - **读屏可达**：同步状态逐项播报。
//!
//! # 为什么往返失真是标注而不是驳回
//!
//! nova4k 侧是 565 量化格式，深色过渡带必然丢低位——这是格式天花板，
//!   不是同步器错误。驳回会让「可用的 99%」整批不可用；标注则让
//!   失真在账面上看得见（哪个令牌、偏了多少），下游按标注决定要不要
//!   走无损旁路。**可以接受的损失必须留痕，不能接受的精度另有驳回闸。**
//!
//! # 为什么漂移要拦而不是猜
//!
//! 兼容矩阵是实测出来的，没测过的组合可能是好的也可能是崩的——
//!   猜「Compat」等于替没测过的路径背书。拦截 + 明确报错，让调用方
//!   要么降级到已实测组合，要么去补测。
//!
//! # 零 panic 面
//!
//! `[i]` / `unwrap()` / `expect()` 只出现在 `#[cfg(test)]`；
//! 判据区一律 match 记红。

use crate::svstar2::ver01k_dualtheme::RGB888;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（自建；E15 段独占，与 E14/F3412、E13/F3411 等既有段零重叠）
// ---------------------------------------------------------------------------

/// nova4k 同步域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncCode {
    /// 令牌路径非法（空或超长）。
    TokenEmpty,
    /// 工具版本漂移（不在实测矩阵，拦截）。
    ToolDrift,
    /// 格式版本漂移（不在实测矩阵或实测不兼容，拦截）。
    FormatDrift,
    /// 4K 精度不达标（素材宽高不足，驳回）。
    PrecisionLow,
    /// 往返失真（565 量化损失，标注非阻断）。
    RoundTripLoss,
    /// 映射审计冲突（一号多路径或导入未登记）。
    MapConflict,
    /// E07 开放格式联动方言非法。
    LinkageState,
}

impl SyncCode {
    /// 全部码（判据据此核对无遗漏）。
    pub const ALL: [SyncCode; 7] = [
        SyncCode::TokenEmpty,
        SyncCode::ToolDrift,
        SyncCode::FormatDrift,
        SyncCode::PrecisionLow,
        SyncCode::RoundTripLoss,
        SyncCode::MapConflict,
        SyncCode::LinkageState,
    ];

    /// 线上短码（E15 段独占）。
    pub const fn code(self) -> &'static str {
        match self {
            SyncCode::TokenEmpty => "E15-TOKEN-EMPTY",
            SyncCode::ToolDrift => "E15-TOOL-DRIFT",
            SyncCode::FormatDrift => "E15-FORMAT-DRIFT",
            SyncCode::PrecisionLow => "E15-PRECISION-LOW",
            SyncCode::RoundTripLoss => "E15-ROUNDTRIP-LOSS",
            SyncCode::MapConflict => "E15-MAP-CONFLICT",
            SyncCode::LinkageState => "E15-LINKAGE-STATE",
        }
    }

    /// 是否阻断。
    pub const fn blocking(self) -> bool {
        !matches!(self, SyncCode::RoundTripLoss)
    }

    /// 是否事件类（失真标注记账）。
    pub const fn eventful(self) -> bool {
        matches!(self, SyncCode::RoundTripLoss)
    }

    /// 读屏可达句子。
    pub fn spoken(self) -> String {
        let s = match self {
            SyncCode::TokenEmpty => "令牌路径非法。",
            SyncCode::ToolDrift => "工具版本不在实测矩阵，已拦截。",
            SyncCode::FormatDrift => "令牌格式版本未实测或不兼容，已拦截。",
            SyncCode::PrecisionLow => "素材未达 4K 精度，已驳回。",
            SyncCode::RoundTripLoss => "往返失真已标注。",
            SyncCode::MapConflict => "映射审计冲突，已拒绝。",
            SyncCode::LinkageState => "开放格式联动方言非法。",
        };
        format!("{}{}", s, self.code())
    }
}

// ---------------------------------------------------------------------------
// 二、契约常量与版本
// ---------------------------------------------------------------------------

/// 契约版本（冻结）。
pub const SYNC_CONTRACT: &str = "E15-nova4k-sync-v1";
/// 4K 宽度下限（px，UHD）。
pub const FOUR_K_W: u32 = 3840;
/// 4K 高度下限（px，UHD）。
pub const FOUR_K_H: u32 = 2160;
/// 令牌路径字节上限。
pub const PATH_MAX: usize = 128;
/// E07 方言闭集大小。
pub const E07_DIALECTS: usize = 3;

fn validate_path(p: &str) -> Result<(), SyncCode> {
    if p.is_empty() || p.len() > PATH_MAX {
        return Err(SyncCode::TokenEmpty);
    }
    Ok(())
}

/// 版本号（主.次；wire 比对此对）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ver {
    pub major: u16,
    pub minor: u16,
}

impl Ver {
    pub const fn new(major: u16, minor: u16) -> Ver {
        Ver { major, minor }
    }

    /// 版本是否相等（同主同次）。
    pub const fn eq(self, o: Ver) -> bool {
        self.major == o.major && self.minor == o.minor
    }
}

/// 兼容裁决三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compat {
    /// 实测兼容。
    Compatible,
    /// 降级可用（有限制，调用方知情）。
    Degraded,
    /// 实测不兼容。
    Incompatible,
}

// ---------------------------------------------------------------------------
// 三、版本兼容矩阵（二维实测表；矩阵外 = 漂移）
// ---------------------------------------------------------------------------

/// 实测矩阵条目（冻结单源：工具版本 × 格式版本 → 裁决）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompatEntry {
    pub tool: Ver,
    pub fmt: Ver,
    pub verdict: Compat,
}

/// 实测矩阵（四格全测：2 工具 × 2 格式）。
pub const COMPAT_MATRIX: [CompatEntry; 4] = [
    CompatEntry {
        tool: Ver::new(4, 0),
        fmt: Ver::new(2, 0),
        verdict: Compat::Compatible,
    },
    CompatEntry {
        tool: Ver::new(4, 0),
        fmt: Ver::new(2, 1),
        verdict: Compat::Degraded,
    },
    CompatEntry {
        tool: Ver::new(4, 1),
        fmt: Ver::new(2, 0),
        verdict: Compat::Compatible,
    },
    CompatEntry {
        tool: Ver::new(4, 1),
        fmt: Ver::new(2, 1),
        verdict: Compat::Compatible,
    },
];

/// 实测工具版本闭集。
pub const KNOWN_TOOLS: [Ver; 2] = [Ver::new(4, 0), Ver::new(4, 1)];
/// 实测格式版本闭集。
pub const KNOWN_FMTS: [Ver; 2] = [Ver::new(2, 0), Ver::new(2, 1)];

/// 矩阵查表（None = 该组合未实测 = 漂移不许猜）。
pub fn lookup(tool: Ver, fmt: Ver) -> Option<Compat> {
    for e in COMPAT_MATRIX.iter() {
        if e.tool.eq(tool) && e.fmt.eq(fmt) {
            return Some(e.verdict);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 四、素材精度与 565 量化
// ---------------------------------------------------------------------------

/// 同步素材（令牌产出的高清素材）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyncAsset {
    /// 像素宽。
    pub px_w: u32,
    /// 像素高。
    pub px_h: u32,
}

impl SyncAsset {
    /// 恰 4K 素材（3840×2160，恰边界放行用）。
    pub const fn uhd() -> SyncAsset {
        SyncAsset {
            px_w: FOUR_K_W,
            px_h: FOUR_K_H,
        }
    }

    /// 是否达 4K（恰等于即达标：4K 是下界不是目标）。
    pub const fn is_4k(self) -> bool {
        self.px_w >= FOUR_K_W && self.px_h >= FOUR_K_H
    }
}

/// nova4k 侧 565 打包（导出：RGB888 → 5/6/5）。
pub const fn pack565(c: RGB888) -> u16 {
    let r5 = (c.r >> 3) as u16;
    let g6 = (c.g >> 2) as u16;
    let b5 = (c.b >> 3) as u16;
    (r5 << 11) | (g6 << 5) | b5
}

/// nova4k 侧 565 反量化（导入：5/6/5 → RGB888，低位扩填）。
pub const fn unpack565(p: u16) -> RGB888 {
    let r5 = (p >> 11) & 0x1F;
    let g6 = (p >> 5) & 0x3F;
    let b5 = p & 0x1F;
    let r = ((r5 << 3) | (r5 >> 2)) as u8;
    let g = ((g6 << 2) | (g6 >> 4)) as u8;
    let b = ((b5 << 3) | (b5 >> 2)) as u8;
    RGB888::new(r, g, b)
}

// ---------------------------------------------------------------------------
// 五、同步协议（双向 + 审计账）
// ---------------------------------------------------------------------------

/// 映射审计账条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MapEntry {
    /// 令牌路径。
    pub path: String,
    /// nova4k 资产号（一对一）。
    pub nova_id: u32,
}

/// nova4k 同步协议。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SyncProtocol {
    /// 协议侧工具版本（nova4k 版本）。
    pub tool: Ver,
    /// 协议侧令牌格式版本。
    pub fmt: Ver,
    /// 映射审计账（路径 ↔ 资产号一对一）。
    pub map: Vec<MapEntry>,
    /// 失真标注账（往返有损的令牌路径）。
    pub losses: Vec<String>,
    /// 导出计数。
    pub exported: usize,
    /// 导入计数。
    pub imported: usize,
}

impl SyncProtocol {
    /// 新建协议（版本对随建；实际闸在每次同步时查矩阵）。
    pub fn new(tool: Ver, fmt: Ver) -> SyncProtocol {
        SyncProtocol {
            tool,
            fmt,
            map: Vec::new(),
            losses: Vec::new(),
            exported: 0,
            imported: 0,
        }
    }

    /// 版本闸：工具未知 → ToolDrift；格式未知或实测不兼容 → FormatDrift。
    pub fn check_versions(&self) -> Result<Compat, SyncCode> {
        let mut tool_known = false;
        for t in KNOWN_TOOLS.iter() {
            if *t == self.tool {
                tool_known = true;
            }
        }
        if !tool_known {
            return Err(SyncCode::ToolDrift);
        }
        let mut fmt_known = false;
        for f in KNOWN_FMTS.iter() {
            if *f == self.fmt {
                fmt_known = true;
            }
        }
        if !fmt_known {
            return Err(SyncCode::FormatDrift);
        }
        match lookup(self.tool, self.fmt) {
            Some(Compat::Incompatible) => Err(SyncCode::FormatDrift),
            Some(c) => Ok(c),
            None => Err(SyncCode::FormatDrift),
        }
    }

    /// 登记映射（一号多路径即拒；同路径改号是同路径更新）。
    fn register_map(&mut self, path: &str, nova_id: u32) -> Result<(), SyncCode> {
        for m in self.map.iter() {
            if m.nova_id == nova_id && m.path != path {
                return Err(SyncCode::MapConflict);
            }
        }
        for m in self.map.iter_mut() {
            if m.path == path {
                m.nova_id = nova_id;
                return Ok(());
            }
        }
        self.map.push(MapEntry {
            path: String::from(path),
            nova_id,
        });
        Ok(())
    }

    /// 导出（基线 → nova4k）：版本闸 + 4K 精度闸 + 映射登记 + 565 打包。
    ///
    /// - 版本漂移/不兼容 → 拦截（`ToolDrift`/`FormatDrift`）；
    /// - 素材未达 4K → 驳回（`PrecisionLow`，精度是源头问题）；
    /// - 资产号被别的路径占用 → `MapConflict`；
    /// - Degraded 裁决放行（降级可用，调用方已从矩阵知情）。
    pub fn export(
        &mut self,
        path: &str,
        value: RGB888,
        asset: SyncAsset,
        nova_id: u32,
    ) -> Result<u16, SyncCode> {
        validate_path(path)?;
        self.check_versions()?;
        if !asset.is_4k() {
            return Err(SyncCode::PrecisionLow);
        }
        self.register_map(path, nova_id)?;
        self.exported += 1;
        Ok(pack565(value))
    }

    /// 导入（nova4k → 基线）：映射存在性核查 + 565 反量化。
    ///
    /// 未登记映射的导入一律拒（审计账外数据不可信）。
    pub fn import(&mut self, path: &str, packed: u16) -> Result<RGB888, SyncCode> {
        validate_path(path)?;
        if !self.map.iter().any(|m| m.path == path) {
            return Err(SyncCode::MapConflict);
        }
        self.imported += 1;
        Ok(unpack565(packed))
    }

    /// 往返（导出即导回）：失真如实标注（`RoundTripLoss` 非阻断）。
    pub fn round_trip(
        &mut self,
        path: &str,
        value: RGB888,
        asset: SyncAsset,
        nova_id: u32,
    ) -> Result<(RGB888, Option<SyncCode>), SyncCode> {
        let packed = self.export(path, value, asset, nova_id)?;
        let back = self.import(path, packed)?;
        let note = if back == value {
            None
        } else {
            if !self.losses.iter().any(|l| l == path) {
                self.losses.push(String::from(path));
            }
            Some(SyncCode::RoundTripLoss)
        };
        Ok((back, note))
    }

    /// 映射数（O(令牌对) 扫描口径，账长度即条数）。
    pub fn map_count(&self) -> usize {
        self.map.len()
    }

    /// 审计自洽：一号不二路径（正反两向各查一次）。
    pub fn audit_ok(&self) -> bool {
        for i in 0..self.map.len() {
            for j in (i + 1)..self.map.len() {
                if self.map[i].nova_id == self.map[j].nova_id {
                    return false;
                }
                if self.map[i].path == self.map[j].path {
                    return false;
                }
            }
        }
        true
    }

    /// 查某路径的资产号。
    pub fn nova_id_of(&self, path: &str) -> Option<u32> {
        self.map.iter().find(|m| m.path == path).map(|m| m.nova_id)
    }
}

// ---------------------------------------------------------------------------
// 六、E07 开放格式联动 + 读屏
// ---------------------------------------------------------------------------

/// E07 开放格式联接触手（跨批对接点：方言闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpenFormatLinkage {
    /// 联动是否启用。
    pub enabled: bool,
    /// E07 方言号（0..=2 闭集）。
    pub dialect: u8,
}

impl OpenFormatLinkage {
    /// 默认：关闭（方言 0）。
    pub const fn off() -> OpenFormatLinkage {
        OpenFormatLinkage {
            enabled: false,
            dialect: 0,
        }
    }

    /// 校验联动状态（方言越界即拒）。
    pub fn validate(&self) -> Result<(), SyncCode> {
        if self.dialect as usize >= E07_DIALECTS {
            return Err(SyncCode::LinkageState);
        }
        Ok(())
    }
}

/// 同步状态读屏播报：版本裁决/映射数/失真数/导入口径。
pub fn sync_spoken(p: &SyncProtocol) -> String {
    let verdict = match p.check_versions() {
        Ok(Compat::Compatible) => "兼容",
        Ok(Compat::Degraded) => "降级可用",
        Ok(Compat::Incompatible) => "不兼容",
        Err(_) => "版本拦截",
    };
    format!(
        "nova4k 同步：工具 {}.{}，格式 {}.{}，裁决{}；映射{}条，失真标注{}项，导出{}次导入{}次；{}",
        p.tool.major,
        p.tool.minor,
        p.fmt.major,
        p.fmt.minor,
        verdict,
        p.map_count(),
        p.losses.len(),
        p.exported,
        p.imported,
        SYNC_CONTRACT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: RGB888 = RGB888::new(255, 255, 255);
    const MIX: RGB888 = RGB888::new(200, 150, 100);

    #[test]
    fn roundtrip_exact_vs_lossy() {
        let mut p = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
        // 白值往返无损。
        let (back, note) = p.round_trip("c.fg", WHITE, SyncAsset::uhd(), 1).unwrap();
        assert_eq!(back, WHITE);
        assert_eq!(note, None);
        // 混色往返有损 → 标注不阻断。
        let (back2, note2) = p.round_trip("c.mid", MIX, SyncAsset::uhd(), 2).unwrap();
        assert_ne!(back2, MIX);
        assert_eq!(note2, Some(SyncCode::RoundTripLoss));
        assert_eq!(p.losses.len(), 1);
    }

    #[test]
    fn gates() {
        let mut p = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
        // 精度驳回：差一像素即驳。
        assert_eq!(
            p.export("c.p", WHITE, SyncAsset { px_w: 3839, px_h: 2160 }, 1),
            Err(SyncCode::PrecisionLow)
        );
        // 恰 4K 放行。
        assert!(p.export("c.p", WHITE, SyncAsset::uhd(), 1).is_ok());
        // 一号二路径拒。
        assert_eq!(
            p.export("c.q", WHITE, SyncAsset::uhd(), 1),
            Err(SyncCode::MapConflict)
        );
        // 同路径改号放行（更新）。
        assert!(p.export("c.q", WHITE, SyncAsset::uhd(), 2).is_ok());
        assert_eq!(p.nova_id_of("c.q"), Some(2));
        assert!(p.audit_ok());
        // 未登记导入拒。
        assert_eq!(p.import("c.z", 0xFFFF), Err(SyncCode::MapConflict));
    }

    #[test]
    fn version_drift() {
        let bad_tool = SyncProtocol::new(Ver::new(3, 9), Ver::new(2, 1));
        assert_eq!(bad_tool.check_versions(), Err(SyncCode::ToolDrift));
        let bad_fmt = SyncProtocol::new(Ver::new(4, 1), Ver::new(1, 0));
        assert_eq!(bad_fmt.check_versions(), Err(SyncCode::FormatDrift));
        let degraded = SyncProtocol::new(Ver::new(4, 0), Ver::new(2, 1));
        assert_eq!(degraded.check_versions(), Ok(Compat::Degraded));
        let ok = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 0));
        assert_eq!(ok.check_versions(), Ok(Compat::Compatible));
    }
}
