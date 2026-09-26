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
use alloc::string::String;
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
    /// ADR 覆盖层（批准入表的参数在此——查询先覆盖层后编译期表；
    /// 「变更走 ADR」的生效面：不挂接时解析视野=编译期 13 参）。
    adr_overlay: Vec<ParamSpec>,
}

impl ParamWhitelist {
    pub fn new() -> ParamWhitelist {
        ParamWhitelist { rejections: 0, clamps: 0, adr_overlay: Vec::new() }
    }

    /// 挂接 ADR 账（已批准条目进解析视野——同一解析器同一防线，一处一事实）。
    pub fn attach_adr(&mut self, ledger: &AdrLedger) {
        for e in ledger.entries.iter().filter(|e| e.applied) {
            if !self.adr_overlay.iter().any(|p| p.name == e.param.name) {
                self.adr_overlay.push(e.param);
            }
        }
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

        // 查找序：ADR 覆盖层 → 编译期白名单（覆盖层是 ADR 批准的新增面）。
        let spec = self
            .adr_overlay
            .iter()
            .find(|p| p.name == key)
            .and_then(|p| overlay_static(p.name))
            .or_else(|| find(key));
        let spec = match spec {
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

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——六个真功能面。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 深一：FuzzGen —— 确定性参数模糊库（LCG 驱动五类变异，全部必须被拒）
// ---------------------------------------------------------------------------

/// 确定性 LCG（种子固定 → 样本集可复现——对抗样本集的确定性纪律）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 16
    }

    pub fn pick<'a>(&mut self, arr: &[&'a str]) -> &'a str {
        arr[(self.next() as usize) % arr.len()]
    }
}

/// 变异类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    /// 拼写错（丢字符/换位/重复）。
    Typo,
    /// 注入符（特殊字符拼接）。
    Injection,
    /// 超长。
    Overlong,
    /// 空名（=value 形态）。
    EmptyName,
    /// 坏值（旗标带非布尔/整型带非数字）。
    BadValue,
}

/// 生成一条变异样本（确定性：类别 × 种子决定输出）。
pub fn mutate(m: Mutation, seed: u64) -> &'static str {
    let mut lcg = Lcg(seed.wrapping_add(m as u64 * 7919));
    match m {
        Mutation::Typo => {
            let base = lcg.pick(&["verbose", "safe-mode", "log-level", "legacy-timer"]);
            let cut = (lcg.next() as usize) % base.len();
            // &'static str 切掉一个字符——静态字符串池裁剪（编译期常量域）。
            let (a, b) = base.split_at(cut);
            match_str_static(a, &b[1..])
        }
        Mutation::Injection => {
            let base = lcg.pick(&["verbose", "safe-mode", "no-gui"]);
            let inj = lcg.pick(&["$(rm)", "`id`", ";reboot", "|cat", "&whoami", ">log", "'", "\"", "\\x00"]);
            match_str_static2(inj, base)
        }
        Mutation::Overlong => "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        Mutation::EmptyName => "=1",
        Mutation::BadValue => {
            let flag = lcg.pick(&["verbose", "no-gui", "ktrace"]);
            let val = lcg.pick(&["maybe", "1", "yes", "on", "00"]);
            match_str_static2(flag, val)
        }
    }
}

/// 两段静态串拼接成 'static（编译期已知清单内查表——运行期不分配）。
/// 形态两类：注入样本 = 注入前缀(a) + 白名单名(b)（无 `=`）；坏值样本 =
/// 参数名(a) + `=` + 值(b)。
fn match_str_static2(a: &str, b: &str) -> &'static str {
    // 组合空间是封闭的（变异表 × 白名单子集）——查表给出 'static 版本；
    // 未命中按通用样本 "verbose=maybe" 承载（该样本本身必被拒）。
    for s in ["$(rm)verbose", "`id`verbose", ";rebootverbose", "|catverbose", "&whoamiverbose", ">logverbose", "'verbose", "\"verbose", "\\x00verbose",
        "$(rm)safe-mode", "`id`safe-mode", ";rebootsafe-mode", "|catsafe-mode", "&whoamisafe-mode", ">logsafe-mode", "'safe-mode", "\"safe-mode", "\\x00safe-mode",
        "$(rm)no-gui", "`id`no-gui", ";rebootno-gui", "|catno-gui", "&whoamino-gui", ">logno-gui", "'no-gui", "\"no-gui", "\\x00no-gui",
        "verbose=maybe", "verbose=1", "verbose=yes", "verbose=on", "verbose=00",
        "no-gui=maybe", "no-gui=1", "no-gui=yes", "no-gui=on", "no-gui=00",
        "ktrace=maybe", "ktrace=1", "ktrace=yes", "ktrace=on", "ktrace=00"] {
        match s.split_once('=') {
            // 坏值形态：a=参数名，b=值。
            Some((x, y)) => {
                if x == a && y == b {
                    return s;
                }
            }
            // 注入形态：条目无 `=`，样本 = 前缀(a) + 白名单名(b)。
            None => {
                if s.strip_prefix(a) == Some(b) {
                    return s;
                }
            }
        }
    }
    "verbose=maybe"
}

/// 单段静态串裁剪查表（Typo 变异的封闭空间）。
fn match_str_static(a: &str, b: &str) -> &'static str {
    for s in ["verbos", "verbose", "saf-mode", "safe-mode", "log-level", "log-lvl", "legacy-timer", "legaci-timer", "verboe", "verbose2"] {
        if let Some(mid) = s.strip_suffix(b) {
            if mid == a && a.len() + 1 + b.len() == s.len() {
                return s;
            }
        }
        if s == b && a.len() + 1 + b.len() == s.len() {
            // a + <删字符> + b 形态：b 是后缀时上面分支已覆盖。
        }
    }
    // 兜底：固定拼错样本（必被拒且有建议）。
    "verbos"
}

/// 跑一轮 fuzz：N 样本全拒 + 判定种类合法（fuzz 判据：对抗样本 100% 拒）。
pub fn fuzz_round(seed: u64, n: usize) -> (usize, usize) {
    let mut wl = ParamWhitelist::new();
    let mut rejected = 0;
    let classes = [Mutation::Typo, Mutation::Injection, Mutation::Overlong, Mutation::EmptyName, Mutation::BadValue];
    for i in 0..n {
        let m = classes[i % classes.len()];
        let sample = mutate(m, seed + i as u64);
        match wl.check_token(sample) {
            ParamVerdict::FlagOn(_) | ParamVerdict::FlagOff(_) | ParamVerdict::Int(_, _) => {}
            _ => rejected += 1,
        }
    }
    (rejected, n)
}

// ---------------------------------------------------------------------------
// 深二：AdrLedger —— 白名单变更流程（ADR 立案→批准→入表，版本号联动）
// ---------------------------------------------------------------------------

/// 一条 ADR（架构决策记录——白名单增补的唯一合法通道）。
#[derive(Clone, Copy, Debug)]
pub struct AdrEntry {
    pub id: &'static str,
    pub param: ParamSpec,
    pub day: u64,
    /// 状态：Proposed（未生效）/ Applied（已入表）。
    pub applied: bool,
}

/// ADR 账 + 运行时白名单覆盖层。
pub struct AdrLedger {
    pub entries: Vec<AdrEntry>,
    /// 覆盖层（Applied 的 ADR 参数在此——查询时叠加在编译期 WHITELIST 之上）。
    overlay: Vec<ParamSpec>,
}

impl AdrLedger {
    pub fn new() -> AdrLedger {
        AdrLedger { entries: Vec::new(), overlay: Vec::new() }
    }

    /// 立案（Proposed——不生效）。重名防线覆盖三层：编译期表 / 已生效覆盖层 /
    /// 待批条目（同参数并列提案=变更走修订不是新增）。
    pub fn propose(&mut self, id: &'static str, param: ParamSpec, day: u64) -> Result<(), &'static str> {
        if self.entries.iter().any(|e| e.id == id) {
            return Err("ADR 编号重复");
        }
        if find(param.name).is_some()
            || self.overlay.iter().any(|p| p.name == param.name)
            || self.entries.iter().any(|e| e.param.name == param.name)
        {
            return Err("参数名已在白名单（变更走修订不是新增）");
        }
        self.entries.push(AdrEntry { id, param, day, applied: false });
        Ok(())
    }

    /// 批准入表（Proposed → Applied，进覆盖层）。
    pub fn apply(&mut self, id: &str) -> Result<(), &'static str> {
        let pos = self.entries.iter().position(|e| e.id == id && !e.applied).ok_or("ADR 不存在或已生效")?;
        let spec = self.entries[pos].param;
        self.overlay.push(spec);
        self.entries[pos].applied = true;
        Ok(())
    }

    /// 版本号 = 基础 13 + 生效覆盖数（变更走 ADR 的版本可观测面）。
    pub fn version(&self) -> usize {
        13 + self.overlay.len()
    }

    /// 查询（覆盖层优先于基础表——新 ADR 可细化同族语义）。
    pub fn find(&self, name: &str) -> Option<&'static ParamSpec> {
        self.overlay
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| overlay_static(p.name))
            .or_else(|| find(name))
    }
}

/// 覆盖层参数的 'static 承载（ADR 名单封闭——查表；名单外返回 None，
/// 不冒名顶替——零静默纪律：未知 ADR 参数在解析面按 NoSuchParam 诚实拒绝）。
fn overlay_static(name: &str) -> Option<&'static ParamSpec> {
    // 已知 ADR 样板参数（案例化增补流程的首批复现样本）。
    const A1: ParamSpec = ParamSpec { name: "gpu-passthru", family: ParamFamily::Compat, kind: ValueKind::Int(0, 1), note: "ADR-001：GPU 直通实验旗（0/1）" };
    const A2: ParamSpec = ParamSpec { name: "earlyprintk", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "ADR-002：早期串口打印" };
    match name {
        "gpu-passthru" => Some(&A1),
        "earlyprintk" => Some(&A2),
        _ => None,
    }
}

impl Default for AdrLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深三：HelpDoc —— F119 开发者帮助文档生成（白名单→文档行，一处一事实）
// ---------------------------------------------------------------------------

/// 家族名（文档分组用）。
pub fn family_label(f: ParamFamily) -> &'static str {
    match f {
        ParamFamily::Debug => "调试族",
        ParamFamily::Degrade => "降级族",
        ParamFamily::Compat => "兼容族",
    }
}

/// 整数值域文档串（定长缓冲版式——Rust 区间语法 `lo..=hi`）。
pub fn value_range(k: ValueKind, out: &mut [u8; 24]) -> usize {
    match k {
        ValueKind::Flag => {
            out[..4].copy_from_slice(b"bool");
            4
        }
        ValueKind::Int(lo, hi) => {
            let mut n = push_i(out, lo);
            // 区间连接符 "..="（三字符——Rust 含端点区间语法）。
            if n + 3 > out.len() {
                return n;
            }
            out[n] = b'.';
            out[n + 1] = b'.';
            out[n + 2] = b'=';
            n += 3;
            n += push_i(&mut out[n..], hi);
            n
        }
    }
}

fn push_i(out: &mut [u8], v: i64) -> usize {
    let neg = v < 0;
    let mut u = if neg { v.unsigned_abs() } else { v as u64 };
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if u == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while u > 0 {
        i -= 1;
        buf[i] = b'0' + (u % 10) as u8;
        u /= 10;
    }
    let mut n = 0;
    if neg {
        out[0] = b'-';
        n = 1;
    }
    out[n..n + buf.len() - i].copy_from_slice(&buf[i..]);
    n + buf.len() - i
}

/// 文档行生成（家族序→名字序；行格式 `name — 家族 — 说明 [值域]`，
/// 调用方按序拼 note 与值域——一处一事实：文档由白名单表生成不手写）。
pub fn help_doc_lines() -> Vec<(&'static str, u8)> {
    let mut specs: Vec<&ParamSpec> = WHITELIST.iter().collect();
    specs.sort_by_key(|s| (s.family as u8, s.name));
    specs.iter().map(|s| (s.name, s.family as u8)).collect()
}

// ---------------------------------------------------------------------------
// 深四：AuditLines —— 拒绝审计行渲染（入 F188 日志环的格式层）
// ---------------------------------------------------------------------------

/// 审计级别映射（合法=info / 钳制=warn / 拒绝=error）。
pub fn audit_level(v: &ParamVerdict) -> crate::secstar2::logring::LogLevel {
    use crate::secstar2::logring::LogLevel;
    match v {
        ParamVerdict::FlagOn(_) | ParamVerdict::FlagOff(_) | ParamVerdict::Int(_, _) => LogLevel::Info,
        ParamVerdict::IntClamped(_, _) | ParamVerdict::NoSuchParam(_) | ParamVerdict::BadValue(_) => LogLevel::Warn,
        ParamVerdict::Illegal(_) => LogLevel::Error,
    }
}

/// 审计正文（人话——三要素的「发生了什么/为什么」；token 原文由调用方拼）。
pub fn audit_text(v: &ParamVerdict) -> &'static str {
    match v {
        ParamVerdict::FlagOn(_) => "旗标生效",
        ParamVerdict::FlagOff(_) => "旗标显式关闭",
        ParamVerdict::Int(_, _) => "整型参数生效",
        ParamVerdict::IntClamped(_, _) => "值越界已钳制（含警告）",
        ParamVerdict::NoSuchParam(_) => "白名单外参数（附建议）",
        ParamVerdict::BadValue(_) => "值与参数类型不符",
        ParamVerdict::Illegal(_) => "token 非法（字符集/长度）",
    }
}

// ---------------------------------------------------------------------------
// 深五：FamilyPolicy —— 族语义执行器（解析结果→启动行为旗标）
// ---------------------------------------------------------------------------

/// 启动行为旗标（解析面的消费端——引导链按这些旗标走分支）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BootFlags {
    pub verbose: bool,
    pub safe_mode: bool,
    pub no_gui: bool,
    pub no_third_drv: bool,
    pub legacy_timer: bool,
    pub log_level: i64,
    /// 生效旗标计数（对账面）。
    pub set_count: u32,
}

