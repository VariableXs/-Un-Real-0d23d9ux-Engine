//! UNX-F0502 · 串口输出原语（putchar/write 行）（CoRun·Unxstar · 串口深化之一）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0502`
//!
//! **判据 J1**：输出三件（putchar/write/串口摘要行）字节序一致（10/10 用例
//! 回环核），CR/LF 翻译账注。
//!
//! ## 要点一：输出三件 API 同管道同序
//!
//! putchar（逐字节）/ write（批量 write_str/write_checked/write_raw）/
//! 串口摘要行（summary 生成后回灌）三件全部走同一条 feed 管道：过滤→
//! 行限→CR/LF 翻译→转义→TX 环→排空到 sink。同管道 ⇒ 同字节序；
//! 回环校验器十用例对三件逐字节对拍（10/10 回环核）。
//!
//! ## 要点二：CR/LF 翻译矩阵与翻译账注
//!
//! 翻译开关二态（PassThrough/LfToCrLf）：开态下每个 `\n` 翻译为
//! `\r\n`（膨胀 1 字节），每次翻译在翻译账（TransLedger）落一条
//! 账注（序号/tick/输入偏移/原文/译文）——账注可审计不静默。
//!
//! ## 要点三：TX 环形缓冲 + 流控状态机
//!
//! TX 环定容量+高低水位；环满时显性丢弃并计数（不静默吞）。流控四态
//! 闭集（Ready/Busy/Blocked/Recovering），迁移表驱动，Blocked 期拒写
//! （FLOW_BLOCKED）——禁真实端口 IO，sink 为可注入 trait，默认回环
//! 内存 sink 保确定性。
//!
//! ## 要点四：波特率与 LCR 配置账只记账
//!
//! 波特率闭集五档（9600..115200），除数闭式 `1843200/(16*baud)`，
//! LCR 字节由数据位/停止位/校验位合成——只落账（ConfigLedger）不真实
//! 写端口（写端口面归 F0571 UART 驱），首次提交即冻结。
//!
//! ## 要点五：摘要行守恒对账
//!
//! 摘要行单行落：accepted/produced/translations/escapes/filtered/
//! dropped/lines/ops/conserved/tick。守恒恒等式：accepted + 翻译膨胀
//! (+1/次) + 转义膨胀 (+3/次) == produced + dropped——破恒等式即
//! CONSERVATION_BROKEN（摘要行自我声明 conserved 位）。
//!
//! ## 要点六：零 panic 面 + 诊断码独占 0x99xx 段
//!
//! 所有输入不可信：UTF-8 逐序列校验（边界/续字节/过长/代理/U+10FFFF）、
//! 控制字符三策略（Pass/Drop/Reject）、行限守护。诊断码独占 `0x99xx`
//! 段（grep 全 src 确认：vck31 对账面明写 0x9800..0x9900 开区间，
//! 0x99xx 无诊断段声明；0x9999 测试噪声与本闭集 0x9901..0x9910 互异）。
//! 本模块零 panic 面——不用 unwrap/expect/panic，错误一律 Err + 中文诊断。

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0x99xx 段——16 码互异）
// ---------------------------------------------------------------------------

/// 串口输出诊断码。独占 `0x99xx` 段（0x9901..0x9910）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoCode(pub u16);

impl SoCode {
    /// sink 未注入（调用侧传空——防御位）。
    pub const SINK_UNAVAILABLE: SoCode = SoCode(0x9901);
    /// sink 已关闭（回环 sink 被显性 close 后再写）。
    pub const SINK_CLOSED: SoCode = SoCode(0x9902);
    /// sink 写故障（注入故障点命中 / sink 容量尽）。
    pub const SINK_FAULT: SoCode = SoCode(0x9903);
    /// TX 环满溢出（字节被显性丢弃——会计数不静默）。
    pub const TX_OVERFLOW: SoCode = SoCode(0x9904);
    /// 流控阻塞期拒写（Blocked 态 gate 命中）。
    pub const FLOW_BLOCKED: SoCode = SoCode(0x9905);
    /// UTF-8 序列非法（首字节/续字节/过长/代理/超 U+10FFFF）。
    pub const UTF8_INVALID: SoCode = SoCode(0x9906);
    /// 控制字符被 Reject 策略拒收。
    pub const CTRL_REJECTED: SoCode = SoCode(0x9907);
    /// 单行超长（行限 At(n) 命中且当前字节非行终止符）。
    pub const LINE_TOO_LONG: SoCode = SoCode(0x9908);
    /// 流控状态迁移非法（force 路径迁移对不在闭集）。
    pub const STATE_ILLEGAL: SoCode = SoCode(0x9909);
    /// 水位/容量配置非法（零容量、高水位越界、高<=低违序）。
    pub const WATERMARK_INVALID: SoCode = SoCode(0x990A);
    /// 波特率不在支持闭集（五档之外）。
    pub const BAUD_UNSUPPORTED: SoCode = SoCode(0x990B);
    /// LCR 组合非法（数据位 5..8 外 / 停止位 1|2 外 / 除数不整除）。
    pub const LCR_MALFORMED: SoCode = SoCode(0x990C);
    /// 配置账已冻结（首次提交后改写拒——初始化恰一次）。
    pub const CONFIG_FROZEN: SoCode = SoCode(0x990D);
    /// 摘要对账守恒破（恒等式不成立——管道漏账）。
    pub const CONSERVATION_BROKEN: SoCode = SoCode(0x990E);
    /// 回环用例逐字节对拍不齐（got != want）。
    pub const CASE_MISMATCH: SoCode = SoCode(0x990F);
    /// 报告重复提交（引导落账恰一次语义违）。
    pub const COMMIT_TWICE: SoCode = SoCode(0x9910);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（中文诊断——错误面唯一出口）。
    pub fn reason(self) -> String {
        match self {
            SoCode::SINK_UNAVAILABLE => "sink 未注入：输出引擎没有可写目标".into(),
            SoCode::SINK_CLOSED => "sink 已关闭：关闭后再写拒收".into(),
            SoCode::SINK_FAULT => "sink 写故障：注入故障点命中或 sink 容量尽".into(),
            SoCode::TX_OVERFLOW => "TX 环满溢出：字节显性丢弃已计数".into(),
            SoCode::FLOW_BLOCKED => "流控阻塞期拒写：先排空恢复再写".into(),
            SoCode::UTF8_INVALID => "UTF-8 序列非法：首字节/续字节/过长/代理/超界".into(),
            SoCode::CTRL_REJECTED => "控制字符被 Reject 策略拒收".into(),
            SoCode::LINE_TOO_LONG => "单行超长：行限命中且当前字节非行终止符".into(),
            SoCode::STATE_ILLEGAL => "流控状态迁移非法：迁移对不在闭集".into(),
            SoCode::WATERMARK_INVALID => "水位/容量配置非法：零容量或水位违序".into(),
            SoCode::BAUD_UNSUPPORTED => "波特率不在支持闭集（9600/19200/38400/57600/115200）".into(),
            SoCode::LCR_MALFORMED => "LCR 组合非法：数据位/停止位越界或除数不整除".into(),
            SoCode::CONFIG_FROZEN => "配置账已冻结：首次提交后改写拒".into(),
            SoCode::CONSERVATION_BROKEN => "摘要守恒破：accepted+膨胀 != produced+dropped".into(),
            SoCode::CASE_MISMATCH => "回环用例对拍不齐：got 与 want 字节序不一致".into(),
            SoCode::COMMIT_TWICE => "报告重复提交：落账恰一次语义违".into(),
            SoCode(_) => "未知 UNX 串口输出域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、串口 sink 抽象（可注入 trait）+ 默认回环内存 sink
// ---------------------------------------------------------------------------

/// 串口 sink 抽象——真实 UART/回环内存统一面（禁真实端口 IO：本模块
/// 只经此 trait 写字节，端口面归 F0571）。
pub trait SerialSink {
    /// 写单字节（sink 满或故障 → Err——显性不静默）。
    fn put_byte(&mut self, b: u8) -> Result<(), SoCode>;
    /// 可写信号（流控状态机的 ready 输入）。
    fn ready(&self) -> bool;
    /// sink 名（审计可读面）。
    fn name(&self) -> &'static str;
}

/// 回环内存 sink（默认注入物——确定性：同输入恒同字节序）。
///
/// 容量上限/关闭位/故障注入点三件齐备：故障注入 `fault_at` 指定第 N 次
/// 写入时返回 SINK_FAULT（确定性故障复现——不依赖墙钟）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackSink {
    buf: Vec<u8>,
    cap: usize,
    closed: bool,
    fault_at: Option<u64>,
    writes: u64,
    rejected: u64,
}

impl LoopbackSink {
    /// 新建回环 sink（容量至少 1）。
    pub fn new(cap: usize) -> LoopbackSink {
        LoopbackSink {
            buf: Vec::new(),
            cap: if cap == 0 { 1 } else { cap },
            closed: false,
            fault_at: None,
            writes: 0,
            rejected: 0,
        }
    }

    /// 新建带故障注入点的回环 sink（第 `at` 次写入返回 SINK_FAULT）。
    pub fn with_fault(cap: usize, at: u64) -> LoopbackSink {
        let mut s = LoopbackSink::new(cap);
        s.fault_at = Some(at);
        s
    }

    /// 显性关闭（关闭后 put_byte → SINK_CLOSED）。
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// 已收字节面（回环核对面——只读不夺走）。
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    /// 收走全部字节（回环核对后清场用）。
    pub fn take_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// 累计写入次数（含失败）。
    pub const fn written(&self) -> u64 {
        self.writes
    }

    /// 因容量尽被拒次数（sink 侧显性账）。
    pub const fn rejected(&self) -> u64 {
        self.rejected
    }
}

impl SerialSink for LoopbackSink {
    fn put_byte(&mut self, b: u8) -> Result<(), SoCode> {
        self.writes += 1;
        if self.closed {
            return Err(SoCode::SINK_CLOSED);
        }
        if let Some(at) = self.fault_at {
            if self.writes == at {
                return Err(SoCode::SINK_FAULT);
            }
        }
        if self.buf.len() >= self.cap {
            self.rejected += 1;
            return Err(SoCode::SINK_FAULT);
        }
        self.buf.push(b);
        Ok(())
    }

    fn ready(&self) -> bool {
        !self.closed
    }

    fn name(&self) -> &'static str {
        "loopback"
    }
}

// ---------------------------------------------------------------------------
// 三、CR/LF 翻译矩阵与翻译账注
// ---------------------------------------------------------------------------

/// CR/LF 翻译矩阵模式（二态闭集）。
///
/// - `PassThrough`：逐字节原样过——二进制安全面（0x0A 原样出）。
/// - `LfToCrLf`：`\n` → `\r\n`（膨胀 +1 字节/次）；`\r` 原样过；
///   其余字节原样过。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrLfMode {
    /// 不翻译（逐字节恒等）。
    PassThrough,
    /// `\n` → `\r\n`（经典终端行尾——每次翻译记账注）。
    LfToCrLf,
}

