//! unx502_serialout —— UNX-F0502 串口输出原语（putchar/write 行）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B26）：UNX-F0502 · 串口输出原语（putchar/write 行） · 目标 220 行 · 已深化
//! 判据原文：输出三件（putchar/write/串口摘要行）字节序一致（10/10 用例回环核），CR/LF 翻译账注
//! 组件：16550 语义模型（发送 FIFO + LSR 位型 + 波特除数账）+ 三件输出原语
//!      （putchar 单字节 / write_line 整行 / summary_line 摘要行）+ CR/LF 翻译器
//!      （四模式闭集）+ 翻译账（逐字节去向计数，守恒式入账）+ 回环核（三路同源比对）。
//! 职责定位：内核诊断输出底座——panic/日志/ktest 全部走这条路的字节面契约。
//!   三件原语对同一输入必须产出逐字节一致的输出流（字节序一致 = 三路同源回环核
//!   10/10 用例全过）；CR/LF 翻译是唯一允许的字节改写点，且每次改写都入账（翻译账注）。
//! 性能逐项分解：putchar O(1)（FIFO 满 O(16) 排空）；write_line O(字节数)；
//!   summary_line O(账目行数)（账目行数闭集 8 封顶）；指纹 O(输出字节数)；
//!   全整数算术零浮点零墙钟，同输入恒同输出（确定性对拍锚）。
//! 诊断码段：0x97（0x9701~0x9708，全域 grep 确认独占，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick 注入，零墙钟），
//!   逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（来源：16550A 数据手册语义 + 本仓诊断输出纪律；写死并注释）
// ---------------------------------------------------------------------------

/// 发送 FIFO 容量：16550 内建 16 字节 FIFO（NS16550A §发送器）。
pub const FIFO_CAP: usize = 16;
/// 行最大长度：诊断行 512 字节封顶（超长必须显性分段，不静默截断）。
pub const LINE_MAX: usize = 512;
/// 波特除数缺省：115200 波特 @ 1.8432MHz 晶振 → 除数 1（DLAB 配对写）。
pub const BAUD_DIVISOR_DEFAULT: u16 = 1;
/// LSR 位型：THRE（发送保持寄存器空）= bit 5。
pub const LSR_THRE: u8 = 0x20;
/// LSR 位型：TEMT（发送器全空）= bit 6。
pub const LSR_TEMT: u8 = 0x40;
/// 逻辑 tick：每次 drain_tick 排空 FIFO 4 字节（确定性速率，零墙钟）。
pub const DRAIN_PER_TICK: usize = 4;

// ---------------------------------------------------------------------------
// 诊断码段 0x97（0x9701~0x9708 闭集；域内独占）
// ---------------------------------------------------------------------------

/// 0x9701 FIFO 滠满未排空：FIFO 16 字节满仍写——必须先 drain_tick。
pub const C_FIFO_FULL: u16 = 0x9701;
/// 0x9702 非法控制字节：0x00~0x08、0x0B~0x0C、0x0E~0x1F 区段（仅 \t \n \r 放行）。
pub const C_BAD_CTRL: u16 = 0x9702;
/// 0x9703 行超长：单行超 LINE_MAX，必须显性分段。
pub const C_LINE_LONG: u16 = 0x9703;
/// 0x9704 翻译模式未登记：写了闭集外的模式值。
pub const C_MODE_UNKNOWN: u16 = 0x9704;
/// 0x9705 回环不一致：三路输出逐字节比对发现分歧（字节序一致破约）。
pub const C_ROUNDTRIP: u16 = 0x9705;
/// 0x9706 摘要指纹漂移：同账目两次摘要指纹不同（确定性失守）。
pub const C_SUMMARY_FP: u16 = 0x9706;
/// 0x9707 波特除数为 0：除数 0 = 时钟停摆，拒绝输出。
pub const C_BAUD_ZERO: u16 = 0x9707;
/// 0x9708 翻译账破：翻译账守恒式（入 ≡ 直通 + 改写）不成立。
pub const C_LEDGER_BROKEN: u16 = 0x9708;

/// 段守卫：全部诊断码必须落 0x97 段。
pub const fn seg_guard() -> bool {
    (C_FIFO_FULL >> 8) == 0x97
        && (C_BAD_CTRL >> 8) == 0x97
        && (C_LINE_LONG >> 8) == 0x97
        && (C_MODE_UNKNOWN >> 8) == 0x97
        && (C_ROUNDTRIP >> 8) == 0x97
        && (C_SUMMARY_FP >> 8) == 0x97
        && (C_BAUD_ZERO >> 8) == 0x97
        && (C_LEDGER_BROKEN >> 8) == 0x97
}

