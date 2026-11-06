//! UNX-F11666 PATH 查找失败回退判据
//!
//! shell 拿到一个命令词以后，只有两条路：**自己执行**，或者**告诉调用者它做不到**。
//! 后者又分三种完全不同的死法——"找到了但不许执行"（126）、"根本没找到"（127）、
//! "找到了、执行了、是子进程自己退的"（任意数）。本卡判的是中间那条路上最容易出
//! 事的一段：**PATH 逐目录扫描走到尽头之后发生了什么**。
//!
//! 为什么"逐目录序"本身就是一个判据：PATH 不是一个集合，而是一个**有序表**。
//! 同名可执行体出现在两个目录里时，赢的是**先扫到的那个**，于是"命中在第几个
//! 目录"是一个可观测、可复核、可以拿去和基准机对表的事实。本卡把这件事做成
//! `value`：**实际尝试过的目录数**。命中时为"命中下标 + 1"，尽头未命中时为整表
//! 长度，带 `/` 的词恒为 0（PATH 一个目录都没扫）。这三个数字把"扫到底"与
//! "提前停"分开了——一个把所有目录都 `stat` 一遍却把结果丢掉的实现，和一个
//! 只扫第一个目录就下结论的实现，在这一个数字上立刻可分辨。
//!
//! 跨 shell 的真实差异（本卡的深度所在，全部落在候选体检这一闸）：
//!
//! * **GNU bash** 走 POSIX 的"首个实体即候选"：逐目录 `stat`，只要名字在那一
//!   目录里存在（哪怕是目录、悬空符号链接、不可执行普通件），就把它当候选直接
//!   `execve`。于是 `/usr/local/bin/foo` 存在但无执行位 → `execve` 回 `EACCES`
//!   → **126 且不再往下扫**，即便 `/bin/foo` 才是那个能跑的真件。
//! * **busybox ash** 在扫表阶段就体检：非普通件、无执行位的一律**跳过继续扫**，
//!   于是同一个现场它可能扫到第三个目录才命中；一路跳完啥也没有 → **127**。
//!   同一条 `Permission denied` 现场，ash 报的是 `command not found` 127，bash
//!   报的是 `Permission denied` 126——**同一现场、两个合法数字**，谁都不许替
//!   对方改写。
//! * **dash** 按 POSIX 停在首个实体，但文案族是 `not found`（不是 bash 的
//!   `command not found`），并且带 `/bin/sh: <行号>: ` 前缀。
//!
//! "回退"在这一层有四条互斥的去路，本卡逐条建档：
//!
//! 1. **PATH 尽头 → not found + 127**：这是卡面点名的主档，归因位 `default`
//!    为真——只有"确实把表走完了"才算这一族。
//! 2. **`command_not_found_handle` 钩子**（bash 独有）：钩子被调用时 `$?` 是
//!    **钩子的返回码**，不再是 127。钩子退 0 就会出现"命令没跑成但 `$?` 是 0"
//!    这种调用方最爱误读的情形；ash/dash 没有这个钩子，配了也被忽略。
//! 3. **当前目录元素被策略拒绝**（Debian `disable-cwd` 那一类补丁）：PATH 里的
//!    空元素本意是 cwd，策略一拒，扫描**在尽头之前**就停了——于是它同样给出
//!    127，但**不许**挂 PATH 尽头的归因位。这正是 `default` 与 `code` 必须
//!    分成两维的理由。
//! 4. **哈希表陈旧**：shell 把找过的路径记进表里，文件被卸载/删掉以后再叫同名，
//!    正确的做法是回到 PATH 重扫；直接复用陈旧项的实现会给出 127，但它一个
//!    目录都没扫（`value` = 0），当场露馅。
//!
//! # 闸门次序
//!
//! `word → build → abs → path → cwd → scan → hit → exec → tail`，九段单调，
//! 后段不得在前段短路之后被"事后补记"。次序不可倒的理由：
//!
//! 1. `word` 最先——空命令词（`sh -c ''`、`foo;` 之间的空段）和纯赋值前缀
//!    （`A=1`）在 POSIX 里是**合法的空命令**，跑通、退 0，根本不该进查找；
//!    超长词（越过 `NAME_MAX`）连 `stat` 都发不出去。把这两类放进扫描段，
//!    就会得到"空字符串在 PATH 里找不到 → 127"这种经典手写 shell 错误。
//! 2. `build` 次之——内建表与命令哈希表**先于** PATH。`cd`/`true`/`:` 命中即
//!    定局，PATH 一个目录都不许扫（`value` 必须为 0）；陈旧哈希项在这里分流：
//!    回扫是正确的，直接复用是 127 且归因为假。
//! 3. `abs` 第三——词里含 `/` 时 POSIX 规定**完全不查 PATH**，直接对这个名字
//!    `execve`。这一闸必须在 PATH 之前：放在后面就会把 `/bin/zzz` 的 127 记成
//!    "扫了 3 个目录没找到"，逐目录序的账当场错一本。
//! 4. `path` 第四——PATH 的**取值形态**定案：未设（走实现默认表）、设为空串
//!    （一个目录都不扫，直接尽头的味道但不是尽头）、元素数越过本卡的上界守卫
//!    （畸形 PATH，扫到界即停）。
//! 5. `cwd` 第五——空元素代表当前目录，策略是否允许它作为候选。这一闸必须独立，
//!    因为它是"127 但不该挂 PATH 尽头归因"的第一号来源。
//! 6. `scan` 第六——逐目录推进本身：推进了几个、跳过几个、在哪个下标停。
//! 7. `hit` 第七——候选选中规则（bash 的首个实体 vs ash 的"只接受可执行普通件"）。
//!    它排在 scan 之后是必然的：没走完表就谈不上"在哪一格选中"。
//! 8. `exec` 第八——`execve` 的答复（0 / EACCES / EISDIR / ELOOP / ENOEXEC）。
//!    必须在 hit 之后：只有候选被选中，才谈得上"它执行回了什么"；反过来
//!    ENOEXEC 之后 shell 把它当脚本重跑，退出码是脚本的，本卡记档不再自造。
//! 9. `tail` 收尾——表走完了才轮到回退四去路。任何"127 + PATH 尽头归因"的申报
//!    都必须在这里被回头核对：**只要 abs/cwd/build 任一段拦下，归因位就为假**。
//!
//! # 关键差异（本卡深度所在）
//!
//! * **本卡 vs UNX-F11664**：F11664 判的是**那个数本身**（`$?` 的 126/127 档）
//!   以及三通道一致；它明确写着"行怎么逐字拼出来是 F11665 的事"。本卡判的是
//!   **这个数是在第几个目录之后给出的**：逐目录序、跳过计数、命中下标、四种
//!   提前停止。F11664 拿到 127 就能结案，本卡拿到 127 才刚开始问它扫了几个目录。
//! * **本卡 vs UNX-F11665**：F11665 判 ash 那句 `command not found` 的**逐字
//!   文案 + 127**。本卡只借它的**族属**（有没有说出"找不到"这一族），不重判
//!   字节级文案的拼装；本卡的文案函数存在只为让释义可读，判据维度是阶段/码/
//!   目录数/落点/归因五样。
//! * **本卡 vs UNX-F11667**：F11667 起是 `busybox --help` 与 usage 全文那一族，
//!   发生在**任何 PATH 查找之前**（`--help` 在主二进制层就被拦下，退 0）。
//!   本卡一次都不进 usage 面。
//! * **本卡的排他职责**：① 逐目录序的计数（`value`）；② 四种"127 但不是 PATH
//!   尽头"的落点分离（`note` + `default`）；③ bash 停首个实体 / ash 跳过续扫
//!   这一条候选策略轴；④ 钩子改写 `$?` 的档位。
//! * **谁判死谁放行**：表走完、码 127、归因位真、`value` 等于实际元素数 →
//!   放行；报 127 却声称"扫完了"而元素数对不上（提前停在 cwd 或陈旧哈希）→
//!   判死；报 0 却连一个目录都没扫又不是内建/带斜杠命中 → 判死。
//!
//! 判据六维：结束阶段（stage）、退出码（code）、尝试目录数（value）、结论落点
//! （note）、PATH 尽头归因位（default）、成败申报（estate）。判序固定：
//! 状态 → 阶段 → 码 → 目录数 → 落点 → 归因。
//!
//! 语料 14 槽：8 槽申报正确（命中 2、带斜杠 2、拒绝 1、回退 3），6 槽复刻同一
//! "PATH 走到尽头没找到"现场、各挂一维错报（estate/stage/code/value/note/default）。
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11666）。
pub const SEG_BASE: u32 = 0x6d1d0;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 档位数。
pub const TIER_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// errno（Linux x86 真值；本卡只按 errno 归族，不重判锚位）
// ---------------------------------------------------------------------------

/// 没有这个文件（`No such file or directory`）。
pub const ERR_NONE_E: i32 = 0;
/// 找不到文件（`No such file or directory`），未命中的正主。
pub const ERR_NOENT: i32 = 2;
/// 不是目录（`Not a directory`），PATH 元素本身是文件时。
pub const ERR_NOTDIR: i32 = 20;
/// 目标是目录（`Is a directory`），把目录名当命令 execve 的下场。
pub const ERR_ISDIR: i32 = 21;
/// 权限不足（`Permission denied`），找到实体但没执行位。
pub const ERR_ACCES: i32 = 13;
/// 格式错（`Exec format error`），零长件/无 shebang 走脚本重跑。
pub const ERR_NOEXEC: i32 = 8;
/// 符号链接成环（`Too many levels of symbolic links`）。
pub const ERR_LOOP: i32 = 40;
/// 名字过长（`File name too long`）。
pub const ERR_NAMETOOLONG: i32 = 36;

/// 合法 errno 集合的大小（本卡涉及的 8 个值）。
pub const ERRNO_SET_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// 退出码档
// ---------------------------------------------------------------------------

/// 跑通。
pub const EXIT_OK: u32 = 0;
/// 一般错误（钩子自身的默认失败档常用 1）。
pub const EXIT_GENERAL: u32 = 1;
/// 找到了但没法执行（POSIX 保留给 shell）。
pub const EXIT_DENIED: u32 = 126;
/// 根本找不到（POSIX 保留给 shell）。
pub const EXIT_NOTFOUND: u32 = 127;
/// shell 保留档的下沿（128+signum 归子进程，本卡不判）。
pub const EXIT_SIGNAL_BASE: u32 = 128;
/// 退出码的可表示上界（8 位）。
pub const EXIT_MASK: u32 = 255;

/// `NAME_MAX`：单个命令词还能发 `stat` 的上界。
pub const NAME_MAX: usize = 255;

/// 本卡的 PATH 元素上界守卫：超过它就扫到界即停。
pub const MAX_ELEMENTS: usize = 8;

/// PATH 目录事实表的槽数（形状里最多能声明多少个目录）。
pub const DIR_SLOTS: usize = 10;

/// PATH 未设时实现默认表的元素数（POSIX `confstr(_CS_PATH)` 的两项）。
pub const DEF_ELEMS: usize = 2;

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类：对应九道闸 + 语料 + 重放）
// ---------------------------------------------------------------------------

/// 命令词体检段。
pub const OP_WORD: u8 = 0x01;
/// 内建与哈希表段。
pub const OP_BUILD: u8 = 0x02;
/// 带斜杠直路段。
pub const OP_ABS: u8 = 0x03;
/// PATH 取值定案段。
pub const OP_PATH: u8 = 0x04;
/// 当前目录策略段。
pub const OP_CWD: u8 = 0x05;
/// 逐目录推进段。
pub const OP_SCAN: u8 = 0x06;
/// 候选选中段。
pub const OP_HIT: u8 = 0x07;
/// `execve` 答复段。
pub const OP_EXEC: u8 = 0x08;
/// 结论回退段。
pub const OP_TAIL: u8 = 0x09;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x0a;
/// 账本重放。
pub const OP_REPLAY: u8 = 0x0b;

/// 操作码个数（九道闸 + 语料 + 重放）。
pub const OP_COUNT: usize = 11;

/// 闸段总数（九道）。
pub const GATE_COUNT: usize = 9;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const PN_ESTAGE: u32 = SEG_BASE + 0x01;
/// 退出码申报不符。
pub const PN_ECODE: u32 = SEG_BASE + 0x02;
/// 尝试目录数申报不符。
pub const PN_EVALUE: u32 = SEG_BASE + 0x03;
/// 结论落点申报不符。
pub const PN_ENOTE: u32 = SEG_BASE + 0x04;
/// 归因位申报不符。
pub const PN_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 归因破断：落点说走到了尽头、闸却停在尽头之前。
pub const PN_ETAIL: u32 = SEG_BASE + 0x06;
/// 成败互报。
pub const PN_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const PN_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const PN_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const PN_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const PN_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const PN_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const PN_EBAL: u32 = SEG_BASE + 0x0d;

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

/// 折入一个有符号数（先展成 u64，保持与 fold_word 同构）。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as i64 as u64)
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

/// 有符号十进制文本。
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

/// 段基址文本（错误码定位用，带 `0x` 前缀、6 位定宽）。
fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 6).as_str());
    s
}

// ---------------------------------------------------------------------------
// 文案（真实 shell 表面措辞逐字复刻）
// ---------------------------------------------------------------------------

/// strerror(errno) 的 glibc/Linux 原文（本卡涉及的八个值）。
pub fn strerror(code: i32) -> &'static str {
    match code {
        ERR_NONE_E => "No error",
        ERR_NOENT => "No such file or directory",
        ERR_ACCES => "Permission denied",
        ERR_ISDIR => "Is a directory",
        ERR_LOOP => "Too many levels of symbolic links",
        ERR_NOEXEC => "Exec format error",
        ERR_NOTDIR => "Not a directory",
        ERR_NAMETOOLONG => "File name too long",
        _ => "",
    }
}

/// 候选事实对应的 errno（无errno 的档返回 0）。
pub fn cand_errno(c: Cand) -> i32 {
    match c {
        Cand::Absent | Cand::Dangling => ERR_NOENT,
        Cand::RegularExec => ERR_NONE_E,
        Cand::RegularNoExec | Cand::NonRegular => ERR_ACCES,
        Cand::Subdir => ERR_ISDIR,
        Cand::Loop => ERR_LOOP,
        Cand::ZeroLen => ERR_NOEXEC,
    }
}

/// 该 errno 对应的 POSIX 退出码档（126 或 127）。
pub fn errno_exit(e: i32) -> u32 {
    match e {
        ERR_NOENT => EXIT_NOTFOUND,
        ERR_ACCES | ERR_ISDIR | ERR_LOOP | ERR_NOTDIR | ERR_NAMETOOLONG => EXIT_DENIED,
        ERR_NOEXEC => EXIT_DENIED,
        _ => EXIT_OK,
    }
}

/// bash 的程序名（`argv[0]` 的 basename 约定）。
pub fn prog_name(sh: Shell) -> &'static str {
    match sh {
        Shell::Bash => "bash",
        Shell::Ash => "ash",
        Shell::Dash => "/bin/sh",
    }
}

/// 该 shell 的"找不到"主文案逐字。
///
/// dash 的行号紧跟程序名（`/bin/sh: 1: foo: not found`），bash 与 ash 不带行号
/// 且说完整的 `command not found`——两处顺序写反，文案就不是那一家 shell 写的。
pub fn msg_notfound(sh: Shell, word: &str, line: u32) -> String {
    let mut s = String::from(prog_name(sh));
    s.push_str(": ");
    match sh {
        Shell::Bash | Shell::Ash => {
            s.push_str(word);
            s.push_str(": command not found");
        }
        Shell::Dash => {
            s.push_str(dec_text(line).as_str());
            s.push_str(": ");
            s.push_str(word);
            s.push_str(": not found");
        }
    }
    s
}

/// `shell: <path>: <strerror>` 族（带路径的诊断，bash/dash/ash 同构）。
///
/// dash 同样把行号放在程序名之后、路径之前，与 `msg_notfound` 同一条排版规则。
pub fn msg_path_error(sh: Shell, path: &str, e: i32, line: u32) -> String {
    let mut s = String::from(prog_name(sh));
    s.push_str(": ");
    match sh {
        Shell::Dash => {
            s.push_str(dec_text(line).as_str());
            s.push_str(": ");
            s.push_str(path);
            s.push_str(": ");
            s.push_str(strerror(e));
        }
        Shell::Bash | Shell::Ash => {
            s.push_str(path);
            s.push_str(": ");
            s.push_str(strerror(e));
        }
    }
    s
}

/// GNU bash：找到实体但没执行位——`bash: /usr/local/bin/foo: Permission denied`。
pub fn msg_bash_denied(path: &str) -> String {
    msg_path_error(Shell::Bash, path, ERR_ACCES, 1)
}

/// busybox ash：同一现场它给的是找不到——`ash: foo: command not found`。
pub fn msg_ash_skip(word: &str) -> String {
    msg_notfound(Shell::Ash, word, 1)
}

/// dash：`/bin/sh: 1: foo: not found`。
pub fn msg_dash_notfound(word: &str, line: u32) -> String {
    msg_notfound(Shell::Dash, word, line)
}

/// 候选是目录——`bash: /usr/bin/logs: Is a directory`。
pub fn msg_is_a_directory(sh: Shell, path: &str, line: u32) -> String {
    msg_path_error(sh, path, ERR_ISDIR, line)
}

/// 符号链接成环——`bash: foo: Too many levels of symbolic links`。
pub fn msg_too_many_links(sh: Shell, path: &str, line: u32) -> String {
    msg_path_error(sh, path, ERR_LOOP, line)
}

/// 带斜杠直路落空——`bash: /tmp/nope: No such file or directory`。
pub fn msg_no_such(sh: Shell, path: &str, line: u32) -> String {
    msg_path_error(sh, path, ERR_NOENT, line)
}

/// 零长件/无 shebang：shell 按脚本重跑，成功且**无声**（返回空串）。
pub fn msg_script_ran(rc: u32) -> String {
    if rc == EXIT_OK {
        return String::new();
    }
    let mut s = String::from("script exited with ");
    s.push_str(dec_text(rc).as_str());
    s
}

/// 本卡自研审计文案：当前目录元素被策略拒绝，扫描在尽头之前停止。
pub fn msg_cwd_refused(word: &str, tried: usize) -> String {
    let mut s = String::from("cwd element refused by policy for '");
    s.push_str(word);
    s.push_str("' (");
    s.push_str(dec_text(tried as u32).as_str());
    s.push_str(" dirs tried before end, not a PATH-exhaustion verdict)");
    s
}

/// 本卡自研审计文案：PATH 元素数越过上界守卫，扫到界即停。
pub fn msg_walk_cap(used: usize, limit: usize) -> String {
    let mut s = String::from("PATH walk truncated at ");
    s.push_str(dec_text(limit as u32).as_str());
    s.push_str(" elements (spec declares ");
    s.push_str(dec_text(used as u32).as_str());
    s.push_str(", malformed: the end was never reached)");
    s
}

/// 本卡自研审计文案：直接复用了已经不存在的哈希表项。
pub fn msg_hash_stale(path: &str) -> String {
    let mut s = String::from("stale hash entry reused: '");
    s.push_str(path);
    s.push_str("' (file gone, PATH was not rescanned)");
    s
}

/// 本卡自研审计文案：钩子被调用并退了 0——命令没跑成，`$?` 却是 0。
pub fn msg_hook_zero(word: &str) -> String {
    let mut s = String::from("command_not_found_handle ran for '");
    s.push_str(word);
    s.push_str("' and returned 0 (nothing was executed)");
    s
}

/// 本卡自研审计文案：钩子的返回码顶掉了 127。
pub fn msg_hook_fail(word: &str, rc: u32) -> String {
    let mut s = String::from("command_not_found_handle ran for '");
    s.push_str(word);
    s.push_str("' and returned ");
    s.push_str(dec_text(rc).as_str());
    s.push_str(" (the 127 slot is occupied by the handler)");
    s
}

/// 本卡自研审计文案：这个 shell 根本没有钩子。
pub fn msg_hook_ignored(word: &str, sh: Shell) -> String {
    let mut s = String::from("handler configured but ");
    s.push_str(prog_name(sh));
    s.push_str(" has no command_not_found_handle for '");
    s.push_str(word);
    s.push_str("' (ignored, exit 127)");
    s
}

/// 本卡自研审计文案：命令词越界，连 `stat` 都发不出去。
///
/// `len` 是形状声明的词长（真实越界词是 300 字节的构造名，这里只把数字进文案，
/// 不把整串塞进语料表——判据要的是"没发 stat"这一事实，不是那 300 个字节本身）。
pub fn msg_bad_word(len: u32, kind: WordKind) -> String {
    match kind {
        WordKind::Overlong => {
            let mut s = String::from("command word is ");
            s.push_str(dec_text(len).as_str());
            s.push_str(" bytes, over NAME_MAX (");
            s.push_str(dec_text(NAME_MAX as u32).as_str());
            s.push_str("); no stat issued");
            s
        }
        WordKind::Empty => String::from("empty command word: POSIX legal no-op, exit 0"),
        WordKind::AssignOnly => {
            String::from("assignment-only word: POSIX legal no-op, exit 0, PATH untouched")
        }
        WordKind::Normal => String::new(),
    }
}

/// 内建命中——内建表答了话，PATH 一个目录都不许扫。
pub fn msg_builtin(word: &str) -> String {
    let mut s = String::from("builtin '");
    s.push_str(word);
    s.push_str("' answered before PATH (0 dirs consulted)");
    s
}

/// 哈希命中——记住的路径还在，直接执行。
pub fn msg_cached(word: &str, path: &str) -> String {
    let mut s = String::from("hash hit for '");
    s.push_str(word);
    s.push_str("' -> ");
    s.push_str(path);
    s.push_str(" (PATH skipped)");
    s
}

/// 逐目录序释义——命中落在第几个目录。
pub fn msg_hit(word: &str, path: &str, tried: usize) -> String {
    let mut s = String::from("exec '");
    s.push_str(word);
    s.push_str("' from ");
    s.push_str(path);
    s.push_str(" (dir #");
    s.push_str(dec_text(tried as u32).as_str());
    s.push(')');
    s
}

/// 从词里第一个斜杠起截取（含斜杠）；无斜杠则原样返回。
///
/// 用 `get(n..)` 而不是 `&word[n..]`：后者在字节不是字符边界时会 panic，
/// 而本模块的判据代码不许有 panic 路径。
pub fn trunc_word(word: &str) -> &str {
    let mut rest = word;
    while let Some(c) = rest.chars().next() {
        if c == '/' {
            return rest;
        }
        let n = c.len_utf8();
        match rest.get(n..) {
            Some(t) => rest = t,
            None => break,
        }
    }
    rest
}

/// errno 行：`errno=13 Permission denied`（数字与文本缝在一行，改一处必改两处）。
///
/// 把 `i32_text` 与 `strerror` 钉在同一句里，是为了让"只改数字不改文本"（或反之）
/// 的补丁当场红——`name_ok` 逐字比对这一行。
pub fn errno_text(code: i32) -> String {
    let mut s = String::from("errno=");
    s.push_str(i32_text(code).as_str());
    s.push(' ');
    s.push_str(strerror(code));
    s
}

/// 单个场景的指纹：形状 + 判决 + 文案三处一起折叠。
///
/// `fold_i32` 存在的意义就在这里——errno 是带符号量，走 `fold_word` 会把 -1 与
/// 0xffffffff 折叠成同一个值，符号信息丢失。
pub fn scene_seal(s: &Shape, o: &Outcome) -> u64 {
    let mut h = o.seal();
    h = fold_i32(h, o.note.errno());
    h = fold_i32(h, cand_errno(s.abs_cand));
    h = fold_word(h, s.shell as u64);
    h = fold_word(h, s.walk_len() as u64);
    h = fold_str(h, message(s, o).as_str());
    fold_word(h, Tier::of(o) as u64)
}

// ---------------------------------------------------------------------------
// 枚举：shell 面
// ---------------------------------------------------------------------------

/// 被测的是哪一套 shell。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    /// GNU bash。
    Bash,
    /// busybox ash。
    Ash,
    /// dash（POSIX `/bin/sh` 的另一实现）。
    Dash,
}

