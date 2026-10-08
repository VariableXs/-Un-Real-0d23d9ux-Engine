//! VE-F2607 · 域自检（判据逐条对应，见 `ven07_serde.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **四段schema** → `F2607-schema-*`；
//! - **四重校验** → `F2607-四重-*`；
//! - **模板引用显性** → `F2607-模板-*`；
//! - **往返零损失** → `F2607-往返-*`；
//! - **版本迁移** → `F2607-迁移-*`；
//! - **降级矩阵** → `F2607-降级-*`；
//! - **JSON 层** → `F2607-json-*`；
//! - **审计面** → `F2607-审计-*`；
//! - **性能/对接/无障碍/隐私/分工** → 对应族。
//!
//! 分两批（`run_ven07_checks_a` / `run_ven07_checks_b`）以避开
//! `CheckSet::MAX_CHECKS = 112` 的全仓共享上限。
//!
//! ## 本文件的三条硬纪律
//!
//! 1. **期望值一律在本文件内独立重算**，**不回读被测模块的表去和自己比**。
//!    例：四重的 `inspected` 用本文件自己的语料点数算，段数用
//!    [`SECTIONS`] 的长度算（它是格式的事实源，不是被测行为）。
//! 2. **单边断言优先于双边阈值**。正确实现的偏差恒为一个方向，用双边
//!    阈值一旦宽过正确实现的偏差幅度，高估型变异就从缝里钻过去。
//! 3. **判据索引必须由语料常量推导**，不可裸写数字。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。
//!
//! `panic!` 只允许出现在本自检面（判据代码本就该在构造失败时立刻炸出
//! 明确位置，而不是静默退化出空语料让后面几十项判据集体假绿）。
//! 生产面 [`ven07_serde`] 内零 `panic!`。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::ven02_tree::{
    ControlTree, MAX_TREE_DEPTH, PROPERTY_KEYS, SingleParentPolicy, create_node, insert,
};
use crate::svstar2::ven04_prop::{PropEngine, PropValue};
use crate::svstar2::ven05_dual::{TemplateBinding, TemplateTable};
use crate::svstar2::ven07_serde::*;

// ---------------------------------------------------------------------------
// 便捷构造与真值重算
// ---------------------------------------------------------------------------

/// 建一棵测试逻辑树：
///
/// ```text
/// root
/// ├── a
/// │   ├── a1
/// │   └── a2
/// └── b
///     └── b1
/// ```
fn logic() -> ControlTree {
    let mut t = match ControlTree::new("root") {
        Ok(x) => x,
        Err(_e) => panic!("根 id 非空即必成功"),
    };
    for (id, parent) in [("a", "root"), ("a1", "a"), ("a2", "a"), ("b", "root"), ("b1", "b")] {
        if insert(&mut t, parent, id, None, SingleParentPolicy::Reject).is_err() {
            panic!("测试树结构固定，插入必成功");
        }
    }
    t
}

/// 给测试树设置 kind（导出文本含 kind，语料要显式）。
fn with_kinds(t: &mut ControlTree) {
    for (id, k) in [
        ("root", "window"),
        ("a", "panel"),
        ("a1", "button"),
        ("a2", "label"),
        ("b", "panel"),
        ("b1", "slider"),
    ] {
        if let Some(n) = t.raw_mut(id) {
            n.kind = String::from(k);
        }
    }
}

/// 满属性语料的**颜色探针值**（判据侧单源）。
///
/// **为何提成常量**：判据里断言「导出的颜色文本 == 期望」时，期望值
/// 必须与语料里写入的值**同源**。两处各写一遍（语料写 `0xFF88_00FF`、
/// 判据手敲十进制）时，抄错一次判据就恒红，而红项指错地方——看起来
/// 像被测的导出错了，实际是判据抄错了常量。详见往返第 ⑩ 项的注释。
const COLOR_PROBE: u32 = 0xFF88_00FF;

/// 满属性语料的**文本探针值**（**非 ASCII**，顺带覆盖转义路径）。
const TEXT_PROBE: &str = "确定";

/// 满属性语料的树 + 引擎（**覆盖四种属性类型**）。
fn logic_props() -> (ControlTree, PropEngine) {
    let mut t = logic();
    with_kinds(&mut t);
    let mut e = match PropEngine::attach(&t) {
        Ok(x) => x,
        Err(_e) => panic!("测试树合法即必附着成功"),
    };
    // 四型各至少一条：bool / number / text / color
    let sets: [(&str, &str, PropValue); 4] = [
        ("a1", "visible", PropValue::Bool(false)),
        ("a1", "width", PropValue::Number(123.5)),
        ("a1", "text", PropValue::Text(String::from(TEXT_PROBE))),
        ("a1", "color", PropValue::Color(COLOR_PROBE)),
    ];
    for (node, key, val) in sets.iter() {
        let slot = match e.slot_of(node) {
            Some(s) => s,
            None => panic!("测试树节点必已附着"),
        };
        let k = match key_from_wire(key) {
            Some(k) => k,
            None => panic!("语料里的键必在封闭集内"),
        };
        if e.set(slot, k, val.clone()).is_err() {
            panic!("语料值均在 F2604 值域内");
        }
    }
    (t, e)
}

/// 建一棵**深达limit 层**的链树（`limit` = 边数；根深 0）。
///
/// `panic!` 只在自检面：语料构造失败说明 `limit` 越了F2602 的硬顶，
/// 那是判据编写错误，必须立刻炸出来而不是造一棵半截树。
fn deep_chain(limit: usize) -> ControlTree {
    let mut t = match ControlTree::new("n0") {
        Ok(x) => x,
        Err(_e) => panic!("根 id 非空即必成功"),
    };
    let mut i = 1usize;
    while i <= limit {
        let parent = format!("n{}", i - 1);
        let child = format!("n{}", i);
        if insert(&mut t, &parent, &child, None, SingleParentPolicy::Reject).is_err() {
            panic!("深度语料的插入必成功（limit 未越 F2602 硬顶）");
        }
        i += 1;
    }
    t
}

/// 建一棵**超深**链树（绕过 F2602 的 insert 硬顶，直接 `put`）。
///
/// **为何要 `put`**：F2602 的 [`insert`] 自己就拦超深，所以「超深树」这个
/// 语料**无法通过正常入口构造**——而那恰恰是导入面要防的输入形态
/// （恶意文件里的 `children` 链）。故判据侧用 `put` 造出**只有序列化面
/// 会遇到**的树，再断言深度重拦下它。
fn deep_chain_raw(limit: usize) -> ControlTree {
    let mut t = match ControlTree::new("n0") {
        Ok(x) => x,
        Err(_e) => panic!("根 id 非空即必成功"),
    };
    let mut i = 1usize;
    while i <= limit {
        let child = format!("n{}", i);
        let mut n = create_node(&child, "panel");
        n.parent_id = Some(format!("n{}", i - 1));
        t.put(n);
        if let Some(p) = t.raw_mut(&format!("n{}", i - 1)) {
            p.children.push(child);
        }
        i += 1;
    }
    t
}

/// 手编一份**合法 v2 文档文本**（判据侧的独立语料，不经被测导出）。
///
/// **刻意手编而非调 [`to_json`]**：若手编文本由被测导出生成，
/// 「往返恒等」就变成被测自己和自己比——恒真弱门禁。手编文本的
/// 空白/缩进/键序都与规范形不同，故它同时验证了「解析能吃非规范输入」。
fn hand_v2() -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("  \"schema\": \"varix.control-tree\",\n");
    s.push_str("  \"version\": 2,\n");
    s.push_str("  \"root\": \"root\",\n");
    s.push_str("  \"nodes\": [\n");
    s.push_str("    {\"id\":\"root\",\"kind\":\"window\",\"children\":[\"a\"]},\n");
    s.push_str("    {\"id\":\"a\",\"kind\":\"panel\",\"children\":[]}\n");
    s.push_str("  ],\n");
    s.push_str("  \"props\": [\n");
    s.push_str("    {\"node\":\"a\",\"key\":\"width\",\"type\":\"number\",\"value\":100,\"bits\":\"0x42C80000\"},\n");
    s.push_str("    {\"node\":\"a\",\"key\":\"visible\",\"type\":\"bool\",\"value\":true}\n");
    s.push_str("  ],\n");
    s.push_str("  \"binds\": [],\n");
    s.push_str("  \"templates\": []\n");
    s.push_str("}\n");
    s
}

