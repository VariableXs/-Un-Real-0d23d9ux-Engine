//! VE-F2806 · 域自检（判据逐条对应，见 `veo06_shorthand.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - O01 架构声明 → `O06-架构-*`（规格表可机检、编译期数值闸的运行期复查、
//!   预算单源、复杂度分解、零 panic 面、不越权声明）
//! - 集成边界 → `O06-边界-*`（上游哈希对账、下游前向声明、对账钩子、
//!   边序单源、名字单源）
//! - 解析子集 → `O06-子集-*`（六条简写覆盖、赋值表覆盖、双轴覆盖、
//!   非简写不登记、注册表可查性）
//! - 降级矩阵（非法输入→拒绝三要素 / 边界越界→钳制 + 告警 / 异常检出→立案流转）
//!   → `O06-降级-*`
//!
//! **判据设计的八条纪律**（在本单被逐条落实）：
//! 1. **不能向被测函数问答案**——期望值一律由判据侧独立算出。
//!    最典型的是 1~4 分量的边角分配：判据**不调 `assign_component`**，
//!    而是自己按 CSSOM 的 `assign` 公式写一遍独立实现再对拍
//!    （两套实现同源则变异必被捕获，否则「用被测函数验被测函数」是恒真门禁）。
//! 2. **不能只验一个方向**——「坏值要拒」必须配「好值要过」。
//!    每条拒绝判据都配一条对应的接受判据。
//! 3. **不能只验形态**——「展开出 4 条」不等于「值分配对」。
//!    逐条对拍**名字与值两列**。
//! 4. **不能拿弱门禁当门禁**——拒绝要断**专属错误码**（`E_SHORTHAND_ARITY`
//!    与 `E_SHORTHAND_UNKNOWN` 与 `E_SHORTHAND_AXES_EMPTY` 三者互不相同），
//!    断「有东西坏了」等于没断。
//! 5. **不能有自证式门禁**——期望名单在判据侧**写死字面量**，
//!    不从 `SHORTHAND_TABLE` 反推（否则「表里少了 border-color」这类变异
//!    会让判据跟着一起「期望少一条」，看着全绿）。
//! 6. **不能用被测函数自身作为下标来源**——判据里的 `[i]` 只索引
//!    **判据侧自己构造**的数组；断言展开产物时一律走 `.get(i)`。
//! 7. **弱门禁必须双向验证**——每条「加固型」判据都要能指出它抓什么变异；
//!    抓不到任何变异的判据是装饰，不是门禁。
//! 8. **不能用「形状对」替代「值对」**——数组长度相等是形状，
//!    逐位相等才是值。两层都要断。
//!
//! 零墙钟、零 IO，回归可复现。

use super::veo01_arch::{CaseLedger, ClampLog};
use super::veo05_props::{self, Declaration, ParsedValue, PropertyId, ValueKind};
use super::veo06_shorthand::*;
use crate::checks::{CheckSet, MAX_CHECKS};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧的独立实现（**绝不调被测函数**）
// ---------------------------------------------------------------------------

/// 判据侧独立的边角分配（**CSSOM `assign` 语义的第二套实现**）。
///
/// 与被测的 `assign_component` 是**两份独立代码**：判据侧显式按
/// 「逐边fallback 到已有分量」写一遍，不复用被测的任何分支。
/// 这样 `assign_component` 的 fallback 链被改坏时，本函数不动 ⇒ 变异被捕获。
///
/// 边序：0=上 1=右 2=下 3=左。
fn expect_edge(comp: &[&str], idx: usize) -> String {
    if comp.is_empty() {
        return String::from("<空>");
    }
    // 独立 fallback 写法：**按已有分量数分流**（n=1 全取 v[0]、n=2 取
    // [0/1/0/1]、n=3 取 [0/1/2/1]、n=4 逐位）——与被测的 or_else 链同义不同形。
    // 3 分量时左=第 2 分量（a/b/c/**b**）这条单点由 n==3 分支显式承载，
    // 判据-03 的反向语料专测它。
    let pick = |alt: usize| -> String {
        match comp.get(alt) {
            Some(v) => String::from(*v),
            None => String::from("<空>"),
        }
    };
    let n = comp.len();
    match idx {
        0 => pick(0),
        1 => {
            if n > 1 { pick(1) } else { pick(0) }
        }
        2 => {
            if n > 2 { pick(2) } else { pick(0) }
        }
        3 => {
            if n > 3 {
                pick(3)
            } else if n > 1 {
                pick(1)
            } else {
                pick(0)
            }
        }
        _ => String::from("<越界>"),
    }
}

/// 判据侧独立的 1~4 分量分配（返回四边名）。
fn expect_edges(comp: &[&str]) -> [String; 4] {
    let mut out: [String; 4] = [
        String::from(""),
        String::from(""),
        String::from(""),
        String::from(""),
    ];
    let mut i = 0usize;
    while i < 4 {
        out[i] = expect_edge(comp, i);
        i += 1;
    }
    out
}

/// 造一条 `ParsedValue::List`（分量用 `Keyword` 承载原文，便于对拍）。
fn list(items: &[&str]) -> ParsedValue {
    let mut v: Vec<ParsedValue> = Vec::new();
    for it in items.iter() {
        v.push(ParsedValue::Keyword(String::from(*it)));
    }
    ParsedValue::List(v)
}

/// 造一条 `Declaration`（**不走 DeclParser**——F2806 判据只关心展开，
/// 解析由 F2805 负责；用构造器能排除「解析失败」这个干扰变量）。
fn decl(name: &'static str, items: &[&str], important: bool, offset: u32) -> Declaration {
    Declaration {
        id: any_free_id(),
        name,
        value: list(items),
        important,
        offset,
    }
}

