//! VE-F0408 · 域自检（判据逐条对应，见 `vec08_comment.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 不嵌套语义 → `C08-嵌套-*`（内层 /* 报错指向内层开符）
//! - 文档提取 → `C08-文档-*`（标注语法表逐标注）
//! - 元数据分离 → `C08-分离-*`（文档注释不进 comments 流、独立元数据流）
//! - 零语义 → `C08-零语义-*`（注释内容任意、字符串保护）

use super::vec08_comment::*;
use crate::checks::CheckSet;

/// VE-F0408 域自检。
pub fn run_vec08_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec08");
    let cfg = CommentConfig::default_config();

    // ---- 行/块注释基本提取 ----

    // 行注释：内容不含定界符、行号记账
    {
        let r = scan_comments("a // 行注\nb // 二行\n", &cfg).unwrap();
        set.add(
            "C08-基本-行注释提取",
            r.comments.len() == 2
                && r.comments[0].kind == CommentKind::Line
                && r.comments[0].text == " 行注"
                && r.comments[0].line == 1
                && r.comments[1].line == 2,
            "",
        );
    }
    // 块注释：内容与原文保真（原文含定界符）
    {
        let r = scan_comments("x /* 块内 */ y", &cfg).unwrap();
        set.add(
            "C08-基本-块注释提取",
            r.comments.len() == 1
                && r.comments[0].kind == CommentKind::Block
                && r.comments[0].text == " 块内 "
                && r.comments[0].original == "/* 块内 */",
            "",
        );
    }
    // 块注释跨行
    {
        let r = scan_comments("/* a\nb\nc */", &cfg).unwrap();
        set.add(
            "C08-基本-块注释跨行",
            r.comments.len() == 1 && r.comments[0].text.contains('\n'),
            "",
        );
    }

    // ---- 判据：不嵌套语义 ----

    // 内层 /* 报错指向内层开符
    {
        let e = scan_comments("/* a /* b */", &cfg).unwrap_err();
        set.add(
            "C08-嵌套-内层报错指向内层开符",
            e.code == "E_CMT_NESTED" && e.pos == 5 && e.is_complete(),
            "",
        );
    }
    // 嵌套形态的合法对照：*/ 前的独立注释正常
    {
        let r = scan_comments("/* a */ /* b */", &cfg).unwrap();
        set.add("C08-嵌套-独立注释正常", r.comments.len() == 2, "");
    }

    // ---- 判据：未闭合指向开符 ----

    {
        let e = scan_comments("x /* abc", &cfg).unwrap_err();
        let (l, c) = CommentError::line_col("x /* abc", e.pos);
        set.add(
            "C08-未闭合-块注释指向开符",
            e.code == "E_CMT_UNCLOSED" && e.pos == 2 && l == 1 && c == 3,
            "",
        );
    }
    // 跨行未闭合仍指向开符行
    {
        let e = scan_comments("/* a\nb\nc", &cfg).unwrap_err();
        let (l, _) = CommentError::line_col("/* a\nb\nc", e.pos);
        set.add("C08-未闭合-跨行仍指向首行开符", e.code == "E_CMT_UNCLOSED" && l == 1, "");
    }

    // ---- 判据：文档提取 ----

    // 行文档 @brief
    {
        let r = scan_comments("/// @brief 简述文本\nfn()", &cfg).unwrap();
        let a = &r.doc_meta.annotations;
        set.add(
            "C08-文档-行文档brief",
            a.len() == 1 && a[0].tag == "brief" && a[0].text == "简述文本" && a[0].line == 1,
            "",
        );
    }
    // 行文档 @param：参数名 + 文本分离
    {
        let r = scan_comments("/// @param uv 纹理坐标\n", &cfg).unwrap();
        let a = &r.doc_meta.annotations;
        set.add(
            "C08-文档-param带参数名",
            a.len() == 1 && a[0].tag == "param" && a[0].arg.as_deref() == Some("uv")
                && a[0].text == "纹理坐标",
            "",
        );
    }
    // 块文档多标注逐行提取（含 * 行首装饰剥除）
    {
        let r = scan_comments("/**\n * @brief 摘要\n * @return 无\n */", &cfg).unwrap();
        let a = &r.doc_meta.annotations;
        set.add(
            "C08-文档-块文档多标注",
            a.len() == 2
                && a[0].tag == "brief"
                && a[0].text == "摘要"
                && a[1].tag == "return"
                && a[1].text == "无",
            "",
        );
    }
    // 标注全表覆盖：6 个合法标注都进表
    {
        let src = "/// @brief b\n/// @param p v\n/// @return r\n/// @see s\n/// @deprecated d\n/// @since 1\n";
        let r = scan_comments(src, &cfg).unwrap();
        let tags: Vec<&str> = r.doc_meta.annotations.iter().map(|a| a.tag.as_str()).collect();
        set.add(
            "C08-文档-标注全表",
            tags == vec!["brief", "param", "return", "see", "deprecated", "since"],
            "",
        );
    }
    // 未知标注 → 警告不阻断（仍有结果产出）
    {
        let r = scan_comments("/// @unknown 啥\n", &cfg).unwrap();
        set.add(
            "C08-文档-未知标注警告",
            r.doc_meta.annotations.is_empty()
                && r.doc_meta.warnings.len() == 1
                && r.doc_meta.warnings[0].contains("@unknown"),
            "",
        );
    }
    // @param 缺参数名 → 警告
    {
        let r = scan_comments("/// @param 没名字直接是文本\n", &cfg).unwrap();
        // 「没名字直接是文本」是一个词——会被当参数名；用纯空白后接文本触发
        let r2 = scan_comments("/// @param   \n", &cfg).unwrap();
        set.add(
            "C08-文档-param缺名警告",
            r.doc_meta.warnings.is_empty()
                // 缺名不阻断：标注仍保留（arg=None）+ 警告一条
                && r2.doc_meta.annotations.len() == 1
                && r2.doc_meta.annotations[0].arg.is_none()
                && r2.doc_meta.warnings.len() == 1,
            "",
        );
    }

    // ---- 判据：元数据分离 ----

    // 文档注释不进 comments 流；普通注释不进 doc_meta
    {
        let r = scan_comments("/// @brief 文\n// 普通\n/* 块 */\n", &cfg).unwrap();
        let kinds: Vec<CommentKind> = r.comments.iter().map(|c| c.kind).collect();
        set.add(
            "C08-分离-两流不混",
            r.comments.len() == 2
                && kinds == vec![CommentKind::Line, CommentKind::Block]
                && r.doc_meta.annotations.len() == 1
                && r.doc_originals.len() == 1
                && r.doc_originals[0] == "/// @brief 文",
            "",
        );
    }

    // ---- 判据：零语义 ----

    // 字符串保护：串内注释定界符不触发注释词法
    {
        let r = scan_comments("let s = \"a /* b */ c\"; // 行注\n", &cfg).unwrap();
        set.add(
            "C08-零语义-字符串保护",
            r.comments.len() == 1
                && r.comments[0].kind == CommentKind::Line
                && r.comments[0].line == 1,
            "",
        );
    }
    // 原始串保护：r#"…"# 内的 /* 与 // 全忽略
    {
        let r = scan_comments("r#\"/* 不算 */ // 也不算\"# // 真注释\n", &cfg).unwrap();
        set.add(
            "C08-零语义-原始串保护",
            r.comments.len() == 1 && r.comments[0].text.contains("真注释"),
            "",
        );
    }
    // 注释内容可以是非法 token 文本——扫描器零语义消费
    {
        let r = scan_comments("/* }} \\u{ \\xZZ @#$ */\n// \"未闭合引号\n", &cfg).unwrap();
        set.add(
            "C08-零语义-注释内容任意",
            r.comments.len() == 2 && r.doc_meta.annotations.is_empty(),
            "",
        );
    }
    // 空输入与无注释输入
    {
        let a = scan_comments("", &cfg).unwrap();
        let b = scan_comments("fn x() {}", &cfg).unwrap();
        set.add(
            "C08-边界-空与无注释",
            a.comments.is_empty() && b.comments.is_empty() && b.doc_meta.warnings.is_empty(),
            "",
        );
    }
    // 文档块内的非标注行不产出、警告为空
    {
        let r = scan_comments("/**\n自由行\n@brief 有\n*/", &cfg).unwrap();
        set.add(
            "C08-边界-块内自由行忽略",
            r.doc_meta.annotations.len() == 1 && r.doc_meta.warnings.is_empty(),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VE-F0408 全绿闸：红项逐行枚举（定位用），零红才算过。
    #[test]
    fn vec08_checks_all_green() {
        let set = run_vec08_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "vec08 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
                .0
                .iter()
                .flatten()
                .filter(|c| !c.passed)
                .map(|c| c.name)
                .collect::<Vec<_>>()
        );
    }
}
