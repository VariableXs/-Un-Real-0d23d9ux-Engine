//! 转译治理边界表（WP-301 · B-401 改名直通类全覆盖 + B-404 差异表诚实性）。
//!
//! MD2 篇 4.2：Linux 用户态约四百个系统调用入口，按"依赖频率乘实现难度"
//! 分四类治理——改名直通/语义适配/柜台自实现/明确拒绝。**治理表先冻结
//! 再写代码——治理表是这个包的宪法**（MD3 行 108），写代码迁就边界的
//! 行为一律返工。拒绝不是失败——每一项拒绝都在差异表登记，应用撞墙时
//! 得到指名的错误与文档指引（Q23），这是"诚实的边界"的柜台层兑现。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 治理边界表（先冻结）
// ---------------------------------------------------------------------------

/// 治理表行数（四类家族代表——表即治理面，枚举穷举不设通配）。
pub const GOV_ROWS: usize = 32;

/// Linux errno：ENOSYS（拒绝类的指名错误码，MD2 篇 4.2）。
pub const ENOSYS: i32 = 38;

/// 系统调用治理面（篇 4.2 四类家族代表，表序即冻结序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysCall {
    // 改名直通族（篇 4.2 第一类：VARIX 原生语义本就对齐，只做参数格式转换）
    Open,
    Read,
    Write,
    Close,
    Lseek,
    Stat,
    Getpid,
    Exit,
    Wait,
    Mmap,
    Munmap,
    Brk,
    Mprotect,
    Gettimeofday,
    ClockGettime,
    Getenv,
    // 语义适配族（第二类：信号映射/克隆标志子集/poll 系/uname/sysinfo）
    Sigaction,
    Clone,
    Poll,
    Select,
    EpollCtl,
    Uname,
    Sysinfo,
    // 柜台自实现族（第三类：futex/memfd/伪文件/熵源）
    Futex,
    Memfd,
    ProcFile,
    GetRandom,
    // 明确拒绝族（第四类：io_uring/cgroup/namespace/模块加载/bpf）
    IoUring,
    CgroupCtl,
    Namespace,
    InitModule,
    Bpf,
}

/// 四类治理（篇 4.2 分类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysClass {
    /// 改名直通：参数格式转换 + errno 原值直通（C-4）。
    DirectPass,
    /// 语义适配：语义子集支持，简化项如实返回对应错误码。
    Adapt,
    /// 柜台自实现：Linux 特有但用户态依赖重。
    SelfImpl,
    /// 明确拒绝：返回 ENOSYS 并在差异表登记。
    Deny,
}

/// 治理边界表（冻结面：每行恰一个枚举成员——先冻结再写代码）。
pub const GOV_TABLE: [(SysCall, SysClass); GOV_ROWS] = [
    (SysCall::Open, SysClass::DirectPass),
    (SysCall::Read, SysClass::DirectPass),
    (SysCall::Write, SysClass::DirectPass),
    (SysCall::Close, SysClass::DirectPass),
    (SysCall::Lseek, SysClass::DirectPass),
    (SysCall::Stat, SysClass::DirectPass),
    (SysCall::Getpid, SysClass::DirectPass),
    (SysCall::Exit, SysClass::DirectPass),
    (SysCall::Wait, SysClass::DirectPass),
    (SysCall::Mmap, SysClass::DirectPass),
    (SysCall::Munmap, SysClass::DirectPass),
    (SysCall::Brk, SysClass::DirectPass),
    (SysCall::Mprotect, SysClass::DirectPass),
    (SysCall::Gettimeofday, SysClass::DirectPass),
    (SysCall::ClockGettime, SysClass::DirectPass),
    (SysCall::Getenv, SysClass::DirectPass),
    (SysCall::Sigaction, SysClass::Adapt),
    (SysCall::Clone, SysClass::Adapt),
    (SysCall::Poll, SysClass::Adapt),
    (SysCall::Select, SysClass::Adapt),
    (SysCall::EpollCtl, SysClass::Adapt),
    (SysCall::Uname, SysClass::Adapt),
    (SysCall::Sysinfo, SysClass::Adapt),
    (SysCall::Futex, SysClass::SelfImpl),
    (SysCall::Memfd, SysClass::SelfImpl),
    (SysCall::ProcFile, SysClass::SelfImpl),
    (SysCall::GetRandom, SysClass::SelfImpl),
    (SysCall::IoUring, SysClass::Deny),
    (SysCall::CgroupCtl, SysClass::Deny),
    (SysCall::Namespace, SysClass::Deny),
    (SysCall::InitModule, SysClass::Deny),
    (SysCall::Bpf, SysClass::Deny),
];

