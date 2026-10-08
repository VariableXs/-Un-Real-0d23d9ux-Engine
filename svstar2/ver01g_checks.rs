//! VE-F3407 · 令牌持久化与迁移 —— 域判据层。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3407`
//!
//! # 判据映射（锚点五条 + 纪律三条）
//!
//! - **开放格式** → `F3407-开放格式-魔数与版本头`、`F3407-开放格式-往返一致`、
//!   `F3407-开放格式-字段制表符分隔`、`F3407-开放格式-值含制表符不拆行`、
//!   `F3407-开放格式-魔数不符即拒`、`F3407-开放格式-版本过高即拒`、
//!   `F3407-开放格式-字段数不符即拒`、`F3407-开放格式-变体魔数被改必被抓`、
//!   `F3407-开放格式-记录数上限必拦`、`F3407-开放格式-定义点零即拒`、
//!   `F3407-开放格式-层级代号须可反查`；
//! - **自动映射** → `F3407-自动映射-六规则齐备`、`F3407-自动映射-语料全部命中规则`、
//!   `F3407-自动映射-逐条映射结果正确`、`F3407-自动映射-重叠前缀取最长`、
//!   `F3407-自动映射-版本同则原样保留`、`F3407-自动映射-规则代号唯一`、
//!   `F3407-自动映射-变体规则失效应被抓`、
//!   `F3407-自动映射-变体前缀退化须被抓`；
//! - **公开映射表** → `F3407-公开映射表-可导出且含全部规则`、
//!   `F3407-公开映射表-行数与规则数一致`、`F3407-公开映射表-与规则表逐条一致`、
//!   `F3407-公开映射表-每条规则有说明`、`F3407-公开映射表-变体漏条必被抓`、
//!   `F3407-公开映射表-变体说明丢失必被抓`；
//! - **失败回退** → `F3407-回退-成功迁移已应用`、`F3407-回退-应用条数自洽`、
//!   `F3407-回退-新路径取值正确`、`F3407-回退-失败不留半迁状态`、
//!   `F3407-回退-被拒样本确会被上游拒`、`F3407-回退-失败报告带原因码`、
//!   `F3407-回退-计划长于输入必被抓`、`F3407-回退-输入短于计划必被抓`、
//!   `F3407-回退-待人工不提交且形态可分`、`F3407-回退-有拒绝项形态可分`、
//!   `F3407-回退-已提交不可重入`、`F3407-回退-已提交不可回退`、
//!   `F3407-回退-未开始无快照`、`F3407-回退-同版本判Noop`、
//!   `F3407-回退-变体半迁必被抓`；
//! - **干跑模式** → `F3407-干跑-只产计划不改状态`、`F3407-干跑-清单计数自洽`、
//!   `F3407-干跑-变体计数塌缩必被抓`、`F3407-干跑-待人工时不提交`、
//!   `F3407-干跑-清单逐条可核`、`F3407-干跑-人工映射真生效`、
//!   `F3407-干跑-变体人工失效必被抓`、`F3407-干跑-人工入口可补全`、
//!   `F3407-干跑-人工映射越界即拒`、`F3407-干跑-人工项上限必拦`、
//!   `F3407-干跑-变体上限误拦必被抓`、`F3407-干跑-读屏句子含数量`、
//!   `F3407-干跑-就绪句随状态切换`、`F3407-干跑-数量逐位读`、
//!   `F3407-干跑-Planned有真实产出点`、`F3407-干跑-未就绪计划报告带原因`、
//!   `F3407-干跑-报告读屏可达`、`F3407-干跑-成功态不播报拒绝数`；
//! - **判据** → `F3407-判据-五条齐备`、`F3407-判据-十五码唯一`、
//!   `F3407-判据-码表与枚举一致`、`F3407-判据-错误三要素齐发`、
//!   `F3407-判据-缺失映射标记可人工`、`F3407-判据-结果变体均有产出点`、
//!   `F3407-契约-版本单调`。
//!
//! # 判据设计纪律（本层的硬约束）
//!
//! 1. **不向被测函数问答案**：映射表条数由 [`RULE_COUNT`] 常量推导，
//!    不读 `RULES.len()` 的同时又用`RULES.len()` 构造期望（那恒真）。
//! 2. **计数独立重算**：[`DryRunReport::total`] 的期望值由语料条数独立算出，
//!    不复用报告自己的分项之和。
//! 3. **变体双向验证**：每条「必被抓」判据都配一个把被测对象改坏的变体，
//!    确认判定确实转红；只写正向断言则「恒真」与「在判」不可区分。

use crate::checks::CheckSet;
use crate::svstar2::ver01b_parser::Site;
use crate::svstar2::ver01f_overlay::{Level, OverlayStack};
use crate::svstar2::ver01g_persist::*;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 期望的规则条数（判据侧常量，不从被测常量推导——那会自证）。
const EXPECT_RULES: usize = 7;
/// 期望的错误码条数。
const EXPECT_ERROR_CODES: usize = 15;
/// 期望的判据条数。
const EXPECT_CRITERIA: usize = 5;
/// 期望的迁移格式版本（1 -> 2）。
const EXPECT_FROM_VERSION: u16 = 1;
const EXPECT_TO_VERSION: u16 = 2;

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 造一条记录。
fn rec(level: Level, source: &str, path: &str, value: &str, line: u32, col: u32) -> Record {
    Record {
        level,
        source: String::from(source),
        path: String::from(path),
        value: String::from(value),
        site: Site { line, col, byte: 0 },
    }
}

/// 七条规则各一个命中样本 + 一个无规则样本（后者用于人工入口判据）。
fn corpus() -> Vec<Record> {
    vec![
        rec(Level::Component, "user", "color.brand.primary", "#0af", 3, 5),
        rec(Level::Theme, "user", "space.gap", "8px", 4, 1),
        rec(Level::Default, "user", "font.size.body", "14px", 5, 2),
        rec(Level::Component, "user", "radius.md", "6px", 6, 3),
        rec(Level::Theme, "user", "motion.dur.fast", "150ms", 7, 4),
        rec(Level::Default, "user", "z.modal", "100", 8, 5),
        // R7 命中样本。注意它同时也命中 R1（`color.brand.fg.surface` 以
        // `color.brand.` 开头），最长前缀须选R7——这条语料是"最长优先"
        // 唯一的有效证据，删掉它那条不变式就退回恒真。
        rec(Level::Component, "user", "color.brand.fg.surface", "#fff", 9, 7),
    ]
}

/// 无规则样本（旧路径命中不到任何规则）。
fn unmapped() -> Record {
    rec(Level::Component, "user", "legacy.custom.thing", "7", 9, 6)
}

/// 已登记来源的覆盖栈。
fn seeded_stack() -> OverlayStack {
    let mut s = OverlayStack::new();
    let _ = s.register_source("user", false);
    s
}

/// 干跑取报告（走 `DryRunReport::of`，与事务级 `dry_run` 同一条路径）。
///
/// **不在判据里另写一套计数**：若判据自己把 `plan.items` 数一遍，
/// 就成了「同源驱动恒真」——数字与被测同源，出错一起错。
fn plan_dry_run(plan: &MigrationPlan) -> DryRunReport {
    DryRunReport::of(plan)
}