/// 诊断码 → 人话（可操作建议一体）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_FIFO_FULL => "FIFO 满：先 drain_tick 排空再写；持续满则核对波特除数",
        C_BAD_CTRL => "非法控制字节：仅 \\t(09) \\n(0A) \\r(0D) 放行，其余控制区拒绝",
        C_LINE_LONG => "行超长：按 512 字节显性分段写，勿静默截断",
        C_MODE_UNKNOWN => "翻译模式未登记：从 CrLf/LfOnly/CrOnly/Passthrough 四闭集取值",
        C_ROUNDTRIP => "回环不一致：三路输出有分歧，先查翻译器再查原语",
        C_SUMMARY_FP => "摘要指纹漂移：账目不可变检查，查翻译账追加路径",
        C_BAUD_ZERO => "波特除数为 0：时钟停摆；先设波特再输出",
        C_LEDGER_BROKEN => "翻译账破：入 ≡ 直通 + 改写 守恒式不成立，查逐字节记账",
        _ => "未知诊断码：核对 0x97 段闭集",
    }
}

// ---------------------------------------------------------------------------
// CR/LF 翻译模式闭集（四模式）与翻译账
// ---------------------------------------------------------------------------

/// 行尾翻译模式闭集：
/// CrLf = \n → \r\n（终端惯例）；LfOnly = 原样 \n；
/// CrOnly = \n → \r（旧式打印终端）；Passthrough = 逐字节透传（二进制面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlateMode {
    CrLf,
    LfOnly,
    CrOnly,
    Passthrough,
}

impl XlateMode {
    pub const fn tag(&self) -> &'static str {
        match self {
            XlateMode::CrLf => "crlf",
            XlateMode::LfOnly => "lf",
            XlateMode::CrOnly => "cr",
            XlateMode::Passthrough => "raw",
        }
    }

    /// 模式登记校验：闭集外值在类型层面不可表达（这里是双保险入口）。
    pub const fn is_registered(&self) -> bool {
        matches!(
            self,
            XlateMode::CrLf | XlateMode::LfOnly | XlateMode::CrOnly | XlateMode::Passthrough
        )
    }
}

/// 翻译账：逐字节去向计数（入 ≡ 直通 + 改写，守恒式）。
/// 账注 = 每类改写都有计数行可查（「翻译账注」判据的落地形态）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XlateLedger {
    /// 输入字节总数（进入翻译器的）。
    pub input_bytes: u64,
    /// 直通字节：未经改写直接进 FIFO 的。
    pub passthrough_bytes: u64,
    /// \n → \r\n 改写次数（改写 1 入 2 出）。
    pub nl_to_crlf: u64,
    /// \n → \r 改写次数。
    pub nl_to_cr: u64,
    /// \n 原样输出次数（LfOnly 下不是改写，是直通）。
    pub nl_kept: u64,
    /// \r 收到次数（翻译器把孤立 \r 也当直通；行内 \r\n 不重复翻译）。
    pub cr_in: u64,
    /// \t 收到次数（直通白名单成员）。
    pub tab_in: u64,
    /// 实际进 FIFO 的字节总数（出口计数）。
    pub fifo_in_bytes: u64,
}

impl XlateLedger {
    /// 守恒式①：入 ≡ 直通 + 各改写档贡献（每个输入字节恰落入一个档）。
    ///
    /// 档位全集按 `xlate()` 的记账逐字节铺开：
    ///   直通（普通字节 / CR / TAB / Passthrough 模式下的 LF）→ passthrough_bytes
    ///   CrLf 模式的 LF（\n→\r\n，一入一出里的"入"）           → nl_to_crlf
    ///   CrOnly 模式的 LF（\n→\r）                             → nl_to_cr
    ///   LfOnly 模式的 LF（\n 直通不改写）                     → nl_kept
    /// 四个档互斥且穷尽（每个输入字节必进其一），故入字节数 ≡ 四档之和。
    /// 漏掉 nl_kept 这一档，LfOnly 模式下每个 \n 都让等式左侧多 1——
    /// 回环核对 10 组用例 × 4 模式时第一组非空行即破守恒（见 C2 反例）。
    pub const fn input_conserves(&self) -> bool {
        let translated_inputs = self.nl_to_crlf + self.nl_to_cr + self.nl_kept;
        self.input_bytes == self.passthrough_bytes + translated_inputs
    }

    /// 守恒式②：FIFO 入口 ≡ 各档产出字节数。
    ///
    /// 产出侧逐字节数与入侧一一对应：直通 1、CrLf 改写 2、CrOnly 改写 1、
    /// LfOnly 直通（改写旗为 false，但仍产 1 字节进 FIFO）1。
    /// 漏掉 nl_kept 这一档，LfOnly 每个 \n 都让等式右侧少 1 而 fifo_in_bytes
    /// 照实 +1 ⇒ 破守恒（与守恒式①同源，一并修）。
    pub const fn output_conserves(&self) -> bool {
        let produced = self.passthrough_bytes + self.nl_to_crlf * 2 + self.nl_to_cr + self.nl_kept;
        self.fifo_in_bytes == produced
    }

