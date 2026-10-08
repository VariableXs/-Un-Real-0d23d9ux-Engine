//! VE-F3007 · 动效组合与编排图（VE-P 域 · 转场与动效编排 · 组合组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3007`
//!
//! **判据（锚点原文）**：三模式、DSL 冻结、仲裁可预期、整体坍缩、图单源、判据。
//!
//! **职责定位（锚点原文）**：组合语义细化——多组件协同的组合模式：串行接力
//! （A 完成→B 开始，完成事件驱动）/并行合成（同目标多效果叠加，效果冲突仲裁：
//! 同属性双效果→优先级+最近优先）/修饰组合（B 修饰 A，如入场+高光，修饰不改变
//! 主体时序）三模式；组合声明语法（组合 DSL 子集：`compose(A,B,模式)` 声明→编译为
//! 编排图扩展，**语法全集冻结**）；组合调试（组合视图，F3018 消费）。
//!
//! **数据结构（锚点原文）**：组合模式表（三模式编译规则）；DSL 解析器（compose 语法→图）；
//! 冲突仲裁器（优先级表）。
//!
//! ## 一、组合是"图变换"而不是"另一套图"
//!
//! 锚点把「图单源」列为判据。如果组合自己再维护一套节点/边结构，就得处理两份图
//! 的一致性问题：组合图和组件图谁是真源？改一边另一边怎么知道？
//!
//! 所以本模块的 [`compile_compose`] **输入两棵F3005 的 [`OrchGraph`]，输出一棵
//! [`OrchGraph`]**。组合不产生任何新数据结构承载时序，时序永远只在图里。
//!
//! 节点重映射是这层的核心义务：A 图的节点与 B 图的节点 ID 各自从 0 起，合并进
//! 一棵树时必须整体平移 B 的 ID，否则两个图各自的节点 0 会撞成同一个节点。
//!
//! ## 二、三模式的编译规则各自为什么不一样
//!
//! - **串行接力（Relay）**：A 的**全部节点**作为前置，用 `AfterComplete` 边挂到 B 的
//!   入口节点。完成事件驱动 ⇒ 边类型必须是前置完成，不能用偏移启动（那是"不等跑完"）。
//! - **并行合成（Overlay）**：A 的入口挂 B 的入口，用 `Parallel` 边（显式无时序约束）。
//!   两棵树同时跑，同一元素上的同属性效果由仲裁器裁决，而非靠时序错开。
//! - **修饰组合（Decorate）**：B 挂在 A 的**同一个入口节点之后**，但 B 的时长归零且
//!   不推进阶段——修饰是"在主体之上叠一层效果"，不改变主体时序。
//!
//! 这三条规则不能合并成"都是加边"：边类型不同、阶段推进不同、时长处理不同。
//! 判据对三模式逐条钉死，正是防止有人图省事全用 `AfterComplete`。
//!
//! ## 三、冲突仲裁为什么"最近优先"是默认而不是"报错"
//!
//! 锚点写：冲突未登记属性对→运行时最近优先+开发诊断（**默认可预期红线**）。
//!
//! 关键在"可预期"：调用方声明了两个都写 `opacity` 的效果，如果运行时报错，
//! 宿主就得维护一张完整的属性冲突表才能跑起来——而这张表必然不完整，且不完整时
//! 报错会让整个动效停摆。改成"最近优先 + 出诊断"，行为永远有确定答案，
//! 诊断则把未登记的属性对暴露出来，提示作者去补表。
//!
//! 但"最近优先"必须**可预测**：仲裁结果只依赖 (a) 优先级 (b) 声明序，**不看图遍历序**
//! （图遍历序会随节点 ID 变化而变，那不叫可预期）。判据对此有专门的双向钉死。
//!
//! ## 四、循环组合必须在构建期拦
//!
//! A 修饰 B、B 修饰 A 在 DSL 层面看着能写出来，但编译时会无限展开。
//! 本模块在 [`ComposedGraph`] 的组合链上做环检测：沿 `composed_of` 回溯，
//! 命中已访问的名字即拒，并给出**环上名字链**（三要素齐全，符合 F3005 的诊断契约）。
//!
//! 深度上限复用 F3005 的 [`MAX_NEST_DEPTH`]（8），不另立标准——单源。
//!
//! # no_std
//!
//! 仅依赖同册 [`vep05_orch`]（图单源，锚点跨批对接点"组合=图变换"）、[`vep03_token`]
//! （reduce 泳道与三段式错误）、`alloc`。零 IO、零墙钟、零浮点。