/// 单字节翻译矩阵（纯函数——(首字节, 次字节, 产出字节数) 三元组）。
///
/// 次字节仅当产出数为 2 时有效；产出数闭集 {1, 2}。
pub const fn translate_byte(mode: CrLfMode, b: u8) -> (u8, u8, u8) {
    match mode {
        CrLfMode::PassThrough => (b, 0, 1),
        CrLfMode::LfToCrLf => {
            if b == b'\n' {
                (b'\r', b'\n', 2)
            } else {
                (b, 0, 1)
            }
        }
    }
}

/// 单条翻译账注（每次 `\n→\r\n` 翻译恰一条——账注可审计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransEntry {
    /// 账注序号（自 1 递增——翻译账全局序）。
    pub seq: u64,
    /// 逻辑 tick（禁墙钟——确定性时间轴）。
    pub tick: u64,
    /// 输入偏移（该 `\n` 是管道接受的第几个字节，自 0 起）。
    pub at: u64,
    /// 原文字节（恒 0x0A——翻译账只记 LF 翻译）。
    pub from: u8,
    /// 译文字节对（恒 [\r, \n]）。
    pub to: [u8; 2],
}

/// 翻译账（滚动容量——超容截断显性标注不静默）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransLedger {
    entries: Vec<TransEntry>,
    cap: usize,
    count: u64,
    truncated: bool,
}

impl TransLedger {
    /// 新建翻译账（容量至少 1）。
    pub fn new(cap: usize) -> TransLedger {
        TransLedger {
            entries: Vec::new(),
            cap: if cap == 0 { 1 } else { cap },
            count: 0,
            truncated: false,
        }
    }

    /// 落一条翻译账注（只在翻译发生时由引擎调用——账注与翻译一一对应）。
    pub fn note(&mut self, tick: u64, at: u64, from: u8) {
        self.count += 1;
        if self.entries.len() < self.cap {
            self.entries.push(TransEntry {
                seq: self.count,
                tick,
                at,
                from,
                to: [b'\r', b'\n'],
            });
        } else {
            self.truncated = true;
        }
    }

    /// 账注总数（含被截断未存明细的——账面总数不因滚动窗缩水）。
    pub const fn total(&self) -> u64 {
        self.count
    }

    /// 存留明细（<= cap 条——滚动窗内最近账注）。
    pub fn entries(&self) -> &[TransEntry] {
        &self.entries
    }

    /// 是否发生明细截断。
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

// ---------------------------------------------------------------------------
// 四、UTF-8 合法性校验器（多字节序列边界）
// ---------------------------------------------------------------------------

/// UTF-8 故障类别（闭集五类——诊断明细面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Utf8FaultKind {
    /// 首字节非法（孤立续字节 0x80..0xBF 当首）。
    BadLead,
    /// 续字节非法（非 0x80..0xBF）或序列截断。
    BadCont,
    /// 过长编码（0xC0/0xC1 首字节；E0 后越下界；F0 后越下界）。
    Overlong,
    /// UTF-16 代理区（U+D800..U+DFFF——ED A0..BF）。
    Surrogate,
    /// 超 U+10FFFF（F5..FF 首字节；F4 后越上界）。
    BeyondU10,
}

/// UTF-8 故障位点（at 为输入内偏移——write_checked 首错即停）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utf8Fault {
    /// 首错字节偏移。
    pub at: usize,
    /// 故障类别。
    pub kind: Utf8FaultKind,
}

/// 首字节分类（Ok(序列长) / Err(类别)——闭式矩阵）。
fn utf8_lead_class(b0: u8) -> Result<usize, Utf8FaultKind> {
    if b0 < 0x80 {
        Ok(1)
    } else if b0 < 0xC0 {
        Err(Utf8FaultKind::BadLead)
    } else if b0 < 0xC2 {
        // 0xC0/0xC1 必过长（两字节编码最短 U+80 = 0xC2 0x80）。
        Err(Utf8FaultKind::Overlong)
    } else if b0 < 0xE0 {
        Ok(2)
    } else if b0 < 0xF0 {
        Ok(3)
    } else if b0 < 0xF5 {
        Ok(4)
    } else {
        // 0xF5..=0xFF 必超 U+10FFFF。
        Err(Utf8FaultKind::BeyondU10)
    }
}

/// 续字节判定（0x80..=0xBF）。
const fn is_cont(b: u8) -> bool {
    b >= 0x80 && b < 0xC0
}

/// 校验输入开头的单个 UTF-8 序列（Ok(序列长) / Err(位点+类别)）。
///
/// 规则矩阵：① 首字节分类定长；② 长度不足 → BadCont 于缺位；
/// ③ 续字节逐位验；④ E0/F0 越下界 → Overlong；⑤ ED A0..BF →
/// Surrogate；⑥ F4 90.. → BeyondU10。全部边界显性不猜。
pub fn utf8_check_seq(bytes: &[u8]) -> Result<usize, Utf8Fault> {
    if bytes.is_empty() {
        return Ok(0);
    }
    let b0 = bytes[0];
    let l = match utf8_lead_class(b0) {
        Ok(l) => l,
        Err(kind) => return Err(Utf8Fault { at: 0, kind }),
    };
    if bytes.len() < l {
        return Err(Utf8Fault {
            at: bytes.len(),
            kind: Utf8FaultKind::BadCont,
        });
    }
    let b1 = bytes[1];
    match l {
        1 => Ok(1),
        2 => {
            if !is_cont(b1) {
                Err(Utf8Fault { at: 1, kind: Utf8FaultKind::BadCont })
            } else {
                Ok(2)
            }
        }
        3 => {
            // E0 下界（过长）与 ED 代理区（UTF-16 半体）双特殊面。
            if b0 == 0xE0 && !(b1 >= 0xA0 && b1 <= 0xBF) {
                return Err(Utf8Fault { at: 1, kind: Utf8FaultKind::Overlong });
            }
            if b0 == 0xED && !(b1 >= 0x80 && b1 <= 0x9F) {
                return Err(Utf8Fault { at: 1, kind: Utf8FaultKind::Surrogate });
            }
            if !is_cont(b1) {
                return Err(Utf8Fault { at: 1, kind: Utf8FaultKind::BadCont });
            }
            if !is_cont(bytes[2]) {
                return Err(Utf8Fault { at: 2, kind: Utf8FaultKind::BadCont });
            }
            Ok(3)
        }
        _ => {
            // F0 下界（过长）与 F4 上界（超 U+10FFFF）双特殊面。
            if b0 == 0xF0 && !(b1 >= 0x90 && b1 <= 0xBF) {
                return Err(Utf8Fault { at: 1, kind: Utf8FaultKind::Overlong });
            }
            if b0 == 0xF4 && !(b1 >= 0x80 && b1 <= 0x8F) {
                return Err(Utf8Fault { at: 1, kind: Utf8FaultKind::BeyondU10 });
            }
            if !is_cont(b1) {
                return Err(Utf8Fault { at: 1, kind: Utf8FaultKind::BadCont });
            }
            if !is_cont(bytes[2]) {
                return Err(Utf8Fault { at: 2, kind: Utf8FaultKind::BadCont });
            }
            if !is_cont(bytes[3]) {
                return Err(Utf8Fault { at: 3, kind: Utf8FaultKind::BadCont });
            }
            Ok(4)
        }
    }
}

/// 全序列扫描（首错即停——at 为全输入内偏移）。
pub fn utf8_scan(bytes: &[u8]) -> Result<(), Utf8Fault> {
    let mut i = 0usize;
    while i < bytes.len() {
        let l = utf8_check_seq(&bytes[i..]).map_err(|f| Utf8Fault {
            at: i + f.at,
            kind: f.kind,
        })?;
        i += l;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、TX 环形缓冲（容量 + 水位 + 溢出显性丢弃计数）
// ---------------------------------------------------------------------------

/// TX 环水位形态（闭集四态——流控状态机的 level 输入画像）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WmState {
    /// 低于低水位（排空充分）。
    BelowLo,
    /// 低高水位之间（正常带）。
    Between,
    /// 达高水位（Busy 带）。
    AtOrAboveHi,
    /// 全满（Blocked 带入口）。
    Full,
}

/// TX 环形缓冲（定容量 FIFO——head/len 双量，tail = head+len 模 cap）。
///
/// 纪律：① push 满时显性丢弃并计数（Ok(false)——不静默吞不 panic）；
/// ② 水位只读面供流控状态机；③ drain 逐字节排到 sink，sink 故障即停
/// （余量留环内——下次续排，不丢账）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxRing {
    buf: Vec<u8>,
    cap: usize,
    head: usize,
    len: usize,
    hi_wm: usize,
    lo_wm: usize,
    drops: u64,
    drop_bytes: u64,
}

impl TxRing {
    /// 新建 TX 环（校验：容量>0、高水位<=容量、低<=高——违者
    /// WATERMARK_INVALID）。
    pub fn new(cap: usize, hi_wm: usize, lo_wm: usize) -> Result<TxRing, SoCode> {
        if cap == 0 || hi_wm > cap || lo_wm > hi_wm {
            return Err(SoCode::WATERMARK_INVALID);
        }
        Ok(TxRing {
            buf: vec![0u8; cap],
            cap,
            head: 0,
            len: 0,
            hi_wm,
            lo_wm,
            drops: 0,
            drop_bytes: 0,
        })
    }