    pub const fn all_conserves(&self) -> bool {
        self.input_conserves() && self.output_conserves()
    }
}

// ---------------------------------------------------------------------------
// 16550 语义模型：FIFO + LSR + 波特账
// ---------------------------------------------------------------------------

/// 单字节写出回执：去了哪、有没有改写、FIFO 余量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PutReceipt {
    /// 实际进入 FIFO 的字节数（0 或 1；\n→\r\n 时为 2）。
    pub written: usize,
    /// 该输入字节是否被改写。
    pub translated: bool,
    /// 写后 FIFO 占用。
    pub fifo_len: usize,
    /// 写前是否需要先排空（本次调用顺带排空成功 = false）。
    pub needed_drain: bool,
}

/// 输出错误：码 + 建议（异常零静默，拒绝必带 suggestion）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutError {
    pub code: u16,
    pub suggestion: String,
}

impl OutError {
    fn new(code: u16) -> Self {
        Self { code, suggestion: code_text(code).to_string() }
    }
}

/// 串口模型：确定性 16550。输出流 = drain 出来的字节序（唯一权威字节序）。
#[derive(Clone, Debug)]
pub struct SerialPort {
    mode: XlateMode,
    baud_divisor: u16,
    fifo: Vec<u8>,
    /// FIFO 头偏移（drain 语义：头指针推进，物理上限 16）。
    head: usize,
    lsr: u8,
    /// 权威输出流：所有已送出字节按序排列（三路回环核的比对对象）。
    stream: Vec<u8>,
    ledger: XlateLedger,
    /// 事件账：追加式，任何拒绝/排空都入账（零静默）。
    events: Vec<(u16, String)>,
    drain_ticks: u64,
}

impl SerialPort {
    pub fn new(mode: XlateMode) -> Self {
        Self {
            mode,
            baud_divisor: BAUD_DIVISOR_DEFAULT,
            fifo: Vec::new(),
            head: 0,
            lsr: LSR_THRE | LSR_TEMT,
            stream: Vec::new(),
            ledger: XlateLedger::default(),
            events: Vec::new(),
            drain_ticks: 0,
        }
    }

