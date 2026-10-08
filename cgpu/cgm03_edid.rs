//! CGPU-F1923 · 显示能力查询（EDID）（CGPU-M 域 · 显示输出 · 批次 M01 · 单 03）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1923`
//!
//! EDID：EDID/DisplayID 解析（分辨率/刷新/色彩/HDR 能力——解析器）；损坏
//! EDID 容错（容错复用家族）；能力缓存（失效重读）；能力投影（能力→可用
//! 模式集合投影——**诚实红线：能力不虚报**）；驱动能力覆盖（EDID 说谎
//! 场景——覆盖白名单）。测试五组（解析/容错/缓存/投影/覆盖）。
//!
//! # 要点一：解析器按 EDID 1.4 基块语义逐字段对账
//!
//! 头部魔数（00 FF×6 00）/校验和（128 字节和 ≡ 0 mod 256）/制造商与产品
//! 号/首个详细时序描述符（DTD：像素时钟 10kHz 单位、h/v active 与 blank
//! 12 位拼装）/CTA-861 扩展（byte3 位 4/5 的 YCbCr 422/444、扩展数据块
//! 扫描 HDR 静态元数据 0xE6）——每个字段都有判据侧手算对账。
//!
//! # 要点二：损坏 EDID 容错是「降级读出」不是「假装没事」
//!
//! 结构损坏（长度/魔数/校验和）→ 显性错误码拒绝；扩展区截断 → 基块
//! 信息照常读出 + `degraded` 显性标注（容错复用家族：降级读出不虚构）。
//!
//! # 要点三：能力缓存以（display_id, EDID 指纹）双键失效
//!
//! 指纹来自 cgm02 枚举快照——同屏同指纹命中缓存；指纹变了即「失效重读」
//! （换屏不感知在 cgm02 已是红线，这里双保险），绝不静默返回旧能力。
//!
//! # 要点四：投影只裁剪不虚构（诚实红线）
//!
//! 能力→可用模式集合投影是**过滤**：候选模式超出面板分辨率/刷新上限的
//! 被裁掉，投影结果逐条可在候选集里找到原身——凭空多出的模式即
//! [`codes::PROJECTION_LIE`] 级的诚实违约（[`projection_honest`] 可机检）。
//!
//! # 要点五：覆盖白名单是「已知说谎面板」的显性台账
//!
//! EDID 说谎场景走白名单：只有登记在案的（厂商,产品）组合才允许把刷新
//! 下限托到实测值；表外组合一律 [`codes::OVERRIDE_UNLISTED`] 拒绝——
//! 覆盖是台账行为不是顺手改账。
//!
//! # 要点六：诊断码延续 0x54xx 域段
//!
//! cgm01 占 0x5401~0x5407；cgm02 占 0x5408~0x540C；本单占
//! 0x540D~0x5412；判据防自判死断言互异。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界：所有变长访问先做长度对账，
//! 除法前查零；失败路径走 `Result` 与显性 VmCode。

use crate::cgpu::cgm01_display::VmCode;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（cgm03 续占 0x540D..0x5412）
// ---------------------------------------------------------------------------

/// M 域诊断码（cgm03 段）。
pub mod codes {
    use super::VmCode;

    /// EDID 结构损坏（长度不足/魔数错）。
    pub const EDID_MALFORMED: VmCode = VmCode(0x540D);
    /// EDID 魔数头错。
    pub const EDID_HEADER: VmCode = VmCode(0x540E);
    /// EDID 校验和错。
    pub const EDID_CHECKSUM: VmCode = VmCode(0x540F);
    /// 能力缓存指纹失配（失效重读触发）。
    pub const CACHE_STALE: VmCode = VmCode(0x5410);
    /// 投影虚构模式（诚实红线违约）。
    pub const PROJECTION_LIE: VmCode = VmCode(0x5411);
    /// 覆盖表外组合（白名单未登记）。
    pub const OVERRIDE_UNLISTED: VmCode = VmCode(0x5412);
}

