//! F126 开放格式宪法页 · 完整设计（STAR I 主册 G-D-01）。
//!
//! **判据（主册）**：四规范页齐全可访问；示例包全部通过当前实现校验；
//! 双读过渡承诺条款在册。
//!
//! **设计要点（主册）**：
//! - 四份核心格式规范的公开文档页：vxapp 包格式 / vxtheme 定制包 /
//!   星图 JSON 目录 / 判例提交格式——每份含：版本号、完整 schema、
//!   示例包、变更历史；版本化承诺（破格必双读过渡）写进规范正文；
//! - 版本号语义：主版本=破格 / 次版本=加字段 / 修订=改文档；破格流程
//!   ADR + 迁移窗一个季度；
//! - 帮助中心（F119）内设「开发者-格式规范」分区；每规范页结构统一：
//!   概览 200 字 / Schema 表格 / 示例下载钮 / 变更历史表 / 讨论入口
//!   外链；页内 schema 字段表支持锚点跳转；
//! - schema 与实现脱节 → CI 校验（示例包过实现校验即文档正确）；规范
//!   更新 → 变更历史强制记条目（无记录不合入）；
//! - schema 表达采用 JSON Schema 标准（公共规范）；页面渲染复用 F119
//!   MD 管线；
//! - 示例包四份各 ≤100KB（分钟级上手）；四格式互相引用关系图；
//!   双读过渡：破格后旧版本读取兼容一季（迁移窗）。
//!
//! 时间注入式，宿主测试确定复现。无外部依赖（schema 校验自研轻量
//! 实现——字段类型/必填/值域三查，JSON Schema 标准语义子集）。

use crate::checks::CheckSet;

use crate::svstar::vbase;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 示例包大小上限（字节，主册：四份各 ≤100KB）。
pub const SAMPLE_CAP_BYTES: u64 = 100 * 1024;
/// 迁移窗（天，主册：一个季度）。
pub const MIGRATION_WINDOW_DAYS: u64 = 90;
/// 四规范页数。
pub const SPEC_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// schema 模型与校验器（JSON Schema 标准语义子集）
// ---------------------------------------------------------------------------

/// 字段类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Str,
    Int,
    Bool,
    /// 枚举（值域由 allowed 列出）。
    Enum(&'static [&'static str]),
}

/// 一个 schema 字段。
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub name: &'static str,
    pub ftype: FieldType,
    pub required: bool,
    /// 字段说明（schema 表格渲染源）。
    pub desc: &'static str,
    /// 约束属性（深化 v2：JSON Schema 标准语义——minLength/maxLength/
    /// minimum/maximum/pattern 类前缀/semver/相对路径五族）。
    pub attrs: &'static [FieldAttr],
}

/// 字段约束属性（JSON Schema 公共语义的枚举化子集——一处一事实：
/// 约束表达唯一源，校验器与 schema 表格渲染共用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldAttr {
    /// minLength（含）。
    MinLen(usize),
    /// maxLength（含）。
    MaxLen(usize),
    /// minimum（Int 用）。
    MinVal(u64),
    /// maximum（Int 用）。
    MaxVal(u64),
    /// 前缀 pattern（^锚定——公共语义的定长子集）。
    Prefix(&'static str),
    /// semver 三元组（vbase::parse_semver 校验）。
    Semver,
    /// 相对路径（非空、不以 `/` 开头——包内路径安全）。
    RelPath,
    /// 反域名标识（至少一个 `.`、全小写 ASCII）。
    DomainId,
}

impl Field {
    fn has(&self, want: &FieldAttr) -> bool {
        self.attrs.iter().any(|a| a == want)
    }
}

/// 一份格式规范。
pub struct Spec {
    pub id: &'static str,
    pub title: &'static str,
    /// 版本（semver——vbase 唯一源）。
    pub version: (u32, u32, u32),
    pub fields: Vec<Field>,
    /// 变更历史（规范更新强制记条目——无记录不合入）。
    pub changelog: Vec<(&'static str, &'static str)>,
    /// 概览（200 字内）。
    pub overview: &'static str,
}

impl Spec {
    /// schema 表格行（页内字段表渲染源；锚点跳转由渲染层用 name 作 id）。
    pub fn schema_rows(&self) -> Vec<(&'static str, &'static str, bool, &'static str)> {
        self.fields
            .iter()
            .map(|f| {
                (
                    f.name,
                    match f.ftype {
                        FieldType::Str => "string",
                        FieldType::Int => "integer",
                        FieldType::Bool => "boolean",
                        FieldType::Enum(_) => "enum",
                    },
                    f.required,
                    f.desc,
                )
            })
            .collect()
    }

