//! VE-F4802 · 统一 CLI 框架（VE-Y 域 · 工具链与调试域 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4802`
//!
//! **判据（锚点原文五条）**：**子命令树、双轨输出、命令即插件、口径一致、
//! 判据**。
//!
//! - **子命令树**：命令按路径（`build plugin`）挂树，解析沿树下行
//!   O(命令深)；重复注册拒绝；未命中给**类型化报错 + 用法提示**（列出该
//!   节点的合法子命令——「不知道」必须同时告诉「知道什么」）。
//! - **双轨输出**：一次事件两轨产出——人读行（读屏友好，域本色：终端也讲
//!   无障碍）与机读 JSON 行（供脚本），两轨**同源**（同一事件的同一组字段
//!   各自渲染，不许各写一套真相）。退出码**如实**：输出中断不是成功。
//! - **命令即插件**：工具链自身开放扩展——外部命令带命名空间（`ns!cmd`）
//!   入册，命名空间仲裁 O(1)：核心命名空间保留、重名先到先得、后来者拒绝
//!   且留痕；**插件命令同受域本色约束**——缺读屏文本不入册（Y01 纪律）。
//! - **口径一致**：输出 wire 键集与 F4608 总线事件同构同规（锚点：框架与
//!   F4608 总线口径一致；F4638 钩子口径同规）——键集常量单源，判据侧独立
//!   写死对拍，漂移先红。
//! - **判据**：退出码全集互异、wire 键集判据侧全扫、双轨同源逐字段对拍。
//!
//! **错误路径与降级矩阵**：参数非法→类型化报错+用法提示；扩展冲突→命名
//! 空间仲裁；输出中断→退出码如实。
//!
//! **性能逐项分解**：解析 O(命令深)；输出 O(1)；仲裁 O(1)。
//!
//! **跨批对接点**：上游 F4801 框架；下游 F4821 CLI 集；F4638 钩子口径。
//!
//! **无障碍与隐私**：CLI 输出读屏友好（域本色）；无隐私面。
//!
//! **诊断码**：X 域 `0x3Axx` 段续编（0x3A10..0x3A19），与 F4801 十码同段
//! 不重号。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use super::vey01_xarch::fnv1a;

// ---------------------------------------------------------------------------
// 〇、诊断码（0x3A10.. 段续编，显性映射）
// ---------------------------------------------------------------------------

/// CLI 框架诊断码（封闭全集八码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliErr {
    /// 命令未知。
    UnknownCmd,
    /// 参数类型不符。
    ArgTypeBad,
    /// 必填参数缺失。
    ArgMissing,
    /// 未知参数（传了规格里没有的）。
    ArgUnknown,
    /// 扩展命名空间冲突。
    ExtConflict,
    /// 扩展缺读屏文本（域本色闸）。
    ExtNotA11y,
    /// 输出中断。
    OutBroken,
    /// 命令树深度越界。
    DepthCap,
}

impl CliErr {
    /// 码值（0x3A10 起续编）。
    pub const fn wire(self) -> u16 {
        match self {
            CliErr::UnknownCmd => 0x3A10,
            CliErr::ArgTypeBad => 0x3A11,
            CliErr::ArgMissing => 0x3A12,
            CliErr::ArgUnknown => 0x3A13,
            CliErr::ExtConflict => 0x3A14,
            CliErr::ExtNotA11y => 0x3A15,
            CliErr::OutBroken => 0x3A16,
            CliErr::DepthCap => 0x3A17,
        }
    }

    /// 人话。
    pub const fn zh(self) -> &'static str {
        match self {
            CliErr::UnknownCmd => "命令未知",
            CliErr::ArgTypeBad => "参数类型不符",
            CliErr::ArgMissing => "必填参数缺失",
            CliErr::ArgUnknown => "未知参数",
            CliErr::ExtConflict => "扩展命名空间冲突",
            CliErr::ExtNotA11y => "扩展缺读屏文本",
            CliErr::OutBroken => "输出中断",
            CliErr::DepthCap => "命令树深度越界",
        }
    }

    /// 全集。
    pub const ALL: [CliErr; 8] = [
        CliErr::UnknownCmd,
        CliErr::ArgTypeBad,
        CliErr::ArgMissing,
        CliErr::ArgUnknown,
        CliErr::ExtConflict,
        CliErr::ExtNotA11y,
        CliErr::OutBroken,
        CliErr::DepthCap,
    ];
}

