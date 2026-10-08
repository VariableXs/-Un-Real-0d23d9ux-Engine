//! AI-07 · 键位管理系统（#461~#480）。
//!
//! 主规格「第一部分：键盘快捷键」76 条默认键位 × 分类速查浮层（#461~#463）/
//! 设置页实时冲突检测（#464）/ 录制器（修饰键等待 #466、多键叠加 #467、
//! 序列键 #468、500ms 超时 #469）/ 三级冲突（完全 #470、包含 #471、修饰 #472）/
//! 定位 #473 / 交换 #474 / 覆盖 #475 / 六预设 #476 / JSON 导入导出 #477 /
//! 重置 #478 / 风格自适应 #479 / `/keys` 命令 #480。
//!
//! 零 AI：纯确定性状态机与集合运算，无网络、无随机。

/// 修饰键位标志。
pub const MOD_CTRL: u8 = 1;
/// Shift 修饰位。
pub const MOD_SHIFT: u8 = 2;
/// Alt 修饰位。
pub const MOD_ALT: u8 = 4;

/// 超时判定阈值（#469：最后键后 500ms 无新键 → 匹配或清空）。
pub const TIMEOUT_MS: u64 = 500;

/// 冲突定位高亮闪烁次数（#473）。
pub const BLINK_TIMES: u8 = 3;

/// 一个键位和弦：修饰键 + 若干非修饰键（多键叠加时 >1）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Chord {
    pub mods: u8,
    pub keys: Vec<String>,
}

fn modifier_bit(token: &str) -> Option<u8> {
    match token.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "ctl" => Some(MOD_CTRL),
        "shift" => Some(MOD_SHIFT),
        "alt" | "option" | "opt" => Some(MOD_ALT),
        _ => None,
    }
}

impl Chord {
    /// 解析 `Ctrl+Shift+Z` / `D+W+RightEnter` / `` ` `` / `滚轮`。
    pub fn parse(s: &str) -> Chord {
        let mut mods = 0u8;
        let mut keys = Vec::new();
        for tok in s.split('+') {
            let t = tok.trim();
            if t.is_empty() {
                continue;
            }
            match modifier_bit(t) {
                Some(m) => mods |= m,
                None => keys.push(t.to_string()),
            }
        }
        Chord { mods, keys }
    }

    pub fn render(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.mods & MOD_CTRL != 0 {
            parts.push("Ctrl".into());
        }
        if self.mods & MOD_SHIFT != 0 {
            parts.push("Shift".into());
        }
        if self.mods & MOD_ALT != 0 {
            parts.push("Alt".into());
        }
        parts.extend(self.keys.iter().cloned());
        parts.join("+")
    }

    /// 包含关系：修饰键相同且 keys 是本和弦 keys 的**严格前缀**
    /// （`Ctrl+A` 是 `Ctrl+A+B` 的前缀 → 包含冲突 #471）。
    pub fn is_prefix_of_keys(&self, other: &Chord) -> bool {
        self.mods == other.mods
            && self.keys.len() < other.keys.len()
            && other.keys.starts_with(&self.keys[..])
    }

    /// 修饰关系：非修饰键完全相同、修饰位为真子集
    /// （`Ctrl+Z` 与 `Ctrl+Shift+Z` → 修饰冲突 #472，仅提示）。
    pub fn is_modifier_variant_of(&self, other: &Chord) -> bool {
        self.keys == other.keys
            && !self.keys.is_empty()
            && self.mods != other.mods
            && (self.mods & other.mods) == self.mods
    }

    /// 完全相同的键组合。
    pub fn identical(&self, other: &Chord) -> bool {
        self.mods == other.mods && self.keys == other.keys
    }
}

/// 一条功能绑定。
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    /// 功能名（如「撤销」）。
    pub action: String,
    /// 速查浮层分类（基础/视图/面板/画布/节点/鼠标）。
    pub category: &'static str,
    /// 主键位序列（序列键 >1 个和弦，如 `G,G`）。
    pub seq: Vec<Chord>,
    /// 同义键（同一功能的其它按键，如漫游的 A/S/D）。
    pub alts: Vec<String>,
    /// 作用域：true=系统级/宿主转发，false=窗口内。
    pub global: bool,
    /// 画布跳转目标（#463）。
    pub pos: (f32, f32),
    /// 所属面板（#463）。
    pub panel: &'static str,
}

impl Binding {
    pub fn key_render(&self) -> String {
        let mut s: Vec<String> = self.seq.iter().map(|c| c.render()).collect();
        s.extend(self.alts.iter().cloned());
        s.join(" / ")
    }

    /// 主键位规格（供 JSON 往返与 `/keys export`）。
    pub fn key_spec(&self) -> String {
        self.seq.iter().map(|c| c.render()).collect::<Vec<_>>().join(",")
    }

    pub fn is_unbound(&self) -> bool {
        self.seq.is_empty()
    }
}

/// 键位解析：`,` 分隔序列键，`+` 分隔和弦内键。
pub fn parse_seq(spec: &str) -> Vec<Chord> {
    spec.split(',')
        .map(|part| Chord::parse(part))
        .filter(|c| !c.keys.is_empty())
        .collect()
}

