//! UNX-F21590 · 下游会签复核（AI-29/30/61/66-70/87）
//!
//! 诊断段 0x91049~0x91050（F2-B40 域收官段 10/100）。
//!
//! 判据原文（施工书 UNX-F21590-J1）：复核 30 例，消费面零破坏。
//!
//! 设计要点（为什么「零破坏」只能靠分类表算出来，不能靠会议声明）：
//! 九家消费方对「破坏」的定义各不相同——接口移除谁都会喊破坏，而新增一个
//! 可选参数、只改文档，喊破坏的多半是在护自己的实现细节。若把判定权交给
//! 会签会上的口头结论，零破坏断言就退化成一句祝福。故把「变更种类 → 是否
//! 破坏」写成编译期常量表（`ChangeKind::is_breaking`），复核只负责逐例套表：
//! 表是唯一权威，谁想放宽就得改表，而改表在 diff 里一眼可见。
//!
//! 兼容窗挂在**依赖边**上而非消费方身上：一家消费方同时依赖 1.x 的核心派发
//! 与 4.x 的中断路由，若只给它一个窗口，两个接口里必有一个永远「越窗」——
//! 那是模型错，不是消费方错。所以 `Dep` 携带 (iface, 窗口下界主版本, 上界次
//! 版本)，越窗判定逐边进行。
//!
//! 码段只有两位（0x91049 干净 / 0x91050 有破坏），成因有八种，故与码并列另设
//! `Blame`：码回答「消费面是否被破」，Blame 回答「破在哪一类」。判据同时断言
//! 两者，定位比八个平级码更细，且成因可增长而码段不膨胀。
//!
//! 豁免必须带失效序号：永久豁免等于把破坏合法化，而带序号的豁免会在下一次
//! 复核自动复现为 `WaiverExpired`——「豁免到期」本身就是要报的违规。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 复核样本数（施工书：30 例）。
pub const SAMPLE_TARGET: usize = 30;
/// 成因槽位数（与 `Blame` 变体数一致）。
pub const BLAME_SLOTS: usize = 8;

/// 复核诊断码（本单码段仅两位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakCode {
    Clean = 0x91049,
    BreakFound = 0x91050,
}

impl BreakCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            BreakCode::Clean => "消费面零破坏",
            BreakCode::BreakFound => "消费面存在未消解的破坏",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            BreakCode::Clean => "30 例逐条套分类表后无活跃破坏",
            BreakCode::BreakFound => "存在破坏性变更、越窗版本、未登记消费方或过期豁免",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            BreakCode::Clean => "归档零破坏断言并放行下游",
            BreakCode::BreakFound => "按违规清单逐条消解，或补签带失效序号的豁免",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会静默截断）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 破坏成因（八元闭集，定位粒度在此而不在码上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blame {
    Removed,
    Narrowed,
    Semantics,
    /// 变更版本落在依赖边的兼容窗之外。
    OutOfWindow,
    UnknownConsumer,
    /// 案例引用了消费方未声明的依赖。
    Uncovered,
    /// 豁免已失效却仍被引用。
    WaiverExpired,
    /// 样本数不是 30。
    CaseCount,
}