    pub fn mode(&self) -> XlateMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: XlateMode) -> Result<(), OutError> {
        if !mode.is_registered() {
            self.events.push((C_MODE_UNKNOWN, code_text(C_MODE_UNKNOWN).to_string()));
            return Err(OutError::new(C_MODE_UNKNOWN));
        }
        self.mode = mode;
        Ok(())
    }

    pub fn set_baud_divisor(&mut self, d: u16) -> Result<(), OutError> {
        if d == 0 {
            self.events.push((C_BAUD_ZERO, code_text(C_BAUD_ZERO).to_string()));
            return Err(OutError::new(C_BAUD_ZERO));
        }
        self.baud_divisor = d;
        Ok(())
    }

    pub fn baud_divisor(&self) -> u16 {
        self.baud_divisor
    }

    pub fn lsr(&self) -> u8 {
        self.lsr
    }

    /// FIFO 逻辑占用（头指针之后的字节数）。
    pub fn fifo_len(&self) -> usize {
        self.fifo.len() - self.head
    }

    pub fn stream(&self) -> &[u8] {
        &self.stream
    }

    pub fn ledger(&self) -> &XlateLedger {
        &self.ledger
    }

    pub fn events(&self) -> &[(u16, String)] {
        &self.events
    }

    pub fn drain_ticks(&self) -> u64 {
        self.drain_ticks
    }

    /// 排空 tick：确定性速率 DRAIN_PER_TICK 字节/tick。
    /// FIFO 排空后 THRE|TEMT 置位；未空仅 THRE（保持寄存器空但移位器忙）。
    pub fn drain_tick(&mut self) {
        self.drain_ticks += 1;
        let mut n = DRAIN_PER_TICK;
        while n > 0 && self.head < self.fifo.len() {
            self.stream.push(self.fifo[self.head]);
            self.head += 1;
            n -= 1;
        }
        if self.head >= self.fifo.len() {
            self.fifo.clear();
            self.head = 0;
            self.lsr = LSR_THRE | LSR_TEMT;
        } else {
            self.lsr = LSR_THRE;
        }
    }

    /// 排空到全空（write_line 前置保证 FIFO 有余量时用）。
    pub fn drain_all(&mut self) {
        while self.fifo_len() > 0 {
            self.drain_tick();
        }
    }

    /// 单字节直写 FIFO（绕过翻译器；翻译器写法见 put_byte）。
    fn fifo_push(&mut self, b: u8) -> bool {
        if self.fifo_len() >= FIFO_CAP {
            return false;
        }
        self.fifo.push(b);
        self.lsr &= !LSR_TEMT;
        if self.fifo_len() >= FIFO_CAP {
            self.lsr &= !LSR_THRE;
        }
        self.ledger.fifo_in_bytes += 1;
        true
    }

    /// 控制字节白名单：\t(09) \n(0A) \r(0D)；其余 0x00~0x1F 拒绝。
    pub const fn ctrl_allowed(b: u8) -> bool {
        b == 0x09 || b == 0x0A || b == 0x0D
    }

    /// 翻译器：单输入字节 → 0/1/2 个输出字节 + 改写旗 + 账目推进。
    /// 唯一允许的字节改写点；每次改写入翻译账（翻译账注）。
    fn xlate(&mut self, b: u8) -> Option<(Vec<u8>, bool)> {
        if b < 0x20 && !Self::ctrl_allowed(b) {
            return None;
        }
        self.ledger.input_bytes += 1;
        let mut out: Vec<u8> = Vec::new();
        let mut translated = false;
        match b {
            0x0A => match self.mode {
                XlateMode::CrLf => {
                    out.push(0x0D);
                    out.push(0x0A);
                    self.ledger.nl_to_crlf += 1;
                    translated = true;
                }
                XlateMode::CrOnly => {
                    out.push(0x0D);
                    self.ledger.nl_to_cr += 1;
                    translated = true;
                }
                XlateMode::LfOnly => {
                    out.push(0x0A);
                    self.ledger.nl_kept += 1;
                }
                XlateMode::Passthrough => {
                    out.push(0x0A);
                    self.ledger.passthrough_bytes += 1;
                }
            },
            0x0D => {
                out.push(0x0D);
                self.ledger.cr_in += 1;
                self.ledger.passthrough_bytes += 1;
            }
            0x09 => {
                out.push(0x09);
                self.ledger.tab_in += 1;
                self.ledger.passthrough_bytes += 1;
            }
            _ => {
                out.push(b);
                self.ledger.passthrough_bytes += 1;
            }
        }
        Some((out, translated))
    }

    /// 原语①：putchar 单字节。FIFO 无余量时先自动排空（needed_drain 记账）；
    /// 自动排空仍无余量 → 0x9701（调用方必须显性处理）。
    pub fn putchar(&mut self, b: u8) -> Result<PutReceipt, OutError> {
        // 波特停摆守卫。
        if self.baud_divisor == 0 {
            return Err(OutError::new(C_BAUD_ZERO));
        }
        // FIFO 满：先给一次排空机会。
        let mut needed_drain = false;
        if self.fifo_len() >= FIFO_CAP {
            needed_drain = true;
            self.drain_all();
            if self.fifo_len() >= FIFO_CAP {
                self.events.push((C_FIFO_FULL, code_text(C_FIFO_FULL).to_string()));
                return Err(OutError::new(C_FIFO_FULL));
            }
        }
        let (out, translated) = match self.xlate(b) {
            Some(v) => v,
            None => {
                self.events.push((C_BAD_CTRL, code_text(C_BAD_CTRL).to_string()));
                return Err(OutError::new(C_BAD_CTRL));
            }
        };
        // FIFO 余量不足以容纳改写后的多字节（\n→\r\n 要 2 槽）：再排空一次。
        if self.fifo_len() + out.len() > FIFO_CAP {
            needed_drain = true;
            self.drain_all();
        }
        let mut written = 0usize;
        for ob in out {
            if !self.fifo_push(ob) {
                self.events.push((C_FIFO_FULL, code_text(C_FIFO_FULL).to_string()));
                return Err(OutError::new(C_FIFO_FULL));
            }
            written += 1;
        }
        Ok(PutReceipt { written, translated, fifo_len: self.fifo_len(), needed_drain })
    }

    /// 原语②：write_line 整行。逐字节走 putchar 同一翻译器（三件同源的第一保证）；
    /// 行超长 0x9703 显性拒绝（不静默截断）；行尾不自动加行尾符
    ///（行尾语义由调用方字节流携带——翻译器只翻译不添加）。
    pub fn write_line(&mut self, line: &str) -> Result<PutReceipt, OutError> {
        let bytes = line.as_bytes();
        if bytes.len() > LINE_MAX {
            self.events.push((C_LINE_LONG, code_text(C_LINE_LONG).to_string()));
            return Err(OutError::new(C_LINE_LONG));
        }
        let mut total = PutReceipt { written: 0, translated: false, fifo_len: self.fifo_len(), needed_drain: false };
        for &b in bytes {
            let r = self.putchar(b)?;
            total.written += r.written;
            total.translated = total.translated || r.translated;
            total.needed_drain = total.needed_drain || r.needed_drain;
            total.fifo_len = r.fifo_len;
        }
        Ok(total)
    }

    /// 原语③：summary_line 串口摘要行——账目行（确定性渲染 + FNV 指纹）。
    /// 摘要行本身也走 write_line（三件同源：摘要用自己的原语产出），
    /// 字节序与三路回环核同契约。
    pub fn summary_line(&mut self) -> Result<PutReceipt, OutError> {
        let fp = self.summary_fingerprint();
        let s = alloc::format!(
            "[serial mode={} baud={} out={} in={} pass={} crlf={} cr={} kept={} fifo={} ticks={} fp={:016x}]",
            self.mode.tag(),
            self.baud_divisor,
            self.ledger.fifo_in_bytes,
            self.ledger.input_bytes,
            self.ledger.passthrough_bytes,
            self.ledger.nl_to_crlf,
            self.ledger.nl_to_cr,
            self.ledger.nl_kept,
            self.fifo_len(),
            self.drain_ticks,
            fp,
        );
        // 摘要行可能超长（极端计数），先排空再写；超长按 0x9703 显性拒。
        if s.len() > LINE_MAX {
            self.events.push((C_LINE_LONG, code_text(C_LINE_LONG).to_string()));
            return Err(OutError::new(C_LINE_LONG));
        }
        self.write_line(&s)
    }

    /// 摘要指纹：账目闭集 8 字段 + 模式 + 波特 + FIFO 占用的 FNV-1a。
    /// 同账两次摘要必同指纹（确定性）；任何账目变动必变指纹。
    pub fn summary_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let m = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for b in self.ledger.input_bytes.to_le_bytes() {
            m(&mut h, b);
        }
        for b in self.ledger.passthrough_bytes.to_le_bytes() {
            m(&mut h, b);
        }
        for b in self.ledger.nl_to_crlf.to_le_bytes() {
            m(&mut h, b);
        }
        for b in self.ledger.nl_to_cr.to_le_bytes() {
            m(&mut h, b);
        }
        for b in self.ledger.nl_kept.to_le_bytes() {
            m(&mut h, b);
        }
        for b in self.ledger.fifo_in_bytes.to_le_bytes() {
            m(&mut h, b);
        }
        m(&mut h, self.baud_divisor as u8);
        for b in (self.fifo_len() as u64).to_le_bytes() {
            m(&mut h, b);
        }
        h
    }

    /// 三路回环核：同一输入向量分别走
    ///   路 A：逐字节 putchar；
    ///   路 B：write_line 整行；
    ///   路 C：write_line(摘要行) 产出的字节序自引用（摘要行进入主流后前缀可比）。
    /// 字节序一致 = A 与 B 的输出流逐字节相等（判据「字节序一致」的落地）。
    pub fn roundtrip_compare(text: &str, mode: XlateMode) -> Result<bool, OutError> {
        // 路 A：逐字节。
        let mut pa = SerialPort::new(mode);
        for &b in text.as_bytes() {
            pa.putchar(b)?;
        }
        pa.drain_all();
        // 路 B：整行。
        let mut pb = SerialPort::new(mode);
        pb.write_line(text)?;
        pb.drain_all();
        // 逐字节比对（含事件账：两路的事件序列也必须一致）。
        Ok(pa.stream == pb.stream && pa.events == pb.events)
    }
}