    /// 容量。
    pub const fn capacity(&self) -> usize {
        self.cap
    }

    /// 当前水位（环内滞留字节数）。
    pub const fn level(&self) -> usize {
        self.len
    }

    /// 高水位。
    pub const fn high_watermark(&self) -> usize {
        self.hi_wm
    }

    /// 低水位。
    pub const fn low_watermark(&self) -> usize {
        self.lo_wm
    }

    /// 是否全满。
    pub const fn is_full(&self) -> bool {
        self.len == self.cap
    }

    /// 是否全空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 水位形态（流控输入画像）。
    pub const fn wm_state(&self) -> WmState {
        if self.len == self.cap {
            WmState::Full
        } else if self.len >= self.hi_wm {
            WmState::AtOrAboveHi
        } else if self.len < self.lo_wm {
            WmState::BelowLo
        } else {
            WmState::Between
        }
    }

    /// 推入单字节（满 → 显性丢弃计数，返回 Ok(false)——不 panic）。
    pub fn push(&mut self, b: u8) -> bool {
        if self.len == self.cap {
            self.drops += 1;
            self.drop_bytes += 1;
            return false;
        }
        let tail = (self.head + self.len) % self.cap;
        self.buf[tail] = b;
        self.len += 1;
        true
    }

    /// 弹出单字节（空 → None）。
    pub fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let b = self.buf[self.head];
        self.head = (self.head + 1) % self.cap;
        self.len -= 1;
        Some(b)
    }

    /// 排空到 sink（sink ready 且环非空时逐字节写；sink 故障即停——
    /// 余量留环内续排）。返回成功写入 sink 的字节数。
    pub fn drain(&mut self, sink: &mut dyn SerialSink) -> Result<usize, SoCode> {
        let mut n = 0usize;
        while self.len > 0 {
            if !sink.ready() {
                break;
            }
            let b = match self.pop() {
                Some(b) => b,
                None => break,
            };
            sink.put_byte(b)?;
            n += 1;
        }
        Ok(n)
    }

    /// 累计丢弃次数（溢出显性账）。
    pub const fn drops(&self) -> u64 {
        self.drops
    }

    /// 累计丢弃字节数（每丢弃恰 1 字节——与 drops 恒等）。
    pub const fn drop_bytes(&self) -> u64 {
        self.drop_bytes
    }
}

// ---------------------------------------------------------------------------
// 六、流控状态机（就绪/忙/阻塞/恢复——迁移表闭式）
// ---------------------------------------------------------------------------

/// 流控状态（闭集四态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowState {
    /// 就绪：水位低于高水位且 sink 可写——可写。
    Ready,
    /// 忙：水位达高水位但未满——仍可写（背压预警带）。
    Busy,
    /// 阻塞：环满或 sink 不可写——拒写（FLOW_BLOCKED）。
    Blocked,
    /// 恢复：阻塞解除后一拍过渡——观察带（可写）。
    Recovering,
}

impl FlowState {
    /// 状态名（快照/渲染单点）。
    pub const fn name(self) -> &'static str {
        match self {
            FlowState::Ready => "ready",
            FlowState::Busy => "busy",
            FlowState::Blocked => "blocked",
            FlowState::Recovering => "recovering",
        }
    }

    /// 阻塞位（Blocked 态 gate 拒写）。
    pub const fn is_blocked(self) -> bool {
        matches!(self, FlowState::Blocked)
    }
}

/// 流控状态机（迁移表驱动 + tick 驻留账——禁墙钟）。
///
/// 迁移表（闭集）：
/// - Ready   → Busy（level 达高水位）/ Blocked（sink 不可写）/ Ready
/// - Busy    → Blocked（环满或 sink 不可写）/ Ready（level 低于低水位）/ Busy
/// - Blocked → Recovering（sink 可写且 level 低于低水位）/ Blocked
/// - Recovering → Ready（sink 可写）/ Blocked（sink 又不可写——回退）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowCtrl {
    state: FlowState,
    hi_wm: usize,
    lo_wm: usize,
    entered_tick: u64,
    transitions: u64,
    blocked_total: u64,
    recoveries: u64,
}

impl FlowCtrl {
    /// 新建流控（校验：高水位>=1、低<=高——违者 WATERMARK_INVALID）。
    pub fn new(hi_wm: usize, lo_wm: usize) -> Result<FlowCtrl, SoCode> {
        if hi_wm == 0 || lo_wm > hi_wm {
            return Err(SoCode::WATERMARK_INVALID);
        }
        Ok(FlowCtrl {
            state: FlowState::Ready,
            hi_wm,
            lo_wm,
            entered_tick: 0,
            transitions: 0,
            blocked_total: 0,
            recoveries: 0,
        })
    }

    /// 当前状态。
    pub const fn state(&self) -> FlowState {
        self.state
    }

    /// 高水位。
    pub const fn high_watermark(&self) -> usize {
        self.hi_wm
    }

    /// 低水位。
    pub const fn low_watermark(&self) -> usize {
        self.lo_wm
    }

    /// 状态迁移总数。
    pub const fn transitions(&self) -> u64 {
        self.transitions
    }

    /// 累计阻塞时长（逻辑 tick 差和——离开 Blocked 时结算）。
    pub const fn blocked_total(&self) -> u64 {
        self.blocked_total
    }

    /// 恢复次数（进入 Recovering 计一次）。
    pub const fn recoveries(&self) -> u64 {
        self.recoveries
    }

    /// 状态机步进（每字节写前调——迁移表闭式全覆盖）。
    pub fn step(&mut self, level: usize, full: bool, sink_ready: bool, tick: u64) -> FlowState {
        let old = self.state;
        let next = match old {
            FlowState::Ready => {
                if !sink_ready {
                    FlowState::Blocked
                } else if level >= self.hi_wm {
                    FlowState::Busy
                } else {
                    FlowState::Ready
                }
            }
            FlowState::Busy => {
                if full || !sink_ready {
                    FlowState::Blocked
                } else if level < self.lo_wm {
                    FlowState::Ready
                } else {
                    FlowState::Busy
                }
            }
            FlowState::Blocked => {
                if sink_ready && level < self.lo_wm {
                    FlowState::Recovering
                } else {
                    FlowState::Blocked
                }
            }
            FlowState::Recovering => {
                if !sink_ready {
                    FlowState::Blocked
                } else {
                    FlowState::Ready
                }
            }
        };
        if next != old {
            if old == FlowState::Blocked {
                // 离开阻塞：结算本段阻塞驻留（tick 差——逻辑时间轴）。
                let dwell = if tick >= self.entered_tick { tick - self.entered_tick } else { 0 };
                self.blocked_total += dwell;
            }
            if next == FlowState::Recovering {
                self.recoveries += 1;
            }
            self.transitions += 1;
            self.entered_tick = tick;
            self.state = next;
        }
        self.state
    }

    /// 写前闸（Blocked → FLOW_BLOCKED——调用侧先排空恢复）。
    pub const fn gate(&self) -> Result<(), SoCode> {
        if self.state.is_blocked() {
            Err(SoCode::FLOW_BLOCKED)
        } else {
            Ok(())
        }
    }

    /// 强设状态（恢复/排空路径用——迁移对必须在合法闭集内，
    /// 违者 STATE_ILLEGAL 不改态）。
    pub fn force(&mut self, to: FlowState, tick: u64) -> Result<(), SoCode> {
        let legal = match (self.state, to) {
            (a, b) if a == b => true,
            (FlowState::Ready, FlowState::Busy) => true,
            (FlowState::Ready, FlowState::Blocked) => true,
            (FlowState::Busy, FlowState::Ready) => true,
            (FlowState::Busy, FlowState::Blocked) => true,
            (FlowState::Blocked, FlowState::Recovering) => true,
            (FlowState::Recovering, FlowState::Ready) => true,
            (FlowState::Recovering, FlowState::Blocked) => true,
            _ => false,
        };
        if !legal {
            return Err(SoCode::STATE_ILLEGAL);
        }
        if to != self.state {
            if self.state == FlowState::Blocked {
                let dwell = if tick >= self.entered_tick { tick - self.entered_tick } else { 0 };
                self.blocked_total += dwell;
            }
            if to == FlowState::Recovering {
                self.recoveries += 1;
            }
            self.transitions += 1;
            self.entered_tick = tick;
            self.state = to;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 七、波特率与 LCR 配置账（只记账不真实写端口）
// ---------------------------------------------------------------------------

/// UART 基频（经典 1843200 Hz——除数闭式分子）。
pub const UART_BASE_HZ: u32 = 1_843_200;

/// 支持波特率闭集（五档——档外 BAUD_UNSUPPORTED）。
pub const SUPPORTED_BAUDS: [u32; 5] = [9600, 19200, 38400, 57600, 115200];

/// 校验位（闭集三态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    /// 无校验。
    None,
    /// 奇校验。
    Odd,
    /// 偶校验。
    Even,
}

/// LCR 配置方案（除数 + LCR 字节——只算不写端口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LcrPlan {
    /// 波特率。
    pub baud: u32,
    /// 除数（= 1843200/(16*baud)——必须整除）。
    pub divisor: u16,
    /// 数据位（5..=8）。
    pub data_bits: u8,
    /// 停止位（1|2）。
    pub stop_bits: u8,
    /// 校验位。
    pub parity: Parity,
    /// LCR 字节（数据位低 2 位 | 停止位 0x04 | 校验 0x08/0x18）。
    pub lcr: u8,
}

/// 波特率/LCR 方案合成（纯函数——只出方案不碰端口）。
///
/// 规则：① 波特率必须在五档闭集内；② 数据位 5..=8、停止位 1|2；
/// ③ 除数 = UART_BASE_HZ/(16*baud) 必须整除且 >=1；④ LCR 按位合成。
/// 任一违者：BAUD_UNSUPPORTED / LCR_MALFORMED。
pub fn plan_lcr(baud: u32, data_bits: u8, stop_bits: u8, parity: Parity) -> Result<LcrPlan, SoCode> {
    let mut known = false;
    let mut i = 0usize;
    while i < SUPPORTED_BAUDS.len() {
        if SUPPORTED_BAUDS[i] == baud {
            known = true;
            break;
        }
        i += 1;
    }
    if !known {
        return Err(SoCode::BAUD_UNSUPPORTED);
    }
    if !(5..=8).contains(&data_bits) {
        return Err(SoCode::LCR_MALFORMED);
    }
    if stop_bits != 1 && stop_bits != 2 {
        return Err(SoCode::LCR_MALFORMED);
    }
    let denom = 16u32.checked_mul(baud);
    let denom = match denom {
        Some(d) if d != 0 => d,
        _ => return Err(SoCode::LCR_MALFORMED),
    };
    if UART_BASE_HZ % denom != 0 {
        return Err(SoCode::LCR_MALFORMED);
    }
    let divisor = UART_BASE_HZ / denom;
    if divisor == 0 || divisor > 0xFFFF {
        return Err(SoCode::LCR_MALFORMED);
    }
    let mut lcr: u8 = (data_bits - 5) & 0x03;
    if stop_bits == 2 {
        lcr |= 0x04;
    }
    match parity {
        Parity::None => {}
        Parity::Odd => lcr |= 0x08,
        Parity::Even => lcr |= 0x18,
    }
    Ok(LcrPlan {
        baud,
        divisor: divisor as u16,
        data_bits,
        stop_bits,
        parity,
        lcr,
    })
}

/// 配置账条目（提交序号 + tick + 方案——审计可回放）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LcrEntry {
    /// 提交序号（自 1 递增）。
    pub seq: u64,
    /// 逻辑 tick。
    pub tick: u64,
    /// 方案。
    pub plan: LcrPlan,
}

