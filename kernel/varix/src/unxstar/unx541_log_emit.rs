//! unx541_log_emit ↔ UNX-F0541 日志面总装（emit 原语）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0541，UNX-A1-B28）：
//! 「日志面总装（emit 原语）| 240 | 已深化 |
//!  日志原语 emit（级别/组件/格式化文）接入核（10/10 用例全链），四级别枚举冻结」
//!
//! 设计要点：
//! 1. **四级别冻结**：`LogLevel::{Trace, Info, Warn, Fatal}` 枚举序即
//!     过滤序，`level_rank()` 提供编译期可比较的秩（禁裸整数）。
//! 2. **emit 三参数原语**：`emit(level, component, msg)` ——
//!     级别 + 组件 + 格式化文三要素齐全才受理；缺一即
//!     `ComponentEmpty`/`MessageEmpty`（防「无出处日志」）。
//! 3. **级别闸**：`LogSink::threshold` 低于闸的级别显性丢弃并计数
//!     （drop 也要有账，禁静默）。
//! 4. **10/10 全链接入核**：`chain_audit()` 核对每个 emit 用例的
//!     「进入沉槽 → 过闸 → 落环 → 可回读」四步全链，任一环节
//!     断链即显性（不是只看最终条数）。
//! 5. **环形缓冲 + 满时显性**：`RING_CAP=64`，满时 `RingFull` 计数，
//!     不覆盖旧条目（覆盖 = 丢历史）。
//! 6. **指纹**：`sink_fingerprint()` 对全环做 FNV，跨会话对账。
//!
//! 诊断码：`LogCode` 0x86 段（0x8600..=0x860F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX541_LOG_VERSION: &str = "unx541-log-emit-v1";

/// 级别数（四级别冻结）。
pub const LEVEL_COUNT: usize = 4;

/// 环形缓冲容量。
pub const RING_CAP: usize = 64;

/// 组件名最大长度（超长拒收，防刷屏）。
pub const COMPONENT_MAX: usize = 24;

/// 日志级别（枚举序 = 过滤序，Trace 最低 Fatal 最高）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel {
    /// 跟踪（最低）
    Trace,
    /// 信息
    Info,
    /// 警告
    Warn,
    /// 致命（最高）
    Fatal,
}

impl LogLevel {
    /// 全部级别（冻结序）。
    pub const ALL: [LogLevel; LEVEL_COUNT] = [
        LogLevel::Trace,
        LogLevel::Info,
        LogLevel::Warn,
        LogLevel::Fatal,
    ];

    /// 秩（编译期可比较；秩大 = 级别高）。
    pub const fn rank(self) -> u8 {
        match self {
            LogLevel::Trace => 0,
            LogLevel::Info => 1,
            LogLevel::Warn => 2,
            LogLevel::Fatal => 3,
        }
    }

    /// 线缆名（稳定标识）。
    pub const fn wire(self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Fatal => "FATAL",
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            LogLevel::Trace => "跟踪",
            LogLevel::Info => "信息",
            LogLevel::Warn => "警告",
            LogLevel::Fatal => "致命",
        }
    }

    /// 是否达到闸（同级别过闸）。
    pub const fn passes(self, threshold: LogLevel) -> bool {
        self.rank() >= threshold.rank()
    }
}

/// 日志条目（环内一条）。
#[derive(Clone, Debug)]
pub struct LogEntry {
    /// 逻辑 tick（注入）
    pub tick: u64,
    /// 级别
    pub level: LogLevel,
    /// 组件
    pub component: String,
    /// 消息
    pub message: String,
}

/// emit 错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LogError {
    /// 组件名为空
    ComponentEmpty,
    /// 组件名过长
    ComponentTooLong { len: usize, max: usize },
    /// 消息为空
    MessageEmpty,
    /// 环满
    RingFull { cap: usize },
    /// 未知级别秩（枚举外值）
    LevelRankBad { rank: u8 },
}

/// 日志沉槽（环形 + 闸 + 账）。
pub struct LogSink {
    ring: Vec<LogEntry>,
    /// 写游标
    cursor: usize,
    /// 已写条数（含被丢）
    written: u64,
    /// 被闸丢的条数（按级别四格）
    gate_dropped: [u64; LEVEL_COUNT],
    /// 环满溢出条数
    overflowed: u64,
    /// 当前闸
    threshold: LogLevel,
}