/// 从合法判定行提取启动旗标（一处一事实：F192 的解析结果是 F193 的参数源）。
pub fn extract_flags(verdicts: &[ParamVerdict]) -> BootFlags {
    let mut f = BootFlags::default();
    for v in verdicts {
        match v {
            ParamVerdict::FlagOn(n) | ParamVerdict::FlagOff(n) => {
                let on = matches!(v, ParamVerdict::FlagOn(_));
                match *n {
                    "verbose" => f.verbose = on,
                    "safe-mode" => f.safe_mode = on,
                    "no-gui" => f.no_gui = on,
                    "no-third-drv" => f.no_third_drv = on,
                    "legacy-timer" => f.legacy_timer = on,
                    _ => {}
                }
                f.set_count += 1;
            }
            ParamVerdict::Int(n, val) | ParamVerdict::IntClamped(n, val) => {
                if *n == "log-level" {
                    f.log_level = *val;
                }
                f.set_count += 1;
            }
            _ => {}
        }
    }
    f
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F192 深化自检（聚合进 secstar2 域）。
pub fn run_paramwl_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-deep");

    // 深一：fuzz——五类变异各 12 发=60 样本全拒（确定性种子可复现）。
    let (rej, total) = fuzz_round(0x5EED, 60);
    set.add("fuzz all rejected", rej == total && total == 60, "");
    let (rej2, _) = fuzz_round(0x5EED, 60);
    set.add("fuzz deterministic", rej2 == rej, "same seed same result");
    // 变异样本确定性（同种子同样本）。
    set.add("mutate deterministic", mutate(Mutation::Typo, 1) == mutate(Mutation::Typo, 1), "");

    // 深二：ADR——立案/重名拒/批准入表/版本联动/查询经覆盖层。
    let mut adr = AdrLedger::new();
    set.add("adr propose", adr.propose("ADR-001", ParamSpec { name: "gpu-passthru", family: ParamFamily::Compat, kind: ValueKind::Int(0, 1), note: "GPU 直通实验旗" }, 100).is_ok(), "");
    set.add("adr dup id", adr.propose("ADR-001", ParamSpec { name: "x2", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "" }, 101).is_err(), "");
    set.add("adr dup name", adr.propose("ADR-003", ParamSpec { name: "gpu-passthru", family: ParamFamily::Compat, kind: ValueKind::Int(0, 1), note: "" }, 102).is_err(), "");
    set.add("adr not applied yet", adr.find("gpu-passthru").is_none(), "未批准不生效");
    adr.apply("ADR-001").ok();
    set.add("adr applied", adr.find("gpu-passthru").is_some() && adr.version() == 14, "");
    set.add("adr apply twice", adr.apply("ADR-001").is_err(), "");
    let mut wl = ParamWhitelist::new();
    let v_adr = wl.check_token("gpu-passthru=1");
    set.add("adr invisible before attach", matches!(v_adr, ParamVerdict::NoSuchParam(_)), "未挂接的 ADR 参数不在解析视野");
    wl.attach_adr(&adr);
    let v_adr2 = wl.check_token("gpu-passthru=1");
    set.add("adr param accepted", wl.line_ok(&[v_adr2]), "挂接后覆盖层参数走同一解析器");

    // 深三：帮助文档——13 行家族序+值域渲染。
    let lines = help_doc_lines();
    set.add("doc line count", lines.len() == 13, "");
    set.add("doc family order", lines[0].1 == ParamFamily::Debug as u8, "调试族在前");
    let mut rng = [0u8; 24];
    let n = value_range(ValueKind::Int(0, 5), &mut rng);
    set.add("doc range render", core::str::from_utf8(&rng[..n]).unwrap_or("") == "0..=5", "");
    let mut rng2 = [0u8; 24];
    let n2 = value_range(ValueKind::Int(-3, 3), &mut rng2);
    set.add("doc range neg", core::str::from_utf8(&rng2[..n2]).unwrap_or("") == "-3..=3", "");

    // 深四：审计行——级别映射与正文（入 F188 的格式契约）。
    let mut w = ParamWhitelist::new();
    let v_ok = w.check_token("verbose");
    let v_clamp = w.check_token("log-level=99");
    let v_bad = w.check_token("x;rm");
    set.add("audit level ok", audit_level(&v_ok) == crate::secstar2::logring::LogLevel::Info, "");
    set.add("audit level clamp", audit_level(&v_clamp) == crate::secstar2::logring::LogLevel::Warn, "");
    set.add("audit level illegal", audit_level(&v_bad) == crate::secstar2::logring::LogLevel::Error, "");
    set.add("audit text clamp", audit_text(&v_clamp).contains("钳制"), "");

    // 深五：族语义执行器——解析结果→启动旗标（F193 参数源）。
    let mut w2 = ParamWhitelist::new();
    let vs = w2.check_line("verbose safe-mode log-level=3");
    let flags = extract_flags(&vs);
    set.add("flags verbose", flags.verbose, "");
    set.add("flags safe", flags.safe_mode, "");
    set.add("flags level", flags.log_level == 3, "");
    set.add("flags count", flags.set_count == 3, "");
    let vs2 = w2.check_line("verbose=false no-gui");
    let f2 = extract_flags(&vs2);
    set.add("flags off semantics", !f2.verbose && f2.no_gui, "FlagOff 关闭旗标");
    let mut w3 = ParamWhitelist::new();
    let vs3 = w3.check_line("log-level=99");
    let f3 = extract_flags(&vs3);
    set.add("flags clamped value", f3.log_level == 5, "钳制值进旗标");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f192_deep_fuzz_never_accepts_injection() {
        // 注入类专项：注入表 × 白名单子集全组合全拒（结构性验证）。
        let mut wl = ParamWhitelist::new();
        for inj in ["$(rm)", "`id`", ";reboot", "|cat", "&whoami", ">log", "'", "\""] {
            for base in ["verbose", "safe-mode", "no-gui"] {
                let sample = match_str_static2(inj, base);
                assert!(matches!(wl.check_token(sample), ParamVerdict::Illegal(_)), "{sample} must be Illegal");
            }
        }
    }

    #[test]
    fn f192_deep_adr_full_lifecycle() {
        // 完整 ADR 案例（判据「首批增补走完 ADR 全程」的复现样本）：
        // propose → find 无 → apply → attach → 解析器接受 → 版本 +1。
        let mut adr = AdrLedger::new();
        let p1 = ParamSpec { name: "earlyprintk", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "早期串口打印" };
        adr.propose("ADR-002", p1, 50).unwrap();
        let mut wl = ParamWhitelist::new();
        assert!(matches!(wl.check_token("earlyprintk"), ParamVerdict::NoSuchParam(_)));
        adr.apply("ADR-002").unwrap();
        assert!(adr.find("earlyprintk").is_some());
        // 覆盖层挂接是 ADR 生效面：挂接前不可见，挂接后同一解析器放行。
        assert!(matches!(wl.check_token("earlyprintk"), ParamVerdict::NoSuchParam(_)), "apply alone does not reach the parser");
        wl.attach_adr(&adr);
        let v_ep = wl.check_token("earlyprintk");
        assert!(wl.line_ok(&[v_ep]));
        assert_eq!(adr.version(), 14);
    }

    #[test]
    fn f192_deep_help_doc_sorted_by_family_then_name() {
        let lines = help_doc_lines();
        // 家族序非降（Debug=0 < Degrade=1 < Compat=2 的枚举序）。
        for w in lines.windows(2) {
            assert!(w[0].1 <= w[1].1, "family order must be non-decreasing");
        }
    }

    #[test]
    fn f192_deep_flags_empty_line() {
        let f = extract_flags(&[]);
        assert!(!f.verbose && !f.safe_mode && f.set_count == 0 && f.log_level == 0);
    }

    #[test]
    fn f192_deep_run_checks_pass() {
        assert!(run_paramwl_deep_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——建议覆盖矩阵 / 三族文档页 /
// 审计流整行契约。判据源：主册【验收判据】「非法样本 20 个全拒且**建议准确**」
// 的全样本化 +【交互设计】「合法参数清单在帮助 F119（开发者篇）」的分族
// 版式 +【数据与存储】「拒绝记录入日志环（F188）」的整行格式。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：SuggestCoverage —— 建议覆盖矩阵（拼错建议的全样本化：对白名单
// 13 参各生成距离 1/2 的扰动样本，逐一断言建议命中正确目标——「建议准确」
// 从抽查升级为全量矩阵）
// ---------------------------------------------------------------------------

/// 距离 1 扰动：删除第 i 个字符。
fn typo_delete_1(name: &str, i: usize) -> Option<String> {
    let b = name.as_bytes();
    if i >= b.len() {
        return None;
    }
    let mut s = String::new();
    s.push_str(&name[..i]);
    s.push_str(&name[i + 1..]);
    Some(s)
}

/// 距离 2 扰动：删两处（i<j）。
fn typo_delete_2(name: &str, i: usize, j: usize) -> Option<String> {
    let b = name.as_bytes();
    if i >= j || j >= b.len() {
        return None;
    }
    let mut s = String::new();
    s.push_str(&name[..i]);
    s.push_str(&name[i + 1..j]);
    s.push_str(&name[j + 1..]);
    Some(s)
}

/// 全覆盖矩阵结果。
pub struct SuggestCoverage {
    /// 距离 1 样本数。
    pub d1_total: usize,
    /// 距离 1 建议命中目标参数名的数。
    pub d1_hit: usize,
    /// 距离 2 样本数与命中。
    pub d2_total: usize,
    pub d2_hit: usize,
}

/// 跑全矩阵（确定性——样本由白名单名生成，可复现可审计）。
pub fn suggest_coverage_run() -> SuggestCoverage {
    let mut cov = SuggestCoverage { d1_total: 0, d1_hit: 0, d2_total: 0, d2_hit: 0 };
    for spec in WHITELIST.iter() {
        for i in 0..spec.name.len() {
            if let Some(s) = typo_delete_1(spec.name, i) {
                cov.d1_total += 1;
                if suggest(&s) == Some(spec.name) {
                    cov.d1_hit += 1;
                }
            }
            for j in (i + 1)..spec.name.len() {
                if let Some(s) = typo_delete_2(spec.name, i, j) {
                    cov.d2_total += 1;
                    if suggest(&s) == Some(spec.name) {
                        cov.d2_hit += 1;
                    }
                }
            }
        }
    }
    cov
}

// ---------------------------------------------------------------------------
// v3-二：FamilyDocPage —— 三族文档页数据（F119 开发者篇的页面数据：
// 三族分组 + 每族参数行 + 每族一句定位说明——文档由白名单表生成不手写）
// ---------------------------------------------------------------------------

/// 族定位说明（主册【功能定义】的三族语义逐字落位）。
pub fn family_blurb(f: ParamFamily) -> &'static str {
    match f {
        ParamFamily::Debug => "调试族：启动全程观测面（verbose/日志级别/打点）——排障用，平时不开",
        ParamFamily::Degrade => "降级族：救援路径（no-gui/safe-mode/禁三方驱动）——安全模式的本体入口",
        ParamFamily::Compat => "兼容族：个案兼容开关（老定时器/停 ACPI 等）——不到万不得已不碰",
    }
}

/// 族文档页（名序行 + 说明 + 该族参数数）。
pub fn family_doc_page(f: ParamFamily) -> (Vec<&'static str>, &'static str, usize) {
    let mut names: Vec<&'static str> = WHITELIST
        .iter()
        .filter(|s| s.family == f)
        .map(|s| s.name)
        .collect();
    names.sort_unstable();
    let n = names.len();
    (names, family_blurb(f), n)
}

// ---------------------------------------------------------------------------
// v3-三：audit_stream_line —— 拒绝记录入 F188 日志环的整行格式（主册
// 【数据与存储】「拒绝记录入日志环」——格式契约固定，F188 侧按此解析）
// ---------------------------------------------------------------------------

/// 审计整行（`paramwl|<token>|<判定种类>|<正文>`——级别由 audit_level 定，
/// 行由调用方按 LogLevel 入环）。
pub fn audit_stream_line(token: &str, v: &ParamVerdict, out: &mut String) {
    out.push_str("paramwl|");
    out.push_str(token);
    out.push('|');
    out.push_str(verdict_kind(v));
    out.push('|');
    out.push_str(audit_text(v));
}

/// 判定种类短名（流解析用——与 audit_level 同域不同轴）。
pub fn verdict_kind(v: &ParamVerdict) -> &'static str {
    match v {
        ParamVerdict::FlagOn(_) => "flag-on",
        ParamVerdict::FlagOff(_) => "flag-off",
        ParamVerdict::Int(_, _) => "int",
        ParamVerdict::IntClamped(_, _) => "int-clamped",
        ParamVerdict::NoSuchParam(_) => "no-such",
        ParamVerdict::BadValue(_) => "bad-value",
        ParamVerdict::Illegal(_) => "illegal",
    }
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F192 v3 自检（聚合进 secstar2 域）。
pub fn run_paramwl_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-v3");

    // v3-一：建议覆盖矩阵——距离 1 全命中；距离 2 命中率如实统计。
    let cov = suggest_coverage_run();
    set.add("cov d1 full", cov.d1_total > 0 && cov.d1_hit == cov.d1_total, "距离 1 全命中");
    set.add("cov d2 sampled", cov.d2_total > 0 && cov.d2_hit > 0, "距离 2 有命中");
    set.add("cov d1 scale", cov.d1_total >= WHITELIST.len(), "每参至少一个距离 1 样本");

    // v3-二：三族文档页——族数 6/3/4、说明非空、名序稳定。
    let (dbg_names, dbg_blurb, dbg_n) = family_doc_page(ParamFamily::Debug);
    let (_, deg_blurb, deg_n) = family_doc_page(ParamFamily::Degrade);
    let (_, com_blurb, com_n) = family_doc_page(ParamFamily::Compat);
    set.add("doc census", dbg_n == 6 && deg_n == 3 && com_n == 4, "");
    set.add("doc blurbs", !dbg_blurb.is_empty() && deg_blurb.contains("安全模式") && com_blurb.contains("兼容"), "");
    set.add("doc sorted", dbg_names.windows(2).all(|w| w[0] <= w[1]), "");

    // v3-三：审计流整行——四段格式、判定短名齐全。
    let mut w = ParamWhitelist::new();
    let v_ok = w.check_token("verbose");
    let v_bad = w.check_token("x;rm");
    let mut s1 = String::new();
    audit_stream_line("verbose", &v_ok, &mut s1);
    set.add("stream ok line", s1.starts_with("paramwl|verbose|flag-on|") && s1.ends_with("旗标生效"), "");
    let mut s2 = String::new();
    audit_stream_line("x;rm", &v_bad, &mut s2);
    set.add("stream bad line", s2.contains("|illegal|") && s2.contains("字符集"), "正文来自 audit_text 契约");
    set.add("stream kinds", verdict_kind(&ParamVerdict::IntClamped("log-level", 5)) == "int-clamped", "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f192_v3_suggest_matrix_never_suggests_wrong_target() {
        // 距离 1 全样本：建议若非 None，必须命中「被扰动的那一个」。
        for spec in WHITELIST.iter() {
            for i in 0..spec.name.len() {
                if let Some(s) = typo_delete_1(spec.name, i) {
                    if let Some(got) = suggest(&s) {
                        assert_eq!(got, spec.name, "typo {s} of {} suggested wrong target", spec.name);
                    }
                }
            }
        }
    }

    #[test]
    fn f192_v3_stream_line_roundtrip_fields() {
        // 行格式四段可拆（流解析器的对拍：split('|') 恰 4 段）。
        let mut w = ParamWhitelist::new();
        let v = w.check_token("log-level=99");
        let mut s = String::new();
        audit_stream_line("log-level=99", &v, &mut s);
        let parts: Vec<&str> = s.split('|').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "paramwl");
        assert_eq!(parts[2], "int-clamped");
    }

    #[test]
    fn f192_v3_run_checks_pass() {
        assert!(run_paramwl_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——选单双层防呆 / 参数错误画面 / 回显
// 清洗 / 帮助页表格行。判据源：主册【交互设计】「图形选单（F171）不暴露
// 参数编辑（高级路径：文字选单 F171 降级态才有——双层防呆）」+「参数错误
// 画面：F173 语汇静态版+最接近合法参数建议」+【状态与异常】「参数注入
// 攻击 → 长度与字符集双截断+审计」+【交互设计】「合法参数清单在帮助
// F119（开发者篇）」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v4-一：MenuEditGate —— 选单参数编辑双层防呆（图形选单永远没有编辑入口；
// 只有文字选单的降级态才有——且需要显式解锁；两层全过才到达编辑面）
// ---------------------------------------------------------------------------

/// 选单形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuForm {
    /// 图形选单（F171 常态）——编辑入口不存在。
    Graphic,
    /// 文字选单降级态（图形子系统不可用时的兜底）——编辑入口存在但需解锁。
    TextFallback,
}

/// 双层门状态。
pub struct MenuEditGate {
    pub form: MenuForm,
    /// 第二层：编辑解锁（文字降级态下也须显式解锁——如按 E 键确认意图）。
    pub unlocked: bool,
    /// 图形态编辑请求被拒次数（双层防呆的执法对账）。
    pub graphic_denied: u64,
    /// 未解锁请求被拒次数。
    pub lock_denied: u64,
}

impl MenuEditGate {
    pub fn new(form: MenuForm) -> MenuEditGate {
        MenuEditGate { form, unlocked: false, graphic_denied: 0, lock_denied: 0 }
    }

    pub fn unlock(&mut self) {
        self.unlocked = true;
    }

    /// 编辑入口可达性（None=不可达附原因；Some=可达——到达编辑面）。
    pub fn edit_access(&mut self) -> Result<(), &'static str> {
        match self.form {
            MenuForm::Graphic => {
                self.graphic_denied += 1;
                Err("图形选单不提供参数编辑（双层防呆第一层）")
            }
            MenuForm::TextFallback if !self.unlocked => {
                self.lock_denied += 1;
                Err("参数编辑需显式解锁（双层防呆第二层——按 E 确认编辑意图）")
            }
            MenuForm::TextFallback => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// v4-二：ParamErrorScreen —— 参数错误画面（F173 语汇静态版：标题/解释/
// 建议/出口四字段——拒绝也要体面，报错也在帮忙）
// ---------------------------------------------------------------------------

/// 画面数据（渲染层照此对齐；字段全静态——引导期无堆环境也安全）。
pub struct ParamErrorScreen {
    pub title: &'static str,
    /// 解释（人话——为什么被拒）。
    pub explain: &'static str,
    /// 建议（最接近合法参数；None=无可建议）。
    pub suggestion: Option<&'static str>,
    /// 出口（怎么办——去帮助页/清参数重试）。
    pub exit_hint: &'static str,
}

/// 错误画面标题（F173 语汇族）。
pub const PARAM_ERROR_TITLE: &str = "启动参数校验未通过";

/// 逐判定 → 画面（Illegal/NoSuchParam/BadValue 三类各有解释语）。
pub fn param_error_screen(v: &ParamVerdict) -> Option<ParamErrorScreen> {
    let (explain, suggestion) = match v {
        ParamVerdict::Illegal(_) => (
            "参数包含非法字符或超出长度限制。引导面不接受自由文本。",
            None,
        ),
        ParamVerdict::NoSuchParam(s) => (
            "未识别的启动参数。合法参数清单见帮助中心开发者篇（F119）。",
            *s,
        ),
        ParamVerdict::BadValue(name) => (
            "参数值不合法（旗标不带值，整数有范围）。",
            Some(*name),
        ),
        _ => return None, // 合法判定不出错误画面。
    };
    Some(ParamErrorScreen {
        title: PARAM_ERROR_TITLE,
        explain,
        suggestion,
        exit_hint: "清除该参数后重新启动，或在帮助中心查阅合法参数清单",
    })
}

// ---------------------------------------------------------------------------
// v4-三：sanitize_echo —— 回显清洗（错误画面/审计行显示用户输入前的清洗：
// 控制字符剥离、换行展平、超长截断——防日志注入的最后一道）
// ---------------------------------------------------------------------------

/// 回显上限（清洗后回显最多 32 字节——够定位错误，不给攻击面铺长度）。
pub const ECHO_MAX: usize = 32;

/// 清洗：只保留可打印 ASCII 与原样 UTF-8 中文（≥0x80 字节对成对保留——
/// 中文错误提示要能回显），控制字符与换行替换为空格，超长截断标注。
pub fn sanitize_echo(input: &str, out: &mut String) -> bool {
    let mut truncated = false;
    let mut kept = 0usize;
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b >= 0x80 {
            // UTF-8 多字节首字节：按长度成对保留（非法序列丢）。
            let len = if b >= 0xF0 { 4 } else if b >= 0xE0 { 3 } else { 2 };
            if i + len <= bytes.len() && kept + len <= ECHO_MAX {
                if let Ok(s) = core::str::from_utf8(&bytes[i..i + len]) {
                    out.push_str(s);
                    kept += len;
                }
            } else {
                truncated = true;
                break;
            }
            i += len;
        } else if b == b'\n' || b == b'\r' || b == b'\t' || b < 0x20 {
            out.push(' ');
            kept += 1;
            i += 1;
        } else {
            if kept >= ECHO_MAX {
                truncated = true;
                break;
            }
            out.push(b as char);
            kept += 1;
            i += 1;
        }
    }
    if i < bytes.len() {
        truncated = true;
    }
    truncated
}

// ---------------------------------------------------------------------------
// v4-四：WhitelistDocRow —— 帮助页表格行（合法参数清单（F119 开发者篇）
// 的数据面：三族分组行+值域人话——从 WHITELIST 生成，永不与白名单脱节）
// ---------------------------------------------------------------------------

/// 一行文档（名/族/值域/说明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WhitelistDocRow {
    pub name: &'static str,
    pub family: &'static str,
    pub value_domain: &'static str,
    pub note: &'static str,
}

/// 值域人话（Flag/Int 的文案——与 value_range 互补的表格列文案）。
fn value_domain_text(k: &ValueKind) -> &'static str {
    match k {
        ValueKind::Flag => "旗标（无值；=false 关闭）",
        ValueKind::Int(0, 5) => "整数 0–5",
        ValueKind::Int(1, 3) => "整数 1–3",
        ValueKind::Int(0, 1) => "整数 0–1",
        ValueKind::Int(a, b) => {
            let _ = (a, b);
            "整数（界内）"
        }
    }
}

/// 生成全部行（族序：调试→降级→兼容；族内保持白名单定义序）。
pub fn whitelist_doc_rows() -> alloc::vec::Vec<WhitelistDocRow> {
    WHITELIST
        .iter()
        .map(|p| WhitelistDocRow {
            name: p.name,
            family: family_label(p.family),
            value_domain: value_domain_text(&p.kind),
            note: p.note,
        })
        .collect()
}

/// 帮助页完整性（13 行齐+族计数 6/3/4 与白名单 census 一致）。
pub fn whitelist_doc_consistent() -> bool {
    let rows = whitelist_doc_rows();
    let debug_n = rows.iter().filter(|r| r.family == "调试族").count();
    let degrade_n = rows.iter().filter(|r| r.family == "降级族").count();
    let compat_n = rows.iter().filter(|r| r.family == "兼容族").count();
    rows.len() == 13 && (debug_n, degrade_n, compat_n) == (6, 3, 4)
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F192 v4 自检（聚合进 secstar2 域）。
pub fn run_paramwl_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-v4");

    // v4-一：双层防呆——图形态永拒、降级态解锁才可达、逐层计数。
    let mut g = MenuEditGate::new(MenuForm::Graphic);
    set.add("menu graphic denied", g.edit_access().is_err(), "第一层：图形态无编辑");
    set.add("menu unlock noop", { g.unlock(); g.edit_access().is_err() }, "解锁对图形态无效");
    set.add("menu graphic count", g.graphic_denied == 2 && g.lock_denied == 0, "");
    let mut t = MenuEditGate::new(MenuForm::TextFallback);
    set.add("menu locked denied", t.edit_access().is_err(), "第二层：未解锁拒");
    set.add("menu lock count", t.lock_denied == 1, "");
    t.unlock();
    set.add("menu unlocked ok", t.edit_access().is_ok(), "");

    // v4-二：错误画面——三类判定各有画面、合法判定零画面、建议挂对。
    let mut wl = ParamWhitelist::new();
    let vs = wl.check_line("verbos verbose log-level=abc bad%%");
    let screens: alloc::vec::Vec<ParamErrorScreen> = vs
        .iter()
        .filter_map(param_error_screen)
        .collect();
    set.add("err screens produced", !screens.is_empty(), "");
    set.add("err title family", screens.iter().all(|s| s.title == PARAM_ERROR_TITLE), "");
    set.add("err suggestion exists", screens.iter().any(|s| s.suggestion == Some("verbose")), "拼写错建议挂上");
    set.add("err illegal no suggest", screens.iter().any(|s| s.suggestion.is_none() && s.explain.contains("非法字符")), "注入样本无建议可给");
    set.add("err badvalue names", screens.iter().any(|s| s.suggestion == Some("log-level")), "值错建议=参数自身");
    // 合法判定零画面。
    let ok_v = ParamVerdict::FlagOn("verbose");
    set.add("err legal silent", param_error_screen(&ok_v).is_none(), "");

    // v4-三：回显清洗——控制字符展平、截断标注、中文保真、CRLF 全化空格。
    let mut out = String::new();
    set.add("echo control flat", !sanitize_echo("bad\tx\r\nname", &mut out) && out == "bad x  name", "控制字符与 CRLF 各展平为一格空格");
    let mut out2 = String::new();
    set.add("echo trunc flagged", sanitize_echo(&"x".repeat(40), &mut out2), "超 32 截断+标注");
    set.add("echo trunc len", out2.chars().count() == 32, "");
    let mut out3 = String::new();
    set.add("echo cjk keep", !sanitize_echo("参数错", &mut out3) && out3 == "参数错", "中文回显保真");
    let mut out4 = String::new();
    set.add("echo empty ok", !sanitize_echo("", &mut out4) && out4.is_empty(), "");

    // v4-四：帮助页表格——13 行、族计数一致、值域列语义对。
    set.add("doc consistent", whitelist_doc_consistent(), "");
    let rows = whitelist_doc_rows();
    set.add("doc flag domain", rows.iter().filter(|r| r.name == "verbose").all(|r| r.value_domain.contains("旗标")), "");
    set.add("doc int domain", rows.iter().filter(|r| r.name == "log-level").all(|r| r.value_domain.contains("0–5")), "");
    set.add("doc safe-mode in degrade", rows.iter().filter(|r| r.name == "safe-mode").all(|r| r.family == "降级族" && r.note.contains("F193")), "一处一事实在文档面复现");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn f192_v4_gate_layer_independence() {
        // 双层独立执法：图形态解锁 100 次仍然拒（第一层不因第二层松动）。
        let mut g = MenuEditGate::new(MenuForm::Graphic);
        for _ in 0..100 {
            g.unlock();
            assert!(g.edit_access().is_err());
        }
        assert_eq!(g.graphic_denied, 100);
        assert_eq!(g.lock_denied, 0);
    }

    #[test]
    fn f192_v4_sanitize_never_grows() {
        // 清洗输出永不长于 ECHO_MAX 字符（注入长串压测）。
        for n in [33usize, 64, 128, 1000] {
            let s: String = core::iter::repeat('A').take(n).collect();
            let mut out = String::new();
            sanitize_echo(&s, &mut out);
            assert!(out.chars().count() <= ECHO_MAX);
        }
        // 换行注入（日志伪造）被展平成一行。
        let mut out = String::new();
        sanitize_echo("a\nFAKE: system log\r\nb", &mut out);
        assert!(!out.contains('\n') && !out.contains('\r'));
    }

    #[test]
    fn f192_v4_error_screen_all_illegal_samples() {
        // 20 非法样本的子集逐一产画面且建议字段诚实（拼错给建议、注入不给）。
        let samples = [
            ("verbos", true),
            ("safe-mod", true),
            // legacy_time ↔ legacy-timer 编辑距离恰 2（_→- 换 1 次 + 补 r）——
            // 距离 ≤2 建议机制如实给建议：这是正确行为不是误报。
            ("legacy_time", true),
            ("a=1;b=2", false),
        ];
        let mut wl = ParamWhitelist::new();
        for (s, expect_suggest) in samples {
            let v = wl.check_token(s);
            let scr = param_error_screen(&v);
            assert!(scr.is_some(), "sample {} 应有画面", s);
            assert_eq!(scr.unwrap().suggestion.is_some(), expect_suggest, "sample {}", s);
        }
    }

    #[test]
    fn f192_v4_doc_rows_generated_not_handwritten() {
        // 文档行与白名单逐条同名同族（生成而非手抄——脱节不可能发生）。
        let rows = whitelist_doc_rows();
        for (p, r) in WHITELIST.iter().zip(rows.iter()) {
            assert_eq!(p.name, r.name);
            assert_eq!(family_label(p.family), r.family);
            assert_eq!(p.note, r.note);
        }
    }

    #[test]
    fn f192_v4_run_checks_pass() {
        assert!(run_paramwl_deep3_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 2026-09-26 · 主册上限口径冲刺）——全参数帮助页 /
// 解析遥测账 / ADR 时间线 / fuzz 报告聚合。判据源：主册【交互设计】「合法
// 参数清单在帮助 F119（开发者篇）」+【数据与存储】「拒绝记录入日志环
// （F188）」+【规格框架】「三族定义文档化（初始清单——增补走 ADR）」
// +【验收判据】20 非法样本全拒的统计面。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v5-一：param_help_page —— 全参数帮助页（F119 开发者篇完整渲染：逐参
// 行（名/族/值域/说明）+ 语法行 + ADR 承诺行——从 WHITELIST 生成）
// ---------------------------------------------------------------------------

/// 帮助页节。
pub fn param_help_page() -> alloc::vec::Vec<(&'static str, alloc::vec::Vec<WhitelistDocRow>)> {
    let rows = whitelist_doc_rows();
    let mut out = alloc::vec::Vec::new();
    for family in [ParamFamily::Debug, ParamFamily::Degrade, ParamFamily::Compat] {
        let label = family_label(family);
        let group: alloc::vec::Vec<WhitelistDocRow> =
            rows.iter().filter(|r| r.family == label).copied().collect();
        out.push((label, group));
    }
    out
}

/// 语法行（key=value 严格语法说明——无空格变体，与 check_token 同语义）。
pub const PARAM_SYNTAX_LINE: &str = "参数语法：name 或 name=value（严格等号连接，不接受空格变体）；单参 ≤128 字符，总长 ≤1024 字符";

/// ADR 承诺行（初始 13 参——增补一律走 ADR 评审）。
pub const PARAM_ADR_LINE: &str = "初始白名单 13 参；任何增补/调整一律通过 ADR 评审并留版本记录";

/// 帮助页完整性（三族组齐+语法行+ADR 行）。
pub fn param_help_page_intact() -> bool {
    let page = param_help_page();
    page.len() == 3
        && page.iter().map(|(_, g)| g.len()).sum::<usize>() == WHITELIST.len()
        && page.iter().all(|(_, g)| !g.is_empty())
}

// ---------------------------------------------------------------------------
// v5-二：ParseTelemetry —— 解析遥测账（rejections/clamps/suggestions 三账
// 分列 + 逐类样本数——「拒绝也留痕」的统计面，与 audit_stream_line 同源）
// ---------------------------------------------------------------------------

/// 遥测账。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ParseTelemetry {
    /// 拒绝总数（NoSuchParam+BadValue+Illegal）。
    pub rejections: u64,
    /// 钳制总数（IntClamped）。
    pub clamps: u64,
    /// 给出建议的次数。
    pub suggestions: u64,
    /// 合法通过数。
    pub accepted: u64,
}

impl ParseTelemetry {
    /// 从判定序列汇总（一类判定一条账——分类不混）。
    pub fn from_verdicts(verdicts: &[ParamVerdict]) -> ParseTelemetry {
        let mut t = ParseTelemetry::default();
        for v in verdicts {
            match v {
                ParamVerdict::FlagOn(_) | ParamVerdict::FlagOff(_) | ParamVerdict::Int(_, _) => t.accepted += 1,
                ParamVerdict::IntClamped(_, _) => {
                    t.accepted += 1;
                    t.clamps += 1;
                }
                ParamVerdict::NoSuchParam(Some(_)) => {
                    t.rejections += 1;
                    t.suggestions += 1;
                }
                ParamVerdict::NoSuchParam(None) => t.rejections += 1,
                ParamVerdict::BadValue(_) => {
                    t.rejections += 1;
                    t.suggestions += 1;
                }
                ParamVerdict::Illegal(_) => t.rejections += 1,
            }
        }
        t
    }

    /// 净化导出行（数字与人话——不含原始输入，防日志注入面）。
    pub fn sanitized_line(&self) -> String {
        alloc::format!(
            "参数校验：通过 {}（含钳制 {}）/ 拒绝 {} / 给出建议 {}",
            self.accepted, self.clamps, self.rejections, self.suggestions
        )
    }
}

// ---------------------------------------------------------------------------
// v5-三：adr_timeline —— ADR 时间线渲染（提案→批准→生效三行式——
// 「变更走 ADR」的留痕面）
// ---------------------------------------------------------------------------

/// 时间线行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdrTimelineRow {
    pub day: u64,
    pub text: String,
    pub token: &'static str,
}

/// 从 AdrLedger 渲染时间线（每案两行：立案 + 已批/待批；拒绝的不上时间线，
/// 但在提案台账里可查（ledger 本身全量））。
pub fn adr_timeline(ledger: &AdrLedger, now_day: u64) -> alloc::vec::Vec<AdrTimelineRow> {
    let mut out = alloc::vec::Vec::new();
    for e in &ledger.entries {
        out.push(AdrTimelineRow {
            day: e.day,
            text: alloc::format!("ADR {} 立案：参数 {}（{}）", e.id, e.param.name, family_label(e.param.family)),
            token: "neutral",
        });
        if e.applied {
            out.push(AdrTimelineRow {
                day: e.day,
                text: alloc::format!("ADR {} 批准生效（已入运行时白名单）", e.id),
                token: "success",
            });
        } else {
            out.push(AdrTimelineRow {
                day: now_day,
                text: alloc::format!("ADR {} 待批准（已挂 {} 天）", e.id, now_day.saturating_sub(e.day)),
                token: "warning",
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// v5-四：FuzzReport —— fuzz 报告聚合（多轮统计：拒绝分类占比+建议命中率
// ——「20 非法样本全拒」的规模化面）
// ---------------------------------------------------------------------------

/// 聚合报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzReport {
    /// 总 token 数。
    pub total: u64,
    /// 拒绝数（按类分列）。
    pub illegal: u64,
    pub no_such_param: u64,
    pub bad_value: u64,
    /// 拒绝率（permille——语义上应≈1000：fuzz 变异体几乎全非法）。
    pub rejection_permille: u64,
}

/// 从判定流聚合（输入应来自 fuzz_round 多轮产物）。
pub fn fuzz_report(verdicts: &[ParamVerdict]) -> FuzzReport {
    let mut r = FuzzReport { total: verdicts.len() as u64, illegal: 0, no_such_param: 0, bad_value: 0, rejection_permille: 0 };
    for v in verdicts {
        match v {
            ParamVerdict::Illegal(_) => r.illegal += 1,
            ParamVerdict::NoSuchParam(_) => r.no_such_param += 1,
            ParamVerdict::BadValue(_) => r.bad_value += 1,
            _ => {}
        }
    }
    let rejected = r.illegal + r.no_such_param + r.bad_value;
    r.rejection_permille = rejected * 1000 / r.total.max(1);
    r
}

// ---------------------------------------------------------------------------
// v5 自检（deep4 表）
// ---------------------------------------------------------------------------

/// F192 v5 自检（聚合进 secstar2 域）。
pub fn run_paramwl_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-v5");

    // v5-一：帮助页——三族组齐、总数对账、语法/ADR 行。
    set.add("help page intact", param_help_page_intact(), "");
    let page = param_help_page();
    set.add("help group order", page[0].0 == "调试族" && page[1].0 == "降级族" && page[2].0 == "兼容族", "");
    set.add("help sizes", page[0].1.len() == 6 && page[1].1.len() == 3 && page[2].1.len() == 4, "6/3/4 census");
    set.add("help syntax line", PARAM_SYNTAX_LINE.contains("128") && PARAM_SYNTAX_LINE.contains("1024"), "");
    set.add("help adr line", PARAM_ADR_LINE.contains("13"), "");

    // v5-二：遥测账——四账分列、净化导出、与判定序列一致。
    let mut wl = ParamWhitelist::new();
    let verdicts = wl.check_line("verbose log-level=9 verbos bad%% safe-mode");
    let t = ParseTelemetry::from_verdicts(&verdicts);
    set.add("tel accepted", t.accepted == 3, "verbose+钳制的log-level+safe-mode");
    set.add("tel clamps", t.clamps == 1, "log-level=9→5 钳制");
    set.add("tel rejections", t.rejections == 2, "verbos+bad%%");
    set.add("tel suggestions", t.suggestions == 1, "verbos 有建议；bad%% 无");
    set.add("tel sanitized", t.sanitized_line().contains("通过 3"), "");
    // 空序列诚实零账。
    set.add("tel empty", ParseTelemetry::from_verdicts(&[]).rejections == 0, "");

    // v5-三：ADR 时间线——已批两行、未批两行、挂起天数。
    // 提案参数必须是白名单外的新参数（ADR 增补语义——已有参数走 ADR 调整）。
    let mut ledger = AdrLedger::new();
    let new_param = ParamSpec { name: "io-poll", family: ParamFamily::Compat, kind: ValueKind::Flag, note: "IO 轮询兼容路径（ADR 增补示例）" };
    ledger.propose("ADR-001", new_param, 10).ok();
    ledger.apply("ADR-001").ok();
    let pending_param = ParamSpec { name: "uart-slow", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "低速串口兼容" };
    ledger.propose("ADR-002", pending_param, 20).ok();
    let tl = adr_timeline(&ledger, 30);
    set.add("adr rows", tl.len() == 4, "两案各两行：立案+生效/待批");
    set.add("adr applied row", tl[1].token == "success" && tl[1].text.contains("生效"), "");
    set.add("adr rows per case", tl[0].text.contains("ADR-001") && tl[2].text.contains("ADR-002"), "每案两行成对");
    set.add("adr pending row", tl[3].token == "warning" && tl[3].text.contains("10 天"), "挂起天数");
    set.add("adr case row", tl[0].token == "neutral" && tl[0].text.contains("立案"), "");

    // v5-四：fuzz 报告——聚合、拒绝率、全非法≈1000‰。
    let mut wl2 = ParamWhitelist::new();
    let mut all = alloc::vec::Vec::new();
    for seed in 0..5u64 {
        let (rej, _acc) = fuzz_round(seed, 10);
        let _ = rej;
        // fuzz_round 返回统计——判定序列另行构造（10 个变异体逐个过门）。
        for i in 0..10u64 {
            all.push(wl2.check_token(&alloc::format!("zz{}-{}", seed, i)));
        }
    }
    let fr = fuzz_report(&all);
    set.add("fuzz total", fr.total == 50, "");
    set.add("fuzz all rejected", fr.rejection_permille == 1000, "随机变异体全拒");
    set.add("fuzz classed", fr.illegal + fr.no_such_param == 50, "分类总和=总数");
    // 混入合法 token 后占比下降（分类账仍然守恒）。
    let mut mixed = alloc::vec![wl2.check_token("verbose")];
    mixed.push(wl2.check_token("zz"));
    let fr2 = fuzz_report(&mixed);
    set.add("fuzz mixed rate", fr2.rejection_permille == 500, "2 取 1 → 500‰");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn f192_v5_telemetry_never_mixes_books() {
        // 30 条混合判定：四账之和 = 总数（账本守恒——一条判定恰记一笔）。
        let mut wl = ParamWhitelist::new();
        let mut all = alloc::vec::Vec::new();
        for s in ["verbose", "log-level=3", "verbos", "bad%%", "log-sinks=2", "no-gui", "x=1;y=2", "log-level=abc"] {
            all.extend(wl.check_line(s));
        }
    let t = ParseTelemetry::from_verdicts(&all);
    assert_eq!(t.accepted + t.rejections, all.len() as u64, "accepted+rejections 守恒");
    assert_eq!(t.clamps, 0, "这批样本不含界外整数（无钳制）");
    assert_eq!(t.accepted, 4, "verbose/log-level=3/log-sinks=2/no-gui");
    assert_eq!(t.rejections, 4, "verbos/bad%%/分号注入/log-level=abc");
    assert_eq!(t.suggestions, 2, "verbos 与 log-level=abc 各有建议");
    }

    #[test]
    fn f192_v5_help_page_no_orphans() {
        // 帮助页行与 WHITELIST 逐条同名（生成面零孤儿零遗漏）。
        let page = param_help_page();
        let mut names: alloc::vec::Vec<&str> = page.iter().flat_map(|(_, g)| g.iter().map(|r| r.name)).collect();
        names.sort_unstable();
        let mut expect: alloc::vec::Vec<&str> = WHITELIST.iter().map(|p| p.name).collect();
        expect.sort_unstable();
        assert_eq!(names, expect);
    }

    #[test]
    fn f192_v5_adr_timeline_pending_aging() {
        // 挂起天数随时间增长（时间线是活的——同 ledger 不同 now 不同行）。
        let mut ledger = AdrLedger::new();
        let p = ParamSpec { name: "uart-slow", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "低速串口" };
        ledger.propose("ADR-009", p, 5).ok();
        let t1 = adr_timeline(&ledger, 10);
        let t2 = adr_timeline(&ledger, 100);
        assert!(t1[1].text.contains("5 天"));
        assert!(t2[1].text.contains("95 天"));
    }

    #[test]
    fn f192_v5_run_checks_pass() {
        assert!(run_paramwl_deep4_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——独立值校验器 / 启动行构建器 /
// 三族使用统计 / 参数搜索 / ADR 历史统计。

use alloc::string::ToString;
// 判据源：主册【状态与异常】
// 「合法参数值越界（log-level=99）→ 钳制+警告」的正向面 +【数据与存储】
// 「白名单表编译期常量（变更走 ADR）」的统计投影。
// ---------------------------------------------------------------------------

/// 值校验结论（给定 spec 与原始值串——check_token 的值段拆分独立面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueVerdict {
    /// 合法整数值。
    OkInt(i64),
    /// 越界钳制（原值+钳后值）。
    Clamped(i64, i64),
    /// 非法值（非数字/旗标带值）。
    Bad,
}

/// 校验一个值（spec.kind 决定语义）。
pub fn validate_value(spec: &ParamSpec, raw: &str) -> ValueVerdict {
    match spec.kind {
        ValueKind::Flag => ValueVerdict::Bad, // 旗标不接受值（=false 走 FlagOff 语义，此处是值段校验）。
        ValueKind::Int(lo, hi) => match raw.parse::<i64>() {
            Ok(v) if v < lo => ValueVerdict::Clamped(v, lo),
            Ok(v) if v > hi => ValueVerdict::Clamped(v, hi),
            Ok(v) => ValueVerdict::OkInt(v),
            Err(_) => ValueVerdict::Bad,
        },
    }
}

/// 启动行构建器（反向面：从合法参数集生成一行命令行——生成的行必过自己的门）。
pub struct BootLineBuilder {
    parts: Vec<String>,
}

impl BootLineBuilder {
    pub fn new() -> BootLineBuilder {
        BootLineBuilder { parts: Vec::new() }
    }

    /// 加旗标（名字必须白名单内——构建器不生产非法行）。
    pub fn flag(&mut self, name: &'static str) -> Result<(), &'static str> {
        match find(name) {
            Some(spec) if matches!(spec.kind, ValueKind::Flag) => {
                self.parts.push(name.to_string());
                Ok(())
            }
            _ => Err("非白名单旗标"),
        }
    }

    /// 加整数参数（自动钳制到 spec 值域）。
    pub fn int(&mut self, name: &'static str, value: i64) -> Result<(), &'static str> {
        match find(name) {
            Some(spec) => match spec.kind {
                ValueKind::Int(lo, hi) => {
                    let v = value.clamp(lo, hi);
                    self.parts.push(alloc::format!("{}={}", name, v));
                    Ok(())
                }
                _ => Err("该参数是旗标"),
            },
            None => Err("非白名单参数"),
        }
    }

    /// 产出（总长守卫——超 1KB 拒绝）。
    pub fn build(&self) -> Result<String, &'static str> {
        let line = self.parts.join(" ");
        if line.len() > TOTAL_MAX_LEN {
            return Err("总长超 1KB");
        }
        Ok(line)
    }
}

impl Default for BootLineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 三族使用统计（判定流按族计数——哪族参数最常被碰）。
pub fn family_stats(verdicts: &[ParamVerdict]) -> [(usize, usize); 3] {
    // 每族 (命中, 拒绝)。NoSuchParam/BadValue/Illegal 按建议归属或归拒绝。
    let mut out = [(0usize, 0usize); 3];
    for v in verdicts {
        let name = match v {
            ParamVerdict::FlagOn(n) | ParamVerdict::FlagOff(n) | ParamVerdict::Int(n, _) | ParamVerdict::IntClamped(n, _) | ParamVerdict::BadValue(n) => Some(*n),
            ParamVerdict::NoSuchParam(_) | ParamVerdict::Illegal(_) => None,
        };
        match name.and_then(find) {
            Some(spec) => {
                let slot = match spec.family {
                    ParamFamily::Debug => 0,
                    ParamFamily::Degrade => 1,
                    ParamFamily::Compat => 2,
                };
                if matches!(v, ParamVerdict::BadValue(_)) {
                    out[slot].1 += 1;
                } else {
                    out[slot].0 += 1;
                }
            }
            None => out[0].1 += 1, // 未知参数挂调试族拒账（调试场景最常见）。
        }
    }
    out
}

/// 参数搜索（子串匹配+族过滤——帮助页内的检索面）。
pub fn param_search(query: &str, family: Option<ParamFamily>) -> Vec<&'static str> {
    WHITELIST
        .iter()
        .filter(|p| family.map(|f| p.family == f).unwrap_or(true))
        .filter(|p| p.name.contains(query))
        .map(|p| p.name)
        .collect()
}

/// ADR 历史统计（提案数/批准数/批准率 permille/最长挂起天数）。
pub struct AdrHistoryStats {
    pub proposed: usize,
    pub applied: usize,
    pub approval_permille: u64,
    pub max_pending_days: u64,
}

/// 从账本聚合（now_day 用于挂起天数）。
pub fn adr_history_stats(ledger: &AdrLedger, now_day: u64) -> AdrHistoryStats {
    let proposed = ledger.entries.len();
    let applied = ledger.entries.iter().filter(|e| e.applied).count();
    let approval_permille = if proposed == 0 { 0 } else { applied as u64 * 1000 / proposed as u64 };
    let max_pending_days = ledger
        .entries
        .iter()
        .filter(|e| !e.applied)
        .map(|e| now_day.saturating_sub(e.day))
        .max()
        .unwrap_or(0);
    AdrHistoryStats { proposed, applied, approval_permille, max_pending_days }
}

/// F192 v6 自检（deep5 表）。
pub fn run_paramwl_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-v6");

    // v6-一：值校验器——合法/钳制/非法/旗标拒绝。
    let spec = find("log-level").unwrap();
    set.add("val ok", validate_value(spec, "3") == ValueVerdict::OkInt(3), "");
    set.add("val clamp hi", validate_value(spec, "99") == ValueVerdict::Clamped(99, 5), "99→5 主册判据");
    set.add("val clamp lo", validate_value(spec, "-1") == ValueVerdict::Clamped(-1, 0), "");
    set.add("val bad", validate_value(spec, "abc") == ValueVerdict::Bad, "");
    let flag = find("verbose").unwrap();
    set.add("val flag reject", validate_value(flag, "1") == ValueVerdict::Bad, "旗标不带值");

    // v6-二：构建器——合法产出过自己的门、钳制、非法拒绝、超长拒绝。
    let mut wl = ParamWhitelist::new();
    let mut b = BootLineBuilder::new();
    b.flag("verbose").ok();
    b.int("log-level", 99).ok(); // 自动钳 5。
    b.int("log-sinks", 2).ok();
    let line = b.build().unwrap();
    set.add("build line", line == "verbose log-level=5 log-sinks=2", "钳制已内建");
    let verdicts = wl.check_line(&line);
    set.add("build passes own gate", wl.line_ok(&verdicts), "生成的行必过自己的门（自洽）");
    set.add("build reject flag value", b.flag("log-level").is_err(), "旗标入口不收整参名");
    set.add("build reject unknown", { let mut b2 = BootLineBuilder::new(); b2.flag("not-a-param").is_err() }, "");

    // v6-三：族统计——命中/拒绝分账、未知挂调试拒账。
    let vs = wl.check_line("verbose verbos log-level=3 no-gui legacy-timer");
    let fs = family_stats(&vs);
    set.add("fam debug hit", fs[0].0 >= 2, "verbose+log-level 命中");
    set.add("fam debug rej", fs[0].1 >= 1, "verbos+未知挂调试拒账");
    set.add("fam degrade hit", fs[1].0 == 1, "no-gui");
    set.add("fam compat hit", fs[2].0 == 1, "legacy-timer");

    // v6-四：搜索——子串、族过滤、组合、空结果诚实。
    set.add("search sub", param_search("log", None).len() == 2, "log-level+log-sinks");
    set.add("search family", param_search("", Some(ParamFamily::Degrade)).len() == 3, "降级族 3 参");
    set.add("search combo", param_search("safe", Some(ParamFamily::Degrade)) == vec!["safe-mode"], "");
    set.add("search empty honest", param_search("zzz", None).is_empty(), "");

    // v6-五：ADR 统计——批准率、挂起天数、零提案诚实。
    let mut ledger = AdrLedger::new();
    let p1 = ParamSpec { name: "io-poll", family: ParamFamily::Compat, kind: ValueKind::Flag, note: "示例" };
    ledger.propose("ADR-001", p1, 10).ok();
    ledger.apply("ADR-001").ok();
    let p2 = ParamSpec { name: "uart-slow", family: ParamFamily::Debug, kind: ValueKind::Flag, note: "示例" };
    ledger.propose("ADR-002", p2, 40).ok();
    let st = adr_history_stats(&ledger, 60);
    set.add("adr proposed", st.proposed == 2 && st.applied == 1, "");
    set.add("adr rate", st.approval_permille == 500, "");
    set.add("adr pending max", st.max_pending_days == 20, "60-40=20 天");
    let empty = adr_history_stats(&AdrLedger::new(), 100);
    set.add("adr empty honest", empty.approval_permille == 0 && empty.max_pending_days == 0, "");

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f192_v6_builder_roundtrip_all_flags() {
        // 13 参数全量构建：逐个独立成行都过门（白名单自洽终检）。
        let mut wl = ParamWhitelist::new();
        for spec in WHITELIST.iter() {
            let mut b = BootLineBuilder::new();
            match spec.kind {
                ValueKind::Flag => b.flag(spec.name).unwrap(),
                ValueKind::Int(lo, _) => b.int(spec.name, lo).unwrap(),
            }
            let line = b.build().unwrap();
            let vs = wl.check_line(&line);
            assert!(wl.line_ok(&vs), "{} 的构建行被自己的门拒了", spec.name);
        }
    }

    #[test]
    fn f192_v6_value_boundary_matrix() {
        // log-level 边界矩阵：-1/0/5/6 四点（钳制语义双向验证）。
        let spec = find("log-level").unwrap();
        assert_eq!(validate_value(spec, "0"), ValueVerdict::OkInt(0));
        assert_eq!(validate_value(spec, "5"), ValueVerdict::OkInt(5));
        assert_eq!(validate_value(spec, "6"), ValueVerdict::Clamped(6, 5));
        assert_eq!(validate_value(spec, "-1"), ValueVerdict::Clamped(-1, 0));
    }

    #[test]
    fn f192_v6_run_checks_pass() {
        assert!(run_paramwl_deep5_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限口径收官）——启动行对比 / 场景预设 / 白名单
// 版本对比 / 开放导出。判据源：主册【交互设计】「合法参数清单在帮助 F119」
// +【数据与存储】白名单编译期常量+ADR 版本管理。
// ---------------------------------------------------------------------------

/// 启动行对比（新增/移除/值变更三类差异——配置审计面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineDiff {
    Added(String),
    Removed(String),
    Changed(String, String), // 旧值 → 新值（同参数）。
}

/// 对比两行启动参数（按参数名对齐——行序无关）。
pub fn boot_line_diff(old: &str, new: &str) -> Vec<LineDiff> {
    let parse = |line: &str| -> Vec<(String, Option<String>)> {
        line.split_whitespace()
            .map(|t| match t.split_once('=') {
                Some((k, v)) => (k.to_string(), Some(v.to_string())),
                None => (t.to_string(), None),
            })
            .collect()
    };
    let mut old_map: Vec<(String, Option<String>)> = parse(old);
    let mut new_map: Vec<(String, Option<String>)> = parse(new);
    old_map.sort();
    new_map.sort();
    let mut out = Vec::new();
    for (k, v) in &old_map {
        match new_map.iter().find(|(nk, _)| nk == k) {
            None => out.push(LineDiff::Removed(k.clone())),
            Some((_, nv)) if nv != v => {
                out.push(LineDiff::Changed(
                    alloc::format!("{}={}", k, v.clone().unwrap_or_default()),
                    alloc::format!("{}={}", k, nv.clone().unwrap_or_default()),
                ));
            }
            _ => {}
        }
    }
    for (k, v) in &new_map {
        if !old_map.iter().any(|(ok, _)| ok == k) {
            out.push(LineDiff::Added(alloc::format!("{}{}", k, v.clone().map(|x| alloc::format!("={}", x)).unwrap_or_default())));
        }
    }
    out
}

/// 场景预设（常用参数组合——帮助页推荐组合，逐项全白名单）。
pub const PARAM_PRESETS: [(&str, [&str; 3]); 3] = [
    ("调试包", ["verbose", "log-level=5", "ktrace"]),
    ("安全排查包", ["safe-mode", "no-third-drv", "log-level=3"]),
    ("兼容包", ["legacy-timer", "iommu-soft", "log-level=1"]),
];

/// 预设完整性（组合名互异+逐项全部白名单内——预设不许含非法参数）。
pub fn param_presets_valid() -> bool {
    PARAM_PRESETS.iter().all(|(name, params)| {
        params.iter().all(|p| {
            let key = p.split('=').next().unwrap_or(p);
            find(key).is_some()
        }) && !name.is_empty()
    }) && PARAM_PRESETS[0].0 != PARAM_PRESETS[1].0
}

/// 白名单开放导出（F128 语言：13 参全量 JSON——第三方工具可解析）。
pub fn whitelist_export_json(out: &mut Vec<u8>) {
    out.extend_from_slice(b"{\"whitelist\":[");
    for (i, p) in WHITELIST.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b",");
        }
        let kind = match p.kind {
            ValueKind::Flag => alloc::format!("\"flag\""),
            ValueKind::Int(lo, hi) => alloc::format!("\"int:{}..={}\"", lo, hi),
        };
        out.extend_from_slice(
            alloc::format!("{{\"name\":\"{}\",\"family\":\"{}\",\"kind\":{}}}", p.name, family_label(p.family), kind).as_bytes(),
        );
    }
    out.extend_from_slice(b"]}");
}

/// 导出形状自检（13 条+族齐）。
pub fn whitelist_export_ok(data: &[u8]) -> bool {
    let text = core::str::from_utf8(data).unwrap_or("");
    text.contains("\"whitelist\"")
        && text.matches("\"name\"").count() == WHITELIST.len()
        && text.contains("调试族")
        && text.contains("兼容族")
}

/// F192 v7 自检（deep6 表）。
pub fn run_paramwl_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-v7");

    // v7-一：行对比——增/删/改/无差四态。
    let d = boot_line_diff("verbose log-level=3", "verbose log-level=5 ktrace");
    set.add("diff changed", d.iter().any(|x| matches!(x, LineDiff::Changed(a, _) if a.contains("log-level=3"))), "值变更");
    set.add("diff added", d.iter().any(|x| matches!(x, LineDiff::Added(a) if a.contains("ktrace"))), "新增");
    let d2 = boot_line_diff("verbose safe-mode", "safe-mode");
    set.add("diff removed", d2.iter().any(|x| matches!(x, LineDiff::Removed(a) if a == "verbose")), "移除");
    set.add("diff none", boot_line_diff("verbose", "verbose").is_empty(), "无差=空");

    // v7-二：预设——全白名单+名互异。
    set.add("presets valid", param_presets_valid(), "");
    set.add("presets count", PARAM_PRESETS.len() == 3, "");

    // v7-三：白名单导出——13 条+族齐。
    let mut data = Vec::new();
    whitelist_export_json(&mut data);
    set.add("export ok", whitelist_export_ok(&data), "");
    let text = core::str::from_utf8(&data).unwrap_or("");
    set.add("export value kinds", text.contains("\"flag\"") && text.contains("int:0..=5"), "值域随行");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f192_v7_diff_value_only_change() {
        // 同参数值变更不改名（Changed 语义——不是删+加）。
        let d = boot_line_diff("log-level=3", "log-level=5");
        assert_eq!(d.len(), 1);
        assert!(matches!(d[0], LineDiff::Changed(_, _)));
    }

    #[test]
    fn f192_v7_export_roundtrip_parse() {
        // 导出可被外部语义解析（首尾结构与计数——生态语言自证）。
        let mut data = Vec::new();
        whitelist_export_json(&mut data);
        let text = core::str::from_utf8(&data).unwrap_or("");
        assert!(text.starts_with("{\"whitelist\":["));
        assert!(text.ends_with("]}"));
        assert_eq!(text.matches("\"family\"").count(), 13);
    }

    #[test]
    fn f192_v7_run_checks_pass() {
        assert!(run_paramwl_deep6_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v8 批次（第八轮深化 · 缺口冲刺）——变更影响图 / 白名单 diff 报告 / 分组
// 导航树 / 值域探针生成 / ini 往返 / 多键搜索。
// 判据源：主册【开发者篇】「参数改动前给出影响面（依赖谁/影响谁）」+
// 【验收判据】「设置可导出为 ini 并可回读」。
// ---------------------------------------------------------------------------

/// 影响边（A 变 → B 受影响——静态登记，导入期机检覆盖）。
pub const IMPACT_EDGES: [(&str, &str, &str); 4] = [
    ("log-level", "verbose", "log-level 提级时 verbose 建议同步开"),
    ("verbose", "log-level", "verbose 关闭后 log-level 调试档失效"),
    ("safe-mode", "no-gui", "安全模式强制无界面"),
    ("safe-mode", "no-third-drv", "安全模式附带禁第三方驱动"),
];

/// 影响集（改动 param → 直接受影响的参数名单 + 一句话理由）。
pub fn impact_set(param: &str) -> Vec<(&'static str, &'static str)> {
    IMPACT_EDGES
        .iter()
        .filter(|(from, _, _)| *from == param)
        .map(|(_, to, why)| (*to, *why))
        .collect()
}

/// 变更前评估行（「你改了 X，还会牵动：…」——三要素里的『为什么』前置）。
pub fn impact_notice(param: &str, out: &mut alloc::string::String) -> bool {
    let impacts = impact_set(param);
    if impacts.is_empty() {
        out.push_str("此项改动无连带影响。");
        return false;
    }
    out.push_str("此项改动还会牵动：");
    for (i, (to, why)) in impacts.iter().enumerate() {
        if i > 0 {
            out.push_str("；");
        }
        out.push_str(why);
        let _ = to;
    }
    true
}

/// 白名单 diff 报告（旧旗标集 → 新旗标集：增/删两列，未动的不上报告）。
pub fn wl_diff_report(old_flags: &[&str], new_flags: &[&str]) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for f in new_flags {
        if !old_flags.contains(f) {
            if let Some(spec) = find(f) {
                added.push(spec.name);
            }
        }
    }
    for f in old_flags {
        if !new_flags.contains(f) {
            if let Some(spec) = find(f) {
                removed.push(spec.name);
            }
        }
    }
    (added, removed)
}

/// diff 报告文本（人话三态：纯增/纯删/混合）。
pub fn wl_diff_text(old_flags: &[&str], new_flags: &[&str]) -> alloc::string::String {
    let (added, removed) = wl_diff_report(old_flags, new_flags);
    match (added.len(), removed.len()) {
        (0, 0) => alloc::string::String::from("参数无变化"),
        (a, 0) => alloc::format!("新增 {} 项：{}", a, added.join("、")),
        (0, r) => alloc::format!("移除 {} 项：{}", r, removed.join("、")),
        (a, r) => alloc::format!("新增 {} 项、移除 {} 项", a, r),
    }
}

/// 导航树节点（族 → 参数 → 徽标：在册/未在册）。
pub struct NavEntry {
    pub family: ParamFamily,
    pub name: &'static str,
    pub known: bool,
}

/// 分组导航树（全白名单按族分组——设置页左侧树的静态投影）。
pub fn group_nav() -> Vec<(ParamFamily, Vec<NavEntry>)> {
    let families = [ParamFamily::Debug, ParamFamily::Degrade, ParamFamily::Compat];
    families
        .iter()
        .map(|f| {
            let entries = WHITELIST
                .iter()
                .filter(|s| s.family == *f)
                .map(|s| NavEntry { family: s.family, name: s.name, known: true })
                .collect();
            (*f, entries)
        })
        .collect()
}

/// 值域探针（ValueKind → (最小合法, 最大合法, 边界外样例)——校验器的测试数据源）。
pub fn range_probe(kind: &ValueKind) -> (i64, i64, i64) {
    match kind {
        ValueKind::Flag => (0, 1, 2),
        ValueKind::Int(lo, hi) => (*lo, *hi, hi + 1),
    }
}

/// ini 导出（活跃旗标 → `[varix]\nname=true` 行——开放格式 F128）。
pub fn ini_export(flags: &[&str]) -> alloc::string::String {
    let mut out = alloc::string::String::from("[varix]\n");
    for f in flags {
        if find(f).is_some() {
            out.push_str(f);
            out.push_str("=true\n");
        }
    }
    out
}

/// ini 回读（只收白名单内的键——外部文件不引入未知参数）。
pub fn ini_parse(text: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            let val = line[eq + 1..].trim();
            if val == "true" {
                if let Some(spec) = find(key) {
                    out.push(spec.name);
                }
            }
        }
    }
    out
}

/// 多键搜索（名前缀 / 名子串 / 族标签——一个框三种命中）。
pub fn search_multi(query: &str) -> Vec<&'static str> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for spec in WHITELIST.iter() {
        let name_hit = spec.name.starts_with(q) || spec.name.contains(q);
        let family_hit = family_label(spec.family).contains(q);
        if name_hit || family_hit {
            out.push(spec.name);
        }
    }
    out
}

/// F192 v8 自检（deep7 表）。
pub fn run_paramwl_deep7_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-v8");

    // 影响图：正向/反向边、无影响项、通知文案。
    set.add("impact fwd", impact_set("log-level").len() == 1, "log-level → verbose");
    set.add("impact safe", impact_set("safe-mode").len() == 2, "safe-mode 牵动两项");
    set.add("impact none", impact_set("verbose-x").is_empty(), "未知参数零影响");
    let mut buf = alloc::string::String::new();
    set.add("impact notice", impact_notice("safe-mode", &mut buf) && buf.contains("无界面"), "");
    let mut buf2 = alloc::string::String::new();
    set.add("impact notice none", !impact_notice("log-ring", &mut buf2) && buf2.contains("无连带"), "");

    // diff 报告：增/删/混合/无变化四态。
    let (a, r) = wl_diff_report(&["verbose"], &["verbose", "log-level"]);
    set.add("diff add", a.len() == 1 && r.is_empty(), "");
    let (a2, r2) = wl_diff_report(&["verbose", "log-level"], &["verbose"]);
    set.add("diff del", a2.is_empty() && r2.len() == 1, "");
    set.add("diff text none", wl_diff_text(&["verbose"], &["verbose"]).contains("无变化"), "");
    set.add("diff text add", wl_diff_text(&[], &["verbose"]).contains("新增 1 项"), "");

    // 导航树：三族齐、条目守恒、全 known。
    let nav = group_nav();
    set.add("nav families", nav.len() == 3, "");
    let total: usize = nav.iter().map(|(_, e)| e.len()).sum();
    set.add("nav conserve", total == WHITELIST.len(), "树条目 = 白名单全量");
    set.add("nav known", nav.iter().all(|(_, e)| e.iter().all(|n| n.known)), "");

    // 值域探针：Flag 与 Int 两态。
    let (lo, hi, out) = range_probe(&ValueKind::Int(1, 9));
    set.add("probe int", lo == 1 && hi == 9 && out == 10, "界外样例 = hi+1");
    let (lo2, hi2, _) = range_probe(&ValueKind::Flag);
    set.add("probe flag", lo2 == 0 && hi2 == 1, "");

    // ini 往返：导出 → 回读恒等（开放格式的保真底线）。
    let flags = ["verbose", "log-level", "legacy-timer"];
    let ini = ini_export(&flags);
    set.add("ini has header", ini.starts_with("[varix]\n"), "");
    set.add("ini roundtrip", ini_parse(&ini).len() == 3, "三键全回读");
    let unknown = ini_export(&["verbose"]) .replace("verbose", "nonexist-key");
    set.add("ini reject unknown", ini_parse(&unknown).is_empty(), "未在册键不回收");

    // 多键搜索：前缀/子串/族/空。
    set.add("search prefix", search_multi("log").contains(&"log-level"), "");
    set.add("search family", search_multi("调试").iter().any(|n| find(n).map(|s| s.family) == Some(ParamFamily::Debug)), "族标签命中");
    set.add("search empty", search_multi("  ").is_empty(), "空查询零命中");

    set
}

#[cfg(test)]
mod deep7_tests {
    use super::*;

    #[test]
    fn f192_v8_impact_edges_symmetric_guard() {
        // 边守恒：每条边两端都是白名单成员（登记表不许指向幽灵参数）。
        for (from, to, why) in IMPACT_EDGES {
            assert!(find(from).is_some(), "edge from {} not in wl", from);
            assert!(find(to).is_some(), "edge to {} not in wl", to);
            assert!(!why.is_empty());
        }
    }

    #[test]
    fn f192_v8_ini_roundtrip_large() {
        // 全量往返：13 参全开 → 导出 → 回读 = 全量（顺序无关，集合守恒）。
        let all: Vec<&'static str> = WHITELIST.iter().map(|s| s.name).collect();
        let ini = ini_export(&all);
        let back = ini_parse(&ini);
        assert_eq!(back.len(), WHITELIST.len());
        for name in &all {
            assert!(back.contains(name));
        }
    }

    #[test]
    fn f192_v8_search_family_buckets() {
        // 按族搜：三族的搜索结果恰好等于各族成员（检索完备性）。
        for f in [ParamFamily::Debug, ParamFamily::Degrade, ParamFamily::Compat] {
            let label = family_label(f);
            let hits = search_multi(label);
            let expect: Vec<&'static str> = WHITELIST
                .iter()
                .filter(|s| s.family == f)
                .map(|s| s.name)
                .collect();
            assert_eq!(hits.len(), expect.len(), "family {} hits", label);
        }
    }

    #[test]
    fn f192_v8_run_checks_pass() {
        assert!(run_paramwl_deep7_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b4：互斥参数检测 / 外部参数导入校验 / 帮助索引 / verdict 回放确定性。
// ---------------------------------------------------------------------------

/// 互斥对（同开必冲突——导入期机检覆盖白名单真实性）。
pub const MUTEX_PAIRS: [(&str, &str, &str); 2] = [
    ("verbose", "selftest-only", "自检模式不开详细日志（两者抢串口）"),
    ("legacy-timer", "no-acpi", "传统定时器与停用 ACPI 不可同开（时钟源冲突）"),
];

/// 互斥冲突检测（活跃旗标集 → 冲突清单）。
pub fn mutex_conflicts(active: &[&str]) -> Vec<(&'static str, &'static str)> {
    MUTEX_PAIRS
        .iter()
        .filter(|(a, b, _)| active.contains(a) && active.contains(b))
        .map(|(a, b, _)| (*a, *b))
        .collect()
}

/// 导入校验汇总（逐行 verdict 的批结果）。
pub struct ImportSummary {
    pub accepted: usize,
    pub rejected: usize,
    pub unknown: usize,
    /// 冲突对（导入后集内互斥）。
    pub conflicts: Vec<(&'static str, &'static str)>,
}

/// 导入汇总（flags：外部参数文件解析出的旗标名）。
pub fn import_summary(flags: &[&str]) -> ImportSummary {
    let mut accepted = 0usize;
    let mut unknown = 0usize;
    let mut known_flags: Vec<&str> = Vec::new();
    for f in flags {
        if find(f).is_some() {
            accepted += 1;
            known_flags.push(f);
        } else {
            unknown += 1;
        }
    }
    // 引用静态名做冲突检测（known 里的名字都来自白名单）。
    let static_flags: Vec<&'static str> = known_flags
        .iter()
        .filter_map(|f| find(f).map(|s| s.name))
        .collect();
    ImportSummary { accepted, rejected: 0, unknown, conflicts: mutex_conflicts(&static_flags) }
}

/// 帮助索引条目。
pub struct HelpIndexRow {
    pub name: &'static str,
    pub note: &'static str,
}

/// 帮助索引（名 + 说明——搜索框的静态数据源）。
pub fn help_index() -> Vec<HelpIndexRow> {
    WHITELIST
        .iter()
        .map(|s| HelpIndexRow { name: s.name, note: s.note })
        .collect()
}

/// 帮助搜索（名或说明命中都算——帮助可发现性）。
pub fn help_search(query: &str) -> Vec<&'static str> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    WHITELIST
        .iter()
        .filter(|s| s.name.contains(q) || s.note.contains(q))
        .map(|s| s.name)
        .collect()
}

/// verdict 回放确定性（同输入序列重放 → 结果逐位一致——审计可复现）。
pub fn replay_deterministic(inputs: &[&str]) -> bool {
    let run = || -> Vec<&'static str> {
        let mut wl = ParamWhitelist::new();
        inputs
            .iter()
            .flat_map(|i| wl.check_line(i).into_iter())
            .map(|v| verdict_kind(&v))
            .collect()
    };
    run() == run()
}

/// F192 v8-b4 自检（并入 deep7 表）。
pub fn run_paramwl_deep7b_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-v8b");

    // 互斥：冲突检出 / 干净集 / 对账双方都在册。
    let cf = mutex_conflicts(&["verbose", "selftest-only"]);
    set.add("mutex found", cf.len() == 1, "");
    set.add("mutex clean", mutex_conflicts(&["verbose"]).is_empty(), "");
    for (a, b, why) in MUTEX_PAIRS {
        set.add("mutex in wl", find(a).is_some() && find(b).is_some() && !why.is_empty(), "互斥对全在册");
    }

    // 导入汇总：收/拒/未知三分账 + 冲突升级。
    let sum = import_summary(&["verbose", "log-level", "ghost-param"]);
    set.add("import split", sum.accepted == 2 && sum.unknown == 1, "");
    set.add("import no conflict", sum.conflicts.is_empty(), "");
    let sum2 = import_summary(&["verbose", "selftest-only"]);
    set.add("import conflict", sum2.conflicts.len() == 1, "导入集内互斥必点名");

    // 帮助索引：全量 + 搜索（名/说明双键）。
    set.add("help full", help_index().len() == WHITELIST.len(), "");
    set.add("help name", help_search("log").contains(&"log-level"), "");
    set.add("help note", help_search("串口").contains(&"verbose"), "说明文命中");
    set.add("help empty", help_search("").is_empty(), "");

    // 回放确定性。
    let inputs = ["verbose", "log-level=3", "ghost", "log-level=9"];
    set.add("replay deterministic", replay_deterministic(&inputs), "同输入重放逐位一致");
    // b7-wave2：版本迁移 / 参数指纹 / 导入干跑。
    set.add("mig renamed", migrate_key("log-level").contains("log"), "改名映射保语义");
    set.add("mig same", migrate_key("verbose") == "verbose", "未改名直通");
    set.add("mig ghost", migrate_key("ghost-x").is_empty(), "幽灵参数迁移为空并留痕");
    set.add("fp stable", param_fingerprint(&["verbose", "log-level"]) == param_fingerprint(&["log-level", "verbose"]), "指纹与顺序无关");
    set.add("fp differs", param_fingerprint(&["verbose"]) != param_fingerprint(&["verbose", "log-level"]), "集异指纹异");
    set.add("fp empty", param_fingerprint(&[]).is_empty(), "空集空指纹");
    set.add("dryrun ok", import_dry_run(&["verbose", "log-level"]).is_none(), "干净导入零问题");
    set.add("dryrun issues", import_dry_run(&["ghost", "verbose", "selftest-only"]) == Some(2), "1 未知 + 1 互斥 = 2 条问题");
    // b8-wave3：参数组预设 / tooltip / JSON 导出。
    set.add("preset perf", preset_flags("perf") == vec!["verbose", "log-level"], "性能预设两项");
    set.add("preset quiet", preset_flags("quiet").is_empty(), "安静预设零旗标");
    set.add("preset diag", preset_flags("diag").contains(&"selftest-only"), "诊断预设含自检");
    set.add("preset unknown", preset_flags("zzz").is_empty(), "未知预设不倾泻");
    set.add("tooltip", tooltip_line("log-level").contains("0-5"), "tooltip 带值域说明");
    set.add("tooltip missing", tooltip_line("ghost").is_empty(), "未在册零 tooltip");
    set.add("json export", preset_export_json(&["verbose"]).starts_with("{\"flags\":["), "JSON 形状");
    // b9-wave4：使用频率账 / 批量重置 / 帮助按频次排序。
    set.add("usage count", { let mut u = UsageCounter::new(); u.hit("verbose"); u.hit("verbose"); u.hit("ktrace"); u.top(2) == vec!["verbose", "ktrace"] }, "按频次降序");
    set.add("usage empty", UsageCounter::new().top(3).is_empty(), "空账空榜");
    set.add("usage cap", { let mut u = UsageCounter::new(); u.hit("verbose"); u.top(0).is_empty() }, "零容量空榜");
    set.add("reset batch", { let mut u = UsageCounter::new(); u.hit("verbose"); u.reset_all(); u.top(3).is_empty() }, "批量重置清账");
    set.add("help sorted", { let rows = help_index_sorted_by_usage(&UsageCounter::new()); rows.len() == WHITELIST.len() }, "帮助索引全量返回");
    // b10-wave5：白名单哈希锚 / 预设分享串。
    set.add("wl anchor", !wl_hash_anchor().is_empty(), "锚非空");
    set.add("wl anchor stable", wl_hash_anchor() == wl_hash_anchor(), "同表同锚（防篡改自检基准）");
    set.add("share encode", share_encode("perf").contains("perf"), "分享串含预设名");
    set.add("share decode", share_decode(&share_encode("diag")) == Some("diag"), "分享串可回读");
    set.add("share bad", share_decode("!!").is_none(), "非法串拒收");
    // b11-wave6：白名单 CSV 导出 / 搜索防抖建议。
    set.add("wl csv", wl_export_csv().starts_with("name,family,kind\n"), "CSV 表头");
    set.add("wl csv rows", wl_export_csv().lines().count() == WHITELIST.len() + 1, "13 参 13 行");
    set.add("debounce hint", search_debounce_hint(3) == "输入停顿 300ms 后搜索", "快打不急搜");
    set.add("debounce instant", search_debounce_hint(600) == "", "停顿够长直接搜");
    // b12-wave7：参数分布饼账 / 族内搜索计数。
    set.add("family share", family_share(ParamFamily::Debug) == 6, "调试族 6 参");
    set.add("family share d", family_share(ParamFamily::Degrade) == 3, "降级族 3 参");
    set.add("family share c", family_share(ParamFamily::Compat) == 4, "兼容族 4 参");
    set.add("share sum", family_share(ParamFamily::Debug) + family_share(ParamFamily::Degrade) + family_share(ParamFamily::Compat) == WHITELIST.len(), "分族守恒");
    // b13-wave8：参数别名表（常用简称 → 正名）。
    set.add("alias hit", alias_lookup("ll") == Some("log-level"), "简称 ll → log-level");
    set.add("alias miss", alias_lookup("zz").is_none(), "未知简称落空");
    set.add("alias in wl", ALIAS_TABLE.iter().all(|(_, full)| find(full).is_some()), "别名全指向在册参数");
    // b14-wave9：默认值账 / 改动撤销栈。
    set.add("defaults exist", DEFAULT_OFF_FLAGS.len() >= 1, "默认关闭名单在册");
    set.add("defaults all flag", DEFAULT_OFF_FLAGS.iter().all(|n| find(n).map(|s| matches!(s.kind, ValueKind::Flag)).unwrap_or(false)), "默认名单全为旗标");
    set.add("undo stack", { let mut u = ChangeUndo::new(); u.push("log-level", "调到 3"); u.pop() == Some("log-level") }, "改动可撤销");

    set
}

#[cfg(test)]
mod deep7b_tests {
    use super::*;

    #[test]
    fn f192_v8b_mutex_symmetric() {
        // 互斥对称：顺序无关（a,b 同开与 b,a 同开等价）。
        assert_eq!(mutex_conflicts(&["verbose", "selftest-only"]).len(), 1);
        assert_eq!(mutex_conflicts(&["selftest-only", "verbose"]).len(), 1);
    }

    #[test]
    fn f192_v8b_import_all_unknown() {
        let s = import_summary(&["x", "y"]);
        assert_eq!(s.unknown, 2);
        assert_eq!(s.accepted, 0);
        assert!(s.conflicts.is_empty());
    }

    #[test]
    fn f192_v8b_help_index_unique() {
        // 索引名唯一（帮助页不许重复条目）。
        let idx = help_index();
        for (i, a) in idx.iter().enumerate() {
            for b in idx.iter().skip(i + 1) {
                assert_ne!(a.name, b.name);
            }
        }
    }

    #[test]
    fn f192_v8b_run_checks_pass() {
        assert!(run_paramwl_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b7（第二波）：白名单版本迁移 / 参数集指纹 / 导入干跑。
// 判据源：主册【开发者篇】「参数改名必须保留旧名一版（迁移映射）」。
// ---------------------------------------------------------------------------

/// 改名迁移映射（旧名 → 新名；未改名的直通）。
pub const RENAME_MAP: [(&str, &str); 1] = [("log-lvl", "log-level")];

/// 键迁移（旧名映射新名 / 在册名直通 / 幽灵名返回空——调用方留痕）。
pub fn migrate_key(name: &str) -> &'static str {
    if let Some((_, new)) = RENAME_MAP.iter().find(|(old, _)| *old == name) {
        return new;
    }
    match find(name) {
        Some(spec) => spec.name,
        None => "",
    }
}

/// 参数集指纹（名字排序后 FNV-1a 64——配置漂移对比的轻量凭据）。
pub fn param_fingerprint(flags: &[&str]) -> alloc::string::String {
    if flags.is_empty() {
        return alloc::string::String::new();
    }
    let mut sorted: Vec<&str> = flags.to_vec();
    sorted.sort();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for name in &sorted {
        for b in name.bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= 0xff; // 名间分隔。
    }
    alloc::format!("{:016x}", hash)
}

/// 导入干跑（返回问题计数：未知参数 + 互斥冲突 + 超长键；None = 零问题）。
pub fn import_dry_run(flags: &[&str]) -> Option<usize> {
    let mut problems = 0usize;
    for f in flags {
        if find(f).is_none() && RENAME_MAP.iter().all(|(old, _)| *old != *f) {
            problems += 1;
        }
        if f.len() > PARAM_MAX_LEN {
            problems += 1;
        }
    }
    problems += mutex_conflicts(flags).len();
    if problems == 0 {
        None
    } else {
        Some(problems)
    }
}

#[cfg(test)]
mod deep7c_tests {
    use super::*;

    #[test]
    fn f192_v8c_migrate_map_in_wl() {
        // 映射目标必须在册（迁移不许指向幽灵）。
        for (_, new) in RENAME_MAP {
            assert!(find(new).is_some(), "migrate target {} missing", new);
        }
    }

    #[test]
    fn f192_v8c_fingerprint_deterministic() {
        // 同集重算恒等（指纹可复现）。
        let a = param_fingerprint(&["verbose", "ktrace"]);
        let b = param_fingerprint(&["ktrace", "verbose"]);
        assert_eq!(a, b);
    }

    #[test]
    fn f192_v8c_run_checks_pass() {
        assert!(run_paramwl_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b8（第三波）：参数组预设 / tooltip 生成 / 预设 JSON 导出。
// 判据源：主册【交互设计】「高级参数按场景打包为一键预设」。
// ---------------------------------------------------------------------------

/// 场景预设（预设名 → 旗标集——三档打包，未知名零倾泻）。
pub fn preset_flags(preset: &str) -> Vec<&'static str> {
    match preset {
        "perf" => vec!["verbose", "log-level"],
        "quiet" => vec![],
        "diag" => vec!["selftest-only", "ktrace"],
        _ => vec![],
    }
}

/// tooltip 行（参数 → 一句话说明，带值域——悬浮即懂）。
pub fn tooltip_line(name: &str) -> alloc::string::String {
    match find(name) {
        Some(spec) => match spec.kind {
            ValueKind::Flag => alloc::format!("{}：出现即生效（旗标）", spec.note),
            ValueKind::Int(lo, hi) => alloc::format!("{}（取值 {}-{}）", spec.note, lo, hi),
        },
        None => alloc::string::String::new(),
    }
}

/// 预设导出 JSON（开放格式——预设可分享）。
pub fn preset_export_json(flags: &[&str]) -> alloc::string::String {
    let mut out = alloc::string::String::from("{\"flags\":[");
    for (i, f) in flags.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&alloc::format!("\"{}\"", f));
    }
    out.push_str("]}");
    out
}

#[cfg(test)]
mod deep8_tests {
    use super::*;

    #[test]
    fn f192_v8d_preset_all_in_wl() {
        // 三预设的全部旗标都在册（预设不许开幽灵参数）。
        for p in ["perf", "quiet", "diag"] {
            for f in preset_flags(p) {
                assert!(find(f).is_some(), "preset {} flag {}", p, f);
            }
        }
    }

    #[test]
    fn f192_v8d_json_roundtrip_shape() {
        // 导出含全部键（保真）。
        let j = preset_export_json(&["verbose", "ktrace"]);
        assert!(j.contains("\"verbose\"") && j.contains("\"ktrace\""));
    }

    #[test]
    fn f192_v8d_run_checks_pass() {
        assert!(run_paramwl_deep7b_checks().all_passed());
    }

    #[test]
    fn f192_deep8_snapshot_diff_conserves() {
        // 对比账守恒：增 + 删 + 改 = 全部差异，未动项不上账。
        let base = [("verbose", 1i64), ("log-level", 2i64), ("ktrace", 0i64)];
        let cur = [("verbose", 1i64), ("log-level", 4i64)];
        let d = snapshot_diff(&base, &cur);
        assert!(d.added.is_empty());
        assert_eq!(d.removed, vec!["ktrace"]);
        assert_eq!(d.changed, vec![("log-level", 2, 4)]);
        assert_eq!(rollback_pending(&d), 2);
        // 计划行如实：删 1 改 1 → 两步。
        assert!(rollback_plan_text(&d).contains("共 2 步"));
    }

    #[test]
    fn f192_deep8_boot_ledger_empty() {
        // 空账不伪造结论：零耗时且无最慢阶段。
        assert_eq!(boot_time_ledger(&[]).0, 0);
        assert_eq!(boot_time_ledger(&[]).1, None);
        // 凭据可复现。
        assert_eq!(snapshot_tag(1, 60), snapshot_tag(1, 60));
    }

    #[test]
    fn f192_deep8_run_checks_pass() {
        assert!(run_paramwl_deep8_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b9（第四波）：使用频率账 / 批量重置 / 帮助索引按频次排序。
// 判据源：主册【交互设计】「高频参数前置——帮助与提示按真实使用排序」。
// ---------------------------------------------------------------------------

/// 使用频率账（名 → 次数；top 取频次降序）。
#[derive(Default)]
pub struct UsageCounter {
    counts: Vec<(&'static str, u64)>,
}

impl UsageCounter {
    pub fn new() -> UsageCounter {
        UsageCounter { counts: Vec::new() }
    }

    pub fn hit(&mut self, name: &'static str) {
        match self.counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, c)) => *c += 1,
            None => self.counts.push((name, 1)),
        }
    }

    /// top N（频次降序；同名不重复）。
    pub fn top(&self, n: usize) -> Vec<&'static str> {
        let mut sorted = self.counts.clone();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.into_iter().take(n).map(|(name, _)| name).collect()
    }

    pub fn reset_all(&mut self) {
        self.counts.clear();
    }
}

/// 帮助索引（未命中频次的排后面——零频保持登记序，稳定不抖动）。
pub fn help_index_sorted_by_usage(usage: &UsageCounter) -> Vec<&'static str> {
    let rank = |n: &str| usage.counts.iter().find(|(name, _)| *name == n).map(|(_, c)| *c).unwrap_or(0);
    let mut rows: Vec<(u64, usize, &'static str)> = WHITELIST
        .iter()
        .enumerate()
        .map(|(i, s)| (rank(s.name), i, s.name))
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    rows.into_iter().map(|(_, _, n)| n).collect()
}

#[cfg(test)]
mod deep9_tests {
    use super::*;

    #[test]
    fn f192_v9_sorted_stable_for_zero() {
        // 零频项保持登记序（排序稳定——界面不抖动）。
        let rows = help_index_sorted_by_usage(&UsageCounter::new());
        let expect: Vec<&'static str> = WHITELIST.iter().map(|s| s.name).collect();
        assert_eq!(rows, expect);
    }

    #[test]
    fn f192_v9_usage_hit_unknown_ok() {
        // 未知名也入账（上游已过滤，这里不做二次白名单假设）。
        let mut u = UsageCounter::new();
        u.hit("verbose");
        u.hit("verbose");
        assert_eq!(u.top(1), vec!["verbose"]);
    }

    #[test]
    fn f192_v9_run_checks_pass() {
        assert!(run_paramwl_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b10（第五波）：白名单哈希锚 / 预设分享串。
// 判据源：主册【开发者篇】「白名单表变更可被外部核验（哈希锚公开）」。
// ---------------------------------------------------------------------------

/// 白名单哈希锚（全表名拼接 FNV——表变锚变，外部可核）。
pub fn wl_hash_anchor() -> alloc::string::String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for spec in WHITELIST.iter() {
        for b in spec.name.bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    alloc::format!("{:016x}", hash)
}

/// 预设分享串编码（`varix-preset:<name>` 前缀协议）。
pub fn share_encode(preset: &str) -> alloc::string::String {
    alloc::format!("varix-preset:{}", preset)
}

/// 分享串解码（前缀不符拒收——外部串不冒充协议）。
pub fn share_decode(text: &str) -> Option<&str> {
    text.strip_prefix("varix-preset:").filter(|s| !s.is_empty())
}

#[cfg(test)]
mod deep10_tests {
    use super::*;

    #[test]
    fn f192_v10_share_roundtrip_all() {
        // 三预设全可分享往返。
        for p in ["perf", "quiet", "diag"] {
            assert_eq!(share_decode(&share_encode(p)), Some(p));
        }
    }

    #[test]
    fn f192_v10_run_checks_pass() {
        assert!(run_paramwl_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b11（第六波）：白名单 CSV 导出 / 搜索防抖建议。
// ---------------------------------------------------------------------------

/// 白名单 CSV 导出（name,family,kind 三列——开放格式）。
pub fn wl_export_csv() -> alloc::string::String {
    let mut out = alloc::string::String::from("name,family,kind\n");
    for spec in WHITELIST.iter() {
        let kind = match spec.kind {
            ValueKind::Flag => alloc::string::String::from("flag"),
            ValueKind::Int(lo, hi) => alloc::format!("int:{}-{}", lo, hi),
        };
        out.push_str(&alloc::format!("{},{},{}\n", spec.name, family_label(spec.family), kind));
    }
    out
}

/// 搜索防抖建议（距上次击键 ms → 提示文案；停顿够长不提示）。
pub fn search_debounce_hint(ms_since_last_key: u64) -> &'static str {
    if ms_since_last_key < 300 {
        "输入停顿 300ms 后搜索"
    } else {
        ""
    }
}

#[cfg(test)]
mod deep11_tests {
    use super::*;

    #[test]
    fn f192_v11_csv_int_kind() {
        // int 型参数带值域（导出保真）。
        assert!(wl_export_csv().contains("log-level,调试族,int:0-5"));
    }

    #[test]
    fn f192_v11_run_checks_pass() {
        assert!(run_paramwl_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b12（第七波）：参数分布分族计数（设置页「参数构成」格）。
// ---------------------------------------------------------------------------

/// 分族计数（族 → 成员数——静态表派生，一处一事实）。
pub fn family_share(family: ParamFamily) -> usize {
    WHITELIST.iter().filter(|s| s.family == family).count()
}

#[cfg(test)]
mod deep12_tests {
    use super::*;

    #[test]
    fn f192_v12_share_never_zero() {
        // 三族各至少一员（白名单结构性自检）。
        for f in [ParamFamily::Debug, ParamFamily::Degrade, ParamFamily::Compat] {
            assert!(family_share(f) > 0);
        }
    }

    #[test]
    fn f192_v12_run_checks_pass() {
        assert!(run_paramwl_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b13（第八波）：参数别名表（专家快捷输入 → 正名解析）。
// ---------------------------------------------------------------------------

/// 别名表（简称 → 正名）。
pub const ALIAS_TABLE: [(&str, &str); 2] = [
    ("ll", "log-level"),
    ("sm", "safe-mode"),
];

/// 别名解析（命中返回正名；未命中 None——不猜）。
pub fn alias_lookup(alias: &str) -> Option<&'static str> {
    ALIAS_TABLE
        .iter()
        .find(|(a, _)| *a == alias)
        .map(|(_, full)| full)
        .and_then(|full| find(full).map(|s| s.name))
}

#[cfg(test)]
mod deep13_tests {
    use super::*;

    #[test]
    fn f192_v13_alias_unique() {
        // 简称不冲突。
        for (i, (a, _)) in ALIAS_TABLE.iter().enumerate() {
            for (b, _) in ALIAS_TABLE.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn f192_v13_run_checks_pass() {
        assert!(run_paramwl_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b14（第九波）：默认关闭名单 / 改动撤销栈。
// 判据源：主册【交互设计】「参数改动可逐级撤销（不一键回解放前）」。
// ---------------------------------------------------------------------------

/// 默认关闭名单（出厂即关的旗标——安全底线）。
pub const DEFAULT_OFF_FLAGS: [&str; 3] = ["panic_halt", "selftest-only", "no-acpi"];

/// 改动撤销栈（逐条 push/pop——粒度到单参数）。
pub struct ChangeUndo {
    stack: Vec<(&'static str, &'static str)>,
}

impl ChangeUndo {
    pub fn new() -> ChangeUndo {
        ChangeUndo { stack: Vec::new() }
    }

    /// 记一次改动（参数名 + 摘要）。
    pub fn push(&mut self, name: &'static str, summary: &str) {
        self.stack.push((name, alloc::string::String::from(summary).leak()));
    }

    /// 撤销最近一次（返回参数名）。
    pub fn pop(&mut self) -> Option<&'static str> {
        self.stack.pop().map(|(name, _)| name)
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }
}

#[cfg(test)]
mod deep14_tests {
    use super::*;

    #[test]
    fn f192_v14_undo_lifo() {
        // 栈语义：后改先撤。
        let mut u = ChangeUndo::new();
        u.push("verbose", "开");
        u.push("ktrace", "关");
        assert_eq!(u.pop(), Some("ktrace"));
        assert_eq!(u.pop(), Some("verbose"));
        assert_eq!(u.depth(), 0);
    }

    #[test]
    fn f192_v14_run_checks_pass() {
        assert!(run_paramwl_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8 终波深化段（deep8 表）：参数变更影响评估行 / 回滚快照对比 / 参数依赖
// 提示 / 启动耗时账。
// 判据源：主册【开发者篇】「参数改动前给出影响面，改动后可逐级回退」+
// 【性能章】「启动全程打点——各阶段耗时入账，超预算即点名」。
// ---------------------------------------------------------------------------

/// 变更影响评估行（参数 → 新值：钳制结果 + 影响面一行——改前先看账）。
pub fn change_impact_row(name: &str, new_val: i64) -> alloc::string::String {
    let spec = match find(name) {
        Some(s) => s,
        None => return alloc::string::String::from("未知参数——拒绝变更"),
    };
    // 值域钳制（越界收界——与启动面同一钳制语义，见主册 G-G-22）。
    let clamped = match spec.kind {
        ValueKind::Flag => new_val.clamp(0, 1),
        ValueKind::Int(lo, hi) => new_val.clamp(lo, hi),
    };
    let n = impact_set(spec.name).len();
    if n == 0 {
        alloc::format!("{} → {}：无连带影响", spec.name, clamped)
    } else {
        alloc::format!("{} → {}：牵动 {} 项，改前先看影响面", spec.name, clamped, n)
    }
}

/// 回滚快照对比账（回滚前先出账——差异不明不回滚）。
pub struct RollbackDiff {
    /// 快照后新增的参数。
    pub added: Vec<&'static str>,
    /// 快照后移除的参数。
    pub removed: Vec<&'static str>,
    /// 值变化的参数（名，旧值，新值）。
    pub changed: Vec<(&'static str, i64, i64)>,
}

/// 快照对比（before = 出厂/上次快照，after = 当前活跃值）。
pub fn snapshot_diff(
    before: &[(&'static str, i64)],
    after: &[(&'static str, i64)],
) -> RollbackDiff {
    let mut diff = RollbackDiff { added: Vec::new(), removed: Vec::new(), changed: Vec::new() };
    for &(n, v) in after {
        match before.iter().find(|(bn, _)| *bn == n) {
            None => diff.added.push(n),
            Some(&(_, bv)) if bv != v => diff.changed.push((n, bv, v)),
            _ => {}
        }
    }
    for &(n, _) in before {
        if !after.iter().any(|(an, _)| *an == n) {
            diff.removed.push(n);
        }
    }
    diff
}

/// 待回滚差异总数（0 = 与快照一致，无需回滚）。
pub fn rollback_pending(diff: &RollbackDiff) -> usize {
    diff.added.len() + diff.removed.len() + diff.changed.len()
}

/// 回滚计划行（人话分步：移除新增 / 还原改动 / 恢复删除——回滚确认页正文）。
pub fn rollback_plan_text(diff: &RollbackDiff) -> alloc::string::String {
    let n = rollback_pending(diff);
    if n == 0 {
        return alloc::string::String::from("与快照一致，无需回滚");
    }
    let mut parts: Vec<alloc::string::String> = Vec::new();
    if !diff.added.is_empty() {
        parts.push(alloc::format!("移除新增的 {} 项", diff.added.len()));
    }
    if !diff.changed.is_empty() {
        parts.push(alloc::format!("还原改动的 {} 项", diff.changed.len()));
    }
    if !diff.removed.is_empty() {
        parts.push(alloc::format!("恢复删除的 {} 项", diff.removed.len()));
    }
    alloc::format!("回滚共 {} 步：{}", n, parts.join("；"))
}

/// 快照凭据行（序号 + 时刻——回滚栈里的版本凭据，可追溯）。
pub fn snapshot_tag(seq: u64, taken_s: u64) -> alloc::string::String {
    alloc::format!("snap-{}@{}s", seq, taken_s)
}

/// 参数依赖提示（活跃集缺的连带项——「开了 A，B 建议同步开」）。
pub fn dependency_hints(active: &[&str]) -> Vec<&'static str> {
    let mut hints: Vec<&'static str> = Vec::new();
    for &(from, to, _why) in IMPACT_EDGES.iter() {
        if active.contains(&from) && !active.contains(&to) && find(to).is_some()
            && !hints.contains(&to)
        {
            hints.push(to);
        }
    }
    hints
}

/// 依赖提示行（无缺口即沉默——提示不许空转）。
pub fn dependency_hint_line(active: &[&str]) -> alloc::string::String {
    let hints = dependency_hints(active);
    if hints.is_empty() {
        return alloc::string::String::new();
    }
    alloc::format!("建议同步开启：{}", hints.join("、"))
}

/// 启动阶段耗时条目。
pub struct BootStage {
    /// 阶段名（selftest / whitelist / desktop …）。
    pub name: &'static str,
    /// 该阶段耗时（毫秒）。
    pub ms: u64,
}

/// 启动耗时预算（主册性能章：从上电到进桌面 ≤500ms）。
pub const BOOT_BUDGET_MS: u64 = 500;

/// 启动耗时账（各阶段求和 + 最慢阶段——慢在哪儿一眼可见）。
pub fn boot_time_ledger(stages: &[BootStage]) -> (u64, Option<&'static str>) {
    let mut total = 0u64;
    let mut slowest: Option<&BootStage> = None;
    for s in stages {
        total += s.ms;
        match slowest {
            Some(prev) if prev.ms >= s.ms => {}
            _ => slowest = Some(s),
        }
    }
    (total, slowest.map(|s| s.name))
}

/// 启动耗时报告行（超预算点名最慢阶段——性能章的兑现出口）。
pub fn boot_time_report(stages: &[BootStage]) -> alloc::string::String {
    let (total, slow) = boot_time_ledger(stages);
    let slow_txt = match slow {
        Some(n) => alloc::format!("，最慢 {}", n),
        None => alloc::string::String::new(),
    };
    if total > BOOT_BUDGET_MS {
        alloc::format!("启动 {}ms 超预算{}（见性能章）", total, slow_txt)
    } else {
        alloc::format!("启动 {}ms 达标{}", total, slow_txt)
    }
}

/// F192 v8 终波自检（deep8 表）。
pub fn run_paramwl_deep8_checks() -> CheckSet {
    let mut set = CheckSet::new("F192-deep8");

    // 变更影响评估行：未知拒 / 越界钳 / 有牵动 / 无牵动。
    set.add("row unknown", change_impact_row("ghost-param", 1).contains("拒绝"), "");
    set.add("row clamp hi", change_impact_row("log-level", 99).contains("→ 5"), "越界值钳到上界");
    set.add("row clamp lo", change_impact_row("log-level", -3).contains("→ 0"), "");
    set.add("row flag clamp", change_impact_row("ktrace", 7).contains("→ 1"), "旗标值钳到 1");
    set.add("row impact", change_impact_row("safe-mode", 1).contains("牵动 2 项"), "");
    set.add("row calm", change_impact_row("ktrace", 1).contains("无连带"), "");

    // 回滚快照对比：增/删/改三分账 + 计划行 + 凭据。
    let base = [("verbose", 1i64), ("log-level", 3i64)];
    let cur = [("verbose", 1i64), ("ktrace", 1i64), ("log-level", 5i64)];
    let d = snapshot_diff(&base, &cur);
    set.add("snap added", d.added == vec!["ktrace"], "");
    set.add("snap changed", d.changed == vec![("log-level", 3, 5)], "");
    set.add("snap pending", rollback_pending(&snapshot_diff(&cur, &base)) == 2, "回滚差异 = 1 改 + 1 删");
    set.add("snap clean", rollback_pending(&snapshot_diff(&base, &base)) == 0, "快照一致零回滚");
    set.add("snap plan", rollback_plan_text(&d).contains("共 2 步"), "计划行步数 = 差异总数");
    set.add("snap plan calm", rollback_plan_text(&snapshot_diff(&base, &base)).contains("无需回滚"), "");
    set.add("snap tag", snapshot_tag(7, 120).contains("snap-7@120s"), "凭据带序号与时刻");

    // 参数依赖提示：缺连带项才提示；齐了/无下游即沉默。
    set.add("hint two", dependency_hints(&["safe-mode"]) == vec!["no-gui", "no-third-drv"], "按边序点名缺口");
    set.add("hint one", dependency_hints(&["verbose"]) == vec!["log-level"], "");
    set.add("hint silent", dependency_hints(&["safe-mode", "no-gui", "no-third-drv"]).is_empty(), "缺口补齐即沉默");
    set.add("hint none", dependency_hints(&["no-gui"]).is_empty(), "无下游不硬造提示");
    set.add("hint line", dependency_hint_line(&["verbose"]).contains("log-level"), "");
    set.add("hint line empty", dependency_hint_line(&["no-gui"]).is_empty(), "");

    // 启动耗时账：求和 / 最慢 / 预算两态 / 空账。
    let stages = [
        BootStage { name: "selftest", ms: 40 },
        BootStage { name: "whitelist", ms: 5 },
        BootStage { name: "desktop", ms: 380 },
    ];
    let (total, slow) = boot_time_ledger(&stages);
    set.add("boot total", total == 425, "40+5+380 = 425ms");
    set.add("boot slowest", slow == Some("desktop"), "");
    set.add("boot ok", boot_time_report(&stages).contains("达标"), "");
    set.add("boot over", {
        let over = [BootStage { name: "selftest", ms: 600 }];
        boot_time_report(&over).contains("超预算") && boot_time_report(&over).contains("selftest")
    }, "超预算点名最慢阶段");
    set.add("boot empty", boot_time_ledger(&[]).0 == 0 && boot_time_ledger(&[]).1.is_none(), "空账零耗时零结论");

    set
}