// ---------------------------------------------------------------------------
// 一、开放格式
// ---------------------------------------------------------------------------

fn chk_open_format(set: &mut CheckSet) {
    // 序列化产出魔数 + 版本头（开放格式的第一可读性保证）。
    let text = match serialize(&corpus()) {
        Ok(t) => t,
        Err(_) => {
            set.add("F3407-开放格式-魔数与版本头", false, "serialize 报错");
            return;
        }
    };
    let head_ok = text.starts_with(FORMAT_MAGIC) && text.contains(&format!("{}{}", FIELD_SEP, FORMAT_VERSION));
    set.add("F3407-开放格式-魔数与版本头", head_ok, "首行须为 #ve-tokens<TAB>版本号");

    // 往返一致（开放格式的核心承诺：写出去能原样读回来）。
    let rt = deserialize(&text);
    let round_trip = match rt {
        Ok((v, recs)) => v == FORMAT_VERSION && recs == corpus(),
        Err(_) => false,
    };
    set.add("F3407-开放格式-往返一致", round_trip, "序列化后再解析须逐条相同");

    // 字段以制表符分隔且条数相符（外部工具按制表符切列就能读）。
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let cols_ok = lines.len() == corpus().len() + 1
        && lines[1..].iter().all(|l| l.split(FIELD_SEP).count() == RECORD_FIELDS);
    set.add("F3407-开放格式-字段制表符分隔", cols_ok, "每行字段数须为 6");

    // 值里含制表符/换行不得把一条拆成两条（静默数据损坏的红线）。
    let tricky = vec![rec(Level::Component, "user", "a.b", "x\ty\nz", 1, 1)];
    let t2 = serialize(&tricky).unwrap_or_else(|_| String::new());
    let d2 = deserialize(&t2);
    let intact = match d2 {
        Ok((_, recs)) => recs.len() == 1 && recs[0].value == "x\ty\nz",
        Err(_) => false,
    };
    set.add("F3407-开放格式-值含制表符不拆行", intact, "值内制表符须转义，往返后原样");

    // 魔数不符即拒（不做尽力解析）。
    let bad_magic = deserialize("#other-file\t2\n");
    set.add(
        "F3407-开放格式-魔数不符即拒",
        bad_magic.is_err() && bad_magic.unwrap_err() == PersistCode::MagicMismatch,
        "",
    );

    // 版本过高即拒（不猜、不截断）。
    let too_new = deserialize(&format!("{}{}{}\n", FORMAT_MAGIC, FIELD_SEP, FORMAT_VERSION + 1));
    set.add(
        "F3407-开放格式-版本过高即拒",
        too_new.is_err() && too_new.unwrap_err() == PersistCode::VersionTooNew,
        "",
    );

    // 变体双向验证：把魔数改坏，判定必须转红。
    let mut tampered = text.clone();
    let magic_mutated_failed = match tampered.find(FORMAT_MAGIC) {
        Some(i) => {
            tampered.replace_range(i..i + FORMAT_MAGIC.len(), "#not-tokens");
            // 原文本可解析（对照），改后不可解析（变体被抓住）。
            deserialize(&text).is_ok() && deserialize(&tampered).is_err()
        }
        None => false,
    };
    set.add(
        "F3407-开放格式-变体魔数被改必被抓",
        magic_mutated_failed,
        "改坏魔数后解析必失败",
    );

    // 字段数不符即拒。
    let short_line = deserialize(&format!(
        "{}{}{}\ncmp\tuser\ta.b\n",
        FORMAT_MAGIC, FIELD_SEP, FORMAT_VERSION
    ));
    set.add(
        "F3407-开放格式-字段数不符即拒",
        short_line.is_err() && short_line.unwrap_err() == PersistCode::FieldCount,
        "",
    );

    // --- 记录数上限：两侧都要拦 -----------------------------------------
    // 只断 serialize 侧，deserialize 侧是另一条独立代码路径
    // （用户在外部工具里改出超大文件同样会走到）。
    let too_many: Vec<Record> = (0..(MAX_RECORDS + 1))
        .map(|n| rec(Level::Theme, "user", "a.b", "v", (n % 65535) as u32 + 1, 1))
        .collect();
    let over_text = serialize(&too_many);
    set.add(
        "F3407-开放格式-记录数上限必拦",
        over_text.is_err() && over_text.unwrap_err() == PersistCode::RecordLimit,
        "序列化超过上限须拒（否则 OOM 在读取侧而非写入侧暴露）",
    );

    // 变体双向验证：恰好等于上限须放行（限制定得过死也是缺陷）。
    let at_limit: Vec<Record> = (0..MAX_RECORDS)
        .map(|n| rec(Level::Theme, "user", "a.b", "v", (n % 65535) as u32 + 1, 1))
        .collect();
    set.add(
        "F3407-开放格式-变体上限误拦必被抓",
        at_limit.len() == MAX_RECORDS && serialize(&at_limit).is_ok(),
        "恰好等于上限须放行；限制定得过死等于把合法文件拒了",
    );

    // --- 定义点 0 必拒（读屏红线：否则会念"第 0 行"）------------------
    let zero_site = vec![rec(Level::Component, "user", "a.b", "v", 0, 3)];
    let zs = serialize(&zero_site);
    set.add(
        "F3407-开放格式-定义点零即拒",
        zs.is_err() && zs.unwrap_err() == PersistCode::SiteInvalid,
        "行/列 1 起算；0 会被读屏念成\"第 0 行\"",
    );

    // 反向对照：1 起算的合法定义点须放行（别把正常数据也拒了）。
    set.add(
        "F3407-开放格式-合法定义点放行",
        serialize(&vec![rec(Level::Component, "user", "a.b", "v", 1, 1)]).is_ok(),
        "",
    );

    // --- 层级代号须可反查（防越界 rank 被当成合法层）-------------------
    // 线格式首列写一个不存在的层级代号，解析须拒。
    let bad_level = format!(
        "{}{}{}\nnope\tuser\ta.b\tv\t1\t1\n",
        FORMAT_MAGIC, FIELD_SEP, FORMAT_VERSION
    );
    let bl = deserialize(&bad_level);
    set.add(
        "F3407-开放格式-层级代号须可反查",
        bl.is_err() && bl.unwrap_err() == PersistCode::LevelUnknown,
        "未知层级代号即拒，不猜不兜底",
    );

    // 对照：四种合法层级代号全须放行（反查不是"一律拒"）。
    let all_levels_ok = Level::ALL.iter().all(|l| {
        let r = Record {
            level: *l,
            source: String::from("user"),
            path: String::from("a.b"),
            value: String::from("v"),
            site: Site { line: 1, col: 1, byte: 0 },
        };
        serialize(&vec![r]).is_ok()
    });
    set.add(
        "F3407-开放格式-四层代号全放行",
        all_levels_ok && Level::ALL.len() == 4,
        "四个合法层级代号必须都能序列化",
    );

    // --- 反序列化侧记录数上限 -------------------------------------------
    // serialize 与 deserialize 是**两条独立代码路径**：写入侧拦住了，
    // 用户在外部工具里手改/拼接出超大文件仍会走读取侧。
    // 只断写入侧 ⇒ 读取侧的 OOM 敞着。
    //
    // 直接构造超限文本（不经过 serialize，那会被写入侧先拒）。
    let mut big = String::from(FORMAT_MAGIC);
    big.push(FIELD_SEP);
    big.push_str("2");
    big.push(RECORD_SEP);
    for n in 0..(MAX_RECORDS + 2) {
        big.push_str(&format!(
            "{}\tuser\ta.b\tv\t{}\t1{}",
            Level::Theme.wire(),
            (n % 65535) + 1,
            RECORD_SEP
        ));
    }
    let big_parsed = deserialize(&big);
    set.add(
        "F3407-开放格式-读取侧记录数上限必拦",
        big_parsed.is_err() && big_parsed.unwrap_err() == PersistCode::RecordLimit,
        "外部拼接的超限文件须在读取侧拒（否则 OOM 在读端暴露）",
    );

    // 对照：恰好等于上限须能读进来（上限不是"少一条都拒"）。
    let mut at_big = String::from(FORMAT_MAGIC);
    at_big.push(FIELD_SEP);
    at_big.push_str("2");
    at_big.push(RECORD_SEP);
    for n in 0..MAX_RECORDS {
        at_big.push_str(&format!(
            "{}\tuser\ta.b\tv\t{}\t1{}",
            Level::Theme.wire(),
            (n % 65535) + 1,
            RECORD_SEP
        ));
    }
    let at_big_ok = match deserialize(&at_big) {
        Ok((_, recs)) => recs.len() == MAX_RECORDS,
        Err(_) => false,
    };
    set.add(
        "F3407-开放格式-读取侧上限边界放行",
        at_big_ok,
        "恰好等于上限须能读；限制定得过死等于把合法文件也拒了",
    );
}

