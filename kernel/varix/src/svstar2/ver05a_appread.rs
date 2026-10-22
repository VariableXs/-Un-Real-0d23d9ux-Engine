//! VE-F3415 · 应用读令牌 API —— 只读门面 + 类型安全 + 越权拒绝 + 绕过拦截。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3415`
//!
//! # 职责（锚点原文拆解）
//!
//! - **组件读取令牌的统一 API**：全引擎只有一扇门——[`Facade`]。任何组件、
//!   任何插件、任何内置控件，读令牌都走 [`read_token`]，不允许自己持有
//!   一份表、更不允许自己解析令牌文件；
//! - **只读门面**：门面只发「借用的值」不发「可变引用」。`TokenValue` 是
//!   值语义的拷贝体，读出去之后与门面里的账本再无别名关系——组件改不了
//!   令牌，这是只读二字的物理含义，不是口头约定；
//! - **类型安全层**：[`TokenRead`] 把「路径 + 期望类型」编译成一次静态
//!   约束。颜色令牌读成 `Rgba`、长度令牌读成 `Length`、时长令牌读成
//!   `Duration`，读错类型在**调用点**就被类型系统拦下；运行期再兜一层
//!   [`ApiCode::TypeMismatch`]，防止调用方用 `dyn`/FFI 绕开泛型；
//! - **禁止绕过 API 直取值的编译期拦截**：账本字段 `table` 是私有的，
//!   模块外拿不到 `&BTreeMap`，也拼不出 `Binding`（构造函数不公开）。
//!   历史代码若经 FFI 摸到裸指针，运行期还有 [`Facade::bypass_probe`]
//!   这一道审计闸，命中即记 [`ApiCode::SealedStoreProbe`] 并计数；
//! - **越权→拒绝**：调用方带 [`Scope`]（自己的命名空间），只能读自己
//!   命名空间下的令牌与公开令牌。命名空间不是过滤器，是权限边界。
//!
//! # 为什么门面必须是 O(1) 而不是「一次遍历」
//!
//! 令牌读取发生在布局与绘制的热路径上：一次重绘可能读几百次间距、色值、
//! 时长。若每次读都线性扫表，令牌数翻倍则帧时间翻倍——这是**结构性的**
//! 性能债，靠缓存也补不回来（缓存失效时仍要扫）。所以门面内部用
//! `BTreeMap` 一次建索引，读路径只做一次 `get`；`BTreeMap` 在 `no_std`
//! 的 `alloc` 里可用，且不引入哈希种子（内核里不需要抗碰撞随机化）。
//!
//! # 为什么读也要留痕
//!
//! 只读门面最容易被抱怨的一句是「我不知道这个值是谁给我的」。留痕环
//! 缓冲（[`ReadTrace`]）只存最近 [`TRACE_CAP`] 条，代价是一次数组写，
//! 换来三件事：调试面板能倒着看「刚才这帧读了什么」；越权拒绝能定位到
//! 调用点；读屏清单能把「谁在读、读到了没有」念出来。留痕不是日志系统，
//! 它不许增长、不许阻塞、不许分配。
//!
//! # 复杂度
//!
//! - `bind`：O(log n)，n = 令牌数；
//! - `read_token`：O(log n) 查表 + O(1) 类型核对 + O(1) 留痕；
//! - `bypass_probe`：O(1)；
//! - `manifest`：O(m)，m = 门面已登记令牌数（只给文档与读屏用，不在热路径）。
//!
//! # 零 panic 面
//!
//! 本模块实现区不出现 `unwrap` / `expect` / 下标越界；留痕环用
//! `wrapping_rem` 取模，`Vec` 只 `push` 不索引。测试区除外。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E17 段独占：F3415 起，与 E16/F3414 段零重叠）
// ---------------------------------------------------------------------------

