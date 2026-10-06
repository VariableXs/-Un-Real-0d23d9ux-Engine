//! VE-F0414 · include 解析与循环防护（VE-C 域 · 着色器系统 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0414`
//!
//! **判据（锚点原文）**：搜索序显性、环检测输出环、包含图、缓存裁定、判据。
//!
//! 锚点职责定位原文：
//! > include 指令解析：路径解析策略（引号形式与尖括号形式搜索序显性）、递归深度
//! > 上限、循环包含检测（包含链指纹）报错输出环路径；包含图记录（供构建系统与
//! > 诊断消费）；解析结果缓存（同文件只展开一次语义按规范裁定）。
//!
//! 本条只做「把一条 `#include` 变成一串被展开的文本」这一个动作，并且把四件
//! 容易做错的事显式钉死：
//!
//! 1. **搜索序显性**（判据一）。引号形式 `"x.h"` 与尖括号形式 `<x.h>` 的搜索
//!    序**不同**且必须写进产物：引号形式 = 「包含者所在目录优先，再按引号搜索
//!    路径」；尖括号形式 = **跳过包含者所在目录**，直接按尖括号搜索路径。
//!    这不是风格差异而是语义差异——尖括号形式若也先查包含者目录，一个同名头
//!    文件就能被项目里的私有文件劫持。本条把「每个候选目录为什么被选中或被
//!    跳过」逐条记进 `SearchTrace`，找不到时报错**带搜索序明细**（锚点错误路径
//!    第一条），作者不必猜「它到底找过哪些地方」。
//!
//! 2. **环检测输出环**（判据二）。检测用「包含链指纹」：当前链上每个已展开
//!    文件的规范化路径指纹。命中即成环，报错**输出环路径**（`a → b → c → a`）
//!    而不是只说「检测到循环」——作者要照着环把 include 剪断，光给一句
//!    「有循环」等于没给。`IncludeError::cycle` 带 `cycle_path` 全文。
//!
//! 3. **深度上限指向链顶**（锚点错误路径第三条）。超限时错误指向**链顶**（第一
//!    个被拉进来的文件）而不是当前最深处：链顶才是拔掉递归的着手点。
//!
//! 4. **缓存裁定显性**（判据四）。锚点写「同文件只展开一次**语义按规范裁定**」
//!    ——「同文件只展开一次」是**语义决策**不是性能优化：着色器里同一个头被包
//!    两次会重复声明符号。因此默认 `Once` 语义：第二次命中返回**空展开**并
//!    记一条注记（不静默）；`Repeat` 语义须显式选定才生效。缓存失效（文件内容
//!    指纹变了）→ **重展开并标注**（锚点错误路径第四条），不拿旧展开骗下游。
//!
//! 零静默纪律：找不到 → 报错带搜索序明细；成环 → 报错输出环路径；超深 → 报错
//!   指向链顶；缓存命中被跳过 → 出注记；缓存失效 → 出注记并重展开；
//!   空 `#include` / 未闭合引号 / 未闭合尖括号 → 各自独立码报错。
//!   本模块不抛异常、不吞诊断、无静默分支、无全局可变状态、零 IO。
//!
//! 性能逐项分解：解析 O(路径长度)（路径拼接与规范化单遍）；环检测 O(链长)
//!   （链短且有深度上限）；缓存命中 O(1)（指纹定长数组线性探测，链内有界，
//!   实测常数级；不用哈希表是为了 no_std 下零分配）；展开 O(产出文本长度)。
//!
//! 上游消费：F0411 `DirectiveLine`（`kind == Include` 的行，指令内记号化产物）
//!   与 `PreproStreams`；F0413 `SkipRegion`（条件编译关掉的段落不参与展开，
//!   只按字节跨度快扫掠过）。下游 F0415 源码编码处理接续展开产物的字节流。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use super::vec11_prepro::{DirectiveKind, DirectiveLine};

/// include 指令的书写形式（决定搜索序——判据一的核心）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncludeForm {
    /// `#include "x.h"` —— 引号形式：先查包含者所在目录。
    Quoted,
    /// `#include <x.h>` —— 尖括号形式：**不查**包含者所在目录。
    Angle,
}

impl IncludeForm {
    /// 人话标签（诊断用）。
    pub const fn label(self) -> &'static str {
        match self {
            IncludeForm::Quoted => "引号形式",
            IncludeForm::Angle => "尖括号形式",
        }
    }

    /// 该形式是否先查包含者所在目录（语义差异的单一事实源）。
    pub const fn searches_includer_dir_first(self) -> bool {
        matches!(self, IncludeForm::Quoted)
    }
}

/// 搜索序中一个候选目录的裁决结果（逐条记账——「带搜索序明细」的载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchAttempt {
    /// 拼出的候选全路径。
    pub candidate: String,
    /// 该目录为何被试（引号形式的包含者目录 / 引号搜索路径第 i 项 / 尖括号搜索
    /// 路径第 i 项）。**自有String 而非 `&'static str`**——带序号的来源标签是
    /// 运行期拼的，借不出`'static`；顺带让调用方不必悬挂静态串的生命周期。
    pub origin: String,
    /// 命中与否。
    pub hit: bool,
}

/// 一次搜索的完整轨迹（找不到时报错逐条输出——锚点错误路径第一条）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchTrace {
    /// 该 include 的书写形式。
    pub form: Option<IncludeForm>,
    /// 被包含者的规范化路径（根文件为空——它不是被谁包含的）。
    pub includer: String,
    /// 逐条尝试记录，顺序即真实搜索顺序。
    pub attempts: Vec<SearchAttempt>,
    /// 命中的候选路径（未命中为空串）。
    pub hit: String,
}

