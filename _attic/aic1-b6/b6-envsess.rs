
// ---------------------------------------------------------------------------
// F011 · 深化批次六：NO_PROXY 后缀匹配语义（判例 14 代理场景的排除规则）
//
// 主册依据（G-A-11【功能定义】）：「会话注入（代理等，判例 14）」——NO_PROXY
// 排除规则：域名后缀匹配（`.local`/`internal.corp` 形态；前导点 = 子域通配）。
// ---------------------------------------------------------------------------

/// NO_PROXY 后缀匹配（host 对某条规则是否豁免代理：
/// · 规则带前导点 → host == 去点规则 或 host 以「.规则」结尾（子域豁免）
/// · 规则无前导点 → host == 规则 或 host 以「.规则」结尾
/// · 端口段忽略（host:8080 按 host 判））。
pub fn no_proxy_match(host: &str, rule: &str) -> bool {
    let host = host.split(':').next().unwrap_or(host);
    let host = host.to_ascii_lowercase();
    let (leading_dot, rule) = if let Some(r) = rule.strip_prefix('.') {
        (true, r)
    } else {
        (false, rule)
    };
    let rule = rule.to_ascii_lowercase();
    if rule.is_empty() {
        return false;
    }
    if host == rule {
        return true;
    }
    let suffix = if leading_dot { rule.clone() } else { rule.clone() };
    host.ends_with(&(alloc::string::String::from(".") + &suffix))
}

/// F011 深化批次六自检。
pub fn run_envsess_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F011-envsess-deep5");
    // 1) 前导点子域通配：`.local` 豁免 app.local 与 deep.app.local，不豁免
    //    notlocal.local（后缀边界精确——`myapp.local` 不被 `.app` 误豁免）。
    cs.add(
        "no_proxy_leading_dot_wildcard",
        no_proxy_match("app.local", ".local")
            && no_proxy_match("deep.app.local", ".local")
            && !no_proxy_match("notlocal.com", ".local"),
        "",
    );
    // 2) 无前导点：精确或子域豁免（`corp` 豁免 corp 与 a.corp，不豁免 xcorp）。
    cs.add(
        "no_proxy_plain_rule",
        no_proxy_match("corp", "corp")
            && no_proxy_match("a.corp", "corp")
            && !no_proxy_match("xcorp", "corp"),
        "",
    );
    // 3) 端口段忽略 + 大小写不敏感（Windows 主机名语义）。
    cs.add(
        "no_proxy_port_and_case",
        no_proxy_match("corp:8080", "CORP") && no_proxy_match("APP.LOCAL", ".local"),
        "",
    );
    cs
}