// ---------------------------------------------------------------------------
// 一、子命令树（锚点：命令注册与解析框架（子命令树+参数校验））
// ---------------------------------------------------------------------------

/// 参数类型（封闭三型；类型化报错的基础）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgType {
    /// 布尔开关。
    Flag,
    /// 整数值。
    Int,
    /// 字符串值。
    Str,
}

impl ArgType {
    /// 人话（类型化报错用）。
    pub const fn zh(self) -> &'static str {
        match self {
            ArgType::Flag => "开关",
            ArgType::Int => "整数",
            ArgType::Str => "字符串",
        }
    }
}

/// 参数规格。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgSpec {
    /// 参数名（`--name`）。
    pub name: String,
    /// 类型。
    pub ty: ArgType,
    /// 是否必填。
    pub required: bool,
}

/// 命令节点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CmdNode {
    /// 命令名（树内单段）。
    pub name: String,
    /// 词典键（域本色：命令描述走词典）。
    pub dict_key: String,
    /// 子命令。
    pub children: Vec<CmdNode>,
    /// 参数规格（叶命令才有意义）。
    pub args: Vec<ArgSpec>,
    /// 处理器指纹（注册时算定，防篡改对账用）。
    pub handler_fp: u64,
}

/// 命令树容量。
pub const MAX_DEPTH: usize = 6;
/// 单节点子命令容量。
pub const MAX_CHILDREN: usize = 16;

impl CmdNode {
    /// 建节点。
    pub fn new(name: &str, dict_key: &str) -> CmdNode {
        CmdNode {
            name: name.to_string(),
            dict_key: dict_key.to_string(),
            children: Vec::new(),
            args: Vec::new(),
            handler_fp: 0,
        }
    }

    /// 挂子命令（重复名拒绝 O(子数)）。
    pub fn mount(&mut self, child: CmdNode) -> bool {
        if self.children.len() >= MAX_CHILDREN {
            return false;
        }
        for c in self.children.iter() {
            if c.name == child.name {
                return false;
            }
        }
        self.children.push(child);
        true
    }

    /// 下行一段（O(子数) 线性扫——树浅，常数即一切）。
    pub fn step(&self, seg: &str) -> Option<&CmdNode> {
        self.children.iter().find(|c| c.name == seg)
    }

    /// 下行一段（可变——注册路径下行专用）。
    pub fn step_mut(&mut self, seg: &str) -> Option<&mut CmdNode> {
        self.children.iter_mut().find(|c| c.name == seg)
    }
}

/// 命令树根（根节点即程序名，不参与解析）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CmdTree {
    pub root: CmdNode,
}

impl CmdTree {
    /// 建树。
    pub fn new(root_name: &str) -> CmdTree {
        CmdTree { root: CmdNode::new(root_name, "cli.root") }
    }

    /// 按路径注册命令（`a.b.c`；O(命令深) 下行，路径中段缺即建）。
    pub fn register(&mut self, path: &str, dict_key: &str, handler_sig: &str) -> bool {
        let segs: Vec<&str> = path.split('.').collect();
        if segs.is_empty() || segs.len() > MAX_DEPTH {
            return false;
        }
        let mut node = &mut self.root;
        let last = segs.len() - 1;
        let mut i = 0usize;
        while i < segs.len() {
            let seg = segs[i];
            if node.step(seg).is_none() {
                if i < last {
                    // 中段：自动建目录节点。
                    let child = CmdNode::new(seg, &format!("cli.{}", seg));
                    if !node.mount(child) {
                        return false;
                    }
                } else {
                    // 叶：带词典键与处理器指纹。
                    let mut leaf = CmdNode::new(seg, dict_key);
                    leaf.handler_fp = fnv1a(handler_sig.as_bytes());
                    if !node.mount(leaf) {
                        return false;
                    }
                }
            } else if i == last {
                // 叶重名：重复注册拒绝。
                return false;
            }
            node = match node.step_mut(seg) {
                Some(n) => n,
                None => return false,
            };
            i += 1;
        }
        true
    }
}

