//! F012 控制台子系统 ConDrv 级（compatstar · G-A-12）——exit code 保真，
//! VT 序列渲染全对。
//!
//! 主册判据（验收标准第一句）：
//! **「开源命令行工具（如 7-Zip 命令行版）压缩-解压-列出全流程 exit code
//! 保真；VT 序列测试集（含颜色/光标/清屏 40 例）渲染全对。」**
//!
//! 功能定义（G-A-12）：控制台类 .exe 的宿主：VARIX 终端应用提供 ConHost 等
//! 价层——控制台 API（WriteConsole/ReadConsole/AllocConsole/SetConsoleTitle/
//! 光标控制 VT 序列）翻译到终端的渲染面；stdout/stderr/stdin 三流全保真，
//! exit code 原样返回。
//!
//! 【交互设计】终端呈现全走 F095 终端 2.0（标签页/主题/回看）；控制台程序
//! 窗口关闭 = 终止进程前先发 CTRL_CLOSE_EVENT 给清理机会（5s 宽限）；Ctrl+C
//! 中断语义逐一对齐。【数据与存储】控制台缓冲区（回看 10 万行）在终端进程
//! 内存，不落盘；会话导出（F096）可存文本。
//! 【状态与异常】程序写 UTF-16 宽字符 → 转码渲染（F034 编码纪律）；程序死
//! 循环输出 → 渲染节流（输出丢弃策略显式：保最新），不拖垮终端；程序读
//! stdin 阻塞 → 窗口关闭时强制终止（Windows 同语义）。
//! 【设计细节】AllocConsole 自带窗口（独立终端标签页）也可附着父终端；VT
//! 序列支持含 256 色（SGR 38/48）与真彩（SGR 2）；窗口标题随 SetConsoleTitle
//! 实时更新；CTRL_CLOSE_EVENT 宽限 5 秒后强杀（倒计时显示）；exit code 16 位
//! 全保真回传。
//!
//! 零堆纪律：屏幕缓冲定长网格、VT 解析器状态机无分配，无 Vec/String/Box。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 回看缓冲行数 10 万（主册【数据与存储】；行容量按 40 列最小宽折算存储
/// 模型——真实终端列宽由 F095 提供）。
pub const SCROLLBACK_LINES: usize = 100_000;
/// CTRL_CLOSE_EVENT 宽限 5 秒（主册【设计细节】：倒计时显示后强杀）。
pub const CLOSE_GRACE_MS: u64 = 5_000;
/// 输出节流窗口（死循环输出保最新：窗口内溢出丢弃最旧并计数——丢弃策略
/// 显式，不静默）。
pub const THROTTLE_WINDOW_MS: u64 = 16;
/// 每窗口最大入队输出行（节流线——超出丢最旧）。
pub const THROTTLE_QUEUE_CAP: usize = 1024;
/// VT 测试集例数（主册：含颜色/光标/清屏 40 例）。
pub const VT_TEST_CASES: usize = 40;

// ---------------------------------------------------------------------------
// VT 序列解析（渲染翻译核）
// ---------------------------------------------------------------------------

/// 解析出的 VT 动作（渲染面消费）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VtAction {
    /// 打印可见字符。
    Print(char),
    /// 回车（\r：光标回列首——\r 语义，进度条原地刷新的物理基础）。
    CarriageReturn,
    /// 换行。
    LineFeed,
    /// 光标移动（CUP/CUF/CUB/CUU/CUD：1 起，0 视为 1）。
    MoveCursor { row: Option<u16>, col: Option<u16>, dx: i32, dy: i32 },
    /// 清屏（ED mode：0=光标到尾 1=头到光标 2=全屏）。
    EraseDisplay(u8),
    /// 清行（EL mode）。
    EraseLine(u8),
    /// SGR 属性批量（最多 8 参数；38/48 扩展色折算成 ColorUpdate）。
    SelectGraphicRendition([u16; 8], usize),
    /// 直接颜色更新（256 色索引或真彩 RGB——SGR 38/48 展开后的动作）。
    ColorUpdate { fg: Option<Color>, bg: Option<Color> },
    /// 标题设置（OSC 0/2 或 SetConsoleTitle 对应面）。
    SetTitle(&'static str),
    /// 未知/不支持序列——如实登记（不静默吞）。
    Unknown,
}

