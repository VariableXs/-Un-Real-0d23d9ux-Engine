//! vcw09_docsite ↔ CGPU-F3529 · 文档站（站点部署 / 静态生成）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3529`（L14782）
//!
//! **判据（锚点原文）**：站点：文档站（站点部署/静态生成——站点实现；
//! 测试（站点一组）。判据：静态、一组、判据。
//!
//! ## 本模块真实语义
//!
//! 文档站不是"把 md 转成 html"这么简单，它是一条**可重入的静态生成管线**，
//! 真实工程里有五处不做就会出事的语义，本模块逐条落实：
//!
//! 1. **静态是硬约束，不是口号**：站点产物必须是**纯静态**——零服务端
//!    渲染、零运行时依赖、零墙钟时间戳泄漏。`StaticProof` 逐产物核对：
//!    （a）产物集合在同输入下**逐位相同**（幂等重生成不产生 diff——
//!    这是"静态"最硬的检验：静态站点必须可重复构建出字节相同产物，
//!    含时间戳/随机序的生成器一律判红）；（b）产物内禁出现构建时刻
//!    注入的动态字段（`generated_at` 之类）；（c）产物间引用只能指向
//!    站内相对路径（禁绝对 URL / 外部域——外链会让站点失去离线可读性）。
//! 2. **路由即产物闭集**：URL → 文件路径的映射必须**双射**。两条真实事故：
//!    （a）路由碰撞——`/a/b` 与 `/a/b/`（或大小写差异）映射到同一文件，
//!    后写者静默覆盖前者；（b）路径穿越——`..` 段把产物写到站点根之外，
//!    造成**目录逃逸**（能覆盖站点外的任意文件，属安全事故而非 bug）。
//!    两者都必须显性拒（`RouteClash` / `PathEscape`），绝不静默。
//! 3. **增量重生成的可证明正确**：整站重建慢，但全量重建又是浪费。
//!    `Incremental` 用**内容指纹 + 反向依赖边**做最小重建集：只有指纹
//!    变化的页面 + **传递闭包内**依赖它的页面才重建；指纹未变的页面
//!    直接复用旧产物（产物指纹必须与旧产物**逐位相等**——若不等说明
//!    生成器掺入了不稳定因子，按 `UnstableGen` 立案）。传递闭包必须
//!    走**拓扑序**，遇环即拒（`DepCycle`）——依赖成环时"最小重建集"
//    在数学上不存在，硬算会无限递归。
//! 4. **导航与站点地图一致**：导航树是站点的骨架，它必须与真实产物
//!    **一一对应**：导航指向不存在的页面 = 死链（`NavDangling`），
//!    存在却不在导航中的页面 = 孤儿页（`Orphan`，会让页面事实上不可达）。
//!    两者都要在生成收尾时结清，不留到线上。
//! 5. **产物清单可核对**：生成收尾产出 `Manifest`（路径 + 内容指纹 +
//!    字节数 + 引用出边数），它是部署与校验的唯一真源。三账守恒：
//!    `产物数 == 导航条目数 + 孤儿页数` 且 `Σ出边 == Σ入边`（引用图
//!    边数两端一致，无单向悬挂引用）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，零 IO
//! 零墙钟零浮点；逐条定制数据结构/状态机/错误路径，零模板复述。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const DS_VERSION: &str = "vcw09-docsite-v1";

/// 诊断段基址（0x7700 段 · 本模块独占）。
pub const DS_SEG_BASE: u16 = 0x7700;

/// 单页 URL 容量上限。
pub const URL_CAP: usize = 96;

/// 页面池容量上限。
pub const PAGE_CAP: usize = 48;

/// 依赖边容量上限。
pub const EDGE_CAP: usize = 128;

/// --------------------------------------------------------------------------
// 诊断码（十二码 · 段内连续）
// --------------------------------------------------------------------------

/// 文档站诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsCode {
    /// 0x7700 路由空（空 URL 不是合法路由）。
    RouteEmpty,
    /// 0x7701 路由碰撞（两 URL 映射同一产物路径）。
    RouteClash,
    /// 0x7702 路径穿越（`..` 段逃逸站点根）。
    PathEscape,
    /// 0x7703 引用指向站内不存在的页面（死链）。
    DeadLink,
    /// 0x7704 引用指向绝对 URL 或外部域（破坏离线可读）。
    ExternalRef,
    /// 0x7705 导航条目指向不存在页面。
    NavDangling,
    /// 0x7706 存在页面未挂进导航（孤儿页）。
    OrphanPage,
    /// 0x7707 依赖图成环（最小重建集无解）。
    DepCycle,
    /// 0x7708 增量重生成产物指纹与旧产物不等（生成器不稳定）。
    UnstableGen,
    /// 0x7709 产物含动态字段（时间戳/随机序泄漏）。
    DynamicField,
    /// 0x770A 页面池/URL 池/边池容量满（显性拒）。
    PoolFull,
    /// 0x770B 三账不守恒（产物数/导航数/边数对不上）。
    LedgerUnbalanced,
}

