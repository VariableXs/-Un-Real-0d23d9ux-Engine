//! VE-F0015 · 渲染错误分类与上抛纪律（VE-A 域 · 内核图形抽象层 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0015`
//!
//! **判据（锚点原文）**：渲染错误的分类学（资源/命令/同步/驱动四类）与上抛
//! 纪律（零静默：每个错误必须到达总日志中心），错误码三要素（发生了什么/
//! 为什么/下一步）；错误分类含自动归因建议（四类错误的常见根因对照表）；
//! 上抛管道含背压保护（错误风暴不淹日志中心）；三要素含读屏完整播报；
//! 错误码与总日志中心的双向检索（从日志回查错误详情）。错误路径：静默→
//! 阻断级；分类错误→修正；码缺三要素→补齐。
//!
//! **设计要点**：
//! - **四类分类学**：[`ErrClass`] 四类（资源/命令/同步/驱动），分类错误
//!   →修正（外部上报错误分类时若与归因建议冲突，给修正建议入账）；
//! - **三要素**：[`RenderError`] 强制三字段（what/why/next）——码缺三要素
//!   →补齐：构造期缺要素直接拒绝（边界防护在源头，不给静默空字段机会）；
//! - **零静默 + 上抛纪律**：`raise()` 是唯一上抛口，错误必达总日志中心
//!   （[`LogCenter`]）；管道断链（日志中心满且背压拒绝）时错误转入本地
//!   兜底账并显性计数——断链也可见，不是静默丢；
//! - **自动归因建议**：四类 × 常见根因对照表（静态公开），raise 时按
//!   错误码前缀自动匹配归因建议附进 why/next；
//! - **背压保护**：日志中心容量 + 窗口丢弃策略（错误风暴时不淹日志中心：
//!   新错误按窗口聚合计数，风暴消退后留聚合条目）——丢弃量显性入账；
//!   静默→阻断级：任何静默尝试（绕过 raise 的直写）在审计层判阻断级；
//! - **双向检索**：正查（错误码 → 日志中心条目）、反查（日志条目 →
//!   错误详情）双向可检索；
//! - **读屏完整播报**：三要素一次播全（`screen_text()`），不截断。
//!
//! **跨批对接点**：总日志中心对接；上游 F0012 探针告警、F0013 回退事件。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 总日志中心容量（背压保护的水位线）。
pub const LOG_CENTER_CAPACITY: usize = 256;

/// 背压聚合窗口（窗口内同类错误聚合为一条 + 计数）。
pub const BACKPRESSURE_WINDOW: usize = 16;

/// 与总日志中心的契约版本。
pub const LOG_LINK_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 二、四类分类学（资源/命令/同步/驱动）
// ---------------------------------------------------------------------------

/// 渲染错误四大类（锚点分类学）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrClass {
    /// 资源类（显存/纹理/缓冲的创建与生命周期）。
    Resource,
    /// 命令类（命令缓冲/提交/执行）。
    Command,
    /// 同步类（围栏/信号量/等待链）。
    Sync,
    /// 驱动类（设备丢失/驱动崩溃/TDR）。
    Driver,
}