impl Blame {
    /// 定位文本：现象与成因合写一句。
    ///
    /// 成因不是诊断码，三要素由 `BreakCode` 统一承载；此处合成的句子会直接进
    /// 违规清单，一段话读完就知道「哪儿坏了、为什么算坏」，比列两段更省事。
    pub const fn what(self) -> &'static str {
        match self {
            Blame::Removed => "接口被移除：调用点全部失效，属最硬的不兼容",
            Blame::Narrowed => "参数域被收窄：原合法入参变非法，调用方无法预判",
            Blame::Semantics => "返回语义被改变：签名未动而含义变了，最难发现",
            Blame::OutOfWindow => "变更落在兼容窗之外：消费方只声明了该区间的兼容",
            Blame::UnknownConsumer => "案例指向未登记消费方：登记表没这家，判不了区间",
            Blame::Uncovered => "案例引用未声明依赖：依赖清单已过时",
            Blame::WaiverExpired => "豁免已失效仍被引用：豁免只在失效序号前有效",
            Blame::CaseCount => "复核样本数不是 30：少例多例都让零破坏断言失去覆盖",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            Blame::Removed => "保留旧接口并标注弃用，或让消费方切新版",
            Blame::Narrowed => "恢复原参数域，另开严格版本",
            Blame::Semantics => "恢复原语义，或以新接口名承载新语义",
            Blame::OutOfWindow => "升级依赖边兼容窗，或回退变更版本",
            Blame::UnknownConsumer => "补登记消费方及其依赖集",
            Blame::Uncovered => "更新依赖集声明后重跑",
            Blame::WaiverExpired => "补签新序号豁免，或消解破坏",
            Blame::CaseCount => "补齐至 30 例后重跑",
        }
    }
    /// 直方图槽位。
    pub const fn slot(self) -> usize {
        self as usize
    }
    /// 是否可由豁免抵消。
    ///
    /// 只有「变更本身」三类可豁免；越窗、未登记、清单过时、豁免过期属于流程
    /// 缺陷，签豁免抵掉它们等于把账做平而不是把事做完。
    pub const fn is_waivable(self) -> bool {
        matches!(self, Blame::Removed | Blame::Narrowed | Blame::Semantics)
    }
}

/// 变更种类（分类表：是否破坏由此判定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    Removed,
    ParamNarrowed,
    ReturnSemanticsChanged,
    OptionalParamAdded,
    DocOnly,
}

impl ChangeKind {
    /// 名称（报告用）。
    pub const fn label(self) -> &'static str {
        match self {
            ChangeKind::Removed => "removed",
            ChangeKind::ParamNarrowed => "param-narrowed",
            ChangeKind::ReturnSemanticsChanged => "return-semantics",
            ChangeKind::OptionalParamAdded => "optional-param",
            ChangeKind::DocOnly => "doc-only",
        }
    }
    /// 分类表：唯一权威。
    pub const fn is_breaking(self) -> bool {
        match self {
            ChangeKind::Removed => true,
            ChangeKind::ParamNarrowed => true,
            ChangeKind::ReturnSemanticsChanged => true,
            ChangeKind::OptionalParamAdded => false,
            ChangeKind::DocOnly => false,
        }
    }
    /// 映射到成因（非破坏类返回 None）。
    pub const fn blame(self) -> Option<Blame> {
        match self {
            ChangeKind::Removed => Some(Blame::Removed),
            ChangeKind::ParamNarrowed => Some(Blame::Narrowed),
            ChangeKind::ReturnSemanticsChanged => Some(Blame::Semantics),
            _ => None,
        }
    }
}

/// 一条依赖边：接口 + 兼容窗（主版本固定 + 次版本上界）。
///
/// 窗口挂在边上而非消费方上——见文件头「兼容窗挂在依赖边上」的取舍说明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dep {
    pub iface: &'static str,
    pub window_major: u16,
    pub window_minor: u16,
}

impl Dep {
    /// 构造依赖边。
    pub const fn new(iface: &'static str, window_major: u16, window_minor: u16) -> Dep {
        Dep { iface, window_major, window_minor }
    }
    /// 兼容窗是否覆盖某版本（同主版本且次版本不高于上界）。
    pub const fn accepts(&self, major: u16, minor: u16) -> bool {
        major == self.window_major && minor <= self.window_minor
    }
}

/// 消费方登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Consumer {
    pub name: &'static str,
    /// 依赖边集合（会签时冻结）。
    pub deps: &'static [Dep],
}

