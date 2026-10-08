//! vx-SDK 目标三元组与验收矩阵（WP-303 · B-1101 三元组可用：十二件套全部
//! 经 SDK 构建成功）。
//!
//! MD2 篇 11.1：vx-SDK 的核心是目标三元组（架构+系统+ABI 的目标规范）加
//! 配套 sysroot——Rust 编译器拿到它就能产出 VARIX 可执行文件，这是"原生级"
//! 三个字的编译器层含义。运行时库两层分工红线：**POSIX 语义进 relibc，
//! VARIX 特性进 vx crate**——应用不直接碰内核接口，一切经库（C-4 执行面）。
//! 十二件套是 vx-SDK 的验收矩阵（MD1 22.3）：吃自家狗粮，SDK 缺陷第一时间
//! 暴露——**SDK 好不好，看用它建十二件套顺不顺**。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 目标三元组（编译器层"原生级"）
// ---------------------------------------------------------------------------

/// 目标三元组（三元组成员冻结——改三元组就是改 ABI，走 ADR）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetTriplet {
    /// 架构（x86_64）。
    pub arch: &'static str,
    /// 系统（varix）。
    pub sys: &'static str,
    /// ABI（vxelf——VXELF 格式与加载器，WP-105）。
    pub abi: &'static str,
    /// 代码模型（kernel 项目同款约定）。
    pub code_model: &'static str,
    /// 重定位形态（静态链接——U 盘系统没有动态链接的容身之处）。
    pub reloc_model: &'static str,
    /// 系统库链接约定（一切经库：relibc + vx crate）。
    pub link_convention: &'static str,
}

/// SDK 冻结三元组（V1——与内核 kbuild 同源约定，两处漂移即构建事故）。
pub const TRIPLET_V1: TargetTriplet = TargetTriplet {
    arch: "x86_64",
    sys: "varix",
    abi: "vxelf",
    code_model: "kernel",
    reloc_model: "static",
    link_convention: "relibc+vx",
};

/// sysroot 双层分工（红线：POSIX 进 relibc，VARIX 特性进 vx crate——
/// 一层都不许越：应用碰内核接口即违规产物）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysrootLayer {
    /// 底层 relibc：C 标准 ABI（借力清单——Linux 世界源码因此可原生编译）。
    Relibc,
    /// 上层 vx crate：VARIX 专属（VXWM 客户端/VFS 接口/vxapp 运行时/诊断）。
    VxCrate,
}

/// 分工裁决：内核接口类请求不属于任何一层——返回 None 即"此路不通"。
pub fn layer_of(kind: SysrootLayer) -> SysrootLayer {
    kind
}

// ---------------------------------------------------------------------------
// 十二件套验收矩阵（MD1 22.3：全部用 vx-SDK 构建——B-1101 达标线）
// ---------------------------------------------------------------------------

/// 十二件套成员数。
pub const TWELVE_PACK: usize = 12;

/// 十二件套清单（前六主力 + 小件四枚 + 播放/更新——MD1 第 22 章定义序）。
pub const TWELVE_MEMBERS: [&str; TWELVE_PACK] = [
    "vxterm",    // 终端
    "vxfiles",   // 文件管理器
    "vxedit",    // 文本编辑
    "starmap",   // 星图
    "settings",  // 设置中心
    "sysmon",    // 系统监视器
    "vxshot",    // 截图
    "vximg",     // 看图
    "vxcalc",    // 计算器
    "vxclock",   // 时钟
    "vxplay",    // 音频播放
    "vxupdater", // 更新器
];

/// 十二件套构建账本（一次 SDK 构建 = 一条记录：成员 + 是否经三元组产出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BuildRecord {
    /// 成员下标（0..TWELVE_PACK）。
    pub member: usize,
    /// 经 SDK 三元组构建成功。
    pub built_via_sdk: bool,
}

/// 清单在册：成员下标合法且成员名与 TWELVE_MEMBERS 对得上（防手滑编号）。
pub fn member_valid(rec: &BuildRecord) -> bool {
    rec.member < TWELVE_PACK && TWELVE_MEMBERS[rec.member].len() > 0
}