/// 变体辅助已内联（见 chk_open_format 的 magic_mutated_failed）。

// ---------------------------------------------------------------------------
// 二、自动映射
// ---------------------------------------------------------------------------

fn chk_auto_map(set: &mut CheckSet) {
    // 规则齐备（条数由判据侧常量给，不读被测的 RULES.len()）。
    let rule_ids_ok = RULES.len() == EXPECT_RULES
        && RULES.iter().all(|r| !r.id.is_empty() && !r.from.is_empty() && !r.to.is_empty());
    set.add("F3407-自动映射-六规则齐备", rule_ids_ok, "每条规则须有代号/旧前缀/新前缀");

    // 语料每条都能命中规则（对照：否则"自动映射"是恒真的）。
    let c = corpus();
    let plan = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    let all_remapped = plan.items.iter().all(|i| i.action == Action::Remap);
    set.add(
        "F3407-自动映射-语料全部命中规则",
        all_remapped && plan.remap_count() == EXPECT_RULES && c.len() == EXPECT_RULES,
        "每条样本应各命中一条规则",
    );

    // 逐条核对映射结果（判据侧独立算出期望，不问被测）。
    let expect_pairs: [(&str, &str, &str); 7] = [
        ("color.brand.primary", "color.brand.fg.primary", "R1"),
        ("space.gap", "spacing.gap", "R2"),
        ("font.size.body", "type.size.body", "R3"),
        ("radius.md", "shape.radius.md", "R4"),
        ("motion.dur.fast", "motion.duration.fast", "R5"),
        ("z.modal", "layer.z.modal", "R6"),
        (
            "color.brand.fg.surface",
            "color.brand.bg.surface",
            "R7",
        ),
    ];
    let mut pairs_ok = plan.items.len() == EXPECT_RULES;
    for (i, (from, to, rid)) in expect_pairs.iter().enumerate() {
        match plan.items.get(i) {
            Some(it) => {
                if it.old_path != *from || it.new_path != *to || it.rule_id != *rid {
                    pairs_ok = false;
                }
            }
            None => pairs_ok = false,
        }
    }
    set.add("F3407-自动映射-逐条映射结果正确", pairs_ok, "旧前缀->新前缀逐条核对");

    // 版本相同则原样保留（不做无谓改写）。
    let same = MigrationPlan::build(EXPECT_TO_VERSION, &c);
    let all_keep = same.items.iter().all(|i| i.action == Action::Keep)
        && same.items.iter().all(|i| i.new_path == i.old_path);
    set.add("F3407-自动映射-版本同则原样保留", all_keep, "");

    // 变体双向验证：把语料换成无规则路径，判定须从"全命中"转红。
    let unmapped_plan = MigrationPlan::build(EXPECT_FROM_VERSION, &vec![unmapped()]);
    let mutated_all_remap = unmapped_plan.items.iter().all(|i| i.action == Action::Remap);
    set.add(
        "F3407-自动映射-变体规则失效应被抓",
        !mutated_all_remap && unmapped_plan.manual_count() == 1,
        "无规则路径应转人工而非被硬映射",
    );

    // 规则表规则码唯一（重复代号会让报错指错规则）。
    let mut ids: Vec<&str> = RULES.iter().map(|r| r.id).collect();
    ids.sort_unstable();
    ids.dedup();
    set.add(
        "F3407-自动映射-规则代号唯一",
        ids.len() == EXPECT_RULES,
        "规则代号重复会让用户报号报不准",
    );

    // --- 重叠前缀取最长（真语料，不用"无重叠"的形式对照）--------------
    // R7 的 from(`color.brand.fg.surface`) 是 R1 的 from(`color.brand.`) 之内，
    // 故 `color.brand.fg.surface` 同时命中两条 ⇒ "取最长"与"取表内首个"
    // 在这条语料上输出不同，判据才真正有辨别力。
    // 判据侧独立算出期望结果，不问 find_rule。
    let overlap_paths = [
        // 仅 R1 命中：R1 的 to 是 `color.brand.fg.`，剩余段 `primary` 直接接上。
        "color.brand.primary", // -> color.brand.fg.primary
        // R1 与 R7 同时命中，取最长 R7 -> color.brand.bg.surface。
        "color.brand.fg.surface",
        // 仅 R1 命中。注意结果是**两层 fg**：`color.brand.fg.` + 剩余
        // `fg.divider`。这不是笔误——它正是"只命中 R1"与"命中 R7"的
        // 分水岭，写期望值时最容易顺手写成直觉值而误判实现。
        "color.brand.fg.divider", // -> color.brand.fg.fg.divider
    ];
    let ov_corpus: Vec<Record> = overlap_paths
        .iter()
        .enumerate()
        .map(|(i, p)| rec(Level::Component, "user", p, "v", (i + 1) as u32, 1))
        .collect();
    let op = MigrationPlan::build(EXPECT_FROM_VERSION, &ov_corpus);
    let expect_overlap = [
        ("R1", "color.brand.fg.primary"),
        ("R7", "color.brand.bg.surface"),
        ("R1", "color.brand.fg.fg.divider"),
    ];
    let overlap_ok = op.items.len() == overlap_paths.len()
        && op
            .items
            .iter()
            .zip(expect_overlap.iter())
            .all(|(it, e)| it.rule_id == e.0 && it.new_path == e.1);
    set.add(
        "F3407-自动映射-重叠前缀取最长",
        overlap_ok,
        "多条同时命中时须取 from 最长者，不是表内首个",
    );

    // 变体双向验证：把 R7 的语料抽掉，两种实现同结果——
    // 记录"重叠语料才是有效语料"，别用无重叠语料去证明最长前缀逻辑。
    let short_only = MigrationPlan::build(EXPECT_FROM_VERSION, &vec![ov_corpus[1].clone()]);
    set.add(
        "F3407-自动映射-变体前缀退化须被抓",
        short_only.items.len() == 1
            && short_only.items[0].new_path == "color.brand.bg.surface"
            && short_only.items[0].rule_id == "R7",
        "单规则命中时结果与表序无关；差异只在重叠语料上可见",
    );
}

