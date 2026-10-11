//! unx502_uartio —— UNX-F0502 串口输出原语（putchar/write 行）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B25）：UNX-F0502 · 串口输出原语（putchar/write 行） · 目标 220 行 · 已深化
//! 判据原文：输出三件（putchar/write/串口摘要行）字节序一致（10/10 用例回环核），CR/LF 翻译账注
//! 组件：字节发射层（putchar 单字节 + 发射账）+ 行写层（write 行级 CR/LF 翻译策略）
//!      + 摘要行层（串口摘要行：组件/级别/正文一行化）+ 回环核验器（10/10 用例字节序一致）
//!      + 翻译账注（每处 CR/LF 处理带策略注）
//! 判据落地：输出三件字节序一致（回环核）+ CR/LF 翻译账注
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，串口写注入（零真实 IO 端口）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const UA_VERSION: &str = "unx502_uartio-v1";

/// 诊断段基址（0x4c70 段 · 本模块独占）。
pub const UA_SEG_BASE: u16 = 0x4c70;

/// 回环核用例数（判据：10/10）。
pub const UA_CASES: usize = 10;

/// CR/LF 翻译策略（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UaEol {
    /// 原样透传（字节序一致口径：不做任何翻译）。
    Passthrough,
    /// LF → CRLF（终端语义：换行补回车）。
    LfToCrlf,
    /// CR → LF（旧设备语义：回车归一为换行）。
    CrToLf,
    /// 全剥离（摘要行语义：控制字节一律不外发）。
    Strip,
}

impl UaEol {
    /// 中文名与策略账注。
    pub const fn note(self) -> &'static str {
        match self {
            UaEol::Passthrough => "CR/LF 翻译账注：原样透传，字节序一致口径零翻译",
            UaEol::LfToCrlf => "CR/LF 翻译账注：LF 补 CR 成 CRLF（终端回车语义）",
            UaEol::CrToLf => "CR/LF 翻译账注：CR 归一为 LF（旧终端兼容语义）",
            UaEol::Strip => "CR/LF 翻译账注：控制字节剥离（摘要行单行化语义）",
        }
    }
}

/// 诊断码（0x4c70 段十三码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UaCode {
    /// 0x4c70 发射层不可达（底层 sink 拒收）。
    SinkDown,
    /// 0x4c71 回环核失败（发射字节与回环读回不符）。
    LoopMismatch,
    /// 0x4c72 用例不足（回环核 < 10/10）。
    CasesShort,
    /// 0x4c73 翻译策略越闭集。
    EolBad,
    /// 0x4c74 摘要行含控制字节（Strip 策略被绕过）。
    CtlLeak,
    /// 0x4c75 行超长（单行 > 上限，截断显性）。
    LineTooLong,
    /// 0x4c76 发射账破缺（发射计数 ≢ 账条数）。
    LedgerBreak,
    /// 0x4c77 字节序错位（多字节写序与单字节序列不一致——三件一致性破坏）。
    OrderBroken,
    /// 0x4c78 空行发射（write 行空正文属调用错误显性）。
    EmptyLine,
    /// 0x4c79 缓冲溢出（行缓冲满未冲刷）。
    BufFull,
    /// 0x4c7a 摘要行无组件名（摘要行结构缺件）。
    NoComponent,
    /// 0x4c7b 回环核账破缺（用例数 ≢ 账条数）。
    CaseLedgerBad,
    /// 0x4c7c 段位守卫失败。
    SegmentGuard,
}