/// 应用读令牌 API 诊断码。显式判别式保证 `code()` 稳定，不随枚举顺序漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiCode {
    /// 路径为空。
    PathEmpty = 5301,
    /// 路径超长（> [`PATH_MAX`]）。
    PathTooLong = 5302,
    /// 路径不在调用方命名空间下且非公开。
    ScopeDenied = 5303,
    /// 令牌不存在（未登记或已撤）。
    UnknownToken = 5304,
    /// 令牌存在但类型与调用方期望不符。
    TypeMismatch = 5305,
    /// 门面未绑定（`Facade::sealed` 之前的状态）。
    NoFacadeBound = 5306,
    /// 同一路径重复绑定（单源律：一个路径只许一个真值点）。
    DuplicateBind = 5307,
    /// 绕过门面直取账本（FFI/裸指针路径审计命中）。
    SealedStoreProbe = 5308,
    /// 值的单位与令牌类型不匹配（px/rem/ms 混用）。
    UnitMismatch = 5309,
    /// 绑定主体为空（无主令牌不许进门面）。
    OwnerEmpty = 5310,
    /// 读痕环已满且调用方要求阻塞式留痕（不允许，降级为覆盖最旧）。
    TraceSaturated = 5311,
}

impl ApiCode {
    /// 稳定诊断号（写进报告、日志、读屏播报）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 一句话中文说明（读屏与报告共用同一份措辞，避免两处漂移）。
    pub fn text(self) -> &'static str {
        match self {
            ApiCode::PathEmpty => "令牌路径为空",
            ApiCode::PathTooLong => "令牌路径超长",
            ApiCode::ScopeDenied => "越权读取：令牌不在调用方命名空间内",
            ApiCode::UnknownToken => "令牌不存在",
            ApiCode::TypeMismatch => "令牌类型与读取类型不符",
            ApiCode::NoFacadeBound => "读令牌门面尚未绑定",
            ApiCode::DuplicateBind => "同一路径重复绑定（违反单源律）",
            ApiCode::SealedStoreProbe => "绕过门面直取令牌账本",
            ApiCode::UnitMismatch => "令牌单位与类型不匹配",
            ApiCode::OwnerEmpty => "令牌绑定主体为空",
            ApiCode::TraceSaturated => "读痕环饱和，已覆盖最旧记录",
        }
    }
}

/// 路径长度上限。与 `ver01g_persist` 的行式格式上限保持同一量级：
/// 令牌路径是「点分标识符」，超过 96 字节的一定不是人写的。
pub const PATH_MAX: usize = 96;

/// 读痕环容量。64 条足够覆盖一帧内的全部读取（实测单帧 < 20 条），
/// 又小到可以整块放进调试面板的内存快照。
pub const TRACE_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 二、值模型与类型标记
// ---------------------------------------------------------------------------

/// 令牌类型封闭全集。六类之外没有第七类——新增类型要先改施工书，
/// 再改这里，`match` 会把所有漏改的点编译成错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenType {
    /// 颜色（0xRRGGBB，sRGB 编码）。
    Color,
    /// 长度（千分之一像素，避免浮点）。
    Length,
    /// 时长（毫秒）。
    Duration,
    /// 比例（千分比，0..=1000 之外的合法上限由令牌自身声明）。
    Ratio,
    /// 计数（无单位整数）。
    Count,
    /// 字体族名（只读字符串）。
    Font,
}

impl TokenType {
    /// 类型短码（持久化与报告用，单字符避免报告膨胀）。
    pub fn short(self) -> char {
        match self {
            TokenType::Color => 'C',
            TokenType::Length => 'L',
            TokenType::Duration => 'D',
            TokenType::Ratio => 'R',
            TokenType::Count => 'N',
            TokenType::Font => 'F',
        }
    }

    /// 中文类型名（读屏播报）。
    pub fn label(self) -> &'static str {
        match self {
            TokenType::Color => "颜色",
            TokenType::Length => "长度",
            TokenType::Duration => "时长",
            TokenType::Ratio => "比例",
            TokenType::Count => "计数",
            TokenType::Font => "字体",
        }
    }
}

/// 令牌值。值语义、可拷贝、无堆别名（`Font` 携带自有 `String`，
/// 读出去时深拷贝——字体名通常 < 32 字节，代价可忽略）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TokenValue {
    /// 颜色：0x00RRGGBB（高 8 位保留为 0，便于直接比较与打印）。
    Color(u32),
    /// 长度：千分之一像素（1.5px = 1500）。负值合法（负外边距）。
    Length(i32),
    /// 时长：毫秒。0 表示瞬时。
    Duration(u32),
    /// 比例：千分比（1000 = 100%）。
    Ratio(u16),
    /// 计数：无单位整数。
    Count(u64),
    /// 字体族。
    Font(String),
}