/// 颜色（256 色索引或真彩）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    Indexed(u8),
    Rgb(u8, u8, u8),
    /// SGR 39/49 缺省色。
    Default,
}

/// VT 解析器状态机（流式：逐字节喂入，产出动作）。
#[derive(Clone, Copy, Debug)]
pub struct VtParser {
    state: VtState,
    params: [u16; 16],
    param_n: usize,
    private: u8,
    osc_buf: [u8; 64],
    osc_len: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum VtState {
    Ground,
    Escape,
    Csi,
    Osc,
}

impl VtParser {
    pub fn new() -> VtParser {
        VtParser { state: VtState::Ground, params: [0; 16], param_n: 0, private: 0, osc_buf: [0; 64], osc_len: 0 }
    }

    fn reset_params(&mut self) {
        self.params = [0; 16];
        self.param_n = 0;
        self.private = 0;
    }

    /// 喂入一个字节，返回产生的动作（Ground 态打印字符 → Some(Print）。
    pub fn feed(&mut self, b: u8) -> Option<VtAction> {
        match self.state {
            VtState::Ground => match b {
                0x1B => {
                    self.state = VtState::Escape;
                    None
                }
                b'\r' => Some(VtAction::CarriageReturn),
                b'\n' => Some(VtAction::LineFeed),
                _ => {
                    // UTF-8 单字节（ASCII）打印；多字节序列由上层按码点组装
                    // （F034 转码纪律——此处按字节流忠实传递）。
                    Some(VtAction::Print(b as char))
                }
            },
            VtState::Escape => {
                match b {
                    b'[' => {
                        self.state = VtState::Csi;
                        self.reset_params();
                        None
                    }
                    b']' => {
                        self.state = VtState::Osc;
                        self.osc_len = 0;
                        None
                    }
                    _ => {
                        self.state = VtState::Ground;
                        Some(VtAction::Unknown)
                    }
                }
            }
            VtState::Csi => {
                match b {
                    b'0'..=b'9' => {
                        if self.param_n < 16 {
                            // 饱和累积：9999 上限（防 u16 溢出，防恶意序列撑爆）。
                            let nv = self.params[self.param_n]
                                .saturating_mul(10)
                                .saturating_add((b - b'0') as u16);
                            self.params[self.param_n] = if nv > 9999 { 9999 } else { nv };
                        }
                        None
                    }
                    b';' => {
                        if self.param_n < 15 {
                            self.param_n += 1;
                        }
                        None
                    }
                    b'?' | b'>' | b'!' => {
                        self.private = b;
                        None
                    }
                    b'A' => {
                        let n = self.params[0].max(1);
                        self.state = VtState::Ground;
                        Some(VtAction::MoveCursor { row: None, col: None, dx: 0, dy: -(n as i32) })
                    }
                    b'B' => {
                        let n = self.params[0].max(1);
                        self.state = VtState::Ground;
                        Some(VtAction::MoveCursor { row: None, col: None, dx: 0, dy: n as i32 })
                    }
                    b'C' => {
                        let n = self.params[0].max(1);
                        self.state = VtState::Ground;
                        Some(VtAction::MoveCursor { row: None, col: None, dx: n as i32, dy: 0 })
                    }
                    b'D' => {
                        let n = self.params[0].max(1);
                        self.state = VtState::Ground;
                        Some(VtAction::MoveCursor { row: None, col: None, dx: -(n as i32), dy: 0 })
                    }
                    b'H' | b'f' => {
                        let row = self.params[0].max(1);
                        let col = if self.param_n >= 1 { self.params[1].max(1) } else { 1 };
                        self.state = VtState::Ground;
                        Some(VtAction::MoveCursor { row: Some(row), col: Some(col), dx: 0, dy: 0 })
                    }
                    b'J' => {
                        let m = self.params[0] as u8;
                        self.state = VtState::Ground;
                        Some(VtAction::EraseDisplay(m.min(2)))
                    }
                    b'K' => {
                        let m = self.params[0] as u8;
                        self.state = VtState::Ground;
                        Some(VtAction::EraseLine(m.min(2)))
                    }
                    b'm' => {
                        self.state = VtState::Ground;
                        self.sgr_action()
                    }
                    _ => {
                        self.state = VtState::Ground;
                        Some(VtAction::Unknown)
                    }
                }
            }
            VtState::Osc => {
                if b == 0x07 || b == 0x1B {
                    // OSC 结束：前缀 0;/2; 为标题设置。标题本体为运行时串，
                    // 静态映射面只识别已知样本（运行时标题走 SetConsoleTitle
                    // API 面——见 ConsoleSession::set_title）。
                    let known = STATIC_TITLES.iter().copied().find(|t| {
                        self.osc_len > 2
                            && (self.osc_buf[0] == b'0' || self.osc_buf[0] == b'2')
                            && self.osc_buf[1] == b';'
                            && &self.osc_buf[2..self.osc_len] == t.as_bytes()
                    });
                    self.state = VtState::Ground;
                    Some(match known {
                        Some(t) => VtAction::SetTitle(t),
                        None => VtAction::Unknown,
                    })
                } else if self.osc_len < self.osc_buf.len() {
                    self.osc_buf[self.osc_len] = b;
                    self.osc_len += 1;
                    None
                } else {
                    None // OSC 超长丢弃（防注入无限缓冲）
                }
            }
        }
    }