impl Shell {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Shell::Bash => "gnu-bash",
            Shell::Ash => "busybox-ash",
            Shell::Dash => "dash",
        }
    }

    /// 是否在"首个存在的实体"就停下候选（POSIX 流派：bash/dash）。
    ///
    /// busybox ash 相反：它在扫表阶段就做体检，不可执行的一律跳过继续扫，
    /// 于是同一份 PATH 可能给出 126（bash）与 127（ash）两个都是合法的答复。
    pub fn stops_on_first_entity(&self) -> bool {
        match self {
            Shell::Bash | Shell::Dash => true,
            Shell::Ash => false,
        }
    }

    /// 该 shell 是否把某个候选事实当作"可以交给 execve 的候选"。
    ///
    /// POSIX 流派只看"名字存在"；ash 流派只认可执行普通件。
    pub fn accepts(&self, c: Cand) -> bool {
        match self {
            Shell::Bash | Shell::Dash => c.exists(),
            Shell::Ash => c.executable(),
        }
    }

    /// 是否实现 `command_not_found_handle`（bash 独有）。
    pub fn has_hook(&self) -> bool {
        match self {
            Shell::Bash => true,
            Shell::Ash | Shell::Dash => false,
        }
    }

    /// 文案是否带行号（dash 的 `/bin/sh: 1: foo: not found`）。
    pub fn uses_lineno(&self) -> bool {
        match self {
            Shell::Dash => true,
            Shell::Bash | Shell::Ash => false,
        }
    }

    /// 是否默认拒绝把当前目录当候选（Debian `disable-cwd` 那一类构建）。
    pub fn refuses_cwd_by_default(&self) -> bool {
        match self {
            Shell::Bash => true,
            Shell::Ash | Shell::Dash => false,
        }
    }

    /// 次序（画像表与 `SHELL_SET` 对齐用）。
    pub fn index(&self) -> usize {
        match self {
            Shell::Bash => 0,
            Shell::Ash => 1,
            Shell::Dash => 2,
        }
    }

    /// PATH 未设时使用的实现默认表文本（POSIX `confstr(_CS_PATH)` 档）。
    pub fn default_path(&self) -> &'static str {
        match self {
            Shell::Bash | Shell::Dash => "/bin:/usr/bin",
            Shell::Ash => "/bin:/usr/bin",
        }
    }

    /// 找不到时的文案族名。
    pub fn notfound_family(&self) -> &'static str {
        match self {
            Shell::Bash => "command not found",
            Shell::Ash => "command not found",
            Shell::Dash => "not found",
        }
    }

    /// 该 shell 允许的最大 PATH 元素数（本卡的上界守卫来源）。
    pub fn walk_bound(&self) -> usize {
        match self {
            Shell::Bash => MAX_ELEMENTS,
            Shell::Ash => MAX_ELEMENTS,
            Shell::Dash => MAX_ELEMENTS,
        }
    }
}

/// 三个 shell 的画像表（判据要把形状与画像对表，不许凭印象）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellPolicy {
    /// 哪个 shell。
    pub shell: Shell,
    /// 停在首个实体。
    pub stop_first: bool,
    /// 有钩子。
    pub hook: bool,
    /// 文案带行号。
    pub lineno: bool,
    /// 默认拒绝 cwd。
    pub cwd_refused: bool,
    /// 找不到时的码档（恒 127，画像表把它钉成事实）。
    pub notfound_code: u32,
    /// 拒绝执行时的码档（恒 126）。
    pub denied_code: u32,
}

impl ShellPolicy {
    /// 画像文本。
    pub fn text(&self) -> String {
        let mut s = String::from(self.shell.text());
        s.push_str("/stop=");
        s.push_str(bool_text(self.stop_first));
        s.push_str("/hook=");
        s.push_str(bool_text(self.hook));
        s.push_str("/cwd=");
        s.push_str(bool_text(!self.cwd_refused));
        s
    }

    /// 画像声明的候选策略与 `Shell::accepts` 是否同族。
    ///
    /// 这一条是画像表存在的唯一理由：表说"停在首个实体"，`accepts()` 就必须
    /// 对"存在但不可执行"给出真；两处任一处改错，本函数给出假。
    pub fn agrees_with(&self, sh: Shell) -> bool {
        if self.shell != sh {
            return false;
        }
        if self.stop_first != sh.stops_on_first_entity() {
            return false;
        }
        if self.hook != sh.has_hook() {
            return false;
        }
        if self.lineno != sh.uses_lineno() {
            return false;
        }
        if self.cwd_refused != sh.refuses_cwd_by_default() {
            return false;
        }
        let entity = sh.accepts(Cand::RegularNoExec);
        if entity != self.stop_first {
            return false;
        }
        sh.accepts(Cand::RegularExec) && !sh.accepts(Cand::Absent)
    }
}

/// 三个 shell 的画像（顺序与 `Shell::index()` 对齐）。
pub fn policies() -> [ShellPolicy; 3] {
    [
        ShellPolicy {
            shell: Shell::Bash,
            stop_first: true,
            hook: true,
            lineno: false,
            cwd_refused: true,
            notfound_code: EXIT_NOTFOUND,
            denied_code: EXIT_DENIED,
        },
        ShellPolicy {
            shell: Shell::Ash,
            stop_first: false,
            hook: false,
            lineno: false,
            cwd_refused: false,
            notfound_code: EXIT_NOTFOUND,
            denied_code: EXIT_DENIED,
        },
        ShellPolicy {
            shell: Shell::Dash,
            stop_first: true,
            hook: false,
            lineno: true,
            cwd_refused: false,
            notfound_code: EXIT_NOTFOUND,
            denied_code: EXIT_DENIED,
        },
    ]
}

/// 命令词的形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordKind {
    /// 正常词，可以进查找。
    Normal,
    /// 空词（`sh -c ''`、`a;;b` 的空段）：POSIX 合法空命令。
    Empty,
    /// 只有赋值前缀（`A=1`）：POSIX 合法空命令，PATH 不参与。
    AssignOnly,
    /// 越过 `NAME_MAX`：连 `stat` 都发不出去。
    Overlong,
}

impl WordKind {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            WordKind::Normal => "normal",
            WordKind::Empty => "empty",
            WordKind::AssignOnly => "assign-only",
            WordKind::Overlong => "overlong",
        }
    }

    /// 是否算"合法的空命令"（成功档，且不进查找）。
    pub fn is_noop(&self) -> bool {
        match self {
            WordKind::Empty | WordKind::AssignOnly => true,
            WordKind::Normal | WordKind::Overlong => false,
        }
    }

    /// 是否该进 PATH 查找。
    pub fn searchable(&self) -> bool {
        match self {
            WordKind::Normal => true,
            WordKind::Empty | WordKind::AssignOnly | WordKind::Overlong => false,
        }
    }

    /// 次序（判据表用）。
    pub fn index(&self) -> usize {
        match self {
            WordKind::Normal => 0,
            WordKind::Empty => 1,
            WordKind::AssignOnly => 2,
            WordKind::Overlong => 3,
        }
    }
}

/// PATH 变量本身的取值形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathState {
    /// 已设为一份表。
    Set,
    /// 未设置：走实现默认表。
    Unset,
    /// 设为空串：一个目录都不扫。
    Empty,
}

impl PathState {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            PathState::Set => "set",
            PathState::Unset => "unset",
            PathState::Empty => "empty-string",
        }
    }

    /// 是否"PATH 根本没参与"（此时尝试目录数必须为 0）。
    pub fn no_search(&self) -> bool {
        match self {
            PathState::Empty => true,
            PathState::Set | PathState::Unset => false,
        }
    }

    /// 该形态下要扫几个目录（`used` 是形状声明的元素数）。
    pub fn elements(&self, used: usize) -> usize {
        match self {
            PathState::Set => used,
            PathState::Unset => {
                if used < DEF_ELEMS {
                    used
                } else {
                    DEF_ELEMS
                }
            }
            PathState::Empty => 0,
        }
    }
}

/// 命令哈希表的状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashState {
    /// 没记过（首次查找）。
    Miss,
    /// 记过且文件还在。
    Fresh,
    /// 记过但文件已不在（卸载、`rm`、换 inode）。
    Stale,
}

impl HashState {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            HashState::Miss => "uncached",
            HashState::Fresh => "cached",
            HashState::Stale => "stale",
        }
    }

    /// 是否需要在 PATH 之前就有答复。
    pub fn answers_first(&self) -> bool {
        match self {
            HashState::Fresh => true,
            HashState::Miss | HashState::Stale => false,
        }
    }

    /// 陈旧项复用是否合法（永远不合法：必须回扫）。
    pub fn reuse_is_ok(&self) -> bool {
        match self {
            HashState::Stale => false,
            HashState::Miss | HashState::Fresh => true,
        }
    }
}

/// PATH 尽头回退时挂着的钩子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookState {
    /// 没定义钩子。
    Absent,
    /// 定义了且返回 0。
    Zero,
    /// 定义了且返回非 0。
    NonZero,
}

impl HookState {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            HookState::Absent => "no-handler",
            HookState::Zero => "handler-0",
            HookState::NonZero => "handler-nz",
        }
    }

    /// 是否会在尽头改写 `$?`。
    pub fn overrides_rc(&self) -> bool {
        match self {
            HookState::Absent => false,
            HookState::Zero | HookState::NonZero => true,
        }
    }

    /// 钩子答复的码（`rc` 是形状声明的钩子返回码）。
    pub fn code(&self, rc: u32) -> Option<u32> {
        match self {
            HookState::Absent => None,
            HookState::Zero => Some(EXIT_OK),
            HookState::NonZero => Some(rc),
        }
    }
}

/// 某个 PATH 目录里，该命令名对应的事实。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cand {
    /// 根本没有这个名字（`ENOENT`）。
    Absent,
    /// 符号链接指向不存在的目标（`ENOENT`，但 `lstat` 看得见）。
    Dangling,
    /// 同名的是个目录（`EISDIR`）。
    Subdir,
    /// 普通件但没有执行位（`EACCES`）。
    RegularNoExec,
    /// 普通件、可执行——真候选。
    RegularExec,
    /// 非普通件（FIFO/socket/设备），`execve` 给 `EACCES`。
    NonRegular,
    /// 符号链接成环（`ELOOP`）。
    Loop,
    /// 零长件或无 shebang（`ENOEXEC`，shell 按脚本重跑）。
    ZeroLen,
}

impl Cand {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Cand::Absent => "absent",
            Cand::Dangling => "dangling-link",
            Cand::Subdir => "subdir",
            Cand::RegularNoExec => "noexec-regular",
            Cand::RegularExec => "regular-exec",
            Cand::NonRegular => "non-regular",
            Cand::Loop => "link-loop",
            Cand::ZeroLen => "zero-length",
        }
    }

    /// 名字在这个目录里"存在"吗（POSIX 流派的候选判据）。
    ///
    /// `Absent` 是唯一一个不存在的档：`Dangling` 的 `lstat` 看得见名字，
    /// 所以 bash 会把它当候选并拿到 `ENOENT` —— 这正是 127 的第二个来源。
    pub fn exists(&self) -> bool {
        match self {
            Cand::Absent => false,
            Cand::Dangling
            | Cand::Subdir
            | Cand::RegularNoExec
            | Cand::RegularExec
            | Cand::NonRegular
            | Cand::Loop
            | Cand::ZeroLen => true,
        }
    }

    /// 这个档能不能直接跑起来（ash 流派的候选判据）。
    pub fn executable(&self) -> bool {
        match self {
            Cand::RegularExec => true,
            Cand::Absent
            | Cand::Dangling
            | Cand::Subdir
            | Cand::RegularNoExec
            | Cand::NonRegular
            | Cand::Loop
            | Cand::ZeroLen => false,
        }
    }

    /// 是否"扫过去就跳过、继续下一个目录"的档（对 ash 而言）。
    pub fn ash_skips(&self) -> bool {
        !self.executable()
    }

    /// 对应的 errno。
    pub fn errno(&self) -> i32 {
        cand_errno(*self)
    }

    /// 对应码档（0/126/127）。
    pub fn exit_code(&self) -> u32 {
        match self {
            Cand::RegularExec => EXIT_OK,
            Cand::Absent | Cand::Dangling => EXIT_NOTFOUND,
            Cand::Subdir | Cand::RegularNoExec | Cand::NonRegular | Cand::Loop => EXIT_DENIED,
            Cand::ZeroLen => EXIT_DENIED,
        }
    }

    /// 是否属于"126 族"（找到了但没法执行）。
    pub fn denied_family(&self) -> bool {
        match self {
            Cand::Subdir | Cand::RegularNoExec | Cand::NonRegular | Cand::Loop => true,
            Cand::Absent | Cand::Dangling | Cand::RegularExec | Cand::ZeroLen => false,
        }
    }

    /// 次序（判据表用）。
    pub fn index(&self) -> usize {
        match self {
            Cand::Absent => 0,
            Cand::Dangling => 1,
            Cand::Subdir => 2,
            Cand::RegularNoExec => 3,
            Cand::RegularExec => 4,
            Cand::NonRegular => 5,
            Cand::Loop => 6,
            Cand::ZeroLen => 7,
        }
    }
}

/// 候选事实的完整枚举（判据要逐档体检，不许漏档）。
pub const CAND_SET: [Cand; 8] = [
    Cand::Absent,
    Cand::Dangling,
    Cand::Subdir,
    Cand::RegularNoExec,
    Cand::RegularExec,
    Cand::NonRegular,
    Cand::Loop,
    Cand::ZeroLen,
];

/// 全部 shell 的枚举（判据要逐 shell 体检）。
pub const SHELL_SET: [Shell; 3] = [Shell::Bash, Shell::Ash, Shell::Dash];

// ---------------------------------------------------------------------------
// 枚举：九道闸
// ---------------------------------------------------------------------------

/// 一次查找结束在哪一闸。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 命令词体检。
    Word,
    /// 内建表 / 命令哈希表。
    Build,
    /// 词含 `/`：完全不查 PATH。
    Abs,
    /// PATH 取值定案。
    Path,
    /// 当前目录元素策略。
    Cwd,
    /// 逐目录推进。
    Scan,
    /// 候选选中。
    Hit,
    /// `execve` 答复。
    Exec,
    /// PATH 尽头回退。
    Tail,
}

impl Stage {
    /// 次序（闸门单调性的根据）。
    pub fn order(&self) -> usize {
        match self {
            Stage::Word => 0,
            Stage::Build => 1,
            Stage::Abs => 2,
            Stage::Path => 3,
            Stage::Cwd => 4,
            Stage::Scan => 5,
            Stage::Hit => 6,
            Stage::Exec => 7,
            Stage::Tail => 8,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Word => "word",
            Stage::Build => "builtin-hash",
            Stage::Abs => "absolute",
            Stage::Path => "path-value",
            Stage::Cwd => "cwd-policy",
            Stage::Scan => "dir-scan",
            Stage::Hit => "candidate",
            Stage::Exec => "execve",
            Stage::Tail => "path-end",
        }
    }

    /// 是否失败落点（走到 Tail 之后仍报 0 的场合由 note 决定，不在这里判）。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Word | Stage::Build | Stage::Abs | Stage::Path | Stage::Cwd | Stage::Scan
            | Stage::Hit | Stage::Exec => true,
            Stage::Tail => false,
        }
    }

    /// 是否"PATH 真的被扫过"的段（scan 及其之后，减去 abs/build 的旁路）。
    pub fn searched(&self) -> bool {
        match self {
            Stage::Scan | Stage::Hit | Stage::Exec | Stage::Tail => true,
            Stage::Word | Stage::Build | Stage::Abs | Stage::Path | Stage::Cwd => false,
        }
    }

    /// 是否"一个目录都不该扫"的段（此时 `value` 必须为 0）。
    pub fn zero_dirs(&self) -> bool {
        match self {
            Stage::Word | Stage::Build | Stage::Abs | Stage::Path => true,
            Stage::Cwd | Stage::Scan | Stage::Hit | Stage::Exec | Stage::Tail => false,
        }
    }

    /// 该段能不能给出"PATH 尽头"归因。
    pub fn attributable(&self) -> bool {
        match self {
            Stage::Tail => true,
            Stage::Word | Stage::Build | Stage::Abs | Stage::Path | Stage::Cwd | Stage::Scan
            | Stage::Hit | Stage::Exec => false,
        }
    }

    /// 由下标取回一段（越界给 `None`，判据表用）。
    pub fn at(i: usize) -> Option<Stage> {
        match i {
            0 => Some(Stage::Word),
            1 => Some(Stage::Build),
            2 => Some(Stage::Abs),
            3 => Some(Stage::Path),
            4 => Some(Stage::Cwd),
            5 => Some(Stage::Scan),
            6 => Some(Stage::Hit),
            7 => Some(Stage::Exec),
            8 => Some(Stage::Tail),
            _ => None,
        }
    }
}

/// 九道闸的固定次序表（判据要按这张表逐段推进）。
pub const GATE_LINE: [Stage; GATE_COUNT] = [
    Stage::Word,
    Stage::Build,
    Stage::Abs,
    Stage::Path,
    Stage::Cwd,
    Stage::Scan,
    Stage::Hit,
    Stage::Exec,
    Stage::Tail,
];

// ---------------------------------------------------------------------------
// 枚举：结论落点
// ---------------------------------------------------------------------------

/// 最后那一句结论落在哪一档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 命中可执行普通件并跑通。
    Hit,
    /// 词含 `/`，直接执行成功。
    Direct,
    /// 词含 `/`，直接执行落空（127，但不是 PATH 尽头）。
    DirectMiss,
    /// 内建答复，PATH 未参与。
    Builtin,
    /// 哈希表答复，PATH 未参与。
    Cached,
    /// 复用了陈旧哈希项（127，未回扫）。
    HashStale,
    /// 空命令词（合法 0）。
    EmptyWord,
    /// 纯赋值词（合法 0）。
    AssignOnly,
    /// 词形畸形（越 `NAME_MAX`）。
    BadName,
    /// 找到实体但没执行位（126）。
    Denied,
    /// 同名是目录（126）。
    Subdir,
    /// 符号链接成环（126）。
    Loop,
    /// 悬空符号链接：名字存在、`execve` 回 `ENOENT`（127，但不是 PATH 尽头）。
    LinkGone,
    /// `ENOEXEC`：shell 按脚本重跑。
    Script,
    /// 当前目录被策略拒绝，扫描在尽头之前停止（127）。
    CwdBlocked,
    /// PATH 元素数越守卫，扫到界即停（127）。
    WalkCap,
    /// PATH 尽头，没找到（127，归因位真）。
    NotFound,
    /// 尽头钩子退 0（`$?` = 0，命令其实没跑）。
    HookOk,
    /// 尽头钩子退非 0（`$?` = 钩子返回码）。
    HookFail,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Hit => "exec-hit",
            Note::Direct => "direct-exec",
            Note::DirectMiss => "direct-miss",
            Note::Builtin => "builtin",
            Note::Cached => "hash-hit",
            Note::HashStale => "hash-stale",
            Note::EmptyWord => "empty-word",
            Note::AssignOnly => "assign-only",
            Note::BadName => "bad-word",
            Note::Denied => "perm-denied",
            Note::Subdir => "is-a-directory",
            Note::Loop => "link-loop",
            Note::LinkGone => "link-gone",
            Note::Script => "enoexec-script",
            Note::CwdBlocked => "cwd-refused",
            Note::WalkCap => "walk-capped",
            Note::NotFound => "not-found",
            Note::HookOk => "handler-zero",
            Note::HookFail => "handler-fail",
        }
    }

    /// 是否属于"跑成了"这一族（`$?` 该是 0）。
    pub fn success(&self) -> bool {
        match self {
            Note::Hit | Note::Direct | Note::Builtin | Note::Cached | Note::EmptyWord
            | Note::AssignOnly | Note::HookOk => true,
            Note::Script => true,
            Note::DirectMiss | Note::HashStale | Note::BadName | Note::Denied | Note::Subdir
            | Note::Loop | Note::LinkGone | Note::CwdBlocked | Note::WalkCap | Note::NotFound
            | Note::HookFail => false,
        }
    }

    /// 是否属于"127 档"（找不到）的文案族。
    pub fn notfound_family(&self) -> bool {
        match self {
            Note::NotFound | Note::DirectMiss | Note::HashStale | Note::CwdBlocked
            | Note::WalkCap | Note::LinkGone => true,
            Note::Hit | Note::Direct | Note::Builtin | Note::Cached | Note::EmptyWord
            | Note::AssignOnly | Note::BadName | Note::Denied | Note::Subdir | Note::Loop
            | Note::Script | Note::HookOk | Note::HookFail => false,
        }
    }

    /// 是否属于"126 档"（不许执行）的文案族。
    pub fn denied_family(&self) -> bool {
        match self {
            Note::Denied | Note::Subdir | Note::Loop => true,
            Note::Hit | Note::Direct | Note::DirectMiss | Note::LinkGone | Note::Builtin
            | Note::Cached | Note::HashStale | Note::EmptyWord | Note::AssignOnly
            | Note::BadName | Note::Script | Note::CwdBlocked | Note::WalkCap | Note::NotFound
            | Note::HookOk | Note::HookFail => false,
        }
    }

    /// 是否只能由"PATH 真的走到尽头"给出（归因位必须为真的档）。
    pub fn path_end_only(&self) -> bool {
        match self {
            Note::NotFound | Note::HookOk | Note::HookFail => true,
            Note::Hit | Note::Direct | Note::DirectMiss | Note::LinkGone | Note::Builtin
            | Note::Cached | Note::HashStale | Note::EmptyWord | Note::AssignOnly
            | Note::BadName | Note::Denied | Note::Subdir | Note::Loop | Note::Script
            | Note::CwdBlocked | Note::WalkCap => false,
        }
    }

    /// 该档出现在哪一闸（判据要把 note 与 stage 成对核对）。
    pub fn stage(&self) -> Stage {
        match self {
            Note::EmptyWord | Note::AssignOnly | Note::BadName => Stage::Word,
            Note::Builtin | Note::Cached | Note::HashStale => Stage::Build,
            Note::Direct | Note::DirectMiss => Stage::Abs,
            Note::WalkCap => Stage::Scan,
            Note::CwdBlocked => Stage::Cwd,
            Note::Denied | Note::Subdir | Note::Loop | Note::LinkGone | Note::Script
            | Note::Hit => Stage::Exec,
            Note::NotFound | Note::HookOk | Note::HookFail => Stage::Tail,
        }
    }

    /// 该档对应的 errno（无errno 的档给 0）。
    pub fn errno(&self) -> i32 {
        match self {
            Note::DirectMiss | Note::HashStale => ERR_NOENT,
            Note::Denied => ERR_ACCES,
            Note::Subdir => ERR_ISDIR,
            Note::Loop => ERR_LOOP,
            Note::Script => ERR_NOEXEC,
            Note::LinkGone => ERR_NOENT,
            Note::NotFound | Note::HookOk | Note::HookFail => ERR_NOENT,
            Note::BadName => ERR_NAMETOOLONG,
            Note::CwdBlocked | Note::WalkCap => ERR_ACCES,
            Note::Hit | Note::Direct | Note::Builtin | Note::Cached | Note::EmptyWord
            | Note::AssignOnly => ERR_NONE_E,
        }
    }

    /// 该档的默认退出码（`Script`/`HookFail` 由形状给的实码覆盖）。
    pub fn code(&self) -> u32 {
        match self {
            Note::Hit | Note::Direct | Note::Builtin | Note::Cached | Note::EmptyWord
            | Note::AssignOnly | Note::HookOk => EXIT_OK,
            Note::NotFound | Note::DirectMiss | Note::HashStale | Note::CwdBlocked
            | Note::WalkCap | Note::LinkGone => EXIT_NOTFOUND,
            Note::Denied | Note::Subdir | Note::Loop | Note::BadName => EXIT_DENIED,
            Note::Script => EXIT_DENIED,
            Note::HookFail => EXIT_GENERAL,
        }
    }

    /// 是否"命令其实没被执行，却可能被读成成功"。
    pub fn silent_zero(&self) -> bool {
        match self {
            Note::HookOk => true,
            Note::Hit | Note::Direct | Note::Builtin | Note::Cached | Note::EmptyWord
            | Note::AssignOnly | Note::Script | Note::NotFound | Note::DirectMiss
            | Note::LinkGone | Note::HashStale | Note::Denied | Note::Subdir | Note::Loop
            | Note::BadName | Note::CwdBlocked | Note::WalkCap | Note::HookFail => false,
        }
    }
}

/// 结论落点的完整枚举。
pub const NOTE_SET: [Note; 19] = [
    Note::Hit,
    Note::Direct,
    Note::DirectMiss,
    Note::Builtin,
    Note::Cached,
    Note::HashStale,
    Note::EmptyWord,
    Note::AssignOnly,
    Note::BadName,
    Note::Denied,
    Note::Subdir,
    Note::Loop,
    Note::LinkGone,
    Note::Script,
    Note::CwdBlocked,
    Note::WalkCap,
    Note::NotFound,
    Note::HookOk,
    Note::HookFail,
];

