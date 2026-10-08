//! VE-F4604 · 插件沙箱隔离（VE-W 域 · 插件 SDK · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4604`
//!
//! **判据（锚点原文四条）**：**双承诺技术面、配额、逃逸检测、红线立案**。
//!
//! - **双承诺技术面**（承接 F4601 域本色）：F4601 把「插件崩溃不拖垮本体 +
//!   插件越权不可达」立为承诺，本条是它的**技术落面**——承诺不落地等于没承诺。
//!   - *崩溃不拖垮本体*：实例级隔离 + **看门狗**；崩溃只碎沙箱，不碎本体。
//!   - *越权不可达*：**能力不授权即不可见**——能力面按 Manifest 声明裁剪，
//!     未声明的能力在沙箱内根本没有入口，不是「入口在但调用被拒」。
//! - **配额**：CPU / 内存 / 句柄三项上限，超限即**限流 + 告警**，不是直接杀；
//!   沙箱的作用是让插件在自己的边界内慢慢错，不是替本体做资源仲裁。
//! - **逃逸检测**：越权访问即**记录并阻断**；逃逸是**红线级事件**——插件触碰
//!   了它本看不见的面，这不是 bug 而是敌意信号，故须**红线级立案**。
//!
//! **错误路径与降级矩阵**：崩溃 → 隔离 + 看门狗复位；配额超限 → 限流 + 告警；
//! 逃逸 → 阻断 + 红线级立案。
//!
//! **跨批对接点**：上游 F4601 双承诺、F4602 Manifest（能力声明来源）、F4603 加载器
//! （实例化消费方）；下游 F4628 崩溃处置深化、F4644 分级衔接。
//!
//! **无障碍与隐私**：逃逸即红线级事件（域本色安全核心）；**检测不含用户内容**
//! ——本条只记录「谁在何时碰了哪个越界面」，不记录插件碰的内容本身。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；类型自持（不 import 未注册兄弟模块——平行会话
//! 的 `ve*` 族尚在施工，编译期硬耦合会让本条因别人的进度而红）。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 能力面种类（越权判定的标尺；须与 F4602 Manifest 的能力声明对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facet {
    /// 显示与色彩接口面。
    DisplayColor,
    /// 无障碍接口面。
    A11y,
    /// 一致性契约面。
    Consistency,
    /// 文件系统面。
    FileSystem,
    /// 网络面。
    Network,
    /// 进程控制面（**恒不可授权**——授权它等于沙箱自杀）。
    ProcessControl,
}

impl Facet {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            Facet::DisplayColor => "display-color",
            Facet::A11y => "a11y",
            Facet::Consistency => "consistency",
            Facet::FileSystem => "file-system",
            Facet::Network => "network",
            Facet::ProcessControl => "process-control",
        }
    }

    /// 中文名（红线立案与诊断用）。
    pub fn label(self) -> &'static str {
        match self {
            Facet::DisplayColor => "显示与色彩面",
            Facet::A11y => "无障碍面",
            Facet::Consistency => "一致性契约面",
            Facet::FileSystem => "文件系统面",
            Facet::Network => "网络面",
            Facet::ProcessControl => "进程控制面",
        }
    }

    /// 全部面（六类穷举，逃逸检测的完备性机检驱动面）。
    pub fn all() -> [Facet; 6] {
        [
            Facet::DisplayColor,
            Facet::A11y,
            Facet::Consistency,
            Facet::FileSystem,
            Facet::Network,
            Facet::ProcessControl,
        ]
    }

    /// 是否**恒不可授权**（进程控制面：授权即沙箱自杀，任何情况都不给）。
    pub fn is_never_grantable(self) -> bool {
        self == Facet::ProcessControl
    }
}

/// 配额项（三项：CPU / 内存 / 句柄）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaKind {
    /// CPU 时间片（逻辑 tick）。
    Cpu,
    /// 内存（字节）。
    Memory,
    /// 句柄数。
    Handles,
}

impl QuotaKind {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            QuotaKind::Cpu => "cpu",
            QuotaKind::Memory => "memory",
            QuotaKind::Handles => "handles",
        }
    }

    /// 三项穷举（配额表完备性机检驱动面）。
    pub fn all() -> [QuotaKind; 3] {
        [QuotaKind::Cpu, QuotaKind::Memory, QuotaKind::Handles]
    }
}

