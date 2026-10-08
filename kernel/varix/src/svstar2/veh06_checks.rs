//! VE-F1406 · 域自检（判据逐条对应，见 `veh06_audiotoken.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 语义映射 → `H06-语义-*`
//! - 三级覆盖 → `H06-覆盖-*`
//! - 主题联动 → `H06-主题-*`
//! - 缺省链 → `H06-缺省-*`
//! - 唯一来源 → `H06-唯一-*`

use super::veh06_audiotoken::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// VE-F1406 域自检。
pub fn run_veh06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh06");

    // ---- 判据：语义映射（解耦：换资源不改代码）----

    {
        let mut t = TokenTable::new();
        t.define(Level::System, "notification.pop", "resource=pop_default.wav;volume=700;latency=normal");
        // UI 只发语义名
        let r = t.resolve("notification.pop");
        match r {
            Resolved::Play(spec, lv) => {
                let ok = spec.resource == "pop_default.wav"
                    && spec.volume_permille == 700
                    && spec.latency == LatencyClass::Normal
                    && lv == Level::System;
                // 换资源不改代码：只改令牌表，语义名不变
                t.define(Level::System, "notification.pop", "resource=pop_new.wav;volume=800;latency=immediate");
                let r2 = t.resolve("notification.pop");
                let swapped = matches!(r2, Resolved::Play(ref s2, _) if s2.resource == "pop_new.wav"
                    && s2.volume_permille == 800
                    && s2.latency == LatencyClass::Immediate);
                set.add("H06-语义-解耦换资源不改码", ok && swapped, "");
            }
            _ => set.add("H06-语义-解耦换资源不改码", false, "应命中播放"),
        }
    }
    // 坏串静默降级不报错（解析失败 = 合法静默 + 遥测）
    {
        let mut t = TokenTable::new();
        t.define(Level::System, "bad.token", "volume=700;latency=normal"); // 缺 resource
        let r = t.resolve("bad.token");
        let silent = matches!(r, Resolved::Silent);
        let telemetried = t.parse_error_telemetry.iter().any(|(n, c)| n == "bad.token" && *c == 1);
        set.add("H06-语义-坏串静默降级", silent && telemetried, "");
    }

    // ---- 判据：三级覆盖（临时 > 应用级 > 系统级，最终值与来源可查询）----

    {
        let mut t = TokenTable::new();
        t.define(Level::System, "ui.click", "resource=click_sys.wav;volume=600;latency=immediate");
        t.define(Level::App, "ui.click", "resource=click_app.wav;volume=700;latency=immediate");
        // 应用级覆盖系统级
        let r1 = t.peek("ui.click");
        let app_wins = matches!(r1, Resolved::Play(ref s, Level::App) if s.resource == "click_app.wav");
        // 临时覆盖应用级
        t.define(Level::Temporary, "ui.click", "resource=click_tmp.wav;volume=900;latency=immediate");
        let r2 = t.peek("ui.click");
        let tmp_wins = matches!(r2, Resolved::Play(ref s, Level::Temporary) if s.resource == "click_tmp.wav");
        // 临时撤销 → 回落应用级（覆盖链回退）
        t.undefine(Level::Temporary, "ui.click");
        let r3 = t.peek("ui.click");
        let fallback = matches!(r3, Resolved::Play(ref s, Level::App) if s.resource == "click_app.wav");
        // 系统级只在没有更高层定义时生效
        t.undefine(Level::App, "ui.click");
        let r4 = t.peek("ui.click");
        let sys_back = matches!(r4, Resolved::Play(_, Level::System));
        set.add(
            "H06-覆盖-三级覆盖与回退",
            app_wins && tmp_wins && fallback && sys_back,
            "",
        );
    }

    // ---- 判据：主题联动（换肤连声音换，F3401 音频侧）----

    {
        let mut t = TokenTable::new();
        t.define(Level::System, "ui.open", "resource=open_a.wav;volume=600;latency=normal");
        let before = t.peek("ui.open");
        // 主题 A 携带音频令牌段
        t.apply_theme(&[(
            "ui.open".to_string(),
            "resource=open_themeA.wav;volume=750;latency=normal".to_string(),
        )]);
        let after_a = t.peek("ui.open");
        let themed = matches!(after_a, Resolved::Play(ref s, Level::Theme) if s.resource == "open_themeA.wav");
        // 主题级覆盖系统级但被应用级覆盖（主题是轴不是顶）
        t.define(Level::App, "ui.open", "resource=open_app.wav;volume=700;latency=normal");
        let app_over_theme = matches!(t.peek("ui.open"), Resolved::Play(ref s, Level::App) if s.resource == "open_app.wav");
        // 换主题 B：声音跟着换
        t.undefine(Level::App, "ui.open");
        t.apply_theme(&[(
            "ui.open".to_string(),
            "resource=open_themeB.wav;volume=650;latency=normal".to_string(),
        )]);
        let after_b = t.peek("ui.open");
        let switched = matches!(after_b, Resolved::Play(ref s, Level::Theme) if s.resource == "open_themeB.wav");
        set.add(
            "H06-主题-换肤连声音换",
            matches!(before, Resolved::Play(_, Level::System))
                && themed
                && app_over_theme
                && switched,
            "",
        );
    }

    // ---- 判据：缺省链（主题缺 → 系统默认 → 静默，永不报错）----

    {
        let mut t = TokenTable::new();
        // 全链无定义 → 静默（合法态）+ 缺失遥测
        let r1 = t.resolve("ghost.sound");
        let silent = matches!(r1, Resolved::Silent);
        let telemetried = t.missing_telemetry.iter().any(|(n, c)| n == "ghost.sound" && *c == 1);
        // 主题缺但系统有 → 系统默认生效
        t.define(Level::System, "ui.close", "resource=close_default.wav;volume=500;latency=normal");
        let r2 = t.peek("ui.close");
        let sys_default = matches!(r2, Resolved::Play(ref s, _) if s.resource == "close_default.wav");
        set.add(
            "H06-缺省-静默合法缺失显性",
            silent && telemetried && sys_default,
            "",
        );
    }

    // ---- 判据：唯一来源（多来源违规 + 硬编码拦截）----

    {
        let mut lint = SourceLint::new();
        lint.bind("按钮点击", "ui.click");
        // 误登记第二来源
        lint.bind("按钮点击", "ui.click_alt");
        // 调用点：一个合规一个硬编码
        let sites = vec![
            ("按钮点击".to_string(), CallStyle::ViaToken),
            ("拖拽释放".to_string(), CallStyle::RawResource),
        ];
        let v = lint.lint(&sites);
        let multi = v.iter().any(|x| x.code == "E_MULTI_SOURCE" && x.interaction == "按钮点击");
        let raw = v.iter().any(|x| x.code == "E_RAW_RESOURCE" && x.interaction == "拖拽释放");
        // 收敛为唯一来源后多来源违规消失（解除误登记）
        assert!(lint.unbind("按钮点击", "ui.click_alt"));
        let v2 = lint.lint(&sites);
        let multi_gone = !v2.iter().any(|x| x.code == "E_MULTI_SOURCE");
        set.add(
            "H06-唯一-多来源与硬编码拦截",
            multi && raw && multi_gone,
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let mut t = TokenTable::new();
        t.define(Level::System, "ui.click", "resource=click.wav;volume=700;latency=immediate");
        let s = resolved_summary(&t.peek("ui.click"), "ui.click");
        let s2 = resolved_summary(&Resolved::Silent, "ghost");
        set.add(
            "H06-读屏-解析摘要可播",
            s.contains("click.wav") && s.contains("系统级") && s2.contains("静默"),
            "",
        );
    }
    {
        let run = || {
            let mut t = TokenTable::new();
            t.define(Level::System, "a", "resource=a.wav;volume=500;latency=normal");
            t.apply_theme(&[("a".to_string(), "resource=t.wav;volume=600;latency=relaxed".to_string())]);
            t.define(Level::Temporary, "a", "resource=x.wav;volume=900;latency=immediate");
            let r = t.resolve("a");
            let _ = t.resolve("missing");
            (resolved_summary(&r, "a"), t.missing_telemetry.clone())
        };
        let a = run();
        let b = run();
        set.add("H06-确定-同操作同账面", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec};

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veh06_checks_all_green() {
        let set = run_veh06_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-H06 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 覆盖优先序的数学：临时 4 > 应用 3 > 主题 2 > 系统 1。
    #[test]
    fn precedence_order() {
        assert!(Level::Temporary.precedence() > Level::App.precedence());
        assert!(Level::App.precedence() > Level::Theme.precedence());
        assert!(Level::Theme.precedence() > Level::System.precedence());
    }

    /// 解析器：合法串全字段、缺 resource 拒、坏 latency 拒。
    #[test]
    fn spec_parser_matrix() {
        let ok = TokenSpec::parse("resource=a.wav;volume=250;latency=relaxed").unwrap();
        assert_eq!(ok.resource, "a.wav");
        assert_eq!(ok.volume_permille, 250);
        assert_eq!(ok.latency, LatencyClass::Relaxed);
        assert!(TokenSpec::parse("volume=250").is_none());
        assert!(TokenSpec::parse("resource=;volume=250").is_none());
        assert!(TokenSpec::parse("resource=a.wav;latency=bogus").is_none());
        // 越界音量按钳制语义（与 Send 电平同源）：99999 → 1000
        let clamped = TokenSpec::parse("resource=a.wav;volume=99999").unwrap();
        assert_eq!(clamped.volume_permille, 1000);
    }
}