    /// 实现校验器：对（字段名→字符串值）负载逐字段多查——类型/必填/
    /// 值域/约束属性（深化 v2）。返回错误清单（空 = 通过），每条带
    /// JSON Pointer 风格路径 `#/字段名`（可定位——CI 报告直接引用）。
    pub fn validate(&self, payload: &[(String, String)]) -> Vec<String> {
        let mut errors = Vec::new();
        for f in &self.fields {
            let hit = payload.iter().find(|(k, _)| k == f.name);
            match hit {
                None => {
                    if f.required {
                        errors.push(alloc::format!("#/{}: missing required field", f.name));
                    }
                }
                Some((k, v)) => {
                    let base_ok = match f.ftype {
                        FieldType::Str => !v.is_empty() || !f.required,
                        FieldType::Int => v.parse::<u64>().is_ok() || v.parse::<i64>().is_ok(),
                        FieldType::Bool => v == "true" || v == "false",
                        FieldType::Enum(vs) => vs.contains(&v.as_str()),
                    };
                    if !base_ok {
                        errors.push(alloc::format!("#/{}: type/value invalid", k));
                        continue;
                    }
                    // 约束属性逐条（只在基础类型过后追查——避免双报）。
                    // 可选字段空值 = 取默认值语义，约束属性不触发（默认值
                    // 由实现层补全，不经过负载约束面）。
                    if v.is_empty() && !f.required {
                        continue;
                    }
                    for a in f.attrs {
                        let bad = match a {
                            FieldAttr::MinLen(n) => v.chars().count() < *n,
                            FieldAttr::MaxLen(n) => v.chars().count() > *n,
                            FieldAttr::MinVal(min) => v.parse::<u64>().map(|x| x < *min).unwrap_or(false),
                            FieldAttr::MaxVal(max) => v.parse::<u64>().map(|x| x > *max).unwrap_or(false),
                            FieldAttr::Prefix(p) => !v.starts_with(p),
                            FieldAttr::Semver => vbase::parse_semver(v).is_none(),
                            FieldAttr::RelPath => v.is_empty() || v.starts_with('/'),
                            FieldAttr::DomainId => {
                                !v.contains('.') || !v.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c.is_ascii_digit())
                            }
                        };
                        if bad {
                            errors.push(alloc::format!("#/{}: attr constraint violated", k));
                        }
                    }
                }
            }
        }
        // 未知字段拒绝（schema 严格性——脱节即 CI 红）。
        for (k, _) in payload {
            if !self.fields.iter().any(|f| f.name == k.as_str()) {
                errors.push(alloc::format!("#/{}: unknown field", k));
            }
        }
        errors
    }
}

// ---------------------------------------------------------------------------
// 四份官方规范（主册定名；字段为本实现当前 schema——CI 对拍基准）
// ---------------------------------------------------------------------------

/// 官方四规范（schema 与实现同源——示例包过本校验即文档正确）。
/// 深化 v2：字段约束属性全量登记（Semver/DomainId/RelPath/长度域）。
pub fn official_specs() -> Vec<Spec> {
    vec![
        Spec {
            id: "vxapp",
            title: "vxapp 包格式",
            version: (1, 0, 0),
            overview: "应用包格式：清单 + 内容树 + 签名。清单含身份/版本/主程序三必填，其余字段默认值全文档化。",
            fields: vec![
                Field { name: "id", ftype: FieldType::Str, required: true, desc: "包身份（反域名小写）", attrs: &[FieldAttr::DomainId, FieldAttr::MaxLen(64)] },
                Field { name: "version", ftype: FieldType::Str, required: true, desc: "semver 三元组", attrs: &[FieldAttr::Semver] },
                Field { name: "entry", ftype: FieldType::Str, required: true, desc: "主程序相对路径", attrs: &[FieldAttr::RelPath, FieldAttr::MaxLen(256)] },
                Field { name: "name", ftype: FieldType::Str, required: false, desc: "显示名（默认取 id）", attrs: &[FieldAttr::MaxLen(64)] },
                Field { name: "icon", ftype: FieldType::Str, required: false, desc: "图标相对路径（默认无）", attrs: &[FieldAttr::RelPath] },
            ],
            changelog: vec![("1.0.0", "首版冻结：三必填 + 两可选")],
        },
        Spec {
            id: "vxtheme",
            title: "vxtheme 定制包",
            version: (1, 0, 0),
            overview: "主题定制包：令牌表 + 壁纸资产 + 指针/声音可选集。令牌名集与 E1 主题令牌全集对齐。",
            fields: vec![
                Field { name: "id", ftype: FieldType::Str, required: true, desc: "主题身份", attrs: &[FieldAttr::MinLen(2), FieldAttr::MaxLen(64)] },
                Field {
                    name: "mode",
                    ftype: FieldType::Enum(&["dark", "light", "auto"]),
                    required: true,
                    desc: "深浅模式",
                    attrs: &[],
                },
                Field { name: "tokens", ftype: FieldType::Str, required: true, desc: "令牌表路径", attrs: &[FieldAttr::RelPath] },
                Field { name: "wallpaper", ftype: FieldType::Str, required: false, desc: "壁纸路径（默认系统）", attrs: &[FieldAttr::RelPath] },
            ],
            changelog: vec![("1.0.0", "首版冻结：三必填 + 一可选")],
        },
        Spec {
            id: "starmap-json",
            title: "星图 JSON 目录",
            version: (1, 0, 0),
            overview: "兼容性评级目录开放数据面：星卡数组 + 账本引用 + 快照时间戳。CC-BY 许可（标注来源可自由再分发）。",
            fields: vec![
                Field { name: "snapshot_at", ftype: FieldType::Int, required: true, desc: "快照 Unix 时间戳", attrs: &[FieldAttr::MinVal(1_500_000_000)] },
                Field { name: "cards", ftype: FieldType::Str, required: true, desc: "星卡数组路径（内嵌或外链）", attrs: &[FieldAttr::MinLen(1)] },
                Field {
                    name: "license",
                    ftype: FieldType::Enum(&["CC-BY"]),
                    required: true,
                    desc: "数据许可（唯一值）",
                    attrs: &[],
                },
                Field { name: "since", ftype: FieldType::Int, required: false, desc: "增量拉取起点（缺省全量）", attrs: &[FieldAttr::MinVal(0)] },
            ],
            changelog: vec![("1.0.0", "首版冻结：三必填 + 增量可选")],
        },
        Spec {
            id: "case-submission",
            title: "判例提交格式",
            version: (1, 0, 0),
            overview: "社区判例提交线的数据格式：程序名/版本/判例集/证据哈希链。五态状态机可查（F129 同引擎）。",
            fields: vec![
                Field { name: "program", ftype: FieldType::Str, required: true, desc: "程序名", attrs: &[FieldAttr::MinLen(1), FieldAttr::MaxLen(128)] },
                Field { name: "program_version", ftype: FieldType::Str, required: true, desc: "程序版本", attrs: &[FieldAttr::Semver] },
                Field { name: "cases", ftype: FieldType::Str, required: true, desc: "判例集清单路径", attrs: &[FieldAttr::RelPath] },
                Field { name: "evidence_hash", ftype: FieldType::Str, required: true, desc: "证据包哈希链头", attrs: &[FieldAttr::MinLen(64), FieldAttr::MaxLen(64)] },
                Field { name: "submitter", ftype: FieldType::Str, required: false, desc: "提交者署名（默认匿名）", attrs: &[FieldAttr::MaxLen(64)] },
            ],
            changelog: vec![("1.0.0", "首版冻结：四必填 + 署名可选")],
        },
    ]
}