impl ErrClass {
    /// 类名（读屏与检索用）。
    pub fn name(self) -> &'static str {
        match self {
            ErrClass::Resource => "资源",
            ErrClass::Command => "命令",
            ErrClass::Sync => "同步",
            ErrClass::Driver => "驱动",
        }
    }

    /// 自动归因：四类错误的常见根因对照表（判据点名）。
    ///
    /// 返回 (常见根因, 下一步建议)——构造错误时自动附进三要素。
    pub fn common_causes(self, code: &str) -> (&'static str, &'static str) {
        match self {
            ErrClass::Resource => match code {
                c if c.starts_with("E_OOM") => ("显存/内存水位超限", "先降档裁剪再重试；持续超限则触发预算仲裁复核"),
                c if c.starts_with("E_HANDLE") => ("句柄悬垂或代数过期", "复核句柄表生成号；用完即还，禁止复用已销毁句柄"),
                _ => ("资源生命周期管理失序", "检查创建/销毁配对与引用计数"),
            },
            ErrClass::Command => match code {
                c if c.starts_with("E_SUBMIT") => ("提交时机越界（帧边界外提交）", "提交必须落在帧预算内；越界命令退回重排"),
                c if c.starts_with("E_NESTED") => ("命令嵌套/重入", "拉平命令序列，禁止段内嵌套 begin"),
                _ => ("命令序非法", "按帧任务图拓扑序重排命令"),
            },
            ErrClass::Sync => match code {
                c if c.starts_with("E_DEADLOCK") => ("等待环（循环等待）", "按等待链图打断环；核对白名单豁免"),
                c if c.starts_with("E_TIMEOUT") => ("等待超时（预算耗尽）", "核对超时默认值与预算；必要时升级降级链"),
                _ => ("同步原语语义误用", "核对 wait/signal 配对与归属"),
            },
            ErrClass::Driver => match code {
                c if c.starts_with("E_TDR") => ("GPU 超时复位", "走设备丢失恢复状态机（A09 三段）"),
                c if c.starts_with("E_LOST") => ("设备丢失", "热拔重选或软渲回退；原因分类入账"),
                _ => ("驱动交互异常", "核对驱动版本指纹与规避开关"),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// 三、错误码三要素（发生了什么/为什么/下一步）
// ---------------------------------------------------------------------------

/// 渲染错误（三要素强制齐备——构造期缺要素直接拒绝）。
#[derive(Clone, Debug)]
pub struct RenderError {
    /// 错误码（E_ 前缀稳定码，双向检索的锚）。
    pub code: String,
    /// 四类之一。
    pub class: ErrClass,
    /// 要素一：发生了什么。
    pub what: String,
    /// 要素二：为什么（自动归因建议优先，调用方补充在后）。
    pub why: String,
    /// 要素三：下一步。
    pub next: String,
}

impl RenderError {
    /// 构造（码缺三要素→补齐的源头防线：空要素拒绝构造）。
    pub fn new(
        code: &str,
        class: ErrClass,
        what: &str,
        extra_why: &str,
        extra_next: &str,
    ) -> Result<Self, &'static str> {
        if code.is_empty() || !code.starts_with("E_") {
            return Err("E_CODE_INVALID");
        }
        if what.trim().is_empty() || extra_why.trim().is_empty() || extra_next.trim().is_empty() {
            return Err("E_TRIAD_INCOMPLETE");
        }
        let (cause, advice) = class.common_causes(code);
        Ok(RenderError {
            code: code.to_string(),
            class,
            what: what.to_string(),
            why: format!("{}；调用方补充：{}", cause, extra_why),
            next: format!("{}；调用方补充：{}", advice, extra_next),
        })
    }

    /// 读屏完整播报（三要素一次播全，不截断——判据点名）。
    pub fn screen_text(&self) -> String {
        format!(
            "渲染错误 {}（{}类）：发生了什么——{}。为什么——{}。下一步——{}。",
            self.code, self.class.name(), self.what, self.why, self.next
        )
    }
}

// ---------------------------------------------------------------------------
// 四、上抛管道与总日志中心（零静默 + 背压 + 双向检索）
// ---------------------------------------------------------------------------

/// 总日志中心条目。
#[derive(Clone, Debug)]
pub struct LogEntry {
    /// 错误码（正查锚）。
    pub code: String,
    /// 类名。
    pub class: &'static str,
    /// 读屏全文（反查时回放的详情）。
    pub text: String,
    /// 入账时刻（逻辑 tick）。
    pub tick: u64,
    /// 背压聚合计数（1 = 独立错误；>1 = 风暴聚合条目）。
    pub aggregated: u32,
}

/// 总日志中心（容量水位 + 背压聚合 + 双向检索）。
pub struct LogCenter {
    /// 日志条目。
    pub entries: Vec<LogEntry>,
    /// 背压丢弃/聚合总量（风暴被吸收的错误数——显性入账）。
    pub backpressure_absorbed: u32,
    dropped: usize,
    tick: u64,
}

impl LogCenter {
    /// 构造。
    pub fn new() -> Self {
        LogCenter { entries: Vec::new(), backpressure_absorbed: 0, dropped: 0, tick: 0 }
    }

    /// 逻辑时钟推进。
    pub fn advance_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    /// 上抛唯一入口（零静默纪律）：错误必达。
    ///
    /// 背压保护：容量满时启用聚合窗口——同类同码错误在窗口内聚合计数，
    /// 不再新增条目（错误风暴不淹日志中心）；风暴条目保留最新详情。
    /// 返回 true = 已入账；false = 断链（中心不可用），调用方须走本地兜底。
    pub fn admit(&mut self, err: &RenderError) -> bool {
        self.tick = self.tick.saturating_add(1);
        if self.entries.len() < LOG_CENTER_CAPACITY {
            // 风暴聚合：末尾同码同类条目且在其聚合窗口内 → 计数 +1。
            if let Some(last) = self.entries.last_mut() {
                if last.code == err.code && last.aggregated > 0
                    && self.tick.saturating_sub(last.tick) <= BACKPRESSURE_WINDOW as u64
                {
                    last.aggregated += 1;
                    self.backpressure_absorbed += 1;
                    return true;
                }
            }
            self.entries.push(LogEntry {
                code: err.code.clone(),
                class: err.class.name(),
                text: err.screen_text(),
                tick: self.tick,
                aggregated: 1,
            });
            return true;
        }
        // 容量满：聚合窗口内的同码风暴继续吸收（不新增条目）。
        if let Some(last) = self.entries.last_mut() {
            if last.code == err.code
                && self.tick.saturating_sub(last.tick) <= BACKPRESSURE_WINDOW as u64
            {
                last.aggregated += 1;
                self.backpressure_absorbed += 1;
                return true;
            }
        }
        // 无可聚合 → 淘汰最旧（背压显性入账，绝不静默丢）。
        self.entries.remove(0);
        self.dropped += 1;
        self.entries.push(LogEntry {
            code: err.code.clone(),
            class: err.class.name(),
            text: err.screen_text(),
            tick: self.tick,
            aggregated: 1,
        });
        true
    }

    /// 静默审计（静默→阻断级判据）：检查绕过 raise 的直写尝试。
    ///
    /// 返回阻断级结论——任何直写都要求改走 raise（审计显性化）。
    pub fn audit_silence(&self, direct_writes: usize) -> (bool, String) {
        if direct_writes == 0 {
            (true, "无静默直写，上抛纪律合规".to_string())
        } else {
            (
                false,
                format!(
                    "阻断级：检测到 {} 次绕过 raise 的直写尝试——静默即违例，全部改走 raise",
                    direct_writes
                ),
            )
        }
    }

    /// 正查：错误码 → 日志条目（双向检索之一）。
    pub fn query_by_code(&self, code: &str) -> Vec<&LogEntry> {
        self.entries.iter().filter(|e| e.code == code).collect()
    }

    /// 反查：日志条目 → 错误详情全文（双向检索之二）。
    pub fn detail_of(&self, code: &str, tick: u64) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.code == code && e.tick == tick)
            .map(|e| e.text.as_str())
    }

    /// 累计丢弃数（背压显性化）。
    pub fn dropped(&self) -> usize {
        self.dropped
    }
}