// ---------------------------------------------------------------------------
// 枚举：档位
// ---------------------------------------------------------------------------

/// 四档统计（按实际结果投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 命令真的跑起来了（内建、哈希、逐目录命中、脚本重跑）。
    Hit,
    /// 带 `/` 的直路（POSIX 规定完全不查 PATH）。
    Direct,
    /// 找到了但没跑成（126 族与被拒执行）。
    Denied,
    /// PATH 尽头与各类"127 但不该记成尽头"。
    Fallback,
}

impl Tier {
    /// 由结果投影。
    ///
    /// 归因位先看：它是本卡最硬的一维，凡"确实把表走完了"的结论一律进 fallback，
    /// 哪怕钩子把它报成 0。其次看闸：停在 abs 段的进 direct。剩下的按成败分 hit/denied。
    pub fn of(o: &Outcome) -> Tier {
        if o.default {
            Tier::Fallback
        } else if o.stage == Stage::Abs {
            Tier::Direct
        } else if o.ok() {
            Tier::Hit
        } else {
            Tier::Denied
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Hit => 0,
            Tier::Direct => 1,
            Tier::Denied => 2,
            Tier::Fallback => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Hit => "hit",
            Tier::Direct => "direct",
            Tier::Denied => "denied",
            Tier::Fallback => "fallback",
        }
    }

    /// 该档是否允许出现码 0 的槽。
    ///
    /// 注意 fallback 档**也**允许——钩子退 0 就是"命令没跑成但 `$?` 是 0"，
    /// 这一档的存在正是本卡不许把"码 0"当成"跑成了"的理由。
    pub fn may_zero(&self) -> bool {
        match self {
            Tier::Hit | Tier::Direct | Tier::Fallback => true,
            Tier::Denied => false,
        }
    }

    /// 该档是否"PATH 被真正扫过"（direct 档按 POSIX 完全不扫）。
    pub fn scanned(&self) -> bool {
        match self {
            Tier::Hit | Tier::Denied | Tier::Fallback => true,
            Tier::Direct => false,
        }
    }

    /// 该档允许出现的落点（判据要检查档位与落点的相容性）。
    ///
    /// 这四组集合不是随手列的，它们必须与 `Tier::of` 的三级投影**互为逆命题**：
    /// - fallback 档 ⇔ `path_end_only()` 的那三个落点；
    /// - direct 档 ⇔ `exec_outcome(Stage::Abs, ..)` 能产出的落点；
    /// - denied 档 ⇔ 所有"没跑成、又没走到尽头"的落点（本卡最吃这一档，
    ///   127 借来的那几条路全在这里）；
    /// - hit 档 ⇔ 跑成了且不归因尽头的落点。
    /// 任何一处投影与集合不一致，`judge_ok` 就会红。
    pub fn admits(&self, n: Note) -> bool {
        match self {
            Tier::Hit => {
                matches!(
                    n,
                    Note::Hit | Note::Builtin | Note::Cached | Note::EmptyWord
                        | Note::AssignOnly | Note::Script
                )
            }
            Tier::Direct => {
                matches!(
                    n,
                    Note::Direct | Note::DirectMiss | Note::Denied | Note::Subdir | Note::Loop
                        | Note::Script
                )
            }
            Tier::Denied => {
                matches!(
                    n,
                    Note::Denied | Note::Subdir | Note::Loop | Note::LinkGone | Note::BadName
                        | Note::Script | Note::HashStale | Note::CwdBlocked | Note::WalkCap
                )
            }
            Tier::Fallback => matches!(n, Note::NotFound | Note::HookOk | Note::HookFail),
        }
    }
}

/// 档位表。
pub const TIER_SET: [Tier; TIER_COUNT] =
    [Tier::Hit, Tier::Direct, Tier::Denied, Tier::Fallback];

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
    /// 尝试目录数错。
    Value,
    /// 结论落点错。
    Note,
    /// 归因位错。
    Default,
    /// 归因破断（结构维）。
    Tail,
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
            Fault::Stage => PN_ESTAGE,
            Fault::Code => PN_ECODE,
            Fault::Value => PN_EVALUE,
            Fault::Note => PN_ENOTE,
            Fault::Default => PN_EDEFAULT,
            Fault::Tail => PN_ETAIL,
            Fault::Estate => PN_ESTATE,
            Fault::Count => PN_ECOUNT,
            Fault::Slot => PN_ESLOT,
            Fault::Seal => PN_ESEAL,
            Fault::Nobs => PN_ENOBS,
            Fault::Matrix => PN_EMATRIX,
            Fault::Bal => PN_EBAL,
        }
    }

    /// 是否非零。
    pub fn alive(&self) -> bool {
        match self {
            Fault::Ok => false,
            Fault::Stage | Fault::Code | Fault::Value | Fault::Note | Fault::Default
            | Fault::Tail | Fault::Estate | Fault::Count | Fault::Slot | Fault::Seal
            | Fault::Nobs | Fault::Matrix | Fault::Bal => true,
        }
    }

    /// 短名（渲染与释义用）。
    pub fn short(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Stage => "stage",
            Fault::Code => "code",
            Fault::Value => "dirs",
            Fault::Note => "note",
            Fault::Default => "attrib",
            Fault::Tail => "tail-broken",
            Fault::Estate => "estate",
            Fault::Count => "count",
            Fault::Slot => "slot",
            Fault::Seal => "seal",
            Fault::Nobs => "no-obs",
            Fault::Matrix => "matrix",
            Fault::Bal => "unbalanced",
        }
    }

    /// 是否六维之一的申报错（账本败格只收这六档）。
    pub fn is_claim_dim(&self) -> bool {
        match self {
            Fault::Estate | Fault::Stage | Fault::Code | Fault::Value | Fault::Note
            | Fault::Default => true,
            Fault::Ok | Fault::Tail | Fault::Count | Fault::Slot | Fault::Seal | Fault::Nobs
            | Fault::Matrix | Fault::Bal => false,
        }
    }
}

/// 缺陷短名表（与 `Fault::short` 逐字一致，判据要交叉核对）。
pub const FAULT_SHORT: [&'static str; 14] = [
    "ok",
    "stage",
    "code",
    "dirs",
    "note",
    "attrib",
    "tail-broken",
    "estate",
    "count",
    "slot",
    "seal",
    "no-obs",
    "matrix",
    "unbalanced",
];

// ---------------------------------------------------------------------------
// PATH 目录事实
// ---------------------------------------------------------------------------

/// 一个 PATH 元素：目录名 + 该命令名在这个目录里的事实。
///
/// 空元素（`""`）按 POSIX 表示当前目录，本卡把它单独标出来，因为策略拒绝 cwd
/// 的那一类构建**只**在这一格上不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirFact {
    /// 目录名（空串表示 PATH 里的空元素，即当前目录）。
    pub path: &'static str,
    /// 该命令名在这个目录里的事实。
    pub cand: Cand,
}

impl DirFact {
    /// 空白格（超出 `used` 的槽位不参与判定）。
    pub const fn blank() -> DirFact {
        DirFact { path: "unused", cand: Cand::Absent }
    }

    /// 造一格。
    pub const fn new(path: &'static str, cand: Cand) -> DirFact {
        DirFact { path, cand }
    }

    /// 是否当前目录元素。
    pub fn is_cwd(&self) -> bool {
        self.path.is_empty() || self.path == "."
    }

    /// 是否"这格有可能给出答复"（策略拒绝 cwd 时用得上）。
    pub fn answers(&self) -> bool {
        self.cand.exists()
    }

    /// 文本（释义与交叉核对用）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.is_cwd() {
            s.push_str("(cwd)");
        } else {
            s.push_str(self.path);
        }
        s.push('#');
        s.push_str(self.cand.name());
        s
    }

    /// 该格在给定 shell 下是否成为候选。
    pub fn chosen_by(&self, sh: Shell) -> bool {
        sh.accepts(self.cand)
    }
}

/// 十格空白表（形状里 `used` 之外的部分一律取它）。
pub const fn blank_facts() -> [DirFact; DIR_SLOTS] {
    [
        DirFact::blank(),
        DirFact::blank(),
        DirFact::blank(),
        DirFact::blank(),
        DirFact::blank(),
        DirFact::blank(),
        DirFact::blank(),
        DirFact::blank(),
        DirFact::blank(),
        DirFact::blank(),
    ]
}

/// 造一张十格事实表：只填前 `n` 格，其余留白（谓词与测试的造形助手）。
///
/// 语料表本身用的是逐格写明的 `const` 数组（笔误必须能在表里被看见），本函数
/// 只给复算场景用；调用方必须自己保证 `items` 长度不超过 `DIR_SLOTS`，
/// 超出部分被丢弃（由 `shape_ok` 负责把畸形形状判死，不靠本函数报错）。
pub fn facts(items: &[DirFact]) -> [DirFact; DIR_SLOTS] {
    let mut out = [DirFact::blank(); DIR_SLOTS];
    let mut i = 0usize;
    while i < items.len() && i < DIR_SLOTS {
        if let (Some(src), Some(dst)) = (items.get(i), out.get_mut(i)) {
            *dst = *src;
        }
        i += 1;
    }
    out
}

/// 本卡语料允许出现的目录名全集（形状里出现表外的名字即判死）。
pub const DIR_UNIVERSE: [&'static str; 12] = [
    "/bin",
    "/usr/bin",
    "/sbin",
    "/usr/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/opt/bin",
    "/mnt/nofs",
    "/tmp/ok-exec",
    "/tmp/nope",
    "",
    ".",
];

