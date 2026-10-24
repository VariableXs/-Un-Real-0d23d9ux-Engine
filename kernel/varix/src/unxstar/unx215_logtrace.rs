//! UNX-F0215 · 日志-报告联动（事件溯源）
//!
//! 锚点判据（施工书原文）：UNX-F0215-J1 报告异常项可溯源至日志
//! 行号，追溯成功率 100%。
//!
//! 设计：日志簿（行号单调）+ 报告异常项（携带事件键）→ 溯源器
//! （事件键 → 日志行号反查）→ 红线：①每个异常项必须溯源成功
//! （漏一条即红）；②行号引用必须真实存在（悬空引用拒）；
//! ③日志追加后历史行号不变（追加式，永不重写）。
//!
//! 错误码段：0x2C70（全仓普查后独占分配，AI-WBX1022）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const F0215_VERSION: &str = "F0215-logtrace-v1";

/// 诊断码段基址（UNX-F0215 独占）。
pub const SEG_BASE: u16 = 0x2C70;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 溯源失败（事件键无日志行）。
    TraceMiss = SEG_BASE + 1,
    /// 悬空行号引用。
    DanglingLine = SEG_BASE + 2,
    /// 日志行号倒挂。
    LineRegress = SEG_BASE + 3,
    /// 异常项缺事件键。
    NoEventKey = SEG_BASE + 4,
    /// 空日志溯源。
    EmptyLog = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 日志行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// 行号（从 1 单调）。
    pub no: u64,
    /// 事件键（可空——普通行无键）。
    pub event_key: Option<String>,
    /// 行文本。
    pub text: String,
}

/// 追加式日志簿。
#[derive(Debug, Default)]
pub struct LogBook {
    /// 行集。
    pub lines: Vec<LogLine>,
    next_no: u64,
}

impl LogBook {
    /// 追加一行（普通）。
    pub fn append(&mut self, text: &str) -> u64 {
        self.next_no += 1;
        let no = self.next_no;
        self.lines.push(LogLine {
            no,
            event_key: None,
            text: String::from(text),
        });
        no
    }

    /// 追加带事件键行。
    pub fn append_keyed(&mut self, text: &str, key: &str) -> u64 {
        self.next_no += 1;
        let no = self.next_no;
        self.lines.push(LogLine {
            no,
            event_key: Some(String::from(key)),
            text: String::from(text),
        });
        no
    }

    /// 追加式红线：历史行不被重写（长度只增、前缀稳定）。
    pub fn append_only_ok(&self, snapshot: &[LogLine]) -> bool {
        snapshot.len() <= self.lines.len()
            && snapshot
                .iter()
                .zip(self.lines.iter())
                .all(|(a, b)| a == b)
    }

    /// 按事件键反查行号。
    pub fn trace(&self, key: &str) -> Result<u64, u16> {
        if self.lines.is_empty() {
            return Err(E::EmptyLog.code());
        }
        self.lines
            .iter()
            .find(|l| l.event_key.as_deref() == Some(key))
            .map(|l| l.no)
            .ok_or(E::TraceMiss.code())
    }
}

/// 报告异常项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anomaly {
    /// 事件键（溯源锚）。
    pub event_key: String,
    /// 摘要。
    pub summary: String,
}

/// 溯源器。
#[derive(Debug, Default)]
pub struct Tracer {
    /// 异常项。
    pub anomalies: Vec<Anomaly>,
}

impl Tracer {
    /// 溯源全部异常项：每个都必须找到日志行号。
    pub fn trace_all(&self, log: &LogBook) -> Result<Vec<(String, u64)>, u16> {
        if self.anomalies.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for a in self.anomalies.iter() {
            if a.event_key.is_empty() {
                return Err(E::NoEventKey.code());
            }
            let no = log.trace(&a.event_key)?;
            // 悬空引用红线：行号必须真实在簿。
            if !log.lines.iter().any(|l| l.no == no) {
                return Err(E::DanglingLine.code());
            }
            out.push((a.event_key.clone(), no));
        }
        Ok(out)
    }
}

/// J1 自证（判据原文：异常项可溯源至日志行号，追溯成功率 100%）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：日志簿 6 行（2 行带键）+ 2 个异常项。
    let mut log = LogBook::default();
    log.append("fw-handoff ok");
    log.append_keyed("disk-read retry x3", "evt-disk-retry");
    log.append("paging built");
    log.append("heap 64MiB");
    log.append_keyed("gfx-mode degraded to text", "evt-gfx-degrade");
    log.append("user handoff");

    let mut t = Tracer::default();
    t.anomalies.push(Anomaly {
        event_key: String::from("evt-disk-retry"),
        summary: String::from("重试链"),
    });
    t.anomalies.push(Anomaly {
        event_key: String::from("evt-gfx-degrade"),
        summary: String::from("显示降级"),
    });

    // 二、J1 主体：全部溯源成功 100%，行号指向真实行。
    let traced = t.trace_all(&log)?;
    if traced.len() != 2 {
        return Err(E::TraceMiss.code());
    }
    if traced[0].1 != 2 || traced[1].1 != 5 {
        return Err(E::TraceMiss.code());
    }
    for (_, no) in traced.iter() {
        if !log.lines.iter().any(|l| l.no == *no) {
            return Err(E::DanglingLine.code());
        }
    }

    // 三、追加式红线：快照后追加，历史不变。
    let snapshot: Vec<LogLine> = log.lines.clone();
    let new_no = log.append("post-snapshot line");
    if new_no != 7 {
        return Err(E::LineRegress.code());
    }
    if !log.append_only_ok(&snapshot) {
        return Err(E::LineRegress.code());
    }
    // 追加后原溯源仍有效。
    if log.trace("evt-disk-retry")? != 2 {
        return Err(E::TraceMiss.code());
    }

    // 四、红线逐项：未知键溯源失败 / 空日志 / 缺键异常项。
    if log.trace("evt-ghost").is_ok() {
        return Err(E::TraceMiss.code());
    }
    if LogBook::default().trace("x").is_ok() {
        return Err(E::EmptyLog.code());
    }
    let mut bad = Tracer::default();
    bad.anomalies.push(Anomaly {
        event_key: String::new(),
        summary: String::from("无键"),
    });
    if bad.trace_all(&log).is_ok() {
        return Err(E::NoEventKey.code());
    }

    // 五、判别式段位对拍。
    if E::TraceMiss.code() != SEG_BASE + 1 || E::EmptyLog.code() != SEG_BASE + 5 {
        return Err(E::TraceMiss.code());
    }
    Ok(())
}