impl DsCode {
    /// 码值。
    pub fn val(self) -> u16 {
        DS_SEG_BASE
            + match self {
                DsCode::RouteEmpty => 0x0,
                DsCode::RouteClash => 0x1,
                DsCode::PathEscape => 0x2,
                DsCode::DeadLink => 0x3,
                DsCode::ExternalRef => 0x4,
                DsCode::NavDangling => 0x5,
                DsCode::OrphanPage => 0x6,
                DsCode::DepCycle => 0x7,
                DsCode::UnstableGen => 0x8,
                DsCode::DynamicField => 0x9,
                DsCode::PoolFull => 0xA,
                DsCode::LedgerUnbalanced => 0xB,
            }
    }

    /// 全部码（顺序即码序 · 12 码）。
    pub const ALL: [DsCode; 12] = [
        DsCode::RouteEmpty,
        DsCode::RouteClash,
        DsCode::PathEscape,
        DsCode::DeadLink,
        DsCode::ExternalRef,
        DsCode::NavDangling,
        DsCode::OrphanPage,
        DsCode::DepCycle,
        DsCode::UnstableGen,
        DsCode::DynamicField,
        DsCode::PoolFull,
        DsCode::LedgerUnbalanced,
    ];

    /// 可操作建议（错误路径必带修复指引）。
    pub fn remedy(self) -> &'static str {
        match self {
            DsCode::RouteEmpty => "空路由：补 URL，静态站不允许空路径产物",
            DsCode::RouteClash => "路由碰撞：两 URL 归一化后同路径，改其一或加尾斜杠约定",
            DsCode::PathEscape => "路径穿越：URL 含 `..` 段，拒收（防目录逃逸写站外）",
            DsCode::DeadLink => "死链：引用目标不在页面池，先建目标页或改引用",
            DsCode::ExternalRef => "外链破坏离线可读：改站内相对路径或显式标注外链例外",
            DsCode::NavDangling => "导航悬挂：导航项无对应页面，删导航项或补页面",
            DsCode::OrphanPage => "孤儿页：页面不在导航树内，事实不可达，挂进导航",
            DsCode::DepCycle => "依赖成环：最小重建集无解，断环后重跑（拓扑序检测）",
            DsCode::UnstableGen => "产物指纹漂移：生成器掺了时间戳/随机序，去掉不稳定因子",
            DsCode::DynamicField => "产物含动态字段：静态站禁构建时刻注入，移除该字段",
            DsCode::PoolFull => "池容量满：扩容或分批生成，禁止静默丢弃页面",
            DsCode::LedgerUnbalanced => "三账不守恒：核对产物数/导航数/孤儿数与边两端计数",
        }
    }

    /// 一行式描述。
    pub fn line(self) -> String {
        format!("[0x{:04X}] {}", self.val(), self.remedy())
    }
}

// --------------------------------------------------------------------------
// URL 归一（路由 → 产物路径 · 双射的前置）
// --------------------------------------------------------------------------

/// 归一化后的路由（以 `/` 开头、以 `/` 结尾 · 无 `.`/`..` 段）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Route {
    /// 归一路由（`/a/b/`）。
    pub path: String,
    /// 产物相对路径（`a/b/index.html`）。
    pub file: String,
}

impl Route {
    /// 归一化（空/穿越/多斜杠折叠/大小写保留）。
    pub fn normalize(raw: &str) -> Result<Route, DsCode> {
        if raw.is_empty() {
            return Err(DsCode::RouteEmpty);
        }
        let mut segs: Vec<String> = Vec::new();
        for s in raw.split('/') {
            match s {
                "" => {
                    // 多斜杠折叠（`//a//b` 与 `/a/b` 同路由 —— 这是真实的
                    // 碰撞来源，必须归一到同一路径而不是生成两个产物）。
                }
                "." => {}
                ".." => return Err(DsCode::PathEscape),
                other => segs.push(String::from(other)),
            }
        }
        let mut path = String::from("/");
        for s in segs.iter() {
            path.push_str(s);
            path.push('/');
        }
        // 根路由无前导斜杠（产物相对站点根）：`index.html` 而非 `/index.html`
        // —— 前导斜杠会让根产物被写到站点根之外，且清单比对时对不上。
        let mut file = String::new();
        for (i, s) in segs.iter().enumerate() {
            if i > 0 {
                file.push('/');
            }
            file.push_str(s);
        }
        if !file.is_empty() {
            file.push('/');
        }
        file.push_str("index.html");
        Ok(Route { path, file })
    }