/// 主规格「第一部分：键盘快捷键」76 条默认键位（分类, 功能, 键位, 同义键）。
pub const DEFAULT_KEYS: &[(&str, &str, &str, &str)] = &[
    // 一、基础操作（15）
    ("基础", "撤销", "Ctrl+Z", ""),
    ("基础", "重做", "Ctrl+Y", ""),
    ("基础", "保存", "Ctrl+S", ""),
    ("基础", "搜索", "Ctrl+F", ""),
    ("基础", "全局搜索", "Ctrl+Shift+F", ""),
    ("基础", "全选", "Ctrl+A", ""),
    ("基础", "复制", "Ctrl+C", ""),
    ("基础", "粘贴", "Ctrl+V", ""),
    ("基础", "剪切", "Ctrl+X", ""),
    ("基础", "复制节点", "Ctrl+D", ""),
    ("基础", "删除", "Delete", ""),
    ("基础", "重命名", "F2", ""),
    ("基础", "关闭/取消/回退", "Esc", ""),
    ("基础", "补全/跳转", "Tab", ""),
    ("基础", "渐进展开/暂停", "Space", ""),
    // 二、视图与可视化（14）
    ("视图", "框架视图", "F1", ""),
    ("视图", "数据流叠加", "F3", ""),
    ("视图", "热力图叠加", "F4", ""),
    ("视图", "运行/调试", "F5", ""),
    ("视图", "生命周期", "F6", ""),
    ("视图", "异常路径", "F7", ""),
    ("视图", "并发视图", "F8", ""),
    ("视图", "对比模式", "F9", ""),
    ("视图", "X光模式", "F10", ""),
    ("视图", "全屏", "F11", ""),
    ("视图", "动画播放", "F12", ""),
    ("视图", "流程图", "Ctrl+Shift+2", ""),
    ("视图", "依赖图", "Ctrl+Shift+5", ""),
    ("视图", "循环切换可视化模式", "Ctrl+Shift+V", ""),
    // 三、面板控制（13）
    ("面板", "切换侧边栏", "Ctrl+B", ""),
    ("面板", "切换终端", "Ctrl+J", ""),
    ("面板", "命令面板", "Ctrl+K", ""),
    ("面板", "命令面板(备用)", "Ctrl+Shift+P", ""),
    ("面板", "导航面板", "Ctrl+Shift+N", ""),
    ("面板", "理解面板", "Ctrl+Shift+U", ""),
    ("面板", "改进面板", "Ctrl+Shift+I", ""),
    ("面板", "创作面板", "Ctrl+Shift+C", ""),
    ("面板", "分析面板", "Ctrl+Shift+A", ""),
    ("面板", "终端(非像素风)", "`", ""),
    ("面板", "终端(像素风)", "T", ""),
    ("面板", "命令输入", "/", ""),
    ("面板", "键位速查", "?", ""),
    // 四、画布导航（14）
    ("画布", "缩放", "滚轮", ""),
    ("画布", "切换级别", "Ctrl+滚轮", ""),
    ("画布", "平移", "中键拖拽", ""),
    ("画布", "漫游", "W", "A S D"),
    ("画布", "旋转左", "Q", ""),
    ("画布", "旋转右/复位", "R", ""),
    ("画布", "鹰眼", "E", ""),
    ("画布", "X光", "X", ""),
    ("画布", "跳转左上", "Home", ""),
    ("画布", "跳转右下", "End", ""),
    ("画布", "上翻页", "PageUp", ""),
    ("画布", "下翻页", "PageDown", ""),
    ("画布", "切级别", "1", "2 3 4 5 6 7"),
    ("画布", "下钻一级", "N", ""),
    // 五、节点操作（13）
    ("节点", "回退一级", "B", ""),
    ("节点", "标记光点", "M", ""),
    ("节点", "展示标记", "Ctrl+M", ""),
    ("节点", "清除标记", "Ctrl+Shift+M", ""),
    ("节点", "锁定节点", "Ctrl+L", ""),
    ("节点", "分组", "Ctrl+G", ""),
    ("节点", "进入分析", "Ctrl+E", ""),
    ("节点", "全屏分析", "Ctrl+Shift+E", ""),
    ("节点", "包裹框", "Ctrl+Shift+W", ""),
    ("节点", "代码库识别", "Ctrl+I", ""),
    ("节点", "语义查询", "Alt+S", ""),
    ("节点", "悬停语义开关", "Ctrl+Shift+H", ""),
    ("节点", "语义搜索", "Ctrl+Shift+S", ""),
    // 六、鼠标操作（7）
    ("鼠标", "选中", "单击", ""),
    ("鼠标", "展开/折叠", "双击", ""),
    ("鼠标", "多选", "Shift+点击", ""),
    ("鼠标", "框选", "Shift+拖拽", ""),
    ("鼠标", "复制", "Alt+拖拽", ""),
    ("鼠标", "上下文菜单", "右键", ""),
    ("鼠标", "前进/后退", "鼠标侧键", ""),
];

