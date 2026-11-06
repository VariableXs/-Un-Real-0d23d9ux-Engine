//! UNX-F11678 notfound stderr/stdout 分离判据
//!
//! busybox 是"一个二进制挂两百张脸"的多调用体：同一段 main() 既要在未知名上
//! 报 `applet 'xyz' not found`，又要在 `--help`/`--list` 上把帮助名单吐给用户。
//! 这两类输出**去向不同**——错误归 stderr(fd2)、帮助归 stdout(fd1)——而用户
//! （以及 CI 里的 `busybox xyz 2>/dev/null` 这类管道）只靠"哪条流上有字节"来
//! 判断这次调用到底成没成。这张卡判的就是这条分界线：
//!
//! * 未知 applet 的错误行必须**只**出现在 fd2；`2>/dev/null` 之后 fd1 必须
//!   干干净净（0 字节），否则 shell 脚本会把报错当成程序的正经输出吃掉；
//! * `busybox --help`/`--list`/`<applet> --help` 的帮助文本必须**只**出现在
//!   fd1；`1>/dev/null` 之后 fd2 必须无声，否则日志采集器会把帮助当成报错；
//! * 未知名伪符号链接（argv[0] 未注册）降级成 usage：降级档的 usage 在基准机
//!   上随 F11662 落在 fd2 且退 1——"降级 usage 不许借 stdout 的壳"是本卡与
//!   F11662 的联签点；
//! * `2>&1` 合并重定向到管道时，stderr 无缓冲、stdout 全缓冲：**错误行必须
//!   先于帮助行落地**。顺序一翻，读日志的人会先看到 usage 再看到"找不到"，
//!   把一次 127 读成一次"程序在提示我用法"——通道对、顺序错，同样是分离破断；
//! * 写进 stdout 缓冲区的字节如果在 exit 前没被 flush 就消失，fd1 的"0 字节"
//!   是假干净：本卡把 flush 单列一闸，把"缓冲区吞字"与"根本没写"分开记账。
//!
//! 通道是这台机器的语义合同：fd1 是"程序的成果"，fd2 是"程序对世界的抱怨"。
//! POSIX 与 busybox 上游都只在这一条上从不妥协。本卡因此把**落错通道**判死，
//! 而不是降权：报错上 stdout 会让 `$(busybox nope)` 的命令替换吞掉整句错误，
//! 帮助上 stderr 会让 `busybox --help 2>/dev/null | head` 变成空白页。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! `argv → lookup → verdict → emit → order → flush → exit`，七道闸秩 0..6。
//!
//! 1. `argv`：argv 防线。argv[0]/argv[1] 为空的防御档在这里定案——它什么都
//!    不该产出（连错误行都不许有），晚一步判就会把"防御档的沉默"误记成
//!    "报错丢失"。裸调用（无任何参数）也在这闸领用法义务：usage 归 fd2、退 1。
//! 2. `lookup`：applet 表查名。裁剪件、大小写错件、未注册件全部查不到，
//!    "查不到"这个事实在本闸定案；伪链未注册名在本闸降级（usage 档），不进入
//!    notfound 档——这是 F11662 与 F11671 两卡的接口，次序颠倒就会把降级档
//!    的退 1 误判成 notfound 的退 127。
//! 3. `verdict`：场景定案。由"入口方式 × 名字状态 × 请求种类"三元组决定这次
//!    调用的**通道义务**（fd1/fd2/无声）与应然退出码（127/1/0）。义务必须先于
//!    观测存在，否则 emit 闸无从判断"什么算落错"。
//! 4. `emit`：通道闸（本卡主闸）。产出的字节必须落在义务通道上；落错 → 泄漏
//!    档；该产出的一个字节都没有 → 吞字档。两档都判死，但必须分开——一个
//!    "有字、错门"，一个"无字、失联"，混档之后审计脚本无法定位是重定向错了
//!    还是实现漏了。
//! 5. `order`：合流次序闸。只有 emit 全对（字都去了该去的门）才谈得上"两条
//!    门都有字时谁先落地"。把 order 放在 emit 之前，会把"错误整句走了 stdout"
//!    的现场误报成次序问题，掩盖通道破断本身。
//! 6. `flush`：冲刷闸。fd1 上落了字但进程退出前缓冲区没冲出去 → 假干净。
//!    它必须最后于 emit/order：先承认"写了、门也对、序也对"，才谈得上
//!    "写进管道的字有没有真的到达读者"。
//! 7. `exit`：退出码聚合。127 = notfound 族（直接调用与 sh 外部命令两路同码，
//!    与 F11664/F11665 联签），1 = 用法/降级/裸调用族，0 = 帮助族与正常执行。
//!    码是前六闸的投影，不许单独申报：任何"码 0 但 emit 红过"的申报都要在这
//!    里被回头核对。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * 与 F11677（帮助文本双机快照，判**文本内容**逐字一致）的边界：那张卡管
//!   "字写了什么"，本卡管"字去了哪条流、以什么序、有没有真的到达"。同一段
//!   usage 文本，F11677 判它的字节形，本卡判它的落点；两卡互不重复。
//! * 与 F11674（notfound 文案逐字判据）的边界：F11674 钉 `applet '<名>' not
//!   found` 模板与名字呈现；模板逐字对了，本卡还要求这句**只**在 fd2 出现。
//!   本卡的 `message()` 复刻同一模板只为给判据提供落点核对，不重判模板本身。
//! * 与 F11679（场景 × 退出码矩阵）的边界：那张卡把"码归哪一列"当主判据，
//!   本卡把"字节归哪条流"当主判据。127 申报对了但错误行走了 stdout，F11679
//!   绿、本卡红；这正是两张卡必须并存的原因。
//! * 与 F11662（argv[0] 未注册名降级主行为）的边界：F11662 判"降不降级、
//!   退不退 1"；本卡判"降级 usage 落在哪条流"。降级档 usage 借 stdout 的壳
//!   是本卡判死的独立形态，F11662 不管。
//! * 本卡的排他职责：① 义务通道表（场景 → fd）；② 污染字节宽度（`value`）；
//!   ③ 合流次序（`order` 闸）；④ 冲刷真实性（`flush` 闸）。四件事在其它任何
//!   B24 卡上都没有落点。
//! * 谁判死谁放行：错误族字节全在 fd2、帮助族字节全在 fd1、合流时错误先落地、
//!   fd1 的字真的被冲出——四条全中放行；任何一条破（泄漏/吞字/翻序/失联）判死。
//!
//! 判据六维：结束阶段（stage）、退出码（code）、污染字节宽度（value）、
//! 文案落点（note）、通道义务兑现（default）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → 退出码 → 字节 → 注解 → 义务兑现。
//!
//! 语料 14 槽：8 槽申报正确（静默 1、帮助 3、诊断 2、notfound 2），6 槽复刻
//! 同一"直接调用未知名"现场、各挂一维错报（estate/stage/code/value/note/
//! default）。账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11678，取自全仓分配表，不许自改）。
pub const SEG_BASE: u32 = 0x6d2d0;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 档位数（静默/帮助/诊断/notfound 四档，即通道义务的四族）。
pub const TIER_COUNT: usize = 4;

/// 与 F11664 的段位交叉署名：notfound 退出码三通道一致的那张卡的段基址。
/// 本卡不 import 对方符号，只复算位距，由 `name_ok()` 校验。
pub const ANCHOR_F11664_SEG: u32 = 0x6d290;
/// F11664 与本卡的位距（0x6d2d0 - 0x6d290）。
pub const ANCHOR_F11664_GAP: u32 = 0x40;
/// 与 F11677（帮助文本双机快照）的位距（0x6d2d0 - 0x6d2c0）。
pub const ANCHOR_F11677_GAP: u32 = 0x10;
/// 与 F11679（场景 × 退出码矩阵，本卡后继）的位距。
pub const ANCHOR_F11679_GAP: u32 = 0x10;

// ---------------------------------------------------------------------------
// 文件描述符与通道
// ---------------------------------------------------------------------------

/// 没有落点：字节谁也没去（吞字档的观测形态）。
pub const FD_NONE: u8 = 0;
/// 标准输出：程序成果的通道，帮助族义务落点。
pub const FD_OUT: u8 = 1;
/// 标准错误：程序对世界的抱怨，错误族义务落点。
pub const FD_ERR: u8 = 2;
/// 场景无通道义务（静默档）：任何字节都算污染。
pub const FD_SILENT: u8 = 3;
/// 通道总数（stdout 与 stderr 两条）。
pub const CHANNEL_COUNT: usize = 2;

// ---------------------------------------------------------------------------
// 退出码（与 B24 批共同的三码；2 为 POSIX 用法档，归 F11679 立卡，本卡只署名）
// ---------------------------------------------------------------------------

/// 退出码：全绿（帮助族与正常执行）。
pub const EXIT_OK: u32 = 0;
/// 退出码：用法/降级/裸调用族。
pub const EXIT_USAGE: u32 = 1;
/// 退出码：applet 未找到族（与 F11664/F11665 三通道同码）。
pub const EXIT_NOTFOUND: u32 = 127;
/// 退出码：POSIX 用法错档（awk/sed 类 applet 的选项语法错）。
/// 本卡不产出这一码，只署名声明——它由后继卡 F11679 立矩阵列。
pub const EXIT_POSIX: u32 = 2;

/// 单条文案字节数的合法上界（usage 全文不足 4KB，超过即坏观测）。
pub const MSG_MAX: u32 = 0x0000_4000;

/// 合法 fd 取值的个数（NONE/OUT/ERR/SILENT 四值）。
pub const FD_SET_SIZE: usize = 4;

/// busybox 版本号（本卡文案锚定的基准机版本，逐字呈现于 usage 首行）。
pub const BB_VERSION: &str = "BusyBox v1.36.1";

// ---------------------------------------------------------------------------
// 基准机线路字节实测（含结尾换行）
// ---------------------------------------------------------------------------
//
// 本卡的 `value` 不是随手写的整数：它是"这条流上真的动了多少字节"。为了让这个
// 数字可以被复算而不是被声明，这里把基准机上八类现场的线路字节数钉成常量，
// `message_ok()` 再用文案函数逐字重算它们——文案与字节数对不上，就是坏现场。
// 名单正文（`--list`）逐字归 F11677，本卡只钉它的总字节数并核对表头是其前缀。

/// `busybox frobnicate` 在 fd2 上的字节数：`applet 'frobnicate' not found` + `\n`。
pub const WIRE_NOTFOUND: u32 = 30;

/// `sh frobnicate` 在 fd2 上的字节数：`sh: frobnicate: not found` + `\n`。
pub const WIRE_ASH: u32 = 26;

/// usage 全文在流上的字节数：首行 35 + 用法行 41（各含结尾换行）。
pub const WIRE_USAGE: u32 = 76;

/// usage 首行（含换行）的字节数：`BusyBox v1.36.1 multi-call binary.` + `\n`。
pub const WIRE_USAGE_HEAD: u32 = 35;

/// usage 用法行（含换行）的字节数：`usage: busybox [function] [arguments]...` + `\n`。
pub const WIRE_USAGE_LINE: u32 = 41;

/// `busybox ls --help` 在 fd1 上的字节数：首行 + `usage: ls [OPTIONS]...` + `\n`。
pub const WIRE_APPLET_HELP: u32 = 58;

/// `busybox ls -Z` 在 fd2 上的字节数：`ls: unrecognized option: Z` + `\n`。
pub const WIRE_BADOPTS: u32 = 27;

/// `busybox --list` 在 fd1 上的总字节数（基准机 v1.36.1  defconfig 实测）；
/// 表头行 `"Currently defined functions:"` + `\n` 占 29 字节，其余是名单正文。
pub const WIRE_ROSTER: u32 = 3180;

/// 表头行（含换行）的字节数。
pub const WIRE_ROSTER_HEAD: u32 = 29;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 结束阶段申报不符。
pub const SS_ESTAGE: u32 = SEG_BASE + 0x01;
/// 退出码申报不符。
pub const SS_ECODE: u32 = SEG_BASE + 0x02;
/// 污染字节宽度申报不符。
pub const SS_EVALUE: u32 = SEG_BASE + 0x03;
/// 文案落点申报不符。
pub const SS_ENOTE: u32 = SEG_BASE + 0x04;
/// 通道义务兑现申报不符。
pub const SS_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 通道归属破断：义务兑现位与落点/字节宽度三者不同族（结构维）。
pub const SS_ESTREAM: u32 = SEG_BASE + 0x06;
/// 成败互报。
pub const SS_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const SS_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const SS_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const SS_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const SS_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const SS_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const SS_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类：对应七道闸 + 语料 + 重放）
// ---------------------------------------------------------------------------

/// argv 防线段。
pub const OP_ARGV: u8 = 0x01;
/// applet 查名段。
pub const OP_LOOKUP: u8 = 0x02;
/// 场景定案段。
pub const OP_VERDICT: u8 = 0x03;
/// 通道落点段。
pub const OP_EMIT: u8 = 0x04;
/// 合流次序段。
pub const OP_ORDER: u8 = 0x05;
/// 冲刷真实性段。
pub const OP_FLUSH: u8 = 0x06;
/// 退出码聚合段。
pub const OP_EXIT: u8 = 0x07;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x08;
/// 账本重放。
pub const OP_REPLAY: u8 = 0x09;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端逐字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut shift = 56u32;
    loop {
        v = fold_byte(v, (((w >> shift) & 0xff) as u32) as u8);
        if shift == 0 {
            break;
        }
        shift -= 8;
    }
    v
}

/// 折入一个字符串。
fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut v = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            v = fold_byte(v, *b);
        }
        i += 1;
    }
    v
}

/// 折入一个小整数（与 fold_word 同构展宽）。
fn fold_small(h: u64, v: u8) -> u64 {
    fold_word(h, v as u64)
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// 十进制文本（u32，逐位从后往前填，不递归）。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        i -= 1;
        if let Some(b) = buf.get_mut(i) {
            *b = b'0' + ((n % 10) as u8);
        }
        n /= 10;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 定宽小写十六进制文本。
fn hex_text(v: u32, digits: usize) -> String {
    let mut s = String::new();
    let mut n = digits;
    while n > 0 {
        n -= 1;
        let shift = (n as u32) * 4;
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
    }
    s
}

/// u32 文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 有符号十进制文本（本卡的偏移量与差值全部走这里，符号必须可见）。
fn i32_text(v: i32) -> String {
    if v == 0 {
        return String::from("0");
    }
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text(v.unsigned_abs()).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// bool 文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// 通道名的固定呈现（审计文本用它点名 fd）。
pub fn fd_name(fd: u8) -> &'static str {
    match fd {
        FD_NONE => "nowhere",
        FD_OUT => "stdout",
        FD_ERR => "stderr",
        _ => "unknown",
    }
}

/// 义务通道名的固定呈现（SILENT 与 NONE 必须分词，吞字与无声不是一回事）。
pub fn obligation_name(ob: u8) -> &'static str {
    match ob {
        FD_NONE => "unbound",
        FD_OUT => "stdout-only",
        FD_ERR => "stderr-only",
        FD_SILENT => "silent",
        _ => "unknown",
    }
}

/// 通道 fd 的数值是否合法（0/1/2/3 四值之一）。
pub fn fd_legal(fd: u8) -> bool {
    fd == FD_NONE || fd == FD_OUT || fd == FD_ERR || fd == FD_SILENT
}

/// 把 `sh: ` 前缀贴进文案（ash 报错的程序名取自 argv[0] 的 basename）。
fn ash_prefix() -> String {
    String::from("sh: ")
}

// ---------------------------------------------------------------------------
// 文案（真实 busybox / ash 表面措辞逐字复刻 + 本卡自研审计文案）
// ---------------------------------------------------------------------------

/// busybox 未知 applet 的直接报错——`applet '<名>' not found`（F11674 同源模板）。
pub fn msg_applet_not_found(name: &str) -> String {
    let mut s = String::from("applet '");
    s.push_str(name);
    s.push_str("' not found");
    s
}

/// busybox 对未知名给出建议时的后缀行——`No help available for '<名>'`。
pub fn msg_no_help_available(name: &str) -> String {
    let mut s = String::from("No help available for '");
    s.push_str(name);
    s.push('\'');
    s
}

/// ash 的外部命令未找到——`sh: <名>: not found`（与 F11665 同码同族）。
pub fn msg_ash_not_found(name: &str) -> String {
    let mut s = ash_prefix();
    s.push_str(name);
    s.push_str(": not found");
    s
}

/// usage 首行（裸调用/降级/帮助族共用；版本号逐字锚定基准机）。
pub fn msg_usage_head() -> String {
    let mut s = String::from(BB_VERSION);
    s.push_str(" multi-call binary.");
    s
}

/// usage 的用法行（第二行，逐字）。
pub fn msg_usage_line() -> String {
    String::from("usage: busybox [function] [arguments]...")
}

/// 帮助族全文（usage 首行 + 用法行，两行以 \n 分隔，结尾无换行）。
pub fn msg_usage_full() -> String {
    let mut s = msg_usage_head();
    s.push('\n');
    s.push_str(msg_usage_line().as_str());
    s
}

/// busybox applet 选项报错——`<applet>: unrecognized option: <opt>`。
/// busybox 的 getopt 长名报错用冒号形（GNU 用引号形，两式不可混认）。
pub fn msg_bb_unrecognized(applet: &str, opt: &str) -> String {
    let mut s = String::from(applet);
    s.push_str(": unrecognized option: ");
    s.push_str(opt);
    s
}

/// busybox 对已知 applet 的逐 applet usage 引导行——`Currently defined functions:`。
pub fn msg_roster_header() -> String {
    String::from("Currently defined functions:")
}

/// 本卡自研审计文案：错误族字节污染了 stdout。
pub fn msg_stdout_contaminated(bytes: u32) -> String {
    let mut s = String::from("stdout contaminated by error line (");
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes on fd 1)");
    s
}

/// 本卡自研审计文案：帮助族字节泄漏到了 stderr。
pub fn msg_help_leaked(bytes: u32) -> String {
    let mut s = String::from("help text leaked to stderr (");
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes on fd 2)");
    s
}

/// 本卡自研审计文案：静默档却产出了字节。
pub fn msg_quiet_run_noisy(bytes: u32, fd: u8) -> String {
    let mut s = String::from("quiet run emitted output (");
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes on ");
    s.push_str(fd_name(fd));
    s.push(')');
    s
}

/// 本卡自研审计文案：错误该上 stderr 却一个字都没出现（吞字档）。
pub fn msg_error_dropped() -> String {
    String::from("not-found error never reached stderr")
}

/// 本卡自研审计文案：帮助该上 stdout 却一个字都没出现（失联档）。
pub fn msg_help_dropped() -> String {
    String::from("help text never reached stdout")
}

/// 本卡自研审计文案：`2>&1` 合流时帮助行抢在错误行前落地。
pub fn msg_order_flipped(name: &str) -> String {
    let mut s = String::from("merged pipe: usage landed before the error for '");
    s.push_str(name);
    s.push('\'');
    s
}

/// 本卡自研审计文案：fd1 缓冲区在 exit 前未冲刷，字节假死。
pub fn msg_unflushed(bytes: u32) -> String {
    let mut s = String::from("stdout buffer dropped at exit (");
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes never flushed)");
    s
}

/// 本卡自研审计文案：argv 防线拒绝空名（防御档，不产出任何流字节）。
pub fn msg_empty_argv() -> String {
    String::from("busybox: empty applet name refused before lookup")
}

/// 通道归属破断的审计文案：义务兑现、字节宽度与落点三者不同族。
pub fn msg_stream_broken(note: &str, value: u32, code: u32) -> String {
    let mut s = String::from("stream attribution broken (note=");
    s.push_str(note);
    s.push_str(", leak=");
    s.push_str(dec_text(value).as_str());
    s.push_str(", exit=");
    s.push_str(dec_text(code).as_str());
    s.push(')');
    s
}

/// 段基址的定宽十六进制呈现（审计文本用，宽度 5，逐字 `0x` 前缀）。
pub fn seg_text() -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(SEG_BASE, 5).as_str());
    s
}
// ---------------------------------------------------------------------------
// 枚举：入口方式
// ---------------------------------------------------------------------------

/// 这次调用是**怎么**到达 busybox 的。入口方式决定错误文案的样式，
/// 也决定本卡能不能主张"这句该走 stderr"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvokeWay {
    /// `busybox <名字> ...`：显式多路调用，argv[1] 即 applet 名。
    BusyboxDirect,
    /// 伪符号链接：`ln -s busybox foo; ./foo`，名字来自 argv[0]。
    SymlinkArgv0,
    /// shell 外部命令：`sh -c 'foo'`，由 ash 的 PATH 查找兜底。
    AshExternal,
}

impl InvokeWay {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            InvokeWay::BusyboxDirect => "busybox-direct",
            InvokeWay::SymlinkArgv0 => "symlink-argv0",
            InvokeWay::AshExternal => "ash-external",
        }
    }

    /// 这条入口是否自己产出错误文案（ash 的错误由 shell 生成，不是 applet 表）。
    pub fn emits_own_error(&self) -> bool {
        match self {
            InvokeWay::AshExternal => false,
            InvokeWay::BusyboxDirect | InvokeWay::SymlinkArgv0 => true,
        }
    }

    /// 这条入口对未注册名的处置：直接调用走 notfound 档（127），
    /// 伪链走降级 usage 档（1），ash 走 shell 档（127 但另一族文案）。
    pub fn unknown_disposition(&self) -> &'static str {
        match self {
            InvokeWay::BusyboxDirect => "notfound-127",
            InvokeWay::SymlinkArgv0 => "downgrade-usage-1",
            InvokeWay::AshExternal => "shell-127",
        }
    }

    /// 未知名在这条入口上的应然退出码。
    pub fn unknown_code(&self) -> u32 {
        match self {
            InvokeWay::BusyboxDirect | InvokeWay::AshExternal => EXIT_NOTFOUND,
            InvokeWay::SymlinkArgv0 => EXIT_USAGE,
        }
    }

    /// 这条入口能否以裸调用（无任何参数）出现——只有直接调用有裸档。
    pub fn supports_bare(&self) -> bool {
        match self {
            InvokeWay::BusyboxDirect => true,
            InvokeWay::SymlinkArgv0 | InvokeWay::AshExternal => false,
        }
    }

    /// 这条入口能否走 `2>&1` 合流观测（ash 场景由 shell 统一管合流，单列）。
    pub fn merged_visible(&self) -> bool {
        match self {
            InvokeWay::AshExternal => false,
            InvokeWay::BusyboxDirect | InvokeWay::SymlinkArgv0 => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 枚举：名字在 applet 表里的状态
// ---------------------------------------------------------------------------

/// 被请求的名字在 applet 表里的真实状态。未注册、被裁掉、大小写错在表面上
/// 都表现为"查不到"，但三者是三条不同的卡（F11661/F11663/F11671）立过的档，
/// 本卡把它们并进同一族义务（stderr-only），并让状态维进入指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameState {
    /// 已注册：表里查得到，正常派发。
    Registered,
    /// 从未注册过：真·未知名字。
    Unknown,
    /// 上游有、本配置裁掉了（CONFIG 档缺失，与 F11676 联签）。
    Cropped,
    /// 大小写错（`LS` 之于 `ls`，busybox 表全小写，与 F11671 联签）。
    CaseWrong,
    /// 空名：argv 防线的对象。
    Empty,
}

impl NameState {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            NameState::Registered => "registered",
            NameState::Unknown => "unknown",
            NameState::Cropped => "cropped",
            NameState::CaseWrong => "case-wrong",
            NameState::Empty => "empty",
        }
    }

    /// 是否查不到（三族未注册形态并成一族义务）。
    pub fn miss(&self) -> bool {
        match self {
            NameState::Unknown | NameState::Cropped | NameState::CaseWrong => true,
            NameState::Registered | NameState::Empty => false,
        }
    }

    /// 是否应产出 notfound 文案（空名不配拥有文案，那是防御档的沉默）。
    pub fn wants_error_line(&self) -> bool {
        match self {
            NameState::Unknown | NameState::Cropped | NameState::CaseWrong => true,
            NameState::Registered | NameState::Empty => false,
        }
    }

    /// 查不到的名字在文案里应当被逐字引用（引用形归 F11674 判，本卡只管有没有）。
    pub fn quoted_in_message(&self) -> bool {
        match self {
            NameState::Empty => false,
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 枚举：请求种类
// ---------------------------------------------------------------------------

/// 用户这次到底在要什么。请求种类与名字状态共同决定通道义务。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Req {
    /// 正常执行一个 applet。
    Run,
    /// 裸调用：`busybox` 不带任何参数。
    Bare,
    /// 全局帮助：`busybox --help` 或 `busybox help`。
    Help,
    /// 名单：`busybox --list`。
    List,
    /// 逐 applet 帮助：`busybox ls --help`。
    AppletHelp,
    /// 已知 applet 领了它不认的选项。
    BadOption,
}

impl Req {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Req::Run => "run",
            Req::Bare => "bare",
            Req::Help => "help",
            Req::List => "list",
            Req::AppletHelp => "applet-help",
            Req::BadOption => "bad-option",
        }
    }

    /// 是否帮助族（义务通道 = stdout）。
    pub fn help_family(&self) -> bool {
        match self {
            Req::Help | Req::List | Req::AppletHelp => true,
            Req::Run | Req::Bare | Req::BadOption => false,
        }
    }

    /// 是否诊断族（义务通道 = stderr，且有义务产出字节）。
    pub fn diag_family(&self) -> bool {
        match self {
            Req::Bare | Req::BadOption => true,
            Req::Run | Req::Help | Req::List | Req::AppletHelp => false,
        }
    }

    /// 本请求对名字状态的要求：帮助族不问名字、选项错必须有名、裸调用必须空名。
    pub fn accepts_state(&self, st: &NameState) -> bool {
        match self {
            Req::Run => *st != NameState::Empty,
            Req::Bare => *st == NameState::Empty || *st == NameState::Registered,
            Req::Help | Req::List => true,
            Req::AppletHelp | Req::BadOption => *st == NameState::Registered,
        }
    }

    /// 是否只有直接调用能发出这一请求。
    pub fn direct_only(&self) -> bool {
        match self {
            Req::Bare | Req::Help | Req::List => true,
            Req::Run | Req::AppletHelp | Req::BadOption => false,
        }
    }
}

/// 请求种类数（六种的在册数，供枚举自检复算）。
pub const REQ_COUNT: usize = 6;

/// 入口方式数（三种）。
pub const WAY_COUNT: usize = 3;

/// 名字状态数（五种）。
pub const STATE_COUNT: usize = 5;

// ---------------------------------------------------------------------------
// 枚举：闸门阶段
// ---------------------------------------------------------------------------

/// 闸门阶段（纵线七段，秩 0..6）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// argv 防线（空名/裸调用定案）。
    Argv,
    /// applet 表查名（未注册/降级定案）。
    Lookup,
    /// 场景定案（义务通道与应然码）。
    Verdict,
    /// 通道落点（本卡主闸）。
    Emit,
    /// 合流次序。
    Order,
    /// 冲刷真实性。
    Flush,
    /// 退出码聚合。
    Exit,
}