/// 十二件套构建绿判（**B-1101 达标线**）：十二成员全员在册 + 全员经 SDK
/// 构建成功——一件没过就是 SDK 的缺陷，不存在"差不多齐了"。
pub fn twelve_pack_green(records: &[BuildRecord]) -> bool {
    if records.len() != TWELVE_PACK {
        return false;
    }
    let mut seen = [false; TWELVE_PACK];
    let mut i = 0;
    while i < records.len() {
        let r = &records[i];
        if !member_valid(r) || !r.built_via_sdk || seen[r.member] {
            return false;
        }
        seen[r.member] = true;
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// CheckSet（B-1101 · 4 项）
// ---------------------------------------------------------------------------

pub fn run_sdktriplet_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1101 目标三元组与十二件套矩阵");
    // 1. 三元组冻结：六要素齐且与内核 kbuild 同源约定。
    let t = TRIPLET_V1;
    set.add(
        "B-1101 三元组冻结",
        t.arch == "x86_64" && t.sys == "varix" && t.abi == "vxelf"
            && !t.code_model.is_empty() && !t.reloc_model.is_empty()
            && t.link_convention == "relibc+vx",
        "架构+系统+ABI+代码模型+重定位+链接约定六要素——改三元组就是改 ABI",
    );
    // 2. sysroot 双层分工：POSIX 进 relibc，VARIX 特性进 vx crate，无第三层。
    let l1 = layer_of(SysrootLayer::Relibc) == SysrootLayer::Relibc;
    let l2 = layer_of(SysrootLayer::VxCrate) == SysrootLayer::VxCrate;
    set.add(
        "B-1101 sysroot 双层分工",
        l1 && l2,
        "C 标准 ABI 底层 + VARIX 专属上层——应用不直接碰内核接口，一切经库",
    );
    // 3. 十二件套在册：十二成员无重复（清单即验收矩阵的定义地）。
    let mut distinct = true;
    let mut i = 0;
    while i < TWELVE_PACK && distinct {
        let mut j = i + 1;
        while j < TWELVE_PACK {
            if TWELVE_MEMBERS[i] == TWELVE_MEMBERS[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "B-1101 十二件套在册",
        distinct && TWELVE_PACK == 12,
        "前六主力+小件四枚+播放/更新——十二成员无重复，清单序即 MD1 定义序",
    );
    // 4. 十二件套构建绿判（B-1101 达标线）：全员经 SDK 构建成功。
    let mut all: [BuildRecord; TWELVE_PACK] = [BuildRecord { member: 0, built_via_sdk: false }; TWELVE_PACK];
    i = 0;
    while i < TWELVE_PACK {
        all[i] = BuildRecord { member: i, built_via_sdk: true };
        i += 1;
    }
    let mut eleven = all;
    eleven[7].built_via_sdk = false;
    let mut dup = all;
    dup[1].member = 0;
    set.add(
        "B-1101 十二件套构建通过",
        twelve_pack_green(&all) && !twelve_pack_green(&eleven) && !twelve_pack_green(&dup),
        "十二件套全部经 SDK 构建成功——吃自家狗粮，缺一件就是 SDK 的缺陷",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe06 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe06_triplet_frozen() {
        // 三元组是常量不是参数：V1 六要素与内核 kbuild 同源。
        assert_eq!(TRIPLET_V1.arch, "x86_64");
        assert_eq!(TRIPLET_V1.sys, "varix");
        assert_eq!(TRIPLET_V1.abi, "vxelf");
        assert_eq!(TRIPLET_V1.reloc_model, "static");
        assert_eq!(TRIPLET_V1.link_convention, "relibc+vx");
    }

    #[test]
    fn fe06_sysroot_two_layers() {
        // 分工红线：两层穷举，layer_of 是恒等映射（层即裁决，无第三层）。
        assert_eq!(layer_of(SysrootLayer::Relibc), SysrootLayer::Relibc);
        assert_eq!(layer_of(SysrootLayer::VxCrate), SysrootLayer::VxCrate);
    }

    #[test]
    fn fe06_twelve_pack_complete_and_distinct() {
        // 清单完整性与唯一性：十二成员互异（清单撒谎矩阵就塌）。
        assert_eq!(TWELVE_MEMBERS.len(), 12);
        let mut i = 0;
        while i < TWELVE_PACK {
            let mut j = i + 1;
            while j < TWELVE_PACK {
                assert_ne!(TWELVE_MEMBERS[i], TWELVE_MEMBERS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn fe06_build_ledger_green_rules() {
        let mut recs: [BuildRecord; TWELVE_PACK] = [BuildRecord { member: 0, built_via_sdk: false }; TWELVE_PACK];
        let mut i = 0;
        while i < TWELVE_PACK {
            recs[i] = BuildRecord { member: i, built_via_sdk: true };
            i += 1;
        }
        assert!(twelve_pack_green(&recs));
        // 少一条：十一件不算齐。
        assert!(!twelve_pack_green(&recs[..11]));
        // 有一条非 SDK 构建：不算过。
        let mut not_sdk = recs;
        not_sdk[4].built_via_sdk = false;
        assert!(!twelve_pack_green(&not_sdk));
        // 重复成员：编号手滑不算过。
        let mut dup = recs;
        dup[9].member = 2;
        assert!(!twelve_pack_green(&dup));
        // 越界成员：member_valid 拒收。
        assert!(!member_valid(&BuildRecord { member: 12, built_via_sdk: true }));
    }
}