impl Consumer {
    /// 构造登记项。
    pub const fn new(name: &'static str, deps: &'static [Dep]) -> Consumer {
        Consumer { name, deps }
    }
    /// 查依赖边。
    pub fn dep(&self, iface: &str) -> Option<&Dep> {
        self.deps.iter().find(|d| str_eq(d.iface, iface))
    }
    /// 是否声明依赖该接口。
    pub fn depends_on(&self, iface: &str) -> bool {
        self.dep(iface).is_some()
    }
    /// 登记项自洽：名非空、依赖非空、首条依赖窗口有效。
    pub fn is_well_formed(&self) -> bool {
        self.deps.first().map_or(false, |d| !self.name.is_empty() && d.window_major > 0)
    }
}

/// 一条变更。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Change {
    pub iface: &'static str,
    pub major: u16,
    pub minor: u16,
    pub kind: ChangeKind,
}

impl Change {
    /// 构造变更。
    pub const fn new(iface: &'static str, major: u16, minor: u16, kind: ChangeKind) -> Change {
        Change { iface, major, minor, kind }
    }
}

/// 一条复核样本（消费方 / 接口 / 期望行为 / 实测行为）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    pub consumer: &'static str,
    pub iface: &'static str,
    pub expected: &'static str,
    pub actual: &'static str,
}

impl Case {
    /// 构造样本。
    pub const fn new(
        consumer: &'static str,
        iface: &'static str,
        expected: &'static str,
        actual: &'static str,
    ) -> Case {
        Case { consumer, iface, expected, actual }
    }
    /// 行为是否与期望一致。
    pub fn agrees(&self) -> bool {
        str_eq(self.expected, self.actual)
    }
}

/// 条件豁免：带失效序号，过期即复发。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Waiver {
    pub consumer: &'static str,
    pub iface: &'static str,
    /// 失效序号（含）：`now_seq > until_seq` 即失效。
    pub until_seq: u32,
    pub condition: &'static str,
}

impl Waiver {
    /// 构造豁免。
    pub const fn new(
        consumer: &'static str,
        iface: &'static str,
        until_seq: u32,
        condition: &'static str,
    ) -> Waiver {
        Waiver { consumer, iface, until_seq, condition }
    }
    /// 是否仍然有效。
    pub const fn is_live(&self, now_seq: u32) -> bool {
        now_seq <= self.until_seq
    }
    /// 是否覆盖某案例。
    pub fn covers(&self, consumer: &str, iface: &str) -> bool {
        str_eq(self.consumer, consumer) && str_eq(self.iface, iface)
    }
}

/// 一条破坏记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Breach {
    pub blame: Blame,
    pub consumer: &'static str,
    pub iface: &'static str,
    /// 是否已被有效豁免抵消。
    pub waived: bool,
}

impl Breach {
    /// 是否为仍需消解的活跃破坏。
    pub const fn is_active(&self) -> bool {
        !self.waived
    }
    /// 一行文本（违规清单用）。
    pub fn line(&self) -> String {
        let mut out = String::with_capacity(64);
        out.push_str(self.consumer);
        out.push('/');
        out.push_str(self.iface);
        out.push(':');
        out.push_str(self.blame.what());
        out.push_str(if self.waived { " [waived]" } else { " [active]" });
        out    }
}

/// 复核故障（`review` 与登记路径的 Err 侧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fault {
    pub code: u32,
    pub blame: Blame,
    pub consumer: &'static str,
    pub iface: &'static str,
}

impl Fault {
    const fn new(blame: Blame, consumer: &'static str, iface: &'static str) -> Fault {
        Fault { code: BreakCode::BreakFound.code(), blame, consumer, iface }
    }
}

/// 复核报告。
pub struct ReviewReport {
    /// 参与复核的样本数。
    pub cases: usize,
    /// 全部破坏记录（含已豁免）。
    pub breaches: Vec<Breach>,
    /// 活跃破坏数。
    pub active: usize,
    /// 被豁免抵消的破坏数。
    pub waived: usize,
    /// 复核时点序号（豁免按此判定）。
    pub seq: u32,
}

