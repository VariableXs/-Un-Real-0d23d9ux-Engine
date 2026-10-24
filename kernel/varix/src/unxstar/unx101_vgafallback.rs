//! unx101_vgafallback — UNX-F0101 VGA 文本模式后备通道（深化版）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0101 · UNX-A1-B05）：帧簿不可用
//! 时自动落 VGA 文本通道输出，切换零丢行。
//!
//! **分层深化声明**：零 IO——VGA 80×25 文本缓冲建模为纯视图（字符 +
//! 属性字节对），真实端口写由引导主链执行；本册做**切换的语义学**：
//! 帧簿健康判定、落通道状态机、零丢行行账守恒、滚动不丢行账面。
//!
//! **零丢行语义**（判据核心）：行账三分——产出（produced，上游写入
//! 行缓冲）/ 投递（delivered，通道刷出）/ 丢弃（dropped，永远是 0——
//! 守恒式 produced == delivered，任何例外即 DroppedLine 立案）。切换
//! 时帧簿未刷出的积压行**先入 VGA 排队再继续新行**——积压补发不是
//! 截断，行号连续无跳（行号审计面：delivered 行号序列必须是 1..N
//! 连续，跳号即事故）。
//!
//! **切换状态机**（三态单向为主）：Active（帧簿在岗）/ Degraded（已落
//! VGA）/ Restored（帧簿恢复——**不自动切回**，防抖动：恢复只是状态
//! 登记，切回须显性指令——半吊子的双向自动切换是行序混乱之源）。
//!
//! **滚动模型**：VGA 25 行缓冲满即滚动（滚出行已投递——显示面丢失
//! 但投递面完成，账本不记丢）；滚动计数显性（滚动过快 = 输出过载
//! 信号，日志采样降频建议面）。
//!
//! 诊断枚举 VgaCode 十六码（0x31 段 0x3100..=0x310F 互异 +
//! [`segment_guard`]）。no_std 零 unwrap 零 panic 面。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX101_VGA_VERSION: &str = "unx101-vgafallback-v1";

/// 诊断段基值（0x31 段——UNX 域 0x2E/0x2F/0x30 已占）。
pub const SEG_BASE: u16 = 0x3100;

/// VGA 文本列数。
pub const VGA_COLS: usize = 80;

/// VGA 文本行数。
pub const VGA_ROWS: usize = 25;

/// 行缓冲容量（切换积压排队上限——策略值）。
pub const LINE_BUFFER_CAP: usize = 256;

/// 告警账容量。
pub const ALERT_CAP: usize = 24;

// ---------------------------------------------------------------------------
// 诊断码（VgaCode 十六码 · 0x31 段互异）
// ---------------------------------------------------------------------------

/// 诊断码（VGA 后备通道——0x31 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VgaCode {
    /// 0x3100 行账守恒破坏（产出 ≠ 投递——丢行事故）。
    DroppedLine,
    /// 0x3101 行号跳号（投递序列不连续——切换补发漏行）。
    LineGap,
    /// 0x3102 行缓冲满（积压超 LINE_BUFFER_CAP——拒收计数）。
    LineBufferFull,
    /// 0x3103 双通道皆不可用（帧簿坏 + VGA 未就绪——输出黑洞）。
    BothChannelsDown,
    /// 0x3104 非法切换（Restored 态又收 Degraded 请求——状态机违例）。
    IllegalTransition,
    /// 0x3105 VGA 就绪位未置（通道未初始化硬落——显性拒）。
    VgaNotReady,
    /// 0x3106 行超宽（> 80 列——折行或截断策略面）。
    LineTooWide,
    /// 0x3107 属性色越界（前景/背景不在 16 色闭集）。
    ColorOutOfRange,
    /// 0x3108 空行刷（空串投递——浪费通道带宽显性记账）。
    EmptyLineFlush,
    /// 0x3109 滚动过载（单位时间滚动超阈——输出过载信号）。
    ScrollOverload,
    /// 0x310A 告警账满（旧账保留）。
    AlertFull,
    /// 0x310B 切回未授权（Restored 登记≠切回——须显性指令）。
    UnauthoredSwitchback,
    /// 0x310C 快照指纹不符（往返破坏）。
    SnapshotFingerprint,
    /// 0x310D 折行残行（超宽行折行后残尾空——账面注记）。
    WrapResidue,
    /// 0x310E 光标越界（行 ≥ 25 或列 ≥ 80）。
    CursorOutOfRange,
    /// 0x310F 段守恒对账失败。
    SegmentGuard,
}

impl VgaCode {
    /// 十六码闭集。
    pub const ALL: [VgaCode; 16] = [
        VgaCode::DroppedLine,
        VgaCode::LineGap,
        VgaCode::LineBufferFull,
        VgaCode::BothChannelsDown,
        VgaCode::IllegalTransition,
        VgaCode::VgaNotReady,
        VgaCode::LineTooWide,
        VgaCode::ColorOutOfRange,
        VgaCode::EmptyLineFlush,
        VgaCode::ScrollOverload,
        VgaCode::AlertFull,
        VgaCode::UnauthoredSwitchback,
        VgaCode::SnapshotFingerprint,
        VgaCode::WrapResidue,
        VgaCode::CursorOutOfRange,
        VgaCode::SegmentGuard,
    ];

    /// 码值（连续）。
    pub const fn code(self) -> u16 {
        match self {
            VgaCode::DroppedLine => 0x3100,
            VgaCode::LineGap => 0x3101,
            VgaCode::LineBufferFull => 0x3102,
            VgaCode::BothChannelsDown => 0x3103,
            VgaCode::IllegalTransition => 0x3104,
            VgaCode::VgaNotReady => 0x3105,
            VgaCode::LineTooWide => 0x3106,
            VgaCode::ColorOutOfRange => 0x3107,
            VgaCode::EmptyLineFlush => 0x3108,
            VgaCode::ScrollOverload => 0x3109,
            VgaCode::AlertFull => 0x310A,
            VgaCode::UnauthoredSwitchback => 0x310B,
            VgaCode::SnapshotFingerprint => 0x310C,
            VgaCode::WrapResidue => 0x310D,
            VgaCode::CursorOutOfRange => 0x310E,
            VgaCode::SegmentGuard => 0x310F,
        }
    }

    /// 人读诊断（互异非空）。
    pub const fn detail(self) -> &'static str {
        match self {
            VgaCode::DroppedLine => {
                "行账守恒破坏：产出须等于投递——丢行是后备通道最高级事故，建议审计切换窗口"
            }
            VgaCode::LineGap => {
                "行号跳号：投递序列须 1..N 连续——跳号即补发漏行，建议核对积压排队"
            }
            VgaCode::LineBufferFull => {
                "行缓冲满：积压超容量上限——新行拒收计数，建议降频输出或扩容评审"
            }
            VgaCode::BothChannelsDown => {
                "双通道皆不可用：输出黑洞——建议 VGA 就绪位核查与帧簿故障面排查"
            }
            VgaCode::IllegalTransition => {
                "非法切换：状态机违例迁移，建议按迁移表显性枚举"
            }
            VgaCode::VgaNotReady => {
                "VGA 就绪位未置：通道未初始化硬落是赌运气，建议初始化后再投递"
            }
            VgaCode::LineTooWide => {
                "行超宽：超过 80 列——折行或截断须策略显性，建议折行保全行文"
            }
            VgaCode::ColorOutOfRange => {
                "属性色越界：前景背景须在 16 色闭集，建议钳制后投递"
            }
            VgaCode::EmptyLineFlush => {
                "空行刷：空串投递浪费通道带宽，建议跳过并记账"
            }
            VgaCode::ScrollOverload => {
                "滚动过载：单位时间滚动超阈——输出过载，建议日志降频"
            }
            VgaCode::AlertFull => {
                "告警账满：旧账保留新告警拒入，建议归档"
            }
            VgaCode::UnauthoredSwitchback => {
                "切回未授权：Restored 登记不等于切回——须显性指令防抖动"
            }
            VgaCode::SnapshotFingerprint => {
                "快照指纹不符：往返恒等破坏，建议排查序列化格式漂移"
            }
            VgaCode::WrapResidue => {
                "折行残行：超宽行折行后残尾——账面注记不丢字"
            }
            VgaCode::CursorOutOfRange => {
                "光标越界：行须小于 25 列须小于 80，建议钳制"
            }
            VgaCode::SegmentGuard => {
                "段守恒对账失败：十六码须连续互异界内——段位事故立即立案"
            }
        }
    }
}

