//! diag3 — WP-204 · B-605 诊断三件套（MD2 篇 6.4）。
//!
//! 判据 B-605：JSON 与人读双格式，判例取证可用。
//! MD2 原文（6.4）："vx-ping（ICMP 回显与统计）、vx-route（路由表与接口
//! 状态）、vx-capture（抓包落盘，pcap 格式，与通用分析工具互通）是网络
//! 判例的取证基础。实现要点：三件套走内核诊断通道而非普通 socket（raw
//! 权限收敛在系统工具里），输出全部机器可读（JSON 行）加人类可读双格式
//! ——星卡脚本可以直接消费，人也能直接看。判例挂钩：Steam 登录
//! （SC-041）、git 推拉（SC-005）等网络相关判例的失败归因第一步都是
//! 三件套取证。"
//!
//! 宿主可测形态：三工具枚举（ping/route/capture）+ 诊断通道权限模型
//! （系统工具身份可取、应用身份被拒——raw 权限收敛）+ 单一报告数据源
//! 双格式渲染（JSON 行字段序列 / 人读字段序列）+ 双格式逐字段一致对账
//! （同一份数据两种渲染，内容零漂移）+ 判例挂钩常量（SC-041/SC-005）。

use crate::checks::CheckSet;

/// 报告字段容量（每份报告最多字段数）。
pub const FIELD_CAP: usize = 8;
/// 诊断通道 nonce 容量（报告环对账用）。
pub const REPORT_CAP: usize = 16;

/// 诊断三件套（MD1 第 17.4 节取证基础）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VxTool {
    /// ICMP 回显与统计。
    Ping,
    /// 路由表与接口状态。
    Route,
    /// 抓包落盘（pcap 格式，与通用分析工具互通）。
    Capture,
}

pub const TOOLS_ALL: [VxTool; 3] = [VxTool::Ping, VxTool::Route, VxTool::Capture];

/// 判例挂钩（MD2 6.4：网络判例失败归因第一步是三件套取证）。
pub const CASE_STEAM_LOGIN: &str = "SC-041";
pub const CASE_GIT_PUSH: &str = "SC-005";

/// 调用方身份（raw 权限收敛在系统工具里）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Caller {
    /// 系统工具（vx-* 本体）——诊断通道唯一合法身份。
    SystemTool,
    /// 普通应用——不得走诊断通道（不许绕过开 raw socket）。
    App,
}

/// 工具名（JSON/人读渲染共用 key 面）。
pub const fn tool_name(t: VxTool) -> &'static str {
    match t {
        VxTool::Ping => "vx-ping",
        VxTool::Route => "vx-route",
        VxTool::Capture => "vx-capture",
    }
}

/// 探针报告（**唯一数据源**——双格式从这里渲染，内容零漂移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeReport {
    pub tool: VxTool,
    /// 字段序列（key 固定序；val 整数化——宿主模型零 f32）。
    pub keys: [&'static str; FIELD_CAP],
    pub vals: [u64; FIELD_CAP],
    pub n: usize,
}

/// 探针数据生成（确定性——同 seed 同报告）。
pub fn probe(tool: VxTool, seed: u64) -> ProbeReport {
    let mut g = crate::comprecover::Lcg(seed);
    let _ = g.next();
    match tool {
        VxTool::Ping => ProbeReport {
            tool,
            keys: ["tool", "sent", "recv", "loss_permille", "avg_rtt_us", "", "", ""],
            vals: [1, 10, 10 - (g.next() % 3), (g.next() % 3) * 100, 800 + g.next() % 400, 0, 0, 0],
            n: 5,
        },
        VxTool::Route => ProbeReport {
            tool,
            keys: ["tool", "ifaces", "routes", "default_gw", "", "", "", ""],
            vals: [2, 2, 4 + g.next() % 3, 0x0A00_0001, 0, 0, 0, 0],
            n: 4,
        },
        VxTool::Capture => ProbeReport {
            tool,
            keys: ["tool", "frames", "format_pcap", "bytes", "", "", "", ""],
            vals: [3, 100 + g.next() % 900, 1, (100 + g.next() % 900) * 1500, 0, 0, 0, 0],
            n: 4,
        },
    }
}

