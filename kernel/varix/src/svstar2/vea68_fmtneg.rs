//! VE-F0068 · 呈现格式协商（VE-A 域 · A03b 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0068`
//!
//! **职责定位（锚点原文）**：surface 支持格式列表与交换链能力求交集，按
//! 优先序选择（SRGB 视觉优先、线性备选），色彩空间语义与呈现管线全程同步
//! （格式选了 SRGB 就全程 SRGB 语义，不静默换）。
//!
//! **数据结构（锚点原文）**：格式交集表；优先序列表；色彩空间标记。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 交集为空 → 显性失败并报告两侧支持集（不静默挑一个凑合）；
//! - 色彩空间不一致 → 拒绝呈现不静默转换；
//! - 查询失败 → 保守默认格式标注（默认值必须带「默认」标注）。
//!
//! **性能逐项分解（锚点原文）**：交集 O(格式数) 协商期一次性；运行时 O(1)；
//! 零转换成本（语义同步设计）。
//!
//! **跨批对接点（锚点原文）**：上游 F0067；下游 F0075 一致性测试覆盖格式
//! 协商矩阵。
//!
//! **无障碍与隐私（锚点原文）**：无直接无障碍面。
//!
//! **判据（锚点原文）**：交集协商、SRGB 优先、语义同步、显性失败、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE89E 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const FMTNEG_VERSION: &str = "vea68_fmtneg-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0xE89E;

// ---- 诊断码 ----
pub const XC_EMPTY: u32 = SEG_BASE + 1; // 交集为空（显性失败）
pub const XC_CS_MISMATCH: u32 = SEG_BASE + 2; // 色彩空间不一致（拒绝呈现）
pub const XC_QUERY_FAIL: u32 = SEG_BASE + 3; // 查询失败（保守默认标注）

/// 像素格式（呈现协商用的稳定子集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFmt {
    Bgra8Srgb,
    Bgra8Unorm,
    Rgba8Srgb,
    Rgba8Unorm,
    Rgba16Float,
}

impl PixelFmt {
    /// 色彩空间语义：SRGB 格式即 SRGB 语义（格式即语义，不静默换）。
    pub fn is_srgb(self) -> bool {
        matches!(self, PixelFmt::Bgra8Srgb | PixelFmt::Rgba8Srgb)
    }
    /// 稳定编号（交集表与审计留痕用）。
    pub fn id(self) -> u8 {
        match self {
            PixelFmt::Bgra8Srgb => 1,
            PixelFmt::Bgra8Unorm => 2,
            PixelFmt::Rgba8Srgb => 3,
            PixelFmt::Rgba8Unorm => 4,
            PixelFmt::Rgba16Float => 5,
        }
    }
}

/// 色彩空间标记（与格式语义绑定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    Linear,
}

impl ColorSpace {
    /// 与格式语义是否一致（语义同步判定的机器形式）。
    pub fn matches(self, f: PixelFmt) -> bool {
        match self {
            ColorSpace::Srgb => f.is_srgb(),
            ColorSpace::Linear => !f.is_srgb(),
        }
    }
}

/// 格式协商器。
pub struct FmtNegotiator {
    /// surface 支持格式表（来自查询）。
    surface_support: Vec<PixelFmt>,
    /// 交换链能力格式表。
    chain_support: Vec<PixelFmt>,
    /// 优先序列表（SRGB 视觉优先、线性备选——锚点原文的机器形式）。
    priority: Vec<PixelFmt>,
    last_code: u32,
    /// 查询失败标记（保守默认路径的依据）。
    query_failed: bool,
    /// 协商结果留痕（选定格式 × 色彩空间）。
    chosen: Option<(PixelFmt, ColorSpace)>,
}

impl FmtNegotiator {
    /// 常规构造（查询成功路径）。
    pub fn new(surface: Vec<PixelFmt>, chain: Vec<PixelFmt>) -> FmtNegotiator {
        // 优先序：全部 SRGB 格式在前（表内稳定序），线性在后，HDR 最后。
        let mut priority: Vec<PixelFmt> = Vec::new();
        for f in [PixelFmt::Bgra8Srgb, PixelFmt::Rgba8Srgb].iter() {
            priority.push(*f);
        }
        for f in [PixelFmt::Bgra8Unorm, PixelFmt::Rgba8Unorm, PixelFmt::Rgba16Float].iter() {
            priority.push(*f);
        }
        FmtNegotiator {
            surface_support: surface,
            chain_support: chain,
            priority,
            last_code: 0,
            query_failed: false,
            chosen: None,
        }
    }

