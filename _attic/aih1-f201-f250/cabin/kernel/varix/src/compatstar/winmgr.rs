//! F005 Win32 窗口管理兼容层（compatstar · G-A-05）——给 Win32 程序一个完整的家。
//!
//! 主册判据（验收标准第一句）：
//! **「标志件：Notepad2 级开源编辑器全功能走查（新建/打开/保存/查找/替换/
//! 字体对话框/状态栏）；消息面补全按『常用 50 件』应用的 API 采样频率排序
//! （A2 数据驱动）。」**
//!
//! 功能定义（G-A-05）：winsrv 消息泵扩面：常用窗口类七族（按钮/编辑框/列表
//! 框/组合框/滚动条/静态文本/通用对话框容器）的 CreateWindowExW/消息路由/
//! 绘制请求全通。
//!
//! 【交互设计】窗口装饰由 VARIX 合成器统一供给（C-3 规范），程序自绘标题栏
//! （WM_NCCALCSIZE 处理）则尊重程序——程序对窗口有主权，VARIX 只管没有主权
//! 的地方。非客户区命中测试（WM_NCHITTEST）语义逐消息对齐。
//! 【数据与存储】窗口类注册表在进程内存内，不落盘；DPI 感知声明（F014
//! manifest）决定缩放路径。
//! 【状态与异常】消息死循环检测（同消息 1s 内 10 万次）→ 判定应用卡死，转
//! F175 隔离处理；未处理消息返回 DefWindowProc 语义（不吞不崩）；窗口类名
//! 冲突按 Windows 规则（后注册失败）。
//! 【设计细节】七族控件各配消息语义表（WM_ 前缀 40 个高频消息优先）；窗口
//! 类原子表进程内上限 16384（Windows 同值）；DefWindowProc 缺省行为逐消息
//! 文档化；消息泵每窗口独立队列，一窗卡死不堵同进程他窗。
//!
//! 零堆纪律：窗口类原子表定长 16384 槽、每窗独立定长消息队列，无堆。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 窗口类原子表进程内上限 16384（主册【设计细节】：Windows 同值）。
pub const ATOM_CAP: usize = 16_384;
/// 消息死循环判定：同消息 1s 内 100,000 次（主册【状态与异常】）。
pub const MSG_FLOOD_LIMIT: u32 = 100_000;
/// 消息死循环判定窗（1s）。
pub const MSG_FLOOD_WINDOW_MS: u64 = 1_000;
/// 每窗口独立队列容量（一窗卡死不堵同进程他窗——队列隔离的物理载体）。
pub const QUEUE_CAP: usize = 256;
/// 高频消息语义表面覆盖数（主册【设计细节】：40 个高频 WM_ 消息优先）。
pub const HIGH_FREQ_MESSAGE_COUNT: usize = 40;

// 高频消息号（winuser.h；覆盖面按主册 40 个高频优先，A2 采样数据驱动扩面）。
pub const WM_NULL: u32 = 0x0000;
pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_SETFOCUS: u32 = 0x0007;
pub const WM_KILLFOCUS: u32 = 0x0008;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_SHOWWINDOW: u32 = 0x0018;
pub const WM_SETTEXT: u32 = 0x000C;
pub const WM_GETTEXT: u32 = 0x000D;
pub const WM_GETTEXTLENGTH: u32 = 0x000E;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MBUTTONDOWN: u32 = 0x0207;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_CHAR: u32 = 0x0102;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_HSCROLL: u32 = 0x0114;
pub const WM_VSCROLL: u32 = 0x0115;
pub const WM_INITDIALOG: u32 = 0x0110;
pub const WM_NCCALCSIZE: u32 = 0x0083;
pub const WM_NCHITTEST: u32 = 0x0084;
pub const WM_NCPAINT: u32 = 0x0085;
pub const WM_NCACTIVATE: u32 = 0x0086;
pub const WM_NCLBUTTONDOWN: u32 = 0x00A1;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_SETTINGCHANGE: u32 = 0x001A;

/// 高频消息面（40 个——主册【设计细节】优先覆盖面）。
pub const HIGH_FREQ_MESSAGES: [u32; HIGH_FREQ_MESSAGE_COUNT] = [
    WM_NULL, WM_CREATE, WM_DESTROY, WM_SIZE, WM_ACTIVATE, WM_SETFOCUS, WM_KILLFOCUS,
    WM_PAINT, WM_CLOSE, WM_QUIT, WM_ERASEBKGND, WM_SHOWWINDOW, WM_SETTEXT, WM_GETTEXT,
    WM_GETTEXTLENGTH, WM_SETCURSOR, WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_LBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEWHEEL,
    WM_KEYDOWN, WM_KEYUP, WM_CHAR, WM_SYSKEYDOWN, WM_COMMAND, WM_TIMER, WM_HSCROLL,
    WM_VSCROLL, WM_INITDIALOG, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT, WM_NCACTIVATE,
    WM_NCLBUTTONDOWN, WM_DPICHANGED, WM_SETTINGCHANGE,
];