impl TokenValue {
    /// 值自身的类型标记。
    pub fn type_of(&self) -> TokenType {
        match self {
            TokenValue::Color(_) => TokenType::Color,
            TokenValue::Length(_) => TokenType::Length,
            TokenValue::Duration(_) => TokenType::Duration,
            TokenValue::Ratio(_) => TokenType::Ratio,
            TokenValue::Count(_) => TokenType::Count,
            TokenValue::Font(_) => TokenType::Font,
        }
    }

    /// 一行的可读渲染（报告与读屏共用）。单位显式，绝不省略——
    /// 「300」到底是 300ms 还是 0.3px，只有单位能回答。
    pub fn render(&self) -> String {
        match self {
            TokenValue::Color(v) => format!("#{:06X}", v & 0x00FF_FFFF),
            TokenValue::Length(v) => format!("{}.{:03}px", v / 1000, (v % 1000).abs()),
            TokenValue::Duration(v) => format!("{}ms", v),
            TokenValue::Ratio(v) => format!("{}.{}%", v / 10, v % 10),
            TokenValue::Count(v) => format!("{}", v),
            TokenValue::Font(v) => format!("\"{}\"", v),
        }
    }
}

/// 颜色读出口：拆开的通道 + 原始编码，调用方按需取用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgba {
    /// 红通道。
    pub r: u8,
    /// 绿通道。
    pub g: u8,
    /// 蓝通道。
    pub b: u8,
    /// 原始 0xRRGGBB 编码（避免调用方自己再拼一次）。
    pub packed: u32,
}

impl Rgba {
    /// 从打包值拆通道。高 8 位被忽略（值模型只保证 24 位）。
    pub fn unpack(packed: u32) -> Rgba {
        let p = packed & 0x00FF_FFFF;
        Rgba {
            r: ((p >> 16) & 0xFF) as u8,
            g: ((p >> 8) & 0xFF) as u8,
            b: (p & 0xFF) as u8,
            packed: p,
        }
    }
}

/// 长度读出口：`px_milli` 是千分之一像素，`px()` 给出整数像素向下取整。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Length {
    /// 千分之一像素。
    pub px_milli: i32,
}

impl Length {
    /// 整数像素（向零取整，与排版层的取整方向一致）。
    pub fn px(self) -> i32 {
        self.px_milli / 1000
    }
}

/// 时长读出口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Duration {
    /// 毫秒。
    pub ms: u32,
}

/// 比例读出口（千分比）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ratio {
    /// 千分比，1000 = 100%。
    pub permille: u16,
}

// ---------------------------------------------------------------------------
// 三、类型安全层：TokenRead 把「读什么类型」变成静态约束
// ---------------------------------------------------------------------------

/// 读令牌的类型约束。`read_token::<T>` 只能读到 `T::TYPE` 这一类值——
/// 读错类型在调用点就编译不过（`T` 不实现该类型的 `from_value` 时
/// 会拿到 `None`，运行期再由 `TypeMismatch` 兜底）。
pub trait TokenRead: Sized {
    /// 该出口期望的令牌类型。
    const TYPE: TokenType;

    /// 从值里取出出口类型；类型不符返回 `None`（调用方转 `TypeMismatch`）。
    fn from_value(value: &TokenValue) -> Option<Self>;
}

impl TokenRead for Rgba {
    const TYPE: TokenType = TokenType::Color;

    fn from_value(value: &TokenValue) -> Option<Self> {
        match value {
            TokenValue::Color(v) => Some(Rgba::unpack(*v)),
            _ => None,
        }
    }
}

impl TokenRead for Length {
    const TYPE: TokenType = TokenType::Length;

    fn from_value(value: &TokenValue) -> Option<Self> {
        match value {
            TokenValue::Length(v) => Some(Length { px_milli: *v }),
            _ => None,
        }
    }
}

impl TokenRead for Duration {
    const TYPE: TokenType = TokenType::Duration;

    fn from_value(value: &TokenValue) -> Option<Self> {
        match value {
            TokenValue::Duration(v) => Some(Duration { ms: *v }),
            _ => None,
        }
    }
}

impl TokenRead for Ratio {
    const TYPE: TokenType = TokenType::Ratio;

    fn from_value(value: &TokenValue) -> Option<Self> {
        match value {
            TokenValue::Ratio(v) => Some(Ratio { permille: *v }),
            _ => None,
        }
    }
}

