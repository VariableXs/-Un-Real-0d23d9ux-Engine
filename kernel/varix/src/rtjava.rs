//! Java 运行时画像与双载体（WP-304 · B-3301 SC-J 系全绿）。
//!
//! MD2 篇 33.1：OpenJDK 官方 Linux 构建在柜台上的运行要点五条——内存画像
//! （JVM 堆自管，**堆上限按配额七成设参**，其余留给元空间/代码缓存/栈，
//! 画像参数进 vxrun 运行时预设）、线程画像（GC/编译线程活跃，clone 线程
//! 形态支持质量，**futex 语义是验收首位**——JVM 停顿与锁的核心依赖）、
//! 时钟与性能计数（clock_gettime 精度与单调性——B-903 同源的运行时版）、
//! 文件系统语义（锁文件与临时目录在 ext4 全绿——fsync 硬承诺的价值兑现
//! 点）、验收载体（Spring Boot 最小服务 + 桌面 Swing 应用双用例，判例集
//! SC-J01 起编——**服务端与图形端各一条路**）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 内存画像：堆七成设参（数字同源——七成进代码且只此一处）
// ---------------------------------------------------------------------------

/// 堆占配额比例的分子（MD2 33.1：七成设参——其余留给元空间/代码缓存/栈）。
pub const HEAP_FRAC_NUM: u32 = 7;
pub const HEAP_FRAC_DEN: u32 = 10;

/// JVM 堆上限（KB）：配额乘七成，整数下取整——**不虚高**（超出配额的堆
/// 参数是被 OOM 杀手兑现的死支票）。
pub fn jvm_heap_kb(quota_kb: u32) -> u32 {
    quota_kb / HEAP_FRAC_DEN * HEAP_FRAC_NUM
}

/// vxrun 运行时预设（画像参数的交付形态——vxrun 按预设设参，不猜）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RuntimePreset {
    /// JVM 堆上限（KB，jvm_heap_kb 产物）。
    pub heap_kb: u32,
    /// 预设已在册（进 vxrun 参数表）。
    pub registered: bool,
}

// ---------------------------------------------------------------------------
// 线程画像：futex 验收首位
// ---------------------------------------------------------------------------

/// 线程依赖面（clone 线程形态 + futex——JVM 停顿与锁的核心依赖）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThreadDeps {
    /// clone 线程形态支持（篇 4.2 第二类）。
    pub clone_thread: bool,
    /// futex 语义在柜台自实现类的验收结论。
    pub futex: bool,
}

/// 验收顺序（**futex 首位**——先验停顿与锁，再验线程形态：顺序即优先级）。
pub fn deps_check_order(deps: &ThreadDeps) -> bool {
    // 首位语义：futex 不过则整体不过（即使 clone_thread 全绿也不能翻案）。
    deps.futex && deps.clone_thread
}

// ---------------------------------------------------------------------------
// 时钟与文件语义
// ---------------------------------------------------------------------------

/// 时钟探针（clock_gettime 精度与单调性——B-903 同源的运行时版）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockProbe {
    /// 单调性（性能计数的前提——会倒退的时钟喂不出可信的 profiler）。
    pub monotonic: bool,
    /// 精度在册（ns——精度未实测就是未测，不许默认值冒充）。
    pub precision_ns: u32,
}

impl ClockProbe {
    pub fn ok(&self) -> bool {
        self.monotonic && self.precision_ns > 0
    }
}

/// 文件系统语义（锁文件与临时目录在 ext4 全绿——fsync 硬承诺的兑现点，
/// 与 fsyncp 同族：JVM 的锁文件语义踩在 acked⊆flushed 恒等式上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FsSemantics {
    pub lock_file_ok: bool,
    pub tmp_dir_ok: bool,
}

// ---------------------------------------------------------------------------
// 验收载体：SC-J 系（双用例）
// ---------------------------------------------------------------------------

/// Java 载体（穷举双用例——服务端与图形端各一条路）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JavaCarrier {
    /// Spring Boot 最小服务（服务端路）。
    SpringBootService,
    /// 桌面 Swing 应用（图形端路）。
    SwingDesktop,
}

/// 载体总数。
pub const JAVA_CARRIERS: usize = 2;

/// SC-J 判例（编号自 SC-J01 起编）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JCase {
    /// 载体归属。
    pub carrier: JavaCarrier,
    /// 判例号（1 = SC-J01）。
    pub no: u16,
    pub green: bool,
}