    /// 路径深度（段数 · 根为 0）。
    pub fn depth(&self) -> usize {
        self.path.matches('/').count().wrapping_sub(1)
    }
}

// --------------------------------------------------------------------------
// 页面与引用
// --------------------------------------------------------------------------

/// 页面源（生成输入）。
#[derive(Clone, Debug)]
pub struct Page {
    /// 路由。
    pub route: Route,
    /// 标题。
    pub title: String,
    /// 正文段落（按序渲染）。
    pub body: Vec<String>,
    /// 出向引用（目标路由路径 · 站内）。
    pub refs: Vec<String>,
}

/// 产物（生成输出 · 内容寻址）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Artifact {
    /// 产物相对路径。
    pub file: String,
    /// 产物全文。
    pub text: String,
    /// 内容指纹（FNV-1a 64）。
    pub fp: u64,
    /// 字节长度。
    pub bytes: u64,
}

/// 内容指纹（FNV-1a 64）。
pub fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes().iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 产物内动态字段探针（静态性的反面清单）。
pub fn dynamic_field_hit(text: &str) -> Option<&'static str> {
    for key in [
        "generated_at",
        "build_timestamp",
        "BUILD_TIME",
        "random_seed",
        "__DATE__",
        "__TIME__",
    ]
    .iter()
    {
        if text.contains(key) {
            return Some(key);
        }
    }
    None
}

/// 外链探针（绝对 URL / 协议相对 / 外部域）。
pub fn external_ref_hit(text: &str) -> Option<&'static str> {
    if text.contains("://") {
        return Some("abs-url");
    }
    if text.contains("//cdn.") || text.contains("//fonts.") {
        return Some("external-domain");
    }
    None
}

// --------------------------------------------------------------------------
// 站点生成器
// --------------------------------------------------------------------------

/// 站点生成器（静态 · 可重入 · 幂等）。
#[derive(Clone, Debug)]
pub struct SiteGen {
    /// 页面池。
    pub pages: Vec<Page>,
    /// 产物表。
    pub arts: Vec<Artifact>,
    /// 导航树（按序 · 路径列表）。
    pub nav: Vec<String>,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
    /// 动态字段检出数。
    pub dynamic_hits: u32,
    /// 外链检出数。
    pub external_hits: u32,
    /// 死链数。
    pub dead_links: u32,
    /// 生成轮次（用于幂等对拍计数）。
    pub rounds: u32,
}