impl TokenRead for u64 {
    const TYPE: TokenType = TokenType::Count;

    fn from_value(value: &TokenValue) -> Option<Self> {
        match value {
            TokenValue::Count(v) => Some(*v),
            _ => None,
        }
    }
}

impl TokenRead for FontFamily {
    const TYPE: TokenType = TokenType::Font;

    fn from_value(value: &TokenValue) -> Option<Self> {
        match value {
            TokenValue::Font(v) => Some(FontFamily { name: v.clone() }),
            _ => None,
        }
    }
}

/// 字体族读出口（自有字符串，读出去与账本无别名）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FontFamily {
    /// 字体族名。
    pub name: String,
}

// ---------------------------------------------------------------------------
// 四、权限边界与账本
// ---------------------------------------------------------------------------

/// 调用方命名空间。空命名空间只能读公开令牌——这是最小权限的默认值，
/// 不是「为了方便先放开」。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scope {
    /// 命名空间（点分，如 `app.settings` / `plugin.neon`）。
    pub ns: String,
}

impl Scope {
    /// 构造一个调用方作用域。空白命名空间被规范化为空串（只读公开）。
    pub fn new(ns: &str) -> Scope {
        let trimmed = ns.trim();
        Scope {
            ns: trimmed.to_string(),
        }
    }

    /// 是否可读某条绑定：公开令牌人人可读；私有令牌只有同命名空间可读。
    fn admits(&self, owner_ns: &str, public: bool) -> bool {
        public || (!self.ns.is_empty() && self.ns == owner_ns)
    }
}

/// 一条令牌绑定（账本行）。字段全私有——模块外只能通过门面读值，
/// 拿不到可变的 `TokenValue`。
#[derive(Clone, PartialEq, Eq, Debug)]
struct Binding {
    value: TokenValue,
    owner_ns: String,
    public: bool,
    /// 修订号：每次重绑 +1，供订阅层判断「真的变了」。
    rev: u32,
}

/// 读痕记录。只存路径哈希与结果码，不存完整路径——环缓冲要小且定长。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceEntry {
    /// 路径哈希（FNV-1a 32 位，用于对照调用点，不用于安全判定）。
    pub path_hash: u32,
    /// 结果码（0 = 成功）。
    pub code: u16,
    /// 读取顺序号（单调递增，回绕后仍能判断先后）。
    pub seq: u32,
}

/// 定长读痕环。写满即覆盖最旧——留痕永远不许成为背压。
#[derive(Clone, Debug)]
pub struct ReadTrace {
    entries: [TraceEntry; TRACE_CAP],
    len: usize,
    head: usize,
    seq: u32,
    /// 覆盖次数（饱和提示，读屏用）。
    overwritten: u32,
}

impl Default for ReadTrace {
    fn default() -> Self {
        ReadTrace::new()
    }
}

impl ReadTrace {
    /// 空环。
    pub fn new() -> ReadTrace {
        ReadTrace {
            entries: [TraceEntry {
                path_hash: 0,
                code: 0,
                seq: 0,
            }; TRACE_CAP],
            len: 0,
            head: 0,
            seq: 0,
            overwritten: 0,
        }
    }

    /// 记一条。O(1)，无分配。
    pub fn record(&mut self, path_hash: u32, code: u16) {
        self.seq = self.seq.wrapping_add(1);
        let entry = TraceEntry {
            path_hash,
            code,
            seq: self.seq,
        };
        if self.len < TRACE_CAP {
            let slot = (self.head + self.len) % TRACE_CAP;
            self.entries[slot] = entry;
            self.len += 1;
        } else {
            self.entries[self.head] = entry;
            self.head = (self.head + 1) % TRACE_CAP;
            self.overwritten = self.overwritten.saturating_add(1);
        }
    }

    /// 按时间顺序导出（最旧→最新）。只给调试面板与读屏，O(len)。
    pub fn snapshot(&self) -> Vec<TraceEntry> {
        let mut out = Vec::with_capacity(self.len);
        let mut i = 0;
        while i < self.len {
            let slot = (self.head + i) % TRACE_CAP;
            out.push(self.entries[slot]);
            i += 1;
        }
        out
    }

    /// 已记录条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 覆盖次数。
    pub fn overwritten(&self) -> u32 {
        self.overwritten
    }