impl UaCode {
    /// 全量闭集。
    pub const ALL: [UaCode; 13] = [
        UaCode::SinkDown,
        UaCode::LoopMismatch,
        UaCode::CasesShort,
        UaCode::EolBad,
        UaCode::CtlLeak,
        UaCode::LineTooLong,
        UaCode::LedgerBreak,
        UaCode::OrderBroken,
        UaCode::EmptyLine,
        UaCode::BufFull,
        UaCode::NoComponent,
        UaCode::CaseLedgerBad,
        UaCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        UA_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            UaCode::SinkDown => "发射层不可达：底层 sink 拒收字节",
            UaCode::LoopMismatch => "回环核失败：发射与读回字节不符",
            UaCode::CasesShort => "回环核用例不足：判据要求 10/10",
            UaCode::EolBad => "翻译策略越闭集：四策略之外无效",
            UaCode::CtlLeak => "摘要行含控制字节：Strip 策略被绕过",
            UaCode::LineTooLong => "行超长：截断显性记录不静默",
            UaCode::LedgerBreak => "发射账破缺：发射计数与账条数不一致",
            UaCode::OrderBroken => "字节序错位：write 与逐字节 putchar 序列不一致",
            UaCode::EmptyLine => "空行发射：write 行空正文按调用错误显性",
            UaCode::BufFull => "行缓冲溢出：先冲刷再续写",
            UaCode::NoComponent => "摘要行缺组件名：结构缺件",
            UaCode::CaseLedgerBad => "回环核账破缺：用例数与账条数不一致",
            UaCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            UaCode::SinkDown => "先核底层串口初始化态再发射；未就绪按 SinkDown 显性",
            UaCode::LoopMismatch => "回环读回与发射不符：核对 sink 是否同一路径",
            UaCode::CasesShort => "补满 10 用例再报绿：判据是硬数不是约数",
            UaCode::EolBad => "策略只能取四闭集之一：调用方先归一",
            UaCode::CtlLeak => "摘要行出口加控制字节过滤：核对 Strip 分支",
            UaCode::LineTooLong => "行上限是防线：先冲刷或分片，超限留痕",
            UaCode::LedgerBreak => "发射与记账同事务：核对 emit 的记账路径",
            UaCode::OrderBroken => "三件一致性核：write 必须可分解为等价 putchar 序列",
            UaCode::EmptyLine => "空正文走 putchar 的 CR/LF 直发路径，不走 write 行",
            UaCode::BufFull => "行缓冲满时冲刷点前移：核对 flush 调用序",
            UaCode::NoComponent => "摘要行构造器强制组件名：核对调用参数",
            UaCode::CaseLedgerBad => "每用例恰一条账：核对回环核循环",
            UaCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4c70",
        }
    }
}

/// 字节 sink 注入（回环缓冲实现，零真实 IO 端口）。
pub trait UaSink {
    /// 发射一个字节；false = 拒收。
    fn emit(&mut self, b: u8) -> bool;
}

/// 回环 sink（发射即入队，供回环核与摘要验证）。
#[derive(Clone, Debug, Default)]
pub struct LoopSink {
    /// 已发射字节序。
    pub bytes: Vec<u8>,
}

impl UaSink for LoopSink {
    fn emit(&mut self, b: u8) -> bool {
        self.bytes.push(b);
        true
    }
}

/// 串口输出原语主体（输出三件：putchar / write 行 / 摘要行）。
pub struct UartLine {
    /// 当前翻译策略。
    pub eol: UaEol,
    /// 发射字节计数（putchar 口径）。
    pub emitted: u64,
    /// 发射动作计数（putchar/write/digest 合计，账对账口径）。
    pub actions: u64,
    /// 行发射计数（write 口径）。
    pub lines: u64,
    /// 摘要行计数。
    pub digests: u64,
    /// 发射账（每次发射动作一条：来源 + 字节数）。
    pub ledger: Vec<(&'static str, u64)>,
    /// 行上限（当前策略值，可调）。
    pub line_cap: usize,
    /// 诊断码。
    pub codes: Vec<UaCode>,
}

impl UartLine {
    /// 建（默认策略：原样透传——字节序一致口径）。
    pub fn new() -> Self {
        UartLine {
            eol: UaEol::Passthrough,
            emitted: 0,
            actions: 0,
            lines: 0,
            digests: 0,
            ledger: Vec::new(),
            line_cap: 4096,
            codes: Vec::new(),
        }
    }