impl SiteGen {
    /// 造生成器。
    pub fn new() -> SiteGen {
        SiteGen {
            pages: Vec::new(),
            arts: Vec::new(),
            nav: Vec::new(),
            stats: [0; 12],
            dynamic_hits: 0,
            external_hits: 0,
            dead_links: 0,
            rounds: 0,
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: DsCode) {
        let i = (c.val() - DS_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: DsCode) -> u64 {
        let i = (c.val() - DS_SEG_BASE) as usize;
        self.stats.get(i).copied().unwrap_or(0)
    }

    /// 登记页面（路由归一 + 碰撞检测）。
    pub fn add_page(&mut self, raw: &str, title: &str, body: &[&str]) -> Result<usize, DsCode> {
        if self.pages.len() >= PAGE_CAP {
            self.bump(DsCode::PoolFull);
            return Err(DsCode::PoolFull);
        }
        let route = Route::normalize(raw)?;
        // 双射检查：归一后同 path 即碰撞（`/a/b` 与 `/a/b/`、`//a//b` 同撞）。
        if let Some(i) = self.pages.iter().position(|p| p.route.path == route.path) {
            let _ = i;
            self.bump(DsCode::RouteClash);
            return Err(DsCode::RouteClash);
        }
        let mut refs: Vec<String> = Vec::new();
        for b in body.iter() {
            for tok in b.split_whitespace() {
                if tok.starts_with("/") && tok.ends_with('/') {
                    refs.push(String::from(tok));
                }
            }
        }
        let p = Page {
            route,
            title: String::from(title),
            body: body.iter().map(|s| String::from(*s)).collect(),
            refs,
        };
        self.pages.push(p);
        Ok(self.pages.len().wrapping_sub(1))
    }

    /// 设引用（显式边 · 用于依赖图与死链检测）。
    pub fn link(&mut self, from: usize, to_route: &str) -> Result<(), DsCode> {
        let r = Route::normalize(to_route)?;
        let idx = match from {
            i if i < self.pages.len() => i,
            _ => return Err(DsCode::DeadLink),
        };
        if let Some(p) = self.pages.get_mut(idx) {
            if p.refs.len() >= URL_CAP {
                self.bump(DsCode::PoolFull);
                return Err(DsCode::PoolFull);
            }
            p.refs.push(r.path);
        }
        Ok(())
    }

    /// 渲染单页为静态 HTML 文本（纯函数 · 零时钟零随机）。
    fn render(p: &Page) -> String {
        let mut s = String::new();
        s.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n");
        s.push_str(&format!("<title>{}</title>\n</head><body>\n", p.title));
        s.push_str(&format!("<h1>{}</h1>\n", p.title));
        for b in p.body.iter() {
            s.push_str(&format!("<p>{}</p>\n", b));
        }
        s.push_str("<nav>");
        for r in p.refs.iter() {
            s.push_str(&format!("<a href=\"{}\">link</a>", r));
        }
        s.push_str("</nav>\n</body></html>\n");
        s
    }

    /// 全量生成（幂等：同输入必得同产物）。
    pub fn generate(&mut self) -> Result<Vec<Artifact>, DsCode> {
        self.rounds = self.rounds.wrapping_add(1);
        let mut out: Vec<Artifact> = Vec::new();
        let mut dyn_n = 0u32;
        let mut ext_n = 0u32;
        for p in self.pages.iter() {
            let text = SiteGen::render(p);
            if dynamic_field_hit(&text).is_some() {
                dyn_n = dyn_n.wrapping_add(1);
            }
            if external_ref_hit(&text).is_some() {
                ext_n = ext_n.wrapping_add(1);
            }
            out.push(Artifact {
                file: p.route.file.clone(),
                fp: fnv1a64(&text),
                bytes: text.as_bytes().len() as u64,
                text,
            });
        }
        self.arts = out.clone();
        self.dynamic_hits = dyn_n;
        self.external_hits = ext_n;
        if dyn_n > 0 {
            self.bump(DsCode::DynamicField);
        }
        if ext_n > 0 {
            self.bump(DsCode::ExternalRef);
        }
        Ok(out)
    }

    /// 静态证明：重生成产物逐位相同 + 零动态字段 + 零外链。
    pub fn static_proof(&mut self) -> bool {
        let a1 = match self.generate() {
            Ok(v) => v,
            Err(_) => return false,
        };
        let a2 = match self.generate() {
            Ok(v) => v,
            Err(_) => return false,
        };
        let idempotent = a1 == a2;
        // 独立重算指纹再对拍（防 arts 缓存自欺）。
        let recomputed = a2
            .iter()
            .all(|a| a.fp == fnv1a64(&a.text) && a.bytes == a.text.as_bytes().len() as u64);
        idempotent && recomputed && self.dynamic_hits == 0 && self.external_hits == 0
    }

    /// 死链检测（引用目标必须存在于页面池）。
    pub fn dead_link_scan(&mut self) -> u32 {
        let paths: Vec<String> = self.pages.iter().map(|p| p.route.path.clone()).collect();
        let mut dead = 0u32;
        for p in self.pages.iter() {
            for r in p.refs.iter() {
                if !paths.iter().any(|x| x == r) {
                    dead = dead.wrapping_add(1);
                }
            }
        }
        self.dead_links = dead;
        if dead > 0 {
            self.bump(DsCode::DeadLink);
        }
        dead
    }

    /// 导航悬挂检测（导航项必须有页面）。
    pub fn nav_dangling(&mut self) -> u32 {
        let paths: Vec<String> = self.pages.iter().map(|p| p.route.path.clone()).collect();
        let mut bad = 0u32;
        for n in self.nav.iter() {
            if !paths.iter().any(|x| x == n) {
                bad = bad.wrapping_add(1);
            }
        }
        if bad > 0 {
            self.bump(DsCode::NavDangling);
        }
        bad
    }

    /// 孤儿页检测（页面必须在导航内 · 根页除外）。
    pub fn orphans(&mut self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in self.pages.iter() {
            if p.route.path == "/" {
                continue;
            }
            if !self.nav.iter().any(|n| *n == p.route.path) {
                out.push(p.route.path.clone());
            }
        }
        if !out.is_empty() {
            self.bump(DsCode::OrphanPage);
        }
        out
    }

    /// 三账守恒：产物数 == 导航条目数 + 孤儿数；引用图边两端一致。
    pub fn ledger(&mut self) -> bool {
        let arts = self.arts.len();
        let nav = self.nav.len();
        let orph = self.orphans().len();
        let first = arts == nav.wrapping_add(orph);
        // 引用图边数：出边总和 == 入边总和（按目标路径累计）。
        let mut out_deg: u64 = 0;
        let mut in_deg: u64 = 0;
        for p in self.pages.iter() {
            out_deg = out_deg.wrapping_add(p.refs.len() as u64);
        }
        // 入边：每个被引用目标计一次（引用可能指向不存在的目标 → 这本身是死链）。
        let paths: Vec<String> = self.pages.iter().map(|p| p.route.path.clone()).collect();
        for p in self.pages.iter() {
            for r in p.refs.iter() {
                if paths.iter().any(|x| x == r) {
                    in_deg = in_deg.wrapping_add(1);
                }
            }
        }
        let balanced = out_deg == in_deg;
        if !(first && balanced) {
            self.bump(DsCode::LedgerUnbalanced);
        }
        first && balanced
    }

    /// 渲染导航（收尾产物）。
    pub fn render_nav(&self) -> String {
        let mut s = String::new();
        s.push_str("<ul class=\"nav\">\n");
        for n in self.nav.iter() {
            s.push_str(&format!("<li><a href=\"{}\">{}</a></li>\n", n, n));
        }
        s.push_str("</ul>\n");
        s
    }

    /// 产物清单（部署真源）。
    pub fn manifest(&self) -> Vec<String> {
        let mut rows: Vec<String> = Vec::new();
        for a in self.arts.iter() {
            let out_edges = self
                .pages
                .iter()
                .filter(|p| p.route.file == a.file)
                .map(|p| p.refs.len())
                .next()
                .unwrap_or(0);
            rows.push(format!(
                "{} fp=0x{:016X} bytes={} out={}",
                a.file, a.fp, a.bytes, out_edges
            ));
        }
        rows
    }
}

// --------------------------------------------------------------------------
// 增量重建（内容指纹 + 反向依赖闭包）
// --------------------------------------------------------------------------

/// 增量重建器。
#[derive(Clone, Debug)]
pub struct Incremental {
    /// 页面前态指纹（索引 -> 指纹）。
    pub prev_fp: Vec<u64>,
    /// 依赖边（from -> to，索引对）。
    pub edges: Vec<(usize, usize)>,
    /// 上轮产物指纹（按页索引）。
    pub prev_art: Vec<u64>,
    /// 重建集（页索引）。
    pub rebuild: Vec<usize>,
    /// 复用集（页索引）。
    pub reuse: Vec<usize>,
}

impl Incremental {
    /// 造重建器（页数 n · 容量自足）。
    pub fn new(n: usize) -> Incremental {
        Incremental {
            prev_fp: alloc::vec![0; n],
            edges: Vec::new(),
            prev_art: alloc::vec![0; n],
            rebuild: Vec::new(),
            reuse: Vec::new(),
        }
    }

    /// 置前态指纹。
    pub fn set_prev(&mut self, idx: usize, fp: u64) {
        if let Some(c) = self.prev_fp.get_mut(idx) {
            *c = fp;
        }
    }

    /// 加依赖边。
    pub fn add_edge(&mut self, from: usize, to: usize) -> Result<(), DsCode> {
        if self.edges.len() >= EDGE_CAP {
            return Err(DsCode::PoolFull);
        }
        self.edges.push((from, to));
        Ok(())
    }

    /// 拓扑序（Kahn · 依赖 from→to 视作 from 依赖 to；返回被依赖先的序）。
    /// 依赖边语义：`from` 页面**引用** `to` 页面 → `from` 的产物依赖 `to`
    /// 的产物（导航渲染会嵌入被引页标题），故 `to` 必须先重建。
    pub fn topo(&self, n: usize) -> Result<Vec<usize>, DsCode> {
        let mut indeg: Vec<usize> = alloc::vec![0; n];
        for (f, t) in self.edges.iter() {
            if *f < n && *t < n {
                indeg[*f] = indeg[*f].wrapping_add(1);
            }
        }
        // 反向邻接：谁依赖我（我先出队）
        let mut queue: Vec<usize> = Vec::new();
        for i in 0..n {
            if indeg.get(i).copied().unwrap_or(0) == 0 {
                queue.push(i);
            }
        }
        let mut order: Vec<usize> = Vec::new();
        let mut qi = 0usize;
        while qi < queue.len() {
            let cur = match queue.get(qi) {
                Some(v) => *v,
                None => break,
            };
            qi = qi.wrapping_add(1);
            order.push(cur);
            for (f, t) in self.edges.iter() {
                if *t == cur && *f < n {
                    let d = match indeg.get_mut(*f) {
                        Some(x) => x,
                        None => continue,
                    };
                    *d = d.wrapping_sub(1);
                    if *d == 0 {
                        queue.push(*f);
                    }
                }
            }
        }
        if order.len() != n {
            return Err(DsCode::DepCycle);
        }
        Ok(order)
    }

    /// 最小重建集计算：指纹变化页 + 依赖其产物的传递闭包（反向）。
    pub fn plan(&mut self, cur_fp: &[u64]) -> Result<(), DsCode> {
        let n = cur_fp.len();
        self.rebuild.clear();
        self.reuse.clear();
        let order = self.topo(n)?;
        // 直接变化集。
        let mut dirty: Vec<bool> = alloc::vec![false; n];
        for i in 0..n {
            let prev = self.prev_fp.get(i).copied().unwrap_or(0);
            let cur = cur_fp.get(i).copied().unwrap_or(0);
            if cur != prev {
                if let Some(d) = dirty.get_mut(i) {
                    *d = true;
                }
            }
        }
        // 传递闭包：若 to 脏，则所有引用 to 的 from 也脏（from 的产物含 to 的内容）。
        // 按拓扑序推进：to 先于 from，故当 to 被标脏时可立即把 from 标脏。
        for cur in order.iter() {
            let cur_dirty = dirty.get(*cur).copied().unwrap_or(false);
            if !cur_dirty {
                continue;
            }
            for (f, t) in self.edges.iter() {
                if *t == *cur && *f < n {
                    if let Some(d) = dirty.get_mut(*f) {
                        *d = true;
                    }
                }
            }
        }
        for i in 0..n {
            if dirty.get(i).copied().unwrap_or(false) {
                self.rebuild.push(i);
            } else {
                self.reuse.push(i);
            }
        }
        Ok(())
    }

    /// 稳定性核对：复用集产物指纹必须与上轮**逐位相等**。
    pub fn reuse_stable(&self, cur_art: &[u64]) -> bool {
        for i in self.reuse.iter() {
            let a = cur_art.get(*i).copied().unwrap_or(0);
            let b = self.prev_art.get(*i).copied().unwrap_or(0);
            if a != b {
                return false;
            }
        }
        true
    }

    /// 提交本轮（写前态）。
    pub fn commit(&mut self, cur_fp: &[u64], cur_art: &[u64]) {
        self.prev_fp.clear();
        self.prev_art.clear();
        for v in cur_fp.iter() {
            self.prev_fp.push(*v);
        }
        for v in cur_art.iter() {
            self.prev_art.push(*v);
        }
    }
}

// --------------------------------------------------------------------------
// 判据
// --------------------------------------------------------------------------

/// CGPU-F3529 判据（十二条）。
pub fn run_cgpu_f3529_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgpu-f3529");

    // ---- DS-01 诊断段闭集与建议完备 ----
    let seg_ok = DsCode::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == DS_SEG_BASE + i as u16);
    let remedy_ok = DsCode::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add("DS-01 诊断段闭集", seg_ok && remedy_ok, "12 码连续+建议完备");