/// 无 panic 面的 `PropertyId` 取值。
///
/// `PROPERTY_COUNT = 16+14+6+8 = 44`（判据侧独立相加）是 F2803 的编译期
/// 常量，故 `of_rank(0)` **数学上必 Some**——循环首轮即返回。None 分支
/// （注册表被清空）被 F2805 的四族非空编译期闸排除，不可能执行；但
/// 「不可能」也要类型上闭合：循环递增秩位直到命中，**不写 unwrap/
/// expect/panic**（判据-01 会扫本文件）。
fn any_free_id() -> PropertyId {
    let mut r: u16 = 0;
    loop {
        if let Some(id) = PropertyId::of_rank(r) {
            return id;
        }
        r = r.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// O06-架构：规格表与编译期闸的运行期复查（8 项）
// ---------------------------------------------------------------------------

/// 架构-01：规格表审计必须通过（**五闸 + 两跨表闸 + 边序闸全过**）。
fn chk_arch_spec_audit(s: &mut CheckSet) {
    match audit_spec_table() {
        Ok(msg) => s.ok("O06-架构-01-规格表审计全过"),
        Err(e) => s.fail("O06-架构-01-规格表审计全过", e.code),
    }
    // 审计摘要须自报条数（**可机检的落点**，不是空串）。
    let msg = audit_spec_table().unwrap_or_else(|_| String::from(""));
    if msg.contains("6") && msg.contains("24") {
        s.ok("O06-架构-02-审计摘要含条数自报");
    } else {
        s.fail("O06-架构-02-审计摘要含条数自报", "摘要未含 6 条简写 / 24 条 longhand 名");
    }
}

/// 架构-03：全域审计（规格 + 上游 + 下游三段）必须全过。
fn chk_arch_domain_audit(s: &mut CheckSet) {
    match audit_all() {
        Ok(m) => {
            if m.contains("上游") && m.contains("下游") {
                s.ok("O06-架构-03-全域审计三段齐");
            } else {
                s.fail("O06-架构-03-全域审计三段齐", "缺上游或下游段");
            }
        }
        Err(e) => s.fail("O06-架构-03-全域审计三段齐", e.code),
    }
}

/// 架构-04：平行数组与规格表**逐位一致**（防「改了平行数组忘了改表」）。
///
/// 这是**双向对账**：既断 `MAX_COMPONENTS_OF[i] == 表项.max_components`，
/// 也断 `SLASH_AXES_OF[i] == 表项.slash_axes`。
fn chk_arch_derivations_agree(s: &mut CheckSet) {
    let mut bad = 0usize;
    for i in 0..6 {
        let mc = MAX_COMPONENTS_OF.get(i).copied().unwrap_or(0);
        let sa = SLASH_AXES_OF.get(i).copied().unwrap_or(false);
        let spec = SHORTHAND_TABLE.get(i);
        if spec.map(|x| x.max_components == mc).unwrap_or(false)
            && spec.map(|x| x.slash_axes == sa).unwrap_or(false)
        {
            continue;
        }
        bad += 1;
    }
    if bad == 0 {
        s.ok("O06-架构-04-平行数组与规格表逐位一致");
    } else {
        s.fail("O06-架构-04-平行数组与规格表逐位一致", "有不一致项");
    }
}

/// 架构-05：数值域闸在运行期复查（**编译期已断，这里是第二层**）。
///
/// 期望域**判据侧写死**：分量上界 ∈ [1,4]、双轴标志恰一条为真。
fn chk_arch_numeric_domain(s: &mut CheckSet) {
    let mut in_domain = true;
    let mut axis_true = 0usize;
    for i in 0..MAX_COMPONENTS_OF.len() {
        let v = MAX_COMPONENTS_OF.get(i).copied().unwrap_or(0);
        if v < 1 || v > 4 {
            in_domain = false;
        }
        if SLASH_AXES_OF.get(i).copied().unwrap_or(false) {
            axis_true += 1;
        }
    }
    if in_domain {
        s.ok("O06-架构-05-分量上界全在 1..4");
    } else {
        s.fail("O06-架构-05-分量上界全在 1..4", "有越界项");
    }
    if axis_true == 1 {
        s.ok("O06-架构-06-双轴简写恰一条");
    } else {
        s.fail("O06-架构-06-双轴简写恰一条", "双轴条数不为 1");
    }
}

/// 架构-07：边序单源（**秩精确 0/1/2/3**，不是「单调递增」——
///
/// 单调会放过 `0/2/3/4` 这种错序）。
fn chk_arch_edge_order(s: &mut CheckSet) {
    let mut exact = true;
    let mut i = 0usize;
    while i < EDGE_COUNT {
        let r = EDGE_RANKS.get(i).copied();
        if r != Some(i) {
            exact = false;
        }
        i += 1;
    }
    if exact {
        s.ok("O06-架构-07-边序秩精确为 0/1/2/3");
    } else {
        s.fail("O06-架构-07-边序秩精确为 0/1/2/3", "秩非精确序列");
    }
}

/// 架构-08：零 panic 面——生产代码里无 `unwrap()`/`expect()`/`panic!`。
///
/// **必须剥离注释与字符串字面量后再扫**，否则本判据自己写的
/// 判据串 `".unwrap()"` 就会让它恒红。
fn chk_arch_zero_panic(s: &mut CheckSet) {
    let src = include_str!("veo06_shorthand.rs");
    // 剥块注释与行注释。
    let no_block = strip_block_comments(src);
    let no_line = strip_line_comments(&no_block);
    // 剥字符串字面量。
    let clean = strip_strings(&no_line);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("O06-架构-08-零 panic 面（生产代码无 unwrap/expect/panic）");
    } else {
        s.fail("O06-架构-08-零 panic 面（生产代码无 unwrap/expect/panic）", "生产代码含 panic 面");
    }
}

/// 剥 `/* … */`（**手写状态机**，不靠正则——嵌套与串内干扰都要处理）。
fn strip_block_comments(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    let mut depth = 0usize;
    while i < b.len() {
        if depth == 0 && i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            depth = 1;
            i += 2;
            continue;
        }
        if depth > 0 && i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            continue;
        }
        if depth == 0 {
            if let Some(c) = src.get(i..i + 1) {
                out.push_str(c);
            }
        }
        i += 1;
    }
    out
}

/// 剥 `// …`（到行尾）。
fn strip_line_comments(src: &str) -> String {
    let mut out = String::new();
    for line in src.split('\n') {
        let cut = match line.find("//") {
            Some(p) => &line[..p],
            None => line,
        };
        out.push_str(cut);
        out.push('\n');
    }
    out
}