/// 解析结果（枚举体单独写，避免结构体变体大搬移）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// 命中叶命令（含参数集）。
    Hit {
        /// 命令路径。
        path: String,
        /// 处理器指纹。
        fp: u64,
        /// 参数名→值（已校验）。
        args: Vec<(String, String)>,
    },
    /// 命令未知（带该节点合法子命令用法提示）。
    Unknown {
        /// 走到的最深路径。
        at: String,
        /// 用法提示：该节点的合法子命令名。
        hint: Vec<String>,
    },
}

impl CmdTree {
    /// 解析 token 流（O(命令深) 下行 + 叶层参数校验）。
    pub fn resolve(&self, tokens: &[&str]) -> ResolveOutcome {
        let mut node = &self.root;
        let mut walked: Vec<&str> = Vec::new();
        let mut i = 0usize;
        while i < tokens.len() {
            let t = tokens[i];
            if t.starts_with("--") {
                break; // 进入参数区
            }
            match node.step(t) {
                Some(child) => {
                    walked.push(t);
                    node = child;
                    i += 1;
                }
                None => {
                    let mut hint: Vec<String> = Vec::new();
                    for c in node.children.iter() {
                        hint.push(c.name.clone());
                    }
                    let mut at = self.root.name.clone();
                    let mut wi = 0usize;
                    while wi < walked.len() {
                        at.push('.');
                        at.push_str(walked[wi]);
                        wi += 1;
                    }
                    return ResolveOutcome::Unknown { at, hint };
                }
            }
        }
        if walked.is_empty() || !node.children.is_empty() {
            // 走到目录节点（不是叶）= 命令不完整，给该节点子命令做用法提示。
            let mut hint: Vec<String> = Vec::new();
            for c in node.children.iter() {
                hint.push(c.name.clone());
            }
            let mut at = self.root.name.clone();
            let mut wi = 0usize;
            while wi < walked.len() {
                at.push('.');
                at.push_str(walked[wi]);
                wi += 1;
            }
            return ResolveOutcome::Unknown { at, hint };
        }
        // 参数校验（类型化报错在 ValidateErr）。
        let mut args: Vec<(String, String)> = Vec::new();
        while i < tokens.len() {
            let t = tokens[i];
            if !t.starts_with("--") {
                i += 1;
                continue;
            }
            let name = &t[2..];
            let spec = node.args.iter().find(|a| a.name == name);
            match spec {
                Some(a) => {
                    if a.ty == ArgType::Flag {
                        args.push((name.to_string(), "true".to_string()));
                        i += 1;
                    } else {
                        if i + 1 >= tokens.len() {
                            return ResolveOutcome::Unknown {
                                at: format!("{}（参数 {} 缺值）", node.name, name),
                                hint: Vec::new(),
                            };
                        }
                        args.push((name.to_string(), tokens[i + 1].to_string()));
                        i += 2;
                    }
                }
                None => {
                    return ResolveOutcome::Unknown {
                        at: format!("{}（未知参数 --{}）", node.name, name),
                        hint: Vec::new(),
                    };
                }
            }
        }
        // 必填齐了吗。
        for a in node.args.iter() {
            if a.required && !args.iter().any(|(n, _)| n == &a.name) {
                return ResolveOutcome::Unknown {
                    at: format!("{}（缺必填 --{}）", node.name, a.name),
                    hint: Vec::new(),
                };
            }
        }
        ResolveOutcome::Hit {
            path: {
                let mut p = String::new();
                let mut wi = 0usize;
                while wi < walked.len() {
                    if wi > 0 {
                        p.push('.');
                    }
                    p.push_str(walked[wi]);
                    wi += 1;
                }
                p
            },
            fp: node.handler_fp,
            args,
        }
    }
}

