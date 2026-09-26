# -*- coding: utf-8 -*-
"""F192 深化重构：变异查表直配对 + 白名单覆盖层查找。"""
import io

p = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\secstar2\paramwl.rs"
s = io.open(p, encoding="utf-8").read()

# 1) match_str_static2 → direct (a,b) pair table
old_fn = s[s.index("/// 两段静态串拼接成"):s.index("/// 单段静态串裁剪查表")]
new_fn = '''/// 两段静态串→'static 组合样本（编译期封闭表——运行期零分配）。
/// 注入形态：a=注入符 b=白名单名 → `a+b`；坏值形态：a=旗标名 b=坏值 → `a=b`。
fn match_str_static2(a: &str, b: &str) -> &'static str {
    match (a, b) {
        ("$(rm)", "verbose") => "$(rm)verbose",
        ("`id`", "verbose") => "`id`verbose",
        (";reboot", "verbose") => ";rebootverbose",
        ("|cat", "verbose") => "|catverbose",
        ("&whoami", "verbose") => "&whoamiverbose",
        (">log", "verbose") => ">logverbose",
        ("'", "verbose") => "'verbose",
        ("\\"".to_string().as_str(), "verbose") => "\\"verbose",
        ("$(rm)", "safe-mode") => "$(rm)safe-mode",
        ("`id`", "safe-mode") => "`id`safe-mode",
        (";reboot", "safe-mode") => ";rebootsafe-mode",
        ("|cat", "safe-mode") => "|catsafe-mode",
        ("&whoami", "safe-mode") => "&whoamisafe-mode",
        (">log", "safe-mode") => ">logsafe-mode",
        ("'", "safe-mode") => "'safe-mode",
        ("\\"".to_string().as_str(), "safe-mode") => "\\"safe-mode",
        ("$(rm)", "no-gui") => "$(rm)no-gui",
        ("`id`", "no-gui") => "`id`no-gui",
        (";reboot", "no-gui") => ";rebootno-gui",
        ("|cat", "no-gui") => "|catno-gui",
        ("&whoami", "no-gui") => "&whoamino-gui",
        (">log", "no-gui") => ">logno-gui",
        ("'", "no-gui") => "'no-gui",
        ("\\"".to_string().as_str(), "no-gui") => "\\"no-gui",
        ("verbose", "maybe") => "verbose=maybe",
        ("verbose", "1") => "verbose=1",
        ("verbose", "yes") => "verbose=yes",
        ("verbose", "on") => "verbose=on",
        ("verbose", "00") => "verbose=00",
        ("no-gui", "maybe") => "no-gui=maybe",
        ("no-gui", "1") => "no-gui=1",
        ("no-gui", "yes") => "no-gui=yes",
        ("no-gui", "on") => "no-gui=on",
        ("no-gui", "00") => "no-gui=00",
        ("ktrace", "maybe") => "ktrace=maybe",
        ("ktrace", "1") => "ktrace=1",
        ("ktrace", "yes") => "ktrace=yes",
        ("ktrace", "on") => "ktrace=on",
        ("ktrace", "00") => "ktrace=00",
        _ => "verbose=maybe",
    }
}

'''
s = s.replace(old_fn, new_fn)

# 2) whitelist overlay-aware lookup
s = s.replace(
    """    /// 校验一个 token（key / key=value）。
    pub fn check_token(&mut self, tok: &str) -> ParamVerdict {""",
    """    /// 校验一个 token（key / key=value）——基础白名单。
    pub fn check_token(&mut self, tok: &str) -> ParamVerdict {
        self.check_token_in(&[], tok)
    }

    /// 校验一个 token（覆盖层优先——ADR 增补参数走同一解析器同一防线）。
    pub fn check_token_in(&mut self, extra: &[&'static ParamSpec], tok: &str) -> ParamVerdict {""", 1)

s = s.replace(
    """        let spec = match find(key) {
            Some(s) => s,
            None => {
                self.rejections += 1;
                return ParamVerdict::NoSuchParam(suggest(key));
            }
        };""",
    """        let spec = match find_in(extra, key) {
            Some(s) => s,
            None => {
                self.rejections += 1;
                return ParamVerdict::NoSuchParam(suggest(key));
            }
        };""", 1)

