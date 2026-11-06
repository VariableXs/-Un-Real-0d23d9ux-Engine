//! UNX-F11664 notfound 退出码 127 判据
//!
//! shell 每跑一条命令都要给调用者留一个数：`$?`。POSIX 把这三个数留给了
//! shell 自己，不允许程序占用——**0 是跑通，126 是"找到了但没法执行"，
//! 127 是"根本找不到"**；再往上 128+signum 是"被信号打死"。busybox ash、
//! GNU bash、dash 在三条不同的代码路径上各自实现了同一套约定，于是这条
//! 约定就成了跨 shell 唯一可以硬碰硬对表的数字。本卡判的就是**这个数**，
//! 以及更要命的一件事：**这个数是谁给的**。
//!
//! 数是会撞车的。`/usr/bin/false` 退 1，不撞；但如果某个程序自己
//! `exit(127)`（Debian 的 `invoke-rc.d`、若干 systemd 单元脚本、乃至
//! 另一层 shell 把上层的 127 透传下来），`$?` 上看到的字节和"命令找不到"
//! **一模一样**。所以单看数字无法归因，必须三通道并读：
//!
//! 1. **退出码通道**——`$?` 那个数（本卡的 `Outcome::value`）。
//! 2. **stderr 文案通道**——有没有出现"找不到"这一族的行（本卡只认它的
//!    **族属**：静默 / notfound 行 / 权限行 / 目录行 / 子进程自己的字节；
//!    **行怎么逐字拼出来是 UNX-F11665 的判据，本卡一律不碰字节**）。
//! 3. **内部 errno 通道**——`execve` 到底回了什么（本卡的 `Outcome::code`：
//!    ENOENT / EACCES / EISDIR / ENOEXEC / ELOOP / 零）。
//!
//! 三通道一致，才是卡面写的"127 档判据绿"。任意两通道自洽而第三通道背离，
//! 本卡判死并把这一档命名为 `Note::ChanSplit`：例如 `$?`=127 却一个字节都
//! 没往 stderr 写（静默失败的 shell），或者 stderr 老老实实打了 `not found`
//! 而 `$?` 是 1（把错误行当装饰打印的 shell），或者 `execve` 回了 EACCES
//! 却报 127（把"不许执行"谎报成"不存在"——这条最阴，因为它同时污染了
//! 安全语义：调用者会以为路径里没有这个文件）。
//!
//! # 闸门次序
//!
//! `word → builtin → hash → search → errno → spawn → rc → commit`，八段
//! 单调，后段不得在前段短路后被"事后补记"。次序不可倒的理由：
//!
//! 1. `word` 最先——空命令字（`sh -c ''`、`foo;` 之间的空段、只有赋值前缀
//!    的 `A=1`）在 POSIX 里是**合法的空命令**，跑通、退 0，根本不该进查找。
//!    把它放进查找段就会得到"空字符串在 PATH 里找不到 → 127"，这是手写
//!    shell 的经典错误，也是本卡闸一存在的唯一理由。
//! 2. `builtin` 次之——内建表**先于** PATH。`cd`/`echo`/`true`/`:` 命中
//!    即定局，PATH 一个目录都不许扫。这一段的判据是硬事实：命中内建时
//!    `path_consulted` 必须为假；一旦为真，说明这个 shell 先扫了盘再问
//!    内建表，那么一个被裁掉的 `/bin/echo` 就会把 `echo` 打成 127。
//! 3. `hash` 第三——shell 会把找过的绝对路径记进哈希表（bash 的 `hash`）。
//!    记中但文件已不在（卸载、`rm`、升级换 inode）时，**必须回到 PATH
//!    重找一次**；直接拿陈旧路径的 ENOENT 判死，就漏掉了"第二条 PATH
//!    目录里还有同名可执行文件"这种真实构建。本卡把这一维做成 `Lookup::HashStale`。
//! 4. `search` 第四——PATH 逐目录扫。命中与否、扫了几个目录、词里带不带
//!    `/`（带 `/` 就完全不走 PATH），全在这段定。目录候选点在本段按 shell
//!    自己的策略分流：`DirPolicy::Skipped`（busybox ash 走 `stat` 后跳过
//!    非普通文件，于是 PATH 尽头就是没找到 → 127）与 `DirPolicy::Candidate`
//!    （GNU bash 把目录也送进 `execve`，于是拿到 EISDIR → 126）。
//!    **同一个现场、两个合法数字**，分流的唯一依据就是这条策略维。
//! 5. `errno` 第五——把失败原因归档成数：ENOENT→127、EACCES→126、
//!    EISDIR→126、ELOOP→126、ENOEXEC→126（除非这层 shell 选择按脚本
//!    重跑，见 `spawn`）。这一段之后，数字就已经定死了。
//! 6. `spawn` 第六——真正 `fork`/`execve` 成功。段一走过，shell 的
//!    "判词位"当场作废：**此后 `$?` 只能来自子进程**，本卡把 `default`
//!    强制置假并把档落在 `Foreign`。这就是卡面要求的"跑过但非零，绝不允许
//!    被记成 127"的物理实现。
//! 7. `rc` 第七——子进程的返回码原样入账（含 128+signum 的信号档）。
//!    子进程自己退 127 是本卡最难的活：数字相同、`errno` 通道为零、文案
//!    通道是子进程写的字节，三通道一起指向"这不是 shell 的判词"。
//! 8. `commit` 收尾——三通道都记齐且互相不自相矛盾，才算观测完整。
//!
//! # 关键差异（本卡深度所在）
//!
//! - **本卡 vs UNX-F11665**：同一起 notfound 事件，本卡只判**数**，
//!   F11665 只判**字**。本卡的 stderr 通道被抽象成 `TextChan` 五档族属，
//!   连一个字节都不比对；F11665 反过来把 `ash: foo: not found` 与
//!   `bash: line 1: foo: command not found` 的前缀、引号、行号、尾句点逐字
//!   钉死，却不判这个数该是 126 还是 127（它只按 F11664 的表**引用**结论做
//!   联签）。两卡互为对方的第二台机器：本卡绿而 F11665 红，说明数对字歪；
//!   反之说明字对数歪。
//! - **本卡 vs UNX-F11661**：F11661 判 busybox **applet 注册表**的答复面，
//!   键是编译期表名，失败原因是"表里没有"，现场只有一条 notfound 行加一个
//!   码档；本卡判的是**运行期查找**——内建表、命令哈希表、PATH 逐目录、
//!   `execve` 的 errno——这条链上任何一个环节都能产生 126 或 127，而 busybox
//!   的表里根本没有 PATH、也没有 EACCES/EISDIR 这一族。同一句
//!   `busybox: 'foo' applet not found` 在 F11661 是现场本体，在本卡只是
//!   `TextChan` 的一个族属取值，甚至连前缀都不看。
//! - **本卡 vs UNX-F11662/F11663**：那两张判 `argv[0]` 伪名降级块与裁剪件
//!   等文案，都发生在 busybox **进 shell 之前**；本卡全部现场都已经到了
//!   "某层 shell 要执行一个命令字"这一步，126/127 的归因链里没有 applet 表。
//! - **本卡最硬的一条界线**：127 是"不存在"，126 是"存在但不许执行"。
//!   任何把 EACCES 归档成 127 的实现都在谎报文件系统事实，本卡直接判死，
//!   且判死在 `errno` 段——不给它到 `commit` 的机会。
//!
//! 判序：状态 → 阶段 → errno 通道 → 退出码通道 → 注解 → 判词位。
//!
//! 语料 14 槽：8 槽申报正确（跑通 4、127 档 2、126 档 1、外数 1），6 槽
//! 各挂一处错误申报（状态/阶段/码/值/注解/判词位）。账本 14 格 8 成 6 败，
//! 冻结后重放同指纹。
//!
//! 本模块是 `no_std` 纯功能判据：不 fork、不 execve、不读真实 PATH，把上面
//! 八闸与三通道建成一张可复核的事实表，再由十三项聚合判据（EX01…EX13）
//! 一次全跑。所有取值走 `match` / `.get()` / `checked_*` / `saturating_*`，
//! 生产码零 `unwrap`、零 `unsafe`、零 `std`。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11664，VX-1029-5 批分配值，不得自改）。
pub const SEG_BASE: u32 = 0x6d1a0;

/// 语料槽数。
pub const CORPUS: usize = 14usize;

/// 账本格数（与语料同尺：一格一槽）。
pub const SAMPLES: usize = 14usize;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8usize;

/// 申报错误的槽数。
pub const REJECT: usize = 6usize;

/// 统计档位数。
pub const TIER_COUNT: usize = 4usize;

/// 闸段数（八闸）。
pub const STAGE_COUNT: usize = 8usize;

/// 账本容量下限（败格数，显式声明）。
pub const CAP: usize = 6;

/// PATH 里可存放的目录项上限（模型值，用于越界体检）。
pub const PATH_DIR_MAX: u8 = 32;

/// 命令字最长值（`ARG_MAX` 的一小档，本卡只用来挡明显不合法的形状）。
pub const WORD_MAX: usize = 127;

// ---------------------------------------------------------------------------
// POSIX 状态档——shell 自己占用的那三个数
// ---------------------------------------------------------------------------

/// 跑通。程序可以用 0，但"跑通了"这件事只有 shell 的 0 档才算数。
pub const EXIT_OK: u16 = 0;

/// 程序可自由占用的最大档：1..=125 一律是"子进程给的数"。
pub const EXIT_PROGRAM_MAX: u16 = 125;

/// 126——找到了，但 `execve` 没让执行（权限、目录、格式、软链接成环）。
pub const EXIT_NOT_EXEC: u16 = 126;

/// 127——根本没找到。本卡的主判档，POSIX 留给 shell 的"command not found"。
pub const EXIT_NOT_FOUND: u16 = 127;

/// 128——被信号打死的偏移基：`128 + signum`。
pub const EXIT_SIGNAL_BASE: u16 = 128;

/// shell 用法/语法错档（POSIX 留给 shell 自身的 2）。本卡用它承载
/// "三通道互相矛盾、观测作废"的现场：数不可信时绝不冒充 127。
pub const EXIT_USAGE: u16 = 2;

/// SIGKILL 的号（128+9=137，CI 容器里最常见的一个"看着像 127"的数）。
pub const SIG_KILL: u8 = 9;

/// SIGSEGV 的号（128+11=139）。
pub const SIG_SEGV: u8 = 11;

/// SIGTERM 的号（128+15=143）。
pub const SIG_TERM: u8 = 15;

/// 最大可信信号号（Linux 现代内核 64；超过即观测作废）。
pub const SIG_MAX: u8 = 64;

/// 子进程自行 `exit(127)` 时 shell 看到的数——与本卡主判档同字节，归因相反。
pub const EXIT_CHILD_MIMIC: u16 = 127;

// ---------------------------------------------------------------------------
// errno 通道——`execve`/校验回的那个数
// ---------------------------------------------------------------------------

/// 零：`execve` 成功，或者压根没到执行那步（内建/空命令）。
pub const CH_ENONE: i32 = 0;

/// 2 ENOENT——文件不存在。notfound 的正牌 errno。
pub const CH_ENOENT: i32 = 2;

/// 13 EACCES——存在，但没执行权限。126 的正牌 errno。
pub const CH_EACCES: i32 = 13;

/// 21 EISDIR——存在且是目录。带 `/` 的词命中目录时的 errno。
pub const CH_EISDIR: i32 = 21;

/// 8 ENOEXEC——存在、是普通文件、但没有可执行格式（无魔数也无 shebang）。
pub const CH_ENOEXEC: i32 = 8;

/// 40 ELOOP——软链接成环。归 126，绝不归 127：文件确实存在。
pub const CH_ELOOP: i32 = 40;

/// 22 EINVAL——本卡用于"三通道互相矛盾"的观测作废档。真机不会回它，
/// 它是判据自己造出来的哨兵，因此也绝不能出现在 127 档的合法组合里。
pub const CH_EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 13 个错误码（SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 闸段申报不符：停在的闸与判据复算的闸不是同一道。
pub const EX_ESTAGE: u32 = SEG_BASE + 0x01u32;

/// errno 通道申报不符：内部失败原因归错族。
pub const EX_ECODE: u32 = SEG_BASE + 0x02u32;

/// 退出码通道申报不符：`$?` 那个数报错档。
pub const EX_EVALUE: u32 = SEG_BASE + 0x03u32;

/// 注解申报不符：判词族属（127 档 / 126 档 / 外数 / 跑通）认错。
pub const EX_ENOTE: u32 = SEG_BASE + 0x04u32;

/// 判词位申报不符：这个 127 到底是不是 shell 给的说法被改。
pub const EX_EDEFAULT: u32 = SEG_BASE + 0x05u32;

/// 三通道不一致：数/文案/errno 三者互相背离，观测作废。
pub const EX_ECHANNEL: u32 = SEG_BASE + 0x06u32;

/// 成败互报：申报绿而复算红（或反之）。
pub const EX_ESTATE: u32 = SEG_BASE + 0x07u32;

/// 计数不符：语料规模、成败分布、档位覆盖对不上。
pub const EX_ECOUNT: u32 = SEG_BASE + 0x08u32;

/// 槽位非法：往不存在的槽写结论，或申报越界。
pub const EX_ESLOT: u32 = SEG_BASE + 0x09u32;

/// 指纹不符：重放重建的账本与冻结指纹不一致。
pub const EX_ESEAL: u32 = SEG_BASE + 0x0au32;

/// 无观测：语料没跑满，或某槽的期望缺失。
pub const EX_ENOBS: u32 = SEG_BASE + 0x0bu32;

/// 矩阵不平：四档行数与语料规模不等，或有空档。
pub const EX_EMATRIX: u32 = SEG_BASE + 0x0cu32;

/// 账本不平：逐格对账失败（成格值、败格码、槽位连续性）。
pub const EX_EBAL: u32 = SEG_BASE + 0x0du32;

/// 十三码表——按 `SEG_BASE + 0x01..0x0d` 次序，供逐码点名与撞码自检。
pub const CODE_TABLE: [u32; 13usize] = [
    EX_ESTAGE,
    EX_ECODE,
    EX_EVALUE,
    EX_ENOTE,
    EX_EDEFAULT,
    EX_ECHANNEL,
    EX_ESTATE,
    EX_ECOUNT,
    EX_ESLOT,
    EX_ESEAL,
    EX_ENOBS,
    EX_EMATRIX,
    EX_EBAL,
];

// ---------------------------------------------------------------------------
// 操作码（账本格的来源）
// ---------------------------------------------------------------------------

/// 命令字体检。
pub const OP_WORD: u8 = 0x01;
/// 内建表裁决。
pub const OP_BUILTIN: u8 = 0x02;
/// 命令哈希表。
pub const OP_HASH: u8 = 0x03;
/// PATH 逐目录查找。
pub const OP_SEARCH: u8 = 0x04;
/// errno 归档成数。
pub const OP_ERRNO: u8 = 0x05;
/// 实际 fork/exec。
pub const OP_SPAWN: u8 = 0x06;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x07;
/// 重放对指纹。
pub const OP_REPLAY: u8 = 0x08;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（一次乘法一次折叠，非展开循环，保证跨构建可重放）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端序逐字节，逐位右移不用减法越界的写法）。
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

/// 折入一个字符串（逐字节 `get`，零下标取值）。
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

// ---------------------------------------------------------------------------
// 数字文本
// ---------------------------------------------------------------------------

/// 十进制文本（u32，不递归，余数从后往前落槽）。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        i -= 1;
        if let Some(slot) = buf.get_mut(i) {
            *slot = b'0' + ((n % 10) as u8);
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

/// 有符号十进制文本（errno 通道要打出负数形态，真机 errno 恒正但判据留位）。
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

/// u32 文本（对外可见，供释义与判据复用）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// bool 文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

// ---------------------------------------------------------------------------
// 字节串基元（no_std 下不依赖 Pattern，全部手写）
// ---------------------------------------------------------------------------

/// 两串是否逐字相等。显式写出来是为了让"逐字"在代码里可见。
pub fn same_text(a: &str, b: &str) -> bool {
    let x = a.as_bytes();
    let y = b.as_bytes();
    if x.len() != y.len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.len() {
        match (x.get(i), y.get(i)) {
            (Some(p), Some(q)) => {
                if p != q {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 串长度（字节数）。
pub fn text_len(s: &str) -> usize {
    s.as_bytes().len()
}

/// 串里是否含某个字节。
pub fn has_byte(s: &str, needle: u8) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == needle {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 从 `from` 起在 `hay` 里找 `needle`（手写窗口匹配，零切片）。
pub fn find_from(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || from > h.len() {
        return None;
    }
    let mut i = from;
    while i + n.len() <= h.len() {
        let mut k = 0usize;
        let mut hit = true;
        while k < n.len() {
            match (h.get(i + k), n.get(k)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        hit = false;
                    }
                }
                _ => hit = false,
            }
            if !hit {
                break;
            }
            k += 1;
        }
        if hit {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 是否以前缀开头。
pub fn starts_with(hay: &str, pre: &str) -> bool {
    find_from(hay, pre, 0) == Some(0)
}

/// 串里是否含 `/`，以及是否以 `/` 结尾。
///
/// 带 `/` 的命令字**完全不走 PATH**，这是闸四的分流点；尾斜杠
/// （`cat dir/`）会让 basename 变空，是另一类形状。
pub fn slash_shape(s: &str) -> (bool, bool) {
    let b = s.as_bytes();
    let mut any = false;
    let mut trailing = false;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'/' {
                any = true;
                trailing = i + 1 == b.len();
            }
        }
        i += 1;
    }
    (any, trailing)
}

/// 命令字是否"含斜杠"（POSIX 意义下的 pathname 形态）。
pub fn word_has_slash(s: &str) -> bool {
    slash_shape(s).0
}

/// 是否全是空白（空格/制表）。只由空白组成的命令字等价于空命令。
pub fn is_blank_word(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c != b' ' && *c != b'\t' {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 是否形如"只有赋值前缀"（`A=1`、`FOO=bar`）——POSIX 里这也是空命令，
/// 只做变量赋值、退 0，绝不进查找链。判据：含 `=`，且 `=` 之前的片段
/// 是合法变量名字符集（字母/下划线/数字，首字符非数字）。
pub fn is_assign_only(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let b = s.as_bytes();
    let eq = match find_from(s, "=", 0) {
        Some(i) => i,
        None => return false,
    };
    if eq == 0 {
        return false;
    }
    let mut i = 0usize;
    while i < eq {
        if let Some(c) = b.get(i) {
            let ok = (*c == b'_')
                || (*c >= b'a' && *c <= b'z')
                || (*c >= b'A' && *c <= b'Z')
                || (i > 0 && *c >= b'0' && *c <= b'9');
            if !ok {
                return false;
            }
        }
        i += 1;
    }
    // `=` 之后不得再出现空白或斜杠，否则就是"赋值 + 命令"的复合形态。
    let mut j = eq + 1;
    while j < b.len() {
        if let Some(c) = b.get(j) {
            if *c == b' ' || *c == b'/' || *c == b'\t' {
                return false;
            }
        }
        j += 1;
    }
    true
}

/// 是否含控制字符（< 0x20 或 0x7f）。命令字里的控制字符多半是引号没配对
/// 或 here-doc 串位，观测本身不可信。
pub fn has_control(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c < 0x20 || *c == 0x7f {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 是否全部由 `.` 组成（`.` 与 `..` 是合法路径片段，绝不是可执行文件名；
/// 带 `/` 时它们是 `SlashTarget`，不带 `/` 时它们是普通"找不到"）。
pub fn is_dots(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c != b'.' {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// PATH 串的目录项数（分隔符 `:` 计数 +1；空串算 0 项）。
///
/// 本卡用它把闸四的"扫了几个目录"和申报的 `dirs_scanned` 对表：
/// PATH 尽头必须是**扫满**，否则就是提前退出。
pub fn path_dir_count(path: &str) -> usize {
    if path.is_empty() {
        return 0;
    }
    let b = path.as_bytes();
    let mut n = 1usize;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b':' {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 取最后一个 `/` 之后的基名（shell 的程序名前缀来源）。
pub fn last_component(s: &'static str) -> &'static str {
    match s.rsplit_once('/') {
        Some((_, tail)) => tail,
        None => s,
    }
}

// ---------------------------------------------------------------------------
// 命令字形态
// ---------------------------------------------------------------------------

/// 命令字自身的形态档。
///
/// 闸一只看这个词长什么样，不看文件系统：空/空白/纯赋值是 POSIX 认可的
/// "空命令"，其余才谈得上查找。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordForm {
    /// 裸名（不含 `/`），要进 PATH 逐目录。
    Bare,
    /// 含 `/` 的词：POSIX 规定**完全不走 PATH**，直接把它当候选路径。
    Slashed,
    /// 空词（`sh -c ''`、连续两个 `;` 之间的空段）。
    Empty,
    /// 只有空白（`sh -c '   '`）。
    Blank,
    /// 只有赋值前缀（`A=1`）：做赋值、退 0、不查找。
    Assign,
    /// 纯点（`.` / `..`）：路径片段而非命令名。
    Dots,
    /// 含控制字符：观测不可信。
    Control,
    /// 超出本卡建模上限。
    Overlong,
}

impl WordForm {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            WordForm::Bare => "bare",
            WordForm::Slashed => "slashed",
            WordForm::Empty => "empty",
            WordForm::Blank => "blank",
            WordForm::Assign => "assign-only",
            WordForm::Dots => "dots",
            WordForm::Control => "control",
            WordForm::Overlong => "overlong",
        }
    }

    /// 是否是 POSIX 的"空命令"——退 0，绝不进查找链。
    pub fn is_no_command(&self) -> bool {
        match self {
            WordForm::Empty | WordForm::Blank | WordForm::Assign => true,
            _ => false,
        }
    }

    /// 是否是观测不可用的畸形词（判死在闸一）。
    pub fn is_void(&self) -> bool {
        match self {
            WordForm::Control | WordForm::Overlong => true,
            _ => false,
        }
    }

    /// 该形态是否允许走 PATH：只有裸名走。含斜杠的词、纯点都不走。
    pub fn consults_path(&self) -> bool {
        match self {
            WordForm::Bare => true,
            _ => false,
        }
    }
}

/// 命令字 → 形态档。
///
/// 判序刻意是：空 → 空白 → 控制字符 → 纯赋值 → 纯点 → 含斜杠 → 超长 → 裸名。
/// 控制字符排在含斜杠之前，是因为 `foo\n` 这类尾巴先破坏观测本身的可信度；
/// 纯点排在含斜杠之前，是因为 `.` 不带斜杠却仍是"路径片段不是命令名"。
pub fn word_form(w: &str) -> WordForm {
    if w.is_empty() {
        return WordForm::Empty;
    }
    if is_blank_word(w) {
        return WordForm::Blank;
    }
    if has_control(w) {
        return WordForm::Control;
    }
    if is_assign_only(w) {
        return WordForm::Assign;
    }
    if is_dots(w) {
        return WordForm::Dots;
    }
    if word_has_slash(w) {
        return WordForm::Slashed;
    }
    if text_len(w) > WORD_MAX {
        return WordForm::Overlong;
    }
    WordForm::Bare
}

// ---------------------------------------------------------------------------
// 查找答复
// ---------------------------------------------------------------------------

/// 查找链的答复档——本卡的"这个词到底是怎么没找到的"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// 内建表命中：PATH 一个目录都不许扫。
    Builtin,
    /// 命令哈希表命中且路径仍有效：跳过 PATH 直接执行。
    HashHit,
    /// 命令哈希表命中但路径已陈旧：必须回 PATH 重找（`rehash` 维决定）。
    HashStale,
    /// PATH 逐目录命中一个候选。
    PathHit,
    /// PATH 扫尽无候选——notfound 的正牌答复。
    PathMiss,
    /// PATH 里唯一候选是目录：数字由 `DirPolicy` 分流。
    DirOnPath,
    /// 词含 `/`，候选就是词本身（可能是缺失/目录/无权限/坏格式）。
    SlashTarget,
}

impl Lookup {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Lookup::Builtin => "builtin",
            Lookup::HashHit => "hash-hit",
            Lookup::HashStale => "hash-stale",
            Lookup::PathHit => "path-hit",
            Lookup::PathMiss => "path-miss",
            Lookup::DirOnPath => "dir-on-path",
            Lookup::SlashTarget => "slash-target",
        }
    }

    /// 是否"找到了"（内建/哈希有效/PATH 命中/斜杠目标存在）。
    ///
    /// 注意 `SlashTarget` 归在这里：词含 `/` 时查找这一层是"目标即候选"，
    /// 存在与否由 errno 通道说话，不由这一层说话。
    pub fn found(&self) -> bool {
        match self {
            Lookup::Builtin
            | Lookup::HashHit
            | Lookup::PathHit
            | Lookup::DirOnPath
            | Lookup::SlashTarget => true,
            _ => false,
        }
    }

    /// 是否"根本没找到"档（答复直接给 127）。
    pub fn is_miss(&self) -> bool {
        match self {
            Lookup::PathMiss => true,
            _ => false,
        }
    }

    /// 这一层是否会产生"扫 PATH"的动作。
    pub fn walks_path(&self) -> bool {
        match self {
            Lookup::PathHit | Lookup::PathMiss | Lookup::DirOnPath => true,
            _ => false,
        }
    }

    /// 归属的闸段（答复在哪一道闸上定型）。
    pub fn gate(&self) -> Stage {
        match self {
            Lookup::Builtin => Stage::Builtin,
            Lookup::HashHit | Lookup::HashStale => Stage::Hash,
            Lookup::PathHit | Lookup::PathMiss | Lookup::DirOnPath => Stage::Search,
            Lookup::SlashTarget => Stage::Search,
        }
    }
}

/// 目录候选策略——同一个"PATH 里那个名字是个目录"现场，两家 shell 给出
/// 两个都合法的数字，分流点只能显式建模。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirPolicy {
    /// busybox ash 一路：`stat` 到非普通文件就跳过，于是 PATH 扫尽无候选
    /// → ENOENT → **127**。
    Skipped,
    /// GNU bash 一路：把目录也塞进 `execve`，于是拿到 EISDIR → **126**。
    Candidate,
}

impl DirPolicy {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            DirPolicy::Skipped => "skip-dir",
            DirPolicy::Candidate => "exec-dir",
        }
    }

    /// 该策略下目录候选给出的数字。
    pub fn exit_of(&self) -> u16 {
        match self {
            DirPolicy::Skipped => EXIT_NOT_FOUND,
            DirPolicy::Candidate => EXIT_NOT_EXEC,
        }
    }

    /// 该策略下目录候选给出的 errno 通道值。
    pub fn errno_of(&self) -> i32 {
        match self {
            DirPolicy::Skipped => CH_ENOENT,
            DirPolicy::Candidate => CH_EISDIR,
        }
    }
}

// ---------------------------------------------------------------------------
// errno 通道
// ---------------------------------------------------------------------------

/// 内部 errno 通道的档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    /// 零——`execve` 成功，或压根没到执行那步。
    ENone,
    /// ENOENT——不存在。唯一合法地通向 127 的 errno。
    Enoent,
    /// EACCES——存在但无执行权限。通向 126。
    Eacces,
    /// EISDIR——存在且是目录（带 `/` 的词或 Candidate 策略的目录候选）。
    Eisdir,
    /// ENOEXEC——普通文件但无可执行格式。通向 126（除非按脚本重跑）。
    Enoexec,
    /// ELOOP——软链接成环。通向 126：文件确实存在过。
    Eloop,
    /// EINVAL——本卡哨兵：三通道互相矛盾、观测作废。真机不回它。
    Einval,
}

impl Errno {
    /// 数值。
    pub fn value(&self) -> i32 {
        match self {
            Errno::ENone => CH_ENONE,
            Errno::Enoent => CH_ENOENT,
            Errno::Eacces => CH_EACCES,
            Errno::Eisdir => CH_EISDIR,
            Errno::Enoexec => CH_ENOEXEC,
            Errno::Eloop => CH_ELOOP,
            Errno::Einval => CH_EINVAL,
        }
    }

    /// 由数值回档（未知数值一律落哨兵档，绝不冒充 ENOENT）。
    pub fn of(v: i32) -> Errno {
        match v {
            CH_ENONE => Errno::ENone,
            CH_ENOENT => Errno::Enoent,
            CH_EACCES => Errno::Eacces,
            CH_EISDIR => Errno::Eisdir,
            CH_ENOEXEC => Errno::Enoexec,
            CH_ELOOP => Errno::Eloop,
            _ => Errno::Einval,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Errno::ENone => "0",
            Errno::Enoent => "ENOENT",
            Errno::Eacces => "EACCES",
            Errno::Eisdir => "EISDIR",
            Errno::Enoexec => "ENOEXEC",
            Errno::Eloop => "ELOOP",
            Errno::Einval => "EINVAL",
        }
    }

    /// 是否"存在"这一族——除 ENOENT 与哨兵以外，文件都在那儿。
    ///
    /// 这一维是本卡最硬的分界：**存在**却执行不了的，绝不能报 127。
    pub fn implies_exists(&self) -> bool {
        match self {
            Errno::Eacces | Errno::Eisdir | Errno::Enoexec | Errno::Eloop => true,
            _ => false,
        }
    }

    /// 是否通向 127。
    pub fn is_not_found(&self) -> bool {
        match self {
            Errno::Enoent => true,
            _ => false,
        }
    }

    /// 是否通向 126。
    pub fn is_not_exec(&self) -> bool {
        match self {
            Errno::Eacces | Errno::Eisdir | Errno::Enoexec | Errno::Eloop => true,
            _ => false,
        }
    }

    /// errno → 数字的映射（闸五的表）。
    ///
    /// 这张表就是"127 档判据"的数轴本身：唯一通向 127 的是 ENOENT，
    /// 其余"存在但执行不了"的一律 126，零与哨兵不在这条路上。
    pub fn exit_of(&self) -> u16 {
        match self {
            Errno::Enoent => EXIT_NOT_FOUND,
            Errno::Eacces | Errno::Eisdir | Errno::Enoexec | Errno::Eloop => EXIT_NOT_EXEC,
            Errno::ENone | Errno::Einval => EXIT_USAGE,
        }
    }
}

// ---------------------------------------------------------------------------
// stderr 文案通道（只判族属，逐字归 F11665）
// ---------------------------------------------------------------------------

/// stderr 通道观察到的**族属**。
///
/// 本卡到这里为止：知道"是 notfound 那一族"就够了，`ash: foo: not found`
/// 还是 `bash: line 1: foo: command not found` 是 F11665 的判据。
/// 这条截止线就是两卡的物理边界。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextChan {
    /// 静默：stderr 一个字节都没有。
    Silent,
    /// notfound 那一族的行。
    NotFoundLine,
    /// 权限那一族的行（`Permission denied` / `can't execute`）。
    PermLine,
    /// 目录那一族的行（`Is a directory`）。
    IsDirLine,
    /// 可执行格式那一族的行（`Exec format error`）。
    ExecFmtLine,
    /// 行是子进程自己写的字节，不是 shell 的判词。
    ChildBytes,
}

impl TextChan {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            TextChan::Silent => "silent",
            TextChan::NotFoundLine => "notfound",
            TextChan::PermLine => "permission",
            TextChan::IsDirLine => "isdir",
            TextChan::ExecFmtLine => "execfmt",
            TextChan::ChildBytes => "child-own",
        }
    }

    /// 是否 shell 自己下的判词条（子进程字节与静默都不是）。
    pub fn is_shell_verdict(&self) -> bool {
        match self {
            TextChan::NotFoundLine | TextChan::PermLine | TextChan::IsDirLine | TextChan::ExecFmtLine => {
                true
            }
            _ => false,
        }
    }

    /// 该族属对应的 errno 通道期望值（三通道一致性的文案侧投影）。
    pub fn expected_errno(&self) -> i32 {
        match self {
            TextChan::NotFoundLine => CH_ENOENT,
            TextChan::PermLine => CH_EACCES,
            TextChan::IsDirLine => CH_EISDIR,
            TextChan::ExecFmtLine => CH_ENOEXEC,
            TextChan::Silent | TextChan::ChildBytes => CH_ENONE,
        }
    }

    /// 该族属对应的数字期望（三通道一致性的数侧投影）。
    pub fn expected_exit(&self) -> u16 {
        match self {
            TextChan::NotFoundLine => EXIT_NOT_FOUND,
            TextChan::PermLine | TextChan::IsDirLine | TextChan::ExecFmtLine => EXIT_NOT_EXEC,
            TextChan::Silent | TextChan::ChildBytes => EXIT_OK,
        }
    }
}

/// 数字的归因档——本卡的灵魂维：这个数是谁给的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// shell 说"找不到"。127 且三通道同真的唯一合法来源。
    ShellNotFound,
    /// shell 说"找到了但执行不了"。通向 126。
    ShellNotExec,
    /// 子进程的返回码（含被信号打死与 126/127 冒充档）。
    ChildStatus,
    /// 内建命令在当前进程里返回的数。
    Builtin,
    /// POSIX 空命令（赋值/空白）→ 0。
    Empty,
    /// 观测作废：三通道互相矛盾。
    Void,
}

impl Origin {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Origin::ShellNotFound => "shell-notfound",
            Origin::ShellNotExec => "shell-notexec",
            Origin::ChildStatus => "child-status",
            Origin::Builtin => "builtin",
            Origin::Empty => "empty-command",
            Origin::Void => "void",
        }
    }

    /// 是否 shell 自己给的数（只有这两档 shell 才是数字的作者）。
    pub fn is_shell(&self) -> bool {
        match self {
            Origin::ShellNotFound | Origin::ShellNotExec => true,
            _ => false,
        }
    }

    /// 是否 shell 的 notfound 判词——`default` 位的真身。
    pub fn is_notfound_verdict(&self) -> bool {
        match self {
            Origin::ShellNotFound => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 闸段
// ---------------------------------------------------------------------------

/// 八闸档——数字在哪一道闸上定型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 闸一：命令字体检。
    Word,
    /// 闸二：内建表裁决。
    Builtin,
    /// 闸三：命令哈希表。
    Hash,
    /// 闸四：PATH 逐目录查找。
    Search,
    /// 闸五：errno 归档成数。
    Errno,
    /// 闸六：实际 fork/exec 的守卫段（跑过还想报 127 就在这里判死）。
    Spawn,
    /// 闸七：子进程返回码归因（126/127 冒充档在这里点名）。
    Rc,
    /// 闸八：观测收尾，数字来自子进程且无任何 shell 判词介入。
    Commit,
}

impl Stage {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Word => "word",
            Stage::Builtin => "builtin",
            Stage::Hash => "hash",
            Stage::Search => "search",
            Stage::Errno => "errno",
            Stage::Spawn => "spawn",
            Stage::Rc => "rc",
            Stage::Commit => "commit",
        }
    }

    /// 次序（0 基，单调不倒退的尺子）。
    pub fn order(&self) -> usize {
        match self {
            Stage::Word => 0,
            Stage::Builtin => 1,
            Stage::Hash => 2,
            Stage::Search => 3,
            Stage::Errno => 4,
            Stage::Spawn => 5,
            Stage::Rc => 6,
            Stage::Commit => 7,
        }
    }

    /// 由次序回档；越界落 `Commit`（闸门自检里越界另有码，这里只做渲染兜底）。
    pub fn from_order(i: usize) -> Stage {
        match i {
            0 => Stage::Word,
            1 => Stage::Builtin,
            2 => Stage::Hash,
            3 => Stage::Search,
            4 => Stage::Errno,
            5 => Stage::Spawn,
            6 => Stage::Rc,
            _ => Stage::Commit,
        }
    }

    /// 是否已经越过"跑过"这条线（`Spawn` 及之后）。
    ///
    /// 越过之后，shell 的 notfound 判词位必须永久熄灭——本卡最重要的一条单向门。
    pub fn past_spawn(&self) -> bool {
        match self {
            Stage::Spawn | Stage::Rc | Stage::Commit => true,
            _ => false,
        }
    }

    /// 该段给出的数是否为 shell 判词。
    pub fn shell_verdict(&self) -> bool {
        match self {
            Stage::Hash | Stage::Search | Stage::Errno => true,
            _ => false,
        }
    }

    /// 闸段总数。
    pub const fn count() -> usize {
        STAGE_COUNT
    }
}