/// **段守恒对账**。
pub const fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < VgaCode::ALL.len() {
        let c = VgaCode::ALL[i].code();
        if c < SEG_BASE || c != SEG_BASE + i as u16 {
            return false;
        }
        let mut j = i + 1;
        while j < VgaCode::ALL.len() {
            if VgaCode::ALL[i].code() == VgaCode::ALL[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// VGA 文本面（16 色闭集 + 属性字节 + 80×25 缓冲视图）
// ---------------------------------------------------------------------------

/// VGA 16 色（IRGB 四比特——前景/背景共用闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VgaColor {
    /// 0 黑。
    Black,
    /// 1 蓝。
    Blue,
    /// 2 绿。
    Green,
    /// 3 青。
    Cyan,
    /// 4 红。
    Red,
    /// 5 品红。
    Magenta,
    /// 6 棕。
    Brown,
    /// 7 浅灰（默认前景）。
    LightGray,
    /// 8 深灰。
    DarkGray,
    /// 9 亮蓝。
    LightBlue,
    /// 10 亮绿。
    LightGreen,
    /// 11 亮青。
    LightCyan,
    /// 12 亮红。
    LightRed,
    /// 13 亮品红。
    LightMagenta,
    /// 14 黄。
    Yellow,
    /// 15 白。
    White,
}

impl VgaColor {
    /// 16 色闭集。
    pub const ALL: [VgaColor; 16] = [
        VgaColor::Black,
        VgaColor::Blue,
        VgaColor::Green,
        VgaColor::Cyan,
        VgaColor::Red,
        VgaColor::Magenta,
        VgaColor::Brown,
        VgaColor::LightGray,
        VgaColor::DarkGray,
        VgaColor::LightBlue,
        VgaColor::LightGreen,
        VgaColor::LightCyan,
        VgaColor::LightRed,
        VgaColor::LightMagenta,
        VgaColor::Yellow,
        VgaColor::White,
    ];

    /// 四比特值。
    pub const fn bits(self) -> u8 {
        match self {
            VgaColor::Black => 0,
            VgaColor::Blue => 1,
            VgaColor::Green => 2,
            VgaColor::Cyan => 3,
            VgaColor::Red => 4,
            VgaColor::Magenta => 5,
            VgaColor::Brown => 6,
            VgaColor::LightGray => 7,
            VgaColor::DarkGray => 8,
            VgaColor::LightBlue => 9,
            VgaColor::LightGreen => 10,
            VgaColor::LightCyan => 11,
            VgaColor::LightRed => 12,
            VgaColor::LightMagenta => 13,
            VgaColor::Yellow => 14,
            VgaColor::White => 15,
        }
    }

    /// 比特值反解（越界 None——ColorOutOfRange 判定源）。
    pub const fn from_bits(b: u8) -> Option<VgaColor> {
        if b > 15 {
            return None;
        }
        let mut i = 0usize;
        while i < 16 {
            if VgaColor::ALL[i].bits() == b {
                return Some(VgaColor::ALL[i]);
            }
            i += 1;
        }
        None
    }
}

/// 文本属性字节（前景低 4 位/背景高 4 位——VGA 硬件布局）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Attr(pub u8);

impl Attr {
    /// 默认属性（浅灰前景黑背景）。
    pub const DEFAULT: Attr = Attr(0x07);

    /// 构造（色越界 None）。
    pub const fn new(fg: VgaColor, bg: VgaColor) -> Option<Attr> {
        Some(Attr(fg.bits() | (bg.bits() << 4)))
    }

    /// 裸构造（越界值照样打包——ColorOutOfRange 由调用侧闸）。
    pub const fn raw(fg_bits: u8, bg_bits: u8) -> Attr {
        Attr(fg_bits | (bg_bits << 4))
    }

    /// 前景四比特。
    pub const fn fg(self) -> u8 {
        self.0 & 0x0F
    }

    /// 背景四比特。
    pub const fn bg(self) -> u8 {
        self.0 >> 4
    }

    /// 合法性（双色均须在 16 色闭集——四比特天然 ≤ 15 恒真，保留对账面）。
    pub const fn valid(self) -> bool {
        VgaColor::from_bits(self.fg()).is_some() && VgaColor::from_bits(self.bg()).is_some()
    }
}

// ---------------------------------------------------------------------------
// 通道状态机（Active/Degraded/Restored——迁移表全枚举）
// ---------------------------------------------------------------------------

/// 输出通道状态三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelState {
    /// 帧簿在岗。
    Active,
    /// 已落 VGA 文本。
    Degraded,
    /// 帧簿恢复登记（不自动切回）。
    Restored,
}

impl ChannelState {
    /// 三闭集。
    pub const ALL: [ChannelState; 3] = [
        ChannelState::Active,
        ChannelState::Degraded,
        ChannelState::Restored,
    ];

    /// 中文名（互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            ChannelState::Active => "帧簿在岗",
            ChannelState::Degraded => "已落文本",
            ChannelState::Restored => "恢复登记",
        }
    }
}

/// 切换事件（上游报告驱动）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchEvent {
    /// 帧簿故障（触发落 VGA）。
    FramebookFail,
    /// 帧簿恢复（只登记不切回）。
    FramebookRecover,
    /// 显性切回指令（唯一合法切回路径）。
    ExplicitSwitchback,
}

/// 迁移判定（表全枚举——合法 Some/违例 None，IllegalTransition 判定源）。
pub const fn transition(cur: ChannelState, ev: SwitchEvent) -> Option<ChannelState> {
    match cur {
        ChannelState::Active => match ev {
            SwitchEvent::FramebookFail => Some(ChannelState::Degraded),
            SwitchEvent::FramebookRecover => Some(ChannelState::Active), // 幂等
            SwitchEvent::ExplicitSwitchback => Some(ChannelState::Active), // 幂等
        },
        ChannelState::Degraded => match ev {
            SwitchEvent::FramebookFail => Some(ChannelState::Degraded), // 幂等
            SwitchEvent::FramebookRecover => Some(ChannelState::Restored),
            SwitchEvent::ExplicitSwitchback => Some(ChannelState::Active),
        },
        ChannelState::Restored => match ev {
            SwitchEvent::FramebookFail => Some(ChannelState::Degraded), // 再故障再落
            SwitchEvent::FramebookRecover => Some(ChannelState::Restored), // 幂等
            SwitchEvent::ExplicitSwitchback => Some(ChannelState::Active),
        },
    }
}

// ---------------------------------------------------------------------------
// 行账本（零丢行守恒 + 行号连续审计）
// ---------------------------------------------------------------------------

/// 行账本（判据"零丢行"的本体）。
#[derive(Clone, Copy, Debug)]
pub struct LineLedger {
    /// 产出行数（上游写入）。
    pub produced: u64,
    /// 投递行数（通道刷出）。
    pub delivered: u64,
    /// 丢弃行数（恒 0——非 0 即 DroppedLine 立案）。
    pub dropped: u64,
    /// 积压行数（排队未刷——produced - delivered）。
    pub backlog: u64,
    /// 滚动行数（VGA 显示面滚出——投递面已完成）。
    pub scrolled: u64,
    /// 下一个应投递行号（1 起——连续审计锚）。
    pub next_line_no: u64,
}

impl LineLedger {
    /// 产出一行（行号发放——连续性锚递增）。
    pub const fn produce(&mut self) -> u64 {
        let no = self.next_line_no;
        self.next_line_no += 1;
        self.produced += 1;
        self.backlog += 1;
        no
    }

    /// 投递一行（积压消耗——行号须恰为下一应投号，跳号 None）。
    /// 应投号单点推导 = delivered + 1（行号 1 起连续投递）——与产出行号
    /// 游标 next_line_no 分离：共用一个游标会让 produce 的递增吞掉
    /// deliver 的审计锚（首行号永远 Gap——游标双语义冲突）。
    pub fn deliver(&mut self, line_no: u64) -> Result<(), VgaCode> {
        if line_no != self.delivered + 1 {
            return Err(VgaCode::LineGap);
        }
        if self.backlog == 0 {
            return Err(VgaCode::DroppedLine);
        }
        self.delivered += 1;
        self.backlog -= 1;
        Ok(())
    }

    /// 滚动一行（投递面已完成——显示面滚出计数）。
    pub const fn scroll_one(&mut self) {
        self.scrolled += 1;
    }

    /// 守恒（产出 == 投递 + 积压 且丢弃恒 0）。
    pub const fn conserved(&self) -> bool {
        self.dropped == 0 && self.produced == self.delivered + self.backlog
    }
}

