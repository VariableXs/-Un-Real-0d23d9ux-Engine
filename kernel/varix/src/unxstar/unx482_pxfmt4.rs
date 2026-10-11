//! unx482_pxfmt4 —— UNX-F0482 像素格式判定（BGRX/XRGB 族）· 四分发独立实现（双实现并存之二）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B24）：UNX-F0482 · 像素格式判定（BGRX/XRGB 族） · 目标 220 行 · 已深化
//! 判据原文：格式判定四分发（32bpp: BGRX/XRGB/24bpp/16bpp）落账，判定依据账注（红蓝序实测抽样核）
//! 组件：四分发判定树（bpp 叉 → 32bpp 红蓝序叉）+ 双样本红蓝序实测器（写满值读通道）
//!      + 判定依据账注（每条判定强制带 basis 注）+ 格式账（append-only）+ 描述符预筛门
//! 判据落地：四分发落账 + 判定依据账注（红蓝序实测抽样核）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，显存读写注入（零真实 IO）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const PX_VERSION: &str = "unx482_pxfmt4-v1";

/// 诊断段基址（0x4c50 段 · 与并行实现同段位不同码位区间 0x4c50-0x4c5f 本实现独用 0x4c60 以下间隔）。
/// 注：双实现并存，本实现码位取段内 0x4c58 起（前段留予先入库实现）。
pub const PX_SEG_BASE: u16 = 0x4c58;

/// 格式闭集（四分发四叶 + Unknown）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PxFormat {
    /// 32bpp：低字节 = 蓝（BGRX 内存序）。
    Bgrx,
    /// 32bpp：低字节 = 红（XRGB 内存序）。
    Xrgb,
    /// 24bpp：三字节无填充。
    Rgb888,
    /// 16bpp：RGB565 位域。
    Rgb565,
    /// 证据不足，显性未知。
    Undet,
}

impl PxFormat {
    /// 名与 bpp。
    pub const fn spec(self) -> (&'static str, u8) {
        match self {
            PxFormat::Bgrx => ("BGRX", 32),
            PxFormat::Xrgb => ("XRGB", 32),
            PxFormat::Rgb888 => ("RGB888", 24),
            PxFormat::Rgb565 => ("RGB565", 16),
            PxFormat::Undet => ("未判定", 0),
        }
    }
}

/// 诊断码（本实现独用 0x4c58~0x4c5f 八码——段内分区防撞）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PxCode {
    /// 0x4c58 描述符 bpp 越四分发闭集。
    BppClosed,
    /// 0x4c59 探针偏移越帧缓冲界。
    ProbeOob,
    /// 0x4c5a 写读回环失败（回环一致性是通道判定的前置）。
    RwLoop,
    /// 0x4c5b 双样本通道证据互斥。
    ProbesDiffer,
    /// 0x4c5c 判定落账缺依据注。
    NoBasis,
    /// 0x4c5d 同帧缓冲重复判定且结论相异。
    RejudgeDiff,
    /// 0x4c5e 账实破缺（账条数 ≢ 判定计数）。
    LedgerBad,
    /// 0x4c5f 段位守卫失败。
    SegmentGuard,
}