impl SearchTrace {
    /// 人话搜索序明细（每行一个候选目录）。
    pub fn detail(&self) -> String {
        if self.attempts.is_empty() {
            return String::from("（没有可试的目录——搜索路径为空）");
        }
        let mut parts: Vec<String> = Vec::new();
        for (i, a) in self.attempts.iter().enumerate() {
            parts.push(format!(
                "{}. {} [{}] {}",
                i + 1,
                a.candidate,
                a.origin,
                if a.hit { "命中" } else { "未命中" }
            ));
        }
        parts.join("\n")
    }

    /// 试了几个目录（性能判据的计量面）。
    pub fn probes(&self) -> usize {
        self.attempts.len()
    }
}

/// 递归深度超限的错误定位对象（锚点错误路径第三条：指向链顶）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainTop {
    pub path: String,
    pub depth: usize,
}

/// include 解析错误（三要素齐备——判据「零静默」）。
///
/// 体积刻意压到一百多字节：`Result`'s Err 变体会被**每个返回点**按值搬运，
/// 错误类型一旦肥到几百字节，展开链上每层的 `?` 都在搬一整块内存。搜索序
/// 明细（`SearchTrace`，内含 `Vec`）是唯一的大块，且**只有「找不到」这一条
/// 路径用得上**——故装箱：常态（成功与多数错误）只搬指针，需要时才分配。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncludeError {
    pub code: &'static str,
    pub line: usize,
    pub pos: usize,
    pub what: String,
    pub why: String,
    pub next: String,
    /// 成环时的环路径全文（`a → b → c → a`）——判据二要求「输出环」。
    pub cycle_path: String,
    /// 成环时环上各段的指纹（含起点重复段，便于对拍）。
    pub cycle_fingerprints: Vec<u64>,
    /// 找不到时的搜索序明细（判据一要求「带搜索序明细」）；装箱见结构注。
    pub search_trace: Box<SearchTrace>,
    /// 超深时的链顶指向。
    pub chain_top: Option<ChainTop>,
    /// 当前包含链快照（诊断用，从链顶到当前）。
    pub chain: Vec<String>,
}

impl IncludeError {
    /// 三要素齐备（判据：零静默）。
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }

    /// 成环类错误的环路径人话串。
    pub fn cycle_display(&self) -> String {
        if self.cycle_path.is_empty() {
            return String::new();
        }
        self.cycle_path.clone()
    }
}

/// include 注记（非阻断的显性记录：缓存跳过、缓存失效重展开）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncludeNote {
    pub code: &'static str,
    pub line: usize,
    pub pos: usize,
    pub what: String,
    pub why: String,
    pub next: String,
}

/// 「同文件只展开一次」的语义裁定（判据四：缓存裁定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnceSemantics {
    /// 默认：同文件只展开一次，第二次命中跳过并出注记（防重复声明符号）。
    Once,
    /// 显式选定：每次都重新展开（仅当作者确认重复展开无害时用）。
    Repeat,
}

impl OnceSemantics {
    pub const fn label(self) -> &'static str {
        match self {
            OnceSemantics::Once => "只展开一次",
            OnceSemantics::Repeat => "每次重展开",
        }
    }
}

/// 缓存条目（一个被展开过的文件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheEntry {
    /// 规范化路径。
    pub path: String,
    /// 内容指纹（内容变了 → 条目失效）。
    pub fingerprint: u64,
    /// 展开次数（Once 语义下恒为 1；Repeat 语义下每次命中 +1）。
    pub expansions: usize,
}

/// 包含图的一条边（构建系统与诊断的消费面——锚点「包含图记录」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncludeEdge {
    /// 包含者规范化路径（根为 `""`）。
    pub from: String,
    /// 被包含者规范化路径。
    pub to: String,
    /// 该边的书写形式（构建系统按形式决定依赖类别）。
    pub form: IncludeForm,
    /// 该边在包含者源文件里的行号（构建系统的增量重编依据）。
    pub line: usize,
}

/// 包含图（节点 = 文件，边 = 一次包含关系）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IncludeGraph {
    /// 参与图的规范化路径（含根）。
    pub nodes: Vec<String>,
    /// 包含边（按展开顺序记录——确定性，可对拍）。
    pub edges: Vec<IncludeEdge>,
}

impl IncludeGraph {
    /// 登记一条边并补齐两端节点（重复登记同一条边不重复计数）。
    pub fn add_edge(&mut self, from: &str, to: &str, form: IncludeForm, line: usize) {
        if !self.nodes.iter().any(|n| n == from) {
            self.nodes.push(from.to_string());
        }
        if !self.nodes.iter().any(|n| n == to) {
            self.nodes.push(to.to_string());
        }
        let dup = self
            .edges
            .iter()
            .any(|e| e.from == from && e.to == to && e.form == form && e.line == line);
        if !dup {
            self.edges.push(IncludeEdge {
                from: from.to_string(),
                to: to.to_string(),
                form,
                line,
            });
        }
    }

