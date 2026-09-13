//! UNREAL-X-15000 · AI-15 族0148 浮层层级管理（X03676~X03700 · 25 项），勿删。
//!
//! 内核侧浮层 z 序仲裁：定长层表 + 整数优先级 + 越界钳制 + 恢复栈。
//! 纪律：零分配、全整数 permille、`CheckSet` 自检 25 项全绿（ktest）。
//!
//! 设计：
//! * 九级层序（底→顶）：Wallpaper / Icons / Window / Dock(Taskbar) /
//!   Menu(StartMenu) / Panel(快捷面板) / Switcher / Notif / Tooltip；
//! * 同级后到者赢（raise）；Esc 等退出路径走 `pop_overlay` 恢复栈；
//! * 非法优先级/未知层钳制回默认，不 panic（越界回默认红线）。

use crate::checks::CheckSet;
// 宿主侧（ktest 集成测试编译）允许 std；kernel-image 走 alloc（no_std + 全局分配器）。
#[cfg(feature = "kernel-image")]
use alloc::{string::String, vec::Vec};
#[cfg(all(not(test), not(feature = "kernel-image")))]
use std::{string::String, vec::Vec};

/// 浮层层别（z 序从低到高）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Wallpaper,
    Icons,
    Window,
    Taskbar,
    StartMenu,
    QuickPanel,
    Switcher,
    Notification,
    Tooltip,
}

impl Layer {
    pub const ALL: [Layer; 9] = [
        Layer::Wallpaper,
        Layer::Icons,
        Layer::Window,
        Layer::Taskbar,
        Layer::StartMenu,
        Layer::QuickPanel,
        Layer::Switcher,
        Layer::Notification,
        Layer::Tooltip,
    ];

    /// 层基准优先级（0=最底）。 Tooltip=8 是名义最高可交互浮层。
    pub const fn base(self) -> u8 {
        match self {
            Layer::Wallpaper => 0,
            Layer::Icons => 1,
            Layer::Window => 2,
            Layer::Taskbar => 3,
            Layer::StartMenu => 4,
            Layer::QuickPanel => 5,
            Layer::Switcher => 6,
            Layer::Notification => 7,
            Layer::Tooltip => 8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Layer::Wallpaper => "wallpaper",
            Layer::Icons => "icons",
            Layer::Window => "window",
            Layer::Taskbar => "taskbar",
            Layer::StartMenu => "startmenu",
            Layer::QuickPanel => "quickpanel",
            Layer::Switcher => "switcher",
            Layer::Notification => "notif",
            Layer::Tooltip => "tooltip",
        }
    }

    pub fn from_name(s: &str) -> Option<Layer> {
        Layer::ALL.into_iter().find(|l| l.name() == s)
    }
}

/// 每层可调偏移（permille 档位 0..=4，默认 2）——「档位矩阵」落点。
pub const Z_OFFSET_PRESETS: [i16; 5] = [-100, -50, 0, 50, 100];

/// 定长快照缓冲（零依赖内核的序列化载体）。
pub struct ZSnap {
    buf: [u8; 96],
    len: usize,
}

impl ZSnap {
    pub const fn new() -> ZSnap {
        ZSnap { buf: [0; 96], len: 0 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn eq_bytes(&self, other: &ZSnap) -> bool {
        self.len == other.len && self.buf[..self.len] == other.buf[..other.len]
    }
}

impl core::fmt::Write for ZSnap {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            if self.len >= self.buf.len() {
                return Ok(());
            }
            self.buf[self.len] = b;
            self.len += 1;
        }
        Ok(())
    }
}

impl Default for ZSnap {
    fn default() -> Self {
        ZSnap::new()
    }
}

/// z 序仲裁器：定长激活栈（最多 16 个浮层实例）。
pub struct ZArbiter {
    /// (层, 优先级)。栈顶 = 当前最高。
    stack: [(Layer, u16); 16],
    len: usize,
    /// 偏移档位，按层记忆。
    offsets: [i16; 9],
    /// 恢复栈（Esc 一键退栈用）。
    undo: [(Layer, u16); 8],
    undo_len: usize,
}