impl Stage {
    /// 次序（纵线不可倒）。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Argv => 0,
            Stage::Lookup => 1,
            Stage::Verdict => 2,
            Stage::Emit => 3,
            Stage::Order => 4,
            Stage::Flush => 5,
            Stage::Exit => 6,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Argv => "argv",
            Stage::Lookup => "lookup",
            Stage::Verdict => "verdict",
            Stage::Emit => "emit",
            Stage::Order => "order",
            Stage::Flush => "flush",
            Stage::Exit => "exit",
        }
    }

    /// 是否失败落点（Exit 之外都算某道闸拦下）。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Exit => false,
            _ => true,
        }
    }

    /// 是否流面闸门（emit/order/flush 三段直接观测字节）。
    pub fn stream_plane(&self) -> bool {
        match self {
            Stage::Emit | Stage::Order | Stage::Flush => true,
            Stage::Argv | Stage::Lookup | Stage::Verdict | Stage::Exit => false,
        }
    }

    /// 是否定案闸门（argv/lookup/verdict 三段产出义务，不看字节）。
    pub fn decides_obligation(&self) -> bool {
        match self {
            Stage::Argv | Stage::Lookup | Stage::Verdict => true,
            Stage::Emit | Stage::Order | Stage::Flush | Stage::Exit => false,
        }
    }
}

/// 阶段总数。
pub const STAGE_COUNT: usize = 7;

// ---------------------------------------------------------------------------
// 枚举：文案落点
// ---------------------------------------------------------------------------

/// 文案落点（现场形态的分类；right 族 = 落点对门，defect 族 = 分离破断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 正常执行，两门皆无声。
    Clean,
    /// 未found 错误行正确落在 stderr（直接调用）。
    NotFoundErr,
    /// shell 的 not found 行正确落在 stderr（ash 路）。
    AshNotFound,
    /// 裸调用 usage 正确落在 stderr（F11662 联签档）。
    BareUsage,
    /// 伪链降级 usage 正确落在 stderr（降级族）。
    DowngradeUsage,
    /// applet 选项报错正确落在 stderr。
    UsageErr,
    /// 全局帮助正确落在 stdout。
    HelpOut,
    /// `--list` 名单正确落在 stdout。
    ListOut,
    /// 逐 applet usage 正确落在 stdout。
    AppletUsage,
    /// 错误族字节污染了 stdout——判死。
    ErrLeak,
    /// 帮助族字节泄漏到了 stderr——判死。
    HelpLeak,
    /// 静默档却产出字节——判死。
    SilentLeak,
    /// 错误该有却一个字都没有（吞字档）——判死。
    ErrMissing,
    /// 帮助该有却一个字都没有（失联档）——判死。
    HelpMissing,
    /// 合流时帮助抢在错误前落地（翻序档）——判死。
    OrderFlipped,
    /// stdout 字节未冲刷即退出（假干净档）——判死。
    OutLost,
    /// argv 防线拒绝空名（防御档，无字节产出）。
    EmptyArgv,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Clean => "clean",
            Note::NotFoundErr => "notfound-stderr",
            Note::AshNotFound => "shell-stderr",
            Note::BareUsage => "bare-usage-stderr",
            Note::DowngradeUsage => "downgrade-usage-stderr",
            Note::UsageErr => "option-stderr",
            Note::HelpOut => "help-stdout",
            Note::ListOut => "list-stdout",
            Note::AppletUsage => "applet-usage-stdout",
            Note::ErrLeak => "error-on-stdout",
            Note::HelpLeak => "help-on-stderr",
            Note::SilentLeak => "quiet-noisy",
            Note::ErrMissing => "error-dropped",
            Note::HelpMissing => "help-dropped",
            Note::OrderFlipped => "order-flipped",
            Note::OutLost => "unflushed-lost",
            Note::EmptyArgv => "empty-argv",
        }
    }

    /// 是否工具自认成功（应然码 0 的那一族落点）。
    pub fn success(&self) -> bool {
        match self {
            Note::Clean | Note::HelpOut | Note::ListOut | Note::AppletUsage => true,
            _ => false,
        }
    }

    /// 是否对档族（字节落在了义务通道上，成败另说）。
    pub fn right_family(&self) -> bool {
        match self {
            Note::NotFoundErr
            | Note::AshNotFound
            | Note::BareUsage
            | Note::DowngradeUsage
            | Note::UsageErr
            | Note::HelpOut
            | Note::ListOut
            | Note::AppletUsage => true,
            _ => false,
        }
    }

    /// 是否分离破断族（本卡判死的七种形态）。
    pub fn defect_family(&self) -> bool {
        match self {
            Note::ErrLeak
            | Note::HelpLeak
            | Note::SilentLeak
            | Note::ErrMissing
            | Note::HelpMissing
            | Note::OrderFlipped
            | Note::OutLost => true,
            _ => false,
        }
    }

    /// 是否无声族（本来就不该有字节）。
    pub fn quiet_family(&self) -> bool {
        match self {
            Note::Clean | Note::EmptyArgv => true,
            _ => false,
        }
    }

    /// 是否"有字节落错门"的泄漏子类（污染宽度必须大于零的那三档）。
    pub fn leak_subfamily(&self) -> bool {
        match self {
            Note::ErrLeak | Note::HelpLeak | Note::SilentLeak => true,
            _ => false,
        }
    }

    /// 是否"该有字却没有"的吞字子类（污染宽度必须为零的那两档）。
    pub fn dropped_subfamily(&self) -> bool {
        match self {
            Note::ErrMissing | Note::HelpMissing => true,
            _ => false,
        }
    }

    /// 本落点在错误族（1）/帮助族（2）/静默（0）的三分类，供审计文本点名。
    pub fn msg_side(&self) -> u8 {
        match self {
            Note::NotFoundErr
            | Note::AshNotFound
            | Note::BareUsage
            | Note::DowngradeUsage
            | Note::UsageErr
            | Note::ErrLeak
            | Note::ErrMissing => 1,
            Note::HelpOut | Note::ListOut | Note::AppletUsage | Note::HelpLeak | Note::HelpMissing => 2,
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 枚举：结果档位
// ---------------------------------------------------------------------------

/// 结果档位。四档即通道义务四族：静默 / 帮助(stdout) / 诊断(stderr，非127) /
/// notfound(stderr，127)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 无声成功（正常执行，两门全净）。
    Quiet,
    /// 帮助族成功（stdout 有货、stderr 必须净）。
    Roster,
    /// 诊断族（stderr 有货、退出码 1：裸调用/选项错/降级）。
    Diag,
    /// notfound 族（stderr 有货、退出码 127）。
    Notfound,
}

impl Tier {
    /// 由结果投影。
    ///
    /// 分档只看退出码与落点族，红档槽仍留在自己那一档里——把"错误走了 stdout"
    /// 的红槽挪去别的档，等于替申报者掩盖了案发现场。
    pub fn of(o: &Outcome) -> Tier {
        if o.code == EXIT_NOTFOUND {
            Tier::Notfound
        } else if o.code == EXIT_OK {
            if o.note == Note::Clean {
                Tier::Quiet
            } else {
                Tier::Roster
            }
        } else {
            Tier::Diag
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Quiet => 0,
            Tier::Roster => 1,
            Tier::Diag => 2,
            Tier::Notfound => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Quiet => "quiet",
            Tier::Roster => "roster",
            Tier::Diag => "diag",
            Tier::Notfound => "notfound",
        }
    }

    /// 是否成功档（应然退出码 0）。
    pub fn success(&self) -> bool {
        match self {
            Tier::Quiet | Tier::Roster => true,
            Tier::Diag | Tier::Notfound => false,
        }
    }

    /// 该档的义务通道（帮助档 stdout、错误两档 stderr、静默档无）。
    pub fn obligation(&self) -> u8 {
        match self {
            Tier::Quiet => FD_SILENT,
            Tier::Roster => FD_OUT,
            Tier::Diag | Tier::Notfound => FD_ERR,
        }
    }

    /// 是否 stderr 义务的两档之一（notfound 与诊断共用一条通道，只分码）。
    pub fn stderr_side(&self) -> bool {
        match self {
            Tier::Diag | Tier::Notfound => true,
            Tier::Quiet | Tier::Roster => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 枚举：申报缺陷
// ---------------------------------------------------------------------------

/// 申报缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错。
    Stage,
    /// 退出码错。
    Code,
    /// 污染宽度错。
    Value,
    /// 落点错。
    Note,
    /// 义务兑现错。
    Default,
    /// 通道归属三元组破断（结构维）。
    Stream,
    /// 成败互报。
    Estate,
    /// 计数错。
    Count,
    /// 槽位错。
    Slot,
    /// 指纹错。
    Seal,
    /// 无观测。
    Nobs,
    /// 矩阵不平。
    Matrix,
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Stage => SS_ESTAGE,
            Fault::Code => SS_ECODE,
            Fault::Value => SS_EVALUE,
            Fault::Note => SS_ENOTE,
            Fault::Default => SS_EDEFAULT,
            Fault::Stream => SS_ESTREAM,
            Fault::Estate => SS_ESTATE,
            Fault::Count => SS_ECOUNT,
            Fault::Slot => SS_ESLOT,
            Fault::Seal => SS_ESEAL,
            Fault::Nobs => SS_ENOBS,
            Fault::Matrix => SS_EMATRIX,
            Fault::Bal => SS_EBAL,
        }
    }

    /// 是否非零。
    pub fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 缺陷短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Value => "value",
        Fault::Note => "note",
        Fault::Default => "default",
        Fault::Stream => "stream",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Bal => "bal",
    }
}
// ---------------------------------------------------------------------------
// 通道视图
// ---------------------------------------------------------------------------

/// 一条物理通道的观测画像。本卡把"通道"立成结构维的意义在于：
/// 判定不直接看散落的 fd 数字，而是看两条通道各自的**字节存量**与**是否被冲**，
/// 泄漏与吞字都必须在通道视图上留下可复算的痕迹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    /// 文件描述符（1 或 2）。
    pub fd: u8,
    /// 该通道实际收到的字节数。
    pub bytes: u32,
    /// 该通道在进程退出前是否完成冲刷（stderr 恒为 true：无缓冲）。
    pub flushed: bool,
}

impl Channel {
    /// 空白通道。
    pub const fn blank() -> Channel {
        Channel { fd: FD_NONE, bytes: 0, flushed: false }
    }

    /// 造一条通道。
    pub const fn made(fd: u8, bytes: u32, flushed: bool) -> Channel {
        Channel { fd, bytes, flushed }
    }

    /// 是否 stdout。
    pub fn is_stdout(&self) -> bool {
        self.fd == FD_OUT
    }

    /// 是否 stderr。
    pub fn is_stderr(&self) -> bool {
        self.fd == FD_ERR
    }

    /// 是否收到过任何字节。
    pub fn used(&self) -> bool {
        self.bytes != 0
    }

    /// 是否干净：无字节，或有字节且已冲刷（假干净在这里会被拆穿）。
    pub fn pure(&self) -> bool {
        if self.bytes == 0 {
            return true;
        }
        self.flushed
    }

    /// 是否坏通道（fd 不是 1/2，或 stderr 谎称未冲刷）。
    pub fn broken(&self) -> bool {
        if self.fd != FD_OUT && self.fd != FD_ERR {
            return true;
        }
        if self.fd == FD_ERR && !self.flushed {
            return true;
        }
        self.bytes > MSG_MAX
    }

    /// 有效到达读者的字节（未冲刷的一律不计到达）。
    pub fn delivered(&self) -> u32 {
        if self.flushed {
            self.bytes
        } else {
            0
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_small(h, self.fd);
        h = fold_word(h, self.bytes as u64);
        fold_word(h, self.flushed as u64)
    }
}

/// 两条通道的默认视图：stdout 全缓冲但已冲、stderr 无缓冲。
/// 观测（Shape）会覆盖它；这张表只作为"没有任何输出"的基准形状参与复算。
pub fn channels() -> [Channel; CHANNEL_COUNT] {
    [
        Channel { fd: FD_OUT, bytes: 0, flushed: true },
        Channel { fd: FD_ERR, bytes: 0, flushed: true },
    ]
}

/// 把一次观测折算成两条通道的存量（错误 tuple 与帮助 tuple 各自的 fd 归位）。
pub fn view_channels(err_len: u32, err_fd: u8, help_len: u32, help_fd: u8, flushed: bool) -> [Channel; CHANNEL_COUNT] {
    let mut out = Channel { fd: FD_OUT, bytes: 0, flushed };
    let mut err = Channel { fd: FD_ERR, bytes: 0, flushed: true };
    if err_fd == FD_OUT {
        out.bytes = out.bytes.saturating_add(err_len);
    } else if err_fd == FD_ERR {
        err.bytes = err.bytes.saturating_add(err_len);
    }
    if help_fd == FD_OUT {
        out.bytes = out.bytes.saturating_add(help_len);
    } else if help_fd == FD_ERR {
        err.bytes = err.bytes.saturating_add(help_len);
    }
    [out, err]
}

// ---------------------------------------------------------------------------
// 场景义务表（入口 × 请求 × 名字状态 → 通道义务 / 应然码 / 应然落点）
// ---------------------------------------------------------------------------

/// 一个被册立的场景：本卡全部通道义务的**唯一来源**。
/// `define()` 的 verdict 闸与自检谓词 `scenes_ok()` 各自独立实现，两者必须全等。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scene {
    /// 场景标签。
    pub tag: &'static str,
    /// 入口方式。
    pub way: InvokeWay,
    /// 请求种类。
    pub req: Req,
    /// 名字状态。
    pub st: NameState,
    /// 义务通道（FD_OUT / FD_ERR / FD_SILENT）。
    pub ob: u8,
    /// 应然退出码。
    pub code: u32,
    /// 应然落点。
    pub note: Note,
    /// 是否有产出义务（义务通道上必须至少有一个字节）。
    pub must_emit: bool,
}

impl Scene {
    /// 空白场景。
    pub const fn blank() -> Scene {
        Scene {
            tag: "",
            way: InvokeWay::BusyboxDirect,
            req: Req::Run,
            st: NameState::Registered,
            ob: FD_SILENT,
            code: EXIT_OK,
            note: Note::Clean,
            must_emit: false,
        }
    }

    /// 是否 127 族。
    pub fn is_notfound(&self) -> bool {
        self.code == EXIT_NOTFOUND
    }

    /// 义务通道是否 stderr。
    pub fn err_bound(&self) -> bool {
        self.ob == FD_ERR
    }

    /// 义务通道是否 stdout。
    pub fn out_bound(&self) -> bool {
        self.ob == FD_OUT
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.way.text());
        h = fold_str(h, self.req.text());
        h = fold_str(h, self.st.text());
        h = fold_small(h, self.ob);
        h = fold_word(h, self.code as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.must_emit as u64)
    }
}

/// 场景总数（册面 12 个，覆盖三入口全组合）。
pub const SCENE_COUNT: usize = 12;

/// 十二个册立场景。`scenes_ok()` 会把这张表逐行喂给 `define()` 复算，
/// 表与纵线任何一处不一致都要露出来。
pub fn scenes() -> [Scene; SCENE_COUNT] {
    [
        Scene {
            tag: "direct-run-registered",
            way: InvokeWay::BusyboxDirect,
            req: Req::Run,
            st: NameState::Registered,
            ob: FD_SILENT,
            code: EXIT_OK,
            note: Note::Clean,
            must_emit: false,
        },
        Scene {
            tag: "direct-run-unknown",
            way: InvokeWay::BusyboxDirect,
            req: Req::Run,
            st: NameState::Unknown,
            ob: FD_ERR,
            code: EXIT_NOTFOUND,
            note: Note::NotFoundErr,
            must_emit: true,
        },
        Scene {
            tag: "direct-run-cropped",
            way: InvokeWay::BusyboxDirect,
            req: Req::Run,
            st: NameState::Cropped,
            ob: FD_ERR,
            code: EXIT_NOTFOUND,
            note: Note::NotFoundErr,
            must_emit: true,
        },
        Scene {
            tag: "direct-run-case",
            way: InvokeWay::BusyboxDirect,
            req: Req::Run,
            st: NameState::CaseWrong,
            ob: FD_ERR,
            code: EXIT_NOTFOUND,
            note: Note::NotFoundErr,
            must_emit: true,
        },
        Scene {
            tag: "direct-help",
            way: InvokeWay::BusyboxDirect,
            req: Req::Help,
            st: NameState::Registered,
            ob: FD_OUT,
            code: EXIT_OK,
            note: Note::HelpOut,
            must_emit: true,
        },
        Scene {
            tag: "direct-list",
            way: InvokeWay::BusyboxDirect,
            req: Req::List,
            st: NameState::Registered,
            ob: FD_OUT,
            code: EXIT_OK,
            note: Note::ListOut,
            must_emit: true,
        },
        Scene {
            tag: "direct-applet-help",
            way: InvokeWay::BusyboxDirect,
            req: Req::AppletHelp,
            st: NameState::Registered,
            ob: FD_OUT,
            code: EXIT_OK,
            note: Note::AppletUsage,
            must_emit: true,
        },
        Scene {
            tag: "direct-bare",
            way: InvokeWay::BusyboxDirect,
            req: Req::Bare,
            st: NameState::Empty,
            ob: FD_ERR,
            code: EXIT_USAGE,
            note: Note::BareUsage,
            must_emit: true,
        },
        Scene {
            tag: "direct-badopts",
            way: InvokeWay::BusyboxDirect,
            req: Req::BadOption,
            st: NameState::Registered,
            ob: FD_ERR,
            code: EXIT_USAGE,
            note: Note::UsageErr,
            must_emit: true,
        },
        Scene {
            tag: "symlink-unknown",
            way: InvokeWay::SymlinkArgv0,
            req: Req::Run,
            st: NameState::Unknown,
            ob: FD_ERR,
            code: EXIT_USAGE,
            note: Note::DowngradeUsage,
            must_emit: true,
        },
        Scene {
            tag: "ash-unknown",
            way: InvokeWay::AshExternal,
            req: Req::Run,
            st: NameState::Unknown,
            ob: FD_ERR,
            code: EXIT_NOTFOUND,
            note: Note::AshNotFound,
            must_emit: true,
        },
        Scene {
            tag: "ash-badopts",
            way: InvokeWay::AshExternal,
            req: Req::BadOption,
            st: NameState::Registered,
            ob: FD_ERR,
            code: EXIT_USAGE,
            note: Note::UsageErr,
            must_emit: true,
        },
    ]
}

/// 按三元组查场景（线性小表，12 行；查不到即未册立）。
pub fn scene_of(way: InvokeWay, req: Req, st: NameState) -> Option<Scene> {
    let tab = scenes();
    let mut i = 0usize;
    while i < tab.len() {
        if let Some(sc) = tab.get(i) {
            if sc.way == way && sc.req == req && sc.st == st {
                return Some(*sc);
            }
        }
        i += 1;
    }
    None
}

/// 该场景是否要求字节必须存在（义务通道不许空手）。
pub fn scene_requires_emit(way: InvokeWay, req: Req, st: NameState) -> bool {
    match scene_of(way, req, st) {
        Some(sc) => sc.must_emit,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 形状（本卡输入的全部维度）
// ---------------------------------------------------------------------------

/// 一次调用的形状：入口 + 请求 + 名字状态 + 两 tuple 的实际落点 + 合流与冲刷观测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 被请求的名字（裸调用为空串）。
    pub name: &'static str,
    /// 入口方式。
    pub way: InvokeWay,
    /// 名字状态。
    pub state: NameState,
    /// 请求种类。
    pub req: Req,
    /// 错误族文案字节数（0 = 本轮没有错误族文案）。
    pub err_len: u32,
    /// 错误族文案实际落到的 fd（0/1/2）。
    pub err_fd: u8,
    /// 帮助族文案字节数（0 = 本轮没有帮助族文案；
    /// 在错误场景里它表示随错误附带的 usage 降级文本）。
    pub help_len: u32,
    /// 帮助族文案实际落到的 fd（0/1/2）。
    pub help_fd: u8,
    /// 是否 `2>&1` 合流观测（管道场景）。
    pub merged: bool,
    /// 合流时错误行是否先落地。
    pub err_first: bool,
    /// stdout 是否在退出前完成冲刷。
    pub flushed: bool,
}

impl Shape {
    /// 默认形状：直接调用正常执行，两门无声。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            name: "ls",
            way: InvokeWay::BusyboxDirect,
            state: NameState::Registered,
            req: Req::Run,
            err_len: 0,
            err_fd: FD_NONE,
            help_len: 0,
            help_fd: FD_NONE,
            merged: false,
            err_first: true,
            flushed: true,
        }
    }

    /// 换"错误 tuple"的副本（谓词把同一场景推到不同落点）。
    pub const fn clone_err(&self, len: u32, fd: u8) -> Shape {
        Shape {
            tag: self.tag,
            name: self.name,
            way: self.way,
            state: self.state,
            req: self.req,
            err_len: len,
            err_fd: fd,
            help_len: self.help_len,
            help_fd: self.help_fd,
            merged: self.merged,
            err_first: self.err_first,
            flushed: self.flushed,
        }
    }

    /// 换"帮助 tuple"的副本（把帮助/降级文本推到另一条通道）。
    pub const fn clone_help(&self, len: u32, fd: u8) -> Shape {
        Shape {
            tag: self.tag,
            name: self.name,
            way: self.way,
            state: self.state,
            req: self.req,
            err_len: self.err_len,
            err_fd: self.err_fd,
            help_len: len,
            help_fd: fd,
            merged: self.merged,
            err_first: self.err_first,
            flushed: self.flushed,
        }
    }

    /// 换"入口方式"的副本（三入口对照复算）。
    pub const fn clone_way(&self, w: InvokeWay) -> Shape {
        Shape {
            tag: self.tag,
            name: self.name,
            way: w,
            state: self.state,
            req: self.req,
            err_len: self.err_len,
            err_fd: self.err_fd,
            help_len: self.help_len,
            help_fd: self.help_fd,
            merged: self.merged,
            err_first: self.err_first,
            flushed: self.flushed,
        }
    }

    /// 换"名字状态"的副本（未注册三形态对照）。
    pub const fn clone_state(&self, st: NameState) -> Shape {
        Shape {
            tag: self.tag,
            name: self.name,
            way: self.way,
            state: st,
            req: self.req,
            err_len: self.err_len,
            err_fd: self.err_fd,
            help_len: self.help_len,
            help_fd: self.help_fd,
            merged: self.merged,
            err_first: self.err_first,
            flushed: self.flushed,
        }
    }

    /// 本轮产出的文案总字节（tuple 求和，饱和加）。
    pub fn emitted_total(&self) -> u32 {
        self.err_len.saturating_add(self.help_len)
    }

    /// 落在 stdout 上的字节。
    pub fn out_landed(&self) -> u32 {
        let a = if self.err_fd == FD_OUT { self.err_len } else { 0 };
        let b = if self.help_fd == FD_OUT { self.help_len } else { 0 };
        a.saturating_add(b)
    }

    /// 落在 stderr 上的字节。
    pub fn err_landed(&self) -> u32 {
        let a = if self.err_fd == FD_ERR { self.err_len } else { 0 };
        let b = if self.help_fd == FD_ERR { self.help_len } else { 0 };
        a.saturating_add(b)
    }

    /// 谁的门也没进的字节（合法域里必须为零；非零即坏观测）。
    pub fn stranded_bytes(&self) -> u32 {
        let a = if self.err_fd == FD_NONE { self.err_len } else { 0 };
        let b = if self.help_fd == FD_NONE { self.help_len } else { 0 };
        a.saturating_add(b)
    }

    /// 本场景的义务通道（与场景表同源、独立实现的直接判定式；
    /// `scenes_ok()` 负责把两处复算对到一起）。
    pub fn channel_of(&self) -> u8 {
        match self.req {
            Req::Bare | Req::BadOption => FD_ERR,
            Req::Help | Req::List | Req::AppletHelp => FD_OUT,
            Req::Run => {
                if self.state.miss() {
                    FD_ERR
                } else {
                    FD_SILENT
                }
            }
        }
    }

    /// 本场景的应然退出码（伪链未注册名降级退 1，其余未注册族退 127）。
    pub fn expected_code(&self) -> u32 {
        match self.req {
            Req::Bare | Req::BadOption => EXIT_USAGE,
            Req::Help | Req::List | Req::AppletHelp => EXIT_OK,
            Req::Run => {
                if self.state.miss() {
                    self.way.unknown_code()
                } else {
                    EXIT_OK
                }
            }
        }
    }

    /// 本场景的应然落点。
    pub fn right_note(&self) -> Note {
        match self.req {
            Req::Bare => Note::BareUsage,
            Req::BadOption => Note::UsageErr,
            Req::Help => Note::HelpOut,
            Req::List => Note::ListOut,
            Req::AppletHelp => Note::AppletUsage,
            Req::Run => {
                if !self.state.miss() {
                    return Note::Clean;
                }
                match self.way {
                    InvokeWay::SymlinkArgv0 => Note::DowngradeUsage,
                    InvokeWay::AshExternal => Note::AshNotFound,
                    InvokeWay::BusyboxDirect => Note::NotFoundErr,
                }
            }
        }
    }

    /// 污染字节宽度：落在**义务通道对面**的字节。
    ///
    /// 口径细分（这正是 emit 与 order 两闸能分开的前提）：
    /// * stderr 义务场景：错误行本身走上 stdout 即污染；附带的 usage 降级文本
    ///   走 stdout 属**被容忍形态**（不同版本 busybox 的 usage 落点不一），
    ///   不计污染，改由 order/flush 两闸看管它的次序与到达。
    /// * stdout 义务场景：帮助文本走 stderr 即污染。
    /// * 静默场景：任何字节都算污染。
    pub fn wrong_bytes(&self, ob: u8) -> u32 {
        match ob {
            FD_ERR => {
                if self.err_fd == FD_OUT {
                    self.err_len
                } else {
                    0
                }
            }
            FD_OUT => {
                if self.help_fd == FD_ERR {
                    self.help_len
                } else {
                    0
                }
            }
            FD_SILENT => self.emitted_total(),
            _ => 0,
        }
    }

    /// 是否处于"两门都有货且合流"的次序观测态。
    pub fn order_observable(&self) -> bool {
        self.merged && self.out_landed() > 0 && self.err_landed() > 0
    }

    /// 通道 fd 观测是否合法。
    pub fn fds_legal(&self) -> bool {
        self.err_fd <= FD_ERR && self.help_fd <= FD_ERR
    }

    /// 形状是否合法可判（只禁结构自相矛盾，落点错是观测不是坏形状）。
    pub fn valid(&self) -> bool {
        if !self.fds_legal() {
            return false;
        }
        if self.err_len > MSG_MAX || self.help_len > MSG_MAX {
            return false;
        }
        if self.err_len == 0 && self.err_fd != FD_NONE {
            return false;
        }
        if self.help_len == 0 && self.help_fd != FD_NONE {
            return false;
        }
        if self.stranded_bytes() != 0 {
            return false;
        }
        if !self.req.accepts_state(&self.state) {
            return false;
        }
        if self.req.direct_only() && self.way != InvokeWay::BusyboxDirect {
            return false;
        }
        if self.way == InvokeWay::SymlinkArgv0 && self.req != Req::Run {
            return false;
        }
        if self.way == InvokeWay::AshExternal && self.req.help_family() {
            return false;
        }
        if self.req == Req::Bare && !self.way.supports_bare() {
            return false;
        }
        if self.merged && !self.way.merged_visible() {
            return false;
        }
        if self.req == Req::Run && !self.state.miss() && self.state != NameState::Registered {
            return false;
        }
        if self.req == Req::Run && self.state == NameState::Empty {
            return false;
        }
        if self.name.is_empty() && self.req != Req::Bare {
            return false;
        }
        true
    }

    /// 指纹：对全部判定字段敏感。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.name);
        h = fold_str(h, self.way.text());
        h = fold_str(h, self.state.text());
        h = fold_str(h, self.req.text());
        h = fold_word(h, self.err_len as u64);
        h = fold_small(h, self.err_fd);
        h = fold_word(h, self.help_len as u64);
        h = fold_small(h, self.help_fd);
        h = fold_word(h, self.merged as u64);
        h = fold_word(h, self.err_first as u64);
        fold_word(h, self.flushed as u64)
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次判定的结果（六维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段（最后拦下这次调用的闸）。
    pub stage: Stage,
    /// 退出码（0/1/127）。
    pub code: u32,
    /// 污染字节宽度（正确运行恒为 0）。
    pub value: u32,
    /// 文案落点。
    pub note: Note,
    /// 义务兑现位：场景有通道义务且字节全部落对了门才为真。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome { stage: Stage::Exit, code: EXIT_OK, value: 0, note: Note::Clean, default: false }
    }

    /// 是否成功（码 0 且落点属成功族）。
    pub fn ok(&self) -> bool {
        self.code == EXIT_OK && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否 127 族。
    pub fn notfound(&self) -> bool {
        self.code == EXIT_NOTFOUND
    }

    /// 义务兑现归因：兑现位为真必须"停在出口闸、零污染、落点对门"。
    pub fn attributed(&self) -> bool {
        self.default && self.stage == Stage::Exit && self.value == 0 && self.note.right_family()
    }

    /// 通道三元组自洽：兑现位、污染宽度与落点族必须同族。
    ///
    /// 兑现 ⇒ 出口闸 + 零污染 + 对档；泄漏三档 ⇒ 污染必大于零；
    /// 吞字两档 ⇒ 污染必等于零且停 emit 闸。任何一条反了都是结构破断，
    /// 由 `SS_ESTREAM` 记账——它不进 judge 的六维，只进结构闸。
    pub fn stream_consistent(&self, _s: &Shape) -> bool {
        if self.default {
            if self.stage != Stage::Exit || self.value != 0 || !self.note.right_family() {
                return false;
            }
        }
        if self.note.leak_subfamily() && self.value == 0 {
            return false;
        }
        if self.note.dropped_subfamily() && self.value != 0 {
            return false;
        }
        if self.note == Note::OrderFlipped && (self.stage != Stage::Order || self.value != 0) {
            return false;
        }
        if self.note == Note::OutLost && (self.stage != Stage::Flush || self.value == 0) {
            return false;
        }
        if self.note.defect_family() && self.default {
            return false;
        }
        if self.value > 0 && !self.note.leak_subfamily() && self.note != Note::OutLost {
            return false;
        }
        true
    }

    /// 落点与码是否同族（127 只配 notfound 两档，0 只配成功族）。
    pub fn code_note_coherent(&self) -> bool {
        match self.code {
            EXIT_NOTFOUND => {
                self.note == Note::NotFoundErr
                    || self.note == Note::AshNotFound
                    || self.note == Note::ErrLeak
                    || self.note == Note::ErrMissing
                    || self.note == Note::OrderFlipped
                    || self.note == Note::OutLost
            }
            EXIT_OK => self.note.success() || self.note.defect_family(),
            EXIT_USAGE => {
                self.note == Note::BareUsage
                    || self.note == Note::DowngradeUsage
                    || self.note == Note::UsageErr
                    || self.note == Note::EmptyArgv
                    || self.note == Note::ErrLeak
                    || self.note == Note::ErrMissing
                    || self.note == Note::HelpLeak
                    || self.note == Note::OrderFlipped
            }
            _ => false,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.default as u64)
    }
}
// ---------------------------------------------------------------------------
// 定义纵线（闸门次序：argv → lookup → verdict → emit → order → flush → exit）
// ---------------------------------------------------------------------------