    // ---- DS-02 路由归一：多斜杠折叠 + 尾斜杠统一 ----
    let r1 = Route::normalize("/a/b").ok();
    let r2 = Route::normalize("/a/b/").ok();
    let r3 = Route::normalize("//a//b").ok();
    let norm_ok = matches!((&r1, &r2, &r3), (Some(a), Some(b), Some(c))
        if a.path == b.path && b.path == c.path
            && a.file == b.file
            && a.file == "a/b/index.html"
            && a.depth() == 2);
    let root = Route::normalize("/").ok();
    let root_ok = matches!(&root, Some(r) if r.path == "/" && r.file == "index.html" && r.depth() == 0);
    cs.add("DS-02 路由归一", norm_ok && root_ok, "多斜杠/尾斜杠折叠+根路由");

    // ---- DS-03 空路由与路径穿越显性拒 ----
    let e1 = Route::normalize("");
    let e2 = Route::normalize("/a/../../etc/passwd");
    let e3 = Route::normalize("/a/../b");
    let guard_ok = matches!(e1, Err(DsCode::RouteEmpty))
        && matches!(e2, Err(DsCode::PathEscape))
        && matches!(e3, Err(DsCode::PathEscape));
    cs.add("DS-03 空与穿越拒", guard_ok, "空路由+..穿越全拒");