fn panel_of(category: &str) -> &'static str {
    match category {
        "基础" => "编辑器",
        "视图" => "画布",
        "面板" => "底部终端",
        "画布" => "画布",
        "节点" => "右侧详情",
        _ => "画布",
    }
}

/// 冲突种类（#470~#472）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// 完全冲突：两功能绑定完全相同键（🔴红色边框，弹出覆盖/交换/取消）。
    Complete,
    /// 包含冲突：一个是另一个的前缀（🟡黄色边框，提示可能误触）。
    Contains,
    /// 修饰冲突：同键不同修饰（🟢绿色提示，仅提示）。
    Modifier,
}

impl ConflictKind {
    pub fn border(self) -> &'static str {
        match self {
            ConflictKind::Complete => "#FF3B30",
            ConflictKind::Contains => "#FFD60A",
            ConflictKind::Modifier => "#34C759",
        }
    }
    pub fn severity(self) -> u8 {
        match self {
            ConflictKind::Complete => 2,
            ConflictKind::Contains => 1,
            ConflictKind::Modifier => 0,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ConflictKind::Complete => "完全冲突",
            ConflictKind::Contains => "包含冲突",
            ConflictKind::Modifier => "修饰冲突",
        }
    }
}

/// 一条冲突记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conflict {
    pub a: usize,
    pub b: usize,
    pub kind: ConflictKind,
}

/// 冲突定位结果（#473）：滚动行 + 闪烁 3 次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locate {
    pub scroll_to: usize,
    pub blink: u8,
}

/// 速查浮层数据（#461）。
#[derive(Debug, Clone)]
pub struct Overlay {
    pub groups: Vec<(&'static str, Vec<usize>)>,
    pub total: usize,
}

impl Overlay {
    pub fn categories(&self) -> usize {
        self.groups.len()
    }
}

/// 记录器状态（#465~#469）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecState {
    /// 修饰键已按住，等待下一个键（#466）。
    Waiting,
    /// 已录得一个键。
    Recorded,
    /// 超时清空（#469）。
    TimedOut,
}

/// 键位录制器：点击输入框 → 按键自动录制（#465）。
#[derive(Debug, Clone)]
pub struct Recorder {
    pub seq: Vec<Chord>,
    pub pending_mods: u8,
    pub last_ms: u64,
    pub timeout_ms: u64,
    pub finished: bool,
}

impl Default for Recorder {
    fn default() -> Self {
        Recorder::new()
    }
}

impl Recorder {
    pub fn new() -> Self {
        Recorder {
            seq: Vec::new(),
            pending_mods: 0,
            last_ms: 0,
            timeout_ms: TIMEOUT_MS,
            finished: false,
        }
    }

    /// 按下一个键。修饰键只累积（按住不放等待下一个键），松开由 `release` 收尾。
    pub fn press(&mut self, token: &str, now: u64) -> RecState {
        if let Some(m) = modifier_bit(token) {
            self.pending_mods |= m;
            self.last_ms = now;
            self.finished = false;
            return RecState::Waiting;
        }
        // 同一键连续按两次 → 序列键（#468），另起和弦。
        let same_as_last = self
            .seq
            .last()
            .map(|c| c.mods == self.pending_mods && c.keys.last().map_or(false, |k| k == token))
            .unwrap_or(false);
        if same_as_last {
            self.seq.push(Chord { mods: self.pending_mods, keys: vec![token.into()] });
            self.pending_mods = 0;
            self.last_ms = now;
            self.finished = false;
            return RecState::Recorded;
        }
        // 非修饰键叠加：超时窗口内的后续非修饰键并入同一和弦（#467）。
        let mergeable = self
            .seq
            .last()
            .map(|c| c.mods == self.pending_mods && now.saturating_sub(self.last_ms) <= self.timeout_ms)
            .unwrap_or(false);
        if mergeable {
            if let Some(last) = self.seq.last_mut() {
                last.keys.push(token.into());
            }
        } else {
            self.seq.push(Chord { mods: self.pending_mods, keys: vec![token.into()] });
        }
        self.pending_mods = 0;
        self.last_ms = now;
        self.finished = false;
        RecState::Recorded
    }

    /// 修饰键松开结束（#466）。
    pub fn release(&mut self, token: &str) -> RecState {
        if let Some(m) = modifier_bit(token) {
            self.pending_mods &= !m;
            return RecState::Recorded;
        }
        RecState::Waiting
    }

    /// 超时判定（#469）：最后键后 500ms 无新键 → 匹配或清空。
    pub fn tick(&mut self, now: u64) -> RecState {
        if self.seq.is_empty() {
            return RecState::Waiting;
        }
        if now.saturating_sub(self.last_ms) > self.timeout_ms {
            self.finished = true;
            RecState::TimedOut
        } else {
            RecState::Waiting
        }
    }

    pub fn clear(&mut self) {
        self.seq.clear();
        self.pending_mods = 0;
        self.finished = false;
    }

    pub fn render(&self) -> String {
        if self.seq.is_empty() {
            return String::new();
        }
        self.seq.iter().map(|c| format!("[{}]", c.render())).collect::<Vec<_>>().join("+")
    }
}

