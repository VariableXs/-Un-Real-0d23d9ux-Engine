//! F192 内核参数白名单（secstar2 · G-G-22）——引导面不接受自由文本。
//!
//! **判据（主册）**：白名单三族参数全通；非法样本 20 个（拼错/超长/注入符）
//! 全拒且建议准确；钳制路径实测。
//!
//! **功能定义（主册 G-G-22）**：启动参数仅接受白名单三族：调试族（verbose/
//! log-level）/降级族（no-gui/safe-mode 前置）/兼容族（legacy-timer 等个案）；
//! 未知参数拒绝并记录。
//!
//! 【交互设计】参数错误画面：F173 语汇静态版+「未识别的启动参数」+最接近
//! 合法参数建议（编辑距离 1 建议）；合法参数清单在帮助 F119（开发者篇）；
//! 图形选单（F171）不暴露参数编辑（高级路径：文字选单 F171 降级态才有
//! ——双层防呆）。
//! 【数据与存储】白名单表编译期常量（变更走 ADR）；拒绝记录入日志环（F188）。
//! 【状态与异常】参数注入攻击（超长/特殊字符）→ 长度与字符集双截断+审计；
//! 合法参数值越界（log-level=99）→ 钳制+警告。
//! 【设计细节】三族定义文档化：调试 6 参/降级 3 参/兼容 4 参（初始清单——
//! 增补走 ADR）；参数语法 key=value 严格（无空格变体）；长度上限 128 字符/
//! 参数、总长 1KB；建议算法阈值 ≤2 编辑距离；F193 安全模式即降级族成员
//! （一处一事实）。
//!
//! 接缝纪律：与 cmdline（旧 F017 通用 key=value 框架）语法对齐（等号分隔、
//! flag 即空值）；拒绝记录注入 F188 日志环由调用方完成。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 白名单（编译期常量——变更走 ADR）
// ---------------------------------------------------------------------------

/// 参数上限：单参数 128 字符。
pub const PARAM_MAX_LEN: usize = 128;
/// 参数上限：总长 1KB。
pub const TOTAL_MAX_LEN: usize = 1024;
/// 建议算法阈值：编辑距离 ≤2。
pub const SUGGEST_MAX_DIST: usize = 2;

/// 参数族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamFamily {
    /// 调试族（verbose/log-level 等 6 参）。
    Debug,
    /// 降级族（no-gui/safe-mode 前置等 3 参）。
    Degrade,
    /// 兼容族（legacy-timer 等个案 4 参）。
    Compat,
}

/// 白名单条目。
#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub name: &'static str,
    pub family: ParamFamily,
    /// 值型：Flag（无值）或值域 [min,max]（整数钳制）。
    pub kind: ValueKind,
    pub note: &'static str,
}

/// 值型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// 布尔旗标（出现即真；显式 =false 关闭）。
    Flag,
    /// 整数值（界外钳制+警告）。
    Int(i64, i64),
}

/// 白名单表（调试 6 + 降级 3 + 兼容 4 = 13 参初始清单）。
pub const WHITELIST: [ParamSpec; 13] = [
    // 调试族（6）
    ParamSpec { name: "verbose", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "启动全程串口详录" },
    ParamSpec { name: "log-level", family: ParamFamily::Debug, kind: ValueKind::Int(0, 5), note: "日志级别 0-5" },
    ParamSpec { name: "log-sinks", family: ParamFamily::Debug, kind: ValueKind::Int(1, 3), note: "日志汇 1-3" },
    ParamSpec { name: "ktrace", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "内核打点" },
    ParamSpec { name: "panic_halt", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "panic 停机不复位" },
    ParamSpec { name: "selftest-only", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "只跑自检即重启" },
    // 降级族（3）——F193 安全模式即本族成员（一处一事实）
    ParamSpec { name: "no-gui", family: ParamFamily::Degrade, kind: ValueKind::Flag, note: "不进桌面" },
    ParamSpec { name: "safe-mode", family: ParamFamily::Degrade, kind: ValueKind::Flag, note: "安全模式（F193）" },
    ParamSpec { name: "no-third-drv", family: ParamFamily::Degrade, kind: ValueKind::Flag, note: "禁第三方驱动" },
    // 兼容族（4）
    ParamSpec { name: "legacy-timer", family: ParamFamily::Compat, kind: ValueKind::Flag, note: "传统定时器路径" },
    ParamSpec { name: "no-acpi", family: ParamFamily::Compat, kind: ValueKind::Flag, note: "停用 ACPI 表" },
    ParamSpec { name: "iommu-soft", family: ParamFamily::Compat, kind: ValueKind::Flag, note: "IOMMU 软件模拟" },
    ParamSpec { name: "memmap-core", family: ParamFamily::Compat, kind: ValueKind::Int(0, 1), note: "内存图保留策略 0-1" },
];

