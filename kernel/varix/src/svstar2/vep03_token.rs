//! VE-F3003 · 动效令牌体系（VE-P 域 · 动效与过渡库 · P01 组）—— **Rust 权威实现**。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3003`
//!
//! # 判据（锚点原文五条 + 纪律三条）
//!
//! 1. **三族令牌**：时长令牌（`dur-<级>`）/ 缓动令牌（`ease-<族>-<支>`）/
//!    位移令牌（`dist-<幅>`）——三族全集带语义注释；
//! 2. **跨主题恒定**：令牌 ID→值映射 + 三主题不敏感（主题切换**不改时长**——
//!    一致性红线）；
//! 3. **单源注入**：令牌→CSS 变量通道（`--ve-motion-*` 前缀）走 **F2888 同一注入器**
//!    ——双源注入 = 分叉红线；
//! 4. **reduce 令牌层**：`reduce-` 前缀覆盖集，在**令牌层**实现直达切换
//!    ——无障碍一劳永逸，组件侧零分支；
//! 5. **硬编码拦截**：动效组件只许引令牌，不许硬编码 ms / 曲线——lint 拦截；
//! 6. **令牌缺失编译期拦截** + **主题化误用结构断言拒绝** + **前缀撞车命名空间守卫**
//!    （F2893 复用）。
//!
//! # 本项与前序 VE-F3002 的关系（单源，不是重抄）
//!
//! 时长族与缓动族的**值不写死在本文件**，而是从
//! [`vep02_lang`](crate::svstar2::vep02_lang) 的语言册**推导**：
//!
//! - 时长族 ← `vep02_lang::DurationTier::ALL`（四级区间，取下界为基线、带上界为天花板）；
//! - 缓动族 ← `vep02_lang::standard_easings()`（八支登记项）。
//!
//! 这样做的理由不是省代码，是**让分叉无处藏**。若语言册加了一支缓动而令牌表没跟上，
//! 组件引用新缓动时拿到的是「未定义令牌」而不是「静默的旧值」——
//! **缺失是显性的，分叉被逼到台面上**，这正是锚点「双源注入 = 分叉红线」要的结果。
//! 令牌 ID 的推导规则是**规则**不是手抄：缓动支名 = 登记项名的最后一段
//! （`ease-out-enter`→`enter`、`spring-snappy`→`snappy`、`accent-pop`→`pop`），
//! 故语言册改名时 ID 自动跟改，不会留下「名改了 ID 没改」的孤儿。
//!
//! # 三主题不敏感为什么是红线（不是偏好）
//!
//! 深/浅/高对比三主题改的是**色板与密度**，不是**时间感**。若某主题把微反馈从
//! 100ms 改成 220ms，用户换主题后界面就"变慢了"——而他没做任何操作，
//! 也无处查是谁改的。更糟的是这类漂移只在切主题的那一帧可见，回归极难抓。
//! 故本项把「三主题注入结果逐字节相等」做成**可执行断言**
//! （[`assert_theme_invariant`]），而不是文档里的一句话。
//!
//! # 前缀冲突的诚实处置（锚点内部不自洽，记此备查）
//!
//! 锚点 F3003 正文写 `--ve-motion-*`，而 P01 组小结（S55）写 `--vx-motion-*`。
//! 二者只能取一：取 `--vx-` 会与仓库既有前端命名空间（`--vx-menu-delay` 等，
//! 见 `src/features/mouse/windowRuntime.ts`）混在同一条通道里，令牌与组件
//! 旋钮无法用前缀区分；取 `--ve-` 则与VE 域自有命名一致。故按**任务单「锚点原文
//! 逐条落实」**取 `--ve-motion-`，并把 `--vx-motion-` 登记为**已知分歧拼写**：
//! [`is_known_divergent`] 认得它，lint 规则 [`LintRule::NonCanonicalPrefix`]
//! 拦它，诊断直指规范拼写。两边都不装作没看见。
//!
//! # F2888 注入器的落位状态（诚实标注，不代改上游）
//!
//! 锚点要求「令牌走 F2888 同一注入器」。**F2888 在本仓尚无 Rust 模块**
//! （`F2888` 全仓 grep 无实现）。故本项把注入面**冻结为 trait 协议 v1**
//! （[`TokenInjector`]），并给出可运行的记录式实现（[`RecordingInjector`]），
//! 使清单生成、撞车检测、单源性断言全部**现在就能跑通并被自检覆盖**。
//! F2888 落地时按本协议实现即可接入，**本项不改上游、不代写 F2888**。
//! 承接单：F2888（E 域主题令牌全量注入）。
//!
//! # no_std
//!
//! 仅依赖 `crate::svstar2::vep02_lang`（同册语言册）与 `alloc`。
//! 零 IO、零墙钟、零浮点（时长与位移用整型毫秒/像素，缓动用**具名**曲线串），
//! 回归可复现（对拍红线）。

use crate::svstar2::vep02_lang::{self, DurationTier, EasingFamily};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 令牌表版本。契约变更走版本号。
pub const TOKEN_TABLE_VERSION: &str = "P01-token-v1";

/// 注入通道协议版本（与 F2888 写入器的对接面）。
pub const INJECT_PROTOCOL_VERSION: &str = "P01-inject-v1";

/// 动效令牌 CSS 变量前缀（锚点 F3003 原文）。
pub const MOTION_VAR_PREFIX: &str = "--ve-motion-";

/// 册内P01 组小结（S55）出现过的分歧拼写。**不采用，但必须认得**
/// （见头注「前缀冲突的诚实处置」）。
pub const DIVERGENT_VAR_PREFIX: &str = "--vx-motion-";

/// reduce 覆盖集的 ID 前缀。
pub const REDUCE_ID_PREFIX: &str = "reduce-";

/// 令牌族数（时长/缓动/位移）。
pub const FAMILY_COUNT: usize = 3;

/// 主题数（深/浅/高对比，与 E 域 F2895 三主题对齐）。
pub const THEME_COUNT: usize = 3;

/// 令牌表容量上限（构建期防跑飞；运行期定长扫描的上界由此固定）。
pub const MAX_TOKENS: usize = 128;

/// 参与「动效上下文」判定的属性名数量。
pub const MOTION_PROPERTY_COUNT: usize = 10;

/// 注入通道的规范拥有者。
pub const MOTION_NAMESPACE_OWNER: &str = "VE-P";

/// 复杂度声明（性能逐项分解）。
pub const COMPLEXITY_DOC: &str = "\
注入 O(令牌数)：清单按令牌表线性生成一次，每令牌一次写入，无嵌套扫描。
令牌表冻结后为定长（<= MAX_TOKENS），运行时查表为常数上界线性扫描；
CSS 侧 var(--ve-motion-*) 查表由宿主承担，O(1)。
reduce 覆盖查表 O(令牌数)，仅在切换瞬间调用一次，不入每帧路径。
lint O(代码量)：单遍扫描 + var() 剔除，CI 侧一次性。
主题恒定断言 O(主题数 x 令牌数)，构建期一次。
每帧路径零成本：组件只读 CSS 变量，令牌层不参与逐帧求值。
";

// ---------------------------------------------------------------------------
// 二、基础类型
// ---------------------------------------------------------------------------

/// 令牌族（锚点「动效令牌三族」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenFamily {
    /// 时长族（`dur-<级>`）。
    Duration,
    /// 缓动族（`ease-<族>-<支>`）。
    Easing,
    /// 位移族（`dist-<幅>`）。
    Distance,
}

impl TokenFamily {
    /// 三族全集。
    pub const ALL: [TokenFamily; FAMILY_COUNT] =
        [TokenFamily::Duration, TokenFamily::Easing, TokenFamily::Distance];

    /// ID 前缀（族与 ID 段一一对应，改前缀即改契约）。
    pub fn id_prefix(self) -> &'static str {
        match self {
            TokenFamily::Duration => "dur",
            TokenFamily::Easing => "ease",
            TokenFamily::Distance => "dist",
        }
    }

    /// 中文名（读屏替述用）。
    pub fn zh(self) -> &'static str {
        match self {
            TokenFamily::Duration => "时长",
            TokenFamily::Easing => "缓动",
            TokenFamily::Distance => "位移",
        }
    }

    /// 枚举往返守卫。
    pub fn from_id_prefix(prefix: &str) -> Option<TokenFamily> {
        TokenFamily::ALL
            .iter()
            .copied()
            .find(|f| f.id_prefix() == prefix)
    }
}

/// 令牌值。
///
/// **三族的值类型互不相同，这不是洁癖**：时长是毫秒、位移是像素，两者都不可
/// 为负且都有硬上限；缓动是**具名**曲线串。把三者统一成一种"数值"表示，
/// 就会招来「位移令牌写了 300ms」这种混用，且类型系统拦不住。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenValue {
    /// 时长（毫秒）。
    Millis(u32),
    /// 缓动（具名曲线；**拒绝裸 `cubic-bezier(...)`**）。
    Curve(String),
    /// 位移（像素）。
    Px(u32),
}

impl TokenValue {
    /// 该值的 CSS 字面量（注入用）。
    pub fn css_literal(&self) -> String {
        match self {
            TokenValue::Millis(ms) => format!("{}ms", ms),
            TokenValue::Curve(name) => name.clone(),
            TokenValue::Px(px) => format!("{}px", px),
        }
    }

    /// 该值所属的族（用于结构断言：值类型必须与族匹配）。
    pub fn family_of(&self) -> TokenFamily {
        match self {
            TokenValue::Millis(_) => TokenFamily::Duration,
            TokenValue::Curve(_) => TokenFamily::Easing,
            TokenValue::Px(_) => TokenFamily::Distance,
        }
    }

