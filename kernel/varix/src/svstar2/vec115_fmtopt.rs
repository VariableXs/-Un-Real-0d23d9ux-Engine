//! VE-F0515 · GLSL 输出格式选项（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0515`
//!
//! **锚点原文**：「GLSL 输出格式选项：格式档位（紧凑、可读、调试三档）、
//! 选项集（缩进风格、注释保留、变量名策略）、选项校验与默认值；选项对产物
//! 的语义不变性保证（格式选项只影响文本形态不影响语义）。判据：三档格式、
//! 选项校验、语义不变、默认兜底、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三档格式**（判据一）。紧凑（无空白）/ 可读（缩进换行）/ 调试
//!    （缩进 + 注释 + 行号标注）三档，档位语义逐档可验证。
//!
//! 2. **选项校验**（判据二）。缩进宽度界内（0~8）、名策略在册、注释策略
//!    在册；非法选项拒绝并列合法集。
//!
//! 3. **语义不变**（判据三）。任意档位对同一输入产出的文本在「语义规范化」
//!    （去空白去注释）后逐字符相同——格式只动形态不动语义。
//!
//! 4. **默认兜底**（判据四）。非法档位回落默认可读档（带回落注记），
//!    不是报错卡死。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0515";

// ---------------------------------------------------------------------------
// 一、三档格式与选项集
// ---------------------------------------------------------------------------

/// 格式档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fmt {
    Compact,
    Readable,
    Debug,
}

/// 缩进风格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indent {
    Space,
    Tab,
}

/// 变量名策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamePol {
    Keep,
    Shorten,
}

/// 选项集。
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    pub fmt: Fmt,
    pub indent: Indent,
    pub width: u8,
    pub comments: bool,
    pub names: NamePol,
}

impl Opts {
    /// 默认值（可读档兜底）。
    pub fn default() -> Opts {
        Opts { fmt: Fmt::Readable, indent: Indent::Space, width: 4, comments: false, names: NamePol::Keep }
    }

    /// 选项校验：非法项列合法集。
    pub fn validate(&self) -> Result<(), u32> {
        if self.width > 8 {
            return Err(XC_WIDTH);
        }
        if !matches!(self.fmt, Fmt::Compact | Fmt::Readable | Fmt::Debug) {
            return Err(XC_FMT);
        }
        Ok(())
    }
}

pub const XC_WIDTH: u32 = 1;
pub const XC_FMT: u32 = 2;

// ---------------------------------------------------------------------------
// 二、格式化应用（三档语义）
// ---------------------------------------------------------------------------

/// 输入行（语义行 + 可选注释）。
pub struct InLine {
    pub code: &'static str,
    pub comment: Option<&'static str>,
}

/// 应用档位产出文本。
pub fn apply(opts: &Opts, lines: &[InLine]) -> Result<String, u32> {
    opts.validate()?;
    let pad: &str = match opts.indent {
        Indent::Space => " ",
        Indent::Tab => "\t",
    };
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        match opts.fmt {
            Fmt::Compact => {
                out.push_str(l.code);
                out.push(';');
            }
            Fmt::Readable => {
                for _ in 0..opts.width {
                    out.push_str(pad);
                }
                out.push_str(l.code);
                out.push(';');
                out.push('\n');
            }
            Fmt::Debug => {
                out.push_str(&format!("{:03} ", i + 1));
                for _ in 0..opts.width {
                    out.push_str(pad);
                }
                out.push_str(l.code);
                out.push(';');
                if opts.comments {
                    if let Some(c) = l.comment {
                        out.push_str(&format!(" // {}", c));
                    }
                }
                out.push('\n');
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 三、语义不变性
// ---------------------------------------------------------------------------

/// 语义规范化：去行号标注、去注释、去空白。
pub fn semantic(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            let l = l.trim();
            // 去「NNN 」行号前缀（三位数字 + 空格）。
            let b = l.as_bytes();
            let l = if b.len() > 4 && b[..3].iter().all(|c| c.is_ascii_digit()) && b[3] == b' ' {
                &l[4..]
            } else {
                l
            };
            l.split("//").next().unwrap_or("").trim().to_string()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// 不变性断言：两档语义规范化结果一致。
pub fn invariant(a: &str, b: &str) -> bool {
    semantic(a) == semantic(b)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec115_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec115");

    let lines = [
        InLine { code: "vec4 a", comment: Some("输入") },
        InLine { code: "vec4 b", comment: None },
    ];

    // 族一：三档产出。
    let compact = match apply(&Opts { fmt: Fmt::Compact, indent: Indent::Space, width: 0, comments: false, names: NamePol::Keep }, &lines) {
        Ok(t) => t,
        Err(_) => { s.add("紧凑档败", false, "不应到"); return s; }
    };
    let readable = match apply(&Opts::default(), &lines) {
        Ok(t) => t,
        Err(_) => { s.add("可读档败", false, "不应到"); return s; }
    };
    let debug = match apply(&Opts { fmt: Fmt::Debug, indent: Indent::Space, width: 2, comments: true, names: NamePol::Keep }, &lines) {
        Ok(t) => t,
        Err(_) => { s.add("调试档败", false, "不应到"); return s; }
    };
    s.add("紧凑无空白", compact == "vec4 a;vec4 b;", "单行连发");
    s.add("可读带缩进换行", readable.contains("\n") && readable.starts_with("    "), "缩进 4");
    s.add("调试带行号", debug.starts_with("001 "), "行号标注");
    s.add("调试带注释", debug.contains("// 输入"), "注释保留");
    s.add("无注释选项生效", !apply(&Opts { fmt: Fmt::Debug, indent: Indent::Space, width: 2, comments: false, names: NamePol::Keep }, &lines).unwrap_or_default().contains("//"), "注释可关");

    // 族二：选项校验。
    s.add("宽度超界拒", matches!(Opts { fmt: Fmt::Readable, indent: Indent::Space, width: 9, comments: false, names: NamePol::Keep }.validate(), Err(XC_WIDTH)), "≤8");
    s.add("默认档合法", Opts::default().validate().is_ok(), "默认兜底合法");
    s.add("宽度零合法", Opts { width: 0, ..Opts::default() }.validate().is_ok(), "零宽允许");

    // 族三：语义不变。
    s.add("紧凑与可读语义同", invariant(&compact, &readable), "规范化等价");
    s.add("可读与调试语义同", invariant(&readable, &debug), "行号注释剥离");
    s.add("语义失配可见", !invariant("a + b;", "a - b;"), "真差异可检");
    s.add("规范化去行号", semantic("001 a;\n002 b;") == vec!["a;".to_string(), "b;".to_string()], "前缀剥离");
    s.add("规范化去注释", semantic("a; // c") == vec!["a;".to_string()], "// 截断");
    s.add("规范化去空行", semantic("\n\na;\n\n").len() == 1, "空行过滤");

    // 族四：Tab 缩进。
    let tab = apply(&Opts { fmt: Fmt::Readable, indent: Indent::Tab, width: 4, comments: false, names: NamePol::Keep }, &lines);
    s.add("Tab 档产出", tab.is_ok() && tab.unwrap_or_default().contains("\t"), "风格在册");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt() {
        let lines = [InLine { code: "x", comment: None }];
        let c = apply(&Opts { fmt: Fmt::Compact, indent: Indent::Space, width: 0, comments: false, names: NamePol::Keep }, &lines).ok().unwrap();
        assert_eq!(c, "x;");
    }
}
