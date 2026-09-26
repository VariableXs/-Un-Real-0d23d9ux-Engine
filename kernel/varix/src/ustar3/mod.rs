//! I 通用域·三分队（Varix STAR I start · I 域 F501~F550 · AI-U3 分工包）。
//!
//! 五十项判据实装层（判据唯一源：主册《Varix STAR I start.md》
//! F501~F550 各节【验收判据】第一句 + 通用验收十二查）。
//! 主题分文件（一处一事实，域内逐项一个 run_fXXX_checks 自检入口）：
//!
//! | 文件 | 覆盖项 |
//! | --- | --- |
//! | `deskicons` | F501 桌面图标文字可读性 / F502 图标文字两行封顶 / F503 图标网格密度 |
//! | `locksec` | F504 PIN 快速解锁 / F505 蓝牙动态锁 / F506 访客模式 / F507 锁屏防截图 / F508 应用防截标记 |
//! | `filesec` | F509 文件粉碎 / F510 单文件加密 / F511 剪贴板一键清空 / F512 截图历史 |
//! | `pointerfx` | F513 Ctrl 定位指针 / F514 声音视觉提示 / F519 大写锁定提示音 / F520 标题栏中键最小化 / F522 指针轨迹显示 / F523 打字时隐藏指针 |
//! | `explorerx` | F515 查找与替换 / F517 资源管理器启动页 / F521 截图保存位置 / F525 快捷键速查卡导出 / F526 状态栏 / F527 导航树折叠展开 / F528 树与列表双向同步 |
//! | `copyops` | F524 撤销清空回收站 / F529 复制前空间预检 / F530 复制后校验 / F531 复制任务队列化 / F532 打开失败人话诊断 / F533 只读介质提醒 / F534 长路径全程支持 |
//! | `winkeys` | F516 通知横幅位置 / F518 输入法切换键 / F535 Win+数字 / F536 Win+T / F537 Win+逗号 / F538 Alt+Esc / F539 桌面布局锁定 / F548 任务管理器置顶 |
//! | `sysdev` | F540 内存诊断 / F541 网络重置 / F542 ClickLock / F543 分设备音量记忆 / F544 通知音量独立分级 / F545 蓝牙耳机电量 / F546 新设备接入通知 / F547 音量左右平衡 |
//! | `clockcal` | F549 时钟悬停完整日期（公历+星期+农历 2026-2030 离线内置） |
//! | `anchor` | F550 I 域批次六验收锚点（F526~F550 二十五检查点入总检） |
//!
//! 共同纪律（对齐 perfstar/K1 批）：零堆热路径（无 String/Vec/Box/format!
//! 进逻辑路径，定长数组 + core 运算）；判据唯一源（每项域头注释逐条摘录
//! 主册判据）；常量注释写明主册依据；自检经 robust.rs 域函数指针表注册
//! （50 域直排注册，禁改直排调用）。

pub mod anchor;
pub mod clockcal;
pub mod copyops;
pub mod deskicons;
pub mod explorerx;
pub mod filesec;
pub mod locksec;
pub mod pointerfx;
pub mod sysdev;
pub mod winkeys;

/// 全包聚合自检（供域外快速对账使用；robust.rs 域表仍逐域直排注册，
/// 保证每项独立红绿可见）。
pub fn run_ustar3_all_checks() -> (usize, usize, bool) {
    const DOMAINS: [fn() -> crate::checks::CheckSet; 50] = [
        deskicons::run_f501_checks,
        deskicons::run_f502_checks,
        deskicons::run_f503_checks,
        locksec::run_f504_checks,
        locksec::run_f505_checks,
        locksec::run_f506_checks,
        locksec::run_f507_checks,
        locksec::run_f508_checks,
        filesec::run_f509_checks,
        filesec::run_f510_checks,
        filesec::run_f511_checks,
        filesec::run_f512_checks,
        pointerfx::run_f513_checks,
        pointerfx::run_f514_checks,
        explorerx::run_f515_checks,
        winkeys::run_f516_checks,
        explorerx::run_f517_checks,
        winkeys::run_f518_checks,
        pointerfx::run_f519_checks,
        pointerfx::run_f520_checks,
        explorerx::run_f521_checks,
        pointerfx::run_f522_checks,
        pointerfx::run_f523_checks,
        copyops::run_f524_checks,
        explorerx::run_f525_checks,
        explorerx::run_f526_checks,
        explorerx::run_f527_checks,
        explorerx::run_f528_checks,
        copyops::run_f529_checks,
        copyops::run_f530_checks,
        copyops::run_f531_checks,
        copyops::run_f532_checks,
        copyops::run_f533_checks,
        copyops::run_f534_checks,
        winkeys::run_f535_checks,
        winkeys::run_f536_checks,
        winkeys::run_f537_checks,
        winkeys::run_f538_checks,
        winkeys::run_f539_checks,
        sysdev::run_f540_checks,
        sysdev::run_f541_checks,
        sysdev::run_f542_checks,
        sysdev::run_f543_checks,
        sysdev::run_f544_checks,
        sysdev::run_f545_checks,
        sysdev::run_f546_checks,
        sysdev::run_f547_checks,
        winkeys::run_f548_checks,
        clockcal::run_f549_checks,
        anchor::run_f550_checks,
    ];
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut all_green = true;
    for f in DOMAINS {
        let cs = f();
        let (p, fl) = cs.tally();
        passed += p;
        failed += fl;
        if !cs.all_passed() {
            all_green = false;
        }
    }
    (passed, failed, all_green)
}