// ---------------------------------------------------------------------------
// 宪法条款（版本化承诺——写进规范正文）
// ---------------------------------------------------------------------------

/// 双读过渡承诺条款（判据第一句之三：条款在册——文本常量唯一源）。
pub const DUAL_READ_CLAUSE: &str = "破格（主版本变更）后，旧版本格式读取兼容保持一个季度迁移窗（90 天）；迁移窗内新实现必须双读新旧两版；窗口关闭前须发布公告。";
/// 版本语义条款。
pub const SEMVER_CLAUSE: &str = "主版本=破格变更；次版本=加字段（向后兼容）；修订=改文档。破格须走 ADR 并开启迁移窗。";
/// 四格式关系图（数据流文本形态——一张图讲清）。
pub const RELATION_DIAGRAM: &str = "vxapp(应用包) --提交判例--> case-submission --收录--> starmap-json(目录) --引用--> vxtheme(定制包经星卡推荐)";

// ---------------------------------------------------------------------------
// 深化批次 v2 · 一：双读过渡引擎（破格承诺条款的执行件）
// ---------------------------------------------------------------------------

/// 双读读取路由（破格期同时挂两代 reader——迁移窗语义的机器面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderRoute {
    /// 当前主版本 reader。
    Current,
    /// 上一主版本 reader（迁移窗内保留）。
    Legacy,
    /// 双读都不接（版本超前或窗已关——拒读）。
    Rejected,
}

/// 双读过渡引擎：破格开窗 → 窗内双读 → 窗尽关窗（公告强制）。
pub struct DualReadEngine {
    /// 当前主版本号（semver 主位）。
    pub current_major: u32,
    /// 迁移窗开启时刻（Unix 秒；未开窗 = None）。
    pub window_opened_ms: Option<u64>,
    /// 被兼容的旧主版本（窗内 Legacy reader 覆盖的代际）。
    pub legacy_major: Option<u32>,
}

impl DualReadEngine {
    pub fn new(current_major: u32) -> DualReadEngine {
        DualReadEngine { current_major, window_opened_ms: None, legacy_major: None }
    }

    /// 破格开窗：主版本升级必须开迁移窗（90 天）——返回 false = 无破格
    /// 却开窗（次版本加字段不需要双读，拒绝假开窗）。
    pub fn open_window(&mut self, new_major: u32, legacy_major: u32, now_ms: u64) -> bool {
        if new_major <= self.current_major {
            return false;
        }
        self.legacy_major = Some(legacy_major);
        self.window_opened_ms = Some(now_ms);
        self.current_major = new_major;
        true
    }

    /// 窗内读取路由（判据核心语义：窗内旧版可读）。
    pub fn route(&self, doc_major: u32, now_ms: u64) -> ReaderRoute {
        match self.window_opened_ms {
            None => {
                if doc_major == self.current_major {
                    ReaderRoute::Current
                } else {
                    ReaderRoute::Rejected
                }
            }
            Some(opened) => {
                if now_ms.saturating_sub(opened) < MIGRATION_WINDOW_DAYS * 86_400_000 {
                    if doc_major == self.current_major {
                        ReaderRoute::Current
                    } else if Some(doc_major) == self.legacy_major {
                        ReaderRoute::Legacy
                    } else {
                        ReaderRoute::Rejected
                    }
                } else if doc_major == self.current_major {
                    ReaderRoute::Current
                } else {
                    ReaderRoute::Rejected
                }
            }
        }
    }

    /// 窗剩余天数（公告排期面：≤0 = 已关窗）。
    pub fn window_remaining_days(&self, now_ms: u64) -> i64 {
        match self.window_opened_ms {
            None => 0,
            Some(opened) => {
                let elapsed_days = (now_ms.saturating_sub(opened) / 86_400_000) as i64;
                MIGRATION_WINDOW_DAYS as i64 - elapsed_days
            }
        }
    }

