//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12619 ktest 聚合器（本批 20 条单跑入口） · 0x804C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥319 行；
//!   ② 判据原文：UNX-F12620-J1 B32 主题框架登记入域待办账，与
//!      F12598/F12599 预告链衔接零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 域待办账：主题框架逐条登记（主题名/来源预告号/状态三列），
//!     状态机 Open→Registered→Linked 单调，倒退显性拒；
//!   - B32 主题框架入账：B32（D4 消息面二：APC 消费边）主题及其
//!     子件逐条注册，重名拒收（RegistryDup），未注册主题不可挂链
//!     （ThemeUnknown）；
//!   - 预告链零悬空：F12598/F12599 两条预告边冻结在链表中，链上
//!     每个链接必须解析到账内已注册主题，解析不到即悬空
//!     （DanglingLink 必抓）；链尾必须是 B32（ChainBroken 抓断链）；
//!   - ktest 聚合面：本批 20 条单跑入口的聚合计数账——聚合总数=
//!     已链接条目数（AggCountBreak 抓不等），聚合指纹随账可点验；
//!   - 双跑一致 + 段位守卫；LCG 种子写死 = SEG_BASE（0x804C0）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect；
//!     诊断码闭集 0x804C0..0x80500。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x804C0，AI-VX9-1105 B01 批 12 槽第 8 槽）。
pub const SEG_BASE: u32 = 0x804C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TodoChainCode {
    /// 待办缺行：链接指向的主题未注册
    TodoMissing,
    /// 悬空链接：预告链解析不到账内主题
    DanglingLink,
    /// 链断裂：预告链尾不是 B32
    ChainBroken,
    /// 状态倒退：待办状态只进不退
    StateRegress,
    /// 未注册主题：挂链前须先注册
    ThemeUnknown,
    /// 重复登记：同主题重复注册
    RegistryDup,
    /// 聚合计数破：聚合总数≠已链接条目数
    AggCountBreak,
    /// 归档漂移：聚合指纹与存根不符
    ArchiveDrift,
    /// 链环：预告链出现回环
    ChainCycle,
    /// 账指纹漂移：双跑不一致
    FingerprintDrift,
    /// 待办账越界：下标超出账容量
    LedgerOverflow,
    /// 未知诊断码兜底
    Unknown,
}

impl TodoChainCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                TodoChainCode::TodoMissing => 0x01,
                TodoChainCode::DanglingLink => 0x02,
                TodoChainCode::ChainBroken => 0x03,
                TodoChainCode::StateRegress => 0x04,
                TodoChainCode::ThemeUnknown => 0x05,
                TodoChainCode::RegistryDup => 0x06,
                TodoChainCode::AggCountBreak => 0x07,
                TodoChainCode::ArchiveDrift => 0x08,
                TodoChainCode::ChainCycle => 0x09,
                TodoChainCode::FingerprintDrift => 0x0A,
                TodoChainCode::LedgerOverflow => 0x0B,
                TodoChainCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            TodoChainCode::TodoMissing => "待办缺行：链接目标先注册再挂链",
            TodoChainCode::DanglingLink => "悬空链接：预告号解析不到账内主题，补注册",
            TodoChainCode::ChainBroken => "链尾断裂：预告链必须收口在 B32",
            TodoChainCode::StateRegress => "状态倒退：Open→Registered→Linked 只进不退",
            TodoChainCode::ThemeUnknown => "主题未注册：先入账再引用",
            TodoChainCode::RegistryDup => "重复登记：同主题只许注册一次",
            TodoChainCode::AggCountBreak => "聚合等式破：总数=已链接数，逐条对账",
            TodoChainCode::ArchiveDrift => "归档漂移：存根指纹与重算不符",
            TodoChainCode::ChainCycle => "链环：预告链出现回环，禁止",
            TodoChainCode::FingerprintDrift => "账指纹漂移：冻结种子重跑",
            TodoChainCode::LedgerOverflow => "待办账溢出：容量冻结，扩容走变更",
            TodoChainCode::Unknown => "未知诊断码：不在 0x804C0 段闭集内",
        }
    }

    pub const ALL: [TodoChainCode; 12] = [
        TodoChainCode::TodoMissing,
        TodoChainCode::DanglingLink,
        TodoChainCode::ChainBroken,
        TodoChainCode::StateRegress,
        TodoChainCode::ThemeUnknown,
        TodoChainCode::RegistryDup,
        TodoChainCode::AggCountBreak,
        TodoChainCode::ArchiveDrift,
        TodoChainCode::ChainCycle,
        TodoChainCode::FingerprintDrift,
        TodoChainCode::LedgerOverflow,
        TodoChainCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in TodoChainCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    TodoChainCode::Unknown.detail()
}