impl PxCode {
    /// 全量闭集。
    pub const ALL: [PxCode; 8] = [
        PxCode::BppClosed,
        PxCode::ProbeOob,
        PxCode::RwLoop,
        PxCode::ProbesDiffer,
        PxCode::NoBasis,
        PxCode::RejudgeDiff,
        PxCode::LedgerBad,
        PxCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        PX_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            PxCode::BppClosed => "bpp 越四分发闭集：判定树只覆盖 32/24/16",
            PxCode::ProbeOob => "探针偏移越帧缓冲界：探针点必须落在缓冲内",
            PxCode::RwLoop => "写读回环失败：通道判定前置条件不成立",
            PxCode::ProbesDiffer => "双样本通道证据互斥：红蓝序不可判，显性 Unknown",
            PxCode::NoBasis => "判定落账缺依据注：违反账注强制要求",
            PxCode::RejudgeDiff => "重复判定结论相异：以最新为准旧结论立案",
            PxCode::LedgerBad => "账实破缺：账条数与判定计数不一致",
            PxCode::SegmentGuard => "段位守卫失败：诊断码越出本实现分区",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            PxCode::BppClosed => "描述符上报层先钳闭集；越界属上报错误",
            PxCode::ProbeOob => "探针点取第 1 行第 1 像素（偏移 0），先验窗",
            PxCode::RwLoop => "先核 stride 与映射有效性：回环失败时通道序无从谈起",
            PxCode::ProbesDiffer => "补第三样本仲裁；仍互斥则按 Unknown 显性出账",
            PxCode::NoBasis => "落账调用点强制携带 PfBasisNote：核对判定树出口",
            PxCode::RejudgeDiff => "二次判定前先查账：同基址已有结论须先立案再重判",
            PxCode::LedgerBad => "判定与落账同一事务：核对判定树出口的记账",
            PxCode::SegmentGuard => "诊断码分配越段：核对本实现分区基址 0x4c58",
        }
    }
}

/// 显存读写注入（零真实 IO）。
pub trait PxMem {
    /// 写 32 位到偏移。
    fn put(&mut self, off: u64, v: u32);
    /// 读 32 位。
    fn get(&self, off: u64) -> Option<u32>;
}

/// 依据账注（判定结论的「为什么」——判据强制项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PxBasis {
    /// 双样本红蓝序实测：低字节通道响应为蓝。
    BlueAtLow,
    /// 双样本红蓝序实测：低字节通道响应为红。
    RedAtLow,
    /// bpp 直判（16 位域 / 24 三字节结构唯一）。
    BppStructural,
    /// 证据互斥显性未知。
    ProbesSplit,
}

impl PxBasis {
    /// 中文账注。
    pub const fn note(self) -> &'static str {
        match self {
            PxBasis::BlueAtLow => "红蓝序实测抽样核：蓝色满值响应落在低字节 → BGRX",
            PxBasis::RedAtLow => "红蓝序实测抽样核：红色满值响应落在低字节 → XRGB",
            PxBasis::BppStructural => "bpp 结构直判：位域布局由 bpp 唯一决定，无需通道探针",
            PxBasis::ProbesSplit => "红蓝序双样本互斥：证据不足，显性未判定",
        }
    }
}

/// 判定账条目。
#[derive(Clone, Copy, Debug)]
pub struct PxEntry {
    /// 帧缓冲基址标识。
    pub base: u64,
    /// 结论。
    pub fmt: PxFormat,
    /// 依据注。
    pub basis: PxBasis,
    /// 实测样本数。
    pub samples: u32,
}

/// 四分发判定器（独立实现之二：判定树 + 强制账注）。
pub struct PxJudge4 {
    /// 判定账。
    pub ledger: Vec<PxEntry>,
    /// 诊断码。
    pub codes: Vec<PxCode>,
    /// 判定计数。
    pub count: u64,
}

impl PxJudge4 {
    /// 建。
    pub fn new() -> Self {
        PxJudge4 { ledger: Vec::new(), codes: Vec::new(), count: 0 }
    }

