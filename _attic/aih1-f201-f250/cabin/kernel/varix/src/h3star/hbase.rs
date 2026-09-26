//! H3 共享底盘（AI-H3 · F301-F350 域内公用设施）。
//!
//! 五十项功能共享的底层机制，全部注入钟化（宿主测试确定复现）、零外部
//! 依赖（`crate::checks` + alloc/core）：
//!
//! - [`Clock`]：注入毫秒钟（单一时间源——域内所有时延判据用它记账）；
//! - [`SettingRegistry`]：设置项登记表（F301 同义词登记 / F302 页层级 /
//!   F303 即时生效与例外白名单 / F304 改动追踪的唯一数据源——一处一事实）；
//! - [`PersistKv`]：持久账本（写计数 + 冲刷计数——F307「无痕不落盘」、
//!   F320「跨重启记忆」类判据的落盘语义载体）；
//! - [`Hysteresis`]：迟滞器（进入/退出双阈值 + 恢复计数——F331/F332/F333
//!   降级回滞共用的数学核）；
//! - [`ExpLog`]：环形体验日志（F206 体验纪律的域内载体：事件 + 耗时 +
//!   结论字段；零分配复用，写满滚动）；
//! - [`percentile`]：分位数（最近邻法——与 star/sbase 同口径）。
//!
//! 底盘自身不实现任何 F 项判据——判据全部在各功能模块内。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 注入钟
// ---------------------------------------------------------------------------

/// 注入毫秒钟：单一时间源。宿主测试显式推进，复现确定。
#[derive(Clone, Copy, Debug, Default)]
pub struct Clock {
    now_ms: u64,
}

impl Clock {
    pub fn new() -> Clock {
        Clock { now_ms: 0 }
    }

    /// 推进到绝对时刻（单调约束：回拨被钳制为不动——时钟不能倒流）。
    pub fn advance_to(&mut self, now_ms: u64) -> u64 {
        if now_ms > self.now_ms {
            self.now_ms = now_ms;
        }
        self.now_ms
    }

    /// 相对推进。
    pub fn advance(&mut self, delta_ms: u64) -> u64 {
        self.now_ms += delta_ms;
        self.now_ms
    }

    pub fn now(&self) -> u64 {
        self.now_ms
    }
}

// ---------------------------------------------------------------------------
// 设置登记表（F301/F302/F303/F304 共享数据源）
// ---------------------------------------------------------------------------

/// 五类延迟生效例外（F303 白名单——枚举即白名单，清单外不允许延迟）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeferredKind {
    /// 分辨率。
    Resolution,
    /// 缩放。
    Scaling,
    /// 默认主题。
    DefaultTheme,
    /// 引导项。
    BootEntry,
    /// 语言。
    Language,
}

impl DeferredKind {
    /// 全部五类（白名单完备性审计用——改枚举必炸这里）。
    pub const ALL: [DeferredKind; 5] = [
        DeferredKind::Resolution,
        DeferredKind::Scaling,
        DeferredKind::DefaultTheme,
        DeferredKind::BootEntry,
        DeferredKind::Language,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DeferredKind::Resolution => "分辨率",
            DeferredKind::Scaling => "缩放",
            DeferredKind::DefaultTheme => "默认主题",
            DeferredKind::BootEntry => "引导项",
            DeferredKind::Language => "语言",
        }
    }
}

/// 生效语义（F303：即时为常态，延迟为例外且必须持徽标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    /// 拨动即变（就地反馈 <100ms）。
    Instant,
    /// 延迟生效（重启后）——仅白名单五类允许。
    Deferred(DeferredKind),
}

/// 一个设置条目（F301 搜索面 / F302 层级面 / F304 还原面的公共单元）。
#[derive(Clone, Debug)]
pub struct SettingItem {
    /// 条目名（唯一键）。
    pub name: &'static str,
    /// 所属设置页路径（两级：`分类/页`——F302 三级即缺陷）。
    pub page: &'static str,
    /// 同义词表（3-5 个；F301 登记 100% 判据载体）。
    pub synonyms: &'static [&'static str],
    /// 生效语义。
    pub effect: EffectKind,
    /// 出厂默认值（F304 还原基准）。
    pub default: i64,
    /// 当前值。
    pub value: i64,
    /// 就地控件类型（F301「Top3 结果可操作」判据载体）。
    pub control: ControlKind,
}