/// 落点结算：verdict 给出义务/应然码/应然落点后，本函数按
/// emit → order → flush → exit 的次序把观测字节折算成结论。
///
/// 每一闸只在上一闸放行后才执行：先看"有没有字"，再看"字进错门没有"，
/// 两门都有字时才谈次序，最后才谈冲刷。倒序会把"整句错误走了 stdout"
/// 误报成次序问题，掩盖通道破断本身。
fn settle(s: &Shape, code: u32, note: Note, ob: u8) -> Outcome {
    // ---- emit（其一）：义务通道该收到的主文案一个字都没有 ----
    if ob == FD_ERR && s.err_len == 0 {
        return Outcome { stage: Stage::Emit, code, value: 0, note: Note::ErrMissing, default: false };
    }
    if ob == FD_OUT && s.help_len == 0 {
        return Outcome { stage: Stage::Emit, code, value: 0, note: Note::HelpMissing, default: false };
    }
    // ---- emit（其二）：主文案落到了义务通道的对面 ----
    let wrong = s.wrong_bytes(ob);
    if wrong > 0 {
        let n = match ob {
            FD_ERR => Note::ErrLeak,
            FD_OUT => Note::HelpLeak,
            _ => Note::SilentLeak,
        };
        return Outcome { stage: Stage::Emit, code, value: wrong, note: n, default: false };
    }
    // ---- order：合流且两门皆有货时，错误行必须先行 ----
    if s.order_observable() && !s.err_first {
        return Outcome { stage: Stage::Order, code, value: 0, note: Note::OrderFlipped, default: false };
    }
    // ---- flush：stdout 落了字却没冲出去 ----
    let out_bytes = s.out_landed();
    if out_bytes > 0 && !s.flushed {
        return Outcome { stage: Stage::Flush, code, value: out_bytes, note: Note::OutLost, default: false };
    }
    // ---- exit：全部放行 ----
    Outcome { stage: Stage::Exit, code, value: 0, note, default: ob != FD_SILENT }
}

/// 判定一次调用在"错误入 stderr / 帮助入 stdout"合同下的收尾结论。
///
/// argv 闸的防御档是唯一不落 settle 的分支：它按合同**不该有任何字节**，
/// 走 settle 反而会把"防御性沉默"记成吞字。伪链未注册名在 lookup 闸就降级
/// （义务与错误族同门、码取 1），不进 verdict 的 notfound 档——降级 usage
/// 借 stdout 的壳会被 settle 的 emit 闸抓成 HelpLeak 同族泄漏。
pub fn define(s: &Shape) -> Outcome {
    // ---- argv：防御档 ----
    if s.state == NameState::Empty && s.req != Req::Bare {
        return Outcome {
            stage: Stage::Argv,
            code: EXIT_USAGE,
            value: 0,
            note: Note::EmptyArgv,
            default: false,
        };
    }
    // ---- verdict + emit/order/flush/exit ----
    settle(s, s.expected_code(), s.right_note(), s.channel_of())
}

/// 现场文案（note → 真实文本；成功且无声的落点返回空串）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::Clean => String::new(),
        Note::EmptyArgv => msg_empty_argv(),
        Note::NotFoundErr => msg_applet_not_found(s.name),
        Note::AshNotFound => msg_ash_not_found(s.name),
        Note::BareUsage | Note::DowngradeUsage => msg_usage_full(),
        Note::UsageErr => msg_bb_unrecognized(s.name, "Z"),
        Note::HelpOut => msg_usage_full(),
        Note::ListOut => msg_roster_header(),
        Note::AppletUsage => {
            let mut t = msg_usage_head();
            t.push('\n');
            t.push_str("usage: ");
            t.push_str(s.name);
            t.push_str(" [OPTIONS]...");
            t
        }
        Note::ErrLeak => msg_stdout_contaminated(o.value),
        Note::HelpLeak => msg_help_leaked(o.value),
        Note::SilentLeak => {
            let fd = if s.err_fd != FD_NONE { s.err_fd } else { s.help_fd };
            msg_quiet_run_noisy(o.value, fd)
        }
        Note::ErrMissing => msg_error_dropped(),
        Note::HelpMissing => msg_help_dropped(),
        Note::OrderFlipped => msg_order_flipped(s.name),
        Note::OutLost => msg_unflushed(o.value),
    }
}

/// 泄漏审计行：只有破断形态才有内容，正确运行返回空串。
/// 与 `message()` 分家的理由：message 面向日志复原，本函数面向
/// 分离合同的违约核算，两者对同一次运行的口径必须能互相对账。
pub fn leak_report(s: &Shape, o: &Outcome) -> String {
    if !o.note.defect_family() {
        return String::new();
    }
    let mut t = String::from("want ");
    t.push_str(obligation_name(o_bound_after_verdict(s)));
    t.push_str(" / leak=");
    t.push_str(dec_text(o.value).as_str());
    t.push_str(" / note=");
    t.push_str(o.note.name());
    t
}

/// 场景义务通道（verdict 之后、emit 之前的那个"应然"）——供审计行点名。
pub fn o_bound_after_verdict(s: &Shape) -> u8 {
    s.channel_of()
}

// ---------------------------------------------------------------------------
// 申报与用例
// ---------------------------------------------------------------------------

/// 一槽申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 结束阶段。
    pub stage: Stage,
    /// 退出码。
    pub code: u32,
    /// 污染字节宽度。
    pub value: u32,
    /// 文案落点。
    pub note: Note,
    /// 义务兑现位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Exit, code: EXIT_OK, value: 0, note: Note::Clean, default: false }
    }

    /// 是否申报成功（码 0 且落点属成功族）。
    pub fn success(&self) -> bool {
        self.code == EXIT_OK && self.note.success()
    }
}

/// 一槽用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 空白用例。
    pub const fn blank() -> Case {
        Case { shape: Shape::blank(), claim: Claim::blank() }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        self.shape.tag
    }

    /// 实际结果。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 对判。
    pub fn judge(&self) -> Fault {
        judge(self)
    }

    /// 是否一致。
    pub fn ok(&self) -> bool {
        judge(self) == Fault::Ok
    }
}

// ---------------------------------------------------------------------------
// 语料（14 槽：8 活 + 6 死）
// ---------------------------------------------------------------------------

/// 8 个可正确申报的形状 + 6 个复刻形状（复刻自 direct-notfound-unknown）。
#[rustfmt::skip]
pub const SHAPES: [Shape; CORPUS] = [
    Shape { tag: "direct-run-ls", name: "ls", way: InvokeWay::BusyboxDirect,
        state: NameState::Registered, req: Req::Run,
        err_len: 0, err_fd: FD_NONE, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "direct-notfound-unknown", name: "frobnicate", way: InvokeWay::BusyboxDirect,
        state: NameState::Unknown, req: Req::Run,
        err_len: WIRE_NOTFOUND, err_fd: FD_ERR, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "ash-notfound-unknown", name: "frobnicate", way: InvokeWay::AshExternal,
        state: NameState::Unknown, req: Req::Run,
        err_len: WIRE_ASH, err_fd: FD_ERR, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "direct-help-global", name: "busybox", way: InvokeWay::BusyboxDirect,
        state: NameState::Registered, req: Req::Help,
        err_len: 0, err_fd: FD_NONE, help_len: WIRE_USAGE, help_fd: FD_OUT,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "direct-applet-help", name: "ls", way: InvokeWay::BusyboxDirect,
        state: NameState::Registered, req: Req::AppletHelp,
        err_len: 0, err_fd: FD_NONE, help_len: WIRE_APPLET_HELP, help_fd: FD_OUT,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "direct-list-roster", name: "busybox", way: InvokeWay::BusyboxDirect,
        state: NameState::Registered, req: Req::List,
        err_len: 0, err_fd: FD_NONE, help_len: WIRE_ROSTER, help_fd: FD_OUT,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "direct-bare-usage", name: "", way: InvokeWay::BusyboxDirect,
        state: NameState::Empty, req: Req::Bare,
        err_len: WIRE_USAGE_HEAD, err_fd: FD_ERR, help_len: WIRE_USAGE_LINE, help_fd: FD_ERR,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "direct-badopts-ls", name: "ls", way: InvokeWay::BusyboxDirect,
        state: NameState::Registered, req: Req::BadOption,
        err_len: WIRE_BADOPTS, err_fd: FD_ERR, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "estate-swap", name: "frobnicate", way: InvokeWay::BusyboxDirect,
        state: NameState::Unknown, req: Req::Run,
        err_len: WIRE_NOTFOUND, err_fd: FD_ERR, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "stage-swap", name: "frobnicate", way: InvokeWay::BusyboxDirect,
        state: NameState::Unknown, req: Req::Run,
        err_len: WIRE_NOTFOUND, err_fd: FD_ERR, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "code-swap", name: "frobnicate", way: InvokeWay::BusyboxDirect,
        state: NameState::Unknown, req: Req::Run,
        err_len: WIRE_NOTFOUND, err_fd: FD_ERR, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "value-swap", name: "frobnicate", way: InvokeWay::BusyboxDirect,
        state: NameState::Unknown, req: Req::Run,
        err_len: WIRE_NOTFOUND, err_fd: FD_ERR, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "note-swap", name: "frobnicate", way: InvokeWay::BusyboxDirect,
        state: NameState::Unknown, req: Req::Run,
        err_len: WIRE_NOTFOUND, err_fd: FD_ERR, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
    Shape { tag: "default-swap", name: "frobnicate", way: InvokeWay::BusyboxDirect,
        state: NameState::Unknown, req: Req::Run,
        err_len: WIRE_NOTFOUND, err_fd: FD_ERR, help_len: 0, help_fd: FD_NONE,
        merged: false, err_first: true, flushed: true },
];

/// 与 `SHAPES` 一一对应的申报。
///
/// 前 8 槽逐维等于现场；后 6 槽各只错一维，错的那维依次是
/// estate/stage/code/value/note/default。
#[rustfmt::skip]
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Exit, code: 0, value: 0, note: Note::Clean, default: false },
    Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::NotFoundErr, default: true },
    Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::AshNotFound, default: true },
    Claim { stage: Stage::Exit, code: 0, value: 0, note: Note::HelpOut, default: true },
    Claim { stage: Stage::Exit, code: 0, value: 0, note: Note::AppletUsage, default: true },
    Claim { stage: Stage::Exit, code: 0, value: 0, note: Note::ListOut, default: true },
    Claim { stage: Stage::Exit, code: 1, value: 0, note: Note::BareUsage, default: true },
    Claim { stage: Stage::Exit, code: 1, value: 0, note: Note::UsageErr, default: true },
    Claim { stage: Stage::Exit, code: 0, value: 0, note: Note::Clean, default: false },
    Claim { stage: Stage::Flush, code: 127, value: 0, note: Note::NotFoundErr, default: true },
    Claim { stage: Stage::Exit, code: 1, value: 0, note: Note::NotFoundErr, default: true },
    Claim { stage: Stage::Exit, code: 127, value: WIRE_NOTFOUND, note: Note::NotFoundErr, default: true },
    Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::ErrLeak, default: true },
    Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::NotFoundErr, default: false },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    SS_ESTATE,
    SS_ESTAGE,
    SS_ECODE,
    SS_EVALUE,
    SS_ENOTE,
    SS_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、静默档、帮助档、诊断档、notfound 档、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 1, 3, 2, 2, 929];

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            out[i] = Case { shape: *s, claim: *c };
        }
        i += 1;
    }
    out
}

/// 取一槽。
pub fn case_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    cases().get(i).copied()
}

/// 覆盖计数。
pub fn cover(n: usize) -> Result<(), u32> {
    if n != CORPUS {
        return Err(SS_ENOBS);
    }
    Ok(())
}

/// 期望缺陷。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        return Some(Fault::Ok);
    }
    REJECT_FAULTS.get(i - ACCEPT).copied()
}

/// 对判：状态 → 阶段 → 退出码 → 污染宽度 → 落点 → 义务兑现。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let k = c.claim;
    if a.ok() != k.success() {
        return Fault::Estate;
    }
    if a.stage != k.stage {
        return Fault::Stage;
    }
    if a.code != k.code {
        return Fault::Code;
    }
    if a.value != k.value {
        return Fault::Value;
    }
    if a.note != k.note {
        return Fault::Note;
    }
    if a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 逐槽释义。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let msg = message(&c.shape, &o);
    let mut s = String::from(c.shape.name);
    s.push_str(" way=");
    s.push_str(c.shape.way.text());
    s.push_str(" req=");
    s.push_str(c.shape.req.text());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" exit=");
    s.push_str(dec_text(o.code).as_str());
    s.push_str(" leak=");
    s.push_str(dec_text(o.value).as_str());
    s.push_str(" bound=");
    s.push_str(obligation_name(c.shape.channel_of()));
    s.push_str(" note=");
    s.push_str(o.note.name());
    if !msg.is_empty() {
        s.push_str(" msg=\"");
        s.push_str(msg.as_str());
        s.push('"');
    }
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 期望表（语料的第二份陈述）
// ---------------------------------------------------------------------------

/// 14 槽的期望结束阶段（校验器按纵线实际会走到哪一闸）。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
];

/// 14 槽的期望退出码。
const WANT_CODE: [u32; CORPUS] = [0, 127, 127, 0, 0, 0, 1, 1, 127, 127, 127, 127, 127, 127];

/// 14 槽的期望污染宽度。
const WANT_VALUE: [u32; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// 14 槽的期望文案落点。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Clean,
    Note::NotFoundErr,
    Note::AshNotFound,
    Note::HelpOut,
    Note::AppletUsage,
    Note::ListOut,
    Note::BareUsage,
    Note::UsageErr,
    Note::NotFoundErr,
    Note::NotFoundErr,
    Note::NotFoundErr,
    Note::NotFoundErr,
    Note::NotFoundErr,
    Note::NotFoundErr,
];

/// 14 槽的期望义务兑现位。
const WANT_DEFAULT: [bool; CORPUS] = [
    false, true, true, true, true, true, true, true, true, true, true, true, true, true,
];

/// 14 槽标签。
const WANT_TAGS: [&str; CORPUS] = [
    "direct-run-ls",
    "direct-notfound-unknown",
    "ash-notfound-unknown",
    "direct-help-global",
    "direct-applet-help",
    "direct-list-roster",
    "direct-bare-usage",
    "direct-badopts-ls",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 八格申报正确的下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 六格申报错误的下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 复刻槽（后六槽）的标签后缀。
const SWAP_SUFFIX: &str = "-swap";
// ---------------------------------------------------------------------------
// 统计矩阵
// ---------------------------------------------------------------------------

/// 一档统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档实际槽数。
    pub total: usize,
    /// 该档申报正确数。
    pub good: usize,
}

impl Row {
    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }

    /// 该档是否全对（有槽且申报全对）。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }

    /// 该档红档数。
    pub fn red(&self) -> usize {
        if self.good <= self.total {
            self.total - self.good
        } else {
            self.total
        }
    }

    /// 该档是否只由复刻槽变红（本卡的立卡理由：活槽不许有红）。
    pub fn reds_are_replicas(&self) -> bool {
        self.red() == 0 || self.red() == REJECT
    }

    /// 渲染一行。
    pub fn render(&self, t: Tier) -> String {
        let mut s = String::from(t.name());
        s.push_str(" total=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" good=");
        s.push_str(u32_text(self.good as u32).as_str());
        s.push_str(" red=");
        s.push_str(u32_text(self.red() as u32).as_str());
        s.push('\n');
        s
    }
}

/// 四档矩阵。
///
/// 分档只看**实际结果**落进哪一档，申报正确与否单独计。本卡的红档槽全部复刻
/// 自 direct-notfound-unknown，因此它们的"实际"仍落在 notfound 档——把谎报的
/// 槽挪档等于替申报者销毁案发现场，覆盖面也就看不见了。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（quiet/roster/diag/notfound）。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 空矩阵。
    pub const fn blank() -> Matrix {
        Matrix { rows: [Row::blank(); TIER_COUNT] }
    }

    /// 由语料统计。
    pub fn of() -> Matrix {
        let mut m = Matrix::blank();
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => return m,
            };
            let t = Tier::of(&c.actual()).index();
            let good = judge(&c) == Fault::Ok;
            if let Some(row) = m.rows.get_mut(t) {
                row.total += 1;
                if good {
                    row.good += 1;
                }
            }
            i += 1;
        }
        m
    }

    /// 取一档。
    pub fn row(&self, t: Tier) -> Option<Row> {
        self.rows.get(t.index()).copied()
    }

    /// 实际槽总数（必须等于语料规模）。
    pub fn slots(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.total;
            }
            i += 1;
        }
        n
    }

    /// 申报正确总数。
    pub fn goods(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.good;
            }
            i += 1;
        }
        n
    }

    /// 全对档数。
    pub fn perfect_rows(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.perfect() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 覆盖是否平（每档都有槽——四档即通道义务四族，缺一族即矩阵不平）。
    pub fn covered(&self) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            match self.rows.get(i) {
                Some(r) => {
                    if r.total == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// stderr 义务的两档合计（诊断 + notfound）。
    pub fn stderr_slots(&self) -> usize {
        let mut n = 0usize;
        if let Some(r) = self.rows.get(Tier::Diag.index()) {
            n += r.total;
        }
        if let Some(r) = self.rows.get(Tier::Notfound.index()) {
            n += r.total;
        }
        n
    }

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let tiers = [Tier::Quiet, Tier::Roster, Tier::Diag, Tier::Notfound];
        let mut s = String::new();
        let mut i = 0usize;
        while i < tiers.len() {
            if let Some(t) = tiers.get(i) {
                if let Some(r) = self.row(*t) {
                    s.push_str(r.render(*t).as_str());
                }
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 汇总报表
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// 静默档正确数。
    pub quiet: u32,
    /// 帮助档正确数。
    pub roster: u32,
    /// 诊断档正确数。
    pub diag: u32,
    /// notfound 档正确数。
    pub notfound: u32,
    /// 对判命中数（实际缺陷 == 期望缺陷）。
    pub matched: u32,
}

impl Report {
    /// 千分表示（四舍五入）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched * 1000 + self.total / 2) / self.total
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 闸门：不到全绿就报计数错。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(SS_ECOUNT)
        }
    }

    /// 码 0 档合计（静默 + 帮助）。
    pub fn zero_cells(&self) -> u32 {
        self.quiet + self.roster
    }

    /// 码非 0 档合计（诊断 + notfound）。
    pub fn nonzero_cells(&self) -> u32 {
        self.diag + self.notfound
    }

    /// stderr 义务档正确数（诊断 + notfound，即"错误必须进 stderr"的兑现面）。
    pub fn err_bound_cells(&self) -> u32 {
        self.diag + self.notfound
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("ss-split corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" quiet=");
        s.push_str(u32_text(self.quiet).as_str());
        s.push_str(" roster=");
        s.push_str(u32_text(self.roster).as_str());
        s.push_str(" diag=");
        s.push_str(u32_text(self.diag).as_str());
        s.push_str(" notfound=");
        s.push_str(u32_text(self.notfound).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染的逐字钉版。
const RENDER_OK: &str =
    "ss-split corpus=14 accept=8 reject=6 quiet=1 roster=3 diag=2 notfound=2 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入的作用是把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，
/// 报表就必须掉 matched、rate 随之离开 1000。若注入后仍报全绿，说明 matched
/// 是硬编码的，而不是算出来的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(SS_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut quiet = 0u32;
    let mut roster = 0u32;
    let mut diag = 0u32;
    let mut notfound = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(SS_ENOBS),
        };
        let a = c.actual();
        let mut f = judge(&c);
        if let Some((j, forced)) = over {
            if j == i {
                f = forced;
            }
        }
        let want = match expected(i) {
            Some(v) => v,
            None => return Err(SS_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Quiet => quiet += 1,
                Tier::Roster => roster += 1,
                Tier::Diag => diag += 1,
                Tier::Notfound => notfound += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, quiet, roster, diag, notfound, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 观测纵线是 `argv → lookup → verdict → emit → order → flush → exit`，
/// 七段只能向前。回退（例如已经判死在冲刷闸又声称停在落点闸）说明申报者
/// 不是按闸顺序观测的，而是拿着文案倒推——这类申报在真实故障里最危险，
/// 因为文案看着完全合理。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// 上一段次序（-1 表示未走）。
    last: i32,
    /// 已走段数。
    steps: u32,
    /// 是否已关。
    closed: bool,
}

impl Gate {
    /// 新闸门。
    pub const fn new() -> Gate {
        Gate { last: -1, steps: 0, closed: false }
    }

    /// 走一段。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            return Err(SS_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(SS_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(SS_ESTATE);
        }
        self.closed = true;
        Ok(self.steps)
    }

    /// 段数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 上一段。
    pub const fn last(&self) -> i32 {
        self.last
    }

    /// 是否已关。
    pub const fn closed(&self) -> bool {
        self.closed
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽位（必须严格等于当前长度）。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本：14 格，8 成 6 败，冻结后拒登记。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格；槽位不等于当前长度、越界或已冻结一律拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SS_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 逐格对账：槽位连续、成格值等于 `VALS`、败格码等于 `REJECT_CODES`、
    /// 败格值必归零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(SS_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SS_EBAL),
            };
            if s.slot as usize != i {
                return Err(SS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SS_EVALUE);
                        }
                    }
                    None => return Err(SS_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(SS_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SS_EVALUE);
                        }
                    }
                    None => return Err(SS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(SS_EBAL);
        }
        Ok(())
    }

    /// 全账指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SS_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建一份账本并比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(SS_ESEAL)
        }
    }

    /// 冻结指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数结果 → 样本。
fn count_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 建账本：7 格统计 + 1 格红档率（929）+ 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_VERDICT, r.map(|x| x.quiet)));
    let _ = l.register(count_cell(4, OP_EMIT, r.map(|x| x.roster)));
    let _ = l.register(count_cell(5, OP_ORDER, r.map(|x| x.diag)));
    let _ = l.register(count_cell(6, OP_EXIT, r.map(|x| x.notfound)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_FLUSH,
                ok: false,
                code: f.code(),
                value: 0,
            });
        }
        i += 1;
    }
    l
}

// ---------------------------------------------------------------------------
// 注入
// ---------------------------------------------------------------------------

/// 四路注入：形状表、判据、渲染、账本。
///
/// 四路各自独立，任何一路静默退化都会在这里变成 false：
/// 1. 表路——段基址、枚举、通道视图与场景义务表、形状合法域；
/// 2. 判路——七闸纵线、通道三元组、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && anchor_ok() && enums_ok() && chans_ok() && scenes_ok() && shape_ok();
    out[1] = define_ok() && stream_ok() && judge_ok() && matrix_ok();
    out[2] = corpus_ok() && slots_ok() && reds_ok() && message_ok() && detail_ok();
    out[3] = gate_ok()
        && ledger_ok()
        && {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            a && b
        };
    out
}

/// 段基址与位距的交叉署名复算（本卡与 B24 邻卡的段位闭合）。
pub fn anchors_agree() -> bool {
    if SEG_BASE != 0x6d2d0 {
        return false;
    }
    if ANCHOR_F11664_SEG.checked_add(ANCHOR_F11664_GAP) != Some(SEG_BASE) {
        return false;
    }
    ANCHOR_F11677_GAP == 0x10 && ANCHOR_F11679_GAP == 0x10
}
// ---------------------------------------------------------------------------
// 判据基元
// ---------------------------------------------------------------------------