/// 查白名单。
pub fn find(name: &str) -> Option<&'static ParamSpec> {
    WHITELIST.iter().find(|p| p.name == name)
}

// ---------------------------------------------------------------------------
// Levenshtein（建议算法——简化版：两行滚动数组，零堆）
// ---------------------------------------------------------------------------

/// 编辑距离（≤上限早停——只用于 ≤2 阈值判定，超出直接返回上限+1）。
pub fn edit_distance_capped(a: &[u8], b: &[u8], cap: usize) -> usize {
    if a.len().abs_diff(b.len()) > cap {
        return cap + 1;
    }
    let mut prev: [usize; PARAM_MAX_LEN + 1] = [0; PARAM_MAX_LEN + 1];
    let mut cur: [usize; PARAM_MAX_LEN + 1] = [0; PARAM_MAX_LEN + 1];
    for (j, p) in prev.iter_mut().enumerate() {
        *p = j;
    }
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        let mut row_min = cur[0];
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
            row_min = row_min.min(cur[j + 1]);
        }
        if row_min > cap {
            return cap + 1;
        }
        prev.copy_from_slice(&cur);
    }
    prev[b.len()]
}

/// 最接近的合法参数建议（编辑距离 1..=2 内取最小者；并列取白名单序）。
/// 编辑距离 0（输入已是合法名）不构成「建议」——恒 None。
pub fn suggest(input: &str) -> Option<&'static str> {
    let bytes = input.as_bytes();
    let mut best: Option<(&'static str, usize)> = None;
    for spec in WHITELIST.iter() {
        let d = edit_distance_capped(bytes, spec.name.as_bytes(), SUGGEST_MAX_DIST);
        if d >= 1 && d <= SUGGEST_MAX_DIST && best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((spec.name, d));
        }
    }
    best.map(|(n, _)| n)
}

// ---------------------------------------------------------------------------
// 解析主体
// ---------------------------------------------------------------------------