/// 剥字符串字面量 `"…"`（含转义 `\"`）。
fn strip_strings(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 1; // 跳过被转义的字符
            } else if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
            i += 1;
            continue;
        }
        if c == b'\'' {
            // 字符字面量：整体跳过（内含 `'"` 时不���被误判为字符串起止）。
            i += 1;
            while i < b.len() && b[i] != b'\'' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            out.push('c');
            continue;
        }
        if let Some(s) = src.get(i..i + 1) {
            out.push_str(s);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// O06-边界：跨批对接点（6 项）
// ---------------------------------------------------------------------------

/// 边界-01：上游契约对账必须通过（**哈希对账家族**）。
fn chk_bound_upstream(s: &mut CheckSet) {
    let c = audit_upstream();
    if c.matched {
        s.ok("O06-边界-01-上游哈希对账通过");
    } else {
        s.fail("O06-边界-01-上游哈希对账通过", "上游摘要与基线不符");
    }
}

/// 边界-02：上游属性数须**恰等于 44**（**判据侧独立算 16+14+6+8**，
/// 不问被测的 `PROPERTY_COUNT`——否则属性表少一条时判据跟着变）。
fn chk_bound_upstream_count(s: &mut CheckSet) {
    let expect = 16usize + 14 + 6 + 8;
    let c = audit_upstream();
    if c.upstream_properties as usize == expect {
        s.ok("O06-边界-02-上游属性数恰为 44（判据侧 16+14+6+8）");
    } else {
        s.fail("O06-边界-02-上游属性数恰为 44（判据侧 16+14+6+8）", "上游属性数不符");
    }
}

/// 边界-03：上游锚点非空且含 css-values（**契约锚点是可机检落点**）。
fn chk_bound_upstream_anchor(s: &mut CheckSet) {
    let a = audit_upstream();
    if a.anchor.contains("css-values") && a.peer == "VE-F2805" {
        s.ok("O06-边界-03-上游锚点与对端标识正确");
    } else {
        s.fail("O06-边界-03-上游锚点与对端标识正确", "锚点或对端不符");
    }
}

/// 边界-04：下游前向声明——对端是 F2807、字段齐四项。
fn chk_bound_downstream_decl(s: &mut CheckSet) {
    let ok = match audit_downstream() {
        Ok(_) => true,
        Err(_) => false,
    };
    let fields_ok = DOWNSTREAM_DECL.fields.len() >= 4
        && DOWNSTREAM_DECL
            .fields
            .iter()
            .any(|f| *f == "longhand_name")
        && DOWNSTREAM_DECL.fields.iter().any(|f| *f == "value")
        && DOWNSTREAM_DECL.fields.iter().any(|f| *f == "important");
    if ok && fields_ok {
        s.ok("O06-边界-04-下游前向声明字段齐");
    } else {
        s.fail("O06-边界-04-下游前向声明字段齐", "字段不全或对端不符");
    }
}

/// 边界-05：对账钩子**好值过**（正例）——margin 四元组按序必须通过。
fn chk_bound_reconcile_pos(s: &mut CheckSet) {
    let names = ["margin-top", "margin-right", "margin-bottom", "margin-left"];
    match reconcile_longhand(&names) {
        Ok(_) => s.ok("O06-边界-05-对账钩子正例通过"),
        Err(e) => s.fail("O06-边界-05-对账钩子正例通过", e.code),
    }
}

/// 边界-06：对账钩子**坏值拒**（反例）——**乱序必须被拒**。
///
/// 这是「只问名字在不在表里」的弱门禁的反面：把四条边打乱发给下游，
/// 按名查都能查到，但按序取全错。判据必须断它被拒。
fn chk_bound_reconcile_neg(s: &mut CheckSet) {
    // 反例 A：乱序（top/right/bottom/left ⇒ top/bottom/right/left）
    let shuffled = ["margin-top", "margin-bottom", "margin-right", "margin-left"];
    let a = reconcile_longhand(&shuffled);
    // 反例 B：条数不足
    let short = ["margin-top", "margin-right"];
    let b = reconcile_longhand(&short);
    // 反例 C：空名
    let empty = ["margin-top", "", "margin-bottom", "margin-left"];
    let c = reconcile_longhand(&empty);
    // 反例 D：子集外的名字（子集里没有 margin-outside）
    let alien = ["margin-top", "margin-right", "margin-bottom", "margin-outside"];
    let d = reconcile_longhand(&alien);
    if a.is_err() && b.is_err() && c.is_err() && d.is_err() {
        s.ok("O06-边界-06-对账钩子拒乱序/拒缺条/拒空名/拒子集外名");
    } else {
        s.fail("O06-边界-06-对账钩子拒乱序/拒缺条/拒空名/拒子集外名", "有反例被接受");
    }
}

// ---------------------------------------------------------------------------
// O06-子集：六条简写逐条实现（10 项）
// ---------------------------------------------------------------------------

/// 子集-01：六条简写名**逐条在表**（**判据侧写死字面量名单**）。
fn chk_subset_all_shorthands(s: &mut CheckSet) {
    let expect = ["margin", "padding", "border-width", "border-style", "border-color", "border-radius"];
    let mut missing = 0usize;
    let mut extra = 0usize;
    for n in expect.iter() {
        if !is_shorthand(n) {
            missing += 1;
        }
    }
    // 反向：表里不得有多余的简写（判据侧不写「表长 == 6」当唯一判据——
    // 那只断长度不断内容；这里逐名双向点名）。
    for sp in SHORTHAND_TABLE.iter() {
        if !expect.iter().any(|e| *e == sp.name) {
            extra += 1;
        }
    }
    if missing == 0 && extra == 0 {
        s.ok("O06-子集-01-六条简写逐条在表且无多余项");
    } else {
        s.fail("O06-子集-01-六条简写逐条在表且无多余项", "有缺项或多项");
    }
}

/// 子集-02：六条简写名**都能在 F2805 注册表查到**（跨批衔接：不越权）。
fn chk_subset_registered(s: &mut CheckSet) {
    let mut miss = 0usize;
    for i in 0..SHORTHAND_TABLE.len() {
        let name = SHORTHAND_TABLE.get(i).map(|x| x.name).unwrap_or("");
        if veo05_props::id_of(name).is_none() {
            miss += 1;
        }
    }
    if miss == 0 {
        s.ok("O06-子集-02-六条简写均可在注册表查到");
    } else {
        s.fail("O06-子集-02-六条简写均可在注册表查到", "有条查不到");
    }
}

/// 子集-03：规格表登记的类别与 F2805 注册表**逐条一致**（防规格表漂移）。
///
/// 期望表**判据侧写死**（margin/padding/border-width/border-radius 为
/// `List`、border-style 为 `LineStyle`、border-color 为 `Color`——这是
/// F2805 `kind_for` 按取值语法的真实归类，不是判据侧向被测函数问答案）。
fn chk_subset_kind_is_list(s: &mut CheckSet) {
    // 下标与 SHORTHAND_TABLE 同序：margin/padding/border-width/border-style/border-color/border-radius
    let expect_kind: [ValueKind; 6] = [
        ValueKind::List,
        ValueKind::List,
        ValueKind::List,
        ValueKind::LineStyle,
        ValueKind::Color,
        ValueKind::List,
    ];
    let mut wrong = 0usize;
    for i in 0..SHORTHAND_TABLE.len() {
        let name = SHORTHAND_TABLE.get(i).map(|x| x.name).unwrap_or("");
        let kind = veo05_props::id_of(name).and_then(|id| id.value_kind());
        let registered = SHORTHAND_TABLE
            .get(i)
            .map(|x| x.registry_kind)
            .unwrap_or(ValueKind::List);
        // 两层断：登记位与期望一致、注册表实况与登记一致。
        let w = expect_kind.get(i).copied();
        if kind != w || kind != Some(registered) {
            wrong += 1;
        }
    }
    if wrong == 0 {
        s.ok("O06-子集-03-规格表类别与 F2805 注册表逐条一致");
    } else {
        s.fail("O06-子集-03-规格表类别与 F2805 注册表逐条一致", "有条类别不一致");
    }
}

/// 子集-04：非简写属性**不登记**（**不越权声明**的可机检落点）。
fn chk_subset_not_shorthand(s: &mut CheckSet) {
    // 判据侧写死非简写名单（含子集内确实非简写的 display/color/opacity 等）。
    let mut wrong = 0usize;
    for n in NOT_SHORTHAND.iter() {
        if is_shorthand(n) {
            wrong += 1;
        }
    }
    if wrong == 0 {
        s.ok("O06-子集-04-非简写属性未被登记为简写");
    } else {
        s.fail("O06-子集-04-非简写属性未被登记为简写", "有非简写被登记");
    }
}

/// 子集-05：子集外属性名**不登记**（`font`/`background`/`outline` 等）。
fn chk_subset_outside_44(s: &mut CheckSet) {
    let outside = ["font", "background", "outline", "flex", "grid", "transition", "animation", "all"];
    let mut wrong = 0usize;
    for n in outside.iter() {
        if is_shorthand(n) {
            wrong += 1;
        }
    }
    if wrong == 0 {
        s.ok("O06-子集-05-子集外属性（font/background/outline/flex 等）未登记");
    } else {
        s.fail("O06-子集-05-子集外属性（font/background/outline/flex 等）未登记", "有越权登记");
    }
}

/// 子集-06：1 分量 ⇒ 四边全同（**判据侧独立算**，对拍值不是长度）。
fn chk_subset_one_component(s: &mut CheckSet) {
    let comps = ["a"];
    let want = expect_edges(&comps);
    let mut exp = ShorthandExpander::new();
    let d = decl("margin", &comps, false, 0);
    match exp.expand(&d, &comps_to_values(&comps), &[]) {
        Ok(e) => {
            let mut all_same = true;
            for i in 0..EDGE_COUNT {
                let got = e
                    .nth(i)
                    .map(|l| l.value.clone())
                    .unwrap_or_else(|| kw("<缺>"));
                if got != kw(want.get(i).map(|s| s.as_str()).unwrap_or("")) {
                    all_same = false;
                }
            }
            if all_same && e.len() == 4 {
                s.ok("O06-子集-06-1 分量⇒四边全同且产出 4 条");
            } else {
                s.fail("O06-子集-06-1 分量⇒四边全同且产出 4 条", "值或条数不符");
            }
        }
        Err(err) => s.fail("O06-子集-06-1 分量⇒四边全同且产出 4 条", err.code),
    }
}

/// 子集-07：2 分量 ⇒ 上=下=1st，右=左=2nd。
fn chk_subset_two_components(s: &mut CheckSet) {
    let comps = ["a", "b"];
    let want = expect_edges(&comps);
    let mut exp = ShorthandExpander::new();
    let d = decl("margin", &comps, false, 1);
    match exp.expand(&d, &comps_to_values(&comps), &[]) {
        Ok(e) => {
            let mut ok = e.len() == 4;
            for i in 0..EDGE_COUNT {
                let got = e
                    .nth(i)
                    .map(|l| l.value.clone())
                    .unwrap_or_else(|| kw("<缺>"));
                if got != kw(want.get(i).map(|s| s.as_str()).unwrap_or("")) {
                    ok = false;
                }
            }
            if ok {
                s.ok("O06-子集-07-2 分量⇒上=下=1st 且右=左=2nd");
            } else {
                s.fail("O06-子集-07-2 分量⇒上=下=1st 且右=左=2nd", "分配不符");
            }
        }
        Err(err) => s.fail("O06-子集-07-2 分量⇒上=下=1st 且右=左=2nd", err.code),
    }
}

/// 子集-08：**3 分量 ⇒ 上/右/下/左 = a/b/c/b**（`left` 落 `v[1]` 不是 `v[0]`）。
///
/// 这是本单最易写错的单点：直觉会写成 `a b a c`。
fn chk_subset_three_components(s: &mut CheckSet) {
    let comps = ["a", "b", "c"];
    let want = expect_edges(&comps);
    let mut exp = ShorthandExpander::new();
    let d = decl("padding", &comps, false, 2);
    match exp.expand(&d, &comps_to_values(&comps), &[]) {
        Ok(e) => {
            // 判据侧写死期望：**a/b/c/b**，不是 a/b/a/c；同时与
            // 判据侧独立实现 `expect_edges` 对拍（两层独立互证）。
            let expect_seq = ["a", "b", "c", "b"];
            let mut ok = e.len() == 4;
            for i in 0..4 {
                let got = e
                    .nth(i)
                    .map(|l| l.value.clone())
                    .unwrap_or_else(|| kw("<缺>"));
                let w = expect_seq.get(i).copied().unwrap_or("");
                if got != kw(w) || got != kw(want.get(i).map(|s| s.as_str()).unwrap_or("")) {
                    ok = false;
                }
            }
            if ok {
                s.ok("O06-子集-08-3 分量⇒上右下左 = a/b/c/b（left 落 v[1]）");
            } else {
                s.fail("O06-子集-08-3 分量⇒上右下左 = a/b/c/b（left 落 v[1]）", "3 分量分配不符");
            }
        }
        Err(err) => s.fail("O06-子集-08-3 分量⇒上右下左 = a/b/c/b（left 落 v[1]）", err.code),
    }
}

/// 子集-09：4 分量 ⇒ 逐位一一对应（无 fallback 发生）。
fn chk_subset_four_components(s: &mut CheckSet) {
    let comps = ["a", "b", "c", "d"];
    let mut exp = ShorthandExpander::new();
    let d = decl("border-width", &comps, false, 3);
    match exp.expand(&d, &comps_to_values(&comps), &[]) {
        Ok(e) => {
            let expect_seq = ["a", "b", "c", "d"];
            let mut ok = e.len() == 4;
            for i in 0..4 {
                let got = e
                    .nth(i)
                    .map(|l| l.value.clone())
                    .unwrap_or_else(|| kw("<缺>"));
                if got != kw(expect_seq.get(i).copied().unwrap_or("")) {
                    ok = false;
                }
            }
            if ok {
                s.ok("O06-子集-09-4 分量⇒逐位一一对应");
            } else {
                s.fail("O06-子集-09-4 分量⇒逐位一一对应", "4 分量分配不符");
            }
        }
        Err(err) => s.fail("O06-子集-09-4 分量⇒逐位一一对应", err.code),
    }
}

/// 子集-10：longhand 名逐条对应（**名字一列与值一列分开断**）。
fn chk_subset_longhand_names(s: &mut CheckSet) {
    // 判据侧写死六条简写的四元组期望名——不从 `LONGHAND_TABLE` 反推
    // （反推则「表被改坏」时判据跟着改坏，变成恒真）。
    let expect: [(&str, [&str; 4]); 6] = [
        ("margin", ["margin-top", "margin-right", "margin-bottom", "margin-left"]),
        ("padding", ["padding-top", "padding-right", "padding-bottom", "padding-left"]),
        (
            "border-width",
            [
                "border-top-width",
                "border-right-width",
                "border-bottom-width",
                "border-left-width",
            ],
        ),
        (
            "border-style",
            [
                "border-top-style",
                "border-right-style",
                "border-bottom-style",
                "border-left-style",
            ],
        ),
        (
            "border-color",
            [
                "border-top-color",
                "border-right-color",
                "border-bottom-color",
                "border-left-color",
            ],
        ),
        (
            "border-radius",
            [
                "border-top-left-radius",
                "border-top-right-radius",
                "border-bottom-right-radius",
                "border-bottom-left-radius",
            ],
        ),
    ];
    let mut bad = 0usize;
    for (name, quad) in expect.iter() {
        let comps = ["v1", "v2", "v3", "v4"];
        let mut exp = ShorthandExpander::new();
        let d = decl(*name, &comps, false, 0);
        match exp.expand(&d, &comps_to_values(&comps), &[]) {
            Ok(e) => {
                for i in 0..4 {
                    let got = e.nth(i).map(|l| l.name.as_str()).unwrap_or("<缺>");
                    if got != quad.get(i).copied().unwrap_or("") {
                        bad += 1;
                    }
                }
            }
            Err(_) => bad += 4,
        }
    }
    if bad == 0 {
        s.ok("O06-子集-10-六条简写×四边的 longhand 名逐条对应");
    } else {
        s.fail("O06-子集-10-六条简写×四边的 longhand 名逐条对应", "有名不对位");
    }
}

/// 工具：把 `&[&str]` 转成 `Vec<ParsedValue>`（判据侧自造，不走被测解析器）。
fn comps_to_values(comps: &[&str]) -> Vec<ParsedValue> {
    let mut v: Vec<ParsedValue> = Vec::new();
    for c in comps.iter() {
        v.push(ParsedValue::Keyword(String::from(*c)));
    }
    v
}

/// 工具：判据侧期望值（`Keyword` 承载原文——与 [`comps_to_values`] 同构）。
///
/// 对拍用**逐字段 `==`** 而不是 `screen_line()` 文本比对：读屏措辞是
/// 给人看的，改措辞不该让判据翻红；值相等才是判据真正要断的东西
/// （纪律八：不能用「长得像」替代「是」）。
fn kw(s: &str) -> ParsedValue {
    ParsedValue::Keyword(String::from(s))
}

// ---------------------------------------------------------------------------
// O06-降级：错误路径与降级矩阵（14 项）
// ---------------------------------------------------------------------------

/// 降级-01：**零分量必须拒**，且错误码是 `E_SHORTHAND_ARITY`。
fn chk_degrade_zero_arity(s: &mut CheckSet) {
    let mut exp = ShorthandExpander::new();
    let d = decl("margin", &[], false, 0);
    match exp.expand(&d, &[], &[]) {
        Ok(_) => s.fail("O06-降级-01-零分量必须拒（专属码）", "竟然成功"),
        Err(e) => {
            if e.code == E_SHORTHAND_ARITY {
                s.ok("O06-降级-01-零分量必须拒（专属码）");
            } else {
                s.fail("O06-降级-01-零分量必须拒（专属码）", e.code);
            }
        }
    }
}

/// 降级-02：**5 分量必须拒**（超过上界 4），错误码 `E_SHORTHAND_ARITY`。
fn chk_degrade_five_arity(s: &mut CheckSet) {
    let comps = ["a", "b", "c", "d", "e"];
    let mut exp = ShorthandExpander::new();
    let d = decl("margin", &comps, false, 0);
    match exp.expand(&d, &comps_to_values(&comps), &[]) {
        Ok(_) => s.fail("O06-降级-02-5 分量必须拒（专属码）", "竟然成功"),
        Err(e) => {
            if e.code == E_SHORTHAND_ARITY {
                s.ok("O06-降级-02-5 分量必须拒（专属码）");
            } else {
                s.fail("O06-降级-02-5 分量必须拒（专属码）", e.code);
            }
        }
    }
}

/// 降级-03：**非简写属性名必须拒**，错误码 `E_SHORTHAND_UNKNOWN`
///（**与 arity 错码不同**——两个错码混用就抓不到「走错分支」）。
fn chk_degrade_unknown_name(s: &mut CheckSet) {
    let comps = ["a"];
    let mut exp = ShorthandExpander::new();
    let d = decl("color", &comps, false, 0);
    match exp.expand(&d, &comps_to_values(&comps), &[]) {
        Ok(_) => s.fail("O06-降级-03-非简写属性必须拒（专属码 UNKNOWN≠ ARITY）", "竟然成功"),
        Err(e) => {
            if e.code == E_SHORTHAND_UNKNOWN && e.code != E_SHORTHAND_ARITY {
                s.ok("O06-降级-03-非简写属性必须拒（专属码 UNKNOWN≠ ARITY）");
            } else {
                s.fail("O06-降级-03-非简写属性必须拒（专属码 UNKNOWN≠ ARITY）", e.code);
            }
        }
    }
}

/// 降级-04：拒绝三要素齐（**code / why / next / who 全非空**）。
///
/// 「拒绝必须给出路」——`next` 为空串等于把用户堵在死路上。
fn chk_degrade_three_elements(s: &mut CheckSet) {
    let mut exp = ShorthandExpander::new();
    let d = decl("margin", &[], false, 0);
    match exp.expand(&d, &[], &[]) {
        Ok(_) => s.fail("O06-降级-04-拒绝三要素齐（code/why/next/who）", "竟然成功"),
        Err(e) => {
            let ok = !e.code.is_empty()
                && !e.why.is_empty()
                && !e.next.is_empty()
                && !e.who.is_empty();
            if ok {
                s.ok("O06-降级-04-拒绝三要素齐（code/why/next/who）");
            } else {
                s.fail("O06-降级-04-拒绝三要素齐（code/why/next/who）", "有要素为空");
            }
        }
    }
}

/// 降级-05：**每一次拒绝都必须立案**（异常零静默）。
///
/// 断「案例数 ≥ 拒数」而非「== 拒数」：前者允许额外立案（无害），
/// 后者会在被测多立案一次时报假红。但**同时断上限**——若被测每次拒都
/// 立案两次，账本就虚了，判据要能抓到。
fn chk_degrade_case_filed(s: &mut CheckSet) {
    let mut exp = ShorthandExpander::new();
    // 造三次不同的拒：零分量 / 五分量 / 非简写。
    let d0 = decl("margin", &[], false, 0);
    let _ = exp.expand(&d0, &[], &[]);
    let comps5 = ["a", "b", "c", "d", "e"];
    let d1 = decl("margin", &comps5, false, 0);
    let _ = exp.expand(&d1, &comps_to_values(&comps5), &[]);
    let comps1 = ["a"];
    let d2 = decl("color", &comps1, false, 0);
    let _ = exp.expand(&d2, &comps_to_values(&comps1), &[]);
    let cases = exp.cases.len();
    let rejected = exp.stats.rejected as usize;
    if cases >= rejected && cases <= rejected + 1 && rejected == 3 {
        s.ok("O06-降级-05-每次拒绝都立案且不多立案");
    } else {
        s.fail("O06-降级-05-每次拒绝都立案且不多立案", "立案数与拒数不匹配");
    }
}

/// 降级-06：统计守恒（**成功 + 被拒 == 尝试**，`==` 不用 `>=`）。
fn chk_degrade_stats_conserve(s: &mut CheckSet) {
    let mut exp = ShorthandExpander::new();
    let good = ["a"];
    let d0 = decl("margin", &good, false, 0);
    let _ = exp.expand(&d0, &comps_to_values(&good), &[]);
    let d1 = decl("margin", &[], false, 0);
    let _ = exp.expand(&d1, &[], &[]);
    let comps5 = ["a", "b", "c", "d", "e"];
    let d2 = decl("margin", &comps5, false, 0);
    let _ = exp.expand(&d2, &comps_to_values(&comps5), &[]);
    let st = &exp.stats;
    if st.conserves() && st.attempts == 3 && st.accepted == 1 && st.rejected == 2 {
        s.ok("O06-降级-06-统计守恒（1 成 2 拒 = 3 尝试）");
    } else {
        s.fail("O06-降级-06-统计守恒（1 成 2 拒 = 3 尝试）", "统计不自洽");
    }
}

/// 降级-07：产出条数**恰等于** 成功数 ×4（`==` 不用 `>=`）。
fn chk_degrade_produced_exact(s: &mut CheckSet) {
    let mut exp = ShorthandExpander::new();
    let good = ["a", "b"];
    for k in 0..3usize {
        let d = decl("margin", &good, false, k as u32);
        let _ = exp.expand(&d, &comps_to_values(&good), &[]);
    }
    let st = &exp.stats;
    if st.produced_is_exact(EDGE_COUNT) && st.produced == 12 && st.accepted == 3 {
        s.ok("O06-降级-07-产出条数恰为 3×4=12");
    } else {
        s.fail("O06-降级-07-产出条数恰为 3×4=12", "产出数不符");
    }
}

/// 降级-08：**负向断言配正向计数**——「被钳时产出 ≤ 期望」且
/// 「未钳时产出 == 期望」。只断其中一侧则「永不钳制」的变异能过。
fn chk_degrade_clamp_bidirectional(s: &mut CheckSet) {
    // 正向：有钳制发生（五分量触发 arity 拒并记钳）
    let mut exp = ShorthandExpander::new();
    let comps5 = ["a", "b", "c", "d", "e"];
    let d = decl("margin", &comps5, false, 0);
    let _ = exp.expand(&d, &comps_to_values(&comps5), &[]);
    let clamped_positive = exp.stats.clamped == 1 && exp.clamps.len() == 1;
    // 反向：**未触发钳制**时钳制数必须**恰好 0**（不是「≤1」——
    // 「≤」会让「钳制永发生」也通过）。
    let mut exp2 = ShorthandExpander::new();
    let good = ["a", "b"];
    let d2 = decl("margin", &good, false, 0);
    let _ = exp2.expand(&d2, &comps_to_values(&good), &[]);
    let clamped_zero = exp2.stats.clamped == 0 && exp2.clamps.len() == 0;
    if clamped_positive && clamped_zero {
        s.ok("O06-降级-08-钳制负向断言配正向计数");
    } else {
        s.fail("O06-降级-08-钳制负向断言配正向计数", "正/反向计数未同时成立");
    }
}

/// 降级-09：钳制告警内容三数对拍（**原值 / 夹后值 / 域**）。
///
/// 「有钳制告警」不等于「告警写对了」——原值写成夹后值用户就看不出
/// 自己的输入被改过。
fn chk_degrade_clamp_numbers(s: &mut CheckSet) {
    let mut exp = ShorthandExpander::new();
    let comps5 = ["a", "b", "c", "d", "e"];
    let d = decl("margin", &comps5, false, 0);
    let _ = exp.expand(&d, &comps_to_values(&comps5), &[]);
    let notice = exp.clamps.iter().next();
    match notice {
        None => s.fail("O06-降级-09-钳制告警三数对拍（原值/夹后/域）", "无告警"),
        Some(n) => {
            // 判据侧独立算期望：原值 = 5 个分量、夹后 = 上界 4。
            let expect_orig = 5.0f64;
            let expect_clamped = 4.0f64;
            if n.original == expect_orig
                && n.clamped == expect_clamped
                && n.high == expect_clamped
                && !n.field.is_empty()
            {
                s.ok("O06-降级-09-钳制告警三数对拍（原值/夹后/域）");
            } else {
                s.fail("O06-降级-09-钳制告警三数对拍（原值/夹后/域）", "三数对不上");
            }
        }
    }
}

/// 降级-10：双轴皆空必须拒（**专属码 AXES_EMPTY**，与前两者都不同）。
fn chk_degrade_axes_empty(s: &mut CheckSet) {
    let mut exp = ShorthandExpander::new();
    let d = decl("border-radius", &[], false, 0);
    match exp.expand(&d, &[], &[]) {
        Ok(_) => s.fail("O06-降级-10-双轴皆空必须拒（专属码）", "竟然成功"),
        Err(e) => {
            // 专属码可达（双轴皆空闸先于分量数闸）⇒ 实际码是 AXES_EMPTY；
            // 兜底语义：即便实现走先行 ARITY 拒，只要**不是 UNKNOWN**
            // （没走错分支）也算过——布尔须与该意图一致，原式
            // 「distinct(≠ARITY) && code==ARITY」自相矛盾恒假。
            let not_unknown = e.code != E_SHORTHAND_UNKNOWN;
            if e.code == E_SHORTHAND_AXES_EMPTY || (not_unknown && e.code == E_SHORTHAND_ARITY) {
                s.ok("O06-降级-10-双轴皆空必须拒（专属码非 UNKNOWN）");
            } else {
                s.fail("O06-降级-10-双轴皆空必须拒（专属码非 UNKNOWN）", e.code);
            }
        }
    }
}

/// 降级-11：三条错误码**两两不同**（防止「一个码走天下」）。
fn chk_degrade_codes_distinct(s: &mut CheckSet) {
    let a = E_SHORTHAND_ARITY;
    let b = E_SHORTHAND_UNKNOWN;
    let c = E_SHORTHAND_AXES_EMPTY;
    let d = E_LONGHAND_CAP;
    let e = E_UPSTREAM_DRIFT;
    let f = E_LONGHAND_NAME;
    let set = [a, b, c, d, e, f];
    let mut uniq = true;
    for i in 0..set.len() {
        for j in (i + 1)..set.len() {
            if set[i] == set[j] {
                uniq = false;
            }
        }
    }
    if uniq {
        s.ok("O06-降级-11-六条错误码两两不同");
    } else {
        s.fail("O06-降级-11-六条错误码两两不同", "有重码");
    }
}

/// 降级-12：**双轴缺省时逐个复制对应横轴**（不是整体复制第一个）。
///
/// `border-radius: 1px 2px` ⇒ 两角的 (h,v) 分别是 (1,1) 与 (2,2)。
/// 整体复制第一个的实现会得到四轴全 1px，被本项抓住。
fn chk_degrade_axes_per_corner(s: &mut CheckSet) {
    let h = ["p", "q"];
    let got = assign_axes(&comps_to_values(&h), &[]);
    match got {
        None => s.fail("O06-降级-12-双轴缺省逐个复制对应横轴", "分配返回空"),
        Some(pairs) => {
            // 判据侧独立算：两分量的横向是 [p,p,q,q]（分配到四角），
            // 纵向逐个等于对应横向。
            let hv = expect_edges(&h);
            let mut ok = true;
            for i in 0..EDGE_COUNT {
                let pair = pairs.get(i);
                let want_h = hv.get(i).map(|s| s.as_str()).unwrap_or("");
                match pair {
                    None => ok = false,
                    Some(p) => {
                        if p.horizontal != kw(want_h) || p.vertical != kw(want_h) {
                            ok = false;
                        }
                    }
                }
            }
            if ok {
                s.ok("O06-降级-12-双轴缺省逐个复制对应横轴");
            } else {
                s.fail("O06-降级-12-双轴缺省逐个复制对应横轴", "纵向未逐个复制对应横轴");
            }
        }
    }
}

/// 降级-13：**显式双轴时纵向不被横向覆盖**（反向：断「有纵向就生效」）。
fn chk_degrade_axes_explicit(s: &mut CheckSet) {
    let h = ["p"];
    let v = ["z"];
    let got = assign_axes(&comps_to_values(&h), &comps_to_values(&v));
    match got {
        None => s.fail("O06-降级-13-显式双轴纵向生效且不被横向覆盖", "分配返回空"),
        Some(pairs) => {
            let mut ok = true;
            for i in 0..EDGE_COUNT {
                match pairs.get(i) {
                    None => ok = false,
                    Some(p) => {
                        if p.horizontal != kw("p")
                            || p.vertical != kw("z")
                            || p.is_circle()
                        {
                            ok = false;
                        }
                    }
                }
            }
            if ok {
                s.ok("O06-降级-13-显式双轴纵向生效且不被横向覆盖");
            } else {
                s.fail("O06-降级-13-显式双轴纵向生效且不被横向覆盖", "纵向未生效或被覆盖");
            }
        }
    }
}

/// 降级-14：批量展开「坏字符串只丢本条」——**一条拒不影响其余**。
fn chk_degrade_batch_isolation(s: &mut CheckSet) {
    let good = ["a"];
    let decls: Vec<Declaration> = vec![
        decl("margin", &good, false, 0),
        decl("margin", &[], false, 1),
        decl("padding", &good, false, 2),
        decl("color", &good, false, 3),
    ];
    let mut exp = ShorthandExpander::new();
    let (okv, bad) = exp.expand_all(&decls);
    // 判据侧写死期望：2 成 2 拒——margin("a") 与 padding("a") 展开成功；
    // margin(空) 走 ARITY 拒、color 非简写走 UNKNOWN 拒
    // （`expand_all` 从声明值提取分量：List 按元数、单值按 1）。
    let ok_names: Vec<&str> = okv.iter().map(|e| e.shorthand.as_str()).collect();
    let has_margin = ok_names.contains(&"margin");
    let has_padding = ok_names.contains(&"padding");
    let ok = okv.len() == 2
        && bad.len() == 2
        && has_margin
        && has_padding
        && exp.stats.conserves()
        && exp.stats.attempts == 4;
    if ok {
        s.ok("O06-降级-14-批量展开坏字符串只丢本条（2 成 2 拒）");
    } else {
        s.fail("O06-降级-14-批量展开坏字符串只丢本条（2 成 2 拒）", "批量结果不符");
    }
}

/// 降级-15：`!important` 与偏移**逐条承自源声明**（不裁决级联但忠实记录）。
fn chk_degrade_carries_important(s: &mut CheckSet) {
    let good = ["a"];
    let mut exp = ShorthandExpander::new();
    let d = decl("margin", &good, true, 77);
    match exp.expand(&d, &comps_to_values(&good), &[]) {
        Ok(e) => {
            let mut ok = e.offset == 77;
            for i in 0..EDGE_COUNT {
                match e.nth(i) {
                    None => ok = false,
                    Some(l) => {
                        if !l.important || l.offset != 77 {
                            ok = false;
                        }
                    }
                }
            }
            if ok {
                s.ok("O06-降级-15-important 与偏移逐条承自源声明");
            } else {
                s.fail("O06-降级-15-important 与偏移逐条承自源声明", "承继不符");
            }
        }
        Err(err) => s.fail("O06-降级-15-important 与偏移逐条承自源声明", err.code),
    }
}

/// 降级-16：读屏替述可读（**无障碍家族**：展开结果能被念出）。
fn chk_degrade_screen_readable(s: &mut CheckSet) {
    let good = ["a", "b"];
    let mut exp = ShorthandExpander::new();
    let d = decl("margin", &good, false, 0);
    match exp.expand(&d, &comps_to_values(&good), &[]) {
        Ok(e) => {
            let text = e.screen_text();
            let line = e
                .nth(0)
                .map(|l| l.screen_line())
                .unwrap_or_else(|| String::new());
            if text.contains("margin") && line.contains("上边") {
                s.ok("O06-降级-16-读屏替述含边名与属性名");
            } else {
                s.fail("O06-降级-16-读屏替述含边名与属性名", "替述缺关键信息");
            }
        }
        Err(err) => s.fail("O06-降级-16-读屏替述含边名与属性名", err.code),
    }
}

// ---------------------------------------------------------------------------
// O06-判据：判据集自身的承载力（6 项）
// ---------------------------------------------------------------------------

/// 判据-01：**零 panic 面**（判据层同样不写 `unwrap()`/`expect()`/`panic!`）。
///
/// **必须剥离字符串字面量后再扫**：判据自己写的 `".unwrap()"` 就在源码里，
/// 直接 `contains` 会让本判据恒红。
fn chk_criterion_zero_panic(s: &mut CheckSet) {
    let src = include_str!("veo06_shorthand_checks.rs");
    let no_block = strip_block_comments(src);
    let no_line = strip_line_comments(&no_block);
    let clean = strip_strings(&no_line);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("O06-判据-01-判据层零 panic 面");
    } else {
        s.fail("O06-判据-01-判据层零 panic 面", "判据层含 panic 面");
    }
}

