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
//!
//! **v4 深化批次新增（AI-U1）**：
//! - 盘符域验证 [`DriveLetterMgr::letter_in_domain`]：只接受 C-Z
//!   （A/B 保留软驱历史、盘符越界直接拒——不是所有 char 都能当盘符）；
//! - 自动分配 [`DriveLetterMgr::auto_assign_letter`]：从域内找第一个
//!   空闲字母（新卷接入的默认落位——不给随机字母）；
//! - 挂载点冲突检测：两个卷挂到同一空目录 → 后者被拒（一个挂载点
//!   只能有一个卷——确认前拦截）；
//! - 卷移除 [`DriveLetterMgr::remove_volume`]：U 盘拔出语义——登记的
//!   lnk 目标瞬间断链，用 [`DriveLetterMgr::broken_lnks`] 扫描器诚实
//!   报告（不假装链接还活着）；
//! - 事件过滤 [`DriveLetterMgr::events_of_kind`]：时间线按类别检索
//!   （诊断面按「改符/卷标/挂载/修复」分面查看——F372 消费端）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

/// 盘符域下界（C: 起——A/B 保留软驱历史位）。
pub const LETTER_DOMAIN_LO: u8 = b'C';
/// 盘符域上界。
pub const LETTER_DOMAIN_HI: u8 = b'Z';

/// 一条变更留痕（F372 时间线事件形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountEvent {
    pub t_ms: u64,
    /// 事件类别：letter-change / label-change / mount-dir / lnk-fix / volume-remove。
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