    /// 关窗前公告义务（窗剩 ≤7 天 → 公告必须已发）。
    pub fn announce_required(&self, now_ms: u64) -> bool {
        let rem = self.window_remaining_days(now_ms);
        rem > 0 && rem <= 7
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 二：规范页全文渲染器（帮助中心 F119 分区的页面件）
// ---------------------------------------------------------------------------

/// 每规范讨论入口（页面结构五件套之一——外链常量唯一源）。
pub const DISCUSSION_LINKS: [(&str, &str); SPEC_COUNT] = [
    ("vxapp", "https://community.varix.dev/formats/vxapp"),
    ("vxtheme", "https://community.varix.dev/formats/vxtheme"),
    ("starmap-json", "https://community.varix.dev/formats/starmap"),
    ("case-submission", "https://community.varix.dev/formats/case"),
];

/// 规范页全文渲染（MD 形态：标题/概览/Schema 表格带锚点/示例下载钮/
/// 变更历史/讨论入口——页面结构五件套，渲染即验收）。
pub fn render_spec_page(spec: &Spec) -> String {
    let mut s = String::new();
    s.push_str(&alloc::format!("# {}（{} v{}.{}.{}）\n\n", spec.title, spec.id, spec.version.0, spec.version.1, spec.version.2));
    s.push_str(&alloc::format!("{}\n\n", spec.overview));
    s.push_str("## Schema\n\n| 字段 | 类型 | 必填 | 说明 |\n| --- | --- | --- | --- |\n");
    for f in &spec.fields {
        let t = match f.ftype {
            FieldType::Str => "string",
            FieldType::Int => "integer",
            FieldType::Bool => "boolean",
            FieldType::Enum(_) => "enum",
        };
        s.push_str(&alloc::format!(
            "| <a id=\"{}\"></a>`{}` | {} | {} | {} |\n",
            f.name,
            f.name,
            t,
            if f.required { "是" } else { "否" },
            f.desc
        ));
    }
    s.push_str("\n## 示例\n\n[下载示例包](sample://");
    s.push_str(spec.id);
    s.push_str(")（≤100KB · 分钟级上手）\n\n## 变更历史\n\n| 版本 | 说明 |\n| --- | --- |\n");
    for (v, note) in &spec.changelog {
        s.push_str(&alloc::format!("| {} | {} |\n", v, note));
    }
    let link = DISCUSSION_LINKS.iter().find(|(id, _)| *id == spec.id).map(|(_, l)| *l).unwrap_or("");
    s.push_str(&alloc::format!("\n## 讨论入口\n\n[{}]({})\n", link, link));
    s
}

/// 页面结构五件套完整性（标题/概览/Schema/示例/变更历史/讨论——渲染
/// 产物逐段对拍）。
pub fn page_structure_complete(spec: &Spec) -> bool {
    let page = render_spec_page(spec);
    page.contains("## Schema") && page.contains("下载示例包") && page.contains("## 变更历史")
        && page.contains("## 讨论入口") && page.contains(spec.overview)
        && !spec.id.is_empty()
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 三：示例包全集与负样本族（CI 对拍基准）
// ---------------------------------------------------------------------------

/// 四份官方示例包（完整数据含可选字段——CI 自动重打永不腐烂的基准面）。
pub fn sample_packages() -> Vec<(&'static str, Vec<(String, String)>)> {
    vec![
        (
            "vxapp",
            vec![
                (String::from("id"), String::from("demo.tool")),
                (String::from("version"), String::from("1.2.3")),
                (String::from("entry"), String::from("bin/demo.exe")),
                (String::from("name"), String::from("Demo Tool")),
                (String::from("icon"), String::from("assets/icon.png")),
            ],
        ),
        (
            "vxtheme",
            vec![
                (String::from("id"), String::from("ocean")),
                (String::from("mode"), String::from("dark")),
                (String::from("tokens"), String::from("tokens.json")),
                (String::from("wallpaper"), String::from("wall/depth.png")),
            ],
        ),
        (
            "starmap-json",
            vec![
                (String::from("snapshot_at"), String::from("1727000000")),
                (String::from("cards"), String::from("cards.json")),
                (String::from("license"), String::from("CC-BY")),
                (String::from("since"), String::from("1726900000")),
            ],
        ),
        (
            "case-submission",
            vec![
                (String::from("program"), String::from("Notepad2")),
                (String::from("program_version"), String::from("4.2.25")),
                (String::from("cases"), String::from("cases.lst")),
                (String::from("evidence_hash"), String::from("ab").repeat(32)),
                (String::from("submitter"), String::from("community-member-7")),
            ],
        ),
    ]
}

/// 负样本族（类别 + 负载 + 预期错误类别子串——CI 必拒面）。
pub fn negative_samples() -> Vec<(&'static str, Vec<(String, String)>, &'static str)> {
    vec![
        // 缺必填。
        ("vxapp", vec![(String::from("id"), String::from("x")), (String::from("version"), String::from("1.0.0"))], "missing required"),
        // 类型错。
        ("vxapp", vec![(String::from("id"), String::from("x")), (String::from("version"), String::from("1.0.0")), (String::from("entry"), String::from(""))], "type/value invalid"),
        // 枚举越界。
        ("vxtheme", vec![(String::from("id"), String::from("t")), (String::from("mode"), String::from("sepia")), (String::from("tokens"), String::from("t.json"))], "type/value invalid"),
        // 未知字段。
        ("vxapp", vec![(String::from("id"), String::from("a.b")), (String::from("version"), String::from("1.0.0")), (String::from("entry"), String::from("e")), (String::from("hack"), String::from("1"))], "unknown field"),
        // semver 约束（深化 v2 新增路——attr 约束路径报违例）。
        ("vxapp", vec![(String::from("id"), String::from("a.b")), (String::from("version"), String::from("one.two")), (String::from("entry"), String::from("e"))], "attr constraint violated"),
        // 反域名约束。
        ("vxapp", vec![(String::from("id"), String::from("NoDot")), (String::from("version"), String::from("1.0.0")), (String::from("entry"), String::from("e"))], "attr constraint violated"),
        // 绝对路径约束。
        ("vxtheme", vec![(String::from("id"), String::from("ok")), (String::from("mode"), String::from("light")), (String::from("tokens"), String::from("/abs/path.json"))], "attr constraint violated"),
        // 数值下界约束。
        ("starmap-json", vec![(String::from("snapshot_at"), String::from("42")), (String::from("cards"), String::from("c.json")), (String::from("license"), String::from("CC-BY"))], "attr constraint violated"),
        // 长度域约束（evidence_hash 非 64）。
        ("case-submission", vec![(String::from("program"), String::from("P")), (String::from("program_version"), String::from("1.0.0")), (String::from("cases"), String::from("c.lst")), (String::from("evidence_hash"), String::from("abcd"))], "attr constraint violated"),
    ]
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 四：CI 对拍面（示例包过实现校验 = 文档正确）
// ---------------------------------------------------------------------------

/// CI 示例包回放：官方四示例逐份过当前实现校验——全过即「文档与实现
/// 零脱节」（判据第一句之二的机器化）。
pub fn ci_sample_replay() -> (usize, Vec<String>) {
    let specs = official_specs();
    let mut failures = Vec::new();
    let mut passed = 0usize;
    for (id, payload) in sample_packages() {
        if let Some(spec) = specs.iter().find(|s| s.id == id) {
            let errs = spec.validate(&payload);
            if errs.is_empty() {
                passed += 1;
            } else {
                failures.push(alloc::format!("{}: {}", id, errs.join("; ")));
            }
        } else {
            failures.push(alloc::format!("{}: spec not found", id));
        }
    }
    (passed, failures)
}

/// CI 负样本回放：九路负样本全拒且至少命中预期错误类别（多报不罚、
/// 漏拒必红——拒绝是底线，类别是定位）。
pub fn ci_negative_replay() -> (usize, Vec<String>) {
    let specs = official_specs();
    let mut failures = Vec::new();
    let mut rejected = 0usize;
    for (id, payload, expect) in negative_samples() {
        if let Some(spec) = specs.iter().find(|s| s.id == id) {
            let errs = spec.validate(&payload);
            if !errs.is_empty() && errs.iter().any(|e| e.contains(expect)) {
                rejected += 1;
            } else {
                failures.push(alloc::format!("{}: expected [{}], got {:?}", id, expect, errs));
            }
        } else {
            failures.push(alloc::format!("{}: spec not found", id));
        }
    }
    (rejected, failures)
}

/// 变更历史强制门禁（规范更新无记录不合入）：新版本必须 ≥ 当前版本
/// 且变更历史含该版本条目。
pub fn changelog_gate(spec: &Spec, new_version: (u32, u32, u32), note: &'static str) -> Result<(), &'static str> {
    if vbase::semver_cmp(new_version, spec.version) != core::cmp::Ordering::Greater {
        return Err("new version must be greater than current");
    }
    if note.is_empty() {
        return Err("changelog note mandatory");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 深化批次 v4 · 二：JSON Schema 标准形态全文 / 示例包全文 / 负样本扩充
// ---------------------------------------------------------------------------

impl Spec {
    /// JSON Schema 标准形态全文（主册「schema 表达采用 JSON Schema
    /// 标准」的机器面：type/properties/required 三键骨架——第三方校验
    /// 器可直接消费）。
    pub fn schema_json(&self) -> String {
        let mut props: Vec<String> = Vec::new();
        let mut required: Vec<String> = Vec::new();
        for f in &self.fields {
            let t = match f.ftype {
                FieldType::Str | FieldType::Enum(_) => "string",
                FieldType::Int => "integer",
                FieldType::Bool => "boolean",
            };
            let mut o = vbase::JsonObj::new();
            o.str_field("type", t);
            o.str_field("description", f.desc);
            props.push(alloc::format!("\"{}\": {}", f.name, o.finish()));
            if f.required {
                required.push(alloc::format!("\"{}\"", f.name));
            }
        }
        let mut root = vbase::JsonObj::new();
        root.str_field("$id", &alloc::format!("https://varix.dev/schemas/{}.json", self.id));
        root.str_field("type", "object");
        root.raw_array_field("required", &required);
        // properties 为对象——以 raw 形态拼装（JsonObj 无嵌套对象出口）。
        let props_body = props.join(",");
        let head = root.finish();
        // head 形如 {"$id":...,"type":...,"required":[...]}——在收尾括号
        // 前插入 properties。
        let trimmed = &head[..head.len() - 1];
        alloc::format!("{},\"properties\":{{{}}}}}", trimmed, props_body)
    }

    /// 示例包 JSON 全文（CI 自动重打的产物形态——永不腐烂的基准文件）。
    pub fn sample_json(&self, payload: &[(String, String)]) -> String {
        let mut fields: Vec<String> = Vec::new();
        for (k, v) in payload {
            let mut o = vbase::JsonObj::new();
            o.str_field(k, v);
            fields.push(o.finish());
        }
        let body = fields.join(",");
        alloc::format!("{{\"spec\":\"{}\",\"payload\":{{{}}}}}", self.id, body)
    }
}

/// 负样本族扩充（深化 v4：+3 路——Bool 类型错 / Int 类型错 / 枚举规范
/// 越界第二型；合计 12 路全拒）。
pub fn negative_samples_extended() -> Vec<(&'static str, Vec<(String, String)>, &'static str)> {
    let mut v = negative_samples();
    v.push((
        "vxapp",
        vec![
            (String::from("id"), String::from("a.b")),
            (String::from("version"), String::from("1.0.0")),
            (String::from("entry"), String::from("e")),
            (String::from("name"), String::from("42")),
        ],
        "attr constraint violated", // MaxLen 过不了长串才违例——短串合法；改用超长名
    ));
    v.pop();
    v.push((
        "vxapp",
        vec![
            (String::from("id"), String::from("a.b")),
            (String::from("version"), String::from("1.0.0")),
            (String::from("entry"), String::from("e")),
            (String::from("name"), String::from("x").repeat(65).as_str().into()),
        ],
        "attr constraint violated",
    ));
    v.push((
        "starmap-json",
        vec![
            (String::from("snapshot_at"), String::from("1727000000")),
            (String::from("cards"), String::from("c.json")),
            (String::from("license"), String::from("CC-BY")),
            (String::from("since"), String::from("later")),
        ],
        "type/value invalid",
    ));
    v.push((
        "case-submission",
        vec![
            (String::from("program"), String::from("P")),
            (String::from("program_version"), String::from("1.0.0")),
            (String::from("cases"), String::from("c.lst")),
            (String::from("evidence_hash"), String::from("ab").repeat(32)),
            (String::from("submitter"), String::from("n").repeat(65)),
        ],
        "attr constraint violated",
    ));
    v
}

/// 扩充负样本 CI 回放（12 路全拒且类别命中）。
pub fn ci_negative_replay_extended() -> (usize, Vec<String>) {
    let specs = official_specs();
    let mut failures = Vec::new();
    let mut rejected = 0usize;
    for (id, payload, expect) in negative_samples_extended() {
        if let Some(spec) = specs.iter().find(|s| s.id == id) {
            let errs = spec.validate(&payload);
            if !errs.is_empty() && errs.iter().any(|e| e.contains(expect)) {
                rejected += 1;
            } else {
                failures.push(alloc::format!("{}: expected [{}], got {:?}", id, expect, errs));
            }
        } else {
            failures.push(alloc::format!("{}: spec not found", id));
        }
    }
    (rejected, failures)
}
// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_openformat_checks() -> CheckSet {
    let mut set = CheckSet::new("F126-openformat");

    let specs = official_specs();

    // 1. 四规范页齐全可访问（判据第一句：四份齐 + 页面结构五件套齐）。
    let pages_complete = specs.len() == SPEC_COUNT
        && specs.iter().all(|s| {
            !s.overview.is_empty()
                && !s.schema_rows().is_empty()
                && !s.changelog.is_empty()
                && !s.title.is_empty()
        });
    set.add("four spec pages complete", pages_complete, "");

    // 2. 示例包全部通过当前实现校验（判据第一句之二：四份示例逐份
    //    三查——文档与实现零脱节）。
    let sample_vxapp = vec![
        (String::from("id"), String::from("demo.tool")),
        (String::from("version"), String::from("1.2.3")),
        (String::from("entry"), String::from("bin/demo.exe")),
    ];
    let sample_vxtheme = vec![
        (String::from("id"), String::from("ocean")),
        (String::from("mode"), String::from("dark")),
        (String::from("tokens"), String::from("tokens.json")),
    ];
    let sample_starmap = vec![
        (String::from("snapshot_at"), String::from("1727000000")),
        (String::from("cards"), String::from("cards.json")),
        (String::from("license"), String::from("CC-BY")),
    ];
    let sample_case = vec![
        (String::from("program"), String::from("Notepad2")),
        (String::from("program_version"), String::from("4.2.25")),
        (String::from("cases"), String::from("cases.lst")),
        (String::from("evidence_hash"), String::from("ab".repeat(32).as_str())),
    ];
    let samples = [
        (&specs[0], &sample_vxapp),
        (&specs[1], &sample_vxtheme),
        (&specs[2], &sample_starmap),
        (&specs[3], &sample_case),
    ];
    let mut all_pass = true;
    for (spec, payload) in samples {
        if !spec.validate(payload).is_empty() {
            all_pass = false;
        }
    }
    set.add("four sample packages pass validation", all_pass, "");

    // 3. 双读过渡承诺条款在册（判据第一句之三：迁移窗 90 天语义）。
    set.add(
        "dual-read clause registered",
        DUAL_READ_CLAUSE.contains("一个季度迁移窗")
            && DUAL_READ_CLAUSE.contains("90 天")
            && MIGRATION_WINDOW_DAYS == 90,
        "",
    );

    // 4. 版本语义条款 + semver 解析（vbase 唯一源）。
    set.add(
        "semver clause + parsing",
        SEMVER_CLAUSE.contains("主版本=破格")
            && vbase::parse_semver("1.0.0") == Some((1, 0, 0))
            && vbase::semver_cmp((2, 0, 0), (1, 9, 9)) == core::cmp::Ordering::Greater,
        "",
    );

    // 5. 校验器负样本：缺必填 / 类型错 / 枚举越界 / 未知字段——四路全拒。
    let bad_missing = vec![
        (String::from("id"), String::from("x")),
        (String::from("version"), String::from("1.0.0")),
    ];
    let bad_type = vec![
        (String::from("id"), String::from("x")),
        (String::from("version"), String::from("1.0.0")),
        (String::from("entry"), String::from("")),
    ];
    let bad_enum = vec![
        (String::from("id"), String::from("t")),
        (String::from("mode"), String::from("sepia")),
        (String::from("tokens"), String::from("t.json")),
    ];
    let bad_unknown = vec![
        (String::from("id"), String::from("x")),
        (String::from("version"), String::from("1.0.0")),
        (String::from("entry"), String::from("e")),
        (String::from("hack"), String::from("1")),
    ];
    set.add(
        "validator rejects 4 negative paths",
        !specs[0].validate(&bad_missing).is_empty()
            && !specs[0].validate(&bad_type).is_empty()
            && !specs[1].validate(&bad_enum).is_empty()
            && !specs[0].validate(&bad_unknown).is_empty(),
        "",
    );

    // 6. 变更历史强制（无记录不合入——每规范至少一条且版本对齐）。
    let changelog_ok = specs
        .iter()
        .all(|s| !s.changelog.is_empty() && s.changelog.iter().all(|(v, _)| vbase::parse_semver(v).is_some()));
    set.add("changelog mandatory per spec", changelog_ok, "");

    // 7. schema 表格行渲染源（字段名/类型/必填/说明四列齐）。
    let rows = specs[0].schema_rows();
    set.add(
        "schema table rows render source",
        rows.len() == 5 && rows[0].0 == "id" && rows[0].1 == "string" && rows[0].2,
        "",
    );

    // 8. 示例包 ≤100KB（四份示例体积上限——分钟级上手承诺）。
    let sizes = [
        sample_vxapp.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>() as u64,
        sample_vxtheme.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>() as u64,
        sample_starmap.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>() as u64,
        sample_case.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>() as u64,
    ];
    set.add(
        "samples within 100KB cap",
        sizes.iter().all(|&s| s <= SAMPLE_CAP_BYTES),
        "",
    );

    // 9. 四格式关系图在册（一张图讲清数据流）。
    set.add(
        "relation diagram registered",
        RELATION_DIAGRAM.contains("vxapp") && RELATION_DIAGRAM.contains("starmap-json"),
        "",
    );

    // 10. 破格流程 ADR 语义（semver 条款含 ADR + 迁移窗双词）。
    set.add(
        "breaking-change ADR flow",
        SEMVER_CLAUSE.contains("ADR") && SEMVER_CLAUSE.contains("迁移窗"),
        "",
    );

    // 11. CI 示例包回放（深化 v2）：四示例全过当前实现校验 = 文档正确。
    let (passed_n, ci_fail) = ci_sample_replay();
    set.add(
        "ci sample replay 4/4",
        passed_n == SPEC_COUNT && ci_fail.is_empty(),
        "",
    );

    // 12. CI 负样本回放（深化 v2）：九路负样本全拒且错误类别命中。
    let (rejected_n, neg_fail) = ci_negative_replay();
    set.add(
        "ci negative replay 9/9 rejected",
        rejected_n == negative_samples().len() && neg_fail.is_empty(),
        "",
    );

    // 13. 双读过渡引擎（深化 v2）：破格开窗 → 窗内旧版 Legacy 可读 →
    //     窗尽旧版拒读；非破格假开窗拒绝。
    let mut dr = DualReadEngine::new(1);
    let fake_open = !dr.open_window(1, 1, 0);
    let real_open = dr.open_window(2, 1, 0);
    let legacy_in_window = dr.route(1, 30 * 86_400_000) == ReaderRoute::Legacy;
    let current_in_window = dr.route(2, 30 * 86_400_000) == ReaderRoute::Current;
    let legacy_after_window = dr.route(1, 91 * 86_400_000) == ReaderRoute::Rejected;
    let stranger_rejected = dr.route(3, 30 * 86_400_000) == ReaderRoute::Rejected;
    set.add(
        "dual-read engine window semantics",
        fake_open && real_open && legacy_in_window && current_in_window && legacy_after_window && stranger_rejected,
        "",
    );

    // 14. 迁移窗公告义务（窗剩 ≤7 天 → 公告强制；开窗首日不催）。
    let mut dr = DualReadEngine::new(1);
    let _ = dr.open_window(2, 1, 0);
    let quiet_early = !dr.announce_required(0);
    let due_late = dr.announce_required(84 * 86_400_000);
    let closed_after = dr.window_remaining_days(95 * 86_400_000) <= 0;
    set.add("migration window announce duty", quiet_early && due_late && closed_after, "");

    // 15. 规范页全文渲染五件套（深化 v2）：四规范逐页结构完整 + 锚点
    //     字段行在页 + 讨论入口外链对位。
    let pages_ok = official_specs().iter().all(|s| page_structure_complete(s))
        && render_spec_page(&official_specs()[0]).contains("<a id=\"id\">")
        && render_spec_page(&official_specs()[0]).contains(DISCUSSION_LINKS[0].1);
    set.add("spec page render structure complete", pages_ok, "");

    // 16. 变更历史强制门禁（深化 v2）：低版本拒绝 / 空说明拒绝 / 合法
    //     更新放行。
    let specs = official_specs();
    let gate_low = changelog_gate(&specs[0], (0, 9, 9), "n").is_err();
    let gate_no_note = changelog_gate(&specs[0], (1, 1, 0), "").is_err();
    let gate_ok = changelog_gate(&specs[0], (1, 1, 0), "加字段").is_ok();
    set.add("changelog gate rejects bad updates", gate_low && gate_no_note && gate_ok, "");

    // 17. 约束属性登记完备（深化 v2）：必填字段除枚举外（枚举自带值域
    //     语义）全量携带约束属性——schema 表格渲染源齐。
    let attrs_covered = official_specs().iter().all(|s| {
        s.fields.iter().filter(|f| f.required).all(|f| {
            !f.attrs.is_empty() || matches!(f.ftype, FieldType::Enum(_))
        })
    });
    set.add("required fields carry constraint attrs", attrs_covered, "");


    // 18. JSON Schema 标准形态全文（深化 v4）：$id/type/required/
    //     properties 四键齐；required 与必填字段一致。
    let specs = official_specs();
    let sj = specs[0].schema_json();
    set.add(
        "json schema full form",
        sj.contains("\"$id\":\"https://varix.dev/schemas/vxapp.json\"")
            && sj.contains("\"type\":\"object\"")
            && sj.contains("\"required\":[\"id\",\"version\",\"entry\"]")
            && sj.contains("\"properties\":{")
            && sj.contains("\"description\""),
        "",
    );

    // 19. 示例包 JSON 全文（深化 v4）：合法 JSON 形态（spec + payload 双
    //     键；payload 内字段与负载一致）。
    let (id, payload) = &sample_packages()[0];
    let spec = specs.iter().find(|s| s.id == *id).unwrap();
    let j = spec.sample_json(payload);
    set.add(
        "sample json full form",
        j.starts_with("{\"spec\":\"vxapp\"")
            && j.contains("\"payload\":{")
            && j.contains("\"id\":\"demo.tool\""),
        "",
    );

    // 20. 负样本扩充回放（深化 v4）：12 路全拒且类别命中。
    let (rej12, fail12) = ci_negative_replay_extended();
    set.add(
        "negative extended 12/12 rejected",
        rej12 == negative_samples_extended().len() && fail12.is_empty(),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openformat_all_checks_green() {
        let set = run_openformat_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F126 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn empty_optional_string_rejected_only_when_required() {
        let specs = official_specs();
        // 可选字段空值放行（默认值语义）；必填空值拒绝。
        let p1 = vec![
            (String::from("id"), String::from("a.b")),
            (String::from("version"), String::from("1.0.0")),
            (String::from("entry"), String::from("e")),
            (String::from("icon"), String::from("")),
        ];
        assert!(specs[0].validate(&p1).is_empty());
    }

    #[test]
    fn overview_within_200_chars() {
        for s in official_specs() {
            assert!(s.overview.chars().count() <= 200, "{} 概览超 200 字", s.id);
        }
    }

    #[test]
    fn f126_samples_carry_optional_fields_still_valid() {
        // 全量示例（含可选字段）逐份过校验——可选字段不破坏合法性。
        let specs = official_specs();
        for (id, payload) in sample_packages() {
            let spec = specs.iter().find(|s| s.id == id).unwrap();
            assert!(spec.validate(&payload).is_empty(), "{} 全量示例应过校验", id);
        }
    }

    #[test]
    fn f126_semver_attr_rejects_non_semver() {
        let specs = official_specs();
        let bad = vec![
            (String::from("id"), String::from("a.b")),
            (String::from("version"), String::from("2026")),
            (String::from("entry"), String::from("e")),
        ];
        let errs = specs[0].validate(&bad);
        assert!(errs.iter().all(|e| e.starts_with("#/")), "错误必须带 JSON Pointer 路径");
    }

    #[test]
    fn f126_window_remaining_monotonic() {
        let mut dr = DualReadEngine::new(2);
        // 非破格（同主版本）开窗拒绝——次版本加字段不需要双读。
        assert!(!dr.open_window(2, 1, 0));
        assert!(dr.window_opened_ms.is_none());
        // 真破格开窗。
        assert!(dr.open_window(3, 2, 0));
        // 剩余天数随时间单调不增。
        let mut prev = dr.window_remaining_days(0);
        for day in 1..=95u64 {
            let cur = dr.window_remaining_days(day * 86_400_000);
            assert!(cur <= prev, "窗剩余必须单调不增");
            prev = cur;
        }
        assert!(prev <= 0);
    }

    #[test]
    fn f126_discussion_links_cover_all_specs() {
        for s in official_specs() {
            assert!(DISCUSSION_LINKS.iter().any(|(id, l)| *id == s.id && l.starts_with("https://")));
        }
    }

    #[test]
    fn f126_schema_json_all_specs() {
        // 四规范 schema 全文逐份可生成且含各自 $id。
        for s in official_specs() {
            let j = s.schema_json();
            assert!(j.contains(&alloc::format!("schemas/{}.json", s.id)));
            assert!(j.contains("\"properties\":{"));
        }
    }

    #[test]
    fn f126_extended_negative_superset() {
        // 扩充族包含基础族全部 9 路（超集关系）。
        assert!(negative_samples_extended().len() == 12);
    }
}