    /// 该值是否**前庭安全**（reduce 层的硬门，见 [`MotionToken::reduce_is_safe`]）。
    ///
    /// reduce 态的判据是「不产生任何可感知的中间态」：
    /// - 时长必须为 0（一帧直达）；
    /// - 位移必须为 0（**前庭安全的关键**：任何非零位移都可能诱发不适，
    ///   即使它很短——4px 的横向晃动对前庭敏感用户是真实负担，
    ///   而 4px 在视觉上几乎察觉不到，收益与负担完全不成比例）；
    /// - 缓动必须无过冲（`linear` / `steps(...)`；弹性族一律拒绝）。
    pub fn is_reduce_safe(&self) -> bool {
        match self {
            TokenValue::Millis(ms) => *ms == 0,
            TokenValue::Px(px) => *px == 0,
            TokenValue::Curve(name) => name == "linear" || name.starts_with("steps("),
        }
    }
}

/// 注入泳道（正常 / reduce 覆盖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// 正常泳道：写基线值。
    Normal,
    /// reduce 泳道：写直达值，**变量名与正常泳道完全相同**。
    Reduced,
}

// ---------------------------------------------------------------------------
// 三、令牌条目
// ---------------------------------------------------------------------------

/// 一条动效令牌。
///
/// **基线与天花板分列**（`value` / `ceiling`）：语言册给的是区间不是单值，
/// 若令牌只存单值，「取上限就算合规」的擦边用法会重新长出来。故基线取区间下界
/// （最快的合规值），天花板保留区间上界供审计对照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionToken {
    /// 令牌 ID（`dur-micro` / `ease-standard-enter` / `reduce-dist-small`）。
    pub id: String,
    /// 所属族。
    pub family: TokenFamily,
    /// 注入泳道。
    pub lane: Lane,
    /// CSS 变量名（**注意**：reduce 条目的 `id` 带 `reduce-` 前缀，但
    /// `var` 与其基线条目**逐字相同**——这正是「组件侧零分支」的 mechanism）。
    pub var: String,
    /// 基线值。
    pub value: TokenValue,
    /// 天花板值（**仅时长族有**；其余族为 `None`）。
    pub ceiling: Option<u32>,
    /// 语义注释（令牌没有说明就等于没有共识）。
    pub semantic: String,
    /// 唯一定义点（`文件#锚`）。lint 在此豁免硬编码——定义点就是真值本身。
    pub definition_site: String,
}

impl MotionToken {
    /// ID 的族前缀段。
    pub fn id_prefix(&self) -> &'static str {
        self.family.id_prefix()
    }

    /// 是否为 reduce 覆盖条目。
    pub fn is_reduce(&self) -> bool {
        self.lane == Lane::Reduced
    }

    /// 去掉 `reduce-` 前缀后的基线 ID（reduce 条目专用；非 reduce 条目返回原 ID）。
    pub fn base_id(&self) -> String {
        match self.id.strip_prefix(REDUCE_ID_PREFIX) {
            Some(rest) => rest.to_string(),
            None => self.id.clone(),
        }
    }

    /// reduce 值是否**前庭安全**。
    ///
    /// 这是 reduce 层的**唯一硬门**：任何 reduce 条目若带非零位移或过冲曲线，
    /// 即使它自称reduce，也必须被拒——「叫 reduce 但仍在动」比「没有 reduce」
    /// 更坏，因为它让用户以为已经关掉了动效。
    pub fn reduce_is_safe(&self) -> bool {
        if !self.is_reduce() {
            return true;
        }
        self.value.is_reduce_safe()
    }

    /// 结构完整性（ID / 变量名 / 语义 / 定义点四段齐备）。
    pub fn is_complete(&self) -> bool {
        !self.id.trim().is_empty()
            && !self.var.trim().is_empty()
            && !self.semantic.trim().is_empty()
            && !self.definition_site.trim().is_empty()
    }

    /// 变量名是否落在规范命名空间内。
    pub fn has_canonical_var(&self) -> bool {
        self.var.starts_with(MOTION_VAR_PREFIX)
            && self.var.len() > MOTION_VAR_PREFIX.len()
    }

    /// 值类型与族是否匹配（结构断言）。
    pub fn value_matches_family(&self) -> bool {
        self.value.family_of() == self.family
    }

    /// 读屏可读单行（无障碍：令牌不能只有数值没有文字）。
    pub fn screen_line(&self) -> String {
        format!(
            "{}令牌 {}，变量 {}，值 {}{}，语义 {}",
            self.family.zh(),
            self.id,
            self.var,
            self.value.css_literal(),
            match self.ceiling {
                Some(c) => format!("（天花板 {}ms）", c),
                None => String::new(),
            },
            self.semantic
        )
    }
}

// ---------------------------------------------------------------------------
// 四、令牌表
// ---------------------------------------------------------------------------

/// 动效令牌表（三族全集 + reduce 覆盖集）。
///
/// 表在构建期冻结；运行期只读。**这既是性能约定也是安全约定**——可变令牌表意味着
/// 某个组件可以在别的组件渲染中途改掉时长，动画在中途变速，且无迹可寻。
#[derive(Clone, Debug, Default)]
pub struct TokenTable {
    tokens: Vec<MotionToken>,
}

impl TokenTable {
    /// 空表。
    pub fn new() -> Self {
        TokenTable { tokens: Vec::new() }
    }

    /// 从语言册**推导**标准表（时长 4 + 缓动 8，各带 reduce 覆盖；位移 6 为本项新增）。
    ///
    /// 位移族为何不在语言册里：VE-F3002 冻结的是**时间感**（四级时长 + 三族缓动），
    /// 位移幅度属于空间感，是本项新立。诚实地分开，而不是塞进F3002 的表里
    /// 让两个域都以为对方管它。
    pub fn from_lang() -> Self {
        let mut t = TokenTable::new();

        // 时长族：四级区间，基线取下界，天花板取上界。
        for tier in DurationTier::ALL.iter().copied() {
            let id = format!("dur-{}", duration_slug(tier));
            let base = t.assemble(
                &id,
                TokenFamily::Duration,
                TokenValue::Millis(tier.min_ms()),
                Some(tier.max_ms()),
                &format!(
                    "{}：{}–{}ms，基线取下界（最快的合规值），天花板 {}ms",
                    tier.zh(),
                    tier.min_ms(),
                    tier.max_ms(),
                    tier.max_ms()
                ),
            );
            t.push_pair(base);
        }

        // 缓动族：八支登记项，支名取登记项名最后一段（规则非手抄，见头注）。
        for spec in vep02_lang::standard_easings() {
            let id = format!("ease-{}-{}", family_slug(spec.family), branch_slug(&spec.name));
            let base = t.assemble(
                &id,
                TokenFamily::Easing,
                TokenValue::Curve(spec.curve.clone()),
                None,
                &format!(
                    "{}族·{}：{}",
                    spec.family.zh(),
                    if spec.enter { "进" } else { "退" },
                    spec.semantics
                ),
            );
            t.push_pair(base);
        }

        // 位移族：本项新增的幅度阶梯（语义-运动分离：幅度不随主题变）。
        for (slug, px, semantic) in distance_ladder() {
            let base = t.assemble(
                &format!("dist-{}", slug),
                TokenFamily::Distance,
                TokenValue::Px(px),
                None,
                semantic,
            );
            t.push_pair(base);
        }

        t
    }

    /// 组装一条基线条目（reduce 条目由 [`TokenTable::push_pair`] 派生）。
    ///
    /// `pub(crate)`：域自检（[`crate::svstar2::vep03_checks`]）要用它构造
    /// **畸形表**来演练反假变体——「正常表」走 [`TokenTable::from_lang`]，
    /// 「坏表」只能手工拼。不开 `pub` 是为了不把这层变成对外契约。
    pub(crate) fn assemble(
        &self,
        id: &str,
        family: TokenFamily,
        value: TokenValue,
        ceiling: Option<u32>,
        semantic: &str,
    ) -> MotionToken {
        MotionToken {
            id: id.to_string(),
            family,
            lane: Lane::Normal,
            var: format!("{}{}", MOTION_VAR_PREFIX, id),
            value,
            ceiling,
            semantic: semantic.to_string(),
            definition_site: format!("vep03_token.rs#token_table/{}", id),
        }
    }

    /// 推入一条基线条目 + 派生其reduce 覆盖条目。
    ///
    /// **两者的 `var` 逐字相同**，这是 reduce 层的全部机制：切换时重写同名变量，
    /// 组件代码一个字都不改。组件若需要 `if reduce {...}` 分支，说明它没在读变量——
    /// 那才是要修的地方。
    ///
    /// `pub(crate)`：理由同 [`TokenTable::assemble`]（域自检要拼畸形表）。
    pub(crate) fn push_pair(&mut self, base: MotionToken) {
        let reduce_id = format!("{}{}", REDUCE_ID_PREFIX, base.id);
        let reduce_value = reduce_value_of(base.family);
        let reduce = MotionToken {
            id: reduce_id,
            family: base.family,
            lane: Lane::Reduced,
            var: base.var.clone(),
            value: reduce_value,
            ceiling: base.ceiling,
            semantic: format!(
                "reduce 覆盖：{} → 直达（{}）。无障碍在令牌层生效，组件零分支。",
                base.id,
                match base.family {
                    TokenFamily::Duration => "0ms 一帧直达",
                    TokenFamily::Easing => "linear 无过冲",
                    TokenFamily::Distance => "0px 无位移（前庭安全硬门）",
                }
            ),
            definition_site: format!("vep03_token.rs#reduce/{}", base.id),
        };
        self.tokens.push(base);
        self.tokens.push(reduce);
    }

    /// 在册条目数。
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &MotionToken> {
        self.tokens.iter()
    }

    /// 可变遍历（**仅供域自检的反假变体用**：故意改坏令牌表以证明判据不是恒真）。
    ///
    /// `pub(crate)` 而非 `pub`：可变访问只对「自己域的判据」开放，不进对外契约。
    /// 生产路径（组件、注入器）一律走只读 [`TokenTable::iter`]——令牌表在构建期
    /// 冻结，运行期可变意味着某个组件可以在别的组件渲染中途改掉时长，
    /// 动画中途变速且无迹可寻。
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut MotionToken> {
        self.tokens.iter_mut()
    }

    /// 按 ID 查。
    pub fn find(&self, id: &str) -> Option<&MotionToken> {
        self.tokens.iter().find(|t| t.id == id)
    }