/// 键位表（一份配置三端同一份，注册目标分级由 C16 决定）。
#[derive(Debug, Clone)]
pub struct Keymap {
    pub binds: Vec<Binding>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::default_map()
    }
}

impl Keymap {
    /// 默认键位（76 条，主规格「第一部分」）。
    pub fn default_map() -> Keymap {
        let mut binds = Vec::new();
        for (i, (cat, action, key, alts)) in DEFAULT_KEYS.iter().enumerate() {
            binds.push(Binding {
                action: (*action).into(),
                category: cat,
                seq: parse_seq(key),
                alts: alts.split_whitespace().map(|s| s.to_string()).collect(),
                global: cat == &"基础" && *key == "Ctrl+S",
                pos: ((i % 8) as f32 * 120.0, (i / 8) as f32 * 80.0),
                panel: panel_of(cat),
            });
        }
        Keymap { binds }
    }

    pub fn len(&self) -> usize {
        self.binds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.binds.is_empty()
    }

    /// #461 键位速查浮层：按分类分组列出全部键位。
    pub fn overlay(&self) -> Overlay {
        let mut groups: Vec<(&'static str, Vec<usize>)> = Vec::new();
        for cat in ["基础", "视图", "面板", "画布", "节点", "鼠标"] {
            let idx: Vec<usize> = self
                .binds
                .iter()
                .enumerate()
                .filter(|(_, b)| b.category == cat)
                .map(|(i, _)| i)
                .collect();
            if !idx.is_empty() {
                groups.push((cat, idx));
            }
        }
        Overlay { groups, total: self.binds.len() }
    }

    pub fn find(&self, action: &str) -> Option<usize> {
        self.binds.iter().position(|b| b.action == action)
    }

    /// 按（分类, 功能）定位：不同分类下允许存在同名功能（如基础/鼠标各有一个「复制」）。
    pub fn find_in(&self, category: &str, action: &str) -> Option<usize> {
        self.binds
            .iter()
            .position(|b| b.category == category && b.action == action)
    }

    /// #462 速查搜索：搜功能名（中文/英文）或键名（`Ctrl+Z`）双向匹配，实时过滤。
    pub fn search(&self, q: &str) -> Vec<usize> {
        let needle = q.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return (0..self.binds.len()).collect();
        }
        self.binds
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                b.action.to_ascii_lowercase().contains(&needle)
                    || b.key_render().to_ascii_lowercase().contains(&needle)
                    || b.seq
                        .iter()
                        .any(|c| c.keys.iter().any(|k| k.to_ascii_lowercase() == needle))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// #463 速查点击跳转：画布飞到对应功能位置 / 面板。
    pub fn jump(&self, idx: usize) -> Option<(f32, f32, &'static str)> {
        self.binds.get(idx).map(|b| (b.pos.0, b.pos.1, b.panel))
    }

    /// #464 键位设置页面：左侧分类列表 + 右侧编辑器 + 实时冲突检测。
    pub fn settings_page(&self) -> (Vec<&'static str>, usize, bool) {
        let cats: Vec<&'static str> = self.overlay().groups.iter().map(|(c, _)| *c).collect();
        (cats, self.binds.len(), !self.conflicts().is_empty())
    }

    fn chords_of(&self, i: usize) -> &[Chord] {
        &self.binds[i].seq
    }

    /// #470~#472 冲突检测（完全 → 包含 → 修饰，取最严重）。
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut out = Vec::new();
        for i in 0..self.binds.len() {
            for j in (i + 1)..self.binds.len() {
                let a = self.chords_of(i);
                let b = self.chords_of(j);
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                let identical = a.len() == b.len()
                    && a.iter().zip(b.iter()).all(|(x, y)| x.identical(y));
                if identical {
                    out.push(Conflict { a: i, b: j, kind: ConflictKind::Complete });
                    continue;
                }
                let contains = a.iter().any(|x| b.iter().any(|y| x.is_prefix_of_keys(y)))
                    || b.iter().any(|x| a.iter().any(|y| x.is_prefix_of_keys(y)));
                if contains {
                    out.push(Conflict { a: i, b: j, kind: ConflictKind::Contains });
                    continue;
                }
                let modifier = a
                    .iter()
                    .any(|x| b.iter().any(|y| x.is_modifier_variant_of(y)));
                if modifier {
                    out.push(Conflict { a: i, b: j, kind: ConflictKind::Modifier });
                }
            }
        }
        out
    }

    /// #473 冲突定位：点击 → 滚动到冲突功能行 + 高亮闪烁 3 次。
    pub fn locate(&self, c: &Conflict) -> Locate {
        Locate { scroll_to: c.a, blink: BLINK_TIMES }
    }

    /// #474 冲突交换：两个功能互换键位（含同义键）。
    pub fn swap(&mut self, a: usize, b: usize) -> bool {
        if a >= self.binds.len() || b >= self.binds.len() || a == b {
            return false;
        }
        let (sa, aa) = (self.binds[a].seq.clone(), self.binds[a].alts.clone());
        let (sb, ab) = (self.binds[b].seq.clone(), self.binds[b].alts.clone());
        self.binds[a].seq = sb;
        self.binds[a].alts = ab;
        self.binds[b].seq = sa;
        self.binds[b].alts = aa;
        true
    }