    /// 查询失败降级构造：保守默认格式（Bgra8Srgb）并标注「默认」。
    pub fn fallback() -> FmtNegotiator {
        let mut n = FmtNegotiator::new(
            Vec::new(),
            Vec::new(),
        );
        n.surface_support.push(PixelFmt::Bgra8Srgb);
        n.chain_support.push(PixelFmt::Bgra8Srgb);
        n.query_failed = true;
        n.last_code = XC_QUERY_FAIL;
        n
    }

    /// 交集协商：按优先序找第一个「两侧都支持」的格式。
    /// 交集为空 → 显性失败（错误携带两侧支持集）。
    pub fn negotiate(&mut self) -> Result<(PixelFmt, ColorSpace), u32> {
        for want in self.priority.iter() {
            let s_ok = self.surface_support.iter().any(|f| f.id() == want.id());
            let c_ok = self.chain_support.iter().any(|f| f.id() == want.id());
            if s_ok && c_ok {
                let cs = if want.is_srgb() { ColorSpace::Srgb } else { ColorSpace::Linear };
                self.chosen = Some((*want, cs));
                return Ok((*want, cs));
            }
        }
        self.last_code = XC_EMPTY;
        Err(XC_EMPTY)
    }

    /// 交集为空时的两侧支持集报告（显性失败——锚点原文）。
    pub fn empty_report(&self) -> String {
        let side = |v: &Vec<PixelFmt>| -> String {
            if v.is_empty() {
                String::from("（空）")
            } else {
                let mut s = String::new();
                for (i, f) in v.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    s.push_str(&alloc::format!("{}", f.id()));
                }
                s
            }
        };
        alloc::format!("surface 支持[{}] 交换链支持[{}]", side(&self.surface_support), side(&self.chain_support))
    }

    /// 语义同步校验：呈现管线的色彩空间标记必须与选定格式的语义一致，
    /// 不一致 → 拒绝呈现（不静默转换——锚点原文铁律）。
    pub fn verify_pipeline(&mut self, fmt: PixelFmt, cs: ColorSpace) -> Result<(), u32> {
        if cs.matches(fmt) {
            Ok(())
        } else {
            self.last_code = XC_CS_MISMATCH;
            Err(XC_CS_MISMATCH)
        }
    }

    pub fn query_failed(&self) -> bool {
        self.query_failed
    }
    pub fn chosen(&self) -> Option<(PixelFmt, ColorSpace)> {
        self.chosen
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let src = if self.query_failed { "保守默认" } else { "协商" };
        match self.chosen {
            Some((f, cs)) => {
                let c = if cs == ColorSpace::Srgb { "SRGB" } else { "线性" };
                alloc::format!("FMT {}（{}）格式id{} 色彩空间{}", src, if self.query_failed { "默认" } else { "选定" }, f.id(), c)
            }
            None => alloc::format!("FMT {}未完成 交集{}", src, if self.last_code == XC_EMPTY { "空" } else { "未求" }),
        }
    }
}