impl Default for LineLedger {
    /// 行号 1 起（连续审计锚——derive 零值会与注释语义漂移）。
    fn default() -> Self {
        LineLedger {
            produced: 0,
            delivered: 0,
            dropped: 0,
            backlog: 0,
            scrolled: 0,
            next_line_no: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// VGA 文本缓冲视图（80×25 纯函数——零端口零 IO）
// ---------------------------------------------------------------------------

/// VGA 文本缓冲视图（字符+属性对——零 IO 建模，端口写由主链执行）。
pub struct VgaTextBuffer {
    /// 80×25 单元（低字节字符/高字节属性——双 Vec 平铺）。
    chars: Vec<u8>,
    attrs: Vec<u8>,
    /// 光标（行, 列）。
    pub cur_row: usize,
    pub cur_col: usize,
    /// 当前属性。
    pub attr: Attr,
    /// 滚动计数。
    pub scrolls: u64,
}

impl VgaTextBuffer {
    /// 空缓冲（默认属性浅灰黑底）。
    pub fn new() -> VgaTextBuffer {
        let n = VGA_COLS * VGA_ROWS;
        let mut chars = Vec::new();
        let mut attrs = Vec::new();
        let mut i = 0usize;
        while i < n {
            chars.push(b' ');
            attrs.push(Attr::DEFAULT.0);
            i += 1;
        }
        VgaTextBuffer {
            chars,
            attrs,
            cur_row: 0,
            cur_col: 0,
            attr: Attr::DEFAULT,
            scrolls: 0,
        }
    }

    /// 光标越界钳制校验（CursorOutOfRange 判定源）。
    pub const fn cursor_ok(row: usize, col: usize) -> bool {
        row < VGA_ROWS && col < VGA_COLS
    }

    /// 写一字符（光标推进；行满换行；屏满滚动——滚动计数显性）。
    pub fn put_char(&mut self, c: u8) {
        if c == b'\n' {
            self.newline();
            return;
        }
        self.chars[self.cur_row * VGA_COLS + self.cur_col] = c;
        self.attrs[self.cur_row * VGA_COLS + self.cur_col] = self.attr.0;
        self.cur_col += 1;
        if self.cur_col >= VGA_COLS {
            self.newline();
        }
    }

    /// 换行（行满滚动——顶部行滚出）。
    fn newline(&mut self) {
        self.cur_col = 0;
        self.cur_row += 1;
        if self.cur_row >= VGA_ROWS {
            self.scroll_up();
            self.cur_row = VGA_ROWS - 1;
        }
    }

    /// 滚动一行（顶行丢失——显示面；调用侧把 scrolled 计入账本）。
    fn scroll_up(&mut self) {
        let mut col = 0usize;
        while col < VGA_COLS {
            let mut row = 0usize;
            while row < VGA_ROWS - 1 {
                let src = (row + 1) * VGA_COLS + col;
                let dst = row * VGA_COLS + col;
                self.chars[dst] = self.chars[src];
                self.attrs[dst] = self.attrs[src];
                row += 1;
            }
            // 底行清空。
            let bot = (VGA_ROWS - 1) * VGA_COLS + col;
            self.chars[bot] = b' ';
            self.attrs[bot] = Attr::DEFAULT.0;
            col += 1;
        }
        self.scrolls += 1;
    }

    /// 单元读回（越界 None）。
    pub fn cell(&self, row: usize, col: usize) -> Option<(u8, u8)> {
        if !Self::cursor_ok(row, col) {
            return None;
        }
        Some((self.chars[row * VGA_COLS + col], self.attrs[row * VGA_COLS + col]))
    }

    /// 一行文本读回（字符面——空格截尾）。
    pub fn row_text(&self, row: usize) -> Option<String> {
        if row >= VGA_ROWS {
            return None;
        }
        let mut s = String::new();
        let mut col = 0usize;
        let mut last_nonspace = 0usize;
        while col < VGA_COLS {
            let (c, _) = self.cell(row, col)?;
            if c != b' ' {
                last_nonspace = col + 1;
            }
            col += 1;
        }
        let mut col2 = 0usize;
        while col2 < last_nonspace {
            let (c, _) = self.cell(row, col2)?;
            if c >= 0x20 && c < 0x7F {
                s.push(c as char);
            } else {
                s.push('.');
            }
            col2 += 1;
        }
        Some(s)
    }
}

impl Default for VgaTextBuffer {
    fn default() -> Self {
        VgaTextBuffer::new()
    }
}

// ---------------------------------------------------------------------------
// 行缓冲（切换积压排队——零丢行的机制面）
// ---------------------------------------------------------------------------

/// 行缓冲（切换时积压排队——容量界 LINE_BUFFER_CAP）。
pub struct LineQueue {
    /// (行号, 文本) 队列。
    items: Vec<(u64, String)>,
    /// 拒收计数（容量满）。
    pub rejected_full: u32,
}

impl LineQueue {
    /// 空队列。
    pub fn new() -> LineQueue {
        LineQueue {
            items: Vec::new(),
            rejected_full: 0,
        }
    }

    /// 入队（FIFO——积压补发序）。
    pub fn enqueue(&mut self, line_no: u64, text: &str) -> Result<(), VgaCode> {
        if self.items.len() >= LINE_BUFFER_CAP {
            self.rejected_full += 1;
            return Err(VgaCode::LineBufferFull);
        }
        self.items.push((line_no, String::from(text)));
        Ok(())
    }

    /// 出队（FIFO 头）。
    pub fn dequeue(&mut self) -> Option<(u64, String)> {
        if self.items.is_empty() {
            return None;
        }
        Some(self.items.remove(0))
    }

    /// 在队数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for LineQueue {
    fn default() -> Self {
        LineQueue::new()
    }
}

// ---------------------------------------------------------------------------
// 后备通道主编排（状态机 + 行账 + 队列 + VGA 缓冲联动）
// ---------------------------------------------------------------------------

/// 后备通道（判据本体的编排面——帧簿不可用自动落 + 零丢行守恒）。
pub struct VgaFallback {
    /// 通道状态。
    pub state: ChannelState,
    /// 帧簿可用性（上游健康位）。
    pub framebook_ok: bool,
    /// VGA 就绪位。
    pub vga_ready: bool,
    /// 行账本。
    pub ledger: LineLedger,
    /// 积压队列。
    pub queue: LineQueue,
    /// VGA 缓冲视图。
    pub screen: VgaTextBuffer,
    /// 告警账。
    pub alerts: AlertList,
    /// 投递行号序列（连续审计——最近 8 行环形记忆）。
    recent: Vec<u64>,
}

/// 告警账（轻量版——容量 ALERT_CAP）。
pub struct AlertList {
    /// 行（码|文）。
    pub rows: Vec<String>,
    /// 逐码计数（十六槽）。
    pub per_code: [u32; 16],
    /// 拒入计数。
    pub rejected_full: u32,
}

impl AlertList {
    /// 空账。
    pub fn new() -> AlertList {
        AlertList {
            rows: Vec::new(),
            per_code: [0; 16],
            rejected_full: 0,
        }
    }

    /// 追加。
    pub fn push(&mut self, c: VgaCode, note: &str) {
        if self.rows.len() >= ALERT_CAP {
            self.rejected_full += 1;
            return;
        }
        let idx = (c.code() - SEG_BASE) as usize;
        self.per_code[idx] += 1;
        let mut line = String::from(note);
        line.push_str(" | ");
        line.push_str(c.detail());
        self.rows.push(alloc::format!("{:#06x}|{}", c.code(), line));
    }

    /// 在账数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

impl Default for AlertList {
    fn default() -> Self {
        AlertList::new()
    }
}

impl VgaFallback {
    /// 构造（帧簿初态 + VGA 就绪位注入）。
    pub fn new(framebook_ok: bool, vga_ready: bool) -> VgaFallback {
        let state = if framebook_ok {
            ChannelState::Active
        } else if vga_ready {
            ChannelState::Degraded
        } else {
            ChannelState::Active // 双通道皆坏也进 Active——投递时 BothChannelsDown 显性
        };
        VgaFallback {
            state,
            framebook_ok,
            vga_ready,
            ledger: LineLedger::default(),
            queue: LineQueue::new(),
            screen: VgaTextBuffer::new(),
            alerts: AlertList::new(),
            recent: Vec::new(),
        }
    }

    /// **切换事件处理**（迁移表全枚举——违例显性拒不静默）。
    pub fn on_event(&mut self, ev: SwitchEvent) -> Result<ChannelState, VgaCode> {
        match transition(self.state, ev) {
            Some(next) => {
                if ev == SwitchEvent::FramebookFail {
                    self.framebook_ok = false;
                    if !self.vga_ready {
                        self.alerts.push(VgaCode::BothChannelsDown, "双通道皆坏");
                    }
                }
                if ev == SwitchEvent::FramebookRecover {
                    self.framebook_ok = true;
                    // Restored 只登记——不自动切回（防抖动纪律）。
                }
                if ev == SwitchEvent::ExplicitSwitchback {
                    if !self.framebook_ok {
                        self.alerts.push(VgaCode::UnauthoredSwitchback, "帧簿仍坏切回");
                        return Err(VgaCode::UnauthoredSwitchback);
                    }
                }
                self.state = next;
                Ok(next)
            }
            None => {
                self.alerts.push(VgaCode::IllegalTransition, "迁移违例");
                Err(VgaCode::IllegalTransition)
            }
        }
    }

    /// **产出一行**（上游写入——积压入队；超宽折行保全行文；空行跳过记账）。
    pub fn produce_line(&mut self, text: &str) -> Result<u64, VgaCode> {
        if text.is_empty() {
            self.alerts.push(VgaCode::EmptyLineFlush, "空行跳过");
            return Err(VgaCode::EmptyLineFlush);
        }
        let line_no = self.ledger.produce();
        // 折行（>80 列按 80 折——全行文保全，残尾注记）。
        let bytes = text.as_bytes();
        let mut off = 0usize;
        let mut first = true;
        while off < bytes.len() || first {
            let end = if off + VGA_COLS <= bytes.len() {
                off + VGA_COLS
            } else {
                bytes.len()
            };
            let chunk = &text[off..end.min(text.len())];
            let q = self.queue.enqueue(line_no, chunk);
            if q.is_err() {
                // 容量满——丢弃？不：拒绝产出（行号已发）→ 记 dropped 并立案。
                self.ledger.dropped += 1;
                self.alerts.push(VgaCode::LineBufferFull, "积压满拒收");
                return Err(VgaCode::LineBufferFull);
            }
            if off + VGA_COLS < bytes.len() {
                self.alerts.push(VgaCode::WrapResidue, "折行续段");
            }
            off += VGA_COLS;
            first = false;
            if off >= bytes.len() {
                break;
            }
        }
        Ok(line_no)
    }

    /// **刷出积压**（投递面——FIFO 逐行入 VGA 缓冲；VGA 未就绪显性拒；
    /// 零丢行守恒在账本面自证）。
    pub fn flush_backlog(&mut self) -> Result<usize, VgaCode> {
        if !self.vga_ready {
            self.alerts.push(VgaCode::VgaNotReady, "VGA 未就绪");
            return Err(VgaCode::VgaNotReady);
        }
        let mut n = 0usize;
        while let Some((line_no, text)) = self.queue.dequeue() {
            // 行号连续性（账本面审计）。
            if self.ledger.deliver(line_no).is_err() {
                self.alerts.push(VgaCode::LineGap, "行号跳号");
                return Err(VgaCode::LineGap);
            }
            // 写屏（逐字符——滚动计数入账）。
            let b = text.as_bytes();
            let mut i = 0usize;
            let before = self.screen.scrolls;
            while i < b.len() {
                self.screen.put_char(b[i]);
                i += 1;
            }
            self.screen.put_char(b'\n');
            let gained = self.screen.scrolls - before;
            let mut k = 0u64;
            while k < gained {
                self.ledger.scroll_one();
                k += 1;
            }
            // 最近行号环形记忆（8 行）。
            self.recent.push(line_no);
            if self.recent.len() > 8 {
                self.recent.remove(0);
            }
            n += 1;
        }
        Ok(n)
    }

    /// 最近投递行号序列（审计面——连续性）。
    pub fn recent_lines(&self) -> &[u64] {
        &self.recent
    }

    /// 零丢行自证（守恒 + 最近序列连续）。
    pub fn zero_drop_ok(&self) -> bool {
        if !self.ledger.conserved() {
            return false;
        }
        let mut i = 1usize;
        while i < self.recent.len() {
            if self.recent[i] != self.recent[i - 1] + 1 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 一行摘要。
    pub fn summary(&self) -> String {
        alloc::format!(
            "U101 VGA后备: 态={} 产出={} 投递={} 积压={} 滚动={} 零丢行={}",
            self.state.zh(),
            self.ledger.produced,
            self.ledger.delivered,
            self.ledger.backlog,
            self.ledger.scrolled,
            if self.zero_drop_ok() { "真" } else { "假" }
        )
    }
}

// ---------------------------------------------------------------------------
// 快照（行账 + 告警账 '|' 序列化 + FNV + 往返）
// ---------------------------------------------------------------------------

/// 后备通道快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VgaSnap {
    /// 版本戳。
    pub version: String,
    /// 状态序数。
    pub state_ord: u64,
    /// 行账四数（产出/投递/积压/滚动）。
    pub produced: u64,
    pub delivered: u64,
    pub backlog: u64,
    pub scrolled: u64,
    /// 告警行。
    pub rows: Vec<String>,
}

/// FNV-1a 64。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl VgaSnap {
    /// 定格。
    pub fn capture(f: &VgaFallback) -> VgaSnap {
        VgaSnap {
            version: String::from(UNX101_VGA_VERSION),
            state_ord: match f.state {
                ChannelState::Active => 0,
                ChannelState::Degraded => 1,
                ChannelState::Restored => 2,
            },
            produced: f.ledger.produced,
            delivered: f.ledger.delivered,
            backlog: f.ledger.backlog,
            scrolled: f.ledger.scrolled,
            rows: f.alerts.rows.clone(),
        }
    }

    /// 序列化。
    pub fn serialize(&self) -> String {
        let mut s = alloc::format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.version,
            self.state_ord,
            self.produced,
            self.delivered,
            self.backlog,
            self.scrolled,
            self.rows.len()
        );
        let mut i = 0usize;
        while i < self.rows.len() {
            s.push('|');
            s.push_str(&self.rows[i]);
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64(self.serialize().as_bytes())
    }

    /// 往返恒等。
    pub fn roundtrip(&self) -> Result<(), VgaCode> {
        match parse_snap(&self.serialize()) {
            Some(p) => {
                if p == *self && p.fingerprint() == self.fingerprint() {
                    Ok(())
                } else {
                    Err(VgaCode::SnapshotFingerprint)
                }
            }
            None => Err(VgaCode::SnapshotFingerprint),
        }
    }
}

/// 十进制解析。
fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let d = (b[i] as u64).wrapping_sub(b'0' as u64);
        if d > 9 {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    Some(v)
}

/// 快照反序列化。
pub fn parse_snap(s: &str) -> Option<VgaSnap> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() < 7 {
        return None;
    }
    let version = String::from(parts[0]);
    let state_ord = parse_u64(parts[1])?;
    let produced = parse_u64(parts[2])?;
    let delivered = parse_u64(parts[3])?;
    let backlog = parse_u64(parts[4])?;
    let scrolled = parse_u64(parts[5])?;
    let rows_len = parse_u64(parts[6])? as usize;
    if parts.len() != 7 + rows_len || state_ord > 2 {
        return None;
    }
    let mut rows = Vec::new();
    let mut i = 0usize;
    while i < rows_len {
        rows.push(String::from(parts[7 + i]));
        i += 1;
    }
    Some(VgaSnap {
        version,
        state_ord,
        produced,
        delivered,
        backlog,
        scrolled,
        rows,
    })
}

// ---------------------------------------------------------------------------
// 多口径参考探针（四口径）
// ---------------------------------------------------------------------------

/// 口径一：零丢行主链——帧簿故障落通道、产 5 行、刷出 5 行、守恒真、
/// 行号 1..5 连续、屏面文本可读回。
fn probe_zero_drop_chain() -> bool {
    let mut fb = VgaFallback::new(true, true);
    let _ = fb.on_event(SwitchEvent::FramebookFail);
    let mut i = 0usize;
    let mut ok = true;
    while i < 5 {
        let t = alloc::format!("后备行 {}", i);
        if fb.produce_line(&t).is_err() {
            ok = false;
        }
        i += 1;
    }
    if fb.flush_backlog().unwrap_or(0) != 5 {
        ok = false;
    }
    ok && fb.zero_drop_ok() && fb.ledger.produced == 5 && fb.ledger.delivered == 5
        && fb.ledger.dropped == 0
        && fb.state == ChannelState::Degraded
        && fb.screen.row_text(0).unwrap_or_default().contains("0")
}

/// 口径二：状态机迁移表全枚举——合法迁移 Some/违例 None 三态×三事件
/// 九格逐格判定。
fn probe_transition_table() -> bool {
    use ChannelState::*;
    use SwitchEvent::*;
    // Active 行。
    transition(Active, FramebookFail) == Some(Degraded)
        && transition(Active, FramebookRecover) == Some(Active)
        && transition(Active, ExplicitSwitchback) == Some(Active)
        // Degraded 行。
        && transition(Degraded, FramebookFail) == Some(Degraded)
        && transition(Degraded, FramebookRecover) == Some(Restored)
        && transition(Degraded, ExplicitSwitchback) == Some(Active)
        // Restored 行。
        && transition(Restored, FramebookFail) == Some(Degraded)
        && transition(Restored, FramebookRecover) == Some(Restored)
        && transition(Restored, ExplicitSwitchback) == Some(Active)
}

/// 口径三：切换窗口积压补发——Active 产 3 行（帧簿在岗不刷）→ 故障
/// 落通道 → 积压 3 行补发 + 新行续投——行号 1..8 连续零跳。
fn probe_switch_backlog() -> bool {
    let mut fb = VgaFallback::new(true, true);
    let mut i = 0usize;
    while i < 3 {
        let t = alloc::format!("前置 {}", i);
        let _ = fb.produce_line(&t);
        i += 1;
    }
    let _ = fb.on_event(SwitchEvent::FramebookFail);
    let mut j = 3usize;
    while j < 8 {
        let t = alloc::format!("后续 {}", j);
        let _ = fb.produce_line(&t);
        j += 1;
    }
    let flushed = fb.flush_backlog().unwrap_or(0);
    flushed == 8
        && fb.zero_drop_ok()
        && fb.ledger.delivered == 8
        && fb.recent_lines().len() == 8
        && fb.recent_lines()[0] == 1
        && fb.recent_lines()[7] == 8
}

/// 口径四：滚动不丢行 + 段守恒 + 快照往返——产 30 行刷出（触发滚动）
/// 守恒仍真；快照 capture→serialize→parse 恒等。
fn probe_scroll_snapshot() -> bool {
    if !segment_guard() {
        return false;
    }
    let mut fb = VgaFallback::new(false, true);
    let mut i = 0usize;
    while i < 30 {
        let t = alloc::format!("第 {} 行内容填充至一定宽度", i);
        let _ = fb.produce_line(&t);
        i += 1;
    }
    let _ = fb.flush_backlog();
    let scroll_ok = fb.ledger.scrolled > 0 && fb.ledger.conserved() && fb.zero_drop_ok();
    let snap = VgaSnap::capture(&fb);
    let rt = snap.roundtrip().is_ok() && snap.delivered == 30;
    scroll_ok && rt
}

/// 汇总参考探针。
pub fn reference_budget_check() -> bool {
    probe_zero_drop_chain()
        && probe_transition_table()
        && probe_switch_backlog()
        && probe_scroll_snapshot()
}

// ---------------------------------------------------------------------------
// 判据（CheckSet——判据面零 panic）
// ---------------------------------------------------------------------------

/// UNX-F0101 域自检（判据四组：通道×3、零丢行×3、缓冲×3、判据元×1）。
pub fn run_unx101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0101");

    // 通-01：状态机迁移表九格全枚举。
    if probe_transition_table() {
        s.ok("U101-通-01-迁移表九格全枚举");
    } else {
        s.fail("U101-通-01-迁移表九格全枚举", "状态机失效");
    }

    // 通-02：Restored 不自动切回——FramebookRecover 只登记（态=Restored
    // 非 Active），显性指令才 Active；帧簿仍坏时切回显性拒。
    let mut fb = VgaFallback::new(true, true);
    let _ = fb.on_event(SwitchEvent::FramebookFail);
    let st = fb.on_event(SwitchEvent::FramebookRecover);
    let noauto = st == Ok(ChannelState::Restored) && fb.state == ChannelState::Restored;
    let _ = fb.on_event(SwitchEvent::FramebookFail);
    let sb = fb.on_event(SwitchEvent::ExplicitSwitchback);
    let denied = sb == Err(VgaCode::UnauthoredSwitchback) && fb.state == ChannelState::Degraded;
    if noauto && denied {
        s.ok("U101-通-02-Restored不自动切回");
    } else {
        s.fail("U101-通-02-Restored不自动切回", "防抖动失效");
    }

    // 通-03：双通道皆坏显性告警（FramebookFail 事件触发 BothChannelsDown
    // 立案——告警面走事件通道）+ 16 色闭集恰值（from_bits 双向）。
    let mut fbd = VgaFallback::new(false, false);
    let _ = fbd.on_event(SwitchEvent::FramebookFail);
    let both_down = fbd.alerts.len() >= 1;
    let mut col_ok = true;
    let mut b = 0u8;
    while b < 16 {
        if VgaColor::from_bits(b) != Some(VgaColor::ALL[b as usize]) {
            col_ok = false;
        }
        b += 1;
    }
    let col_over = VgaColor::from_bits(16).is_none();
    let attr_ok = Attr::new(VgaColor::Yellow, VgaColor::Blue) == Some(Attr(0x1E))
        && Attr::DEFAULT.0 == 0x07
        && Attr::raw(0x07, 0x00).valid();
    if both_down && col_ok && col_over && attr_ok {
        s.ok("U101-通-03-双坏告警与16色闭集");
    } else {
        s.fail("U101-通-03-双坏告警与16色闭集", "色面或告警失效");
    }

    // 零-01：主链零丢行（故障→产 5→刷 5→守恒+屏读回）。
    if probe_zero_drop_chain() {
        s.ok("U101-零-01-主链零丢行");
    } else {
        s.fail("U101-零-01-主链零丢行", "零丢行失效");
    }

    // 零-02：切换窗口积压补发（3 前置 + 5 后续 = 8 连续行号）。
    if probe_switch_backlog() {
        s.ok("U101-零-02-切换窗口积压补发");
    } else {
        s.fail("U101-零-02-切换窗口积压补发", "补发面失效");
    }

    // 零-03：跳号显性立案——错序 deliver 拒（LineGap）。
    let mut lg = LineLedger::default();
    let n1 = lg.produce();
    let n2 = lg.produce();
    let skip = lg.deliver(n2) == Err(VgaCode::LineGap);
    let inord = lg.deliver(n1).is_ok() && lg.deliver(n2).is_ok();
    let empty_del = {
        let mut e = LineLedger::default();
        e.deliver(1) == Err(VgaCode::DroppedLine)
    };
    if skip && inord && empty_del && n1 == 1 && n2 == 2 {
        s.ok("U101-零-03-跳号与空刷显性立案");
    } else {
        s.fail("U101-零-03-跳号与空刷显性立案", "行账闸失效");
    }

    // 缓-01：超宽折行保全行文（120 字符行 → 80+40 两段入队——残尾注记）。
    let mut fbw = VgaFallback::new(false, true);
    let long = "A".repeat(120);
    let _ = fbw.produce_line(&long);
    let wrapped = fbw.queue.len() == 2
        && fbw.alerts.per_code[(VgaCode::WrapResidue.code() - SEG_BASE) as usize] >= 1;
    if wrapped {
        s.ok("U101-缓-01-超宽折行保全文");
    } else {
        s.fail("U101-缓-01-超宽折行保全文", "折行面失效");
    }

    // 缓-02：空行显性跳过 + VGA 未就绪显性拒。
    let mut fbe = VgaFallback::new(true, true);
    let empty = fbe.produce_line("") == Err(VgaCode::EmptyLineFlush);
    let mut fbn = VgaFallback::new(false, false);
    let _ = fbn.produce_line("有内容");
    let nrd = fbn.flush_backlog() == Err(VgaCode::VgaNotReady);
    if empty && nrd {
        s.ok("U101-缓-02-空行与未就绪显性");
    } else {
        s.fail("U101-缓-02-空行与未就绪显性", "闸面失效");
    }

    // 缓-03：滚动不丢行（30 行触发滚动守恒仍真）+ 快照往返 + 段守恒。
    if probe_scroll_snapshot() {
        s.ok("U101-缓-03-滚动不丢行快照往返");
    } else {
        s.fail("U101-缓-03-滚动不丢行快照往返", "滚动或快照失效");
    }

    // 组-01：十六码段守恒 + 详情互异非空。
    let codes = VgaCode::ALL;
    let mut codes_ok = codes.len() == 16;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if codes[i].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i].code() == codes[j].code() || codes[i].detail() == codes[j].detail() {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok && segment_guard() {
        s.ok("U101-组-01-十六码段守恒");
    } else {
        s.fail("U101-组-01-十六码段守恒", "码面漂移");
    }

    // 组-02：参考探针四口径全真。
    if reference_budget_check() {
        s.ok("U101-组-02-四口径探针全真");
    } else {
        s.fail("U101-组-02-四口径探针全真", "探针失效");
    }

    // 元-01：零 panic 面 + 空态平凡 + 光标越界闸 + 版本非空。
    let src = include_str!("unx101_vgafallback.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let empty_fb = VgaFallback::new(true, true);
    let snap0 = VgaSnap::capture(&empty_fb);
    let empty_ok = empty_fb.ledger.conserved()
        && empty_fb.zero_drop_ok()
        && empty_fb.summary().contains("U101")
        && snap0.roundtrip().is_ok()
        && !UNX101_VGA_VERSION.is_empty()
        && !VgaTextBuffer::cursor_ok(VGA_ROWS, 0)
        && !VgaTextBuffer::cursor_ok(0, VGA_COLS)
        && VgaTextBuffer::cursor_ok(VGA_ROWS - 1, VGA_COLS - 1)
        && VgaTextBuffer::new().row_text(0) == Some(String::new())
        && VgaTextBuffer::new().row_text(VGA_ROWS).is_none();
    if hits == 0 && empty_ok {
        s.ok("U101-元-01-零panic空态平凡");
    } else {
        s.fail("U101-元-01-零panic空态平凡", "panic 面或空态失效");
    }

    // 深-01：光标移动九操作语义恰值（边界钳制+Tab 制表步进手算）。
    if probe_cursor_semantics() {
        s.ok("U101-深-01-光标移动语义");
    } else {
        s.fail("U101-深-01-光标移动语义", "移动面失效");
    }

    // 深-02：滚动区域闸与迁移格数恰值（区域滚动静止界外行）。
    if probe_scroll_region() {
        s.ok("U101-深-02-滚动区域语义");
    } else {
        s.fail("U101-深-02-滚动区域语义", "区域面失效");
    }

    // 深-03：擦除六态闭式恰值（行内/屏内区间端点手算+边界空区间）。
    if probe_erase_semantics() {
        s.ok("U101-深-03-擦除闭集语义");
    } else {
        s.fail("U101-深-03-擦除闭集语义", "擦除面失效");
    }

    // 深-04：宽度分类与推进恰值（控制/窄/宽三态+反滚对称恒等）。
    if probe_width_semantics() {
        s.ok("U101-深-04-宽度分类语义");
    } else {
        s.fail("U101-深-04-宽度分类语义", "宽度面失效");
    }

    // 深-05：双缓冲 diff 恰值（同构恒等+单格变+单行 12 字差+
    // 刷新率整除恰值——最小刷新面）。
    if probe_buffer_diff() {
        s.ok("U101-深-05-缓冲diff恰值");
    } else {
        s.fail("U101-深-05-缓冲diff恰值", "diff面失效");
    }

    // 深-06：属性字节双语义——16 色闭集互异 + 双模式往返恒等 +
    // bit7 双语义恰值（闪烁模式 bit7=blink 背景钳 3 位 / 亮背景
    // 模式 bit7 归背景第四位）+ 默认属性 0x07。
    if probe_attr_byte() {
        s.ok("U101-深-06-属性字节双语义");
    } else {
        s.fail("U101-深-06-属性字节双语义", "属性面失效");
    }

    // 深-07：SGR 编码面——默认属性恰串 + 亮色码恰值 + 反解往返 +
    // 坏串全拒（缺前缀/段数/参越界/非数）。
    if probe_sgr_stream() {
        s.ok("U101-深-07-SGR编码往返");
    } else {
        s.fail("U101-深-07-SGR编码往返", "SGR面失效");
    }

    // 深-08：拷贝方向账——六例恰值（分离/紧邻/重叠低向/重叠高向/
    // 原地/零长）+ 滚动拷贝计划恰值（上滚正向/下滚反向/步数账）。
    if probe_copy_direction() {
        s.ok("U101-深-08-拷贝方向账");
    } else {
        s.fail("U101-深-08-拷贝方向账", "拷贝面失效");
    }

    s
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

// ===========================================================================
// 深化六区：光标移动语义闭集 / Tab 步进 / 滚动区域——后备通道不只
// 「能显示」，移动语义的边界行为（列零退格/行底 Tab/区域外滚动）
// 全部显性才是终端模拟的完整面
// ===========================================================================

/// 光标移动操作闭集（终端控制语义九操作——每格边界行为显性定义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorOp {
    /// 左移一列（列 0 不回退——边界钳制显性）。
    Left,
    /// 右移一列（列尾换行面由 put_char 管，此处列尾不动）。
    Right,
    /// 上移一行（行 0 不动）。
    Up,
    /// 下移一行（行底不动——滚动由显性 ScrollOp 触发）。
    Down,
    /// 行首。
    Home,
    /// 行尾列（最后可写列——VGA_COLS-1，不是越界列）。
    End,
    /// 回车（列零——语义同 Home 但语义学上分开：回车是流语义）。
    CarriageReturn,
    /// 退格（列 0 不回退到上一行——经典终端边界：退格只在行内）。
    Backspace,
    /// 制表步进（下一 8 列制表位——列尾钳制在最后可写列）。
    Tab,
}

impl CursorOp {
    /// 九操作闭集。
    pub const ALL: [CursorOp; 9] = [
        CursorOp::Left,
        CursorOp::Right,
        CursorOp::Up,
        CursorOp::Down,
        CursorOp::Home,
        CursorOp::End,
        CursorOp::CarriageReturn,
        CursorOp::Backspace,
        CursorOp::Tab,
    ];
}

/// 光标态（行/列双游标——移动语义的操作数与结果面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorPos {
    /// 行（0..VGA_ROWS）。
    pub row: usize,
    /// 列（0..VGA_COLS）。
    pub col: usize,
}

/// **纯函数移动语义**（不持缓冲——同光标同操作必同结果；
/// 移动语义与缓冲写面分离：移动只算坐标，写面才碰格子）。
pub const fn cursor_apply(pos: CursorPos, op: CursorOp) -> CursorPos {
    match op {
        CursorOp::Left => CursorPos {
            row: pos.row,
            col: if pos.col > 0 { pos.col - 1 } else { 0 },
        },
        CursorOp::Right => CursorPos {
            row: pos.row,
            col: if pos.col + 1 < VGA_COLS { pos.col + 1 } else { pos.col },
        },
        CursorOp::Up => CursorPos {
            row: if pos.row > 0 { pos.row - 1 } else { 0 },
            col: pos.col,
        },
        CursorOp::Down => CursorPos {
            row: if pos.row + 1 < VGA_ROWS { pos.row + 1 } else { pos.row },
            col: pos.col,
        },
        CursorOp::Home => CursorPos { row: pos.row, col: 0 },
        CursorOp::End => CursorPos { row: pos.row, col: VGA_COLS - 1 },
        CursorOp::CarriageReturn => CursorPos { row: pos.row, col: 0 },
        CursorOp::Backspace => CursorPos {
            row: pos.row,
            col: if pos.col > 0 { pos.col - 1 } else { 0 },
        },
        CursorOp::Tab => {
            // 下一 8 列制表位：col/8*8+8；越列尾钳最后可写列。
            let next = (pos.col / 8 + 1) * 8;
            CursorPos {
                row: pos.row,
                col: if next < VGA_COLS { next } else { VGA_COLS - 1 },
            }
        }
    }
}

/// 滚动区域（上下界行——区域滚动只滚界内行，界外行静止；
/// 终端模拟的 scroll region 语义：set-top/set-bottom 两锚）。
#[derive(Clone, Copy, Debug)]
pub struct ScrollRegion {
    /// 顶行（含）。
    pub top: usize,
    /// 底行（含）。
    pub bottom: usize,
}

impl ScrollRegion {
    /// 构造闸（top <= bottom 且全在屏内——非法区域显性拒）。
    pub fn new(top: usize, bottom: usize) -> Result<ScrollRegion, VgaCode> {
        if top > bottom || bottom >= VGA_ROWS {
            return Err(VgaCode::IllegalTransition);
        }
        Ok(ScrollRegion { top, bottom })
    }

    /// 全屏区域（默认——顶 0 底末行）。
    pub const fn fullscreen() -> ScrollRegion {
        ScrollRegion {
            top: 0,
            bottom: VGA_ROWS - 1,
        }
    }

    /// 高度（界内行数——滚动账的计数锚）。
    pub const fn height(&self) -> usize {
        self.bottom - self.top + 1
    }
}

/// 区域滚动语义纯函数（行内容映射：区域滚动把 (top+1..=bottom) 上移
/// 一格、bottom 行清空——返回 (src, dst) 对账面；全屏滚动是区域特例）。
pub const fn region_scroll_pairs(top: usize, bottom: usize) -> usize {
    // 迁移格数 = (bottom - top) * VGA_COLS（每行全列搬移）。
    (bottom - top) * VGA_COLS
}

/// 深化面参考探针——移动语义九操作恰值（边界行为手算对拍）。
pub fn probe_cursor_semantics() -> bool {
    let mid = CursorPos { row: 10, col: 40 };
    // 中域行为。
    let mid_ok = cursor_apply(mid, CursorOp::Left) == CursorPos { row: 10, col: 39 }
        && cursor_apply(mid, CursorOp::Right) == CursorPos { row: 10, col: 41 }
        && cursor_apply(mid, CursorOp::Up) == CursorPos { row: 9, col: 40 }
        && cursor_apply(mid, CursorOp::Down) == CursorPos { row: 11, col: 40 }
        && cursor_apply(mid, CursorOp::Home) == CursorPos { row: 10, col: 0 }
        && cursor_apply(mid, CursorOp::End) == CursorPos { row: 10, col: 79 }
        && cursor_apply(mid, CursorOp::CarriageReturn) == CursorPos { row: 10, col: 0 }
        && cursor_apply(mid, CursorOp::Backspace) == CursorPos { row: 10, col: 39 };
    // 边界行为（列 0 退格不回行/行 0 上移不动/行底下移不动/列尾右移不动）。
    let corner = CursorPos { row: 0, col: 0 };
    let edge_ok = cursor_apply(corner, CursorOp::Backspace) == corner
        && cursor_apply(corner, CursorOp::Left) == corner
        && cursor_apply(corner, CursorOp::Up) == corner;
    let bottom_right = CursorPos { row: VGA_ROWS - 1, col: VGA_COLS - 1 };
    let bottom_ok = cursor_apply(bottom_right, CursorOp::Down) == bottom_right
        && cursor_apply(bottom_right, CursorOp::Right) == bottom_right;
    // Tab 步进恰值（0→8、7→8、8→16、79→79 钳制）。
    let tab0 = cursor_apply(CursorPos { row: 0, col: 0 }, CursorOp::Tab);
    let tab7 = cursor_apply(CursorPos { row: 0, col: 7 }, CursorOp::Tab);
    let tab8 = cursor_apply(CursorPos { row: 0, col: 8 }, CursorOp::Tab);
    let tab79 = cursor_apply(CursorPos { row: 0, col: 79 }, CursorOp::Tab);
    let tab_ok = tab0.col == 8 && tab7.col == 8 && tab8.col == 16 && tab79.col == 79;
    mid_ok && edge_ok && bottom_ok && tab_ok
}

/// 深化面参考探针——滚动区域闸与语义恰值。
pub fn probe_scroll_region() -> bool {
    // 构造闸：top>bottom 拒/bottom 越屏拒。
    let bad1 = ScrollRegion::new(5, 3).is_err();
    let bad2 = ScrollRegion::new(0, VGA_ROWS).is_err();
    // 正例：区域 5..=10 高 6 行。
    let r = match ScrollRegion::new(5, 10) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let h_ok = r.height() == 6;
    // 全屏特例：高度 = VGA_ROWS。
    let full = ScrollRegion::fullscreen();
    let full_ok = full.height() == VGA_ROWS;
    // 迁移格数恰值：区域滚动搬 (bottom-top)*VGA_COLS = 5*80 = 400 格。
    let pairs_ok = region_scroll_pairs(5, 10) == 400
        && region_scroll_pairs(0, VGA_ROWS - 1) == (VGA_ROWS - 1) * VGA_COLS;
    bad1 && bad2 && h_ok && full_ok && pairs_ok
}

// ---------------------------------------------------------------------------
// 深化七区：文本缓冲操作扩展面——光标/滚动之外是「格子怎么被擦、
// 字符占多宽、屏幕怎么最小刷新」的显示适配面：擦除闭集（ANSI EL/ED
// 语义纯函数化）/ 字符宽度分类（CJK 双宽推进）/ 反向滚动对称面 /
// 双缓冲 diff（最小刷新账）。全部纯函数零 IO——与移动/滚动面同构。
// ---------------------------------------------------------------------------

/// 擦除范围闭集（ANSI EL/ED 六态——行内三态 + 屏内三态全显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EraseScope {
    /// 行内：光标（含）到行尾。
    LineRight,
    /// 行内：行首到光标（含）。
    LineLeft,
    /// 行内：整行。
    LineAll,
    /// 屏内：光标（含）到屏尾。
    ScreenBelow,
    /// 屏内：屏首到光标（含）。
    ScreenAbove,
    /// 屏内：全屏。
    ScreenAll,
}

impl EraseScope {
    /// 六态闭集。
    pub const ALL: [EraseScope; 6] = [
        EraseScope::LineRight,
        EraseScope::LineLeft,
        EraseScope::LineAll,
        EraseScope::ScreenBelow,
        EraseScope::ScreenAbove,
        EraseScope::ScreenAll,
    ];
}

/// **擦除格闭式**（scope + 光标 → 擦除格清单 [start, end) 左闭右开——
/// 平铺索引面；擦除不动光标位（擦除是内容操作不是移动操作））。
///
/// 闭式纪律：边界光标（行尾列/屏尾行）各态手算恰值——左闭右开
/// 区间端点单调（start <= end 恒成立，空区间 = 无可擦）。
pub const fn erase_range(scope: EraseScope, pos: CursorPos) -> (usize, usize) {
    let row_start = pos.row * VGA_COLS;
    let row_end = row_start + VGA_COLS;
    let cur = row_start + pos.col;
    match scope {
        EraseScope::LineRight => (cur, row_end),
        EraseScope::LineLeft => (row_start, cur + 1),
        EraseScope::LineAll => (row_start, row_end),
        EraseScope::ScreenBelow => (cur, VGA_COLS * VGA_ROWS),
        EraseScope::ScreenAbove => (0, cur + 1),
        EraseScope::ScreenAll => (0, VGA_COLS * VGA_ROWS),
    }
}

/// 擦除格数（闭式差——账面用）。
pub const fn erase_count(scope: EraseScope, pos: CursorPos) -> usize {
    let (s, e) = erase_range(scope, pos);
    e - s
}

/// 字符宽度分类（显示适配面——VGA 字符推进列数闭式）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharWidth {
    /// 控制字符（退格/回车/换行——推进 0，语义由流面管）。
    Control,
    /// 窄字符（ASCII 可打印——推进 1）。
    Narrow,
    /// 宽字符（CJK 全角——推进 2，占双格）。
    Wide,
}