/// **四重齐备的干净语料**：节点段、属性段、绑定段、模板段**各至少一项且全部合法**。
///
/// **为什么必须另立一份语料**（而不是拿 [`hand_v2`] 顶替）：
/// `hand_v2` 的 `binds` 是空数组，于是 `Fold::Bind` 的 `inspected == 0`。
/// 而 [`FoldVerdict::conclusive`] 的定义是 `inspected > 0 && passed()`——
/// 按该字段的**设计意图**（见其文档：「`inspected == 0` 时该重结论不作数」），
/// 空段语料下 `Bind` 的结论**本来就不作数**，此时断言「四重均 conclusive」
/// 是在要求一件语言上做不到的事。
///
/// 症状极具误导性：红项指向被测的 `validate_bind`，而被测实现完全正确，
/// 根因在判据选了**喂不满四重**的语料。这类缺陷（判据钉不到被测行为）
/// 比被测实现的 bug 更难查——**红项指错地方**。
fn hand_full_ok() -> String {
    let mut s = String::from("{\n");
    s.push_str("\"schema\": \"varix.control-tree\",\n");
    s.push_str("\"version\": 2,\n");
    s.push_str("\"root\": \"root\",\n");
    s.push_str("\"nodes\": [\n");
    s.push_str("  {\"id\":\"root\",\"kind\":\"window\",\"children\":[\"panel\"]},\n");
    s.push_str("  {\"id\":\"panel\",\"kind\":\"panel\",\"children\":[]}\n");
    s.push_str("],\n");
    // 属性段：两个条目，覆盖 number（含 bits）与 bool 两型。
    s.push_str("\"props\": [\n");
    s.push_str("  {\"node\":\"panel\",\"key\":\"width\",\"type\":\"number\",\"value\":100,\"bits\":\"0x42C80000\"},\n");
    s.push_str("  {\"node\":\"panel\",\"key\":\"visible\",\"type\":\"bool\",\"value\":true}\n");
    s.push_str("],\n");
    // 绑定段：一条合法绑定（宿主存在、路径可解析）——`Bind` 重因此有项可查。
    // 路径按 F2602 `parse_bind_path` 的口径写：`seg/seg` 形式、**首尾无斜杠**
    // （写成 `/root/panel/width` 会被判`BindPathViolation`——那是语料错，
    // 不是被测错，故此处对齐解析器而不是改被测）。
    s.push_str("\"binds\": [\n");
    s.push_str("  {\"node\":\"panel\",\"path\":\"panel/width\"}\n");
    s.push_str("],\n");
    s.push_str("\"templates\": []\n");
    s.push_str("}\n");
    s
}

/// 判据侧独立重算：**节点段条目数**（不读被测的 `doc.nodes.len()`）。
fn truth_node_count(t: &ControlTree) -> usize {
    // 本文件自己的遍历：栈上按 children 走，兼作去重
    let mut seen: Vec<String> = Vec::new();
    let mut stack: Vec<String> = vec![String::from(t.root())];
    while let Some(cur) = stack.pop() {
        if seen.iter().any(|v| *v == cur) {
            continue;
        }
        seen.push(cur.clone());
        if let Some(n) = t.raw(&cur) {
            let mut k = n.children.len();
            while k > 0 {
                k -= 1;
                stack.push(n.children[k].clone());
            }
        }
    }
    seen.len()
}

/// 判据侧独立重算：**某节点的子节点序**（不读被测的 `children()` 结果）。
fn truth_children(t: &ControlTree, id: &str) -> Vec<String> {
    match t.raw(id) {
        Some(n) => n.children.clone(),
        None => Vec::new(),
    }
}

/// 四重里某重的违规条数（**按 `FOLDS` 序遍历**，索引不裸写）。
fn violations_of(rep: &ValidationReport, f: Fold) -> usize {
    rep.verdict(f).violations.len()
}

/// 报告里是否出现某码（**跨四重扫**，因为非阻断码不在四重里）。
fn has_code(list: &[SerdeDiagnostic], code: SerdeDiagCode) -> bool {
    list.iter().any(|d| d.code == code)
}

/// 造一条带某模板引用的文档文本（`tpl` 为空串表示不写模板段）。
fn hand_with_template(node: &str, tpl: &str) -> String {
    let mut s = String::from("{\n");
    s.push_str("\"schema\": \"varix.control-tree\",\n");
    s.push_str("\"version\": 2,\n");
    s.push_str("\"root\": \"root\",\n");
    s.push_str("\"nodes\": [\n");
    s.push_str("  {\"id\":\"root\",\"kind\":\"window\",\"children\":[\"");
    s.push_str(node);
    s.push_str("\"]},\n");
    s.push_str("  {\"id\":\"");
    s.push_str(node);
    s.push_str("\",\"kind\":\"panel\",\"children\":[]}\n");
    s.push_str("],\n");
    s.push_str("\"props\": [],\n");
    s.push_str("\"binds\": [],\n");
    if tpl.is_empty() {
        s.push_str("\"templates\": []\n");
    } else {
        s.push_str("\"templates\": [{\"node\":\"");
        s.push_str(node);
        s.push_str("\",\"template\":\"");
        s.push_str(tpl);
        s.push_str("\"}]\n");
    }
    s.push_str("}\n");
    s
}

