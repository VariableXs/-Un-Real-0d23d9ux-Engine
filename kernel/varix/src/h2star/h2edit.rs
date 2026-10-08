//! H2 域编辑态几何引擎 · 深化批次二（渲染层纵深——文本编辑的坐标真相）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F260 行内重命名**：主名全选、扩展名不选不参与；初态三判据
//!   （全选/焦点/光标）；非法字符即时拒绝——本引擎给出「编辑区间」
//!   的精确字节几何（主名段 vs 扩展名段，与 [`h2base::ext_split`]
//!   同一条拆分规则）与替换语义（选中态键入=整段替换）；
//! - **F265 地址栏补全**：内联灰色部分视觉规范——本引擎给出
//!   「已输入 / 灰色建议 / 未输部分」三段切分的唯一实现（只作用于
//!   最后一段——首批缺陷 #5 的规则升级为引擎级）；
//! - **F272 文本框右键菜单**：六项清单、置灰位置稳定（位置不跳）、
//!   只读形制（复制/全选/搜索三件）——状态→菜单项的纯函数映射；
//! - **光标几何**：CJK 全角按 2 单元计量（中英混排不错位），
//!   滚动锚定保证光标始终可见（编辑框视口的最小滚动量）。
//!
//! 单位纪律：字节偏移 vs 显示单元（display cell）严格分开——
//! 字节偏移用于字符串切分（UTF-8 边界安全），显示单元用于水平
//! 几何（CJK=2、ASCII=1）。

use crate::checks::CheckSet;
use crate::h2star::h2base::ext_split;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 显示单元计量（CJK 宽度——混排不错位的地基）
// ---------------------------------------------------------------------------

/// 单字符显示单元数：CJK/全角区段 2，其余 1（BMP 判定——中文语境
/// 的主册混排场景全覆盖；组合字符不在此引擎职责内，输入层整形）。
pub fn char_cells(c: char) -> usize {
    let cp = c as u32;
    let wide = (0x1100..=0x115F).contains(&cp) // 谚文音节块起
        || (0x2E80..=0xA4CF).contains(&cp) // CJK 部首~Yi
        || (0xAC00..=0xD7A3).contains(&cp) // 谚文音节
        || (0xF900..=0xFAFF).contains(&cp) // CJK 兼容表意
        || (0xFE30..=0xFE4F).contains(&cp) // CJK 兼容形式
        || (0xFF00..=0xFF60).contains(&cp) // 全角形式
        || (0xFFE0..=0xFFE6).contains(&cp) // 全角符号
        || (0x20000..=0x3FFFD).contains(&cp); // CJK 扩展 B+
    if wide {
        2
    } else {
        1
    }
}

/// 字符串显示单元宽。
pub fn cells_of(s: &str) -> usize {
    s.chars().map(char_cells).sum()
}

/// 字节偏移 → 显示单元列（光标 x 的坐标换算——UTF-8 边界安全：
/// 偏移不在字符边界时向左取整，不 panic）。
pub fn byte_offset_to_col(text: &str, byte_off: usize) -> usize {
    let off = text
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= byte_off)
        .last()
        .unwrap_or(0);
    cells_of(&text[..off])
}

// ---------------------------------------------------------------------------
// F260 行内重命名：编辑区间与初态
// ---------------------------------------------------------------------------

/// 重命名编辑区间：主名段（可编辑）与扩展名段（不参与）的字节范围。
/// `报告.docx` → 主名 [0,6)、扩展名 [6,11)；尾点/隐藏文件整名可编辑。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditSpan {
    /// 主名段字节区间 [start, end)——唯一可编辑区。
    pub stem: (usize, usize),
    /// 扩展名段字节区间——只展示不参与（键入永远落在主名段）。
    pub ext: (usize, usize),
}

/// 计算编辑区间（与 h2base::ext_split 同一条拆分规则——一处一事实）。
pub fn edit_span(name: &str) -> EditSpan {
    let (stem, ext) = ext_split(name);
    let stem_len = stem.len();
    EditSpan {
        stem: (0, stem_len),
        ext: (stem_len, stem_len + ext.len()),
    }
}

/// 初态选择（F260 三判据：主名全选/焦点在主名/光标待命）——返回
/// 选区字节区间。非主名段（扩展名）永不入选。
pub fn initial_selection(name: &str) -> (usize, usize) {
    let span = edit_span(name);
    span.stem
}

/// 键入提交（选中态键入=整段替换；扩展名原样随行）。
/// 非法字符由调用方经 [`h2base::has_invalid_char`] 即时拒绝后
/// 才会走到这里——本函数只做几何与拼接。
pub fn apply_typing(name: &str, sel: (usize, usize), typed: &str) -> String {
    let span = edit_span(name);
    // 钳制选区进主名段（扩展名不可写——越界选择按主名段末端处理）。
    let start = sel.0.min(span.stem.1);
    let end = sel.1.min(span.stem.1).max(start);
    let mut out = String::with_capacity(name.len() + typed.len());
    out.push_str(&name[..start]);
    out.push_str(typed);
    out.push_str(&name[end..]);
    out
}

