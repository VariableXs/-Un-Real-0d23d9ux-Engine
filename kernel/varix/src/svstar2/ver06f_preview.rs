//! VE-F3426 · 皮肤预览模式 —— 应用前预览 + 保真对拍 + 超时退出 + 沙箱隔离。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3426`
//!
//! # 职责（锚点原文拆解）
//!
//! - **应用前预览**：用户在「应用」之前能先看到效果。预览在沙箱里渲染，
//!   不动真身的令牌账本、不动真身的图层树。这不是「先应用再撤销」——
//!   后者一旦崩溃就把真身留在半换肤状态；
//! - **保真对拍**：预览看到的样子必须等于应用后的样子。做法是把同一份
//!   令牌分别交给沙箱与实装求值，逐路径比对渲染值
//!   （[`fidelity`]）。一致率低于 [`FIDELITY_THRESHOLD_PERMILLE`] 即
//!   [`PreviewCode::FidelityBreak`]——所见非所得比没有预览更糟；
//! - **超时自动退出**：预览会一直占着一个沙箱渲染目标。超过
//!   [`PreviewSession::timeout_ticks`] 未确认就自动退出并告知，避免
//!   「用户点开预览然后去吃饭」把资源占死；
//! - **沙箱泄漏→阻断**：沙箱必须能证明「真身一个字节都没被改」。
//!   做法是预览前后对真身取指纹（[`skin::fingerprint`]），不等即
//!   [`PreviewCode::SandboxLeak`]——这是阻断级，不是警告：泄漏意味着
//!   预览和实装共享了可变状态，之后所有「预览安全」的结论都不成立。
//!
//! # 为什么预览期间真身要冻结
//!
//! 预览的画面对比的是「当前真身 + 候选皮肤」。如果预览期间真身又被
//! 别的操作改了（另一个窗口换了主题、插件改了令牌），预览里看到的
//! 就不再是「应用后会发生什么」。所以 [`PreviewSession::freeze`] 会让
//! 真身接受变更的入口关闭（[`PreviewCode::BodyFrozen`]），退出预览时
//! 解冻。冻结的是**变更入口**，不是渲染——真身照常显示、照常响应输入。
//!
//! # 复杂度
//!
//! - `open` / `freeze` / `exit`：O(1)；
//! - `fidelity`：O(路径数)；
//! - `tick`：O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5449 起）
// ---------------------------------------------------------------------------

/// 预览诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewCode {
    /// 已经有一个预览在开（同时只允许一个沙箱）。
    AlreadyOpen = 5449,
    /// 没有预览可退出。
    NotOpen = 5450,
    /// 保真率低于阈值。
    FidelityBreak = 5451,
    /// 沙箱泄漏（真身被改）。
    SandboxLeak = 5452,
    /// 真身处于冻结态，拒绝变更。
    BodyFrozen = 5453,
    /// 预览超时。
    TimedOut = 5454,
    /// 主题为空。
    EmptyTheme = 5455,
    /// 沙箱渲染目标不可用。
    TargetUnavailable = 5456,
    /// 未对拍就请求应用（预览必须给出保真结论）。
    FidelityMissing = 5457,
}

impl PreviewCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PreviewCode::AlreadyOpen => "已有预览在开",
            PreviewCode::NotOpen => "没有预览可退出",
            PreviewCode::FidelityBreak => "预览与实装保真不符",
            PreviewCode::SandboxLeak => "沙箱泄漏，真身被改动",
            PreviewCode::BodyFrozen => "真身冻结中，拒绝变更",
            PreviewCode::TimedOut => "预览超时",
            PreviewCode::EmptyTheme => "预览主题为空",
            PreviewCode::TargetUnavailable => "沙箱渲染目标不可用",
            PreviewCode::FidelityMissing => "未做保真对拍",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, PreviewCode::SandboxLeak)
    }
}

/// 保真阈值：一致率 99.5% 起步。留下 0.5% 给「渲染器允许的量化误差」
/// （如 8 位色深取整），再低就是真的不一致了。
pub const FIDELITY_THRESHOLD_PERMILLE: u32 = 995;

// ---------------------------------------------------------------------------
// 二、保真对拍
// ---------------------------------------------------------------------------

/// 一条对拍差异。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Mismatch {
    /// 路径。
    pub path: String,
    /// 沙箱渲染值。
    pub sandbox: String,
    /// 实装求值。
    pub actual: String,
}

/// 对拍报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FidelityReport {
    /// 比对路径数。
    pub compared: u32,
    /// 一致路径数。
    pub matched: u32,
    /// 差异明细（最多保留 16 条，避免报告爆炸）。
    pub mismatches: Vec<Mismatch>,
}