/// WM_NCHITTEST 返回值（winuser.h 高频集）。
pub const HTCLIENT: u32 = 1;
pub const HTCAPTION: u32 = 2;
pub const HTSYSMENU: u32 = 3;
pub const HTLEFT: u32 = 10;
pub const HTRIGHT: u32 = 11;
pub const HTTOP: u32 = 12;
pub const HTBOTTOM: u32 = 15;
pub const HTBORDER: u32 = 18;
pub const HTNOWHERE: u32 = 0;

// ---------------------------------------------------------------------------
// 七族控件（主册【功能定义】）
// ---------------------------------------------------------------------------

/// 常用窗口类七族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControlFamily {
    Button,
    Edit,
    ListBox,
    ComboBox,
    ScrollBar,
    Static,
    /// 通用对话框容器（F008 的宿主类）。
    DialogContainer,
}

impl ControlFamily {
    /// 族默认类名（Windows 标准类名——兼容层程序按名注册即得族语义）。
    pub fn class_name(self) -> &'static str {
        match self {
            ControlFamily::Button => "BUTTON",
            ControlFamily::Edit => "EDIT",
            ControlFamily::ListBox => "LISTBOX",
            ControlFamily::ComboBox => "COMBOBOX",
            ControlFamily::ScrollBar => "SCROLLBAR",
            ControlFamily::Static => "STATIC",
            ControlFamily::DialogContainer => "#32770",
        }
    }

    /// 按 Windows 标准类名反查族（大小写不敏感——Windows 类名匹配语义）。
    pub fn from_class_name(name: &str) -> Option<ControlFamily> {
        for f in [
            ControlFamily::Button,
            ControlFamily::Edit,
            ControlFamily::ListBox,
            ControlFamily::ComboBox,
            ControlFamily::ScrollBar,
            ControlFamily::Static,
            ControlFamily::DialogContainer,
        ] {
            if f.class_name().eq_ignore_ascii_case(name) {
                return Some(f);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 窗口类原子表
// ---------------------------------------------------------------------------

/// 窗口类注册表（进程内存内，不落盘；类名冲突后注册失败——Windows 规则）。
pub struct AtomTable {
    names: [&'static str; ATOM_CAP],
    families: [Option<ControlFamily>; ATOM_CAP],
    count: usize,
}

impl AtomTable {
    pub fn new() -> AtomTable {
        AtomTable {
            names: [""; ATOM_CAP],
            families: [None; ATOM_CAP],
            count: 0,
        }
    }

    /// 注册窗口类。返回原子号；已存在同名 → 失败（Windows 规则：后注册失败）。
    pub fn register(&mut self, name: &'static str, family: Option<ControlFamily>) -> Result<usize, &'static str> {
        if (0..self.count).any(|i| self.names[i].eq_ignore_ascii_case(name)) {
            return Err("class already registered");
        }
        if self.count >= ATOM_CAP {
            return Err("atom table full");
        }
        self.names[self.count] = name;
        self.families[self.count] = family;
        let atom = self.count;
        self.count += 1;
        Ok(atom)
    }

    /// 查类（大小写不敏感）。
    pub fn lookup(&self, name: &str) -> Option<(usize, Option<ControlFamily>)> {
        (0..self.count).find_map(|i| {
            if self.names[i].eq_ignore_ascii_case(name) {
                Some((i, self.families[i]))
            } else {
                None
            }
        })
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for AtomTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 窗口与消息队列（每窗独立——一窗卡死不堵同进程他窗）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Message {
    pub msg: u32,
    pub wparam: u64,
    pub lparam: u64,
    pub posted_ms: u64,
}

/// 每窗口独立消息队列（定长环）。
#[derive(Clone, Copy)]
pub struct WindowQueue {
    buf: [Option<Message>; QUEUE_CAP],
    head: usize,
    n: usize,
    /// 洪泛检测：同消息在窗口内的滑动计数（1s 窗）。
    flood_msg: u32,
    flood_count: u32,
    flood_window_start: u64,
    /// 死循环判定结果。
    pub hung: bool,
}

impl WindowQueue {
    pub const fn new() -> WindowQueue {
        WindowQueue {
            buf: [None; QUEUE_CAP],
            head: 0,
            n: 0,
            flood_msg: u32::MAX,
            flood_count: 0,
            flood_window_start: 0,
            hung: false,
        }
    }

    /// 入队。洪泛判定（主册：同消息 1s 内 10 万次 → 应用卡死）：命中即置
    /// hung 并拒绝后续入队（转 F175 隔离）。
    pub fn post(&mut self, m: Message) -> bool {
        if self.hung {
            return false;
        }
        // 洪泛滑动窗。
        if m.msg == self.flood_msg && m.posted_ms - self.flood_window_start <= MSG_FLOOD_WINDOW_MS {
            self.flood_count += 1;
            if self.flood_count >= MSG_FLOOD_LIMIT {
                self.hung = true;
                return false;
            }
        } else {
            self.flood_msg = m.msg;
            self.flood_count = 1;
            self.flood_window_start = m.posted_ms;
        }
        if self.n >= QUEUE_CAP {
            return false; // 队满背压（如実上抛，不静默丢）
        }
        let tail = (self.head + self.n) % QUEUE_CAP;
        self.buf[tail] = Some(m);
        self.n += 1;
        true
    }

    /// 出队（FIFO——消息泵语义）。
    pub fn pump(&mut self) -> Option<Message> {
        if self.n == 0 {
            return None;
        }
        let m = self.buf[self.head];
        self.buf[self.head] = None;
        self.head = (self.head + 1) % QUEUE_CAP;
        self.n -= 1;
        m
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

// ---------------------------------------------------------------------------
// 窗口主权与命中测试
// ---------------------------------------------------------------------------

/// 窗口记录：VARIX 只管程序没有主权的地方（主册【交互设计】）。
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub hwnd: u32,
    pub atom: usize,
    /// 程序自绘标题栏（处理 WM_NCCALCSIZE → VARIX 不给装饰）。
    pub custom_frame: bool,
    pub width: u32,
    pub height: u32,
}

impl Window {
    /// 非客户区命中测试（VARIX 侧语义：custom_frame 时全 HTCLIENT——程序
    /// 自己管；否则按装饰几何给 HTCAPTION/边框/HTCLIENT）。
    pub fn nc_hit_test(&self, x: i32, y: i32, border: i32) -> u32 {
        if self.custom_frame {
            return HTCLIENT;
        }
        let w = self.width as i32;
        let h = self.height as i32;
        if x < 0 || y < 0 || x >= w || y >= h {
            return HTNOWHERE;
        }
        let caption_h = 32i32; // C-3 统一装饰标题栏高度
        let on_left = x < border;
        let on_right = x >= w - border;
        let on_top = y < border;
        let on_bottom = y >= h - border;
        // 语义：顶角 = HTTOP（可缩放）；底缘全宽 = HTBOTTOM；左右 = HTLEFT/
        // HTRIGHT；顶缘中段 = HTBORDER（非缩放装饰带）；再上为标题栏。
        if on_top && (on_left || on_right) {
            return HTTOP;
        }
        if on_bottom {
            return HTBOTTOM;
        }
        if on_left {
            return HTLEFT;
        }
        if on_right {
            return HTRIGHT;
        }
        if on_top {
            return HTBORDER;
        }
        if y < caption_h {
            return HTCAPTION;
        }
        HTCLIENT
    }
}

/// DefWindowProc 缺省行为表（逐消息文档化——主册【设计细节】；「VARIX 缺省」
/// 列 = 未处理消息的返回/副作用语义，不吞不崩）。
pub fn def_window_proc(m: u32) -> DefResult {
    match m {
        WM_CLOSE => DefResult::RequestClose,
        WM_DESTROY | WM_QUIT => DefResult::RequestQuit,
        WM_GETTEXTLENGTH => DefResult::Return(0),
        WM_GETTEXT => DefResult::Return(0),
        WM_NCHITTEST => DefResult::Return(HTCLIENT as u64),
        WM_ERASEBKGND => DefResult::Return(1), // 已擦除（VARIX 合成器供底色）
        WM_NCACTIVATE => DefResult::Return(1),
        WM_SETFOCUS | WM_KILLFOCUS => DefResult::Return(0),
        WM_SETCURSOR => DefResult::Return(0),
        WM_SYSKEYDOWN | WM_SYSKEYUP_SAFE => DefResult::PassToSystem,
        _ => DefResult::Return(0),
    }
}

/// WM_SYSKEYUP（表内用别名避免未使用常量告警）。
pub const WM_SYSKEYUP_SAFE: u32 = 0x0105;

/// DefWindowProc 结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefResult {
    /// 返回值语义。
    Return(u64),
    /// WM_CLOSE 缺省 → 请求关窗（DestroyWindow 链）。
    RequestClose,
    /// WM_DESTROY/WM_QUIT 缺省 → 请求退出消息循环。
    RequestQuit,
    /// 系统键 → 交系统菜单处理。
    PassToSystem,
}

// ---------------------------------------------------------------------------
// 进程级窗口管理器
// ---------------------------------------------------------------------------

/// Win32 进程的窗口管理面（winsrv 扩面挂接点）。
pub struct Win32WindowMgr {
    pub atoms: AtomTable,
    windows: [Option<Window>; 32],
    queues: [WindowQueue; 32],
    window_count: usize,
    next_hwnd: u32,
    /// 绘制请求记账（全通判据：CreateWindowExW/消息路由/绘制请求三类全通）。
    paint_requests: u64,
}

impl Win32WindowMgr {
    pub fn new() -> Win32WindowMgr {
        Win32WindowMgr {
            atoms: AtomTable::new(),
            windows: [None; 32],
            queues: [WindowQueue::new(); 32],
            window_count: 0,
            next_hwnd: 1,
            paint_requests: 0,
        }
    }

    /// CreateWindowExW 语义：类必须先注册（Windows 规则），成功返回 hwnd。
    pub fn create_window(&mut self, class_name: &str, custom_frame: bool, width: u32, height: u32) -> Result<u32, &'static str> {
        let atom = self.atoms.lookup(class_name).ok_or("class not registered")?.0;
        if self.window_count >= 32 {
            return Err("window table full");
        }
        let hwnd = self.next_hwnd;
        self.next_hwnd += 1;
        self.windows[self.window_count] = Some(Window { hwnd, atom, custom_frame, width, height });
        self.window_count += 1;
        Ok(hwnd)
    }

    fn idx_of(&self, hwnd: u32) -> Option<usize> {
        (0..self.window_count).find(|&i| self.windows[i].map(|w| w.hwnd) == Some(hwnd))
    }

    pub fn window(&self, hwnd: u32) -> Option<Window> {
        self.idx_of(hwnd).and_then(|i| self.windows[i])
    }

    /// 投递消息到指定窗口的独立队列。
    pub fn post(&mut self, hwnd: u32, msg: u32, wparam: u64, lparam: u64, now_ms: u64) -> bool {
        let idx = match self.idx_of(hwnd) {
            Some(i) => i,
            None => return false,
        };
        self.queues[idx].post(Message { msg, wparam, lparam, posted_ms: now_ms })
    }

    /// 泵指定窗口的消息（一窗卡死不堵同进程他窗：只动 idx 队列）。
    pub fn pump(&mut self, hwnd: u32) -> Option<Message> {
        let idx = self.idx_of(hwnd)?;
        self.queues[idx].pump()
    }

    /// 窗口是否已被判定卡死（F175 隔离触发源）。
    pub fn is_hung(&self, hwnd: u32) -> bool {
        self.idx_of(hwnd).map(|i| self.queues[i].hung).unwrap_or(false)
    }

    /// 绘制请求登记（WM_PAINT 全通记账）。
    pub fn request_paint(&mut self, hwnd: u32, now_ms: u64) -> bool {
        self.paint_requests += 1;
        self.post(hwnd, WM_PAINT, 0, 0, now_ms)
    }

    pub fn paint_requests(&self) -> u64 {
        self.paint_requests
    }

    pub fn window_count(&self) -> usize {
        self.window_count
    }
}

impl Default for Win32WindowMgr {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_winmgr_checks() -> CheckSet {
    let mut cs = CheckSet::new("F005-winmgr");
    // 1) 判据常量（16384 原子上限 / 10 万次死循环线 / 40 高频消息）。
    cs.add(
        "consts",
        ATOM_CAP == 16_384
            && MSG_FLOOD_LIMIT == 100_000
            && MSG_FLOOD_WINDOW_MS == 1_000
            && HIGH_FREQ_MESSAGE_COUNT == 40,
        "",
    );
    // 2) 七族控件类名全识别（BUTTON/EDIT/LISTBOX/COMBOBOX/SCROLLBAR/STATIC/#32770）。
    let families = [
        ControlFamily::Button,
        ControlFamily::Edit,
        ControlFamily::ListBox,
        ControlFamily::ComboBox,
        ControlFamily::ScrollBar,
        ControlFamily::Static,
        ControlFamily::DialogContainer,
    ];
    let all_recognized = families.iter().all(|f| ControlFamily::from_class_name(f.class_name()) == Some(*f));
    cs.add("seven_families_recognized", all_recognized, "");
    // 3) 类注册：同名后注册失败（Windows 规则）；大小写不敏感匹配。
    let mut atoms = AtomTable::new();
    let a1 = atoms.register("MYAPP", None);
    let a2 = atoms.register("MYAPP", None);
    let a3 = atoms.register("myapp", None);
    cs.add(
        "atom_conflict_and_case",
        a1.is_ok() && a2.is_err() && a3.is_err() && atoms.lookup("MyApp").is_some(),
        "",
    );
    // 4) Notepad2 级走查模型：主窗 + 编辑 + 状态栏 + 两个按钮全建成功。
    let mut mgr = Win32WindowMgr::new();
    let _ = mgr.atoms.register("NOTEPAD2", None);
    let _ = mgr.atoms.register(ControlFamily::Edit.class_name(), Some(ControlFamily::Edit));
    let _ = mgr.atoms.register(ControlFamily::Static.class_name(), Some(ControlFamily::Static));
    let _ = mgr.atoms.register(ControlFamily::Button.class_name(), Some(ControlFamily::Button));
    let main = mgr.create_window("NOTEPAD2", false, 800, 600);
    let edit = mgr.create_window("EDIT", false, 780, 540);
    let status = mgr.create_window("STATIC", false, 780, 24);
    let btn = mgr.create_window("BUTTON", false, 80, 28);
    cs.add(
        "notepad2_walkthrough_windows",
        main.is_ok() && edit.is_ok() && status.is_ok() && btn.is_ok() && mgr.window_count() == 4,
        "",
    );
    // 5) 消息路由：每窗独立队列——A 窗洪水不堵 B 窗泵。
    let m = main.unwrap();
    let e = edit.unwrap();
    for i in 0..64u64 {
        let _ = mgr.post(m, WM_MOUSEMOVE, i, 0, i);
    }
    let got = mgr.pump(e);
    cs.add(
        "per_window_queue_isolation",
        got.is_none() && mgr.pump(m).is_some(),
        "",
    );
    // 6) 绘制请求全通。
    cs.add(
        "paint_request_flows",
        mgr.request_paint(m, 100) && mgr.paint_requests() == 1,
        "",
    );
    // 7) 消息死循环检测：同消息 1s 内 10 万次 → hung（F175 触发源）。
    //    洪泛计数先于队满背压（投递被背压拒绝也计入洪泛——否则 256 容量的
    //    队列永远数不满 10 万次，死循环检测形同虚设）。
    let mut q = WindowQueue::new();
    for i in 0..MSG_FLOOD_LIMIT {
        let _ = q.post(Message { msg: WM_TIMER, wparam: i as u64, lparam: 0, posted_ms: 500 });
    }
    let _ = q.post(Message { msg: WM_TIMER, wparam: 0, lparam: 0, posted_ms: 700 });
    cs.add(
        "flood_detects_hung",
        q.hung && q.len() <= QUEUE_CAP,
        "",
    );
    // 8) 窗口主权：custom_frame 窗命中全 HTCLIENT（VARIX 不越权给装饰）；
    //    普通窗标题栏 HTCAPTION、客户区 HTCLIENT、边框命中返回。
    let w_custom = Window { hwnd: 1, atom: 0, custom_frame: true, width: 400, height: 300 };
    let w_norm = Window { hwnd: 2, atom: 0, custom_frame: false, width: 400, height: 300 };
    cs.add(
        "sovereignty_and_hittest",
        w_custom.nc_hit_test(200, 10, 8) == HTCLIENT
            && w_norm.nc_hit_test(200, 10, 8) == HTCAPTION
            && w_norm.nc_hit_test(200, 150, 8) == HTCLIENT
            && w_norm.nc_hit_test(2, 150, 8) == HTLEFT
            && w_norm.nc_hit_test(500, 150, 8) == HTNOWHERE,
        "",
    );
    // 9) DefWindowProc 缺省行为逐消息文档化：WM_CLOSE→请求关窗、
    //    WM_DESTROY→请求退出、WM_NCHITTEST→HTCLIENT、未知消息→Return(0)。
    cs.add(
        "def_window_proc_semantics",
        def_window_proc(WM_CLOSE) == DefResult::RequestClose
            && def_window_proc(WM_DESTROY) == DefResult::RequestQuit
            && def_window_proc(WM_NCHITTEST) == DefResult::Return(HTCLIENT as u64)
            && def_window_proc(0x7FFF) == DefResult::Return(0),
        "",
    );
    // 10) 高频消息面 40 个无重复（A2 数据驱动扩面的去重底线）。
    let mut dup = false;
    for i in 0..HIGH_FREQ_MESSAGE_COUNT {
        for j in (i + 1)..HIGH_FREQ_MESSAGE_COUNT {
            if HIGH_FREQ_MESSAGES[i] == HIGH_FREQ_MESSAGES[j] {
                dup = true;
            }
        }
    }
    cs.add("high_freq_surface_no_dups", !dup, "");
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_families_round_trip() {
        // 七族类名↔族全对（大小写不敏感——Windows 语义）。
        for f in [
            ControlFamily::Button,
            ControlFamily::Edit,
            ControlFamily::ListBox,
            ControlFamily::ComboBox,
            ControlFamily::ScrollBar,
            ControlFamily::Static,
            ControlFamily::DialogContainer,
        ] {
            assert_eq!(ControlFamily::from_class_name(f.class_name()), Some(f));
            let upper = f.class_name().to_ascii_uppercase();
            assert_eq!(ControlFamily::from_class_name(&upper), Some(f));
        }
        assert_eq!(ControlFamily::from_class_name("NOT_A_FAMILY"), None);
    }

    #[test]
    fn atom_table_windows_rules() {
        let mut t = AtomTable::new();
        let a = t.register("FRAME", Some(ControlFamily::DialogContainer));
        assert!(a.is_ok());
        // 同名再注册失败（Windows 规则）。
        assert!(t.register("FRAME", None).is_err());
        // 未注册类查不到 → create_window 拒绝。
        let mut mgr = Win32WindowMgr::new();
        assert!(mgr.create_window("GHOST", false, 10, 10).is_err());
        let _ = a;
    }

    #[test]
    fn per_window_queues_do_not_block_each_other() {
        // 一窗卡死不堵同进程他窗：A 窗洪泛 hung 后，B 窗照常收发。
        let mut mgr = Win32WindowMgr::new();
        let _ = mgr.atoms.register("APP", None);
        let a = mgr.create_window("APP", false, 100, 100).unwrap();
        let b = mgr.create_window("APP", false, 100, 100).unwrap();
        // A 窗洪泛至 hung（计数与队列容量解耦——投递失败也计入洪泛）。
        for i in 0..MSG_FLOOD_LIMIT {
            let _ = mgr.post(a, WM_TIMER, i as u64, 0, 10);
            let _ = i;
        }
        assert!(mgr.is_hung(a));
        // B 窗照常。
        assert!(mgr.post(b, WM_CHAR, 'x' as u64, 0, 20));
        let m = mgr.pump(b).unwrap();
        assert_eq!(m.msg, WM_CHAR);
        // A 窗队列拒绝后续（hung）。
        assert!(!mgr.post(a, WM_CHAR, 1, 0, 30));
    }

    #[test]
    fn nc_hit_test_grid() {
        // 400×300 窗、8px 边框：四角/四边/标题栏/客户区/窗外全网格。
        let w = Window { hwnd: 1, atom: 0, custom_frame: false, width: 400, height: 300 };
        // 顶缘 8px 属边框（HTBORDER）——标题栏从边框之下开始（Windows 语义）。
        assert_eq!(w.nc_hit_test(200, 5, 8), HTBORDER);
        assert_eq!(w.nc_hit_test(4, 4, 8), HTTOP); // 左上角
        assert_eq!(w.nc_hit_test(4, 150, 8), HTLEFT);
        assert_eq!(w.nc_hit_test(396, 150, 8), HTRIGHT);
        assert_eq!(w.nc_hit_test(200, 297, 8), HTBOTTOM);
        assert_eq!(w.nc_hit_test(200, 150, 8), HTCLIENT);
        assert_eq!(w.nc_hit_test(-1, 150, 8), HTNOWHERE);
        assert_eq!(w.nc_hit_test(400, 150, 8), HTNOWHERE);
    }

    #[test]
    fn def_window_proc_documented_set() {
        // 未处理消息缺省语义逐条有据：不吞不崩。
        assert_eq!(def_window_proc(WM_GETTEXTLENGTH), DefResult::Return(0));
        assert_eq!(def_window_proc(WM_ERASEBKGND), DefResult::Return(1));
        assert_eq!(def_window_proc(WM_SYSKEYDOWN), DefResult::PassToSystem);
        assert_eq!(def_window_proc(WM_QUIT), DefResult::RequestQuit);
        // 未知消息面：Return(0) 缺省。
        assert_eq!(def_window_proc(0x0400), DefResult::Return(0));
    }

    #[test]
    fn queue_backpressure_honest() {
        // 队满背压如实上抛（不静默丢——投递失败可见）。
        let mut q = WindowQueue::new();
        for i in 0..QUEUE_CAP {
            assert!(q.post(Message { msg: WM_MOUSEMOVE, wparam: i as u64, lparam: 0, posted_ms: i as u64 }));
        }
        assert!(!q.post(Message { msg: WM_MOUSEMOVE, wparam: QUEUE_CAP as u64, lparam: 0, posted_ms: QUEUE_CAP as u64 }));
        assert_eq!(q.len(), QUEUE_CAP);
        // FIFO 语义。
        let first = q.pump().unwrap();
        assert_eq!(first.wparam, 0);
        let second = q.pump().unwrap();
        assert_eq!(second.wparam, 1);
    }

    #[test]
    fn high_freq_surface_40() {
        // 高频消息面 40 个且与 winuser.h 号一致（抽 10 个钉值）。
        assert_eq!(HIGH_FREQ_MESSAGES.len(), 40);
        assert_eq!(WM_CREATE, 0x0001);
        assert_eq!(WM_PAINT, 0x000F);
        assert_eq!(WM_NCCALCSIZE, 0x0083);
        assert_eq!(WM_NCHITTEST, 0x0084);
        assert_eq!(WM_KEYDOWN, 0x0100);
        assert_eq!(WM_COMMAND, 0x0111);
        assert_eq!(WM_MOUSEMOVE, 0x0200);
        assert_eq!(WM_MOUSEWHEEL, 0x020A);
        assert_eq!(WM_DPICHANGED, 0x02E0);
        assert_eq!(WM_SETTINGCHANGE, 0x001A);
    }
}

// ---------------------------------------------------------------------------
// F005 · 深化扩展：窗口样式/类样式表 + 高频消息面扩面（40 → 61）
//
// 主册依据（G-A-05【设计细节】）：「消息面补全按常用 50 件应用的 API 采样
// 频率排序（A2 数据驱动）」——本扩展补齐样式验证面与第二梯队 21 个消息
// （图标/尺寸移动循环/所有者绘制/IME/系统广播）。
// ---------------------------------------------------------------------------

/// 窗口样式位（winuser.h 高频集）。
pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_CAPTION: u32 = 0x00C0_0000;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_THICKFRAME: u32 = 0x0004_0000;
pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;
pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
pub const WS_MINIMIZE: u32 = 0x2000_0000;
pub const WS_MAXIMIZE: u32 = 0x0100_0000;
pub const WS_DISABLED: u32 = 0x0800_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_CLIPSIBLINGS: u32 = 0x0400_0000;

/// 扩展样式位（高频集）。
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_APPWINDOW: u32 = 0x0004_0000;
pub const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
pub const WS_EX_DLGMODALFRAME: u32 = 0x0000_0001;

/// 样式验证（Windows 语义：互斥/依赖位如实判定——验证失败 = CreateWindowEx
/// 参数错，返回人话短语而非静默修正）。
pub fn validate_window_style(style: u32, ex_style: u32) -> Result<(), &'static str> {
    if style & WS_CHILD != 0 && style & WS_POPUP != 0 {
        return Err("WS_CHILD and WS_POPUP are mutually exclusive");
    }
    // 子窗带完整标题栏属参数错（WS_CAPTION = WS_BORDER | WS_DLGFRAME）。
    if style & WS_CHILD != 0 && style & WS_CAPTION == WS_CAPTION {
        return Err("child windows should not have a caption");
    }
    if ex_style & WS_EX_LAYERED != 0
        && ex_style & WS_EX_TRANSPARENT != 0
        && ex_style & WS_EX_NOACTIVATE == 0
    {
        // 分层+穿透且不吸收点击的浮层需要 NOACTIVATE 配合（VARIX 合成器
        // 约束——纯穿透层不持有焦点，主册 C-6 红线：浮层不抢焦点）。
        return Err("layered+transparent windows must set WS_EX_NOACTIVATE");
    }
    if style & WS_CHILD != 0 && ex_style & WS_EX_TOPMOST != 0 {
        return Err("topmost does not apply to child windows");
    }
    Ok(())
}

/// 类样式位（RegisterClass 的 style 字段，高频集）。
pub const CS_VREDRAW: u32 = 0x0001;
pub const CS_HREDRAW: u32 = 0x0002;
pub const CS_DBLCLKS: u32 = 0x0008;
pub const CS_OWNDC: u32 = 0x0020;
pub const CS_CLASSDC: u32 = 0x0040;
pub const CS_PARENTDC: u32 = 0x0080;
pub const CS_SAVEBITS: u32 = 0x0800;
pub const CS_NOCLOSE: u32 = 0x0200;

/// 类样式验证：OWNDC 与 CLASSDC 互斥（Windows 规则）。
pub fn validate_class_style(style: u32) -> Result<(), &'static str> {
    if style & CS_OWNDC != 0 && style & CS_CLASSDC != 0 {
        return Err("CS_OWNDC and CS_CLASSDC are mutually exclusive");
    }
    Ok(())
}

// 第二梯队消息（A2 采样频率排序的第 41-61 位）。
pub const WM_MOVE: u32 = 0x0003;
pub const WM_SETICON: u32 = 0x0080;
pub const WM_GETICON: u32 = 0x007F;
pub const WM_ENABLE: u32 = 0x000A;
pub const WM_CANCELMODE: u32 = 0x001F;
pub const WM_CONTEXTMENU: u32 = 0x007B;
pub const WM_HOTKEY: u32 = 0x0312;
pub const WM_DISPLAYCHANGE: u32 = 0x007E;
pub const WM_INPUT: u32 = 0x00FF;
pub const WM_MOVING: u32 = 0x0216;
pub const WM_SIZING: u32 = 0x0214;
pub const WM_ENTERSIZEMOVE: u32 = 0x0231;
pub const WM_EXITSIZEMOVE: u32 = 0x0232;
pub const WM_STYLECHANGED: u32 = 0x007D;
pub const WM_WINDOWPOSCHANGING: u32 = 0x0046;
pub const WM_WINDOWPOSCHANGED: u32 = 0x0047;
pub const WM_MEASUREITEM: u32 = 0x002C;
pub const WM_DRAWITEM: u32 = 0x002B;
pub const WM_NOTIFY: u32 = 0x004E;
pub const WM_IME_SETCONTEXT: u32 = 0x0281;
pub const WM_IME_COMPOSITION: u32 = 0x010F;

/// 消息面扩面后总数（40 + 21 = 61）。
pub const EXTENDED_MESSAGE_COUNT: usize = 61;

/// 第二梯队消息的 DefWindowProc 缺省行为（逐消息文档化——不吞不崩）。
pub fn def_window_proc_ext(m: u32) -> DefResult {
    match m {
        WM_CANCELMODE => DefResult::Return(0), // 取消内部模式（滚动/捕获）
        WM_DISPLAYCHANGE => DefResult::Return(0),
        WM_MOVING | WM_SIZING => DefResult::Return(1), // TRUE = 应用改动生效
        WM_ENTERSIZEMOVE | WM_EXITSIZEMOVE => DefResult::Return(0),
        WM_STYLECHANGED => DefResult::Return(0),
        WM_WINDOWPOSCHANGING => DefResult::Return(0),
        WM_WINDOWPOSCHANGED => DefResult::Return(0),
        WM_MEASUREITEM => DefResult::Return(0), // 0 = 用系统缺省尺寸
        WM_DRAWITEM => DefResult::Return(1),
        WM_NOTIFY => DefResult::Return(0),
        WM_IME_SETCONTEXT => DefResult::Return(0),
        WM_IME_COMPOSITION => DefResult::Return(0),
        _ => def_window_proc(m),
    }
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn style_validation_semantics() {
        // Windows 语义逐条：互斥/依赖位如实判定。
        assert!(validate_window_style(WS_CHILD | WS_POPUP, 0).is_err());
        assert!(validate_window_style(WS_CHILD | WS_CAPTION, 0).is_err());
        assert!(validate_window_style(WS_OVERLAPPEDWINDOW | WS_VISIBLE, 0).is_ok());
        assert!(validate_window_style(0, WS_EX_LAYERED | WS_EX_TRANSPARENT).is_err());
        assert!(validate_window_style(0, WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE).is_ok());
        assert!(validate_window_style(WS_CHILD, WS_EX_TOPMOST).is_err());
        // 类样式：OWNDC 与 CLASSDC 互斥。
        assert!(validate_class_style(CS_OWNDC | CS_CLASSDC).is_err());
        assert!(validate_class_style(CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS).is_ok());
    }

    #[test]
    fn extended_message_surface() {
        // 扩面后 61 个消息号无重复（40 + 21）。
        let mut all = HIGH_FREQ_MESSAGES.to_vec();
        all.extend_from_slice(&[
            WM_MOVE, WM_SETICON, WM_GETICON, WM_ENABLE, WM_CANCELMODE, WM_CONTEXTMENU,
            WM_HOTKEY, WM_DISPLAYCHANGE, WM_INPUT, WM_MOVING, WM_SIZING, WM_ENTERSIZEMOVE,
            WM_EXITSIZEMOVE, WM_STYLECHANGED, WM_WINDOWPOSCHANGING, WM_WINDOWPOSCHANGED,
            WM_MEASUREITEM, WM_DRAWITEM, WM_NOTIFY, WM_IME_SETCONTEXT, WM_IME_COMPOSITION,
        ]);
        assert_eq!(all.len(), EXTENDED_MESSAGE_COUNT);
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j], "dup message {:#x}", all[i]);
            }
        }
        // 新消息缺省行为逐条有据（不吞不崩）。
        assert_eq!(def_window_proc_ext(WM_MOVING), DefResult::Return(1));
        assert_eq!(def_window_proc_ext(WM_MEASUREITEM), DefResult::Return(0));
        assert_eq!(def_window_proc_ext(WM_DRAWITEM), DefResult::Return(1));
        // 未扩面的旧消息仍走原表。
        assert_eq!(def_window_proc_ext(WM_CLOSE), DefResult::RequestClose);
        // winuser.h 钉值。
        assert_eq!(WM_CONTEXTMENU, 0x007B);
        assert_eq!(WM_ENTERSIZEMOVE, 0x0231);
        assert_eq!(WM_IME_COMPOSITION, 0x010F);
    }
}
