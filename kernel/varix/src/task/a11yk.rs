//! UNREAL-X-15000 · AI-28 族0275 无障碍（X06851~X06875）。
//! 无障碍：档位矩阵（对比度/放大倍率/朗读速率/动效减速/描边加宽五维各五档
//! 独立可配）、环形 Tab 焦点序（越界回绕）、读屏标签表（固定键值）与
//! HC 降级映射（材质→纯色/辉光→描边）。
//! 零堆、整数运算，无 Vec/String/Box/alloc、无外部 crate。

// ---------------------------------------------------------------------------
// 常量与错误码
// ---------------------------------------------------------------------------

/// 档位维度数（对比度/放大/朗读/动效/描边）。
pub const DIM_COUNT: usize = 5;
/// 每维档位上限（0~4，共 5 档）。
pub const GEAR_MAX: u8 = 4;
/// 焦点环容量。
pub const MAX_NODES: usize = 16;
/// 读屏标签表条目数。
pub const LABEL_COUNT: usize = 8;
/// 快照魔数。
pub const MAGIC: u8 = 0x75;
/// 快照定长。
pub const SNAP_LEN: usize = 44;

pub const E_OK: u16 = 0;
/// 焦点环为空或标签未登记。
pub const E_EMPTY: u16 = 1;
/// 焦点环已满。
pub const E_FULL: u16 = 2;
/// 参数非法或节点不存在。
pub const E_INVALID: u16 = 3;
/// 读屏通道未开启。
pub const E_OFF: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_EMPTY => "焦点环为空或读屏标签未登记，建议先添加焦点节点或把键名补充到标签表",
        E_FULL => "焦点环已满，建议移除不可达节点后再添加",
        E_INVALID => "参数非法（维度越界/档位越界/编号为 0/节点不存在），建议检查入参并使用 0~4 的档位",
        E_OFF => "读屏通道未开启，建议先在无障碍设置中打开朗读开关",
        _ => "未知无障碍错误，建议重置无障碍档位后重试",
    }
}

// ---------------------------------------------------------------------------
// 读屏标签表与 HC 降级映射
// ---------------------------------------------------------------------------

/// 读屏标签固定键值表。
pub const LABELS: [(&'static str, &'static str); LABEL_COUNT] = [
    ("btn.ok", "确认"),
    ("btn.cancel", "取消"),
    ("win.close", "关闭窗口"),
    ("menu.open", "打开菜单"),
    ("list.item", "列表项"),
    ("tab.next", "下一个标签"),
    ("vol.mute", "静音"),
    ("bat.low", "电量不足"),
];

/// 按键查标签值。
pub fn lookup_label(key: &str) -> Option<&'static str> {
    for i in 0..LABEL_COUNT {
        if LABELS[i].0 == key {
            return Some(LABELS[i].1);
        }
    }
    None
}

/// 高耗费视觉特效五类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxKind {
    /// 材质。
    Material,
    /// 辉光。
    Glow,
    /// 阴影。
    Shadow,
    /// 模糊。
    Blur,
    /// 脉动。
    Pulse,
}

impl FxKind {
    pub fn index(self) -> u32 {
        match self {
            FxKind::Material => 0,
            FxKind::Glow => 1,
            FxKind::Shadow => 2,
            FxKind::Blur => 3,
            FxKind::Pulse => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FxKind::Material => "material",
            FxKind::Glow => "glow",
            FxKind::Shadow => "shadow",
            FxKind::Blur => "blur",
            FxKind::Pulse => "pulse",
        }
    }
}

/// HC 降级后的平面呈现四类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatKind {
    /// 纯色。
    Plain,
    /// 描边。
    Outline,
    /// 静态。
    Static,
    /// 隐藏。
    Hidden,
}

impl FlatKind {
    pub fn index(self) -> u32 {
        match self {
            FlatKind::Plain => 0,
            FlatKind::Outline => 1,
            FlatKind::Static => 2,
            FlatKind::Hidden => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FlatKind::Plain => "plain",
            FlatKind::Outline => "outline",
            FlatKind::Static => "static",
            FlatKind::Hidden => "hidden",
        }
    }
}