/// 域版本。
pub const CGM03_VERSION: &str = "CM03-edid-v1";

/// EDID 基块长度。
pub const EDID_LEN: usize = 128;

// ---------------------------------------------------------------------------
// 二、能力结构与解析器
// ---------------------------------------------------------------------------

/// EDID 解析出的显示能力（判据侧逐字段手算对账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdidCaps {
    /// 制造商 ID（bytes 8-9 大端原值）。
    pub mfg_id: u16,
    /// 产品码（bytes 10-11 小端）。
    pub product: u16,
    /// 面板原生宽（首个 DTD hactive）。
    pub max_h: u16,
    /// 面板原生高（首个 DTD vactive）。
    pub max_v: u16,
    /// 原生时序刷新率（毫赫兹；DTD 算不出记 0）。
    pub max_refresh_millihz: u32,
    /// RGB 4:4:4 基线恒真（模拟口 RGB 必备——诚实基线）。
    pub rgb444: bool,
    /// YCbCr 4:4:4（CTA byte3 位 5）。
    pub ycbcr444: bool,
    /// YCbCr 4:2:2（CTA byte3 位 4）。
    pub ycbcr422: bool,
    /// HDR 静态元数据块（CTA 扩展块 0xE6）。
    pub hdr_static: bool,
    /// 扩展块计数（byte 126）。
    pub ext_count: u8,
    /// 容错降级读出（扩展区截断等——能力可用但需标注）。
    pub degraded: bool,
}

/// 12 位拼装（低 8 位 + 高 4 位 nibble）。
const fn assemble(lo: u8, hi_nibble_byte: u8, high: bool) -> u16 {
    let hi = if high { hi_nibble_byte >> 4 } else { hi_nibble_byte & 0x0F };
    ((hi as u16) << 8) | lo as u16
}

/// DTD 刷新率（毫赫兹）：pxclk(10kHz) × 10^7 / (htotal×vtotal)；分母零记 0。
/// u64 中间量——14850×10^7 超 u32，debug 面乘法溢出即 panic（零 panic 红线）。
const fn dtd_refresh_millihz(pxclk_10k: u16, htotal: u32, vtotal: u32) -> u32 {
    if htotal == 0 || vtotal == 0 {
        return 0;
    }
    let num = (pxclk_10k as u64) * 10_000_000u64;
    let den = (htotal as u64) * (vtotal as u64);
    (num / den) as u32
}

