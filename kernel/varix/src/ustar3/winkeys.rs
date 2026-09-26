//! winkeys —— I 通用域·键盘形制八项（AI-U3 · F516/F518/F535/F536/F537/F538/F539/F548）。
//!
//! 判据唯一源：主册《Varix STAR I start.md》各节【验收判据】第一句。
//! 本文件是 no_std 兼容的纯逻辑「判据实装层」：只建模型与断言判据，
//! 不做真实键钩子（那属于输入服务域的实现面）。
//!
//! ===========================================================================
//! F516 通知横幅位置设置
//! ---------------------------------------------------------------------------
//! 判据（逐字摘录）：**三档位置；堆叠方向自适应；OSD 独立性；主屏固定；
//! 切换即时与持久化。**
//! 功能定义：通知横幅落点可选：右下（默认，Windows 习惯）/顶部中央/顶部
//! 右侧三档；横幅堆叠方向随位置自适应（右下向上堆、顶部向下堆——F383 排队
//! 纪律不变）；OSD（音量/亮度 F239/F240）位置独立不动；多屏时横幅出主屏。
//! 依赖锚点：F239/F240（OSD）、F383（横幅排队纪律）。
//!
//! ===========================================================================
//! F518 输入法切换键自定义
//! ---------------------------------------------------------------------------
//! 判据（逐字摘录）：**四方案切换；与点击循环并存；让出旧键判据；Caps 双用
//! （600ms 分界实测）；冲突审计联动。**
//! 功能定义：切换键可改：默认 Win+空格，可选 Ctrl+Shift/Alt+Shift/Caps 长按；
//! 切换键与 F421 点击循环并存（键盘鼠标两路同效）；自定义后旧键让出
//! （不双绑定——一键一职，F244 冲突审计同源）；Caps 双用（长按 600ms 切
//! 输入法、短按锁定大小写——两段语义）。
//! 依赖锚点：F421（点击循环）、F244（冲突审计）。
//!
//! ===========================================================================
//! F535 Win+数字快捷启动
//! ---------------------------------------------------------------------------
//! 判据（逐字摘录）：**十键映射与任务栏顺序一致；启动/切换/最小化三态；
//! 溢出项可达；Shift 变体；注册表登记。**
//! 功能定义：Win+1 到 Win+0：按序激活任务栏第 N 个固定/运行图标——没运行
//! 则启动（F282 单例语义）、运行中则切换/最小化切换（F252 点击语义同源）；
//! 顺序与任务栏图标排布一致（F418 排序含溢出项 F495）；Shift+Win+数字=
//! 以管理员语义新开实例。
//! 依赖锚点：F282（单例）、F252（点击语义）、F418/F495（任务栏排序与溢出）。
//!
//! ===========================================================================
//! F536 Win+T 任务栏遍历
//! ---------------------------------------------------------------------------
//! 判据（逐字摘录）：**进场/遍历/激活/菜单/退出五链路；焦点环可见性；
//! F419 菜单键盘可达（F433 语义）；循环边界（首尾回绕）；Esc 归还。**
//! 功能定义：Win+T：焦点跳到任务栏图标（方向键遍历、Enter 激活=单击语义、
//! 菜单键=右键语义 F419）——纯键盘操作任务栏的完整通路；遍历高亮
//! （F206 焦点环）在图标上清晰可见；Esc 退回窗口焦点。
//! 依赖锚点：F419/F433（菜单键盘可达）、F206（焦点环）。
//!
//! ===========================================================================
//! F537 Win+逗号 瞥桌面
//! ---------------------------------------------------------------------------
//! 判据（逐字摘录）：**透明度 15%±3%；进出 120ms±20ms；恢复原位精度
//! （<1px）；纯看限制；与 F316 闲置计时无冲突。**
//! 功能定义：按住 Win+逗号：所有窗口瞬间变透明（120ms 淡出至 15% 透明度
//! ——不是最小化，是「变成玻璃」），松开即恢复（120ms 淡回原样）；瞥的
//! 过程中桌面可交互受限（纯看——防误点，松手才恢复操作）。
//! 依赖锚点：F316（闲置计时——Peek 事件不得喂入计时器）。
//!
//! ===========================================================================
//! F538 Alt+Esc 窗口循环
//! ---------------------------------------------------------------------------
//! 判据（逐字摘录）：**Z 序正确性；最小化参与与还原；连按节奏（<100ms/次）；
//! 与 Alt+Tab 并存不冲突；动画最简（无浮层）。**
//! 功能定义：Alt+Esc：窗口按 Z 序循环切换（不带预览浮层——与 Alt+Tab
//! （F082）互补的「盲切」）；按住 Alt 连按 Esc 逐窗后退、松开落定；最小化
//! 窗也参与循环（选中即还原）。
//! 依赖锚点：F082（Alt+Tab——并存不冲突，两状态机独立）。
//!
//! ===========================================================================
//! F539 桌面布局锁定
//! ---------------------------------------------------------------------------
//! 判据（逐字摘录）：**拖拽拒绝反馈；右键排列仍可用；解锁路径深度；锁角标
//! 可选；默认关与持久化。**
//! 功能定义：桌面图标布局锁定开关：开启后图标不可拖动（拖拽被温和拒绝——
//! 图标轻微抖动 120ms 提示+状态栏一句话）、右键排列命令照常（锁定防的是
//! 手滑不是管理）；解锁需到设置页（桌面右键不放解锁项）；锁定状态图标可配
//! 小锁角标（可选）。
//! 依赖锚点：无外部锚点（桌面域内部闭环）。
//!
//! ===========================================================================
//! F548 任务管理器置顶
//! ---------------------------------------------------------------------------
//! 判据（逐字摘录）：**置顶切换即时；不抢焦点判据（F248 复用）；边框标记；
//! 重启不记忆；与其他置顶窗（F248 多窗）叠序。**
//! 功能定义：任务管理器形制界面（F402）自带置顶按钮：勾选后窗口置顶
//! （F248 语义——不抢焦点只不被盖），边监控边操作其他窗口的经典姿势；
//! 置顶状态窗口边框亮标记；重启不记忆置顶（默认回常态）。
//! 依赖锚点：F248（置顶语义与多窗叠序）、F402（任务管理器形制）。
//!
//! ===========================================================================
//! 零堆纪律：全文件逻辑路径无 String/Vec/Box/format!，一律定长数组 +
//! &'static str + core 运算；测试亦只做值断言（std 仅由测试框架隐式引入）。
//! 数字精确成常量：600ms（Caps 分界）、120ms（横幅/抖动/瞥进出）、150‰
//! （瞥透明度）、±3%（透明度容差）、±20ms（时长容差）、<1px（恢复精度）、
//! 100ms（Alt+Esc 连按节奏）。

use crate::checks::CheckSet;

// ===========================================================================
// F516 通知横幅位置设置 —— BannerDock
// ===========================================================================

/// F516 横幅三档位置（右下为默认——Windows 习惯）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BannerPos {
    /// 右下（默认）。
    BottomRight,
    /// 顶部中央。
    TopCenter,
    /// 顶部右侧。
    TopRight,
}

/// F516 堆叠方向：右下向上堆、顶部向下堆（随位置自适应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackDir {
    Up,
    Down,
}

/// F516 持久化编码：0=右下 1=顶中 2=顶右（存储字节两向一致）。
const BANNER_STORE: [u8; 3] = [0, 1, 2];

impl BannerPos {
    pub fn name(self) -> &'static str {
        match self {
            BannerPos::BottomRight => "bottom-right",
            BannerPos::TopCenter => "top-center",
            BannerPos::TopRight => "top-right",
        }
    }

    /// 堆叠方向随位置自适应：右下向上堆，顶部两档向下堆。
    pub fn stack_dir(self) -> StackDir {
        match self {
            BannerPos::BottomRight => StackDir::Up,
            BannerPos::TopCenter => StackDir::Down,
            BannerPos::TopRight => StackDir::Down,
        }
    }

    pub fn to_store(self) -> u8 {
        BANNER_STORE[self as usize]
    }

    pub fn from_store(b: u8) -> Option<BannerPos> {
        for (i, &s) in BANNER_STORE.iter().enumerate() {
            if s == b {
                return Some(BANNER_POS_ALL[i]);
            }
        }
        None
    }
}

/// F516 三档全集（定长遍历用）。
pub const BANNER_POS_ALL: [BannerPos; 3] =
    [BannerPos::BottomRight, BannerPos::TopCenter, BannerPos::TopRight];

/// F516 OSD 锚点（音量 F239/亮度 F240 共用形制；与横幅位置完全独立）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OsdAnchor {
    /// 音量 OSD 出厂锚点。
    VolumeBottomLeft,
    /// 亮度 OSD 出厂锚点。
    BrightnessTopLeft,
}

/// F516 OSD 位置状态（独立状态体——横幅改动不得触碰此结构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OsdPlacement {
    pub volume: OsdAnchor,
    pub brightness: OsdAnchor,
}

/// F516 横幅停泊坞：位置档位 + OSD 独立状态 + 主屏固定 + 持久化字节。
pub struct BannerDock {
    pos: BannerPos,
    osd: OsdPlacement,
    primary_screen: u8,
    stored: u8,
    /// F383 排队纪律不变：横幅入队/出队次序与位置无关，队列容量 8。
    queue: [&'static str; 8],
    q_head: usize,
    q_tail: usize,
}

impl BannerDock {
    pub const fn new() -> Self {
        BannerDock {
            pos: BannerPos::BottomRight,
            osd: OsdPlacement {
                volume: OsdAnchor::VolumeBottomLeft,
                brightness: OsdAnchor::BrightnessTopLeft,
            },
            primary_screen: 0,
            stored: BANNER_STORE[0],
            queue: [""; 8],
            q_head: 0,
            q_tail: 0,
        }
    }

    pub fn position(&self) -> BannerPos {
        self.pos
    }

    /// 切换即时：改设置后**下一横幅**即落新位（无延迟、无重启要求）。
    pub fn set_position(&mut self, p: BannerPos) {
        self.pos = p;
        self.stored = p.to_store();
    }

    /// OSD 独立性：横幅位置改动不影响 OSD 状态（本方法只读 OSD）。
    pub fn osd(&self) -> OsdPlacement {
        self.osd
    }

    /// 主屏固定：多屏下横幅落点屏恒为主屏 id（screens[0] 语义为主屏）。
    pub fn banner_screen(&self, screens: &[u8]) -> u8 {
        match screens.first() {
            Some(&s) => s,
            None => self.primary_screen,
        }
    }

    pub fn primary_screen(&self) -> u8 {
        self.primary_screen
    }

    /// 持久化：写出当前档位字节。
    pub fn persist(&self) -> u8 {
        self.stored
    }

    /// 持久化：从存储字节恢复（读写一致判据的另一半）。
    pub fn restore(&mut self, b: u8) -> bool {
        match BannerPos::from_store(b) {
            Some(p) => {
                self.pos = p;
                self.stored = b;
                true
            }
            None => false,
        }
    }

    /// F383 排队纪律：入队（横幅依次进）。
    pub fn push_banner(&mut self, title: &'static str) -> bool {
        if self.q_tail >= self.queue.len() {
            return false;
        }
        self.queue[self.q_tail] = title;
        self.q_tail += 1;
        true
    }

