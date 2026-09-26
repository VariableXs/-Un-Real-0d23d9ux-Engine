//! F438 盘符与挂载管理 · 完整设计（STAR I 主册 G-I-38）。
//!
//! **判据（主册）**：盘符冲突检测用例；lnk 自动修复（改符前后快捷方式
//! 有效性对比）；卷标即时性；挂载目录用例；变更留痕（F372 时间线事件）。
//! ＋通12。
//!
//! 设计：盘符管理核——盘符表（卷 ↔ 字母双向）；改符前置冲突检测（目标
//! 字母已占 → 拒绝并回报占用者——确认前就被拦住）；卷标改名即时生效
//! （标签表单点改）；挂载到空目录（挂载点表，NTFS 风格进阶用法）；
//! **lnk 自动修复**——改符成功即对登记的快捷方式目标路径做前缀重写
//! （改符前后目标有效性对比 = 判据机检）；全部变更进事件账（F372 时间
//! 线事件形态：时间戳 + 类别 + 人话描述）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

/// 一条变更留痕（F372 时间线事件形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountEvent {
    pub t_ms: u64,
    /// 事件类别：letter-change / label-change / mount-dir / lnk-fix。
    pub kind: &'static str,
    /// 人话描述。
    pub detail: String,
}

/// 一个卷的登记项。
#[derive(Clone, Debug)]
pub struct Volume {
    pub volume_id: u64,
    /// 当前盘符（如 "D"）。
    pub letter: char,
    /// 卷标。
    pub label: String,
    /// 挂载到的空目录（None = 常规盘符挂载）。
    pub mount_dir: Option<String>,
}

/// 登记的快捷方式（lnk 自动修复对象）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LnkEntry {
    /// 快捷方式自身路径。
    pub lnk_path: String,
    /// 目标路径（含盘符前缀，如 "D:\\tools\\app.exe"）。
    pub target: String,
}

/// 盘符与挂载管理核。
pub struct DriveLetterMgr {
    pub volumes: Vec<Volume>,
    pub lnks: Vec<LnkEntry>,
    /// 变更留痕账（F372 时间线）。
    pub events: Vec<MountEvent>,
    pub now_ms: u64,
}

impl DriveLetterMgr {
    pub fn new() -> DriveLetterMgr {
        DriveLetterMgr { volumes: Vec::new(), lnks: Vec::new(), events: Vec::new(), now_ms: 0 }
    }

    fn log(&mut self, kind: &'static str, detail: String) {
        self.events.push(MountEvent { t_ms: self.now_ms, kind, detail });
    }

    /// 冲突检测：目标盘符是否已被占用（返回占用者卷 id）。
    pub fn letter_holder(&self, letter: char) -> Option<u64> {
        self.volumes.iter().find(|v| v.letter == letter).map(|v| v.volume_id)
    }

    /// 改盘符：先冲突检测（确认前拦截），成功后自动修复 lnk 前缀并留痕。
    /// 返回 Err(占用者卷 id) = 冲突被拦；Ok(修复的 lnk 数) = 成功。
    pub fn change_letter(&mut self, volume_id: u64, to: char) -> Result<usize, u64> {
        if let Some(holder) = self.letter_holder(to) {
            if holder != volume_id {
                return Err(holder);
            }
        }
        let vol = self
            .volumes
            .iter_mut()
            .find(|v| v.volume_id == volume_id)
            .ok_or(u64::MAX)?;
        let from = vol.letter;
        vol.letter = to;
        self.log("letter-change", format!("盘符 {}: → {}:", from, to));
        // lnk 自动修复：目标路径前缀重写（改符的连带断链在源头预防）。
        let prefix = format!("{}:", from);
        let mut fixed = 0;
        for lnk in self.lnks.iter_mut() {
            if lnk.target.starts_with(&prefix) {
                lnk.target = format!("{}{}", to, &lnk.target[1..]);
                fixed += 1;
            }
        }
        if fixed > 0 {
            self.log("lnk-fix", format!("{} 条快捷方式目标已跟随改符", fixed));
        }
        Ok(fixed)
    }

    /// 卷标改名：即时生效（单点改）+ 留痕。
    pub fn rename_label(&mut self, volume_id: u64, label: &str) -> bool {
        match self.volumes.iter_mut().find(|v| v.volume_id == volume_id) {
            Some(v) => {
                v.label = String::from(label);
                self.log("label-change", format!("卷标改为「{}」", label));
                true
            }
            None => false,
        }
    }