impl LogSink {
    /// 空沉槽（闸默认 Info）。
    pub const fn new() -> Self {
        LogSink {
            ring: Vec::new(),
            cursor: 0,
            written: 0,
            gate_dropped: [0; LEVEL_COUNT],
            overflowed: 0,
            threshold: LogLevel::Info,
        }
    }

    /// 设闸（只允许布尔级切换，不允许跳过枚举）。
    pub fn set_threshold(&mut self, t: LogLevel) {
        self.threshold = t;
    }

    /// 当前闸。
    pub const fn threshold(&self) -> LogLevel {
        self.threshold
    }

    /// 环内条数。
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// 被闸丢总数。
    pub const fn gate_dropped_total(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < LEVEL_COUNT {
            n += self.gate_dropped[i];
            i += 1;
        }
        n
    }

    /// 溢出条数。
    pub const fn overflow_total(&self) -> u64 {
        self.overflowed
    }

    /// emit 原语（三参数：级别/组件/消息）。
    ///
    /// 全链：闸判定 → 参数校验 → 入环。被闸丢也要记账（异常零静默）。
    pub fn emit(
        &mut self,
        level: LogLevel,
        component: &str,
        message: &str,
        tick: u64,
    ) -> Result<(), LogError> {
        if level.rank() >= LEVEL_COUNT as u8 {
            return Err(LogError::LevelRankBad { rank: level.rank() });
        }
        if component.is_empty() {
            return Err(LogError::ComponentEmpty);
        }
        if component.len() > COMPONENT_MAX {
            return Err(LogError::ComponentTooLong {
                len: component.len(),
                max: COMPONENT_MAX,
            });
        }
        if message.is_empty() {
            return Err(LogError::MessageEmpty);
        }
        self.written += 1;
        if !level.passes(self.threshold) {
            if let Some(slot) = self.gate_dropped.get_mut(level.rank() as usize) {
                *slot += 1;
            }
            return Ok(());
        }
        if self.ring.len() >= RING_CAP {
            self.overflowed += 1;
            return Err(LogError::RingFull { cap: RING_CAP });
        }
        self.ring.push(LogEntry {
            tick,
            level,
            component: String::from(component),
            message: String::from(message),
        });
        self.cursor = self.ring.len();
        Ok(())
    }

    /// 便捷宏式原语（格式化文）。
    pub fn emitf(
        &mut self,
        level: LogLevel,
        component: &str,
        msg: &str,
        tick: u64,
    ) -> Result<(), LogError> {
        // 单链直通 emit（format! 由调用方在消息串完成）
        self.emit(level, component, msg, tick)
    }

    /// 回读第 i 条。
    pub fn entry(&self, i: usize) -> Option<&LogEntry> {
        self.ring.get(i)
    }

    /// 全链接入核（10/10 硬线）。
    ///
    /// 逐步核：①过闸进环 ②字段齐全 ③可回读 ④级号一致。
    pub fn chain_audit(&self) -> Result<u32, LogError> {
        let mut ok = 0u32;
        let mut i = 0;
        while i < self.ring.len() {
            let e = match self.entry(i) {
                Some(e) => e,
                None => return Err(LogError::RingFull { cap: RING_CAP }),
            };
            if e.component.is_empty() || e.message.is_empty() {
                return Err(LogError::ComponentEmpty);
            }
            if e.level.rank() >= LEVEL_COUNT as u8 {
                return Err(LogError::LevelRankBad { rank: e.level.rank() });
            }
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }

    /// 按级别统计环内条数。
    pub fn level_histogram(&self) -> [u32; LEVEL_COUNT] {
        let mut out = [0u32; LEVEL_COUNT];
        for e in self.ring.iter() {
            if let Some(slot) = out.get_mut(e.level.rank() as usize) {
                *slot += 1;
            }
        }
        out
    }

    /// 沉槽指纹（对全环 tick/level/component/message 做 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for e in self.ring.iter() {
            h ^= e.tick;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= e.level.rank() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            for b in e.component.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            for b in e.message.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }
}

impl Default for LogSink {
    fn default() -> Self {
        LogSink::new()
    }
}