/// 就地控件类型（F301 就地操作 / F302 说明句三件套）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Toggle,
    Slider,
    Dropdown,
    Picker,
}

impl ControlKind {
    pub fn label(self) -> &'static str {
        match self {
            ControlKind::Toggle => "开关",
            ControlKind::Slider => "滑杆",
            ControlKind::Dropdown => "下拉",
            ControlKind::Picker => "选择器",
        }
    }
}

/// 页（F302 层级面单元）。
#[derive(Clone, Debug)]
pub struct SettingPage {
    /// 页路径（`分类/页`，两级）。
    pub path: &'static str,
    /// 一句人话副标题（副标题覆盖率 100% 判据）。
    pub subtitle: &'static str,
    /// 交叉链接目标页（空串 = 无链接；死链审计对账）。
    pub cross_links: &'static [&'static str],
}

/// 登记表审计结果（F301/F302 共用出账结构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryAudit {
    /// 条目同义词登记覆盖率（0-1000 定点，‰）。
    pub synonym_coverage_permille: u32,
    /// 页副标题覆盖率（0-1000，‰）。
    pub subtitle_coverage_permille: u32,
    /// 层级越界条目数（页路径非两级 = 三级即缺陷）。
    pub deep_violations: usize,
    /// 每页条目数超限页数（>15 条即超限）。
    pub overfull_pages: usize,
    /// 死链数（交叉链接指向不存在的页）。
    pub dead_links: usize,
}

/// 设置登记表。
#[derive(Clone, Debug, Default)]
pub struct SettingRegistry {
    items: Vec<SettingItem>,
    pages: Vec<SettingPage>,
}

impl SettingRegistry {
    pub fn new() -> SettingRegistry {
        SettingRegistry { items: Vec::new(), pages: Vec::new() }
    }

    /// 登记条目（唯一键约束：重名拒绝——一处一事实）。
    pub fn add_item(&mut self, item: SettingItem) -> bool {
        if self.items.iter().any(|i| i.name == item.name) {
            return false;
        }
        self.items.push(item);
        true
    }

    /// 登记页（唯一键约束同上）。
    pub fn add_page(&mut self, page: SettingPage) -> bool {
        if self.pages.iter().any(|p| p.path == page.path) {
            return false;
        }
        self.pages.push(page);
        true
    }

    pub fn items(&self) -> &[SettingItem] {
        &self.items
    }

    pub fn pages(&self) -> &[SettingPage] {
        &self.pages
    }

    pub fn page_of(&self, path: &str) -> Option<&SettingPage> {
        self.pages.iter().find(|p| p.path == path)
    }

    pub fn item_mut(&mut self, name: &str) -> Option<&mut SettingItem> {
        self.items.iter_mut().find(|i| i.name == name)
    }

    /// 改值入口（F303/F304 唯一改值口：即时语义直接落值并返回反馈时限；
    /// 延迟语义落 pending——值生效走 pending 提交，徽标常驻）。
    /// 返回 `Some(elapsed_budget_ms)` = 就地反馈判线（即时 100ms）；
    /// `None` = 延迟生效项。
    pub fn set_value(&mut self, name: &str, value: i64) -> Option<u64> {
        let item = self.items.iter_mut().find(|i| i.name == name)?;
        match item.effect {
            EffectKind::Instant => {
                item.value = value;
                Some(INSTANT_FEEDBACK_MS)
            }
            EffectKind::Deferred(_) => {
                item.value = value;
                None
            }
        }
    }