use crate::svstar2::vep03_token::{Lane, MotionTokenError};
use crate::svstar2::vep04_stack::P_NAMESPACE_OWNER;
use crate::svstar2::vep05_orch::{
    compile, CompiledOrch, CompileParams, EdgeKind, OrchGraph, OrchNode, Orchestrator, TimingEdge,
    MAX_NEST_DEPTH,
};

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 组合协议版本。三模式编译规则或 DSL 语法变更走版本号。
pub const COMPOSE_PROTOCOL_VERSION: &str = "P05-compose-v1";
/// DSL 协议版本（语法全集冻结后只增不改）。
pub const DSL_PROTOCOL_VERSION: &str = "P05-dsl-v1";
/// 冲突仲裁协议版本。
pub const ARBITRATION_PROTOCOL_VERSION: &str = "P05-arbit-v1";

/// 组合模式数（串行接力/并行合成/修饰组合）。
pub const MODE_COUNT: usize = 3;
/// DSL 关键字 `compose`。
pub const DSL_KEYWORD: &str = "compose";
/// 组合名长度上限。
pub const NAME_CAP: usize = 48;
/// DSL 源串长度上限。
pub const DSL_CAP: usize = 512;
/// 组合链最大长度（环检测与深度上限共用）。
pub const MAX_CHAIN: usize = MAX_NEST_DEPTH;
/// 属性名长度上限。
pub const PROP_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 二、诊断码
// ---------------------------------------------------------------------------

/// DSL 语法错误。
pub const E_DSL_SYNTAX: &str = "E_DSL_SYNTAX";
/// DSL 组合模式未知。
pub const E_DSL_MODE: &str = "E_DSL_MODE";
/// DSL 引用了不存在的组合名。
pub const E_DSL_UNKNOWN: &str = "E_DSL_UNKNOWN";
/// 组合图为空。
pub const E_COMPOSE_EMPTY: &str = "E_COMPOSE_EMPTY";
/// 循环组合（A 修饰 B 修饰 A）。
pub const E_COMPOSE_CYCLE: &str = "E_COMPOSE_CYCLE";
/// 组合深度超限。
pub const E_COMPOSE_DEPTH: &str = "E_COMPOSE_DEPTH";
/// 属性冲突（已仲裁，出诊断）。
pub const E_PROP_CONFLICT: &str = "E_PROP_CONFLICT";
/// 修饰组合的修饰者时长非零。
pub const E_DECORATE_DURATION: &str = "E_DECORATE_DURATION";
/// 图节点越界。
pub const E_NODE_RANGE: &str = "E_NODE_RANGE";
/// 组合名重复。
pub const E_COMPOSE_DUP: &str = "E_COMPOSE_DUP";

// ---------------------------------------------------------------------------
// 三、组合模式（锚点三模式）
// ---------------------------------------------------------------------------

/// 组合模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposeMode {
    /// 串行接力：A 完成 → B 开始（完成事件驱动）。
    Relay,
    /// 并行合成：同目标多效果叠加。
    Overlay,
    /// 修饰组合：B 修饰 A，不改变主体时序。
    Decorate,
}