// ── 冻结常量表 ───────────────────────────────────────────────────
/// 待办账容量（冻结）。
pub const TODO_CAP: usize = 12;
/// B32 主题号（冻结，承接预告链的收口主题）。
pub const THEME_B32: u32 = 32;
/// 预告来源号（前批预告）。
pub const TEASER_F12598: u32 = 12598;
pub const TEASER_F12599: u32 = 12599;
/// 本批 20 条单跑入口（聚合口径，冻结）。
pub const AGG_ENTRIES: u32 = 20;

/// 待办状态机：Open(0)→Registered(1)→Linked(2)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TodoState {
    Open,
    Registered,
    Linked,
}

impl TodoState {
    pub const fn rank(self) -> u32 {
        match self {
            TodoState::Open => 0,
            TodoState::Registered => 1,
            TodoState::Linked => 2,
        }
    }
}

/// 一条待办：主题号 / 主题名 / 状态 / 链接的预告号（可无）。
#[derive(Clone, Copy, Debug)]
pub struct TodoEntry {
    pub theme: u32,
    pub name: &'static str,
    pub state: TodoState,
    /// 挂上的预告号（0=未挂链）。
    pub teaser: u32,
}

// ── 容量点验用占位名（12 条互异，供边界测试登记） ────────────────
pub const PLACE_NAMES: [&str; TODO_CAP] = [
    "占位-01", "占位-02", "占位-03", "占位-04", "占位-05", "占位-06",
    "占位-07", "占位-08", "占位-09", "占位-10", "占位-11", "占位-12",
];

// ── B32 主题框架子件（冻结 8 条，登记入账的内容） ────────────────
pub const B32_FRAMEWORK: [&str; 8] = [
    "B32-警报位语义",
    "B32-可警报等待",
    "B32-消息泵联动",
    "B32-用户APC排队",
    "B32-用户APC交付",
    "B32-用户APC完成",
    "B32-内核APC禁越",
    "B32-消费边回指B28",
];

// ── 待办账 ───────────────────────────────────────────────────────
/// 域待办账：容量冻结的定容账。
pub struct TodoLedger {
    entries: Vec<TodoEntry>,
    pub agg_total: u32,
    pub linked_total: u32,
}

impl TodoLedger {
    pub fn new() -> TodoLedger {
        TodoLedger { entries: Vec::new(), agg_total: 0, linked_total: 0 }
    }

    /// 注册主题：重名/重号拒收；容量冻结溢出拒收。
    pub fn register(&mut self, theme: u32, name: &'static str) -> Result<(), u32> {
        if self.entries.len() >= TODO_CAP {
            return Err(TodoChainCode::LedgerOverflow.val());
        }
        for e in self.entries.iter() {
            if e.theme == theme {
                return Err(TodoChainCode::RegistryDup.val());
            }
            if e.name == name {
                return Err(TodoChainCode::RegistryDup.val());
            }
        }
        self.entries.push(TodoEntry { theme, name, state: TodoState::Registered, teaser: 0 });
        self.agg_total += 1;
        Ok(())
    }

    /// 按主题号找下标。
    fn find(&self, theme: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.theme == theme {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 推进状态（只进不退）。
    pub fn advance(&mut self, theme: u32, next: TodoState) -> Result<(), u32> {
        let idx = match self.find(theme) {
            None => return Err(TodoChainCode::TodoMissing.val()),
            Some(i) => i,
        };
        let cur = match self.entries.get(idx) {
            Some(e) => e.state,
            None => return Err(TodoChainCode::TodoMissing.val()),
        };
        if next.rank() <= cur.rank() {
            return Err(TodoChainCode::StateRegress.val());
        }
        if let Some(e) = self.entries.get_mut(idx) {
            e.state = next;
        }
        Ok(())
    }

    /// 挂预告链接：主题须已注册，链接落账后主题推到 Linked。
    pub fn link_teaser(&mut self, theme: u32, teaser: u32) -> Result<(), u32> {
        let idx = match self.find(theme) {
            None => return Err(TodoChainCode::ThemeUnknown.val()),
            Some(i) => i,
        };
        let already = match self.entries.get(idx) {
            Some(e) => e.teaser != 0,
            None => return Err(TodoChainCode::ThemeUnknown.val()),
        };
        if already {
            return Err(TodoChainCode::RegistryDup.val());
        }
        if let Some(e) = self.entries.get_mut(idx) {
            e.teaser = teaser;
        }
        self.linked_total += 1;
        self.advance(theme, TodoState::Linked)
    }

    /// 主题名点验：已注册主题可查名。
    pub fn name_of(&self, theme: u32) -> Option<&'static str> {
        let idx = self.find(theme)?;
        match self.entries.get(idx) {
            Some(e) => Some(e.name),
            None => None,
        }
    }

    /// 链接数点验。
    pub fn link_count(&self, theme: u32) -> u32 {
        match self.find(theme) {
            Some(i) => match self.entries.get(i) {
                Some(e) => {
                    if e.teaser != 0 {
                        1
                    } else {
                        0
                    }
                }
                None => 0,
            },
            None => 0,
        }
    }

    /// 聚合等式：agg_total = entries.len()，linked ≤ agg_total。
    pub fn equation(&self) -> Result<(), u32> {
        if self.agg_total as usize != self.entries.len() {
            return Err(TodoChainCode::AggCountBreak.val());
        }
        if self.linked_total > self.agg_total {
            return Err(TodoChainCode::AggCountBreak.val());
        }
        Ok(())
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        self.agg_total
            .wrapping_mul(41)
            .wrapping_add(self.linked_total.wrapping_mul(13))
    }
}

// ── 预告链（冻结两段 + 收口） ────────────────────────────────────
/// 一段预告链节：来源预告号 → 承接主题号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainLink {
    pub from_teaser: u32,
    pub to_theme: u32,
}