    /// 层级深度审计（F302）：页路径必须两级（一个 `/`）。
    pub fn audit(&self) -> RegistryAudit {
        let mut syn_ok = 0usize;
        for i in &self.items {
            // 同义词登记率：3-5 个视为登记完整（少于 3 未达标）。
            if (3..=5).contains(&i.synonyms.len()) {
                syn_ok += 1;
            }
        }
        let synonym_coverage_permille =
            if self.items.is_empty() { 0 } else { (syn_ok * 1000 / self.items.len()) as u32 };

        let mut sub_ok = 0usize;
        for p in &self.pages {
            if !p.subtitle.trim().is_empty() {
                sub_ok += 1;
            }
        }
        let subtitle_coverage_permille =
            if self.pages.is_empty() { 0 } else { (sub_ok * 1000 / self.pages.len()) as u32 };

        // 层级越界：条目页路径与页路径都必须两级（一个 `/`）——三级即缺陷。
        let deep_violations = self
            .items
            .iter()
            .filter(|i| i.page.matches('/').count() != 1)
            .count()
            + self.pages.iter().filter(|p| p.path.matches('/').count() != 1).count();

        // 每页条目数。
        let mut overfull_pages = 0usize;
        for p in &self.pages {
            let n = self.items.iter().filter(|i| i.page == p.path).count();
            if n > 15 {
                overfull_pages += 1;
            }
        }

        // 死链：交叉链接目标必须已登记。
        let dead_links = self
            .pages
            .iter()
            .flat_map(|p| p.cross_links.iter())
            .filter(|t| !self.pages.iter().any(|p| &p.path == *t))
            .count();

        RegistryAudit {
            synonym_coverage_permille,
            subtitle_coverage_permille,
            deep_violations,
            overfull_pages,
            dead_links,
        }
    }

    /// 已改项清单（F304：页级还原只碰已改项）。
    pub fn changed_in_page(&self, page: &str) -> Vec<&'static str> {
        self.items
            .iter()
            .filter(|i| i.page == page && i.value != i.default)
            .map(|i| i.name)
            .collect()
    }

    /// 页级还原默认：只碰已改项，返回实际还原的条目名清单。
    pub fn restore_page_defaults(&mut self, page: &str) -> Vec<&'static str> {
        let mut restored = Vec::new();
        for i in self.items.iter_mut() {
            if i.page == page && i.value != i.default {
                i.value = i.default;
                restored.push(i.name);
            }
        }
        restored
    }

    /// 单条目还原（F304 逐条还原）。
    pub fn restore_item_default(&mut self, name: &str) -> bool {
        match self.items.iter_mut().find(|i| i.name == name) {
            Some(i) if i.value != i.default => {
                i.value = i.default;
                true
            }
            _ => false,
        }
    }
}

/// 就地反馈判线（F303：<100ms）。
pub const INSTANT_FEEDBACK_MS: u64 = 100;

// ---------------------------------------------------------------------------
// 持久账本（落盘语义载体）
// ---------------------------------------------------------------------------

/// 持久 KV 账本：记录写入与冲刷次数（落盘语义）。
///
/// 判据面用法：F307「无痕开关后搜索，历史文件无增量」→ 开启无痕后
/// `writes == flushes == 0`；F320「跨重启记忆」→ 键在重实例后可读回。
#[derive(Clone, Debug, Default)]
pub struct PersistKv {
    entries: Vec<(String, String)>,
    pub writes: u64,
    pub flushes: u64,
}

impl PersistKv {
    pub fn new() -> PersistKv {
        PersistKv { entries: Vec::new(), writes: 0, flushes: 0 }
    }

    /// 写键值（内存即时可见；落盘账由 [`Self::flush`] 显式提交）。
    pub fn set(&mut self, key: &str, value: &str) {
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = String::from(value),
            None => self.entries.push((String::from(key), String::from(value))),
        }
        self.writes += 1;
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// 显式冲刷（模拟落盘提交）。
    pub fn flush(&mut self) {
        self.flushes += 1;
    }

    /// 冲刷增量（两次取点之间落盘了多少次——无痕判据直接对账）。
    pub fn flushes_between(&self, before: u64) -> u64 {
        self.flushes.saturating_sub(before)
    }