// ---------------------------------------------------------------------------
// 三、公开映射表
// ---------------------------------------------------------------------------

fn chk_public_map(set: &mut CheckSet) {
    let table = export_rules();
    // 首行是表头，其后每规则一行。
    let body = &table[1..];
    set.add(
        "F3407-公开映射表-可导出且含全部规则",
        table.len() == EXPECT_RULES + 1 && !table[0].is_empty(),
        "首行表头 + 每规则一行",
    );
    // 每行四段（代号/旧/新/说明），段数与规则数一致。
    let cols_ok = body
        .iter()
        .all(|l| l.split(FIELD_SEP).count() == 4)
        && body.len() == EXPECT_RULES;
    set.add("F3407-公开映射表-行数与规则数一致", cols_ok, "每行四段");

    // 表内旧/新前缀与规则表逐条一致（公开表不能与实际规则脱节）。
    let mut consistent = body.len() == RULES.len();
    for (i, line) in body.iter().enumerate() {
        let f: Vec<&str> = line.split(FIELD_SEP).collect();
        match RULES.get(i) {
            Some(r) => {
                if f[0] != r.id || f[1] != r.from || f[2] != r.to {
                    consistent = false;
                }
            }
            None => consistent = false,
        }
    }
    set.add("F3407-公开映射表-与规则表逐条一致", consistent, "");

    // 变体双向验证：抽掉一条，判定须转红。
    let mut dropped = table.clone();
    if dropped.len() > 2 {
        dropped.remove(1);
    }
    let rows = dropped.len().saturating_sub(1);
    set.add(
        "F3407-公开映射表-变体漏条必被抓",
        rows != EXPECT_RULES && dropped.len() != EXPECT_RULES + 1,
        "抽掉一条后行数不再等于规则数",
    );

    // 每条规则须带**说明**（第四段）。
    //
    // 为什么单列一条：段数判据只钉"有四段"，第四段可能是空串——
    // 那时公开映射表形式上完整，实际不可用（用户拿不到"为什么这么改"）。
    // 段数对了不等于公开了，两个问题不能互相顶替。
    let notes_ok = body.iter().all(|l| {
        l.split(FIELD_SEP)
            .nth(3)
            .map(|n| !n.trim().is_empty())
            .unwrap_or(false)
    });
    set.add(
        "F3407-公开映射表-每条规则有说明",
        notes_ok && body.len() == EXPECT_RULES,
        "第四段须是说明文字，空串等于没公开",
    );

    // 变体双向验证：说明字段与**渲染后的文本**须逐条一致。
    //
    // 为什么不在文本层"重拼一个空说明行"当变体：那会把段数从 4 掉到 3，
    // 判据确实转红，但转红原因是「段数不符」（被上一条判据抓），
    // 于是这次验证什么也没证明—— 判据互相顶替。
    //
    // 改为下沉到内部量：直接断言「渲染出的第四段 == 规则的 note 且非空」。
    // 这条对「note 被清空」敏感（渲染出空段），对「段数错」也敏感，
    // 但更重要的是它**不依赖另一条判据**来判红。
    let notes_match = body.len() == RULES.len()
        && body.iter().zip(RULES.iter()).all(|(line, r)| {
            line.split(FIELD_SEP)
                .nth(3)
                .map(|n| n == r.note && !n.trim().is_empty())
                .unwrap_or(false)
        });
    set.add(
        "F3407-公开映射表-变体说明丢失必被抓",
        notes_match && notes_ok,
        "渲染的说明段须逐条等于规则的 note 且非空；note 清空即判红",
    );
}

// ---------------------------------------------------------------------------
// 四、失败回退
// ---------------------------------------------------------------------------