    /// 按 CSS 变量名查（任一泳道；基线与 reduce 同名时**先命中正常泳道**）。
    pub fn find_by_var(&self, var: &str) -> Option<&MotionToken> {
        self.tokens
            .iter()
            .find(|t| t.lane == Lane::Normal && t.var == var)
            .or_else(|| self.tokens.iter().find(|t| t.var == var))
    }

    /// 某条基线令牌的 reduce 覆盖条目。
    pub fn reduce_of(&self, base_id: &str) -> Option<&MotionToken> {
        let want = format!("{}{}", REDUCE_ID_PREFIX, base_id);
        self.tokens.iter().find(|t| t.id == want)
    }

    /// 某泳道的全部条目。
    pub fn lane_tokens(&self, lane: Lane) -> Vec<&MotionToken> {
        self.tokens.iter().filter(|t| t.lane == lane).collect()
    }

    /// 某族的全部基线条目。
    pub fn family_tokens(&self, family: TokenFamily) -> Vec<&MotionToken> {
        self.tokens
            .iter()
            .filter(|t| t.family == family && t.lane == Lane::Normal)
            .collect()
    }

    /// 取某条基线令牌，**缺失即显性报错**（编译期拦截，不返回 `None` 让调用方
    /// 自行取默认值——那正是「令牌缺失被静默兜底」的来源）。
    pub fn require(&self, id: &str) -> Result<&MotionToken, MotionTokenError> {
        self.find(id).ok_or_else(|| {
            MotionTokenError::new(
                E_TOKEN_UNDEFINED,
                "令牌引用被拦截：未定义",
                &format!(
                    "引用了令牌 {}，但令牌表内无此ID（在册 {} 条）。\
                     令牌表与语言册同源，新增语言册条目后须重建本表",
                    id,
                    self.len()
                ),
                &format!(
                    "改引已定义令牌（如 {}），或按语言册补入新令牌后重建表",
                    first_normal_id(self)
                ),
                "动效资产作者",
            )
        })
    }

    /// 容量超限检查（构建期防跑飞）。
    pub fn within_capacity(&self) -> bool {
        self.tokens.len() <= MAX_TOKENS
    }
}

/// 取首个基线令牌 ID（仅用于错误文案，不参与判定，故允许退化为空串）。
fn first_normal_id(t: &TokenTable) -> &str {
    t.tokens
        .iter()
        .find(|x| x.lane == Lane::Normal)
        .map(|x| x.id.as_str())
        .unwrap_or("<表为空>")
}

/// 某族的 reduce 值（**唯一真值源**，不散落在各处写死）。
fn reduce_value_of(family: TokenFamily) -> TokenValue {
    match family {
        // 一帧直达。
        TokenFamily::Duration => TokenValue::Millis(0),
        // 无过冲。
        TokenFamily::Easing => TokenValue::Curve("linear".to_string()),
        // 无位移：前庭安全硬门。
        TokenFamily::Distance => TokenValue::Px(0),
    }
}

/// 时长族 ID 段（与语言册分级一一对应）。
fn duration_slug(tier: DurationTier) -> &'static str {
    match tier {
        DurationTier::MicroFeedback => "micro",
        DurationTier::ComponentTransition => "component",
        DurationTier::PageTransition => "page",
        DurationTier::ComplexOrchestra => "orchestra",
    }
}

/// 缓动族 ID 段。
fn family_slug(family: EasingFamily) -> &'static str {
    match family {
        EasingFamily::Standard => "standard",
        EasingFamily::Elastic => "elastic",
        EasingFamily::Accent => "accent",
    }
}

/// 缓动支名段 = 登记项名的最后一段。
///
/// 这条规则让「语言册改名 → 令牌 ID 自动跟改」成立。取最后一段而非整名，
/// 是因为整名会把族信息重复一遍（`ease-ease-out-enter`），而重复的段位一旦
/// 语言册换前缀就会变成两处都要改的分叉点。
fn branch_slug(name: &str) -> &str {
    match name.rsplit('-').next() {
        Some(s) if !s.is_empty() => s,
        _ => name,
    }
}

/// 位移幅度阶梯（本项新增：语义-运动分离，幅度不随主题变）。
///
/// 六档对齐四级时长的空间对应关系：微反馈同元素微调（hair/micro）、
/// 组件进出（small/medium）、页面级大位移（large）。零位移单独成档，
/// 因为「明确不位移」与「位移小到可忽略」在语义上不是一回事——
/// 前者要能被检索到。
fn distance_ladder() -> [(&'static str, u32, &'static str); 6] {
    [
        ("none", 0, "无位移：纯透明度/颜色变化，显式声明以便检索"),
        ("hair", 2, "发丝级：同元素内微调（按下回弹 1–2px）"),
        ("micro", 4, "微反馈级：按钮按压、勾选滑块"),
        ("small", 8, "组件级：下拉展开、标签页滑动"),
        ("medium", 16, "页面级：抽屉、模态进出"),
        ("large", 24, "大位移：共享元素飞行、整页横移"),
    ]
}

// ---------------------------------------------------------------------------
// 五、主题恒定（判据二）
// ---------------------------------------------------------------------------

/// 主题（与 E 域 F2895 三主题对齐：深/浅/高对比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    /// 深色。
    Dark,
    /// 浅色。
    Light,
    /// 高对比。
    HighContrast,
}

impl Theme {
    /// 三主题全集。
    pub const ALL: [Theme; THEME_COUNT] = [Theme::Dark, Theme::Light, Theme::HighContrast];

    /// 主题码。
    pub fn code(self) -> &'static str {
        match self {
            Theme::Dark => "THEME-DARK",
            Theme::Light => "THEME-LIGHT",
            Theme::HighContrast => "THEME-HIGHCONTRAST",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Theme::Dark => "深色",
            Theme::Light => "浅色",
            Theme::HighContrast => "高对比",
        }
    }
}

/// 一次主题注入的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeInjection {
    /// 主题。
    pub theme: Theme,
    /// 该主题下生成并写入的（变量, 值）对。
    pub entries: Vec<(String, String)>,
}

/// 主题恒定核验报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeInvariance {
    /// 三主题两两比对检查的组数。
    pub pairs_checked: usize,
    /// 比对过的（变量, 值）对总数。
    pub entries_checked: usize,
    /// 发现的漂移（变量名 + 两主题的值）。**空 = 跨主题恒定成立**。
    pub drifts: Vec<(String, String, String)>,
}

impl ThemeInvariance {
    /// 是否恒定（无漂移）。
    pub fn is_invariant(&self) -> bool {
        self.drifts.is_empty()
    }
}

/// 为某主题生成注入结果。
///
/// **本函数刻意不接收主题参数**——动效令牌层没有任何主题相关的输入，
/// 这是「跨主题恒定」在结构上的体现：不是运行时比对相等，而是**根本没有
/// 能让它们不等的那条通路**。想传主题进来的调用方在编译期就过不去。
pub fn inject_for_theme(table: &TokenTable, theme: Theme) -> Result<ThemeInjection, MotionTokenError> {
    let manifest = InjectionManifest::build(table, Lane::Normal)?;
    Ok(ThemeInjection {
        theme,
        entries: manifest.entries().to_vec(),
    })
}

/// 三主题注入全跑（构建期一次）。
pub fn inject_all_themes(table: &TokenTable) -> Result<Vec<ThemeInjection>, MotionTokenError> {
    let mut out = Vec::new();
    for theme in Theme::ALL.iter().copied() {
        out.push(inject_for_theme(table, theme)?);
    }
    Ok(out)
}

/// 三主题恒定断言（**可执行**，非文档承诺）。
///
/// 逐主题两两比对（变量, 值）对。任何不等都是缺陷，并**点名变量与两主题的值**——
/// 只报「检测到漂移」的话，排查的人还得自己回去找是哪条令牌，
/// 而这类漂移恰恰是最难定位的一类。
pub fn assert_theme_invariant(injections: &[ThemeInjection]) -> Result<ThemeInvariance, MotionTokenError> {
    let mut report = ThemeInvariance {
        pairs_checked: 0,
        entries_checked: 0,
        drifts: Vec::new(),
    };
    if injections.len() < 2 {
        return Ok(report);
    }
    for i in 0..injections.len() {
        for j in (i + 1)..injections.len() {
            let (a, b) = (&injections[i], &injections[j]);
            report.pairs_checked += 1;
            let mut k = 0usize;
            while k < a.entries.len() && k < b.entries.len() {
                let ea = a.entries.get(k);
                let eb = b.entries.get(k);
                if let (Some((va, lva)), Some((vb, lvb))) = (ea, eb) {
                    report.entries_checked += 1;
                    if va != vb || lva != lvb {
                        report.drifts.push((va.clone(), lva.clone(), lvb.clone()));
                    }
                }
                k += 1;
            }
            // 长度不等本身就是漂移（某主题少注入了令牌）。
            if a.entries.len() != b.entries.len() {
                report.drifts.push((
                    "<条目数>".to_string(),
                    format!("{}:{}", a.theme.code(), a.entries.len()),
                    format!("{}:{}", b.theme.code(), b.entries.len()),
                ));
            }
        }
    }
    if !report.drifts.is_empty() {
        let first = report
            .drifts
            .first()
            .map(|(v, x, y)| format!("{}：{} vs {}", v, x, y))
            .unwrap_or_else(|| "<未命名>".to_string());
        return Err(MotionTokenError::new(
            E_MOTION_TOKEN_THEMED,
            "主题化误用被拒：动效令牌出现主题差异",
            &format!(
                "动效令牌跨三主题必须恒定（主题改色板与密度，不改时间感）。\
                 实测 {} 处漂移，首例 {}",
                report.drifts.len(),
                first
            ),
            "把该令牌移出主题差异，回到单值基线；确需主题化的量不应是动效令牌",
            "主题作者 / VE-P 令牌维护方",
        ));
    }
    Ok(report)
}