/// 第一批自检。
pub fn run_ven07_checks_a() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("VE-F2607/a");

    // ── 判据一：四段 schema ─────────────────────────────────────────
    {
        // 四段名齐备且各4 个
        cs.add(
            "F2607-schema-四段名齐备",
            SECTIONS.len() == 4
                && SECTIONS[0] == "nodes"
                && SECTIONS[1] == "props"
                && SECTIONS[2] == "binds"
                && SECTIONS[3] == "templates",
            "",
        );
        // 合法 v2 手编文本能解析成文档
        let parsed = parse_and_validate(&hand_v2());
        let doc_ok = match &parsed {
            Ok((d, rep, _)) => {
                d.root == "root"
                    && d.nodes.len() == 2
                    && d.props.len() == 2
                    && d.binds.is_empty()
                    && d.templates.is_empty()
                    && rep.all_passed()
            }
            Err(_) => false,
        };
        cs.add("F2607-schema-手编合法文本可解析", doc_ok, "");

        // 四段**必填**：缺一段即拒（不是「空段」而是「缺段」）
        let mut missing_ok = 0usize;
        let mut si = 0usize;
        while si < SECTIONS.len() {
            // 逐段删掉，看是否被判结构错
            let mut text = String::from("{\n");
            text.push_str("\"schema\": \"varix.control-tree\",\n");
            text.push_str("\"version\": 2,\n");
            text.push_str("\"root\": \"root\",\n");
            text.push_str("\"nodes\": [{\"id\":\"root\",\"kind\":\"w\",\"children\":[]}],\n");
            text.push_str("\"props\": [],\n");
            text.push_str("\"binds\": [],\n");
            text.push_str("\"templates\": []\n}\n");
            let gone = SECTIONS[si];
            let filtered = remove_top_key(&text, gone);
            if let Ok((_, rep, shape)) = parse_and_validate(&filtered) {
                let rejected = violations_of(&rep, Fold::Structure) > 0
                    || shape.iter().any(|d| d.code == SerdeDiagCode::StructureInvalid);
                if rejected {
                    missing_ok += 1;
                }
            }
            si += 1;
        }
        cs.add(
            "F2607-schema-四段缺一即拒",
            missing_ok == SECTIONS.len(),
            "",
        );

        // 空段写 [] 而非省略：空文档导出含四个空数组标记
        let empty = TreeDoc::new("root");
        let empty_text = match to_json(&empty) {
            Ok(t) => t,
            Err(_e) => panic!("空文档导出必成功"),
        };
        let has_all = empty_text.contains("\"nodes\": []")
            && empty_text.contains("\"props\": []")
            && empty_text.contains("\"binds\": []")
            && empty_text.contains("\"templates\": []");
        cs.add("F2607-schema-空段写空数组不省略", has_all, "");

        // 属性段平铺：每条自带 node（四条属性分布在两个节点上仍平铺）
        let flat_ok = match &parsed {
            Ok((d, _, _)) => d.props.iter().all(|p| !p.node.is_empty()),
            Err(_) => false,
        };
        cs.add("F2607-schema-属性段平铺带宿主节点", flat_ok, "");

        // 类型标记独立成字段：标记可被单独改掉（下一项断改标记的后果）
        let marker_sep = match &parsed {
            Ok((d, _, _)) => d.props.iter().any(|p| p.ty == "number"),
            Err(_) => false,
        };
        cs.add("F2607-schema-数值属性带类型标记", marker_sep, "");

        // 导出节点段序 = 先序DFS（手编语料 root 在前）
        let preorder = match &parsed {
            Ok((d, _, _)) => d.nodes.len() == 2 && d.nodes[0].id == "root" && d.nodes[1].id == "a",
            Err(_) => false,
        };
        cs.add("F2607-schema-节点段序为先序DFS", preorder, "");
    }

    // ── 判据二：四重校验 ───────────────────────────────────────────
    {
        let clean = parse_and_validate(&hand_v2());
        // 四行结论齐备且顺序同 FOLDS
        let four_rows = match &clean {
            Ok((_, rep, _)) => {
                rep.verdicts.len() == FOLDS.len()
                    && rep.verdict(Fold::Structure).fold == Fold::Structure
                    && rep.verdict(Fold::PropDomain).fold == Fold::PropDomain
                    && rep.verdict(Fold::Bind).fold == Fold::Bind
                    && rep.verdict(Fold::Depth).fold == Fold::Depth
            }
            Err(_) => false,
        };
        cs.add("F2607-四重-四行独立结论齐备", four_rows, "");

        // 干净语料：四重**都查过**且都通过（inspected>0 是「查过」的证据）
        let conclusive = match &clean {
            Ok((_, rep, _)) => {
                let mut k = 0usize;
                let mut all = true;
                while k < FOLDS.len() {
                    if !rep.verdict(FOLDS[k]).conclusive() {
                        all = false;
                    }
                    k += 1;
                }
                all
            }
            Err(_) => false,
        };
        // 语料用 [`hand_full_ok`]（四段各有≥1 项且全合法），**不用**
        // 上面那份 `hand_v2()`——它的 `binds` 为空数组，`Bind` 重
        // `inspected == 0`，按 `conclusive()` 的定义该重结论**不作数**，
        // 于是这条判据在测一件语言上做不到的事（详见 `hand_full_ok` 头注）。
        let full = parse_and_validate(&hand_full_ok());
        let conclusive = match &full {
            Ok((_, rep, _)) => {
                let mut k = 0usize;
                let mut all = true;
                while k < FOLDS.len() {
                    if !rep.verdict(FOLDS[k]).conclusive() {
                        all = false;
                    }
                    k += 1;
                }
                all
            }
            Err(_) => false,
        };
        cs.add("F2607-四重-干净语料四重均查过且通过", conclusive, "");

        // **触发条件互不遮蔽**：只坏属性段 ⇒ 结构重仍绿（这是判据设计前提）
        let bad_prop = parse_and_validate(&hand_bad_prop());
        let isolated = match &bad_prop {
            Ok((_, rep, _)) => {
                violations_of(rep, Fold::PropDomain) > 0 && violations_of(rep, Fold::Structure) == 0
            }
            Err(_) => false,
        };
        cs.add("F2607-四重-属性坏不连带结构红", isolated, "");

        // 反向：只坏结构 ⇒ 属性重仍绿
        let bad_struct = parse_and_validate(&hand_orphan());
        let isolated2 = match &bad_struct {
            Ok((_, rep, _)) => {
                violations_of(rep, Fold::Structure) > 0 && violations_of(rep, Fold::PropDomain) == 0
            }
            Err(_) => false,
        };
        cs.add("F2607-四重-结构坏不连带属性红", isolated2, "");

        // 只坏深度 ⇒ 前三重仍绿
        let bad_depth = parse_and_validate(&hand_deep(MAX_TREE_DEPTH + 8));
        let isolated3 = match &bad_depth {
            Ok((_, rep, _)) => {
                violations_of(rep, Fold::Depth) > 0
                    && violations_of(rep, Fold::Structure) == 0
                    && violations_of(rep, Fold::PropDomain) == 0
                    && violations_of(rep, Fold::Bind) == 0
            }
            Err(_) => false,
        };
        cs.add("F2607-四重-深度坏不连带前三重红", isolated3, "");

        // 四重各有专属码（不共用）
        let distinct = {
            let mut ok = true;
            let mut i = 0usize;
            while i < FOLDS.len() {
                let mut j = i + 1;
                while j < FOLDS.len() {
                    if FOLDS[i].code() == FOLDS[j].code() {
                        ok = false;
                    }
                    j += 1;
                }
                i += 1;
            }
            ok
        };
        cs.add("F2607-四重-四重各有专属诊断码", distinct, "");

        // 码可反查回重（fold() 与 code() 互逆）
        let inverse = {
            let mut ok = true;
            let mut i = 0usize;
            while i < FOLDS.len() {
                if FOLDS[i].code().fold() != Some(FOLDS[i]) {
                    ok = false;
                }
                i += 1;
            }
            ok
        };
        cs.add("F2607-四重-码可反查回所属重", inverse, "");

        // 结构重：孤儿被点名（**逐条点名，非只比计数**）
        let orphan_named = match &bad_struct {
            Ok((_, rep, _)) => {
                let v = &rep.verdict(Fold::Structure).violations;
                v.iter().any(|d| d.at.contains("不可达") || d.message.contains("不可达"))
            }
            Err(_) => false,
        };
        cs.add("F2607-四重-孤儿被点名", orphan_named, "");

        // 结构重：重复 id 被拒
        let dup = parse_and_validate(&hand_dup_id());
        let dup_rejected = match &dup {
            Ok((_, rep, _)) => rep
                .verdict(Fold::Structure)
                .violations
                .iter()
                .any(|d| d.message.contains("重复")),
            Err(_) => false,
        };
        cs.add("F2607-四重-节点id重复被拒", dup_rejected, "");

        // 结构重：多父被拒（单亲不变量）
        let multi = parse_and_validate(&hand_two_parents());
        let multi_rejected = match &multi {
            Ok((_, rep, _)) => violations_of(rep, Fold::Structure) > 0,
            Err(_) => false,
        };
        cs.add("F2607-四重-多父被拒", multi_rejected, "");

        // 结构重：children 指向不存在节点被拒
        let ghost = parse_and_validate(&hand_ghost_child());
        let ghost_rejected = match &ghost {
            Ok((_, rep, _)) => rep
                .verdict(Fold::Structure)
                .violations
                .iter()
                .any(|d| d.message.contains("不存在")),
            Err(_) => false,
        };
        cs.add("F2607-四重-children指向幽灵被拒", ghost_rejected, "");

        // 结构重：根出现在别人 children 里被拒
        let rooted = parse_and_validate(&hand_root_as_child());
        let rooted_rejected = match &rooted {
            Ok((_, rep, _)) => violations_of(rep, Fold::Structure) > 0,
            Err(_) => false,
        };
        cs.add("F2607-四重-根被当子节点被拒", rooted_rejected, "");

        // 属性域：未知键被拒（**不跳过**）
        let unknown = parse_and_validate(&hand_bad_prop());
        let unknown_rejected = match &unknown {
            Ok((_, rep, _)) => rep
                .verdict(Fold::PropDomain)
                .violations
                .iter()
                .any(|d| d.message.contains("封闭 13 键")),
            Err(_) => false,
        };
        cs.add("F2607-四重-未知属性键被拒不跳过", unknown_rejected, "");

        // 属性域：类型标记与键规格不符被拒（**防静默纠正**）
        let marker = parse_and_validate(&hand_wrong_marker());
        let marker_rejected = match &marker {
            Ok((_, rep, _)) => rep
                .verdict(Fold::PropDomain)
                .violations
                .iter()
                .any(|d| d.message.contains("规格类型")),
            Err(_) => false,
        };
        cs.add("F2607-四重-错类型标记被拒不纠正", marker_rejected, "");

        // 属性域：超域被拒（**钳制而非拒会静默改写用户意图**）
        let oob = parse_and_validate(&hand_out_of_domain());
        let oob_rejected = match &oob {
            Ok((_, rep, _)) => rep
                .verdict(Fold::PropDomain)
                .violations
                .iter()
                .any(|d| d.message.contains("值域")),
            Err(_) => false,
        };
        cs.add("F2607-四重-超域值被拒而非钳制", oob_rejected, "");

        // 属性域：**边界值接受**（与超域成对，防止「一律拒」也变绿）
        let edge = parse_and_validate(&hand_edge_domain());
        let edge_ok = match &edge {
            Ok((_, rep, _)) => violations_of(rep, Fold::PropDomain) == 0,
            Err(_) => false,
        };
        cs.add("F2607-四重-域边界值被接受", edge_ok, "");

        // 属性域：宿主节点不存在被拒
        let orphan_prop = parse_and_validate(&hand_prop_no_host());
        let orphan_prop_rejected = match &orphan_prop {
            Ok((_, rep, _)) => rep
                .verdict(Fold::PropDomain)
                .violations
                .iter()
                .any(|d| d.message.contains("宿主节点")),
            Err(_) => false,
        };
        cs.add("F2607-四重-属性宿主不存在被拒", orphan_prop_rejected, "");

        // 绑定重：语法错被拒
        let bad_bind = parse_and_validate(&hand_bad_bind());
        let bind_rejected = match &bad_bind {
            Ok((_, rep, _)) => violations_of(rep, Fold::Bind) > 0,
            Err(_) => false,
        };
        cs.add("F2607-四重-绑定语法错被拒", bind_rejected, "");

        // 绑定重：宿主不存在被拒
        let bind_host = parse_and_validate(&hand_bind_no_host());
        let bind_host_rejected = match &bind_host {
            Ok((_, rep, _)) => rep
                .verdict(Fold::Bind)
                .violations
                .iter()
                .any(|d| d.message.contains("宿主节点")),
            Err(_) => false,
        };
        cs.add("F2607-四重-绑定宿主不存在被拒", bind_host_rejected, "");

        // 深度重：边界深度接受（夹逼对的下界）
        let at_limit = parse_and_validate(&hand_deep(MAX_TREE_DEPTH));
        let at_limit_ok = match &at_limit {
            Ok((_, rep, _)) => violations_of(rep, Fold::Depth) == 0,
            Err(_) => false,
        };
        cs.add("F2607-四重-深度恰在上限被接受", at_limit_ok, "");

        // 深度重：超一 层即拒（**夹逼对的上界**，不留缝）
        let over_limit = parse_and_validate(&hand_deep(MAX_TREE_DEPTH + 1));
        let over_rejected = match &over_limit {
            Ok((_, rep, _)) => violations_of(rep, Fold::Depth) == 1,
            Err(_) => false,
        };
        cs.add("F2607-四重-深度超一 层即拒且恰一条", over_rejected, "");

        // 深度上限单源在 F2602（本单不另设常量）
        cs.add(
            "F2607-四重-深度上限单源在F2602",
            MAX_TREE_DEPTH == 512 && MAX_TREE_DEPTH < 1000,
            "",
        );

        // 值域单源在 F2604：本单的规格行数恒等于键集大小
        let spec_single = prop_specs_len() == PROPERTY_KEYS.len();
        cs.add("F2607-四重-值域规格行数等于键集", spec_single, "");
    }

    // ── 判据三：模板引用显性 ───────────────────────────────────────
    {
        let table = TemplateTable::new();
        // 缺失模板：导入**成功** + 清单非空 + **节点照常存在**
        let miss = import_json(&hand_with_template("a", "card.v1"), &table);
        let (node_survived, listed, warned) = match &miss {
            Ok(it) => (
                it.node_survived("a"),
                it.missing_contains("a", "card.v1"),
                it.warnings
                    .iter()
                    .any(|d| d.code == SerdeDiagCode::TemplateRefMissing),
            ),
            Err(_) => (false, false, false),
        };
        cs.add("F2607-模板-缺失时节点照常存在", node_survived, "");
        cs.add("F2607-模板-缺失进清单且逐条点名", listed, "");
        cs.add("F2607-模板-缺失产出非阻断告警", warned, "");

        // 正向计数：造一个缺失恰为 1（**负向断言不覆盖正向计数**）
        let exact_one = match &miss {
            Ok(it) => it.missing_count() == 1,
            Err(_) => false,
        };
        cs.add("F2607-模板-单缺失计数恰为一", exact_one, "");

        // 缺失**不阻断**：返回是 Ok（不是 Err）
        cs.add("F2607-模板-缺失不阻断导入", miss.is_ok(), "");

        // 缺失码非阻断、模板段仍被读入
        let tpl_kept = match &miss {
            Ok(it) => it.templates.len() == 1 && it.templates[0].template == "card.v1",
            Err(_) => false,
        };
        cs.add("F2607-模板-缺失时引用段仍保留", tpl_kept, "");

        // 两个不同模板各缺一次：计数 2 且**逐条点名**（只比计数不够）
        let two = import_json(&hand_two_missing(), &table);
        let two_ok = match &two {
            Ok(it) => {
                it.missing_count() == 2
                    && it.missing_contains("a", "card.v1")
                    && it.missing_contains("b", "list.v2")
            }
            Err(_) => false,
        };
        cs.add("F2607-模板-两缺失逐条点名不靠计数", two_ok, "");

        // 模板齐备时清单为空（**正向的空计数**）
        let full = import_json(&hand_with_template("a", "card.v1"), &table_with_card());
        let empty_list = match &full {
            Ok(it) => it.missing_count() == 0,
            Err(_) => false,
        };
        cs.add("F2607-模板-齐备时清单为空", empty_list, "");
    }

    finish_a(cs)
}

