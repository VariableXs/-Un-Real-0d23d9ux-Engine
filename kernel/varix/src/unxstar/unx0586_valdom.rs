//! unx0586_valdom —— UNX-F0586 参数值域核（域断言）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//!       第 2127 行（UNX-A1-B30 · 命令行参数账 F581–F600 批次表 · AI-01 承办）：
//!
//!   | UNX-F0586 | 参数值域核（域断言） | 200 | 已深化 |
//!   | 值域核账齐（整数 8 键域表+字符串 8 键长表），越域拒收率 100%（注入 4/4） |
//!
//! 同批相邻条目（定位原文）：UNX-F0585 三型值解析承接其产出的三型值，
//! 本条对其落域断言；UNX-F0590 缺省值账与本条做键对账联动（缺省联动面）。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 职责定位：参数域的值域断言是「垃圾进内核」的最后一道闸——语法解析器
//!   （F0582/F0585）只保证「长得像值」，值域核保证「值站得住」。整数键
//!   走三级校验（解析→域界→对齐），字符串键走三级校验（长度→字符集→
//!   转义），每一级留痕可查；越域值全量拒收并入簿留存（原文截断保存），
//!   拒收时给出可操作的合法域中文建议。
//!
//! 组件清单（全手工定制，无空壳）：
//!   1. INT_TABLE      整数 8 键域表（min/max/对齐步长/单位后缀，逐键域来源注释）
//!   2. STR_TABLE      字符串 8 键长表（max_len/字符集闭集/闭集名）
//!   3. DEFAULTS       缺省表 16 行（8 整数键 + 8 字符串键，含单位注记）
//!   4. parse_signed_u64  符号感知十进制解析（u64 显性饱和钳制）
//!   5. Stage/Trace3   三级校验逐级留痕（级次有序性可审计）
//!   6. DomainChecker  值域核主体：check_int / check_str 双通道
//!   7. RejectLedger   越域拒收簿 16 槽（原文截断保存 + 中文建议留存）
//!   8. EventRing      事件账 32 槽滑窗（收/拒/注入/对账/对拍/簿满/闸）
//!   9. suggest 面     域外值建议（「该键合法域 [64, 4096] MB，实得 8192 MB」式）
//!  10. reconcile 面   缺省联动对账（域有缺省无 / 缺省有域无 / 单位不符 /
//!                     缺省值自身越域，四类失配全检出）
//!  11. injection 面   四注入（越上界/越下界/错对齐/坏字符集）全检出 = 4/4
//!  12. dual 面        双跑确定性对拍（同批次两跑指纹与逐项结果全等）
//!  13. audit/health   账目恒等式自审 + 健康分（0~1000）
//!  14. fnv64          FNV-1a 64 位账面指纹
//!
//! 性能逐项分解：
//!   check_int：键查表 O(8) + 解析 O(字节) + 域界 O(1) + 对齐 O(1)；
//!   check_str：键查表 O(8) + 长度 O(1) + 字符集 O(字节×闭集) + 转义 O(字节)；
//!   指纹 O(账面字节)；注入面 O(4)；对账 O(键)；对拍 O(批×单检)。
//!
//! 诊断码段：0x1490~0x149F（16 码闭集）。段位择取实录：优先段 0x1460 被
//!   svstar2/ves444_s24lexin.rs（SEG_BASE=0x1460，另 bfport.rs 0x1460~0x1463、
//!   ckport.rs 0x1468 起）占用；0x1470 被 unxstar/unx1063_nohz.rs 占用；
//!   0x1480 被 svstar2/ves445_s24token.rs 占用——顺位改取 0x1490，全 kernel
//!   树 grep 0x149x 零命中，独占确认。
//!
//! 纪律声明：no_std + alloc；零 unwrap/expect/panic!/运行期 assert!（编译期
//!   const assert 允许）；可失败全走 Result<T, ValDomError>；计数递增一律
//!   wrapping、健康分扣减一律 saturating、解析溢出显性饱和钳制；>64KB
//!   结构体不上栈（账簿/事件/留痕全走 Vec 堆）；const fn 内只用 while+索引、
//!   手工字节比较，不用 slice::get/Vec 索引/Ord::min/max/for 循环。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════
// 版本与全局常量（来源逐条注记）
// ═══════════════════════════════════════════════════════════════════════

/// 模块版本。
pub const VD_VERSION: &str = "unx0586_valdom-v1";

/// 诊断段基址（0x1490 段 · 本模块独占 · 全 kernel 树 grep 0x149x 零命中；
/// 0x1460/0x1470/0x1480 顺位被占，实录见头注诊断码段一节）。
pub const VD_SEG_BASE: u16 = 0x1490;

/// 诊断段顶（基址 + 0x0F = 0x149F，16 码闭集）。
pub const VD_SEG_TOP: u16 = 0x149F;

/// 整数域表键数（判据点名：整数 8 键域表）。
pub const INT_N: usize = 8;

/// 字符串域长表键数（判据点名：字符串 8 键长表）。
pub const STR_N: usize = 8;

/// 缺省表行数（8 整数键 + 8 字符串键，与两张域表同构全联动）。
pub const DEF_N: usize = 16;

/// 拒收簿槽位上限（16 槽；满后溢出计数、拒收账不丢）。
pub const LEDGER_CAP: usize = 16;

/// 事件账滑窗上限（32 槽；满后滑出最旧并计 dropped）。
pub const EVENT_CAP: usize = 32;

/// 近期三级留痕保留条数上限（8 条；新进旧出）。
pub const RECENT_CAP: usize = 8;

/// 拒收簿原文截断保存字节预算（24 字节 + 截断尾注；防越域长串灌爆账簿）。
pub const RAW_KEEP: usize = 24;

/// 拒收簿键名截断保存字节预算（24 字节）。
pub const KEY_KEEP: usize = 24;

/// 拒收率满刻度 ×1000（判据口径：100% = 1000‰，整数定点无浮点）。
pub const REJECT_RATE_FULL: u64 = 1_000;

// ═══════════════════════════════════════════════════════════════════════
// 诊断码闭集（0x1490~0x149F，每变体 detail() 中文可操作诊断）
// ═══════════════════════════════════════════════════════════════════════

/// 值域核诊断码闭集（#[repr(u16)]，码值即段内偏移身份）。
#[repr(u16)]
#[derive(Clone, Copy, Debug)]
pub enum ValDomError {
    /// 0x1490 键不在任何域表（未登记键一律拒收）。
    UnknownKey = 0x1490,
    /// 0x1491 整数解析失败（符号/数字非法或空串）。
    IntParse = 0x1491,
    /// 0x1492 整数越上界（高于域表 max）。
    IntOverUpper = 0x1492,
    /// 0x1493 整数越下界（低于域表 min，含负号注入）。
    IntUnderLower = 0x1493,
    /// 0x1494 整数对齐失配（域内但不落步长网格）。
    IntMisalign = 0x1494,
    /// 0x1495 字符串超长（超 max_len 字节预算）。
    StrTooLong = 0x1495,
    /// 0x1496 字符集外字节（闭集外）。
    StrBadChar = 0x1496,
    /// 0x1497 转义序列非法/残缺（转义闭集外）。
    StrBadEscape = 0x1497,
    /// 0x1498 空串拒收（闭集键不容空值）。
    StrEmpty = 0x1498,
    /// 0x1499 拒收簿满（槽位耗尽，溢出入事件账）。
    LedgerFull = 0x1499,
    /// 0x149A 域有缺省无（域表键在缺省表无行）。
    DefaultMissing = 0x149A,
    /// 0x149B 缺省有域无（缺省表行键不属任何域表）。
    DefaultOrphan = 0x149B,
    /// 0x149C 注入漏检（四注入未全检出）。
    InjectMissed = 0x149C,
    /// 0x149D 双跑指纹不齐（对拍不一致或批次为空）。
    FpMismatch = 0x149D,
    /// 0x149E 留痕/账目自洽破缺（级次乱序或恒等式不成立）。
    TraceBroken = 0x149E,
    /// 0x149F 缺省联动单位不符或缺省值自身越域。
    UnitMismatch = 0x149F,
}