/// 配置账（首次提交即冻结——真机串口初始化恰一次）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLedger {
    entries: Vec<LcrEntry>,
    frozen: bool,
}

impl ConfigLedger {
    /// 新建空配置账。
    pub const fn new() -> ConfigLedger {
        ConfigLedger {
            entries: Vec::new(),
            frozen: false,
        }
    }

    /// 是否已冻结。
    pub const fn frozen(&self) -> bool {
        self.frozen
    }

    /// 已提交条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判定（冻结位与条目数双零——起步形态）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && !self.frozen
    }

    /// 条目面。
    pub fn entries(&self) -> &[LcrEntry] {
        &self.entries
    }

    /// 提交方案（冻结后 CONFIG_FROZEN 拒——只记账不写端口）。
    pub fn commit(&mut self, tick: u64, plan: LcrPlan) -> Result<u64, SoCode> {
        if self.frozen {
            return Err(SoCode::CONFIG_FROZEN);
        }
        self.frozen = true;
        let seq = self.entries.len() as u64 + 1;
        self.entries.push(LcrEntry { seq, tick, plan });
        Ok(seq)
    }
}

/// 配置账声明（判据对拍锚——只记账不真实写端口）。
pub const CONFIG_NOTE: &str =
    "波特率与 LCR 只记账不真实写端口——写端口面归 F0571 UART 驱，配置账首次提交即冻结";

// ---------------------------------------------------------------------------
// 八、核心输出引擎：三件 API + 转义模式 + 审计账滚动窗
// ---------------------------------------------------------------------------

/// 控制字符策略（闭集三态——不可见字节集：0x00..0x1F 除 \t\r\n，及 0x7F）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// 全过（二进制安全——控制字符原样出）。
    Pass,
    /// 丢弃不可见字节（计数 filtered——不报错）。
    Drop,
    /// 拒收不可见字节（CTRL_REJECTED——首错即停）。
    Reject,
}

/// 行限（Off 不限 / At(n) 单行至多 n 字节——行终止符 \r\n 不计入不触发）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineLimit {
    /// 不限行长（二进制流形态）。
    Off,
    /// 限 n 字节（n>=1）。
    At(usize),
}

/// 输出引擎配置（翻译/过滤/转义/行限四维闭式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SerialConfig {
    /// CR/LF 翻译开关（true = \n→\r\n）。
    pub crlf: bool,
    /// 控制字符策略。
    pub filter: FilterMode,
    /// 转义输出开关（不可见字节 → \xNN 四字节记法）。
    pub escape: bool,
    /// 行限。
    pub line_max: LineLimit,
}

/// 输出统计账（守恒恒等式的原子量——全 u64 不溢出面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SerialStats {
    /// 管道接受字节数（过滤后口径——被 Drop 的不计入）。
    pub accepted: u64,
    /// 推入 TX 环字节数（翻译/转义膨胀后口径）。
    pub produced: u64,
    /// CR/LF 翻译次数（每 +1 膨胀 1 字节）。
    pub translations: u64,
    /// 转义次数（每 +1 膨胀 3 字节：1 字节 → \xNN 4 字节）。
    pub escapes: u64,
    /// 被过滤丢弃字节数（Drop 策略口径）。
    pub filtered: u64,
    /// TX 环满显性丢弃字节数。
    pub dropped: u64,
    /// 已完成行数（\n 计数口径）。
    pub lines: u64,
    /// 输出操作次数（putchar 计 1，write 计 1——不分字节）。
    pub ops: u64,
}

impl SerialStats {
    /// 全零账（引擎起步形态）。
    pub const fn zero() -> SerialStats {
        SerialStats {
            accepted: 0,
            produced: 0,
            translations: 0,
            escapes: 0,
            filtered: 0,
            dropped: 0,
            lines: 0,
            ops: 0,
        }
    }

    /// 膨胀字节数（翻译 +1/次 + 转义 +3/次——守恒恒等式左侧增量）。
    pub const fn expand_bytes(&self) -> u64 {
        self.translations + self.escapes * 3
    }

    /// 守恒恒等式：accepted + 膨胀 == produced + dropped。
    pub const fn conserved(&self) -> bool {
        self.accepted + self.expand_bytes() == self.produced + self.dropped
    }
}

/// 单次输出回执（op 级聚合——调用侧可审计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutReceipt {
    /// 接受字节数。
    pub accepted: u32,
    /// 产出入环字节数。
    pub produced: u32,
    /// 本次翻译次数。
    pub translations: u32,
    /// 本次转义次数。
    pub escapes: u32,
    /// 本次过滤丢弃数。
    pub filtered: u32,
    /// 本次环满丢弃数。
    pub dropped: u32,
    /// 本次完成行数。
    pub lines: u32,
    /// 本次实际排到 sink 的字节数。
    pub flushed: u32,
}

impl PutReceipt {
    /// 全零回执。
    pub const fn zero() -> PutReceipt {
        PutReceipt {
            accepted: 0,
            produced: 0,
            translations: 0,
            escapes: 0,
            filtered: 0,
            dropped: 0,
            lines: 0,
            flushed: 0,
        }
    }
}

/// 审计操作类别（闭集六态——审计账滚动窗的 op 维度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// putchar 逐字节。
    PutChar,
    /// write_str / write_checked 批量。
    WriteStr,
    /// write_raw 二进制。
    WriteRaw,
    /// write_line 行写。
    WriteLine,
    /// flush 排空。
    Flush,
    /// 摘要生成。
    Summary,
}

impl OpKind {
    /// 类别名（审计渲染单点）。
    pub const fn name(self) -> &'static str {
        match self {
            OpKind::PutChar => "putchar",
            OpKind::WriteStr => "write_str",
            OpKind::WriteRaw => "write_raw",
            OpKind::WriteLine => "write_line",
            OpKind::Flush => "flush",
            OpKind::Summary => "summary",
        }
    }
}

/// 审计账条目（op 级——一次输出操作恰一条）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditEntry {
    /// 全局序号（滚动窗统一编——自 1 递增）。
    pub seq: u64,
    /// 逻辑 tick。
    pub tick: u64,
    /// 操作类别。
    pub op: OpKind,
    /// 接受字节数。
    pub in_b: u32,
    /// 产出入环字节数。
    pub out_b: u32,
    /// 是否整体成功。
    pub ok: bool,
    /// 失败诊断码（ok=false 时有值）。
    pub code: Option<SoCode>,
}

/// 审计账滚动窗（定容环形——超容覆盖最老并计 wrapped，不静默）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditWindow {
    slots: Vec<Option<AuditEntry>>,
    head: usize,
    count: usize,
    wrapped: u64,
    seq: u64,
}

impl AuditWindow {
    /// 新建滚动窗（容量至少 1）。
    pub fn new(cap: usize) -> AuditWindow {
        AuditWindow {
            slots: vec![None; if cap == 0 { 1 } else { cap }],
            head: 0,
            count: 0,
            wrapped: 0,
            seq: 0,
        }
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// 窗内条目数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 覆盖次数（滚动窗溢出显性账）。
    pub const fn wrapped(&self) -> u64 {
        self.wrapped
    }

    /// 追加条目（满 → 覆盖最老——wrapped+1；seq 统一编发不重号）。
    pub fn push(&mut self, mut e: AuditEntry) {
        self.seq += 1;
        e.seq = self.seq;
        let cap = self.slots.len();
        if self.count < cap {
            let idx = (self.head + self.count) % cap;
            self.slots[idx] = Some(e);
            self.count += 1;
        } else {
            self.slots[self.head] = Some(e);
            self.head = (self.head + 1) % cap;
            self.wrapped += 1;
        }
    }

    /// 按时间序取出窗内条目（旧→新——快照不夺走）。
    pub fn entries(&self) -> Vec<AuditEntry> {
        let mut out = Vec::new();
        let cap = self.slots.len();
        let mut i = 0usize;
        while i < self.count {
            let idx = (self.head + i) % cap;
            if let Some(e) = self.slots[idx] {
                out.push(e);
            }
            i += 1;
        }
        out
    }
}

/// 单字节喂入回执（feed 管道内账——op 级聚合的原子）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeedOut {
    accepted: u32,
    produced: u32,
    translations: u32,
    escapes: u32,
    filtered: u32,
    dropped: u32,
    lines: u32,
    flushed: u32,
}

/// 不可见字节判定（0x00..0x1F 除 \t\r\n，及 0x7F——过滤/转义共用矩阵）。
const fn is_invisible_byte(b: u8) -> bool {
    (b < 0x20 && b != b'\t' && b != b'\r' && b != b'\n') || b == 0x7F
}