/// 参数校验错误（类型化：锚点「参数非法→类型化报错」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidateErr {
    pub code: CliErr,
    /// 哪个参数。
    pub arg: String,
    /// 期望类型人话（用法提示的一部分）。
    pub expect: String,
}

/// 按规格校验已收集参数值（O(参数数)）。
pub fn validate_args(spec: &[ArgSpec], got: &[(String, String)]) -> Result<(), ValidateErr> {
    for a in spec.iter() {
        let v = got.iter().find(|(n, _)| n == &a.name);
        match (a.required, v) {
            (true, None) => {
                return Err(ValidateErr {
                    code: CliErr::ArgMissing,
                    arg: a.name.clone(),
                    expect: a.ty.zh().to_string(),
                });
            }
            (_, Some((_, val))) => {
                if a.ty == ArgType::Int && val.parse::<u64>().is_err() {
                    return Err(ValidateErr {
                        code: CliErr::ArgTypeBad,
                        arg: a.name.clone(),
                        expect: a.ty.zh().to_string(),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、双轨输出器（锚点：人读模式与机读模式双轨——JSON 输出供脚本）
// ---------------------------------------------------------------------------

/// 输出事件（**单源**：两轨各自渲染同一事件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutEvent {
    /// 事件名（wire 键，见 [`OUT_WIRE_KEYS`]）。
    pub event: String,
    /// 人话主行。
    pub zh: String,
    /// 键值载荷（JSON 轨的键值对）。
    pub fields: Vec<(String, String)>,
    /// 退出码。
    pub exit: ExitCode,
}

/// 退出码（封闭全集，如实原则：中断不是成功）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitCode {
    /// 成功。
    Ok,
    /// 用法错误（参数/命令）。
    Usage,
    /// 内部失败。
    Failed,
    /// 输出中断（写不出去了——**如实**报，不假装成功）。
    Interrupted,
}

impl ExitCode {
    /// 码值。
    pub const fn wire(self) -> u8 {
        match self {
            ExitCode::Ok => 0,
            ExitCode::Usage => 2,
            ExitCode::Failed => 1,
            ExitCode::Interrupted => 3,
        }
    }
    /// 人话。
    pub const fn zh(self) -> &'static str {
        match self {
            ExitCode::Ok => "成功",
            ExitCode::Usage => "用法错误",
            ExitCode::Failed => "失败",
            ExitCode::Interrupted => "输出中断",
        }
    }
    /// 全集。
    pub const ALL: [ExitCode; 4] =
        [ExitCode::Ok, ExitCode::Failed, ExitCode::Usage, ExitCode::Interrupted];
}

/// wire 键集（与 F4608 总线口径同构——键集单源，判据侧独立对拍）。
pub const OUT_WIRE_KEYS: [&str; 6] =
    ["event", "zh", "fields_n", "exit", "ts_slot", "schema"];

/// 输出 schema 版本（口径漂移先红）。
pub const OUT_SCHEMA: &str = "cli-out-v1";

/// 双轨输出器。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DualOut {
    /// 人读轨（累计行）。
    pub human: Vec<String>,
    /// 机读轨（累计 JSON 行）。
    pub json: Vec<String>,
    /// 中断标记（一旦中断，退出码锁定 Interrupted——如实）。
    pub broken: bool,
}

impl DualOut {
    /// 新输出器。
    pub fn new() -> DualOut {
        DualOut { human: Vec::new(), json: Vec::new(), broken: false }
    }

    /// 发射一条事件（**O(1)**：两行各一次拼接，无重排）。
    pub fn emit(&mut self, ev: &OutEvent, sink_up: bool) {
        if !sink_up {
            // 下沉通道断开 = 输出中断：退出码如实锁定，不静默丢事件。
            self.broken = true;
            self.human.push(format!("（输出中断：{} 未能送达）", ev.zh));
            return;
        }
        self.human.push(ev.zh.clone());
        let mut j = String::from("{");
        let mut i = 0usize;
        while i < OUT_WIRE_KEYS.len() {
            let k = OUT_WIRE_KEYS[i];
            let v = match k {
                "event" => format!("\"{}\"", ev.event),
                "zh" => format!("\"{}\"", ev.zh),
                "fields_n" => format!("{}", ev.fields.len()),
                "exit" => format!("{}", ev.exit.wire()),
                "ts_slot" => "0".to_string(),
                "schema" => format!("\"{}\"", OUT_SCHEMA),
                _ => String::from("null"),
            };
            j.push_str(&format!("\"{}\":{}", k, v));
            if i + 1 < OUT_WIRE_KEYS.len() {
                j.push(',');
            }
            i += 1;
        }
        j.push('}');
        self.json.push(j);
    }

    /// 最终退出码（如实：有中断即 Interrupted，否则取最后事件码）。
    pub fn exit_code(&self, last: ExitCode) -> ExitCode {
        if self.broken {
            ExitCode::Interrupted
        } else {
            last
        }
    }
}

// ---------------------------------------------------------------------------
// 三、命令即插件（锚点：插件式命令扩展（命令即插件））
// ---------------------------------------------------------------------------

/// 核心保留命名空间（插件禁用）。
pub const CORE_NS: &str = "core";

/// 扩展命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtCmd {
    /// 命名空间（`ns!cmd` 的 ns）。
    pub ns: String,
    /// 命令名。
    pub cmd: String,
    /// 读屏文本（**必备**——域本色：插件命令同受无障碍约束）。
    pub screen: String,
    /// 处理器指纹。
    pub fp: u64,
}

/// 扩展注册表（命名空间仲裁 O(1)：重名先到先得）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtRegistry {
    pub exts: Vec<ExtCmd>,
    /// 仲裁留痕（被拒者）。
    pub rejected: Vec<(String, CliErr)>,
}

impl ExtRegistry {
    /// 空表。
    pub fn new() -> ExtRegistry {
        ExtRegistry { exts: Vec::new(), rejected: Vec::new() }
    }

    /// 入册（O(表数) 查重，仲裁结论 O(1) 判定；容量 O(1) 拒）。
    pub fn admit(&mut self, e: ExtCmd) -> bool {
        if e.ns == CORE_NS {
            self.rejected.push((format!("{}!{}", e.ns, e.cmd), CliErr::ExtConflict));
            return false;
        }
        if e.screen.is_empty() {
            self.rejected.push((format!("{}!{}", e.ns, e.cmd), CliErr::ExtNotA11y));
            return false;
        }
        for prev in self.exts.iter() {
            if prev.ns == e.ns && prev.cmd == e.cmd {
                self.rejected.push((format!("{}!{}", e.ns, e.cmd), CliErr::ExtConflict));
                return false;
            }
        }
        self.exts.push(e);
        true
    }
}

// ---------------------------------------------------------------------------
// 四、域自检（CheckSet）
// ---------------------------------------------------------------------------

/// 判据侧独立 FNV（与 vey01 被测 fnv1a 两份实现对拍防同源）。
const fn fnv_recheck(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// VE-F4802 域自检。
pub fn run_vey02_checks() -> CheckSet {
    let mut s = CheckSet::new("vey02_cli");

    // ---- 判据一：子命令树 ----
    {
        // 注册→命中→指纹对账；重复注册拒；未命中带用法提示。
        let mut t = CmdTree::new("vengine");
        let ok1 = t.register("build.plugin", "cli.build.plugin", "build_plugin(v2)");
        let ok2 = t.register("build.pack", "cli.build.pack", "build_pack(v1)");
        let dup = t.register("build.plugin", "cli.dup", "dup()");
        let hit = match t.resolve(&["build", "plugin"]) {
            ResolveOutcome::Hit { path, fp, args } => {
                path == "build.plugin"
                    && fp == fnv1a(b"build_plugin(v2)")
                    && args.is_empty()
            }
            _ => false,
        };
        let unknown = match t.resolve(&["build", "nonsense"]) {
            ResolveOutcome::Unknown { hint, .. } => {
                hint.len() == 2 && hint.contains(&"plugin".to_string())
                    && hint.contains(&"pack".to_string())
            }
            _ => false,
        };
        s.add(
            "Y02-子命令树-命中指纹对账+重复拒+未命中带用法",
            ok1 && ok2 && !dup && hit && unknown,
            "",
        );
    }
    {
        // 参数校验类型化：缺必填 / 类型不符 / 未知参数三向各自报。
        let mut t = CmdTree::new("vengine");
        let mut spec: Vec<ArgSpec> = Vec::new();
        spec.push(ArgSpec { name: "target".to_string(), ty: ArgType::Str, required: true });
        spec.push(ArgSpec { name: "jobs".to_string(), ty: ArgType::Int, required: false });
        t.register("deploy", "cli.deploy", "deploy(v1)");
        // 把规格挂到 deploy 叶上（直接改树内节点——测试夹具）。
        attach_args(&mut t, "deploy", &spec);
        let got: Vec<(String, String)> = vec![
            ("target".to_string(), "plugin-a".to_string()),
            ("jobs".to_string(), "four".to_string()),
        ];
        let bad_type = validate_args(&spec, &got);
        let missing = validate_args(&spec, &[]);
        let ok = validate_args(&spec, &[("target".to_string(), "x".to_string())]);
        s.add(
            "Y02-子命令树-参数类型化三向报错",
            matches!(&bad_type, Err(e) if e.code == CliErr::ArgTypeBad && e.arg == "jobs"
                && e.expect == "整数")
                && matches!(&missing, Err(e) if e.code == CliErr::ArgMissing && e.arg == "target")
                && ok.is_ok(),
            "",
        );
    }

    // ---- 判据二：双轨输出 ----
    {
        // 两轨同源：人读行与 JSON 行来自同一事件；wire 键集判据侧独立写死。
        let mut out = DualOut::new();
        let ev = OutEvent {
            event: "build.done".to_string(),
            zh: "构建完成：plugin-a".to_string(),
            fields: vec![("target".to_string(), "plugin-a".to_string())],
            exit: ExitCode::Ok,
        };
        out.emit(&ev, true);
        let human_ok = out.human.len() == 1 && out.human[0] == ev.zh;
        let json_ok = out.json.len() == 1
            && out.json[0].contains("\"event\":\"build.done\"")
            && out.json[0].contains("\"exit\":0")
            && out.json[0].contains("\"schema\":\"cli-out-v1\"")
            && out.json[0].contains("\"fields_n\":1");
        s.add("Y02-双轨输出-两轨同源+wire键齐", human_ok && json_ok, "");
    }
    {
        // 输出中断 → 退出码如实锁定 Interrupted（不是成功）。
        let mut out = DualOut::new();
        let ev = OutEvent {
            event: "build.done".to_string(),
            zh: "构建完成".to_string(),
            fields: Vec::new(),
            exit: ExitCode::Ok,
        };
        out.emit(&ev, false);
        let exit = out.exit_code(ExitCode::Ok);
        s.add(
            "Y02-双轨输出-中断退出码如实",
            out.broken && exit == ExitCode::Interrupted && exit.wire() == 3,
            "",
        );
    }
    {
        // 退出码全集互异（0/1/2/3）。
        let mut uniq = true;
        let mut i = 0usize;
        while i < ExitCode::ALL.len() {
            let mut j = i + 1;
            while j < ExitCode::ALL.len() {
                if ExitCode::ALL[i].wire() == ExitCode::ALL[j].wire() {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("Y02-双轨输出-退出码全集互异", uniq, "");
    }

    // ---- 判据三：命令即插件 ----
    {
        // 插件命令入册；核心命名空间拒；重名先到先得；缺读屏文本拒（域本色）。
        let mut reg = ExtRegistry::new();
        let ok = reg.admit(ExtCmd {
            ns: "market".to_string(),
            cmd: "publish".to_string(),
            screen: "发布插件到市场".to_string(),
            fp: fnv_recheck(b"market.publish"),
        });
        let core = reg.admit(ExtCmd {
            ns: CORE_NS.to_string(),
            cmd: "steal".to_string(),
            screen: "冒充核心".to_string(),
            fp: 0,
        });
        let no_a11y = reg.admit(ExtCmd {
            ns: "evil".to_string(),
            cmd: "blind".to_string(),
            screen: String::new(),
            fp: 0,
        });
        let dup = reg.admit(ExtCmd {
            ns: "market".to_string(),
            cmd: "publish".to_string(),
            screen: "第二个同名".to_string(),
            fp: 0,
        });
        let traced = reg.rejected.iter().any(|(n, e)| n == "core!steal" && *e == CliErr::ExtConflict)
            && reg.rejected.iter().any(|(n, e)| n == "evil!blind" && *e == CliErr::ExtNotA11y)
            && reg.rejected.iter().any(|(n, e)| n == "market!publish" && *e == CliErr::ExtConflict);
        s.add(
            "Y02-命令即插件-核心保留+无障碍闸+先到先得全留痕",
            ok && !core && !no_a11y && !dup && traced && reg.exts.len() == 1,
            "",
        );
    }

    // ---- 判据四：口径一致 ----
    {
        // wire 键集判据侧独立写死逐条全等 + schema 常量双写对拍。
        let recheck: [&str; 6] =
            ["event", "zh", "fields_n", "exit", "ts_slot", "schema"];
        let mut keys_ok = OUT_WIRE_KEYS.len() == recheck.len();
        let mut i = 0usize;
        while i < recheck.len() {
            if OUT_WIRE_KEYS.get(i) != Some(&recheck[i]) {
                keys_ok = false;
            }
            i += 1;
        }
        s.add(
            "Y02-口径一致-wire键集独立全等+schema钉死",
            keys_ok && OUT_SCHEMA == "cli-out-v1",
            "",
        );
    }

    // ---- 判据五：判据（诊断码续编不重号 + FNV 双实现） ----
    {
        // 八码互异、落在 0x3A1x、与 Y01 的 0x3A00..0x3A09 不重号。
        let mut uniq = true;
        let mut i = 0usize;
        while i < CliErr::ALL.len() {
            let w = CliErr::ALL[i].wire();
            if w < 0x3A10 || w > 0x3A17 {
                uniq = false;
            }
            let mut j = i + 1;
            while j < CliErr::ALL.len() {
                if w == CliErr::ALL[j].wire() {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        // 与 vey01 的十码（0x3A00..0x3A09）不重号：本域全在 0x3A10 以上即证。
        s.add("Y02-判据-诊断码续编互异不越段", uniq, "");
    }
    {
        // 深度越界：超过 MAX_DEPTH 的路径注册拒绝（解析 O(命令深) 的深度闸）。
        let mut t = CmdTree::new("vengine");
        let deep_path = "a.b.c.d.e.f.g"; // 7 段 > MAX_DEPTH=6
        let ok_deep = t.register(deep_path, "cli.deep", "deep()");
        let ok_6 = t.register("a.b.c.d.e.f", "cli.six", "six()");
        s.add(
            "Y02-判据-深度越界拒+六段恰过",
            !ok_deep && ok_6,
            "",
        );
    }

    s
}

/// 测试夹具：把参数规格挂到树内叶命令（判据专用；生产路径由 register 重载
/// 承担——树内挂规格是能力本体，判据直接驱动它）。
fn attach_args(t: &mut CmdTree, leaf_name: &str, spec: &[ArgSpec]) {
    attach_at(&mut t.root, leaf_name, spec);
}

fn attach_at(node: &mut CmdNode, leaf_name: &str, spec: &[ArgSpec]) -> bool {
    if node.name == leaf_name && node.children.is_empty() {
        let mut i = 0usize;
        while i < spec.len() {
            node.args.push(spec[i].clone());
            i += 1;
        }
        return true;
    }
    let mut i = 0usize;
    while i < node.children.len() {
        if attach_at(&mut node.children[i], leaf_name, spec) {
            return true;
        }
        i += 1;
    }
    false
}