impl ReviewReport {
    /// 裁决码。
    pub fn code(&self) -> BreakCode {
        if self.active == 0 {
            BreakCode::Clean
        } else {
            BreakCode::BreakFound
        }
    }
    /// 零破坏断言。
    pub fn is_zero_break(&self) -> bool {
        self.active == 0
    }
    /// 违规清单（只列活跃项，报告要能直接当工单用）。
    pub fn active_list(&self) -> String {
        let mut out = String::with_capacity(160);
        for b in self.breaches.iter() {
            if b.is_active() {
                if !out.is_empty() {
                    out.push(';');
                }
                out.push_str(&b.line());
            }
        }
        out
    }
    /// 成因计数（含已豁免：看「豁免都压在哪儿」同样重要）。
    pub fn count_of(&self, blame: Blame) -> usize {
        self.breaches.iter().filter(|b| b.blame == blame).count()
    }
    /// 报告指纹：样本数 + 逐条 (成因, 消费方字节, 接口字节, 豁免位)。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv_u64(0xcbf2_9ce4_8422_2325, self.cases as u64);
        for b in self.breaches.iter() {
            h = fnv_u64(h, b.blame.slot() as u64);
            h = feed_str(h, b.consumer);
            h = feed_str(h, b.iface);
            h = fnv_u64(h, if b.waived { 1 } else { 0 });
        }
        h
    }
    /// 导出文本（字段序固定，便于逐行 diff）。
    pub fn export(&self) -> String {
        let mut out = String::with_capacity(192);
        out.push_str("unx-f21590 cases=");
        out.push_str(&self.cases.to_string());
        out.push_str(" active=");
        out.push_str(&self.active.to_string());
        out.push_str(" waived=");
        out.push_str(&self.waived.to_string());
        out.push_str(" seq=");
        out.push_str(&self.seq.to_string());
        out.push_str(" code=");
        out.push_str(&hex32(self.code().code()));
        out.push_str(" list=[");
        out.push_str(&self.active_list());
        out.push(']');
        out
    }
}

/// 下游消费面：登记表 + 豁免表 + 复核时点。
pub struct DownSurface {
    consumers: Vec<Consumer>,
    waivers: Vec<Waiver>,
    now_seq: u32,
}

impl DownSurface {
    /// 新建空面。
    pub const fn new(now_seq: u32) -> DownSurface {
        DownSurface { consumers: Vec::new(), waivers: Vec::new(), now_seq }
    }
    /// 登记消费方。
    ///
    /// 重复与残缺登记都报 `BreakFound` + 定位字段：登记表脏了，后面所有
    /// 「零破坏」结论都不可信，必须在最早的位置拦下。
    pub fn enroll(&mut self, c: Consumer) -> Result<(), Fault> {
        if !c.is_well_formed() {
            return Err(Fault::new(Blame::UnknownConsumer, c.name, "enroll"));
        }
        if self.consumer(c.name).is_some() {
            return Err(Fault::new(Blame::UnknownConsumer, c.name, "duplicate"));
        }
        self.consumers.push(c);
        Ok(())
    }
    /// 追加一条豁免。
    pub fn grant(&mut self, w: Waiver) -> Result<(), Fault> {
        if !w.is_live(self.now_seq) {
            // 出生即过期的豁免没有意义，只会污染台账。
            return Err(Fault::new(Blame::WaiverExpired, w.consumer, w.iface));
        }
        self.waivers.push(w);
        Ok(())
    }
    /// 按名查消费方。
    pub fn consumer(&self, name: &str) -> Option<&Consumer> {
        self.consumers.iter().find(|c| str_eq(c.name, name))
    }
    /// 登记家数。
    pub fn len(&self) -> usize {
        self.consumers.len()
    }
    /// 消费方列表（夹具生成样本用）。
    pub fn consumers(&self) -> &[Consumer] {
        &self.consumers
    }
    /// 覆盖某案例的豁免。
    pub fn waiver_for(&self, consumer: &str, iface: &str) -> Option<&Waiver> {
        self.waivers.iter().find(|w| w.covers(consumer, iface))
    }
    /// 复核时点。
    pub const fn now_seq(&self) -> u32 {
        self.now_seq
    }
    /// 换一个复核时点（保留登记表与豁免，用于测「豁免到期」）。
    pub fn rebase(&mut self, now_seq: u32) {
        self.now_seq = now_seq;
    }