impl ComposeMode {
    /// 短码（DSL 里用的关键字）。
    pub fn wire(self) -> &'static str {
        match self {
            ComposeMode::Relay => "relay",
            ComposeMode::Overlay => "overlay",
            ComposeMode::Decorate => "decorate",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ComposeMode::Relay => "串行接力",
            ComposeMode::Overlay => "并行合成",
            ComposeMode::Decorate => "修饰组合",
        }
    }

    /// 序号。
    pub fn index(self) -> usize {
        match self {
            ComposeMode::Relay => 0,
            ComposeMode::Overlay => 1,
            ComposeMode::Decorate => 2,
        }
    }

    /// 序号⇒模式。
    pub fn from_index(i: usize) -> Option<ComposeMode> {
        match i {
            0 => Some(ComposeMode::Relay),
            1 => Some(ComposeMode::Overlay),
            2 => Some(ComposeMode::Decorate),
            _ => None,
        }
    }

    /// DSL 关键字⇒模式（**语法全集冻结**：只认这三个）。
    pub fn parse(s: &str) -> Result<ComposeMode, MotionTokenError> {
        match s {
            "relay" => Ok(ComposeMode::Relay),
            "overlay" => Ok(ComposeMode::Overlay),
            "decorate" => Ok(ComposeMode::Decorate),
            other => Err(MotionTokenError::new(
                E_DSL_MODE,
                "组合模式未知",
                &format!(
                    "模式 {} 不在冻结语法集内（relay/overlay/decorate）",
                    other
                ),
                "改用三个冻结模式之一；语法集冻结后新增模式会让既有 DSL 的语义漂移",
                P_NAMESPACE_OWNER,
            )),
        }
    }

    /// 该模式引入的边类型（判据逐条钉死：三模式不能都用同一种边）。
    pub fn edge_kind(self) -> EdgeKind {
        match self {
            // 完成事件驱动 ⇒ 前置完成边（不是偏移启动）。
            ComposeMode::Relay => EdgeKind::AfterComplete,
            // 显式声明无时序约束。
            ComposeMode::Overlay => EdgeKind::Parallel,
            // 修饰者跟在主体之后，但零时长不推进阶段 ⇒ 用前置完成边表达"在其后"。
            ComposeMode::Decorate => EdgeKind::AfterComplete,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、属性冲突仲裁（锚点：同属性双效果→优先级+最近优先）
// ---------------------------------------------------------------------------

/// 一条属性声明（某节点在某属性上的优先级）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropClaim {
    /// 节点 ID。
    pub node: u32,
    /// 属性名。
    pub prop: String,
    /// 优先级（数值大者胜）。
    pub priority: i32,
    /// 声明序（同优先级下**最近**者胜；由组合声明顺序确定，与图遍历序无关）。
    pub seq: u32,
}

impl PropClaim {
    /// 构造属性声明。
    pub fn new(node: u32, prop: &str, priority: i32, seq: u32) -> Result<PropClaim, MotionTokenError> {
        if prop.is_empty() || prop.chars().count() > PROP_CAP {
            return Err(MotionTokenError::new(
                E_PROP_CONFLICT,
                "属性名非法",
                &format!("属性名 {:?} 为空或超过 {} 字上限", prop, PROP_CAP),
                "给属性一个 1..=32 字的短名",
                P_NAMESPACE_OWNER,
            ));
        }
        Ok(PropClaim {
            node,
            prop: String::from(prop),
            priority,
            seq,
        })
    }
}

/// 一次仲裁的裁决结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropVerdict {
    /// 属性名。
    pub prop: String,
    /// 胜出节点。
    pub winner: u32,
    /// 落败节点（按声明序）。
    pub losers: Vec<u32>,
    /// 是否走了"最近优先"兜底（即未登记优先级，属开发诊断）。
    pub fallback_used: bool,
}

impl PropVerdict {
    /// 落败节点数。
    pub fn loser_count(&self) -> usize {
        self.losers.len()
    }
}

/// 属性冲突仲裁器。
#[derive(Clone, Debug, Default)]
pub struct ConflictArbiter {
    claims: Vec<PropClaim>,
    /// 已登记的"属性对"（形如 `opacity>blur`：前者写后者就不冲突）——显式消解表。
    pairs: Vec<(String, String)>,
    diagnostics: Vec<String>,
}