// ---------------------------------------------------------------------------
// 注解（判词族属）
// ---------------------------------------------------------------------------

/// 判词的族属——数字背后"这件事是什么"的一句话说清。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// POSIX 空命令：赋值或空白，退 0，不查找。
    EmptyWord,
    /// 内建命令在当前进程里返回。
    BuiltinOk,
    /// 外部命令跑完，返回码原样入账。
    ChildRan,
    /// 外部命令**自己**退 127：数字撞了 shell 预留档，归因是子进程。
    ChildMimic,
    /// 外部命令**自己**退 126：同上，撞的是 126。
    ChildNotExec,
    /// 子进程被信号打死，数字是 128+signum。
    Signaled,
    /// ENOEXEC 后这层 shell 把文件当脚本重跑，数字来自那层子 shell。
    FallbackRan,
    /// 127 档判词：找不到，三通道同真。本卡主判档。
    NotFound127,
    /// 126 档判词（权限/软链接环/格式）：存在但执行不了。
    NotExec126,
    /// 126 档判词的目录支：候选是目录。
    IsDir126,
    /// 观测作废：三通道互相矛盾，数字不可信。
    ChanSplit,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::EmptyWord => "empty-command",
            Note::BuiltinOk => "builtin-ran",
            Note::ChildRan => "child-ran",
            Note::ChildMimic => "child-mimic-127",
            Note::ChildNotExec => "child-mimic-126",
            Note::Signaled => "killed-by-signal",
            Note::FallbackRan => "enoexec-fallback",
            Note::NotFound127 => "verdict-127",
            Note::NotExec126 => "verdict-126",
            Note::IsDir126 => "verdict-126-isdir",
            Note::ChanSplit => "channel-split",
        }
    }

    /// 是否"命令找到了、事情发生了"——本卡 `ok()` 的注解侧条件。
    ///
    /// 注意 `ChildMimic` 也在这里：子进程退 127 是**跑通了**的一件事，
    /// 尽管数字长得像判词。这一条就是本卡与"只看数"的实现的根本分歧。
    pub fn ran(&self) -> bool {
        match self {
            Note::EmptyWord
            | Note::BuiltinOk
            | Note::ChildRan
            | Note::ChildMimic
            | Note::ChildNotExec
            | Note::Signaled
            | Note::FallbackRan => true,
            _ => false,
        }
    }

    /// 是否 shell 自己的 127 判词族。
    pub fn is_notfound(&self) -> bool {
        match self {
            Note::NotFound127 => true,
            _ => false,
        }
    }

    /// 是否 shell 自己的 126 判词族。
    pub fn is_notexec(&self) -> bool {
        match self {
            Note::NotExec126 | Note::IsDir126 => true,
            _ => false,
        }
    }

    /// 是否观测作废档。
    pub fn is_void(&self) -> bool {
        match self {
            Note::ChanSplit => true,
            _ => false,
        }
    }

    /// 数字归因档。
    pub fn origin(&self) -> Origin {
        match self {
            Note::NotFound127 => Origin::ShellNotFound,
            Note::NotExec126 | Note::IsDir126 => Origin::ShellNotExec,
            Note::ChildRan | Note::ChildMimic | Note::ChildNotExec | Note::Signaled | Note::FallbackRan => {
                Origin::ChildStatus
            }
            Note::BuiltinOk => Origin::Builtin,
            Note::EmptyWord => Origin::Empty,
            Note::ChanSplit => Origin::Void,
        }
    }

    /// 该注解合法的 errno 通道值（`None` 表示不约束）。
    pub fn errno_channel(&self) -> i32 {
        match self {
            Note::NotFound127 => CH_ENOENT,
            Note::NotExec126 => CH_EACCES,
            Note::IsDir126 => CH_EISDIR,
            Note::ChanSplit => CH_EINVAL,
            _ => CH_ENONE,
        }
    }

    /// 该注解合法的数字。
    pub fn exit_channel(&self) -> u16 {
        match self {
            Note::NotFound127 => EXIT_NOT_FOUND,
            Note::NotExec126 | Note::IsDir126 => EXIT_NOT_EXEC,
            Note::ChanSplit => EXIT_USAGE,
            _ => EXIT_OK,
        }
    }
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 四档统计面——全部由**实际结果**投影，不看申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 跑通：数字 0 且三通道说"事情发生了"。
    Run,
    /// 127 档：shell 的 notfound 判词，三通道同真。本卡的正牌档。
    NotFound,
    /// 126 档：shell 的"存在但执行不了"判词。
    NotExec,
    /// 外数：既不是 shell 的 127 也不是 126——子进程自己的数、信号档、
    /// 内建返回、观测作废全在这一档。**子进程退 127 落在这一档而不是
    /// `NotFound`，就是本卡那条不可倒的序。**
    Foreign,
}

impl Tier {
    /// 由实际结果投影。
    ///
    /// 判序：判词位 → 126 判词族 → 跑通且数为零 → 外数。判词位排第一，
    /// 是因为它已经内含"值==127 且 errno==ENOENT 且文案是 notfound 族
    /// 且没跑过子进程"四重事实；反过来先比值就会把子进程冒充的 127 收进
    /// 正牌档——那正是本卡要判死的情形。
    pub fn of(o: &Outcome) -> Tier {
        if o.default {
            return Tier::NotFound;
        }
        if o.note.is_notexec() {
            return Tier::NotExec;
        }
        if o.ok() && o.value == EXIT_OK {
            return Tier::Run;
        }
        Tier::Foreign
    }

    /// 0 基档号。
    pub fn index(&self) -> usize {
        match self {
            Tier::Run => 0,
            Tier::NotFound => 1,
            Tier::NotExec => 2,
            Tier::Foreign => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Run => "run",
            Tier::NotFound => "notfound",
            Tier::NotExec => "notexec",
            Tier::Foreign => "foreign",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        match self {
            Tier::Run => true,
            _ => false,
        }
    }

    /// 是否 shell 判词面（127/126 两档）。
    pub fn shell_face(&self) -> bool {
        match self {
            Tier::NotFound | Tier::NotExec => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 申报缺陷
// ---------------------------------------------------------------------------

/// 缺陷档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 闸段错。
    Stage,
    /// errno 通道错。
    Code,
    /// 退出码通道错。
    Value,
    /// 注解错。
    Note,
    /// 判词位错。
    Default,
    /// 三通道不一致。
    Channel,
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
            Fault::Stage => EX_ESTAGE,
            Fault::Code => EX_ECODE,
            Fault::Value => EX_EVALUE,
            Fault::Note => EX_ENOTE,
            Fault::Default => EX_EDEFAULT,
            Fault::Channel => EX_ECHANNEL,
            Fault::Estate => EX_ESTATE,
            Fault::Count => EX_ECOUNT,
            Fault::Slot => EX_ESLOT,
            Fault::Seal => EX_ESEAL,
            Fault::Nobs => EX_ENOBS,
            Fault::Matrix => EX_EMATRIX,
            Fault::Bal => EX_EBAL,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Stage => "stage",
            Fault::Code => "errno",
            Fault::Value => "exit",
            Fault::Note => "note",
            Fault::Default => "verdict",
            Fault::Channel => "channel",
            Fault::Estate => "estate",
            Fault::Count => "count",
            Fault::Slot => "slot",
            Fault::Seal => "seal",
            Fault::Nobs => "nobs",
            Fault::Matrix => "matrix",
            Fault::Bal => "bal",
        }
    }

    /// 是否非零缺陷。
    pub fn alive(&self) -> bool {
        match self {
            Fault::Ok => false,
            _ => true,
        }
    }

    /// 是否六维申报缺陷之一（后六槽只允许出现这六种）。
    pub fn is_claim_fault(&self) -> bool {
        match self {
            Fault::Estate | Fault::Stage | Fault::Code | Fault::Value | Fault::Note | Fault::Default => {
                true
            }
            _ => false,
        }
    }
}

/// 缺陷短名（释义用）。
pub fn fault_short(f: Fault) -> &'static str {
    f.name()
}

/// 数字是否落在 shell 预留档（126/127/128+signum）。
///
/// 程序不该占用这三档，但现实里天天有人占——本函数把"撞车"显式化，
/// 归因位（`default`）才不会被数字本身骗到。
pub fn shell_reserved(v: u16) -> bool {
    if v == EXIT_NOT_EXEC || v == EXIT_NOT_FOUND {
        return true;
    }
    if v >= EXIT_SIGNAL_BASE {
        let off = v - EXIT_SIGNAL_BASE;
        return off <= SIG_MAX as u16;
    }
    false
}

// ---------------------------------------------------------------------------
// 形状
// ---------------------------------------------------------------------------

/// 一次"shell 要执行一个命令字"现场的全部维度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// shell 自己的 argv[0]（状态行渲染用；本卡不比对它的字节）。
    pub argv0: &'static str,
    /// 命令字（原样，含斜杠与否都是事实）。
    pub word: &'static str,
    /// PATH 里的目录项数（0 = 未设/为空）。
    pub path_dirs: u8,
    /// 实际扫过的目录数（闸四的"扫满"证据）。
    pub dirs_scanned: u8,
    /// 查找链答复档。
    pub lookup: Lookup,
    /// 哈希陈旧时是否回 PATH 重找。
    pub rehash: bool,
    /// PATH 是否被咨询过（内建与含斜杠词必须为假）。
    pub path_consulted: bool,
    /// 目录候选策略。
    pub dir_policy: DirPolicy,
    /// 内部 errno 通道（观测到的失败原因）。
    pub exec_errno: Errno,
    /// ENOEXEC 时是否按脚本重跑。
    pub fallback: bool,
    /// 是否真的起了子进程。
    pub spawned: bool,
    /// 子进程返回码（仅 `spawned` 时有意义）。
    pub child_rc: u16,
    /// 子进程是否被信号打死。
    pub signaled: bool,
    /// 打死子进程的信号号（仅 `signaled` 时有意义）。
    pub sig: u8,
    /// stderr 通道的族属（只判族，不判字节）。
    pub text: TextChan,
    /// 内建命令在当前进程的返回值。
    pub builtin_rc: u16,
}

impl Shape {
    /// 空白形状（一条真实存在的常见现场：PATH 四目录、扫满、没找到）。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            argv0: "sh",
            word: "",
            path_dirs: 4,
            dirs_scanned: 4,
            lookup: Lookup::PathMiss,
            rehash: true,
            path_consulted: true,
            dir_policy: DirPolicy::Skipped,
            exec_errno: Errno::Enoent,
            fallback: false,
            spawned: false,
            child_rc: 0,
            signaled: false,
            sig: 0,
            text: TextChan::NotFoundLine,
            builtin_rc: 0,
        }
    }

    /// 命令字形态。
    pub fn form(&self) -> WordForm {
        word_form(self.word)
    }

    /// 该词是否应当咨询 PATH（闸四的事实来源）。
    pub fn should_walk_path(&self) -> bool {
        self.form().consults_path()
    }

    /// 现场指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.argv0);
        h = fold_str(h, self.word);
        h = fold_word(h, self.path_dirs as u64);
        h = fold_word(h, self.dirs_scanned as u64);
        h = fold_str(h, self.lookup.name());
        h = fold_word(h, self.rehash as u64);
        h = fold_word(h, self.path_consulted as u64);
        h = fold_str(h, self.dir_policy.name());
        h = fold_word(h, self.exec_errno.value() as i64 as u64);
        h = fold_word(h, self.fallback as u64);
        h = fold_word(h, self.spawned as u64);
        h = fold_word(h, self.child_rc as u64);
        h = fold_word(h, self.signaled as u64);
        h = fold_word(h, self.sig as u64);
        h = fold_str(h, self.text.name());
        h = fold_word(h, self.builtin_rc as u64);
        h
    }
}

/// 判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 数字定型在哪道闸。
    pub stage: Stage,
    /// errno 通道（内部失败原因）。
    pub code: i32,
    /// 退出码通道（`$?` 那个数）。
    pub value: u16,
    /// 判词族属。
    pub note: Note,
    /// 判词位：这个 127 是 shell 给的、且三通道同真。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Word,
            code: CH_ENOENT,
            value: EXIT_NOT_FOUND,
            note: Note::NotFound127,
            default: true,
        }
    }

    /// 是否"命令找到了、事情发生了"。
    ///
    /// errno 通道必须为零**且**注解属于 ran 族——两个条件缺一不可：
    /// 只查零会把观测作废档（EINVAL）以外的假成功放过去，只查注解会把
    /// "内建跑通却带回 ENOENT"的矛盾现场放过去。
    pub fn ok(&self) -> bool {
        self.code == CH_ENONE && self.note.ran()
    }

    /// 是否失败现场。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否 shell 的 127 判词面。
    pub fn not_found(&self) -> bool {
        self.note.is_notfound()
    }

    /// 数字归因。
    pub fn origin(&self) -> Origin {
        self.note.origin()
    }

    /// 三通道是否自洽（本卡的联签形式条件）。
    ///
    /// 判据不是"值等于 127"，而是"判词位为真时四个事实同时为真"。
    /// 作废档在这里直接返回假——它的三通道本来就互相矛盾。
    pub fn channels_agree(&self) -> bool {
        if self.note.is_void() {
            return false;
        }
        if self.default {
            if self.value != EXIT_NOT_FOUND {
                return false;
            }
            if self.code != CH_ENOENT {
                return false;
            }
            if self.note != Note::NotFound127 {
                return false;
            }
        }
        if self.note.is_notfound() && self.value != EXIT_NOT_FOUND {
            return false;
        }
        if self.note.is_notexec() && self.value != EXIT_NOT_EXEC {
            return false;
        }
        if self.note.is_notexec() && !Errno::of(self.code).is_not_exec() {
            return false;
        }
        if self.note.is_notfound() && self.code != CH_ENOENT {
            return false;
        }
        if self.note.ran() && self.code != CH_ENONE {
            return false;
        }
        true
    }

    /// 结果指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as i64 as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_word(h, self.default as u64);
        h
    }
}

/// 查找链的候选答复（闸三/四/五的中间量，私有）。
enum Cand {
    /// 有候选且链条继续往下走执行。
    Exec,
    /// 找不到，归因段随附。
    NotFound(Stage),
    /// 找到了但执行不了。
    NotExec(Errno, Stage),
    /// 通道矛盾，判死在该段。
    Void(Stage),
}

/// 作废结果（三通道矛盾）。
fn void_out(g: Stage) -> Outcome {
    Outcome {
        stage: g,
        code: CH_EINVAL,
        value: EXIT_USAGE,
        note: Note::ChanSplit,
        default: false,
    }
}

/// 候选的 errno 分流（`PathHit`/`HashHit` 之后）。
///
/// 竞态（扫到又没了）落 `NotFound`；权限与软链接环落 `NotExec`；
/// ENOEXEC 看 `fallback`：重跑就继续走执行链，不重跑就 126。
fn candidate_errno(s: &Shape, g: Stage) -> Cand {
    match s.exec_errno {
        Errno::ENone => Cand::Exec,
        Errno::Enoent => Cand::NotFound(g),
        Errno::Eacces => Cand::NotExec(Errno::Eacces, Stage::Errno),
        Errno::Eisdir => Cand::NotExec(Errno::Eisdir, Stage::Errno),
        Errno::Eloop => Cand::NotExec(Errno::Eloop, Stage::Errno),
        Errno::Enoexec => {
            if s.fallback {
                Cand::Exec
            } else {
                Cand::NotExec(Errno::Enoexec, Stage::Errno)
            }
        }
        Errno::Einval => Cand::Void(Stage::Errno),
    }
}

/// 闸四：PATH 逐目录查找的答复。
fn path_answer(s: &Shape) -> Cand {
    if s.dirs_scanned > s.path_dirs {
        return Cand::Void(Stage::Search);
    }
    match s.lookup {
        Lookup::PathHit => {
            if !s.path_consulted || s.dirs_scanned == 0 {
                return Cand::Void(Stage::Search);
            }
            candidate_errno(s, Stage::Search)
        }
        Lookup::PathMiss => {
            // PATH 尽头必须是扫满：少扫一个目录就宣布找不到，是把
            // "我没看"说成"那儿没有"。
            if !s.path_consulted || s.dirs_scanned != s.path_dirs {
                return Cand::Void(Stage::Search);
            }
            if !s.exec_errno.is_not_found() {
                return Cand::Void(Stage::Search);
            }
            Cand::NotFound(Stage::Search)
        }
        Lookup::DirOnPath => {
            match s.dir_policy {
                DirPolicy::Skipped => {
                    if !s.path_consulted || s.dirs_scanned != s.path_dirs {
                        return Cand::Void(Stage::Search);
                    }
                    Cand::NotFound(Stage::Search)
                }
                DirPolicy::Candidate => {
                    if !s.path_consulted {
                        return Cand::Void(Stage::Search);
                    }
                    Cand::NotExec(Errno::Eisdir, Stage::Errno)
                }
            }
        }
        Lookup::SlashTarget => {
            // 词含 `/`：PATH 一个目录都不许扫。
            if s.path_consulted || s.dirs_scanned != 0 {
                return Cand::Void(Stage::Search);
            }
            match s.exec_errno {
                Errno::ENone => Cand::Exec,
                Errno::Enoent => Cand::NotFound(Stage::Errno),
                Errno::Eacces => Cand::NotExec(Errno::Eacces, Stage::Errno),
                Errno::Eisdir => Cand::NotExec(Errno::Eisdir, Stage::Errno),
                Errno::Eloop => Cand::NotExec(Errno::Eloop, Stage::Errno),
                Errno::Enoexec => {
                    if s.fallback {
                        Cand::Exec
                    } else {
                        Cand::NotExec(Errno::Enoexec, Stage::Errno)
                    }
                }
                Errno::Einval => Cand::Void(Stage::Errno),
            }
        }
        _ => Cand::Void(Stage::Search),
    }
}

/// 126 判词的文案族期望。
fn notexec_text(e: Errno) -> TextChan {
    match e {
        Errno::Eacces => TextChan::PermLine,
        Errno::Eisdir => TextChan::IsDirLine,
        Errno::Enoexec => TextChan::ExecFmtLine,
        Errno::Eloop => TextChan::PermLine,
        _ => TextChan::Silent,
    }
}

/// 闸六～闸八：执行链（真正起了子进程之后的归因）。
fn finish_exec(s: &Shape) -> Outcome {
    // 闸六：候选可用却没起子进程，说明"执行"这件事只存在于申报里。
    if !s.spawned {
        return void_out(Stage::Spawn);
    }
    // 闸六：既然真跑了，shell 就不该再打自己的判词条。
    if s.text.is_shell_verdict() {
        return void_out(Stage::Spawn);
    }
    // 闸七：信号优先——被打死的进程没有返回码可言。
    if s.signaled {
        if s.sig == 0 || s.sig > SIG_MAX {
            return void_out(Stage::Rc);
        }
        let v = EXIT_SIGNAL_BASE.saturating_add(s.sig as u16);
        return Outcome {
            stage: Stage::Rc,
            code: CH_ENONE,
            value: v,
            note: Note::Signaled,
            default: false,
        };
    }
    // 闸七：子进程冒充 shell 预留档——数字相同，归因必须反着写。
    if s.child_rc == EXIT_NOT_FOUND {
        return Outcome {
            stage: Stage::Rc,
            code: CH_ENONE,
            value: EXIT_NOT_FOUND,
            note: Note::ChildMimic,
            default: false,
        };
    }
    if s.child_rc == EXIT_NOT_EXEC {
        return Outcome {
            stage: Stage::Rc,
            code: CH_ENONE,
            value: EXIT_NOT_EXEC,
            note: Note::ChildNotExec,
            default: false,
        };
    }
    // 闸八：原样入账。
    let note = if s.fallback { Note::FallbackRan } else { Note::ChildRan };
    Outcome {
        stage: Stage::Commit,
        code: CH_ENONE,
        value: s.child_rc,
        note,
        default: false,
    }
}

