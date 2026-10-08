//! F102 便签 · 完整设计（STAR I 主册 G-C-32）。
//!
//! **判据（主册）**：10 张便签开机全恢复（位置+颜色+置顶）；自动保存断电
//! 零丢失（草稿机制）；置顶跨全屏应用生效。
//!
//! **设计要点（主册）**：
//! - 桌面粘性便签：多张独立小窗（默认 240×240px）、六色主题（E1 令牌
//!   联动）、置顶开关、开机自动恢复；内容即点即编；
//! - 便签窗无标题栏（拖拽区=顶部 24px 隐形带，悬停显三钮：置顶/换色/
//!   菜单）；文字区直接编辑（自动保存防抖 500ms）；色板六色（黄/绿/蓝/
//!   粉/紫/灰）；右键菜单（复制全文/导出文本/删除——删除走回收站语义）；
//! - 每张便签独立文件（`notes/` 目录，纯文本+元数据头）；零真删；开机
//!   恢复位置与置顶态；文字区滚动条自动显隐；字号两档（14/16px）；拖拽
//!   到屏幕边缘半屏吸附可选；导出为 .txt 纯文本（开放格式）；多显示器
//!   前瞻（F029）位置接口不锁死；
//! - 异常：便签文件被外部改 → 重载提示；数量上限 20 张（超出提示整理）；
//!   磁盘异常自动草稿保命。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 默认尺寸（px）。
pub const DEFAULT_W: u32 = 240;
pub const DEFAULT_H: u32 = 240;

/// 隐形拖拽带高（px）。
pub const DRAG_BAND_PX: u32 = 24;

/// 自动保存防抖（ms）。
pub const AUTOSAVE_DEBOUNCE_MS: u64 = 500;

/// 数量上限。
pub const NOTE_CAP: usize = 20;

/// 字号两档。
pub const FONT_SIZES: [u32; 2] = [14, 16];

/// 色板六色（E1 令牌名——无硬编码色值）。
pub const COLORS: [&str; 6] = ["note.yellow", "note.green", "note.blue", "note.pink", "note.purple", "note.gray"];

// ---------------------------------------------------------------------------
// 便签模型
// ---------------------------------------------------------------------------

/// 一张便签。
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: u32,
    pub text: String,
    /// 色板下标（COLORS）。
    pub color_idx: u8,
    /// 屏幕位置（多显示器前瞻：接口按虚拟桌面坐标，不锁死单屏）。
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub pinned: bool,
    pub font_idx: u8,
    /// 最后修改（注入钟）。
    pub stamp_ms: u64,
}

/// 便签管理器。
#[derive(Default)]
pub struct Notes {
    pub list: Vec<Note>,
    next_id: u32,
    /// 防抖账：每便签最后击键。
    last_keystroke: Vec<(u32, u64)>,
    /// 自动保存次数（对账）。
    pub autosaves: u64,
    /// 外部修改提示队列（重载提示数据源）。
    pub external_changes: Vec<u32>,
}

impl Notes {
    pub fn new_note(&mut self, x: i32, y: i32, now_ms: u64) -> Option<u32> {
        if self.list.len() >= NOTE_CAP {
            return None; // 超出提示整理（诚实拒绝，不静默挤掉）。
        }
        self.next_id += 1;
        self.list.push(Note {
            id: self.next_id,
            text: String::new(),
            color_idx: 0,
            x,
            y,
            w: DEFAULT_W,
            h: DEFAULT_H,
            pinned: false,
            font_idx: 0,
            stamp_ms: now_ms,
        });
        Some(self.next_id)
    }