/// JSON 行格式的一个字段（机器可读面——星卡脚本直接消费）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JsonField {
    pub key: &'static str,
    pub val: u64,
}

/// 人读格式的一个字段（人也能直接看）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HumanField {
    pub key: &'static str,
    pub val: u64,
}

/// JSON 行渲染：`{"key":val,...}` 的字段序列模型（渲染面）。
pub fn render_json(r: &ProbeReport) -> ([JsonField; FIELD_CAP], usize) {
    let mut out = [JsonField { key: "", val: 0 }; FIELD_CAP];
    for i in 0..r.n {
        out[i] = JsonField { key: r.keys[i], val: r.vals[i] };
    }
    (out, r.n)
}

/// 人读渲染：`key = val` 的字段序列模型。
pub fn render_human(r: &ProbeReport) -> ([HumanField; FIELD_CAP], usize) {
    let mut out = [HumanField { key: "", val: 0 }; FIELD_CAP];
    for i in 0..r.n {
        out[i] = HumanField { key: r.keys[i], val: r.vals[i] };
    }
    (out, r.n)
}

/// 双格式一致对账：字段数、顺序、key、val 全部一致（判据核心）。
pub fn formats_consistent(r: &ProbeReport) -> bool {
    let (j, nj) = render_json(r);
    let (h, nh) = render_human(r);
    nj == nh
        && (0..nj).all(|i| j[i].key == h[i].key && j[i].val == h[i].val && !j[i].key.is_empty())
}

/// 内核诊断通道：raw 权限收敛点（非普通 socket）。
pub struct DiagChannel {
    pub taken: [bool; REPORT_CAP],
    pub n: usize,
}

impl DiagChannel {
    pub const fn new() -> DiagChannel {
        DiagChannel { taken: [false; REPORT_CAP], n: 0 }
    }

    /// 取报告：系统工具放行；应用身份拒绝（留痕：不消耗环槽）。
    /// 报告**即时返回调用方**（非排队缓冲）——取数记录环形滚动只留
    /// 最近 REPORT_CAP 条：取证永不因记录环满而失败。
    pub fn fetch(&mut self, who: Caller, tool: VxTool, seed: u64) -> Result<ProbeReport, &'static str> {
        match who {
            Caller::SystemTool => {
                self.taken[self.n % REPORT_CAP] = true;
                self.n += 1;
                Ok(probe(tool, seed))
            }
            Caller::App => Err("raw 权限收敛在系统工具里"),
        }
    }
}

// ---------------------------------------------------------------- 对练

/// 三件套对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct DiagDrillSummary {
    pub rounds: u32,
    pub probes: u64,
    /// 双格式逐字段一致
    pub consistent: bool,
    /// 权限收敛（应用身份全拒、系统工具全放）
    pub auth_correct: bool,
}