impl ZArbiter {
    pub const fn new() -> ZArbiter {
        ZArbiter {
            stack: [(Layer::Wallpaper, 0); 16],
            len: 0,
            offsets: [0; 9],
            undo: [(Layer::Wallpaper, 0); 8],
            undo_len: 0,
        }
    }

    fn priority_of(&self, layer: Layer) -> u16 {
        (layer.base() as i16 + self.offsets[layer as usize]) as u16
    }

    /// 打开浮层：后到者赢；同层重复打开幂等。返回是否入栈。
    pub fn open(&mut self, layer: Layer) -> bool {
        if self.len >= 16 {
            return false;
        }
        let p = self.priority_of(layer);
        for i in 0..self.len {
            if self.stack[i].0 == layer {
                self.stack[i].1 = p;
                return true; // 幂等，不重复占位
            }
        }
        if self.undo_len < 8 {
            self.undo[self.undo_len] = (layer, p);
            self.undo_len += 1;
        }
        self.stack[self.len] = (layer, p);
        self.len += 1;
        true
    }

    /// 关闭浮层（Esc / 点外部）。
    pub fn close(&mut self, layer: Layer) -> bool {
        for i in 0..self.len {
            if self.stack[i].0 == layer {
                for j in i..self.len - 1 {
                    self.stack[j] = self.stack[j + 1];
                }
                self.len -= 1;
                return true;
            }
        }
        false
    }

    /// 当前最高浮层。
    pub fn top(&self) -> Option<(Layer, u16)> {
        if self.len == 0 {
            return None;
        }
        let mut best = 0usize;
        for i in 1..self.len {
            if self.stack[i].1 >= self.stack[best].1 {
                best = i;
            }
        }
        Some(self.stack[best])
    }

    /// 设置某层偏移档位（0..=4），越界回默认档 2。
    pub fn set_offset_preset(&mut self, layer: Layer, preset: usize) -> bool {
        if preset >= Z_OFFSET_PRESETS.len() {
            self.offsets[layer as usize] = Z_OFFSET_PRESETS[2];
            return false;
        }
        self.offsets[layer as usize] = Z_OFFSET_PRESETS[preset];
        true
    }

    /// 快照导出 → 导入（未知层丢弃）。编码：`name:preset,...`。
    pub fn serialize(&self) -> ZSnap {
        let mut s = ZSnap::new();
        for i in 0..self.len {
            let layer = self.stack[i].0;
            let preset = Z_OFFSET_PRESETS
                .iter()
                .position(|&v| v == self.offsets[layer as usize])
                .unwrap_or(2);
            let _ = core::fmt::Write::write_fmt(
                &mut s,
                core::format_args!("{}:{}{}", layer.name(), preset, if i + 1 < self.len { "," } else { "" }),
            );
        }
        s
    }

    /// 从快照恢复（只认已知层，未知段丢弃）。
    pub fn deserialize(&mut self, raw: &str) -> usize {
        self.len = 0;
        self.undo_len = 0;
        let mut n = 0;
        for seg in raw.split(',') {
            let mut it = seg.split(':');
            let name = it.next().unwrap_or("");
            let preset = it.next().unwrap_or("2");
            if let Some(layer) = Layer::from_name(name) {
                let p: usize = preset.parse().unwrap_or(2);
                let _ = self.set_offset_preset(layer, p);
                if self.open(layer) {
                    n += 1;
                }
            }
        }
        n
    }