    /// 复核 30 例。
    ///
    /// 每例只产出一条破坏（首个成因优先）：一条案例被两种原因同时击穿时，报
    /// 第一条才能让运维先修最上游那个；报全部只会让工单更吓人、修得更慢。
    /// 唯一例外是过期豁免——它是独立事实，会顶掉其他成因。
    pub fn review(&self, changes: &[Change], cases: &[Case]) -> Result<ReviewReport, Fault> {
        if cases.len() != SAMPLE_TARGET {
            return Err(Fault::new(Blame::CaseCount, "-", "case-count"));
        }
        let mut breaches: Vec<Breach> = Vec::new();
        let mut active = 0usize;
        let mut waived = 0usize;
        for case in cases.iter() {
            let mut blame = None;
            match self.consumer(case.consumer) {
                None => blame = Some(Blame::UnknownConsumer),
                Some(c) => match c.dep(case.iface) {
                    None => blame = Some(Blame::Uncovered),
                    Some(dep) => {
                        if let Some(ch) = changes.iter().find(|x| str_eq(x.iface, case.iface)) {
                            if !dep.accepts(ch.major, ch.minor) {
                                blame = Some(Blame::OutOfWindow);
                            } else if let Some(b) = ch.kind.blame() {
                                blame = Some(b);
                            }
                        }
                        if blame.is_none() && !case.agrees() {
                            blame = Some(Blame::Semantics);
                        }
                    }
                },
            }
            // 无破坏时连豁免都不必查：查了等于承认「没坏也能签豁免」。
            let blame = match blame {
                Some(b) => b,
                None => continue,
            };
            let hit = self.waiver_for(case.consumer, case.iface);
            let taken = match hit {
                Some(w) if w.is_live(self.now_seq) && blame.is_waivable() => {
                    waived += 1;
                    Breach { blame, consumer: case.consumer, iface: case.iface, waived: true }
                }
                // 豁免存在但已过期（或该成因本不可豁免）：按过期复发处置。
                Some(_) => {
                    active += 1;
                    Breach { blame: Blame::WaiverExpired, consumer: case.consumer, iface: case.iface, waived: false }
                }
                None => {
                    active += 1;
                    Breach { blame, consumer: case.consumer, iface: case.iface, waived: false }
                }
            };
            breaches.push(taken);
        }
        Ok(ReviewReport { cases: cases.len(), breaches, active, waived, seq: self.now_seq })
    }
}

/// 字节级字符串比较。
///
/// 不用 `==`：这里要显式表达「比的是名字内容而非指针」——登记表里存的是
/// `&'static str`，两个同名字面量是否同地址由编译器决定，不能拿来做语义相等。
fn str_eq(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| p == q)
}

/// FNV-1a 64 单步。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 把字符串按字节喂进 FNV-1a。
pub fn feed_str(mut h: u64, s: &str) -> u64 {
    for b in s.as_bytes() {
        h = fnv_u64(h, *b as u64);
    }
    h
}

/// 32 位十六进制（码值直出）。
pub fn hex32(v: u32) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(8);
    for shift in (0..32u32).step_by(4).rev() {
        s.push(HEX[((v >> shift) & 0xF) as usize] as char);
    }
    s
}

/// 标准下游面：九家消费方，依赖两两重叠。
///
/// 依赖刻意重叠：零破坏断言的价值在于「同一接口被多家消费」时仍成立；
/// 九家各依赖不同接口只能证明分类表没写错，证明不了消费面。
pub fn standard_surface() -> Result<DownSurface, Fault> {
    let mut s = DownSurface::new(120);
    ROSTER.iter().try_for_each(|c| s.enroll(*c))?;
    Ok(s)
}