/// 单参数解析结果。
///
/// 零堆纪律：verdict 载荷只存 'static（白名单名/原因枚举/建议名）——未知
/// 参数的原始名不出现在 verdict 里，由调用方按 token 序号拼进审计文本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamVerdict {
    /// 白名单旗标（真）。
    FlagOn(&'static str),
    /// 白名单旗标显式关（=false）。
    FlagOff(&'static str),
    /// 白名单整数值（已钳制）。
    Int(&'static str, i64),
    /// 值越界被钳制（+警告——审计面）。
    IntClamped(&'static str, i64),
    /// 白名单外参数（附建议——编辑距离 ≤2 才有）。
    NoSuchParam(Option<&'static str>),
    /// 名合法但值非法（旗标带非布尔值/整型值解析失败/整型缺值）——建议=该参数自身。
    BadValue(&'static str),
    /// 非法 token（超长/空名/字符集违规/注入样本）。
    Illegal(&'static str),
}

/// 白名单校验器（一行启动参数 → 逐 token 判定）。
/// 字符集防线见 [`ParamWhitelist::char_ok`]：只放行字母数字与 `-_=.,/:+`。
pub struct ParamWhitelist {
    /// 拒绝计数（入日志环 F188 的对账源）。
    pub rejections: u64,
    /// 钳制计数（值越界警告——审计面）。
    pub clamps: u64,
}

impl ParamWhitelist {
    pub fn new() -> ParamWhitelist {
        ParamWhitelist { rejections: 0, clamps: 0 }
    }

    /// 字符集合法性（注入防线：只放行保守集合——含可见 ASCII 安全子集）。
    fn char_ok(c: u8) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, b'-' | b'=' | b'_' | b'.' | b',' | b'/' | b':' | b'+')
    }

    /// 校验一个 token（key / key=value）。
    pub fn check_token(&mut self, tok: &str) -> ParamVerdict {
        // 长度防线（注入样本：超长 → 双截断语义=拒收+审计）。
        if tok.len() > PARAM_MAX_LEN {
            self.rejections += 1;
            return ParamVerdict::Illegal("参数超长（>128 字符）");
        }
        if tok.is_empty() {
            self.rejections += 1;
            return ParamVerdict::Illegal("空参数");
        }
        if !tok.bytes().all(Self::char_ok) {
            self.rejections += 1;
            return ParamVerdict::Illegal("含非法字符（注入防线）");
        }

        let (key, val) = match tok.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (tok, None),
        };
        // key 不能为空（=value 形态）。
        if key.is_empty() {
            self.rejections += 1;
            return ParamVerdict::Illegal("缺参数名");
        }

        let spec = match find(key) {
            Some(s) => s,
            None => {
                self.rejections += 1;
                return ParamVerdict::NoSuchParam(suggest(key));
            }
        };

        match (spec.kind, val) {
            (ValueKind::Flag, None) => ParamVerdict::FlagOn(spec.name),
            (ValueKind::Flag, Some("true")) => ParamVerdict::FlagOn(spec.name),
            (ValueKind::Flag, Some("false")) => ParamVerdict::FlagOff(spec.name),
            (ValueKind::Flag, Some(_)) => {
                // 旗标带非布尔值 → 严格语法拒绝（无空格变体、无值变体）。
                self.rejections += 1;
                ParamVerdict::BadValue(spec.name)
            }
            (ValueKind::Int(lo, hi), Some(v)) => match v.parse::<i64>() {
                Ok(n) if n >= lo && n <= hi => ParamVerdict::Int(spec.name, n),
                Ok(n) => {
                    let clamped = n.clamp(lo, hi);
                    self.clamps += 1;
                    ParamVerdict::IntClamped(spec.name, clamped)
                }
                Err(_) => {
                    self.rejections += 1;
                    ParamVerdict::BadValue(spec.name)
                }
            },
            (ValueKind::Int(_, _), None) => {
                self.rejections += 1;
                ParamVerdict::BadValue(spec.name)
            }
        }
    }

    /// 校验整行（空格分隔；总长 1KB 防线先行）。
    /// 返回逐 token 判定（ Illegal/Unknown 计数已累计——审计零静默）。
    pub fn check_line(&mut self, line: &str) -> Vec<ParamVerdict> {
        if line.len() > TOTAL_MAX_LEN {
            self.rejections += 1;
            return vec![ParamVerdict::Illegal("启动参数总长超限（>1KB）")];
        }
        line.split_whitespace().map(|t| self.check_token(t)).collect()
    }

    /// 全行是否干净通过（引导放行判据——任何 Unknown/Illegal 即拒）。
    pub fn line_ok(&self, verdicts: &[ParamVerdict]) -> bool {
        verdicts.iter().all(|v| {
            matches!(v, ParamVerdict::FlagOn(_) | ParamVerdict::FlagOff(_) | ParamVerdict::Int(_, _) | ParamVerdict::IntClamped(_, _))
        })
    }
}

impl Default for ParamWhitelist {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F192 自检（聚合进 secstar2 域）。
pub fn run_paramwl_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-paramwl");

    // 判据一：三族参数全通（13 参逐个过）。
    let mut wl = ParamWhitelist::new();
    let ok_line = "verbose log-level=3 safe-mode legacy-timer no-acpi iommu-soft memmap-core=1 \
        ktrace panic_halt selftest-only no-gui no-third-drv log-sinks=2";
    let vs = wl.check_line(ok_line);
    set.add("family all pass", vs.len() == 13 && wl.line_ok(&vs), "");
    set.add("flag on", vs.contains(&ParamVerdict::FlagOn("verbose")), "");
    set.add("int ok", vs.contains(&ParamVerdict::Int("log-level", 3)), "");
    set.add("flag off path", {
        let v = wl.check_token("verbose=false");
        v == ParamVerdict::FlagOff("verbose")
    }, "");

    // 判据二：非法样本 20 个全拒 + 建议准确（拼写错误建议最近合法名）。
    let mut wl2 = ParamWhitelist::new();
    let illegal_samples = [
        // 拼错（编辑距离 ≤2 → 建议）x6
        "verbos", "log-lvl=2", "saf-mode", "legaci-timer", "no-guii", "loglevel=1",
        // 注入符 x6
        "a$(reboot)", "x`id`", "p;reboot", "k|cat", "s&reboot", "v>log",
        // 超长 x1
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        // 空名/缺名 x2
        "=5", "log-level=",
        // 越界值 x3
        "log-level=99", "memmap-core=7", "log-sinks=-3",
        // 未知参数 x2
        "rootfs=ext9", "wine-dbg=1",
    ];
    let mut rejected = 0usize;
    let mut suggested = 0usize;
    for s in illegal_samples.iter() {
        let v = wl2.check_token(s);
        match v {
            ParamVerdict::Illegal(_) | ParamVerdict::NoSuchParam(_) | ParamVerdict::BadValue(_) => rejected += 1,
            ParamVerdict::IntClamped(_, _) => rejected += 1, // 越界=钳制+警告（拒绝原值）
            _ => {}
        }
        if let ParamVerdict::NoSuchParam(Some(sug)) = v {
            if find(sug).is_some() {
                suggested += 1;
            }
        }
    }
    set.add("20 illegal rejected", rejected == 20, "");
    set.add("suggestions valid", suggested >= 6, "");
    set.add("suggest verbos", suggest("verbos") == Some("verbose"), "");
    set.add("suggest saf-mode", suggest("saf-mode") == Some("safe-mode"), "");
    set.add("no suggestion for garbage", suggest("a$(reboot)").is_none(), "");
    // 拒绝账：拼错 6 + 注入 6 + 超长 1 + 空名 1 + 坏值 1 + 未知 2 = 17；
    // 越界 3 条走钳制账（clamps=3）不算拒绝——两条账各归各（一处一事实）。
    set.add("rejections counted", wl2.rejections == 17, "");
    set.add("clamps counted", wl2.clamps == 3, "");

    // 判据三：钳制路径（越界值钳进界+警告计数）。
    let mut wl3 = ParamWhitelist::new();
    let v = wl3.check_token("log-level=99");
    set.add("clamp value", v == ParamVerdict::IntClamped("log-level", 5), "");
    set.add("clamp counted", wl3.clamps == 1, "");
    let v2 = wl3.check_token("log-level=-1");
    set.add("clamp low", v2 == ParamVerdict::IntClamped("log-level", 0), "");
    let v3 = wl3.check_token("log-level=4");
    set.add("in range no clamp", v3 == ParamVerdict::Int("log-level", 4) && wl3.clamps == 2, "");

    // 总长 1KB 防线。
    let mut wl4 = ParamWhitelist::new();
    let long_line = "verbose ".repeat(200);
    let vs4 = wl4.check_line(&long_line);
    set.add("total len gate", vs4.len() == 1 && matches!(vs4[0], ParamVerdict::Illegal(_)), "");

    // F193 联动：safe-mode 是降级族成员（一处一事实）。
    set.add("safe-mode in degrade", find("safe-mode").map(|s| s.family) == Some(ParamFamily::Degrade), "");
    // 三族定义文档化：6+3+4=13。
    let (dbg, deg, com) = WHITELIST.iter().fold((0, 0, 0), |acc, s| match s.family {
        ParamFamily::Debug => (acc.0 + 1, acc.1, acc.2),
        ParamFamily::Degrade => (acc.0, acc.1 + 1, acc.2),
        ParamFamily::Compat => (acc.0, acc.1, acc.2 + 1),
    });
    set.add("family census", dbg == 6 && deg == 3 && com == 4, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f192_edit_distance_basics() {
        assert_eq!(edit_distance_capped(b"verbose", b"verbose", 2), 0);
        assert_eq!(edit_distance_capped(b"verbos", b"verbose", 2), 1);
        assert_eq!(edit_distance_capped(b"verbse", b"verbose", 2), 1);
        assert_eq!(edit_distance_capped(b"v", b"verbose", 2), 3, "capped early");
        assert_eq!(edit_distance_capped(b"xyz", b"safe-mode", 2), 3);
    }

    #[test]
    fn f192_suggest_picks_nearest() {
        // 与两个候选都近 → 取距离最小者。
        assert_eq!(suggest("log-lvl"), Some("log-level"));
        assert_eq!(suggest("iommu-sof"), Some("iommu-soft"));
        assert_eq!(suggest("memmap-core"), None, "exact name never suggests itself");
    }

    #[test]
    fn f192_line_rejects_any_unknown() {
        let mut wl = ParamWhitelist::new();
        let vs = wl.check_line("verbose bogus-param safe-mode");
        assert!(!wl.line_ok(&vs), "one unknown rejects the whole line");
        assert_eq!(wl.rejections, 1);
    }

    #[test]
    fn f192_flag_true_false_semantics() {
        let mut wl = ParamWhitelist::new();
        assert_eq!(wl.check_token("no-gui"), ParamVerdict::FlagOn("no-gui"));
        assert_eq!(wl.check_token("no-gui=true"), ParamVerdict::FlagOn("no-gui"));
        assert_eq!(wl.check_token("no-gui=false"), ParamVerdict::FlagOff("no-gui"));
        // 旗标带非法值 → 严格拒绝。
        assert_eq!(wl.check_token("no-gui=maybe"), ParamVerdict::BadValue("no-gui"));
    }

    #[test]
    fn f192_charset_gate() {
        let mut wl = ParamWhitelist::new();
        // 空格在 token 内不可能（split_whitespace），但控制字符/UTF-8 中文拒。
        assert!(matches!(wl.check_token("参数"), ParamVerdict::Illegal(_)));
        assert!(matches!(wl.check_token("log\tlevel"), ParamVerdict::Illegal(_)));
    }

    #[test]
    fn f192_mixed_line_full_pass() {
        let mut wl = ParamWhitelist::new();
        let vs = wl.check_line("log-level=5 verbose memmap-core=0");
        assert!(wl.line_ok(&vs));
        assert!(vs.contains(&ParamVerdict::Int("memmap-core", 0)));
    }

    #[test]
    fn f192_run_checks_pass() {
        assert!(run_paramwl_checks().all_passed());
    }
}