/// 治理表冻结自检：32 行无重复无遗漏、四类各有人口。
pub fn table_frozen_ok() -> bool {
    if GOV_TABLE.len() != GOV_ROWS {
        return false;
    }
    let (mut direct, mut adapt, mut self_impl, mut deny) = (0usize, 0usize, 0usize, 0usize);
    let mut i = 0;
    while i < GOV_ROWS {
        let mut j = i + 1;
        while j < GOV_ROWS {
            if GOV_TABLE[i].0 == GOV_TABLE[j].0 {
                return false; // 同一调用出现两行——冻结面被破坏
            }
            j += 1;
        }
        match GOV_TABLE[i].1 {
            SysClass::DirectPass => direct += 1,
            SysClass::Adapt => adapt += 1,
            SysClass::SelfImpl => self_impl += 1,
            SysClass::Deny => deny += 1,
        }
        i += 1;
    }
    direct > 0 && adapt > 0 && self_impl > 0 && deny > 0
}

/// 柜台唯一分派入口：查治理表定类（表是唯一权威——呈现面无权改写）。
pub fn classify(s: SysCall) -> SysClass {
    let mut i = 0;
    while i < GOV_ROWS {
        if GOV_TABLE[i].0 == s {
            return GOV_TABLE[i].1;
        }
        i += 1;
    }
    SysClass::Deny // 不可达（表穷举）——防御性默认拒
}

/// 未登记调用号默认拒（治理表外一律拒绝并留痕——不许静默成功）。
pub fn classify_num(num: u32) -> SysClass {
    // 宿主模型面：登记号 0..GOV_ROWS 映射到治理表行；表外号码默认拒。
    // 真实号表随 syscall ABI 面（VARIABLE-200 规范）接线，同源策略不变。
    if (num as usize) < GOV_ROWS {
        GOV_TABLE[num as usize].1
    } else {
        SysClass::Deny
    }
}

/// errno 原值直通（C-4）：柜台对直通族不改写错误码——应用按 Linux 语义
/// 分支的行为保持一致。
pub fn direct_errno_passthrough(internal: i32) -> i32 {
    internal
}

// ---------------------------------------------------------------------------
// 差异表（拒绝 100% 登记 + 撞墙报错指名，B-404）
// ---------------------------------------------------------------------------

/// 简化项（语义适配族的如实降级：子集外返回对应错误码，不留模糊）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimplifiedItem {
    /// clone 仅支持新线程/新进程两种形态，其余标志位返回不支持。
    CloneFlagsSubset,
    /// epoll 支持水平触发与边缘触发，简化项如实返回对应错误码。
    EpollSimplified,
}

/// 差异表行：拒绝族 5 项 + 简化项 2 项 = 7 行（拒绝 100% 登记）。
pub const DIVERGENCE_ROWS: usize = 7;