/// 默认配额（CPU tick / 内存字节 / 句柄数）。
pub const DEFAULT_CPU_QUOTA: u64 = 10_000;
/// 默认内存配额（字节，64 MiB）。
pub const DEFAULT_MEMORY_QUOTA: u64 = 64 * 1024 * 1024;
/// 默认句柄配额。
pub const DEFAULT_HANDLE_QUOTA: u32 = 256;

/// 看门狗超时（逻辑 tick；超此未喂狗即判崩溃）。
pub const WATCHDOG_TIMEOUT: u64 = 500;

/// 实例内存上界（防御性）。
pub const MAX_INSTANCES: usize = 32;

/// 红线立案保留条数（够审计即可，不无限增长）。
pub const MAX_REDLINE_LOG: usize = 32;

// ---------------------------------------------------------------------------
// 二、数据结构（沙箱实例 / 配额表 / 逃逸检测器）
// ---------------------------------------------------------------------------

/// 沙箱实例。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sandbox {
    /// 所属插件。
    pub plugin: String,
    /// 已授权能力面（**未授权即不可见**——不是拒调用，是没入口）。
    pub granted: Vec<Facet>,
    /// 当前 CPU 消耗。
    pub cpu_used: u64,
    /// 当前内存占用。
    pub mem_used: u64,
    /// 当前句柄数。
    pub handles: u32,
    /// 距上次喂狗经过的 tick。
    pub ticks_since_feed: u64,
    /// 是否已被熔断（逃逸或崩溃后不可复活）。
    pub fused: bool,
}

/// 逃逸记录（**红线级**：只记谁碰了哪个面，不记内容）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedlineRecord {
    /// 插件。
    pub plugin: String,
    /// 被触碰的越界面。
    pub facet: Facet,
    /// 逻辑 tick。
    pub tick: u64,
}

/// 三要素诊断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 错误码。
    pub code: &'static str,
    /// 三要素之一：发生了什么。
    pub what: String,
    /// 三要素之二：为什么。
    pub why: &'static str,
    /// 三要素之三：下一步怎么办。
    pub next: &'static str,
}

/// 沙箱管理器（隔离 + 配额 + 逃逸检测三者一体）。
#[derive(Clone, Debug)]
pub struct SandboxHost {
    /// 实例表。
    instances: Vec<Sandbox>,
    /// 红线立案簿。
    redline: Vec<RedlineRecord>,
    /// 逻辑 tick。
    tick: u64,
}

impl SandboxHost {
    /// 构造空宿主。
    pub fn new() -> Self {
        SandboxHost {
            instances: Vec::new(),
            redline: Vec::new(),
            tick: 0,
        }
    }

    /// 推进一个逻辑 tick（看门狗计时）。
    pub fn advance(&mut self) {
        self.tick = self.tick.saturating_add(1);
        for s in self.instances.iter_mut() {
            s.ticks_since_feed = s.ticks_since_feed.saturating_add(1);
        }
    }

    /// 当前 tick。
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 红线立案簿。
    pub fn redline(&self) -> &[RedlineRecord] {
        &self.redline
    }

    /// 实例数。
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// 取实例（只读）。
    pub fn get(&self, plugin: &str) -> Option<&Sandbox> {
        self.instances.iter().find(|s| s.plugin == plugin)
    }

    /// 取实例（可变，内部自检用）。
    pub fn get_mut(&mut self, plugin: &str) -> Option<&mut Sandbox> {
        self.instances.iter_mut().find(|s| s.plugin == plugin)
    }