/// 名称与段基址：常量链、尺寸、退出码、操作码、通道值域、交叉署名一致性。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d2d0 {
        return false;
    }
    if SS_ESTAGE != SEG_BASE + 0x01
        || SS_ECODE != SEG_BASE + 0x02
        || SS_EVALUE != SEG_BASE + 0x03
        || SS_ENOTE != SEG_BASE + 0x04
        || SS_EDEFAULT != SEG_BASE + 0x05
        || SS_ESTREAM != SEG_BASE + 0x06
    {
        return false;
    }
    if SS_ESTATE != SEG_BASE + 0x07
        || SS_ECOUNT != SEG_BASE + 0x08
        || SS_ESLOT != SEG_BASE + 0x09
        || SS_ESEAL != SEG_BASE + 0x0a
        || SS_ENOBS != SEG_BASE + 0x0b
        || SS_EMATRIX != SEG_BASE + 0x0c
        || SS_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    // 退出码必须是本批册定的三码；2 只署名不产出（矩阵列归 F11679）。
    if EXIT_OK != 0 || EXIT_USAGE != 1 || EXIT_NOTFOUND != 127 || EXIT_POSIX != 2 {
        return false;
    }
    if FD_NONE != 0 || FD_OUT != 1 || FD_ERR != 2 || FD_SILENT != 3 {
        return false;
    }
    if CHANNEL_COUNT != 2 || FD_SET_SIZE != 4 {
        return false;
    }
    if OP_ARGV != 1 || OP_LOOKUP != 2 || OP_VERDICT != 3 || OP_EMIT != 4 {
        return false;
    }
    if OP_ORDER != 5 || OP_FLUSH != 6 || OP_EXIT != 7 || OP_CORPUS != 8 || OP_REPLAY != 9 {
        return false;
    }
    if STAGE_COUNT != 7 || REQ_COUNT != 6 || WAY_COUNT != 3 || STATE_COUNT != 5 {
        return false;
    }
    if SCENE_COUNT != scenes().len() {
        return false;
    }
    if MSG_MAX != 0x4000 {
        return false;
    }
    if !anchors_agree() {
        return false;
    }
    // fd 名与义务名必须分词：nowhere 与 silent 不是一回事（吞字 vs 无声）。
    if fd_name(FD_NONE) != "nowhere" || fd_name(FD_OUT) != "stdout" || fd_name(FD_ERR) != "stderr"
    {
        return false;
    }
    if obligation_name(FD_SILENT) == obligation_name(FD_NONE) {
        return false;
    }
    if !fd_legal(FD_ERR) || fd_legal(4) {
        return false;
    }
    if BB_VERSION != "BusyBox v1.36.1" {
        return false;
    }
    let mut v = String::new();
    v.push_str("applet '");
    v.is_empty() == false && ash_prefix().as_str() == "sh: "
}

/// 枚举：七闸次序、落点四族不重不漏、档位投影、入口处置方向、缺陷码互斥。
fn enums_ok() -> bool {
    let stages = [
        Stage::Argv,
        Stage::Lookup,
        Stage::Verdict,
        Stage::Emit,
        Stage::Order,
        Stage::Flush,
        Stage::Exit,
    ];
    if stages.len() != STAGE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < stages.len() {
        let s = match stages.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() as usize != i {
            return false;
        }
        // 只有最后一段不是失败落点。
        if i + 1 == stages.len() {
            if s.failure() {
                return false;
            }
        } else if !s.failure() {
            return false;
        }
        // 流面三段：emit/order/flush。
        if s.stream_plane() != (i == 3 || i == 4 || i == 5) {
            return false;
        }
        // 定案三段：argv/lookup/verdict。
        if s.decides_obligation() != (i < 3) {
            return false;
        }
        let mut k = i + 1;
        while k < stages.len() {
            if let Some(o) = stages.get(k) {
                if o.name() == s.name() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }

    let notes = [
        Note::Clean,
        Note::NotFoundErr,
        Note::AshNotFound,
        Note::BareUsage,
        Note::DowngradeUsage,
        Note::UsageErr,
        Note::HelpOut,
        Note::ListOut,
        Note::AppletUsage,
        Note::ErrLeak,
        Note::HelpLeak,
        Note::SilentLeak,
        Note::ErrMissing,
        Note::HelpMissing,
        Note::OrderFlipped,
        Note::OutLost,
        Note::EmptyArgv,
    ];
    if notes.len() != 17 {
        return false;
    }
    let mut n = 0usize;
    while n < notes.len() {
        let a = match notes.get(n) {
            Some(v) => *v,
            None => return false,
        };
        let mut m = n + 1;
        while m < notes.len() {
            if let Some(b) = notes.get(m) {
                if a.name() == b.name() {
                    return false;
                }
            }
            m += 1;
        }
        // 四族互斥：对档 / 破断 / 无声，且每个落点恰属一族（right+defect+quiet=1）。
        let fam = a.right_family() as u8 + a.defect_family() as u8 + a.quiet_family() as u8;
        if fam != 1 {
            return false;
        }
        // 泄漏子族与吞字子族都必须是破断族；两子族互斥。
        if a.leak_subfamily() && (!a.defect_family() || a.dropped_subfamily()) {
            return false;
        }
        if a.dropped_subfamily() && (!a.defect_family() || a.leak_subfamily()) {
            return false;
        }
        // 成功落点只能出现在对档族或无声族里。
        if a.success() && (a.defect_family() || a.dropped_subfamily()) {
            return false;
        }
        if a.success() != (a == Note::Clean || a == Note::HelpOut || a == Note::ListOut
            || a == Note::AppletUsage)
        {
            return false;
        }
        n += 1;
    }
    if Note::ErrLeak.name() != "error-on-stdout" || Note::HelpLeak.name() != "help-on-stderr" {
        return false;
    }
    if Note::NotFoundErr.msg_side() != 1 || Note::HelpOut.msg_side() != 2 {
        return false;
    }
    if Note::Clean.msg_side() != 0 || Note::SilentLeak.msg_side() != 0 {
        return false;
    }

    let tiers = [Tier::Quiet, Tier::Roster, Tier::Diag, Tier::Notfound];
    let mut t = 0usize;
    while t < tiers.len() {
        let x = match tiers.get(t) {
            Some(v) => *v,
            None => return false,
        };
        if x.index() != t {
            return false;
        }
        if x.success() != (t < 2) {
            return false;
        }
        // 义务通道：静默档无声、帮助档 stdout、诊断与 notfound 档 stderr。
        let want_ob = match t {
            0 => FD_SILENT,
            1 => FD_OUT,
            _ => FD_ERR,
        };
        if x.obligation() != want_ob {
            return false;
        }
        if x.stderr_side() != (t >= 2) {
            return false;
        }
        let mut y = t + 1;
        while y < tiers.len() {
            if let Some(z) = tiers.get(y) {
                if x.name() == z.name() {
                    return false;
                }
            }
            y += 1;
        }
        t += 1;
    }

    // 入口处置方向：直接/sh 两路 127，伪链降级 1；只有直接调用有裸档。
    if InvokeWay::BusyboxDirect.unknown_code() != EXIT_NOTFOUND {
        return false;
    }
    if InvokeWay::AshExternal.unknown_code() != EXIT_NOTFOUND {
        return false;
    }
    if InvokeWay::SymlinkArgv0.unknown_code() != EXIT_USAGE {
        return false;
    }
    if !InvokeWay::BusyboxDirect.supports_bare() {
        return false;
    }
    if InvokeWay::SymlinkArgv0.supports_bare() || InvokeWay::AshExternal.supports_bare() {
        return false;
    }
    if InvokeWay::AshExternal.merged_visible() {
        return false;
    }
    if !InvokeWay::SymlinkArgv0.merged_visible() {
        return false;
    }
    if InvokeWay::AshExternal.emits_own_error() {
        return false;
    }
    if InvokeWay::BusyboxDirect.unknown_disposition() != "notfound-127" {
        return false;
    }
    if InvokeWay::SymlinkArgv0.unknown_disposition() != "downgrade-usage-1" {
        return false;
    }

    // 名字状态：查不到三形态、空名不配文案、除空名外都要引用。
    if !NameState::Unknown.miss() || !NameState::Cropped.miss() || !NameState::CaseWrong.miss() {
        return false;
    }
    if NameState::Registered.miss() || NameState::Empty.miss() {
        return false;
    }
    if NameState::Empty.wants_error_line() || !NameState::Cropped.wants_error_line() {
        return false;
    }
    if NameState::Empty.quoted_in_message() || !NameState::CaseWrong.quoted_in_message() {
        return false;
    }

    // 请求族划分：帮助族三口、诊断族两口；互斥且与通道一致。
    let reqs = [Req::Run, Req::Bare, Req::Help, Req::List, Req::AppletHelp, Req::BadOption];
    if reqs.len() != REQ_COUNT {
        return false;
    }
    let mut r = 0usize;
    while r < reqs.len() {
        let x = match reqs.get(r) {
            Some(v) => *v,
            None => return false,
        };
        if x.help_family() && x.diag_family() {
            return false;
        }
        if x.help_family() != (x == Req::Help || x == Req::List || x == Req::AppletHelp) {
            return false;
        }
        if x.diag_family() != (x == Req::Bare || x == Req::BadOption) {
            return false;
        }
        if x.direct_only() != (x == Req::Bare || x == Req::Help || x == Req::List) {
            return false;
        }
        let mut q = r + 1;
        while q < reqs.len() {
            if let Some(z) = reqs.get(q) {
                if x.text() == z.text() {
                    return false;
                }
            }
            q += 1;
        }
        r += 1;
    }
    if !Req::Run.accepts_state(&NameState::Unknown) || Req::Run.accepts_state(&NameState::Empty) {
        return false;
    }
    if !Req::Bare.accepts_state(&NameState::Empty) || Req::Bare.accepts_state(&NameState::Unknown) {
        return false;
    }
    if !Req::AppletHelp.accepts_state(&NameState::Registered) {
        return false;
    }
    if !Req::Help.accepts_state(&NameState::Unknown) {
        return false;
    }

    let faults = [
        Fault::Ok,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Stream,
        Fault::Estate,
        Fault::Count,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Bal,
    ];
    let mut f = 0usize;
    let mut seen = [0u32; 14];
    while f < faults.len() {
        let x = match faults.get(f) {
            Some(v) => *v,
            None => return false,
        };
        if x.alive() == (x == Fault::Ok) {
            return false;
        }
        if fault_short(x).is_empty() {
            return false;
        }
        if let Some(w) = seen.get_mut(f) {
            *w = x.code();
        }
        let mut g = 0usize;
        while g < f {
            if let Some(prev) = seen.get(g) {
                if *prev == x.code() {
                    return false;
                }
            }
            g += 1;
        }
        f += 1;
    }
    Fault::Ok.code() == 0
        && Fault::Stage.code() == SS_ESTAGE
        && Fault::Stream.code() == SS_ESTREAM
        && Fault::Estate.code() == SS_ESTATE
        && Fault::Bal.code() == SS_EBAL
}

/// 通道视图：两条默认通道、视图折算守恒、坏通道识别、到达口径。
fn chans_ok() -> bool {
    let cs = channels();
    if cs.len() != CHANNEL_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < cs.len() {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.broken() {
            return false;
        }
        if !c.pure() {
            return false;
        }
        if c.used() {
            return false;
        }
        i += 1;
    }
    // 第 0 条必须是 stdout、第 1 条必须是 stderr。
    if let Some(c0) = cs.get(0) {
        if !c0.is_stdout() {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c1) = cs.get(1) {
        if !c1.is_stderr() {
            return false;
        }
    } else {
        return false;
    }
    // 视图折算：错误 tuple 在 stdout、帮助 tuple 在 stderr 时的存量分账。
    let vs = view_channels(30, FD_OUT, 44, FD_ERR, false);
    let v0 = match vs.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let v1 = match vs.get(1) {
        Some(v) => *v,
        None => return false,
    };
    if v0.bytes != 30 || v1.bytes != 44 {
        return false;
    }
    // 总量守恒：两通道存量之和等于 tuple 之和。
    if v0.bytes.saturating_add(v1.bytes) != 74 {
        return false;
    }
    // 未冲刷的 stdout 不许冒充到达。
    if v0.pure() || v0.delivered() != 0 {
        return false;
    }
    // stderr 恒冲刷：谎称未冲刷是坏通道；到达数必须等于存量。
    if v1.delivered() != v1.bytes {
        return false;
    }
    let bad = Channel { fd: FD_ERR, bytes: 10, flushed: false };
    if !bad.broken() {
        return false;
    }
    let worse = Channel { fd: FD_SILENT, bytes: 0, flushed: true };
    if !worse.broken() {
        return false;
    }
    let huge = Channel { fd: FD_OUT, bytes: MSG_MAX + 1, flushed: true };
    if !huge.broken() {
        return false;
    }
    // 同字节不同 fd 必须指纹不同；冲刷位也必须进指纹。
    let a = Channel { fd: FD_OUT, bytes: 8, flushed: true };
    let b = Channel { fd: FD_ERR, bytes: 8, flushed: true };
    let c = Channel { fd: FD_OUT, bytes: 8, flushed: false };
    if a.seal() == b.seal() || a.seal() == c.seal() {
        return false;
    }
    // 空通道可复算：同一构造两次指纹相同。
    let d1 = Channel::made(FD_OUT, 12, true);
    let d2 = Channel::made(FD_OUT, 12, true);
    d1.seal() == d2.seal() && d1.pure() && !d1.broken() && d1.used()
}

/// 场景义务表：十二行全部良构、与 define 的独立实现逐行全等、义务不悬空。
fn scenes_ok() -> bool {
    let tab = scenes();
    if tab.len() != SCENE_COUNT {
        return false;
    }
    let mut i = 0usize;
    let mut codes = [0u32; SCENE_COUNT];
    while i < tab.len() {
        let sc = match tab.get(i) {
            Some(v) => *v,
            None => return false,
        };
        // 义务值域：只能是 OUT/ERR/SILENT 三口（NONE 是落点不是义务）。
        if sc.ob != FD_OUT && sc.ob != FD_ERR && sc.ob != FD_SILENT {
            return false;
        }
        // 码与通道同族：127/降级/诊断必 stderr；0 帮助必 stdout；静默必 0。
        if sc.err_bound() != (sc.code == EXIT_NOTFOUND || sc.code == EXIT_USAGE) {
            return false;
        }
        if sc.out_bound() != sc.req.help_family() {
            return false;
        }
        if sc.ob == FD_SILENT && sc.code != EXIT_OK {
            return false;
        }
        // 有义务必产出；静默档不许有产出义务。
        if sc.must_emit && sc.ob == FD_SILENT {
            return false;
        }
        if !sc.must_emit && sc.ob != FD_SILENT {
            return false;
        }
        // 落点与义务同门。
        if sc.err_bound() && sc.note.msg_side() != 1 {
            return false;
        }
        if sc.out_bound() && sc.note.msg_side() != 2 {
            return false;
        }
        // 查名逻辑自洽：Run 场景里"名字查不到"与"文案属错误族"必须同真同假
        // （空名走防御档的沉默，注册名走无声成功，两样都不许有错误行）。
        if sc.req == Req::Run {
            let err_note = sc.note == Note::NotFoundErr
                || sc.note == Note::AshNotFound
                || sc.note == Note::DowngradeUsage;
            if sc.st.wants_error_line() != err_note {
                return false;
            }
            if sc.st.miss() != err_note {
                return false;
            }
        }
        // 表内三元组互不相同。
        let mut j = 0usize;
        while j < i {
            if let Some(prev) = tab.get(j) {
                if prev.way == sc.way && prev.req == sc.req && prev.st == sc.st {
                    return false;
                }
                if prev.tag == sc.tag {
                    return false;
                }
            }
            j += 1;
        }
        // 与 define 的独立实现对表：造一个"干净落点"的形状喂纵线，
        // 结果必须与册面逐维全等。
        let mut sh = Shape::blank();
        sh.tag = sc.tag;
        sh.way = sc.way;
        sh.req = sc.req;
        sh.state = sc.st;
        if sc.ob == FD_ERR && sc.st != NameState::Empty {
            sh.err_len = WIRE_NOTFOUND;
            sh.err_fd = FD_ERR;
        }
        if sc.ob == FD_OUT {
            sh.help_len = WIRE_USAGE;
            sh.help_fd = FD_OUT;
        }
        if sc.req == Req::Bare {
            sh.err_len = WIRE_USAGE_HEAD;
            sh.err_fd = FD_ERR;
            sh.help_len = WIRE_USAGE_LINE;
            sh.help_fd = FD_ERR;
        }
        let o = define(&sh);
        if o.code != sc.code || o.note != sc.note || o.stage != Stage::Exit {
            return false;
        }
        if o.default != (sc.ob != FD_SILENT) {
            return false;
        }
        if let Some(w) = codes.get_mut(i) {
            *w = sc.code;
        }
        i += 1;
    }
    // 码列非悬空：127/1/0 三口在表内都出现（2 归 F11679，本表零出现）。
    let mut has0 = false;
    let mut has1 = false;
    let mut has127 = false;
    let mut has2 = false;
    let mut k = 0usize;
    while k < codes.len() {
        if let Some(c) = codes.get(k) {
            if *c == EXIT_OK {
                has0 = true;
            } else if *c == EXIT_USAGE {
                has1 = true;
            } else if *c == EXIT_NOTFOUND {
                has127 = true;
            } else if *c == EXIT_POSIX {
                has2 = true;
            }
        }
        k += 1;
    }
    if !has0 || !has1 || !has127 || has2 {
        return false;
    }
    // scene_of 与表同行号一致；未册立组合必须查无。
    match scene_of(InvokeWay::SymlinkArgv0, Req::Run, NameState::Unknown) {
        Some(sc) => sc.note == Note::DowngradeUsage && sc.err_bound(),
        None => false,
    }
}

/// 形状合法域：空白合法、逐项破坏必判死、tuple 守恒、clone 系保序。
fn shape_ok() -> bool {
    let base = Shape::blank();
    if !base.valid() {
        return false;
    }
    if base.channel_of() != FD_SILENT || base.expected_code() != EXIT_OK {
        return false;
    }
    // 坏 fd 值。
    let mut a = base.clone_err(10, 7);
    if a.valid() {
        return false;
    }
    // 零字节却声称落了 fd。
    a = base.clone_err(0, FD_ERR);
    if a.valid() {
        return false;
    }
    // 字节超过文案上界。
    a = base.clone_err(MSG_MAX + 1, FD_ERR);
    if a.valid() {
        return false;
    }
    // 字节谁也进不了（stranded）。
    a = base.clone_err(16, FD_NONE);
    if a.valid() {
        return false;
    }
    // 帮助场景配了错误 tuple。
    let mut h = Shape::blank();
    h.req = Req::Help;
    h.help_len = WIRE_USAGE;
    h.help_fd = FD_OUT;
    h.name = "busybox";
    if !h.valid() {
        return false;
    }
    // 帮助场景**配了错误 tuple 是合法观测**（错误自己走 stderr 并不违形状），
    // 本卡把它留给判据去定性；但"有字节谁也进不了"是坏观测，必须先挡。
    let hb = h.clone_err(WIRE_BADOPTS, FD_ERR);
    if !hb.valid() {
        return false;
    }
    if h.clone_err(WIRE_BADOPTS, FD_NONE).valid() {
        return false;
    }
    // 帮助场景把帮助挪到 fd 之外（NONE）同样坏。
    if h.clone_help(WIRE_USAGE, FD_NONE).valid() {
        return false;
    }
    // 裸调用不许配非空名请求，也不许从伪链发出。
    let mut b = Shape::blank();
    b.req = Req::Bare;
    b.state = NameState::Empty;
    b.name = "";
    b.err_len = WIRE_USAGE_HEAD;
    b.err_fd = FD_ERR;
    if !b.valid() {
        return false;
    }
    if b.clone_way(InvokeWay::SymlinkArgv0).valid() {
        return false;
    }
    // 伪链只许发 Run。
    let mut sy = b.clone_way(InvokeWay::SymlinkArgv0);
    sy.req = Req::Run;
    sy.state = NameState::Unknown;
    sy.name = "foo";
    if !sy.valid() {
        return false;
    }
    sy.req = Req::List;
    if sy.valid() {
        return false;
    }
    // ash 不许发帮助族。
    let mut ash = Shape::blank();
    ash.way = InvokeWay::AshExternal;
    ash.req = Req::Run;
    ash.state = NameState::Unknown;
    ash.name = "frobnicate";
    ash.err_len = WIRE_ASH;
    ash.err_fd = FD_ERR;
    if !ash.valid() {
        return false;
    }
    ash.req = Req::Help;
    if ash.valid() {
        return false;
    }
    // ash 场景不许开合流观测（合流口径归 shell）。
    ash.req = Req::Run;
    ash.merged = true;
    if ash.valid() {
        return false;
    }
    // Run 不许配空名。
    let mut er = Shape::blank();
    er.state = NameState::Empty;
    er.name = "";
    if er.valid() {
        return false;
    }
    // AppletHelp/BadOption 必须有名有姓。
    let mut ap = Shape::blank();
    ap.req = Req::AppletHelp;
    ap.help_len = WIRE_APPLET_HELP;
    ap.help_fd = FD_OUT;
    if !ap.valid() {
        return false;
    }
    ap.state = NameState::Unknown;
    if ap.valid() {
        return false;
    }
    // 污染宽度口径（本卡收窄）：stderr 义务时，**错误整句**落 stdout 才算污染；
    // 附带 usage 落 stdout 属版本差异，交由 order/flush 两闸兜底，不计入污染。
    let nf = match case_at(1) {
        Some(c) => c.shape,
        None => return false,
    };
    if nf.wrong_bytes(FD_ERR) != 0 {
        return false;
    }
    let leaked = nf.clone_err(WIRE_NOTFOUND, FD_OUT);
    if leaked.wrong_bytes(FD_ERR) != WIRE_NOTFOUND {
        return false;
    }
    // 泄漏现场 stderr 门是空的，次序观测不成立。
    if leaked.order_observable() {
        return false;
    }
    // 合流次序观测面：merged 且两门皆有货才算看得见。
    let mut merged_pair = nf.clone_help(WIRE_USAGE_LINE, FD_OUT);
    merged_pair.merged = true;
    if !merged_pair.order_observable() {
        return false;
    }
    // 收窄口径下附带 usage 走 stdout 的污染宽度为零。
    if merged_pair.wrong_bytes(FD_ERR) != 0 {
        return false;
    }
    if !merged_pair.valid() {
        return false;
    }
    // 两门同走 stderr：观测面关（stdout 无货），污染仍为零。
    let mut both_err = nf.clone_help(WIRE_USAGE_LINE, FD_ERR);
    both_err.merged = true;
    if both_err.order_observable() {
        return false;
    }
    if both_err.wrong_bytes(FD_ERR) != 0 {
        return false;
    }
    // 非 merged 时两门皆有货也不构成次序观测（分流采集无先后可言）。
    let split_pair = nf.clone_help(WIRE_USAGE_LINE, FD_OUT);
    if split_pair.order_observable() {
        return false;
    }
    // 静默义务的污染宽度 = 两 tuple 存量之和。
    if nf.wrong_bytes(FD_SILENT) != WIRE_NOTFOUND {
        return false;
    }
    // clone 系保序：换落点不动场景三元组与 tag。
    let c1 = nf.clone_err(WIRE_NOTFOUND, FD_OUT);
    c1.way == nf.way && c1.req == nf.req && c1.state == nf.state && c1.tag == nf.tag
}

/// 落点纵线：七闸各拦各的形态，正确场景放行到 exit，倒退路径不存在。
fn define_ok() -> bool {
    // 基准 notfound 现场（正确落点）。
    let base = match case_at(1) {
        Some(c) => c.shape,
        None => return false,
    };
    let o = define(&base);
    if o.stage != Stage::Exit || o.code != EXIT_NOTFOUND || o.value != 0 {
        return false;
    }
    if o.note != Note::NotFoundErr || !o.default || !o.attributed() {
        return false;
    }
    if !o.stream_consistent(&base) || !o.code_note_coherent() {
        return false;
    }
    // 吞字：该有字却一个没有 → emit 闸 ErrMissing。
    let dropped = base.clone_err(0, FD_NONE);
    let od = define(&dropped);
    if od.stage != Stage::Emit || od.note != Note::ErrMissing || od.default {
        return false;
    }
    if od.value != 0 || od.code != EXIT_NOTFOUND {
        return false;
    }
    // 泄漏：错误整句走了 stdout → emit 闸 ErrLeak，污染宽度 = 全句线路字节。
    let leak = base.clone_err(WIRE_NOTFOUND, FD_OUT);
    let ol = define(&leak);
    if ol.stage != Stage::Emit || ol.note != Note::ErrLeak || ol.value != WIRE_NOTFOUND {
        return false;
    }
    if ol.default || ol.ok() {
        return false;
    }
    if !ol.stream_consistent(&leak) {
        return false;
    }
    // 码 0 场景的错误落点也算失败申报面：帮助泄漏走 HelpLeak。
    let help = match case_at(3) {
        Some(c) => c.shape,
        None => return false,
    };
    let hl = help.clone_help(WIRE_USAGE, FD_ERR);
    let oh = define(&hl);
    if oh.stage != Stage::Emit || oh.note != Note::HelpLeak || oh.code != EXIT_OK {
        return false;
    }
    if oh.ok() || oh.value != WIRE_USAGE {
        return false;
    }
    // 帮助场景的**主文案**走 stderr 是 HelpLeak；此时错误 tuple 空着，
    // 说明本卡只钉"义务通道上到底缺了什么/多了什么"，不重复记第二笔账。
    if oh.note.right_family() {
        return false;
    }
    // 静默档开口即污染：正常执行的 applet 吐出一行选项报错。
    let noisy = Shape {
        tag: "quiet-noisy",
        name: "ls",
        way: InvokeWay::BusyboxDirect,
        state: NameState::Registered,
        req: Req::Run,
        err_len: WIRE_BADOPTS,
        err_fd: FD_ERR,
        help_len: 0,
        help_fd: FD_NONE,
        merged: false,
        err_first: true,
        flushed: true,
    };
    let on = define(&noisy);
    if on.stage != Stage::Emit || on.note != Note::SilentLeak || on.value != WIRE_BADOPTS {
        return false;
    }
    if on.code != EXIT_OK || on.default {
        return false;
    }
    // 闸序不可倒：两 tuple 同时落错门时，emit 必须先于 order 抓到（污染
    // 宽度取错误整句，次序问题被更严重的通道破断掩盖）。
    let both_wrong = base.clone_err(WIRE_NOTFOUND, FD_OUT).clone_help(WIRE_USAGE_LINE, FD_ERR);
    let ow = define(&both_wrong);
    if ow.stage != Stage::Emit || ow.note != Note::ErrLeak {
        return false;
    }
    if ow.value != WIRE_NOTFOUND {
        return false;
    }
    // 翻序可达构造：错误整句仍在 stderr（emit 放行），附带 usage 走 stdout
    // （收窄口径下合法），合流观测两门皆有货，而 usage 抢在错误行之前。
    let mut flipped = base.clone_help(WIRE_USAGE_LINE, FD_OUT);
    flipped.merged = true;
    flipped.err_first = false;
    let of1 = define(&flipped);
    if of1.stage != Stage::Order || of1.note != Note::OrderFlipped {
        return false;
    }
    if of1.value != 0 || of1.default || of1.code != EXIT_NOTFOUND {
        return false;
    }
    if !of1.stream_consistent(&flipped) || !of1.code_note_coherent() {
        return false;
    }
    // 同一现场只把 err_first 修回真，立刻放行到出口闸——证明 order 闸
    // 只对次序维度敏感。
    flipped.err_first = true;
    let of2 = define(&flipped);
    if of2.stage != Stage::Exit || of2.note != Note::NotFoundErr || of2.value != 0 {
        return false;
    }
    // 翻序档不借 stdout 的壳：义务 ERR、usage 也走 stderr 时观测面关闭，
    // 翻序无从谈起，直接出口。
    let mut same_door = base.clone_help(WIRE_USAGE_LINE, FD_ERR);
    same_door.merged = true;
    same_door.err_first = false;
    let of3 = define(&same_door);
    if of3.stage != Stage::Exit || of3.note != Note::NotFoundErr {
        return false;
    }
    // 观测面关闭的第二例：非合流。
    let mut unmerged = base.clone_help(WIRE_USAGE_LINE, FD_OUT);
    unmerged.err_first = false;
    let of4 = define(&unmerged);
    if of4.stage != Stage::Exit || of4.note != Note::NotFoundErr {
        return false;
    }
    // 冲刷假死：stdout 落了字却没冲（帮助场景 flush=false）。
    let mut uf = help.clone_help(WIRE_USAGE, FD_OUT);
    uf.flushed = false;
    let ou = define(&uf);
    if ou.stage != Stage::Flush || ou.note != Note::OutLost || ou.value != WIRE_USAGE {
        return false;
    }
    if ou.default || ou.ok() {
        return false;
    }
    // argv 防御档：空名不发文案、不落 settle。
    let mut empty = Shape::blank();
    empty.state = NameState::Empty;
    empty.name = "";
    empty.req = Req::BadOption;
    let oe = define(&empty);
    if oe.stage != Stage::Argv || oe.note != Note::EmptyArgv || oe.code != EXIT_USAGE {
        return false;
    }
    if oe.default || oe.value != 0 {
        return false;
    }
    // 伪链降级：降级档的**主文案就是 usage**，因此它走错门同样是泄漏——
    // 这条是 F11662 的联签点（降级 usage 不许借 stdout 的壳）。
    let mut sym = Shape::blank();
    sym.tag = "symlink-down";
    sym.way = InvokeWay::SymlinkArgv0;
    sym.state = NameState::Unknown;
    sym.name = "foo";
    sym.err_len = WIRE_USAGE;
    sym.err_fd = FD_ERR;
    sym.help_len = 0;
    sym.help_fd = FD_NONE;
    let os = define(&sym);
    if os.stage != Stage::Exit || os.code != EXIT_USAGE || os.note != Note::DowngradeUsage {
        return false;
    }
    if !os.default {
        return false;
    }
    let sym_borrow = sym.clone_err(WIRE_USAGE, FD_OUT);
    let ob2 = define(&sym_borrow);
    if ob2.stage != Stage::Emit || ob2.note != Note::ErrLeak || ob2.value != WIRE_USAGE {
        return false;
    }
    if !sym.valid() || !sym_borrow.valid() {
        return false;
    }
    // ash 路 127：同码异文案。
    let ash = match case_at(2) {
        Some(c) => c.shape,
        None => return false,
    };
    let oa = define(&ash);
    if oa.code != EXIT_NOTFOUND || oa.note != Note::AshNotFound || oa.stage != Stage::Exit {
        return false;
    }
    // 裸调用：usage 在 stderr、码 1，两 tuple 同门不算污染。
    let bare = match case_at(6) {
        Some(c) => c.shape,
        None => return false,
    };
    let obr = define(&bare);
    if obr.code != EXIT_USAGE || obr.note != Note::BareUsage || obr.value != 0 {
        return false;
    }
    if obr.stage != Stage::Exit || !obr.default {
        return false;
    }
    // 每个正确落点的指纹稳定（复算同值）。
    base.seal() == match case_at(1) {
        Some(c) => c.shape.seal(),
        None => 0,
    } && o.seal() == define(&base).seal()
}

/// 通道三元组：兑现位、污染宽度、落点族的同族不变量；破断必落 SS_ESTREAM 口径。
fn stream_ok() -> bool {
    // 正确落点：attributed 全中。
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = define(&c.shape);
        if !o.stream_consistent(&c.shape) {
            return false;
        }
        if o.default && !o.attributed() {
            return false;
        }
        if c.shape.channel_of() == FD_SILENT && o.default {
            return false;
        }
        if Tier::of(&o).obligation() != c.shape.channel_of() {
            return false;
        }
        i += 1;
    }
    // 人造违约三元组必须被抓：兑现位真但污染 nonzero。
    let bad1 = Outcome { stage: Stage::Exit, code: EXIT_OK, value: 5, note: Note::HelpOut, default: true };
    if bad1.stream_consistent(&Shape::blank()) {
        return false;
    }
    // 泄漏档却报零污染。
    let bad2 = Outcome { stage: Stage::Emit, code: EXIT_NOTFOUND, value: 0, note: Note::ErrLeak, default: false };
    if bad2.stream_consistent(&Shape::blank()) {
        return false;
    }
    // 吞字档却报有污染。
    let bad3 = Outcome { stage: Stage::Emit, code: EXIT_NOTFOUND, value: 9, note: Note::ErrMissing, default: false };
    if bad3.stream_consistent(&Shape::blank()) {
        return false;
    }
    // 翻序档停在别的闸。
    let bad4 = Outcome { stage: Stage::Emit, code: EXIT_NOTFOUND, value: 0, note: Note::OrderFlipped, default: false };
    if bad4.stream_consistent(&Shape::blank()) {
        return false;
    }
    // 冲刷档零宽度（假死必须有账）。
    let bad5 = Outcome { stage: Stage::Flush, code: EXIT_OK, value: 0, note: Note::OutLost, default: false };
    if bad5.stream_consistent(&Shape::blank()) {
        return false;
    }
    // 破断族却自称兑现。
    let bad6 = Outcome { stage: Stage::Exit, code: EXIT_OK, value: 0, note: Note::HelpLeak, default: true };
    if bad6.stream_consistent(&Shape::blank()) || bad6.attributed() {
        return false;
    }
    // 码族不协调：127 配帮助落点。
    let bad7 = Outcome { stage: Stage::Exit, code: EXIT_NOTFOUND, value: 0, note: Note::HelpOut, default: false };
    if bad7.code_note_coherent() {
        return false;
    }
    // 缺陷码 SS_ESTREAM 与其余十二码互不相同。
    let mut ks = [SS_ESTAGE, SS_ECODE, SS_EVALUE, SS_ENOTE, SS_EDEFAULT, SS_ESTREAM];
    let more = [SS_ESTATE, SS_ECOUNT, SS_ESLOT, SS_ESEAL, SS_ENOBS, SS_EMATRIX, SS_EBAL];
    let mut k = 0usize;
    while k < more.len() {
        if let Some(v) = more.get(k) {
            if let Some(dst) = ks.get_mut(k) {
                *dst = *v;
            }
        }
        k += 1;
    }
    let mut a = 0usize;
    while a < ks.len() {
        let x = match ks.get(a) {
            Some(v) => *v,
            None => return false,
        };
        let mut b = a + 1;
        while b < ks.len() {
            if let Some(y) = ks.get(b) {
                if x == *y {
                    return false;
                }
            }
            b += 1;
        }
        if x <= SEG_BASE || x > SEG_BASE + 0x0d {
            return false;
        }
        a += 1;
    }
    true
}

/// 对判：六维次序、每维单独破坏必被对应缺陷抓到、全对为 Ok。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        if i < ACCEPT {
            if want != Fault::Ok || !c.ok() {
                return false;
            }
        } else if !want.alive() {
            return false;
        }
        i += 1;
    }
    // 现场申报恒 Ok：对每槽造"逐维等于现场"的申报，判据必须放行。
    let mut k = 0usize;
    while k < CORPUS {
        let c = match cs.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let a = define(&c.shape);
        let mirror = Case {
            shape: c.shape,
            claim: Claim { stage: a.stage, code: a.code, value: a.value, note: a.note, default: a.default },
        };
        if judge(&mirror) != Fault::Ok {
            return false;
        }
        k += 1;
    }
    // 单维注入次序：先坏 Estate 的两例必须报 Estate 而不是后面的维。
    let base = match cs.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let a = define(&base.shape);
    // 成败互报（码 0 + 成功落点）抢在阶段错之前。
    let e1 = Case {
        shape: base.shape,
        claim: Claim { stage: Stage::Flush, code: 0, value: 0, note: Note::Clean, default: false },
    };
    if judge(&e1) != Fault::Estate {
        return false;
    }
    // 只坏阶段。
    let e2 = Case { shape: base.shape, claim: Claim { stage: Stage::Order, ..a_claim_of(&a) } };
    if judge(&e2) != Fault::Stage {
        return false;
    }
    // 只坏码（同不成败族：127→1，两边都非成功）。
    let e3 = Case { shape: base.shape, claim: Claim { code: EXIT_USAGE, ..a_claim_of(&a) } };
    if judge(&e3) != Fault::Code {
        return false;
    }
    // 只坏宽度。
    let e4 = Case { shape: base.shape, claim: Claim { value: 13, ..a_claim_of(&a) } };
    if judge(&e4) != Fault::Value {
        return false;
    }
    // 只坏落点（同族内换名，避免连带翻成败位）。
    let e5 = Case { shape: base.shape, claim: Claim { note: Note::AshNotFound, ..a_claim_of(&a) } };
    if judge(&e5) != Fault::Note {
        return false;
    }
    // 只坏兑现位。
    let e6 = Case { shape: base.shape, claim: Claim { default: false, ..a_claim_of(&a) } };
    if judge(&e6) != Fault::Default {
        return false;
    }
    // 越界槽的期望缺陷不存在。
    if expected(CORPUS).is_some() {
        return false;
    }
    if expected(ACCEPT).is_none() || expected(REJECT - 1 + ACCEPT).is_none() {
        return false;
    }
    // 期望缺陷表与缺陷码表同序同族。
    let mut m = 0usize;
    while m < REJECT {
        let f = match REJECT_FAULTS.get(m) {
            Some(v) => *v,
            None => return false,
        };
        let c = match REJECT_CODES.get(m) {
            Some(v) => *v,
            None => return false,
        };
        if f.code() != c {
            return false;
        }
        m += 1;
    }
    true
}