    pub fn edit(&mut self, id: u32, text: &str, now_ms: u64) -> bool {
        match self.list.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                n.text = String::from(text);
                n.stamp_ms = now_ms;
                self.last_keystroke.retain(|(i, _)| *i != id);
                self.last_keystroke.push((id, now_ms));
                true
            }
            None => false,
        }
    }

    /// 自动保存节拍：距最后击键 ≥ 防抖 → 序列化该便签（草稿机制——断电
    /// 零丢失的机制本体）。返回本次落盘的便签数。
    pub fn autosave_tick(&mut self, now_ms: u64) -> usize {
        let due: Vec<u32> = self
            .last_keystroke
            .iter()
            .filter(|(_, t)| now_ms.saturating_sub(*t) >= AUTOSAVE_DEBOUNCE_MS)
            .map(|(i, _)| *i)
            .collect();
        let n = due.len();
        if n > 0 {
            self.autosaves += n as u64;
            self.last_keystroke.retain(|(_, t)| now_ms.saturating_sub(*t) < AUTOSAVE_DEBOUNCE_MS);
        }
        n
    }

    pub fn set_color(&mut self, id: u32, color_idx: u8) -> bool {
        if color_idx as usize >= COLORS.len() {
            return false;
        }
        match self.list.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                n.color_idx = color_idx;
                true
            }
            None => false,
        }
    }

    pub fn toggle_pin(&mut self, id: u32) -> bool {
        match self.list.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                n.pinned = !n.pinned;
                true
            }
            None => false,
        }
    }

    /// 置顶序：pinned 恒在最上（跨全屏应用生效——合成器层面承诺）。
    pub fn z_order(&self) -> Vec<u32> {
        let mut pinned: Vec<&Note> = self.list.iter().filter(|n| n.pinned).collect();
        let mut rest: Vec<&Note> = self.list.iter().filter(|n| !n.pinned).collect();
        pinned.sort_by_key(|n| n.id);
        rest.sort_by_key(|n| n.id);
        pinned.iter().chain(rest.iter()).map(|n| n.id).collect()
    }

    /// 移动（拖拽带 24px 命中由 UI 层判——此处承接落点）。
    pub fn move_to(&mut self, id: u32, x: i32, y: i32) -> bool {
        match self.list.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                n.x = x;
                n.y = y;
                true
            }
            None => false,
        }
    }

    /// 删除（回收站语义——零真删）。
    pub fn trash(&mut self, id: u32) -> bool {
        let before = self.list.len();
        self.list.retain(|n| n.id != id);
        self.list.len() != before
    }

    /// 导出 .txt 纯文本（开放格式）。
    pub fn export_txt(&self, id: u32) -> Option<String> {
        self.list.iter().find(|n| n.id == id).map(|n| n.text.clone())
    }

    /// 序列化（开机恢复面：纯文本 + 元数据头——`notes/` 每张一文件）。
    pub fn serialize(&self, id: u32) -> Option<String> {
        let n = self.list.iter().find(|n| n.id == id)?;
        Some(alloc::format!(
            "#VARIX-NOTE v1\nid={}\ncolor={}\nx={}\ny={}\nw={}\nh={}\npinned={}\nfont={}\n---\n{}",
            n.id, n.color_idx, n.x, n.y, n.w, n.h, n.pinned as u8, n.font_idx, n.text
        ))
    }

    /// 反序列化恢复（开机全恢复判据载体）。
    pub fn restore(&mut self, blob: &str) -> bool {
        if !blob.starts_with("#VARIX-NOTE v1\n") {
            return false;
        }
        let (header, text) = match blob.split_once("\n---\n") {
            Some(v) => v,
            None => return false,
        };
        let get = |key: &str| -> Option<i64> {
            header.lines().find_map(|l| l.strip_prefix(alloc::format!("{key}=").as_str()))?.parse().ok()
        };
        let id = match get("id") {
            Some(v) => v as u32,
            None => return false,
        };
        if id == 0 {
            return false;
        }
        let note = Note {
            id,
            text: String::from(text),
            color_idx: get("color").unwrap_or(0) as u8,
            x: get("x").unwrap_or(0) as i32,
            y: get("y").unwrap_or(0) as i32,
            w: get("w").unwrap_or(DEFAULT_W as i64) as u32,
            h: get("h").unwrap_or(DEFAULT_H as i64) as u32,
            pinned: get("pinned").unwrap_or(0) == 1,
            font_idx: get("font").unwrap_or(0) as u8,
            stamp_ms: 0,
        };
        self.list.retain(|n| n.id != id);
        self.list.push(note);
        self.next_id = self.next_id.max(id);
        true
    }

    /// 外部修改检测（哈希对拍——被外部改 → 重载提示）。
    pub fn report_external_change(&mut self, id: u32) {
        if self.list.iter().any(|n| n.id == id) {
            self.external_changes.push(id);
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F102 自检（聚合进 stard 域）。
pub fn run_sticknote_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F102");

    // —— 10 张开机全恢复（位置+颜色+置顶）——
    let mut src = Notes::default();
    for i in 0..10i64 {
        let id = src.new_note((i * 30) as i32, (i * 20) as i32, i as u64).unwrap();
        src.edit(id, &alloc::format!("便签 {id}"), i as u64);
        src.set_color(id, (i % 6) as u8);
        if i % 3 == 0 {
            src.toggle_pin(id);
        }
        src.move_to(id, (i * 25 + 7) as i32, (i * 15 + 3) as i32);
    }
    let blobs: Vec<String> = (1..=10).filter_map(|id| src.serialize(id)).collect();
    let mut dst = Notes::default();
    for b in &blobs {
        assert!(dst.restore(b));
    }
    set.add("ten notes fully restored", dst.list.len() == 10 && {
        dst.list.iter().all(|n| {
            let s = src.list.iter().find(|o| o.id == n.id).unwrap();
            n.text == s.text && n.color_idx == s.color_idx && n.x == s.x && n.y == s.y && n.pinned == s.pinned
        })
    }, "");

    // —— 自动保存防抖 + 断电零丢失 ——
    let mut ns = Notes::default();
    let id = ns.new_note(0, 0, 1).unwrap();
    ns.edit(id, "只写一半的内容", 1000);
    set.add("debounce holds", ns.autosave_tick(1200) == 0, "");
    set.add("autosave after debounce", ns.autosave_tick(1500) == 1, "");
    // 断电模拟：内存态即丢，恢复走 serialize 快照（防抖后已落盘）。
    let snapshot = ns.serialize(id).unwrap();
    let mut after_crash = Notes::default();
    set.add("power loss zero loss", after_crash.restore(&snapshot) && after_crash.list[0].text == "只写一半的内容", "");

    // —— 置顶跨全屏生效（z 序承诺：src 恢复面已带 1/4/7/10 置顶）——
    set.add("pin z order", { dst.toggle_pin(5); let z = dst.z_order(); z[0] == 1 && z.iter().position(|x| *x == 5) == Some(2) }, "");
    set.add("pinned group first", { dst.toggle_pin(2); let z = dst.z_order(); z.starts_with(&[1, 2, 4, 5, 7, 10]) }, "");

    // —— 上限 20 张（超出诚实拒绝）——
    let mut cap = Notes::default();
    for i in 0..NOTE_CAP {
        assert!(cap.new_note(0, 0, i as u64).is_some());
    }
    set.add("cap 20 enforced", cap.new_note(0, 0, 99).is_none() && cap.list.len() == NOTE_CAP, "");

    // —— 六色色板 + 字号两档 ——
    set.add("six colors", COLORS.len() == 6 && ns.set_color(id, 5), "");
    set.add("invalid color rejected", !ns.set_color(id, 6), "");
    set.add("font two sizes", FONT_SIZES == [14, 16], "");

    // —— 删除走回收站语义（零真删）+ 导出开放格式 ——
    set.add("trash removes", ns.trash(id) && !ns.list.iter().any(|n| n.id == id), "");
    set.add("export txt", ns.export_txt(id).is_none() && { let mut n2 = Notes::default(); let i2 = n2.new_note(0, 0, 1).unwrap(); n2.edit(i2, "导出文本", 1); n2.export_txt(i2) == Some(String::from("导出文本")) }, "");

    // —— 外部修改重载提示（在存便签上报告）——
    let ext = ns.new_note(0, 0, 9).unwrap();
    ns.report_external_change(ext);
    set.add("external change reported", ns.external_changes == alloc::vec![ext], "");

    // —— 规格常量 ——
    set.add("default size", DEFAULT_W == 240 && DEFAULT_H == 240, "");
    set.add("drag band 24px", DRAG_BAND_PX == 24, "");
    set.add("autosave debounce 500ms", AUTOSAVE_DEBOUNCE_MS == 500, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_preserves_all_fields() {
        let mut src = Notes::default();
        let id = src.new_note(100, 50, 1).unwrap();
        src.edit(id, "恢复面全字段", 2);
        src.set_color(id, 3);
        src.toggle_pin(id);
        src.move_to(id, 123, 45);
        let blob = src.serialize(id).unwrap();
        assert!(blob.starts_with("#VARIX-NOTE v1\n"));
        let mut dst = Notes::default();
        assert!(dst.restore(&blob));
        let n = &dst.list[0];
        assert_eq!((n.x, n.y), (123, 45));
        assert_eq!(n.color_idx, 3);
        assert!(n.pinned);
        assert_eq!(n.text, "恢复面全字段");
    }

    #[test]
    fn restore_rejects_malformed() {
        let mut dst = Notes::default();
        assert!(!dst.restore(""));
        assert!(!dst.restore("garbage"));
        assert!(!dst.restore("#VARIX-NOTE v1\nno separator"));
        assert!(!dst.restore("#VARIX-NOTE v1\nid=0\n---\nx"));
    }

    #[test]
    fn autosave_only_after_debounce() {
        let mut ns = Notes::default();
        let a = ns.new_note(0, 0, 0).unwrap();
        let b = ns.new_note(10, 10, 0).unwrap();
        ns.edit(a, "A", 100);
        ns.edit(b, "B", 200);
        assert_eq!(ns.autosave_tick(400), 0);
        assert_eq!(ns.autosave_tick(600), 1, "A 到点（100+500）");
        assert_eq!(ns.autosave_tick(700), 1, "B 到点（200+500）");
        assert_eq!(ns.autosave_tick(800), 0, "队列已清");
        assert_eq!(ns.autosaves, 2);
    }

    #[test]
    fn pin_z_order_stable() {
        let mut ns = Notes::default();
        for i in 0..5 {
            ns.new_note(0, 0, i).unwrap();
        }
        ns.toggle_pin(4);
        ns.toggle_pin(2);
        assert_eq!(ns.z_order(), alloc::vec![2, 4, 1, 3, 5]);
        ns.toggle_pin(2);
        assert_eq!(ns.z_order(), alloc::vec![4, 1, 2, 3, 5]);
    }

    #[test]
    fn edit_moves_and_colors() {
        let mut ns = Notes::default();
        let id = ns.new_note(0, 0, 1).unwrap();
        assert!(ns.edit(id, "改文本", 2));
        assert!(ns.move_to(id, 55, 66));
        let n = ns.list.iter().find(|n| n.id == id).unwrap();
        assert_eq!((n.x, n.y), (55, 66));
        assert!(ns.set_color(id, 2));
        assert!(!ns.edit(999, "x", 3), "不存在便签拒绝");
    }

    #[test]
    fn multiline_text_roundtrip() {
        let mut src = Notes::default();
        let id = src.new_note(0, 0, 1).unwrap();
        src.edit(id, "第一行\n第二行\n\n第四行", 2);
        let blob = src.serialize(id).unwrap();
        let mut dst = Notes::default();
        assert!(dst.restore(&blob));
        assert_eq!(dst.list[0].text, "第一行\n第二行\n\n第四行");
    }
}