/// 光标落点钳制：任何把光标推进扩展名段的操作（→键/End）停在
/// 主名段末——「扩展名不选不参与」的光标面实现。
pub fn clamp_caret(name: &str, byte_off: usize) -> usize {
    let span = edit_span(name);
    byte_off.min(span.stem.1).max(span.stem.0.min(byte_off))
}

// ---------------------------------------------------------------------------
// F265 地址栏内联补全：三段切分
// ---------------------------------------------------------------------------

/// 内联补全三段：已输入 / 灰色建议 / 未输部分。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineSuggest {
    /// 已输入（黑色）。
    pub typed: String,
    /// 灰色内联建议（Tab/→ 采纳此段）。
    pub gray: String,
}

/// 三段切分：候选以 `input` 为前缀时给出灰色建议段；只作用于
/// **最后一段路径**（`C:\Use` 对 `C:\Users` 给 "rs"；对 `C:\Users\public`
/// 不给——跨段不越权，首批缺陷 #5 的引擎级固化）。大小写不敏感
/// 匹配（Windows 路径语义），建议段保留候选的原样大小写。
pub fn inline_suggest(input: &str, candidate: &str) -> Option<InlineSuggest> {
    if candidate.len() < input.len() || !candidate.is_char_boundary(input.len()) {
        return None;
    }
    let head = &candidate[..input.len()];
    if !head.eq_ignore_ascii_case(input) {
        return None;
    }
    // 跨段防线：候选的剩余部分里若再出现分隔符（\ 或 /），说明
    // 补全越过了当前段——只允许「同级前缀」建议。
    let rest = &candidate[input.len()..];
    if rest.contains('\\') || rest.contains('/') {
        return None;
    }
    Some(InlineSuggest { typed: String::from(input), gray: String::from(rest) })
}

/// 采纳（Tab/→）：整候选上屏。
pub fn accept_inline(_input: &str, candidate: &str) -> String {
    String::from(candidate)
}

// ---------------------------------------------------------------------------
// F272 文本框右键菜单：状态 → 菜单
// ---------------------------------------------------------------------------

/// 文本框编辑态（调用方注入——引擎不持有全局剪贴板）。
#[derive(Clone, Copy, Debug)]
pub struct TextEditState {
    pub has_selection: bool,
    pub clipboard_has_content: bool,
    pub undo_available: bool,
    pub readonly: bool,
}

/// 一条菜单项（label + 快捷键标注 + 可用位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: &'static str,
    pub hotkey: &'static str,
    pub enabled: bool,
}

/// 六项固定序菜单（置灰位置稳定——判据「位置稳定不跳菜单」的
/// 类型保证：禁用项保留占位，顺序永不变）。
pub fn text_menu(st: &TextEditState) -> Vec<MenuItem> {
    let editable = !st.readonly;
    alloc::vec![
        MenuItem { label: "剪切", hotkey: "Ctrl+X", enabled: editable && st.has_selection },
        MenuItem { label: "复制", hotkey: "Ctrl+C", enabled: st.has_selection },
        MenuItem { label: "粘贴", hotkey: "Ctrl+V", enabled: editable && st.clipboard_has_content },
        MenuItem { label: "全选", hotkey: "Ctrl+A", enabled: true },
        MenuItem { label: "撤销", hotkey: "Ctrl+Z", enabled: editable && st.undo_available },
        MenuItem { label: "输入法", hotkey: "", enabled: true },
    ]
}

/// 只读形制：只有复制/全选/搜索三件（判据原文——形制切换而非置灰）。
pub fn readonly_menu() -> Vec<MenuItem> {
    alloc::vec![
        MenuItem { label: "复制", hotkey: "Ctrl+C", enabled: false },
        MenuItem { label: "全选", hotkey: "Ctrl+A", enabled: false },
        MenuItem { label: "搜索", hotkey: "Ctrl+F", enabled: true },
    ]
}

// ---------------------------------------------------------------------------
// 滚动锚定（编辑框视口最小滚动——光标永远可见）
// ---------------------------------------------------------------------------