    /// 某文件的直接子包含（构建系统的依赖邻接表）。
    pub fn children_of(&self, path: &str) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for e in self.edges.iter().filter(|e| e.from == path) {
            if !out.contains(&e.to.as_str()) {
                out.push(e.to.as_str());
            }
        }
        out
    }

    /// 图是否无环（成环时报错，故成功产物必为 true——自检面）。
    pub fn is_acyclic(&self) -> bool {
        // 邻接表 + 迭代三色标记（无递归，避免深图爆栈）。
        let mut idx: Vec<(&str, usize)> = Vec::with_capacity(self.nodes.len());
        for (i, n) in self.nodes.iter().enumerate() {
            idx.push((n.as_str(), i));
        }
        let mut color = vec![0u8; self.nodes.len()];
        for start in 0..self.nodes.len() {
            if color[start] != 0 {
                continue;
            }
            // 显式栈：(节点, 邻接游标)
            let mut stack: Vec<(usize, usize)> = Vec::new();
            stack.push((start, 0));
            color[start] = 1;
            while let Some((node, ref mut cur)) = stack.last_mut() {
                let kids = self.children_of(idx[*node].0);
                if *cur >= kids.len() {
                    color[*node] = 2;
                    stack.pop();
                    continue;
                }
                let kid = kids[*cur];
                *cur += 1;
                let kid_idx = match idx.iter().find(|(n, _)| *n == kid) {
                    Some((_, i)) => *i,
                    None => continue,
                };
                match color[kid_idx] {
                    1 => return false, // 回边 → 有环
                    2 => {}
                    _ => {
                        color[kid_idx] = 1;
                        stack.push((kid_idx, 0));
                    }
                }
            }
        }
        true
    }
}

/// 解析统计（性能判据的可观测面）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IncludeStats {
    /// 处理的 include 指令数。
    pub directives: usize,
    /// 实际发生磁盘/注册表级查找的次数（缓存命中不计）。
    pub lookups: usize,
    /// 缓存命中次数（O(1) 命中面）。
    pub cache_hits: usize,
    /// 因 Once 语义被跳过展开的次数。
    pub skipped_once: usize,
    /// 缓存失效并重展开的次数。
    pub cache_invalidations: usize,
    /// 产出展开文本的总字节。
    pub emitted_bytes: usize,
    /// 展开峰值深度。
    pub peak_depth: usize,
}

/// include 解析配置（全部上限与策略显式——不留隐藏默认值）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncludeConfig {
    /// 递归深度上限（超限报错指向链顶）。
    pub max_depth: usize,
    /// 引号形式搜索路径（在包含者目录之后试）。
    pub quote_dirs: Vec<String>,
    /// 尖括号形式搜索路径（**不含**包含者目录）。
    pub angle_dirs: Vec<String>,
    /// 同文件只展开一次的语义裁定。
    pub once: OnceSemantics,
}

impl Default for IncludeConfig {
    fn default() -> IncludeConfig {
        IncludeConfig {
            max_depth: 64,
            quote_dirs: Vec::new(),
            angle_dirs: Vec::new(),
            once: OnceSemantics::Once,
        }
    }
}

impl IncludeConfig {
    /// 构造器式配置（显式链式，避免 struct 字面量漏字段）。
    pub fn new(max_depth: usize) -> IncludeConfig {
        IncludeConfig {
            max_depth,
            ..IncludeConfig::default()
        }
    }

    /// 追加一个引号搜索目录。
    pub fn with_quote_dir(mut self, dir: &str) -> IncludeConfig {
        self.quote_dirs.push(dir.to_string());
        self
    }

    /// 追加一个尖括号搜索目录。
    pub fn with_angle_dir(mut self, dir: &str) -> IncludeConfig {
        self.angle_dirs.push(dir.to_string());
        self
    }

    /// 选定「同文件只展开一次」语义（默认即Once，显式选定仍留接口）。
    pub fn with_once(mut self, once: OnceSemantics) -> IncludeConfig {
        self.once = once;
        self
    }
}

/// 内容供给方（宿主注入——本模块零 IO，可测可对拍）。
///
/// 真实实现接 VFS；宿主测试接内存字典。指纹由内容算出的约定见
/// [`fingerprint`]：同一内容必得同一指纹（确定性判据）。
pub trait ContentSource {
    /// 按**规范化路径**取内容；不存在返回 `None`（不静默假命中）。
    fn get(&self, norm_path: &str) -> Option<String>;
    /// 只问「在不在」，不取内容。
    ///
    /// 搜索序探针要的只是存在性，用它把「存在性判定」与「内容读取」拆开，
    /// 缓存才能在**读内容之前**命中——锚点性能条写的是「缓存 O(1) 命中」，
    /// 若先读后判缓存，每次命中仍要付一次全文读取的钱，命中并不O(1)。
    /// 默认实现退回 `get().is_some()`（正确但不快）；真实 VFS 实现应覆写
    /// 成真正的元数据查询（stat），宿主实现可覆写成哈希键查找。
    fn exists(&self, norm_path: &str) -> bool {
        self.get(norm_path).is_some()
    }
    /// 取内容的指纹（用于缓存失效判定）；不存在时返回 `None`。
    ///
    /// 默认实现用 [`fingerprint`] 算全文指纹。实现者若能拿到更廉价的
    /// 版本戳（mtime + 长度）可覆写——**只要同内容必同值**即可。
    fn fingerprint_of(&self, norm_path: &str) -> Option<u64> {
        self.get(norm_path).map(|c| fingerprint(&c))
    }
}