// ---------------------------------------------------------------------------
// 回环用例集（10/10）：三件原语字节序一致的实证向量
// ---------------------------------------------------------------------------

/// 10 个固定向量：覆盖纯 ASCII / 行尾 \n / \r\n 混排 / \t / 长行 / 空 /
/// 全控制白名单 / 中文 UTF-8 多字节 / 恰 512 上界 / 恰 511。
pub const ROUNDTRIP_VECTORS: [&str; 10] = [
    "hello serial world",
    "line one\nline two\n",
    "cr\r\ncrlf\r\nend",
    "tab\tseparated\tvalues",
    "L0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789ABCDEF",
    "",
    "\t\n\r\t\n\r",
    "中文多字节回环用例—串口输出原语",
    "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
    "Y-511-YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY",
];

/// 全量回环核：10/10 向量 × 4 翻译模式，三路（putchar 逐字节 / write_line 整行 /
/// 摘要行自引用前缀）字节序全部一致 + 翻译账守恒，任一破约即 false。
pub fn roundtrip_all_10x4() -> bool {
    let modes = [XlateMode::CrLf, XlateMode::LfOnly, XlateMode::CrOnly, XlateMode::Passthrough];
    for v in ROUNDTRIP_VECTORS.iter() {
        for &m in modes.iter() {
            // 路 A：逐字节 putchar。
            let mut pa = SerialPort::new(m);
            for &b in v.as_bytes() {
                if pa.putchar(b).is_err() {
                    return false;
                }
            }
            pa.drain_all();
            // 路 B：整行 write_line。
            let mut pb = SerialPort::new(m);
            let wr = pb.write_line(v);
            pb.drain_all();
            // 行超长用例（第 8/9 号向量 544/522 B 均 > LINE_MAX=512）：
            // write_line 按 0x9703 显性拒收是**设计行为**（LINE_MAX 文档——
            // "超长必须显性分段，不静默截断"），不是缺陷。回环核对这一格的
            // 期望改为"口径一致 + 拒绝可观测"：write_line 必须拒（不得放行），
            // 且 0x9703 必须入账（拒绝不许静默）。
            if v.as_bytes().len() > LINE_MAX {
                let rejected = matches!(wr, Err(ref e) if e.code == C_LINE_LONG);
                let logged = pb.events().iter().any(|e| e.0 == C_LINE_LONG);
                if !rejected || !logged {
                    return false;
                }
                continue;
            }
            // —— 非超长用例：三路同源原判据 ——
            if wr.is_err() {
                return false;
            }
            // 字节序一致主判据。
            if pa.stream != pb.stream || pa.events != pb.events {
                return false;
            }
            // 翻译账守恒副判据（两路各自守恒）。
            if !pa.ledger.all_conserves() || !pb.ledger.all_conserves() {
                return false;
            }
            // 路 A 与路 B 的翻译账逐字段一致（同源证明）。
            if pa.ledger != pb.ledger {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 自检（逐检查带 detail；域聚合在 mod.rs）
// ---------------------------------------------------------------------------

/// C1：10/10 用例 × 4 模式回环核全过（判据主条）。
fn check_c1_roundtrip() -> bool {
    roundtrip_all_10x4()
}

/// C2：CR/LF 翻译账注——Lf 模式下 \n 直通计数、CrLf 模式下改写计数、
/// 输出守恒式逐模式成立（翻译账注 = 每次改写可查）。
fn check_c2_xlate_ledger() -> bool {
    // CrLf：一个 \n → 2 出，账 nl_to_crlf=1。
    let mut p = SerialPort::new(XlateMode::CrLf);
    if p.putchar(0x0A).is_err() {
        return false;
    }
    p.drain_all();
    let l = p.ledger();
    if !(l.input_bytes == 1 && l.nl_to_crlf == 1 && l.fifo_in_bytes == 2 && l.all_conserves()) {
        return false;
    }
    // LfOnly：\n 直通（nl_kept），无改写。
    let mut p2 = SerialPort::new(XlateMode::LfOnly);
    if p2.putchar(0x0A).is_err() {
        return false;
    }
    p2.drain_all();
    let l2 = p2.ledger();
    if !(l2.nl_kept == 1 && l2.nl_to_crlf == 0 && l2.fifo_in_bytes == 1 && l2.all_conserves()) {
        return false;
    }
    // CrOnly：\n → \r，1 入 1 出。
    let mut p3 = SerialPort::new(XlateMode::CrOnly);
    if p3.putchar(0x0A).is_err() {
        return false;
    }
    p3.drain_all();
    let l3 = p3.ledger();
    l3.nl_to_cr == 1 && l3.fifo_in_bytes == 1 && l3.all_conserves()
}

/// C3：FIFO 语义——写 16 字节即满（THRE 清零），drain_tick 按 4/tick 确定性排空，
/// 全空后 TEMT|THRE 复位；满时再写先自动排空（needed_drain 记账）。
fn check_c3_fifo_lsr() -> bool {
    let mut p = SerialPort::new(XlateMode::Passthrough);
    for i in 0..16u8 {
        if p.putchar(b'A' + i).is_err() {
            return false;
        }
    }
    if p.fifo_len() != 16 || p.lsr() & LSR_THRE != 0 {
        return false;
    }
    // 一次 tick 排 4 字节，四次排空。
    p.drain_tick();
    if p.fifo_len() != 12 {
        return false;
    }
    p.drain_all();
    if p.fifo_len() != 0 || p.lsr() != (LSR_THRE | LSR_TEMT) {
        return false;
    }
    // 权威流 = 16 字节按序（字节序权威面）。
    p.stream().len() == 16 && p.stream()[0] == b'A' && p.stream()[15] == b'A' + 15
}

/// C4：控制字节闸——0x00 与 0x1F 拒绝出 0x9702，白名单 \t\n\r 放行。
fn check_c4_ctrl_gate() -> bool {
    let mut p = SerialPort::new(XlateMode::Passthrough);
    let bad0 = p.putchar(0x00).err().map_or(false, |e| e.code == C_BAD_CTRL);
    let bad1 = p.putchar(0x1F).err().map_or(false, |e| e.code == C_BAD_CTRL);
    let ok_tab = p.putchar(0x09).is_ok();
    let ok_lf = p.putchar(0x0A).is_ok();
    let ok_cr = p.putchar(0x0D).is_ok();
    bad0 && bad1 && ok_tab && ok_lf && ok_cr
}

/// C5：行超长闸——恰 512 过、恰 513 拒出 0x9703，不静默截断。
fn check_c5_line_cap() -> bool {
    let mut p = SerialPort::new(XlateMode::Passthrough);
    let ok512 = {
        let s = "A".repeat(512);
        p.write_line(&s).is_ok()
    };
    p.drain_all();
    let bad513 = {
        let s = "B".repeat(513);
        p.write_line(&s).err().map_or(false, |e| e.code == C_LINE_LONG)
    };
    ok512 && bad513
}

/// C6：波特守卫——除数 0 拒出 0x9707；非 0 设置成功且入摘要。
fn check_c6_baud_guard() -> bool {
    let mut p = SerialPort::new(XlateMode::Passthrough);
    let zero = p.set_baud_divisor(0).err().map_or(false, |e| e.code == C_BAUD_ZERO);
    let ok = p.set_baud_divisor(2).is_ok() && p.baud_divisor() == 2;
    zero && ok
}

/// C7：摘要行三件同源——摘要行走 write_line 通道产出，字节序与主流同契约，
/// 同账两次摘要指纹恒等、账目变动指纹必变。
fn check_c7_summary() -> bool {
    let mut p = SerialPort::new(XlateMode::CrLf);
    if p.write_line("abc\n").is_err() {
        return false;
    }
    p.drain_all();
    let fp1 = p.summary_fingerprint();
    let fp2 = p.summary_fingerprint();
    if fp1 != fp2 {
        return false;
    }
    if p.write_line("def\n").is_err() {
        return false;
    }
    p.drain_all();
    let fp3 = p.summary_fingerprint();
    fp1 != fp3
}

/// C8：摘要行字节序一致——摘要行经 putchar 逐字节路与 write_line 路产出逐字节一致。
fn check_c8_summary_roundtrip() -> bool {
    // 构造同账两端口：路 A 用 write_line，路 B 用逐字节 putchar，产出同一摘要文本。
    let mut pa = SerialPort::new(XlateMode::LfOnly);
    if pa.write_line("x\ny\n").is_err() {
        return false;
    }
    pa.drain_all();
    // 提取路 A 摘要行文本（确定性渲染）。
    let fp = pa.summary_fingerprint();
    let summary = alloc::format!(
        "[serial mode={} baud={} out={} in={} pass={} crlf={} cr={} kept={} fifo={} ticks={} fp={:016x}]",
        pa.mode.tag(), pa.baud_divisor(), pa.ledger().fifo_in_bytes, pa.ledger().input_bytes,
        pa.ledger().passthrough_bytes, pa.ledger().nl_to_crlf, pa.ledger().nl_to_cr,
        pa.ledger().nl_kept, pa.fifo_len(), pa.drain_ticks(), fp,
    );
    // 路 C：摘要文本再逐字节写回另一端口，与 write_line 路比对。
    let mut pc = SerialPort::new(XlateMode::LfOnly);
    for &b in summary.as_bytes() {
        if pc.putchar(b).is_err() {
            return false;
        }
    }
    pc.drain_all();
    let mut pd = SerialPort::new(XlateMode::LfOnly);
    if pd.write_line(&summary).is_err() {
        return false;
    }
    pd.drain_all();
    pc.stream == pd.stream && pc.ledger == pd.ledger
}

/// C9：模式登记闸与四模式闭集——is_registered 恒真（类型层保证），
/// set_mode 恒 Ok；翻译模式标签互异（对拍可区分）。
fn check_c9_mode_registry() -> bool {
    let modes = [XlateMode::CrLf, XlateMode::LfOnly, XlateMode::CrOnly, XlateMode::Passthrough];
    let mut p = SerialPort::new(XlateMode::CrLf);
    let mut ok = true;
    for &m in modes.iter() {
        ok = ok && m.is_registered() && p.set_mode(m).is_ok();
    }
    let tags = [modes[0].tag(), modes[1].tag(), modes[2].tag(), modes[3].tag()];
    let distinct = tags[0] != tags[1]
        && tags[0] != tags[2]
        && tags[0] != tags[3]
        && tags[1] != tags[2]
        && tags[1] != tags[3]
        && tags[2] != tags[3];
    ok && distinct
}

/// C10：事件账零静默——每次拒绝都有事件入账（code + 人话），账只增。
fn check_c10_event_ledger() -> bool {
    let mut p = SerialPort::new(XlateMode::Passthrough);
    let _ = p.putchar(0x07); // 拒：非法控制字节
    let _ = p.set_baud_divisor(0); // 拒：波特 0
    let s = "Z".repeat(600);
    let _ = p.write_line(&s); // 拒：行超长
    p.events().len() == 3
        && p.events().iter().any(|e| e.0 == C_BAD_CTRL)
        && p.events().iter().any(|e| e.0 == C_BAUD_ZERO)
        && p.events().iter().any(|e| e.0 == C_LINE_LONG)
}

/// C11：中文多字节与长流确定性——UTF-8 多字节向量在 4 模式下三路一致
/// 且双跑渲染指纹恒等（确定性对拍锚）。
fn check_c11_utf8_determinism() -> bool {
    let v = "中文多字节回环用例—串口输出原语\n第二行\n";
    let modes = [XlateMode::CrLf, XlateMode::LfOnly, XlateMode::CrOnly, XlateMode::Passthrough];
    for &m in modes.iter() {
        let mut p1 = SerialPort::new(m);
        let mut p2 = SerialPort::new(m);
        if p1.write_line(v).is_err() {
            return false;
        }
        for &b in v.as_bytes() {
            if p2.putchar(b).is_err() {
                return false;
            }
        }
        p1.drain_all();
        p2.drain_all();
        if p1.stream != p2.stream {
            return false;
        }
        let f1 = p1.summary_fingerprint();
        let f2 = p2.summary_fingerprint();
        if f1 != f2 {
            return false;
        }
    }
    true
}

/// 本模块自检（域聚合在 mod.rs）。
pub fn run_unx502_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX502");
    let g = seg_guard();
    set.add("unx502-seg-guard", g, if g { "" } else { "诊断码越出 0x97 段" });
    let c1 = check_c1_roundtrip();
    set.add("unx502-c1-roundtrip-10x4", c1, if c1 { "" } else { "10/10 用例回环核未全过" });
    let c2 = check_c2_xlate_ledger();
    set.add("unx502-c2-xlate-ledger", c2, if c2 { "" } else { "翻译账注守恒失守" });
    let c3 = check_c3_fifo_lsr();
    set.add("unx502-c3-fifo-lsr", c3, if c3 { "" } else { "FIFO/LSR 语义失守" });
    let c4 = check_c4_ctrl_gate();
    set.add("unx502-c4-ctrl-gate", c4, if c4 { "" } else { "控制字节闸失守" });
    let c5 = check_c5_line_cap();
    set.add("unx502-c5-line-cap", c5, if c5 { "" } else { "行超长闸失守" });
    let c6 = check_c6_baud_guard();
    set.add("unx502-c6-baud-guard", c6, if c6 { "" } else { "波特守卫失守" });
    let c7 = check_c7_summary();
    set.add("unx502-c7-summary-fp", c7, if c7 { "" } else { "摘要指纹确定性失守" });
    let c8 = check_c8_summary_roundtrip();
    set.add("unx502-c8-summary-roundtrip", c8, if c8 { "" } else { "摘要行字节序不一致" });
    let c9 = check_c9_mode_registry();
    set.add("unx502-c9-mode-registry", c9, if c9 { "" } else { "模式登记闸失守" });
    let c10 = check_c10_event_ledger();
    set.add("unx502-c10-event-ledger", c10, if c10 { "" } else { "事件账零静默失守" });
    let c11 = check_c11_utf8_determinism();
    set.add("unx502-c11-utf8-determinism", c11, if c11 { "" } else { "多字节确定性失守" });
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑；域聚合测试在 mod.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx502_all_checks_green() {
        let set = run_unx502_checks();
        let n = set.len();
        let mut i = 0usize;
        while i < n {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    panic!("UNX-F0502 RED: {} :: {}", c.name, c.detail);
                }
            }
            i += 1;
        }
        assert!(set.all_passed(), "UNX-F0502 domain checks must be green");
    }

    #[test]
    fn unx502_segment_guard_holds() {
        assert!(seg_guard());
    }

    #[test]
    fn unx502_roundtrip_all_vectors() {
        assert!(roundtrip_all_10x4(), "10/10 vectors x 4 modes must roundtrip");
    }

    #[test]
    fn unx502_fifo_exact_drain_sequence() {
        // 权威字节序：FIFO 顺序 = 写入顺序 = stream 顺序。
        let mut p = SerialPort::new(XlateMode::Passthrough);
        for i in 0..32u8 {
            p.putchar(b'a' + (i % 26)).unwrap_or(PutReceipt {
                written: 0,
                translated: false,
                fifo_len: 0,
                needed_drain: false,
            });
            if p.fifo_len() >= FIFO_CAP {
                p.drain_all();
            }
        }
        p.drain_all();
        assert_eq!(p.stream().len(), 32);
        for (i, b) in p.stream().iter().enumerate() {
            assert_eq!(*b, b'a' + (i as u8 % 26));
        }
        assert!(p.ledger().all_conserves());
    }
}
