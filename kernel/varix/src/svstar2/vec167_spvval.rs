//! VE-F0567 · SPIR-V 验证器对接
//!
//! spirv-val 生态工具集成——调用封装（版本管理、超时、崩溃隔离）、错误解析
//! （诊断文本到结构化错误到 ID 上下文回溯）、验证策略（每产物必验的硬门禁，
//! 验证未过模块禁止流出编译器）；错误回传三要素与 F0551 同构（回溯链复用到源码）；
//! 批量验证档（过夜批量排队）；诚实降级：spirv-val 不可用时进入只产出不验证态，
//! 产物带未验标记并阻断发布路径。
//!
//! 跨批对接：上游 F0561 架构、F0551 机制同构；下游 F0569 对拍、F0572 套件。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 工具版本登记（冻结策略）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ToolVersion {
    pub major: u32,
    pub minor: u32,
}

impl ToolVersion {
    pub fn compatible(&self, required: ToolVersion) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

/// 调用结果
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CallOutcome {
    Ok,
    Timeout,
    Crash,
    Unavailable,
}

/// 调用封装器：超时 + 崩溃隔离
#[derive(Clone, Debug)]
pub struct ValCaller {
    /// 登记的工具版本
    pub version: Option<ToolVersion>,
    /// 隔离重试计数
    pub retries: u32,
    /// 工具可用性
    pub available: bool,
}

impl ValCaller {
    pub fn new(available: bool, version: Option<ToolVersion>) -> Self {
        ValCaller { version, retries: 0, available }
    }

    /// 版本不匹配 → 升级流程前置判定
    pub fn version_ok(&self, required: ToolVersion) -> bool {
        match self.version {
            Some(v) => v.compatible(required),
            None => false,
        }
    }

    /// 单次调用：模拟验证（输出合法性与可用性决定结果）
    pub fn call(&mut self, valid_output: bool) -> CallOutcome {
        if !self.available {
            return CallOutcome::Unavailable;
        }
        if valid_output {
            CallOutcome::Ok
        } else {
            CallOutcome::Crash
        }
    }

    /// 崩溃隔离重试：仍败则阻断
    pub fn call_with_retry(&mut self, valid_output: bool, max_retries: u32) -> (CallOutcome, bool) {
        let mut out = self.call(valid_output);
        let mut attempts = 0;
        while out == CallOutcome::Crash && attempts < max_retries {
            self.retries += 1;
            attempts += 1;
            out = self.call(valid_output);
        }
        (out.clone(), out == CallOutcome::Ok)
    }
}

/// 结构化错误：诊断文本解析产物（三要素：位置、ID 上下文、类别）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValError {
    /// ID 上下文（出错的目标 ID）
    pub id_context: u32,
    /// 诊断正文
    pub text: String,
    /// 类别（与 F0551 三分类同构）
    pub class: ErrCat,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrCat {
    Syntax,
    Semantic,
    SpecVersion,
}

/// 诊断文本解析：`error: <id>: <text>` 格式
pub fn parse_diag(raw: &str) -> Option<ValError> {
    let rest = raw.strip_prefix("error: ")?;
    let colon = rest.find(':')?;
    let id: u32 = rest[..colon].trim().parse().ok()?;
    let text = rest[colon + 1..].trim().to_string();
    let class = if text.contains("capability") || text.contains("version") {
        ErrCat::SpecVersion
    } else if text.contains("expected") || text.contains("opcode") {
        ErrCat::Syntax
    } else {
        ErrCat::Semantic
    };
    Some(ValError { id_context: id, text, class })
}

/// 验证策略状态机
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyState {
    /// 常态：每产物必验
    Enforced,
    /// 诚实降级：工具不可用 → 只产出不验证
    Degraded,
}

/// 产物验证记录
#[derive(Clone, Debug)]
pub struct VerifyRecord {
    pub module_tag: u32,
    pub passed: bool,
    pub degraded: bool,
}

impl VerifyRecord {
    pub fn can_publish(&self) -> bool {
        self.passed && !self.degraded
    }
}

/// 硬门禁：每产物必验；降级时未验标记 + 阻断发布
pub fn gate(state: VerifyState, passed: bool) -> VerifyRecord {
    match state {
        VerifyState::Enforced => VerifyRecord { module_tag: 0, passed, degraded: false },
        VerifyState::Degraded => VerifyRecord { module_tag: 0, passed: false, degraded: true },
    }
}

/// 批量验证档：过夜批量排队
#[derive(Clone, Debug, Default)]
pub struct BatchQueue {
    pub pending: Vec<u32>,
    pub done: Vec<(u32, bool)>,
}

impl BatchQueue {
    pub fn enqueue(&mut self, tag: u32) -> bool {
        if self.pending.contains(&tag) || self.done.iter().any(|(t, _)| *t == tag) {
            return false;
        }
        self.pending.push(tag);
        true
    }