/// 小写十六进制半字节表（\xNN 记法用）。
const HEX_LOWER: [u8; 16] = [
    b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd', b'e', b'f',
];

/// 转义展开（不可见字节 → [\, x, hi, lo] 四字节——共 3 字节膨胀）。
pub fn escape_expand(b: u8) -> [u8; 4] {
    [b'\\', b'x', HEX_LOWER[(b >> 4) as usize & 0x0F], HEX_LOWER[(b & 0x0F) as usize]]
}

/// 串口输出引擎（三件 API 同管道——字节序一致性由构造保证）。
///
/// 管道序：流控步进 → 写闸 → 过滤 → 行限 → CR/LF 翻译（账注）→
/// 转义 → TX 环（溢出显性丢）→ 排空 sink → 审计落窗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialOut {
    /// TX 环。
    pub ring: TxRing,
    /// 流控状态机。
    pub flow: FlowCtrl,
    /// 配置。
    pub cfg: SerialConfig,
    /// 统计账。
    pub stats: SerialStats,
    /// 审计滚动窗。
    pub audit: AuditWindow,
    /// CR/LF 翻译账。
    pub trans: TransLedger,
    /// 当前行内字节计数（行限守护用——\r/\n 归零）。
    pub line_len: usize,
    /// 最近一次 UTF-8 故障位点（write_checked 失败后可查）。
    pub last_fault: Option<Utf8Fault>,
    /// 最近 tick 快照。
    pub tick: u64,
}

impl SerialOut {
    /// 新建引擎（全量校验：环容量/水位/行限/账容——违者 Err 中文诊断）。
    pub fn new(
        cfg: SerialConfig,
        ring_cap: usize,
        hi_wm: usize,
        lo_wm: usize,
        trans_cap: usize,
        audit_cap: usize,
    ) -> Result<SerialOut, SoCode> {
        let ring = TxRing::new(ring_cap, hi_wm, lo_wm)?;
        let flow = FlowCtrl::new(hi_wm, lo_wm)?;
        if let LineLimit::At(n) = cfg.line_max {
            if n == 0 {
                return Err(SoCode::WATERMARK_INVALID);
            }
        }
        Ok(SerialOut {
            ring,
            flow,
            cfg,
            stats: SerialStats::zero(),
            audit: AuditWindow::new(audit_cap),
            trans: TransLedger::new(trans_cap),
            line_len: 0,
            last_fault: None,
            tick: 0,
        })
    }

    /// 管道喂单字节（全路径——三件 API 的共同底座）。
    fn feed(&mut self, sink: &mut dyn SerialSink, tick: u64, b: u8) -> Result<FeedOut, SoCode> {
        self.tick = tick;
        // ① 流控步进 + 写闸（Blocked 期拒写——先排空恢复）。
        let full = self.ring.is_full();
        let st = self.flow.step(self.ring.level(), full, sink.ready(), tick);
        if st.is_blocked() {
            return Err(SoCode::FLOW_BLOCKED);
        }
        // ② 控制字符过滤（Pass 直落 / Drop 计数 / Reject 报错）。
        if is_invisible_byte(b) && self.cfg.filter != FilterMode::Pass {
            if self.cfg.filter == FilterMode::Reject {
                return Err(SoCode::CTRL_REJECTED);
            }
            self.stats.filtered += 1;
            return Ok(FeedOut {
                accepted: 0,
                produced: 0,
                translations: 0,
                escapes: 0,
                filtered: 1,
                dropped: 0,
                lines: 0,
                flushed: 0,
            });
        }
        // ③ 行限守护（\r/\n 是行终止符——恰界不触发，超界首错即停）。
        if let LineLimit::At(n) = self.cfg.line_max {
            if self.line_len >= n && b != b'\r' && b != b'\n' {
                return Err(SoCode::LINE_TOO_LONG);
            }
        }
        // ④ CR/LF 翻译（账注与翻译一一对应——账注不静默）。
        let mode = if self.cfg.crlf {
            CrLfMode::LfToCrLf
        } else {
            CrLfMode::PassThrough
        };
        let (b0, b1, n) = translate_byte(mode, b);
        let mut translations = 0u32;
        if n == 2 {
            self.trans.note(tick, self.stats.accepted, b);
            self.stats.translations += 1;
            translations = 1;
        }
        // ⑤ 产出逐字节：转义模式把不可见字节摊成 \xNN 四字节；推环
        //    （满 → 显性丢弃计数）。
        let mut out = FeedOut {
            accepted: 1,
            produced: 0,
            translations,
            escapes: 0,
            filtered: 0,
            dropped: 0,
            lines: 0,
            flushed: 0,
        };
        let mut k = 0usize;
        while k < n as usize {
            let raw = if k == 0 { b0 } else { b1 };
            if self.cfg.escape && is_invisible_byte(raw) {
                let esc = escape_expand(raw);
                let mut j = 0usize;
                while j < 4 {
                    if self.ring.push(esc[j]) {
                        out.produced += 1;
                    } else {
                        self.stats.dropped += 1;
                        out.dropped += 1;
                    }
                    j += 1;
                }
                self.stats.escapes += 1;
                out.escapes += 1;
            } else if self.ring.push(raw) {
                out.produced += 1;
            } else {
                self.stats.dropped += 1;
                out.dropped += 1;
            }
            k += 1;
        }
        self.stats.accepted += 1;
        self.stats.produced += out.produced as u64;
        // ⑥ 行计数（\n 完成一行；\r 归零行内计数不完成行）。
        if b == b'\n' {
            self.stats.lines += 1;
            self.line_len = 0;
            out.lines = 1;
        } else if b == b'\r' {
            self.line_len = 0;
        } else {
            self.line_len += 1;
        }
        // ⑦ 排空到 sink（sink 故障即停——余量留环内续排不丢账）。
        out.flushed = self.ring.drain(sink)? as u32;
        Ok(out)
    }

    /// 审计落窗（op 级——成功/失败都落，失败带诊断码；seq 由窗统一编）。
    fn audit_op(&mut self, op: OpKind, tick: u64, r: &PutReceipt, ok: bool, code: Option<SoCode>) {
        self.audit.push(AuditEntry {
            seq: 0,
            tick,
            op,
            in_b: r.accepted,
            out_b: r.produced,
            ok,
            code,
        });
    }

    /// 【三件之一】putchar——单字节输出（判据主面）。
    pub fn putchar(&mut self, sink: &mut dyn SerialSink, tick: u64, b: u8) -> Result<PutReceipt, SoCode> {
        self.stats.ops += 1;
        let mut r = PutReceipt::zero();
        match self.feed(sink, tick, b) {
            Ok(f) => {
                r.accepted = f.accepted;
                r.produced = f.produced;
                r.translations = f.translations;
                r.escapes = f.escapes;
                r.filtered = f.filtered;
                r.dropped = f.dropped;
                r.lines = f.lines;
                r.flushed = f.flushed;
                self.audit_op(OpKind::PutChar, tick, &r, true, None);
                Ok(r)
            }
            Err(c) => {
                self.audit_op(OpKind::PutChar, tick, &r, false, Some(c));
                Err(c)
            }
        }
    }

    /// 批量喂入底座（逐字节 feed + op 级审计一次——首错即停）。
    fn run_bytes(&mut self, sink: &mut dyn SerialSink, tick: u64, bytes: &[u8], op: OpKind) -> Result<PutReceipt, SoCode> {
        self.stats.ops += 1;
        let mut r = PutReceipt::zero();
        for &b in bytes {
            match self.feed(sink, tick, b) {
                Ok(f) => {
                    r.accepted += f.accepted;
                    r.produced += f.produced;
                    r.translations += f.translations;
                    r.escapes += f.escapes;
                    r.filtered += f.filtered;
                    r.dropped += f.dropped;
                    r.lines += f.lines;
                    r.flushed += f.flushed;
                }
                Err(c) => {
                    self.audit_op(op, tick, &r, false, Some(c));
                    return Err(c);
                }
            }
        }
        self.audit_op(op, tick, &r, true, None);
        Ok(r)
    }

    /// 【三件之二】write_str——批量写 &str（类型即 UTF-8 保证；仍走
    /// 同一 feed 管道——与 putchar 字节序由构造恒等）。
    pub fn write_str(&mut self, sink: &mut dyn SerialSink, tick: u64, s: &str) -> Result<PutReceipt, SoCode> {
        self.run_bytes(sink, tick, s.as_bytes(), OpKind::WriteStr)
    }

    /// write_checked——批量写字节切片（UTF-8 全扫描前置——首错即停
    /// 不产出半个序列；位点落 last_fault 可查）。
    pub fn write_checked(&mut self, sink: &mut dyn SerialSink, tick: u64, bytes: &[u8]) -> Result<PutReceipt, SoCode> {
        if let Err(f) = utf8_scan(bytes) {
            self.last_fault = Some(f);
            self.stats.ops += 1;
            self.audit_op(OpKind::WriteStr, tick, &PutReceipt::zero(), false, Some(SoCode::UTF8_INVALID));
            return Err(SoCode::UTF8_INVALID);
        }
        self.run_bytes(sink, tick, bytes, OpKind::WriteStr)
    }

    /// write_raw——二进制安全批量写（无 UTF-8 前置——逐字节即字节序）。
    pub fn write_raw(&mut self, sink: &mut dyn SerialSink, tick: u64, bytes: &[u8]) -> Result<PutReceipt, SoCode> {
        self.run_bytes(sink, tick, bytes, OpKind::WriteRaw)
    }

    /// 【三件之二·行形】write_line_checked——写一行并补行尾（行内容
    /// 不含行尾；行尾经同管道翻译面——crlf 开时落 \r\n）。
    pub fn write_line_checked(&mut self, sink: &mut dyn SerialSink, tick: u64, line: &[u8]) -> Result<PutReceipt, SoCode> {
        if let Err(f) = utf8_scan(line) {
            self.last_fault = Some(f);
            self.stats.ops += 1;
            self.audit_op(OpKind::WriteLine, tick, &PutReceipt::zero(), false, Some(SoCode::UTF8_INVALID));
            return Err(SoCode::UTF8_INVALID);
        }
        let mut buf = Vec::with_capacity(line.len() + 1);
        buf.extend_from_slice(line);
        buf.push(b'\n');
        self.run_bytes(sink, tick, &buf, OpKind::WriteLine)
    }