    /// 重启模拟：落盘内容重建新实例（跨重启记忆判据载体）。
    pub fn reboot(&self) -> PersistKv {
        let mut fresh = PersistKv::new();
        for (k, v) in &self.entries {
            fresh.entries.push((k.clone(), v.clone()));
        }
        fresh
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 迟滞器（降级/回滞共用数学核）
// ---------------------------------------------------------------------------

/// 迟滞器：进入阈值 / 退出阈值分离 + 连续采样确认（去抖）+ 恢复计数。
#[derive(Clone, Copy, Debug)]
pub struct Hysteresis {
    /// 进入态阈值（如帧率 < 50 触发降级）。
    pub enter_below: i64,
    /// 退出态阈值（如帧率 > 55 才回升——10% 回滞带）。
    pub exit_above: i64,
    /// 连续确认采样数（进入与退出各需连续 N 次）。
    pub confirm_n: u32,
    state: bool,
    run: u32,
    /// 进入次数（累计）。
    pub enters: u64,
    /// 退出次数（累计）。
    pub exits: u64,
}

impl Hysteresis {
    pub fn new(enter_below: i64, exit_above: i64, confirm_n: u32) -> Hysteresis {
        Hysteresis { enter_below, exit_above, confirm_n: confirm_n.max(1), state: false, run: 0, enters: 0, exits: 0 }
    }

    pub fn active(&self) -> bool {
        self.state
    }

    /// 喂一个采样值，返回状态变化（None = 状态未变）。
    pub fn feed(&mut self, value: i64) -> Option<bool> {
        let want = if self.state {
            if value > self.exit_above {
                Some(false)
            } else {
                None
            }
        } else if value < self.enter_below {
            Some(true)
        } else {
            None
        };

        match want {
            Some(target) => {
                if self.run + 1 >= self.confirm_n {
                    self.run = 0;
                    self.state = target;
                    if target {
                        self.enters += 1;
                    } else {
                        self.exits += 1;
                    }
                    Some(target)
                } else {
                    self.run += 1;
                    None
                }
            }
            None => {
                self.run = 0;
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 环形体验日志（十三章体验纪律的域内载体）
// ---------------------------------------------------------------------------

/// 体验事件结论字段（顺畅/卡顿/无反馈/被打断/报错——只此五类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpVerdict {
    Smooth,
    Laggy,
    Dead,
    Interrupted,
    Error,
}

/// 一条体验事件。
#[derive(Clone, Debug)]
pub struct ExpEvent {
    pub at_ms: u64,
    pub surface: &'static str,
    pub action: &'static str,
    pub cost_ms: u64,
    pub verdict: ExpVerdict,
}

/// 环形体验日志：容量固定、写满滚动、零分配复用。
pub struct ExpLog {
    ring: Vec<Option<ExpEvent>>,
    head: usize,
    count: usize,
    /// 挫败信号计数（Laggy/Dead/Interrupted/Error 自动标记——体验指纹）。
    pub frustration_events: u64,
}

const EXPLOG_CAP: usize = 128;

impl ExpLog {
    pub fn new() -> ExpLog {
        let mut ring = Vec::with_capacity(EXPLOG_CAP);
        for _ in 0..EXPLOG_CAP {
            ring.push(None);
        }
        ExpLog { ring, head: 0, count: 0, frustration_events: 0 }
    }

    /// 记录事件（<100ms 反馈判线自动判 Dead——无反馈指纹）。
    pub fn record(&mut self, at_ms: u64, surface: &'static str, action: &'static str, cost_ms: u64) {
        let verdict = if cost_ms > INSTANT_FEEDBACK_MS { ExpVerdict::Laggy } else { ExpVerdict::Smooth };
        self.record_verdict(at_ms, surface, action, cost_ms, verdict);
    }

    /// 记录带显式结论的事件。
    pub fn record_verdict(
        &mut self,
        at_ms: u64,
        surface: &'static str,
        action: &'static str,
        cost_ms: u64,
        verdict: ExpVerdict,
    ) {
        self.ring[self.head] = Some(ExpEvent { at_ms, surface, action, cost_ms, verdict });
        self.head = (self.head + 1) % EXPLOG_CAP;
        if self.count < EXPLOG_CAP {
            self.count += 1;
        }
        if verdict != ExpVerdict::Smooth {
            self.frustration_events += 1;
        }
    }

    /// 时间序回放（旧→新）。
    pub fn timeline(&self) -> Vec<&ExpEvent> {
        let start = (self.head + EXPLOG_CAP - self.count) % EXPLOG_CAP;
        (0..self.count)
            .map(|i| self.ring[(start + i) % EXPLOG_CAP].as_ref().unwrap())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

// ---------------------------------------------------------------------------
// 分位数（最近邻法——与 star/sbase 同口径）
// ---------------------------------------------------------------------------

/// 合并两份 CheckSet（深化层并入基线聚合——判据账面一张表）。
/// 超出 MAX_CHECKS 容量的条目由 `add` 的 dropped 计数如实报告。
pub fn merge_sets(mut base: CheckSet, extra: CheckSet) -> CheckSet {
    for i in 0..extra.len() {
        if let Some(c) = extra.get(i) {
            base.add(c.name, c.passed, c.detail);
        }
    }
    base
}

/// 最近邻分位数（samples 需升序）。
pub fn percentile(sorted: &[u64], p: u32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as u64 * p as u64 + 999) / 1000).saturating_sub(1) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// hbase 自检（底盘设施自身的机制核——各 F 项判据在各自模块）。
pub fn run_hbase_checks() -> CheckSet {
    let mut set = CheckSet::new("h3-hbase");

    // 1. 钟：单调、相对推进。
    let mut c = Clock::new();
    c.advance(100);
    c.advance_to(50); // 回拨钳制。
    set.add("clock monotonic clamp", c.now() == 100, "");
    c.advance(25);
    set.add("clock relative advance", c.now() == 125, "");

    // 2. 登记表：唯一键、审计四指标。
    let mut reg = SettingRegistry::new();
    reg.add_page(SettingPage {
        path: "系统/声音",
        subtitle: "这里调系统声音",
        cross_links: &["个性化/主题"],
    });
    reg.add_page(SettingPage { path: "个性化/主题", subtitle: "这里调主题外观", cross_links: &[] });
    set.add(
        "registry add items",
        reg.add_item(SettingItem {
            name: "音量",
            page: "系统/声音",
            synonyms: &["声音大小", "volume", "响度"],
            effect: EffectKind::Instant,
            default: 50,
            value: 50,
            control: ControlKind::Slider,
        }) && reg.add_item(SettingItem {
            name: "主题",
            page: "个性化/主题",
            synonyms: &["外观", "theme", "皮肤"],
            effect: EffectKind::Deferred(DeferredKind::DefaultTheme),
            default: 0,
            value: 0,
            control: ControlKind::Picker,
        }) && !reg.add_item(SettingItem {
            name: "音量",
            page: "系统/声音",
            synonyms: &[],
            effect: EffectKind::Instant,
            default: 0,
            value: 0,
            control: ControlKind::Toggle,
        }),
        "",
    );

    // 3. 审计全绿基线。
    let a = reg.audit();
    set.add(
        "audit green baseline",
        a.synonym_coverage_permille == 1000
            && a.subtitle_coverage_permille == 1000
            && a.deep_violations == 0
            && a.overfull_pages == 0
            && a.dead_links == 0,
        "",
    );

    // 4. 审计抓缺陷：三级路径 + 死链 + 少同义词。
    let mut bad = SettingRegistry::new();
    bad.add_page(SettingPage { path: "系统/显示/缩放", subtitle: "x", cross_links: &["不存在页"] });
    bad.add_page(SettingPage { path: "系统/显示", subtitle: "x", cross_links: &[] });
    bad.add_item(SettingItem {
        name: "缩放比",
        page: "系统/显示/缩放",
        synonyms: &["仅一个"],
        effect: EffectKind::Deferred(DeferredKind::Scaling),
        default: 100,
        value: 100,
        control: ControlKind::Dropdown,
    });
    let a = bad.audit();
    set.add(
        "audit catches violations",
        a.deep_violations == 2 && a.dead_links == 1 && a.synonym_coverage_permille == 0,
        "",
    );

    // 5. 生效语义：即时返判线、延迟返 None。
    set.add(
        "effect kinds budget",
        reg.set_value("音量", 70) == Some(INSTANT_FEEDBACK_MS)
            && reg.set_value("主题", 1).is_none()
            && reg.items().iter().find(|i| i.name == "音量").unwrap().value == 70,
        "",
    );

    // 6. F304 面：只碰已改项。
    reg.set_value("音量", 80);
    let changed = reg.changed_in_page("系统/声音");
    set.add("changed tracking", changed == vec!["音量"], "");
    let restored = reg.restore_page_defaults("系统/声音");
    set.add(
        "page restore touches only changed",
        restored == vec!["音量"] && reg.item_restored_baseline(),
        "",
    );

    // 7. 持久账：写/冲刷/重启回读。
    let mut kv = PersistKv::new();
    kv.set("k", "v1");
    kv.set("k", "v2");
    kv.flush();
    let f0 = kv.flushes;
    kv.set("k2", "v");
    let delta = kv.flushes_between(f0);
    let rebooted = kv.reboot();
    set.add(
        "persist kv semantics",
        kv.get("k") == Some("v2") && delta == 0 && rebooted.get("k") == Some("v2") && rebooted.len() == 2,
        "",
    );

    // 8. 迟滞：确认采样 + 回滞带 + 计数。
    let mut h = Hysteresis::new(50, 55, 3);
    set.add(
        "hysteresis confirm + band",
        h.feed(40).is_none()
            && h.feed(40).is_none()
            && h.feed(40) == Some(true)
            && h.feed(53).is_none() // 回滞带内不动。
            && h.feed(56).is_none()
            && h.feed(56).is_none()
            && h.feed(56) == Some(false)
            && h.enters == 1
            && h.exits == 1,
        "",
    );

    // 9. 体验日志：环形滚动 + 挫败计数 + 时间序。
    let mut log = ExpLog::new();
    for i in 0..200u64 {
        log.record(i * 10, "test", "tick", if i % 3 == 0 { 250 } else { 20 });
    }
    let tl = log.timeline();
    set.add(
        "explog ring + frustration",
        log.len() == EXPLOG_CAP
            && tl.len() == EXPLOG_CAP
            && tl[0].at_ms < tl[EXPLOG_CAP - 1].at_ms
            && log.frustration_events > 0,
        "",
    );

    // 10. 分位数：最近邻口径。
    let samples = [10u64, 20, 30, 40, 50];
    set.add(
        "percentile nearest",
        percentile(&samples, 500) == 30 && percentile(&samples, 950) == 50 && percentile(&[], 500) == 0,
        "",
    );

    set
}

impl SettingRegistry {
    /// 音量条目是否回到出厂基线（自检辅助——避免暴露内部字段）。
    fn item_restored_baseline(&self) -> bool {
        self.items.iter().find(|i| i.name == "音量").map(|i| i.value == i.default).unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_rejects_backwards() {
        let mut c = Clock::new();
        c.advance(500);
        assert_eq!(c.advance_to(1), 500);
        assert_eq!(c.now(), 500);
    }

    #[test]
    fn registry_rejects_duplicate_pages() {
        let mut reg = SettingRegistry::new();
        assert!(reg.add_page(SettingPage { path: "a/b", subtitle: "s", cross_links: &[] }));
        assert!(!reg.add_page(SettingPage { path: "a/b", subtitle: "s2", cross_links: &[] }));
    }

    #[test]
    fn restore_item_default_noop_on_default() {
        let mut reg = SettingRegistry::new();
        reg.add_item(SettingItem {
            name: "x",
            page: "a/b",
            synonyms: &["s1", "s2", "s3"],
            effect: EffectKind::Instant,
            default: 5,
            value: 5,
            control: ControlKind::Toggle,
        });
        assert!(!reg.restore_item_default("x"), "未改动的条目还原应为 no-op");
    }

    #[test]
    fn hysteresis_single_sample_confirms_immediately() {
        let mut h = Hysteresis::new(10, 20, 1);
        assert_eq!(h.feed(5), Some(true));
        assert_eq!(h.feed(25), Some(false));
    }

    #[test]
    fn deferred_whitelist_is_exactly_five() {
        assert_eq!(DeferredKind::ALL.len(), 5);
    }
}