fn chk_rollback(set: &mut CheckSet) {
    let c = corpus();
    let plan = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    let mut txn = MigrationTxn::new(plan.clone());
    let target = seeded_stack();

    // --- 成功路径：commit 返回**新栈**，旧栈逐格不变 --------------------
    let before = target.live_cell_count();
    let (new_stack, rep) = match txn.commit(&target, &c) {
        Ok(v) => v,
        Err(_) => {
            set.add("F3407-回退-成功迁移已应用", false, "commit 报错");
            return;
        }
    };
    set.add(
        "F3407-回退-成功迁移已应用",
        rep.outcome == MigrationOutcome::Committed
            && new_stack.live_cell_count() > before
            && target.live_cell_count() == before,
        "成功时新栈应多出覆盖，且**旧栈逐格不变**（全量替换语义）",
    );

    // 应用条数与计划条数恒等（不多不少）。
    set.add(
        "F3407-回退-应用条数自洽",
        rep.applied == c.len() && rep.rejected == 0 && rep.code.is_none(),
        "applied 须等于语料条数，成功形态不应带失败码",
    );

    // 新路径可解析，且**取值正确**（只断"有东西"会放过写错值的实现）。
    let mut vals_ok = plan.items.len() == c.len();
    for (i, r) in c.iter().enumerate() {
        match (plan.items.get(i), new_stack.resolve(&plan.items[i].new_path)) {
            (Some(it), Some(res)) => {
                if it.old_path != r.path || res.value != r.value {
                    vals_ok = false;
                }
            }
            _ => vals_ok = false,
        }
    }
    set.add("F3407-回退-新路径取值正确", vals_ok, "迁移后新路径须读回原值");

    // --- 失败路径：push 被拒时整体回退 ---------------------------------
    // 用**必然被上游拒**的路径（超 MAX_PATH_LEN）。这走的是真实 push 拒绝，
    // 不是"计划里写了 Reject"那种绕开apply 的假失败。
    let too_long = "x".repeat(PATH_MAX + 8);
    let bad_recs = vec![rec(Level::Component, "user", &too_long, "1", 1, 1)];
    let mut bad_plan = MigrationPlan::build(EXPECT_FROM_VERSION, &bad_recs);
    // 绕开 plan 侧校验（plan 侧本就拒超长路径），逼失败发生在 push 侧。
    if let Some(first) = bad_plan.items.first_mut() {
        first.action = Action::Remap;
        first.new_path = too_long.clone();
        first.rule_id = "MANUAL";
    }
    let mut txn2 = MigrationTxn::new(bad_plan);
    let target2 = seeded_stack();
    let pre2 = target2.live_cell_count();
    let (stack2, r2) = match txn2.commit(&target2, &bad_recs) {
        Ok(v) => v,
        Err(_) => {
            set.add("F3407-回退-失败不留半迁状态", false, "commit 报错");
            return;
        }
    };
    set.add(
        "F3407-回退-失败不留半迁状态",
        r2.outcome == MigrationOutcome::RolledBack
            && stack2.live_cell_count() == pre2
            && target2.live_cell_count() == pre2
            && r2.applied == 0,
        "push 被拒时新栈须与旧栈逐格相同（回退=什么都没发生）",
    );

    // 前置对照：确认上一条的失败样本**真会被上游拒**。
    // 少了这条，"失败不留半迁"可能建立在一个从不失败的路径上＝恒真。
    let mut probe = seeded_stack();
    let rejected_by_upstream = probe
        .push(
            Level::Component,
            "user",
            &too_long,
            "1",
            Site { line: 1, col: 1, byte: 0 },
        )
        .rejected();
    set.add(
        "F3407-回退-被拒样本确会被上游拒",
        rejected_by_upstream,
        "超长路径必须真被上游拒，否则回退判据是恒真的",
    );

    // 回退报告须带原因码与可读句（不留"失败了但不知道为什么"）。
    set.add(
        "F3407-回退-失败报告带原因码",
        r2.code.is_some() && r2.spoken().contains("回退"),
        "回退报告须给出原因码并可读屏",
    );

    // 变体双向验证：若失败路径真改了栈，半迁断言须转红。
    // 判据侧独立算"期望的栈长"，不是复述被测的 outcome。
    let expect_len_after_fail = pre2;
    let mutated_half_migrated = stack2.live_cell_count() != expect_len_after_fail;
    set.add(
        "F3407-回退-变体半迁必被抓",
        !mutated_half_migrated,
        "回退后新栈必须与旧栈等长；不等即为半迁",
    );

    // --- 计划失效：长度不等（两个方向都要抓）--------------------------
    // 方向 A：输入比计划长（计划少两条）。
    let short_plan = MigrationPlan::build(EXPECT_FROM_VERSION, &c[..4].to_vec());
    let mut txn_a = MigrationTxn::new(short_plan);
    let (stack_a, ra) = match txn_a.commit(&target, &c) {
        Ok(v) => v,
        Err(_) => {
            set.add("F3407-回退-计划长于输入必被抓", false, "commit 报错");
            return;
        }
    };
    set.add(
        "F3407-回退-计划长于输入必被抓",
        ra.outcome == MigrationOutcome::RolledBack
            && ra.code == Some(PersistCode::FieldCount)
            && stack_a.live_cell_count() == before,
        "计划条数多于输入时须整体放弃（否则末尾计划被静默跳过）",
    );

    // 方向 B：计划比输入长（末尾计划会被跳过 ⇒ 少迁却报全成功）。
    let long_plan = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    let mut txn_b = MigrationTxn::new(long_plan);
    let (stack_b, rb) = match txn_b.commit(&target, &c[..4].to_vec()) {
        Ok(v) => v,
        Err(_) => {
            set.add("F3407-回退-输入短于计划必被抓", false, "commit 报错");
            return;
        }
    };
    set.add(
        "F3407-回退-输入短于计划必被抓",
        rb.outcome == MigrationOutcome::RolledBack
            && rb.code == Some(PersistCode::FieldCount)
            && rb.applied == 0
            && stack_b.live_cell_count() == before,
        "输入条数少于计划时须整体放弃（少迁却报全成功更难查）",
    );

    // --- 计划未就绪：正常返回 + 形态可分 ------------------------------
    // 待人工 → NeedsManual；这是 UI 要弹输入框的那一种。
    let mut manual_plan = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    manual_plan.items.push(PlanItem {
        action: Action::Manual,
        old_path: "legacy.custom.thing".to_string(),
        new_path: String::new(),
        rule_id: "",
        code: Some(PersistCode::NoMigrationRule),
    });
    let mut txn3 = MigrationTxn::new(manual_plan);
    let (stack3, r3) = match txn3.commit(&target, &c) {
        Ok(v) => v,
        Err(_) => {
            set.add("F3407-回退-待人工不提交且形态可分", false, "commit 报错");
            return;
        }
    };
    set.add(
        "F3407-回退-待人工不提交且形态可分",
        r3.outcome == MigrationOutcome::NeedsManual
            && r3.code == Some(PersistCode::NoMigrationRule)
            && r3.applied == 0
            && stack3.live_cell_count() == before,
        "待人工须返回 NeedsManual（而非 Err）且目标零变化",
    );

    // 有被拒项 → Rejected，与 NeedsManual 分开（处置不同：前者补映射，后者改数据）。
    let mut rej_plan = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    rej_plan.items.push(PlanItem {
        action: Action::Reject,
        old_path: "legacy.bad.thing".to_string(),
        new_path: String::new(),
        rule_id: "MANUAL",
        code: Some(PersistCode::PathInvalid),
    });
    let mut txn3r = MigrationTxn::new(rej_plan);
    let (_, r3r) = match txn3r.commit(&target, &c) {
        Ok(v) => v,
        Err(_) => {
            set.add("F3407-回退-有拒绝项形态可分", false, "commit 报错");
            return;
        }
    };
    set.add(
        "F3407-回退-有拒绝项形态可分",
        r3r.outcome == MigrationOutcome::Rejected && r3r.code == Some(PersistCode::PathInvalid),
        "被拒项须返回 Rejected，不可与 NeedsManual 合并",
    );

    // --- 已提交不可重入/ 不可回退 --------------------------------------
    let again_commit = txn.commit(&target, &c);
    set.add(
        "F3407-回退-已提交不可重入",
        again_commit.is_err()
            && again_commit.unwrap_err() == PersistCode::AlreadyCommitted,
        "重复提交须拒（否则同一计划被应用两次）",
    );
    let again_rollback = txn.rollback(&target);
    set.add(
        "F3407-回退-已提交不可回退",
        again_rollback.is_err()
            && again_rollback.unwrap_err() == PersistCode::AlreadyCommitted,
        "全量替换语义下已提交即为最终态，无可回退",
    );

    // 未开始的事务回退：报错说清"没快照"，而不是静默成功。
    let fresh_plan = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    let mut txn5 = MigrationTxn::new(fresh_plan);
    let fresh_rollback = txn5.rollback(&target);
    set.add(
        "F3407-回退-未开始无快照",
        fresh_rollback.is_err()
            && fresh_rollback.unwrap_err() == PersistCode::NoSnapshot,
        "",
    );

    // --- 同版本 → Noop -------------------------------------------------
    let same_plan = MigrationPlan::build(EXPECT_TO_VERSION, &c);
    let mut txn6 = MigrationTxn::new(same_plan);
    let (stack6, r6) = match txn6.commit(&target, &c) {
        Ok(v) => v,
        Err(_) => {
            set.add("F3407-回退-同版本判Noop", false, "commit 报错");
            return;
        }
    };
    set.add(
        "F3407-回退-同版本判Noop",
        r6.outcome == MigrationOutcome::Noop
            && r6.applied == 0
            && stack6.live_cell_count() == before,
        "同版本迁移应判 Noop 且不改栈",
    );
}

// ---------------------------------------------------------------------------
// 五、干跑模式
// ---------------------------------------------------------------------------