    /// 批量执行：逐项过门禁
    pub fn run(&mut self, all_valid: bool) -> usize {
        let n = self.pending.len();
        for tag in self.pending.drain(..) {
            self.done.push((tag, all_valid));
        }
        n
    }

    pub fn failures(&self) -> Vec<u32> {
        self.done.iter().filter(|(_, ok)| !ok).map(|(t, _)| *t).collect()
    }
}

pub fn run_vec167_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec167");
    // 族一：版本管理
    let v = ToolVersion { major: 2024, minor: 3 };
    s.add("版本-兼容判定", v.compatible(ToolVersion { major: 2024, minor: 1 }), "ok");
    s.add("版本-小版本低不兼容", !v.compatible(ToolVersion { major: 2024, minor: 5 }), "low");
    s.add("版本-大版本不兼容", !v.compatible(ToolVersion { major: 2023, minor: 9 }), "major");
    // 族二：调用封装
    let mut c = ValCaller::new(true, Some(v));
    s.add("调用-合法产物通过", c.call(true) == CallOutcome::Ok, "ok");
    s.add("调用-非法产物崩溃", c.call(false) == CallOutcome::Crash, "crash");
    let mut cu = ValCaller::new(false, None);
    s.add("调用-不可用", cu.call(true) == CallOutcome::Unavailable, "unavail");
    s.add("调用-无版本不兼容", !cu.version_ok(ToolVersion { major: 1, minor: 0 }), "nover");
    // 族三：崩溃隔离重试
    let mut cr = ValCaller::new(true, Some(v));
    let (out, ok) = cr.call_with_retry(false, 2);
    s.add("重试-仍败阻断", !ok && out == CallOutcome::Crash, "block");
    s.add("重试-隔离计数", cr.retries == 2, "count");
    let mut cr2 = ValCaller::new(true, Some(v));
    let (out2, ok2) = cr2.call_with_retry(true, 3);
    s.add("重试-通过零重试", ok2 && out2 == CallOutcome::Ok && cr2.retries == 0, "pass");
    // 族四：错误解析三要素
    let e = parse_diag("error: 42: id 42 is not defined");
    s.add("解析-ID上下文", e.as_ref().map(|x| x.id_context == 42).unwrap_or(false), "id");
    s.add("解析-语义分类", e.as_ref().map(|x| x.class == ErrCat::Semantic).unwrap_or(false), "sem");
    let e2 = parse_diag("error: 7: expected opcode");
    s.add("解析-语法分类", e2.as_ref().map(|x| x.class == ErrCat::Syntax).unwrap_or(false), "syn");
    let e3 = parse_diag("error: 5: capability Shader required for version");
    s.add("解析-规范版本分类", e3.as_ref().map(|x| x.class == ErrCat::SpecVersion).unwrap_or(false), "spec");
    s.add("解析-非法格式拒绝", parse_diag("nothing here").is_none(), "bad");
    // 族五：硬门禁与诚实降级
    let rec = gate(VerifyState::Enforced, true);
    s.add("门禁-验证过可发布", rec.can_publish(), "pub");
    let rec2 = gate(VerifyState::Enforced, false);
    s.add("门禁-未过禁发布", !rec2.can_publish(), "nopub");
    let rec3 = gate(VerifyState::Degraded, false);
    s.add("降级-未验阻断发布", rec3.degraded && !rec3.can_publish(), "degrade");
    // 族六：批量档
    let mut q = BatchQueue::default();
    s.add("批量-入队", q.enqueue(1) && q.enqueue(2), "enq");
    s.add("批量-重复拒绝", !q.enqueue(1), "dup");
    let n = q.run(true);
    s.add("批量-全部执行", n == 2 && q.pending.is_empty(), "run");
    s.add("批量-失败清单", q.failures().is_empty(), "pass");
    let mut q2 = BatchQueue::default();
    q2.enqueue(3);
    q2.enqueue(4);
    q2.run(false);
    s.add("批量-失败登记", q2.failures() == vec![3, 4], "fail");
    s
}