    /// #475 冲突覆盖：新覆盖旧 → 旧功能清空键位，显示「未绑定」。
    pub fn overwrite(&mut self, new_idx: usize, old_idx: usize) -> Option<String> {
        if new_idx >= self.binds.len() || old_idx >= self.binds.len() || new_idx == old_idx {
            return None;
        }
        let new_key = self.binds[new_idx].seq.clone();
        self.binds[new_idx].seq = new_key;
        self.binds[old_idx].seq.clear();
        self.binds[old_idx].alts.clear();
        Some(format!("「{}」未绑定", self.binds[old_idx].action))
    }

    /// #480 `/keys set <功能> <键>`。
    pub fn set_key(&mut self, action: &str, spec: &str) -> bool {
        match self.find(action) {
            Some(i) => {
                self.binds[i].seq = parse_seq(spec);
                !self.binds[i].seq.is_empty()
            }
            None => false,
        }
    }

    /// #480 `/keys clear <功能>`。
    pub fn clear_key(&mut self, action: &str) -> bool {
        match self.find(action) {
            Some(i) => {
                self.binds[i].seq.clear();
                self.binds[i].alts.clear();
                true
            }
            None => false,
        }
    }

    /// #478 重置：恢复默认。
    pub fn reset(&mut self) {
        *self = Keymap::default_map();
    }

