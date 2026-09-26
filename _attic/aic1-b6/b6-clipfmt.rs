
// ---------------------------------------------------------------------------
// F017 · 深化批次六：剪贴板格式枚举快照（EnumClipboardFormats 同语义）
//
// 主册依据（G-A-17【功能定义】）：「多格式共存（同份数据多格式挂载）」——
// 枚举面：当前条目挂载的全部格式列举（诊断工具「格式详情」的数据源）。输入
// 为 ClipEntry.formats 挂载序切片（既有结构面——一处一事实）。
// ---------------------------------------------------------------------------

/// 格式枚举（挂载序保真；输出缓冲满如实截断——返回已枚举数）。
pub fn enumerate_formats(formats: &[u16], out: &mut [u16]) -> usize {
    let mut n = 0usize;
    for &cf in formats {
        if n >= out.len() {
            break;
        }
        out[n] = cf;
        n += 1;
    }
    n
}

/// F017 深化批次六自检。
pub fn run_clipfmt_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt-deep5");
    // 1) 多格式条目枚举：三格式挂载 → 枚举全数保真（格式序保持挂载序）。
    let mounted = [CF_UNICODETEXT, CF_TEXT, CF_DIB];
    let mut out = [0u16; 8];
    let n = enumerate_formats(&mounted, &mut out);
    cs.add(
        "enumerate_formats_mounted_order",
        n == 3 && out[0] == CF_UNICODETEXT && out[1] == CF_TEXT && out[2] == CF_DIB,
        "",
    );
    // 2) 小缓冲截断如实（只枚举前 2 个——不冒充全量）。
    let mut small = [0u16; 2];
    let n2 = enumerate_formats(&mounted, &mut small);
    cs.add(
        "enumerate_formats_truncation_honest",
        n2 == 2 && small[0] == CF_UNICODETEXT && small[1] == CF_TEXT,
        "",
    );
    // 3) 空挂载枚举 = 0（零格式不造假）。
    let none: [u16; 0] = [];
    let mut out3 = [0u16; 8];
    let n3 = enumerate_formats(&none, &mut out3);
    cs.add("enumerate_formats_empty", n3 == 0, "");
    cs
}