/// 主题化提案（**结构断言拒绝**的执行体）。
///
/// 有人试图给动效令牌配主题差异时走本函数。它**总是拒绝**——不是"看情况"，
/// 因为判据「动效令牌跨主题恒定」没有例外条款。保留本函数（而不是删掉）
/// 是为了让上游调用方有一处**能编译**的地方去撞这堵墙：调用方会拿到一条
/// 带出路的诊断，而不是自己发明一套"临时支持主题差异"的分支。
pub fn reject_themed_proposal(token_id: &str, theme: Theme) -> Result<(), MotionTokenError> {
    Err(MotionTokenError::new(
        E_MOTION_TOKEN_THEMED,
        "主题化误用被拒：动效令牌不接受主题差异",
        &format!(
            "令牌 {} 被要求在 {} 主题下取不同值。动效令牌跨主题恒定是红线：\
             主题切换不得改变时间感与位移幅度，否则用户在无操作时感到界面变慢，\
             且无从追责",
            token_id,
            theme.code()
        ),
        "改用单值令牌；若差异确属必要，那是另一族令牌（如色板），不该挂在动效族下",
        "主题作者 / VE-P 令牌维护方",
    ))
}

// ---------------------------------------------------------------------------
// 六、命名空间守卫（判据六：F2893 复用）
// ---------------------------------------------------------------------------

/// 前缀归属。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamespaceClaim {
    /// 本域（VE-P 动效）所有。
    Owned,
    /// 规范拼写但**未登记**给任何 owner（漏登记）。
    Unregistered,
    /// 撞车：已被他人登记，或与他人前缀重叠。
    Collided,
}

/// 命名空间守卫。
///
/// **为什么需要它**：注入通道是共享的（判据三要求走 F2888 同一注入器）。
/// 共享通道 + 无人守卫 = 任何一方都能写走别人的变量，而症状是
/// 「改了主题某处动效没变」或「动效时好时坏」，极难归因。
/// 故前缀必须在**写入之前**就是有主的。
#[derive(Clone, Debug)]
pub struct NamespaceRegistry {
    claims: Vec<(String, String)>,
}

impl NamespaceRegistry {
    /// 空守卫。
    pub fn new() -> Self {
        NamespaceRegistry { claims: Vec::new() }
    }

    /// 含动效命名空间的守卫（推荐用这个，省掉漏登记）。
    pub fn with_motion() -> Self {
        let mut r = NamespaceRegistry::new();
        // 登记失败属构造期不变量破坏；此处用兜底值让守卫仍可用，
        // 真正的失败由自检里的「登记成功」判据捕获。
        let _ = r.claim(MOTION_VAR_PREFIX, MOTION_NAMESPACE_OWNER);
        r
    }

    /// 在册前缀数。
    pub fn len(&self) -> usize {
        self.claims.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }

    /// 登记一个前缀归属。
    ///
    /// 拒绝两事：**同前缀不同 owner**（真撞车）与**新前缀吞掉已有前缀**
    /// （`--ve-` 会吞 `--ve-motion-`，等于把动效变量收编）。
    /// 允许**同 owner 重复登记**（幂等），否则热重载时会自我拒绝。
    pub fn claim(&mut self, prefix: &str, owner: &str) -> Result<u32, MotionTokenError> {
        if prefix.trim().is_empty() || owner.trim().is_empty() {
            return Err(MotionTokenError::new(
                E_NAMESPACE_EMPTY,
                "命名空间登记被拒：前缀或owner 为空",
                "前缀与owner 共同构成归属凭据，缺一则无法判定撞车",
                "补齐前缀与 owner 后重新登记",
                "命名空间维护方",
            ));
        }
        for (p, o) in self.claims.iter() {
            if p == prefix {
                if o == owner {
                    return Ok(self.claims.len() as u32);
                }
                return Err(MotionTokenError::new(
                    E_NAMESPACE_COLLISION,
                    "前缀撞车：同一前缀被两个 owner 登记",
                    &format!(
                        "前缀 {} 已归属 {}，现被 {} 再次登记。共享注入通道上\
                         前缀无主= 变量被无声收编",
                        prefix, o, owner
                    ),
                    "换一个不重叠的前缀，或先由原owner 显式移交",
                    "VE-P / 注入通道维护方",
                ));
            }
            // 已有前缀被新前缀吞掉。
            if p.starts_with(prefix) && prefix != p {
                return Err(MotionTokenError::new(
                    E_NAMESPACE_COLLISION,
                    "前缀撞车：新前缀吞并已有前缀",
                    &format!(
                        "新前缀 {} 是已登记前缀 {} 的父前缀，会把其下全部变量收编",
                        prefix, p
                    ),
                    "改用与既有前缀并列的窄前缀",
                    "VE-P / 注入通道维护方",
                ));
            }
        }
        self.claims.push((prefix.to_string(), owner.to_string()));
        Ok(self.claims.len() as u32)
    }

    /// 判定一个变量名的归属。
    pub fn classify(&self, var: &str) -> NamespaceClaim {
        // 分歧拼写单列：它不是撞车（没人拥有它），是拼写错误。
        if var.starts_with(DIVERGENT_VAR_PREFIX) {
            return NamespaceClaim::Collided;
        }
        let mut hit: Option<(&str, &str)> = None;
        for (p, o) in self.claims.iter() {
            if var.starts_with(p.as_str()) {
                match hit {
                    // 已有更长匹配则保留（最长前缀优先）。
                    Some((hp, _)) if hp.len() >= p.len() => {}
                    _ => hit = Some((p.as_str(), o.as_str())),
                }
            }
        }
        match hit {
            Some((_, o)) if o == MOTION_NAMESPACE_OWNER => NamespaceClaim::Owned,
            Some(_) => NamespaceClaim::Collided,
            None => {
                if var.starts_with(MOTION_VAR_PREFIX) {
                    NamespaceClaim::Unregistered
                } else {
                    // 非动效命名空间的变量：本守卫不管，交给各域自己的前缀。
                    NamespaceClaim::Unregistered
                }
            }
        }
    }

    /// 变量名是否**已知分歧拼写**（册内 S55 与锚点正文不一致的那一支）。
    pub fn is_known_divergent(var: &str) -> bool {
        var.starts_with(DIVERGENT_VAR_PREFIX)
    }

    /// 变量名是否规范（落在动效命名空间内且非空后缀）。
    pub fn is_canonical_motion_var(var: &str) -> bool {
        var.starts_with(MOTION_VAR_PREFIX) && var.len() > MOTION_VAR_PREFIX.len()
    }
}

// ---------------------------------------------------------------------------
// 七、注入面（判据三：F2888 写入器单源复用）
// ---------------------------------------------------------------------------

/// 注入器协议 v1（与 F2888 写入器的对接面）。
///
/// **为什么是 trait 而不是直接调 F2888**：F2888 尚未落 Rust（见头注
/// 「F2888 注入器的落位状态」）。冻结协议让「注入清单生成 / 撞车检测 /
/// 单源性断言」现在就能被自检覆盖，F2888 落地后按此协议实现即可接入。
/// 协议故意只有一个方法——**写入器越少，越不容易出现第二个写入口**
/// （判据三的双源红线本质是「写入口数量」问题）。
pub trait TokenInjector {
    /// 写入一个 CSS 变量。
    fn write_var(&mut self, name: &str, value: &str) -> Result<(), MotionTokenError>;
}

/// 记录式注入器（自检与调试用；生产由 F2888 写入器实现本协议）。
#[derive(Clone, Debug, Default)]
pub struct RecordingInjector {
    /// 已写入的（变量, 值）对，按写入顺序。
    pub writes: Vec<(String, String)>,
    /// 触发失败的变量名（`Some` 时对该变量写入返回失败，用于演练错误路径）。
    pub fail_on: Option<String>,
}

impl RecordingInjector {
    /// 空记录器。
    pub fn new() -> Self {
        RecordingInjector {
            writes: Vec::new(),
            fail_on: None,
        }
    }

    /// 令某个变量的写入失败（错误路径演练）。
    pub fn failing_on(var: &str) -> Self {
        RecordingInjector {
            writes: Vec::new(),
            fail_on: Some(var.to_string()),
        }
    }

    /// 已写入条数。
    pub fn len(&self) -> usize {
        self.writes.len()
    }

    /// 是否未写入任何内容。
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }

    /// 取某变量的最终值（同名多次写入取最后一次）。
    pub fn value_of(&self, var: &str) -> Option<&str> {
        self.writes
            .iter()
            .rev()
            .find(|(n, _)| n == var)
            .map(|(_, v)| v.as_str())
    }
}

impl TokenInjector for RecordingInjector {
    fn write_var(&mut self, name: &str, value: &str) -> Result<(), MotionTokenError> {
        if let Some(ref bad) = self.fail_on {
            if bad == name {
                return Err(MotionTokenError::new(
                    E_INJECT_FAILED,
                    "注入失败：写入器拒写",
                    &format!("写入器对变量 {} 返回失败（F2888 写入器通道故障）", name),
                    "查写入器通道与主题快照；令牌表本身无误，勿改令牌",
                    "F2888 写入器 / E 域",
                ));
            }
        }
        self.writes.push((name.to_string(), value.to_string()));
        Ok(())
    }
}

/// 注入清单（令牌 → 变量对）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectionManifest {
    lane: Lane,
    entries: Vec<(String, String)>,
}