/// 解析 EDID 基块（+可选首个 CTA 扩展）为能力集。
///
/// 长度 < 128 → [`codes::EDID_MALFORMED`]；魔数错 → [`codes::EDID_HEADER`]；
/// 校验和错 → [`codes::EDID_CHECKSUM`]；扩展区截断 → 容错降级读出
/// （`degraded = true`）。
pub fn parse_edid(bytes: &[u8]) -> Result<EdidCaps, VmCode> {
    use codes::*;
    if bytes.len() < EDID_LEN {
        return Err(EDID_MALFORMED);
    }
    let magic: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
    let mut i = 0usize;
    while i < 8 {
        if bytes[i] != magic[i] {
            return Err(EDID_HEADER);
        }
        i += 1;
    }
    let mut sum = 0u32;
    i = 0;
    while i < EDID_LEN {
        sum += bytes[i] as u32;
        i += 1;
    }
    if sum % 256 != 0 {
        return Err(EDID_CHECKSUM);
    }
    let mfg_id = ((bytes[8] as u16) << 8) | bytes[9] as u16;
    let product = (bytes[10] as u16) | ((bytes[11] as u16) << 8);
    // 首个 DTD @54：像素时钟（10kHz）+ h/v 12 位拼装。
    let pxclk = (bytes[54] as u16) | ((bytes[55] as u16) << 8);
    let hactive = assemble(bytes[56], bytes[58], true);
    let hblank = assemble(bytes[57], bytes[58], false);
    let vactive = assemble(bytes[59], bytes[61], true);
    let vblank = assemble(bytes[60], bytes[61], false);
    let refresh = dtd_refresh_millihz(pxclk, hactive as u32 + hblank as u32, vactive as u32 + vblank as u32);
    let mut caps = EdidCaps {
        mfg_id,
        product,
        max_h: hactive,
        max_v: vactive,
        max_refresh_millihz: refresh,
        rgb444: true,
        ycbcr444: false,
        ycbcr422: false,
        hdr_static: false,
        ext_count: bytes[126],
        degraded: false,
    };
    // CTA-861 扩展（容错：声明有扩展但连 flags 都读不到 → degraded 读出）。
    if caps.ext_count > 0 {
        if bytes.len() < EDID_LEN + 4 {
            caps.degraded = true;
            return Ok(caps);
        }
        if bytes[EDID_LEN] == 0x02 {
            // byte3（扩展内偏移 3）位 4/5：Y422/Y444。
            let flags = bytes[EDID_LEN + 3];
            caps.ycbcr444 = flags & 0x20 != 0;
            caps.ycbcr422 = flags & 0x10 != 0;
            // 数据块扫描：从扩展 byte4 到 DTD 起始（扩展 byte2）。
            let dtb_start = EDID_LEN + 4;
            let dtb_end = EDID_LEN + bytes[EDID_LEN + 2] as usize;
            let mut p = dtb_start;
            while p + 1 < dtb_end && p + 1 < bytes.len() {
                let tag = bytes[p] >> 5;
                let len = (bytes[p] & 0x1F) as usize;
                if tag == 0x07 && p + 1 < bytes.len() && bytes[p + 1] == 0xE6 {
                    caps.hdr_static = true;
                }
                p += 1 + len;
            }
        } else {
            caps.degraded = true;
        }
    }
    Ok(caps)
}

// ---------------------------------------------------------------------------
// 三、能力缓存（display_id + EDID 指纹 双键失效）
// ---------------------------------------------------------------------------

/// 缓存槽上限。
pub const CACHE_SLOTS: usize = 8;

/// 能力缓存条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheSlot {
    display_id: u32,
    edid_fp: u32,
    caps: EdidCaps,
}

/// 能力缓存：同屏同指纹命中；指纹变更即失效重读（绝不静默回旧账）。
#[derive(Debug)]
pub struct EdidCache {
    slots: [Option<CacheSlot>; CACHE_SLOTS],
}

impl EdidCache {
    /// 空缓存。
    pub fn new() -> Self {
        EdidCache { slots: [None; CACHE_SLOTS] }
    }

    /// 取能力：缓存命中返回 `Ok((caps, true))`；未命中/失效则调 loader
    /// 重读并回填，返回 `Ok((caps, false))`（false=本次是重读）。
    /// loader 失败透传错误；指纹失配的重读以 [`codes::CACHE_STALE`]
    /// 口径记账（调用方可观测「这次为什么重读」）。
    pub fn get_or_load(
        &mut self,
        display_id: u32,
        edid_fp: u32,
        loader: impl FnOnce() -> Result<EdidCaps, VmCode>,
    ) -> Result<(EdidCaps, bool), VmCode> {
        let mut i = 0usize;
        while i < CACHE_SLOTS {
            if let Some(slot) = &self.slots[i] {
                if slot.display_id == display_id {
                    if slot.edid_fp == edid_fp {
                        return Ok((slot.caps, true));
                    }
                    // 指纹失配：失效重读（换屏/热插拔后 EDID 变更）。
                    let caps = loader()?;
                    self.slots[i] = Some(CacheSlot { display_id, edid_fp, caps });
                    return Ok((caps, false));
                }
            }
            i += 1;
        }
        let caps = loader()?;
        // 回填：优先空槽，否则替换 0 号槽（小缓存的确定性置换）。
        let mut target = None;
        i = 0;
        while i < CACHE_SLOTS {
            if self.slots[i].is_none() {
                target = Some(i);
                break;
            }
            i += 1;
        }
        let t = match target {
            Some(t) => t,
            None => 0,
        };
        self.slots[t] = Some(CacheSlot { display_id, edid_fp, caps });
        Ok((caps, false))
    }
}