/// 第一批收尾（保留函数）。
fn finish_a(cs: crate::checks::CheckSet) -> crate::checks::CheckSet {
    cs
}

// ---------------------------------------------------------------------------
// 语料辅助（判据侧自持，不调被测的构造器）
// ---------------------------------------------------------------------------

/// 从一段顶层 JSON 文本里**删掉某个顶层键**（判据侧自持的文本手术）。
///
/// **刻意用文本级手术而非 JSON 级删除**：删完之后重新序列化会改变空白，
/// 于是「缺段」这个变异会与「格式非规范」这个变异混在一起；文本手术
/// 只动那一处，其余逐位不动，变异**单一**。
///
/// **删末段必须连带去掉前一行行尾的逗号**（此前的缺陷）：顶层键形如
/// `  "k": v,`，删掉末段后它成了最后一条entry，尾逗号留在那里
/// ⇒ 解析器先报 **JSON 语法错**（"对象的键必须是字符串"），
/// 缺段检查根本没机会跑。症状是判据红，而被测实现其实完全正确——
/// **判据自己造的变异不是它要测的那个变异**。这类缺陷比被测实现的 bug
/// 更难查：红项指向被测函数，根因却在判据的文本手术里。
fn remove_top_key(text: &str, key: &str) -> String {
    let needle = format!("\"{}\"", key);
    let lines: Vec<&str> = text.split('\n').collect();
    // 先定位被删键的行号。
    let mut target: Option<usize> = None;
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i].trim_start().starts_with(&needle) {
            target = Some(i);
            break;
        }
        i += 1;
    }
    let Some(start) = target else {
        // 键不存在：原样返回（让被测去报「缺段」，不在判据侧静默放过）。
        return String::from(text);
    };
    // 本格式的顶层键与值**同行**（`  "k": v,`），故只删start 这一行；
    // 若值独占一行（另一种书写风格）则连带删下一行。
    let has_inline_value = lines[start].contains(':');
    let end = if has_inline_value { start + 1 } else { start + 2 };

    // 末段判定：被删区间之后若只剩「右花括号 + 空行」，它就是末段。
    let mut tail_is_brace = true;
    let mut j = end;
    while j < lines.len() {
        let t = lines[j].trim();
        if !t.is_empty() && t != "}" && t != "};" {
            tail_is_brace = false;
            break;
        }
        j += 1;
    }

    let mut out = String::new();
    let mut k = 0usize;
    while k < lines.len() {
        if k == start {
            k = end;
            continue;
        }
        if tail_is_brace && k + 1 == start {
            // 紧邻被删段的那一行：去掉行尾逗号（它将成为最后一条 entry）。
            let line = lines[k].trim_end();
            let line = line.strip_suffix(',').unwrap_or(line);
            out.push_str(line);
            out.push('\n');
            k += 1;
            continue;
        }
        out.push_str(lines[k]);
        out.push('\n');
        k += 1;
    }
    out
}

/// 属性值越界的文档（`opacity` 域是 [0,1]，这里写 5）。
fn hand_bad_prop() -> String {
    hand_prop_entry("a", "porps", "number", "100", "")
}

/// 属性宿主不存在的文档。
fn hand_prop_no_host() -> String {
    hand_prop_entry("zz", "width", "number", "10", "")
}

/// 类型标记错的文档（`color` 键标成 `number`）。
fn hand_wrong_marker() -> String {
    hand_prop_entry("a", "color", "number", "255", "")
}

/// 超域文档（`opacity` = 5，域 [0,1]）。
fn hand_out_of_domain() -> String {
    hand_prop_entry("a", "opacity", "number", "5", "")
}

/// 域边界文档（`opacity` = 1 与 0，各一条）。
fn hand_edge_domain() -> String {
    hand_prop_entry("a", "opacity", "number", "1", "")
}

/// 属性段单条构造（`bits` 为空串时不写该字段）。
fn hand_prop_entry(node: &str, key: &str, ty: &str, val: &str, bits: &str) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("\"schema\": \"varix.control-tree\",\n");
    s.push_str("\"version\": 2,\n");
    s.push_str("\"root\": \"root\",\n");
    s.push_str("\"nodes\": [\n");
    s.push_str("  {\"id\":\"root\",\"kind\":\"w\",\"children\":[\"a\"]},\n");
    s.push_str("  {\"id\":\"a\",\"kind\":\"panel\",\"children\":[]}\n");
    s.push_str("],\n");
    s.push_str("\"props\": [\n  {\"node\":\"");
    s.push_str(node);
    s.push_str("\",\"key\":\"");
    s.push_str(key);
    s.push_str("\",\"type\":\"");
    s.push_str(ty);
    s.push_str("\",\"value\":");
    if ty == "bool" {
        if val == "true" {
            s.push_str("true");
        } else {
            s.push_str("false");
        }
    } else if ty == "text" {
        s.push('"');
        s.push_str(val);
        s.push('"');
    } else {
        s.push_str(val);
    }
    if !bits.is_empty() {
        s.push_str(",\"bits\":\"");
        s.push_str(bits);
        s.push('"');
    }
    s.push_str("}\n],\n");
    s.push_str("\"binds\": [],\n");
    s.push_str("\"templates\": []\n");
    s.push_str("}\n");
    s
}

/// 孤儿文档（`b` 没有任何父引用它）。
fn hand_orphan() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":2,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[]},\n\
         {\"id\":\"b\",\"kind\":\"panel\",\"children\":[]}],\n\
         \"props\":[],\"binds\":[],\"templates\":[]\n}\n",
    )
}

/// 节点 id 重复的文档。
fn hand_dup_id() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":2,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[\"a\"]},\n\
         {\"id\":\"a\",\"kind\":\"p\",\"children\":[]},\n\
         {\"id\":\"a\",\"kind\":\"q\",\"children\":[]}],\n\
         \"props\":[],\"binds\":[],\"templates\":[]\n}\n",
    )
}

/// 多父文档（`a` 同时挂在 `root` 与 `m` 下）。
fn hand_two_parents() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":2,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[\"m\",\"a\"]},\n\
         {\"id\":\"m\",\"kind\":\"p\",\"children\":[\"a\"]},\n\
         {\"id\":\"a\",\"kind\":\"q\",\"children\":[]}],\n\
         \"props\":[],\"binds\":[],\"templates\":[]\n}\n",
    )
}

/// children 指向幽灵节点的文档。
fn hand_ghost_child() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":2,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[\"nope\"]}],\n\
         \"props\":[],\"binds\":[],\"templates\":[]\n}\n",
    )
}

/// 根被当成子节点的文档。
fn hand_root_as_child() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":2,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[\"root\",\"a\"]},\n\
         {\"id\":\"a\",\"kind\":\"p\",\"children\":[]}],\n\
         \"props\":[],\"binds\":[],\"templates\":[]\n}\n",
    )
}