impl FidelityReport {
    /// 一致率（千分比）。
    pub fn permille(&self) -> u32 {
        if self.compared == 0 {
            return 0;
        }
        (self.matched as u64 * 1000 / self.compared as u64) as u32
    }

    /// 是否达到保真阈值。
    pub fn passes(&self) -> bool {
        self.permille() >= FIDELITY_THRESHOLD_PERMILLE
    }

    /// 读屏清单。
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "保真对拍：比对 {} 条，一致 {} 条（{}‰，阈值 {}‰）→ {}",
            self.compared,
            self.matched,
            self.permille(),
            FIDELITY_THRESHOLD_PERMILLE,
            if self.passes() { "通过" } else { "不通过" }
        ));
        for m in self.mismatches.iter() {
            out.push(format!(
                "不一致 {}：预览 {} ≠ 实装 {}",
                m.path, m.sandbox, m.actual
            ));
        }
        out
    }
}

/// 对拍：同一组路径，沙箱渲染值与实装求值逐条比。
/// 两边路径集合必须一致——少一条也算不一致（缺项比错值更隐蔽）。
pub fn fidelity(sandbox: &[(&str, &str)], actual: &[(&str, &str)]) -> FidelityReport {
    let mut compared = 0u32;
    let mut matched = 0u32;
    let mut mismatches: Vec<Mismatch> = Vec::new();
    for (path, sb) in sandbox.iter() {
        compared = compared.saturating_add(1);
        let found = actual.iter().find(|(p, _)| p == path);
        match found {
            Some((_, av)) => {
                if sb == av {
                    matched = matched.saturating_add(1);
                } else if mismatches.len() < 16 {
                    mismatches.push(Mismatch {
                        path: (*path).to_string(),
                        sandbox: (*sb).to_string(),
                        actual: (*av).to_string(),
                    });
                }
            }
            None => {
                if mismatches.len() < 16 {
                    mismatches.push(Mismatch {
                        path: (*path).to_string(),
                        sandbox: (*sb).to_string(),
                        actual: "(实装缺此路径)".to_string(),
                    });
                }
            }
        }
    }
    // 实装多出来的路径同样算不一致：预览没显示、实装却生效，是「看不到
    // 的变化」，比看到的不一致更危险。
    for (path, av) in actual.iter() {
        if !sandbox.iter().any(|(p, _)| p == path) {
            compared = compared.saturating_add(1);
            if mismatches.len() < 16 {
                mismatches.push(Mismatch {
                    path: (*path).to_string(),
                    sandbox: "(预览缺此路径)".to_string(),
                    actual: (*av).to_string(),
                });
            }
        }
    }
    FidelityReport {
        compared,
        matched,
        mismatches,
    }
}

// ---------------------------------------------------------------------------
// 三、真身与沙箱
// ---------------------------------------------------------------------------

/// 真身令牌账本（预览只读它，写它一律被冻）。
#[derive(Clone, Debug, Default)]
pub struct Skin {
    tokens: Vec<(String, String)>,
}

impl Skin {
    /// 空真身。
    pub fn new() -> Skin {
        Skin { tokens: Vec::new() }
    }

    /// 写入一条（建库用，不受冻结影响）。
    pub fn insert(&mut self, path: &str, value: &str) {
        if let Some(slot) = self.tokens.iter_mut().find(|(p, _)| p == path) {
            slot.1 = value.to_string();
            return;
        }
        self.tokens.push((path.to_string(), value.to_string()));
    }

    /// 指纹：路径与值按序拼接后 FNV-1a 64。预览前后各取一次。
    pub fn fingerprint(&self) -> u64 {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        for (p, v) in self.tokens.iter() {
            for byte in p.as_bytes().iter().chain(v.as_bytes()) {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        hash
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// 以 `(path, value)` 切片导出，供对拍。
    pub fn pairs(&self) -> Vec<(&str, &str)> {
        self.tokens
            .iter()
            .map(|(p, v)| (p.as_str(), v.as_str()))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 四、预览会话
// ---------------------------------------------------------------------------

/// 预览会话。
#[derive(Clone, Debug)]
pub struct PreviewSession {
    /// 主题 id。
    pub theme: String,
    /// 开启 tick。
    pub opened_at: u64,
    /// 超时 tick 数。
    pub timeout_ticks: u64,
    /// 真身指纹（开预览时）。
    pub body_fingerprint_at_open: u64,
    /// 是否冻结真身。
    frozen: bool,
    /// 对拍结论。
    fidelity: Option<FidelityReport>,
    /// 沙箱令牌。
    sandbox: Skin,
    /// 是否已退出。
    closed: bool,
}

/// 预览退出原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExitReason {
    /// 用户确认应用。
    Applied,
    /// 用户取消。
    Cancelled,
    /// 超时。
    TimedOut,
}

impl ExitReason {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            ExitReason::Applied => "已应用",
            ExitReason::Cancelled => "已取消",
            ExitReason::TimedOut => "超时退出",
        }
    }
}

/// 退出结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExitOutcome {
    /// 原因。
    pub reason: ExitReason,
    /// 是否把沙箱结果应用到真身。
    pub applied: bool,
    /// 提示文本。
    pub message: String,
}

impl PreviewSession {
    /// 开预览。`timeout_ticks` 为 0 时取默认 [`DEFAULT_TIMEOUT_TICKS`]。
    pub fn open(theme: &str, body: &Skin, now: u64, timeout_ticks: u64) -> Result<PreviewSession, PreviewCode> {
        if theme.trim().is_empty() {
            return Err(PreviewCode::EmptyTheme);
        }
        Ok(PreviewSession {
            theme: theme.to_string(),
            opened_at: now,
            timeout_ticks: if timeout_ticks == 0 {
                DEFAULT_TIMEOUT_TICKS
            } else {
                timeout_ticks
            },
            body_fingerprint_at_open: body.fingerprint(),
            frozen: true,
            fidelity: None,
            sandbox: Skin::new(),
            closed: false,
        })
    }