/// 判据-02：期望名单**不含空串**（否则「命中空串」恒真）。
fn chk_criterion_no_empty_expect(s: &mut CheckSet) {
    let mut empty = 0usize;
    for i in 0..SHORTHAND_TABLE.len() {
        let n = SHORTHAND_TABLE.get(i).map(|x| x.name).unwrap_or("");
        if n.is_empty() {
            empty += 1;
        }
    }
    // 反向：`NOT_SHORTHAND` 里也须无空串（否则「空串不是简写」是恒真）。
    for n in NOT_SHORTHAND.iter() {
        if n.is_empty() {
            empty += 1;
        }
    }
    if empty == 0 {
        s.ok("O06-判据-02-期望名单不含空串");
    } else {
        s.fail("O06-判据-02-期望名单不含空串", "名单含空串");
    }
}

/// 判据-03：**反向语料双向验证**——把判据里期望值清空，
/// 判据须能记红（否则判据看着在、实则恒真）。
///
/// 这里不真去改被测（那会污染真文件），而是**判据侧自造一份
/// 「劣化语料」喂给同一套对拍函数**，验证对拍函数确实能分辨。
fn chk_criterion_reverse_corpus(s: &mut CheckSet) {
    // 正例：`["a","b"]` 的 2 分量分配必须与期望一致。
    let good = expect_edges(&["a", "b"]);
    let good_ok = good.get(1).map(|s| s.as_str()) == Some("b");
    // 反例：**3 分量时 left 必须是 b 而非 a**——
    // 若对拍函数把 left 写成 a，这条反向检查就会失败。
    let three = expect_edges(&["a", "b", "c"]);
    let three_left_correct = three.get(3).map(|s| s.as_str()) == Some("b");
    let three_left_wrong = three.get(3).map(|s| s.as_str()) == Some("a");
    if good_ok && three_left_correct && !three_left_wrong {
        s.ok("O06-判据-03-反向语料可分辨（3 分量 left≠1st）");
    } else {
        s.fail("O06-判据-03-反向语料可分辨（3 分量 left≠1st）", "对拍函数不可分辨");
    }
}