/// 宽度单点（字节面分类：0x00..0x20 控制面窄表显性——0x08 退格/
/// 0x0A 换行/0x0D 回车是流语义不计推进；CJK 双宽按 U+FF01..U+FF60
/// 全角区与 CJK 区段在 UTF-8 首字节面的近似：0xEF 首字节（U+E000..
/// U+FFFF 三字节序列）按宽计——显示适配近似闭式，不解析完整 UTF-8）。
pub const fn width_of(c: u8) -> CharWidth {
    if c < 0x20 {
        CharWidth::Control
    } else if c < 0x7F {
        CharWidth::Narrow
    } else {
        // 扩展字节面：UTF-8 首字节 0xEF（CJK/全角三字节序列起点）
        // 按宽计，其余扩展窄（Latin-1 补充等单宽近似）。
        if c == 0xEF {
            CharWidth::Wide
        } else {
            CharWidth::Narrow
        }
    }
}

/// 宽度 → 推进列数（写面推进闭式——宽字符占双格）。
pub const fn advance_of(w: CharWidth) -> usize {
    match w {
        CharWidth::Control => 0,
        CharWidth::Narrow => 1,
        CharWidth::Wide => 2,
    }
}

/// **反向滚动迁移格数**（bottom 行进 top——正向滚动的对称面：
/// (bottom-top)*VGA_COLS 同式，方向反演账面恒等）。
pub const fn scroll_down_pairs(top: usize, bottom: usize) -> usize {
    (bottom - top) * VGA_COLS
}