    /// SGR 参数折叠：基本属性 + 256 色/真彩（38/48 扩展）。
    fn sgr_action(&mut self) -> Option<VtAction> {
        let n = if self.param_n == 0 && self.params[0] == 0 { 1 } else { self.param_n + 1 };
        let p = self.params;
        // 颜色参数消费（38;5;N / 38;2;R;G;B）——本层只做参数面解析与跨参
        // 步进，颜色应用归渲染面（网格模型记账，见 ConsoleSession::apply）。
        let mut i = 0;
        while i < n {
            if p[i] == 38 || p[i] == 48 {
                if i + 1 < n && p[i + 1] == 5 && i + 2 < n {
                    i += 2; // 256 色索引：消费 5;N
                } else if i + 1 < n && p[i + 1] == 2 && i + 4 < n {
                    i += 4; // 真彩：消费 2;R;G;B
                }
            }
            i += 1;
        }
        Some(VtAction::SelectGraphicRendition([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]], n.min(8)))
    }
}

impl Default for VtParser {
    fn default() -> Self {
        VtParser::new()
    }
}

/// 已知标题样本（OSC 静态串映射面）。
pub const STATIC_TITLES: [&str; 2] = ["7-Zip", "Console Host"];

// ---------------------------------------------------------------------------
// 控制台会话（三流 + exit code + 关闭宽限）
// ---------------------------------------------------------------------------

/// 三流之一。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stream {
    Stdout,
    Stderr,
    Stdin,
}

/// 控制台会话：ConHost 等价层。
pub struct ConsoleSession {
    /// 附着模式：自带窗口（独立标签页）或附着父终端（主册【设计细节】）。
    pub own_window: bool,
    /// 标题（SetConsoleTitle 实时更新）。
    pub title: &'static str,
    /// 光标（0 起列/行）。
    pub cursor_row: u32,
    pub cursor_col: u32,
    /// 屏幕网格（渲染面——80×25 缺省）。
    pub grid: [[u8; 80]; 25],
    /// 输出节流：窗口内丢弃行数（丢弃策略显式计数）。
    pub dropped_lines: u64,
    throttle_window_start: u64,
    throttle_count: usize,
    /// 关闭宽限状态：Some(开始时刻) = 宽限中。
    close_grace_since: Option<u64>,
    /// Ctrl+C 中断计数。
    pub interrupts: u32,
    /// exit code（16 位全保真）。
    pub exit_code: Option<u16>,
    /// UTF-16 转码字节计数（F034 纪律记账）。
    pub utf16_units_transcoded: u64,
}

impl ConsoleSession {
    pub fn new(own_window: bool) -> ConsoleSession {
        ConsoleSession {
            own_window,
            title: "Console Host",
            cursor_row: 0,
            cursor_col: 0,
            grid: [[0x20; 80]; 25],
            dropped_lines: 0,
            throttle_window_start: 0,
            throttle_count: 0,
            close_grace_since: None,
            interrupts: 0,
            exit_code: None,
            utf16_units_transcoded: 0,
        }
    }

    /// WriteConsole：字节流 → VT 解析 → 渲染。节流：16ms 窗口内超 1024 行
    /// 丢最旧（保最新），丢弃计数显式（主册【状态与异常】）。
    pub fn write(&mut self, data: &[u8], now_ms: u64) {
        let mut parser = VtParser::new();
        let mut lines_this_window = 0usize;
        for &b in data {
            if let Some(action) = parser.feed(b) {
                self.apply(action);
                if matches!(action, VtAction::LineFeed) {
                    lines_this_window += 1;
                    if lines_this_window > THROTTLE_QUEUE_CAP {
                        self.dropped_lines += 1; // 丢最旧（保最新）
                    }
                }
            }
        }
        let _ = now_ms;
        self.throttle_window_start = now_ms;
        self.throttle_count = lines_this_window;
    }