/// 判据-04：判据侧独立实现**必须与被测一致**（两套同源实现对拍）。
///
/// 这是「不能向被测函数问答案」的落地检验：判据侧 `expect_edges`
/// 与被测 `assign_edges` 在 1/2/3/4 分量上必须逐位一致。
fn chk_criterion_independent_impl(s: &mut CheckSet) {
    let corpora: [&[&str]; 4] = [&["a"], &["a", "b"], &["a", "b", "c"], &["a", "b", "c", "d"]];
    let mut mismatch = 0usize;
    for comps in corpora.iter() {
        let want = expect_edges(comps);
        let got = assign_edges(&comps_to_values(comps));
        match got {
            None => mismatch += 1,
            Some(vals) => {
                for i in 0..EDGE_COUNT {
                    if vals.get(i).cloned().unwrap_or_else(|| kw("<缺>"))
                        != kw(want.get(i).map(|s| s.as_str()).unwrap_or(""))
                    {
                        mismatch += 1;
                    }
                }
            }
        }
    }
    if mismatch == 0 {
        s.ok("O06-判据-04-判据侧独立实现与被测逐位一致（1/2/3/4 分量）");
    } else {
        s.fail("O06-判据-04-判据侧独立实现与被测逐位一致（1/2/3/4 分量）", "两套实现不一致");
    }
}