/// 该目录名是否在全集里。
pub fn dir_known(path: &'static str) -> bool {
    let mut i = 0usize;
    while i < DIR_UNIVERSE.len() {
        if let Some(p) = DIR_UNIVERSE.get(i) {
            if *p == path {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 把形状声明的 PATH 元素拼成 `PATH=` 那一句的右值（逐字，含空元素）。
pub fn spec_text(s: &Shape) -> String {
    match s.path_state {
        PathState::Unset => String::from(s.shell.default_path()),
        PathState::Empty => String::new(),
        PathState::Set => {
            let mut out = String::new();
            let mut i = 0usize;
            let n = s.used;
            while i < n {
                if i > 0 {
                    out.push(':');
                }
                if let Some(f) = s.dirs.get(i) {
                    out.push_str(f.path);
                }
                i += 1;
            }
            out
        }
    }
}

/// 词里是否含 `/`（POSIX：含 `/` 就完全不走 PATH）。
pub fn has_slash(word: &str) -> bool {
    let bytes = word.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == b'/' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 落点与闸是否相容（`Denied/Subdir/Loop/Script` 四个档在 abs 与 exec 两处都能出）。
pub fn note_stage_ok(n: Note, st: Stage) -> bool {
    match n {
        Note::EmptyWord | Note::AssignOnly | Note::BadName => st == Stage::Word,
        Note::Builtin | Note::Cached | Note::HashStale => st == Stage::Build,
        Note::Direct | Note::DirectMiss => st == Stage::Abs,
        Note::CwdBlocked => st == Stage::Cwd,
        Note::WalkCap => st == Stage::Scan,
        Note::NotFound | Note::HookOk | Note::HookFail => st == Stage::Tail,
        Note::Hit | Note::LinkGone => st == Stage::Exec,
        Note::Denied | Note::Subdir | Note::Loop | Note::Script => {
            st == Stage::Exec || st == Stage::Abs
        }
    }
}

// ---------------------------------------------------------------------------
// 形状：一条命令词在某 shell 里的全部现场维度
// ---------------------------------------------------------------------------

/// 一次查找输入的全部维度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 命令词。
    pub word: &'static str,
    /// 声明词长（越界词的实测字节数，语料不放 300 字节的字面量）。
    pub word_len: u32,
    /// 词形态。
    pub word_kind: WordKind,
    /// 哪个 shell。
    pub shell: Shell,
    /// dash 文案里的行号。
    pub line: u32,
    /// 词是否命中内建表。
    pub builtin: bool,
    /// 命令哈希表状态。
    pub hash_state: HashState,
    /// 哈希表里记住的路径。
    pub hash_path: &'static str,
    /// 陈旧项是否被直接复用（合法做法只有回扫）。
    pub hash_reuse: bool,
    /// PATH 取值形态。
    pub path_state: PathState,
    /// 形状声明的元素数。
    pub used: usize,
    /// 逐目录事实表（顺序即扫描顺序）。
    pub dirs: [DirFact; DIR_SLOTS],
    /// 当前目录元素是否被允许当候选。
    pub cwd_allowed: bool,
    /// 带 `/` 直路时该路径的事实。
    pub abs_cand: Cand,
    /// 尽头钩子状态。
    pub hook: HookState,
    /// 钩子返回码（`HookState::NonZero` 时有效）。
    pub hook_rc: u32,
    /// `ENOEXEC` 后脚本重跑的返回码。
    pub script_rc: u32,
}

impl Shape {
    /// 默认形状：bash、正常词、PATH 已设、无哈希、无钩子。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            word: "zzz",
            word_len: 3,
            word_kind: WordKind::Normal,
            shell: Shell::Bash,
            line: 1,
            builtin: false,
            hash_state: HashState::Miss,
            hash_path: "",
            hash_reuse: false,
            path_state: PathState::Set,
            used: 0,
            dirs: blank_facts(),
            cwd_allowed: true,
            abs_cand: Cand::Absent,
            hook: HookState::Absent,
            hook_rc: 0,
            script_rc: 0,
        }
    }

    /// 换"逐目录事实表"的副本（把同一形状推到不同目录序复算）。
    pub fn with_facts(&self, n: usize, items: &[DirFact]) -> Shape {
        let mut out = Shape {
            tag: self.tag,
            word: self.word,
            word_len: self.word_len,
            word_kind: self.word_kind,
            shell: self.shell,
            line: self.line,
            builtin: self.builtin,
            hash_state: self.hash_state,
            hash_path: self.hash_path,
            hash_reuse: self.hash_reuse,
            path_state: self.path_state,
            used: n,
            dirs: blank_facts(),
            cwd_allowed: self.cwd_allowed,
            abs_cand: self.abs_cand,
            hook: self.hook,
            hook_rc: self.hook_rc,
            script_rc: self.script_rc,
        };
        let mut i = 0usize;
        while i < items.len() && i < DIR_SLOTS {
            if let (Some(src), Some(dst)) = (items.get(i), out.dirs.get_mut(i)) {
                *dst = *src;
            }
            i += 1;
        }
        out
    }

    /// 换 shell 的副本（跨 shell 对照复算：同一张表两个合法数字）。
    pub const fn with_shell(&self, sh: Shell) -> Shape {
        Shape {
            tag: self.tag,
            word: self.word,
            word_len: self.word_len,
            word_kind: self.word_kind,
            shell: sh,
            line: self.line,
            builtin: self.builtin,
            hash_state: self.hash_state,
            hash_path: self.hash_path,
            hash_reuse: self.hash_reuse,
            path_state: self.path_state,
            used: self.used,
            dirs: self.dirs,
            cwd_allowed: self.cwd_allowed,
            abs_cand: self.abs_cand,
            hook: self.hook,
            hook_rc: self.hook_rc,
            script_rc: self.script_rc,
        }
    }

    /// 换单一维度的副本（谓词做单维注入用）。
    pub const fn with_cwd(&self, allowed: bool) -> Shape {
        Shape {
            tag: self.tag,
            word: self.word,
            word_len: self.word_len,
            word_kind: self.word_kind,
            shell: self.shell,
            line: self.line,
            builtin: self.builtin,
            hash_state: self.hash_state,
            hash_path: self.hash_path,
            hash_reuse: self.hash_reuse,
            path_state: self.path_state,
            used: self.used,
            dirs: self.dirs,
            cwd_allowed: allowed,
            abs_cand: self.abs_cand,
            hook: self.hook,
            hook_rc: self.hook_rc,
            script_rc: self.script_rc,
        }
    }

    /// 换钩子状态的副本。
    pub const fn with_hook(&self, h: HookState, rc: u32) -> Shape {
        Shape {
            tag: self.tag,
            word: self.word,
            word_len: self.word_len,
            word_kind: self.word_kind,
            shell: self.shell,
            line: self.line,
            builtin: self.builtin,
            hash_state: self.hash_state,
            hash_path: self.hash_path,
            hash_reuse: self.hash_reuse,
            path_state: self.path_state,
            used: self.used,
            dirs: self.dirs,
            cwd_allowed: self.cwd_allowed,
            abs_cand: self.abs_cand,
            hook: h,
            hook_rc: rc,
            script_rc: self.script_rc,
        }
    }

    /// 换成带 `/` 的直路词（同一候选事实推到 abs 闸复算）。
    pub const fn with_abs(&self, c: Cand) -> Shape {
        Shape {
            tag: self.tag,
            word: self.word,
            word_len: self.word_len,
            word_kind: self.word_kind,
            shell: self.shell,
            line: self.line,
            builtin: self.builtin,
            hash_state: self.hash_state,
            hash_path: self.hash_path,
            hash_reuse: self.hash_reuse,
            path_state: self.path_state,
            used: self.used,
            dirs: self.dirs,
            cwd_allowed: self.cwd_allowed,
            abs_cand: c,
            hook: self.hook,
            hook_rc: self.hook_rc,
            script_rc: self.script_rc,
        }
    }

    /// 本次实际要扫的元素数（含上界守卫的截断）。
    pub fn walk_len(&self) -> usize {
        let n = self.path_state.elements(self.used);
        let bound = self.shell.walk_bound();
        if n > bound {
            bound
        } else {
            n
        }
    }

    /// 声明元素数是否越过上界守卫。
    pub fn capped(&self) -> bool {
        self.path_state.elements(self.used) > self.shell.walk_bound()
    }

    /// 词是否走带斜杠直路。
    pub fn direct(&self) -> bool {
        self.word_kind == WordKind::Normal && has_slash(self.word)
    }

    /// 取第 `i` 格事实。
    pub fn fact(&self, i: usize) -> Option<DirFact> {
        if i >= self.walk_len() {
            return None;
        }
        self.dirs.get(i).copied()
    }

    /// 第一个可执行命中的下标（没有则 `None`）。
    pub fn first_exec(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.walk_len() {
            if let Some(f) = self.dirs.get(i) {
                if f.cand.executable() && !(f.is_cwd() && !self.cwd_allowed) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 是否在内建/哈希段就有答复（此时 PATH 不许被扫）。
    pub fn answered_before_path(&self) -> bool {
        self.builtin || self.hash_state.answers_first() || (self.hash_state == HashState::Stale && self.hash_reuse)
    }

    /// 钩子在本 shell 上是否真的会被调用。
    pub fn hook_active(&self) -> bool {
        self.shell.has_hook() && self.hook.overrides_rc()
    }
}

/// 带斜杠直路命中（PATH 被 POSIX 规定完全跳过）。
pub fn msg_direct_exec(word: &str) -> String {
    let mut s = String::from("exec ");
    s.push_str(word);
    s.push_str(" directly (PATH skipped by POSIX: the word contains '/')");
    s
}

/// 命中目录里那个真件的路径（`<dir>/<word>`）。
///
/// `tried` 是**实际尝试过的目录数**，而"尝试过"与"表上的下标"在两处会脱开：
/// 策略拒绝当前目录的那一格根本不被 stat，也就不计数。所以这里不许用
/// `tried - 1` 直接当下标，必须按与 `define` 相同的推进规则重走一遍表，取第
/// `tried` 次尝试所在的那一格——否则被拒的 cwd 会让整个释义行指错目录。
pub fn resolved_path(s: &Shape, tried: u32) -> String {
    match attempted_dir(s, tried) {
        Some(dir) => {
            if dir.is_empty() {
                let mut out = String::from("./");
                out.push_str(s.word);
                out
            } else {
                let mut out = String::from(dir);
                out.push('/');
                out.push_str(s.word);
                out
            }
        }
        None => String::from(s.word),
    }
}

/// 第 `n` 次尝试落在哪一格的路径（`n` 从 1 起；越界或 `n` 为 0 给 `None`）。
pub fn attempted_dir(s: &Shape, n: u32) -> Option<&'static str> {
    if n == 0 {
        return None;
    }
    let mut tried = 0u32;
    let mut i = 0usize;
    while i < s.walk_len() {
        let f = match s.dirs.get(i) {
            Some(v) => *v,
            None => break,
        };
        if f.is_cwd() && !s.cwd_allowed {
            if f.cand.executable() {
                // 扫描就断在这里：这一格没被算进尝试数，其后也不存在第 n 次尝试
                return None;
            }
            i += 1;
            continue;
        }
        tried = tried.saturating_add(1);
        if tried == n {
            return Some(f.path);
        }
        if s.shell.accepts(f.cand) {
            return None;
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次查找的现场结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段（最后拦下这次运行的闸）。
    pub stage: Stage,
    /// 退出码（0/126/127/钩子码/脚本码）。
    pub code: u32,
    /// 实际尝试过的目录数（逐目录序的账）。
    pub value: u32,
    /// 结论落点。
    pub note: Note,
    /// 归因位：结论是否来自"PATH 走到尽头"。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome { stage: Stage::Tail, code: EXIT_OK, value: 0, note: Note::Hit, default: false }
    }

    /// 是否成功（码 0 且落点属成功族）。
    pub fn ok(&self) -> bool {
        self.code == EXIT_OK && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否"钩子顶掉了 127 且真的什么都没执行"（静默零现场）。
    pub fn silent_zero_check(&self) -> bool {
        self.note.silent_zero() && self.code == EXIT_OK
    }

    /// 是否归因命中：确实走到尽头、落点属尽头族、闸停在 Tail。
    pub fn attributed(&self) -> bool {
        self.default && self.note.path_end_only() && self.stage == Stage::Tail
    }

    /// 是否"报了 127 却不是走到尽头"——本卡最阴的一类现场。
    pub fn borrowed_127(&self) -> bool {
        self.code == EXIT_NOTFOUND && !self.default
    }

    /// 命中下标（1 基；0 表示 PATH 未参与或未命中）。
    pub fn hit_index(&self) -> u32 {
        if self.note == Note::Hit {
            self.value
        } else {
            0
        }
    }

    /// 三元组自洽：归因位、落点、闸、码、目录数五者必须互相指认。
    ///
    /// 这套关系是本卡判据的骨头：`127` 可以由四条不同的路给出（尽头、直路落空、
    /// 陈旧哈希、cwd 被拒、扫到界），只有把"归因位 ⇔ 落点属尽头族 ⇔ 闸停在 Tail"
    /// 三条绑在一起，才能把"其实提前停了却谎称扫完"的那类实现抓出来。
    pub fn consistent(&self, s: &Shape) -> bool {
        if self.default != self.note.path_end_only() {
            return false;
        }
        if !note_stage_ok(self.note, self.stage) {
            return false;
        }
        if self.stage.zero_dirs() && self.value != 0 {
            return false;
        }
        if self.value > s.walk_len() as u32 {
            return false;
        }
        if self.default && self.stage != Stage::Tail {
            return false;
        }
        if self.note.notfound_family() && !self.note.path_end_only() && self.code != EXIT_NOTFOUND
        {
            return false;
        }
        if self.note.denied_family() && self.code != EXIT_DENIED {
            return false;
        }
        if self.note == Note::NotFound && self.code != EXIT_NOTFOUND {
            return false;
        }
        if self.note == Note::HookOk && self.code != EXIT_OK {
            return false;
        }
        if self.note == Note::HookOk && !s.hook_active() {
            return false;
        }
        if self.note == Note::HookFail && self.code != s.hook_rc {
            return false;
        }
        if self.note == Note::HookFail && !s.hook_active() {
            return false;
        }
        if self.note.success() && self.code != EXIT_OK && self.note != Note::Script {
            return false;
        }
        if self.note == Note::Script && self.code != s.script_rc {
            return false;
        }
        if s.path_state.no_search() && self.value != 0 {
            return false;
        }
        if s.answered_before_path() && self.stage != Stage::Build && self.stage != Stage::Word {
            if self.note != Note::HashStale && self.note != Note::Cached && self.note != Note::Builtin
            {
                return false;
            }
        }
        true
    }

    /// 相对声明元素数的亏空（审计用，饱和减）：还差几个目录没看。
    pub fn unscanned(&self, s: &Shape) -> u32 {
        let total = s.path_state.elements(s.used) as u32;
        total.saturating_sub(self.value)
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
// 定义纵线（闸门次序：word → build → abs → path → cwd → scan → hit → exec → tail）
// ---------------------------------------------------------------------------

/// 候选事实 → 执行结论（abs 与 exec 两处共用一张映射表）。
///
/// 两处的**唯一**区别是闸位与"命中族"的落点名：带斜杠的路径成功时记 `Direct`、
/// 落空时记 `DirectMiss`；走 PATH 命中时记 `Hit`、悬空链接记 `LinkGone`。
/// 码与 errno 在两处必须一致，否则同一现场会因为查询入口不同而给出不同数字。
fn exec_outcome(st: Stage, c: Cand, tried: u32, script_rc: u32) -> Outcome {
    match c {
        Cand::RegularExec => Outcome {
            stage: st,
            code: EXIT_OK,
            value: tried,
            note: if st == Stage::Abs { Note::Direct } else { Note::Hit },
            default: false,
        },
        Cand::Absent | Cand::Dangling => Outcome {
            stage: st,
            code: EXIT_NOTFOUND,
            value: tried,
            note: if st == Stage::Abs { Note::DirectMiss } else { Note::LinkGone },
            default: false,
        },
        Cand::Subdir => Outcome {
            stage: st,
            code: EXIT_DENIED,
            value: tried,
            note: Note::Subdir,
            default: false,
        },
        Cand::RegularNoExec | Cand::NonRegular => Outcome {
            stage: st,
            code: EXIT_DENIED,
            value: tried,
            note: Note::Denied,
            default: false,
        },
        Cand::Loop => Outcome {
            stage: st,
            code: EXIT_DENIED,
            value: tried,
            note: Note::Loop,
            default: false,
        },
        Cand::ZeroLen => Outcome {
            stage: st,
            code: script_rc,
            value: tried,
            note: Note::Script,
            default: false,
        },
    }
}

/// 判定一次命令词查找的结论。
///
/// 每一闸只在上一闸放行后才执行；任何一段拦下就立刻定案，后面的段不再有权改写
/// 它。这是"逐目录序"能被反查的唯一前提：一旦允许"先报 127、事后再补一句其实
/// 没扫完"，`value` 这个数就彻底失去意义。
pub fn define(s: &Shape) -> Outcome {
    // ---- word：命令词体检 ----
    if !s.word_kind.searchable() {
        return match s.word_kind {
            WordKind::Empty => Outcome {
                stage: Stage::Word,
                code: EXIT_OK,
                value: 0,
                note: Note::EmptyWord,
                default: false,
            },
            WordKind::AssignOnly => Outcome {
                stage: Stage::Word,
                code: EXIT_OK,
                value: 0,
                note: Note::AssignOnly,
                default: false,
            },
            WordKind::Overlong => Outcome {
                stage: Stage::Word,
                code: EXIT_DENIED,
                value: 0,
                note: Note::BadName,
                default: false,
            },
            WordKind::Normal => Outcome::blank(),
        };
    }

    // ---- build：内建表与命令哈希表先于 PATH ----
    if s.builtin {
        return Outcome {
            stage: Stage::Build,
            code: EXIT_OK,
            value: 0,
            note: Note::Builtin,
            default: false,
        };
    }
    if s.hash_state == HashState::Fresh {
        return Outcome {
            stage: Stage::Build,
            code: EXIT_OK,
            value: 0,
            note: Note::Cached,
            default: false,
        };
    }
    if s.hash_state == HashState::Stale && s.hash_reuse {
        // 陈旧项被直接复用：一个目录都没扫就报 127，归因位必须为假。
        return Outcome {
            stage: Stage::Build,
            code: EXIT_NOTFOUND,
            value: 0,
            note: Note::HashStale,
            default: false,
        };
    }

    // ---- abs：词含 '/'，POSIX 规定完全不查 PATH ----
    if s.direct() {
        return exec_outcome(Stage::Abs, s.abs_cand, 0, s.script_rc);
    }

    // ---- path：PATH 取值定案（未设走默认表、空串一格不扫） ----
    let n = s.walk_len();
    let capped = s.capped();

    // ---- scan + hit + exec：逐目录推进 ----
    let mut tried = 0u32;
    let mut i = 0usize;
    while i < n {
        let f = match s.dirs.get(i) {
            Some(v) => *v,
            None => break,
        };
        // ---- cwd：当前目录元素的策略 ----
        if f.is_cwd() && !s.cwd_allowed {
            if f.cand.executable() {
                // 本来能命中的那一格被策略拒了：扫描在尽头之前停止，归因位为假。
                return Outcome {
                    stage: Stage::Cwd,
                    code: EXIT_NOTFOUND,
                    value: tried,
                    note: Note::CwdBlocked,
                    default: false,
                };
            }
            // 这一格本来也给不出答复：整格跳过，不计入尝试数。
            i += 1;
            continue;
        }
        tried = tried.saturating_add(1);
        if s.shell.accepts(f.cand) {
            return exec_outcome(Stage::Exec, f.cand, tried, s.script_rc);
        }
        i += 1;
    }

    // ---- 上界守卫：声明的元素数越过守卫，扫到界即停 ----
    if capped {
        return Outcome {
            stage: Stage::Scan,
            code: EXIT_NOTFOUND,
            value: tried,
            note: Note::WalkCap,
            default: false,
        };
    }

    // ---- tail：PATH 尽头，四条回退去路 ----
    tail_outcome(s, tried)
}

/// PATH 尽头的回退结论（钩子只属于 bash）。
fn tail_outcome(s: &Shape, tried: u32) -> Outcome {
    if s.hook_active() {
        return match s.hook {
            HookState::Zero => Outcome {
                stage: Stage::Tail,
                code: EXIT_OK,
                value: tried,
                note: Note::HookOk,
                default: true,
            },
            HookState::NonZero => Outcome {
                stage: Stage::Tail,
                code: s.hook_rc,
                value: tried,
                note: Note::HookFail,
                default: true,
            },
            HookState::Absent => Outcome {
                stage: Stage::Tail,
                code: EXIT_NOTFOUND,
                value: tried,
                note: Note::NotFound,
                default: true,
            },
        };
    }
    Outcome {
        stage: Stage::Tail,
        code: EXIT_NOTFOUND,
        value: tried,
        note: Note::NotFound,
        default: true,
    }
}

/// 现场文案（note → 真实文本；成功且无声的落点返回空串）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::Hit => msg_hit(s.word, resolved_path(s, o.value).as_str(), o.value as usize),
        Note::Direct => msg_direct_exec(s.word),
        Note::DirectMiss => msg_no_such(s.shell, s.word, s.line),
        Note::LinkGone => msg_no_such(s.shell, resolved_path(s, o.value).as_str(), s.line),
        Note::Builtin => msg_builtin(s.word),
        Note::Cached => msg_cached(s.word, s.hash_path),
        Note::HashStale => msg_hash_stale(s.hash_path),
        Note::EmptyWord => msg_bad_word(s.word_len, WordKind::Empty),
        Note::AssignOnly => msg_bad_word(s.word_len, WordKind::AssignOnly),
        Note::BadName => msg_bad_word(s.word_len, WordKind::Overlong),
        Note::Denied => {
            let st = if o.stage == Stage::Abs { s.word } else { "" };
            if st.is_empty() {
                msg_path_error(s.shell, resolved_path(s, o.value).as_str(), ERR_ACCES, s.line)
            } else {
                msg_path_error(s.shell, st, ERR_ACCES, s.line)
            }
        }
        Note::Subdir => {
            if o.stage == Stage::Abs {
                msg_is_a_directory(s.shell, s.word, s.line)
            } else {
                msg_is_a_directory(s.shell, resolved_path(s, o.value).as_str(), s.line)
            }
        }
        Note::Loop => {
            if o.stage == Stage::Abs {
                msg_too_many_links(s.shell, s.word, s.line)
            } else {
                msg_too_many_links(s.shell, resolved_path(s, o.value).as_str(), s.line)
            }
        }
        Note::Script => msg_script_ran(o.code),
        Note::CwdBlocked => msg_cwd_refused(s.word, o.value as usize),
        Note::WalkCap => msg_walk_cap(s.used, s.shell.walk_bound()),
        Note::NotFound => msg_notfound(s.shell, s.word, s.line),
        Note::HookOk => msg_hook_zero(s.word),
        Note::HookFail => msg_hook_fail(s.word, o.code),
    }
}

/// 告警/说明文案（与错误文案分开：`message()` 只给那句诊断）。
///
/// 唯一需要额外说明的尽头现场是"配了钩子但这 shell 根本没有钩子"：诊断行仍然
/// 只说 `not found`，本卡把"钩子被忽略"这件事单独记在告警行里，免得读者把
/// "没调用钩子"读成"钩子退出了 127"。
pub fn warning(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::HookOk => msg_hook_zero(s.word),
        Note::HookFail => msg_hook_fail(s.word, o.code),
        Note::HashStale => msg_hash_stale(s.hash_path),
        Note::CwdBlocked => msg_cwd_refused(s.word, o.value as usize),
        Note::WalkCap => msg_walk_cap(s.used, s.shell.walk_bound()),
        Note::NotFound => {
            if s.hook.overrides_rc() && !s.shell.has_hook() {
                msg_hook_ignored(s.word, s.shell)
            } else {
                String::new()
            }
        }
        Note::Hit | Note::Direct | Note::DirectMiss | Note::LinkGone | Note::Builtin
        | Note::Cached | Note::EmptyWord | Note::AssignOnly | Note::BadName | Note::Denied
        | Note::Subdir | Note::Loop | Note::Script => String::new(),
    }
}

/// 结果 → 申报（完美申报的构造器：判据与测试都靠它把"应然"写成表）。
pub fn claim_of(o: &Outcome) -> Claim {
    Claim { stage: o.stage, code: o.code, value: o.value, note: o.note, default: o.default }
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
    /// 尝试目录数。
    pub value: u32,
    /// 结论落点。
    pub note: Note,
    /// 归因位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Tail, code: EXIT_OK, value: 0, note: Note::Hit, default: false }
    }

    /// 是否申报成功（码 0 且落点属成功族）。
    pub fn success(&self) -> bool {
        self.code == EXIT_OK && self.note.success()
    }

    /// 申报的落点是否自称"走到了 PATH 尽头"。
    pub fn claims_path_end(&self) -> bool {
        self.default
    }

    /// 申报档位的投影（与 `Tier::of` 同一条投影规则，供交叉核对）。
    pub fn tier(&self) -> Tier {
        if self.default {
            Tier::Fallback
        } else if self.stage == Stage::Abs {
            Tier::Direct
        } else if self.success() {
            Tier::Hit
        } else {
            Tier::Denied
        }
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

/// 8 个可正确申报的形状 + 6 个复刻形状（复刻自 `ash-tail-notfound`）。
pub const SHAPES: [Shape; CORPUS] = [
    // 0：ash 第一个目录就命中——逐目录序最短的一条路。
    Shape {
        tag: "ash-hit-first-dir",
        word: "ls",
        word_len: 2,
        word_kind: WordKind::Normal,
        shell: Shell::Ash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::RegularExec),
            DirFact::new("/usr/bin", Cand::RegularExec),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 1：bash 扫到第三个目录才命中——前两格连名字都没有（Absent），
    //      这一条把"扫了两个才中"和"只看了一个"分开。
    Shape {
        tag: "bash-hit-third-dir",
        word: "grep",
        word_len: 4,
        word_kind: WordKind::Normal,
        shell: Shell::Bash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 3,
        dirs: [
            DirFact::new("/usr/local/bin", Cand::Absent),
            DirFact::new("/sbin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::RegularExec),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 2：词含 '/'，POSIX 规定完全不查 PATH。
    Shape {
        tag: "bash-abs-direct-hit",
        word: "/tmp/ok-exec",
        word_len: 12,
        word_kind: WordKind::Normal,
        shell: Shell::Bash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::RegularExec,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 3：带斜杠的落空——127 但不是 PATH 尽头（dash 文案族）。
    Shape {
        tag: "dash-abs-direct-miss",
        word: "/tmp/nope",
        word_len: 9,
        word_kind: WordKind::Normal,
        shell: Shell::Dash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::RegularExec),
            DirFact::new("/usr/bin", Cand::RegularExec),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 4：主现场——ash 把两格表走完，尽头给 127（后六槽全部复刻这一格）。
    Shape {
        tag: "ash-tail-notfound",
        word: "zzz",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Ash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 5：ash 跳过无执行位那一格继续扫，最后仍然走到尽头——与槽 4 用户面不可分辨。
    Shape {
        tag: "ash-skip-nonexec-tail",
        word: "foo",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Ash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/usr/local/bin", Cand::RegularNoExec),
            DirFact::new("/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 6：同一张表交给 bash——停在首个实体，126 且不再往下扫。
    Shape {
        tag: "bash-nonexec-stop-126",
        word: "foo",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Bash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/usr/local/bin", Cand::RegularNoExec),
            DirFact::new("/bin", Cand::RegularExec),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 7：bash 尽头钩子退 0——命令没跑成，`$?` 却是 0，归因位仍为真。
    Shape {
        tag: "bash-hook-zero",
        word: "zzz",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Bash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 3,
        dirs: [
            DirFact::new("/bin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::Absent),
            DirFact::new("/opt/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Zero,
        hook_rc: 0,
        script_rc: 0,
    },
    // 8：成败互报——把"走到尽头 127"申报成"命中并跑通"。
    Shape {
        tag: "estate-swap",
        word: "zzz",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Ash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 9：闸位错——落点说走到尽头，闸却申报停在逐目录推进段。
    Shape {
        tag: "stage-swap",
        word: "zzz",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Ash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 10：码错——把 127 报成一般错误 1。
    Shape {
        tag: "code-swap",
        word: "zzz",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Ash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 11：目录数错——只报扫了一格（提前停的实现最爱报这个数）。
    Shape {
        tag: "value-swap",
        word: "zzz",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Ash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 12：落点错——把"扫到上界守卫就停"那一档说成"走到尽头"。
    Shape {
        tag: "note-swap",
        word: "zzz",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Ash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
    // 13：归因位错——落点、码、目录数全对，却不肯承认这是 PATH 尽头给的。
    Shape {
        tag: "default-swap",
        word: "zzz",
        word_len: 3,
        word_kind: WordKind::Normal,
        shell: Shell::Ash,
        line: 1,
        builtin: false,
        hash_state: HashState::Miss,
        hash_path: "",
        hash_reuse: false,
        path_state: PathState::Set,
        used: 2,
        dirs: [
            DirFact::new("/bin", Cand::Absent),
            DirFact::new("/usr/bin", Cand::Absent),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
            DirFact::blank(),
        ],
        cwd_allowed: true,
        abs_cand: Cand::Absent,
        hook: HookState::Absent,
        hook_rc: 0,
        script_rc: 0,
    },
];

/// 前 8 槽逐维等于现场；后 6 槽各只错一维，错的那维依次是
/// estate/stage/code/value/note/default。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Exec, code: 0, value: 1, note: Note::Hit, default: false },
    Claim { stage: Stage::Exec, code: 0, value: 3, note: Note::Hit, default: false },
    Claim { stage: Stage::Abs, code: 0, value: 0, note: Note::Direct, default: false },
    Claim { stage: Stage::Abs, code: 127, value: 0, note: Note::DirectMiss, default: false },
    Claim { stage: Stage::Tail, code: 127, value: 2, note: Note::NotFound, default: true },
    Claim { stage: Stage::Tail, code: 127, value: 2, note: Note::NotFound, default: true },
    Claim { stage: Stage::Exec, code: 126, value: 1, note: Note::Denied, default: false },
    Claim { stage: Stage::Tail, code: 0, value: 3, note: Note::HookOk, default: true },
    Claim { stage: Stage::Exec, code: 0, value: 2, note: Note::Hit, default: false },
    Claim { stage: Stage::Scan, code: 127, value: 2, note: Note::NotFound, default: true },
    Claim { stage: Stage::Tail, code: 1, value: 2, note: Note::NotFound, default: true },
    Claim { stage: Stage::Tail, code: 127, value: 1, note: Note::NotFound, default: true },
    Claim { stage: Stage::Tail, code: 127, value: 2, note: Note::WalkCap, default: true },
    Claim { stage: Stage::Tail, code: 127, value: 2, note: Note::NotFound, default: false },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    PN_ESTATE,
    PN_ESTAGE,
    PN_ECODE,
    PN_EVALUE,
    PN_ENOTE,
    PN_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、命中档、直路档、拒绝档、回退档、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 2, 1, 3, 929];

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
        return Err(PN_ENOBS);
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

/// 对判：状态 → 阶段 → 退出码 → 目录数 → 落点 → 归因位。
///
/// 次序是钉死的：成败互报必须先看（它把"报 0"与"报 127"两类谎分开），然后按
/// 闸从前到后。反过来先比目录数，就会把"提前停但没扫完"的槽错判成 Value，
/// 而它的真正病灶在闸位上。
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
    let mut s = String::from(c.shape.shell.text());
    s.push_str(" word=");
    s.push_str(c.shape.word);
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" code=");
    s.push_str(dec_text(o.code).as_str());
    s.push_str(" dirs=");
    s.push_str(dec_text(o.value).as_str());
    s.push_str(" attrib=");
    s.push_str(bool_text(o.default));
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

/// 归因位文本（本卡把 `default` 这一维读成"是不是 PATH 尽头给的"）。
pub fn attrib_text(v: bool) -> &'static str {
    if v {
        "path-end"
    } else {
        "early-stop"
    }
}

// ---------------------------------------------------------------------------
// 期望表（语料的第二份陈述）
// ---------------------------------------------------------------------------

/// 14 槽的期望结束阶段。
///
/// 这份表是把 `SHAPES`/`CLAIMS` 用**另一种形式**再写一遍：申报表说的是"我声明
/// 这次查找结束在哪一闸"，期望表说的是"校验器按纵线实际会走到哪一闸"。两者一致
/// 才算数，任一处笔误都会在这里露出来。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exec,
    Stage::Exec,
    Stage::Abs,
    Stage::Abs,
    Stage::Tail,
    Stage::Tail,
    Stage::Exec,
    Stage::Tail,
    Stage::Tail,
    Stage::Tail,
    Stage::Tail,
    Stage::Tail,
    Stage::Tail,
    Stage::Tail,
];

/// 14 槽的期望退出码。
const WANT_CODE: [u32; CORPUS] =
    [0, 0, 0, 127, 127, 127, 126, 0, 127, 127, 127, 127, 127, 127];

/// 14 槽的期望尝试目录数。
const WANT_VALUE: [u32; CORPUS] =
    [1, 3, 0, 0, 2, 2, 1, 3, 2, 2, 2, 2, 2, 2];

/// 14 槽的期望结论落点。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Hit,
    Note::Hit,
    Note::Direct,
    Note::DirectMiss,
    Note::NotFound,
    Note::NotFound,
    Note::Denied,
    Note::HookOk,
    Note::NotFound,
    Note::NotFound,
    Note::NotFound,
    Note::NotFound,
    Note::NotFound,
    Note::NotFound,
];

/// 14 槽的期望归因位。
const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, true, true, false, true, true, true, true, true, true, true,
];

/// 14 槽的期望 shell。
const WANT_SHELL: [Shell; CORPUS] = [
    Shell::Ash,
    Shell::Bash,
    Shell::Bash,
    Shell::Dash,
    Shell::Ash,
    Shell::Ash,
    Shell::Bash,
    Shell::Bash,
    Shell::Ash,
    Shell::Ash,
    Shell::Ash,
    Shell::Ash,
    Shell::Ash,
    Shell::Ash,
];

/// 14 槽标签。
const WANT_TAGS: [&str; CORPUS] = [
    "ash-hit-first-dir",
    "bash-hit-third-dir",
    "bash-abs-direct-hit",
    "dash-abs-direct-miss",
    "ash-tail-notfound",
    "ash-skip-nonexec-tail",
    "bash-nonexec-stop-126",
    "bash-hook-zero",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 14 槽的期望档位（把 `Tier::of` 的投影也用另一种形式复写一遍）。
const WANT_TIER: [Tier; CORPUS] = [
    Tier::Hit,
    Tier::Hit,
    Tier::Direct,
    Tier::Direct,
    Tier::Fallback,
    Tier::Fallback,
    Tier::Denied,
    Tier::Fallback,
    Tier::Fallback,
    Tier::Fallback,
    Tier::Fallback,
    Tier::Fallback,
    Tier::Fallback,
    Tier::Fallback,
];

/// 八格申报正确的下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 六格申报错误的下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 复刻槽（后六槽）的标签后缀。
const SWAP_SUFFIX: &str = "-swap";

/// 主现场槽（后六槽全部复刻它）的下标。
const SCENE_SLOT: usize = 4;

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

    /// 该档红档是否全由复刻槽贡献（本卡的立卡理由）。
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
/// 分档只看**实际结果**落进哪一档，申报正确与否单独计。本卡的档是按
/// "归因位 → 闸位 → 成败"三级投影的，因此钩子退 0 那一格进 fallback 而不是
/// hit——工具的 `$?` 明明是 0，但它的现场是"PATH 走完了"，把它塞进 hit 档
/// 就等于替 shell 补了一条它没执行过的命令。红档槽仍留在自己那一档里，
/// 覆盖面才看得见。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（hit/direct/denied/fallback）。
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

    /// 覆盖是否平（每档都有槽）。
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

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < TIER_COUNT {
            let t = match TIER_SET.get(i) {
                Some(v) => *v,
                None => return s,
            };
            if let Some(r) = self.row(t) {
                s.push_str(r.render(t).as_str());
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
    /// 命中档正确数。
    pub hit: u32,
    /// 直路档正确数。
    pub direct: u32,
    /// 拒绝档正确数。
    pub denied: u32,
    /// 回退档正确数。
    pub fallback: u32,
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
            Err(PN_ECOUNT)
        }
    }

    /// 跑成了的槽数：命中、显式路径命中、钩子退 0 三类都是 `$? == 0`。
    pub fn run_cells(&self) -> u32 {
        self.hit + self.direct + self.fallback
    }

    /// 没跑成但仍留在 PATH 判定范围内的槽数（126 一类）。
    pub fn stop_cells(&self) -> u32 {
        self.denied
    }

    /// 四档合计（必须等于 accept）。
    pub fn tier_sum(&self) -> u32 {
        self.hit.saturating_add(self.direct).saturating_add(self.denied).saturating_add(self.fallback)
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("path-notfound corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" hit=");
        s.push_str(u32_text(self.hit).as_str());
        s.push_str(" direct=");
        s.push_str(u32_text(self.direct).as_str());
        s.push_str(" denied=");
        s.push_str(u32_text(self.denied).as_str());
        s.push_str(" fallback=");
        s.push_str(u32_text(self.fallback).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染的逐字钉版。
const RENDER_OK: &str = "path-notfound corpus=14 accept=8 reject=6 hit=2 direct=2 denied=1 fallback=3 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入的作用是把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，报表就
/// 必须掉 matched、rate 随之离开 1000。若注入后仍报全绿，说明 matched 是硬编码
/// 的，而不是算出来的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(PN_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut hit = 0u32;
    let mut direct = 0u32;
    let mut denied = 0u32;
    let mut fallback = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(PN_ENOBS),
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
            None => return Err(PN_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Hit => hit += 1,
                Tier::Direct => direct += 1,
                Tier::Denied => denied += 1,
                Tier::Fallback => fallback += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, hit, direct, denied, fallback, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 观测纵线是 `word → build → abs → path → cwd → scan → hit → exec → tail`，
/// 九段只能向前。回退（例如已经判死在 tail 段又声称停在 scan 段）说明申报者不是
/// 按闸顺序观测的，而是拿着 `$?` 倒推——这类申报在真实 shell 里最危险，因为
/// 127 这个数字在四条路上都可能出现，倒推者挑哪条都能自圆其说。
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
            return Err(PN_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(PN_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(PN_ESTATE);
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

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::from("slot=");
        s.push_str(dec_text(self.slot as u32).as_str());
        s.push_str(" op=");
        s.push_str(dec_text(self.op as u32).as_str());
        s.push_str(" ok=");
        s.push_str(bool_text(self.ok));
        s.push_str(" code=");
        s.push_str(seg_text(self.code).as_str());
        s.push_str(" value=");
        s.push_str(dec_text(self.value).as_str());
        s
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
            return Err(PN_ESLOT);
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
            return Err(PN_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PN_EBAL),
            };
            if s.slot as usize != i {
                return Err(PN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PN_EVALUE);
                        }
                    }
                    None => return Err(PN_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(PN_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PN_EVALUE);
                        }
                    }
                    None => return Err(PN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(PN_EBAL);
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
            return Err(PN_EBAL);
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
            Err(PN_ESEAL)
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
    let _ = l.register(count_cell(3, OP_BUILD, r.map(|x| x.hit)));
    let _ = l.register(count_cell(4, OP_ABS, r.map(|x| x.direct)));
    let _ = l.register(count_cell(5, OP_EXEC, r.map(|x| x.denied)));
    let _ = l.register(count_cell(6, OP_TAIL, r.map(|x| x.fallback)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_TAIL,
                ok: false,
                code: f.code(),
                value: 0,
            });
        }
        i += 1;
    }
    l
}

/// 阶段闸门路：九段只能向前，回退、已关再走都要被拒绝。
fn gate_ok() -> bool {
    let all = [
        Stage::Word,
        Stage::Build,
        Stage::Abs,
        Stage::Path,
        Stage::Cwd,
        Stage::Scan,
        Stage::Hit,
        Stage::Exec,
        Stage::Tail,
    ];
    let mut g = Gate::new();
    let mut i = 0usize;
    while i < all.len() {
        let s = match all.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() != i || g.step(s).is_err() {
            return false;
        }
        if g.last() != i as i32 || g.steps() != (i + 1) as u32 {
            return false;
        }
        i += 1;
    }
    if g.close() != Ok(all.len() as u32) {
        return false;
    }
    // 已关再走/再关都是状态错。
    if g.step(Stage::Tail).is_ok() || g.close().is_ok() {
        return false;
    }
    // 回退：倒退段被拒；同账步数与 closed 位对得上。
    let mut h = Gate::new();
    if h.step(Stage::Scan).is_err() || h.step(Stage::Exec).is_err() {
        return false;
    }
    if h.step(Stage::Abs).is_ok() {
        return false;
    }
    if h.closed() || h.steps() != 2 {
        return false;
    }
    if h.close() != Ok(2u32) || !h.closed() {
        return false;
    }
    true
}

/// 账本路：建账逐格对账、冻结重放、冻结后登记被拒。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES || l.audit().is_err() {
        return false;
    }
    // 槽位必须连续：跳槽登记要被拒，重复槽也要被拒。
    let mut bad = Ledger::new();
    if bad.register(count_cell(1, OP_CORPUS, Ok(14))).is_ok() {
        return false;
    }
    if bad.register(count_cell(0, OP_CORPUS, Ok(14))).is_err() {
        return false;
    }
    if bad.register(count_cell(0, OP_CORPUS, Ok(14))).is_ok() {
        return false;
    }
    // 冻结后指纹可读、重放一致；冻结后登记与再冻结都被拒。
    let mut m = build_ledger();
    let root = match m.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0 || m.root() != root || !m.is_frozen() || m.replay() != Ok(root) {
        return false;
    }
    if m.register(count_cell(0, OP_CORPUS, Ok(14))).is_ok() {
        return false;
    }
    if m.freeze().is_ok() || m.rejects() != 1 {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 注入
// ---------------------------------------------------------------------------

/// 四路注入：形状表、判据、渲染、账本。
///
/// 四路各自独立，任何一路静默退化都会在这里变成 false：
/// 1. 表路——段基址、枚举、shell 画像、目录事实与形状合法域；
/// 2. 判路——九闸纵线、归因三元组、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && policy_ok() && shape_ok();
    out[1] = define_ok() && tail_ok() && judge_ok() && matrix_ok();
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

// ---------------------------------------------------------------------------
// 十三判据的私有谓词（第 1 组：表路）
// ---------------------------------------------------------------------------

/// 本卡标签（检查项名前缀，与卡面 `TAG` 一栏逐字一致）。
pub const TAG: &'static str = "PN";

/// 自检域之名（`CheckSet::new` 用的那一个）。
pub const DOMAIN: &'static str = "unx11666_path_notfound";

/// 13 个错误码按声明次序排成一列——码表连续性检查的唯一入口。
///
/// 写成函数而不是常量，是为了让"码列"与 `code_line()` 的调用点一起被审查：任何
/// 一个 `PN_E*` 改了值，这里必然断链，而改掉的常量又同时被 `Fault::code()` 与
/// `Ledger::audit()` 使用，三处不可能各自圆谎。
fn code_line() -> [u32; 13] {
    [
        PN_ESTAGE, PN_ECODE, PN_EVALUE, PN_ENOTE, PN_EDEFAULT, PN_ETAIL, PN_ESTATE, PN_ECOUNT,
        PN_ESLOT, PN_ESEAL, PN_ENOBS, PN_EMATRIX, PN_EBAL,
    ]
}

/// 14 档缺陷按 `Fault::code()` 的应然次序排成一列。
fn fault_line() -> [Fault; 14] {
    [
        Fault::Ok, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default,
        Fault::Tail, Fault::Estate, Fault::Count, Fault::Slot, Fault::Seal, Fault::Nobs,
        Fault::Matrix, Fault::Bal,
    ]
}

/// 八个 errno 的集合（与 `strerror` 的分支表一一对应）。
fn errno_line() -> [i32; ERRNO_SET_SIZE] {
    [
        ERR_NONE_E, ERR_NOENT, ERR_NOTDIR, ERR_ISDIR, ERR_ACCES, ERR_NOEXEC, ERR_LOOP,
        ERR_NAMETOOLONG,
    ]
}

/// 判据 01：段基址、码列连续、尺寸常量、退出码档与文本助手的逐字形状。
///
/// 这一路的红线是"数字对不对"，不是"数字好不好"：`0x6d1d0` 是卡面钉死的段基址，
/// 13 个码必须从 `+0x01` 起连续到 `+0x0d` 且不重号——账本里败格的码就是它们，
/// 一旦重号，审计就会把两个不同的缺陷读成同一个。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d1d0 {
        return false;
    }
    if TAG.len() != 2 || !TAG.starts_with("P") || !TAG.ends_with("N") {
        return false;
    }
    if DOMAIN.len() != 24 || !DOMAIN.starts_with("unx11666_") {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if ACCEPT + REJECT != CORPUS || SAMPLES != CORPUS {
        return false;
    }
    if GATE_COUNT != 9 || DIR_SLOTS != 10 || MAX_ELEMENTS != 8 || DEF_ELEMS != 2 {
        return false;
    }
    if NAME_MAX != 255 || errno_line().len() != ERRNO_SET_SIZE {
        return false;
    }
    // 退出码档：0 / 1 / 126 / 127 / 128 五档的相对次序是 POSIX 定的
    if EXIT_OK != 0 || EXIT_GENERAL != 1 || EXIT_DENIED != 126 {
        return false;
    }
    if EXIT_NOTFOUND != 127 || EXIT_SIGNAL_BASE != 128 || EXIT_MASK != 255 {
        return false;
    }
    if EXIT_DENIED + 1 != EXIT_NOTFOUND || EXIT_NOTFOUND + 1 != EXIT_SIGNAL_BASE {
        return false;
    }
    // 操作码：1..=11 连续且计数等于表长
    let ops = [
        OP_WORD, OP_BUILD, OP_ABS, OP_PATH, OP_CWD, OP_SCAN, OP_HIT, OP_EXEC, OP_TAIL,
        OP_CORPUS, OP_REPLAY,
    ];
    if ops.len() != OP_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < ops.len() {
        let v = match ops.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v as usize != i + 1 {
            return false;
        }
        i += 1;
    }
    // 13 个错误码：连续、不重号、都落在段内
    let codes = code_line();
    i = 0usize;
    while i < codes.len() {
        let v = match codes.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v != SEG_BASE.saturating_add((i as u32) + 1) {
            return false;
        }
        if v <= SEG_BASE || v > SEG_BASE + 0x0d {
            return false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if let Some(w) = codes.get(j) {
                if *w == v {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // errno 集合：八个值互不相同，且都能查到文本
    let es = errno_line();
    i = 0usize;
    while i < es.len() {
        let e = match es.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if strerror(e).is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < es.len() {
            if let Some(f) = es.get(j) {
                if *f == e {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if !strerror(99).is_empty() || !strerror(-1).is_empty() {
        return false;
    }
    // 数字助手的逐字形状
    if dec_text(0) != "0" || dec_text(7) != "7" || dec_text(126) != "126" {
        return false;
    }
    if dec_text(929) != "929" || dec_text(4294967295) != "4294967295" {
        return false;
    }
    if u32_text(1000) != "1000" || u32_text(0) != "0" {
        return false;
    }
    if hex_text(0x6d1d1, 6) != "06d1d1" || hex_text(0, 4) != "0000" || hex_text(0xffff, 4) != "ffff"
    {
        return false;
    }
    if i32_text(0) != "0" || i32_text(-2) != "-2" || i32_text(21) != "21" {
        return false;
    }
    if bool_text(true) != "true" || bool_text(false) != "false" {
        return false;
    }
    if seg_text(SEG_BASE) != "0x06d1d0" || seg_text(PN_ESTAGE) != "0x06d1d1" {
        return false;
    }
    if errno_text(ERR_ACCES) != "errno=13 Permission denied" {
        return false;
    }
    if errno_text(ERR_NONE_E) != "errno=0 No error"
        || errno_text(ERR_NOENT) != "errno=2 No such file or directory"
    {
        return false;
    }
    if attrib_text(true) != "path-end" || attrib_text(false) != "early-stop" {
        return false;
    }
    // 程序名与文案族
    if prog_name(Shell::Bash) != "bash" || prog_name(Shell::Ash) != "ash" {
        return false;
    }
    if prog_name(Shell::Dash) != "/bin/sh" {
        return false;
    }
    true
}

/// 判据 02：枚举全覆盖、九闸单调、落点与闸相容、缺陷码一一对应。
///
/// 这一路管的是"枚举不许漏档"。`NOTE_SET`/`CAND_SET`/`TIER_SET` 三张表如果漏了
/// 一个档，`message()` 的 `match` 就少一条分支（编译期红），但**分支齐了不代表
/// 语义齐**：落点↔闸、档位↔落点、缺陷↔码三组映射必须逐条对表，错一条就会把
/// "abs 段落的 127"记成"尽头给的 127"。
fn enums_ok() -> bool {
    // 九闸：GATE_LINE 的次序就是 order()，且 Stage::at 反查同表
    if GATE_LINE.len() != GATE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < GATE_LINE.len() {
        let st = match GATE_LINE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if st.order() != i {
            return false;
        }
        match Stage::at(i) {
            Some(v) => {
                if v != st {
                    return false;
                }
            }
            None => return false,
        }
        if st.name().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < GATE_LINE.len() {
            if let Some(w) = GATE_LINE.get(j) {
                if w.name() == st.name() || *w == st {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if Stage::at(GATE_COUNT).is_some() {
        return false;
    }
    // 九闸的三个布尔轴：可归因者唯有 tail；零目录者恰好四段；已扫描者恰好四段
    let mut attributable = 0usize;
    let mut zero_dirs = 0usize;
    let mut searched = 0usize;
    i = 0usize;
    while i < GATE_COUNT {
        let st = match Stage::at(i) {
            Some(v) => v,
            None => return false,
        };
        if st.attributable() {
            attributable += 1;
            if st != Stage::Tail {
                return false;
            }
        }
        if st.zero_dirs() {
            zero_dirs += 1;
        }
        if st.searched() {
            searched += 1;
            if st.order() < Stage::Scan.order() {
                return false;
            }
        }
        i += 1;
    }
    if attributable != 1 || zero_dirs != 4 || searched != 4 {
        return false;
    }
    // 落点表：19 档齐全、名号唯一、都能落到某一闸，且与自身 stage() 相容
    if NOTE_SET.len() != 19 {
        return false;
    }
    i = 0usize;
    while i < NOTE_SET.len() {
        let n = match NOTE_SET.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if n.name().is_empty() {
            return false;
        }
        if !note_stage_ok(n, n.stage()) {
            return false;
        }
        let mut hit = 0usize;
        let mut j = 0usize;
        while j < GATE_COUNT {
            if let Some(st) = Stage::at(j) {
                if note_stage_ok(n, st) {
                    hit += 1;
                }
            }
            j += 1;
        }
        // 除 exec/abs 两处共用的四档外，每档只许落在恰好一闸
        if hit != 1 && hit != 2 {
            return false;
        }
        if n.path_end_only() != (n.stage() == Stage::Tail) {
            return false;
        }
        if n.notfound_family() && n.code() != EXIT_NOTFOUND {
            return false;
        }
        if n.denied_family() && n.code() != EXIT_DENIED {
            return false;
        }
        if n.success() && n != Note::Script && n.code() != EXIT_OK {
            return false;
        }
        if n.silent_zero() != (n == Note::HookOk) {
            return false;
        }
        if n.silent_zero() && !n.success() {
            return false;
        }
        let mut k = i + 1;
        while k < NOTE_SET.len() {
            if let Some(w) = NOTE_SET.get(k) {
                if *w == n || w.name() == n.name() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    // 候选表：8 档齐全，exists/executable 恰有一处重叠空集（RegularExec 唯一可跑者）
    if CAND_SET.len() != 8 || Cand::ZeroLen.index() != 7 {
        return false;
    }
    let mut exec_n = 0usize;
    let mut exist_n = 0usize;
    i = 0usize;
    while i < CAND_SET.len() {
        let c = match CAND_SET.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c.index() != i {
            return false;
        }
        if c.name().is_empty() {
            return false;
        }
        if c.executable() {
            exec_n += 1;
            if !c.exists() || c != Cand::RegularExec {
                return false;
            }
        }
        if c.exists() {
            exist_n += 1;
        }
        if c.ash_skips() != !c.executable() {
            return false;
        }
        // 126 族 ⇔ errno 落 126 档且不可执行；ZeroLen 是唯一例外（ENOEXEC 之后
        // 由 shell 按脚本重跑，码归脚本，本卡不许把它算进"找到了但没法执行"）
        if c != Cand::ZeroLen {
            if c.denied_family() != (errno_exit(c.errno()) == EXIT_DENIED && !c.executable()) {
                return false;
            }
        }
        if errno_exit(c.errno()) == EXIT_OK && c != Cand::RegularExec {
            return false;
        }
        i += 1;
    }
    if exec_n != 1 || exist_n != 7 {
        return false;
    }
    // 档位表：四档 index 连续、名号唯一
    if TIER_SET.len() != TIER_COUNT {
        return false;
    }
    i = 0usize;
    while i < TIER_SET.len() {
        let t = match TIER_SET.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if t.index() != i {
            return false;
        }
        if t.name().is_empty() {
            return false;
        }
        if t == Tier::Denied {
            // denied 档是本卡唯一不许出现码 0 的一档：它收的全部是"没跑成、
            // 又没走到尽头"的落点，故其中不许混进成功族（Script 除外：它的码
            // 由脚本给，0 与非 0 两可），也不许混进尽头族。
            let mut k = 0usize;
            while k < NOTE_SET.len() {
                let n = match NOTE_SET.get(k) {
                    Some(x) => *x,
                    None => return false,
                };
                if t.admits(n) {
                    if n.path_end_only() {
                        return false;
                    }
                    if n.success() && n != Note::Script {
                        return false;
                    }
                }
                k += 1;
            }
        }
        if t == Tier::Fallback {
            // fallback 档 ⇔ path_end_only：三档尽头落点全收，其余一档不收
            let mut k = 0usize;
            while k < NOTE_SET.len() {
                let n = match NOTE_SET.get(k) {
                    Some(x) => *x,
                    None => return false,
                };
                if t.admits(n) != n.path_end_only() {
                    return false;
                }
                k += 1;
            }
        }
        if t == Tier::Direct {
            // direct 档 ⇔ abs 闸的真实产出：把八档候选逐一推到 abs 闸，
            // 落点必须全被本档准入；换到 exec 闸则一律不许进 direct。
            let mut k = 0usize;
            while k < CAND_SET.len() {
                let c = match CAND_SET.get(k) {
                    Some(x) => *x,
                    None => return false,
                };
                let a = exec_outcome(Stage::Abs, c, 0, 0);
                if Tier::of(&a) != Tier::Direct || !t.admits(a.note) {
                    return false;
                }
                let e = exec_outcome(Stage::Exec, c, 1, 0);
                if Tier::of(&e) == Tier::Direct || !Tier::of(&e).admits(e.note) {
                    return false;
                }
                k += 1;
            }
        }
        let mut j = i + 1;
        while j < TIER_SET.len() {
            if let Some(w) = TIER_SET.get(j) {
                if w.name() == t.name() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if Tier::Denied.may_zero() || !Tier::Hit.may_zero() || !Tier::Fallback.may_zero() {
        return false;
    }
    if Tier::Direct.scanned() || !Tier::Fallback.scanned() || !Tier::Denied.scanned() {
        return false;
    }
    // 投影与准入表互为逆命题：把 19 档落点各自喂给 Tier::of，其档位必须准入该落点
    i = 0usize;
    while i < NOTE_SET.len() {
        let n = match NOTE_SET.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let o = Outcome { stage: n.stage(), code: n.code(), value: 0, note: n, default: n.path_end_only() };
        if !Tier::of(&o).admits(n) {
            return false;
        }
        i += 1;
    }
    // 词形态：4 档，可搜索者唯有 Normal
    if WordKind::Normal.searchable() != true {
        return false;
    }
    if WordKind::Empty.searchable() || WordKind::AssignOnly.searchable() {
        return false;
    }
    if WordKind::Overlong.searchable() || !WordKind::Empty.is_noop() {
        return false;
    }
    if WordKind::AssignOnly.is_noop() != true || WordKind::Normal.is_noop() {
        return false;
    }
    if WordKind::Overlong.is_noop() {
        return false;
    }
    // PATH 形态：空串完全不扫；未设截到默认表长
    if !PathState::Empty.no_search() || PathState::Set.no_search() || PathState::Unset.no_search()
    {
        return false;
    }
    if PathState::Empty.elements(8) != 0 || PathState::Set.elements(3) != 3 {
        return false;
    }
    if PathState::Unset.elements(9) != DEF_ELEMS || PathState::Unset.elements(1) != 1 {
        return false;
    }
    // 哈希与钩子
    if !HashState::Fresh.answers_first() || HashState::Miss.answers_first() {
        return false;
    }
    if HashState::Stale.answers_first() {
        return false;
    }
    if !HashState::Fresh.reuse_is_ok() || HashState::Stale.reuse_is_ok() {
        return false;
    }
    if !HashState::Miss.reuse_is_ok() {
        return false;
    }
    if HookState::Absent.overrides_rc() {
        return false;
    }
    if !HookState::Zero.overrides_rc() || !HookState::NonZero.overrides_rc() {
        return false;
    }
    if HookState::Absent.code(9).is_some() {
        return false;
    }
    if HookState::Zero.code(9) != Some(EXIT_OK) || HookState::NonZero.code(9) != Some(9) {
        return false;
    }
    // 缺陷表：码列与 PN_E* 一一对应，alive ⇔ 非零，短名与 FAULT_SHORT 同序
    let fs = fault_line();
    if fs.len() != 14 || FAULT_SHORT.len() != 14 {
        return false;
    }
    i = 0usize;
    while i < fs.len() {
        let f = match fs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want = match i {
            0 => 0u32,
            _ => match code_line().get(i - 1) {
                Some(v) => *v,
                None => return false,
            },
        };
        if f.code() != want {
            return false;
        }
        if f.alive() != (f.code() != 0) {
            return false;
        }
        match FAULT_SHORT.get(i) {
            Some(t) => {
                if *t != f.short() {
                    return false;
                }
            }
            None => return false,
        }
        // 六维申报缺陷恰好是 Estate/Stage/Code/Value/Note/Default 这六个（fault_line
        // 里位于下标 1..=5 与 7），Tail/Count/Slot/Seal/Nobs/Matrix/Bal 都是结构维，
        // 账本败格只收前者——这条映射错一个，build_ledger 的六格就会与审计对不上。
        let want_dim = (i >= 1 && i <= 5) || i == 7;
        if f.is_claim_dim() != want_dim {
            return false;
        }
        i += 1;
    }
    // 六维申报缺陷必须恰好六个，且与 REJECT_FAULTS 同集合
    let mut dims = 0usize;
    i = 0usize;
    while i < REJECT_FAULTS.len() {
        if let Some(f) = REJECT_FAULTS.get(i) {
            if f.is_claim_dim() {
                dims += 1;
            }
        }
        i += 1;
    }
    if dims != REJECT || REJECT_FAULTS.len() != REJECT {
        return false;
    }
    i = 0usize;
    while i < REJECT {
        let f = match REJECT_FAULTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let c = match REJECT_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if f.code() != c {
            return false;
        }
        if c != fault_expect(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// `REJECT_CODES` 的应然值：第 `i` 个败格收第 `i` 号缺陷码。
///
/// 这张小表的存在理由与期望表相同——`REJECT_CODES` 与 `Fault::code()` 是两份
/// 陈述，任何一处被改都必须在这里露出来，而不是靠账本恰好没审计到。
fn fault_expect(i: usize) -> u32 {
    match i {
        0 => PN_ESTATE,
        1 => PN_ESTAGE,
        2 => PN_ECODE,
        3 => PN_EVALUE,
        4 => PN_ENOTE,
        5 => PN_EDEFAULT,
        _ => PN_EBAL,
    }
}

/// POSIX 的 PATH 元素数：分隔符数加一（空字段也占一格，那一格是当前目录）。
fn path_elems(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut n = 1usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == b':' {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 该 shell 在画像表里应然的四个轴（与 `policies()` 互为两份陈述）。
fn expect_stop(sh: Shell) -> bool {
    sh == Shell::Bash || sh == Shell::Dash
}

/// 钩子只属于 bash：另外两家配了也只是被忽略。
fn expect_hook(sh: Shell) -> bool {
    sh == Shell::Bash
}

/// 只有 dash 的文案带行号。
fn expect_lineno(sh: Shell) -> bool {
    sh == Shell::Dash
}

/// Debian `disable-cwd` 那一类补丁只打在 bash 上（本卡的画像设定）。
fn expect_cwd_refused(sh: Shell) -> bool {
    sh == Shell::Bash
}

/// 判据 03：shell 画像与候选策略轴。
///
/// 本卡最深的差异就在这一轴上：**同一份 PATH、同一个无执行位的普通件，bash 给
/// 126、ash 给 127，两个都是对的**。画像表若与 `Shell::accepts` 不同源，跨 shell
/// 的对照复算就会把"对的"判成"错的"，所以这里要求画像与函数逐 shell 相等，
/// 并且逐条与卡面事实（谁停在首个实体、谁跳过续扫、谁有钩子、谁带行号）对齐。
fn policy_ok() -> bool {
    let ps = policies();
    if ps.len() != SHELL_SET.len() || SHELL_SET.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < ps.len() {
        let p = match ps.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let sh = match SHELL_SET.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if p.shell != sh {
            return false;
        }
        if !p.agrees_with(sh) {
            return false;
        }
        if i != sh.index() {
            return false;
        }
        if p.notfound_code != EXIT_NOTFOUND || p.denied_code != EXIT_DENIED {
            return false;
        }
        if p.stop_first != expect_stop(sh) || p.hook != expect_hook(sh) {
            return false;
        }
        if p.lineno != expect_lineno(sh) || p.cwd_refused != expect_cwd_refused(sh) {
            return false;
        }
        if sh.walk_bound() != MAX_ELEMENTS {
            return false;
        }
        if sh.default_path().is_empty() {
            return false;
        }
        if path_elems(sh.default_path()) != DEF_ELEMS {
            return false;
        }
        if sh.uses_lineno() != expect_lineno(sh) || sh.has_hook() != expect_hook(sh) {
            return false;
        }
        if sh.stops_on_first_entity() != expect_stop(sh) {
            return false;
        }
        if sh.refuses_cwd_by_default() != expect_cwd_refused(sh) {
            return false;
        }
        if sh.notfound_family().is_empty() {
            return false;
        }
        i += 1;
    }
    // 画像表不许有两个 shell 同序，也不许有 shell 缺席
    i = 0usize;
    while i < ps.len() {
        let a = match ps.get(i) {
            Some(x) => x.shell,
            None => return false,
        };
        let mut j = i + 1;
        while j < ps.len() {
            if let Some(b) = ps.get(j) {
                if b.shell == a {
                    return false;
                }
            }
            j += 1;
        }
        let mut found = false;
        let mut k = 0usize;
        while k < SHELL_SET.len() {
            if let Some(s) = SHELL_SET.get(k) {
                if *s == a {
                    found = true;
                }
            }
            k += 1;
        }
        if !found {
            return false;
        }
        i += 1;
    }
    // 候选策略轴逐档核对：POSIX 流派收"存在即候选"，ash 流派收"可执行才候选"
    let mut k = 0usize;
    while k < CAND_SET.len() {
        let c = match CAND_SET.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if Shell::Bash.accepts(c) != c.exists() {
            return false;
        }
        if Shell::Dash.accepts(c) != c.exists() {
            return false;
        }
        if Shell::Ash.accepts(c) != c.executable() {
            return false;
        }
        if c.denied_family() && (Shell::Bash.accepts(c) == Shell::Ash.accepts(c)) {
            // 126 族档正是两家的分水岭：bash 收它、ash 跳它，决不许同收或同跳
            return false;
        }
        if c == Cand::RegularExec && !(Shell::Bash.accepts(c) && Shell::Ash.accepts(c)) {
            return false;
        }
        if c == Cand::Absent && (Shell::Bash.accepts(c) || Shell::Ash.accepts(c)) {
            return false;
        }
        k += 1;
    }
    // 悬空链接是"存在但 execve 回 ENOENT"的档：bash 收它并拿到 127，
    // 这就是 127 的第二来源，绝不算进 126 族。
    if !Cand::Dangling.exists() || Cand::Dangling.executable() || Cand::Dangling.denied_family() {
        return false;
    }
    if cand_errno(Cand::Dangling) != ERR_NOENT {
        return false;
    }
    if errno_exit(cand_errno(Cand::Dangling)) != EXIT_NOTFOUND {
        return false;
    }
    if errno_exit(cand_errno(Cand::RegularNoExec)) != EXIT_DENIED {
        return false;
    }
    if errno_exit(cand_errno(Cand::ZeroLen)) != EXIT_DENIED {
        return false;
    }
    if cand_errno(Cand::RegularExec) != ERR_NONE_E {
        return false;
    }
    if !Cand::RegularNoExec.ash_skips() || !Cand::Subdir.ash_skips() || Cand::RegularExec.ash_skips()
    {
        return false;
    }
    // 文案族归属：bash/ash 说满 `command not found`，dash 说 `not found` 且行号紧跟程序名
    if msg_notfound(Shell::Bash, "zzz", 1) != "bash: zzz: command not found" {
        return false;
    }
    if msg_notfound(Shell::Ash, "zzz", 1) != "ash: zzz: command not found" {
        return false;
    }
    if msg_notfound(Shell::Dash, "zzz", 1) != "/bin/sh: 1: zzz: not found" {
        return false;
    }
    if msg_notfound(Shell::Dash, "foo", 42) != "/bin/sh: 42: foo: not found" {
        return false;
    }
    if !Shell::Bash.notfound_family().contains("command not found") {
        return false;
    }
    if Shell::Dash.notfound_family().contains("command") {
        return false;
    }
    if msg_ash_skip("foo") != msg_notfound(Shell::Ash, "foo", 1) {
        return false;
    }
    if msg_dash_notfound("foo", 3) != msg_notfound(Shell::Dash, "foo", 3) {
        return false;
    }
    if msg_bash_denied("/usr/local/bin/foo") != msg_path_error(Shell::Bash, "/usr/local/bin/foo", ERR_ACCES, 1)
    {
        return false;
    }
    if msg_bash_denied("/usr/local/bin/foo") != "bash: /usr/local/bin/foo: Permission denied" {
        return false;
    }
    if msg_no_such(Shell::Dash, "/tmp/nope", 9) != "/bin/sh: 9: /tmp/nope: No such file or directory" {
        return false;
    }
    if msg_is_a_directory(Shell::Bash, "/bin/logs", 1) != "bash: /bin/logs: Is a directory" {
        return false;
    }
    if msg_too_many_links(Shell::Ash, "/bin/loop", 1) != "ash: /bin/loop: Too many levels of symbolic links"
    {
        return false;
    }
    if msg_script_ran(EXIT_OK).is_empty() != true {
        return false;
    }
    if msg_script_ran(3) != "script exited with 3" {
        return false;
    }
    if msg_hook_ignored("zzz", Shell::Ash) != "handler configured but ash has no command_not_found_handle for 'zzz' (ignored, exit 127)"
    {
        return false;
    }
    true
}

/// 判据 04：形状合法域。
///
/// 语料的 14 个形状必须先合法，判据才有意义：目录名越出全集、`used` 越过表宽、
/// 超出 `used` 的格子没留白、哈希态与记住的路径不成对、钩子态与返回码不成对，
/// 都会让"扫到第几个目录"这一维变成没根的数字。这一路还把 `Shape` 的三个派生量
/// （`walk_len`/`fact`/`first_exec`）与 `spec_text`/`resolved_path` 的读数对齐，
/// 于是形状自相矛盾时红的是形状，而不是被它喂进 `define` 之后的判决。
fn shape_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < SHAPES.len() {
        let s = match SHAPES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if s.tag.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < SHAPES.len() {
            if let Some(w) = SHAPES.get(j) {
                if w.tag == s.tag {
                    return false;
                }
            }
            j += 1;
        }
        // 词形与词长：正常词不许空串，越界词的词长必须真越过 NAME_MAX
        match s.word_kind {
            WordKind::Normal => {
                if s.word.is_empty() || s.word_len as usize != s.word.len() {
                    return false;
                }
            }
            WordKind::Empty => {
                if !s.word.is_empty() || s.word_len != 0 {
                    return false;
                }
            }
            WordKind::AssignOnly => {
                if s.word.is_empty() || s.word_len as usize != s.word.len() {
                    return false;
                }
            }
            WordKind::Overlong => {
                if (s.word_len as usize) <= NAME_MAX {
                    return false;
                }
            }
        }
        if s.line == 0 {
            return false;
        }
        // PATH 形态与声明元素数
        if s.used > MAX_ELEMENTS {
            return false;
        }
        match s.path_state {
            PathState::Set => {
                if s.used == 0 {
                    return false;
                }
            }
            PathState::Unset => {
                if s.used != DEF_ELEMS {
                    return false;
                }
            }
            PathState::Empty => {
                if s.used != 0 {
                    return false;
                }
            }
        }
        if s.walk_len() > DIR_SLOTS || s.walk_len() > s.shell.walk_bound() {
            return false;
        }
        if s.path_state.elements(s.used) < s.walk_len() {
            return false;
        }
        // 声明的表与拼出来的表必须是同一份：元素数与分隔符数互证
        let spec = spec_text(&s);
        if s.path_state == PathState::Set {
            if path_elems(spec.as_str()) != s.used {
                return false;
            }
        } else if s.path_state == PathState::Empty {
            if !spec.is_empty() {
                return false;
            }
        } else if path_elems(spec.as_str()) != s.path_state.elements(s.used) {
            return false;
        }
        // 逐格：used 之内必须在全集里，used 之外必须留白
        let mut k = 0usize;
        while k < DIR_SLOTS {
            let f = match s.dirs.get(k) {
                Some(x) => *x,
                None => return false,
            };
            if k < s.walk_len() {
                if !dir_known(f.path) {
                    return false;
                }
            } else if f.path != "unused" || f.cand != Cand::Absent {
                return false;
            }
            if f.is_cwd() != (f.path.is_empty() || f.path == ".") {
                return false;
            }
            if f.answers() != f.cand.exists() {
                return false;
            }
            if f.chosen_by(s.shell) != s.shell.accepts(f.cand) {
                return false;
            }
            k += 1;
        }
        // fact() 与 walk_len() 同界
        if s.fact(s.walk_len()).is_some() {
            return false;
        }
        if s.walk_len() > 0 && s.fact(0).is_none() {
            return false;
        }
        // 哈希三件套
        match s.hash_state {
            HashState::Miss => {
                if !s.hash_path.is_empty() || s.hash_reuse {
                    return false;
                }
            }
            HashState::Fresh => {
                if s.hash_path.is_empty() || s.hash_reuse {
                    return false;
                }
            }
            HashState::Stale => {
                if s.hash_path.is_empty() {
                    return false;
                }
            }
        }
        if s.hash_reuse && s.hash_state != HashState::Stale {
            return false;
        }
        // 钩子态与返回码成对
        match s.hook {
            HookState::Absent => {
                if s.hook_rc != 0 {
                    return false;
                }
            }
            HookState::Zero => {
                if s.hook_rc != 0 {
                    return false;
                }
            }
            HookState::NonZero => {
                if s.hook_rc == 0 || s.hook_rc > EXIT_MASK {
                    return false;
                }
            }
        }
        if s.hook_active() != (s.shell.has_hook() && s.hook.overrides_rc()) {
            return false;
        }
        // 内建与直路：带斜杠的词不是内建，且其路径必须在全集里
        if s.builtin && (s.word_kind != WordKind::Normal || has_slash(s.word)) {
            return false;
        }
        if s.direct() {
            if !has_slash(s.word) || s.word_kind != WordKind::Normal {
                return false;
            }
            if !dir_known(s.word) {
                return false;
            }
            if trunc_word(s.word) != s.word {
                // 直路词必须从第一个斜杠起整条给出（相对形态 "./x" 由 cwd 档另判）
                return false;
            }
        } else if has_slash(s.word) && s.word_kind == WordKind::Normal {
            return false;
        }
        // 派生量与语料表复算一次：第一个可执行档的下标必须与形状自己一致
        if let Some(k0) = s.first_exec() {
            if k0 >= s.walk_len() {
                return false;
            }
            let f = match s.dirs.get(k0) {
                Some(x) => *x,
                None => return false,
            };
            if !f.cand.executable() {
                return false;
            }
            if s.shell.accepts(f.cand) != true {
                return false;
            }
        }
        // 内建与哈希先于 PATH：这两段只要有答复，闸就绝不许走到 build 之后，
        // 且尝试目录数必须为 0——"内建也扫了一遍 PATH"是最典型的双执行 bug。
        if s.builtin || s.hash_state.answers_first() {
            let o = define(&s);
            if o.stage.order() > Stage::Build.order() || o.value != 0 {
                return false;
            }
        }
        // 陈旧项被复用停在 build 段且不回扫；不复用则必须回扫（闸走到 build 之后）
        if s.hash_state == HashState::Stale {
            let o = define(&s);
            if s.hash_reuse {
                if o.stage != Stage::Build || o.value != 0 || o.default {
                    return false;
                }
            } else if o.stage == Stage::Build {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 复算场景构造器（谓词与测试共用；语料表仍是逐格写明的 SHAPES）
// ---------------------------------------------------------------------------

/// 场景基架：bash、词 `zzz`、PATH 已设、无哈希、无钩子、允许 cwd。
fn scene(tag: &'static str) -> Shape {
    let mut s = Shape::blank();
    s.tag = tag;
    s
}

/// 逐目录命中（第一格就有真件）。
fn scene_hit_ash() -> Shape {
    let mut s = scene("sc-hit-ash");
    s.shell = Shell::Ash;
    let d = [DirFact::new("/bin", Cand::RegularExec), DirFact::new("/usr/bin", Cand::RegularExec)];
    s = s.with_facts(2, d.as_slice());
    s
}

/// 同一张表交给 bash 与 ash：无执行位在前，真件在后（本卡的分水岭场景）。
fn scene_noexec_split(sh: Shell) -> Shape {
    let mut s = scene("sc-noexec-split");
    s.shell = sh;
    let d = [
        DirFact::new("/usr/local/bin", Cand::RegularNoExec),
        DirFact::new("/bin", Cand::RegularExec),
    ];
    s.with_facts(2, d.as_slice())
}

/// 带斜杠的直路命中。
fn scene_abs_hit() -> Shape {
    let mut s = scene("sc-abs-hit");
    s.word = "/tmp/ok-exec";
    s.word_len = 12;
    s.abs_cand = Cand::RegularExec;
    let d = [DirFact::new("/bin", Cand::RegularExec), DirFact::new("/usr/bin", Cand::RegularExec)];
    s.with_facts(2, d.as_slice())
}

/// 带斜杠的直路落空（127，但不是尽头）。
fn scene_abs_miss() -> Shape {
    let mut s = scene("sc-abs-miss");
    s.word = "/tmp/nope";
    s.word_len = 9;
    s.abs_cand = Cand::Absent;
    let d = [DirFact::new("/bin", Cand::RegularExec), DirFact::new("/usr/bin", Cand::RegularExec)];
    s.with_facts(2, d.as_slice())
}

/// 尽头未命中（本卡主现场的复算形）。
fn scene_tail(sh: Shell) -> Shape {
    let mut s = scene("sc-tail");
    s.shell = sh;
    let d = [DirFact::new("/bin", Cand::Absent), DirFact::new("/usr/bin", Cand::Absent)];
    s.with_facts(2, d.as_slice())
}

/// 尽头钩子：`rc` 是钩子返回码，0 表示钩子退 0。
fn scene_hook(sh: Shell, rc: u32) -> Shape {
    let mut s = scene_tail(sh);
    s.tag = "sc-hook";
    if rc == 0 {
        s = s.with_hook(HookState::Zero, 0);
    } else {
        s = s.with_hook(HookState::NonZero, rc);
    }
    s
}

/// 当前目录元素被策略拒绝，且被拒的那一格本来能命中。
fn scene_cwd_block() -> Shape {
    let mut s = scene("sc-cwd-block");
    s.shell = Shell::Ash;
    let d = [DirFact::new("", Cand::RegularExec), DirFact::new("/bin", Cand::RegularExec)];
    s.with_facts(2, d.as_slice()).with_cwd(false)
}

/// 当前目录元素被拒，但那一格给不出答复：整格跳过，扫描继续。
fn scene_cwd_skip() -> Shape {
    let mut s = scene("sc-cwd-skip");
    s.shell = Shell::Ash;
    let d = [
        DirFact::new("", Cand::RegularNoExec),
        DirFact::new("/usr/local/bin", Cand::Absent),
        DirFact::new("/bin", Cand::RegularExec),
    ];
    s.with_facts(3, d.as_slice()).with_cwd(false)
}

/// 陈旧哈希项：`reuse` 为真时直接复用（错路），为假时回扫 PATH（对路）。
fn scene_stale(reuse: bool) -> Shape {
    let mut s = scene_tail(Shell::Bash);
    s.tag = "sc-stale";
    s.hash_state = HashState::Stale;
    s.hash_path = "/bin/zzz";
    s.hash_reuse = reuse;
    s
}

/// 内建答复。
fn scene_builtin() -> Shape {
    let mut s = scene_tail(Shell::Bash);
    s.tag = "sc-builtin";
    s.builtin = true;
    s.word = "cd";
    s.word_len = 2;
    s
}

/// 哈希命中（记住的路径还在）。
fn scene_cached() -> Shape {
    let mut s = scene_tail(Shell::Bash);
    s.tag = "sc-cached";
    s.hash_state = HashState::Fresh;
    s.hash_path = "/bin/zzz";
    s
}

/// PATH 元素数越过上界守卫（畸形 PATH，扫到界即停）。
fn scene_walk_cap() -> Shape {
    let mut s = scene("sc-walk-cap");
    s.shell = Shell::Ash;
    let d = [
        DirFact::new("/bin", Cand::Absent),
        DirFact::new("/usr/bin", Cand::Absent),
        DirFact::new("/sbin", Cand::Absent),
        DirFact::new("/usr/sbin", Cand::Absent),
        DirFact::new("/usr/local/bin", Cand::Absent),
        DirFact::new("/usr/local/sbin", Cand::Absent),
        DirFact::new("/opt/bin", Cand::Absent),
        DirFact::new("/mnt/nofs", Cand::Absent),
        DirFact::new("/bin", Cand::RegularExec),
    ];
    s.with_facts(9, d.as_slice())
}

/// 悬空符号链接（bash 收它当候选，`execve` 回 `ENOENT`）。
fn scene_dangling() -> Shape {
    let mut s = scene("sc-dangling");
    s.shell = Shell::Bash;
    let d = [DirFact::new("/bin", Cand::Dangling), DirFact::new("/usr/bin", Cand::RegularExec)];
    s.with_facts(2, d.as_slice())
}

/// `PATH=` 空串：一个目录都不扫。
fn scene_path_empty() -> Shape {
    let mut s = scene("sc-path-empty");
    s.shell = Shell::Bash;
    s.path_state = PathState::Empty;
    let d = [DirFact::new("/bin", Cand::RegularExec), DirFact::new("/usr/bin", Cand::RegularExec)];
    s.with_facts(2, d.as_slice())
}

/// `ENOEXEC`：零长件被 shell 当脚本重跑，码归脚本。
fn scene_script(rc: u32) -> Shape {
    let mut s = scene("sc-script");
    s.shell = Shell::Bash;
    s.script_rc = rc;
    let d = [DirFact::new("/bin", Cand::ZeroLen), DirFact::new("/usr/bin", Cand::Absent)];
    s.with_facts(2, d.as_slice())
}

/// 判据 05：九闸纵线（谁先谁后、谁拦下、`value` 记的是第几格）。
///
/// 这一路把"闸门次序"从文档变成机器：每一段都在自己的场景上被逐一拦下过，
/// 且每段的产出都被复算一次。特别地，无执行位那一格在 bash 与 ash 之下的两个
/// 合法答复（126 停在原地 / 127 扫到底）必须同时成立——本卡的跨 shell 深度
/// 全在这一对上，任何"替对方改写数字"的实现都会在这一路红。
fn define_ok() -> bool {
    // ---- 语料逐槽：自洽、可重放、档位准入 ----
    let mut i = 0usize;
    while i < SHAPES.len() {
        let s = match SHAPES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let a = define(&s);
        let b = define(&s);
        if a.stage != b.stage || a.code != b.code || a.value != b.value || a.note != b.note {
            return false;
        }
        if a.seal() != b.seal() {
            return false;
        }
        if !a.consistent(&s) {
            return false;
        }
        if !note_stage_ok(a.note, a.stage) {
            return false;
        }
        if !Tier::of(&a).admits(a.note) {
            return false;
        }
        if a.value > s.walk_len() as u32 {
            return false;
        }
        if a.default != a.note.path_end_only() {
            return false;
        }
        if a.attributed() != (a.default && a.stage == Stage::Tail) {
            return false;
        }
        if a.borrowed_127() != (a.code == EXIT_NOTFOUND && !a.default) {
            return false;
        }
        if a.unscanned(&s) > s.walk_len() as u32 {
            return false;
        }
        // 完美申报必须判成一致
        let perfect = Case { shape: s, claim: claim_of(&a) };
        if judge(&perfect) != Fault::Ok {
            return false;
        }
        // 逐段推进的闸门必须正好走到该段的次序
        let mut g = Gate::new();
        let mut k = 0usize;
        while k <= a.stage.order() {
            let st = match Stage::at(k) {
                Some(v) => v,
                None => return false,
            };
            if g.step(st).is_err() {
                return false;
            }
            k += 1;
        }
        if g.steps() as usize != a.stage.order() + 1 {
            return false;
        }
        if g.close().is_err() {
            return false;
        }
        i += 1;
    }
    // ---- word 段：三类非搜索词各拦一次 ----
    let mut w1 = scene_tail(Shell::Ash);
    w1.word_kind = WordKind::Empty;
    w1.word = "";
    w1.word_len = 0;
    let o1 = define(&w1);
    if o1.stage != Stage::Word || o1.code != EXIT_OK || o1.note != Note::EmptyWord || o1.value != 0
    {
        return false;
    }
    if !o1.ok() || o1.default {
        return false;
    }
    if Tier::of(&o1) != Tier::Hit {
        return false;
    }
    let mut w2 = scene_tail(Shell::Ash);
    w2.word_kind = WordKind::AssignOnly;
    w2.word = "A=1";
    w2.word_len = 3;
    let o2 = define(&w2);
    if o2.stage != Stage::Word || o2.note != Note::AssignOnly || o2.code != EXIT_OK {
        return false;
    }
    if message(&w2, &o2) != "assignment-only word: POSIX legal no-op, exit 0, PATH untouched" {
        return false;
    }
    let mut w3 = scene_tail(Shell::Ash);
    w3.word_kind = WordKind::Overlong;
    w3.word_len = 300;
    let o3 = define(&w3);
    if o3.stage != Stage::Word || o3.note != Note::BadName || o3.code != EXIT_DENIED {
        return false;
    }
    if o3.consistent(&w3) != true {
        return false;
    }
    if message(&w3, &o3) != "command word is 300 bytes, over NAME_MAX (255); no stat issued" {
        return false;
    }
    if Tier::of(&o3) != Tier::Denied || !Tier::Denied.admits(o3.note) {
        return false;
    }
    // ---- build 段：内建与哈希都必须在 PATH 之前，且一个目录都不扫 ----
    let sb = scene_builtin();
    let ob = define(&sb);
    if ob.stage != Stage::Build || ob.code != EXIT_OK || ob.note != Note::Builtin || ob.value != 0
    {
        return false;
    }
    if message(&sb, &ob) != "builtin 'cd' answered before PATH (0 dirs consulted)" {
        return false;
    }
    let sc = scene_cached();
    let oc = define(&sc);
    if oc.stage != Stage::Build || oc.note != Note::Cached || oc.value != 0 || !oc.ok() {
        return false;
    }
    if message(&sc, &oc) != "hash hit for 'zzz' -> /bin/zzz (PATH skipped)" {
        return false;
    }
    // ---- abs 段：带斜杠完全不查 PATH（value 恒 0），命中与落空同闸 ----
    let sa = scene_abs_hit();
    let oa = define(&sa);
    if oa.stage != Stage::Abs || oa.note != Note::Direct || oa.code != EXIT_OK || oa.value != 0 {
        return false;
    }
    if oa.unscanned(&sa) != sa.path_state.elements(sa.used) as u32 {
        return false;
    }
    if Tier::of(&oa) != Tier::Direct {
        return false;
    }
    let sm = scene_abs_miss();
    let om = define(&sm);
    if om.stage != Stage::Abs || om.note != Note::DirectMiss || om.code != EXIT_NOTFOUND {
        return false;
    }
    if om.default || om.attributed() || !om.borrowed_127() {
        return false;
    }
    if message(&sm, &om) != "bash: /tmp/nope: No such file or directory" {
        return false;
    }
    // ---- path 段：PATH 设为空串时一格不扫，但归因仍是"表走完了" ----
    let sp = scene_path_empty();
    let op = define(&sp);
    if op.stage != Stage::Tail || op.note != Note::NotFound || op.value != 0 {
        return false;
    }
    if !op.default || op.code != EXIT_NOTFOUND {
        return false;
    }
    // ---- scan/hit/exec 段：同一张表在两个 shell 下的两个合法数字 ----
    let b1 = scene_noexec_split(Shell::Bash);
    let o126 = define(&b1);
    if o126.stage != Stage::Exec || o126.note != Note::Denied || o126.code != EXIT_DENIED {
        return false;
    }
    if o126.value != 1 || o126.default {
        return false;
    }
    if message(&b1, &o126) != "bash: /usr/local/bin/zzz: Permission denied" {
        return false;
    }
    if Tier::of(&o126) != Tier::Denied {
        return false;
    }
    let a1 = scene_noexec_split(Shell::Ash);
    let o127 = define(&a1);
    if o127.stage != Stage::Exec || o127.note != Note::Hit || o127.code != EXIT_OK {
        return false;
    }
    if o127.value != 2 || o127.hit_index() != 2 || !o127.ok() {
        return false;
    }
    if message(&a1, &o127) != "exec 'zzz' from /bin/zzz (dir #2)" {
        return false;
    }
    if Tier::of(&o127) != Tier::Hit {
        return false;
    }
    // 同一形状的尝试目录数：ash 比 bash 多一格，且两人都没走到尽头
    if o127.value <= o126.value {
        return false;
    }
    if o126.default || o127.default {
        return false;
    }
    // 命中下标决定赢家：把真件挪到前面，bash 也就跟着命中了
    let mut swap = scene("sc-swap-order");
    swap.shell = Shell::Bash;
    let dsw = [
        DirFact::new("/bin", Cand::RegularExec),
        DirFact::new("/usr/local/bin", Cand::RegularNoExec),
    ];
    let swap = swap.with_facts(2, dsw.as_slice());
    let os = define(&swap);
    if os.note != Note::Hit || os.value != 1 {
        return false;
    }
    if resolved_path(&swap, os.value) != "/bin/zzz" {
        return false;
    }
    // ---- exec 段逐档：八档候选在 exec 闸的答复全表复算 ----
    let mut k = 0usize;
    while k < CAND_SET.len() {
        let c = match CAND_SET.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let e = exec_outcome(Stage::Exec, c, 1, 7);
        let a = exec_outcome(Stage::Abs, c, 0, 7);
        if !note_stage_ok(e.note, e.stage) || !note_stage_ok(a.note, a.stage) {
            return false;
        }
        if e.code != a.code {
            return false;
        }
        if e.value != 1 || a.value != 0 {
            return false;
        }
        if e.default || a.default {
            return false;
        }
        match c {
            Cand::RegularExec => {
                if e.note != Note::Hit || a.note != Note::Direct || e.code != EXIT_OK {
                    return false;
                }
            }
            Cand::Absent | Cand::Dangling => {
                if e.note != Note::LinkGone || a.note != Note::DirectMiss {
                    return false;
                }
                if e.code != EXIT_NOTFOUND || a.code != EXIT_NOTFOUND {
                    return false;
                }
                if e.default || a.default {
                    return false;
                }
            }
            Cand::Subdir => {
                if e.note != Note::Subdir || e.code != EXIT_DENIED {
                    return false;
                }
                if message(&scene_abs_hit(), &e) != "bash: /tmp/ok-exec: Is a directory" {
                    return false;
                }
            }
            Cand::RegularNoExec | Cand::NonRegular => {
                if e.note != Note::Denied || e.code != EXIT_DENIED {
                    return false;
                }
            }
            Cand::Loop => {
                if e.note != Note::Loop || e.code != EXIT_DENIED {
                    return false;
                }
            }
            Cand::ZeroLen => {
                if e.note != Note::Script || a.note != Note::Script || e.code != 7 {
                    return false;
                }
            }
        }
        if !e.consistent(&scene_abs_hit()) {
            return false;
        }
        k += 1;
    }
    // ENOEXEC 之后码归脚本：0 与非 0 两可，落点不许变
    let s7 = scene_script(7);
    let o7 = define(&s7);
    if o7.note != Note::Script || o7.code != 7 || o7.stage != Stage::Exec {
        return false;
    }
    if o7.consistent(&s7) != true || message(&s7, &o7) != "script exited with 7" {
        return false;
    }
    if Tier::of(&o7) != Tier::Denied {
        return false;
    }
    let s0 = scene_script(0);
    let o0 = define(&s0);
    if o0.code != EXIT_OK || !o0.ok() || Tier::of(&o0) != Tier::Hit {
        return false;
    }
    if message(&s0, &o0).is_empty() != true || !o0.consistent(&s0) {
        return false;
    }
    true
}

/// 判据 06：回退四去路的归因（尽头、钩子、cwd 被拒、陈旧哈希）。
///
/// 这四条去路都会（或都可能会）报 127，区别只在**归因位与尝试目录数**：
/// 尽头那条真的把表走完了，其余三条都在尽头之前停下。这一路把四条去路逐条
/// 走一遍，并要求"报了 127 却停在前面"的三条一律挂假归因位——本卡不许 127
/// 这个数字替实现补一句"其实扫完了"。
fn tail_ok() -> bool {
    // ---- 去路一：PATH 尽头没找到（唯一挂真归因位的一条） ----
    let ash = scene_tail(Shell::Ash);
    let oa = define(&ash);
    if oa.stage != Stage::Tail || oa.note != Note::NotFound || oa.code != EXIT_NOTFOUND {
        return false;
    }
    if !oa.default || !oa.attributed() || oa.value != 2 {
        return false;
    }
    if message(&ash, &oa) != "ash: zzz: command not found" {
        return false;
    }
    if Tier::of(&oa) != Tier::Fallback || !Tier::Fallback.admits(oa.note) {
        return false;
    }
    // 三条 shell 的尽头族文案同族不同字：bash/ash 说满，dash 带行号
    let dash = scene_tail(Shell::Dash);
    let od = define(&dash);
    if od.note != Note::NotFound || od.code != EXIT_NOTFOUND || !od.attributed() {
        return false;
    }
    if message(&dash, &od) != "/bin/sh: 1: zzz: not found" {
        return false;
    }
    if message(&ash, &oa) == message(&dash, &od) {
        return false;
    }
    // 尽头那条不许把尝试数报少：`unscanned` 必须归零
    if oa.unscanned(&ash) != 0 || od.unscanned(&dash) != 0 {
        return false;
    }
    // ---- 去路二：钩子顶掉 127（只有 bash 真有钩子） ----
    let h0 = scene_hook(Shell::Bash, 0);
    let oh0 = define(&h0);
    if oh0.stage != Stage::Tail || oh0.note != Note::HookOk || oh0.code != EXIT_OK {
        return false;
    }
    if !oh0.ok() || !oh0.default || oh0.value != 2 {
        return false;
    }
    if oh0.silent_zero_check() != true || !oh0.note.silent_zero() {
        return false;
    }
    if message(&h0, &oh0) != "command_not_found_handle ran for 'zzz' and returned 0 (nothing was executed)"
    {
        return false;
    }
    if warning(&h0, &oh0).is_empty() {
        return false;
    }
    if Tier::of(&oh0) != Tier::Fallback {
        // 钩子退 0 也进 fallback 档：命令其实没跑，不许替 shell 补一条它没执行的命令
        return false;
    }
    if oh0.consistent(&h0) != true {
        return false;
    }
    let h123 = scene_hook(Shell::Bash, 123);
    let oh1 = define(&h123);
    if oh1.note != Note::HookFail || oh1.code != 123 || !oh1.default || oh1.failed() != true {
        return false;
    }
    if oh1.borrowed_127() {
        return false;
    }
    if message(&h123, &oh1) != "command_not_found_handle ran for 'zzz' and returned 123 (the 127 slot is occupied by the handler)"
    {
        return false;
    }
    if !oh1.consistent(&h123) {
        return false;
    }
    // 另外两家：钩子配了也只是被忽略，码必须仍是 127，且告警行要说清被忽略
    let ignored = scene_hook(Shell::Ash, 0);
    let oi = define(&ignored);
    if oi.note != Note::NotFound || oi.code != EXIT_NOTFOUND || !oi.default {
        return false;
    }
    if warning(&ignored, &oi) != msg_hook_ignored("zzz", Shell::Ash) {
        return false;
    }
    if message(&ignored, &oi) != "ash: zzz: command not found" {
        return false;
    }
    if !ignored.hook.overrides_rc() || ignored.shell.has_hook() {
        return false;
    }
    if ignored.hook_active() {
        return false;
    }
    let nz = scene_hook(Shell::Dash, 9);
    if define(&nz).code != EXIT_NOTFOUND || nz.hook_active() {
        return false;
    }
    // 纯尽头现场没有告警行：两行必须分得开
    if !warning(&ash, &oa).is_empty() {
        return false;
    }
    // ---- 去路三：当前目录被策略拒绝，扫描在尽头之前停 ----
    let cb = scene_cwd_block();
    let oc = define(&cb);
    if oc.stage != Stage::Cwd || oc.note != Note::CwdBlocked || oc.code != EXIT_NOTFOUND {
        return false;
    }
    if oc.default || oc.attributed() || !oc.borrowed_127() {
        return false;
    }
    if oc.value != 0 || oc.unscanned(&cb) != 2 {
        return false;
    }
    if message(&cb, &oc) != "cwd element refused by policy for 'zzz' (0 dirs tried before end, not a PATH-exhaustion verdict)"
    {
        return false;
    }
    if warning(&cb, &oc) != msg_cwd_refused("zzz", 0) {
        return false;
    }
    if Tier::of(&oc) != Tier::Denied {
        return false;
    }
    // 被拒的那格给不出答复时：整格跳过、不计尝试数，扫描继续走完
    let cs = scene_cwd_skip();
    let os2 = define(&cs);
    if os2.stage != Stage::Exec || os2.note != Note::Hit || os2.value != 2 {
        return false;
    }
    if !os2.ok() || os2.default {
        return false;
    }
    if resolved_path(&cs, os2.value) != "/bin/zzz" {
        return false;
    }
    // 同一张表放开 cwd：当前目录就是第一格命中者
    let cw = scene_cwd_block();
    let allow = cw.with_cwd(true);
    let oa2 = define(&allow);
    if oa2.note != Note::Hit || oa2.value != 1 {
        return false;
    }
    if resolved_path(&allow, oa2.value) != "./zzz" {
        return false;
    }
    // ---- 去路四：陈旧哈希项被直接复用 ----
    let sr = scene_stale(true);
    let osr = define(&sr);
    if osr.stage != Stage::Build || osr.note != Note::HashStale || osr.code != EXIT_NOTFOUND {
        return false;
    }
    if osr.value != 0 || osr.default || osr.attributed() || !osr.borrowed_127() {
        return false;
    }
    if osr.unscanned(&sr) != 2 {
        return false;
    }
    if message(&sr, &osr) != "stale hash entry reused: '/bin/zzz' (file gone, PATH was not rescanned)"
    {
        return false;
    }
    if !sr.answered_before_path() {
        return false;
    }
    // 正确做法（回扫）：闸必须越过 build，数字与主现场完全一致
    let rr = scene_stale(false);
    let orr = define(&rr);
    if orr.stage != Stage::Tail || orr.note != Note::NotFound || orr.value != 2 {
        return false;
    }
    if !orr.default || orr.stage == Stage::Build {
        return false;
    }
    if orr.seal() != oa.seal() {
        // ash 与 bash 的尽头现场只在文案上不同，判决五维全同
        let aa = define(&ash);
        if orr.seal() != aa.seal() {
            return false;
        }
    }
    // ---- 附加去路：上界守卫截断（畸形 PATH，从未走到尽头） ----
    let wc = scene_walk_cap();
    let owc = define(&wc);
    if owc.stage != Stage::Scan || owc.note != Note::WalkCap || owc.code != EXIT_NOTFOUND {
        return false;
    }
    if owc.default || owc.borrowed_127() != true || owc.value != MAX_ELEMENTS as u32 {
        return false;
    }
    if owc.unscanned(&wc) != 1 {
        return false;
    }
    if message(&wc, &owc) != "PATH walk truncated at 8 elements (spec declares 9, malformed: the end was never reached)"
    {
        return false;
    }
    if !wc.capped() || wc.walk_len() != MAX_ELEMENTS {
        return false;
    }
    // ---- 悬空链接：execve 回 ENOENT 的第三个 127 来源 ----
    let dg = scene_dangling();
    let odg = define(&dg);
    if odg.stage != Stage::Exec || odg.note != Note::LinkGone || odg.code != EXIT_NOTFOUND {
        return false;
    }
    if odg.default || !odg.borrowed_127() || odg.value != 1 {
        return false;
    }
    if Tier::of(&odg) != Tier::Denied {
        return false;
    }
    if errno_exit(cand_errno(Cand::Dangling)) != odg.code {
        return false;
    }
    // 四条去路的落点互不相同，且只有尽头族挂归因位
    let fam = [
        oa.note, oh0.note, oh1.note, oc.note, osr.note, owc.note, odg.note,
    ];
    let mut n = 0usize;
    while n < fam.len() {
        let x = match fam.get(n) {
            Some(v) => *v,
            None => return false,
        };
        let mut m = n + 1;
        while m < fam.len() {
            if let Some(y) = fam.get(m) {
                if *y == x {
                    return false;
                }
            }
            m += 1;
        }
        n += 1;
    }
    if oa.note.path_end_only() != true || oh0.note.path_end_only() != true {
        return false;
    }
    if oc.note.path_end_only() || osr.note.path_end_only() || owc.note.path_end_only() {
        return false;
    }
    true
}

/// 判据与测试共用的复算场景全集（22 形，覆盖九闸每一段的产出）。
///
/// 这一张表不是为了"多测几个形状"，而是为了让 `judge_ok` 能对**任意**现场断言
/// "完美申报必判一致、单维造假必判对该维"：只在 14 槽语料上验，就验不出"语料
/// 恰好绕开了那个缺陷"的情形。
fn scene_set() -> [Shape; 22] {
    let mut out = [Shape::blank(); 22];
    if let Some(x) = out.get_mut(0) {
        *x = scene_hit_ash();
    }
    if let Some(x) = out.get_mut(1) {
        *x = scene_noexec_split(Shell::Bash);
    }
    if let Some(x) = out.get_mut(2) {
        *x = scene_noexec_split(Shell::Ash);
    }
    if let Some(x) = out.get_mut(3) {
        *x = scene_abs_hit();
    }
    if let Some(x) = out.get_mut(4) {
        *x = scene_abs_miss();
    }
    if let Some(x) = out.get_mut(5) {
        *x = scene_tail(Shell::Ash);
    }
    if let Some(x) = out.get_mut(6) {
        *x = scene_tail(Shell::Bash);
    }
    if let Some(x) = out.get_mut(7) {
        *x = scene_tail(Shell::Dash);
    }
    if let Some(x) = out.get_mut(8) {
        *x = scene_hook(Shell::Bash, 0);
    }
    if let Some(x) = out.get_mut(9) {
        *x = scene_hook(Shell::Bash, 123);
    }
    if let Some(x) = out.get_mut(10) {
        *x = scene_hook(Shell::Ash, 0);
    }
    if let Some(x) = out.get_mut(11) {
        *x = scene_cwd_block();
    }
    if let Some(x) = out.get_mut(12) {
        *x = scene_cwd_skip();
    }
    if let Some(x) = out.get_mut(13) {
        *x = scene_stale(true);
    }
    if let Some(x) = out.get_mut(14) {
        *x = scene_stale(false);
    }
    if let Some(x) = out.get_mut(15) {
        *x = scene_walk_cap();
    }
    if let Some(x) = out.get_mut(16) {
        *x = scene_dangling();
    }
    if let Some(x) = out.get_mut(17) {
        *x = scene_path_empty();
    }
    if let Some(x) = out.get_mut(18) {
        *x = scene_script(7);
    }
    if let Some(x) = out.get_mut(19) {
        *x = scene_script(0);
    }
    if let Some(x) = out.get_mut(20) {
        *x = scene_builtin();
    }
    if let Some(x) = out.get_mut(21) {
        *x = scene_cached();
    }
    out
}

/// 同族换落点（单维注入用：换档不许顺带把成败也换掉）。
fn other_note(n: Note) -> Note {
    if n.success() {
        return match n {
            Note::Hit => Note::Builtin,
            Note::Direct => Note::Cached,
            Note::Builtin => Note::Cached,
            Note::Cached => Note::Hit,
            Note::EmptyWord => Note::AssignOnly,
            Note::AssignOnly => Note::EmptyWord,
            Note::HookOk => Note::Direct,
            Note::Script => Note::Hit,
        };
    }
    match n {
        Note::NotFound => Note::DirectMiss,
        _ => Note::NotFound,
    }
}

/// 同族换退出码（单维注入用：0 ↔ 非 0 会连带翻转成败档，只能挑不会翻的那一边）。
fn other_code(c: u32) -> u32 {
    if c == EXIT_OK {
        EXIT_GENERAL
    } else if c == EXIT_GENERAL {
        EXIT_NOTFOUND
    } else if c == EXIT_DENIED {
        EXIT_GENERAL
    } else {
        EXIT_DENIED
    }
}

/// 前一闸（单维注入用；首闸则取次闸）。
fn other_stage(st: Stage) -> Stage {
    if st.order() == 0 {
        return match Stage::at(1) {
            Some(v) => v,
            None => st,
        };
    }
    match Stage::at(st.order() - 1) {
        Some(v) => v,
        None => st,
    }
}

/// 判据 07：申报对判的六维、固定优先序与值域。
///
/// `judge` 是这张卡的裁决器，它必须同时满足三件事：**完美申报恒判一致**（否则
/// 判据自己发疯）、**单维造假判对该维**（否则维度混在一起没法归因）、**多维造假
/// 按固定优先序判最前那一维**（否则同一份日志两次审计给出两个缺陷码）。这一路在
/// 22 形场景上逐维注入，把三条都跑成真。
fn judge_ok() -> bool {
    let cs = cases();
    // ---- (a) 语料逐槽：judge 与期望表逐字一致 ----
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let e = match expected(i) {
            Some(x) => x,
            None => return false,
        };
        if judge(&c) != e {
            return false;
        }
        if c.judge() != judge(&c) {
            return false;
        }
        if c.ok() != (judge(&c) == Fault::Ok) {
            return false;
        }
        if c.tag() != c.shape.tag {
            return false;
        }
        if (i < ACCEPT) != (judge(&c) == Fault::Ok) {
            return false;
        }
        if judge(&c).alive() != (judge(&c) != Fault::Ok) {
            return false;
        }
        i += 1;
    }
    if expected(CORPUS).is_some() {
        return false;
    }
    if case_at(CORPUS).is_some() || case_at(0).is_none() {
        return false;
    }
    if cover(CORPUS).is_err() || cover(CORPUS - 1).is_ok() {
        return false;
    }
    // ---- (b) 任意场景的完美申报都判一致，且判决自洽 ----
    let ss = scene_set();
    i = 0usize;
    while i < ss.len() {
        let s = match ss.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let a = define(&s);
        if !a.consistent(&s) {
            return false;
        }
        if !Tier::of(&a).admits(a.note) {
            return false;
        }
        let perfect = Case { shape: s, claim: claim_of(&a) };
        if judge(&perfect) != Fault::Ok || !perfect.ok() {
            return false;
        }
        if scene_seal(&s, &a) == 0 {
            return false;
        }
        i += 1;
    }
    // ---- (c) 单维注入逐维命中 ----
    i = 0usize;
    while i < ss.len() {
        let s = match ss.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let a = define(&s);
        let base = claim_of(&a);
        // stage 维：换闸不动成败
        let k = Claim { stage: other_stage(a.stage), ..base };
        if judge(&Case { shape: s, claim: k }) != Fault::Stage {
            return false;
        }
        // value 维：尝试目录数 +1
        let k = Claim { value: a.value.saturating_add(1), ..base };
        if k.value == a.value {
            return false;
        }
        if judge(&Case { shape: s, claim: k }) != Fault::Value {
            return false;
        }
        // note 维：同族换档
        let n2 = other_note(a.note);
        if n2 == a.note {
            return false;
        }
        let k = Claim { note: n2, ..base };
        if judge(&Case { shape: s, claim: k }) != Fault::Note {
            return false;
        }
        // default 维：只翻归因位
        let k = Claim { default: !a.default, ..base };
        if judge(&Case { shape: s, claim: k }) != Fault::Default {
            return false;
        }
        // code 维：0 报成非 0 属成败互报，非 0 报成另一个非 0 才是纯码错
        let c2 = other_code(a.code);
        if c2 == a.code {
            return false;
        }
        let want = if a.code == EXIT_OK { Fault::Estate } else { Fault::Code };
        if judge(&Case { shape: s, claim: Claim { code: c2, ..base } }) != want {
            return false;
        }
        // estate 维：把成败整个翻过去
        let flip = if a.ok() {
            Claim { code: EXIT_NOTFOUND, note: Note::DirectMiss, ..base }
        } else {
            Claim { code: EXIT_OK, note: Note::Hit, ..base }
        };
        if judge(&Case { shape: s, claim: flip }) != Fault::Estate {
            return false;
        }
        i += 1;
    }
    // ---- (d) 多维造假按固定优先序判最前一维 ----
    let s = match SHAPES.get(SCENE_SLOT) {
        Some(x) => *x,
        None => return false,
    };
    let a = define(&s);
    let b = claim_of(&a);
    let all = Claim {
        stage: other_stage(a.stage),
        code: other_code(a.code),
        value: a.value.saturating_add(1),
        note: other_note(a.note),
        default: !a.default,
    };
    if judge(&Case { shape: s, claim: all }) != Fault::Stage {
        return false;
    }
    let no_stage = Claim { stage: a.stage, ..all };
    if judge(&Case { shape: s, claim: no_stage }) != Fault::Code {
        return false;
    }
    let only_tail = Claim { code: a.code, ..no_stage };
    if judge(&Case { shape: s, claim: only_tail }) != Fault::Value {
        return false;
    }
    let two = Claim { value: a.value, ..only_tail };
    if judge(&Case { shape: s, claim: two }) != Fault::Note {
        return false;
    }
    let one = Claim { note: a.note, ..two };
    if judge(&Case { shape: s, claim: one }) != Fault::Default {
        return false;
    }
    if judge(&Case { shape: s, claim: b }) != Fault::Ok {
        return false;
    }
    // ---- (e) 值域只在六个申报维与 Ok 之内，且七档全可达 ----
    let mut seen_ok = false;
    let mut seen = [false; 6];
    i = 0usize;
    while i < CORPUS {
        let f = match cs.get(i) {
            Some(c) => judge(c),
            None => return false,
        };
        if f != Fault::Ok && !f.is_claim_dim() {
            return false;
        }
        if f == Fault::Ok {
            seen_ok = true;
        }
        let idx = fault_slot(&f);
        if idx < 6 {
            if let Some(x) = seen.get_mut(idx) {
                *x = true;
            }
        }
        i += 1;
    }
    if !seen_ok {
        return false;
    }
    i = 0usize;
    while i < seen.len() {
        if let Some(x) = seen.get(i) {
            if !*x {
                return false;
            }
        }
        i += 1;
    }
    // ---- (f) 期望表与 REJECT_FAULTS 同源 ----
    i = ACCEPT;
    while i < CORPUS {
        if expected(i) != REJECT_FAULTS.get(i - ACCEPT).copied() {
            return false;
        }
        i += 1;
    }
    i = 0usize;
    while i < ACCEPT {
        if expected(i) != Some(Fault::Ok) {
            return false;
        }
        i += 1;
    }
    true
}

/// 六维缺陷在 `REJECT_FAULTS` 里的下标（非申报维给 6）。
fn fault_slot(f: &Fault) -> usize {
    match f {
        Fault::Estate => 0,
        Fault::Stage => 1,
        Fault::Code => 2,
        Fault::Value => 3,
        Fault::Note => 4,
        Fault::Default => 5,
        Fault::Ok | Fault::Tail | Fault::Count | Fault::Slot | Fault::Seal | Fault::Nobs
        | Fault::Matrix | Fault::Bal => 6,
    }
}

/// 判据 08：四档矩阵的统计与两份陈述的交叉核对。
///
/// 矩阵按**实际结果**分档，红档槽留在自己那一档里，于是覆盖面看得见；
/// 钩子退 0 那一格必须进 fallback 而不是 hit——工具的 `$?` 是 0，但现场是
/// "PATH 走完了"，把它塞进 hit 档就等于替 shell 补了一条它没执行过的命令。
/// 这一路同时要求矩阵、报表、`WANT_TIER` 期望表三份陈述互相吻合。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS {
        return false;
    }
    if m.goods() != ACCEPT {
        return false;
    }
    if m.slots() - m.goods() != REJECT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    if m.perfect_rows() != 3 {
        return false;
    }
    let want = [
        Row { total: 2, good: 2 },
        Row { total: 2, good: 2 },
        Row { total: 1, good: 1 },
        Row { total: 9, good: 3 },
    ];
    let mut i = 0usize;
    let mut sum = 0usize;
    let mut reds = 0usize;
    while i < TIER_COUNT {
        let t = match TIER_SET.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let r = match m.row(t) {
            Some(x) => x,
            None => return false,
        };
        let w = match want.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if r.total != w.total || r.good != w.good {
            return false;
        }
        if r.red() != w.total - w.good {
            return false;
        }
        if !r.reds_are_replicas() {
            return false;
        }
        if r.perfect() != (r.red() == 0) {
            return false;
        }
        if r.total == 0 {
            return false;
        }
        sum += r.total;
        reds += r.red();
        i += 1;
    }
    if sum != CORPUS || reds != REJECT {
        return false;
    }
    if let Some(f) = m.row(Tier::Fallback) {
        // 回退档 9 格 = 3 个正确槽（尽头、跳档续扫尽头、钩子退 0）+ 6 个复刻槽
        if f.red() != REJECT || f.total != 9 {
            return false;
        }
    } else {
        return false;
    }
    // 矩阵与报表：每档的 good 必须等于该档的正确槽数
    let rep = match run() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if rep.total as usize != CORPUS || rep.accept as usize != ACCEPT || rep.reject as usize != REJECT
    {
        return false;
    }
    if rep.tier_sum() as usize != ACCEPT {
        return false;
    }
    let cells = [rep.hit, rep.direct, rep.denied, rep.fallback];
    i = 0usize;
    while i < TIER_COUNT {
        let t = match TIER_SET.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let r = match m.row(t) {
            Some(x) => x,
            None => return false,
        };
        let v = match cells.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v as usize != r.good {
            return false;
        }
        i += 1;
    }
    if VALS.get(3).copied() != Some(rep.hit) || VALS.get(4).copied() != Some(rep.direct) {
        return false;
    }
    if VALS.get(5).copied() != Some(rep.denied) || VALS.get(6).copied() != Some(rep.fallback) {
        return false;
    }
    if VALS.get(0).copied() != Some(rep.total) || VALS.get(1).copied() != Some(rep.accept) {
        return false;
    }
    if VALS.get(2).copied() != Some(rep.reject) {
        return false;
    }
    if rep.run_cells() != rep.hit + rep.direct + rep.fallback {
        return false;
    }
    if rep.stop_cells() != rep.denied {
        return false;
    }
    if rep.run_cells() + rep.stop_cells() != rep.accept {
        return false;
    }
    // 第三份陈述：WANT_TIER 必须与投影、与申报投影都吻合
    i = 0usize;
    while i < CORPUS {
        let c = match cs_ref(i) {
            Some(x) => x,
            None => return false,
        };
        let t = match WANT_TIER.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if Tier::of(&c.actual()) != t {
            return false;
        }
        if c.claim.tier() == Tier::Denied && c.claim.success() {
            return false;
        }
        if c.claim.claims_path_end() != c.claim.default {
            return false;
        }
        if c.claim.default && c.claim.tier() != Tier::Fallback {
            return false;
        }
        if c.actual().default && t != Tier::Fallback {
            return false;
        }
        i += 1;
    }
    // 渲染：四行、逐字、含红档数
    let txt = m.render();
    if txt != "hit total=2 good=2 red=0\ndirect total=2 good=2 red=0\ndenied total=1 good=1 red=0\nfallback total=9 good=3 red=6\n"
    {
        return false;
    }
    if txt.matches('\n').count() != TIER_COUNT {
        return false;
    }
    let blank = Matrix::blank();
    if blank.slots() != 0 || blank.goods() != 0 || blank.covered() || blank.perfect_rows() != 0 {
        return false;
    }
    if blank.row(Tier::Hit).is_none() {
        return false;
    }
    if Row::blank().perfect() || Row::blank().red() != 0 {
        return false;
    }
    true
}

/// 取一槽用例（`case_at` 的 `Option` 在判据里不便直接接，包一层）。
fn cs_ref(i: usize) -> Option<Case> {
    case_at(i)
}

/// 六个申报维的名字（复刻槽标签的第二份陈述）。
fn dim_name(i: usize) -> &'static str {
    match i {
        0 => "estate",
        1 => "stage",
        2 => "code",
        3 => "value",
        4 => "note",
        5 => "default",
        _ => "unknown",
    }
}

/// 申报与实际的差异维数（复刻槽必须恰好差一维，成败档除外）。
fn deviate(a: &Outcome, k: &Claim) -> usize {
    let mut n = 0usize;
    if a.stage != k.stage {
        n += 1;
    }
    if a.code != k.code {
        n += 1;
    }
    if a.value != k.value {
        n += 1;
    }
    if a.note != k.note {
        n += 1;
    }
    if a.default != k.default {
        n += 1;
    }
    n
}

/// 取一槽的期望申报（六张 `WANT_*` 分列表拼回一条 `Claim`）。
///
/// 期望表在文件里是**按维分列**写的——因为分列才能一眼看出某一维在 14 槽上的
/// 全部取值（比如 `WANT_VALUE` 那一行就是逐目录序的账本）。判据要逐槽比对，
/// 就得先把它拼回申报形状，这层转换本身也走 `Option`，不留越界索引。
fn want_claim(i: usize) -> Option<Claim> {
    let st = match WANT_STAGE.get(i) {
        Some(x) => *x,
        None => return None,
    };
    let cd = match WANT_CODE.get(i) {
        Some(x) => *x,
        None => return None,
    };
    let vl = match WANT_VALUE.get(i) {
        Some(x) => *x,
        None => return None,
    };
    let nt = match WANT_NOTE.get(i) {
        Some(x) => *x,
        None => return None,
    };
    let df = match WANT_DEFAULT.get(i) {
        Some(x) => *x,
        None => return None,
    };
    Some(Claim { stage: st, code: cd, value: vl, note: nt, default: df })
}

/// 判据 09：语料表与三份陈述（形状、申报、期望表）互相吻合。
///
/// `SHAPES`/`CLAIMS` 是"现场与申报"，《WANT_*》是"校验器按纵线实际会读到的数"，
/// `run()` 的报表是"判决汇总"。三份必须两两对得上：任何一份被单独改坏（哪怕只是
/// 把一个 `note` 从 `NotFound` 改成 `WalkCap`），另一份就会当场指出来。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if WANT_STAGE.len() != CORPUS || WANT_CODE.len() != CORPUS || WANT_VALUE.len() != CORPUS {
        return false;
    }
    if WANT_NOTE.len() != CORPUS || WANT_DEFAULT.len() != CORPUS || WANT_SHELL.len() != CORPUS {
        return false;
    }
    if WANT_TAGS.len() != CORPUS || WANT_TIER.len() != CORPUS {
        return false;
    }
    if REJECT_FAULTS.len() != REJECT || REJECT_CODES.len() != REJECT {
        return false;
    }
    let cs = cases();
    if cover(cs.len()).is_err() {
        return false;
    }
    // 表宽对了还要能取到：越界槽一律当失败处理。
    if cs_ref(CORPUS).is_some() {
        return false;
    }
    let mut hit_n = 0usize;
    let mut direct_n = 0usize;
    let mut denied_n = 0usize;
    let mut fallback_n = 0usize;
    let mut ok_n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let a = c.actual();
        let w = match want_claim(i) {
            Some(x) => x,
            None => return false,
        };
        // 实际纵线读到的五维，逐维与期望表对。
        if a.stage != w.stage {
            return false;
        }
        if a.code != w.code {
            return false;
        }
        if a.value != w.value {
            return false;
        }
        if a.note != w.note {
            return false;
        }
        if a.default != w.default {
            return false;
        }
        // 形状侧的两份陈述：shell 与标签。标签是槽位身份，写重了整本账就混了。
        let wsh = match WANT_SHELL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c.shape.shell != wsh {
            return false;
        }
        let wtag = match WANT_TAGS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c.shape.tag != wtag {
            return false;
        }
        if c.tag() != wtag {
            return false;
        }
        // 实际结论必须过五维自洽这一关（归因位 ⇔ 落点族 ⇔ 闸位 ⇔ 码 ⇔ 目录数）。
        if !a.consistent(&c.shape) {
            return false;
        }
        // 期望表与申报表在两族槽上的关系是不同的：前 8 槽两者就是同一条申报，
        // 后 6 槽申报表是被故意改坏的那一份，故只许与期望表差申报维。
        let f = judge(&c);
        let want_f = match expected(i) {
            Some(x) => x,
            None => return false,
        };
        if f != want_f {
            return false;
        }
        if i < ACCEPT {
            if c.claim != w {
                return false;
            }
            if f != Fault::Ok {
                return false;
            }
            ok_n += 1;
            match Tier::of(&a) {
                Tier::Hit => hit_n += 1,
                Tier::Direct => direct_n += 1,
                Tier::Denied => denied_n += 1,
                Tier::Fallback => fallback_n += 1,
            }
        } else {
            if f == Fault::Ok {
                return false;
            }
            if !f.is_claim_dim() {
                return false;
            }
            let wcode = match REJECT_CODES.get(i - ACCEPT) {
                Some(x) => *x,
                None => return false,
            };
            if f.code() != wcode {
                return false;
            }
            if f.code() < SEG_BASE + 0x01 || f.code() > SEG_BASE + 0x0d {
                return false;
            }
        }
        i += 1;
    }
    if ok_n != ACCEPT {
        return false;
    }
    if CORPUS - ok_n != REJECT {
        return false;
    }
    // 汇总侧：报表与逐槽统计必须同数，且 `VALS` 前七格就是这张报表。
    let rep = match run() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if hit_n as u32 != rep.hit || direct_n as u32 != rep.direct {
        return false;
    }
    if denied_n as u32 != rep.denied || fallback_n as u32 != rep.fallback {
        return false;
    }
    if rep.rate() != 1000 || !rep.green() {
        return false;
    }
    if rep.gate().is_err() {
        return false;
    }
    if rep.render() != RENDER_OK {
        return false;
    }
    if !render_ok_shape_ok() {
        return false;
    }
    let mut v = 0usize;
    while v < ACCEPT {
        let x = match VALS.get(v) {
            Some(y) => *y,
            None => return false,
        };
        let want = match [rep.total, rep.accept, rep.reject, rep.hit, rep.direct, rep.denied, rep.fallback, rep.rate()].get(v) {
            Some(y) => *y,
            None => return false,
        };
        if x != want {
            return false;
        }
        v += 1;
    }
    // 期望表的码列与归因列必须自洽：127 只在归因位为真或落点属 127 族时出现。
    i = 0usize;
    while i < CORPUS {
        let w = match want_claim(i) {
            Some(x) => x,
            None => return false,
        };
        if w.code == EXIT_NOTFOUND && !w.note.notfound_family() && w.note != Note::HookFail {
            return false;
        }
        if w.note == Note::NotFound && !w.default {
            return false;
        }
        if w.note.path_end_only() && w.stage != Stage::Tail {
            return false;
        }
        i += 1;
    }
    true
}

/// 十进制串 → 数（渲染文本的反向读法，供文案审计用）。
///
/// 有专门的必要：`dec_text` 只能"数 → 串"，而审计钉版文案要的是"串 → 数"，
/// 否则就只能把整串逐字比对一遍——那只检查了拼写，没检查数字到底对不对。
fn dec_of(text: &str) -> Option<u32> {
    if text.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut seen = 0usize;
    for ch in text.chars() {
        let d = match ch {
            '0' => 0u32,
            '1' => 1u32,
            '2' => 2u32,
            '3' => 3u32,
            '4' => 4u32,
            '5' => 5u32,
            '6' => 6u32,
            '7' => 7u32,
            '8' => 8u32,
            '9' => 9u32,
            _ => return None,
        };
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => return None,
        };
        v = match v.checked_add(d) {
            Some(x) => x,
            None => return None,
        };
        seen += 1;
    }
    if seen == 0 {
        return None;
    }
    Some(v)
}

/// 取 `键=值` 里的值部分。
fn field_value(token: &str, key: &str) -> Option<&str> {
    if !token.starts_with(key) {
        return None;
    }
    if token.len() <= key.len() {
        return None;
    }
    Some(&token[key.len()..])
}

/// 两份申报在六个比较位上的差异标志（位序与 `judge` 的短路次序一致）。
fn claim_flags(a: &Claim, b: &Claim) -> [bool; 6] {
    [
        a.success() != b.success(),
        a.stage != b.stage,
        a.code != b.code,
        a.value != b.value,
        a.note != b.note,
        a.default != b.default,
    ]
}

/// 差异标志里数得出几个真。
fn flags_true(f: &[bool; 6]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < 6 {
        if f[i] {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 判据 10：槽位分区与复刻槽的"只改一维"。
///
/// 后 6 槽不是六个新现场，而是**同一个主现场被单独改坏一维**的六份复刻。这件事
/// 必须用结构钉死：形状除标签外与主现场逐字段相同，申报除被点名的那一维外与主
/// 现场逐维相同。否则"红档"就可能是六个各说各话的现场，缺陷类别与病灶之间没有
/// 可复核的对应关系——那张表就退化成"有 6 个槽报错了"这种没有信息量的统计。
fn slots_ok() -> bool {
    if ACCEPT + REJECT != CORPUS {
        return false;
    }
    if ACCEPT_SLOTS.len() != ACCEPT || REJECT_SLOTS.len() != REJECT {
        return false;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        if ACCEPT_SLOTS[i] != i {
            return false;
        }
        i += 1;
    }
    i = 0usize;
    while i < REJECT {
        if REJECT_SLOTS[i] != ACCEPT + i {
            return false;
        }
        i += 1;
    }
    if SCENE_SLOT >= ACCEPT {
        return false;
    }
    let base = match SHAPES.get(SCENE_SLOT) {
        Some(x) => *x,
        None => return false,
    };
    let base_claim = match CLAIMS.get(SCENE_SLOT) {
        Some(x) => *x,
        None => return false,
    };
    // 主现场本身必须是申报正确的那一类：PATH 走完、归因位为真、结论是 not found。
    let base_actual = base_claim;
    let _ = base_actual;
    let bo = define(&base);
    if bo.note != Note::NotFound || !bo.default || bo.stage != Stage::Tail {
        return false;
    }
    if judge(&Case { shape: base, claim: base_claim }) != Fault::Ok {
        return false;
    }
    // 标签唯一（含 8 个活槽）：标签是槽位身份，重名等于两槽共用一份事实。
    let mut seen = [""; CORPUS];
    i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if s.tag.is_empty() {
            return false;
        }
        let mut k = 0usize;
        while k < i {
            if let Some(p) = seen.get(k) {
                if *p == s.tag {
                    return false;
                }
            }
            k += 1;
        }
        if let Some(dst) = seen.get_mut(i) {
            *dst = s.tag;
        }
        i += 1;
    }
    // 8 个活槽的形状两两不同：同一现场不许占两个申报正确的位置。
    i = 0usize;
    while i < ACCEPT {
        let a = match SHAPES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < ACCEPT {
            let b = match SHAPES.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 6 个复刻槽：形状 = 主现场改标签，申报 = 主现场改一维，标签点名那一维。
    i = 0usize;
    while i < REJECT {
        let slot = match REJECT_SLOTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let s = match SHAPES.get(slot) {
            Some(x) => *x,
            None => return false,
        };
        let mut expect = base;
        if let Some(w) = WANT_TAGS.get(slot) {
            expect.tag = *w;
        } else {
            return false;
        }
        if s != expect {
            return false;
        }
        if s.word != base.word || s.shell != base.shell || s.used != base.used {
            return false;
        }
        let mut k = 0usize;
        while k < DIR_SLOTS {
            let one = match s.dirs.get(k) {
                Some(x) => *x,
                None => return false,
            };
            let two = match base.dirs.get(k) {
                Some(x) => *x,
                None => return false,
            };
            if one != two {
                return false;
            }
            k += 1;
        }
        // 标签的第二份陈述：`dim_name(i)` + 后缀。
        let mut tname = String::from(dim_name(i));
        tname.push_str(SWAP_SUFFIX);
        if s.tag != tname.as_str() {
            return false;
        }
        let want_fault = match REJECT_FAULTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if fault_slot(&want_fault) != i {
            return false;
        }
        let c = match CLAIMS.get(slot) {
            Some(x) => *x,
            None => return false,
        };
        let fl = claim_flags(&base_claim, &c);
        if flags_true(&fl) < 1 {
            return false;
        }
        if i == 0 {
            // 成败位单独不可翻（`success()` 由码与落点导出），所以这一格必须
            // 跨多维翻，且第一处差异一定落在成败位上——这正是 `judge` 先比成败的理由。
            if !fl[0] {
                return false;
            }
            if flags_true(&fl) < 2 {
                return false;
            }
            if !c.success() {
                return false;
            }
        } else {
            if flags_true(&fl) != 1 {
                return false;
            }
            if !fl[i] {
                return false;
            }
            if fl[0] {
                return false;
            }
            if c.success() != base_claim.success() {
                return false;
            }
        }
        if judge(&Case { shape: s, claim: c }) != want_fault {
            return false;
        }
        let dv = deviate(&define(&s), &c);
        if i == 0 {
            if dv < 2 {
                return false;
            }
        } else if dv != 1 {
            return false;
        }
        i += 1;
    }
    // 缺陷码表不单调：`estate` 在判定次序上是第一位、在码表里却是第七位。
    // 这一点要钉住，否则读表的人会以为两者同序，把 REJECT_CODES 当成升序去二分。
    let r0 = match REJECT_CODES.get(0) {
        Some(x) => *x,
        None => return false,
    };
    let r1 = match REJECT_CODES.get(1) {
        Some(x) => *x,
        None => return false,
    };
    if r1 >= r0 {
        return false;
    }
    if REJECT_CODES.len() != REJECT {
        return false;
    }
    i = 1;
    while i < REJECT {
        let a = match REJECT_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let b = match REJECT_CODES.get(i - 1) {
            Some(x) => *x,
            None => return false,
        };
        if i >= 2 && a <= b {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据 11：红档的显式性与注入效果。
///
/// `run_with` 的注入点只有一个 `(槽, 缺陷)`，报表必须**真的**跟着动：期望表本身
/// 也在被测面上。若 `matched` 是硬编码的 14，注入后仍报全绿——这一条就是专门抓
/// 这种实现的。同时钉住 `(0, Code)` 这一格产出的 929：账本第 8 格的红档率不是
/// 抄来的常数，是这一路算出来的。
fn reds_ok() -> bool {
    let clean = match run() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if !clean.green() || clean.rate() != 1000 {
        return false;
    }
    if clean.reject as usize != REJECT || clean.accept as usize != ACCEPT {
        return false;
    }
    // 每个复刻槽：缺陷显式等于表里的期望，且报表把它记成 reject。
    let mut i = 0usize;
    while i < REJECT {
        let slot = match REJECT_SLOTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let c = match cs_ref(slot) {
            Some(x) => x,
            None => return false,
        };
        let want = match REJECT_FAULTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        if expected(slot) != Some(want) {
            return false;
        }
        if !want.alive() || !want.is_claim_dim() {
            return false;
        }
        if want.code() == 0 {
            return false;
        }
        if detail(&c).is_empty() {
            return false;
        }
        i += 1;
    }
    // 每个活槽：缺陷必须是 Ok，期望表也必须是 Ok。
    i = 0usize;
    while i < ACCEPT {
        let c = match cs_ref(i) {
            Some(x) => x,
            None => return false,
        };
        if judge(&c) != Fault::Ok || expected(i) != Some(Fault::Ok) {
            return false;
        }
        if !c.ok() {
            return false;
        }
        i += 1;
    }
    // 越界槽一律拒绝，不许静默当成"没注入"。
    if run_with(Some((CORPUS, Fault::Code))).is_ok() {
        return false;
    }
    if run_with(Some((CORPUS + 99, Fault::Ok))).is_ok() {
        return false;
    }
    if run_with(Some((0, Fault::Code))).is_err() {
        return false;
    }
    // 把某槽"注入成它本来就有的缺陷"，报表必须一字不变——注入只在真的改坏时生效。
    i = 0usize;
    while i < CORPUS {
        let e = match expected(i) {
            Some(x) => x,
            None => return false,
        };
        let same = match run_with(Some((i, e))) {
            Ok(x) => x,
            Err(_) => return false,
        };
        if same.total != clean.total || same.matched != clean.matched {
            return false;
        }
        if same.accept != clean.accept || same.reject != clean.reject {
            return false;
        }
        if same.rate() != clean.rate() {
            return false;
        }
        if same.render() != clean.render() {
            return false;
        }
        i += 1;
    }
    // 反向：注入成另一种缺陷，matched 必须掉，rate 必须离开 1000。
    let faults = match fault_line().get(..) {
        Some(x) => *x,
        None => return false,
    };
    i = 0usize;
    while i < CORPUS {
        let e = match expected(i) {
            Some(x) => x,
            None => return false,
        };
        let mut k = 0usize;
        while k < faults.len() {
            let f = match faults.get(k) {
                Some(x) => *x,
                None => return false,
            };
            if f != e {
                let r = match run_with(Some((i, f))) {
                    Ok(x) => x,
                    Err(_) => return false,
                };
                if r.matched != clean.matched - 1 {
                    return false;
                }
                if r.rate() == 1000 || r.green() {
                    return false;
                }
                if r.gate().is_ok() {
                    return false;
                }
                if r.render() == clean.render() {
                    return false;
                }
                if r.total != CORPUS as u32 {
                    return false;
                }
                if r.accept + r.reject != r.total {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    // 第 8 格红档率：注入 `(0, Code)` 得到 13/14 → 929，且必须等于 `VALS[7]`。
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if red.matched != CORPUS as u32 - 1 {
        return false;
    }
    if red.rate() != 929 {
        return false;
    }
    if red.accept as usize != ACCEPT - 1 || red.reject as usize != REJECT + 1 {
        return false;
    }
    if red.hit as usize != 1 {
        return false;
    }
    if red.tier_sum() as usize != ACCEPT - 1 {
        return false;
    }
    if VALS[7] != red.rate() {
        return false;
    }
    // 千分位的四舍五入方向：13/14 若向下取整是 928，若向上是 930，只有这条公式给 929。
    let hand = red.matched.saturating_mul(1000).saturating_add(red.total / 2) / red.total;
    if hand != red.rate() {
        return false;
    }
    // 全绿时的 rate 恒为 1000；空表（total 0）不许炸，只能给 0。
    let empty = Report {
        total: 0,
        accept: 0,
        reject: 0,
        hit: 0,
        direct: 0,
        denied: 0,
        fallback: 0,
        matched: 0,
    };
    if empty.rate() != 0 || empty.green() || empty.gate().is_ok() {
        return false;
    }
    if empty.render().is_empty() {
        return false;
    }
    // 四档矩阵的红档：6 个红档全在复刻槽所属的回退档里。
    let m = Matrix::of();
    if let Some(f) = m.row(Tier::Fallback) {
        if f.red() != REJECT || !f.reds_are_replicas() {
            return false;
        }
    } else {
        return false;
    }
    if m.perfect_rows() != 3 {
        return false;
    }
    true
}

// __CONT__
/// 文案路：语料逐槽的 `message()` 与告警位自洽。
fn message_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let o = define(&s);
        let m = message(&s, &o);
        // 唯一允许无声的现场：脚本重跑成功（rc=0）。其余诊断一律非空。
        if m.is_empty() && !(o.note == Note::Script && o.code == EXIT_OK) {
            return false;
        }
        // 带壳诊断的行首必须是 `程序名: `；本卡自研审计文案不带这个前缀。
        if !m.is_empty() {
            let mut head = String::from(prog_name(s.shell));
            head.push_str(": ");
            let audit = o.note == Note::CwdBlocked
                || o.note == Note::WalkCap
                || o.note == Note::HashStale
                || o.note == Note::HookOk
                || o.note == Note::HookFail
                || o.note == Note::Script;
            if !audit && !m.starts_with(head.as_str()) {
                return false;
            }
        }
        // 告警位只许落在告警族上；其余族必须无声。
        let w = warning(&s, &o);
        match o.note {
            Note::HookOk | Note::HookFail | Note::HashStale | Note::CwdBlocked | Note::WalkCap => {
                if w.is_empty() {
                    return false;
                }
            }
            Note::NotFound => {}
            _ => {
                if !w.is_empty() {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

/// 释义路：语料逐槽的 `detail()` 与 `message()` 对得上。
fn detail_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let o = define(&s);
        let c = Case { shape: s, claim: claim_of(&o) };
        let d = detail(&c);
        // 释义以 shell 名开头、换行收尾，且逐字带上结论落点名。
        if !d.starts_with(s.shell.text()) || !d.ends_with('\n') {
            return false;
        }
        let mut tail = String::from(" note=");
        tail.push_str(o.note.name());
        if !d.contains(tail.as_str()) {
            return false;
        }
        // msg 段只在诊断非空时出现。
        let m = message(&s, &o);
        if d.contains(" msg=\"") != !m.is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 次序排、每个键的值与常量表同号。
///
/// 这一条与 `run().render() == RENDER_OK` 是两件事：后者说"算出来的等于钉住的"，
/// 前者说"钉住的那一份自己就没有抄错数"。只查后者时，钉版与实现同时被改坏的
/// 情形（比如把 `fallback` 与 `denied` 两列一起对调）会蒙混过关。
fn render_ok_shape_ok() -> bool {
    if RENDER_OK.matches('\n').count() != 1 {
        return false;
    }
    if !RENDER_OK.ends_with("ppm\n") {
        return false;
    }
    if !RENDER_OK.starts_with("path-notfound ") {
        return false;
    }
    let mut it = RENDER_OK.split(' ');
    let head = match it.next() {
        Some(x) => x,
        None => return false,
    };
    if head != "path-notfound" {
        return false;
    }
    let keys = [
        "corpus=",
        "accept=",
        "reject=",
        "hit=",
        "direct=",
        "denied=",
        "fallback=",
        "match=",
        "rate=",
    ];
    let vals = [
        CORPUS as u32,
        ACCEPT as u32,
        REJECT as u32,
        *match VALS.get(3) {
            Some(x) => x,
            None => return false,
        },
        *match VALS.get(4) {
            Some(x) => x,
            None => return false,
        },
        *match VALS.get(5) {
            Some(x) => x,
            None => return false,
        },
        *match VALS.get(6) {
            Some(x) => x,
            None => return false,
        },
        CORPUS as u32,
        1000u32,
    ];
    let mut n = 0usize;
    while n < keys.len() {
        let token = match it.next() {
            Some(x) => x,
            None => return false,
        };
        let k = match keys.get(n) {
            Some(x) => *x,
            None => return false,
        };
        let tail = match field_value(token, k) {
            Some(x) => x,
            None => return false,
        };
        let num = if n + 1 == keys.len() {
            match tail.strip_suffix("ppm\n") {
                Some(x) => x,
                None => return false,
            }
        } else {
            tail
        };
        let got = match dec_of(num) {
            Some(x) => x,
            None => return false,
        };
        let want = match vals.get(n) {
            Some(x) => *x,
            None => return false,
        };
        if got != want {
            return false;
        }
        n += 1;
    }
    if it.next().is_some() {
        return false;
    }
    true
}

// __CONT__