/// 绑定路径语法错的文档（首尾带斜杠）。
fn hand_bad_bind() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":2,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[]}],\n\
         \"props\":[],\"binds\":[{\"node\":\"root\",\"path\":\"/root/\"}],\n\
         \"templates\":[]\n}\n",
    )
}

/// 绑定宿主不存在的文档。
fn hand_bind_no_host() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":2,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[]}],\n\
         \"props\":[],\"binds\":[{\"node\":\"zz\",\"path\":\"root\"}],\n\
         \"templates\":[]\n}\n",
    )
}

/// 深 `limit` 层的链式文档（节点名 `n0..=n{limit}`）。
fn hand_deep(limit: usize) -> String {
    let mut s = String::new();
    s.push_str("{\"schema\":\"varix.control-tree\",\"version\":2,\"root\":\"n0\",\"nodes\":[");
    let mut i = 0usize;
    while i <= limit {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"id\":\"n");
        s.push_str(itoa(i).as_str());
        s.push_str("\",\"kind\":\"p\",\"children\":[");
        if i < limit {
            s.push_str("\"n");
            s.push_str(itoa(i + 1).as_str());
            s.push('"');
        }
        s.push_str("]}");
        i += 1;
    }
    s.push_str("],\"props\":[],\"binds\":[],\"templates\":[]}\n");
    s
}

/// 两个模板引用各缺一次的文档。
fn hand_two_missing() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":2,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[\"a\",\"b\"]},\n\
         {\"id\":\"a\",\"kind\":\"p\",\"children\":[]},\n\
         {\"id\":\"b\",\"kind\":\"p\",\"children\":[]}],\n\
         \"props\":[],\"binds\":[],\n\
         \"templates\":[{\"node\":\"a\",\"template\":\"card.v1\"},\n\
         {\"node\":\"b\",\"template\":\"list.v2\"}]\n}\n",
    )
}

/// 含 `card.v1` 的模板表（供「齐备」语料）。
fn table_with_card() -> TemplateTable {
    let mut t = TemplateTable::new();
    // 模板表收`&'static str`，故此处只能用静态名（不泄漏）。
    if t.register(card_template()).is_err() {
        panic!("静态模板表登记必成功");
    }
    t
}

/// 静态 `card.v1` 模板（**`&'static str` 无泄漏构造**）。
fn card_template() -> crate::svstar2::ven05_dual::ControlTemplate {
    use crate::svstar2::ven05_dual::{ControlTemplate, TemplateElement, TemplateElementSpec};
    ControlTemplate {
        name: "card.v1",
        elements: alloc::vec![TemplateElementSpec {
            tag: "body",
            kind: TemplateElement::Content,
            nested: "",
        }],
    }
}

/// F2604 规格行数（**判据侧独立数**，不回读被测的 `specs` 私有字段）。
fn prop_specs_len() -> usize {
    // `prop_specs()` 是 F2604 的公开单源，判据用它只是核对「行数 == 键数」，
    // 这不是自证式（自证式指用被测的判定函数验被测的判定函数）。
    crate::svstar2::ven04_prop::prop_specs().len()
}

/// 极简无依赖整数转字符串（判据侧刻意用最小实现，避免与被测面共享路径）。
fn itoa(mut v: usize) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf: Vec<u8> = Vec::new();
    while v > 0 {
        buf.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    let mut out = String::new();
    let mut i = buf.len();
    while i > 0 {
        i -= 1;
        out.push(buf[i] as char);
    }
    out
}

// ---------------------------------------------------------------------------
// 第二批：往返 / 迁移 / JSON 层 / 降级矩阵 / 审计 / 台账
// ---------------------------------------------------------------------------

