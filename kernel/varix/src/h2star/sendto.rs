//! F263 「发送到」菜单 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：四项默认清单；蓝牙离线不显示判据；防重名逻辑
//! 用例；扩展折叠行为审计。
//!
//! **设计要点（主册）**：右键「发送到」子菜单默认四项：桌面快捷方式、
//! 压缩文件夹（F092 内置压缩）、共享卷 S: 根、蓝牙发送（设备在线时才
//! 显示）——动态项缺席时菜单不出现空坑；扩展机制同 F258 纪律：vxapp
//! 可申请加入但默认折叠；发送到桌面自动防重名（「XX - 副本.lnk」）。
//!
//! 实装：默认四项清单（唯一源）；动态项可见性（蓝牙在线性注入）；
//! 防重名复用 [`h2base::bump_copy_name`]；第三方扩展折叠区（审计留痕）。

use crate::checks::CheckSet;
use crate::h2star::h2base::bump_copy_name;

use alloc::string::String;
use alloc::vec::Vec;

/// 发送到目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendTarget {
    DesktopShortcut,
    CompressFolder,
    SharedVolumeS,
    Bluetooth,
    /// 第三方扩展（折叠区）。
    Extension,
}

/// 默认四项清单（唯一源）。
pub const DEFAULT_FOUR: [SendTarget; 4] = [
    SendTarget::DesktopShortcut,
    SendTarget::CompressFolder,
    SendTarget::SharedVolumeS,
    SendTarget::Bluetooth,
];

/// 一条被折叠的扩展申请（审计账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldAudit {
    pub app: String,
    pub at_min: u64,
}

/// 发送到服务。
pub struct SendTo {
    /// 蓝牙在线态（动态注入——离线时菜单不出现空坑）。
    pub bluetooth_online: bool,
    /// 折叠区扩展 + 审计。
    extensions: Vec<(String, SendTarget)>,
    pub audits: Vec<FoldAudit>,
}

impl SendTo {
    pub fn new() -> SendTo {
        SendTo {
            bluetooth_online: false,
            extensions: Vec::new(),
            audits: Vec::new(),
        }
    }

    /// 可见菜单（动态项缺席即隐藏——空坑为零；扩展默认不出现）。
    pub fn visible(&self) -> Vec<SendTarget> {
        DEFAULT_FOUR
            .iter()
            .copied()
            .filter(|t| *t != SendTarget::Bluetooth || self.bluetooth_online)
            .collect()
    }

    /// 「更多」折叠区入口（有扩展才渲染——空区不出现）。
    pub fn more(&self) -> Vec<(String, SendTarget)> {
        self.extensions.clone()
    }

    /// vxapp 申请加入：进折叠区并留审计（主四项不动）。
    pub fn register_extension(&mut self, app: &str, at_min: u64) {
        self.extensions.push((String::from(app), SendTarget::Extension));
        self.audits.push(FoldAudit { app: String::from(app), at_min });
    }

    /// 发送到桌面：防重名（目标名与桌面现存比对，撞名递增「 - 副本」）。
    /// bump 作用于候选主名迭代推进——阶梯可数、上限 1000 步防线。
    pub fn desktop_shortcut_name(name: &str, exists: impl Fn(&str) -> bool) -> String {
        let mut stem = String::from(name);
        let mut cand = alloc::format!("{}.lnk", stem);
        let mut guard = 0u32;
        while exists(&cand) {
            stem = bump_copy_name(&stem);
            cand = alloc::format!("{}.lnk", stem);
            guard += 1;
            if guard > 1000 {
                break;
            }
        }
        cand
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_sendto_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F263");
    let mut st = SendTo::new();
    // 四项默认清单（默认蓝牙离线 → 三项可见）。
    set.add(
        "F263 four defaults",
        DEFAULT_FOUR.len() == 4 && st.visible().len() == 3,
        "offline hides bt",
    );
    // 蓝牙在线 → 四项齐全，不出现空坑。
    st.bluetooth_online = true;
    set.add(
        "F263 bt online shows",
        st.visible().len() == 4 && st.visible()[3] == SendTarget::Bluetooth,
        "dynamic slot",
    );
    // 防重名：桌面已有「方案.lnk」→ 「方案 - 副本.lnk」；再撞递增。
    let existing = ["方案.lnk", "方案 - 副本.lnk"];
    let exists = |n: &str| existing.contains(&n);
    set.add(
        "F263 dedupe",
        SendTo::desktop_shortcut_name("方案", exists) == "方案 - 副本 (2).lnk",
        "副本 ladder",
    );
    set.add(
        "F263 dedupe free slot",
        SendTo::desktop_shortcut_name("新东西", exists) == "新东西.lnk",
        "no bump when free",
    );
    // 扩展折叠 + 审计：主四项纹丝不动。
    let before = st.visible().len();
    st.register_extension("某笔记", 10);
    st.register_extension("某云盘", 11);
    set.add(
        "F263 extension folded",
        st.more().len() == 2
            && st.visible().len() == before
            && st.audits.len() == 2
            && st.audits[0].at_min == 10,
        "fold+audit",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f263_sendto_flow() {
        let set = run_sendto_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F263 自检红 {f}/{p}");
    }

    #[test]
    fn dedupe_never_loops_forever() {
        // 极端：99 层副本全占满——函数仍然返回（不挂死）。
        let mut n = String::from("A");
        let exists = |_: &str| true;
        let _ = SendTo::desktop_shortcut_name(&n, exists);
        // 用 h2base 阶梯直验 99 层收敛。
        for _ in 0..200 {
            n = bump_copy_name(&n);
        }
        assert!(n.contains("(200)"), "阶梯可数：{}", n);
    }
}