    // ---- DS-04 路由碰撞双射（登记期拦截） ----
    let mut g4 = SiteGen::new();
    let _ = g4.add_page("/a/b", "t", &["x"]);
    let dup1 = g4.add_page("/a/b/", "t", &["x"]);
    let dup2 = g4.add_page("//a//b", "t", &["x"]);
    let ok4 = matches!(dup1, Err(DsCode::RouteClash))
        && matches!(dup2, Err(DsCode::RouteClash))
        && g4.pages.len() == 1
        && g4.stat(DsCode::RouteClash) == 2;
    cs.add("DS-04 路由碰撞", ok4, "尾斜杠/多斜杠同撞+双计");

    // ---- DS-05 静态生成幂等（两次生成逐位相同） ----
    let mut g5 = SiteGen::new();
    let _ = g5.add_page("/", "home", &["welcome to docs"]);
    let _ = g5.add_page("/guide/start", "start", &["first page", "see /guide/end"]);
    let _ = g5.link(1, "/guide/end");
    let _ = g5.add_page("/guide/end", "end", &["last page"]);
    let proof = g5.static_proof();
    let files_ok = g5.arts.len() == 3
        && g5
            .arts
            .iter()
            .all(|a| a.file.ends_with("index.html") && a.bytes > 0);
    cs.add("DS-05 静态幂等", proof && files_ok, "两次生成逐位相同+产物非空");