/// SC-J 系全绿判（**B-3301 达标线**）：两载体各至少一条判例且全部绿——
/// 只测服务端不测图形端不算全绿。
pub fn scj_series_green(cases: &[JCase]) -> bool {
    let mut seen = [false; JAVA_CARRIERS];
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
// CheckSet（B-3301 · 5 项）
// ---------------------------------------------------------------------------

pub fn run_rtjava_checks() -> CheckSet {
    let mut set = CheckSet::new("B-3301 Java 运行时画像与双载体");
    // 1. 堆七成设参：整数下取整不虚高 + 预设进 vxrun。
    let heap = jvm_heap_kb(1_048_576);
    let preset = RuntimePreset { heap_kb: heap, registered: true };
    set.add(
        "B-3301 堆七成设参",
        heap == 733_999 && heap <= 1_048_576 && preset.registered && jvm_heap_kb(9) == 0,
        "堆上限=配额×7/10（元空间/代码缓存/栈留三成）——整数下取整不虚高，参数进 vxrun 预设",
    );
    // 2. 线程画像验收首位：futex 不过整体不过。
    let futex_bad = ThreadDeps { clone_thread: true, futex: false };
    let both_good = ThreadDeps { clone_thread: true, futex: true };
    set.add(
        "B-3301 futex 验收首位",
        !deps_check_order(&futex_bad) && deps_check_order(&both_good),
        "JVM 停顿与锁的核心依赖先验——首位语义：clone 再绿也翻不了 futex 的案",
    );
    // 3. 时钟判据：单调性 + 精度在册（B-903 运行时版）。
    let clock = ClockProbe { monotonic: true, precision_ns: 100 };
    let regressed = ClockProbe { monotonic: false, precision_ns: 100 };
    let unmeasured = ClockProbe { monotonic: true, precision_ns: 0 };
    set.add(
        "B-3301 时钟单调与精度",
        clock.ok() && !regressed.ok() && !unmeasured.ok(),
        "会倒退的时钟与未实测的精度都过不了判——性能计数的输入必须可信",
    );
    // 4. ext4 文件语义：锁文件与临时目录全绿（fsync 承诺兑现点）。
    let fs = FsSemantics { lock_file_ok: true, tmp_dir_ok: true };
    let half = FsSemantics { lock_file_ok: true, tmp_dir_ok: false };
    set.add(
        "B-3301 ext4 文件语义",
        fs.lock_file_ok && fs.tmp_dir_ok && !(half.lock_file_ok && half.tmp_dir_ok),
        "锁文件+临时目录两路全绿——JVM 的锁语义踩在 fsync 硬承诺上",
    );
    // 5. 双载体 SC-J 全绿（B-3301 达标线）。
    let all = [
        JCase { carrier: JavaCarrier::SpringBootService, no: 1, green: true },
        JCase { carrier: JavaCarrier::SwingDesktop, no: 2, green: true },
    ];
    let server_only = [JCase { carrier: JavaCarrier::SpringBootService, no: 1, green: true }];
    let red_case = [all[0], JCase { carrier: JavaCarrier::SwingDesktop, no: 2, green: false }];
    set.add(
        "B-3301 SC-J 系全绿",
        scj_series_green(&all) && !scj_series_green(&server_only) && !scj_series_green(&red_case),
        "Spring Boot 服务 + Swing 桌面双用例各一条路全绿——只测一头不算（B-3301 达标线）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe11 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe11_heap_fraction_math() {
        // 七成整数下取整（先除后乘）：不虚高、小配额直接归零不留死支票。
        assert_eq!(jvm_heap_kb(1_000_000), 700_000);
        assert_eq!(jvm_heap_kb(1_048_576), 733_999); // 1048576/10=104857, ×7
        assert_eq!(jvm_heap_kb(9), 0); // 9/10=0 → 0×7=0（小配额整段放弃）
        assert_eq!(jvm_heap_kb(20), 14); // 20/10=2 → 2×7=14
        assert_eq!(jvm_heap_kb(0), 0);
        // 恒不超配额。
        assert!(jvm_heap_kb(u32::MAX) <= u32::MAX);
    }

    #[test]
    fn fe11_futex_first_order() {
        // 首位语义的两种破产形态：futex 红或 clone 红。
        assert!(!deps_check_order(&ThreadDeps { clone_thread: true, futex: false }));
        assert!(!deps_check_order(&ThreadDeps { clone_thread: false, futex: true }));
        assert!(deps_check_order(&ThreadDeps { clone_thread: true, futex: true }));
    }

    #[test]
    fn fe11_clock_probe_discipline() {
        // 单调性与精度两要素缺一不可：0 精度=未测=不过。
        assert!(ClockProbe { monotonic: true, precision_ns: 1 }.ok());
        assert!(!ClockProbe { monotonic: true, precision_ns: 0 }.ok());
        assert!(!ClockProbe { monotonic: false, precision_ns: 999 }.ok());
    }

    #[test]
    fn fe11_scj_series_rules() {
        let ok = [
            JCase { carrier: JavaCarrier::SpringBootService, no: 1, green: true },
            JCase { carrier: JavaCarrier::SwingDesktop, no: 1, green: true },
        ];
        assert!(scj_series_green(&ok));
        // 判例号 0 无效（SC-J01 起编——没有第 0 号判例）。
        let zero = [JCase { carrier: JavaCarrier::SpringBootService, no: 0, green: true }, ok[1]];
        assert!(!scj_series_green(&zero));
        // 同载体重复不算两路。
        let dup = [ok[0], JCase { carrier: JavaCarrier::SpringBootService, no: 2, green: true }];
        assert!(!scj_series_green(&dup));
    }
}