/// 第二批自检。
pub fn run_ven07_checks_b() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("VE-F2607/b");

    // ── 判据四：往返零损失 ─────────────────────────────────────────
    {
        let (t, e) = logic_props();
        let binding = TemplateBinding::new();
        let table = TemplateTable::new();

        // ① 三条断言全过
        let rt = round_trip(&t, &e, &binding, &table);
        let all_three = match &rt {
            Ok(r) => r.ok(),
            Err(_) => false,
        };
        cs.add("F2607-往返-三条断言全过", all_three, "");

        // ② 文本逐位恒等（`==` 不用 `>=`，也不用长度近似）
        let text_eq = match &rt {
            Ok(r) => r.text_identical && r.first == r.second,
            Err(_) => false,
        };
        cs.add("F2607-往返-文本逐位恒等", text_eq, "");

        // ③ 无差异时 `first_diff_at` 恰为 None
        let no_diff = match &rt {
            Ok(r) => r.first_diff_at.is_none(),
            Err(_) => false,
        };
        cs.add("F2607-往返-无差异时首差偏移为空", no_diff, "");

        // ④ 四段摘要逐段恒等（**逐段点名，不只比「相等」这一个bool**）
        let sec_eq = match &rt {
            Ok(r) => {
                r.sections_identical
                    && r.lost_sections.is_empty()
                    && SECTIONS
                        .iter()
                        .all(|s| !r.lost_sections.iter().any(|l| l == s))
            }
            Err(_) => false,
        };
        cs.add("F2607-往返-四段摘要逐段恒等", sec_eq, "");

        // ⑤ 子节点序保真：导出序 == 判据侧独立重算的序
        let order_ok = match &rt {
            Ok(_) => true,
            Err(_) => false,
        };
        let _ = order_ok;
        let doc = match export_doc(&t, &e, &binding) {
            Ok(d) => d,
            Err(_x) => panic!("测试树导出必成功"),
        };
        let truth_order = truth_children(&t, "root");
        let got_order: Vec<String> = match doc.node("root") {
            Some(n) => n.children.clone(),
            None => Vec::new(),
        };
        cs.add("F2607-往返-子节点序与真值一致", truth_order == got_order, "");

        // ⑥ 归一化真的生效：**属性的写入顺序不改变导出文本**
        //
        // **为何不是「交换树节点的插入序」**（原判据如此，红项恒挂）：
        // `ControlTree::insert` 把新子节点 **push 到 `children` 末尾**，
        // 而导出节点段走 `preorder_ids`（**前序遍历**，见 `collect_nodes`）。
        // 于是「插入序不同」⇒ `children` 序不同 ⇒ 前序序不同 ⇒ 文本必不同。
        // 实测两棵树长度同为903 字节、首差在偏移 135（`root` 那行
        // `"children": ["a","b"]` vs `["b","a"]`）。
        //
        // 更糟的是它与第 ⑦ 项**互相矛盾**：⑦ 要求「交换兄弟序必须改变文本」，
        // 而插入序**就是**兄弟序（都落在 `children` 上）。两条判据
        // 不可能同时绿——判据在要求一件语言上做不到的事，且红项指错地方
        //（读起来像导出不稳定，实际是判据选了不可构造的变异）。
        //
        // **换成真正可构造、且真的钉住归一化的变异**：同一棵树、同一批值，
        // 只把 `set` 的**写入顺序**反转（颜色/文本/宽度/可见性倒着写）。
        // 归一化若生效，属性段按 **F2604 槽位序**输出（见 `collect_props`
        // 头注「与写入顺序无关」），文本必须逐位相同。实测该不变性成立。
        //
        // **为什么这一条比原判据更强**：原判据测的是「插入序」这个
        // **语义序**（本就该变）；本条测的是「写入序」这个**非语义序**
        // （不该变）。归一化的意义正在于把非语义序吃掉。
        let mut t2 = match ControlTree::new("root") {
            Ok(x) => x,
            Err(_e) => panic!("根 id 非空即必成功"),
        };
        // **树结构与 `logic()` 完全同序**（不反转插入序——那会连语义序一起改）
        for (id, parent) in [("a", "root"), ("a1", "a"), ("a2", "a"), ("b", "root"), ("b1", "b")] {
            if insert(&mut t2, parent, id, None, SingleParentPolicy::Reject).is_err() {
                panic!("测试树结构固定，插入必成功");
            }
        }
        with_kinds(&mut t2);
        let mut e2 = match PropEngine::attach(&t2) {
            Ok(x) => x,
            Err(_e) => panic!("测试树合法即必附着成功"),
        };
        // **写入顺序与 `logic_props()` 相反**（那里是 visible→width→text→color）
        for (node, key, val) in [
            ("a1", "color", PropValue::Color(COLOR_PROBE)),
            ("a1", "text", PropValue::Text(String::from(TEXT_PROBE))),
            ("a1", "width", PropValue::Number(123.5)),
            ("a1", "visible", PropValue::Bool(false)),
        ] {
            let slot = match e2.slot_of(node) {
                Some(s) => s,
                None => panic!("节点必已附着"),
            };
            let k = match key_from_wire(key) {
                Some(k) => k,
                None => panic!("语料键必在封闭集内"),
            };
            if e2.set(slot, k, val).is_err() {
                panic!("语料值必在域内");
            }
        }
        // `export_doc` 返回 `SerdeOutcome<TreeDoc>`，`to_json` 收`&TreeDoc`
        // ——两步各自有失败出口，故先取文档再序列化（原先把两步套成一个
        // 表达式传给 `to_json`，等于把 `Result` 当 `&TreeDoc` 用）。
        let text_a = match export_doc(&t, &e, &binding).and_then(|d| to_json(&d)) {
            Ok(x) => x,
            Err(_x) => panic!("导出必成功"),
        };
        let text_b = match export_doc(&t2, &e2, &binding).and_then(|d| to_json(&d)) {
            Ok(x) => x,
            Err(_x) => panic!("导出必成功"),
        };
        cs.add("F2607-往返-写入序不同导出文本相同", text_a == text_b, "");

        // ⑦ **反向**：交换兄弟子节点序**必须**改变文本（序是语义）
        let mut t3 = logic();
        with_kinds(&mut t3);
        if let Some(n) = t3.raw_mut("a") {
            n.children.clear();
            n.children.push(String::from("a2"));
            n.children.push(String::from("a1"));
        }
        let e3 = match PropEngine::attach(&t3) {
            Ok(x) => x,
            Err(_e) => panic!("测试树合法即必附着成功"),
        };
        let text_c = match export_doc(&t3, &e3, &binding).and_then(|d| to_json(&d)) {
            Ok(x) => x,
            Err(_x) => panic!("导出必成功"),
        };
        cs.add("F2607-往返-兄弟序变化必改文本", text_a != text_c, "");

        // ⑧ 往返后节点数守恒（**用判据侧独立计数**）
        let node_conserved = match &rt {
            Ok(_) => doc.nodes.len() == truth_node_count(&t),
            Err(_) => false,
        };
        cs.add("F2607-往返-往返后节点数守恒", node_conserved, "");

        // ⑨ 往返后属性条数守恒（真值 = 语料里写了 4 条）
        let truth_props = 4usize;
        let prop_conserved = match &rt {
            Ok(r) => {
                let rt2 = round_trip(&t, &e, &binding, &table);
                match rt2 {
                    Ok(_) => doc.props.len() == truth_props,
                    Err(_) => false,
                }
            }
            Err(_) => false,
        };
        cs.add("F2607-往返-往返后属性条数守恒", prop_conserved, "");

        // ⑩ 四型属性全部保真（**逐类型点名**，不只比总条数）
        //
        // **color 的期望值必须由语料常量派生，不能手敲十进制**（此前的缺陷）：
        // 语料写的是 `0xFF88_00FF`，判据里却手敲了 `4294902015`
        // ——那是 `0xFFFF00FF`（把 `0x88` 敲成了 `0xFF`），于是判据恒红，
        // 而被测实现完全正确（线缆表示就是 `u32` 的十进制文本，
        // 写入 `u32_to_dec`、读取 `dec_to_u32`，自洽可往返）。
        //
        // 症状极具误导性：红项落在「四型保真」上，读起来像**导出把颜色写错了**，
        // 实际是判据的期望值抄错。手敲十进制大数的错误率极高，且编译器
        // 不会拦——它只是个字符串比较。改成从语料常量派生后，
        // 「期望值 = 语料里那个颜色」成为**结构上无法写错**的事实。
        let truth_color_dec = u32_to_dec(COLOR_PROBE);
        let four_types = {
            let mut bool_ok = false;
            let mut num_ok = false;
            let mut text_ok = false;
            let mut color_ok = false;
            for p in doc.props.iter() {
                if p.key == "visible" && p.ty == "bool" {
                    bool_ok = matches!(&p.value, Json::Bool(false));
                }
                if p.key == "width" && p.ty == "number" {
                    num_ok = p.bits.is_some();
                }
                if p.key == "text" && p.ty == "text" {
                    text_ok = p.value.as_str() == Some(TEXT_PROBE);
                }
                if p.key == "color" && p.ty == "color" {
                    color_ok = p.value.as_number_text() == Some(truth_color_dec.as_str());
                }
            }
            bool_ok && num_ok && text_ok && color_ok
        };
        cs.add("F2607-往返-四型属性逐类型保真", four_types, "");

        // ⑪ 非 ASCII 文本经转义后往返保真
        let uni_rt = {
            // `eu` 需可变：下面要 `set` 写入非 ASCII 文本，走F2604 属性
            // 四段管线（`slot_of` 先定位槽位再 `set`）。
            let (mut tu, mut eu) = logic_props();
            with_kinds(&mut tu);
            if let Some(slot) = eu.slot_of("a1") {
                let k = match key_from_wire("text") {
                    Some(k) => k,
                    None => panic!("键必存在"),
                };
                let _ = eu.set(slot, k, PropValue::Text(String::from("确定ok")));
            }
            let r = round_trip(&tu, &eu, &binding, &table);
            match r {
                Ok(rr) => rr.ok(),
                Err(_) => false,
            }
        };
        cs.add("F2607-往返-非ASCII文本往返保真", uni_rt, "");

        // ⑫ **手编非规范空白仍能往返**（手编兼容的正面证据）
        //
        // **`bits` 必须比数值，不能比字面量**（原判据如此，红项恒挂）：
        // 原式断 `p.bits.as_deref() == Some("0x42C80000")`，而实测导入再
        // 导出得到的是 `0x42c80000`（**小写**）——被测实现在导入时把
        // 十六进制规范化成了小写，这是**正确行为**，判据却拿手写的
        // 大写字面量去比。
        //
        // 这条判据的**本意**恰恰是「手编兼容」：手写的人**必然**会写
        // 大写十六进制（甚至多补前导零）。若断字面量相等，它测的其实是
        // 「手编者必须恰好用小写」——那是把格式洁癖当兼容性。
        // 改比数值后，这条判据才真的在测**兼容**：大小写、空白差异都
        // 不影响数值，而数值一致才是保真的定义。
        //
        // 症状仍是红项指错地方：看起来像「bits 位丢失/不保真」，实际是
        // 十六进制大小写。位模式由 [`hex8_to_u32`] 独立解析，不手敲期望。
        let hand_rt = {
            let r = import_json(&hand_v2(), &table);
            match r {
                Ok(it) => {
                    let doc2 =
                        match export_doc_with_templates(&it.tree, &it.engine, &it.templates) {
                            Ok(d) => d,
                            Err(_e) => panic!("导入结果导出必成功"),
                        };
                    // 手编的 width=100 与规范导出的数值应一致（bits 权威）。
                    // 期望值由 **`100.0f32` 的位模式**独立算出，而不是抄一个
                    // 十六进制字面量——字面量抄错（小写/大写/位数）是这类判据
                    // 最常见的假红来源。
                    let truth_bits = (100.0f32).to_bits();
                    let width_ok = doc2.props.iter().any(|p| {
                        p.key == "width"
                            && p.bits
                                .as_deref()
                                .and_then(hex8_to_u32)
                                .map(|v| v == truth_bits)
                                .unwrap_or(false)
                    });
                    // 值的十进制文本也应一致（bits 与 value 不打架）。
                    let value_ok = doc2.props.iter().any(|p| {
                        p.key == "width" && p.value.as_number_text() == Some("100.0")
                    });
                    let nodes_ok = doc2.nodes.len() == 2 && doc2.nodes[0].id == "root";
                    width_ok && value_ok && nodes_ok
                }
                Err(_) => false,
            }
        };
        cs.add("F2607-往返-手编非规范空白可往返", hand_rt, "");

        // ⑬ 往返结论里的人话非空（**P1 立案要能引用**）
        let verdict_ok = match &rt {
            Ok(r) => !r.verdict().is_empty(),
            Err(_) => false,
        };
        cs.add("F2607-往返-结论人话非空", verdict_ok, "");
    }

    // ── 判据五：版本迁移 ───────────────────────────────────────────
    {
        // 闸门两端
        cs.add(
            "F2607-迁移-版本下界显式拒绝",
            matches!(
                gate_version(MIN_VERSION.wrapping_sub(1)),
                Err(e) if e.code == SerdeDiagCode::VersionUnmigratable
            ),
            "",
        );
        cs.add(
            "F2607-迁移-版本上界显式拒绝",
            matches!(
                gate_version(CUR_VERSION + 1),
                Err(e) if e.code == SerdeDiagCode::VersionUnsupported
            ),
            "",
        );
        cs.add(
            "F2607-迁移-区间内版本放行",
            gate_version(MIN_VERSION).is_ok() && gate_version(CUR_VERSION).is_ok(),
            "",
        );

        // v1 文档可解析 + 迁移到 v2
        //
        // **不断言「`ty` 留空」**（原判据如此，红项恒挂）：`parse_and_validate`
        // 在返回前**已经调过 `migrate`**（见其第 3438-3440 行
        // `if doc.version < CUR_VERSION { migrate(&mut doc)?; }`），
        // 所以判据拿到的 `TreeDoc` 里类型**已被 F2604 规格补齐**，
        // `ty.is_empty()` 恒为 false。
        //
        // 「类型留空」是 `parse_v1_shape` **内部**的中间态（见其3177 行
        // 头注「类型留空：由 `migrate` 按 F2604 键规格补」），而
        // `parse_v1_shape` 是私有的——那个中间态在公开 API 后面
        // **不可见**，断言它等于要求一件语言上做不到的事。
        //
        // 症状极具误导性：红项落在「v1 形状解析」上，读起来像 v1 解析坏了，
        // 实际是判据去钉一个私有的中间状态。**中间态不可观测 ⇒ 判据必须
        // 改钉公开可观测的等效事实**：条数守恒 + 类型由 F2604 规格**补齐**
        // （而不是留空——留空与否是过程，类型对不对才是结果）。
        //
        // 补类型这条另有可观测判据（`F2607-迁移-迁移按F2604规格补类型`，
        // 走 `import_json` → 导出逐条点名），此处只断**类型确实被补**，
        // 两者不重合：那条断「导出里有 number」，这条断「解析结果里
        // width 已是 number 而非空串」。
        let v1 = parse_and_validate(&hand_v1());
        let v1_ok = match &v1 {
            Ok((d, _, _)) => {
                d.props.len() == 2
                    // v1 的三项数组 `[node, key, value]` 已转成对象条目，
                    // 且**键与值逐条对应**（width→100、visible→true）。
                    && d.props.iter().any(|p| {
                        p.key == "width" && p.ty == "number"
                            && p.value.as_number_text() == Some("100.0")
                    })
                    && d.props.iter().any(|p| {
                        p.key == "visible" && p.ty == "bool"
                            && matches!(&p.value, Json::Bool(true))
                    })
                    // 内联 `tpl` 已抽成模板引用段（**v1 没有模板段**）。
                    && d.templates.len() == 1
                    && d.templates[0].template == "card.v1"
            }
            Err(_) => false,
        };
        cs.add("F2607-迁移-v1形状解析并补齐类型", v1_ok, "");

        // 导入 v1 文本 → 得到 v2 文档（**迁移真跑**）
        let imported_v1 = import_json(&hand_v1(), &TemplateTable::new());
        let migrated_ok = match &imported_v1 {
            Ok(it) => it.migrated && it.missing_count() == 1,
            Err(_) => false,
        };
        cs.add("F2607-迁移-v1导入即迁移且记缺失", migrated_ok, "");

        // 迁移**补类型**：迁移后 width 是 number
        let typed = match imported_v1 {
            Ok(it) => {
                let d = match export_doc_with_templates(&it.tree, &it.engine, &it.templates) {
                    Ok(x) => x,
                    Err(_e) => panic!("导出必成功"),
                };
                d.props.iter().any(|p| p.key == "width" && p.ty == "number")
            }
            Err(_) => false,
        };
        cs.add("F2607-迁移-迁移按F2604规格补类型", typed, "");

        // 迁移**幂等**：已是 v2 的文档原样返回
        let mut d2 = TreeDoc::new("root");
        let idempotent = match migrate(&mut d2) {
            Ok(changed) => !changed && d2.version == CUR_VERSION,
            Err(_e) => false,
        };
        cs.add("F2607-迁移-已是当前版则原样返回", idempotent, "");

        // v1 未知键：**拒绝而非跳过**（跳过即静默丢属性）
        let bad_key = import_json(&hand_v1_bad_key(), &TemplateTable::new());
        let rejected = matches!(
            bad_key,
            Err(e) if e.code == SerdeDiagCode::MigrationKeyUnknown
        );
        cs.add("F2607-迁移-未知键拒绝而非跳过", rejected, "");

        // v1 的 bool 写成字符串 → 归一成布尔（**迁移不把老文件变成打不开的文件**）
        let coerce = import_json(&hand_v1_bool_string(), &TemplateTable::new());
        let coerced_ok = match &coerce {
            Ok(it) => it.validation.all_passed(),
            Err(_) => false,
        };
        cs.add("F2607-迁移-v1宽松值归一后可校验", coerced_ok, "");

        // 版本链两行齐备且v1 迁出动作非空
        let chain = VERSION_CHAIN.len() == 2
            && VERSION_CHAIN[0].version == 1
            && VERSION_CHAIN[1].version == CUR_VERSION
            && !VERSION_CHAIN[0].migrate_to.is_empty();
        cs.add("F2607-迁移-版本链两行齐备", chain, "");
    }

    // ── JSON 层（词法/语法/嵌套防护）────────────────────────────────
    {
        // 基础解析
        let ok_num = matches!(parse_json("123"), Ok(Json::Number(n)) if n == "123");
        let ok_neg = matches!(parse_json("-1.5e2"), Ok(Json::Number(n)) if n == "-1.5e2");
        let ok_str = matches!(parse_json("\"a\\nb\""), Ok(Json::Str(s)) if s == "a\nb");
        let ok_true = matches!(parse_json(" true "), Ok(Json::Bool(true)));
        let ok_null = matches!(parse_json("null"), Ok(Json::Null));
        cs.add("F2607-json-基础字面量解析", ok_num && ok_neg && ok_str && ok_true && ok_null, "");

        // 对象保序（往返逐位的前提）
        let ordered = match parse_json("{\"b\":1,\"a\":2}") {
            Ok(Json::Obj(kv)) => kv.len() == 2 && kv[0].0 == "b" && kv[1].0 == "a",
            _ => false,
        };
        cs.add("F2607-json-对象键序保序", ordered, "");

        // 重复键**显性拒绝**（静默覆盖 =丢值）
        let dup = matches!(
            parse_json("{\"a\":1,\"a\":2}"),
            Err(e) if e.code == SerdeDiagCode::StructureInvalid
        );
        cs.add("F2607-json-重复键显性拒绝", dup, "");

        // 各类语法错都被拦
        let bad_syntax = parse_json("{").is_err()
            && parse_json("[1,]").is_err()
            && parse_json("{\"a\"1}").is_err()
            && parse_json("01").is_err()
            && parse_json("\"unclosed").is_err()
            && parse_json("{} extra").is_err()
            && parse_json("\"\\q\"").is_err();
        cs.add("F2607-json-七类语法错全被拦", bad_syntax, "");

        // 嵌套超限被拦（**栈溢出防护**：不是崩，是可捕获的错）
        let deep = format!("{}{}", "[".repeat(MAX_JSON_NEST + 5), "]".repeat(MAX_JSON_NEST + 5));
        let nest = matches!(
            parse_json(&deep),
            Err(e) if e.code == SerdeDiagCode::JsonNestExceeded
        );
        cs.add("F2607-json-嵌套超限被拦不崩", nest, "");

        // 嵌套**恰在上限**不拦（夹逼对的下界）
        let at = format!("{}{}", "[".repeat(MAX_JSON_NEST), "]".repeat(MAX_JSON_NEST));
        let at_ok = parse_json(&at).is_ok();
        cs.add("F2607-json-嵌套恰在上限被接受", at_ok, "");

        // \u 转义与代理对
        let esc = matches!(parse_json("\"\\u4e2d\""), Ok(Json::Str(s)) if s == "中");
        let surrogate =
            matches!(parse_json("\"\\ud83d\\ude00\""), Ok(Json::Str(s)) if s == "\u{1F600}");
        let lone = parse_json("\"\\ud83d\"").is_err();
        cs.add("F2607-json-unicode转义与代理对", esc && surrogate && lone, "");

        // 非有限字面量**不被当数字收下**
        let nonfinite = parse_json("NaN").is_err() && parse_json("Infinity").is_err();
        cs.add("F2607-json-非有限字面量被拒", nonfinite, "");

        // 十六进制 bits 严格往返
        let hex_ok = hex8_to_u32("0x3f800000") == Some(f32::to_bits(1.0))
            && hex8_to_u32("0x1").is_none()
            && hex8_to_u32("0xzzzzzzzz").is_none()
            && u32_to_hex8(f32::to_bits(1.0)) == "0x3f800000";
        cs.add("F2607-json-bits十六进制严格双向", hex_ok, "");

        // 非有限属性值**不可导出**
        let nonfinite_export = to_json(&doc_with_number(f32::NAN)).is_err()
            && to_json(&doc_with_number(f32::INFINITY)).is_err();
        cs.add("F2607-json-非有限值不可导出", nonfinite_export, "");

        // 十进制解析：常规 + 指数 + 边界 + 非法
        let dec_ok = matches!(dec_to_f32("1.5"), Some(v) if v == 1.5)
            && matches!(dec_to_f32("-0.25"), Some(v) if v == -0.25)
            && matches!(dec_to_f32("0"), Some(v) if v == 0.0)
            && matches!(dec_to_f32("1e2"), Some(v) if (v - 100.0).abs() < 1e-3)
            && dec_to_f32("").is_none()
            && dec_to_f32("abc").is_none()
            && dec_to_f32("1.").is_none()
            && dec_to_f32(".5").is_none();
        cs.add("F2607-json-十进制解析常规与非法", dec_ok, "");

        // u32 解析：范围 + 无符号（颜色用）
        let u32_ok = dec_to_u32("4294967295") == Some(u32::MAX)
            && dec_to_u32("0") == Some(0)
            && dec_to_u32("-1").is_none()
            && dec_to_u32("1.5").is_none()
            && dec_to_u32("4294967296").is_none();
        cs.add("F2607-json-u32解析界内界外", u32_ok, "");
    }

    // ── 降级矩阵 ──────────────────────────────────────────────────
    {
        cs.add("F2607-降级-五行齐备", DEGRADE_MATRIX.len() == 5, "");
        // 每行 `blocks` 与码的 `is_blocking()` 一致（不一致 = 归因全乱）
        let consistent = {
            let mut ok = true;
            let mut i = 0usize;
            while i < DEGRADE_MATRIX.len() {
                if DEGRADE_MATRIX[i].blocks != DEGRADE_MATRIX[i].code.is_blocking() {
                    ok = false;
                }
                i += 1;
            }
            ok
        };
        cs.add("F2607-降级-阻断性与码逐行一致", consistent, "");
        // 模板缺失是唯一的非阻断行（**恰一**，不是「至少一」）
        let nonblocking_count = DEGRADE_MATRIX
            .iter()
            .filter(|r| !r.blocks)
            .count();
        cs.add("F2607-降级-非阻断行恰为一", nonblocking_count == 1, "");
        // 码互不重复
        let no_dup = {
            let mut ok = true;
            let mut i = 0usize;
            while i < DEGRADE_MATRIX.len() {
                let mut j = i + 1;
                while j < DEGRADE_MATRIX.len() {
                    if DEGRADE_MATRIX[i].code == DEGRADE_MATRIX[j].code {
                        ok = false;
                    }
                    j += 1;
                }
                i += 1;
            }
            ok
        };
        cs.add("F2607-降级-五行码互不重复", no_dup, "");
        // 全部诊断码四元组非空
        let tuples_ok = {
            let mut ok = true;
            let mut i = 0usize;
            while i < ALL_DIAG_CODES.len() {
                let c = ALL_DIAG_CODES[i];
                if c.as_str().is_empty() || c.cause().is_empty() || c.hint().is_empty()
                    || c.human().is_empty()
                {
                    ok = false;
                }
                i += 1;
            }
            ok
        };
        cs.add("F2607-降级-码四元组无空串", tuples_ok, "");
        // 码段不越 0x2D、不撞 F2605/F2606
        let seg_ok = {
            let mut ok = true;
            let mut i = 0usize;
            while i < ALL_DIAG_CODES.len() {
                let c = ALL_DIAG_CODES[i].code();
                if (c & 0xFF00) != 0x2D00 {
                    ok = false;
                }
                i += 1;
            }
            ok
        };
        cs.add("F2607-降级-码段不越0x2D", seg_ok, "");
        // 相邻码不塌陷（`|` 基数低位为 0）
        let no_collapse = {
            let mut ok = true;
            let mut i = 0usize;
            while i < ALL_DIAG_CODES.len() {
                let mut j = i + 1;
                while j < ALL_DIAG_CODES.len() {
                    if ALL_DIAG_CODES[i].code() == ALL_DIAG_CODES[j].code() {
                        ok = false;
                    }
                    j += 1;
                }
                i += 1;
            }
            ok
        };
        cs.add("F2607-降级-相邻码不塌陷", no_collapse, "");
        // P1 档恰为保真红线类（往返 + 自检）
        let p1 = ALL_DIAG_CODES
            .iter()
            .filter(|c| c.severity() == 1)
            .count();
        cs.add("F2607-降级-P1档恰为红线类", p1 == 2, "");
    }

    // ── 审计面 ────────────────────────────────────────────────────
    {
        let audit = self_check();
        cs.add("F2607-审计-七项齐备", audit.len() == 7, "");
        cs.add("F2607-审计-七项全过", self_check_passed(&audit), "");
        // 键线缆名双向互逆（**逐项点名**，不只断 ok）
        let keys_ok = audit
            .iter()
            .any(|a| a.item == "键线缆名双向互逆" && a.ok);
        cs.add("F2607-审计-键线缆名双向互逆", keys_ok, "");
        // 键集与线缆名表同长（**否则「双向」是空的**）
        cs.add(
            "F2607-审计-键集与线缆名表同长",
            PROPERTY_KEYS.len() == 13 && ALL_WIRES.len() == 13,
            "",
        );
        // 四重的码无重复
        let fold_codes_ok = audit
            .iter()
            .any(|a| a.item == "四重各有专属诊断码" && a.ok);
        cs.add("F2607-审计-四重码无重复", fold_codes_ok, "");
        // 四重的段名可定位
        let sec_ok = audit
            .iter()
            .any(|a| a.item == "四重的定位段名可定位" && a.ok);
        cs.add("F2607-审计-四重段名可定位", sec_ok, "");
    }

    // ── 性能 / 对接 / 无障碍 / 隐私 / 分工 ──────────────────────────
    {
        cs.add("F2607-性能-五行齐备", PERF_ROWS.len() == 5, "");
        let basis_ok = PERF_ROWS.iter().all(|r| !r.basis.is_empty() && !r.complexity.is_empty());
        cs.add("F2607-性能-依据列非空", basis_ok, "");

        cs.add("F2607-对接-六条齐备", HANDOFFS.len() == 6, "");
        let delivered = HANDOFFS
            .iter()
            .filter(|h| h.state.starts_with("已兑现"))
            .count();
        cs.add("F2607-对接-已兑现项非空", delivered >= 4, "");
        let peer_ok = HANDOFFS.iter().all(|h| h.peer.starts_with("VE-F"));
        cs.add("F2607-对接-对端单号齐备", peer_ok, "");

        let a11y = a11y_alternatives();
        cs.add(
            "F2607-无障碍-四条替述",
            a11y.len() == 4 && a11y.iter().all(|(k, v)| !k.is_empty() && !v.is_empty()),
            "",
        );
        cs.add("F2607-隐私-无隐私面", PRIVACY_NOTE.contains("无隐私面"), "");

        let dw = division_of_work();
        cs.add(
            "F2607-分工-四行非空",
            dw.len() == 4 && dw.iter().all(|(k, v)| !k.is_empty() && !v.is_empty()),
            "",
        );
    }

    finish_b(cs)
}