    /// write_line（&str 面）。
    pub fn write_line(&mut self, sink: &mut dyn SerialSink, tick: u64, s: &str) -> Result<PutReceipt, SoCode> {
        let mut buf = Vec::with_capacity(s.len() + 1);
        buf.extend_from_slice(s.as_bytes());
        buf.push(b'\n');
        self.run_bytes(sink, tick, &buf, OpKind::WriteLine)
    }

    /// flush——排空环内余量（审计 op=Flush）。
    pub fn flush(&mut self, sink: &mut dyn SerialSink, tick: u64) -> Result<usize, SoCode> {
        self.tick = tick;
        let n = self.ring.drain(sink)?;
        self.audit.push(AuditEntry {
            seq: 0,
            tick,
            op: OpKind::Flush,
            in_b: 0,
            out_b: n as u32,
            ok: true,
            code: None,
        });
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// 九、摘要行生成与守恒对账（三件之三）
// ---------------------------------------------------------------------------

/// 摘要快照（SerialStats 同构值——摘要行/守恒对账的输入）。
pub type Summary = SerialStats;

/// 摘要守恒恒等式声明（判据对拍锚）。
pub const CONSERVATION_NOTE: &str = "守恒恒等式：accepted + translations*1 + escapes*3 == produced + dropped——摘要行 conserved 位自我声明，破恒等式即 CONSERVATION_BROKEN";

impl Summary {
    /// 会计面同账判定（忽略 ops——putchar 逐字节与 write 批量的 ops
    /// 天然不同，字节序与守恒量必须同）。
    pub const fn accounting_eq(&self, other: &Summary) -> bool {
        self.accepted == other.accepted
            && self.produced == other.produced
            && self.translations == other.translations
            && self.escapes == other.escapes
            && self.filtered == other.filtered
            && self.dropped == other.dropped
            && self.lines == other.lines
    }

    /// 【三件之三】串口摘要行（单行——判据「串口摘要行」本体）。
    ///
    /// 形态：`# varix-serialout accepted=N produced=N trans=N esc=N
    /// filt=N drop=N lines=N ops=N conserved=<bool> tick=T`
    pub fn line(&self, tick: u64) -> String {
        format!(
            "# varix-serialout accepted={} produced={} trans={} esc={} filt={} drop={} lines={} ops={} conserved={} tick={}",
            self.accepted,
            self.produced,
            self.translations,
            self.escapes,
            self.filtered,
            self.dropped,
            self.lines,
            self.ops,
            self.conserved(),
            tick,
        )
    }

    /// 摘要 kv 行组（多行渲染面——与 F0041 报告 kv 同构）。
    pub fn kv_lines(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push("# varix-serialout-summary".to_string());
        out.push(format!("accepted={}", self.accepted));
        out.push(format!("produced={}", self.produced));
        out.push(format!("translations={}", self.translations));
        out.push(format!("escapes={}", self.escapes));
        out.push(format!("filtered={}", self.filtered));
        out.push(format!("dropped={}", self.dropped));
        out.push(format!("lines={}", self.lines));
        out.push(format!("ops={}", self.ops));
        out.push(format!("conserved={}", self.conserved()));
        out
    }

    /// kv 行数闸（行数 == 结构闭式——渲染确定性对拍锚）。
    pub const fn kv_expected_len() -> usize {
        // 头 1 + 八计数 + conserved 1 = 10。
        10
    }

    /// 守恒闸（恒等式破 → Err(CONSERVATION_BROKEN)——摘要出账前闸）。
    pub const fn conservation_gate(&self) -> Result<(), SoCode> {
        if self.conserved() {
            Ok(())
        } else {
            Err(SoCode::CONSERVATION_BROKEN)
        }
    }
}

// ---------------------------------------------------------------------------
// 十、回环校验器（10 个内建用例——三件字节序一致性逐字节对拍）
// ---------------------------------------------------------------------------

/// 回环核用例环容量（超长/二进制例足容）。
pub const CASE_RING_CAP: usize = 128;
/// 回环核用例高水位。
pub const CASE_HI_WM: usize = 96;
/// 回环核用例低水位。
pub const CASE_LO_WM: usize = 32;
/// 回环核用例 sink 容量。
pub const CASE_SINK_CAP: usize = 4096;
/// 回环核用例逻辑 tick（禁墙钟——定值保确定性）。
pub const CASE_TICK: u64 = 100;

/// 判据原文（逐字对拍锚）。
pub const J1_NOTE: &str =
    "输出三件（putchar/write/串口摘要行）字节序一致（10/10 用例回环核），CR/LF 翻译账注";

/// 发射路径（三路对拍——putchar 逐字节 / write 批量 / write_line 逐行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitPath {
    /// 逐字节 putchar。
    PerByte,
    /// 批量 write（二进制用 write_raw，余用 write_checked）。
    Batch,
    /// 逐行 write_line（仅输入以 \n 结尾时适用——行写语义天然补行尾）。
    Lines,
}

/// 回环核用例规格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaseSpec {
    /// 用例号（1..=10）。
    pub id: usize,
    /// 用例名。
    pub name: &'static str,
    /// CR/LF 翻译开关。
    pub crlf: bool,
    /// 过滤策略。
    pub filter: FilterMode,
    /// 行限。
    pub line_max: LineLimit,
    /// 二进制面（Batch 路走 write_raw——不做 UTF-8 前置）。
    pub binary: bool,
}

/// 用例输入（按 id 闭式构造——确定性数据源）。
pub fn case_input(id: usize) -> Vec<u8> {
    match id {
        1 => Vec::new(),
        2 => vec![b'A'],
        3 => b"a\nb\n".to_vec(),
        4 => b"a\rb\r".to_vec(),
        5 => b"x\r\ny\nz".to_vec(),
        6 => {
            let mut v = vec![b'X'; 65];
            v.push(b'\n');
            v
        }
        7 => vec![0xE4, 0xB8, 0xAD, 0x0A, 0xE6, 0x96, 0x87],
        8 => vec![b'a', 0x01, b'\t', b'b', 0x7F, 0x0A, b'c'],
        9 => {
            let mut v = vec![b'X'; 64];
            v.push(b'\n');
            v
        }
        _ => {
            let mut v = Vec::with_capacity(256);
            let mut i = 0u16;
            while i <= 255 {
                v.push(i as u8);
                i += 1;
            }
            v
        }
    }
}

/// 期望输出（独立手写面——与引擎无共享代码，对拍才有意义）。
pub fn expected_bytes(id: usize) -> Vec<u8> {
    match id {
        1 => Vec::new(),
        2 => vec![b'A'],
        3 => vec![b'a', b'\r', b'\n', b'b', b'\r', b'\n'],
        4 => vec![b'a', b'\r', b'b', b'\r'],
        5 => vec![b'x', b'\r', b'\r', b'\n', b'y', b'\r', b'\n', b'z'],
        6 => vec![b'X'; 64],
        7 => vec![0xE4, 0xB8, 0xAD, 0x0D, 0x0A, 0xE6, 0x96, 0x87],
        8 => vec![b'a', b'\t', b'b', b'\r', b'\n', b'c'],
        9 => {
            let mut v = vec![b'X'; 64];
            v.push(b'\r');
            v.push(b'\n');
            v
        }
        _ => {
            let mut v = Vec::with_capacity(256);
            let mut i = 0u16;
            while i <= 255 {
                v.push(i as u8);
                i += 1;
            }
            v
        }
    }
}

/// 期望错误码（6 号超长行 → LINE_TOO_LONG；余全过）。
pub fn expected_err(id: usize) -> Option<SoCode> {
    if id == 6 {
        Some(SoCode::LINE_TOO_LONG)
    } else {
        None
    }
}

/// 用例表（10 例闭集——判据「10/10 用例」本体）。
pub fn case_table() -> Vec<CaseSpec> {
    vec![
        CaseSpec { id: 1, name: "空串", crlf: true, filter: FilterMode::Drop, line_max: LineLimit::At(64), binary: false },
        CaseSpec { id: 2, name: "单字符", crlf: true, filter: FilterMode::Drop, line_max: LineLimit::At(64), binary: false },
        CaseSpec { id: 3, name: "纯LF", crlf: true, filter: FilterMode::Drop, line_max: LineLimit::At(64), binary: false },
        CaseSpec { id: 4, name: "纯CR", crlf: true, filter: FilterMode::Drop, line_max: LineLimit::At(64), binary: false },
        CaseSpec { id: 5, name: "混合", crlf: true, filter: FilterMode::Drop, line_max: LineLimit::At(64), binary: false },
        CaseSpec { id: 6, name: "超长行", crlf: true, filter: FilterMode::Drop, line_max: LineLimit::At(64), binary: false },
        CaseSpec { id: 7, name: "UTF-8中文多字节", crlf: true, filter: FilterMode::Drop, line_max: LineLimit::At(64), binary: false },
        CaseSpec { id: 8, name: "控制字符过滤", crlf: true, filter: FilterMode::Drop, line_max: LineLimit::At(64), binary: false },
        CaseSpec { id: 9, name: "行尾恰好边界", crlf: true, filter: FilterMode::Drop, line_max: LineLimit::At(64), binary: false },
        CaseSpec { id: 10, name: "二进制安全逐字节", crlf: false, filter: FilterMode::Pass, line_max: LineLimit::Off, binary: true },
    ]
}

/// 首个错位点（逐字节对拍——None 即全等含长度；长度差落在共同前缀之后）。
pub fn first_mismatch(a: &[u8], b: &[u8]) -> Option<usize> {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < n {
        if a[i] != b[i] {
            return Some(i);
        }
        i += 1;
    }
    if a.len() != b.len() {
        return Some(n);
    }
    None
}