/// 现场 → 申报的投影（judge_ok 造"其余维全对"的单注案例用）。
fn a_claim_of(o: &Outcome) -> Claim {
    Claim { stage: o.stage, code: o.code, value: o.value, note: o.note, default: o.default }
}

// ---------------------------------------------------------------------------
// 判据 08：矩阵统计
// ---------------------------------------------------------------------------

/// 矩阵：四档分布、复刻槽全部留在案发现场、报表与矩阵两本账互相核对。
///
/// 这张卡最容易被做坏的地方就是"分档"：如果把谎报的槽按**申报**归档，六格复刻
/// 会被申报拽进 quiet/roster 甚至别处，`notfound` 档看着干净，红档率也看着干净
/// ——判据就此失去定位能力。本函数把"分档只看实际结果"这条钉死成可复算的账。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS {
        return false;
    }
    if m.goods() != ACCEPT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    let quiet = match m.row(Tier::Quiet) {
        Some(r) => r,
        None => return false,
    };
    let roster = match m.row(Tier::Roster) {
        Some(r) => r,
        None => return false,
    };
    let diag = match m.row(Tier::Diag) {
        Some(r) => r,
        None => return false,
    };
    let notfound = match m.row(Tier::Notfound) {
        Some(r) => r,
        None => return false,
    };
    if quiet.total != 1 || quiet.good != 1 || quiet.red() != 0 {
        return false;
    }
    if roster.total != 3 || roster.good != 3 || roster.red() != 0 {
        return false;
    }
    if diag.total != 2 || diag.good != 2 || diag.red() != 0 {
        return false;
    }
    // 六格复刻贴在同一张 notfound 现场上：该档 total 8（活槽 2 + 复刻 6）、
    // good 2、red 6——红档全在复刻槽，活槽一根手指头都不许红。
    if notfound.total != 8 || notfound.good != 2 || notfound.red() != REJECT {
        return false;
    }
    if !notfound.reds_are_replicas() {
        return false;
    }
    let mut i = 0usize;
    while i < TIER_COUNT {
        if let Some(r) = m.rows.get(i) {
            if !r.reds_are_replicas() {
                return false;
            }
            if r.red() != r.total.saturating_sub(r.good) {
                return false;
            }
        }
        i += 1;
    }
    if m.perfect_rows() != 3 {
        return false;
    }
    // stderr 义务两档合计：诊断 2 + notfound 8 = 10（分档按实际，不按申报）。
    if m.stderr_slots() != 10 {
        return false;
    }
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r.total != CORPUS as u32 || r.accept != ACCEPT as u32 || r.reject != REJECT as u32 {
        return false;
    }
    // 报表与矩阵是两本账：四档数字必须互相指认。
    if r.quiet != quiet.good as u32
        || r.roster != roster.good as u32
        || r.diag != diag.good as u32
        || r.notfound != notfound.good as u32
    {
        return false;
    }
    if r.zero_cells() + r.nonzero_cells() != r.accept {
        return false;
    }
    if r.err_bound_cells() != m.stderr_slots() as u32 - REJECT as u32 {
        return false;
    }
    if r.matched != r.total || !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.rate() != 1000 || r.render().as_str() != RENDER_OK {
        return false;
    }
    // 注入一槽缺陷：matched 必降、rate 必离 1000、gate 必报计数错。
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if red.matched != r.matched - 1 || red.rate() != 929 {
        return false;
    }
    if red.gate() != Err(SS_ECOUNT) || red.green() {
        return false;
    }
    if red.accept != r.accept - 1 || red.reject != r.reject + 1 {
        return false;
    }
    // 注入的是静默档那格：该档正确数必须掉下来，别的档不受影响。
    if red.quiet != r.quiet - 1 {
        return false;
    }
    if red.roster != r.roster || red.diag != r.diag || red.notfound != r.notfound {
        return false;
    }
    if !matches!(run_with(Some((CORPUS, Fault::Code))), Err(SS_ESLOT)) {
        return false;
    }
    // 逐行渲染可寻址；整张矩阵的渲染逐字钉版。
    if m.row(Tier::Quiet).map(|x| x.render(Tier::Quiet)).is_none() {
        return false;
    }
    if m.render().as_str() != MATRIX_OK {
        return false;
    }
    if quiet.render(Tier::Quiet).as_str() != "quiet total=1 good=1 red=0\n" {
        return false;
    }
    if notfound.render(Tier::Notfound).as_str() != "notfound total=8 good=2 red=6\n" {
        return false;
    }
    if m.rows.get(TIER_COUNT).is_some() {
        return false;
    }
    matches!(m.row(Tier::Notfound), Some(x) if x.total == 8)
}

/// 四档矩阵全绿渲染的逐字钉版（与 `RENDER_OK` 平级的第二份文本账）。
const MATRIX_OK: &str = "quiet total=1 good=1 red=0\n\
    roster total=3 good=3 red=0\n\
    diag total=2 good=2 red=0\n\
    notfound total=8 good=2 red=6\n";

// ---------------------------------------------------------------------------
// 语料的第三份陈述：差分位
// ---------------------------------------------------------------------------

/// 差分位：申报与现场在"成败"这维不一致。
const D_ESTATE: u8 = 1;
/// 差分位：结束阶段不一致。
const D_STAGE: u8 = 2;
/// 差分位：退出码不一致。
const D_CODE: u8 = 4;
/// 差分位：污染字节宽度不一致。
const D_VALUE: u8 = 8;
/// 差分位：文案落点不一致。
const D_NOTE: u8 = 16;
/// 差分位：义务兑现位不一致。
const D_DEFAULT: u8 = 32;
/// 除成败外其余三维同时不一致（estate-swap 那格的形态：整份绿申报贴到红现场上，
/// 阶段与宽度两维恰好与现场撞对——申报者抄了模板，只敢改码与落点）。
const D_CLAIM_REST: u8 = D_CODE | D_NOTE | D_DEFAULT;

/// 14 槽申报与现场的差分掩码（语料的第三份陈述）。
///
/// 前 8 槽必须逐维等于现场（掩码 0）；后 6 槽各只允许**判序里最先撞上的那一维**
/// 决定缺陷归属，所以掩码写死在这里：任何一处手滑多改了另一维，这条就会炸开，
/// 而不是悄悄换个缺陷名继续绿。
const WANT_DIFF: [u8; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    D_ESTATE | D_CLAIM_REST,
    D_STAGE,
    D_CODE,
    D_VALUE,
    D_NOTE,
    D_DEFAULT,
];

/// 数一维：申报与现场到底差在哪几维（与 `judge()` 独立实现，互校用）。
fn dims_off(a: &Outcome, k: &Claim) -> u8 {
    let mut m = 0u8;
    if a.ok() != k.success() {
        m |= D_ESTATE;
    }
    if a.stage != k.stage {
        m |= D_STAGE;
    }
    if a.code != k.code {
        m |= D_CODE;
    }
    if a.value != k.value {
        m |= D_VALUE;
    }
    if a.note != k.note {
        m |= D_NOTE;
    }
    if a.default != k.default {
        m |= D_DEFAULT;
    }
    m
}

/// 掩码里的置位数（手写，不用 std 的 count_ones 以保持口径一致）。
fn dim_popcount(mut m: u8) -> usize {
    let mut n = 0usize;
    while m != 0 {
        if m & 1 != 0 {
            n += 1;
        }
        m = (m >> 1) as u8;
    }
    n
}

/// 掩码 → 判序里第一个撞上的缺陷（与 `judge()` 同序的独立重导）。
fn dim_fault(m: u8) -> Fault {
    if m & D_ESTATE != 0 {
        return Fault::Estate;
    }
    if m & D_STAGE != 0 {
        return Fault::Stage;
    }
    if m & D_CODE != 0 {
        return Fault::Code;
    }
    if m & D_VALUE != 0 {
        return Fault::Value;
    }
    if m & D_NOTE != 0 {
        return Fault::Note;
    }
    if m & D_DEFAULT != 0 {
        return Fault::Default;
    }
    Fault::Ok
}

/// 两处形状是否同一现场（逐判定字段比对，标签除外）。
///
/// 复刻槽的定义就是"同一现场、申报不同"；若现场本身被顺手改掉，红档就不再只
/// 指向那一维缺陷，判据的归因力被稀释。
fn same_scene(x: &Shape, y: &Shape) -> bool {
    x.name == y.name
        && x.way == y.way
        && x.state == y.state
        && x.req == y.req
        && x.err_len == y.err_len
        && x.err_fd == y.err_fd
        && x.help_len == y.help_len
        && x.help_fd == y.help_fd
        && x.merged == y.merged
        && x.err_first == y.err_first
        && x.flushed == y.flushed
}

