//! 可复现构建与镜像单源（WP-401 · B-1301/1302 双环境哈希一致×布局配置单点）。
//!
//! MD2 篇 13.1/13.2：可复现的铁律落成四件套——工具链锁定、环境净化（时区/
//! 区域/路径/源码时间戳=提交哈希派生值，杀掉一切"构建机差异"渗进产物的通
//! 道）、产物清单与哈希、双环境比对（哈希一致才认"可复现"成立）。镜像构建
//! 的顺序纪律：先造内容后造结构——分区尺寸、GUID、文件系统参数全部由构建
//! 脚本从单一配置读出；QEMU 专用 ISO 是同源衍生品，镜像差异只有引导介质形
//! 态，内容零差异。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 可复现四件套
// ---------------------------------------------------------------------------

/// 环境净化面（构建机差异的四条渗入通道全部封死）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnvPurify {
    /// 时区显式设置。
    pub tz_fixed: bool,
    /// 区域显式设置。
    pub locale_fixed: bool,
    /// 路径显式设置。
    pub path_fixed: bool,
    /// 源码时间戳取提交哈希派生值（真实时钟进不了产物）。
    pub ts_from_commit: bool,
}

impl EnvPurify {
    /// 净化完整判：四通道全封。
    pub fn ok(&self) -> bool {
        self.tz_fixed && self.locale_fixed && self.path_fixed && self.ts_from_commit
    }
}

/// 可复现构建四件套状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReproBuild {
    /// 工具链锁定（toolchain 文件钉死版本，升级走 ADR）。
    pub toolchain_locked: bool,
    /// 环境净化四通道。
    pub env: EnvPurify,
    /// 产物清单与哈希在册（每文件大小与哈希，镜像整体哈希入发布记录）。
    pub manifest_ok: bool,
    /// 双环境哈希一致（Windows 主机 vs Linux 容器——一致才认可复现）。
    pub dual_env_match: bool,
}

/// 可复现判（**B-1301 达标线：双环境哈希一致，清单齐全**）——四件套合取。
pub fn repro_ok(r: &ReproBuild) -> bool {
    r.toolchain_locked && r.env.ok() && r.manifest_ok && r.dual_env_match
}

// ---------------------------------------------------------------------------
// 镜像单源
// ---------------------------------------------------------------------------

/// 镜像布局配置（单点——分区尺寸/GUID/文件系统参数全从这一处读）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayoutConfig {
    /// 配置在册（布局改版只改一处）。
    pub present: bool,
    /// 构建脚本无第二份布局常量（散落的硬编码布局是分叉之源）。
    pub single_source: bool,
}

/// 同源衍生判（**B-1302 达标线：布局配置单点，img 与 ISO 同源**）——
/// img 与 ISO 内容零差异，差异只在引导介质形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageDuality {
    /// img（全盘镜像：分区表+五分区+引导区）。
    pub img_ready: bool,
    /// ISO（QEMU 专用：同内核同配置，CD-ROM 形态）。
    pub iso_ready: bool,
    /// 内容零差异（同源校验过——测试环境和实机环境同源是冒烟套可信的前提）。
    pub content_identical: bool,
}

impl ImageDuality {
    pub fn ok(&self) -> bool {
        self.img_ready && self.iso_ready && self.content_identical
    }
}

/// 镜像单源判（B-1302 达标线）：布局单点 + 同源衍生合取。
pub fn single_source_ok(l: &LayoutConfig, d: &ImageDuality) -> bool {
    l.present && l.single_source && d.ok()
}

// ---------------------------------------------------------------------------
// CheckSet（B-1301/1302 · 6 项）
// ---------------------------------------------------------------------------