/// **双缓冲 diff 账**（最小刷新面——逐格对拍，变化格 (row, col)
/// 清单枚举序确定；渲染驱动只重画变化格的优化账）。
pub struct BufferDiff {
    /// 变化格（行优先枚举序）。
    pub cells: Vec<(usize, usize)>,
    /// 对拍格总数（恒 == 2000——对拍完整性锚）。
    pub compared: usize,
}

impl BufferDiff {
    /// **逐格对拍**（双缓冲全格——字符与属性任一变即立案）。
    pub fn of(a: &VgaTextBuffer, b: &VgaTextBuffer) -> BufferDiff {
        let mut cells = Vec::new();
        let mut compared = 0usize;
        let mut row = 0usize;
        while row < VGA_ROWS {
            let mut col = 0usize;
            while col < VGA_COLS {
                compared += 1;
                match (a.cell(row, col), b.cell(row, col)) {
                    (Some((ca, aa)), Some((cb, ab))) => {
                        if ca != cb || aa != ab {
                            cells.push((row, col));
                        }
                    }
                    _ => cells.push((row, col)), // 读失败按异物立案（不可达面）
                }
                col += 1;
            }
            row += 1;
        }
        BufferDiff { cells, compared }
    }

    /// 零差平凡真（同构屏）。
    pub fn identical(&self) -> bool {
        self.cells.is_empty() && self.compared == VGA_COLS * VGA_ROWS
    }