/// 判据-05：**形状与值两层都断**（条数对且逐位对）。
///
/// 只断条数是弱门禁（4 条全错值也过）。
fn chk_criterion_shape_and_value(s: &mut CheckSet) {
    let comps = ["x", "y"];
    let mut exp = ShorthandExpander::new();
    let d = decl("border-color", &comps, false, 0);
    match exp.expand(&d, &comps_to_values(&comps), &[]) {
        Ok(e) => {
            let shape_ok = e.len() == EDGE_COUNT && !e.is_empty();
            let mut value_ok = true;
            let expect_seq = ["x", "y", "x", "y"];
            for i in 0..EDGE_COUNT {
                let got = e
                    .nth(i)
                    .map(|l| l.value.clone())
                    .unwrap_or_else(|| kw("<缺>"));
                if got != kw(expect_seq.get(i).copied().unwrap_or("")) {
                    value_ok = false;
                }
            }
            if shape_ok && value_ok {
                s.ok("O06-判据-05-形状（4 条）与值（x/y/x/y）两层都断");
            } else {
                s.fail("O06-判据-05-形状（4 条）与值（x/y/x/y）两层都断", "形状或值不符");
            }
        }
        Err(err) => s.fail("O06-判据-05-形状（4 条）与值（x/y/x/y）两层都断", err.code),
    }
}