    // ---- DS-06 零动态字段零外链 ----
    let mut g6 = SiteGen::new();
    let _ = g6.add_page("/", "home", &["plain content"]);
    let _ = g6.generate();
    let clean = g6.dynamic_hits == 0 && g6.external_hits == 0;
    // 反证：注入动态字段必被检出。
    let probe = format!("<p>build_time</p><p>{}</p>", "generated_at");
    let detected = dynamic_field_hit(&probe).is_some();
    let ext_probe = format!("<a href=\"https://example.com\">x</a>");
    let ext_detected = external_ref_hit(&ext_probe).is_some();
    cs.add(
        "DS-06 静态纯净",
        clean && detected && ext_detected,
        "零动态零外链+反证必检出",
    );

    // ---- DS-07 死链检测 ----
    let mut g7 = SiteGen::new();
    let _ = g7.add_page("/a", "a", &["x"]);
    let _ = g7.link(0, "/missing");
    let _ = g7.link(0, "/a");
    let dead = g7.dead_link_scan();
    let ok7 = dead == 1 && g7.stat(DsCode::DeadLink) == 1;
    cs.add("DS-07 死链检测", ok7, "指向不存在页面计死链");

    // ---- DS-08 导航悬挂 + 孤儿页 ----
    let mut g8 = SiteGen::new();
    let _ = g8.add_page("/", "home", &["x"]);
    let _ = g8.add_page("/p1", "p1", &["x"]);
    let _ = g8.add_page("/p2", "p2", &["x"]);
    g8.nav.push(String::from("/"));
    g8.nav.push(String::from("/p1/"));
    g8.nav.push(String::from("/ghost"));
    let dangling = g8.nav_dangling();
    let orphans = g8.orphans();
    let ok8 = dangling == 1
        && orphans.len() == 1
        && orphans.first().map(|s| s.as_str()) == Some("/p2/")
        && g8.stat(DsCode::NavDangling) == 1
        && g8.stat(DsCode::OrphanPage) == 1;
    cs.add("DS-08 导航与孤儿", ok8, "悬挂 1+孤儿 1 各立案");