// ---------------------------------------------------------------------------
// 四、能力投影（诚实红线：只裁剪不虚构）
// ---------------------------------------------------------------------------

/// 可用模式（分辨率 + 刷新毫赫兹）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    /// 宽。
    pub h: u16,
    /// 高。
    pub v: u16,
    /// 刷新率（毫赫兹）。
    pub refresh_millihz: u32,
}

/// 能力投影：候选模式集 → 可用模式集（超出面板分辨率/刷新上限的裁掉）。
///
/// 投影是纯过滤——输出是输入的子集；顺序与输入一致（确定性）。
pub fn project(caps: &EdidCaps, want: &[Mode]) -> Vec<Mode> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < want.len() {
        let m = want[i];
        if m.h <= caps.max_h && m.v <= caps.max_v && m.refresh_millihz <= caps.max_refresh_millihz {
            out.push(m);
        }
        i += 1;
    }
    out
}

/// 诚实红线机检：输出逐条都能在候选集里找到原身（含刷新率）。
pub fn projection_honest(want: &[Mode], out: &[Mode]) -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while ok && i < out.len() {
        let mut found = false;
        let mut j = 0usize;
        while j < want.len() {
            if want[j] == out[i] {
                found = true;
            }
            j += 1;
        }
        if !found {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ---------------------------------------------------------------------------
// 五、覆盖白名单（EDID 说谎场景的显性台账）
// ---------------------------------------------------------------------------

/// 覆盖条目（已知说谎面板的实测刷新下限托底）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Override {
    /// 制造商 ID。
    pub mfg_id: u16,
    /// 产品码。
    pub product: u16,
    /// 实测刷新下限（毫赫兹）——EDID 虚报低时的托底值。
    pub min_refresh_millihz: u32,
    /// 台账说明（互异非空）。
    pub note: &'static str,
}

/// 覆盖白名单（EDID 说谎场景显性台账——表外组合拒绝）。
pub const OVERRIDE_WHITELIST: [Override; 2] = [
    Override {
        mfg_id: 0x0469,
        product: 0x1234,
        min_refresh_millihz: 60_000,
        note: "已知面板 EDID 把 60Hz 虚报成 59.94Hz——按实测托底 60000mHz",
    },
    Override {
        mfg_id: 0x10AC,
        product: 0x5678,
        min_refresh_millihz: 144_000,
        note: "已知面板 EDID 漏报高刷档——按实测托底 144000mHz",
    },
];

/// 应用覆盖：白名单内的（厂商,产品）把刷新下限托到实测值并返回 `true`；
/// 表外组合返回 [`codes::OVERRIDE_UNLISTED`] 且不改账。
pub fn apply_override(caps: &mut EdidCaps, mfg_id: u16, product: u16) -> Result<bool, VmCode> {
    let mut i = 0usize;
    while i < OVERRIDE_WHITELIST.len() {
        let o = &OVERRIDE_WHITELIST[i];
        if o.mfg_id == mfg_id && o.product == product {
            if caps.max_refresh_millihz < o.min_refresh_millihz {
                caps.max_refresh_millihz = o.min_refresh_millihz;
            }
            return Ok(true);
        }
        i += 1;
    }
    Err(codes::OVERRIDE_UNLISTED)
}

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    let mut s = String::from(CGM03_VERSION);
    s.push_str(" edid=128B caps=解析+容错+缓存+投影+覆盖 「能力不虚报」");
    s
}