/// 差异表（冻结面：每行指名错误码 + 人话文档指引，Q23）。
pub const DIVERGENCES: [(SysCall, i32, &str); DIVERGENCE_ROWS] = [
    (SysCall::IoUring, ENOSYS, "io_uring 全族不支持：异步 IO 语义与 U 盘整机 IO 模型不合，应用应有退化路径（Q23 详见差异手册 D-01）"),
    (SysCall::CgroupCtl, ENOSYS, "cgroup 管控族不支持：单机整机的资源治理走配额服务（Q23 详见差异手册 D-02）"),
    (SysCall::Namespace, ENOSYS, "namespace 族不支持：容器隔离语义随 STAR II 预研（Q23 详见差异手册 D-03）"),
    (SysCall::InitModule, ENOSYS, "模块加载族不支持：内核模块清单封闭，无运行期加载（Q23 详见差异手册 D-04）"),
    (SysCall::Bpf, ENOSYS, "bpf 族不支持：观测面走诊断三件套（Q23 详见差异手册 D-05）"),
    (SysCall::Clone, ENOSYS, "clone 仅支持新线程/新进程两种标志形态，其余标志位返回不支持（Q23 详见差异手册 D-06）"),
    (SysCall::EpollCtl, ENOSYS, "epoll 语义子集：简化项在触发行为上有边界，子集外如实返回对应错误码（Q23 详见差异手册 D-07）"),
];

/// 撞墙查询：应用撞到差异面时得到指名错误与人话指引（B-404 达标线）。
pub fn divergence_for(s: SysCall) -> Option<&'static str> {
    let mut i = 0;
    while i < DIVERGENCE_ROWS {
        if DIVERGENCES[i].0 == s {
            return Some(DIVERGENCES[i].2);
        }
        i += 1;
    }
    None
}

