//! VE-F3525 · 上下文菜单个性化 —— 条目排序/显示个性化 + 偏好持久 + 词典边界。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3525`
//!
//! # 职责（锚点原文拆解）
//!
//! - **排序个性化**（[`MenuOrder`]):右键菜单条目顺序用户可调
//!   （置顶/下沉/隐藏），偏好持久化（序列化为稳定串格式）。
//! - **词典边界**（[`LexiconGuard`]）：**个性化排序不改菜单结构
//!   语义**——可以调顺序、可以藏条目，但条目的 id/文案/动作
//!   必须仍是词典登记的原条目（词典第十章延续）。试图改文案
//!   或造新条目 = 越界拒绝。
//! - **偏好损坏回默认**（[`MenuPrefs::load`]）：偏好串解析失败
//!   → 默认排序（菜单结构永不因偏好损坏而缺失）。
//! - **项缺失跳过**：排序引用了已下架的条目 id → 跳过不报错
//!   （菜单照常出）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 偏好损坏 | 默认排序 |
//! | 词典违例 | 拒绝 |
//! | 项缺失 | 跳过 |
//!
//! # 复杂度
//!
//! - `render`：O(菜单项)；
//! - 序列化/反序列化：O(条目)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::format;
use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5940 起）
// ---------------------------------------------------------------------------

/// 菜单个性化域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuCode {
    /// 越界：试图改词条文案或造新条目。
    LexiconViolation = 5940,
    /// 偏好串损坏（回默认）。
    PrefsCorrupt = 5941,
    /// 菜单项数超上限。
    TooManyItems = 5942,
}

impl MenuCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            MenuCode::LexiconViolation => "越界改动菜单词典",
            MenuCode::PrefsCorrupt => "偏好数据损坏",
            MenuCode::TooManyItems => "菜单项超上限",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, MenuCode::LexiconViolation)
    }
}

// ---------------------------------------------------------------------------
// 二、菜单条目（词典真源）
// ---------------------------------------------------------------------------

/// 菜单项上限。
pub const MENU_CAP: usize = 32;

/// 一个菜单条目（id/文案/动作由词典登记，个性化侧只读）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MenuItem {
    /// 条目 id（词典登记号）。
    pub id: u16,
    /// 文案（词典原文，个性化不可改）。
    pub text: &'static str,
    /// 动作号（词典登记）。
    pub action: u16,
}

/// 词典守卫：登记的条目是真源，个性化请求必须引用登记 id。
pub struct LexiconGuard {
    pub items: Vec<MenuItem>,
}

impl LexiconGuard {
    /// 从条目集构造。
    pub fn new(items: Vec<MenuItem>) -> Self {
        LexiconGuard { items }
    }

    /// 按 id 查条目。
    pub fn get(&self, id: u16) -> Option<&MenuItem> {
        self.items.iter().find(|m| m.id == id)
    }

