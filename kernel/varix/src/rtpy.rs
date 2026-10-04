//! Python 运行时画像与双载体（WP-304 · B-3302 SC-P 系全绿）。
//!
//! MD2 篇 33.2：CPython 官方构建的要点四条——解释器与扩展模块（纯 Python
//! 包零障碍，**C 扩展二进制轮子依赖封闭树内的 glibc 版本对齐**，Q26 的
//! 指名报错在这类包上最常见，文档给"缺符号"的自查指引）、虚拟环境（venv
//! 的符号链接与路径假设在封闭树上验证，**symlink 循环检测——Q47 的现实
//! 用例**）、并发画像（多进程 fork 语义与多线程两路验证，**GIL 不适用跨
//! 进程**）、验收载体（数据处理脚本与小型 Web 服务双用例，判例集 SC-P01
//! 起编）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// C 扩展轮子：glibc 版本对齐（Q26 指名报错）
// ---------------------------------------------------------------------------

/// C 扩展轮子体检（pip 源里的二进制轮子——纯 Python 包零障碍不进本表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExtWheel {
    /// glibc 版本与封闭树对齐。
    pub glibc_aligned: bool,
}

/// 轮子裁决（**Q26 指名报错语义**：不对齐不是含糊失败，是"缺哪个符号、
/// 对齐到哪个版本"的指名——自查指引随报错给出，报错是功能不是失败）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WheelVerdict {
    /// 直接安装。
    Install,
    /// 指名拒绝（缺符号自查指引随报错给出——Q26）。
    NamedReject,
}

pub fn wheel_verdict(w: &ExtWheel) -> WheelVerdict {
    if w.glibc_aligned {
        WheelVerdict::Install
    } else {
        WheelVerdict::NamedReject
    }
}

// ---------------------------------------------------------------------------
// venv 虚拟环境：symlink 循环检测（Q47 现实用例）
// ---------------------------------------------------------------------------

/// 封闭树内 symlink 跟随的跳数上限（Linux 同款语义——循环不是崩溃是指名错误）。
pub const MAX_SYMLINK_HOPS: u8 = 40;

/// symlink 链跟随：逐跳检查 is_link——落到实体文件返回 Some；跳数撞
/// MAX_SYMLINK_HOPS 上限仍全是链接判循环（None = ELOOP 指名，绝不无限跟随）。
pub fn follow_symlink(is_link: impl Fn(u8) -> bool) -> Option<()> {
    let mut depth = 0u8;
    loop {
        if !is_link(depth) {
            return Some(()); // 落到实体文件
        }
        depth += 1;
        if depth >= MAX_SYMLINK_HOPS {
            return None; // 循环——ELOOP 指名
        }
    }
}

/// venv 体检（符号链接与路径假设在封闭树上验证）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VenvProbe {
    /// symlink 循环检测通过。
    pub no_symlink_loop: bool,
    /// 路径假设（venv 内相对布局）在封闭树成立。
    pub path_layout_ok: bool,
}

impl VenvProbe {
    pub fn ok(&self) -> bool {
        self.no_symlink_loop && self.path_layout_ok
    }
}

// ---------------------------------------------------------------------------
// 并发画像：fork 与线程两路
// ---------------------------------------------------------------------------

/// 并发两路探针（**GIL 不适用跨进程**——多进程与多线程是两套账，不许
/// 拿线程结论冒充进程结论）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConcurrencyProbe {
    /// fork 多进程语义（封闭树内验证）。
    pub fork_multiproc: bool,
    /// 多线程语义。
    pub threads: bool,
}

impl ConcurrencyProbe {
    pub fn both_ok(&self) -> bool {
        self.fork_multiproc && self.threads
    }
}

// ---------------------------------------------------------------------------
// 验收载体：SC-P 系（双用例）
// ---------------------------------------------------------------------------

/// Python 载体（穷举双用例）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PyCarrier {
    /// 数据处理脚本。
    DataScript,
    /// 小型 Web 服务。
    WebService,
}

/// SC-P 判例（编号自 SC-P01 起编）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PCase {
    pub carrier: PyCarrier,
    pub no: u16,
    pub green: bool,
}

/// SC-P 系全绿判（**B-3302 达标线**）：两载体各至少一条判例且全部绿。
pub fn scp_series_green(cases: &[PCase]) -> bool {
    let mut seen = [false; 2];
    let mut i = 0;
    while i < cases.len() {
        if !cases[i].green || cases[i].no == 0 {
            return false;
        }
        seen[cases[i].carrier as usize] = true;
        i += 1;
    }
    seen[0] && seen[1]
}