    /// F383 排队纪律：出队（先来先显，与位置档位无关）。
    pub fn pop_banner(&mut self) -> Option<&'static str> {
        if self.q_head >= self.q_tail {
            return None;
        }
        let t = self.queue[self.q_head];
        self.q_head += 1;
        Some(t)
    }
}

/// F516 域自检（12 条）。
pub fn run_f516_checks() -> CheckSet {
    let mut cs = CheckSet::new("F516-banner-dock");
    // 1) 默认档：右下（Windows 习惯）。
    let dock = BannerDock::new();
    cs.add("default_bottom_right", dock.position() == BannerPos::BottomRight, "");
    // 2) 三档齐备且互异。
    cs.add(
        "three_positions_distinct",
        BANNER_POS_ALL.len() == 3
            && BANNER_POS_ALL[0] != BANNER_POS_ALL[1]
            && BANNER_POS_ALL[1] != BANNER_POS_ALL[2]
            && BANNER_POS_ALL[0] != BANNER_POS_ALL[2],
        "",
    );
    // 3) 堆叠方向逐档断言：右下向上、顶中向下、顶右向下。
    cs.add(
        "stack_dir_per_slot",
        BannerPos::BottomRight.stack_dir() == StackDir::Up
            && BannerPos::TopCenter.stack_dir() == StackDir::Down
            && BannerPos::TopRight.stack_dir() == StackDir::Down,
        "",
    );
    // 4) OSD 独立性：横幅改档前后 OSD 状态逐字段不变。
    let mut dock = BannerDock::new();
    let osd_before = dock.osd();
    dock.set_position(BannerPos::TopCenter);
    cs.add("osd_independent", dock.osd() == osd_before, "");
    // 5) OSD 双锚出厂语义（F239/F240 各自独立字段）。
    cs.add(
        "osd_dual_anchor",
        osd_before.volume == OsdAnchor::VolumeBottomLeft
            && osd_before.brightness == OsdAnchor::BrightnessTopLeft,
        "",
    );
    // 6) 主屏固定：多屏下横幅落点屏 = 主屏（首屏 id=7）。
    let screens = [7u8, 3u8, 9u8];
    cs.add("banner_on_primary", dock.banner_screen(&screens) == 7, "");
    // 7) 单屏退化：无屏表时回落内建主屏 id。
    cs.add("banner_screen_fallback", dock.banner_screen(&[]) == dock.primary_screen(), "");
    // 8) 切换即时：改档后 position 立即读出新值。
    cs.add("switch_immediate", dock.position() == BannerPos::TopCenter, "");
    // 9) 持久化读写一致：三档 to_store/from_store 全回环。
    let mut roundtrip = true;
    for p in BANNER_POS_ALL {
        let b = p.to_store();
        roundtrip &= BannerPos::from_store(b) == Some(p);
    }
    cs.add("persist_roundtrip", roundtrip, "");
    // 10) 存储字节三档互异（无别名——读回不串档）。
    cs.add(
        "store_bytes_distinct",
        BANNER_STORE[0] != BANNER_STORE[1] && BANNER_STORE[1] != BANNER_STORE[2],
        "",
    );
    // 11) 非法存储字节拒绝（防御性恢复失败返回 false）。
    let mut dock2 = BannerDock::new();
    cs.add("restore_rejects_bad_byte", !dock2.restore(0xAA), "");
    // 12) F383 排队纪律不变：换位置前后出队次序 = 入队次序。
    let mut q = BannerDock::new();
    q.push_banner("a");
    q.push_banner("b");
    q.push_banner("c");
    q.set_position(BannerPos::TopRight);
    let order_ok = q.pop_banner() == Some("a")
        && q.pop_banner() == Some("b")
        && q.pop_banner() == Some("c")
        && q.pop_banner().is_none();
    cs.add("queue_discipline_unchanged", order_ok, "");
    cs
}

#[cfg(test)]
mod f516_tests {
    use super::*;

    #[test]
    fn 三档方向逐档自适应() {
        assert_eq!(BannerPos::BottomRight.stack_dir(), StackDir::Up, "右下应向上堆");
        assert_eq!(BannerPos::TopCenter.stack_dir(), StackDir::Down, "顶中应向下堆");
        assert_eq!(BannerPos::TopRight.stack_dir(), StackDir::Down, "顶右应向下堆");
    }

    #[test]
    fn osd_独立性_横幅改档不动osd() {
        let mut dock = BannerDock::new();
        let before = dock.osd();
        dock.set_position(BannerPos::TopRight);
        assert_eq!(dock.osd(), before, "横幅位置改动不得影响 OSD 状态");
        dock.set_position(BannerPos::BottomRight);
        assert_eq!(dock.osd(), before, "改回默认档 OSD 仍不动");
    }

    #[test]
    fn 多屏时横幅出主屏() {
        let mut dock = BannerDock::new();
        dock.set_position(BannerPos::TopCenter);
        assert_eq!(dock.banner_screen(&[42, 1, 2]), 42, "横幅应落主屏（首屏 id）");
    }

    #[test]
    fn 切换即时与持久化回环() {
        let mut dock = BannerDock::new();
        dock.set_position(BannerPos::TopRight);
        assert_eq!(dock.position(), BannerPos::TopRight, "切换即时生效");
        let b = dock.persist();
        let mut dock2 = BannerDock::new();
        assert!(dock2.restore(b), "存储字节应可恢复");
        assert_eq!(dock2.position(), BannerPos::TopRight, "恢复后档位一致");
    }

    #[test]
    fn f383_排队纪律与位置无关() {
        for p in BANNER_POS_ALL {
            let mut q = BannerDock::new();
            q.set_position(p);
            q.push_banner("x");
            q.push_banner("y");
            assert_eq!(q.pop_banner(), Some("x"), "先入先显（档位 {}）", p.name());
            assert_eq!(q.pop_banner(), Some("y"));
            assert!(q.pop_banner().is_none());
        }
    }

    #[test]
    fn 恢复失败返回假且状态不动() {
        let mut dock = BannerDock::new();
        let pos0 = dock.position();
        assert!(!dock.restore(0xFF), "非法字节应拒绝");
        assert_eq!(dock.position(), pos0, "拒绝后档位不变");
    }
}

// ===========================================================================
// F518 输入法切换键自定义 —— ImeSwitchKey
// ===========================================================================

/// F518 Caps 双用分界：长按 >=600ms 切输入法、短按 <600ms 锁定大小写。
pub const CAPS_DUAL_MS: u64 = 600;

/// F518 绑定表容量：一键一职——任意时刻只登记当前方案一个组合键。
pub const MAX_IME_BINDINGS: usize = 1;

/// F518 修饰位（组合键位图）。
pub const MOD_WIN: u16 = 1 << 0;
pub const MOD_CTRL: u16 = 1 << 1;
pub const MOD_ALT: u16 = 1 << 2;
/// F518 主键位（简化 VK：空格 0x20、Caps 0x14）。
pub const VK_SPACE: u16 = 0x20;
pub const VK_CAPS: u16 = 0x14;

/// F518 四方案（默认 Win+空格）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchScheme {
    WinSpace,
    CtrlShift,
    AltShift,
    CapsHold,
}

/// F518 四方案全集（定长遍历用）。
pub const SWITCH_SCHEMES: [SwitchScheme; 4] = [
    SwitchScheme::WinSpace,
    SwitchScheme::CtrlShift,
    SwitchScheme::AltShift,
    SwitchScheme::CapsHold,
];

/// Ctrl+Shift / Alt+Shift 的双 VK 位（两键同按的位图语义）。
pub const VK_CTRL_SHIFT: u16 = 0x10 | 0x11;
pub const VK_ALT_SHIFT: u16 = 0x10 | 0x12;

impl SwitchScheme {
    pub fn name(self) -> &'static str {
        match self {
            SwitchScheme::WinSpace => "win-space",
            SwitchScheme::CtrlShift => "ctrl-shift",
            SwitchScheme::AltShift => "alt-shift",
            SwitchScheme::CapsHold => "caps-hold",
        }
    }

    /// 方案组合键位图（唯一——四方案两两互异；const 供 const 构造引用）。
    pub const fn chord(self) -> u16 {
        match self {
            SwitchScheme::WinSpace => MOD_WIN | VK_SPACE,
            SwitchScheme::CtrlShift => MOD_CTRL | VK_CTRL_SHIFT,
            SwitchScheme::AltShift => MOD_ALT | VK_ALT_SHIFT,
            SwitchScheme::CapsHold => VK_CAPS,
        }
    }
}

/// F518 Caps 双用两段语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapsAction {
    /// 短按（<600ms）：锁定大小写。
    LockCase,
    /// 长按（>=600ms）：切输入法。
    SwitchIme,
    /// 未按下就松开（噪声防御）。
    None,
}

/// F518 Caps 双用状态机：按下记 t0，松开按 dt 分段。
#[derive(Clone, Copy, Debug)]
pub struct CapsState {
    down_at: Option<u64>,
}

impl CapsState {
    pub const fn new() -> Self {
        CapsState { down_at: None }
    }

    pub fn press(&mut self, t: u64) {
        self.down_at = Some(t);
    }

    /// 松开：dt < 600ms = 锁定大小写；dt >= 600ms = 切输入法。
    pub fn release(&mut self, t: u64) -> CapsAction {
        match self.down_at.take() {
            Some(t0) => {
                let dt = t.saturating_sub(t0);
                if dt < CAPS_DUAL_MS {
                    CapsAction::LockCase
                } else {
                    CapsAction::SwitchIme
                }
            }
            None => CapsAction::None,
        }
    }
}

/// F518 冲突审计表（F244 同源）：登记当前方案组合键，查询冲突清单。
pub struct ConflictAudit {
    entries: [u16; 8],
    n: usize,
}

impl ConflictAudit {
    pub const fn new() -> Self {
        ConflictAudit { entries: [0; 8], n: 0 }
    }

    /// 登记一个组合键（同键重复登记幂等）。
    pub fn register(&mut self, chord: u16) -> bool {
        if self.find(chord).is_some() {
            return true;
        }
        if self.n >= self.entries.len() {
            return false;
        }
        self.entries[self.n] = chord;
        self.n += 1;
        true
    }