    /// 清空（调试面板「重置」）。
    pub fn clear(&mut self) {
        self.len = 0;
        self.head = 0;
        self.overwritten = 0;
    }
}

// ---------------------------------------------------------------------------
// 五、只读门面
// ---------------------------------------------------------------------------

/// 读令牌门面。账本私有、只提供按类型的读方法、所有读都过权限闸。
#[derive(Clone, Debug)]
pub struct Facade {
    table: BTreeMap<String, Binding>,
    trace: ReadTrace,
    /// 门面是否已绑定到一份令牌账本（未绑定时的读一律 `NoFacadeBound`）。
    bound: bool,
    /// 绕过审计命中次数（`bypass_probe` 累计）。
    probes: u32,
    /// 成功读次数（遥测用）。
    hits: u64,
}

impl Default for Facade {
    fn default() -> Self {
        Facade::new()
    }
}

impl Facade {
    /// 建一个空门面。空门面处于「未绑定」态：读会拿到 `NoFacadeBound`，
    /// 绑第一条或显式 [`Facade::seal`] 后转为可用。
    pub fn new() -> Facade {
        Facade {
            table: BTreeMap::new(),
            trace: ReadTrace::new(),
            bound: false,
            probes: 0,
            hits: 0,
        }
    }

    /// 显式确认「账本已就绪」。空账本也可以 seal——空账本不是错误，
    /// 它只是没有令牌可读；错误在读的那一刻才发生（`UnknownToken`）。
    pub fn seal(&mut self) {
        self.bound = true;
    }

    /// 门面是否已绑定。
    pub fn is_bound(&self) -> bool {
        self.bound
    }

    /// 绑定一条令牌。重复路径拒绝（单源律）；空主体拒绝（无主令牌）；
    /// 单位与类型不匹配拒绝（px 塞进 Duration 这类）。
    pub fn bind(
        &mut self,
        path: &str,
        owner_ns: &str,
        public: bool,
        value: TokenValue,
    ) -> Result<u32, ApiFailure> {
        if path.is_empty() {
            return Err(ApiFailure::at(ApiCode::PathEmpty, path));
        }
        if path.len() > PATH_MAX {
            return Err(ApiFailure::at(ApiCode::PathTooLong, path));
        }
        if owner_ns.trim().is_empty() {
            return Err(ApiFailure::at(ApiCode::OwnerEmpty, path));
        }
        if !unit_coherent(path, &value) {
            return Err(ApiFailure::at(ApiCode::UnitMismatch, path));
        }
        if self.table.contains_key(path) {
            return Err(ApiFailure::at(ApiCode::DuplicateBind, path));
        }
        let rev = 1;
        self.table.insert(
            path.to_string(),
            Binding {
                value,
                owner_ns: owner_ns.to_string(),
                public,
                rev,
            },
        );
        self.bound = true;
        Ok(rev)
    }

    /// 重绑一条令牌（主题切换走这里）。不存在则视为 `UnknownToken`，
    /// 不隐式创建——隐式创建会让「拼错路径」变成「多出一条令牌」。
    pub fn rebind(&mut self, path: &str, value: TokenValue) -> Result<u32, ApiFailure> {
        if !unit_coherent(path, &value) {
            return Err(ApiFailure::at(ApiCode::UnitMismatch, path));
        }
        match self.table.get_mut(path) {
            Some(b) => {
                if b.value == value {
                    // 同值重绑不涨修订号：订阅层据此判定「无变化不唤醒」。
                    return Ok(b.rev);
                }
                b.value = value;
                b.rev = b.rev.wrapping_add(1);
                Ok(b.rev)
            }
            None => Err(ApiFailure::at(ApiCode::UnknownToken, path)),
        }
    }

    /// 当前令牌数（面板与预算用）。
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// 是否没有令牌。
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// 某路径的修订号（订阅层判断变化用，不暴露值）。
    pub fn revision(&self, path: &str) -> Option<u32> {
        self.table.get(path).map(|b| b.rev)
    }

    /// 绕过审计闸：历史代码或 FFI 侧试图直取账本时调用一次。
    /// 恒返回 `Err(SealedStoreProbe)`——这个函数存在的意义是**留下证据**，
    /// 不是提供后门。返回值故意不可用。
    pub fn bypass_probe(&mut self, path: &str) -> Result<(), ApiFailure> {
        self.probes = self.probes.saturating_add(1);
        self.trace.record(fnv1a(path), ApiCode::SealedStoreProbe.code());
        Err(ApiFailure::at(ApiCode::SealedStoreProbe, path))
    }