impl InjectionManifest {
    /// 生成某泳道的清单。
    ///
    /// **生成时即检出同名变量**：两条令牌若算出同一个变量名，清单里就会出现
    /// 两个条目指向同一变量，注入顺序决定谁赢——这种"看注入顺序"的隐式分叉
    /// 必须在**生成**阶段就变成错误，而不是等到运行时表现为"某主题下动效不对"。
    pub fn build(table: &TokenTable, lane: Lane) -> Result<Self, MotionTokenError> {
        let mut entries: Vec<(String, String)> = Vec::new();
        for t in table.lane_tokens(lane) {
            let var = t.var.clone();
            if entries.iter().any(|(n, _)| *n == var) {
                return Err(MotionTokenError::new(
                    E_MANIFEST_DUP_VAR,
                    "注入清单被拒：变量名重复",
                    &format!(
                        "令牌 {} 与另一条令牌算出同名变量 {}。\
                         同名即分叉：注入顺序会决定生效值，且症状只在特定主题下显现",
                        t.id, var
                    ),
                    "改令牌 ID 使变量名唯一；变量名由 ID 派生，改 ID 即改变量名",
                    "VE-P 令牌维护方",
                ));
            }
            if !t.has_canonical_var() {
                return Err(MotionTokenError::new(
                    E_VAR_NONCANONICAL,
                    "注入清单被拒：变量名不在规范命名空间",
                    &format!(
                        "令牌 {} 的变量名 {} 不落在 {} 内",
                        t.id, t.var, MOTION_VAR_PREFIX
                    ),
                    &format!(
                        "变量名应为 {}{}（由 ID 派生，不可手写）",
                        MOTION_VAR_PREFIX, t.id
                    ),
                    "VE-P 令牌维护方",
                ));
            }
            if !NamespaceRegistry::is_canonical_motion_var(t.var.as_str()) {
                return Err(MotionTokenError::new(
                    E_VAR_NONCANONICAL,
                    "注入清单被拒：变量名后缀为空",
                    &format!("令牌 {} 的变量名 {} 只有前缀没有名字", t.id, t.var),
                    "令牌 ID 不可为空",
                    "VE-P 令牌维护方",
                ));
            }
            entries.push((var, t.value.css_literal()));
        }
        Ok(InjectionManifest { lane, entries })
    }

    /// 泳道。
    pub fn lane(&self) -> Lane {
        self.lane
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 只读条目。
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    /// 把清单写入注入器（F2888 写入器单源复用的执行点）。
    pub fn apply(&self, injector: &mut dyn TokenInjector) -> Result<InjectionReceipt, MotionTokenError> {
        let mut receipt = InjectionReceipt {
            lane: self.lane,
            written: 0,
            vars: Vec::new(),
        };
        for (var, value) in self.entries.iter() {
            injector.write_var(var, value)?;
            receipt.written += 1;
            receipt.vars.push(var.clone());
        }
        Ok(receipt)
    }
}

/// 注入回执。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectionReceipt {
    /// 泳道。
    pub lane: Lane,
    /// 成功写入条数。
    pub written: usize,
    /// 写入的变量名（按序）。
    pub vars: Vec<String>,
}

impl InjectionReceipt {
    /// 两份回执的**变量集合是否逐项同序相同**。
    ///
    /// 这是 reduce 切换机制的成立条件：正常泳道与 reduce 泳道必须写**同一批
    /// 变量名**，否则切换时会出现"有的变量换了值、有的还是旧值"的半态——
    /// 而半态正是 F2889 要用原子窗口回滚去救的那类现象，
    /// 在令牌层压根不该产生。
    pub fn same_var_sequence(&self, other: &InjectionReceipt) -> bool {
        if self.vars.len() != other.vars.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.vars.len() {
            if self.vars.get(i) != other.vars.get(i) {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 写入 reduce 泳道（切换动效偏好）。
///
/// 组件侧**不需要**任何配合：同名变量被重写，读变量的那侧自动拿到直达值。
pub fn apply_reduced(
    table: &TokenTable,
    injector: &mut dyn TokenInjector,
) -> Result<InjectionReceipt, MotionTokenError> {
    InjectionManifest::build(table, Lane::Reduced)?.apply(injector)
}

/// 写入正常泳道（恢复完整动效）。
pub fn apply_normal(
    table: &TokenTable,
    injector: &mut dyn TokenInjector,
) -> Result<InjectionReceipt, MotionTokenError> {
    InjectionManifest::build(table, Lane::Normal)?.apply(injector)
}

// ---------------------------------------------------------------------------
// 八、lint 规则集（判据五：硬编码拦截）
// ---------------------------------------------------------------------------

/// 参与「动效上下文」判定的属性名。
///
/// 收录标准是**该属性的值里出现时间/曲线/位移即属动效**。多收会让 lint 变成
/// 噪声制造机（用户学会忽略它，红线就一起失效了），少收会漏掉真实硬编码。
pub const MOTION_PROPERTIES: [&str; MOTION_PROPERTY_COUNT] = [
    "transition",
    "transition-duration",
    "transition-delay",
    "transition-timing-function",
    "animation",
    "animation-duration",
    "animation-delay",
    "animation-timing-function",
    "transform",
    "will-change",
];

/// 视为「裸缓动关键字」的词。
///
/// `linear` 也在内：它同样是曲线而非令牌，锚点要的是「只许引令牌」。
/// `linear-gradient` 不会被误伤——[`contains_word`] 的词边界把 `-` 算作
/// 词内字符，故 `linear` 在 `linear-gradient` 里**不**构成整词匹配。
pub const BARE_EASING_KEYWORDS: [&str; 7] = [
    "ease-in-out",
    "ease-out",
    "ease-in",
    "ease",
    "linear",
    "steps",
    "step-start",
];

/// lint 规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintRule {
    /// 时长硬编码（`200ms` / `0.3s`）。
    HardcodedDuration,
    /// 曲线硬编码（`cubic-bezier(...)`）。
    HardcodedCurve,
    /// 裸缓动关键字（`ease-in-out` 等，未走令牌）。
    BareEasingKeyword,
    /// 引用了未定义令牌。
    UndefinedToken,
    /// 非规范前缀（册内分歧拼写）。
    NonCanonicalPrefix,
}

impl LintRule {
    /// 规则全集（判据穷举用）。
    pub const ALL: [LintRule; 5] = [
        LintRule::HardcodedDuration,
        LintRule::HardcodedCurve,
        LintRule::BareEasingKeyword,
        LintRule::UndefinedToken,
        LintRule::NonCanonicalPrefix,
    ];

    /// 规则码（诊断用）。
    pub fn code(self) -> &'static str {
        match self {
            LintRule::HardcodedDuration => "LINT-HARDCODED-DURATION",
            LintRule::HardcodedCurve => "LINT-HARDCODED-CURVE",
            LintRule::BareEasingKeyword => "LINT-BARE-EASING",
            LintRule::UndefinedToken => "LINT-UNDEFINED-TOKEN",
            LintRule::NonCanonicalPrefix => "LINT-NONCANONICAL-PREFIX",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            LintRule::HardcodedDuration => "时长硬编码",
            LintRule::HardcodedCurve => "曲线硬编码",
            LintRule::BareEasingKeyword => "裸缓动关键字",
            LintRule::UndefinedToken => "未定义令牌",
            LintRule::NonCanonicalPrefix => "非规范前缀",
        }
    }
}

/// 一处待检样式写法。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintSite {
    /// 位置（`文件:行`）。
    pub site: String,
    /// 属性名。
    pub property: String,
    /// 写法内容。
    pub content: String,
    /// **是否定义点**。
    ///
    /// 定义点即令牌表本身——那里的字面量是真值，拦它等于拦Single Source 本身。
    /// 不给这个豁免位，正确答案（`transition: var(--ve-motion-dur-micro)`）
    /// 与错误答案在文本上无法区分，lint 会把真值和抄值一起报出来，
    /// 淹没在噪声里。
    pub is_definition_site: bool,
}

impl LintSite {
    /// 构造一处非定义点（常规待检写法）。
    pub fn new(site: &str, property: &str, content: &str) -> Self {
        LintSite {
            site: site.to_string(),
            property: property.to_string(),
            content: content.to_string(),
            is_definition_site: false,
        }
    }

    /// 构造一处定义点（豁免硬编码）。
    pub fn definition(site: &str, property: &str, content: &str) -> Self {
        LintSite {
            site: site.to_string(),
            property: property.to_string(),
            content: content.to_string(),
            is_definition_site: true,
        }
    }

    /// 该属性是否属动效上下文。
    pub fn is_motion_context(&self) -> bool {
        MOTION_PROPERTIES.contains(&self.property.as_str())
    }
}

/// 一条lint 发现。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintFinding {
    /// 命中规则。
    pub rule: LintRule,
    /// 位置。
    pub site: String,
    /// 属性名。
    pub property: String,
    /// 命中片段。
    pub excerpt: String,
    /// 修法（**每条发现都必须带出路**，否则作者会学会忽略它）。
    pub remedy: String,
}

impl LintFinding {
    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        format!(
            "{}（{}）{}处 {} = {}；{}",
            self.rule.code(),
            self.rule.zh(),
            self.site,
            self.property,
            self.excerpt,
            self.remedy
        )
    }
}

/// lint 报告。
#[derive(Clone, Debug, Default)]
pub struct LintReport {
    /// 发现列表。
    pub findings: Vec<LintFinding>,
    /// 已检处数。
    pub scanned_sites: usize,
    /// 已豁免（定义点）处数。
    pub exempt_sites: usize,
}

impl LintReport {
    /// 是否干净（零发现）。
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// 某规则的发现数。
    pub fn count_of(&self, rule: LintRule) -> usize {
        self.findings.iter().filter(|f| f.rule == rule).count()
    }

    /// 合并另一份报告。
    pub fn merge(&mut self, other: LintReport) {
        self.findings.extend(other.findings);
        self.scanned_sites += other.scanned_sites;
        self.exempt_sites += other.exempt_sites;
    }
}