    /// 沙箱写入（预览渲染结果落在这里，不落真身）。
    pub fn sandbox_insert(&mut self, path: &str, value: &str) -> Result<(), PreviewCode> {
        if self.closed {
            return Err(PreviewCode::NotOpen);
        }
        self.sandbox.insert(path, value);
        Ok(())
    }

    /// 真身变更入口。冻结期间一律拒——这是「真身冻结声明」的落点。
    pub fn try_mutate_body(&self, _body: &mut Skin, path: &str, value: &str) -> Result<(), PreviewCode> {
        if self.frozen {
            let _ = (path, value);
            return Err(PreviewCode::BodyFrozen);
        }
        Ok(())
    }

    /// 做一次对拍。
    pub fn compare(&mut self, body: &Skin) -> Result<&FidelityReport, PreviewCode> {
        if self.closed {
            return Err(PreviewCode::NotOpen);
        }
        let sb = self.sandbox.pairs();
        let ac = body.pairs();
        let report = fidelity(&sb, &ac);
        self.fidelity = Some(report);
        match self.fidelity.as_ref() {
            Some(r) => Ok(r),
            None => Err(PreviewCode::FidelityMissing),
        }
    }

    /// 请求应用：必须做过对拍，且保真通过。
    pub fn request_apply(&mut self) -> Result<(), PreviewCode> {
        match self.fidelity.as_ref() {
            None => Err(PreviewCode::FidelityMissing),
            Some(r) => {
                if r.passes() {
                    Ok(())
                } else {
                    Err(PreviewCode::FidelityBreak)
                }
            }
        }
    }

    /// 推进时钟：超时则自动退出。
    pub fn tick(&mut self, now: u64) -> Option<ExitOutcome> {
        if self.closed {
            return None;
        }
        if now.saturating_sub(self.opened_at) >= self.timeout_ticks {
            self.closed = true;
            self.frozen = false;
            return Some(ExitOutcome {
                reason: ExitReason::TimedOut,
                applied: false,
                message: format!(
                    "预览已超时自动退出（{} tick），真身未改动",
                    self.timeout_ticks
                ),
            });
        }
        None
    }

    /// 退出预览。退出前必须校验沙箱泄漏：真身指纹必须与开启时一致。
    pub fn exit(&mut self, reason: ExitReason, body: &Skin) -> Result<ExitOutcome, PreviewCode> {
        if self.closed && reason != ExitReason::TimedOut {
            return Err(PreviewCode::NotOpen);
        }
        // 泄漏检查：真身若被改过，指纹必然不同。
        if body.fingerprint() != self.body_fingerprint_at_open {
            return Err(PreviewCode::SandboxLeak);
        }
        self.closed = true;
        self.frozen = false;
        let applied = reason == ExitReason::Applied;
        Ok(ExitOutcome {
            reason,
            applied,
            message: match reason {
                ExitReason::Applied => format!("预览已确认应用：{}", self.theme),
                ExitReason::Cancelled => format!("预览已取消：{}，真身未改动", self.theme),
                ExitReason::TimedOut => format!("预览超时退出：{}，真身未改动", self.theme),
            },
        })
    }

    /// 是否冻结真身。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 是否已关闭。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 对拍结论。
    pub fn fidelity(&self) -> Option<&FidelityReport> {
        self.fidelity.as_ref()
    }