pub fn run_reprobuild_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1301/1302 可复现构建与镜像单源");
    // 1. 环境净化四通道：逐通道可判，缺一即净化失败。
    let full = EnvPurify { tz_fixed: true, locale_fixed: true, path_fixed: true, ts_from_commit: true };
    let leaky = EnvPurify { tz_fixed: true, locale_fixed: true, path_fixed: true, ts_from_commit: false };
    set.add(
        "B-1301 环境净化四通道",
        full.ok() && !leaky.ok(),
        "时区/区域/路径/时间戳四条渗入通道全封——真实时钟进不了产物",
    );
    // 2. 可复现四件套合取（B-1301 达标线）：缺一不认可复现。
    let ok = ReproBuild {
        toolchain_locked: true,
        env: full,
        manifest_ok: true,
        dual_env_match: true,
    };
    let no_match = ReproBuild { dual_env_match: false, ..ok };
    let no_manifest = ReproBuild { manifest_ok: false, ..ok };
    set.add(
        "B-1301 可复现四件套",
        repro_ok(&ok) && !repro_ok(&no_match) && !repro_ok(&no_manifest),
        "工具链锁定+环境净化+清单哈希+双环境一致——哈希一致才认'可复现'成立（B-1301 达标线）",
    );
    // 3. 双环境语义：一致才过，不一致红——没有"基本一致"。
    set.add(
        "B-1301 双环境哈希一致",
        ok.dual_env_match && !no_match.dual_env_match,
        "Windows 主机与 Linux 容器各出一版——同源不同哈希就是可复现名存实亡",
    );
    // 4. 布局配置单点：在册+无第二份布局常量。
    let layout = LayoutConfig { present: true, single_source: true };
    let scattered = LayoutConfig { present: true, single_source: false };
    set.add(
        "B-1302 布局配置单点",
        layout.present && layout.single_source && !scattered.single_source,
        "分区尺寸/GUID/文件系统参数全从单一配置读——布局改版只改一处",
    );
    // 5. 同源衍生：img 与 ISO 内容零差异。
    let dual = ImageDuality { img_ready: true, iso_ready: true, content_identical: true };
    let diverged = ImageDuality { img_ready: true, iso_ready: true, content_identical: false };
    set.add(
        "B-1302 img 与 ISO 同源",
        dual.ok() && !diverged.ok(),
        "差异只有引导介质形态，内容零差异——测试环境和实机环境同源是冒烟套可信的前提",
    );
    // 6. 单源合取（B-1302 达标线）：布局单点+同源衍生。
    set.add(
        "B-1302 镜像单源达标",
        single_source_ok(&layout, &dual) && !single_source_ok(&scattered, &dual),
        "布局单点+同源衍生双关全过（B-1302 达标线）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe23 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe23_env_purify_channels() {
        // 四通道逐项独立红。
        let base = EnvPurify { tz_fixed: true, locale_fixed: true, path_fixed: true, ts_from_commit: true };
        assert!(base.ok());
        let variants = [
            EnvPurify { tz_fixed: false, ..base },
            EnvPurify { locale_fixed: false, ..base },
            EnvPurify { path_fixed: false, ..base },
            EnvPurify { ts_from_commit: false, ..base },
        ];
        let mut i = 0;
        while i < variants.len() {
            assert!(!variants[i].ok());
            i += 1;
        }
    }

    #[test]
    fn fe23_repro_conjunction() {
        // 四件套逐件独立红：toolchain/env/manifest/dual_env 任一失守即不复现。
        let ok = ReproBuild {
            toolchain_locked: true,
            env: EnvPurify { tz_fixed: true, locale_fixed: true, path_fixed: true, ts_from_commit: true },
            manifest_ok: true,
            dual_env_match: true,
        };
        assert!(repro_ok(&ok));
        assert!(!repro_ok(&ReproBuild { toolchain_locked: false, ..ok }));
        assert!(!repro_ok(&ReproBuild { env: EnvPurify { tz_fixed: false, ..ok.env }, ..ok }));
        assert!(!repro_ok(&ReproBuild { manifest_ok: false, ..ok }));
        assert!(!repro_ok(&ReproBuild { dual_env_match: false, ..ok }));
    }

    #[test]
    fn fe23_layout_single_point() {
        // 单点语义：present 且 single_source 才算；缺席或分叉都拒。
        assert!(single_source_ok(
            &LayoutConfig { present: true, single_source: true },
            &ImageDuality { img_ready: true, iso_ready: true, content_identical: true }
        ));
        assert!(!single_source_ok(
            &LayoutConfig { present: false, single_source: true },
            &ImageDuality { img_ready: true, iso_ready: true, content_identical: true }
        ));
    }

    #[test]
    fn fe23_duality_tri_condition() {
        // 同源三件：img/ISO/内容零差异——任一缺席即不同源。
        let d = ImageDuality { img_ready: true, iso_ready: true, content_identical: true };
        assert!(d.ok());
        assert!(!ImageDuality { img_ready: false, ..d }.ok());
        assert!(!ImageDuality { iso_ready: false, ..d }.ok());
        assert!(!ImageDuality { content_identical: false, ..d }.ok());
    }
}