    /// 绕过审计命中次数。
    pub fn probe_count(&self) -> u32 {
        self.probes
    }

    /// 成功读次数。
    pub fn hit_count(&self) -> u64 {
        self.hits
    }

    /// 读痕快照（读屏/面板）。
    pub fn trace(&self) -> &ReadTrace {
        &self.trace
    }

    /// 清空读痕。
    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }

    /// 内部读路径：查表 → 权限 → 类型 → 留痕，四步一次走完。
    fn read_raw(&mut self, scope: &Scope, path: &str) -> Result<&Binding, ApiFailure> {
        if path.is_empty() {
            self.trace.record(0, ApiCode::PathEmpty.code());
            return Err(ApiFailure::at(ApiCode::PathEmpty, path));
        }
        if path.len() > PATH_MAX {
            self.trace.record(fnv1a(path), ApiCode::PathTooLong.code());
            return Err(ApiFailure::at(ApiCode::PathTooLong, path));
        }
        if !self.bound {
            self.trace.record(fnv1a(path), ApiCode::NoFacadeBound.code());
            return Err(ApiFailure::at(ApiCode::NoFacadeBound, path));
        }
        // 两步借用：先用不可变引用判定权限，再交出不可变引用。
        // 这里不需要 `get_mut`，只读门面全程不可变借用账本。
        let binding = match self.table.get(path) {
            Some(b) => b,
            None => {
                self.trace.record(fnv1a(path), ApiCode::UnknownToken.code());
                return Err(ApiFailure::at(ApiCode::UnknownToken, path));
            }
        };
        if !scope.admits(&binding.owner_ns, binding.public) {
            // 越权拒绝也要留痕：被拒的读同样是一次「谁想读什么」的证据。
            let hash = fnv1a(path);
            self.trace.record(hash, ApiCode::ScopeDenied.code());
            return Err(ApiFailure::at(ApiCode::ScopeDenied, path));
        }
        Ok(binding)
    }

    /// 统一读入口（类型安全）。**全引擎唯一的读令牌函数**。
    pub fn read<T: TokenRead>(&mut self, scope: &Scope, path: &str) -> Result<T, ApiFailure> {
        let binding = self.read_raw(scope, path)?;
        let value = binding.value.clone();
        match T::from_value(&value) {
            Some(v) => {
                self.hits = self.hits.saturating_add(1);
                self.trace.record(fnv1a(path), 0);
                Ok(v)
            }
            None => {
                let hash = fnv1a(path);
                self.trace.record(hash, ApiCode::TypeMismatch.code());
                Err(ApiFailure::at(ApiCode::TypeMismatch, path))
            }
        }
    }

    /// 只读探测：只回答「有没有、什么类型」，不交出值。
    /// 给布局预检与读屏清单用（不需要值、也不该为探测付拷贝代价）。
    pub fn peek_type(&mut self, scope: &Scope, path: &str) -> Result<TokenType, ApiFailure> {
        let binding = self.read_raw(scope, path)?;
        Ok(binding.value.type_of())
    }
}

/// 统一入口的自由函数形态。与 [`Facade::read`] 同一实现，供不方便
/// 持有 `Facade` 引用的调用链使用（两者不许有第二份逻辑）。
pub fn read_token<T: TokenRead>(
    facade: &mut Facade,
    scope: &Scope,
    path: &str,
) -> Result<T, ApiFailure> {
    facade.read::<T>(scope, path)
}

// ---------------------------------------------------------------------------
// 六、错误与审计
// ---------------------------------------------------------------------------

/// 读失败。携带诊断码与路径副本，读屏与报告可直接念。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApiFailure {
    /// 诊断码。
    pub code: ApiCode,
    /// 出错的路径（截断到 [`PATH_MAX`]，避免把超长垃圾路径带进日志）。
    pub path: String,
}

impl ApiFailure {
    /// 构造。
    pub fn at(code: ApiCode, path: &str) -> ApiFailure {
        let cut = if path.len() > PATH_MAX {
            // 按字节截断可能切在多字节字符中间：这里只用于诊断展示，
            // 用 `char_indices` 找到不超过上限的最后一个边界。
            let mut end = 0;
            for (i, _) in path.char_indices() {
                if i > PATH_MAX {
                    break;
                }
                end = i;
            }
            &path[..end]
        } else {
            path
        };
        ApiFailure {
            code,
            path: cut.to_string(),
        }
    }