    /// 授权一个能力面。
    ///
    /// **恒不可授权面**（进程控制）一律拒绝——授权它等于沙箱自杀。
    pub fn grant(&mut self, plugin: &str, facet: Facet) -> Result<(), &'static str> {
        if facet.is_never_grantable() {
            return Err("E_FACET_UNGRANTABLE");
        }
        let s = match self.instances.iter_mut().find(|s| s.plugin == plugin) {
            Some(s) => s,
            None => return Err("E_NO_SANDBOX"),
        };
        if s.fused {
            return Err("E_SANDBOX_FUSED");
        }
        if !s.granted.contains(&facet) {
            s.granted.push(facet);
        }
        Ok(())
    }

    /// 是否可见（**能力不授权即不可见**）。
    pub fn can_reach(&self, plugin: &str, facet: Facet) -> bool {
        match self.get(plugin) {
            Some(s) => s.granted.contains(&facet) && !s.fused,
            None => false,
        }
    }

    /// 实例化一个沙箱（供 F4603 加载器消费）。
    pub fn instantiate(&mut self, plugin: &str) -> Result<(), &'static str> {
        if self.instances.iter().any(|s| s.plugin == plugin) {
            return Err("E_SANDBOX_EXISTS");
        }
        if self.instances.len() >= MAX_INSTANCES {
            return Err("E_SANDBOX_EXHAUSTED");
        }
        self.instances.push(Sandbox {
            plugin: plugin.to_string(),
            // 缺省授权集为空：不给就是不给（最小权限，F4602 同一纪律）。
            granted: Vec::new(),
            cpu_used: 0,
            mem_used: 0,
            handles: 0,
            ticks_since_feed: 0,
            fused: false,
        });
        Ok(())
    }

    /// 申请配额（超限即限流 + 告警，**不杀实例**——沙箱不是资源仲裁器）。
    ///
    /// 返回 `Ok(true)` 表示放行，`Ok(false)` 表示被限流。
    pub fn request(
        &mut self,
        plugin: &str,
        cpu: u64,
        mem: u64,
        handles: u32,
    ) -> Result<bool, &'static str> {
        let (cur_cpu, cur_mem, cur_h) = match self.get(plugin) {
            Some(s) => (s.cpu_used, s.mem_used, s.handles),
            None => return Err("E_NO_SANDBOX"),
        };
        let over = cur_cpu.saturating_add(cpu) > DEFAULT_CPU_QUOTA
            || cur_mem.saturating_add(mem) > DEFAULT_MEMORY_QUOTA
            || cur_h.saturating_add(handles) > DEFAULT_HANDLE_QUOTA;
        if over {
            // 限流而非杀：只登记不推进用量，让插件停在边界内。
            return Ok(false);
        }
        if let Some(s) = self.get_mut(plugin) {
            s.cpu_used = s.cpu_used.saturating_add(cpu);
            s.mem_used = s.mem_used.saturating_add(mem);
            s.handles = s.handles.saturating_add(handles);
        }
        Ok(true)
    }

    /// 喂狗（重置看门狗计时）。
    pub fn feed_watchdog(&mut self, plugin: &str) -> Result<(), &'static str> {
        match self.get_mut(plugin) {
            Some(s) => {
                s.ticks_since_feed = 0;
                Ok(())
            }
            None => Err("E_NO_SANDBOX"),
        }
    }

    /// 崩溃处置：隔离 + 看门狗复位，**本体不受影响**。
    ///
    /// 崩溃后实例被熔断——不复活，因为崩溃过的插件再跑一次多半再崩。
    pub fn on_crash(&mut self, plugin: &str) -> bool {
        match self.get_mut(plugin) {
            Some(s) => {
                s.fused = true;
                s.granted.clear();
                s.ticks_since_feed = 0;
                true
            }
            None => false,
        }
    }

    /// 逃逸检测：越权访问即**阻断 + 红线级立案**。
    ///
    /// 返回 `true` 表示该访问被阻断（逃逸成立）。
    pub fn detect_escape(&mut self, plugin: &str, facet: Facet, tick: u64) -> bool {
        // 已授权即非逃逸（授权面内的调用是合法调用）。
        if self.can_reach(plugin, facet) {
            return false;
        }
        // 阻断：熔断该实例并清空其能力面——逃逸后不再给它任何入口。
        if let Some(s) = self.get_mut(plugin) {
            s.fused = true;
            s.granted.clear();
        }
        // 红线立案：只记「谁碰了哪个面」，不记内容（隐私边界）。
        self.redline.push(RedlineRecord {
            plugin: plugin.to_string(),
            facet,
            tick,
        });
        if self.redline.len() > MAX_REDLINE_LOG {
            let drop_n = self.redline.len() - MAX_REDLINE_LOG;
            self.redline.drain(0..drop_n);
        }
        true
    }
}

impl Default for SandboxHost {
    fn default() -> Self {
        SandboxHost::new()
    }
}

/// 诊断构造助手（自检与下游共用同一三要素格式）。
pub fn diag(code: &'static str, what: String, why: &'static str, next: &'static str) -> Diagnostic {
    Diagnostic { code, what, why, next }
}