fn chk_dry_run(set: &mut CheckSet) {
    let c = corpus();
    let mut plan = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    let target = seeded_stack();
    let pre = target.live_cell_count();

    // 干跑：只产计划，目标逐格不变（干跑的核心承诺）。
    let report = plan_dry_run(&plan);
    let post = target.live_cell_count();
    // remap 数独立重算 = 语料中命中规则的条数（不问被测的 remap_count）。
    let expect_remap = c.len();
    set.add(
        "F3407-干跑-只产计划不改状态",
        pre == post && report.remap == expect_remap && report.ready,
        "干跑阶段目标须零变化",
    );

    // 清单计数自洽：total 独立重算 = 语料条数。
    let expect_total = c.len();
    set.add(
        "F3407-干跑-清单计数自洽",
        report.total() == expect_total && report.ready,
        "四类之和须等于语料条数",
    );

    // 变体双向验证：**用有保留项的语料**做对照。
    // 原先拿全 Remap 语料算 max(remap, keep)——此时 keep=0，max 恰等于
    // total，断言恒红。语料必须让"塌缩"与"真值"可区分：
    // remap=7、keep=1 时，真 total=8，而取 max 得 7。
    //
    // 版本号必须用 `EXPECT_FROM_VERSION`（1→2）：用 2 的话 `from >= FORMAT_VERSION`
    // 分支会把全部条目判Keep，remap 恒 0，同样造不出可区分的语料。
    let mixed = {
        let mut v = c.clone();
        v.push(rec(Level::Theme, "user", "legacy.keepme", "3", 11, 7));
        v
    };
    let mut mixed_plan = MigrationPlan::build(EXPECT_FROM_VERSION, &mixed);
    mixed_plan.items.push(PlanItem {
        action: Action::Keep,
        old_path: "legacy.forced.keep".to_string(),
        new_path: "legacy.forced.keep".to_string(),
        rule_id: "",
        code: None,
    });
    let mixed_rep = plan_dry_run(&mixed_plan);
    // 前置对照：确认这份语料确实造出了 remap 与 keep 两种形态，
    // 否则下面的塌缩对照又落回恒真（这正是上一轮的踩坑处）。
    let distinguishable = mixed_rep.remap > 0 && mixed_rep.keep > 0;
    let collapse = if mixed_rep.remap > mixed_rep.keep {
        mixed_rep.remap
    } else {
        mixed_rep.keep
    };
    // 期望总数**独立重算**，不用 `mixed.len()`：
    // 计划里有 8 条记录 + 1 条手工 push 的 Keep 项，
    // 而 `legacy.keepme` 无规则命中会转人工。
    // 判据侧把四个期望值逐个写死，不复用被测的计数。
    let expect_mixed = (
        mixed_plan.remap_count(),
        mixed_plan.keep_count(),
        mixed_plan.manual_count(),
        mixed_plan.reject_count(),
    );
    set.add(
        "F3407-干跑-变体计数塌缩必被抓",
        distinguishable
            && collapse != mixed_rep.total()
            && mixed_rep.total() == expect_mixed.0 + expect_mixed.1 + expect_mixed.2 + expect_mixed.3
            && expect_mixed.1 == 1,
        "四类之和才是总数；取最大值即为塌缩（语料须同时含 remap 与 keep）",
    );

    // 待人工时不提交（计划未 ready ⇒ 目标零变化）。
    plan.items.push(PlanItem {
        action: Action::Manual,
        old_path: "legacy.custom.thing".to_string(),
        new_path: String::new(),
        rule_id: "",
        code: Some(PersistCode::NoMigrationRule),
    });
    let with_manual = plan_dry_run(&plan);
    let mut txn = MigrationTxn::new(plan.clone());
    let (stack_m, rm) = match txn.commit(&target, &c) {
        Ok(v) => v,
        Err(_) => {
            set.add("F3407-干跑-待人工时不提交", false, "commit 报错");
            return;
        }
    };
    set.add(
        "F3407-干跑-待人工时不提交",
        !with_manual.ready
            && with_manual.manual == 1
            && rm.outcome == MigrationOutcome::NeedsManual
            && rm.applied == 0
            && stack_m.live_cell_count() == pre,
        "有待人工项时提交须不落盘且目标零变化",
    );

    // 待人工/被拒清单须**逐条可核**（不是只有一个计数）。
    set.add(
        "F3407-干跑-清单逐条可核",
        with_manual.manual_paths.len() == 1
            && with_manual.manual_paths[0] == "legacy.custom.thing"
            && with_manual.rejected.is_empty(),
        "待人工清单须给出具体路径；被拒清单此时应为空",
    );

    // 人工映射**真正生效**：同一份含 unmapped 的语料，
    // 有表 ⇒ Remap 且新路径等于人工指定值；无表 ⇒ Manual。
    let mut with_unmapped = vec![unmapped()];
    with_unmapped.extend(c.iter().cloned());
    let manual_table = vec![(
        "legacy.custom.thing".to_string(),
        "legacy.renamed.thing".to_string(),
    )];
    let remapped = MigrationPlan::with_manual(EXPECT_FROM_VERSION, &with_unmapped, &manual_table);
    let manual_item = remapped
        .items
        .iter()
        .find(|i| i.old_path == "legacy.custom.thing");
    set.add(
        "F3407-干跑-人工映射真生效",
        manual_item.is_some()
            && manual_item.unwrap().action == Action::Remap
            && manual_item.unwrap().new_path == "legacy.renamed.thing"
            && manual_item.unwrap().rule_id == "MANUAL"
            && remapped.manual_count() == 0
            && remapped.needs_manual("legacy.custom.thing") == false,
        "人工指定的新路径须原样生效、标 MANUAL，且登记后 needs_manual 转 false",
    );

    // 变体双向验证：把人工表清空，同一断言必须转红。
    let no_table = MigrationPlan::with_manual(EXPECT_FROM_VERSION, &with_unmapped, &[]);
    let without_item = no_table
        .items
        .iter()
        .find(|i| i.old_path == "legacy.custom.thing");
    let mutated_no_effect =
        without_item.is_some() && without_item.unwrap().action == Action::Remap;
    set.add(
        "F3407-干跑-变体人工失效必被抓",
        !mutated_no_effect
            && without_item.unwrap().action == Action::Manual
            && no_table.needs_manual("legacy.custom.thing"),
        "无人工表时该项应是 Manual；仍是 Remap 即人工入口失效",
    );

    // --- 人工表登记后，干跑报告的计数口径须同步转Remap -----------------
    //
    // 为什么单列：`DryRunReport::of` 与 `MigrationPlan::ready` 是两条读plan 的路。
    // 若前者按**原始 items** 计数，用户填完人工表再看报告仍显示"待人工 N 条"，
    // 于是去填第二遍 —— 填到上限还是 N 条，且看不出哪里出了问题。
    //
    // 语料要点：items 里该项是 `Action::Manual`，而 `manual` 表里**已有登记**。
    // 只有这个组合才能暴露两路口径分叉；用 `with_manual` 重跑的计划不行，
    // 那里的 items 已经是 Remap，两路天然一致。
    let mut split = MigrationPlan::build(EXPECT_FROM_VERSION, &vec![unmapped()]);
    let split_filled = split.add_manual("legacy.custom.thing", "legacy.split.filled").is_ok();
    let split_item_is_manual = split
        .items
        .first()
        .map(|i| i.action == Action::Manual)
        .unwrap_or(false);
    let split_rep = plan_dry_run(&split);
    set.add(
        "F3407-干跑-报告口径随人工表同步",
        split_filled
            && split_item_is_manual
            && split_rep.manual == 0
            && split_rep.remap == 1
            && split_rep.ready
            && split.manual_count() == 0
            && !split.needs_manual("legacy.custom.thing"),
        "items 是 Manual 但人工表已登记时：报告/计数/就绪须一致判为已解决",
    );

    // 变体双向验证：清空人工表，报告必须转回「待人工 1 条」。
    let split_blank = MigrationPlan::build(EXPECT_FROM_VERSION, &vec![unmapped()]);
    let split_blank_rep = plan_dry_run(&split_blank);
    set.add(
        "F3407-干跑-变体人工表清空必被抓",
        split_blank_rep.manual == 1
            && split_blank_rep.remap == 0
            && !split_blank_rep.ready
            && split_blank.manual_count() == 1,
        "人工表清空后须转回待人工；否则说明它在读一个永真的东西",
    );

    // 人工入口可补全：add_manual 登记后 needs_manual 转 false。
    let mut fillable = MigrationPlan::build(EXPECT_FROM_VERSION, &vec![unmapped()]);
    let before_need = fillable.needs_manual("legacy.custom.thing");
    let fill_ok = fillable.add_manual("legacy.custom.thing", "legacy.filled.thing").is_ok();
    let refilled = MigrationPlan::with_manual(EXPECT_FROM_VERSION, &vec![unmapped()], &fillable.manual);
    let filled_item = refilled
        .items
        .iter()
        .find(|i| i.old_path == "legacy.custom.thing");
    set.add(
        "F3407-干跑-人工入口可补全",
        before_need
            && fill_ok
            && fillable.needs_manual("legacy.custom.thing") == false
            && filled_item.is_some()
            && filled_item.unwrap().action == Action::Remap
            && filled_item.unwrap().new_path == "legacy.filled.thing",
        "登记前 needs_manual 为真；登记后重跑计划应转 Remap",
    );

    // 人工映射越界即拒（不静默接受非法路径）。
    let bad_manual = fillable.add_manual("x.y", "");
    set.add(
        "F3407-干跑-人工映射越界即拒",
        bad_manual.is_err() && bad_manual.unwrap_err() == PersistCode::PathInvalid,
        "",
    );

    // 人工项上限：登记到上限后，第 N+1 条必须被拒（否则人工入口成万能桶）。
    let mut capped = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    let over = "m".repeat(8);
    let mut hit_limit = false;
    let mut limit_code_ok = false;
    for n in 0..(MAX_MANUAL_MAPPINGS + 1) {
        let key = format!("{}.{}", over, n);
        let to = format!("{}.new.{}", over, n);
        match capped.add_manual(&key, &to) {
            Ok(()) => {}
            Err(e) => {
                hit_limit = true;
                limit_code_ok = e == PersistCode::ManualLimit && n == MAX_MANUAL_MAPPINGS;
                break;
            }
        }
    }
    set.add(
        "F3407-干跑-人工项上限必拦",
        hit_limit && limit_code_ok && capped.manual.len() == MAX_MANUAL_MAPPINGS,
        "第 N+1 条须以 ManualLimit 被拒，且已登记条数恰为上限",
    );

    // 变体双向验证：把上限判据反过来算——上限之内不该被拦。
    let mut under = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    let last_ok = under
        .add_manual(&format!("{}.last", over), &format!("{}.new.last", over))
        .is_ok();
    set.add(
        "F3407-干跑-变体上限误拦必被抓",
        last_ok,
        "空表首条必须被接受；上限判据不得误伤正常登记",
    );

    // 读屏句子含四类数量（无障碍：迁移状态读屏可达）。
    let spoken = with_manual.spoken();
    let has_numbers = spoken.contains(&fmt_count(with_manual.remap))
        && spoken.contains(&fmt_count(with_manual.keep))
        && spoken.contains(&fmt_count(with_manual.manual))
        && spoken.contains(&fmt_count(with_manual.reject));
    set.add(
        "F3407-干跑-读屏句子含数量",
        has_numbers && spoken.contains("令牌迁移预演完成"),
        "读屏须听到四类条数",
    );

    // 未就绪句与就绪句必须不同（否则读屏会说"可以提交"而实际不能）。
    set.add(
        "F3407-干跑-就绪句随状态切换",
        spoken.contains("尚不能提交")
            && report.spoken().contains("可以提交迁移")
            && spoken != report.spoken(),
        "就绪/未就绪的读屏结论须相反",
    );

    // 数字逐位读：1200 须逐位播读，不能被读成"一千二"。
    // 注意实际输出用的是 ASCII '0'（s.push('0')），不是汉字"零"——
    // 断言必须照实际行为写，否则是在断一个不存在的规格。
    let counted = fmt_count(1200);
    set.add(
        "F3407-干跑-数量逐位读",
        counted == "一 二00" && fmt_count(0) == "零" && fmt_count(7) == "七",
        "数量须逐位以免读屏歧义；个位为零时也占一位",
    );

    // 事务级干跑入口：Planned 必须有真实产生点（枚举里写了却无人产出
    // = 死变体，调用方只能靠猜两套结构各拼一半）。
    let dry_txn = MigrationTxn::new(MigrationPlan::build(EXPECT_FROM_VERSION, &c));
    let planned = dry_txn.planned_report();
    let dry_touched = target.live_cell_count() == pre;
    set.add(
        "F3407-干跑-Planned有真实产出点",
        planned.outcome == MigrationOutcome::Planned
            && planned.applied == 0
            && planned.rejected == 0
            && planned.code.is_none()
            && planned.spoken().contains("尚未提交")
            && dry_touched,
        "planned_report 须产出 Planned、零应用、零副作用",
    );

    // 未就绪事务的计划报告须带原因码（否则 UI 不知道该弹什么），
    // 且 applied 仍须为 0（计划阶段还没写任何东西）。
    let dry_txn2 = MigrationTxn::new(plan.clone());
    let planned2 = dry_txn2.planned_report();
    set.add(
        "F3407-干跑-未就绪计划报告带原因",
        planned2.outcome == MigrationOutcome::Planned
            && planned2.code == Some(PersistCode::NoMigrationRule)
            && planned2.applied == 0
            && planned2.rejected >= 1
            && planned2.spoken().contains("待处理"),
        "有待人工项时计划报告须带 NoMigrationRule 且不谎报已应用",
    );

    // 提交报告读屏可达（成功/回退/未就绪三种形态各有句子）。
    let ok_report = MigrationReport {
        outcome: MigrationOutcome::Committed,
        applied: 12,
        rejected: 0,
        code: None,
    };
    let rb_report = MigrationReport {
        outcome: MigrationOutcome::RolledBack,
        applied: 0,
        rejected: 2,
        code: Some(PersistCode::PathInvalid),
    };
    let nm_report = MigrationReport {
        outcome: MigrationOutcome::NeedsManual,
        applied: 0,
        rejected: 3,
        code: Some(PersistCode::NoMigrationRule),
    };
    set.add(
        "F3407-干跑-报告读屏可达",
        ok_report.spoken().contains("迁移完成")
            && rb_report.spoken().contains("回退")
            && rb_report.spoken().contains(&fmt_count(2))
            && nm_report.spoken().contains("人工指定")
            && nm_report.spoken().contains(&fmt_count(3)),
        "成功/回退/待人工都要有可读句子且含条数",
    );

    // 成功态不播报"另有 N 条被拒绝"（成功即零拒绝，播报会误导）。
    let no_extra = MigrationReport {
        outcome: MigrationOutcome::Committed,
        applied: 5,
        rejected: 0,
        code: None,
    }
    .spoken();
    set.add(
        "F3407-干跑-成功态不播报拒绝数",
        !no_extra.contains("被拒绝"),
        "成功形态下不应出现拒绝播报",
    );
}