    /// 刷新率（千分制）：变化格 * 1000 / 总格——最小刷新账。
    pub fn permille(&self) -> u32 {
        ((self.cells.len() as u32) * 1000) / (self.compared as u32)
    }
}

/// 深化面参考探针——擦除六态闭式恰值（边界光标手算对拍）。
pub fn probe_erase_semantics() -> bool {
    // 光标 (2, 40)：行内三态——行平铺起 160、光标格 200。
    let pos = CursorPos { row: 2, col: 40 };
    let lr = erase_range(EraseScope::LineRight, pos);
    let ll = erase_range(EraseScope::LineLeft, pos);
    let la = erase_range(EraseScope::LineAll, pos);
    let line_ok = lr == (200, 240)
        && erase_count(EraseScope::LineRight, pos) == 40
        && ll == (160, 201)
        && erase_count(EraseScope::LineLeft, pos) == 41
        && la == (160, 240)
        && erase_count(EraseScope::LineAll, pos) == 80;
    // 屏内三态——屏总 2000 格。
    let sb = erase_range(EraseScope::ScreenBelow, pos);
    let sa = erase_range(EraseScope::ScreenAbove, pos);
    let sall = erase_range(EraseScope::ScreenAll, pos);
    let screen_ok = sb == (200, 2000)
        && erase_count(EraseScope::ScreenBelow, pos) == 1800
        && sa == (0, 201)
        && erase_count(EraseScope::ScreenAbove, pos) == 201
        && sall == (0, 2000)
        && erase_count(EraseScope::ScreenAll, pos) == 2000;
    // 边界光标：行尾列 79（最后可写列）的 LineRight 恰 1 格——
    // EL 语义含光标格自身（闭式 (cur, row_end) 端点差 1）；行首
    // 列 0 的 LineLeft 同理恰 1 格（区间两端对称）。
    let end_pos = CursorPos { row: 3, col: VGA_COLS - 1 };
    let edge_ok = erase_count(EraseScope::LineRight, end_pos) == 1
        && erase_count(EraseScope::LineLeft, CursorPos { row: 3, col: 0 }) == 1
        && erase_count(EraseScope::LineAll, end_pos) == VGA_COLS;
    line_ok && screen_ok && edge_ok
}