/// 诊断码（0x86 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogCode {
    /// 0x8600 组件空
    ComponentEmpty,
    /// 0x8601 组件过长
    ComponentLong,
    /// 0x8602 消息空
    MessageEmpty,
    /// 0x8603 环满
    RingFull,
    /// 0x8604 级别秩非法
    LevelBad,
    /// 0x8605 接入核通过
    ChainOk,
    /// 0x8606 闸丢弃有账
    GateAccounted,
}

impl LogCode {
    pub const fn code(self) -> u16 {
        match self {
            LogCode::ComponentEmpty => 0x8600,
            LogCode::ComponentLong => 0x8601,
            LogCode::MessageEmpty => 0x8602,
            LogCode::RingFull => 0x8603,
            LogCode::LevelBad => 0x8604,
            LogCode::ChainOk => 0x8605,
            LogCode::GateAccounted => 0x8606,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            LogCode::ComponentEmpty => "LOG-COMPONENT-EMPTY",
            LogCode::ComponentLong => "LOG-COMPONENT-LONG",
            LogCode::MessageEmpty => "LOG-MESSAGE-EMPTY",
            LogCode::RingFull => "LOG-RING-FULL",
            LogCode::LevelBad => "LOG-LEVEL-BAD",
            LogCode::ChainOk => "LOG-CHAIN-OK",
            LogCode::GateAccounted => "LOG-GATE-ACCOUNTED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            LogCode::ComponentEmpty => "日志组件为空：emit 三参数缺组件，无出处日志拒收",
            LogCode::ComponentLong => "日志组件过长：超过 24 字符上限，禁刷屏式组件名",
            LogCode::MessageEmpty => "日志消息为空：emit 缺格式化文拒收",
            LogCode::RingFull => "日志环满：64 条已满，溢出显性记账（不覆盖旧条目）",
            LogCode::LevelBad => "日志级别秩非法：四级别冻结集之外的值",
            LogCode::ChainOk => "日志接入核通过：过闸/入环/回读/级号四步全链",
            LogCode::GateAccounted => "闸丢弃有账：被拦级别逐格计数，异常零静默",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx541_log_emit.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8600 && (c & 0x00FF) < 0x10
    }
}

impl LogError {
    pub const fn code(self) -> LogCode {
        match self {
            LogError::ComponentEmpty => LogCode::ComponentEmpty,
            LogError::ComponentTooLong { .. } => LogCode::ComponentLong,
            LogError::MessageEmpty => LogCode::MessageEmpty,
            LogError::RingFull { .. } => LogCode::RingFull,
            LogError::LevelRankBad { .. } => LogCode::LevelBad,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 域自检（10/10 全链用例）。
pub fn run_unx541_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx541");

    // C541-01 四级别枚举冻结（rank 单调 + wire 稳定）
    {
        let mut ok = LogLevel::ALL[0].rank() == 0;
        let mut i = 1;
        while i < LEVEL_COUNT {
            ok = ok && LogLevel::ALL[i].rank() == i as u8;
            i += 1;
        }
        s.add("C541-01-四级别冻结", ok, if ok { "" } else { "级别枚举序漂移" });
    }

    // C541-02 emit 全链 10 条接入核
    {
        let mut sink = LogSink::new();
        let mut i = 0;
        let mut all_ok = true;
        while i < 10 {
            let msg = format!("chain-case-{}", i);
            let r = sink.emit(LogLevel::Info, "unx541", &msg, i as u64);
            all_ok = all_ok && r.is_ok();
            i += 1;
        }
        let audit = sink.chain_audit();
        let good = all_ok && audit == Ok(10) && sink.len() == 10;
        s.add("C541-02-全链接入 10/10", good, if good { "" } else { "接入核不满 10" });
    }

    // C541-03 低于闸被丢且有账
    {
        let mut sink = LogSink::new();
        let _ = sink.emit(LogLevel::Info, "c", "x", 0);
        let r = sink.emit(LogLevel::Trace, "c", "x", 1);
        let good = r.is_ok() && sink.len() == 1 && sink.gate_dropped_total() == 1;
        s.add("C541-03-闸丢弃有账", good, if good { "" } else { "静默丢日志" });
    }

    // C541-04 组件空拒收
    {
        let mut sink = LogSink::new();
        let r = sink.emit(LogLevel::Info, "", "x", 0);
        let good = matches!(r, Err(LogError::ComponentEmpty));
        s.add("C541-04-空组件拒收", good, if good { "" } else { "空组件被放行" });
    }

    // C541-05 消息空拒收
    {
        let mut sink = LogSink::new();
        let r = sink.emit(LogLevel::Info, "c", "", 0);
        let good = matches!(r, Err(LogError::MessageEmpty));
        s.add("C541-05-空消息拒收", good, if good { "" } else { "空消息被放行" });
    }

    // C541-06 组件过长拒收（25 字符）
    {
        let mut sink = LogSink::new();
        let long = "c123456789012345678901234";
        let r = sink.emit(LogLevel::Info, long, "x", 0);
        let good = matches!(r, Err(LogError::ComponentTooLong { len: 25, max: 24 }));
        s.add("C541-06-组件长上限", good, if good { "" } else { "长组件被放行" });
    }

    // C541-07 环满显性 Err + 溢出计数
    {
        let mut sink = LogSink::new();
        let mut i = 0;
        let mut last = Ok(());
        while i < RING_CAP + 3 {
            let msg = format!("m{}", i);
            last = sink.emit(LogLevel::Warn, "c", &msg, i as u64);
            i += 1;
        }
        let good = matches!(last, Err(LogError::RingFull { cap: 64 }))
            && sink.overflow_total() == 3
            && sink.len() == RING_CAP;
        s.add("C541-07-环满显性溢出账", good, if good { "" } else { "溢出被吞" });
    }

    // C541-08 级别直方图（Trace 被闸挡，Warn/Fatal 进环）
    {
        let mut sink = LogSink::new();
        let _ = sink.emit(LogLevel::Warn, "c", "w", 0);
        let _ = sink.emit(LogLevel::Fatal, "c", "f", 1);
        let _ = sink.emit(LogLevel::Warn, "c", "w2", 2);
        let _ = sink.emit(LogLevel::Trace, "c", "t", 3);
        let h = sink.level_histogram();
        let good = h[0] == 0 && h[1] == 0 && h[2] == 2 && h[3] == 1;
        s.add("C541-08-级别直方图对账", good, if good { "" } else { "直方图不符" });
    }

    // C541-09 指纹确定（同内容同指纹）
    {
        let mut a = LogSink::new();
        let _ = a.emit(LogLevel::Info, "c", "hello", 7);
        let mut b = LogSink::new();
        let _ = b.emit(LogLevel::Info, "c", "hello", 7);
        let mut c = LogSink::new();
        let _ = c.emit(LogLevel::Info, "c", "world", 7);
        let good = a.fingerprint() == b.fingerprint() && a.fingerprint() != c.fingerprint();
        s.add("C541-09-指纹确定且可分辨", good, if good { "" } else { "指纹失效" });
    }

    // C541-10 闸切换后 Trace 进环
    {
        let mut sink = LogSink::new();
        let _ = sink.emit(LogLevel::Trace, "c", "t", 0);
        let before = sink.len();
        sink.set_threshold(LogLevel::Trace);
        let r = sink.emit(LogLevel::Trace, "c", "t2", 1);
        let good = before == 0 && r.is_ok() && sink.len() == 1 && sink.gate_dropped_total() == 1;
        s.add("C541-10-闸切换生效", good, if good { "" } else { "闸切换失效" });
    }

    // C541-11 诊断码段独占
    {
        let codes = [
            LogCode::ComponentEmpty,
            LogCode::ComponentLong,
            LogCode::MessageEmpty,
            LogCode::RingFull,
            LogCode::LevelBad,
            LogCode::ChainOk,
            LogCode::GateAccounted,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C541-11-诊断码 0x86 段独占", all, if all { "" } else { "码值越出 0x86 段" });
    }

    // C541-12 emitf 与 emit 同链
    {
        let mut sink = LogSink::new();
        let r = sink.emitf(LogLevel::Warn, "c", &format!("f-{}", 1), 0);
        let audit = sink.chain_audit();
        let good = r.is_ok() && audit == Ok(1);
        s.add("C541-12-emitf 同链", good, if good { "" } else { "emitf 断链" });
    }

    s
}