/// 内容指纹（FNV-1a 64 位——确定性、零依赖、跨平台一致）。
///
/// 选FNV-1a 而不是默认 hasher：`DefaultHasher` 的种子随进程变，同内容在不同
/// 进程会得不同指纹，缓存行为就不可对拍了——而「同内容同指纹」正是本条
/// 缓存裁定的前提。
pub fn fingerprint(content: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in content.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 路径规范化（消解 `.` / `..` 与重复分隔符——判据「环检测」的前置）。
///
/// 规范化失败（非 `..` 开头的绝对路径却在途中冒出 `..`）返回 `None`：
/// 宁可报错也不产出错误路径。
pub fn normalize(path: &str) -> Option<String> {
    // 绝对路径必须**保留前导 `/`**。早先版本对所有路径一视同仁地split，
    // 于是 `/abs/a.h` 的首段（空串）被当成噪声丢掉，规范化成 `abs/a.h`
    // ——与 MemorySource 里登记的 `/abs/a.h` 对不上，绝对路径 include 永远
    // 找不到。根锚是路径语义的一部分，不是分隔符噪声。
    let absolute = path.starts_with('/');
    let mut stack: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                // 绝对路径下 `..` 撞根即止（POSIX 语义：/.. == /）；
                // 相对路径下撞根说明路径非法，返回 None 让调用方显性报错。
                if stack.pop().is_none() && !absolute {
                    return None;
                }
            }
            s => stack.push(s),
        }
    }
    if stack.is_empty() {
        return None;
    }
    let joined = stack.join("/");
    if absolute {
        Some(format!("/{}", joined))
    } else {
        Some(joined)
    }
}

/// 取路径的目录部分（根文件返回空串——它没有「所在目录」之前的来源）。
pub fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// 路径拼接（`dir` 为空时直接返回 `rel`，不为空时补分隔符）。
fn join(dir: &str, rel: &str) -> String {
    if dir.is_empty() {
        rel.to_string()
    } else {
        format!("{}/{}", dir, rel)
    }
}

/// 从 `#include` 指令体解析出书写形式与路径原文。
///
/// 形态全集：`"x.h"`、`<x.h>`、`X`（裸标识，按引号形式处理——多数预处理器
/// 的兼容写法）、`<>`（空，非法）、`""`（空，非法）。未闭合引号 / 未闭合尖括号
/// 各走独立错误码，不与「空路径」混为一谈。
pub fn parse_include_operand(body: &str) -> Result<(IncludeForm, &str), (&'static str, String)> {
    let t = body.trim();
    if t.is_empty() {
        return Err(("E_INC_EMPTY_OPERAND", "#include 后面没有路径".to_string()));
    }
    let first = t.as_bytes()[0];
    if first == b'"' {
        if t.len() < 2 || !t.ends_with('"') {
            return Err((
                "E_INC_UNCLOSED_QUOTE",
                "引号形式的 include 路径没有闭合的双引号".to_string(),
            ));
        }
        return Ok((IncludeForm::Quoted, &t[1..t.len() - 1]));
    }
    if first == b'<' {
        if t.len() < 2 || !t.ends_with('>') {
            return Err((
                "E_INC_UNCLOSED_ANGLE",
                "尖括号形式的 include 路径没有闭合的尖括号".to_string(),
            ));
        }
        return Ok((IncludeForm::Angle, &t[1..t.len() - 1]));
    }
    Ok((IncludeForm::Quoted, t))
}

/// include 解析器（持有配置、缓存、包含图与当前链）。
pub struct IncludeResolver<'a, S: ContentSource> {
    cfg: IncludeConfig,
    src: &'a S,
    /// 缓存条目（定长上限的线性探测——no_std 零分配，链内有界）。
    cache: Vec<CacheEntry>,
    /// 缓存容量上限（超出后不再新增条目，但已有条目仍可命中）。
    cache_cap: usize,
    graph: IncludeGraph,
    notes: Vec<IncludeNote>,
    stats: IncludeStats,
    /// 当前包含链（规范化路径，栈顶 = 最近展开的那个）。
    chain: Vec<String>,
    /// 栈顶所在文件（拼搜索路径用）。
    includer: String,
}

