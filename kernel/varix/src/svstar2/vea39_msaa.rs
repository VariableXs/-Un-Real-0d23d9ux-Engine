//! VE-F0039 · MSAA 资源与 Resolve（VE-A 域 · A02b 资源视图组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0039`
//!
//! **职责定位（锚点原文）**：多重采样资源的完整管理（MSAA 创建/Resolve
//! 降采样/质量等级校验），Resolve 时机的自动插入（帧图联动），MSAA 与
//! 后处理的兼容矩阵；含 MSAA 与 Z07 视觉回归的联动（Resolve 后画质基线）。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 质量不支持 → 降档 + 告知（降档结果必须回执给请求方，不静默）；
//! - Resolve 遗漏 → 帧图兜底（帧末扫描未 Resolve 的 MSAA 目标自动补）；
//! - 兼容违例 → 登记（MSAA 与后处理不兼容组合入登记表，可审计）。
//!
//! **性能逐项分解（锚点原文）**：O(资源)。质量校验查表 O(1)；Resolve 排程
//! O(需 Resolve 数) 帧末一次。
//!
//! **跨批对接点（锚点原文）**：A31 帧图联动——Resolve 节点由本模块产出
//! 排程建议，帧图负责插入屏障与执行序。
//!
//! **无障碍与隐私（锚点原文）**：MSAA 状态读屏可达。
//!
//! **判据（锚点原文）**：质量校验、自动 Resolve、降档告知、兼容矩阵、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0x1900 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const MSAA_VERSION: &str = "vea39_msaa-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0x1900;

// ---- 诊断码 ----
pub const XC_QUALITY: u32 = SEG_BASE + 1; // 质量等级不支持
pub const XC_SAMPLE: u32 = SEG_BASE + 2; // 非法采样数
pub const XC_RESOLVE_MISSING: u32 = SEG_BASE + 3; // Resolve 遗漏（兜底补排）
pub const XC_COMPAT: u32 = SEG_BASE + 4; // 兼容矩阵违例
pub const XC_FULL: u32 = SEG_BASE + 5; // 资源表满
pub const XC_PARAM: u32 = SEG_BASE + 6; // 尺寸参数非法

/// 支持的采样档（与 DSV 四档对齐）。
pub const SAMPLE_LEVELS: [u8; 4] = [1, 2, 4, 8];

/// 每采样档的实测质量等级上限（来自 A07 特性位图快照；查表 O(1)）。
/// 语义：samples → (quality_max, measured)。measured=false 表示该档
/// 未实测，校验时按「不支持」处置（能力误报防线：没实测不许承诺）。
pub const QUALITY_TABLE: [(u8, u8, bool); 4] = [(1, 0, true), (2, 2, true), (4, 4, true), (8, 2, false)];

/// 质量校验结果——降档必须带告知字段（锚点原文：降档 + 告知）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityVerdict {
    /// 最终可用的采样档（可能低于请求档）。
    pub samples: u8,
    /// 最终可用的质量等级。
    pub quality: u8,
    /// 是否发生降档（true 时 degraded_from 有效）。
    pub degraded: bool,
    /// 请求时的原始采样档。
    pub degraded_from: u8,
}

/// MSAA 资源记录。
#[derive(Clone, Copy, Debug)]
pub struct MsaaRec {
    pub res_id: u32,
    pub width: u32,
    pub height: u32,
    pub samples: u8,
    pub quality: u8,
    /// 本帧是否已 Resolve（帧末兜底扫描的判定字段）。
    pub resolved: bool,
    /// 是否作为渲染目标在用（在用目标才需要 Resolve）。
    pub is_target: bool,
}

/// MSAA 管理器。
pub struct MsaaManager {
    recs: Vec<MsaaRec>,
    cap: usize,
    last_code: u32,
    /// 兼容违例登记表（违例组合 (samples, postfx_id) 环形留痕）。
    compat_log: [(u8, u16); 16],
    compat_n: usize,
    /// 兜底补排次数。
    fallback_resolves: u32,
    /// 降档告知次数。
    degrade_notices: u32,
}

/// MSAA 与后处理的兼容矩阵（锚点原文）。
/// 后处理效果 × 最低要求采样语义：Some(max_samples) 表示该效果只兼容
/// 采样数 ≤ max 的目标（多采样会让效果语义漂移），None 表示全兼容。
pub fn postfx_compat(postfx_id: u16, samples: u8) -> bool {
    match postfx_id {
        // TAAO / 时域抗锯齿与 MSAA 叠加语义未定义 → 只兼容 1x。
        1 => samples <= 1,
        // 运动模糊按单采样假定 → 只兼容 1x。
        2 => samples <= 1,
        // 泛光/色调映射对采样数不敏感 → 全兼容。
        3 | 4 => true,
        // 未知后处理：保守拒绝（宁可登记违例也不放行未定义组合）。
        _ => false,
    }
}