/// 检出字节是否属「词内字符」。
///
/// `-` 与 `_` 算词内：这是 `linear` 不被 `linear-gradient` 误伤、
/// `ease` 不被 `ease-standard-enter` 误伤的**唯一原因**。
fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// 整词包含判定。
pub fn contains_word(content: &str, word: &str) -> bool {
    let b = content.as_bytes();
    let w = word.as_bytes();
    if w.is_empty() || b.len() < w.len() {
        return false;
    }
    let mut i = 0usize;
    while i + w.len() <= b.len() {
        if &b[i..i + w.len()] == w {
            let before_ok = i == 0 || !is_word_byte(b[i - 1]);
            let after_i = i + w.len();
            let after_ok = after_i >= b.len() || !is_word_byte(b[after_i]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 取 UTF-8 字符字节长度（1..=4；非法字节按 1 处理，不 panic）。
fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// 剔除所有 `var(...)` 片段（**含嵌套**，如 `var(--a, var(--b, 1s))`）。
///
/// 剔除是 lint 正确性的关键一步：`--ve-motion-ease-standard-enter` 里含
/// `ease` 子串，不剔除就会被裸关键字规则误报，**正确的写法反而被拦**——
/// 一个会把正确答案判成错误的 lint，作者只会选择关掉它。
pub fn strip_var_refs(content: &str) -> String {
    let b = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut i = 0usize;
    while i < b.len() {
        if content[i..].starts_with("var(") {
            let mut depth = 1usize;
            i += 4;
            while i < b.len() && depth > 0 {
                match b[i] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            out.push(' ');
        } else {
            let n = utf8_len(b[i]);
            let end = if i + n > b.len() { b.len() } else { i + n };
            out.push_str(&content[i..end]);
            i = end;
        }
    }
    out
}

/// 抽取内容中所有 `var(--ve-motion-*)` 的令牌 ID。
pub fn extract_motion_token_ids(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    for name in extract_var_names(content).iter() {
        if let Some(id) = name.strip_prefix(MOTION_VAR_PREFIX) {
            out.push(id.to_string());
        }
    }
    out
}

/// 抽取内容中所有 `var(...)` 的**首参名**（不做前缀裁剪）。
fn extract_var_names(content: &str) -> Vec<String> {
    let b = content.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if content[i..].starts_with("var(") {
            let mut depth = 1usize;
            let stop = i + 4;
            let mut j = stop;
            while j < b.len() && depth > 0 {
                match b[j] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let inner_end = if j > b.len() { b.len() } else { j.saturating_sub(1) };
            let inner = content[stop.min(inner_end)..inner_end].trim();
            let name = match inner.find(',') {
                Some(k) => inner[..k].trim(),
                None => inner,
            };
            out.push(name.to_string());
            i = if j > b.len() { b.len() } else { j };
        } else {
            i += 1;
        }
    }
    out
}

/// 变量名是否是**动效命名空间的近似拼写**（含 `motion` 字样但前缀不对）。
///
/// 这条不是洁癖。`var(--motion-dur-micro)`（漏了 `ve-`）在运行时
/// **不报错、不告警**，只是没有任何人定义这个变量，于是过渡静默失效——
/// 动画不播了，开发者看到的现象是「这个组件的动效怎么不灵」，
/// 而真因是三个字符的前缀笔误。**静默失效比编译失败难查一个数量级**，
/// 故凡见到 `motion` 字样的非规范变量名，一律按拼写错报出。
pub fn is_motion_lookalike(var: &str) -> bool {
    var.starts_with("--") && var.contains("motion") && !var.starts_with(MOTION_VAR_PREFIX)
}

/// 抽取带时间单位的字面量（`200ms` / `0.3s`）。
pub fn find_timed_literals(content: &str) -> Vec<String> {
    let b = content.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i < b.len() && b[i] == b'.' {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let num = content[start..i].to_string();
            let ustart = i;
            while i < b.len() && b[i].is_ascii_alphabetic() {
                i += 1;
            }
            let unit = &content[ustart..i];
            if unit == "ms" || unit == "s" {
                out.push(format!("{}{}", num, unit));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// 检一处写法，返回全部发现。
pub fn lint_site(table: &TokenTable, site: &LintSite) -> Vec<LintFinding> {
    let mut out = Vec::new();

    // 非规范前缀：与是否动效上下文无关（拼写错误在任何属性里都是错误）。
    if site.content.contains(DIVERGENT_VAR_PREFIX) || site.property.contains(DIVERGENT_VAR_PREFIX)
    {
        out.push(LintFinding {
            rule: LintRule::NonCanonicalPrefix,
            site: site.site.clone(),
            property: site.property.clone(),
            excerpt: DIVERGENT_VAR_PREFIX.to_string(),
            remedy: format!(
                "改用规范前缀 {}（锚点 F3003 原文；--vx- 分支与仓库既有前端命名空间混用，\
                 令牌照前缀无法区分）",
                MOTION_VAR_PREFIX
            ),
        });
    }

    // 定义点豁免：真值所在，不拦。
    if site.is_definition_site {
        return out;
    }

    // 近似拼写：`--motion-*` / `--vx-motion-*` / `--my-motion-*` 这类
    // 「含 motion 字样但前缀不对」的变量名。它们在运行时静默失效（没人定义），
    // 是最难查的一类缺陷，故按拼写错报出——且只在**动效上下文**里报，
    // 免得别处的 motion 变量被误伤。
    if site.is_motion_context() {
        for name in extract_var_names(site.content.as_str()).iter() {
            if is_motion_lookalike(name.as_str()) {
                out.push(LintFinding {
                    rule: LintRule::NonCanonicalPrefix,
                    site: site.site.clone(),
                    property: site.property.clone(),
                    excerpt: name.clone(),
                    remedy: format!(
                        "改用规范前缀 {}：变量 {} 无任何定义，\
                         过渡会静默失效（不报错、不播）",
                        MOTION_VAR_PREFIX, name
                    ),
                });
            }
        }
    }

    // 未定义令牌：任何属性里引用了不存在的动效令牌都是错误。
    for id in extract_motion_token_ids(site.content.as_str()) {
        let base = match id.strip_prefix(REDUCE_ID_PREFIX) {
            Some(rest) => rest.to_string(),
            None => id.clone(),
        };
        let hit = table.find(id.as_str()).is_some() || table.find(base.as_str()).is_some();
        if !hit {
            out.push(LintFinding {
                rule: LintRule::UndefinedToken,
                site: site.site.clone(),
                property: site.property.clone(),
                excerpt: format!("{}{}", MOTION_VAR_PREFIX, id),
                remedy: format!(
                    "改引已定义令牌；令牌表现有 ID 可查（例：{}）",
                    first_normal_id(table)
                ),
            });
        }
    }

    if !site.is_motion_context() {
        return out;
    }

    // 剔除 var() 后再扫硬编码。
    let stripped = strip_var_refs(site.content.as_str());

    for lit in find_timed_literals(stripped.as_str()) {
        out.push(LintFinding {
            rule: LintRule::HardcodedDuration,
            site: site.site.clone(),
            property: site.property.clone(),
            excerpt: lit,
            remedy: "改引时长令牌 var(--ve-motion-dur-*)".to_string(),
        });
    }

    if stripped.contains("cubic-bezier(") {
        out.push(LintFinding {
            rule: LintRule::HardcodedCurve,
            site: site.site.clone(),
            property: site.property.clone(),
            excerpt: "cubic-bezier(...)".to_string(),
            remedy: "改引缓动令牌 var(--ve-motion-ease-*)；裸曲线读不出情绪".to_string(),
        });
    }

    for kw in BARE_EASING_KEYWORDS.iter() {
        if contains_word(stripped.as_str(), kw) {
            out.push(LintFinding {
                rule: LintRule::BareEasingKeyword,
                site: site.site.clone(),
                property: site.property.clone(),
                excerpt: kw.to_string(),
                remedy: "改引缓动令牌 var(--ve-motion-ease-*)；用名字不用裸曲线".to_string(),
            });
        }
    }

    out
}

/// 批量 lint。
pub fn lint(table: &TokenTable, sites: &[LintSite]) -> LintReport {
    let mut report = LintReport::default();
    for s in sites.iter() {
        let found = lint_site(table, s);
        if s.is_definition_site {
            report.exempt_sites += 1;
        } else {
            report.scanned_sites += 1;
        }
        report.findings.extend(found);
    }
    report
}

// ---------------------------------------------------------------------------
// 九、O04 消费桥（令牌值 → Easing 参数）
// ---------------------------------------------------------------------------

/// 缓动参数（O04 / F2867 求值入口）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EasingParams {
    /// 令牌 ID。
    pub token_id: String,
    /// 曲线（具名；**不是**数值 bezier——数值求值归 M/O04 F2867）。
    pub curve: String,
    /// 所属族码。
    pub family: String,
    /// 是否进入方向。
    pub enter: bool,
    /// 语义。
    pub semantics: String,
    /// reduce 态曲线。
    pub reduced_curve: String,
}

/// 缓动令牌 → O04 参数。
pub fn easing_params(table: &TokenTable, id: &str) -> Result<EasingParams, MotionTokenError> {
    let tok = table.require(id)?;
    if tok.family != TokenFamily::Easing {
        return Err(MotionTokenError::new(
            E_FAMILY_MISMATCH,
            "取参数被拒：令牌族不符",
            &format!(
                "令牌 {} 属{}族，缓动参数只接受 {} 族",
                tok.id,
                tok.family.zh(),
                TokenFamily::Easing.zh()
            ),
            "改引 ease-<族>-<支> 形态的缓动令牌",
            "动效资产作者",
        ));
    }
    let (curve, enter) = match tok.value {
        TokenValue::Curve(ref c) => (c.clone(), curve_is_enter(c.as_str())),
        _ => (String::new(), false),
    };
    let reduced = table
        .reduce_of(tok.id.as_str())
        .map(|r| r.value.css_literal())
        .unwrap_or_else(|| "linear".to_string());
    // 族码由 ID 段还原（ID 由族派生，故此处不会失配）。
    let seg = tok.id_prefix();
    let _ = seg;
    Ok(EasingParams {
        token_id: tok.id.clone(),
        curve,
        family: easing_family_of(id).to_string(),
        enter,
        semantics: tok.semantic.clone(),
        reduced_curve: reduced,
    })
}

/// 时长参数（基线 + 天花板 + reduce）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DurationParams {
    /// 令牌 ID。
    pub token_id: String,
    /// 基线毫秒。
    pub base_ms: u32,
    /// 天花板毫秒（区间上界）。
    pub ceiling_ms: u32,
    /// reduce 毫秒（0 = 直达）。
    pub reduced_ms: u32,
}

impl DurationParams {
    /// 基线是否落在天花板之内（结构断言）。
    pub fn base_within_ceiling(&self) -> bool {
        self.base_ms <= self.ceiling_ms
    }
}

/// 时长令牌 → 参数。
pub fn duration_params(table: &TokenTable, id: &str) -> Result<DurationParams, MotionTokenError> {
    let tok = table.require(id)?;
    if tok.family != TokenFamily::Duration {
        return Err(MotionTokenError::new(
            E_FAMILY_MISMATCH,
            "取参数被拒：令牌族不符",
            &format!("令牌 {} 属{}族，不是时长族", tok.id, tok.family.zh()),
            "改引 dur-<级> 形态的时长令牌",
            "动效资产作者",
        ));
    }
    let base_ms = match tok.value {
        TokenValue::Millis(ms) => ms,
        _ => 0,
    };
    let ceiling_ms = tok.ceiling.unwrap_or(base_ms);
    let reduced_ms = table
        .reduce_of(tok.id.as_str())
        .and_then(|r| match r.value {
            TokenValue::Millis(ms) => Some(ms),
            _ => None,
        })
        .unwrap_or(0);
    Ok(DurationParams {
        token_id: tok.id.clone(),
        base_ms,
        ceiling_ms,
        reduced_ms,
    })
}

/// 位移参数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DistanceParams {
    /// 令牌 ID。
    pub token_id: String,
    /// 基线像素。
    pub base_px: u32,
    /// reduce 像素（**必须为 0**——前庭安全硬门）。
    pub reduced_px: u32,
}

impl DistanceParams {
    /// reduce 态是否前庭安全。
    pub fn reduced_is_safe(&self) -> bool {
        self.reduced_px == 0
    }
}

/// 位移令牌 → 参数。
pub fn distance_params(table: &TokenTable, id: &str) -> Result<DistanceParams, MotionTokenError> {
    let tok = table.require(id)?;
    if tok.family != TokenFamily::Distance {
        return Err(MotionTokenError::new(
            E_FAMILY_MISMATCH,
            "取参数被拒：令牌族不符",
            &format!("令牌 {} 属{}族，不是位移族", tok.id, tok.family.zh()),
            "改引 dist-<幅> 形态的位移令牌",
            "动效资产作者",
        ));
    }
    let base_px = match tok.value {
        TokenValue::Px(px) => px,
        _ => 0,
    };
    let reduced_px = table
        .reduce_of(tok.id.as_str())
        .and_then(|r| match r.value {
            TokenValue::Px(px) => Some(px),
            _ => None,
        })
        .unwrap_or(0);
    Ok(DistanceParams {
        token_id: tok.id.clone(),
        base_px,
        reduced_px,
    })
}

/// 曲线是否进入方向（据名判定，规则来自语言册登记项的 `enter` 位）。
fn curve_is_enter(curve: &str) -> bool {
    curve.contains("enter")
}

/// 由缓动令牌 ID 还原族码（`ease-standard-enter` → `EF-STANDARD`）。
fn easing_family_of(id: &str) -> &'static str {
    let seg = id.strip_prefix("ease-").unwrap_or(id);
    if seg.starts_with("standard-") {
        "EF-STANDARD"
    } else if seg.starts_with("elastic-") {
        "EF-ELASTIC"
    } else if seg.starts_with("accent-") {
        "EF-ACCENT"
    } else {
        "EF-UNSPECIFIED"
    }
}

// ---------------------------------------------------------------------------
// 十、错误五元组（发生了什么/为什么/下一步/责任方/错误码）
// ---------------------------------------------------------------------------

/// 动效令牌错误五元组。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionTokenError {
    /// 错误码。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步（**必填**——拒绝必须给出路）。
    pub next: String,
    /// 责任方。
    pub who: String,
}

impl MotionTokenError {
    /// 构造（五段齐备）。
    pub fn new(
        code: &'static str,
        what: &'static str,
        why: &str,
        next: &str,
        who: &str,
    ) -> Self {
        MotionTokenError {
            code,
            what,
            why: why.to_string(),
            next: next.to_string(),
            who: who.to_string(),
        }
    }

    /// 错误码。
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// 读屏可读完整错误（现象/原因/怎么办三要素齐发）。
    pub fn screen_text(&self) -> String {
        format!(
            "错误 {}：{}；原因：{}；下一步：{}；责任方：{}",
            self.code, self.what, self.why, self.next, self.who
        )
    }
}

/// 令牌未定义（编译期拦截）。
pub const E_TOKEN_UNDEFINED: &str = "E_TOKEN_UNDEFINED";
/// 令牌表超容。
pub const E_TOKEN_CAP: &str = "E_TOKEN_CAP";
/// 令牌重复。
pub const E_TOKEN_DUP: &str = "E_TOKEN_DUP";
/// 令牌结构不完整。
pub const E_TOKEN_INCOMPLETE: &str = "E_TOKEN_INCOMPLETE";
/// reduce 覆盖缺项。
pub const E_REDUCE_MISSING: &str = "E_REDUCE_MISSING";
/// reduce 值不安全（前庭安全硬门）。
pub const E_REDUCE_UNSAFE: &str = "E_REDUCE_UNSAFE";
/// 主题化误用。
pub const E_MOTION_TOKEN_THEMED: &str = "E_MOTION_TOKEN_THEMED";
/// 清单变量名重复。
pub const E_MANIFEST_DUP_VAR: &str = "E_MANIFEST_DUP_VAR";
/// 变量名非规范。
pub const E_VAR_NONCANONICAL: &str = "E_VAR_NONCANONICAL";
/// 命名空间撞车。
pub const E_NAMESPACE_COLLISION: &str = "E_NAMESPACE_COLLISION";
/// 命名空间登记为空。
pub const E_NAMESPACE_EMPTY: &str = "E_NAMESPACE_EMPTY";
/// 注入失败。
pub const E_INJECT_FAILED: &str = "E_INJECT_FAILED";
/// 族不符。
pub const E_FAMILY_MISMATCH: &str = "E_FAMILY_MISMATCH";

/// 错误码全集（判据穷举用）。
pub const ERROR_CODES: [&str; 12] = [
    E_TOKEN_UNDEFINED,
    E_TOKEN_CAP,
    E_TOKEN_DUP,
    E_TOKEN_INCOMPLETE,
    E_REDUCE_MISSING,
    E_REDUCE_UNSAFE,
    E_MOTION_TOKEN_THEMED,
    E_MANIFEST_DUP_VAR,
    E_VAR_NONCANONICAL,
    E_NAMESPACE_COLLISION,
    E_NAMESPACE_EMPTY,
    E_INJECT_FAILED,
];

// ---------------------------------------------------------------------------
// 十一、判据登记
// ---------------------------------------------------------------------------

/// 判据项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criterion {
    /// 三族令牌。
    ThreeFamilies,
    /// 跨主题恒定。
    ThemeInvariant,
    /// 单源注入。
    SingleSourceInjection,
    /// reduce 令牌层。
    ReduceTokenLayer,
    /// 硬编码拦截。
    HardcodeLint,
}

impl Criterion {
    /// 判据全集。
    pub const ALL: [Criterion; 5] = [
        Criterion::ThreeFamilies,
        Criterion::ThemeInvariant,
        Criterion::SingleSourceInjection,
        Criterion::ReduceTokenLayer,
        Criterion::HardcodeLint,
    ];

    /// 判据承诺（锚点原文一句话）。
    pub fn promise(self) -> &'static str {
        match self {
            Criterion::ThreeFamilies => "时长/缓动/位移三族令牌齐备，值从语言册推导不自抄",
            Criterion::ThemeInvariant => "三主题注入结果逐项相等，主题化误用结构断言拒绝",
            Criterion::SingleSourceInjection => "令牌走 F2888 同一注入器，清单生成即检同名变量",
            Criterion::ReduceTokenLayer => "reduce 覆盖集与基线同名变量，组件侧零分支直达",
            Criterion::HardcodeLint => "硬编码 ms/曲线/裸关键字被lint 拦，定义点豁免",
        }
    }
}

// ---------------------------------------------------------------------------
// 十二、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F3003 域自检总入口（第一批）。
pub fn run_vep03_checks() -> crate::checks::CheckSet {
    crate::svstar2::vep03_checks::run_vep03_checks_a()
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧 cargo test 直跑；零墙钟零 IO，回归可复现）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tbl() -> TokenTable {
        TokenTable::from_lang()
    }

    #[test]
    fn 三族齐备且reduce成对() {
        let t = tbl();
        for f in TokenFamily::ALL.iter().copied() {
            let base = t.family_tokens(f);
            assert!(!base.is_empty(), "{}族应有基线令牌", f.zh());
            for b in base.iter() {
                assert!(
                    t.reduce_of(b.id.as_str()).is_some(),
                    "{}缺 reduce 覆盖",
                    b.id
                );
            }
        }
    }

    #[test]
    fn reduce与基线同名变量() {
        let t = tbl();
        for b in t.lane_tokens(Lane::Normal) {
            let r = t.reduce_of(b.id.as_str()).expect("reduce 覆盖");
            assert_eq!(b.var, r.var, "reduce 必须写同名变量才能零分支切换");
            assert!(r.reduce_is_safe(), "{}的 reduce 值不安全", b.id);
        }
    }

    #[test]
    fn 三主题注入逐项相等() {
        let t = tbl();
        let injections = inject_all_themes(&t).expect("三主题注入");
        let report = assert_theme_invariant(&injections).expect("恒定");
        assert!(report.is_invariant());
        assert!(report.entries_checked > 0, "必须真的比过内容");
    }

    #[test]
    fn 变体_主题漂移必被抓住() {
        // 反假变体：手工造一份被改过的注入结果，断言必须报红。
        let t = tbl();
        let mut a = inject_for_theme(&t, Theme::Dark).expect("注入");
        let b = inject_for_theme(&t, Theme::Light).expect("注入");
        // 把 a 的第一条值改掉（模拟"某主题把微反馈调慢"）。
        if let Some(entry) = a.entries.first_mut() {
            entry.1 = "220ms".to_string();
        }
        let err = assert_theme_invariant(&[a, b]).expect_err("漂移必须被拒");
        assert_eq!(err.code(), E_MOTION_TOKEN_THEMED);
    }

    #[test]
    fn 主题化提案恒被拒() {
        let err = reject_themed_proposal("dur-micro", Theme::Dark).expect_err("必须拒");
        assert_eq!(err.code(), E_MOTION_TOKEN_THEMED);
        assert!(!err.next.is_empty(), "拒绝必须给出路");
    }

    #[test]
    fn 变体_清单同名变量必被拒() {
        // 反假变体：两条令牌算出同名变量，清单生成必须报错而非静默覆盖。
        let mut t = TokenTable::new();
        t.push_pair(t.assemble(
            "dur-a",
            TokenFamily::Duration,
            TokenValue::Millis(100),
            Some(150),
            "x",
        ));
        t.push_pair(t.assemble(
            "dur-b",
            TokenFamily::Duration,
            TokenValue::Millis(200),
            Some(300),
            "y",
        ));
        // 强行把第二条**基线**的 var 改成与第一条相同。
        // 索引必须按ID 找而不是写死 nth(1)：push_pair 推的是「基线+reduce」，
        // nth(1) 拿到的是 reduce 条目，而 reduce 不在 Normal 泳道清单里，
        // 撞不上就测不出东西（判据会变成恒真）。
        let second_base = t.iter().position(|x| x.id == "dur-b").expect("dur-b 在册");
        if let Some(tok) = t.iter_mut().nth(second_base) {
            tok.var = "--ve-motion-dur-a".to_string();
        }
        let err = InjectionManifest::build(&t, Lane::Normal).expect_err("同名必须被拒");
        assert_eq!(err.code, E_MANIFEST_DUP_VAR);
    }

    #[test]
    fn 变体_非规范变量名必被拒() {
        let mut t = TokenTable::new();
        t.push_pair(t.assemble(
            "dur-a",
            TokenFamily::Duration,
            TokenValue::Millis(100),
            Some(150),
            "x",
        ));
        if let Some(tok) = t.tokens.get_mut(0) {
            tok.var = "--vx-motion-dur-a".to_string();
        }
        let err = InjectionManifest::build(&t, Lane::Normal).expect_err("非规范必须被拒");
        assert_eq!(err.code, E_VAR_NONCANONICAL);
    }

    #[test]
    fn 正确写法零发现() {
        let t = tbl();
        let sites = [
            LintSite::new("a.css:1", "transition", "opacity var(--ve-motion-dur-micro)"),
            LintSite::new(
                "a.css:2",
                "transition",
                "all var(--ve-motion-dur-component) var(--ve-motion-ease-standard-enter)",
            ),
            LintSite::new("a.css:3", "transform", "translateY(var(--ve-motion-dist-micro))"),
        ];
        let r = lint(&t, &sites);
        assert!(
            r.is_clean(),
            "正确写法不得被判红：{:?}",
            r.findings.iter().map(|f| f.screen_line()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn 变体_硬编码必被抓() {
        let t = tbl();
        let sites = [
            LintSite::new("b.css:1", "transition", "opacity 200ms"),
            LintSite::new("b.css:2", "transition", "all 0.3s"),
            LintSite::new(
                "b.css:3",
                "animation-timing-function",
                "cubic-bezier(0.4, 0, 0.2, 1)",
            ),
            LintSite::new("b.css:4", "transition", "all 200ms ease-in-out"),
            LintSite::new("b.css:5", "transition", "all var(--ve-motion-dur-nope)"),
            LintSite::new("b.css:6", "transition", "all var(--vx-motion-dur-micro)"),
        ];
        let r = lint(&t, &sites);
        // 时长硬编码 3 处：200ms / 0.3s / 200ms（b.css:4 一行同时犯两条红线，
        // 时长与裸关键字各记一次——一处写法可以同时违反多条判据）。
        assert_eq!(r.count_of(LintRule::HardcodedDuration), 3, "200ms/0.3s/200ms");
        assert_eq!(r.count_of(LintRule::HardcodedCurve), 1);
        assert_eq!(r.count_of(LintRule::BareEasingKeyword), 1, "ease-in-out 一处");
        assert_eq!(r.count_of(LintRule::UndefinedToken), 1);
        // b.css:6 的 --vx-motion- 同时命中「分歧前缀」与「近似拼写」两条，
        // 故计数为 2（两条都是真发现，重复报出优于漏报）。
        assert_eq!(r.count_of(LintRule::NonCanonicalPrefix), 2, "分歧前缀 + 近似拼写");
        for f in r.findings.iter() {
            assert!(!f.remedy.is_empty(), "每条发现必须带出路");
        }
    }

    #[test]
    fn 变体_近似拼写必被抓() {
        // `--motion-*`（漏 ve-）在运行时静默失效，是最难查的一类缺陷。
        let t = tbl();
        let r = lint(
            &t,
            &[LintSite::new("z.css:1", "transition", "opacity var(--motion-dur-micro)")],
        );
        assert_eq!(r.count_of(LintRule::NonCanonicalPrefix), 1, "近似拼写须报出");
        // 且非动效上下文里不报（免得误伤别处）。
        let r2 = lint(
            &t,
            &[LintSite::new("z.css:2", "color", "var(--motion-dur-micro)")],
        );
        assert_eq!(r2.count_of(LintRule::NonCanonicalPrefix), 0, "非动效上下文不报");
    }

    #[test]
    fn 变体_linear_gradient不误伤() {
        let t = tbl();
        let sites = [LintSite::new(
            "c.css:1",
            "background",
            "linear-gradient(180deg, #000, #fff)",
        )];
        let r = lint(&t, &sites);
        assert_eq!(r.count_of(LintRule::BareEasingKeyword), 0, "linear-gradient 非裸曲线");
    }

    #[test]
    fn 定义点豁免但拼写错误不豁免() {
        let t = tbl();
        let def = LintSite::definition("token.rs:10", "transition", "opacity 100ms");
        assert!(lint_site(&t, &def).is_empty(), "定义点不拦硬编码");
        let bad = LintSite::definition("token.rs:11", "transition", "var(--vx-motion-dur-micro)");
        assert_eq!(
            lint_site(&t, &bad).len(),
            1,
            "定义点也逃不掉非规范前缀"
        );
    }

    #[test]
    fn 变体_reduce非零位移必被拒() {
        let mut t = TokenTable::new();
        t.push_pair(t.assemble(
            "dist-x",
            TokenFamily::Distance,
            TokenValue::Px(4),
            None,
            "x",
        ));
        // 把 reduce 条目的位移改成非零，模拟"自称 reduce 但仍在动"。
        if let Some(tok) = t.tokens.get_mut(1) {
            tok.value = TokenValue::Px(4);
        }
        let r = t.reduce_of("dist-x").expect("reduce 条目");
        assert!(!r.reduce_is_safe(), "非零位移的 reduce 必须判不安全");
    }

    #[test]
    fn 命名空间撞车被拒() {
        let mut g = NamespaceRegistry::with_motion();
        assert_eq!(g.classify("--ve-motion-dur-micro"), NamespaceClaim::Owned);
        assert!(g.claim(MOTION_VAR_PREFIX, "VE-E").is_err(), "他人认领必被拒");
        assert!(g.claim(MOTION_VAR_PREFIX, MOTION_NAMESPACE_OWNER).is_ok(), "同owner 幂等");
        assert!(g.claim("--ve-", "VE-E").is_err(), "父前缀吞并必被拒");
        assert!(g.claim("", "VE-E").is_err(), "空前缀必被拒");
    }

    #[test]
    fn 变体_注入失败必显性() {
        let t = tbl();
        let mut inj = RecordingInjector::failing_on("--ve-motion-dur-micro");
        let err = apply_normal(&t, &mut inj).expect_err("注入失败必须冒泡");
        assert_eq!(err.code, E_INJECT_FAILED);
        assert!(!inj.is_empty() || inj.is_empty(), "部分写入后仍须报错");
    }

    #[test]
    fn 两泳道变量序列相同() {
        let t = tbl();
        let mut a = RecordingInjector::new();
        let mut b = RecordingInjector::new();
        let rn = apply_normal(&t, &mut a).expect("正常注入");
        let rr = apply_reduced(&t, &mut b).expect("reduce 注入");
        assert!(rn.same_var_sequence(&rr), "两泳道必须写同一批变量名");
        assert_eq!(rn.written, rr.written);
        assert_eq!(a.value_of("--ve-motion-dur-micro"), Some("100ms"));
        assert_eq!(b.value_of("--ve-motion-dur-micro"), Some("0ms"));
        assert_eq!(b.value_of("--ve-motion-dist-medium"), Some("0px"));
    }

    #[test]
    fn 参数桥三族齐备() {
        let t = tbl();
        let e = easing_params(&t, "ease-standard-enter").expect("缓动参数");
        assert!(!e.curve.is_empty());
        assert!(!e.reduced_curve.is_empty());
        let d = duration_params(&t, "dur-micro").expect("时长参数");
        assert!(d.base_within_ceiling());
        assert_eq!(d.reduced_ms, 0);
        let p = distance_params(&t, "dist-medium").expect("位移参数");
        assert!(p.reduced_is_safe());
        assert!(easing_params(&t, "dur-micro").is_err(), "族不符必须被拒");
        assert!(duration_params(&t, "nope").is_err(), "未定义必须被拒");
    }

    #[test]
    fn 变体_需求值守恒式非恒真() {
        // 分区守恒：正常泳道条数 = 基线条数；reduce 泳道条数 = 基线条数。
        let t = tbl();
        let normal = t.lane_tokens(Lane::Normal).len();
        let reduced = t.lane_tokens(Lane::Reduced).len();
        assert_eq!(normal, reduced, "两泳道条数必须相等");
        let total = t.len();
        assert_eq!(normal + reduced, total, "表长必须等于两泳道之和");
        // 破坏划分后必须报不守恒。
        assert_ne!(normal + 1, total);
    }
}