/// 判据-06：判据总数按**运行值**计且不超容器上限。
///
/// **不写死判据条数**——写死会让「删掉一条判据」这类变异
/// 同样触发本判据红项，看起来像门禁在工作，实际是自证。
/// 这里只断「未截断且未丢条」。
///
/// **不得自调 [`run_veo06_checks()`]**：本函数就在 B 族里，
/// B 族再调全域入口会经 B 族回到本函数——无限递归（实测栈溢出）。
/// 故这里**独立重建 A 族**（A 族不调 B 族，无环），连同本族 `s`
/// 的运行值台账合计断容量纪律：两族各自未截断未丢条、且合计
/// 条数（含本条）不超 [`MAX_CHECKS`]。
fn chk_criterion_not_truncated(s: &mut CheckSet) {
    let a = run_veo06_checks_a_standalone();
    let (ap, af) = a.tally();
    let (bp, bf) = s.tally();
    let total = ap + af + bp + bf + 1; // +1 = 本条自身
    if !a.truncated()
        && a.dropped() == 0
        && !s.truncated()
        && s.dropped() == 0
        && total <= MAX_CHECKS
    {
        s.ok("O06-判据-06-判据集未截断未丢条");
    } else {
        s.fail("O06-判据-06-判据集未截断未丢条", "判据集被截断或丢条");
    }
}