impl ValDomError {
    /// 码值（const fn：DUAL_EXPECTED 基准向量编译期取码用）。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 中文可操作诊断（detail）：说什么坏、下一步动哪里。
    pub fn detail(self) -> &'static str {
        match self {
            ValDomError::UnknownKey => "键不在任何域表：先查 domain_summary_int/domain_summary_str 取合法键清单，未登记键一律拒收",
            ValDomError::IntParse => "整数解析失败：仅容「可选 +/- 前缀 + 至少一位十进制数字」，空串/夹字/前后空白一律拒收",
            ValDomError::IntOverUpper => "越上界：实得值高于域表上限；按域外建议函数取域顶或压值入域",
            ValDomError::IntUnderLower => "越下界：实得值低于域表下限（负号注入亦落此码）；按建议取域底",
            ValDomError::IntMisalign => "对齐失配：值在域内但不落「自 min 起步长网格」；按建议取向下取整合法值",
            ValDomError::StrTooLong => "字符串超长：超域长表 max_len 字节预算；压缩值长或换短键名",
            ValDomError::StrBadChar => "字符集外：存在闭集外字节；改用域表闭集内字符",
            ValDomError::StrBadEscape => "转义非法：反斜杠序列不在合法转义闭集或残缺；改用合法转义或去除反斜杠",
            ValDomError::StrEmpty => "空串拒收：闭集键不容空值；给出非空合法值",
            ValDomError::LedgerFull => "拒收簿满：16 槽耗尽；拒收账不受影响，先消费账簿再继续灌样本",
            ValDomError::DefaultMissing => "域有缺省无：域表键在缺省表无行；补缺省行后再对账",
            ValDomError::DefaultOrphan => "缺省有域无：缺省表行键不属任何域表；删孤行或先入域表",
            ValDomError::InjectMissed => "注入漏检：四注入未全检出；先查对应通道校验逻辑再上账",
            ValDomError::FpMismatch => "双跑指纹不齐：同批次两跑账面不一致或批次为空；先查表驱动路径是否被旁路",
            ValDomError::TraceBroken => "留痕/账目自洽破缺：级次乱序或「检查数=收+拒」恒等式不成立；先修校验流水再上账",
            ValDomError::UnitMismatch => "缺省联动单位不符或缺省值自身越域：修缺省表单位/值后重跑对账",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 拒收方向（账簿/事件账的方向维度；方向账之和恒等于拒收总数）
// ═══════════════════════════════════════════════════════════════════════

/// 拒收方向闭集（9 向；NoDir 供非拒收事件占位）。
#[derive(Clone, Copy, Debug)]
pub enum RejectDir {
    /// 无方向（非拒收事件/未知键拒收占位）。
    NoDir,
    /// 解析失败。
    ParseFail,
    /// 越上界。
    Above,
    /// 越下界。
    Below,
    /// 错对齐。
    Misalign,
    /// 超长。
    TooLong,
    /// 字符集外。
    BadChar,
    /// 转义非法。
    BadEscape,
    /// 空串。
    Empty,
}

/// 方向数（含 NoDir 占位向）。
pub const DIR_N: usize = 9;

/// 方向账槽序（渲染与账目遍历的固定顺序）。
pub const DIR_ORDER: [RejectDir; DIR_N] = [
    RejectDir::NoDir,
    RejectDir::ParseFail,
    RejectDir::Above,
    RejectDir::Below,
    RejectDir::Misalign,
    RejectDir::TooLong,
    RejectDir::BadChar,
    RejectDir::BadEscape,
    RejectDir::Empty,
];

impl RejectDir {
    /// 方向账槽位下标。
    pub fn idx(self) -> usize {
        match self {
            RejectDir::NoDir => 0,
            RejectDir::ParseFail => 1,
            RejectDir::Above => 2,
            RejectDir::Below => 3,
            RejectDir::Misalign => 4,
            RejectDir::TooLong => 5,
            RejectDir::BadChar => 6,
            RejectDir::BadEscape => 7,
            RejectDir::Empty => 8,
        }
    }

    /// 中文方向名。
    pub fn text(self) -> &'static str {
        match self {
            RejectDir::NoDir => "无方向",
            RejectDir::ParseFail => "解析失败",
            RejectDir::Above => "越上界",
            RejectDir::Below => "越下界",
            RejectDir::Misalign => "错对齐",
            RejectDir::TooLong => "超长",
            RejectDir::BadChar => "字符集外",
            RejectDir::BadEscape => "转义非法",
            RejectDir::Empty => "空串",
        }
    }
}

/// 方向下标 → 中文名（越纲防御，渲染用）。
pub fn dir_name(i: usize) -> &'static str {
    if i < DIR_N {
        DIR_ORDER[i].text()
    } else {
        "越纲方向"
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 整数 8 键域表（判据点名：整数 8 键域表；逐键域来源注释）
// ═══════════════════════════════════════════════════════════════════════

/// 整数域规格（min/max 闭区间；step 为「自 min 起网格」的对齐步长；
/// unit 为单位后缀，域外建议与对账联动共用）。
#[derive(Clone, Copy, Debug)]
pub struct IntSpec {
    /// 参数键名。
    pub key: &'static str,
    /// 域下界（含）。
    pub min: i64,
    /// 域上界（含）。
    pub max: i64,
    /// 对齐步长（值 v 须满足 (v - min) % step == 0；≥1）。
    pub step: u64,
    /// 单位后缀（缺省表单位注记对账基准）。
    pub unit: &'static str,
}

/// 整数 8 键域表（每键域来源逐条注记，域值冻结不open）。
pub const INT_TABLE: [IntSpec; INT_N] = [
    // 键 1 loglevel：内核日志级别。域来源： printk 八级惯例（0=emerg..
    // 7=debug）——0~7 闭集、步长 1；本核日志分五级但收满八级入账。
    IntSpec { key: "loglevel", min: 0, max: 7, step: 1, unit: "级" },
    // 键 2 uart.baud：串口波特率。域来源：16550 类 UART 常用底档 9600
    // （再低仅旧终端），上限 4_000_000（高频时钟 PCI 串口可达）；步长 1。
    IntSpec { key: "uart.baud", min: 9_600, max: 4_000_000, step: 1, unit: "波特" },
    // 键 3 console.cols：控制台列数。域来源：VGA 文本 80 列惯例为基准，
    // 最小 20（BIOS 可见最小文本面），上限 512（帧缓冲文本 4 倍放大）；步长 1。
    IntSpec { key: "console.cols", min: 20, max: 512, step: 1, unit: "列" },
    // 键 4 console.rows：控制台行数。域来源：VGA 文本 25 行惯例为基准，
    // 最小 10（可辨引导信息），上限 256（一页帧缓冲极限）；步长 1。
    IntSpec { key: "console.rows", min: 10, max: 256, step: 1, unit: "行" },
    // 键 5 heap.mb：内核堆驻留量。域来源：引导交接最小驻留 64 MB（页表+
    // 元数据底座），上限 4_096 MB（32 位页框索引上限 4 GB）；步长 64 MB
    // （2 MB 大页 ×32 的分配网格，防碎屑驻留）。
    IntSpec { key: "heap.mb", min: 64, max: 4_096, step: 64, unit: "MB" },
    // 键 6 stack.kb：任务内核栈。域来源：x86_64 内核栈底档 8 KB（半档于
    // 常用 16 KB 网格），上限 1_024 KB（1 MB 溢出保险丝）；步长 8 KB。
    IntSpec { key: "stack.kb", min: 8, max: 1_024, step: 8, unit: "KB" },
    // 键 7 sched.slice.us：调度时间片。域来源：下限 1_000 微秒（1 ms，
    // 低于此上下文切换税吃满），上限 1_000_000 微秒（1 s，交互卡死线）；
    // 步长 100 微秒（HPET 可比分辨率网格）。
    IntSpec { key: "sched.slice.us", min: 1_000, max: 1_000_000, step: 100, unit: "微秒" },
    // 键 8 dma.win.mb：DMA 窗口预算。域来源：下限 1 MB（bounce buffer
    // 最小可用窗），上限 1_024 MB（1 GB，24 位寻址设备保守顶）；步长 1。
    IntSpec { key: "dma.win.mb", min: 1, max: 1_024, step: 1, unit: "MB" },
];

// ═══════════════════════════════════════════════════════════════════════
// 字符串 8 键长表（判据点名：字符串 8 键长表；max_len + 字符集闭集）
// ═══════════════════════════════════════════════════════════════════════

/// 字符串域规格（max_len 为原始字节预算；charset 为字节闭集；
/// 反斜杠是转义元字符，归 L3 转义级管辖，字符集级让渡）。
#[derive(Clone, Copy, Debug)]
pub struct StrSpec {
    /// 参数键名。
    pub key: &'static str,
    /// 长度上限（原始字节，含）。
    pub max_len: u16,
    /// 字符集闭集（字节级；反斜杠除外——元字符让渡 L3）。
    pub charset: &'static [u8],
    /// 闭集名（域外建议与渲染用）。
    pub cs_name: &'static str,
}

// ── 字符集闭集定义（字节串字面量冻结） ─────────────────────────────────
/// 主机名闭集：小写/大写字母 + 数字 + 连字符（RFC 1123 主机名收窄版）。
const CS_HOSTNAME: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-";
/// 控制台设备闭集：ttyS 前缀字母 + 数字 + 点分（ttyS0 / ttyS1.2 形）。
const CS_CONSOLE_DEV: &[u8] = b"ttyS0123456789.";
/// 十六进制小写闭集：引导 ID 用（大写经 \x 转义亦不放宽——键值本体限小写）。
const CS_HEX_LOWER: &[u8] = b"0123456789abcdef";
/// 日志标签闭集：字母 + 数字 + 下划线（C 标识符子集）。
const CS_LOG_TAG: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
/// 视频模式闭集：小写字母 + 数字（1024x768 形，x 即小写字母内）。
const CS_VIDEO: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
/// 区域名闭集：小写字母 + 连字符（zh-hans / en-us 形）。
const CS_LOCALE: &[u8] = b"abcdefghijklmnopqrstuvwxyz-";
/// 内核选项闭集：字母 + 数字 + 下划线 + 连字符（nokaslr / no-kaslr 形）。
const CS_KOPT: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";
/// 命令前缀闭集：大写字母 + 数字（VX / VX2 形，短促可辨）。
const CS_PREFIX: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// 字符串 8 键长表（每键长度上限来源逐条注记）。
pub const STR_TABLE: [StrSpec; STR_N] = [
    // 键 1 hostname：主机名。上限 32 字节——RFC 1035 标签上限 63 过宽，
    // 引导期收窄到 32（一行日志内可完整呈现）；闭集字母数字连字符。
    StrSpec { key: "hostname", max_len: 32, charset: CS_HOSTNAME, cs_name: "主机名闭集（字母数字连字符）" },
    // 键 2 console.dev：控制台设备名。上限 16 字节——ttyS + 两位数字 +
    // 点分实例足够；闭集 ttyS 字母 + 数字 + 点。
    StrSpec { key: "console.dev", max_len: 16, charset: CS_CONSOLE_DEV, cs_name: "控制台设备闭集" },
    // 键 3 boot.id：引导 ID。上限 12 字节——8 字节时间戳 + 4 字节随机
    // 十六进制足够全局唯一；闭集小写十六进制。
    StrSpec { key: "boot.id", max_len: 12, charset: CS_HEX_LOWER, cs_name: "引导 ID 十六进制闭集" },
    // 键 4 log.tag：日志标签。上限 16 字节——行首标签区预算；闭集
    // 字母数字下划线（C 标识符子集，宏拼接安全）。
    StrSpec { key: "log.tag", max_len: 16, charset: CS_LOG_TAG, cs_name: "日志标签闭集" },
    // 键 5 video.mode：视频模式名。上限 8 字节——1024x768 形即满档；
    // 闭集小写字母数字。
    StrSpec { key: "video.mode", max_len: 8, charset: CS_VIDEO, cs_name: "视频模式闭集（小写字母数字）" },
    // 键 6 locale：区域名。上限 8 字节——zh-hans / en-us 形即满档；
    // 闭集小写字母连字符。
    StrSpec { key: "locale", max_len: 8, charset: CS_LOCALE, cs_name: "区域名闭集（小写字母连字符）" },
    // 键 7 kernel.opt：内核选项名。上限 24 字节——单 token 界 64（F0589）
    // 收窄到 24（选项名非值）；闭集字母数字下划线连字符。
    StrSpec { key: "kernel.opt", max_len: 24, charset: CS_KOPT, cs_name: "内核选项闭集" },
    // 键 8 cmd.prefix：命令前缀。上限 8 字节——VX / VX2 形短促可辨；
    // 闭集大写字母数字。
    StrSpec { key: "cmd.prefix", max_len: 8, charset: CS_PREFIX, cs_name: "命令前缀闭集（大写字母数字）" },
];

// ═══════════════════════════════════════════════════════════════════════
// 缺省表（16 行 = 8 整数键 + 8 字符串键；与两张域表同构全联动）
// ═══════════════════════════════════════════════════════════════════════

/// 缺省表行（key + 缺省值 + 单位注记；字符串键单位为空串——单位对账只对整数键）。
#[derive(Clone, Copy, Debug)]
pub struct DefRow {
    /// 参数键名。
    pub key: &'static str,
    /// 缺省值原文（须过自身域检——reconcile 的 def_invalid 面复检）。
    pub val: &'static str,
    /// 单位注记（整数键须与域表 unit 全等，否则单位不符）。
    pub unit: &'static str,
}

/// 缺省表 16 行（值冻结；每行均过自身域检——见 reconcile_builtin 测试）。
pub const DEFAULTS: [DefRow; DEF_N] = [
    // 整数键缺省（单位注记与 INT_TABLE.unit 逐字对齐）
    DefRow { key: "loglevel", val: "6", unit: "级" },
    DefRow { key: "uart.baud", val: "115200", unit: "波特" },
    DefRow { key: "console.cols", val: "80", unit: "列" },
    DefRow { key: "console.rows", val: "25", unit: "行" },
    DefRow { key: "heap.mb", val: "512", unit: "MB" },
    DefRow { key: "stack.kb", val: "16", unit: "KB" },
    DefRow { key: "sched.slice.us", val: "4000", unit: "微秒" },
    DefRow { key: "dma.win.mb", val: "64", unit: "MB" },
    // 字符串键缺省（单位空串）
    DefRow { key: "hostname", val: "varix", unit: "" },
    DefRow { key: "console.dev", val: "ttyS0", unit: "" },
    DefRow { key: "boot.id", val: "0d23d90010", unit: "" },
    DefRow { key: "log.tag", val: "boot", unit: "" },
    DefRow { key: "video.mode", val: "1024x768", unit: "" },
    DefRow { key: "locale", val: "zh-hans", unit: "" },
    DefRow { key: "kernel.opt", val: "nokaslr", unit: "" },
    DefRow { key: "cmd.prefix", val: "VX", unit: "" },
];

// ═══════════════════════════════════════════════════════════════════════
// 编译期自审（const fn：只 while+索引+手工字节比较，无 for/get/Ord）
// ═══════════════════════════════════════════════════════════════════════

/// const str 全等（逐字节 while 比较；const 语境禁 derive PartialEq 的 ==）。
const fn vd_str_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut i = 0;
    while i < ab.len() {
        if ab[i] != bb[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// const str 非空。
const fn vd_str_nonempty(a: &str) -> bool {
    a.as_bytes().len() > 0
}

/// 整数域表自审：键非空、min≤max、步长≥1、单位非空。
const fn int_table_check() -> bool {
    let mut i = 0;
    while i < INT_N {
        let s = INT_TABLE[i];
        if !vd_str_nonempty(s.key) {
            return false;
        }
        if s.min > s.max {
            return false;
        }
        if s.step == 0 {
            return false;
        }
        if !vd_str_nonempty(s.unit) {
            return false;
        }
        i += 1;
    }
    true
}

/// 字符串域长表自审：键非空、max_len≥1、闭集非空、闭集名非空。
const fn str_table_check() -> bool {
    let mut i = 0;
    while i < STR_N {
        let s = STR_TABLE[i];
        if !vd_str_nonempty(s.key) {
            return false;
        }
        if s.max_len == 0 {
            return false;
        }
        if s.charset.len() == 0 {
            return false;
        }
        if !vd_str_nonempty(s.cs_name) {
            return false;
        }
        i += 1;
    }
    true
}

/// 缺省联动自审：域表每键在缺省表有行（域有缺省无=否）；
/// 缺省表每键恰属一张域表一次（缺省有域无=否且键唯一）。
const fn defaults_linked() -> bool {
    if DEFAULTS.len() != DEF_N {
        return false;
    }
    // 域表 → 缺省表方向
    let mut i = 0;
    while i < INT_N {
        let k = INT_TABLE[i].key;
        let mut hit = false;
        let mut j = 0;
        while j < DEF_N {
            if vd_str_eq(DEFAULTS[j].key, k) {
                hit = true;
            }
            j += 1;
        }
        if !hit {
            return false;
        }
        i += 1;
    }
    let mut i2 = 0;
    while i2 < STR_N {
        let k = STR_TABLE[i2].key;
        let mut hit = false;
        let mut j = 0;
        while j < DEF_N {
            if vd_str_eq(DEFAULTS[j].key, k) {
                hit = true;
            }
            j += 1;
        }
        if !hit {
            return false;
        }
        i2 += 1;
    }
    // 缺省表 → 域表方向（恰命中一次）
    let mut j = 0;
    while j < DEF_N {
        let k = DEFAULTS[j].key;
        let mut hits = 0;
        let mut i3 = 0;
        while i3 < INT_N {
            if vd_str_eq(INT_TABLE[i3].key, k) {
                hits += 1;
            }
            i3 += 1;
        }
        let mut i4 = 0;
        while i4 < STR_N {
            if vd_str_eq(STR_TABLE[i4].key, k) {
                hits += 1;
            }
            i4 += 1;
        }
        if hits != 1 {
            return false;
        }
        j += 1;
    }
    true
}

// 编译期闸（const assert 允许；任何表被改破即编译失败）
const _: () = assert!(VD_SEG_BASE == 0x1490);
const _: () = assert!(VD_SEG_TOP == VD_SEG_BASE + 0x0F);
const _: () = assert!(INT_N == 8);
const _: () = assert!(STR_N == 8);
const _: () = assert!(DEF_N == 16);
const _: () = assert!(int_table_check());
const _: () = assert!(str_table_check());
const _: () = assert!(defaults_linked());

// ═══════════════════════════════════════════════════════════════════════
// 解析层：符号感知十进制 → u64 显性饱和
// ═══════════════════════════════════════════════════════════════════════

/// 解析产出（幅值 u64 + 符号 + 饱和痕 + 位数十进制）。
#[derive(Clone, Copy, Debug)]
pub struct Parsed {
    /// true = 负号前缀（含「-」）。
    pub negative: bool,
    /// 十进制幅值；溢出时显性饱和钳制至 u64::MAX 并置 satur。
    pub mag: u64,
    /// 饱和痕（mag*10+d 溢出即置，域界级必拒）。
    pub satur: bool,
    /// 十进制位数（≥1）。
    pub digits: usize,
}

/// 符号感知十进制解析：
///   文法 = [+-]? DIGIT+（不容前后空白、不容夹字）；
///   溢出处理 = checked_mul/checked_add，任一溢出即 satur=true 且幅值钳 u64::MAX。
pub fn parse_signed_u64(raw: &str) -> Result<Parsed, ValDomError> {
    let b = raw.as_bytes();
    if b.is_empty() {
        return Err(ValDomError::IntParse);
    }
    let mut i = 0usize;
    let mut negative = false;
    if b[0] == b'+' || b[0] == b'-' {
        negative = b[0] == b'-';
        i = 1;
    }
    if i >= b.len() {
        // 只有符号没有数字
        return Err(ValDomError::IntParse);
    }
    let mut mag: u64 = 0;
    let mut satur = false;
    let mut digits = 0usize;
    while i < b.len() {
        let c = b[i];
        if c < b'0' || c > b'9' {
            return Err(ValDomError::IntParse);
        }
        let d = (c - b'0') as u64;
        // 显性饱和：两段 checked，任一溢出即钳 u64::MAX 并留痕
        match mag.checked_mul(10) {
            Some(m) => match m.checked_add(d) {
                Some(v) => mag = v,
                None => {
                    satur = true;
                    mag = u64::MAX;
                }
            },
            None => {
                satur = true;
                mag = u64::MAX;
            }
        }
        digits += 1;
        i += 1;
    }
    Ok(Parsed { negative, mag, satur, digits })
}

/// 幅值+符号 → i64 域空间（饱和钳 i64::MIN/i64::MAX；wrapping_neg 处理 2^63）。
fn to_i64_domain(p: &Parsed) -> i64 {
    if !p.negative {
        if p.mag > i64::MAX as u64 {
            i64::MAX
        } else {
            p.mag as i64
        }
    } else if p.mag > (i64::MAX as u64) + 1 {
        i64::MIN
    } else {
        // mag ≤ 2^63：as i64 对 2^63 得 i64::MIN，wrapping_neg 幂等
        (p.mag as i64).wrapping_neg()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 三级留痕（Stage/Trace3）：级次有序性可审计
// ═══════════════════════════════════════════════════════════════════════

/// 单级留痕（ran=本级已执行；ok=通过；code=拒收码，过级为 0）。
#[derive(Clone, Copy, Debug)]
pub struct Stage {
    /// 级名（L1 解析 / L2 域界 / L3 对齐 / L1 长度 / L2 字符集 / L3 转义）。
    pub name: &'static str,
    /// 本级是否已执行（未达级 ran=false）。
    pub ran: bool,
    /// 本级是否通过。
    pub ok: bool,
    /// 拒收码（过级 0）。
    pub code: u16,
    /// 级内注记（&'static 闭集）。
    pub note: &'static str,
}

impl Stage {
    /// 过级留痕。
    pub const fn pass(name: &'static str, note: &'static str) -> Stage {
        Stage { name, ran: true, ok: true, code: 0, note }
    }

    /// 拒级留痕。
    pub const fn fail(name: &'static str, code: u16, note: &'static str) -> Stage {
        Stage { name, ran: true, ok: false, code, note }
    }

    /// 未达级留痕（级次短路——前级拒则后级不跑）。
    pub const fn skip(name: &'static str) -> Stage {
        Stage { name, ran: false, ok: false, code: 0, note: "未达（级次短路）" }
    }
}

/// 三级留痕（整数：解析→域界→对齐；字符串：长度→字符集→转义）。
#[derive(Clone, Copy, Debug)]
pub struct Trace3 {
    /// 留痕主体键名（&'static，域表键）。
    pub subject: &'static str,
    /// 第一级。
    pub s1: Stage,
    /// 第二级。
    pub s2: Stage,
    /// 第三级。
    pub s3: Stage,
}

impl Trace3 {
    /// 级次有序性：后级已达 ⇒ 前级已达且通过（失败即短路，不允许越级）。
    pub fn ordered(&self) -> bool {
        if self.s2.ran && !(self.s1.ran && self.s1.ok) {
            return false;
        }
        if self.s3.ran && !(self.s2.ran && self.s2.ok) {
            return false;
        }
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 工具层：截断保存 / 字符集扫描 / 转义扫描
// ═══════════════════════════════════════════════════════════════════════

/// UTF-8 安全截断（超预算回退到字符边界，尾注「…(截)」标截断保存）。
pub fn trunc_flag(s: &str, keep: usize) -> (String, bool) {
    let b = s.as_bytes();
    if b.len() <= keep {
        return (s.to_string(), false);
    }
    let mut cut = keep;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::new();
    out.push_str(&s[..cut]);
    out.push_str("…(截)");
    (out, true)
}

/// 截断保存（不关心截断痕标记）。
pub fn trunc(s: &str, keep: usize) -> String {
    let (h, _) = trunc_flag(s, keep);
    h
}

/// 字节是否在闭集内（线性扫；闭集均 ≤ 90 字节，O(闭集) 可忽略）。
fn charset_has(cs: &[u8], c: u8) -> bool {
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == c {
            return true;
        }
        i += 1;
    }
    false
}

/// 十六进制位（大小写均收——\xHH 转义闭集；键值本体闭集另限）。
fn is_hex(c: u8) -> bool {
    (c >= b'0' && c <= b'9') || (c >= b'a' && c <= b'f') || (c >= b'A' && c <= b'F')
}

/// 转义序列扫描（L3）：
///   合法转义闭集 = \\ \" \n \t \xHH（HH 两位十六进制，大小写均收）；
///   Ok(逻辑长) = 非转义字节逐字节计 1、合法转义整体计 1；
///   Err(位置) = 首个非法/残缺转义的起始字节下标。
fn scan_escape(b: &[u8]) -> Result<usize, usize> {
    let mut i = 0usize;
    let mut logical = 0usize;
    while i < b.len() {
        if b[i] == b'\\' {
            if i + 1 >= b.len() {
                // 残缺：孤立反斜杠收笔
                return Err(i);
            }
            let n = b[i + 1];
            match n {
                b'n' | b't' | b'\\' | b'"' => {
                    logical += 1;
                    i += 2;
                }
                b'x' => {
                    if i + 3 >= b.len() {
                        return Err(i);
                    }
                    if !is_hex(b[i + 2]) || !is_hex(b[i + 3]) {
                        return Err(i);
                    }
                    logical += 1;
                    i += 4;
                }
                _ => return Err(i),
            }
        } else {
            logical += 1;
            i += 1;
        }
    }
    Ok(logical)
}

// ═══════════════════════════════════════════════════════════════════════
// 域外值建议（detail 精神的动态延伸：越域时给合法域与最近合法值）
// ═══════════════════════════════════════════════════════════════════════

/// 整数域摘要（「heap.mb 合法域 [64, 4096] MB，步长 64 MB」式）。
pub fn domain_summary_int(key: &str) -> Result<String, ValDomError> {
    match int_index(key) {
        Some(i) => {
            let s = INT_TABLE[i];
            Ok(format!(
                "{} 合法域 [{}, {}] {}，步长 {} {}（域来源见 INT_TABLE 逐键注释）",
                s.key, s.min, s.max, s.unit, s.step, s.unit
            ))
        }
        None => Err(ValDomError::UnknownKey),
    }
}

/// 字符串域摘要（「hostname 长度上限 32 字节，字符集闭集「主机名闭集…」」式）。
pub fn domain_summary_str(key: &str) -> Result<String, ValDomError> {
    match str_index(key) {
        Some(i) => {
            let s = STR_TABLE[i];
            Ok(format!(
                "{} 长度上限 {} 字节，字符集闭集「{}」",
                s.key, s.max_len, s.cs_name
            ))
        }
        None => Err(ValDomError::UnknownKey),
    }
}

/// 越上界建议：「该键合法域 [min, max] unit，实得 got unit；建议改取域顶 max unit」。
pub fn hint_int_above(s: &IntSpec, got: i64) -> String {
    format!(
        "越上界：该键「{}」合法域 [{}, {}] {}，实得 {} {}；建议改取域顶 {} {}",
        s.key, s.min, s.max, s.unit, got, s.unit, s.max, s.unit
    )
}

/// 越下界建议：「该键合法域 [min, max] unit，实得 got unit；建议改取域底 min unit」。
pub fn hint_int_below(s: &IntSpec, got: i64) -> String {
    format!(
        "越下界：该键「{}」合法域 [{}, {}] {}，实得 {} {}；建议改取域底 {} {}",
        s.key, s.min, s.max, s.unit, got, s.unit, s.min, s.unit
    )
}

/// 对齐失配建议：给域界、步长、实得值与向下取整最近合法值。
pub fn hint_int_misalign(s: &IntSpec, got: i64) -> String {
    let near = nearest_legal_down(s, got);
    format!(
        "对齐失配：该键「{}」合法域 [{}, {}] {}，步长 {} {}（自 {} 起网格），实得 {} {}；向下取整建议 {} {}（域内最近合法值）",
        s.key, s.min, s.max, s.unit, s.step, s.unit, s.min, got, s.unit, near, s.unit
    )
}

/// 解析失败建议（静态文案，样本入拒收簿可查原文）。
pub fn hint_int_parse() -> String {
    "解析失败：仅容「可选 +/- 前缀 + 至少一位十进制数字」；原文已截断入拒收簿可查".to_string()
}

/// 未知键建议。
pub fn hint_unknown(key: &str) -> String {
    format!(
        "未知键「{}」：整数键见 INT_TABLE（8 键）、字符串键见 STR_TABLE（8 键），未登记键一律拒收",
        trunc(key, KEY_KEEP)
    )
}

/// 长度越界建议。
pub fn hint_str_len(s: &StrSpec, got: usize) -> String {
    format!(
        "长度越界：该键上限 {} 字节，实得 {} 字节；建议压缩至 {} 字节内",
        s.max_len, got, s.max_len
    )
}

/// 字符集外建议：报首犯字节位置与字节值。
pub fn hint_str_char(s: &StrSpec, pos: usize, byte: u8) -> String {
    format!(
        "字符集外：第 {} 字节 {} 不在闭集「{}」；合法集见 STR_TABLE",
        pos,
        hex_byte(byte),
        s.cs_name
    )
}

/// 转义非法建议：报位置与合法转义闭集。
pub fn hint_str_escape(pos: usize) -> String {
    format!(
        "转义非法：第 {} 字节起序列不在合法转义闭集（仅容 \\\\ \\\" \\n \\t \\xHH）；建议改用合法转义或去除反斜杠",
        pos
    )
}

/// 空串建议。
pub fn hint_str_empty() -> String {
    "空串拒收：闭集键不容空值；建议给出非空合法值".to_string()
}

/// 字节十六进制渲染（0xXX）。
pub fn hex_byte(b: u8) -> String {
    format!("0x{:02X}", b)
}

/// 域内向下取整最近合法值（自 min 起网格；越域侧钳回域界）。
pub fn nearest_legal_down(s: &IntSpec, v: i64) -> i64 {
    if v < s.min {
        return s.min;
    }
    if v > s.max {
        return s.max;
    }
    let off = (v - s.min) as u64;
    let steps = off / s.step;
    s.min + (steps.wrapping_mul(s.step)) as i64
}

// ═══════════════════════════════════════════════════════════════════════
// 键查表（运行时；与 const 自审共用同一批表）
// ═══════════════════════════════════════════════════════════════════════

/// 键型（Int/Str/Unknown）。
#[derive(Clone, Copy, Debug)]
pub enum KeyKind {
    /// 整数域键。
    Int,
    /// 字符串域键。
    Str,
    /// 未登记键。
    Unknown,
}

impl KeyKind {
    /// 中文键型名。
    pub fn text(self) -> &'static str {
        match self {
            KeyKind::Int => "整数键",
            KeyKind::Str => "字符串键",
            KeyKind::Unknown => "未登记键",
        }
    }
}

/// 整数域表键查（O(8)）。
pub fn int_index(key: &str) -> Option<usize> {
    let mut i = 0;
    while i < INT_N {
        if INT_TABLE[i].key == key {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 字符串域长表键查（O(8)）。
pub fn str_index(key: &str) -> Option<usize> {
    let mut i = 0;
    while i < STR_N {
        if STR_TABLE[i].key == key {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 键型判定（先整数表后字符串表）。
pub fn key_kind(key: &str) -> KeyKind {
    if int_index(key).is_some() {
        return KeyKind::Int;
    }
    if str_index(key).is_some() {
        return KeyKind::Str;
    }
    KeyKind::Unknown
}

/// 缺省表查找（&'static DEFAULTS 或外部行集均可用）。
fn find_def<'a>(rows: &'a [DefRow], key: &str) -> Option<&'a DefRow> {
    for r in rows {
        if r.key == key {
            return Some(r);
        }
    }
    None
}

/// 内建缺省值解析（无参场景缺省生效的取值口）。
pub fn default_of(key: &str) -> Result<&'static str, ValDomError> {
    match find_def(&DEFAULTS, key) {
        Some(r) => Ok(r.val),
        None => Err(ValDomError::DefaultMissing),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 校验产出（CheckedInt / CheckedStr，含三级留痕）
// ═══════════════════════════════════════════════════════════════════════

/// 整数键校验产出。
#[derive(Clone, Debug)]
pub struct CheckedInt {
    /// 域表键名。
    pub key: &'static str,
    /// 域内值（i64 空间）。
    pub value: i64,
    /// 原文字节长。
    pub raw_len: usize,
    /// 单位后缀（域表 unit）。
    pub unit: &'static str,
    /// 解析饱和痕（true 表示原文超 u64 幅值、被钳制后拒收）。
    pub satur: bool,
    /// 三级留痕。
    pub trace: Trace3,
}

/// 字符串键校验产出。
#[derive(Clone, Debug)]
pub struct CheckedStr {
    /// 域表键名。
    pub key: &'static str,
    /// 原始字节长（L1 度量口径）。
    pub raw_len: usize,
    /// 逻辑长（合法转义整体计 1；L3 产物）。
    pub logical_len: usize,
    /// 三级留痕。
    pub trace: Trace3,
}

// ═══════════════════════════════════════════════════════════════════════
// 越域拒收簿（16 槽；原文截断保存 + 中文建议留存；满后溢出计数不丢账）
// ═══════════════════════════════════════════════════════════════════════

/// 拒收簿条目。
#[derive(Clone, Debug)]
pub struct RejectEntry {
    /// 键名（截断保存，KEY_KEEP 预算）。
    pub key: String,
    /// 越域值原文（截断保存，RAW_KEEP 预算；截断尾注「…(截)」）。
    pub raw: String,
    /// 拒收方向。
    pub dir: RejectDir,
    /// 诊断码。
    pub code: u16,
    /// 域外建议（中文可操作）。
    pub hint: String,
    /// 受理时点（核内 tick）。
    pub tick: u64,
    /// 原文是否被截断保存。
    pub truncated: bool,
}

/// 越域拒收簿（判据点名：越域值样本留存）。
#[derive(Clone, Debug)]
pub struct RejectLedger {
    slots: Vec<RejectEntry>,
    /// 满后溢出计数（拒收账不受影响——拒收恒全量入账，只是样本不再留存）。
    pub overflow: u64,
}

impl RejectLedger {
    /// 空簿。
    pub fn new() -> Self {
        RejectLedger { slots: Vec::new(), overflow: 0 }
    }

    /// 当前条目数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否已满。
    pub fn is_full(&self) -> bool {
        self.slots.len() >= LEDGER_CAP
    }

    /// 入簿（满则溢出计数、返回 false）。
    pub fn push(&mut self, e: RejectEntry) -> bool {
        if self.slots.len() >= LEDGER_CAP {
            self.overflow = self.overflow.wrapping_add(1);
            return false;
        }
        self.slots.push(e);
        true
    }

    /// 按位取条目。
    pub fn entry(&self, i: usize) -> Option<&RejectEntry> {
        self.slots.get(i)
    }

    /// 全量条目（渲染/导出用）。
    pub fn all(&self) -> &[RejectEntry] {
        &self.slots
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 事件账（32 槽滑窗；收/拒/注入/对账/对拍/簿满/闸 七类）
// ═══════════════════════════════════════════════════════════════════════

/// 事件类闭集。
#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum EventKind {
    /// 收值过闸。
    Accept = 1,
    /// 拒收越域。
    Reject = 2,
    /// 注入面结果。
    Inject = 3,
    /// 缺省对账结果。
    Recon = 4,
    /// 双跑对拍结果。
    Dual = 5,
    /// 拒收簿满溢出。
    Ledger = 6,
    /// 闸面（注入闸/对拍闸/自审闸）结果。
    Gate = 7,
}

impl EventKind {
    /// 中文事件名。
    pub fn text(self) -> &'static str {
        match self {
            EventKind::Accept => "收值",
            EventKind::Reject => "拒收",
            EventKind::Inject => "注入",
            EventKind::Recon => "对账",
            EventKind::Dual => "对拍",
            EventKind::Ledger => "簿满",
            EventKind::Gate => "闸面",
        }
    }
}

/// 事件条目。
#[derive(Clone, Debug)]
pub struct Event {
    /// 时点（核内 tick）。
    pub tick: u64,
    /// 事件类。
    pub kind: EventKind,
    /// 关联诊断码（0=无）。
    pub code: u16,
    /// 方向（仅拒收事件有义，他类 NoDir）。
    pub dir: RejectDir,
}

/// 事件账（32 槽滑窗；满则滑出最旧并计 dropped——事件账是观测面不是存证面）。
#[derive(Clone, Debug)]
pub struct EventRing {
    slots: Vec<Event>,
    /// 滑出事件数。
    pub dropped: u64,
}

impl EventRing {
    /// 空账。
    pub fn new() -> Self {
        EventRing { slots: Vec::new(), dropped: 0 }
    }

    /// 入账（滑窗）。
    pub fn push(&mut self, e: Event) {
        if self.slots.len() >= EVENT_CAP {
            self.slots.remove(0);
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.slots.push(e);
    }

    /// 当前事件数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 全量事件（渲染用）。
    pub fn all(&self) -> &[Event] {
        &self.slots
    }
}

// ═══════════════════════════════════════════════════════════════════════
// FNV-1a 64 位账面指纹
// ═══════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位偏移基（Landon Curt Noll 公开常量：0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数（Landon Curt Noll 公开常量：0x0100000001b3）。
pub const FNV_PRIME: u64 = 0x0100000001b3;

/// FNV-1a 64 位（逐字节 xor-mul，乘法显性 wrapping）。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════
// 值域核主体（DomainChecker）
// ═══════════════════════════════════════════════════════════════════════

/// 值域核（判据点名：值域核账齐）。
///
/// 含 Vec 字段的核体不 derive Copy（纪律）； Clone 可用于快照对拍。
#[derive(Clone, Debug)]
pub struct DomainChecker {
    /// 核内时点（每受理自增，wrapping）。
    pub ticks: u64,
    /// 整数通道受理数（含未知键拒收）。
    pub checks_int: u64,
    /// 字符串通道受理数（含未知键拒收）。
    pub checks_str: u64,
    /// 收值数（三级全过）。
    pub accepted: u64,
    /// 拒收数（任一级拒）。
    pub rejected: u64,
    /// 方向账（下标即 RejectDir::idx；方向账之和恒等于 rejected）。
    pub rej_by_dir: [u64; DIR_N],
    /// 码账（下标 = 码 - VD_SEG_BASE；码账之和恒等于 rejected）。
    pub rej_by_code: [u64; 16],
    /// 越域拒收簿。
    pub ledger: RejectLedger,
    /// 事件账。
    pub events: EventRing,
    /// 近期三级留痕（新进旧出，上限 RECENT_CAP）。
    pub recent_traces: Vec<Trace3>,
    /// 最近一次注入面报告。
    pub last_inject: Option<InjectReport>,
    /// 最近一次对账报告。
    pub last_recon: Option<ReconReport>,
    /// 最近一次对拍报告。
    pub last_dual: Option<DualReport>,
}

impl DomainChecker {
    /// 新核（零堆预占，全懒增长）。
    pub fn new() -> Self {
        DomainChecker {
            ticks: 0,
            checks_int: 0,
            checks_str: 0,
            accepted: 0,
            rejected: 0,
            rej_by_dir: [0; DIR_N],
            rej_by_code: [0; 16],
            ledger: RejectLedger::new(),
            events: EventRing::new(),
            recent_traces: Vec::new(),
            last_inject: None,
            last_recon: None,
            last_dual: None,
        }
    }

    /// 时点。
    pub fn tick(&self) -> u64 {
        self.ticks
    }

    /// 拒收率 ×1000（‰ 整数定点；判据口径：注入 4/4 ⇒ 1000‰）。
    pub fn rejection_permille(&self) -> u64 {
        let total = self.accepted.wrapping_add(self.rejected);
        if total == 0 {
            return 0;
        }
        self.rejected.wrapping_mul(REJECT_RATE_FULL) / total
    }

    // ── 内部：时点 / 留痕滚动 / 拒收记账 ────────────────────────────
    fn tick_inc(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
    }

    fn remember(&mut self, t: Trace3) {
        self.recent_traces.push(t);
        if self.recent_traces.len() > RECENT_CAP {
            self.recent_traces.remove(0);
        }
    }

    fn count_reject(&mut self, dir: RejectDir, code: u16) {
        self.rejected = self.rejected.wrapping_add(1);
        let di = dir.idx();
        self.rej_by_dir[di] = self.rej_by_dir[di].wrapping_add(1);
        let off = code.wrapping_sub(VD_SEG_BASE) as usize;
        if off < 16 {
            self.rej_by_code[off] = self.rej_by_code[off].wrapping_add(1);
        }
    }

    /// 拒收记账 + 入簿（截断保存）+ 事件账。
    fn record_reject(&mut self, key: &str, raw: &str, dir: RejectDir, code: ValDomError, hint: String) {
        self.count_reject(dir, code.code());
        let (kept, truncated) = trunc_flag(raw, RAW_KEEP);
        let entry = RejectEntry {
            key: trunc(key, KEY_KEEP),
            raw: kept,
            dir,
            code: code.code(),
            hint,
            tick: self.ticks,
            truncated,
        };
        if !self.ledger.push(entry) {
            // 簿满：样本不再留存但拒收账照记，簿满入事件账
            self.events.push(Event {
                tick: self.ticks,
                kind: EventKind::Ledger,
                code: ValDomError::LedgerFull.code(),
                dir,
            });
        }
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::Reject,
            code: code.code(),
            dir,
        });
    }

    // ── 整数通道：解析 → 域界 → 对齐（三级各留痕） ──────────────────
    /// 整数键值域断言。
    pub fn check_int(&mut self, key: &str, raw: &str) -> Result<CheckedInt, ValDomError> {
        self.tick_inc();
        self.checks_int = self.checks_int.wrapping_add(1);
        let idx = match int_index(key) {
            Some(i) => i,
            None => {
                let hint = hint_unknown(key);
                self.record_reject(key, raw, RejectDir::NoDir, ValDomError::UnknownKey, hint);
                return Err(ValDomError::UnknownKey);
            }
        };
        let spec = INT_TABLE[idx];
        // ── L1 解析（符号 + 十进制 + 显性饱和） ──
        let parsed = match parse_signed_u64(raw) {
            Ok(p) => p,
            Err(e) => {
                let t = Trace3 {
                    subject: spec.key,
                    s1: Stage::fail("L1 解析", e.code(), "符号/数字非法或空串"),
                    s2: Stage::skip("L2 域界"),
                    s3: Stage::skip("L3 对齐"),
                };
                self.remember(t);
                self.record_reject(key, raw, RejectDir::ParseFail, e, hint_int_parse());
                return Err(e);
            }
        };
        // 饱和解析留「过级」痕（文法本身合法），方向裁决交域界级：
        let s1 = if parsed.satur {
            Stage::pass("L1 解析", "文法命中（幅值溢出已显性饱和钳制）")
        } else {
            Stage::pass("L1 解析", "符号+十进制文法命中")
        };
        let value = to_i64_domain(&parsed);
        // ── L2 域界（min/max 闭区间；饱和值按符号必越一侧） ──
        if parsed.satur && parsed.negative {
            let t = Trace3 {
                subject: spec.key,
                s1,
                s2: Stage::fail("L2 域界", ValDomError::IntUnderLower.code(), "饱和钳制值低于下限"),
                s3: Stage::skip("L3 对齐"),
            };
            self.remember(t);
            let hint = hint_int_below(&spec, value);
            self.record_reject(key, raw, RejectDir::Below, ValDomError::IntUnderLower, hint);
            return Err(ValDomError::IntUnderLower);
        }
        if parsed.satur || value > spec.max {
            let t = Trace3 {
                subject: spec.key,
                s1,
                s2: Stage::fail("L2 域界", ValDomError::IntOverUpper.code(), "高于上限（饱和钳制值亦落此）"),
                s3: Stage::skip("L3 对齐"),
            };
            self.remember(t);
            let hint = hint_int_above(&spec, value);
            self.record_reject(key, raw, RejectDir::Above, ValDomError::IntOverUpper, hint);
            return Err(ValDomError::IntOverUpper);
        }
        if value < spec.min {
            let t = Trace3 {
                subject: spec.key,
                s1,
                s2: Stage::fail("L2 域界", ValDomError::IntUnderLower.code(), "低于下限（负号注入亦落此）"),
                s3: Stage::skip("L3 对齐"),
            };
            self.remember(t);
            let hint = hint_int_below(&spec, value);
            self.record_reject(key, raw, RejectDir::Below, ValDomError::IntUnderLower, hint);
            return Err(ValDomError::IntUnderLower);
        }
        let s2 = Stage::pass("L2 域界", "min/max 闭区间命中");
        // ── L3 对齐（自 min 起步长网格） ──
        let off = (value - spec.min) as u64;
        if off % spec.step != 0 {
            let t = Trace3 {
                subject: spec.key,
                s1,
                s2,
                s3: Stage::fail("L3 对齐", ValDomError::IntMisalign.code(), "不落步长网格"),
            };
            self.remember(t);
            let hint = hint_int_misalign(&spec, value);
            self.record_reject(key, raw, RejectDir::Misalign, ValDomError::IntMisalign, hint);
            return Err(ValDomError::IntMisalign);
        }
        let s3 = Stage::pass("L3 对齐", "步长网格命中");
        let t = Trace3 { subject: spec.key, s1, s2, s3 };
        self.remember(t);
        self.accepted = self.accepted.wrapping_add(1);
        self.events.push(Event { tick: self.ticks, kind: EventKind::Accept, code: 0, dir: RejectDir::NoDir });
        Ok(CheckedInt {
            key: spec.key,
            value,
            raw_len: raw.as_bytes().len(),
            unit: spec.unit,
            satur: parsed.satur,
            trace: t,
        })
    }

    // ── 字符串通道：长度 → 字符集 → 转义（三级各留痕） ───────────────
    /// 字符串键值域断言。
    pub fn check_str(&mut self, key: &str, raw: &str) -> Result<CheckedStr, ValDomError> {
        self.tick_inc();
        self.checks_str = self.checks_str.wrapping_add(1);
        let idx = match str_index(key) {
            Some(i) => i,
            None => {
                let hint = hint_unknown(key);
                self.record_reject(key, raw, RejectDir::NoDir, ValDomError::UnknownKey, hint);
                return Err(ValDomError::UnknownKey);
            }
        };
        let spec = STR_TABLE[idx];
        let b = raw.as_bytes();
        // ── L1 长度（空串拒收 + 字节预算） ──
        if b.is_empty() {
            let t = Trace3 {
                subject: spec.key,
                s1: Stage::fail("L1 长度", ValDomError::StrEmpty.code(), "空串"),
                s2: Stage::skip("L2 字符集"),
                s3: Stage::skip("L3 转义"),
            };
            self.remember(t);
            self.record_reject(key, raw, RejectDir::Empty, ValDomError::StrEmpty, hint_str_empty());
            return Err(ValDomError::StrEmpty);
        }
        if b.len() > spec.max_len as usize {
            let t = Trace3 {
                subject: spec.key,
                s1: Stage::fail("L1 长度", ValDomError::StrTooLong.code(), "超字节预算"),
                s2: Stage::skip("L2 字符集"),
                s3: Stage::skip("L3 转义"),
            };
            self.remember(t);
            let hint = hint_str_len(&spec, b.len());
            self.record_reject(key, raw, RejectDir::TooLong, ValDomError::StrTooLong, hint);
            return Err(ValDomError::StrTooLong);
        }
        let s1 = Stage::pass("L1 长度", "字节预算内");
        // ── L2 字符集（反斜杠元字符让渡 L3） ──
        let mut bad: Option<(usize, u8)> = None;
        let mut i = 0usize;
        while i < b.len() {
            let c = b[i];
            if c == b'\\' {
                i += 1;
                continue;
            }
            if !charset_has(spec.charset, c) {
                bad = Some((i, c));
                break;
            }
            i += 1;
        }
        if let Some((pos, byte)) = bad {
            let t = Trace3 {
                subject: spec.key,
                s1,
                s2: Stage::fail("L2 字符集", ValDomError::StrBadChar.code(), "闭集外字节"),
                s3: Stage::skip("L3 转义"),
            };
            self.remember(t);
            let hint = hint_str_char(&spec, pos, byte);
            self.record_reject(key, raw, RejectDir::BadChar, ValDomError::StrBadChar, hint);
            return Err(ValDomError::StrBadChar);
        }
        let s2 = Stage::pass("L2 字符集", "闭集命中（反斜杠让渡 L3）");
        // ── L3 转义合法性 ──
        match scan_escape(b) {
            Ok(logical) => {
                let s3 = Stage::pass("L3 转义", "转义闭集命中");
                let t = Trace3 { subject: spec.key, s1, s2, s3 };
                self.remember(t);
                self.accepted = self.accepted.wrapping_add(1);
                self.events.push(Event { tick: self.ticks, kind: EventKind::Accept, code: 0, dir: RejectDir::NoDir });
                Ok(CheckedStr { key: spec.key, raw_len: b.len(), logical_len: logical, trace: t })
            }
            Err(pos) => {
                let t = Trace3 {
                    subject: spec.key,
                    s1,
                    s2,
                    s3: Stage::fail("L3 转义", ValDomError::StrBadEscape.code(), "非法/残缺转义"),
                };
                self.remember(t);
                let hint = hint_str_escape(pos);
                self.record_reject(key, raw, RejectDir::BadEscape, ValDomError::StrBadEscape, hint);
                Err(ValDomError::StrBadEscape)
            }
        }
    }

    // ── 账面指纹（FNV-1a 64 于规范态串） ─────────────────────────────
    /// 规范态串（版本+计数+方向账+码账+拒收簿逐条；对拍与指纹共用）。
    pub fn canonical_state(&self) -> String {
        let mut s = String::new();
        s.push_str(VD_VERSION);
        s.push('|');
        s.push_str(&format!(
            "t{}/i{}/s{}/a{}/r{}",
            self.ticks, self.checks_int, self.checks_str, self.accepted, self.rejected
        ));
        s.push('|');
        let mut i = 0;
        while i < DIR_N {
            s.push_str(&format!(",{}", self.rej_by_dir[i]));
            i += 1;
        }
        s.push('|');
        let mut i = 0;
        while i < 16 {
            s.push_str(&format!(",{}", self.rej_by_code[i]));
            i += 1;
        }
        s.push('|');
        let n = self.ledger.len();
        let mut i = 0;
        while i < n {
            if let Some(e) = self.ledger.entry(i) {
                s.push_str(&format!(
                    "({};{};{};{};{};{})",
                    e.key, e.raw, e.dir.idx(), e.code, e.tick, e.truncated as u8
                ));
            }
            i += 1;
        }
        s
    }

    /// 账面指纹（FNV-1a 64 位）。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.canonical_state().as_bytes())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 注入面（判据点名：越域拒收率 100%（注入 4/4））
// ═══════════════════════════════════════════════════════════════════════

/// 注入用例。
#[derive(Clone, Debug)]
pub struct InjectCase {
    /// 用例名（中文）。
    pub name: &'static str,
    /// 注入键。
    pub key: &'static str,
    /// 注入原文。
    pub raw: &'static str,
    /// 期望拒收码。
    pub want: u16,
    /// 实得码（0=被收，即漏检）。
    pub got: u16,
    /// 是否检出（got == want）。
    pub caught: bool,
}

/// 注入面报告（4 用例闭集）。
#[derive(Clone, Debug)]
pub struct InjectReport {
    /// 四用例。
    pub cases: [InjectCase; 4],
    /// 检出数。
    pub caught: u8,
    /// 用例总数（恒 4）。
    pub total: u8,
}

impl InjectReport {
    /// 判据口径：4/4 全检出。
    pub fn all_caught(&self) -> bool {
        self.caught == self.total
    }

    /// 注入检出率 ×1000（100% = 1000‰）。
    pub fn permille(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        (self.caught as u64).wrapping_mul(REJECT_RATE_FULL) / (self.total as u64)
    }
}

/// 单注入执行（scratch 核上跑，主账面不受染）。
fn inject_one(scratch: &mut DomainChecker, name: &'static str, key: &'static str, raw: &'static str, want: ValDomError) -> InjectCase {
    let got = match key_kind(key) {
        KeyKind::Int => match scratch.check_int(key, raw) {
            Err(e) => e.code(),
            Ok(_) => 0,
        },
        KeyKind::Str => match scratch.check_str(key, raw) {
            Err(e) => e.code(),
            Ok(_) => 0,
        },
        KeyKind::Unknown => 0,
    };
    let caught = got == want.code();
    InjectCase { name, key, raw, want: want.code(), got, caught }
}

impl DomainChecker {
    /// 四注入全跑（scratch 核；主账面只留报告与事件）。
    pub fn injection_suite(&mut self) -> InjectReport {
        let mut scratch = DomainChecker::new();
        // 注 1：越上界——heap.mb 域顶 4096，注入 8192（判据示例值）。
        let c1 = inject_one(&mut scratch, "越上界", "heap.mb", "8192", ValDomError::IntOverUpper);
        // 注 2：越下界——loglevel 域底 0，注入 -3（负号注入落越下界码）。
        let c2 = inject_one(&mut scratch, "越下界", "loglevel", "-3", ValDomError::IntUnderLower);
        // 注 3：错对齐——heap.mb 步长 64，注入 100（域内但不落网格）。
        let c3 = inject_one(&mut scratch, "错对齐", "heap.mb", "100", ValDomError::IntMisalign);
        // 注 4：坏字符集——hostname 闭集外字节 $。
        let c4 = inject_one(&mut scratch, "坏字符集", "hostname", "node$$", ValDomError::StrBadChar);
        let cases = [c1, c2, c3, c4];
        let mut caught = 0u8;
        for c in &cases {
            if c.caught {
                caught += 1;
            }
        }
        let rep = InjectReport { cases, caught, total: 4 };
        self.tick_inc();
        self.last_inject = Some(rep.clone());
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::Inject,
            code: if rep.all_caught() { 0 } else { ValDomError::InjectMissed.code() },
            dir: RejectDir::NoDir,
        });
        rep
    }

    /// 注入闸：四注入须全检出（判据口径 4/4），否则 E_INJECT_MISSED 上账。
    pub fn inject_gate(&mut self) -> Result<InjectReport, ValDomError> {
        let rep = self.injection_suite();
        self.tick_inc();
        if rep.all_caught() {
            self.events.push(Event { tick: self.ticks, kind: EventKind::Gate, code: 0, dir: RejectDir::NoDir });
            Ok(rep)
        } else {
            self.events.push(Event {
                tick: self.ticks,
                kind: EventKind::Gate,
                code: ValDomError::InjectMissed.code(),
                dir: RejectDir::NoDir,
            });
            Err(ValDomError::InjectMissed)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 缺省联动对账（域有缺省无 / 缺省有域无 / 单位不符 / 缺省值自身越域）
// ═══════════════════════════════════════════════════════════════════════

/// 对账报告（四类失配分列；ok = 四列全空）。
#[derive(Clone, Debug)]
pub struct ReconReport {
    /// 域有缺省无（域表键在缺省表无行）。
    pub missing: Vec<String>,
    /// 缺省有域无（缺省表行键不属任何域表）。
    pub orphan: Vec<String>,
    /// 单位不符（整数键缺省行单位与域表 unit 不全等）。
    pub unit_bad: Vec<String>,
    /// 缺省值自身越域（scratch 复检不过）。
    pub def_invalid: Vec<String>,
    /// 对账干净。
    pub ok: bool,
}

impl DomainChecker {
    /// 缺省联动对账（外部行集；测试注入失配行用）。
    pub fn reconcile(&mut self, rows: &[DefRow]) -> ReconReport {
        self.tick_inc();
        let mut rep = ReconReport {
            missing: Vec::new(),
            orphan: Vec::new(),
            unit_bad: Vec::new(),
            def_invalid: Vec::new(),
            ok: false,
        };
        // 1) 域表 → 缺省表：每键须有行；整数键另对单位。
        let mut i = 0;
        while i < INT_N {
            let k = INT_TABLE[i].key;
            match find_def(rows, k) {
                None => rep.missing.push(format!("域有缺省无：{}", k)),
                Some(r) => {
                    if r.unit != INT_TABLE[i].unit {
                        rep.unit_bad.push(format!(
                            "单位不符：{} 域表「{}」缺省表「{}」",
                            k, INT_TABLE[i].unit, r.unit
                        ));
                    }
                }
            }
            i += 1;
        }
        let mut i2 = 0;
        while i2 < STR_N {
            let k = STR_TABLE[i2].key;
            if find_def(rows, k).is_none() {
                rep.missing.push(format!("域有缺省无：{}", k));
            }
            i2 += 1;
        }
        // 2) 缺省表 → 域表：每行键须属某张域表（缺省有域无）。
        for r in rows {
            if matches!(key_kind(r.key), KeyKind::Unknown) {
                rep.orphan.push(format!("缺省有域无：{}", r.key));
            }
        }
        // 3) 缺省值自身过域（scratch 复检；缺省值也必须站得住）。
        let mut scratch = DomainChecker::new();
        for r in rows {
            let res = match key_kind(r.key) {
                KeyKind::Int => match scratch.check_int(r.key, r.val) {
                    Ok(_) => None,
                    Err(e) => Some(e),
                },
                KeyKind::Str => match scratch.check_str(r.key, r.val) {
                    Ok(_) => None,
                    Err(e) => Some(e),
                },
                KeyKind::Unknown => None,
            };
            if let Some(e) = res {
                rep.def_invalid.push(format!("缺省越域：{} = {}（{}）", r.key, r.val, e.detail()));
            }
        }
        rep.ok = rep.missing.is_empty()
            && rep.orphan.is_empty()
            && rep.unit_bad.is_empty()
            && rep.def_invalid.is_empty();
        self.tick_inc();
        self.last_recon = Some(rep.clone());
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::Recon,
            code: if rep.ok { 0 } else { ValDomError::DefaultMissing.code() },
            dir: RejectDir::NoDir,
        });
        rep
    }

    /// 内建缺省表对账（无参场景口径）。
    pub fn reconcile_builtin(&mut self) -> ReconReport {
        self.reconcile(&DEFAULTS)
    }

    /// 对账闸：干净则 Ok，失配按类上码（missing→A，orphan→B，他类→单位/值不符）。
    pub fn reconcile_gate(&mut self, rows: &[DefRow]) -> Result<ReconReport, ValDomError> {
        let rep = self.reconcile(rows);
        if rep.ok {
            return Ok(rep);
        }
        if !rep.missing.is_empty() {
            return Err(ValDomError::DefaultMissing);
        }
        if !rep.orphan.is_empty() {
            return Err(ValDomError::DefaultOrphan);
        }
        Err(ValDomError::UnitMismatch)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 双跑确定性对拍（同批次两独立跑：指纹全等 + 逐项结果全等）
// ═══════════════════════════════════════════════════════════════════════

/// 对拍基准批（12 项：8 收 4 拒，覆盖两通道与四注入同类拒绝）。
pub const DUAL_BATCH: [(&str, &str); 12] = [
    ("heap.mb", "512"),
    ("loglevel", "6"),
    ("hostname", "varix"),
    ("heap.mb", "8192"),
    ("loglevel", "-3"),
    ("heap.mb", "100"),
    ("hostname", "node$$"),
    ("console.dev", "ttyS0"),
    ("boot.id", "0d23d90010"),
    ("uart.baud", "115200"),
    ("stack.kb", "16"),
    ("video.mode", "1024x768"),
];

/// 对拍基准期望向量（0=收，余为诊断码；与 DUAL_BATCH 逐项对应）。
pub const DUAL_EXPECTED: [u16; 12] = [
    0,
    0,
    0,
    ValDomError::IntOverUpper.code(),
    ValDomError::IntUnderLower.code(),
    ValDomError::IntMisalign.code(),
    ValDomError::StrBadChar.code(),
    0,
    0,
    0,
    0,
    0,
];

/// 对拍报告。
#[derive(Clone, Debug)]
pub struct DualReport {
    /// A 跑指纹。
    pub fp_a: u64,
    /// B 跑指纹。
    pub fp_b: u64,
    /// A 跑逐项结果（0=收，余为诊断码）。
    pub outcomes_a: Vec<u16>,
    /// B 跑逐项结果。
    pub outcomes_b: Vec<u16>,
    /// 两跑全等（指纹等 + 逐项等）。
    pub identical: bool,
    /// 批次项数。
    pub items: usize,
}

/// 单项执行（按键型分派；未知键出未知码且不占核受理数）。
fn run_one(c: &mut DomainChecker, k: &str, v: &str) -> u16 {
    match key_kind(k) {
        KeyKind::Int => match c.check_int(k, v) {
            Ok(_) => 0,
            Err(e) => e.code(),
        },
        KeyKind::Str => match c.check_str(k, v) {
            Ok(_) => 0,
            Err(e) => e.code(),
        },
        KeyKind::Unknown => ValDomError::UnknownKey.code(),
    }
}

impl DomainChecker {
    /// 双跑对拍（批次为空不构成对拍 → E_FP_MISmatch 口径上账）。
    pub fn dual_run(&mut self, batch: &[(&str, &str)]) -> Result<DualReport, ValDomError> {
        if batch.is_empty() {
            return Err(ValDomError::FpMismatch);
        }
        let mut a = DomainChecker::new();
        let mut b = DomainChecker::new();
        let mut oa: Vec<u16> = Vec::new();
        let mut ob: Vec<u16> = Vec::new();
        for item in batch {
            let (k, v) = *item;
            oa.push(run_one(&mut a, k, v));
            ob.push(run_one(&mut b, k, v));
        }
        let fa = a.fingerprint();
        let fb = b.fingerprint();
        let identical = fa == fb && oa == ob;
        let rep = DualReport {
            fp_a: fa,
            fp_b: fb,
            outcomes_a: oa,
            outcomes_b: ob,
            identical,
            items: batch.len(),
        };
        self.tick_inc();
        self.last_dual = Some(rep.clone());
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::Dual,
            code: if rep.identical { 0 } else { ValDomError::FpMismatch.code() },
            dir: RejectDir::NoDir,
        });
        Ok(rep)
    }

    /// 对拍闸：两跑须全等。
    pub fn dual_gate(&mut self, batch: &[(&str, &str)]) -> Result<DualReport, ValDomError> {
        let rep = self.dual_run(batch)?;
        self.tick_inc();
        if rep.identical {
            self.events.push(Event { tick: self.ticks, kind: EventKind::Gate, code: 0, dir: RejectDir::NoDir });
            Ok(rep)
        } else {
            self.events.push(Event {
                tick: self.ticks,
                kind: EventKind::Gate,
                code: ValDomError::FpMismatch.code(),
                dir: RejectDir::NoDir,
            });
            Err(ValDomError::FpMismatch)
        }
    }

    /// 基准对拍闸：双跑全等之外，逐项结果还须与冻结期望向量全等
    /// （表驱动路径或拒收码漂移即破）。
    pub fn canonical_baseline_gate(&mut self) -> Result<(DualReport, bool), ValDomError> {
        let rep = self.dual_run(&DUAL_BATCH)?;
        let mut hit = rep.outcomes_a.len() == DUAL_EXPECTED.len();
        let mut i = 0;
        while hit && i < rep.outcomes_a.len() {
            if rep.outcomes_a[i] != DUAL_EXPECTED[i] {
                hit = false;
            }
            i += 1;
        }
        self.tick_inc();
        if rep.identical && hit {
            self.events.push(Event { tick: self.ticks, kind: EventKind::Gate, code: 0, dir: RejectDir::NoDir });
            Ok((rep, true))
        } else {
            self.events.push(Event {
                tick: self.ticks,
                kind: EventKind::Gate,
                code: ValDomError::FpMismatch.code(),
                dir: RejectDir::NoDir,
            });
            Err(ValDomError::FpMismatch)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 自审与健康（账目恒等式 + 级次有序性 → 健康分 0~1000）
// ═══════════════════════════════════════════════════════════════════════

/// 健康报告。
#[derive(Clone, Debug)]
pub struct Health {
    /// 健康分（0~1000）。
    pub score: u32,
    /// 判词（绿/黄/红）。
    pub verdict: &'static str,
    /// 注入面 4/4。
    pub inject_ok: bool,
    /// 对账干净。
    pub recon_ok: bool,
    /// 拒收簿未溢出。
    pub ledger_ok: bool,
    /// 自审通过。
    pub audit_ok: bool,
}

impl DomainChecker {
    /// 账目自审：
    ///   恒等式 1：检查数（整数+字符串）= 收 + 拒；
    ///   恒等式 2：方向账之和 = 拒收总数；
    ///   级次性：近期留痕逐条级次有序（失败短路、不许越级）。
    pub fn audit_ok(&self) -> bool {
        if self.checks_int.wrapping_add(self.checks_str) != self.accepted.wrapping_add(self.rejected) {
            return false;
        }
        let mut sum = 0u64;
        let mut i = 0;
        while i < DIR_N {
            sum = sum.wrapping_add(self.rej_by_dir[i]);
            i += 1;
        }
        if sum != self.rejected {
            return false;
        }
        for t in &self.recent_traces {
            if !t.ordered() {
                return false;
            }
        }
        true
    }

    /// 自审闸：破缺 → E_TRACE_BROKEN 上账。
    pub fn audit_gate(&self) -> Result<(), ValDomError> {
        if self.audit_ok() {
            Ok(())
        } else {
            Err(ValDomError::TraceBroken)
        }
    }

    /// 健康分：基准 1000；注入缺/败 -400，对账缺/败 -300，
    /// 簿溢出 -150，自审败 -150（全 saturating，不分既不给绿）。
    pub fn health(&self) -> Health {
        let audit_ok = self.audit_ok();
        let inject_ok = match &self.last_inject {
            Some(r) => r.all_caught(),
            None => false,
        };
        let recon_ok = match &self.last_recon {
            Some(r) => r.ok,
            None => false,
        };
        let ledger_ok = self.ledger.overflow == 0;
        let mut score: u32 = 1000;
        if !inject_ok {
            score = score.saturating_sub(400);
        }
        if !recon_ok {
            score = score.saturating_sub(300);
        }
        if !ledger_ok {
            score = score.saturating_sub(150);
        }
        if !audit_ok {
            score = score.saturating_sub(150);
        }
        let verdict = if score >= 900 {
            "绿：域核账齐"
        } else if score >= 600 {
            "黄：账面有瑕"
        } else {
            "红：域核失守"
        };
        Health { score, verdict, inject_ok, recon_ok, ledger_ok, audit_ok }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 渲染面（账单 / 拒收簿 / 三级留痕）
// ═══════════════════════════════════════════════════════════════════════

/// 三级流水简介（观测口径冻结文案）。
pub fn pipeline_brief() -> &'static str {
    "整数三级：L1 解析（符号+十进制+显性饱和）→ L2 域界（min/max）→ L3 对齐（自 min 起步长网格）；字符串三级：L1 长度（字节预算+空串拒收）→ L2 字符集（闭集）→ L3 转义（\\\\ \\\" \\n \\t \\xHH 闭集）"
}

/// 单级留痕行。
fn stage_line(st: &Stage) -> String {
    if !st.ran {
        format!("[跳] {}", st.name)
    } else if st.ok {
        format!("[过] {} · {}", st.name, st.note)
    } else {
        format!("[拒] {} · 0x{:04X} · {}", st.name, st.code, st.note)
    }
}

impl DomainChecker {
    /// 值域核账单（判据账齐渲染）。
    pub fn render_report(&self) -> String {
        let mut s = String::new();
        s.push_str("UNX-F0586 参数值域核 · 账单\n");
        s.push_str(&format!(
            "版本 {} · 诊断段 0x{:04X}~0x{:04X}\n",
            VD_VERSION, VD_SEG_BASE, VD_SEG_TOP
        ));
        s.push_str(&format!(
            "整数域表 {} 键 · 字符串域长表 {} 键 · 缺省表 {} 行\n",
            INT_N, STR_N, DEF_N
        ));
        s.push_str(pipeline_brief());
        s.push('\n');
        s.push_str(&format!(
            "受理 整数 {} / 字符串 {} · 收 {} · 拒 {} · 拒收率 {}‰\n",
            self.checks_int,
            self.checks_str,
            self.accepted,
            self.rejected,
            self.rejection_permille()
        ));
        s.push_str("方向账：");
        let mut i = 0;
        while i < DIR_N {
            s.push_str(&format!("{}={}", dir_name(i), self.rej_by_dir[i]));
            if i + 1 < DIR_N {
                s.push_str(" · ");
            }
            i += 1;
        }
        s.push('\n');
        if let Some(r) = &self.last_inject {
            s.push_str(&format!(
                "注入面 {}/{} = {}‰（判据：100%）\n",
                r.caught,
                r.total,
                r.permille()
            ));
        } else {
            s.push_str("注入面 未跑\n");
        }
        if let Some(r) = &self.last_recon {
            s.push_str(&format!(
                "缺省对账 ok={} 失配={}（域有缺省无） 孤行={}（缺省有域无） 单位={} 缺省越域={}\n",
                r.ok,
                r.missing.len(),
                r.orphan.len(),
                r.unit_bad.len(),
                r.def_invalid.len()
            ));
        } else {
            s.push_str("缺省对账 未跑\n");
        }
        if let Some(d) = &self.last_dual {
            s.push_str(&format!(
                "双跑对拍 identical={} fpA={:016X} fpB={:016X} 批次 {} 项\n",
                d.identical, d.fp_a, d.fp_b, d.items
            ));
        } else {
            s.push_str("双跑对拍 未跑\n");
        }
        let h = self.health();
        s.push_str(&format!("健康 {} / 1000 · {}\n", h.score, h.verdict));
        s.push_str(&format!("账面指纹 {:016X}\n", self.fingerprint()));
        s
    }

    /// 拒收簿渲染（原文/方向/码/建议四列）。
    pub fn render_ledger(&self) -> String {
        let mut s = String::new();
        s.push_str("越域拒收簿：\n");
        let n = self.ledger.len();
        let mut i = 0;
        while i < n {
            if let Some(e) = self.ledger.entry(i) {
                s.push_str(&format!(
                    "[{}] tick={} 码 0x{:04X} 方向 {} 键「{}」原文「{}」{}\n",
                    i,
                    e.tick,
                    e.code,
                    e.dir.text(),
                    e.key,
                    e.raw,
                    if e.truncated { "（截断保存）" } else { "" }
                ));
                s.push_str(&format!("    建议：{}\n", e.hint));
            }
            i += 1;
        }
        if n == 0 {
            s.push_str("（空）\n");
        }
        if self.ledger.overflow > 0 {
            s.push_str(&format!("簿满溢出 {} 次（拒收账不受影响）\n", self.ledger.overflow));
        }
        s
    }

    /// 近期三级留痕渲染（新→旧）。
    pub fn render_traces(&self) -> String {
        let mut s = String::new();
        s.push_str("近期三级留痕（新→旧，上限 8）：\n");
        let n = self.recent_traces.len();
        let mut i = n;
        while i > 0 {
            i -= 1;
            let t = &self.recent_traces[i];
            s.push_str(&format!("· 键 {}\n", t.subject));
            s.push_str(&format!("  {}\n", stage_line(&t.s1)));
            s.push_str(&format!("  {}\n", stage_line(&t.s2)));
            s.push_str(&format!("  {}\n", stage_line(&t.s3)));
        }
        if n == 0 {
            s.push_str("（空）\n");
        }
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 冒烟测试（cfg(test)：四注入 4/4 / 边界值 / 缺省对账 / 双跑一致 /
// 建议文案 / 解析边角 / 截断保存 / 簿满溢出 / FNV 公开向量 / 健康分）
// 测试体只用 ? 与 assert!，零 unwrap/expect。
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 期望拒收的整数通道断言（检出=true）。
    fn expect_err_int(d: &mut DomainChecker, key: &str, raw: &str, want: ValDomError) -> bool {
        match d.check_int(key, raw) {
            Err(e) => e.code() == want.code(),
            Ok(_) => false,
        }
    }

    /// 期望拒收的字符串通道断言（检出=true）。
    fn expect_err_str(d: &mut DomainChecker, key: &str, raw: &str, want: ValDomError) -> bool {
        match d.check_str(key, raw) {
            Err(e) => e.code() == want.code(),
            Ok(_) => false,
        }
    }

    #[test]
    fn const_tables_frozen() {
        assert!(INT_N == 8 && STR_N == 8 && DEF_N == 16);
        assert!(VD_SEG_BASE == 0x1490 && VD_SEG_TOP == 0x149F);
        assert!(LEDGER_CAP == 16 && EVENT_CAP == 32 && RECENT_CAP == 8);
        assert!(REJECT_RATE_FULL == 1_000);
    }

    #[test]
    fn injection_4_of_4() {
        let mut d = DomainChecker::new();
        let rep = d.injection_suite();
        assert_eq!(rep.caught, 4);
        assert_eq!(rep.total, 4);
        assert!(rep.all_caught());
        assert_eq!(rep.permille(), 1_000);
        // 注入跑在 scratch 上：主账面零染（只留报告与事件）
        assert_eq!(d.checks_int, 0);
        assert_eq!(d.checks_str, 0);
        assert_eq!(d.rejected, 0);
        // 逐用例码位核对
        assert!(rep.cases[0].caught && rep.cases[0].want == ValDomError::IntOverUpper.code());
        assert!(rep.cases[1].caught && rep.cases[1].want == ValDomError::IntUnderLower.code());
        assert!(rep.cases[2].caught && rep.cases[2].want == ValDomError::IntMisalign.code());
        assert!(rep.cases[3].caught && rep.cases[3].want == ValDomError::StrBadChar.code());
    }

    #[test]
    fn inject_gate_pass() -> Result<(), ValDomError> {
        let mut d = DomainChecker::new();
        let rep = d.inject_gate()?;
        assert_eq!(rep.caught, 4);
        Ok(())
    }

    #[test]
    fn int_boundaries() -> Result<(), ValDomError> {
        let mut d = DomainChecker::new();
        // 域底 / 网格中档 / 域顶全收
        let lo = d.check_int("heap.mb", "64")?;
        assert_eq!(lo.value, 64);
        assert_eq!(lo.unit, "MB");
        let grid = d.check_int("heap.mb", "4032")?;
        assert_eq!(grid.value, 4032);
        let hi = d.check_int("heap.mb", "4096")?;
        assert_eq!(hi.value, 4096);
        // 越下界（域底 -1）
        assert!(expect_err_int(&mut d, "heap.mb", "63", ValDomError::IntUnderLower));
        // 越上界（域顶 +64）
        assert!(expect_err_int(&mut d, "heap.mb", "4160", ValDomError::IntOverUpper));
        // 错对齐（域内不落网格）
        assert!(expect_err_int(&mut d, "heap.mb", "100", ValDomError::IntMisalign));
        assert!(expect_err_int(&mut d, "heap.mb", "129", ValDomError::IntMisalign));
        // 负号注入（域底 0 的 loglevel）
        assert!(expect_err_int(&mut d, "loglevel", "-3", ValDomError::IntUnderLower));
        // loglevel 域顶 7 收、8 拒
        assert!(d.check_int("loglevel", "7").is_ok());
        assert!(expect_err_int(&mut d, "loglevel", "8", ValDomError::IntOverUpper));
        // 留痕级次全有序
        for t in &d.recent_traces {
            assert!(t.ordered());
        }
        // 自审恒等式成立
        d.audit_gate()?;
        Ok(())
    }

    #[test]
    fn string_checks() -> Result<(), ValDomError> {
        let mut d = DomainChecker::new();
        // 常规收值 + 逻辑长
        let ok = d.check_str("hostname", "varix-node-1")?;
        assert_eq!(ok.logical_len, 12);
        // 长度边界：恰好 32 收
        let edge = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; // 32 字节
        d.check_str("hostname", edge)?;
        // 33 字节拒（超长）
        assert!(expect_err_str(
            &mut d,
            "hostname",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ValDomError::StrTooLong
        ));
        // 空串拒
        assert!(expect_err_str(&mut d, "hostname", "", ValDomError::StrEmpty));
        // 坏字符集（$ 闭集外）
        assert!(expect_err_str(&mut d, "hostname", "node$", ValDomError::StrBadChar));
        // 合法转义 \n：raw 6 字节 → 逻辑 5
        let esc = d.check_str("log.tag", "boot\\n")?;
        assert_eq!(esc.raw_len, 6);
        assert_eq!(esc.logical_len, 5);
        // 合法转义 \xHH（大写十六进制亦收）：t \x4F g → 逻辑 3
        let hu = d.check_str("log.tag", "t\\x4Fg")?;
        assert_eq!(hu.logical_len, 3);
        // 非法转义 \q
        assert!(expect_err_str(&mut d, "log.tag", "boot\\q", ValDomError::StrBadEscape));
        // 残缺转义（孤立反斜杠收笔）
        assert!(expect_err_str(&mut d, "log.tag", "boot\\", ValDomError::StrBadEscape));
        // \x 残缺（只给一位）
        assert!(expect_err_str(&mut d, "log.tag", "t\\x4", ValDomError::StrBadEscape));
        // \x 位非法（g 非十六进制）
        assert!(expect_err_str(&mut d, "log.tag", "t\\xgg", ValDomError::StrBadEscape));
        // 合法自转义 \\\\
        let dd = d.check_str("log.tag", "a\\\\b")?;
        assert_eq!(dd.logical_len, 3);
        // 留痕级次全有序
        for t in &d.recent_traces {
            assert!(t.ordered());
        }
        d.audit_gate()?;
        Ok(())
    }

    #[test]
    fn parse_edges() -> Result<(), ValDomError> {
        let mut d = DomainChecker::new();
        // 空串 / 纯符号 / 夹字 / 前导空白 全拒于解析级
        assert!(expect_err_int(&mut d, "loglevel", "", ValDomError::IntParse));
        assert!(expect_err_int(&mut d, "loglevel", "+", ValDomError::IntParse));
        assert!(expect_err_int(&mut d, "loglevel", "12a", ValDomError::IntParse));
        assert!(expect_err_int(&mut d, "loglevel", " 6", ValDomError::IntParse));
        // 幅值溢出：显性饱和 → 域界级必拒（uart.baud 上限 4M）
        assert!(expect_err_int(
            &mut d,
            "uart.baud",
            "99999999999999999999999",
            ValDomError::IntOverUpper
        ));
        // 负幅值溢出：钳 i64::MIN → 越下界
        assert!(expect_err_int(
            &mut d,
            "uart.baud",
            "-99999999999999999999999",
            ValDomError::IntUnderLower
        ));
        // 前导 + 合法
        let mut d2 = DomainChecker::new();
        let c = d2.check_int("loglevel", "+6")?;
        assert_eq!(c.value, 6);
        assert!(!c.satur);
        Ok(())
    }

    #[test]
    fn unknown_key_rejected() {
        let mut d = DomainChecker::new();
        assert!(matches!(d.check_int("nope", "1"), Err(ValDomError::UnknownKey)));
        assert!(matches!(d.check_str("nope", "x"), Err(ValDomError::UnknownKey)));
        assert!(matches!(default_of("nope"), Err(ValDomError::DefaultMissing)));
    }

    #[test]
    fn reconcile_builtin_clean() {
        let mut d = DomainChecker::new();
        let rep = d.reconcile_builtin();
        assert!(rep.ok);
        assert!(rep.missing.is_empty());
        assert!(rep.orphan.is_empty());
        assert!(rep.unit_bad.is_empty());
        assert!(rep.def_invalid.is_empty());
        // 内建对账闸应过
        assert!(d.reconcile_gate(&DEFAULTS).is_ok());
    }

    #[test]
    fn reconcile_mismatch_detected() {
        let mut d = DomainChecker::new();
        let rows = [
            DefRow { key: "heap.mb", val: "512", unit: "MB" },
            // 故意漏 loglevel 等 → 域有缺省无
            DefRow { key: "hostname", val: "varix", unit: "" },
            DefRow { key: "ghost.key", val: "1", unit: "" }, // 缺省有域无
            DefRow { key: "uart.baud", val: "115200", unit: "波特每秒" }, // 单位不符
        ];
        let rep = d.reconcile(&rows);
        assert!(!rep.ok);
        // 域有缺省无：16 键中缺 13（16 - heap.mb/hostname/uart.baud）
        assert_eq!(rep.missing.len(), 13);
        // 缺省有域无：ghost.key 恰一
        assert_eq!(rep.orphan.len(), 1);
        // 单位不符：uart.baud 恰一
        assert_eq!(rep.unit_bad.len(), 1);
        // ghost.key 不属域表 → 不做缺省越域复检；其余三行缺省值全过域
        assert!(rep.def_invalid.is_empty());
        // 对账闸按类上码：missing 优先
        assert!(matches!(d.reconcile_gate(&rows), Err(ValDomError::DefaultMissing)));
        // 变体：全键齐 + 一条孤儿行 → DefaultOrphan（missing 先判，故须全键齐）
        let mut rows2: Vec<DefRow> = Vec::new();
        for r in DEFAULTS.iter() {
            rows2.push(*r);
        }
        rows2.push(DefRow { key: "ghost.key", val: "1", unit: "" });
        assert!(matches!(d.reconcile_gate(&rows2), Err(ValDomError::DefaultOrphan)));
        // 变体：全键齐但 uart.baud 单位错 → UnitMismatch
        let mut rows3: Vec<DefRow> = Vec::new();
        for r in DEFAULTS.iter() {
            if r.key == "uart.baud" {
                rows3.push(DefRow { key: r.key, val: r.val, unit: "波特每秒" });
            } else {
                rows3.push(*r);
            }
        }
        assert!(matches!(d.reconcile_gate(&rows3), Err(ValDomError::UnitMismatch)));
        // 变体：缺省值自身越域（heap.mb 缺省 8192 越上界）→ UnitMismatch
        let mut rows4: Vec<DefRow> = Vec::new();
        for r in DEFAULTS.iter() {
            if r.key == "heap.mb" {
                rows4.push(DefRow { key: r.key, val: "8192", unit: r.unit });
            } else {
                rows4.push(*r);
            }
        }
        let rep4 = d.reconcile(&rows4);
        assert_eq!(rep4.def_invalid.len(), 1);
        assert!(rep4.def_invalid[0].contains("越上界"));
    }

    #[test]
    fn dual_run_identical() -> Result<(), ValDomError> {
        let mut d = DomainChecker::new();
        let rep = d.dual_gate(&DUAL_BATCH)?;
        assert!(rep.identical);
        assert_eq!(rep.fp_a, rep.fp_b);
        assert_eq!(rep.outcomes_a.len(), 12);
        assert_eq!(rep.outcomes_b.len(), 12);
        // 逐项两跑一致
        for i in 0..rep.outcomes_a.len() {
            assert_eq!(rep.outcomes_a[i], rep.outcomes_b[i]);
        }
        Ok(())
    }

    #[test]
    fn canonical_baseline_hit() -> Result<(), ValDomError> {
        let mut d = DomainChecker::new();
        let (rep, hit) = d.canonical_baseline_gate()?;
        assert!(hit);
        assert!(rep.identical);
        // 期望向量与冻结表口径一致
        assert_eq!(rep.outcomes_a.len(), DUAL_EXPECTED.len());
        for i in 0..DUAL_EXPECTED.len() {
            assert_eq!(rep.outcomes_a[i], DUAL_EXPECTED[i]);
        }
        Ok(())
    }

    #[test]
    fn empty_batch_not_dualed() {
        let mut d = DomainChecker::new();
        let empty: [(&str, &str); 0] = [];
        assert!(matches!(d.dual_run(&empty), Err(ValDomError::FpMismatch)));
    }

    #[test]
    fn suggestion_text() {
        // 越上界建议含判据示例文案「该键合法域 [64, 4096] …实得 8192」
        let mut d = DomainChecker::new();
        assert!(expect_err_int(&mut d, "heap.mb", "8192", ValDomError::IntOverUpper));
        assert_eq!(d.ledger.len(), 1);
        let hint = match d.ledger.entry(0) {
            Some(e) => e.hint.clone(),
            None => String::new(),
        };
        assert!(hint.contains("合法域 [64, 4096]"));
        assert!(hint.contains("实得 8192"));
        assert!(hint.contains("域顶 4096"));
        // 错对齐建议含向下取整合法值（100 → 64）
        let mut d2 = DomainChecker::new();
        assert!(expect_err_int(&mut d2, "heap.mb", "100", ValDomError::IntMisalign));
        let hint2 = match d2.ledger.entry(0) {
            Some(e) => e.hint.clone(),
            None => String::new(),
        };
        assert!(hint2.contains("步长 64"));
        assert!(hint2.contains("向下取整建议 64"));
        // 字符集外建议含首犯字节与闭集名
        let mut d3 = DomainChecker::new();
        assert!(expect_err_str(&mut d3, "hostname", "node$", ValDomError::StrBadChar));
        let hint3 = match d3.ledger.entry(0) {
            Some(e) => e.hint.clone(),
            None => String::new(),
        };
        assert!(hint3.contains("0x24"));
        assert!(hint3.contains("主机名闭集"));
        // 域摘要面
        let s = match domain_summary_int("heap.mb") {
            Ok(x) => x,
            Err(_) => String::new(),
        };
        assert!(s.contains("[64, 4096]"));
        assert!(s.contains("步长 64"));
        let t = match domain_summary_str("hostname") {
            Ok(x) => x,
            Err(_) => String::new(),
        };
        assert!(t.contains("32 字节"));
        assert!(matches!(domain_summary_int("ghost.key"), Err(ValDomError::UnknownKey)));
    }

    #[test]
    fn ledger_truncation() {
        let mut d = DomainChecker::new();
        // 41 字节原文（hostname 上限 32）→ 超长拒 + 截断保存
        let long = "v0123456789012345678901234567890123456789";
        assert!(expect_err_str(&mut d, "hostname", long, ValDomError::StrTooLong));
        assert_eq!(d.ledger.len(), 1);
        assert!(matches!(d.ledger.entry(0), Some(e) if e.truncated));
        let raw = match d.ledger.entry(0) {
            Some(e) => e.raw.clone(),
            None => String::new(),
        };
        assert!(raw.contains("…(截)"));
        // 24 字节预算 + 4 字符尾注
        assert!(raw.chars().count() <= RAW_KEEP + 4);
        // 未超预算原文不截断
        let mut d2 = DomainChecker::new();
        assert!(expect_err_str(&mut d2, "hostname", "node$", ValDomError::StrBadChar));
        assert!(matches!(d2.ledger.entry(0), Some(e) if !e.truncated));
    }

    #[test]
    fn ledger_overflow_counted() {
        let mut d = DomainChecker::new();
        let mut i = 0;
        while i < LEDGER_CAP + 3 {
            let _ = expect_err_int(&mut d, "loglevel", "-1", ValDomError::IntUnderLower);
            i += 1;
        }
        // 簿 16 槽封顶；溢出 3 计数；拒收账不受簿满影响
        assert_eq!(d.ledger.len(), LEDGER_CAP);
        assert_eq!(d.ledger.overflow, 3);
        assert_eq!(d.rejected, (LEDGER_CAP + 3) as u64);
        // 码账与方向账恒等式仍成立
        assert!(d.audit_gate().is_ok());
    }

    #[test]
    fn rejection_rate_accounting() {
        let mut d = DomainChecker::new();
        // 注入面 scratch 4/4 = 1000‰
        let rep = d.injection_suite();
        assert_eq!(rep.permille(), 1_000);
        // 主账面：3 收 3 拒 → 500‰；再补 1 收 1 拒 → 6 检 4 拒 = 666‰? 精算：
        // 3 收（512/1024/varix）+ 3 拒（-9/9999/bad char$）= 6 检 3 拒 = 500‰
        let _ = d.check_int("heap.mb", "512");
        let _ = d.check_int("heap.mb", "1024");
        let _ = d.check_str("hostname", "varix");
        let _ = d.check_int("loglevel", "-9");
        let _ = d.check_int("heap.mb", "9999");
        let _ = d.check_str("hostname", "bad char$");
        assert_eq!(d.accepted, 3);
        assert_eq!(d.rejected, 3);
        assert_eq!(d.rejection_permille(), 500);
        // 方向账逐向核对
        assert_eq!(d.rej_by_dir[RejectDir::Below.idx()], 1);
        assert_eq!(d.rej_by_dir[RejectDir::Above.idx()], 1);
        assert_eq!(d.rej_by_dir[RejectDir::BadChar.idx()], 1);
        // 码账逐码核对
        assert_eq!(d.rej_by_code[0], 0); // UnknownKey 零
        assert_eq!(d.rej_by_code[(ValDomError::IntUnderLower.code() - VD_SEG_BASE) as usize], 1);
        assert_eq!(d.rej_by_code[(ValDomError::IntOverUpper.code() - VD_SEG_BASE) as usize], 1);
        assert_eq!(d.rej_by_code[(ValDomError::StrBadChar.code() - VD_SEG_BASE) as usize], 1);
        // 自审过闸
        assert!(d.audit_gate().is_ok());
    }

    #[test]
    fn event_ring_records() {
        let mut d = DomainChecker::new();
        let _ = d.check_int("heap.mb", "64");
        let _ = d.check_int("heap.mb", "100"); // 错对齐
        assert!(d.events.len() >= 2);
        let mut accept = 0;
        let mut reject = 0;
        for e in d.events.all() {
            match e.kind {
                EventKind::Accept => accept += 1,
                EventKind::Reject => reject += 1,
                _ => {}
            }
        }
        assert!(accept >= 1);
        assert!(reject >= 1);
    }

    #[test]
    fn fnv_known_vectors() {
        // FNV-1a 64 公开测试向量（Landon Curt Noll 参考实现口径）
        assert_eq!(fnv64(b""), FNV_OFFSET);
        assert_eq!(fnv64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn fingerprint_reflects_state() {
        let mut a = DomainChecker::new();
        let mut b = DomainChecker::new();
        // 同输入同指纹
        let _ = a.check_int("heap.mb", "512");
        let _ = b.check_int("heap.mb", "512");
        assert_eq!(a.fingerprint(), b.fingerprint());
        // 异输入异指纹
        let _ = a.check_int("heap.mb", "576");
        assert!(a.fingerprint() != b.fingerprint());
    }

    #[test]
    fn health_green_after_gates() -> Result<(), ValDomError> {
        let mut d = DomainChecker::new();
        let inj = d.inject_gate()?;
        assert!(inj.all_caught());
        let rec = d.reconcile_builtin();
        assert!(rec.ok);
        let dual = d.dual_gate(&DUAL_BATCH)?;
        assert!(dual.identical);
        d.audit_gate()?;
        let h = d.health();
        assert!(h.score >= 900);
        assert!(h.inject_ok);
        assert!(h.recon_ok);
        assert!(h.ledger_ok);
        assert!(h.audit_ok);
        assert_eq!(h.verdict, "绿：域核账齐");
        Ok(())
    }

    #[test]
    fn health_degraded_when_unrun() {
        let mut d = DomainChecker::new();
        let _ = d.check_int("loglevel", "6");
        // 注入/对账未跑 → 必扣分不给绿（1000-400-300=300）
        let h = d.health();
        assert_eq!(h.score, 300);
        assert_eq!(h.verdict, "红：域核失守");
        assert!(!h.inject_ok);
        assert!(!h.recon_ok);
    }

    #[test]
    fn health_penalizes_recon_failure() {
        let mut d = DomainChecker::new();
        let _ = d.inject_gate();
        let rows = [DefRow { key: "ghost.key", val: "1", unit: "" }];
        let _ = d.reconcile_gate(&rows); // 对账失败
        let h = d.health();
        assert!(!h.recon_ok);
        assert!(h.score < 900);
        assert_eq!(h.verdict, "黄：账面有瑕");
    }

    #[test]
    fn default_of_hits_all_16_keys() -> Result<(), ValDomError> {
        let mut i = 0;
        while i < INT_N {
            let v = default_of(INT_TABLE[i].key)?;
            assert!(!v.is_empty());
            i += 1;
        }
        let mut i2 = 0;
        while i2 < STR_N {
            let v = default_of(STR_TABLE[i2].key)?;
            assert!(!v.is_empty());
            i2 += 1;
        }
        Ok(())
    }

    #[test]
    fn render_faces_nonempty() {
        let mut d = DomainChecker::new();
        let _ = d.inject_gate();
        let _ = d.reconcile_builtin();
        let _ = d.check_int("heap.mb", "8192");
        let _ = d.check_str("hostname", "node$$");
        let rep = d.render_report();
        assert!(rep.contains("UNX-F0586"));
        assert!(rep.contains("0x1490"));
        assert!(rep.contains("注入面 4/4"));
        assert!(rep.contains("拒收率"));
        let led = d.render_ledger();
        assert!(led.contains("越上界"));
        assert!(led.contains("建议"));
        let tr = d.render_traces();
        assert!(tr.contains("L2 域界"));
        assert!(tr.contains("L2 字符集"));
        assert!(tr.contains("[拒]"));
    }

    #[test]
    fn pipeline_brief_frozen() {
        let b = pipeline_brief();
        assert!(b.contains("L1 解析"));
        assert!(b.contains("L3 对齐"));
        assert!(b.contains("L3 转义"));
    }

    #[test]
    fn key_kind_dispatch() {
        assert!(matches!(key_kind("heap.mb"), KeyKind::Int));
        assert!(matches!(key_kind("hostname"), KeyKind::Str));
        assert!(matches!(key_kind("ghost.key"), KeyKind::Unknown));
        assert!(matches!(run_one_kind_probe(), ()));
    }

    /// run_one 未知键路径（不占核受理数）。
    fn run_one_kind_probe() {
        let mut c = DomainChecker::new();
        let code = run_one(&mut c, "ghost.key", "1");
        assert_eq!(code, ValDomError::UnknownKey.code());
        assert_eq!(c.checks_int, 0);
        assert_eq!(c.checks_str, 0);
    }
}