/// 九家消费方的冻结登记表。
///
/// 提成 `const` 而非函数内 `let`：`Consumer` 持有 `&'static [Dep]`，函数内
/// 的数组字面量会在语句末尾释放，借用活不到登记完成。
const ROSTER: [Consumer; 9] = [
        Consumer::new("ai29.fs", &[Dep::new("ai26.core.dispatch", 1, 4), Dep::new("ai26.mem.arena", 2, 1)]),
        Consumer::new("ai30.net", &[Dep::new("ai26.core.dispatch", 1, 4), Dep::new("ai26.irq.route", 4, 2)]),
        Consumer::new("ai61.sched", &[Dep::new("ai26.sched.tick", 3, 6), Dep::new("ai26.core.dispatch", 1, 4)]),
        Consumer::new("ai66.vfs", &[Dep::new("ai26.mem.arena", 2, 1), Dep::new("ai26.sched.tick", 3, 6)]),
        Consumer::new("ai67.task", &[Dep::new("ai26.sched.tick", 3, 6), Dep::new("ai26.mem.arena", 2, 1)]),
        Consumer::new("ai68.cgroup", &[Dep::new("ai26.sched.tick", 3, 6), Dep::new("ai26.irq.route", 4, 2)]),
        Consumer::new("ai69.irq", &[Dep::new("ai26.irq.route", 4, 2), Dep::new("ai26.core.dispatch", 1, 4)]),
        Consumer::new("ai70.mm", &[Dep::new("ai26.mem.arena", 2, 1), Dep::new("ai26.irq.route", 4, 2)]),
        Consumer::new("ai87.dma", &[Dep::new("ai26.irq.route", 4, 2), Dep::new("ai26.mem.arena", 2, 1)]),
];

/// 生成 30 例标准样本：九家 × 两条依赖边（18 组）轮转到 30。
pub fn standard_cases(s: &DownSurface) -> Vec<Case> {
    let pairs: Vec<(&'static str, &'static str)> = s
        .consumers()
        .iter()
        .flat_map(|c| c.deps.iter().map(move |d| (c.name, d.iface)))
        .collect();
    let mut out = Vec::with_capacity(SAMPLE_TARGET);
    let n = pairs.len();
    if n == 0 {
        return out;
    }
    let mut k = 0usize;
    while out.len() < SAMPLE_TARGET {
        if let Some((consumer, iface)) = pairs.get(k % n) {
            out.push(Case::new(consumer, iface, "allow", "allow"));
        }
        k += 1;
    }
    out
}

/// 标准夹具（消费面 + 30 例 + 空变更表）。
pub fn standard_fixture() -> Result<(DownSurface, Vec<Case>), Fault> {
    let surface = standard_surface()?;
    let cases = standard_cases(&surface);
    Ok((surface, cases))
}

/// 空白下游面：同一张登记表、不同台账，用于「有豁免 / 无豁免」对照。
fn fresh_surface() -> DownSurface {
    let mut s = DownSurface::new(120);
    // 登记项已由 `standard_surface` 证明可装配，这里只造对照组，无需再报错。
    ROSTER.iter().for_each(|c| {
        let _ = s.enroll(*c);
    });
    s
}