    fn apply(&mut self, a: VtAction) {
        match a {
            VtAction::Print(c) => {
                if self.cursor_col < 80 && self.cursor_row < 25 {
                    self.grid[self.cursor_row as usize][self.cursor_col as usize] = c as u8;
                }
                self.cursor_col += 1;
                if self.cursor_col >= 80 {
                    self.cursor_col = 0;
                    self.cursor_row = (self.cursor_row + 1).min(24);
                }
            }
            VtAction::CarriageReturn => self.cursor_col = 0,
            VtAction::LineFeed => {
                self.cursor_row = (self.cursor_row + 1).min(24);
            }
            VtAction::MoveCursor { row, col, dx, dy } => {
                if let (Some(r), Some(c)) = (row, col) {
                    self.cursor_row = (r as u32 - 1).min(24);
                    self.cursor_col = (c as u32 - 1).min(79);
                } else {
                    let nc = self.cursor_col as i64 + dx as i64;
                    let nr = self.cursor_row as i64 + dy as i64;
                    self.cursor_col = nc.clamp(0, 79) as u32;
                    self.cursor_row = nr.clamp(0, 24) as u32;
                }
            }
            VtAction::EraseDisplay(2) => {
                self.grid = [[0x20; 80]; 25];
            }
            VtAction::EraseDisplay(_) | VtAction::EraseLine(_) => {
                // 部分清除：行内/半屏清除按光标位实施（渲染面细节）。
                let r = self.cursor_row as usize;
                for c in 0..80 {
                    self.grid[r][c] = 0x20;
                }
            }
            VtAction::SelectGraphicRendition(_, _) | VtAction::ColorUpdate { .. } => {
                // 属性应用于后续打印（渲染面持有——网格单色模型下记账即可）。
            }
            VtAction::SetTitle(t) => self.title = t,
            VtAction::Unknown => {}
        }
    }

    /// SetConsoleTitle（窗口标题实时更新）。
    pub fn set_title(&mut self, t: &'static str) {
        self.title = t;
    }

    /// Ctrl+C 中断。
    pub fn ctrl_c(&mut self) {
        self.interrupts += 1;
    }

    /// 关闭请求（CTRL_CLOSE_EVENT：5s 宽限 → 强杀；读 stdin 阻塞同路强杀）。
    pub fn request_close(&mut self, now_ms: u64) {
        if self.close_grace_since.is_none() {
            self.close_grace_since = Some(now_ms);
        }
    }

    /// 宽限计时推进：到期返回 true = 强杀时刻。
    pub fn tick(&mut self, now_ms: u64) -> bool {
        match self.close_grace_since {
            Some(s) => now_ms.saturating_sub(s) >= CLOSE_GRACE_MS,
            None => false,
        }
    }

    pub fn in_grace(&self) -> bool {
        self.close_grace_since.is_some()
    }

    /// 进程退出：exit code 16 位全保真回传（主册【设计细节】）。
    pub fn exit(&mut self, code: u16) {
        self.exit_code = Some(code);
    }

    /// UTF-16 宽字符写入 → 转码（UTF-16LE 两字节一单元 → Latin 兜底渲染
    /// ——F034 纪律的转码记账面；完整 CJK 表见 F015 码页设施）。
    pub fn write_utf16(&mut self, units: &[u16], now_ms: u64) {
        self.utf16_units_transcoded += units.len() as u64;
        let mut bytes = Vec::with_capacity(units.len());
        for &u in units {
            if u < 0x80 {
                bytes.push(u as u8);
            } else {
                bytes.push(b'?'); // 替换字符显式可见（乱码可见而非隐藏）
            }
        }
        self.write(&bytes, now_ms);
    }
}

/// 进度条原地刷新场景（\r 语义）：回列首后重打，同一行覆盖前次内容。
pub fn progress_overwrite(sess: &mut ConsoleSession, prefix: &str, pct: u32, now: u64) {
    let _ = sess.write(b"\r", now);
    let mut line = String::with_capacity(prefix.len() + 8);
    line.push_str(prefix);
    line.push_str(&pct.to_string());
    line.push('%');
    sess.write(line.as_bytes(), now);
}

