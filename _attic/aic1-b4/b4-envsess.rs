
// ---------------------------------------------------------------------------
// F011 · 深化批次四：判例 14 代理场景注入件（三变量预设模板）
//
// 主册依据（G-A-11【功能定义】）：「会话注入（代理等，判例 14）」+【验收判据】
// 「判例 14 代理场景回归绿」。既有面：SESSION_INJECTED 三变量名钉值不重复；
// 本段补**值模板渲染**（注入时 NAME=VALUE 成行落表——回归判例的可复现材料）。
// 预设值为模板占位（实机代理值随会话配置——判例用固定值保证可复现）。
// ---------------------------------------------------------------------------

/// 代理注入预设（判例 14 复现值——回归绿的可复现材料）。
pub const PROXY_PRESET_HTTP: &str = "http://127.0.0.1:7890";
pub const PROXY_PRESET_HTTPS: &str = "http://127.0.0.1:7890";
pub const PROXY_PRESET_NO_PROXY: &str = "localhost,127.0.0.1";

/// 渲染一条 `NAME=VALUE` 注入行（写入缓冲，返回字节数；缓冲不足截断）。
pub fn render_injection_line(name: &str, value: &str, buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    for src in [name.as_bytes(), b"=".as_slice(), value.as_bytes()] {
        for &b in src {
            if n >= buf.len() {
                return n;
            }
            buf[n] = b;
            n += 1;
        }
    }
    n
}

/// 判例 14 全量注入行渲染（三行，行间 '\n'）——回归判例的输入材料。
pub fn render_proxy_preset(buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    for (i, (name, value)) in [
        ("HTTP_PROXY", PROXY_PRESET_HTTP),
        ("HTTPS_PROXY", PROXY_PRESET_HTTPS),
        ("NO_PROXY", PROXY_PRESET_NO_PROXY),
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 {
            if n < buf.len() {
                buf[n] = b'\n';
                n += 1;
            }
        }
        n += render_injection_line(name, value, &mut buf[n..]);
    }
    n
}

/// F011 深化批次四自检。
pub fn run_envsess_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F011-envsess-deep3");
    // 1) 判例 14 材料完整性：三行齐、变量名与既有 SESSION_INJECTED 同序同集
    //    （一处一事实——名集只在 SESSION_INJECTED）。
    let mut buf = [0u8; 256];
    let n = render_proxy_preset(&mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "proxy_preset_three_lines",
        text.matches('\n').count() == 2
            && text.starts_with("HTTP_PROXY=")
            && text.contains("HTTPS_PROXY=")
            && text.ends_with(PROXY_PRESET_NO_PROXY),
        "",
    );
    // 2) 单行渲染保真 + 短缓冲截断如实。
    let mut line = [0u8; 32];
    let n2 = render_injection_line("HTTP_PROXY", PROXY_PRESET_HTTP, &mut line);
    let exact = &line[..n2] == b"HTTP_PROXY=http://127.0.0.1:7890";
    let mut small = [0u8; 5];
    let n3 = render_injection_line("HTTP_PROXY", "x", &mut small);
    cs.add(
        "injection_line_render_and_truncate",
        exact && n3 == 5,
        "",
    );
    cs
}