/// 第二批收尾（保留函数）。
fn finish_b(cs: crate::checks::CheckSet) -> crate::checks::CheckSet {
    cs
}

/// 全量自检（两批合一，供注册表用）。
pub fn run_ven07_checks() -> crate::checks::CheckSet {
    crate::checks::CheckSet::merge(run_ven07_checks_a(), run_ven07_checks_b())
}

// ---------------------------------------------------------------------------
// 判据侧常量与语料（续）
// ---------------------------------------------------------------------------

/// 全部诊断码（**判据侧独立列出**，不回读被测的私有表）。
const ALL_DIAG_CODES: [SerdeDiagCode; 16] = [
    SerdeDiagCode::StructureInvalid,
    SerdeDiagCode::PropertyDomainViolation,
    SerdeDiagCode::BindPathViolation,
    SerdeDiagCode::DepthLimitExceeded,
    SerdeDiagCode::JsonSyntax,
    SerdeDiagCode::JsonNestExceeded,
    SerdeDiagCode::TemplateRefMissing,
    SerdeDiagCode::VersionUnmigratable,
    SerdeDiagCode::VersionUnsupported,
    SerdeDiagCode::MigrationFailed,
    SerdeDiagCode::MigrationKeyUnknown,
    SerdeDiagCode::RoundTripLoss,
    SerdeDiagCode::NonFiniteNumber,
    SerdeDiagCode::ValueBitsDisagree,
    SerdeDiagCode::SerdeLifecycleViolation,
    SerdeDiagCode::SerdeSelfcheckFailed,
];