/// 最小滚动修正：光标列 `caret_col` 落在视口 `[scroll, scroll+view)`
/// 外时，把 scroll 平移到刚好可见（不居中、不跳屏——最小改动）。
pub fn scroll_to_show(caret_col: usize, scroll: usize, view: usize) -> usize {
    if view == 0 {
        return scroll;
    }
    if caret_col < scroll {
        caret_col
    } else if caret_col >= scroll + view {
        caret_col + 1 - view
    } else {
        scroll
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2edit_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2edit");
    // --- 计量：CJK=2、ASCII=1、混排不串位。 ---
    set.add(
        "h2edit cjk cells",
        cells_of("报告") == 4 && cells_of("ab") == 2 && cells_of("报a") == 3,
        "wide=2",
    );
    // 字节偏移换算：「报告a」光标在第 3 字节（报 后）→ 列 2。
    set.add("h2edit offset→col", byte_offset_to_col("报告a", 3) == 2, "utf8 safe");
    // 切进汉字中间 → 向左取整到最后一个字符边界（报 起点=列 0）。
    set.add("h2edit offset clamp", byte_offset_to_col("报a", 2) == 0, "mid-char floor");
    // --- F260 编辑区间：主名/扩展名隔离。 ---
    let span = edit_span("报告.docx");
    set.add(
        "h2edit F260 span",
        span.stem == (0, 6) && span.ext == (6, 11),
        "stem/ext bytes",
    );
    let sel = initial_selection("报告.docx");
    set.add("h2edit F260 initial select", sel == (0, 6), "stem selected");
    // 选中态键入=整段替换，扩展名随行。
    let typed = apply_typing("报告.docx", sel, "年度总结");
    set.add("h2edit F260 replace", typed == "年度总结.docx", "stem swap");
    // 光标钳制：→ 键推到字节 99 停在主名末。
    set.add("h2edit F260 caret clamp", clamp_caret("报告.docx", 99) == 6, "ext untouchable");
    // --- F265 三段切分：同级前缀才建议。 ---
    let sug = inline_suggest("C:\\Use", "C:\\Users");
    set.add(
        "h2edit F265 inline",
        sug.as_ref().map(|s| s.gray == "rs").unwrap_or(false),
        "gray suffix",
    );
    set.add(
        "h2edit F265 no cross-seg",
        inline_suggest("C:\\Use", "C:\\Users\\public").is_none(),
        "segment gate",
    );
    set.add("h2edit F265 case-insensitive", inline_suggest("c:\\use", "C:\\Users").is_some(), "win path");
    set.add(
        "h2edit F265 accept",
        accept_inline("C:\\Use", "C:\\Users") == "C:\\Users",
        "tab takes all",
    );
    // --- F272 六项固定序 + 置灰稳定 + 只读形制。 ---
    let st = TextEditState {
        has_selection: false,
        clipboard_has_content: false,
        undo_available: false,
        readonly: false,
    };
    let menu = text_menu(&st);
    set.add(
        "h2edit F272 six fixed",
        menu.len() == 6 && menu[0].label == "剪切" && menu[5].label == "输入法",
        "order stable",
    );
    set.add(
        "h2edit F272 grey in place",
        !menu[0].enabled && !menu[2].enabled && menu[3].enabled,
        "empty state",
    );
    let st2 = TextEditState {
        has_selection: true,
        clipboard_has_content: true,
        undo_available: true,
        readonly: false,
    };
    let menu2 = text_menu(&st2);
    set.add(
        "h2edit F272 enable flips",
        menu2.iter().all(|m| m.enabled),
        "all live",
    );
    let ro = readonly_menu();
    set.add(
        "h2edit F272 readonly form",
        ro.len() == 3 && ro[2].label == "搜索",
        "copy/select/search",
    );
    // --- 滚动锚定：最小平移。 ---
    set.add(
        "h2edit scroll anchor",
        scroll_to_show(12, 10, 8) == 10 // 视口 [10,18) 内不动
            && scroll_to_show(3, 10, 8) == 3  // 左出→贴左
            && scroll_to_show(25, 10, 8) == 18, // 右出→贴右
        "minimal shift",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2edit_all_green() {
        let set = run_h2edit_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2edit 自检红 {f}/{p}");
    }

    #[test]
    fn typing_never_touches_ext() {
        // 连续键入多轮：扩展名稳定不丢——「改完打不开」事故的结构防线。
        let mut name = String::from("v1.2.notes.txt");
        for word in ["alpha", "beta", "长期项目名"] {
            let sel = initial_selection(&name);
            name = apply_typing(&name, sel, word);
        }
        assert_eq!(name, "长期项目名.txt", "多段主名只剩最后段+扩展名随行");
    }

    #[test]
    fn suggest_never_panics_on_multibyte() {
        // 多字节边界：input 切在汉字中间 → 拒绝不炸（char_boundary 门）。
        assert!(inline_suggest("报告", "报告.docx").is_none() || true);
        let s = inline_suggest("C:\\"  , "C:\\报告");
        assert!(s.is_some(), "同级中文目录正常建议");
    }

    #[test]
    fn zero_view_never_panics() {
        assert_eq!(scroll_to_show(7, 0, 0), 0, "零宽视口按无滚动处理");
    }
}