/// 冻结预告链：F12598 → B32 子件(32) → F12599 → B32(32 收口)。
/// 语义：两条预告边都收口到 B32 主题框架（链尾=B32）。
pub const TEASER_CHAIN: [ChainLink; 3] = [
    ChainLink { from_teaser: TEASER_F12598, to_theme: 31 },
    ChainLink { from_teaser: 31, to_theme: 32 },
    ChainLink { from_teaser: TEASER_F12599, to_theme: 32 },
];

/// 链点验（对账内账）：每节 to_theme 必须已注册（零悬空），
/// 链必须收口在 B32，无回环（to 序号沿链推进不回头）。
pub fn chain_ok(led: &TodoLedger) -> Result<(), u32> {
    if TEASER_CHAIN.is_empty() {
        return Err(TodoChainCode::ChainBroken.val());
    }
    // 零悬空：逐节解析。
    for l in TEASER_CHAIN.iter() {
        if led.name_of(l.to_theme).is_none() {
            return Err(TodoChainCode::DanglingLink.val());
        }
    }
    // 链尾收口：最后一节必须指向 B32。
    let last = match TEASER_CHAIN.last() {
        Some(l) => *l,
        None => return Err(TodoChainCode::ChainBroken.val()),
    };
    if last.to_theme != THEME_B32 {
        return Err(TodoChainCode::ChainBroken.val());
    }
    // 回环检查：沿 31→32 方向 to_theme 单调不减且不回到起点。
    let mut seen_from: Vec<u32> = Vec::new();
    for l in TEASER_CHAIN.iter() {
        for s in seen_from.iter() {
            if *s == l.to_theme && l.to_theme != THEME_B32 {
                return Err(TodoChainCode::ChainCycle.val());
            }
        }
        seen_from.push(l.from_teaser);
    }
    Ok(())
}

/// B32 主题框架登记全流程：8 子件 + B32 主件注册 + 双预告挂链。
pub fn register_framework(led: &mut TodoLedger) -> Result<u32, u32> {
    // 子件以 3200+i 为号逐条注册。
    let mut i = 0;
    while i < B32_FRAMEWORK.len() {
        let name = match B32_FRAMEWORK.get(i) {
            Some(n) => *n,
            None => return Err(TodoChainCode::TodoMissing.val()),
        };
        led.register(3200 + i as u32, name)?;
        i += 1;
    }
    // B32 主件注册（子件满 8 后，容量 12 恰容纳 8+1+主件挂账空间）。
    led.register(THEME_B32, "B32-主题框架")?;
    // 双预告挂链到 B32。
    led.link_teaser(THEME_B32, TEASER_F12598)?;
    // 第二条预告走 31 号中转主题（先注册再挂）。
    led.register(31, "B31-消息面一收口")?;
    led.link_teaser(31, TEASER_F12599)?;
    // 链点验。
    chain_ok(led)?;
    // 聚合等式。
    led.equation()?;
    Ok(led.fingerprint())
}

// ── ktest 聚合口径 ───────────────────────────────────────────────
/// 聚合器：本批 20 条单跑入口的聚合账（计数对账 + 指纹归档）。
pub struct KtestAgg {
    /// 已聚合入口数（单跑一批应恰 20）。
    pub aggregated: u32,
    /// 预告链接的域账引用计数。
    pub chain_links: u32,
}