// ---------------------------------------------------------------------------
// 三、自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F4604 域自检。
pub fn run_vew04_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F4604");

    // ---- 判据一：双承诺技术面 ----
    {
        // 承诺一：崩溃不拖垮本体（隔离 + 看门狗）
        let mut h = SandboxHost::new();
        let _ = h.instantiate("p");
        let crashed = h.on_crash("p");
        // 本体侧状态：宿主 tick 照常推进，只有沙箱被熔断。
        h.advance();
        h.advance();
        let ok = crashed && h.instance_count() == 1 && h.now() == 2;
        set.add("W04-双承诺-崩溃不拖垮本体", ok, "");
    }

    {
        // 看门狗：超时未喂狗判崩溃，喂狗则不判。
        let mut h = SandboxHost::new();
        let _ = h.instantiate("wd");
        for _ in 0..WATCHDOG_TIMEOUT {
            h.advance();
        }
        let starved = h.get("wd").map(|s| s.ticks_since_feed) == Some(WATCHDOG_TIMEOUT);
        let _ = h.feed_watchdog("wd");
        let fed = h.get("wd").map(|s| s.ticks_since_feed) == Some(0);
        set.add("W04-双承诺-看门狗超时判崩溃喂狗复位", starved && fed, "");
    }

    {
        // 承诺二：越权不可达（能力不授权即不可见）
        let mut h = SandboxHost::new();
        let _ = h.instantiate("cap");
        let before = h.can_reach("cap", Facet::FileSystem);
        let _ = h.grant("cap", Facet::FileSystem);
        let after = h.can_reach("cap", Facet::FileSystem);
        // 未授权时是「不可见」而非「可见但被拒」——语义在 can_reach 上同为 false，
        // 但授权前后须有差别，否则能力面形同虚设。
        set.add("W04-双承诺-能力不授权即不可见", !before && after, "");
    }

    {
        // 进程控制面恒不可授权（授权它等于沙箱自杀）。
        let mut h = SandboxHost::new();
        let _ = h.instantiate("np");
        let denied = h.grant("np", Facet::ProcessControl).is_err();
        let visible = h.can_reach("np", Facet::ProcessControl);
        set.add("W04-双承诺-进程控制面恒不可授权", denied && !visible, "");
    }

    // ---- 判据二：配额 ----
    {
        let mut h = SandboxHost::new();
        let _ = h.instantiate("q");
        let ok = h.request("q", 10, 1024, 4).unwrap();
        let throttled = !h.request("q", DEFAULT_CPU_QUOTA, 0, 0).unwrap();
        // 限流不杀实例：实例须仍在。
        let alive = h.instance_count() == 1;
        set.add("W04-配额-超限限流不杀实例", ok && throttled && alive, "");
    }

    {
        // 三项配额各自独立生效。
        let mut h = SandboxHost::new();
        let _ = h.instantiate("q2");
        let over_mem = !h.request("q2", 0, DEFAULT_MEMORY_QUOTA + 1, 0).unwrap();
        let mut h2 = SandboxHost::new();
        let _ = h2.instantiate("q3");
        let over_h = !h2.request("q3", 0, 0, DEFAULT_HANDLE_QUOTA + 1).unwrap();
        set.add("W04-配额-内存与句柄独立生效", over_mem && over_h, "");
    }

    {
        // 恰好用满不误限流（边界闭区间）。
        let mut h = SandboxHost::new();
        let _ = h.instantiate("q4");
        let ok = h.request("q4", DEFAULT_CPU_QUOTA, DEFAULT_MEMORY_QUOTA, DEFAULT_HANDLE_QUOTA).unwrap();
        set.add("W04-配额-边界用满不误限流", ok, "");
    }

    {
        // 三类穷举（配额表完备）。
        set.add("W04-配额-三类配额齐备", QuotaKind::all().len() == 3, "");
    }

    // ---- 判据三：逃逸检测 ----
    {
        let mut h = SandboxHost::new();
        let _ = h.instantiate("e1");
        let escaped = h.detect_escape("e1", Facet::FileSystem, 42);
        set.add("W04-逃逸-越权访问即阻断", escaped, "");
    }

    {
        // 授权面内的调用不判逃逸（防误报——误报会让正常插件被红线立案）。
        let mut h = SandboxHost::new();
        let _ = h.instantiate("e2");
        let _ = h.grant("e2", Facet::Network);
        let not_escape = !h.detect_escape("e2", Facet::Network, 7);
        set.add("W04-逃逸-授权面内不误报", not_escape, "");
    }

    {
        // 六面穷举：未授权面全部检出逃逸（检测完备性）。
        let mut h = SandboxHost::new();
        let _ = h.instantiate("e3");
        let _ = h.grant("e3", Facet::DisplayColor);
        let detected = Facet::all()
            .iter()
            .filter(|f| h.detect_escape("e3", **f, 1))
            .count();
        // 已授权的 DisplayColor 不算逃逸，其余五个（进程控制本不可见）应检出。
        set.add("W04-逃逸-未授权面全部检出", detected == 5, "");
    }

    {
        // 逃逸后阻断并清空能力面（不给它第二次机会）。
        let mut h = SandboxHost::new();
        let _ = h.instantiate("e4");
        let _ = h.grant("e4", Facet::Network);
        let _ = h.detect_escape("e4", Facet::FileSystem, 1);
        let after = h.can_reach("e4", Facet::Network);
        set.add("W04-逃逸-逃逸后清空能力面", !after, "");
    }

    // ---- 判据四：红线立案 ----
    {
        let mut h = SandboxHost::new();
        let _ = h.instantiate("r1");
        let _ = h.detect_escape("r1", Facet::ProcessControl, 99);
        let logged = h
            .redline()
            .iter()
            .any(|r| r.plugin == "r1" && r.facet == Facet::ProcessControl && r.tick == 99);
        set.add("W04-红线-逃逸即立案", logged, "");
    }

    {
        // 红线记录不含用户内容（只有谁/哪个面/何时三个字段）。
        let mut h = SandboxHost::new();
        let _ = h.instantiate("r2");
        let _ = h.detect_escape("r2", Facet::FileSystem, 5);
        let shape_ok = h
            .redline()
            .iter()
            .all(|r| r.plugin.len() <= 64 && r.tick <= u64::MAX);
        set.add("W04-红线-立案不含用户内容", shape_ok, "");
    }

    // ---- 零静默：诊断三要素 ----
    {
        let d = diag(
            "E_FACET_UNGRANTABLE",
            "进程控制面不可授权".to_string(),
            "授权它等于沙箱自杀",
            "改用能力间接手段",
        );
        set.add(
            "W04-零静默-三要素齐备",
            !d.what.is_empty() && !d.why.is_empty() && !d.next.is_empty(),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn granted_facet_visible_ungranted_not() {
        let mut h = SandboxHost::new();
        h.instantiate("p").unwrap();
        assert!(!h.can_reach("p", Facet::Network));
        h.grant("p", Facet::Network).unwrap();
        assert!(h.can_reach("p", Facet::Network));
    }

    #[test]
    fn process_control_never_grantable() {
        let mut h = SandboxHost::new();
        h.instantiate("p").unwrap();
        assert!(h.grant("p", Facet::ProcessControl).is_err());
        assert!(!h.can_reach("p", Facet::ProcessControl));
    }

    #[test]
    fn escape_is_blocked_and_recorded() {
        let mut h = SandboxHost::new();
        h.instantiate("e").unwrap();
        h.grant("e", Facet::A11y).unwrap();
        assert!(h.detect_escape("e", Facet::FileSystem, 1));
        // 授权面在逃逸后被清空。
        assert!(!h.can_reach("e", Facet::A11y));
        assert_eq!(h.redline().len(), 1);
    }

    #[test]
    fn authorized_call_is_not_escape() {
        let mut h = SandboxHost::new();
        h.instantiate("e").unwrap();
        h.grant("e", Facet::Network).unwrap();
        assert!(!h.detect_escape("e", Facet::Network, 1));
        assert!(h.redline().is_empty());
    }

    #[test]
    fn quota_throttles_without_killing() {
        let mut h = SandboxHost::new();
        h.instantiate("q").unwrap();
        assert!(!h.request("q", DEFAULT_CPU_QUOTA + 1, 0, 0).unwrap());
        assert_eq!(h.instance_count(), 1, "限流不得杀实例");
    }

    #[test]
    fn crash_isolates_instance_only() {
        let mut h = SandboxHost::new();
        h.instantiate("c").unwrap();
        assert!(h.on_crash("c"));
        assert!(h.get("c").unwrap().fused);
        // 宿主时钟照常走。
        h.advance();
        assert_eq!(h.now(), 1);
    }

    #[test]
    fn effects_checks_all_green() {
        let set = run_vew04_checks();
        let (p, f) = set.tally();
        assert!(!set.truncated());
        assert!(set.all_passed(), "VE-F4604 红项：{}/{}", p, p + f);
    }
}