    fn note(&mut self, c: PxCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 双样本红蓝序实测（判据核心路径）：
    /// 样本 A：写 0x00FF0000（红满）→ 读回低 8 位非零即红在低字节；
    /// 样本 B：写 0x000000FF（蓝满）→ 读回低 8 位非零即蓝在低字节；
    /// 两样本结论必须互斥且恰一成立，互斥失败 = 显性 Unknown。
    fn red_blue_probe<M: PxMem>(m: &mut M, off: u64) -> Result<PxBasis, PxCode> {
        // 回环前置：全 1 写读。
        m.put(off, 0xFFFF_FFFF);
        if m.get(off) != Some(0xFFFF_FFFF) {
            return Err(PxCode::RwLoop);
        }
        // 样本 A：红满。
        m.put(off, 0x00FF_0000);
        let ra = m.get(off).ok_or(PxCode::RwLoop)?;
        let red_low = (ra & 0xFF) == 0xFF;
        // 样本 B：蓝满。
        m.put(off, 0x0000_00FF);
        let rb = m.get(off).ok_or(PxCode::RwLoop)?;
        let blue_low = (rb & 0xFF) == 0xFF;
        // 互斥核：恰一成立。
        match (red_low, blue_low) {
            (true, false) => Ok(PxBasis::RedAtLow),
            (false, true) => Ok(PxBasis::BlueAtLow),
            _ => Err(PxCode::ProbesDiffer),
        }
    }

    /// 四分发判定（入口）。
    pub fn judge<M: PxMem>(
        &mut self,
        m: &mut M,
        base: u64,
        span: u64,
        bpp: u32,
    ) -> Result<PxFormat, (PxCode, String)> {
        // 叉 0：bpp 闭集门。
        if !matches!(bpp, 16 | 24 | 32) {
            self.note(PxCode::BppClosed);
            return Err((PxCode::BppClosed, PxCode::BppClosed.advice().to_string()));
        }
        // 探针点：偏移 0（第 1 像素）。
        if span < 4 {
            self.note(PxCode::ProbeOob);
            return Err((PxCode::ProbeOob, PxCode::ProbeOob.advice().to_string()));
        }
        // 叉 1：16bpp。
        let (fmt, basis, samples) = if bpp == 16 {
            // 结构直判 + 位域回环核（5/6/5 全位写读）。
            m.put(0, 0x0000_7BEF);
            let r = m.get(0).ok_or(PxCode::RwLoop).map_err(|c| (c, c.advice().to_string()))?;
            if r & 0xFFFF != 0x7BEF {
                return Err((PxCode::RwLoop, PxCode::RwLoop.advice().to_string()));
            }
            (PxFormat::Rgb565, PxBasis::BppStructural, 1)
        } else if bpp == 24 {
            // 叉 2：24bpp 结构直判 + 低 24 位回环核。
            m.put(0, 0x005A_3C96);
            let r = m.get(0).ok_or(PxCode::RwLoop).map_err(|c| (c, c.advice().to_string()))?;
            if r & 0x00FF_FFFF != 0x005A_3C96 {
                return Err((PxCode::RwLoop, PxCode::RwLoop.advice().to_string()));
            }
            (PxFormat::Rgb888, PxBasis::BppStructural, 1)
        } else {
            // 叉 3：32bpp 红蓝序二分。
            match Self::red_blue_probe(m, 0) {
                Ok(b) => {
                    let f = if b == PxBasis::BlueAtLow { PxFormat::Bgrx } else { PxFormat::Xrgb };
                    (f, b, 2)
                }
                Err(PxCode::ProbesDiffer) => (PxFormat::Undet, PxBasis::ProbesSplit, 2),
                Err(c) => return Err((c, c.advice().to_string())),
            }
        };
        // 重复判定核。
        if let Some(prev) = self.ledger.iter().find(|e| e.base == base) {
            if prev.fmt != fmt {
                self.note(PxCode::RejudgeDiff);
                return Err((PxCode::RejudgeDiff, PxCode::RejudgeDiff.advice().to_string()));
            }
        }
        // 落账（依据注强制）。
        if basis.note().is_empty() {
            self.note(PxCode::NoBasis);
            return Err((PxCode::NoBasis, PxCode::NoBasis.advice().to_string()));
        }
        self.ledger.push(PxEntry { base, fmt, basis, samples });
        self.count += 1;
        Ok(fmt)
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.ledger.len() as u64 == self.count
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        let mut s = format!("四分发判定：{} 条账 / {} 次判定，", self.ledger.len(), self.count);
        for e in &self.ledger {
            s.push_str(&format!("@{:#x}={}({}；{}样本) ", e.base, e.fmt.spec().0, e.basis.note(), e.samples));
        }
        if !self.codes.is_empty() {
            s.push_str("；诊断：");
            for c in &self.codes {
                s.push_str(&format!("[{:04X}]", c.val()));
            }
        }
        s
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    PxCode::ALL.iter().all(|c| c.val() >= PX_SEG_BASE && c.val() < PX_SEG_BASE + 8)
}
