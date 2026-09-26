
// ---------------------------------------------------------------------------
// F013 · 深化批次五：.lnk 属性页六字段渲染（右键属性页的数据源）
//
// 主册依据（G-A-13【交互设计】）：「属性（属性页显示全部六字段可编辑）」——
// 六字段：目标路径/参数/工作目录/图标位置/热键/窗口风格。渲染面把六字段
/// 成行写出（属性页消费），截断如实。
// ---------------------------------------------------------------------------

/// 属性页六字段包（解析面的聚合视图——解析器既有面供给）。
#[derive(Clone, Copy)]
pub struct LnkProperty {
    pub target: &'static str,
    pub params: &'static str,
    pub workdir: &'static str,
    pub icon_loc: &'static str,
    pub hotkey: &'static str,
    pub show_cmd: &'static str,
}

/// 六行渲染（`字段名: 值`——缓冲不足行级截断，返回写字节数）。
pub fn render_property_rows(p: &LnkProperty, buf: &mut [u8]) -> usize {
    let rows = [
        ("目标", p.target),
        ("参数", p.params),
        ("工作目录", p.workdir),
        ("图标位置", p.icon_loc),
        ("热键", p.hotkey),
        ("窗口风格", p.show_cmd),
    ];
    let mut n = 0usize;
    for (i, (name, val)) in rows.iter().enumerate() {
        if i > 0 && n < buf.len() {
            buf[n] = b'\n';
            n += 1;
        }
        for src in [name.as_bytes(), b": ".as_slice(), val.as_bytes()] {
            for &b in src {
                if n >= buf.len() {
                    return n;
                }
                buf[n] = b;
                n += 1;
            }
        }
    }
    n
}

/// F013 深化批次五自检。
pub fn run_lnkfile_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile-deep4");
    // 1) 六行全渲染：字段名与值逐行保真、行数 = 5 个换行。
    let prop = LnkProperty {
        target: "C:\\Tools\\tool.exe",
        params: "--flag",
        workdir: "C:\\Tools",
        icon_loc: "C:\\Tools\\tool.exe,0",
        hotkey: "Ctrl+Alt+T",
        show_cmd: "常规窗口",
    };
    let mut buf = [0u8; 512];
    let n = render_property_rows(&prop, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "property_page_six_rows",
        text.matches('\n').count() == 5
            && text.contains("目标: C:\\Tools\\tool.exe")
            && text.contains("热键: Ctrl+Alt+T")
            && text.ends_with("窗口风格: 常规窗口"),
        "",
    );
    // 2) 空字段如实空值（未设参数 = 「参数: 」行仍在——六字段不缩水）。
    let emptyish = LnkProperty {
        target: "D:\\a.exe",
        params: "",
        workdir: "",
        icon_loc: "D:\\a.exe,0",
        hotkey: "无",
        show_cmd: "最大化",
    };
    let n2 = render_property_rows(&emptyish, &mut buf);
    let t2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    cs.add(
        "property_page_empty_fields_visible",
        t2.contains("参数: \n") && t2.contains("工作目录: \n"),
        "",
    );
    // 3) 短缓冲行级截断（不冒充完整页）。
    let mut small = [0u8; 10];
    let n3 = render_property_rows(&prop, &mut small);
    cs.add("property_page_truncation_honest", n3 == 10, "");
    cs
}