/// 深化面参考探针——宽度分类与推进恰值（控制/窄/宽三态）。
pub fn probe_width_semantics() -> bool {
    let w_ok = width_of(0x08) == CharWidth::Control
        && width_of(0x0A) == CharWidth::Control
        && width_of(0x0D) == CharWidth::Control
        && width_of(0x1F) == CharWidth::Control
        && width_of(b'A') == CharWidth::Narrow
        && width_of(0x7E) == CharWidth::Narrow
        && width_of(0xEF) == CharWidth::Wide
        && width_of(0x80) == CharWidth::Narrow;
    let a_ok = advance_of(CharWidth::Control) == 0
        && advance_of(CharWidth::Narrow) == 1
        && advance_of(CharWidth::Wide) == 2;
    // 反滚对称：scroll_down_pairs == region_scroll_pairs（同式反演）。
    let sym_ok = scroll_down_pairs(5, 10) == region_scroll_pairs(5, 10)
        && scroll_down_pairs(0, VGA_ROWS - 1) == (VGA_ROWS - 1) * VGA_COLS;
    w_ok && a_ok && sym_ok
}

/// 深化面参考探针——双缓冲 diff 恰值（同构恒等 + 单格变恰 1 +
/// 单行写 12 字符恰 12 差异 + 刷新率恰值）。
pub fn probe_buffer_diff() -> bool {
    let a = VgaTextBuffer::new();
    let d0 = BufferDiff::of(&a, &a);
    if !d0.identical() || d0.permille() != 0 {
        return false;
    }
    // 单格变：b 在 (5, 20) 写 'X'（属性默认同——字符面差异恰 1）。
    let mut b = VgaTextBuffer::new();
    b.cur_row = 5;
    b.cur_col = 20;
    b.put_char(b'X');
    let d1 = BufferDiff::of(&a, &b);
    let one_ok = d1.cells.len() == 1
        && d1.cells[0] == (5, 20)
        && d1.compared == VGA_COLS * VGA_ROWS
        && d1.permille() == 0; // 1*1000/2000 = 0（整除下取）
    // 写一行 12 字符：恰 12 差异格（行 5 列 0..12）。
    let mut c = VgaTextBuffer::new();
    c.cur_row = 5;
    c.cur_col = 0;
    let mut k = 0usize;
    while k < 12 {
        c.put_char(b'0' + k as u8);
        k += 1;
    }
    let d2 = BufferDiff::of(&a, &c);
    let twelve_ok = d2.cells.len() == 12
        && d2.cells[0] == (5, 0)
        && d2.cells[11] == (5, 11)
        && d2.permille() == 6; // 12*1000/2000 = 6
    one_ok && twelve_ok
}

// ===========================================================================
// 深化八区：属性字节双语义 / ANSI SGR 编码 / 重叠拷贝方向账——后备
// 通道不只「写格子」：属性面（VGA 硬件属性字节的 bit7 双语义闸）、
// 互操作面（VGA 缓冲 → ANSI 终端流的 SGR 重放编码——后备显示挂到
// 串口终端上的翻译层）、安全面（重叠拷贝方向判定——帧缓冲大块
// memmove 在真机翻车教训的纯函数镜像：方向选错 = 自我覆盖）。
// 色彩闭集复用核心区 [`VgaColor`]（bits/from_bits 单点）——深化区
// 只增 zh 人读名与亮色判定两个 const 面，不重复定义闭集本体。
// ===========================================================================

impl VgaColor {
    /// 中文名（互异非空——深化区增面）。
    pub const fn zh(self) -> &'static str {
        match self {
            VgaColor::Black => "黑",
            VgaColor::Blue => "蓝",
            VgaColor::Green => "绿",
            VgaColor::Cyan => "青",
            VgaColor::Red => "红",
            VgaColor::Magenta => "品红",
            VgaColor::Brown => "棕",
            VgaColor::LightGray => "浅灰",
            VgaColor::DarkGray => "深灰",
            VgaColor::LightBlue => "亮蓝",
            VgaColor::LightGreen => "亮绿",
            VgaColor::LightCyan => "亮青",
            VgaColor::LightRed => "亮红",
            VgaColor::LightMagenta => "亮品红",
            VgaColor::Yellow => "黄",
            VgaColor::White => "白",
        }
    }

    /// 亮色判定（8..15——SGR 亮色码域，深化区增面）。
    pub const fn is_light(self) -> bool {
        self.bits() >= 8
    }
}

/// bit7 双语义模式闸：VGA 属性字节最高位在「闪烁允许」模式下是
/// 闪烁位（背景钳 3 位 = 8 色），在「闪烁禁止（亮背景）」模式下是
/// 背景第四位（背景 16 色、无闪烁）——同一字节两种读法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlinkMode {
    /// bit7 = 闪烁位（背景 8 色）。
    Blinking,
    /// bit7 = 背景亮色位（背景 16 色无闪烁）。
    BrightBg,
}

/// VGA 属性字节面（前景 4 位 / 背景 3 或 4 位（模式依赖）/ 闪烁位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttrByte {
    pub fg: VgaColor,
    pub bg: VgaColor,
    pub blink: bool,
}

/// 默认属性（0x07 黑底浅灰字——VGA BIOS 经典值）。
pub const ATTR_DEFAULT: u8 = 0x07;

impl AttrByte {
    /// 装配（bg 亮色 + Blinking 模式 = 语义冲突——背景钳回 3 位域）。
    pub const fn pack(a: AttrByte, mode: BlinkMode) -> u8 {
        let fg = a.fg.bits() & 0x0F;
        match mode {
            BlinkMode::Blinking => {
                let bg = a.bg.bits() & 0x07;
                let blink = if a.blink { 0x80 } else { 0x00 };
                fg | (bg << 4) | blink
            }
            BlinkMode::BrightBg => fg | (a.bg.bits() << 4),
        }
    }

    /// 拆解（模式化读法单点）。
    pub const fn unpack(byte: u8, mode: BlinkMode) -> AttrByte {
        let fg = VgaColor::from_bits(byte & 0x0F);
        match mode {
            BlinkMode::Blinking => {
                let bg = VgaColor::from_bits((byte >> 4) & 0x07);
                let fg = match fg {
                    Some(c) => c,
                    None => VgaColor::Black,
                };
                let bg = match bg {
                    Some(c) => c,
                    None => VgaColor::Black,
                };
                AttrByte { fg, bg, blink: byte & 0x80 != 0 }
            }
            BlinkMode::BrightBg => {
                let fg = match fg {
                    Some(c) => c,
                    None => VgaColor::Black,
                };
                let bg = match VgaColor::from_bits((byte >> 4) & 0x0F) {
                    Some(c) => c,
                    None => VgaColor::Black,
                };
                AttrByte { fg, bg, blink: false }
            }
        }
    }

    /// 十六进制线格式（aa——外带诊断面）。
    pub fn wire(a: AttrByte, mode: BlinkMode) -> String {
        alloc::format!("{:02x}", AttrByte::pack(a, mode))
    }
}

/// **SGR 重放编码**（VGA 属性 → ANSI SGR 选择GraphicRendition 序列
/// ——后备缓冲 dump 到串口终端的翻译面；reset 起手清历史态）。
pub fn sgr_reset() -> String {
    String::from("\x1b[0m")
}

/// 属性 → SGR 序列（`\x1b[0;<fg>;<bg>m`：前景 30..37 暗 / 90..97 亮；
/// 背景 40..47 暗 / 100..107 亮——BrightBg 模式下背景亮色才有亮码）。
pub fn attr_to_sgr(a: AttrByte, mode: BlinkMode) -> String {
    let fg_ord = a.fg.bits();
    let fg_code: u8 = if fg_ord >= 8 { 90 + fg_ord - 8 } else { 30 + fg_ord };
    let bg_ord = match mode {
        BlinkMode::Blinking => a.bg.bits() & 0x07,
        BlinkMode::BrightBg => a.bg.bits(),
    };
    let bg_code: u8 = if bg_ord >= 8 { 100 + bg_ord - 8 } else { 40 + bg_ord };
    alloc::format!("\x1b[0;{};{}m", fg_code, bg_code)
}

/// SGR 十进制参数解析（1..3 位数字，闭式钳 255）。
fn parse_sgr_param(s: &str) -> Option<u8> {
    if s.is_empty() || s.len() > 3 {
        return None;
    }
    let mut v: u16 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let d = (b[i] as u16).wrapping_sub(b'0' as u16);
        if d > 9 {
            return None;
        }
        v = v * 10 + d;
        i += 1;
    }
    if v > 255 {
        return None;
    }
    Some(v as u8)
}

/// SGR 参数 → 色（30..37 暗 / 90..97 亮 → 前景；40..47 / 100..107 →
/// 背景；其余码域 None——只吃颜色码，字型码不吃）。
fn sgr_param_color(code: u8, bright: bool) -> Option<VgaColor> {
    if code >= 30 && code <= 37 {
        VgaColor::from_bits(code - 30)
    } else if code >= 90 && code <= 97 {
        VgaColor::from_bits(code - 90 + 8)
    } else if !bright && code >= 40 && code <= 47 {
        VgaColor::from_bits(code - 40)
    } else if bright && code >= 100 && code <= 107 {
        VgaColor::from_bits(code - 100 + 8)
    } else {
        None
    }
}