/// 语料三份陈述（形状表、期望表、差分表）互相对账，且标签纪律成立。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if cases().len() != CORPUS {
        return false;
    }
    if cover(CORPUS) != Ok(()) {
        return false;
    }
    if cover(CORPUS - 1) != Err(SS_ENOBS) || cover(0) != Err(SS_ENOBS) {
        return false;
    }
    // 段基址的十六进制形态与常量互校（0x6d2d0 段，末码 0x6d2dd）。
    if hex_text(SEG_BASE, 5).as_str() != "6d2d0" {
        return false;
    }
    if hex_text(SS_EBAL, 5).as_str() != "6d2dd" {
        return false;
    }
    if hex_text(SEG_BASE, 6).as_str() != "06d2d0" {
        return false;
    }
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !c.shape.valid() {
            return false;
        }
        if c.shape.tag.is_empty() {
            return false;
        }
        // 复刻槽全部来自"直接调用未知名"，名字必须一致才有案发现场可比。
        if c.shape.name.is_empty() && c.shape.req != Req::Bare {
            return false;
        }
        let want_tag = match WANT_TAGS.get(i) {
            Some(t) => *t,
            None => return false,
        };
        if c.tag() != want_tag {
            return false;
        }
        let a = c.actual();
        // 期望表是"纵线实跑"的另一种写法：与实跑逐维相同才算数。
        if WANT_STAGE.get(i) != Some(&a.stage) {
            return false;
        }
        if WANT_CODE.get(i) != Some(&a.code) {
            return false;
        }
        if WANT_VALUE.get(i) != Some(&a.value) {
            return false;
        }
        if WANT_NOTE.get(i) != Some(&a.note) {
            return false;
        }
        if WANT_DEFAULT.get(i) != Some(&a.default) {
            return false;
        }
        // 活槽的申报与期望表全等；死格只在被判那一维分开。
        if i < ACCEPT {
            if WANT_NOTE.get(i) != Some(&c.claim.note) || WANT_CODE.get(i) != Some(&c.claim.code) {
                return false;
            }
            if WANT_STAGE.get(i) != Some(&c.claim.stage) || WANT_VALUE.get(i) != Some(&c.claim.value) {
                return false;
            }
            if WANT_DEFAULT.get(i) != Some(&c.claim.default) {
                return false;
            }
        }
        let m = match WANT_DIFF.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if dims_off(&a, &c.claim) != m {
            return false;
        }
        // 独立重导的判序与 judge() 必须一致（两处实现互相看住）。
        if judge(&c) != dim_fault(m) {
            return false;
        }
        if i < ACCEPT && m != 0 {
            return false;
        }
        if i >= ACCEPT && m == 0 {
            return false;
        }
        i += 1;
    }
    // 标签唯一；复刻槽一律带 -swap 后缀，活槽不带。
    let mut p = 0usize;
    while p < CORPUS {
        let t = match WANT_TAGS.get(p) {
            Some(v) => *v,
            None => return false,
        };
        let mut q = p + 1;
        while q < CORPUS {
            if let Some(u) = WANT_TAGS.get(q) {
                if *u == t {
                    return false;
                }
            }
            q += 1;
        }
        if p < ACCEPT && t.ends_with(SWAP_SUFFIX) {
            return false;
        }
        if p >= ACCEPT && !t.ends_with(SWAP_SUFFIX) {
            return false;
        }
        p += 1;
    }
    // 六格复刻同一现场：实际结果指纹互等，且等于活槽 1 的指纹；
    // 形状本身只许在标签上不同（现场不许被顺手改掉）。
    let base = match cs.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let base_seal = base.actual().seal();
    let mut k = ACCEPT;
    while k < CORPUS {
        let one = match cs.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if one.actual().seal() != base_seal {
            return false;
        }
        if !same_scene(&one.shape, &base.shape) {
            return false;
        }
        if one.shape.seal() == base.shape.seal() {
            return false;
        }
        if one.tag() == base.tag() {
            return false;
        }
        k += 1;
    }
    // 复刻槽彼此申报必须两两不同（否则"申报全同则缺陷全同"无从反证）。
    let mut x = ACCEPT;
    while x < CORPUS {
        let a = match cs.get(x) {
            Some(v) => *v,
            None => return false,
        };
        let mut y = x + 1;
        while y < CORPUS {
            if let Some(b) = cs.get(y) {
                if b.claim == a.claim {
                    return false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    // 活槽申报的独立性：前两槽现场不同，缺陷全同为 Ok 也不能互相冒充。
    base.tag() == "direct-notfound-unknown"
        && match cs.get(0) {
            Some(v) => v.actual().seal() != base_seal,
            None => false,
        }
        && dim_popcount(D_ESTATE | D_CLAIM_REST) == 4
        && dim_popcount(D_STAGE) == 1
        && dim_popcount(0) == 0
}
/// 槽位划分：活槽/死槽两半覆盖 0..14 不重不漏，值表与码表各就各位。
fn slots_ok() -> bool {
    if ACCEPT_SLOTS.len() != ACCEPT || REJECT_SLOTS.len() != REJECT {
        return false;
    }
    if ACCEPT + REJECT != CORPUS {
        return false;
    }
    if VALS.len() != ACCEPT {
        return false;
    }
    if REJECT_CODES.len() != REJECT {
        return false;
    }
    let mut seen = [false; CORPUS];
    let mut i = 0usize;
    while i < ACCEPT {
        let v = match ACCEPT_SLOTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v >= CORPUS || seen.get(v) != Some(&false) {
            return false;
        }
        if let Some(dst) = seen.get_mut(v) {
            *dst = true;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let v = match REJECT_SLOTS.get(j) {
            Some(x) => *x,
            None => return false,
        };
        if v >= CORPUS || seen.get(v) != Some(&false) {
            return false;
        }
        if let Some(dst) = seen.get_mut(v) {
            *dst = true;
        }
        j += 1;
    }
    let mut k = 0usize;
    while k < CORPUS {
        if seen.get(k) != Some(&true) {
            return false;
        }
        k += 1;
    }
    // 两半各自有序：活槽是 0..8，死槽是 8..14（复刻顺序即判序顺序）。
    let mut p = 0usize;
    while p < ACCEPT {
        match ACCEPT_SLOTS.get(p) {
            Some(x) => {
                if *x != p {
                    return false;
                }
            }
            None => return false,
        }
        match REJECT_SLOTS.get(p) {
            Some(x) => {
                if *x != ACCEPT + p {
                    return false;
                }
            }
            None => {
                if p < REJECT {
                    return false;
                }
            }
        }
        p += 1;
    }
    // expected() 与两半划分一致：活槽恒 Ok、死槽恒 alive 且互不相同。
    let mut q = 0usize;
    while q < CORPUS {
        let f = match expected(q) {
            Some(v) => v,
            None => return false,
        };
        if q < ACCEPT {
            if f != Fault::Ok {
                return false;
            }
        } else if f == Fault::Ok || !f.alive() {
            return false;
        }
        if q + 1 == CORPUS && expected(q + 1).is_some() {
            return false;
        }
        q += 1;
    }
    // 缺陷码互不相同、都在本段内、且与 Fault 枚举的 code() 同源。
    let mut a = 0usize;
    while a < REJECT {
        let c = match REJECT_CODES.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if c <= SEG_BASE || c > SEG_BASE + 0x0d {
            return false;
        }
        let f = match REJECT_FAULTS.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if f.code() != c {
            return false;
        }
        if !f.alive() {
            return false;
        }
        let mut b = a + 1;
        while b < REJECT {
            if let Some(y) = REJECT_CODES.get(b) {
                if *y == c {
                    return false;
                }
            }
            if let Some(g) = REJECT_FAULTS.get(b) {
                if *g == f {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    // 值表前三格是语料计数，末格是红档率——不许被拿来冒充统计。
    if VALS.get(0) != Some(&14) || VALS.get(1) != Some(&8) || VALS.get(2) != Some(&6) {
        return false;
    }
    if VALS.get(ACCEPT - 1) != Some(&929) {
        return false;
    }
    let mut sum = 0u32;
    let mut t = 3usize;
    while t < ACCEPT - 1 {
        if let Some(v) = VALS.get(t) {
            if *v == 0 || *v > CORPUS as u32 {
                return false;
            }
            sum += *v;
        } else {
            return false;
        }
        t += 1;
    }
    // 四档正确数之和等于活槽数（本卡的红档只在复刻槽，不占档位数）。
    sum == ACCEPT as u32 && fault_short(Fault::Ok) == "ok" && dim_popcount(0xFF) == 8
}

/// 红档显式：注入必须改变数字；不改变数字的"注入"证明 matched 是硬编码。
fn reds_ok() -> bool {
    let green = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if green.rate() != 1000 || green.matched != CORPUS as u32 {
        return false;
    }
    // 逐槽注入"真正不同的缺陷"：matched 必降一、rate 必为 929。
    let mut i = 0usize;
    while i < CORPUS {
        let want = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        // 造一个与期望不同的注入缺陷：Ok 用 Code，其余用 Ok（申报复位）。
        let forced = if want == Fault::Ok { Fault::Code } else { Fault::Ok };
        let r = match run_with(Some((i, forced))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if r.matched != green.matched - 1 {
            return false;
        }
        if r.rate() != 929 {
            return false;
        }
        if r.total != green.total || r.rate() == green.rate() {
            return false;
        }
        i += 1;
    }
    // 注入与期望同值的"假注入"不许动数字——这条抓住硬编码的 matched。
    let mut j = 0usize;
    while j < CORPUS {
        let want = match expected(j) {
            Some(v) => v,
            None => return false,
        };
        let same = match run_with(Some((j, want))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if same.matched != green.matched || same.rate() != 1000 || !same.green() {
            return false;
        }
        if same.accept != green.accept
            || same.quiet != green.quiet
            || same.roster != green.roster
            || same.diag != green.diag
            || same.notfound != green.notfound
        {
            return false;
        }
        j += 1;
    }
    // 929 的算术可复算：(13*1000 + 14/2) / 14。
    if (13u32 * 1000 + 14 / 2) / 14 != 929 {
        return false;
    }
    // 全注入满 14 次也不会把 matched 打到 0 以下；越界槽必须报槽位错。
    if !matches!(run_with(Some((0, Fault::Slot))), Ok(_)) {
        return false;
    }
    if !matches!(run_with(Some((CORPUS, Fault::Slot))), Err(SS_ESLOT)) {
        return false;
    }
    if !matches!(run_with(Some((CORPUS + 99, Fault::Code))), Err(SS_ESLOT)) {
        return false;
    }
    // 红档率不进语料统计：注入后的 reject 比绿时多一、accept 少一，两本账自洽。
    let one = match run_with(Some((1, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if one.accept + one.reject != one.total {
        return false;
    }
    if one.quiet + one.roster + one.diag + one.notfound != one.accept {
        return false;
    }
    if green.quiet + green.roster + green.diag + green.notfound != green.accept {
        return false;
    }
    // 注入 1 号槽（notfound 档）后该档正确数必须掉下来——档位账跟着实跑走。
    if one.notfound != green.notfound - 1 {
        return false;
    }
    if one.roster != green.roster || one.quiet != green.quiet || one.diag != green.diag {
        return false;
    }
    true
}
/// 文案逐字：真实表面措辞 + 本卡审计文案 + 线路字节数三方互校。
///
/// 这一条把两件事钉在一起：① 文案必须与基准机上真看到的字符串逐字相同（引号、
/// 冒号、换行、大小写都不许改）；② `WIRE_*` 那几个字节数必须由文案本身算出来，
/// 而不是另写一个魔法数——数字与文案脱钩，判据的 `value` 就成了自说自话。
fn message_ok() -> bool {
    // ---- 真实表面文案逐字 ----
    if msg_applet_not_found("frobnicate").as_str() != "applet 'frobnicate' not found" {
        return false;
    }
    if msg_no_help_available("frobnicate").as_str() != "No help available for 'frobnicate'" {
        return false;
    }
    if msg_ash_not_found("frobnicate").as_str() != "sh: frobnicate: not found" {
        return false;
    }
    if msg_usage_head().as_str() != "BusyBox v1.36.1 multi-call binary." {
        return false;
    }
    if msg_usage_line().as_str() != "usage: busybox [function] [arguments]..." {
        return false;
    }
    if msg_usage_full().as_str()
        != "BusyBox v1.36.1 multi-call binary.\nusage: busybox [function] [arguments]..."
    {
        return false;
    }
    if msg_bb_unrecognized("ls", "Z").as_str() != "ls: unrecognized option: Z" {
        return false;
    }
    if msg_roster_header().as_str() != "Currently defined functions:" {
        return false;
    }
    // ---- 线路字节数由文案本身算出（含结尾换行一个字节）----
    if msg_applet_not_found("frobnicate").len() as u32 + 1 != WIRE_NOTFOUND {
        return false;
    }
    if msg_ash_not_found("frobnicate").len() as u32 + 1 != WIRE_ASH {
        return false;
    }
    if msg_usage_head().len() as u32 + 1 != WIRE_USAGE_HEAD {
        return false;
    }
    if msg_usage_line().len() as u32 + 1 != WIRE_USAGE_LINE {
        return false;
    }
    if msg_usage_full().len() as u32 + 1 != WIRE_USAGE {
        return false;
    }
    if msg_bb_unrecognized("ls", "Z").len() as u32 + 1 != WIRE_BADOPTS {
        return false;
    }
    if msg_roster_header().len() as u32 + 1 != WIRE_ROSTER_HEAD {
        return false;
    }
    // 名单：本卡只钉总字节，正文逐字归 F11677——但表头必须是总字节的一部分。
    if WIRE_ROSTER <= WIRE_ROSTER_HEAD {
        return false;
    }
    if WIRE_ROSTER > MSG_MAX {
        return false;
    }
    // usage 全文 = 两行之和（换行归首行那笔，两笔相加即全文）。
    if WIRE_USAGE_HEAD + WIRE_USAGE_LINE != WIRE_USAGE {
        return false;
    }
    // ---- message()：八个活槽的落点文案与现场同源 ----
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        let m = message(&c.shape, &o);
        if o.note == Note::Clean {
            if !m.is_empty() {
                return false;
            }
        } else if m.is_empty() {
            return false;
        }
        if m.contains('\0') {
            return false;
        }
        i += 1;
    }
    if let Some(c) = cs.get(0) {
        if message(&c.shape, &c.actual()).len() != 0 {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c) = cs.get(1) {
        if message(&c.shape, &c.actual()).as_str() != "applet 'frobnicate' not found" {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c) = cs.get(2) {
        if message(&c.shape, &c.actual()).as_str() != "sh: frobnicate: not found" {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c) = cs.get(4) {
        if message(&c.shape, &c.actual()).as_str()
            != "BusyBox v1.36.1 multi-call binary.\nusage: ls [OPTIONS]..."
        {
            return false;
        }
        // 逐 applet usage 的字节数同样可复算：首行 + `usage: ls [OPTIONS]...`。
        if message(&c.shape, &c.actual()).len() as u32 + 1 != WIRE_APPLET_HELP {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c) = cs.get(5) {
        if message(&c.shape, &c.actual()).as_str() != "Currently defined functions:" {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c) = cs.get(6) {
        if message(&c.shape, &c.actual()).as_str() != msg_usage_full().as_str() {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c) = cs.get(7) {
        if message(&c.shape, &c.actual()).as_str() != "ls: unrecognized option: Z" {
            return false;
        }
    } else {
        return false;
    }
    // ---- 七种破断形态的审计文案逐字 ----
    let nf = match cs.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let help = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let mut flipped = nf.shape.clone_help(WIRE_USAGE_LINE, FD_OUT);
    flipped.merged = true;
    flipped.err_first = false;
    let mut unflushed = help.shape.clone_help(WIRE_USAGE, FD_OUT);
    unflushed.flushed = false;
    let cases7: [(Shape, &str); 8] = [
        (
            nf.shape.clone_err(WIRE_NOTFOUND, FD_OUT),
            "stdout contaminated by error line (30 bytes on fd 1)",
        ),
        (
            help.shape.clone_help(WIRE_USAGE, FD_ERR),
            "help text leaked to stderr (76 bytes on fd 2)",
        ),
        (
            nf.shape.clone_err(0, FD_NONE),
            "not-found error never reached stderr",
        ),
        (
            help.shape.clone_help(0, FD_NONE),
            "help text never reached stdout",
        ),
        (flipped, "merged pipe: usage landed before the error for 'frobnicate'"),
        (unflushed, "stdout buffer dropped at exit (76 bytes never flushed)"),
        (
            Shape {
                tag: "quiet-noisy",
                name: "ls",
                way: InvokeWay::BusyboxDirect,
                state: NameState::Registered,
                req: Req::Run,
                err_len: WIRE_BADOPTS,
                err_fd: FD_ERR,
                help_len: 0,
                help_fd: FD_NONE,
                merged: false,
                err_first: true,
                flushed: true,
            },
            "quiet run emitted output (27 bytes on stderr)",
        ),
        (
            {
                let mut e = Shape::blank();
                e.state = NameState::Empty;
                e.name = "";
                e.req = Req::BadOption;
                e
            },
            "busybox: empty applet name refused before lookup",
        ),
    ];
    let mut notes = [Note::Clean; 8];
    let mut z = 0usize;
    while z < cases7.len() {
        let (sh, want) = match cases7.get(z) {
            Some(v) => (v.0, v.1),
            None => return false,
        };
        let o = define(&sh);
        if !o.note.defect_family() && o.note != Note::EmptyArgv {
            return false;
        }
        if message(&sh, &o).as_str() != want {
            return false;
        }
        if let Some(dst) = notes.get_mut(z) {
            *dst = o.note;
        }
        z += 1;
    }
    // 八种形态互不相同：文案不同 ⇒ 落点也不同（一态一码，不许混档）。
    let mut x = 0usize;
    while x < notes.len() {
        let n = match notes.get(x) {
            Some(v) => *v,
            None => return false,
        };
        let mut y = x + 1;
        while y < notes.len() {
            if let Some(m) = notes.get(y) {
                if *m == n {
                    return false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    // ---- leak_report：破断才有账，正确运行零字 ----
    let leaky = nf.shape.clone_err(WIRE_NOTFOUND, FD_OUT);
    let lo = define(&leaky);
    if leak_report(&leaky, &lo).as_str() != "want stderr-only / leak=30 / note=error-on-stdout" {
        return false;
    }
    if leak_report(&nf.shape, &nf.actual()).len() != 0 {
        return false;
    }
    let help_leak = help.shape.clone_help(WIRE_USAGE, FD_ERR);
    let ho = define(&help_leak);
    if leak_report(&help_leak, &ho).as_str() != "want stdout-only / leak=76 / note=help-on-stderr" {
        return false;
    }
    // ---- 结构破断审计文案（SS_ESTREAM 口径的文本形）----
    if msg_stream_broken("error-on-stdout", WIRE_NOTFOUND, EXIT_NOTFOUND).as_str()
        != "stream attribution broken (note=error-on-stdout, leak=30, exit=127)"
    {
        return false;
    }
    // ---- 段基址文本：`0x` 前缀 + 定宽 5 位，逐字 ----
    if seg_text().as_str() != "0x6d2d0" {
        return false;
    }
    if u32_text(WIRE_NOTFOUND).as_str() != "30" || i32_text(-1).as_str() != "-1" {
        return false;
    }
    if bool_text(true) != "true" || bool_text(false) != "false" {
        return false;
    }
    true
}

/// 释义逐字：槽位的行式日志文本必须能被人读、且与现场同源。
fn detail_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let d = detail(&c);
        if d.is_empty() || !d.ends_with('\n') {
            return false;
        }
        if !d.contains("stage=exit") || !d.contains("leak=0") {
            return false;
        }
        if !d.contains(c.shape.way.text()) || !d.contains(c.shape.req.text()) {
            return false;
        }
        if !d.contains(obligation_name(c.shape.channel_of())) {
            return false;
        }
        if !d.contains(c.actual().note.name()) {
            return false;
        }
        i += 1;
    }
    // 逐字钉版：活槽 1 与活槽 0。
    if let Some(c) = cs.get(1) {
        if detail(&c).as_str()
            != "frobnicate way=busybox-direct req=run stage=exit exit=127 leak=0 \
               bound=stderr-only note=notfound-stderr msg=\"applet 'frobnicate' not found\"\n"
        {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c) = cs.get(0) {
        if detail(&c).as_str()
            != "ls way=busybox-direct req=run stage=exit exit=0 leak=0 bound=silent note=clean\n"
        {
            return false;
        }
    } else {
        return false;
    }
    // 破断现场的释义文本：leak 宽度进文本、落点族换名。
    if let Some(c) = cs.get(1) {
        let leaky = Case {
            shape: c.shape.clone_err(WIRE_NOTFOUND, FD_OUT),
            claim: a_claim_of(&define(&c.shape.clone_err(WIRE_NOTFOUND, FD_OUT))),
        };
        let d = detail(&leaky);
        if !d.contains("exit=127") || !d.contains("leak=30") {
            return false;
        }
        if !d.contains("note=error-on-stdout") {
            return false;
        }
        if d.contains("note=notfound-stderr") {
            return false;
        }
    } else {
        return false;
    }
    // 空名防御档的释义：argv 闸定案、文案是防御行、bound 仍按 verdict 前的
    // 通道义务呈现（BadOption → stderr-only），三者必须同时出现在一行里。
    let mut e = Shape::blank();
    e.state = NameState::Empty;
    e.name = "";
    e.req = Req::BadOption;
    let eo = define(&e);
    let ec = Case { shape: e, claim: a_claim_of(&eo) };
    let ed = detail(&ec);
    if !ed.contains("note=empty-argv") || !ed.contains("stage=argv") {
        return false;
    }
    if !ed.contains("msg=\"busybox: empty applet name refused before lookup\"") {
        return false;
    }
    if !ed.contains("exit=1") || !ed.contains("leak=0") {
        return false;
    }
    // 阶段文本必须与 Stage::name() 同源，不许出现第二套拼写。
    if ed.contains("stage=Argv") {
        return false;
    }
    true
}
/// 阶段闸门：七段只向前；倒退即 SS_ESTAGE，关闸后再走/再关即 SS_ESTATE。
///
/// 这道闸防的是一种很具体的假绿：申报者拿到文案之后**倒推**阶段
/// （"错误行没到，那就是 emit 拦下的"），于是同一现场在不同轮次里报出不同阶段。
/// 闸门按观测顺序记账，倒推的申报会撞在回退分支上。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.last() != -1 || g.closed() {
        return false;
    }
    let seq = [
        Stage::Argv,
        Stage::Lookup,
        Stage::Verdict,
        Stage::Emit,
        Stage::Order,
        Stage::Flush,
        Stage::Exit,
    ];
    let mut i = 0usize;
    while i < seq.len() {
        let s = match seq.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if g.step(s) != Ok(()) {
            return false;
        }
        if g.steps() != (i as u32) + 1 {
            return false;
        }
        if let Some(want) = seq.get(i) {
            if g.last() != want.order() as i32 {
                return false;
            }
        }
        i += 1;
    }
    // 同段重复前进（同一次调用里两闸同号）不算回退：闸门只管方向。
    if g.step(Stage::Exit) != Ok(()) {
        return false;
    }
    if g.steps() != 8 {
        return false;
    }
    // 回退必须撞上 SS_ESTAGE，且不记账。
    if g.step(Stage::Emit) != Err(SS_ESTAGE) {
        return false;
    }
    if g.steps() != 8 || g.last() != Stage::Exit.order() as i32 {
        return false;
    }
    if g.close() != Ok(8) {
        return false;
    }
    if !g.closed() {
        return false;
    }
    if g.step(Stage::Exit) != Err(SS_ESTATE) {
        return false;
    }
    if g.close() != Err(SS_ESTATE) {
        return false;
    }
    // 空闸门也要能关（未观测也是一种合法收尾），段数为 0。
    let mut h = Gate::new();
    if h.close() != Ok(0) || h.steps() != 0 {
        return false;
    }
    // 越级前进允许（现场只跑到 emit 就崩了）：闸门只禁回退。
    let mut k = Gate::new();
    if k.step(Stage::Emit) != Ok(()) || k.step(Stage::Exit) != Ok(()) {
        return false;
    }
    if k.close() != Ok(2) {
        return false;
    }
    // 现场侧证：语料每槽的实际阶段都落在闸序里，且 stage.order() 单调对应。
    let cs = cases();
    let mut p = 0usize;
    while p < CORPUS {
        let c = match cs.get(p) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        let mut one = Gate::new();
        if one.step(Stage::Argv) != Ok(()) || one.step(o.stage) != Ok(()) {
            return false;
        }
        if one.steps() != 2 {
            return false;
        }
        // 失败阶段与通道面的对应：Exit 之外的段都属"某道闸拦下"。
        if o.stage.failure() != (o.stage != Stage::Exit) {
            return false;
        }
        p += 1;
    }
    // 七闸的文本与秩一一对应，无重无漏。
    let mut a = 0usize;
    while a < STAGE_COUNT {
        let s = match seq.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() as usize != a {
            return false;
        }
        let mut b = a + 1;
        while b < STAGE_COUNT {
            if let Some(t) = seq.get(b) {
                if t.name() == s.name() {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    Stage::Exit.name() == "exit" && Stage::Argv.name() == "argv" && seq.len() == STAGE_COUNT
}

/// 账本：14 格逐格对账、槽位纪律、冻结重放同指纹、拒绝计数可寻。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES {
        return false;
    }
    if l.is_empty() {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.rejects() != 0 {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    // 槽位严格连续；每格的 op 与本卡的账目分工一致。
    let ops = [
        OP_CORPUS,
        OP_CORPUS,
        OP_CORPUS,
        OP_VERDICT,
        OP_EMIT,
        OP_ORDER,
        OP_EXIT,
        OP_REPLAY,
        OP_FLUSH,
        OP_FLUSH,
        OP_FLUSH,
        OP_FLUSH,
        OP_FLUSH,
        OP_FLUSH,
    ];
    let mut i = 0usize;
    while i < SAMPLES {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot as usize != i {
            return false;
        }
        let want_op = match ops.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.op != want_op {
            return false;
        }
        if i < ACCEPT {
            if !s.ok || s.code != 0 {
                return false;
            }
            if let Some(want) = VALS.get(i) {
                if s.value != *want {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            if s.ok || s.value != 0 {
                return false;
            }
            if let Some(want) = REJECT_CODES.get(i - ACCEPT) {
                if s.code != *want {
                    return false;
                }
            } else {
                return false;
            }
        }
        i += 1;
    }
    // 第七格（0 基）即红档率，必须是 929 且独占一格。
    if let Some(red) = l.sample(7) {
        if red.value != 929 {
            return false;
        }
    } else {
        return false;
    }
    let mut dup = 0usize;
    let mut a = 0usize;
    while a < SAMPLES {
        if let Some(x) = l.sample(a) {
            let mut b = a + 1;
            while b < SAMPLES {
                if let Some(y) = l.sample(b) {
                    if x.op == y.op && x.value == y.value && x.value == 929 {
                        dup += 1;
                    }
                }
                b += 1;
            }
        }
        a += 1;
    }
    if dup != 0 {
        return false;
    }
    // 越界取格返回 None（不许 panic）。
    if l.sample(SAMPLES).is_some() || l.sample(SAMPLES + 7).is_some() {
        return false;
    }
    // 指纹稳定且非零；未冻结时 root 为零。
    if l.root() != 0 || l.is_frozen() {
        return false;
    }
    let seal_a = l.seal();
    if seal_a == 0 {
        return false;
    }
    if build_ledger().seal() != seal_a {
        return false;
    }
    if l.replay() != Ok(seal_a) {
        return false;
    }
    // 冻结：root 非零、置位、重放同指纹；重复冻结被拒并计数。
    let mut f = build_ledger();
    let root = match f.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0 || !f.is_frozen() || f.root() != root {
        return false;
    }
    if f.replay() != Ok(root) {
        return false;
    }
    if f.freeze() != Err(SS_EBAL) {
        return false;
    }
    if f.rejects() != 1 {
        return false;
    }
    // 冻结后任何登记都被拒（槽号对不对都不许再写）。
    if f.register(Sample { slot: 14, op: OP_CORPUS, ok: true, code: 0, value: 1 }) != Err(SS_ESLOT) {
        return false;
    }
    if f.rejects() != 2 {
        return false;
    }
    if f.audit() != Ok(()) {
        return false;
    }
    // 槽位纪律：跳号、重号、越界一律拒，且账本长度不动。
    let mut m = Ledger::new();
    if m.len() != 0 || !m.is_empty() {
        return false;
    }
    if m.register(Sample { slot: 1, op: OP_ARGV, ok: true, code: 0, value: 1 }) != Err(SS_ESLOT) {
        return false;
    }
    if m.len() != 0 || m.rejects() != 1 {
        return false;
    }
    if m.register(Sample { slot: 0, op: OP_ARGV, ok: true, code: 0, value: 1 }) != Ok(()) {
        return false;
    }
    if m.register(Sample { slot: 0, op: OP_ARGV, ok: true, code: 0, value: 1 }) != Err(SS_ESLOT) {
        return false;
    }
    if m.len() != 1 {
        return false;
    }
    if m.register(Sample { slot: 99, op: OP_ARGV, ok: true, code: 0, value: 1 }) != Err(SS_ESLOT) {
        return false;
    }
    if m.sample(0).map(|x| x.value) != Some(1) {
        return false;
    }
    // 未记满的账本不许对账通过（长度、成数、败数三样都管）。
    if m.audit() != Err(SS_EBAL) {
        return false;
    }
    // 空账本的指纹与满账本不同（指纹对全部格敏感，不是只对项目数敏感）。
    if Ledger::new().seal() == seal_a {
        return false;
    }
    true
}

/// 交叉署名：本卡与 B24/B25 邻卡之间的段位关系自洽（位距即分配表的硬约束）。
fn anchor_ok() -> bool {
    if !anchors_agree() {
        return false;
    }
    if SEG_BASE != ANCHOR_F11664_SEG + ANCHOR_F11664_GAP {
        return false;
    }
    if SEG_BASE.checked_sub(ANCHOR_F11677_GAP) != Some(0x6d2c0) {
        return false;
    }
    if SEG_BASE.checked_add(ANCHOR_F11679_GAP) != Some(0x6d2e0) {
        return false;
    }
    if SEG_BASE.checked_add(0x20) != Some(0x6d2f0) {
        return false;
    }
    if SEG_BASE.checked_add(0x50) != Some(0x6d320) {
        return false;
    }
    if SEG_BASE.checked_add(0x60) != Some(0x6d330) {
        return false;
    }
    // 十三码全在段位窗口内、互不相同、且不含基址本身。
    let codes = [
        SS_ESTAGE,
        SS_ECODE,
        SS_EVALUE,
        SS_ENOTE,
        SS_EDEFAULT,
        SS_ESTREAM,
        SS_ESTATE,
        SS_ECOUNT,
        SS_ESLOT,
        SS_ESEAL,
        SS_ENOBS,
        SS_EMATRIX,
        SS_EBAL,
    ];
    if codes.len() != 13 {
        return false;
    }
    let mut a = 0usize;
    while a < codes.len() {
        let c = match codes.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if c <= SEG_BASE || c > SEG_BASE + 0x0d {
            return false;
        }
        let mut b = a + 1;
        while b < codes.len() {
            if let Some(y) = codes.get(b) {
                if *y == c {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    codes.get(12) == Some(&SS_EBAL) && codes.get(0) == Some(&SS_ESTAGE)
}
/// F11678「notfound stderr/stdout 分离判据」的十三项自检。
///
/// 每项一个红灯理由，全部纯计算：不读文件、不起进程、不碰时钟、不分线程。
/// 十三项与 13 个错误码一一对应——哪个理由红了，就报哪一码的口径。
pub fn run_unx11678_checks() -> CheckSet {
    let s_name = "unx11678_stderr_stdout_split";
    let mut s = CheckSet::new(s_name);

    s.add("SS01-名称与段基址", name_ok() && anchor_ok(), "static");

    s.add("SS02-枚举与七闸", enums_ok(), "static");

    s.add("SS03-通道视图与义务表", chans_ok() && scenes_ok(), "static");

    s.add("SS04-形状合法域", shape_ok(), "static");

    s.add("SS05-落点七闸纵线", define_ok(), "static");

    s.add("SS06-通道三元组归因", stream_ok(), "static");

    s.add("SS07-申报对判", judge_ok(), "static");

    s.add("SS08-矩阵统计", matrix_ok(), "static");

    s.add("SS09-语料与三份陈述", corpus_ok(), "static");

    s.add("SS10-槽位与红档显式", slots_ok() && reds_ok(), "static");

    s.add("SS11-文案逐字", message_ok(), "static");

    s.add("SS12-释义与阶段闸", detail_ok() && gate_ok(), "static");

    s.add(
        "SS13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            let four = inj.iter().all(|x| *x);
            a && b && ledger_ok() && four
        },
        "static",
    );

    s
}
// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 造形助手：只在测试里出现，不参与生产判定 ----

    /// 取一槽现场形状（越界即 panic，测试里可以接受）。
    fn scene_shape(i: usize) -> Shape {
        match case_at(i) {
            Some(c) => c.shape,
            None => panic!("槽 {} 不存在", i),
        }
    }

    /// 把形状配成申报与现场逐维全等的用例（"诚实申报"）。
    fn honest(s: &Shape) -> Case {
        let o = define(s);
        Case { shape: *s, claim: a_claim_of(&o) }
    }

    /// 未知名直接调用（本卡的主案发现场）。
    fn notfound_scene() -> Shape {
        scene_shape(1)
    }

    /// 全局帮助现场（stdout 有货、码 0）。
    fn help_scene() -> Shape {
        scene_shape(3)
    }

    /// 错误整句走 stdout 的泄漏现场。
    fn err_leak() -> Shape {
        notfound_scene().clone_err(WIRE_NOTFOUND, FD_OUT)
    }

    /// 帮助全文走 stderr 的泄漏现场。
    fn help_leak() -> Shape {
        help_scene().clone_help(WIRE_USAGE, FD_ERR)
    }

    /// 合流且 usage 抢在错误前的翻序现场。
    fn order_flip() -> Shape {
        let mut s = notfound_scene().clone_help(WIRE_USAGE_LINE, FD_OUT);
        s.merged = true;
        s.err_first = false;
        s
    }

    /// 落了字却没冲刷的假干净现场。
    fn unflushed() -> Shape {
        let mut s = help_scene().clone_help(WIRE_USAGE, FD_OUT);
        s.flushed = false;
        s
    }

    /// 伪链未注册名（降级 usage 档）。
    fn symlink_scene() -> Shape {
        let mut s = Shape::blank();
        s.tag = "t-symlink";
        s.way = InvokeWay::SymlinkArgv0;
        s.state = NameState::Unknown;
        s.name = "foo";
        s.err_len = WIRE_USAGE;
        s.err_fd = FD_ERR;
        s
    }

    // ---- 常量与段位 ----

    #[test]
    fn seg_base_is_assigned_and_codes_are_contiguous() {
        assert_eq!(SEG_BASE, 0x6d2d0);
        assert_eq!(SS_ESTAGE, SEG_BASE + 0x01);
        assert_eq!(SS_ECODE, SEG_BASE + 0x02);
        assert_eq!(SS_EVALUE, SEG_BASE + 0x03);
        assert_eq!(SS_ENOTE, SEG_BASE + 0x04);
        assert_eq!(SS_EDEFAULT, SEG_BASE + 0x05);
        assert_eq!(SS_ESTREAM, SEG_BASE + 0x06);
        assert_eq!(SS_ESTATE, SEG_BASE + 0x07);
        assert_eq!(SS_ECOUNT, SEG_BASE + 0x08);
        assert_eq!(SS_ESLOT, SEG_BASE + 0x09);
        assert_eq!(SS_ESEAL, SEG_BASE + 0x0a);
        assert_eq!(SS_ENOBS, SEG_BASE + 0x0b);
        assert_eq!(SS_EMATRIX, SEG_BASE + 0x0c);
        assert_eq!(SS_EBAL, SEG_BASE + 0x0d);
    }

    #[test]
    fn thirteen_codes_are_pairwise_distinct() {
        let codes = [
            SS_ESTAGE,
            SS_ECODE,
            SS_EVALUE,
            SS_ENOTE,
            SS_EDEFAULT,
            SS_ESTREAM,
            SS_ESTATE,
            SS_ECOUNT,
            SS_ESLOT,
            SS_ESEAL,
            SS_ENOBS,
            SS_EMATRIX,
            SS_EBAL,
        ];
        assert_eq!(codes.len(), 13);
        for (i, a) in codes.iter().enumerate() {
            assert!(*a > SEG_BASE && *a <= SEG_BASE + 0x0d, "码 {:x} 出段", *a);
            for b in codes.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn scale_constants_match_the_corpus_shape() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert_eq!(VALS.len(), ACCEPT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        assert_eq!(SCENE_COUNT, 12);
        assert_eq!(REQ_COUNT, 6);
        assert_eq!(WAY_COUNT, 3);
        assert_eq!(STATE_COUNT, 5);
        assert_eq!(STAGE_COUNT, 7);
        assert_eq!(CHANNEL_COUNT, 2);
        assert_eq!(FD_SET_SIZE, 4);
    }

    #[test]
    fn fd_and_exit_constants_are_the_real_surface() {
        assert_eq!(FD_NONE, 0);
        assert_eq!(FD_OUT, 1);
        assert_eq!(FD_ERR, 2);
        assert_eq!(FD_SILENT, 3);
        assert_eq!(EXIT_OK, 0);
        assert_eq!(EXIT_USAGE, 1);
        assert_eq!(EXIT_NOTFOUND, 127);
        assert_eq!(EXIT_POSIX, 2);
        assert_eq!(MSG_MAX, 0x4000);
        assert_eq!(BB_VERSION, "BusyBox v1.36.1");
        assert!(fd_legal(FD_OUT) && fd_legal(FD_ERR));
        assert!(!fd_legal(FD_SILENT) && !fd_legal(FD_NONE) && !fd_legal(9));
    }

    #[test]
    fn op_codes_cover_seven_gates_plus_corpus_and_replay() {
        assert_eq!(OP_ARGV, 1);
        assert_eq!(OP_LOOKUP, 2);
        assert_eq!(OP_VERDICT, 3);
        assert_eq!(OP_EMIT, 4);
        assert_eq!(OP_ORDER, 5);
        assert_eq!(OP_FLUSH, 6);
        assert_eq!(OP_EXIT, 7);
        assert_eq!(OP_CORPUS, 8);
        assert_eq!(OP_REPLAY, 9);
        let ops = [
            OP_ARGV, OP_LOOKUP, OP_VERDICT, OP_EMIT, OP_ORDER, OP_FLUSH, OP_EXIT, OP_CORPUS,
            OP_REPLAY,
        ];
        for (i, o) in ops.iter().enumerate() {
            assert_eq!(*o as usize, i + 1);
        }
    }

    #[test]
    fn wire_bytes_are_the_message_lengths_plus_newline() {
        assert_eq!(msg_applet_not_found("frobnicate").len() + 1, WIRE_NOTFOUND as usize);
        assert_eq!(msg_ash_not_found("frobnicate").len() + 1, WIRE_ASH as usize);
        assert_eq!(msg_usage_head().len() + 1, WIRE_USAGE_HEAD as usize);
        assert_eq!(msg_usage_line().len() + 1, WIRE_USAGE_LINE as usize);
        assert_eq!(msg_usage_full().len() + 1, WIRE_USAGE as usize);
        assert_eq!(msg_bb_unrecognized("ls", "Z").len() + 1, WIRE_BADOPTS as usize);
        assert_eq!(msg_roster_header().len() + 1, WIRE_ROSTER_HEAD as usize);
        assert_eq!(WIRE_USAGE_HEAD + WIRE_USAGE_LINE, WIRE_USAGE);
        assert!(WIRE_ROSTER > WIRE_ROSTER_HEAD && WIRE_ROSTER < MSG_MAX);
    }

    #[test]
    fn anchors_agree_with_the_allocation_table() {
        assert!(anchors_agree());
        assert!(anchor_ok());
        assert_eq!(ANCHOR_F11664_SEG + ANCHOR_F11664_GAP, SEG_BASE);
        assert_eq!(ANCHOR_F11677_GAP, 0x10);
        assert_eq!(ANCHOR_F11679_GAP, 0x10);
        assert_eq!(SEG_BASE - ANCHOR_F11677_GAP, 0x6d2c0);
        assert_eq!(SEG_BASE + ANCHOR_F11679_GAP, 0x6d2e0);
    }

    #[test]
    fn number_text_helpers_render_truthfully() {
        assert_eq!(dec_text(0), "0");
        assert_eq!(dec_text(7), "7");
        assert_eq!(dec_text(127), "127");
        assert_eq!(dec_text(3180), "3180");
        assert_eq!(u32_text(929), "929");
        assert_eq!(i32_text(0), "0");
        assert_eq!(i32_text(-1), "-1");
        assert_eq!(i32_text(127), "127");
        assert_eq!(hex_text(SEG_BASE, 5), "6d2d0");
        assert_eq!(hex_text(SS_EBAL, 5), "6d2dd");
        assert_eq!(hex_text(SEG_BASE, 6), "06d2d0");
        assert_eq!(seg_text(), "0x6d2d0");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
    }

    #[test]
    fn channel_and_obligation_names_cover_the_domain() {
        assert_eq!(fd_name(FD_NONE), "nowhere");
        assert_eq!(fd_name(FD_OUT), "stdout");
        assert_eq!(fd_name(FD_ERR), "stderr");
        assert_eq!(fd_name(9), "unknown");
        assert_eq!(obligation_name(FD_NONE), "unbound");
        assert_eq!(obligation_name(FD_OUT), "stdout-only");
        assert_eq!(obligation_name(FD_ERR), "stderr-only");
        assert_eq!(obligation_name(FD_SILENT), "silent");
        assert_eq!(obligation_name(4), "unknown");
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Estate), "estate");
        assert_eq!(fault_short(Fault::Stream), "stream");
        assert_eq!(fault_short(Fault::Bal), "bal");
    }

    #[test]
    fn fnv_folds_are_order_sensitive() {
        let a = fold_str(FNV_OFF, "ab");
        let b = fold_str(FNV_OFF, "ba");
        assert_ne!(a, b, "FNV-1a 是顺序敏感的：换序必须换指纹");
        assert_eq!(fold_str(FNV_OFF, ""), FNV_OFF, "空串不折任何字节");
        assert_ne!(fold_str(FNV_OFF, "a"), FNV_OFF);
        assert_eq!(fold_word(FNV_OFF, 1), fold_small(FNV_OFF, 1), "小整数与字折叠同构");
        assert_ne!(fold_word(FNV_OFF, 0), FNV_OFF);
        assert_ne!(fold_small(FNV_OFF, 1), fold_small(FNV_OFF, 2));
        assert_eq!(
            fold_byte(FNV_OFF, 0),
            FNV_OFF.wrapping_mul(FNV_PRIME).wrapping_mul(FNV_PRIME).wrapping_mul(FNV_PRIME)
                .wrapping_mul(FNV_PRIME).wrapping_mul(FNV_PRIME).wrapping_mul(FNV_PRIME)
                .wrapping_mul(FNV_PRIME).wrapping_mul(FNV_PRIME),
            "每折一个字节走八轮乘法是 FNV-1a 的定义"
        );
        assert_eq!(FNV_OFF, 0xcbf2_9ce4_8422_2325);
        assert_eq!(FNV_PRIME, 0x100_0000_01b3);
    }

    // ---- 枚举族不变量 ----

    #[test]
    fn stage_ranks_and_names_are_locked() {
        let seq = [
            Stage::Argv,
            Stage::Lookup,
            Stage::Verdict,
            Stage::Emit,
            Stage::Order,
            Stage::Flush,
            Stage::Exit,
        ];
        let names = ["argv", "lookup", "verdict", "emit", "order", "flush", "exit"];
        for (i, s) in seq.iter().enumerate() {
            assert_eq!(s.order() as usize, i);
            assert_eq!(s.name(), match names.get(i) { Some(v) => *v, None => "" });
            assert_eq!(s.failure(), *s != Stage::Exit);
            assert_eq!(s.decides_obligation(), i < 3);
            assert_eq!(s.stream_plane(), (3..6).contains(&i));
        }
    }

    #[test]
    fn note_families_are_mutually_exclusive() {
        let all = [
            Note::Clean,
            Note::NotFoundErr,
            Note::AshNotFound,
            Note::BareUsage,
            Note::DowngradeUsage,
            Note::UsageErr,
            Note::HelpOut,
            Note::ListOut,
            Note::AppletUsage,
            Note::ErrLeak,
            Note::HelpLeak,
            Note::SilentLeak,
            Note::ErrMissing,
            Note::HelpMissing,
            Note::OrderFlipped,
            Note::OutLost,
            Note::EmptyArgv,
        ];
        assert_eq!(all.len(), 17);
        for n in all.iter() {
            let mut fam = 0u8;
            if n.success() {
                fam += 1;
            }
            if n.right_family() {
                fam += 1;
            }
            if n.defect_family() {
                fam += 1;
            }
            if n.quiet_family() {
                fam += 1;
            }
            // success ⊂ right；quiet 族与 right/defect 互斥。
            let ok = (n.success() && n.right_family() && !n.defect_family() && !n.quiet_family())
                || (*n == Note::Clean || *n == Note::EmptyArgv)
                || (n.right_family() && !n.success() && !n.defect_family() && !n.quiet_family())
                || (n.defect_family() && !n.right_family() && !n.success() && !n.quiet_family());
            assert!(ok, "落点 {:?} 家族归属不唯一（{} 族）", n, fam);
            assert_eq!(n.leak_subfamily() && n.dropped_subfamily(), false);
            if n.leak_subfamily() {
                assert!(n.defect_family());
            }
            if n.dropped_subfamily() {
                assert!(n.defect_family());
            }
        }
        assert_eq!(Note::ErrLeak.msg_side(), 1);
        assert_eq!(Note::HelpLeak.msg_side(), 2);
        assert_eq!(Note::Clean.msg_side(), 0);
        assert_eq!(Note::OrderFlipped.msg_side(), 0);
        assert!(Note::NotFoundErr.success() == false);
        assert!(Note::HelpOut.success());
    }

    #[test]
    fn tier_projection_follows_code_and_note() {
        let mut o = Outcome::blank();
        assert_eq!(Tier::of(&o), Tier::Quiet);
        o.code = EXIT_NOTFOUND;
        o.note = Note::NotFoundErr;
        assert_eq!(Tier::of(&o), Tier::Notfound);
        o.code = EXIT_OK;
        o.note = Note::HelpOut;
        assert_eq!(Tier::of(&o), Tier::Roster);
        o.code = EXIT_USAGE;
        o.note = Note::BareUsage;
        assert_eq!(Tier::of(&o), Tier::Diag);
        assert_eq!(Tier::Quiet.index(), 0);
        assert_eq!(Tier::Notfound.index(), 3);
        assert_eq!(Tier::Quiet.obligation(), FD_SILENT);
        assert_eq!(Tier::Roster.obligation(), FD_OUT);
        assert_eq!(Tier::Diag.obligation(), FD_ERR);
        assert_eq!(Tier::Notfound.obligation(), FD_ERR);
        assert!(Tier::Diag.stderr_side() && Tier::Notfound.stderr_side());
        assert!(!Tier::Quiet.stderr_side() && !Tier::Roster.stderr_side());
        assert!(Tier::Quiet.success() && Tier::Roster.success());
        assert!(!Tier::Diag.success() && !Tier::Notfound.success());
        for t in [Tier::Quiet, Tier::Roster, Tier::Diag, Tier::Notfound].iter() {
            assert_ne!(t.name(), "");
        }
    }

    #[test]
    fn fault_codes_map_onto_the_thirteen_constants() {
        assert_eq!(Fault::Ok.code(), 0);
        assert!(!Fault::Ok.alive());
        assert_eq!(Fault::Stage.code(), SS_ESTAGE);
        assert_eq!(Fault::Code.code(), SS_ECODE);
        assert_eq!(Fault::Value.code(), SS_EVALUE);
        assert_eq!(Fault::Note.code(), SS_ENOTE);
        assert_eq!(Fault::Default.code(), SS_EDEFAULT);
        assert_eq!(Fault::Stream.code(), SS_ESTREAM);
        assert_eq!(Fault::Estate.code(), SS_ESTATE);
        assert_eq!(Fault::Count.code(), SS_ECOUNT);
        assert_eq!(Fault::Slot.code(), SS_ESLOT);
        assert_eq!(Fault::Seal.code(), SS_ESEAL);
        assert_eq!(Fault::Nobs.code(), SS_ENOBS);
        assert_eq!(Fault::Matrix.code(), SS_EMATRIX);
        assert_eq!(Fault::Bal.code(), SS_EBAL);
        for f in [
            Fault::Ok, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default,
            Fault::Stream, Fault::Estate, Fault::Count, Fault::Slot, Fault::Seal, Fault::Nobs,
            Fault::Matrix, Fault::Bal,
        ]
        .iter()
        {
            assert_eq!(f.alive(), *f != Fault::Ok);
        }
    }

    #[test]
    fn reject_tables_pair_fault_with_code_in_judge_order() {
        assert_eq!(
            REJECT_FAULTS,
            [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default]
        );
        assert_eq!(
            REJECT_CODES,
            [SS_ESTATE, SS_ESTAGE, SS_ECODE, SS_EVALUE, SS_ENOTE, SS_EDEFAULT]
        );
        for (i, f) in REJECT_FAULTS.iter().enumerate() {
            assert_eq!(f.code(), match REJECT_CODES.get(i) { Some(c) => *c, None => 0 });
            assert_eq!(expected(i), Some(Fault::Ok));
            assert_eq!(expected(i + ACCEPT), Some(*f));
        }
        assert_eq!(expected(CORPUS), None);
    }

    #[test]
    fn request_and_state_families_partition_the_domain() {
        let reqs = [
            Req::Run,
            Req::Bare,
            Req::Help,
            Req::List,
            Req::AppletHelp,
            Req::BadOption,
        ];
        assert_eq!(reqs.iter().filter(|r| r.help_family()).count(), 3);
        assert_eq!(reqs.iter().filter(|r| r.diag_family()).count(), 2);
        assert_eq!(reqs.iter().filter(|r| r.accepts_state(&NameState::Empty)).count(), 3);
        assert_eq!(reqs.iter().filter(|r| r.direct_only()).count(), 3);
        for r in reqs.iter() {
            assert!(!(r.help_family() && r.diag_family()));
            assert_ne!(r.text(), "");
        }
        let states = [
            NameState::Registered,
            NameState::Unknown,
            NameState::Cropped,
            NameState::CaseWrong,
            NameState::Empty,
        ];
        assert_eq!(states.iter().filter(|s| s.miss()).count(), 3);
        assert_eq!(states.iter().filter(|s| s.wants_error_line()).count(), 3);
        assert_eq!(states.iter().filter(|s| s.quoted_in_message()).count(), 4);
        for s in states.iter() {
            assert_eq!(s.miss(), s.wants_error_line());
        }
    }

    #[test]
    fn invoke_ways_report_their_real_disposition() {
        assert_eq!(InvokeWay::BusyboxDirect.unknown_code(), EXIT_NOTFOUND);
        assert_eq!(InvokeWay::SymlinkArgv0.unknown_code(), EXIT_USAGE);
        assert_eq!(InvokeWay::AshExternal.unknown_code(), EXIT_NOTFOUND);
        assert_eq!(InvokeWay::BusyboxDirect.unknown_disposition(), "notfound-127");
        assert_eq!(InvokeWay::SymlinkArgv0.unknown_disposition(), "downgrade-usage-1");
        assert_eq!(InvokeWay::AshExternal.unknown_disposition(), "shell-127");
        assert!(InvokeWay::BusyboxDirect.emits_own_error());
        assert!(!InvokeWay::AshExternal.emits_own_error());
        assert!(InvokeWay::BusyboxDirect.supports_bare());
        assert!(!InvokeWay::SymlinkArgv0.supports_bare());
        assert!(InvokeWay::SymlinkArgv0.merged_visible());
        assert!(!InvokeWay::AshExternal.merged_visible());
        assert_eq!(InvokeWay::AshExternal.text(), "ash-external");
    }

    // ---- 通道视图与义务表 ----

    #[test]
    fn channel_view_routes_bytes_by_fd() {
        let v = view_channels(WIRE_NOTFOUND, FD_ERR, 0, FD_NONE, true);
        let out = match v.get(0) { Some(c) => *c, None => Channel::blank() };
        let err = match v.get(1) { Some(c) => *c, None => Channel::blank() };
        assert_eq!(out.bytes, 0);
        assert_eq!(err.bytes, WIRE_NOTFOUND);
        assert!(out.is_stdout() && err.is_stderr());
        assert!(err.pure() && err.delivered() == err.bytes);
        let v2 = view_channels(WIRE_NOTFOUND, FD_OUT, WIRE_USAGE_LINE, FD_OUT, false);
        let out2 = match v2.get(0) { Some(c) => *c, None => Channel::blank() };
        assert_eq!(out2.bytes, WIRE_NOTFOUND + WIRE_USAGE_LINE);
        assert_eq!(out2.delivered(), 0);
        assert!(!out2.pure());
        assert!(out2.broken() == false);
        let base = channels();
        assert_eq!(base.len(), CHANNEL_COUNT);
        for c in base.iter() {
            assert_eq!(c.bytes, 0);
            assert!(c.flushed && c.pure() && !c.used());
        }
        let bad = Channel::made(FD_ERR, 10, false);
        assert!(bad.broken(), "stderr 谎称未冲刷即结构破断");
        let worse = Channel::made(FD_SILENT, 10, true);
        assert!(worse.broken());
        assert_ne!(bad.seal(), worse.seal());
    }

    #[test]
    fn scene_table_is_the_unique_obligation_source() {
        let tab = scenes();
        assert_eq!(tab.len(), SCENE_COUNT);
        assert!(scenes_ok());
        for sc in tab.iter() {
            assert_ne!(sc.tag, "");
            assert!(sc.ob == FD_ERR || sc.ob == FD_OUT || sc.ob == FD_SILENT);
            assert_eq!(sc.is_notfound(), sc.code == EXIT_NOTFOUND);
            if sc.err_bound() {
                assert!(sc.code == EXIT_NOTFOUND || sc.code == EXIT_USAGE);
                assert!(sc.must_emit);
            }
            if sc.out_bound() {
                assert_eq!(sc.code, EXIT_OK);
                assert!(sc.must_emit);
            }
        }
        assert_eq!(tab.iter().filter(|s| s.code == EXIT_NOTFOUND).count(), 4);
        assert_eq!(tab.iter().filter(|s| s.code == EXIT_USAGE).count(), 4);
        assert_eq!(tab.iter().filter(|s| s.code == EXIT_OK).count(), 4);
        assert_eq!(tab.iter().filter(|s| s.ob == FD_SILENT).count(), 1);
    }

    #[test]
    fn scene_lookup_covers_registered_combos_only() {
        let s = scene_of(InvokeWay::SymlinkArgv0, Req::Run, NameState::Unknown);
        assert_eq!(s.map(|x| x.note), Some(Note::DowngradeUsage));
        assert_eq!(s.map(|x| x.code), Some(EXIT_USAGE));
        assert_eq!(scene_of(InvokeWay::SymlinkArgv0, Req::Run, NameState::Registered), None);
        assert_eq!(scene_of(InvokeWay::SymlinkArgv0, Req::Help, NameState::Registered), None);
        assert_eq!(scene_of(InvokeWay::AshExternal, Req::Help, NameState::Registered), None);
        let ash = scene_of(InvokeWay::AshExternal, Req::Run, NameState::Unknown);
        assert_eq!(ash.map(|x| x.note), Some(Note::AshNotFound));
        assert_eq!(ash.map(|x| x.code), Some(EXIT_NOTFOUND));
        assert!(scene_requires_emit(InvokeWay::AshExternal, Req::Run, NameState::Unknown));
        assert!(!scene_requires_emit(InvokeWay::BusyboxDirect, Req::Run, NameState::Registered));
        assert!(!scene_requires_emit(InvokeWay::SymlinkArgv0, Req::Run, NameState::Empty));
        assert_eq!(scene_of(InvokeWay::BusyboxDirect, Req::Run, NameState::Empty), None);
    }

    // ---- 形状合法域与污染口径 ----

    #[test]
    fn blank_shape_is_a_quiet_legal_scene() {
        let b = Shape::blank();
        assert!(b.valid());
        assert_eq!(b.channel_of(), FD_SILENT);
        assert_eq!(b.expected_code(), EXIT_OK);
        assert_eq!(b.right_note(), Note::Clean);
        assert_eq!(b.out_landed(), 0);
        assert_eq!(b.err_landed(), 0);
        assert_eq!(b.stranded_bytes(), 0);
        assert_eq!(b.emitted_total(), 0);
        assert!(!b.order_observable());
        assert_eq!(b.wrong_bytes(FD_SILENT), 0);
        assert_eq!(b.seal(), Shape::blank().seal());
    }

    #[test]
    fn validity_rejects_self_contradictory_observations() {
        assert!(shape_ok());
        let b = Shape::blank();
        assert!(!b.clone_err(10, 7).valid(), "非法 fd 必须是坏形状");
        assert!(!b.clone_err(0, FD_ERR).valid(), "零字节却声称落了 fd");
        assert!(!b.clone_err(MSG_MAX + 1, FD_ERR).valid(), "超出文案上界");
        assert!(!b.clone_err(16, FD_NONE).valid(), "字节谁也没进");
        assert!(!b.clone_help(16, FD_NONE).valid());
        let mut run_empty = b.clone_way(InvokeWay::BusyboxDirect);
        run_empty.state = NameState::Empty;
        run_empty.name = "";
        assert!(!run_empty.valid(), "Run 不许配空名");
        let mut symlink_help = b.clone_way(InvokeWay::SymlinkArgv0);
        symlink_help.req = Req::List;
        symlink_help.state = NameState::Registered;
        assert!(!symlink_help.valid(), "伪链只许发 Run");
        let mut ash_help = b.clone_way(InvokeWay::AshExternal);
        ash_help.req = Req::Help;
        ash_help.name = "busybox";
        assert!(!ash_help.valid(), "ash 不发帮助族");
        ash_help.req = Req::Run;
        ash_help.state = NameState::Unknown;
        ash_help.name = "frobnicate";
        ash_help.err_len = WIRE_ASH;
        ash_help.err_fd = FD_ERR;
        assert!(ash_help.valid());
        let mut ash_merged = ash_help.clone_way(InvokeWay::AshExternal);
        ash_merged.merged = true;
        assert!(!ash_merged.valid(), "合流口径归 shell，本卡不许开");
    }

    #[test]
    fn wrong_bytes_uses_the_narrowed_ruler() {
        let nf = notfound_scene();
        assert_eq!(nf.wrong_bytes(FD_ERR), 0);
        assert_eq!(nf.clone_err(WIRE_NOTFOUND, FD_OUT).wrong_bytes(FD_ERR), WIRE_NOTFOUND);
        // 收窄口径：错误场景里附带 usage 走 stdout 不计污染（由 order/flush 兜底）。
        assert_eq!(nf.clone_help(WIRE_USAGE_LINE, FD_OUT).wrong_bytes(FD_ERR), 0);
        assert_eq!(nf.wrong_bytes(FD_OUT), 0);
        assert_eq!(help_scene().clone_help(WIRE_USAGE, FD_ERR).wrong_bytes(FD_OUT), WIRE_USAGE);
        assert_eq!(help_scene().wrong_bytes(FD_OUT), 0);
        assert_eq!(nf.wrong_bytes(FD_SILENT), nf.emitted_total());
        assert_eq!(nf.wrong_bytes(FD_NONE), 0);
        assert_eq!(Shape::blank().wrong_bytes(FD_SILENT), 0);
    }

    #[test]
    fn order_observation_needs_merge_and_both_doors() {
        let nf = notfound_scene();
        assert!(!nf.order_observable(), "未合流且单门：看不见次序");
        let mut merged = nf.clone_help(WIRE_USAGE_LINE, FD_OUT);
        merged.merged = true;
        assert!(merged.order_observable());
        assert_eq!(merged.out_landed(), WIRE_USAGE_LINE);
        assert_eq!(merged.err_landed(), WIRE_NOTFOUND);
        let mut both_err = nf.clone_help(WIRE_USAGE_LINE, FD_ERR);
        both_err.merged = true;
        assert!(!both_err.order_observable(), "两门同走 stderr：stdout 无货");
        assert_eq!(both_err.err_landed(), WIRE_NOTFOUND + WIRE_USAGE_LINE);
        assert!(!nf.clone_help(WIRE_USAGE_LINE, FD_OUT).order_observable());
    }

    // ---- 落点纵线 ----

    #[test]
    fn define_passes_all_live_slots_to_exit() {
        assert!(define_ok());
        for i in 0..ACCEPT {
            let c = match case_at(i) { Some(v) => v, None => panic!("槽 {}", i) };
            let o = define(&c.shape);
            assert_eq!(o.stage, Stage::Exit, "槽 {} 不该被中途拦下", i);
            assert_eq!(o, c.actual());
            assert_eq!(o.value, 0);
            assert_eq!(o.note.right_family(), true);
            assert_eq!(o.default, c.shape.channel_of() != FD_SILENT);
            assert!(o.stream_consistent(&c.shape));
            assert!(o.code_note_coherent());
            assert_eq!(o.attributed(), o.default);
        }
        assert_eq!(define(&notfound_scene()).code, EXIT_NOTFOUND);
        assert_eq!(define(&help_scene()).code, EXIT_OK);
        assert_eq!(define(&scene_shape(6)).code, EXIT_USAGE);
    }

    #[test]
    fn emit_gate_catches_leak_and_drop_forms() {
        let o = define(&err_leak());
        assert_eq!((o.stage, o.note, o.value, o.code), (Stage::Emit, Note::ErrLeak, WIRE_NOTFOUND, EXIT_NOTFOUND));
        assert!(!o.default && !o.ok());
        let h = define(&help_leak());
        assert_eq!((h.stage, h.note, h.value, h.code), (Stage::Emit, Note::HelpLeak, WIRE_USAGE, EXIT_OK));
        let d = define(&notfound_scene().clone_err(0, FD_NONE));
        assert_eq!((d.stage, d.note, d.value), (Stage::Emit, Note::ErrMissing, 0));
        let hd = define(&help_scene().clone_help(0, FD_NONE));
        assert_eq!((hd.stage, hd.note, hd.value, hd.code), (Stage::Emit, Note::HelpMissing, 0, EXIT_OK));
        let noisy = define(&Shape {
            tag: "t-quiet-noisy",
            err_len: WIRE_BADOPTS,
            err_fd: FD_ERR,
            ..Shape::blank()
        });
        assert_eq!((noisy.stage, noisy.note, noisy.value, noisy.code), (Stage::Emit, Note::SilentLeak, WIRE_BADOPTS, EXIT_OK));
        // 泄漏档的污染宽度必须与通道视图对上（一笔账，两处复算）。
        let v = view_channels(WIRE_NOTFOUND, FD_OUT, 0, FD_NONE, true);
        assert_eq!(match v.get(0) { Some(c) => c.bytes, None => 0 }, o.value);
    }

    #[test]
    fn order_and_flush_gates_are_reachable_and_ordered() {
        let f = define(&order_flip());
        assert_eq!((f.stage, f.note, f.value), (Stage::Order, Note::OrderFlipped, 0));
        assert!(!f.default && !f.ok());
        assert_eq!(f.code, EXIT_NOTFOUND);
        let mut fixed = order_flip();
        fixed.err_first = true;
        let g = define(&fixed);
        assert_eq!((g.stage, g.note), (Stage::Exit, Note::NotFoundErr));
        let u = define(&unflushed());
        assert_eq!((u.stage, u.note, u.value, u.code), (Stage::Flush, Note::OutLost, WIRE_USAGE, EXIT_OK));
        // 落错门优先于翻序：两 tuple 同时错门时报泄漏，不报次序。
        let both = notfound_scene().clone_err(WIRE_NOTFOUND, FD_OUT).clone_help(WIRE_USAGE_LINE, FD_ERR);
        let b = define(&both);
        assert_eq!((b.stage, b.note, b.value), (Stage::Emit, Note::ErrLeak, WIRE_NOTFOUND));
        // 翻序现场同时未冲刷：order 先于 flush（闸门次序不可倒）。
        let mut mixed = order_flip();
        mixed.flushed = false;
        let m = define(&mixed);
        assert_eq!((m.stage, m.note), (Stage::Order, Note::OrderFlipped));
    }

    #[test]
    fn argv_guard_short_circuits_before_settle() {
        let mut e = Shape::blank();
        e.state = NameState::Empty;
        e.name = "";
        e.req = Req::BadOption;
        let o = define(&e);
        assert_eq!((o.stage, o.note, o.code, o.value, o.default), (Stage::Argv, Note::EmptyArgv, EXIT_USAGE, 0, false));
        assert!(!o.attributed());
        assert_eq!(message(&e, &o), "busybox: empty applet name refused before lookup");
        // 裸调用不是防御档：它有产出义务，走 settle。
        let bare = scene_shape(6);
        let ob = define(&bare);
        assert_eq!((ob.stage, ob.note, ob.code), (Stage::Exit, Note::BareUsage, EXIT_USAGE));
        assert!(ob.default);
        assert_eq!(ob.value, 0, "两 tuple 同走 stderr 不计污染");
    }

    #[test]
    fn downgrade_usage_must_not_borrow_stdout() {
        let s = symlink_scene();
        let o = define(&s);
        assert_eq!((o.stage, o.code, o.note, o.default), (Stage::Exit, EXIT_USAGE, Note::DowngradeUsage, true));
        let borrowed = define(&s.clone_err(WIRE_USAGE, FD_OUT));
        assert_eq!((borrowed.stage, borrowed.note, borrowed.value), (Stage::Emit, Note::ErrLeak, WIRE_USAGE));
        assert_eq!(scene_of(InvokeWay::SymlinkArgv0, Req::Run, NameState::Unknown).map(|x| x.code), Some(EXIT_USAGE));
        assert_ne!(o.code, EXIT_NOTFOUND, "降级档不是 127（那是 F11662 的接口）");
    }

    #[test]
    fn two_paths_one_code_direct_and_ash() {
        let d = define(&notfound_scene());
        let a = define(&scene_shape(2));
        assert_eq!(d.code, a.code);
        assert_ne!(d.note, a.note);
        assert_eq!(d.note, Note::NotFoundErr);
        assert_eq!(a.note, Note::AshNotFound);
        assert_eq!(message(&scene_shape(2), &a), "sh: frobnicate: not found");
        assert!(d.default && a.default);
        // ash 的文案比直接调用短四个字节：前缀换成 "sh: "，而且名字不带引号。
        assert_eq!(
            scene_shape(2).err_len + 4,
            notfound_scene().err_len,
            "两句文案的字节差是固定的四个字节"
        );
    }

    // ---- 对判与语料 ----

    #[test]
    fn judge_accepts_every_mirrored_claim() {
        assert!(judge_ok());
        for i in 0..CORPUS {
            let c = match case_at(i) { Some(v) => v, None => panic!("槽 {}", i) };
            assert_eq!(judge(&honest(&c.shape)), Fault::Ok);
            assert_eq!(c.ok(), i < ACCEPT);
            assert_eq!(judge(&c), match expected(i) { Some(v) => v, None => Fault::Bal });
        }
    }

    #[test]
    fn judge_order_put_estate_first() {
        let s = notfound_scene();
        let a = define(&s);
        // 同时坏成败、阶段、码、宽度、落点、兑现位：报 Estate。
        let all = Case {
            shape: s,
            claim: Claim { stage: Stage::Flush, code: 0, value: 9, note: Note::Clean, default: false },
        };
        assert_eq!(judge(&all), Fault::Estate);
        assert_eq!(dims_off(&a, &all.claim) & D_ESTATE, D_ESTATE);
        // 只坏阶段。
        assert_eq!(judge(&Case { shape: s, claim: Claim { stage: Stage::Order, ..a_claim_of(&a) } }), Fault::Stage);
        // 只坏码。
        assert_eq!(judge(&Case { shape: s, claim: Claim { code: EXIT_USAGE, ..a_claim_of(&a) } }), Fault::Code);
        // 只坏宽度。
        assert_eq!(judge(&Case { shape: s, claim: Claim { value: 13, ..a_claim_of(&a) } }), Fault::Value);
        // 只坏落点（同族内换名）。
        assert_eq!(judge(&Case { shape: s, claim: Claim { note: Note::AshNotFound, ..a_claim_of(&a) } }), Fault::Note);
        // 只坏兑现位。
        assert_eq!(judge(&Case { shape: s, claim: Claim { default: false, ..a_claim_of(&a) } }), Fault::Default);
        // 全等。
        assert_eq!(judge(&Case { shape: s, claim: a_claim_of(&a) }), Fault::Ok);
        assert_eq!(dim_fault(D_ESTATE | D_DEFAULT), Fault::Estate);
        assert_eq!(dim_fault(D_DEFAULT), Fault::Default);
        assert_eq!(dim_fault(0), Fault::Ok);
        assert_eq!(dim_popcount(0), 0);
        assert_eq!(dim_popcount(0xFF), 8);
    }

    #[test]
    fn corpus_expectation_tables_match_the_run() {
        assert!(corpus_ok());
        for i in 0..CORPUS {
            let c = match case_at(i) { Some(v) => v, None => panic!("槽 {}", i) };
            let a = c.actual();
            assert_eq!(a.stage, match WANT_STAGE.get(i) { Some(v) => *v, None => Stage::Exit });
            assert_eq!(a.code, match WANT_CODE.get(i) { Some(v) => *v, None => 0 });
            assert_eq!(a.value, match WANT_VALUE.get(i) { Some(v) => *v, None => 1 });
            assert_eq!(a.note, match WANT_NOTE.get(i) { Some(v) => *v, None => Note::EmptyArgv });
            assert_eq!(a.default, match WANT_DEFAULT.get(i) { Some(v) => *v, None => true });
            assert_eq!(c.tag(), match WANT_TAGS.get(i) { Some(v) => *v, None => "" });
            assert_eq!(dims_off(&a, &c.claim), match WANT_DIFF.get(i) { Some(v) => *v, None => 9 });
        }
        assert_eq!(WANT_CODE.iter().filter(|c| **c == EXIT_NOTFOUND).count(), 8);
        assert_eq!(WANT_CODE.iter().filter(|c| **c == EXIT_POSIX).count(), 0);
        assert_eq!(WANT_TAGS.get(11), Some(&"value-swap"));
        assert_eq!(WANT_DIFF.get(8), Some(&(D_ESTATE | D_CLAIM_REST)));
    }

    #[test]
    fn replicas_share_the_scene_and_differ_only_in_claim() {
        let base = match case_at(1) { Some(c) => c, None => panic!("no slot 1") };
        for k in ACCEPT..CORPUS {
            let one = match case_at(k) { Some(c) => c, None => panic!("no slot {}", k) };
            assert!(same_scene(&one.shape, &base.shape), "槽 {} 的现场被改过", k);
            assert_eq!(one.actual().seal(), base.actual().seal());
            assert_ne!(one.shape.seal(), base.shape.seal(), "标签必须参与指纹");
            assert_ne!(one.claim, base.claim);
            assert!(one.tag().ends_with(SWAP_SUFFIX));
            assert!(!base.tag().ends_with(SWAP_SUFFIX));
        }
        assert_eq!(base.tag(), "direct-notfound-unknown");
        assert!(base.tag().ends_with("unknown") && !base.tag().ends_with(SWAP_SUFFIX));
    }

    #[test]
    fn case_helpers_stay_in_bounds() {
        assert_eq!(cases().len(), CORPUS);
        assert!(case_at(0).is_some());
        assert_eq!(case_at(CORPUS), None);
        assert_eq!(case_at(CORPUS + 5), None);
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS - 1), Err(SS_ENOBS));
        assert_eq!(cover(0), Err(SS_ENOBS));
        let b = Case::blank();
        assert_eq!(b.claim, Claim::blank());
        assert!(!b.ok());
        assert_eq!(Claim::blank().success(), true);
        assert!(!a_claim_of(&define(&err_leak())).success());
    }

    // ---- 矩阵、报表与注入 ----

    #[test]
    fn matrix_rows_match_the_corpus_distribution() {
        assert!(matrix_ok());
        let m = Matrix::of();
        assert_eq!(m.slots(), CORPUS);
        assert_eq!(m.goods(), ACCEPT);
        assert!(m.covered());
        assert_eq!(m.perfect_rows(), 3);
        assert_eq!(m.stderr_slots(), 10);
        assert_eq!(m.row(Tier::Quiet).map(|r| (r.total, r.good, r.red())), Some((1, 1, 0)));
        assert_eq!(m.row(Tier::Roster).map(|r| (r.total, r.good, r.red())), Some((3, 3, 0)));
        assert_eq!(m.row(Tier::Diag).map(|r| (r.total, r.good, r.red())), Some((2, 2, 0)));
        assert_eq!(m.row(Tier::Notfound).map(|r| (r.total, r.good, r.red())), Some((8, 2, 6)));
        assert_eq!(m.render(), MATRIX_OK);
        assert_eq!(Row::blank().red(), 0);
        assert!(!Row::blank().perfect());
    }

    #[test]
    fn report_renders_the_pinned_line_and_gates() {
        let r = match run() { Ok(v) => v, Err(_) => panic!("run 红了") };
        assert_eq!(r.render(), RENDER_OK);
        assert_eq!((r.total, r.accept, r.reject, r.matched), (14, 8, 6, 14));
        assert_eq!((r.quiet, r.roster, r.diag, r.notfound), (1, 3, 2, 2));
        assert_eq!(r.rate(), 1000);
        assert!(r.green() && r.gate() == Ok(()));
        assert_eq!(r.zero_cells(), 4);
        assert_eq!(r.nonzero_cells(), 4);
        assert_eq!(r.err_bound_cells(), 4);
        assert_eq!(r.zero_cells() + r.nonzero_cells(), r.accept);
        let empty = Report { total: 0, accept: 0, reject: 0, quiet: 0, roster: 0, diag: 0, notfound: 0, matched: 0 };
        assert_eq!(empty.rate(), 0);
        assert!(!empty.green());
        assert_eq!(empty.gate(), Err(SS_ECOUNT));
    }

    #[test]
    fn injection_moves_the_rate_off_one_thousand() {
        assert!(reds_ok());
        let green = match run() { Ok(v) => v, Err(_) => panic!("run 红了") };
        let red = match run_with(Some((0, Fault::Code))) { Ok(v) => v, Err(_) => panic!("注入越界") };
        assert_eq!(red.matched, green.matched - 1);
        assert_eq!(red.rate(), 929);
        assert_eq!(red.gate(), Err(SS_ECOUNT));
        assert!(!red.green());
        assert_eq!(red.quiet, 0);
        assert_eq!(red.accept, 7);
        assert_eq!(red.reject, 7);
        assert_eq!((13 * 1000 + 14 / 2) / 14, 929);
        assert!(matches!(run_with(Some((CORPUS, Fault::Code))), Err(SS_ESLOT)));
        // 注入与期望同值时数字不动：这条抓住硬编码的 matched。
        let fake = match run_with(Some((3, Fault::Ok))) { Ok(v) => v, Err(_) => panic!("注入越界") };
        assert_eq!(fake.matched, 14);
        assert_eq!(fake.rate(), 1000);
        assert_eq!(fake.accept, green.accept);
    }

    #[test]
    fn slots_and_vals_are_the_declared_distribution() {
        assert!(slots_ok());
        assert_eq!(VALS, [14, 8, 6, 1, 3, 2, 2, 929]);
        assert_eq!(ACCEPT_SLOTS, [0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(REJECT_SLOTS, [8, 9, 10, 11, 12, 13]);
        let r = match run() { Ok(v) => v, Err(_) => panic!("run 红了") };
        assert_eq!(
            [r.total, r.accept, r.reject, r.quiet, r.roster, r.diag, r.notfound, 929],
            VALS
        );
    }

    // ---- 文案、释义与审计文本 ----

    #[test]
    fn surface_messages_are_verbatim() {
        assert_eq!(msg_applet_not_found("frobnicate"), "applet 'frobnicate' not found");
        assert_eq!(msg_no_help_available("frobnicate"), "No help available for 'frobnicate'");
        assert_eq!(msg_ash_not_found("frobnicate"), "sh: frobnicate: not found");
        assert_eq!(msg_usage_head(), "BusyBox v1.36.1 multi-call binary.");
        assert_eq!(msg_usage_line(), "usage: busybox [function] [arguments]...");
        assert_eq!(
            msg_usage_full(),
            "BusyBox v1.36.1 multi-call binary.\nusage: busybox [function] [arguments]..."
        );
        assert_eq!(msg_bb_unrecognized("ls", "Z"), "ls: unrecognized option: Z");
        assert_eq!(msg_roster_header(), "Currently defined functions:");
        assert_eq!(
            msg_stdout_contaminated(WIRE_NOTFOUND),
            "stdout contaminated by error line (30 bytes on fd 1)"
        );
        assert_eq!(
            msg_help_leaked(WIRE_USAGE),
            "help text leaked to stderr (76 bytes on fd 2)"
        );
        assert_eq!(
            msg_quiet_run_noisy(WIRE_BADOPTS, FD_ERR),
            "quiet run emitted output (27 bytes on stderr)"
        );
        assert_eq!(msg_error_dropped(), "not-found error never reached stderr");
        assert_eq!(msg_help_dropped(), "help text never reached stdout");
        assert_eq!(
            msg_order_flipped("frobnicate"),
            "merged pipe: usage landed before the error for 'frobnicate'"
        );
        assert_eq!(msg_unflushed(WIRE_USAGE), "stdout buffer dropped at exit (76 bytes never flushed)");
        assert_eq!(
            msg_empty_argv(),
            "busybox: empty applet name refused before lookup"
        );
        assert_eq!(
            msg_stream_broken("error-on-stdout", WIRE_NOTFOUND, EXIT_NOTFOUND),
            "stream attribution broken (note=error-on-stdout, leak=30, exit=127)"
        );
    }

    #[test]
    fn message_dispatch_matches_the_gate_that_caught_it() {
        assert!(message_ok());
        let nf = notfound_scene();
        assert_eq!(message(&nf, &define(&nf)), "applet 'frobnicate' not found");
        assert_eq!(message(&scene_shape(0), &define(&scene_shape(0))), "");
        assert_eq!(message(&err_leak().clone(), &define(&err_leak())), msg_stdout_contaminated(WIRE_NOTFOUND));
        assert_eq!(message(&order_flip(), &define(&order_flip())), msg_order_flipped("frobnicate"));
        assert_eq!(message(&unflushed(), &define(&unflushed())), msg_unflushed(WIRE_USAGE));
        assert_eq!(message(&help_scene(), &define(&help_scene())), msg_usage_full());
        assert_eq!(message(&scene_shape(5), &define(&scene_shape(5))), msg_roster_header());
        assert_eq!(message(&scene_shape(4), &define(&scene_shape(4))), "BusyBox v1.36.1 multi-call binary.\nusage: ls [OPTIONS]...");
    }

    #[test]
    fn leak_report_only_writes_for_breakage() {
        let nf = notfound_scene();
        assert_eq!(leak_report(&nf, &define(&nf)), "");
        assert_eq!(
            leak_report(&err_leak(), &define(&err_leak())),
            "want stderr-only / leak=30 / note=error-on-stdout"
        );
        assert_eq!(
            leak_report(&help_leak(), &define(&help_leak())),
            "want stdout-only / leak=76 / note=help-on-stderr"
        );
        let noisy = Shape { tag: "t-noisy", err_len: WIRE_BADOPTS, err_fd: FD_ERR, ..Shape::blank() };
        assert_eq!(
            leak_report(&noisy, &define(&noisy)),
            "want silent / leak=27 / note=quiet-noisy"
        );
        assert_eq!(o_bound_after_verdict(&nf), FD_ERR);
        assert_eq!(o_bound_after_verdict(&Shape::blank()), FD_SILENT);
    }

    #[test]
    fn detail_lines_are_self_describing() {
        assert!(detail_ok());
        let c = match case_at(1) { Some(v) => v, None => panic!("no slot 1") };
        assert_eq!(
            detail(&c),
            "frobnicate way=busybox-direct req=run stage=exit exit=127 leak=0 bound=stderr-only note=notfound-stderr msg=\"applet 'frobnicate' not found\"\n"
        );
        let q = match case_at(0) { Some(v) => v, None => panic!("no slot 0") };
        assert_eq!(detail(&q), "ls way=busybox-direct req=run stage=exit exit=0 leak=0 bound=silent note=clean\n");
        let leak_case = honest(&err_leak());
        let d = detail(&leak_case);
        assert!(d.contains("leak=30") && d.contains("note=error-on-stdout") && d.ends_with('\n'));
        assert!(!d.contains("note=notfound-stderr"));
    }

    // ---- 阶段闸与账本 ----

    #[test]
    fn gate_forbids_regression_and_reopen() {
        assert!(gate_ok());
        let mut g = Gate::new();
        assert_eq!((g.steps(), g.last(), g.closed()), (0, -1, false));
        for s in [
            Stage::Argv,
            Stage::Lookup,
            Stage::Verdict,
            Stage::Emit,
            Stage::Order,
            Stage::Flush,
            Stage::Exit,
        ]
        .iter()
        {
            assert_eq!(g.step(*s), Ok(()));
        }
        assert_eq!(g.steps(), 7);
        assert_eq!(g.last(), Stage::Exit.order() as i32);
        assert_eq!(g.step(Stage::Emit), Err(SS_ESTAGE));
        assert_eq!(g.step(Stage::Argv), Err(SS_ESTAGE));
        assert_eq!(g.steps(), 7, "被拒的步不许记账");
        assert_eq!(g.close(), Ok(7));
        assert!(g.closed());
        assert_eq!(g.close(), Err(SS_ESTATE));
        assert_eq!(g.step(Stage::Exit), Err(SS_ESTATE));
        let mut h = Gate::new();
        assert_eq!(h.close(), Ok(0));
        assert_eq!(h.steps(), 0);
    }

    #[test]
    fn ledger_register_requires_the_exact_slot() {
        let mut l = Ledger::new();
        assert_eq!((l.len(), l.is_empty(), l.rejects()), (0, true, 0));
        assert!(l.sample(0).is_none());
        assert_eq!(l.audit(), Err(SS_EBAL));
        assert_eq!(l.register(Sample { slot: 3, op: OP_ARGV, ok: true, code: 0, value: 3 }), Err(SS_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 0);
        assert_eq!(l.register(Sample { slot: 0, op: OP_ARGV, ok: true, code: 0, value: 3 }), Ok(()));
        assert_eq!(l.register(Sample { slot: 0, op: OP_ARGV, ok: true, code: 0, value: 3 }), Err(SS_ESLOT));
        assert_eq!(l.len(), 1);
        assert_eq!(l.register(Sample { slot: 20, op: OP_ARGV, ok: true, code: 0, value: 3 }), Err(SS_ESLOT));
        assert_eq!(l.sample(0).map(|x| x.value), Some(3));
        assert_eq!((l.ok_count(), l.err_count()), (1, 0));
        assert_eq!(l.seal(), l.seal());
    }

    #[test]
    fn ledger_audits_fourteen_cells_against_the_tables() {
        let l = build_ledger();
        assert!(ledger_ok());
        assert_eq!((l.len(), l.ok_count(), l.err_count(), l.rejects()), (14, 8, 6, 0));
        assert_eq!(l.audit(), Ok(()));
        for i in 0..ACCEPT {
            let s = match l.sample(i) { Some(v) => v, None => panic!("格 {}", i) };
            assert!(s.ok && s.code == 0);
            assert_eq!(s.value, match VALS.get(i) { Some(v) => *v, None => 0 });
        }
        for i in ACCEPT..SAMPLES {
            let s = match l.sample(i) { Some(v) => v, None => panic!("格 {}", i) };
            assert!(!s.ok && s.value == 0);
            assert_eq!(s.code, match REJECT_CODES.get(i - ACCEPT) { Some(v) => *v, None => 0 });
        }
        assert_eq!(l.sample(7).map(|s| (s.op, s.value)), Some((OP_REPLAY, 929)));
        assert_eq!(l.sample(0).map(|s| s.op), Some(OP_CORPUS));
        assert_eq!(l.sample(3).map(|s| s.op), Some(OP_VERDICT));
        assert_eq!(l.sample(8).map(|s| s.op), Some(OP_FLUSH));
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replays_the_same_root() {
        let mut l = build_ledger();
        assert!(!l.is_frozen());
        assert_eq!(l.root(), 0);
        let before = l.seal();
        let root = match l.freeze() { Ok(v) => v, Err(_) => panic!("冻结失败") };
        assert!(root != 0 && l.is_frozen());
        assert_eq!(root, before);
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(build_ledger().seal(), before, "建账必须可重放");
        assert_eq!(l.freeze(), Err(SS_EBAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(
            l.register(Sample { slot: 14, op: OP_CORPUS, ok: true, code: 0, value: 1 }),
            Err(SS_ESLOT)
        );
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.audit(), Ok(()), "冻结后的账本仍与表对得上");
        assert_ne!(Ledger::new().seal(), before);
    }

    #[test]
    fn four_injection_paths_all_hold() {
        let inj = inject_four();
        assert_eq!(inj.len(), 4);
        for (i, v) in inj.iter().enumerate() {
            assert!(*v, "注入路 {} 静默退化", i);
        }
        assert!(name_ok() && enums_ok() && chans_ok() && scenes_ok() && shape_ok());
        assert!(define_ok() && stream_ok() && judge_ok() && matrix_ok());
        assert!(corpus_ok() && slots_ok() && reds_ok() && message_ok() && detail_ok());
        assert!(gate_ok() && ledger_ok() && anchor_ok());
    }

    #[test]
    fn thirteen_checks_are_registered_and_green() {
        let s = run_unx11678_checks();
        assert_eq!(s.domain, "unx11678_stderr_stdout_split");
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "红灯：{}", s.red_items_text());
        assert_eq!((s.passed_count(), s.failed_count()), (13, 0));
        let names: alloc::vec::Vec<&str> = s.iter().map(|c| c.name).collect();
        assert_eq!(match names.first() { Some(v) => *v, None => "" }, "SS01-名称与段基址");
        assert_eq!(
            match names.get(12) { Some(v) => *v, None => "" },
            "SS13-冻结重放四注入"
        );
        assert_eq!(names.len(), 13);
        for n in names.iter() {
            assert!(n.starts_with("SS"), "判据名必须带本卡前缀：{}", n);
        }
        for (i, n) in names.iter().enumerate() {
            for m in names.iter().skip(i + 1) {
                assert_ne!(n, m);
            }
        }
        assert_eq!(match names.get(11) { Some(v) => *v, None => "" }, "SS12-释义与阶段闸");
    }

    #[test]
    fn stream_attribution_is_consistent_across_all_forms() {
        assert!(stream_ok());
        let forms = [
            define(&err_leak()),
            define(&help_leak()),
            define(&notfound_scene().clone_err(0, FD_NONE)),
            define(&help_scene().clone_help(0, FD_NONE)),
            define(&order_flip()),
            define(&unflushed()),
            define(&Shape { tag: "t-noisy", err_len: 5, err_fd: FD_ERR, ..Shape::blank() }),
        ];
        for o in forms.iter() {
            assert!(!o.default, "破断档不许带兑现位：{:?}", o.note);
            assert!(o.note.defect_family());
            assert!(!o.ok());
            assert_eq!(o.attributed(), false);
            if o.note.leak_subfamily() {
                assert!(o.value > 0);
            }
            if o.note.dropped_subfamily() {
                assert_eq!(o.value, 0);
            }
            assert!(o.code_note_coherent(), "落点与码不同族：{:?}", o.note);
            assert_ne!(o.seal(), 0);
        }
        // 兑现位与污染宽度反着写就是结构破断，`stream_consistent` 必须拒。
        let broken = Outcome { stage: Stage::Exit, code: EXIT_OK, value: 0, note: Note::ErrLeak, default: true };
        assert!(!broken.stream_consistent(&notfound_scene()));
        assert!(!broken.attributed());
        let broken2 = Outcome { stage: Stage::Emit, code: EXIT_NOTFOUND, value: 30, note: Note::ErrMissing, default: false };
        assert!(!broken2.stream_consistent(&notfound_scene()));
        let broken3 = Outcome { stage: Stage::Exit, code: EXIT_OK, value: 7, note: Note::Clean, default: false };
        assert!(!broken3.stream_consistent(&Shape::blank()));
        // 空白结果的成败位与码同族，但不是任何现场的兑现结果。
        let b = Outcome::blank();
        assert!(b.ok() && !b.failed() && !b.notfound() && !b.default);
        assert_eq!(b.seal(), Outcome::blank().seal());
    }

    #[test]
    fn fingerprints_separate_every_observable_dimension() {
        let nf = notfound_scene();
        assert_eq!(nf.seal(), notfound_scene().seal());
        assert_ne!(nf.seal(), nf.clone_err(WIRE_NOTFOUND + 1, FD_ERR).seal());
        assert_ne!(nf.seal(), nf.clone_err(WIRE_NOTFOUND, FD_OUT).seal());
        assert_ne!(nf.seal(), nf.clone_way(InvokeWay::AshExternal).seal());
        assert_ne!(nf.seal(), nf.clone_state(NameState::Cropped).seal());
        let mut merged = nf.clone();
        merged.merged = true;
        assert_ne!(nf.seal(), merged.seal());
        let mut unflushed2 = nf.clone();
        unflushed2.flushed = false;
        assert_ne!(nf.seal(), unflushed2.seal());
        // 场景与结果的指纹各自独立：同一现场的两个落点不同指纹。
        assert_ne!(define(&nf).seal(), define(&err_leak()).seal());
        assert_ne!(nf.seal(), nf.clone_help(WIRE_USAGE_LINE, FD_OUT).seal());
    }
}