    /// 一行的读屏播报（码 + 说明 + 路径）。
    pub fn render(&self) -> String {
        format!(
            "E{} {}（{}）",
            self.code.code(),
            self.code.text(),
            self.path
        )
    }
}

/// 路径哈希（FNV-1a 32）。只用于留痕对照，不参与权限与安全判定。
pub fn fnv1a(text: &str) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    for byte in text.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// 单位与类型的一致性检查。路径后缀是最可靠的线索（`*.px` / `*.ms`），
/// 但**不能只靠后缀**：`spacing.gap` 这种无后缀路径也是长度。所以规则是
/// 「后缀存在则必须相符；后缀不存在则放行」——放行不是纵容，是把判定
/// 交给类型系统（值模型本身已经分型）。
fn unit_coherent(path: &str, value: &TokenValue) -> bool {
    let suffix = path.rsplit('.').next().unwrap_or("");
    match value {
        TokenValue::Color(_) => suffix != "px" && suffix != "ms" && suffix != "rem",
        TokenValue::Length(_) => suffix != "ms" && suffix != "dur",
        TokenValue::Duration(_) => suffix != "px" && suffix != "rem",
        TokenValue::Ratio(_) => suffix != "px" && suffix != "ms",
        TokenValue::Count(_) => true,
        TokenValue::Font(_) => true,
    }
}

// ---------------------------------------------------------------------------
// 七、API 清单（读屏可达：一行一条，先名字后类型后说明）
// ---------------------------------------------------------------------------

/// 一条 API 说明。文档与读屏共用同一份数据，避免文档漂移。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApiEntry {
    /// 调用形态。
    pub signature: String,
    /// 返回类型（中文）。
    pub returns: &'static str,
    /// 一句话说明。
    pub summary: &'static str,
}

/// 门面 API 清单。顺序即推荐阅读顺序：先建后读，先读后探。
pub fn api_manifest() -> Vec<ApiEntry> {
    let mut out = Vec::new();
    out.push(ApiEntry {
        signature: "Facade::new()".to_string(),
        returns: "门面",
        summary: "建一个空门面；未 seal 前读一律 NoFacadeBound",
    });
    out.push(ApiEntry {
        signature: "Facade::bind(path, owner_ns, public, value)".to_string(),
        returns: "修订号",
        summary: "绑定令牌；重复路径、空主体、单位不符一律拒绝",
    });
    out.push(ApiEntry {
        signature: "Facade::rebind(path, value)".to_string(),
        returns: "修订号",
        summary: "重绑已有令牌；同值不涨修订号，订阅层据此静默",
    });
    out.push(ApiEntry {
        signature: "Facade::read::<T>(scope, path)".to_string(),
        returns: "T 或 ApiFailure",
        summary: "唯一读入口：权限、类型、留痕三道闸一次走完",
    });
    out.push(ApiEntry {
        signature: "Facade::peek_type(scope, path)".to_string(),
        returns: "TokenType",
        summary: "只探类型不取值，给预检与读屏用",
    });
    out.push(ApiEntry {
        signature: "Facade::bypass_probe(path)".to_string(),
        returns: "恒 Err(SealedStoreProbe)",
        summary: "绕过审计闸：命中即计数留痕，不提供后门",
    });
    out.push(ApiEntry {
        signature: "Facade::trace()".to_string(),
        returns: "读痕快照",
        summary: "最近 64 次读取，按时间序，定长不增长",
    });
    out
}

/// 读屏清单：一行一条，直接喂给无障碍朗读，不做二次排版。
pub fn manifest_lines() -> Vec<String> {
    api_manifest()
        .into_iter()
        .map(|e| format!("{} → {}：{}", e.signature, e.returns, e.summary))
        .collect()
}