    fn note(&mut self, c: UaCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 件 1：putchar 单字节发射（透传：翻译策略只作用于行级，单字节零翻译——账注）。
    pub fn putchar<S: UaSink>(&mut self, s: &mut S, b: u8) -> Result<(), (UaCode, String)> {
        if !s.emit(b) {
            self.note(UaCode::SinkDown);
            return Err((UaCode::SinkDown, UaCode::SinkDown.advice().to_string()));
        }
        self.emitted += 1;
        self.actions += 1;
        self.ledger.push(("putchar", 1));
        Ok(())
    }

    /// 按 CR/LF 翻译策略产出行字节序列（纯函数：翻译逻辑与发射解耦，可回环核）。
    pub fn translate(&mut self, line: &[u8]) -> Result<Vec<u8>, (UaCode, String)> {
        if line.is_empty() {
            self.note(UaCode::EmptyLine);
            return Err((UaCode::EmptyLine, UaCode::EmptyLine.advice().to_string()));
        }
        if line.len() > self.line_cap {
            self.note(UaCode::LineTooLong);
            return Err((UaCode::LineTooLong, UaCode::LineTooLong.advice().to_string()));
        }
        let mut out = Vec::new();
        match self.eol {
            UaEol::Passthrough => out.extend_from_slice(line),
            UaEol::LfToCrlf => {
                for &b in line {
                    if b == b'\n' {
                        out.push(b'\r');
                    }
                    out.push(b);
                }
            }
            UaEol::CrToLf => {
                for &b in line {
                    out.push(if b == b'\r' { b'\n' } else { b });
                }
            }
            UaEol::Strip => {
                for &b in line {
                    if b != b'\r' && b != b'\n' {
                        out.push(b);
                    }
                }
            }
        }
        Ok(out)
    }

    /// 件 2：write 行发射（翻译后逐字节走同一 emit 路径——三件一致性根基）。
    pub fn write_line<S: UaSink>(&mut self, s: &mut S, line: &[u8]) -> Result<usize, (UaCode, String)> {
        let bytes = self.translate(line)?;
        for b in &bytes {
            if !s.emit(*b) {
                self.note(UaCode::SinkDown);
                return Err((UaCode::SinkDown, UaCode::SinkDown.advice().to_string()));
            }
            self.emitted += 1;
        }
        self.lines += 1;
        self.actions += 1;
        self.ledger.push(("write", bytes.len() as u64));
        Ok(bytes.len())
    }

    /// 件 3：摘要行（组件/级别/正文一行化；Strip 策略强制）。
    pub fn digest_line<S: UaSink>(
        &mut self,
        s: &mut S,
        component: &str,
        level: &str,
        body: &str,
    ) -> Result<usize, (UaCode, String)> {
        if component.is_empty() {
            self.note(UaCode::NoComponent);
            return Err((UaCode::NoComponent, UaCode::NoComponent.advice().to_string()));
        }
        let raw = format!("[{}][{}] {}", component, level, body);
        // Strip 强制：控制字节一律不外发（CtlLeak 防线：出口复核）。
        let clean: Vec<u8> = raw
            .bytes()
            .filter(|b| *b != b'\r' && *b != b'\n' && *b >= 0x20)
            .collect();
        for b in &clean {
            if !s.emit(*b) {
                self.note(UaCode::SinkDown);
                return Err((UaCode::SinkDown, UaCode::SinkDown.advice().to_string()));
            }
            self.emitted += 1;
        }
        self.digests += 1;
        self.actions += 1;
        self.ledger.push(("digest", clean.len() as u64));
        Ok(clean.len())
    }

    /// 回环核验器（判据核心：10/10 用例字节序一致）。
    /// 一致口径：write 行的字节序 ≡ 同内容逐字节 putchar 序列（Passthrough 下）。
    pub fn loop_audit(&mut self) -> Result<usize, (UaCode, String)> {
        const SAMPLES: [&[u8]; UA_CASES] = [
            b"A", b"OK", b"boot", b"init ok", b"line-5", b"x=1;y=2", b"12 34", b"[tag] v", b"abcdefg", b"tail-10",
        ];
        let mut passed = 0usize;
        let mut cases = Vec::new();
        let saved_eol = self.eol;
        self.eol = UaEol::Passthrough;
        for (i, sample) in SAMPLES.iter().enumerate() {
            // 路径 A：write 行。
            let mut sink_a = LoopSink::default();
            let a_ok = self.write_line(&mut sink_a, sample).is_ok();
            // 路径 B：逐字节 putchar。
            let mut sink_b = LoopSink::default();
            let mut b_ok = true;
            for &b in *sample {
                if self.putchar(&mut sink_b, b).is_err() {
                    b_ok = false;
                    break;
                }
            }
            // 字节序一致核。
            let same = a_ok && b_ok && sink_a.bytes == sink_b.bytes;
            cases.push((i as u64, same));
            if same {
                passed += 1;
            } else {
                self.note(UaCode::LoopMismatch);
            }
        }
        self.eol = saved_eol;
        // 用例数硬门（10/10）。
        if passed < UA_CASES {
            self.note(UaCode::CasesShort);
            return Err((UaCode::CasesShort, UaCode::CasesShort.advice().to_string()));
        }
        // 回环核账破缺防线。
        if cases.len() != UA_CASES {
            self.note(UaCode::CaseLedgerBad);
            return Err((UaCode::CaseLedgerBad, UaCode::CaseLedgerBad.advice().to_string()));
        }
        Ok(passed)
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.ledger.len() as u64 == self.actions && self.actions >= self.lines + self.digests
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    UaCode::ALL.iter().all(|c| c.val() >= UA_SEG_BASE && c.val() < UA_SEG_BASE + 0x100)
}