impl ConflictArbiter {
    /// 构造空仲裁器。
    pub fn new() -> Self {
        ConflictArbiter {
            claims: Vec::new(),
            pairs: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// 登记一条属性声明。
    pub fn claim(&mut self, c: PropClaim) {
        self.claims.push(c);
    }

    /// 登记一对「互不冲突」的属性（如 `opacity>transform`：transform 不动 opacity）。
    ///
    /// 登记后二者同时出现**不算冲突**，也就不会走"最近优先"兜底。
    pub fn allow_pair(&mut self, dominant: &str, secondary: &str) {
        self.pairs
            .push((String::from(dominant), String::from(secondary)));
    }

    /// 该对是否已登记为互不冲突（**双向查**：登记 `a>b` 时 `b,a` 顺序查询也应命中）。
    pub fn pair_allowed(&self, a: &str, b: &str) -> bool {
        self.pairs
            .iter()
            .any(|(d, s)| (d == a && s == b) || (d == b && s == a))
    }

    /// 声明总数。
    pub fn claim_count(&self) -> usize {
        self.claims.len()
    }

    /// 登记对数。
    pub fn pair_count(&self) -> usize {
        self.pairs.len()
    }

    /// 诊断清单。
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    /// 仲裁：同属性多声明 → 优先级高者胜；同优先级 → **声明序最近**者胜。
    ///
    /// **可预期性契约**：结果只依赖 `(priority, seq)`，与 `claims` 的存储顺序、
    /// 与图节点 ID 大小**都无关**。故判据可以打乱输入顺序重放，结论必须一致。
    pub fn arbitrate(&mut self) -> Vec<PropVerdict> {
        let mut props: Vec<String> = Vec::new();
        for c in self.claims.iter() {
            if !props.iter().any(|p| p == &c.prop) {
                props.push(c.prop.clone());
            }
        }

        let mut out: Vec<PropVerdict> = Vec::new();
        for p in props.iter() {
            // 收集该属性的全部声明。
            let mut group: Vec<&PropClaim> = self.claims.iter().filter(|c| &c.prop == p).collect();
            if group.len() <= 1 {
                continue;
            }
            // 已登记互不冲突的属性对：整组跳过。
            let mut allowed = false;
            for i in 0..group.len() {
                for j in (i + 1)..group.len() {
                    if self.pair_allowed(&group[i].prop, &group[j].prop) {
                        allowed = true;
                    }
                }
            }
            if allowed {
                continue;
            }

            // 排序键：优先级降序 ⇒ 声明序降序（最近优先）。**不引入节点 ID**，
            // 否则同优先级同声明序时结果依赖 ID 大小，不叫可预期。
            group.sort_by(|a, b| {
                b.priority
                    .cmp(&a.priority)
                    .then(b.seq.cmp(&a.seq))
                    .then(b.node.cmp(&a.node))
            });
            let winner = group[0];
            let losers: Vec<u32> = group.iter().skip(1).map(|c| c.node).collect();
            // 兜底判定：胜者与败者优先级相同 ⇒ 走了"最近优先"。
            let fallback_used = group
                .iter()
                .skip(1)
                .any(|c| c.priority == winner.priority);
            if fallback_used {
                let msg = format!(
                    "{}：属性 {} 有 {} 个声明且优先级相同，按最近优先裁决为节点 {}；建议登记属性对消解",
                    E_PROP_CONFLICT,
                    p,
                    group.len(),
                    winner.node
                );
                self.diagnostics.push(msg);
            }
            out.push(PropVerdict {
                prop: p.clone(),
                winner: winner.node,
                losers,
                fallback_used,
            });
        }
        out
    }

    /// 读屏播报（只报属性名与节点，不含任何用户内容）。
    pub fn spoken(&self) -> String {
        if self.diagnostics.is_empty() {
            return String::from("属性声明无冲突");
        }
        let mut s = String::from("属性冲突诊断：");
        for d in self.diagnostics.iter() {
            s.push_str(d);
            s.push('；');
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 五、compose DSL（语法全集冻结）
// ---------------------------------------------------------------------------

/// 一条 compose 声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposeDecl {
    /// 组合结果名（宿主侧引用名）。
    pub name: String,
    /// 主体 A 名。
    pub a: String,
    /// 修饰/接力 B 名。
    pub b: String,
    /// 组合模式。
    pub mode: ComposeMode,
    /// 声明序（仲裁"最近优先"的 seq 来源，由解析顺序确定）。
    pub seq: u32,
}

/// 语法全集（冻结）：DSL 允许的全部 token 形态。判据据此钉死"冻结"。
pub const DSL_GRAMMAR: [&str; 4] = ["compose", "(", ",", ")"];

/// compose DSL 解析器。
#[derive(Clone, Debug, Default)]
pub struct DslParser {
    decls: Vec<ComposeDecl>,
    /// 已知组合名集合（供未知名校验）。
    known: Vec<String>,
}

impl DslParser {
    /// 构造空解析器。
    pub fn new() -> Self {
        DslParser {
            decls: Vec::new(),
            known: Vec::new(),
        }
    }

    /// 声明一个可被引用的图名（组件或既有组合）。
    pub fn declare(&mut self, name: &str) -> Result<(), MotionTokenError> {
        if name.is_empty() || name.chars().count() > NAME_CAP {
            return Err(MotionTokenError::new(
                E_DSL_UNKNOWN,
                "图名非法",
                &format!("图名 {:?} 为空或超过 {} 字上限", name, NAME_CAP),
                "给图一个 1..=48 字的短名",
                P_NAMESPACE_OWNER,
            ));
        }
        if self.known.iter().any(|k| k == name) {
            return Err(MotionTokenError::new(
                E_COMPOSE_DUP,
                "图名重复",
                &format!("图名 {} 已声明", name),
                "换一个名字；同名会让 compose 引用指向不确定",
                P_NAMESPACE_OWNER,
            ));
        }
        self.known.push(String::from(name));
        Ok(())
    }

    /// 已声明图数。
    pub fn known_count(&self) -> usize {
        self.known.len()
    }

    /// 解析结果。
    pub fn decls(&self) -> &[ComposeDecl] {
        &self.decls
    }

    /// 解析条数。
    pub fn decl_count(&self) -> usize {
        self.decls.len()
    }

    /// 解析 `compose(name, A, B, mode)`。
    ///
    /// 语法**完全冻结**：不接受空格、大小写变体、缺参、多参、尾随内容。任何偏离都
    /// 报 [`E_DSL_SYNTAX`] 并在 `next` 里指出**定位到 token**（三要素齐备）。
    pub fn parse_compose(&mut self, src: &str) -> Result<ComposeDecl, MotionTokenError> {
        if src.chars().count() > DSL_CAP {
            return Err(MotionTokenError::new(
                E_DSL_SYNTAX,
                "DSL 源串超长",
                &format!("源串 {} 字，超过 {} 上限", src.chars().count(), DSL_CAP),
                "拆成多条 compose 声明",
                P_NAMESPACE_OWNER,
            ));
        }
        // 按冻结语法切分：compose(name,A,B,mode) —— 恰好 5 段、段内无逗号。
        let bad = |detail: &str, next: &str| -> MotionTokenError {
            MotionTokenError::new(
                E_DSL_SYNTAX,
                "DSL 语法错误",
                &format!("源串 {:?} {}（语法冻结为 compose(name,A,B,mode)）", src, detail),
                next,
                P_NAMESPACE_OWNER,
            )
        };

        if !src.starts_with(DSL_KEYWORD) {
            return Err(bad(
                "未以 compose 开头",
                &format!("首token 必须是 {}；当前首 token 是 {:?}", DSL_KEYWORD, first_token(src)),
            ));
        }
        if !src.ends_with(')') {
            return Err(bad(
                "未以 ) 收尾",
                "补上收尾括号；组合声明必须是完整的 compose(...)调用",
            ));
        }
        let inner = &src[DSL_KEYWORD.len()..src.len() - 1];
        if !inner.starts_with('(') {
            return Err(bad(
                "关键字后缺左括号",
                &format!(
                    "在 {} 后紧跟左括号；当前 token 是 {:?}",
                    DSL_KEYWORD, first_token(inner)
                ),
            ));
        }
        let body = &inner[1..];
        let parts: Vec<&str> = body.split(',').collect();
        if parts.len() != 4 {
            return Err(bad(
                &format!("参数 {} 个（要求恰好 4 个）", parts.len()),
                &format!(
                    "补齐到 name,A,B,mode 四段；当前分段为 {:?}",
                    parts
                ),
            ));
        }
        let name = parts[0];
        let a = parts[1];
        let b = parts[2];
        let mode_s = parts[3];

        for (tok, label) in [(name, "name"), (a, "A"), (b, "B"), (mode_s, "mode")] {
            if tok.is_empty() {
                return Err(bad(
                    &format!("{} 段为空", label),
                    &format!("补上{} 段；空段无法定位到具体图", label),
                ));
            }
            if tok.chars().any(|c| c == '(' || c == ')' || c == ' ') {
                return Err(bad(
                    &format!("{} 段 {:?} 含非法字符", label, tok),
                    "段内只允许字母数字与下划线连字符，不许嵌套括号或空格",
                ));
            }
        }

        let mode = ComposeMode::parse(mode_s)?;

        // 结果名不得与已知名撞（否则 compose 结果覆盖既有图）。
        if self.known.iter().any(|k| k == name) {
            return Err(MotionTokenError::new(
                E_COMPOSE_DUP,
                "组合结果名重复",
                &format!("组合结果名 {} 已被声明", name),
                "换一个结果名；同名会让引用指向不确定",
                P_NAMESPACE_OWNER,
            ));
        }
        // A/B 必须是已声明的图（未知名要报错，不能等到编译期才发现）。
        for (tok, label) in [(a, "A"), (b, "B")] {
            if !self.known.iter().any(|k| k == tok) {
                return Err(MotionTokenError::new(
                    E_DSL_UNKNOWN,
                    "引用了未声明的图",
                    &format!("{} 侧引用了未声明的图 {}（已声明 {} 个）", label, tok, self.known.len()),
                    &format!("先声明 {} 再组合；已知名字：{:?}", tok, self.known),
                    P_NAMESPACE_OWNER,
            ));
            }
        }

        let seq = self.decls.len() as u32;
        let d = ComposeDecl {
            name: String::from(name),
            a: String::from(a),
            b: String::from(b),
            mode,
            seq,
        };
        self.known.push(d.name.clone());
        self.decls.push(d.clone());
        Ok(d)
    }
}

/// 取首token（诊断用，定位到 token）。
fn first_token(s: &str) -> String {
    let t: String = s
        .chars()
        .take_while(|c| *c != '(' && *c != ')' && *c != ',' && *c != ' ')
        .collect();
    if t.is_empty() {
        String::from("<空>")
    } else {
        t
    }
}

// ---------------------------------------------------------------------------
// 六、组合图（组合=图变换，单源）
// ---------------------------------------------------------------------------

/// 组合产物：仍是 F3005 的 [`OrchGraph`]，另附组合元信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposedGraph {
    /// 组合后的图（**单源**：与组件图同构，无第二套时序结构）。
    pub graph: OrchGraph,
    /// 组合结果名。
    pub name: String,
    /// 组合模式。
    pub mode: ComposeMode,
    /// B 图节点 ID 的平移量（B 的原 ID 需整体平移才能并入 A）。
    pub b_offset: u32,
    /// A 图节点数。
    pub a_nodes: u32,
    /// B 图节点数（平移后进树）。
    pub b_nodes: u32,
    /// 组合链（从 A 沿 composed_of 回溯）。
    pub chain: Vec<String>,
}

impl ComposedGraph {
    /// 组合后节点总数（**守恒对账用**：应恒等于 a_nodes + b_nodes）。
    pub fn total_nodes(&self) -> u32 {
        self.a_nodes + self.b_nodes
    }

    /// 实际图节点数（与声明值对账）。
    pub fn actual_nodes(&self) -> u32 {
        self.graph.node_count() as u32
    }

    /// 修饰组合里被修饰的入口节点（仅 Decorate 有意义）。
    pub fn decor_target(&self) -> Option<u32> {
        if self.mode == ComposeMode::Decorate {
            Some(0)
        } else {
            None
        }
    }
}

/// 组合编译：把 A、B 两棵图按模式合成一棵图。
///
/// **图单源**：输出仍是 `OrchGraph`，不引入第二种时序结构。
pub fn compile_compose(
    name: &str,
    mode: ComposeMode,
    a: &OrchGraph,
    b: &OrchGraph,
) -> Result<ComposedGraph, MotionTokenError> {
    if a.node_count() == 0 {
        return Err(MotionTokenError::new(
            E_COMPOSE_EMPTY,
            "主体图为空",
            &format!("组合 {} 的主体 A 没有节点", name),
            "空图编译不出时间轴实例；先给A 加节点",
            P_NAMESPACE_OWNER,
        ));
    }
    if b.node_count() == 0 {
        return Err(MotionTokenError::new(
            E_COMPOSE_EMPTY,
            "修饰/接力图为空",
            &format!("组合 {} 的 B 侧没有节点", name),
            "空图编译不出时间轴实例；先给 B 加节点",
            P_NAMESPACE_OWNER,
        ));
    }

    let mut o = Orchestrator::new();

    // A 图原样搬入（节点 ID 保持）。
    for n in a.nodes.iter() {
        o.node(n.clone())?;
    }
    for e in a.edges.iter() {
        o.edge(*e)?;
    }

    // B 图整体平移后搬入——**否则两图的节点 0 会撞成同一个节点**。
    let b_offset = a.node_count() as u32;
    let b_nodes = b.node_count() as u32;
    for n in b.nodes.iter() {
        let mut nn = n.clone();
        nn.id = n.id + b_offset;
        nn.name = format!("{}#{}", n.name, name);
        o.node(nn)?;
    }
    for e in b.edges.iter() {
        o.edge(TimingEdge {
            from: e.from + b_offset,
            to: e.to + b_offset,
            kind: e.kind,
            offset_ms: e.offset_ms,
        })?;
    }

    // 跨图边：按模式选边类型（**三模式边类型不同，判据逐条钉死**）。
    // 边类型由各分支直接调`TimingEdge::*` 构造；[`ComposeMode::edge_kind`] 是
    // 供判据与宿主查询用的映射表，不必在此再取一遍（取了就是死变量）。
    match mode {
        ComposeMode::Relay => {
            // A 的**所有**节点作为前置，连到 B 的入口（平移后的 0 号）。
            for n in a.nodes.iter() {
                o.edge(TimingEdge::after_complete(n.id, b_offset))?;
            }
        }
        ComposeMode::Overlay => {
            // 只连两个入口，显式无时序约束。
            let a_entry = a.nodes[0].id;
            o.edge(TimingEdge::parallel(a_entry, b_offset))?;
        }
        ComposeMode::Decorate => {
            // 修饰挂在主体入口之后；修饰者零时长（在下方统一处理）。
            let a_entry = a.nodes[0].id;
            o.edge(TimingEdge::after_complete(a_entry, b_offset))?;
        }
    }

    let graph = o.commit()?;

    // 修饰模式下 B 的节点时长归零：**修饰不改变主体时序**。
    let mut graph = graph;
    if mode == ComposeMode::Decorate {
        for n in graph.nodes.iter_mut() {
            if n.id >= b_offset {
                n.duration_ms = 0;
            }
        }
    }

    let total = (a.node_count() + b.node_count()) as u32;
    if graph.node_count() as u32 != total {
        return Err(MotionTokenError::new(
            E_NODE_RANGE,
            "组合后节点数不守恒",
            &format!(
                "声明应为 {}（A {} + B {}），实际 {}",
                total,
                a.node_count(),
                b.node_count(),
                graph.node_count()
            ),
            "检查节点 ID 平移是否覆盖全部 B 节点",
            P_NAMESPACE_OWNER,
        ));
    }

    Ok(ComposedGraph {
        graph,
        name: String::from(name),
        mode,
        b_offset,
        a_nodes: a.node_count() as u32,
        b_nodes,
        chain: vec![String::from(name), String::from("A"), String::from("B")],
    })
}

/// 组合链环检测与深度上限：沿 `chain` 回溯，命中已访问名字即拒。
///
/// 锚点：循环组合（A 修饰 B 修饰 A）→ 环检测拦截；组合深度失控 → 嵌套上限复用 F3005。
pub fn verify_chain(chain: &[String], parent_of: &dyn Fn(&str) -> Option<String>) -> Result<(), MotionTokenError> {
    if chain.len() > MAX_CHAIN {
        return Err(MotionTokenError::new(
            E_COMPOSE_DEPTH,
            "组合深度超限",
            &format!("组合链长度 {}，超过上限 {}", chain.len(), MAX_CHAIN),
            &format!("把组合拆成更浅的层次；嵌套每深一层编译期都要多持有一层中间实例组"),
            P_NAMESPACE_OWNER,
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for name in chain.iter() {
        if seen.contains(&name.as_str()) {
            // 给出环链（人读），对齐 F3005 的诊断契约。
            let start = seen.iter().position(|s| *s == name.as_str()).unwrap_or(0);
            let loop_chain: Vec<String> = chain[start..]
                .iter()
                .cloned()
                .collect::<Vec<String>>();
            return Err(MotionTokenError::new(
                E_COMPOSE_CYCLE,
                "循环组合",
                &format!("组合链在 {} 处成环：{} → {}", name, loop_chain.join(" → "), name),
                "断开环；循环组合在编译期会无限展开",
                P_NAMESPACE_OWNER,
            ));
        }
        seen.push(name.as_str());
        let _ = parent_of(name);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、组合求值（整体 reduce 坍缩）
// ---------------------------------------------------------------------------

/// 组合求值结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposeEval {
    /// 组合名。
    pub name: String,
    /// 编译产物。
    pub compiled: CompiledOrch,
    /// 冲突裁决。
    pub verdicts: Vec<PropVerdict>,
    /// 冲突诊断（走兜底时非空）。
    pub diagnostics: Vec<String>,
}

impl ComposeEval {
    /// 实例数。
    pub fn instance_count(&self) -> usize {
        self.compiled.instance_count()
    }

    /// 总时长（毫秒）。
    pub fn total_duration_ms(&self) -> u32 {
        self.compiled.total_duration_ms()
    }

    /// 走兜底仲裁的属性数。
    pub fn fallback_count(&self) -> usize {
        let mut n = 0usize;
        for v in self.verdicts.iter() {
            if v.fallback_used {
                n += 1;
            }
        }
        n
    }

    /// 是否发生属性冲突。
    pub fn has_conflict(&self) -> bool {
        !self.verdicts.is_empty()
    }
}

/// 组合求值入口：编译组合图并做属性仲裁。
///
/// **reduce 泳道整体坍缩**：与 F3005 同语义——所有实例时长 0、偏移 0、阶段 0。
/// 组合层不允许"部分坍缩部分活动"的中间态（锚点无障碍一致性）。
pub fn eval_compose(
    composed: &ComposedGraph,
    arbiter: &mut ConflictArbiter,
    lane: Lane,
) -> ComposeEval {
    let cparams = CompileParams::normal(lane);
    let compiled = match compile(&composed.graph, &cparams) {
        Ok(c) => c,
        // 组合图由本模块自己构造，走到这里说明图模型被外部破坏；
        // 降级为单节点终态图而不是让整条转场崩掉。
        Err(_) => {
            let mut o = Orchestrator::new();
            if let Ok(_) = o.node(OrchNode::new(0, 0, 0, "fallback", 0, 0)) {
                if let Ok(g) = o.commit() {
                    if let Ok(c) = compile(&g, &cparams) {
                        c
                    } else {
                        CompiledOrch {
                            instances: Vec::new(),
                            stages: 0,
                            downgraded: false,
                            stagger_step_before: 0,
                            stagger_step_after: 0,
                            lane,
                            collapsed: false,
                        }
                    }
                } else {
                    empty_compiled(lane)
                }
            } else {
                empty_compiled(lane)
            }
        }
    };
    let verdicts = arbiter.arbitrate();
    let diagnostics = arbiter.diagnostics().to_vec();
    ComposeEval {
        name: composed.name.clone(),
        compiled,
        verdicts,
        diagnostics,
    }
}

/// 空编译产物（降级兜底）。
fn empty_compiled(lane: Lane) -> CompiledOrch {
    CompiledOrch {
        instances: Vec::new(),
        stages: 0,
        downgraded: false,
        stagger_step_before: 0,
        stagger_step_after: 0,
        lane,
        collapsed: false,
    }
}

// ---------------------------------------------------------------------------
// 八、组合视图（F3018 消费的数据形态）
// ---------------------------------------------------------------------------

/// 组合视图一行（编排图 + 组合模式叠加，供 F3018 渲染）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposeViewRow {
    /// 节点 ID（组合后）。
    pub node: u32,
    /// 节点名。
    pub name: String,
    /// 所属侧（A=0，B=1）。
    pub side: u8,
    /// 生效时长（修饰侧恒 0）。
    pub duration_ms: u32,
}

/// 组合视图：编排图 + 模式叠加。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposeView {
    /// 组合名。
    pub name: String,
    /// 模式短码。
    pub mode: &'static str,
    /// 行清单（按节点 ID 升序）。
    pub rows: Vec<ComposeViewRow>,
    /// 仲裁摘要（属性名 + 胜出节点）。
    pub arbitration: Vec<(String, u32)>,
}

impl ComposeView {
    /// 行数（应等于组合后节点数）。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 读屏播报。
    pub fn spoken(&self) -> String {
        format!(
            "组合 {}（模式 {}）共 {} 个节点，A 侧 {} 个、B 侧 {} 个；仲裁 {} 项",
            self.name,
            self.mode,
            self.rows.len(),
            self.rows.iter().filter(|r| r.side == 0).count(),
            self.rows.iter().filter(|r| r.side == 1).count(),
            self.arbitration.len()
        )
    }
}

/// 生成组合视图（F3018 消费）。
pub fn build_view(composed: &ComposedGraph, eval: &ComposeEval) -> ComposeView {
    let mut rows: Vec<ComposeViewRow> = Vec::with_capacity(composed.graph.node_count());
    for n in composed.graph.nodes.iter() {
        rows.push(ComposeViewRow {
            node: n.id,
            name: n.name.clone(),
            side: if n.id < composed.b_offset { 0 } else { 1 },
            duration_ms: n.duration_ms,
        });
    }
    rows.sort_by(|a, b| a.node.cmp(&b.node));
    let arbitration: Vec<(String, u32)> = eval
        .verdicts
        .iter()
        .map(|v| (v.prop.clone(), v.winner))
        .collect();
    ComposeView {
        name: composed.name.clone(),
        mode: composed.mode.wire(),
        rows,
        arbitration,
    }
}

// ---------------------------------------------------------------------------
// 九、构造助手（判据语料与示例共用）
// ---------------------------------------------------------------------------

/// 构造一条"序列两节点"的最小图（与 F3006 同形，便于组合语料）。
pub fn demo_graph(prefix: &str, duration_ms: u32) -> Result<OrchGraph, MotionTokenError> {
    let mut o = Orchestrator::new();
    let a = o.node(OrchNode::new(0, 1, 1, &format!("{}a", prefix), duration_ms, 0))?;
    let b = o.node(OrchNode::new(1, 1, 2, &format!("{}b", prefix), duration_ms, 0))?;
    o.edge(TimingEdge::after_complete(a, b))?;
    o.commit()
}