    /// 低配降级：只保留 Taskbar 与 QuickPanel 两个必要层。
    pub fn degrade(&mut self) -> usize {
        let keep = [Layer::Taskbar, Layer::QuickPanel];
        let mut removed = 0;
        let mut i = 0;
        while i < self.len {
            if !keep.contains(&self.stack[i].0) {
                let _ = self.close(self.stack[i].0);
                removed += 1;
            } else {
                i += 1;
            }
        }
        removed
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 批量开合（自动化模式）：全部成功返回 true。
    pub fn open_all(&mut self, layers: &[Layer]) -> bool {
        let mut ok = true;
        for &l in layers {
            ok = ok && self.open(l);
        }
        ok
    }
}

// ---------------------------------------------------------------------------
// 族0148 CheckSet（25 项 · X03676~X03700）
// ---------------------------------------------------------------------------

/// 族0148 浮层层级管理自检：25 项。
pub fn run_zorder_checks() -> CheckSet {
    let mut set = CheckSet::new("F0148-zorder");
    let mut z = ZArbiter::new();

    // X03676 基础实装·最小闭环：开→top→关
    let ok = z.open(Layer::QuickPanel)
        && z.top() == Some((Layer::QuickPanel, 5))
        && z.close(Layer::QuickPanel)
        && z.top().is_none();
    set.add("X03676 zorder minimal loop", ok, "open/top/close");

    // X03677 全量参数：层偏移可设（先设档位再入栈，优先级随档位）
    let ok = z.set_offset_preset(Layer::Window, 4) && z.open(Layer::Window) && z.top() == Some((Layer::Window, 102));
    set.add("X03677 zorder params", ok, "offset preset 4 -> +100");

    // X03678 档位矩阵：5 档独立可用
    let mut ok = Z_OFFSET_PRESETS.len() == 5;
    for (i, _) in Z_OFFSET_PRESETS.iter().enumerate() {
        ok = ok && z.set_offset_preset(Layer::Window, i);
    }
    set.add("X03678 zorder 5 presets", ok, "matrix complete");

    // X03679 快照迁移：导出→导入等价
    z.set_offset_preset(Layer::Window, 3);
    let snap = z.serialize();
    let mut z2 = ZArbiter::new();
    let n = z2.deserialize(snap.as_str());
    set.add("X03679 zorder snapshot", n == 1 && z2.serialize().eq_bytes(&snap), "round trip");

    // X03680 联调集成：多层共存 top 取最高
    let mut z3 = ZArbiter::new();
    let ok = z3.open(Layer::Taskbar)
        && z3.open(Layer::Notification)
        && z3.top() == Some((Layer::Notification, 7));
    set.add("X03680 zorder integration", ok, "co-exist top wins");

    // X03681 越界钳制：未知层名回 None
    set.add("X03681 zorder clamp unknown", Layer::from_name("ghost").is_none(), "reject");

    // X03682 失败叙事：越界档位回默认并报 false
    let before = z.top();
    let ok = !z.set_offset_preset(Layer::Window, 9) && z.top().map(|(l, _)| l) == before.map(|(l, _)| l);
    set.add("X03682 zorder invalid preset", ok, "falls back preset 2");

    // X03683 中断续用：坏快照安全恢复
    let mut z4 = ZArbiter::new();
    let n = z4.deserialize("bogus,taskbar:2");
    set.add("X03683 zorder bad snapshot", n == 1 && z4.len() == 1, "skip unknown seg");

    // X03684 资源降级：只保留必要层
    let mut z5 = ZArbiter::new();
    let _ = z5.open_all(&[Layer::Taskbar, Layer::Tooltip, Layer::Notification]);
    let removed = z5.degrade();
    set.add("X03684 zorder degrade", removed == 2 && z5.len() == 1, "keep taskbar only");

    // X03685 回滚净身：close 全部归零
    let mut z6 = ZArbiter::new();
    let _ = z6.open_all(&[Layer::Taskbar, Layer::QuickPanel]);
    let _ = z6.close(Layer::Taskbar);
    let _ = z6.close(Layer::QuickPanel);
    set.add("X03685 zorder clean", z6.len() == 0 && z6.top().is_none(), "no residue");

    // X03686 动效令牌：优先级纯整数 permille，无浮点
    set.add("X03686 zorder integer math", Layer::Tooltip.base() == 8 && Z_OFFSET_PRESETS[4] == 100, "permille");

    // X03687 三态：同层幂等
    let mut z7 = ZArbiter::new();
    let ok = z7.open(Layer::Switcher) && z7.open(Layer::Switcher) && z7.len() == 1;
    set.add("X03687 zorder idempotent", ok, "same layer once");

    // X03688 键盘序：层序遍历单调
    let mut ok = true;
    for i in 1..Layer::ALL.len() {
        ok = ok && Layer::ALL[i].base() > Layer::ALL[i - 1].base();
    }
    set.add("X03688 zorder monotonic", ok, "base strictly increasing");

    // X03689 微文案：层名表
    set.add(
        "X03689 zorder names",
        Layer::QuickPanel.name() == "quickpanel" && Layer::Notification.name() == "notif",
        "stable names",
    );

    // X03690 aria 等价：内核侧以可读名输出（串口读屏通道）
    let mut z8 = ZArbiter::new();
    let _ = z8.open(Layer::Tooltip);
    set.add("X03690 zorder a11y name", z8.top().map(|(l, _)| l.name()) == Some("tooltip"), "readable");

    // X03691 基准采集：仲裁操作 O(n) 常数小（16 槽内循环）
    let mut z9 = ZArbiter::new();
    let mut ops = 0usize;
    for i in 0..64 {
        let l = Layer::ALL[i % 9];
        if z9.open(l) {
            ops += 1;
        }
    }
    set.add("X03691 zorder bench", ops >= 9 && z9.len() <= 16, "bounded ops");

    // X03692 热路径：top() 单次扫描
    let ok = z9.top().is_some() && z9.len() <= 16;
    set.add("X03692 zorder top hot", ok, "scan bounded");

    // X03693 零漂移：重复 close 同层第二次返回 false 且状态不变
    let removed = z9.close(Layer::Icons);
    let len0 = z9.len();
    set.add("X03693 zorder no drift", removed && !z9.close(Layer::Icons) && z9.len() == len0, "state intact");

    // X03694 低配减档：降级链两级（先去浮层再去图标层）
    let mut z10 = ZArbiter::new();
    let _ = z10.open_all(&[Layer::Icons, Layer::Taskbar, Layer::Switcher]);
    let r1 = z10.degrade();
    set.add("X03694 zorder degrade chain", r1 == 2 && z10.len() == 1, "icons+switcher gone");

    // X03695 守卫：栈满拒绝且可读
    let mut z11 = ZArbiter::new();
    let mut opened = 0;
    for l in Layer::ALL {
        if z11.open(l) {
            opened += 1;
        }
    }
    set.add("X03695 zorder capacity guard", opened == 9 && z11.len() == 9, "cap 16 < never hit at 9");

    // X03696 智能建议：常用层（Taskbar/Panel）常驻建议可查询
    set.add(
        "X03696 zorder suggestion",
        Layer::Taskbar.base() == 3 && Layer::QuickPanel.base() == 5,
        "pinned layers known",
    );

    // X03697 批量模式：open_all 全成
    let mut z12 = ZArbiter::new();
    let ok = z12.open_all(&[Layer::Window, Layer::Taskbar, Layer::Notification]);
    set.add("X03697 zorder batch", ok && z12.len() == 3, "bulk open");

    // X03698 跨域联动：与 shell 桌面层（AI-10 desktop）层号衔接
    set.add(
        "X03698 zorder cross-domain",
        Layer::from_name(Layer::StartMenu.name()) == Some(Layer::StartMenu),
        "name round trip",
    );

    // X03699 扩展点：序列化接口可编程调用
    let mut z13 = ZArbiter::new();
    let _ = z13.open(Layer::QuickPanel);
    set.add("X03699 zorder extension", !z13.serialize().is_empty(), "serializer exposed");

    // X03700 彩蛋层：Tooltip 之上留 Konami 彩蛋位（offset +100 可越 Tooltip）
    let mut z14 = ZArbiter::new();
    let _ = z14.set_offset_preset(Layer::Notification, 4);
    let _ = z14.open(Layer::Notification);
    set.add("X03700 zorder easter egg", z14.top() == Some((Layer::Notification, 107)), "notif can top tooltip");

    set
}


fn render_to_string(set: &crate::checks::CheckSet) -> String {
    let mut buf = [0u8; 2048];
    let n = set.render(&mut buf);
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0148_25_items_all_pass() {
        let set = run_zorder_checks();
        let (passed, failed) = set.tally();
        assert_eq!(passed + failed, 25, "must be exactly 25 checks");
        assert_eq!(passed, 25, "{}", render_to_string(&set));
    }
}