impl Default for LogCenter {
    fn default() -> Self {
        Self::new()
    }
}

/// 上抛管道（渲染侧的门面：分类正确性复核 + 必达上抛）。
pub struct RaisePipeline {
    /// 日志中心引用（唯一出口）。
    pub center: LogCenter,
    /// 分类修正账（分类错误→修正）。
    pub corrections: Vec<(String, &'static str, &'static str)>,
    /// 断链兜底账（管道断链时的本地留存——断链也可见）。
    pub fallback: Vec<(String, u64)>,
    /// 直写尝试计数（静默审计输入）。
    pub direct_writes: usize,
    raised: u32,
}

impl RaisePipeline {
    /// 构造。
    pub fn new() -> Self {
        RaisePipeline {
            center: LogCenter::new(),
            corrections: Vec::new(),
            fallback: Vec::new(),
            direct_writes: 0,
            raised: 0,
        }
    }

    /// 上抛（零静默主流程）：构造错误 → 分类复核 → 必达日志中心。
    ///
    /// 分类复核：归因表与调用方 class 冲突时给修正建议入账
    /// （分类错误→修正判据）；中心断链时入本地兜底账并显性计数。
    pub fn raise(&mut self, err: RenderError) -> bool {
        self.raised += 1;
        // 分类修正：资源类错误码报成驱动类等常见错分 → 建议修正。
        let suggested = match err.code.as_str() {
            c if c.starts_with("E_OOM") || c.starts_with("E_HANDLE") => ErrClass::Resource,
            c if c.starts_with("E_SUBMIT") || c.starts_with("E_NESTED") => ErrClass::Command,
            c if c.starts_with("E_DEADLOCK") || c.starts_with("E_TIMEOUT") => ErrClass::Sync,
            c if c.starts_with("E_TDR") || c.starts_with("E_LOST") => ErrClass::Driver,
            _ => err.class,
        };
        if suggested != err.class {
            self.corrections.push((
                err.code.clone(),
                err.class.name(),
                suggested.name(),
            ));
        }
        let ok = self.center.admit(&err);
        if !ok {
            self.fallback.push((err.code.clone(), self.center.tick));
        }
        ok
    }