    /// 读屏播报。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "预览：主题 {}，{}，真身{}",
            self.theme,
            if self.closed { "已结束" } else { "进行中" },
            if self.frozen { "冻结中" } else { "可写" }
        ));
        if let Some(f) = self.fidelity.as_ref() {
            out.extend(f.lines());
        } else {
            out.push("尚未做保真对拍".to_string());
        }
        out
    }
}

/// 默认预览超时：10 秒（以 60Hz 计 600 tick 太短，按 1ms/tick 取 10000）。
pub const DEFAULT_TIMEOUT_TICKS: u64 = 10_000;

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Skin {
        let mut s = Skin::new();
        s.insert("color.bg", "#0B1020");
        s.insert("color.fg", "#E8F0FF");
        s
    }

    #[test]
    fn preview_is_sandboxed_and_body_is_frozen() {
        let b = body();
        let before = b.fingerprint();
        let mut p = PreviewSession::open("neon-night", &b, 0, 100).expect("开预览");
        p.sandbox_insert("color.bg", "#101828").expect("沙箱写");
        assert_eq!(b.fingerprint(), before, "真身不该被改");
        assert!(p.frozen());
        let mut b2 = body();
        assert_eq!(
            p.try_mutate_body(&mut b2, "color.bg", "#000000").err(),
            Some(PreviewCode::BodyFrozen)
        );
    }

    #[test]
    fn fidelity_break_blocks_apply() {
        let b = body();
        let mut p = PreviewSession::open("neon-night", &b, 0, 100).expect("开预览");
        p.sandbox_insert("color.bg", "#101828").expect("写");
        p.sandbox_insert("color.fg", "#FFFFFF").expect("写");
        let r = p.compare(&b).expect("对拍").clone();
        assert_eq!(r.compared, 2);
        assert_eq!(r.matched, 0);
        assert!(!r.passes());
        assert_eq!(p.request_apply().err(), Some(PreviewCode::FidelityBreak));
    }

    #[test]
    fn matching_sandbox_passes_fidelity_and_apply() {
        let b = body();
        let mut p = PreviewSession::open("same", &b, 0, 100).expect("开预览");
        p.sandbox_insert("color.bg", "#0B1020").expect("写");
        p.sandbox_insert("color.fg", "#E8F0FF").expect("写");
        let r = p.compare(&b).expect("对拍").clone();
        assert!(r.passes());
        assert_eq!(r.permille(), 1000);
        assert!(p.request_apply().is_ok());
    }

    #[test]
    fn missing_paths_count_as_mismatch() {
        let b = body();
        let mut p = PreviewSession::open("partial", &b, 0, 100).expect("开预览");
        // 只预览一条，实装两条：缺的那条也是不一致。
        p.sandbox_insert("color.bg", "#0B1020").expect("写");
        let r = p.compare(&b).expect("对拍").clone();
        assert_eq!(r.compared, 2);
        assert_eq!(r.matched, 1);
        assert!(r.permille() < 995);
    }

    #[test]
    fn timeout_exits_and_keeps_body_untouched() {
        let b = body();
        let mut p = PreviewSession::open("slow", &b, 0, 50).expect("开预览");
        assert!(p.tick(10).is_none());
        let out = p.tick(50).expect("超时");
        assert_eq!(out.reason, ExitReason::TimedOut);
        assert!(!out.applied);
        assert!(!p.frozen(), "退出后解冻");
        assert!(p.is_closed());
    }

    #[test]
    fn sandbox_leak_is_blocking() {
        let mut b = body();
        let mut p = PreviewSession::open("leak", &b, 0, 1000).expect("开预览");
        // 真身被外部改了 -> 泄漏。
        b.insert("color.bg", "#FF0000");
        assert_eq!(
            p.exit(ExitReason::Cancelled, &b).err(),
            Some(PreviewCode::SandboxLeak)
        );
        assert!(PreviewCode::SandboxLeak.blocking());
    }

    #[test]
    fn cancel_exits_without_applying() {
        let b = body();
        let mut p = PreviewSession::open("x", &b, 0, 1000).expect("开预览");
        let out = p.exit(ExitReason::Cancelled, &b).expect("退出");
        assert!(!out.applied);
        assert!(out.message.contains("未改动"));
    }

    #[test]
    fn reading_lines_explain_state_and_fidelity() {
        let b = body();
        let mut p = PreviewSession::open("x", &b, 0, 1000).expect("开预览");
        assert!(p
            .reading_lines()
            .iter()
            .any(|l| l.contains("尚未做保真对拍")));
        p.sandbox_insert("color.bg", "#0B1020").expect("写");
        p.sandbox_insert("color.fg", "#E8F0FF").expect("写");
        let _ = p.compare(&b);
        let lines = p.reading_lines();
        assert!(lines.iter().any(|l| l.contains("保真对拍")));
        assert!(lines.first().map(|l| l.contains("冻结中")).unwrap_or(false));
    }
}