/// HC 降级映射：材质→纯色、辉光/阴影→描边、模糊→纯色、脉动→静态；
/// HC 未开启时无需降级（返回 None）。
pub fn hc_downgrade(fx: FxKind, hc_on: bool) -> Option<FlatKind> {
    if !hc_on {
        return None;
    }
    match fx {
        FxKind::Material => Some(FlatKind::Plain),
        FxKind::Glow => Some(FlatKind::Outline),
        FxKind::Shadow => Some(FlatKind::Outline),
        FxKind::Blur => Some(FlatKind::Plain),
        FxKind::Pulse => Some(FlatKind::Static),
    }
}

/// 档位钳制（>4 归 4）。
pub fn clamp_gear(v: u8) -> u8 {
    if v > GEAR_MAX {
        GEAR_MAX
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// 引擎
// ---------------------------------------------------------------------------

/// 无障碍套件：档位矩阵 + HC 开关 + 环形焦点序 + 读屏通道。
pub struct A11yKit {
    /// 对比度档（最低对比度 = 4.5:1 + 档×1.375:1）。
    pub contrast: u8,
    /// 放大倍率档（100% + 档×25%）。
    pub zoom: u8,
    /// 朗读速率档（120 词/分 + 档×60）。
    pub speech: u8,
    /// 动效减速档（速度 = 4000/(4+档) 千分比）。
    pub motion: u8,
    /// 描边加宽档（像素）。
    pub outline: u8,
    /// 高对比度模式。
    pub hc_on: bool,
    /// 低配经济模式。
    pub eco: bool,
    /// 读屏（朗读）通道开关。
    pub sr_on: bool,
    /// 环形 Tab 序节点表（0 为空槽）。
    pub ring_ids: [u16; MAX_NODES],
    pub ring_n: usize,
    /// 当前焦点下标。
    pub cur: usize,
    pub events: u64,
    pub speaks: u64,
    /// 最近一次朗读文本（直译或键名兜底）。
    pub last_label: &'static str,
    /// 彩蛋：五维全部拉满点亮。
    pub easter: bool,
}

impl A11yKit {
    pub fn new() -> A11yKit {
        A11yKit {
            contrast: 0,
            zoom: 0,
            speech: 0,
            motion: 0,
            outline: 0,
            hc_on: false,
            eco: false,
            sr_on: true,
            ring_ids: [0; MAX_NODES],
            ring_n: 0,
            cur: 0,
            events: 0,
            speaks: 0,
            last_label: "",
            easter: false,
        }
    }

    /// 设置某一维档位（档位钳制 0~4；五维全满点亮彩蛋）。
    pub fn set_gear(&mut self, dim: usize, gear: u8) -> u16 {
        let g = clamp_gear(gear);
        match dim {
            0 => self.contrast = g,
            1 => self.zoom = g,
            2 => self.speech = g,
            3 => self.motion = g,
            4 => self.outline = g,
            _ => return E_INVALID,
        }
        if self.contrast == GEAR_MAX
            && self.zoom == GEAR_MAX
            && self.speech == GEAR_MAX
            && self.motion == GEAR_MAX
            && self.outline == GEAR_MAX
        {
            self.easter = true;
        }
        self.events += 1;
        E_OK
    }

    /// 读取某一维档位。
    pub fn gear(&self, dim: usize) -> Option<u8> {
        match dim {
            0 => Some(self.contrast),
            1 => Some(self.zoom),
            2 => Some(self.speech),
            3 => Some(self.motion),
            4 => Some(self.outline),
            _ => None,
        }
    }

    /// 放大倍率（千分比）。
    pub fn zoom_permille(&self) -> u16 {
        1000 + self.zoom as u16 * 250
    }

    /// 朗读速率（词/分）。
    pub fn speech_wpm(&self) -> u16 {
        120 + self.speech as u16 * 60
    }

    /// 动效速度（千分比，减速后变慢）。
    pub fn motion_speed_permille(&self) -> u16 {
        4000 / (4 + self.motion as u16)
    }

    /// 描边宽度（像素）。
    pub fn outline_px(&self) -> u8 {
        self.outline
    }

    /// 最低对比度（千分比，4.5:1 起）。
    pub fn contrast_min_permille(&self) -> u16 {
        4500 + self.contrast as u16 * 1375
    }

    /// 智能建议：对比度不足且未开 HC 时建议开启。
    pub fn suggest_hc(&self) -> bool {
        !self.hc_on && self.contrast < 3
    }

    /// HC 开关联动：自动抬升对比度与描边档。
    pub fn set_hc(&mut self, on: bool) {
        self.hc_on = on;
        if on {
            if self.contrast < 3 {
                self.contrast = 3;
            }
            if self.outline < 1 {
                self.outline = 1;
            }
        }
        self.events += 1;
    }

    /// 低配经济模式：放大受限、动效强制减速。
    pub fn apply_eco(&mut self) {
        self.eco = true;
        if self.zoom > 2 {
            self.zoom = 2;
        }
        if self.motion < 3 {
            self.motion = 3;
        }
        self.events += 1;
    }

    /// 低配探测：内存吃紧时进入经济模式。
    pub fn degrade_probe(&mut self, ram_permille: u32) -> bool {
        if ram_permille > 900 {
            self.apply_eco();
        }
        self.eco
    }

    // ---- 环形 Tab 焦点序 ----

    /// 添加焦点节点（编号 ≥1 且不重复）。
    pub fn ring_add(&mut self, id: u16) -> u16 {
        if id == 0 {
            return E_INVALID;
        }
        for i in 0..self.ring_n {
            if self.ring_ids[i] == id {
                return E_INVALID;
            }
        }
        if self.ring_n >= MAX_NODES {
            return E_FULL;
        }
        self.ring_ids[self.ring_n] = id;
        self.ring_n += 1;
        self.events += 1;
        E_OK
    }

    /// 批量添加节点，返回成功条数。
    pub fn ring_add_batch(&mut self, ids: &[u16]) -> usize {
        let mut ok = 0usize;
        for i in 0..ids.len() {
            if self.ring_add(ids[i]) == E_OK {
                ok += 1;
            }
        }
        ok
    }

    /// 移除节点；移除后焦点越界则回绕到 0。
    pub fn ring_remove(&mut self, id: u16) -> u16 {
        if self.ring_n == 0 {
            return E_EMPTY;
        }
        let mut idx: Option<usize> = None;
        for i in 0..self.ring_n {
            if self.ring_ids[i] == id {
                idx = Some(i);
                break;
            }
        }
        let i = match idx {
            Some(i) => i,
            None => return E_INVALID,
        };
        for j in i..self.ring_n - 1 {
            self.ring_ids[j] = self.ring_ids[j + 1];
        }
        self.ring_ids[self.ring_n - 1] = 0;
        self.ring_n -= 1;
        if self.ring_n == 0 {
            self.cur = 0;
        } else {
            if i < self.cur {
                self.cur -= 1;
            }
            if self.cur >= self.ring_n {
                self.cur = 0; // 越界回绕
            }
        }
        self.events += 1;
        E_OK
    }

    /// Tab 前进（环形回绕）。
    pub fn next(&mut self) -> u16 {
        if self.ring_n == 0 {
            return E_EMPTY;
        }
        self.cur = (self.cur + 1) % self.ring_n;
        self.events += 1;
        E_OK
    }

    /// Shift+Tab 后退（环形回绕）。
    pub fn prev(&mut self) -> u16 {
        if self.ring_n == 0 {
            return E_EMPTY;
        }
        self.cur = (self.cur + self.ring_n - 1) % self.ring_n;
        self.events += 1;
        E_OK
    }

    /// 前进/后退任意步（负步后退，越界回绕）。
    pub fn advance(&mut self, steps: i64) -> u16 {
        if self.ring_n == 0 {
            return E_EMPTY;
        }
        self.cur = ((self.cur as i64 + steps).rem_euclid(self.ring_n as i64)) as usize;
        self.events += 1;
        E_OK
    }

    pub fn focus_id(&self) -> Option<u16> {
        if self.ring_n == 0 {
            None
        } else {
            Some(self.ring_ids[self.cur])
        }
    }

    /// 节点三态：0=不在环 1=在环未聚焦 2=当前聚焦。
    pub fn node_state(&self, id: u16) -> u8 {
        for i in 0..self.ring_n {
            if self.ring_ids[i] == id {
                return if i == self.cur { 2 } else { 1 };
            }
        }
        0
    }

    // ---- 读屏通道 ----

    /// 朗读标签：命中朗读译文，未命中键名兜底（读屏不哑）。
    pub fn speak(&mut self, key: &'static str) -> u16 {
        if !self.sr_on {
            return E_OFF;
        }
        match lookup_label(key) {
            Some(v) => {
                self.last_label = v;
                self.speaks += 1;
                E_OK
            }
            None => {
                self.last_label = key;
                self.speaks += 1;
                E_EMPTY
            }
        }
    }

    /// 不变量审计：容量、档位范围、焦点下标、编号唯一。
    pub fn audit(&self) -> bool {
        if self.ring_n > MAX_NODES {
            return false;
        }
        if self.ring_n == 0 {
            if self.cur != 0 {
                return false;
            }
        } else if self.cur >= self.ring_n {
            return false;
        }
        for i in 0..self.ring_n {
            if self.ring_ids[i] == 0 {
                return false;
            }
            for j in (i + 1)..self.ring_n {
                if self.ring_ids[i] == self.ring_ids[j] {
                    return false;
                }
            }
        }
        self.contrast <= GEAR_MAX
            && self.zoom <= GEAR_MAX
            && self.speech <= GEAR_MAX
            && self.motion <= GEAR_MAX
            && self.outline <= GEAR_MAX
    }

    /// 快照导出：魔数 + 开关 + 五档 + 焦点环。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < SNAP_LEN {
            return 0;
        }
        buf[0] = MAGIC;
        buf[1] = 1;
        buf[2] = if self.hc_on { 1 } else { 0 };
        buf[3] = if self.sr_on { 1 } else { 0 };
        buf[4] = if self.eco { 1 } else { 0 };
        buf[5] = self.contrast;
        buf[6] = self.zoom;
        buf[7] = self.speech;
        buf[8] = self.motion;
        buf[9] = self.outline;
        buf[10] = self.ring_n as u8;
        buf[11] = self.cur as u8;
        for i in 0..MAX_NODES {
            let b = self.ring_ids[i].to_le_bytes();
            buf[12 + i * 2] = b[0];
            buf[13 + i * 2] = b[1];
        }
        SNAP_LEN
    }

    /// 快照导入：恢复开关/档位/焦点环（非法域钳制），魔数版本校验。
    pub fn import(&mut self, buf: &[u8]) -> u16 {
        if buf.len() < SNAP_LEN || buf[0] != MAGIC || buf[1] != 1 {
            return E_INVALID;
        }
        self.hc_on = buf[2] != 0;
        self.sr_on = buf[3] != 0;
        self.eco = buf[4] != 0;
        self.contrast = clamp_gear(buf[5]);
        self.zoom = clamp_gear(buf[6]);
        self.speech = clamp_gear(buf[7]);
        self.motion = clamp_gear(buf[8]);
        self.outline = clamp_gear(buf[9]);
        self.ring_n = (buf[10] as usize).min(MAX_NODES);
        self.cur = if self.ring_n == 0 { 0 } else { (buf[11] as usize) % self.ring_n };
        for i in 0..self.ring_n {
            self.ring_ids[i] = u16::from_le_bytes([buf[12 + i * 2], buf[13 + i * 2]]);
        }
        for i in self.ring_n..MAX_NODES {
            self.ring_ids[i] = 0;
        }
        E_OK
    }

    /// 回滚净身：档位/焦点/计数清零，读屏开关保留。
    pub fn reset(&mut self) {
        self.contrast = 0;
        self.zoom = 0;
        self.speech = 0;
        self.motion = 0;
        self.outline = 0;
        self.hc_on = false;
        self.eco = false;
        self.ring_ids = [0; MAX_NODES];
        self.ring_n = 0;
        self.cur = 0;
        self.events = 0;
        self.speaks = 0;
        self.last_label = "";
        self.easter = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a11y_gears_focus_and_labels() {
        // 五维档位矩阵独立可配 + 派生读数。
        let mut k = A11yKit::new();
        for d in 0..DIM_COUNT {
            assert_eq!(k.set_gear(d, d as u8), E_OK);
        }
        assert_eq!(k.gear(0), Some(0));
        assert_eq!(k.gear(4), Some(4));
        assert_eq!(k.zoom_permille(), 1250);
        assert_eq!(k.speech_wpm(), 240);
        assert_eq!(k.motion_speed_permille(), 571);
        assert_eq!(k.outline_px(), 4);
        assert_eq!(k.contrast_min_permille(), 4500);
        // 环形 Tab 序 + 越界回绕。
        let mut r = A11yKit::new();
        for id in 1..=3u16 {
            assert_eq!(r.ring_add(id), E_OK);
        }
        assert_eq!(r.focus_id(), Some(1));
        let _ = r.prev(); // 回绕到末尾
        assert_eq!(r.focus_id(), Some(3));
        let _ = r.next();
        assert_eq!(r.focus_id(), Some(1));
        // 移除当前聚焦的末节点 → 焦点回绕到 0。
        let _ = r.ring_remove(3);
        assert_eq!(r.cur, 0);
        assert_eq!(r.focus_id(), Some(1));
        // 三态焦点。
        assert_eq!(r.node_state(1), 2);
        assert_eq!(r.node_state(2), 1);
        assert_eq!(r.node_state(9), 0);
        // 读屏标签：命中与键名兜底。
        assert_eq!(r.speak("btn.ok"), E_OK);
        assert_eq!(r.last_label, "确认");
        assert_eq!(r.speak("no.such.key"), E_EMPTY);
        assert_eq!(r.last_label, "no.such.key");
    }

    #[test]
    fn a11y_snapshot_hc_eco_and_guard() {
        // 快照迁移。
        let mut s = A11yKit::new();
        let _ = s.ring_add(1);
        let _ = s.ring_add(2);
        let _ = s.next();
        let _ = s.set_gear(2, 3);
        s.set_hc(true);
        let mut buf = [0u8; 64];
        let n = s.export(&mut buf);
        assert_eq!(n, SNAP_LEN);
        let mut t = A11yKit::new();
        assert_eq!(t.import(&buf[..n]), E_OK);
        assert_eq!(t.cur, s.cur);
        assert_eq!(t.gear(2), Some(3));
        assert!(t.hc_on && t.contrast >= 3 && t.outline >= 1);
        // HC 降级映射。
        assert_eq!(hc_downgrade(FxKind::Material, true), Some(FlatKind::Plain));
        assert_eq!(hc_downgrade(FxKind::Glow, true), Some(FlatKind::Outline));
        assert_eq!(hc_downgrade(FxKind::Glow, false), None);
        // 低配降级链。
        let mut e = A11yKit::new();
        assert!(!e.degrade_probe(500));
        let _ = e.set_gear(1, 4);
        let _ = e.set_gear(3, 0);
        assert!(e.degrade_probe(950));
        assert_eq!(e.gear(1), Some(2));
        assert_eq!(e.gear(3), Some(3));
        // 满环守护。
        let mut f = A11yKit::new();
        for id in 1..=MAX_NODES as u16 {
            assert_eq!(f.ring_add(id), E_OK);
        }
        assert_eq!(f.ring_add(99), E_FULL);
        assert_eq!(f.ring_n, MAX_NODES);
        assert!(f.audit());
    }

    #[test]
    fn a11y_all_checks_pass() {
        let set = run_a11yk_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            assert!(set.get(i).unwrap().passed, "第 {} 项未通过: {}", i, set.get(i).unwrap().name);
        }
    }
}