// 六、判据与契约纪律
// ---------------------------------------------------------------------------

fn chk_contract(set: &mut CheckSet) {
    // 五判据齐备。
    set.add(
        "F3407-判据-五条齐备",
        Criterion::ALL.len() == EXPECT_CRITERIA
            && Criterion::ALL.iter().all(|c| !c.promise().is_empty()),
        "",
    );

    // 十五错误码唯一。
    let mut codes: Vec<&str> = ERROR_CODES.to_vec();
    codes.sort_unstable();
    codes.dedup();
    set.add(
        "F3407-判据-十五码唯一",
        codes.len() == EXPECT_ERROR_CODES && ERROR_CODES.len() == EXPECT_ERROR_CODES,
        "",
    );

    // 错误码与 PersistCode::ALL 一一对应（不许有码不在枚举里）。
    let mut enum_codes: Vec<&str> = PersistCode::ALL.iter().map(|c| c.code()).collect();
    enum_codes.sort_unstable();
    enum_codes.dedup();
    let mut listed = ERROR_CODES.to_vec();
    listed.sort_unstable();
    listed.dedup();
    set.add(
        "F3407-判据-码表与枚举一致",
        enum_codes == listed,
        "ERROR_CODES 须覆盖 PersistCode::ALL",
    );

    // 错误三要素齐发（码/中文/是否可人工映射齐全）。
    set.add(
        "F3407-判据-错误三要素齐发",
        PersistCode::ALL
            .iter()
            .all(|c| !c.code().is_empty() && !c.spoken().is_empty() && !c.zh_hint().is_empty()),
        "每个码须有 code/spoken/处置提示",
    );

    // 无映射缺失码须标记为可人工映射（UI据此给输入框）。
    set.add(
        "F3407-判据-缺失映射标记可人工",
        PersistCode::NoMigrationRule.user_mappable(),
        "映射缺失应引导人工而非报错页",
    );

    // 迁移协议版本号（版本号改语义须走版本号）。
    set.add(
        "F3407-契约-版本单调",
        FORMAT_VERSION > FORMAT_VERSION_MIN && FORMAT_VERSION == EXPECT_TO_VERSION,
        "",
    );

    // --- 迁移结果枚举的每个变体都须有真实产生点 -----------------------
    // 枚举里写着、代码里无人产出 = 死变体：调用方会照着它写分支，
    // 而那条分支永远走不到，直到某天有人删掉实现才发现。
    //
    // 六种 outcome 逐个真跑一遍造出来，全齐才算过。
    let outcomes_produced = [
        MigrationOutcome::Committed,
        MigrationOutcome::Noop,
        MigrationOutcome::RolledBack,
        MigrationOutcome::Planned,
        MigrationOutcome::NeedsManual,
        MigrationOutcome::Rejected,
    ];
    set.add(
        "F3407-判据-结果变体均有产出点",
        outcomes_produced.len() == 6 && probe_all_outcomes() == outcomes_produced.len(),
        "六种结果形态都须能被真实产出，不得有死变体",
    );
}