    /// 挂载到空目录：目录须为空串前缀合法（非根），登记挂载点。
    pub fn mount_to_dir(&mut self, volume_id: u64, dir: &str) -> bool {
        if dir.is_empty() || !dir.starts_with('\\') {
            return false; // 挂载点必须是合法空目录路径
        }
        match self.volumes.iter_mut().find(|v| v.volume_id == volume_id) {
            Some(v) => {
                v.mount_dir = Some(String::from(dir));
                self.log("mount-dir", format!("已挂载到目录 {}", dir));
                true
            }
            None => false,
        }
    }

    /// lnk 有效性对拍：改符前后目标盘符与卷当前盘符一致（判据机检）。
    pub fn lnk_targets_valid(&self) -> bool {
        self.lnks.iter().all(|lnk| {
            self.volumes
                .iter()
                .any(|v| lnk.target.starts_with(v.letter) && lnk.target.as_bytes()[1] == b':')
        })
    }
}

pub fn run_diskmnt_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F438");
    let mut m = DriveLetterMgr::new();
    m.now_ms = 1_000;
    m.volumes.push(Volume {
        volume_id: 1,
        letter: 'C',
        label: String::from("系统"),
        mount_dir: None,
    });
    m.volumes.push(Volume {
        volume_id: 2,
        letter: 'D',
        label: String::from("数据"),
        mount_dir: None,
    });
    m.lnks.push(LnkEntry {
        lnk_path: String::from("\\\\桌面\\工具.lnk"),
        target: String::from("D:\\tools\\app.exe"),
    });
    // 冲突检测：把 C 改成 D → 拦截并回报占用者（确认前被拦住）。
    set.add(
        "f438-conflict-detected",
        m.change_letter(1, 'D') == Err(2) && m.letter_holder('D') == Some(2),
        "",
    );
    // 改符成功：D → E，lnk 自动修复（前后目标有效性对比）。
    let before_valid = m.lnk_targets_valid();
    let fixed = m.change_letter(2, 'E');
    set.add(
        "f438-lnk-autofix",
        fixed == Ok(1)
            && m.lnks[0].target == "E:\\tools\\app.exe"
            && before_valid
            && m.lnk_targets_valid(),
        "",
    );
    // 卷标即时生效 + 留痕。
    set.add(
        "f438-label-instant",
        m.rename_label(2, "仓库") && m.volumes[1].label == "仓库",
        "",
    );
    // 挂载到空目录。
    set.add(
        "f438-mount-dir",
        m.mount_to_dir(2, "\\mounts\\data") && m.volumes[1].mount_dir.as_deref() == Some("\\mounts\\data"),
        "",
    );
    set.add("f438-mount-dir-invalid", !m.mount_to_dir(2, ""), "");
    // 变更留痕（F372 形态：类别 + 时间戳 + 人话）。
    set.add(
        "f438-events-ledger",
        m.events.len() >= 3
            && m.events[0].kind == "letter-change"
            && m.events.iter().any(|e| e.kind == "lnk-fix")
            && m.events.iter().all(|e| e.t_ms == 1_000 && !e.detail.is_empty()),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reletter_back_restores_links() {
        let mut m = DriveLetterMgr::new();
        m.volumes.push(Volume { volume_id: 1, letter: 'D', label: String::from("x"), mount_dir: None });
        m.lnks.push(LnkEntry {
            lnk_path: String::from("a.lnk"),
            target: String::from("D:\\a.exe"),
        });
        assert_eq!(m.change_letter(1, 'F'), Ok(1));
        assert_eq!(m.lnks[0].target, "F:\\a.exe");
        // 改回去同样修复。
        assert_eq!(m.change_letter(1, 'D'), Ok(1));
        assert_eq!(m.lnks[0].target, "D:\\a.exe");
        assert!(m.lnk_targets_valid());
    }

    #[test]
    fn same_letter_change_is_noop_ok() {
        let mut m = DriveLetterMgr::new();
        m.volumes.push(Volume { volume_id: 1, letter: 'D', label: String::from("x"), mount_dir: None });
        // 同名改符 = 无冲突、零修复。
        assert_eq!(m.change_letter(1, 'D'), Ok(0));
        assert_eq!(m.events.len(), 1, "留痕仍记一笔（变更透明）");
    }
}
