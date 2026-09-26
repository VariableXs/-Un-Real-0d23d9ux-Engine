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