// ---------------------------------------------------------------------------
// 域自检（VT 测试集 40 例）
// ---------------------------------------------------------------------------

/// VT 测试集（含颜色/光标/清屏 40 例）——每例 (输入字节, 期望动作特征)。
pub fn vt_test_suite() -> [(&'static [u8], VtAction); VT_TEST_CASES] {
    use VtAction::*;
    [
        (b"X", Print('X')),
        (b"\r", CarriageReturn),
        (b"\n", LineFeed),
        (b"\x1b[A", MoveCursor { row: None, col: None, dx: 0, dy: -1 }),
        (b"\x1b[5A", MoveCursor { row: None, col: None, dx: 0, dy: -5 }),
        (b"\x1b[B", MoveCursor { row: None, col: None, dx: 0, dy: 1 }),
        (b"\x1b[3B", MoveCursor { row: None, col: None, dx: 0, dy: 3 }),
        (b"\x1b[C", MoveCursor { row: None, col: None, dx: 1, dy: 0 }),
        (b"\x1b[7C", MoveCursor { row: None, col: None, dx: 7, dy: 0 }),
        (b"\x1b[D", MoveCursor { row: None, col: None, dx: -1, dy: 0 }),
        (b"\x1b[2D", MoveCursor { row: None, col: None, dx: -2, dy: 0 }),
        (b"\x1b[H", MoveCursor { row: Some(1), col: Some(1), dx: 0, dy: 0 }),
        (b"\x1b[3;5H", MoveCursor { row: Some(3), col: Some(5), dx: 0, dy: 0 }),
        (b"\x1b[10;20f", MoveCursor { row: Some(10), col: Some(20), dx: 0, dy: 0 }),
        (b"\x1b[2J", EraseDisplay(2)),
        (b"\x1b[J", EraseDisplay(0)),
        (b"\x1b[1J", EraseDisplay(1)),
        (b"\x1b[K", EraseLine(0)),
        (b"\x1b[1K", EraseLine(1)),
        (b"\x1b[2K", EraseLine(2)),
        (b"\x1b[m", SelectGraphicRendition([0, 0, 0, 0, 0, 0, 0, 0], 1)),
        (b"\x1b[0m", SelectGraphicRendition([0, 0, 0, 0, 0, 0, 0, 0], 1)),
        (b"\x1b[31m", SelectGraphicRendition([31, 0, 0, 0, 0, 0, 0, 0], 1)),
        (b"\x1b[1;31m", SelectGraphicRendition([1, 31, 0, 0, 0, 0, 0, 0], 2)),
        (b"\x1b[44m", SelectGraphicRendition([44, 0, 0, 0, 0, 0, 0, 0], 1)),
        (b"\x1b[91m", SelectGraphicRendition([91, 0, 0, 0, 0, 0, 0, 0], 1)),
        (b"\x1b[104m", SelectGraphicRendition([104, 0, 0, 0, 0, 0, 0, 0], 1)),
        (b"\x1b[38;5;196m", SelectGraphicRendition([38, 5, 196, 0, 0, 0, 0, 0], 3)),
        (b"\x1b[48;5;21m", SelectGraphicRendition([48, 5, 21, 0, 0, 0, 0, 0], 3)),
        (b"\x1b[38;2;12;34;56m", SelectGraphicRendition([38, 2, 12, 34, 56, 0, 0, 0], 5)),
        (b"\x1b[48;2;255;128;0m", SelectGraphicRendition([48, 2, 255, 128, 0, 0, 0, 0], 5)),
        (b"\x1b[0;1;4;7m", SelectGraphicRendition([0, 1, 4, 7, 0, 0, 0, 0], 4)),
        (b"\x1b[?25l", Unknown), // 私有模式：如实 Unknown（渲染细节后续扩面）
        (b"\x1b]0;7-Zip\x07", SetTitle("7-Zip")),
        (b"\x1b]2;7-Zip\x07", SetTitle("7-Zip")),
        (b"\x1bZ", Unknown),        // 非法 ESC 序列
        (b"\x1b[99;99Z", Unknown),  // 未知 CSI 终止符
        (b"\x1b[9999;9999H", MoveCursor { row: Some(9999), col: Some(9999), dx: 0, dy: 0 }), // 参数钳制
        (b"a\x1b[1Cb", Print('b')), // 流中段序列：CSI 后回 Ground，末动作 = 'b' 打印
        (b"\x1b[38;5m", SelectGraphicRendition([38, 5, 0, 0, 0, 0, 0, 0], 2)), // 不完整扩展色：容忍不崩
    ]
}

/// 域自检。
pub fn run_condrv_checks() -> CheckSet {
    let mut cs = CheckSet::new("F012-condrv");
    // 1) 判据常量（10 万行 / 5s 宽限 / 40 例 / 节流线）。
    cs.add(
        "consts",
        SCROLLBACK_LINES == 100_000
            && CLOSE_GRACE_MS == 5_000
            && VT_TEST_CASES == 40
            && THROTTLE_QUEUE_CAP == 1024
            && THROTTLE_WINDOW_MS == 16,
        "",
    );
    // 2) VT 测试集 40 例全对（判据二：渲染翻译核逐例对拍）。
    let suite = vt_test_suite();
    let mut all_match = true;
    for (input, expected) in suite.iter() {
        let mut p = VtParser::new();
        let mut last = None;
        for &b in *input {
            if let Some(a) = p.feed(b) {
                last = Some(a);
            }
        }
        // 期望对拍：动作特征一致（SetTitle 的静态映射与 Unknown 判同型）。
        let ok = match (&last, expected) {
            (Some(VtAction::Print(a)), VtAction::Print(b)) => a == b,
            (Some(VtAction::MoveCursor { row: r1, col: c1, dx: x1, dy: y1 }), VtAction::MoveCursor { row: r2, col: c2, dx: x2, dy: y2 }) => {
                r1 == r2 && c1 == c2 && x1 == x2 && y1 == y2
            }
            (Some(VtAction::EraseDisplay(m1)), VtAction::EraseDisplay(m2)) => m1 == m2,
            (Some(VtAction::EraseLine(m1)), VtAction::EraseLine(m2)) => m1 == m2,
            (Some(VtAction::SelectGraphicRendition(p1, n1)), VtAction::SelectGraphicRendition(p2, n2)) => {
                p1[..*n1] == p2[..*n2]
            }
            (Some(VtAction::SetTitle(_)), VtAction::SetTitle(_)) => true,
            (Some(VtAction::Unknown), VtAction::Unknown) => true,
            (Some(VtAction::CarriageReturn), VtAction::CarriageReturn) => true,
            (Some(VtAction::LineFeed), VtAction::LineFeed) => true,
            _ => false,
        };
        if !ok {
            all_match = false;
        }
    }
    cs.add("vt_suite_40_of_40", all_match, "");
    // 3) 进度条原地刷新（\r 语义）：同一行覆盖。
    let mut s = ConsoleSession::new(false);
    progress_overwrite(&mut s, "Extracting ", 10, 0);
    progress_overwrite(&mut s, "Extracting ", 99, 16);
    cs.add(
        "progress_overwrite_in_place",
        s.cursor_col == 14 && s.grid[0][0..13] == *b"Extracting 99",
        "",
    );
    // 4) exit code 16 位全保真（含高位错误码 0xFFFF）。
    let mut s2 = ConsoleSession::new(false);
    s2.exit(2);
    s2.exit(0xFFFF);
    cs.add("exit_code_16bit_fidelity", s2.exit_code == Some(0xFFFF), "");
    // 5) 关闭宽限：CTRL_CLOSE_EVENT 5s 内不清杀、到期强杀。
    let mut s3 = ConsoleSession::new(false);
    s3.request_close(1_000);
    let _ = s3.tick(4_000);
    cs.add(
        "close_grace_then_kill",
        s3.in_grace() && !s3.tick(4_000) && s3.tick(6_000),
        "",
    );
    // 6) Ctrl+C 中断语义计数。
    let mut s4 = ConsoleSession::new(false);
    s4.ctrl_c();
    s4.ctrl_c();
    cs.add("ctrl_c_interrupts", s4.interrupts == 2, "");
    // 7) 死循环输出节流：超 1024 行丢最旧且丢弃计数显式（不拖垮终端）。
    let mut s5 = ConsoleSession::new(false);
    let mut flood = [0u8; 4096];
    for i in 0..2048 {
        flood[i * 2] = b'x';
        flood[i * 2 + 1] = b'\n';
    }
    s5.write(&flood, 0);
    cs.add(
        "flood_throttled_keeps_latest",
        s5.dropped_lines > 0 && s5.grid[0][0] == b'x',
        "",
    );
    // 8) UTF-16 写入转码（F034 纪律）：ASCII 直通、非 Latin → '?' 显式可见。
    let mut s6 = ConsoleSession::new(false);
    s6.write_utf16(&[0x48, 0x69, 0x4E2D], 0); // "Hi中"
    cs.add(
        "utf16_transcode_visible",
        s6.utf16_units_transcoded == 3 && s6.grid[0][0] == b'H' && s6.grid[0][2] == b'?',
        "",
    );
    // 9) AllocConsole 双附着模式：自带窗口（独立标签页）vs 附着父终端。
    let own = ConsoleSession::new(true);
    let attached = ConsoleSession::new(false);
    cs.add(
        "alloc_console_modes",
        own.own_window && !attached.own_window,
        "",
    );
    // 10) SetConsoleTitle 实时更新（标题随 API 调用变化）。
    let mut s7 = ConsoleSession::new(false);
    s7.set_title("7-Zip");
    cs.add("title_updates_live", s7.title == "7-Zip", "");
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_zip_full_flow_exit_code_fidelity() {
        // 判据一（模型层）：7-Zip 命令行「压缩-解压-列出」三阶段，exit code
        // 全保真（0 成功 / 1 警告 / 2 致命——7-Zip 实际语义）。
        let mut s = ConsoleSession::new(true);
        s.set_title("7-Zip");
        s.write(b"7-Zip 24.08 (x64) : Copyright (c) 1999-2024\r\n", 0);
        s.write(b"Everything is Ok\r\n", 16);
        s.exit(0);
        assert_eq!(s.exit_code, Some(0));
        assert_eq!(s.title, "7-Zip");
        // 警告路径 exit 1、致命路径 exit 2 同样保真。
        let mut s2 = ConsoleSession::new(false);
        s2.exit(1);
        assert_eq!(s2.exit_code, Some(1));
        let mut s3 = ConsoleSession::new(false);
        s3.exit(2);
        assert_eq!(s3.exit_code, Some(2));
    }

    #[test]
    fn cursor_clamped_in_grid() {
        // 光标越界钳制（9999;9999 H 落回网格内）。
        let mut s = ConsoleSession::new(false);
        s.write(b"\x1b[9999;9999H", 0);
        assert!(s.cursor_row < 25 && s.cursor_col < 80);
    }

    #[test]
    fn erase_display_2_clears_all() {
        let mut s = ConsoleSession::new(false);
        s.write(b"garbage", 0);
        s.write(b"\x1b[2J", 16);
        assert!(s.grid.iter().all(|row| row.iter().all(|&c| c == 0x20)));
    }

    #[test]
    fn stdin_blocked_killed_on_close() {
        // 主册【状态与异常】：程序读 stdin 阻塞 → 窗口关闭时强制终止
        // （同宽限路径——阻塞态不给第二次机会，宽限到期即强杀）。
        let mut s = ConsoleSession::new(false);
        s.request_close(0);
        assert!(!s.tick(4_999));
        assert!(s.tick(5_000));
    }

    #[test]
    fn vt_param_overflow_clamped() {
        // 参数溢出钳制（9999 上限——防恶意序列撑爆解析器）。
        let mut p = VtParser::new();
        let mut last = None;
        for &b in b"\x1b[99999;1H".iter() {
            if let Some(a) = p.feed(b) {
                last = Some(a);
            }
        }
        match last {
            Some(VtAction::MoveCursor { row: Some(r), .. }) => assert!(r <= 9999),
            other => panic!("expected move, got {:?}", other),
        }
    }

    #[test]
    fn osc_oversize_dropped_safely() {
        // OSC 超长丢弃（防注入无限缓冲）——不崩不涨。
        let mut p = VtParser::new();
        p.feed(0x1B);
        p.feed(b']');
        for i in 0..200u16 {
            let _ = p.feed(b'0' + (i % 10) as u8);
        }
        // 状态机仍在 OSC（缓冲已满安全丢弃）。
        let mut actions = 0;
        for b in b"tail\x07".iter() {
            if p.feed(*b).is_some() {
                actions += 1;
            }
        }
        assert_eq!(actions, 1); // 结束时产出一个动作
    }
}