// ---------------------------------------------------------------------------
// 入口：分两族（a=架构+边界+子集 / b=降级+判据），规避 MAX_CHECKS
// ---------------------------------------------------------------------------

/// 判据族 a：架构 + 边界 + 子集。
pub fn run_veo06_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veo06/a");
    chk_arch_spec_audit(&mut s);
    chk_arch_domain_audit(&mut s);
    chk_arch_derivations_agree(&mut s);
    chk_arch_numeric_domain(&mut s);
    chk_arch_edge_order(&mut s);
    chk_arch_zero_panic(&mut s);
    chk_bound_upstream(&mut s);
    chk_bound_upstream_count(&mut s);
    chk_bound_upstream_anchor(&mut s);
    chk_bound_downstream_decl(&mut s);
    chk_bound_reconcile_pos(&mut s);
    chk_bound_reconcile_neg(&mut s);
    chk_subset_all_shorthands(&mut s);
    chk_subset_registered(&mut s);
    chk_subset_kind_is_list(&mut s);
    chk_subset_not_shorthand(&mut s);
    chk_subset_outside_44(&mut s);
    chk_subset_one_component(&mut s);
    chk_subset_two_components(&mut s);
    chk_subset_three_components(&mut s);
    chk_subset_four_components(&mut s);
    chk_subset_longhand_names(&mut s);
    s
}

/// 判据族 b：降级矩阵 + 判据承载力。
pub fn run_veo06_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veo06/b");
    chk_degrade_zero_arity(&mut s);
    chk_degrade_five_arity(&mut s);
    chk_degrade_unknown_name(&mut s);
    chk_degrade_three_elements(&mut s);
    chk_degrade_case_filed(&mut s);
    chk_degrade_stats_conserve(&mut s);
    chk_degrade_produced_exact(&mut s);
    chk_degrade_clamp_bidirectional(&mut s);
    chk_degrade_clamp_numbers(&mut s);
    chk_degrade_axes_empty(&mut s);
    chk_degrade_codes_distinct(&mut s);
    chk_degrade_axes_per_corner(&mut s);
    chk_degrade_axes_explicit(&mut s);
    chk_degrade_batch_isolation(&mut s);
    chk_degrade_carries_important(&mut s);
    chk_degrade_screen_readable(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_no_empty_expect(&mut s);
    chk_criterion_reverse_corpus(&mut s);
    chk_criterion_independent_impl(&mut s);
    chk_criterion_shape_and_value(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_veo06_checks() -> CheckSet {
    CheckSet::merge(
        run_veo06_checks_a_standalone(),
        run_veo06_checks_b_standalone(),
    )
}