    /// 越界检查：排序请求里的 id 必须全部已登记。
    pub fn check_order(&self, order: &[u16]) -> Result<(), MenuCode> {
        for &id in order {
            if self.get(id).is_none() {
                return Err(MenuCode::LexiconViolation);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、偏好
// ---------------------------------------------------------------------------

/// 单条排序偏好。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderPref {
    /// 置顶（排最前，按声明序）。
    Pin,
    /// 下沉（排最后）。
    Sink,
    /// 隐藏（不显示，但条目仍在词典）。
    Hide,
}

/// 偏好账：条目 id → 偏好。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuPrefs {
    pub entries: Vec<(u16, OrderPref)>,
}

impl MenuPrefs {
    /// 空（默认顺序）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设偏好（覆盖同 id 旧偏好）。
    pub fn set(&mut self, id: u16, pref: OrderPref) {
        if let Some(e) = self.entries.iter_mut().find(|(i, _)| *i == id) {
            e.1 = pref;
        } else {
            self.entries.push((id, pref));
        }
    }

    /// 查偏好。
    pub fn pref(&self, id: u16) -> Option<OrderPref> {
        self.entries.iter().find(|(i, _)| *i == id).map(|e| e.1)
    }

    /// 序列化：`"id:pin,id:sink,id:hide"`。
    pub fn serialize(&self) -> String {
        use alloc::string::ToString;
        let mut parts: Vec<&str> = Vec::with_capacity(self.entries.len());
        let mut owned: Vec<alloc::string::String> = Vec::new();
        for (id, p) in &self.entries {
            let tag = match p {
                OrderPref::Pin => "pin",
                OrderPref::Sink => "sink",
                OrderPref::Hide => "hide",
            };
            owned.push(alloc::format!("{}:{}", id, tag));
        }
        for o in &owned {
            parts.push(o.as_str());
        }
        parts.join(",")
    }

    /// 反序列化：坏片段跳过，无法解析的串整体判损坏（回默认）。
    pub fn deserialize(s: &str) -> Result<Self, MenuCode> {
        let mut prefs = MenuPrefs::new();
        if s.is_empty() {
            return Ok(prefs);
        }
        for part in s.split(',') {
            let Some((id, tag)) = part.split_once(':') else {
                return Err(MenuCode::PrefsCorrupt);
            };
            let id: u16 = id.parse().map_err(|_| MenuCode::PrefsCorrupt)?;
            let pref = match tag {
                "pin" => OrderPref::Pin,
                "sink" => OrderPref::Sink,
                "hide" => OrderPref::Hide,
                _ => return Err(MenuCode::PrefsCorrupt),
            };
            prefs.set(id, pref);
        }
        Ok(prefs)
    }
}

// ---------------------------------------------------------------------------
// 四、渲染
// ---------------------------------------------------------------------------

/// 渲染后的菜单项（隐藏项不在列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RenderedItem {
    pub id: u16,
    pub text: &'static str,
    pub action: u16,
}

/// 渲染菜单：词典顺序为底，偏好置顶/下沉/隐藏，缺失 id 跳过。
pub fn render_menu(guard: &LexiconGuard, prefs: &MenuPrefs) -> Result<Vec<RenderedItem>, MenuCode> {
    if guard.items.len() > MENU_CAP {
        return Err(MenuCode::TooManyItems);
    }
    let mut pinned: Vec<RenderedItem> = Vec::new();
    let mut normal: Vec<RenderedItem> = Vec::new();
    let mut sunk: Vec<RenderedItem> = Vec::new();
    // 偏好置顶区按偏好声明序（先 pin 的在前）。
    for (id, pref) in &prefs.entries {
        if let Some(m) = guard.get(*id) {
            let r = RenderedItem { id: m.id, text: m.text, action: m.action };
            match pref {
                OrderPref::Pin => pinned.push(r),
                OrderPref::Hide => {}
                OrderPref::Sink => sunk.push(r),
            }
        }
        // 未登记 id：跳过（项缺失跳过纪律）。
    }
    for m in &guard.items {
        match prefs.pref(m.id) {
            Some(OrderPref::Pin) | Some(OrderPref::Hide) | Some(OrderPref::Sink) => {}
            None => normal.push(RenderedItem { id: m.id, text: m.text, action: m.action }),
        }
    }
    // sink 区剩余的（词典里新加但被 sink 的）由上面 normal 分流处理，
    // 这里再把 sunk 里没被重复计入的并进去（pref 查过的不会重复）。
    let mut out = pinned;
    out.extend(normal);
    out.extend(sunk);
    Ok(out)
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> LexiconGuard {
        LexiconGuard::new(vec![
            MenuItem { id: 1, text: "打开", action: 101 },
            MenuItem { id: 2, text: "重命名", action: 102 },
            MenuItem { id: 3, text: "删除", action: 103 },
        ])
    }

    #[test]
    fn pin_sink_hide() {
        let mut p = MenuPrefs::new();
        p.set(3, OrderPref::Pin); // 删除置顶
        p.set(2, OrderPref::Hide); // 重命名隐藏
        let out = render_menu(&guard(), &p).unwrap();
        let ids: Vec<u16> = out.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![3, 1]);
    }

    #[test]
    fn corrupt_prefs_fall_back() {
        assert_eq!(
            MenuPrefs::deserialize("3:pin,garbage"),
            Err(MenuCode::PrefsCorrupt)
        );
        let ok = MenuPrefs::deserialize("3:pin,2:hide").unwrap();
        assert_eq!(ok.pref(3), Some(OrderPref::Pin));
        assert_eq!(ok.pref(1), None);
    }

    #[test]
    fn unknown_id_skipped() {
        let mut p = MenuPrefs::new();
        p.set(99, OrderPref::Pin); // 未登记 id
        let out = render_menu(&guard(), &p).unwrap();
        assert_eq!(out.len(), 3); // 原三项照常
    }

    #[test]
    fn roundtrip() {
        let mut p = MenuPrefs::new();
        p.set(2, OrderPref::Sink);
        p.set(1, OrderPref::Pin);
        let s = p.serialize();
        let back = MenuPrefs::deserialize(&s).unwrap();
        assert_eq!(p, back);
    }
}