// =====================================================================
// 自检（判据锚点：交集协商 / SRGB 优先 / 语义同步 / 显性失败）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0068 域自检。
pub fn run_vea68_checks() -> CheckSet {
    let mut set = CheckSet::new("vea68");

    use PixelFmt::*;

    // C68-01 交集协商：双方都支持的 SRGB 格式胜出
    {
        let mut n = FmtNegotiator::new(
            Vec::from([Bgra8Srgb, Rgba8Unorm]),
            Vec::from([Bgra8Srgb, Rgba16Float]),
        );
        let r = n.negotiate();
        let ok = r == Ok((Bgra8Srgb, ColorSpace::Srgb));
        chk(&mut set, "C68-01 SRGB 胜出", ok, "SRGB 优先失效");
    }

    // C68-02 SRGB 优先：SRGB 与线性同时可用时选 SRGB
    {
        let mut n = FmtNegotiator::new(
            Vec::from([Rgba8Unorm, Bgra8Srgb]),
            Vec::from([Rgba8Unorm, Bgra8Srgb]),
        );
        let r = n.negotiate();
        let ok = r == Ok((Bgra8Srgb, ColorSpace::Srgb));
        chk(&mut set, "C68-02 优先序", ok, "优先序失准");
    }

    // C68-03 线性备选：SRGB 双方缺一即线性接棒
    {
        let mut n = FmtNegotiator::new(
            Vec::from([Rgba8Unorm]),
            Vec::from([Rgba8Unorm]),
        );
        let r = n.negotiate();
        let ok = r == Ok((Rgba8Unorm, ColorSpace::Linear));
        chk(&mut set, "C68-03 线性备选", ok, "备选未接棒");
    }

    // C68-04 交集为空显性失败
    {
        let mut n = FmtNegotiator::new(
            Vec::from([Rgba16Float]),
            Vec::from([Bgra8Srgb]),
        );
        let ok = n.negotiate() == Err(XC_EMPTY);
        chk(&mut set, "C68-04 交集空显性", ok, "空交集被静默");
    }

    // C68-05 显性失败携带两侧支持集
    {
        let n = FmtNegotiator::new(
            Vec::from([Rgba16Float]),
            Vec::from([Bgra8Srgb]),
        );
        let rep = n.empty_report();
        let ok = rep.contains("surface 支持[5]") && rep.contains("交换链支持[1]");
        chk(&mut set, "C68-05 两侧报告", ok, "报告缺支持集");
    }

    // C68-06 语义同步：SRGB 格式 + 线性标记 = 拒绝呈现
    {
        let mut n = FmtNegotiator::new(Vec::from([Bgra8Srgb]), Vec::from([Bgra8Srgb]));
        let ok = n.verify_pipeline(Bgra8Srgb, ColorSpace::Linear) == Err(XC_CS_MISMATCH)
            && n.verify_pipeline(Bgra8Srgb, ColorSpace::Srgb).is_ok();
        chk(&mut set, "C68-06 语义同步", ok, "错配未拒绝");
    }

    // C68-07 语义同步：线性格式 + 线性标记 = 放行
    {
        let mut n = FmtNegotiator::new(Vec::from([Rgba8Unorm]), Vec::from([Rgba8Unorm]));
        let ok = n.verify_pipeline(Rgba8Unorm, ColorSpace::Linear).is_ok();
        chk(&mut set, "C68-07 线性配对", ok, "合法配对被拒");
    }

    // C68-08 查询失败降级：保守默认带标注
    {
        let mut n = FmtNegotiator::fallback();
        let r = n.negotiate();
        let ok = r == Ok((Bgra8Srgb, ColorSpace::Srgb))
            && n.query_failed()
            && n.status_line().contains("默认");
        chk(&mut set, "C68-08 保守默认", ok, "默认未标注");
    }

    // C68-09 格式即语义：is_srgb 与 ColorSpace::matches 一致
    {
        let ok = Bgra8Srgb.is_srgb()
            && Rgba8Srgb.is_srgb()
            && !Bgra8Unorm.is_srgb()
            && !Rgba16Float.is_srgb()
            && ColorSpace::Srgb.matches(Rgba8Srgb)
            && ColorSpace::Linear.matches(Rgba16Float);
        chk(&mut set, "C68-09 格式即语义", ok, "语义映射失准");
    }

    // C68-10 格式 id 唯一（交集表键）
    {
        let all = [Bgra8Srgb, Bgra8Unorm, Rgba8Srgb, Rgba8Unorm, Rgba16Float];
        let mut ids: Vec<u8> = Vec::new();
        for f in all.iter() {
            ids.push(f.id());
        }
        ids.sort_unstable();
        let mut ok = ids.len() == 5;
        for w in ids.windows(2) {
            if w[0] == w[1] {
                ok = false;
            }
        }
        chk(&mut set, "C68-10 id 唯一", ok, "格式 id 撞号");
    }

    // C68-11 协商结果留痕可查询
    {
        let mut n = FmtNegotiator::new(Vec::from([Bgra8Srgb]), Vec::from([Bgra8Srgb]));
        let _ = n.negotiate();
        let ok = n.chosen() == Some((Bgra8Srgb, ColorSpace::Srgb));
        chk(&mut set, "C68-11 结果留痕", ok, "结果未留痕");
    }

    // C68-12 单侧支持不成立（交集语义）
    {
        let mut n = FmtNegotiator::new(Vec::from([Rgba8Srgb]), Vec::from([Rgba8Unorm]));
        let ok = n.negotiate() == Err(XC_EMPTY);
        chk(&mut set, "C68-12 单侧不算", ok, "单侧支持被放行");
    }

    // C68-13 确定性
    {
        let seq = || -> u8 {
            let mut n = FmtNegotiator::new(
                Vec::from([Rgba8Unorm, Bgra8Srgb]),
                Vec::from([Bgra8Srgb, Rgba16Float]),
            );
            match n.negotiate() {
                Ok((f, _)) => f.id(),
                Err(_) => 0,
            }
        };
        chk(&mut set, "C68-13 确定性", seq() == seq() && seq() == 1, "两次不一致");
    }

    // C68-14 读屏行要素
    {
        let mut n = FmtNegotiator::new(Vec::from([Bgra8Srgb]), Vec::from([Bgra8Srgb]));
        let _ = n.negotiate();
        let line = n.status_line();
        let ok = line.contains("FMT") && line.contains("格式id") && line.contains("色彩空间");
        chk(&mut set, "C68-14 读屏要素", ok, "读屏行缺要素");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea68_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