/// 单路径执行（新引擎 + 新回环 sink——路径间零串扰）。
fn run_path(spec: &CaseSpec, path: EmitPath, input: &[u8]) -> (Vec<u8>, Option<SoCode>, SerialStats) {
    let cfg = SerialConfig {
        crlf: spec.crlf,
        filter: spec.filter,
        escape: false,
        line_max: spec.line_max,
    };
    let mut eng = match SerialOut::new(cfg, CASE_RING_CAP, CASE_HI_WM, CASE_LO_WM, 64, 64) {
        Ok(e) => e,
        Err(c) => return (Vec::new(), Some(c), SerialStats::zero()),
    };
    let mut snk = LoopbackSink::new(CASE_SINK_CAP);
    let mut err: Option<SoCode> = None;
    match path {
        EmitPath::PerByte => {
            let mut i = 0usize;
            while i < input.len() {
                let b = input[i];
                if let Err(c) = eng.putchar(&mut snk, CASE_TICK, b) {
                    err = Some(c);
                    break;
                }
                i += 1;
            }
        }
        EmitPath::Batch => {
            if spec.binary {
                if let Err(c) = eng.write_raw(&mut snk, CASE_TICK, input) {
                    err = Some(c);
                }
            } else if let Err(c) = eng.write_checked(&mut snk, CASE_TICK, input) {
                err = Some(c);
            }
        }
        EmitPath::Lines => {
            // 逐行写：\n 触发 write_line（行内容不含行尾——行写补尾）；
            // 尾段兜底仅当输入不以 \n 结尾时按批量补（防御面）。
            let mut seg: Vec<u8> = Vec::new();
            let mut i = 0usize;
            while i < input.len() {
                let b = input[i];
                if b == b'\n' {
                    if let Err(c) = eng.write_line_checked(&mut snk, CASE_TICK, &seg) {
                        err = Some(c);
                        break;
                    }
                    seg.clear();
                } else {
                    seg.push(b);
                }
                i += 1;
            }
            if err.is_none() && !seg.is_empty() {
                if let Err(c) = eng.write_checked(&mut snk, CASE_TICK, &seg) {
                    err = Some(c);
                }
            }
        }
    }
    let stats = eng.stats;
    let bytes = snk.bytes().to_vec();
    (bytes, err, stats)
}

/// 用例对拍结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// 用例号。
    pub id: usize,
    /// 用例名。
    pub name: &'static str,
    /// 三路字节序一致（PerByte == Batch == Lines(如适用)）。
    pub three_way: bool,
    /// got 与 want 首个错位点。
    pub mismatch_at: Option<usize>,
    /// 实际输出（PerByte 路为基准面）。
    pub got: Vec<u8>,
    /// 期望输出（独立手写面）。
    pub want: Vec<u8>,
    /// 实际错误码。
    pub err: Option<SoCode>,
    /// 期望错误码。
    pub want_err: Option<SoCode>,
    /// 翻译账总数（账注面——crlf 用例与实际处理的 \n 数一致）。
    pub translations: u64,
    /// 用例整体过（三路一致 + got==want + err==want_err）。
    pub ok: bool,
}

/// 跑单个用例（三路发射 + 期望对拍——10/10 回环核的原子）。
pub fn run_case(spec: &CaseSpec) -> CaseResult {
    let input = case_input(spec.id);
    let want = expected_bytes(spec.id);
    let want_err = expected_err(spec.id);
    let (a, ea, sa) = run_path(spec, EmitPath::PerByte, &input);
    let (b, eb, _) = run_path(spec, EmitPath::Batch, &input);
    let mut three_way = a == b && ea == eb;
    // Lines 路仅当输入以 \n 结尾（行写语义天然补行尾——其余输入不适用）。
    if !input.is_empty() && input[input.len() - 1] == b'\n' {
        let (c, ec, _) = run_path(spec, EmitPath::Lines, &input);
        three_way = three_way && a == c && ea == ec;
    }
    let mismatch_at = first_mismatch(&a, &want);
    let ok = mismatch_at.is_none() && a.len() == want.len() && ea == want_err && three_way;
    CaseResult {
        id: spec.id,
        name: spec.name,
        three_way,
        mismatch_at,
        got: a,
        want,
        err: ea,
        want_err,
        translations: sa.translations,
        ok,
    }
}

/// 摘要行自核（三件之三的对拍面）：
/// ① 同输入 putchar 逐字节 vs write_checked 批量——会计量（除 ops）同、
/// sink 字节序同、摘要行文本同；② 双侧守恒；③ 摘要行文本回灌第三引擎
/// ——sink 字节 == 摘要文本字节（摘要行自身字节序一致）。
pub fn summary_selfcheck() -> bool {
    let cfg = SerialConfig {
        crlf: true,
        filter: FilterMode::Drop,
        escape: false,
        line_max: LineLimit::At(64),
    };
    let mut e1 = match SerialOut::new(cfg, CASE_RING_CAP, CASE_HI_WM, CASE_LO_WM, 64, 64) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let mut e2 = match SerialOut::new(cfg, CASE_RING_CAP, CASE_HI_WM, CASE_LO_WM, 64, 64) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let mut s1 = LoopbackSink::new(CASE_SINK_CAP);
    let mut s2 = LoopbackSink::new(CASE_SINK_CAP);
    // "中\n文\n"——多字节 + 翻译双覆盖。
    let text: [u8; 7] = [0xE4, 0xB8, 0xAD, 0x0A, 0xE6, 0x96, 0x87];
    let mut i = 0usize;
    while i < text.len() {
        if e1.putchar(&mut s1, CASE_TICK, text[i]).is_err() {
            return false;
        }
        i += 1;
    }
    if e2.write_checked(&mut s2, CASE_TICK, &text).is_err() {
        return false;
    }
    let z1 = e1.stats;
    let z2 = e2.stats;
    if !(z1.conserved() && z2.conserved()) {
        return false;
    }
    if !z1.accounting_eq(&z2) {
        return false;
    }
    if s1.bytes() != s2.bytes() {
        return false;
    }
    let l1 = z1.line(CASE_TICK);
    let l2 = z2.line(CASE_TICK);
    if l1 != l2 {
        return false;
    }
    // 摘要行文本回灌第三引擎——字节序逐字节一致。
    let mut e3 = match SerialOut::new(cfg, CASE_RING_CAP, CASE_HI_WM, CASE_LO_WM, 64, 64) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let mut s3 = LoopbackSink::new(CASE_SINK_CAP);
    let lb = l1.as_bytes();
    if e3.write_raw(&mut s3, CASE_TICK + 1, lb).is_err() {
        return false;
    }
    s3.bytes() == lb
}

/// 回环核总报告（10 用例 + 摘要自核——判据 J1 落账形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifierReport {
    /// 逐用例结果（序 = 用例号 1..=10）。
    pub cases: Vec<CaseResult>,
    /// 过用例数。
    pub passed: usize,
    /// 全过位（10/10）。
    pub all_ok: bool,
    /// 摘要行自核位（三件之三对拍）。
    pub summary_check: bool,
    /// 判据原文。
    pub note: &'static str,
}

/// 跑全量回环核（10/10 用例 + 摘要自核——总入口）。
pub fn verify_all() -> VerifierReport {
    let table = case_table();
    let mut cases = Vec::with_capacity(table.len());
    let mut passed = 0usize;
    let mut i = 0usize;
    while i < table.len() {
        let r = run_case(&table[i]);
        if r.ok {
            passed += 1;
        }
        cases.push(r);
        i += 1;
    }
    let all_ok = passed == cases.len();
    let summary_check = summary_selfcheck();
    VerifierReport {
        cases,
        passed,
        all_ok,
        summary_check,
        note: J1_NOTE,
    }
}

// ---------------------------------------------------------------------------
// 十一、真机落账槽（单次提交——回环核报告引导后可查不可改）
// ---------------------------------------------------------------------------

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

static VERIFY_SLOT: ReportSlot = ReportSlot::new();
static COMMITTED: AtomicBool = AtomicBool::new(false);

/// 报告落账槽（零分配初始化——UnsafeCell 包裹 Option）。
struct ReportSlot {
    slot: UnsafeCell<Option<VerifierReport>>,
}

unsafe impl Sync for ReportSlot {}

impl ReportSlot {
    const fn new() -> ReportSlot {
        ReportSlot {
            slot: UnsafeCell::new(None),
        }
    }
}

/// 引导期提交回环核报告（恰一次——重复提交拒 COMMIT_TWICE）。
pub fn boot_commit(report: VerifierReport) -> Result<(), SoCode> {
    if COMMITTED.swap(true, Ordering::AcqRel) {
        return Err(SoCode::COMMIT_TWICE);
    }
    unsafe {
        let slot = VERIFY_SLOT.slot.get();
        if (*slot).is_some() {
            return Err(SoCode::COMMIT_TWICE);
        }
        *slot = Some(report);
    }
    Ok(())
}

/// 引导后读取报告（未提交返回 None——只读不夺走）。
pub fn committed_report() -> Option<VerifierReport> {
    if !COMMITTED.load(Ordering::Acquire) {
        return None;
    }
    unsafe {
        let slot = VERIFY_SLOT.slot.get();
        (*slot).clone()
    }
}

/// 不阻塞语义声明（与 F0041 同纪律——回环核失败只入报告不中止引导）。
pub const NON_BLOCKING_NOTE: &str =
    "回环核不过时报告列出对拍不齐用例且不阻塞引导——对拍面是报告不是中止";

// ---------------------------------------------------------------------------
// 十二、深化区：翻译账注渲染 / 审计窗快照 / 流控画像 / 配置账 kv
// （全部纯函数零 IO——深化区不新增诊断码，复用 0x99xx 闭集）
// ---------------------------------------------------------------------------

/// 翻译账注渲染（逐条一行——账注可读面；账注与翻译一一对应可对拍）。
pub fn trans_kv_lines(ledger: &TransLedger) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("trans.total={}", ledger.total()));
    out.push(format!("trans.kept={}", ledger.entries().len()));
    out.push(format!("trans.truncated={}", ledger.truncated()));
    let mut i = 0usize;
    while i < ledger.entries().len() {
        let e = &ledger.entries()[i];
        out.push(format!(
            "trans.{}=tick:{} at:{} from:0x{:02X} to:0x{:02X}0x{:02X}",
            e.seq, e.tick, e.at, e.from, e.to[0], e.to[1]
        ));
        i += 1;
    }
    out
}