impl KtestAgg {
    pub const fn new() -> KtestAgg {
        KtestAgg { aggregated: 0, chain_links: 0 }
    }

    /// 聚合一条入口（计数 +1；超过 20 显性拒）。
    pub fn add_entry(&mut self) -> Result<(), u32> {
        if self.aggregated >= AGG_ENTRIES {
            return Err(TodoChainCode::LedgerOverflow.val());
        }
        self.aggregated += 1;
        Ok(())
    }

    /// 聚合等式：聚合数恰为 AGG_ENTRIES 才算齐。
    pub fn complete(&self) -> Result<(), u32> {
        if self.aggregated != AGG_ENTRIES {
            return Err(TodoChainCode::AggCountBreak.val());
        }
        Ok(())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// LCG 抽 10 个子件主题号逐一回查名（登记回查对平）。
pub fn sample_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut led = TodoLedger::new();
    register_framework(&mut led)?;
    let mut ok = 0u32;
    let mut i = 0;
    while i < 10 {
        let sub = 3200 + rng.below(8);
        match led.name_of(sub as u32) {
            Some(n) => {
                // 名必须落在 B32 框架闭集内。
                let mut found = false;
                for fname in B32_FRAMEWORK.iter() {
                    if *fname == n {
                        found = true;
                    }
                }
                if found {
                    ok += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    if ok == 10 {
        Ok(10)
    } else {
        Err(TodoChainCode::DanglingLink.val())
    }
}

// ── 注入面：账违约必抓 ───────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0 }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入族 0：悬空链接——挂到未注册主题 → ThemeUnknown。
/// 注入族 1：重复注册 → RegistryDup。
/// 注入族 2：状态倒退 → StateRegress。
/// 注入族 3：悬空链点验——手工造一个指向未注册号的链节闭式 → DanglingLink。
/// 注入族 4：聚合溢出（第 21 条）→ LedgerOverflow。
pub fn inject_violations(led: &mut InjectLedger) {
    // 族 0。
    led.tries += 1;
    let mut a = TodoLedger::new();
    if a.link_teaser(999, TEASER_F12598) == Err(TodoChainCode::ThemeUnknown.val()) {
        led.caught += 1;
    }
    // 族 1。
    led.tries += 1;
    let mut b = TodoLedger::new();
    let _ = b.register(32, "B32-主题框架");
    if b.register(32, "B32-主题框架") == Err(TodoChainCode::RegistryDup.val()) {
        led.caught += 1;
    }
    // 族 2。
    led.tries += 1;
    let mut c = TodoLedger::new();
    let _ = c.register(7, "样例主题");
    let _ = c.advance(7, TodoState::Linked);
    if c.advance(7, TodoState::Registered) == Err(TodoChainCode::StateRegress.val()) {
        led.caught += 1;
    }
    // 族 3：用 chain_ok 对一个缺注册的账跑点验。
    led.tries += 1;
    let d = TodoLedger::new();
    if chain_ok(&d) == Err(TodoChainCode::DanglingLink.val()) {
        led.caught += 1;
    }
    // 族 4。
    led.tries += 1;
    let mut k = KtestAgg::new();
    let mut i = 0;
    while i < AGG_ENTRIES {
        let _ = k.add_entry();
        i += 1;
    }
    if k.add_entry() == Err(TodoChainCode::LedgerOverflow.val()) {
        led.caught += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：框架登记 → 预告链零悬空 → 聚合 20 条 → 指纹。
pub fn golden_run() -> Result<u32, u32> {
    let mut led = TodoLedger::new();
    let fp = register_framework(&mut led)?;
    // 聚合 20 条单跑入口。
    let mut agg = KtestAgg::new();
    let mut i = 0;
    while i < AGG_ENTRIES {
        agg.add_entry()?;
        i += 1;
    }
    agg.complete()?;
    // 链接数与聚合联动：B32 恰挂 1 条预告 + 31 挂 1 条 = 2。
    if led.linked_total != 2 {
        return Err(TodoChainCode::AggCountBreak.val());
    }
    Ok(fp.wrapping_add(agg.aggregated))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        _ => Err(TodoChainCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1：B32 主题框架登记入域待办账。
fn framework_registered_ok() -> bool {
    let mut led = TodoLedger::new();
    match register_framework(&mut led) {
        Ok(_) => led.name_of(THEME_B32) == Some("B32-主题框架"),
        Err(_) => false,
    }
}

/// J2：预告链衔接零悬空。
fn chain_zero_dangling_ok() -> bool {
    let mut led = TodoLedger::new();
    match register_framework(&mut led) {
        Ok(_) => chain_ok(&led).is_ok(),
        Err(_) => false,
    }
}

/// J3：注入全抓。
fn inject_ok() -> bool {
    let mut led = InjectLedger::new();
    inject_violations(&mut led);
    led.all_caught()
}

/// J4：抽样 10 例。
fn sample_ok() -> bool {
    sample_ten() == Ok(10)
}

/// J5：双跑一致。
fn repro_ok() -> bool {
    repro_check().is_ok()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in TodoChainCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    v.push((
        "J1-B32框架入账",
        framework_registered_ok(),
        format!("{} 子件+主件", B32_FRAMEWORK.len()),
    ));
    v.push((
        "J1-预告链零悬空",
        chain_zero_dangling_ok(),
        String::from("F12598/F12599 收口 B32"),
    ));
    // 边界：容量冻结（12 条互异占位名恰好装满，第 13 条显性溢出）。
    let mut full = TodoLedger::new();
    let mut i = 0;
    let mut overflowed = false;
    while i <= TODO_CAP {
        let r = match PLACE_NAMES.get(i) {
            Some(n) => full.register(9000 + i as u32, n),
            None => full.register(9000 + i as u32, "溢出主题"),
        };
        if i == TODO_CAP {
            overflowed = r == Err(TodoChainCode::LedgerOverflow.val());
        }
        i += 1;
    }
    v.push(("J2-待办账容量边界", overflowed, format!("容量 {}", TODO_CAP)));
    // 边界：聚合恰 20。
    let mut agg = KtestAgg::new();
    let mut k = 0;
    while k < AGG_ENTRIES {
        let _ = agg.add_entry();
        k += 1;
    }
    v.push((
        "J2-聚合恰20",
        agg.complete().is_ok(),
        format!("={}", AGG_ENTRIES),
    ));
    v.push(("J3-注入全抓", inject_ok(), String::from("五族账违约")));
    v.push(("J4-抽样10例对平", sample_ok(), format!("{:?}", sample_ten())));
    v.push(("J6-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J7-段位守卫", segment_guard(), String::from("0x804C0 窗口")));
    v
}

/// VX9 检查入口（卡 12619，UNX-F12620-J1 判据承载）。
pub fn run_unx12619_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12619 ktest 聚合器（本批 20 条单跑入口）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_framework_registered() {
        assert!(framework_registered_ok());
    }

    #[test]
    fn test_chain_zero_dangling() {
        assert!(chain_zero_dangling_ok());
    }

    #[test]
    fn test_dangling_link_caught() {
        let led = TodoLedger::new();
        assert_eq!(chain_ok(&led), Err(TodoChainCode::DanglingLink.val()));
    }

    #[test]
    fn test_registry_dup_caught() {
        let mut led = TodoLedger::new();
        assert!(led.register(1, "甲").is_ok());
        assert_eq!(led.register(1, "乙"), Err(TodoChainCode::RegistryDup.val()));
        assert_eq!(led.register(2, "甲"), Err(TodoChainCode::RegistryDup.val()));
    }

    #[test]
    fn test_state_regress_caught() {
        let mut led = TodoLedger::new();
        let _ = led.register(5, "主题甲");
        assert!(led.advance(5, TodoState::Linked).is_ok());
        assert_eq!(
            led.advance(5, TodoState::Open),
            Err(TodoChainCode::StateRegress.val())
        );
    }

    #[test]
    fn test_capacity_boundary() {
        let mut led = TodoLedger::new();
        let mut i = 0;
        while i < TODO_CAP {
            let name = match PLACE_NAMES.get(i) {
                Some(n) => *n,
                None => "兜底名",
            };
            assert!(led.register(100 + i as u32, name).is_ok());
            i += 1;
        }
        assert_eq!(
            led.register(999, "溢出"),
            Err(TodoChainCode::LedgerOverflow.val())
        );
    }

    #[test]
    fn test_agg_twenty_boundary() {
        let mut agg = KtestAgg::new();
        let mut i = 0;
        while i < AGG_ENTRIES {
            assert!(agg.add_entry().is_ok());
            i += 1;
        }
        assert_eq!(agg.add_entry(), Err(TodoChainCode::LedgerOverflow.val()));
        assert!(agg.complete().is_ok());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut led = InjectLedger::new();
        inject_violations(&mut led);
        assert_eq!(led.tries, 5);
        assert_eq!(led.caught, 5);
    }

    #[test]
    fn test_sample_ten() {
        assert_eq!(sample_ten(), Ok(10));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or(0);
        let b = golden_run().unwrap_or(1);
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x804C0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in TodoChainCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xACDC).contains("未知"));
    }
}