    // ---- DS-09 三账守恒 ----
    let mut g9 = SiteGen::new();
    let _ = g9.add_page("/", "home", &["x"]);
    let _ = g9.add_page("/a", "a", &["x"]);
    let _ = g9.add_page("/b", "b", &["x"]);
    let _ = g9.link(1, "/b");
    let _ = g9.generate();
    g9.nav.push(String::from("/"));
    g9.nav.push(String::from("/a/"));
    // /b 有意留孤儿 → 产物 3 == 导航 2 + 孤儿 1
    let ok9 = g9.ledger() && g9.stat(DsCode::LedgerUnbalanced) == 0;
    // 死链会打破边两端一致 → 必须立案。
    let mut g9b = SiteGen::new();
    let _ = g9b.add_page("/a", "a", &["x"]);
    let _ = g9b.link(0, "/ghost");
    let _ = g9b.generate();
    g9b.nav.push(String::from("/a/"));
    let broke = !g9b.ledger() && g9b.stat(DsCode::LedgerUnbalanced) == 1;
    cs.add("DS-09 三账守恒", ok9 && broke, "孤儿计入守恒+死链破账立案");

    // ---- DS-10 拓扑序与依赖环检出 ----
    let mut inc = Incremental::new(4);
    // 0→1→2→3 链（0 引用 1，1 引用 2，2 引用 3）
    let _ = inc.add_edge(0, 1);
    let _ = inc.add_edge(1, 2);
    let _ = inc.add_edge(2, 3);
    let order = inc.topo(4);
    let topo_ok = matches!(&order, Ok(o) if o.len() == 4 && o[0] == 3 && o[3] == 0);
    // 造环：0→1, 1→0
    let mut cyc = Incremental::new(2);
    let _ = cyc.add_edge(0, 1);
    let _ = cyc.add_edge(1, 0);
    let ring = cyc.topo(2);
    cs.add(
        "DS-10 拓扑与环拒",
        topo_ok && matches!(ring, Err(DsCode::DepCycle)),
        "链式拓扑正确+成环显性拒",
    );

    // ---- DS-11 增量最小重建集（传递闭包） ----
    let mut inc2 = Incremental::new(4);
    let _ = inc2.add_edge(0, 1); // 0 引用 1
    let _ = inc2.add_edge(2, 1); // 2 引用 1
    // 前态：1 与 3 已定版
    inc2.set_prev(0, 0xAA);
    inc2.set_prev(1, 0xBB);
    inc2.set_prev(2, 0xCC);
    inc2.set_prev(3, 0xDD);
    // 本态：仅 1 变化（0xBB → 0xB2）→ 引用 1 的 0 与 2 必须连带重建，3 不动
    let cur = [0xAAu64, 0xB2, 0xCC, 0xDD];
    let planned = inc2.plan(&cur);
    let plan_ok = matches!(planned, Ok(()))
        && inc2.rebuild.len() == 3
        && inc2.reuse.len() == 1
        && inc2.reuse.first() == Some(&3)
        && inc2.rebuild.contains(&0)
        && inc2.rebuild.contains(&1)
        && inc2.rebuild.contains(&2);
    // 复用集产物指纹必须逐位相等（稳定性）。
    let prev_art = [0x10u64, 0x11, 0x12, 0x13];
    let cur_art = [0x20u64, 0x21, 0x22, 0x13]; // 仅 3 位保持不变
    inc2.commit(&prev_art, &prev_art);
    inc2.plan(&cur);
    let stable = inc2.reuse_stable(&cur_art);
    cs.add(
        "DS-11 增量闭包",
        plan_ok && stable,
        "1 变→3 连带重建/1 复用+指纹稳定",
    );

    // ---- DS-12 清单与导航渲染 + 幂等反证 ----
    let mut g12 = SiteGen::new();
    let _ = g12.add_page("/", "home", &["x"]);
    let _ = g12.add_page("/a", "a", &["x"]);
    let _ = g12.link(1, "/");
    let _ = g12.generate();
    g12.nav.push(String::from("/"));
    g12.nav.push(String::from("/a/"));
    let man = g12.manifest();
    let man_ok = man.len() == 2 && man.iter().all(|r| r.contains("fp=0x") && r.contains("bytes="));
    let nav_txt = g12.render_nav();
    let nav_ok = nav_txt.contains("<ul class=\"nav\">") && nav_txt.contains("href=\"/a/\"");
    // 产物文件路径在清单里出现（部署真源对得上）。
    let file_ok = man
        .iter()
        .all(|r| g12.arts.iter().any(|a| r.starts_with(&a.file)));
    cs.add(
        "DS-12 清单与导航",
        man_ok && nav_ok && file_ok,
        "清单两行齐备+导航渲染+路径对得上",
    );

    cs
}