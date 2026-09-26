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
                        FieldType::Enum(vs) => "enum",
                    },
                    f.required,
                    f.desc,
                )
            })
            .collect()
    }

    /// 实现校验器：对（字段名→字符串值）负载逐字段三查——类型/必填/
    /// 值域。返回错误清单（空 = 通过）。
    pub fn validate(&self, payload: &[(String, String)]) -> Vec<String> {
        let mut errors = Vec::new();
        for f in &self.fields {
            let hit = payload.iter().find(|(k, _)| k == f.name);
            match hit {
                None => {
                    if f.required {
                        errors.push(alloc::format!("missing required field: {}", f.name));
                    }
                }
                Some((k, v)) => {
                    let ok = match f.ftype {
                        FieldType::Str => !v.is_empty() || !f.required,
                        FieldType::Int => v.parse::<u64>().is_ok() || v.parse::<i64>().is_ok(),
                        FieldType::Bool => v == "true" || v == "false",
                        FieldType::Enum(vs) => vs.contains(&v.as_str()),
                    };
                    if !ok {
                        errors.push(alloc::format!("field {} type/value invalid", k));
                    }
                }
            }
        }
        // 未知字段拒绝（schema 严格性——脱节即 CI 红）。
        for (k, _) in payload {
            if !self.fields.iter().any(|f| f.name == k.as_str()) {
                errors.push(alloc::format!("unknown field: {}", k));
            }
        }
        errors
    }
}

// ---------------------------------------------------------------------------
// 四份官方规范（主册定名；字段为本实现当前 schema——CI 对拍基准）
// ---------------------------------------------------------------------------

/// 官方四规范（schema 与实现同源——示例包过本校验即文档正确）。
pub fn official_specs() -> Vec<Spec> {
    vec![
        Spec {
            id: "vxapp",
            title: "vxapp 包格式",
            version: (1, 0, 0),
            overview: "应用包格式：清单 + 内容树 + 签名。清单含身份/版本/主程序三必填，其余字段默认值全文档化。",
            fields: vec![
                Field { name: "id", ftype: FieldType::Str, required: true, desc: "包身份（反域名小写）" },
                Field { name: "version", ftype: FieldType::Str, required: true, desc: "semver 三元组" },
                Field { name: "entry", ftype: FieldType::Str, required: true, desc: "主程序相对路径" },
                Field { name: "name", ftype: FieldType::Str, required: false, desc: "显示名（默认取 id）" },
                Field { name: "icon", ftype: FieldType::Str, required: false, desc: "图标相对路径（默认无）" },
            ],
            changelog: vec![("1.0.0", "首版冻结：三必填 + 两可选")],
        },
        Spec {
            id: "vxtheme",
            title: "vxtheme 定制包",
            version: (1, 0, 0),
            overview: "主题定制包：令牌表 + 壁纸资产 + 指针/声音可选集。令牌名集与 E1 主题令牌全集对齐。",
            fields: vec![
                Field { name: "id", ftype: FieldType::Str, required: true, desc: "主题身份" },
                Field {
                    name: "mode",
                    ftype: FieldType::Enum(&["dark", "light", "auto"]),
                    required: true,
                    desc: "深浅模式",
                },
                Field { name: "tokens", ftype: FieldType::Str, required: true, desc: "令牌表路径" },
                Field { name: "wallpaper", ftype: FieldType::Str, required: false, desc: "壁纸路径（默认系统）" },
            ],
            changelog: vec![("1.0.0", "首版冻结：三必填 + 一可选")],
        },
        Spec {
            id: "starmap-json",
            title: "星图 JSON 目录",
            version: (1, 0, 0),
            overview: "兼容性评级目录开放数据面：星卡数组 + 账本引用 + 快照时间戳。CC-BY 许可（标注来源可自由再分发）。",
            fields: vec![
                Field { name: "snapshot_at", ftype: FieldType::Int, required: true, desc: "快照 Unix 时间戳" },
                Field { name: "cards", ftype: FieldType::Str, required: true, desc: "星卡数组路径（内嵌或外链）" },
                Field {
                    name: "license",
                    ftype: FieldType::Enum(&["CC-BY"]),
                    required: true,
                    desc: "数据许可（唯一值）",
                },
                Field { name: "since", ftype: FieldType::Int, required: false, desc: "增量拉取起点（缺省全量）" },
            ],
            changelog: vec![("1.0.0", "首版冻结：三必填 + 增量可选")],
        },
        Spec {
            id: "case-submission",
            title: "判例提交格式",
            version: (1, 0, 0),
            overview: "社区判例提交线的数据格式：程序名/版本/判例集/证据哈希链。五态状态机可查（F129 同引擎）。",
            fields: vec![
                Field { name: "program", ftype: FieldType::Str, required: true, desc: "程序名" },
                Field { name: "program_version", ftype: FieldType::Str, required: true, desc: "程序版本" },
                Field { name: "cases", ftype: FieldType::Str, required: true, desc: "判例集清单路径" },
                Field { name: "evidence_hash", ftype: FieldType::Str, required: true, desc: "证据包哈希链头" },
                Field { name: "submitter", ftype: FieldType::Str, required: false, desc: "提交者署名（默认匿名）" },
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
            (String::from("id"), String::from("x")),
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
}