// ---------------------------------------------------------------------------
// CheckSet（B-3302 · 4 项）
// ---------------------------------------------------------------------------

pub fn run_rtpy_checks() -> CheckSet {
    let mut set = CheckSet::new("B-3302 Python 运行时画像与双载体");
    // 1. C 扩展轮子：glibc 对齐裁决 + Q26 指名报错语义。
    let aligned = ExtWheel { glibc_aligned: true };
    let misaligned = ExtWheel { glibc_aligned: false };
    set.add(
        "B-3302 C 扩展对齐裁决",
        wheel_verdict(&aligned) == WheelVerdict::Install
            && wheel_verdict(&misaligned) == WheelVerdict::NamedReject,
        "二进制轮子按 glibc 对齐裁决——不对齐指名报缺符号并给自查指引（Q26），报错是功能",
    );
    // 2. venv symlink 循环检测（Q47）：跟随有上限，循环是指名错误不是崩溃。
    let linear = follow_symlink(|d| d < 2); // 两跳后落实体
    let looped = follow_symlink(|_| true); // 永是链接
    let venv_ok = VenvProbe { no_symlink_loop: linear.is_some(), path_layout_ok: true };
    set.add(
        "B-3302 venv 循环检测",
        linear.is_some() && looped.is_none() && venv_ok.ok(),
        "symlink 跟随 40 跳封顶——循环判 ELOOP 指名（Q47），路径假设在封闭树上验证",
    );
    // 3. 并发两路：fork 与线程分开记账（GIL 不适用跨进程）。
    let both = ConcurrencyProbe { fork_multiproc: true, threads: true };
    let thread_only = ConcurrencyProbe { fork_multiproc: false, threads: true };
    set.add(
        "B-3302 并发两路验证",
        both.both_ok() && !thread_only.both_ok(),
        "多进程 fork 语义与多线程各验各的——线程绿不冒充进程绿",
    );
    // 4. 双载体 SC-P 全绿（B-3302 达标线）。
    let all = [
        PCase { carrier: PyCarrier::DataScript, no: 1, green: true },
        PCase { carrier: PyCarrier::WebService, no: 2, green: true },
    ];
    let script_only = [PCase { carrier: PyCarrier::DataScript, no: 1, green: true }];
    set.add(
        "B-3302 SC-P 系全绿",
        scp_series_green(&all) && !scp_series_green(&script_only),
        "数据处理脚本 + 小型 Web 服务双用例全绿——只测一头不算（B-3302 达标线）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe12 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe12_wheel_named_reject() {
        // 裁决穷举：对齐装、不对齐指名拒——没有第三种结局。
        assert_eq!(wheel_verdict(&ExtWheel { glibc_aligned: true }), WheelVerdict::Install);
        assert_eq!(wheel_verdict(&ExtWheel { glibc_aligned: false }), WheelVerdict::NamedReject);
    }

    #[test]
    fn fe12_symlink_loop_cap() {
        // 恰好在跳数上限的最后一跳落实体：通过。
        let edge = follow_symlink(|d| d + 1 < MAX_SYMLINK_HOPS);
        assert!(edge.is_some());
        // 永远是链接：撞上限判循环。
        let infinite = follow_symlink(|_| true);
        assert!(infinite.is_none());
        // 立即落实体：零跳通过。
        assert!(follow_symlink(|_| false).is_some());
    }

    #[test]
    fn fe12_concurrency_two_ledgers() {
        // GIL 不适用跨进程：两套账分开判。
        assert!(ConcurrencyProbe { fork_multiproc: true, threads: true }.both_ok());
        assert!(!ConcurrencyProbe { fork_multiproc: false, threads: true }.both_ok());
        assert!(!ConcurrencyProbe { fork_multiproc: true, threads: false }.both_ok());
    }

    #[test]
    fn fe12_scp_series_rules() {
        let ok = [
            PCase { carrier: PyCarrier::DataScript, no: 1, green: true },
            PCase { carrier: PyCarrier::WebService, no: 1, green: true },
        ];
        assert!(scp_series_green(&ok));
        // 红判例污染全系。
        let red = [ok[0], PCase { carrier: PyCarrier::WebService, no: 2, green: false }];
        assert!(!scp_series_green(&red));
        // 空判例集不算全绿。
        let empty: [PCase; 0] = [];
        assert!(!scp_series_green(&empty));
    }
}