/// 拒绝族指名错误码：全部 ENOSYS（篇 4.2 明示）。
pub fn deny_errno(s: SysCall) -> i32 {
    if classify(s) == SysClass::Deny {
        ENOSYS
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-401/404 · 8 项）
// ---------------------------------------------------------------------------

pub fn run_lxgov_checks() -> CheckSet {
    let mut set = CheckSet::new("B-401/404 转译治理边界");
    // 1. 治理表冻结：穷举无重复、四类各有人口。
    set.add(
        "B-401 四类分类齐",
        table_frozen_ok(),
        "治理表先冻结再写代码——直通/适配/自实现/拒绝四类全有人口",
    );
    // 2. 直通族全覆盖：16 个直通调用全部判 DirectPass。
    let direct_all = [
        SysCall::Open, SysCall::Read, SysCall::Write, SysCall::Close,
        SysCall::Lseek, SysCall::Stat, SysCall::Getpid, SysCall::Exit,
        SysCall::Wait, SysCall::Mmap, SysCall::Munmap, SysCall::Brk,
        SysCall::Mprotect, SysCall::Gettimeofday, SysCall::ClockGettime, SysCall::Getenv,
    ];
    let mut i2 = 0;
    let mut all_direct = true;
    while i2 < direct_all.len() {
        if classify(direct_all[i2]) != SysClass::DirectPass {
            all_direct = false;
        }
        i2 += 1;
    }
    set.add(
        "B-401 直通族全覆盖",
        all_direct && direct_all.len() == 16,
        "文件读写/进程基础/内存/时间环境四组直通调用全过（篇 4.2 第一类）",
    );
    // 3. errno 原值直通（C-4）：柜台不改写直通族错误码。
    let passthrough_ok = direct_errno_passthrough(2) == 2
        && direct_errno_passthrough(13) == 13
        && direct_errno_passthrough(0) == 0;
    set.add(
        "B-401 直通 errno 原值",
        passthrough_ok,
        "参数格式转换后 errno 原值直通——应用按 Linux 语义分支行为一致",
    );
    // 4. 未登记默认拒：治理表外号码拒绝不静默。
    set.add(
        "B-401 未登记默认拒",
        classify_num(9999) == SysClass::Deny && classify_num(GOV_ROWS as u32) == SysClass::Deny,
        "治理表外一律拒绝并留痕——不许静默成功（诚实边界）",
    );
    // 5. 拒绝族全登记：5 个拒绝调用差异表全部有行。
    let deny_all = [SysCall::IoUring, SysCall::CgroupCtl, SysCall::Namespace, SysCall::InitModule, SysCall::Bpf];
    let mut i5 = 0;
    let mut all_registered = true;
    while i5 < deny_all.len() {
        if divergence_for(deny_all[i5]).is_none() {
            all_registered = false;
        }
        i5 += 1;
    }
    set.add(
        "B-404 拒绝族全登记",
        all_registered,
        "拒绝项 100% 登记——拒绝不是失败是登记在册的诚实边界",
    );
    // 6. 报错指名：ENOSYS + 人话指引（Q23）。
    set.add(
        "B-404 报错指名",
        deny_errno(SysCall::IoUring) == ENOSYS && !DIVERGENCES[0].2.is_empty(),
        "撞墙得到指名错误与文档指引——报错是功能不是失败",
    );
    // 7. 简化项如实：语义适配的降级也登记（不留模糊）。
    set.add(
        "B-404 简化项如实",
        divergence_for(SysCall::Clone).is_some() && divergence_for(SysCall::EpollCtl).is_some(),
        "clone 标志子集/epoll 语义子集的简化项如实登记——子集外返回对应错误码",
    );
    // 8. 差异表诚实：直通族无差异登记（差异表只记真实差异）。
    set.add(
        "B-404 差异表诚实",
        divergence_for(SysCall::Open).is_none() && divergence_for(SysCall::Read).is_none(),
        "直通族零登记——差异表不掺水，有差异才有行",
    );
    set
}

// ---------------------------------------------------------------------------
// 单测（fd01 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fd01_gov_table_frozen_exhaustive() {
        assert!(table_frozen_ok());
        // 表序即冻结序：行 0 是 Open（直通），行 31 是 Bpf（拒绝）。
        assert_eq!(GOV_TABLE[0].0, SysCall::Open);
        assert_eq!(GOV_TABLE[GOV_ROWS - 1].0, SysCall::Bpf);
        assert_eq!(classify(SysCall::Futex), SysClass::SelfImpl);
        assert_eq!(classify(SysCall::Sigaction), SysClass::Adapt);
    }

    #[test]
    fn fd01_direct_family_full_coverage() {
        // 直通族 16 行 + 拒绝族 5 行 + 自实现 4 行 + 适配 7 行 = 32。
        let mut direct = 0;
        let mut deny = 0;
        let mut i = 0;
        while i < GOV_ROWS {
            match GOV_TABLE[i].1 {
                SysClass::DirectPass => direct += 1,
                SysClass::Deny => deny += 1,
                _ => {}
            }
            i += 1;
        }
        assert_eq!(direct, 16);
        assert_eq!(deny, 5);
        assert_eq!(direct + deny, 21);
    }

    #[test]
    fn fd01_default_deny_unknown() {
        // 表外号码（含边界）一律拒——治理表外无静默成功。
        assert_eq!(classify_num(u32::MAX), SysClass::Deny);
        assert_eq!(classify_num(GOV_ROWS as u32 + 1), SysClass::Deny);
        // 已登记号码按表裁决：行 28 是 IoUring（拒绝）。
        assert_eq!(classify_num(28), SysClass::Deny);
    }

    #[test]
    fn fd01_divergence_honesty() {
        // 拒绝 5 行 + 简化 2 行 = 7 行；直通族零登记。
        assert_eq!(DIVERGENCES.len(), 7);
        assert!(divergence_for(SysCall::IoUring).is_some());
        assert!(divergence_for(SysCall::Getenv).is_none());
        // 指名错误码全为 ENOSYS。
        let mut i = 0;
        while i < DIVERGENCES.len() {
            assert_eq!(DIVERGENCES[i].1, ENOSYS);
            i += 1;
        }
    }
}