/// 翻译账注行数闸（3 + 存留条数——渲染确定性对拍锚）。
pub fn trans_kv_expected_len(ledger: &TransLedger) -> usize {
    3 + ledger.entries().len()
}

/// 审计窗快照（op 分布画像——闭集六态计数 + 滚动账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditCensus {
    /// putchar 条数。
    pub putchar: u32,
    /// write_str 条数。
    pub write_str: u32,
    /// write_raw 条数。
    pub write_raw: u32,
    /// write_line 条数。
    pub write_line: u32,
    /// flush 条数。
    pub flush: u32,
    /// summary 条数。
    pub summary: u32,
    /// 失败条数。
    pub failed: u32,
    /// 窗内条目总数。
    pub total: u32,
    /// 覆盖次数。
    pub wrapped: u64,
}

/// 审计窗普查单点（窗内条目闭集计数——op 维度画像）。
pub fn audit_census(win: &AuditWindow) -> AuditCensus {
    let mut c = AuditCensus {
        putchar: 0,
        write_str: 0,
        write_raw: 0,
        write_line: 0,
        flush: 0,
        summary: 0,
        failed: 0,
        total: 0,
        wrapped: win.wrapped(),
    };
    let entries = win.entries();
    let mut i = 0usize;
    while i < entries.len() {
        let e = entries[i];
        match e.op {
            OpKind::PutChar => c.putchar += 1,
            OpKind::WriteStr => c.write_str += 1,
            OpKind::WriteRaw => c.write_raw += 1,
            OpKind::WriteLine => c.write_line += 1,
            OpKind::Flush => c.flush += 1,
            OpKind::Summary => c.summary += 1,
        }
        if !e.ok {
            c.failed += 1;
        }
        c.total += 1;
        i += 1;
    }
    c
}

/// 流控画像（状态/驻留/迁移账——快照面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowProfile {
    /// 当前状态名。
    pub state: &'static str,
    /// 迁移总数。
    pub transitions: u64,
    /// 累计阻塞 tick。
    pub blocked_total: u64,
    /// 恢复次数。
    pub recoveries: u64,
    /// 环水位。
    pub level: usize,
    /// 环容量。
    pub cap: usize,
    /// 水位形态名。
    pub wm: &'static str,
}

/// 流控画像单点（引擎快照——快照不改态）。
pub fn flow_profile(eng: &SerialOut) -> FlowProfile {
    FlowProfile {
        state: eng.flow.state().name(),
        transitions: eng.flow.transitions(),
        blocked_total: eng.flow.blocked_total(),
        recoveries: eng.flow.recoveries(),
        level: eng.ring.level(),
        cap: eng.ring.capacity(),
        wm: match eng.ring.wm_state() {
            WmState::BelowLo => "below-lo",
            WmState::Between => "between",
            WmState::AtOrAboveHi => "at-or-above-hi",
            WmState::Full => "full",
        },
    }
}

/// 配置账 kv 渲染（提交条目逐行——配置审计面）。
pub fn config_kv_lines(ledger: &ConfigLedger) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("cfg.frozen={}", ledger.frozen()));
    out.push(format!("cfg.entries={}", ledger.entries().len()));
    let mut i = 0usize;
    while i < ledger.entries().len() {
        let e = &ledger.entries()[i];
        let parity = match e.plan.parity {
            Parity::None => "none",
            Parity::Odd => "odd",
            Parity::Even => "even",
        };
        out.push(format!(
            "cfg.{}=tick:{} baud:{} div:{} data:{} stop:{} parity:{} lcr:0x{:02X}",
            e.seq, e.tick, e.plan.baud, e.plan.divisor, e.plan.data_bits, e.plan.stop_bits, parity, e.plan.lcr
        ));
        i += 1;
    }
    out
}

/// TX 环账 kv 渲染（水位/丢弃显性面——环账审计）。
pub fn ring_kv_lines(ring: &TxRing) -> Vec<String> {
    let wm = match ring.wm_state() {
        WmState::BelowLo => "below-lo",
        WmState::Between => "between",
        WmState::AtOrAboveHi => "at-or-above-hi",
        WmState::Full => "full",
    };
    vec![
        format!("ring.cap={}", ring.capacity()),
        format!("ring.level={}", ring.level()),
        format!("ring.hi_wm={}", ring.high_watermark()),
        format!("ring.lo_wm={}", ring.low_watermark()),
        format!("ring.drops={}", ring.drops()),
        format!("ring.drop_bytes={}", ring.drop_bytes()),
        format!("ring.wm={}", wm),
    ]
}

/// 报告 kv 渲染（总报告 → 行组——与 F0041 报告 kv 同构）。
pub fn report_kv_lines(r: &VerifierReport) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("# varix-serialout-verify".to_string());
    out.push(format!("passed={}/{}", r.passed, r.cases.len()));
    out.push(format!("all_ok={}", r.all_ok));
    out.push(format!("summary_check={}", r.summary_check));
    let mut i = 0usize;
    while i < r.cases.len() {
        let c = &r.cases[i];
        let err_part = match c.err {
            Some(code) => format!(" err=0x{:04X}", code.code()),
            None => String::new(),
        };
        out.push(format!(
            "case.{}={} ok={} three_way={} trans={}{}",
            c.id, c.name, c.ok, c.three_way, c.translations, err_part
        ));
        i += 1;
    }
    out
}

/// 报告 kv 行数闸（4 + 用例数——渲染确定性对拍锚）。
pub fn report_kv_expected_len(r: &VerifierReport) -> usize {
    4 + r.cases.len()
}

// ---------------------------------------------------------------------------
// 十三、守恒对账独立面（摘要行守恒的双路验证——账面 vs 逐字节重放）
// ---------------------------------------------------------------------------

/// 逐字节重放守恒核（不看引擎账面——纯字节重算恒等式）：
/// 输入经过滤/翻译/转义膨胀闭式重演期望字节数，与账面 produced 对拍。
/// 口径独立——账面漏账在此显形。
pub fn replay_conservation(
    input_len: usize,
    filtered: u64,
    translations: u64,
    escapes: u64,
    produced: u64,
    dropped: u64,
) -> bool {
    let accepted = input_len as u64 - filtered;
    let expand = translations + escapes * 3;
    accepted + expand == produced + dropped
}

/// 守恒闸入口（账面守恒 + 重放守恒双闸——任一破即 CONSERVATION_BROKEN）。
pub fn conservation_gate_full(stats: &SerialStats, input_len: usize) -> Result<(), SoCode> {
    stats.conservation_gate()?;
    if replay_conservation(
        input_len,
        stats.filtered,
        stats.translations,
        stats.escapes,
        stats.produced,
        stats.dropped,
    ) {
        Ok(())
    } else {
        Err(SoCode::CONSERVATION_BROKEN)
    }
}

// ---------------------------------------------------------------------------
// 十四、转义模式深化（\xNN 记法的批量渲染面——独立于引擎的重演对拍）
// ---------------------------------------------------------------------------

/// 批量转义渲染（字节串 → 转义后字节串——独立于引擎的重演面，
/// 可与引擎 escape 开态输出逐字节对拍）。
pub fn escape_render(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if is_invisible_byte(b) {
            let e = escape_expand(b);
            out.push(e[0]);
            out.push(e[1]);
            out.push(e[2]);
            out.push(e[3]);
        } else {
            out.push(b);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 十五、边界深化：行限恰界矩阵 / 水位配置闭检 / 过滤翻译孪生重演
// （深化区纯函数——复用诊断码闭集不占新段）
// ---------------------------------------------------------------------------

/// 行限恰界判定（行内已 n 字节，来 b——闭式三态）。
///
/// - `Ok(true)`：接受（含行终止符恰界——9 号用例面）。
/// - `Err(LINE_TOO_LONG)`：超界（6 号用例面）。
/// - `Err(WATERMARK_INVALID)`：At(0) 非法配置。
pub fn line_limit_gate(line_max: LineLimit, line_len: usize, b: u8) -> Result<bool, SoCode> {
    match line_max {
        LineLimit::Off => Ok(true),
        LineLimit::At(n) => {
            if n == 0 {
                return Err(SoCode::WATERMARK_INVALID);
            }
            if b == b'\r' || b == b'\n' {
                return Ok(true);
            }
            if line_len >= n {
                Err(SoCode::LINE_TOO_LONG)
            } else {
                Ok(true)
            }
        }
    }
}

/// 水位配置闭检（独立于 TxRing::new 的纯函数面——配置前置闸）。
pub const fn watermark_ok(cap: usize, hi: usize, lo: usize) -> bool {
    cap > 0 && hi <= cap && lo <= hi && hi > 0
}

/// 过滤矩阵独立重演（引擎 filter 的纯函数孪生——对拍面）。
pub fn filter_replay(bytes: &[u8], mode: FilterMode) -> Result<Vec<u8>, SoCode> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if is_invisible_byte(b) && mode != FilterMode::Pass {
            if mode == FilterMode::Reject {
                return Err(SoCode::CTRL_REJECTED);
            }
            i += 1;
            continue;
        }
        out.push(b);
        i += 1;
    }
    Ok(out)
}

/// 翻译矩阵独立重演（引擎 translate 的批量孪生——对拍面）。
pub fn translate_replay(bytes: &[u8], mode: CrLfMode) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let (b0, b1, n) = translate_byte(mode, bytes[i]);
        out.push(b0);
        if n == 2 {
            out.push(b1);
        }
        i += 1;
    }
    out
}

/// 管道孪生重演（过滤→翻译→转义全序——与引擎 feed 管道逐字节同构，
/// 孪生面与引擎对拍即字节序一致性双路闭证）。
pub fn pipeline_replay(bytes: &[u8], cfg: &SerialConfig) -> Result<Vec<u8>, SoCode> {
    let f = filter_replay(bytes, cfg.filter)?;
    let t = translate_replay(&f, if cfg.crlf { CrLfMode::LfToCrLf } else { CrLfMode::PassThrough });
    if cfg.escape {
        Ok(escape_render(&t))
    } else {
        Ok(t)
    }
}