    /// #479 风格自适应：像素风 `T` = 终端，其他风 `` ` `` = 终端。
    pub fn terminal_key(style: &str) -> &'static str {
        if style == "pixel" {
            "T"
        } else {
            "`"
        }
    }

    /// #479 应用风格：切换终端主键位，返回当前生效键。
    pub fn apply_style(&mut self, style: &str) -> &'static str {
        let key = Keymap::terminal_key(style);
        let target = if style == "pixel" { "终端(像素风)" } else { "终端(非像素风)" };
        let other = if style == "pixel" { "终端(非像素风)" } else { "终端(像素风)" };
        if let Some(i) = self.find(target) {
            self.binds[i].seq = parse_seq(key);
        }
        if let Some(i) = self.find(other) {
            self.binds[i].seq.clear();
        }
        key
    }

    /// #477 导出 JSON。
    pub fn to_json(&self) -> String {
        let items: Vec<String> = self
            .binds
            .iter()
            .map(|b| {
                format!(
                    "{{\"action\":\"{}\",\"cat\":\"{}\",\"key\":\"{}\",\"alts\":\"{}\"}}",
                    b.action,
                    b.category,
                    b.key_spec(),
                    b.alts.join(" ")
                )
            })
            .collect();
        format!("{{\"version\":1,\"count\":{},\"binds\":[{}]}}", self.binds.len(), items.join(","))
    }

    /// #477 导入 JSON（按分类+功能定位；未知功能忽略）。
    pub fn from_json(&mut self, json: &str) -> usize {
        let mut applied = 0;
        for part in json.split("\"action\":\"").skip(1) {
            let Some(aq) = part.find('"') else { continue };
            let action = &part[..aq];
            let rest = &part[aq..];
            let cat = rest
                .find("\"cat\":\"")
                .and_then(|p| {
                    let a = &rest[p + 7..];
                    a.find('"').map(|q| a[..q].to_string())
                })
                .unwrap_or_default();
            let Some(kp) = rest.find("\"key\":\"") else { continue };
            let kafter = &rest[kp + 7..];
            let Some(kq) = kafter.find('"') else { continue };
            let key = &kafter[..kq];
            let tail = &kafter[kq..];
            let alts = tail
                .find("\"alts\":\"")
                .and_then(|p| {
                    let a = &tail[p + 8..];
                    a.find('"').map(|q| a[..q].to_string())
                })
                .unwrap_or_default();
            let idx = if cat.is_empty() { self.find(action) } else { self.find_in(&cat, action) };
            if let Some(i) = idx {
                self.binds[i].seq = parse_seq(key);
                self.binds[i].alts = alts.split_whitespace().map(|s| s.to_string()).collect();
                applied += 1;
            }
        }
        applied
    }

    /// #476 预设方案（默认 / VS Code / Vim / Minecraft / 手柄 / 单手）。
    pub fn presets() -> [&'static str; 6] {
        ["default", "vscode", "vim", "minecraft", "gamepad", "onehand"]
    }

    pub fn preset(name: &str) -> Option<Keymap> {
        if !Keymap::presets().contains(&name) {
            return None;
        }
        let mut m = Keymap::default_map();
        match name {
            "vscode" => {
                // 命令面板主键 Ctrl+Shift+P，终端反引号，全屏 F11。
                m.set_key("命令面板", "Ctrl+Shift+P");
                m.set_key("命令面板(备用)", "Ctrl+K");
                m.set_key("终端(非像素风)", "`");
                m.apply_style("modern");
            }
            "vim" => {
                // hjkl 漫游 + Esc 回退 + 序列键 gg。
                m.set_key("漫游", "H");
                m.set_key("回退一级", "Esc");
                m.set_key("下钻一级", "L");
                if let Some(i) = m.find("渐进展开/暂停") {
                    m.binds[i].seq = parse_seq("G,G");
                }
            }
            "minecraft" => {
                m.apply_style("pixel");
                m.set_key("漫游", "W");
                m.set_key("标记光点", "Q");
            }
            "gamepad" => {
                m.set_key("选中", "PadA");
                m.set_key("展开/折叠", "PadB");
                m.set_key("关闭/取消/回退", "PadX");
                m.set_key("键位速查", "PadY");
                m.set_key("切换侧边栏", "LB");
                m.set_key("切换终端", "RB");
            }
            _ => {
                // 单手：全部主键收拢到左手区（QWE/ASD/ZXC 邻域 + Ctrl）。
                m.set_key("撤销", "Ctrl+Z");
                m.set_key("重做", "Ctrl+A");
                m.set_key("保存", "Ctrl+S");
                m.set_key("搜索", "Ctrl+F");
                m.set_key("全选", "Ctrl+Q");
                m.set_key("复制", "Ctrl+C");
                m.set_key("粘贴", "Ctrl+V");
                m.set_key("剪切", "Ctrl+X");
                m.set_key("复制节点", "Ctrl+D");
                m.set_key("删除", "Ctrl+W");
                m.set_key("重命名", "Ctrl+R");
            }
        }
        Some(m)
    }

    /// #480 `/keys` 命令全集：list/search/set/clear/reset/preset/conflicts/export/import。
    pub fn keys_command(&mut self, line: &str) -> String {
        let t = line.trim().strip_prefix("/keys").unwrap_or(line.trim()).trim();
        let mut it = t.split_whitespace();
        let sub = it.next().unwrap_or("list");
        let arg1 = it.next().unwrap_or("");
        let arg2: Vec<&str> = it.collect();
        match sub {
            "list" => format!("共 {} 个键位", self.binds.len()),
            "search" => {
                let idx = self.search(arg1);
                if idx.is_empty() {
                    "无匹配键位".into()
                } else {
                    idx.iter()
                        .map(|&i| format!("{} → {}", self.binds[i].action, self.binds[i].key_render()))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            "set" => {
                let spec = arg2.join("+");
                if self.set_key(arg1, &spec) {
                    format!("已设置 {arg1} = {spec}")
                } else {
                    format!("未找到功能 {arg1}")
                }
            }
            "clear" => {
                if self.clear_key(arg1) {
                    format!("{arg1} 未绑定")
                } else {
                    format!("未找到功能 {arg1}")
                }
            }
            "reset" => {
                self.reset();
                format!("已恢复默认（{} 个键位）", self.binds.len())
            }
            "preset" => match Keymap::preset(arg1) {
                Some(m) => {
                    *self = m;
                    format!("已应用预设 {arg1}")
                }
                None => format!("未知预设 {arg1}"),
            },
            "conflicts" => {
                let cs = self.conflicts();
                if cs.is_empty() {
                    "无冲突".into()
                } else {
                    cs.iter()
                        .map(|c| {
                            format!(
                                "{} ↔ {} : {}",
                                self.binds[c.a].action,
                                self.binds[c.b].action,
                                c.kind.label()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            "export" => self.to_json(),
            "import" => {
                let json = std::iter::once(arg1)
                    .chain(arg2.iter().copied())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("已导入 {} 项", self.from_json(&json))
            }
            _ => format!("未知子命令 {sub}"),
        }
    }
}

/// #461~#480 域自检。
pub fn run_keymap_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("keymap");

    // #461 键位速查浮层
    let m = Keymap::default_map();
    let ov = m.overlay();
    let cats = ov.categories();
    let total = ov.total;
    s.add(
        "#461 键位速查浮层",
        total >= 68 && cats == 6 && ov.groups[0].1.len() == 15,
        "分类列出全部键位（≥68）",
    );

    // #462 速查搜索：搜功能名 与 搜键名 命中同一项
    let by_name = m.search("撤销");
    let by_key = m.search("Ctrl+Z");
    s.add(
        "#462 速查搜索",
        by_name == vec![0] && by_key == vec![0] && m.search("ctrL+sHift+f").len() == 1,
        "功能名/键名双向匹配 + 大小写不敏感",
    );

    // #463 速查点击跳转
    let j = m.jump(0);
    s.add(
        "#463 速查点击跳转",
        j.map(|(x, _, panel)| x >= 0.0 && panel == "编辑器").unwrap_or(false) && m.jump(999).is_none(),
        "画布飞到功能位置/面板",
    );

    // #464 键位设置页面
    let (pcats, plen, has_conflict) = m.settings_page();
    s.add(
        "#464 键位设置页面",
        pcats.len() == 6 && plen == total && has_conflict,
        "分类列表 + 编辑器 + 实时冲突检测",
    );

    // #465 键位录制器
    let mut rec = Recorder::new();
    let st = rec.press("Z", 0);
    s.add("#465 键位录制器", st == RecState::Recorded && rec.render() == "[Z]", "点击输入框→按键自动录制");

    // #466 修饰键识别：按住 Ctrl 不放，再按 Z → 合并为一个和弦
    let mut rec2 = Recorder::new();
    let r1 = rec2.press("Ctrl", 0);
    let r2 = rec2.press("Z", 20);
    let chord_ok = rec2.seq.len() == 1 && rec2.seq[0].mods == MOD_CTRL && rec2.seq[0].keys == vec!["Z".to_string()];
    rec2.release("Ctrl");
    s.add(
        "#466 修饰键识别",
        r1 == RecState::Waiting && r2 == RecState::Recorded && chord_ok && rec2.pending_mods == 0,
        "按住等待下一个键，松开结束",
    );

    // #467 多键叠加：D+W+RightEnter 按顺序录成一个和弦
    let mut rec3 = Recorder::new();
    rec3.press("D", 0);
    rec3.press("W", 100);
    rec3.press("RightEnter", 200);
    let overlay_ok = rec3.seq.len() == 1
        && rec3.seq[0].keys == vec!["D".to_string(), "W".to_string(), "RightEnter".to_string()];
    s.add("#467 多键叠加", overlay_ok && rec3.render() == "[D+W+RightEnter]", "非修饰键组合按顺序录制");

    // #468 序列键：G,G → 两个和弦
    let mut rec4 = Recorder::new();
    rec4.press("G", 0);
    rec4.press("G", 120);
    s.add(
        "#468 序列键",
        rec4.seq.len() == 2 && rec4.render() == "[G]+[G]",
        "同一键连续按两次",
    );

    // #469 超时判定：500ms
    let mut rec5 = Recorder::new();
    rec5.press("G", 0);
    let before = rec5.tick(500);
    let after = rec5.tick(501);
    s.add(
        "#469 超时判定",
        before == RecState::Waiting && after == RecState::TimedOut && rec5.finished && rec5.timeout_ms == 500,
        "最后键后 500ms 无新键→匹配或清空",
    );

    // #470 完全冲突（🔴）
    let mut c1 = Keymap::default_map();
    c1.set_key("重命名", "Ctrl+Z");
    let cs = c1.conflicts();
    let complete = cs.iter().find(|c| c.kind == ConflictKind::Complete);
    s.add(
        "#470 完全冲突",
        complete.is_some()
            && ConflictKind::Complete.border() == "#FF3B30"
            && complete.map(|c| c.a == 0 || c.b == 0).unwrap_or(false),
        "两功能绑定完全相同键→红色边框",
    );

    // #471 包含冲突（🟡）：Ctrl+A 是 Ctrl+A+B 前缀
    let mut c2 = Keymap::default_map();
    c2.set_key("分组", "Ctrl+A+B");
    let contains = c2
        .conflicts()
        .into_iter()
        .find(|c| c.kind == ConflictKind::Contains);
    s.add(
        "#471 包含冲突",
        contains.is_some() && ConflictKind::Contains.border() == "#FFD60A",
        "前缀关系→黄色边框提示误触",
    );

    // #472 修饰冲突（🟢）：Ctrl+Z 与 Ctrl+Shift+Z
    let mut c3 = Keymap::default_map();
    c3.set_key("分组", "Ctrl+Shift+Z");
    let modifier = c3
        .conflicts()
        .into_iter()
        .find(|c| c.kind == ConflictKind::Modifier);
    s.add(
        "#472 修饰冲突",
        modifier.is_some() && ConflictKind::Modifier.border() == "#34C759" && ConflictKind::Modifier.severity() == 0,
        "同键不同修饰→绿色仅提示",
    );

    // #473 冲突定位
    let loc = c3.locate(&modifier.unwrap());
    s.add(
        "#473 冲突定位",
        loc.blink == 3 && loc.scroll_to == 0,
        "跳转冲突行 + 高亮闪烁 3 次",
    );

    // #474 冲突交换
    let mut c4 = Keymap::default_map();
    let ka = c4.binds[0].seq.clone();
    let kb = c4.binds[1].seq.clone();
    let swapped = c4.swap(0, 1);
    s.add(
        "#474 冲突交换",
        swapped && c4.binds[0].seq == kb && c4.binds[1].seq == ka,
        "两功能互换键位",
    );

    // #475 冲突覆盖
    let mut c5 = Keymap::default_map();
    let sel = c5.find("选中").unwrap();
    let msg = c5.overwrite(sel, 0).unwrap();
    let old_unbound = c5.binds[0].is_unbound();
    s.add(
        "#475 冲突覆盖",
        msg.contains("未绑定") && old_unbound && !c5.binds[sel].is_unbound(),
        "新覆盖旧→旧功能「未绑定」",
    );

    // #476 预设方案 6 种
    let vim = Keymap::preset("vim");
    let vsc = Keymap::preset("vscode");
    let mc = Keymap::preset("minecraft");
    let pad = Keymap::preset("gamepad");
    let one = Keymap::preset("onehand");
    let vim_ok = vim
        .as_ref()
        .map(|m| m.find("漫游").map(|i| m.binds[i].seq[0].keys[0] == "H").unwrap_or(false))
        .unwrap_or(false);
    let mc_ok = mc.as_ref().map(|m| m.find("终端(像素风)").map(|i| m.binds[i].seq[0].keys[0] == "T").unwrap_or(false)).unwrap_or(false);
    let pad_ok = pad.as_ref().map(|m| m.find("选中").map(|i| m.binds[i].seq[0].keys[0] == "PadA").unwrap_or(false)).unwrap_or(false);
    s.add(
        "#476 预设方案",
        Keymap::presets().len() == 6
            && vim_ok
            && vsc.is_some()
            && mc_ok
            && pad_ok
            && one.is_some()
            && Keymap::preset("nope").is_none(),
        "默认/VS Code/Vim/Minecraft/手柄/单手",
    );

    // #477 导入导出 JSON
    let mut json_map = Keymap::default_map();
    json_map.set_key("重命名", "F4");
    let json = json_map.to_json();
    let mut restored = Keymap::default_map();
    let applied = restored.from_json(&json);
    s.add(
        "#477 导入导出",
        json.starts_with("{\"version\":1") && json.contains("\"count\":76") && applied == 76
            && restored.binds[restored.find("重命名").unwrap()].key_render() == "F4",
        "JSON 往返一致",
    );

    // #478 重置
    let mut rst = Keymap::default_map();
    rst.set_key("重命名", "F9");
    rst.reset();
    s.add("#478 重置", rst.binds == Keymap::default_map().binds, "恢复默认");

    // #479 风格自适应
    let mut sty = Keymap::default_map();
    let kp = sty.apply_style("pixel");
    let pixel_bound = sty.find("终端(像素风)").map(|i| !sty.binds[i].is_unbound()).unwrap_or(false);
    let other_unbound = sty.find("终端(非像素风)").map(|i| sty.binds[i].is_unbound()).unwrap_or(false);
    let kn = sty.apply_style("modern");
    s.add(
        "#479 风格自适应",
        kp == "T" && pixel_bound && other_unbound && kn == "`" && Keymap::terminal_key("pixel") == "T",
        "像素风 T=终端，其他风反引号=终端",
    );

    // #480 键位命令
    let mut cmd = Keymap::default_map();
    let list = cmd.keys_command("/keys list");
    let search = cmd.keys_command("/keys search 撤销");
    let set = cmd.keys_command("/keys set 分组 Ctrl+Shift+G");
    let conflicts = cmd.keys_command("/keys conflicts");
    let exported = cmd.keys_command("/keys export");
    let imported = cmd.keys_command("/keys import {\"action\":\"分组\",\"key\":\"Ctrl+Shift+J\"}");
    let reset = cmd.keys_command("/keys reset");
    let preset = cmd.keys_command("/keys preset vscode");
    let clear = cmd.keys_command("/keys clear 分组");
    s.add(
        "#480 键位命令",
        list.contains("76")
            && search.contains("撤销")
            && set.contains("Ctrl+Shift+G")
            && !conflicts.is_empty()
            && exported.contains("binds")
            && imported.contains("已导入 1 项")
            && reset.contains("恢复默认")
            && preset.contains("已应用预设 vscode")
            && clear.contains("未绑定"),
        "list/search/set/clear/reset/preset/conflicts/export/import",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f461_overlay_groups() {
        let m = Keymap::default_map();
        let ov = m.overlay();
        assert_eq!(ov.categories(), 6);
        assert!(ov.total >= 68);
    }

    #[test]
    fn f466_modifier_waits() {
        let mut r = Recorder::new();
        assert_eq!(r.press("Shift", 0), RecState::Waiting);
        assert_eq!(r.press("Ctrl", 1), RecState::Waiting);
        assert_eq!(r.press("K", 2), RecState::Recorded);
        assert_eq!(r.seq[0].mods, MOD_CTRL | MOD_SHIFT);
        assert_eq!(r.seq[0].render(), "Ctrl+Shift+K");
    }

    #[test]
    fn f467_overlay_keys_merge() {
        let mut r = Recorder::new();
        r.press("D", 0);
        r.press("W", 10);
        r.press("RightEnter", 20);
        assert_eq!(r.seq.len(), 1);
    }

    #[test]
    fn f469_timeout_clears() {
        let mut r = Recorder::new();
        r.press("G", 0);
        assert_eq!(r.tick(499), RecState::Waiting);
        assert_eq!(r.tick(501), RecState::TimedOut);
        r.clear();
        assert!(r.seq.is_empty());
    }

    #[test]
    fn f475_overwrite_unbinds_old() {
        let mut m = Keymap::default_map();
        let sel = m.find("选中").unwrap();
        m.overwrite(sel, 0).unwrap();
        assert!(m.binds[0].is_unbound());
    }

    #[test]
    fn f477_json_roundtrip() {
        let m = Keymap::default_map();
        let mut r = Keymap::default_map();
        assert_eq!(r.from_json(&m.to_json()), 76);
        assert_eq!(r.binds, m.binds);
    }

    #[test]
    fn f479_style_terminal_key() {
        assert_eq!(Keymap::terminal_key("pixel"), "T");
        assert_eq!(Keymap::terminal_key("star"), "`");
    }
}