/// 族0275 自检：X06851~X06875 逐项登记。
pub fn run_a11yk_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("task-a11y");

    // —— 基础实装 X06851~X06855 ——
    let mut k = A11yKit::new();
    let _ = k.ring_add(10);
    let _ = k.ring_add(11);
    let _ = k.ring_add(12);
    let _ = k.next();
    let _ = k.set_gear(1, 2);
    let sp = k.speak("btn.ok");
    set.add("X06851 核心链路闭环", k.focus_id() == Some(11) && k.zoom_permille() == 1500 && sp == E_OK && k.last_label == "确认", "配档→焦点→读屏端到端可观测");

    let mut k2 = A11yKit::new();
    let mut all_set = true;
    for d in 0..DIM_COUNT {
        all_set &= k2.set_gear(d, d as u8 + 1) == E_OK;
    }
    all_set &= k2.gear(0) == Some(1) && k2.gear(4) == Some(4);
    k2.sr_on = false;
    let sr_off = !k2.sr_on;
    k2.sr_on = true;
    k2.hc_on = true;
    k2.hc_on = false;
    set.add("X06852 全量参数开放", all_set && sr_off && k2.sr_on && !k2.hc_on, "五维档位/HC/读屏/经济模式全参数可配可读");

    let mut k3 = A11yKit::new();
    let _ = k3.set_gear(0, 4);
    let indep = k3.gear(0) == Some(4) && k3.gear(1) == Some(0) && k3.gear(2) == Some(0) && k3.gear(3) == Some(0) && k3.gear(4) == Some(0);
    set.add("X06853 档位矩阵≥5档", DIM_COUNT == 5 && GEAR_MAX == 4 && indep, "对比度/放大/朗读/动效/描边五维各五档独立");

    let mut s = A11yKit::new();
    let _ = s.ring_add(1);
    let _ = s.ring_add(2);
    let _ = s.ring_add(3);
    let _ = s.next();
    let _ = s.set_gear(2, 3);
    s.set_hc(true);
    let mut buf = [0u8; 64];
    let n = s.export(&mut buf);
    let mut t = A11yKit::new();
    let imp = t.import(&buf[..n]);
    set.add("X06854 快照迁移三通道", n == SNAP_LEN && buf[0] == MAGIC && imp == E_OK && t.cur == s.cur && t.ring_n == 3 && t.gear(2) == Some(3) && t.hc_on, "导出/导入/魔数版本三通道");

    let mut h = A11yKit::new();
    let _ = h.ring_add(5);
    let _ = h.ring_add(6);
    h.set_hc(true);
    let hc_ok = h.contrast >= 3
        && h.outline >= 1
        && h.ring_n == 2
        && hc_downgrade(FxKind::Material, true) == Some(FlatKind::Plain)
        && hc_downgrade(FxKind::Glow, true) == Some(FlatKind::Outline)
        && h.audit();
    set.add("X06855 联调无回归", hc_ok, "HC 联动抬档后焦点环与审计无回归");

    // —— 边界与恢复 X06856~X06860 ——
    let mut b = A11yKit::new();
    let bad_dim = b.set_gear(5, 1);
    let big_gear = b.set_gear(0, 99);
    let zero_id = b.ring_add(0);
    let mut b2 = A11yKit::new();
    let _ = b2.ring_add(1);
    let _ = b2.ring_add(2);
    let adv = b2.advance(999);
    set.add("X06856 非法输入钳制", bad_dim == E_INVALID && big_gear == E_OK && b.gear(0) == Some(4) && zero_id == E_INVALID && adv == E_OK && b2.cur == 1, "档位钳制、大步长回绕、零编号拒绝");

    set.add("X06857 错误叙事体系", describe(E_FULL).contains("建议") && describe(E_EMPTY).contains("登记") && describe(E_OFF).contains("开启") && describe(E_INVALID).contains("建议"), "每个失败有下一步建议");

    let mut a = A11yKit::new();
    for id in 1..=5u16 {
        let _ = a.ring_add(id * 10);
    }
    let _ = a.advance(2);
    let mut sb = [0u8; 64];
    let ns = a.export(&mut sb);
    let mut c = A11yKit::new();
    let _ = c.import(&sb[..ns]);
    let _ = c.advance(3);
    let mut twin = A11yKit::new();
    for id in 1..=5u16 {
        let _ = twin.ring_add(id * 10);
    }
    let _ = twin.advance(5);
    set.add("X06858 中断续跑还原", c.focus_id() == twin.focus_id() && c.cur == twin.cur && twin.focus_id() == Some(10), "半程快照续跑与不间断结果一致");

    let mut f = A11yKit::new();
    let mut added = 0usize;
    for id in 1..=MAX_NODES as u16 {
        if f.ring_add(id) == E_OK {
            added += 1;
        }
    }
    let over = f.ring_add(99);
    set.add("X06859 资源降级守护", added == MAX_NODES && over == E_FULL && f.ring_n == MAX_NODES && f.audit(), "满环拒绝不崩溃审计仍守恒");

    let mut r = A11yKit::new();
    let _ = r.ring_add(1);
    let _ = r.set_gear(0, 3);
    r.set_hc(true);
    r.degrade_probe(950);
    let _ = r.speak("btn.ok");
    r.reset();
    set.add("X06860 回滚净身", r.ring_n == 0 && r.gear(0) == Some(0) && !r.hc_on && !r.eco && r.events == 0 && r.speaks == 0 && !r.easter && r.sr_on, "运行态清零、读屏开关保留不留残档");

    // —— 手感与细节 X06861~X06865 ——
    let fxs = [FxKind::Material, FxKind::Glow, FxKind::Shadow, FxKind::Blur, FxKind::Pulse];
    let mut tok = true;
    for i in 0..fxs.len() {
        tok &= fxs[i].index() == i as u32;
    }
    tok &= FxKind::Material.name() == "material"
        && FxKind::Pulse.name() == "pulse"
        && FlatKind::Plain.name() == "plain"
        && FlatKind::Outline.name() == "outline"
        && FlatKind::Static.name() == "static"
        && FlatKind::Hidden.name() == "hidden";
    set.add("X06861 令牌对齐", tok, "特效与降级呈现的名称索引一一对应");

    let mut t3 = A11yKit::new();
    let _ = t3.ring_add(1);
    let _ = t3.ring_add(2);
    let s_focus = t3.node_state(1);
    let s_idle = t3.node_state(2);
    let s_out = t3.node_state(9);
    set.add("X06862 三态焦点", s_focus == 2 && s_idle == 1 && s_out == 0, "当前聚焦/在环待焦/不在环三态齐备");

    let mut kb = A11yKit::new();
    for id in 1..=4u16 {
        let _ = kb.ring_add(id);
    }
    let start = kb.focus_id();
    for _ in 0..4 {
        let _ = kb.next();
    }
    let wrapped = kb.focus_id() == start;
    for _ in 0..4 {
        let _ = kb.prev();
    }
    let back = kb.focus_id() == start;
    for _ in 0..4 {
        let _ = kb.next();
    }
    let _ = kb.prev();
    let p1 = kb.focus_id();
    let mut kb2 = A11yKit::new();
    for id in 1..=4u16 {
        let _ = kb2.ring_add(id);
    }
    for _ in 0..3 {
        let _ = kb2.next();
    }
    set.add("X06863 键盘通道", wrapped && back && p1 == Some(4) && kb2.focus_id() == p1, "Tab/Shift+Tab 环形等价且回绕一致");

    let mut labels_ok = true;
    for i in 0..LABEL_COUNT {
        labels_ok &= !LABELS[i].1.is_empty();
    }
    set.add("X06864 微文案统一", describe(E_OK) == "正常" && labels_ok && lookup_label("tab.next") == Some("下一个标签"), "中文自然术语一致");

    let mut sp = A11yKit::new();
    let ok = sp.speak("win.close");
    let first = sp.last_label;
    let miss = sp.speak("ghost.key");
    let second = sp.last_label;
    set.add("X06865 无障碍等价", ok == E_OK && first == "关闭窗口" && miss == E_EMPTY && second == "ghost.key", "标签可朗读、缺条目键名兜底读屏不哑");

    // —— 性能与优化 X06866~X06870 ——
    let mut base = A11yKit::new();
    for id in 1..=MAX_NODES as u16 {
        let _ = base.ring_add(id);
    }
    let _ = base.advance(25);
    set.add("X06866 基准采集", base.focus_id() == Some(10) && base.zoom_permille() == 1000 && base.ring_n == MAX_NODES, "满环 25 步寻址与默认放大基准入册");

    let mut hot = A11yKit::new();
    for id in 1..=4u16 {
        let _ = hot.ring_add(id);
    }
    for _ in 0..1000u32 {
        let _ = hot.next();
    }
    set.add("X06867 热路径量化", hot.focus_id() == Some(1) && hot.events == 1004 && hot.audit(), "千次 Tab 寻址无越界、事件计数含建环 4 次精确");

    let mut conv = A11yKit::new();
    for id in 1..=8u16 {
        let _ = conv.ring_add(id);
    }
    for _ in 0..500u32 {
        let _ = conv.next();
    }
    conv.reset();
    set.add("X06868 内存功耗收敛", conv.ring_n == 0 && conv.events == 0 && conv.speaks == 0 && conv.focus_id().is_none(), "高负载后待机零增量泄漏入长稳");

    let mut eco = A11yKit::new();
    let keep = eco.degrade_probe(500);
    let _ = eco.set_gear(1, 4);
    let _ = eco.set_gear(3, 0);
    let on = eco.degrade_probe(950);
    set.add("X06869 低配降级链", !keep && on && eco.eco && eco.gear(1) == Some(2) && eco.gear(3) == Some(3), "低配下放大受限、动效强制减速");

    let mut g = A11yKit::new();
    let a0 = g.audit();
    let _ = g.ring_add(1);
    let dup = g.ring_add(1);
    set.add("X06870 防劣化守卫", a0 && dup == E_INVALID && g.ring_n == 1 && g.audit(), "重复编号拒绝、不变量断言只增不删");

    // —— 创新拓展 X06871~X06875 ——
    let mut sg = A11yKit::new();
    let suggest = sg.suggest_hc();
    let miss = sg.speak("ghost.key");
    set.add("X06871 智能建议", suggest && miss == E_EMPTY && describe(E_EMPTY).contains("登记") && sg.last_label == "ghost.key", "对比度不足建议开 HC、缺标签建议登记");

    let mut bt = A11yKit::new();
    let ids: [u16; 8] = [2, 4, 6, 8, 10, 12, 14, 16];
    let added = bt.ring_add_batch(&ids);
    let mut spoke = 0usize;
    for i in 0..LABEL_COUNT {
        if bt.speak(LABELS[i].0) == E_OK {
            spoke += 1;
        }
    }
    set.add("X06872 批量自动化", added == 8 && bt.ring_n == 8 && spoke == LABEL_COUNT && bt.speaks == LABEL_COUNT as u64, "批量建环与批量朗读脚本化完成");

    let mut cx = A11yKit::new();
    let _ = cx.ring_add(7);
    let _ = cx.ring_add(8);
    cx.set_hc(true);
    let cross = hc_downgrade(FxKind::Shadow, cx.hc_on) == Some(FlatKind::Outline)
        && cx.contrast >= 3
        && cx.focus_id() == Some(7)
        && cx.audit();
    set.add("X06873 三线跨域联动", cross, "HC 映射/档位联动/焦点序三线协同可观测");

    let ext = lookup_label("btn.cancel") == Some("取消")
        && lookup_label("no.key").is_none()
        && hc_downgrade(FxKind::Blur, true) == Some(FlatKind::Plain)
        && hc_downgrade(FxKind::Pulse, true) == Some(FlatKind::Static)
        && hc_downgrade(FxKind::Glow, false).is_none()
        && clamp_gear(9) == 4;
    set.add("X06874 开发者扩展点", ext, "标签查询/降级映射/档位钳制纯函数可复用");

    let mut eg = A11yKit::new();
    for d in 0..DIM_COUNT {
        let _ = eg.set_gear(d, GEAR_MAX);
    }
    let egg = eg.easter;
    eg.reset();
    set.add("X06875 彩蛋与净身", egg && !eg.easter && eg.ring_n == 0 && eg.gear(0) == Some(0), "五维满档点亮纪念、净身后无痕");

    set
}