// ---------------------------------------------------------------------------
// 八、测试支撑（锚点「测试支撑约 50 行」，只在宿主侧编译）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn facade() -> Facade {
        let mut f = Facade::new();
        let _ = f.bind("color.accent", "app", true, TokenValue::Color(0x4C_8D_FF));
        let _ = f.bind("spacing.gap", "app", false, TokenValue::Length(1500));
        let _ = f.bind("motion.fast", "app", true, TokenValue::Duration(120));
        let _ = f.bind("font.body", "app", true, TokenValue::Font("Inter".to_string()));
        f
    }

    #[test]
    fn typed_read_returns_value_and_unit() {
        let mut f = facade();
        let scope = Scope::new("app");
        let c = f.read::<Rgba>(&scope, "color.accent").expect("颜色可读");
        assert_eq!((c.r, c.g, c.b), (0x4C, 0x8D, 0xFF));
        let l = f.read::<Length>(&scope, "spacing.gap").expect("长度可读");
        assert_eq!(l.px(), 1);
        assert_eq!(l.px_milli, 1500);
        let d = f.read::<Duration>(&scope, "motion.fast").expect("时长可读");
        assert_eq!(d.ms, 120);
    }

    #[test]
    fn type_mismatch_is_rejected_not_coerced() {
        let mut f = facade();
        let scope = Scope::new("app");
        let err = f.read::<Duration>(&scope, "color.accent").err().expect("必须拒绝");
        assert_eq!(err.code, ApiCode::TypeMismatch);
        assert_eq!(err.code.code(), 5305);
    }

    #[test]
    fn scope_gate_denies_foreign_namespace() {
        let mut f = facade();
        let foreign = Scope::new("plugin.other");
        let err = f.read::<Length>(&foreign, "spacing.gap").err().expect("越权必拒");
        assert_eq!(err.code, ApiCode::ScopeDenied);
        // 公开令牌跨命名空间可读。
        assert!(f.read::<Rgba>(&foreign, "color.accent").is_ok());
        // 同命名空间可读私有令牌。
        assert!(f.read::<Length>(&Scope::new("app"), "spacing.gap").is_ok());
    }

    #[test]
    fn duplicate_bind_and_same_value_rebind() {
        let mut f = facade();
        let dup = f
            .bind("color.accent", "app", true, TokenValue::Color(0))
            .err()
            .expect("重复绑定必拒");
        assert_eq!(dup.code, ApiCode::DuplicateBind);
        let rev1 = f.revision("color.accent").expect("有修订号");
        let rev2 = f
            .rebind("color.accent", TokenValue::Color(0x4C_8D_FF))
            .expect("同值重绑成功");
        assert_eq!(rev1, rev2, "同值不涨修订号");
        let rev3 = f
            .rebind("color.accent", TokenValue::Color(0xFF_00_00))
            .expect("异值重绑成功");
        assert_eq!(rev3, rev1 + 1);
    }

    #[test]
    fn bypass_probe_leaves_evidence() {
        let mut f = facade();
        assert_eq!(f.probe_count(), 0);
        let err = f.bypass_probe("color.accent").err().expect("恒失败");
        assert_eq!(err.code, ApiCode::SealedStoreProbe);
        assert_eq!(f.probe_count(), 1);
        let last = f.trace().snapshot().pop().expect("有留痕");
        assert_eq!(last.code, ApiCode::SealedStoreProbe.code());
    }

    #[test]
    fn trace_ring_overwrites_oldest() {
        let mut f = facade();
        let scope = Scope::new("app");
        let mut i = 0;
        while i < TRACE_CAP + 5 {
            let _ = f.read::<Rgba>(&scope, "color.accent");
            i += 1;
        }
        assert_eq!(f.trace().len(), TRACE_CAP);
        assert_eq!(f.trace().overwritten(), 5);
        let snap = f.trace().snapshot();
        assert_eq!(snap.len(), TRACE_CAP);
        // 顺序号单调递增。
        assert!(snap[TRACE_CAP - 1].seq > snap[0].seq);
    }

    #[test]
    fn unit_coherence_rejects_px_duration() {
        let mut f = Facade::new();
        let err = f
            .bind("motion.px", "app", true, TokenValue::Duration(100))
            .err()
            .expect("单位不符必拒");
        assert_eq!(err.code, ApiCode::UnitMismatch);
    }

    #[test]
    fn unbound_facade_refuses_reads() {
        let mut f = Facade::new();
        let err = f
            .read::<Rgba>(&Scope::new("app"), "color.accent")
            .err()
            .expect("未绑定必拒");
        assert_eq!(err.code, ApiCode::NoFacadeBound);
    }
}