/// SGR 序列反解（`\x1b[0;<fg>;<bg>m` 三段闸——前缀/段数/色码域/
/// 亮暗互斥（前景亮码配背景暗码域，全亮或全暗均可）全显性）。
pub fn sgr_to_attr(s: &str) -> Option<AttrByte> {
    if !s.starts_with("\x1b[0;") || !s.ends_with('m') {
        return None;
    }
    let body = &s[4..s.len() - 1];
    let parts: Vec<&str> = body.split(';').collect();
    if parts.len() != 2 {
        return None;
    }
    let fg_code = parse_sgr_param(parts[0])?;
    let bg_code = parse_sgr_param(parts[1])?;
    let fg = sgr_param_color(fg_code, true)?;
    // 背景码域收紧：只吃 40..47 暗 / 100..107 亮（前景码不吃）。
    let bg = if bg_code >= 40 && bg_code <= 47 {
        sgr_param_color(bg_code, false)?
    } else if bg_code >= 100 && bg_code <= 107 {
        sgr_param_color(bg_code, true)?
    } else {
        return None;
    };
    Some(AttrByte { fg, bg, blink: false })
}

/// **重叠拷贝方向判定**（帧缓冲 memmove 语义纯函数镜像——方向选错
/// = 源区未读先被覆盖 = 自我覆盖事故）：
/// - 零长或原地 → None（无操作）；
/// - 不重叠或目的在源下方（dst < src）→ Forward（低地址向高安全）；
/// - 重叠且目的在源上方（dst > src）→ Backward（高地址向低安全）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyDir {
    /// 正向（低地址 → 高地址逐单元拷）。
    Forward,
    /// 反向（高地址 → 低地址逐单元拷）。
    Backward,
}

/// 方向判定（src/dst 为单元下标，len 为单元数）。
pub const fn copy_dir(src: usize, dst: usize, len: usize) -> Option<CopyDir> {
    if len == 0 || src == dst {
        return None;
    }
    // 重叠判定：[src, src+len) ∩ [dst, dst+len) 非空。
    let overlap = dst < src + len && src < dst + len;
    if overlap && dst > src {
        Some(CopyDir::Backward)
    } else {
        Some(CopyDir::Forward)
    }
}

/// **滚动拷贝计划**（整屏行滚动 = 缓冲区大块移动——shift 正 = 内容
/// 上滚（源在目的下方 = dst < src → Forward）；shift 负 = 下滚 →
/// Backward；shift 0 → None。步数账 = 拷贝单元数）。
pub const fn copy_plan(rows: usize, cols: usize, shift: isize) -> Option<(CopyDir, usize)> {
    if shift == 0 || rows == 0 || cols == 0 {
        return None;
    }
    // 包裹取绝对值（isize::MIN 取负会 panic——零 panic 面纪律）。
    let mag = if shift < 0 {
        (0isize).wrapping_sub(shift) as usize
    } else {
        shift as usize
    };
    if mag >= rows {
        return None;
    }
    let len = (rows - mag) * cols;
    if shift > 0 {
        Some((CopyDir::Forward, len))
    } else {
        Some((CopyDir::Backward, len))
    }
}

/// 深-06 判据：属性字节双语义——16 色闭集互异 + 双模式往返恒等 +
/// bit7 双语义恰值 + 默认属性 0x07 + 线格式。
pub fn probe_attr_byte() -> bool {
    // 十六色闭集：序数互异 + 反解往返 + 中文名互异非空。
    let mut colors_ok = VgaColor::ALL.len() == 16;
    let mut i = 0usize;
    while i < 16 && colors_ok {
        let c = VgaColor::ALL[i];
        colors_ok = colors_ok
            && c.bits() == i as u8
            && VgaColor::from_bits(i as u8) == Some(c)
            && !c.zh().is_empty();
        let mut j = i + 1;
        while j < 16 {
            if c.zh() == VgaColor::ALL[j].zh() {
                colors_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let oob = VgaColor::from_bits(16).is_none();
    // 双模式往返：全 256 字节 × 全 256 字节（pack∘unpack 恒等）。
    let mut round_ok = true;
    let mut b = 0u32;
    while b < 256 {
        let byte = b as u8;
        if AttrByte::pack(AttrByte::unpack(byte, BlinkMode::Blinking), BlinkMode::Blinking) != byte {
            round_ok = false;
        }
        if AttrByte::pack(AttrByte::unpack(byte, BlinkMode::BrightBg), BlinkMode::BrightBg) != byte {
            round_ok = false;
        }
        b += 1;
    }
    // bit7 双语义恰值：0xFF 在闪烁模式 = 白字/浅灰底/闪；亮背景模式
    // = 白字/白底/不闪——同一字节两种读法。
    let blinky = AttrByte::unpack(0xFF, BlinkMode::Blinking);
    let bright = AttrByte::unpack(0xFF, BlinkMode::BrightBg);
    let dual_ok = blinky.fg == VgaColor::White
        && blinky.bg == VgaColor::LightGray
        && blinky.blink
        && bright.fg == VgaColor::White
        && bright.bg == VgaColor::White
        && !bright.blink;
    // 装配冲突语义：BrightBg 属性走 Blinking 装配 → 背景钳 3 位
    // （White 0x0F & 0x07 = 0x07 → 0x77 非亮色）。
    let clamp = AttrByte::pack(
        AttrByte { fg: VgaColor::LightGray, bg: VgaColor::White, blink: false },
        BlinkMode::Blinking,
    ) == 0x77;
    // 默认属性恰值：0x07 = 浅灰字黑底不闪。
    let def = AttrByte::unpack(ATTR_DEFAULT, BlinkMode::Blinking);
    let def_ok = def.fg == VgaColor::LightGray && def.bg == VgaColor::Black && !def.blink;
    // 线格式恰值。
    let wire_ok = AttrByte::wire(def, BlinkMode::Blinking) == "07";
    colors_ok && oob && round_ok && dual_ok && clamp && def_ok && wire_ok
}

/// 深-07 判据：SGR 编码面——reset 恰串 + 默认属性恰串 + 亮色码恰值
/// + 反解往返恒等 + 坏串全拒（缺前缀/缺尾/段数/色码域/非数）。
pub fn probe_sgr_stream() -> bool {
    let reset_ok = sgr_reset() == "\x1b[0m";
    // 默认属性 → "\x1b[0;37;40m"（浅灰前景暗码 / 黑底暗码）。
    let def = AttrByte::unpack(ATTR_DEFAULT, BlinkMode::Blinking);
    let enc = attr_to_sgr(def, BlinkMode::Blinking);
    let enc_ok = enc == "\x1b[0;37;40m";
    // 亮红前景（0x0C）+ 黑底 → "\x1b[0;94;40m"。
    let lite = AttrByte { fg: VgaColor::LightRed, bg: VgaColor::Black, blink: false };
    let lite_enc = attr_to_sgr(lite, BlinkMode::Blinking);
    let lite_ok = lite_enc == "\x1b[0;94;40m";
    // 亮背景模式：白底 → "\x1b[0;37;107m"。
    let wb = AttrByte { fg: VgaColor::LightGray, bg: VgaColor::White, blink: false };
    let wb_enc = attr_to_sgr(wb, BlinkMode::BrightBg);
    let wb_ok = wb_enc == "\x1b[0;37;107m";
    // 反解往返：编码串反解回属性（blink 面不进 SGR——往返恒等按
    // blink=false 比对）。
    let rt = sgr_to_attr(&enc);
    let rt_ok = match rt {
        Some(a) => a.fg == def.fg && a.bg == def.bg && !a.blink,
        None => false,
    };
    let rt2 = sgr_to_attr(&wb_enc);
    let rt2_ok = match rt2 {
        Some(a) => a.fg == VgaColor::LightGray && a.bg == VgaColor::White,
        None => false,
    };
    // 坏串全拒。
    let bad_ok = sgr_to_attr("\x1b[0;37m").is_none()
        && sgr_to_attr("\x1b[0;37;40").is_none()
        && sgr_to_attr("[0;37;40m").is_none()
        && sgr_to_attr("\x1b[1;37;40m").is_none()
        && sgr_to_attr("\x1b[0;99;40m").is_none()
        && sgr_to_attr("\x1b[0;37;48m").is_none()
        && sgr_to_attr("\x1b[0;3a;40m").is_none()
        && sgr_to_attr("\x1b[0;377;40m").is_none();
    reset_ok && enc_ok && lite_ok && wb_ok && rt_ok && rt2_ok && bad_ok
}

/// 深-08 判据：拷贝方向账——六例恰值（分离正向/紧邻正向/重叠高向
/// 反向/重叠低向正向/原地 None/零长 None）+ 滚动计划恰值（上滚正向
/// 步数 = (25-1)*80 / 下滚反向 / 全滚 None / 零滚动 None）+ 步数账。
pub fn probe_copy_direction() -> bool {
    // 分离区（dst 完全在源右侧之外）→ Forward。
    let apart = copy_dir(0, 100, 50) == Some(CopyDir::Forward);
    // 紧邻（dst == src+len 无重叠）→ Forward。
    let adjacent = copy_dir(0, 50, 50) == Some(CopyDir::Forward);
    // 重叠且 dst > src（高向）→ Backward。
    let high = copy_dir(0, 10, 50) == Some(CopyDir::Backward);
    // 重叠且 dst < src（低向）→ Forward。
    let low = copy_dir(10, 0, 50) == Some(CopyDir::Forward);
    // 原地 → None；零长 → None。
    let inplace = copy_dir(7, 7, 50).is_none() && copy_dir(0, 9, 0).is_none();
    let dir_ok = apart && adjacent && high && low && inplace;
    // 滚动计划：上滚 1 行（内容上移）→ Forward + (25-1)*80=1920 步。
    let up = copy_plan(VGA_ROWS, VGA_COLS, 1) == Some((CopyDir::Forward, (VGA_ROWS - 1) * VGA_COLS));
    // 下滚 2 行 → Backward + 23*80=1840 步。
    let down = copy_plan(VGA_ROWS, VGA_COLS, -2) == Some((CopyDir::Backward, (VGA_ROWS - 2) * VGA_COLS));
    // 全滚（mag >= rows）→ None；零滚动 → None。
    let edge = copy_plan(VGA_ROWS, VGA_COLS, 25).is_none() && copy_plan(VGA_ROWS, VGA_COLS, 0).is_none();
    // 步数账闭式：拷贝步数恒 = (rows - |shift|) * cols。
    let steps_ok = match copy_plan(25, 80, 3) {
        Some((_, n)) => n == 22 * 80,
        None => false,
    };
    dir_ok && up && down && edge && steps_ok
}