/// 闸三～闸五：查找答复与词形态互认，再按答复分流。
///
/// 这张互认表本身就是判据：`Lookup` 说走了 PATH 而命令字含 `/`，或
/// `Lookup` 说是斜杠目标而命令字是裸名，都是申报与现场对不上，
/// 判死在候选被消费之前。
fn classify_answer(s: &Shape) -> Cand {
    let form = s.form();
    match s.lookup {
        Lookup::Builtin => Cand::Void(Stage::Builtin),
        Lookup::SlashTarget => {
            if form != WordForm::Slashed {
                return Cand::Void(Stage::Search);
            }
            path_answer(s)
        }
        Lookup::HashHit | Lookup::HashStale => {
            if form != WordForm::Bare {
                return Cand::Void(Stage::Hash);
            }
            hash_answer(s)
        }
        Lookup::PathHit | Lookup::PathMiss | Lookup::DirOnPath => {
            if form != WordForm::Bare {
                return Cand::Void(Stage::Search);
            }
            path_answer(s)
        }
    }
}

/// 校验一条 shell 执行现场，给出三通道一致的结论。
///
/// 闸门次序：`word → builtin → hash → search → errno → spawn → rc → commit`。
/// 每一段只在上一段放行后执行；一旦某段判死，后面的段不再消费这段的事实。
pub fn define(s: &Shape) -> Outcome {
    // ---- 闸一：命令字体检 ----
    let form = s.form();
    if form.is_void() {
        return void_out(Stage::Word);
    }
    if form.is_no_command() {
        // POSIX 空命令：不查找、不报错、退 0。带判词条或带 errno 就是谎报。
        if s.text.is_shell_verdict() || s.exec_errno != Errno::ENone {
            return void_out(Stage::Word);
        }
        return Outcome {
            stage: Stage::Word,
            code: CH_ENONE,
            value: EXIT_OK,
            note: Note::EmptyWord,
            default: false,
        };
    }

    // ---- 闸二：内建表先于 PATH ----
    if s.lookup == Lookup::Builtin {
        if s.path_consulted || s.dirs_scanned != 0 || s.spawned {
            return void_out(Stage::Builtin);
        }
        if s.text.is_shell_verdict() {
            return void_out(Stage::Builtin);
        }
        return Outcome {
            stage: Stage::Builtin,
            code: CH_ENONE,
            value: s.builtin_rc,
            note: Note::BuiltinOk,
            default: false,
        };
    }

    // ---- 闸三/四/五：查找链候选 ----
    let cand = classify_answer(s);

    // ---- 闸五～闸八：按候选答复定型 ----
    match cand {
        Cand::Void(g) => void_out(g),
        Cand::NotFound(g) => {
            // 闸六守卫：真跑过就不许记 127。这是本卡不可倒的那道门。
            if s.spawned {
                return void_out(Stage::Spawn);
            }
            // 三通道一致：文案必须是 notfound 族。
            if s.text != TextChan::NotFoundLine {
                return void_out(g);
            }
            Outcome {
                stage: g,
                code: CH_ENOENT,
                value: EXIT_NOT_FOUND,
                note: Note::NotFound127,
                default: true,
            }
        }
        Cand::NotExec(e, g) => {
            if s.spawned {
                return void_out(Stage::Spawn);
            }
            if s.text != notexec_text(e) {
                return void_out(g);
            }
            let note = if e == Errno::Eisdir { Note::IsDir126 } else { Note::NotExec126 };
            Outcome {
                stage: g,
                code: e.value(),
                value: EXIT_NOT_EXEC,
                note,
                default: false,
            }
        }
        Cand::Exec => finish_exec(s),
    }
}

/// 闸三：命令哈希表的答复（把复算交给闸四或直接给出候选）。
fn hash_answer(s: &Shape) -> Cand {
    match s.lookup {
        Lookup::HashHit => {
            if s.path_consulted || s.dirs_scanned != 0 {
                return Cand::Void(Stage::Hash);
            }
            candidate_errno(s, Stage::Hash)
        }
        Lookup::HashStale => {
            if s.rehash {
                if !s.path_consulted {
                    return Cand::Void(Stage::Hash);
                }
                rehash_answer(s)
            } else {
                if s.path_consulted || s.dirs_scanned != 0 {
                    return Cand::Void(Stage::Hash);
                }
                if !s.exec_errno.is_not_found() {
                    return Cand::Void(Stage::Hash);
                }
                Cand::NotFound(Stage::Hash)
            }
        }
        _ => Cand::Void(Stage::Hash),
    }
}

/// 陈旧哈希回 PATH 重找之后的答复。
///
/// 重找必须真扫到尽头；扫到一半就宣布结果，与压根没重找同罪。
fn rehash_answer(s: &Shape) -> Cand {
    if s.dirs_scanned > s.path_dirs {
        return Cand::Void(Stage::Search);
    }
    if s.spawned {
        if s.dirs_scanned == 0 || s.exec_errno != Errno::ENone {
            return Cand::Void(Stage::Search);
        }
        return Cand::Exec;
    }
    if s.dirs_scanned != s.path_dirs {
        return Cand::Void(Stage::Search);
    }
    if s.exec_errno.is_not_found() {
        return Cand::NotFound(Stage::Search);
    }
    if s.exec_errno.is_not_exec() {
        return Cand::NotExec(s.exec_errno, Stage::Errno);
    }
    Cand::Void(Stage::Search)
}

// ---------------------------------------------------------------------------
// 文案：状态面（数字域——逐字行文案归 UNX-F11665，本卡一行都不比对）
// ---------------------------------------------------------------------------

/// glibc `strerror` 的逐字文本（errno 通道的官方名字）。
///
/// 本卡拥有的是**数字与 errno 的那一对**，不是 shell 拼出来的整行。
pub fn msg_strerror(e: Errno) -> String {
    match e {
        Errno::ENone => String::from("Success"),
        Errno::Enoent => String::from("No such file or directory"),
        Errno::Eacces => String::from("Permission denied"),
        Errno::Eisdir => String::from("Is a directory"),
        Errno::Enoexec => String::from("Exec format error"),
        Errno::Eloop => String::from("Too many levels of symbolic links"),
        Errno::Einval => String::from("Invalid argument"),
    }
}

/// busybox 风格的小写理由尾（`bb_error_msg` 里 strerror 的本土化写法）。
pub fn msg_reason_tag(e: Errno) -> &'static str {
    match e {
        Errno::ENone => "success",
        Errno::Enoent => "no such file",
        Errno::Eacces => "permission denied",
        Errno::Eisdir => "is a directory",
        Errno::Enoexec => "exec format error",
        Errno::Eloop => "too many links",
        Errno::Einval => "invalid argument",
    }
}

/// `echo $?` 打出的原样字节（数字通道的官方载体）。
pub fn msg_dollar_q(v: u16) -> String {
    let mut s = dec_text(v as u32);
    s.push('\n');
    s
}

/// 数字所属的 POSIX 档族。
pub fn msg_exit_family(v: u16) -> &'static str {
    if v == EXIT_OK {
        return "success";
    }
    if v <= EXIT_PROGRAM_MAX {
        return "program";
    }
    if v == EXIT_NOT_EXEC {
        return "reserved-not-executable";
    }
    if v == EXIT_NOT_FOUND {
        return "reserved-not-found";
    }
    if v >= EXIT_SIGNAL_BASE {
        let off = v - EXIT_SIGNAL_BASE;
        if off >= 1 && off <= SIG_MAX as u16 {
            return "signal";
        }
    }
    "out-of-band"
}

/// 信号名（Linux 常用档；未知号回 `SIG?`，绝不冒充已知名）。
pub fn sig_name(sig: u8) -> &'static str {
    match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        10 => "SIGUSR1",
        11 => "SIGSEGV",
        12 => "SIGUSR2",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        17 => "SIGCHLD",
        18 => "SIGCONT",
        19 => "SIGSTOP",
        20 => "SIGTSTP",
        _ => "SIG?",
    }
}

/// 信号档的算术行：`128 + 9 = 137 (SIGKILL)`。
pub fn msg_signal_line(sig: u8) -> String {
    let mut s = String::from("128 + ");
    s.push_str(dec_text(sig as u32).as_str());
    s.push_str(" = ");
    let v = EXIT_SIGNAL_BASE.saturating_add(sig as u16);
    s.push_str(dec_text(v as u32).as_str());
    s.push_str(" (");
    s.push_str(sig_name(sig));
    s.push(')');
    s
}

/// 三通道一致性行（本卡的"三通道一致"逐字形态）。
pub fn msg_channel(o: &Outcome, spawned: bool) -> String {
    let mut s = String::from("exit=");
    s.push_str(dec_text(o.value as u32).as_str());
    s.push_str(" errno=");
    s.push_str(Errno::of(o.code).name());
    s.push_str(" spawned=");
    s.push_str(if spawned { "yes" } else { "no" });
    s
}

/// 判词位为真的 127 现场的状态行（逐字常量，供判据对表）。
pub const VERDICT_127_LIT: &str = "exit=127 errno=ENOENT spawned=no";

/// 子进程冒充 127 的状态行（数字相同、归因相反的逐字对照）。
pub const MIMIC_127_LIT: &str = "exit=127 errno=0 spawned=yes";

/// 状态面主行：`词 stage=… errno=… exit=… 族=…`。
pub fn message(s: &Shape, o: &Outcome) -> String {
    let mut t = String::from(s.word);
    t.push_str(" stage=");
    t.push_str(o.stage.name());
    t.push_str(" errno=");
    t.push_str(Errno::of(o.code).name());
    t.push_str(" exit=");
    t.push_str(dec_text(o.value as u32).as_str());
    t.push_str(" note=");
    t.push_str(o.note.name());
    t.push_str(" origin=");
    t.push_str(o.origin().name());
    t
}

/// 状态面尾行：shell 归属与词形态（`sh/bare`）。
pub fn message_tail(s: &Shape) -> String {
    let mut t = String::from(last_component(s.argv0));
    t.push('/');
    t.push_str(s.form().name());
    t.push_str(" dirs=");
    t.push_str(dec_text(s.dirs_scanned as u32).as_str());
    t.push('/');
    t.push_str(dec_text(s.path_dirs as u32).as_str());
    t
}

// ---------------------------------------------------------------------------
// 申报与用例
// ---------------------------------------------------------------------------

/// 一槽申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报的定型闸段。
    pub stage: Stage,
    /// 申报的 errno 通道。
    pub code: i32,
    /// 申报的退出码。
    pub value: u16,
    /// 申报的判词族属。
    pub note: Note,
    /// 申报的判词位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Commit,
            code: CH_ENONE,
            value: EXIT_OK,
            note: Note::ChildRan,
            default: false,
        }
    }

    /// 是否申报"命令跑通了"。
    ///
    /// 与 `Outcome::ok()` 同构：errno 通道为零 **且** 注解属于 ran 族。
    /// 用 stage 判成功会把"闸七申报但带着 ENOENT"的自相矛盾当成成功。
    pub fn success(&self) -> bool {
        self.code == CH_ENONE && self.note.ran()
    }

    /// 是否申报 127 判词。
    pub fn claims_verdict(&self) -> bool {
        self.default
    }

    /// 申报的归因档。
    pub fn origin(&self) -> Origin {
        self.note.origin()
    }

    /// 申报自身三通道是否自洽（一个红档申报也可能是自洽的错申报）。
    pub fn self_consistent(&self) -> bool {
        if self.note.is_void() {
            return false;
        }
        if self.default {
            if self.value != EXIT_NOT_FOUND || self.code != CH_ENOENT {
                return false;
            }
            if !self.note.is_notfound() {
                return false;
            }
        }
        if self.note.is_notfound() {
            return self.value == EXIT_NOT_FOUND && self.code == CH_ENOENT;
        }
        if self.note.is_notexec() {
            return self.value == EXIT_NOT_EXEC && Errno::of(self.code).is_not_exec();
        }
        if self.note.ran() {
            return self.code == CH_ENONE;
        }
        true
    }
}

/// 一槽用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 现场。
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
// 语料（14 槽 = 8 活 + 6 死）
// ---------------------------------------------------------------------------

/// 8 个可正确申报的现场 + 6 个"同一个 127 判词、六种误报"的现场。
///
/// 后六槽的**到达路径互不相同**（裸名扫尽、带斜杠缺失、目录被跳过、PATH
/// 为空、陈旧哈希、另一层 shell 的 argv0），实际判词却逐位相同——这正是
/// 本卡要的红档显式：一档数字能被六条路走歪，现场本身没有区别。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 内建命中：PATH 一个目录都不许扫，数字是内建在当前进程的返回值。
    Shape {
        tag: "builtin-true-ran",
        argv0: "sh",
        word: "true",
        path_dirs: 4,
        dirs_scanned: 0,
        lookup: Lookup::Builtin,
        rehash: true,
        path_consulted: false,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::ENone,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::Silent,
        builtin_rc: 0,
    },
    // 1 PATH 命中并跑通：数字 0，三通道说"事情发生了"。
    Shape {
        tag: "path-hit-child-zero",
        argv0: "bash",
        word: "ls",
        path_dirs: 6,
        dirs_scanned: 3,
        lookup: Lookup::PathHit,
        rehash: true,
        path_consulted: true,
        dir_policy: DirPolicy::Candidate,
        exec_errno: Errno::ENone,
        fallback: false,
        spawned: true,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::Silent,
        builtin_rc: 0,
    },
    // 2 只有赋值前缀：POSIX 空命令，退 0，绝不进查找链。
    Shape {
        tag: "assign-only-empty",
        argv0: "sh",
        word: "PREFIX=1",
        path_dirs: 0,
        dirs_scanned: 0,
        lookup: Lookup::PathMiss,
        rehash: true,
        path_consulted: false,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::ENone,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::Silent,
        builtin_rc: 0,
    },
    // 3 哈希命中且仍有效：跳过 PATH 直接执行成功。
    Shape {
        tag: "hash-hit-exec-ok",
        argv0: "bash",
        word: "grep",
        path_dirs: 5,
        dirs_scanned: 0,
        lookup: Lookup::HashHit,
        rehash: true,
        path_consulted: false,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::ENone,
        fallback: false,
        spawned: true,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::Silent,
        builtin_rc: 0,
    },
    // 4 PATH 扫尽无候选：本卡主判档。
    Shape {
        tag: "path-exhausted-127",
        argv0: "sh",
        word: "nosuchtool",
        path_dirs: 4,
        dirs_scanned: 4,
        lookup: Lookup::PathMiss,
        rehash: true,
        path_consulted: true,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::Enoent,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::NotFoundLine,
        builtin_rc: 0,
    },
    // 5 哈希陈旧且回 PATH 重找：数字与闸四那一路同为 127，归因不同。
    Shape {
        tag: "stale-hash-rehash-127",
        argv0: "bash",
        word: "oldname",
        path_dirs: 3,
        dirs_scanned: 3,
        lookup: Lookup::HashStale,
        rehash: true,
        path_consulted: true,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::Enoent,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::NotFoundLine,
        builtin_rc: 0,
    },
    // 6 找到了但没有执行权限：126，绝非 127——文件明明在那儿。
    Shape {
        tag: "eacces-found-126",
        argv0: "sh",
        word: "helper",
        path_dirs: 6,
        dirs_scanned: 3,
        lookup: Lookup::PathHit,
        rehash: true,
        path_consulted: true,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::Eacces,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::PermLine,
        builtin_rc: 0,
    },
    // 7 子进程自己退 127：数字撞车，归因必须是子进程，判词位为假。
    Shape {
        tag: "child-exits-127",
        argv0: "sh",
        word: "init-script",
        path_dirs: 4,
        dirs_scanned: 2,
        lookup: Lookup::PathHit,
        rehash: true,
        path_consulted: true,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::ENone,
        fallback: false,
        spawned: true,
        child_rc: 127,
        signaled: false,
        sig: 0,
        text: TextChan::ChildBytes,
        builtin_rc: 0,
    },
    // 8 裸名扫尽，申报却谎称跑通。
    Shape {
        tag: "estate-swap",
        argv0: "ash",
        word: "definitely-not-here",
        path_dirs: 4,
        dirs_scanned: 4,
        lookup: Lookup::PathMiss,
        rehash: true,
        path_consulted: true,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::Enoent,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::NotFoundLine,
        builtin_rc: 0,
    },
    // 9 带斜杠的缺失目标：数字同样 127，定型闸却在 errno。
    Shape {
        tag: "stage-swap",
        argv0: "sh",
        word: "/usr/local/bin/nope",
        path_dirs: 4,
        dirs_scanned: 0,
        lookup: Lookup::SlashTarget,
        rehash: true,
        path_consulted: false,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::Enoent,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::NotFoundLine,
        builtin_rc: 0,
    },
    // 10 PATH 里那个名字是目录，本 shell 跳过它 → 127；申报写成 EACCES。
    Shape {
        tag: "code-swap",
        argv0: "ash",
        word: "mydir",
        path_dirs: 5,
        dirs_scanned: 5,
        lookup: Lookup::DirOnPath,
        rehash: true,
        path_consulted: true,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::Enoent,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::NotFoundLine,
        builtin_rc: 0,
    },
    // 11 PATH 为空（未设）：裸名无处可寻，数字仍 127；申报写成 126。
    Shape {
        tag: "value-swap",
        argv0: "sh",
        word: "whatever",
        path_dirs: 0,
        dirs_scanned: 0,
        lookup: Lookup::PathMiss,
        rehash: true,
        path_consulted: true,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::Enoent,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::NotFoundLine,
        builtin_rc: 0,
    },
    // 12 陈旧哈希未重找、直接硬执行：数字 127 但归因在 hash 闸；
    //    申报把族属写成 126 那一族。
    Shape {
        tag: "note-swap",
        argv0: "bash",
        word: "gone-cmd",
        path_dirs: 4,
        dirs_scanned: 0,
        lookup: Lookup::HashStale,
        rehash: false,
        path_consulted: false,
        dir_policy: DirPolicy::Skipped,
        exec_errno: Errno::Enoent,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::NotFoundLine,
        builtin_rc: 0,
    },
    // 13 另一层 shell 的现场（argv0 为 dash）：判词位被抹掉。
    Shape {
        tag: "default-swap",
        argv0: "dash",
        word: "missing-thing",
        path_dirs: 3,
        dirs_scanned: 3,
        lookup: Lookup::PathMiss,
        rehash: true,
        path_consulted: true,
        dir_policy: DirPolicy::Candidate,
        exec_errno: Errno::Enoent,
        fallback: false,
        spawned: false,
        child_rc: 0,
        signaled: false,
        sig: 0,
        text: TextChan::NotFoundLine,
        builtin_rc: 0,
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Builtin, code: CH_ENONE, value: 0, note: Note::BuiltinOk, default: false },
    Claim { stage: Stage::Commit, code: CH_ENONE, value: 0, note: Note::ChildRan, default: false },
    Claim { stage: Stage::Word, code: CH_ENONE, value: 0, note: Note::EmptyWord, default: false },
    Claim { stage: Stage::Commit, code: CH_ENONE, value: 0, note: Note::ChildRan, default: false },
    Claim { stage: Stage::Search, code: CH_ENOENT, value: 127, note: Note::NotFound127, default: true },
    Claim { stage: Stage::Search, code: CH_ENOENT, value: 127, note: Note::NotFound127, default: true },
    Claim { stage: Stage::Errno, code: CH_EACCES, value: 126, note: Note::NotExec126, default: false },
    Claim { stage: Stage::Rc, code: CH_ENONE, value: 127, note: Note::ChildMimic, default: false },
    Claim { stage: Stage::Commit, code: CH_ENONE, value: 0, note: Note::ChildRan, default: false },
    Claim { stage: Stage::Search, code: CH_ENOENT, value: 127, note: Note::NotFound127, default: true },
    Claim { stage: Stage::Search, code: CH_EACCES, value: 127, note: Note::NotFound127, default: true },
    Claim { stage: Stage::Search, code: CH_ENOENT, value: 126, note: Note::NotFound127, default: true },
    Claim { stage: Stage::Hash, code: CH_ENOENT, value: 127, note: Note::NotExec126, default: true },
    Claim { stage: Stage::Search, code: CH_ENOENT, value: 127, note: Note::NotFound127, default: false },
];

/// 后 6 槽的期望缺陷（次序固定：Estate/Stage/Code/Value/Note/Default）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 后 6 槽的缺陷码（六个互不相同的 `E*`）。
pub const REJECT_CODES: [u32; REJECT] = [
    EX_ESTATE,
    EX_ESTAGE,
    EX_ECODE,
    EX_EVALUE,
    EX_ENOTE,
    EX_EDEFAULT,
];

/// 账本 8 个成格的值：总数、成、败、跑通、127 档、126 档、外数、红档率‰。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 4, 2, 1, 1, 929];

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            if let Some(slot) = out.get_mut(i) {
                *slot = Case { shape: *s, claim: *c };
            }
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
        return Err(EX_ENOBS);
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

/// 对判。
///
/// 判序固定为 Estate → Stage → Code → Value → Note → Default → Ok。
/// Estate 最先的理由：把"127 现场"说成"跑通了"（或反之）是本卡最贵的一类
/// 误报——它直接推翻归因；如果先比阶段，一条既报成功又报 `Commit` 段的
/// 假申报会被降级成"数字笔误"，造假就被洗成了手滑。
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

/// 该槽除申报外的**通道自洽**缺陷（与 `judge` 正交的第二把尺）。
///
/// `judge` 比的是"申报对不对"，本函数比的是"现场本身成不成一句话"：
/// 六维全对但三通道背离的现场，`judge` 会说 Ok，这里必须说 Channel。
/// 两把尺同时存在，才不会出现"错得自洽就算对"的漏洞。
pub fn channel_fault(c: &Case) -> Fault {
    let a = c.actual();
    if a.note.is_void() {
        return Fault::Channel;
    }
    if !a.channels_agree() {
        return Fault::Channel;
    }
    Fault::Ok
}

/// 逐槽释义。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let mut s = String::from(c.shape.tag);
    s.push_str(" ");
    s.push_str(message(&c.shape, &o).as_str());
    s.push_str(" claim=");
    s.push_str(c.claim.stage.name());
    s.push(':');
    s.push_str(Errno::of(c.claim.code).name());
    s.push(':');
    s.push_str(dec_text(c.claim.value as u32).as_str());
    s.push_str(" fault=");
    s.push_str(fault_short(judge(c)));
    s
}

/// 释义尾行（数字通道的官方形态）。
pub fn detail_tail(c: &Case) -> String {
    let o = c.actual();
    let mut s = String::from("[$?] ");
    s.push_str(msg_dollar_q(o.value).as_str());
    s
}

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式复写——任一处笔误都要露出来）
// ---------------------------------------------------------------------------

/// 每槽期望闸段（形式二：直接写 `Stage`，与 `CLAIMS` 交叉校验）。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Builtin,
    Stage::Commit,
    Stage::Word,
    Stage::Commit,
    Stage::Search,
    Stage::Search,
    Stage::Errno,
    Stage::Rc,
    Stage::Commit,
    Stage::Search,
    Stage::Search,
    Stage::Search,
    Stage::Hash,
    Stage::Search,
];

/// 每槽期望 errno 通道（形式二）。
const WANT_CODE: [i32; CORPUS] = [
    CH_ENONE,
    CH_ENONE,
    CH_ENONE,
    CH_ENONE,
    CH_ENOENT,
    CH_ENOENT,
    CH_EACCES,
    CH_ENONE,
    CH_ENONE,
    CH_ENOENT,
    CH_EACCES,
    CH_ENOENT,
    CH_ENOENT,
    CH_ENOENT,
];

/// 每槽期望数字（形式二）。
const WANT_VALUE: [u16; CORPUS] = [0, 0, 0, 0, 127, 127, 126, 127, 0, 127, 127, 126, 127, 127];

/// 每槽期望注解（形式二）。
const WANT_NOTE: [Note; CORPUS] = [
    Note::BuiltinOk,
    Note::ChildRan,
    Note::EmptyWord,
    Note::ChildRan,
    Note::NotFound127,
    Note::NotFound127,
    Note::NotExec126,
    Note::ChildMimic,
    Note::ChildRan,
    Note::NotFound127,
    Note::NotFound127,
    Note::NotFound127,
    Note::NotExec126,
    Note::NotFound127,
];

/// 每槽期望判词位（形式二）。
const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, true, true, false, false, false, true, true, true, true, false,
];