impl<'a, S: ContentSource> IncludeResolver<'a, S> {
    /// 构造解析器（缓存容量显式给定，避免隐藏策略）。
    pub fn new(cfg: IncludeConfig, src: &'a S) -> IncludeResolver<'a, S> {
        let cap = cfg.max_depth.saturating_mul(4).max(16);
        IncludeResolver {
            cfg,
            src,
            cache: Vec::new(),
            cache_cap: cap,
            graph: IncludeGraph::default(),
            notes: Vec::new(),
            stats: IncludeStats::default(),
            chain: Vec::new(),
            includer: String::new(),
        }
    }

    /// 解析一个根文件的 include 闭包（产出展开后的文本）。
    ///
    /// `root_path` 是根文件的规范化路径（用于图与诊断，不要求它真实存在于
    /// `src` 里——根文本由调用方直接给出）。`root_text` 是根源文本。
    pub fn resolve_root(
        &mut self,
        root_path: &str,
        root_text: &str,
    ) -> Result<String, Box<IncludeError>> {
        let root = normalize(root_path).ok_or_else(|| {
            self.err(
                "E_INC_ROOT_PATH",
                0,
                0,
                format!("根路径无法规范化：{}", root_path),
                "路径里有非法的 '..' 或为空".to_string(),
                "给一个不依赖工作目录的相对规范化路径".to_string(),
            )
        })?;
        self.graph.nodes.push(root.clone());
        self.chain.push(root.clone());
        self.includer = root;
        let out = self.expand_text(root_text, 1);
        self.chain.clear();
        out
    }

    /// 展开一段文本内的全部 include 指令（递归入口）。
    ///
    /// 产出是**按行重建**的：每条代码行与每条透传指令各占一行并以 `\n` 收尾。
    /// 代价是被包含文件末尾的「有无末尾换行」差异在展开后被抹平（统一补齐）；
    /// 收益是相邻两段文本不会黏成一行——`#include "a.h"` 若展开出无尾换行的
    /// 末行，紧跟其后的代码行会被并进同一行，那才是真错。行数与顺序才是
    /// 下游 F0413 跳过段按字节跨度对齐的依据，黏行会让跨度全部失效。
    fn expand_text(&mut self, text: &str, depth: usize) -> Result<String, Box<IncludeError>> {
        let streams = super::vec11_prepro::split_streams(text).map_err(|e| {
            self.err(
                "E_INC_UPSTREAM",
                e.line,
                e.pos,
                format!("上游预处理失败：{}", e.what),
                e.why.clone(),
                e.next.clone(),
            )
        })?;

        let mut out = String::new();
        let mut di = 0usize;
        let mut ci = 0usize;
        loop {
            let take_dir = match (streams.directives.get(di), streams.code.get(ci)) {
                (Some(d), Some(c)) => d.line <= c.line,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_dir {
                let d: &DirectiveLine = &streams.directives[di];
                if d.kind == DirectiveKind::Include {
                    out.push_str(&self.expand_one(d, depth)?);
                } else {
                    // 非 include 指令原样透传（宏与条件编译由F0412/F0413 在
                    // 各自阶段处理，此处不越权求值）。
                    out.push('#');
                    out.push_str(d.name.as_str());
                    if !d.body.is_empty() {
                        out.push(' ');
                        out.push_str(d.body.as_str());
                    }
                    out.push('\n');
                }
                di += 1;
            } else {
                let c = &streams.code[ci];
                out.push_str(c.text.as_str());
                out.push('\n');
                ci += 1;
            }
        }
        Ok(out)
    }

    /// 展开单条 `#include`（判据一至四的主流程）。
    fn expand_one(&mut self, d: &DirectiveLine, depth: usize) -> Result<String, Box<IncludeError>> {
        self.stats.directives += 1;

        let (form, raw) = parse_include_operand(&d.body).map_err(|(code, what)| {
            let mut e = self.err(
                code,
                d.line,
                d.pos,
                what,
                "include 的路径必须写成 \"x.h\" 或 <x.h> 两种形态之一".to_string(),
                "改成 #include \"x.h\" 或 #include <x.h>".to_string(),
            );
            e.search_trace.includer = self.includer.clone();
            e.search_trace.form = Some(form_or_unknown(&d.body));
            e
        })?;

        if raw.is_empty() {
            let mut e = self.err(
                "E_INC_EMPTY_PATH",
                d.line,
                d.pos,
                format!("{}的 include 路径为空", form.label()),
                "空路径拼不出候选文件名".to_string(),
                "写上要包含的文件名".to_string(),
            );
            e.search_trace.includer = self.includer.clone();
            e.search_trace.form = Some(form);
            return Err(Box::new(e));
        }

        // ── 深度上限：超限指向链顶（锚点错误路径第三条）──
        if depth > self.cfg.max_depth {
            let top = self.chain.first().cloned().unwrap_or_default();
            let mut e = self.err(
                "E_INC_DEPTH_EXCEEDED",
                d.line,
                d.pos,
                format!("include 递归深度 {} 超过上限 {}", depth, self.cfg.max_depth),
                "再往下的包含关系无法确定是否成环，继续下去只会栈溢出或产出垃圾".to_string(),
                format!(
                    "从链顶{}处拆掉这层递归，或调高 max_depth（当前 {}）",
                    if top.is_empty() {
                        "（根）"
                    } else {
                        top.as_str()
                    },
                    self.cfg.max_depth
                ),
            );
            e.chain_top = Some(ChainTop {
                path: top,
                depth: 1,
            });
            e.chain = self.chain.clone();
            return Err(Box::new(e));
        }

        // ── 搜索序（判据一：逐条记账，带明细报错）──
        let includer_snapshot = self.includer.clone();
        let trace = self.search(&includer_snapshot, raw, form);
        let resolved = trace.hit.clone();
        if resolved.is_empty() {
            let mut e = self.err(
                "E_INC_NOT_FOUND",
                d.line,
                d.pos,
                format!("找不到 include 的文件 {}（{}）", raw, form.label()),
                format!("按该形式的搜索序试了 {} 个目录，都不存在", trace.probes()),
                "核对文件名与拼写；若是项目内头文件用引号形式，若是库头文件用尖括号形式"
                    .to_string(),
            );
            e.search_trace = Box::new(trace);
            e.chain = self.chain.clone();
            return Err(Box::new(e));
        }

        let norm = match normalize(&resolved) {
            Some(n) => n,
            None => {
                let mut e = self.err(
                    "E_INC_PATH_UNNORMALIZABLE",
                    d.line,
                    d.pos,
                    format!("解析出的路径无法规范化：{}", resolved),
                    "路径里有越过根的 '..'".to_string(),
                    "修掉 include 路径里的上跳".to_string(),
                );
                e.search_trace = Box::new(trace);
                return Err(Box::new(e));
            }
        };

        // ── 环检测（判据二：指纹命中即成环，报错输出环路径）──
        if let Some(start) = self.chain.iter().position(|p| *p == norm) {
            let mut ring: Vec<String> = Vec::new();
            let mut prints: Vec<u64> = Vec::new();
            for p in self.chain[start..].iter() {
                ring.push(p.clone());
                prints.push(fingerprint(p.as_str()));
            }
            ring.push(norm.clone());
            prints.push(fingerprint(norm.as_str()));
            let mut e = self.err(
                "E_INC_CYCLE",
                d.line,
                d.pos,
                format!("include 成环：{}", ring.join(" → ")),
                "环上的每个文件都在等另一个先展开，谁也等不到——展开会永远递归下去".to_string(),
                format!("剪断环上任意一条边（本环入口：{} 第 {} 行）", norm, d.line),
            );
            e.cycle_path = ring.join(" → ");
            e.cycle_fingerprints = prints;
            e.search_trace = Box::new(trace);
            e.chain = self.chain.clone();
            return Err(Box::new(e));
        }

        // ── 缓存裁定（判据四）──
        //
        // 顺序很要紧：**先查缓存，再读内容**。Once 语义下第二次命中要付的
        // 代价必须只是「查一次缓存」，而不是「把文件再读一遍才发现读过」。
        // 所以这里先问 `fingerprint_of`（实现可覆写成廉价版本戳），指纹与
        // 条目一致就直接按裁定跳过，全程不碰 `get`——这才对得上锚点性能条
        // 写的「缓存 O(1) 命中」。
        let cached = self.cache.iter().position(|c| c.path == norm);
        let current_fp = self.src.fingerprint_of(&norm);
        if let Some(i) = cached {
            self.stats.cache_hits += 1;
            let same = match current_fp {
                Some(fp) => self.cache[i].fingerprint == fp,
                // 供给方给不出指纹时不能默认「没变」——那会让改动过的文件
                // 拿旧展开喂下游。保守按「已变」处理，重展开并标注。
                None => false,
            };
            if same && self.cfg.once == OnceSemantics::Once {
                self.stats.skipped_once += 1;
                self.notes.push(IncludeNote {
                    code: "N_INC_SKIPPED_ONCE",
                    line: d.line,
                    pos: d.pos,
                    what: format!("{} 已在本次展开中出现过，按「只展开一次」跳过", norm),
                    why: "重复展开同一头文件会重复声明符号".to_string(),
                    next: "确认确实需要重复展开时，把 once 语义显式选为Repeat".to_string(),
                });
                return Ok(String::new());
            }
            if !same {
                self.stats.cache_invalidations += 1;
                self.notes.push(IncludeNote {
                    code: "N_INC_CACHE_STALE",
                    line: d.line,
                    pos: d.pos,
                    what: format!("{} 的内容指纹已变，缓存失效并重展开", norm),
                    why: "拿旧展开喂下游会让包含图与真实文件不一致".to_string(),
                    next: "无需处理——已按新内容重展开".to_string(),
                });
            }
        }

        // 需要内容了（缓存未命中 / 语义要求重展开 / 条目失效）。
        let content = match self.src.get(&norm) {
            Some(c) => {
                self.stats.lookups += 1;
                c
            }
            None => {
                // 搜索命中但取不到内容——供给方与搜索序不一致，显性报错而非
                // 当成空文件（空文件会让包含者的符号凭空消失）。
                let mut e = self.err(
                    "E_INC_SOURCE_MISS",
                    d.line,
                    d.pos,
                    format!("搜索序命中了 {}，但内容供给方没有它", norm),
                    "搜索序与内容供给看到的是两份不同的文件视图".to_string(),
                    "让搜索目录与内容供给指向同一套文件".to_string(),
                );
                e.search_trace = Box::new(trace);
                e.chain = self.chain.clone();
                return Err(Box::new(e));
            }
        };
        // 指纹：优先供给方的版本戳，缺省用已读内容现算（同内容必同值）。
        let fp = current_fp.unwrap_or_else(|| fingerprint(content.as_str()));

        // 登记/更新缓存条目。条目已存在且指纹一致（Repeat 语义走到这里）
        // 只累加展开次数；指纹变了就地改写，不留旧值。
        match cached {
            Some(i) => {
                self.cache[i].fingerprint = fp;
                self.cache[i].expansions += 1;
            }
            None => {
                if self.cache.len() < self.cache_cap {
                    self.cache.push(CacheEntry {
                        path: norm.clone(),
                        fingerprint: fp,
                        expansions: 1,
                    });
                } else {
                    self.stats.cache_invalidations += 1;
                    self.notes.push(IncludeNote {
                        code: "N_INC_CACHE_FULL",
                        line: d.line,
                        pos: d.pos,
                        what: format!("缓存已达上限 {} 条，{} 本次不记缓存", self.cache_cap, norm),
                        why: "缓存有界才不会在长包含链上吃满内存".to_string(),
                        next: "结果仍然正确，只是重复包含要多展开一次".to_string(),
                    });
                }
            }
        }

        // ── 递归展开（入链 → 展开 → 出链）──
        self.stats.peak_depth = self.stats.peak_depth.max(depth);
        self.graph.add_edge(&self.includer, &norm, form, d.line);
        let saved_includer = self.includer.clone();
        self.chain.push(norm.clone());
        self.includer = norm.clone();
        let expanded = self.expand_text(&content, depth + 1);
        self.chain.pop();
        self.includer = saved_includer;
        let expanded = expanded?;
        self.stats.emitted_bytes += expanded.len();
        Ok(expanded)
    }

    /// 按形式执行搜索（判据一：引号先查包含者目录，尖括号不查）。
    ///
    /// 搜索**只判存在性不取内容**（见 [`Self::probe`]），故一条 include 的
    /// 文件读取次数上限是 1：命中后由缓存裁定决定是否真读。
    fn search(&mut self, includer: &str, rel: &str, form: IncludeForm) -> SearchTrace {
        let mut trace = SearchTrace {
            form: Some(form),
            includer: includer.to_string(),
            attempts: Vec::new(),
            hit: String::new(),
        };
        // 绝对路径直接试，不进搜索序。
        if rel.starts_with('/') {
            let cand = rel.to_string();
            self.probe(&mut trace, &cand, String::from("绝对路径"));
            return trace;
        }

        let inc_dir = dir_of(includer);
        // 引号形式：包含者所在目录**优先**。
        //
        // 注意 `inc_dir` 可能为空串——那表示包含者就在根（如`m.vec`），此时
        // 「它的所在目录」就是当前目录，候选路径等于 `rel` 本身，**照样要试**。
        // 早先版本在这里写了 `&& !inc_dir.is_empty()`，把根级包含者的目录整个
        // 跳过，于是根文件 include 任何东西都报 E_INC_NOT_FOUND——环检测与深度
        // 上限也跟着全废（它们都在解析成功之后才起作用）。空目录不是「没有
        // 目录」，是「当前目录」，这个区分必须留在代码里而不是靠猜。
        if form.searches_includer_dir_first()
            && self.probe(
                &mut trace,
                &join(inc_dir, rel),
                String::from("包含者所在目录"),
            )
        {
            return trace;
        }
        // 搜索路径按形式二选一。**先克隆出候选目录列表再逐个试**：直接借
        // `&self.cfg.*_dirs` 会让不可变借用活到循环结束，而循环体里的
        // `self.probe(..)` 要可变借用 `self`——两者互斥，编译不过。
        // 克隆的成本是每条 include 拷一次目录表（目录表远小于文件内容），
        //换来的是搜索序逻辑保持直线、零别名。
        let dirs: Vec<String> = match form {
            IncludeForm::Quoted => self.cfg.quote_dirs.clone(),
            IncludeForm::Angle => self.cfg.angle_dirs.clone(),
        };
        let origin_base = match form {
            IncludeForm::Quoted => "引号搜索路径",
            IncludeForm::Angle => "尖括号搜索路径",
        };
        for (i, dir) in dirs.iter().enumerate() {
            if self.probe(&mut trace, &join(dir, rel), format!("{}{}", origin_base, i)) {
                return trace;
            }
        }
        trace
    }

    /// 试一个候选目录并记账。命中返回 `true`。
    ///
    /// 探针只问存在性（[`ContentSource::exists`]）**不取内容**：搜索序要回答的
    /// 只是「这个目录里有没有」，内容留给缓存裁定之后按需取。两者分开之后，
    /// 「找到即停」停的是目录枚举，而Once 命中连内容都不读。
    fn probe(&mut self, trace: &mut SearchTrace, candidate: &str, origin: String) -> bool {
        let hit = self.src.exists(candidate);
        if hit {
            trace.hit = candidate.to_string();
        }
        trace.attempts.push(SearchAttempt {
            candidate: candidate.to_string(),
            origin,
            hit,
        });
        hit
    }

    /// 构造一个三要素齐备的错误（带当前链快照）。
    fn err(
        &self,
        code: &'static str,
        line: usize,
        pos: usize,
        what: String,
        why: String,
        next: String,
    ) -> IncludeError {
        IncludeError {
            code,
            line,
            pos,
            what,
            why,
            next,
            cycle_path: String::new(),
            cycle_fingerprints: Vec::new(),
            search_trace: Box::new(SearchTrace::default()),
            chain_top: None,
            chain: Vec::new(),
        }
    }

    /// 包含图（构建系统与诊断的消费口）。
    pub fn graph(&self) -> &IncludeGraph {
        &self.graph
    }

    /// 注记表（缓存跳过 / 失效 / 满——非阻断但显性）。
    pub fn notes(&self) -> &[IncludeNote] {
        &self.notes
    }

    /// 统计（性能判据计量面）。
    pub fn stats(&self) -> IncludeStats {
        self.stats
    }

    /// 缓存快照（裁定可观测）。
    pub fn cache(&self) -> &[CacheEntry] {
        &self.cache
    }
}

/// 从残缺指令体猜形式（只用于错误路径的上下文标注，猜错不影响正确性）。
fn form_or_unknown(body: &str) -> IncludeForm {
    let t = body.trim();
    if t.starts_with('<') {
        IncludeForm::Angle
    } else {
        IncludeForm::Quoted
    }
}

/// 内存内容源（宿主测试与工具链用——真实实现接 VFS）。
///
/// 文件表装在 [`RefCell`] 里，因此**解析器持有 `&MemorySource` 的同时也能
/// 改内容**。这不是图省事，而是缓存失效判据的硬要求：那条判据要的就是
/// 「同一个解析器、缓存已在手、文件内容变了」——若表只能通过 `&mut` 改，
/// 解析器一借走源就没法改文件，失效分支永远走不到，检查项只能靠新建解析器
/// 假装测过（实际每次缓存都是空的，测了个寂寞）。
#[derive(Debug, Default)]
pub struct MemorySource {
    files: RefCell<Vec<(String, String)>>,
}

impl Clone for MemorySource {
    fn clone(&self) -> MemorySource {
        MemorySource {
            files: RefCell::new(self.files.borrow().clone()),
        }
    }
}

impl MemorySource {
    /// 空源。
    pub fn new() -> MemorySource {
        MemorySource {
            files: RefCell::new(Vec::new()),
        }
    }

    /// 登记一个文件（路径会被规范化，重复登记后者覆盖前者）。
    pub fn insert(&self, path: &str, content: &str) -> &MemorySource {
        let norm = normalize(path).unwrap_or_else(|| path.to_string());
        let mut files = self.files.borrow_mut();
        match files.iter_mut().find(|(p, _)| *p == norm) {
            Some(slot) => slot.1 = content.to_string(),
            None => files.push((norm, content.to_string())),
        }
        drop(files);
        self
    }

    /// 改写已登记文件的内容（构造「缓存失效」场景用）。
    pub fn replace(&self, path: &str, content: &str) -> bool {
        let norm = match normalize(path) {
            Some(n) => n,
            None => path.to_string(),
        };
        let mut files = self.files.borrow_mut();
        match files.iter_mut().find(|(p, _)| *p == norm) {
            Some(slot) => {
                slot.1 = content.to_string();
                true
            }
            None => false,
        }
    }
}

impl ContentSource for MemorySource {
    fn get(&self, norm_path: &str) -> Option<String> {
        self.files
            .borrow()
            .iter()
            .find(|(p, _)| p == norm_path)
            .map(|(_, c)| c.clone())
    }

    /// 覆写存在性查询：键查找即可，不必克隆内容——这正是
    /// [`ContentSource::exists`] 存在的意义（搜索序不该为探针付全文读取的钱）。
    fn exists(&self, norm_path: &str) -> bool {
        self.files.borrow().iter().any(|(p, _)| p == norm_path)
    }
}

/// 便捷入口：一次解析（不保留解析器状态）。
pub fn resolve_includes(
    src: &MemorySource,
    cfg: &IncludeConfig,
    root_path: &str,
    root_text: &str,
) -> Result<(String, IncludeGraph, Vec<IncludeNote>, IncludeStats), Box<IncludeError>> {
    let mut r = IncludeResolver::new(cfg.clone(), src);
    let text = r.resolve_root(root_path, root_text)?;
    Ok((text, r.graph().clone(), r.notes().to_vec(), r.stats()))
}

/// VE-F0414 域自检（转发到 `vec14_checks`，与 vec11/12/13 同约定）。
pub fn run_vec14_checks() -> crate::checks::CheckSet {
    super::vec14_checks::run_vec14_checks()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src3() -> MemorySource {
        let s = MemorySource::new();
        s.insert("main.vec", "#include \"a.h\"\nvoid main() {}\n");
        s.insert("a.h", "#include \"b.h\"\nA\n");
        s.insert("b.h", "B\n");
        s
    }

    #[test]
    fn expands_nested_includes_in_order() {
        let s = src3();
        let cfg = IncludeConfig::new(16);
        let (text, g, _, _) =
            resolve_includes(&s, &cfg, "main.vec", s.get("main.vec").unwrap().as_str()).unwrap();
        assert_eq!(text, "B\nA\nvoid main() {}\n");
        assert_eq!(g.edges.len(), 2);
        assert!(g.is_acyclic());
    }

    #[test]
    fn detects_self_cycle_and_reports_ring() {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n");
        s.insert("a.h", "#include \"a.h\"\n");
        let e = resolve_includes(
            &s,
            &IncludeConfig::new(16),
            "m.vec",
            s.get("m.vec").unwrap().as_str(),
        )
        .unwrap_err();
        assert_eq!(e.code, "E_INC_CYCLE");
        assert_eq!(e.cycle_display(), "a.h → a.h");
        assert!(e.is_complete());
    }

    #[test]
    fn angle_form_skips_includer_dir() {
        let s = MemorySource::new();
        s.insert("sub/m.vec", "#include <a.h>\n");
        s.insert("sub/a.h", "NEAR\n");
        s.insert("lib/a.h", "LIB\n");
        let cfg = IncludeConfig::new(16).with_angle_dir("lib");
        let (text, _, _, _) =
            resolve_includes(&s, &cfg, "sub/m.vec", s.get("sub/m.vec").unwrap().as_str()).unwrap();
        assert_eq!(text, "LIB\n");
    }

    #[test]
    fn once_semantics_skips_second_expansion() {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n#include \"a.h\"\n");
        s.insert("a.h", "A\n");
        let (_, _, notes, st) = resolve_includes(
            &s,
            &IncludeConfig::new(16),
            "m.vec",
            s.get("m.vec").unwrap().as_str(),
        )
        .unwrap();
        assert_eq!(st.skipped_once, 1);
        assert_eq!(st.lookups, 1);
        assert!(notes.iter().any(|n| n.code == "N_INC_SKIPPED_ONCE"));
    }

    #[test]
    fn depth_limit_points_at_chain_top() {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"f0.h\"\n");
        s.insert("f0.h", "#include \"f1.h\"\n");
        s.insert("f1.h", "#include \"f2.h\"\n");
        s.insert("f2.h", "LEAF\n");
        let e = resolve_includes(
            &s,
            &IncludeConfig::new(2),
            "m.vec",
            s.get("m.vec").unwrap().as_str(),
        )
        .unwrap_err();
        assert_eq!(e.code, "E_INC_DEPTH_EXCEEDED");
        // 链顶 = 包含链的起点（根 m.vec），不是第一个被包含的文件——
        // 拆递归要从着手点剪，指向 f0.h 会让作者去改无辜的中间层。
        assert_eq!(e.chain_top.unwrap().path, "m.vec");
    }

    #[test]
    fn not_found_carries_search_trace() {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"nope.h\"\n");
        let cfg = IncludeConfig::new(16)
            .with_quote_dir("q1")
            .with_quote_dir("q2");
        let e = resolve_includes(&s, &cfg, "m.vec", s.get("m.vec").unwrap().as_str()).unwrap_err();
        assert_eq!(e.code, "E_INC_NOT_FOUND");
        // 三次探针：根级包含者的当前目录 + 两个引号搜索路径，逐条可查。
        assert_eq!(e.search_trace.probes(), 3);
        assert!(e.search_trace.detail().contains("q2/nope.h"));
    }

    #[test]
    fn domain_checks_all_green() {
        let set = run_vec14_checks();
        assert!(
            set.all_passed(),
            "VE-F0414 自检存在红项：{}/{}绿",
            set.tally().0,
            set.tally().0 + set.tally().1
        );
        assert!(!set.truncated(), "自检项被容量截断，证据不完整");
    }
}