/// 随机工具 × 确定性 seed 对练：双格式一致 + 权限面。
pub fn run_diag_drills(seed: u64, rounds: u32) -> DiagDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = DiagDrillSummary::default();
    sum.rounds = rounds;
    sum.consistent = true;
    sum.auth_correct = true;
    let mut ch = DiagChannel::new();
    for i in 0..rounds {
        let tool = TOOLS_ALL[(g.next() % 3) as usize];
        let s = seed.wrapping_add(i as u64);
        // 系统工具：可取 + 双格式一致
        match ch.fetch(Caller::SystemTool, tool, s) {
            Ok(r) => {
                sum.probes += 1;
                if r.tool != tool || !formats_consistent(&r) {
                    sum.consistent = false;
                }
            }
            Err(_) => sum.auth_correct = false,
        }
        // 应用身份：必拒
        if ch.fetch(Caller::App, tool, s).is_ok() {
            sum.auth_correct = false;
        }
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_diag3_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-605 诊断三件套");
    {
        // 三工具齐
        set.add(
            "B-605 三件套工具齐",
            TOOLS_ALL.len() == 3
                && tool_name(VxTool::Ping) == "vx-ping"
                && tool_name(VxTool::Route) == "vx-route"
                && tool_name(VxTool::Capture) == "vx-capture",
            "vx-ping / vx-route / vx-capture（MD1 17.4）",
        );
    }
    {
        // raw 权限收敛：应用身份被拒
        let mut ch = DiagChannel::new();
        set.add(
            "B-605 raw 权限收敛（应用被拒）",
            ch.fetch(Caller::App, VxTool::Ping, 1).is_err() && ch.n == 0,
            "走内核诊断通道而非普通 socket",
        );
    }
    {
        // 系统工具身份可取三件套
        let mut ch = DiagChannel::new();
        let ok = TOOLS_ALL.iter().all(|t| ch.fetch(Caller::SystemTool, *t, 7).is_ok());
        set.add(
            "B-605 系统工具可取三件套",
            ok && ch.n == 3,
            "raw 权限收敛在系统工具里",
        );
    }
    {
        // 双格式字段数一致
        let r = probe(VxTool::Ping, 42);
        set.add(
            "B-605 双格式字段数一致",
            render_json(&r).1 == render_human(&r).1 && render_json(&r).1 == r.n,
            "同一数据源两种渲染",
        );
    }
    {
        // 双格式逐字段一致（对账恒等式）
        let r = probe(VxTool::Capture, 42);
        set.add(
            "B-605 双格式逐字段一致",
            formats_consistent(&r),
            "JSON 行与人读内容零漂移——星卡可消费人也可读",
        );
    }
    {
        // 确定性：同 seed 同报告（取证可复现）
        let a = probe(VxTool::Route, 99);
        let b = probe(VxTool::Route, 99);
        set.add(
            "B-605 取证可复现",
            a == b,
            "同 seed 同报告——判例归因的复现面",
        );
    }
    {
        // 判例挂钩常量
        set.add(
            "B-605 判例挂钩在册",
            CASE_STEAM_LOGIN == "SC-041" && CASE_GIT_PUSH == "SC-005",
            "Steam 登录 / git 推拉失败归因第一步是三件套取证",
        );
    }
    {
        // 三件套对练
        let sum = run_diag_drills(0xB605, 60);
        set.add(
            "B-605 三件套对练",
            sum.rounds == 60 && sum.consistent && sum.auth_correct && sum.probes == 60,
            "JSON 与人读双格式，判例取证可用（判据原文）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f705_auth_gate() {
        let mut ch = DiagChannel::new();
        assert!(ch.fetch(Caller::App, VxTool::Ping, 1).is_err());
        assert!(ch.fetch(Caller::SystemTool, VxTool::Ping, 1).is_ok());
        assert_eq!(ch.n, 1, "拒绝不消耗环槽");
    }

    #[test]
    fn f705_dual_format_consistent() {
        for t in TOOLS_ALL {
            let r = probe(t, 5);
            assert!(formats_consistent(&r), "{} 双格式必须逐字段一致", tool_name(t));
        }
    }

    #[test]
    fn f705_probe_deterministic() {
        let a = probe(VxTool::Ping, 123);
        let b = probe(VxTool::Ping, 123);
        assert_eq!(a, b);
        let c = probe(VxTool::Ping, 124);
        assert_ne!(a, c);
    }

    #[test]
    fn f705_drill_deterministic() {
        let a = run_diag_drills(11, 30);
        let b = run_diag_drills(11, 30);
        assert_eq!(a, b);
        assert!(a.consistent && a.auth_correct);
        assert_eq!(a.probes, 30);
    }
}