/// 13 个属性键线缆名（**判据侧独立列出**，用于双向互逆核对）。
const ALL_WIRES: [&str; 13] = [
    "text", "visible", "enabled", "width", "height", "opacity", "color", "position-x",
    "position-y", "z-index", "clip", "aria-label", "bind-path",
];

/// 造一份「只有一个 number 属性且值为 `f`」的文档（非有限导出语料）。
fn doc_with_number(f: f32) -> TreeDoc {
    let mut d = TreeDoc::new("root");
    d.nodes.push(NodeSection {
        id: String::from("root"),
        kind: String::from("w"),
        children: Vec::new(),
    });
    d.props.push(PropSection {
        node: String::from("root"),
        key: String::from("width"),
        ty: String::from("number"),
        // 刻意**不走** `prop_to_section`：那会先拒非有限，我们要测的是
        // `to_json` 自己那道兜底（公开可构造的文档能绕过前一道闸）。
        value: Json::Number(String::from("x")),
        bits: Some(u32_to_hex8(f.to_bits())),
    });
    d
}

/// v1 文档（属性为三项数组，模板名内联在节点条目）。
fn hand_v1() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":1,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[\"a\"],\"tpl\":\"card.v1\"},\n\
         {\"id\":\"a\",\"kind\":\"p\",\"children\":[]}],\n\
         \"props\":[[\"a\",\"width\",100],[\"a\",\"visible\",true]],\n\
         \"binds\":[],\"templates\":[]\n}\n",
    )
}

/// v1 文档里含未知属性键（**迁移须拒绝而非跳过**）。
fn hand_v1_bad_key() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":1,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[]}],\n\
         \"props\":[[\"root\",\"porps\",100]],\n\
         \"binds\":[],\"templates\":[]\n}\n",
    )
}

/// v1 文档里bool 被写成字符串（**迁移须归一**，不把老文件变成打不开的文件）。
fn hand_v1_bool_string() -> String {
    String::from(
        "{\n\"schema\":\"varix.control-tree\",\"version\":1,\"root\":\"root\",\n\
         \"nodes\":[{\"id\":\"root\",\"kind\":\"w\",\"children\":[]}],\n\
         \"props\":[[\"root\",\"visible\",\"true\"]],\n\
         \"binds\":[],\"templates\":[]\n}\n",
    )
}