    /// 累计上抛数。
    pub fn raised(&self) -> u32 {
        self.raised
    }

    /// 静默审计转发。
    pub fn audit_silence(&self) -> (bool, String) {
        self.center.audit_silence(self.direct_writes)
    }
}

impl Default for RaisePipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0015 域自检（判据逐条映射见 `vea15_checks.rs`）。
pub fn run_vea15_checks() -> CheckSet {
    super::vea15_checks::run_vea15_checks()
}

// ---------------------------------------------------------------------------
// 六、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn err(code: &str, class: ErrClass) -> RenderError {
        RenderError::new(code, class, "测试错误", "测试根因补充", "测试下一步").unwrap()
    }

    #[test]
    fn vea15_triad_and_classification() {
        // 三要素齐备。
        let e = err("E_OOM_VRAM", ErrClass::Resource);
        assert!(!e.what.is_empty() && e.why.contains("水位") && e.next.contains("预算仲裁"));
        assert!(e.screen_text().contains("发生了什么") && e.screen_text().contains("下一步"));
        // 缺要素拒绝构造（码缺三要素→补齐的源头防线）。
        assert_eq!(RenderError::new("E_X", ErrClass::Sync, "", "y", "z").unwrap_err(), "E_TRIAD_INCOMPLETE");
        assert_eq!(RenderError::new("BAD", ErrClass::Sync, "w", "y", "z").unwrap_err(), "E_CODE_INVALID");
        // 四类归因表各有一档。
        for (c, cl) in [
            ("E_TDR_1", ErrClass::Driver),
            ("E_DEADLOCK_1", ErrClass::Sync),
            ("E_SUBMIT_1", ErrClass::Command),
            ("E_HANDLE_1", ErrClass::Resource),
        ] {
            let e = err(c, cl);
            assert!(!e.why.is_empty() && !e.next.is_empty(), "{} 归因非空", c);
        }
    }

    #[test]
    fn vea15_backpressure_storm_and_query() {
        let mut p = RaisePipeline::new();
        // 风暴：30 条同码错误打进小中心。
        for _ in 0..30 {
            assert!(p.raise(err("E_LOST_DEV", ErrClass::Driver)));
        }
        assert!(p.center.backpressure_absorbed >= 1, "风暴应被聚合吸收");
        assert!(p.center.entries.len() < 30, "聚合后条目应远小于 30");
        assert_eq!(p.center.entries.last().unwrap().aggregated >= 2, true);
        // 正查/反查双向检索。
        let hits = p.center.query_by_code("E_LOST_DEV");
        assert!(!hits.is_empty());
        let first = hits.first().unwrap();
        let detail = p.center.detail_of(&first.code, first.tick).unwrap();
        assert!(detail.contains("E_LOST_DEV") && detail.contains("下一步"));
        assert!(p.center.query_by_code("E_NOPE").is_empty());
    }

    #[test]
    fn vea15_correction_silence_audit_and_capacity() {
        let mut p = RaisePipeline::new();
        // 分类错误→修正：E_OOM 报成驱动类 → 建议资源类。
        p.raise(err("E_OOM_VRAM", ErrClass::Driver));
        assert_eq!(p.corrections.len(), 1);
        assert_eq!(p.corrections[0], ("E_OOM_VRAM".to_string(), "驱动", "资源"));
        // 分对则零修正。
        p.raise(err("E_OOM_HOST", ErrClass::Resource));
        assert_eq!(p.corrections.len(), 1);
        // 静默审计：直写即阻断级。
        let (ok0, _) = p.audit_silence();
        assert!(ok0);
        p.direct_writes = 2;
        let (ok1, msg) = p.audit_silence();
        assert!(!ok1 && msg.contains("阻断级") && msg.contains("2"));
        // 容量淘汰显性化：塞满 + 再入 → 淘汰计数增长且必达不丢。
        let mut big = RaisePipeline::new();
        for i in 0..LOG_CENTER_CAPACITY {
            let code = alloc::format!("E_OOM_{:04}", i);
            big.raise(RenderError::new(&code, ErrClass::Resource, "w", "y", "z").unwrap());
        }
        assert_eq!(big.center.entries.len(), LOG_CENTER_CAPACITY);
        assert!(big.raise(err("E_LOST_X", ErrClass::Driver)));
        assert_eq!(big.center.entries.len(), LOG_CENTER_CAPACITY, "满容量滚动保持");
        assert_eq!(big.center.dropped(), 1, "淘汰显性入账");
    }

    #[test]
    fn vea15_checks_all_green() {
        let set = run_vea15_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0015 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