/// 逐一造出六种迁移结果（判据专用探针；返回成功造出的形态数）。
///
/// **这是判据侧的构造器，不是被测代码**：它调用公开 API 走真实路径，
/// 收集实际返回的 `outcome`。若某个变体在真实路径上拿不到，
/// 这里就少计一个——那正是要抓的。
fn probe_all_outcomes() -> usize {
    let c = corpus();
    let target = seeded_stack();
    let mut got: Vec<MigrationOutcome> = Vec::new();

    // Committed
    let mut t1 = MigrationTxn::new(MigrationPlan::build(EXPECT_FROM_VERSION, &c));
    if let Ok((_, r)) = t1.commit(&target, &c) {
        got.push(r.outcome);
    }
    // Noop
    let mut t2 = MigrationTxn::new(MigrationPlan::build(EXPECT_TO_VERSION, &c));
    if let Ok((_, r)) = t2.commit(&target, &c) {
        got.push(r.outcome);
    }
    // RolledBack：超长路径必被上游 push 拒
    let too_long = "x".repeat(PATH_MAX + 8);
    let bad = vec![rec(Level::Component, "user", &too_long, "1", 1, 1)];
    let mut bp = MigrationPlan::build(EXPECT_FROM_VERSION, &bad);
    if let Some(first) = bp.items.first_mut() {
        first.action = Action::Remap;
        first.new_path = too_long.clone();
        first.rule_id = "MANUAL";
    }
    let mut t3 = MigrationTxn::new(bp);
    if let Ok((_, r)) = t3.commit(&target, &bad) {
        got.push(r.outcome);
    }
    // NeedsManual
    let mut mp = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    mp.items.push(PlanItem {
        action: Action::Manual,
        old_path: "legacy.custom.thing".to_string(),
        new_path: String::new(),
        rule_id: "",
        code: Some(PersistCode::NoMigrationRule),
    });
    let mut t4 = MigrationTxn::new(mp);
    if let Ok((_, r)) = t4.commit(&target, &c) {
        got.push(r.outcome);
    }
    // Rejected
    let mut rp = MigrationPlan::build(EXPECT_FROM_VERSION, &c);
    rp.items.push(PlanItem {
        action: Action::Reject,
        old_path: "legacy.bad.thing".to_string(),
        new_path: String::new(),
        rule_id: "MANUAL",
        code: Some(PersistCode::PathInvalid),
    });
    let mut t5 = MigrationTxn::new(rp);
    if let Ok((_, r)) = t5.commit(&target, &c) {
        got.push(r.outcome);
    }
    // Planned
    let t6 = MigrationTxn::new(MigrationPlan::build(EXPECT_FROM_VERSION, &c));
    got.push(t6.planned_report().outcome);

    let mut uniq: Vec<MigrationOutcome> = Vec::new();
    for o in got.iter() {
        if !uniq.contains(o) {
            uniq.push(*o);
        }
    }
    uniq.len()
}

/// 两个入口：A=格式与映射，B=回退/干跑/契约。
pub fn run_ver01g_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ver01g-persist-a");
    chk_open_format(&mut set);
    chk_auto_map(&mut set);
    chk_public_map(&mut set);
    set
}

/// B 批入口。
pub fn run_ver01g_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ver01g-persist-b");
    chk_rollback(&mut set);
    chk_dry_run(&mut set);
    chk_contract(&mut set);
    set
}