/// 盘符是否在合法域内（C-Z；A/B 与非字母一律拒绝）。
pub fn letter_in_domain(c: char) -> bool {
    let b = c as u32;
    (LETTER_DOMAIN_LO as u32..=LETTER_DOMAIN_HI as u32).contains(&b)
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

    /// 自动分配：域内第一个空闲字母（新卷默认落位）。
    /// 域满（C-Z 全占）返回 None——诚实，不溢出域外。
    pub fn auto_assign_letter(&self) -> Option<char> {
        (LETTER_DOMAIN_LO..=LETTER_DOMAIN_HI)
            .map(|b| b as char)
            .find(|&c| self.letter_holder(c).is_none())
    }

    /// 登记新卷：盘符在域内且未占用才收。
    pub fn add_volume(&mut self, volume_id: u64, letter: char, label: &str) -> Result<(), &'static str> {
        if !letter_in_domain(letter) {
            return Err("盘符必须在 C-Z 域内");
        }
        if self.letter_holder(letter).is_some() {
            return Err("盘符已被占用——换一个或用自动分配");
        }
        self.volumes.push(Volume {
            volume_id,
            letter,
            label: String::from(label),
            mount_dir: None,
        });
        self.log("volume-add", format!("新卷 {} 已登记为 {}:", label, letter));
        Ok(())
    }

    /// 改盘符：先冲突检测（确认前拦截），成功后自动修复 lnk 前缀并留痕。
    /// 返回 Err(占用者卷 id) = 冲突被拦；Ok(修复的 lnk 数) = 成功。
    pub fn change_letter(&mut self, volume_id: u64, to: char) -> Result<usize, u64> {
        if !letter_in_domain(to) {
            return Err(u64::MAX);
        }
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

    /// 挂载到空目录：路径合法 + 无他卷占用该挂载点（冲突确认前拦截）。
    pub fn mount_to_dir(&mut self, volume_id: u64, dir: &str) -> bool {
        if dir.is_empty() || !dir.starts_with('\\') {
            return false; // 挂载点必须是合法空目录路径
        }
        if self
            .volumes
            .iter()
            .any(|v| v.volume_id != volume_id && v.mount_dir.as_deref() == Some(dir))
        {
            self.log("mount-dir", format!("挂载点 {} 已被其他卷占用——拒绝", dir));
            return false;
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

    /// 移除卷（U 盘拔出语义）：卷出表；指向该卷的 lnk 立即成为断链——
    /// 诚实记账（不假装链接还活着），留痕待查。
    pub fn remove_volume(&mut self, volume_id: u64) -> bool {
        let Some(pos) = self.volumes.iter().position(|v| v.volume_id == volume_id) else {
            return false;
        };
        let letter = self.volumes[pos].letter;
        self.volumes.remove(pos);
        self.log("volume-remove", format!("卷 {} 已移除——指向 {} 的快捷方式可能断链", volume_id, letter));
        true
    }

    /// 断链扫描：目标盘符已无持有者的 lnk 清单（拔盘/改符后的对账面）。
    pub fn broken_lnks(&self) -> Vec<&LnkEntry> {
        self.lnks
            .iter()
            .filter(|lnk| {
                let bytes = lnk.target.as_bytes();
                bytes.len() >= 2 && bytes[1] == b':'
                    && !self.volumes.iter().any(|v| v.letter as u8 == bytes[0])
            })
            .collect()
    }

    /// lnk 有效性对拍：改符前后目标盘符与卷当前盘符一致（判据机检）。
    pub fn lnk_targets_valid(&self) -> bool {
        self.lnks.iter().all(|lnk| {
            self.volumes
                .iter()
                .any(|v| lnk.target.starts_with(v.letter) && lnk.target.as_bytes()[1] == b':')
        })
    }

    /// 时间线按类别检索（F372 消费端——诊断面分面查看）。
    pub fn events_of_kind(&self, kind: &str) -> Vec<&MountEvent> {
        self.events.iter().filter(|e| e.kind == kind).collect()
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
    // 盘符域验证：A/B 与非字母直接拒（不是所有 char 都能当盘符）。
    set.add(
        "f438-letter-domain-guard",
        m.change_letter(2, 'A') == Err(u64::MAX)
            && m.change_letter(2, '1') == Err(u64::MAX)
            && letter_in_domain('C')
            && letter_in_domain('Z')
            && !letter_in_domain('B')
            && !letter_in_domain('a'),
        "",
    );
    // 自动分配：域内第一个空闲（当前占 C/E → 应给 D）。
    set.add("f438-auto-assign-first-free", m.auto_assign_letter() == Some('D'), "");
    // 登记新卷：占用拒绝 + 越域拒绝 + 空闲成功。
    set.add(
        "f438-add-volume-guards",
        m.add_volume(3, 'C', "重复") == Err("盘符已被占用——换一个或用自动分配")
            && m.add_volume(3, 'A', "越域") == Err("盘符必须在 C-Z 域内")
            && m.add_volume(3, 'D', "新卷").is_ok()
            && m.auto_assign_letter() == Some('F'),
        "",
    );
    // 卷标即时生效 + 留痕。
    set.add(
        "f438-label-instant",
        m.rename_label(2, "仓库") && m.volumes[1].label == "仓库",
        "",
    );
    // 挂载到空目录 + 冲突拦截（同挂载点只许一个卷）。
    set.add(
        "f438-mount-dir",
        m.mount_to_dir(2, "\\mounts\\data") && m.volumes[1].mount_dir.as_deref() == Some("\\mounts\\data"),
        "",
    );
    set.add("f438-mount-dir-invalid", !m.mount_to_dir(2, ""), "");
    set.add(
        "f438-mount-dir-conflict",
        !m.mount_to_dir(3, "\\mounts\\data")
            && m.events.iter().any(|e| e.detail.contains("已被其他卷占用")),
        "",
    );
    // 卷移除 + 断链扫描（拔盘语义：诚实报告断链，不假装链接活着）。
    m.lnks.push(LnkEntry {
        lnk_path: String::from("\\\\桌面\\资料.lnk"),
        target: String::from("D:\\docs\\a.docx"),
    });
    set.add("f438-remove-volume", m.remove_volume(3) && !m.remove_volume(99), "");
    set.add(
        "f438-broken-lnk-scan",
        m.broken_lnks().len() == 1 && m.broken_lnks()[0].target == "D:\\docs\\a.docx",
        "",
    );
    // 拔卷释放的盘符立即可复用（D 被移除 → 自动分配回到 D）。
    set.add("f438-letter-reuse-after-remove", m.auto_assign_letter() == Some('D'), "");
    // 变更留痕（F372 形态：类别 + 时间戳 + 人话）+ 按类别检索。
    set.add(
        "f438-events-ledger",
        m.events.len() >= 6
            && m.events[0].kind == "letter-change"
            && m.events.iter().any(|e| e.kind == "lnk-fix")
            && m.events.iter().all(|e| e.t_ms == 1_000 && !e.detail.is_empty()),
        "",
    );
    set.add(
        "f438-events-filter",
        m.events_of_kind("lnk-fix").len() == 1
            && m.events_of_kind("mount-dir").len() == 2
            && m.events_of_kind("volume-remove").len() == 1
            && m.events_of_kind("不存在类别").is_empty(),
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

    #[test]
    fn domain_exhaustion_is_honest() {
        let mut m = DriveLetterMgr::new();
        // 占满 C..H 六个位（小域测试——真实域 24 位同理）。
        for (i, b) in (b'C'..=b'H').enumerate() {
            assert!(m.add_volume(i as u64 + 1, b as char, "v").is_ok());
        }
        assert_eq!(m.auto_assign_letter(), Some('I'), "下一个空闲顺延");
        // 全域占满（24 个卷）→ None，不溢出域外。
        for (i, b) in (b'I'..=b'Z').enumerate() {
            assert!(m.add_volume(100 + i as u64, b as char, "v").is_ok());
        }
        assert_eq!(m.auto_assign_letter(), None, "域满诚实返回 None");
    }

    #[test]
    fn broken_scan_and_repair_cycle() {
        let mut m = DriveLetterMgr::new();
        m.volumes.push(Volume { volume_id: 1, letter: 'D', label: String::from("x"), mount_dir: None });
        m.lnks.push(LnkEntry { lnk_path: String::from("a.lnk"), target: String::from("D:\\a.exe") });
        m.lnks.push(LnkEntry { lnk_path: String::from("b.lnk"), target: String::from("Q:\\b.exe") });
        // Q 盘不存在 → 只 b.lnk 断链；D 卷还在 → a.lnk 健康。
        assert_eq!(m.broken_lnks().len(), 1);
        assert_eq!(m.broken_lnks()[0].lnk_path, "b.lnk");
        // 拔掉 D → a.lnk 也断。
        assert!(m.remove_volume(1));
        assert_eq!(m.broken_lnks().len(), 2);
    }
}