s = s.replace(
    """    /// 校验整行（空格分隔；总长 1KB 防线先行）。
    /// 返回逐 token 判定（ Illegal/Unknown 计数已累计——审计零静默）。
    pub fn check_line(&mut self, line: &str) -> Vec<ParamVerdict> {
        if line.len() > TOTAL_MAX_LEN {
            self.rejections += 1;
            return vec![ParamVerdict::Illegal("启动参数总长超限（>1KB）")];
        }
        line.split_whitespace().map(|t| self.check_token(t)).collect()
    }""",
    """    /// 校验整行（空格分隔；总长 1KB 防线先行）。
    /// 返回逐 token 判定（ Illegal/Unknown 计数已累计——审计零静默）。
    pub fn check_line(&mut self, line: &str) -> Vec<ParamVerdict> {
        self.check_line_in(&[], line)
    }

    /// 整行校验（覆盖层版——行内每个 token 都先查覆盖层）。
    pub fn check_line_in(&mut self, extra: &[&'static ParamSpec], line: &str) -> Vec<ParamVerdict> {
        if line.len() > TOTAL_MAX_LEN {
            self.rejections += 1;
            return vec![ParamVerdict::Illegal("启动参数总长超限（>1KB）")];
        }
        line.split_whitespace().map(|t| self.check_token_in(extra, t)).collect()
    }""", 1)

s = s.replace(
    """/// 查白名单。
pub fn find(name: &str) -> Option<&'static ParamSpec> {
    WHITELIST.iter().find(|p| p.name == name)
}""",
    """/// 查白名单。
pub fn find(name: &str) -> Option<&'static ParamSpec> {
    WHITELIST.iter().find(|p| p.name == name)
}

/// 查白名单（覆盖层优先——ADR 增补先于编译期表）。
fn find_in(extra: &[&'static ParamSpec], name: &str) -> Option<&'static ParamSpec> {
    extra.iter().copied().find(|p| p.name == name).or_else(|| find(name))
}""", 1)

# 3) AdrLedger overlay_specs + checks via overlay
s = s.replace(
    """    /// 查询（覆盖层优先于基础表——新 ADR 可细化同族语义）。
    pub fn find(&self, name: &str) -> Option<&'static ParamSpec> {
        if let Some(p) = self.overlay.iter().find(|p| p.name == name) {
            // 覆盖层条目来自调用方 'static 入参——生命周期由 ADR 账持有。
            return Some(overlay_static(p.name));
        }
        find(name)
    }""",
    """    /// 查询（覆盖层优先于基础表——新 ADR 可细化同族语义）。
    pub fn find(&self, name: &str) -> Option<&'static ParamSpec> {
        if self.overlay.iter().any(|p| p.name == name) {
            return Some(overlay_static(name));
        }
        find(name)
    }

    /// 覆盖层快照（喂给 `check_token_in`/`check_line_in` 的查找域）。
    pub fn overlay_specs(&self) -> Vec<&'static ParamSpec> {
        self.overlay.iter().map(|p| overlay_static(p.name)).collect()
    }""", 1)

s = s.replace(
    '''    let v_adr = wl.check_token("gpu-passthru=1");
    set.add("adr param accepted", wl.line_ok(&[v_adr]), "覆盖层参数走同一解析器");''',
    '''    let specs = adr.overlay_specs();
    let v_adr = wl.check_token_in(&specs, "gpu-passthru=1");
    set.add("adr param accepted", wl.line_ok(&[v_adr]), "覆盖层参数走同一解析器");''', 1)

s = s.replace(
    '''        assert!(adr.find("earlyprintk").is_some());
        assert!(wl.line_ok(&[v_ep]));''',
    '''        assert!(adr.find("earlyprintk").is_some());
        let specs = adr.overlay_specs();
        assert!(wl.line_ok(&[wl.check_token_in(&specs, "earlyprintk")]));''', 1)

s = s.replace(
    '''        assert!(matches!(wl.check_token("earlyprintk"), ParamVerdict::NoSuchParam(_)));''',
    '''        assert!(
            matches!(wl.check_token("earlyprintk"), ParamVerdict::NoSuchParam(_)),
            "未批准的 ADR 参数不生效"
        );''', 1)

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("refactored ok")