/// 判据入口（UNX-F21590-J1 及其配套十三条）。
///
/// 判据常量独立写死：`30` / `9` / `0x91049` / `0x91050` 以字面量出现。
pub fn run_unx21590_downcosign_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21590");
    let br = Breach { blame: Blame::Removed, consumer: "ai29.fs", iface: "ai26.mem.arena", waived: false };

    // 1) 契约与夹具先行（码值 + 三要素 + 分类表 + 定位文本）。
    cs.add(
        "f21590.contract_and_fixture",
        standard_fixture().is_ok()
            && BreakCode::Clean.code() == 0x91049
            && BreakCode::BreakFound.code() == 0x91050
            && !BreakCode::BreakFound.what().is_empty()
            && !BreakCode::BreakFound.why().is_empty()
            && !BreakCode::BreakFound.advice().is_empty()
            && ChangeKind::Removed.is_breaking()
            && !ChangeKind::OptionalParamAdded.is_breaking()
            && ChangeKind::DocOnly.blame().is_none()
            && Blame::CaseCount.slot() + 1 == BLAME_SLOTS
            && Blame::Removed.advice().contains("弃用")
            && br.line().contains("ai29.fs") && br.line().contains("ai26.mem.arena"),
        "夹具不可构造，或码值/三要素/分类表/定位文本不达标",
    );

    let (surface, cases) = match standard_fixture() {
        Ok(v) => v,
        Err(_) => {
            cs.add("f21590.fixture_unavailable", false, "标准夹具不可用，其余判据无法运行");
            return cs;
        }
    };

    // 2) 登记面：九家、名不重复、依赖边正查有反查无。
    let list = surface.consumers();
    let names_ok = list
        .iter()
        .enumerate()
        .all(|(i, c)| list.iter().enumerate().all(|(j, d)| i == j || !str_eq(c.name, d.name)));
    let edge_ok = match surface.consumer("ai29.fs") {
        Some(c) => c.is_well_formed() && c.depends_on("ai26.mem.arena") && !c.depends_on("ai26.irq.route"),
        None => false,
    };
    cs.add("f21590.registry_nine", surface.len() == 9 && names_ok && edge_ok, "消费方不足九家、重名或依赖边不可查");

    // 3) 零破坏正向：30 例 + 空变更表 → Clean。
    let clean = surface.review(&[], &cases).map_or(false, |r| {
        r.cases == 30 && r.active == 0 && r.waived == 0 && r.is_zero_break()
            && r.code() == BreakCode::Clean && r.active_list().is_empty()
    });
    cs.add("f21590.clean_zero_break", clean, "空变更表下零破坏断言不成立");

    // 4) 样本数 29 / 31 → 结构性拒绝（0x91050 + CaseCount）。
    let short: Vec<Case> = cases.iter().take(29).copied().collect();
    let mut long = cases.clone();
    if let Some(first) = cases.first() {
        long.push(*first);
    }
    let count_ok = matches!(
        (surface.review(&[], &short).err(), surface.review(&[], &long).err()),
        (Some(x), Some(y)) if x.code == 0x91050 && y.code == 0x91050
            && x.blame == Blame::CaseCount && y.blame == Blame::CaseCount
    );
    cs.add("f21590.case_count_enforced", count_ok, "29 或 31 例未被结构性拒绝");

    // 5) 接口移除 → 破坏，成因 Removed。
    let removed = [Change::new("ai26.mem.arena", 2, 1, ChangeKind::Removed)];
    let ok = surface.review(&removed, &cases).map_or(false, |r| {
        r.code() == BreakCode::BreakFound && r.count_of(Blame::Removed) > 0
            && r.active_list().contains("接口被移除")
    });
    cs.add("f21590.removed_breaks", ok, "接口移除未被判为破坏");

    // 6) 参数域收窄 → 破坏，成因 Narrowed。
    let narrowed = [Change::new("ai26.sched.tick", 3, 6, ChangeKind::ParamNarrowed)];
    let ok = surface
        .review(&narrowed, &cases)
        .map_or(false, |r| r.count_of(Blame::Narrowed) > 0 && r.active_list().contains("参数域被收窄"));
    cs.add("f21590.narrowed_breaks", ok, "参数域收窄未被判为破坏");

    // 7) 返回语义改变 → 破坏，成因 Semantics。
    let semantics = [Change::new("ai26.irq.route", 4, 2, ChangeKind::ReturnSemanticsChanged)];
    let ok = surface
        .review(&semantics, &cases)
        .map_or(false, |r| r.count_of(Blame::Semantics) > 0 && r.code() == BreakCode::BreakFound);
    cs.add("f21590.semantics_breaks", ok, "返回语义改变未被判为破坏");

    // 8) 非破坏两类（新增可选参数 / 仅改文档）→ 仍为零破坏。
    let soft = [
        Change::new("ai26.core.dispatch", 1, 4, ChangeKind::OptionalParamAdded),
        Change::new("ai26.mem.arena", 2, 1, ChangeKind::DocOnly),
    ];
    let ok = surface.review(&soft, &cases).map_or(false, |r| {
        r.is_zero_break() && r.code() == BreakCode::Clean && r.breaches.is_empty() && r.active == 0
    }) && ChangeKind::OptionalParamAdded.label() == "optional-param";
    cs.add("f21590.nonbreaking_allowed", ok, "可选参数或纯文档变更被误判为破坏");

    // 9) 未登记消费方 → 破坏，成因 UnknownConsumer。
    let mut ghost = cases.clone();
    if let Some(first) = ghost.first_mut() {
        first.consumer = "ai99.ghost";
    }
    let ok = surface.review(&[], &ghost).map_or(false, |r| {
        r.code() == BreakCode::BreakFound && r.count_of(Blame::UnknownConsumer) == 1
            && r.active_list().contains("ai99.ghost")
    });
    cs.add("f21590.unknown_consumer", ok, "未登记消费方未被识别");

    // 10) 案例引用未声明的依赖 → 破坏，成因 Uncovered。
    let mut stray = cases.clone();
    // ai29.fs 只依赖 core.dispatch / mem.arena，换成 irq.route 即越界。
    for item in stray.iter_mut() {
        if str_eq(item.consumer, "ai29.fs") {
            item.iface = "ai26.irq.route";
        }
    }
    let ok = surface
        .review(&[], &stray)
        .map_or(false, |r| r.count_of(Blame::Uncovered) > 0 && r.code() == BreakCode::BreakFound);
    cs.add("f21590.uncovered_dependency", ok, "未声明依赖未被识别");

    // 11) 有效豁免抵消破坏：仍记入 waived，但活跃数下降。
    let base = surface.review(&removed, &cases).map_or(0, |r| r.active);
    let mut s2 = fresh_surface();
    let granted = s2.grant(Waiver::new("ai29.fs", "ai26.mem.arena", 200, "计划下周期切新版")).is_ok();
    let ok = s2.review(&removed, &cases).map_or(false, |r| {
        r.waived >= 1 && r.active < base && r.count_of(Blame::Removed) >= 1
    });
    cs.add("f21590.waiver_suppresses", granted && ok, "有效豁免未抵消破坏");

    // 12) 豁免过期 → 复发为 WaiverExpired，破坏重新活跃。
    let mut s3 = fresh_surface();
    s3.rebase(50);
    let granted = s3.grant(Waiver::new("ai29.fs", "ai26.mem.arena", 100, "临时豁免")).is_ok();
    s3.rebase(200);
    let ok = s3.review(&removed, &cases).map_or(false, |r| {
        r.count_of(Blame::WaiverExpired) >= 1 && r.code() == BreakCode::BreakFound
    });
    cs.add("f21590.waiver_expired", granted && ok, "过期豁免未复发为违规");

    // 13) 兼容窗逐边判定：越窗即破坏，与变更种类无关（此处是纯文档变更）。
    let window = [Change::new("ai26.mem.arena", 3, 0, ChangeKind::DocOnly)];
    let ok = surface
        .review(&window, &cases)
        .map_or(false, |r| r.count_of(Blame::OutOfWindow) > 0 && r.code() == BreakCode::BreakFound);
    cs.add("f21590.compat_window", ok, "越窗版本未被判为破坏");

    // 14) 确定性：两次独立装配的复核指纹与导出文本逐位相同。
    let (text, fp) = surface
        .review(&[], &cases)
        .map_or((String::new(), 0u64), |r| (r.export(), r.fingerprint()));
    let same = match standard_fixture() {
        Ok((o, oc)) => o.review(&[], &oc).map_or(false, |y| y.export() == text && y.fingerprint() == fp),
        Err(_) => false,
    };
    cs.add(
        "f21590.deterministic_export",
        same && text.contains("cases=30")
            && text.contains("active=0")
            && text.contains("code=00091049")
            && surface.now_seq() == 120,
        "两次复核不一致或导出字段缺失",
    );

    cs
}