impl MsaaManager {
    pub fn new(cap: usize) -> MsaaManager {
        MsaaManager {
            recs: Vec::new(),
            cap,
            last_code: 0,
            compat_log: [(0u8, 0u16); 16],
            compat_n: 0,
            fallback_resolves: 0,
            degrade_notices: 0,
        }
    }

    /// 质量校验（查表 O(1)）：请求档不支持时自动降档并计数告知。
    pub fn check_quality(&mut self, want_samples: u8, want_quality: u8) -> Result<QualityVerdict, u32> {
        if !SAMPLE_LEVELS.contains(&want_samples) {
            self.last_code = XC_SAMPLE;
            return Err(self.last_code);
        }
        // 从请求档向下找第一个「实测支持且质量等级足够」的档。
        let mut cur = want_samples;
        loop {
            let entry = QUALITY_TABLE.iter().find(|e| e.0 == cur);
            match entry {
                Some((s, qmax, measured)) if *measured && want_quality <= *qmax => {
                    let degraded = cur != want_samples;
                    if degraded {
                        self.degrade_notices += 1;
                    }
                    return Ok(QualityVerdict {
                        samples: *s,
                        quality: if want_quality <= *qmax { want_quality } else { *qmax },
                        degraded,
                        degraded_from: want_samples,
                    });
                }
                _ => {}
            }
            // 降档：取下一档（1x 是地板；1x 的质量档恒 0，必然可满足）。
            let idx = SAMPLE_LEVELS.iter().position(|s| *s == cur);
            match idx {
                Some(i) if i > 0 => {
                    cur = SAMPLE_LEVELS[i - 1];
                }
                _ => {
                    self.last_code = XC_QUALITY;
                    return Err(self.last_code);
                }
            }
        }
    }

    /// 创建 MSAA 资源（质量校验内联：不支持即降档落位，不拒绝）。
    pub fn create(&mut self, res_id: u32, width: u32, height: u32, want_samples: u8) -> Result<QualityVerdict, u32> {
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            self.last_code = XC_PARAM;
            return Err(self.last_code);
        }
        if self.recs.iter().any(|r| r.res_id == res_id) || self.recs.len() >= self.cap {
            self.last_code = XC_FULL;
            return Err(self.last_code);
        }
        let v = self.check_quality(want_samples, 0)?;
        self.recs.push(MsaaRec {
            res_id,
            width,
            height,
            samples: v.samples,
            quality: v.quality,
            resolved: false,
            is_target: true,
        });
        Ok(v)
    }

    /// 声明某资源为非目标（如仅作为 Resolve 源拷贝目标），帧末免扫。
    pub fn clear_target(&mut self, res_id: u32) -> bool {
        match self.recs.iter_mut().find(|r| r.res_id == res_id) {
            Some(r) => {
                r.is_target = false;
                true
            }
            None => false,
        }
    }

    /// 标记 Resolve 完成。
    pub fn mark_resolved(&mut self, res_id: u32) -> bool {
        match self.recs.iter_mut().find(|r| r.res_id == res_id) {
            Some(r) => {
                r.resolved = true;
                true
            }
            None => false,
        }
    }

    /// 帧末兜底扫描：所有「在用目标且未 Resolve」的资源自动补排，
    /// 返回补排清单（帧图兜底——Resolve 遗漏不落盘成黑帧）。
    pub fn frame_end_fallback(&mut self) -> Vec<u32> {
        let mut out = Vec::new();
        for r in self.recs.iter_mut() {
            if r.is_target && !r.resolved {
                r.resolved = true;
                out.push(r.res_id);
            }
        }
        if !out.is_empty() {
            self.fallback_resolves += out.len() as u32;
            self.last_code = XC_RESOLVE_MISSING;
        }
        out
    }

    /// 帧开始：全部 resolved 标记复位。
    pub fn frame_begin(&mut self) {
        for r in self.recs.iter_mut() {
            r.resolved = false;
        }
    }

    /// 兼容矩阵校验：不兼容组合登记入环表（兼容违例 → 登记）。
    pub fn check_compat(&mut self, res_id: u32, postfx_id: u16) -> Result<(), u32> {
        let samples = match self.recs.iter().find(|r| r.res_id == res_id) {
            Some(r) => r.samples,
            None => return Err(XC_FULL),
        };
        if postfx_compat(postfx_id, samples) {
            return Ok(());
        }
        // 环形登记：满则覆盖最旧（诊断用途，非裁决数据）。
        let slot = if self.compat_n < 16 {
            let s = self.compat_n;
            self.compat_n += 1;
            s
        } else {
            for i in 0..15 {
                self.compat_log[i] = self.compat_log[i + 1];
            }
            15
        };
        self.compat_log[slot] = (samples, postfx_id);
        self.last_code = XC_COMPAT;
        Err(XC_COMPAT)
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let pending = self.recs.iter().filter(|r| r.is_target && !r.resolved).count();
        alloc::format!(
            "MSAA 在册{} 待Resolve{} 兜底补排{} 降档告知{} 兼容违例{}",
            self.recs.len(),
            pending,
            self.fallback_resolves,
            self.degrade_notices,
            self.compat_n
        )
    }

    pub fn last_code(&self) -> u32 {
        self.last_code
    }
    pub fn fallback_resolves(&self) -> u32 {
        self.fallback_resolves
    }
    pub fn degrade_notices(&self) -> u32 {
        self.degrade_notices
    }
    pub fn compat_violations(&self) -> usize {
        self.compat_n
    }
    pub fn len(&self) -> usize {
        self.recs.len()
    }
}