/// 每槽标签（形式二：字符串表，与 `SHAPES` 交叉校验）。
const WANT_TAGS: [&str; CORPUS] = [
    "builtin-true-ran",
    "path-hit-child-zero",
    "assign-only-empty",
    "hash-hit-exec-ok",
    "path-exhausted-127",
    "stale-hash-rehash-127",
    "eacces-found-126",
    "child-exits-127",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 每槽期望缺陷短名（形式二：字符串表，与 `expected()` 交叉校验）。
const WANT_FAULT: [&str; CORPUS] = [
    "ok",
    "ok",
    "ok",
    "ok",
    "ok",
    "ok",
    "ok",
    "ok",
    "estate",
    "stage",
    "errno",
    "exit",
    "note",
    "verdict",
];

/// 每槽期望归因（形式二：字符串表，与 `CLAIMS` 的 note 投影交叉校验）。
const WANT_ORIGIN: [&str; CORPUS] = [
    "builtin",
    "child-status",
    "empty-command",
    "child-status",
    "shell-notfound",
    "shell-notfound",
    "shell-notexec",
    "child-status",
    "child-status",
    "shell-notfound",
    "shell-notfound",
    "shell-notfound",
    "shell-notexec",
    "shell-notfound",
];

/// 每槽实际结果的期望档（由 `define` 复算，不看申报）。
const WANT_TIER: [Tier; CORPUS] = [
    Tier::Run,
    Tier::Run,
    Tier::Run,
    Tier::Run,
    Tier::NotFound,
    Tier::NotFound,
    Tier::NotExec,
    Tier::Foreign,
    Tier::NotFound,
    Tier::NotFound,
    Tier::NotFound,
    Tier::NotFound,
    Tier::NotFound,
    Tier::NotFound,
];

/// 成格槽位表。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 败格槽位表。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 后六槽各坏哪一维的中文说明（红档显式单子的第二种形式）。
const REJECT_DIM_TEXT: [&str; REJECT] = [
    "状态维：127 现场被申报成跑通",
    "闸段维：errno 闸定的数被申报成查找闸定的",
    "errno 维：ENOENT 被申报成 EACCES",
    "数字维：127 被申报成 126",
    "注解维：notfound 族被申报成 notexec 族",
    "判词位：shell 给的 127 被抹掉归因",
];

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
/// 分档只看**实际结果**，申报正确与否单独计。本卡六格误报的现场全部落
/// `NotFound` 档，于是那一行必然出现 `total=8 good=2 red=6`：覆盖面看着满，
/// 红档却挤在同一行——这就是把 total 与 good 拆成两维的理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（run/notfound/notexec/foreign）。
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
                n = n.saturating_add(r.total);
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
                n = n.saturating_add(r.good);
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

    /// 红档总数。
    pub fn reds(&self) -> usize {
        self.slots().saturating_sub(self.goods())
    }

    /// 挤在 127 档里的红档数（本卡的红档显式指标）。
    pub fn verdict_reds(&self) -> usize {
        match self.rows.get(Tier::NotFound.index()) {
            Some(r) => r.red(),
            None => 0,
        }
    }

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let tiers = [Tier::Run, Tier::NotFound, Tier::NotExec, Tier::Foreign];
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

/// 全矩阵逐字文本（供判据对表）。
pub const MATRIX_LIT: &str = "run total=4 good=4 red=0\nnotfound total=8 good=2 red=6\nnotexec total=1 good=1 red=0\nforeign total=1 good=1 red=0\n";

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
    /// 跑通档正确数。
    pub run: u32,
    /// 127 档正确数。
    pub notfound: u32,
    /// 126 档正确数。
    pub notexec: u32,
    /// 外数档正确数。
    pub foreign: u32,
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
            Err(EX_ECOUNT)
        }
    }

    /// 成功档合计。
    pub fn success_cells(&self) -> u32 {
        self.run
    }

    /// 失败档合计（shell 判词面）。
    pub fn verdict_cells(&self) -> u32 {
        self.notfound.saturating_add(self.notexec)
    }

    /// 127 档占申报正确格的比例（百分数，整型）。
    pub fn verdict_share(&self) -> u32 {
        if self.accept == 0 {
            return 0;
        }
        (self.verdict_cells() * 100) / self.accept
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("notfound-exit corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" run=");
        s.push_str(u32_text(self.run).as_str());
        s.push_str(" notfound=");
        s.push_str(u32_text(self.notfound).as_str());
        s.push_str(" notexec=");
        s.push_str(u32_text(self.notexec).as_str());
        s.push_str(" foreign=");
        s.push_str(u32_text(self.foreign).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染文本（逐字钉住）。
const RENDER_OK: &str = "notfound-exit corpus=14 accept=8 reject=6 run=4 notfound=2 notexec=1 foreign=1 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，matched
/// 必须掉、rate 必须离开 1000。若注入后仍报全绿，说明 matched 是硬编码的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(EX_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut run = 0u32;
    let mut notfound = 0u32;
    let mut notexec = 0u32;
    let mut foreign = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(EX_ENOBS),
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
            None => return Err(EX_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Run => run += 1,
                Tier::NotFound => notfound += 1,
                Tier::NotExec => notexec += 1,
                Tier::Foreign => foreign += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, run, notfound, notexec, foreign, matched })
}

/// 注入第 0 槽缺陷后的红档率（账本第 8 格的来源）。
pub fn red_rate() -> u32 {
    match run_with(Some((0usize, Fault::Code))) {
        Ok(r) => r.rate(),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 判定纵线 `word → builtin → hash → search → errno → spawn → rc → commit`
/// 八段只许向前。回退（例如已经判死在 `errno` 段，又声称回到 `hash` 段
/// 复核）说明申报者没有按闸门次序观测，而是事后倒推——这类申报在双机对照
/// 里最危险：数字与文案都完全合理，只有归因链是假的。
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
            return Err(EX_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(EX_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(EX_ESTATE);
        }
        self.closed = true;
        Ok(self.steps)
    }

    /// 段数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 上一段次序。
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
    /// 槽位（必须严格等于当前长度才可登记）。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功格。
    pub ok: bool,
    /// 缺陷码（成功格恒零）。
    pub code: u32,
    /// 观测值（失败格恒零）。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }

    /// 格指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.slot as u64);
        h = fold_word(h, self.op as u64);
        h = fold_word(h, self.ok as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h
    }

    /// 格文本。
    pub fn text(&self) -> String {
        let mut s = String::from("slot ");
        s.push_str(dec_text(self.slot as u32).as_str());
        s.push_str(" op ");
        s.push_str(dec_text(self.op as u32).as_str());
        s.push_str(" ok ");
        s.push_str(bool_text(self.ok));
        s.push_str(" code ");
        s.push_str(hex_text(self.code, 5).as_str());
        s.push_str(" value ");
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
            return Err(EX_ESLOT);
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
        self.len.saturating_sub(self.ok_count())
    }

    /// 逐格复核：槽位连续、成格值逐格等于 `VALS`、败格码逐格等于
    /// `REJECT_CODES` 且败格值必须为 0。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(EX_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EX_EBAL),
            };
            if s.slot as usize != i {
                return Err(EX_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EX_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EX_EVALUE);
                        }
                    }
                    None => return Err(EX_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(EX_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EX_EVALUE);
                        }
                    }
                    None => return Err(EX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(EX_EBAL);
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
            return Err(EX_EBAL);
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
            Err(EX_ESEAL)
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

    /// 全账文本。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(g) = self.rows.get(i) {
                s.push_str(g.text().as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
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
    let _ = l.register(count_cell(3, OP_BUILTIN, r.map(|x| x.run)));
    let _ = l.register(count_cell(4, OP_SEARCH, r.map(|x| x.notfound)));
    let _ = l.register(count_cell(5, OP_ERRNO, r.map(|x| x.notexec)));
    let _ = l.register(count_cell(6, OP_SPAWN, r.map(|x| x.foreign)));

    let red = run_with(Some((0usize, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_ERRNO,
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
// 四路注入
// ---------------------------------------------------------------------------

/// 四路注入：表路 / 判路 / 渲染路 / 账本路。
///
/// 1. 表路——段基址、码档表、errno 映射、闸段次序；
/// 2. 判路——八闸纵线、126/127 分界、归因位、对判、矩阵；
/// 3. 渲染路——语料、期望表复写、红档显式、状态面文本、释义；
/// 4. 账本路——阶段闸门单调、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && codes_ok() && stages_ok() && lookup_ok();
    out[1] = define_ok() && boundary_ok() && origin_ok() && chan_ok() && judge_ok() && matrix_ok();
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
// 判据基元（13 条判据的私有谓词）
// ---------------------------------------------------------------------------

/// 造一条探针现场（只服务于谓词自检，不进语料、不进账本）。
fn probe(tag: &'static str, word: &'static str, lookup: Lookup) -> Shape {
    let mut s = Shape::blank();
    s.tag = tag;
    s.word = word;
    s.lookup = lookup;
    s
}

/// 名称与段基址：常量链、尺值、码档、errno、操作码、锚点字面量。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d1a0 {
        return false;
    }
    if EX_ESTAGE != SEG_BASE + 0x01
        || EX_ECODE != SEG_BASE + 0x02
        || EX_EVALUE != SEG_BASE + 0x03
        || EX_ENOTE != SEG_BASE + 0x04
        || EX_EDEFAULT != SEG_BASE + 0x05
        || EX_ECHANNEL != SEG_BASE + 0x06
    {
        return false;
    }
    if EX_ESTATE != SEG_BASE + 0x07
        || EX_ECOUNT != SEG_BASE + 0x08
        || EX_ESLOT != SEG_BASE + 0x09
        || EX_ESEAL != SEG_BASE + 0x0a
        || EX_ENOBS != SEG_BASE + 0x0b
        || EX_EMATRIX != SEG_BASE + 0x0c
    {
        return false;
    }
    if EX_EBAL != SEG_BASE + 0x0d {
        return false;
    }
    // 十三码表逐位对表，且不得有重复码。
    let mut t = 0usize;
    while t < CODE_TABLE.len() {
        let want = match SEG_BASE.checked_add(t as u32 + 1) {
            Some(v) => v,
            None => return false,
        };
        match CODE_TABLE.get(t) {
            Some(v) => {
                if *v != want {
                    return false;
                }
            }
            None => return false,
        }
        let mut k = t + 1;
        while k < CODE_TABLE.len() {
            if let (Some(a), Some(b)) = (CODE_TABLE.get(t), CODE_TABLE.get(k)) {
                if a == b {
                    return false;
                }
            }
            k += 1;
        }
        t += 1;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if STAGE_COUNT != 8 || CAP != 6 || PATH_DIR_MAX != 32 || WORD_MAX != 127 {
        return false;
    }
    if ACCEPT + REJECT != CORPUS {
        return false;
    }
    // POSIX 留给 shell 的三个数。
    if EXIT_OK != 0 || EXIT_PROGRAM_MAX != 125 {
        return false;
    }
    if EXIT_NOT_EXEC != 126 || EXIT_NOT_FOUND != 127 || EXIT_SIGNAL_BASE != 128 {
        return false;
    }
    if EXIT_USAGE != 2 || EXIT_CHILD_MIMIC != 127 {
        return false;
    }
    if SIG_KILL != 9 || SIG_SEGV != 11 || SIG_TERM != 15 || SIG_MAX != 64 {
        return false;
    }
    // errno 通道的取值必须是真机值，不是本卡随手编的数。
    if CH_ENONE != 0 || CH_ENOENT != 2 || CH_EACCES != 13 || CH_EISDIR != 21 {
        return false;
    }
    if CH_ENOEXEC != 8 || CH_ELOOP != 40 || CH_EINVAL != 22 {
        return false;
    }
    if OP_WORD != 1 || OP_BUILTIN != 2 || OP_HASH != 3 || OP_SEARCH != 4 {
        return false;
    }
    if OP_ERRNO != 5 || OP_SPAWN != 6 || OP_CORPUS != 7 || OP_REPLAY != 8 {
        return false;
    }
    // 锚点字面量必须能被生成器逐字复现——这就是"与基准机一致"的物理形式。
    let mut v = Shape::blank();
    v.tag = "anchor-check";
    v.word = "nosuchtool";
    let o = define(&v);
    if msg_channel(&o, v.spawned) != VERDICT_127_LIT {
        return false;
    }
    let mut m = probe("mimic-check", "init-script", Lookup::PathHit);
    m.path_dirs = 4;
    m.dirs_scanned = 2;
    m.exec_errno = Errno::ENone;
    m.spawned = true;
    m.child_rc = 127;
    m.text = TextChan::ChildBytes;
    let om = define(&m);
    if msg_channel(&om, m.spawned) != MIMIC_127_LIT {
        return false;
    }
    let mut w = String::new();
    w.push_str("reserved-not-found");
    text_len(w.as_str()) == 18 && !has_byte(w.as_str(), b'\n')
}

/// 码档与 errno 映射表：数轴的每一格都要点得名。
fn codes_ok() -> bool {
    // errno → 数字的唯一合法映射。
    if Errno::Enoent.exit_of() != EXIT_NOT_FOUND {
        return false;
    }
    if Errno::Eacces.exit_of() != EXIT_NOT_EXEC
        || Errno::Eisdir.exit_of() != EXIT_NOT_EXEC
        || Errno::Enoexec.exit_of() != EXIT_NOT_EXEC
        || Errno::Eloop.exit_of() != EXIT_NOT_EXEC
    {
        return false;
    }
    if Errno::ENone.exit_of() != EXIT_USAGE || Errno::Einval.exit_of() != EXIT_USAGE {
        return false;
    }
    // 存在性分类：ENOENT 以外都承认"文件在那儿"。
    if Errno::Enoent.implies_exists() || Errno::Einval.implies_exists() {
        return false;
    }
    if !Errno::Eacces.implies_exists() || !Errno::Eisdir.implies_exists() {
        return false;
    }
    if !Errno::Enoexec.implies_exists() || !Errno::Eloop.implies_exists() {
        return false;
    }
    if Errno::Enoent.is_not_exec() || Errno::Eacces.is_not_found() {
        return false;
    }
    // 数值↔档位的双射（未知数必须落哨兵档，不许冒充 ENOENT）。
    let vals = [CH_ENONE, CH_ENOENT, CH_EACCES, CH_EISDIR, CH_ENOEXEC, CH_ELOOP, CH_EINVAL];
    let mut i = 0usize;
    while i < vals.len() {
        let v = match vals.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if Errno::of(v).value() != v {
            return false;
        }
        i += 1;
    }
    if Errno::of(9999) != Errno::Einval || Errno::of(-1) != Errno::Einval {
        return false;
    }
    // 文案通道的族属期望必须与 errno 映射同表（三通道一致性的静态面）。
    if TextChan::NotFoundLine.expected_errno() != CH_ENOENT {
        return false;
    }
    if TextChan::NotFoundLine.expected_exit() != EXIT_NOT_FOUND {
        return false;
    }
    if TextChan::PermLine.expected_errno() != CH_EACCES || TextChan::PermLine.expected_exit() != 126 {
        return false;
    }
    if TextChan::IsDirLine.expected_errno() != CH_EISDIR || TextChan::IsDirLine.expected_exit() != 126 {
        return false;
    }
    if TextChan::ExecFmtLine.expected_errno() != CH_ENOEXEC {
        return false;
    }
    if TextChan::Silent.is_shell_verdict() || TextChan::ChildBytes.is_shell_verdict() {
        return false;
    }
    if !TextChan::NotFoundLine.is_shell_verdict() || !TextChan::PermLine.is_shell_verdict() {
        return false;
    }
    if !TextChan::IsDirLine.is_shell_verdict() || !TextChan::ExecFmtLine.is_shell_verdict() {
        return false;
    }
    // shell 预留档判定。
    if !shell_reserved(126) || !shell_reserved(127) || !shell_reserved(137) {
        return false;
    }
    if shell_reserved(0) || shell_reserved(1) || shell_reserved(125) || shell_reserved(200) {
        return false;
    }
    if shell_reserved(EXIT_SIGNAL_BASE + SIG_MAX as u16 + 1) {
        return false;
    }
    // 数字族属。
    if msg_exit_family(0) != "success" || msg_exit_family(1) != "program" {
        return false;
    }
    if msg_exit_family(125) != "program" || msg_exit_family(126) != "reserved-not-executable" {
        return false;
    }
    if msg_exit_family(127) != "reserved-not-found" || msg_exit_family(137) != "signal" {
        return false;
    }
    if msg_exit_family(200) != "out-of-band" {
        return false;
    }
    // `$?` 的官方载体：只有数字和一个换行，别的一律不许混进来。
    if msg_dollar_q(127) != "127\n" || msg_dollar_q(0) != "0\n" {
        return false;
    }
    if msg_dollar_q(137) != "137\n" {
        return false;
    }
    // 信号算术。
    if msg_signal_line(SIG_KILL) != "128 + 9 = 137 (SIGKILL)" {
        return false;
    }
    if msg_signal_line(SIG_SEGV) != "128 + 11 = 139 (SIGSEGV)" {
        return false;
    }
    if sig_name(9) != "SIGKILL" || sig_name(11) != "SIGSEGV" || sig_name(15) != "SIGTERM" {
        return false;
    }
    if sig_name(64) != "SIG?" || sig_name(0) != "SIG?" {
        return false;
    }
    // strerror 逐字（首字母大写、无句号、无换行）。
    let es = [
        Errno::ENone,
        Errno::Enoent,
        Errno::Eacces,
        Errno::Eisdir,
        Errno::Enoexec,
        Errno::Eloop,
        Errno::Einval,
    ];
    let mut k = 0usize;
    while k < es.len() {
        let e = match es.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let t = msg_strerror(e);
        if t.is_empty() || has_byte(t.as_str(), b'\n') || has_byte(t.as_str(), b'.') {
            return false;
        }
        k += 1;
    }
    if msg_strerror(Errno::Enoent) != "No such file or directory" {
        return false;
    }
    if msg_strerror(Errno::Eacces) != "Permission denied" {
        return false;
    }
    if msg_strerror(Errno::Eisdir) != "Is a directory" {
        return false;
    }
    if msg_strerror(Errno::Enoexec) != "Exec format error" {
        return false;
    }
    if msg_strerror(Errno::Eloop) != "Too many levels of symbolic links" {
        return false;
    }
    if msg_reason_tag(Errno::Enoent) != "no such file" || msg_reason_tag(Errno::Eisdir) != "is a directory"
    {
        return false;
    }
    // 文本基元：进制与定宽，判据里所有数字都从它们出来。
    if u32_text(929) != "929" || hex_text(SEG_BASE, 5) != "6d1a0" {
        return false;
    }
    if i32_text(-2) != "-2" || i32_text(0) != "0" || i32_text(21) != "21" {
        return false;
    }
    if bool_text(true) != "true" || bool_text(false) != "false" {
        return false;
    }
    dec_text(0) == "0" && dec_text(1000000000) == "1000000000"
}

/// 闸段次序：八段单调、文本唯一、越界兜底、"跑过"这条线的位置。
fn stages_ok() -> bool {
    if Stage::count() != STAGE_COUNT {
        return false;
    }
    let ss = [
        Stage::Word,
        Stage::Builtin,
        Stage::Hash,
        Stage::Search,
        Stage::Errno,
        Stage::Spawn,
        Stage::Rc,
        Stage::Commit,
    ];
    let mut i = 0usize;
    while i < ss.len() {
        let s = match ss.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if s.order() != i {
            return false;
        }
        if Stage::from_order(s.order()) != s {
            return false;
        }
        let mut k = i + 1;
        while k < ss.len() {
            if let (Some(a), Some(b)) = (ss.get(i), ss.get(k)) {
                if a.name() == b.name() {
                    return false;
                }
                if a.order() == b.order() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    if Stage::from_order(99) != Stage::Commit {
        return false;
    }
    // "跑过"这条线：Spawn 及其之后不得再持有 shell 判词。
    if !Stage::Spawn.past_spawn() || !Stage::Rc.past_spawn() || !Stage::Commit.past_spawn() {
        return false;
    }
    if Stage::Errno.past_spawn() || Stage::Search.past_spawn() {
        return false;
    }
    if Stage::Word.past_spawn() || Stage::Builtin.past_spawn() || Stage::Hash.past_spawn() {
        return false;
    }
    // shell 判词只在 hash/search/errno 三段产生。
    if !Stage::Hash.shell_verdict() || !Stage::Search.shell_verdict() || !Stage::Errno.shell_verdict() {
        return false;
    }
    if Stage::Commit.shell_verdict() || Stage::Rc.shell_verdict() || Stage::Spawn.shell_verdict() {
        return false;
    }
    if Stage::Word.shell_verdict() || Stage::Builtin.shell_verdict() {
        return false;
    }
    if Stage::Commit.name() != "commit" || Stage::Word.name() != "word" {
        return false;
    }
    Stage::Search.name() == "search" && Stage::Errno.name() == "errno"
}

/// 查找答复档与目录策略：分区不重不漏、策略分流给出两个合法数字。
fn lookup_ok() -> bool {
    let ls = [
        Lookup::Builtin,
        Lookup::HashHit,
        Lookup::HashStale,
        Lookup::PathHit,
        Lookup::PathMiss,
        Lookup::DirOnPath,
        Lookup::SlashTarget,
    ];
    let mut found = 0usize;
    let mut miss = 0usize;
    let mut walk = 0usize;
    let mut i = 0usize;
    while i < ls.len() {
        let l = match ls.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if l.found() {
            found += 1;
        }
        if l.is_miss() {
            miss += 1;
        }
        if l.walks_path() {
            walk += 1;
        }
        // found 与 is_miss 必须互斥。
        if l.found() && l.is_miss() {
            return false;
        }
        i += 1;
    }
    if found != 5 || miss != 1 || walk != 3 {
        return false;
    }
    // 答复归属的闸段必须与档位语义一致。
    if Lookup::Builtin.gate() != Stage::Builtin {
        return false;
    }
    if Lookup::HashHit.gate() != Stage::Hash || Lookup::HashStale.gate() != Stage::Hash {
        return false;
    }
    if Lookup::PathMiss.gate() != Stage::Search || Lookup::DirOnPath.gate() != Stage::Search {
        return false;
    }
    if Lookup::PathHit.gate() != Stage::Search || Lookup::SlashTarget.gate() != Stage::Search {
        return false;
    }
    // 目录策略：同一天现场的两个合法数字。
    if DirPolicy::Skipped.exit_of() != EXIT_NOT_FOUND || DirPolicy::Skipped.errno_of() != CH_ENOENT {
        return false;
    }
    if DirPolicy::Candidate.exit_of() != EXIT_NOT_EXEC || DirPolicy::Candidate.errno_of() != CH_EISDIR {
        return false;
    }
    if DirPolicy::Skipped.name() == DirPolicy::Candidate.name() {
        return false;
    }
    // 用 define 复算两条路，确认策略真的分流。
    let mut skip = probe("dir-skip", "mydir", Lookup::DirOnPath);
    skip.path_dirs = 5;
    skip.dirs_scanned = 5;
    skip.dir_policy = DirPolicy::Skipped;
    let o1 = define(&skip);
    let mut cand = probe("dir-cand", "mydir", Lookup::DirOnPath);
    cand.path_dirs = 5;
    cand.dirs_scanned = 5;
    cand.dir_policy = DirPolicy::Candidate;
    cand.text = TextChan::IsDirLine;
    let o2 = define(&cand);
    if o1.value != 127 || !o1.default || o1.code != CH_ENOENT {
        return false;
    }
    if o2.value != 126 || o2.code != CH_EISDIR || o2.default {
        return false;
    }
    if o2.note != Note::IsDir126 || Tier::of(&o2) != Tier::NotExec {
        return false;
    }
    // 目录被跳过时"没扫满"必须判死，而不是提前给 127。
    let mut half = probe("dir-half", "mydir", Lookup::DirOnPath);
    half.path_dirs = 5;
    half.dirs_scanned = 3;
    half.dir_policy = DirPolicy::Skipped;
    if define(&half).note != Note::ChanSplit {
        return false;
    }
    // 词形态↔答复档互认：带斜杠的词不许走 PATH，裸名不许冒充斜杠目标。
    let mut bad = probe("slash-walk", "/bin/nope", Lookup::SlashTarget);
    bad.path_consulted = true;
    bad.dirs_scanned = 1;
    if define(&bad).note != Note::ChanSplit {
        return false;
    }
    let bad2 = probe("bare-as-slash", "nope", Lookup::SlashTarget);
    if define(&bad2).note != Note::ChanSplit {
        return false;
    }
    let bad3 = probe("slash-as-path", "/bin/nope", Lookup::PathMiss);
    if define(&bad3).note != Note::ChanSplit {
        return false;
    }
    // 命令字形态档。
    if !WordForm::Empty.is_no_command() || !WordForm::Blank.is_no_command() {
        return false;
    }
    if !WordForm::Assign.is_no_command() || WordForm::Bare.is_no_command() {
        return false;
    }
    if !WordForm::Control.is_void() || !WordForm::Overlong.is_void() {
        return false;
    }
    if WordForm::Dots.is_void() || WordForm::Slashed.consults_path() {
        return false;
    }
    if !WordForm::Bare.consults_path() {
        return false;
    }
    if word_form("") != WordForm::Empty {
        return false;
    }
    if word_form("   ") != WordForm::Blank || word_form("\t") != WordForm::Blank {
        return false;
    }
    if word_form("A=1") != WordForm::Assign || word_form("MY_VAR=x") != WordForm::Assign {
        return false;
    }
    if word_form("1A=x") != WordForm::Bare {
        return false;
    }
    if word_form("A B=1") != WordForm::Bare || word_form("/tmp/A=1") != WordForm::Slashed {
        return false;
    }
    if word_form(".") != WordForm::Dots || word_form("..") != WordForm::Dots {
        return false;
    }
    if word_form("ls") != WordForm::Bare || word_form("/bin/ls") != WordForm::Slashed {
        return false;
    }
    if word_form("./x") != WordForm::Slashed || word_form("dir/") != WordForm::Slashed {
        return false;
    }
    if word_form("foo\nbar") != WordForm::Control {
        return false;
    }
    // PATH 目录计数（闸四"扫满"这把尺的刻度）。
    if path_dir_count("") != 0 {
        return false;
    }
    if path_dir_count("/bin") != 1 {
        return false;
    }
    if path_dir_count("/bin:/usr/bin") != 2 || path_dir_count("/a:/b:/c:/d") != 4 {
        return false;
    }
    text_len("nosuchtool") == 10 && last_component("/usr/local/bin/sh") == "sh"
}

/// 八闸纵线：每段都要能被一条正牌现场定在当段，且过了 `spawn` 判词位熄灭。
fn define_ok() -> bool {
    // 闸一正例：POSIX 空命令，退 0，不查找。
    let mut g1 = probe("gate1-ok", "", Lookup::PathMiss);
    g1.text = TextChan::Silent;
    g1.exec_errno = Errno::ENone;
    let o1 = define(&g1);
    if o1.stage != Stage::Word || o1.value != EXIT_OK || o1.code != CH_ENONE {
        return false;
    }
    if o1.note != Note::EmptyWord || !o1.ok() || o1.default {
        return false;
    }
    // 闸一反例：空命令却带着 notfound 判词条——谎报，判死在闸一。
    let g1b = probe("gate1-lie", "", Lookup::PathMiss);
    let ob = define(&g1b);
    if ob.stage != Stage::Word || ob.note != Note::ChanSplit {
        return false;
    }
    if ob.value != EXIT_USAGE || ob.code != CH_EINVAL || ob.default {
        return false;
    }
    // 闸二正例：内建命中，PATH 零扫描。
    let mut g2 = probe("gate2-ok", "true", Lookup::Builtin);
    g2.path_consulted = false;
    g2.dirs_scanned = 0;
    g2.text = TextChan::Silent;
    g2.exec_errno = Errno::ENone;
    let o2 = define(&g2);
    if o2.stage != Stage::Builtin || o2.note != Note::BuiltinOk || !o2.ok() {
        return false;
    }
    if o2.default || o2.value != EXIT_OK {
        return false;
    }
    // 闸二反例：命中内建却扫了 PATH——内建必须抢先，扫了就是抢跑。
    let g2b = probe("gate2-lie", "echo", Lookup::Builtin);
    let o2b = define(&g2b);
    if o2b.stage != Stage::Builtin || o2b.note != Note::ChanSplit {
        return false;
    }
    // 闸三正例：陈旧哈希、不回找，数字定在 hash 段。
    let mut g3 = probe("gate3-ok", "gone", Lookup::HashStale);
    g3.rehash = false;
    g3.path_consulted = false;
    g3.dirs_scanned = 0;
    let o3 = define(&g3);
    if o3.stage != Stage::Hash || o3.value != EXIT_NOT_FOUND || !o3.default {
        return false;
    }
    if o3.code != CH_ENOENT || o3.note != Note::NotFound127 {
        return false;
    }
    // 闸三反例：哈希命中却还说扫了 PATH。
    let mut g3b = probe("gate3-lie", "grep", Lookup::HashHit);
    g3b.dirs_scanned = 2;
    let o3b = define(&g3b);
    if o3b.stage != Stage::Hash || o3b.note != Note::ChanSplit {
        return false;
    }
    // 闸四正例：PATH 扫尽。
    let o4 = define(&probe("gate4-ok", "nosuch", Lookup::PathMiss));
    if o4.stage != Stage::Search || !o4.default || o4.value != 127 {
        return false;
    }
    // 闸四反例：少扫一个目录就宣布尽头。
    let mut g4b = probe("gate4-lie", "nosuch", Lookup::PathMiss);
    g4b.dirs_scanned = 2;
    if define(&g4b).note != Note::ChanSplit {
        return false;
    }
    // 闸五正例：errno 归档成 126。
    let mut g5 = probe("gate5-ok", "helper", Lookup::PathHit);
    g5.path_dirs = 6;
    g5.dirs_scanned = 3;
    g5.exec_errno = Errno::Eacces;
    g5.text = TextChan::PermLine;
    let o5 = define(&g5);
    if o5.stage != Stage::Errno || o5.value != EXIT_NOT_EXEC || o5.code != CH_EACCES {
        return false;
    }
    if o5.note != Note::NotExec126 || o5.default {
        return false;
    }
    // 闸六正例（守卫位）：跑过了还想记 127，判死在 spawn 段。
    let mut g6 = probe("gate6-guard", "nosuch", Lookup::PathMiss);
    g6.spawned = true;
    let o6 = define(&g6);
    if o6.stage != Stage::Spawn || o6.note != Note::ChanSplit || o6.default {
        return false;
    }
    // 闸六反例：候选可用却没起子进程。
    let mut g6b = probe("gate6-lie", "ls", Lookup::PathHit);
    g6b.path_dirs = 4;
    g6b.dirs_scanned = 1;
    g6b.exec_errno = Errno::ENone;
    g6b.spawned = false;
    g6b.text = TextChan::Silent;
    let o6b = define(&g6b);
    if o6b.stage != Stage::Spawn || o6b.note != Note::ChanSplit {
        return false;
    }
    // 闸七正例：子进程自己退 127，归因翻面。
    let mut g7 = probe("gate7-mimic", "svc", Lookup::PathHit);
    g7.path_dirs = 4;
    g7.dirs_scanned = 2;
    g7.exec_errno = Errno::ENone;
    g7.spawned = true;
    g7.child_rc = 127;
    g7.text = TextChan::ChildBytes;
    let o7 = define(&g7);
    if o7.stage != Stage::Rc || o7.value != 127 || o7.default || o7.note != Note::ChildMimic {
        return false;
    }
    // 闸七正例：信号档 128+9。
    let mut g7s = g7;
    g7s.tag = "gate7-signal";
    g7s.signaled = true;
    g7s.sig = SIG_KILL;
    let o7s = define(&g7s);
    if o7s.stage != Stage::Rc || o7s.value != 137 || o7s.note != Note::Signaled {
        return false;
    }
    // 闸八正例：返回码原样入账。
    let mut g8 = g7;
    g8.tag = "gate8-ok";
    g8.child_rc = 3;
    g8.signaled = false;
    g8.sig = 0;
    let o8 = define(&g8);
    if o8.stage != Stage::Commit || o8.value != 3 || o8.note != Note::ChildRan || !o8.ok() {
        return false;
    }
    // 确定性：同输入两次复算逐位相同。
    let mut i = 0usize;
    let cs = cases();
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if define(&c.shape) != define(&c.shape) {
            return false;
        }
        let a = c.actual();
        if a.stage.past_spawn() && a.default {
            return false;
        }
        if a.default && a.stage.order() > Stage::Errno.order() {
            return false;
        }
        i += 1;
    }
    // 空白形状本身就是一例谎报：空词却挂着 notfound 判词条，判死在闸一。
    // 这条正是闸一存在的唯一理由——先体检词形，再谈查找。
    let b = Shape::blank();
    let ob2 = define(&b);
    if ob2.stage != Stage::Word || ob2.note != Note::ChanSplit || ob2.default {
        return false;
    }
    if ob2.value != EXIT_USAGE || ob2.code != CH_EINVAL {
        return false;
    }
    // 把词填上、判词条留着：同一形状立刻变成主判档现场。
    let mut b3 = Shape::blank();
    b3.word = "nosuchtool";
    let ob3 = define(&b3);
    ob3.stage == Stage::Search && ob3.default && ob3.value == EXIT_NOT_FOUND
}

/// 127 与 126 的分界：存在性决定数字，文案族必须跟上。
fn boundary_ok() -> bool {
    // 四种"存在但执行不了"的 errno，走带斜杠这条路，一律 126。
    let pairs = [
        (Errno::Eacces, TextChan::PermLine),
        (Errno::Eisdir, TextChan::IsDirLine),
        (Errno::Enoexec, TextChan::ExecFmtLine),
        (Errno::Eloop, TextChan::PermLine),
    ];
    let mut i = 0usize;
    while i < pairs.len() {
        let (e, tc) = match (pairs.get(i), ()) {
            (Some(p), _) => (p.0, p.1),
            _ => return false,
        };
        let mut s = probe("boundary-126", "./target", Lookup::SlashTarget);
        s.path_consulted = false;
        s.dirs_scanned = 0;
        s.exec_errno = e;
        s.text = tc;
        let o = define(&s);
        if o.value != EXIT_NOT_EXEC || o.default || o.code != e.value() {
            return false;
        }
        if !o.note.is_notexec() || Tier::of(&o) != Tier::NotExec {
            return false;
        }
        if o.stage != Stage::Errno {
            return false;
        }
        i += 1;
    }
    // ENOENT 走同一条路：127，且判词位为真。
    let mut s = probe("boundary-127", "./target", Lookup::SlashTarget);
    s.path_consulted = false;
    s.dirs_scanned = 0;
    s.exec_errno = Errno::Enoent;
    let o = define(&s);
    if o.value != EXIT_NOT_FOUND || !o.default || o.code != CH_ENOENT {
        return false;
    }
    if Tier::of(&o) != Tier::NotFound {
        return false;
    }
    // 背离：errno 说 EACCES、文案却打 notfound 族——观测作废。
    let mut s2 = probe("boundary-split", "./target", Lookup::SlashTarget);
    s2.path_consulted = false;
    s2.dirs_scanned = 0;
    s2.exec_errno = Errno::Eacces;
    s2.text = TextChan::NotFoundLine;
    if define(&s2).note != Note::ChanSplit {
        return false;
    }
    // 背离：文案打权限行、errno 却是 ENOENT。
    let mut s3 = probe("boundary-split2", "./target", Lookup::SlashTarget);
    s3.path_consulted = false;
    s3.dirs_scanned = 0;
    s3.exec_errno = Errno::Enoent;
    s3.text = TextChan::PermLine;
    if define(&s3).note != Note::ChanSplit {
        return false;
    }
    // 背离：静默的 127（把错误行当可选装饰的 shell）。
    let mut s4 = probe("boundary-silent", "nosuch", Lookup::PathMiss);
    s4.text = TextChan::Silent;
    let o4 = define(&s4);
    if o4.note != Note::ChanSplit || o4.default {
        return false;
    }
    // 背离：notfound 文案配上 126 数字（现场不可能存在，复算必须拒答）。
    let mut s5 = probe("boundary-mix", "helper", Lookup::PathHit);
    s5.path_dirs = 4;
    s5.dirs_scanned = 1;
    s5.exec_errno = Errno::Eisdir;
    s5.text = TextChan::NotFoundLine;
    if define(&s5).note != Note::ChanSplit {
        return false;
    }
    // 语料面上：127 判词永远带 ENOENT，126 判词永远带"存在"errno。
    let cs = cases();
    let mut k = 0usize;
    while k < CORPUS {
        let a = match cs.get(k) {
            Some(c) => c.actual(),
            None => return false,
        };
        if a.value == EXIT_NOT_FOUND && a.default && a.code != CH_ENOENT {
            return false;
        }
        if a.note.is_notexec() && !Errno::of(a.code).implies_exists() {
            return false;
        }
        if a.note.is_notexec() && a.value == EXIT_NOT_FOUND {
            return false;
        }
        k += 1;
    }
    // ENOEXEC 重跑这条路：数字来自那层子 shell，绝不落 126。
    let mut fb = probe("boundary-fallback", "garbage.sh", Lookup::PathHit);
    fb.path_dirs = 4;
    fb.dirs_scanned = 2;
    fb.exec_errno = Errno::Enoexec;
    fb.fallback = true;
    fb.spawned = true;
    fb.child_rc = 126;
    fb.text = TextChan::ChildBytes;
    let of = define(&fb);
    of.value == 126 && !of.default && of.note == Note::ChildNotExec && of.stage == Stage::Rc
}

/// 归因位：谁给的数，冒充档怎么处理，信号档怎么算。
fn origin_ok() -> bool {
    // 注解 → 归因 的映射逐条点名。
    let ns = [
        Note::EmptyWord,
        Note::BuiltinOk,
        Note::ChildRan,
        Note::ChildMimic,
        Note::ChildNotExec,
        Note::Signaled,
        Note::FallbackRan,
        Note::NotFound127,
        Note::NotExec126,
        Note::IsDir126,
        Note::ChanSplit,
    ];
    let mut i = 0usize;
    while i < ns.len() {
        let n = match ns.get(i) {
            Some(x) => *x,
            None => return false,
        };
        match n.origin() {
            Origin::ShellNotFound => {
                if !n.is_notfound() {
                    return false;
                }
            }
            Origin::ShellNotExec => {
                if !n.is_notexec() {
                    return false;
                }
            }
            Origin::ChildStatus => {
                if !n.ran() {
                    return false;
                }
            }
            Origin::Builtin | Origin::Empty => {
                if !n.ran() {
                    return false;
                }
            }
            Origin::Void => {
                if !n.is_void() {
                    return false;
                }
            }
        }
        i += 1;
    }
    if ns.len() != 11 {
        return false;
    }
    // ran 族与判词族互斥：同一条注解不能既是"跑过"又是 shell 判词。
    let mut j = 0usize;
    while j < ns.len() {
        let a = match ns.get(j) {
            Some(x) => *x,
            None => return false,
        };
        if a.ran() && (a.is_notfound() || a.is_notexec()) {
            return false;
        }
        j += 1;
    }
    // 子进程冒充 126：同样落外数档，判词位为假。
    let mut m = probe("origin-mimic-126", "svc", Lookup::PathHit);
    m.path_dirs = 4;
    m.dirs_scanned = 1;
    m.exec_errno = Errno::ENone;
    m.spawned = true;
    m.child_rc = 126;
    m.text = TextChan::ChildBytes;
    let om = define(&m);
    if om.note != Note::ChildNotExec || om.default || Tier::of(&om) != Tier::Foreign {
        return false;
    }
    if !om.ok() || om.origin() != Origin::ChildStatus {
        return false;
    }
    // 信号档算术：128+signum，非法信号作废。
    let sigs = [1u8, 2, 3, 9, 11, 13, 15, 64];
    let mut k = 0usize;
    while k < sigs.len() {
        let sg = match sigs.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let mut p = m;
        p.tag = "origin-signal";
        p.child_rc = 0;
        p.signaled = true;
        p.sig = sg;
        let o = define(&p);
        let want = match EXIT_SIGNAL_BASE.checked_add(sg as u16) {
            Some(v) => v,
            None => return false,
        };
        if o.value != want || o.note != Note::Signaled || o.default {
            return false;
        }
        if Tier::of(&o) != Tier::Foreign {
            return false;
        }
        k += 1;
    }
    let mut bad0 = m;
    bad0.tag = "origin-sig-zero";
    bad0.signaled = true;
    bad0.sig = 0;
    if define(&bad0).stage != Stage::Rc || define(&bad0).note != Note::ChanSplit {
        return false;
    }
    let mut bign = m;
    bign.tag = "origin-sig-over";
    bign.signaled = true;
    bign.sig = 65;
    if define(&bign).note != Note::ChanSplit {
        return false;
    }
    // 被打死的进程不许同时带 shell 判词条。
    let mut killd = m;
    killd.tag = "origin-sig-verify";
    killd.signaled = true;
    killd.sig = SIG_KILL;
    killd.text = TextChan::PermLine;
    if define(&killd).stage != Stage::Spawn {
        return false;
    }
    // 内建返回值不是 shell 判词，也不是子进程状态。
    let mut bi = probe("origin-builtin", "false", Lookup::Builtin);
    bi.path_consulted = false;
    bi.dirs_scanned = 0;
    bi.exec_errno = Errno::ENone;
    bi.text = TextChan::Silent;
    bi.builtin_rc = 1;
    let bo = define(&bi);
    if bo.value != 1 || bo.origin() != Origin::Builtin || Tier::of(&bo) != Tier::Foreign {
        return false;
    }
    // ENOEXEC 不重跑时是 126；重跑时数字归子 shell。
    let mut ne = probe("origin-noexec", "junk", Lookup::PathHit);
    ne.path_dirs = 4;
    ne.dirs_scanned = 1;
    ne.exec_errno = Errno::Enoexec;
    ne.text = TextChan::ExecFmtLine;
    let no = define(&ne);
    if no.value != EXIT_NOT_EXEC || no.origin() != Origin::ShellNotExec {
        return false;
    }
    ne.fallback = true;
    ne.spawned = true;
    ne.child_rc = 0;
    ne.text = TextChan::Silent;
    ne.tag = "origin-noexec-fallback";
    let nf = define(&ne);
    if nf.note != Note::FallbackRan || nf.origin() != Origin::ChildStatus || !nf.ok() {
        return false;
    }
    // 归因档自身的性质。
    if !Origin::ShellNotFound.is_shell() || !Origin::ShellNotExec.is_shell() {
        return false;
    }
    if Origin::ChildStatus.is_shell() || Origin::Void.is_shell() || Origin::Builtin.is_shell() {
        return false;
    }
    if !Origin::ShellNotFound.is_notfound_verdict() || Origin::ShellNotExec.is_notfound_verdict() {
        return false;
    }
    Origin::ChildStatus.is_notfound_verdict() == false && Origin::Empty.name() == "empty-command"
}

/// 三通道一致：数/文案/errno 同真才算 127 档绿。
fn chan_ok() -> bool {
    // 判词位为真 ⟹ 四个事实同真。
    let mut s = probe("chan-verdict", "nosuchtool", Lookup::PathMiss);
    s.path_dirs = 4;
    s.dirs_scanned = 4;
    s.exec_errno = Errno::Enoent;
    s.text = TextChan::NotFoundLine;
    s.spawned = false;
    let o = define(&s);
    if !o.channels_agree() || !o.default {
        return false;
    }
    if o.value != EXIT_NOT_FOUND || o.code != CH_ENOENT || o.note != Note::NotFound127 {
        return false;
    }
    if s.spawned {
        return false;
    }
    if msg_channel(&o, s.spawned) != VERDICT_127_LIT {
        return false;
    }
    let live4 = match CLAIMS.get(4) {
        Some(x) => *x,
        None => return false,
    };
    if channel_fault(&Case { shape: s, claim: live4 }) != Fault::Ok {
        return false;
    }
    // 两个锚点字面量必须不同（数字相同、归因相反）。
    if same_text(VERDICT_127_LIT, MIMIC_127_LIT) {
        return false;
    }
    if !has_byte(VERDICT_127_LIT, b'=') || !find_from(MIMIC_127_LIT, "spawned=yes", 0).is_some() {
        return false;
    }
    // 作废档的三通道永远不自洽。
    let mut bad = probe("chan-split", "nosuch", Lookup::PathMiss);
    bad.text = TextChan::Silent;
    let ob = define(&bad);
    if ob.channels_agree() || !ob.note.is_void() {
        return false;
    }
    if channel_fault(&Case { shape: bad, claim: Claim::blank() }) != Fault::Channel {
        return false;
    }
    // 逐槽：现场自洽 ⟺ 非作废。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let a = c.actual();
        if a.channels_agree() == a.note.is_void() {
            return false;
        }
        if i < ACCEPT && !a.channels_agree() {
            return false;
        }
        if i >= ACCEPT && !a.channels_agree() {
            return false;
        }
        i += 1;
    }
    // 活格的申报自身也必须三通道自洽（红档是"现场对、申报歪"，
    // 不是"现场本来就歪"）。
    let mut k = 0usize;
    while k < ACCEPT {
        let cl = match CLAIMS.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if !cl.self_consistent() {
            return false;
        }
        if channel_fault(&Case { shape: match SHAPES.get(k) { Some(x) => *x, None => return false }, claim: cl })
            != Fault::Ok
        {
            return false;
        }
        k += 1;
    }
    // 判词位只在 shell 判词三段出现。
    let mut t = 0usize;
    while t < CORPUS {
        let c = match cs.get(t) {
            Some(x) => *x,
            None => return false,
        };
        let a = c.actual();
        if a.default && !a.stage.shell_verdict() {
            return false;
        }
        t += 1;
    }
    text_len(VERDICT_127_LIT) == 32 && text_len(MIMIC_127_LIT) == 28
}

/// 申报六维对判：判序固定，且先到的缺陷吃掉后到的缺陷。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want = match expected(i) {
            Some(x) => x,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        if c.ok() != (want == Fault::Ok) {
            return false;
        }
        i += 1;
    }
    if cs.len() != CORPUS {
        return false;
    }
    // 判序：Estate 先于一切。
    let a = Case {
        shape: match SHAPES.get(4) {
            Some(x) => *x,
            None => return false,
        },
        claim: Claim {
            stage: Stage::Rc,
            code: CH_ENONE,
            value: 0,
            note: Note::ChildRan,
            default: false,
        },
    };
    if judge(&a) != Fault::Estate {
        return false;
    }
    // 判序：Stage 先于 Code/Value/Note/Default。
    let b = Case {
        shape: a.shape,
        claim: Claim {
            stage: Stage::Hash,
            code: CH_EACCES,
            value: 126,
            note: Note::NotExec126,
            default: false,
        },
    };
    if judge(&b) != Fault::Stage {
        return false;
    }
    // 判序：Code 先于 Value。
    let c2 = Case {
        shape: a.shape,
        claim: Claim {
            stage: Stage::Search,
            code: CH_EACCES,
            value: 126,
            note: Note::NotFound127,
            default: true,
        },
    };
    if judge(&c2) != Fault::Code {
        return false;
    }
    // 判序：Value 先于 Note，Note 先于 Default。
    let d = Case {
        shape: a.shape,
        claim: Claim {
            stage: Stage::Search,
            code: CH_ENOENT,
            value: 126,
            note: Note::NotExec126,
            default: false,
        },
    };
    if judge(&d) != Fault::Value {
        return false;
    }
    let e = Case {
        shape: a.shape,
        claim: Claim {
            stage: Stage::Search,
            code: CH_ENOENT,
            value: 127,
            note: Note::IsDir126,
            default: false,
        },
    };
    if judge(&e) != Fault::Note {
        return false;
    }
    let f = Case {
        shape: a.shape,
        claim: Claim {
            stage: Stage::Search,
            code: CH_ENOENT,
            value: 127,
            note: Note::NotFound127,
            default: false,
        },
    };
    if judge(&f) != Fault::Default {
        return false;
    }
    // 全对即 Ok。
    let g = Case {
        shape: a.shape,
        claim: match CLAIMS.get(4) {
            Some(x) => *x,
            None => return false,
        },
    };
    if judge(&g) != Fault::Ok || !g.ok() {
        return false;
    }
    // 六缺陷码互不相同且落在段内。
    let mut k = 0usize;
    while k < REJECT {
        let fc = match REJECT_FAULTS.get(k) {
            Some(x) => x.code(),
            None => return false,
        };
        if fc < SEG_BASE + 1 || fc > SEG_BASE + 0x0d {
            return false;
        }
        if !fc.alive_bit() {
            return false;
        }
        let mut m = k + 1;
        while m < REJECT {
            if let Some(g2) = REJECT_FAULTS.get(m) {
                if fc == g2.code() {
                    return false;
                }
            }
            m += 1;
        }
        k += 1;
    }
    if REJECT_CODES.len() != REJECT || REJECT_FAULTS.len() != REJECT {
        return false;
    }
    let mut z = 0usize;
    while z < REJECT {
        let want = match REJECT_FAULTS.get(z) {
            Some(x) => x.code(),
            None => return false,
        };
        match REJECT_CODES.get(z) {
            Some(v) => {
                if *v != want {
                    return false;
                }
            }
            None => return false,
        }
        z += 1;
    }
    fault_short(Fault::Ok) == "ok" && fault_short(Fault::Channel) == "channel"
}

/// 小工具：缺陷码是否非零（`Fault::code()` 对 Ok 返回 0）。
trait AliveBit {
    fn alive_bit(&self) -> bool;
}

impl AliveBit for u32 {
    fn alive_bit(&self) -> bool {
        *self != 0
    }
}

// ---------------------------------------------------------------------------
// 维号（把"坏在哪一维"用第三种形式再复写一遍）
// ---------------------------------------------------------------------------

/// 每槽坏掉的申报维号；`CAP`（6）表示该槽申报无缺陷。
///
/// 这是 `expected()` 的**第三种形式**：第一种是 `Fault` 枚举、第二种是
/// `WANT_FAULT` 的短名串，这里直接给维号。三者各自独立成立，于是
/// "六个死槽一槽一维、且严格按 Estate→Stage→Code→Value→Note→Default 排"
/// 这条纪律不可能靠改一处就糊过去。
const WANT_NDX: [usize; CORPUS] = [
    CAP,
    CAP,
    CAP,
    CAP,
    CAP,
    CAP,
    CAP,
    CAP,
    0usize,
    1,
    2,
    3,
    4,
    5,
];

/// 缺陷档 → 申报维号（判序那把尺的显式刻度）。
fn fault_ndx(f: Fault) -> usize {
    match f {
        Fault::Estate => 0usize,
        Fault::Stage => 1usize,
        Fault::Code => 2usize,
        Fault::Value => 3usize,
        Fault::Note => 4usize,
        Fault::Default => 5usize,
        _ => CAP,
    }
}

/// 申报维号 → 维名（与 `Fault::name()` 的六个申报档逐字对齐）。
fn dim_name(ndx: usize) -> &'static str {
    match ndx {
        0usize => "estate",
        1usize => "stage",
        2usize => "errno",
        3usize => "exit",
        4usize => "note",
        5usize => "verdict",
        _ => "none",
    }
}

/// 申报与实际**逐维**比对，返回不同的维数。
///
/// 本卡要求六个死槽里除状态维（那个谎是整句话翻面，五维全歪）以外
/// 各只坏一维；这个函数就是"只坏一维"的尺子。
fn dims_differ(c: &Case) -> usize {
    let a = c.actual();
    let k = c.claim;
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

/// 第一个不同的维号（判序的第二种实现，用来交叉校验 `judge`）。
fn first_diff(c: &Case) -> usize {
    let a = c.actual();
    let k = c.claim;
    if a.ok() != k.success() {
        return 0usize;
    }
    if a.stage != k.stage {
        return 1usize;
    }
    if a.code != k.code {
        return 2usize;
    }
    if a.value != k.value {
        return 3usize;
    }
    if a.note != k.note {
        return 4usize;
    }
    if a.default != k.default {
        return 5usize;
    }
    CAP
}

/// 四格布尔是否全真（不用下标直取，逐格 `get`）。
fn four_true(f: [bool; 4]) -> bool {
    let mut i = 0usize;
    while i < 4 {
        match f.get(i) {
            Some(v) => {
                if !*v {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 串里换行数（逐字节扫，账本行数与矩阵行数的尺子）。
fn newline_count(s: &str) -> usize {
    let mut n = 0usize;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'\n' {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 判据 10：语料表与死槽纪律
// ---------------------------------------------------------------------------

/// 语料自洽：标签唯一且与期望表逐字对齐、活 8 死 6、死槽一槽一维并按序。
fn corpus_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS {
        return false;
    }
    if cover(CORPUS) != Ok(()) {
        return false;
    }
    if cover(CORPUS - 1) == Ok(()) {
        return false;
    }
    if case_at(CORPUS).is_some() {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c.shape.tag.is_empty() {
            return false;
        }
        if c.shape.word.is_empty() && i >= ACCEPT {
            // 死槽必须是一个真实的查找现场，空词在闸一就出去了。
            return false;
        }
        if text_len(c.shape.word) > WORD_MAX {
            return false;
        }
        let w = match WANT_TAGS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !same_text(c.shape.tag, w) {
            return false;
        }
        let mut j = i + 1;
        while j < CORPUS {
            if let Some(d) = cs.get(j) {
                if same_text(c.shape.tag, d.shape.tag) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 活槽全对；死槽恰坏一个申报维，且缺陷档就是声称那一档。
    let mut live = 0usize;
    let mut dead = 0usize;
    i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let f = judge(&c);
        let want = match expected(i) {
            Some(x) => x,
            None => return false,
        };
        if f != want {
            return false;
        }
        if i < ACCEPT {
            if f != Fault::Ok || !c.ok() {
                return false;
            }
            if dims_differ(&c) != 0 {
                return false;
            }
            live += 1;
        } else {
            if f == Fault::Ok || !f.is_claim_fault() {
                return false;
            }
            let ndx = i - ACCEPT;
            if first_diff(&c) != ndx || fault_ndx(f) != ndx {
                return false;
            }
            // estate 那一槽是整句翻面（五维全歪）；其余五槽只坏自己那一维。
            let want_dims = if ndx == 0usize { 5usize } else { 1usize };
            if dims_differ(&c) != want_dims {
                return false;
            }
            dead += 1;
        }
        i += 1;
    }
    if live != ACCEPT || dead != REJECT {
        return false;
    }
    // 六个缺陷维各用一次，中文维名与短名表两两对齐。
    let mut seen = [false; 6usize];
    i = 0usize;
    while i < REJECT {
        let f = match REJECT_FAULTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let ndx = fault_ndx(f);
        if ndx >= CAP {
            return false;
        }
        if let Some(slot) = seen.get_mut(ndx) {
            if *slot {
                return false;
            }
            *slot = true;
        } else {
            return false;
        }
        let t = match REJECT_DIM_TEXT.get(i) {
            Some(x) => *x,
            None => return false,
        };
        // 每条说明都必须是"维名：一句话"，全角冒号的 UTF-8 首字节 0xEF。
        if t.is_empty() || !has_byte(t, 0xefu8) || !has_byte(t, 0xbcu8) || !has_byte(t, 0x9au8) {
            return false;
        }
        match WANT_FAULT.get(i + ACCEPT) {
            Some(x) => {
                if !same_text(x, dim_name(ndx)) || !same_text(x, fault_short(f)) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    // 六条中文维说明互不相同（红档单子的第二种形式）。
    i = 0usize;
    while i < REJECT {
        let a = match REJECT_DIM_TEXT.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < REJECT {
            if let Some(b) = REJECT_DIM_TEXT.get(j) {
                if same_text(a, b) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 槽位表：成格与败格互斥、并起来恰是全 corpus。
    let mut acc = [false; CORPUS];
    let mut p = 0usize;
    while p < ACCEPT {
        if let Some(v) = ACCEPT_SLOTS.get(p) {
            if *v >= CORPUS {
                return false;
            }
            if let Some(cell) = acc.get_mut(*v) {
                if *cell {
                    return false;
                }
                *cell = true;
            } else {
                return false;
            }
        } else {
            return false;
        }
        p += 1;
    }
    p = 0usize;
    while p < REJECT {
        if let Some(v) = REJECT_SLOTS.get(p) {
            if *v >= CORPUS {
                return false;
            }
            if let Some(cell) = acc.get_mut(*v) {
                if *cell {
                    return false;
                }
                *cell = true;
            } else {
                return false;
            }
        } else {
            return false;
        }
        p += 1;
    }
    let mut q = 0usize;
    let mut filled = 0usize;
    while q < CORPUS {
        if let Some(cell) = acc.get(q) {
            if *cell {
                filled += 1;
            }
        }
        q += 1;
    }
    filled == CORPUS
}

// ---------------------------------------------------------------------------
// 判据 10（形式二）：期望表复写与申报表交叉核
// ---------------------------------------------------------------------------

/// 六张期望表把 `CLAIMS` 用第二种形式复写一遍，任一处笔误都要露出来。
fn slots_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let k = c.claim;
        if let Some(w) = WANT_STAGE.get(i) {
            if *w != k.stage {
                return false;
            }
        } else {
            return false;
        }
        if let Some(w) = WANT_CODE.get(i) {
            if *w != k.code {
                return false;
            }
        } else {
            return false;
        }
        if let Some(w) = WANT_VALUE.get(i) {
            if *w != k.value {
                return false;
            }
        } else {
            return false;
        }
        if let Some(w) = WANT_NOTE.get(i) {
            if *w != k.note {
                return false;
            }
        } else {
            return false;
        }
        if let Some(w) = WANT_DEFAULT.get(i) {
            if *w != k.default {
                return false;
            }
        } else {
            return false;
        }
        if let Some(w) = WANT_NDX.get(i) {
            if *w != first_diff(&c) {
                return false;
            }
        } else {
            return false;
        }
        // 归因维：期望串必须等于申报 note 的投影（不是实际 note 的投影）。
        if let Some(w) = WANT_ORIGIN.get(i) {
            if !same_text(w, k.origin().name()) {
                return false;
            }
        } else {
            return false;
        }
        // 档位维：期望档由实际结果复算，与申报无关。
        if let Some(w) = WANT_TIER.get(i) {
            if *w != Tier::of(&c.actual()) {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    // 三种形式的缺陷表必须同时成立。
    i = ACCEPT;
    while i < CORPUS {
        let f = match expected(i) {
            Some(x) => x,
            None => return false,
        };
        if let Some(s) = WANT_FAULT.get(i) {
            if !same_text(fault_short(f), s) {
                return false;
            }
        } else {
            return false;
        }
        if let Some(n) = WANT_NDX.get(i) {
            if *n != fault_ndx(f) {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    // 活槽的期望缺陷全为 ok，且期望串与实际短名互斥。
    let mut p = 0usize;
    while p < ACCEPT {
        match WANT_FAULT.get(p) {
            Some(x) => {
                if !same_text(x, "ok") {
                    return false;
                }
            }
            None => return false,
        }
        p += 1;
    }
    if expected(CORPUS).is_some() {
        return false;
    }
    if fault_ndx(Fault::Ok) != CAP || fault_ndx(Fault::Channel) != CAP {
        return false;
    }
    if !same_text(dim_name(CAP), "none") || !same_text(dim_name(0usize), "estate") {
        return false;
    }
    // 维号回查：dim_name(fault_ndx(f)) 恒等于 f.name()。
    let mut m = 0usize;
    while m < REJECT {
        if let Some(f) = REJECT_FAULTS.get(m) {
            if !same_text(dim_name(fault_ndx(*f)), f.name()) {
                return false;
            }
        } else {
            return false;
        }
        m += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 判据 11：横计矩阵
// ---------------------------------------------------------------------------

/// `Row`/`Matrix` 的算术与真语料分布。
fn matrix_ok() -> bool {
    // Row 算术：空档不算全对；红档不得为负。
    let r0 = Row::blank();
    if r0.perfect() || r0.red() != 0 {
        return false;
    }
    let r1 = Row { total: 4usize, good: 4usize };
    if !r1.perfect() || r1.red() != 0 {
        return false;
    }
    let r2 = Row { total: 8usize, good: 2usize };
    if r2.perfect() || r2.red() != 6 {
        return false;
    }
    let r3 = Row { total: 2usize, good: 5usize };
    if r3.red() != 2usize {
        return false;
    }
    if same_text(r3.render(Tier::NotFound).as_str(), r2.render(Tier::NotFound).as_str()) {
        return false;
    }
    if !same_text(r2.render(Tier::NotFound).as_str(), "notfound total=8 good=2 red=6\n") {
        return false;
    }
    if !same_text(r1.render(Tier::Run).as_str(), "run total=4 good=4 red=0\n") {
        return false;
    }
    // 真语料矩阵。
    let m = Matrix::of();
    if m.slots() != CORPUS {
        return false;
    }
    if m.goods() != ACCEPT {
        return false;
    }
    if m.reds() != REJECT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    if m.perfect_rows() != 3usize {
        return false;
    }
    if m.verdict_reds() != REJECT {
        return false;
    }
    let want = [
        (Tier::Run, 4usize, 4usize),
        (Tier::NotFound, 8usize, 2usize),
        (Tier::NotExec, 1usize, 1usize),
        (Tier::Foreign, 1usize, 1usize),
    ];
    let mut i = 0usize;
    while i < want.len() {
        let (t, tot, good) = match want.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let r = match m.row(t) {
            Some(x) => x,
            None => return false,
        };
        if r.total != tot || r.good != good || r.red() != tot - good {
            return false;
        }
        i += 1;
    }
    // 渲染逐字对齐常量锚。
    if !same_text(m.render().as_str(), MATRIX_LIT) {
        return false;
    }
    if newline_count(MATRIX_LIT) != TIER_COUNT {
        return false;
    }
    // 空矩阵：不平、无全对档、红档为零。
    let e = Matrix::blank();
    if e.covered() || e.perfect_rows() != 0 || e.slots() != 0 {
        return false;
    }
    if e.row(Tier::NotFound) != Some(Row::blank()) {
        return false;
    }
    // 分档只看实际结果这一条：判词位排第一。
    let mut fake = Outcome::blank();
    fake.value = EXIT_NOT_EXEC;
    fake.note = Note::NotExec126;
    fake.code = CH_EACCES;
    if Tier::of(&fake) != Tier::NotFound {
        return false;
    }
    fake.default = false;
    if Tier::of(&fake) != Tier::NotExec {
        return false;
    }
    // 子进程冒充的 127 绝不进正牌档（本卡不可倒的那条序）。
    let cs = cases();
    let mut k = 0usize;
    while k < CORPUS {
        let c = match cs.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let a = c.actual();
        let t = Tier::of(&a);
        if a.value == EXIT_NOT_FOUND && !a.default {
            if t == Tier::NotFound {
                return false;
            }
        }
        if a.note == Note::ChildMimic && t != Tier::Foreign {
            return false;
        }
        if t.success() != (a.value == EXIT_OK && a.ok()) {
            return false;
        }
        if t.shell_face() && !a.stage.shell_verdict() {
            return false;
        }
        k += 1;
    }
    // 四档索引闭合且文本互异。
    let tiers = [Tier::Run, Tier::NotFound, Tier::NotExec, Tier::Foreign];
    let mut z = 0usize;
    while z < tiers.len() {
        let t = match tiers.get(z) {
            Some(x) => *x,
            None => return false,
        };
        if t.index() != z {
            return false;
        }
        let mut y = z + 1;
        while y < tiers.len() {
            if let Some(o) = tiers.get(y) {
                if same_text(t.name(), o.name()) {
                    return false;
                }
            }
            y += 1;
        }
        z += 1;
    }
    tiers.len() == TIER_COUNT
}

// ---------------------------------------------------------------------------
// 判据 11（红档显式）：报表与注入
// ---------------------------------------------------------------------------

/// 汇总、注入、红档率与越界槽。
fn reds_ok() -> bool {
    let r = match run() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if r.total != CORPUS as u32 || r.accept != ACCEPT as u32 || r.reject != REJECT as u32 {
        return false;
    }
    if r.run != 4 || r.notfound != 2 || r.notexec != 1 || r.foreign != 1 {
        return false;
    }
    if r.matched != CORPUS as u32 || r.rate() != 1000 {
        return false;
    }
    if !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.success_cells() != 4 || r.verdict_cells() != 3 {
        return false;
    }
    if r.verdict_share() != 37 {
        return false;
    }
    if r.accept.saturating_add(r.reject) != r.total {
        return false;
    }
    // 六个红档全挤在 127 档里。
    let m = Matrix::of();
    if m.verdict_reds() != REJECT || m.row(Tier::Run) != Some(Row { total: 4, good: 4 }) {
        return false;
    }
    // 注入一槽：matched 必掉、rate 必离开 1000、gate 必红。
    let mut i = 0usize;
    while i < CORPUS {
        let inj = match run_with(Some((i, Fault::Slot))) {
            Ok(x) => x,
            Err(_) => return false,
        };
        if inj.matched != CORPUS as u32 - 1 {
            return false;
        }
        if inj.rate() != 929 || inj.green() || inj.gate() == Ok(()) {
            return false;
        }
        i += 1;
    }
    if red_rate() != 929 {
        return false;
    }
    // 注入的槽号若越界，必须报槽位错，而不是悄悄全绿。
    match run_with(Some((CORPUS, Fault::Stage))) {
        Err(c) => {
            if c != EX_ESLOT {
                return false;
            }
        }
        Ok(_) => return false,
    }
    match run_with(Some((99usize, Fault::Note))) {
        Err(c) => {
            if c != EX_ESLOT {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 把死槽的缺陷抹平也必须掉分（期望表本身在被测面里）：
    // 强行宣称第 9 槽没有缺陷，matched 掉一格、成格反倒多一格。
    let flat = match run_with(Some((ACCEPT, Fault::Ok))) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if flat.matched != CORPUS as u32 - 1 {
        return false;
    }
    if flat.accept != ACCEPT as u32 + 1 || flat.reject != REJECT as u32 - 1 {
        return false;
    }
    if flat.notfound != 3 || flat.green() || flat.gate() == Ok(()) {
        return false;
    }
    // 零总数报表不得除零。
    let z = Report {
        total: 0,
        accept: 0,
        reject: 0,
        run: 0,
        notfound: 0,
        notexec: 0,
        foreign: 0,
        matched: 0,
    };
    if z.rate() != 0 || z.green() || z.gate() != Err(EX_ECOUNT) {
        return false;
    }
    if z.verdict_share() != 0 {
        return false;
    }
    // 四档计数相加必须等于成格数。
    let sum = r.run.saturating_add(r.notfound).saturating_add(r.notexec).saturating_add(r.foreign);
    sum == r.accept && r.accept == ACCEPT as u32
}

// ---------------------------------------------------------------------------
// 判据 12：状态面渲染（数字域，逐字锚）
// ---------------------------------------------------------------------------

/// `msg_*` 与 `message` 系列的逐字文本。
///
/// 这里出现的只有**数字与 errno 的官方名字**，一行 shell 文案都没有——
/// 那是 UNX-F11665 的判据面。
fn message_ok() -> bool {
    // glibc strerror 原文。
    let strs = [
        (Errno::ENone, "Success"),
        (Errno::Enoent, "No such file or directory"),
        (Errno::Eacces, "Permission denied"),
        (Errno::Eisdir, "Is a directory"),
        (Errno::Enoexec, "Exec format error"),
        (Errno::Eloop, "Too many levels of symbolic links"),
        (Errno::Einval, "Invalid argument"),
    ];
    let mut i = 0usize;
    while i < strs.len() {
        let (e, w) = match strs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !same_text(msg_strerror(e).as_str(), w) {
            return false;
        }
        if text_len(w) < 4 {
            return false;
        }
        i += 1;
    }
    // busybox 小写理由尾。
    let tags = [
        (Errno::ENone, "success"),
        (Errno::Enoent, "no such file"),
        (Errno::Eacces, "permission denied"),
        (Errno::Eisdir, "is a directory"),
        (Errno::Enoexec, "exec format error"),
        (Errno::Eloop, "too many links"),
        (Errno::Einval, "invalid argument"),
    ];
    i = 0usize;
    while i < tags.len() {
        let (e, w) = match tags.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !same_text(msg_reason_tag(e), w) {
            return false;
        }
        i += 1;
    }
    // `echo $?` 的载体：十进制加换行，位数不许错。
    if !same_text(msg_dollar_q(EXIT_NOT_FOUND).as_str(), "127\n") {
        return false;
    }
    if text_len(msg_dollar_q(EXIT_NOT_FOUND).as_str()) != 4 {
        return false;
    }
    if !same_text(msg_dollar_q(EXIT_OK).as_str(), "0\n") {
        return false;
    }
    if !same_text(msg_dollar_q(137).as_str(), "137\n") {
        return false;
    }
    // POSIX 档族归属。
    let fams = [
        (0u16, "success"),
        (1u16, "program"),
        (EXIT_PROGRAM_MAX, "program"),
        (125u16, "program"),
        (EXIT_NOT_EXEC, "reserved-not-executable"),
        (EXIT_NOT_FOUND, "reserved-not-found"),
        (EXIT_SIGNAL_BASE, "out-of-band"),
        (137u16, "signal"),
        (192u16, "signal"),
        (193u16, "out-of-band"),
        (600u16, "out-of-band"),
    ];
    i = 0usize;
    while i < fams.len() {
        let (v, w) = match fams.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !same_text(msg_exit_family(v), w) {
            return false;
        }
        i += 1;
    }
    // 预留档探测：126/127/128+signum 是预留，125 与 200 不是。
    if !shell_reserved(EXIT_NOT_FOUND) || !shell_reserved(EXIT_NOT_EXEC) {
        return false;
    }
    if !shell_reserved(137) || !shell_reserved(192) {
        return false;
    }
    if shell_reserved(EXIT_PROGRAM_MAX) || shell_reserved(193) || shell_reserved(0) {
        return false;
    }
    // 信号名表：未知号绝不冒充已知名。
    if !same_text(sig_name(SIG_KILL), "SIGKILL") || !same_text(sig_name(SIG_SEGV), "SIGSEGV") {
        return false;
    }
    if !same_text(sig_name(SIG_TERM), "SIGTERM") || !same_text(sig_name(1), "SIGHUP") {
        return false;
    }
    if !same_text(sig_name(0), "SIG?") || !same_text(sig_name(16), "SIG?") {
        return false;
    }
    if !same_text(sig_name(200), "SIG?") {
        return false;
    }
    if !same_text(msg_signal_line(SIG_KILL).as_str(), "128 + 9 = 137 (SIGKILL)") {
        return false;
    }
    if !same_text(msg_signal_line(SIG_SEGV).as_str(), "128 + 11 = 139 (SIGSEGV)") {
        return false;
    }
    if text_len(msg_signal_line(SIG_TERM).as_str()) != 24 {
        return false;
    }
    // 信号越界的现场在推演里就作废，而不是渲染出一个假信号名。
    let mut bad = probe("sig-over", "svc", Lookup::PathHit);
    bad.path_dirs = 4;
    bad.dirs_scanned = 2;
    bad.spawned = true;
    bad.text = TextChan::ChildBytes;
    bad.signaled = true;
    bad.sig = SIG_MAX + 1;
    if define(&bad).note != Note::ChanSplit {
        return false;
    }
    // 主状态行逐字（正牌 127 现场）。
    let cs = cases();
    let c4 = match cs.get(4usize) {
        Some(x) => *x,
        None => return false,
    };
    if !same_text(
        message(&c4.shape, &c4.actual()).as_str(),
        "nosuchtool stage=search errno=ENOENT exit=127 note=verdict-127 origin=shell-notfound",
    ) {
        return false;
    }
    if !same_text(message_tail(&c4.shape).as_str(), "sh/bare dirs=4/4") {
        return false;
    }
    // 冒充 127 的那一槽：数字相同，origin 必须反着写。
    let c7 = match cs.get(7usize) {
        Some(x) => *x,
        None => return false,
    };
    if !same_text(
        message(&c7.shape, &c7.actual()).as_str(),
        "init-script stage=rc errno=0 exit=127 note=child-mimic-127 origin=child-status",
    ) {
        return false;
    }
    if !same_text(message_tail(&c7.shape).as_str(), "sh/bare dirs=2/4") {
        return false;
    }
    if msg_channel(&c7.actual(), c7.shape.spawned) != MIMIC_127_LIT {
        return false;
    }
    if msg_channel(&c4.actual(), c4.shape.spawned) != VERDICT_127_LIT {
        return false;
    }
    // 目录分流：同一个 mydir 现场，两条策略给出两行数。
    let mut skip = probe("dir-skip", "mydir", Lookup::DirOnPath);
    skip.path_dirs = 5;
    skip.dirs_scanned = 5;
    skip.dir_policy = DirPolicy::Skipped;
    let mut cand = skip;
    cand.tag = "dir-cand";
    cand.dir_policy = DirPolicy::Candidate;
    cand.dirs_scanned = 2;
    cand.text = TextChan::IsDirLine;
    let os = define(&skip);
    let oc = define(&cand);
    if os.value != EXIT_NOT_FOUND || oc.value != EXIT_NOT_EXEC {
        return false;
    }
    if !same_text(Errno::of(os.code).name(), "ENOENT") {
        return false;
    }
    if !same_text(Errno::of(oc.code).name(), "EISDIR") {
        return false;
    }
    if !same_text(dec_text(DirPolicy::Skipped.exit_of() as u32).as_str(), "127") {
        return false;
    }
    if !same_text(dec_text(DirPolicy::Candidate.exit_of() as u32).as_str(), "126") {
        return false;
    }
    if DirPolicy::Skipped.errno_of() != CH_ENOENT || DirPolicy::Candidate.errno_of() != CH_EISDIR {
        return false;
    }
    if same_text(skip.dir_policy.name(), cand.dir_policy.name()) {
        return false;
    }
    // 数字文本基元本身要站得住（否则上面每一行都不可信）。
    if !same_text(u32_text(0).as_str(), "0") || !same_text(u32_text(929).as_str(), "929") {
        return false;
    }
    if !same_text(i32_text(0).as_str(), "0") || !same_text(i32_text(-2).as_str(), "-2") {
        return false;
    }
    if !same_text(hex_text(EX_ESTAGE, 5).as_str(), "6d1a1") {
        return false;
    }
    if !same_text(bool_text(true), "true") || !same_text(bool_text(false), "false") {
        return false;
    }
    if !same_text(dec_text(100000).as_str(), "100000") {
        return false;
    }
    u32_text(u32::MAX) == "4294967295"
}

// ---------------------------------------------------------------------------
// 判据 12（尾）：逐槽释义与全绿文本
// ---------------------------------------------------------------------------

/// `detail`/`detail_tail`/`Report::render` 的逐字形态。
fn detail_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    let mut lines = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let d = detail(&c);
        if !starts_with(d.as_str(), c.shape.tag) {
            return false;
        }
        if !has_byte(d.as_str(), b'=') {
            return false;
        }
        let f = judge(&c);
        let short = fault_short(f);
        if find_from(d.as_str(), "fault=", 0).is_none() {
            return false;
        }
        let tail = detail_tail(&c);
        if !starts_with(tail.as_str(), "[$?] ") {
            return false;
        }
        let a = c.actual();
        let dq = msg_dollar_q(a.value);
        // 尾行 = "[$?] " + `echo $?` 的原样字节：前缀与数字两半都要对上。
        if text_len(tail.as_str()) != 5 + text_len(dq.as_str()) {
            return false;
        }
        if find_from(tail.as_str(), dq.as_str(), 5).is_none() {
            return false;
        }
        if find_from(d.as_str(), short, 0).is_none() {
            return false;
        }
        if i < ACCEPT && !same_text(short, "ok") {
            return false;
        }
        lines += 1;
        i += 1;
    }
    if lines != CORPUS {
        return false;
    }
    // 尾行的数字与状态行的数字必须同源（两形一致才谈得上交叉校验）。
    if let Some(c) = cs.get(4usize) {
        if !same_text(detail_tail(c).as_str(), "[$?] 127\n") {
            return false;
        }
        if find_from(detail(c).as_str(), "claim=search:ENOENT:127", 0).is_none() {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c) = cs.get(11usize) {
        if find_from(detail(c).as_str(), "fault=exit", 0).is_none() {
            return false;
        }
        if find_from(detail(c).as_str(), "claim=search:ENOENT:126", 0).is_none() {
            return false;
        }
    } else {
        return false;
    }
    // 全绿渲染文本逐字钉住。
    let r = match run() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if !same_text(r.render().as_str(), RENDER_OK) {
        return false;
    }
    if newline_count(RENDER_OK) != 1usize {
        return false;
    }
    if !has_byte(RENDER_OK, b'=') {
        return false;
    }
    if find_from(RENDER_OK, "rate=1000ppm", 0).is_none() {
        return false;
    }
    if find_from(RENDER_OK, "corpus=14", 0).is_none() {
        return false;
    }
    if text_len(RENDER_OK) != 101 {
        return false;
    }
    // 注入后的渲染必须与锚不同（锚只描述全绿态）。
    let inj = match run_with(Some((0usize, Fault::Stage))) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if same_text(inj.render().as_str(), RENDER_OK) {
        return false;
    }
    if find_from(inj.render().as_str(), "rate=929ppm", 0).is_none() {
        return false;
    }
    // 矩阵文本与报表文本给出同一组数。
    let m = Matrix::of();
    let mt = m.render();
    if find_from(mt.as_str(), "run total=4 good=4 red=0", 0).is_none() {
        return false;
    }
    if find_from(mt.as_str(), "notfound total=8 good=2 red=6", 0).is_none() {
        return false;
    }
    if find_from(mt.as_str(), "foreign total=1 good=1 red=0", 0).is_none() {
        return false;
    }
    if newline_count(mt.as_str()) != TIER_COUNT {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 判据 13（前件）：阶段闸门单调
// ---------------------------------------------------------------------------

/// `Gate` 单调不倒退，越闸与关后再走各归其码。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.closed() || g.last() != -1 {
        return false;
    }
    // 首步不许是中段以外倒着来：先走到 search。
    if g.step(Stage::Search) != Ok(()) {
        return false;
    }
    if g.last() != 3 || g.steps() != 1 {
        return false;
    }
    // 回退：errno 之后不许回到 hash，且拒绝不落账。
    match g.step(Stage::Hash) {
        Err(c) => {
            if c != EX_ESTAGE {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if g.last() != 3 || g.steps() != 1 || g.closed() {
        return false;
    }
    // 同段重复允许（同一道闸上多条观测）。
    if g.step(Stage::Search) != Ok(()) {
        return false;
    }
    if g.steps() != 2 || g.last() != 3 {
        return false;
    }
    // 一路走满。
    if g.step(Stage::Errno) != Ok(()) || g.step(Stage::Spawn) != Ok(()) {
        return false;
    }
    if g.step(Stage::Rc) != Ok(()) || g.step(Stage::Commit) != Ok(()) {
        return false;
    }
    if g.steps() != 6 {
        return false;
    }
    // 关闸之后一切动作都是状态错，且关闸只能关一次。
    if g.close() != Ok(6) {
        return false;
    }
    if !g.closed() {
        return false;
    }
    match g.step(Stage::Commit) {
        Err(c) => {
            if c != EX_ESTATE {
                return false;
            }
        }
        Ok(()) => return false,
    }
    match g.close() {
        Err(c) => {
            if c != EX_ESTATE {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if g.steps() != 6 {
        return false;
    }
    // 正门：八段全序走一遍，段数等于段数表。
    let mut h = Gate::new();
    let mut i = 0usize;
    while i < Stage::count() {
        let s = Stage::from_order(i);
        if h.step(s) != Ok(()) {
            return false;
        }
        if h.last() != i as i32 {
            return false;
        }
        i += 1;
    }
    if h.close() != Ok(STAGE_COUNT as u32) {
        return false;
    }
    // 回档闭合：order 与 from_order 互逆，且次序严格递增。
    let mut prev = -1i32;
    i = 0usize;
    while i < STAGE_COUNT {
        let s = Stage::from_order(i);
        if s.order() != i {
            return false;
        }
        if s.name().is_empty() {
            return false;
        }
        let o = s.order() as i32;
        if o <= prev {
            return false;
        }
        prev = o;
        i += 1;
    }
    if Stage::from_order(STAGE_COUNT + 4) != Stage::Commit {
        return false;
    }
    // 闸后线：spawn 及以后为"已越过跑过这条线"，恰三段。
    let mut past = 0usize;
    let mut verdict = 0usize;
    i = 0usize;
    while i < STAGE_COUNT {
        let s = Stage::from_order(i);
        if s.past_spawn() {
            past += 1;
        }
        if s.shell_verdict() {
            verdict += 1;
        }
        i += 1;
    }
    if past != 3 || verdict != 3 {
        return false;
    }
    if !Stage::Spawn.past_spawn() || Stage::Errno.past_spawn() {
        return false;
    }
    if !Stage::Search.shell_verdict() || Stage::Rc.shell_verdict() {
        return false;
    }
    // 闸门上的两个码必须是段内两个不同的码。
    if EX_ESTAGE == EX_ESTATE || EX_ESTAGE == 0 || EX_ESTATE == 0 {
        return false;
    }
    g.last() == 7
}

// ---------------------------------------------------------------------------
// 判据 13（前件）：账本
// ---------------------------------------------------------------------------

/// `Ledger` 的槽位纪律、逐格对账、封存重放与篡改敏感。
fn ledger_ok() -> bool {
    let e = Ledger::new();
    if e.len() != 0 || !e.is_empty() || e.root() != 0 || e.is_frozen() {
        return false;
    }
    if e.rejects() != 0 || e.sample(0).is_some() {
        return false;
    }
    if e.audit() == Ok(()) {
        return false;
    }
    // 插队登记：槽位必须严格等于当前长度。
    let mut l = Ledger::new();
    match l.register(Sample { slot: 1, op: OP_WORD, ok: true, code: 0, value: 14 }) {
        Err(c) => {
            if c != EX_ESLOT {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if l.len() != 0 || l.rejects() != 1 {
        return false;
    }
    if l.register(Sample { slot: 0, op: OP_WORD, ok: true, code: 0, value: 14 }) != Ok(()) {
        return false;
    }
    if l.len() != 1 || l.rejects() != 1 || l.is_empty() {
        return false;
    }
    if l.sample(0).is_none() || l.sample(1).is_some() {
        return false;
    }
    // 满账：14 格 = 7 统计 + 1 红档 + 6 缺陷。
    let mut f = build_ledger();
    if f.len() != SAMPLES || f.ok_count() != ACCEPT || f.err_count() != REJECT {
        return false;
    }
    if f.audit() != Ok(()) {
        return false;
    }
    if f.sample(CORPUS).is_some() {
        return false;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        let s = match f.sample(i) {
            Some(x) => x,
            None => return false,
        };
        let v = match VALS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !s.ok || s.code != 0 || s.value != v || s.slot as usize != i {
            return false;
        }
        if i == ACCEPT - 1 && v != 929 {
            return false;
        }
        i += 1;
    }
    i = 0usize;
    while i < REJECT {
        let s = match f.sample(ACCEPT + i) {
            Some(x) => x,
            None => return false,
        };
        let c = match REJECT_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if s.ok || s.value != 0 || s.code != c {
            return false;
        }
        i += 1;
    }
    // 文本面：一格一行，行数等于格数。
    let txt = f.render();
    if newline_count(txt.as_str()) != SAMPLES {
        return false;
    }
    if find_from(txt.as_str(), "slot 0 op ", 0).is_none() {
        return false;
    }
    if find_from(txt.as_str(), "value 929", 0).is_none() {
        return false;
    }
    if !has_byte(txt.as_str(), b'\n') {
        return false;
    }
    // 指纹非零、冻结、重放同根。
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
    // 冻结后拒登记，且拒一次记一次数。
    let before = f.rejects();
    match f.register(Sample { slot: 14, op: OP_CORPUS, ok: true, code: 0, value: 1 }) {
        Err(c) => {
            if c != EX_ESLOT {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if f.rejects() != before + 1 || f.len() != SAMPLES {
        return false;
    }
    // 二次冻结是平衡错（不是静默成功）。
    match f.freeze() {
        Err(c) => {
            if c != EX_EBAL {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 篡改一格：对账与重放都必须现形。
    let mut t = f;
    if let Some(cell) = t.rows.get_mut(0usize) {
        cell.value = 999;
    } else {
        return false;
    }
    if t.audit() == Ok(()) {
        return false;
    }
    if t.replay() == Ok(root) {
        return false;
    }
    match t.replay() {
        Err(c) => {
            if c != EX_ESEAL {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 败格带上观测值也是错（缺陷码格的 value 恒零）。
    let mut b = build_ledger();
    if let Some(cell) = b.rows.get_mut(ACCEPT) {
        cell.value = 7;
    } else {
        return false;
    }
    if b.audit() != Err(EX_EVALUE) {
        return false;
    }
    // 成格带上缺陷码也是错。
    let mut d = build_ledger();
    if let Some(cell) = d.rows.get_mut(1usize) {
        cell.code = EX_ECODE;
    } else {
        return false;
    }
    if d.audit() != Err(EX_EVALUE) {
        return false;
    }
    // 少一格就是不平。
    let mut s = Ledger::new();
    let full = build_ledger();
    let mut k = 0usize;
    while k < SAMPLES - 1 {
        if let Some(g) = full.sample(k) {
            if s.register(g) != Ok(()) {
                return false;
            }
        } else {
            return false;
        }
        k += 1;
    }
    if s.audit() != Err(EX_EBAL) {
        return false;
    }
    // 缺陷次序被打乱（把两个败格码对调）也要被逐格对账拦下。
    let mut w = build_ledger();
    let a0 = match w.sample(ACCEPT) {
        Some(x) => x,
        None => return false,
    };
    let a1 = match w.sample(ACCEPT + 1) {
        Some(x) => x,
        None => return false,
    };
    if a0.code == a1.code {
        return false;
    }
    if let Some(cell) = w.rows.get_mut(ACCEPT) {
        cell.code = a1.code;
    } else {
        return false;
    }
    if let Some(cell) = w.rows.get_mut(ACCEPT + 1) {
        cell.code = a0.code;
    } else {
        return false;
    }
    if w.audit() == Ok(()) {
        return false;
    }
    // 空白样本与样本指纹：同格同指纹、异格异指纹。
    let s0 = Sample::blank();
    if s0.ok || s0.code != 0 || s0.value != 0 || s0.slot != 0 || s0.op != 0 {
        return false;
    }
    if s0.seal() != Sample::blank().seal() {
        return false;
    }
    if s0.seal() == a0.seal() {
        return false;
    }
    if !has_byte(s0.text().as_str(), b' ') {
        return false;
    }
    if find_from(s0.text().as_str(), "ok false", 0).is_none() {
        return false;
    }
    // 账本的根对格改动敏感（同一份账两次折叠必同）。
    if build_ledger().seal() != full.seal() {
        return false;
    }
    if build_ledger().seal() == 0 {
        return false;
    }
    if fold_word(FNV_OFF, 0) == fold_word(FNV_OFF, 1) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 13 条判据的入口
// ---------------------------------------------------------------------------

/// 本卡的判据签发面：恰好 13 条，逐条一个私有谓词。
///
/// 判据次序就是纵线次序：先把**表**钉住（名称/码表/闸段/答复档），
/// 再把**推演**钉住（define、126/127 分界、归因、三通道自洽），
/// 然后才谈**申报**（六维对判、语料纪律、横计矩阵、渲染面），
/// 最后落到闸门与账本——冻结重放四注入是全卡的收口。
pub fn run_unx11664_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11664_bb_notfound_exit");
    s.add(
        "EX01-名称与段基址",
        name_ok(),
        "SEG_BASE=0x6d1a0 起十三条码连续互异、码名回查闭合、126/127/128+signum 三档预留数各归其位",
    );
    s.add(
        "EX02-错误码表与缺陷档",
        codes_ok(),
        "CODE_TABLE 十三格与 Fault::code() 逐格对齐，Ok 恒零、非 Ok 恒非零且全落在本段内",
    );
    s.add(
        "EX03-八闸纵线",
        stages_ok(),
        "word→commit 八段名/序/回档三形闭合，past_spawn 与 shell_verdict 各恰三段",
    );
    s.add(
        "EX04-查找答复与目录分流",
        lookup_ok(),
        "七答复档的 found/walks_path/gate 自洽，同一个目录现场两策略分别给 127 与 126",
    );
    s.add(
        "EX05-现场推演 define",
        define_ok(),
        "八闸正反例逐段定型，跑过之后判词位必灭，同输入两次复算逐位相同",
    );
    s.add(
        "EX06-126 与 127 的分界",
        boundary_ok(),
        "EACCES/EISDIR/ENOEXEC/ELOOP 一律 126，唯 ENOENT 通 127，文案族必须跟上数字",
    );
    s.add(
        "EX07-数字归因档",
        origin_ok(),
        "六归因档互斥闭合，子进程退 127 记 child-status，判词位只在 shell-notfound 为真",
    );
    s.add(
        "EX08-三通道一致",
        chan_ok(),
        "判词位为真⟹数/errno/文案族三通道同真，作废档永不自洽，两条状态行字面互异",
    );
    s.add(
        "EX09-六维对判",
        judge_ok(),
        "判序 Estate→Stage→Code→Value→Note→Default 固定，先到的缺陷吃掉后到的，六码互异",
    );
    s.add(
        "EX10-语料与期望表",
        corpus_ok() && slots_ok(),
        "活 8 死 6、标签唯一、死槽一槽一维且按序，六张 WANT 表用第二形式复写申报表并交叉核",
    );
    s.add(
        "EX11-横计矩阵与红档显式",
        matrix_ok() && reds_ok(),
        "四档分布 4/8/1/1、good 与 total 两维独立、六个红档全挤在 127 档、红档率 929",
    );
    s.add(
        "EX12-状态面与逐字锚",
        message_ok() && detail_ok(),
        "strerror 原文、$? 载体、信号算术行、主状态行与全绿渲染逐字对齐，本卡不出一行 shell 文案",
    );
    s.add(
        "EX13-冻结重放四注入",
        {
            let mut led = build_ledger();
            let audited = led.audit() == Ok(());
            let sealed = match led.freeze() {
                Ok(r) => r != 0 && led.is_frozen() && led.replay() == Ok(r),
                Err(_) => false,
            };
            let four = inject_four();
            audited && sealed && led.rejects() == 0 && gate_ok() && ledger_ok() && four_true(four)
        },
        "满账逐格对平后封存，重放同根，冻结后拒登记零次，表路/判路/渲染路/账本路四路注入全绿",
    );
    s
}

// __CHUNK9__

// TEMP-PROBE-BEGIN
/// 临时逐谓词定位（交付前删除）。
pub fn __probe() -> [(&'static str, bool); 17] {
    [
        ("name_ok", name_ok()),
        ("codes_ok", codes_ok()),
        ("stages_ok", stages_ok()),
        ("lookup_ok", lookup_ok()),
        ("define_ok", define_ok()),
        ("boundary_ok", boundary_ok()),
        ("origin_ok", origin_ok()),
        ("chan_ok", chan_ok()),
        ("judge_ok", judge_ok()),
        ("corpus_ok", corpus_ok()),
        ("slots_ok", slots_ok()),
        ("matrix_ok", matrix_ok()),
        ("reds_ok", reds_ok()),
        ("message_ok", message_ok()),
        ("detail_ok", detail_ok()),
        ("gate_ok", gate_ok()),
        ("ledger_ok", ledger_ok()),
    ]
}

/// 临时：四路注入与账本收口（交付前删除）。
pub fn __probe2() -> [(&'static str, bool); 8] {
    let four = inject_four();
    let mut l = build_ledger();
    let audited = l.audit() == Ok(());
    let frz = match l.freeze() {
        Ok(r) => r != 0,
        Err(_) => false,
    };
    let rep = l.replay().is_ok();
    let f0 = match four.get(0) {
        Some(x) => *x,
        None => false,
    };
    let f1 = match four.get(1) {
        Some(x) => *x,
        None => false,
    };
    let f2 = match four.get(2) {
        Some(x) => *x,
        None => false,
    };
    let f3 = match four.get(3) {
        Some(x) => *x,
        None => false,
    };
    [
        ("four0", f0),
        ("four1", f1),
        ("four2", f2),
        ("four3", f3),
        ("audit", audited),
        ("freeze", frz),
        ("replay", rep),
        ("run_ok", run().is_ok()),
    ]
}

/// 临时：矩阵逐档 total/good（交付前删除）。
pub fn __matrix() -> [usize; 8] {
    let m = Matrix::of();
    let mut out = [0usize; 8];
    let mut i = 0usize;
    while i < TIER_COUNT {
        if let Some(r) = m.row(Tier::from_index(i)) {
            if let Some(a) = out.get_mut(i * 2) {
                *a = r.total;
            }
            if let Some(b) = out.get_mut(i * 2 + 1) {
                *b = r.good;
            }
        }
        i += 1;
    }
    out
}

impl Tier {
    /// 临时回档（交付前删除）。
    pub fn from_index(i: usize) -> Tier {
        match i {
            0 => Tier::Run,
            1 => Tier::NotFound,
            2 => Tier::NotExec,
            _ => Tier::Foreign,
        }
    }
}

/// 临时：逐槽实际/申报（交付前删除）。
pub fn __slot(i: usize) -> (&'static str, u32, i32, u32, &'static str, bool, &'static str) {
    let cs = cases();
    let c = match cs.get(i) {
        Some(x) => *x,
        None => return ("oob", 0, 0, 0, "", false, ""),
    };
    let a = c.actual();
    (
        c.shape.tag,
        a.value as u32,
        a.code,
        a.stage.order() as u32,
        a.note.name(),
        a.default,
        fault_short(judge(&c)),
    )
}

/// 临时：状态行渲染（交付前删除）。
pub fn __text(i: usize) -> &'static str {
    let cs = cases();
    match cs.get(i) {
        Some(c) => {
            let a = c.actual();
            a.note.name()
        }
        None => "oob",
    }
}
// TEMP-PROBE-END

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一条"裸名扫尽"的 127 现场：本卡主判档的最小正例。
    fn miss_127() -> Shape {
        let mut s = Shape::blank();
        s.tag = "unit-miss";
        s.word = "nosuchtool";
        s.path_dirs = 4;
        s.dirs_scanned = 4;
        s.lookup = Lookup::PathMiss;
        s.path_consulted = true;
        s.exec_errno = Errno::Enoent;
        s.text = TextChan::NotFoundLine;
        s
    }

    /// 造一条"PATH 命中并跑通"的现场：执行链正例。
    fn hit_exec(child_rc: u16) -> Shape {
        let mut s = Shape::blank();
        s.tag = "unit-hit";
        s.word = "prog";
        s.path_dirs = 4;
        s.dirs_scanned = 2;
        s.lookup = Lookup::PathHit;
        s.path_consulted = true;
        s.exec_errno = Errno::ENone;
        s.spawned = true;
        s.child_rc = child_rc;
        s.text = TextChan::Silent;
        s
    }

    #[test]
    fn t01_seg_base_is_pinned() {
        assert_eq!(SEG_BASE, 0x6d1a0);
        assert_eq!(EX_ESTAGE, SEG_BASE + 0x01);
        assert_eq!(EX_EBAL, SEG_BASE + 0x0d);
    }

    #[test]
    fn t02_thirteen_codes_live_inside_the_segment() {
        assert_eq!(CODE_TABLE.len(), 13);
        let mut i = 0usize;
        while i < CODE_TABLE.len() {
            assert!(CODE_TABLE[i] > SEG_BASE);
            assert!(CODE_TABLE[i] <= SEG_BASE + 0x0d);
            i += 1;
        }
    }

    #[test]
    fn t03_thirteen_codes_are_pairwise_distinct() {
        let mut i = 0usize;
        while i < CODE_TABLE.len() {
            let mut j = i + 1usize;
            while j < CODE_TABLE.len() {
                assert_ne!(CODE_TABLE[i], CODE_TABLE[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t04_reject_codes_follow_the_fault_order() {
        assert_eq!(REJECT_CODES.len(), REJECT);
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        let mut k = 0usize;
        while k < REJECT {
            assert_eq!(REJECT_CODES[k], REJECT_FAULTS[k].code());
            assert_eq!(Fault::Ok.code(), 0);
            assert!(REJECT_FAULTS[k].alive());
            assert!(REJECT_FAULTS[k].is_claim_fault());
            k += 1;
        }
    }

    #[test]
    fn t05_vals_cells_are_the_pinned_statistics() {
        assert_eq!(VALS.len(), ACCEPT);
        assert_eq!(VALS[0] as usize, CORPUS);
        assert_eq!(VALS[1] as usize, ACCEPT);
        assert_eq!(VALS[2] as usize, REJECT);
        assert_eq!(VALS[3] + VALS[4] + VALS[5] + VALS[6], ACCEPT as u32);
        assert_eq!(VALS[7], 929);
        assert_eq!(VALS[7], red_rate());
    }

    #[test]
    fn t06_fold_helpers_are_not_identities() {
        let h = FNV_OFF;
        assert_eq!(fold_byte(h, b'A'), fold_str(h, "A"));
        assert_ne!(fold_byte(h, b'A'), h);
        assert_ne!(fold_word(h, 0), h);
        assert_ne!(fold_word(h, 1), fold_word(h, 2));
        assert_eq!(fold_str(h, "ab"), fold_str(fold_str(h, "a"), "b"));
    }

    #[test]
    fn t07_decimal_and_hex_renderers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(127).as_str(), "127");
        assert_eq!(u32_text(1000).as_str(), "1000");
        assert_eq!(hex_text(0x0a, 4).as_str(), "000a");
        assert_eq!(i32_text(-2).as_str(), "-2");
        assert_eq!(i32_text(0).as_str(), "0");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
    }

    #[test]
    fn t08_word_form_classifies_the_shape_family() {
        assert_eq!(word_form(""), WordForm::Empty);
        assert_eq!(word_form("   "), WordForm::Blank);
        assert_eq!(word_form("a\nb"), WordForm::Control);
        assert_eq!(word_form("A=1"), WordForm::Assign);
        assert_eq!(word_form(".."), WordForm::Dots);
        assert_eq!(word_form("/usr/bin/ls"), WordForm::Slashed);
        assert_eq!(word_form("ls"), WordForm::Bare);
    }

    #[test]
    fn t09_word_form_predicates_partition_the_eight_forms() {
        let table = [
            WordForm::Bare,
            WordForm::Slashed,
            WordForm::Empty,
            WordForm::Blank,
            WordForm::Assign,
            WordForm::Dots,
            WordForm::Control,
            WordForm::Overlong,
        ];
        let mut i = 0usize;
        while i < table.len() {
            let f = table[i];
            assert!(!f.name().is_empty());
            assert!(!f.is_no_command() || !f.is_void());
            if f.consults_path() {
                assert_eq!(f, WordForm::Bare);
            }
            i += 1;
        }
        assert!(WordForm::Empty.is_no_command());
        assert!(WordForm::Blank.is_no_command());
        assert!(WordForm::Assign.is_no_command());
        assert!(!WordForm::Dots.is_no_command());
        assert!(WordForm::Control.is_void());
        assert!(WordForm::Overlong.is_void());
        assert!(WordForm::Bare.consults_path());
        assert!(!WordForm::Slashed.consults_path());
    }

    #[test]
    fn t10_byte_primitives_are_hand_written_correctly() {
        assert!(same_text("abc", "abc"));
        assert!(!same_text("abc", "abd"));
        assert!(!same_text("ab", "abc"));
        assert_eq!(text_len("abc"), 3);
        assert!(has_byte("a/b", b'/'));
        assert!(!has_byte("ab", b'/'));
        assert_eq!(find_from("aXbXc", "X", 0), Some(1));
        assert_eq!(find_from("aXbXc", "X", 2), Some(3));
        assert_eq!(find_from("aXbXc", "X", 4), None);
        assert_eq!(find_from("abc", "", 0), None);
        assert!(starts_with("bash: x", "bash"));
        assert!(!starts_with("ash: x", "bash"));
    }

    #[test]
    fn t11_slash_and_blank_helpers() {
        assert_eq!(slash_shape("/a/b"), (true, false));
        assert_eq!(slash_shape("/a/b/"), (true, true));
        assert_eq!(slash_shape("bare"), (false, false));
        assert!(word_has_slash("a/b"));
        assert!(!word_has_slash("ab"));
        assert!(is_blank_word(" \t "));
        assert!(!is_blank_word(""));
        assert!(!is_blank_word(" a"));
    }

    #[test]
    fn t12_assign_only_is_the_posix_empty_command() {
        assert!(is_assign_only("A=1"));
        assert!(is_assign_only("FOO=bar"));
        assert!(is_assign_only("_x9=v"));
        assert!(!is_assign_only("=1"));
        assert!(!is_assign_only("1A=1"));
        assert!(!is_assign_only("A=1 ls"));
        assert!(!is_assign_only("A=/x/y"));
        assert!(!is_assign_only("plain"));
    }

    #[test]
    fn t13_control_and_dots_helpers() {
        assert!(has_control("a\nb"));
        assert!(has_control("\u{7f}"));
        assert!(!has_control("abc"));
        assert!(is_dots("."));
        assert!(is_dots("..."));
        assert!(!is_dots("./"));
        assert!(!is_dots(""));
    }

    #[test]
    fn t14_path_dir_count_and_basename() {
        assert_eq!(path_dir_count(""), 0);
        assert_eq!(path_dir_count("/bin"), 1);
        assert_eq!(path_dir_count("/bin:/usr/bin:/sbin"), 3);
        assert_eq!(last_component("/bin/sh"), "sh");
        assert_eq!(last_component("sh"), "sh");
        assert_eq!(last_component("/bin/"), "");
    }

    #[test]
    fn t15_lookup_answers_are_exclusive() {
        let table = [
            Lookup::Builtin,
            Lookup::HashHit,
            Lookup::HashStale,
            Lookup::PathHit,
            Lookup::PathMiss,
            Lookup::DirOnPath,
            Lookup::SlashTarget,
        ];
        let mut miss = 0usize;
        let mut walkers = 0usize;
        let mut i = 0usize;
        while i < table.len() {
            let l = table[i];
            assert!(!l.name().is_empty());
            if l.is_miss() {
                miss += 1;
                assert!(!l.found());
            }
            if l.walks_path() {
                walkers += 1;
                assert_eq!(l.gate(), Stage::Search);
            }
            assert_eq!(l.found(), !matches!(l, Lookup::HashStale | Lookup::PathMiss));
            i += 1;
        }
        assert_eq!(miss, 1);
        assert_eq!(walkers, 3);
        assert_eq!(Lookup::Builtin.gate(), Stage::Builtin);
        assert_eq!(Lookup::HashStale.gate(), Stage::Hash);
        assert_eq!(Lookup::PathMiss.gate(), Stage::Search);
        assert_eq!(Lookup::SlashTarget.gate(), Stage::Search);
        assert_eq!(Lookup::SlashTarget.name(), "slash-target");
    }

    #[test]
    fn t16_one_directory_two_legal_numbers() {
        assert_eq!(DirPolicy::Skipped.exit_of(), EXIT_NOT_FOUND);
        assert_eq!(DirPolicy::Candidate.exit_of(), EXIT_NOT_EXEC);
        assert_eq!(DirPolicy::Skipped.errno_of(), CH_ENOENT);
        assert_eq!(DirPolicy::Candidate.errno_of(), CH_EISDIR);
        assert_ne!(DirPolicy::Skipped.exit_of(), DirPolicy::Candidate.exit_of());
        assert_ne!(DirPolicy::Skipped.name(), DirPolicy::Candidate.name());
    }

    #[test]
    fn t17_errno_channel_maps_to_the_exit_table() {
        assert_eq!(Errno::ENone.value(), CH_ENONE);
        assert_eq!(Errno::Enoent.value(), 2);
        assert_eq!(Errno::Eacces.value(), 13);
        assert_eq!(Errno::Eisdir.value(), 21);
        assert_eq!(Errno::Enoexec.value(), 8);
        assert_eq!(Errno::Eloop.value(), 40);
        assert_eq!(Errno::Einval.value(), 22);
        assert!(Errno::Enoent.is_not_found());
        assert!(!Errno::Eacces.is_not_found());
        assert!(Errno::Eacces.is_not_exec());
        assert!(Errno::Eisdir.is_not_exec());
        assert!(Errno::Enoexec.is_not_exec());
        assert!(Errno::Eloop.is_not_exec());
        assert!(!Errno::ENone.is_not_exec());
        assert_eq!(Errno::Enoent.exit_of(), 127);
        assert_eq!(Errno::Eacces.exit_of(), 126);
        assert_eq!(Errno::Eloop.exit_of(), 126);
        assert_eq!(Errno::ENone.exit_of(), EXIT_USAGE);
        assert!(!Errno::Enoent.implies_exists());
        assert!(Errno::Eacces.implies_exists());
    }

    #[test]
    fn t18_errno_wire_round_trip_never_impersonates_enoent() {
        let table = [
            Errno::ENone,
            Errno::Enoent,
            Errno::Eacces,
            Errno::Eisdir,
            Errno::Enoexec,
            Errno::Eloop,
            Errno::Einval,
        ];
        let mut i = 0usize;
        while i < table.len() {
            assert_eq!(Errno::of(table[i].value()), table[i]);
            assert!(!table[i].name().is_empty());
            i += 1;
        }
        assert_eq!(Errno::of(9999), Errno::Einval);
        assert_eq!(Errno::of(-1), Errno::Einval);
    }

    #[test]
    fn t19_text_channel_projects_onto_the_number() {
        assert!(TextChan::NotFoundLine.is_shell_verdict());
        assert!(TextChan::PermLine.is_shell_verdict());
        assert!(!TextChan::Silent.is_shell_verdict());
        assert!(!TextChan::ChildBytes.is_shell_verdict());
        assert_eq!(TextChan::NotFoundLine.expected_errno(), CH_ENOENT);
        assert_eq!(TextChan::NotFoundLine.expected_exit(), EXIT_NOT_FOUND);
        assert_eq!(TextChan::PermLine.expected_exit(), EXIT_NOT_EXEC);
        assert_eq!(TextChan::IsDirLine.expected_errno(), CH_EISDIR);
        assert_eq!(TextChan::ExecFmtLine.expected_errno(), CH_ENOEXEC);
        assert_eq!(TextChan::Silent.expected_exit(), EXIT_OK);
        assert_eq!(TextChan::ChildBytes.expected_errno(), CH_ENONE);
    }

    #[test]
    fn t20_note_family_is_a_partition() {
        assert!(!Note::NotFound127.ran());
        assert!(Note::ChildMimic.ran());
        assert!(Note::ChildRan.ran());
        assert!(Note::ChildNotExec.ran());
        assert!(Note::Signaled.ran());
        assert!(Note::FallbackRan.ran());
        assert!(Note::EmptyWord.ran());
        assert!(Note::BuiltinOk.ran());
        assert!(Note::NotFound127.is_notfound());
        assert!(Note::NotExec126.is_notexec());
        assert!(Note::IsDir126.is_notexec());
        assert!(Note::ChanSplit.is_void());
        assert_eq!(Note::NotFound127.origin(), Origin::ShellNotFound);
        assert_eq!(Note::IsDir126.origin(), Origin::ShellNotExec);
        assert_eq!(Note::ChildMimic.origin(), Origin::ChildStatus);
        assert_eq!(Note::BuiltinOk.origin(), Origin::Builtin);
        assert_eq!(Note::EmptyWord.origin(), Origin::Empty);
        assert_eq!(Note::ChanSplit.origin(), Origin::Void);
        assert_eq!(Note::NotFound127.errno_channel(), CH_ENOENT);
        assert_eq!(Note::NotExec126.errno_channel(), CH_EACCES);
        assert_eq!(Note::IsDir126.errno_channel(), CH_EISDIR);
        assert_eq!(Note::ChanSplit.errno_channel(), CH_EINVAL);
        assert_eq!(Note::ChanSplit.exit_channel(), EXIT_USAGE);
    }

    #[test]
    fn t21_origin_says_who_signed_the_number() {
        assert!(Origin::ShellNotFound.is_shell());
        assert!(Origin::ShellNotExec.is_shell());
        assert!(!Origin::ChildStatus.is_shell());
        assert!(!Origin::Builtin.is_shell());
        assert!(!Origin::Empty.is_shell());
        assert!(!Origin::Void.is_shell());
        assert!(Origin::ShellNotFound.is_notfound_verdict());
        assert!(!Origin::ShellNotExec.is_notfound_verdict());
        assert_ne!(Origin::ShellNotFound.name(), Origin::ShellNotExec.name());
    }

    #[test]
    fn t22_eight_stage_gates_are_monotone_and_total() {
        assert_eq!(Stage::count(), STAGE_COUNT);
        let mut i = 0usize;
        while i < STAGE_COUNT {
            let s = Stage::from_order(i);
            assert_eq!(s.order(), i);
            assert!(!s.name().is_empty());
            assert_eq!(s.past_spawn(), i >= 5);
            assert_eq!(s.shell_verdict(), i == 2 || i == 3 || i == 4);
            i += 1;
        }
        assert_eq!(Stage::from_order(STAGE_COUNT), Stage::Commit);
        assert_eq!(Stage::from_order(usize::MAX), Stage::Commit);
        assert_eq!(Stage::Word.name(), "word");
        assert_eq!(Stage::Commit.name(), "commit");
        assert!(Stage::Search.shell_verdict());
        assert!(!Stage::Builtin.shell_verdict());
        assert!(!Stage::Builtin.past_spawn());
        assert!(Stage::Rc.past_spawn());
    }

    #[test]
    fn t23_shell_reserved_covers_126_127_and_signals() {
        assert!(shell_reserved(EXIT_NOT_EXEC));
        assert!(shell_reserved(EXIT_NOT_FOUND));
        assert!(shell_reserved(128 + 9));
        assert!(!shell_reserved(0));
        assert!(!shell_reserved(1));
        assert!(!shell_reserved(EXIT_PROGRAM_MAX));
        assert!(shell_reserved(128));
        assert!(shell_reserved(128 + SIG_MAX as u16));
        assert!(!shell_reserved(128 + SIG_MAX as u16 + 1));
    }

    #[test]
    fn t24_exit_family_boundaries() {
        assert_eq!(msg_exit_family(0), "success");
        assert_eq!(msg_exit_family(1), "program");
        assert_eq!(msg_exit_family(125), "program");
        assert_eq!(msg_exit_family(126), "reserved-not-executable");
        assert_eq!(msg_exit_family(127), "reserved-not-found");
        assert_eq!(msg_exit_family(137), "signal");
        assert_eq!(msg_exit_family(128), "out-of-band");
        assert_eq!(msg_exit_family(300), "out-of-band");
    }

    #[test]
    fn t25_signal_names_and_arithmetic_line() {
        assert_eq!(sig_name(9), "SIGKILL");
        assert_eq!(sig_name(11), "SIGSEGV");
        assert_eq!(sig_name(15), "SIGTERM");
        assert_eq!(sig_name(0), "SIG?");
        assert_eq!(sig_name(200), "SIG?");
        assert_eq!(msg_signal_line(9).as_str(), "128 + 9 = 137 (SIGKILL)");
        assert_eq!(msg_signal_line(11).as_str(), "128 + 11 = 139 (SIGSEGV)");
    }

    #[test]
    fn t26_define_empty_command_exits_zero() {
        let mut s = Shape::blank();
        s.word = "PREFIX=1";
        s.text = TextChan::Silent;
        s.exec_errno = Errno::ENone;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Word);
        assert_eq!(o.code, CH_ENONE);
        assert_eq!(o.value, EXIT_OK);
        assert_eq!(o.note, Note::EmptyWord);
        assert!(!o.default);
        assert!(o.ok());
        assert_eq!(Tier::of(&o), Tier::Run);
    }

    #[test]
    fn t27_empty_command_carrying_a_verdict_is_void() {
        let mut s = Shape::blank();
        s.word = "   ";
        s.text = TextChan::NotFoundLine;
        s.exec_errno = Errno::ENone;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Word);
        assert_eq!(o.note, Note::ChanSplit);
        assert_eq!(o.code, CH_EINVAL);
        assert_eq!(o.value, EXIT_USAGE);
        assert!(!o.ok());
        assert_eq!(Tier::of(&o), Tier::Foreign);

        let mut s2 = Shape::blank();
        s2.word = "A=1";
        s2.text = TextChan::Silent;
        s2.exec_errno = Errno::Enoent;
        assert_eq!(define(&s2).note, Note::ChanSplit);
    }

    #[test]
    fn t28_builtin_hit_never_consults_path() {
        let mut s = Shape::blank();
        s.word = "cd";
        s.lookup = Lookup::Builtin;
        s.text = TextChan::Silent;
        s.exec_errno = Errno::ENone;
        s.path_consulted = false;
        s.dirs_scanned = 0;
        s.builtin_rc = 3;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Builtin);
        assert_eq!(o.value, 3);
        assert_eq!(o.note, Note::BuiltinOk);
        assert!(!o.default);
        assert!(o.ok());
        assert_eq!(Tier::of(&o), Tier::Foreign);
    }

    #[test]
    fn t29_builtin_that_walked_the_path_is_void() {
        let mut s = Shape::blank();
        s.word = "cd";
        s.lookup = Lookup::Builtin;
        s.text = TextChan::Silent;
        s.exec_errno = Errno::ENone;
        s.path_consulted = true;
        s.dirs_scanned = 2;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Builtin);
        assert_eq!(o.note, Note::ChanSplit);
        assert!(!o.channels_agree());

        let mut s2 = Shape::blank();
        s2.word = "cd";
        s2.lookup = Lookup::Builtin;
        s2.text = TextChan::Silent;
        s2.exec_errno = Errno::ENone;
        s2.path_consulted = false;
        s2.dirs_scanned = 0;
        s2.spawned = true;
        assert_eq!(define(&s2).stage, Stage::Builtin);
        assert_eq!(define(&s2).note, Note::ChanSplit);
    }

    #[test]
    fn t30_path_miss_is_the_canonical_127() {
        let o = define(&miss_127());
        assert_eq!(o.stage, Stage::Search);
        assert_eq!(o.code, CH_ENOENT);
        assert_eq!(o.value, EXIT_NOT_FOUND);
        assert_eq!(o.note, Note::NotFound127);
        assert!(o.default);
        assert!(!o.ok());
        assert!(o.not_found());
        assert!(o.channels_agree());
        assert_eq!(o.origin(), Origin::ShellNotFound);
        assert_eq!(Tier::of(&o), Tier::NotFound);
    }

    #[test]
    fn t31_short_scan_or_unconsulted_path_cannot_be_127() {
        let mut short = miss_127();
        short.dirs_scanned = 3;
        assert_eq!(define(&short).note, Note::ChanSplit);
        assert_eq!(define(&short).stage, Stage::Search);

        let mut unconsulted = miss_127();
        unconsulted.path_consulted = false;
        assert_eq!(define(&unconsulted).note, Note::ChanSplit);

        let mut wrong_errno = miss_127();
        wrong_errno.exec_errno = Errno::Eacces;
        assert_eq!(define(&wrong_errno).note, Note::ChanSplit);

        let mut over = miss_127();
        over.dirs_scanned = 9;
        assert_eq!(define(&over).stage, Stage::Search);
        assert_eq!(define(&over).note, Note::ChanSplit);
    }

    #[test]
    fn t32_silent_stderr_breaks_the_notfound_verdict() {
        let mut s = miss_127();
        s.text = TextChan::Silent;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Search);
        assert_eq!(o.note, Note::ChanSplit);
        assert_eq!(o.value, EXIT_USAGE);
        assert!(!o.default);
        assert!(!o.channels_agree());
    }

    #[test]
    fn t33_once_spawned_127_is_never_the_verdict() {
        let mut s = miss_127();
        s.spawned = true;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Spawn);
        assert_eq!(o.note, Note::ChanSplit);
        assert!(!o.default);
        assert_eq!(o.value, EXIT_USAGE);
    }

    #[test]
    fn t34_eacces_is_126_not_127() {
        let mut s = Shape::blank();
        s.word = "helper";
        s.path_dirs = 6;
        s.dirs_scanned = 3;
        s.lookup = Lookup::PathHit;
        s.path_consulted = true;
        s.exec_errno = Errno::Eacces;
        s.text = TextChan::PermLine;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Errno);
        assert_eq!(o.code, CH_EACCES);
        assert_eq!(o.value, EXIT_NOT_EXEC);
        assert_eq!(o.note, Note::NotExec126);
        assert!(!o.default);
        assert!(!o.not_found());
        assert_eq!(Tier::of(&o), Tier::NotExec);

        let mut wrong = s;
        wrong.text = TextChan::NotFoundLine;
        let v = define(&wrong);
        assert_eq!(v.note, Note::ChanSplit);
        assert_eq!(v.stage, Stage::Errno);
    }

    #[test]
    fn t35_directory_on_path_splits_by_policy() {
        let mut skip = Shape::blank();
        skip.word = "mydir";
        skip.path_dirs = 5;
        skip.dirs_scanned = 5;
        skip.lookup = Lookup::DirOnPath;
        skip.path_consulted = true;
        skip.dir_policy = DirPolicy::Skipped;
        skip.exec_errno = Errno::Enoent;
        skip.text = TextChan::NotFoundLine;
        let a = define(&skip);
        assert_eq!(a.value, EXIT_NOT_FOUND);
        assert_eq!(a.stage, Stage::Search);
        assert_eq!(a.note, Note::NotFound127);

        let mut cand = skip;
        cand.dir_policy = DirPolicy::Candidate;
        cand.dirs_scanned = 1;
        cand.text = TextChan::IsDirLine;
        let b = define(&cand);
        assert_eq!(b.value, EXIT_NOT_EXEC);
        assert_eq!(b.stage, Stage::Errno);
        assert_eq!(b.note, Note::IsDir126);
        assert_eq!(b.code, CH_EISDIR);
        assert_ne!(a.value, b.value);
    }

    #[test]
    fn t36_slashed_word_never_walks_the_path() {
        let mut s = Shape::blank();
        s.word = "/usr/local/bin/nope";
        s.lookup = Lookup::SlashTarget;
        s.path_consulted = false;
        s.dirs_scanned = 0;
        s.exec_errno = Errno::Enoent;
        s.text = TextChan::NotFoundLine;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Errno);
        assert_eq!(o.value, EXIT_NOT_FOUND);
        assert_eq!(o.note, Note::NotFound127);
        assert!(o.default);

        let mut walked = s;
        walked.path_consulted = true;
        walked.dirs_scanned = 1;
        assert_eq!(define(&walked).note, Note::ChanSplit);
        assert_eq!(define(&walked).stage, Stage::Search);
    }

    #[test]
    fn t37_stale_hash_must_rehash_the_whole_path() {
        let mut s = Shape::blank();
        s.word = "oldname";
        s.lookup = Lookup::HashStale;
        s.rehash = true;
        s.path_dirs = 3;
        s.dirs_scanned = 3;
        s.path_consulted = true;
        s.exec_errno = Errno::Enoent;
        s.text = TextChan::NotFoundLine;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Search);
        assert_eq!(o.note, Note::NotFound127);
        assert!(o.default);

        let mut half = s;
        half.dirs_scanned = 1;
        assert_eq!(define(&half).note, Note::ChanSplit);

        let mut lazy = s;
        lazy.rehash = false;
        lazy.path_consulted = false;
        lazy.dirs_scanned = 0;
        let l = define(&lazy);
        assert_eq!(l.stage, Stage::Hash);
        assert_eq!(l.note, Note::NotFound127);
        assert!(l.default);
    }

    #[test]
    fn t38_child_exit_127_is_foreign_not_notfound() {
        let o = define(&hit_exec(EXIT_CHILD_MIMIC));
        assert_eq!(o.stage, Stage::Rc);
        assert_eq!(o.code, CH_ENONE);
        assert_eq!(o.value, EXIT_NOT_FOUND);
        assert_eq!(o.note, Note::ChildMimic);
        assert!(!o.default);
        assert!(o.ok());
        assert_eq!(o.origin(), Origin::ChildStatus);
        assert_eq!(Tier::of(&o), Tier::Foreign);

        let n = define(&hit_exec(EXIT_NOT_EXEC));
        assert_eq!(n.note, Note::ChildNotExec);
        assert_eq!(n.value, EXIT_NOT_EXEC);
        assert_eq!(Tier::of(&n), Tier::Foreign);
    }

    #[test]
    fn t39_signal_beats_the_return_code() {
        let mut s = hit_exec(0);
        s.signaled = true;
        s.sig = SIG_KILL;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Rc);
        assert_eq!(o.value, 137);
        assert_eq!(o.note, Note::Signaled);
        assert_eq!(o.code, CH_ENONE);
        assert!(!o.default);

        let mut zero = s;
        zero.sig = 0;
        assert_eq!(define(&zero).note, Note::ChanSplit);
        assert_eq!(define(&zero).stage, Stage::Rc);

        let mut huge = s;
        huge.sig = SIG_MAX + 1;
        assert_eq!(define(&huge).note, Note::ChanSplit);
    }

    #[test]
    fn t40_exec_chain_guards_the_spawn_gate() {
        let mut never_ran = hit_exec(0);
        never_ran.spawned = false;
        let o = define(&never_ran);
        assert_eq!(o.stage, Stage::Spawn);
        assert_eq!(o.note, Note::ChanSplit);

        let mut shell_talk = hit_exec(0);
        shell_talk.text = TextChan::NotFoundLine;
        assert_eq!(define(&shell_talk).stage, Stage::Spawn);
        assert_eq!(define(&shell_talk).note, Note::ChanSplit);

        let mut plain = hit_exec(2);
        plain.fallback = false;
        let p = define(&plain);
        assert_eq!(p.stage, Stage::Commit);
        assert_eq!(p.value, 2);
        assert_eq!(p.note, Note::ChildRan);
        assert_eq!(Tier::of(&p), Tier::Foreign);

        let mut fb = hit_exec(0);
        fb.fallback = true;
        assert_eq!(define(&fb).note, Note::FallbackRan);
    }

    #[test]
    fn t41_define_is_a_pure_function() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            let a = define(&c.shape);
            let b = define(&c.shape);
            assert_eq!(a, b);
            assert_eq!(a.seal(), b.seal());
            assert_eq!(a.ok(), !a.failed());
            assert_eq!(a.origin(), a.note.origin());
            i += 1;
        }
    }

    #[test]
    fn t42_corpus_split_matches_the_contract() {
        assert_eq!(CORPUS, ACCEPT + REJECT);
        assert_eq!(CORPUS, 14);
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert_eq!(cases().len(), CORPUS);
        assert!(case_at(CORPUS).is_none());
        assert!(case_at(0).is_some());
        assert_eq!(SAMPLES, CORPUS);
    }

    #[test]
    fn t43_tags_are_unique_and_restated() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(c.tag(), WANT_TAGS[i]);
            let mut j = i + 1usize;
            while j < CORPUS {
                if let Some(d) = case_at(j) {
                    assert_ne!(c.tag(), d.tag());
                    assert_ne!(c.shape.seal(), d.shape.seal());
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t44_accept_slots_are_judged_clean() {
        assert_eq!(ACCEPT_SLOTS.len(), ACCEPT);
        let mut i = 0usize;
        while i < ACCEPT {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(ACCEPT_SLOTS[i], i);
            assert_eq!(judge(&c), Fault::Ok);
            assert_eq!(c.judge(), Fault::Ok);
            assert!(c.ok());
            assert_eq!(expected(i), Some(Fault::Ok));
            assert_eq!(channel_fault(&c), Fault::Ok);
            assert_eq!(dims_differ(&c), 0);
            assert_eq!(first_diff(&c), CAP);
            assert_eq!(WANT_FAULT[i], "ok");
            i += 1;
        }
    }

    #[test]
    fn t45_reject_slots_hit_their_own_dimension() {
        assert_eq!(REJECT_SLOTS.len(), REJECT);
        let mut k = 0usize;
        while k < REJECT {
            let slot = ACCEPT + k;
            let c = match case_at(slot) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(REJECT_SLOTS[k], slot);
            assert_eq!(judge(&c), REJECT_FAULTS[k]);
            assert_eq!(expected(slot), Some(REJECT_FAULTS[k]));
            assert_eq!(fault_ndx(REJECT_FAULTS[k]), k);
            assert_eq!(WANT_NDX[slot], k);
            assert_eq!(WANT_FAULT[slot], REJECT_FAULTS[k].name());
            assert_eq!(dim_name(k), REJECT_FAULTS[k].name());
            assert!(!REJECT_DIM_TEXT[k].is_empty());
            assert_eq!(REJECT_CODES[k], REJECT_FAULTS[k].code());
            k += 1;
        }
        assert!(expected(CORPUS).is_none());
    }

    #[test]
    fn t46_reject_slots_diverge_in_exactly_one_dimension() {
        let mut k = 0usize;
        while k < REJECT {
            let c = match case_at(ACCEPT + k) {
                Some(x) => x,
                None => return,
            };
            let a = c.actual();
            let mut diff = 0usize;
            if a.stage != c.claim.stage {
                diff += 1;
            }
            if a.code != c.claim.code {
                diff += 1;
            }
            if a.value != c.claim.value {
                diff += 1;
            }
            if a.note != c.claim.note {
                diff += 1;
            }
            if a.default != c.claim.default {
                diff += 1;
            }
            if k == 0 {
                // 状态维那一槽很特殊：成败翻面连带五维全歪。
                assert_eq!(diff, 5);
                assert_ne!(a.ok(), c.claim.success());
            } else {
                assert_eq!(diff, 1);
                assert_eq!(a.ok(), c.claim.success());
            }
            assert_eq!(dims_differ(&c), diff);
            k += 1;
        }
    }

    #[test]
    fn t47_want_tables_restate_the_claim_table() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            // WANT_* 是**申报表**的第二种形式（逐格复写 CLAIMS）。
            assert_eq!(c.claim.stage, WANT_STAGE[i]);
            assert_eq!(c.claim.code, WANT_CODE[i]);
            assert_eq!(c.claim.value, WANT_VALUE[i]);
            assert_eq!(c.claim.note, WANT_NOTE[i]);
            assert_eq!(c.claim.default, WANT_DEFAULT[i]);
            assert_eq!(c.claim.origin().name(), WANT_ORIGIN[i]);
            // WANT_TIER 反过来只看**实际结果**（`Tier::of` 只吃 Outcome）。
            let a = c.actual();
            assert_eq!(Tier::of(&a), WANT_TIER[i]);
            assert!(Tier::of(&a).index() < TIER_COUNT);
            // 活槽两表同形：申报逐位等于实际，于是 WANT_* 同时钉住两侧。
            if i < ACCEPT {
                assert_eq!(a.stage, WANT_STAGE[i]);
                assert_eq!(a.code, WANT_CODE[i]);
                assert_eq!(a.value, WANT_VALUE[i]);
                assert_eq!(a.note, WANT_NOTE[i]);
                assert_eq!(a.default, WANT_DEFAULT[i]);
                assert_eq!(a.origin().name(), WANT_ORIGIN[i]);
            }
            i += 1;
        }
    }

    #[test]
    fn t48_fault_order_is_the_judge_order() {
        let base = match case_at(4) {
            Some(x) => x,
            None => return,
        };
        assert!(base.actual().default);

        let mut estate = base;
        estate.claim = Claim::blank();
        assert_eq!(judge(&estate), Fault::Estate);

        let mut stage_then_estate = base;
        stage_then_estate.claim.stage = Stage::Hash;
        stage_then_estate.claim.code = CH_ENONE;
        stage_then_estate.claim.value = 0;
        stage_then_estate.claim.note = Note::ChildRan;
        stage_then_estate.claim.default = false;
        assert_eq!(judge(&stage_then_estate), Fault::Estate);

        let mut stage = base;
        stage.claim.stage = Stage::Hash;
        assert_eq!(judge(&stage), Fault::Stage);

        let mut code = base;
        code.claim.code = CH_EACCES;
        assert_eq!(judge(&code), Fault::Code);

        let mut value = base;
        value.claim.value = EXIT_NOT_EXEC;
        assert_eq!(judge(&value), Fault::Value);

        let mut note = base;
        note.claim.note = Note::NotExec126;
        assert_eq!(judge(&note), Fault::Note);

        let mut default = base;
        default.claim.default = false;
        assert_eq!(judge(&default), Fault::Default);

        let mut all_wrong = base;
        all_wrong.claim = Claim { stage: Stage::Word, code: CH_EINVAL, value: 2, note: Note::ChanSplit, default: false };
        // 作废族申报的 success() 也是假，于是 Estate 放行、第一处偏离落在闸段。
        assert_eq!(judge(&all_wrong), Fault::Stage);
    }

    #[test]
    fn t49_fault_index_and_dim_name_round_trip() {
        let mut k = 0usize;
        while k < REJECT {
            let f = REJECT_FAULTS[k];
            assert_eq!(fault_ndx(f), k);
            assert_eq!(dim_name(k), f.name());
            assert_eq!(f.code(), REJECT_CODES[k]);
            k += 1;
        }
        assert_eq!(fault_ndx(Fault::Ok), CAP);
        assert_eq!(dim_name(CAP), "none");
        assert_eq!(dim_name(99), "none");
        assert!(!Fault::Ok.alive());
        assert!(!Fault::Count.is_claim_fault());
        assert!(!Fault::Channel.is_claim_fault());
    }

    #[test]
    fn t50_channel_fault_is_the_second_ruler() {
        let mut s = miss_127();
        s.text = TextChan::Silent;
        let o = define(&s);
        let echoed = Claim { stage: o.stage, code: o.code, value: o.value, note: o.note, default: o.default };
        let bad = Case { shape: s, claim: echoed };
        assert_eq!(judge(&bad), Fault::Ok);
        assert_eq!(channel_fault(&bad), Fault::Channel);

        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            let want = if define(&c.shape).channels_agree() { Fault::Ok } else { Fault::Channel };
            assert_eq!(channel_fault(&c), want);
            i += 1;
        }
    }

    #[test]
    fn t51_message_lines_are_pinned() {
        let c = match case_at(4) {
            Some(x) => x,
            None => return,
        };
        let o = c.actual();
        assert_eq!(msg_strerror(Errno::Enoent).as_str(), "No such file or directory");
        assert_eq!(msg_strerror(Errno::Eacces).as_str(), "Permission denied");
        assert_eq!(msg_strerror(Errno::Eisdir).as_str(), "Is a directory");
        assert_eq!(msg_strerror(Errno::Enoexec).as_str(), "Exec format error");
        assert_eq!(msg_strerror(Errno::Eloop).as_str(), "Too many levels of symbolic links");
        assert_eq!(msg_reason_tag(Errno::Enoent), "no such file");
        assert_eq!(msg_dollar_q(127).as_str(), "127\n");
        assert_eq!(msg_channel(&o, false).as_str(), VERDICT_127_LIT);
        assert_eq!(
            message(&c.shape, &o).as_str(),
            "nosuchtool stage=search errno=ENOENT exit=127 note=verdict-127 origin=shell-notfound"
        );
        assert_eq!(message_tail(&c.shape).as_str(), "sh/bare dirs=4/4");
    }

    #[test]
    fn t52_the_mimic_line_differs_from_the_verdict_line() {
        let c = match case_at(7) {
            Some(x) => x,
            None => return,
        };
        let o = c.actual();
        assert_eq!(o.value, EXIT_NOT_FOUND);
        assert!(!o.default);
        assert_eq!(msg_channel(&o, true).as_str(), MIMIC_127_LIT);
        assert_ne!(VERDICT_127_LIT, MIMIC_127_LIT);

        let v = match case_at(4) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(msg_channel(&v.actual(), false).as_str(), VERDICT_127_LIT);
        assert_eq!(v.actual().value, o.value);
        assert_ne!(v.actual().origin(), o.origin());
    }

    #[test]
    fn t53_detail_carries_the_slot_and_the_fault() {
        let c = match case_at(0) {
            Some(x) => x,
            None => return,
        };
        let text = detail(&c);
        assert!(text.as_str().contains(c.tag()));
        assert!(text.as_str().contains("claim=builtin:0:0"));
        assert!(text.as_str().contains("fault=ok"));
        assert_eq!(detail_tail(&c).as_str(), "[$?] 0\n");

        let bad = match case_at(11) {
            Some(x) => x,
            None => return,
        };
        let t2 = detail(&bad);
        assert!(t2.as_str().contains(bad.tag()));
        assert!(t2.as_str().contains("fault=exit"));
        assert_eq!(detail_tail(&bad).as_str(), "[$?] 127\n");
        assert_ne!(c.shape.seal(), bad.shape.seal());
    }

    #[test]
    fn t54_matrix_splits_total_from_good() {
        let m = Matrix::of();
        assert_eq!(m.slots(), CORPUS);
        assert_eq!(m.goods(), ACCEPT);
        assert_eq!(m.reds(), REJECT);
        assert_eq!(m.verdict_reds(), REJECT);
        assert!(m.covered());
        let run = match m.row(Tier::Run) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(run.total, 4);
        assert_eq!(run.good, 4);
        assert!(run.perfect());
        assert_eq!(run.red(), 0);
        let nf = match m.row(Tier::NotFound) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(nf.total, 8);
        assert_eq!(nf.good, 2);
        assert_eq!(nf.red(), 6);
        assert!(!nf.perfect());
        assert_eq!(m.perfect_rows(), 3);
        assert_eq!(m.render().as_str(), MATRIX_LIT);
        assert!(!Matrix::blank().covered());
        assert_eq!(Matrix::blank().slots(), 0);
        assert_eq!(Row::blank().red(), 0);
    }

    #[test]
    fn t55_tier_table_is_exclusive() {
        let table = [Tier::Run, Tier::NotFound, Tier::NotExec, Tier::Foreign];
        assert_eq!(table.len(), TIER_COUNT);
        let mut i = 0usize;
        while i < table.len() {
            assert_eq!(table[i].index(), i);
            assert_eq!(table[i].success(), i == 0);
            assert_eq!(table[i].shell_face(), i == 1 || i == 2);
            i += 1;
        }
        let mut j = 0usize;
        while j < CORPUS {
            let c = match case_at(j) {
                Some(x) => x,
                None => return,
            };
            let t = Tier::of(&c.actual());
            assert_eq!(t, WANT_TIER[j]);
            j += 1;
        }
    }

    #[test]
    fn t56_report_renders_the_pinned_green_line() {
        let rep = match run() {
            Ok(x) => x,
            Err(_) => return,
        };
        assert!(rep.green());
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.total, CORPUS as u32);
        assert_eq!(rep.accept, ACCEPT as u32);
        assert_eq!(rep.reject, REJECT as u32);
        assert_eq!(rep.run + rep.notfound + rep.notexec + rep.foreign, rep.accept);
        assert_eq!(rep.matched, rep.total);
        assert_eq!(rep.rate(), 1000);
        assert_eq!(rep.gate(), Ok(()));
        assert_eq!(rep.success_cells(), 4);
        assert_eq!(rep.verdict_cells(), 3);
        assert_eq!(rep.verdict_share(), 37);
    }

    #[test]
    fn t57_zero_corpus_report_is_never_green() {
        let e = Report {
            total: 0,
            accept: 0,
            reject: 0,
            run: 0,
            notfound: 0,
            notexec: 0,
            foreign: 0,
            matched: 0,
        };
        assert_eq!(e.rate(), 0);
        assert!(!e.green());
        assert_eq!(e.gate(), Err(EX_ECOUNT));
        assert_eq!(e.verdict_share(), 0);
        assert_ne!(e.render().as_str(), RENDER_OK);
    }

    #[test]
    fn t58_injection_costs_the_numerator() {
        let base = match run() {
            Ok(x) => x,
            Err(_) => return,
        };
        let bad = match run_with(Some((2usize, Fault::Value))) {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_eq!(bad.matched + 1, base.matched);
        assert!(!bad.green());
        assert!(bad.rate() < 1000);
        assert_eq!(bad.gate(), Err(EX_ECOUNT));
        assert_eq!(bad.accept + bad.reject, base.accept + base.reject);
        assert_eq!(red_rate(), 929);
        assert_eq!(run_with(Some((CORPUS, Fault::Ok))).map(|_| ()), Err(EX_ESLOT));
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS + 1), Err(EX_ENOBS));
    }

    #[test]
    fn t59_gate_only_walks_forward() {
        let mut g = Gate::new();
        assert_eq!(g.last(), -1);
        assert_eq!(g.steps(), 0);
        assert!(!g.closed());
        assert_eq!(g.step(Stage::Word), Ok(()));
        assert_eq!(g.step(Stage::Search), Ok(()));
        assert_eq!(g.step(Stage::Hash), Err(EX_ESTAGE));
        assert_eq!(g.steps(), 2);
        assert_eq!(g.last(), 3);
        let mut i = 4usize;
        while i < STAGE_COUNT {
            assert_eq!(g.step(Stage::from_order(i)), Ok(()));
            i += 1;
        }
        assert_eq!(g.steps(), STAGE_COUNT as u32 - 2);
        assert_eq!(g.close(), Ok(STAGE_COUNT as u32 - 2));
        assert!(g.closed());
        assert_eq!(g.step(Stage::Commit), Err(EX_ESTATE));
        assert_eq!(g.close(), Err(EX_ESTATE));
    }

    #[test]
    fn t60_ledger_registers_every_cell() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.rejects(), 0);
        assert!(!l.is_frozen());
        assert_eq!(l.root(), 0);
        assert!(l.sample(SAMPLES).is_none());
        let first = match l.sample(0) {
            Some(x) => x,
            None => return,
        };
        assert!(first.ok);
        assert_eq!(first.value, VALS[0]);
        assert_eq!(first.op, OP_CORPUS);
        let last = match l.sample(SAMPLES - 1) {
            Some(x) => x,
            None => return,
        };
        assert!(!last.ok);
        assert_eq!(last.code, REJECT_CODES[REJECT - 1]);
        assert_eq!(last.value, 0);
        assert!(!l.render().as_str().is_empty());
        assert_eq!(newline_count(l.render().as_str()), SAMPLES);
    }

    #[test]
    fn t61_ledger_refuses_out_of_order_slots() {
        let mut l = Ledger::new();
        let early = Sample { slot: 1, op: OP_WORD, ok: true, code: 0, value: 2 };
        assert_eq!(l.register(early), Err(EX_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 0);
        assert!(l.is_empty());
        let first = Sample { slot: 0, op: OP_WORD, ok: true, code: 0, value: 2 };
        assert_eq!(l.register(first), Ok(()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.ok_count(), 1);
        assert_eq!(l.audit(), Err(EX_EBAL));
        assert!(first.text().as_str().contains("slot 0"));
        assert_eq!(first.seal(), first.seal());
        assert_ne!(first.seal(), Sample::blank().seal());
    }

    #[test]
    fn t62_ledger_freezes_then_refuses_every_write() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(EX_EBAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.audit(), Ok(()));
        let extra = Sample { slot: SAMPLES as u8, op: OP_REPLAY, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(extra), Err(EX_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.len(), SAMPLES);

        let fresh = Ledger::new();
        assert!(fresh.audit().is_err());
        assert!(fresh.replay().is_err());
    }

    #[test]
    fn t63_tampered_ledger_is_caught_cell_by_cell() {
        let mut l = Ledger::new();
        let mut i = 0usize;
        while i < SAMPLES {
            let good = i < ACCEPT;
            let cell = if good {
                Sample { slot: i as u8, op: OP_CORPUS, ok: true, code: 0, value: 1 }
            } else {
                Sample { slot: i as u8, op: OP_ERRNO, ok: false, code: REJECT_CODES[0], value: 0 }
            };
            assert_eq!(l.register(cell), Ok(()));
            i += 1;
        }
        assert_eq!(l.audit(), Err(EX_EVALUE));
        assert_eq!(l.len(), SAMPLES);
    }

    #[test]
    fn t64_four_injections_are_all_green() {
        let four = inject_four();
        assert_eq!(four.len(), 4);
        assert!(four_true(four));
        assert!(four[0]);
        assert!(four[1]);
        assert!(four[2]);
        assert!(four[3]);
        assert!(name_ok());
        assert!(codes_ok());
        assert!(stages_ok());
        assert!(lookup_ok());
        assert!(define_ok());
        assert!(boundary_ok());
        assert!(origin_ok());
        assert!(chan_ok());
        assert!(judge_ok());
        assert!(corpus_ok());
        assert!(slots_ok());
        assert!(matrix_ok());
        assert!(reds_ok());
        assert!(message_ok());
        assert!(detail_ok());
        assert!(gate_ok());
        assert!(ledger_ok());
    }

    #[test]
    fn t65_probe_shape_is_the_blank_field() {
        let p = probe("unit-probe", "ls", Lookup::PathHit);
        assert_eq!(p.tag, "unit-probe");
        assert_eq!(p.word, "ls");
        assert_eq!(p.lookup, Lookup::PathHit);
        assert_eq!(p.form(), WordForm::Bare);
        assert!(p.should_walk_path());
        assert_eq!(p.dirs_scanned, p.path_dirs);
        let mut slashed = probe("unit-slash", "/bin/ls", Lookup::SlashTarget);
        assert!(!slashed.should_walk_path());
        slashed.path_consulted = false;
        assert_eq!(slashed.form(), WordForm::Slashed);
        assert_ne!(p.seal(), slashed.seal());
    }

    #[test]
    fn t66_all_thirteen_checks_are_green() {
        let s = run_unx11664_checks();
        assert_eq!(s.domain, "unx11664_bb_notfound_exit");
        assert_eq!(s.len(), 13);
        assert!(!s.is_empty());
        assert!(s.all_passed());
        assert_eq!(s.tally(), (13, 0));
        assert!(!s.truncated());
        let last = match s.get(12) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(last.name, "EX13-冻结重放四注入");
        assert!(last.passed);
        let mut i = 0usize;
        while i < s.len() {
            let c = match s.get(i) {
                Some(x) => x,
                None => return,
            };
            assert!(c.passed);
            assert!(c.name.starts_with("EX"));
            assert!(!c.detail.is_empty());
            i += 1;
        }
    }
}