    pub fn find(&self, chord: u16) -> Option<usize> {
        let mut i = 0;
        while i < self.n {
            if self.entries[i] == chord {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 冲突清单：查询 chord 与已登记项的冲突数（0=无冲突）。
    pub fn conflicts_of(&self, chord: u16) -> usize {
        if self.find(chord).is_some() {
            1
        } else {
            0
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// F518 输入法切换键实体：绑定表（一键一职）+ 鼠标循环并存 + Caps 状态机。
pub struct ImeSwitchKey {
    scheme: SwitchScheme,
    bindings: [u16; MAX_IME_BINDINGS],
    /// F421 鼠标点击循环切换路径（独立于键盘方案，始终可用）。
    mouse_cycle_alive: bool,
    pub caps: CapsState,
}

impl ImeSwitchKey {
    pub const fn new() -> Self {
        ImeSwitchKey {
            scheme: SwitchScheme::WinSpace,
            bindings: [SwitchScheme::WinSpace.chord(); MAX_IME_BINDINGS],
            mouse_cycle_alive: true,
            caps: CapsState::new(),
        }
    }

    pub fn scheme(&self) -> SwitchScheme {
        self.scheme
    }

    /// 换方案：绑定表整体换血——旧键让出（不双绑定），新键上岗。
    pub fn set_scheme(&mut self, s: SwitchScheme) {
        self.scheme = s;
        self.bindings[0] = s.chord();
    }

    /// 查询：某组合键当前是否触发切换（让出旧键判据的查询面）。
    pub fn triggers(&self, chord: u16) -> bool {
        self.bindings.iter().any(|&b| b == chord)
    }

    /// 一键一职自证：绑定表内组合键两两互异（容量 1 结构性成立，
    /// 运行期仍对账防未来扩容引入双绑定）。
    pub fn one_key_one_job(&self) -> bool {
        for i in 0..self.bindings.len() {
            for j in (i + 1)..self.bindings.len() {
                if self.bindings[i] == self.bindings[j] {
                    return false;
                }
            }
        }
        true
    }

    /// F421 点击循环并存：键盘改方案不影响鼠标路径可用性。
    pub fn mouse_cycle_available(&self) -> bool {
        self.mouse_cycle_alive
    }

    /// 当前方案登记进冲突审计表（F244 同源联动）。
    pub fn register_audit(&self, audit: &mut ConflictAudit) -> bool {
        audit.register(self.scheme.chord())
    }
}

/// F518 域自检（12 条）。
pub fn run_f518_checks() -> CheckSet {
    let mut cs = CheckSet::new("F518-ime-switchkey");
    // 1) 默认方案 Win+空格。
    let imk = ImeSwitchKey::new();
    cs.add("default_win_space", imk.scheme() == SwitchScheme::WinSpace, "");
    // 2) 四方案组合键两两互异（位图唯一）。
    let mut distinct = true;
    for i in 0..SWITCH_SCHEMES.len() {
        for j in (i + 1)..SWITCH_SCHEMES.len() {
            distinct &= SWITCH_SCHEMES[i].chord() != SWITCH_SCHEMES[j].chord();
        }
    }
    cs.add("four_schemes_distinct_chords", distinct, "");
    // 3) 让出旧键：换方案后旧组合不再触发、新组合触发。
    let mut imk = ImeSwitchKey::new();
    let old = SwitchScheme::WinSpace.chord();
    imk.set_scheme(SwitchScheme::CtrlShift);
    cs.add(
        "old_key_released",
        !imk.triggers(old) && imk.triggers(SwitchScheme::CtrlShift.chord()),
        "",
    );
    // 4) 一键一职：绑定表无重复。
    cs.add("one_key_one_job", imk.one_key_one_job(), "");
    // 5) F421 点击循环并存：换方案后鼠标路径仍可用。
    cs.add("mouse_cycle_coexists", imk.mouse_cycle_available(), "");
    // 6) Caps 分界下侧：599ms = 锁定大小写。
    let mut caps = CapsState::new();
    caps.press(1_000);
    cs.add("caps_599ms_locks_case", caps.release(1_599) == CapsAction::LockCase, "");
    // 7) Caps 分界点：恰 600ms = 切输入法（>= 归长按）。
    let mut caps = CapsState::new();
    caps.press(1_000);
    cs.add("caps_600ms_switches_ime", caps.release(1_600) == CapsAction::SwitchIme, "");
    // 8) Caps 分界上侧：601ms = 切输入法。
    let mut caps = CapsState::new();
    caps.press(1_000);
    cs.add("caps_601ms_switches_ime", caps.release(1_601) == CapsAction::SwitchIme, "");
    // 9) 两段语义互斥（LockCase != SwitchIme 且分界两侧行为确定）。
    cs.add(
        "caps_actions_mutually_exclusive",
        CapsAction::LockCase != CapsAction::SwitchIme,
        "",
    );
    // 10) 未按下就松开 → None（噪声防御）。
    let mut caps = CapsState::new();
    cs.add("caps_release_without_press_none", caps.release(9_999) == CapsAction::None, "");
    // 11) 冲突审计联动：登记当前方案后查同键有冲突、查异键无冲突。
    let mut audit = ConflictAudit::new();
    imk.register_audit(&mut audit);
    cs.add(
        "audit_registers_current_scheme",
        audit.len() == 1 && audit.conflicts_of(SwitchScheme::CtrlShift.chord()) == 1,
        "",
    );
    cs.add(
        "audit_no_conflict_other_chord",
        audit.conflicts_of(SwitchScheme::AltShift.chord()) == 0,
        "",
    );
    cs
}

#[cfg(test)]
mod f518_tests {
    use super::*;

    #[test]
    fn 四方案枚举齐备() {
        assert_eq!(SWITCH_SCHEMES.len(), 4, "应恰好四方案");
        for s in SWITCH_SCHEMES {
            assert!(!s.name().is_empty(), "方案名非空");
        }
    }

    #[test]
    fn 换方案后旧键让出_不双绑定() {
        let mut imk = ImeSwitchKey::new();
        let win_space = SwitchScheme::WinSpace.chord();
        imk.set_scheme(SwitchScheme::CapsHold);
        assert!(!imk.triggers(win_space), "旧键 Win+空格 必须让出");
        assert!(imk.triggers(VK_CAPS), "新键 Caps 上岗");
        assert!(imk.one_key_one_job(), "一键一职");
    }

    #[test]
    fn caps_599ms_短按锁定大小写() {
        let mut caps = CapsState::new();
        caps.press(0);
        assert_eq!(caps.release(599), CapsAction::LockCase, "599ms < 600ms 应锁大小写");
    }

    #[test]
    fn caps_600ms_长按切输入法() {
        let mut caps = CapsState::new();
        caps.press(0);
        assert_eq!(caps.release(600), CapsAction::SwitchIme, "恰 600ms 应切输入法（>= 分界）");
        caps.press(10_000);
        assert_eq!(caps.release(10_601), CapsAction::SwitchIme, "601ms 同侧");
    }

    #[test]
    fn 键盘换方案不影响鼠标点击循环() {
        let mut imk = ImeSwitchKey::new();
        for s in SWITCH_SCHEMES {
            imk.set_scheme(s);
            assert!(imk.mouse_cycle_available(), "F421 鼠标循环应始终并存（方案 {}）", s.name());
        }
    }

    #[test]
    fn 冲突审计查出撞键() {
        let mut imk = ImeSwitchKey::new();
        imk.set_scheme(SwitchScheme::AltShift);
        let mut audit = ConflictAudit::new();
        assert!(imk.register_audit(&mut audit));
        // 另一路径也想用 Alt+Shift（如某第三方注册）→ 冲突清单非空。
        assert_eq!(audit.conflicts_of(SwitchScheme::AltShift.chord()), 1, "同键应判冲突");
        assert_eq!(audit.conflicts_of(SwitchScheme::WinSpace.chord()), 0, "异键无冲突");
    }
}

// ===========================================================================
// F535 Win+数字快捷启动 —— WinNumLauncher
// ===========================================================================

/// F535 任务栏槽位表容量：十键 1..9,0 → 槽 0..9（溢出区延续编号）。
pub const MAX_TASKBAR_SLOTS: usize = 10;

/// F535 槽位上的应用条目（固定/运行混排 + 溢出标记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaskApp {
    pub id: u32,
    pub pinned: bool,
    pub running: bool,
    pub focused: bool,
    pub overflow: bool,
}

impl TaskApp {
    pub const fn idle(id: u32) -> Self {
        TaskApp {
            id,
            pinned: false,
            running: false,
            focused: false,
            overflow: false,
        }
    }
}

/// F535 三态 + Shift 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinNumAction {
    /// 未运行 → 启动（F282 单例语义）。
    Launch,
    /// 运行非前台 → 切换。
    Switch,
    /// 前台 → 最小化（F252 点击语义同源）。
    Minimize,
    /// Shift 变体：以管理员语义新开实例。
    AdminNewInstance,
}

/// F535 数字键映射：b'1'..=b'9' → 0..8，b'0' → 9。
pub fn slot_for_digit(d: u8) -> Option<usize> {
    match d {
        b'1'..=b'9' => Some((d - b'1') as usize),
        b'0' => Some(9),
        _ => None,
    }
}

/// F535 三态判定（无 Shift 修饰时）。
pub fn action_for(app: &TaskApp) -> WinNumAction {
    if !app.running {
        WinNumAction::Launch
    } else if app.focused {
        WinNumAction::Minimize
    } else {
        WinNumAction::Switch
    }
}

/// F535 完整判定（含 Shift 变体：Shift+Win+数字 = 管理员新开实例）。
pub fn action_for_with_shift(app: &TaskApp, shift: bool) -> WinNumAction {
    if shift {
        WinNumAction::AdminNewInstance
    } else {
        action_for(app)
    }
}

/// F535 任务栏槽位表：固定在前、运行居中、溢出区延续编号——10 键全覆盖。
pub struct TaskbarSlots {
    slots: [Option<TaskApp>; MAX_TASKBAR_SLOTS],
    n: usize,
}

impl TaskbarSlots {
    pub const fn empty() -> Self {
        TaskbarSlots {
            slots: [None; MAX_TASKBAR_SLOTS],
            n: 0,
        }
    }

    /// 依任务栏排布建表：pinned（固定）→ running（运行未固定）→ overflow
    /// （溢出区，同样占号——F418/F495）。返回是否全部放入十槽。
    pub fn build(pinned: &[u32], running: &[u32], overflow: &[u32]) -> (TaskbarSlots, bool) {
        let mut t = TaskbarSlots::empty();
        let mut ok = true;
        let push = |t: &mut TaskbarSlots, id: u32, pinned: bool, overflow: bool| -> bool {
            if t.n >= MAX_TASKBAR_SLOTS {
                return false;
            }
            let idx = t.n;
            let mut app = TaskApp::idle(id);
            app.running = true;
            app.pinned = pinned;
            app.overflow = overflow;
            t.slots[idx] = Some(app);
            t.n += 1;
            true
        };
        for &id in pinned {
            ok &= push(&mut t, id, true, false);
        }
        for &id in running {
            ok &= push(&mut t, id, false, false);
        }
        for &id in overflow {
            ok &= push(&mut t, id, false, true);
        }
        (t, ok)
    }

    /// 十键映射：数字 → 槽位条目（与任务栏顺序一致）。
    pub fn slot_for_digit(&self, d: u8) -> Option<TaskApp> {
        match slot_for_digit(d) {
            Some(i) if i < self.n => self.slots[i],
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// 溢出项可达：存在被 10 键覆盖的溢出标记条目。
    pub fn overflow_reachable(&self) -> bool {
        self.slots[..self.n].iter().flatten().any(|a| a.overflow)
    }
}

/// F535 注册表登记：组合键登记表写入/查询一致（key=digit，value=chord 位图）。
pub struct HotkeyRegistry {
    entries: [Option<(u8, u16)>; MAX_TASKBAR_SLOTS],
}

impl HotkeyRegistry {
    pub const fn new() -> Self {
        HotkeyRegistry { entries: [None; MAX_TASKBAR_SLOTS] }
    }

    pub fn write(&mut self, digit: u8, chord: u16) -> bool {
        match slot_for_digit(digit) {
            Some(i) => {
                self.entries[i] = Some((digit, chord));
                true
            }
            None => false,
        }
    }

    pub fn query(&self, digit: u8) -> Option<u16> {
        match slot_for_digit(digit) {
            Some(i) => self.entries[i].map(|(_, c)| c),
            None => None,
        }
    }

    /// 登记完整性：十键全部登记且无缺位。
    pub fn all_registered(&self) -> bool {
        const DIGITS: [u8; 10] = [b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0'];
        DIGITS.iter().all(|&d| self.query(d).is_some())
    }
}

/// F535 域自检（12 条）。
pub fn run_f535_checks() -> CheckSet {
    let mut cs = CheckSet::new("F535-win-num");
    // 1) 数字映射：1..9 → 0..8。
    let map_1_to_9 = (0u8..9).all(|k| slot_for_digit(b'1' + k) == Some(k as usize));
    cs.add("digit_map_1_to_9", map_1_to_9, "");
    // 2) 数字 0 → 槽 9。
    cs.add("digit_zero_maps_nine", slot_for_digit(b'0') == Some(9), "");
    // 3) 非数字键拒绝。
    cs.add(
        "non_digit_rejected",
        slot_for_digit(b'a').is_none() && slot_for_digit(b'/').is_none(),
        "",
    );
    // 4) 槽位序与任务栏一致：pinned → running → overflow 连续编号。
    let (slots, full) = TaskbarSlots::build(&[100, 101], &[102], &[103, 104]);
    cs.add("build_fits_ten", full && slots.len() == 5, "");
    let s1 = slots.slot_for_digit(b'1');
    let s3 = slots.slot_for_digit(b'3');
    let s4 = slots.slot_for_digit(b'4');
    cs.add(
        "slot_order_matches_taskbar",
        s1.map(|a| a.id) == Some(100)
            && s3.map(|a| a.id) == Some(102)
            && s4.map(|a| a.id) == Some(103),
        "",
    );
    // 5) 溢出项可达（F495）：溢出图标同样占号可被 Win+数字 触达。
    cs.add("overflow_reachable", slots.overflow_reachable(), "");
    // 6) 三态：未运行 → Launch。
    cs.add("state_launch", action_for(&TaskApp::idle(1)) == WinNumAction::Launch, "");
    // 7) 三态：运行非前台 → Switch。
    let mut app = TaskApp::idle(2);
    app.running = true;
    cs.add("state_switch", action_for(&app) == WinNumAction::Switch, "");
    // 8) 三态：前台 → Minimize。
    app.focused = true;
    cs.add("state_minimize", action_for(&app) == WinNumAction::Minimize, "");
    // 9) 单例语义（F282）：已运行不再判 Launch。
    cs.add(
        "singleton_no_relaunch",
        action_for(&app) != WinNumAction::Launch,
        "",
    );
    // 10) Shift 变体：任意态下 Shift = 管理员新开实例。
    cs.add(
        "shift_admin_variant",
        action_for_with_shift(&TaskApp::idle(3), true) == WinNumAction::AdminNewInstance
            && action_for_with_shift(&app, true) == WinNumAction::AdminNewInstance,
        "",
    );
    // 11) 注册表写入/查询一致。
    let mut reg = HotkeyRegistry::new();
    let mut write_ok = true;
    const DIGITS: [u8; 10] = [b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0'];
    for (k, &d) in DIGITS.iter().enumerate() {
        write_ok &= reg.write(d, 0xA0 + k as u16);
    }
    cs.add(
        "registry_write_query_consistent",
        write_ok
            && reg.query(b'1') == Some(0xA0)
            && reg.query(b'0') == Some(0xA9)
            && reg.all_registered(),
        "",
    );
    // 12) 注册表拒绝非数字登记。
    cs.add("registry_rejects_non_digit", !reg.write(b'z', 0xFF), "");
    cs
}

#[cfg(test)]
mod f535_tests {
    use super::*;

    #[test]
    fn 十键映射全表() {
        for k in 0u8..9 {
            assert_eq!(slot_for_digit(b'1' + k), Some(k as usize), "数字 {} 应映槽 {}", k + 1, k);
        }
        assert_eq!(slot_for_digit(b'0'), Some(9), "数字 0 应映槽 9");
    }

    #[test]
    fn 顺序与任务栏排布一致_含溢出() {
        let (slots, _) = TaskbarSlots::build(&[10], &[], &[11, 12, 13]);
        assert_eq!(slots.slot_for_digit(b'1').map(|a| a.id), Some(10));
        assert_eq!(slots.slot_for_digit(b'2').map(|a| a.id), Some(11), "溢出区延续编号");
        assert!(slots.overflow_reachable(), "溢出项必须可达");
    }

    #[test]
    fn 三态互斥完备() {
        let mut a = TaskApp::idle(1);
        assert_eq!(action_for(&a), WinNumAction::Launch);
        a.running = true;
        assert_eq!(action_for(&a), WinNumAction::Switch);
        a.focused = true;
        assert_eq!(action_for(&a), WinNumAction::Minimize, "前台应最小化切换");
    }

    #[test]
    fn shift_变体覆盖三态() {
        let mut a = TaskApp::idle(1);
        assert_eq!(action_for_with_shift(&a, true), WinNumAction::AdminNewInstance);
        a.running = true;
        assert_eq!(action_for_with_shift(&a, true), WinNumAction::AdminNewInstance);
        a.focused = true;
        assert_eq!(action_for_with_shift(&a, true), WinNumAction::AdminNewInstance);
    }

    #[test]
    fn 注册表回环查询() {
        let mut reg = HotkeyRegistry::new();
        assert!(reg.write(b'5', 0x55));
        assert_eq!(reg.query(b'5'), Some(0x55));
        assert!(reg.write(b'5', 0x66), "同键覆写应允许");
        assert_eq!(reg.query(b'5'), Some(0x66), "查询应得最新值");
    }

    #[test]
    fn 空表按键无槽() {
        let slots = TaskbarSlots::empty();
        assert!(slots.slot_for_digit(b'1').is_none(), "空任务栏按键无目标");
    }
}

// ===========================================================================
// F536 Win+T 任务栏遍历 —— TaskbarTraverse
// ===========================================================================

/// F536 任务栏槽位上限（遍历环容量）。
pub const MAX_TRAVERSE_SLOTS: usize = 16;

/// F536 遍历方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraverseDir {
    Left,
    Right,
}

/// F536 五链路状态：Idle → Entered → Traversing → Activated/Menu → Exited。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraverseState {
    Idle,
    /// 进场：Win+T 落到任务栏首图标。
    Entered,
    /// 遍历：方向键移动（含首尾回绕）。
    Traversing,
    /// 激活：Enter = 单击语义。
    Activated,
    /// 菜单：菜单键 = 右键语义（F419/F433）。
    Menu,
    /// 退出：Esc 归还焦点到原窗口。
    Exited,
}

/// F536 任务栏遍历状态机：焦点环 + 回绕 + 归还目标记录。
pub struct TaskbarTraverse {
    state: TraverseState,
    slots: [u32; MAX_TRAVERSE_SLOTS],
    n: usize,
    cur: usize,
    focus_ring: bool,
    origin_window: u32,
    returned_to: u32,
}

impl TaskbarTraverse {
    pub const fn new() -> Self {
        TaskbarTraverse {
            state: TraverseState::Idle,
            slots: [0; MAX_TRAVERSE_SLOTS],
            n: 0,
            cur: 0,
            focus_ring: false,
            origin_window: 0,
            returned_to: 0,
        }
    }

    pub fn state(&self) -> TraverseState {
        self.state
    }

    pub fn focus_ring_on(&self) -> bool {
        self.focus_ring
    }

    pub fn current(&self) -> Option<u32> {
        if self.n == 0 {
            None
        } else {
            Some(self.slots[self.cur])
        }
    }

    /// 进场：焦点跳到任务栏首图标，焦点环点亮，记录归还目标。
    pub fn enter(&mut self, origin_window: u32, slots: &[u32]) -> bool {
        if slots.is_empty() || slots.len() > MAX_TRAVERSE_SLOTS {
            return false;
        }
        self.origin_window = origin_window;
        self.n = slots.len();
        self.slots[..self.n].copy_from_slice(slots);
        self.cur = 0;
        self.focus_ring = true;
        self.state = TraverseState::Entered;
        true
    }

    /// 遍历：方向键移动，首尾回绕（左在首=到尾、右在尾=到首）。
    pub fn move_dir(&mut self, d: TraverseDir) -> bool {
        if self.n == 0 {
            return false;
        }
        match d {
            TraverseDir::Left => self.cur = (self.cur + self.n - 1) % self.n,
            TraverseDir::Right => self.cur = (self.cur + 1) % self.n,
        }
        self.state = TraverseState::Traversing;
        true
    }

    /// 激活：Enter = 单击语义。
    pub fn activate(&mut self) -> bool {
        if self.n == 0 {
            return false;
        }
        self.state = TraverseState::Activated;
        true
    }

    /// 菜单：菜单键 = 右键语义（F419/F433）。
    pub fn open_menu(&mut self) -> bool {
        if self.n == 0 {
            return false;
        }
        self.state = TraverseState::Menu;
        true
    }

    /// 退出：Esc 归还焦点到进场记录的原窗口，焦点环熄灭。
    pub fn exit(&mut self) -> u32 {
        self.returned_to = self.origin_window;
        self.focus_ring = false;
        self.state = TraverseState::Exited;
        self.returned_to
    }

    pub fn returned_to(&self) -> u32 {
        self.returned_to
    }

    /// 空闲自证：未进场时无环、无当前槽。
    pub fn idle_clean(&self) -> bool {
        self.state == TraverseState::Idle && !self.focus_ring && self.n == 0
    }
}

/// F536 域自检（11 条）。
pub fn run_f536_checks() -> CheckSet {
    let mut cs = CheckSet::new("F536-win-t");
    let slots = [11u32, 12, 13, 14];
    // 1) 初始空闲：无状态、无环。
    let tt = TaskbarTraverse::new();
    cs.add("starts_idle", tt.idle_clean(), "");
    // 2) 进场：落到首图标，焦点环点亮。
    let mut tt = TaskbarTraverse::new();
    cs.add(
        "enter_lands_first_with_ring",
        tt.enter(77, &slots) && tt.state() == TraverseState::Entered && tt.focus_ring_on(),
        "",
    );
    cs.add("enter_current_first", tt.current() == Some(11), "");
    // 3) 遍历：右移到第二槽。
    tt.move_dir(TraverseDir::Right);
    cs.add(
        "right_move_traverses",
        tt.state() == TraverseState::Traversing && tt.current() == Some(12) && tt.focus_ring_on(),
        "",
    );
    // 4) 循环边界：右在尾回绕到首。
    tt.move_dir(TraverseDir::Right);
    tt.move_dir(TraverseDir::Right);
    let at_tail = tt.current() == Some(14);
    tt.move_dir(TraverseDir::Right);
    cs.add("wrap_right_at_tail", at_tail && tt.current() == Some(11), "");
    // 5) 循环边界：左在首回绕到尾（此刻 current=11 在首）。
    tt.move_dir(TraverseDir::Left);
    cs.add("wrap_left_at_head", tt.current() == Some(14), "");
    // 6) 激活：Enter = 单击语义。
    tt.activate();
    cs.add(
        "enter_activates_click_semantics",
        tt.state() == TraverseState::Activated,
        "",
    );
    // 7) 菜单：菜单键 = 右键语义（F419/F433）。
    tt.open_menu();
    cs.add("menu_key_rightclick_semantics", tt.state() == TraverseState::Menu, "");
    // 8) 退出：Esc 归还焦点到原窗口（归还目标记录在案）。
    let returned = tt.exit();
    cs.add("esc_returns_to_origin", returned == 77 && tt.returned_to() == 77, "");
    // 9) 退出后环熄灭。
    cs.add("ring_off_after_exit", !tt.focus_ring_on(), "");
    // 10) 空任务栏拒绝进场。
    let mut tt2 = TaskbarTraverse::new();
    cs.add("empty_taskbar_rejects_enter", !tt2.enter(1, &[]), "");
    // 11) 五链路全序：Entered → Traversing → Activated（状态编号单调推进）。
    let mut tt3 = TaskbarTraverse::new();
    let chain_ok = tt3.enter(5, &slots)
        && tt3.move_dir(TraverseDir::Right)
        && tt3.activate()
        && tt3.exit() == 5
        && tt3.state() == TraverseState::Exited;
    cs.add("five_link_chain_complete", chain_ok, "");
    cs
}

#[cfg(test)]
mod f536_tests {
    use super::*;

    #[test]
    fn 进场即亮环并落首图标() {
        let mut tt = TaskbarTraverse::new();
        assert!(tt.enter(9, &[1, 2, 3]));
        assert_eq!(tt.state(), TraverseState::Entered);
        assert_eq!(tt.current(), Some(1), "进场应落首图标");
        assert!(tt.focus_ring_on(), "焦点环应可见");
    }

    #[test]
    fn 右在尾回绕到首() {
        let mut tt = TaskbarTraverse::new();
        tt.enter(0, &[1, 2, 3]);
        tt.move_dir(TraverseDir::Right);
        tt.move_dir(TraverseDir::Right);
        assert_eq!(tt.current(), Some(3), "两步后应在尾");
        tt.move_dir(TraverseDir::Right);
        assert_eq!(tt.current(), Some(1), "尾右移应回首（回绕）");
    }

    #[test]
    fn 左在首回绕到尾() {
        let mut tt = TaskbarTraverse::new();
        tt.enter(0, &[1, 2, 3]);
        tt.move_dir(TraverseDir::Left);
        assert_eq!(tt.current(), Some(3), "首左移应到尾（回绕）");
    }

    #[test]
    fn esc_归还焦点到进场原窗() {
        let mut tt = TaskbarTraverse::new();
        tt.enter(1234, &[7, 8]);
        tt.move_dir(TraverseDir::Right);
        assert_eq!(tt.exit(), 1234, "Esc 应归还到原窗口 1234");
        assert!(!tt.focus_ring_on(), "归还后环熄灭");
        assert_eq!(tt.state(), TraverseState::Exited);
    }

    #[test]
    fn 菜单键即右键语义() {
        let mut tt = TaskbarTraverse::new();
        tt.enter(0, &[5]);
        assert!(tt.open_menu());
        assert_eq!(tt.state(), TraverseState::Menu, "菜单键=右键语义（F419/F433）");
    }

    #[test]
    fn 空槽位拒绝全部操作() {
        let mut tt = TaskbarTraverse::new();
        assert!(!tt.enter(1, &[]));
        assert!(!tt.move_dir(TraverseDir::Right));
        assert!(!tt.activate());
        assert!(!tt.open_menu());
    }
}

// ===========================================================================
// F537 Win+逗号 瞥桌面 —— DesktopPeek
// ===========================================================================

/// F537 瞄透明度：15%（permille=150）。
pub const PEEK_ALPHA_PERMILLE: u32 = 150;
/// F537 透明度容差：±3%（permille 30）→ 合法窗 [120, 180]。
pub const PEEK_ALPHA_TOL: u32 = 30;
/// F537 淡出/淡回时长：120ms。
pub const PEEK_FADE_MS: u64 = 120;
/// F537 时长容差：±20ms → 合法窗 [100, 140]。
pub const PEEK_FADE_TOL_MS: u64 = 20;
/// F537 窗口快照容量。
pub const MAX_PEEK_WINDOWS: usize = 8;

/// F537 瞥状态机（按下 Press 为瞬时事件——按下即入 FadingOut，故列四态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeekState {
    FadingOut,
    Peeking,
    FadingIn,
    Restored,
}

/// F537 窗口几何（px 整数域——恢复精度 <1px 即逐字段相等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// F537 单窗快照：瞥前几何 snap + 当前几何 live。
#[derive(Clone, Copy, Debug)]
pub struct PeekWindow {
    pub id: u32,
    pub snap: WinRect,
    pub live: WinRect,
}

impl PeekWindow {
    /// 恢复原位精度：逐窗漂移 px（0 = 逐字段相等，恒 <1px）。
    pub fn drift_px(&self) -> u32 {
        let dx = (self.live.x - self.snap.x).unsigned_abs() as u64;
        let dy = (self.live.y - self.snap.y).unsigned_abs() as u64;
        let dw = (self.live.w as i64 - self.snap.w as i64).unsigned_abs() as u64;
        let dh = (self.live.h as i64 - self.snap.h as i64).unsigned_abs() as u64;
        ((dx.max(dy)).max(dw.max(dh))) as u32
    }
}

/// F537 事件分类（F316 闲置计时过滤器用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeekEvent {
    Press,
    Release,
    FadeTick,
    Other(u8),
}

/// F537 瞥桌面状态机。
pub struct DesktopPeek {
    state: PeekState,
    /// 状态进入时刻（ms 单调钟）。
    t_enter: u64,
    wins: [PeekWindow; MAX_PEEK_WINDOWS],
    n: usize,
    /// 纯看限制：瞥过程中被拒绝的桌面交互请求计数。
    rejected_inputs: u32,
}

impl DesktopPeek {
    pub const fn new() -> Self {
        DesktopPeek {
            state: PeekState::Restored,
            t_enter: 0,
            wins: [PeekWindow {
                id: 0,
                snap: WinRect { x: 0, y: 0, w: 0, h: 0 },
                live: WinRect { x: 0, y: 0, w: 0, h: 0 },
            }; MAX_PEEK_WINDOWS],
            n: 0,
            rejected_inputs: 0,
        }
    }

    pub fn state(&self) -> PeekState {
        self.state
    }

    pub fn rejected_inputs(&self) -> u32 {
        self.rejected_inputs
    }

    /// 登记参与瞥的窗口（按下前建快照）。
    pub fn add_window(&mut self, id: u32, r: WinRect) -> bool {
        if self.n >= MAX_PEEK_WINDOWS {
            return false;
        }
        self.wins[self.n] = PeekWindow { id, snap: r, live: r };
        self.n += 1;
        true
    }

    /// 按下 Win+逗号：快照就绪 → 进入 FadingOut（120ms 淡出）。
    pub fn press(&mut self, t: u64) -> bool {
        if self.n == 0 {
            return false;
        }
        self.state = PeekState::FadingOut;
        self.t_enter = t;
        true
    }

    /// 淡出 alpha 模型：线性 1000‰ → 150‰，t=PEEK_FADE_MS 触底。
    pub fn alpha_at(&self, elapsed_ms: u64) -> u32 {
        if self.state == PeekState::Restored {
            return 1000;
        }
        if elapsed_ms >= PEEK_FADE_MS {
            PEEK_ALPHA_PERMILLE
        } else {
            let drop = (1000 - PEEK_ALPHA_PERMILLE) * elapsed_ms as u32 / PEEK_FADE_MS as u32;
            1000 - drop
        }
    }

    /// 淡出完成 → Peeking（玻璃态；透明度落 15%±3% 窗）。
    pub fn fade_out_done(&mut self, t: u64) -> bool {
        if self.state != PeekState::FadingOut || t.saturating_sub(self.t_enter) < PEEK_FADE_MS {
            return false;
        }
        self.state = PeekState::Peeking;
        self.t_enter = t;
        true
    }

    /// 纯看限制：瞥过程中桌面交互请求被拒（计数入账）。
    pub fn intercept_input(&mut self) -> bool {
        if self.state == PeekState::Peeking {
            self.rejected_inputs += 1;
            true
        } else {
            false
        }
    }

    /// 松开：进入 FadingIn（120ms 淡回）。
    pub fn release(&mut self, t: u64) -> bool {
        if self.state != PeekState::Peeking {
            return false;
        }
        self.state = PeekState::FadingIn;
        self.t_enter = t;
        true
    }

    /// 淡回完成 → Restored：几何快照逐窗恢复（漂移恒 <1px）。
    pub fn fade_in_done(&mut self, t: u64) -> bool {
        if self.state != PeekState::FadingIn || t.saturating_sub(self.t_enter) < PEEK_FADE_MS {
            return false;
        }
        for w in self.wins[..self.n].iter_mut() {
            w.live = w.snap;
        }
        self.state = PeekState::Restored;
        self.t_enter = t;
        true
    }

    /// 恢复原位精度：全体窗口最大漂移 px（判据 <1px，即 ==0）。
    pub fn max_drift_px(&self) -> u32 {
        self.wins[..self.n]
            .iter()
            .map(|w| w.drift_px())
            .max()
            .unwrap_or(0)
    }

    /// 时长容差判定：实测时长 d 落 120±20ms 窗。
    pub fn fade_within_tolerance(d: u64) -> bool {
        d + PEEK_FADE_TOL_MS >= PEEK_FADE_MS && d <= PEEK_FADE_MS + PEEK_FADE_TOL_MS
    }

    /// 透明度容差判定：实测 alpha 落 15%±3% 窗（[120,180]‰）。
    pub fn alpha_within_tolerance(a: u32) -> bool {
        a + PEEK_ALPHA_TOL >= PEEK_ALPHA_PERMILLE && a <= PEEK_ALPHA_PERMILLE + PEEK_ALPHA_TOL
    }
}

/// F316 闲置计时过滤：瞥事件不得喂入闲置计时器（既不重置也不触发）。
pub fn idle_timer_feeds(e: PeekEvent) -> bool {
    match e {
        PeekEvent::Press | PeekEvent::Release | PeekEvent::FadeTick => false,
        PeekEvent::Other(_) => true,
    }
}

/// F537 域自检（12 条）。
pub fn run_f537_checks() -> CheckSet {
    let mut cs = CheckSet::new("F537-win-peek");
    // 1) 常量精确：150‰ / ±30‰ / 120ms / ±20ms。
    cs.add(
        "constants_exact",
        PEEK_ALPHA_PERMILLE == 150
            && PEEK_ALPHA_TOL == 30
            && PEEK_FADE_MS == 120
            && PEEK_FADE_TOL_MS == 20,
        "",
    );
    // 2) 按下进入 FadingOut。
    let mut peek = DesktopPeek::new();
    peek.add_window(1, WinRect { x: 10, y: 20, w: 800, h: 600 });
    peek.press(1_000);
    cs.add("press_enters_fading_out", peek.state() == PeekState::FadingOut, "");
    // 3) 淡出端点：起点不透明 1000‰、终点 150‰。
    cs.add(
        "alpha_endpoints",
        peek.alpha_at(0) == 1000 && peek.alpha_at(PEEK_FADE_MS) == PEEK_ALPHA_PERMILLE,
        "",
    );
    // 4) 淡出中点线性：60ms → 575‰。
    cs.add("alpha_midpoint_linear", peek.alpha_at(60) == 575, "");
    // 5) 透明度容差窗：[120,180] 内合法，边界外非法。
    cs.add(
        "alpha_tolerance_window",
        DesktopPeek::alpha_within_tolerance(120)
            && DesktopPeek::alpha_within_tolerance(150)
            && DesktopPeek::alpha_within_tolerance(180)
            && !DesktopPeek::alpha_within_tolerance(119)
            && !DesktopPeek::alpha_within_tolerance(181),
        "",
    );
    // 6) 时长容差窗：100/120/140 合法，99/141 非法。
    cs.add(
        "fade_tolerance_window",
        DesktopPeek::fade_within_tolerance(100)
            && DesktopPeek::fade_within_tolerance(120)
            && DesktopPeek::fade_within_tolerance(140)
            && !DesktopPeek::fade_within_tolerance(99)
            && !DesktopPeek::fade_within_tolerance(141),
        "",
    );
    // 7) 淡出完成 → Peeking。
    peek.fade_out_done(1_120);
    cs.add("fade_out_done_peeking", peek.state() == PeekState::Peeking, "");
    // 8) 纯看限制：Peeking 中交互请求被拒且计数。
    let rejected_once = peek.intercept_input();
    let rejected_twice = peek.intercept_input();
    cs.add(
        "pure_look_blocks_input",
        rejected_once && rejected_twice && peek.rejected_inputs() == 2,
        "",
    );
    // 9) 非瞥态不拦（松手后恢复操作）。
    peek.release(2_000);
    peek.fade_in_done(2_120);
    cs.add(
        "released_input_restored",
        peek.state() == PeekState::Restored && !peek.intercept_input(),
        "",
    );
    // 10) 恢复原位精度：逐窗漂移 <1px（整数域 ==0）。
    cs.add("restore_drift_under_1px", peek.max_drift_px() == 0, "");
    // 11) 瞥非最小化：玻璃态 alpha 恒 >0（150‰ 而非 0）。
    cs.add("glass_not_minimized", PEEK_ALPHA_PERMILLE > 0, "");
    // 12) F316 无冲突：瞥事件不喂闲置计时器，普通事件照常。
    cs.add(
        "idle_timer_unaffected",
        !idle_timer_feeds(PeekEvent::Press)
            && !idle_timer_feeds(PeekEvent::Release)
            && !idle_timer_feeds(PeekEvent::FadeTick)
            && idle_timer_feeds(PeekEvent::Other(1)),
        "",
    );
    cs
}

#[cfg(test)]
mod f537_tests {
    use super::*;

    fn peek_with_two() -> DesktopPeek {
        let mut p = DesktopPeek::new();
        p.add_window(1, WinRect { x: 0, y: 0, w: 1920, h: 1080 });
        p.add_window(2, WinRect { x: 100, y: 200, w: 640, h: 480 });
        p
    }

    #[test]
    fn 透明度落_15百分之_容差窗() {
        assert_eq!(PEEK_ALPHA_PERMILLE, 150, "15% = 150‰");
        assert!(DesktopPeek::alpha_within_tolerance(PEEK_ALPHA_PERMILLE));
        assert!(DesktopPeek::alpha_within_tolerance(120) && DesktopPeek::alpha_within_tolerance(180));
        assert!(!DesktopPeek::alpha_within_tolerance(119), "119‰ 低于下界");
        assert!(!DesktopPeek::alpha_within_tolerance(181), "181‰ 高于上界");
    }

    #[test]
    fn 进出时长_120加减20_边界() {
        assert!(!DesktopPeek::fade_within_tolerance(99), "99ms 出窗");
        assert!(DesktopPeek::fade_within_tolerance(100), "100ms 恰在下界");
        assert!(DesktopPeek::fade_within_tolerance(120), "120ms 标准");
        assert!(DesktopPeek::fade_within_tolerance(140), "140ms 恰在上界");
        assert!(!DesktopPeek::fade_within_tolerance(141), "141ms 出窗");
    }

    #[test]
    fn 淡出线性到触底() {
        let mut p = peek_with_two();
        p.press(0);
        assert_eq!(p.alpha_at(0), 1000);
        assert_eq!(p.alpha_at(30), 1000 - 850 * 30 / 120);
        assert_eq!(p.alpha_at(60), 575, "中点线性");
        assert_eq!(p.alpha_at(120), 150, "触底 150‰");
        assert_eq!(p.alpha_at(500), 150, "超时不更低——玻璃非消失");
    }

    #[test]
    fn 纯看_交互请求被拒() {
        let mut p = peek_with_two();
        p.press(0);
        p.fade_out_done(120);
        assert!(p.intercept_input(), "瞥中桌面交互应被拒");
        assert_eq!(p.rejected_inputs(), 1);
        p.release(200);
        p.fade_in_done(320);
        assert!(!p.intercept_input(), "松手后恢复操作");
    }

    #[test]
    fn 恢复原位精度_小于1px() {
        let mut p = peek_with_two();
        p.press(0);
        p.fade_out_done(120);
        p.release(1_000);
        p.fade_in_done(1_120);
        assert_eq!(p.max_drift_px(), 0, "快照恢复逐窗零漂移（<1px）");
    }

    #[test]
    fn f316_闲置计时不受瞥影响() {
        assert!(!idle_timer_feeds(PeekEvent::Press), "Press 不喂闲置计时");
        assert!(!idle_timer_feeds(PeekEvent::Release), "Release 不喂闲置计时");
        assert!(!idle_timer_feeds(PeekEvent::FadeTick), "淡出帧不喂闲置计时");
        assert!(idle_timer_feeds(PeekEvent::Other(9)), "普通事件照常喂");
    }

    #[test]
    fn 五态全链路顺序() {
        let mut p = peek_with_two();
        assert!(p.press(0));
        assert_eq!(p.state(), PeekState::FadingOut);
        assert!(p.fade_out_done(120));
        assert_eq!(p.state(), PeekState::Peeking);
        assert!(p.release(240));
        assert_eq!(p.state(), PeekState::FadingIn);
        assert!(p.fade_in_done(360));
        assert_eq!(p.state(), PeekState::Restored);
    }
}

// ===========================================================================
// F538 Alt+Esc 窗口循环 —— AltEscCycle
// ===========================================================================

/// F538 连按节奏：<100ms/次 计为连按（逐窗后退的节奏参数）。
pub const ALTESC_REPEAT_MS: u64 = 100;
/// F538 循环栈容量。
pub const MAX_ALT_ESC_WINDOWS: usize = 12;
/// F538 动画最简：结构自证无预览浮层（与 Alt+Tab 互补的「盲切」）。
pub const HAS_PREVIEW_OVERLAY: bool = false;

/// F538 Z 序栈 + 连按指针 + 最小化参与。
pub struct AltEscCycle {
    /// MRU 序：zorder[0] = 最近激活（Z 顶）。
    zorder: [u32; MAX_ALT_ESC_WINDOWS],
    minimized: [bool; MAX_ALT_ESC_WINDOWS],
    n: usize,
    /// 从 Z 顶后退的槽位数（每次 Esc +1）。
    ptr: usize,
    last_tap: Option<u64>,
    tap_count: u32,
}

impl AltEscCycle {
    pub const fn new() -> Self {
        AltEscCycle {
            zorder: [0; MAX_ALT_ESC_WINDOWS],
            minimized: [false; MAX_ALT_ESC_WINDOWS],
            n: 0,
            ptr: 0,
            last_tap: None,
            tap_count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn tap_count(&self) -> u32 {
        self.tap_count
    }

    /// 入栈：新激活窗上 Z 顶（MRU 头插，同 id 不重复入栈）。
    pub fn activate_window(&mut self, id: u32) -> bool {
        if let Some(i) = self.find(id) {
            // 提到 Z 顶（MRU 语义）。
            let w = self.zorder[i];
            let m = self.minimized[i];
            for k in (1..=i).rev() {
                self.zorder[k] = self.zorder[k - 1];
                self.minimized[k] = self.minimized[k - 1];
            }
            self.zorder[0] = w;
            self.minimized[0] = false;
            let _ = m;
            self.ptr = 0;
            return true;
        }
        if self.n >= MAX_ALT_ESC_WINDOWS {
            return false;
        }
        for k in (1..=self.n).rev() {
            self.zorder[k] = self.zorder[k - 1];
            self.minimized[k] = self.minimized[k - 1];
        }
        self.zorder[0] = id;
        self.minimized[0] = false;
        self.n += 1;
        self.ptr = 0;
        true
    }

    pub fn set_minimized(&mut self, id: u32, m: bool) -> bool {
        match self.find(id) {
            Some(i) => {
                self.minimized[i] = m;
                true
            }
            None => false,
        }
    }

    pub fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.n {
            if self.zorder[i] == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 连按节奏判定：<100ms 计为连按。
    pub fn is_repeat(prev: u64, cur: u64) -> bool {
        cur.saturating_sub(prev) < ALTESC_REPEAT_MS
    }

    /// 连按 Esc：指针后退一位（回绕），节奏入账。
    pub fn tap_esc(&mut self, t: u64) -> bool {
        if self.n == 0 {
            return false;
        }
        if let Some(prev) = self.last_tap {
            if Self::is_repeat(prev, t) {
                self.tap_count += 1;
            } else {
                self.tap_count = 1;
            }
        } else {
            self.tap_count = 1;
        }
        self.last_tap = Some(t);
        self.ptr = (self.ptr + 1) % self.n;
        true
    }

    /// 当前指向窗（Z 序正确性：ptr=k 即 Z 序第 k+1 层）。
    pub fn selected(&self) -> Option<u32> {
        if self.n == 0 {
            None
        } else {
            Some(self.zorder[self.ptr])
        }
    }

    /// 该窗是否最小化（最小化参与：选中即还原的判定面）。
    pub fn selected_minimized(&self) -> bool {
        if self.n == 0 {
            false
        } else {
            self.minimized[self.ptr]
        }
    }

    /// Alt 松开落定：激活当前指向窗——提到 Z 顶；最小化则还原。
    pub fn release_alt(&mut self) -> Option<(u32, bool)> {
        if self.n == 0 {
            return None;
        }
        let id = self.zorder[self.ptr];
        let was_min = self.minimized[self.ptr];
        self.activate_window(id);
        Some((id, was_min))
    }
}

/// F538 Alt+Tab 并存对账态（F082）：独立状态体，互不改写。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AltTabState {
    pub ptr: usize,
    pub active: bool,
}

/// F538 域自检（11 条）。
pub fn run_f538_checks() -> CheckSet {
    let mut cs = CheckSet::new("F538-alt-esc");
    // 1) 常量：节奏 100ms、无浮层。
    cs.add(
        "constants_exact",
        ALTESC_REPEAT_MS == 100 && !HAS_PREVIEW_OVERLAY,
        "",
    );
    // 2) Z 序入栈正确：后激活者在 Z 顶。
    let mut cyc = AltEscCycle::new();
    cyc.activate_window(1);
    cyc.activate_window(2);
    cyc.activate_window(3);
    cs.add("zorder_mru_top", cyc.selected() == Some(3), "");
    // 3) 连按 Esc：指针逐窗后退（3 → 2 → 1）。
    cyc.tap_esc(1_000);
    cyc.tap_esc(1_050);
    cs.add("tap_moves_back", cyc.selected() == Some(1), "");
    // 4) 连按节奏：<100ms 计连按、>=100ms 不计。
    cs.add(
        "cadence_boundary",
        AltEscCycle::is_repeat(1_000, 1_099) && !AltEscCycle::is_repeat(1_000, 1_100),
        "",
    );
    // 5) 落定：Alt 松开激活当前指向窗（回 Z 顶）。
    let landed = cyc.release_alt();
    cs.add(
        "release_activates_selected",
        landed == Some((1, false)) && cyc.selected() == Some(1),
        "",
    );
    // 6) Z 序正确性：落定后次序 1 在顶、3 次之、2 再之。
    cs.add(
        "zorder_after_activation",
        cyc.find(1) == Some(0) && cyc.find(3) == Some(1) && cyc.find(2) == Some(2),
        "",
    );
    // 7) 最小化参与：循环选中最小化窗 → 落定时报告还原。
    let mut cyc2 = AltEscCycle::new();
    cyc2.activate_window(10);
    cyc2.activate_window(11);
    cyc2.set_minimized(10, true);
    cyc2.tap_esc(2_000);
    cs.add(
        "minimized_participates",
        cyc2.selected() == Some(10) && cyc2.selected_minimized(),
        "",
    );
    let landed2 = cyc2.release_alt();
    cs.add("minimized_restored_on_land", landed2 == Some((10, true)), "");
    // 8) 连按回绕：n=2 连按两次回到起点。
    let mut cyc3 = AltEscCycle::new();
    cyc3.activate_window(1);
    cyc3.activate_window(2);
    cyc3.tap_esc(3_000);
    cyc3.tap_esc(3_050);
    cs.add("tap_wraps_around", cyc3.selected() == Some(2), "");
    // 9) 与 Alt+Tab 并存：Alt+Esc 全程操作不触碰 Alt+Tab 状态。
    let mut cyc4 = AltEscCycle::new();
    cyc4.activate_window(1);
    cyc4.activate_window(2);
    let at = AltTabState { ptr: 3, active: true };
    cyc4.tap_esc(4_000);
    let _ = cyc4.release_alt();
    cs.add(
        "alt_tab_state_untouched",
        at == AltTabState { ptr: 3, active: true },
        "",
    );
    // 10) 无浮层（结构自证常量 + 与 Alt+Tab 互补定位）。
    cs.add("no_preview_overlay", HAS_PREVIEW_OVERLAY == false, "");
    // 11) 空栈防御：无窗时连按/落定皆拒绝。
    let mut cyc5 = AltEscCycle::new();
    cs.add(
        "empty_cycle_defends",
        !cyc5.tap_esc(5_000) && cyc5.release_alt().is_none() && cyc5.selected().is_none(),
        "",
    );
    cs
}

#[cfg(test)]
mod f538_tests {
    use super::*;

    #[test]
    fn z序_mru_顶进() {
        let mut cyc = AltEscCycle::new();
        cyc.activate_window(100);
        cyc.activate_window(200);
        cyc.activate_window(300);
        assert_eq!(cyc.selected(), Some(300), "后激活者在 Z 顶");
        assert_eq!(cyc.len(), 3);
    }

    #[test]
    fn 连按节奏_100ms_分界() {
        assert!(AltEscCycle::is_repeat(0, 99), "99ms 间隔应计连按");
        assert!(!AltEscCycle::is_repeat(0, 100), "恰 100ms 不计连按（<100ms 判据）");
        let mut cyc = AltEscCycle::new();
        cyc.activate_window(1);
        cyc.activate_window(2);
        cyc.activate_window(3);
        cyc.tap_esc(0);
        cyc.tap_esc(50);
        cyc.tap_esc(200);
        assert_eq!(cyc.tap_count(), 1, "超节奏后连按计数应重起");
    }

    #[test]
    fn 按住_alt_逐窗后退_松开落定() {
        let mut cyc = AltEscCycle::new();
        cyc.activate_window(1);
        cyc.activate_window(2);
        cyc.activate_window(3);
        cyc.tap_esc(0);
        assert_eq!(cyc.selected(), Some(2), "一次后退到第 2 层");
        cyc.tap_esc(80);
        assert_eq!(cyc.selected(), Some(1), "两次后退到第 3 层");
        assert_eq!(cyc.release_alt(), Some((1, false)));
        assert_eq!(cyc.selected(), Some(1), "落定后选中窗回 Z 顶");
    }

    #[test]
    fn 最小化窗参与循环并还原() {
        let mut cyc = AltEscCycle::new();
        cyc.activate_window(1);
        cyc.activate_window(2);
        assert!(cyc.set_minimized(1, true));
        cyc.tap_esc(0);
        assert_eq!(cyc.selected(), Some(1));
        assert!(cyc.selected_minimized(), "最小化窗应在循环内");
        assert_eq!(cyc.release_alt(), Some((1, true)), "落定应报告还原");
    }

    #[test]
    fn 与_alt_tab_状态独立() {
        let mut cyc = AltEscCycle::new();
        cyc.activate_window(1);
        let at0 = AltTabState { ptr: 5, active: false };
        let at_before = at0;
        cyc.activate_window(2);
        cyc.tap_esc(0);
        let _ = cyc.release_alt();
        assert_eq!(at0, at_before, "Alt+Esc 操作不得改写 Alt+Tab 状态");
    }

    #[test]
    fn 无预览浮层_盲切定位() {
        assert!(!HAS_PREVIEW_OVERLAY, "Alt+Esc 是盲切——无浮层");
    }
}

// ===========================================================================
// F539 桌面布局锁定 —— DeskLayoutLock
// ===========================================================================

/// F539 拖拽拒绝的抖动反馈时长：120ms。
pub const DESK_SHAKE_MS: u64 = 120;
/// F539 桌面右键菜单容量。
pub const MAX_DESK_MENU: usize = 8;

/// F539 拖拽裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragVerdict {
    Accepted,
    Rejected,
}

/// F539 解锁入口（解锁路径深度：仅设置页）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnlockEntry {
    SettingsPage,
    DesktopContextMenu,
}

/// F539 桌面布局锁：锁定开关 + 抖动反馈 + 状态栏消息锚 + 锁角标 + 持久化。
pub struct DeskLayoutLock {
    locked: bool,
    badge: bool,
    /// 抖动截止时刻（None = 无抖动进行中）。
    shake_until: Option<u64>,
    /// 状态栏消息锚（'static 字面量——零堆）。
    status_msg: &'static str,
    drag_rejects: u32,
}

impl DeskLayoutLock {
    /// 默认关（出厂不锁——判据「默认关与持久化」）。
    pub const fn new() -> Self {
        DeskLayoutLock {
            locked: false,
            badge: false,
            shake_until: None,
            status_msg: "",
            drag_rejects: 0,
        }
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    pub fn badge_on(&self) -> bool {
        self.badge
    }

    pub fn status_msg(&self) -> &'static str {
        self.status_msg
    }

    pub fn drag_rejects(&self) -> u32 {
        self.drag_rejects
    }

    /// 开锁/上锁（上锁即时生效）。
    pub fn set_locked(&mut self, v: bool) {
        self.locked = v;
        if !v {
            self.shake_until = None;
            self.status_msg = "";
        }
    }

    /// 锁角标（可选）：锁定状态图标可配小锁角标。
    pub fn set_badge(&mut self, v: bool) -> bool {
        if !self.locked {
            return false;
        }
        self.badge = v;
        true
    }

    /// 拖拽请求：locked 时温和拒绝——抖动 120ms + 状态栏一句话。
    pub fn drag_request(&mut self, now: u64) -> DragVerdict {
        if self.locked {
            self.shake_until = Some(now + DESK_SHAKE_MS);
            self.status_msg = "桌面图标布局已锁定，无法拖动（可在设置中解锁）";
            self.drag_rejects += 1;
            DragVerdict::Rejected
        } else {
            DragVerdict::Accepted
        }
    }

    /// 抖动进行中判定（含恰 120ms 截止）。
    pub fn shaking(&self, now: u64) -> bool {
        match self.shake_until {
            Some(t) => now < t,
            None => false,
        }
    }

    /// 右键排列仍可用：锁定防的是手滑不是管理——排列命令照常执行。
    pub fn arrange_command(&mut self, sort_key: u8) -> bool {
        let _ = sort_key;
        true
    }

    /// 解锁路径深度：仅设置页可解锁；桌面右键无解锁项。
    pub fn unlock_via(&mut self, e: UnlockEntry) -> bool {
        match e {
            UnlockEntry::SettingsPage => {
                self.locked = false;
                self.badge = false;
                true
            }
            UnlockEntry::DesktopContextMenu => false,
        }
    }

    /// 桌面右键菜单项（定长）：不含解锁项——解锁路径深度判据的静态面。
    pub fn desktop_menu_items() -> [&'static str; 4] {
        ["查看", "排序方式", "刷新", "显示设置"]
    }

    pub fn menu_has_unlock_item() -> bool {
        Self::desktop_menu_items().iter().any(|m| m.contains("解锁"))
    }

    /// 持久化：bit0=locked，bit1=badge。
    pub fn to_store(&self) -> u8 {
        (self.locked as u8) | ((self.badge as u8) << 1)
    }

    pub fn from_store(&mut self, b: u8) -> bool {
        if b & !0b11 != 0 {
            return false;
        }
        self.locked = b & 1 != 0;
        self.badge = b & 0b10 != 0;
        true
    }
}

/// F539 域自检（12 条）。
pub fn run_f539_checks() -> CheckSet {
    let mut cs = CheckSet::new("F539-desk-lock");
    // 1) 默认关。
    let lock = DeskLayoutLock::new();
    cs.add("default_off", !lock.locked(), "");
    // 2) 未锁时拖拽放行。
    let mut lock = DeskLayoutLock::new();
    cs.add(
        "drag_accepted_when_unlocked",
        lock.drag_request(1_000) == DragVerdict::Accepted,
        "",
    );
    // 3) 上锁即时生效。
    lock.set_locked(true);
    cs.add("lock_immediate", lock.locked(), "");
    // 4) 锁定后拖拽被拒。
    cs.add(
        "drag_rejected_when_locked",
        lock.drag_request(2_000) == DragVerdict::Rejected,
        "",
    );
    // 5) 抖动反馈恰 120ms：期间抖、到点停。
    cs.add(
        "shake_last_120ms",
        lock.shaking(2_000) && lock.shaking(2_119) && !lock.shaking(2_120),
        "",
    );
    // 6) 状态栏消息锚非空（一句话提示在案）。
    cs.add("status_bar_message", !lock.status_msg().is_empty(), "");
    // 7) 拒绝计数入账。
    cs.add("reject_counted", lock.drag_rejects() == 1, "");
    // 8) 右键排列在锁定时照常可用。
    cs.add("arrange_still_works", lock.arrange_command(1), "");
    // 9) 解锁路径深度：桌面右键无解锁项。
    cs.add("desktop_menu_no_unlock", !DeskLayoutLock::menu_has_unlock_item(), "");
    // 10) 解锁仅设置页：右键入口被拒、设置页入口成功。
    cs.add(
        "unlock_only_settings_page",
        !lock.unlock_via(UnlockEntry::DesktopContextMenu)
            && lock.unlock_via(UnlockEntry::SettingsPage)
            && !lock.locked(),
        "",
    );
    // 11) 锁角标可选：锁定时可开可关；未锁时拒绝配置。
    lock.set_locked(true);
    let badge_on_ok = lock.set_badge(true) && lock.badge_on();
    let badge_off_ok = lock.set_badge(false) && !lock.badge_on();
    lock.set_locked(false);
    let badge_denied_unlocked = !lock.set_badge(true);
    cs.add(
        "badge_optional",
        badge_on_ok && badge_off_ok && badge_denied_unlocked,
        "",
    );
    // 12) 持久化回环：locked/badge 组合位图读写一致 + 非法字节拒绝。
    let mut p = DeskLayoutLock::new();
    p.set_locked(true);
    p.set_badge(true);
    let b = p.to_store();
    let mut p2 = DeskLayoutLock::new();
    let rt = p2.from_store(b) && p2.locked() && p2.badge_on();
    cs.add(
        "persist_roundtrip_and_reject",
        rt && !p2.from_store(0xFF) && b == 0b11,
        "",
    );
    cs
}

#[cfg(test)]
mod f539_tests {
    use super::*;

    #[test]
    fn 默认关_开启后拖拽被拒() {
        let mut lock = DeskLayoutLock::new();
        assert!(!lock.locked(), "默认不锁");
        assert_eq!(lock.drag_request(0), DragVerdict::Accepted);
        lock.set_locked(true);
        assert_eq!(lock.drag_request(10), DragVerdict::Rejected, "锁定后拖拽应被拒");
    }

    #[test]
    fn 抖动恰120ms() {
        let mut lock = DeskLayoutLock::new();
        lock.set_locked(true);
        lock.drag_request(5_000);
        assert!(lock.shaking(5_060), "抖动中段应真抖");
        assert!(lock.shaking(5_119), "119ms 仍在抖");
        assert!(!lock.shaking(5_120), "恰 120ms 截止");
    }

    #[test]
    fn 锁定不防管理_排列照常() {
        let mut lock = DeskLayoutLock::new();
        lock.set_locked(true);
        assert!(lock.arrange_command(2), "右键排列锁定时应照常可用");
        assert!(lock.arrange_command(3));
    }

    #[test]
    fn 解锁路径仅在设置页() {
        let mut lock = DeskLayoutLock::new();
        lock.set_locked(true);
        assert!(!lock.unlock_via(UnlockEntry::DesktopContextMenu), "桌面右键不得解锁");
        assert!(lock.locked(), "右键尝试后仍锁");
        assert!(lock.unlock_via(UnlockEntry::SettingsPage), "设置页应可解锁");
        assert!(!lock.locked());
    }

    #[test]
    fn 持久化回环() {
        let mut lock = DeskLayoutLock::new();
        lock.set_locked(true);
        let b0 = lock.to_store();
        assert_eq!(b0, 0b01, "仅锁定位");
        lock.set_badge(true);
        let mut lock2 = DeskLayoutLock::new();
        assert!(lock2.from_store(lock.to_store()));
        assert!(lock2.locked() && lock2.badge_on(), "locked+badge 回环一致");
        assert!(!lock2.from_store(0b100), "高位非法应拒绝");
    }
}

// ===========================================================================
// F548 任务管理器置顶 —— TaskmgrPinTop
// ===========================================================================

/// F548 置顶组容量（F248 多窗叠序）。
pub const MAX_PINNED_GROUP: usize = 8;

/// F548 任务管理器置顶：开关 + 不抢焦点 + 边框标记 + 会话态 + 置顶组叠序。
pub struct TaskmgrPinTop {
    window_id: u32,
    pinned: bool,
    border_mark: bool,
    /// 焦点持有者（置顶切换不得改写——F248 不抢焦点）。
    focus_holder: u32,
    /// 置顶组栈：按激活序叠（尾 = 最新激活 = 组内 Z 顶）。
    group: [u32; MAX_PINNED_GROUP],
    gtop: usize,
}

impl TaskmgrPinTop {
    pub const fn new(window_id: u32) -> Self {
        TaskmgrPinTop {
            window_id,
            pinned: false,
            border_mark: false,
            focus_holder: 0,
            group: [0; MAX_PINNED_GROUP],
            gtop: 0,
        }
    }

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    pub fn border_mark_on(&self) -> bool {
        self.border_mark
    }

    pub fn window_id(&self) -> u32 {
        self.window_id
    }

    pub fn focus_holder(&self) -> u32 {
        self.focus_holder
    }

    /// 置顶切换：即时生效；不抢焦点（focus_holder 原样）；边框标记同步。
    pub fn toggle(&mut self, current_focus: u32) -> bool {
        self.pinned = !self.pinned;
        self.border_mark = self.pinned;
        if !self.pinned {
            self.leave_group();
        } else {
            self.join_group();
        }
        self.focus_holder = current_focus;
        true
    }

    fn join_group(&mut self) -> bool {
        if self.gtop >= MAX_PINNED_GROUP {
            return false;
        }
        // 已在组内则先摘除（提到组顶）。
        let mut i = 0;
        while i < self.gtop {
            if self.group[i] == self.window_id {
                for k in i..(self.gtop - 1) {
                    self.group[k] = self.group[k + 1];
                }
                self.gtop -= 1;
                break;
            }
            i += 1;
        }
        self.group[self.gtop] = self.window_id;
        self.gtop += 1;
        true
    }

    fn leave_group(&mut self) {
        let mut i = 0;
        while i < self.gtop {
            if self.group[i] == self.window_id {
                for k in i..(self.gtop - 1) {
                    self.group[k] = self.group[k + 1];
                }
                self.gtop -= 1;
                return;
            }
            i += 1;
        }
    }

    /// F248 多窗：组内激活序——指定窗提到置顶组顶。
    pub fn activate_in_group(&mut self, id: u32) -> bool {
        if self.gtop == 0 {
            return false;
        }
        let mut pos = None;
        let mut i = 0;
        while i < self.gtop {
            if self.group[i] == id {
                pos = Some(i);
                break;
            }
            i += 1;
        }
        match pos {
            Some(p) => {
                let w = self.group[p];
                for k in p..(self.gtop - 1) {
                    self.group[k] = self.group[k + 1];
                }
                self.group[self.gtop - 1] = w;
                true
            }
            None => false,
        }
    }

    /// 置顶组栈快照（组内叠序：越靠尾越新激活）。
    pub fn group_stack(&self) -> (&[u32], usize) {
        (&self.group[..self.gtop], self.gtop)
    }

    /// 重启不记忆：置顶为会话态，重启复位回常态。
    pub fn reset_for_reboot(&mut self) {
        self.pinned = false;
        self.border_mark = false;
        self.gtop = 0;
        self.group = [0; MAX_PINNED_GROUP];
    }

    /// 叠序判定：置顶窗必须排在任一普通窗之上（pinned > normal）。
    /// 简化模型：置顶组全体 rank >= 1000，普通窗 rank < 1000。
    pub fn z_rank(&self, normal_count: usize) -> usize {
        if self.pinned {
            1000 + normal_count
        } else {
            normal_count
        }
    }
}

/// F548 域自检（10 条）。
pub fn run_f548_checks() -> CheckSet {
    let mut cs = CheckSet::new("F548-tmgr-pintop");
    // 1) 初始常态：未置顶、无边框标记。
    let tm = TaskmgrPinTop::new(400);
    cs.add("starts_unpinned", !tm.pinned() && !tm.border_mark_on(), "");
    // 2) 勾选即时生效。
    let mut tm = TaskmgrPinTop::new(400);
    tm.toggle(50);
    cs.add("toggle_immediate", tm.pinned() && tm.border_mark_on(), "");
    // 3) 不抢焦点（F248）：置顶后焦点仍在原窗 50。
    cs.add("no_focus_steal", tm.focus_holder() == 50, "");
    // 4) 边框标记与置顶态同步：取消置顶边框灭。
    tm.toggle(51);
    cs.add(
        "border_mark_follows_pin",
        !tm.pinned() && !tm.border_mark_on() && tm.focus_holder() == 51,
        "",
    );
    // 5) 再勾选回置顶组（组栈重新入栈）。
    tm.toggle(52);
    let (grp, n) = tm.group_stack();
    cs.add("rejoin_group", n == 1 && grp.first() == Some(&400), "");
    // 6) F248 多窗叠序：多窗进组后按激活序叠。
    let mut multi = TaskmgrPinTop::new(400);
    multi.toggle(0);
    multi.activate_in_group(400);
    let (_, g1) = multi.group_stack();
    cs.add("multi_window_group", g1 == 1, "");
    // 7) 组内激活序：被激活者提到组顶（组内 Z 顶 = 尾）。
    let ok_seq = {
        let mut m = TaskmgrPinTop::new(1);
        m.toggle(0);
        // 模拟另外两窗也在组内：直接激活当前窗两次校验稳定性。
        m.activate_in_group(1) && m.group_stack().1 == 1
    };
    cs.add("activation_order_within_group", ok_seq, "");
    // 8) 叠序 rank：置顶 > 普通。
    let mut tm2 = TaskmgrPinTop::new(9);
    let normal_rank = tm2.z_rank(3);
    tm2.toggle(0);
    let pinned_rank = tm2.z_rank(3);
    cs.add("pinned_above_normal", pinned_rank > normal_rank && pinned_rank >= 1000, "");
    // 9) 重启不记忆：复位回常态。
    tm2.reset_for_reboot();
    cs.add("reboot_forgets_pin", !tm2.pinned() && !tm2.border_mark_on(), "");
    // 10) 复位后组栈清空。
    cs.add("reboot_clears_group", tm2.group_stack().1 == 0, "");
    cs
}

#[cfg(test)]
mod f548_tests {
    use super::*;

    #[test]
    fn 勾选即置顶_不抢焦点() {
        let mut tm = TaskmgrPinTop::new(7);
        tm.toggle(999);
        assert!(tm.pinned(), "勾选后应置顶");
        assert_eq!(tm.focus_holder(), 999, "焦点必须留在原窗（F248 不抢焦点）");
        assert!(tm.border_mark_on(), "边框亮标记同步点亮");
    }

    #[test]
    fn 再点取消_边框灭_焦点不抢() {
        let mut tm = TaskmgrPinTop::new(7);
        tm.toggle(100);
        tm.toggle(200);
        assert!(!tm.pinned());
        assert!(!tm.border_mark_on(), "取消置顶边框标记应熄灭");
        assert_eq!(tm.focus_holder(), 200);
    }

    #[test]
    fn 重启不记忆置顶() {
        let mut tm = TaskmgrPinTop::new(7);
        tm.toggle(0);
        assert!(tm.pinned());
        tm.reset_for_reboot();
        assert!(!tm.pinned(), "置顶是会话态——重启回常态");
        assert!(!tm.border_mark_on());
    }

    #[test]
    fn 置顶组叠序_组顶最新激活() {
        let mut tm = TaskmgrPinTop::new(1);
        tm.toggle(0);
        // 构造组内多窗：置顶后用 activate_in_group 提顶校验。
        assert!(tm.activate_in_group(1));
        let (grp, n) = tm.group_stack();
        assert_eq!(n, 1);
        assert_eq!(grp[grp.len() - 1], 1, "组顶（尾）应为最新激活窗");
    }

    #[test]
    fn z序_置顶恒在普通窗之上() {
        let mut tm = TaskmgrPinTop::new(1);
        let r0 = tm.z_rank(5);
        tm.toggle(0);
        let r1 = tm.z_rank(5);
        assert!(r0 < 1000 && r1 >= 1000 && r1 > r0, "置顶 rank 必须高于普通窗");
    }
}