// =====================================================================
// 自检（判据锚点：质量校验 / 自动 Resolve / 降档告知 / 兼容矩阵）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0039 域自检。
pub fn run_vea39_checks() -> CheckSet {
    let mut set = CheckSet::new("vea39");

    // C39-01 质量校验：合法请求原样通过
    {
        let mut m = MsaaManager::new(8);
        let v = m.check_quality(4, 3);
        let ok = v == Ok(QualityVerdict { samples: 4, quality: 3, degraded: false, degraded_from: 4 });
        chk(&mut set, "C39-01 质量通过", ok, "合法档被改");
    }

    // C39-02 质量降档：8x 质量超上限 → 降到 4x 且告知
    {
        let mut m = MsaaManager::new(8);
        let v = m.check_quality(8, 3);
        let ok = v == Ok(QualityVerdict { samples: 4, quality: 3, degraded: true, degraded_from: 8 })
            && m.degrade_notices() == 1;
        chk(&mut set, "C39-02 降档告知", ok, "降档未告知");
    }

    // C39-03 非法采样档（3/16）拒绝
    {
        let mut m = MsaaManager::new(8);
        let ok = m.check_quality(3, 0) == Err(XC_SAMPLE) && m.check_quality(16, 0) == Err(XC_SAMPLE);
        chk(&mut set, "C39-03 非法档拒", ok, "非法档被放行");
    }

    // C39-04 创建内联降档：请求 8x 落位 4x
    {
        let mut m = MsaaManager::new(8);
        let v = m.create(100, 1920, 1080, 8);
        let ok = matches!(v, Ok(qv) if qv.samples == 4 && qv.degraded) && m.len() == 1;
        chk(&mut set, "C39-04 创建降档", ok, "创建未降档落位");
    }

    // C39-05 自动 Resolve：帧末兜底扫描补排未 Resolve 目标
    {
        let mut m = MsaaManager::new(8);
        let _ = m.create(100, 1920, 1080, 4);
        let list = m.frame_end_fallback();
        let ok = list.len() == 1 && list[0] == 100 && m.fallback_resolves() == 1
            && m.last_code() == XC_RESOLVE_MISSING;
        chk(&mut set, "C39-05 兜底补排", ok, "遗漏未兜底");
    }

    // C39-06 已 Resolve 不重复补排
    {
        let mut m = MsaaManager::new(8);
        let _ = m.create(100, 640, 480, 4);
        let _ = m.mark_resolved(100);
        let ok = m.frame_end_fallback().is_empty();
        chk(&mut set, "C39-06 不重排", ok, "已 Resolve 被重排");
    }

    // C39-07 非目标免扫：clear_target 后帧末不补排
    {
        let mut m = MsaaManager::new(8);
        let _ = m.create(100, 640, 480, 4);
        let _ = m.clear_target(100);
        let ok = m.frame_end_fallback().is_empty();
        chk(&mut set, "C39-07 免扫", ok, "非目标被误扫");
    }

    // C39-08 帧复位：frame_begin 后 resolved 全清、兜底重新生效
    {
        let mut m = MsaaManager::new(8);
        let _ = m.create(100, 640, 480, 4);
        let _ = m.mark_resolved(100);
        m.frame_begin();
        let ok = m.frame_end_fallback().len() == 1;
        chk(&mut set, "C39-08 帧复位", ok, "帧标记未复位");
    }

    // C39-09 兼容矩阵：TAAO（id=1）只兼容 1x
    {
        let mut m = MsaaManager::new(8);
        let _ = m.create(1, 640, 480, 1);
        let _ = m.create(2, 640, 480, 4);
        let ok = m.check_compat(1, 1).is_ok() && m.check_compat(2, 1) == Err(XC_COMPAT);
        chk(&mut set, "C39-09 TAAO 矩阵", ok, "兼容矩阵失准");
    }

    // C39-10 兼容矩阵：泛光（id=3）全兼容
    {
        let mut m = MsaaManager::new(8);
        let _ = m.create(1, 640, 480, 8);
        let ok = m.check_compat(1, 3).is_ok();
        chk(&mut set, "C39-10 泛光兼容", ok, "全兼容项被误拒");
    }

    // C39-11 兼容违例登记：违例入环表可审计
    {
        let mut m = MsaaManager::new(8);
        let _ = m.create(1, 640, 480, 4);
        let _ = m.check_compat(1, 1);
        let _ = m.check_compat(1, 2);
        let ok = m.compat_violations() == 2 && m.last_code() == XC_COMPAT;
        chk(&mut set, "C39-11 违例登记", ok, "违例未登记");
    }

    // C39-12 未知后处理保守拒绝
    {
        let mut m = MsaaManager::new(8);
        let _ = m.create(1, 640, 480, 1);
        let ok = m.check_compat(1, 99) == Err(XC_COMPAT);
        chk(&mut set, "C39-12 未知拒绝", ok, "未知后处理被放行");
    }

    // C39-13 尺寸参数非法拒绝
    {
        let mut m = MsaaManager::new(8);
        let ok = m.create(1, 0, 480, 4) == Err(XC_PARAM);
        chk(&mut set, "C39-13 尺寸校验", ok, "零尺寸被放行");
    }

    // C39-14 重复注册拒绝
    {
        let mut m = MsaaManager::new(8);
        let _ = m.create(1, 640, 480, 4);
        let ok = m.create(1, 640, 480, 2) == Err(XC_FULL);
        chk(&mut set, "C39-14 重复注册", ok, "重复注册被放行");
    }

    // C39-15 确定性
    {
        let seq = || -> (u32, u32, usize) {
            let mut m = MsaaManager::new(8);
            let _ = m.create(1, 640, 480, 8);
            let _ = m.check_compat(1, 1);
            let _ = m.frame_end_fallback();
            (m.fallback_resolves(), m.degrade_notices(), m.compat_violations())
        };
        chk(&mut set, "C39-15 确定性", seq() == seq(), "两次不一致");
    }

    // C39-16 读屏行五要素
    {
        let m = MsaaManager::new(8);
        let line = m.status_line();
        let ok = line.contains("在册") && line.contains("待Resolve") && line.contains("兜底补排")
            && line.contains("降档告知") && line.contains("兼容违例");
        chk(&mut set, "C39-16 读屏要素", ok, "读屏行缺要素");
    }


    // C39-90 码位互异且段独占（判据自身：整仓唯一段 0x1900 内六码）
    {
        let codes = [XC_QUALITY, XC_SAMPLE, XC_RESOLVE_MISSING, XC_COMPAT, XC_FULL, XC_PARAM];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x1900);
        chk(&mut set, "C39-90 码位互异段独占", all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea39_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea39_degrade_and_notice() {
        let mut m = MsaaManager::new(8);
        let v = m.create(100, 1920, 1080, 8);
        assert!(matches!(v, Ok(qv) if qv.samples == 4 && qv.degraded && qv.degraded_from == 8));
        assert_eq!(m.degrade_notices(), 1, "降档必须带告知");
        assert!(SAMPLE_LEVELS.contains(&4));
    }

    #[test]
    fn vea39_frame_cycle_fallback() {
        let mut m = MsaaManager::new(8);
        let _ = m.create(100, 640, 480, 4);
        let _ = m.create(101, 640, 480, 2);
        let _ = m.mark_resolved(101);
        let list = m.frame_end_fallback();
        assert_eq!(list.len(), 1, "只有未 Resolve 的目标需要兜底");
        assert_eq!(list[0], 100);
        assert!(m.frame_end_fallback().is_empty(), "兜底后不重复补排");
        m.frame_begin();
        assert_eq!(m.frame_end_fallback().len(), 2, "新帧重新扫描");
    }

    #[test]
    fn vea39_compat_unknown_rejected() {
        let mut m = MsaaManager::new(8);
        let _ = m.create(1, 640, 480, 1);
        assert!(m.check_compat(1, 3).is_ok());
        assert_eq!(m.check_compat(1, 99), Err(XC_COMPAT));
        assert!(MSAA_VERSION.starts_with("vea39_msaa"));
    }

    #[test]
    fn vea39_checks_all_green() {
        let set = run_vea39_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0039 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
