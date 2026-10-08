//! F030 深化批次二 · MSI 特性树与目录校验面（compatstar2/deep · G-A-30）。
//!
//! 批次一深化覆盖 MSI 五表/Inno 任务类型/NSIS 段标志；本批补齐：MSI 特性树
//! 选择传播（子特性勾选 → 父特性自动安装——Feature 树语义）、安装进度加权
//! 聚合（文件数 + 字节量双权重——「如实透传」进度条的模型面）、安装目录
//! 合法性校验（DOS 保留名/非法字符/尾点尾空格）、ProductCode/UpgradeCode
//! 区分（同 UpgradeCode = 升级路径，异 = 独立产品——MSI 规范语义）。
//!
//! 零堆纪律：定长树表，无 alloc。

use crate::checks::CheckSet;

/// 特性树容量。
pub const MAX_FEATURES: usize = 16;
/// DOS 保留基名（大小写不敏感——安装目录校验）。
pub const DOS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// MSI 特性节点。
#[derive(Clone, Copy)]
pub struct FeatureNode {
    pub name: &'static str,
    /// 父特性下标（u16::MAX = 根特性）。
    pub parent: u16,
    pub selected: bool,
}

/// MSI 特性树：勾选子特性 → 祖先链全部标记（Feature 树的安装传播语义）。
pub struct FeatureTree {
    pub nodes: [Option<FeatureNode>; MAX_FEATURES],
    pub count: usize,
}

impl FeatureTree {
    pub const fn new() -> Self {
        FeatureTree { nodes: [None; MAX_FEATURES], count: 0 }
    }
    pub fn add(&mut self, name: &'static str, parent: u16) -> Option<usize> {
        if self.count >= MAX_FEATURES {
            return None;
        }
        // 父必须已存在（或为根标记 u16::MAX）。
        if parent != u16::MAX && (parent as usize) >= self.count {
            return None;
        }
        self.nodes[self.count] = Some(FeatureNode { name, parent, selected: false });
        self.count += 1;
        Some(self.count - 1)
    }
    /// 勾选：自身 + 全部祖先置 selected（返回受影响节点数）。
    pub fn select_propagate(&mut self, idx: usize) -> usize {
        if idx >= self.count {
            return 0;
        }
        let mut touched = 0usize;
        let mut cur = idx as u16;
        while cur != u16::MAX {
            let node = &mut self.nodes[cur as usize];
            if let Some(n) = node {
                if !n.selected {
                    n.selected = true;
                    touched += 1;
                }
            }
            cur = self.nodes[cur as usize].unwrap().parent;
        }
        touched
    }
}

/// 安装进度加权聚合：文件完成度 40% + 字节完成度 60%（如实透传的
/// 聚合口径；permille）。
pub fn progress_permille(files_done: usize, files_total: usize, bytes_done: u64, bytes_total: u64) -> u32 {
    let fp = if files_total == 0 { 1000u64 } else { files_done as u64 * 1000 / files_total as u64 };
    let bp = if bytes_total == 0 { 1000u64 } else { bytes_done * 1000 / bytes_total };
    ((fp * 4 + bp * 6) / 10) as u32
}

/// 安装目录合法性校验：DOS 保留基名/非法字符/尾点尾空格全拒。
pub fn validate_install_dir(dir: &str) -> Result<(), &'static str> {
    if dir.is_empty() {
        return Err("empty-path");
    }
    for seg in dir.split(['\\', '/']) {
        if seg.is_empty() {
            continue; // 盘符根或分隔连写
        }
        if seg.ends_with('.') || seg.ends_with(' ') {
            return Err("trailing-dot-or-space");
        }
        if seg.contains(['<', '>', '"', '|', '?', '*']) {
            return Err("illegal-char");
        }
        // 冒号仅允许盘符位（第 2 字符）。
        if seg.contains(':') && !(seg.len() == 2 && seg.as_bytes()[1] == b':') {
            return Err("misplaced-colon");
        }
        let base = match seg.split_once('.') {
            Some((b, _)) => b,
            None => seg,
        };
        let upper = base.as_bytes();
        for r in DOS_RESERVED.iter() {
            if upper.len() == r.len() && upper.iter().zip(r.bytes()).all(|(a, b)| a.eq_ignore_ascii_case(&b)) {
                return Err("reserved-dos-name");
            }
        }
    }
    Ok(())
}

/// 升级判定：同 UpgradeCode → 升级路径（ProductCode 必须更新）；
/// 异 UpgradeCode → 独立产品共存。
pub fn upgrade_applies(old_upgrade: &'static str, new_upgrade: &'static str, new_product: &'static str, installed_product: &'static str) -> Result<bool, &'static str> {
    if old_upgrade != new_upgrade {
        return Ok(false); // 独立产品
    }
    if new_product == installed_product {
        return Err("same-product-code"); // 升级必须换 ProductCode
    }
    Ok(true)
}

/// 域自检（深化批次二）。
pub fn run_f030d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F030-installr-d2");
    // 1) 特性树传播：勾选孙 → 子与父全部点亮（3 节点受影响）。
    let mut t = FeatureTree::new();
    let core = t.add("Core", u16::MAX).unwrap();
    let sdk = t.add("SDK", core as u16).unwrap();
    let docs = t.add("Docs", sdk as u16).unwrap();
    let touched = t.select_propagate(docs);
    cs.add(
        "feature_propagate",
        touched == 3
            && t.nodes[core].unwrap().selected
            && t.nodes[sdk].unwrap().selected
            && t.nodes[docs].unwrap().selected,
        "",
    );
    // 2) 孤儿特性拒绝：父不存在 → add 失败。
    let mut t2 = FeatureTree::new();
    cs.add("orphan_feature_rejected", t2.add("Ghost", 5).is_none(), "");
    // 3) 进度加权：文件全完 + 字节半程 → 400 + 300 = 700‰。
    cs.add("progress_weighted", progress_permille(10, 10, 500, 1000) == 700, "");
    // 4) 目录校验：保留名/非法字符/尾点/冒号错位全命中。
    cs.add(
        "install_dir_validation",
        validate_install_dir("C:\\Program Files\\app") == Ok(())
            && validate_install_dir("C:\\CON\\x") == Err("reserved-dos-name")
            && validate_install_dir("C:\\a<b\\x") == Err("illegal-char")
            && validate_install_dir("C:\\app.\\x") == Err("trailing-dot-or-space")
            && validate_install_dir("C:x\\y") == Err("misplaced-colon"),
        "",
    );
    // 5) 升级码语义：同 UpgradeCode + 新 ProductCode = 升级；同产品码拒绝。
    cs.add(
        "upgrade_code_semantics",
        upgrade_applies("UP-1", "UP-1", "PD-2", "PD-1") == Ok(true)
            && upgrade_applies("UP-1", "UP-2", "PD-9", "PD-1") == Ok(false)
            && upgrade_applies("UP-1", "UP-1", "PD-1", "PD-1") == Err("same-product-code"),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_names_full_roster() {
        // 定长拼接（no_std 测试档零堆纪律：不用 String/Vec）。
        for r in DOS_RESERVED.iter() {
            let mut buf = [0u8; 16];
            buf[0] = b'C';
            buf[1] = b':';
            buf[2] = b'\\';
            buf[3..3 + r.len()].copy_from_slice(r.as_bytes());
            let dir = core::str::from_utf8(&buf[..3 + r.len()]).unwrap();
            assert_eq!(validate_install_dir(dir), Err("reserved-dos-name"), "{} 应拒绝", r);
        }
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f030d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
